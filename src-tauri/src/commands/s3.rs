use crate::commands::file::FileOpState;
use crate::models::{
    DirListing, FileEntry, FmError, ProgressEvent, S3AclGrant, S3BucketAcl,
    S3BucketEncryption, S3BucketVersioning, S3CorsRule, S3EncryptionRule, S3LifecycleRule,
    S3LifecycleTransition, S3MultipartUpload, S3ObjectMetadata, S3ObjectProperties,
    S3ObjectVersion, S3PublicAccessBlock, S3Tag, SearchDone, SearchEvent, SearchResult,
};
use aws_config::BehaviorVersion;
use aws_credential_types::provider::ProvideCredentials;
use aws_sdk_s3::Client as S3Client;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tauri::ipc::Channel;
use tauri::State;
use tokio::sync::Semaphore;

// ── State ────────────────────────────────────────────────────────────────────

pub struct S3State(pub Mutex<HashMap<String, S3Connection>>);

pub(crate) struct S3Connection {
    pub(crate) client: S3Client,
    pub(crate) bucket: String,
    #[allow(dead_code)]
    pub(crate) region: String,
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn s3err(msg: impl Into<String>) -> FmError {
    FmError::S3(msg.into())
}

/// Extract the key portion from an s3://bucket/key path.
fn strip_s3_prefix(path: &str, bucket: &str) -> String {
    let prefix = format!("s3://{}/", bucket);
    if let Some(rest) = path.strip_prefix(&prefix) {
        rest.to_string()
    } else {
        path.to_string()
    }
}

/// Build an s3://bucket/key path.
fn s3_path(bucket: &str, key: &str) -> String {
    format!("s3://{}/{}", bucket, key)
}

// ── Multipart upload constants ──────────────────────────────────────────────

const MULTIPART_THRESHOLD: u64 = 8 * 1024 * 1024; // 8 MiB
const PART_SIZE: u64 = 8 * 1024 * 1024; // 8 MiB
const MAX_CONCURRENT_PARTS: usize = 4;
const PART_RETRIES: u32 = 2;

/// List ALL objects under a prefix (handles pagination), returns (key, size, modified_epoch_ms).
async fn list_all_objects(
    client: &S3Client,
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

        let resp = req.send().await.map_err(|e| s3err(e.to_string()))?;

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

// ── Multipart upload helpers ─────────────────────────────────────────────────

/// Upload a single part with retries and linear backoff.
/// Reads `length` bytes from `file_path` at `offset` on each attempt (bounded memory).
async fn upload_part_with_retry(
    client: &S3Client,
    bucket: &str,
    key: &str,
    upload_id: &str,
    part_number: i32,
    file_path: &std::path::Path,
    offset: u64,
    length: u64,
    cancel_flag: &AtomicBool,
) -> Result<(i32, String), FmError> {
    use tokio::io::{AsyncReadExt, AsyncSeekExt};

    for attempt in 0..=PART_RETRIES {
        if cancel_flag.load(Ordering::Relaxed) {
            return Err(FmError::Other("cancelled".into()));
        }

        // Read chunk from disk (re-read on each retry to avoid holding data during backoff)
        let mut file = tokio::fs::File::open(file_path)
            .await
            .map_err(|e| FmError::Io(e))?;
        file.seek(std::io::SeekFrom::Start(offset)).await.map_err(FmError::Io)?;
        let mut buf = vec![0u8; length as usize];
        file.read_exact(&mut buf).await.map_err(FmError::Io)?;

        let result = client
            .upload_part()
            .bucket(bucket)
            .key(key)
            .upload_id(upload_id)
            .part_number(part_number)
            .body(buf.into())
            .send()
            .await;

        match result {
            Ok(resp) => {
                let etag = resp
                    .e_tag()
                    .ok_or_else(|| s3err("Missing ETag in upload_part response"))?
                    .to_string();
                return Ok((part_number, etag));
            }
            Err(e) => {
                if attempt < PART_RETRIES {
                    let backoff = std::time::Duration::from_millis(500 * (attempt as u64 + 1));
                    tokio::time::sleep(backoff).await;
                } else {
                    return Err(s3err(format!(
                        "Part {} failed after {} retries: {}",
                        part_number,
                        PART_RETRIES + 1,
                        e,
                    )));
                }
            }
        }
    }
    unreachable!()
}

/// Orchestrate a full multipart upload for a single large file.
async fn upload_file_multipart(
    client: &S3Client,
    bucket: &str,
    key: &str,
    file_path: &std::path::Path,
    file_size: u64,
    cancel_flag: &Arc<AtomicBool>,
    bytes_done: &Arc<AtomicU64>,
    op_id: &str,
    bytes_total: u64,
    files_done: u32,
    files_total: u32,
    current_file: &str,
    channel: &Channel<ProgressEvent>,
) -> Result<(), FmError> {
    // 1. Create multipart upload
    let create_resp = client
        .create_multipart_upload()
        .bucket(bucket)
        .key(key)
        .send()
        .await
        .map_err(|e| s3err(e.to_string()))?;

    let upload_id = create_resp
        .upload_id()
        .ok_or_else(|| s3err("Missing upload_id from create_multipart_upload"))?
        .to_string();

    // 2. Calculate parts with dynamic part size (handle files > 80 GiB within 10k part limit)
    let part_size = std::cmp::max(PART_SIZE, file_size / 10_000 + 1);
    let num_parts = ((file_size + part_size - 1) / part_size) as i32;

    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_PARTS));
    let mut handles = Vec::with_capacity(num_parts as usize);

    // 3. Spawn tasks for each part
    for i in 0..num_parts {
        let offset = i as u64 * part_size;
        let length = std::cmp::min(part_size, file_size - offset);
        let part_number = i + 1; // S3 part numbers are 1-based

        let client = client.clone();
        let bucket = bucket.to_string();
        let key = key.to_string();
        let upload_id = upload_id.clone();
        let file_path = file_path.to_path_buf();
        let cancel_flag = cancel_flag.clone();
        let sem = semaphore.clone();
        let bytes_done = bytes_done.clone();
        let channel = channel.clone();
        let op_id = op_id.to_string();
        let current_file = current_file.to_string();

        let handle = tokio::spawn(async move {
            let _permit = sem
                .acquire()
                .await
                .map_err(|_| FmError::Other("semaphore closed".into()))?;

            let result = upload_part_with_retry(
                &client,
                &bucket,
                &key,
                &upload_id,
                part_number,
                &file_path,
                offset,
                length,
                &cancel_flag,
            )
            .await?;

            // Update progress atomically
            let new_bytes = bytes_done.fetch_add(length, Ordering::Relaxed) + length;
            let _ = channel.send(ProgressEvent {
                id: op_id,
                bytes_done: new_bytes,
                bytes_total,
                current_file,
                files_done,
                files_total,
            });

            Ok::<(i32, String), FmError>(result)
        });

        handles.push(handle);
    }

    // 4. Join all handles, collect results
    let mut completed_parts: Vec<(i32, String)> = Vec::with_capacity(num_parts as usize);
    let mut first_error: Option<FmError> = None;

    for handle in handles {
        match handle.await {
            Ok(Ok(part)) => completed_parts.push(part),
            Ok(Err(e)) => {
                if first_error.is_none() {
                    first_error = Some(e);
                }
            }
            Err(e) => {
                if first_error.is_none() {
                    first_error = Some(FmError::Other(format!("Task join error: {}", e)));
                }
            }
        }
    }

    // 5. On any failure → abort multipart upload (best-effort) → return error
    if let Some(err) = first_error {
        let _ = client
            .abort_multipart_upload()
            .bucket(bucket)
            .key(key)
            .upload_id(&upload_id)
            .send()
            .await;
        return Err(err);
    }

    // 6. Sort parts by number → complete multipart upload
    completed_parts.sort_by_key(|(num, _)| *num);

    let parts: Vec<_> = completed_parts
        .iter()
        .map(|(num, etag)| {
            aws_sdk_s3::types::CompletedPart::builder()
                .part_number(*num)
                .e_tag(etag)
                .build()
        })
        .collect();

    let completed_upload = aws_sdk_s3::types::CompletedMultipartUpload::builder()
        .set_parts(Some(parts))
        .build();

    client
        .complete_multipart_upload()
        .bucket(bucket)
        .key(key)
        .upload_id(&upload_id)
        .multipart_upload(completed_upload)
        .send()
        .await
        .map_err(|e| s3err(e.to_string()))?;

    Ok(())
}

// ── Multipart copy helpers ───────────────────────────────────────────────────

const COPY_MULTIPART_THRESHOLD: u64 = 5 * 1024 * 1024 * 1024; // 5 GiB

/// Server-side multipart copy for objects larger than 5 GiB.
async fn copy_object_multipart(
    src_bucket: &str,
    src_key: &str,
    dest_client: &S3Client,
    dest_bucket: &str,
    dest_key: &str,
    object_size: u64,
) -> Result<(), FmError> {
    // 1. Create multipart upload on destination
    let create_resp = dest_client
        .create_multipart_upload()
        .bucket(dest_bucket)
        .key(dest_key)
        .send()
        .await
        .map_err(|e| s3err(e.to_string()))?;

    let upload_id = create_resp
        .upload_id()
        .ok_or_else(|| s3err("Missing upload_id from create_multipart_upload"))?
        .to_string();

    // 2. Calculate part size (dynamic sizing to stay within 10k part limit)
    let part_size = std::cmp::max(PART_SIZE, object_size / 10_000 + 1);
    let num_parts = ((object_size + part_size - 1) / part_size) as i32;
    let copy_source = format!("{}/{}", src_bucket, src_key);

    let mut completed_parts: Vec<(i32, String)> = Vec::with_capacity(num_parts as usize);

    // 3. Upload parts sequentially (server-side copy, no data through client)
    for i in 0..num_parts {
        let offset = i as u64 * part_size;
        let end = std::cmp::min(offset + part_size, object_size) - 1;
        let part_number = i + 1;

        let result = dest_client
            .upload_part_copy()
            .bucket(dest_bucket)
            .key(dest_key)
            .upload_id(&upload_id)
            .part_number(part_number)
            .copy_source(&copy_source)
            .copy_source_range(format!("bytes={}-{}", offset, end))
            .send()
            .await;

        match result {
            Ok(resp) => {
                let etag = resp
                    .copy_part_result()
                    .and_then(|r| r.e_tag())
                    .ok_or_else(|| s3err("Missing ETag in upload_part_copy response"))?
                    .to_string();
                completed_parts.push((part_number, etag));
            }
            Err(e) => {
                // Abort on failure (best-effort)
                let _ = dest_client
                    .abort_multipart_upload()
                    .bucket(dest_bucket)
                    .key(dest_key)
                    .upload_id(&upload_id)
                    .send()
                    .await;
                return Err(s3err(e.to_string()));
            }
        }
    }

    // 4. Complete multipart upload
    completed_parts.sort_by_key(|(num, _)| *num);

    let parts: Vec<_> = completed_parts
        .iter()
        .map(|(num, etag)| {
            aws_sdk_s3::types::CompletedPart::builder()
                .part_number(*num)
                .e_tag(etag)
                .build()
        })
        .collect();

    let completed_upload = aws_sdk_s3::types::CompletedMultipartUpload::builder()
        .set_parts(Some(parts))
        .build();

    dest_client
        .complete_multipart_upload()
        .bucket(dest_bucket)
        .key(dest_key)
        .upload_id(&upload_id)
        .multipart_upload(completed_upload)
        .send()
        .await
        .map_err(|e| s3err(e.to_string()))?;

    Ok(())
}

