use crate::commands::file::FileOpState;
use crate::models::{
    DirListing, FmError, ProgressEvent, S3BucketAcl, S3BucketEncryption, S3BucketVersioning,
    S3CorsRule, S3LifecycleRule, S3MultipartUpload, S3ObjectMetadata, S3ObjectProperties,
    S3ObjectVersion, S3PublicAccessBlock, S3Tag, SearchEvent, TransferCheckpoint,
};
use crate::s3::{self, build_s3_client, s3err, S3State, BANDWIDTH_LIMIT};
use crate::s3::service::{S3Bucket, S3Service};
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tauri::ipc::Channel;
use tauri::State;

// ── Helper ──────────────────────────────────────────────────────────────────

/// Extract an S3Service from the connection state for the given id.
fn get_service(state: &State<'_, S3State>, id: &str) -> Result<S3Service, FmError> {
    let map = state.0.lock().map_err(|e| s3err(e.to_string()))?;
    let conn = map.get(id).ok_or_else(|| s3err("S3 connection not found"))?;
    Ok(S3Service::new(conn.client.clone(), conn.bucket.clone()))
}

// ── Commands ────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn s3_check_credentials() -> Result<bool, FmError> {
    s3::service::check_credentials().await
}

#[tauri::command]
pub async fn s3_list_buckets(
    region: String,
    endpoint: Option<String>,
    profile: Option<String>,
    access_key: Option<String>,
    secret_key: Option<String>,
    role_arn: Option<String>,
    external_id: Option<String>,
    session_name: Option<String>,
    session_duration_secs: Option<i32>,
) -> Result<Vec<S3Bucket>, FmError> {
    let client = build_s3_client(
        &region,
        endpoint.as_deref(),
        profile.as_deref(),
        access_key.as_deref(),
        secret_key.as_deref(),
        role_arn.as_deref(),
        external_id.as_deref(),
        session_name.as_deref(),
        session_duration_secs,
        None,
    )
    .await?;
    s3::service::list_buckets(&client).await
}

#[tauri::command]
pub async fn s3_connect(
    state: State<'_, S3State>,
    id: String,
    bucket: String,
    region: String,
    endpoint: Option<String>,
    profile: Option<String>,
    access_key: Option<String>,
    secret_key: Option<String>,
    role_arn: Option<String>,
    external_id: Option<String>,
    session_name: Option<String>,
    session_duration_secs: Option<i32>,
    use_transfer_acceleration: Option<bool>,
) -> Result<(), FmError> {
    let client = build_s3_client(
        &region,
        endpoint.as_deref(),
        profile.as_deref(),
        access_key.as_deref(),
        secret_key.as_deref(),
        role_arn.as_deref(),
        external_id.as_deref(),
        session_name.as_deref(),
        session_duration_secs,
        use_transfer_acceleration,
    )
    .await?;

    // Validate bucket access
    client
        .head_bucket()
        .bucket(&bucket)
        .send()
        .await
        .map_err(|e| s3err(format!("Cannot access bucket '{}': {}", bucket, e)))?;

    let conn = s3::S3Connection {
        client,
        bucket,
        region,
    };

    let mut map = state.0.lock().map_err(|e| s3err(e.to_string()))?;
    map.insert(id, conn);
    Ok(())
}

#[tauri::command]
pub async fn s3_disconnect(state: State<'_, S3State>, id: String) -> Result<(), FmError> {
    let mut map = state.0.lock().map_err(|e| s3err(e.to_string()))?;
    map.remove(&id);
    Ok(())
}

#[tauri::command]
pub async fn s3_list_objects(
    state: State<'_, S3State>,
    id: String,
    prefix: String,
) -> Result<DirListing, FmError> {
    let service = get_service(&state, &id)?;
    service.list_objects(&prefix).await
}

