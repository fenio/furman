use aws_sdk_s3::Client as S3Client;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;

use crate::models::FmError;

// ── Bandwidth throttling ───────────────────────────────────────────────────

/// Global bandwidth limit in bytes per second. 0 = unlimited.
pub static BANDWIDTH_LIMIT: AtomicU64 = AtomicU64::new(0);

/// Sleep to enforce the global bandwidth limit after transferring `bytes`.
pub async fn throttle(bytes: u64) {
    let limit = BANDWIDTH_LIMIT.load(Ordering::Relaxed);
    if limit == 0 || bytes == 0 {
        return;
    }
    let secs = bytes as f64 / limit as f64;
    if secs > 0.001 {
        tokio::time::sleep(Duration::from_secs_f64(secs)).await;
    }
}

// ── Error helper ────────────────────────────────────────────────────────────

pub fn s3err(msg: impl Into<String>) -> FmError {
    FmError::S3(msg.into())
}

// ── Path utilities ──────────────────────────────────────────────────────────

/// Extract the key portion from an s3://bucket/key path.
pub fn strip_s3_prefix(path: &str, bucket: &str) -> String {
    let prefix = format!("s3://{}/", bucket);
    if let Some(rest) = path.strip_prefix(&prefix) {
        rest.to_string()
    } else {
        path.to_string()
    }
}

/// Build an s3://bucket/key path.
pub fn s3_path(bucket: &str, key: &str) -> String {
    format!("s3://{}/{}", bucket, key)
}

// ── Multipart upload constants ──────────────────────────────────────────────

pub const MULTIPART_THRESHOLD: u64 = 8 * 1024 * 1024; // 8 MiB
pub const PART_SIZE: u64 = 8 * 1024 * 1024; // 8 MiB
pub const MAX_CONCURRENT_PARTS: usize = 4;
pub const PART_RETRIES: u32 = 2;
pub const COPY_MULTIPART_THRESHOLD: u64 = 5 * 1024 * 1024 * 1024; // 5 GiB

/// Max size for preview download (50 MB).
pub const PREVIEW_MAX_SIZE: u64 = 50 * 1024 * 1024;

// ── Object listing ──────────────────────────────────────────────────────────

