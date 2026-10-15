use super::{compat, metadata, service::S3Service};
use aws_sdk_s3::{
    config::{retry::RetryConfig, Region},
    Client, Config,
};
use aws_smithy_http_client::test_util::{ReplayEvent, StaticReplayClient};
use aws_smithy_types::body::SdkBody;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
};
use tauri::http::{Request, Response};

const ACL: &str = "<AccessControlPolicy><Owner><ID>owner</ID></Owner><AccessControlList><Grant><Grantee xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:type=\"CanonicalUser\"><ID>owner</ID></Grantee><Permission>FULL_CONTROL</Permission></Grant><Grant><Grantee xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:type=\"Group\"><URI>http://acs.amazonaws.com/groups/global/AllUsers</URI></Grantee><Permission>READ</Permission></Grant></AccessControlList></AccessControlPolicy>";
const COPY: &str = "<CopyObjectResult><ETag>\"new\"</ETag></CopyObjectResult>";
const CREATE: &str =
    "<InitiateMultipartUploadResult><UploadId>upload</UploadId></InitiateMultipartUploadResult>";
const PART: &str = "<CopyPartResult><ETag>\"part\"</ETag><ChecksumCRC32C>AAAAAA==</ChecksumCRC32C></CopyPartResult>";
const TAGS: &str =
    "<Tagging><TagSet><Tag><Key>a&amp;b</Key><Value>space + value</Value></Tag></TagSet></Tagging>";
const COMPLETE: &str =
    "<CompleteMultipartUploadResult><ETag>\"new\"</ETag></CompleteMultipartUploadResult>";

