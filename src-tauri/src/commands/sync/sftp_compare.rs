//! Page-bounded read-only SFTP on a comparison-owned subsystem. Dropping the
//! session closes its channel and outstanding handles, never the pane session.
use super::{is_excluded, FileInfo};
use crate::models::FmError;
use crate::s3::scan::{ScanBudget, ScanProgress};
use crate::sftp::{helpers::parse_sftp_path, sftperr};
use russh_sftp::{
    client::{error::Error as SftpError, RawSftpSession},
    protocol::{FileAttributes, OpenFlags, StatusCode},
};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicBool;
use tokio::io::{AsyncRead, AsyncWrite};

pub(super) struct Session {
    raw: RawSftpSession,
    host: String,
    port: u16,
}

impl Session {
    pub(super) async fn from_ssh(
        ssh: &russh::client::Handle<crate::sftp::client::SshHandler>,
        host: String,
        port: u16,
        cancel: &AtomicBool,
        budget: &ScanBudget,
    ) -> Result<Self, FmError> {
        let channel = budget
            .request(cancel, async {
                ssh.channel_open_session()
                    .await
                    .map_err(|e| sftperr(format!("Open comparison channel: {e}")))
            })
            .await?;
        budget
            .request(cancel, async {
                channel
                    .request_subsystem(true, "sftp")
                    .await
                    .map_err(|e| sftperr(format!("Start comparison SFTP subsystem: {e}")))
            })
            .await?;
        Self::new(channel.into_stream(), host, port, cancel, budget).await
    }

    pub(super) async fn new<S>(
        stream: S,
        host: String,
        port: u16,
        cancel: &AtomicBool,
        budget: &ScanBudget,
    ) -> Result<Self, FmError>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let raw = RawSftpSession::new(stream);
        budget
            .request(cancel, async {
                raw.init()
                    .await
                    .map_err(|e| sftperr(format!("Initialize comparison SFTP subsystem: {e}")))
            })
            .await?;
        Ok(Self { raw, host, port })
    }
}

fn remote_path(session: &Session, uri: &str) -> Result<String, FmError> {
    let (host, port, path) = parse_sftp_path(uri)
        .ok_or_else(|| sftperr("Comparison requires an absolute sftp://host:port/folder path"))?;
    if host != session.host || port != session.port {
        return Err(sftperr(
            "Comparison path does not belong to its SFTP connection",
        ));
    }
    validate_path(&path)?;
    Ok(path.trim_end_matches('/').to_owned() + "/")
}

fn validate_path(path: &str) -> Result<(), FmError> {
    if !path.starts_with('/')
        || path.contains('\0')
        || path.split('/').any(|part| part == "." || part == "..")
    {
        return Err(sftperr(
            "Comparison requires an absolute SFTP path without dot segments",
        ));
    }
    Ok(())
}

fn same_file(before: &FileAttributes, after: &FileAttributes) -> bool {
    before.is_regular()
        && after.is_regular()
        && before.size.is_some()
        && before.size == after.size
        && before.mtime.is_some()
        && before.mtime == after.mtime
        && before.permissions == after.permissions
}

async fn metadata(
    session: &Session,
    path: &str,
    cancel: &AtomicBool,
    budget: &ScanBudget,
) -> Result<FileAttributes, FmError> {
    budget
        .request(cancel, async {
            session
                .raw
                .lstat(path)
                .await
                .map(|reply| reply.attrs)
                .map_err(|e| sftperr(format!("Inspect SFTP comparison entry: {e}")))
        })
        .await
}

