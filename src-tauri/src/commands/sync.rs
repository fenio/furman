use crate::s3::S3State;
use crate::models::{FmError, SyncEntry, SyncEvent};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::UNIX_EPOCH;
use tauri::ipc::Channel;
use tauri::State;

// ── Managed state ───────────────────────────────────────────────────────

pub struct SyncState(pub Mutex<HashMap<String, Arc<AtomicBool>>>);

// ── Commands ────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn sync_diff(
    id: String,
    source_backend: String,  // "local" | "s3"
    source_path: String,     // local dir path or s3://bucket/prefix
    source_s3_id: String,    // "" for local
    dest_backend: String,
    dest_path: String,
    dest_s3_id: String,
    channel: Channel<SyncEvent>,
    s3_state: State<'_, S3State>,
    sync_state: State<'_, SyncState>,
) -> Result<(), FmError> {
    let cancel_flag = Arc::new(AtomicBool::new(false));
    {
        let mut map = sync_state
            .0
            .lock()
            .map_err(|e| FmError::Other(e.to_string()))?;
        map.insert(id.clone(), cancel_flag.clone());
    }

    // Collect source files
    let source_files = collect_files(
        &source_backend,
        &source_path,
        &source_s3_id,
        &s3_state,
    )
    .await?;

    if cancel_flag.load(Ordering::Relaxed) {
        cleanup(&sync_state, &id);
        return Ok(());
    }

    // Collect dest files
    let dest_files = collect_files(
        &dest_backend,
        &dest_path,
        &dest_s3_id,
        &s3_state,
    )
    .await?;

    if cancel_flag.load(Ordering::Relaxed) {
        cleanup(&sync_state, &id);
        return Ok(());
    }

    // Compare and stream entries
    let mut scanned: u32 = 0;
    let mut new_count: u32 = 0;
    let mut modified: u32 = 0;
    let mut deleted: u32 = 0;
    let mut same_count: u32 = 0;

    // Keys in source
    for (rel_path, (src_size, src_modified)) in &source_files {
        if cancel_flag.load(Ordering::Relaxed) {
            cleanup(&sync_state, &id);
            return Ok(());
        }

        let entry = if let Some((dst_size, dst_modified)) = dest_files.get(rel_path) {
            if src_size != dst_size || *src_modified > *dst_modified {
                modified += 1;
                SyncEntry {
                    relative_path: rel_path.clone(),
                    status: "modified".to_string(),
                    source_size: *src_size,
                    dest_size: *dst_size,
                    source_modified: *src_modified,
                    dest_modified: *dst_modified,
                }
            } else {
                same_count += 1;
                SyncEntry {
                    relative_path: rel_path.clone(),
                    status: "same".to_string(),
                    source_size: *src_size,
                    dest_size: *dst_size,
                    source_modified: *src_modified,
                    dest_modified: *dst_modified,
                }
            }
        } else {
            new_count += 1;
            SyncEntry {
                relative_path: rel_path.clone(),
                status: "new".to_string(),
                source_size: *src_size,
                dest_size: 0,
                source_modified: *src_modified,
                dest_modified: 0,
            }
        };

        let _ = channel.send(SyncEvent::Entry(entry));
        scanned += 1;

        if scanned % 100 == 0 {
            let _ = channel.send(SyncEvent::Progress { scanned });
        }
    }

    // Keys only in dest (deleted from source perspective)
    for (rel_path, (dst_size, dst_modified)) in &dest_files {
        if cancel_flag.load(Ordering::Relaxed) {
            cleanup(&sync_state, &id);
            return Ok(());
        }

        if !source_files.contains_key(rel_path) {
            deleted += 1;
            let entry = SyncEntry {
                relative_path: rel_path.clone(),
                status: "deleted".to_string(),
                source_size: 0,
                dest_size: *dst_size,
                source_modified: 0,
                dest_modified: *dst_modified,
            };
            let _ = channel.send(SyncEvent::Entry(entry));
            scanned += 1;

            if scanned % 100 == 0 {
                let _ = channel.send(SyncEvent::Progress { scanned });
            }
        }
    }

    let total = new_count + modified + deleted + same_count;
    let _ = channel.send(SyncEvent::Done {
        total,
        new_count,
        modified,
        deleted,
    });

    cleanup(&sync_state, &id);
    Ok(())
}

#[tauri::command]
pub fn cancel_sync(id: String, state: State<'_, SyncState>) -> Result<(), FmError> {
    let map = state
        .0
        .lock()
        .map_err(|e| FmError::Other(e.to_string()))?;
    if let Some(flag) = map.get(&id) {
        flag.store(true, Ordering::Relaxed);
    }
    Ok(())
}

// ── Helpers ─────────────────────────────────────────────────────────────

fn cleanup(sync_state: &State<'_, SyncState>, id: &str) {
    if let Ok(mut map) = sync_state.0.lock() {
        map.remove(id);
    }
}

