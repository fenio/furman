use super::*;
use aws_smithy_http_client::test_util::{ReplayEvent, StaticReplayClient};
use aws_smithy_types::body::SdkBody;
use base64::Engine;
use std::{collections::BTreeMap, fs, path::Path, sync::atomic::AtomicU64};
use tauri::http::{Request, Response};

pub(super) fn fixture(local: bool, sse: bool) -> (tempfile::TempDir, Paths, Job) {
    fixture_at(local, sse, "https://job.test")
}
fn fixture_at(local: bool, sse: bool, endpoint: &str) -> (tempfile::TempDir, Paths, Job) {
    fixture_with_names(
        local,
        sse,
        endpoint,
        "source-bucket",
        "destination-bucket",
        None,
        "src//",
        "dest//",
    )
}
fn fixture_with_names(
    local: bool,
    sse: bool,
    endpoint: &str,
    source_bucket: &str,
    destination_bucket: &str,
    access: Option<&str>,
    source_prefix: &str,
    destination_prefix: &str,
) -> (tempfile::TempDir, Paths, Job) {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().canonicalize().unwrap();
    let paths = Paths {
        config: directory.join("config.json"),
        store: directory.join("saved-jobs.json"),
    };
    let profile = |id: &str, bucket: &str| serde_json::json!({"id":id,"name":id,"type":"s3","credentialType":"keychain","accessKeyId":access.map(str::to_owned).unwrap_or_else(||format!("FAKE_{id}")),"bucket":bucket,"region":"us-east-1","endpoint":endpoint,"provider":"linode","sseCustomerKeyEnabled":sse});
    fs::write(&paths.config, serde_json::to_vec(&serde_json::json!({"connections":[profile("source",source_bucket),profile("destination",destination_bucket)]})).unwrap()).unwrap();
    let source = if local {
        let source = directory.join("local-source");
        fs::create_dir(&source).unwrap();
        jobs::SourceRequest::Local {
            path: source.to_str().unwrap().into(),
        }
    } else {
        jobs::SourceRequest::S3 {
            profile_id: "source".into(),
            prefix: source_prefix.into(),
        }
    };
    let reviewed = jobs::list(&paths).unwrap();
    let saved = jobs::save(
        &paths,
        jobs::SaveRequest {
            expected_revision: reviewed.revision,
            expected_configuration_revision: reviewed.configuration_revision,
            id: None,
            name: "Native comparison".into(),
            source,
            destination_profile_id: "destination".into(),
            destination_prefix: destination_prefix.into(),
            exclude_patterns: vec![],
            acknowledged: true,
        },
    )
    .unwrap();
    (root, paths, saved.jobs[0].definition.clone())
}
pub(super) fn replay(responses: Vec<Response<SdkBody>>) -> StaticReplayClient {
    StaticReplayClient::new(
        responses
            .into_iter()
            .map(|response| {
                ReplayEvent::new(
                    Request::builder()
                        .uri("https://job.test/")
                        .body(SdkBody::empty())
                        .unwrap(),
                    response,
                )
            })
            .collect(),
    )
}
pub(super) fn empty() -> Response<SdkBody> {
    Response::builder()
        .status(200)
        .body(SdkBody::empty())
        .unwrap()
}
pub(super) fn listing(prefix: &str, value: Option<(&str, u64, &str)>) -> Response<SdkBody> {
    let contents = value.map(|(suffix,size,etag)| format!("<Contents><Key>{prefix}{suffix}</Key><Size>{size}</Size><ETag>{etag}</ETag></Contents>")).unwrap_or_default();
    Response::builder()
        .status(200)
        .body(SdkBody::from(format!(
            "<ListBucketResult><IsTruncated>false</IsTruncated>{contents}</ListBucketResult>"
        )))
        .unwrap()
}
pub(super) fn payload(bytes: &str, etag: &str) -> Response<SdkBody> {
    Response::builder()
        .status(200)
        .header("content-length", bytes.len())
        .header("etag", etag)
        .body(SdkBody::from(bytes.to_owned()))
        .unwrap()
}
fn key(id: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(if id.starts_with("source") {
        [1u8; 32]
    } else {
        [2u8; 32]
    })
}
pub(super) fn dependencies(
    http: StaticReplayClient,
) -> (KeyReader, EndpointFactory, Arc<Mutex<Vec<String>>>) {
    let reads = Arc::new(Mutex::new(vec![]));
    let recorded = reads.clone();
    let reader: KeyReader = Arc::new(move |id| {
        recorded.lock().unwrap().push(id.clone());
        Ok(Some(if id.ends_with(":sse-c") {
            key(&id)
        } else {
            "FAKE_SECRET".into()
        }))
    });
    let factory: EndpointFactory = Arc::new(move |auth| {
        let namespace = namespace(&auth.pin.configuration)?;
        let client = Client::from_conf(
            aws_sdk_s3::Config::builder()
                .behavior_version_latest()
                .region(Region::new(auth.pin.configuration.region.clone()))
                .endpoint_url(namespace.clone())
                .force_path_style(true)
                .credentials_provider(Credentials::new(
                    auth.access.as_str(),
                    auth.secret.as_str(),
                    None,
                    None,
                    "test",
                ))
                .retry_config(RetryConfig::standard().with_max_attempts(2))
                .http_client(http.clone())
                .interceptor(ReadNamespaceGuard {
                    origin: url::Url::parse(&namespace).unwrap(),
                })
                .build(),
        );
        let client = crate::s3::compat::configure_conditions(
            client,
            Some(&auth.pin.configuration.provider),
            Some(auth.access.as_str()),
        );
        let client = crate::s3::sse_c::configure(client, auth.sse.as_deref().map(String::as_str))?;
        Ok(Endpoint {
            client,
            instance: fresh_id(),
            namespace,
            bucket: auth.pin.configuration.bucket,
        })
    });
    (reader, factory, reads)
}
pub(super) async fn bind_fixture(
    state: &JobRunState,
    paths: &Paths,
    job: &Job,
    http: &StaticReplayClient,
) -> RunStatus {
    let (reader, factory, _) = dependencies(http.clone());
    bind_with(
        state,
        paths.clone(),
        fresh_id(),
        job.id.clone(),
        job.revision.clone(),
        true,
        reader,
        factory,
    )
    .await
    .unwrap()
}

