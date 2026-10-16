use super::*;
use crate::job_runs::{
    scan,
    tests::{bind_fixture, empty, fixture, listing, payload, replay},
};
use aws_smithy_types::body::SdkBody;
use std::{
    fs,
    io::{Seek, SeekFrom},
    time::Duration,
};
use tauri::http::Response;

fn missing() -> Response<SdkBody> {
    Response::builder()
        .status(404)
        .body(SdkBody::from("<Error><Code>NoSuchKey</Code></Error>"))
        .unwrap()
}
fn head(size: u64, etag: &str) -> Response<SdkBody> {
    Response::builder()
        .status(200)
        .header("content-length", size)
        .header("etag", etag)
        .body(SdkBody::empty())
        .unwrap()
}
fn request(status: &RunStatus, rows: &[super::super::collect::Row], policy: Policy) -> Request {
    Request {
        run_id: status.run_id.clone(),
        manifest_id: status.manifest_id.clone().unwrap(),
        entry_ids: rows.iter().map(|row| row.id.clone()).collect(),
        policy,
        max_snapshot_bytes: 1024 * 1024,
        approved: true,
    }
}
async fn compared(
    local: bool,
    sse: bool,
    tail: Vec<Response<SdkBody>>,
) -> (
    tempfile::TempDir,
    crate::jobs::Paths,
    JobRunState,
    RunStatus,
    Vec<super::super::collect::Row>,
    aws_smithy_http_client::test_util::StaticReplayClient,
) {
    let (root, paths, job) = fixture(local, sse);
    if let SourcePin::Local { path, .. } = &job.source {
        fs::write(Path::new(path).join("file"), "abc").unwrap();
    }
    let mut responses = vec![empty()];
    if !local {
        responses.extend([
            empty(),
            listing("src//", Some(("file", 3, "source-etag"))),
            payload("abc", "source-etag"),
        ]);
    }
    responses.push(listing("dest//", None));
    responses.extend(tail);
    let http = replay(responses);
    let state = JobRunState::default();
    let bound = bind_fixture(&state, &paths, &job, &http).await;
    let status = scan(&state, &bound.run_id, Arc::new(|_| {})).await.unwrap();
    assert_eq!(status.phase, "complete");
    let rows = state
        .page(&status.run_id, status.manifest_id.as_deref().unwrap(), 0)
        .unwrap();
    (root, paths, state, status, rows, http)
}

#[tokio::test]
async fn native_local_preparation_retains_private_checked_bytes_and_capped_lease_without_writes() {
    let (_root, paths, state, status, rows, http) = compared(true, false, vec![missing()]).await;
    let expires = state.0.lock().unwrap().slot.as_ref().unwrap().expires;
    let result = prepare(
        &state,
        request(&status, &rows, Policy::CreateOnly),
        Arc::new(|_| {}),
    )
    .await
    .unwrap();
    assert_eq!(result.phase, "prepared", "{:?}", result.message);
    assert!(!result.execution_available);
    let preview = result.preview.unwrap();
    assert_eq!(preview.bytes, 3);
    assert_eq!(preview.entries[0].destination_key, "dest//file");
    assert!(preview.verification_required);
    assert!(!preview.execution_available);
    assert!(preview.expires_seconds <= status.expires_seconds);
    assert_eq!(preview.manifest_id, status.manifest_id.clone().unwrap());
    {
        let inner = state.0.lock().unwrap();
        let slot = inner.slot.as_ref().unwrap();
        assert_eq!(slot.expires, expires);
        let plan = slot.preview.as_ref().unwrap();
        let snapshot = &plan.items[0].snapshot;
        assert_eq!(snapshot.file.metadata().unwrap().mode() & 0o777, 0o600);
        assert_eq!(snapshot.file.metadata().unwrap().nlink(), 0);
        let mut file = snapshot.file.try_clone().unwrap();
        file.seek(SeekFrom::Start(0)).unwrap();
        let mut bytes = vec![];
        file.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"abc");
        let SourcePin::Local { path, .. } = &slot.binding.as_ref().unwrap().job.source else {
            panic!()
        };
        fs::write(Path::new(path).join("file"), "changed after snapshot").unwrap();
        file.seek(SeekFrom::Start(0)).unwrap();
        bytes.clear();
        file.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"abc");
        assert!(plan
            .check_owner(
                slot.binding.as_ref().unwrap(),
                "different-run",
                status.manifest_id.as_deref().unwrap(),
                expires
            )
            .is_err());
        assert!(plan
            .check_owner(
                slot.binding.as_ref().unwrap(),
                &status.run_id,
                "different-manifest",
                expires
            )
            .is_err());
    }
    assert!(prepare(
        &state,
        request(&status, &rows, Policy::CreateOnly),
        Arc::new(|_| {})
    )
    .await
    .is_err());
    assert_eq!(
        state
            .page(&status.run_id, status.manifest_id.as_deref().unwrap(), 0)
            .unwrap()
            .len(),
        1
    );
    let json = serde_json::to_string(&state.status(&status.run_id).unwrap()).unwrap();
    assert!(!json.contains("sha256\":"));
    assert!(!json.contains("source-etag"));
    assert!(!json.contains("FAKE_SECRET"));
    assert!(http
        .actual_requests()
        .all(|request| matches!(request.method(), "GET" | "HEAD")));
    assert_eq!(http.actual_requests().count(), 3);
    state.release(&status.run_id).unwrap();
    assert!(state.status(&status.run_id).unwrap().preview.is_none());
    let released = super::super::run_lock(&paths);
    assert!(
        released.is_ok(),
        "{}",
        released
            .err()
            .map(|failure| failure.to_string())
            .unwrap_or_default()
    );
}

