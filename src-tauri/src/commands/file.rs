use crate::models::{FmError, ProgressEvent, TrashInfo, TransferCheckpoint};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::ipc::Channel;

pub struct OpFlags {
    pub cancel: AtomicBool,
    pub pause: AtomicBool,
}

pub struct FileOpState(pub Mutex<HashMap<String, Arc<OpFlags>>>);

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Count files (non-directory entries) inside a path recursively.
fn count_files(path: &Path) -> u32 {
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
fn total_bytes(path: &Path) -> u64 {
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

/// Check result for copy_recursive: either success, pause, or error.
enum CopyResult {
    Done,
    Paused,
}

/// Recursively copy a file or directory, sending progress through the channel.
fn copy_recursive(
    src: &Path,
    dst: &Path,
    id: &str,
    bytes_done: &mut u64,
    bytes_total: u64,
    files_done: &mut u32,
    files_total: u32,
    channel: &Channel<ProgressEvent>,
    flags: &OpFlags,
    completed_files: &mut Vec<String>,
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
                channel,
                flags,
                completed_files,
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

        let _ = channel.send(ProgressEvent {
            id: id.to_string(),
            bytes_done: *bytes_done,
            bytes_total,
            current_file: src.to_string_lossy().into_owned(),
            files_done: *files_done,
            files_total,
        });
    }
    Ok(CopyResult::Done)
}

// ── Commands ─────────────────────────────────────────────────────────────────

/// Copy one or more files/directories to `destination` with progress reporting.
/// Returns None on success, Some(checkpoint) on pause.
#[tauri::command]
pub fn copy_files(
    id: String,
    sources: Vec<String>,
    destination: String,
    channel: Channel<ProgressEvent>,
    state: tauri::State<'_, FileOpState>,
) -> Result<Option<TransferCheckpoint>, FmError> {
    let dest = PathBuf::from(&destination);

    // Pre-calculate totals for progress.
    let mut bytes_total: u64 = 0;
    let mut files_total: u32 = 0;
    for src in &sources {
        let p = PathBuf::from(src);
        bytes_total += total_bytes(&p);
        files_total += count_files(&p);
    }

    let flags = Arc::new(OpFlags {
        cancel: AtomicBool::new(false),
        pause: AtomicBool::new(false),
    });
    {
        let mut map = state.0.lock().map_err(|e| FmError::Other(e.to_string()))?;
        map.insert(id.clone(), flags.clone());
    }

    let mut bytes_done: u64 = 0;
    let mut files_done: u32 = 0;
    let mut completed_files: Vec<String> = Vec::new();

    let result = (|| {
        for src in &sources {
            let src_path = PathBuf::from(src);
            let file_name = src_path
                .file_name()
                .ok_or_else(|| FmError::Other(format!("invalid source path: {src}")))?;
            let dst_path = dest.join(file_name);

            match copy_recursive(
                &src_path,
                &dst_path,
                &id,
                &mut bytes_done,
                bytes_total,
                &mut files_done,
                files_total,
                &channel,
                &flags,
                &mut completed_files,
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
    })();

    // Clean up the flags from state.
    if let Ok(mut map) = state.0.lock() {
        map.remove(&id);
    }

    result
}

/// Move one or more files/directories to `destination` with progress reporting.
///
/// Attempts a fast `rename` first; falls back to copy + delete if the rename
/// fails (e.g. cross-device move).
/// Returns None on success, Some(checkpoint) on pause.
#[tauri::command]
pub fn move_files(
    id: String,
    sources: Vec<String>,
    destination: String,
    channel: Channel<ProgressEvent>,
    state: tauri::State<'_, FileOpState>,
) -> Result<Option<TransferCheckpoint>, FmError> {
    let dest = PathBuf::from(&destination);

    // Pre-calculate totals.
    let mut bytes_total: u64 = 0;
    let mut files_total: u32 = 0;
    for src in &sources {
        let p = PathBuf::from(src);
        bytes_total += total_bytes(&p);
        files_total += count_files(&p);
    }

    let flags = Arc::new(OpFlags {
        cancel: AtomicBool::new(false),
        pause: AtomicBool::new(false),
    });
    {
        let mut map = state.0.lock().map_err(|e| FmError::Other(e.to_string()))?;
        map.insert(id.clone(), flags.clone());
    }

    let mut bytes_done: u64 = 0;
    let mut files_done: u32 = 0;
    let mut completed_files: Vec<String> = Vec::new();

    let result = (|| {
        for src in &sources {
            if flags.cancel.load(Ordering::Relaxed) {
                return Err(FmError::Other("Operation cancelled".into()));
            }
            if flags.pause.load(Ordering::Relaxed) {
                return Ok(Some(TransferCheckpoint {
                    files_completed: completed_files.clone(),
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

                let _ = channel.send(ProgressEvent {
                    id: id.clone(),
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
                    &id,
                    &mut bytes_done,
                    bytes_total,
                    &mut files_done,
                    files_total,
                    &channel,
                    &flags,
                    &mut completed_files,
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
    })();

    // Clean up the flags from state.
    if let Ok(mut map) = state.0.lock() {
        map.remove(&id);
    }

    result
}

/// Delete one or more files/directories.
///
/// When `use_trash` is true the `trash` crate is used to move items to the
/// system trash instead of permanently deleting them.
#[tauri::command]
pub fn delete_files(paths: Vec<String>, use_trash: bool) -> Result<(), FmError> {
    for p in &paths {
        let path = PathBuf::from(p);
        if !path.exists() {
            return Err(FmError::NotFound(p.clone()));
        }

        if use_trash {
            trash::delete(&path)?;
        } else if path.is_dir() {
            fs::remove_dir_all(&path)?;
        } else {
            fs::remove_file(&path)?;
        }
    }
    Ok(())
}

/// Delete files and return trash location info for undo support.
///
/// On macOS, uses NSFileManager.trashItemAtURL to get the resulting trash URL.
/// On Linux, uses the trash crate.
#[tauri::command]
pub fn delete_files_undoable(paths: Vec<String>) -> Result<Vec<TrashInfo>, FmError> {
    let mut results = Vec::new();

    for p in &paths {
        let path = PathBuf::from(p);
        if !path.exists() {
            return Err(FmError::NotFound(p.clone()));
        }

        let trash_path = trash_item_platform(&path)?;
        results.push(TrashInfo {
            original_path: p.clone(),
            trash_path,
        });
    }

    Ok(results)
}

#[cfg(target_os = "macos")]
fn trash_item_platform(path: &Path) -> Result<String, FmError> {
    use objc2::rc::Retained;
    use objc2_foundation::{NSFileManager, NSString, NSURL};

    let abs = path.canonicalize().map_err(|e| FmError::Io(e))?;
    let path_str = abs.to_string_lossy();
    let ns_path = NSString::from_str(&path_str);
    let url = NSURL::fileURLWithPath(&ns_path);

    let fm = NSFileManager::defaultManager();
    let mut resulting_url: Option<Retained<NSURL>> = None;

    let ok = fm.trashItemAtURL_resultingItemURL_error(
        &url,
        Some(&mut resulting_url),
    );

    if !ok.is_ok() {
        return Err(FmError::Other(format!("Failed to trash: {}", path_str)));
    }

    let trash_url = resulting_url
        .ok_or_else(|| FmError::Other("No resulting trash URL".into()))?;
    let trash_path_ns = trash_url.path()
        .ok_or_else(|| FmError::Other("Trash URL has no path".into()))?;
    Ok(trash_path_ns.to_string())
}

#[cfg(not(target_os = "macos"))]
fn trash_item_platform(path: &Path) -> Result<String, FmError> {
    // On Linux, use the trash crate and record the original path
    // The trash crate moves items but doesn't easily return the trash location
    trash::delete(path)?;
    // Return a sentinel — undo on Linux would need trash::os_limited which is
    // not reliably available, so we return the original path as a marker
    Ok(path.to_string_lossy().into_owned())
}

/// Restore files from trash by moving them back to their original locations.
///
/// On macOS, this simply renames the trash path back to the original path.
/// The caller is responsible for ensuring the original parent directory exists.
#[tauri::command]
pub fn restore_from_trash(items: Vec<TrashInfo>) -> Result<(), FmError> {
    for item in &items {
        let trash_path = PathBuf::from(&item.trash_path);
        let original_path = PathBuf::from(&item.original_path);

        if !trash_path.exists() {
            return Err(FmError::NotFound(format!(
                "Trash item not found: {}",
                item.trash_path
            )));
        }

        // Ensure the parent directory exists
        if let Some(parent) = original_path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }

        fs::rename(&trash_path, &original_path)?;
    }
    Ok(())
}

/// Check which source items would collide with existing files at the destination.
///
/// Returns the list of source paths whose filename already exists in `destination`.
#[tauri::command]
pub fn check_conflicts(sources: Vec<String>, destination: String) -> Vec<String> {
    let dest = PathBuf::from(&destination);
    sources
        .into_iter()
        .filter(|src| {
            if let Some(name) = PathBuf::from(src).file_name() {
                dest.join(name).exists()
            } else {
                false
            }
        })
        .collect()
}

/// Rename a file or directory.
///
/// `new_name` is just the file/directory name, not a full path.  The item
/// stays in the same parent directory.
#[tauri::command]
pub fn rename_file(path: String, new_name: String) -> Result<(), FmError> {
    let src = PathBuf::from(&path);
    if !src.exists() {
        return Err(FmError::NotFound(path));
    }

    // Reject names containing path separators to prevent path traversal.
    if new_name.contains('/') || new_name.contains('\0') {
        return Err(FmError::Other(
            "new_name must be a plain file name without path separators".into(),
        ));
    }

    let parent = src
        .parent()
        .ok_or_else(|| FmError::Other("cannot determine parent directory".into()))?;
    let dst = parent.join(&new_name);

    if dst.exists() {
        return Err(FmError::AlreadyExists(dst.to_string_lossy().into_owned()));
    }

    fs::rename(&src, &dst)?;
    Ok(())
}

/// Cancel a running file operation (copy, move, or extract) by its ID.
#[tauri::command]
pub fn cancel_file_operation(
    id: String,
    state: tauri::State<'_, FileOpState>,
) -> Result<(), FmError> {
    let map = state.0.lock().map_err(|e| FmError::Other(e.to_string()))?;
    if let Some(flags) = map.get(&id) {
        flags.cancel.store(true, Ordering::Relaxed);
    }
    Ok(())
}

/// Pause a running file operation by its ID.
#[tauri::command]
pub fn pause_file_operation(
    id: String,
    state: tauri::State<'_, FileOpState>,
) -> Result<(), FmError> {
    let map = state.0.lock().map_err(|e| FmError::Other(e.to_string()))?;
    if let Some(flags) = map.get(&id) {
        flags.pause.store(true, Ordering::Relaxed);
    }
    Ok(())
}