#[test]
fn default_namespaces_do_not_adopt_custom_endpoint_or_unknown_partition_fallbacks() {
    let mut config = ProfileConfiguration {
        bucket: "bucket".into(),
        region: "us-east-1".into(),
        endpoint: None,
        provider: "aws".into(),
        proxy_endpoint: None,
        proxy_auth: false,
        sse_customer_key_enabled: false,
        acceleration: false,
    };
    assert_eq!(
        namespace(&config).unwrap(),
        "https://s3.us-east-1.amazonaws.com/"
    );
    config.region = "cn-north-1".into();
    assert_eq!(
        namespace(&config).unwrap(),
        "https://s3.cn-north-1.amazonaws.com.cn/"
    );
    config.region = "us-iso-east-1".into();
    assert!(namespace(&config).is_err());
    config.endpoint = Some("https://custom.example/".into());
    assert_eq!(namespace(&config).unwrap(), "https://custom.example/");
    config.acceleration = true;
    assert!(namespace(&config).is_err());
}

#[test]
fn operation_ids_are_canonicalized_before_duplicate_and_early_cancel_checks() {
    let state = JobRunState::default();
    let op = fresh_id();
    let (_, _, guard) = state.begin(&op).unwrap();
    drop(guard);
    assert!(state.begin(&op.to_uppercase()).is_err());
    let next = fresh_id();
    state.release(&format!("urn:uuid:{next}")).unwrap();
    assert!(state.begin(&next).is_err());
}

#[test]
fn effective_origin_aliases_cannot_bypass_folder_overlap_checks() {
    let (_root, _paths, mut job) = fixture(false, false);
    let SourcePin::S3 { pin } = &mut job.source else {
        panic!()
    };
    pin.configuration.bucket = job.destination.configuration.bucket.clone();
    pin.configuration.endpoint = None;
    pin.prefix = "shared/".into();
    job.destination.configuration.endpoint = Some("https://s3.us-east-1.amazonaws.com/".into());
    job.destination.prefix = "shared/child/".into();
    let source = namespace(&pin.configuration).unwrap();
    let destination = namespace(&job.destination.configuration).unwrap();
    assert!(reject_resolved_overlap(&job, &source, &destination).is_err());
    job.destination.prefix = "disjoint/".into();
    assert!(reject_resolved_overlap(&job, &source, &destination).is_ok());
}

#[tokio::test]
async fn changed_profile_configuration_fails_before_keychain_or_bucket_access() {
    let (_root, paths, job) = fixture(false, false);
    let mut config: serde_json::Value =
        serde_json::from_slice(&fs::read(&paths.config).unwrap()).unwrap();
    config["connections"][1]["endpoint"] = "https://changed.test".into();
    fs::write(&paths.config, serde_json::to_vec(&config).unwrap()).unwrap();
    let http = replay(vec![]);
    let state = JobRunState::default();
    let (reader, factory, reads) = dependencies(http.clone());
    assert!(bind_with(
        &state,
        paths,
        fresh_id(),
        job.id,
        job.revision,
        true,
        reader,
        factory
    )
    .await
    .is_err());
    assert!(reads.lock().unwrap().is_empty());
    assert_eq!(http.actual_requests().count(), 0);
}

