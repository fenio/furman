use crate::{
    jobs::{self, Paths, Report, SaveRequest},
    models::FmError,
};

// Read-only binding/scanning is isolated from all ordinary transfer registries.
// Preparation retains read-only snapshots; no job execute command exists.
#[tauri::command]
pub fn saved_jobs_list() -> Result<Report, FmError> {
    jobs::list(&Paths::home()?)
}

#[tauri::command]
pub fn saved_jobs_save(request: SaveRequest) -> Result<Report, FmError> {
    jobs::save(&Paths::home()?, request)
}

#[tauri::command]
pub fn saved_jobs_delete(
    expected_revision: String,
    id: String,
    acknowledged: bool,
) -> Result<Report, FmError> {
    jobs::delete(&Paths::home()?, &expected_revision, &id, acknowledged)
}

#[tauri::command]
pub async fn saved_job_bind(
    state: tauri::State<'_, crate::job_runs::JobRunState>,
    op_id: String,
    job_id: String,
    revision: String,
    approved: bool,
) -> Result<crate::job_runs::RunStatus, FmError> {
    crate::job_runs::bind(&state, Paths::home()?, op_id, job_id, revision, approved).await
}

#[tauri::command]
pub async fn saved_job_scan(
    state: tauri::State<'_, crate::job_runs::JobRunState>,
    run_id: String,
    channel: tauri::ipc::Channel<crate::s3::scan::ScanProgress>,
) -> Result<crate::job_runs::RunStatus, FmError> {
    crate::job_runs::scan(
        &state,
        &run_id,
        std::sync::Arc::new(move |progress| {
            let _ = channel.send(progress);
        }),
    )
    .await
}

#[tauri::command]
pub fn saved_job_release(
    state: tauri::State<'_, crate::job_runs::JobRunState>,
    handle: String,
) -> Result<(), FmError> {
    state.release(&handle)
}

#[tauri::command]
pub fn saved_job_status(
    state: tauri::State<'_, crate::job_runs::JobRunState>,
    run_id: String,
) -> Result<crate::job_runs::RunStatus, FmError> {
    state.status(&run_id)
}

#[tauri::command]
pub fn saved_job_manifest_page(
    state: tauri::State<'_, crate::job_runs::JobRunState>,
    run_id: String,
    manifest_id: String,
    offset: u32,
) -> Result<Vec<crate::job_runs::collect::Row>, FmError> {
    state.page(&run_id, &manifest_id, offset as usize)
}

#[tauri::command]
pub async fn saved_job_prepare(
    state: tauri::State<'_, crate::job_runs::JobRunState>,
    request: crate::job_runs::preview::Request,
    channel: tauri::ipc::Channel<crate::s3::scan::ScanProgress>,
) -> Result<crate::job_runs::RunStatus, FmError> {
    crate::job_runs::preview::prepare(
        &state,
        request,
        std::sync::Arc::new(move |progress| {
            let _ = channel.send(progress);
        }),
    )
    .await
}