pub(super) async fn collect(
    session: &Session,
    uri: &str,
    checksum: bool,
    cancel: &AtomicBool,
    excludes: &[String],
    budget: &ScanBudget,
    progress: &(dyn Fn(ScanProgress) + Send + Sync),
) -> Result<HashMap<String, FileInfo>, FmError> {
    let root = remote_path(session, uri)?;
    let mut pending = vec![(root, String::new(), 0usize)];
    let mut files = HashMap::new();
    let mut metadata_bytes = 0usize;
    while let Some((directory, relative, depth)) = pending.pop() {
        budget.check(cancel)?;
        if depth > 128 {
            return Err(
                budget.limited("SFTP comparison directory depth limit reached (128 levels)")
            );
        }
        let inspect_path = directory.trim_end_matches('/');
        let inspect_path = if inspect_path.is_empty() {
            "/"
        } else {
            inspect_path
        };
        let before = metadata(session, inspect_path, cancel, budget).await?;
        if !before.is_dir() || before.is_symlink() {
            return Err(sftperr(
                "Comparison requires real SFTP directories, not symlinks",
            ));
        }
        let handle = budget
            .request(cancel, async {
                session
                    .raw
                    .opendir(&directory)
                    .await
                    .map(|reply| reply.handle)
                    .map_err(|e| sftperr(format!("Open SFTP comparison folder: {e}")))
            })
            .await?;
        let mut names = HashSet::new();
        loop {
            let page = budget
                .request(cancel, async {
                    match session.raw.readdir(&handle).await {
                        Ok(page) => Ok(Some(page)),
                        Err(SftpError::Status(status)) if status.status_code == StatusCode::Eof => {
                            Ok(None)
                        }
                        Err(e) => Err(sftperr(format!("Read SFTP comparison folder page: {e}"))),
                    }
                })
                .await?;
            let Some(page) = page else {
                break;
            };
            if page.files.is_empty() {
                return Err(sftperr("SFTP returned an empty directory page without EOF"));
            }
            for entry in page.files {
                budget.object(cancel)?;
                let name = entry.filename;
                // Dot entries are protocol navigation records, never tree data.
                if name == "." || name == ".." {
                    continue;
                }
                if name.is_empty() || name.contains(['/', '\0']) || !names.insert(name.clone()) {
                    return Err(sftperr(
                        "SFTP listing returned an ambiguous or invalid filename",
                    ));
                }
                let rel = format!("{relative}{name}");
                metadata_bytes =
                    metadata_bytes.saturating_add(rel.len().saturating_mul(3).saturating_add(256));
                if metadata_bytes > 32 * 1024 * 1024 {
                    return Err(budget.limited("SFTP comparison metadata limit reached (32 MiB estimated); narrow the root or exclusions"));
                }
                if is_excluded(&rel, excludes) {
                    continue;
                }
                let path = format!("{directory}{name}");
                // Never trust readdir attrs as lstat; servers may omit types.
                let meta = metadata(session, &path, cancel, budget).await?;
                if meta.is_dir() && is_excluded(&format!("{rel}/"), excludes) {
                    continue;
                }
                if meta.is_symlink() {
                    return Err(sftperr(format!("SFTP comparison refuses symbolic links: {rel}. Exclude this entry explicitly.")));
                }
                if meta.is_dir() {
                    pending.push((format!("{path}/"), format!("{rel}/"), depth + 1));
                } else if meta.is_regular() {
                    let size = meta
                        .size
                        .ok_or_else(|| sftperr("SFTP file size was not returned"))?;
                    let modified = meta
                        .mtime
                        .ok_or_else(|| sftperr("SFTP modification time was not returned"))?
                        as i64
                        * 1000;
                    let hash = if checksum {
                        Some(hash_file(session, &path, &meta, cancel, budget, progress).await?)
                    } else {
                        None
                    };
                    if files.insert(rel, (size, modified, hash)).is_some() {
                        return Err(sftperr("Duplicate SFTP comparison path"));
                    }
                } else {
                    return Err(sftperr(format!(
                        "SFTP comparison requires regular files: {rel}"
                    )));
                }
                if budget.progress().scanned % 100 == 0 {
                    progress(budget.progress());
                }
            }
        }
        budget
            .request(cancel, async {
                session
                    .raw
                    .close(handle)
                    .await
                    .map(|_| ())
                    .map_err(|e| sftperr(format!("Close SFTP comparison folder: {e}")))
            })
            .await?;
        let after = metadata(session, inspect_path, cancel, budget).await?;
        if !after.is_dir() || after.is_symlink() || before.mtime != after.mtime {
            return Err(sftperr(
                "SFTP directory changed during comparison; compare again",
            ));
        }
        progress(budget.progress());
    }
    budget.check(cancel)?;
    Ok(files)
}