/// Copy a single object, using multipart copy for objects >= 5 GiB.
async fn copy_single_or_multipart(
    src_bucket: &str,
    src_key: &str,
    dest_client: &S3Client,
    dest_bucket: &str,
    dest_key: &str,
    object_size: u64,
) -> Result<(), FmError> {
    if object_size < COPY_MULTIPART_THRESHOLD {
        let copy_source = format!("{}/{}", src_bucket, src_key);
        dest_client
            .copy_object()
            .bucket(dest_bucket)
            .key(dest_key)
            .copy_source(&copy_source)
            .send()
            .await
            .map_err(|e| s3err(e.to_string()))?;
    } else {
        copy_object_multipart(src_bucket, src_key, dest_client, dest_bucket, dest_key, object_size)
            .await?;
    }
    Ok(())
}

// ── S3Bucket model ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize)]
pub struct S3Bucket {
    pub name: String,
    pub created: i64, // epoch ms, 0 if unknown
}

// ── Commands ─────────────────────────────────────────────────────────────────

/// Build an S3 client from credentials without storing it in state.
async fn build_s3_client(
    region: &str,
    endpoint: Option<&str>,
    profile: Option<&str>,
    access_key: Option<&str>,
    secret_key: Option<&str>,
) -> Result<S3Client, FmError> {
    let mut loader = if let (Some(ak), Some(sk)) = (access_key, secret_key) {
        let creds = aws_credential_types::Credentials::new(
            ak.to_string(),
            sk.to_string(),
            None,
            None,
            "furman-manual",
        );
        aws_config::defaults(BehaviorVersion::latest())
            .region(aws_config::Region::new(region.to_string()))
            .credentials_provider(creds)
    } else if let Some(prof) = profile {
        aws_config::defaults(BehaviorVersion::latest())
            .region(aws_config::Region::new(region.to_string()))
            .profile_name(prof)
    } else {
        aws_config::defaults(BehaviorVersion::latest())
            .region(aws_config::Region::new(region.to_string()))
    };

    if let Some(ep) = endpoint {
        if !ep.is_empty() {
            loader = loader.endpoint_url(ep);
        }
    }

    let config = loader.load().await;

    let mut s3_config_builder = aws_sdk_s3::config::Builder::from(&config);
    if endpoint.is_some_and(|ep| !ep.is_empty()) {
        s3_config_builder = s3_config_builder.force_path_style(true);
    }
    Ok(S3Client::from_conf(s3_config_builder.build()))
}

/// Check whether the default AWS credential chain has valid credentials.
#[tauri::command]
pub async fn s3_check_credentials() -> Result<bool, FmError> {
    let config = aws_config::defaults(BehaviorVersion::latest()).load().await;
    let provider = config.credentials_provider();
    match provider {
        Some(p) => {
            let result = p.provide_credentials().await;
            Ok(result.is_ok())
        }
        None => Ok(false),
    }
}

/// List all accessible buckets using temporary credentials (no state stored).
#[tauri::command]
pub async fn s3_list_buckets(
    region: String,
    endpoint: Option<String>,
    profile: Option<String>,
    access_key: Option<String>,
    secret_key: Option<String>,
) -> Result<Vec<S3Bucket>, FmError> {
    let client = build_s3_client(
        &region,
        endpoint.as_deref(),
        profile.as_deref(),
        access_key.as_deref(),
        secret_key.as_deref(),
    )
    .await?;

    let resp = client
        .list_buckets()
        .send()
        .await
        .map_err(|e| s3err(format!("Could not list buckets: {}", e)))?;

    let buckets = resp
        .buckets()
        .iter()
        .filter_map(|b| {
            let name = b.name()?.to_string();
            let created = b
                .creation_date()
                .and_then(|d| d.to_millis().ok())
                .unwrap_or(0);
            Some(S3Bucket { name, created })
        })
        .collect();

    Ok(buckets)
}

/// Connect to an S3 bucket and validate with head_bucket.
#[tauri::command]
pub async fn s3_connect(
    state: State<'_, S3State>,
    id: String,
    bucket: String,
    region: String,
    endpoint: Option<String>,
    profile: Option<String>,
    access_key: Option<String>,
    secret_key: Option<String>,
) -> Result<(), FmError> {
    let client = build_s3_client(
        &region,
        endpoint.as_deref(),
        profile.as_deref(),
        access_key.as_deref(),
        secret_key.as_deref(),
    )
    .await?;

    // Validate bucket access
    client
        .head_bucket()
        .bucket(&bucket)
        .send()
        .await
        .map_err(|e| s3err(format!("Cannot access bucket '{}': {}", bucket, e)))?;

    let conn = S3Connection {
        client,
        bucket,
        region,
    };

    let mut map = state.0.lock().map_err(|e| s3err(e.to_string()))?;
    map.insert(id, conn);
    Ok(())
}

/// Disconnect from an S3 bucket.
#[tauri::command]
pub async fn s3_disconnect(state: State<'_, S3State>, id: String) -> Result<(), FmError> {
    let mut map = state.0.lock().map_err(|e| s3err(e.to_string()))?;
    map.remove(&id);
    Ok(())
}

/// List objects in an S3 bucket at the given prefix, returning a DirListing.
#[tauri::command]
pub async fn s3_list_objects(
    state: State<'_, S3State>,
    id: String,
    prefix: String,
) -> Result<DirListing, FmError> {
    let (client, bucket) = {
        let map = state.0.lock().map_err(|e| s3err(e.to_string()))?;
        let conn = map.get(&id).ok_or_else(|| s3err("S3 connection not found"))?;
        (conn.client.clone(), conn.bucket.clone())
    };

    let mut entries: Vec<FileEntry> = Vec::new();

    // Add ".." entry to navigate up
    let parent_prefix = if prefix.is_empty() {
        String::new()
    } else {
        let trimmed = prefix.trim_end_matches('/');
        match trimmed.rfind('/') {
            Some(pos) => format!("{}/", &trimmed[..pos]),
            None => String::new(),
        }
    };

    entries.push(FileEntry {
        name: "..".to_string(),
        path: s3_path(&bucket, &parent_prefix),
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
        storage_class: None,
    });

    // Paginated listing with delimiter
    let mut continuation_token: Option<String> = None;
    let mut total_size: u64 = 0;

    loop {
        let mut req = client
            .list_objects_v2()
            .bucket(&bucket)
            .delimiter("/")
            .prefix(&prefix);

        if let Some(token) = &continuation_token {
            req = req.continuation_token(token);
        }

        let resp = req.send().await.map_err(|e| s3err(e.to_string()))?;

        // Common prefixes → directories
        for cp in resp.common_prefixes() {
            let pfx: &str = match cp.prefix() {
                Some(p) => p,
                None => continue,
            };
            let name = pfx
                .strip_prefix(&prefix)
                .unwrap_or(pfx)
                .trim_end_matches('/')
                .to_string();
            if name.is_empty() {
                continue;
            }
            entries.push(FileEntry {
                name,
                path: s3_path(&bucket, pfx),
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
                storage_class: None,
            });
        }

        // Objects → files
        for obj in resp.contents() {
            let key: &str = match obj.key() {
                Some(k) => k,
                None => continue,
            };
            // Skip the prefix itself (some S3 implementations return it)
            if key == prefix {
                continue;
            }
            let name = key
                .strip_prefix(&prefix)
                .unwrap_or(key)
                .to_string();
            if name.is_empty() || name.ends_with('/') {
                continue;
            }
            let size = obj.size().unwrap_or(0) as u64;
            total_size += size;
            let modified = obj
                .last_modified()
                .and_then(|t| t.to_millis().ok())
                .unwrap_or(0);
            let extension = if name.contains('.') {
                name.rsplit('.').next().map(|s| s.to_string())
            } else {
                None
            };

            entries.push(FileEntry {
                name,
                path: s3_path(&bucket, key),
                size,
                is_dir: false,
                is_symlink: false,
                symlink_target: None,
                modified,
                permissions: 0,
                owner: String::new(),
                group: String::new(),
                extension,
                git_status: None,
                storage_class: obj.storage_class().map(|s| s.as_str().to_string()),
            });
        }

        if resp.is_truncated() == Some(true) {
            continuation_token = resp.next_continuation_token().map(|s| s.to_string());
        } else {
            break;
        }
    }

    Ok(DirListing {
        path: s3_path(&bucket, &prefix),
        entries,
        total_size,
        free_space: 0,
    })
}

/// Download S3 objects to a local destination directory with progress.
/// Returns None on success, Some(checkpoint) on pause.
#[tauri::command]
pub async fn s3_download(
    state: State<'_, S3State>,
    file_op_state: State<'_, FileOpState>,
    id: String,
    op_id: String,
    keys: Vec<String>,
    destination: String,
    channel: Channel<ProgressEvent>,
) -> Result<Option<crate::models::TransferCheckpoint>, FmError> {
    let (client, bucket) = {
        let map = state.0.lock().map_err(|e| s3err(e.to_string()))?;
        let conn = map.get(&id).ok_or_else(|| s3err("S3 connection not found"))?;
        (conn.client.clone(), conn.bucket.clone())
    };

    let flags = Arc::new(crate::commands::file::OpFlags {
        cancel: AtomicBool::new(false),
        pause: AtomicBool::new(false),
    });
    {
        let mut map = file_op_state.0.lock().map_err(|e| FmError::Other(e.to_string()))?;
        map.insert(op_id.clone(), flags.clone());
    }

    let dest = std::path::PathBuf::from(&destination);

    // Resolve actual keys: for prefix keys (dirs), list all children
    let mut resolved: Vec<(String, u64)> = Vec::new();
    for raw_key in &keys {
        let key = strip_s3_prefix(raw_key, &bucket);
        if key.ends_with('/') {
            let children = list_all_objects(&client, &bucket, &key).await?;
            for (k, size, _) in children {
                resolved.push((k, size));
            }
        } else {
            let head = client
                .head_object()
                .bucket(&bucket)
                .key(&key)
                .send()
                .await
                .map_err(|e| s3err(e.to_string()))?;
            let size = head.content_length().unwrap_or(0) as u64;
            resolved.push((key, size));
        }
    }

    let files_total = resolved.len() as u32;
    let bytes_total: u64 = resolved.iter().map(|(_, s)| *s).sum();
    let mut bytes_done: u64 = 0;
    let mut files_done: u32 = 0;
    let mut completed_files: Vec<String> = Vec::new();

    let result = async {
        for (key, _size) in &resolved {
            if flags.cancel.load(Ordering::Relaxed) {
                return Err(FmError::Other("Operation cancelled".into()));
            }
            if flags.pause.load(Ordering::Relaxed) {
                return Ok(Some(crate::models::TransferCheckpoint {
                    files_completed: completed_files,
                    bytes_done,
                    bytes_total,
                    files_done,
                    files_total,
                }));
            }

            let filename = key.rsplit('/').next().unwrap_or(key);
            let base_prefix = if keys.len() == 1 && keys[0].ends_with('/') {
                strip_s3_prefix(&keys[0], &bucket)
            } else {
                match key.rfind('/') {
                    Some(pos) => key[..pos + 1].to_string(),
                    None => String::new(),
                }
            };
            let relative = key.strip_prefix(&base_prefix).unwrap_or(key);
            let local_path = dest.join(relative);

            if let Some(parent) = local_path.parent() {
                std::fs::create_dir_all(parent)?;
            }

            let resp = client
                .get_object()
                .bucket(&bucket)
                .key(key)
                .send()
                .await
                .map_err(|e| s3err(e.to_string()))?;

            let body = resp
                .body
                .collect()
                .await
                .map_err(|e| s3err(e.to_string()))?;
            std::fs::write(&local_path, body.into_bytes())?;

            let file_size = std::fs::metadata(&local_path).map(|m| m.len()).unwrap_or(0);
            bytes_done += file_size;
            files_done += 1;
            completed_files.push(key.clone());

            let _ = channel.send(ProgressEvent {
                id: op_id.clone(),
                bytes_done,
                bytes_total,
                current_file: filename.to_string(),
                files_done,
                files_total,
            });
        }
        Ok(None)
    }.await;

    if let Ok(mut map) = file_op_state.0.lock() {
        map.remove(&op_id);
    }

    result
}

