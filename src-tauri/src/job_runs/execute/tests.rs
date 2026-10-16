use super::*;
use crate::job_runs::{
    scan,
    tests::{bind_fixture, empty, fixture_with_names, listing, payload, replay},
};
use aws_smithy_http_client::test_util::StaticReplayClient;
use aws_smithy_types::body::SdkBody;
use std::{fs, path::Path, time::Duration};
use tauri::http::Response;

fn absent() -> Response<SdkBody> {
    Response::builder()
        .status(404)
        .body(SdkBody::from("<Error><Code>NoSuchKey</Code></Error>"))
        .unwrap()
}
fn verified(text: &str, sse: bool) -> Response<SdkBody> {
    let mut response = payload(text, "written-etag");
    response
        .headers_mut()
        .insert("content-type", "application/octet-stream".parse().unwrap());
    if sse {
        response.headers_mut().insert(
            "x-amz-server-side-encryption-customer-algorithm",
            "AES256".parse().unwrap(),
        );
    }
    response
}
fn ack() -> Response<SdkBody> {
    Response::builder()
        .status(200)
        .header("etag", "written-etag")
        .body(SdkBody::empty())
        .unwrap()
}
async fn prepared(
    sse: bool,
    tail: Vec<Response<SdkBody>>,
) -> (
    tempfile::TempDir,
    crate::jobs::Paths,
    JobRunState,
    RunStatus,
    StaticReplayClient,
) {
    let (root, paths, job) = fixture_with_names(
        true,
        sse,
        "https://job.test",
        "source-bucket",
        "destination-bucket",
        None,
        "src/",
        "dest/",
    );
    let SourcePin::Local { path, .. } = &job.source else {
        panic!()
    };
    fs::write(Path::new(path).join("file"), "abc").unwrap();
    let mut responses = vec![empty(), listing("dest/", None), absent()];
    responses.extend(tail);
    let http = replay(responses);
    let state = JobRunState::default();
    let bound = bind_fixture(&state, &paths, &job, &http).await;
    let compared = scan(&state, &bound.run_id, Arc::new(|_| {})).await.unwrap();
    let rows = state
        .page(&bound.run_id, compared.manifest_id.as_deref().unwrap(), 0)
        .unwrap();
    let result = preview::prepare(
        &state,
        preview::Request {
            run_id: bound.run_id,
            manifest_id: compared.manifest_id.unwrap(),
            entry_ids: rows.iter().map(|r| r.id.clone()).collect(),
            policy: preview::Policy::CreateOnly,
            max_snapshot_bytes: 100,
            approved: true,
        },
        Arc::new(|_| {}),
    )
    .await
    .unwrap();
    assert_eq!(result.phase, "prepared", "{:?}", result.message);
    (root, paths, state, result, http)
}
async fn reviewed(state: &JobRunState, prepared: &RunStatus) -> RunStatus {
    let result = review(
        state,
        ReviewRequest {
            run_id: prepared.run_id.clone(),
            preview_id: prepared.preview.as_ref().unwrap().id.clone(),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        result.phase, "awaiting_execution_approval",
        "{:?}",
        result.message
    );
    result
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
    }
}
async fn wait(state: &JobRunState, id: &str) -> RunStatus {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let status = state.status(id).unwrap();
            if status.phase == "stopped" {
                return status;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn create_only_consumes_native_approval_and_verifies_exact_snapshot_with_independent_sse_destination(
) {
    for sse in [false, true] {
        let (_root, paths, state, prepared, http) =
            prepared(sse, vec![absent(), absent(), ack(), verified("abc", sse)]).await;
        let review = reviewed(&state, &prepared).await;
        assert!(review.execution_available);
        assert_eq!(http.actual_requests().count(), 4);
        let approval = request(&review);
        let duplicate = request(&review);
        start(&state, approval).await.unwrap();
        assert!(start(&state, duplicate).await.is_err());
        let result = wait(&state, &review.run_id).await;
        let execution = result.execution.unwrap();
        assert_eq!(
            execution.status,
            Status::Succeeded,
            "{:?}",
            execution.message
        );
        assert!(execution.outcome_recorded);
        assert_eq!(execution.counts.verified_bytes, 3);
        assert_eq!(execution.counts.verified_files, 1);
        assert!(result.execution_review.is_none());
        assert!(!result.execution_available);
        let history = state.history(&paths).unwrap();
        assert_eq!(history.records[0].status, Status::Succeeded);
        assert_eq!(
            history.records[0]
                .execution
                .as_ref()
                .unwrap()
                .verified_files,
            1
        );
        assert!(history.pauses.is_empty());
        let requests: Vec<_> = http.actual_requests().collect();
        let writes: Vec<_> = requests.iter().filter(|r| r.method() == "PUT").collect();
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].headers().get("if-none-match"), Some("*"));
        assert_eq!(
            url::Url::parse(writes[0].uri()).unwrap().path(),
            "/destination-bucket/dest/file"
        );
        assert_eq!(
            writes[0]
                .headers()
                .contains_key("x-amz-server-side-encryption-customer-key"),
            sse
        );
        assert_eq!(writes[0].body().bytes().unwrap(), b"abc");
        assert_eq!(requests.len(), 7);
    }
}

