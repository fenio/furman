//! Run against an isolated TLS MinIO endpoint; never disable certificate checks.
use app_lib::read_s3_file;
use app_lib::s3::{
    service::{self, S3Service},
    sse_c,
    verification::object_sha256,
};
use aws_sdk_s3::{primitives::ByteStream, Client};
use aws_smithy_http_client::{tls, Builder};
use base64::{engine::general_purpose::STANDARD, Engine};
use std::sync::atomic::AtomicBool;

fn client() -> Client {
    let endpoint =
        std::env::var("SSE_C_TEST_ENDPOINT").expect("Set SSE_C_TEST_ENDPOINT to TLS MinIO");
    let certificate = std::fs::read(
        std::env::var("SSE_C_TEST_CA").expect("Set SSE_C_TEST_CA to the test CA PEM"),
    )
    .unwrap();
    let tls_context = tls::TlsContext::builder()
        .with_trust_store(tls::TrustStore::empty().with_pem_certificate(certificate))
        .build()
        .unwrap();
    let http = Builder::new().build_with_connector_fn(move |_, _| {
        aws_smithy_http_client::Connector::builder()
            .tls_provider(tls::Provider::Rustls(
                tls::rustls_provider::CryptoMode::AwsLc,
            ))
            .tls_context(tls_context.clone())
            .build()
    });
    Client::from_conf(
        aws_sdk_s3::Config::builder()
            .behavior_version_latest()
            .retry_config(aws_sdk_s3::config::retry::RetryConfig::standard())
            .region(aws_sdk_s3::config::Region::new("us-east-1"))
            .credentials_provider(aws_credential_types::Credentials::new(
                "minioadmin",
                "minioadmin",
                None,
                None,
                "test",
            ))
            .endpoint_url(endpoint)
            .force_path_style(true)
            .http_client(http)
            .build(),
    )
}