#[tokio::test]
async fn native_binding_and_complete_manifests_keep_private_clients_hashes_and_opaque_etags() {
    let (_root, paths, job) = fixture(false, true);
    let http = replay(vec![
        empty(),
        empty(),
        listing("src//", Some(("nested/file", 3, "source-opaque"))),
        payload("abc", "source-opaque"),
        listing("dest//", Some(("nested/file", 3, "destination-opaque"))),
        payload("abd", "destination-opaque"),
    ]);
    let state = JobRunState::default();
    let (reader, factory, reads) = dependencies(http.clone());
    let bound = bind_with(
        &state,
        paths,
        fresh_id(),
        job.id,
        job.revision,
        true,
        reader,
        factory,
    )
    .await
    .unwrap();
    assert_eq!(bound.phase, "bound");
    assert_ne!(bound.source_instance, bound.destination_instance);
    assert!(!bound.execution_available);
    assert_eq!(
        *reads.lock().unwrap(),
        ["destination", "destination:sse-c", "source", "source:sse-c"]
    );
    let result = scan(&state, &bound.run_id, Arc::new(|_| {})).await.unwrap();
    assert_eq!(result.phase, "complete");
    assert!(matches!(result.outcome, Some(ScanOutcome::Complete)));
    assert_eq!(result.progress.s3_bytes_read, 6);
    assert_eq!(result.progress.requests, 4);
    let manifest_id = result.manifest_id.unwrap();
    let rows = state.page(&bound.run_id, &manifest_id, 0).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].relative_path, "nested/file");
    assert_eq!(rows[0].status, "modified");
    assert!(state.page(&bound.run_id, "wrong-manifest", 0).is_err());
    assert!(state.page(&fresh_id(), &manifest_id, 0).is_err());
    assert!(scan(&state, &bound.run_id, Arc::new(|_| {})).await.is_err());
    let encoded = serde_json::to_string(&rows).unwrap();
    assert!(!encoded.contains("sha256"));
    assert!(!encoded.contains("opaque"));
    assert!(!encoded.contains("FAKE_SECRET"));
    let inner = state.0.lock().unwrap();
    let entry = &inner
        .slot
        .as_ref()
        .unwrap()
        .manifest
        .as_ref()
        .unwrap()
        .entries[0];
    assert_eq!(
        entry.source.as_ref().unwrap().etag.as_deref(),
        Some("source-opaque")
    );
    assert_ne!(entry.source.as_ref().unwrap().sha256, "source-opaque");
    drop(inner);
    let requests: Vec<_> = http.actual_requests().collect();
    assert_eq!(requests.len(), 6);
    assert!(requests
        .iter()
        .all(|request| matches!(request.method(), "GET" | "HEAD")));
    assert!(requests[2].uri().to_string().contains("src%2F%2F"));
    assert_eq!(
        requests[3].headers().get("if-match").unwrap(),
        "source-opaque"
    );
    assert_eq!(
        requests[5].headers().get("if-match").unwrap(),
        "destination-opaque"
    );
    assert_eq!(
        requests[3]
            .headers()
            .get("x-amz-server-side-encryption-customer-key")
            .unwrap(),
        key("source")
    );
    assert_eq!(
        requests[5]
            .headers()
            .get("x-amz-server-side-encryption-customer-key")
            .unwrap(),
        key("destination")
    );
    assert!(requests[0]
        .headers()
        .get("x-amz-server-side-encryption-customer-key")
        .is_none());
}

#[tokio::test]
async fn unapproved_missing_or_precancelled_binding_never_reads_keys_or_sends_requests() {
    let (_root, paths, job) = fixture(false, false);
    let state = JobRunState::default();
    let http = replay(vec![]);
    let (reader, factory, reads) = dependencies(http.clone());
    assert!(bind_with(
        &state,
        paths.clone(),
        fresh_id(),
        job.id.clone(),
        job.revision.clone(),
        false,
        reader.clone(),
        factory.clone()
    )
    .await
    .is_err());
    assert!(reads.lock().unwrap().is_empty());
    let op = fresh_id();
    state.release(&op).unwrap();
    assert!(bind_with(
        &state,
        paths.clone(),
        op,
        job.id.clone(),
        job.revision.clone(),
        true,
        reader,
        factory.clone()
    )
    .await
    .is_err());
    assert!(reads.lock().unwrap().is_empty());
    assert!(bind_with(
        &state,
        paths,
        fresh_id(),
        job.id,
        job.revision,
        true,
        Arc::new(|_| Ok(None)),
        factory
    )
    .await
    .is_err());
    assert_eq!(http.actual_requests().count(), 0);
    assert!(state
        .0
        .lock()
        .unwrap()
        .slot
        .as_ref()
        .unwrap()
        .manifest
        .is_none());
}

#[tokio::test]
async fn managed_identity_edits_expiry_and_limits_invalidate_manifest_handles() {
    let (_root, paths, job) = fixture(true, false);
    let http = replay(vec![empty(), listing("dest//", None)]);
    let state = JobRunState::default();
    let bound = bind_fixture(&state, &paths, &job, &http).await;
    let result = scan(&state, &bound.run_id, Arc::new(|_| {})).await.unwrap();
    let manifest = result.manifest_id.unwrap();
    jobs::secret_edit(&paths, "destination", || Ok(())).unwrap();
    let changed = state.status(&bound.run_id).unwrap();
    assert_eq!(changed.phase, "stopped");
    assert!(changed.manifest_id.is_none());
    assert!(state.page(&bound.run_id, &manifest, 0).is_err());
    let (_root, paths, job) = fixture(true, false);
    let http = replay(vec![empty()]);
    let state = JobRunState::default();
    let bound = bind_fixture(&state, &paths, &job, &http).await;
    {
        let mut inner = state.0.lock().unwrap();
        inner.slot.as_mut().unwrap().expires = Instant::now() - Duration::from_secs(1);
    }
    assert_eq!(state.status(&bound.run_id).unwrap().phase, "stopped");
    let (_root, paths, job) = fixture(true, false);
    let http = replay(vec![empty()]);
    let state = JobRunState::default();
    let bound = bind_fixture(&state, &paths, &job, &http).await;
    {
        let mut inner = state.0.lock().unwrap();
        Arc::get_mut(inner.slot.as_mut().unwrap().binding.as_mut().unwrap())
            .unwrap()
            .global_limits
            .max_objects += 1;
    }
    assert_eq!(state.status(&bound.run_id).unwrap().phase, "stopped");
}