#[tokio::test]
async fn preapproval_identity_payload_and_destination_drift_never_send_put() {
    for kind in 0..5 {
        let (_root, paths, state, prepared, http) =
            prepared(false, vec![if kind == 2 { empty() } else { absent() }]).await;
        if kind == 0 {
            let inner = state.0.lock().unwrap();
            let binding = inner.slot.as_ref().unwrap().binding.as_ref().unwrap();
            let SourcePin::Local { path, .. } = &binding.job.source else {
                panic!()
            };
            fs::write(Path::new(path).join("file"), "different").unwrap();
        }
        if kind == 1 {
            let inner = state.0.lock().unwrap();
            let snapshot =
                &inner.slot.as_ref().unwrap().preview.as_ref().unwrap().items[0].snapshot;
            snapshot.file.write_at(b"xyz", 0).unwrap();
        }
        if kind == 3 {
            let mut config: serde_json::Value =
                serde_json::from_slice(&fs::read(&paths.config).unwrap()).unwrap();
            config["connections"][1]["region"] = "eu-west-1".into();
            fs::write(&paths.config, serde_json::to_vec(&config).unwrap()).unwrap();
        }
        if kind == 4 {
            state
                .0
                .lock()
                .unwrap()
                .slot
                .as_mut()
                .unwrap()
                .preview
                .as_mut()
                .unwrap()
                .items[0]
                .snapshot
                .sha256 = "changed-native-digest".into();
        }
        let result = review(
            &state,
            ReviewRequest {
                run_id: prepared.run_id.clone(),
                preview_id: prepared.preview.unwrap().id,
            },
        )
        .await;
        assert!(result.is_err() || result.as_ref().unwrap().phase == "stopped");
        assert_eq!(
            http.actual_requests()
                .filter(|r| r.method() == "PUT")
                .count(),
            0
        );
        assert!(state
            .status(&prepared.run_id)
            .unwrap()
            .execution_review
            .is_none());
    }
}

#[tokio::test]
async fn approved_execution_still_rechecks_source_and_missing_destination_and_cannot_replay() {
    for conflict in [false, true] {
        let (_root, _paths, state, prepared, http) = prepared(
            false,
            vec![absent(), if conflict { empty() } else { absent() }],
        )
        .await;
        let review = reviewed(&state, &prepared).await;
        if !conflict {
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
            fs::write(Path::new(path).join("file"), "changed").unwrap();
        }
        start(&state, request(&review)).await.unwrap();
        let result = wait(&state, &review.run_id).await;
        assert_eq!(result.execution.unwrap().status, Status::Failed);
        assert_eq!(
            http.actual_requests()
                .filter(|r| r.method() == "PUT")
                .count(),
            0
        );
        assert!(start(&state, request(&review)).await.is_err());
    }
}

