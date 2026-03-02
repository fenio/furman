mod common;

use common::SftpTestContext;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use app_lib::models::ProgressEvent;

fn noop_progress() -> impl Fn(ProgressEvent) + Send + Sync {
    |_| {}
}

fn collect_progress() -> (
    Arc<Mutex<Vec<ProgressEvent>>>,
    impl Fn(ProgressEvent) + Send + Sync,
) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let events_clone = events.clone();
    let cb = move |evt: ProgressEvent| {
        events_clone.lock().unwrap().push(evt);
    };
    (events, cb)
}

// ── P1: Core CRUD ───────────────────────────────────────────────────────────

#[tokio::test]
async fn test_list_directory() {
    let ctx = SftpTestContext::new().await;

    ctx.put_file("hello.txt", b"hello").await;
    ctx.put_file("world.txt", b"world").await;

    let listing = ctx.service.list_objects(&ctx.test_dir).await.unwrap();
    let names: Vec<&str> = listing.entries.iter().map(|e| e.name.as_str()).collect();

    assert!(names.contains(&".."), "Should contain parent entry");
    assert!(names.contains(&"hello.txt"), "Should contain hello.txt");
    assert!(names.contains(&"world.txt"), "Should contain world.txt");

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_create_folder() {
    let ctx = SftpTestContext::new().await;

    let folder_path = format!("{}/new-folder", ctx.test_dir);
    ctx.service.create_folder(&folder_path).await.unwrap();

    let listing = ctx.service.list_objects(&ctx.test_dir).await.unwrap();
    let names: Vec<&str> = listing.entries.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"new-folder"), "Should contain new-folder");

    let folder_entry = listing
        .entries
        .iter()
        .find(|e| e.name == "new-folder")
        .unwrap();
    assert!(folder_entry.is_dir, "new-folder should be a directory");

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_stat_file() {
    let ctx = SftpTestContext::new().await;

    let data = b"stat test data";
    ctx.put_file("stat-test.txt", data).await;

    let path = format!("{}/stat-test.txt", ctx.test_dir);
    let attrs = ctx.service.stat(&path).await.unwrap();

    assert_eq!(attrs.size, Some(data.len() as u64), "Size should match");
    assert!(attrs.is_regular(), "Should be a regular file");
    assert!(!attrs.is_dir(), "Should not be a directory");

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_stat_directory() {
    let ctx = SftpTestContext::new().await;

    ctx.mkdir("stat-dir").await;

    let path = format!("{}/stat-dir", ctx.test_dir);
    let attrs = ctx.service.stat(&path).await.unwrap();

    assert!(attrs.is_dir(), "Should be a directory");
    assert!(!attrs.is_regular(), "Should not be a regular file");

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_delete_file() {
    let ctx = SftpTestContext::new().await;

    ctx.put_file("delete-me.txt", b"bye").await;

    let path = format!("{}/delete-me.txt", ctx.test_dir);
    ctx.service.delete(&[path]).await.unwrap();

    let listing = ctx.service.list_objects(&ctx.test_dir).await.unwrap();
    let names: Vec<&str> = listing.entries.iter().map(|e| e.name.as_str()).collect();
    assert!(!names.contains(&"delete-me.txt"), "File should be gone");

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_delete_directory() {
    let ctx = SftpTestContext::new().await;

    ctx.mkdir("delete-dir").await;
    ctx.put_file_at("delete-dir/a.txt", b"a").await;
    ctx.put_file_at("delete-dir/sub/b.txt", b"b").await;

    let path = format!("{}/delete-dir", ctx.test_dir);
    ctx.service.delete(&[path]).await.unwrap();

    let listing = ctx.service.list_objects(&ctx.test_dir).await.unwrap();
    let names: Vec<&str> = listing.entries.iter().map(|e| e.name.as_str()).collect();
    assert!(!names.contains(&"delete-dir"), "Directory should be gone");

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_rename() {
    let ctx = SftpTestContext::new().await;

    ctx.put_file("old-name.txt", b"rename me").await;

    let path = format!("{}/old-name.txt", ctx.test_dir);
    ctx.service.rename(&path, "new-name.txt").await.unwrap();

    let listing = ctx.service.list_objects(&ctx.test_dir).await.unwrap();
    let names: Vec<&str> = listing.entries.iter().map(|e| e.name.as_str()).collect();
    assert!(!names.contains(&"old-name.txt"), "Old name should be gone");
    assert!(names.contains(&"new-name.txt"), "New name should exist");

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_put_text_and_download_temp() {
    let ctx = SftpTestContext::new().await;

    let remote = format!("{}/hello.txt", ctx.test_dir);
    ctx.service
        .put_text(&remote, "hello from put_text")
        .await
        .unwrap();

    let local = ctx.service.download_temp(&remote).await.unwrap();
    let content = tokio::fs::read_to_string(&local).await.unwrap();
    assert_eq!(content, "hello from put_text");

    // Clean up temp file
    let _ = tokio::fs::remove_file(&local).await;
    ctx.cleanup().await;
}

// ── P2: Download ────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_download_single_file() {
    let ctx = SftpTestContext::new().await;

    let data = b"single file download test content";
    ctx.put_file("download-me.txt", data).await;

    let tmp = tempfile::tempdir().unwrap();
    let remote = format!("{}/download-me.txt", ctx.test_dir);
    let cancel = AtomicBool::new(false);

    ctx.service
        .download(
            &[remote],
            tmp.path().to_str().unwrap(),
            "op-dl-single",
            &cancel,
            &noop_progress(),
        )
        .await
        .unwrap();

    let content = tokio::fs::read_to_string(tmp.path().join("download-me.txt"))
        .await
        .unwrap();
    assert_eq!(content, "single file download test content");

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_download_directory() {
    let ctx = SftpTestContext::new().await;

    ctx.mkdir("dl-dir").await;
    ctx.put_file_at("dl-dir/a.txt", b"aaa").await;
    ctx.put_file_at("dl-dir/sub/b.txt", b"bbb").await;

    let tmp = tempfile::tempdir().unwrap();
    let remote = format!("{}/dl-dir", ctx.test_dir);
    let cancel = AtomicBool::new(false);

    ctx.service
        .download(
            &[remote],
            tmp.path().to_str().unwrap(),
            "op-dl-dir",
            &cancel,
            &noop_progress(),
        )
        .await
        .unwrap();

    let a = tokio::fs::read_to_string(tmp.path().join("dl-dir/a.txt"))
        .await
        .unwrap();
    assert_eq!(a, "aaa");

    let b = tokio::fs::read_to_string(tmp.path().join("dl-dir/sub/b.txt"))
        .await
        .unwrap();
    assert_eq!(b, "bbb");

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_download_progress_events() {
    let ctx = SftpTestContext::new().await;

    ctx.put_file("p1.txt", b"progress1").await;
    ctx.put_file("p2.txt", b"progress2").await;

    let tmp = tempfile::tempdir().unwrap();
    let remotes = vec![
        format!("{}/p1.txt", ctx.test_dir),
        format!("{}/p2.txt", ctx.test_dir),
    ];
    let cancel = AtomicBool::new(false);
    let (events, on_progress) = collect_progress();

    ctx.service
        .download(
            &remotes
                .iter()
                .map(|s| s.as_str().to_string())
                .collect::<Vec<_>>(),
            tmp.path().to_str().unwrap(),
            "op-dl-progress",
            &cancel,
            &on_progress,
        )
        .await
        .unwrap();

    let evts = events.lock().unwrap();

    // Should have scanning events + per-file completion events
    assert!(!evts.is_empty(), "Should have progress events");

    // Find the final event (last per-file completion)
    let final_evt = evts.iter().filter(|e| e.files_total > 0).last().unwrap();
    assert!(final_evt.bytes_total > 0, "bytes_total should be > 0");
    assert_eq!(final_evt.files_total, 2, "files_total should be 2");
    assert_eq!(
        final_evt.files_done, 2,
        "files_done should reach files_total"
    );

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_download_scanning_phase() {
    let ctx = SftpTestContext::new().await;

    ctx.put_file("scan.txt", b"data").await;

    let tmp = tempfile::tempdir().unwrap();
    let remote = format!("{}/scan.txt", ctx.test_dir);
    let cancel = AtomicBool::new(false);
    let (events, on_progress) = collect_progress();

    ctx.service
        .download(
            &[remote],
            tmp.path().to_str().unwrap(),
            "op-dl-scan",
            &cancel,
            &on_progress,
        )
        .await
        .unwrap();

    let evts = events.lock().unwrap();
    // The first event should be a scanning event
    let first = &evts[0];
    assert_eq!(first.bytes_total, 0, "Scanning events have bytes_total=0");
    assert!(
        first.current_file.contains("Scanning"),
        "Scanning phase should mention 'Scanning', got: {}",
        first.current_file
    );

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_download_cancel_during_transfer() {
    let ctx = SftpTestContext::new().await;

    // Create several files so cancel has time to trigger
    for i in 0..5 {
        ctx.put_file(&format!("cancel-{}.txt", i), &vec![0u8; 1024])
            .await;
    }

    let tmp = tempfile::tempdir().unwrap();
    let remotes: Vec<String> = (0..5)
        .map(|i| format!("{}/cancel-{}.txt", ctx.test_dir, i))
        .collect();
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_clone = cancel.clone();
    let events = Arc::new(Mutex::new(Vec::<ProgressEvent>::new()));

    // Set cancel after first file download event
    let events_for_cancel = events.clone();
    let cancel_cb = move |evt: ProgressEvent| {
        events_for_cancel.lock().unwrap().push(evt.clone());
        if evt.files_done >= 1 && evt.files_total > 0 {
            cancel_clone.store(true, Ordering::Relaxed);
        }
    };

    let result = ctx
        .service
        .download(
            &remotes,
            tmp.path().to_str().unwrap(),
            "op-dl-cancel",
            &cancel,
            &cancel_cb,
        )
        .await;

    assert!(result.is_err(), "Download should fail when cancelled");
    let err_msg = format!("{}", result.unwrap_err());
    assert!(
        err_msg.contains("cancelled"),
        "Error should mention 'cancelled', got: {}",
        err_msg
    );

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_download_empty_directory() {
    let ctx = SftpTestContext::new().await;

    ctx.mkdir("empty-dir").await;

    let tmp = tempfile::tempdir().unwrap();
    let remote = format!("{}/empty-dir", ctx.test_dir);
    let cancel = AtomicBool::new(false);
    let (events, cb) = collect_progress();

    let result = ctx
        .service
        .download(
            &[remote],
            tmp.path().to_str().unwrap(),
            "op-dl-empty",
            &cancel,
            &cb,
        )
        .await;

    assert!(result.is_ok(), "Empty directory download should succeed");

    // Should only have scanning events (no file completion events)
    let evts = events.lock().unwrap();
    let file_events: Vec<_> = evts.iter().filter(|e| e.files_total > 0).collect();
    assert!(
        file_events.is_empty(),
        "Should have no file-level events for empty dir"
    );

    ctx.cleanup().await;
}

// ── P3: Upload ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_upload_single_file() {
    let ctx = SftpTestContext::new().await;

    let tmp = tempfile::tempdir().unwrap();
    let local_file = tmp.path().join("upload-me.txt");
    tokio::fs::write(&local_file, b"upload content")
        .await
        .unwrap();

    let cancel = AtomicBool::new(false);
    ctx.service
        .upload(
            &[local_file.to_str().unwrap().to_string()],
            &ctx.test_dir,
            "op-ul-single",
            &cancel,
            &noop_progress(),
        )
        .await
        .unwrap();

    // Verify file exists on server
    let listing = ctx.service.list_objects(&ctx.test_dir).await.unwrap();
    let names: Vec<&str> = listing.entries.iter().map(|e| e.name.as_str()).collect();
    assert!(
        names.contains(&"upload-me.txt"),
        "Uploaded file should appear"
    );

    // Verify content by downloading
    let remote_path = format!("{}/upload-me.txt", ctx.test_dir);
    let local_dl = ctx.service.download_temp(&remote_path).await.unwrap();
    let content = tokio::fs::read_to_string(&local_dl).await.unwrap();
    assert_eq!(content, "upload content");
    let _ = tokio::fs::remove_file(&local_dl).await;

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_upload_directory() {
    let ctx = SftpTestContext::new().await;

    // Create local directory structure
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("upload-dir");
    tokio::fs::create_dir_all(dir.join("sub")).await.unwrap();
    tokio::fs::write(dir.join("root.txt"), b"root file")
        .await
        .unwrap();
    tokio::fs::write(dir.join("sub/nested.txt"), b"nested file")
        .await
        .unwrap();

    let cancel = AtomicBool::new(false);
    ctx.service
        .upload(
            &[dir.to_str().unwrap().to_string()],
            &ctx.test_dir,
            "op-ul-dir",
            &cancel,
            &noop_progress(),
        )
        .await
        .unwrap();

    // Verify root file
    let remote_root = format!("{}/upload-dir/root.txt", ctx.test_dir);
    let dl_root = ctx.service.download_temp(&remote_root).await.unwrap();
    let root_content = tokio::fs::read_to_string(&dl_root).await.unwrap();
    assert_eq!(root_content, "root file");
    let _ = tokio::fs::remove_file(&dl_root).await;

    // Verify nested file
    let remote_nested = format!("{}/upload-dir/sub/nested.txt", ctx.test_dir);
    let dl_nested = ctx.service.download_temp(&remote_nested).await.unwrap();
    let nested_content = tokio::fs::read_to_string(&dl_nested).await.unwrap();
    assert_eq!(nested_content, "nested file");
    let _ = tokio::fs::remove_file(&dl_nested).await;

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_upload_progress_events() {
    let ctx = SftpTestContext::new().await;

    let tmp = tempfile::tempdir().unwrap();
    tokio::fs::write(tmp.path().join("up1.txt"), b"upload1")
        .await
        .unwrap();
    tokio::fs::write(tmp.path().join("up2.txt"), b"upload2")
        .await
        .unwrap();

    let sources: Vec<String> = vec![
        tmp.path().join("up1.txt").to_str().unwrap().to_string(),
        tmp.path().join("up2.txt").to_str().unwrap().to_string(),
    ];

    let cancel = AtomicBool::new(false);
    let (events, on_progress) = collect_progress();

    ctx.service
        .upload(
            &sources,
            &ctx.test_dir,
            "op-ul-progress",
            &cancel,
            &on_progress,
        )
        .await
        .unwrap();

    let evts = events.lock().unwrap();
    assert!(!evts.is_empty(), "Should have progress events");

    let final_evt = evts.last().unwrap();
    assert!(final_evt.bytes_total > 0, "bytes_total should be > 0");
    assert_eq!(final_evt.files_total, 2, "files_total should be 2");
    assert_eq!(
        final_evt.files_done, 2,
        "files_done should reach files_total"
    );

    ctx.cleanup().await;
}

#[tokio::test]
async fn test_upload_creates_parent_dirs() {
    let ctx = SftpTestContext::new().await;

    let tmp = tempfile::tempdir().unwrap();
    let local_file = tmp.path().join("deep.txt");
    tokio::fs::write(&local_file, b"deep content")
        .await
        .unwrap();

    // Upload to a deeply nested path that doesn't exist yet
    let deep_dest = format!("{}/a/b/c", ctx.test_dir);
    let cancel = AtomicBool::new(false);

    ctx.service
        .upload(
            &[local_file.to_str().unwrap().to_string()],
            &deep_dest,
            "op-ul-deep",
            &cancel,
            &noop_progress(),
        )
        .await
        .unwrap();

    // Verify the file is there
    let remote_path = format!("{}/a/b/c/deep.txt", ctx.test_dir);
    let local_dl = ctx.service.download_temp(&remote_path).await.unwrap();
    let content = tokio::fs::read_to_string(&local_dl).await.unwrap();
    assert_eq!(content, "deep content");
    let _ = tokio::fs::remove_file(&local_dl).await;

    ctx.cleanup().await;
}

// ── P4: Edge Cases ──────────────────────────────────────────────────────────

#[tokio::test]
async fn test_download_many_files() {
    let ctx = SftpTestContext::new().await;

    // Create 100 files in nested directories
    let file_count = 100;
    for i in 0..file_count {
        let dir = format!("many/dir-{}", i / 10);
        let name = format!("{}/file-{}.txt", dir, i);
        ctx.put_file_at(&name, format!("content-{}", i).as_bytes())
            .await;
    }

    let tmp = tempfile::tempdir().unwrap();
    let remote = format!("{}/many", ctx.test_dir);
    let cancel = AtomicBool::new(false);
    let (events, on_progress) = collect_progress();

    ctx.service
        .download(
            &[remote],
            tmp.path().to_str().unwrap(),
            "op-dl-many",
            &cancel,
            &on_progress,
        )
        .await
        .unwrap();

    // Verify all files were downloaded
    let mut count = 0;
    for i in 0..file_count {
        let dir = format!("dir-{}", i / 10);
        let path = tmp.path().join(format!("many/{}/file-{}.txt", dir, i));
        assert!(path.exists(), "File {} should exist", path.display());
        let content = tokio::fs::read_to_string(&path).await.unwrap();
        assert_eq!(content, format!("content-{}", i));
        count += 1;
    }
    assert_eq!(count, file_count, "All files should be downloaded");

    // Verify final progress events
    let evts = events.lock().unwrap();
    let final_evt = evts.iter().filter(|e| e.files_total > 0).last().unwrap();
    assert_eq!(
        final_evt.files_done, final_evt.files_total,
        "All files should be reported as done"
    );
    assert_eq!(
        final_evt.files_total, file_count as u32,
        "files_total should be {}",
        file_count
    );

    ctx.cleanup().await;
}

#[tokio::test]
#[ignore] // Requires docker exec to create symlinks; run manually
async fn test_download_symlink_handling() {
    let ctx = SftpTestContext::new().await;

    ctx.put_file("real-file.txt", b"real content").await;

    // Create a symlink via docker exec (requires the container to be named sftp-test)
    let target = format!("{}/real-file.txt", ctx.test_dir);
    let link = format!("{}/link.txt", ctx.test_dir);
    let status = std::process::Command::new("docker")
        .args(["exec", "sftp-test", "ln", "-s", &target, &link])
        .status();

    match status {
        Ok(s) if s.success() => {
            // Download the directory — should not hang or crash
            let tmp = tempfile::tempdir().unwrap();
            let cancel = AtomicBool::new(false);

            let result = ctx
                .service
                .download(
                    &[ctx.test_dir.clone()],
                    tmp.path().to_str().unwrap(),
                    "op-dl-symlink",
                    &cancel,
                    &noop_progress(),
                )
                .await;

            assert!(result.is_ok(), "Download with symlinks should not crash");
        }
        _ => {
            eprintln!("Skipping symlink test: docker exec not available");
        }
    }

    ctx.cleanup().await;
}
