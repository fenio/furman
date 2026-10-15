use super::*;
use aws_sdk_s3::{
    config::{retry::RetryConfig, Region},
    Config,
};
use aws_smithy_http_client::test_util::{ReplayEvent, StaticReplayClient};
use aws_smithy_types::body::SdkBody;
use tauri::http::{Request, Response};

fn endpoint(responses: &[(u16, &[(&str, &str)], &str)]) -> (WriteEndpoint, StaticReplayClient) {
    let http = StaticReplayClient::new(
        responses
            .iter()
            .map(|(status, headers, body)| {
                let mut response = Response::builder().status(*status);
                for (name, value) in *headers {
                    response = response.header(*name, *value);
                }
                ReplayEvent::new(
                    Request::builder()
                        .uri("https://writes.test/bucket/key")
                        .body(SdkBody::empty())
                        .unwrap(),
                    response.body(SdkBody::from(body.to_string())).unwrap(),
                )
            })
            .collect(),
    );
    let client = Client::from_conf(
        Config::builder()
            .behavior_version_latest()
            .region(Region::new("us-east-1"))
            .credentials_provider(aws_credential_types::Credentials::new(
                "test", "test", None, None, "test",
            ))
            .endpoint_url("https://writes.test")
            .force_path_style(true)
            .retry_config(RetryConfig::disabled())
            .http_client(http.clone())
            .build(),
    );
    (
        WriteEndpoint {
            client,
            bucket: "bucket".into(),
            id: "connection".into(),
        },
        http,
    )
}

fn selection(path: &std::path::Path) -> WriteSelection {
    WriteSelection {
        path: path.to_str().unwrap().into(),
        destination_key: None,
        expected_sha256: None,
        expected_source_etag: None,
        expected_destination: None,
        expected_destination_sha256: None,
    }
}

async fn local_plan(endpoint: WriteEndpoint, size: u64) -> (WritePlan, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("file");
    let file = std::fs::File::create(&path).unwrap();
    file.set_len(size).unwrap();
    let plan = prepare(
        None,
        endpoint,
        vec![selection(&path)],
        "dest/".into(),
        Arc::new(AtomicBool::new(false)),
        Arc::new(|_| {}),
    )
    .await
    .unwrap();
    (plan, dir)
}

async fn execute(
    plan: WritePlan,
    policy: WritePolicy,
    approved: bool,
) -> Result<WriteResult, FmError> {
    plan.execute(
        policy,
        approved,
        false,
        "test",
        Arc::new(AtomicBool::new(false)),
        &|_| {},
    )
    .await
}

#[tokio::test]
async fn missing_objects_are_created_conditionally() {
    let (endpoint, http) = endpoint(&[(404, &[], ""), (200, &[("etag", "\"created\"")], "")]);
    let (plan, _dir) = local_plan(endpoint, 3).await;
    assert_eq!(plan.preview().existing, 0);
    assert_eq!(
        execute(plan, WritePolicy::CreateOnly, false)
            .await
            .unwrap()
            .written,
        1
    );
    let requests: Vec<_> = http.actual_requests().collect();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1].headers().get("if-none-match").unwrap(), "*");
    assert!(requests[1].headers().get("if-match").is_none());
}

#[tokio::test]
async fn conflicts_and_unsupported_conditions_never_fall_back() {
    for status in [409, 412, 501] {
        let (endpoint, http) = endpoint(&[
            (404, &[], ""),
            (
                status,
                &[],
                "<Error><Code>PreconditionFailed</Code><Message>guard rejected</Message></Error>",
            ),
        ]);
        let (plan, _dir) = local_plan(endpoint, 3).await;
        let error = execute(plan, WritePolicy::CreateOnly, false)
            .await
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains("not weakened"), "{error}");
        assert!(error.contains("guard rejected"), "{error}");
        assert_eq!(http.actual_requests().count(), 2);
    }
}

#[tokio::test]
async fn replacement_requires_approval_and_uses_exact_opaque_etag() {
    let (endpoint, http) = endpoint(&[
        (200, &[("etag", "\"opaque-not-a-hash\"")], ""),
        (200, &[], ""),
    ]);
    let (plan, _dir) = local_plan(endpoint, 1).await;
    assert_eq!(plan.preview().existing, 1);
    assert_eq!(
        execute(plan, WritePolicy::ReplaceUnchanged, true)
            .await
            .unwrap()
            .written,
        1
    );
    let requests: Vec<_> = http.actual_requests().collect();
    assert_eq!(
        requests[1].headers().get("if-match").unwrap(),
        "\"opaque-not-a-hash\""
    );
    assert!(requests[1].headers().get("if-none-match").is_none());
}