#[tokio::test]
async fn transmitted_failures_and_failed_verification_are_uncertain_never_retried_or_called_success(
) {
    for rejection in [
        None,
        Some((503, "SlowDown")),
        Some((501, "NotImplemented")),
        Some((412, "PreconditionFailed")),
        Some((403, "AccessDenied")),
    ] {
        let mut tail = vec![absent(), absent()];
        if let Some((status, code)) = rejection {
            tail.push(
                Response::builder()
                    .status(status)
                    .body(SdkBody::from(format!("<Error><Code>{code}</Code></Error>")))
                    .unwrap(),
            );
        } else {
            tail.extend([ack(), verified("xyz", false)]);
        }
        let (_root, paths, state, prepared, http) = prepared(false, tail).await;
        let review = reviewed(&state, &prepared).await;
        start(&state, request(&review)).await.unwrap();
        let result = wait(&state, &review.run_id).await;
        let result = result.execution.unwrap();
        assert_eq!(result.status, Status::Uncertain, "{:?}", result.message);
        assert_eq!(result.counts.verified_files, 0);
        assert_eq!(result.counts.uncertain_files, 1);
        assert!(result.outcome_recorded);
        if let Some((status, code)) = rejection {
            let message = result.message.as_deref().unwrap();
            assert!(message.contains(&format!("HTTP {status}")));
            assert!(message.contains(code));
            assert!(!message.contains("SSO"));
        }
        assert_eq!(
            http.actual_requests()
                .filter(|r| r.method() == "PUT")
                .count(),
            1
        );
        let history = state.history(&paths).unwrap();
        assert!(history.pauses.contains_key(&history.records[0].job_id));
    }
}

#[tokio::test]
async fn wrong_unknown_unapproved_expired_and_cancelled_approvals_never_mutate() {
    let (_root, _paths, state, prepared, http) = prepared(false, vec![absent()]).await;
    let review = reviewed(&state, &prepared).await;
    let mut unapproved = request(&review);
    unapproved.approved = false;
    assert!(start(&state, unapproved).await.is_err());
    let mut exclusions = request(&review);
    exclusions.acknowledge_exclusions = false;
    assert!(start(&state, exclusions).await.is_err());
    let mut wrong = request(&review);
    wrong.approval_id = fresh_id();
    assert!(start(&state, wrong).await.is_err());
    let mut cross = request(&review);
    cross.run_id = fresh_id();
    assert!(start(&state, cross).await.is_err());
    assert!(serde_json::from_value::<ExecuteRequest>(serde_json::json!({"run_id":review.run_id,"approval_id":"x","approved":true,"acknowledge_exclusions":true,"key":"caller-key"})).is_err());
    state.release(&review.run_id).unwrap();
    assert!(start(&state, request(&review)).await.is_err());
    assert_eq!(
        http.actual_requests()
            .filter(|r| r.method() == "PUT")
            .count(),
        0
    );
    let (_root, _paths, state, prepared, http) = self::prepared(false, vec![absent()]).await;
    let review = reviewed(&state, &prepared).await;
    state.0.lock().unwrap().slot.as_mut().unwrap().expires =
        Instant::now() - Duration::from_secs(1);
    assert!(start(&state, request(&review)).await.is_err());
    assert_eq!(
        http.actual_requests()
            .filter(|r| r.method() == "PUT")
            .count(),
        0
    );
}

