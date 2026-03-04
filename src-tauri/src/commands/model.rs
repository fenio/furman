use crate::model_inspector;
use crate::models::model_metadata::ModelMetadata;
use crate::models::FmError;

#[tauri::command]
pub fn inspect_model(path: String) -> Result<ModelMetadata, FmError> {
    model_inspector::inspect(&path)
}
