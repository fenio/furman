mod common;

use app_lib::local::{self, OpFlags};
use app_lib::models::FmError;
use common::{collect_progress, noop_progress, LocalTestContext};
use std::sync::atomic::{AtomicBool, Ordering};

// ═══════════════════════════════════════════════════════════════════════════════
// Directory Operations
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn list_directory_basic() {
    let ctx = LocalTestContext::new();
    ctx.create_standard_tree();

    let listing = local::list_directory(ctx.path_str(), true).unwrap();

    // Should contain ".." plus our files and subdir
    assert!(listing.entries.iter().any(|e| e.name == ".."));
    assert!(listing.entries.iter().any(|e| e.name == "file1.txt"));
    assert!(listing.entries.iter().any(|e| e.name == "file2.txt"));
    assert!(listing.entries.iter().any(|e| e.name == "subdir" && e.is_dir));
    assert!(listing.entries.iter().any(|e| e.name == ".hidden"));
    assert!(listing.entries.iter().any(|e| e.name == "empty.txt"));
}

#[test]
fn list_directory_metadata() {
    let ctx = LocalTestContext::new();
    ctx.put_file("test.txt", b"hello world");

    let listing = local::list_directory(ctx.path_str(), true).unwrap();
    let entry = listing.entries.iter().find(|e| e.name == "test.txt").unwrap();

    assert_eq!(entry.size, 11);
    assert!(!entry.is_dir);
    assert!(!entry.is_symlink);
    assert!(entry.modified > 0);
    assert!(entry.permissions > 0);
}

#[test]
fn list_directory_empty() {
    let ctx = LocalTestContext::new();

    let listing = local::list_directory(ctx.path_str(), true).unwrap();
    // Only ".." entry
    assert_eq!(listing.entries.len(), 1);
    assert_eq!(listing.entries[0].name, "..");
}

#[test]
fn list_directory_hidden_files_toggle() {
    let ctx = LocalTestContext::new();
    ctx.put_file(".hidden", b"secret");
    ctx.put_file("visible.txt", b"hi");

    // With hidden
    let with_hidden = local::list_directory(ctx.path_str(), true).unwrap();
    assert!(with_hidden.entries.iter().any(|e| e.name == ".hidden"));

    // Without hidden
    let without_hidden = local::list_directory(ctx.path_str(), false).unwrap();
    assert!(!without_hidden.entries.iter().any(|e| e.name == ".hidden"));
    assert!(without_hidden.entries.iter().any(|e| e.name == "visible.txt"));
}

#[test]
fn list_directory_sorted_dirs_first() {
    let ctx = LocalTestContext::new();
    ctx.put_file("zebra.txt", b"z");
    ctx.put_file("alpha.txt", b"a");
    ctx.mkdir("middle_dir");

    let listing = local::list_directory(ctx.path_str(), false).unwrap();
    // First entry should be "..", second should be the directory, rest are files
    let non_parent: Vec<_> = listing.entries.iter().filter(|e| e.name != "..").collect();
    assert!(non_parent[0].is_dir, "First non-parent entry should be a dir");
    assert_eq!(non_parent[0].name, "middle_dir");
}

#[test]
fn list_directory_symlinks() {
    let ctx = LocalTestContext::new();
    ctx.put_file("target.txt", b"target content");
    ctx.symlink(
        &ctx.root.path().join("target.txt").to_string_lossy(),
        "link.txt",
    );

    let listing = local::list_directory(ctx.path_str(), true).unwrap();
    let link = listing.entries.iter().find(|e| e.name == "link.txt").unwrap();
    assert!(link.is_symlink);
    assert!(link.symlink_target.is_some());
}

#[test]
fn list_directory_unicode_names() {
    let ctx = LocalTestContext::new();
    ctx.put_file("日本語.txt", b"japanese");
    ctx.put_file("émojis 🎉.txt", b"party");
    ctx.put_file("Ünïcödé.txt", b"umlauts");

    let listing = local::list_directory(ctx.path_str(), true).unwrap();
    assert!(listing.entries.iter().any(|e| e.name == "日本語.txt"));
    assert!(listing.entries.iter().any(|e| e.name == "émojis 🎉.txt"));
    assert!(listing.entries.iter().any(|e| e.name == "Ünïcödé.txt"));
}

