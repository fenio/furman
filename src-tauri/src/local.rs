//! Testable core logic for local filesystem operations.
//!
//! This module extracts the non-Tauri-dependent parts of `commands/file.rs`
//! and `commands/directory.rs` so they can be called from integration tests
//! without needing a Tauri runtime.

use crate::models::{FmError, ProgressEvent, TransferCheckpoint};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

// ── Types ────────────────────────────────────────────────────────────────────

/// Flags shared between a running operation and whoever wants to cancel/pause it.
pub struct OpFlags {
    pub cancel: AtomicBool,
    pub pause: AtomicBool,
}

/// Check result for copy_recursive: either success or pause.
pub enum CopyResult {
    Done,
    Paused,
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Count files (non-directory entries) inside a path recursively.
pub fn count_files(path: &Path) -> u32 {
    if !path.is_dir() {
        return 1;
    }
    let mut count: u32 = 0;
    if let Ok(rd) = fs::read_dir(path) {
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() {
                count += count_files(&p);
            } else {
                count += 1;
            }
        }
    }
    count
}

/// Total byte size of a path (recursive for directories).
pub fn total_bytes(path: &Path) -> u64 {
    if !path.is_dir() {
        return fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    }
    let mut total: u64 = 0;
    if let Ok(rd) = fs::read_dir(path) {
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() {
                total += total_bytes(&p);
            } else {
                total += fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
            }
        }
    }
    total
}

/// Minimum interval between progress events (50ms = ~20 updates/sec).
const PROGRESS_INTERVAL_MS: u128 = 50;

/// Recursively copy a file or directory, reporting progress via a closure.
/// Progress events are throttled to avoid IPC overhead for many small files.
pub fn copy_recursive(
    src: &Path,
    dst: &Path,
    id: &str,
    bytes_done: &mut u64,
    bytes_total: u64,
    files_done: &mut u32,
    files_total: u32,
    on_progress: &dyn Fn(ProgressEvent),
    flags: &OpFlags,
    completed_files: &mut Vec<String>,
    last_progress: &mut Instant,
) -> Result<CopyResult, FmError> {
    if flags.cancel.load(Ordering::Relaxed) {
        return Err(FmError::Other("Operation cancelled".into()));
    }
    if flags.pause.load(Ordering::Relaxed) {
        return Ok(CopyResult::Paused);
    }
    if src.is_dir() {
        fs::create_dir_all(dst)?;
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            let child_src = entry.path();
            let child_dst = dst.join(entry.file_name());
            match copy_recursive(
                &child_src,
                &child_dst,
                id,
                bytes_done,
                bytes_total,
                files_done,
                files_total,
                on_progress,
                flags,
                completed_files,
                last_progress,
            )? {
                CopyResult::Done => {}
                CopyResult::Paused => return Ok(CopyResult::Paused),
            }
        }
    } else {
        // Ensure parent directory exists.
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }

        let size = fs::metadata(src).map(|m| m.len()).unwrap_or(0);
        fs::copy(src, dst)?;
        *bytes_done += size;
        *files_done += 1;
        completed_files.push(src.to_string_lossy().into_owned());

        // Throttle progress events; always emit first and final
        let now = Instant::now();
        if *files_done == 1
            || *files_done == files_total
            || now.duration_since(*last_progress).as_millis() >= PROGRESS_INTERVAL_MS
        {
            *last_progress = now;
            on_progress(ProgressEvent {
                id: id.to_string(),
                bytes_done: *bytes_done,
                bytes_total,
                current_file: src.to_string_lossy().into_owned(),
                files_done: *files_done,
                files_total,
            });
        }
    }
    Ok(CopyResult::Done)
}

// ── Core operations ──────────────────────────────────────────────────────────

