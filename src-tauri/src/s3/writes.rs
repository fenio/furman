//! Explicit, single-use content-write approvals. Legacy APIs are not silently
//! reinterpreted as guarded writes; this path never falls back to an unguarded PUT.
#[cfg(test)]
#[path = "writes_tests.rs"]
mod tests;
use super::{
    errors::s3_sdk_error,
    scan::{self, ScanBudget, ScanProgress},
    write_local::LocalFile,
};
use crate::models::{FmError, ProgressEvent};
use aws_sdk_s3::{error::SdkError, operation::head_object::HeadObjectOutput, Client};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    future::Future,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

fn invalid(message: impl Into<String>) -> FmError {
    FmError::S3(message.into())
}

#[derive(Clone)]
pub struct WriteEndpoint {
    pub client: Client,
    pub bucket: String,
    pub id: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DestinationExpectation {
    Missing,
    Match { etag: String },
}

#[derive(Clone, Deserialize)]
pub struct WriteSelection {
    pub path: String,
    pub destination_key: Option<String>,
    pub expected_sha256: Option<String>,
    pub expected_source_etag: Option<String>,
    pub expected_destination: Option<DestinationExpectation>,
    pub expected_destination_sha256: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WritePolicy {
    CreateOnly,
    SkipExisting,
    ReplaceUnchanged,
    Overwrite,
}

#[derive(Clone)]
enum Source {
    Local(LocalFile),
    S3 {
        endpoint: WriteEndpoint,
        key: String,
        etag: String,
        sha256: Option<String>,
    },
}

struct Item {
    source: Source,
    key: String,
    size: u64,
    destination: Option<Option<String>>,
}

pub struct WritePlan {
    id: String,
    created: Instant,
    destination: WriteEndpoint,
    source: Option<WriteEndpoint>,
    items: Vec<Item>,
}

#[derive(Serialize)]
pub struct WritePreviewEntry {
    pub source: String,
    pub key: String,
    pub size: u64,
    pub exists: bool,
}

#[derive(Serialize)]
pub struct WritePreview {
    pub id: String,
    pub bucket: String,
    pub total: usize,
    pub existing: usize,
    pub unguardable: usize,
    pub bytes: u64,
    pub entries: Vec<WritePreviewEntry>,
    pub expires_seconds: u64,
    pub relay: bool,
}

#[derive(Default)]
pub struct WriteState(Mutex<HashMap<String, WritePlan>>);

impl WriteState {
    pub fn insert(&self, plan: WritePlan) -> Result<WritePreview, FmError> {
        let mut plans = self.0.lock().map_err(|e| invalid(e.to_string()))?;
        plans.retain(|_, plan| plan.created.elapsed() < Duration::from_secs(900));
        if plans.len() >= 4 {
            return Err(invalid(
                "Four guarded previews are already pending; close one before preparing another",
            ));
        }
        let preview = plan.preview();
        plans.insert(plan.id.clone(), plan);
        Ok(preview)
    }
    pub fn take(&self, id: &str) -> Result<WritePlan, FmError> {
        let plan = self.0.lock().map_err(|e| invalid(e.to_string()))?.remove(id)
            .ok_or_else(|| invalid("Write preview expired, was discarded, or was already used; prepare a fresh preview"))?;
        if plan.created.elapsed() >= Duration::from_secs(900) {
            return Err(invalid("Write preview expired; prepare a fresh preview"));
        }
        Ok(plan)
    }
    pub fn discard(&self, id: &str) -> Result<(), FmError> {
        self.0
            .lock()
            .map_err(|e| invalid(e.to_string()))?
            .remove(id);
        Ok(())
    }
}

impl WritePlan {
    pub fn preview(&self) -> WritePreview {
        WritePreview {
            id: self.id.clone(),
            bucket: self.destination.bucket.clone(),
            total: self.items.len(),
            existing: self
                .items
                .iter()
                .filter(|item| item.destination.is_some())
                .count(),
            unguardable: self
                .items
                .iter()
                .filter(|item| matches!(item.destination, Some(None)))
                .count(),
            bytes: self.items.iter().map(|item| item.size).sum(),
            expires_seconds: 900,
            relay: self.source.is_some(),
            entries: self
                .items
                .iter()
                .take(50)
                .map(|item| WritePreviewEntry {
                    source: match &item.source {
                        Source::Local(file) => file.display.clone(),
                        Source::S3 { endpoint, key, .. } => {
                            format!("s3://{}/{key}", endpoint.bucket)
                        }
                    },
                    key: item.key.clone(),
                    size: item.size,
                    exists: item.destination.is_some(),
                })
                .collect(),
        }
    }
    pub fn validate_connections(&self, state: &super::S3State) -> Result<(), FmError> {
        let map = state.0.lock().map_err(|e| invalid(e.to_string()))?;
        for endpoint in std::iter::once(&self.destination).chain(self.source.iter()) {
            if !map.get(&endpoint.id).is_some_and(|connection| {
                std::ptr::eq(connection.client.config(), endpoint.client.config())
                    && connection.bucket == endpoint.bucket
            }) {
                return Err(invalid(
                    "A prepared connection changed or disconnected; prepare a fresh write preview",
                ));
            }
        }
        Ok(())
    }
}

fn source_key(path: &str, bucket: &str) -> Result<String, FmError> {
    if path.starts_with("s3://") {
        path.strip_prefix(&format!("s3://{bucket}/"))
            .map(str::to_owned)
            .ok_or_else(|| invalid("Source path belongs to a different bucket"))
    } else {
        Ok(path.into())
    }
}

fn key_for(selection: &WriteSelection, prefix: &str, relative: &str) -> Result<String, FmError> {
    let key = selection
        .destination_key
        .clone()
        .unwrap_or_else(|| format!("{prefix}{relative}"));
    if key.is_empty() || key.len() > 1024 || !key.starts_with(prefix) {
        return Err(invalid(
            "Destination key is empty, too long, or outside the approved prefix",
        ));
    }
    Ok(key)
}

async fn destination_head(
    endpoint: &WriteEndpoint,
    key: &str,
    budget: &ScanBudget,
    cancel: &AtomicBool,
) -> Result<Option<HeadObjectOutput>, FmError> {
    budget
        .request(cancel, async {
            match endpoint
                .client
                .head_object()
                .bucket(&endpoint.bucket)
                .key(key)
                .send()
                .await
            {
                Ok(head) => Ok(Some(head)),
                Err(error)
                    if error
                        .raw_response()
                        .is_some_and(|response| response.status().as_u16() == 404) =>
                {
                    Ok(None)
                }
                Err(error) => Err(s3_sdk_error(
                    "HeadObject (guarded destination preview)",
                    &error,
                )),
            }
        })
        .await
}

pub async fn prepare(
    source: Option<WriteEndpoint>,
    destination: WriteEndpoint,
    selections: Vec<WriteSelection>,
    prefix: String,
    cancel: Arc<AtomicBool>,
    on_progress: Arc<dyn Fn(ScanProgress) + Send + Sync>,
) -> Result<WritePlan, FmError> {
    if selections.is_empty() || selections.len() > 10_000 {
        return Err(invalid("Select between 1 and 10,000 guarded source items"));
    }
    let prefix = if prefix.is_empty() || prefix.ends_with('/') {
        prefix
    } else {
        format!("{prefix}/")
    };
    let budget = Arc::new(ScanBudget::new());
    let mut items = Vec::new();
    let mut expectations = Vec::new();
    if let Some(endpoint) = &source {
        for selection in &selections {
            budget.check(&cancel)?;
            let key = source_key(&selection.path, &endpoint.bucket)?;
            let objects = if key.ends_with('/') {
                if selection.destination_key.is_some()
                    || selection.expected_sha256.is_some()
                    || selection.expected_destination.is_some()
                {
                    return Err(invalid(
                        "Explicit sync mappings must identify individual files",
                    ));
                }
                scan::collect_objects(
                    &endpoint.client,
                    &endpoint.bucket,
                    &key,
                    &budget,
                    &cancel,
                    on_progress.as_ref(),
                )
                .await?
            } else {
                budget.object(&cancel)?;
                let head = budget
                    .request(&cancel, async {
                        endpoint
                            .client
                            .head_object()
                            .bucket(&endpoint.bucket)
                            .key(&key)
                            .send()
                            .await
                            .map_err(|e| s3_sdk_error("HeadObject (guarded source preview)", &e))
                    })
                    .await?;
                vec![scan::ListedObject {
                    key: key.clone(),
                    size: head
                        .content_length()
                        .filter(|size| *size >= 0)
                        .ok_or_else(|| invalid("Source has no usable content length"))?
                        as u64,
                    modified: 0,
                    etag: head.e_tag().map(str::to_owned),
                }]
            };
            for object in objects {
                let etag = object.etag.filter(|etag| !etag.is_empty()).ok_or_else(|| {
                    invalid("Source has no ETag; guarded reads cannot pin its contents")
                })?;
                if selection
                    .expected_source_etag
                    .as_ref()
                    .is_some_and(|expected| expected != &etag)
                {
                    return Err(invalid(
                        "Source changed after comparison; rescan before preparing writes",
                    ));
                }
                let relative = super::verification::destination_key(&object.key, &key, None, "")?;
                let dest_key = key_for(selection, &prefix, &relative)?;
                items.push(Item {
                    source: Source::S3 {
                        endpoint: endpoint.clone(),
                        key: object.key,
                        etag,
                        sha256: selection.expected_sha256.clone(),
                    },
                    key: dest_key,
                    size: object.size,
                    destination: None,
                });
                expectations.push((
                    selection.expected_destination.clone(),
                    selection.expected_destination_sha256.clone(),
                ));
                if items.len() > 10_000 {
                    return Err(invalid(
                        "Guarded preview supports at most 10,000 files; select a smaller scope",
                    ));
                }
            }
        }
    } else {
        let work_budget = budget.clone();
        let work_cancel = cancel.clone();
        let progress = on_progress.clone();
        let prefix = prefix.clone();
        let local = tokio::task::spawn_blocking(move || {
            let mut roots = HashMap::new(); let mut items = Vec::new(); let mut expectations = Vec::new();
            for selection in selections {
                let files = super::write_local::collect(&selection.path, &mut roots, &work_budget, &work_cancel, progress.as_ref())?;
                if selection.destination_key.is_some() && files.len() != 1 { return Err(invalid("Explicit sync mappings must identify one regular file")); }
                for (file, relative) in files {
                    if selection.expected_sha256.as_ref().is_some_and(|expected| expected != &file.sha256) { return Err(invalid("Local contents changed after comparison; rescan before preparing writes")); }
                    let key = key_for(&selection, &prefix, &relative)?;
                    items.push(Item { size: file.size, source: Source::Local(file), key, destination: None });
                    expectations.push((selection.expected_destination.clone(), selection.expected_destination_sha256.clone()));
                    if items.len() > 10_000 { return Err(invalid("Guarded preview supports at most 10,000 files")); }
                }
            }
            Ok::<_, FmError>((items, expectations))
        }).await.map_err(|e| invalid(e.to_string()))??;
        items = local.0;
        expectations = local.1;
    }
    if items.is_empty() {
        return Err(invalid(
            "The selected scope contains no files or S3 folder markers",
        ));
    }
    let mut destinations = HashSet::new();
    let source_keys: HashSet<_> = items
        .iter()
        .filter_map(|item| match &item.source {
            Source::S3 { key, .. } => Some(key.clone()),
            _ => None,
        })
        .collect();
    for (item, (expected, expected_hash)) in items.iter_mut().zip(expectations) {
        if !destinations.insert(item.key.clone()) {
            return Err(invalid(
                "Duplicate destination mapping; select non-overlapping sources",
            ));
        }
        if source
            .as_ref()
            .is_some_and(|s| s.bucket == destination.bucket)
            && source_keys.contains(&item.key)
        {
            return Err(invalid(
                "Destination overlaps a selected source; use a different prefix or bucket",
            ));
        }
        let head = destination_head(&destination, &item.key, &budget, &cancel).await?;
        item.destination = head.as_ref().map(|head| {
            head.e_tag()
                .filter(|etag| !etag.is_empty())
                .map(str::to_owned)
        });
        if let Some(hash) = expected_hash {
            let head = head
                .as_ref()
                .ok_or_else(|| invalid("Destination disappeared since comparison; rescan"))?;
            let etag = head
                .e_tag()
                .filter(|etag| !etag.is_empty())
                .ok_or_else(|| invalid("Destination has no ETag for a checked comparison read"))?;
            let size = head
                .content_length()
                .filter(|size| *size >= 0)
                .ok_or_else(|| invalid("Destination has no usable content length"))?
                as u64;
            let actual = super::verification::object_sha256_bounded(
                &destination.client,
                &destination.bucket,
                &item.key,
                size,
                Some(etag),
                &cancel,
                Some((&budget, on_progress.as_ref())),
            )
            .await?;
            if actual != hash {
                return Err(invalid(
                    "Destination contents changed after comparison; rescan",
                ));
            }
        }
        if let Some(expected) = expected {
            let matches = match (&expected, &item.destination) {
                (DestinationExpectation::Missing, None) => true,
                (DestinationExpectation::Match { etag }, Some(Some(actual))) => {
                    !etag.is_empty() && etag == actual
                }
                _ => false,
            };
            if !matches {
                return Err(invalid(
                    "Destination changed after comparison; rescan before approving writes",
                ));
            }
        }
        on_progress(budget.progress());
    }
    budget.check(&cancel)?;
    Ok(WritePlan {
        id: uuid::Uuid::new_v4().to_string(),
        created: Instant::now(),
        destination,
        source,
        items,
    })
}

#[derive(Serialize)]
pub struct WriteResult {
    pub written: u32,
    pub skipped: u32,
}

fn cancellable<'a, T: 'a>(
    cancel: &'a AtomicBool,
    future: impl Future<Output = Result<T, FmError>> + 'a,
) -> impl Future<Output = Result<T, FmError>> + 'a {
    let future = Box::pin(future);
    async move {
        super::verification::check_cancelled(cancel)?;
        tokio::select! {
            result = future => result,
            _ = async { while !cancel.load(Ordering::Relaxed) { tokio::time::sleep(Duration::from_millis(50)).await; } } => Err(invalid("Operation cancelled; a request already sent may have completed. Inspect the destination before retrying.")),
        }
    }
}

fn write_error<E: aws_sdk_s3::error::ProvideErrorMetadata>(
    operation: &str,
    error: &SdkError<E>,
) -> FmError {
    let detail = s3_sdk_error(operation, error);
    invalid(format!("{detail}\nThe selected write policy was not weakened or retried unconditionally. Conflicts, unsupported conditions, or an uncertain request outcome require a fresh preview."))
}

#[derive(Clone)]
enum Guard {
    Missing,
    Match(String),
    Unconditional,
}

#[derive(Default)]
struct Headers {
    metadata: HashMap<String, String>,
    content_type: Option<String>,
    cache_control: Option<String>,
    disposition: Option<String>,
    encoding: Option<String>,
    language: Option<String>,
}

async fn commit_file(
    endpoint: &WriteEndpoint,
    key: &str,
    path: &std::path::Path,
    size: u64,
    headers: &Headers,
    guard: &Guard,
    cancel: &AtomicBool,
    on_bytes: &(dyn Fn(u64) + Send + Sync),
) -> Result<Option<String>, FmError> {
    use aws_sdk_s3::{
        primitives::ByteStream,
        types::{ChecksumAlgorithm, CompletedMultipartUpload, CompletedPart},
    };
    // A lost response must not trigger an automatic replay of an approved write.
    // Keep all connection configuration/interceptors (including SSE-C), but make
    // this execution attempt single-shot, even for explicit unguarded overwrite.
    let client = Client::from_conf(
        endpoint
            .client
            .config()
            .to_builder()
            .retry_config(aws_sdk_s3::config::retry::RetryConfig::disabled())
            .build(),
    );
    if size <= super::helpers::get_multipart_threshold().min(5 * 1024 * 1024 * 1024) {
        let body = ByteStream::from_path(path)
            .await
            .map_err(|e| invalid(e.to_string()))?;
        let mut request = client
            .put_object()
            .bucket(&endpoint.bucket)
            .key(key)
            .body(body)
            .checksum_algorithm(ChecksumAlgorithm::Crc32C)
            .set_metadata(Some(headers.metadata.clone()))
            .set_content_type(headers.content_type.clone())
            .set_cache_control(headers.cache_control.clone())
            .set_content_disposition(headers.disposition.clone())
            .set_content_encoding(headers.encoding.clone())
            .set_content_language(headers.language.clone());
        request = match guard {
            Guard::Missing => request.if_none_match("*"),
            Guard::Match(etag) => request.if_match(etag),
            Guard::Unconditional => request,
        };
        let response = cancellable(cancel, async {
            request
                .send()
                .await
                .map_err(|e| write_error("PutObject (approved write)", &e))
        })
        .await?;
        on_bytes(size);
        cancellable(cancel, async {
            super::helpers::throttle(size).await;
            Ok(())
        })
        .await?;
        return Ok(response.e_tag().map(str::to_owned));
    }
    let part_size = super::helpers::get_part_size()
        .max(5 * 1024 * 1024)
        .max(size.div_ceil(10_000));
    if part_size > 5 * 1024 * 1024 * 1024u64 {
        return Err(invalid("Object exceeds supported multipart limits"));
    }
    let create = cancellable(cancel, async {
        client
            .create_multipart_upload()
            .bucket(&endpoint.bucket)
            .key(key)
            .checksum_algorithm(ChecksumAlgorithm::Crc32C)
            .set_metadata(Some(headers.metadata.clone()))
            .set_content_type(headers.content_type.clone())
            .set_cache_control(headers.cache_control.clone())
            .set_content_disposition(headers.disposition.clone())
            .set_content_encoding(headers.encoding.clone())
            .set_content_language(headers.language.clone())
            .send()
            .await
            .map_err(|e| write_error("CreateMultipartUpload", &e))
    })
    .await?;
    let upload_id = create
        .upload_id()
        .ok_or_else(|| invalid("Multipart response has no upload ID"))?
        .to_owned();
    let mut result = async {
        let mut parts = Vec::new();
        for number in 0..size.div_ceil(part_size) {
            super::verification::check_cancelled(cancel)?;
            let offset = number * part_size;
            let length = part_size.min(size - offset);
            let body = ByteStream::read_from()
                .path(path)
                .offset(offset)
                .length(aws_smithy_types::byte_stream::Length::Exact(length))
                .build()
                .await
                .map_err(|e| invalid(e.to_string()))?;
            let response = cancellable(cancel, async {
                client
                    .upload_part()
                    .bucket(&endpoint.bucket)
                    .key(key)
                    .upload_id(&upload_id)
                    .part_number(number as i32 + 1)
                    .checksum_algorithm(ChecksumAlgorithm::Crc32C)
                    .body(body)
                    .send()
                    .await
                    .map_err(|e| write_error("UploadPart", &e))
            })
            .await?;
            parts.push(
                CompletedPart::builder()
                    .part_number(number as i32 + 1)
                    .e_tag(
                        response
                            .e_tag()
                            .ok_or_else(|| invalid("UploadPart returned no ETag"))?,
                    )
                    .set_checksum_crc32_c(response.checksum_crc32_c().map(str::to_owned))
                    .build(),
            );
            on_bytes(length);
            cancellable(cancel, async {
                super::helpers::throttle(length).await;
                Ok(())
            })
            .await?;
        }
        super::verification::check_cancelled(cancel)?;
        let mut request = client
            .complete_multipart_upload()
            .bucket(&endpoint.bucket)
            .key(key)
            .upload_id(&upload_id)
            .multipart_upload(
                CompletedMultipartUpload::builder()
                    .set_parts(Some(parts))
                    .build(),
            );
        request = match guard {
            Guard::Missing => request.if_none_match("*"),
            Guard::Match(etag) => request.if_match(etag),
            Guard::Unconditional => request,
        };
        let response = cancellable(cancel, async {
            request
                .send()
                .await
                .map_err(|e| write_error("CompleteMultipartUpload (approved write)", &e))
        })
        .await?;
        Ok(response.e_tag().map(str::to_owned))
    }
    .await;
    if result.is_err() {
        // Never complete without the selected condition after a conflict. Abort
        // staged parts, including completion failures, without hiding the cause.
        let abort = client
            .abort_multipart_upload()
            .bucket(&endpoint.bucket)
            .key(key)
            .upload_id(&upload_id)
            .send();
        if !matches!(
            tokio::time::timeout(Duration::from_secs(30), abort).await,
            Ok(Ok(_))
        ) {
            log::warn!(
                "Guarded multipart cleanup failed; inspect incomplete uploads at the destination"
            );
            if let Err(error) = result {
                result = Err(invalid(format!("{error}\nCould not confirm multipart cleanup. Inspect incomplete uploads at the destination; staged parts may continue to incur storage charges.")));
            }
        }
    }
    result
}

impl WritePlan {
    pub async fn execute(
        self,
        policy: WritePolicy,
        replacement_approved: bool,
        verify: bool,
        id: &str,
        cancel: Arc<AtomicBool>,
        on_progress: &(dyn Fn(ProgressEvent) + Send + Sync),
    ) -> Result<WriteResult, FmError> {
        use sha2::{Digest, Sha256};
        use tokio::io::AsyncWriteExt;
        if self.created.elapsed() >= Duration::from_secs(900) {
            return Err(invalid("Write preview expired; prepare it again"));
        }
        if policy == WritePolicy::Overwrite && !replacement_approved {
            return Err(invalid(
                "Unconditional overwrites require explicit approval",
            ));
        }
        let existing = self
            .items
            .iter()
            .filter(|item| item.destination.is_some())
            .count();
        if policy == WritePolicy::CreateOnly && existing > 0 {
            return Err(invalid(
                "Create-only policy conflicts with existing objects; no files were written",
            ));
        }
        if policy == WritePolicy::ReplaceUnchanged && existing > 0 && !replacement_approved {
            return Err(invalid(
                "Replacing existing contents requires explicit approval",
            ));
        }
        if policy == WritePolicy::ReplaceUnchanged
            && self
                .items
                .iter()
                .any(|item| matches!(item.destination, Some(None)))
        {
            return Err(invalid(
                "An existing object has no ETag; conditional replacement cannot be enforced",
            ));
        }
        super::verification::check_cancelled(&cancel)?;
        let skipped = if policy == WritePolicy::SkipExisting {
            existing as u32
        } else {
            0
        };
        let selected: Vec<_> = self
            .items
            .iter()
            .filter(|item| policy != WritePolicy::SkipExisting || item.destination.is_none())
            .collect();
        let files_total = selected.len() as u32;
        let bytes_total = selected
            .iter()
            .map(|item| item.size * if self.source.is_some() { 2 } else { 1 })
            .sum();
        let bytes_done = std::sync::atomic::AtomicU64::new(0);
        let mut written = 0;
        let temporary = tempfile::tempdir()?;
        for item in selected {
            super::verification::check_cancelled(&cancel)?;
            let report = |stage: &str| {
                on_progress(ProgressEvent {
                    id: id.into(),
                    bytes_done: bytes_done.load(Ordering::Relaxed),
                    bytes_total,
                    current_file: format!("{stage}: {}", item.key),
                    files_done: written,
                    files_total,
                })
            };
            report("Preparing approved source snapshot");
            let snapshot = tempfile::NamedTempFile::new_in(temporary.path())?;
            let (digest, headers) = match &item.source {
                Source::Local(source) => {
                    let source = source.clone();
                    let cancel = cancel.clone();
                    let mut file = snapshot.as_file().try_clone()?;
                    tokio::task::spawn_blocking(move || source.snapshot(&mut file, &cancel))
                        .await
                        .map_err(|e| invalid(e.to_string()))??;
                    (
                        match &item.source {
                            Source::Local(source) => source.sha256.clone(),
                            _ => unreachable!(),
                        },
                        Headers::default(),
                    )
                }
                Source::S3 {
                    endpoint,
                    key,
                    etag,
                    sha256,
                } => {
                    let response = cancellable(&cancel, async {
                        endpoint
                            .client
                            .get_object()
                            .bucket(&endpoint.bucket)
                            .key(key)
                            .if_match(etag)
                            .send()
                            .await
                            .map_err(|e| s3_sdk_error("GetObject (approved source)", &e))
                    })
                    .await?;
                    if response.e_tag() != Some(etag.as_str()) {
                        return Err(invalid("Source GET did not return the approved ETag; provider may not enforce conditional reads. No payload from this file was uploaded."));
                    }
                    let headers = Headers {
                        metadata: response.metadata().cloned().unwrap_or_default(),
                        content_type: response.content_type().map(str::to_owned),
                        cache_control: response.cache_control().map(str::to_owned),
                        disposition: response.content_disposition().map(str::to_owned),
                        encoding: response.content_encoding().map(str::to_owned),
                        language: response.content_language().map(str::to_owned),
                    };
                    let mut body = response.body;
                    let mut file = tokio::fs::File::from_std(snapshot.as_file().try_clone()?);
                    let mut hash = Sha256::new();
                    let mut size = 0;
                    while let Some(chunk) = cancellable(&cancel, async {
                        body.try_next()
                            .await
                            .map_err(|e| invalid(format!("Cannot stage approved source: {e}")))
                    })
                    .await?
                    {
                        file.write_all(&chunk).await?;
                        hash.update(&chunk);
                        size += chunk.len() as u64;
                        if size > item.size {
                            return Err(invalid("Source size changed during guarded copy; no payload from this file was uploaded"));
                        }
                        bytes_done.fetch_add(chunk.len() as u64, Ordering::Relaxed);
                        report("Staging approved S3 source");
                    }
                    file.flush().await?;
                    let digest: String = hash
                        .finalize()
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect();
                    if size != item.size
                        || sha256.as_ref().is_some_and(|expected| expected != &digest)
                    {
                        return Err(invalid(
                            "Source contents no longer match the approved comparison; rescan",
                        ));
                    }
                    (digest, headers)
                }
            };
            let guard = match (policy, &item.destination) {
                (WritePolicy::Overwrite, _) => Guard::Unconditional,
                (WritePolicy::ReplaceUnchanged, Some(Some(etag))) => Guard::Match(etag.clone()),
                _ => Guard::Missing,
            };
            report("Writing with approved destination policy");
            let etag = commit_file(
                &self.destination,
                &item.key,
                snapshot.path(),
                item.size,
                &headers,
                &guard,
                &cancel,
                &|bytes| {
                    bytes_done.fetch_add(bytes, Ordering::Relaxed);
                    report("Uploading guarded payload");
                },
            )
            .await?;
            if verify {
                let etag = etag.ok_or_else(|| invalid("Write completed without an ETag; cannot pin destination verification. Inspect before retrying."))?;
                report("Verifying destination SHA-256");
                let actual = cancellable(
                    &cancel,
                    super::verification::object_sha256(
                        &self.destination.client,
                        &self.destination.bucket,
                        &item.key,
                        item.size,
                        Some(&etag),
                        &cancel,
                    ),
                )
                .await?;
                if actual != digest {
                    return Err(invalid("Destination SHA-256 verification failed after writing; inspect the object before retrying"));
                }
            }
            written += 1;
            on_progress(ProgressEvent {
                id: id.into(),
                bytes_done: bytes_done.load(Ordering::Relaxed),
                bytes_total,
                current_file: format!("Wrote {written}; skipped {skipped}"),
                files_done: written,
                files_total,
            });
        }
        Ok(WriteResult { written, skipped })
    }
}
