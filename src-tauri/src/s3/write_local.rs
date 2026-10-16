//! Capability-relative reads for approved local upload sources.
use super::scan::{ScanBudget, ScanProgress};
use crate::models::FmError;
use nix::{
    dir::Dir,
    fcntl::{open, openat, OFlag},
    sys::stat::Mode,
};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs::File,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::{atomic::AtomicBool, Arc},
};

#[derive(Clone)]
pub struct LocalFile {
    root: Arc<File>,
    relative: PathBuf,
    pub display: String,
    pub size: u64,
    pub sha256: String,
}

fn error(error: nix::errno::Errno) -> FmError {
    FmError::Io(std::io::Error::from_raw_os_error(error as i32))
}

pub(crate) fn relative_open(root: &File, path: &Path, directory: bool) -> Result<File, FmError> {
    let mut fd = root.try_clone()?;
    let parts: Vec<_> = path.components().collect();
    for (index, component) in parts.iter().enumerate() {
        let Component::Normal(name) = component else {
            return Err(FmError::Other("Invalid relative upload path".into()));
        };
        let mut flags = OFlag::O_RDONLY | OFlag::O_CLOEXEC | OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK;
        if directory || index + 1 < parts.len() {
            flags |= OFlag::O_DIRECTORY;
        }
        fd = File::from(openat(&fd, *name, flags, Mode::empty()).map_err(error)?);
    }
    Ok(fd)
}

fn digest(
    mut file: File,
    mut output: Option<&mut File>,
    budget: Option<&ScanBudget>,
    cancel: &AtomicBool,
    on_progress: &(dyn Fn(ScanProgress) + Send + Sync),
) -> Result<(u64, String), FmError> {
    if !file.metadata()?.is_file() {
        return Err(FmError::Other(
            "Guarded uploads require regular files; symlinks and special files are not supported"
                .into(),
        ));
    }
    let before = file.metadata()?;
    let mut hash = Sha256::new();
    let mut size = 0;
    let mut buffer = [0u8; 65536];
    loop {
        if let Some(budget) = budget {
            budget.check(cancel)?;
        } else {
            super::verification::check_cancelled(cancel)?;
        }
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
        if let Some(ref mut output) = output {
            output.write_all(&buffer[..count])?;
        }
        size += count as u64;
        if let Some(budget) = budget {
            budget.bytes(count as u64, false);
            if budget.progress().bytes_read % (1024 * 1024) < count as u64 {
                on_progress(budget.progress());
            }
        }
    }
    let after = file.metadata()?;
    if before.len() != size
        || before.len() != after.len()
        || before.modified()? != after.modified()?
    {
        return Err(FmError::Other(
            "Local source changed while preparing guarded upload; create a fresh preview".into(),
        ));
    }
    Ok((
        size,
        hash.finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    ))
}

impl LocalFile {
    pub fn snapshot(&self, output: &mut File, cancel: &AtomicBool) -> Result<(), FmError> {
        let file = relative_open(&self.root, &self.relative, false)?;
        let (size, hash) = digest(file, Some(output), None, cancel, &|_| {})?;
        if size != self.size || hash != self.sha256 {
            return Err(FmError::Other(format!("Local source no longer matches the approved preview: {}. Nothing from this file was uploaded; preview again.", self.display)));
        }
        Ok(())
    }
}

/// Returns source and relative S3 key. Every path is opened relative to a pinned
/// directory descriptor, including during execution after a directory is renamed.
pub fn collect(
    path: &str,
    roots: &mut HashMap<PathBuf, Arc<File>>,
    budget: &ScanBudget,
    cancel: &AtomicBool,
    on_progress: &(dyn Fn(ScanProgress) + Send + Sync),
) -> Result<Vec<(LocalFile, String)>, FmError> {
    let selected = Path::new(path);
    let metadata = std::fs::symlink_metadata(selected)?;
    if metadata.is_symlink() {
        return Err(FmError::Other(
            "Guarded uploads do not follow symbolic links".into(),
        ));
    }
    let name = selected
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .ok_or_else(|| {
            FmError::Other("Select a named file or directory with a UTF-8 name".into())
        })?;
    let root_path = if metadata.is_dir() {
        selected
    } else {
        selected.parent().unwrap_or(Path::new("."))
    };
    let canonical = root_path.canonicalize()?;
    let root = if let Some(root) = roots.get(&canonical) {
        root.clone()
    } else {
        if roots.len() >= 32 {
            return Err(FmError::Other(
                "Guarded preview supports at most 32 distinct local roots".into(),
            ));
        }
        let root = Arc::new(File::from(
            open(
                &canonical,
                OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_CLOEXEC | OFlag::O_NOFOLLOW,
                Mode::empty(),
            )
            .map_err(error)?,
        ));
        roots.insert(canonical.clone(), root.clone());
        root
    };
    let mut pending = vec![if metadata.is_dir() {
        PathBuf::new()
    } else {
        PathBuf::from(name)
    }];
    let mut files = Vec::new();
    while let Some(relative) = pending.pop() {
        budget.check(cancel)?;
        let file = relative_open(&root, &relative, false)?;
        let meta = file.metadata()?;
        if meta.is_dir() {
            let mut dir = Dir::from_fd(file.into()).map_err(error)?;
            for entry in dir.iter() {
                let entry = entry.map_err(error)?;
                let name = entry.file_name().to_str().map_err(|_| {
                    FmError::Other("Guarded uploads require UTF-8 filenames".into())
                })?;
                if name == "." || name == ".." {
                    continue;
                }
                budget.object(cancel)?;
                pending.push(relative.join(name));
            }
        } else {
            if !metadata.is_dir() {
                budget.object(cancel)?;
            }
            let key = if metadata.is_dir() {
                format!(
                    "{name}/{}",
                    relative
                        .to_str()
                        .ok_or_else(|| FmError::Other("Invalid upload filename".into()))?
                )
            } else {
                name.into()
            };
            if key.len() > 1024 {
                return Err(FmError::Other(
                    "Upload object key exceeds 1,024 bytes".into(),
                ));
            }
            let (size, sha256) = digest(file, None, Some(budget), cancel, on_progress)?;
            files.push((
                LocalFile {
                    root: root.clone(),
                    relative: relative.clone(),
                    display: canonical.join(&relative).to_string_lossy().into_owned(),
                    size,
                    sha256,
                },
                key,
            ));
            if files.len() > 10_000 {
                return Err(FmError::Other(
                    "Guarded preview supports at most 10,000 files; select a smaller scope".into(),
                ));
            }
        }
    }
    Ok(files)
}
