//! Private cleanup evidence, never a restored mutation capability. Reports omit
//! locations and provider upload IDs. A live approved worker advances it; a
//! separate idle local user attestation may resolve evidence, never resume it.
use super::*;

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UploadState {
    Initiating,
    Active,
    Completing,
    Aborting,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::job_runs) struct Upload {
    pub id: String,
    pub run_id: String,
    pub origin: String,
    pub bucket: String,
    pub key: String,
    pub upload_id: Option<String>,
    pub state: UploadState,
    pub parts: u32,
    pub attempted_parts: u32,
    pub acknowledged_parts: u32,
}
#[derive(Serialize)]
pub struct UploadNotice {
    pub id: String,
    pub run_id: String,
    pub state: UploadState,
    pub upload_id_known: bool,
    pub attempted_parts: u32,
    pub acknowledged_parts: u32,
}
impl Upload {
    pub fn notice(&self) -> UploadNotice {
        UploadNotice {
            id: self.id.clone(),
            run_id: self.run_id.clone(),
            state: self.state,
            upload_id_known: self.upload_id.is_some(),
            attempted_parts: self.attempted_parts,
            acknowledged_parts: self.acknowledged_parts,
        }
    }
}
pub(in crate::job_runs) enum UploadChange {
    Begin(Upload),
    Active { id: String, upload_id: String },
    Completing(String),
    Part { id: String, number: u32 },
    PartAcknowledged { id: String, number: u32 },
    Aborting(String),
    Resolved(String),
}
fn bounded(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}
impl Store {
    pub(super) fn validate_uploads(&self) -> Result<(), FmError> {
        if self.uploads.len() > 128 || (self.version < 4 && !self.uploads.is_empty()) {
            return Err(error("Invalid multipart cleanup evidence limits/schema"));
        }
        let mut ids = HashSet::new();
        for upload in &self.uploads {
            let origin = url::Url::parse(&upload.origin)
                .map_err(|_| error("Invalid private multipart origin"))?;
            if !uuid(&upload.id)
                || !ids.insert(&upload.id)
                || !self.records.iter().any(|record| {
                    record.run_id == upload.run_id
                        && !matches!(record.status, Status::Succeeded | Status::Skipped)
                        && record
                            .execution
                            .as_ref()
                            .is_some_and(|counts| counts.multipart_files > 0)
                })
                || !matches!(origin.scheme(), "http" | "https")
                || origin.host_str().is_none()
                || !origin.username().is_empty()
                || origin.password().is_some()
                || origin.query().is_some()
                || origin.fragment().is_some()
                || origin.path() != "/"
                || !bounded(&upload.origin, 2048)
                || !bounded(&upload.bucket, 255)
                || !bounded(&upload.key, 1024)
                || upload
                    .upload_id
                    .as_ref()
                    .is_some_and(|id| !bounded(id, 2048))
                || (upload.state == UploadState::Initiating) != upload.upload_id.is_none()
            {
                return Err(error(
                    "Invalid private multipart cleanup evidence; it was not replaced",
                ));
            }
            if upload.parts == 0
                || upload.parts > 128
                || upload.acknowledged_parts > upload.attempted_parts
                || upload.attempted_parts > upload.parts
                || upload.attempted_parts > upload.acknowledged_parts + 1
                || (upload.state == UploadState::Initiating && upload.attempted_parts != 0)
                || (upload.state == UploadState::Completing
                    && upload.acknowledged_parts != upload.parts)
            {
                return Err(error("Invalid durable multipart part counters"));
            }
        }
        for record in &self.records {
            if self
                .uploads
                .iter()
                .filter(|upload| upload.run_id == record.run_id)
                .count() as u64
                + record.multipart_cleanup_reviews
                > record
                    .execution
                    .as_ref()
                    .map_or(0, |counts| counts.multipart_files)
            {
                return Err(error(
                    "Multipart cleanup evidence exceeds its native selection",
                ));
            }
        }
        Ok(())
    }
}
impl Attempt {
    pub(in crate::job_runs) fn run_id(&self) -> &str {
        &self.run
    }
    pub(in crate::job_runs) fn upload(&self, change: UploadChange) -> Result<(), FmError> {
        let _serial = self.serial.lock().map_err(|_| unavailable())?;
        if !self.healthy() {
            return Err(error(
                "Multipart tracking is unavailable; no further mutation or cleanup was authorized",
            ));
        }
        let result = (|| {
            let storage = Storage::open(&self.paths, false)?.ok_or_else(unavailable)?;
            let _lock = storage.lock()?;
            let (mut store, dirty) = storage.load()?;
            if dirty
                || store.storage_review_required
                || !store
                    .records
                    .iter()
                    .any(|r| r.run_id == self.run && r.status == Status::Executing)
            {
                return Err(error(
                    "Multipart tracking requires its live durable execution intent",
                ));
            }
            match change {
                UploadChange::Begin(upload) => {
                    if upload.run_id != self.run
                        || upload.state != UploadState::Initiating
                        || upload.upload_id.is_some()
                        || store.uploads.len() >= 128
                    {
                        return Err(error("Multipart cleanup evidence admission is full or invalid; no upload was started"));
                    }
                    store.uploads.push(upload);
                }
                UploadChange::Resolved(id) => {
                    let index = store
                        .uploads
                        .iter()
                        .position(|u| u.id == id && u.run_id == self.run)
                        .ok_or_else(|| error("Missing owned multipart cleanup evidence"))?;
                    store.uploads.remove(index);
                }
                change => {
                    let id = match &change {
                        UploadChange::Active { id, .. }
                        | UploadChange::Part { id, .. }
                        | UploadChange::PartAcknowledged { id, .. }
                        | UploadChange::Completing(id)
                        | UploadChange::Aborting(id) => id,
                        _ => unreachable!(),
                    };
                    let upload = store
                        .uploads
                        .iter_mut()
                        .find(|u| &u.id == id && u.run_id == self.run)
                        .ok_or_else(|| error("Missing owned multipart cleanup evidence"))?;
                    match change {
                        UploadChange::Active { upload_id, .. }
                            if upload.state == UploadState::Initiating
                                && bounded(&upload_id, 2048) =>
                        {
                            upload.upload_id = Some(upload_id);
                            upload.state = UploadState::Active;
                        }
                        UploadChange::Part { number, .. }
                            if upload.state == UploadState::Active
                                && number == upload.attempted_parts + 1
                                && upload.acknowledged_parts == upload.attempted_parts
                                && number <= upload.parts =>
                        {
                            upload.attempted_parts = number
                        }
                        UploadChange::PartAcknowledged { number, .. }
                            if upload.state == UploadState::Active
                                && number == upload.attempted_parts
                                && number == upload.acknowledged_parts + 1 =>
                        {
                            upload.acknowledged_parts = number
                        }
                        UploadChange::Completing(_)
                            if upload.state == UploadState::Active
                                && upload.acknowledged_parts == upload.parts =>
                        {
                            upload.state = UploadState::Completing
                        }
                        UploadChange::Aborting(_)
                            if matches!(
                                upload.state,
                                UploadState::Active
                                    | UploadState::Completing
                                    | UploadState::Aborting
                            ) =>
                        {
                            upload.state = UploadState::Aborting
                        }
                        _ => return Err(error("Invalid native multipart state transition")),
                    }
                }
            }
            storage.commit(&mut store)
        })();
        if result.is_err() {
            self.broken.store(true, Ordering::Relaxed);
            self.fault.store(true, Ordering::Relaxed);
            self.cancel.store(true, Ordering::Relaxed);
        }
        result
    }
}
pub(in crate::job_runs) fn resolve_evidence(
    paths: &Paths,
    revision: &str,
    id: &str,
    acknowledged: bool,
) -> Result<Report, FmError> {
    if !acknowledged {
        return Err(error("Explicit external cleanup review is required; this local-only action never checks or cleans up the provider"));
    }
    let storage = Storage::open(paths, false)?.ok_or_else(unavailable)?;
    let _lock = storage.lock()?;
    let (mut store, dirty) = storage.load()?;
    if store.revision != revision
        || dirty
        || store.storage_review_required
        || store.records.iter().any(|r| r.status.active())
    {
        return Err(error("History changed, is active or needs storage recovery; external cleanup review was not recorded"));
    }
    let index = store
        .uploads
        .iter()
        .position(|upload| upload.id == id)
        .ok_or_else(|| error("Unknown or already-reviewed native multipart evidence ID"))?;
    let run = store.uploads[index].run_id.clone();
    let record = store
        .records
        .iter_mut()
        .find(|record| record.run_id == run)
        .ok_or_else(|| error("Multipart attempt is missing"))?;
    record.multipart_cleanup_reviews += 1;
    record.updated_ms = now().max(record.started_ms);
    store.uploads.remove(index);
    storage.commit(&mut store)?;
    Ok(store.report(false))
}
