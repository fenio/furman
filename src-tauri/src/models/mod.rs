use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

// ── FileEntry ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub size: u64,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub symlink_target: Option<String>,
    /// Last-modified time as milliseconds since Unix epoch.
    pub modified: i64,
    /// Unix permission mode bits (e.g. 0o755).
    pub permissions: u32,
    pub owner: String,
    pub group: String,
    pub extension: Option<String>,
    pub git_status: Option<String>,
    pub storage_class: Option<String>,
}

// ── DirListing ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirListing {
    pub path: String,
    pub entries: Vec<FileEntry>,
    pub total_size: u64,
    pub free_space: u64,
}

// ── VolumeInfo ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeInfo {
    pub name: String,
    pub mount_point: String,
    pub total_space: u64,
    pub free_space: u64,
    pub fs_type: String,
}

// ── ProgressEvent ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressEvent {
    pub id: String,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub current_file: String,
    pub files_done: u32,
    pub files_total: u32,
}

// ── FmError ──────────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum FmError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Already exists: {0}")]
    AlreadyExists(String),

    #[error("{0}")]
    Other(String),

    #[error("S3: {0}")]
    S3(String),
}

// Tauri v2 requires command return errors to implement `Into<InvokeError>`.
// The simplest approach: implement `Serialize` so Tauri can auto-convert.
impl Serialize for FmError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

// Convenience conversions ────────────────────────────────────────────────────

impl From<notify::Error> for FmError {
    fn from(e: notify::Error) -> Self {
        FmError::Other(e.to_string())
    }
}

impl From<trash::Error> for FmError {
    fn from(e: trash::Error) -> Self {
        FmError::Other(e.to_string())
    }
}

impl From<nix::Error> for FmError {
    fn from(e: nix::Error) -> Self {
        FmError::Other(format!("nix: {e}"))
    }
}

// ── FileProperties ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileProperties {
    pub name: String,
    pub path: String,
    pub size: u64,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub symlink_target: Option<String>,
    /// Birth time (created) as milliseconds since Unix epoch.
    pub created: i64,
    /// Last modified time as milliseconds since Unix epoch.
    pub modified: i64,
    /// Last accessed time as milliseconds since Unix epoch.
    pub accessed: i64,
    /// Unix permission mode bits (e.g. 0o755).
    pub permissions: u32,
    pub owner: String,
    pub group: String,
    /// "File", "Directory", or "Symlink"
    pub kind: String,
}

// ── KmsKeyInfo ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KmsKeyInfo {
    pub key_id: String,
    pub arn: String,
    pub alias: Option<String>,
}

// ── S3ObjectProperties ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3ObjectProperties {
    pub key: String,
    pub size: u64,
    /// Last modified time as milliseconds since Unix epoch.
    pub modified: i64,
    pub content_type: Option<String>,
    pub etag: Option<String>,
    pub storage_class: Option<String>,
    pub restore_status: Option<String>,
    pub version_id: Option<String>,
}

// ── S3ObjectVersion ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3ObjectVersion {
    pub version_id: String,
    pub is_latest: bool,
    pub is_delete_marker: bool,
    pub size: u64,
    pub modified: i64,
    pub etag: Option<String>,
    pub storage_class: Option<String>,
}

// ── SearchEvent ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub is_dir: bool,
    /// Line number where the match was found (content search only).
    pub line_number: Option<u32>,
    /// Snippet of the matching line (content search only).
    pub snippet: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchDone {
    pub total_found: u32,
    pub cancelled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SearchEvent {
    Result(SearchResult),
    Done(SearchDone),
}

// ── SyncEvent ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncEntry {
    pub relative_path: String,
    pub status: String, // "new" | "modified" | "deleted" | "same"
    pub source_size: u64,
    pub dest_size: u64,
    pub source_modified: i64, // epoch ms, 0 if missing
    pub dest_modified: i64,   // epoch ms, 0 if missing
    pub source_etag: String,  // MD5 hex for local, ETag for S3 (empty if unavailable)
    pub dest_etag: String,    // MD5 hex for local, ETag for S3 (empty if unavailable)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SyncEvent {
    Entry(SyncEntry),
    Progress { scanned: u32 },
    Done {
        total: u32,
        new_count: u32,
        modified: u32,
        deleted: u32,
    },
}

