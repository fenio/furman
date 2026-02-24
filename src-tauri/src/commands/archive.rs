use crate::commands::file::FileOpState;
use crate::models::{DirListing, FileEntry, FmError, ProgressEvent};
use std::collections::HashSet;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::ipc::Channel;

/// Represents a raw entry parsed from 7z output.
struct RawEntry {
    full_path: String,
    size: u64,
    is_dir: bool,
    modified: i64, // epoch ms
}

/// List the contents of an archive at a given internal path.
///
/// Uses the `7z` CLI tool to list archive contents.
/// Supports ZIP, 7Z, RAR, TAR, GZ, and any other format 7z supports.
#[tauri::command]
pub fn list_archive(archive_path: String, internal_path: String) -> Result<DirListing, FmError> {
    let fs_path = Path::new(&archive_path);
    if !fs_path.exists() {
        return Err(FmError::NotFound(archive_path.clone()));
    }

    let raw_entries = list_7z_cli(&archive_path)?;

    let prefix = internal_path.trim_matches('/').to_string();

    build_listing(&archive_path, &prefix, &raw_entries)
}

/// Run `7z l -slt <archive>` and parse the technical listing output.
///
/// The -slt flag produces a machine-readable format with key = value pairs per entry.
fn list_7z_cli(archive_path: &str) -> Result<Vec<RawEntry>, FmError> {
    let output = Command::new("7z")
        .args(["l", "-slt", archive_path])
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                FmError::Other(
                    "7z not found. Install with: brew install 7zip".to_string(),
                )
            } else {
                FmError::Other(format!("Failed to run 7z: {e}"))
            }
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(FmError::Other(format!("7z failed: {stderr}")));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_7z_slt(&stdout)
}

/// Parse 7z -slt output format.
///
/// Each entry is a block of "Key = Value" lines separated by blank lines.
/// Key fields we care about: Path, Size, Folder, Modified
fn parse_7z_slt(output: &str) -> Result<Vec<RawEntry>, FmError> {
    let mut entries = Vec::new();
    let mut current_path: Option<String> = None;
    let mut current_size: u64 = 0;
    let mut current_is_dir = false;
    let mut current_modified: i64 = 0;
    let mut in_entry = false;

    for line in output.lines() {
        let line = line.trim();

        if line.is_empty() {
            // End of a block — save if we have an entry
            if in_entry {
                if let Some(path) = current_path.take() {
                    if !path.is_empty() {
                        entries.push(RawEntry {
                            full_path: path,
                            size: current_size,
                            is_dir: current_is_dir,
                            modified: current_modified,
                        });
                    }
                }
                in_entry = false;
                current_size = 0;
                current_is_dir = false;
                current_modified = 0;
            }
            continue;
        }

        if let Some((key, value)) = line.split_once(" = ") {
            let key = key.trim();
            let value = value.trim();
            match key {
                "Path" => {
                    // Skip the first "Path" which is the archive itself (appears before entries)
                    in_entry = true;
                    current_path = Some(value.replace('\\', "/"));
                }
                "Size" => {
                    current_size = value.parse().unwrap_or(0);
                }
                "Folder" => {
                    current_is_dir = value == "+";
                }
                "Modified" => {
                    current_modified = parse_7z_datetime(value);
                }
                _ => {}
            }
        }
    }

    // Don't forget the last entry
    if in_entry {
        if let Some(path) = current_path.take() {
            if !path.is_empty() {
                entries.push(RawEntry {
                    full_path: path,
                    size: current_size,
                    is_dir: current_is_dir,
                    modified: current_modified,
                });
            }
        }
    }

    // The first entry from 7z -slt is the archive itself; remove it
    if !entries.is_empty() {
        entries.remove(0);
    }

    Ok(entries)
}

/// Parse a 7z datetime string like "2024-01-15 10:30:22" into epoch milliseconds.
fn parse_7z_datetime(s: &str) -> i64 {
    // Try parsing "YYYY-MM-DD HH:MM:SS" format
    chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f"))
        .map(|dt| dt.and_utc().timestamp_millis())
        .unwrap_or(0)
}

