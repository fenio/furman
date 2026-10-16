use super::s3_tests::{ack, acl, object, tags, unlocked};
use super::*;
use crate::job_runs::{
    scan,
    tests::{bind_fixture, empty, fixture_with_names, listing, payload, replay},
};
use aws_smithy_http_client::test_util::StaticReplayClient;
use aws_smithy_types::body::SdkBody;
use std::{fs, path::Path, time::Duration};
use tauri::http::Response;

fn old(encryption: &str) -> Response<SdkBody> {
    let mut response = object(Some("before-version"), encryption);
    if encryption == "kms" {
        response
            .headers_mut()
            .insert("x-amz-server-side-encryption", "aws:kms".parse().unwrap());
    }
    if encryption == "glacier" {
        response
            .headers_mut()
            .insert("x-amz-storage-class", "GLACIER".parse().unwrap());
    }
    if encryption == "client" {
        response.headers_mut().insert(
            "x-amz-meta-furman-encrypted",
            "aes-256-gcm".parse().unwrap(),
        );
    }
    if encryption == "gzip" {
        response
            .headers_mut()
            .insert("content-encoding", "gzip".parse().unwrap());
    }
    response
        .headers_mut()
        .insert("etag", "old-etag".parse().unwrap());
    response
        .headers_mut()
        .insert("content-type", "application/json".parse().unwrap());
    response
        .headers_mut()
        .insert("cache-control", "private, max-age=1".parse().unwrap());
    response.headers_mut().insert(
        "x-amz-meta-preserved",
        "destination-only-metadata".parse().unwrap(),
    );
    *response.body_mut() = SdkBody::from("old");
    response
}
fn head() -> Response<SdkBody> {
    Response::builder()
        .status(200)
        .header("etag", "old-etag")
        .header("content-length", 3)
        .body(SdkBody::empty())
        .unwrap()
}
fn destination_reads(encryption: &str) -> Vec<Response<SdkBody>> {
    vec![
        old(encryption),
        unlocked(),
        acl("old-owner"),
        tags(Some("before-version"), "destination-only-tag"),
    ]
}
fn source_reads() -> Vec<Response<SdkBody>> {
    vec![
        object(Some("source-version"), ""),
        unlocked(),
        acl("source-owner"),
        tags(Some("source-version"), "native-only-tag-value"),
    ]
}
fn review_reads(local: bool, encryption: &str) -> Vec<Response<SdkBody>> {
    let mut reads = destination_reads(encryption);
    if !local {
        reads.extend(source_reads());
    }
    reads
}
fn written(local: bool, encryption: &str) -> Response<SdkBody> {
    let mut response = if local {
        old(encryption)
    } else {
        object(Some("after-version"), encryption)
    };
    response
        .headers_mut()
        .insert("etag", "source-etag".parse().unwrap());
    response
        .headers_mut()
        .insert("x-amz-version-id", "after-version".parse().unwrap());
    *response.body_mut() = SdkBody::from("abc");
    response
}
fn execution_reads(local: bool, encryption: &str) -> Vec<Response<SdkBody>> {
    let mut reads = review_reads(local, encryption);
    reads.extend([
        ack(Some("after-version")),
        written(local, encryption),
        acl("new-owner"),
        tags(
            Some("after-version"),
            if local {
                "destination-only-tag"
            } else {
                "native-only-tag-value"
            },
        ),
    ]);
    reads
}
async fn prepared(
    local: bool,
    encryption: &str,
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
        encryption == "sse_c",
        "https://job.test",
        "source-bucket",
        "destination-bucket",
        None,
        "src/",
        "dest/",
    );
    if let SourcePin::Local { path, .. } = &job.source {
        fs::write(Path::new(path).join("file"), "abc").unwrap();
    }
    let mut responses = vec![empty()];
    if !local {
        responses.extend([
            empty(),
            listing("src/", Some(("file", 3, "source-etag"))),
            payload("abc", "source-etag"),
        ]);
    }
    responses.extend([
        listing("dest/", Some(("file", 3, "old-etag"))),
        old(encryption),
        head(),
        old(encryption),
    ]);
    if !local {
        responses.push(object(Some("source-version"), ""));
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
            entry_ids: rows.iter().map(|row| row.id.clone()).collect(),
            policy: preview::Policy::ReplaceUnchanged,
            max_snapshot_bytes: 100,
            approved: true,
        },
        Arc::new(|_| {}),
    )
    .await
    .unwrap();
    assert_eq!(prepared.phase, "prepared", "{:?}", prepared.message);
    (root, paths, state, prepared, http)
}
async fn reviewed(state: &JobRunState, prepared: &RunStatus) -> RunStatus {
    review(
        state,
        ReviewRequest {
            run_id: prepared.run_id.clone(),
            preview_id: prepared.preview.as_ref().unwrap().id.clone(),
        },
    )
    .await
    .unwrap()
}
fn request(status: &RunStatus, acknowledged: bool) -> ExecuteRequest {
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
        acknowledge_replacements: acknowledged,
        acknowledge_multipart: false,
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
async fn checked_replacements_require_new_ack_and_preserve_local_destination_or_s3_source_attributes(
) {
    for local in [true, false] {
        for encryption in ["", "AES256", "sse_c"] {
            let mut tail = review_reads(local, encryption);
            tail.extend(execution_reads(local, encryption));
            let (_root, paths, state, prepared, http) = prepared(local, encryption, tail).await;
            assert!(prepared.preview.as_ref().unwrap().can_review_execution);
            let review = reviewed(&state, &prepared).await;
            assert_eq!(
                review.phase, "awaiting_execution_approval",
                "{:?}",
                review.message
            );
            assert_eq!(
                review.execution_review.as_ref().unwrap().replacement_files,
                1
            );
            assert!(start(&state, request(&review, false)).await.is_err());
            start(&state, request(&review, true)).await.unwrap();
            assert!(start(&state, request(&review, true)).await.is_err());
            let stopped = wait(&state, &review.run_id).await;
            let result = stopped.execution.unwrap();
            assert_eq!(result.status, Status::Succeeded, "{:?}", result.message);
            assert!(result.outcome_recorded);
            assert_eq!(result.counts.replacement_files, 1);
            assert_eq!(result.counts.attempted_replacements, 1);
            assert_eq!(result.counts.acknowledged_replacements, 1);
            assert_eq!(result.counts.verified_replacements, 1);
            assert_eq!(result.counts.uncertain_replacements, 0);
            let writes: Vec<_> = http
                .actual_requests()
                .filter(|request| request.method() == "PUT")
                .collect();
            assert_eq!(writes.len(), 1);
            assert_eq!(writes[0].headers().get("if-match"), Some("old-etag"));
            assert!(writes[0].headers().get("if-none-match").is_none());
            assert_eq!(
                writes[0].headers().get("x-amz-meta-preserved"),
                Some(if local {
                    "destination-only-metadata"
                } else {
                    "native-only-metadata-value"
                })
            );
            assert_eq!(
                writes[0].headers().get("content-type"),
                Some(if local {
                    "application/json"
                } else {
                    "text/plain; charset=utf-8"
                })
            );
            assert_eq!(
                writes[0].headers().get("x-amz-server-side-encryption"),
                if encryption == "AES256" {
                    Some("AES256")
                } else {
                    None
                }
            );
            assert!(writes[0]
                .headers()
                .get("x-amz-tagging")
                .unwrap()
                .contains(if local {
                    "destination-only-tag"
                } else {
                    "native-only-tag-value"
                }));
            let history = state.history(&paths).unwrap();
            assert_eq!(
                history.records[0].policy,
                Some(preview::Policy::ReplaceUnchanged)
            );
            assert_eq!(
                history.records[0]
                    .execution
                    .as_ref()
                    .unwrap()
                    .verified_replacements,
                1
            );
            let raw =
                fs::read_to_string(paths.store.parent().unwrap().join("job-runs/journal.json"))
                    .unwrap();
            assert!(!raw.contains("old-etag"));
            assert!(!raw.contains("destination-only-metadata"));
            assert!(!raw.contains("before-version"));
        }
    }
}

#[tokio::test]
async fn same_etag_destination_payload_version_header_tag_or_owner_drift_prevents_replacement() {
    for mode in 0..5 {
        let mut tail = review_reads(true, "");
        let mut current = old("");
        if mode == 0 {
            *current.body_mut() = SdkBody::from("new");
        }
        if mode == 1 {
            current
                .headers_mut()
                .insert("x-amz-version-id", "concurrent-version".parse().unwrap());
        }
        if mode == 2 {
            current
                .headers_mut()
                .insert("cache-control", "changed".parse().unwrap());
        }
        tail.push(current);
        if mode >= 3 {
            tail.extend([
                unlocked(),
                acl(if mode == 3 {
                    "different-owner"
                } else {
                    "old-owner"
                }),
                tags(
                    Some("before-version"),
                    if mode == 4 {
                        "different-tag"
                    } else {
                        "destination-only-tag"
                    },
                ),
            ]);
        }
        let (_root, _paths, state, prepared, http) = prepared(true, "", tail).await;
        let review = reviewed(&state, &prepared).await;
        start(&state, request(&review, true)).await.unwrap();
        let result = wait(&state, &review.run_id).await.execution.unwrap();
        assert_eq!(result.status, Status::Failed);
        assert_eq!(result.counts.attempted_replacements, 0);
        assert_eq!(
            http.actual_requests()
                .filter(|r| r.method() == "PUT")
                .count(),
            0
        );
    }
}

#[tokio::test]
async fn transmitted_replacement_conflict_unsupported_and_verification_failure_are_uncertain_without_fallback(
) {
    for mode in 0..3 {
        let mut tail = review_reads(true, "");
        tail.extend(review_reads(true, ""));
        if mode < 2 {
            let (status, code) = if mode == 0 {
                (412, "PreconditionFailed")
            } else {
                (501, "NotImplemented")
            };
            tail.push(
                Response::builder()
                    .status(status)
                    .body(SdkBody::from(format!("<Error><Code>{code}</Code></Error>")))
                    .unwrap(),
            );
        } else {
            tail.push(ack(Some("after-version")));
            let mut bad = written(true, "");
            *bad.body_mut() = SdkBody::from("bad");
            tail.push(bad);
        }
        let (_root, paths, state, prepared, http) = prepared(true, "", tail).await;
        let review = reviewed(&state, &prepared).await;
        start(&state, request(&review, true)).await.unwrap();
        let result = wait(&state, &review.run_id).await.execution.unwrap();
        assert_eq!(result.status, Status::Uncertain);
        assert!(result.outcome_recorded);
        assert_eq!(result.counts.uncertain_replacements, 1);
        assert_eq!(result.counts.verified_replacements, 0);
        assert_eq!(
            http.actual_requests()
                .filter(|r| r.method() == "PUT")
                .count(),
            1
        );
        assert!(!state.history(&paths).unwrap().pauses.is_empty());
    }
}

#[tokio::test]
async fn unsupported_destination_retention_acl_or_encryption_never_receives_a_replacement() {
    for unsupported in ["kms", "glacier", "client", "gzip"] {
        let (_root, _paths, state, prepared, http) = prepared(true, unsupported, vec![]).await;
        assert!(!prepared.preview.as_ref().unwrap().can_review_execution);
        assert!(review(
            &state,
            ReviewRequest {
                run_id: prepared.run_id.clone(),
                preview_id: prepared.preview.unwrap().id
            }
        )
        .await
        .is_err());
        assert_eq!(
            http.actual_requests()
                .filter(|r| r.method() == "PUT")
                .count(),
            0
        );
    }
    for mode in 0..3 {
        let mut tail = vec![old("")];
        tail.push(if mode == 0 { empty() } else { unlocked() });
        if mode == 1 {
            tail.push(
                Response::builder()
                    .status(403)
                    .body(SdkBody::from("<Error><Code>AccessDenied</Code></Error>"))
                    .unwrap(),
            );
        }
        if mode == 2 {
            tail.push(Response::builder().status(200).body(SdkBody::from("<AccessControlPolicy><Owner><ID>old-owner</ID></Owner><AccessControlList/></AccessControlPolicy>")).unwrap());
        }
        let (_root, _paths, state, prepared, http) = prepared(true, "", tail).await;
        let result = reviewed(&state, &prepared).await;
        assert_eq!(result.phase, "stopped");
        assert!(result.execution_review.is_none());
        assert_eq!(
            http.actual_requests()
                .filter(|r| r.method() == "PUT")
                .count(),
            0
        );
    }
}

#[tokio::test]
async fn scoped_replacement_writer_rejects_weaker_or_wrong_conditions_before_transmission() {
    let (_root, _paths, state, prepared, http) =
        prepared(true, "", vec![ack(Some("after-version"))]).await;
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
    let (client, _) = writer_for(
        binding.clone(),
        "dest/file".into(),
        Arc::new(AtomicBool::new(false)),
        None,
        Arc::new(ScanBudget::with_limits(binding.limits)),
        crate::s3::writes::DestinationExpectation::Match {
            etag: "old-etag".into(),
        },
    )
    .unwrap();
    let before = http.actual_requests().count();
    assert!(client
        .put_object()
        .bucket("destination-bucket")
        .key("dest/file")
        .if_none_match("*")
        .send()
        .await
        .is_err());
    assert!(client
        .put_object()
        .bucket("destination-bucket")
        .key("dest/file")
        .if_match("wrong-etag")
        .send()
        .await
        .is_err());
    assert!(client
        .put_object()
        .bucket("destination-bucket")
        .key("dest/file")
        .if_match("old-etag")
        .if_none_match("*")
        .send()
        .await
        .is_err());
    assert_eq!(http.actual_requests().count(), before);
    client
        .put_object()
        .bucket("destination-bucket")
        .key("dest/file")
        .if_match("old-etag")
        .send()
        .await
        .unwrap();
    assert!(client
        .put_object()
        .bucket("destination-bucket")
        .key("dest/file")
        .if_match("old-etag")
        .send()
        .await
        .is_err());
    assert_eq!(http.actual_requests().count(), before + 1);
    state.release(&prepared.run_id).unwrap();
}

#[tokio::test]
async fn multiple_replacements_keep_confirmed_subcounts_separate_from_partial_and_uncertain_writes()
{
    for unknown in [false, true] {
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
        let listing=Response::builder().status(200).body(SdkBody::from("<ListBucketResult><IsTruncated>false</IsTruncated><Contents><Key>dest/a</Key><Size>3</Size><ETag>old-etag</ETag></Contents><Contents><Key>dest/b</Key><Size>3</Size><ETag>old-etag</ETag></Contents></ListBucketResult>")).unwrap();
        let mut responses = vec![
            empty(),
            listing,
            old(""),
            old(""),
            head(),
            old(""),
            head(),
            old(""),
        ];
        responses.extend(review_reads(true, ""));
        responses.extend(review_reads(true, ""));
        responses.extend(execution_reads(true, ""));
        if unknown {
            responses.extend(review_reads(true, ""));
            responses.push(
                Response::builder()
                    .status(503)
                    .body(SdkBody::from("<Error><Code>SlowDown</Code></Error>"))
                    .unwrap(),
            );
        } else {
            let mut changed = old("");
            *changed.body_mut() = SdkBody::from("new");
            responses.push(changed);
        }
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
                entry_ids: rows.iter().map(|row| row.id.clone()).collect(),
                policy: preview::Policy::ReplaceUnchanged,
                max_snapshot_bytes: 100,
                approved: true,
            },
            Arc::new(|_| {}),
        )
        .await
        .unwrap();
        let reviewed = reviewed(&state, &prepared).await;
        start(&state, request(&reviewed, true)).await.unwrap();
        let result = wait(&state, &reviewed.run_id).await.execution.unwrap();
        assert_eq!(
            result.status,
            if unknown {
                Status::Uncertain
            } else {
                Status::Partial
            }
        );
        assert_eq!(result.counts.replacement_files, 2);
        assert_eq!(result.counts.verified_replacements, 1);
        assert_eq!(result.counts.uncertain_replacements, u64::from(unknown));
        assert_eq!(
            result.counts.attempted_replacements,
            if unknown { 2 } else { 1 }
        );
        assert!(result.outcome_recorded);
        assert_eq!(
            http.actual_requests()
                .filter(|r| r.method() == "PUT")
                .count(),
            if unknown { 2 } else { 1 }
        );
        assert!(!state.history(&paths).unwrap().pauses.is_empty());
    }
}

