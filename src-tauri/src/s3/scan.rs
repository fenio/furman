//! Shared, operation-scoped budgets. Enumeration never returns a partial success.
use super::errors::s3_sdk_error;
use crate::models::{FmError, ProgressEvent};
use aws_sdk_s3::{types::Object, Client};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::RwLock;
use std::time::Duration;
use tokio::time::Instant;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct ScanLimits {
    pub max_objects: u64,
    pub max_requests: u64,
    pub max_seconds: u64,
    pub max_results: u64,
}

impl ScanLimits {
    pub const DEFAULT: Self = Self {
        max_objects: 100_000,
        max_requests: 1000,
        max_seconds: 300,
        max_results: 1000,
    };

    pub fn validate(&self) -> Result<(), FmError> {
        if !(1..=10_000_000).contains(&self.max_objects)
            || !(1..=1_000_000).contains(&self.max_requests)
            || !(1..=86_400).contains(&self.max_seconds)
            || !(1..=10_000).contains(&self.max_results)
        {
            return Err(FmError::Other("Invalid scan limits: objects 1–10,000,000; requests 1–1,000,000; seconds 1–86,400; search results 1–10,000".into()));
        }
        Ok(())
    }
}

static LIMITS: RwLock<ScanLimits> = RwLock::new(ScanLimits::DEFAULT);

pub fn set_limits(limits: ScanLimits) -> Result<(), FmError> {
    limits.validate()?;
    *LIMITS.write().map_err(|e| FmError::Other(e.to_string()))? = limits;
    Ok(())
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ScanOutcome {
    Complete,
    Cancelled,
    Limited,
    Failed,
}

pub fn outcome(result: &Result<(), FmError>) -> ScanOutcome {
    match result {
        Ok(()) => ScanOutcome::Complete,
        Err(FmError::ScanCancelled) => ScanOutcome::Cancelled,
        Err(FmError::ScanLimited(_)) => ScanOutcome::Limited,
        Err(_) => ScanOutcome::Failed,
    }
}

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize)]
pub struct ScanProgress {
    pub scanned: u64,
    pub matched: u64,
    pub requests: u64,
    pub bytes_read: u64,
    pub s3_bytes_read: u64,
    pub elapsed_ms: u64,
}

pub struct ScanBudget {
    pub limits: ScanLimits,
    started: Instant,
    scanned: AtomicU64,
    requests: AtomicU64,
    matched: AtomicU64,
    bytes_read: AtomicU64,
    s3_bytes_read: AtomicU64,
}

impl ScanBudget {
    pub fn new() -> Self {
        let limits = *LIMITS.read().unwrap_or_else(|e| e.into_inner());
        Self::with_limits(limits)
    }

    pub fn with_limits(limits: ScanLimits) -> Self {
        Self {
            limits,
            started: Instant::now(),
            scanned: AtomicU64::new(0),
            requests: AtomicU64::new(0),
            matched: AtomicU64::new(0),
            bytes_read: AtomicU64::new(0),
            s3_bytes_read: AtomicU64::new(0),
        }
    }

    pub fn progress(&self) -> ScanProgress {
        ScanProgress {
            scanned: self.scanned.load(Ordering::Relaxed),
            matched: self.matched.load(Ordering::Relaxed),
            requests: self.requests.load(Ordering::Relaxed),
            bytes_read: self.bytes_read.load(Ordering::Relaxed),
            s3_bytes_read: self.s3_bytes_read.load(Ordering::Relaxed),
            elapsed_ms: self.started.elapsed().as_millis() as u64,
        }
    }

    pub fn limited(&self, reason: impl std::fmt::Display) -> FmError {
        let p = self.progress();
        FmError::ScanLimited(format!(
            "{reason} ({} scanned, {} API calls, {}s elapsed)",
            p.scanned,
            p.requests,
            p.elapsed_ms / 1000
        ))
    }

