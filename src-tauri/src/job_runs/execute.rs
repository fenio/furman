//! Initial guarded execution cohort: single-shot create-only PUT, <=5 MiB/file.
//! Read clients remain read-only; only a consumed native approval derives a
//! per-key conditional writer. No multipart/replacement/delete fallback exists.
use super::{
    attributes::Attributes,
    collect, error, fresh_id, job_sdk_error,
    journal::{Execution, Status},
    preview::{self, Plan},
    Arc, AtomicBool, Binding, BoundSource, File, FmError, Instant, JobRunState, Ordering, Phase,
    RunStatus, ScanBudget, ScanProgress, SourcePin, TaskGuard,
};
use aws_sdk_s3::{
    config::{retry::RetryConfig, Intercept},
    error::ProvideErrorMetadata,
    Client,
};
use aws_smithy_runtime_api::{
    box_error::BoxError,
    client::{
        interceptors::context::BeforeTransmitInterceptorContextMut, orchestrator::Metadata,
        runtime_components::RuntimeComponents,
    },
};
use aws_smithy_types::config_bag::ConfigBag;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::os::unix::fs::FileExt;

#[cfg(test)]
mod tests;

// Only these static native diagnoses may escape SDK source chains. Never format
// arbitrary interceptor/transport sources (which can carry URLs or credentials).
pub(super) const GUARD_ERRORS: &[&str] = &[
    "Job writer only authorizes a conditional create-only PUT",
    "Unexpected job mutation query",
    "Job mutation resolved outside its exact approved key",
    "Job mutation cancelled, expired or blocked by journal storage",
    "Native job identity changed before transmission",
    "A job PUT cannot be replayed",
    "Job execution budget/cancellation/approval expired before transmission",
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewRequest {
    pub run_id: String,
    pub preview_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecuteRequest {
    pub run_id: String,
    pub approval_id: String,
    pub approved: bool,
    pub acknowledge_exclusions: bool,
}
#[derive(Serialize)]
pub struct Review {
    pub approval_id: String,
    pub preview: preview::Preview,
    pub source: String,
    pub destination: String,
    pub exclusions: Vec<String>,
    pub destination_sse_c: bool,
}
pub(super) struct Approval {
    id: String,
    pub plan: Plan,
    manifest: String,
    binding: Arc<Binding>,
    expires: Instant,
}
impl Approval {
    pub fn view(&self, expires: Instant) -> Review {
        let source = match &self.binding.job.source {
            SourcePin::Local { path, .. } => path.clone(),
            SourcePin::S3 { pin } => format!(
                "{} bucket={} prefix={}",
                match &self.binding.source {
                    BoundSource::S3(endpoint) => endpoint.namespace.as_str(),
                    _ => "invalid",
                },
                pin.configuration.bucket,
                pin.prefix
            ),
        };
        Review {
            approval_id: self.id.clone(),
            preview: self.plan.view(expires),
            source,
            destination: format!(
                "{} bucket={} prefix={}",
                self.binding.destination.namespace,
                self.binding.destination.bucket,
                self.binding.job.destination.prefix
            ),
            exclusions: self.binding.job.exclude_patterns.clone(),
            destination_sse_c: self
                .binding
                .job
                .destination
                .configuration
                .sse_customer_key_enabled,
        }
    }
    pub fn validate(&self, run: &str, expires: Instant) -> Result<(), FmError> {
        self.binding.validate()?;
        self.plan
            .check_owner(&self.binding, run, &self.manifest, expires)?;
        if let Some(reason) = self.plan.execution_blocker() {
            return Err(error(&reason));
        }
        if self.expires != expires || !self.binding.attempt.healthy() {
            return Err(error("Execution approval ownership/storage changed"));
        }
        Ok(())
    }
}
#[derive(Clone, Serialize)]
pub struct ExecutionResult {
    pub status: Status,
    pub counts: Execution,
    pub outcome_recorded: bool,
    pub message: Option<String>,
}

fn hex(hash: Sha256) -> String {
    hash.finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
async fn snapshot_bytes(
    binding: Arc<Binding>,
    file: File,
    size: u64,
    expected: String,
    cancel: Arc<AtomicBool>,
) -> Result<Vec<u8>, FmError> {
    tokio::task::spawn_blocking(move || {
        let _lease = binding;
        if size > 5 * 1024 * 1024 {
            return Err(error("Snapshot exceeds single-PUT job cohort"));
        }
        let mut bytes = vec![0; size as usize];
        let mut offset = 0;
        while offset < bytes.len() {
            if cancel.load(Ordering::Relaxed) {
                return Err(FmError::ScanCancelled);
            }
            let count = file
                .read_at(&mut bytes[offset..], offset as u64)
                .map_err(|_| error("Cannot read retained snapshot capability"))?;
            if count == 0 {
                return Err(error("Retained snapshot became incomplete"));
            }
            offset += count;
        }
        let mut hash = Sha256::new();
        hash.update(&bytes);
        if hex(hash) != expected
            || file
                .metadata()
                .map_err(|_| error("Cannot inspect retained snapshot"))?
                .len()
                != size
        {
            return Err(error(
                "Retained snapshot no longer matches the native content digest",
            ));
        }
        Ok(bytes)
    })
    .await
    .map_err(|_| error("Checked snapshot read worker failed"))?
}
async fn read_object(
    binding: &Binding,
    key: &str,
    size: u64,
    etag: &str,
    budget: &ScanBudget,
    cancel: &AtomicBool,
) -> Result<(String, Attributes), FmError> {
    let endpoint = &binding.destination;
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
                .map_err(|failure| job_sdk_error("Job conditional content check", &failure))
        })
        .await?;
    if response.e_tag() != Some(etag)
        || response
            .content_length()
            .and_then(|n| u64::try_from(n).ok())
            != Some(size)
    {
        return Err(error(
            "Conditional content check returned a changed ETag/size",
        ));
    }
    let attributes = Attributes::capture(&response);
    let mut body = response.body;
    let mut hash = Sha256::new();
    let mut bytes = 0u64;
    loop {
        let chunk = budget
            .wait(cancel, async {
                body.try_next().await.map_err(|_| {
                    error(
                        "Job verification body stream failed; no body was included in diagnostics",
                    )
                })
            })
            .await?;
        let Some(chunk) = chunk else {
            break;
        };
        bytes = bytes
            .checked_add(chunk.len() as u64)
            .ok_or_else(|| error("Verification byte overflow"))?;
        budget.bytes(chunk.len() as u64, true);
        if bytes > size {
            return Err(error("Verification body exceeded expected size"));
        }
        hash.update(&chunk);
    }
    if bytes != size {
        return Err(error("Verification body was incomplete"));
    }
    Ok((hex(hash), attributes))
}
async fn missing(
    binding: &Binding,
    key: &str,
    budget: &ScanBudget,
    cancel: &AtomicBool,
) -> Result<(), FmError> {
    budget.request(cancel,async{match binding.destination.client.head_object().bucket(&binding.destination.bucket).key(key).send().await{
        Err(failure) if failure.raw_response().is_some_and(|r|r.status().as_u16()==404)&&matches!(failure.as_service_error().and_then(|e|e.code()),Some("NotFound"|"NoSuchKey"))=>Ok(()),
        Err(failure)=>Err(job_sdk_error("Job create-only destination check",&failure)),Ok(_)=>Err(error("A destination now exists; the consumed approval cannot replace it. Bind/compare again.")),
    }}).await
}
async fn source_matches(
    binding: &Arc<Binding>,
    item: &preview::Item,
    budget: &Arc<ScanBudget>,
    cancel: &Arc<AtomicBool>,
) -> Result<(), FmError> {
    let expected = item
        .entry
        .source
        .as_ref()
        .ok_or_else(|| error("Missing native source expectation"))?;
    match &binding.source {
        BoundSource::Local(root) => {
            let root = root.clone();
            let path = item.entry.row.relative_path.clone();
            let budget = budget.clone();
            let cancel = cancel.clone();
            let lease = binding.clone();
            let actual = tokio::task::spawn_blocking(move || {
                let _lease = lease;
                let file = crate::s3::write_local::relative_open(
                    &root.directory,
                    std::path::Path::new(&path),
                    false,
                )
                .map_err(|_| error("Local source changed or became a symlink/unreadable"))?;
                collect::hash_local(file, &cancel, &budget, &|_| {})
            })
            .await
            .map_err(|_| error("Local source recheck worker failed"))??;
            if actual.size != expected.size || actual.sha256 != expected.sha256 {
                return Err(error(
                    "Local source changed after comparison/preparation; approval was not reused",
                ));
            }
        }
        BoundSource::S3(_) => {
            return Err(error("S3-source execution is not supported by this cohort; source attribute/encryption/ACL/tag preservation needs separate review"));
        }
    }
    Ok(())
}

pub async fn review(state: &JobRunState, request: ReviewRequest) -> Result<RunStatus, FmError> {
    let status = state.status(&request.run_id)?;
    if status.phase != "prepared" {
        return Err(error(
            "A fresh checked preview is required for execution review",
        ));
    }
    let (binding, plan, manifest, expires, cancel) = {
        let mut inner = state.0.lock().map_err(|_| error("Job state unavailable"))?;
        let slot = inner
            .slot
            .as_mut()
            .filter(|slot| {
                slot.id == request.run_id
                    && slot.phase == Phase::Prepared
                    && !slot.cancel.load(Ordering::Relaxed)
            })
            .ok_or_else(|| error("Preview changed or review already started"))?;
        let plan = slot
            .preview
            .as_ref()
            .filter(|plan| plan.id == request.preview_id)
            .ok_or_else(|| error("Wrong/cross-run preview ID"))?;
        if let Some(reason) = plan.execution_blocker() {
            return Err(error(&reason));
        }
        let binding = slot
            .binding
            .clone()
            .ok_or_else(|| error("Missing binding"))?;
        let manifest = slot
            .manifest
            .as_ref()
            .ok_or_else(|| error("Missing native manifest"))?
            .id
            .clone();
        plan.check_owner(&binding, &request.run_id, &manifest, slot.expires)?;
        slot.phase = Phase::Reviewing;
        slot.progress = ScanProgress::default();
        (
            binding,
            slot.preview.take().unwrap(),
            manifest,
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
    binding
        .attempt
        .stage(Status::Reviewing, ScanProgress::default(), None)?;
    let result = async {
        for item in &plan.items {
            binding.validate()?;
            budget.object(&cancel)?;
            missing(&binding, &item.key, &budget, &cancel).await?;
            source_matches(&binding, item, &budget, &cancel).await?;
            snapshot_bytes(
                binding.clone(),
                item.snapshot
                    .file
                    .try_clone()
                    .map_err(|_| error("Snapshot descriptor unavailable"))?,
                item.snapshot.size,
                item.snapshot.sha256.clone(),
                cancel.clone(),
            )
            .await?;
            budget.check(&cancel)?;
        }
        binding.validate()?;
        plan.check_owner(&binding, &request.run_id, &manifest, expires)?;
        binding
            .attempt
            .stage(Status::AwaitingApproval, budget.progress(), None)?;
        budget.check(&cancel)?;
        Ok::<_, FmError>(())
    }
    .await;
    match result {
        Ok(()) => {
            let mut inner = state.0.lock().map_err(|_| error("Job state unavailable"))?;
            let slot = inner
                .slot
                .as_mut()
                .filter(|s| {
                    s.id == request.run_id
                        && s.phase == Phase::Reviewing
                        && s.expires == expires
                        && expires > Instant::now()
                        && !s.cancel.load(Ordering::Relaxed)
                })
                .ok_or(FmError::ScanCancelled)?;
            slot.approval = Some(Approval {
                id: fresh_id(),
                plan,
                manifest,
                binding,
                expires,
            });
            slot.phase = Phase::AwaitingApproval;
            slot.progress = budget.progress();
            let view = slot.view();
            guard.finished = true;
            Ok(view)
        }
        Err(failure) => {
            let result = state.stop(
                &request.run_id,
                super::outcome(&failure),
                failure.to_string(),
            );
            guard.finished = true;
            result
        }
    }
}

struct ScopedWriter {
    binding: Arc<Binding>,
    key: String,
    cancel: Arc<AtomicBool>,
    sent: AtomicBool,
    first_deadline: Option<Instant>,
    budget: Arc<ScanBudget>,
}
#[derive(Debug)]
struct WriterInterceptor(Arc<ScopedWriter>);
impl std::fmt::Debug for ScopedWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ScopedCreateOnlyJobWriter([private])")
    }
}
impl ScopedWriter {
    fn check(
        &self,
        context: &BeforeTransmitInterceptorContextMut<'_>,
        config: &ConfigBag,
    ) -> Result<(), BoxError> {
        let operation = config.load::<Metadata>().map(Metadata::name).unwrap_or("");
        let request = context.request();
        if operation != "PutObject"
            || request.method() != "PUT"
            || request.headers().get("if-none-match") != Some("*")
            || request.headers().get("if-match").is_some()
        {
            return Err("Job writer only authorizes a conditional create-only PUT".into());
        }
        let uri = request.uri();
        super::ReadNamespaceGuard {
            origin: url::Url::parse(&self.binding.destination.namespace)?,
        }
        .validate(uri)?;
        let after = uri.split_once("://").ok_or("Invalid job write URI")?.1;
        let path = after
            .find('/')
            .map(|start| &after[start..])
            .ok_or("Missing job write path")?;
        let (path, query) = path.split_once('?').unwrap_or((path, ""));
        if !query.is_empty() && query != "x-id=PutObject" {
            return Err("Unexpected job mutation query".into());
        }
        if urlencoding::decode(path)?.as_ref()
            != format!("/{}/{}", self.binding.destination.bucket, self.key)
        {
            return Err("Job mutation resolved outside its exact approved key".into());
        }
        if self.cancel.load(Ordering::Relaxed)
            || !self.binding.attempt.healthy()
            || self
                .first_deadline
                .is_some_and(|deadline| deadline <= Instant::now())
        {
            return Err("Job mutation cancelled, expired or blocked by journal storage".into());
        }
        self.binding
            .validate()
            .map_err(|_| "Native job identity changed before transmission")?;
        if self.budget.check(&self.cancel).is_err()
            || !self.binding.attempt.healthy()
            || self
                .first_deadline
                .is_some_and(|deadline| deadline <= Instant::now())
        {
            return Err(
                "Job execution budget/cancellation/approval expired before transmission".into(),
            );
        }
        Ok(())
    }
}
impl Intercept for WriterInterceptor {
    fn name(&self) -> &'static str {
        "FurmanScopedCreateOnlyJobWriter"
    }
    fn modify_before_signing(
        &self,
        c: &mut BeforeTransmitInterceptorContextMut<'_>,
        _: &RuntimeComponents,
        b: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        self.0.check(c, b)
    }
    fn modify_before_transmit(
        &self,
        c: &mut BeforeTransmitInterceptorContextMut<'_>,
        _: &RuntimeComponents,
        b: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        self.0.check(c, b)?;
        if self.0.sent.swap(true, Ordering::Relaxed) {
            return Err("A job PUT cannot be replayed".into());
        }
        Ok(())
    }
}
fn writer(
    binding: Arc<Binding>,
    key: String,
    cancel: Arc<AtomicBool>,
    first_deadline: Option<Instant>,
    budget: Arc<ScanBudget>,
) -> Result<(Client, Arc<ScopedWriter>), FmError> {
    if !matches!(binding.source, BoundSource::Local(_)) {
        return Err(error(
            "Only retained local-root job sources can derive an initial writer",
        ));
    }
    if !crate::s3::compat::conditional_writes_supported(&binding.destination.client) {
        return Err(error(crate::s3::compat::CONDITION_ERROR));
    }
    let original = &binding.destination.client;
    let guards = original
        .config()
        .interceptors()
        .filter(|i| i.name() == "FurmanReadOnlyJobNamespace")
        .count();
    if guards != 1 {
        return Err(error("Cannot derive an unambiguous native job writer"));
    }
    let retained: Vec<_> = original
        .config()
        .interceptors()
        .filter(|i| i.name() != "FurmanReadOnlyJobNamespace")
        .collect();
    let scope = Arc::new(ScopedWriter {
        binding: binding.clone(),
        key,
        cancel,
        sent: AtomicBool::new(false),
        first_deadline,
        budget,
    });
    let mut builder = original.config().to_builder();
    builder.set_interceptors(retained);
    let client = Client::from_conf(
        builder
            .retry_config(RetryConfig::disabled())
            .interceptor(WriterInterceptor(scope.clone()))
            .build(),
    );
    Ok((client, scope))
}