struct ReplacementIntentObserver {
    paths: crate::jobs::Paths,
    backup: Arc<std::sync::Mutex<Option<Vec<u8>>>>,
}
impl std::fmt::Debug for ReplacementIntentObserver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ReplacementIntentObserver([private])")
    }
}
impl Intercept for ReplacementIntentObserver {
    fn name(&self) -> &'static str {
        "ReplacementIntentObserver"
    }
    fn modify_before_transmit(
        &self,
        context: &mut BeforeTransmitInterceptorContextMut<'_>,
        _: &RuntimeComponents,
        cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        if cfg.load::<Metadata>().map(Metadata::name) == Some("PutObject") {
            let report = crate::job_runs::journal::read(&self.paths).unwrap();
            let record = &report.records[0];
            assert_eq!(record.policy, Some(preview::Policy::ReplaceUnchanged));
            assert_eq!(record.status, Status::Executing);
            let counts = record.execution.as_ref().unwrap();
            assert_eq!(counts.replacement_files, 1);
            assert_eq!(counts.attempted_replacements, 1);
            assert_eq!(counts.acknowledged_replacements, 0);
            assert_eq!(
                context.request().headers().get("if-match"),
                Some("old-etag")
            );
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
        }
        Ok(())
    }
    fn read_after_deserialization(
        &self,
        _:&aws_smithy_runtime_api::client::interceptors::context::AfterDeserializationInterceptorContextRef<'_>,
        _: &RuntimeComponents,
        cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        if cfg.load::<Metadata>().map(Metadata::name) == Some("PutObject") {
            fs::write(
                self.paths
                    .store
                    .parent()
                    .unwrap()
                    .join("job-runs/journal.json"),
                "PRIVATE_REPLACEMENT_JOURNAL_CORRUPTION",
            )
            .unwrap();
        }
        Ok(())
    }
}
#[tokio::test]
async fn replacement_intent_is_durable_before_put_and_postwrite_journal_failure_is_not_success() {
    let mut tail = review_reads(true, "");
    tail.extend(review_reads(true, ""));
    tail.push(ack(Some("after-version")));
    let (_root, paths, state, prepared, http) = prepared(true, "", tail).await;
    let backup = Arc::new(std::sync::Mutex::new(None));
    {
        let mut inner = state.0.lock().unwrap();
        let binding = Arc::get_mut(inner.slot.as_mut().unwrap().binding.as_mut().unwrap()).unwrap();
        binding.destination.client = Client::from_conf(
            binding
                .destination
                .client
                .config()
                .to_builder()
                .interceptor(ReplacementIntentObserver {
                    paths: paths.clone(),
                    backup: backup.clone(),
                })
                .build(),
        );
    }
    let reviewed = reviewed(&state, &prepared).await;
    start(&state, request(&reviewed, true)).await.unwrap();
    let result = wait(&state, &reviewed.run_id).await.execution.unwrap();
    assert_eq!(result.status, Status::Uncertain);
    assert!(!result.outcome_recorded);
    assert_eq!(result.counts.acknowledged_replacements, 1);
    assert_eq!(result.counts.uncertain_replacements, 1);
    assert!(!result
        .message
        .unwrap()
        .contains("PRIVATE_REPLACEMENT_JOURNAL_CORRUPTION"));
    assert_eq!(
        http.actual_requests()
            .filter(|r| r.method() == "PUT")
            .count(),
        1
    );
    fs::write(
        paths.store.parent().unwrap().join("job-runs/journal.json"),
        backup.lock().unwrap().as_ref().unwrap(),
    )
    .unwrap();
    let recovered = state.history(&paths).unwrap();
    assert_eq!(recovered.records[0].status, Status::Interrupted);
    assert_eq!(
        recovered.records[0]
            .execution
            .as_ref()
            .unwrap()
            .uncertain_replacements,
        1
    );
    assert!(recovered.in_memory_storage_fault);
    assert!(!recovered.pauses.is_empty());
}