/// Upload local files to an S3 prefix with progress.
/// Returns None on success, Some(checkpoint) on pause.
#[tauri::command]
pub async fn s3_upload(
    state: State<'_, S3State>,
    file_op_state: State<'_, FileOpState>,
    id: String,
    op_id: String,
    sources: Vec<String>,
    dest_prefix: String,
    channel: Channel<ProgressEvent>,
) -> Result<Option<crate::models::TransferCheckpoint>, FmError> {
    let (client, bucket) = {
        let map = state.0.lock().map_err(|e| s3err(e.to_string()))?;
        let conn = map.get(&id).ok_or_else(|| s3err("S3 connection not found"))?;
        (conn.client.clone(), conn.bucket.clone())
    };

    let flags = Arc::new(crate::commands::file::OpFlags {
        cancel: AtomicBool::new(false),
        pause: AtomicBool::new(false),
    });
    {
        let mut map = file_op_state.0.lock().map_err(|e| FmError::Other(e.to_string()))?;
        map.insert(op_id.clone(), flags.clone());
    }

    // Collect all files to upload (expand directories)
    let mut file_list: Vec<(std::path::PathBuf, String)> = Vec::new();
    for source in &sources {
        let src_path = std::path::PathBuf::from(source);
        let name = src_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        if src_path.is_dir() {
            collect_local_files(&src_path, &format!("{}{}", dest_prefix, name), &mut file_list)?;
        } else {
            let key = format!("{}{}", dest_prefix, name);
            file_list.push((src_path, key));
        }
    }

    let files_total = file_list.len() as u32;
    let bytes_total: u64 = file_list
        .iter()
        .map(|(p, _)| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0))
        .sum();
    let mut bytes_done: u64 = 0;
    let mut files_done: u32 = 0;
    let mut completed_files: Vec<String> = Vec::new();

    let result = async {
        for (local_path, key) in &file_list {
            if flags.cancel.load(Ordering::Relaxed) {
                return Err(FmError::Other("Operation cancelled".into()));
            }
            if flags.pause.load(Ordering::Relaxed) {
                return Ok(Some(crate::models::TransferCheckpoint {
                    files_completed: completed_files,
                    bytes_done,
                    bytes_total,
                    files_done,
                    files_total,
                }));
            }

            let file_size = std::fs::metadata(local_path)
                .map(|m| m.len())
                .unwrap_or(0);
            let filename = local_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            if file_size > MULTIPART_THRESHOLD {
                // Large file: multipart upload with concurrent parts
                let atomic_bytes_done = Arc::new(AtomicU64::new(bytes_done));
                upload_file_multipart(
                    &client,
                    &bucket,
                    key,
                    local_path,
                    file_size,
                    &Arc::new(AtomicBool::new(false)), // multipart uses own cancel (via flags check above)
                    &atomic_bytes_done,
                    &op_id,
                    bytes_total,
                    files_done,
                    files_total,
                    &filename,
                    &channel,
                )
                .await?;
                bytes_done = atomic_bytes_done.load(Ordering::Relaxed);
            } else {
                // Small file: single put_object
                let data = std::fs::read(local_path)?;
                let size = data.len() as u64;

                client
                    .put_object()
                    .bucket(&bucket)
                    .key(key)
                    .body(data.into())
                    .send()
                    .await
                    .map_err(|e| s3err(e.to_string()))?;

                bytes_done += size;
            }

            files_done += 1;
            completed_files.push(key.clone());

            let _ = channel.send(ProgressEvent {
                id: op_id.clone(),
                bytes_done,
                bytes_total,
                current_file: filename,
                files_done,
                files_total,
            });
        }
        Ok(None)
    }.await;

    if let Ok(mut map) = file_op_state.0.lock() {
        map.remove(&op_id);
    }

    result
}

/// Recursively collect local files for upload.
fn collect_local_files(
    dir: &std::path::Path,
    prefix: &str,
    out: &mut Vec<(std::path::PathBuf, String)>,
) -> Result<(), FmError> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let key = format!("{}/{}", prefix, name);

        if path.is_dir() {
            collect_local_files(&path, &key, out)?;
        } else {
            out.push((path, key));
        }
    }
    Ok(())
}

/// Server-side copy between S3 locations.
/// Returns None on success, Some(checkpoint) on pause.
#[tauri::command]
pub async fn s3_copy_objects(
    state: State<'_, S3State>,
    file_op_state: State<'_, FileOpState>,
    src_id: String,
    op_id: String,
    src_keys: Vec<String>,
    dest_id: String,
    dest_prefix: String,
    channel: Channel<ProgressEvent>,
) -> Result<Option<crate::models::TransferCheckpoint>, FmError> {
    let (src_client, src_bucket, dest_client, dest_bucket) = {
        let map = state.0.lock().map_err(|e| s3err(e.to_string()))?;
        let src_conn = map.get(&src_id).ok_or_else(|| s3err("Source S3 connection not found"))?;
        let dest_conn = map.get(&dest_id).ok_or_else(|| s3err("Dest S3 connection not found"))?;
        (
            src_conn.client.clone(),
            src_conn.bucket.clone(),
            dest_conn.client.clone(),
            dest_conn.bucket.clone(),
        )
    };

    let flags = Arc::new(crate::commands::file::OpFlags {
        cancel: AtomicBool::new(false),
        pause: AtomicBool::new(false),
    });
    {
        let mut map = file_op_state.0.lock().map_err(|e| FmError::Other(e.to_string()))?;
        map.insert(op_id.clone(), flags.clone());
    }

    let mut resolved: Vec<(String, u64)> = Vec::new();
    for raw_key in &src_keys {
        let key = strip_s3_prefix(raw_key, &src_bucket);
        if key.ends_with('/') {
            let children = list_all_objects(&src_client, &src_bucket, &key).await?;
            for (k, size, _) in children {
                resolved.push((k, size));
            }
        } else {
            let head = src_client
                .head_object()
                .bucket(&src_bucket)
                .key(&key)
                .send()
                .await
                .map_err(|e| s3err(e.to_string()))?;
            let size = head.content_length().unwrap_or(0) as u64;
            resolved.push((key, size));
        }
    }

    let files_total = resolved.len() as u32;
    let bytes_total: u64 = resolved.iter().map(|(_, s)| *s).sum();
    let mut bytes_done: u64 = 0;
    let mut files_done: u32 = 0;
    let mut completed_files: Vec<String> = Vec::new();

    let result = async {
        for (key, size) in &resolved {
            if flags.cancel.load(Ordering::Relaxed) {
                return Err(FmError::Other("Operation cancelled".into()));
            }
            if flags.pause.load(Ordering::Relaxed) {
                return Ok(Some(crate::models::TransferCheckpoint {
                    files_completed: completed_files,
                    bytes_done,
                    bytes_total,
                    files_done,
                    files_total,
                }));
            }

            let filename = key.rsplit('/').next().unwrap_or(key);
            let dest_key = format!("{}{}", dest_prefix, filename);

            copy_single_or_multipart(
                &src_bucket, key, &dest_client, &dest_bucket, &dest_key, *size,
            )
            .await?;

            bytes_done += size;
            files_done += 1;
            completed_files.push(key.clone());

            let _ = channel.send(ProgressEvent {
                id: op_id.clone(),
                bytes_done,
                bytes_total,
                current_file: filename.to_string(),
                files_done,
                files_total,
            });
        }
        Ok(None)
    }.await;

    if let Ok(mut map) = file_op_state.0.lock() {
        map.remove(&op_id);
    }

    result
}

/// Get properties of a single S3 object via head_object.
#[tauri::command]
pub async fn s3_head_object(
    state: State<'_, S3State>,
    id: String,
    key: String,
) -> Result<S3ObjectProperties, FmError> {
    let (client, bucket) = {
        let map = state.0.lock().map_err(|e| s3err(e.to_string()))?;
        let conn = map.get(&id).ok_or_else(|| s3err("S3 connection not found"))?;
        (conn.client.clone(), conn.bucket.clone())
    };

    let actual_key = strip_s3_prefix(&key, &bucket);

    let head = client
        .head_object()
        .bucket(&bucket)
        .key(&actual_key)
        .send()
        .await
        .map_err(|e| s3err(e.to_string()))?;

    let size = head.content_length().unwrap_or(0) as u64;
    let modified = head
        .last_modified()
        .and_then(|t| t.to_millis().ok())
        .unwrap_or(0);
    let content_type = head.content_type().map(|s| s.to_string());
    let etag = head.e_tag().map(|s| s.to_string());
    let storage_class = head.storage_class().map(|s| s.as_str().to_string());
    let restore_status = head.restore().map(|s| s.to_string());
    let version_id = head.version_id().map(|s| s.to_string());

    Ok(S3ObjectProperties {
        key: actual_key,
        size,
        modified,
        content_type,
        etag,
        storage_class,
        restore_status,
        version_id,
    })
}

/// Delete S3 objects. For prefix keys, lists and deletes all children.
#[tauri::command]
pub async fn s3_delete_objects(
    state: State<'_, S3State>,
    id: String,
    keys: Vec<String>,
) -> Result<(), FmError> {
    let (client, bucket) = {
        let map = state.0.lock().map_err(|e| s3err(e.to_string()))?;
        let conn = map.get(&id).ok_or_else(|| s3err("S3 connection not found"))?;
        (conn.client.clone(), conn.bucket.clone())
    };

    let mut to_delete: Vec<String> = Vec::new();
    for raw_key in &keys {
        let key = strip_s3_prefix(raw_key, &bucket);
        if key.ends_with('/') {
            let children = list_all_objects(&client, &bucket, &key).await?;
            for (k, _, _) in children {
                to_delete.push(k);
            }
        } else {
            to_delete.push(key);
        }
    }

    // Batch delete (max 1000 per request)
    for chunk in to_delete.chunks(1000) {
        let objects: Vec<_> = chunk
            .iter()
            .map(|k| {
                aws_sdk_s3::types::ObjectIdentifier::builder()
                    .key(k)
                    .build()
                    .expect("valid object identifier")
            })
            .collect();

        let delete = aws_sdk_s3::types::Delete::builder()
            .set_objects(Some(objects))
            .build()
            .map_err(|e| s3err(e.to_string()))?;

        client
            .delete_objects()
            .bucket(&bucket)
            .delete(delete)
            .send()
            .await
            .map_err(|e| s3err(e.to_string()))?;
    }

    Ok(())
}

