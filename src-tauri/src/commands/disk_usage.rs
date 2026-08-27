use crate::models::{DiskUsageDone, DiskUsageEntry, DiskUsageEvent, DiskUsageLevelData, FmError};
use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::ipc::Channel;

// ── Managed state ───────────────────────────────────────────────────────────

pub struct DiskUsageState(pub Mutex<HashMap<String, Arc<AtomicBool>>>);

// ── Commands ────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn analyze_disk_usage(
    id: String,
    path: String,
    channel: Channel<DiskUsageEvent>,
    state: tauri::State<'_, DiskUsageState>,
) -> Result<(), FmError> {
    let cancel_flag = Arc::new(AtomicBool::new(false));
    {
        let mut map = state.0.lock().map_err(|e| FmError::Other(e.to_string()))?;
        map.insert(id.clone(), cancel_flag.clone());
    }

    std::thread::spawn(move || {
        do_analyze(&path, &channel, &cancel_flag);
    });

    Ok(())
}

#[tauri::command]
pub fn cancel_disk_usage(
    id: String,
    state: tauri::State<'_, DiskUsageState>,
) -> Result<(), FmError> {
    let map = state.0.lock().map_err(|e| FmError::Other(e.to_string()))?;
    if let Some(flag) = map.get(&id) {
        flag.store(true, Ordering::Relaxed);
    }
    Ok(())
}

// ── Implementation ──────────────────────────────────────────────────────────

fn allocated_size(metadata: &fs::Metadata) -> u64 {
    // Unix st_blocks values are expressed in 512-byte units.
    metadata.blocks().saturating_mul(512)
}

/// Recursively walks `path`, collecting direct children with their sizes and
/// emitting a `Level` event for each directory so the frontend can cache every
/// level of the tree for instant navigation — all within a single scan pass.
///
/// Returns `None` if cancelled (the `Done` event has already been sent).
/// Returns `Some((size, item_count, direct_files, direct_dirs))` otherwise.
fn walk_recursive(
    path: &PathBuf,
    cancel_flag: &Arc<AtomicBool>,
    channel: &Channel<DiskUsageEvent>,
    partial_total_size: u64,
    partial_total_files: u64,
    partial_total_dirs: u64,
    files_scanned: &mut u64,
    last_progress: &mut Instant,
) -> Option<(u64, u64, u64, u64)> {
    let mut dir_size: u64 = 0;
    let mut item_count: u64 = 0;
    let mut level_entries: Vec<DiskUsageEntry> = Vec::new();
    let mut level_files: u64 = 0;
    let mut level_dirs: u64 = 0;

    let entries = match fs::read_dir(path) {
        Ok(rd) => rd,
        Err(_) => return Some((0, 0, 0, 0)),
    };

    for entry in entries.flatten() {
        if cancel_flag.load(Ordering::Relaxed) {
            let _ = channel.send(DiskUsageEvent::Done(DiskUsageDone {
                total_size: partial_total_size,
                total_files: partial_total_files,
                total_dirs: partial_total_dirs,
                cancelled: true,
            }));
            return None;
        }

        let child_path = entry.path();
        let child_meta = match fs::symlink_metadata(&child_path) {
            Ok(m) => m,
            Err(_) => continue,
        };

        // Skip symlink directories to avoid loops
        if child_meta.is_symlink() && child_path.is_dir() {
            continue;
        }

        let child_name = entry.file_name().to_string_lossy().to_string();

        if child_meta.is_dir() {
            let (child_size, child_items, child_files, child_dirs) = walk_recursive(
                &child_path,
                cancel_flag,
                channel,
                partial_total_size,
                partial_total_files,
                partial_total_dirs,
                files_scanned,
                last_progress,
            )?;

            dir_size += child_size;
            item_count += child_items + 1;
            level_dirs += 1;

            let _ = child_files; // captured in subtree; not needed here
            let _ = child_dirs;

            level_entries.push(DiskUsageEntry {
                name: child_name,
                path: child_path.to_string_lossy().to_string(),
                size: child_size,
                is_dir: true,
                item_count: child_items,
            });
        } else {
            let file_size = allocated_size(&child_meta);
            dir_size += file_size;
            item_count += 1;
            level_files += 1;
            *files_scanned += 1;

            level_entries.push(DiskUsageEntry {
                name: child_name,
                path: child_path.to_string_lossy().to_string(),
                size: file_size,
                is_dir: false,
                item_count: 1,
            });

            if last_progress.elapsed().as_millis() >= 50 {
                let _ = channel.send(DiskUsageEvent::Progress {
                    files_scanned: *files_scanned,
                });
                *last_progress = Instant::now();
            }
        }
    }

    // Emit Level event so the frontend caches this directory's children —
    // every directory in the tree gets one, enabling instant navigation at
    // any depth without extra scans.
    if !level_entries.is_empty() {
        let _ = channel.send(DiskUsageEvent::Level(DiskUsageLevelData {
            parent_path: path.to_string_lossy().to_string(),
            entries: level_entries,
            total_size: dir_size,
            total_files: level_files,
            total_dirs: level_dirs,
        }));
    }

    Some((dir_size, item_count, level_files, level_dirs))
}

