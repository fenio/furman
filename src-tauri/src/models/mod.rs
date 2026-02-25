use serde::{Deserialize, Serialize};
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
