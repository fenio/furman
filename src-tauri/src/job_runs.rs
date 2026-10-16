//! Explicit native job sessions. Private read clients, manifests and scoped
//! approved writers never enter legacy connection/transfer registries.
mod attributes;
pub(crate) mod collect;
mod controls;
pub(crate) mod execute;
pub(crate) mod journal;
pub(crate) mod preview;
#[cfg(test)]
mod tests;

use crate::{
    jobs::{self, Job, Paths, ProfileConfiguration, S3Pin, SourcePin},
    models::FmError,
    s3::{
        errors::s3_sdk_error,
        scan::{ScanBudget, ScanLimits, ScanOutcome, ScanProgress},
    },
};
use aws_sdk_s3::{
    config::{retry::RetryConfig, Credentials, Region, RequestChecksumCalculation},
    Client,
};
use serde::Serialize;
use std::{
    collections::VecDeque,
    fs::File,
    os::unix::fs::OpenOptionsExt,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

const TTL: Duration = Duration::from_secs(900);
const MAX_ROWS: usize = 10_000;
const NAMESPACE_ERROR: &str = "[JOB_NAMESPACE_MISMATCH] Native job request resolved outside the pinned origin; transmission was blocked";
const READ_ONLY_ERROR: &str =
    "[JOB_READ_ONLY] Native job comparison clients cannot perform this operation";
fn error(message: &str) -> FmError {
    FmError::Other(message.into())
}
fn fresh_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

struct Endpoint {
    client: Client,
    instance: String,
    namespace: String,
    bucket: String,
}
struct LocalRoot {
    directory: File,
    pin: SourcePin,
}
enum BoundSource {
    Local(Arc<LocalRoot>),
    S3(Endpoint),
}
struct Binding {
    paths: Paths,
    job: Job,
    source: BoundSource,
    destination: Endpoint,
    limits: ScanLimits,
    global_limits: ScanLimits,
    bind_progress: ScanProgress,
    _run_lock: Arc<RunLock>,
    attempt: Arc<journal::Attempt>,
}
impl Binding {
    fn validate(&self) -> Result<(), FmError> {
        jobs::validate_binding(&self.paths, &self.job)?;
        if ScanBudget::new().limits != self.global_limits {
            return Err(error(
                "Scan limits changed; bind a fresh job run with the reviewed budgets",
            ));
        }
        if let BoundSource::Local(root) = &self.source {
            if jobs::open_local_root(match &root.pin {
                SourcePin::Local { path, .. } => path,
                _ => unreachable!(),
            })?
            .1 != root.pin
            {
                return Err(error(
                    "Bound local root no longer occupies its reviewed path",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Phase {
    Binding,
    Bound,
    Scanning,
    Complete,
    Preparing,
    Prepared,
    Reviewing,
    AwaitingApproval,
    Executing,
    Stopped,
}

struct Slot {
    id: String,
    op_id: String,
    phase: Phase,
    cancel: Arc<AtomicBool>,
    expires: Instant,
    binding: Option<Arc<Binding>>,
    manifest: Option<collect::Manifest>,
    preview: Option<preview::Plan>,
    attempt: Option<Arc<journal::Attempt>>,
    approval: Option<execute::Approval>,
    execution: Option<execute::ExecutionResult>,
    outcome: Option<ScanOutcome>,
    message: Option<String>,
    progress: ScanProgress,
    expiry_abort: Option<tokio::task::AbortHandle>,
}
impl Drop for Slot {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        if let Some(timer) = self.expiry_abort.take() {
            timer.abort();
        }
    }
}
#[derive(Default)]
struct Inner {
    slot: Option<Slot>,
    used: VecDeque<String>,
    journal_fault: Arc<AtomicBool>,
}
#[derive(Clone, Default)]
pub struct JobRunState(Arc<Mutex<Inner>>);

#[derive(Serialize)]
pub struct RunStatus {
    pub run_id: String,
    pub job_id: Option<String>,
    pub phase: &'static str,
    pub expires_seconds: u64,
    pub outcome: Option<ScanOutcome>,
    pub message: Option<String>,
    pub progress: ScanProgress,
    pub summary: Option<collect::Summary>,
    pub manifest_id: Option<String>,
    pub source_instance: Option<String>,
    pub destination_instance: Option<String>,
    pub source_namespace: Option<String>,
    pub destination_namespace: Option<String>,
    pub limits: Option<ScanLimits>,
    pub execution_available: bool,
    pub preview: Option<preview::Preview>,
    pub execution_review: Option<execute::Review>,
    pub execution: Option<execute::ExecutionResult>,
}

impl Slot {
    fn arm_expiry(&mut self, state: &JobRunState) {
        if let Some(timer) = self.expiry_abort.take() {
            timer.abort();
        }
        let state = Arc::downgrade(&state.0);
        let id = self.id.clone();
        let remaining = self.expires.saturating_duration_since(Instant::now());
        let task = tokio::spawn(async move {
            tokio::time::sleep(remaining).await;
            if let Some(inner) = state.upgrade() {
                let _ = JobRunState(inner).status(&id);
            }
        });
        self.expiry_abort = Some(task.abort_handle());
    }
    fn view(&self) -> RunStatus {
        RunStatus {
            run_id: self.id.clone(),
            job_id: self.binding.as_ref().map(|binding| binding.job.id.clone()),
            phase: match self.phase {
                Phase::Binding => "binding",
                Phase::Bound => "bound",
                Phase::Scanning => "scanning",
                Phase::Complete => "complete",
                Phase::Preparing => "preparing",
                Phase::Prepared => "prepared",
                Phase::Reviewing => "reviewing_execution",
                Phase::AwaitingApproval => "awaiting_execution_approval",
                Phase::Executing => "executing",
                Phase::Stopped => "stopped",
            },
            expires_seconds: if self.phase == Phase::Stopped {
                0
            } else {
                self.expires
                    .saturating_duration_since(Instant::now())
                    .as_secs()
            },
            outcome: self.outcome,
            message: self.message.clone(),
            progress: self.progress,
            summary: self
                .manifest
                .as_ref()
                .map(|manifest| manifest.summary.clone()),
            manifest_id: self.manifest.as_ref().map(|manifest| manifest.id.clone()),
            source_instance: self
                .binding
                .as_ref()
                .and_then(|binding| match &binding.source {
                    BoundSource::S3(endpoint) => Some(endpoint.instance.clone()),
                    _ => None,
                }),
            destination_instance: self
                .binding
                .as_ref()
                .map(|binding| binding.destination.instance.clone()),
            source_namespace: self
                .binding
                .as_ref()
                .and_then(|binding| match &binding.source {
                    BoundSource::S3(endpoint) => Some(endpoint.namespace.clone()),
                    _ => None,
                }),
            destination_namespace: self
                .binding
                .as_ref()
                .map(|binding| binding.destination.namespace.clone()),
            limits: self.binding.as_ref().map(|binding| binding.limits),
            execution_available: self.phase == Phase::AwaitingApproval
                && self.approval.is_some()
                && self
                    .attempt
                    .as_ref()
                    .is_some_and(|attempt| attempt.healthy())
                && !self.cancel.load(Ordering::Relaxed)
                && self.expires > Instant::now(),
            preview: self.preview.as_ref().map(|plan| plan.view(self.expires)),
            execution_review: self
                .approval
                .as_ref()
                .map(|approval| approval.view(self.expires)),
            execution: self.execution.clone(),
        }
    }
}

struct TaskGuard {
    state: JobRunState,
    id: String,
    finished: bool,
}
impl Drop for TaskGuard {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.state.stop_as(
                &self.id,
                ScanOutcome::Cancelled,
                "Job read-only task ended before completion; no manifest was retained".into(),
                Some(journal::Status::Interrupted),
            );
        }
    }
}

impl JobRunState {
    fn journal_fault(&self) -> Result<Arc<AtomicBool>, FmError> {
        Ok(self
            .0
            .lock()
            .map_err(|_| error("Job session state unavailable"))?
            .journal_fault
            .clone())
    }
    pub fn history(&self, paths: &Paths) -> Result<journal::Report, FmError> {
        let fault = self.journal_fault()?;
        let result = match run_lock(paths) {
            Ok(_lease) => journal::recover(paths),
            Err(_) => journal::read(paths).map(|mut report| {
                report.reconciliation_deferred = true;
                report
            }),
        };
        match result {
            Ok(mut report) => {
                report.in_memory_storage_fault = fault.load(Ordering::Relaxed);
                Ok(report)
            }
            Err(failure) => {
                fault.store(true, Ordering::Relaxed);
                let idle = {
                    let mut inner = self
                        .0
                        .lock()
                        .map_err(|_| error("Job session state unavailable"))?;
                    inner.slot.as_mut().and_then(|slot| {
                        if slot.phase == Phase::Stopped {
                            return None;
                        }
                        slot.cancel.store(true, Ordering::Relaxed);
                        slot.manifest = None;
                        slot.preview = None;
                        slot.approval = None;
                        if matches!(
                            slot.phase,
                            Phase::Bound
                                | Phase::Complete
                                | Phase::Prepared
                                | Phase::AwaitingApproval
                        ) {
                            Some(slot.id.clone())
                        } else {
                            None
                        }
                    })
                };
                if let Some(id) = idle {
                    let _=self.stop(&id,ScanOutcome::Failed,"Journal history/storage failure invalidated this run; explicitly recover/review".into());
                }
                Err(failure)
            }
        }
    }
    pub fn review_history(
        &self,
        paths: &Paths,
        revision: &str,
        job_id: Option<&str>,
        acknowledged: bool,
        clear_history: bool,
    ) -> Result<journal::Report, FmError> {
        if !acknowledged {
            return Err(error(
                "Explicit acknowledgement is required for journal review/removal",
            ));
        }
        if clear_history && job_id.is_some() {
            return Err(error("History removal cannot also acknowledge a job pause"));
        }
        let _lease = run_lock(paths)?;
        let fault = self.journal_fault()?;
        let before = journal::read(paths).map_err(|failure| {
            fault.store(true, Ordering::Relaxed);
            failure
        })?;
        if before.revision != revision {
            return Err(error(
                "History changed; refresh before acknowledging review/removal",
            ));
        }
        if job_id.is_some_and(|job| !before.pauses.contains_key(job)) {
            return Err(error(
                "Selected job no longer has this pause; refresh history",
            ));
        }
        if clear_history
            && (fault.load(Ordering::Relaxed)
                || before.storage_review_required
                || !before.pauses.is_empty())
        {
            return Err(error(
                "Unresolved review/storage failures cannot be hidden by removing history",
            ));
        }
        let mut report = journal::review(paths, revision, job_id, acknowledged, clear_history)
            .map_err(|failure| {
                fault.store(true, Ordering::Relaxed);
                failure
            })?;
        if job_id.is_none() && !clear_history {
            self.journal_fault()?.store(false, Ordering::Relaxed);
        }
        report.in_memory_storage_fault = self.journal_fault()?.load(Ordering::Relaxed);
        Ok(report)
    }
    fn attach_attempt(&self, id: &str, attempt: Arc<journal::Attempt>) -> Result<(), FmError> {
        let mut inner = self
            .0
            .lock()
            .map_err(|_| error("Job session state unavailable"))?;
        let slot = inner
            .slot
            .as_mut()
            .filter(|slot| {
                slot.id == id
                    && slot.phase == Phase::Binding
                    && !slot.cancel.load(Ordering::Relaxed)
            })
            .ok_or(FmError::ScanCancelled)?;
        slot.attempt = Some(attempt);
        Ok(())
    }
    fn begin(&self, op_id: &str) -> Result<(String, Arc<AtomicBool>, TaskGuard), FmError> {
        let canonical_op = uuid::Uuid::parse_str(op_id)
            .map_err(|_| error("Invalid job binding operation ID"))?
            .to_string();
        let op_id = canonical_op.as_str();
        let mut inner = self
            .0
            .lock()
            .map_err(|_| error("Job session state unavailable"))?;
        if inner.journal_fault.load(Ordering::Relaxed) {
            return Err(error("[JOB_JOURNAL] Job journal requires explicit local recovery/review before another run. Ordinary workflows remain available."));
        }
        if inner.used.iter().any(|id| id == op_id) {
            return Err(error("This job binding operation was already used or cancelled; explicitly start a new attempt"));
        }
        if inner
            .slot
            .as_ref()
            .is_some_and(|slot| slot.phase != Phase::Stopped)
        {
            return Err(error(
                "A job session is already active. Release it before binding another.",
            ));
        }
        inner.used.push_back(op_id.into());
        while inner.used.len() > 128 {
            inner.used.pop_front();
        }
        let id = fresh_id();
        let cancel = Arc::new(AtomicBool::new(false));
        inner.slot = Some(Slot {
            id: id.clone(),
            op_id: op_id.into(),
            phase: Phase::Binding,
            cancel: cancel.clone(),
            expires: Instant::now() + TTL,
            binding: None,
            manifest: None,
            preview: None,
            attempt: None,
            approval: None,
            execution: None,
            outcome: None,
            message: None,
            progress: ScanProgress::default(),
            expiry_abort: None,
        });
        Ok((
            id.clone(),
            cancel,
            TaskGuard {
                state: self.clone(),
                id,
                finished: false,
            },
        ))
    }
    fn stop(&self, id: &str, outcome: ScanOutcome, message: String) -> Result<RunStatus, FmError> {
        self.stop_as(id, outcome, message, None)
    }
    fn stop_as(
        &self,
        id: &str,
        outcome: ScanOutcome,
        mut message: String,
        terminal: Option<journal::Status>,
    ) -> Result<RunStatus, FmError> {
        let mut inner = self
            .0
            .lock()
            .map_err(|_| error("Job session state unavailable"))?;
        let slot = inner
            .slot
            .as_mut()
            .filter(|slot| slot.id == id)
            .ok_or_else(|| error("Job session is no longer active"))?;
        if slot.phase == Phase::Stopped {
            return Ok(slot.view());
        }
        if slot.phase == Phase::Executing {
            slot.cancel.store(true, Ordering::Relaxed);
            slot.message=Some(format!("Cancellation requested: {message}. Already-sent writes may have committed; the native task retains outcome ownership."));
            return Ok(slot.view());
        }
        if let Some(timer) = slot.expiry_abort.take() {
            timer.abort();
        }
        slot.cancel.store(true, Ordering::Relaxed);
        let mut journal_failure = None;
        if let Some(attempt) = &slot.attempt {
            let terminal = terminal.unwrap_or(match outcome {
                ScanOutcome::Failed => journal::Status::Failed,
                ScanOutcome::Limited => journal::Status::Limited,
                _ => journal::Status::Cancelled,
            });
            if let Err(failure) = attempt.stage(terminal, slot.progress, None) {
                message=format!("{message}\nOutcome could not be recorded. No further job writes are authorized; recover/review journal storage before new job work.\n{failure}");
                journal_failure = Some(failure);
            }
        }
        slot.phase = Phase::Stopped;
        slot.manifest = None;
        slot.preview = None;
        slot.approval = None;
        slot.binding = None;
        slot.attempt = None;
        slot.outcome = Some(outcome);
        slot.message = Some(message);
        let view = slot.view();
        if journal_failure.is_some() {
            return Err(error(view.message.as_deref().unwrap_or(
                "[JOB_JOURNAL] Job outcome could not be recorded; inspect local history",
            )));
        }
        Ok(view)
    }
    pub fn release(&self, handle: &str) -> Result<(), FmError> {
        let canonical_handle = uuid::Uuid::parse_str(handle)
            .map_err(|_| error("Invalid job session/operation ID"))?
            .to_string();
        let handle = canonical_handle.as_str();
        let mut inner = self
            .0
            .lock()
            .map_err(|_| error("Job session state unavailable"))?;
        if let Some(slot) = inner
            .slot
            .as_mut()
            .filter(|slot| slot.id == handle || slot.op_id == handle)
        {
            slot.cancel.store(true, Ordering::Relaxed);
            slot.manifest = None;
            slot.preview = None;
            slot.approval = None;
            // In-flight work owns its binding/run-lock until it actually stops.
            if matches!(
                slot.phase,
                Phase::Bound | Phase::Complete | Phase::Prepared | Phase::AwaitingApproval
            ) {
                if let Some(timer) = slot.expiry_abort.take() {
                    timer.abort();
                }
                let id = slot.id.clone();
                let terminal = if slot.phase == Phase::Bound {
                    journal::Status::Cancelled
                } else {
                    journal::Status::Discarded
                };
                drop(inner);
                self.stop_as(
                    &id,
                    ScanOutcome::Cancelled,
                    "Job session released; browser/SDK requests already sent cannot be recalled"
                        .into(),
                    Some(terminal),
                )?;
                return Ok(());
            }
        } else {
            // Handle close/cancel IPC arriving before a queued bind command.
            inner.used.push_back(handle.into());
            while inner.used.len() > 128 {
                inner.used.pop_front();
            }
        }
        Ok(())
    }
    pub fn status(&self, id: &str) -> Result<RunStatus, FmError> {
        let binding = {
            let inner = self
                .0
                .lock()
                .map_err(|_| error("Job session state unavailable"))?;
            let slot = inner
                .slot
                .as_ref()
                .filter(|slot| slot.id == id)
                .ok_or_else(|| error("Unknown or replaced job session"))?;
            if slot.expires <= Instant::now() && slot.phase != Phase::Executing {
                drop(inner);
                return self.stop(
                    id,
                    ScanOutcome::Cancelled,
                    "Job session expired; bind and compare again".into(),
                );
            }
            slot.binding.clone()
        };
        if let Some(binding) = binding {
            if let Err(failure) = binding.validate() {
                return self.stop(id, ScanOutcome::Failed, failure.to_string());
            }
        }
        let inner = self
            .0
            .lock()
            .map_err(|_| error("Job session state unavailable"))?;
        if inner
            .slot
            .as_ref()
            .filter(|slot| slot.id == id)
            .is_some_and(|slot| slot.expires <= Instant::now() && slot.phase != Phase::Executing)
        {
            drop(inner);
            return self.stop(
                id,
                ScanOutcome::Cancelled,
                "Job session expired during validation; bind and compare again".into(),
            );
        }
        let slot = inner
            .slot
            .as_ref()
            .filter(|slot| slot.id == id)
            .ok_or_else(|| error("Job session replaced"))?;
        if let Some(plan) = &slot.preview {
            let valid = slot
                .binding
                .as_ref()
                .zip(slot.manifest.as_ref())
                .ok_or_else(|| error("Prepared job ownership is unavailable"))
                .and_then(|(binding, manifest)| {
                    plan.check_owner(binding, id, &manifest.id, slot.expires)
                });
            if let Err(failure) = valid {
                drop(inner);
                return self.stop(id, ScanOutcome::Failed, failure.to_string());
            }
        }
        Ok(slot.view())
    }
    pub fn page(
        &self,
        id: &str,
        manifest_id: &str,
        offset: usize,
    ) -> Result<Vec<collect::Row>, FmError> {
        let status = self.status(id)?;
        if !matches!(status.phase, "complete" | "prepared")
            || status.manifest_id.as_deref() != Some(manifest_id)
        {
            return Err(error(status.message.as_deref().unwrap_or(
                "A matching, fresh native complete manifest is required",
            )));
        }
        let inner = self
            .0
            .lock()
            .map_err(|_| error("Job session state unavailable"))?;
        let slot = inner
            .slot
            .as_ref()
            .filter(|slot| {
                slot.id == id
                    && matches!(slot.phase, Phase::Complete | Phase::Prepared)
                    && slot.expires > Instant::now()
                    && !slot.cancel.load(Ordering::Relaxed)
            })
            .ok_or_else(|| error("Job session was cancelled or changed"))?;
        let manifest = slot
            .manifest
            .as_ref()
            .filter(|manifest| manifest.id == manifest_id)
            .ok_or_else(|| error("Manifest no longer exists"))?;
        Ok(manifest
            .entries
            .iter()
            .skip(offset)
            .take(100)
            .map(|entry| entry.row.clone())
            .collect())
    }
}

pub(crate) struct RunLock(File);
impl Drop for RunLock {
    fn drop(&mut self) {
        // Closing alone can leave a fork-inherited duplicate holding this lease
        // until the child execs. Only the final Arc owner unlocks the lease.
        let _ = self.0.unlock();
    }
}
pub(crate) fn run_lock(paths: &Paths) -> Result<RunLock, FmError> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
        .open(paths.store.with_extension("run-lock"))
        .map_err(|_| {
            error("Cannot open the job run lock; check configuration directory permissions")
        })?;
    if !file
        .metadata()
        .map_err(|_| error("Cannot inspect job run lock"))?
        .is_file()
    {
        return Err(error("Job run lock must be a regular file"));
    }
    file.try_lock()
        .map_err(|_| error("Another cooperating process/job task still owns the job run lock"))?;
    Ok(RunLock(file))
}

struct Auth {
    pin: S3Pin,
    access: Zeroizing<String>,
    secret: Zeroizing<String>,
    proxy_user: Zeroizing<String>,
    proxy_secret: Option<Zeroizing<String>>,
    sse: Option<Zeroizing<String>>,
}
type KeyReader = Arc<dyn Fn(String) -> Result<Option<String>, FmError> + Send + Sync>;
type EndpointFactory = Arc<dyn Fn(Auth) -> Result<Endpoint, FmError> + Send + Sync>;
fn read_auth(
    pin: &S3Pin,
    raw: &serde_json::Value,
    cancel: &AtomicBool,
    budget: &ScanBudget,
    read: &(dyn Fn(String) -> Result<Option<String>, FmError> + Send + Sync),
) -> Result<Auth, FmError> {
    let get = |id: String| -> Result<Zeroizing<String>, FmError> {
        budget.check(cancel)?;
        let value = read(id).map(|value| value.map(Zeroizing::new));
        budget.check(cancel)?;
        value.map_err(|_| error("Could not read a required native job keychain entry. Review OS keychain access/approval and the intended saved connection. Raw keychain output was not included; no fallback was used."))?
            .filter(|value| !value.is_empty()).ok_or_else(|| error("Required job credential/proxy/SSE-C entry is missing. Resave the intended connection; no fallback was used."))
    };
    Ok(Auth {
        pin: pin.clone(),
        access: Zeroizing::new(raw["accessKeyId"].as_str().unwrap_or("").into()),
        secret: get(pin.profile_id.clone())?,
        proxy_user: Zeroizing::new(raw["proxyUsername"].as_str().unwrap_or("").into()),
        proxy_secret: if pin.configuration.proxy_auth {
            Some(get(format!("{}:proxy", pin.profile_id))?)
        } else {
            None
        },
        sse: if pin.configuration.sse_customer_key_enabled {
            Some(get(format!("{}:sse-c", pin.profile_id))?)
        } else {
            None
        },
    })
}

fn namespace(config: &ProfileConfiguration) -> Result<String, FmError> {
    if config.acceleration || config.bucket.ends_with("--x-s3") {
        return Err(error("Accelerated/Express job identity binding is not supported yet; ordinary connections are unchanged"));
    }
    if let Some(endpoint) = &config.endpoint {
        return Ok(endpoint.clone());
    }
    let (family, number) = config.region.rsplit_once('-').ok_or_else(|| {
        error("Unsupported AWS region for deterministic job routing; use an explicit root endpoint")
    })?;
    if !matches!(
        family,
        "us-east"
            | "us-west"
            | "us-gov-east"
            | "us-gov-west"
            | "af-south"
            | "ap-east"
            | "ap-south"
            | "ap-southeast"
            | "ap-northeast"
            | "ca-central"
            | "ca-west"
            | "eu-central"
            | "eu-north"
            | "eu-south"
            | "eu-west"
            | "il-central"
            | "me-central"
            | "me-south"
            | "mx-central"
            | "sa-east"
            | "cn-north"
            | "cn-northwest"
    ) || !matches!(number.parse::<u8>(), Ok(1..=9))
    {
        return Err(error("AWS region/partition is not supported by job binding; provide an explicitly reviewed endpoint"));
    }
    Ok(format!(
        "https://s3.{}.amazonaws.com{}/",
        config.region,
        if config.region.starts_with("cn-") {
            ".cn"
        } else {
            ""
        }
    ))
}
fn reject_resolved_overlap(job: &Job, source: &str, destination: &str) -> Result<(), FmError> {
    if let SourcePin::S3 { pin } = &job.source {
        if source == destination && pin.configuration.bucket == job.destination.configuration.bucket
        {
            let a = &pin.prefix;
            let b = &job.destination.prefix;
            if a.starts_with(b) || b.starts_with(a) {
                return Err(error("Effective source and destination folder namespaces overlap; no requests were sent"));
            }
        }
    }
    Ok(())
}
fn reject_overrides() -> Result<(), FmError> {
    if [
        "AWS_ENDPOINT_URL",
        "AWS_ENDPOINT_URL_S3",
        "AWS_USE_FIPS_ENDPOINT",
        "AWS_USE_DUALSTACK_ENDPOINT",
        "AWS_CA_BUNDLE",
    ]
    .iter()
    .any(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty()))
    {
        return Err(error("AWS endpoint/FIPS/dual-stack/custom-CA environment overrides are present. Job binding will not silently ignore or adopt them; review routing or use ordinary connections."));
    }
    Ok(())
}

#[derive(Debug)]
struct ReadNamespaceGuard {
    origin: url::Url,
}
impl aws_sdk_s3::config::Intercept for ReadNamespaceGuard {
    fn name(&self) -> &'static str {
        "FurmanReadOnlyJobNamespace"
    }
    fn modify_before_signing(
        &self,
        context: &mut aws_smithy_runtime_api::client::interceptors::context::BeforeTransmitInterceptorContextMut<'_>,
        _runtime: &aws_smithy_runtime_api::client::runtime_components::RuntimeComponents,
        config: &mut aws_smithy_types::config_bag::ConfigBag,
    ) -> Result<(), aws_smithy_runtime_api::box_error::BoxError> {
        let operation = config
            .load::<aws_smithy_runtime_api::client::orchestrator::Metadata>()
            .map(aws_smithy_runtime_api::client::orchestrator::Metadata::name)
            .unwrap_or("");
        if !matches!(
            operation,
            "HeadBucket"
                | "HeadObject"
                | "ListObjectsV2"
                | "GetObject"
                | "GetObjectAcl"
                | "GetObjectTagging"
                | "GetObjectLockConfiguration"
        ) {
            return Err(READ_ONLY_ERROR.into());
        }
        self.validate(context.request().uri())
    }
    fn modify_before_transmit(
        &self,
        context: &mut aws_smithy_runtime_api::client::interceptors::context::BeforeTransmitInterceptorContextMut<'_>,
        _runtime: &aws_smithy_runtime_api::client::runtime_components::RuntimeComponents,
        _config: &mut aws_smithy_types::config_bag::ConfigBag,
    ) -> Result<(), aws_smithy_runtime_api::box_error::BoxError> {
        if !matches!(context.request().method(), "GET" | "HEAD") {
            return Err(READ_ONLY_ERROR.into());
        }
        self.validate(context.request().uri())
    }
}
impl ReadNamespaceGuard {
    fn validate(&self, uri: &str) -> Result<(), aws_smithy_runtime_api::box_error::BoxError> {
        let actual = url::Url::parse(uri).map_err(|_| NAMESPACE_ERROR)?;
        if actual.scheme() != self.origin.scheme()
            || actual.host_str() != self.origin.host_str()
            || actual.port_or_known_default() != self.origin.port_or_known_default()
            || !actual.username().is_empty()
            || actual.password().is_some()
        {
            return Err(NAMESPACE_ERROR.into());
        }
        Ok(())
    }
}
fn job_sdk_error<E>(operation: &str, failure: &aws_sdk_s3::error::SdkError<E>) -> FmError
where
    E: aws_sdk_s3::error::ProvideErrorMetadata + std::error::Error + 'static,
{
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(failure);
    for _ in 0..20 {
        let Some(current) = source else {
            break;
        };
        let message = current.to_string();
        for known in [NAMESPACE_ERROR, READ_ONLY_ERROR]
            .into_iter()
            .chain(execute::GUARD_ERRORS.iter().copied())
        {
            if message.contains(known) {
                return error(known);
            }
        }
        source = current.source();
    }
    s3_sdk_error(operation, failure)
}

fn client(auth: Auth) -> Result<Endpoint, FmError> {
    let namespace = namespace(&auth.pin.configuration)?;
    let credentials = Credentials::new(
        auth.access.as_str(),
        auth.secret.as_str(),
        None,
        None,
        "furman-native-job-keychain",
    );
    let mut connector = aws_smithy_http_client::Connector::builder().tls_provider(
        aws_smithy_http_client::tls::Provider::Rustls(
            aws_smithy_http_client::tls::rustls_provider::CryptoMode::AwsLc,
        ),
    );
    if let Some(proxy) = &auth.pin.configuration.proxy_endpoint {
        let mut config = aws_smithy_http_client::proxy::ProxyConfig::all(proxy)
            .map_err(|_| error("Invalid pinned job proxy endpoint"))?;
        if let Some(password) = &auth.proxy_secret {
            config = config.with_basic_auth(auth.proxy_user.as_str(), password.as_str());
        }
        connector = connector.proxy_config(config);
    }
    let http = aws_smithy_http_client::Builder::new()
        .build_with_connector_fn(move |_settings, _components| connector.clone().build());
    // Deliberately no ConfigLoader: no shared profile/credential_process/SSO,
    // IMDS, environment endpoint fallback, implicit Express session or STS call.
    let client = Client::from_conf(
        aws_sdk_s3::Config::builder()
            .behavior_version_latest()
            .region(Region::new(auth.pin.configuration.region.clone()))
            .endpoint_url(namespace.clone())
            .force_path_style(true)
            .credentials_provider(credentials)
            .retry_config(RetryConfig::standard().with_max_attempts(2))
            .disable_s3_express_session_auth(true)
            .use_fips(false)
            .use_dual_stack(false)
            .request_checksum_calculation(RequestChecksumCalculation::WhenRequired)
            .http_client(http)
            .interceptor(ReadNamespaceGuard {
                origin: url::Url::parse(&namespace)
                    .map_err(|_| error("Invalid native job namespace"))?,
            })
            .interceptor(crate::s3::activity::interceptor())
            .build(),
    );
    let client = crate::s3::compat::configure_conditions(
        client,
        Some(&auth.pin.configuration.provider),
        Some(auth.access.as_str()),
    );
    let client = crate::s3::sse_c::configure(client, auth.sse.as_deref().map(String::as_str))?;
    Ok(Endpoint {
        client,
        instance: fresh_id(),
        namespace,
        bucket: auth.pin.configuration.bucket,
    })
}

pub async fn bind(
    state: &JobRunState,
    paths: Paths,
    op_id: String,
    job_id: String,
    revision: String,
    approved: bool,
) -> Result<RunStatus, FmError> {
    if !approved {
        return Err(error(
            "Explicit approval is required for native keychain reads and HeadBucket validation",
        ));
    }
    reject_overrides()?;
    bind_with(
        state,
        paths,
        op_id,
        job_id,
        revision,
        approved,
        Arc::new(crate::commands::keychain::keychain_get),
        Arc::new(client),
    )
    .await
}
async fn bind_with(
    state: &JobRunState,
    paths: Paths,
    op_id: String,
    job_id: String,
    revision: String,
    approved: bool,
    read: KeyReader,
    factory: EndpointFactory,
) -> Result<RunStatus, FmError> {
    if !approved {
        return Err(error(
            "Explicit approval is required before native job binding",
        ));
    }
    let (id, cancel, mut guard) = state.begin(&op_id)?;
    {
        let mut inner = state
            .0
            .lock()
            .map_err(|_| error("Job session state unavailable"))?;
        inner
            .slot
            .as_mut()
            .filter(|slot| slot.id == id)
            .ok_or(FmError::ScanCancelled)?
            .arm_expiry(state);
    }
    let binding_started = Instant::now();
    let result = bind_inner(state.clone(), id.clone(), paths, job_id, revision, cancel.clone(), read, factory)
        .await
        .and_then(|mut binding| {
            binding.validate()?;
            if binding_started.elapsed() >= Duration::from_secs(60) {
                return Err(FmError::ScanLimited("Job binding exceeded its 60-second cooperative budget; no bound run was published".into()));
            }
            binding.bind_progress.elapsed_ms = binding_started.elapsed().as_millis() as u64;
            binding.attempt.stage(journal::Status::Bound,binding.bind_progress,None)?;
            if binding_started.elapsed() >= Duration::from_secs(60) {return Err(FmError::ScanLimited("Job binding exceeded its cooperative deadline while recording its stage; no bound handle was published".into()));}
            Ok(binding)
        });
    match result {
        Ok(binding) => {
            let mut inner = state
                .0
                .lock()
                .map_err(|_| error("Job session state unavailable"))?;
            let slot = inner
                .slot
                .as_mut()
                .filter(|slot| {
                    slot.id == id
                        && slot.expires > Instant::now()
                        && !slot.cancel.load(Ordering::Relaxed)
                })
                .ok_or(FmError::ScanCancelled)?;
            slot.progress = binding.bind_progress;
            slot.binding = Some(Arc::new(binding));
            slot.phase = Phase::Bound;
            slot.expires = Instant::now() + TTL;
            slot.arm_expiry(state);
            let view = slot.view();
            guard.finished = true;
            Ok(view)
        }
        Err(failure) => {
            let recorded = state.stop(&id, outcome(&failure), failure.to_string());
            guard.finished = true;
            if let Err(journal_failure) = recorded {
                return Err(journal_failure);
            }
            Err(failure)
        }
    }
}
fn outcome(failure: &FmError) -> ScanOutcome {
    match failure {
        FmError::ScanCancelled => ScanOutcome::Cancelled,
        FmError::ScanLimited(_) => ScanOutcome::Limited,
        _ => ScanOutcome::Failed,
    }
}

async fn bind_inner(
    state: JobRunState,
    run_id: String,
    paths: Paths,
    job_id: String,
    revision: String,
    cancel: Arc<AtomicBool>,
    read: KeyReader,
    factory: EndpointFactory,
) -> Result<Binding, FmError> {
    let budget = Arc::new(ScanBudget::with_limits(ScanLimits {
        max_seconds: 60,
        max_requests: 2,
        ..ScanLimits::DEFAULT
    }));
    let global_limits = ScanBudget::new().limits;
    let run_lock = Arc::new(run_lock(&paths)?);
    let fault = state.journal_fault()?;
    let recovered = journal::recover(&paths).map_err(|failure| {
        fault.store(true, Ordering::Relaxed);
        failure
    })?;
    if recovered.storage_review_required {
        return Err(error(
            "[JOB_JOURNAL] Job journal needs explicit storage recovery review before binding",
        ));
    }
    let progress_guard = BindingProgress {
        state: state.clone(),
        id: run_id.clone(),
        budget: budget.clone(),
    };
    let read_paths = paths.clone();
    let worker_cancel = cancel.clone();
    let worker_lock = run_lock.clone();
    let worker_budget = budget.clone();
    let inputs = tokio::task::spawn_blocking(move || {
        let lease = worker_lock;
        jobs::with_binding_snapshot(&read_paths, &job_id, &revision, |job, profiles| {
            // Deterministic unsupported-routing/alias checks require no secrets.
            let destination_namespace = namespace(&job.destination.configuration)?;
            if let SourcePin::S3 { pin } = &job.source {
                reject_resolved_overlap(
                    job,
                    &namespace(&pin.configuration)?,
                    &destination_namespace,
                )?;
            }
            journal::ensure_ready(&read_paths, &job.id)?;
            let attempt = journal::Attempt::start(
                read_paths.clone(),
                run_id.clone(),
                job,
                lease.clone(),
                fault.clone(),
                worker_cancel.clone(),
            )?;
            if let Err(failure) = state.attach_attempt(&run_id, attempt.clone()) {
                let _ = attempt.stage(journal::Status::Cancelled, ScanProgress::default(), None);
                return Err(failure);
            }
            let destination = read_auth(
                &job.destination,
                &profiles[&job.destination.profile_id],
                &worker_cancel,
                &worker_budget,
                read.as_ref(),
            )?;
            let source = match &job.source {
                SourcePin::S3 { pin } => Some(read_auth(
                    pin,
                    &profiles[&pin.profile_id],
                    &worker_cancel,
                    &worker_budget,
                    read.as_ref(),
                )?),
                _ => None,
            };
            let root = match &job.source {
                SourcePin::Local { path, .. } => {
                    let (directory, pin) = jobs::open_local_root(path)?;
                    if pin != job.source {
                        return Err(error("Local source changed while binding"));
                    }
                    Some(Arc::new(LocalRoot { directory, pin }))
                }
                _ => None,
            };
            Ok((job.clone(), source, destination, root, attempt))
        })
    })
    .await
    .map_err(|_| {
        error("Native keychain/root binding task failed; no connection fallback was used")
    })??;
    if cancel.load(Ordering::Relaxed) {
        return Err(FmError::ScanCancelled);
    }
    let (job, source_auth, destination_auth, root, attempt) = inputs;
    jobs::validate_binding(&paths, &job)?;
    let destination = factory(destination_auth)?;
    let source = if let Some(auth) = source_auth {
        BoundSource::S3(factory(auth)?)
    } else {
        BoundSource::Local(root.ok_or_else(|| error("Local binding missing"))?)
    };
    if let BoundSource::S3(endpoint) = &source {
        reject_resolved_overlap(&job, &endpoint.namespace, &destination.namespace)?;
    }
    let validate = |endpoint: &Endpoint| {
        endpoint
            .client
            .head_bucket()
            .bucket(&endpoint.bucket)
            .send()
    };
    budget
        .request(&cancel, async {
            validate(&destination)
                .await
                .map_err(|failure| job_sdk_error("Bind job destination: HeadBucket", &failure))
        })
        .await?;
    if let BoundSource::S3(endpoint) = &source {
        budget
            .request(&cancel, async {
                validate(endpoint)
                    .await
                    .map_err(|failure| job_sdk_error("Bind job source: HeadBucket", &failure))
            })
            .await?;
    }
    drop(progress_guard);
    Ok(Binding {
        paths,
        job,
        source,
        destination,
        global_limits,
        bind_progress: budget.progress(),
        limits: ScanLimits {
            max_objects: global_limits.max_objects.min(100_000),
            max_seconds: global_limits.max_seconds.min(900),
            ..global_limits
        },
        _run_lock: run_lock,
        attempt,
    })
}

struct BindingProgress {
    state: JobRunState,
    id: String,
    budget: Arc<ScanBudget>,
}
impl Drop for BindingProgress {
    fn drop(&mut self) {
        if let Ok(mut inner) = self.state.0.lock() {
            if let Some(slot) = inner
                .slot
                .as_mut()
                .filter(|slot| slot.id == self.id && slot.phase == Phase::Binding)
            {
                slot.progress = self.budget.progress();
            }
        }
    }
}

pub async fn scan(
    state: &JobRunState,
    id: &str,
    notify: Arc<dyn Fn(ScanProgress) + Send + Sync>,
) -> Result<RunStatus, FmError> {
    let status = state.status(id)?;
    if status.phase != "bound" {
        return Err(error(
            "A fresh bound run is required; each session allows one native comparison",
        ));
    }
    let (binding, cancel) = {
        let mut inner = state
            .0
            .lock()
            .map_err(|_| error("Job session state unavailable"))?;
        let slot = inner
            .slot
            .as_mut()
            .filter(|slot| {
                slot.id == id && slot.phase == Phase::Bound && !slot.cancel.load(Ordering::Relaxed)
            })
            .ok_or_else(|| error("Job comparison already started or was cancelled"))?;
        slot.phase = Phase::Scanning;
        slot.progress = ScanProgress::default();
        (
            slot.binding
                .clone()
                .ok_or_else(|| error("Missing native job binding"))?,
            slot.cancel.clone(),
        )
    };
    let mut guard = TaskGuard {
        state: state.clone(),
        id: id.into(),
        finished: false,
    };
    binding
        .attempt
        .stage(journal::Status::Scanning, ScanProgress::default(), None)?;
    let budget = Arc::new(ScanBudget::with_limits(binding.limits));
    let progress_state = state.clone();
    let progress_id = id.to_owned();
    let progress: Arc<dyn Fn(ScanProgress) + Send + Sync> = Arc::new(move |event| {
        if let Ok(mut inner) = progress_state.0.lock() {
            if let Some(slot) = inner.slot.as_mut().filter(|slot| slot.id == progress_id) {
                slot.progress = event;
            }
        }
        notify(event);
    });
    let result = collect::compare(binding.clone(), cancel.clone(), budget.clone(), progress).await;
    let result = result.and_then(|manifest| {
        budget.check(&cancel)?;
        binding.validate()?;
        budget.check(&cancel)?;
        Ok(manifest)
    });
    match result {
        Ok(manifest) => {
            let terminal = manifest.summary.new + manifest.summary.modified == 0;
            binding.attempt.stage(
                if terminal {
                    journal::Status::NoChanges
                } else {
                    journal::Status::Compared
                },
                budget.progress(),
                None,
            )?;
            if let Err(failure) = budget.check(&cancel) {
                let result = state.stop(id, outcome(&failure), failure.to_string());
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
                    slot.id == id
                        && slot.phase == Phase::Scanning
                        && slot.expires > Instant::now()
                        && !slot.cancel.load(Ordering::Relaxed)
                })
                .ok_or(FmError::ScanCancelled)?;
            slot.phase = Phase::Complete;
            slot.outcome = Some(ScanOutcome::Complete);
            slot.progress = budget.progress();
            slot.manifest = Some(manifest);
            // Do not slide the binding lease by rescanning/reading pages.
            let view = slot.view();
            guard.finished = true;
            Ok(view)
        }
        Err(failure) => {
            if let Ok(mut inner) = state.0.lock() {
                if let Some(slot) = inner.slot.as_mut().filter(|slot| slot.id == id) {
                    slot.progress = budget.progress();
                }
            }
            let view = state.stop(id, outcome(&failure), failure.to_string())?;
            guard.finished = true;
            Ok(view)
        }
    }
}