fn service(replies: &[(u16, &[(&str, &str)], &str)]) -> (S3Service, StaticReplayClient) {
    let http = StaticReplayClient::new(
        replies
            .iter()
            .map(|(status, headers, body)| {
                let mut response = Response::builder().status(*status);
                for (name, value) in *headers {
                    response = response.header(*name, *value);
                }
                ReplayEvent::new(
                    Request::builder()
                        .uri("https://compat.test/bucket/key")
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
            .endpoint_url("https://compat.test")
            .force_path_style(true)
            .credentials_provider(aws_credential_types::Credentials::new(
                "test", "test", None, None, "test",
            ))
            .retry_config(RetryConfig::disabled())
            .http_client(http.clone())
            .build(),
    );
    (S3Service::new(client, "bucket".into()), http)
}

#[test]
fn mime_inference_and_validation_do_not_confuse_ciphertext_or_keys_with_urls() {
    for (key, mime) in [
        ("path/FILE.JSON", "application/json"),
        ("x.html", "text/html"),
        ("a.tar.gz", "application/gzip"),
        ("no-extension", "application/octet-stream"),
        ("x.json?literal-key", "application/octet-stream"),
    ] {
        assert_eq!(compat::content_type(key, false), mime);
        assert_eq!(compat::content_type(key, true), "application/octet-stream");
    }
    for value in [
        "application/json",
        "text/plain; charset=utf-8",
        "image/svg+xml",
    ] {
        assert!(compat::validate_content_type(Some(value)).is_ok());
    }
    for value in [
        "",
        "bad",
        "text/",
        "*/json",
        "text/plain; nope",
        "text/plain; charset=",
        "text/plain\r\nAuthorization: secret",
    ] {
        assert!(compat::validate_content_type(Some(value)).is_err());
    }
}

#[test]
fn transport_policy_is_explicit_and_separate_from_sha256_verification() {
    let (service, _) = service(&[]);
    assert_eq!(
        compat::upload_checksum(&service.client),
        Some(aws_sdk_s3::types::ChecksumAlgorithm::Crc32C)
    );
    let client = Client::from_conf(
        service
            .client
            .config()
            .to_builder()
            .request_checksum_calculation(
                aws_sdk_s3::config::RequestChecksumCalculation::WhenRequired,
            )
            .build(),
    );
    assert_eq!(compat::upload_checksum(&client), None);
}

#[tokio::test]
async fn small_metadata_copy_preserves_attributes_grants_and_encoded_versioned_source() {
    let (service, http) = service(&[
        (
            200,
            &[
                ("etag", "\"opaque\""),
                ("content-length", "3"),
                ("x-amz-version-id", "v+/="),
                ("content-language", "fr"),
                ("expires", "Wed, 21 Oct 2037 07:28:00 GMT"),
                ("x-amz-storage-class", "STANDARD_IA"),
                ("x-amz-server-side-encryption", "AES256"),
            ],
            "",
        ),
        (200, &[], ACL),
        (200, &[], COPY),
    ]);
    service
        .put_object_metadata(
            "folder/a +?.txt",
            Some("application/json"),
            None,
            Some("max-age=60"),
            None,
            &HashMap::from([("custom".into(), "value".into())]),
        )
        .await
        .unwrap();
    let requests: Vec<_> = http.actual_requests().collect();
    assert_eq!(requests.len(), 3);
    let headers = requests[2].headers();
    assert_eq!(headers.get("if-match").unwrap(), "\"opaque\"");
    assert_eq!(
        headers.get("x-amz-copy-source-if-match").unwrap(),
        "\"opaque\""
    );
    assert_eq!(
        headers.get("x-amz-copy-source").unwrap(),
        "bucket/folder%2Fa%20%2B%3F.txt?versionId=v%2B%2F%3D"
    );
    assert_eq!(headers.get("content-language").unwrap(), "fr");
    assert!(headers.get("expires").is_some());
    assert_eq!(headers.get("x-amz-storage-class").unwrap(), "STANDARD_IA");
    assert_eq!(
        headers.get("x-amz-server-side-encryption").unwrap(),
        "AES256"
    );
    assert!(headers
        .get("x-amz-grant-read")
        .unwrap()
        .contains("AllUsers"));
    assert_eq!(
        headers.get("x-amz-grant-full-control").unwrap(),
        "id=\"owner\""
    );
    assert_eq!(headers.get("x-amz-tagging-directive").unwrap(), "COPY");
}

#[tokio::test]
async fn stale_revisions_missing_validators_and_unpreservable_encryption_fail_before_writing() {
    for (headers, expected, custom) in [
        (
            vec![("etag", "\"opaque\""), ("content-length", "3")],
            Some("stale"),
            HashMap::new(),
        ),
        (vec![("content-length", "3")], None, HashMap::new()),
        (
            vec![
                ("etag", "\"opaque\""),
                ("content-length", "3"),
                ("x-amz-server-side-encryption", "aws:kms"),
            ],
            None,
            HashMap::new(),
        ),
        (
            vec![
                ("etag", "\"opaque\""),
                ("content-length", "3"),
                ("x-amz-meta-furman-encrypted", "aes-256-gcm"),
            ],
            None,
            HashMap::new(),
        ),
    ] {
        let (service, http) = service(&[(200, &headers, "")]);
        assert!(service
            .put_object_metadata_checked(
                "key",
                Some("text/plain"),
                None,
                None,
                None,
                &custom,
                expected
            )
            .await
            .is_err());
        assert_eq!(http.actual_requests().count(), 1);
    }
}

#[tokio::test]
async fn acl_denials_and_conditional_copy_errors_never_weaken_or_retry() {
    let (service, http) = service(&[
        (200, &[("etag", "\"opaque\""), ("content-length", "3")], ""),
        (403, &[], "<Error><Code>AccessDenied</Code></Error>"),
    ]);
    assert!(service
        .put_object_metadata("key", None, None, None, None, &HashMap::new())
        .await
        .is_err());
    assert_eq!(http.actual_requests().count(), 2);
    for status in [412, 501, 503] {
        let (service, http) = self::service(&[
            (200, &[("etag", "\"opaque\""), ("content-length", "3")], ""),
            (200, &[], ACL),
            (status, &[], "<Error><Code>NotImplemented</Code></Error>"),
        ]);
        assert!(service
            .put_object_metadata("key", None, None, None, None, &HashMap::new())
            .await
            .is_err());
        assert_eq!(http.actual_requests().count(), 3);
    }
}

#[tokio::test]
async fn large_metadata_edits_pin_parts_preserve_tags_and_condition_completion() {
    for success in [true, false] {
        let (service, http) = service(&[
            (
                200,
                &[
                    ("etag", "\"opaque\""),
                    ("content-length", "9437184"),
                    ("content-language", "fr"),
                ],
                "",
            ),
            (200, &[], ACL),
            (200, &[], TAGS),
            (200, &[], CREATE),
            (200, &[], PART),
            if success {
                (200, &[], COMPLETE)
            } else {
                (412, &[], "<Error><Code>PreconditionFailed</Code></Error>")
            },
            (204, &[], ""),
        ]);
        let result = service
            .put_object_metadata("key", Some("text/plain"), None, None, None, &HashMap::new())
            .await;
        assert_eq!(result.is_ok(), success, "{:?}", result.err());
        let requests: Vec<_> = http.actual_requests().collect();
        assert_eq!(requests.len(), if success { 6 } else { 7 });
        assert_eq!(requests[3].headers().get("content-language").unwrap(), "fr");
        assert_eq!(
            requests[3].headers().get("x-amz-tagging").unwrap(),
            "a%26b=space%20%2B%20value"
        );
        assert_eq!(
            requests[4]
                .headers()
                .get("x-amz-copy-source-if-match")
                .unwrap(),
            "\"opaque\""
        );
        assert_eq!(
            requests[4]
                .headers()
                .get("x-amz-copy-source-range")
                .unwrap(),
            "bytes=0-9437183"
        );
        assert_eq!(requests[5].headers().get("if-match").unwrap(), "\"opaque\"");
        if !success {
            assert_eq!(requests[6].method(), "DELETE");
        }
    }
}

#[tokio::test]
async fn objects_above_five_gib_use_complete_bounded_multipart_ranges() {
    let size = 5 * 1024 * 1024 * 1024u64 + 1;
    let size_header = size.to_string();
    let headers = [
        ("etag", "\"opaque\""),
        ("content-length", size_header.as_str()),
    ];
    let mut replies: Vec<(u16, &[(&str, &str)], &str)> = vec![
        (200, &headers, ""),
        (200, &[], ACL),
        (200, &[], TAGS),
        (200, &[], CREATE),
    ];
    replies.extend((0..41).map(|_| (200, &[][..], PART)));
    replies.push((200, &[], COMPLETE));
    let (service, http) = service(&replies);
    service
        .put_object_metadata("key", Some("text/plain"), None, None, None, &HashMap::new())
        .await
        .unwrap();
    let requests: Vec<_> = http.actual_requests().collect();
    assert_eq!(requests.len(), 46);
    assert_eq!(
        requests[44]
            .headers()
            .get("x-amz-copy-source-range")
            .unwrap(),
        "bytes=5368709120-5368709120"
    );
    assert_eq!(
        requests[45].headers().get("if-match").unwrap(),
        "\"opaque\""
    );
}

#[tokio::test]
async fn only_explicit_unsupported_bulk_deletes_use_single_object_fallback() {
    let (service, http) = service(&[
        (501, &[], "<Error><Code>NotImplemented</Code></Error>"),
        (204, &[], ""),
        (204, &[], ""),
    ]);
    let events = Mutex::new(Vec::new());
    service
        .delete_objects(
            &["a".into(), "b".into()],
            "test",
            &AtomicBool::new(false),
            &|event| events.lock().unwrap().push(event),
        )
        .await
        .unwrap();
    assert_eq!(http.actual_requests().count(), 3);
    assert_eq!(events.lock().unwrap().last().unwrap().files_done, 2);
    for (status, body) in [
        (400, "<Error><Code>InvalidRequest</Code></Error>"),
        (403, "<Error><Code>AccessDenied</Code></Error>"),
        (500, "<Error><Code>InternalError</Code></Error>"),
        (501, "<html>unknown gateway response</html>"),
        (
            200,
            "<DeleteResult><Error><Key>a</Key><Code>NotImplemented</Code></Error></DeleteResult>",
        ),
    ] {
        let (service, http) = self::service(&[(status, &[], body)]);
        assert!(service
            .delete_objects(&["a".into()], "test", &AtomicBool::new(false), &|_| {})
            .await
            .is_err());
        assert_eq!(http.actual_requests().count(), 1);
    }
}

#[tokio::test]
async fn single_delete_fallback_stops_on_cancellation_or_failure_with_partial_counts() {
    let (service, http) = service(&[
        (501, &[], "<Error><Code>NotImplemented</Code></Error>"),
        (204, &[], ""),
    ]);
    let cancel = AtomicBool::new(false);
    assert!(service
        .delete_objects(&["a".into(), "b".into()], "test", &cancel, &|_| cancel
            .store(true, Ordering::Relaxed))
        .await
        .is_err());
    assert_eq!(http.actual_requests().count(), 2);
    let (service, http) = self::service(&[
        (501, &[], "<Error><Code>NotImplemented</Code></Error>"),
        (204, &[], ""),
        (403, &[], "<Error><Code>AccessDenied</Code></Error>"),
    ]);
    let error = service
        .delete_objects(
            &["a".into(), "b".into()],
            "test",
            &AtomicBool::new(false),
            &|_| {},
        )
        .await
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("1 of 2"), "{error}");
    assert_eq!(http.actual_requests().count(), 3);
}

#[test]
fn metadata_revisions_are_order_independent_but_track_edited_attributes() {
    let a = aws_sdk_s3::operation::head_object::HeadObjectOutput::builder()
        .e_tag("opaque")
        .metadata("a", "1")
        .metadata("b", "2")
        .content_type("text/plain")
        .build();
    let b = aws_sdk_s3::operation::head_object::HeadObjectOutput::builder()
        .e_tag("opaque")
        .metadata("b", "2")
        .metadata("a", "1")
        .content_type("text/plain")
        .build();
    let c = aws_sdk_s3::operation::head_object::HeadObjectOutput::builder()
        .e_tag("opaque")
        .metadata("a", "1")
        .metadata("b", "2")
        .content_type("application/json")
        .build();
    assert_eq!(metadata::revision(&a), metadata::revision(&b));
    assert_ne!(metadata::revision(&a), metadata::revision(&c));
}

#[tokio::test]
async fn owner_only_acls_are_omitted_only_when_bucket_owner_enforced() {
    const OWNER: &str = "<AccessControlPolicy><Owner><ID>owner</ID></Owner><AccessControlList><Grant><Grantee><ID>owner</ID><Type>CanonicalUser</Type></Grantee><Permission>FULL_CONTROL</Permission></Grant></AccessControlList></AccessControlPolicy>";
    for enforced in [true, false] {
        let ownership = if enforced {
            (200, &[][..], "<OwnershipControls><Rule><ObjectOwnership>BucketOwnerEnforced</ObjectOwnership></Rule></OwnershipControls>")
        } else {
            (
                404,
                &[][..],
                "<Error><Code>OwnershipControlsNotFoundError</Code></Error>",
            )
        };
        let (service, http) = service(&[
            (200, &[("etag", "\"opaque\""), ("content-length", "3")], ""),
            (200, &[], OWNER),
            ownership,
            (200, &[], COPY),
        ]);
        service
            .put_object_metadata("key", Some("text/plain"), None, None, None, &HashMap::new())
            .await
            .unwrap();
        let requests: Vec<_> = http.actual_requests().collect();
        assert_eq!(requests.len(), 4);
        assert_eq!(
            requests[3]
                .headers()
                .get("x-amz-grant-full-control")
                .is_none(),
            enforced
        );
    }
}

#[tokio::test]
async fn malformed_copy_parts_abort_and_cleanup_errors_are_reported() {
    for abort_status in [204, 503] {
        let (service, http) = service(&[
            (
                200,
                &[("etag", "\"opaque\""), ("content-length", "9437184")],
                "",
            ),
            (200, &[], ACL),
            (200, &[], TAGS),
            (200, &[], CREATE),
            (200, &[], "<CopyPartResult/>"),
            (abort_status, &[], ""),
        ]);
        let error = service
            .put_object_metadata("key", None, None, None, None, &HashMap::new())
            .await
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains("no ETag"), "{error}");
        assert_eq!(
            error.contains("Could not confirm multipart cleanup"),
            abort_status != 204
        );
        let requests: Vec<_> = http.actual_requests().collect();
        assert_eq!(requests.len(), 6);
        assert_eq!(requests[5].method(), "DELETE");
    }
}

#[tokio::test]
async fn an_ambiguous_delete_is_not_retried_into_an_unsupported_response() {
    let (mut service, http) = service(&[
        (500, &[], "<Error><Code>InternalError</Code></Error>"),
        (501, &[], "<Error><Code>NotImplemented</Code></Error>"),
    ]);
    service.client = Client::from_conf(
        service
            .client
            .config()
            .to_builder()
            .retry_config(RetryConfig::standard().with_max_attempts(3))
            .build(),
    );
    let error = service
        .delete_objects(&["a".into()], "test", &AtomicBool::new(false), &|_| {})
        .await
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("HTTP 500"), "{error}");
    assert_eq!(http.actual_requests().count(), 1);
}

