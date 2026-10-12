mod common;

use app_lib::s3::verification::object_sha256;
use common::TestContext;
use std::sync::atomic::{AtomicBool, Ordering};

#[tokio::test]
async fn cross_endpoint_relay_does_not_copy_an_unrelated_bucket() {
    use app_lib::s3::{client::build_s3_client, service};
    use aws_sdk_s3::primitives::ByteStream;
    let endpoint = std::env::var("S3_TEST_OTHER_ENDPOINT")
        .expect("Set S3_TEST_OTHER_ENDPOINT to a second isolated MinIO server");
    let mut ctx = TestContext::new().await;
    let dest = ctx.create_extra_bucket().await;
    let (other, _) = build_s3_client(
        &ctx.config.region,
        Some(&endpoint),
        None,
        Some(&ctx.config.access_key),
        Some(&ctx.config.secret_key),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    service::create_bucket(&other, &ctx.bucket, &ctx.config.region)
        .await
        .unwrap();
    service::create_bucket(&other, &dest, &ctx.config.region)
        .await
        .unwrap();
    ctx.put_object("file", b"right").await;
    other
        .put_object()
        .bucket(&ctx.bucket)
        .key("file")
        .body(ByteStream::from_static(b"wrong"))
        .send()
        .await
        .unwrap();
    let cancel = AtomicBool::new(false);
    ctx.service
        .copy_objects_with_checkpoint(
            &ctx.client,
            &ctx.bucket,
            &["file".into()],
            &other,
            &dest,
            "",
            "relay",
            &cancel,
            &AtomicBool::new(false),
            &|_| {},
            None,
            Some(""),
            true,
            false,
        )
        .await
        .unwrap();
    let result = other
        .get_object()
        .bucket(&dest)
        .key("file")
        .send()
        .await
        .unwrap()
        .body
        .collect()
        .await
        .unwrap()
        .into_bytes();
    assert_eq!(&result[..], b"right");
    // Exercise the range-GET / multipart-upload relay as well as single PUT.
    let large = vec![0x5a; 9 * 1024 * 1024];
    ctx.put_object("large", &large).await;
    ctx.service
        .copy_objects_with_checkpoint(
            &ctx.client,
            &ctx.bucket,
            &["large".into()],
            &other,
            &dest,
            "",
            "relay-large",
            &cancel,
            &AtomicBool::new(false),
            &|_| {},
            None,
            Some(""),
            true,
            false,
        )
        .await
        .unwrap();
    let source_hash = object_sha256(
        &ctx.client,
        &ctx.bucket,
        "large",
        large.len() as u64,
        None,
        &cancel,
    )
    .await
    .unwrap();
    let dest_hash = object_sha256(&other, &dest, "large", large.len() as u64, None, &cancel)
        .await
        .unwrap();
    assert_eq!(source_hash, dest_hash);
    other
        .delete_object()
        .bucket(&dest)
        .key("large")
        .send()
        .await
        .unwrap();
    other
        .delete_object()
        .bucket(&ctx.bucket)
        .key("file")
        .send()
        .await
        .unwrap();
    other
        .delete_object()
        .bucket(&dest)
        .key("file")
        .send()
        .await
        .unwrap();
    service::delete_bucket(&other, &ctx.bucket).await.unwrap();
    service::delete_bucket(&other, &dest).await.unwrap();
    ctx.cleanup().await;
}

#[tokio::test]
async fn cancelled_verification_does_not_mark_the_copy_complete() {
    let mut ctx = TestContext::new().await;
    let dest = ctx.create_extra_bucket().await;
    ctx.put_object("file", b"abc").await;
    let cancel = AtomicBool::new(false);
    let result = ctx
        .service
        .copy_objects_with_checkpoint(
            &ctx.client,
            &ctx.bucket,
            &["file".into()],
            &ctx.client,
            &dest,
            "",
            "cancel-verify",
            &cancel,
            &AtomicBool::new(false),
            &|event| {
                assert_eq!(event.files_done, 0);
                if event.current_file.starts_with("Verifying destination:") {
                    cancel.store(true, Ordering::Relaxed);
                }
            },
            None,
            Some(""),
            true,
            true,
        )
        .await;
    assert!(result.is_err());
    ctx.client
        .head_object()
        .bucket(&ctx.bucket)
        .key("file")
        .send()
        .await
        .unwrap();
    ctx.cleanup().await;
}

#[tokio::test]
async fn migration_preserves_nested_paths_and_verifies_contents() {
    let mut ctx = TestContext::new().await;
    let dest = ctx.create_extra_bucket().await;
    let cancel = AtomicBool::new(false);
    let pause = AtomicBool::new(false);
    for (key, data) in [
        ("source/a/name.txt", b"aaa"),
        ("source/b/name.txt", b"bbb"),
        ("source/space + %.txt", b"ccc"),
    ] {
        ctx.put_object(key, data).await;
    }
    ctx.service
        .copy_objects_with_checkpoint(
            &ctx.client,
            &ctx.bucket,
            &["source/".into()],
            &ctx.client,
            &dest,
            "target",
            "migration",
            &cancel,
            &pause,
            &|_| {},
            None,
            Some("source/"),
            true,
            true,
        )
        .await
        .unwrap();
    for relative in ["a/name.txt", "b/name.txt", "space + %.txt"] {
        let source_hash = object_sha256(
            &ctx.client,
            &ctx.bucket,
            &format!("source/{relative}"),
            3,
            None,
            &cancel,
        )
        .await
        .unwrap();
        let dest_hash = object_sha256(
            &ctx.client,
            &dest,
            &format!("target/{relative}"),
            3,
            None,
            &cancel,
        )
        .await
        .unwrap();
        assert_eq!(source_hash, dest_hash);
    }
    assert!(ctx
        .client
        .head_object()
        .bucket(&dest)
        .key("target/name.txt")
        .send()
        .await
        .is_err());
    ctx.cleanup().await;
}

#[tokio::test]
async fn folder_copy_keeps_folder_name_and_resumes() {
    let mut ctx = TestContext::new().await;
    let dest = ctx.create_extra_bucket().await;
    ctx.put_object("parent/folder/deep/file", b"abc").await;
    let cancel = AtomicBool::new(false);
    let pause = AtomicBool::new(true);
    let keys = ["parent/folder/".into()];
    let checkpoint = ctx
        .service
        .copy_objects_with_checkpoint(
            &ctx.client,
            &ctx.bucket,
            &keys,
            &ctx.client,
            &dest,
            "dest/",
            "pause",
            &cancel,
            &pause,
            &|_| {},
            None,
            None,
            true,
            true,
        )
        .await
        .unwrap()
        .unwrap();
    pause.store(false, Ordering::Relaxed);
    assert!(ctx
        .service
        .copy_objects_with_checkpoint(
            &ctx.client,
            &ctx.bucket,
            &keys,
            &ctx.client,
            &dest,
            "dest/",
            "resume",
            &cancel,
            &pause,
            &|_| {},
            Some(&checkpoint),
            None,
            true,
            true,
        )
        .await
        .unwrap()
        .is_none());
    ctx.client
        .head_object()
        .bucket(&dest)
        .key("dest/folder/deep/file")
        .send()
        .await
        .unwrap();
    ctx.cleanup().await;
}

#[tokio::test]
async fn collisions_fail_before_any_destination_write() {
    let mut ctx = TestContext::new().await;
    let dest = ctx.create_extra_bucket().await;
    ctx.put_object("a/same", b"aaa").await;
    ctx.put_object("b/same", b"bbb").await;
    let result = ctx
        .service
        .copy_objects(
            &ctx.client,
            &ctx.bucket,
            &["a/same".into(), "b/same".into()],
            &ctx.client,
            &dest,
            "",
            "collision",
            &AtomicBool::new(false),
            &AtomicBool::new(false),
            &|_| {},
        )
        .await;
    assert!(result.is_err());
    assert!(ctx
        .client
        .head_object()
        .bucket(&dest)
        .key("same")
        .send()
        .await
        .is_err());
    ctx.cleanup().await;
}

#[tokio::test]
async fn verification_detects_same_size_differences_and_changed_objects() {
    let ctx = TestContext::new().await;
    let cancel = AtomicBool::new(false);
    ctx.put_object("a", b"abc").await;
    ctx.put_object("b", b"abd").await;
    let head = ctx
        .client
        .head_object()
        .bucket(&ctx.bucket)
        .key("a")
        .send()
        .await
        .unwrap();
    let a = object_sha256(&ctx.client, &ctx.bucket, "a", 3, head.e_tag(), &cancel)
        .await
        .unwrap();
    let b = object_sha256(&ctx.client, &ctx.bucket, "b", 3, None, &cancel)
        .await
        .unwrap();
    assert_ne!(a, b);
    assert_eq!(
        a,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    ctx.put_object("a", b"xyz").await;
    assert!(
        object_sha256(&ctx.client, &ctx.bucket, "a", 3, head.e_tag(), &cancel)
            .await
            .is_err()
    );
    assert!(
        object_sha256(&ctx.client, &ctx.bucket, "a", 4, None, &cancel)
            .await
            .is_err()
    );
    cancel.store(true, Ordering::Relaxed);
    assert!(
        object_sha256(&ctx.client, &ctx.bucket, "a", 3, None, &cancel)
            .await
            .is_err()
    );
    ctx.cleanup().await;
}

#[tokio::test]
async fn multipart_etags_do_not_determine_content_equality() {
    use aws_sdk_s3::{
        primitives::ByteStream,
        types::{CompletedMultipartUpload, CompletedPart},
    };
    let ctx = TestContext::new().await;
    let data = b"same contents";
    ctx.put_object("single", data).await;
    let upload = ctx
        .client
        .create_multipart_upload()
        .bucket(&ctx.bucket)
        .key("multipart")
        .send()
        .await
        .unwrap();
    let id = upload.upload_id().unwrap();
    let part = ctx
        .client
        .upload_part()
        .bucket(&ctx.bucket)
        .key("multipart")
        .upload_id(id)
        .part_number(1)
        .body(ByteStream::from_static(data))
        .send()
        .await
        .unwrap();
    ctx.client
        .complete_multipart_upload()
        .bucket(&ctx.bucket)
        .key("multipart")
        .upload_id(id)
        .multipart_upload(
            CompletedMultipartUpload::builder()
                .parts(
                    CompletedPart::builder()
                        .part_number(1)
                        .e_tag(part.e_tag().unwrap())
                        .build(),
                )
                .build(),
        )
        .send()
        .await
        .unwrap();
    let single = ctx
        .client
        .head_object()
        .bucket(&ctx.bucket)
        .key("single")
        .send()
        .await
        .unwrap();
    let multipart = ctx
        .client
        .head_object()
        .bucket(&ctx.bucket)
        .key("multipart")
        .send()
        .await
        .unwrap();
    assert_ne!(single.e_tag(), multipart.e_tag());
    let cancel = AtomicBool::new(false);
    let a = object_sha256(
        &ctx.client,
        &ctx.bucket,
        "single",
        data.len() as u64,
        single.e_tag(),
        &cancel,
    )
    .await
    .unwrap();
    let b = object_sha256(
        &ctx.client,
        &ctx.bucket,
        "multipart",
        data.len() as u64,
        multipart.e_tag(),
        &cancel,
    )
    .await
    .unwrap();
    assert_eq!(a, b);
    ctx.cleanup().await;
}
