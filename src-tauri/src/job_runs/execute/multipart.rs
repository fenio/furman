//! Bounded multipart writes over unlinked native snapshot capabilities. A fresh
//! exact-operation interceptor is single transmission, including each part.
use super::*;
use crate::job_runs::journal::{Upload, UploadChange, UploadState};
use aws_sdk_s3::types::{CompletedMultipartUpload, CompletedPart};

pub(in crate::job_runs) const MAX_FILE_BYTES: u64 = 1024 * 1024 * 1024;
pub(in crate::job_runs) const PART_BYTES: u64 = 8 * 1024 * 1024;
pub(in crate::job_runs) fn needed(size: u64) -> bool {
    size > 5 * 1024 * 1024
}
pub(in crate::job_runs) fn parts(size: u64) -> u64 {
    size.div_ceil(PART_BYTES)
}

pub(super) async fn read_chunk(
    binding: Arc<Binding>,
    file: File,
    offset: u64,
    length: u64,
    cancel: Arc<AtomicBool>,
    budget: Arc<ScanBudget>,
) -> Result<Vec<u8>, FmError> {
    if length > PART_BYTES
        || offset
            .checked_add(length)
            .is_none_or(|end| end > MAX_FILE_BYTES)
    {
        return Err(error("Snapshot chunk exceeds the native multipart bounds"));
    }
    tokio::task::spawn_blocking(move || {
        let _lease = binding;
        let mut bytes = vec![0; length as usize];
        let mut read = 0;
        while read < bytes.len() {
            budget.check(&cancel)?;
            let count = file
                .read_at(&mut bytes[read..], offset + read as u64)
                .map_err(|_| error("Cannot read native snapshot chunk"))?;
            if count == 0 {
                return Err(error("Native snapshot chunk became incomplete"));
            }
            read += count;
            budget.bytes(count as u64, false);
        }
        budget.check(&cancel)?;
        Ok(bytes)
    })
    .await
    .map_err(|_| error("Native snapshot chunk worker failed"))?
}

#[derive(Clone)]
enum Operation {
    Create,
    Part { upload: String, number: i32 },
    Complete { upload: String },
    Abort { upload: String },
}
impl Operation {
    fn name(&self) -> &'static str {
        match self {
            Self::Create => "CreateMultipartUpload",
            Self::Part { .. } => "UploadPart",
            Self::Complete { .. } => "CompleteMultipartUpload",
            Self::Abort { .. } => "AbortMultipartUpload",
        }
    }
    fn method(&self) -> &'static str {
        match self {
            Self::Create | Self::Complete { .. } => "POST",
            Self::Part { .. } => "PUT",
            Self::Abort { .. } => "DELETE",
        }
    }
}
struct Scope {
    binding: Arc<Binding>,
    key: String,
    operation: Operation,
    expected: crate::s3::writes::DestinationExpectation,
    cancel: Arc<AtomicBool>,
    budget: Arc<ScanBudget>,
    deadline: Option<Instant>,
    sent: AtomicBool,
}
impl std::fmt::Debug for Scope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ScopedMultipartJobWriter([private])")
    }
}
#[derive(Debug)]
struct Guard(Arc<Scope>);
const ESCAPED: &str =
    "Multipart job request escaped its exact native operation, key, upload or condition";