#[tokio::test]
#[ignore = "requires an explicitly configured disposable loopback MinIO fixture"]
async fn loopback_minio_checked_local_and_s3_replacements_preserve_the_approved_attribute_policy() {
    for local in [true, false] {
        for versioned in [false, true] {
            replacement_fixture(local, versioned, false, true).await;
        }
    }
}
pub(super) async fn replacement_fixture(local: bool, versioned: bool, large: bool, replace: bool) {
    let endpoint = std::env::var("FURMAN_JOB_TEST_ENDPOINT")
        .expect("Explicit disposable loopback endpoint required");
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
                "loopback-replacement-fixture",
            ))
            .request_checksum_calculation(
                aws_sdk_s3::config::RequestChecksumCalculation::WhenRequired,
            )
            .build(),
    );
    let source = format!("furman-replacement-source-{}", fresh_id());
    let destination = format!("furman-replacement-dest-{}", fresh_id());
    let content = if large {
        vec![b'x'; 8 * 1024 * 1024 + 19]
    } else {
        b"abc".to_vec()
    };
    let result: Result<(), FmError> = async {
        for bucket in [&source, &destination] {
            setup
                .create_bucket()
                .bucket(bucket)
                .send()
                .await
                .map_err(|e| job_sdk_error("Fixture create bucket", &e))?;
            if versioned {
                setup
                    .put_bucket_versioning()
                    .bucket(bucket)
                    .versioning_configuration(
                        aws_sdk_s3::types::VersioningConfiguration::builder()
                            .status(aws_sdk_s3::types::BucketVersioningStatus::Enabled)
                            .build(),
                    )
                    .send()
                    .await
                    .map_err(|e| job_sdk_error("Fixture versioning", &e))?;
            }
        }
        if replace {
            setup
                .put_object()
                .bucket(&destination)
                .key("dest/file")
                .body(aws_sdk_s3::primitives::ByteStream::from(if large {
                    vec![b'o'; 6 * 1024 * 1024]
                } else {
                    b"old".to_vec()
                }))
                .content_type("application/json")
                .cache_control("private, max-age=1")
                .metadata("preserved", "destination-only-metadata")
                .tagging("stage=destination-only-tag")
                .send()
                .await
                .map_err(|e| job_sdk_error("Fixture seed destination", &e))?;
        }
        let source_version = if !local {
            setup
                .put_object()
                .bucket(&source)
                .key("src/file")
                .body(aws_sdk_s3::primitives::ByteStream::from(content.clone()))
                .content_type("text/plain")
                .cache_control("max-age=17")
                .metadata("preserved", "source-only-metadata")
                .tagging("stage=source-only-tag")
                .send()
                .await
                .map_err(|e| job_sdk_error("Fixture seed source", &e))?
                .version_id()
                .map(str::to_owned)
        } else {
            None
        };
        let (_root, paths, job) = fixture_with_names(
            local,
            false,
            &endpoint,
            &source,
            &destination,
            Some(&access),
            "src/",
            "dest/",
        );
        if let SourcePin::Local { path, .. } = &job.source {
            fs::write(Path::new(path).join("file"), &content).unwrap();
        }
        let state = JobRunState::default();
        let secret = secret.clone();
        let bound = crate::job_runs::bind_with(
            &state,
            paths.clone(),
            fresh_id(),
            job.id,
            job.revision,
            true,
            Arc::new(move |_| Ok(Some(secret.to_string()))),
            Arc::new(crate::job_runs::client),
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
                entry_ids: rows.iter().map(|row| row.id.clone()).collect(),
                policy: if replace {
                    preview::Policy::ReplaceUnchanged
                } else {
                    preview::Policy::CreateOnly
                },
                max_snapshot_bytes: content.len() as u64,
                approved: true,
            },
            Arc::new(|_| {}),
        )
        .await?;
        let reviewed = reviewed(&state, &prepared).await;
        if reviewed.phase != "awaiting_execution_approval" {
            return Err(error(
                reviewed
                    .message
                    .as_deref()
                    .unwrap_or("Fixture review failed"),
            ));
        }
        if replace && start(&state, request(&reviewed, false)).await.is_ok() {
            return Err(error(
                "Fixture replacement accepted a create-only acknowledgement",
            ));
        }
        if large && start(&state, request(&reviewed, true)).await.is_ok() {
            return Err(error(
                "Fixture accepted multipart without its separate acknowledgement",
            ));
        }
        let mut approved = request(&reviewed, true);
        approved.acknowledge_multipart = large;
        start(&state, approved).await?;
        let stopped = wait(&state, &reviewed.run_id).await;
        let execution = stopped
            .execution
            .ok_or_else(|| error("Fixture execution outcome missing"))?;
        if execution.status != Status::Succeeded
            || !execution.outcome_recorded
            || execution.counts.verified_replacements != u64::from(replace)
            || execution.counts.multipart_files != u64::from(large)
        {
            return Err(error(
                execution
                    .message
                    .as_deref()
                    .unwrap_or("Fixture replacement not durably verified"),
            ));
        }
        let read = setup
            .get_object()
            .bucket(&destination)
            .key("dest/file")
            .send()
            .await
            .map_err(|e| job_sdk_error("Fixture replacement readback", &e))?;
        if (!local || replace)
            && (read
                .metadata()
                .and_then(|m| m.get("preserved"))
                .map(String::as_str)
                != Some(if local {
                    "destination-only-metadata"
                } else {
                    "source-only-metadata"
                })
                || read.content_type()
                    != Some(if local {
                        "application/json"
                    } else {
                        "text/plain"
                    })
                || read.cache_control()
                    != Some(if local {
                        "private, max-age=1"
                    } else {
                        "max-age=17"
                    }))
        {
            return Err(error("Fixture replacement lost approved attributes"));
        }
        let bytes = read
            .body
            .collect()
            .await
            .map_err(|_| error("Fixture body failed"))?
            .into_bytes();
        if bytes.as_ref() != content {
            return Err(error("Fixture replacement payload mismatch"));
        }
        let tags = setup
            .get_object_tagging()
            .bucket(&destination)
            .key("dest/file")
            .send()
            .await
            .map_err(|e| job_sdk_error("Fixture tag readback", &e))?;
        if (!local || replace)
            && (tags.tag_set().len() != 1
                || tags.tag_set()[0].value()
                    != if local {
                        "destination-only-tag"
                    } else {
                        "source-only-tag"
                    })
        {
            return Err(error("Fixture replacement lost approved tags"));
        }
        if !local {
            let source_read = setup
                .get_object()
                .bucket(&source)
                .key("src/file")
                .send()
                .await
                .map_err(|e| job_sdk_error("Fixture source verification", &e))?;
            if source_read.version_id() != source_version.as_deref() {
                return Err(error("Replacement changed its source version"));
            }
        }
        if state.history(&paths)?.records[0]
            .execution
            .as_ref()
            .is_none_or(|counts| counts.verified_replacements != u64::from(replace))
        {
            return Err(error(
                "Replacement journal did not retain verified subcounts",
            ));
        }
        if !state.history(&paths)?.pending_uploads.is_empty() {
            return Err(error("Fixture left unresolved multipart evidence"));
        }
        let pending = setup
            .list_multipart_uploads()
            .bucket(&destination)
            .send()
            .await
            .map_err(|e| job_sdk_error("Fixture multipart inventory", &e))?;
        if !pending.uploads().is_empty() {
            return Err(error("Fixture left staged provider multipart resources"));
        }
        Ok(())
    }
    .await;
    for bucket in [&source, &destination] {
        if let Ok(pending) = setup.list_multipart_uploads().bucket(bucket).send().await {
            for upload in pending.uploads() {
                if let (Some(key), Some(id)) = (upload.key(), upload.upload_id()) {
                    let _ = setup
                        .abort_multipart_upload()
                        .bucket(bucket)
                        .key(key)
                        .upload_id(id)
                        .send()
                        .await;
                }
            }
        }
        if let Ok(versions) = setup.list_object_versions().bucket(bucket).send().await {
            for version in versions.versions() {
                if let (Some(key), Some(id)) = (version.key(), version.version_id()) {
                    let _ = setup
                        .delete_object()
                        .bucket(bucket)
                        .key(key)
                        .version_id(id)
                        .send()
                        .await;
                }
            }
        }
        for key in ["src/file", "dest/file"] {
            let _ = setup.delete_object().bucket(bucket).key(key).send().await;
        }
        if let Ok(versions) = setup.list_object_versions().bucket(bucket).send().await {
            for marker in versions.delete_markers() {
                if let (Some(key), Some(id)) = (marker.key(), marker.version_id()) {
                    let _ = setup
                        .delete_object()
                        .bucket(bucket)
                        .key(key)
                        .version_id(id)
                        .send()
                        .await;
                }
            }
        }
        let _ = setup.delete_bucket().bucket(bucket).send().await;
    }
    result.unwrap();
}
