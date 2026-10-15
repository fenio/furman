//! Bounded, session-only SDK activity. Deliberately never inspect inputs, URLs,
//! bodies, authorization, encryption headers, credentials or error Debug text.
#[cfg(test)]
#[path = "activity_tests.rs"]
mod tests;
use aws_sdk_s3::config::Intercept;
use aws_smithy_runtime_api::{
    box_error::BoxError,
    client::{
        interceptors::context::{
            BeforeSerializationInterceptorContextRef, BeforeTransmitInterceptorContextMut,
            FinalizerInterceptorContextRef,
        },
        orchestrator::Metadata,
        runtime_components::RuntimeComponents,
    },
};
use aws_smithy_types::config_bag::{ConfigBag, Storable, StoreReplace};
use serde::Serialize;
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc, LazyLock, Mutex,
    },
    time::{Instant, SystemTime, UNIX_EPOCH},
};

const CAPACITY: usize = 500;

#[derive(Clone, Debug, Serialize)]
pub struct Activity {
    pub id: u64,
    pub started_ms: u64,
    pub operation: String,
    pub duration_ms: u64,
    pub outcome: &'static str,
    pub http_status: Option<u16>,
    pub request_id: Option<String>,
    pub attempts: u32,
    pub retries: u32,
}

#[derive(Default)]
struct History {
    generation: u64,
    sequence: u64,
    evicted: u64,
    records: VecDeque<Activity>,
}

impl History {
    fn push(&mut self, generation: u64, mut activity: Activity) {
        if generation != self.generation {
            return;
        }
        self.sequence = self.sequence.saturating_add(1);
        activity.id = self.sequence;
        if self.records.len() == CAPACITY {
            self.records.pop_front();
            self.evicted = self.evicted.saturating_add(1);
        }
        self.records.push_back(activity);
    }
    fn clear(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.records.clear();
        self.evicted = 0;
    }
}

static HISTORY: LazyLock<Arc<Mutex<History>>> =
    LazyLock::new(|| Arc::new(Mutex::new(History::default())));

#[derive(Serialize)]
pub struct ActivitySnapshot {
    pub schema_version: u32,
    pub app_version: &'static str,
    pub capacity: usize,
    pub evicted: u64,
    pub records: Vec<Activity>,
}

pub fn snapshot() -> ActivitySnapshot {
    let history = HISTORY.lock().unwrap_or_else(|error| error.into_inner());
    ActivitySnapshot {
        schema_version: 1,
        app_version: env!("CARGO_PKG_VERSION"),
        capacity: CAPACITY,
        evicted: history.evicted,
        records: history.records.iter().rev().cloned().collect(),
    }
}

pub fn clear() {
    HISTORY
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clear();
}

struct Trace {
    history: Arc<Mutex<History>>,
    generation: u64,
    started: Instant,
    started_ms: u64,
    operation: Mutex<String>,
    attempts: AtomicU32,
    finished: AtomicBool,
}

impl Trace {
    fn set_operation(&self, config: &ConfigBag) {
        if let Some(operation) = config.load::<Metadata>().map(|metadata| metadata.name()) {
            if operation.len() <= 64 && operation.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
                if let Ok(mut name) = self.operation.lock() {
                    *name = operation.into();
                }
            }
        }
    }
    fn finish(&self, outcome: &'static str, http_status: Option<u16>, request_id: Option<String>) {
        if self.finished.swap(true, Ordering::Relaxed) {
            return;
        }
        let attempts = self.attempts.load(Ordering::Relaxed);
        if let Ok(mut history) = self.history.lock() {
            history.push(
                self.generation,
                Activity {
                    id: 0,
                    started_ms: self.started_ms,
                    operation: self
                        .operation
                        .lock()
                        .map(|name| name.clone())
                        .unwrap_or_else(|_| "S3Request".into()),
                    duration_ms: self.started.elapsed().as_millis().min(u64::MAX as u128) as u64,
                    outcome,
                    http_status,
                    request_id,
                    attempts,
                    retries: attempts.saturating_sub(1),
                },
            );
        }
    }
}
impl Drop for Trace {
    fn drop(&mut self) {
        self.finish("abandoned", None, None);
    }
}

#[derive(Clone)]
struct TraceState(Arc<Trace>);
impl std::fmt::Debug for TraceState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("FurmanActivityTrace([history omitted])")
    }
}
impl Storable for TraceState {
    type Storer = StoreReplace<Self>;
}

pub struct ActivityInterceptor {
    history: Arc<Mutex<History>>,
}
impl std::fmt::Debug for ActivityInterceptor {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("FurmanActivity([history omitted])")
    }
}
pub fn interceptor() -> ActivityInterceptor {
    ActivityInterceptor {
        history: HISTORY.clone(),
    }
}

fn identifier(value: &str) -> Option<String> {
    // Request IDs are provider-controlled. Reject unexpected formats rather
    // than retaining arbitrary diagnostic strings. Never accept entire headers.
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_./+=".contains(&byte))
    {
        None
    } else {
        Some(value.into())
    }
}

impl Intercept for ActivityInterceptor {
    fn name(&self) -> &'static str {
        "FurmanActivity"
    }
    fn read_before_execution(
        &self,
        _context: &BeforeSerializationInterceptorContextRef<'_>,
        config: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        let generation = self
            .history
            .lock()
            .map(|history| history.generation)
            .unwrap_or_default();
        config
            .interceptor_state()
            .store_put(TraceState(Arc::new(Trace {
                history: self.history.clone(),
                generation,
                started: Instant::now(),
                started_ms: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
                    .min(u64::MAX as u128) as u64,
                operation: Mutex::new("S3Request".into()),
                attempts: AtomicU32::new(0),
                finished: AtomicBool::new(false),
            })));
        Ok(())
    }
    fn modify_before_transmit(
        &self,
        _context: &mut BeforeTransmitInterceptorContextMut<'_>,
        _runtime: &RuntimeComponents,
        config: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        if let Some(trace) = config.load::<TraceState>() {
            trace.0.set_operation(config);
            trace.0.attempts.fetch_add(1, Ordering::Relaxed);
        }
        Ok(())
    }
    fn read_after_execution(
        &self,
        context: &FinalizerInterceptorContextRef<'_>,
        _runtime: &RuntimeComponents,
        config: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        if let Some(trace) = config.load::<TraceState>() {
            trace.0.set_operation(config);
            let outcome = match context.output_or_error() {
                Some(Ok(_)) => "success",
                Some(Err(error)) if error.is_timeout_error() => "timeout",
                _ => "failed",
            };
            let status = context
                .response()
                .map(|response| response.status().as_u16());
            let request_id = context
                .response()
                .and_then(|response| response.headers().get("x-amz-request-id"))
                .and_then(identifier);
            trace.0.finish(outcome, status, request_id);
        }
        Ok(())
    }
}