#[tauri::command]
pub async fn s3_download(
    state: State<'_, S3State>,
    file_op_state: State<'_, FileOpState>,
    id: String,
    op_id: String,
    keys: Vec<String>,
    destination: String,
    channel: Channel<ProgressEvent>,
) -> Result<Option<TransferCheckpoint>, FmError> {
    let service = get_service(&state, &id)?;

    let flags = Arc::new(crate::commands::file::OpFlags {
        cancel: AtomicBool::new(false),
        pause: AtomicBool::new(false),
    });
    {
        let mut map = file_op_state.0.lock().map_err(|e| FmError::Other(e.to_string()))?;
        map.insert(op_id.clone(), flags.clone());
    }

    let result = service
        .download(
            &keys,
            &destination,
            &op_id,
            &flags.cancel,
            &flags.pause,
            &|evt| { let _ = channel.send(evt); },
        )
        .await;

    if let Ok(mut map) = file_op_state.0.lock() {
        map.remove(&op_id);
    }

    result
}

#[tauri::command]
pub async fn s3_upload(
    state: State<'_, S3State>,
    file_op_state: State<'_, FileOpState>,
    id: String,
    op_id: String,
    sources: Vec<String>,
    dest_prefix: String,
    channel: Channel<ProgressEvent>,
) -> Result<Option<TransferCheckpoint>, FmError> {
    let service = get_service(&state, &id)?;

    let flags = Arc::new(crate::commands::file::OpFlags {
        cancel: AtomicBool::new(false),
        pause: AtomicBool::new(false),
    });
    {
        let mut map = file_op_state.0.lock().map_err(|e| FmError::Other(e.to_string()))?;
        map.insert(op_id.clone(), flags.clone());
    }

    let result = service
        .upload(
            &sources,
            &dest_prefix,
            &op_id,
            &flags.cancel,
            &flags.pause,
            &|evt| { let _ = channel.send(evt); },
        )
        .await;

    if let Ok(mut map) = file_op_state.0.lock() {
        map.remove(&op_id);
    }

    result
}

#[tauri::command]
pub async fn s3_copy_objects(
    state: State<'_, S3State>,
    file_op_state: State<'_, FileOpState>,
    src_id: String,
    op_id: String,
    src_keys: Vec<String>,
    dest_id: String,
    dest_prefix: String,
    channel: Channel<ProgressEvent>,
) -> Result<Option<TransferCheckpoint>, FmError> {
    let (src_client, src_bucket, dest_client, dest_bucket) = {
        let map = state.0.lock().map_err(|e| s3err(e.to_string()))?;
        let src_conn = map.get(&src_id).ok_or_else(|| s3err("Source S3 connection not found"))?;
        let dest_conn = map.get(&dest_id).ok_or_else(|| s3err("Dest S3 connection not found"))?;
        (
            src_conn.client.clone(),
            src_conn.bucket.clone(),
            dest_conn.client.clone(),
            dest_conn.bucket.clone(),
        )
    };

    let flags = Arc::new(crate::commands::file::OpFlags {
        cancel: AtomicBool::new(false),
        pause: AtomicBool::new(false),
    });
    {
        let mut map = file_op_state.0.lock().map_err(|e| FmError::Other(e.to_string()))?;
        map.insert(op_id.clone(), flags.clone());
    }

    // Use a service just for the method call structure; the actual src/dest are passed explicitly
    let service = S3Service::new(src_client.clone(), src_bucket.clone());

    let result = service
        .copy_objects(
            &src_client,
            &src_bucket,
            &src_keys,
            &dest_client,
            &dest_bucket,
            &dest_prefix,
            &op_id,
            &flags.cancel,
            &flags.pause,
            &|evt| { let _ = channel.send(evt); },
        )
        .await;

    if let Ok(mut map) = file_op_state.0.lock() {
        map.remove(&op_id);
    }

    result
}

#[tauri::command]
pub async fn s3_head_object(
    state: State<'_, S3State>,
    id: String,
    key: String,
) -> Result<S3ObjectProperties, FmError> {
    let service = get_service(&state, &id)?;
    service.head_object(&key).await
}

#[tauri::command]
pub async fn s3_delete_objects(
    state: State<'_, S3State>,
    id: String,
    keys: Vec<String>,
) -> Result<(), FmError> {
    let service = get_service(&state, &id)?;
    service.delete_objects(&keys).await
}

