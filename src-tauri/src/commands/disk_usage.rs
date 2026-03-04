use crate::models::{DiskUsageDone, DiskUsageEntry, DiskUsageEvent, FmError};
use std::collections::HashMap;
use std::fs;
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

fn do_analyze(root: &str, channel: &Channel<DiskUsageEvent>, cancel_flag: &Arc<AtomicBool>) {
    let root_path = PathBuf::from(root);
    let mut total_size: u64 = 0;
    let mut total_files: u64 = 0;
    let mut total_dirs: u64 = 0;
    let mut files_scanned: u64 = 0;
    let mut last_progress = Instant::now();

    let entries = match fs::read_dir(&root_path) {
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

    for entry in entries.flatten() {
        if cancel_flag.load(Ordering::Relaxed) {
            let _ = channel.send(DiskUsageEvent::Done(DiskUsageDone {
                total_size,
                total_files,
                total_dirs,
                cancelled: true,
            }));
            return;
        }

        let path = entry.path();
        let metadata = match fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };

        // Skip symlink directories to avoid loops
        if metadata.is_symlink() && path.is_dir() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().to_string();
        let is_dir = metadata.is_dir();

        if is_dir {
            // Walk the directory tree with a stack (non-recursive)
            let mut dir_size: u64 = 0;
            let mut item_count: u64 = 0;
            let mut stack: Vec<PathBuf> = vec![path.clone()];

            while let Some(dir) = stack.pop() {
                if cancel_flag.load(Ordering::Relaxed) {
                    let _ = channel.send(DiskUsageEvent::Done(DiskUsageDone {
                        total_size,
                        total_files,
                        total_dirs,
                        cancelled: true,
                    }));
                    return;
                }

                let sub_entries = match fs::read_dir(&dir) {
                    Ok(rd) => rd,
                    Err(_) => continue,
                };

                for sub_entry in sub_entries.flatten() {
                    let sub_path = sub_entry.path();
                    let sub_meta = match fs::symlink_metadata(&sub_path) {
                        Ok(m) => m,
                        Err(_) => continue,
                    };

                    if sub_meta.is_symlink() && sub_path.is_dir() {
                        continue;
                    }

                    item_count += 1;
                    files_scanned += 1;

                    if sub_meta.is_dir() {
                        stack.push(sub_path);
                    } else {
                        dir_size += sub_meta.len();
                    }

                    // Throttle progress events to 50ms intervals
                    if last_progress.elapsed().as_millis() >= 50 {
                        let _ = channel.send(DiskUsageEvent::Progress { files_scanned });
                        last_progress = Instant::now();
                    }
                }
            }

            total_size += dir_size;
            total_dirs += 1;

            let _ = channel.send(DiskUsageEvent::Entry(DiskUsageEntry {
                name,
                path: path.to_string_lossy().to_string(),
                size: dir_size,
                is_dir: true,
                item_count,
            }));
        } else {
            let file_size = metadata.len();
            total_size += file_size;
            total_files += 1;
            files_scanned += 1;

            let _ = channel.send(DiskUsageEvent::Entry(DiskUsageEntry {
                name,
                path: path.to_string_lossy().to_string(),
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
