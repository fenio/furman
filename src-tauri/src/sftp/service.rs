use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use russh_sftp::client::SftpSession;
use russh_sftp::protocol::FileAttributes;
use tokio::io::AsyncWriteExt;

use crate::models::{DirListing, FileEntry, FmError, ProgressEvent, TransferCheckpoint};

use super::helpers::{sftp_path, sftperr};

// ── SftpService ──────────────────────────────────────────────────────────────

pub struct SftpService {
    pub session: Arc<SftpSession>,
    pub host: String,
    pub port: u16,
    pub operation_timeout_secs: u64,
}

impl SftpService {
    pub fn new(session: Arc<SftpSession>, host: String, port: u16, operation_timeout_secs: u64) -> Self {
        Self {
            session,
            host,
            port,
            operation_timeout_secs,
        }
    }

    /// List directory contents, returning a DirListing with `..` entry.
    pub async fn list_objects(&self, path: &str) -> Result<DirListing, FmError> {
        let entries_raw = self
            .session
            .read_dir(path)
            .await
            .map_err(|e| sftperr(format!("readdir '{path}': {e}")))?;

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
                format!("{path}{name}")
            } else {
                format!("{path}/{name}")
            };
            let entry_path = if is_dir {
                format!("{full_path}/")
            } else {
                full_path
            };

            let extension = if !is_dir {
                let dot = name.rfind('.');
                dot.map(|i| name[i + 1..].to_string())
            } else {
                None
            };

            let modified = meta.mtime.map(|t| t as i64 * 1000).unwrap_or(0);

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
            Ok(None) => 0,
            Err(e) => {
                log::warn!(
                    "SFTP fs_info on '{path}' failed ({e}); reporting free_space as 0"
                );
                0
            }
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
                .map_err(|e| sftperr(format!("stat '{clean}': {e}")))?;