#[tauri::command]
pub async fn s3_create_folder(
    state: State<'_, S3State>,
    id: String,
    key: String,
) -> Result<(), FmError> {
    let service = get_service(&state, &id)?;
    service.create_folder(&key).await
}

#[tauri::command]
pub async fn s3_rename_object(
    state: State<'_, S3State>,
    id: String,
    key: String,
    new_name: String,
) -> Result<(), FmError> {
    let service = get_service(&state, &id)?;
    service.rename_object(&key, &new_name).await
}

#[tauri::command]
pub async fn s3_search_objects(
    state: State<'_, S3State>,
    search_state: State<'_, crate::commands::search::SearchState>,
    id: String,
    search_id: String,
    prefix: String,
    query: String,
    channel: Channel<SearchEvent>,
) -> Result<(), FmError> {
    let service = get_service(&state, &id)?;

    let cancel_flag = Arc::new(AtomicBool::new(false));
    {
        let mut map = search_state.0.lock().map_err(|e| FmError::Other(e.to_string()))?;
        map.insert(search_id.clone(), cancel_flag.clone());
    }

    service
        .search_objects(&prefix, &query, &cancel_flag, &|evt| {
            let _ = channel.send(evt);
        })
        .await
}

#[tauri::command]
pub async fn s3_download_temp(
    state: State<'_, S3State>,
    id: String,
    key: String,
) -> Result<String, FmError> {
    let service = get_service(&state, &id)?;
    service.download_temp(&key).await
}

#[tauri::command]
pub async fn s3_put_text(
    state: State<'_, S3State>,
    id: String,
    key: String,
    content: String,
) -> Result<(), FmError> {
    let service = get_service(&state, &id)?;
    service.put_text(&key, &content).await
}

#[tauri::command]
pub async fn s3_change_storage_class(
    state: State<'_, S3State>,
    id: String,
    key: String,
    target_class: String,
) -> Result<(), FmError> {
    let service = get_service(&state, &id)?;
    service.change_storage_class(&key, &target_class).await
}

#[tauri::command]
pub async fn s3_restore_object(
    state: State<'_, S3State>,
    id: String,
    key: String,
    days: i32,
    tier: String,
) -> Result<(), FmError> {
    let service = get_service(&state, &id)?;
    service.restore_object(&key, days, &tier).await
}

#[tauri::command]
pub async fn s3_list_object_versions(
    state: State<'_, S3State>,
    id: String,
    key: String,
) -> Result<Vec<S3ObjectVersion>, FmError> {
    let service = get_service(&state, &id)?;
    service.list_object_versions(&key).await
}

#[tauri::command]
pub async fn s3_download_version(
    state: State<'_, S3State>,
    id: String,
    key: String,
    version_id: String,
) -> Result<String, FmError> {
    let service = get_service(&state, &id)?;
    service.download_version(&key, &version_id).await
}

#[tauri::command]
pub async fn s3_restore_version(
    state: State<'_, S3State>,
    id: String,
    key: String,
    version_id: String,
) -> Result<(), FmError> {
    let service = get_service(&state, &id)?;
    service.restore_version(&key, &version_id).await
}

#[tauri::command]
pub async fn s3_delete_version(
    state: State<'_, S3State>,
    id: String,
    key: String,
    version_id: String,
) -> Result<(), FmError> {
    let service = get_service(&state, &id)?;
    service.delete_version(&key, &version_id).await
}

#[tauri::command]
pub async fn s3_presign_url(
    state: State<'_, S3State>,
    id: String,
    key: String,
    expires_in_secs: u64,
) -> Result<String, FmError> {
    let service = get_service(&state, &id)?;
    service.presign_url(&key, expires_in_secs).await
}

#[tauri::command]
pub async fn s3_create_bucket(
    region: String,
    bucket_name: String,
    endpoint: Option<String>,
    profile: Option<String>,
    access_key: Option<String>,
    secret_key: Option<String>,
    role_arn: Option<String>,
    external_id: Option<String>,
    session_name: Option<String>,
    session_duration_secs: Option<i32>,
) -> Result<(), FmError> {
    let client = build_s3_client(
        &region,
        endpoint.as_deref(),
        profile.as_deref(),
        access_key.as_deref(),
        secret_key.as_deref(),
        role_arn.as_deref(),
        external_id.as_deref(),
        session_name.as_deref(),
        session_duration_secs,
        None,
    )
    .await?;
    s3::service::create_bucket(&client, &bucket_name, &region).await
}

