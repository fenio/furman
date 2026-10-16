use super::s3_tests::{acl, object, tags, unlocked};
use super::*;
use crate::job_runs::{
    scan,
    tests::{bind_fixture, empty, fixture_with_names, listing, replay},
};
use aws_smithy_http_client::test_util::StaticReplayClient;
use aws_smithy_types::body::SdkBody;
use std::{fs, path::Path, time::Duration};
use tauri::http::Response;

const SIZE: usize = 8 * 1024 * 1024 + 19;
fn bytes() -> Vec<u8> {
    (0..SIZE).map(|i| (i % 251) as u8).collect()
}
fn absent() -> Response<SdkBody> {
    failure(404, "NoSuchKey")
}
fn failure(status: u16, code: &str) -> Response<SdkBody> {
    Response::builder()
        .status(status)
        .body(SdkBody::from(format!("<Error><Code>{code}</Code></Error>")))
        .unwrap()
}
fn source(sse: &str) -> Response<SdkBody> {
    let mut response = object(Some("source-version"), sse);
    response
        .headers_mut()
        .insert("content-length", SIZE.to_string().parse().unwrap());
    *response.body_mut() = SdkBody::from(bytes());
    response
}
fn old(sse: &str) -> Response<SdkBody> {
    let mut response = object(Some("old-version"), sse);
    response
        .headers_mut()
        .insert("etag", "old-etag".parse().unwrap());
    *response.body_mut() = SdkBody::from("old");
    response
}
fn old_reads(sse: &str) -> Vec<Response<SdkBody>> {
    vec![
        old(sse),
        unlocked(),
        acl("old-owner"),
        tags(Some("old-version"), "native-only-tag-value"),
    ]
}
fn source_reads(sse: &str) -> Vec<Response<SdkBody>> {
    vec![
        source(sse),
        unlocked(),
        acl("source-owner"),
        tags(Some("source-version"), "native-only-tag-value"),
    ]
}
fn rechecks(local: bool, replace: bool, sse: &str) -> Vec<Response<SdkBody>> {
    let mut responses = if replace {
        old_reads(sse)
    } else {
        vec![absent()]
    };
    if !local {
        responses.extend(source_reads(sse));
    }
    responses
}
fn initiated() -> Response<SdkBody> {
    Response::builder().status(200).body(SdkBody::from("<InitiateMultipartUploadResult><Bucket>destination-bucket</Bucket><Key>dest/file</Key><UploadId>private-upload/+id</UploadId></InitiateMultipartUploadResult>")).unwrap()
}
fn part() -> Response<SdkBody> {
    Response::builder()
        .status(200)
        .header("etag", "part-etag")
        .body(SdkBody::empty())
        .unwrap()
}
fn completed() -> Response<SdkBody> {
    Response::builder().status(200).header("x-amz-version-id","written-version").body(SdkBody::from("<CompleteMultipartUploadResult><Bucket>destination-bucket</Bucket><Key>dest/file</Key><ETag>written-etag</ETag></CompleteMultipartUploadResult>")).unwrap()
}
fn written(local: bool, replace: bool, sse: &str) -> Response<SdkBody> {
    let mut response = if !local || replace {
        source(sse)
    } else {
        Response::builder()
            .status(200)
            .header("content-length", SIZE)
            .header("content-type", "application/octet-stream")
            .body(SdkBody::from(bytes()))
            .unwrap()
    };
    response
        .headers_mut()
        .insert("etag", "written-etag".parse().unwrap());
    response
        .headers_mut()
        .insert("x-amz-version-id", "written-version".parse().unwrap());
    if sse == "sse_c" {
        response.headers_mut().insert(
            "x-amz-server-side-encryption-customer-algorithm",
            "AES256".parse().unwrap(),
        );
    }
    response
}
fn execution(local: bool, replace: bool, sse: &str) -> Vec<Response<SdkBody>> {
    let mut responses = rechecks(local, replace, sse);
    responses.extend([initiated(), part(), part()]);
    responses.extend(rechecks(local, replace, sse));
    responses.extend([completed(), written(local, replace, sse)]);
    if !local || replace {
        responses.extend([
            acl("destination-owner"),
            tags(Some("written-version"), "native-only-tag-value"),
        ]);
    }
    responses
}
async fn prepared(
    local: bool,
    replace: bool,
    sse: &str,
    tail: Vec<Response<SdkBody>>,
) -> (
    tempfile::TempDir,
    crate::jobs::Paths,
    JobRunState,
    RunStatus,
    StaticReplayClient,
) {
    let (root, paths, job) = fixture_with_names(
        local,
        sse == "sse_c",
        "https://job.test",
        "source-bucket",
        "destination-bucket",
        None,
        "src/",
        "dest/",
    );
    if let SourcePin::Local { path, .. } = &job.source {
        fs::write(Path::new(path).join("file"), bytes()).unwrap();
    }
    let mut responses = vec![empty()];
    if !local {
        responses.extend([
            empty(),
            listing("src/", Some(("file", SIZE as u64, "source-etag"))),
            source(sse),
        ]);
    }
    responses.push(listing(
        "dest/",
        if replace {
            Some(("file", 3, "old-etag"))
        } else {
            None
        },
    ));
    if replace {
        responses.push(old(sse));
        responses.push(
            Response::builder()
                .status(200)
                .header("etag", "old-etag")
                .header("content-length", 3)
                .body(SdkBody::empty())
                .unwrap(),
        );
        responses.push(old(sse));
    } else {
        responses.push(absent());
    }
    if !local {
        responses.push(source(sse));
    }
    responses.extend(tail);
    let http = replay(responses);
    let state = JobRunState::default();
    let bound = bind_fixture(&state, &paths, &job, &http).await;
    let compared = scan(&state, &bound.run_id, Arc::new(|_| {})).await.unwrap();
    let rows = state
        .page(&bound.run_id, compared.manifest_id.as_deref().unwrap(), 0)
        .unwrap();
    let prepared = preview::prepare(
        &state,
        preview::Request {
            run_id: bound.run_id,
            manifest_id: compared.manifest_id.unwrap(),
            entry_ids: rows.iter().map(|r| r.id.clone()).collect(),
            policy: if replace {
                preview::Policy::ReplaceUnchanged
            } else {
                preview::Policy::CreateOnly
            },
            max_snapshot_bytes: SIZE as u64,
            approved: true,
        },
        Arc::new(|_| {}),
    )
    .await
    .unwrap();
    assert_eq!(prepared.phase, "prepared", "{:?}", prepared.message);
    assert!(prepared.preview.as_ref().unwrap().can_review_execution);
    (root, paths, state, prepared, http)
}
async fn reviewed(state: &JobRunState, status: &RunStatus) -> RunStatus {
    let status = review(
        state,
        ReviewRequest {
            run_id: status.run_id.clone(),
            preview_id: status.preview.as_ref().unwrap().id.clone(),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        status.phase, "awaiting_execution_approval",
        "{:?}",
        status.message
    );
    status
}
fn request(status: &RunStatus) -> ExecuteRequest {
    ExecuteRequest {
        run_id: status.run_id.clone(),
        approval_id: status
            .execution_review
            .as_ref()
            .unwrap()
            .approval_id
            .clone(),
        approved: true,
        acknowledge_exclusions: true,
        acknowledge_replacements: true,
        acknowledge_multipart: true,
    }
}
async fn wait(state: &JobRunState, id: &str) -> RunStatus {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let status = state.status(id).unwrap();
            if status.phase == "stopped" {
                return status;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn multipart_jobs_require_distinct_ack_and_preserve_local_s3_replacement_and_encryption_policies(
) {
    for (local, replace, sse) in [
        (true, false, ""),
        (true, true, "AES256"),
        (true, false, "sse_c"),
        (false, false, "AES256"),
        (false, true, ""),
        (false, true, "sse_c"),
    ] {
        let mut tail = rechecks(local, replace, sse);
        tail.extend(execution(local, replace, sse));
        let (_root, paths, state, prepared, http) = prepared(local, replace, sse, tail).await;
        let reviewed = reviewed(&state, &prepared).await;
        let disclosure = reviewed.execution_review.as_ref().unwrap();
        assert_eq!(disclosure.multipart_files, 1);
        assert_eq!(disclosure.multipart_parts, 2);
        let mut denied = request(&reviewed);
        denied.acknowledge_multipart = false;
        assert!(start(&state, denied).await.is_err());
        start(&state, request(&reviewed)).await.unwrap();
        assert!(start(&state, request(&reviewed)).await.is_err());
        let result = wait(&state, &reviewed.run_id).await.execution.unwrap();
        assert_eq!(result.status, Status::Succeeded, "{:?}", result.message);
        assert!(result.outcome_recorded);
        assert_eq!(result.counts.multipart_files, 1);
        assert_eq!(result.counts.verified_files, 1);
        assert_eq!(result.counts.verified_bytes, SIZE as u64);
        assert!(state.history(&paths).unwrap().pending_uploads.is_empty());
        let mutations: Vec<_> = http
            .actual_requests()
            .filter(|r| matches!(r.method(), "PUT" | "POST" | "DELETE"))
            .collect();
        assert_eq!(mutations.len(), 4);
        assert!(mutations[0].uri().contains("uploads"));
        let mut uploaded = Vec::new();
        for (number, part) in mutations[1..3].iter().enumerate() {
            assert!(part.uri().contains(&format!("partNumber={}", number + 1)));
            assert!(part.body().bytes().unwrap().len() <= multipart::PART_BYTES as usize);
            uploaded.extend_from_slice(part.body().bytes().unwrap());
            assert!(part.headers().get("if-match").is_none());
            assert_eq!(
                part.headers()
                    .contains_key("x-amz-server-side-encryption-customer-key"),
                sse == "sse_c"
            );
        }
        assert_eq!(uploaded, bytes());
        assert_eq!(
            mutations[3].headers().get("if-match"),
            if replace { Some("old-etag") } else { None }
        );
        assert_eq!(
            mutations[3].headers().get("if-none-match"),
            if replace { None } else { Some("*") }
        );
        assert_eq!(
            mutations[0].headers().get("x-amz-server-side-encryption"),
            if sse == "AES256" {
                Some("AES256")
            } else {
                None
            }
        );
        if !local || replace {
            assert_eq!(
                mutations[0].headers().get("x-amz-meta-preserved"),
                Some("native-only-metadata-value")
            );
            assert!(mutations[0].headers().get("x-amz-tagging").is_some());
        }
        let raw = fs::read_to_string(paths.store.parent().unwrap().join("job-runs/journal.json"))
            .unwrap();
        assert!(!raw.contains("private-upload"));
        assert!(!raw.contains("dest/file"));
    }
}

#[tokio::test]
async fn failed_parts_never_complete_and_known_upload_cleanup_is_bounded_with_persistent_private_evidence(
) {
    for clean in [true, false] {
        let mut tail = rechecks(true, false, "");
        tail.extend(rechecks(true, false, ""));
        tail.extend([initiated(), failure(503, "SlowDown")]);
        if clean {
            tail.push(empty());
        } else {
            for _ in 0..3 {
                tail.push(failure(403, "AccessDenied"));
            }
        }
        let (_root, paths, state, prepared, http) = prepared(true, false, "", tail).await;
        let reviewed = reviewed(&state, &prepared).await;
        start(&state, request(&reviewed)).await.unwrap();
        let result = wait(&state, &reviewed.run_id).await.execution.unwrap();
        assert_eq!(
            result.status,
            Status::Uncertain,
            "{:?}",
            result.message
        );
        assert!(result.outcome_recorded);
        assert_eq!(result.counts.verified_files, 0);
        let requests: Vec<_> = http.actual_requests().collect();
        assert_eq!(requests.iter().filter(|r| r.method() == "PUT").count(), 1);
        assert_eq!(requests.iter().filter(|r| r.method() == "POST").count(), 1);
        assert_eq!(
            requests.iter().filter(|r| r.method() == "DELETE").count(),
            if clean { 1 } else { 3 }
        );
        let report = state.history(&paths).unwrap();
        assert_eq!(report.pending_uploads.len(), 1);
        let serialized = serde_json::to_string(&report).unwrap();
        assert!(!serialized.contains("private-upload"));
        assert!(!serialized.contains("dest/file"));
        let raw = fs::read_to_string(paths.store.parent().unwrap().join("job-runs/journal.json"))
            .unwrap();
        assert!(raw.contains("private-upload"));
        assert!(!report.pauses.is_empty());
        if !clean {
            assert!(result.message.unwrap().contains("storage charges"));
            assert!(state
                .review_history(&paths, &report.revision, None, true, true)
                .is_err());
        }
    }
}

#[tokio::test]
async fn lost_initiation_receipts_remain_unknown_and_restart_does_not_connect_or_restore_authority()
{
    let mut tail = rechecks(true, false, "");
    tail.extend(rechecks(true, false, ""));
    tail.push(failure(503, "SlowDown"));
    let (_root, paths, state, prepared, http) = prepared(true, false, "", tail).await;
    let reviewed = reviewed(&state, &prepared).await;
    start(&state, request(&reviewed)).await.unwrap();
    let result = wait(&state, &reviewed.run_id).await.execution.unwrap();
    assert_eq!(result.status, Status::Uncertain);
    let before = http.actual_requests().count();
    let restarted = JobRunState::default();
    let report = restarted.history(&paths).unwrap();
    assert_eq!(report.pending_uploads.len(), 1);
    assert!(!report.pending_uploads[0].upload_id_known);
    assert!(restarted.0.lock().unwrap().slot.is_none());
    assert_eq!(http.actual_requests().count(), before);
    assert!(result.message.unwrap().contains("unknown ID"));
    let evidence = report.pending_uploads[0].id.clone();
    assert!(restarted
        .review_multipart_cleanup(&paths, &report.revision, &evidence, false)
        .is_err());
    assert!(restarted
        .review_multipart_cleanup(&paths, "stale", &evidence, true)
        .is_err());
    assert!(restarted
        .review_multipart_cleanup(&paths, &report.revision, &fresh_id(), true)
        .is_err());
    let attested = restarted
        .review_multipart_cleanup(&paths, &report.revision, &evidence, true)
        .unwrap();
    assert!(attested.pending_uploads.is_empty());
    assert_eq!(attested.records[0].multipart_cleanup_reviews, 1);
    assert_eq!(attested.records[0].status, Status::Uncertain);
    assert!(!attested.pauses.is_empty());
    assert!(restarted
        .review_multipart_cleanup(&paths, &attested.revision, &evidence, true)
        .is_err());
    assert_eq!(http.actual_requests().count(), before);
}

#[tokio::test]
async fn multipart_completion_conflicts_unsupported_and_bad_verification_never_fall_back_or_retry()
{
    for mode in 0..3 {
        let mut tail = rechecks(true, false, "");
        tail.extend(rechecks(true, false, ""));
        tail.extend([initiated(), part(), part()]);
        tail.extend(rechecks(true, false, ""));
        if mode < 2 {
            tail.push(failure(
                if mode == 0 { 412 } else { 501 },
                if mode == 0 {
                    "PreconditionFailed"
                } else {
                    "NotImplemented"
                },
            ));
            tail.push(empty());
        } else {
            tail.push(completed());
            let mut wrong = written(true, false, "");
            *wrong.body_mut() = SdkBody::from(vec![0; SIZE]);
            tail.push(wrong);
        }
        let (_root, paths, state, prepared, http) = prepared(true, false, "", tail).await;
        let reviewed = reviewed(&state, &prepared).await;
        start(&state, request(&reviewed)).await.unwrap();
        let result = wait(&state, &reviewed.run_id).await.execution.unwrap();
        assert_eq!(result.status, Status::Uncertain);
        assert_eq!(result.counts.verified_files, 0);
        assert!(result.outcome_recorded);
        assert_eq!(
            http.actual_requests()
                .filter(|r| r.method() == "POST")
                .count(),
            2
        );
        assert_eq!(
            http.actual_requests()
                .filter(|r| r.method() == "PUT")
                .count(),
            2
        );
        assert!(state.history(&paths).unwrap().pending_uploads.is_empty());
    }
}

#[tokio::test]
async fn destination_appearing_while_parts_are_staged_is_not_overwritten() {
    let mut tail = rechecks(true, false, "");
    tail.extend(rechecks(true, false, ""));
    tail.extend([initiated(), part(), part(), empty(), empty()]);
    let (_root, _paths, state, prepared, http) = prepared(true, false, "", tail).await;
    let reviewed = reviewed(&state, &prepared).await;
    start(&state, request(&reviewed)).await.unwrap();
    let result = wait(&state, &reviewed.run_id).await.execution.unwrap();
    assert_eq!(result.status, Status::Failed);
    assert_eq!(result.counts.uncertain_files, 0);
    assert_eq!(
        http.actual_requests()
            .filter(|r| r.method() == "POST")
            .count(),
        1
    );
    assert_eq!(
        http.actual_requests()
            .filter(|r| r.method() == "DELETE")
            .count(),
        1
    );
}

#[tokio::test]
async fn multipart_scoped_writers_reject_wrong_operation_key_upload_condition_and_second_transmission(
) {
    let (_root, _paths, state, prepared, http) = prepared(true, false, "", vec![completed()]).await;
    let binding = state
        .0
        .lock()
        .unwrap()
        .slot
        .as_ref()
        .unwrap()
        .binding
        .clone()
        .unwrap();
    multipart::test_guard(binding, &http).await;
    state.release(&prepared.run_id).unwrap();
}

#[tokio::test]
async fn multipart_minimum_budget_is_refused_before_review_reads_or_upload_admission() {
    let (_root, _paths, state, prepared, http) = prepared(true, false, "", vec![]).await;
    {
        let mut inner = state.0.lock().unwrap();
        let binding = Arc::get_mut(inner.slot.as_mut().unwrap().binding.as_mut().unwrap()).unwrap();
        binding.limits.max_requests = 6;
    }
    let before = http.actual_requests().count();
    let failure = review(
        &state,
        ReviewRequest {
            run_id: prepared.run_id,
            preview_id: prepared.preview.unwrap().id,
        },
    )
    .await
    .err()
    .unwrap();
    assert!(failure.to_string().contains("at least 7"));
    assert_eq!(http.actual_requests().count(), before);
}

struct ObserveMultipart {
    paths: crate::jobs::Paths,
    cancel: Arc<AtomicBool>,
    mode: u8,
    snapshot: Option<File>,
    backup: Arc<std::sync::Mutex<Option<Vec<u8>>>>,
}
impl std::fmt::Debug for ObserveMultipart {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ObserveMultipart([private])")
    }
}
impl Intercept for ObserveMultipart {
    fn name(&self) -> &'static str {
        "ObserveMultipart"
    }
    fn modify_before_transmit(
        &self,
        c: &mut BeforeTransmitInterceptorContextMut<'_>,
        _: &RuntimeComponents,
        cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        let op = cfg.load::<Metadata>().map(Metadata::name).unwrap_or("");
        if !matches!(
            op,
            "CreateMultipartUpload"
                | "UploadPart"
                | "CompleteMultipartUpload"
                | "AbortMultipartUpload"
        ) {
            return Ok(());
        }
        let report = crate::job_runs::journal::read(&self.paths).unwrap();
        assert_eq!(report.records[0].status, Status::Executing);
        assert_eq!(
            report.records[0]
                .execution
                .as_ref()
                .unwrap()
                .attempted_files,
            1
        );
        assert_eq!(report.pending_uploads.len(), 1);
        let pending = &report.pending_uploads[0];
        assert_eq!(
            serde_json::to_value(pending.state).unwrap(),
            match op {
                "CreateMultipartUpload" => "initiating",
                "UploadPart" => "active",
                "CompleteMultipartUpload" => "completing",
                _ => "aborting",
            }
        );
        if op == "UploadPart" {
            let uri = url::Url::parse(c.request().uri()).unwrap();
            let number = uri
                .query_pairs()
                .find(|(key, _)| key == "partNumber")
                .unwrap()
                .1
                .parse::<u32>()
                .unwrap();
            assert_eq!(pending.attempted_parts, number);
            assert_eq!(pending.acknowledged_parts, number - 1);
        }
        *self.backup.lock().unwrap() = Some(
            fs::read(
                self.paths
                    .store
                    .parent()
                    .unwrap()
                    .join("job-runs/journal.json"),
            )
            .unwrap(),
        );
        Ok(())
    }
    fn read_after_deserialization(
        &self,
        _:&aws_smithy_runtime_api::client::interceptors::context::AfterDeserializationInterceptorContextRef<'_>,
        _: &RuntimeComponents,
        cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        let op = cfg.load::<Metadata>().map(Metadata::name).unwrap_or("");
        if (self.mode == 1 && op == "CreateMultipartUpload")
            || (self.mode == 2 && op == "UploadPart")
            || (self.mode == 3 && op == "CompleteMultipartUpload")
        {
            fs::write(
                self.paths
                    .store
                    .parent()
                    .unwrap()
                    .join("job-runs/journal.json"),
                "PRIVATE_MULTIPART_CORRUPTION",
            )
            .unwrap();
        }
        if op == "UploadPart" && self.mode == 4 {
            self.cancel.store(true, Ordering::Relaxed);
        }
        if op == "UploadPart" && self.mode == 5 {
            self.snapshot
                .as_ref()
                .unwrap()
                .write_at(b"different", SIZE as u64 - 9)
                .unwrap();
        }
        Ok(())
    }
}

#[tokio::test]
async fn every_multipart_stage_has_durable_intent_and_postresponse_journal_faults_stop_further_mutations(
) {
    for mode in 0..=5 {
        let mut tail = rechecks(true, false, "");
        tail.extend(rechecks(true, false, ""));
        tail.push(initiated());
        match mode {
            0 | 3 => {
                tail.extend([part(), part()]);
                tail.extend(rechecks(true, false, ""));
                tail.extend([completed(), written(true, false, "")]);
            }
            1 => {}
            2 => tail.push(part()),
            4 => tail.extend([part(), empty()]),
            5 => tail.extend([part(), part(), empty()]),
            _ => unreachable!(),
        }
        let (_root, paths, state, prepared, http) = prepared(true, false, "", tail).await;
        let backup = Arc::new(std::sync::Mutex::new(None));
        {
            let mut inner = state.0.lock().unwrap();
            let slot = inner.slot.as_mut().unwrap();
            let snapshot = slot.preview.as_ref().unwrap().items[0]
                .snapshot
                .file
                .try_clone()
                .unwrap();
            let binding = Arc::get_mut(slot.binding.as_mut().unwrap()).unwrap();
            binding.destination.client = Client::from_conf(
                binding
                    .destination
                    .client
                    .config()
                    .to_builder()
                    .interceptor(ObserveMultipart {
                        paths: paths.clone(),
                        cancel: slot.cancel.clone(),
                        mode,
                        snapshot: Some(snapshot),
                        backup: backup.clone(),
                    })
                    .build(),
            );
        }
        let reviewed = reviewed(&state, &prepared).await;
        start(&state, request(&reviewed)).await.unwrap();
        let result = wait(&state, &reviewed.run_id).await.execution.unwrap();
        if mode == 0 {
            assert_eq!(result.status, Status::Succeeded, "{:?}", result.message);
            continue;
        }
        if mode <= 3 {
            assert_eq!(result.status, Status::Uncertain);
            assert!(!result.outcome_recorded);
            assert_eq!(
                http.actual_requests()
                    .filter(|r| r.method() == "PUT")
                    .count(),
                match mode {
                    1 => 0,
                    2 => 1,
                    _ => 2,
                }
            );
            assert_eq!(
                http.actual_requests()
                    .filter(|r| r.method() == "DELETE")
                    .count(),
                0
            );
            assert!(!result
                .message
                .unwrap()
                .contains("PRIVATE_MULTIPART_CORRUPTION"));
            fs::write(
                paths.store.parent().unwrap().join("job-runs/journal.json"),
                backup.lock().unwrap().as_ref().unwrap(),
            )
            .unwrap();
            let before = http.actual_requests().count();
            let recovered = state.history(&paths).unwrap();
            assert_eq!(recovered.records[0].status, Status::Interrupted);
            assert_eq!(recovered.pending_uploads.len(), 1);
            assert!(recovered.in_memory_storage_fault);
            assert_eq!(http.actual_requests().count(), before);
        } else {
            assert_eq!(
                result.status,
                if mode == 4 {
                    Status::Uncertain
                } else {
                    Status::Failed
                },
                "{:?}",
                result.message
            );
            assert!(result.outcome_recorded);
            assert_eq!(result.counts.verified_files, 0);
            assert_eq!(
                http.actual_requests()
                    .filter(|r| r.method() == "POST")
                    .count(),
                1
            );
            assert_eq!(
                http.actual_requests()
                    .filter(|r| r.method() == "DELETE")
                    .count(),
                1
            );
            assert_eq!(state.history(&paths).unwrap().pending_uploads.len(),usize::from(mode==4));
        }
    }
}

#[tokio::test]
#[ignore = "requires an explicitly configured disposable loopback MinIO fixture"]
async fn loopback_minio_multipart_local_and_s3_creates_and_replacements_verify_payloads_attributes_and_cleanup(
) {
    for local in [true, false] {
        for versioned in [false, true] {
            for replace in [false, true] {
                super::replacement_tests::replacement_fixture(local, versioned, true, replace)
                    .await;
            }
        }
    }
}