#[tokio::test]
async fn known_garage_profiles_reject_conditional_edits_before_requests() {
    let (mut service, http) = service(&[]);
    service.client = compat::configure_conditions(service.client, Some("garage"), None);
    assert!(!compat::conditional_writes_supported(&service.client));
    let error = service
        .put_object_metadata("key", Some("text/plain"), None, None, None, &HashMap::new())
        .await
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("Conditional destination writes are disabled"));
    assert_eq!(http.actual_requests().count(), 0);
    let cloned = Client::from_conf(
        service
            .client
            .config()
            .to_builder()
            .retry_config(RetryConfig::disabled())
            .build(),
    );
    assert!(!compat::conditional_writes_supported(&cloned));
}

#[tokio::test]
async fn signing_backstop_blocks_profile_resolved_garage_keys_without_exposing_them() {
    let (service, http) = service(&[(200, &[], "")]);
    let key_id = "GK0123456789abcdef01234567";
    let client = Client::from_conf(
        service
            .client
            .config()
            .to_builder()
            .credentials_provider(aws_credential_types::Credentials::new(
                key_id,
                "SECRET",
                None,
                None,
                "test-resolved-profile",
            ))
            .build(),
    );
    let client = compat::configure_conditions(client, None, None);
    // The identity is intentionally not resolved during preview. The signed
    // request backstop must stop it before any target object can be mutated.
    assert!(compat::conditional_writes_supported(&client));
    let error = client
        .put_object()
        .bucket("bucket")
        .key("key")
        .if_none_match("*")
        .body(aws_sdk_s3::primitives::ByteStream::from_static(b"payload"))
        .send()
        .await
        .err()
        .unwrap();
    let message = super::errors::s3_sdk_error("Guarded PUT", &error).to_string();
    assert!(
        message.contains("Conditional destination writes are disabled"),
        "{message}"
    );
    assert!(!message.contains(key_id));
    assert!(!message.contains("SECRET"));
    assert_eq!(http.actual_requests().count(), 0);
    assert!(client
        .copy_object()
        .bucket("bucket")
        .key("key")
        .copy_source("bucket/source")
        .if_match("opaque")
        .send()
        .await
        .is_err());
    assert!(client
        .complete_multipart_upload()
        .bucket("bucket")
        .key("key")
        .upload_id("upload")
        .if_match("opaque")
        .multipart_upload(aws_sdk_s3::types::CompletedMultipartUpload::builder().build())
        .send()
        .await
        .is_err());
    assert_eq!(http.actual_requests().count(), 0);
    // Explicitly unguarded operations are not silently reinterpreted as guarded.
    client
        .put_object()
        .bucket("bucket")
        .key("key")
        .body(aws_sdk_s3::primitives::ByteStream::from_static(b"payload"))
        .send()
        .await
        .unwrap();
    assert_eq!(http.actual_requests().count(), 1);
}

#[test]
fn garage_key_format_gate_is_conservative_and_does_not_capture_values() {
    assert!(compat::garage_key_id("GK0123456789abcdef01234567"));
    for value in [
        "GK",
        "AKIA0123456789ABCDEF",
        "GK0123456789abcdef0123456Z",
        "é0123456789abcdef01234567",
    ] {
        assert!(!compat::garage_key_id(value));
    }
    let (service, _) = service(&[]);
    let client =
        compat::configure_conditions(service.client, None, Some("GK0123456789abcdef01234567"));
    assert!(!compat::conditional_writes_supported(&client));
}