#[tauri::command]
pub async fn s3_delete_bucket(
    region: String,
    bucket_name: String,
    endpoint: Option<String>,
    profile: Option<String>,
    access_key: Option<String>,
    secret_key: Option<String>,
    role_arn: Option<String>,
    external_id: Option<String>,
    session_name: Option<String>,
    session_duration_secs: Option<i32>,
) -> Result<(), FmError> {
    let client = build_s3_client(
        &region,
        endpoint.as_deref(),
        profile.as_deref(),
        access_key.as_deref(),
        secret_key.as_deref(),
        role_arn.as_deref(),
        external_id.as_deref(),
        session_name.as_deref(),
        session_duration_secs,
        None,
    )
    .await?;
    s3::service::delete_bucket(&client, &bucket_name).await
}

#[tauri::command]
pub async fn s3_get_bucket_versioning(
    state: State<'_, S3State>,
    id: String,
) -> Result<S3BucketVersioning, FmError> {
    let service = get_service(&state, &id)?;
    service.get_bucket_versioning().await
}

#[tauri::command]
pub async fn s3_put_bucket_versioning(
    state: State<'_, S3State>,
    id: String,
    enabled: bool,
) -> Result<(), FmError> {
    let service = get_service(&state, &id)?;
    service.put_bucket_versioning(enabled).await
}

#[tauri::command]
pub async fn s3_get_bucket_encryption(
    state: State<'_, S3State>,
    id: String,
) -> Result<S3BucketEncryption, FmError> {
    let service = get_service(&state, &id)?;
    service.get_bucket_encryption().await
}

#[tauri::command]
pub async fn s3_get_object_metadata(
    state: State<'_, S3State>,
    id: String,
    key: String,
) -> Result<S3ObjectMetadata, FmError> {
    let service = get_service(&state, &id)?;
    service.get_object_metadata(&key).await
}

#[tauri::command]
pub async fn s3_put_object_metadata(
    state: State<'_, S3State>,
    id: String,
    key: String,
    content_type: Option<String>,
    content_disposition: Option<String>,
    cache_control: Option<String>,
    content_encoding: Option<String>,
    custom: HashMap<String, String>,
) -> Result<(), FmError> {
    let service = get_service(&state, &id)?;
    service
        .put_object_metadata(
            &key,
            content_type.as_deref(),
            content_disposition.as_deref(),
            cache_control.as_deref(),
            content_encoding.as_deref(),
            &custom,
        )
        .await
}

#[tauri::command]
pub async fn s3_get_object_tags(
    state: State<'_, S3State>,
    id: String,
    key: String,
) -> Result<Vec<S3Tag>, FmError> {
    let service = get_service(&state, &id)?;
    service.get_object_tags(&key).await
}

#[tauri::command]
pub async fn s3_put_object_tags(
    state: State<'_, S3State>,
    id: String,
    key: String,
    tags: Vec<S3Tag>,
) -> Result<(), FmError> {
    let service = get_service(&state, &id)?;
    service.put_object_tags(&key, &tags).await
}

#[tauri::command]
pub async fn s3_get_bucket_tags(
    state: State<'_, S3State>,
    id: String,
) -> Result<Vec<S3Tag>, FmError> {
    let service = get_service(&state, &id)?;
    service.get_bucket_tags().await
}

#[tauri::command]
pub async fn s3_put_bucket_tags(
    state: State<'_, S3State>,
    id: String,
    tags: Vec<S3Tag>,
) -> Result<(), FmError> {
    let service = get_service(&state, &id)?;
    service.put_bucket_tags(&tags).await
}

#[tauri::command]
pub async fn s3_list_multipart_uploads(
    state: State<'_, S3State>,
    id: String,
    prefix: Option<String>,
) -> Result<Vec<S3MultipartUpload>, FmError> {
    let service = get_service(&state, &id)?;
    service.list_multipart_uploads(prefix.as_deref()).await
}