impl Scope {
    fn check(
        &self,
        context: &BeforeTransmitInterceptorContextMut<'_>,
        cfg: &ConfigBag,
    ) -> Result<(), BoxError> {
        let request = context.request();
        let operation = cfg.load::<Metadata>().map(Metadata::name).unwrap_or("");
        if operation != self.operation.name() || request.method() != self.operation.method() {
            return Err(ESCAPED.into());
        }
        super::super::ReadNamespaceGuard {
            origin: url::Url::parse(&self.binding.destination.namespace)?,
        }
        .validate(request.uri())?;
        let uri = url::Url::parse(request.uri())?;
        // Parse the literal wire path, not Url::path(), which normalizes dot
        // segments and could authorize a different literal S3 object key.
        let after = request.uri().split_once("://").ok_or(ESCAPED)?.1;
        let path = after
            .find('/')
            .map(|start| &after[start..])
            .ok_or(ESCAPED)?;
        let path = path.split_once('?').map_or(path, |(path, _)| path);
        if urlencoding::decode(path)?.as_ref()
            != format!("/{}/{}", self.binding.destination.bucket, self.key)
        {
            return Err(ESCAPED.into());
        }
        let mut query = std::collections::BTreeMap::new();
        for (key, value) in uri.query_pairs() {
            if query.insert(key.into_owned(), value.into_owned()).is_some() {
                return Err(ESCAPED.into());
            }
        }
        if query.remove("x-id").is_some_and(|value| value != operation) {
            return Err(ESCAPED.into());
        }
        let mut expected = std::collections::BTreeMap::new();
        match &self.operation {
            Operation::Create => {
                expected.insert("uploads".to_owned(), String::new());
            }
            Operation::Part { upload, number } => {
                expected.insert("uploadId".into(), upload.clone());
                expected.insert("partNumber".into(), number.to_string());
            }
            Operation::Complete { upload } | Operation::Abort { upload } => {
                expected.insert("uploadId".into(), upload.clone());
            }
        }
        let headers = request.headers();
        let condition = if matches!(self.operation, Operation::Complete { .. }) {
            match &self.expected {
                crate::s3::writes::DestinationExpectation::Missing => {
                    headers.get("if-none-match") == Some("*") && headers.get("if-match").is_none()
                }
                crate::s3::writes::DestinationExpectation::Match { etag } => {
                    headers.get("if-match") == Some(etag.as_str())
                        && headers.get("if-none-match").is_none()
                }
            }
        } else {
            headers.get("if-match").is_none() && headers.get("if-none-match").is_none()
        };
        if query != expected || !condition {
            return Err(ESCAPED.into());
        }
        if self.cancel.load(Ordering::Relaxed)
            || !self.binding.attempt.healthy()
            || self.deadline.is_some_and(|d| d <= Instant::now())
        {
            return Err("Job mutation cancelled, expired or blocked by journal storage".into());
        }
        self.binding
            .validate()
            .map_err(|_| "Native job identity changed before transmission")?;
        if self.budget.check(&self.cancel).is_err() {
            return Err(
                "Job execution budget/cancellation/approval expired before transmission".into(),
            );
        }
        Ok(())
    }
}
impl Intercept for Guard {
    fn name(&self) -> &'static str {
        "FurmanScopedMultipartJobWriter"
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
            return Err("A multipart job mutation cannot be replayed".into());
        }
        Ok(())
    }
}
fn writer(
    binding: Arc<Binding>,
    key: &str,
    operation: Operation,
    expected: &crate::s3::writes::DestinationExpectation,
    cancel: Arc<AtomicBool>,
    budget: Arc<ScanBudget>,
    deadline: Option<Instant>,
) -> Result<(Client, Arc<Scope>), FmError> {
    crate::s3::compat::check_conditional_writes(&binding.destination.client)?;
    let original = &binding.destination.client;
    if original
        .config()
        .interceptors()
        .filter(|i| i.name() == "FurmanReadOnlyJobNamespace")
        .count()
        != 1
    {
        return Err(error(
            "Cannot derive an unambiguous native multipart job writer",
        ));
    }
    let retained: Vec<_> = original
        .config()
        .interceptors()
        .filter(|i| i.name() != "FurmanReadOnlyJobNamespace")
        .collect();
    let scope = Arc::new(Scope {
        binding: binding.clone(),
        key: key.into(),
        operation,
        expected: expected.clone(),
        cancel,
        budget,
        deadline,
        sent: AtomicBool::new(false),
    });
    let mut builder = original.config().to_builder();
    builder.set_interceptors(retained);
    Ok((
        Client::from_conf(
            builder
                .retry_config(RetryConfig::disabled())
                .interceptor(Guard(scope.clone()))
                .build(),
        ),
        scope,
    ))
}
#[cfg(test)]
pub(super) async fn test_guard(
    binding: Arc<Binding>,
    http: &aws_smithy_http_client::test_util::StaticReplayClient,
) {
    let expected = crate::s3::writes::DestinationExpectation::Match {
        etag: "opaque-native-etag".into(),
    };
    let (client, _) = writer(
        binding.clone(),
        "dest/file",
        Operation::Complete {
            upload: "native/+id".into(),
        },
        &expected,
        Arc::new(AtomicBool::new(false)),
        Arc::new(ScanBudget::with_limits(binding.limits)),
        None,
    )
    .unwrap();
    let before = http.actual_requests().count();
    for key in ["dest//file", "dest/./file", "dest/../dest/file"] {
        assert!(client
            .complete_multipart_upload()
            .bucket("destination-bucket")
            .key(key)
            .upload_id("native/+id")
            .if_match("opaque-native-etag")
            .send()
            .await
            .is_err());
    }
    let redirected = Client::from_conf(
        client
            .config()
            .to_builder()
            .endpoint_url("https://different.test")
            .build(),
    );
    assert!(redirected
        .complete_multipart_upload()
        .bucket("destination-bucket")
        .key("dest/file")
        .upload_id("native/+id")
        .if_match("opaque-native-etag")
        .send()
        .await
        .is_err());
    let (part_client, _) = writer(
        binding.clone(),
        "dest/file",
        Operation::Part {
            upload: "native/+id".into(),
            number: 2,
        },
        &expected,
        Arc::new(AtomicBool::new(false)),
        Arc::new(ScanBudget::with_limits(binding.limits)),
        None,
    )
    .unwrap();
    for (id, number) in [("native/+id", 1), ("other", 2)] {
        assert!(part_client
            .upload_part()
            .bucket("destination-bucket")
            .key("dest/file")
            .upload_id(id)
            .part_number(number)
            .send()
            .await
            .is_err());
    }
    assert!(client
        .complete_multipart_upload()
        .bucket("destination-bucket")
        .key("dest/file")
        .upload_id("other")
        .if_match("opaque-native-etag")
        .send()
        .await
        .is_err());
    assert!(client
        .complete_multipart_upload()
        .bucket("destination-bucket")
        .key("dest/other")
        .upload_id("native/+id")
        .if_match("opaque-native-etag")
        .send()
        .await
        .is_err());
    assert!(client
        .complete_multipart_upload()
        .bucket("destination-bucket")
        .key("dest/file")
        .upload_id("native/+id")
        .if_none_match("*")
        .send()
        .await
        .is_err());
    assert!(client
        .complete_multipart_upload()
        .bucket("destination-bucket")
        .key("dest/file")
        .upload_id("native/+id")
        .if_match("opaque-native-etag")
        .if_none_match("*")
        .send()
        .await
        .is_err());
    assert!(client
        .upload_part()
        .bucket("destination-bucket")
        .key("dest/file")
        .upload_id("native/+id")
        .part_number(1)
        .send()
        .await
        .is_err());
    assert!(client
        .abort_multipart_upload()
        .bucket("destination-bucket")
        .key("dest/file")
        .upload_id("native/+id")
        .send()
        .await
        .is_err());
    assert!(client
        .put_object()
        .bucket("destination-bucket")
        .key("dest/file")
        .if_match("opaque-native-etag")
        .send()
        .await
        .is_err());
    assert_eq!(http.actual_requests().count(), before);
    client
        .complete_multipart_upload()
        .bucket("destination-bucket")
        .key("dest/file")
        .upload_id("native/+id")
        .if_match("opaque-native-etag")
        .send()
        .await
        .unwrap();
    assert!(client
        .complete_multipart_upload()
        .bucket("destination-bucket")
        .key("dest/file")
        .upload_id("native/+id")
        .if_match("opaque-native-etag")
        .send()
        .await
        .is_err());
    assert_eq!(http.actual_requests().count(), before + 1);
}