#[tokio::test]
async fn rejected_policies_and_skip_do_not_send_payloads() {
    for (policy, etag, approved) in [
        (WritePolicy::CreateOnly, true, false),
        (WritePolicy::ReplaceUnchanged, true, false),
        (WritePolicy::ReplaceUnchanged, false, true),
        (WritePolicy::Overwrite, true, false),
        (WritePolicy::SkipExisting, true, false),
    ] {
        let headers = if etag {
            vec![("etag", "\"old\"")]
        } else {
            vec![]
        };
        let (endpoint, http) = endpoint(&[(200, &headers, "")]);
        let (plan, _dir) = local_plan(endpoint, 3).await;
        let result = execute(plan, policy, approved).await;
        if policy == WritePolicy::SkipExisting {
            assert_eq!(result.unwrap().skipped, 1);
        } else {
            assert!(result.is_err());
        }
        assert_eq!(http.actual_requests().count(), 1);
    }
}

#[tokio::test]
async fn explicit_overwrite_is_unconditional_but_never_retried() {
    let (endpoint, http) = endpoint(&[
        (200, &[("etag", "\"old\"")], ""),
        (503, &[], "<Error><Code>SlowDown</Code></Error>"),
    ]);
    let (plan, _dir) = local_plan(endpoint, 3).await;
    assert!(execute(plan, WritePolicy::Overwrite, true).await.is_err());
    let requests: Vec<_> = http.actual_requests().collect();
    assert_eq!(requests.len(), 2);
    assert!(requests[1].headers().get("if-match").is_none());
    assert!(requests[1].headers().get("if-none-match").is_none());
}

#[tokio::test]
async fn changed_local_source_is_rejected_before_any_upload() {
    let (endpoint, http) = endpoint(&[(404, &[], "")]);
    let (plan, dir) = local_plan(endpoint, 3).await;
    std::fs::write(dir.path().join("file"), b"new").unwrap();
    let error = execute(plan, WritePolicy::CreateOnly, false)
        .await
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("approved preview"), "{error}");
    assert_eq!(http.actual_requests().count(), 1);
}

#[tokio::test]
async fn plans_are_single_use_and_expire() {
    let (endpoint, _) = endpoint(&[(404, &[], "")]);
    let (plan, _dir) = local_plan(endpoint, 1).await;
    let state = WriteState::default();
    let preview = state.insert(plan).unwrap();
    let mut plan = state.take(&preview.id).unwrap();
    assert!(state.take(&preview.id).is_err());
    plan.created = Instant::now() - Duration::from_secs(901);
    let preview = state.insert(plan).unwrap();
    assert!(state.take(&preview.id).is_err());
}

#[tokio::test]
async fn duplicate_mappings_fail_before_payloads() {
    let (endpoint, http) = endpoint(&[(404, &[], "")]);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("file");
    std::fs::write(&path, "data").unwrap();
    let result = prepare(
        None,
        endpoint,
        vec![selection(&path), selection(&path)],
        "".into(),
        Arc::new(AtomicBool::new(false)),
        Arc::new(|_| {}),
    )
    .await;
    assert!(result.err().unwrap().to_string().contains("Duplicate"));
    assert_eq!(http.actual_requests().count(), 1);
}

#[tokio::test]
async fn comparison_expectation_blocks_new_destination_conflicts() {
    let (endpoint, http) = endpoint(&[(200, &[("etag", "\"new\"")], "")]);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("file");
    std::fs::write(&path, "data").unwrap();
    let mut entry = selection(&path);
    entry.expected_destination = Some(DestinationExpectation::Missing);
    let result = prepare(
        None,
        endpoint,
        vec![entry],
        "".into(),
        Arc::new(AtomicBool::new(false)),
        Arc::new(|_| {}),
    )
    .await;
    assert!(result
        .err()
        .unwrap()
        .to_string()
        .contains("changed after comparison"));
    assert_eq!(http.actual_requests().count(), 1);
}

#[tokio::test]
async fn cancelled_execution_does_not_upload() {
    let (endpoint, http) = endpoint(&[(404, &[], "")]);
    let (plan, _dir) = local_plan(endpoint, 1).await;
    assert!(plan
        .execute(
            WritePolicy::CreateOnly,
            false,
            false,
            "test",
            Arc::new(AtomicBool::new(true)),
            &|_| {}
        )
        .await
        .is_err());
    assert_eq!(http.actual_requests().count(), 1);
}