#[tokio::test]
async fn derived_writer_rejects_wrong_key_origin_operation_headers_and_repeated_transmission() {
    let (_root, _paths, state, prepared, http) = prepared(false, vec![ack()]).await;
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
    let blocked = crate::s3::compat::configure_conditions(
        binding.destination.client.clone(),
        Some("garage"),
        None,
    );
    assert!(!crate::s3::compat::conditional_writes_supported(&blocked));
    let (client, _) = writer(
        binding.clone(),
        "dest/file".into(),
        Arc::new(AtomicBool::new(false)),
        None,
        Arc::new(ScanBudget::with_limits(binding.limits)),
    )
    .unwrap();
    let (expired, _) = writer(
        binding.clone(),
        "dest/file".into(),
        Arc::new(AtomicBool::new(false)),
        Some(Instant::now() - Duration::from_secs(1)),
        Arc::new(ScanBudget::with_limits(binding.limits)),
    )
    .unwrap();
    assert!(expired
        .put_object()
        .bucket("destination-bucket")
        .key("dest/file")
        .if_none_match("*")
        .send()
        .await
        .is_err());
    let mut limits = binding.limits;
    limits.max_seconds = 0;
    let (late, _) = writer(
        binding.clone(),
        "dest/file".into(),
        Arc::new(AtomicBool::new(false)),
        None,
        Arc::new(ScanBudget::with_limits(limits)),
    )
    .unwrap();
    assert!(late
        .put_object()
        .bucket("destination-bucket")
        .key("dest/file")
        .if_none_match("*")
        .send()
        .await
        .is_err());
    for key in ["other/file", "dest//file", "dest/../file"] {
        assert!(client
            .put_object()
            .bucket("destination-bucket")
            .key(key)
            .if_none_match("*")
            .body(aws_sdk_s3::primitives::ByteStream::from_static(b"abc"))
            .send()
            .await
            .is_err());
    }
    assert!(client
        .put_object()
        .bucket("destination-bucket")
        .key("dest/file")
        .send()
        .await
        .is_err());
    assert!(client
        .delete_object()
        .bucket("destination-bucket")
        .key("dest/file")
        .send()
        .await
        .is_err());
    let redirected = Client::from_conf(
        client
            .config()
            .to_builder()
            .endpoint_url("https://elsewhere.test")
            .build(),
    );
    assert!(redirected
        .put_object()
        .bucket("destination-bucket")
        .key("dest/file")
        .if_none_match("*")
        .send()
        .await
        .is_err());
    assert_eq!(http.actual_requests().count(), 3);
    client
        .put_object()
        .bucket("destination-bucket")
        .key("dest/file")
        .if_none_match("*")
        .send()
        .await
        .unwrap();
    assert!(client
        .put_object()
        .bucket("destination-bucket")
        .key("dest/file")
        .if_none_match("*")
        .send()
        .await
        .is_err());
    assert_eq!(http.actual_requests().count(), 4);
    state.release(&prepared.run_id).unwrap();
}

#[tokio::test]
async fn narrow_cohort_explicitly_blocks_replacements_multipart_and_ambiguous_keys() {
    let (_root, _paths, state, prepared, http) = prepared(false, vec![]).await;
    {
        let mut inner = state.0.lock().unwrap();
        let plan = inner.slot.as_mut().unwrap().preview.as_mut().unwrap();
        let original = plan.items[0].key.clone();
        plan.items[0].key = "dest//file".into();
        assert!(plan.execution_blocker().unwrap().contains("normalizing"));
        plan.items[0].key = original;
        plan.items[0].snapshot.size = 5 * 1024 * 1024 + 1;
        assert!(plan.execution_blocker().unwrap().contains("5 MiB"));
        plan.items[0].snapshot.size = 3;
        plan.policy = preview::Policy::ReplaceUnchanged;
        assert!(plan.execution_blocker().unwrap().contains("Replacements"));
    }
    assert!(review(
        &state,
        ReviewRequest {
            run_id: prepared.run_id.clone(),
            preview_id: prepared.preview.unwrap().id
        }
    )
    .await
    .is_err());
    assert_eq!(http.actual_requests().count(), 3);
}

#[derive(Clone)]
struct ObserveIntent {
    paths: crate::jobs::Paths,
    cancel: Arc<AtomicBool>,
    action: u8,
    put_count: Arc<std::sync::atomic::AtomicU64>,
}
impl std::fmt::Debug for ObserveIntent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ObserveNativeJournal")
    }
}
impl Intercept for ObserveIntent {
    fn name(&self) -> &'static str {
        "ObserveNativeJournal"
    }
    fn modify_before_transmit(
        &self,
        _: &mut BeforeTransmitInterceptorContextMut<'_>,
        _: &RuntimeComponents,
        cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        if cfg.load::<Metadata>().map(Metadata::name) == Some("PutObject") {
            let history = crate::job_runs::journal::read(&self.paths).unwrap();
            let record = &history.records[0];
            assert_eq!(record.status, Status::Executing);
            let counts = record.execution.as_ref().unwrap();
            assert_eq!(counts.attempted_files, 1);
            assert_eq!(counts.verified_files, 0);
            self.put_count.fetch_add(1, Ordering::Relaxed);
        }
        Ok(())
    }
    fn read_after_deserialization(
        &self,
        _:&aws_smithy_runtime_api::client::interceptors::context::AfterDeserializationInterceptorContextRef<'_>,
        _: &RuntimeComponents,
        cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        let operation = cfg.load::<Metadata>().map(Metadata::name).unwrap_or("");
        if (self.action == 1 && operation == "PutObject")
            || (self.action == 2 && operation == "GetObject")
        {
            fs::write(
                self.paths
                    .store
                    .parent()
                    .unwrap()
                    .join("job-runs/journal.json"),
                "PRIVATE_CORRUPTION_MUST_NOT_BE_IN_ERRORS",
            )
            .unwrap();
        }
        if self.action == 3 && operation == "PutObject" {
            self.cancel.store(true, Ordering::Relaxed);
        }
        Ok(())
    }
}