            if meta.is_dir() {
                Box::pin(self.delete_dir_recursive(clean)).await?;
            } else {
                self.session
                    .remove_file(clean)
                    .await
                    .map_err(|e| sftperr(format!("remove '{clean}': {e}")))?;
            }
        }
        Ok(())
    }

    async fn delete_dir_recursive(&self, path: &str) -> Result<(), FmError> {
        let entries = self
            .session
            .read_dir(path)
            .await
            .map_err(|e| sftperr(format!("readdir '{path}': {e}")))?;

        for entry in entries {
            let name = entry.file_name();
            if name == "." || name == ".." {
                continue;
            }
            let child = if path.ends_with('/') {
                format!("{path}{name}")
            } else {
                format!("{path}/{name}")
            };
            let meta = entry.metadata();
            if meta.is_dir() {
                Box::pin(self.delete_dir_recursive(&child)).await?;
            } else {
                self.session
                    .remove_file(&child)
                    .await
                    .map_err(|e| sftperr(format!("remove '{child}': {e}")))?;
            }
        }

        self.session
            .remove_dir(path)
            .await
            .map_err(|e| sftperr(format!("rmdir '{path}': {e}")))?;

        Ok(())
    }

    /// Rename a file or directory.
    pub async fn rename(&self, old_path: &str, new_name: &str) -> Result<(), FmError> {
        let clean = old_path.trim_end_matches('/');
        let parent = parent_path(clean);
        let new_path = format!("{parent}/{new_name}");
        self.session
            .rename(clean, &new_path)
            .await
            .map_err(|e| sftperr(format!("rename '{clean}' → '{new_path}': {e}")))?;
        Ok(())
    }

    /// Create a directory.
    pub async fn create_folder(&self, path: &str) -> Result<(), FmError> {
        self.session
            .create_dir(path)
            .await
            .map_err(|e| sftperr(format!("mkdir '{path}': {e}")))?;
        Ok(())
    }

    /// Get metadata for a single file.
    pub async fn stat(&self, path: &str) -> Result<FileAttributes, FmError> {
        let clean = path.trim_end_matches('/');
        self.session
            .metadata(clean)
            .await
            .map_err(|e| sftperr(format!("stat '{clean}': {e}")))
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
        let pause = AtomicBool::new(false);
        self.download_with_pause(remote_paths, local_dest, op_id, cancel, &pause, on_progress)
            .await
    }

    pub async fn download_with_pause(
        &self,
        remote_paths: &[String],
        local_dest: &str,
        op_id: &str,
        cancel: &AtomicBool,
        pause: &AtomicBool,
        on_progress: &(dyn Fn(ProgressEvent) + Send + Sync),
    ) -> Result<Option<TransferCheckpoint>, FmError> {
        self.download_with_checkpoint(
            remote_paths,
            local_dest,
            op_id,
            cancel,
            pause,
            on_progress,
            None,
        )
        .await
    }

    pub async fn download_with_checkpoint(
        &self,
        remote_paths: &[String],
        local_dest: &str,
        op_id: &str,
        cancel: &AtomicBool,
        pause: &AtomicBool,
        on_progress: &(dyn Fn(ProgressEvent) + Send + Sync),
        checkpoint: Option<&TransferCheckpoint>,
    ) -> Result<Option<TransferCheckpoint>, FmError> {
        // Send an initial "scanning" progress event so the UI shows activity
        on_progress(ProgressEvent {
            id: op_id.to_string(),
            bytes_done: 0,
            bytes_total: 0,
            current_file: "Scanning…".to_string(),
            files_done: 0,
            files_total: 0,
        });

        // First pass: collect all files and calculate total size
        log::info!(
            "SFTP download: scanning {} paths in {}",
            remote_paths.len(),
            local_dest
        );
        let mut file_list: Vec<(String, String, u64)> = Vec::new(); // (remote_path, local_path, size)
        for (i, remote_path) in remote_paths.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                return Err(FmError::Other("cancelled".into()));
            }

            let clean = remote_path.trim_end_matches('/');
            log::info!(
                "SFTP scan [{}/{}]: stat '{}'",
                i + 1,
                remote_paths.len(),
                clean
            );
            let meta = self.stat(clean).await?;
            let name = clean.rsplit('/').next().unwrap_or(clean);
            let local_target = format!("{}/{}", local_dest.trim_end_matches('/'), name);

            if meta.is_dir() {
                log::info!("SFTP scan: recursing into '{clean}'");
                Box::pin(self.collect_remote_files(
                    clean,
                    &local_target,
                    &mut file_list,
                    op_id,
                    on_progress,
                ))
                .await?;
                log::info!(
                    "SFTP scan: done with '{}', {} files so far",
                    clean,
                    file_list.len()
                );
            } else if meta.is_regular() {
                file_list.push((clean.to_string(), local_target, meta.size.unwrap_or(0)));
            } else {
                return Err(sftperr(format!("unsupported remote file type: '{clean}'")));
            }

            // Report scanning progress so the UI stays responsive
            on_progress(ProgressEvent {
                id: op_id.to_string(),
                bytes_done: 0,
                bytes_total: 0,
                current_file: format!("Scanning… {} files found", file_list.len()),
                files_done: 0,
                files_total: 0,
            });
        }

        log::info!(
            "SFTP download: scan complete, {} files, starting download",
            file_list.len()
        );
        let mut bytes_total: u64 = file_list.iter().map(|(_, _, s)| s).sum();
        let mut files_total = file_list.len() as u32;
        let mut bytes_done = checkpoint.map_or(0, |c| c.bytes_done);
        let mut files_done = checkpoint.map_or(0, |c| c.files_done);
        let mut completed_files = checkpoint.map_or_else(Vec::new, |c| c.files_completed.clone());
        let completed: HashSet<String> = completed_files.iter().cloned().collect();
        if let Some(c) = checkpoint {
            bytes_total = c.bytes_total;
            files_total = c.files_total;
        }

        for (remote, local, _size) in &file_list {
            if cancel.load(Ordering::Relaxed) {
                return Err(FmError::Other("cancelled".into()));
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
            if completed.contains(remote) {
                continue;
            }

            // Ensure parent directory exists
            if let Some(parent) = Path::new(local).parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(FmError::Io)?;
            }

            // Download file
            log::info!(
                "SFTP download [{}/{}]: '{}'",
                files_done + 1,
                files_total,
                remote
            );
            let data = match tokio::time::timeout(
                std::time::Duration::from_secs(self.operation_timeout_secs),
                self.session.read(remote),
            )
            .await
            {
                Ok(Ok(d)) => d,
                Ok(Err(e)) => return Err(sftperr(format!("read '{remote}': {e}"))),
                Err(_) => {
                    log::error!(
                        "SFTP download: read '{remote}' timed out after {}s — connection likely dead",
                        self.operation_timeout_secs
                    );
                    return Err(FmError::Other(format!(
                        "SFTP read timed out on '{}' — connection lost",
                        remote.rsplit('/').next().unwrap_or(remote)
                    )));
                }
            };

            tokio::fs::write(local, &data).await.map_err(FmError::Io)?;

            bytes_done += data.len() as u64;
            files_done += 1;
            completed_files.push(remote.clone());

            on_progress(ProgressEvent {
                id: op_id.to_string(),
                bytes_done,
                bytes_total,
                current_file: remote.rsplit('/').next().unwrap_or(remote).to_string(),
                files_done,
                files_total,
            });
        }

        if cancel.load(Ordering::Relaxed) {
            return Err(FmError::Other("cancelled".into()));
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
        Ok(None)
    }

    /// Recursively collect files for download.
    async fn collect_remote_files(
        &self,
        remote_dir: &str,
        local_dir: &str,
        out: &mut Vec<(String, String, u64)>,
        op_id: &str,
        on_progress: &(dyn Fn(ProgressEvent) + Send + Sync),
    ) -> Result<(), FmError> {
        log::info!("SFTP collect: readdir '{remote_dir}'");
        let entries = self
            .session
            .read_dir(remote_dir)
            .await
            .map_err(|e| sftperr(format!("readdir '{remote_dir}': {e}")))?
            .collect::<Vec<_>>();
        log::info!(
            "SFTP collect: '{}' has {} entries",
            remote_dir,
            entries.len()
        );

        for entry in entries {
            let name = entry.file_name();
            let remote_child = format!("{remote_dir}/{name}");
            let local_child = format!("{local_dir}/{name}");
            let meta = entry.metadata();

            if meta.is_dir() {
                // Verify with stat before recursing — some entries (sockets, pipes)
                // can be misreported as directories by the SFTP server
                match self.session.metadata(&remote_child).await {
                    Ok(verified) if verified.is_dir() => {
                        Box::pin(self.collect_remote_files(
                            &remote_child,
                            &local_child,
                            out,
                            op_id,
                            on_progress,
                        ))
                        .await?;
                    }
                    Ok(verified) if verified.is_regular() => {
                        out.push((remote_child, local_child, verified.size.unwrap_or(0)));
                    }
                    Ok(_) => {
                        return Err(sftperr(format!(
                            "unsupported remote file type: '{remote_child}'"
                        )));
                    }
                    Err(e) => return Err(sftperr(format!("stat '{remote_child}': {e}"))),
                }
            } else if meta.is_regular() {
                out.push((remote_child, local_child, meta.size.unwrap_or(0)));
                on_progress(ProgressEvent {
                    id: op_id.to_string(),
                    bytes_done: 0,
                    bytes_total: 0,
                    current_file: format!("Scanning\u{2026} {} files found", out.len()),
                    files_done: 0,
                    files_total: 0,
                });
            }
            // Skip sockets, pipes, and other special files
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
        let pause = AtomicBool::new(false);
        self.upload_with_pause(local_paths, remote_dest, op_id, cancel, &pause, on_progress)
            .await
    }

    pub async fn upload_with_pause(
        &self,
        local_paths: &[String],
        remote_dest: &str,
        op_id: &str,
        cancel: &AtomicBool,
        pause: &AtomicBool,
        on_progress: &(dyn Fn(ProgressEvent) + Send + Sync),
    ) -> Result<Option<TransferCheckpoint>, FmError> {
        self.upload_with_checkpoint(
            local_paths,
            remote_dest,
            op_id,
            cancel,
            pause,
            on_progress,
            None,
        )
        .await
    }

    pub async fn upload_with_checkpoint(
        &self,
        local_paths: &[String],
        remote_dest: &str,
        op_id: &str,
        cancel: &AtomicBool,
        pause: &AtomicBool,
        on_progress: &(dyn Fn(ProgressEvent) + Send + Sync),
        checkpoint: Option<&TransferCheckpoint>,
    ) -> Result<Option<TransferCheckpoint>, FmError> {
        // Collect all local files
        let mut file_list: Vec<(std::path::PathBuf, String, u64)> = Vec::new();
        for local_path in local_paths {
            let path = std::path::Path::new(local_path);
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let remote_target = format!("{}/{}", remote_dest.trim_end_matches('/'), name);

            if path.is_dir() {
                collect_local_files_recursive(path, &remote_target, &mut file_list)?;
            } else {
                let size = path.metadata().map(|m| m.len()).unwrap_or(0);
                file_list.push((path.to_path_buf(), remote_target, size));
            }
        }

        let mut bytes_total: u64 = file_list.iter().map(|(_, _, s)| s).sum();
        let mut files_total = file_list.len() as u32;
        let mut bytes_done = checkpoint.map_or(0, |c| c.bytes_done);
        let mut files_done = checkpoint.map_or(0, |c| c.files_done);
        let mut completed_files = checkpoint.map_or_else(Vec::new, |c| c.files_completed.clone());
        let completed: HashSet<String> = completed_files.iter().cloned().collect();
        if let Some(c) = checkpoint {
            bytes_total = c.bytes_total;
            files_total = c.files_total;
        }

        for (local, remote, _size) in &file_list {
            if cancel.load(Ordering::Relaxed) {
                return Err(FmError::Other("cancelled".into()));
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
            let identity = local.to_string_lossy().into_owned();
            if completed.contains(&identity) {
                continue;
            }

            // Ensure remote parent directory exists
            if let Some(parent) = remote.rsplit_once('/').map(|(p, _)| p) {
                self.ensure_remote_dir(parent).await?;
            }

            let data = tokio::fs::read(local).await.map_err(FmError::Io)?;
            let len = data.len() as u64;

            let mut file = self
                .session
                .create(remote)
                .await
                .map_err(|e| sftperr(format!("create '{remote}': {e}")))?;
            file.write_all(&data)
                .await
                .map_err(|e| sftperr(format!("write '{remote}': {e}")))?;

            bytes_done += len;
            files_done += 1;
            completed_files.push(identity);

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

        if cancel.load(Ordering::Relaxed) {
            return Err(FmError::Other("cancelled".into()));
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
        Ok(None)
    }

    /// Ensure a remote directory and all parents exist.
    async fn ensure_remote_dir(&self, path: &str) -> Result<(), FmError> {
        match self.session.try_exists(path).await {
            Ok(true) => return Ok(()),
            Ok(false) => {}
            Err(e) => {
                log::warn!(
                    "SFTP try_exists on '{path}' failed ({e}); attempting create anyway"
                );
            }
        }
        // Recurse to parent
        if let Some((parent, _)) = path.rsplit_once('/') {
            if !parent.is_empty() {
                Box::pin(self.ensure_remote_dir(parent)).await?;
            }
        }
        // Create this level. If it already exists (race / try_exists failed
        // but the dir does exist), the server returns an error we can ignore;
        // a real failure (permission denied, quota, etc.) will surface when
        // the subsequent file write fails on this path, but log it here for
        // diagnosis.
        if let Err(e) = self.session.create_dir(path).await {
            log::warn!("SFTP create_dir '{path}' failed: {e}");
        }
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
            .map_err(|e| sftperr(format!("read '{remote_path}': {e}")))?;

        tokio::fs::write(&local_path, &data)
            .await
            .map_err(FmError::Io)?;

        Ok(local_path.to_string_lossy().to_string())
    }

    /// Write text content to a remote file.
    pub async fn put_text(&self, remote_path: &str, content: &str) -> Result<(), FmError> {
        let mut file = self
            .session
            .create(remote_path)
            .await
            .map_err(|e| sftperr(format!("create '{remote_path}': {e}")))?;
        file.write_all(content.as_bytes())
            .await
            .map_err(|e| sftperr(format!("write '{remote_path}': {e}")))?;
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
        let remote = format!("{prefix}/{name}");

        if path.is_dir() {
            collect_local_files_recursive(&path, &remote, out)?;
        } else {
            let size = path.metadata().map(|m| m.len()).unwrap_or(0);
            out.push((path, remote, size));
        }
    }
    Ok(())
}
