//! Manifest-owned, read-only preparation. No executable writer or approval token
//! is produced here; payloads remain private unlinked native file capabilities.
#[cfg(test)]
mod tests;
use super::{
    collect::{Entry, Manifest},
    error, fresh_id, job_sdk_error, outcome, Arc, AtomicBool, Binding, BoundSource, Endpoint, File,
    FmError, Instant, JobRunState, Phase, RunStatus, ScanBudget, ScanOutcome, ScanProgress,
    SourcePin, TaskGuard, MAX_ROWS,
};
use crate::s3::writes::DestinationExpectation;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::atomic::Ordering;
use std::{
    collections::HashSet,
    io::{Read, Write},
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::Path,
};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Policy {
    CreateOnly,
    SkipExisting,
    ReplaceUnchanged,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub run_id: String,
    pub manifest_id: String,
    pub entry_ids: Vec<String>,
    pub policy: Policy,
    pub max_snapshot_bytes: u64,
    pub approved: bool,
}

#[derive(Serialize)]
pub struct PreviewEntry {
    pub entry_id: String,
    pub relative_path: String,
    pub destination_key: String,
    pub size: u64,
    pub replaces: bool,
}
#[derive(Serialize)]
pub struct Preview {
    pub id: String,
    pub run_id: String,
    pub manifest_id: String,
    pub job_id: String,
    pub revision: String,
    pub policy: Policy,
    pub files: usize,
    pub skipped: usize,
    pub bytes: u64,
    pub max_snapshot_bytes: u64,
    pub expires_seconds: u64,
    pub relay: bool,
    pub entries: Vec<PreviewEntry>,
    pub entries_truncated: bool,
    pub verification_required: bool,
    pub execution_available: bool,
    pub can_review_execution: bool,
    pub execution_blocker: Option<String>,
}