async fn cleanup(
    binding: Arc<Binding>,
    item: &preview::Item,
    evidence: &str,
    upload: &str,
    unacknowledged_part: bool,
) -> Result<(), FmError> {
    // Cleanup is not payload retry: ignore user cancellation/expired transfer
    // budgets, but never identity changes or broken journal durability.
    binding
        .attempt
        .upload(UploadChange::Aborting(evidence.into()))?;
    let cancel = Arc::new(AtomicBool::new(false));
    let budget = Arc::new(ScanBudget::with_limits(crate::s3::scan::ScanLimits {
        max_requests: 3,
        max_seconds: 30,
        ..binding.limits
    }));
    let mut last = None;
    for _ in 0..3 {
        budget.check(&cancel)?;
        let (client, _) = writer(
            binding.clone(),
            &item.key,
            Operation::Abort {
                upload: upload.into(),
            },
            &item.expected,
            cancel.clone(),
            budget.clone(),
            None,
        )?;
        let result = budget
            .request(&cancel, async {
                match client
                    .abort_multipart_upload()
                    .bucket(&binding.destination.bucket)
                    .key(&item.key)
                    .upload_id(upload)
                    .send()
                    .await
                {
                    Ok(_) => Ok(()),
                    Err(failure)
                        if failure
                            .raw_response()
                            .is_some_and(|r| r.status().as_u16() == 404)
                            && failure.as_service_error().and_then(|e| e.code())
                                == Some("NoSuchUpload") =>
                    {
                        Ok(())
                    }
                    Err(failure) => Err(job_sdk_error(
                        "Job exact-upload multipart cleanup",
                        &failure,
                    )),
                }
            })
            .await;
        match result {
            Ok(()) => {
                if unacknowledged_part {
                    return Err(error("Abort was acknowledged, but a transmitted part had no durable receipt. In-flight parts may finish after abort; cleanup/storage charges cannot be ruled out without independent provider inspection. Private evidence remains unresolved."));
                }
                binding
                    .attempt
                    .upload(UploadChange::Resolved(evidence.into()))?;
                return Ok(());
            }
            Err(failure) => last = Some(failure),
        }
    }
    Err(last.unwrap_or_else(|| error("Multipart cleanup did not complete")))
}