/// Copy one or more files/directories to `destination` with progress reporting.
/// Returns None on success, Some(checkpoint) on pause.
pub fn copy_files_core(
    id: &str,
    sources: &[String],
    destination: &str,
    flags: &OpFlags,
    on_progress: &dyn Fn(ProgressEvent),
) -> Result<Option<TransferCheckpoint>, FmError> {
    let dest = PathBuf::from(destination);

    // Pre-calculate totals for progress.
    let mut bytes_total: u64 = 0;
    let mut files_total: u32 = 0;
    for src in sources {
        let p = PathBuf::from(src);
        bytes_total += total_bytes(&p);
        files_total += count_files(&p);
    }

    let mut bytes_done: u64 = 0;
    let mut files_done: u32 = 0;
    let mut completed_files: Vec<String> = Vec::new();
    let mut last_progress = Instant::now();

    for src in sources {
        let src_path = PathBuf::from(src);
        let file_name = src_path
            .file_name()
            .ok_or_else(|| FmError::Other(format!("invalid source path: {src}")))?;
        let dst_path = dest.join(file_name);

        match copy_recursive(
            &src_path,
            &dst_path,
            id,
            &mut bytes_done,
            bytes_total,
            &mut files_done,
            files_total,
            on_progress,
            flags,
            &mut completed_files,
            &mut last_progress,
        )? {
            CopyResult::Done => {}
            CopyResult::Paused => {
                return Ok(Some(TransferCheckpoint {
                    files_completed: completed_files,
                    bytes_done,
                    bytes_total,
                    files_done,
                    files_total,
                }));
            }
        }
    }
    Ok(None)
}

/// Move one or more files/directories to `destination` with progress reporting.
///
/// Attempts a fast `rename` first; falls back to copy + delete if the rename
/// fails (e.g. cross-device move).
/// Returns None on success, Some(checkpoint) on pause.
pub fn move_files_core(
    id: &str,
    sources: &[String],
    destination: &str,
    flags: &OpFlags,
    on_progress: &dyn Fn(ProgressEvent),
) -> Result<Option<TransferCheckpoint>, FmError> {
    let dest = PathBuf::from(destination);

    // Pre-calculate totals.
    let mut bytes_total: u64 = 0;
    let mut files_total: u32 = 0;
    for src in sources {
        let p = PathBuf::from(src);
        bytes_total += total_bytes(&p);
        files_total += count_files(&p);
    }

    let mut bytes_done: u64 = 0;
    let mut files_done: u32 = 0;
    let mut completed_files: Vec<String> = Vec::new();
    let mut last_progress = Instant::now();

    for src in sources {
        if flags.cancel.load(Ordering::Relaxed) {
            return Err(FmError::Other("Operation cancelled".into()));
        }
        if flags.pause.load(Ordering::Relaxed) {
            return Ok(Some(TransferCheckpoint {
                files_completed: completed_files,
                bytes_done,
                bytes_total,
                files_done,
                files_total,
            }));
        }

        let src_path = PathBuf::from(src);
        let file_name = src_path
            .file_name()
            .ok_or_else(|| FmError::Other(format!("invalid source path: {src}")))?;
        let dst_path = dest.join(file_name);

        // Try fast rename first.
        if fs::rename(&src_path, &dst_path).is_ok() {
            let size = total_bytes(&dst_path);
            let count = count_files(&dst_path);
            bytes_done += size;
            files_done += count;
            completed_files.push(src.clone());

            on_progress(ProgressEvent {
                id: id.to_string(),
                bytes_done,
                bytes_total,
                current_file: src_path.to_string_lossy().into_owned(),
                files_done,
                files_total,
            });
        } else {
            // Cross-device: copy then delete source.
            match copy_recursive(
                &src_path,
                &dst_path,
                id,
                &mut bytes_done,
                bytes_total,
                &mut files_done,
                files_total,
                on_progress,
                flags,
                &mut completed_files,
                &mut last_progress,
            )? {
                CopyResult::Done => {
                    if src_path.is_dir() {
                        fs::remove_dir_all(&src_path)?;
                    } else {
                        fs::remove_file(&src_path)?;
                    }
                }
                CopyResult::Paused => {
                    return Ok(Some(TransferCheckpoint {
                        files_completed: completed_files,
                        bytes_done,
                        bytes_total,
                        files_done,
                        files_total,
                    }));
                }
            }
        }
    }
    Ok(None)
}

// ── Re-exports from commands ─────────────────────────────────────────────────
// These functions have no Tauri-specific parameters and are directly testable.

pub use crate::commands::directory::{create_directory, get_directory_size, list_directory};
pub use crate::commands::file::{
    check_conflicts, delete_files, delete_files_undoable, rename_file, restore_from_trash,
};
