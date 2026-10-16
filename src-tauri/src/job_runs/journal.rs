//! Durable allowlisted attempt outcomes and native-only multipart cleanup evidence.
//! Runtime approvals, source mappings, hashes and payloads are never serialized.
use super::{fresh_id, preview::Policy, FmError, Paths, ScanProgress};
use serde::{Deserialize, Serialize};
#[cfg(target_os = "macos")]
use std::os::unix::io::AsRawFd;
use std::{
    collections::{BTreeMap, HashSet},
    fs::{self, File},
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

mod multipart;
#[cfg(test)]
mod tests;
pub(super) use multipart::resolve_evidence;
pub(super) use multipart::{Upload, UploadChange, UploadState};
const MAX_BYTES: u64 = 8 * 1024 * 1024;
const DIRTY: &[u8] = b"FURMAN-JOB-JOURNAL-DIRTY-V1\n";
const INITIALIZED: &[u8] = b"FURMAN-JOB-JOURNAL-INITIALIZED-V1\n";
fn error(message: &str) -> FmError {
    super::error(&format!("[JOB_JOURNAL] {message}"))
}
fn unavailable() -> FmError {
    error("Saved-job journal storage/durability is unavailable. No further job writes are authorized. Refresh recovery after checking local permissions, free space and filesystem support; an update or sent payload write may already have committed. Inspect destination/history before explicit review. No file contents were included.")
}
fn io_failure(failure: std::io::Error) -> FmError {
    error(&format!("Saved-job journal storage/durability failed ({:?}). Check local permissions, free space and filesystem support, then refresh recovery/review. An update or sent payload write may already have committed; no further job writes are authorized and no file contents were included.",failure.kind()))
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}
fn uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value)
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Binding,
    Bound,
    Scanning,
    Compared,
    Preparing,
    Prepared,
    Reviewing,
    AwaitingApproval,
    Executing,
    Succeeded,
    Skipped,
    Partial,
    Uncertain,
    NoChanges,
    Discarded,
    Cancelled,
    Failed,
    Limited,
    Interrupted,
}
impl Status {
    fn active(self) -> bool {
        matches!(
            self,
            Self::Binding
                | Self::Bound
                | Self::Scanning
                | Self::Compared
                | Self::Preparing
                | Self::Prepared
                | Self::Reviewing
                | Self::AwaitingApproval
                | Self::Executing
        )
    }
    fn pauses(self) -> bool {
        matches!(
            self,
            Self::Failed | Self::Limited | Self::Interrupted | Self::Partial | Self::Uncertain
        )
    }
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Counters {
    pub scanned: u64,
    pub requests: u64,
    pub bytes_read: u64,
    pub s3_bytes_read: u64,
    pub elapsed_ms: u64,
}
impl From<ScanProgress> for Counters {
    fn from(value: ScanProgress) -> Self {
        Self {
            scanned: value.scanned,
            requests: value.requests,
            bytes_read: value.bytes_read,
            s3_bytes_read: value.s3_bytes_read,
            elapsed_ms: value.elapsed_ms,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    #[serde(default)]
    pub multipart_cleanup_reviews: u64,
    pub run_id: String,
    pub job_id: String,
    pub job_revision: String,
    pub started_ms: u64,
    pub updated_ms: u64,
    pub status: Status,
    pub binding: Counters,
    pub comparison: Counters,
    pub preparation: Counters,
    pub policy: Option<Policy>,
    #[serde(default)]
    pub review: Counters,
    #[serde(default)]
    pub execution: Option<Execution>,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Execution {
    #[serde(default)]
    pub multipart_files: u64,
    #[serde(default)]
    pub replacement_files: u64,
    #[serde(default)]
    pub attempted_replacements: u64,
    #[serde(default)]
    pub acknowledged_replacements: u64,
    #[serde(default)]
    pub verified_replacements: u64,
    #[serde(default)]
    pub uncertain_replacements: u64,
    pub selected_files: u64,
    pub skipped_files: u64,
    pub selected_bytes: u64,
    pub attempted_files: u64,
    pub acknowledged_files: u64,
    pub verified_files: u64,
    pub verified_bytes: u64,
    pub uncertain_files: u64,
}
impl Execution {
    fn valid(&self) -> bool {
        self.selected_files > 0
            && self.selected_files <= 128
            && self.skipped_files <= 10_000
            && self.selected_bytes <= 16 * 1024 * 1024 * 1024
            && self.multipart_files <= self.selected_files
            && self.verified_files <= self.acknowledged_files
            && self.acknowledged_files <= self.attempted_files
            && self.attempted_files <= self.selected_files
            && self.uncertain_files <= self.attempted_files.saturating_sub(self.verified_files)
            && self.verified_bytes <= self.selected_bytes
            && (self.verified_files != 0 || self.verified_bytes == 0)
            && self.replacement_files <= self.selected_files
            && self.verified_replacements <= self.acknowledged_replacements
            && self.acknowledged_replacements <= self.attempted_replacements
            && self.attempted_replacements <= self.replacement_files
            && self.attempted_replacements <= self.attempted_files
            && self.acknowledged_replacements <= self.acknowledged_files
            && self.verified_replacements <= self.verified_files
            && self.uncertain_replacements <= self.uncertain_files
            && self.uncertain_replacements
                <= self.attempted_replacements - self.verified_replacements
            && self.acknowledged_files - self.acknowledged_replacements
                <= self.attempted_files - self.attempted_replacements
            && self.verified_files - self.verified_replacements
                <= self.acknowledged_files - self.acknowledged_replacements
            && self.verified_files - self.verified_replacements
                <= self.selected_files - self.replacement_files
            && self.uncertain_files - self.uncertain_replacements
                <= (self.attempted_files - self.attempted_replacements)
                    .saturating_sub(self.verified_files - self.verified_replacements)
    }
    fn follows(&self, old: &Self) -> bool {
        self.selected_files == old.selected_files
            && self.multipart_files == old.multipart_files
            && self.replacement_files == old.replacement_files
            && self.attempted_replacements >= old.attempted_replacements
            && self.acknowledged_replacements >= old.acknowledged_replacements
            && self.verified_replacements >= old.verified_replacements
            && self.uncertain_replacements >= old.uncertain_replacements
            && self.skipped_files == old.skipped_files
            && self.selected_bytes == old.selected_bytes
            && self.attempted_files >= old.attempted_files
            && self.acknowledged_files >= old.acknowledged_files
            && self.verified_files >= old.verified_files
            && self.verified_bytes >= old.verified_bytes
            && self.uncertain_files >= old.uncertain_files
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pause {
    pub run_id: String,
    pub reason: Status,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Store {
    #[serde(default)]
    uploads: Vec<Upload>,
    version: u32,
    revision: String,
    storage_review_required: bool,
    pauses: BTreeMap<String, Pause>,
    records: Vec<Record>,
}
impl Default for Store {
    fn default() -> Self {
        Self {
            version: 4,
            uploads: vec![],
            revision: "missing".into(),
            storage_review_required: false,
            pauses: BTreeMap::new(),
            records: vec![],
        }
    }
}
#[derive(Serialize)]
pub struct Report {
    pub pending_uploads: Vec<multipart::UploadNotice>,
    pub revision: String,
    pub storage_review_required: bool,
    pub reconciliation_deferred: bool,
    pub in_memory_storage_fault: bool,
    pub pauses: BTreeMap<String, Pause>,
    pub records: Vec<Record>,
    pub execution_available: bool,
}
impl Store {
    fn report(&self, dirty: bool) -> Report {
        Report {
            pending_uploads: self.uploads.iter().map(Upload::notice).collect(),
            revision: self.revision.clone(),
            storage_review_required: self.storage_review_required || dirty,
            reconciliation_deferred: false,
            in_memory_storage_fault: false,
            pauses: self.pauses.clone(),
            records: self.records.iter().rev().cloned().collect(),
            execution_available: false,
        }
    }
    fn validate(&self, persisted: bool) -> Result<(), FmError> {
        if !matches!(self.version, 1 | 2 | 3 | 4)
            || (persisted && !uuid(&self.revision))
            || self.records.len() > 501
            || self.pauses.len() > 500
        {
            return Err(error(
                "Unsupported or invalid saved-job journal schema/limits; it was not replaced",
            ));
        }
        let mut ids = HashSet::new();
        self.validate_uploads()?;
        let mut active = 0;
        let mut counts = BTreeMap::new();
        for record in &self.records {
            if record.multipart_cleanup_reviews
                > record
                    .execution
                    .as_ref()
                    .map_or(0, |counts| counts.multipart_files)
                || (self.version < 4 && record.multipart_cleanup_reviews != 0)
            {
                return Err(error("Invalid external multipart cleanup review audit"));
            }
            if self.version < 4
                && record.execution.as_ref().is_some_and(|counts| {
                    counts.multipart_files != 0 || counts.selected_bytes > 128 * 5 * 1024 * 1024
                })
            {
                return Err(error(
                    "An older journal cannot contain multipart execution intent",
                ));
            }
            if self.version < 3
                && record.execution.as_ref().is_some_and(|counts| {
                    counts.replacement_files != 0
                        || counts.attempted_replacements != 0
                        || counts.acknowledged_replacements != 0
                        || counts.verified_replacements != 0
                        || counts.uncertain_replacements != 0
                        || record.policy == Some(Policy::ReplaceUnchanged)
                })
            {
                return Err(error(
                    "An older journal cannot contain executable replacements",
                ));
            }
            if self.version == 1
                && (record.execution.is_some()
                    || matches!(
                        record.status,
                        Status::Reviewing
                            | Status::AwaitingApproval
                            | Status::Executing
                            | Status::Succeeded
                            | Status::Skipped
                            | Status::Partial
                            | Status::Uncertain
                    ))
            {
                return Err(error("A v1 journal cannot contain executable intents"));
            }
            if record.execution.as_ref().is_some_and(|e| !e.valid())
                || (record.execution.is_some()
                    && !matches!(
                        record.policy,
                        Some(Policy::CreateOnly | Policy::SkipExisting | Policy::ReplaceUnchanged)
                    ))
                || record.execution.as_ref().is_some_and(|counts| {
                    counts.replacement_files > 0 && record.policy != Some(Policy::ReplaceUnchanged)
                })
                || matches!(
                    record.status,
                    Status::Executing
                        | Status::Succeeded
                        | Status::Skipped
                        | Status::Partial
                        | Status::Uncertain
                ) && record.execution.is_none()
            {
                return Err(error("Invalid executable journal counters/intent"));
            }
            if matches!(record.status, Status::Succeeded | Status::Skipped)
                && record.execution.as_ref().is_none_or(|e| {
                    e.verified_files != e.selected_files
                        || e.verified_bytes != e.selected_bytes
                        || e.uncertain_files != 0
                        || (record.status == Status::Succeeded && e.skipped_files != 0)
                        || (record.status == Status::Skipped && e.skipped_files == 0)
                })
            {
                return Err(error("Incomplete writes cannot be recorded as successful"));
            }
            if !uuid(&record.run_id)
                || !uuid(&record.job_id)
                || !ids.insert(&record.run_id)
                || record.job_revision.len() > 20
                || !record.job_revision.parse::<u64>().is_ok_and(|revision| {
                    revision > 0 && revision.to_string() == record.job_revision
                })
                || record.updated_ms < record.started_ms
                || [
                    &record.binding,
                    &record.comparison,
                    &record.preparation,
                    &record.review,
                ]
                .iter()
                .any(|c| c.s3_bytes_read > c.bytes_read)
            {
                return Err(error(
                    "Invalid saved-job attempt record; journal was not replaced",
                ));
            }
            if record.status.active() {
                active += 1;
            } else {
                *counts.entry(&record.job_id).or_insert(0usize) += 1;
            }
        }
        if active > 1
            || self.records.len() - active > 500
            || counts.values().any(|count| *count > 50)
        {
            return Err(error("Saved-job journal retention/active limits exceeded"));
        }
        for (job, pause) in &self.pauses {
            if !pause.reason.pauses()
                || !self.records.iter().any(|r| {
                    &r.job_id == job && r.run_id == pause.run_id && r.status == pause.reason
                })
            {
                return Err(error(
                    "Invalid saved-job review pause; journal was not replaced",
                ));
            }
        }
        Ok(())
    }
    fn prune(&mut self, incoming_job: &str) -> Result<(), FmError> {
        loop {
            let per_job = self
                .records
                .iter()
                .filter(|r| r.job_id == incoming_job && !r.status.active())
                .count();
            let total = self.records.iter().filter(|r| !r.status.active()).count();
            if total < 500 && per_job < 50 {
                break;
            }
            let remove = self.records.iter().position(|r| {
                !r.status.active()
                    && (per_job < 50 || r.job_id == incoming_job)
                    && !self.pauses.values().any(|pause| pause.run_id == r.run_id)
                    && !self.uploads.iter().any(|upload| upload.run_id == r.run_id)
            });
            self.records.remove(remove.ok_or_else(||error("Job history is full of unresolved review records; acknowledge review before starting another attempt"))?);
        }
        Ok(())
    }
}

struct Lock(File);
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}
struct Storage {
    dir: PathBuf,
    directory: File,
}
impl Storage {
    fn open(paths: &Paths, create: bool) -> Result<Option<Self>, FmError> {
        let parent = paths.store.parent().ok_or_else(unavailable)?;
        let dir = parent.join("job-runs");
        match fs::symlink_metadata(&dir) {
            Err(failure) if failure.kind() == std::io::ErrorKind::NotFound && !create => {
                return Ok(None)
            }
            Err(failure) if failure.kind() == std::io::ErrorKind::NotFound => {
                fs::DirBuilder::new()
                    .mode(0o700)
                    .create(&dir)
                    .map_err(io_failure)?;
                durable(&File::open(parent).map_err(io_failure)?, true)?;
            }
            Err(_) => return Err(unavailable()),
            Ok(_) => {}
        }
        let directory = fs::OpenOptions::new()
            .read(true)
            .custom_flags(nix::libc::O_DIRECTORY | nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
            .open(&dir)
            .map_err(io_failure)?;
        let storage = Self { dir, directory };
        storage.check_dir()?;
        Ok(Some(storage))
    }
    fn check_dir(&self) -> Result<(), FmError> {
        let metadata = self.directory.metadata().map_err(|_| unavailable())?;
        let at_path = fs::symlink_metadata(&self.dir).map_err(|_| unavailable())?;
        if !at_path.is_dir()
            || metadata.dev() != at_path.dev()
            || metadata.ino() != at_path.ino()
            || metadata.uid() != nix::unistd::geteuid().as_raw()
            || metadata.mode() & 0o077 != 0
        {
            return Err(error("Job journal directory must be an unchanged, owner-only native directory (0700); symlinks/shared directories are refused"));
        }
        Ok(())
    }
    fn lock(&self) -> Result<Lock, FmError> {
        self.check_dir()?;
        let file = self.file("lock", true)?;
        let started = std::time::Instant::now();
        loop {
            match file.try_lock(){Ok(())=>return Ok(Lock(file)),Err(std::fs::TryLockError::WouldBlock) if started.elapsed()<std::time::Duration::from_secs(2)=>std::thread::sleep(std::time::Duration::from_millis(5)),Err(_)=>return Err(error("Job journal metadata lock is unavailable/busy after its bounded wait; refresh local recovery"))}
        }
    }
    fn file(&self, name: &str, create: bool) -> Result<File, FmError> {
        let file = fs::OpenOptions::new()
            .read(true)
            .write(create)
            .create(create)
            .truncate(false)
            .mode(0o600)
            .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
            .open(self.dir.join(name))
            .map_err(io_failure)?;
        let m = file.metadata().map_err(|_| unavailable())?;
        if !m.is_file()
            || m.nlink() != 1
            || m.uid() != nix::unistd::geteuid().as_raw()
            || m.mode() & 0o077 != 0
            || m.len() > MAX_BYTES
        {
            return Err(error("Job journal/lock/marker must be a private bounded regular file; links and special files are refused"));
        }
        Ok(file)
    }
    fn read(&self, name: &str) -> Result<Option<Vec<u8>>, FmError> {
        match fs::symlink_metadata(self.dir.join(name)) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(unavailable()),
            Ok(_) => {}
        }
        let mut bytes = vec![];
        self.file(name, false)?
            .take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| unavailable())?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(error("Job journal exceeds its 8 MiB byte limit"));
        }
        Ok(Some(bytes))
    }
    fn load(&self) -> Result<(Store, bool), FmError> {
        let dirty = match self.read("dirty")? {
            None => false,
            Some(bytes) if bytes == DIRTY => true,
            Some(_) => {
                return Err(error(
                    "Job journal recovery marker is invalid; journal work is blocked",
                ))
            }
        };
        let initialized = match self.read("initialized")? {
            None => false,
            Some(bytes) if bytes == INITIALIZED => true,
            Some(_) => {
                return Err(error(
                    "Job journal initialization marker is invalid; history was not replaced",
                ))
            }
        };
        let store=match self.read("journal.json")? {
            None if initialized=>return Err(error("A previously initialized job journal is missing. Restore/review its private storage; it was not silently treated as empty history")),
            None=>Store::default(),
            Some(_) if !initialized&&!dirty=>return Err(error("Job journal initialization evidence is missing; history was not replaced")),
            Some(bytes)=>{let store:Store=serde_json::from_slice(&bytes).map_err(|_|error("Job journal is corrupt or from an unsupported schema; no contents were included and it was not replaced"))?;store.validate(true)?;store}
        };
        Ok((store, dirty))
    }
    fn commit(&self, store: &mut Store) -> Result<(), FmError> {
        self.commit_with(store, &|_| Ok(()))
    }
    fn commit_with(
        &self,
        store: &mut Store,
        checkpoint: &impl Fn(&str) -> Result<(), FmError>,
    ) -> Result<(), FmError> {
        self.check_dir()?;
        store.version = 4;
        store.revision = fresh_id();
        store.validate(true)?;
        let bytes = serde_json::to_vec(store).map_err(|_| unavailable())?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(error(
                "Job history is at its 8 MiB budget; no journal update started",
            ));
        }
        // A durable conservative dirty marker survives any ambiguous finalization.
        // Recovery persists a storage-review pause before removing this marker.
        checkpoint("before_marker")?;
        let mut marker = self.file("dirty", true)?;
        if marker.metadata().map_err(|_| unavailable())?.len() == 0 {
            marker.write_all(DIRTY).map_err(|_| unavailable())?;
        }
        durable(&marker, false)?;
        durable(&self.directory, true)?;
        checkpoint("marker_durable")?;
        let mut temporary = tempfile::NamedTempFile::new_in(&self.dir).map_err(io_failure)?;
        temporary
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(io_failure)?;
        temporary.write_all(&bytes).map_err(io_failure)?;
        checkpoint("file_written")?;
        durable(temporary.as_file(), false)?;
        checkpoint("file_durable")?;
        self.check_dir()?;
        temporary
            .persist(self.dir.join("journal.json"))
            .map_err(|failure| io_failure(failure.error))?;
        checkpoint("renamed")?;
        durable(&self.directory, true)?;
        checkpoint("directory_durable")?;
        let mut initialized = self.file("initialized", true)?;
        if initialized.metadata().map_err(io_failure)?.len() == 0 {
            initialized.write_all(INITIALIZED).map_err(io_failure)?;
        }
        durable(&initialized, false)?;
        durable(&self.directory, true)?;
        checkpoint("initialized_durable")?;
        fs::remove_file(self.dir.join("dirty")).map_err(io_failure)?;
        checkpoint("marker_removed")?;
        durable(&self.directory, true)?;
        checkpoint("finished")?;
        Ok(())
    }
}
fn durable(file: &File, directory: bool) -> Result<(), FmError> {
    file.sync_all().map_err(io_failure)?;
    #[cfg(target_os = "macos")]
    if !directory {
        // fsync alone need not flush a macOS drive's volatile write cache.
        if unsafe { nix::libc::fcntl(file.as_raw_fd(), nix::libc::F_FULLFSYNC) } < 0 {
            return Err(io_failure(std::io::Error::last_os_error()));
        }
    }
    let _ = directory;
    Ok(())
}

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
pub(super) struct Attempt {
    paths: Paths,
    run: String,
    ended: AtomicBool,
    broken: AtomicBool,
    fault: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    serial: Mutex<()>,
    _lease: Arc<super::RunLock>,
}
impl Attempt {
    pub(super) fn start(
        paths: Paths,
        run: String,
        job: &super::Job,
        lease: Arc<super::RunLock>,
        fault: Arc<AtomicBool>,
        cancel: Arc<AtomicBool>,
    ) -> Result<Arc<Self>, FmError> {
        // A failed start cannot have sent keychain/network work. Conservatively
        // latch errors until explicit local recovery/review in this process.
        if let Err(failure) = start(&paths, &run, job) {
            fault.store(true, Ordering::Relaxed);
            return Err(failure);
        }
        Ok(Arc::new(Self {
            paths,
            run,
            ended: AtomicBool::new(false),
            broken: AtomicBool::new(false),
            fault,
            cancel,
            serial: Mutex::new(()),
            _lease: lease,
        }))
    }
    pub(super) fn stage(
        &self,
        next: Status,
        progress: ScanProgress,
        policy: Option<Policy>,
    ) -> Result<(), FmError> {
        self.update(next, Some(progress), policy, None)
    }
    pub(super) fn execution(&self, next: Status, counts: Execution) -> Result<(), FmError> {
        self.update(next, None, None, Some(counts))
    }
    pub(super) fn healthy(&self) -> bool {
        !self.ended.load(Ordering::Relaxed)
            && !self.broken.load(Ordering::Relaxed)
            && !self.fault.load(Ordering::Relaxed)
    }
    fn update(
        &self,
        next: Status,
        progress: Option<ScanProgress>,
        policy: Option<Policy>,
        execution: Option<Execution>,
    ) -> Result<(), FmError> {
        let _serial = self.serial.lock().map_err(|_| unavailable())?;
        if self.broken.load(Ordering::Relaxed) {
            return Err(error("Job outcome could not be recorded; journal recovery/review is required before any new job work"));
        }
        if self.ended.load(Ordering::Relaxed) {
            return if next.active() {
                Err(error(
                    "This journal attempt is already terminal; bind a fresh run",
                ))
            } else {
                Ok(())
            };
        }
        if next.active() && self.cancel.load(Ordering::Relaxed) {
            return Err(FmError::ScanCancelled);
        }
        if next.active() && self.fault.load(Ordering::Relaxed) {
            self.cancel.store(true, Ordering::Relaxed);
            return Err(error("Journal storage fault invalidated this run; explicit local recovery/review is required"));
        }
        match transition_with(
            &self.paths,
            &self.run,
            next,
            progress,
            policy,
            self.fault.load(Ordering::Relaxed),
            execution,
        ) {
            Ok(()) => {
                if !next.active() {
                    self.ended.store(true, Ordering::Relaxed);
                }
                Ok(())
            }
            Err(failure) => {
                self.broken.store(true, Ordering::Relaxed);
                self.fault.store(true, Ordering::Relaxed);
                self.cancel.store(true, Ordering::Relaxed);
                log::error!("Read-only job journal commit failed; local recovery/review is required before new job work");
                Err(failure)
            }
        }
    }
}
impl Drop for Attempt {
    fn drop(&mut self) {
        if !self.ended.load(Ordering::Relaxed) && !self.broken.load(Ordering::Relaxed) {
            if transition_with(
                &self.paths,
                &self.run,
                Status::Interrupted,
                None,
                None,
                self.fault.load(Ordering::Relaxed),
                None,
            )
            .is_err()
            {
                self.fault.store(true, Ordering::Relaxed);
                log::error!("An abandoned read-only job outcome could not be journaled; local recovery/review is required");
            }
        }
    }
}

pub(super) fn recover(paths: &Paths) -> Result<Report, FmError> {
    let Some(storage) = Storage::open(paths, false)? else {
        return Ok(Store::default().report(false));
    };
    let _lock = storage.lock()?;
    let (mut store, dirty) = storage.load()?;
    let mut changed = dirty;
    if dirty {
        store.storage_review_required = true;
    }
    for record in &mut store.records {
        if record.status.active() {
            record.status = Status::Interrupted;
            if let Some(counts) = &mut record.execution {
                counts.uncertain_replacements = counts
                    .uncertain_replacements
                    .max(counts.attempted_replacements - counts.verified_replacements);
                counts.uncertain_files = counts
                    .uncertain_files
                    .max(counts.attempted_files - counts.verified_files);
            }
            record.updated_ms = now().max(record.started_ms);
            store.pauses.insert(
                record.job_id.clone(),
                Pause {
                    run_id: record.run_id.clone(),
                    reason: Status::Interrupted,
                },
            );
            changed = true;
        }
    }
    if changed {
        storage.commit(&mut store)?;
    }
    Ok(store.report(false))
}
pub(super) fn read(paths: &Paths) -> Result<Report, FmError> {
    let Some(storage) = Storage::open(paths, false)? else {
        return Ok(Store::default().report(false));
    };
    let _lock = storage.lock()?;
    let (store, dirty) = storage.load()?;
    Ok(store.report(dirty))
}
pub(super) fn ensure_ready(paths: &Paths, job_id: &str) -> Result<(), FmError> {
    let report = read(paths)?;
    if report.storage_review_required
        || report.pauses.contains_key(job_id)
        || report.records.iter().any(|r| r.status.active())
    {
        return Err(error("Job is paused or has an unresolved attempt. Refresh journal history and explicitly acknowledge review before binding; no keychain read or network request was started"));
    }
    Ok(())
}
pub(super) fn start(paths: &Paths, run: &str, job: &super::Job) -> Result<(), FmError> {
    let storage = Storage::open(paths, true)?.ok_or_else(unavailable)?;
    let _lock = storage.lock()?;
    let (mut store, dirty) = storage.load()?;
    if dirty || store.storage_review_required || store.pauses.contains_key(&job.id) {
        return Err(error("Job is paused for explicit journal/recovery review. Refresh history, inspect the outcome and acknowledge review; no keychain read or network request was started"));
    }
    if store.records.iter().any(|r| r.status.active()) {
        return Err(error("An unreconciled attempt remains active in the journal; perform local recovery before binding"));
    }
    store.prune(&job.id)?;
    let time = now();
    store.records.push(Record {
        multipart_cleanup_reviews: 0,
        run_id: run.into(),
        job_id: job.id.clone(),
        job_revision: job.revision.clone(),
        started_ms: time,
        updated_ms: time,
        status: Status::Binding,
        binding: Counters::default(),
        comparison: Counters::default(),
        preparation: Counters::default(),
        policy: None,
        review: Counters::default(),
        execution: None,
    });
    storage.commit(&mut store)
}
#[cfg(test)]
pub(super) fn transition(
    paths: &Paths,
    run: &str,
    next: Status,
    progress: ScanProgress,
    policy: Option<Policy>,
) -> Result<(), FmError> {
    transition_with(paths, run, next, Some(progress), policy, false, None)
}
fn transition_with(
    paths: &Paths,
    run: &str,
    next: Status,
    progress: Option<ScanProgress>,
    policy: Option<Policy>,
    storage_fault: bool,
    execution: Option<Execution>,
) -> Result<(), FmError> {
    let storage = Storage::open(paths, false)?.ok_or_else(unavailable)?;
    let _lock = storage.lock()?;
    let (mut store, dirty) = storage.load()?;
    if dirty || store.storage_review_required {
        return Err(error("Job journal has an unresolved durability failure; discard the run and explicitly recover/review"));
    }
    let record = store
        .records
        .iter_mut()
        .find(|r| r.run_id == run)
        .ok_or_else(|| error("Job attempt is missing from its native journal"))?;
    if !record.status.active() {
        return if record.status == next || record.status == Status::NoChanges {
            Ok(())
        } else {
            Err(error(
                "Terminal job attempts cannot resume or change outcome",
            ))
        };
    }
    let valid = matches!(
        (record.status, next),
        (Status::Binding, Status::Bound)
            | (Status::Bound, Status::Scanning)
            | (Status::Scanning, Status::Compared | Status::NoChanges)
            | (Status::Compared, Status::Preparing)
            | (Status::Preparing, Status::Prepared)
            | (Status::Prepared, Status::Reviewing)
            | (Status::Reviewing, Status::AwaitingApproval)
            | (Status::AwaitingApproval, Status::Executing)
            | (
                Status::Executing,
                Status::Executing
                    | Status::Succeeded
                    | Status::Skipped
                    | Status::Partial
                    | Status::Uncertain
            )
    ) || matches!(
        next,
        Status::Discarded
            | Status::Cancelled
            | Status::Failed
            | Status::Limited
            | Status::Interrupted
    );
    if !valid {
        return Err(error("Invalid native journal stage transition"));
    }
    if let Some(counts) = execution {
        if !counts.valid()
            || record
                .execution
                .as_ref()
                .is_some_and(|old| !counts.follows(old))
            || (!matches!(
                next,
                Status::Executing
                    | Status::Succeeded
                    | Status::Skipped
                    | Status::Partial
                    | Status::Uncertain
                    | Status::Failed
                    | Status::Limited
                    | Status::Cancelled
                    | Status::Interrupted
            ))
        {
            return Err(error("Executable intent/counter ownership changed"));
        }
        if record.execution.is_none()
            && !(record.status == Status::AwaitingApproval
                && matches!(
                    next,
                    Status::Executing
                        | Status::Failed
                        | Status::Limited
                        | Status::Cancelled
                        | Status::Interrupted
                )
                && counts.attempted_files == 0)
        {
            return Err(error(
                "Executable intent must be durably started before mutation attempts",
            ));
        }
        record.execution = Some(counts);
    }
    if next == Status::Interrupted {
        if let Some(counts) = &mut record.execution {
            counts.uncertain_replacements = counts
                .uncertain_replacements
                .max(counts.attempted_replacements - counts.verified_replacements);
            counts.uncertain_files = counts
                .uncertain_files
                .max(counts.attempted_files - counts.verified_files);
        }
    }
    let category = if next.active() { next } else { record.status };
    if let Some(progress) = progress {
        let counters = progress.into();
        match category {
            Status::Binding | Status::Bound => record.binding = counters,
            Status::Scanning | Status::Compared => record.comparison = counters,
            Status::Reviewing | Status::AwaitingApproval => record.review = counters,
            _ => record.preparation = counters,
        }
    }
    if let Some(policy) = policy {
        if next != Status::Preparing {
            return Err(error(
                "Job policy can only be recorded at explicit preparation admission",
            ));
        }
        record.policy = Some(policy);
    }
    record.status = next;
    record.updated_ms = now().max(record.started_ms);
    if next.pauses() {
        store.pauses.insert(
            record.job_id.clone(),
            Pause {
                run_id: run.into(),
                reason: next,
            },
        );
    }
    if storage_fault {
        store.storage_review_required = true;
    }
    storage.commit(&mut store)
}
pub(super) fn review(
    paths: &Paths,
    revision: &str,
    job_id: Option<&str>,
    acknowledged: bool,
    clear_history: bool,
) -> Result<Report, FmError> {
    if !acknowledged {
        return Err(error("Explicit acknowledgement is required; review does not reconnect, retry, restore approval or execute"));
    }
    let storage = Storage::open(paths, true)?.ok_or_else(unavailable)?;
    let _lock = storage.lock()?;
    let (mut store, dirty) = storage.load()?;
    if store.revision != revision || dirty || store.records.iter().any(|r| r.status.active()) {
        return Err(error(
            "History changed or needs recovery; refresh before review/removal",
        ));
    }
    if clear_history {
        if job_id.is_some()
            || store.storage_review_required
            || !store.pauses.is_empty()
            || !store.uploads.is_empty()
        {
            return Err(error(
                "Unresolved review pauses cannot be hidden by removing history",
            ));
        }
        store.records.clear();
    } else if let Some(job) = job_id {
        if store.pauses.remove(job).is_none() {
            return Err(error(
                "Selected job no longer has this review pause; refresh history",
            ));
        }
    } else {
        store.storage_review_required = false;
    }
    storage.commit(&mut store)?;
    Ok(store.report(false))
}