/// Collect files from a backend into a HashMap of relative_path → (size, modified_ms).
async fn collect_files(
    backend: &str,
    path: &str,
    s3_id: &str,
    s3_state: &State<'_, S3State>,
) -> Result<HashMap<String, (u64, i64)>, FmError> {
    match backend {
        "local" => collect_local_files_recursive(Path::new(path)),
        "s3" => collect_s3_files(s3_id, path, s3_state).await,
        _ => Err(FmError::Other(format!("Unknown backend: {}", backend))),
    }
}

/// Recursively walk a local directory, returning relative paths with size and mtime.
fn collect_local_files_recursive(
    root: &Path,
) -> Result<HashMap<String, (u64, i64)>, FmError> {
    let mut result = HashMap::new();
    let root_str = root.to_string_lossy().to_string();
    walk_local(root, &root_str, &mut result)?;
    Ok(result)
}

fn walk_local(
    dir: &Path,
    root: &str,
    out: &mut HashMap<String, (u64, i64)>,
) -> Result<(), FmError> {
    let entries = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => return Ok(()),
        Err(e) => return Err(FmError::Io(e)),
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let metadata = match fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };

        // Skip symlink directories to avoid loops
        if metadata.is_symlink() && path.is_dir() {
            continue;
        }

        if metadata.is_dir() {
            walk_local(&path, root, out)?;
        } else {
            let full = path.to_string_lossy().to_string();
            // Build relative path by stripping root prefix
            let rel = if full.starts_with(root) {
                let r = &full[root.len()..];
                if r.starts_with('/') {
                    r[1..].to_string()
                } else {
                    r.to_string()
                }
            } else {
                full
            };

            let size = metadata.len();
            let modified = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);

            out.insert(rel, (size, modified));
        }
    }

    Ok(())
}

/// Collect S3 objects under a prefix, returning relative paths with size and mtime.
async fn collect_s3_files(
    s3_id: &str,
    path: &str,
    s3_state: &State<'_, S3State>,
) -> Result<HashMap<String, (u64, i64)>, FmError> {
    use aws_sdk_s3::Client as S3Client;

    // Extract bucket and prefix from s3://bucket/prefix/ path
    let (bucket, prefix) = parse_s3_path(path)?;

    let client: S3Client;
    let bucket_owned: String;
    {
        let map = s3_state
            .0
            .lock()
            .map_err(|e| FmError::Other(e.to_string()))?;
        let conn = map
            .get(s3_id)
            .ok_or_else(|| FmError::S3("S3 connection not found".to_string()))?;
        client = conn.client.clone();
        bucket_owned = conn.bucket.clone();
    }

    let actual_bucket = if bucket.is_empty() {
        &bucket_owned
    } else {
        &bucket
    };

    let objects = list_all_objects(&client, actual_bucket, &prefix).await?;

    let mut result = HashMap::new();
    for (key, size, modified) in objects {
        // Skip "directory" markers
        if key.ends_with('/') {
            continue;
        }
        // Strip prefix to get relative path
        let rel = if !prefix.is_empty() && key.starts_with(&prefix) {
            let r = &key[prefix.len()..];
            if r.starts_with('/') {
                r[1..].to_string()
            } else {
                r.to_string()
            }
        } else {
            key
        };
        if !rel.is_empty() {
            result.insert(rel, (size, modified));
        }
    }

    Ok(result)
}

/// Parse an s3://bucket/prefix path into (bucket, prefix).
fn parse_s3_path(path: &str) -> Result<(String, String), FmError> {
    if let Some(rest) = path.strip_prefix("s3://") {
        let slash = rest.find('/').unwrap_or(rest.len());
        let bucket = rest[..slash].to_string();
        let prefix = if slash < rest.len() {
            rest[slash + 1..].trim_end_matches('/').to_string()
        } else {
            String::new()
        };
        Ok((bucket, prefix))
    } else {
        // Not an S3 path — treat as prefix only
        Ok((String::new(), path.trim_end_matches('/').to_string()))
    }
}

/// List ALL objects under a prefix (handles pagination).
async fn list_all_objects(
    client: &aws_sdk_s3::Client,
    bucket: &str,
    prefix: &str,
) -> Result<Vec<(String, u64, i64)>, FmError> {
    let mut results = Vec::new();
    let mut continuation_token: Option<String> = None;

    loop {
        let mut req = client
            .list_objects_v2()
            .bucket(bucket)
            .prefix(prefix);

        if let Some(token) = &continuation_token {
            req = req.continuation_token(token);
        }

        let resp = req
            .send()
            .await
            .map_err(|e| FmError::S3(e.to_string()))?;

        for obj in resp.contents() {
            let key = obj.key().unwrap_or_default().to_string();
            let size = obj.size().unwrap_or(0) as u64;
            let modified = obj
                .last_modified()
                .and_then(|t| t.to_millis().ok())
                .unwrap_or(0);
            results.push((key, size, modified));
        }

        if resp.is_truncated() == Some(true) {
            continuation_token = resp.next_continuation_token().map(|s| s.to_string());
        } else {
            break;
        }
    }

    Ok(results)
}
