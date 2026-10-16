use super::*;
use crate::job_runs::{
    scan,
    tests::{bind_fixture, empty, fixture_with_names, listing, payload, replay},
};
use aws_smithy_http_client::test_util::StaticReplayClient;
use aws_smithy_types::body::SdkBody;
use std::{fs, time::Duration};
use tauri::http::Response;

fn absent() -> Response<SdkBody> {
    failure(404, "NoSuchKey")
}
fn failure(status: u16, code: &str) -> Response<SdkBody> {
    Response::builder()
        .status(status)
        .body(SdkBody::from(format!("<Error><Code>{code}</Code></Error>")))
        .unwrap()
}
pub(super) fn unlocked() -> Response<SdkBody> {
    failure(404, "ObjectLockConfigurationNotFoundError")
}
pub(super) fn object(version: Option<&str>, encryption: &str) -> Response<SdkBody> {
    let mut response = payload("abc", "source-etag");
    for (name, value) in [
        ("content-type", "text/plain; charset=utf-8"),
        ("cache-control", "max-age=17"),
        ("content-disposition", "inline"),
        ("content-language", "en"),
        ("content-encoding", "identity"),
        ("x-amz-meta-preserved", "native-only-metadata-value"),
        ("x-amz-tagging-count", "2"),
    ] {
        response.headers_mut().insert(name, value.parse().unwrap());
    }
    if let Some(version) = version {
        response
            .headers_mut()
            .insert("x-amz-version-id", version.parse().unwrap());
    }
    match encryption {
        "AES256" => {
            response
                .headers_mut()
                .insert("x-amz-server-side-encryption", "AES256".parse().unwrap());
        }
        "sse_c" => {
            response.headers_mut().insert(
                "x-amz-server-side-encryption-customer-algorithm",
                "AES256".parse().unwrap(),
            );
        }
        _ => {}
    }
    response
}
pub(super) fn acl(owner: &str) -> Response<SdkBody> {
    Response::builder().status(200).body(SdkBody::from(format!("<AccessControlPolicy><Owner><ID>{owner}</ID></Owner><AccessControlList><Grant><Grantee xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:type=\"CanonicalUser\"><ID>{owner}</ID></Grantee><Permission>FULL_CONTROL</Permission></Grant></AccessControlList></AccessControlPolicy>"))).unwrap()
}
pub(super) fn tags(version: Option<&str>, value: &str) -> Response<SdkBody> {
    let mut response=Response::builder().status(200).body(SdkBody::from(format!("<Tagging><TagSet><Tag><Key>space key</Key><Value>{value}</Value></Tag><Tag><Key>stage</Key><Value>prod / test</Value></Tag></TagSet></Tagging>"))).unwrap();
    if let Some(version) = version {
        response
            .headers_mut()
            .insert("x-amz-version-id", version.parse().unwrap());
    }
    response
}
fn review_reads(version: Option<&str>, encryption: &str) -> Vec<Response<SdkBody>> {
    vec![
        absent(),
        object(version, encryption),
        unlocked(),
        acl("source-owner"),
        tags(version, "native-only-tag-value"),
    ]
}
pub(super) fn ack(version: Option<&str>) -> Response<SdkBody> {
    let mut response = empty();
    response
        .headers_mut()
        .insert("etag", "source-etag".parse().unwrap());
    if let Some(version) = version {
        response
            .headers_mut()
            .insert("x-amz-version-id", version.parse().unwrap());
    }
    response
}
fn execution_reads(
    version: Option<&str>,
    encryption: &str,
    destination_version: Option<&str>,
    destination_encryption: &str,
) -> Vec<Response<SdkBody>> {
    let mut responses = review_reads(version, encryption);
    responses.extend([
        ack(destination_version),
        object(destination_version, destination_encryption),
        acl("destination-owner"),
        tags(destination_version, "native-only-tag-value"),
    ]);
    responses
}
async fn prepared(
    sse: bool,
    source: Response<SdkBody>,
    tail: Vec<Response<SdkBody>>,
) -> (
    tempfile::TempDir,
    crate::jobs::Paths,
    JobRunState,
    RunStatus,
    StaticReplayClient,
) {
    let (root, paths, mut job) = fixture_with_names(
        false,
        sse,
        "https://job.test",
        "source-bucket",
        "destination-bucket",
        None,
        "src/",
        "dest/",
    );
    if sse {
        // Explicitly rebind the fake saved definition to independent origins,
        // exercising private native endpoint/client ownership across the relay.
        let mut config: serde_json::Value =
            serde_json::from_slice(&fs::read(&paths.config).unwrap()).unwrap();
        config["connections"][1]["endpoint"] = "https://destination.test".into();
        fs::write(&paths.config, serde_json::to_vec(&config).unwrap()).unwrap();
        let report = crate::jobs::list(&paths).unwrap();
        let saved = crate::jobs::save(
            &paths,
            crate::jobs::SaveRequest {
                expected_revision: report.revision,
                expected_configuration_revision: report.configuration_revision,
                id: Some(job.id),
                name: "Cross-origin fixture".into(),
                source: crate::jobs::SourceRequest::S3 {
                    profile_id: "source".into(),
                    prefix: "src/".into(),
                },
                destination_profile_id: "destination".into(),
                destination_prefix: "dest/".into(),
                exclude_patterns: vec![],
                acknowledged: true,
            },
        )
        .unwrap();
        job = saved.jobs[0].definition.clone();
    }
    let mut responses = vec![
        empty(),
        empty(),
        listing("src/", Some(("file", 3, "source-etag"))),
        payload("abc", "source-etag"),
        listing("dest/", None),
        absent(),
        source,
    ];
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
            policy: preview::Policy::CreateOnly,
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
fn execute_request(status: &RunStatus) -> ExecuteRequest {
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
        acknowledge_replacements: false,
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
async fn s3_creates_preserve_native_headers_tags_versions_and_independent_encryption() {
    for (source_encryption, destination_sse, version) in [
        ("", false, None),
        ("AES256", false, Some("source-version")),
        ("sse_c", true, Some("source-version")),
        ("AES256", true, None),
    ] {
        let destination_encryption = if destination_sse {
            "sse_c"
        } else {
            source_encryption
        };
        let destination_version = version.map(|_| "destination-version");
        let mut tail = review_reads(version, source_encryption);
        tail.extend(execution_reads(
            version,
            source_encryption,
            destination_version,
            destination_encryption,
        ));
        let (_root, paths, state, prepared, http) =
            prepared(destination_sse, object(version, source_encryption), tail).await;
        assert!(prepared.preview.as_ref().unwrap().can_review_execution);
        let review = reviewed(&state, &prepared).await;
        assert_eq!(
            review.phase, "awaiting_execution_approval",
            "{:?}",
            review.message
        );
        let summary = review.execution_review.as_ref().unwrap();
        assert!(summary.s3_source);
        assert_eq!(summary.preserved_tag_count, 2);
        assert_eq!(
            summary.source_aes256_files,
            usize::from(source_encryption == "AES256")
        );
        let request = execute_request(&review);
        start(&state, request).await.unwrap();
        let stopped = wait(&state, &review.run_id).await;
        let execution = stopped.execution.unwrap();
        assert_eq!(
            execution.status,
            Status::Succeeded,
            "{:?}",
            execution.message
        );
        assert!(execution.outcome_recorded);
        assert_eq!(execution.counts.verified_bytes, 3);
        let requests: Vec<_> = http.actual_requests().collect();
        let writes: Vec<_> = requests.iter().filter(|r| r.method() == "PUT").collect();
        assert_eq!(writes.len(), 1);
        let write = writes[0];
        assert!(write.uri().starts_with(if destination_sse {
            "https://destination.test/"
        } else {
            "https://job.test/"
        }));
        assert_eq!(write.headers().get("if-none-match"), Some("*"));
        assert_eq!(
            write.headers().get("x-amz-meta-preserved"),
            Some("native-only-metadata-value")
        );
        assert_eq!(
            write.headers().get("x-amz-tagging"),
            Some("space%20key=native-only-tag-value&stage=prod%20%2F%20test")
        );
        assert_eq!(write.headers().get("x-amz-storage-class"), Some("STANDARD"));
        assert_eq!(
            write.headers().get("content-type"),
            Some("text/plain; charset=utf-8")
        );
        assert_eq!(write.headers().get("cache-control"), Some("max-age=17"));
        assert_eq!(write.body().bytes().unwrap(), b"abc");
        assert!(write.headers().get("x-amz-grant-full-control").is_none());
        assert_eq!(
            write.headers().get("x-amz-server-side-encryption"),
            if source_encryption == "AES256" && !destination_sse {
                Some("AES256")
            } else {
                None
            }
        );
        let source_gets: Vec<_> = requests
            .iter()
            .filter(|r| {
                r.uri().contains("/source-bucket/src/file")
                    && !r.uri().contains("acl")
                    && !r.uri().contains("tagging")
                    && r.method() == "GET"
            })
            .collect();
        for read in source_gets {
            assert!(read.uri().starts_with("https://job.test/"));
            assert_eq!(read.headers().get("if-match"), Some("source-etag"));
            if destination_sse {
                let key = read
                    .headers()
                    .get("x-amz-server-side-encryption-customer-key")
                    .unwrap();
                assert_ne!(
                    Some(key),
                    write
                        .headers()
                        .get("x-amz-server-side-encryption-customer-key")
                );
            }
        }
        for request in requests.iter().filter(|r| {
            r.uri().contains("acl")
                || r.uri().contains("tagging")
                || r.uri().contains("object-lock")
        }) {
            assert!(request
                .headers()
                .get("x-amz-server-side-encryption-customer-key")
                .is_none());
        }
        if version.is_some() {
            assert!(requests
                .iter()
                .any(|r| r.uri().contains("versionId=source-version")));
            assert!(requests
                .iter()
                .any(|r| r.uri().contains("versionId=destination-version")));
        }
        let history = state.history(&paths).unwrap();
        assert_eq!(history.records[0].status, Status::Succeeded);
        let bytes = fs::read_to_string(paths.store.parent().unwrap().join("job-runs/journal.json"))
            .unwrap();
        for private in [
            "native-only-metadata-value",
            "native-only-tag-value",
            "source-owner",
            "destination-owner",
            "source-version",
            "destination-version",
            "source-etag",
        ] {
            assert!(!bytes.contains(private));
        }
    }
}

#[tokio::test]
async fn unpreservable_source_headers_and_sse_c_downgrade_never_authorize_writes() {
    for (name, value) in [
        ("x-amz-server-side-encryption", "aws:kms"),
        ("x-amz-storage-class", "GLACIER"),
        ("x-amz-object-lock-mode", "GOVERNANCE"),
        ("x-amz-object-lock-legal-hold", "ON"),
        ("expires", "Wed, 21 Oct 2026 07:28:00 GMT"),
        ("x-amz-website-redirect-location", "/elsewhere"),
        ("x-amz-missing-meta", "1"),
        ("x-amz-server-side-encryption-customer-algorithm", "AES256"),
    ] {
        let mut source = object(None, "");
        source.headers_mut().insert(name, value.parse().unwrap());
        let (_root, _paths, state, prepared, http) = prepared(false, source, vec![]).await;
        assert!(
            !prepared.preview.as_ref().unwrap().can_review_execution,
            "{name}"
        );
        assert!(review(
            &state,
            ReviewRequest {
                run_id: prepared.run_id.clone(),
                preview_id: prepared.preview.unwrap().id
            }
        )
        .await
        .is_err());
        assert_eq!(http.actual_requests().count(), 7);
        state.release(&prepared.run_id).unwrap();
    }
}

#[tokio::test]
async fn acl_tag_and_object_lock_denials_ambiguity_and_unsupported_reads_fail_closed_before_mutation(
) {
    for mode in 0..7 {
        let mut tail = vec![absent(), object(None, "")];
        match mode {
            0 => tail.push(failure(403, "AccessDenied")),
            1 => tail.push(failure(501, "NotImplemented")),
            2 => tail.push(empty()),
            _ => {
                tail.push(unlocked());
                if mode == 3 {
                    tail.push(Response::builder().status(200).body(SdkBody::from("<AccessControlPolicy><Owner><ID>source-owner</ID></Owner><AccessControlList/></AccessControlPolicy>")).unwrap());
                } else {
                    tail.push(acl("source-owner"));
                    tail.push(if mode==4{failure(403,"AccessDenied")}else if mode==5{tags(Some("unexpected-version"),"native-only-tag-value")}else{Response::builder().status(200).body(SdkBody::from("<Tagging><TagSet><Tag><Key>duplicate</Key><Value>1</Value></Tag><Tag><Key>duplicate</Key><Value>2</Value></Tag></TagSet></Tagging>")).unwrap()});
                }
            }
        }
        let (_root, _paths, state, prepared, http) = prepared(false, object(None, ""), tail).await;
        let result = reviewed(&state, &prepared).await;
        assert_eq!(result.phase, "stopped", "{mode}");
        assert!(result.execution_review.is_none());
        assert_eq!(
            http.actual_requests()
                .filter(|r| r.method() == "PUT")
                .count(),
            0
        );
        assert!(!result.message.unwrap().contains("SSO"));
    }
}

#[tokio::test]
async fn same_etag_source_version_headers_tags_owner_or_content_drift_invalidates_consumed_approval(
) {
    for mode in 0..5 {
        let mut tail = review_reads(Some("source-version"), "");
        let mut current = object(Some("source-version"), "");
        if mode == 0 {
            current
                .headers_mut()
                .insert("cache-control", "changed-with-same-etag".parse().unwrap());
        }
        if mode == 1 {
            current
                .headers_mut()
                .insert("x-amz-version-id", "changed-version".parse().unwrap());
        }
        if mode == 2 {
            *current.body_mut() = SdkBody::from("xyz");
        }
        tail.extend([absent(), current]);
        if mode >= 3 {
            tail.extend([
                unlocked(),
                acl(if mode == 3 {
                    "changed-owner"
                } else {
                    "source-owner"
                }),
                tags(
                    Some("source-version"),
                    if mode == 4 {
                        "changed-tag"
                    } else {
                        "native-only-tag-value"
                    },
                ),
            ]);
        }
        let (_root, _paths, state, prepared, http) =
            prepared(false, object(Some("source-version"), ""), tail).await;
        let review = reviewed(&state, &prepared).await;
        start(&state, execute_request(&review)).await.unwrap();
        let result = wait(&state, &review.run_id).await;
        assert_eq!(result.execution.unwrap().status, Status::Failed);
        assert_eq!(
            http.actual_requests()
                .filter(|r| r.method() == "PUT")
                .count(),
            0
        );
        assert!(start(&state, execute_request(&review)).await.is_err());
    }
}

#[tokio::test]
async fn failed_destination_attribute_acl_tag_version_or_encryption_verification_is_uncertain_without_retry(
) {
    for mode in 0..6 {
        let mut tail = review_reads(Some("source-version"), "AES256");
        tail.extend(review_reads(Some("source-version"), "AES256"));
        tail.push(ack(Some("destination-version")));
        let mut actual = object(Some("destination-version"), "AES256");
        if mode == 0 {
            actual.headers_mut().insert(
                "x-amz-meta-preserved",
                "different-metadata".parse().unwrap(),
            );
        }
        if mode == 1 {
            actual.headers_mut().remove("x-amz-server-side-encryption");
        }
        if mode == 2 {
            actual
                .headers_mut()
                .insert("x-amz-version-id", "different-version".parse().unwrap());
        }
        if mode == 3 {
            actual
                .headers_mut()
                .insert("x-amz-storage-class", "STANDARD_IA".parse().unwrap());
        }
        tail.push(actual);
        if mode >= 4 {
            tail.push(if mode == 4 {
                failure(403, "AccessDenied")
            } else {
                acl("destination-owner")
            });
            if mode == 5 {
                tail.push(tags(Some("destination-version"), "different-tag"));
            }
        }
        let (_root, paths, state, prepared, http) =
            prepared(false, object(Some("source-version"), "AES256"), tail).await;
        let review = reviewed(&state, &prepared).await;
        start(&state, execute_request(&review)).await.unwrap();
        let result = wait(&state, &review.run_id).await;
        let execution = result.execution.unwrap();
        assert_eq!(
            execution.status,
            Status::Uncertain,
            "{mode}: {:?}",
            execution.message
        );
        assert_eq!(execution.counts.verified_files, 0);
        assert_eq!(execution.counts.acknowledged_files, 1);
        assert!(execution.outcome_recorded);
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
async fn s3_review_refuses_an_execution_budget_that_cannot_cover_known_mandatory_calls() {
    let (_root, _paths, state, prepared, http) = prepared(false, object(None, ""), vec![]).await;
    {
        let mut inner = state.0.lock().unwrap();
        Arc::get_mut(inner.slot.as_mut().unwrap().binding.as_mut().unwrap())
            .unwrap()
            .limits
            .max_requests = 8;
    }
    let failure = review(
        &state,
        ReviewRequest {
            run_id: prepared.run_id.clone(),
            preview_id: prepared.preview.as_ref().unwrap().id.clone(),
        },
    )
    .await
    .err()
    .unwrap()
    .to_string();
    assert!(failure.contains("at least 9"));
    assert_eq!(http.actual_requests().count(), 7);
    assert!(state
        .status(&prepared.run_id)
        .unwrap()
        .execution_review
        .is_none());
}

#[tokio::test]
async fn bounded_portable_tags_and_denied_acl_reads_cannot_be_silently_dropped() {
    for mode in 0..5 {
        let mut tail = vec![absent(), object(None, ""), unlocked()];
        if mode == 0 {
            tail.push(failure(403, "AccessDenied"));
        } else {
            tail.push(acl("source-owner"));
            tail.push(match mode {
                1 => failure(501, "NotImplemented"),
                2 => {
                    let body = (0..11)
                        .map(|index| {
                            format!("<Tag><Key>key{index}</Key><Value>value</Value></Tag>")
                        })
                        .collect::<String>();
                    Response::builder()
                        .status(200)
                        .body(SdkBody::from(format!(
                            "<Tagging><TagSet>{body}</TagSet></Tagging>"
                        )))
                        .unwrap()
                }
                3 => tags(None, "not&amp;portable"),
                _ => tags(None, &"v".repeat(257)),
            });
        }
        let (_root, _paths, state, prepared, http) = prepared(false, object(None, ""), tail).await;
        let reviewed = reviewed(&state, &prepared).await;
        assert_eq!(reviewed.phase, "stopped");
        assert!(reviewed.execution_review.is_none());
        assert_eq!(
            http.actual_requests()
                .filter(|r| r.method() == "PUT")
                .count(),
            0
        );
    }
}

#[tokio::test]
#[ignore = "requires an explicitly configured disposable loopback MinIO fixture"]
async fn loopback_minio_s3_create_preserves_versioned_metadata_headers_and_tags_without_changing_source(
) {
    for versioned in [false, true] {
        loopback_relay_fixture(versioned).await;
    }
}
async fn loopback_relay_fixture(versioned: bool) {
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
                "loopback-s3-job-fixture",
            ))
            .request_checksum_calculation(
                aws_sdk_s3::config::RequestChecksumCalculation::WhenRequired,
            )
            .build(),
    );
    let source = format!("furman-job-attrs-source-{}", fresh_id());
    let destination = format!("furman-job-attrs-dest-{}", fresh_id());
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
        let uploaded = setup
            .put_object()
            .bucket(&source)
            .key("src/nested/file.txt")
            .body(aws_sdk_s3::primitives::ByteStream::from_static(b"abc"))
            .content_type("text/plain; charset=utf-8")
            .cache_control("max-age=17")
            .content_disposition("inline")
            .content_language("en")
            .content_encoding("identity")
            .metadata("preserved", "native-only-metadata-value")
            .tagging("space%20key=native-only-tag-value&stage=prod%20%2F%20test")
            .send()
            .await
            .map_err(|e| job_sdk_error("Fixture seed source", &e))?;
        let (_root, paths, job) = fixture_with_names(
            false,
            false,
            &endpoint,
            &source,
            &destination,
            Some(&access),
            "src/",
            "dest/",
        );
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
                policy: preview::Policy::CreateOnly,
                max_snapshot_bytes: 100,
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
        start(&state, execute_request(&reviewed)).await?;
        let stopped = wait(&state, &reviewed.run_id).await;
        let execution = stopped
            .execution
            .ok_or_else(|| error("Fixture execution outcome missing"))?;
        if execution.status != Status::Succeeded || !execution.outcome_recorded {
            return Err(error(
                execution
                    .message
                    .as_deref()
                    .unwrap_or("Fixture execution was not durably successful"),
            ));
        }
        for (bucket, key) in [
            (&source, "src/nested/file.txt"),
            (&destination, "dest/nested/file.txt"),
        ] {
            let read = setup
                .get_object()
                .bucket(bucket)
                .key(key)
                .send()
                .await
                .map_err(|e| job_sdk_error("Fixture readback", &e))?;
            if read
                .metadata()
                .and_then(|m| m.get("preserved"))
                .map(String::as_str)
                != Some("native-only-metadata-value")
                || read.content_type() != Some("text/plain; charset=utf-8")
                || read.cache_control() != Some("max-age=17")
                || read.content_disposition() != Some("inline")
                || read.content_language() != Some("en")
                || read.content_encoding() != Some("identity")
            {
                return Err(error("Fixture attribute preservation failed"));
            }
            if bucket == &source && read.version_id() != uploaded.version_id() {
                return Err(error("Source version changed during native relay"));
            }
            let version = read.version_id().map(str::to_owned);
            let bytes = read
                .body
                .collect()
                .await
                .map_err(|_| error("Fixture body failed"))?
                .into_bytes();
            if bytes.as_ref() != b"abc" {
                return Err(error("Fixture payload changed"));
            }
            let tags = setup
                .get_object_tagging()
                .bucket(bucket)
                .key(key)
                .set_version_id(version)
                .send()
                .await
                .map_err(|e| job_sdk_error("Fixture tags", &e))?;
            if tags.tag_set().len() != 2
                || !tags
                    .tag_set()
                    .iter()
                    .any(|t| t.key() == "stage" && t.value() == "prod / test")
            {
                return Err(error("Fixture tags not preserved"));
            }
        }
        if state.history(&paths)?.records[0].status != Status::Succeeded {
            return Err(error("Fixture journal outcome missing"));
        }
        Ok(())
    }
    .await;
    for bucket in [&source, &destination] {
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
        let _ = setup.delete_bucket().bucket(bucket).send().await;
    }
    result.unwrap();
}