pub(super) struct Snapshot {
    pub file: File,
    pub size: u64,
    pub sha256: String,
    pub attributes: super::attributes::Attributes,
}
pub(super) struct Item {
    pub entry: Entry,
    pub key: String,
    pub expected: DestinationExpectation,
    pub snapshot: Snapshot,
}
pub(super) struct Plan {
    pub id: String,
    run_id: String,
    manifest_id: String,
    job_id: String,
    revision: String,
    source_instance: Option<String>,
    destination_instance: String,
    expires: Instant,
    pub policy: Policy,
    pub skipped: usize,
    pub bytes: u64,
    destination_sse: bool,
    max_snapshot_bytes: u64,
    pub items: Vec<Item>,
}
impl Plan {
    pub(super) fn view(&self, expires: Instant) -> Preview {
        Preview {
            id: self.id.clone(),
            run_id: self.run_id.clone(),
            manifest_id: self.manifest_id.clone(),
            job_id: self.job_id.clone(),
            revision: self.revision.clone(),
            policy: self.policy,
            files: self.items.len(),
            skipped: self.skipped,
            bytes: self.bytes,
            max_snapshot_bytes: self.max_snapshot_bytes,
            expires_seconds: self
                .expires
                .min(expires)
                .saturating_duration_since(Instant::now())
                .as_secs(),
            relay: self.source_instance.is_some(),
            entries: self
                .items
                .iter()
                .map(|item| PreviewEntry {
                    entry_id: item.entry.row.id.clone(),
                    relative_path: item.entry.row.relative_path.clone(),
                    destination_key: item.key.clone(),
                    size: item.snapshot.size,
                    replaces: matches!(item.expected, DestinationExpectation::Match { .. }),
                })
                .collect(),
            entries_truncated: false,
            verification_required: true,
            execution_available: false,
            can_review_execution: self.execution_blocker().is_none(),
            execution_blocker: self.execution_blocker(),
        }
    }
    pub fn execution_blocker(&self) -> Option<String> {
        if self.source_instance.is_some() {
            return Some("Initial job execution supports local-file sources only. S3 source ACL/tag/encryption/storage preservation needs separate review; this preview remains read-only.".into());
        }
        if self.policy == Policy::ReplaceUnchanged
            || self
                .items
                .iter()
                .any(|item| !matches!(item.expected, DestinationExpectation::Missing))
        {
            return Some("Initial job execution creates missing objects only. Replacements remain non-executable; prepare a new create-only/skip-existing session.".into());
        }
        for item in &self.items {
            if item.snapshot.size > 5 * 1024 * 1024 {
                return Some("Initial job execution supports files up to 5 MiB only. Multipart job execution remains deferred; no weaker fallback is used.".into());
            }
            if item
                .key
                .split('/')
                .any(|segment| matches!(segment, "" | "." | ".."))
                || item
                    .key
                    .chars()
                    .any(|c| c == '%' || c == '\\' || c.is_control())
            {
                return Some("Initial job execution refuses leading/repeated slashes, dot segments, percent/backslash/control characters rather than normalizing destination keys.".into());
            }
            if let Some(reason) = &item.snapshot.attributes.unsupported {
                return Some(reason.clone());
            }
            if item.snapshot.attributes.customer_encrypted && !self.destination_sse {
                return Some("An SSE-C source requires an explicitly SSE-C-enabled job destination; no plaintext downgrade is authorized.".into());
            }
        }
        None
    }
    pub(super) fn check_owner(
        &self,
        binding: &Binding,
        run: &str,
        manifest: &str,
        expires: Instant,
    ) -> Result<(), FmError> {
        let instance = match &binding.source {
            BoundSource::S3(endpoint) => Some(endpoint.instance.as_str()),
            _ => None,
        };
        if self.run_id != run
            || self.manifest_id != manifest
            || self.job_id != binding.job.id
            || self.revision != binding.job.revision
            || self.source_instance.as_deref() != instance
            || self.destination_instance != binding.destination.instance
            || self.expires != expires
            || self.expires <= Instant::now()
        {
            return Err(error(
                "Job preview ownership or expiry changed; bind, compare and prepare again",
            ));
        }
        self.check_snapshots()
    }
    fn check_snapshots(&self) -> Result<(), FmError> {
        for item in &self.items {
            let source = item
                .entry
                .source
                .as_ref()
                .ok_or_else(|| error("Prepared source expectation missing"))?;
            let metadata = item
                .snapshot
                .file
                .metadata()
                .map_err(|_| error("Cannot inspect retained job snapshot"))?;
            if !metadata.is_file()
                || metadata.nlink() != 0
                || metadata.mode() & 0o777 != 0o600
                || metadata.len() != source.size
                || item.snapshot.size != source.size
                || item.snapshot.sha256 != source.sha256
            {
                return Err(error(
                    "Retained job snapshot does not match the native comparison",
                ));
            }
        }
        Ok(())
    }
}