async fn hash_file(
    session: &Session,
    path: &str,
    before: &FileAttributes,
    cancel: &AtomicBool,
    budget: &ScanBudget,
    progress: &(dyn Fn(ScanProgress) + Send + Sync),
) -> Result<String, FmError> {
    let handle = budget
        .request(cancel, async {
            session
                .raw
                .open(path, OpenFlags::READ, FileAttributes::empty())
                .await
                .map(|reply| reply.handle)
                .map_err(|e| sftperr(format!("Open SFTP file read-only: {e}")))
        })
        .await?;
    let opened = budget
        .request(cancel, async {
            session
                .raw
                .fstat(&handle)
                .await
                .map(|reply| reply.attrs)
                .map_err(|e| sftperr(format!("Inspect open SFTP file: {e}")))
        })
        .await?;
    if !same_file(before, &opened) {
        return Err(sftperr("SFTP file changed before hashing"));
    }
    let mut hasher = Sha256::new();
    let mut total = 0u64;
    loop {
        let data = budget
            .request(cancel, async {
                match session.raw.read(&handle, total, 32 * 1024).await {
                    Ok(data) => Ok(Some(data.data)),
                    Err(SftpError::Status(status)) if status.status_code == StatusCode::Eof => {
                        Ok(None)
                    }
                    Err(e) => Err(sftperr(format!("Read SFTP comparison file: {e}"))),
                }
            })
            .await?;
        let Some(data) = data else {
            break;
        };
        if data.is_empty() || data.len() > 32 * 1024 {
            return Err(sftperr("Invalid SFTP comparison read length"));
        }
        total = total
            .checked_add(data.len() as u64)
            .ok_or_else(|| sftperr("SFTP file size overflow"))?;
        if Some(total) > before.size {
            return Err(sftperr("SFTP file grew during hashing"));
        }
        hasher.update(&data);
        budget.bytes(data.len() as u64, false);
        if budget.progress().bytes_read % (1024 * 1024) < data.len() as u64 {
            progress(budget.progress());
        }
    }
    let after = budget
        .request(cancel, async {
            session
                .raw
                .fstat(&handle)
                .await
                .map(|reply| reply.attrs)
                .map_err(|e| sftperr(format!("Recheck open SFTP file: {e}")))
        })
        .await?;
    let path_after = metadata(session, path, cancel, budget).await?;
    budget
        .request(cancel, async {
            session
                .raw
                .close(handle)
                .await
                .map(|_| ())
                .map_err(|e| sftperr(format!("Close SFTP comparison file: {e}")))
        })
        .await?;
    if Some(total) != before.size || !same_file(before, &after) || !same_file(before, &path_after) {
        return Err(sftperr("SFTP file changed during hashing; compare again"));
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paths_and_drift_checks_fail_closed() {
        assert!(validate_path("/folder/literal%20name").is_ok());
        for path in ["relative", "/a/../b", "/a/./b", "/nul\0"] {
            assert!(validate_path(path).is_err());
        }
        let regular = FileAttributes {
            size: Some(3),
            mtime: Some(1),
            permissions: Some(0o100644),
            ..Default::default()
        };
        assert!(same_file(&regular, &regular));
        for changed in [
            FileAttributes {
                size: Some(4),
                ..regular.clone()
            },
            FileAttributes {
                mtime: Some(2),
                ..regular.clone()
            },
            FileAttributes {
                permissions: Some(0o120777),
                ..regular.clone()
            },
            FileAttributes {
                size: None,
                ..regular.clone()
            },
        ] {
            assert!(!same_file(&regular, &changed));
        }
    }
}

#[cfg(test)]
mod protocol_tests;