#[tokio::test]
async fn run_lock_is_single_flight_across_states_and_released_after_cancel() {
    let (_root, paths, job) = fixture(true, false);
    let http = replay(vec![empty(), empty()]);
    let first = JobRunState::default();
    let second = JobRunState::default();
    let bound = bind_fixture(&first, &paths, &job, &http).await;
    let (reader, factory, reads) = dependencies(http.clone());
    assert!(bind_with(
        &second,
        paths.clone(),
        fresh_id(),
        job.id.clone(),
        job.revision.clone(),
        true,
        reader.clone(),
        factory.clone()
    )
    .await
    .is_err());
    assert!(reads.lock().unwrap().is_empty());
    first.release(&bound.run_id).unwrap();
    let next = bind_with(
        &second,
        paths,
        fresh_id(),
        job.id,
        job.revision,
        true,
        reader,
        factory,
    )
    .await
    .unwrap();
    assert_ne!(bound.run_id, next.run_id);
}

#[tokio::test]
async fn replaced_local_roots_are_not_reopened_for_hashing_and_cannot_publish_a_manifest() {
    let (_root, paths, job) = fixture(true, false);
    let SourcePin::Local { path, .. } = &job.source else {
        panic!()
    };
    fs::write(Path::new(path).join("file"), "abc").unwrap();
    let http = replay(vec![empty()]);
    let state = JobRunState::default();
    let bound = bind_fixture(&state, &paths, &job, &http).await;
    let root = {
        let inner = state.0.lock().unwrap();
        let BoundSource::Local(root) = &inner
            .slot
            .as_ref()
            .unwrap()
            .binding
            .as_ref()
            .unwrap()
            .source
        else {
            panic!()
        };
        root.clone()
    };
    fs::rename(path, format!("{path}-original")).unwrap();
    fs::create_dir(path).unwrap();
    fs::write(Path::new(path).join("file"), "bad").unwrap();
    let mut files = BTreeMap::new();
    collect::walk_local(
        &root.directory,
        "",
        &[],
        &AtomicBool::new(false),
        &ScanBudget::new(),
        &|_| {},
        &mut files,
        &AtomicU64::new(0),
    )
    .unwrap();
    use sha2::Digest;
    assert_eq!(
        files["file"].sha256,
        sha2::Sha256::digest(b"abc")
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    );
    assert!(scan(&state, &bound.run_id, Arc::new(|_| {}))
        .await
        .err()
        .unwrap()
        .to_string()
        .contains("fresh bound run"));
    assert!(state.status(&bound.run_id).unwrap().manifest_id.is_none());
    assert_eq!(http.actual_requests().count(), 1);
}

#[tokio::test]
async fn request_guard_blocks_writes_and_origin_changes_before_transmission() {
    let (_root, paths, job) = fixture(true, false);
    let http = replay(vec![empty()]);
    let state = JobRunState::default();
    let bound = bind_fixture(&state, &paths, &job, &http).await;
    let client = {
        state
            .0
            .lock()
            .unwrap()
            .slot
            .as_ref()
            .unwrap()
            .binding
            .as_ref()
            .unwrap()
            .destination
            .client
            .clone()
    };
    let failure = client
        .put_object()
        .bucket("destination-bucket")
        .key("forbidden")
        .body(aws_sdk_s3::primitives::ByteStream::from_static(
            b"never-send",
        ))
        .send()
        .await
        .err()
        .unwrap();
    assert!(job_sdk_error("Forbidden write", &failure)
        .to_string()
        .contains("JOB_READ_ONLY"));
    assert_eq!(http.actual_requests().count(), 1);
    let guard = ReadNamespaceGuard {
        origin: url::Url::parse("https://job.test/").unwrap(),
    };
    assert!(guard
        .validate("https://elsewhere.test/private?token=DO_NOT_EXPOSE")
        .err()
        .unwrap()
        .to_string()
        .contains("JOB_NAMESPACE_MISMATCH"));
    assert!(guard.validate("http://job.test/").is_err());
    assert!(guard.validate("https://job.test:444/").is_err());
    assert!(guard
        .validate("https://job.test:443/path?prefix=somewhere")
        .is_ok());
    state.release(&bound.run_id).unwrap();
}

