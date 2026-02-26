use aws_credential_types::provider::ProvideCredentials;
use aws_config::BehaviorVersion;
use aws_sdk_s3::Client as S3Client;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::AsyncWriteExt;

use crate::models::{
    DirListing, FileEntry, FmError, ProgressEvent, S3AclGrant, S3BucketAcl, S3BucketEncryption,
    S3BucketVersioning, S3CorsRule, S3EncryptionRule, S3LifecycleRule, S3LifecycleTransition,
    S3MultipartUpload, S3ObjectMetadata, S3ObjectProperties, S3ObjectVersion, S3PublicAccessBlock,
    S3Tag, SearchDone, SearchEvent, SearchResult, TransferCheckpoint,
};

use super::helpers::*;

// ── S3Bucket model ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize)]
pub struct S3Bucket {
    pub name: String,
    pub created: i64, // epoch ms, 0 if unknown
}

// ── Standalone functions (no bucket context) ────────────────────────────────

/// Check whether the default AWS credential chain has valid credentials.
pub async fn check_credentials() -> Result<bool, FmError> {
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

/// List all accessible buckets using a client.
pub async fn list_buckets(client: &S3Client) -> Result<Vec<S3Bucket>, FmError> {
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

/// Create a new S3 bucket.
pub async fn create_bucket(client: &S3Client, name: &str, region: &str) -> Result<(), FmError> {
    let mut req = client.create_bucket().bucket(name);

    // us-east-1 must NOT set a location constraint (AWS quirk)
    if region != "us-east-1" {
        let constraint = aws_sdk_s3::types::CreateBucketConfiguration::builder()
            .location_constraint(aws_sdk_s3::types::BucketLocationConstraint::from(region))
            .build();
        req = req.create_bucket_configuration(constraint);
    }

    req.send()
        .await
        .map_err(|e| s3err(format!("Could not create bucket '{}': {}", name, e)))?;

    Ok(())
}

/// Delete an S3 bucket (must be empty).
pub async fn delete_bucket(client: &S3Client, name: &str) -> Result<(), FmError> {
    client
        .delete_bucket()
        .bucket(name)
        .send()
        .await
        .map_err(|e| s3err(format!("Could not delete bucket '{}': {}", name, e)))?;

    Ok(())
}

// ── S3Service ───────────────────────────────────────────────────────────────

pub struct S3Service {
    pub client: S3Client,
    pub bucket: String,
}

impl S3Service {
    pub fn new(client: S3Client, bucket: String) -> Self {
        Self { client, bucket }
    }

    // ── Object Listing & Navigation ─────────────────────────────────────

    /// List objects in the bucket at the given prefix, returning a DirListing.
    pub async fn list_objects(&self, prefix: &str) -> Result<DirListing, FmError> {
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
            path: s3_path(&self.bucket, &parent_prefix),
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
            let mut req = self
                .client
                .list_objects_v2()
                .bucket(&self.bucket)
                .delimiter("/")
                .prefix(prefix);

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
                    .strip_prefix(prefix)
                    .unwrap_or(pfx)
                    .trim_end_matches('/')
                    .to_string();
                if name.is_empty() {
                    continue;
                }
                entries.push(FileEntry {
                    name,
                    path: s3_path(&self.bucket, pfx),
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
                    .strip_prefix(prefix)
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
                    path: s3_path(&self.bucket, key),
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
            path: s3_path(&self.bucket, prefix),
            entries,
            total_size,
            free_space: 0,
        })
    }

    // ── Data Transfer ───────────────────────────────────────────────────

    /// Download S3 objects to a local destination directory.
    /// Returns None on success, Some(checkpoint) on pause.
    pub async fn download(
        &self,
        keys: &[String],
        destination: &str,
        op_id: &str,
        cancel: &AtomicBool,
        pause: &AtomicBool,
        on_progress: &(dyn Fn(ProgressEvent) + Send + Sync),
    ) -> Result<Option<TransferCheckpoint>, FmError> {
        let dest = PathBuf::from(destination);

        // Resolve actual keys: for prefix keys (dirs), list all children
        let mut resolved: Vec<(String, u64)> = Vec::new();
        for raw_key in keys {
            let key = strip_s3_prefix(raw_key, &self.bucket);
            if key.ends_with('/') {
                let children = list_all_objects(&self.client, &self.bucket, &key).await?;
                for (k, size, _) in children {
                    resolved.push((k, size));
                }
            } else {
                let head = self
                    .client
                    .head_object()
                    .bucket(&self.bucket)
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

        for (key, _size) in &resolved {
            if cancel.load(Ordering::Relaxed) {
                return Err(FmError::Other("Operation cancelled".into()));
            }
            if pause.load(Ordering::Relaxed) {
                return Ok(Some(TransferCheckpoint {
                    files_completed: completed_files,
                    bytes_done,
                    bytes_total,
                    files_done,
                    files_total,
                }));
            }

            let filename = key.rsplit('/').next().unwrap_or(key);
            let base_prefix = if keys.len() == 1 && keys[0].ends_with('/') {
                strip_s3_prefix(&keys[0], &self.bucket)
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

            let resp = self
                .client
                .get_object()
                .bucket(&self.bucket)
                .key(key)
                .send()
                .await
                .map_err(|e| s3err(e.to_string()))?;

            let etag = resp.e_tag().map(|s| s.trim_matches('"').to_string());
            let expected_size = *_size;
            let mut body = resp.body;
            let mut file = tokio::fs::File::create(&local_path).await.map_err(FmError::Io)?;
            let mut hasher = md5::Context::new();
            let mut file_bytes: u64 = 0;
            let bytes_done_base = bytes_done;

            loop {
                match body.try_next().await {
                    Ok(Some(chunk)) => {
                        hasher.consume(&chunk);
                        file.write_all(&chunk).await.map_err(FmError::Io)?;
                        file_bytes += chunk.len() as u64;
                        bytes_done = bytes_done_base + file_bytes;
                        throttle(chunk.len() as u64).await;
                        on_progress(ProgressEvent {
                            id: op_id.to_string(),
                            bytes_done,
                            bytes_total,
                            current_file: filename.to_string(),
                            files_done,
                            files_total,
                        });
                    }
                    Ok(None) => break,
                    Err(e) => {
                        drop(file);
                        let _ = tokio::fs::remove_file(&local_path).await;
                        return Err(s3err(e.to_string()));
                    }
                }
            }
            file.flush().await.map_err(FmError::Io)?;
            drop(file);

            // Verify checksum against ETag
            if let Some(ref etag_val) = etag {
                if !etag_val.contains('-') {
                    // Single-part upload: ETag is the MD5 hex digest
                    let computed = format!("{:x}", hasher.compute());
                    if computed != *etag_val {
                        let _ = tokio::fs::remove_file(&local_path).await;
                        return Err(s3err(format!(
                            "Checksum mismatch for '{}': expected {} got {}",
                            key, etag_val, computed
                        )));
                    }
                } else {
                    // Multipart upload: verify file size matches
                    if expected_size > 0 && file_bytes != expected_size {
                        let _ = tokio::fs::remove_file(&local_path).await;
                        return Err(s3err(format!(
                            "Size mismatch for '{}': expected {} got {}",
                            key, expected_size, file_bytes
                        )));
                    }
                }
            }

            files_done += 1;
            completed_files.push(key.clone());

            on_progress(ProgressEvent {
                id: op_id.to_string(),
                bytes_done,
                bytes_total,
                current_file: filename.to_string(),
                files_done,
                files_total,
            });
        }

        Ok(None)
    }

    /// Upload local files to an S3 prefix.
    /// Returns None on success, Some(checkpoint) on pause.
    pub async fn upload(
        &self,
        sources: &[String],
        dest_prefix: &str,
        op_id: &str,
        cancel: &AtomicBool,
        pause: &AtomicBool,
        on_progress: &(dyn Fn(ProgressEvent) + Send + Sync),
    ) -> Result<Option<TransferCheckpoint>, FmError> {
        // Collect all files to upload (expand directories)
        let mut file_list: Vec<(PathBuf, String)> = Vec::new();
        for source in sources {
            let src_path = PathBuf::from(source);
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

        for (local_path, key) in &file_list {
            if cancel.load(Ordering::Relaxed) {
                return Err(FmError::Other("Operation cancelled".into()));
            }
            if pause.load(Ordering::Relaxed) {
                return Ok(Some(TransferCheckpoint {
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
                let cancel_arc = Arc::new(AtomicBool::new(false));

                let op_id_c = op_id.to_string();
                let filename_c = filename.clone();
                let progress_cb = |new_bytes: u64| {
                    on_progress(ProgressEvent {
                        id: op_id_c.clone(),
                        bytes_done: new_bytes,
                        bytes_total,
                        current_file: filename_c.clone(),
                        files_done,
                        files_total,
                    });
                };

                upload_file_multipart(
                    &self.client,
                    &self.bucket,
                    key,
                    local_path,
                    file_size,
                    &cancel_arc,
                    &atomic_bytes_done,
                    &progress_cb,
                )
                .await?;
                bytes_done = atomic_bytes_done.load(Ordering::Relaxed);
            } else {
                // Small file: single put_object
                let data = std::fs::read(local_path)?;
                let size = data.len() as u64;

                self.client
                    .put_object()
                    .bucket(&self.bucket)
                    .key(key)
                    .body(data.into())
                    .send()
                    .await
                    .map_err(|e| s3err(e.to_string()))?;

                throttle(size).await;
                bytes_done += size;
            }

            files_done += 1;
            completed_files.push(key.clone());

            on_progress(ProgressEvent {
                id: op_id.to_string(),
                bytes_done,
                bytes_total,
                current_file: filename,
                files_done,
                files_total,
            });
        }

        Ok(None)
    }

    /// Server-side copy between S3 locations.
    /// Returns None on success, Some(checkpoint) on pause.
    pub async fn copy_objects(
        &self,
        src_client: &S3Client,
        src_bucket: &str,
        src_keys: &[String],
        dest_client: &S3Client,
        dest_bucket: &str,
        dest_prefix: &str,
        op_id: &str,
        cancel: &AtomicBool,
        pause: &AtomicBool,
        on_progress: &(dyn Fn(ProgressEvent) + Send + Sync),
    ) -> Result<Option<TransferCheckpoint>, FmError> {
        let mut resolved: Vec<(String, u64)> = Vec::new();
        for raw_key in src_keys {
            let key = strip_s3_prefix(raw_key, src_bucket);
            if key.ends_with('/') {
                let children = list_all_objects(src_client, src_bucket, &key).await?;
                for (k, size, _) in children {
                    resolved.push((k, size));
                }
            } else {
                let head = src_client
                    .head_object()
                    .bucket(src_bucket)
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

        for (key, size) in &resolved {
            if cancel.load(Ordering::Relaxed) {
                return Err(FmError::Other("Operation cancelled".into()));
            }
            if pause.load(Ordering::Relaxed) {
                return Ok(Some(TransferCheckpoint {
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
                src_bucket, key, dest_client, dest_bucket, &dest_key, *size,
            )
            .await?;

            bytes_done += size;
            files_done += 1;
            completed_files.push(key.clone());

            on_progress(ProgressEvent {
                id: op_id.to_string(),
                bytes_done,
                bytes_total,
                current_file: filename.to_string(),
                files_done,
                files_total,
            });
        }

        Ok(None)
    }

    // ── Object Operations ───────────────────────────────────────────────

    /// Get properties of a single S3 object via head_object.
    pub async fn head_object(&self, key: &str) -> Result<S3ObjectProperties, FmError> {
        let actual_key = strip_s3_prefix(key, &self.bucket);

        let head = self
            .client
            .head_object()
            .bucket(&self.bucket)
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
    pub async fn delete_objects(&self, keys: &[String]) -> Result<(), FmError> {
        let mut to_delete: Vec<String> = Vec::new();
        for raw_key in keys {
            let key = strip_s3_prefix(raw_key, &self.bucket);
            if key.ends_with('/') {
                let children = list_all_objects(&self.client, &self.bucket, &key).await?;
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

            self.client
                .delete_objects()
                .bucket(&self.bucket)
                .delete(delete)
                .send()
                .await
                .map_err(|e| s3err(e.to_string()))?;
        }

        Ok(())
    }

    /// Create a "folder" in S3 by putting a zero-byte object with a trailing-slash key.
    pub async fn create_folder(&self, key: &str) -> Result<(), FmError> {
        // Ensure key ends with /
        let folder_key = if key.ends_with('/') {
            key.to_string()
        } else {
            format!("{}/", key)
        };

        // Check if anything already exists under this prefix
        let check = self
            .client
            .list_objects_v2()
            .bucket(&self.bucket)
            .prefix(&folder_key)
            .max_keys(1)
            .send()
            .await
            .map_err(|e| s3err(e.to_string()))?;

        if !check.contents().is_empty() || !check.common_prefixes().is_empty() {
            return Err(FmError::AlreadyExists(folder_key));
        }

        // Put zero-byte object
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(&folder_key)
            .body(aws_sdk_s3::primitives::ByteStream::from_static(b""))
            .send()
            .await
            .map_err(|e| s3err(e.to_string()))?;

        Ok(())
    }

    /// Rename an S3 object or prefix (copy to new key, then delete original).
    pub async fn rename_object(&self, key: &str, new_name: &str) -> Result<(), FmError> {
        // Validate new_name
        if new_name.contains('/') || new_name.contains('\0') {
            return Err(s3err("Invalid name: must not contain '/' or null bytes"));
        }
        if new_name.is_empty() {
            return Err(s3err("Name cannot be empty"));
        }

        let actual_key = strip_s3_prefix(key, &self.bucket);

        if actual_key.ends_with('/') {
            self.rename_prefix(&actual_key, new_name).await
        } else {
            self.rename_file(&actual_key, new_name).await
        }
    }

    /// Rename a single S3 object by replacing the last path component.
    async fn rename_file(&self, key: &str, new_name: &str) -> Result<(), FmError> {
        let dest_key = match key.rfind('/') {
            Some(pos) => format!("{}/{}", &key[..pos], new_name),
            None => new_name.to_string(),
        };

        // Check destination doesn't already exist
        let dest_head = self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(&dest_key)
            .send()
            .await;
        if dest_head.is_ok() {
            return Err(FmError::AlreadyExists(dest_key));
        }

        // Get source object size for multipart copy routing
        let src_head = self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| s3err(e.to_string()))?;
        let object_size = src_head.content_length().unwrap_or(0) as u64;

        copy_single_or_multipart(
            &self.bucket, key, &self.client, &self.bucket, &dest_key, object_size,
        )
        .await?;

        // Delete original
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| s3err(e.to_string()))?;

        Ok(())
    }

    /// Rename an S3 prefix (folder) by copying all children to the new prefix, then deleting originals.
    async fn rename_prefix(&self, old_prefix: &str, new_name: &str) -> Result<(), FmError> {
        let trimmed = old_prefix.trim_end_matches('/');
        let new_prefix = match trimmed.rfind('/') {
            Some(pos) => format!("{}/{}/", &trimmed[..pos], new_name),
            None => format!("{}/", new_name),
        };

        // Check target prefix is empty
        let check = self
            .client
            .list_objects_v2()
            .bucket(&self.bucket)
            .prefix(&new_prefix)
            .max_keys(1)
            .send()
            .await
            .map_err(|e| s3err(e.to_string()))?;

        if !check.contents().is_empty() {
            return Err(FmError::AlreadyExists(new_prefix));
        }

        // List all objects under old prefix
        let children = list_all_objects(&self.client, &self.bucket, old_prefix).await?;
        if children.is_empty() {
            return Ok(());
        }

        // Copy each object to new prefix
        for (child_key, size, _) in &children {
            let relative = child_key
                .strip_prefix(old_prefix)
                .unwrap_or(child_key);
            let dest_key = format!("{}{}", new_prefix, relative);

            copy_single_or_multipart(
                &self.bucket, child_key, &self.client, &self.bucket, &dest_key, *size,
            )
            .await?;
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

            self.client
                .delete_objects()
                .bucket(&self.bucket)
                .delete(delete)
                .send()
                .await
                .map_err(|e| s3err(e.to_string()))?;
        }

        Ok(())
    }

    /// Search S3 objects by name under a prefix.
    pub async fn search_objects(
        &self,
        prefix: &str,
        query: &str,
        cancel: &AtomicBool,
        on_result: &(dyn Fn(SearchEvent) + Send + Sync),
    ) -> Result<(), FmError> {
        let query_lower = query.to_lowercase();
        let mut continuation_token: Option<String> = None;
        let mut total_found: u32 = 0;
        let mut streamed: u32 = 0;
        const MAX_STREAMED: u32 = 1000;

        loop {
            if cancel.load(Ordering::Relaxed) {
                on_result(SearchEvent::Done(SearchDone {
                    total_found,
                    cancelled: true,
                }));
                return Ok(());
            }

            let mut req = self
                .client
                .list_objects_v2()
                .bucket(&self.bucket)
                .prefix(prefix);

            if let Some(token) = &continuation_token {
                req = req.continuation_token(token);
            }

            let resp = req.send().await.map_err(|e| s3err(e.to_string()))?;

            for obj in resp.contents() {
                if cancel.load(Ordering::Relaxed) {
                    on_result(SearchEvent::Done(SearchDone {
                        total_found,
                        cancelled: true,
                    }));
                    return Ok(());
                }

                let key = match obj.key() {
                    Some(k) => k,
                    None => continue,
                };

                let filename = key.rsplit('/').next().unwrap_or(key);
                if filename.is_empty() {
                    continue;
                }

                if filename.to_lowercase().contains(&query_lower) {
                    total_found += 1;
                    if streamed < MAX_STREAMED {
                        let size = obj.size().unwrap_or(0) as u64;
                        let is_dir = key.ends_with('/');
                        on_result(SearchEvent::Result(SearchResult {
                            path: s3_path(&self.bucket, key),
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

        on_result(SearchEvent::Done(SearchDone {
            total_found,
            cancelled: false,
        }));

        Ok(())
    }

    // ── File Editing & Preview ──────────────────────────────────────────

    /// Download a single S3 object to a temp file and return the local path.
    pub async fn download_temp(&self, key: &str) -> Result<String, FmError> {
        let stripped_key = strip_s3_prefix(key, &self.bucket);

        // Check object size via head_object
        let head = self
            .client
            .head_object()
            .bucket(&self.bucket)
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
        let resp = self
            .client
            .get_object()
            .bucket(&self.bucket)
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

    /// Put text content directly to an S3 key.
    pub async fn put_text(&self, key: &str, content: &str) -> Result<(), FmError> {
        let stripped = strip_s3_prefix(key, &self.bucket);

        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(&stripped)
            .body(content.as_bytes().to_vec().into())
            .send()
            .await
            .map_err(|e| s3err(e.to_string()))?;

        Ok(())
    }

    // ── Storage Class Management ────────────────────────────────────────

    /// Change the storage class of an S3 object by copying it to itself.
    pub async fn change_storage_class(
        &self,
        key: &str,
        target_class: &str,
    ) -> Result<(), FmError> {
        let actual_key = strip_s3_prefix(key, &self.bucket);

        // Check object size — reject >5 GiB (copy_object limit)
        let head = self
            .client
            .head_object()
            .bucket(&self.bucket)
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

        let copy_source = format!("{}/{}", self.bucket, actual_key);
        let storage_class = aws_sdk_s3::types::StorageClass::from(target_class);

        self.client
            .copy_object()
            .bucket(&self.bucket)
            .key(&actual_key)
            .copy_source(&copy_source)
            .storage_class(storage_class)
            .metadata_directive(aws_sdk_s3::types::MetadataDirective::Copy)
            .send()
            .await
            .map_err(|e| s3err(e.to_string()))?;

        Ok(())
    }

    /// Bulk change storage class. Returns list of failed keys.
    pub async fn bulk_change_storage_class(
        &self,
        keys: &[String],
        target_class: &str,
    ) -> Result<Vec<String>, FmError> {
        let storage_class = aws_sdk_s3::types::StorageClass::from(target_class);
        let mut failed: Vec<String> = Vec::new();

        for key in keys {
            let actual_key = strip_s3_prefix(key, &self.bucket);
            let copy_source = format!("{}/{}", self.bucket, actual_key);

            let result = self
                .client
                .copy_object()
                .bucket(&self.bucket)
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

    // ── Glacier & Archive ───────────────────────────────────────────────

    /// Restore an object from Glacier or Deep Archive.
    pub async fn restore_object(
        &self,
        key: &str,
        days: i32,
        tier: &str,
    ) -> Result<(), FmError> {
        let actual_key = strip_s3_prefix(key, &self.bucket);

        let glacier_tier = aws_sdk_s3::types::Tier::from(tier);

        let glacier_params = aws_sdk_s3::types::GlacierJobParameters::builder()
            .tier(glacier_tier)
            .build()
            .map_err(|e| s3err(e.to_string()))?;

        let restore_request = aws_sdk_s3::types::RestoreRequest::builder()
            .days(days)
            .glacier_job_parameters(glacier_params)
            .build();

        self.client
            .restore_object()
            .bucket(&self.bucket)
            .key(&actual_key)
            .restore_request(restore_request)
            .send()
            .await
            .map_err(|e| s3err(e.to_string()))?;

        Ok(())
    }

    // ── Object Versioning ───────────────────────────────────────────────

    /// List all versions of an S3 object.
    pub async fn list_object_versions(&self, key: &str) -> Result<Vec<S3ObjectVersion>, FmError> {
        let actual_key = strip_s3_prefix(key, &self.bucket);
        let mut versions: Vec<S3ObjectVersion> = Vec::new();
        let mut key_marker: Option<String> = None;
        let mut version_id_marker: Option<String> = None;

        loop {
            let mut req = self
                .client
                .list_object_versions()
                .bucket(&self.bucket)
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
    pub async fn download_version(
        &self,
        key: &str,
        version_id: &str,
    ) -> Result<String, FmError> {
        let stripped_key = strip_s3_prefix(key, &self.bucket);

        let resp = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(&stripped_key)
            .version_id(version_id)
            .send()
            .await
            .map_err(|e| s3err(e.to_string()))?;

        let filename = stripped_key.rsplit('/').next().unwrap_or(&stripped_key);
        let short_vid = if version_id.len() > 8 { &version_id[..8] } else { version_id };
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
    pub async fn restore_version(
        &self,
        key: &str,
        version_id: &str,
    ) -> Result<(), FmError> {
        let actual_key = strip_s3_prefix(key, &self.bucket);

        let copy_source = format!(
            "{}/{}?versionId={}",
            self.bucket,
            actual_key,
            urlencoding::encode(version_id)
        );

        self.client
            .copy_object()
            .bucket(&self.bucket)
            .key(&actual_key)
            .copy_source(&copy_source)
            .send()
            .await
            .map_err(|e| s3err(e.to_string()))?;

        Ok(())
    }

    /// Delete a specific version of an S3 object.
    pub async fn delete_version(
        &self,
        key: &str,
        version_id: &str,
    ) -> Result<(), FmError> {
        let actual_key = strip_s3_prefix(key, &self.bucket);

        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(&actual_key)
            .version_id(version_id)
            .send()
            .await
            .map_err(|e| s3err(e.to_string()))?;

        Ok(())
    }

    // ── Presigned URLs ──────────────────────────────────────────────────

    /// Generate a presigned GET URL for an S3 object.
    pub async fn presign_url(
        &self,
        key: &str,
        expires_in_secs: u64,
    ) -> Result<String, FmError> {
        let actual_key = strip_s3_prefix(key, &self.bucket);

        let presign_config = aws_sdk_s3::presigning::PresigningConfig::expires_in(
            std::time::Duration::from_secs(expires_in_secs),
        )
        .map_err(|e| s3err(e.to_string()))?;

        let presigned = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(&actual_key)
            .presigned(presign_config)
            .await
            .map_err(|e| s3err(e.to_string()))?;

        Ok(presigned.uri().to_string())
    }

    // ── Versioning Configuration ────────────────────────────────────────

    /// Get versioning status for the bucket.
    pub async fn get_bucket_versioning(&self) -> Result<S3BucketVersioning, FmError> {
        let resp = self
            .client
            .get_bucket_versioning()
            .bucket(&self.bucket)
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

    /// Enable or suspend versioning on the bucket.
    pub async fn put_bucket_versioning(&self, enabled: bool) -> Result<(), FmError> {
        let status = if enabled {
            aws_sdk_s3::types::BucketVersioningStatus::Enabled
        } else {
            aws_sdk_s3::types::BucketVersioningStatus::Suspended
        };

        let config = aws_sdk_s3::types::VersioningConfiguration::builder()
            .status(status)
            .build();

        self.client
            .put_bucket_versioning()
            .bucket(&self.bucket)
            .versioning_configuration(config)
            .send()
            .await
            .map_err(|e| s3err(e.to_string()))?;

        Ok(())
    }

    // ── Encryption Configuration ────────────────────────────────────────

    /// Get encryption configuration for the bucket.
    pub async fn get_bucket_encryption(&self) -> Result<S3BucketEncryption, FmError> {
        let resp = self
            .client
            .get_bucket_encryption()
            .bucket(&self.bucket)
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

    // ── Object Metadata ─────────────────────────────────────────────────

    /// Get metadata for an S3 object.
    pub async fn get_object_metadata(&self, key: &str) -> Result<S3ObjectMetadata, FmError> {
        let actual_key = strip_s3_prefix(key, &self.bucket);

        let head = self
            .client
            .head_object()
            .bucket(&self.bucket)
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
    pub async fn put_object_metadata(
        &self,
        key: &str,
        content_type: Option<&str>,
        content_disposition: Option<&str>,
        cache_control: Option<&str>,
        content_encoding: Option<&str>,
        custom: &HashMap<String, String>,
    ) -> Result<(), FmError> {
        let actual_key = strip_s3_prefix(key, &self.bucket);

        // Check object size — reject >5 GiB (copy_object limit)
        let head = self
            .client
            .head_object()
            .bucket(&self.bucket)
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

        let copy_source = format!("{}/{}", self.bucket, actual_key);

        let mut req = self
            .client
            .copy_object()
            .bucket(&self.bucket)
            .key(&actual_key)
            .copy_source(&copy_source)
            .metadata_directive(aws_sdk_s3::types::MetadataDirective::Replace);

        if let Some(ct) = content_type {
            req = req.content_type(ct);
        }
        if let Some(cd) = content_disposition {
            req = req.content_disposition(cd);
        }
        if let Some(cc) = cache_control {
            req = req.cache_control(cc);
        }
        if let Some(ce) = content_encoding {
            req = req.content_encoding(ce);
        }

        for (k, v) in custom {
            req = req.metadata(k, v);
        }

        req.send()
            .await
            .map_err(|e| s3err(e.to_string()))?;

        Ok(())
    }

    // ── Tagging ─────────────────────────────────────────────────────────

    /// Get tags for an S3 object.
    pub async fn get_object_tags(&self, key: &str) -> Result<Vec<S3Tag>, FmError> {
        let actual_key = strip_s3_prefix(key, &self.bucket);

        let resp = self
            .client
            .get_object_tagging()
            .bucket(&self.bucket)
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
    pub async fn put_object_tags(
        &self,
        key: &str,
        tags: &[S3Tag],
    ) -> Result<(), FmError> {
        if tags.len() > 10 {
            return Err(s3err("Maximum 10 tags per object"));
        }

        let actual_key = strip_s3_prefix(key, &self.bucket);

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

        self.client
            .put_object_tagging()
            .bucket(&self.bucket)
            .key(&actual_key)
            .tagging(tagging)
            .send()
            .await
            .map_err(|e| s3err(e.to_string()))?;

        Ok(())
    }

    /// Get tags for the bucket.
    pub async fn get_bucket_tags(&self) -> Result<Vec<S3Tag>, FmError> {
        let resp = self
            .client
            .get_bucket_tagging()
            .bucket(&self.bucket)
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

    /// Set tags on the bucket (max 50 tags).
    pub async fn put_bucket_tags(&self, tags: &[S3Tag]) -> Result<(), FmError> {
        if tags.len() > 50 {
            return Err(s3err("Maximum 50 tags per bucket"));
        }

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

        self.client
            .put_bucket_tagging()
            .bucket(&self.bucket)
            .tagging(tagging)
            .send()
            .await
            .map_err(|e| s3err(e.to_string()))?;

        Ok(())
    }

    // ── Multipart Upload Cleanup ────────────────────────────────────────

    /// List incomplete multipart uploads for the bucket.
    pub async fn list_multipart_uploads(
        &self,
        prefix: Option<&str>,
    ) -> Result<Vec<S3MultipartUpload>, FmError> {
        let mut uploads: Vec<S3MultipartUpload> = Vec::new();
        let mut key_marker: Option<String> = None;
        let mut upload_id_marker: Option<String> = None;

        loop {
            let mut req = self.client.list_multipart_uploads().bucket(&self.bucket);

            if let Some(pfx) = prefix {
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
    pub async fn abort_multipart_upload(
        &self,
        key: &str,
        upload_id: &str,
    ) -> Result<(), FmError> {
        self.client
            .abort_multipart_upload()
            .bucket(&self.bucket)
            .key(key)
            .upload_id(upload_id)
            .send()
            .await
            .map_err(|e| s3err(e.to_string()))?;

        Ok(())
    }

    // ── Lifecycle Rules ─────────────────────────────────────────────────

    /// Get bucket lifecycle configuration rules.
    pub async fn get_bucket_lifecycle(&self) -> Result<Vec<S3LifecycleRule>, FmError> {
        let resp = self
            .client
            .get_bucket_lifecycle_configuration()
            .bucket(&self.bucket)
            .send()
            .await;

        match resp {
            Ok(r) => {
                let rules = r
                    .rules()
                    .iter()
                    .map(|rule| {
                        let id_str = rule.id().unwrap_or_default().to_string();

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
    pub async fn put_bucket_lifecycle(
        &self,
        rules: &[S3LifecycleRule],
    ) -> Result<(), FmError> {
        if rules.is_empty() {
            self.client
                .delete_bucket_lifecycle()
                .bucket(&self.bucket)
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

                if let Some(days) = r.expiration_days {
                    builder = builder.expiration(
                        aws_sdk_s3::types::LifecycleExpiration::builder()
                            .days(days)
                            .build(),
                    );
                }

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

                if let Some(days) = r.noncurrent_expiration_days {
                    builder = builder.noncurrent_version_expiration(
                        aws_sdk_s3::types::NoncurrentVersionExpiration::builder()
                            .noncurrent_days(days)
                            .build(),
                    );
                }

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

        self.client
            .put_bucket_lifecycle_configuration()
            .bucket(&self.bucket)
            .lifecycle_configuration(config)
            .send()
            .await
            .map_err(|e| s3err(e.to_string()))?;

        Ok(())
    }

    // ── CORS Configuration ──────────────────────────────────────────────

    /// Get CORS configuration for the bucket.
    pub async fn get_bucket_cors(&self) -> Result<Vec<S3CorsRule>, FmError> {
        let resp = self.client.get_bucket_cors().bucket(&self.bucket).send().await;

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

    /// Set CORS configuration for the bucket.
    pub async fn put_bucket_cors(&self, rules: &[S3CorsRule]) -> Result<(), FmError> {
        if rules.is_empty() {
            self.client
                .delete_bucket_cors()
                .bucket(&self.bucket)
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

        self.client
            .put_bucket_cors()
            .bucket(&self.bucket)
            .cors_configuration(config)
            .send()
            .await
            .map_err(|e| s3err(e.to_string()))?;

        Ok(())
    }

    // ── Public Access Block ─────────────────────────────────────────────

    /// Get public access block configuration for the bucket.
    pub async fn get_public_access_block(&self) -> Result<S3PublicAccessBlock, FmError> {
        let resp = self.client.get_public_access_block().bucket(&self.bucket).send().await;

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

    /// Set public access block configuration for the bucket.
    pub async fn put_public_access_block(
        &self,
        config: &S3PublicAccessBlock,
    ) -> Result<(), FmError> {
        let sdk_config = aws_sdk_s3::types::PublicAccessBlockConfiguration::builder()
            .block_public_acls(config.block_public_acls)
            .ignore_public_acls(config.ignore_public_acls)
            .block_public_policy(config.block_public_policy)
            .restrict_public_buckets(config.restrict_public_buckets)
            .build();

        self.client
            .put_public_access_block()
            .bucket(&self.bucket)
            .public_access_block_configuration(sdk_config)
            .send()
            .await
            .map_err(|e| s3err(e.to_string()))?;

        Ok(())
    }

    // ── Bucket Policy ───────────────────────────────────────────────────

    /// Get bucket policy as a JSON string.
    pub async fn get_bucket_policy(&self) -> Result<String, FmError> {
        let resp = self.client.get_bucket_policy().bucket(&self.bucket).send().await;

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

    /// Set bucket policy from a JSON string. Empty string deletes the policy.
    pub async fn put_bucket_policy(&self, policy: &str) -> Result<(), FmError> {
        if policy.trim().is_empty() {
            self.client
                .delete_bucket_policy()
                .bucket(&self.bucket)
                .send()
                .await
                .map_err(|e| s3err(e.to_string()))?;
            return Ok(());
        }

        // Validate JSON
        let _: serde_json::Value =
            serde_json::from_str(policy).map_err(|e| s3err(format!("Invalid JSON: {}", e)))?;

        self.client
            .put_bucket_policy()
            .bucket(&self.bucket)
            .policy(policy)
            .send()
            .await
            .map_err(|e| s3err(e.to_string()))?;

        Ok(())
    }

    // ── Bucket ACL (Read-Only) ──────────────────────────────────────────

    /// Get bucket ACL.
    pub async fn get_bucket_acl(&self) -> Result<S3BucketAcl, FmError> {
        let resp = self
            .client
            .get_bucket_acl()
            .bucket(&self.bucket)
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
}
