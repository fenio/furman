use super::{
    error, fresh_id, job_sdk_error, Arc, AtomicBool, Binding, BoundSource, Endpoint, File, FmError,
    Ordering, ScanBudget, ScanProgress, Serialize, SourcePin, MAX_ROWS,
};
use glob_match::glob_match;
use nix::{
    dir::Dir,
    fcntl::{openat, OFlag},
    sys::stat::Mode,
};
use sha2::{Digest, Sha256};
use std::sync::atomic::AtomicU64;
use std::{collections::BTreeMap, io::Read, os::unix::fs::MetadataExt, path::Path};

#[derive(Clone)]
pub(super) struct FileInfo {
    pub size: u64,
    pub sha256: String,
    pub etag: Option<String>,
}
#[derive(Clone, Serialize)]
pub struct Row {
    pub id: String,
    pub relative_path: String,
    pub status: &'static str,
    pub source_size: Option<u64>,
    pub destination_size: Option<u64>,
}
#[derive(Clone)]
pub(super) struct Entry {
    pub row: Row,
    pub source: Option<FileInfo>,
    pub destination: Option<FileInfo>,
}
#[derive(Clone, Default, Serialize)]
pub struct Summary {
    pub new: u64,
    pub modified: u64,
    pub destination_only: u64,
    pub same: u64,
}
pub(super) struct Manifest {
    pub id: String,
    pub entries: Vec<Entry>,
    pub summary: Summary,
}
impl Manifest {
    pub(super) fn validate(&self, s3_source: bool) -> Result<(), FmError> {
        let mut ids = std::collections::HashSet::new();
        for entry in &self.entries {
            let status_valid = match entry.row.status {
                "new" => entry.source.is_some() && entry.destination.is_none(),
                "modified" => entry
                    .source
                    .as_ref()
                    .zip(entry.destination.as_ref())
                    .is_some_and(|(source, destination)| {
                        source.size != destination.size || source.sha256 != destination.sha256
                    }),
                "destination_only" => entry.source.is_none() && entry.destination.is_some(),
                _ => false,
            };
            if !status_valid
                || !ids.insert(&entry.row.id)
                || uuid::Uuid::parse_str(&entry.row.id).is_err()
                || entry.row.source_size != entry.source.as_ref().map(|file| file.size)
                || entry.row.destination_size != entry.destination.as_ref().map(|file| file.size)
                || entry
                    .source
                    .as_ref()
                    .is_some_and(|file| file.etag.is_some() != s3_source)
                || entry
                    .destination
                    .as_ref()
                    .is_some_and(|file| file.etag.is_none())
                || entry
                    .source
                    .iter()
                    .chain(entry.destination.iter())
                    .any(|file| {
                        file.sha256.len() != 64
                            || !file.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
                            || file.etag.as_deref().is_some_and(str::is_empty)
                    })
            {
                return Err(error(
                    "Native job manifest invariants failed; no usable complete handle was retained",
                ));
            }
        }
        Ok(())
    }
}