#[tokio::test]
async fn synthetic_duplicate_unapproved_oversized_and_cross_manifest_selections_do_no_network_io() {
    let (_root, _paths, state, status, rows, http) = compared(true, false, vec![]).await;
    for invalid in 0..6 {
        let mut input = request(&status, &rows, Policy::CreateOnly);
        match invalid {
            0 => input.approved = false,
            1 => input.entry_ids.push(input.entry_ids[0].clone()),
            2 => input.entry_ids[0] = fresh_id(),
            3 => input.max_snapshot_bytes = 2,
            4 => input.manifest_id = fresh_id(),
            _ => input.run_id = fresh_id(),
        }
        assert!(prepare(&state, input, Arc::new(|_| {})).await.is_err());
        assert_eq!(http.actual_requests().count(), 2);
        assert_eq!(state.status(&status.run_id).unwrap().phase, "complete");
    }
    let mut raw=serde_json::to_value(serde_json::json!({"run_id":status.run_id,"manifest_id":status.manifest_id,"entry_ids":[],"policy":"overwrite","max_snapshot_bytes":100,"approved":true})).unwrap();
    assert!(serde_json::from_value::<Request>(raw.clone()).is_err());
    raw["policy"] = "create_only".into();
    raw["expected_sha256"] = "invented".into();
    assert!(serde_json::from_value::<Request>(raw).is_err());
}

#[tokio::test]
async fn source_changes_and_new_destination_conflicts_cannot_publish_snapshots() {
    for change_source in [true, false] {
        let (_root, _paths, state, status, rows, http) = compared(
            true,
            false,
            vec![if change_source {
                missing()
            } else {
                head(3, "created")
            }],
        )
        .await;
        if change_source {
            let inner = state.0.lock().unwrap();
            let SourcePin::Local { path, .. } = &inner
                .slot
                .as_ref()
                .unwrap()
                .binding
                .as_ref()
                .unwrap()
                .job
                .source
            else {
                panic!()
            };
            fs::write(Path::new(path).join("file"), "abd").unwrap();
        }
        let result = prepare(
            &state,
            request(&status, &rows, Policy::CreateOnly),
            Arc::new(|_| {}),
        )
        .await
        .unwrap();
        assert_eq!(result.phase, "stopped");
        assert!(matches!(result.outcome, Some(ScanOutcome::Failed)));
        assert!(result.preview.is_none());
        assert!(result.manifest_id.is_none());
        assert_eq!(http.actual_requests().count(), 3);
    }
}

#[tokio::test]
async fn independent_sse_s3_snapshot_gets_keep_opaque_conditions_and_actual_content_hashes() {
    let (_root, _paths, state, status, rows, http) =
        compared(false, true, vec![missing(), payload("abc", "source-etag")]).await;
    let result = prepare(
        &state,
        request(&status, &rows, Policy::CreateOnly),
        Arc::new(|_| {}),
    )
    .await
    .unwrap();
    assert_eq!(result.phase, "prepared");
    let preview = result.preview.unwrap();
    assert!(preview.relay);
    assert!(!preview.can_review_execution);
    assert!(preview.execution_blocker.unwrap().contains("S3 source ACL"));
    let requests: Vec<_> = http.actual_requests().collect();
    assert_eq!(requests.len(), 7);
    assert_eq!(
        requests[6].headers().get("if-match").unwrap(),
        "source-etag"
    );
    assert!(requests[6]
        .headers()
        .get("x-amz-server-side-encryption-customer-key")
        .is_some());
    let inner = state.0.lock().unwrap();
    let plan = inner.slot.as_ref().unwrap().preview.as_ref().unwrap();
    assert_ne!(plan.items[0].snapshot.sha256, "source-etag");
    assert_eq!(plan.items[0].snapshot.size, 3);
    assert_eq!(
        plan.items[0].snapshot.file.metadata().unwrap().mode() & 0o777,
        0o600
    );
    assert_eq!(plan.items[0].snapshot.file.metadata().unwrap().nlink(), 0);
}