/// Create a "folder" in S3 by putting a zero-byte object with a trailing-slash key.
#[tauri::command]
pub async fn s3_create_folder(
    state: State<'_, S3State>,
    id: String,
    key: String,
) -> Result<(), FmError> {
    let (client, bucket) = {
        let map = state.0.lock().map_err(|e| s3err(e.to_string()))?;
        let conn = map.get(&id).ok_or_else(|| s3err("S3 connection not found"))?;
        (conn.client.clone(), conn.bucket.clone())
    };

    // Ensure key ends with /
    let folder_key = if key.ends_with('/') {
        key
    } else {
        format!("{}/", key)
    };

    // Check if anything already exists under this prefix
    let check = client
        .list_objects_v2()
        .bucket(&bucket)
        .prefix(&folder_key)
        .max_keys(1)
        .send()
        .await
        .map_err(|e| s3err(e.to_string()))?;

    if !check.contents().is_empty() || !check.common_prefixes().is_empty() {
        return Err(FmError::AlreadyExists(folder_key));
    }

    // Put zero-byte object
    client
        .put_object()
        .bucket(&bucket)
        .key(&folder_key)
        .body(aws_sdk_s3::primitives::ByteStream::from_static(b""))
        .send()
        .await
        .map_err(|e| s3err(e.to_string()))?;

    Ok(())
}

/// Rename an S3 object or prefix (copy to new key, then delete original).
#[tauri::command]
pub async fn s3_rename_object(
    state: State<'_, S3State>,
    id: String,
    key: String,
    new_name: String,
) -> Result<(), FmError> {
    // Validate new_name
    if new_name.contains('/') || new_name.contains('\0') {
        return Err(s3err("Invalid name: must not contain '/' or null bytes"));
    }
    if new_name.is_empty() {
        return Err(s3err("Name cannot be empty"));
    }

    let (client, bucket) = {
        let map = state.0.lock().map_err(|e| s3err(e.to_string()))?;
        let conn = map.get(&id).ok_or_else(|| s3err("S3 connection not found"))?;
        (conn.client.clone(), conn.bucket.clone())
    };

    let actual_key = strip_s3_prefix(&key, &bucket);

    if actual_key.ends_with('/') {
        // Prefix (folder) rename
        rename_prefix(&client, &bucket, &actual_key, &new_name).await
    } else {
        // Single file rename
        rename_file(&client, &bucket, &actual_key, &new_name).await
    }
}

/// Rename a single S3 object by replacing the last path component.
async fn rename_file(
    client: &S3Client,
    bucket: &str,
    key: &str,
    new_name: &str,
) -> Result<(), FmError> {
    // Compute dest key by replacing last path component
    let dest_key = match key.rfind('/') {
        Some(pos) => format!("{}/{}", &key[..pos], new_name),
        None => new_name.to_string(),
    };

    // Check destination doesn't already exist
    let dest_head = client
        .head_object()
        .bucket(bucket)
        .key(&dest_key)
        .send()
        .await;
    if dest_head.is_ok() {
        return Err(FmError::AlreadyExists(dest_key));
    }

    // Get source object size for multipart copy routing
    let src_head = client
        .head_object()
        .bucket(bucket)
        .key(key)
        .send()
        .await
        .map_err(|e| s3err(e.to_string()))?;
    let object_size = src_head.content_length().unwrap_or(0) as u64;

    // Copy to new key (multipart for large objects)
    copy_single_or_multipart(bucket, key, client, bucket, &dest_key, object_size).await?;

    // Delete original
    client
        .delete_object()
        .bucket(bucket)
        .key(key)
        .send()
        .await
        .map_err(|e| s3err(e.to_string()))?;

    Ok(())
}

/// Rename an S3 prefix (folder) by copying all children to the new prefix, then deleting originals.
async fn rename_prefix(
    client: &S3Client,
    bucket: &str,
    old_prefix: &str,
    new_name: &str,
) -> Result<(), FmError> {
    // old_prefix is like "photos/vacation/" — compute new prefix
    let trimmed = old_prefix.trim_end_matches('/');
    let new_prefix = match trimmed.rfind('/') {
        Some(pos) => format!("{}/{}/", &trimmed[..pos], new_name),
        None => format!("{}/", new_name),
    };

    // Check target prefix is empty
    let check = client
        .list_objects_v2()
        .bucket(bucket)
        .prefix(&new_prefix)
        .max_keys(1)
        .send()
        .await
        .map_err(|e| s3err(e.to_string()))?;

    if !check.contents().is_empty() {
        return Err(FmError::AlreadyExists(new_prefix));
    }

    // List all objects under old prefix
    let children = list_all_objects(client, bucket, old_prefix).await?;
    if children.is_empty() {
        return Ok(());
    }

    // Copy each object to new prefix
    for (child_key, size, _) in &children {
        let relative = child_key
            .strip_prefix(old_prefix)
            .unwrap_or(child_key);
        let dest_key = format!("{}{}", new_prefix, relative);

        copy_single_or_multipart(bucket, child_key, client, bucket, &dest_key, *size).await?;
    }

    // Batch delete originals (max 1000 per request)
    let keys_to_delete: Vec<String> = children.into_iter().map(|(k, _, _)| k).collect();
    for chunk in keys_to_delete.chunks(1000) {
        let objects: Vec<_> = chunk
            .iter()
            .map(|k| {
                aws_sdk_s3::types::ObjectIdentifier::builder()
                    .key(k)
                    .build()
                    .expect("valid object identifier")
            })
            .collect();

        let delete = aws_sdk_s3::types::Delete::builder()
            .set_objects(Some(objects))
            .build()
            .map_err(|e| s3err(e.to_string()))?;

        client
            .delete_objects()
            .bucket(bucket)
            .delete(delete)
            .send()
            .await
            .map_err(|e| s3err(e.to_string()))?;
    }

    Ok(())
}

/// Search S3 objects by name under a prefix, streaming results via channel.
#[tauri::command]
pub async fn s3_search_objects(
    state: State<'_, S3State>,
    search_state: State<'_, crate::commands::search::SearchState>,
    id: String,
    search_id: String,
    prefix: String,
    query: String,
    channel: Channel<SearchEvent>,
) -> Result<(), FmError> {
    let (client, bucket) = {
        let map = state.0.lock().map_err(|e| s3err(e.to_string()))?;
        let conn = map.get(&id).ok_or_else(|| s3err("S3 connection not found"))?;
        (conn.client.clone(), conn.bucket.clone())
    };

    let cancel_flag = Arc::new(AtomicBool::new(false));
    {
        let mut map = search_state.0.lock().map_err(|e| FmError::Other(e.to_string()))?;
        map.insert(search_id.clone(), cancel_flag.clone());
    }

    let query_lower = query.to_lowercase();
    let mut continuation_token: Option<String> = None;
    let mut total_found: u32 = 0;
    let mut streamed: u32 = 0;
    const MAX_STREAMED: u32 = 1000;

    loop {
        if cancel_flag.load(Ordering::Relaxed) {
            let _ = channel.send(SearchEvent::Done(SearchDone {
                total_found,
                cancelled: true,
            }));
            return Ok(());
        }

        let mut req = client
            .list_objects_v2()
            .bucket(&bucket)
            .prefix(&prefix);
        // No delimiter = recursive listing

        if let Some(token) = &continuation_token {
            req = req.continuation_token(token);
        }

        let resp = req.send().await.map_err(|e| s3err(e.to_string()))?;

        for obj in resp.contents() {
            if cancel_flag.load(Ordering::Relaxed) {
                let _ = channel.send(SearchEvent::Done(SearchDone {
                    total_found,
                    cancelled: true,
                }));
                return Ok(());
            }

            let key = match obj.key() {
                Some(k) => k,
                None => continue,
            };

            // Extract filename (last component)
            let filename = key.rsplit('/').next().unwrap_or(key);
            if filename.is_empty() {
                continue;
            }

            // Case-insensitive substring match
            if filename.to_lowercase().contains(&query_lower) {
                total_found += 1;
                if streamed < MAX_STREAMED {
                    let size = obj.size().unwrap_or(0) as u64;
                    let is_dir = key.ends_with('/');
                    let _ = channel.send(SearchEvent::Result(SearchResult {
                        path: s3_path(&bucket, key),
                        name: filename.to_string(),
                        size,
                        is_dir,
                        line_number: None,
                        snippet: None,
                    }));
                    streamed += 1;
                }
            }
        }

        if resp.is_truncated() == Some(true) {
            continuation_token = resp.next_continuation_token().map(|s| s.to_string());
        } else {
            break;
        }
    }

    let _ = channel.send(SearchEvent::Done(SearchDone {
        total_found,
        cancelled: false,
    }));

    Ok(())
}

/// Max size for preview download (50 MB).
const PREVIEW_MAX_SIZE: u64 = 50 * 1024 * 1024;

/// Download a single S3 object to a temp file and return the local path.
/// Used for previewing/editing S3 files in the existing Viewer/Editor.
#[tauri::command]
pub async fn s3_download_temp(
    state: State<'_, S3State>,
    id: String,
    key: String,
) -> Result<String, FmError> {
    let (client, bucket) = {
        let map = state.0.lock().map_err(|e| s3err(e.to_string()))?;
        let conn = map.get(&id).ok_or_else(|| s3err("S3 connection not found"))?;
        (conn.client.clone(), conn.bucket.clone())
    };

    let stripped_key = strip_s3_prefix(&key, &bucket);

    // Check object size via head_object
    let head = client
        .head_object()
        .bucket(&bucket)
        .key(&stripped_key)
        .send()
        .await
        .map_err(|e| s3err(e.to_string()))?;

    let size = head.content_length().unwrap_or(0) as u64;
    if size > PREVIEW_MAX_SIZE {
        return Err(s3err(format!(
            "File is too large for preview ({:.1} MB). Use download instead.",
            size as f64 / (1024.0 * 1024.0)
        )));
    }

    // Build temp path: {temp}/furman-preview/{hash}-{filename}
    let filename = stripped_key.rsplit('/').next().unwrap_or(&stripped_key);
    let hash = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        key.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    };
    let safe_name = format!("{}-{}", &hash[..8], filename);
    let temp_path = std::env::temp_dir().join("furman-preview").join(&safe_name);

    if let Some(parent) = temp_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Download the object
    let resp = client
        .get_object()
        .bucket(&bucket)
        .key(&stripped_key)
        .send()
        .await
        .map_err(|e| s3err(e.to_string()))?;

    let body = resp
        .body
        .collect()
        .await
        .map_err(|e| s3err(e.to_string()))?;
    std::fs::write(&temp_path, body.into_bytes())?;

    Ok(temp_path.to_string_lossy().to_string())
}