#[tokio::test]
async fn multipart_completion_carries_condition_and_conflicts_abort() {
    for success in [false, true] {
        let (endpoint, http) = endpoint(&[
            (404, &[], ""),
            (200, &[], "<InitiateMultipartUploadResult><UploadId>upload-id</UploadId></InitiateMultipartUploadResult>"),
            (200, &[("etag", "\"part1\"")], ""), (200, &[("etag", "\"part2\"")], ""),
            if success { (200, &[], "<CompleteMultipartUploadResult><ETag>\"done\"</ETag></CompleteMultipartUploadResult>") }
            else { (412, &[], "<Error><Code>PreconditionFailed</Code></Error>") },
            (204, &[], ""),
        ]);
        let (plan, _dir) = local_plan(endpoint, 9 * 1024 * 1024).await;
        let result = execute(plan, WritePolicy::CreateOnly, false).await;
        assert_eq!(result.is_ok(), success, "{:?}", result.err());
        let requests: Vec<_> = http.actual_requests().collect();
        assert_eq!(requests.len(), if success { 5 } else { 6 });
        assert_eq!(requests[4].headers().get("if-none-match").unwrap(), "*");
        if !success {
            assert_eq!(requests[5].method(), "DELETE");
        }
    }
}

#[test]
fn pinned_roots_survive_rename_and_reject_symlink_substitution() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("root");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("file"), "safe").unwrap();
    let files = super::super::write_local::collect(
        root.to_str().unwrap(),
        &mut HashMap::new(),
        &ScanBudget::new(),
        &AtomicBool::new(false),
        &|_| {},
    )
    .unwrap();
    let moved = dir.path().join("moved");
    std::fs::rename(&root, &moved).unwrap();
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("file"), "evil").unwrap();
    let mut snapshot = tempfile::tempfile().unwrap();
    files[0]
        .0
        .snapshot(&mut snapshot, &AtomicBool::new(false))
        .unwrap();
    std::fs::remove_file(moved.join("file")).unwrap();
    symlink(root.join("file"), moved.join("file")).unwrap();
    assert!(files[0]
        .0
        .snapshot(&mut snapshot, &AtomicBool::new(false))
        .is_err());
}

#[tokio::test]
async fn s3_source_conflicts_stop_before_destination_put() {
    let (mut source, source_http) = endpoint(&[
        (
            200,
            &[("etag", "\"approved\""), ("content-length", "3")],
            "",
        ),
        (412, &[], "<Error><Code>PreconditionFailed</Code></Error>"),
    ]);
    source.bucket = "source".into();
    let (destination, dest_http) = endpoint(&[(404, &[], "")]);
    let plan = prepare(
        Some(source),
        destination,
        vec![selection(std::path::Path::new("key"))],
        "".into(),
        Arc::new(AtomicBool::new(false)),
        Arc::new(|_| {}),
    )
    .await
    .unwrap();
    assert!(execute(plan, WritePolicy::CreateOnly, false).await.is_err());
    let requests: Vec<_> = source_http.actual_requests().collect();
    assert_eq!(
        requests[1].headers().get("if-match").unwrap(),
        "\"approved\""
    );
    assert_eq!(dest_http.actual_requests().count(), 1);
}

#[tokio::test]
async fn s3_sync_hash_mismatch_does_not_write_payload() {
    let (mut source, _) = endpoint(&[
        (200, &[("etag", "\"opaque\""), ("content-length", "3")], ""),
        (
            200,
            &[("content-length", "3"), ("etag", "\"opaque\"")],
            "new",
        ),
    ]);
    source.bucket = "source".into();
    let (destination, dest_http) = endpoint(&[(404, &[], "")]);
    let mut entry = selection(std::path::Path::new("key"));
    entry.expected_sha256 = Some("0".repeat(64));
    let plan = prepare(
        Some(source),
        destination,
        vec![entry],
        "".into(),
        Arc::new(AtomicBool::new(false)),
        Arc::new(|_| {}),
    )
    .await
    .unwrap();
    let error = execute(plan, WritePolicy::CreateOnly, false)
        .await
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("approved comparison"), "{error}");
    assert_eq!(dest_http.actual_requests().count(), 1);
}