#[tokio::test]
async fn changed_s3_source_digest_and_incomplete_body_never_create_a_usable_preview() {
    for response in [payload("abd", "source-etag"), head(3, "source-etag")] {
        let (_root, _paths, state, status, rows, _http) =
            compared(false, false, vec![missing(), response]).await;
        let result = prepare(
            &state,
            request(&status, &rows, Policy::CreateOnly),
            Arc::new(|_| {}),
        )
        .await
        .unwrap();
        assert_eq!(result.phase, "stopped");
        assert!(result.preview.is_none());
    }
}

#[tokio::test]
async fn destination_read_denials_do_not_count_as_absence() {
    for status_code in [403, 404] {
        let response = Response::builder()
            .status(status_code)
            .body(SdkBody::from("<Error><Code>AccessDenied</Code></Error>"))
            .unwrap();
        let (_root, _paths, state, status, rows, _http) =
            compared(true, false, vec![response]).await;
        let result = prepare(
            &state,
            request(&status, &rows, Policy::CreateOnly),
            Arc::new(|_| {}),
        )
        .await
        .unwrap();
        assert_eq!(result.phase, "stopped");
        assert!(result.preview.is_none());
        assert!(result.message.unwrap().contains("AccessDenied"));
    }
}

#[tokio::test]
async fn cancellation_expiry_and_identity_drift_discard_prepared_payloads() {
    for kind in 0..3 {
        let (_root, paths, state, status, rows, _http) =
            compared(true, false, vec![missing()]).await;
        let result = prepare(
            &state,
            request(&status, &rows, Policy::CreateOnly),
            Arc::new(|_| {}),
        )
        .await
        .unwrap();
        assert_eq!(result.phase, "prepared");
        match kind {
            0 => state.release(&status.run_id).unwrap(),
            1 => {
                state.0.lock().unwrap().slot.as_mut().unwrap().expires =
                    Instant::now() - Duration::from_secs(1)
            }
            _ => crate::jobs::secret_edit(&paths, "destination", || Ok(())).unwrap(),
        }
        let stopped = state.status(&status.run_id).unwrap();
        assert_eq!(stopped.phase, "stopped");
        assert!(stopped.preview.is_none());
        assert!(stopped.manifest_id.is_none());
    }
}

#[tokio::test]
async fn replacements_require_compared_destination_etags_and_real_sha256_not_size_only() {
    for changed in [false, true] {
        let (_root, paths, job) = fixture(true, false);
        let SourcePin::Local { path, .. } = &job.source else {
            panic!()
        };
        fs::write(Path::new(path).join("file"), "abc").unwrap();
        let http = replay(vec![
            empty(),
            listing("dest//", Some(("file", 3, "dest-etag"))),
            payload("abd", "dest-etag"),
            head(3, "dest-etag"),
            payload(if changed { "bad" } else { "abd" }, "dest-etag"),
        ]);
        let state = JobRunState::default();
        let bound = bind_fixture(&state, &paths, &job, &http).await;
        let status = scan(&state, &bound.run_id, Arc::new(|_| {})).await.unwrap();
        let rows = state
            .page(&status.run_id, status.manifest_id.as_deref().unwrap(), 0)
            .unwrap();
        assert_eq!(rows[0].status, "modified");
        assert!(prepare(
            &state,
            request(&status, &rows, Policy::CreateOnly),
            Arc::new(|_| {})
        )
        .await
        .is_err());
        assert_eq!(http.actual_requests().count(), 3);
        assert!(prepare(
            &state,
            request(&status, &rows, Policy::SkipExisting),
            Arc::new(|_| {})
        )
        .await
        .is_err());
        assert_eq!(http.actual_requests().count(), 3);
        let result = prepare(
            &state,
            request(&status, &rows, Policy::ReplaceUnchanged),
            Arc::new(|_| {}),
        )
        .await
        .unwrap();
        assert_eq!(result.phase, if changed { "stopped" } else { "prepared" });
        assert_eq!(result.preview.is_some(), !changed);
        let requests: Vec<_> = http.actual_requests().collect();
        assert_eq!(requests.len(), 5);
        assert_eq!(requests[4].headers().get("if-match").unwrap(), "dest-etag");
        if !changed {
            let inner = state.0.lock().unwrap();
            assert_eq!(
                inner.slot.as_ref().unwrap().preview.as_ref().unwrap().items[0].expected,
                DestinationExpectation::Match {
                    etag: "dest-etag".into()
                }
            );
        }
    }
}