/// Build a DirListing from raw archive entries, showing only immediate children
/// at the given prefix level (like readdir).
fn build_listing(
    archive_path: &str,
    prefix: &str,
    raw_entries: &[RawEntry],
) -> Result<DirListing, FmError> {
    let mut entries: Vec<FileEntry> = Vec::new();
    let mut total_size: u64 = 0;
    let mut seen_dirs: HashSet<String> = HashSet::new();

    // ".." entry — at archive root, points to real filesystem parent;
    // inside a subdirectory, points to parent within archive
    let parent_path = if prefix.is_empty() {
        Path::new(archive_path)
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "/".to_string())
    } else {
        let parent_internal = match prefix.rfind('/') {
            Some(pos) => &prefix[..pos],
            None => "",
        };
        format!("archive://{archive_path}#{parent_internal}")
    };

    entries.push(FileEntry {
        name: "..".to_string(),
        path: parent_path,
        size: 0,
        is_dir: true,
        is_symlink: false,
        symlink_target: None,
        modified: 0,
        permissions: 0,
        owner: String::new(),
        group: String::new(),
        extension: None,
        git_status: None,
    });

    let prefix_with_slash = if prefix.is_empty() {
        String::new()
    } else {
        format!("{prefix}/")
    };

    for raw in raw_entries {
        let entry_path = raw.full_path.trim_end_matches('/');

        let relative = if prefix_with_slash.is_empty() {
            entry_path.to_string()
        } else if let Some(rel) = entry_path.strip_prefix(&prefix_with_slash) {
            rel.to_string()
        } else {
            continue;
        };

        if relative.is_empty() {
            continue;
        }

        if let Some(slash_pos) = relative.find('/') {
            // Deeper entry — implies a subdirectory
            let dir_name = &relative[..slash_pos];
            if seen_dirs.insert(dir_name.to_string()) {
                let internal = if prefix.is_empty() {
                    dir_name.to_string()
                } else {
                    format!("{prefix}/{dir_name}")
                };
                entries.push(FileEntry {
                    name: dir_name.to_string(),
                    path: format!("archive://{archive_path}#{internal}"),
                    size: 0,
                    is_dir: true,
                    is_symlink: false,
                    symlink_target: None,
                    modified: 0,
                    permissions: 0o755,
                    owner: String::new(),
                    group: String::new(),
                    extension: None,
                    git_status: None,
                });
            }
        } else if raw.is_dir {
            if seen_dirs.insert(relative.clone()) {
                let internal = if prefix.is_empty() {
                    relative.clone()
                } else {
                    format!("{prefix}/{relative}")
                };
                entries.push(FileEntry {
                    name: relative.clone(),
                    path: format!("archive://{archive_path}#{internal}"),
                    size: 0,
                    is_dir: true,
                    is_symlink: false,
                    symlink_target: None,
                    modified: raw.modified,
                    permissions: 0o755,
                    owner: String::new(),
                    group: String::new(),
                    extension: None,
                    git_status: None,
                });
            }
        } else {
            let ext = Path::new(&relative)
                .extension()
                .map(|e| e.to_string_lossy().into_owned());
            let internal = if prefix.is_empty() {
                relative.clone()
            } else {
                format!("{prefix}/{relative}")
            };
            total_size += raw.size;
            entries.push(FileEntry {
                name: relative.clone(),
                path: format!("archive://{archive_path}#{internal}"),
                size: raw.size,
                is_dir: false,
                is_symlink: false,
                symlink_target: None,
                modified: raw.modified,
                permissions: 0o644,
                owner: String::new(),
                group: String::new(),
                extension: ext,
                git_status: None,
            });
        }
    }

    // Sort: directories first, then alphabetical case-insensitive (skip "..")
    entries[1..].sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    let archive_name = Path::new(archive_path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| archive_path.to_string());
    let display_path = if prefix.is_empty() {
        format!("[{archive_name}]")
    } else {
        format!("[{archive_name}]/{prefix}")
    };

    Ok(DirListing {
        path: display_path,
        entries,
        total_size,
        free_space: 0,
    })
}

/// Extract specific files/directories from an archive to a destination directory.
///
/// `archive_path` — filesystem path to the archive.
/// `internal_paths` — list of paths inside the archive (the part after `#` in `archive://...#path`).
/// `destination` — filesystem directory to extract into.
#[tauri::command]
pub fn extract_archive(
    id: String,
    archive_path: String,
    internal_paths: Vec<String>,
    destination: String,
    channel: Channel<ProgressEvent>,
    state: tauri::State<'_, FileOpState>,
) -> Result<(), FmError> {
    let fs_path = Path::new(&archive_path);
    if !fs_path.exists() {
        return Err(FmError::NotFound(archive_path.clone()));
    }

    let dest = Path::new(&destination);
    if !dest.exists() {
        return Err(FmError::NotFound(destination.clone()));
    }

    let files_total = internal_paths.len() as u32;

    let cancel_flag = Arc::new(AtomicBool::new(false));
    {
        let mut map = state.0.lock().map_err(|e| FmError::Other(e.to_string()))?;
        map.insert(id.clone(), cancel_flag.clone());
    }

    // Use `7z x` to extract with full paths, targeting specific files.
    // -o sets output directory, -y auto-confirms overwrites.
    let mut args = vec![
        "x".to_string(),
        archive_path.clone(),
        format!("-o{destination}"),
        "-y".to_string(),
    ];
    for p in &internal_paths {
        args.push(p.clone());
    }

    let mut child = Command::new("7z")
        .args(&args)
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                FmError::Other("7z not found. Install with: brew install 7zip".to_string())
            } else {
                FmError::Other(format!("Failed to run 7z: {e}"))
            }
        })?;

    // Poll the child process, checking cancel flag between polls.
    let result = loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if status.success() {
                    break Ok(());
                } else {
                    break Err(FmError::Other(format!(
                        "7z extract failed with exit code: {}",
                        status.code().unwrap_or(-1)
                    )));
                }
            }
            Ok(None) => {
                // Still running — check cancel flag.
                if cancel_flag.load(Ordering::Relaxed) {
                    let _ = child.kill();
                    let _ = child.wait();
                    break Err(FmError::Other("Operation cancelled".into()));
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(e) => {
                break Err(FmError::Other(format!("Failed to wait on 7z: {e}")));
            }
        }
    };

    // Clean up the cancel flag from state.
    if let Ok(mut map) = state.0.lock() {
        map.remove(&id);
    }

    result?;

    // Report completion
    let _ = channel.send(ProgressEvent {
        id,
        bytes_done: 0,
        bytes_total: 0,
        current_file: String::new(),
        files_done: files_total,
        files_total,
    });

    Ok(())
}