#[tokio::test]
async fn durable_intent_precedes_transmission_and_postwrite_journal_fault_or_cancel_never_report_success(
) {
    for action in 0..4 {
        let (_root, paths, state, prepared, http) = prepared(
            false,
            vec![absent(), absent(), ack(), verified("abc", false)],
        )
        .await;
        let count = Arc::new(std::sync::atomic::AtomicU64::new(0));
        {
            let mut inner = state.0.lock().unwrap();
            let slot = inner.slot.as_mut().unwrap();
            let cancel = slot.cancel.clone();
            let binding = Arc::get_mut(slot.binding.as_mut().unwrap()).unwrap();
            binding.destination.client = Client::from_conf(
                binding
                    .destination
                    .client
                    .config()
                    .to_builder()
                    .interceptor(ObserveIntent {
                        paths: paths.clone(),
                        cancel,
                        action,
                        put_count: count.clone(),
                    })
                    .build(),
            );
        }
        let review = reviewed(&state, &prepared).await;
        start(&state, request(&review)).await.unwrap();
        let result = wait(&state, &review.run_id).await;
        let execution = result.execution.unwrap();
        assert_eq!(count.load(Ordering::Relaxed), 1);
        assert_eq!(
            http.actual_requests()
                .filter(|r| r.method() == "PUT")
                .count(),
            1
        );
        if action == 0 {
            assert_eq!(execution.status, Status::Succeeded);
            assert!(execution.outcome_recorded);
        } else {
            assert_eq!(
                execution.status,
                Status::Uncertain,
                "{:?}",
                execution.message
            );
            assert!(!execution
                .message
                .as_deref()
                .unwrap_or("")
                .contains("PRIVATE_CORRUPTION_MUST_NOT_BE_IN_ERRORS"));
            if action < 3 {
                assert!(!execution.outcome_recorded);
                assert!(state.history(&paths).is_err());
            } else {
                assert!(execution.outcome_recorded);
                let history = state.history(&paths).unwrap();
                assert_eq!(history.records[0].status, Status::Uncertain);
                assert!(!history.pauses.is_empty());
            }
        }
    }
}

#[tokio::test]
async fn multiple_files_record_confirmed_partial_and_explicit_skip_existing_without_calling_them_synced(
) {
    for skip in [false, true] {
        let (_root, paths, job) = fixture_with_names(
            true,
            false,
            "https://job.test",
            "source-bucket",
            "destination-bucket",
            None,
            "src/",
            "dest/",
        );
        let SourcePin::Local { path, .. } = &job.source else {
            panic!()
        };
        for name in ["a", "b"] {
            fs::write(Path::new(path).join(name), "abc").unwrap();
        }
        let responses = if skip {
            vec![
                empty(),
                listing("dest/", Some(("a", 3, "old-etag"))),
                payload("old", "old-etag"),
                absent(),
                absent(),
                absent(),
                ack(),
                verified("abc", false),
            ]
        } else {
            vec![
                empty(),
                listing("dest/", None),
                absent(),
                absent(),
                absent(),
                absent(),
                absent(),
                ack(),
                verified("abc", false),
                empty(),
            ]
        };
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
                policy: if skip {
                    preview::Policy::SkipExisting
                } else {
                    preview::Policy::CreateOnly
                },
                max_snapshot_bytes: 100,
                approved: true,
            },
            Arc::new(|_| {}),
        )
        .await
        .unwrap();
        assert_eq!(prepared.phase, "prepared");
        let review = reviewed(&state, &prepared).await;
        start(&state, request(&review)).await.unwrap();
        let result = wait(&state, &review.run_id).await;
        let execution = result.execution.unwrap();
        assert_eq!(
            execution.counts.verified_files, 1,
            "{:?}",
            execution.message
        );
        assert_eq!(execution.counts.verified_bytes, 3);
        assert_eq!(
            execution.status,
            if skip {
                Status::Skipped
            } else {
                Status::Partial
            }
        );
        assert_eq!(execution.counts.skipped_files, if skip { 1 } else { 0 });
        assert!(execution.outcome_recorded);
        assert_eq!(
            http.actual_requests()
                .filter(|r| r.method() == "PUT")
                .count(),
            1
        );
        let history = state.history(&paths).unwrap();
        assert_eq!(history.records[0].status, execution.status);
        assert_eq!(!history.pauses.is_empty(), !skip);
    }
}

