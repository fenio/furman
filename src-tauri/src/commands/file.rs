use crate::models::{FmError, ProgressEvent};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::ipc::Channel;

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
) -> Result<(), FmError> {
    if src.is_dir() {
        fs::create_dir_all(dst)?;
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            let child_src = entry.path();
            let child_dst = dst.join(entry.file_name());
            copy_recursive(
                &child_src,
                &child_dst,
                id,
                bytes_done,
                bytes_total,
                files_done,
                files_total,
                channel,
            )?;
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

        let _ = channel.send(ProgressEvent {
            id: id.to_string(),
            bytes_done: *bytes_done,
            bytes_total,
            current_file: src.to_string_lossy().into_owned(),
            files_done: *files_done,
            files_total,
        });
    }
    Ok(())
}

// ── Commands ─────────────────────────────────────────────────────────────────

/// Copy one or more files/directories to `destination` with progress reporting.
#[tauri::command]
pub fn copy_files(
    sources: Vec<String>,
    destination: String,
    channel: Channel<ProgressEvent>,
) -> Result<(), FmError> {
    let dest = PathBuf::from(&destination);

    // Pre-calculate totals for progress.
    let mut bytes_total: u64 = 0;
    let mut files_total: u32 = 0;
    for src in &sources {
        let p = PathBuf::from(src);
        bytes_total += total_bytes(&p);
        files_total += count_files(&p);
    }

    let id = format!("copy-{}", std::process::id());
    let mut bytes_done: u64 = 0;
    let mut files_done: u32 = 0;

    for src in &sources {
        let src_path = PathBuf::from(src);
        let file_name = src_path
            .file_name()
            .ok_or_else(|| FmError::Other(format!("invalid source path: {src}")))?;
        let dst_path = dest.join(file_name);

        copy_recursive(
            &src_path,
            &dst_path,
            &id,
            &mut bytes_done,
            bytes_total,
            &mut files_done,
            files_total,
            &channel,
        )?;
    }

    Ok(())
}

/// Move one or more files/directories to `destination` with progress reporting.
///
/// Attempts a fast `rename` first; falls back to copy + delete if the rename
/// fails (e.g. cross-device move).
#[tauri::command]
pub fn move_files(
    sources: Vec<String>,
    destination: String,
    channel: Channel<ProgressEvent>,
) -> Result<(), FmError> {
    let dest = PathBuf::from(&destination);

    // Pre-calculate totals.
    let mut bytes_total: u64 = 0;
    let mut files_total: u32 = 0;
    for src in &sources {
        let p = PathBuf::from(src);
        bytes_total += total_bytes(&p);
        files_total += count_files(&p);
    }

    let id = format!("move-{}", std::process::id());
    let mut bytes_done: u64 = 0;
    let mut files_done: u32 = 0;

    for src in &sources {
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
            copy_recursive(
                &src_path,
                &dst_path,
                &id,
                &mut bytes_done,
                bytes_total,
                &mut files_done,
                files_total,
                &channel,
            )?;

            if src_path.is_dir() {
                fs::remove_dir_all(&src_path)?;
            } else {
                fs::remove_file(&src_path)?;
            }
        }
    }

    Ok(())
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
