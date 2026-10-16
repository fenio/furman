//! Guarded single-shot and multipart payload execution.
//! Read clients remain read-only; only a consumed native approval derives a
//! per-key conditional writer. No unconditional/delete fallback exists.
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

pub(super) mod multipart;
#[cfg(test)]
mod multipart_tests;
#[cfg(test)]
mod replacement_tests;
#[cfg(test)]
mod s3_tests;
#[cfg(test)]
mod tests;

// Only these static native diagnoses may escape SDK source chains. Never format
// arbitrary interceptor/transport sources (which can carry URLs or credentials).
pub(super) const GUARD_ERRORS: &[&str] = &[
    "Multipart job request escaped its exact native operation, key, upload or condition",
    "A multipart job mutation cannot be replayed",
    "Job writer requires its exact native If-Match or If-None-Match PUT condition",
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
    #[serde(default)]
    pub acknowledge_replacements: bool,
    #[serde(default)]
    pub acknowledge_multipart: bool,
}
#[derive(Serialize)]
pub struct Review {
    pub approval_id: String,
    pub preview: preview::Preview,
    pub source: String,
    pub destination: String,
    pub exclusions: Vec<String>,
    pub destination_sse_c: bool,
    pub s3_source: bool,
    pub preserved_tag_count: usize,
    pub source_aes256_files: usize,
    pub source_sse_c_files: usize,
    pub replacement_files: usize,
    pub destination_aes256_files: usize,
    pub multipart_files: usize,
    pub multipart_parts: u64,
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
            multipart_files: self
                .plan
                .items
                .iter()
                .filter(|item| multipart::needed(item.snapshot.size))
                .count(),
            multipart_parts: self
                .plan
                .items
                .iter()
                .filter(|item| multipart::needed(item.snapshot.size))
                .map(|item| multipart::parts(item.snapshot.size))
                .sum(),
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
            s3_source: matches!(self.binding.source, BoundSource::S3(_)),
            preserved_tag_count: self
                .plan
                .items
                .iter()
                .filter_map(|item| super::replacement::write_controls(&self.binding, item))
                .map(|controls| controls.tags.len())
                .sum(),
            source_aes256_files: self
                .plan
                .items
                .iter()
                .filter(|item| item.snapshot.attributes.aes256)
                .count(),
            replacement_files: self
                .plan
                .items
                .iter()
                .filter(|item| super::replacement::is_replacement(item))
                .count(),
            destination_aes256_files: self
                .plan
                .items
                .iter()
                .filter(|item| {
                    item.destination
                        .as_ref()
                        .is_some_and(|dest| dest.attributes.aes256)
                })
                .count(),
            source_sse_c_files: self
                .plan
                .items
                .iter()
                .filter(|item| item.snapshot.attributes.customer_encrypted)
                .count(),
        }
    }
    pub fn validate(&self, run: &str, expires: Instant) -> Result<(), FmError> {
        self.binding.validate()?;
        self.plan
            .check_owner(&self.binding, run, &self.manifest, expires)?;
        if let Some(reason) = self.plan.execution_blocker() {
            return Err(error(&reason));
        }
        if matches!(self.binding.source, BoundSource::S3(_))
            && self.plan.items.iter().any(|item| item.controls.is_none())
        {
            return Err(error(
                "Native source ACL/tag preservation review is missing",
            ));
        }
        if self.plan.items.iter().any(|item| {
            super::replacement::is_replacement(item)
                && item
                    .destination
                    .as_ref()
                    .is_none_or(|destination| destination.controls.is_none())
        }) {
            return Err(error("Native destination replacement review is missing"));
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
async fn check_snapshot(
    binding: Arc<Binding>,
    file: File,
    size: u64,
    expected: String,
    cancel: Arc<AtomicBool>,
    budget: Arc<ScanBudget>,
) -> Result<(), FmError> {
    let mut hash = Sha256::new();
    for offset in (0..size).step_by(multipart::PART_BYTES as usize) {
        let bytes = multipart::read_chunk(
            binding.clone(),
            file.try_clone()
                .map_err(|_| error("Snapshot descriptor unavailable"))?,
            offset,
            multipart::PART_BYTES.min(size - offset),
            cancel.clone(),
            budget.clone(),
        )
        .await?;
        hash.update(&bytes);
    }
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
    Ok(())
}
async fn read_object(
    endpoint: &super::Endpoint,
    key: &str,
    size: u64,
    etag: &str,
    budget: &ScanBudget,
    cancel: &AtomicBool,
) -> Result<(String, Attributes), FmError> {
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
) -> Result<Option<super::controls::Controls>, FmError> {
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
        BoundSource::S3(endpoint) => {
            let SourcePin::S3 { pin } = &binding.job.source else {
                return Err(error("Native source kind changed"));
            };
            let key = format!("{}{}", pin.prefix, item.entry.row.relative_path);
            let (digest, attributes) = read_object(
                endpoint,
                &key,
                expected.size,
                expected
                    .etag
                    .as_deref()
                    .ok_or_else(|| error("Missing native source validator"))?,
                budget,
                cancel,
            )
            .await?;
            if digest != expected.sha256 || attributes != item.snapshot.attributes {
                return Err(error("Source payload, version or supported attributes changed after preparation; no approval can be reused"));
            }
            super::controls::require_unlocked_source(endpoint, budget, cancel).await?;
            let controls = super::controls::read(
                endpoint,
                &key,
                attributes.version.as_deref(),
                budget,
                cancel,
            )
            .await?;
            if attributes
                .tag_count
                .is_some_and(|count| count as usize != controls.tags.len())
                || item
                    .controls
                    .as_ref()
                    .is_some_and(|expected| expected != &controls)
            {
                return Err(error("Source ACL/tags changed after native execution review; compare and prepare again"));
            }
            if attributes.content_type.is_none() {
                return Err(error(
                    "S3 source Content-Type is missing; the job writer cannot silently invent one",
                ));
            }
            return Ok(Some(controls));
        }
    }
    Ok(None)
}

pub async fn review(state: &JobRunState, request: ReviewRequest) -> Result<RunStatus, FmError> {
    let status = state.status(&request.run_id)?;
    if status.phase != "prepared" {
        return Err(error(
            "A fresh checked preview is required for execution review",
        ));
    }
    let (binding, mut plan, manifest, expires, cancel) = {
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
        if matches!(binding.source, BoundSource::S3(_))
            || plan.items.iter().any(|item| {
                super::replacement::is_replacement(item) || multipart::needed(item.snapshot.size)
            })
        {
            let minimum_calls = plan
                .items
                .iter()
                .map(|item| super::replacement::minimum_calls(&binding, item))
                .sum::<u64>();
            if minimum_calls > binding.limits.max_requests {
                return Err(error(&format!("Selected execution needs at least {minimum_calls} logical S3 calls, exceeding the reviewed {}-call execution budget. Prepare fewer files or explicitly review higher limits in a fresh bound run; no approval or write was admitted.",binding.limits.max_requests)));
            }
        }
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
        for item in &mut plan.items {
            binding.validate()?;
            budget.object(&cancel)?;
            if super::replacement::is_replacement(item) {
                item.destination =
                    Some(super::replacement::check(&binding, item, &budget, &cancel).await?);
            } else {
                missing(&binding, &item.key, &budget, &cancel).await?;
            }
            item.controls = source_matches(&binding, item, &budget, &cancel).await?;
            check_snapshot(
                binding.clone(),
                item.snapshot
                    .file
                    .try_clone()
                    .map_err(|_| error("Snapshot descriptor unavailable"))?,
                item.snapshot.size,
                item.snapshot.sha256.clone(),
                cancel.clone(),
                budget.clone(),
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
            if let Ok(mut inner) = state.0.lock() {
                if let Some(slot) = inner
                    .slot
                    .as_mut()
                    .filter(|slot| slot.id == request.run_id && slot.phase == Phase::Reviewing)
                {
                    slot.progress = budget.progress();
                }
            }
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
    expected: crate::s3::writes::DestinationExpectation,
}
#[derive(Debug)]
struct WriterInterceptor(Arc<ScopedWriter>);
impl std::fmt::Debug for ScopedWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ScopedConditionalJobWriter([private])")
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
        let condition = match &self.expected {
            crate::s3::writes::DestinationExpectation::Missing => {
                request.headers().get("if-none-match") == Some("*")
                    && request.headers().get("if-match").is_none()
            }
            crate::s3::writes::DestinationExpectation::Match { etag } => {
                request.headers().get("if-match") == Some(etag.as_str())
                    && request.headers().get("if-none-match").is_none()
            }
        };
        if operation != "PutObject" || request.method() != "PUT" || !condition {
            return Err(
                "Job writer requires its exact native If-Match or If-None-Match PUT condition"
                    .into(),
            );
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
        "FurmanScopedConditionalJobWriter"
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
#[cfg(test)]
fn writer(
    binding: Arc<Binding>,
    key: String,
    cancel: Arc<AtomicBool>,
    first_deadline: Option<Instant>,
    budget: Arc<ScanBudget>,
) -> Result<(Client, Arc<ScopedWriter>), FmError> {
    writer_for(
        binding,
        key,
        cancel,
        first_deadline,
        budget,
        crate::s3::writes::DestinationExpectation::Missing,
    )
}
fn writer_for(
    binding: Arc<Binding>,
    key: String,
    cancel: Arc<AtomicBool>,
    first_deadline: Option<Instant>,
    budget: Arc<ScanBudget>,
    expected: crate::s3::writes::DestinationExpectation,
) -> Result<(Client, Arc<ScopedWriter>), FmError> {
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
        expected,
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
        return Err(error("Separate explicit approval of all exact mappings, guarded writes, verification and attribute exclusions is required"));
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
        if approval
            .plan
            .items
            .iter()
            .any(|item| multipart::needed(item.snapshot.size))
            && !request.acknowledge_multipart
        {
            return Err(error("Separate multipart acknowledgement is required: staging can leave billable unfinished uploads; cleanup is bounded and restart never resumes or reconnects"));
        }
        if approval
            .plan
            .items
            .iter()
            .any(super::replacement::is_replacement)
            && !request.acknowledge_replacements
        {
            return Err(error("Separate explicit acknowledgement of existing-object replacements is required; preparation and create approvals cannot authorize replacement"));
        }
        let approval = slot.approval.take().unwrap();
        slot.phase = Phase::Executing;
        slot.manifest = None;
        slot.preview = None;
        if let Some(timer) = slot.expiry_abort.take() {
            timer.abort();
        }
        let counts = Execution {
            multipart_files: approval
                .plan
                .items
                .iter()
                .filter(|item| multipart::needed(item.snapshot.size))
                .count() as u64,
            replacement_files: approval
                .plan
                .items
                .iter()
                .filter(|item| super::replacement::is_replacement(item))
                .count() as u64,
            selected_files: approval.plan.items.len() as u64,
            skipped_files: approval.plan.skipped as u64,
            selected_bytes: approval.plan.bytes,
            ..Execution::default()
        };
        slot.execution = Some(ExecutionResult {
            status: Status::Executing,
            counts,
            outcome_recorded: false,
            message: Some("Native execution admitted; intent must commit before any payload/staging mutation".into()),
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
            counts.uncertain_replacements = counts.uncertain_replacements.max(
                counts
                    .attempted_replacements
                    .saturating_sub(counts.verified_replacements),
            );
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
        multipart_files: approval
            .plan
            .items
            .iter()
            .filter(|item| multipart::needed(item.snapshot.size))
            .count() as u64,
        replacement_files: approval
            .plan
            .items
            .iter()
            .filter(|item| super::replacement::is_replacement(item))
            .count() as u64,
        selected_files: approval.plan.items.len() as u64,
        skipped_files: approval.plan.skipped as u64,
        selected_bytes: approval.plan.bytes,
        ..Execution::default()
    };
    let mut uncertain = false;
    let mut current_replacement = false;
    let budget = Arc::new(ScanBudget::with_limits(binding.limits));
    let result = async {
        approval.validate(&id, approval.expires)?;
        binding.attempt.execution(Status::Executing, counts.clone())?;
        for item in &approval.plan.items {
            binding.validate()?;
            budget.object(&cancel)?;
            current_replacement=super::replacement::is_replacement(item);
            if current_replacement{super::replacement::check(&binding,item,&budget,&cancel).await?;}else{missing(&binding, &item.key, &budget, &cancel).await?;}
            source_matches(&binding, item, &budget, &cancel).await?;
            let large=multipart::needed(item.snapshot.size);
            let bytes = if large {check_snapshot(
                binding.clone(),item.snapshot.file.try_clone().map_err(|_|error("Snapshot descriptor unavailable"))?,item.snapshot.size,item.snapshot.sha256.clone(),cancel.clone(),budget.clone()
            ).await?;None}else{Some(snapshot_bytes(
                binding.clone(),
                item.snapshot.file.try_clone().map_err(|_| error("Snapshot descriptor unavailable"))?,
                item.snapshot.size,
                item.snapshot.sha256.clone(),
                cancel.clone(),
            ).await?)};
            budget.check(&cancel)?;
            counts.attempted_files += 1;
            if current_replacement{counts.attempted_replacements+=1;}
            publish(&state, &id, &counts);
            // This barrier is mandatory even when source/destination reads passed.
            binding.attempt.execution(Status::Executing, counts.clone())?;
            let attributes=super::replacement::write_attributes(&binding,item)?;
            let a = &attributes;
            let (etag,version)=if large {
                multipart::upload(binding.clone(),item,a,cancel.clone(),if counts.attempted_files==1{Some(approval.expires)}else{None},budget.clone(),&mut uncertain).await?
            } else {
            let (client, scope) = writer_for(
                binding.clone(), item.key.clone(), cancel.clone(),
                if counts.attempted_files == 1 { Some(approval.expires) } else { None }, budget.clone(),item.expected.clone(),
            )?;
            let mut put = client.put_object()
                .bucket(&binding.destination.bucket).key(&item.key)
                .body(aws_sdk_s3::primitives::ByteStream::from(bytes.ok_or_else(||error("Missing checked small snapshot"))?))
                .set_checksum_algorithm(crate::s3::compat::upload_checksum(&client))
                .set_metadata(Some(a.metadata.clone())).set_content_type(a.content_type.clone())
                .set_cache_control(a.cache_control.clone()).set_content_disposition(a.disposition.clone())
                .set_content_encoding(a.encoding.clone()).set_content_language(a.language.clone());
            match &item.expected{crate::s3::writes::DestinationExpectation::Missing=>{put=put.if_none_match("*");},crate::s3::writes::DestinationExpectation::Match{etag}=>{put=put.if_match(etag);}}
            if let Some(controls)=super::replacement::write_controls(&binding,item) {
                put=put.set_tagging(controls.tagging()).storage_class(aws_sdk_s3::types::StorageClass::Standard);
            }
            if a.aes256 && !binding.job.destination.configuration.sse_customer_key_enabled {
                put = put.server_side_encryption(aws_sdk_s3::types::ServerSideEncryption::Aes256);
            }
            let sent = budget.request(&cancel, async {
                put.send().await.map_err(|failure| job_sdk_error("Job conditional payload PUT", &failure))
            }).await;
            // A transmitted error/timeout/cancellation is conservatively uncertain,
            // including provider rejection; never replay a mutation to find out.
            uncertain = scope.sent.load(Ordering::Relaxed);
            let response = sent?;
            (response.e_tag().map(str::to_owned),response.version_id().map(str::to_owned))
            };
            counts.acknowledged_files += 1;
            if current_replacement{counts.acknowledged_replacements+=1;}
            publish(&state, &id, &counts);
            binding.attempt.execution(Status::Executing, counts.clone())?;
            uncertain = true;
            let etag = etag.as_deref().filter(|e| !e.is_empty() && e.len() <= 1024)
                .ok_or_else(|| error("Payload write acknowledged without a bounded ETag; mandatory verification cannot be pinned"))?;
            let (digest, actual) = read_object(&binding.destination, &item.key, item.snapshot.size, etag, &budget, &cancel).await?;
            if actual.version!=version{return Err(error("Written object version changed before verification; no success was recorded"));}
            if (current_replacement||large)&&actual.unsupported.is_some(){return Err(error("Written object has unreviewed or unsupported encryption/retention/storage attributes; inspect the destination. No success or rollback was recorded"));}
            let sse = binding.job.destination.configuration.sse_customer_key_enabled;
            if !actual.complete || digest != item.snapshot.sha256 || actual.metadata != a.metadata
                || a.content_type.as_ref().is_some_and(|v| actual.content_type.as_ref() != Some(v))
                || actual.cache_control != a.cache_control || actual.disposition != a.disposition
                || actual.encoding != a.encoding || actual.language != a.language
                || actual.customer_encrypted != sse || (a.aes256 && !sse && !actual.aes256) {
                return Err(error("Written object failed mandatory SHA-256/header/encryption verification; it was not rolled back"));
            }
            if let Some(expected)=super::replacement::write_controls(&binding,item) {
                if !actual.standard_storage{return Err(error("Written object did not retain STANDARD storage; no success was recorded"));}
                let actual_controls=super::controls::read(&binding.destination,&item.key,actual.version.as_deref(),&budget,&cancel).await?;
                if actual_controls.tags!=expected.tags||actual.tag_count.is_some_and(|count|count as usize!=actual_controls.tags.len()){return Err(error("Written object tags failed native verification; it was not rolled back"));}
            }
            uncertain = false;
            counts.verified_files += 1;
            if current_replacement{counts.verified_replacements+=1;}
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
                if current_replacement {
                    counts.uncertain_replacements = counts.uncertain_replacements.max(1);
                }
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