#[tokio::test]
async fn initial_or_pretransmission_journal_failures_block_mutation_and_never_restore_approval() {
    for after_intent in [false, true] {
        let (_root, paths, state, prepared, http) =
            prepared(false, vec![absent(), absent(), ack()]).await;
        let review = reviewed(&state, &prepared).await;
        if !after_intent {
            fs::write(
                paths.store.parent().unwrap().join("job-runs/journal.json"),
                "CORRUPT_BLOCKED_BEFORE_PUT",
            )
            .unwrap();
        } else {
            state
                .0
                .lock()
                .unwrap()
                .journal_fault
                .store(true, Ordering::Relaxed);
        }
        let admitted = start(&state, request(&review)).await;
        if admitted.is_ok() {
            let stopped = wait(&state, &review.run_id).await;
            assert!(!stopped.execution.unwrap().outcome_recorded);
        }
        assert_eq!(
            http.actual_requests()
                .filter(|r| r.method() == "PUT")
                .count(),
            0
        );
        assert!(start(&state, request(&review)).await.is_err());
    }
}

#[tokio::test]
async fn cancellation_immediately_after_admission_is_terminal_without_fake_storage_fault_or_writes()
{
    let (_root, paths, state, prepared, http) = prepared(false, vec![absent()]).await;
    let review = reviewed(&state, &prepared).await;
    start(&state, request(&review)).await.unwrap();
    state.release(&review.run_id).unwrap();
    let result = wait(&state, &review.run_id).await;
    let execution = result.execution.unwrap();
    assert_eq!(
        execution.status,
        Status::Cancelled,
        "{:?}",
        execution.message
    );
    assert!(execution.outcome_recorded);
    assert_eq!(execution.counts.attempted_files, 0);
    assert_eq!(
        http.actual_requests()
            .filter(|r| r.method() == "PUT")
            .count(),
        0
    );
    let history = state.history(&paths).unwrap();
    assert!(!history.in_memory_storage_fault);
    assert!(history.pauses.is_empty());
    assert_eq!(history.records[0].status, Status::Cancelled);
}

#[tokio::test]
async fn execution_budget_is_shared_by_preflight_put_and_mandatory_verification() {
    let (_root, _paths, state, prepared, http) =
        prepared(false, vec![absent(), absent(), ack()]).await;
    {
        let mut inner = state.0.lock().unwrap();
        Arc::get_mut(inner.slot.as_mut().unwrap().binding.as_mut().unwrap())
            .unwrap()
            .limits
            .max_requests = 2;
    }
    let review = reviewed(&state, &prepared).await;
    start(&state, request(&review)).await.unwrap();
    let result = wait(&state, &review.run_id).await;
    let execution = result.execution.unwrap();
    assert_eq!(execution.status, Status::Uncertain);
    assert_eq!(execution.counts.acknowledged_files, 1);
    assert_eq!(execution.counts.verified_files, 0);
    assert!(execution.outcome_recorded);
    assert_eq!(http.actual_requests().count(), 6);
    assert_eq!(
        http.actual_requests()
            .filter(|r| r.method() == "PUT")
            .count(),
        1
    );
    assert!(execution.message.unwrap().contains("budget"));
}