fn select(
    binding: &Binding,
    manifest: &Manifest,
    request: &Request,
) -> Result<(Vec<ItemInput>, usize, u64), FmError> {
    if !request.approved {
        return Err(error("Explicit approval is required for checked snapshot preparation and additional read/download costs"));
    }
    if request.entry_ids.is_empty() || request.entry_ids.len() > MAX_ROWS {
        return Err(error("Select 1–10,000 native job entry IDs"));
    }
    if !(1..=16 * 1024 * 1024 * 1024).contains(&request.max_snapshot_bytes) {
        return Err(error("Snapshot disk budget must be 1 byte through 16 GiB"));
    }
    if !crate::s3::compat::conditional_writes_supported(&binding.destination.client) {
        return Err(error(crate::s3::compat::CONDITION_ERROR));
    }
    manifest.validate(matches!(binding.source, BoundSource::S3(_)))?;
    let mut ids = HashSet::new();
    for id in &request.entry_ids {
        if !ids.insert(id.as_str()) {
            return Err(error("Duplicate job entry IDs are not permitted"));
        }
    }
    let known: std::collections::HashMap<_, _> = manifest
        .entries
        .iter()
        .map(|entry| (entry.row.id.as_str(), entry))
        .collect();
    let mut inputs = vec![];
    let mut skipped = 0;
    let mut bytes = 0u64;
    let mut keys = HashSet::new();
    for id in &request.entry_ids {
        let entry = *known.get(id.as_str()).ok_or_else(|| {
            error("Unknown or cross-run job entry ID; only retained native entries can be prepared")
        })?;
        let source = entry.source.as_ref().ok_or_else(|| error("Destination-only entries are informational and cannot authorize job writes or deletion"))?;
        if entry.destination.is_some() {
            match request.policy { Policy::CreateOnly => return Err(error("Create-only preparation cannot include an existing destination; change the explicit selection/policy")), Policy::SkipExisting => { skipped += 1; continue; }, Policy::ReplaceUnchanged => {} }
        }
        let key = format!(
            "{}{}",
            binding.job.destination.prefix, entry.row.relative_path
        );
        if key.is_empty() || key.len() > 1024 || !keys.insert(key.clone()) {
            return Err(error("Native destination mapping is empty, oversized or colliding; no snapshots were retained"));
        }
        if matches!(binding.source, BoundSource::Local(_))
            && (entry
                .row
                .relative_path
                .split('/')
                .any(|part| matches!(part, "" | "." | ".."))
                || Path::new(&entry.row.relative_path).is_absolute())
        {
            return Err(error("Invalid capability-relative local job path"));
        }
        bytes = bytes
            .checked_add(source.size)
            .ok_or_else(|| error("Selected snapshot byte count overflow"))?;
        if bytes > request.max_snapshot_bytes {
            return Err(FmError::ScanLimited("Selected source bytes exceed the explicit snapshot disk budget; select fewer entries or review a larger budget".into()));
        }
        let expected = match &entry.destination {
            Some(file) => DestinationExpectation::Match {
                etag: file
                    .etag
                    .clone()
                    .ok_or_else(|| error("Missing native destination validator"))?,
            },
            None => DestinationExpectation::Missing,
        };
        inputs.push(ItemInput {
            entry: entry.clone(),
            key,
            expected,
        });
    }
    if inputs.is_empty() {
        return Err(error(
            "All selected entries would be skipped; no payload preview or snapshots were created",
        ));
    }
    if inputs.len() > 128 {
        return Err(error(
            "A job preview retains at most 128 file capabilities; select fewer actionable entries",
        ));
    }
    Ok((inputs, skipped, bytes))
}
struct ItemInput {
    entry: Entry,
    key: String,
    expected: DestinationExpectation,
}

async fn check_destination(
    endpoint: &Endpoint,
    input: &ItemInput,
    cancel: &AtomicBool,
    budget: &ScanBudget,
    progress: &(dyn Fn(ScanProgress) + Send + Sync),
) -> Result<(), FmError> {
    use aws_sdk_s3::error::ProvideErrorMetadata;
    let head = budget
        .request(cancel, async {
            match endpoint
                .client
                .head_object()
                .bucket(&endpoint.bucket)
                .key(&input.key)
                .send()
                .await
            {
                Ok(head) => Ok(Some(head)),
                Err(failure)
                    if failure
                        .raw_response()
                        .is_some_and(|response| response.status().as_u16() == 404)
                        && matches!(
                            failure
                                .as_service_error()
                                .and_then(|failure| failure.code()),
                            Some("NotFound" | "NoSuchKey")
                        ) =>
                {
                    Ok(None)
                }
                Err(failure) => Err(job_sdk_error(
                    "Job preview destination HeadObject",
                    &failure,
                )),
            }
        })
        .await?;
    match (&input.expected, head) {
        (DestinationExpectation::Missing, None) => Ok(()),
        (DestinationExpectation::Match { etag }, Some(head)) => {
            let expected = input
                .entry
                .destination
                .as_ref()
                .ok_or_else(|| error("Native destination expectation missing"))?;
            if head.e_tag() != Some(etag.as_str())
                || head
                    .content_length()
                    .and_then(|size| u64::try_from(size).ok())
                    != Some(expected.size)
            {
                return Err(error(
                    "Destination changed since comparison; no usable job preview was retained",
                ));
            }
            let digest = super::collect::hash_s3(
                endpoint,
                &input.key,
                expected.size,
                etag,
                cancel,
                budget,
                progress,
            )
            .await?;
            if digest != expected.sha256 {
                return Err(error(
                    "Destination content no longer matches the native comparison; compare again",
                ));
            }
            Ok(())
        }
        _ => Err(error(
            "Destination existence changed since comparison; compare and prepare again",
        )),
    }
}