#[tokio::test]
async fn encrypted_reads_versions_and_rekeyed_copies_work() {
    let plain = client();
    let source = sse_c::configure(plain.clone(), Some(&STANDARD.encode([42u8; 32]))).unwrap();
    let destination = sse_c::configure(plain.clone(), Some(&STANDARD.encode([43u8; 32]))).unwrap();
    let wrong = sse_c::configure(plain.clone(), Some(&STANDARD.encode([44u8; 32]))).unwrap();
    let bucket = format!("sse-source-{}", uuid::Uuid::new_v4());
    let dest_bucket = format!("sse-dest-{}", uuid::Uuid::new_v4());
    service::create_bucket(&plain, &bucket, "us-east-1")
        .await
        .unwrap();
    service::create_bucket(&plain, &dest_bucket, "us-east-1")
        .await
        .unwrap();
    let service = S3Service::new(source.clone(), bucket.clone());
    service
        .put_bucket_versioning(true, None, None)
        .await
        .unwrap();
    source
        .put_object()
        .bucket(&bucket)
        .key("folder/file")
        .body(ByteStream::from_static(b"first"))
        .send()
        .await
        .unwrap();
    source
        .put_object()
        .bucket(&bucket)
        .key("folder/file")
        .body(ByteStream::from_static(b"second"))
        .send()
        .await
        .unwrap();
    let properties = service.head_object("folder/file").await.unwrap();
    assert_eq!(properties.size, 6);
    assert_eq!(
        read_s3_file(&source, &bucket, &format!("s3://{bucket}/folder/file"))
            .await
            .unwrap()
            .text
            .as_deref(),
        Some("second")
    );
    assert!(
        read_s3_file(&wrong, &bucket, &format!("s3://{bucket}/folder/file"))
            .await
            .is_err()
    );
    assert!(
        read_s3_file(&source, &bucket, "s3://wrong-bucket/folder/file")
            .await
            .is_err()
    );
    source
        .put_object()
        .bucket(&bucket)
        .key("empty")
        .body(ByteStream::from_static(b""))
        .send()
        .await
        .unwrap();
    assert_eq!(
        read_s3_file(&source, &bucket, &format!("s3://{bucket}/empty"))
            .await
            .unwrap()
            .text
            .as_deref(),
        Some("")
    );
    source
        .put_object()
        .bucket(&bucket)
        .key("binary")
        .body(ByteStream::from_static(b"\0\xff"))
        .send()
        .await
        .unwrap();
    assert!(
        read_s3_file(&source, &bucket, &format!("s3://{bucket}/binary"))
            .await
            .unwrap()
            .text
            .is_none()
    );
    assert_eq!(
        properties.server_side_encryption.as_deref(),
        Some("SSE-C (AES256)")
    );
    assert!(S3Service::new(plain.clone(), bucket.clone())
        .head_object("folder/file")
        .await
        .is_err());
    assert!(S3Service::new(wrong.clone(), bucket.clone())
        .head_object("folder/file")
        .await
        .is_err());
    let versions = service.list_object_versions("folder/file").await.unwrap();
    assert_eq!(versions.len(), 2);
    let old = versions.iter().find(|v| !v.is_latest).unwrap();
    let data = source
        .get_object()
        .bucket(&bucket)
        .key("folder/file")
        .version_id(&old.version_id)
        .send()
        .await
        .unwrap()
        .body
        .collect()
        .await
        .unwrap()
        .into_bytes();
    assert_eq!(&data[..], b"first");

    let cancel = AtomicBool::new(false);
    let downloaded = tempfile::tempdir().unwrap();
    service
        .download(
            &["folder/file".into()],
            downloaded.path().to_str().unwrap(),
            "sse-download",
            &cancel,
            &AtomicBool::new(false),
            &|_| {},
            None,
        )
        .await
        .unwrap();
    assert_eq!(
        std::fs::read(downloaded.path().join("file")).unwrap(),
        b"second"
    );
    assert!(service.presign_url("folder/file", 60).await.is_err());
    service
        .copy_objects_with_checkpoint(
            &source,
            &bucket,
            &["folder/file".into()],
            &destination,
            &dest_bucket,
            "copied",
            "sse-rekey",
            &cancel,
            &AtomicBool::new(false),
            &|_| {},
            None,
            Some("folder/"),
            true,
            true,
        )
        .await
        .unwrap();
    let source_hash = object_sha256(&source, &bucket, "folder/file", 6, None, &cancel)
        .await
        .unwrap();
    let dest_hash = object_sha256(&destination, &dest_bucket, "copied/file", 6, None, &cancel)
        .await
        .unwrap();
    assert_eq!(source_hash, dest_hash);
    assert_eq!(
        read_s3_file(
            &destination,
            &dest_bucket,
            &format!("s3://{dest_bucket}/copied/file")
        )
        .await
        .unwrap()
        .text
        .as_deref(),
        Some("second")
    );
    assert!(
        object_sha256(&source, &dest_bucket, "copied/file", 6, None, &cancel)
            .await
            .is_err()
    );

    // Destination settings also support explicit decryption or encryption.
    service
        .copy_objects_with_checkpoint(
            &source,
            &bucket,
            &["folder/file".into()],
            &plain,
            &dest_bucket,
            "plain",
            "sse-to-plain",
            &cancel,
            &AtomicBool::new(false),
            &|_| {},
            None,
            Some("folder/"),
            true,
            true,
        )
        .await
        .unwrap();
    assert_eq!(
        object_sha256(&plain, &dest_bucket, "plain/file", 6, None, &cancel)
            .await
            .unwrap(),
        source_hash
    );
    plain
        .put_object()
        .bucket(&bucket)
        .key("plain-src")
        .body(ByteStream::from_static(b"plain"))
        .send()
        .await
        .unwrap();
    source
        .get_object()
        .bucket(&bucket)
        .key("plain-src")
        .send()
        .await
        .expect("SSE-C connection must also read unencrypted objects");
    let plain_properties = service.head_object("plain-src").await.unwrap();
    assert_eq!(plain_properties.size, 5);
    assert_eq!(plain_properties.server_side_encryption, None);
    assert_eq!(
        object_sha256(&source, &bucket, "plain-src", 5, None, &cancel)
            .await
            .unwrap(),
        object_sha256(&plain, &bucket, "plain-src", 5, None, &cancel)
            .await
            .unwrap()
    );
    S3Service::new(plain.clone(), bucket.clone())
        .copy_objects_with_checkpoint(
            &plain,
            &bucket,
            &["plain-src".into()],
            &destination,
            &dest_bucket,
            "encrypted",
            "plain-to-sse",
            &cancel,
            &AtomicBool::new(false),
            &|_| {},
            None,
            Some(""),
            true,
            true,
        )
        .await
        .unwrap();
    assert!(plain
        .get_object()
        .bucket(&dest_bucket)
        .key("encrypted/plain-src")
        .send()
        .await
        .is_err());
    assert_eq!(
        object_sha256(
            &destination,
            &dest_bucket,
            "encrypted/plain-src",
            5,
            None,
            &cancel
        )
        .await
        .unwrap(),
        object_sha256(&plain, &bucket, "plain-src", 5, None, &cancel)
            .await
            .unwrap()
    );

    // Exercise multipart encrypted upload and ranged download/upload relay.
    let large = vec![0x5a; 9 * 1024 * 1024];
    let upload = source
        .create_multipart_upload()
        .bucket(&bucket)
        .key("large")
        .send()
        .await
        .unwrap();
    let id = upload.upload_id().unwrap();
    let part = source
        .upload_part()
        .bucket(&bucket)
        .key("large")
        .upload_id(id)
        .part_number(1)
        .body(ByteStream::from(large.clone()))
        .send()
        .await
        .unwrap();
    source
        .complete_multipart_upload()
        .bucket(&bucket)
        .key("large")
        .upload_id(id)
        .multipart_upload(
            aws_sdk_s3::types::CompletedMultipartUpload::builder()
                .parts(
                    aws_sdk_s3::types::CompletedPart::builder()
                        .part_number(1)
                        .e_tag(part.e_tag().unwrap())
                        .build(),
                )
                .build(),
        )
        .send()
        .await
        .unwrap();
    service
        .copy_objects_with_checkpoint(
            &source,
            &bucket,
            &["large".into()],
            &destination,
            &dest_bucket,
            "",
            "sse-multipart",
            &cancel,
            &AtomicBool::new(false),
            &|_| {},
            None,
            Some(""),
            true,
            true,
        )
        .await
        .unwrap();
    assert_eq!(
        object_sha256(&source, &bucket, "large", large.len() as u64, None, &cancel)
            .await
            .unwrap(),
        object_sha256(
            &destination,
            &dest_bucket,
            "large",
            large.len() as u64,
            None,
            &cancel
        )
        .await
        .unwrap()
    );

    service
        .restore_version("folder/file", &old.version_id)
        .await
        .unwrap();
    assert_eq!(service.head_object("folder/file").await.unwrap().size, 5);
    assert!(
        read_s3_file(&source, &bucket, &format!("s3://{bucket}/large"))
            .await
            .is_err()
    );
    assert_eq!(
        read_s3_file(&source, &bucket, &format!("s3://{bucket}/plain-src"))
            .await
            .unwrap()
            .text
            .as_deref(),
        Some("plain")
    );
    for key in ["folder/file", "large", "plain-src", "empty", "binary"] {
        for version in service.list_object_versions(key).await.unwrap() {
            plain
                .delete_object()
                .bucket(&bucket)
                .key(key)
                .version_id(version.version_id)
                .send()
                .await
                .unwrap();
        }
    }
    for key in ["copied/file", "large", "plain/file", "encrypted/plain-src"] {
        plain
            .delete_object()
            .bucket(&dest_bucket)
            .key(key)
            .send()
            .await
            .unwrap();
    }
    service::delete_bucket(&plain, &bucket).await.unwrap();
    service::delete_bucket(&plain, &dest_bucket).await.unwrap();
}