#[tokio::test]
async fn cancelling_a_stalled_production_put_retains_native_outcome_ownership_and_records_uncertainty(
) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        for index in 0..6 {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = vec![];
            let mut buffer = [0; 4096];
            while !bytes.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = socket.read(&mut buffer).await.unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&buffer[..n]);
            }
            let first = std::str::from_utf8(&bytes).unwrap().lines().next().unwrap();
            if index == 5 {
                assert!(first.starts_with("PUT /destination-bucket/dest/file"));
                assert!(std::str::from_utf8(&bytes)
                    .unwrap()
                    .to_ascii_lowercase()
                    .contains("if-none-match: *"));
                let _ = tx.send(());
                std::future::pending::<()>().await;
                return;
            }
            let (code, body) = if index == 0 {
                assert!(first.starts_with("HEAD "));
                ("200 OK", "")
            } else if index == 1 {
                assert!(first.starts_with("GET "));
                (
                    "200 OK",
                    "<ListBucketResult><IsTruncated>false</IsTruncated></ListBucketResult>",
                )
            } else {
                assert!(first.starts_with("HEAD "));
                ("404 Not Found", "")
            };
            socket
                .write_all(
                    format!(
                        "HTTP/1.1 {code}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
        }
    });
    let (_root, paths, job) = fixture_with_names(
        true,
        false,
        &format!("http://{address}"),
        "source-unused",
        "destination-bucket",
        None,
        "src/",
        "dest/",
    );
    let SourcePin::Local { path, .. } = &job.source else {
        panic!()
    };
    fs::write(Path::new(path).join("file"), "abc").unwrap();
    let state = JobRunState::default();
    let bound = crate::job_runs::bind_with(
        &state,
        paths.clone(),
        fresh_id(),
        job.id,
        job.revision,
        true,
        Arc::new(|_| Ok(Some("FAKE_LOOPBACK_SECRET".into()))),
        Arc::new(crate::job_runs::client),
    )
    .await
    .unwrap();
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
            policy: preview::Policy::CreateOnly,
            max_snapshot_bytes: 100,
            approved: true,
        },
        Arc::new(|_| {}),
    )
    .await
    .unwrap();
    let review = reviewed(&state, &prepared).await;
    start(&state, request(&review)).await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), rx)
        .await
        .unwrap()
        .unwrap();
    state.release(&review.run_id).unwrap();
    assert!(state.0.lock().unwrap().slot.as_ref().unwrap().phase == Phase::Executing);
    assert!(crate::job_runs::run_lock(&paths).is_err());
    let result = wait(&state, &review.run_id).await;
    let execution = result.execution.unwrap();
    assert_eq!(execution.status, Status::Uncertain);
    assert_eq!(execution.counts.attempted_files, 1);
    assert_eq!(execution.counts.acknowledged_files, 0);
    assert!(execution.outcome_recorded);
    assert!(crate::job_runs::run_lock(&paths).is_ok());
    assert_eq!(
        state.history(&paths).unwrap().records[0].status,
        Status::Uncertain
    );
    server.abort();
}