    pub fn check(&self, cancel: &AtomicBool) -> Result<(), FmError> {
        if cancel.load(Ordering::Relaxed) {
            return Err(FmError::ScanCancelled);
        }
        if self.started.elapsed() >= Duration::from_secs(self.limits.max_seconds) {
            return Err(self.limited(format!("{}-second deadline", self.limits.max_seconds)));
        }
        Ok(())
    }

    pub fn object(&self, cancel: &AtomicBool) -> Result<(), FmError> {
        self.check(cancel)?;
        if !increment_bounded(&self.scanned, self.limits.max_objects) {
            return Err(self.limited(format!("{}-object budget", self.limits.max_objects)));
        }
        Ok(())
    }

    pub fn matched(&self) -> bool {
        self.matched.fetch_add(1, Ordering::Relaxed) + 1 >= self.limits.max_results
    }

    pub fn bytes(&self, amount: u64, s3: bool) {
        self.bytes_read.fetch_add(amount, Ordering::Relaxed);
        if s3 {
            self.s3_bytes_read.fetch_add(amount, Ordering::Relaxed);
        }
    }

    /// Cancellation/deadline also apply while waiting for a stalled request/body.
    pub fn wait<'a, T: 'a>(
        &'a self,
        cancel: &'a AtomicBool,
        future: impl Future<Output = Result<T, FmError>> + 'a,
    ) -> impl Future<Output = Result<T, FmError>> + 'a {
        // Box before constructing the wrapper future. Boxing inside an async fn
        // still embeds its unpolled input in the parent frame. AWS SDK futures
        // are large enough for nested request/timeout/select wrappers to overflow
        // the standard 2 MiB worker stack in debug builds.
        let future = Box::pin(future);
        async move {
            self.check(cancel)?;
            tokio::select! {
                result = tokio::time::timeout_at(self.started + Duration::from_secs(self.limits.max_seconds), future) => {
                    let value = result.map_err(|_| self.limited(format!("{}-second deadline", self.limits.max_seconds)))??;
                    self.check(cancel)?;
                    Ok(value)
                },
                _ = async {
                    loop {
                        if cancel.load(Ordering::Relaxed) { break; }
                        tokio::time::sleep(Duration::from_millis(50)).await;
                    }
                } => Err(FmError::ScanCancelled),
            }
        }
    }

    pub fn request<'a, T: 'a>(
        &'a self,
        cancel: &'a AtomicBool,
        future: impl Future<Output = Result<T, FmError>> + 'a,
    ) -> impl Future<Output = Result<T, FmError>> + 'a {
        let future = Box::pin(future);
        async move {
            self.check(cancel)?;
            if !increment_bounded(&self.requests, self.limits.max_requests) {
                return Err(self.limited(format!("{}-request budget", self.limits.max_requests)));
            }
            self.wait(cancel, future).await
        }
    }

    pub fn transfer_progress(&self, id: &str, on_progress: &(dyn Fn(ProgressEvent) + Send + Sync)) {
        let p = self.progress();
        on_progress(ProgressEvent {
            id: id.into(),
            bytes_done: 0,
            bytes_total: 0,
            files_done: 0,
            files_total: 0,
            current_file: format!(
                "Preflight scanning: {} objects, {} API calls, {}s — not complete",
                p.scanned,
                p.requests,
                p.elapsed_ms / 1000
            ),
        });
    }
}

// Keep the declared Rust 1.94.1 MSRV without using the newer try_update API or
// emitting fetch_update deprecation warnings on current CI toolchains.
fn increment_bounded(counter: &AtomicU64, limit: u64) -> bool {
    let mut current = counter.load(Ordering::Relaxed);
    while current < limit {
        match counter.compare_exchange_weak(
            current,
            current + 1,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => return true,
            Err(actual) => current = actual,
        }
    }
    false
}