// ── S3BucketVersioning ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3BucketVersioning {
    pub status: String,     // "Enabled" | "Suspended" | "Disabled"
    pub mfa_delete: String, // "Enabled" | "Disabled"
}

// ── S3BucketEncryption ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3EncryptionRule {
    pub sse_algorithm: String,
    pub kms_key_id: Option<String>,
    pub bucket_key_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3BucketEncryption {
    pub rules: Vec<S3EncryptionRule>,
}

// ── S3ObjectMetadata ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3ObjectMetadata {
    pub content_type: Option<String>,
    pub content_disposition: Option<String>,
    pub cache_control: Option<String>,
    pub content_encoding: Option<String>,
    pub custom: HashMap<String, String>,
}

// ── S3Tag ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3Tag {
    pub key: String,
    pub value: String,
}

// ── S3MultipartUpload ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3MultipartUpload {
    pub key: String,
    pub upload_id: String,
    pub initiated: i64,
}

// ── S3LifecycleRule ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3LifecycleTransition {
    pub days: i32,
    pub storage_class: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3LifecycleRule {
    pub id: String,
    pub prefix: String,
    pub enabled: bool,
    pub transitions: Vec<S3LifecycleTransition>,
    pub expiration_days: Option<i32>,
    pub noncurrent_transitions: Vec<S3LifecycleTransition>,
    pub noncurrent_expiration_days: Option<i32>,
    pub abort_incomplete_days: Option<i32>,
}

// ── Transfer Checkpoints ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferCheckpoint {
    pub files_completed: Vec<String>,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub files_done: u32,
    pub files_total: u32,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3CompletedPart {
    pub part_number: i32,
    pub etag: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3UploadCheckpoint {
    pub files_completed: Vec<String>,
    pub current_file_upload_id: Option<String>,
    pub current_file_key: Option<String>,
    pub completed_parts: Vec<S3CompletedPart>,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub files_done: u32,
    pub files_total: u32,
}

// ── S3CorsRule ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3CorsRule {
    pub allowed_origins: Vec<String>,
    pub allowed_methods: Vec<String>,
    pub allowed_headers: Vec<String>,
    pub expose_headers: Vec<String>,
    pub max_age_seconds: Option<i32>,
}

// ── S3PublicAccessBlock ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3PublicAccessBlock {
    pub block_public_acls: bool,
    pub ignore_public_acls: bool,
    pub block_public_policy: bool,
    pub restrict_public_buckets: bool,
}

// ── S3BucketAcl ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3AclGrant {
    pub grantee_type: String,
    pub grantee_id: Option<String>,
    pub grantee_uri: Option<String>,
    pub grantee_email: Option<String>,
    pub grantee_display_name: Option<String>,
    pub permission: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3BucketAcl {
    pub owner_id: String,
    pub owner_display_name: Option<String>,
    pub grants: Vec<S3AclGrant>,
}

// ── S3BucketWebsite ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3BucketWebsite {
    pub enabled: bool,
    pub index_document: String,
    pub error_document: Option<String>,
}

// ── S3BucketLogging ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3BucketLogging {
    pub enabled: bool,
    pub target_bucket: Option<String>,
    pub target_prefix: Option<String>,
}

// ── S3BucketOwnership ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3BucketOwnership {
    pub object_ownership: String, // "BucketOwnerEnforced" | "BucketOwnerPreferred" | "ObjectWriter"
}

// ── S3 Object Lock ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3ObjectLockConfig {
    pub enabled: bool,
    pub default_retention_mode: Option<String>,  // "GOVERNANCE" | "COMPLIANCE"
    pub default_retention_days: Option<i32>,
    pub default_retention_years: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3ObjectRetention {
    pub mode: Option<String>,              // "GOVERNANCE" | "COMPLIANCE"
    pub retain_until_date: Option<String>, // ISO-8601
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3ObjectLegalHold {
    pub status: String, // "ON" | "OFF"
}

// ── Display impls ───────────────────────────────────────────────────────────

impl fmt::Display for ProgressEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[{}] {}/{} bytes, file {}/{}: {}",
            self.id,
            self.bytes_done,
            self.bytes_total,
            self.files_done,
            self.files_total,
            self.current_file,
        )
    }
}