#[tokio::test]
async fn conditional_hashing_rejects_changed_etags_incomplete_bodies_and_incomplete_listings() {
    for response in [
        payload("abc", "changed"),
        Response::builder()
            .status(200)
            .header("etag", "expected")
            .header("content-length", 3)
            .body(SdkBody::from("ab"))
            .unwrap(),
    ] {
        let http = replay(vec![response]);
        let config = ProfileConfiguration {
            bucket: "bucket".into(),
            region: "us-east-1".into(),
            endpoint: Some("https://job.test/".into()),
            provider: "linode".into(),
            proxy_endpoint: None,
            proxy_auth: false,
            sse_customer_key_enabled: false,
            acceleration: false,
        };
        let (_, factory, _) = dependencies(http);
        let endpoint = factory(Auth {
            pin: S3Pin {
                profile_id: "source".into(),
                generation: fresh_id(),
                configuration: config,
                prefix: "".into(),
            },
            access: Zeroizing::new("fake".into()),
            secret: Zeroizing::new("fake".into()),
            proxy_user: Zeroizing::new("".into()),
            proxy_secret: None,
            sse: None,
        })
        .unwrap();
        assert!(collect::hash_s3(
            &endpoint,
            "key",
            3,
            "expected",
            &AtomicBool::new(false),
            &ScanBudget::new(),
            &|_| {}
        )
        .await
        .is_err());
    }
    let (_root, paths, job) = fixture(true, false);
    let http = replay(vec![
        empty(),
        Response::builder()
            .status(200)
            .body(SdkBody::from(
                "<ListBucketResult><IsTruncated>true</IsTruncated></ListBucketResult>",
            ))
            .unwrap(),
    ]);
    let state = JobRunState::default();
    let bound = bind_fixture(&state, &paths, &job, &http).await;
    let result = scan(&state, &bound.run_id, Arc::new(|_| {})).await.unwrap();
    assert_eq!(result.phase, "stopped");
    assert!(result.manifest_id.is_none());
}

#[tokio::test]
async fn bounded_local_scans_preserve_partial_progress_but_never_complete_handles() {
    let (_root, paths, job) = fixture(true, false);
    let SourcePin::Local { path, .. } = &job.source else {
        panic!()
    };
    fs::write(Path::new(path).join("one"), "abc").unwrap();
    fs::write(Path::new(path).join("two"), "def").unwrap();
    let http = replay(vec![empty()]);
    let state = JobRunState::default();
    let bound = bind_fixture(&state, &paths, &job, &http).await;
    {
        let mut inner = state.0.lock().unwrap();
        Arc::get_mut(inner.slot.as_mut().unwrap().binding.as_mut().unwrap())
            .unwrap()
            .limits
            .max_objects = 1;
    }
    let result = scan(&state, &bound.run_id, Arc::new(|_| {})).await.unwrap();
    assert!(matches!(result.outcome, Some(ScanOutcome::Limited)));
    assert!(result.manifest_id.is_none());
    assert_eq!(result.progress.scanned, 1);
    assert_eq!(http.actual_requests().count(), 1);
    let (directory, _) = jobs::open_local_root(path).unwrap();
    let budget = ScanBudget::new();
    let mut files = BTreeMap::new();
    assert!(collect::walk_local(
        &directory,
        "",
        &[],
        &AtomicBool::new(true),
        &budget,
        &|_| {},
        &mut files,
        &AtomicU64::new(0)
    )
    .is_err());
    assert!(collect::walk_local(
        &directory,
        "",
        &[],
        &AtomicBool::new(false),
        &budget,
        &|_| {},
        &mut files,
        &AtomicU64::new(32 * 1024 * 1024)
    )
    .is_err());
}

#[test]
fn manifest_inputs_require_real_hashes_and_difference_caps_never_return_partial_success() {
    let file = collect::FileInfo {
        size: 1,
        sha256: "a".repeat(64),
        etag: Some("opaque".into()),
    };
    let mut files = BTreeMap::new();
    for index in 0..=MAX_ROWS {
        files.insert(format!("{index}"), file.clone());
    }
    assert!(matches!(
        collect::merge(
            files,
            BTreeMap::new(),
            &ScanBudget::new(),
            &AtomicBool::new(false)
        ),
        Err(FmError::ScanLimited(_))
    ));
    let bad = collect::FileInfo {
        size: 1,
        sha256: String::new(),
        etag: Some("opaque".into()),
    };
    assert!(collect::merge(
        BTreeMap::from([("same".into(), bad.clone())]),
        BTreeMap::from([("same".into(), bad)]),
        &ScanBudget::new(),
        &AtomicBool::new(false)
    )
    .is_err());
}