#[tokio::test]
async fn multipart_replacement_uses_preview_etag_and_aborts_unsupported_completion() {
    let (endpoint, http) = endpoint(&[
        (200, &[("etag", "\"approved\"")], ""),
        (200, &[], "<InitiateMultipartUploadResult><UploadId>upload-id</UploadId></InitiateMultipartUploadResult>"),
        (200, &[("etag", "\"part1\"")], ""), (200, &[("etag", "\"part2\"")], ""),
        (501, &[], "<Error><Code>NotImplemented</Code></Error>"), (204, &[], ""),
    ]);
    let (plan, _dir) = local_plan(endpoint, 9 * 1024 * 1024).await;
    assert!(execute(plan, WritePolicy::ReplaceUnchanged, true)
        .await
        .is_err());
    let requests: Vec<_> = http.actual_requests().collect();
    assert_eq!(requests.len(), 6);
    assert_eq!(
        requests[4].headers().get("if-match").unwrap(),
        "\"approved\""
    );
    assert_eq!(requests[5].method(), "DELETE");
}

#[tokio::test]
async fn comparison_destination_hash_is_read_under_opaque_etag() {
    let (endpoint, http) = endpoint(&[
        (200, &[("etag", "\"opaque\""), ("content-length", "3")], ""),
        (200, &[("content-length", "3")], "new"),
    ]);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("file");
    std::fs::write(&path, "data").unwrap();
    let mut entry = selection(&path);
    entry.expected_destination_sha256 = Some("0".repeat(64));
    let result = prepare(
        None,
        endpoint,
        vec![entry],
        "".into(),
        Arc::new(AtomicBool::new(false)),
        Arc::new(|_| {}),
    )
    .await;
    assert!(result
        .err()
        .unwrap()
        .to_string()
        .contains("changed after comparison"));
    let requests: Vec<_> = http.actual_requests().collect();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1].headers().get("if-match").unwrap(), "\"opaque\"");
}

#[tokio::test]
async fn inaccessible_destinations_are_not_treated_as_missing() {
    let (endpoint, http) = endpoint(&[(403, &[], "<Error><Code>AccessDenied</Code></Error>")]);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("file");
    std::fs::write(&path, "data").unwrap();
    let result = prepare(
        None,
        endpoint,
        vec![selection(&path)],
        "".into(),
        Arc::new(AtomicBool::new(false)),
        Arc::new(|_| {}),
    )
    .await;
    assert!(result.err().unwrap().to_string().contains("AccessDenied"));
    assert_eq!(http.actual_requests().count(), 1);
}

#[tokio::test]
async fn compatible_checksum_policy_does_not_weaken_single_or_multipart_guards() {
    for size in [3, 9 * 1024 * 1024] {
        let small = [(404, &[][..], ""), (200, &[][..], "")];
        let large = [(404, &[][..], ""), (200, &[][..], "<InitiateMultipartUploadResult><UploadId>upload</UploadId></InitiateMultipartUploadResult>"),
            (200, &[("etag", "\"part1\"")][..], ""), (200, &[("etag", "\"part2\"")][..], ""),
            (200, &[][..], "<CompleteMultipartUploadResult><ETag>\"done\"</ETag></CompleteMultipartUploadResult>")];
        let (mut endpoint, http) = endpoint(if size == 3 { &small } else { &large });
        endpoint.client = Client::from_conf(
            endpoint
                .client
                .config()
                .to_builder()
                .request_checksum_calculation(
                    aws_sdk_s3::config::RequestChecksumCalculation::WhenRequired,
                )
                .build(),
        );
        let (plan, _dir) = local_plan(endpoint, size).await;
        execute(plan, WritePolicy::CreateOnly, false).await.unwrap();
        let requests: Vec<_> = http.actual_requests().collect();
        for request in &requests {
            assert!(request
                .headers()
                .get("x-amz-sdk-checksum-algorithm")
                .is_none());
            assert!(request.headers().get("x-amz-checksum-crc32c").is_none());
        }
        assert_eq!(
            requests
                .last()
                .unwrap()
                .headers()
                .get("if-none-match")
                .unwrap(),
            "*"
        );
    }
}

#[test]
fn local_preview_rejects_symlinks_and_special_files() {
    use nix::{sys::stat::Mode, unistd::mkfifo};
    let dir = tempfile::tempdir().unwrap();
    let fifo = dir.path().join("fifo");
    mkfifo(&fifo, Mode::S_IRUSR | Mode::S_IWUSR).unwrap();
    let collect = |path: &std::path::Path| {
        super::super::write_local::collect(
            path.to_str().unwrap(),
            &mut HashMap::new(),
            &ScanBudget::new(),
            &AtomicBool::new(false),
            &|_| {},
        )
    };
    assert!(collect(&fifo).is_err());
    std::os::unix::fs::symlink(&fifo, dir.path().join("link")).unwrap();
    assert!(collect(&dir.path().join("link")).is_err());
    assert!(collect(dir.path()).is_err());
}
