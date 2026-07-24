use crate::local;
use crate::models::{FmError, ProgressEvent, TrashInfo, TransferCheckpoint};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use tauri::ipc::Channel;

pub use local::OpFlags;

pub struct FileOpState(pub Mutex<HashMap<String, Arc<OpFlags>>>);

// ── Commands ─────────────────────────────────────────────────────────────────

/// Copy one or more files/directories to `destination` with progress reporting.
/// Returns None on success, Some(checkpoint) on pause.
#[tauri::command]
pub fn copy_files(
    id: String,
    sources: Vec<String>,
    destination: String,
    checkpoint: Option<TransferCheckpoint>,
    channel: Channel<ProgressEvent>,
    state: tauri::State<'_, FileOpState>,
) -> Result<Option<TransferCheckpoint>, FmError> {
    let flags = Arc::new(OpFlags {
        cancel: AtomicBool::new(false),
        pause: AtomicBool::new(false),
    });
    {
        let mut map = state.0.lock().map_err(|e| FmError::Other(e.to_string()))?;
        map.insert(id.clone(), flags.clone());
    }

    let result = local::copy_files_core_with_checkpoint(
        &id,
        &sources,
        &destination,
        &flags,
        &|event| {
            let _ = channel.send(event);
        },
        checkpoint.as_ref(),
    );

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
    checkpoint: Option<TransferCheckpoint>,
    channel: Channel<ProgressEvent>,
    state: tauri::State<'_, FileOpState>,
) -> Result<Option<TransferCheckpoint>, FmError> {
    let flags = Arc::new(OpFlags {
        cancel: AtomicBool::new(false),
        pause: AtomicBool::new(false),
    });
    {
        let mut map = state.0.lock().map_err(|e| FmError::Other(e.to_string()))?;
        map.insert(id.clone(), flags.clone());
    }

    let result = local::move_files_core_with_checkpoint(
        &id,
        &sources,
        &destination,
        &flags,
        &|event| {
            let _ = channel.send(event);
        },
        checkpoint.as_ref(),
    );

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
            std::fs::remove_dir_all(&path)?;
        } else {
            std::fs::remove_file(&path)?;
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
fn trash_item_platform(path: &std::path::Path) -> Result<String, FmError> {
    use objc2::rc::Retained;
    use objc2_foundation::{NSFileManager, NSString, NSURL};

    let abs = path.canonicalize().map_err(FmError::Io)?;
    let path_str = abs.to_string_lossy();
    let ns_path = NSString::from_str(&path_str);
    let url = NSURL::fileURLWithPath(&ns_path);

    let fm = NSFileManager::defaultManager();
    let mut resulting_url: Option<Retained<NSURL>> = None;

    let ok = fm.trashItemAtURL_resultingItemURL_error(&url, Some(&mut resulting_url));

    if ok.is_err() {
        return Err(FmError::Other(format!("Failed to trash: {path_str}")));
    }

    let trash_url = resulting_url.ok_or_else(|| FmError::Other("No resulting trash URL".into()))?;
    let trash_path_ns = trash_url
        .path()
        .ok_or_else(|| FmError::Other("Trash URL has no path".into()))?;
    Ok(trash_path_ns.to_string())
}

#[cfg(not(target_os = "macos"))]
fn trash_item_platform(path: &std::path::Path) -> Result<String, FmError> {
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
                std::fs::create_dir_all(parent)?;
            }
        }

        std::fs::rename(&trash_path, &original_path)?;
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

    std::fs::rename(&src, &dst)?;
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
        flags.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
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
        flags.pause.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    Ok(())
}
