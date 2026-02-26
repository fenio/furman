use crate::models::{
    CfDistribution, CfDistributionConfig, CfDistributionSummary, CfInvalidation, FmError,
};
use crate::cloudfront::CloudFrontService;
use crate::s3::{s3err, S3State};
use tauri::State;

// ── Helper ──────────────────────────────────────────────────────────────────

fn get_cf_service(state: &State<'_, S3State>, id: &str) -> Result<CloudFrontService, FmError> {
    let map = state.0.lock().map_err(|e| s3err(e.to_string()))?;
    let conn = map.get(id).ok_or_else(|| s3err("S3 connection not found"))?;
    let cf_client = aws_sdk_cloudfront::Client::new(&conn.sdk_config);
    Ok(CloudFrontService::new(cf_client, conn.bucket.clone(), conn.region.clone()))
}

// ── Commands ────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn cf_list_distributions(
    state: State<'_, S3State>,
    id: String,
) -> Result<Vec<CfDistributionSummary>, FmError> {
    let svc = get_cf_service(&state, &id)?;
    svc.list_distributions().await
}

#[tauri::command]
pub async fn cf_get_distribution(
    state: State<'_, S3State>,
    id: String,
    dist_id: String,
) -> Result<CfDistribution, FmError> {
    let svc = get_cf_service(&state, &id)?;
    svc.get_distribution(&dist_id).await
}

#[tauri::command]
pub async fn cf_create_distribution(
    state: State<'_, S3State>,
    id: String,
    config: CfDistributionConfig,
) -> Result<CfDistribution, FmError> {
    let svc = get_cf_service(&state, &id)?;
    svc.create_distribution(config).await
}

#[tauri::command]
pub async fn cf_update_distribution(
    state: State<'_, S3State>,
    id: String,
    dist_id: String,
    config: CfDistributionConfig,
    etag: String,
) -> Result<CfDistribution, FmError> {
    let svc = get_cf_service(&state, &id)?;
    svc.update_distribution(&dist_id, config, &etag).await
}

#[tauri::command]
pub async fn cf_delete_distribution(
    state: State<'_, S3State>,
    id: String,
    dist_id: String,
    etag: String,
) -> Result<(), FmError> {
    let svc = get_cf_service(&state, &id)?;
    svc.delete_distribution(&dist_id, &etag).await
}

#[tauri::command]
pub async fn cf_create_invalidation(
    state: State<'_, S3State>,
    id: String,
    dist_id: String,
    paths: Vec<String>,
) -> Result<CfInvalidation, FmError> {
    let svc = get_cf_service(&state, &id)?;
    svc.create_invalidation(&dist_id, paths).await
}

#[tauri::command]
pub async fn cf_list_invalidations(
    state: State<'_, S3State>,
    id: String,
    dist_id: String,
) -> Result<Vec<CfInvalidation>, FmError> {
    let svc = get_cf_service(&state, &id)?;
    svc.list_invalidations(&dist_id).await
}