fn digest_hex(hash: Sha256) -> String {
    hash.finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn private_snapshot() -> Result<File, FmError> {
    let file = tempfile::tempfile().map_err(|_| error("Cannot allocate private job snapshot; check temporary storage permissions/free space and open-file limits"))?;
    // Linux O_TMPFILE may use 0666 masked by umask, unlike named temp files.
    // Enforce the same owner-only contract on every platform before any payload.
    file.set_permissions(std::fs::Permissions::from_mode(0o600))
        .map_err(|_| {
            error("Cannot enforce owner-only job snapshot permissions; no payload was staged")
        })?;
    Ok(file)
}
fn local_snapshot(
    root: &File,
    input: &ItemInput,
    cancel: &AtomicBool,
    budget: &ScanBudget,
    progress: &(dyn Fn(ScanProgress) + Send + Sync),
) -> Result<Snapshot, FmError> {
    let expected = input
        .entry
        .source
        .as_ref()
        .ok_or_else(|| error("Missing local source expectation"))?;
    let mut source = crate::s3::write_local::relative_open(
        root,
        Path::new(&input.entry.row.relative_path),
        false,
    )
    .map_err(|_| {
        error("Cannot open the compared local source through its retained no-follow root")
    })?;
    let before = source
        .metadata()
        .map_err(|_| error("Cannot inspect local source before snapshotting"))?;
    if !before.is_file() || before.len() != expected.size {
        return Err(error("Local source type/size changed since comparison"));
    }
    let mut output = private_snapshot()?;
    let mut hash = Sha256::new();
    let mut bytes = 0u64;
    let mut buffer = [0; 65536];
    loop {
        budget.check(cancel)?;
        let count = source
            .read(&mut buffer)
            .map_err(|_| error("Local source read failed during checked snapshot preparation"))?;
        if count == 0 {
            break;
        }
        budget.bytes(count as u64, false);
        if budget.progress().bytes_read % (1024 * 1024) < count as u64 {
            progress(budget.progress());
        }
        budget.check(cancel)?;
        bytes += count as u64;
        if bytes > expected.size {
            return Err(error(
                "Local source grew during snapshotting; disk admission was not exceeded",
            ));
        }
        output.write_all(&buffer[..count]).map_err(|_| {
            error("Cannot write private job snapshot; check available temporary disk space")
        })?;
        hash.update(&buffer[..count]);
    }
    let after = source
        .metadata()
        .map_err(|_| error("Cannot inspect local source after snapshotting"))?;
    let digest = digest_hex(hash);
    if bytes != expected.size
        || digest != expected.sha256
        || before.len() != after.len()
        || before.modified().ok() != after.modified().ok()
        || before.ctime() != after.ctime()
        || before.ctime_nsec() != after.ctime_nsec()
    {
        return Err(error("Local source changed since comparison or during snapshotting; no usable preview was retained"));
    }
    budget.check(cancel)?;
    Ok(Snapshot {
        file: output,
        size: bytes,
        sha256: digest,
        attributes: super::attributes::Attributes::local(&input.key),
    })
}

async fn s3_snapshot(
    binding: &Arc<Binding>,
    endpoint: &Endpoint,
    prefix: &str,
    input: &ItemInput,
    cancel: &AtomicBool,
    budget: &ScanBudget,
    progress: &(dyn Fn(ScanProgress) + Send + Sync),
) -> Result<Snapshot, FmError> {
    let expected = input
        .entry
        .source
        .as_ref()
        .ok_or_else(|| error("Missing S3 source expectation"))?;
    let etag = expected
        .etag
        .as_deref()
        .ok_or_else(|| error("Missing source conditional validator"))?;
    let key = format!("{prefix}{}", input.entry.row.relative_path);
    if key.len() > 1024 {
        return Err(error("Source key exceeds the native S3 key limit"));
    }
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
                .map_err(|failure| job_sdk_error("Job checked source GetObject", &failure))
        })
        .await?;
    if response.e_tag() != Some(etag)
        || response
            .content_length()
            .and_then(|size| u64::try_from(size).ok())
            != Some(expected.size)
    {
        return Err(error(
            "Source GET does not match the compared opaque ETag/size",
        ));
    }
    let attributes = super::attributes::Attributes::capture(&response);
    let mut output = private_snapshot()?;
    let mut body = response.body;
    let mut hash = Sha256::new();
    let mut bytes = 0u64;
    loop {
        let chunk = budget.wait(cancel, async { body.try_next().await.map_err(|_| error("S3 snapshot body stream failed; no response payload was included in diagnostics")) }).await?;
        let Some(chunk) = chunk else {
            break;
        };
        let count = chunk.len() as u64;
        budget.bytes(count, true);
        if budget.progress().bytes_read % (1024 * 1024) < count {
            progress(budget.progress());
        }
        budget.check(cancel)?;
        bytes = bytes
            .checked_add(chunk.len() as u64)
            .ok_or_else(|| error("Source snapshot byte count overflow"))?;
        if bytes > expected.size {
            return Err(error(
                "Source body exceeded its compared size; snapshot disk admission was not exceeded",
            ));
        }
        hash.update(&chunk);
        let lease = binding.clone();
        // The blocking worker owns both the unlinked file and the run lease.
        // Dropping/cancelling the async task cannot release its lock mid-write.
        output = tokio::task::spawn_blocking(move || {
            let _lease = lease;
            output.write_all(&chunk).map_err(|_| {
                error("Cannot write private job snapshot; check temporary disk space")
            })?;
            Ok::<_, FmError>(output)
        })
        .await
        .map_err(|_| error("Native S3 snapshot disk worker failed"))??;
        budget.check(cancel)?;
    }
    let digest = digest_hex(hash);
    if bytes != expected.size || digest != expected.sha256 {
        return Err(error("S3 source content changed since comparison or the body was incomplete; no usable preview was retained"));
    }
    Ok(Snapshot {
        file: output,
        size: bytes,
        sha256: digest,
        attributes,
    })
}