fn excluded(relative: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|pattern| {
        glob_match(pattern, relative)
            || glob_match(pattern, relative.rsplit('/').next().unwrap_or(relative))
    })
}
fn hex(value: impl AsRef<[u8]>) -> String {
    value
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn reserve_metadata(memory: &AtomicU64, bytes: usize, budget: &ScanBudget) -> Result<(), FmError> {
    if memory
        .fetch_add(bytes as u64 + 256, Ordering::Relaxed)
        .saturating_add(bytes as u64 + 256)
        > 32 * 1024 * 1024
    {
        return Err(budget.limited("32 MiB estimated job manifest input-metadata budget"));
    }
    Ok(())
}

pub(super) async fn compare(
    binding: Arc<Binding>,
    cancel: Arc<AtomicBool>,
    budget: Arc<ScanBudget>,
    progress: Arc<dyn Fn(ScanProgress) + Send + Sync>,
) -> Result<Manifest, FmError> {
    binding.validate()?;
    budget.check(&cancel)?;
    let memory = Arc::new(AtomicU64::new(0));
    let source = match &binding.source {
        BoundSource::S3(endpoint) => {
            let SourcePin::S3 { pin } = &binding.job.source else {
                return Err(error("Source binding does not match the native definition"));
            };
            s3_files(
                endpoint,
                &pin.prefix,
                &binding.job.exclude_patterns,
                &cancel,
                &budget,
                progress.as_ref(),
                &memory,
            )
            .await?
        }
        BoundSource::Local(root) => {
            let root = root.clone();
            let task_cancel = cancel.clone();
            let task_budget = budget.clone();
            let task_progress = progress.clone();
            let patterns = binding.job.exclude_patterns.clone();
            let lease = binding.clone();
            let task_memory = memory.clone();
            tokio::task::spawn_blocking(move || {
                let _lease = lease; // Never release the run lock while local I/O is still running.
                let mut result = BTreeMap::new();
                walk_local(
                    &root.directory,
                    "",
                    &patterns,
                    &task_cancel,
                    &task_budget,
                    task_progress.as_ref(),
                    &mut result,
                    &task_memory,
                )?;
                Ok::<_, FmError>(result)
            })
            .await
            .map_err(|_| error("Local capability-relative job comparison task failed"))??
        }
    };
    budget.check(&cancel)?;
    binding.validate()?;
    let destination = s3_files(
        &binding.destination,
        &binding.job.destination.prefix,
        &binding.job.exclude_patterns,
        &cancel,
        &budget,
        progress.as_ref(),
        &memory,
    )
    .await?;
    budget.check(&cancel)?;
    let manifest = merge(source, destination, &budget, &cancel)?;
    manifest.validate(matches!(binding.source, BoundSource::S3(_)))?;
    progress(budget.progress());
    Ok(manifest)
}

pub(super) fn walk_local(
    directory: &File,
    prefix: &str,
    patterns: &[String],
    cancel: &AtomicBool,
    budget: &ScanBudget,
    progress: &(dyn Fn(ScanProgress) + Send + Sync),
    files: &mut BTreeMap<String, FileInfo>,
    memory: &AtomicU64,
) -> Result<(), FmError> {
    budget.check(cancel)?;
    if prefix.len() > 4096 || prefix.bytes().filter(|byte| *byte == b'/').count() > 128 {
        return Err(budget.limited("Local job path/depth budget (4096 bytes / 128 directories)"));
    }
    // A fresh open file description avoids sharing a directory cursor with the
    // retained root capability or any later snapshot operation.
    let descriptor = openat(
        directory,
        Path::new("."),
        OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_CLOEXEC | OFlag::O_NOFOLLOW,
        Mode::empty(),
    )
    .map_err(|_| error("Cannot enumerate the bound local job directory"))?;
    let mut entries =
        Dir::from_fd(descriptor).map_err(|_| error("Cannot read the bound directory entries"))?;
    for entry in entries.iter() {
        let entry = entry.map_err(|_| error("Local directory enumeration failed"))?;
        let name = entry
            .file_name()
            .to_str()
            .map_err(|_| error("Job comparisons require UTF-8 filenames"))?;
        if matches!(name, "." | "..") {
            continue;
        }
        budget.object(cancel)?;
        let relative = format!("{prefix}{name}");
        if relative.len() > 4096 {
            return Err(budget.limited("4096-byte local relative-path budget"));
        }
        let file = File::from(
            openat(
                directory,
                name,
                OFlag::O_RDONLY | OFlag::O_CLOEXEC | OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK,
                Mode::empty(),
            )
            .map_err(|_| {
                error("Local job entry is unreadable or a symlink; no manifest was retained")
            })?,
        );
        let metadata = file
            .metadata()
            .map_err(|_| error("Cannot inspect local job entry"))?;
        if metadata.is_dir() {
            walk_local(
                &file,
                &format!("{relative}/"),
                patterns,
                cancel,
                budget,
                progress,
                files,
                memory,
            )?;
        } else if metadata.is_file() {
            if !excluded(&relative, patterns) {
                reserve_metadata(memory, relative.len() + 64, budget)?;
                let info = hash_local(file, cancel, budget, progress)?;
                if files.insert(relative, info).is_some() {
                    return Err(error("Duplicate local relative path in job comparison"));
                }
            }
        } else {
            return Err(error(
                "Job comparisons require regular files/directories; special nodes are not followed",
            ));
        }
        if budget.progress().scanned % 100 == 0 {
            progress(budget.progress());
        }
    }
    budget.check(cancel)
}
fn hash_local(
    mut file: File,
    cancel: &AtomicBool,
    budget: &ScanBudget,
    progress: &(dyn Fn(ScanProgress) + Send + Sync),
) -> Result<FileInfo, FmError> {
    let before = file
        .metadata()
        .map_err(|_| error("Cannot inspect local file before hashing"))?;
    let mut hash = Sha256::new();
    let mut bytes = 0u64;
    let mut buffer = [0; 65536];
    loop {
        budget.check(cancel)?;
        let count = file
            .read(&mut buffer)
            .map_err(|_| error("Local file read failed during job hashing"))?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
        bytes += count as u64;
        budget.bytes(count as u64, false);
        if budget.progress().bytes_read % (1024 * 1024) < count as u64 {
            progress(budget.progress());
        }
    }
    let after = file
        .metadata()
        .map_err(|_| error("Cannot inspect local file after hashing"))?;
    if bytes != before.len()
        || before.len() != after.len()
        || before.modified().ok() != after.modified().ok()
        || before.ctime() != after.ctime()
        || before.ctime_nsec() != after.ctime_nsec()
    {
        return Err(error(
            "Local source changed during hashing; bind and compare again",
        ));
    }
    Ok(FileInfo {
        size: bytes,
        sha256: hex(hash.finalize()),
        etag: None,
    })
}

pub(super) async fn s3_files(
    endpoint: &Endpoint,
    prefix: &str,
    patterns: &[String],
    cancel: &AtomicBool,
    budget: &ScanBudget,
    progress: &(dyn Fn(ScanProgress) + Send + Sync),
    memory: &AtomicU64,
) -> Result<BTreeMap<String, FileInfo>, FmError> {
    let mut listed = BTreeMap::new();
    crate::s3::scan::walk_objects(&endpoint.client, &endpoint.bucket, prefix, budget, cancel, |object| {
        let key = object.key().ok_or_else(|| error("Job listing contains an empty key"))?;
        if key.len() > 1024 { return Err(error("Job listing contains an oversized S3 key")); }
        let relative = key.strip_prefix(prefix).ok_or_else(|| error("Provider returned a key outside the exact bound job prefix"))?;
        let size = object.size().filter(|size| *size >= 0).ok_or_else(|| error("Job listing contains a missing or invalid object size"))? as u64;
        if key.ends_with('/') {
            if size != 0 { return Err(error("Nonempty slash-ending objects are not folder markers; job comparison refuses to silently omit them")); }
            return Ok(false);
        }
        if relative.is_empty() { return Err(error("Job listing contains an invalid relative object key")); }
        if !excluded(relative, patterns) {
            let etag = object.e_tag().filter(|etag| !etag.is_empty() && etag.len() <= 1024 && !etag.chars().any(char::is_control)).ok_or_else(|| error("Job S3 hashing requires bounded, opaque ETags to pin conditional reads"))?;
            reserve_metadata(memory, relative.len() + etag.len() + 64, budget)?;
            if listed.insert(relative.to_owned(), (size, etag.to_owned())).is_some() { return Err(error("Duplicate S3 relative keys in job listing; no complete manifest can be established")); }
        }
        Ok(false)
    }, progress).await?;
    let mut result = BTreeMap::new();
    for (relative, (size, etag)) in listed {
        budget.check(cancel)?;
        let key = format!("{prefix}{relative}"); // Never trim, URL-decode or resolve dot segments.
        let sha256 = hash_s3(endpoint, &key, size, &etag, cancel, budget, progress).await?;
        result.insert(
            relative,
            FileInfo {
                size,
                sha256,
                etag: Some(etag),
            },
        );
    }
    Ok(result)
}
pub(super) async fn hash_s3(
    endpoint: &Endpoint,
    key: &str,
    size: u64,
    etag: &str,
    cancel: &AtomicBool,
    budget: &ScanBudget,
    progress: &(dyn Fn(ScanProgress) + Send + Sync),
) -> Result<String, FmError> {
    let response = budget
        .request(cancel, async {
            endpoint
                .client
                .get_object()
                .bucket(&endpoint.bucket)
                .key(key)
                .if_match(etag)
                .send()
                .await
                .map_err(|failure| job_sdk_error("Job conditional GetObject (SHA-256)", &failure))
        })
        .await?;
    if response.e_tag() != Some(etag)
        || response
            .content_length()
            .filter(|length| *length >= 0)
            .map(|length| length as u64)
            != Some(size)
    {
        return Err(error("Conditional GET did not return the listed ETag and size; no complete job manifest was retained"));
    }
    let mut body = response.body;
    let mut hash = Sha256::new();
    let mut bytes = 0u64;
    loop {
        let chunk = budget.wait(cancel, async { body.try_next().await.map_err(|_| error("S3 body stream failed during job hashing; no response body was included in diagnostics")) }).await?;
        let Some(chunk) = chunk else {
            break;
        };
        bytes = bytes
            .checked_add(chunk.len() as u64)
            .ok_or_else(|| error("S3 response byte count overflow"))?;
        if bytes > size {
            return Err(error(
                "S3 body exceeded the listed size; object changed or response is invalid",
            ));
        }
        hash.update(&chunk);
        budget.bytes(chunk.len() as u64, true);
        if budget.progress().bytes_read % (1024 * 1024) < chunk.len() as u64 {
            progress(budget.progress());
        }
    }
    budget.check(cancel)?;
    progress(budget.progress());
    if bytes != size {
        return Err(error(
            "S3 body was incomplete while hashing the pinned job object",
        ));
    }
    Ok(hex(hash.finalize()))
}

pub(super) fn merge(
    mut source: BTreeMap<String, FileInfo>,
    mut destination: BTreeMap<String, FileInfo>,
    budget: &ScanBudget,
    cancel: &AtomicBool,
) -> Result<Manifest, FmError> {
    if source.values().chain(destination.values()).any(|file| {
        file.sha256.len() != 64 || !file.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
    }) || destination
        .values()
        .any(|file| file.etag.as_deref().is_none_or(str::is_empty))
    {
        return Err(error("Missing native content digest/opaque destination validator; equality cannot be established"));
    }
    let mut summary = Summary::default();
    let mut entries = vec![];
    while let Some((path, file)) = source.pop_first() {
        budget.check(cancel)?;
        let dest = destination.remove(&path);
        let status = match &dest {
            Some(dest) if dest.size == file.size && dest.sha256 == file.sha256 => {
                summary.same += 1;
                continue;
            }
            Some(_) => {
                summary.modified += 1;
                "modified"
            }
            None => {
                summary.new += 1;
                "new"
            }
        };
        if entries.len() >= MAX_ROWS {
            return Err(budget.limited("10,000-difference job manifest budget"));
        }
        let row = Row {
            id: fresh_id(),
            relative_path: path,
            status,
            source_size: Some(file.size),
            destination_size: dest.as_ref().map(|file| file.size),
        };
        entries.push(Entry {
            row,
            source: Some(file),
            destination: dest,
        });
    }
    for (path, file) in destination {
        budget.check(cancel)?;
        if entries.len() >= MAX_ROWS {
            return Err(budget.limited("10,000-difference job manifest budget"));
        }
        summary.destination_only += 1;
        let row = Row {
            id: fresh_id(),
            relative_path: path,
            status: "destination_only",
            source_size: None,
            destination_size: Some(file.size),
        };
        entries.push(Entry {
            row,
            source: None,
            destination: Some(file),
        });
    }
    Ok(Manifest {
        id: fresh_id(),
        entries,
        summary,
    })
}