#[test]
fn list_directory_not_found() {
    let result = local::list_directory("/nonexistent/path/that/does/not/exist".to_string(), true);
    assert!(result.is_err());
    match result.unwrap_err() {
        FmError::NotFound(_) => {}
        other => panic!("Expected NotFound, got: {other:?}"),
    }
}

#[test]
fn list_directory_file_path_error() {
    let ctx = LocalTestContext::new();
    ctx.put_file("not_a_dir.txt", b"content");

    let result = local::list_directory(ctx.abs("not_a_dir.txt"), true);
    assert!(result.is_err());
    match result.unwrap_err() {
        FmError::Other(msg) => assert!(msg.contains("not a directory")),
        other => panic!("Expected Other, got: {other:?}"),
    }
}

#[test]
fn create_directory_basic() {
    let ctx = LocalTestContext::new();
    let path = ctx.abs("new_dir");

    local::create_directory(path).unwrap();
    ctx.assert_exists("new_dir");
}

#[test]
fn create_directory_nested_parents() {
    let ctx = LocalTestContext::new();
    let path = ctx.abs("a/b/c/d");

    local::create_directory(path).unwrap();
    ctx.assert_exists("a/b/c/d");
}

#[test]
fn create_directory_already_exists() {
    let ctx = LocalTestContext::new();
    ctx.mkdir("existing");

    let result = local::create_directory(ctx.abs("existing"));
    assert!(result.is_err());
    match result.unwrap_err() {
        FmError::AlreadyExists(_) => {}
        other => panic!("Expected AlreadyExists, got: {other:?}"),
    }
}

#[test]
fn get_directory_size_flat() {
    let ctx = LocalTestContext::new();
    ctx.put_file("a.txt", b"hello"); // 5
    ctx.put_file("b.txt", b"world!"); // 6

    let size = local::get_directory_size(ctx.path_str()).unwrap();
    assert_eq!(size, 11);
}

#[test]
fn get_directory_size_nested() {
    let ctx = LocalTestContext::new();
    ctx.put_file("top.txt", b"top"); // 3
    ctx.put_file("sub/nested.txt", b"nested"); // 6

    let size = local::get_directory_size(ctx.path_str()).unwrap();
    assert_eq!(size, 9);
}

#[test]
fn get_directory_size_empty() {
    let ctx = LocalTestContext::new();
    let size = local::get_directory_size(ctx.path_str()).unwrap();
    assert_eq!(size, 0);
}