/// Put text content directly to an S3 key (used by the editor write-back).
#[tauri::command]
pub async fn s3_put_text(
    state: State<'_, S3State>,
    id: String,
    key: String,
    content: String,
) -> Result<(), FmError> {
    let (client, bucket) = {
        let map = state.0.lock().map_err(|e| s3err(e.to_string()))?;
        let conn = map.get(&id).ok_or_else(|| s3err("S3 connection not found"))?;
        (conn.client.clone(), conn.bucket.clone())
    };

    let stripped = strip_s3_prefix(&key, &bucket);

    client
        .put_object()
        .bucket(&bucket)
        .key(&stripped)
        .body(content.into_bytes().into())
        .send()
        .await
        .map_err(|e| s3err(e.to_string()))?;

    Ok(())
}

/// Change the storage class of an S3 object by copying it to itself.
#[tauri::command]
pub async fn s3_change_storage_class(
    state: State<'_, S3State>,
    id: String,
    key: String,
    target_class: String,
) -> Result<(), FmError> {
    let (client, bucket) = {
        let map = state.0.lock().map_err(|e| s3err(e.to_string()))?;
        let conn = map.get(&id).ok_or_else(|| s3err("S3 connection not found"))?;
        (conn.client.clone(), conn.bucket.clone())
    };

    let actual_key = strip_s3_prefix(&key, &bucket);

    // Check object size — reject >5 GiB (copy_object limit)
    let head = client
        .head_object()
        .bucket(&bucket)
        .key(&actual_key)
        .send()
        .await
        .map_err(|e| s3err(e.to_string()))?;

    let size = head.content_length().unwrap_or(0) as u64;
    if size > COPY_MULTIPART_THRESHOLD {
        return Err(s3err(format!(
            "Object is too large ({:.1} GB) for storage class change via copy. Maximum is 5 GB.",
            size as f64 / (1024.0 * 1024.0 * 1024.0)
        )));
    }

    let copy_source = format!("{}/{}", bucket, actual_key);
    let storage_class = aws_sdk_s3::types::StorageClass::from(target_class.as_str());

    client
        .copy_object()
        .bucket(&bucket)
        .key(&actual_key)
        .copy_source(&copy_source)
        .storage_class(storage_class)
        .metadata_directive(aws_sdk_s3::types::MetadataDirective::Copy)
        .send()
        .await
        .map_err(|e| s3err(e.to_string()))?;

    Ok(())
}

/// Restore an object from Glacier or Deep Archive.
#[tauri::command]
pub async fn s3_restore_object(
    state: State<'_, S3State>,
    id: String,
    key: String,
    days: i32,
    tier: String,
) -> Result<(), FmError> {
    let (client, bucket) = {
        let map = state.0.lock().map_err(|e| s3err(e.to_string()))?;
        let conn = map.get(&id).ok_or_else(|| s3err("S3 connection not found"))?;
        (conn.client.clone(), conn.bucket.clone())
    };

    let actual_key = strip_s3_prefix(&key, &bucket);

    let glacier_tier = aws_sdk_s3::types::Tier::from(tier.as_str());

    let glacier_params = aws_sdk_s3::types::GlacierJobParameters::builder()
        .tier(glacier_tier)
        .build()
        .map_err(|e| s3err(e.to_string()))?;

    let restore_request = aws_sdk_s3::types::RestoreRequest::builder()
        .days(days)
        .glacier_job_parameters(glacier_params)
        .build();

    client
        .restore_object()
        .bucket(&bucket)
        .key(&actual_key)
        .restore_request(restore_request)
        .send()
        .await
        .map_err(|e| s3err(e.to_string()))?;

    Ok(())
}

/// List all versions of an S3 object.
#[tauri::command]
pub async fn s3_list_object_versions(
    state: State<'_, S3State>,
    id: String,
    key: String,
) -> Result<Vec<S3ObjectVersion>, FmError> {
    let (client, bucket) = {
        let map = state.0.lock().map_err(|e| s3err(e.to_string()))?;
        let conn = map.get(&id).ok_or_else(|| s3err("S3 connection not found"))?;
        (conn.client.clone(), conn.bucket.clone())
    };

    let actual_key = strip_s3_prefix(&key, &bucket);
    let mut versions: Vec<S3ObjectVersion> = Vec::new();
    let mut key_marker: Option<String> = None;
    let mut version_id_marker: Option<String> = None;

    loop {
        let mut req = client
            .list_object_versions()
            .bucket(&bucket)
            .prefix(&actual_key);

        if let Some(km) = &key_marker {
            req = req.key_marker(km);
        }
        if let Some(vm) = &version_id_marker {
            req = req.version_id_marker(vm);
        }

        let resp = req.send().await.map_err(|e| s3err(e.to_string()))?;

        for v in resp.versions() {
            let vkey = v.key().unwrap_or_default();
            if vkey != actual_key {
                continue;
            }
            versions.push(S3ObjectVersion {
                version_id: v.version_id().unwrap_or("null").to_string(),
                is_latest: v.is_latest().unwrap_or(false),
                is_delete_marker: false,
                size: v.size().unwrap_or(0) as u64,
                modified: v
                    .last_modified()
                    .and_then(|t| t.to_millis().ok())
                    .unwrap_or(0),
                etag: v.e_tag().map(|s| s.to_string()),
                storage_class: v.storage_class().map(|s| s.as_str().to_string()),
            });
        }

        for dm in resp.delete_markers() {
            let dmkey = dm.key().unwrap_or_default();
            if dmkey != actual_key {
                continue;
            }
            versions.push(S3ObjectVersion {
                version_id: dm.version_id().unwrap_or("null").to_string(),
                is_latest: dm.is_latest().unwrap_or(false),
                is_delete_marker: true,
                size: 0,
                modified: dm
                    .last_modified()
                    .and_then(|t| t.to_millis().ok())
                    .unwrap_or(0),
                etag: None,
                storage_class: None,
            });
        }

        if resp.is_truncated() == Some(true) {
            key_marker = resp.next_key_marker().map(|s| s.to_string());
            version_id_marker = resp.next_version_id_marker().map(|s| s.to_string());
        } else {
            break;
        }
    }

    // Sort by modified descending (newest first)
    versions.sort_by(|a, b| b.modified.cmp(&a.modified));

    Ok(versions)
}

/// Download a specific version of an S3 object to a temp file.
#[tauri::command]
pub async fn s3_download_version(
    state: State<'_, S3State>,
    id: String,
    key: String,
    version_id: String,
) -> Result<String, FmError> {
    let (client, bucket) = {
        let map = state.0.lock().map_err(|e| s3err(e.to_string()))?;
        let conn = map.get(&id).ok_or_else(|| s3err("S3 connection not found"))?;
        (conn.client.clone(), conn.bucket.clone())
    };

    let stripped_key = strip_s3_prefix(&key, &bucket);

    let resp = client
        .get_object()
        .bucket(&bucket)
        .key(&stripped_key)
        .version_id(&version_id)
        .send()
        .await
        .map_err(|e| s3err(e.to_string()))?;

    let filename = stripped_key.rsplit('/').next().unwrap_or(&stripped_key);
    let short_vid = if version_id.len() > 8 { &version_id[..8] } else { &version_id };
    let safe_name = format!("{}-{}", short_vid, filename);
    let temp_path = std::env::temp_dir().join("furman-preview").join(&safe_name);

    if let Some(parent) = temp_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let body = resp
        .body
        .collect()
        .await
        .map_err(|e| s3err(e.to_string()))?;
    std::fs::write(&temp_path, body.into_bytes())?;

    Ok(temp_path.to_string_lossy().to_string())
}

/// Restore a specific version by copying it as the current version.
#[tauri::command]
pub async fn s3_restore_version(
    state: State<'_, S3State>,
    id: String,
    key: String,
    version_id: String,
) -> Result<(), FmError> {
    let (client, bucket) = {
        let map = state.0.lock().map_err(|e| s3err(e.to_string()))?;
        let conn = map.get(&id).ok_or_else(|| s3err("S3 connection not found"))?;
        (conn.client.clone(), conn.bucket.clone())
    };

    let actual_key = strip_s3_prefix(&key, &bucket);

    let copy_source = format!(
        "{}/{}?versionId={}",
        bucket,
        actual_key,
        urlencoding::encode(&version_id)
    );

    client
        .copy_object()
        .bucket(&bucket)
        .key(&actual_key)
        .copy_source(&copy_source)
        .send()
        .await
        .map_err(|e| s3err(e.to_string()))?;

    Ok(())
}

/// Delete a specific version of an S3 object.
#[tauri::command]
pub async fn s3_delete_version(
    state: State<'_, S3State>,
    id: String,
    key: String,
    version_id: String,
) -> Result<(), FmError> {
    let (client, bucket) = {
        let map = state.0.lock().map_err(|e| s3err(e.to_string()))?;
        let conn = map.get(&id).ok_or_else(|| s3err("S3 connection not found"))?;
        (conn.client.clone(), conn.bucket.clone())
    };

    let actual_key = strip_s3_prefix(&key, &bucket);

    client
        .delete_object()
        .bucket(&bucket)
        .key(&actual_key)
        .version_id(&version_id)
        .send()
        .await
        .map_err(|e| s3err(e.to_string()))?;

    Ok(())
}

/// Generate a presigned GET URL for an S3 object.
#[tauri::command]
pub async fn s3_presign_url(
    state: State<'_, S3State>,
    id: String,
    key: String,
    expires_in_secs: u64,
) -> Result<String, FmError> {
    let (client, bucket) = {
        let map = state.0.lock().map_err(|e| s3err(e.to_string()))?;
        let conn = map.get(&id).ok_or_else(|| s3err("S3 connection not found"))?;
        (conn.client.clone(), conn.bucket.clone())
    };

    let actual_key = strip_s3_prefix(&key, &bucket);

    let presign_config = aws_sdk_s3::presigning::PresigningConfig::expires_in(
        std::time::Duration::from_secs(expires_in_secs),
    )
    .map_err(|e| s3err(e.to_string()))?;

    let presigned = client
        .get_object()
        .bucket(&bucket)
        .key(&actual_key)
        .presigned(presign_config)
        .await
        .map_err(|e| s3err(e.to_string()))?;

    Ok(presigned.uri().to_string())
}

// ── Bucket Management ───────────────────────────────────────────────────────

/// Create a new S3 bucket.
#[tauri::command]
pub async fn s3_create_bucket(
    region: String,
    bucket_name: String,
    endpoint: Option<String>,
    profile: Option<String>,
    access_key: Option<String>,
    secret_key: Option<String>,
) -> Result<(), FmError> {
    let client = build_s3_client(
        &region,
        endpoint.as_deref(),
        profile.as_deref(),
        access_key.as_deref(),
        secret_key.as_deref(),
    )
    .await?;

    let mut req = client.create_bucket().bucket(&bucket_name);

    // us-east-1 must NOT set a location constraint (AWS quirk)
    if region != "us-east-1" {
        let constraint = aws_sdk_s3::types::CreateBucketConfiguration::builder()
            .location_constraint(aws_sdk_s3::types::BucketLocationConstraint::from(
                region.as_str(),
            ))
            .build();
        req = req.create_bucket_configuration(constraint);
    }

    req.send()
        .await
        .map_err(|e| s3err(format!("Could not create bucket '{}': {}", bucket_name, e)))?;

    Ok(())
}