#[tauri::command]
pub async fn s3_abort_multipart_upload(
    state: State<'_, S3State>,
    id: String,
    key: String,
    upload_id: String,
) -> Result<(), FmError> {
    let service = get_service(&state, &id)?;
    service.abort_multipart_upload(&key, &upload_id).await
}

#[tauri::command]
pub async fn s3_get_bucket_lifecycle(
    state: State<'_, S3State>,
    id: String,
) -> Result<Vec<S3LifecycleRule>, FmError> {
    let service = get_service(&state, &id)?;
    service.get_bucket_lifecycle().await
}

#[tauri::command]
pub async fn s3_put_bucket_lifecycle(
    state: State<'_, S3State>,
    id: String,
    rules: Vec<S3LifecycleRule>,
) -> Result<(), FmError> {
    let service = get_service(&state, &id)?;
    service.put_bucket_lifecycle(&rules).await
}

#[tauri::command]
pub async fn s3_get_bucket_cors(
    state: State<'_, S3State>,
    id: String,
) -> Result<Vec<S3CorsRule>, FmError> {
    let service = get_service(&state, &id)?;
    service.get_bucket_cors().await
}

#[tauri::command]
pub async fn s3_put_bucket_cors(
    state: State<'_, S3State>,
    id: String,
    rules: Vec<S3CorsRule>,
) -> Result<(), FmError> {
    let service = get_service(&state, &id)?;
    service.put_bucket_cors(&rules).await
}

#[tauri::command]
pub async fn s3_bulk_change_storage_class(
    state: State<'_, S3State>,
    id: String,
    keys: Vec<String>,
    target_class: String,
) -> Result<Vec<String>, FmError> {
    let service = get_service(&state, &id)?;
    service.bulk_change_storage_class(&keys, &target_class).await
}

#[tauri::command]
pub async fn s3_get_public_access_block(
    state: State<'_, S3State>,
    id: String,
) -> Result<S3PublicAccessBlock, FmError> {
    let service = get_service(&state, &id)?;
    service.get_public_access_block().await
}

#[tauri::command]
pub async fn s3_put_public_access_block(
    state: State<'_, S3State>,
    id: String,
    config: S3PublicAccessBlock,
) -> Result<(), FmError> {
    let service = get_service(&state, &id)?;
    service.put_public_access_block(&config).await
}

#[tauri::command]
pub async fn s3_get_bucket_policy(
    state: State<'_, S3State>,
    id: String,
) -> Result<String, FmError> {
    let service = get_service(&state, &id)?;
    service.get_bucket_policy().await
}

#[tauri::command]
pub async fn s3_put_bucket_policy(
    state: State<'_, S3State>,
    id: String,
    policy: String,
) -> Result<(), FmError> {
    let service = get_service(&state, &id)?;
    service.put_bucket_policy(&policy).await
}

#[tauri::command]
pub async fn s3_get_bucket_acl(
    state: State<'_, S3State>,
    id: String,
) -> Result<S3BucketAcl, FmError> {
    let service = get_service(&state, &id)?;
    service.get_bucket_acl().await
}

#[tauri::command]
pub async fn s3_put_bucket_acl(
    state: State<'_, S3State>,
    id: String,
    acl: String,
) -> Result<(), FmError> {
    let service = get_service(&state, &id)?;
    service.put_bucket_acl(&acl).await
}

#[tauri::command]
pub async fn s3_put_bucket_encryption(
    state: State<'_, S3State>,
    id: String,
    sse_algorithm: String,
    kms_key_id: Option<String>,
    bucket_key_enabled: bool,
) -> Result<(), FmError> {
    let service = get_service(&state, &id)?;
    service
        .put_bucket_encryption(&sse_algorithm, kms_key_id.as_deref(), bucket_key_enabled)
        .await
}

#[tauri::command]
pub async fn s3_set_bandwidth_limit(bytes_per_sec: u64) -> Result<(), FmError> {
    BANDWIDTH_LIMIT.store(bytes_per_sec, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}
