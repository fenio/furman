use crate::models::{FmError, SyncEntry, SyncEvent};
use crate::s3::verification::{check_cancelled, object_sha256};
use crate::s3::S3State;
use glob_match::glob_match;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::UNIX_EPOCH;
use tauri::ipc::Channel;
use tauri::State;

// ── Managed state ───────────────────────────────────────────────────────

pub struct SyncState(pub Mutex<HashMap<String, Arc<AtomicBool>>>);

/// File info: (size, modified_ms, optional SHA-256 of actual contents).
type FileInfo = (u64, i64, Option<String>);

// ── Commands ────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn sync_diff(
    id: String,
    source_backend: String, // "local" | "s3"
    source_path: String,    // local dir path or s3://bucket/prefix
    source_s3_id: String,   // "" for local
    dest_backend: String,
    dest_path: String,
    dest_s3_id: String,
    exclude_patterns: Vec<String>,
    compare_mode: String, // "size_mtime" | "sha256" (legacy "checksum" also uses SHA-256)
    channel: Channel<SyncEvent>,
    s3_state: State<'_, S3State>,
    sync_state: State<'_, SyncState>,
) -> Result<(), FmError> {
    if !matches!(
        compare_mode.as_str(),
        "size_mtime" | "size-and-date" | "checksum" | "sha256"
    ) {
        return Err(FmError::Other(format!(
            "Unknown comparison mode: {compare_mode}"
        )));
    }
    let cancel_flag = Arc::new(AtomicBool::new(false));
    {
        let mut map = sync_state
            .0
            .lock()
            .map_err(|e| FmError::Other(e.to_string()))?;
        map.insert(id.clone(), cancel_flag.clone());
    }

    let _cleanup = SyncCleanup {
        state: &sync_state,
        id: id.clone(),
    };
    let use_checksum = compare_mode == "checksum" || compare_mode == "sha256";

    // Collect source files
    let mut source_files = collect_files(
        &source_backend,
        &source_path,
        &source_s3_id,
        use_checksum,
        &s3_state,
        &cancel_flag,
        &exclude_patterns,
    )
    .await?;

    check_cancelled(&cancel_flag)?;

    // Collect dest files
    let mut dest_files = collect_files(
        &dest_backend,
        &dest_path,
        &dest_s3_id,
        use_checksum,
        &s3_state,
        &cancel_flag,
        &exclude_patterns,
    )
    .await?;

    check_cancelled(&cancel_flag)?;

    // Apply exclude filters
    if !exclude_patterns.is_empty() {
        apply_excludes(&mut source_files, &exclude_patterns);
        apply_excludes(&mut dest_files, &exclude_patterns);
    }

    // Compare and stream entries
    let mut scanned: u32 = 0;
    let mut new_count: u32 = 0;
    let mut modified: u32 = 0;
    let mut deleted: u32 = 0;
    let mut same_count: u32 = 0;

    // Keys in source
    for (rel_path, (src_size, src_modified, ref src_etag)) in &source_files {
        check_cancelled(&cancel_flag)?;

        let entry = if let Some((dst_size, dst_modified, ref dst_etag)) = dest_files.get(rel_path) {
            let is_modified = if use_checksum {
                files_differ_checksum(
                    *src_size,
                    src_etag.as_deref(),
                    *dst_size,
                    dst_etag.as_deref(),
                )
            } else {
                files_differ_metadata(*src_size, *src_modified, *dst_size, *dst_modified)
            };

            if is_modified {
                modified += 1;
                SyncEntry {
                    relative_path: rel_path.clone(),
                    status: "modified".to_string(),
                    source_size: *src_size,
                    dest_size: *dst_size,
                    source_modified: *src_modified,
                    dest_modified: *dst_modified,
                    source_etag: src_etag.clone().unwrap_or_default(),
                    dest_etag: dst_etag.clone().unwrap_or_default(),
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
                    source_etag: src_etag.clone().unwrap_or_default(),
                    dest_etag: dst_etag.clone().unwrap_or_default(),
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
                source_etag: src_etag.clone().unwrap_or_default(),
                dest_etag: String::new(),
            }
        };

        let _ = channel.send(SyncEvent::Entry(entry));
        scanned += 1;

        if scanned % 100 == 0 {
            let _ = channel.send(SyncEvent::Progress { scanned });
        }
    }

    // Keys only in dest (deleted from source perspective)
    for (rel_path, (dst_size, dst_modified, ref dst_etag)) in &dest_files {
        check_cancelled(&cancel_flag)?;

        if !source_files.contains_key(rel_path) {
            deleted += 1;
            let entry = SyncEntry {
                relative_path: rel_path.clone(),
                status: "deleted".to_string(),
                source_size: 0,
                dest_size: *dst_size,
                source_modified: 0,
                dest_modified: *dst_modified,
                source_etag: String::new(),
                dest_etag: dst_etag.clone().unwrap_or_default(),
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

    Ok(())
}

#[tauri::command]
pub fn cancel_sync(id: String, state: State<'_, SyncState>) -> Result<(), FmError> {
    let map = state.0.lock().map_err(|e| FmError::Other(e.to_string()))?;
    if let Some(flag) = map.get(&id) {
        flag.store(true, Ordering::Relaxed);
    }
    Ok(())
}

// ── Helpers ─────────────────────────────────────────────────────────────

/// Cleanup on errors as well as success (listing and hashing may fail).
struct SyncCleanup<'a> {
    state: &'a SyncState,
    id: String,
}

impl Drop for SyncCleanup<'_> {
    fn drop(&mut self) {
        if let Ok(mut map) = self.state.0.lock() {
            map.remove(&self.id);
        }
    }
}

fn files_differ_metadata(
    src_size: u64,
    src_modified: i64,
    dst_size: u64,
    dst_modified: i64,
) -> bool {
    src_size != dst_size || src_modified != dst_modified
}

/// Remove entries whose relative paths match any exclude pattern.
fn apply_excludes(files: &mut HashMap<String, FileInfo>, patterns: &[String]) {
    files.retain(|rel_path, _| !is_excluded(rel_path, patterns));
}

fn is_excluded(rel_path: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|pat| {
        let pat = pat.trim();
        if pat.is_empty() {
            return false;
        }
        // Match against the full relative path
        if glob_match(pat, rel_path) {
            return true;
        }
        // Also match against just the filename component
        if let Some(filename) = rel_path.rsplit('/').next() {
            if glob_match(pat, filename) {
                return true;
            }
        }
        false
    })
}

/// Missing hashes must never count as content equality. Hashing errors are
/// propagated by collectors, not downgraded to size-only verification.
fn files_differ_checksum(
    src_size: u64,
    src_etag: Option<&str>,
    dst_size: u64,
    dst_etag: Option<&str>,
) -> bool {
    src_size != dst_size
        || match (src_etag, dst_etag) {
            (Some(source), Some(dest)) if !source.is_empty() && !dest.is_empty() => source != dest,
            _ => true,
        }
}

/// Collect files from a backend into a HashMap of relative_path → file info.
async fn collect_files(
    backend: &str,
    path: &str,
    s3_id: &str,
    use_checksum: bool,
    s3_state: &State<'_, S3State>,
    cancel: &Arc<AtomicBool>,
    exclude_patterns: &[String],
) -> Result<HashMap<String, FileInfo>, FmError> {
    match backend {
        "local" => {
            let path = path.to_owned();
            let cancel = cancel.clone();
            let exclude_patterns = exclude_patterns.to_vec();
            tokio::task::spawn_blocking(move || {
                collect_local_files_recursive(
                    Path::new(&path),
                    use_checksum,
                    &cancel,
                    &exclude_patterns,
                )
            })
            .await
            .map_err(|e| FmError::Other(e.to_string()))?
        }
        "s3" => {
            collect_s3_files(
                s3_id,
                path,
                use_checksum,
                s3_state,
                cancel,
                exclude_patterns,
            )
            .await
        }
        _ => Err(FmError::Other(format!("Unknown backend: {backend}"))),
    }
}

/// Recursively walk a local directory, returning size, mtime and optional SHA-256.
fn collect_local_files_recursive(
    root: &Path,
    use_checksum: bool,
    cancel: &AtomicBool,
    exclude_patterns: &[String],
) -> Result<HashMap<String, FileInfo>, FmError> {
    let mut result = HashMap::new();
    let root_str = root.to_string_lossy().to_string();
    walk_local(
        root,
        &root_str,
        use_checksum,
        cancel,
        exclude_patterns,
        &mut result,
    )?;
    Ok(result)
}

fn walk_local(
    dir: &Path,
    root: &str,
    use_checksum: bool,
    cancel: &AtomicBool,
    exclude_patterns: &[String],
    out: &mut HashMap<String, FileInfo>,
) -> Result<(), FmError> {
    check_cancelled(cancel)?;
    let entries = fs::read_dir(dir)?;
    for entry in entries {
        check_cancelled(cancel)?;
        let entry = entry?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;

        // Skip symlink directories to avoid loops
        if metadata.is_symlink() && path.is_dir() {
            continue;
        }

        if metadata.is_dir() {
            walk_local(&path, root, use_checksum, cancel, exclude_patterns, out)?;
        } else {
            let full = path.to_string_lossy().to_string();
            // Build relative path by stripping root prefix
            let rel = if let Some(r) = full.strip_prefix(root) {
                if r.starts_with('/') {
                    r[1..].to_string()
                } else {
                    r.to_string()
                }
            } else {
                full.clone()
            };

            if is_excluded(&rel, exclude_patterns) {
                continue;
            }
            let size = metadata.len();
            let modified = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);

            let checksum = if use_checksum {
                Some(compute_file_sha256(&path, cancel)?)
            } else {
                None
            };

            out.insert(rel, (size, modified, checksum));
        }
    }

    Ok(())
}