pub(super) async fn upload(
    binding: Arc<Binding>,
    item: &preview::Item,
    a: &Attributes,
    cancel: Arc<AtomicBool>,
    deadline: Option<Instant>,
    budget: Arc<ScanBudget>,
    uncertain: &mut bool,
) -> Result<(Option<String>, Option<String>), FmError> {
    let evidence = fresh_id();
    binding.attempt.upload(UploadChange::Begin(Upload {
        id: evidence.clone(),
        run_id: binding.attempt.run_id().into(),
        origin: binding.destination.namespace.clone(),
        bucket: binding.destination.bucket.clone(),
        key: item.key.clone(),
        upload_id: None,
        state: UploadState::Initiating,
        parts: parts(item.snapshot.size) as u32,
        attempted_parts: 0,
        acknowledged_parts: 0,
    }))?;
    let (client, scope) = writer(
        binding.clone(),
        &item.key,
        Operation::Create,
        &item.expected,
        cancel.clone(),
        budget.clone(),
        deadline,
    )?;
    let mut create = client
        .create_multipart_upload()
        .bucket(&binding.destination.bucket)
        .key(&item.key)
        .set_checksum_algorithm(crate::s3::compat::upload_checksum(&client))
        .set_metadata(Some(a.metadata.clone()))
        .set_content_type(a.content_type.clone())
        .set_cache_control(a.cache_control.clone())
        .set_content_disposition(a.disposition.clone())
        .set_content_encoding(a.encoding.clone())
        .set_content_language(a.language.clone());
    if let Some(controls) = super::super::replacement::write_controls(&binding, item) {
        create = create
            .set_tagging(controls.tagging())
            .storage_class(aws_sdk_s3::types::StorageClass::Standard);
    }
    if a.aes256
        && !binding
            .job
            .destination
            .configuration
            .sse_customer_key_enabled
    {
        create = create.server_side_encryption(aws_sdk_s3::types::ServerSideEncryption::Aes256);
    }
    let created = budget
        .request(&cancel, async {
            create
                .send()
                .await
                .map_err(|failure| job_sdk_error("Job initiate multipart staging", &failure))
        })
        .await;
    *uncertain = scope.sent.load(Ordering::Relaxed);
    let created = match created {
        Ok(created) => created,
        Err(failure) => {
            if !*uncertain {
                binding.attempt.upload(UploadChange::Resolved(evidence))?;
                return Err(failure);
            }
            return Err(error(&format!("{failure}\nMultipart initiation was transmitted without a usable receipt. An upload may exist with an unknown ID; private cleanup evidence is retained. Inspect provider incomplete uploads/lifecycle rules; nothing resumes automatically.")));
        }
    };
    let upload=created.upload_id().filter(|id|!id.is_empty()&&id.len()<=2048&&!id.chars().any(char::is_control)).ok_or_else(||error("Multipart initiation returned no bounded upload ID; private evidence remains unresolved and staging may incur charges"))?.to_owned();
    binding.attempt.upload(UploadChange::Active {
        id: evidence.clone(),
        upload_id: upload.clone(),
    })?;
    *uncertain = false;
    let mut unacknowledged_part = false;
    let result = async {
        let mut completed = Vec::new();
        let mut hash = Sha256::new();
        for index in 0..parts(item.snapshot.size) {
            let offset = index * PART_BYTES;
            let bytes = read_chunk(
                binding.clone(),
                item.snapshot
                    .file
                    .try_clone()
                    .map_err(|_| error("Snapshot descriptor unavailable"))?,
                offset,
                PART_BYTES.min(item.snapshot.size - offset),
                cancel.clone(),
                budget.clone(),
            )
            .await?;
            hash.update(&bytes);
            let number = index as i32 + 1;
            binding.attempt.upload(UploadChange::Part {
                id: evidence.clone(),
                number: number as u32,
            })?;
            let (client, scope) = writer(
                binding.clone(),
                &item.key,
                Operation::Part {
                    upload: upload.clone(),
                    number,
                },
                &item.expected,
                cancel.clone(),
                budget.clone(),
                None,
            )?;
            let response = budget
                .request(&cancel, async {
                    client
                        .upload_part()
                        .bucket(&binding.destination.bucket)
                        .key(&item.key)
                        .upload_id(&upload)
                        .part_number(number)
                        .set_checksum_algorithm(crate::s3::compat::upload_checksum(&client))
                        .body(aws_sdk_s3::primitives::ByteStream::from(bytes))
                        .send()
                        .await
                        .map_err(|failure| {
                            job_sdk_error("Job staged multipart part (not retried)", &failure)
                        })
                })
                .await;
            unacknowledged_part = scope.sent.load(Ordering::Relaxed);
            let response = response?;
            let etag = response
                .e_tag()
                .filter(|etag| {
                    !etag.is_empty() && etag.len() <= 1024 && !etag.chars().any(char::is_control)
                })
                .ok_or_else(|| error("Multipart part returned no bounded opaque ETag"))?;
            binding.attempt.upload(UploadChange::PartAcknowledged {
                id: evidence.clone(),
                number: number as u32,
            })?;
            unacknowledged_part = false;
            completed.push(
                CompletedPart::builder()
                    .part_number(number)
                    .e_tag(etag)
                    .set_checksum_crc32_c(response.checksum_crc32_c().map(str::to_owned))
                    .build(),
            );
        }
        if hex(hash) != item.snapshot.sha256
            || item
                .snapshot
                .file
                .metadata()
                .map_err(|_| error("Cannot inspect staged snapshot"))?
                .len()
                != item.snapshot.size
        {
            return Err(error(
                "Snapshot changed while parts were staged; completion was refused",
            ));
        }
        // Recheck after staging as well as before it. No claim of atomic metadata
        // controls is made; final publication remains conditional on the ETag.
        if super::super::replacement::is_replacement(item) {
            super::super::replacement::check(&binding, item, &budget, &cancel).await?;
        } else {
            missing(&binding, &item.key, &budget, &cancel).await?;
        }
        source_matches(&binding, item, &budget, &cancel).await?;
        binding
            .attempt
            .upload(UploadChange::Completing(evidence.clone()))?;
        let (client, scope) = writer(
            binding.clone(),
            &item.key,
            Operation::Complete {
                upload: upload.clone(),
            },
            &item.expected,
            cancel.clone(),
            budget.clone(),
            None,
        )?;
        let mut complete = client
            .complete_multipart_upload()
            .bucket(&binding.destination.bucket)
            .key(&item.key)
            .upload_id(&upload)
            .multipart_upload(
                CompletedMultipartUpload::builder()
                    .set_parts(Some(completed))
                    .build(),
            );
        complete = match &item.expected {
            crate::s3::writes::DestinationExpectation::Missing => complete.if_none_match("*"),
            crate::s3::writes::DestinationExpectation::Match { etag } => complete.if_match(etag),
        };
        let response = budget
            .request(&cancel, async {
                complete.send().await.map_err(|failure| {
                    job_sdk_error(
                        "Job conditional multipart completion (not retried)",
                        &failure,
                    )
                })
            })
            .await;
        *uncertain = scope.sent.load(Ordering::Relaxed);
        let response = response?;
        binding
            .attempt
            .upload(UploadChange::Resolved(evidence.clone()))?;
        Ok((
            response.e_tag().map(str::to_owned),
            response.version_id().map(str::to_owned),
        ))
    }
    .await;
    if let Err(failure) = result {
        match cleanup(binding, item, &evidence, &upload, unacknowledged_part).await {
            Ok(()) => Err(failure),
            Err(cleanup) => {
                *uncertain = true;
                Err(error(&format!("{failure}\nCould not confirm exact-upload cleanup. Private multipart evidence is retained; staged parts may continue to incur storage charges. Inspect incomplete uploads; startup never reconnects or resumes.\n{cleanup}")))
            }
        }
    } else {
        result
    }
}