/// The callback returns true to stop at the search result limit. A stop is only
/// complete if this is provably the last object of the final page.
pub async fn walk_objects(
    client: &Client,
    bucket: &str,
    prefix: &str,
    budget: &ScanBudget,
    cancel: &AtomicBool,
    mut visit: impl FnMut(&Object) -> Result<bool, FmError>,
    on_progress: &(dyn Fn(ScanProgress) + Send + Sync),
) -> Result<(), FmError> {
    let mut token = None;
    let mut seen = HashSet::new();
    let mut page = 1;
    loop {
        budget.check(cancel)?;
        on_progress(budget.progress());
        let response = budget
            .request(cancel, async {
                on_progress(budget.progress());
                client
                    .list_objects_v2()
                    .bucket(bucket)
                    .prefix(prefix)
                    .max_keys(1000)
                    .set_continuation_token(token.clone())
                    .send()
                    .await
                    .map_err(|e| s3_sdk_error(&format!("ListObjectsV2 (scan, page {page})"), &e))
            })
            .await?;
        page += 1;
        let next = if response.is_truncated() == Some(true) {
            let next = response
                .next_continuation_token()
                .filter(|t| !t.is_empty())
                .ok_or_else(|| FmError::S3("Incomplete scan: missing continuation token".into()))?;
            if !seen.insert(next.to_owned()) {
                return Err(FmError::S3(
                    "Incomplete scan: cyclic continuation token".into(),
                ));
            }
            Some(next.to_owned())
        } else {
            None
        };
        for (index, object) in response.contents().iter().enumerate() {
            budget.object(cancel)?;
            if object.key().is_none_or(str::is_empty) {
                return Err(FmError::S3(
                    "Incomplete scan: object without a valid key".into(),
                ));
            }
            if visit(object)? {
                on_progress(budget.progress());
                return if next.is_none() && index + 1 == response.contents().len() {
                    Ok(())
                } else {
                    Err(budget.limited(format!("{}-result budget", budget.limits.max_results)))
                };
            }
        }
        on_progress(budget.progress());
        if next.is_none() {
            return budget.check(cancel);
        }
        if budget.progress().scanned >= budget.limits.max_objects {
            return Err(budget.limited(format!("{}-object budget", budget.limits.max_objects)));
        }
        token = next;
    }
}

pub struct ListedObject {
    pub key: String,
    pub size: u64,
    pub modified: i64,
    pub etag: Option<String>,
}

pub fn paired_cursor(
    seen: &mut HashSet<(String, Option<String>)>,
    key: Option<&str>,
    secondary: Option<&str>,
) -> Result<(String, Option<String>), FmError> {
    let key = key
        .filter(|key| !key.is_empty())
        .ok_or_else(|| FmError::S3("Incomplete listing: missing key marker".into()))?;
    let cursor = (
        key.to_owned(),
        secondary.filter(|s| !s.is_empty()).map(str::to_owned),
    );
    if !seen.insert(cursor.clone()) {
        return Err(FmError::S3("Incomplete listing: cyclic marker pair".into()));
    }
    Ok(cursor)
}

pub async fn collect_objects(
    client: &Client,
    bucket: &str,
    prefix: &str,
    budget: &ScanBudget,
    cancel: &AtomicBool,
    on_progress: &(dyn Fn(ScanProgress) + Send + Sync),
) -> Result<Vec<ListedObject>, FmError> {
    let mut objects = Vec::new();
    walk_objects(
        client,
        bucket,
        prefix,
        budget,
        cancel,
        |object| {
            let key = object
                .key()
                .ok_or_else(|| FmError::S3("Incomplete scan: object without a key".into()))?;
            objects.push(ListedObject {
                key: key.into(),
                size: object.size().unwrap_or(0).max(0) as u64,
                modified: object
                    .last_modified()
                    .and_then(|t| t.to_millis().ok())
                    .unwrap_or(0),
                etag: object.e_tag().map(str::to_owned),
            });
            Ok(false)
        },
        on_progress,
    )
    .await?;
    Ok(objects)
}