#[tokio::test]
async fn a_session_that_expires_at_completion_cannot_publish_a_manifest() {
    let (_root, paths, job) = fixture(true, false);
    let http = replay(vec![empty(), listing("dest//", None)]);
    let state = JobRunState::default();
    let bound = bind_fixture(&state, &paths, &job, &http).await;
    let progress_state = state.clone();
    let result = scan(
        &state,
        &bound.run_id,
        Arc::new(move |event| {
            if event.requests > 0 {
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
    assert!(result.is_err());
    let status = state.status(&bound.run_id).unwrap();
    assert_eq!(status.phase, "stopped");
    assert!(status.manifest_id.is_none());
}

#[tokio::test]
async fn read_guard_validates_actual_sdk_requests_not_only_the_configured_url() {
    let http = replay(vec![]);
    let sdk = Client::from_conf(
        aws_sdk_s3::Config::builder()
            .behavior_version_latest()
            .region(Region::new("us-east-1"))
            .endpoint_url("https://different.test")
            .force_path_style(true)
            .credentials_provider(Credentials::new("fake", "fake", None, None, "test"))
            .http_client(http.clone())
            .retry_config(RetryConfig::disabled())
            .interceptor(ReadNamespaceGuard {
                origin: url::Url::parse("https://job.test/").unwrap(),
            })
            .build(),
    );
    let failure = sdk
        .head_bucket()
        .bucket("bucket")
        .send()
        .await
        .err()
        .unwrap();
    assert!(job_sdk_error("HeadBucket", &failure)
        .to_string()
        .contains("JOB_NAMESPACE_MISMATCH"));
    assert_eq!(http.actual_requests().count(), 0);
}

#[tokio::test]
async fn native_sse_clients_retain_plain_object_fallback_without_weakening_conditional_get() {
    let (_root, paths, job) = fixture(true, true);
    let http = replay(vec![
        empty(),
        listing("dest//", Some(("plain", 3, "plain-etag"))),
        Response::builder()
            .status(400)
            .body(SdkBody::from("<Error><Code>InvalidRequest</Code></Error>"))
            .unwrap(),
        payload("abc", "plain-etag"),
    ]);
    let state = JobRunState::default();
    let bound = bind_fixture(&state, &paths, &job, &http).await;
    let result = scan(&state, &bound.run_id, Arc::new(|_| {})).await.unwrap();
    assert_eq!(result.phase, "complete");
    assert_eq!(result.summary.unwrap().destination_only, 1);
    let requests: Vec<_> = http.actual_requests().collect();
    assert_eq!(requests.len(), 4);
    assert!(requests[2]
        .headers()
        .get("x-amz-server-side-encryption-customer-key")
        .is_some());
    assert!(requests[3]
        .headers()
        .get("x-amz-server-side-encryption-customer-key")
        .is_none());
    assert_eq!(requests[3].headers().get("if-match").unwrap(), "plain-etag");
}

#[tokio::test]
async fn cancellation_during_native_keychain_access_cannot_publish_bound_clients_or_send_head() {
    let (_root, paths, job) = fixture(true, false);
    let http = replay(vec![]);
    let state = JobRunState::default();
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let started = Mutex::new(Some(started_tx));
    let (finish_tx, finish_rx) = std::sync::mpsc::channel();
    let finish = Mutex::new(finish_rx);
    let reader: KeyReader = Arc::new(move |_| {
        if let Some(signal) = started.lock().unwrap().take() {
            let _ = signal.send(());
        }
        finish.lock().unwrap().recv().unwrap();
        Ok(Some("SECRET_NEVER_EMITTED".into()))
    });
    let (_, factory, _) = dependencies(http.clone());
    let op = fresh_id();
    let task_state = state.clone();
    let task_op = op.clone();
    let task = tokio::spawn(async move {
        bind_with(
            &task_state,
            paths,
            task_op,
            job.id,
            job.revision,
            true,
            reader,
            factory,
        )
        .await
    });
    started_rx.await.unwrap();
    state.release(&op).unwrap();
    finish_tx.send(()).unwrap();
    let failure = task.await.unwrap().err().unwrap();
    assert!(matches!(failure, FmError::ScanCancelled));
    assert_eq!(http.actual_requests().count(), 0);
    assert!(state
        .0
        .lock()
        .unwrap()
        .slot
        .as_ref()
        .unwrap()
        .binding
        .is_none());
}

#[tokio::test]
async fn native_keychain_failures_are_safe_and_never_fall_back_to_environment_credentials() {
    let (_root, paths, job) = fixture(true, false);
    let http = replay(vec![]);
    let state = JobRunState::default();
    let (_, factory, _) = dependencies(http.clone());
    let failure = bind_with(
        &state,
        paths,
        fresh_id(),
        job.id,
        job.revision,
        true,
        Arc::new(|_| Err(error("PRIVATE_KEYCHAIN_SECRET"))),
        factory,
    )
    .await
    .err()
    .unwrap()
    .to_string();
    assert!(failure.contains("keychain"));
    assert!(!failure.contains("PRIVATE_KEYCHAIN_SECRET"));
    assert_eq!(http.actual_requests().count(), 0);
}

#[test]
fn capability_scanner_rejects_symlinks_special_nodes_depth_and_changed_files() {
    use std::io::Write;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().canonicalize().unwrap();
    fs::write(path.join("target"), "abc").unwrap();
    std::os::unix::fs::symlink(path.join("target"), path.join("link")).unwrap();
    let (directory, _) = jobs::open_local_root(path.to_str().unwrap()).unwrap();
    let walk = |prefix: &str, progress: &(dyn Fn(ScanProgress) + Send + Sync)| {
        let mut files = BTreeMap::new();
        collect::walk_local(
            &directory,
            prefix,
            &[],
            &AtomicBool::new(false),
            &ScanBudget::new(),
            progress,
            &mut files,
            &AtomicU64::new(0),
        )
    };
    assert!(walk("", &|_| {}).is_err());
    fs::remove_file(path.join("link")).unwrap();
    nix::unistd::mkfifo(&path.join("fifo"), nix::sys::stat::Mode::S_IRUSR).unwrap();
    assert!(walk("", &|_| {}).is_err());
    fs::remove_file(path.join("fifo")).unwrap();
    assert!(matches!(
        walk(&"x/".repeat(129), &|_| {}),
        Err(FmError::ScanLimited(_))
    ));
    fs::write(path.join("large"), vec![1u8; 2 * 1024 * 1024]).unwrap();
    let changed = AtomicBool::new(false);
    assert!(walk("", &|event| {
        if event.bytes_read >= 1024 * 1024 && !changed.swap(true, Ordering::Relaxed) {
            let mut file = fs::OpenOptions::new()
                .append(true)
                .open(path.join("large"))
                .unwrap();
            file.write_all(b"changed").unwrap();
        }
    })
    .is_err());
}

#[tokio::test]
async fn production_job_client_cancellation_and_deadlines_cover_a_stalled_http_body() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    for deadline in [false, true] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            for index in 0..3 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = vec![];
                let mut buffer = [0; 1024];
                while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                    let count = socket.read(&mut buffer).await.unwrap();
                    assert!(count > 0);
                    request.extend_from_slice(&buffer[..count]);
                }
                let line = std::str::from_utf8(&request)
                    .unwrap()
                    .lines()
                    .next()
                    .unwrap();
                assert!(std::str::from_utf8(&request)
                    .unwrap()
                    .contains("Credential=FAKE_destination/"));
                if index == 0 {
                    assert!(line.starts_with("HEAD "));
                    socket
                        .write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        )
                        .await
                        .unwrap();
                } else if index == 1 {
                    assert!(line.starts_with("GET "));
                    let body="<ListBucketResult><IsTruncated>false</IsTruncated><Contents><Key>dest//file</Key><Size>6</Size><ETag>opaque</ETag></Contents></ListBucketResult>";
                    socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
                } else {
                    assert!(line.starts_with("GET "));
                    socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\nETag: opaque\r\nConnection: close\r\n\r\nabc").await.unwrap();
                    let _ = started_tx.send(());
                    std::future::pending::<()>().await;
                    return;
                }
            }
        });
        let (_root, paths, job) = fixture_at(true, false, &format!("http://{address}"));
        let state = JobRunState::default();
        let bound = bind_with(
            &state,
            paths.clone(),
            fresh_id(),
            job.id,
            job.revision,
            true,
            Arc::new(|_| Ok(Some("FAKE_LOCAL_SERVER_SECRET".into()))),
            Arc::new(client),
        )
        .await
        .unwrap();
        if deadline {
            let mut inner = state.0.lock().unwrap();
            Arc::get_mut(inner.slot.as_mut().unwrap().binding.as_mut().unwrap())
                .unwrap()
                .limits
                .max_seconds = 1;
        }
        let task_state = state.clone();
        let id = bound.run_id.clone();
        let scan_task = tokio::spawn(async move { scan(&task_state, &id, Arc::new(|_| {})).await });
        tokio::time::timeout(Duration::from_secs(3), started_rx)
            .await
            .unwrap()
            .unwrap();
        if !deadline {
            state.release(&bound.run_id).unwrap();
        }
        assert!(run_lock(&paths).is_err());
        let result = tokio::time::timeout(Duration::from_secs(3), scan_task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(matches!(
            (deadline, result.outcome),
            (false, Some(ScanOutcome::Cancelled)) | (true, Some(ScanOutcome::Limited))
        ));
        assert!(result.manifest_id.is_none());
        assert!(run_lock(&paths).is_ok());
        server.abort();
    }
}

