use crate::{
    jobs::{self, Paths, Report, SaveRequest},
    models::FmError,
};

// No connect/scan/prepare/execute command exists in this foundation.
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