#[test]
fn get_directory_size_not_found() {
    let result = local::get_directory_size("/nonexistent".to_string());
    assert!(result.is_err());
    match result.unwrap_err() {
        FmError::NotFound(_) => {}
        other => panic!("Expected NotFound, got: {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Copy Operations
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn copy_single_file() {
    let ctx = LocalTestContext::new();
    ctx.put_file("src.txt", b"copy me");
    let dst = ctx.create_dest();

    let flags = OpFlags {
        cancel: AtomicBool::new(false),
        pause: AtomicBool::new(false),
    };

    let result =
        local::copy_files_core("op1", &[ctx.abs("src.txt")], &dst, &flags, &noop_progress());
    assert!(result.unwrap().is_none());

    ctx.assert_file_content("dst/src.txt", b"copy me");
    // Source should still exist
    ctx.assert_exists("src.txt");
}

#[test]
fn copy_multiple_files() {
    let ctx = LocalTestContext::new();
    ctx.put_file("a.txt", b"aaa");
    ctx.put_file("b.txt", b"bbb");
    let dst = ctx.create_dest();

    let flags = OpFlags {
        cancel: AtomicBool::new(false),
        pause: AtomicBool::new(false),
    };

    let sources = vec![ctx.abs("a.txt"), ctx.abs("b.txt")];
    let result = local::copy_files_core("op1", &sources, &dst, &flags, &noop_progress());
    assert!(result.unwrap().is_none());

    ctx.assert_file_content("dst/a.txt", b"aaa");
    ctx.assert_file_content("dst/b.txt", b"bbb");
}

#[test]
fn copy_recursive_directory() {
    let ctx = LocalTestContext::new();
    ctx.put_file("mydir/file1.txt", b"one");
    ctx.put_file("mydir/sub/file2.txt", b"two");
    let dst = ctx.create_dest();

    let flags = OpFlags {
        cancel: AtomicBool::new(false),
        pause: AtomicBool::new(false),
    };

    let result =
        local::copy_files_core("op1", &[ctx.abs("mydir")], &dst, &flags, &noop_progress());
    assert!(result.unwrap().is_none());

    ctx.assert_file_content("dst/mydir/file1.txt", b"one");
    ctx.assert_file_content("dst/mydir/sub/file2.txt", b"two");
}

#[test]
fn copy_preserves_content() {
    let ctx = LocalTestContext::new();
    let data = b"The quick brown fox jumps over the lazy dog. 0123456789!@#$%";
    ctx.put_file("exact.bin", data);
    let dst = ctx.create_dest();

    let flags = OpFlags {
        cancel: AtomicBool::new(false),
        pause: AtomicBool::new(false),
    };

    local::copy_files_core("op1", &[ctx.abs("exact.bin")], &dst, &flags, &noop_progress())
        .unwrap();
    ctx.assert_file_content("dst/exact.bin", data);
}

#[test]
fn copy_empty_file() {
    let ctx = LocalTestContext::new();
    ctx.put_file("empty.txt", b"");
    let dst = ctx.create_dest();

    let flags = OpFlags {
        cancel: AtomicBool::new(false),
        pause: AtomicBool::new(false),
    };

    local::copy_files_core(
        "op1",
        &[ctx.abs("empty.txt")],
        &dst,
        &flags,
        &noop_progress(),
    )
    .unwrap();
    ctx.assert_file_content("dst/empty.txt", b"");
}

#[test]
fn copy_unicode_filenames() {
    let ctx = LocalTestContext::new();
    ctx.put_file("日本語.txt", b"japanese");
    let dst = ctx.create_dest();

    let flags = OpFlags {
        cancel: AtomicBool::new(false),
        pause: AtomicBool::new(false),
    };

    local::copy_files_core(
        "op1",
        &[ctx.abs("日本語.txt")],
        &dst,
        &flags,
        &noop_progress(),
    )
    .unwrap();
    ctx.assert_file_content("dst/日本語.txt", b"japanese");
}

#[test]
fn copy_nested_deep() {
    let ctx = LocalTestContext::new();
    ctx.put_file("a/b/c/d/e/deep.txt", b"deep");
    let dst = ctx.create_dest();

    let flags = OpFlags {
        cancel: AtomicBool::new(false),
        pause: AtomicBool::new(false),
    };

    local::copy_files_core("op1", &[ctx.abs("a")], &dst, &flags, &noop_progress()).unwrap();
    ctx.assert_file_content("dst/a/b/c/d/e/deep.txt", b"deep");
}

#[test]
fn copy_progress_events_emitted() {
    let ctx = LocalTestContext::new();
    ctx.put_file("p1.txt", b"aaa");
    ctx.put_file("p2.txt", b"bbb");
    let dst = ctx.create_dest();

    let flags = OpFlags {
        cancel: AtomicBool::new(false),
        pause: AtomicBool::new(false),
    };

    let (events, cb) = collect_progress();
    let sources = vec![ctx.abs("p1.txt"), ctx.abs("p2.txt")];
    local::copy_files_core("op1", &sources, &dst, &flags, &cb).unwrap();

    let events = events.lock().unwrap();
    assert!(!events.is_empty(), "Should get at least one progress event");
    // Final event must report all files complete
    let last = events.last().unwrap();
    assert_eq!(last.files_done, 2);
    assert_eq!(last.bytes_total, 6);
    assert_eq!(last.bytes_done, 6);
}

#[test]
fn copy_cancel() {
    let ctx = LocalTestContext::new();
    ctx.put_file("cancel.txt", b"data");
    let dst = ctx.create_dest();

    let flags = OpFlags {
        cancel: AtomicBool::new(true), // pre-cancelled
        pause: AtomicBool::new(false),
    };

    let result = local::copy_files_core(
        "op1",
        &[ctx.abs("cancel.txt")],
        &dst,
        &flags,
        &noop_progress(),
    );
    assert!(result.is_err());
    match result.unwrap_err() {
        FmError::Other(msg) => assert!(msg.contains("cancelled")),
        other => panic!("Expected cancel error, got: {other:?}"),
    }
}

#[test]
fn copy_pause_checkpoint() {
    let ctx = LocalTestContext::new();
    ctx.put_file("pause.txt", b"data");
    let dst = ctx.create_dest();

    let flags = OpFlags {
        cancel: AtomicBool::new(false),
        pause: AtomicBool::new(true), // pre-paused
    };

    let result = local::copy_files_core(
        "op1",
        &[ctx.abs("pause.txt")],
        &dst,
        &flags,
        &noop_progress(),
    );
    let checkpoint = result.unwrap();
    assert!(checkpoint.is_some(), "Should return a pause checkpoint");
}

// ═══════════════════════════════════════════════════════════════════════════════
// Move Operations
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn move_single_file() {
    let ctx = LocalTestContext::new();
    ctx.put_file("src.txt", b"move me");
    let dst = ctx.create_dest();

    let flags = OpFlags {
        cancel: AtomicBool::new(false),
        pause: AtomicBool::new(false),
    };

    let result =
        local::move_files_core("op1", &[ctx.abs("src.txt")], &dst, &flags, &noop_progress());
    assert!(result.unwrap().is_none());

    ctx.assert_file_content("dst/src.txt", b"move me");
    ctx.assert_not_exists("src.txt");
}

#[test]
fn move_multiple_files() {
    let ctx = LocalTestContext::new();
    ctx.put_file("a.txt", b"aaa");
    ctx.put_file("b.txt", b"bbb");
    let dst = ctx.create_dest();

    let flags = OpFlags {
        cancel: AtomicBool::new(false),
        pause: AtomicBool::new(false),
    };

    let sources = vec![ctx.abs("a.txt"), ctx.abs("b.txt")];
    let result = local::move_files_core("op1", &sources, &dst, &flags, &noop_progress());
    assert!(result.unwrap().is_none());

    ctx.assert_file_content("dst/a.txt", b"aaa");
    ctx.assert_file_content("dst/b.txt", b"bbb");
    ctx.assert_not_exists("a.txt");
    ctx.assert_not_exists("b.txt");
}

#[test]
fn move_recursive_directory() {
    let ctx = LocalTestContext::new();
    ctx.put_file("mydir/file.txt", b"content");
    ctx.put_file("mydir/sub/deep.txt", b"deep");
    let dst = ctx.create_dest();

    let flags = OpFlags {
        cancel: AtomicBool::new(false),
        pause: AtomicBool::new(false),
    };

    local::move_files_core("op1", &[ctx.abs("mydir")], &dst, &flags, &noop_progress()).unwrap();

    ctx.assert_file_content("dst/mydir/file.txt", b"content");
    ctx.assert_file_content("dst/mydir/sub/deep.txt", b"deep");
    ctx.assert_not_exists("mydir");
}

#[test]
fn move_progress_events() {
    let ctx = LocalTestContext::new();
    ctx.put_file("m.txt", b"move");
    let dst = ctx.create_dest();

    let flags = OpFlags {
        cancel: AtomicBool::new(false),
        pause: AtomicBool::new(false),
    };

    let (events, cb) = collect_progress();
    local::move_files_core("op1", &[ctx.abs("m.txt")], &dst, &flags, &cb).unwrap();

    let events = events.lock().unwrap();
    assert!(!events.is_empty(), "Should emit progress events");
}

#[test]
fn move_cancel() {
    let ctx = LocalTestContext::new();
    ctx.put_file("cancel.txt", b"data");
    let dst = ctx.create_dest();

    let flags = OpFlags {
        cancel: AtomicBool::new(true),
        pause: AtomicBool::new(false),
    };

    let result = local::move_files_core(
        "op1",
        &[ctx.abs("cancel.txt")],
        &dst,
        &flags,
        &noop_progress(),
    );
    assert!(result.is_err());
}

#[test]
fn move_pause() {
    let ctx = LocalTestContext::new();
    ctx.put_file("pause.txt", b"data");
    let dst = ctx.create_dest();

    let flags = OpFlags {
        cancel: AtomicBool::new(false),
        pause: AtomicBool::new(true),
    };

    let result = local::move_files_core(
        "op1",
        &[ctx.abs("pause.txt")],
        &dst,
        &flags,
        &noop_progress(),
    );
    let checkpoint = result.unwrap();
    assert!(checkpoint.is_some());
}

// ═══════════════════════════════════════════════════════════════════════════════
// Delete Operations
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn delete_permanent_file() {
    let ctx = LocalTestContext::new();
    ctx.put_file("bye.txt", b"gone");

    local::delete_files(vec![ctx.abs("bye.txt")], false).unwrap();
    ctx.assert_not_exists("bye.txt");
}

#[test]
fn delete_permanent_directory() {
    let ctx = LocalTestContext::new();
    ctx.put_file("dir/file.txt", b"content");

    local::delete_files(vec![ctx.abs("dir")], false).unwrap();
    ctx.assert_not_exists("dir");
}

#[test]
fn delete_multiple_items() {
    let ctx = LocalTestContext::new();
    ctx.put_file("a.txt", b"a");
    ctx.put_file("b.txt", b"b");
    ctx.mkdir("c");

    local::delete_files(vec![ctx.abs("a.txt"), ctx.abs("b.txt"), ctx.abs("c")], false).unwrap();
    ctx.assert_not_exists("a.txt");
    ctx.assert_not_exists("b.txt");
    ctx.assert_not_exists("c");
}

#[test]
fn delete_not_found() {
    let result = local::delete_files(vec!["/nonexistent/file".to_string()], false);
    assert!(result.is_err());
    match result.unwrap_err() {
        FmError::NotFound(_) => {}
        other => panic!("Expected NotFound, got: {other:?}"),
    }
}

#[test]
fn delete_trash() {
    let ctx = LocalTestContext::new();
    ctx.put_file("trash_me.txt", b"trash");

    local::delete_files(vec![ctx.abs("trash_me.txt")], true).unwrap();
    ctx.assert_not_exists("trash_me.txt");
}

#[cfg(target_os = "macos")]
#[test]
fn delete_undoable_and_restore() {
    let ctx = LocalTestContext::new();
    ctx.put_file("undo.txt", b"restore me");

    let infos = local::delete_files_undoable(vec![ctx.abs("undo.txt")]).unwrap();
    assert_eq!(infos.len(), 1);
    ctx.assert_not_exists("undo.txt");

    // Restore
    local::restore_from_trash(infos).unwrap();
    ctx.assert_file_content("undo.txt", b"restore me");
}

#[test]
fn restore_not_found() {
    let result = local::restore_from_trash(vec![app_lib::models::TrashInfo {
        original_path: "/tmp/original".to_string(),
        trash_path: "/nonexistent/trash/path".to_string(),
    }]);
    assert!(result.is_err());
    match result.unwrap_err() {
        FmError::NotFound(_) => {}
        other => panic!("Expected NotFound, got: {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Rename Operations
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn rename_file_basic() {
    let ctx = LocalTestContext::new();
    ctx.put_file("old.txt", b"content");

    local::rename_file(ctx.abs("old.txt"), "new.txt".to_string()).unwrap();
    ctx.assert_not_exists("old.txt");
    ctx.assert_file_content("new.txt", b"content");
}

#[test]
fn rename_directory() {
    let ctx = LocalTestContext::new();
    ctx.put_file("olddir/file.txt", b"inside");

    local::rename_file(ctx.abs("olddir"), "newdir".to_string()).unwrap();
    ctx.assert_not_exists("olddir");
    ctx.assert_file_content("newdir/file.txt", b"inside");
}

#[test]
fn rename_not_found() {
    let result = local::rename_file("/nonexistent/path".to_string(), "new".to_string());
    assert!(result.is_err());
    match result.unwrap_err() {
        FmError::NotFound(_) => {}
        other => panic!("Expected NotFound, got: {other:?}"),
    }
}

#[test]
fn rename_already_exists() {
    let ctx = LocalTestContext::new();
    ctx.put_file("existing.txt", b"existing");
    ctx.put_file("target.txt", b"target");

    let result = local::rename_file(ctx.abs("existing.txt"), "target.txt".to_string());
    assert!(result.is_err());
    match result.unwrap_err() {
        FmError::AlreadyExists(_) => {}
        other => panic!("Expected AlreadyExists, got: {other:?}"),
    }
}

#[test]
fn rename_rejects_path_traversal_slash() {
    let ctx = LocalTestContext::new();
    ctx.put_file("safe.txt", b"content");

    let result = local::rename_file(ctx.abs("safe.txt"), "../escape.txt".to_string());
    assert!(result.is_err());
    match result.unwrap_err() {
        FmError::Other(msg) => assert!(msg.contains("path separator")),
        other => panic!("Expected Other with path separator, got: {other:?}"),
    }
}

#[test]
fn rename_rejects_path_traversal_null() {
    let ctx = LocalTestContext::new();
    ctx.put_file("safe.txt", b"content");

    let result = local::rename_file(ctx.abs("safe.txt"), "bad\0name.txt".to_string());
    assert!(result.is_err());
    match result.unwrap_err() {
        FmError::Other(msg) => assert!(msg.contains("path separator")),
        other => panic!("Expected Other with path separator, got: {other:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Conflict Checking
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn check_conflicts_none() {
    let ctx = LocalTestContext::new();
    ctx.put_file("src/a.txt", b"a");
    ctx.mkdir("dst");

    let conflicts = local::check_conflicts(vec![ctx.abs("src/a.txt")], ctx.abs("dst"));
    assert!(conflicts.is_empty());
}

#[test]
fn check_conflicts_some() {
    let ctx = LocalTestContext::new();
    ctx.put_file("src/a.txt", b"a");
    ctx.put_file("src/b.txt", b"b");
    ctx.put_file("dst/a.txt", b"existing");
    // b.txt does NOT exist in dst

    let conflicts = local::check_conflicts(
        vec![ctx.abs("src/a.txt"), ctx.abs("src/b.txt")],
        ctx.abs("dst"),
    );
    assert_eq!(conflicts.len(), 1);
    assert!(conflicts[0].ends_with("a.txt"));
}

#[test]
fn check_conflicts_all() {
    let ctx = LocalTestContext::new();
    ctx.put_file("src/x.txt", b"x");
    ctx.put_file("src/y.txt", b"y");
    ctx.put_file("dst/x.txt", b"existing x");
    ctx.put_file("dst/y.txt", b"existing y");

    let conflicts = local::check_conflicts(
        vec![ctx.abs("src/x.txt"), ctx.abs("src/y.txt")],
        ctx.abs("dst"),
    );
    assert_eq!(conflicts.len(), 2);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Helpers (count_files, total_bytes)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn count_files_single_file() {
    let ctx = LocalTestContext::new();
    ctx.put_file("single.txt", b"data");

    let count = local::count_files(ctx.root.path().join("single.txt").as_path());
    assert_eq!(count, 1);
}

#[test]
fn count_files_directory() {
    let ctx = LocalTestContext::new();
    ctx.put_file("dir/a.txt", b"a");
    ctx.put_file("dir/b.txt", b"b");
    ctx.put_file("dir/sub/c.txt", b"c");

    let count = local::count_files(ctx.root.path().join("dir").as_path());
    assert_eq!(count, 3);
}

#[test]
fn total_bytes_single_file() {
    let ctx = LocalTestContext::new();
    ctx.put_file("five.txt", b"hello"); // 5 bytes

    let bytes = local::total_bytes(ctx.root.path().join("five.txt").as_path());
    assert_eq!(bytes, 5);
}

#[test]
fn total_bytes_directory() {
    let ctx = LocalTestContext::new();
    ctx.put_file("dir/a.txt", b"aaa"); // 3
    ctx.put_file("dir/b.txt", b"bb"); // 2

    let bytes = local::total_bytes(ctx.root.path().join("dir").as_path());
    assert_eq!(bytes, 5);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Edge Cases
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn edge_spaces_in_paths() {
    let ctx = LocalTestContext::new();
    ctx.put_file("path with spaces/file name.txt", b"spaced");
    let dst = ctx.create_dest();

    let flags = OpFlags {
        cancel: AtomicBool::new(false),
        pause: AtomicBool::new(false),
    };

    local::copy_files_core(
        "op1",
        &[ctx.abs("path with spaces")],
        &dst,
        &flags,
        &noop_progress(),
    )
    .unwrap();

    ctx.assert_file_content("dst/path with spaces/file name.txt", b"spaced");
}

#[test]
fn edge_special_characters_in_names() {
    let ctx = LocalTestContext::new();
    ctx.put_file("special!@#$%chars.txt", b"special");
    let dst = ctx.create_dest();

    let flags = OpFlags {
        cancel: AtomicBool::new(false),
        pause: AtomicBool::new(false),
    };

    local::copy_files_core(
        "op1",
        &[ctx.abs("special!@#$%chars.txt")],
        &dst,
        &flags,
        &noop_progress(),
    )
    .unwrap();

    ctx.assert_file_content("dst/special!@#$%chars.txt", b"special");
}

#[test]
fn edge_empty_directory_preserved_during_copy() {
    let ctx = LocalTestContext::new();
    ctx.mkdir("has_empty/empty_child");
    ctx.put_file("has_empty/file.txt", b"data");
    let dst = ctx.create_dest();

    let flags = OpFlags {
        cancel: AtomicBool::new(false),
        pause: AtomicBool::new(false),
    };

    local::copy_files_core(
        "op1",
        &[ctx.abs("has_empty")],
        &dst,
        &flags,
        &noop_progress(),
    )
    .unwrap();

    ctx.assert_exists("dst/has_empty/empty_child");
    ctx.assert_file_content("dst/has_empty/file.txt", b"data");
    // Verify it's actually a directory
    assert!(ctx.root.path().join("dst/has_empty/empty_child").is_dir());
}

#[test]
fn edge_symlink_copy() {
    let ctx = LocalTestContext::new();
    ctx.put_file("target.txt", b"target content");
    // Create absolute symlink
    let target_abs = ctx.root.path().join("target.txt");
    ctx.symlink(&target_abs.to_string_lossy(), "link.txt");
    let dst = ctx.create_dest();

    let flags = OpFlags {
        cancel: AtomicBool::new(false),
        pause: AtomicBool::new(false),
    };

    // Copy the symlink — fs::copy follows symlinks, so dst should have the content
    local::copy_files_core(
        "op1",
        &[ctx.abs("link.txt")],
        &dst,
        &flags,
        &noop_progress(),
    )
    .unwrap();

    ctx.assert_file_content("dst/link.txt", b"target content");
}

// ═══════════════════════════════════════════════════════════════════════════════
// Progress detail checks
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn copy_progress_totals_correct() {
    let ctx = LocalTestContext::new();
    ctx.put_file("dir/a.txt", b"aa"); // 2
    ctx.put_file("dir/b.txt", b"bbb"); // 3
    let dst = ctx.create_dest();

    let flags = OpFlags {
        cancel: AtomicBool::new(false),
        pause: AtomicBool::new(false),
    };

    let (events, cb) = collect_progress();
    local::copy_files_core("op1", &[ctx.abs("dir")], &dst, &flags, &cb).unwrap();

    let events = events.lock().unwrap();
    assert!(!events.is_empty());
    // All events should report same totals
    for ev in events.iter() {
        assert_eq!(ev.files_total, 2);
        assert_eq!(ev.bytes_total, 5);
        assert_eq!(ev.id, "op1");
    }
    // Final event should show all complete
    let last = events.last().unwrap();
    assert_eq!(last.files_done, 2);
    assert_eq!(last.bytes_done, 5);
}

#[test]
fn move_cancel_preserves_source() {
    let ctx = LocalTestContext::new();
    ctx.put_file("keep.txt", b"important");
    let dst = ctx.create_dest();

    let flags = OpFlags {
        cancel: AtomicBool::new(true),
        pause: AtomicBool::new(false),
    };

    let result = local::move_files_core(
        "op1",
        &[ctx.abs("keep.txt")],
        &dst,
        &flags,
        &noop_progress(),
    );
    assert!(result.is_err());
    // Source should still exist since the op was cancelled before starting
    ctx.assert_file_content("keep.txt", b"important");
}

#[test]
fn copy_cancel_mid_operation() {
    let ctx = LocalTestContext::new();
    // Create multiple files so cancel can trigger between files
    ctx.put_file("dir/f1.txt", b"one");
    ctx.put_file("dir/f2.txt", b"two");
    ctx.put_file("dir/f3.txt", b"three");
    let dst = ctx.create_dest();

    let flags = OpFlags {
        cancel: AtomicBool::new(false),
        pause: AtomicBool::new(false),
    };

    // Use a callback that sets cancel after the first file
    let cancel_flag = &flags.cancel;
    let cb = |_event: app_lib::models::ProgressEvent| {
        cancel_flag.store(true, Ordering::Relaxed);
    };

    let result = local::copy_files_core("op1", &[ctx.abs("dir")], &dst, &flags, &cb);
    // Should be cancelled (error) since we set cancel after first file progress
    assert!(result.is_err());
}