/// List ALL objects under a prefix (handles pagination), returns (key, size, modified_epoch_ms).
pub async fn list_all_objects(
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

// ── Multipart upload helpers ────────────────────────────────────────────────

/// Upload a single part with retries and linear backoff.
/// Reads `length` bytes from `file_path` at `offset` on each attempt (bounded memory).
pub async fn upload_part_with_retry(
    client: &S3Client,
    bucket: &str,
    key: &str,
    upload_id: &str,
    part_number: i32,
    file_path: &std::path::Path,
    offset: u64,
    length: u64,
    cancel_flag: &AtomicBool,
) -> Result<(i32, String, Option<String>), FmError> {
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
            .checksum_algorithm(aws_sdk_s3::types::ChecksumAlgorithm::Crc32C)
            .body(buf.into())
            .send()
            .await;

        match result {
            Ok(resp) => {
                let etag = resp
                    .e_tag()
                    .ok_or_else(|| s3err("Missing ETag in upload_part response"))?
                    .to_string();
                let crc32c = resp.checksum_crc32_c().map(|s| s.to_string());
                throttle(length).await;
                return Ok((part_number, etag, crc32c));
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
/// Calls `on_progress` after each part completes.
pub async fn upload_file_multipart(
    client: &S3Client,
    bucket: &str,
    key: &str,
    file_path: &std::path::Path,
    file_size: u64,
    cancel_flag: &Arc<AtomicBool>,
    bytes_done: &Arc<AtomicU64>,
    on_progress: &(dyn Fn(u64) + Send + Sync),
    metadata: Option<&std::collections::HashMap<String, String>>,
) -> Result<(), FmError> {
    // 1. Create multipart upload
    let mut create_req = client
        .create_multipart_upload()
        .bucket(bucket)
        .key(key)
        .checksum_algorithm(aws_sdk_s3::types::ChecksumAlgorithm::Crc32C);
    if let Some(meta) = metadata {
        for (k, v) in meta {
            create_req = create_req.metadata(k, v);
        }
    }
    let create_resp = create_req
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

            Ok::<((i32, String, Option<String>), u64), FmError>((result, new_bytes))
        });

        handles.push(handle);
    }

    // 4. Join all handles, collect results
    let mut completed_parts: Vec<(i32, String, Option<String>)> = Vec::with_capacity(num_parts as usize);
    let mut first_error: Option<FmError> = None;

    for handle in handles {
        match handle.await {
            Ok(Ok((part, new_bytes))) => {
                completed_parts.push(part);
                on_progress(new_bytes);
            }
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
    completed_parts.sort_by_key(|(num, _, _)| *num);

    let parts: Vec<_> = completed_parts
        .iter()
        .map(|(num, etag, crc)| {
            let mut b = aws_sdk_s3::types::CompletedPart::builder()
                .part_number(*num)
                .e_tag(etag);
            if let Some(c) = crc {
                b = b.checksum_crc32_c(c);
            }
            b.build()
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

// ── Multipart copy helpers ──────────────────────────────────────────────────

/// Server-side multipart copy for objects larger than 5 GiB.
pub async fn copy_object_multipart(
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
/// Tries server-side copy first; falls back to download-then-upload when
/// server-side copy fails (e.g. cross-provider copies).
pub async fn copy_single_or_multipart(
    src_client: &S3Client,
    src_bucket: &str,
    src_key: &str,
    dest_client: &S3Client,
    dest_bucket: &str,
    dest_key: &str,
    object_size: u64,
) -> Result<(), FmError> {
    if object_size < COPY_MULTIPART_THRESHOLD {
        let copy_source = format!("{}/{}", src_bucket, src_key);
        let result = dest_client
            .copy_object()
            .bucket(dest_bucket)
            .key(dest_key)
            .copy_source(&copy_source)
            .send()
            .await;
        match result {
            Ok(_) => return Ok(()),
            Err(_) => {
                // Server-side copy failed — fall back to download + upload
                return copy_via_download(
                    src_client, src_bucket, src_key,
                    dest_client, dest_bucket, dest_key,
                    object_size,
                ).await;
            }
        }
    } else {
        let result = copy_object_multipart(
            src_bucket, src_key, dest_client, dest_bucket, dest_key, object_size,
        ).await;
        match result {
            Ok(()) => return Ok(()),
            Err(_) => {
                // Server-side multipart copy failed — fall back to download + upload
                return copy_via_download(
                    src_client, src_bucket, src_key,
                    dest_client, dest_bucket, dest_key,
                    object_size,
                ).await;
            }
        }
    }
}

/// Download from source and upload to destination.
/// Used as fallback when server-side copy fails (cross-provider copies).
/// Uses range-based GETs for large files to keep memory usage bounded.
async fn copy_via_download(
    src_client: &S3Client,
    src_bucket: &str,
    src_key: &str,
    dest_client: &S3Client,
    dest_bucket: &str,
    dest_key: &str,
    object_size: u64,
) -> Result<(), FmError> {
    if object_size < MULTIPART_THRESHOLD {
        // Small file: single GET + PUT
        let resp = src_client
            .get_object()
            .bucket(src_bucket)
            .key(src_key)
            .send()
            .await
            .map_err(|e| s3err(e.to_string()))?;
        let body = resp.body.collect().await
            .map_err(|e| s3err(e.to_string()))?;
        dest_client
            .put_object()
            .bucket(dest_bucket)
            .key(dest_key)
            .body(body.into_bytes().into())
            .send()
            .await
            .map_err(|e| s3err(e.to_string()))?;
    } else {
        // Large file: multipart upload with range-based GETs from source
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

        let part_size = std::cmp::max(PART_SIZE, object_size / 10_000 + 1);
        let num_parts = ((object_size + part_size - 1) / part_size) as i32;
        let mut completed_parts: Vec<(i32, String)> = Vec::with_capacity(num_parts as usize);

        for i in 0..num_parts {
            let start = i as u64 * part_size;
            let end = std::cmp::min(start + part_size, object_size) - 1;
            let part_number = i + 1;
            let chunk_size = end - start + 1;

            // Download chunk via range GET
            let get_resp = src_client
                .get_object()
                .bucket(src_bucket)
                .key(src_key)
                .range(format!("bytes={}-{}", start, end))
                .send()
                .await
                .map_err(|e| {
                    s3err(format!("Range GET failed for part {}: {}", part_number, e))
                })?;

            let chunk = get_resp.body.collect().await
                .map_err(|e| s3err(e.to_string()))?;

            // Upload chunk as part
            let upload_result = dest_client
                .upload_part()
                .bucket(dest_bucket)
                .key(dest_key)
                .upload_id(&upload_id)
                .part_number(part_number)
                .body(chunk.into_bytes().into())
                .send()
                .await;

            match upload_result {
                Ok(resp) => {
                    let etag = resp
                        .e_tag()
                        .ok_or_else(|| s3err("Missing ETag in upload_part response"))?
                        .to_string();
                    completed_parts.push((part_number, etag));
                    throttle(chunk_size).await;
                }
                Err(e) => {
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

        // Complete multipart upload
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
    }

    Ok(())
}

// ── Local file collection ───────────────────────────────────────────────────

/// Recursively collect local files for upload.
pub fn collect_local_files(
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
