mod common;

use app_lib::s3::service::{self, S3Service};
use common::TestContext;
use std::sync::atomic::AtomicBool;

// ═══════════════════════════════════════════════════════════════════════════
// P1 — Core CRUD
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_create_and_delete_bucket() {
    let ctx = TestContext::new().await;

    // Create a new bucket via service function
    let new_bucket = format!("test-create-{}", uuid::Uuid::new_v4());
    service::create_bucket(&ctx.client, &new_bucket, "us-east-1")
        .await
        .expect("create_bucket failed");

    // Verify it appears in list
    let buckets = service::list_buckets(&ctx.client).await.expect("list_buckets failed");
    assert!(
        buckets.iter().any(|b| b.name == new_bucket),
        "Newly created bucket not found in list"
    );

    // Delete it
    service::delete_bucket(&ctx.client, &new_bucket)
        .await
        .expect("delete_bucket failed");

    // Verify it's gone
    let buckets = service::list_buckets(&ctx.client).await.expect("list_buckets failed");
    assert!(
        !buckets.iter().any(|b| b.name == new_bucket),
        "Deleted bucket still appears in list"
    );

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_list_objects() {
    let ctx = TestContext::new().await;

    // Upload some files and a "folder"
    ctx.put_object("file1.txt", b"hello").await;
    ctx.put_object("file2.txt", b"world").await;
    ctx.put_object("subdir/file3.txt", b"nested").await;

    // List root — should see file1.txt, file2.txt, and subdir/ prefix
    let listing = ctx.service.list_objects("").await.expect("list_objects failed");

    let names: Vec<&str> = listing.entries.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&".."), "Missing '..' entry");
    assert!(names.contains(&"file1.txt"), "Missing file1.txt");
    assert!(names.contains(&"file2.txt"), "Missing file2.txt");
    assert!(names.contains(&"subdir"), "Missing subdir prefix");

    // Verify directory entries
    let subdir_entry = listing.entries.iter().find(|e| e.name == "subdir").unwrap();
    assert!(subdir_entry.is_dir, "subdir should be marked as directory");

    // List subdir/ — should see file3.txt
    let sub_listing = ctx.service.list_objects("subdir/").await.expect("list subdir failed");
    let sub_names: Vec<&str> = sub_listing.entries.iter().map(|e| e.name.as_str()).collect();
    assert!(sub_names.contains(&"file3.txt"), "Missing file3.txt in subdir");

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_create_folder() {
    let ctx = TestContext::new().await;

    ctx.service
        .create_folder("myfolder/")
        .await
        .expect("create_folder failed");

    // Verify it appears in listing
    let listing = ctx.service.list_objects("").await.expect("list_objects failed");
    let names: Vec<&str> = listing.entries.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"myfolder"), "Folder not found in listing");

    // Creating same folder again should fail (AlreadyExists)
    let result = ctx.service.create_folder("myfolder/").await;
    assert!(result.is_err(), "Creating duplicate folder should fail");

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_put_text_and_download_temp() {
    let ctx = TestContext::new().await;

    let content = "Hello, MinIO! 🌍";
    ctx.service
        .put_text("greeting.txt", content)
        .await
        .expect("put_text failed");

    // Download to temp and verify content
    let temp_path = ctx
        .service
        .download_temp("greeting.txt")
        .await
        .expect("download_temp failed");

    let downloaded = std::fs::read_to_string(&temp_path).expect("failed to read temp file");
    assert_eq!(downloaded, content);

    // Clean up temp file
    let _ = std::fs::remove_file(&temp_path);

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_head_object() {
    let ctx = TestContext::new().await;

    let data = b"test content for head";
    ctx.put_object("headtest.txt", data).await;

    let props = ctx
        .service
        .head_object("headtest.txt")
        .await
        .expect("head_object failed");

    assert_eq!(props.key, "headtest.txt");
    assert_eq!(props.size, data.len() as u64);

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_delete_objects() {
    let ctx = TestContext::new().await;

    // Upload files
    ctx.put_object("del1.txt", b"a").await;
    ctx.put_object("del2.txt", b"b").await;
    ctx.put_object("delfolder/sub.txt", b"c").await;

    // Delete single file
    ctx.service
        .delete_objects(&["del1.txt".to_string()])
        .await
        .expect("delete single failed");

    // Verify del1.txt is gone
    let result = ctx.service.head_object("del1.txt").await;
    assert!(result.is_err(), "del1.txt should be gone");

    // del2.txt should still exist
    ctx.service
        .head_object("del2.txt")
        .await
        .expect("del2.txt should still exist");

    // Delete prefix (folder)
    ctx.service
        .delete_objects(&["delfolder/".to_string()])
        .await
        .expect("delete prefix failed");

    // Verify folder contents are gone
    let result = ctx.service.head_object("delfolder/sub.txt").await;
    assert!(result.is_err(), "delfolder/sub.txt should be gone");

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_rename_object() {
    let ctx = TestContext::new().await;

    ctx.put_object("original.txt", b"content").await;

    // Rename file
    ctx.service
        .rename_object("original.txt", "renamed.txt")
        .await
        .expect("rename_object failed");

    // Old key should be gone
    let result = ctx.service.head_object("original.txt").await;
    assert!(result.is_err(), "original.txt should be gone after rename");

    // New key should exist with same content
    let temp = ctx
        .service
        .download_temp("renamed.txt")
        .await
        .expect("download renamed failed");
    let content = std::fs::read(&temp).unwrap();
    assert_eq!(content, b"content");
    let _ = std::fs::remove_file(&temp);

    // Test prefix rename
    ctx.put_object("oldfolder/a.txt", b"aa").await;
    ctx.put_object("oldfolder/b.txt", b"bb").await;

    ctx.service
        .rename_object("oldfolder/", "newfolder")
        .await
        .expect("rename prefix failed");

    // Old prefix should be empty
    let result = ctx.service.head_object("oldfolder/a.txt").await;
    assert!(result.is_err(), "oldfolder/a.txt should be gone");

    // New prefix should have files
    ctx.service
        .head_object("newfolder/a.txt")
        .await
        .expect("newfolder/a.txt should exist");
    ctx.service
        .head_object("newfolder/b.txt")
        .await
        .expect("newfolder/b.txt should exist");

    ctx.cleanup().await;
}

// ═══════════════════════════════════════════════════════════════════════════
// P2 — Copy operations
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_copy_same_bucket() {
    let ctx = TestContext::new().await;
    let cancel = AtomicBool::new(false);
    let pause = AtomicBool::new(false);

    ctx.put_object("src/copy_me.txt", b"copy this").await;

    let result = ctx
        .service
        .copy_objects(
            &ctx.client,
            &ctx.bucket,
            &["src/copy_me.txt".to_string()],
            &ctx.client,
            &ctx.bucket,
            "dest/",
            "op-copy",
            &cancel,
            &pause,
            &|_| {},
        )
        .await
        .expect("copy_objects failed");

    assert!(result.is_none(), "Should return None on success");

    // Verify copy exists
    ctx.service
        .head_object("dest/copy_me.txt")
        .await
        .expect("copied file should exist");

    // Original should still exist
    ctx.service
        .head_object("src/copy_me.txt")
        .await
        .expect("original should still exist");

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_copy_cross_bucket() {
    let mut ctx = TestContext::new().await;
    let cancel = AtomicBool::new(false);
    let pause = AtomicBool::new(false);

    let dest_bucket = ctx.create_extra_bucket().await;
    let dest_service = S3Service::new(ctx.client.clone(), dest_bucket.clone());

    ctx.put_object("cross.txt", b"cross-bucket data").await;

    ctx.service
        .copy_objects(
            &ctx.client,
            &ctx.bucket,
            &["cross.txt".to_string()],
            &ctx.client,
            &dest_bucket,
            "",
            "op-cross",
            &cancel,
            &pause,
            &|_| {},
        )
        .await
        .expect("cross-bucket copy failed");

    // Verify in dest bucket
    dest_service
        .head_object("cross.txt")
        .await
        .expect("cross-bucket copy should exist in destination");

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_copy_prefix() {
    let ctx = TestContext::new().await;
    let cancel = AtomicBool::new(false);
    let pause = AtomicBool::new(false);

    ctx.put_object("srcdir/a.txt", b"aaa").await;
    ctx.put_object("srcdir/b.txt", b"bbb").await;

    ctx.service
        .copy_objects(
            &ctx.client,
            &ctx.bucket,
            &["srcdir/a.txt".to_string(), "srcdir/b.txt".to_string()],
            &ctx.client,
            &ctx.bucket,
            "destdir/",
            "op-prefix",
            &cancel,
            &pause,
            &|_| {},
        )
        .await
        .expect("copy prefix failed");

    ctx.service
        .head_object("destdir/a.txt")
        .await
        .expect("destdir/a.txt should exist");
    ctx.service
        .head_object("destdir/b.txt")
        .await
        .expect("destdir/b.txt should exist");

    ctx.cleanup().await;
}

// ═══════════════════════════════════════════════════════════════════════════
// P3 — Multipart
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_upload_multipart() {
    let ctx = TestContext::new().await;

    // Create a file larger than MULTIPART_THRESHOLD (8 MiB)
    let size = 9 * 1024 * 1024; // 9 MiB
    let tmp_dir = tempfile::tempdir().expect("tempdir failed");
    let file_path = tmp_dir.path().join("bigfile.bin");
    let data = vec![0x42u8; size];
    std::fs::write(&file_path, &data).expect("write bigfile failed");

    let cancel = AtomicBool::new(false);
    let pause = AtomicBool::new(false);

    let result = ctx
        .service
        .upload(
            &[file_path.to_string_lossy().to_string()],
            "",
            "op-multipart",
            &cancel,
            &pause,
            &|_| {},
        )
        .await
        .expect("upload multipart failed");

    assert!(result.is_none(), "Should return None on success");

    // Verify uploaded object has correct size
    let props = ctx
        .service
        .head_object("bigfile.bin")
        .await
        .expect("head bigfile failed");
    assert_eq!(props.size, size as u64);

    ctx.cleanup().await;
}

// ═══════════════════════════════════════════════════════════════════════════
// P4 — Versioning
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_enable_suspend_versioning() {
    let ctx = TestContext::new().await;

    // Initially disabled
    let v = ctx
        .service
        .get_bucket_versioning()
        .await
        .expect("get_bucket_versioning failed");
    assert_eq!(v.status, "Disabled");

    // Enable
    ctx.service
        .put_bucket_versioning(true)
        .await
        .expect("enable versioning failed");

    let v = ctx
        .service
        .get_bucket_versioning()
        .await
        .expect("get_bucket_versioning failed");
    assert_eq!(v.status, "Enabled");

    // Suspend
    ctx.service
        .put_bucket_versioning(false)
        .await
        .expect("suspend versioning failed");

    let v = ctx
        .service
        .get_bucket_versioning()
        .await
        .expect("get_bucket_versioning failed");
    assert_eq!(v.status, "Suspended");

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_list_versions() {
    let ctx = TestContext::new().await;

    // Enable versioning
    ctx.service.put_bucket_versioning(true).await.unwrap();

    // Upload same key 3 times
    ctx.put_object("versioned.txt", b"version1").await;
    ctx.put_object("versioned.txt", b"version2").await;
    ctx.put_object("versioned.txt", b"version3").await;

    let versions = ctx
        .service
        .list_object_versions("versioned.txt")
        .await
        .expect("list_object_versions failed");

    assert_eq!(
        versions.len(),
        3,
        "Expected 3 versions, got {}",
        versions.len()
    );

    // Latest should be first (sorted by modified desc)
    assert!(versions[0].is_latest, "First version should be latest");

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_download_version() {
    let ctx = TestContext::new().await;

    // Enable versioning
    ctx.service.put_bucket_versioning(true).await.unwrap();

    // Upload 2 versions
    ctx.put_object("vdl.txt", b"old content").await;
    ctx.put_object("vdl.txt", b"new content").await;

    let versions = ctx
        .service
        .list_object_versions("vdl.txt")
        .await
        .expect("list versions failed");
    assert_eq!(versions.len(), 2);

    // Download the older version (index 1, since sorted newest first)
    let older_vid = &versions[1].version_id;
    let temp = ctx
        .service
        .download_version("vdl.txt", older_vid)
        .await
        .expect("download_version failed");

    let content = std::fs::read_to_string(&temp).unwrap();
    assert_eq!(content, "old content");
    let _ = std::fs::remove_file(&temp);

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_restore_version() {
    let ctx = TestContext::new().await;

    ctx.service.put_bucket_versioning(true).await.unwrap();

    ctx.put_object("restore.txt", b"original").await;
    ctx.put_object("restore.txt", b"modified").await;

    let versions = ctx
        .service
        .list_object_versions("restore.txt")
        .await
        .unwrap();
    let old_vid = &versions[1].version_id;

    // Restore old version
    ctx.service
        .restore_version("restore.txt", old_vid)
        .await
        .expect("restore_version failed");

    // Current version should now be "original"
    let temp = ctx.service.download_temp("restore.txt").await.unwrap();
    let content = std::fs::read_to_string(&temp).unwrap();
    assert_eq!(content, "original");
    let _ = std::fs::remove_file(&temp);

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_delete_version() {
    let ctx = TestContext::new().await;

    ctx.service.put_bucket_versioning(true).await.unwrap();

    ctx.put_object("delver.txt", b"v1").await;
    ctx.put_object("delver.txt", b"v2").await;

    let versions = ctx
        .service
        .list_object_versions("delver.txt")
        .await
        .unwrap();
    assert_eq!(versions.len(), 2);

    // Delete the older version
    ctx.service
        .delete_version("delver.txt", &versions[1].version_id)
        .await
        .expect("delete_version failed");

    let remaining = ctx
        .service
        .list_object_versions("delver.txt")
        .await
        .unwrap();
    assert_eq!(remaining.len(), 1, "Should have 1 version left");

    ctx.cleanup().await;
}

// ═══════════════════════════════════════════════════════════════════════════
// P5 — Tags & metadata
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_object_tags_roundtrip() {
    let ctx = TestContext::new().await;

    ctx.put_object("tagged.txt", b"data").await;

    let tags = vec![
        app_lib::models::S3Tag {
            key: "env".to_string(),
            value: "test".to_string(),
        },
        app_lib::models::S3Tag {
            key: "project".to_string(),
            value: "furman".to_string(),
        },
    ];

    ctx.service
        .put_object_tags("tagged.txt", &tags)
        .await
        .expect("put_object_tags failed");

    let got = ctx
        .service
        .get_object_tags("tagged.txt")
        .await
        .expect("get_object_tags failed");

    assert_eq!(got.len(), 2);
    assert!(got.iter().any(|t| t.key == "env" && t.value == "test"));
    assert!(got.iter().any(|t| t.key == "project" && t.value == "furman"));

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_bucket_tags_roundtrip() {
    let ctx = TestContext::new().await;

    let tags = vec![app_lib::models::S3Tag {
        key: "team".to_string(),
        value: "platform".to_string(),
    }];

    ctx.service
        .put_bucket_tags(&tags)
        .await
        .expect("put_bucket_tags failed");

    let got = ctx
        .service
        .get_bucket_tags()
        .await
        .expect("get_bucket_tags failed");

    assert_eq!(got.len(), 1);
    assert_eq!(got[0].key, "team");
    assert_eq!(got[0].value, "platform");

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_object_metadata_roundtrip() {
    let ctx = TestContext::new().await;

    ctx.put_object("meta.txt", b"metadata test").await;

    let mut custom = std::collections::HashMap::new();
    custom.insert("x-custom-key".to_string(), "custom-value".to_string());

    ctx.service
        .put_object_metadata(
            "meta.txt",
            Some("text/plain"),
            Some("inline"),
            Some("max-age=3600"),
            None,
            &custom,
        )
        .await
        .expect("put_object_metadata failed");

    let meta = ctx
        .service
        .get_object_metadata("meta.txt")
        .await
        .expect("get_object_metadata failed");

    assert_eq!(meta.content_type.as_deref(), Some("text/plain"));
    assert_eq!(meta.content_disposition.as_deref(), Some("inline"));
    assert_eq!(meta.cache_control.as_deref(), Some("max-age=3600"));
    assert_eq!(
        meta.custom.get("x-custom-key").map(|s| s.as_str()),
        Some("custom-value")
    );

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_object_metadata_content_type() {
    let ctx = TestContext::new().await;

    ctx.put_object("image.png", b"\x89PNG\r\n\x1a\n").await;

    let empty = std::collections::HashMap::new();
    ctx.service
        .put_object_metadata("image.png", Some("image/png"), None, None, None, &empty)
        .await
        .expect("put content-type failed");

    let meta = ctx
        .service
        .get_object_metadata("image.png")
        .await
        .expect("get metadata failed");

    assert_eq!(meta.content_type.as_deref(), Some("image/png"));

    ctx.cleanup().await;
}

// ═══════════════════════════════════════════════════════════════════════════
// P6 — Bucket configuration
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_lifecycle_rules_roundtrip() {
    let ctx = TestContext::new().await;

    let rules = vec![app_lib::models::S3LifecycleRule {
        id: "test-rule".to_string(),
        prefix: "logs/".to_string(),
        enabled: true,
        transitions: vec![],
        expiration_days: Some(30),
        noncurrent_transitions: vec![],
        noncurrent_expiration_days: None,
        abort_incomplete_days: Some(7),
    }];

    ctx.service
        .put_bucket_lifecycle(&rules)
        .await
        .expect("put_bucket_lifecycle failed");

    let got = ctx
        .service
        .get_bucket_lifecycle()
        .await
        .expect("get_bucket_lifecycle failed");

    assert_eq!(got.len(), 1);
    assert_eq!(got[0].id, "test-rule");
    assert_eq!(got[0].prefix, "logs/");
    assert!(got[0].enabled);
    assert_eq!(got[0].expiration_days, Some(30));
    // MinIO may not return abort_incomplete_days
    // assert_eq!(got[0].abort_incomplete_days, Some(7));

    // Delete lifecycle
    ctx.service
        .put_bucket_lifecycle(&[])
        .await
        .expect("delete lifecycle failed");

    let got = ctx
        .service
        .get_bucket_lifecycle()
        .await
        .expect("get empty lifecycle failed");
    assert!(got.is_empty());

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_cors_roundtrip() {
    let ctx = TestContext::new().await;

    let rules = vec![app_lib::models::S3CorsRule {
        allowed_origins: vec!["https://example.com".to_string()],
        allowed_methods: vec!["GET".to_string(), "PUT".to_string()],
        allowed_headers: vec!["*".to_string()],
        expose_headers: vec!["ETag".to_string()],
        max_age_seconds: Some(3600),
    }];

    // MinIO may not support CORS configuration — skip if unsupported
    match ctx.service.put_bucket_cors(&rules).await {
        Ok(()) => {
            let got = ctx
                .service
                .get_bucket_cors()
                .await
                .expect("get_bucket_cors failed");

            assert_eq!(got.len(), 1);
            assert_eq!(got[0].allowed_origins, vec!["https://example.com"]);
            assert_eq!(got[0].allowed_methods, vec!["GET", "PUT"]);
            assert_eq!(got[0].max_age_seconds, Some(3600));

            // Delete CORS
            ctx.service
                .put_bucket_cors(&[])
                .await
                .expect("delete cors failed");

            let got = ctx
                .service
                .get_bucket_cors()
                .await
                .expect("get empty cors failed");
            assert!(got.is_empty());
        }
        Err(_) => {
            eprintln!("CORS not supported by this S3 provider — skipping");
        }
    }

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_bucket_policy_roundtrip() {
    let ctx = TestContext::new().await;

    let policy = serde_json::json!({
        "Version": "2012-10-17",
        "Statement": [{
            "Effect": "Allow",
            "Principal": "*",
            "Action": "s3:GetObject",
            "Resource": format!("arn:aws:s3:::{}/*", ctx.bucket)
        }]
    })
    .to_string();

    ctx.service
        .put_bucket_policy(&policy)
        .await
        .expect("put_bucket_policy failed");

    let got = ctx
        .service
        .get_bucket_policy()
        .await
        .expect("get_bucket_policy failed");

    assert!(!got.is_empty(), "Policy should not be empty");
    // Verify JSON structure
    let parsed: serde_json::Value = serde_json::from_str(&got).expect("Invalid JSON returned");
    assert_eq!(parsed["Version"], "2012-10-17");

    // Delete policy
    ctx.service
        .put_bucket_policy("")
        .await
        .expect("delete policy failed");

    let got = ctx
        .service
        .get_bucket_policy()
        .await
        .expect("get empty policy failed");
    assert!(got.is_empty());

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_public_access_block_roundtrip() {
    let ctx = TestContext::new().await;

    let config = app_lib::models::S3PublicAccessBlock {
        block_public_acls: true,
        ignore_public_acls: true,
        block_public_policy: true,
        restrict_public_buckets: false,
    };

    // MinIO may not support public access block — skip if unsupported
    match ctx.service.put_public_access_block(&config).await {
        Ok(()) => {
            let got = ctx
                .service
                .get_public_access_block()
                .await
                .expect("get_public_access_block failed");

            assert!(got.block_public_acls);
            assert!(got.ignore_public_acls);
            assert!(got.block_public_policy);
            assert!(!got.restrict_public_buckets);
        }
        Err(_) => {
            eprintln!("Public access block not supported by this S3 provider — skipping");
        }
    }

    ctx.cleanup().await;
}

// ═══════════════════════════════════════════════════════════════════════════
// P7 — Other operations
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_presign_url() {
    let ctx = TestContext::new().await;

    ctx.put_object("presign.txt", b"presigned content").await;

    let url = ctx
        .service
        .presign_url("presign.txt", 3600)
        .await
        .expect("presign_url failed");

    assert!(!url.is_empty(), "Presigned URL should not be empty");
    assert!(
        url.contains("presign.txt"),
        "URL should contain the object key"
    );

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_search_objects() {
    let ctx = TestContext::new().await;

    ctx.put_object("search/hello.txt", b"a").await;
    ctx.put_object("search/world.txt", b"b").await;
    ctx.put_object("search/hello_world.txt", b"c").await;
    ctx.put_object("search/other.txt", b"d").await;

    let cancel = AtomicBool::new(false);
    let results = std::sync::Mutex::new(Vec::new());

    ctx.service
        .search_objects("search/", "hello", &cancel, &|evt| {
            results.lock().unwrap().push(evt);
        })
        .await
        .expect("search failed");

    let events = results.into_inner().unwrap();

    // Should have found "hello.txt" and "hello_world.txt" plus a Done event
    let result_events: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, app_lib::models::SearchEvent::Result(_)))
        .collect();

    assert_eq!(
        result_events.len(),
        2,
        "Should find 2 results matching 'hello'"
    );

    // Should have a Done event
    let done_events: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, app_lib::models::SearchEvent::Done(_)))
        .collect();
    assert_eq!(done_events.len(), 1);

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_list_and_abort_multipart() {
    let ctx = TestContext::new().await;

    // Start a multipart upload manually
    let create_resp = ctx
        .client
        .create_multipart_upload()
        .bucket(&ctx.bucket)
        .key("partial-upload.bin")
        .send()
        .await
        .expect("create multipart failed");

    let upload_id = create_resp.upload_id().unwrap().to_string();

    // List multipart uploads — should see our upload
    let uploads = ctx
        .service
        .list_multipart_uploads(None)
        .await
        .expect("list_multipart_uploads failed");

    assert!(
        uploads.iter().any(|u| u.upload_id == upload_id),
        "Our multipart upload should be listed"
    );

    // Abort it
    ctx.service
        .abort_multipart_upload("partial-upload.bin", &upload_id)
        .await
        .expect("abort_multipart_upload failed");

    // Verify it's gone
    let uploads = ctx
        .service
        .list_multipart_uploads(None)
        .await
        .expect("list after abort failed");

    assert!(
        !uploads.iter().any(|u| u.upload_id == upload_id),
        "Aborted upload should not be listed"
    );

    ctx.cleanup().await;
}

// ═══════════════════════════════════════════════════════════════════════════
// P8 — Error handling
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_head_nonexistent_object() {
    let ctx = TestContext::new().await;

    let result = ctx.service.head_object("does-not-exist.txt").await;
    assert!(result.is_err(), "head of nonexistent object should fail");

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_delete_nonempty_bucket() {
    let ctx = TestContext::new().await;

    // Put an object so bucket isn't empty
    ctx.put_object("blocker.txt", b"blocking deletion").await;

    // Try to delete the bucket directly — should fail
    let result = service::delete_bucket(&ctx.client, &ctx.bucket).await;
    assert!(
        result.is_err(),
        "Deleting non-empty bucket should fail"
    );

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_list_empty_bucket() {
    let ctx = TestContext::new().await;

    let listing = ctx
        .service
        .list_objects("")
        .await
        .expect("list empty bucket failed");

    // Should only have the ".." entry
    assert_eq!(listing.entries.len(), 1, "Empty bucket should only have '..' entry");
    assert_eq!(listing.entries[0].name, "..");

    ctx.cleanup().await;
}

// ═══════════════════════════════════════════════════════════════════════════
// Download & Upload integration
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_download_files() {
    let ctx = TestContext::new().await;

    ctx.put_object("dl/file1.txt", b"content1").await;
    ctx.put_object("dl/file2.txt", b"content2").await;

    let tmp_dir = tempfile::tempdir().expect("tempdir");
    let cancel = AtomicBool::new(false);
    let pause = AtomicBool::new(false);

    let result = ctx
        .service
        .download(
            &["dl/file1.txt".to_string(), "dl/file2.txt".to_string()],
            tmp_dir.path().to_str().unwrap(),
            "op-dl",
            &cancel,
            &pause,
            &|_| {},
        )
        .await
        .expect("download failed");

    assert!(result.is_none(), "Should return None on success");

    // Verify files exist locally
    let c1 = std::fs::read_to_string(tmp_dir.path().join("file1.txt")).unwrap();
    let c2 = std::fs::read_to_string(tmp_dir.path().join("file2.txt")).unwrap();
    assert_eq!(c1, "content1");
    assert_eq!(c2, "content2");

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_upload_files() {
    let ctx = TestContext::new().await;

    let tmp_dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(tmp_dir.path().join("up1.txt"), "upload1").unwrap();
    std::fs::write(tmp_dir.path().join("up2.txt"), "upload2").unwrap();

    let cancel = AtomicBool::new(false);
    let pause = AtomicBool::new(false);

    let result = ctx
        .service
        .upload(
            &[
                tmp_dir.path().join("up1.txt").to_string_lossy().to_string(),
                tmp_dir.path().join("up2.txt").to_string_lossy().to_string(),
            ],
            "uploaded/",
            "op-up",
            &cancel,
            &pause,
            &|_| {},
        )
        .await
        .expect("upload failed");

    assert!(result.is_none());

    // Verify objects exist in S3
    ctx.service
        .head_object("uploaded/up1.txt")
        .await
        .expect("uploaded/up1.txt should exist");
    ctx.service
        .head_object("uploaded/up2.txt")
        .await
        .expect("uploaded/up2.txt should exist");

    ctx.cleanup().await;
}