pub(crate) async fn prepare(
    state: &JobRunState,
    request: Request,
    notify: Arc<dyn Fn(ScanProgress) + Send + Sync>,
) -> Result<RunStatus, FmError> {
    // Invalid selections/approval do not consume a complete comparison and cause
    // no I/O. Once admitted, preparation is single-shot and any failure releases it.
    let status = state.status(&request.run_id)?;
    if status.phase != "complete"
        || status.manifest_id.as_deref() != Some(request.manifest_id.as_str())
    {
        return Err(error(
            "A matching fresh complete native manifest is required for preparation",
        ));
    }
    let (binding, inputs, skipped, bytes, expires, cancel) = {
        let mut inner = state
            .0
            .lock()
            .map_err(|_| error("Job session state unavailable"))?;
        let slot = inner
            .slot
            .as_mut()
            .filter(|slot| {
                slot.id == request.run_id
                    && slot.phase == Phase::Complete
                    && slot.expires > Instant::now()
                    && !slot.cancel.load(Ordering::Relaxed)
            })
            .ok_or_else(|| error("Job comparison changed or preparation already started"))?;
        let binding = slot
            .binding
            .clone()
            .ok_or_else(|| error("Missing native binding"))?;
        let manifest = slot
            .manifest
            .as_ref()
            .filter(|manifest| manifest.id == request.manifest_id)
            .ok_or_else(|| error("Manifest ownership changed"))?;
        let (inputs, skipped, bytes) = select(&binding, manifest, &request)?;
        slot.phase = Phase::Preparing;
        slot.progress = ScanProgress::default();
        slot.outcome = None;
        (
            binding,
            inputs,
            skipped,
            bytes,
            slot.expires,
            slot.cancel.clone(),
        )
    };
    let mut guard = TaskGuard {
        state: state.clone(),
        id: request.run_id.clone(),
        finished: false,
    };
    let budget = Arc::new(ScanBudget::with_limits(binding.limits));
    binding.attempt.stage(
        super::journal::Status::Preparing,
        ScanProgress::default(),
        Some(request.policy),
    )?;
    let progress_state = state.clone();
    let progress_id = request.run_id.clone();
    let progress: Arc<dyn Fn(ScanProgress) + Send + Sync> = Arc::new(move |event| {
        if let Ok(mut inner) = progress_state.0.lock() {
            if let Some(slot) = inner
                .slot
                .as_mut()
                .filter(|slot| slot.id == progress_id && slot.phase == Phase::Preparing)
            {
                slot.progress = event;
            }
        }
        notify(event);
    });
    let result = async {
        binding.validate()?;
        budget.check(&cancel)?;
        // Check every destination before allocating/downloading any source snapshots.
        for input in &inputs {
            budget.object(&cancel)?;
            check_destination(
                &binding.destination,
                input,
                &cancel,
                &budget,
                progress.as_ref(),
            )
            .await?;
            progress(budget.progress());
        }
        let mut items = vec![];
        for input in inputs {
            budget.check(&cancel)?;
            binding.validate()?;
            let snapshot = match &binding.source {
                BoundSource::Local(root) => {
                    let root = root.clone();
                    let task_input = input.entry.clone();
                    let task_key = input.key.clone();
                    let task_expected = input.expected.clone();
                    let task_cancel = cancel.clone();
                    let task_budget = budget.clone();
                    let task_progress = progress.clone();
                    let lease = binding.clone();
                    tokio::task::spawn_blocking(move || {
                        let _lease = lease;
                        local_snapshot(
                            &root.directory,
                            &ItemInput {
                                entry: task_input,
                                key: task_key,
                                expected: task_expected,
                            },
                            &task_cancel,
                            &task_budget,
                            task_progress.as_ref(),
                        )
                    })
                    .await
                    .map_err(|_| error("Native local snapshot worker failed"))??
                }
                BoundSource::S3(endpoint) => {
                    let SourcePin::S3 { pin } = &binding.job.source else {
                        return Err(error("Source binding kind changed"));
                    };
                    s3_snapshot(
                        &binding,
                        endpoint,
                        &pin.prefix,
                        &input,
                        &cancel,
                        &budget,
                        progress.as_ref(),
                    )
                    .await?
                }
            };
            items.push(Item {
                entry: input.entry,
                key: input.key,
                expected: input.expected,
                snapshot,
            });
            progress(budget.progress());
        }
        binding.validate()?;
        budget.check(&cancel)?;
        let plan = Plan {
            id: fresh_id(),
            run_id: request.run_id.clone(),
            manifest_id: request.manifest_id.clone(),
            job_id: binding.job.id.clone(),
            revision: binding.job.revision.clone(),
            source_instance: match &binding.source {
                BoundSource::S3(endpoint) => Some(endpoint.instance.clone()),
                _ => None,
            },
            destination_instance: binding.destination.instance.clone(),
            expires,
            policy: request.policy,
            destination_sse: binding
                .job
                .destination
                .configuration
                .sse_customer_key_enabled,
            skipped,
            bytes,
            max_snapshot_bytes: request.max_snapshot_bytes,
            items,
        };
        plan.check_owner(&binding, &request.run_id, &request.manifest_id, expires)?;
        plan.check_snapshots()?;
        budget.check(&cancel)?;
        Ok::<_, FmError>(plan)
    }
    .await;
    match result {
        Ok(plan) => {
            binding
                .attempt
                .stage(super::journal::Status::Prepared, budget.progress(), None)?;
            if let Err(failure) = budget.check(&cancel) {
                let result = state.stop(&request.run_id, outcome(&failure), failure.to_string());
                guard.finished = true;
                return result;
            }
            let mut inner = state
                .0
                .lock()
                .map_err(|_| error("Job session state unavailable"))?;
            let slot = inner
                .slot
                .as_mut()
                .filter(|slot| {
                    slot.id == request.run_id
                        && slot.phase == Phase::Preparing
                        && slot.expires == expires
                        && expires > Instant::now()
                        && !slot.cancel.load(Ordering::Relaxed)
                })
                .ok_or(FmError::ScanCancelled)?;
            slot.preview = Some(plan);
            slot.phase = Phase::Prepared;
            slot.progress = budget.progress();
            slot.outcome = Some(ScanOutcome::Complete);
            let view = slot.view();
            guard.finished = true;
            Ok(view)
        }
        Err(failure) => {
            if let Ok(mut inner) = state.0.lock() {
                if let Some(slot) = inner.slot.as_mut().filter(|slot| slot.id == request.run_id) {
                    slot.progress = budget.progress();
                }
            }
            let view = state.stop(&request.run_id, outcome(&failure), failure.to_string())?;
            guard.finished = true;
            Ok(view)
        }
    }
}
