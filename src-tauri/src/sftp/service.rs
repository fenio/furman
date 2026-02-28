use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use russh_sftp::client::SftpSession;
use russh_sftp::protocol::FileAttributes;

use crate::models::{DirListing, FileEntry, FmError, ProgressEvent, TransferCheckpoint};

use super::helpers::{sftp_path, sftperr};

// ── SftpService ──────────────────────────────────────────────────────────────

pub struct SftpService {
    pub session: Arc<SftpSession>,
    pub host: String,
    pub port: u16,
}

impl SftpService {
    pub fn new(session: Arc<SftpSession>, host: String, port: u16) -> Self {
        Self { session, host, port }
    }

    /// List directory contents, returning a DirListing with `..` entry.
    pub async fn list_objects(&self, path: &str) -> Result<DirListing, FmError> {
        let entries_raw = self
            .session
            .read_dir(path)
            .await
            .map_err(|e| sftperr(format!("readdir '{}': {}", path, e)))?;

        let mut entries = Vec::new();

        // Add parent directory entry
        let parent = parent_path(path);
        entries.push(FileEntry {
            name: "..".to_string(),
            path: sftp_path(&self.host, self.port, &parent),
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

        // ReadDir automatically skips "." and ".."
        for entry in entries_raw {
            let name = entry.file_name();
            let meta = entry.metadata();
            let is_dir = meta.is_dir();
            let full_path = if path.ends_with('/') {
                format!("{}{}", path, name)
            } else {
                format!("{}/{}", path, name)
            };
            let entry_path = if is_dir {
                format!("{}/", full_path)
            } else {
                full_path
            };

            let extension = if !is_dir {
                let dot = name.rfind('.');
                dot.map(|i| name[i + 1..].to_string())
            } else {
                None
            };

            let modified = meta
                .mtime
                .map(|t| t as i64 * 1000)
                .unwrap_or(0);

            let permissions = meta.permissions.unwrap_or(0);
            let size = meta.size.unwrap_or(0);

            entries.push(FileEntry {
                name,
                path: sftp_path(&self.host, self.port, &entry_path),
                size,
                is_dir,
                is_symlink: meta.is_symlink(),
                symlink_target: None,
                modified,
                permissions,
                owner: meta.uid.map(|u| u.to_string()).unwrap_or_default(),
                group: meta.gid.map(|g| g.to_string()).unwrap_or_default(),
                extension,
                git_status: None,
                storage_class: None,
            });
        }

        // Try to get filesystem info for free_space
        let free_space = match self.session.fs_info(path).await {
            Ok(Some(info)) => info.blocks_avail * info.fragment_size,
            _ => 0,
        };

        Ok(DirListing {
            path: sftp_path(&self.host, self.port, path),
            entries,
            total_size: 0,
            free_space,
        })
    }

    /// Delete files and directories (recursive for directories).
    pub async fn delete(&self, paths: &[String]) -> Result<(), FmError> {
        for path in paths {
            let clean = path.trim_end_matches('/');
            let meta = self
                .session
                .metadata(clean)
                .await
                .map_err(|e| sftperr(format!("stat '{}': {}", clean, e)))?;

            if meta.is_dir() {
                Box::pin(self.delete_dir_recursive(clean)).await?;
            } else {
                self.session
                    .remove_file(clean)
                    .await
                    .map_err(|e| sftperr(format!("remove '{}': {}", clean, e)))?;
            }
        }
        Ok(())
    }

    async fn delete_dir_recursive(&self, path: &str) -> Result<(), FmError> {
        let entries = self
            .session
            .read_dir(path)
            .await
            .map_err(|e| sftperr(format!("readdir '{}': {}", path, e)))?;

        for entry in entries {
            let name = entry.file_name();
            let child = if path.ends_with('/') {
                format!("{}{}", path, name)
            } else {
                format!("{}/{}", path, name)
            };
            let meta = entry.metadata();
            if meta.is_dir() {
                Box::pin(self.delete_dir_recursive(&child)).await?;
            } else {
                self.session
                    .remove_file(&child)
                    .await
                    .map_err(|e| sftperr(format!("remove '{}': {}", child, e)))?;
            }
        }

        self.session
            .remove_dir(path)
            .await
            .map_err(|e| sftperr(format!("rmdir '{}': {}", path, e)))?;

        Ok(())
    }

    /// Rename a file or directory.
    pub async fn rename(&self, old_path: &str, new_name: &str) -> Result<(), FmError> {
        let clean = old_path.trim_end_matches('/');
        let parent = parent_path(clean);
        let new_path = format!("{}/{}", parent, new_name);
        self.session
            .rename(clean, &new_path)
            .await
            .map_err(|e| sftperr(format!("rename '{}' → '{}': {}", clean, new_path, e)))?;
        Ok(())
    }

    /// Create a directory.
    pub async fn create_folder(&self, path: &str) -> Result<(), FmError> {
        self.session
            .create_dir(path)
            .await
            .map_err(|e| sftperr(format!("mkdir '{}': {}", path, e)))?;
        Ok(())
    }

    /// Get metadata for a single file.
    pub async fn stat(&self, path: &str) -> Result<FileAttributes, FmError> {
        let clean = path.trim_end_matches('/');
        self.session
            .metadata(clean)
            .await
            .map_err(|e| sftperr(format!("stat '{}': {}", clean, e)))
    }

    /// Download remote files to a local destination directory.
    pub async fn download(
        &self,
        remote_paths: &[String],
        local_dest: &str,
        op_id: &str,
        cancel: &AtomicBool,
        on_progress: &(dyn Fn(ProgressEvent) + Send + Sync),
    ) -> Result<Option<TransferCheckpoint>, FmError> {
        // First pass: collect all files and calculate total size
        let mut file_list: Vec<(String, String, u64)> = Vec::new(); // (remote_path, local_path, size)
        for remote_path in remote_paths {
            let clean = remote_path.trim_end_matches('/');
            let meta = self.stat(clean).await?;
            let name = clean.rsplit('/').next().unwrap_or(clean);
            let local_target = format!("{}/{}", local_dest.trim_end_matches('/'), name);

            if meta.is_dir() {
                Box::pin(self.collect_remote_files(clean, &local_target, &mut file_list))
                    .await?;
            } else {
                file_list.push((clean.to_string(), local_target, meta.size.unwrap_or(0)));
            }
        }

        let bytes_total: u64 = file_list.iter().map(|(_, _, s)| s).sum();
        let files_total = file_list.len() as u32;
        let mut bytes_done: u64 = 0;
        let mut files_done: u32 = 0;

        for (remote, local, _size) in &file_list {
            if cancel.load(Ordering::Relaxed) {
                return Err(FmError::Other("cancelled".into()));
            }

            // Ensure parent directory exists
            if let Some(parent) = Path::new(local).parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(FmError::Io)?;
            }

            // Download file
            let data = self
                .session
                .read(remote)
                .await
                .map_err(|e| sftperr(format!("read '{}': {}", remote, e)))?;

            tokio::fs::write(local, &data).await.map_err(FmError::Io)?;

            bytes_done += data.len() as u64;
            files_done += 1;

            on_progress(ProgressEvent {
                id: op_id.to_string(),
                bytes_done,
                bytes_total,
                current_file: remote.rsplit('/').next().unwrap_or(remote).to_string(),
                files_done,
                files_total,
            });
        }

        Ok(None)
    }

    /// Recursively collect files for download.
    async fn collect_remote_files(
        &self,
        remote_dir: &str,
        local_dir: &str,
        out: &mut Vec<(String, String, u64)>,
    ) -> Result<(), FmError> {
        let entries = self
            .session
            .read_dir(remote_dir)
            .await
            .map_err(|e| sftperr(format!("readdir '{}': {}", remote_dir, e)))?;

        for entry in entries {
            let name = entry.file_name();
            let remote_child = format!("{}/{}", remote_dir, name);
            let local_child = format!("{}/{}", local_dir, name);
            let meta = entry.metadata();

            if meta.is_dir() {
                Box::pin(self.collect_remote_files(&remote_child, &local_child, out))
                    .await?;
            } else {
                out.push((remote_child, local_child, meta.size.unwrap_or(0)));
            }
        }
        Ok(())
    }

    /// Upload local files to a remote directory.
    pub async fn upload(
        &self,
        local_paths: &[String],
        remote_dest: &str,
        op_id: &str,
        cancel: &AtomicBool,
        on_progress: &(dyn Fn(ProgressEvent) + Send + Sync),
    ) -> Result<Option<TransferCheckpoint>, FmError> {
        // Collect all local files
        let mut file_list: Vec<(std::path::PathBuf, String, u64)> = Vec::new();
        for local_path in local_paths {
            let path = std::path::Path::new(local_path);
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let remote_target = format!(
                "{}/{}",
                remote_dest.trim_end_matches('/'),
                name
            );

            if path.is_dir() {
                collect_local_files_recursive(path, &remote_target, &mut file_list)?;
            } else {
                let size = path.metadata().map(|m| m.len()).unwrap_or(0);
                file_list.push((path.to_path_buf(), remote_target, size));
            }
        }

        let bytes_total: u64 = file_list.iter().map(|(_, _, s)| s).sum();
        let files_total = file_list.len() as u32;
        let mut bytes_done: u64 = 0;
        let mut files_done: u32 = 0;

        for (local, remote, _size) in &file_list {
            if cancel.load(Ordering::Relaxed) {
                return Err(FmError::Other("cancelled".into()));
            }

            // Ensure remote parent directory exists
            if let Some(parent) = remote.rsplit_once('/').map(|(p, _)| p) {
                self.ensure_remote_dir(parent).await?;
            }

            let data = tokio::fs::read(local).await.map_err(FmError::Io)?;
            let len = data.len() as u64;

            self.session
                .write(remote, &data)
                .await
                .map_err(|e| sftperr(format!("write '{}': {}", remote, e)))?;

            bytes_done += len;
            files_done += 1;

            on_progress(ProgressEvent {
                id: op_id.to_string(),
                bytes_done,
                bytes_total,
                current_file: local
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default(),
                files_done,
                files_total,
            });
        }

        Ok(None)
    }

    /// Ensure a remote directory and all parents exist.
    async fn ensure_remote_dir(&self, path: &str) -> Result<(), FmError> {
        if self.session.try_exists(path).await.unwrap_or(false) {
            return Ok(());
        }
        // Recurse to parent
        if let Some((parent, _)) = path.rsplit_once('/') {
            if !parent.is_empty() {
                Box::pin(self.ensure_remote_dir(parent)).await?;
            }
        }
        // Create this level (ignore error if it already exists)
        let _ = self.session.create_dir(path).await;
        Ok(())
    }

    /// Download a remote file to a temp location, returning the local path.
    pub async fn download_temp(&self, remote_path: &str) -> Result<String, FmError> {
        let name = remote_path.rsplit('/').next().unwrap_or("file");
        let tmp_dir = std::env::temp_dir().join("furman-sftp");
        std::fs::create_dir_all(&tmp_dir).map_err(FmError::Io)?;
        let local_path = tmp_dir.join(name);

        let data = self
            .session
            .read(remote_path)
            .await
            .map_err(|e| sftperr(format!("read '{}': {}", remote_path, e)))?;

        tokio::fs::write(&local_path, &data)
            .await
            .map_err(FmError::Io)?;

        Ok(local_path.to_string_lossy().to_string())
    }

    /// Write text content to a remote file.
    pub async fn put_text(&self, remote_path: &str, content: &str) -> Result<(), FmError> {
        self.session
            .write(remote_path, content.as_bytes())
            .await
            .map_err(|e| sftperr(format!("write '{}': {}", remote_path, e)))?;
        Ok(())
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn parent_path(p: &str) -> String {
    let clean = p.trim_end_matches('/');
    match clean.rsplit_once('/') {
        Some(("", _)) => "/".to_string(),
        Some((parent, _)) => parent.to_string(),
        None => "/".to_string(),
    }
}

fn collect_local_files_recursive(
    dir: &Path,
    prefix: &str,
    out: &mut Vec<(std::path::PathBuf, String, u64)>,
) -> Result<(), FmError> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let remote = format!("{}/{}", prefix, name);

        if path.is_dir() {
            collect_local_files_recursive(&path, &remote, out)?;
        } else {
            let size = path.metadata().map(|m| m.len()).unwrap_or(0);
            out.push((path, remote, size));
        }
    }
    Ok(())
}