/// Hash file contents without buffering the whole file or blocking the UI loop.
fn compute_file_sha256(path: &Path, cancel: &AtomicBool) -> Result<String, FmError> {
    let mut file = fs::File::open(path)?;
    let before = file.metadata()?;
    let mut context = Sha256::new();
    let mut buffer = [0u8; 8192];

    loop {
        check_cancelled(cancel)?;
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        context.update(&buffer[..bytes_read]);
    }
    let after = file.metadata()?;
    if before.len() != after.len() || before.modified()? != after.modified()? {
        return Err(FmError::Other(format!(
            "File changed while verifying: {}",
            path.display()
        )));
    }
    let digest = context.finalize();
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// Collect S3 objects under a directory prefix and optionally hash their contents.
async fn collect_s3_files(
    s3_id: &str,
    path: &str,
    use_checksum: bool,
    s3_state: &State<'_, S3State>,
    cancel: &AtomicBool,
    exclude_patterns: &[String],
) -> Result<HashMap<String, FileInfo>, FmError> {
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

    // Prefixes are directories here: "foo" must not include "foobar".
    let prefix = if prefix.is_empty() {
        prefix
    } else {
        format!("{prefix}/")
    };
    let objects = list_all_objects(&client, actual_bucket, &prefix, cancel).await?;

    let mut result = HashMap::new();
    for (key, size, modified, etag) in objects {
        check_cancelled(cancel)?;
        // Skip "directory" markers
        if key.ends_with('/') {
            continue;
        }
        if is_excluded(key.strip_prefix(&prefix).unwrap_or(&key), exclude_patterns) {
            continue;
        }
        // Strip prefix to get relative path
        let checksum = if use_checksum {
            Some(object_sha256(&client, actual_bucket, &key, size, etag.as_deref(), cancel).await?)
        } else {
            None
        };
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
            result.insert(rel, (size, modified, checksum));
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
/// Returns (key, size, modified_ms, etag).
async fn list_all_objects(
    client: &aws_sdk_s3::Client,
    bucket: &str,
    prefix: &str,
    cancel: &AtomicBool,
) -> Result<Vec<(String, u64, i64, Option<String>)>, FmError> {
    let mut results = Vec::new();
    let mut continuation_token: Option<String> = None;

    loop {
        check_cancelled(cancel)?;
        let mut req = client.list_objects_v2().bucket(bucket).prefix(prefix);

        if let Some(token) = &continuation_token {
            req = req.continuation_token(token);
        }

        let resp = req.send().await.map_err(|e| FmError::S3(e.to_string()))?;

        for obj in resp.contents() {
            let key = obj.key().unwrap_or_default().to_string();
            let size = obj.size().unwrap_or(0) as u64;
            let modified = obj
                .last_modified()
                .and_then(|t| t.to_millis().ok())
                .unwrap_or(0);
            let etag = obj.e_tag().map(std::string::ToString::to_string);
            results.push((key, size, modified, etag));
        }

        if resp.is_truncated() == Some(true) {
            let next = resp.next_continuation_token().ok_or_else(|| {
                FmError::S3("Truncated listing without continuation token".into())
            })?;
            if continuation_token.as_deref() == Some(next) {
                return Err(FmError::S3(
                    "Listing repeated its continuation token".into(),
                ));
            }
            continuation_token = Some(next.to_owned());
        } else {
            break;
        }
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_comparison_is_symmetric() {
        assert!(files_differ_metadata(4, 10, 4, 20));
        assert!(files_differ_metadata(4, 20, 4, 10));
        assert!(files_differ_metadata(4, 10, 5, 10));
        assert!(!files_differ_metadata(4, 10, 4, 10));
    }

    #[test]
    fn hashes_cannot_fall_back_to_size_or_date() {
        assert!(files_differ_checksum(4, Some("aaaa"), 4, Some("bbbb")));
        assert!(files_differ_checksum(4, None, 4, None));
        assert!(files_differ_checksum(4, Some("aaaa"), 4, Some("")));
        assert!(files_differ_checksum(4, Some("aaaa"), 5, Some("aaaa")));
        assert!(!files_differ_checksum(4, Some("aaaa"), 4, Some("aaaa")));
    }

    #[test]
    fn local_sha256_known_vectors_and_errors() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("file");
        let cancel = AtomicBool::new(false);
        fs::write(&path, b"abc").unwrap();
        assert_eq!(
            compute_file_sha256(&path, &cancel).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        fs::write(&path, b"").unwrap();
        assert_eq!(
            compute_file_sha256(&path, &cancel).unwrap(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert!(compute_file_sha256(&temp.path().join("missing"), &cancel).is_err());
        cancel.store(true, Ordering::Relaxed);
        assert!(compute_file_sha256(&path, &cancel).is_err());
    }

    #[test]
    fn failed_and_cancelled_scans_clean_up_state() {
        let state = SyncState(Mutex::new(HashMap::new()));
        state
            .0
            .lock()
            .unwrap()
            .insert("scan".into(), Arc::new(AtomicBool::new(false)));
        {
            let _guard = SyncCleanup {
                state: &state,
                id: "scan".into(),
            };
        }
        assert!(state.0.lock().unwrap().is_empty());
    }

    #[test]
    fn local_collection_hashes_nested_files_and_applies_exclusions() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join("nested")).unwrap();
        fs::write(temp.path().join("nested/file"), b"abc").unwrap();
        fs::write(temp.path().join("skip.tmp"), b"ignored").unwrap();
        let files = collect_local_files_recursive(
            temp.path(),
            true,
            &AtomicBool::new(false),
            &["*.tmp".into()],
        )
        .unwrap();
        assert_eq!(files.len(), 1);
        let (size, _, hash) = &files["nested/file"];
        assert_eq!(*size, 3);
        assert_eq!(
            hash.as_deref(),
            Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
        );
    }
}
