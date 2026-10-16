use crate::{
    jobs::{self, Paths, Report, SaveRequest},
    models::FmError,
};

#[tauri::command]
pub async fn saved_job_execution_review(
    state: tauri::State<'_, crate::job_runs::JobRunState>,
    request: crate::job_runs::execute::ReviewRequest,
) -> Result<crate::job_runs::RunStatus, FmError> {
    crate::job_runs::execute::review(&state, request).await
}
#[tauri::command]
pub async fn saved_job_execute(
    state: tauri::State<'_, crate::job_runs::JobRunState>,
    request: crate::job_runs::execute::ExecuteRequest,
) -> Result<crate::job_runs::RunStatus, FmError> {
    crate::job_runs::execute::start(&state, request).await
}

// Binding/scanning/preparation are isolated from ordinary transfer registries.
// Execution requires a separately reviewed native single-use owned approval.
#[tauri::command]
pub fn saved_jobs_list() -> Result<Report, FmError> {
    jobs::list(&Paths::home()?)
}

#[tauri::command]
pub async fn saved_job_history(
    state: tauri::State<'_, crate::job_runs::JobRunState>,
) -> Result<crate::job_runs::journal::Report, FmError> {
    let state = state.inner().clone();
    let paths = Paths::home()?;
    tokio::task::spawn_blocking(move || state.history(&paths))
        .await
        .map_err(|_| {
            FmError::Other(
                "Local job journal recovery task failed; ordinary workflows remain available"
                    .into(),
            )
        })?
}
#[tauri::command]
pub async fn saved_job_history_review(
    state: tauri::State<'_, crate::job_runs::JobRunState>,
    expected_revision: String,
    job_id: Option<String>,
    acknowledged: bool,
    clear_history: bool,
) -> Result<crate::job_runs::journal::Report, FmError> {
    let state = state.inner().clone();
    let paths = Paths::home()?;
    tokio::task::spawn_blocking(move || {
        state.review_history(
            &paths,
            &expected_revision,
            job_id.as_deref(),
            acknowledged,
            clear_history,
        )
    })
    .await
    .map_err(|_| {
        FmError::Other(
            "Local job journal review task failed; refresh and inspect history before retrying"
                .into(),
        )
    })?
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
pub async fn saved_job_release(
    state: tauri::State<'_, crate::job_runs::JobRunState>,
    handle: String,
) -> Result<(), FmError> {
    let state = state.inner().clone();
    tokio::task::spawn_blocking(move||state.release(&handle)).await.map_err(|_|FmError::Other("Job release/journal task failed; inspect local history and allow native expiry cleanup".into()))?
}

#[tauri::command]
pub async fn saved_job_status(
    state: tauri::State<'_, crate::job_runs::JobRunState>,
    run_id: String,
) -> Result<crate::job_runs::RunStatus, FmError> {
    let state = state.inner().clone();
    tokio::task::spawn_blocking(move || state.status(&run_id))
        .await
        .map_err(|_| {
            FmError::Other("Job status/journal task failed; refresh local history".into())
        })?
}

#[tauri::command]
pub async fn saved_job_manifest_page(
    state: tauri::State<'_, crate::job_runs::JobRunState>,
    run_id: String,
    manifest_id: String,
    offset: u32,
) -> Result<Vec<crate::job_runs::collect::Row>, FmError> {
    let state = state.inner().clone();
    tokio::task::spawn_blocking(move || state.page(&run_id, &manifest_id, offset as usize))
        .await
        .map_err(|_| {
            FmError::Other("Job manifest/journal task failed; no execution is enabled".into())
        })?
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