#[tokio::test]
async fn symlink_substitution_garage_and_cancelled_preparation_never_publish_payloads() {
    for kind in 0..4 {
        let (_root, _paths, state, status, rows, http) = compared(
            true,
            false,
            if kind == 1 { vec![] } else { vec![missing()] },
        )
        .await;
        if kind == 0 {
            let inner = state.0.lock().unwrap();
            let SourcePin::Local { path, .. } = &inner
                .slot
                .as_ref()
                .unwrap()
                .binding
                .as_ref()
                .unwrap()
                .job
                .source
            else {
                panic!()
            };
            fs::remove_file(Path::new(path).join("file")).unwrap();
            std::os::unix::fs::symlink("outside", Path::new(path).join("file")).unwrap();
        }
        if kind == 1 {
            let mut inner = state.0.lock().unwrap();
            let binding =
                Arc::get_mut(inner.slot.as_mut().unwrap().binding.as_mut().unwrap()).unwrap();
            binding.destination.client = crate::s3::compat::configure_conditions(
                binding.destination.client.clone(),
                Some("garage"),
                None,
            );
        }
        let progress_state = state.clone();
        let id = status.run_id.clone();
        let result = prepare(
            &state,
            request(&status, &rows, Policy::CreateOnly),
            Arc::new(move |_| {
                if kind == 2 {
                    progress_state.release(&id).unwrap();
                }
                if kind == 3 {
                    progress_state
                        .0
                        .lock()
                        .unwrap()
                        .slot
                        .as_mut()
                        .unwrap()
                        .expires = Instant::now() - Duration::from_secs(1);
                }
            }),
        )
        .await;
        if kind == 1 {
            assert!(result.is_err());
            assert_eq!(http.actual_requests().count(), 2);
        } else {
            if let Ok(result) = result {
                assert_eq!(result.phase, "stopped");
                assert!(result.preview.is_none());
            }
            assert!(state.status(&status.run_id).unwrap().preview.is_none());
        }
        assert!(http
            .actual_requests()
            .all(|request| matches!(request.method(), "GET" | "HEAD")));
    }
}

#[tokio::test]
async fn destination_only_rows_and_excessive_snapshot_capabilities_fail_before_reads() {
    let (_root, paths, job) = fixture(true, false);
    let http = replay(vec![
        empty(),
        listing("dest//", Some(("only", 3, "etag"))),
        payload("abc", "etag"),
    ]);
    let state = JobRunState::default();
    let bound = bind_fixture(&state, &paths, &job, &http).await;
    let status = scan(&state, &bound.run_id, Arc::new(|_| {})).await.unwrap();
    let rows = state
        .page(&status.run_id, status.manifest_id.as_deref().unwrap(), 0)
        .unwrap();
    assert_eq!(rows[0].status, "destination_only");
    assert!(prepare(
        &state,
        request(&status, &rows, Policy::ReplaceUnchanged),
        Arc::new(|_| {})
    )
    .await
    .is_err());
    assert_eq!(http.actual_requests().count(), 3);
    let (_root, _paths, state, status, _rows, http) = compared(true, false, vec![]).await;
    let rows = {
        let mut inner = state.0.lock().unwrap();
        let manifest = inner.slot.as_mut().unwrap().manifest.as_mut().unwrap();
        let template = manifest.entries[0].clone();
        manifest.entries.clear();
        for index in 0..129 {
            let mut entry = template.clone();
            entry.row.id = fresh_id();
            entry.row.relative_path = format!("file-{index}");
            manifest.entries.push(entry);
        }
        manifest
            .entries
            .iter()
            .map(|entry| entry.row.clone())
            .collect::<Vec<_>>()
    };
    let failure = prepare(
        &state,
        request(&status, &rows, Policy::CreateOnly),
        Arc::new(|_| {}),
    )
    .await
    .err()
    .unwrap();
    assert!(failure.to_string().contains("128"));
    assert_eq!(http.actual_requests().count(), 2);
}