/// Delete an S3 bucket (must be empty).
#[tauri::command]
pub async fn s3_delete_bucket(
    region: String,
    bucket_name: String,
    endpoint: Option<String>,
    profile: Option<String>,
    access_key: Option<String>,
    secret_key: Option<String>,
) -> Result<(), FmError> {
    let client = build_s3_client(
        &region,
        endpoint.as_deref(),
        profile.as_deref(),
        access_key.as_deref(),
        secret_key.as_deref(),
    )
    .await?;

    client
        .delete_bucket()
        .bucket(&bucket_name)
        .send()
        .await
        .map_err(|e| s3err(format!("Could not delete bucket '{}': {}", bucket_name, e)))?;

    Ok(())
}

/// Get versioning status for a bucket.
#[tauri::command]
pub async fn s3_get_bucket_versioning(
    state: State<'_, S3State>,
    id: String,
) -> Result<S3BucketVersioning, FmError> {
    let (client, bucket) = {
        let map = state.0.lock().map_err(|e| s3err(e.to_string()))?;
        let conn = map.get(&id).ok_or_else(|| s3err("S3 connection not found"))?;
        (conn.client.clone(), conn.bucket.clone())
    };

    let resp = client
        .get_bucket_versioning()
        .bucket(&bucket)
        .send()
        .await
        .map_err(|e| s3err(e.to_string()))?;

    let status = match resp.status() {
        Some(s) => s.as_str().to_string(),
        None => "Disabled".to_string(),
    };

    let mfa_delete = match resp.mfa_delete() {
        Some(s) => s.as_str().to_string(),
        None => "Disabled".to_string(),
    };

    Ok(S3BucketVersioning { status, mfa_delete })
}

/// Enable or suspend versioning on a bucket.
#[tauri::command]
pub async fn s3_put_bucket_versioning(
    state: State<'_, S3State>,
    id: String,
    enabled: bool,
) -> Result<(), FmError> {
    let (client, bucket) = {
        let map = state.0.lock().map_err(|e| s3err(e.to_string()))?;
        let conn = map.get(&id).ok_or_else(|| s3err("S3 connection not found"))?;
        (conn.client.clone(), conn.bucket.clone())
    };

    let status = if enabled {
        aws_sdk_s3::types::BucketVersioningStatus::Enabled
    } else {
        aws_sdk_s3::types::BucketVersioningStatus::Suspended
    };

    let config = aws_sdk_s3::types::VersioningConfiguration::builder()
        .status(status)
        .build();

    client
        .put_bucket_versioning()
        .bucket(&bucket)
        .versioning_configuration(config)
        .send()
        .await
        .map_err(|e| s3err(e.to_string()))?;

    Ok(())
}