#[tokio::test]
#[ignore = "requires an explicitly configured disposable loopback MinIO fixture"]
async fn loopback_minio_native_binding_and_comparison_never_modify_payloads() {
    let endpoint = std::env::var("FURMAN_JOB_TEST_ENDPOINT")
        .expect("Set the explicit disposable loopback endpoint");
    let parsed = url::Url::parse(&endpoint).unwrap();
    assert_eq!(parsed.scheme(), "http");
    assert_eq!(parsed.host_str(), Some("127.0.0.1"));
    assert!(parsed.username().is_empty() && parsed.password().is_none());
    let access =
        std::env::var("FURMAN_JOB_TEST_ACCESS_KEY").expect("Set disposable fixture access key");
    let secret = Zeroizing::new(
        std::env::var("FURMAN_JOB_TEST_SECRET_KEY").expect("Set disposable fixture secret key"),
    );
    let setup = Client::from_conf(
        aws_sdk_s3::Config::builder()
            .behavior_version_latest()
            .region(Region::new("us-east-1"))
            .endpoint_url(endpoint.clone())
            .force_path_style(true)
            .credentials_provider(Credentials::new(
                &access,
                secret.as_str(),
                None,
                None,
                "loopback-fixture",
            ))
            .request_checksum_calculation(RequestChecksumCalculation::WhenRequired)
            .build(),
    );
    let source_bucket = format!("furman-job-source-{}", fresh_id());
    let destination_bucket = format!("furman-job-dest-{}", fresh_id());
    let (_root, paths, job) = fixture_with_names(
        false,
        false,
        &endpoint,
        &source_bucket,
        &destination_bucket,
        Some(&access),
        "src/",
        "dest/",
    );
    let state = JobRunState::default();
    let result: Result<(), FmError> = async {
        setup
            .create_bucket()
            .bucket(&source_bucket)
            .send()
            .await
            .map_err(|failure| job_sdk_error("Fixture create source", &failure))?;
        setup
            .create_bucket()
            .bucket(&destination_bucket)
            .send()
            .await
            .map_err(|failure| job_sdk_error("Fixture create destination", &failure))?;
        setup
            .put_object()
            .bucket(&source_bucket)
            .key("src/nested/file")
            .body(aws_sdk_s3::primitives::ByteStream::from_static(b"abc"))
            .send()
            .await
            .map_err(|failure| job_sdk_error("Fixture source payload", &failure))?;
        setup.put_object().bucket(&source_bucket).key("src/new").body(aws_sdk_s3::primitives::ByteStream::from_static(b"new"))
            .send().await.map_err(|failure| job_sdk_error("Fixture new source payload", &failure))?;
        setup
            .put_object()
            .bucket(&destination_bucket)
            .key("dest/nested/file")
            .body(aws_sdk_s3::primitives::ByteStream::from_static(b"abd"))
            .send()
            .await
            .map_err(|failure| job_sdk_error("Fixture destination payload", &failure))?;
        let reader: KeyReader = Arc::new(move |_| Ok(Some(secret.to_string())));
        let bound = bind_with(
            &state,
            paths,
            fresh_id(),
            job.id,
            job.revision,
            true,
            reader,
            Arc::new(client),
        )
        .await?;
        let result = scan(&state, &bound.run_id, Arc::new(|_| {})).await?;
        if result.phase != "complete"
            || result
                .summary
                .as_ref()
                .is_none_or(|summary| summary.modified != 1 || summary.new != 1)
            || result.manifest_id.is_none()
        {
            return Err(error(
                "Loopback native comparison did not retain the expected complete manifest",
            ));
        }
        let rows = state.page(&bound.run_id, result.manifest_id.as_deref().unwrap(), 0)?;
        if rows.len() != 2 || !rows.iter().any(|row| row.relative_path == "nested/file") || !rows.iter().any(|row| row.relative_path == "new") {
            return Err(error(
                "Loopback comparison altered the exact source/destination prefix mapping",
            ));
        }
        let prepared = preview::prepare(&state, preview::Request {
            run_id: bound.run_id.clone(), manifest_id: result.manifest_id.clone().unwrap(),
            entry_ids: rows.iter().map(|row| row.id.clone()).collect(), policy: preview::Policy::ReplaceUnchanged,
            max_snapshot_bytes: 1024 * 1024, approved: true,
        }, Arc::new(|_| {})).await?;
        if prepared.phase != "prepared" || prepared.preview.as_ref().is_none_or(|preview| preview.bytes != 6 || preview.files != 2 || preview.execution_available) {
            return Err(error("Loopback native snapshot preparation did not retain a checked non-executable preview"));
        }
        state.release(&bound.run_id)?;
        for (bucket, key, expected) in [
            (&source_bucket, "src/nested/file", b"abc"),
            (&source_bucket, "src/new", b"new"),
            (&destination_bucket, "dest/nested/file", b"abd"),
        ] {
            let read = setup
                .get_object()
                .bucket(bucket)
                .key(key)
                .send()
                .await
                .map_err(|failure| job_sdk_error("Fixture verify unchanged", &failure))?;
            let bytes = read
                .body
                .collect()
                .await
                .map_err(|_| error("Fixture payload read failed"))?
                .into_bytes();
            if bytes.as_ref() != expected {
                return Err(error(
                    "Read-only native comparison changed a fixture payload",
                ));
            }
        }
        match setup.head_object().bucket(&destination_bucket).key("dest/new").send().await {
            Err(failure) if failure.raw_response().is_some_and(|response| response.status().as_u16() == 404) => {},
            _ => return Err(error("Read-only preparation did not preserve the missing destination")),
        }
        Ok(())
    }
    .await;
    let active_id = {
        state
            .0
            .lock()
            .unwrap()
            .slot
            .as_ref()
            .map(|slot| slot.id.clone())
    };
    if let Some(id) = active_id {
        let _ = state.release(&id);
    }
    let mut cleanup_failed = false;
    for (bucket, key) in [
        (&source_bucket, "src/nested/file"),
        (&source_bucket, "src/new"),
        (&destination_bucket, "dest/nested/file"),
        (&destination_bucket, "dest/new"),
    ] {
        let _ = setup.delete_object().bucket(bucket).key(key).send().await;
    }
    for bucket in [&source_bucket, &destination_bucket] {
        if setup.delete_bucket().bucket(bucket).send().await.is_err() {
            cleanup_failed = true;
        }
    }
    assert!(
        result.is_ok(),
        "{}",
        result
            .err()
            .map(|failure| failure.to_string())
            .unwrap_or_default()
    );
    assert!(
        !cleanup_failed,
        "Disposable loopback fixture cleanup failed"
    );
}