fn do_analyze(root: &str, channel: &Channel<DiskUsageEvent>, cancel_flag: &Arc<AtomicBool>) {
    let root_path = PathBuf::from(root);
    let mut total_size: u64 = 0;
    let mut total_files: u64 = 0;
    let mut total_dirs: u64 = 0;
    let mut files_scanned: u64 = 0;
    let mut last_progress = Instant::now();

    let top_entries = match fs::read_dir(&root_path) {
        Ok(rd) => rd,
        Err(_) => {
            let _ = channel.send(DiskUsageEvent::Done(DiskUsageDone {
                total_size: 0,
                total_files: 0,
                total_dirs: 0,
                cancelled: false,
            }));
            return;
        }
    };

    for top_entry in top_entries.flatten() {
        if cancel_flag.load(Ordering::Relaxed) {
            let _ = channel.send(DiskUsageEvent::Done(DiskUsageDone {
                total_size,
                total_files,
                total_dirs,
                cancelled: true,
            }));
            return;
        }

        let top_path = top_entry.path();
        let top_meta = match fs::symlink_metadata(&top_path) {
            Ok(m) => m,
            Err(_) => continue,
        };

        if top_meta.is_symlink() && top_path.is_dir() {
            continue;
        }

        let top_name = top_entry.file_name().to_string_lossy().to_string();

        if top_meta.is_dir() {
            let result = walk_recursive(
                &top_path,
                cancel_flag,
                channel,
                total_size,
                total_files,
                total_dirs,
                &mut files_scanned,
                &mut last_progress,
            );

            match result {
                None => return, // cancelled; Done already sent
                Some((dir_size, item_count, _, _)) => {
                    total_size += dir_size;
                    total_dirs += 1;

                    let _ = channel.send(DiskUsageEvent::Entry(DiskUsageEntry {
                        name: top_name,
                        path: top_path.to_string_lossy().to_string(),
                        size: dir_size,
                        is_dir: true,
                        item_count,
                    }));
                }
            }
        } else {
            let file_size = allocated_size(&top_meta);
            total_size += file_size;
            total_files += 1;
            files_scanned += 1;

            let _ = channel.send(DiskUsageEvent::Entry(DiskUsageEntry {
                name: top_name,
                path: top_path.to_string_lossy().to_string(),
                size: file_size,
                is_dir: false,
                item_count: 1,
            }));

            if last_progress.elapsed().as_millis() >= 50 {
                let _ = channel.send(DiskUsageEvent::Progress { files_scanned });
                last_progress = Instant::now();
            }
        }
    }

    let _ = channel.send(DiskUsageEvent::Done(DiskUsageDone {
        total_size,
        total_files,
        total_dirs,
        cancelled: false,
    }));
}

#[cfg(test)]
mod tests {
    use super::allocated_size;

    #[test]
    fn sparse_file_reports_allocated_size() {
        let file = tempfile::NamedTempFile::new().unwrap();
        file.as_file().set_len(1024 * 1024 * 1024).unwrap();
        let metadata = file.as_file().metadata().unwrap();

        assert!(allocated_size(&metadata) < metadata.len());
    }
}