pub async fn start(state: &JobRunState, request: ExecuteRequest) -> Result<RunStatus, FmError> {
    if !request.approved || !request.acknowledge_exclusions {
        return Err(error("Separate explicit approval of all exact mappings, create-only writes, verification and attribute exclusions is required"));
    }
    state.status(&request.run_id)?;
    let (approval, cancel) = {
        let mut inner = state.0.lock().map_err(|_| error("Job state unavailable"))?;
        let slot = inner
            .slot
            .as_mut()
            .filter(|s| {
                s.id == request.run_id
                    && s.phase == Phase::AwaitingApproval
                    && s.expires > Instant::now()
                    && !s.cancel.load(Ordering::Relaxed)
            })
            .ok_or_else(|| {
                error("Execution approval expired, was cancelled, or was already consumed")
            })?;
        let approval = slot
            .approval
            .as_ref()
            .filter(|a| a.id == request.approval_id)
            .ok_or_else(|| error("Wrong/cross-run execution approval"))?;
        approval.validate(&request.run_id, slot.expires)?;
        let approval = slot.approval.take().unwrap();
        slot.phase = Phase::Executing;
        slot.manifest = None;
        slot.preview = None;
        if let Some(timer) = slot.expiry_abort.take() {
            timer.abort();
        }
        let counts = Execution {
            selected_files: approval.plan.items.len() as u64,
            skipped_files: approval.plan.skipped as u64,
            selected_bytes: approval.plan.bytes,
            ..Execution::default()
        };
        slot.execution = Some(ExecutionResult {
            status: Status::Executing,
            counts,
            outcome_recorded: false,
            message: Some("Native execution admitted; intent must commit before any PUT".into()),
        });
        (approval, slot.cancel.clone())
    };
    let task_state = state.clone();
    let id = request.run_id.clone();
    tokio::spawn(async move {
        run(task_state, id, approval, cancel).await;
    });
    let inner = state.0.lock().map_err(|_| error("Job state unavailable"))?;
    Ok(inner
        .slot
        .as_ref()
        .filter(|s| s.id == request.run_id)
        .ok_or_else(|| error("Execution state replaced"))?
        .view())
}
fn publish(state: &JobRunState, id: &str, counts: &Execution) {
    if let Ok(mut inner) = state.0.lock() {
        if let Some(slot) = inner
            .slot
            .as_mut()
            .filter(|s| s.id == id && s.phase == Phase::Executing)
        {
            if let Some(result) = &mut slot.execution {
                result.counts = counts.clone();
            }
        }
    }
}
fn finish(
    state: &JobRunState,
    id: &str,
    binding: &Binding,
    status: Status,
    counts: Execution,
    message: Option<String>,
) {
    let recorded = binding.attempt.execution(status, counts.clone());
    let message=match recorded.as_ref(){Ok(())=>message,Err(failure)=>Some(format!("{}\nOutcome could not be recorded. Inspect the destination; writes may have committed. No further writes were started.\n{failure}",message.unwrap_or_default()))};
    let status = if recorded.is_err() {
        if counts.attempted_files > 0 {
            Status::Uncertain
        } else {
            Status::Failed
        }
    } else {
        status
    };
    if let Ok(mut inner) = state.0.lock() {
        if let Some(slot) = inner.slot.as_mut().filter(|s| s.id == id) {
            slot.phase = Phase::Stopped;
            slot.manifest = None;
            slot.preview = None;
            slot.approval = None;
            slot.cancel.store(true, Ordering::Relaxed);
            slot.outcome = Some(match status {
                Status::Succeeded | Status::Skipped => super::ScanOutcome::Complete,
                Status::Cancelled => super::ScanOutcome::Cancelled,
                Status::Limited => super::ScanOutcome::Limited,
                _ => super::ScanOutcome::Failed,
            });
            slot.message = message.clone();
            slot.execution = Some(ExecutionResult {
                status,
                counts,
                outcome_recorded: recorded.is_ok(),
                message,
            });
            slot.binding = None;
            slot.attempt = None;
        }
    }
}
struct ExecutionGuard {
    state: JobRunState,
    id: String,
    binding: Arc<Binding>,
    finished: bool,
}
impl Drop for ExecutionGuard {
    fn drop(&mut self) {
        if !self.finished {
            let mut counts = self
                .state
                .0
                .lock()
                .ok()
                .and_then(|inner| {
                    inner
                        .slot
                        .as_ref()
                        .and_then(|s| s.execution.as_ref().map(|r| r.counts.clone()))
                })
                .unwrap_or_default();
            counts.uncertain_files = counts
                .uncertain_files
                .max(counts.attempted_files.saturating_sub(counts.verified_files));
            finish(&self.state,&self.id,&self.binding,Status::Interrupted,counts,Some("Execution task interrupted; sent writes may have committed. Inspect the destination and explicitly review; nothing resumes.".into()));
        }
    }
}
async fn run(state: JobRunState, id: String, approval: Approval, cancel: Arc<AtomicBool>) {
    let binding = approval.binding.clone();
    let mut guard = ExecutionGuard {
        state: state.clone(),
        id: id.clone(),
        binding: binding.clone(),
        finished: false,
    };
    let mut counts = Execution {
        selected_files: approval.plan.items.len() as u64,
        skipped_files: approval.plan.skipped as u64,
        selected_bytes: approval.plan.bytes,
        ..Execution::default()
    };
    let mut uncertain = false;
    let budget = Arc::new(ScanBudget::with_limits(binding.limits));
    let result = async {
        approval.validate(&id, approval.expires)?;
        binding.attempt.execution(Status::Executing, counts.clone())?;
        for item in &approval.plan.items {
            binding.validate()?;
            budget.object(&cancel)?;
            missing(&binding, &item.key, &budget, &cancel).await?;
            source_matches(&binding, item, &budget, &cancel).await?;
            let bytes = snapshot_bytes(
                binding.clone(),
                item.snapshot.file.try_clone().map_err(|_| error("Snapshot descriptor unavailable"))?,
                item.snapshot.size,
                item.snapshot.sha256.clone(),
                cancel.clone(),
            ).await?;
            budget.check(&cancel)?;
            counts.attempted_files += 1;
            publish(&state, &id, &counts);
            // This barrier is mandatory even when source/destination reads passed.
            binding.attempt.execution(Status::Executing, counts.clone())?;
            let (client, scope) = writer(
                binding.clone(), item.key.clone(), cancel.clone(),
                if counts.attempted_files == 1 { Some(approval.expires) } else { None }, budget.clone(),
            )?;
            let a = &item.snapshot.attributes;
            let mut put = client.put_object()
                .bucket(&binding.destination.bucket).key(&item.key).if_none_match("*")
                .body(aws_sdk_s3::primitives::ByteStream::from(bytes))
                .set_checksum_algorithm(crate::s3::compat::upload_checksum(&client))
                .set_metadata(Some(a.metadata.clone())).set_content_type(a.content_type.clone())
                .set_cache_control(a.cache_control.clone()).set_content_disposition(a.disposition.clone())
                .set_content_encoding(a.encoding.clone()).set_content_language(a.language.clone());
            if a.aes256 && !binding.job.destination.configuration.sse_customer_key_enabled {
                put = put.server_side_encryption(aws_sdk_s3::types::ServerSideEncryption::Aes256);
            }
            let sent = budget.request(&cancel, async {
                put.send().await.map_err(|failure| job_sdk_error("Job conditional create-only PUT", &failure))
            }).await;
            // A transmitted error/timeout/cancellation is conservatively uncertain,
            // including provider rejection; never replay a mutation to find out.
            uncertain = scope.sent.load(Ordering::Relaxed);
            let response = sent?;
            counts.acknowledged_files += 1;
            publish(&state, &id, &counts);
            binding.attempt.execution(Status::Executing, counts.clone())?;
            uncertain = true;
            let etag = response.e_tag().filter(|e| !e.is_empty() && e.len() <= 1024)
                .ok_or_else(|| error("PUT acknowledged without a bounded ETag; mandatory verification cannot be pinned"))?;
            let (digest, actual) = read_object(&binding, &item.key, item.snapshot.size, etag, &budget, &cancel).await?;
            let sse = binding.job.destination.configuration.sse_customer_key_enabled;
            if digest != item.snapshot.sha256 || actual.metadata != a.metadata
                || a.content_type.as_ref().is_some_and(|v| actual.content_type.as_ref() != Some(v))
                || actual.cache_control != a.cache_control || actual.disposition != a.disposition
                || actual.encoding != a.encoding || actual.language != a.language
                || actual.customer_encrypted != sse || (a.aes256 && !sse && !actual.aes256) {
                return Err(error("Written object failed mandatory SHA-256/header/encryption verification; it was not rolled back"));
            }
            uncertain = false;
            counts.verified_files += 1;
            counts.verified_bytes += item.snapshot.size;
            publish(&state, &id, &counts);
            binding.attempt.execution(Status::Executing, counts.clone())?;
        }
        Ok::<_, FmError>(())
    }.await;
    let (status, message) = match result {
        Ok(()) => (
            if counts.skipped_files > 0 {
                Status::Skipped
            } else {
                Status::Succeeded
            },
            None,
        ),
        Err(failure) => {
            if uncertain || counts.acknowledged_files > counts.verified_files {
                counts.uncertain_files = counts.uncertain_files.max(1);
            }
            (
                if counts.uncertain_files > 0 {
                    Status::Uncertain
                } else if counts.verified_files > 0 {
                    Status::Partial
                } else if matches!(failure, FmError::ScanCancelled) {
                    Status::Cancelled
                } else if matches!(failure, FmError::ScanLimited(_)) {
                    Status::Limited
                } else {
                    Status::Failed
                },
                Some(failure.to_string()),
            )
        }
    };
    finish(&state, &id, &binding, status, counts, message);
    guard.finished = true;
}