/// Get encryption configuration for a bucket.
#[tauri::command]
pub async fn s3_get_bucket_encryption(
    state: State<'_, S3State>,
    id: String,
) -> Result<S3BucketEncryption, FmError> {
    let (client, bucket) = {
        let map = state.0.lock().map_err(|e| s3err(e.to_string()))?;
        let conn = map.get(&id).ok_or_else(|| s3err("S3 connection not found"))?;
        (conn.client.clone(), conn.bucket.clone())
    };

    let resp = client
        .get_bucket_encryption()
        .bucket(&bucket)
        .send()
        .await;

    match resp {
        Ok(r) => {
            let rules = r
                .server_side_encryption_configuration()
                .map(|config| {
                    config
                        .rules()
                        .iter()
                        .filter_map(|rule| {
                            let default = rule.apply_server_side_encryption_by_default()?;
                            Some(S3EncryptionRule {
                                sse_algorithm: default
                                    .sse_algorithm()
                                    .as_str()
                                    .to_string(),
                                kms_key_id: default
                                    .kms_master_key_id()
                                    .map(|s| s.to_string()),
                                bucket_key_enabled: rule.bucket_key_enabled().unwrap_or(false),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            Ok(S3BucketEncryption { rules })
        }
        Err(e) => {
            let err_str = e.to_string();
            let err_dbg = format!("{e:?}");
            if err_str.contains("ServerSideEncryptionConfigurationNotFound")
                || err_str.contains("NoSuchConfiguration")
                || err_dbg.contains("ServerSideEncryptionConfigurationNotFound")
                || err_dbg.contains("NoSuchConfiguration")
                || err_dbg.contains("StatusCode(404)")
            {
                Ok(S3BucketEncryption { rules: vec![] })
            } else {
                Err(s3err(err_str))
            }
        }
    }
}

// ── Object Metadata ─────────────────────────────────────────────────────────

/// Get metadata for an S3 object.
#[tauri::command]
pub async fn s3_get_object_metadata(
    state: State<'_, S3State>,
    id: String,
    key: String,
) -> Result<S3ObjectMetadata, FmError> {
    let (client, bucket) = {
        let map = state.0.lock().map_err(|e| s3err(e.to_string()))?;
        let conn = map.get(&id).ok_or_else(|| s3err("S3 connection not found"))?;
        (conn.client.clone(), conn.bucket.clone())
    };

    let actual_key = strip_s3_prefix(&key, &bucket);

    let head = client
        .head_object()
        .bucket(&bucket)
        .key(&actual_key)
        .send()
        .await
        .map_err(|e| s3err(e.to_string()))?;

    let custom: HashMap<String, String> = head
        .metadata()
        .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();

    Ok(S3ObjectMetadata {
        content_type: head.content_type().map(|s| s.to_string()),
        content_disposition: head.content_disposition().map(|s| s.to_string()),
        cache_control: head.cache_control().map(|s| s.to_string()),
        content_encoding: head.content_encoding().map(|s| s.to_string()),
        custom,
    })
}

/// Update metadata for an S3 object via self-copy with REPLACE directive.
#[tauri::command]
pub async fn s3_put_object_metadata(
    state: State<'_, S3State>,
    id: String,
    key: String,
    content_type: Option<String>,
    content_disposition: Option<String>,
    cache_control: Option<String>,
    content_encoding: Option<String>,
    custom: HashMap<String, String>,
) -> Result<(), FmError> {
    let (client, bucket) = {
        let map = state.0.lock().map_err(|e| s3err(e.to_string()))?;
        let conn = map.get(&id).ok_or_else(|| s3err("S3 connection not found"))?;
        (conn.client.clone(), conn.bucket.clone())
    };

    let actual_key = strip_s3_prefix(&key, &bucket);

    // Check object size — reject >5 GiB (copy_object limit)
    let head = client
        .head_object()
        .bucket(&bucket)
        .key(&actual_key)
        .send()
        .await
        .map_err(|e| s3err(e.to_string()))?;

    let size = head.content_length().unwrap_or(0) as u64;
    if size > COPY_MULTIPART_THRESHOLD {
        return Err(s3err(format!(
            "Object is too large ({:.1} GB) for metadata update via copy. Maximum is 5 GB.",
            size as f64 / (1024.0 * 1024.0 * 1024.0)
        )));
    }

    let copy_source = format!("{}/{}", bucket, actual_key);

    let mut req = client
        .copy_object()
        .bucket(&bucket)
        .key(&actual_key)
        .copy_source(&copy_source)
        .metadata_directive(aws_sdk_s3::types::MetadataDirective::Replace);

    if let Some(ct) = &content_type {
        req = req.content_type(ct);
    }
    if let Some(cd) = &content_disposition {
        req = req.content_disposition(cd);
    }
    if let Some(cc) = &cache_control {
        req = req.cache_control(cc);
    }
    if let Some(ce) = &content_encoding {
        req = req.content_encoding(ce);
    }

    // Set custom metadata
    for (k, v) in &custom {
        req = req.metadata(k, v);
    }

    req.send()
        .await
        .map_err(|e| s3err(e.to_string()))?;

    Ok(())
}

// ── Tagging ─────────────────────────────────────────────────────────────────

/// Get tags for an S3 object.
#[tauri::command]
pub async fn s3_get_object_tags(
    state: State<'_, S3State>,
    id: String,
    key: String,
) -> Result<Vec<S3Tag>, FmError> {
    let (client, bucket) = {
        let map = state.0.lock().map_err(|e| s3err(e.to_string()))?;
        let conn = map.get(&id).ok_or_else(|| s3err("S3 connection not found"))?;
        (conn.client.clone(), conn.bucket.clone())
    };

    let actual_key = strip_s3_prefix(&key, &bucket);

    let resp = client
        .get_object_tagging()
        .bucket(&bucket)
        .key(&actual_key)
        .send()
        .await;

    match resp {
        Ok(r) => {
            let tags = r
                .tag_set()
                .iter()
                .map(|t| S3Tag {
                    key: t.key().to_string(),
                    value: t.value().to_string(),
                })
                .collect();
            Ok(tags)
        }
        Err(e) => {
            let err_str = e.to_string();
            let err_dbg = format!("{e:?}");
            if err_str.contains("NoSuchTagSet")
                || err_dbg.contains("NoSuchTagSet")
                || err_dbg.contains("StatusCode(404)")
            {
                Ok(vec![])
            } else {
                Err(s3err(err_str))
            }
        }
    }
}

/// Set tags on an S3 object (max 10 tags).
#[tauri::command]
pub async fn s3_put_object_tags(
    state: State<'_, S3State>,
    id: String,
    key: String,
    tags: Vec<S3Tag>,
) -> Result<(), FmError> {
    if tags.len() > 10 {
        return Err(s3err("Maximum 10 tags per object"));
    }

    let (client, bucket) = {
        let map = state.0.lock().map_err(|e| s3err(e.to_string()))?;
        let conn = map.get(&id).ok_or_else(|| s3err("S3 connection not found"))?;
        (conn.client.clone(), conn.bucket.clone())
    };

    let actual_key = strip_s3_prefix(&key, &bucket);

    let tag_set: Vec<_> = tags
        .iter()
        .filter(|t| !t.key.is_empty())
        .map(|t| {
            aws_sdk_s3::types::Tag::builder()
                .key(&t.key)
                .value(&t.value)
                .build()
                .expect("valid tag")
        })
        .collect();

    let tagging = aws_sdk_s3::types::Tagging::builder()
        .set_tag_set(Some(tag_set))
        .build()
        .map_err(|e| s3err(e.to_string()))?;

    client
        .put_object_tagging()
        .bucket(&bucket)
        .key(&actual_key)
        .tagging(tagging)
        .send()
        .await
        .map_err(|e| s3err(e.to_string()))?;

    Ok(())
}

/// Get tags for an S3 bucket.
#[tauri::command]
pub async fn s3_get_bucket_tags(
    state: State<'_, S3State>,
    id: String,
) -> Result<Vec<S3Tag>, FmError> {
    let (client, bucket) = {
        let map = state.0.lock().map_err(|e| s3err(e.to_string()))?;
        let conn = map.get(&id).ok_or_else(|| s3err("S3 connection not found"))?;
        (conn.client.clone(), conn.bucket.clone())
    };

    let resp = client
        .get_bucket_tagging()
        .bucket(&bucket)
        .send()
        .await;

    match resp {
        Ok(r) => {
            let tags = r
                .tag_set()
                .iter()
                .map(|t| S3Tag {
                    key: t.key().to_string(),
                    value: t.value().to_string(),
                })
                .collect();
            Ok(tags)
        }
        Err(e) => {
            let err_str = e.to_string();
            let err_dbg = format!("{e:?}");
            if err_str.contains("NoSuchTagSet")
                || err_dbg.contains("NoSuchTagSet")
                || err_dbg.contains("StatusCode(404)")
            {
                Ok(vec![])
            } else {
                Err(s3err(err_str))
            }
        }
    }
}

/// Set tags on an S3 bucket (max 50 tags).
#[tauri::command]
pub async fn s3_put_bucket_tags(
    state: State<'_, S3State>,
    id: String,
    tags: Vec<S3Tag>,
) -> Result<(), FmError> {
    if tags.len() > 50 {
        return Err(s3err("Maximum 50 tags per bucket"));
    }

    let (client, bucket) = {
        let map = state.0.lock().map_err(|e| s3err(e.to_string()))?;
        let conn = map.get(&id).ok_or_else(|| s3err("S3 connection not found"))?;
        (conn.client.clone(), conn.bucket.clone())
    };

    let tag_set: Vec<_> = tags
        .iter()
        .filter(|t| !t.key.is_empty())
        .map(|t| {
            aws_sdk_s3::types::Tag::builder()
                .key(&t.key)
                .value(&t.value)
                .build()
                .expect("valid tag")
        })
        .collect();

    let tagging = aws_sdk_s3::types::Tagging::builder()
        .set_tag_set(Some(tag_set))
        .build()
        .map_err(|e| s3err(e.to_string()))?;

    client
        .put_bucket_tagging()
        .bucket(&bucket)
        .tagging(tagging)
        .send()
        .await
        .map_err(|e| s3err(e.to_string()))?;

    Ok(())
}

// ── Multipart Upload Cleanup ────────────────────────────────────────────────

/// List incomplete multipart uploads for a bucket.
#[tauri::command]
pub async fn s3_list_multipart_uploads(
    state: State<'_, S3State>,
    id: String,
    prefix: Option<String>,
) -> Result<Vec<S3MultipartUpload>, FmError> {
    let (client, bucket) = {
        let map = state.0.lock().map_err(|e| s3err(e.to_string()))?;
        let conn = map.get(&id).ok_or_else(|| s3err("S3 connection not found"))?;
        (conn.client.clone(), conn.bucket.clone())
    };

    let mut uploads: Vec<S3MultipartUpload> = Vec::new();
    let mut key_marker: Option<String> = None;
    let mut upload_id_marker: Option<String> = None;

    loop {
        let mut req = client.list_multipart_uploads().bucket(&bucket);

        if let Some(pfx) = &prefix {
            req = req.prefix(pfx);
        }
        if let Some(km) = &key_marker {
            req = req.key_marker(km);
        }
        if let Some(um) = &upload_id_marker {
            req = req.upload_id_marker(um);
        }

        let resp = req.send().await.map_err(|e| s3err(e.to_string()))?;

        for upload in resp.uploads() {
            let key = upload.key().unwrap_or_default().to_string();
            let uid = upload.upload_id().unwrap_or_default().to_string();
            let initiated = upload
                .initiated()
                .and_then(|t| t.to_millis().ok())
                .unwrap_or(0);

            uploads.push(S3MultipartUpload {
                key,
                upload_id: uid,
                initiated,
            });
        }

        if resp.is_truncated() == Some(true) {
            key_marker = resp.next_key_marker().map(|s| s.to_string());
            upload_id_marker = resp.next_upload_id_marker().map(|s| s.to_string());
        } else {
            break;
        }
    }

    Ok(uploads)
}

/// Abort a specific multipart upload.
#[tauri::command]
pub async fn s3_abort_multipart_upload(
    state: State<'_, S3State>,
    id: String,
    key: String,
    upload_id: String,
) -> Result<(), FmError> {
    let (client, bucket) = {
        let map = state.0.lock().map_err(|e| s3err(e.to_string()))?;
        let conn = map.get(&id).ok_or_else(|| s3err("S3 connection not found"))?;
        (conn.client.clone(), conn.bucket.clone())
    };

    client
        .abort_multipart_upload()
        .bucket(&bucket)
        .key(&key)
        .upload_id(&upload_id)
        .send()
        .await
        .map_err(|e| s3err(e.to_string()))?;

    Ok(())
}

// ── Lifecycle Rules ─────────────────────────────────────────────────────────

/// Get bucket lifecycle configuration rules.
#[tauri::command]
pub async fn s3_get_bucket_lifecycle(
    state: State<'_, S3State>,
    id: String,
) -> Result<Vec<S3LifecycleRule>, FmError> {
    let (client, bucket) = {
        let map = state.0.lock().map_err(|e| s3err(e.to_string()))?;
        let conn = map.get(&id).ok_or_else(|| s3err("S3 connection not found"))?;
        (conn.client.clone(), conn.bucket.clone())
    };

    let resp = client
        .get_bucket_lifecycle_configuration()
        .bucket(&bucket)
        .send()
        .await;

    match resp {
        Ok(r) => {
            let rules = r
                .rules()
                .iter()
                .map(|rule| {
                    let id_str = rule.id().unwrap_or_default().to_string();

                    // Extract prefix from filter
                    let prefix = rule
                        .filter()
                        .and_then(|f| f.prefix().map(|s| s.to_string()))
                        .unwrap_or_default();

                    let enabled = rule.status()
                        == &aws_sdk_s3::types::ExpirationStatus::Enabled;

                    let transitions: Vec<S3LifecycleTransition> = rule
                        .transitions()
                        .iter()
                        .filter_map(|t| {
                            Some(S3LifecycleTransition {
                                days: t.days().unwrap_or(0),
                                storage_class: t
                                    .storage_class()
                                    .map(|sc| sc.as_str().to_string())
                                    .unwrap_or_default(),
                            })
                        })
                        .collect();

                    let expiration_days = rule
                        .expiration()
                        .and_then(|e| e.days())
                        .map(|d| d);

                    let noncurrent_transitions: Vec<S3LifecycleTransition> = rule
                        .noncurrent_version_transitions()
                        .iter()
                        .filter_map(|t| {
                            Some(S3LifecycleTransition {
                                days: t.noncurrent_days().unwrap_or(0),
                                storage_class: t
                                    .storage_class()
                                    .map(|sc| sc.as_str().to_string())
                                    .unwrap_or_default(),
                            })
                        })
                        .collect();

                    let noncurrent_expiration_days = rule
                        .noncurrent_version_expiration()
                        .and_then(|e| e.noncurrent_days())
                        .map(|d| d);

                    let abort_incomplete_days = rule
                        .abort_incomplete_multipart_upload()
                        .and_then(|a| a.days_after_initiation())
                        .map(|d| d);

                    S3LifecycleRule {
                        id: id_str,
                        prefix,
                        enabled,
                        transitions,
                        expiration_days,
                        noncurrent_transitions,
                        noncurrent_expiration_days,
                        abort_incomplete_days,
                    }
                })
                .collect();
            Ok(rules)
        }
        Err(e) => {
            let err_str = e.to_string();
            let err_dbg = format!("{e:?}");
            // S3-compatible providers may return 404 or different error codes
            if err_str.contains("NoSuchLifecycleConfiguration")
                || err_dbg.contains("NoSuchLifecycleConfiguration")
                || err_dbg.contains("NoSuchConfiguration")
                || err_dbg.contains("StatusCode(404)")
            {
                Ok(vec![])
            } else {
                Err(s3err(err_str))
            }
        }
    }
}

/// Set bucket lifecycle configuration rules.
#[tauri::command]
pub async fn s3_put_bucket_lifecycle(
    state: State<'_, S3State>,
    id: String,
    rules: Vec<S3LifecycleRule>,
) -> Result<(), FmError> {
    let (client, bucket) = {
        let map = state.0.lock().map_err(|e| s3err(e.to_string()))?;
        let conn = map.get(&id).ok_or_else(|| s3err("S3 connection not found"))?;
        (conn.client.clone(), conn.bucket.clone())
    };

    if rules.is_empty() {
        // Remove lifecycle configuration
        client
            .delete_bucket_lifecycle()
            .bucket(&bucket)
            .send()
            .await
            .map_err(|e| s3err(e.to_string()))?;
        return Ok(());
    }

    let sdk_rules: Vec<aws_sdk_s3::types::LifecycleRule> = rules
        .iter()
        .map(|r| {
            let mut builder = aws_sdk_s3::types::LifecycleRule::builder()
                .id(&r.id)
                .filter(
                    aws_sdk_s3::types::LifecycleRuleFilter::builder()
                        .prefix(&r.prefix)
                        .build(),
                )
                .status(if r.enabled {
                    aws_sdk_s3::types::ExpirationStatus::Enabled
                } else {
                    aws_sdk_s3::types::ExpirationStatus::Disabled
                });

            // Transitions
            for t in &r.transitions {
                let sc = t.storage_class.parse::<aws_sdk_s3::types::TransitionStorageClass>()
                    .unwrap_or(aws_sdk_s3::types::TransitionStorageClass::StandardIa);
                builder = builder.transitions(
                    aws_sdk_s3::types::Transition::builder()
                        .days(t.days)
                        .storage_class(sc)
                        .build(),
                );
            }

            // Expiration
            if let Some(days) = r.expiration_days {
                builder = builder.expiration(
                    aws_sdk_s3::types::LifecycleExpiration::builder()
                        .days(days)
                        .build(),
                );
            }

            // Noncurrent version transitions
            for t in &r.noncurrent_transitions {
                let sc = t.storage_class.parse::<aws_sdk_s3::types::TransitionStorageClass>()
                    .unwrap_or(aws_sdk_s3::types::TransitionStorageClass::StandardIa);
                builder = builder.noncurrent_version_transitions(
                    aws_sdk_s3::types::NoncurrentVersionTransition::builder()
                        .noncurrent_days(t.days)
                        .storage_class(sc)
                        .build(),
                );
            }

            // Noncurrent version expiration
            if let Some(days) = r.noncurrent_expiration_days {
                builder = builder.noncurrent_version_expiration(
                    aws_sdk_s3::types::NoncurrentVersionExpiration::builder()
                        .noncurrent_days(days)
                        .build(),
                );
            }

            // Abort incomplete multipart upload
            if let Some(days) = r.abort_incomplete_days {
                builder = builder.abort_incomplete_multipart_upload(
                    aws_sdk_s3::types::AbortIncompleteMultipartUpload::builder()
                        .days_after_initiation(days)
                        .build(),
                );
            }

            builder.build().expect("valid lifecycle rule")
        })
        .collect();

    let config = aws_sdk_s3::types::BucketLifecycleConfiguration::builder()
        .set_rules(Some(sdk_rules))
        .build()
        .map_err(|e| s3err(e.to_string()))?;

    client
        .put_bucket_lifecycle_configuration()
        .bucket(&bucket)
        .lifecycle_configuration(config)
        .send()
        .await
        .map_err(|e| s3err(e.to_string()))?;

    Ok(())
}

// ── CORS Configuration ──────────────────────────────────────────────────────

#[tauri::command]
pub async fn s3_get_bucket_cors(
    state: State<'_, S3State>,
    id: String,
) -> Result<Vec<S3CorsRule>, FmError> {
    let (client, bucket) = {
        let map = state.0.lock().map_err(|e| s3err(e.to_string()))?;
        let conn = map.get(&id).ok_or_else(|| s3err("S3 connection not found"))?;
        (conn.client.clone(), conn.bucket.clone())
    };

    let resp = client.get_bucket_cors().bucket(&bucket).send().await;

    match resp {
        Ok(r) => {
            let rules = r
                .cors_rules()
                .iter()
                .map(|rule| S3CorsRule {
                    allowed_origins: rule.allowed_origins().iter().map(|s| s.to_string()).collect(),
                    allowed_methods: rule.allowed_methods().iter().map(|s| s.to_string()).collect(),
                    allowed_headers: rule.allowed_headers().iter().map(|s| s.to_string()).collect(),
                    expose_headers: rule.expose_headers().iter().map(|s| s.to_string()).collect(),
                    max_age_seconds: rule.max_age_seconds().map(|v| v as i32),
                })
                .collect();
            Ok(rules)
        }
        Err(e) => {
            let err_str = e.to_string();
            let err_dbg = format!("{e:?}");
            if err_str.contains("NoSuchCORSConfiguration")
                || err_dbg.contains("NoSuchCORSConfiguration")
                || err_dbg.contains("StatusCode(404)")
            {
                Ok(vec![])
            } else {
                Err(s3err(err_str))
            }
        }
    }
}

#[tauri::command]
pub async fn s3_put_bucket_cors(
    state: State<'_, S3State>,
    id: String,
    rules: Vec<S3CorsRule>,
) -> Result<(), FmError> {
    let (client, bucket) = {
        let map = state.0.lock().map_err(|e| s3err(e.to_string()))?;
        let conn = map.get(&id).ok_or_else(|| s3err("S3 connection not found"))?;
        (conn.client.clone(), conn.bucket.clone())
    };

    if rules.is_empty() {
        client
            .delete_bucket_cors()
            .bucket(&bucket)
            .send()
            .await
            .map_err(|e| s3err(e.to_string()))?;
        return Ok(());
    }

    let sdk_rules: Vec<aws_sdk_s3::types::CorsRule> = rules
        .iter()
        .map(|r| {
            let mut builder = aws_sdk_s3::types::CorsRule::builder()
                .set_allowed_origins(Some(r.allowed_origins.clone()))
                .set_allowed_methods(Some(r.allowed_methods.clone()))
                .set_allowed_headers(Some(r.allowed_headers.clone()))
                .set_expose_headers(Some(r.expose_headers.clone()));
            if let Some(max_age) = r.max_age_seconds {
                builder = builder.max_age_seconds(max_age);
            }
            builder.build().expect("valid CORS rule")
        })
        .collect();

    let config = aws_sdk_s3::types::CorsConfiguration::builder()
        .set_cors_rules(Some(sdk_rules))
        .build()
        .map_err(|e| s3err(e.to_string()))?;

    client
        .put_bucket_cors()
        .bucket(&bucket)
        .cors_configuration(config)
        .send()
        .await
        .map_err(|e| s3err(e.to_string()))?;

    Ok(())
}

// ── Bulk Storage Class Change ───────────────────────────────────────────────

#[tauri::command]
pub async fn s3_bulk_change_storage_class(
    state: State<'_, S3State>,
    id: String,
    keys: Vec<String>,
    target_class: String,
) -> Result<Vec<String>, FmError> {
    let (client, bucket) = {
        let map = state.0.lock().map_err(|e| s3err(e.to_string()))?;
        let conn = map.get(&id).ok_or_else(|| s3err("S3 connection not found"))?;
        (conn.client.clone(), conn.bucket.clone())
    };

    let storage_class = aws_sdk_s3::types::StorageClass::from(target_class.as_str());
    let mut failed: Vec<String> = Vec::new();

    for key in &keys {
        let actual_key = strip_s3_prefix(key, &bucket);
        let copy_source = format!("{}/{}", bucket, actual_key);

        let result = client
            .copy_object()
            .bucket(&bucket)
            .key(&actual_key)
            .copy_source(&copy_source)
            .storage_class(storage_class.clone())
            .metadata_directive(aws_sdk_s3::types::MetadataDirective::Copy)
            .send()
            .await;

        if result.is_err() {
            failed.push(key.clone());
        }
    }

    Ok(failed)
}

// ── Public Access Block ─────────────────────────────────────────────────────

#[tauri::command]
pub async fn s3_get_public_access_block(
    state: State<'_, S3State>,
    id: String,
) -> Result<S3PublicAccessBlock, FmError> {
    let (client, bucket) = {
        let map = state.0.lock().map_err(|e| s3err(e.to_string()))?;
        let conn = map.get(&id).ok_or_else(|| s3err("S3 connection not found"))?;
        (conn.client.clone(), conn.bucket.clone())
    };

    let resp = client.get_public_access_block().bucket(&bucket).send().await;

    match resp {
        Ok(r) => {
            if let Some(config) = r.public_access_block_configuration() {
                Ok(S3PublicAccessBlock {
                    block_public_acls: config.block_public_acls().unwrap_or(false),
                    ignore_public_acls: config.ignore_public_acls().unwrap_or(false),
                    block_public_policy: config.block_public_policy().unwrap_or(false),
                    restrict_public_buckets: config.restrict_public_buckets().unwrap_or(false),
                })
            } else {
                Ok(S3PublicAccessBlock {
                    block_public_acls: false,
                    ignore_public_acls: false,
                    block_public_policy: false,
                    restrict_public_buckets: false,
                })
            }
        }
        Err(e) => {
            let err_str = e.to_string();
            let err_dbg = format!("{e:?}");
            if err_str.contains("NoSuchPublicAccessBlockConfiguration")
                || err_dbg.contains("NoSuchPublicAccessBlockConfiguration")
                || err_dbg.contains("StatusCode(404)")
            {
                Ok(S3PublicAccessBlock {
                    block_public_acls: false,
                    ignore_public_acls: false,
                    block_public_policy: false,
                    restrict_public_buckets: false,
                })
            } else {
                Err(s3err(err_str))
            }
        }
    }
}

#[tauri::command]
pub async fn s3_put_public_access_block(
    state: State<'_, S3State>,
    id: String,
    config: S3PublicAccessBlock,
) -> Result<(), FmError> {
    let (client, bucket) = {
        let map = state.0.lock().map_err(|e| s3err(e.to_string()))?;
        let conn = map.get(&id).ok_or_else(|| s3err("S3 connection not found"))?;
        (conn.client.clone(), conn.bucket.clone())
    };

    let sdk_config = aws_sdk_s3::types::PublicAccessBlockConfiguration::builder()
        .block_public_acls(config.block_public_acls)
        .ignore_public_acls(config.ignore_public_acls)
        .block_public_policy(config.block_public_policy)
        .restrict_public_buckets(config.restrict_public_buckets)
        .build();

    client
        .put_public_access_block()
        .bucket(&bucket)
        .public_access_block_configuration(sdk_config)
        .send()
        .await
        .map_err(|e| s3err(e.to_string()))?;

    Ok(())
}

// ── Bucket Policy ───────────────────────────────────────────────────────────

#[tauri::command]
pub async fn s3_get_bucket_policy(
    state: State<'_, S3State>,
    id: String,
) -> Result<String, FmError> {
    let (client, bucket) = {
        let map = state.0.lock().map_err(|e| s3err(e.to_string()))?;
        let conn = map.get(&id).ok_or_else(|| s3err("S3 connection not found"))?;
        (conn.client.clone(), conn.bucket.clone())
    };

    let resp = client.get_bucket_policy().bucket(&bucket).send().await;

    match resp {
        Ok(r) => Ok(r.policy().unwrap_or_default().to_string()),
        Err(e) => {
            let err_str = e.to_string();
            let err_dbg = format!("{e:?}");
            if err_str.contains("NoSuchBucketPolicy")
                || err_dbg.contains("NoSuchBucketPolicy")
                || err_dbg.contains("StatusCode(404)")
            {
                Ok(String::new())
            } else {
                Err(s3err(err_str))
            }
        }
    }
}

#[tauri::command]
pub async fn s3_put_bucket_policy(
    state: State<'_, S3State>,
    id: String,
    policy: String,
) -> Result<(), FmError> {
    let (client, bucket) = {
        let map = state.0.lock().map_err(|e| s3err(e.to_string()))?;
        let conn = map.get(&id).ok_or_else(|| s3err("S3 connection not found"))?;
        (conn.client.clone(), conn.bucket.clone())
    };

    if policy.trim().is_empty() {
        client
            .delete_bucket_policy()
            .bucket(&bucket)
            .send()
            .await
            .map_err(|e| s3err(e.to_string()))?;
        return Ok(());
    }

    // Validate JSON
    let _: serde_json::Value =
        serde_json::from_str(&policy).map_err(|e| s3err(format!("Invalid JSON: {}", e)))?;

    client
        .put_bucket_policy()
        .bucket(&bucket)
        .policy(&policy)
        .send()
        .await
        .map_err(|e| s3err(e.to_string()))?;

    Ok(())
}

// ── Bucket ACL (Read-Only) ──────────────────────────────────────────────────

#[tauri::command]
pub async fn s3_get_bucket_acl(
    state: State<'_, S3State>,
    id: String,
) -> Result<S3BucketAcl, FmError> {
    let (client, bucket) = {
        let map = state.0.lock().map_err(|e| s3err(e.to_string()))?;
        let conn = map.get(&id).ok_or_else(|| s3err("S3 connection not found"))?;
        (conn.client.clone(), conn.bucket.clone())
    };

    let resp = client
        .get_bucket_acl()
        .bucket(&bucket)
        .send()
        .await
        .map_err(|e| s3err(e.to_string()))?;

    let owner_id = resp
        .owner()
        .and_then(|o| o.id())
        .unwrap_or_default()
        .to_string();
    let owner_display_name = resp
        .owner()
        .and_then(|o| o.display_name())
        .map(|s| s.to_string());

    let grants = resp
        .grants()
        .iter()
        .map(|g| {
            let (grantee_type, grantee_id, grantee_uri, grantee_email, grantee_display_name) =
                if let Some(grantee) = g.grantee() {
                    let gt = grantee.r#type().as_str().to_string();
                    (
                        gt,
                        grantee.id().map(|s| s.to_string()),
                        grantee.uri().map(|s| s.to_string()),
                        grantee.email_address().map(|s| s.to_string()),
                        grantee.display_name().map(|s| s.to_string()),
                    )
                } else {
                    (String::new(), None, None, None, None)
                };

            let permission = g
                .permission()
                .map(|p| p.as_str().to_string())
                .unwrap_or_default();

            S3AclGrant {
                grantee_type,
                grantee_id,
                grantee_uri,
                grantee_email,
                grantee_display_name,
                permission,
            }
        })
        .collect();

    Ok(S3BucketAcl {
        owner_id,
        owner_display_name,
        grants,
    })
}