#[tokio::test]
async fn skip_existing_reports_skips_without_snapshotting_or_rechecking_skipped_files() {
    let (_root, paths, job) = fixture(true, false);
    let SourcePin::Local { path, .. } = &job.source else {
        panic!()
    };
    fs::write(Path::new(path).join("new"), "abc").unwrap();
    fs::write(Path::new(path).join("old"), "abc").unwrap();
    let http = replay(vec![
        empty(),
        listing("dest//", Some(("old", 3, "old-etag"))),
        payload("abd", "old-etag"),
        missing(),
    ]);
    let state = JobRunState::default();
    let bound = bind_fixture(&state, &paths, &job, &http).await;
    let status = scan(&state, &bound.run_id, Arc::new(|_| {})).await.unwrap();
    let rows = state
        .page(&status.run_id, status.manifest_id.as_deref().unwrap(), 0)
        .unwrap();
    let result = prepare(
        &state,
        request(&status, &rows, Policy::SkipExisting),
        Arc::new(|_| {}),
    )
    .await
    .unwrap();
    assert_eq!(result.phase, "prepared");
    let preview = result.preview.unwrap();
    assert_eq!(preview.files, 1);
    assert_eq!(preview.skipped, 1);
    assert_eq!(preview.bytes, 3);
    assert_eq!(preview.entries[0].relative_path, "new");
    assert_eq!(http.actual_requests().count(), 4);
}

#[tokio::test]
async fn prepared_client_instance_tampering_invalidates_native_ownership() {
    let (_root, _paths, state, status, rows, _http) = compared(true, false, vec![missing()]).await;
    prepare(
        &state,
        request(&status, &rows, Policy::CreateOnly),
        Arc::new(|_| {}),
    )
    .await
    .unwrap();
    {
        let mut inner = state.0.lock().unwrap();
        Arc::get_mut(inner.slot.as_mut().unwrap().binding.as_mut().unwrap())
            .unwrap()
            .destination
            .instance = fresh_id();
    }
    let result = state.status(&status.run_id).unwrap();
    assert_eq!(result.phase, "stopped");
    assert!(result.preview.is_none());
    assert!(result.message.unwrap().contains("ownership"));
}

#[tokio::test]
async fn cancellation_during_local_snapshot_keeps_run_lease_until_disk_worker_stops() {
    let (_root, paths, job) = fixture(true, false);
    let SourcePin::Local { path, .. } = &job.source else {
        panic!()
    };
    fs::write(Path::new(path).join("large"), vec![1u8; 3 * 1024 * 1024]).unwrap();
    let http = replay(vec![empty(), listing("dest//", None), missing()]);
    let state = JobRunState::default();
    let bound = bind_fixture(&state, &paths, &job, &http).await;
    let status = scan(&state, &bound.run_id, Arc::new(|_| {})).await.unwrap();
    let rows = state
        .page(&status.run_id, status.manifest_id.as_deref().unwrap(), 0)
        .unwrap();
    let mut input = request(&status, &rows, Policy::CreateOnly);
    input.max_snapshot_bytes = 4 * 1024 * 1024;
    let notify_state = state.clone();
    let notify_paths = paths.clone();
    let id = status.run_id.clone();
    let cancelled = Arc::new(AtomicBool::new(false));
    let did_cancel = cancelled.clone();
    let result = prepare(
        &state,
        input,
        Arc::new(move |event| {
            if event.bytes_read >= 1024 * 1024 && !did_cancel.swap(true, Ordering::Relaxed) {
                notify_state.release(&id).unwrap();
                assert!(super::super::run_lock(&notify_paths).is_err());
            }
        }),
    )
    .await
    .unwrap();
    assert!(cancelled.load(Ordering::Relaxed));
    assert_eq!(result.phase, "stopped");
    assert!(matches!(result.outcome, Some(ScanOutcome::Cancelled)));
    assert!(result.preview.is_none());
    assert!(super::super::run_lock(&paths).is_ok());
}

#[test]
fn final_run_lease_drop_unlocks_even_if_a_spawned_child_retains_a_duplicate_descriptor() {
    let (_root, paths, _job) = fixture(true, false);
    let lease = Arc::new(super::super::run_lock(&paths).unwrap());
    let worker = lease.clone();
    let inherited = lease.0.try_clone().unwrap();
    drop(lease);
    assert!(super::super::run_lock(&paths).is_err());
    drop(worker);
    assert!(super::super::run_lock(&paths).is_ok());
    drop(inherited);
}