#[tokio::test]
#[ignore = "requires an explicitly configured disposable loopback MinIO fixture"]
async fn loopback_minio_local_create_execution_and_conflict_never_overwrite_existing() {
    use crate::job_runs::{bind_with, client, Auth};
    let endpoint =
        std::env::var("FURMAN_JOB_TEST_ENDPOINT").expect("Explicit disposable endpoint required");
    let parsed = url::Url::parse(&endpoint).unwrap();
    assert_eq!(parsed.scheme(), "http");
    assert_eq!(parsed.host_str(), Some("127.0.0.1"));
    assert!(parsed.username().is_empty() && parsed.password().is_none());
    let access = std::env::var("FURMAN_JOB_TEST_ACCESS_KEY").unwrap();
    let secret = zeroize::Zeroizing::new(std::env::var("FURMAN_JOB_TEST_SECRET_KEY").unwrap());
    let setup = Client::from_conf(
        aws_sdk_s3::Config::builder()
            .behavior_version_latest()
            .region(aws_sdk_s3::config::Region::new("us-east-1"))
            .endpoint_url(&endpoint)
            .force_path_style(true)
            .credentials_provider(aws_credential_types::Credentials::new(
                &access,
                secret.as_str(),
                None,
                None,
                "loopback-execution-fixture",
            ))
            .request_checksum_calculation(
                aws_sdk_s3::config::RequestChecksumCalculation::WhenRequired,
            )
            .build(),
    );
    let bucket = format!("furman-job-execution-{}", fresh_id());
    setup.create_bucket().bucket(&bucket).send().await.unwrap();
    let result: Result<(), FmError> = async {
        for conflict in [false, true] {
            let (_root, paths, job) = fixture_with_names(
                true,
                false,
                &endpoint,
                "source-unused",
                &bucket,
                Some(&access),
                "src/",
                if conflict { "conflict/" } else { "created/" },
            );
            let SourcePin::Local { path, .. } = &job.source else {
                panic!()
            };
            fs::write(Path::new(path).join("file"), "abc").unwrap();
            let state = JobRunState::default();
            let secret = secret.clone();
            let bound = bind_with(
                &state,
                paths.clone(),
                fresh_id(),
                job.id,
                job.revision,
                true,
                Arc::new(move |_| Ok(Some(secret.to_string()))),
                Arc::new(|auth: Auth| client(auth)),
            )
            .await?;
            let compared = scan(&state, &bound.run_id, Arc::new(|_| {})).await?;
            let rows = state.page(
                &bound.run_id,
                compared
                    .manifest_id
                    .as_deref()
                    .ok_or_else(|| error("Fixture comparison failed"))?,
                0,
            )?;
            let prepared = preview::prepare(
                &state,
                preview::Request {
                    run_id: bound.run_id,
                    manifest_id: compared.manifest_id.unwrap(),
                    entry_ids: rows.iter().map(|r| r.id.clone()).collect(),
                    policy: preview::Policy::CreateOnly,
                    max_snapshot_bytes: 100,
                    approved: true,
                },
                Arc::new(|_| {}),
            )
            .await?;
            let review = reviewed(&state, &prepared).await;
            let key = if conflict {
                "conflict/file"
            } else {
                "created/file"
            };
            if conflict {
                setup
                    .put_object()
                    .bucket(&bucket)
                    .key(key)
                    .body(aws_sdk_s3::primitives::ByteStream::from_static(b"existing"))
                    .send()
                    .await
                    .map_err(|e| job_sdk_error("Fixture conflict seeding", &e))?;
            }
            start(&state, request(&review)).await?;
            let stopped = wait(&state, &review.run_id).await;
            let execution = stopped
                .execution
                .ok_or_else(|| error("Missing fixture execution outcome"))?;
            if execution.status
                != if conflict {
                    Status::Failed
                } else {
                    Status::Succeeded
                }
                || !execution.outcome_recorded
            {
                return Err(error(&format!(
                    "Unexpected fixture outcome: {:?}",
                    execution.message
                )));
            }
            let response = setup
                .get_object()
                .bucket(&bucket)
                .key(key)
                .send()
                .await
                .map_err(|e| job_sdk_error("Fixture execution verification", &e))?;
            let bytes = response
                .body
                .collect()
                .await
                .map_err(|_| error("Fixture verification stream failed"))?
                .into_bytes();
            if bytes.as_ref()
                != if conflict {
                    b"existing".as_slice()
                } else {
                    b"abc".as_slice()
                }
            {
                return Err(error(
                    "Job execution changed a conflicting object or failed to create checked bytes",
                ));
            }
            let history = state.history(&paths)?;
            if history.records[0].status != execution.status {
                return Err(error("Fixture outcome not durably journaled"));
            }
        }
        Ok(())
    }
    .await;
    for key in ["created/file", "conflict/file"] {
        let _ = setup.delete_object().bucket(&bucket).key(key).send().await;
    }
    let cleanup = setup.delete_bucket().bucket(&bucket).send().await;
    result.unwrap();
    cleanup.unwrap();
}
