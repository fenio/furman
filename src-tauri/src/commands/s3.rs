use crate::commands::file::FileOpState;
use crate::models::{
    DirListing, FmError, KmsKeyInfo, ProgressEvent, S3BucketAcl, S3BucketEncryption,
    S3BucketLogging, S3BucketOwnership, S3BucketVersioning, S3BucketWebsite, S3CorsRule,
    S3InventoryConfiguration, S3LifecycleRule, S3MultipartUpload, S3NotificationConfiguration,
    S3ObjectLegalHold, S3ObjectLockConfig, S3ObjectMetadata, S3ObjectProperties, S3ObjectRetention,
    S3ObjectVersion, S3PublicAccessBlock, S3ReplicationConfiguration, S3Tag, SearchEvent,
    TransferCheckpoint,
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
    web_identity_token: Option<String>,
    proxy_url: Option<String>,
    proxy_username: Option<String>,
    proxy_password: Option<String>,
) -> Result<Vec<S3Bucket>, FmError> {
    let (client, _) = build_s3_client(
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
        None,
        web_identity_token.as_deref(),
        proxy_url.as_deref(),
        proxy_username.as_deref(),
        proxy_password.as_deref(),
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
    anonymous: Option<bool>,
    web_identity_token: Option<String>,
    proxy_url: Option<String>,
    proxy_username: Option<String>,
    proxy_password: Option<String>,
) -> Result<(), FmError> {
    let (client, sdk_config) = build_s3_client(
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
        anonymous,
        web_identity_token.as_deref(),
        proxy_url.as_deref(),
        proxy_username.as_deref(),
        proxy_password.as_deref(),
    )
    .await?;

    // Validate bucket access — public buckets often deny HeadBucket, so use ListObjectsV2
    if anonymous.unwrap_or(false) {
        client
            .list_objects_v2()
            .bucket(&bucket)
            .max_keys(1)
            .send()
            .await
            .map_err(|e| s3err(format!("Cannot access public bucket '{}': {}", bucket, e)))?;
    } else {
        client
            .head_bucket()
            .bucket(&bucket)
            .send()
            .await
            .map_err(|e| s3err(format!("Cannot access bucket '{}': {}", bucket, e)))?;
    }

    let conn = s3::S3Connection {
        client,
        bucket,
        region,
        sdk_config,
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
    password: Option<String>,
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
            password.as_deref(),
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
            None,
        )
        .await;

    if let Ok(mut map) = file_op_state.0.lock() {
        map.remove(&op_id);
    }

    result
}

#[tauri::command]
pub async fn s3_upload_encrypted(
    state: State<'_, S3State>,
    file_op_state: State<'_, FileOpState>,
    id: String,
    op_id: String,
    sources: Vec<String>,
    dest_prefix: String,
    password: String,
    encryption_config: Option<crate::s3::crypto::EncryptionConfig>,
    channel: Channel<ProgressEvent>,
) -> Result<Option<TransferCheckpoint>, FmError> {
    let service = get_service(&state, &id)?;
    let config = encryption_config.unwrap_or_default();

    let flags = Arc::new(crate::commands::file::OpFlags {
        cancel: AtomicBool::new(false),
        pause: AtomicBool::new(false),
    });
    {
        let mut map = file_op_state.0.lock().map_err(|e| FmError::Other(e.to_string()))?;
        map.insert(op_id.clone(), flags.clone());
    }

    let result = service
        .upload_encrypted(
            &sources,
            &dest_prefix,
            &password,
            &config,
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
    password: Option<String>,
) -> Result<String, FmError> {
    let service = get_service(&state, &id)?;
    service.download_temp(&key, password.as_deref()).await
}

#[tauri::command]
pub async fn s3_is_object_encrypted(
    state: State<'_, S3State>,
    id: String,
    key: String,
) -> Result<bool, FmError> {
    let service = get_service(&state, &id)?;
    service.is_object_encrypted(&key).await
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
    mfa: Option<String>,
) -> Result<(), FmError> {
    let service = get_service(&state, &id)?;
    service.delete_version(&key, &version_id, mfa.as_deref()).await
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
    web_identity_token: Option<String>,
    proxy_url: Option<String>,
    proxy_username: Option<String>,
    proxy_password: Option<String>,
) -> Result<(), FmError> {
    let (client, _) = build_s3_client(
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
        None,
        web_identity_token.as_deref(),
        proxy_url.as_deref(),
        proxy_username.as_deref(),
        proxy_password.as_deref(),
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
    web_identity_token: Option<String>,
    proxy_url: Option<String>,
    proxy_username: Option<String>,
    proxy_password: Option<String>,
) -> Result<(), FmError> {
    let (client, _) = build_s3_client(
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
        None,
        web_identity_token.as_deref(),
        proxy_url.as_deref(),
        proxy_username.as_deref(),
        proxy_password.as_deref(),
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
    mfa_delete: Option<bool>,
    mfa: Option<String>,
) -> Result<(), FmError> {
    let service = get_service(&state, &id)?;
    service.put_bucket_versioning(enabled, mfa_delete, mfa.as_deref()).await
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
pub async fn s3_get_bucket_website(
    state: State<'_, S3State>,
    id: String,
) -> Result<S3BucketWebsite, FmError> {
    let service = get_service(&state, &id)?;
    service.get_bucket_website().await
}

#[tauri::command]
pub async fn s3_put_bucket_website(
    state: State<'_, S3State>,
    id: String,
    config: S3BucketWebsite,
) -> Result<(), FmError> {
    let service = get_service(&state, &id)?;
    service.put_bucket_website(&config).await
}

#[tauri::command]
pub async fn s3_get_request_payment(
    state: State<'_, S3State>,
    id: String,
) -> Result<bool, FmError> {
    let service = get_service(&state, &id)?;
    service.get_request_payment().await
}

#[tauri::command]
pub async fn s3_put_request_payment(
    state: State<'_, S3State>,
    id: String,
    requester_pays: bool,
) -> Result<(), FmError> {
    let service = get_service(&state, &id)?;
    service.put_request_payment(requester_pays).await
}

#[tauri::command]
pub async fn s3_get_bucket_ownership(
    state: State<'_, S3State>,
    id: String,
) -> Result<S3BucketOwnership, FmError> {
    let service = get_service(&state, &id)?;
    service.get_bucket_ownership().await
}

#[tauri::command]
pub async fn s3_put_bucket_ownership(
    state: State<'_, S3State>,
    id: String,
    ownership: String,
) -> Result<(), FmError> {
    let service = get_service(&state, &id)?;
    service.put_bucket_ownership(&ownership).await
}

#[tauri::command]
pub async fn s3_get_bucket_logging(
    state: State<'_, S3State>,
    id: String,
) -> Result<S3BucketLogging, FmError> {
    let service = get_service(&state, &id)?;
    service.get_bucket_logging().await
}

#[tauri::command]
pub async fn s3_put_bucket_logging(
    state: State<'_, S3State>,
    id: String,
    config: S3BucketLogging,
) -> Result<(), FmError> {
    let service = get_service(&state, &id)?;
    service.put_bucket_logging(&config).await
}

#[tauri::command]
pub async fn s3_set_bandwidth_limit(bytes_per_sec: u64) -> Result<(), FmError> {
    BANDWIDTH_LIMIT.store(bytes_per_sec, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
pub async fn s3_list_kms_keys(
    state: State<'_, S3State>,
    id: String,
) -> Result<Vec<KmsKeyInfo>, FmError> {
    let sdk_config = {
        let map = state.0.lock().map_err(|e| s3err(e.to_string()))?;
        let conn = map.get(&id).ok_or_else(|| s3err("S3 connection not found"))?;
        conn.sdk_config.clone()
    };

    let kms = aws_sdk_kms::Client::new(&sdk_config);

    // Fetch all keys (paginated)
    let mut keys: Vec<(String, String)> = Vec::new();
    let mut marker: Option<String> = None;
    loop {
        let mut req = kms.list_keys();
        if let Some(m) = &marker {
            req = req.marker(m);
        }
        let resp = req.send().await.map_err(|e| s3err(format!("KMS ListKeys: {}", e)))?;
        for entry in resp.keys() {
            if let (Some(kid), Some(arn)) = (entry.key_id(), entry.key_arn()) {
                keys.push((kid.to_string(), arn.to_string()));
            }
        }
        if resp.truncated() {
            marker = resp.next_marker().map(|s| s.to_string());
        } else {
            break;
        }
    }

    // Fetch all aliases (paginated)
    let mut alias_map: HashMap<String, String> = HashMap::new();
    let mut alias_marker: Option<String> = None;
    loop {
        let mut req = kms.list_aliases();
        if let Some(m) = &alias_marker {
            req = req.marker(m);
        }
        let resp = req.send().await.map_err(|e| s3err(format!("KMS ListAliases: {}", e)))?;
        for alias in resp.aliases() {
            if let (Some(name), Some(kid)) = (alias.alias_name(), alias.target_key_id()) {
                alias_map.insert(kid.to_string(), name.to_string());
            }
        }
        if resp.truncated() {
            alias_marker = resp.next_marker().map(|s| s.to_string());
        } else {
            break;
        }
    }

    // Merge
    let result: Vec<KmsKeyInfo> = keys
        .into_iter()
        .map(|(key_id, arn)| {
            let alias = alias_map.get(&key_id).cloned();
            KmsKeyInfo { key_id, arn, alias }
        })
        .collect();

    Ok(result)
}

// ── Object Lock ──────────────────────────────────────────────────────────

#[tauri::command]
pub async fn s3_get_object_lock_configuration(
    state: State<'_, S3State>,
    id: String,
) -> Result<S3ObjectLockConfig, FmError> {
    let service = get_service(&state, &id)?;
    service.get_object_lock_configuration().await
}

#[tauri::command]
pub async fn s3_put_object_lock_configuration(
    state: State<'_, S3State>,
    id: String,
    mode: Option<String>,
    days: Option<i32>,
    years: Option<i32>,
) -> Result<(), FmError> {
    let service = get_service(&state, &id)?;
    service
        .put_object_lock_configuration(mode.as_deref(), days, years)
        .await
}

#[tauri::command]
pub async fn s3_get_object_retention(
    state: State<'_, S3State>,
    id: String,
    key: String,
) -> Result<S3ObjectRetention, FmError> {
    let service = get_service(&state, &id)?;
    service.get_object_retention(&key).await
}

#[tauri::command]
pub async fn s3_put_object_retention(
    state: State<'_, S3State>,
    id: String,
    key: String,
    mode: String,
    retain_until_date: String,
    bypass_governance: bool,
) -> Result<(), FmError> {
    let service = get_service(&state, &id)?;
    service
        .put_object_retention(&key, &mode, &retain_until_date, bypass_governance)
        .await
}

#[tauri::command]
pub async fn s3_get_object_legal_hold(
    state: State<'_, S3State>,
    id: String,
    key: String,
) -> Result<S3ObjectLegalHold, FmError> {
    let service = get_service(&state, &id)?;
    service.get_object_legal_hold(&key).await
}

#[tauri::command]
pub async fn s3_put_object_legal_hold(
    state: State<'_, S3State>,
    id: String,
    key: String,
    status: String,
) -> Result<(), FmError> {
    let service = get_service(&state, &id)?;
    service.put_object_legal_hold(&key, &status).await
}

#[tauri::command]
pub async fn s3_bulk_put_object_retention(
    state: State<'_, S3State>,
    id: String,
    keys: Vec<String>,
    mode: String,
    retain_until_date: String,
    bypass_governance: bool,
) -> Result<Vec<String>, FmError> {
    let service = get_service(&state, &id)?;
    service
        .bulk_put_object_retention(&keys, &mode, &retain_until_date, bypass_governance)
        .await
}

#[tauri::command]
pub async fn s3_batch_put_object_metadata(
    state: State<'_, S3State>,
    file_op_state: State<'_, FileOpState>,
    id: String,
    op_id: String,
    keys: Vec<String>,
    content_type: Option<String>,
    content_disposition: Option<String>,
    cache_control: Option<String>,
    content_encoding: Option<String>,
    custom: HashMap<String, String>,
    channel: Channel<ProgressEvent>,
) -> Result<Vec<String>, FmError> {
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
        .batch_put_object_metadata(
            &keys,
            content_type.as_deref(),
            content_disposition.as_deref(),
            cache_control.as_deref(),
            content_encoding.as_deref(),
            &custom,
            &flags.cancel,
            &|evt| { let _ = channel.send(evt); },
            &op_id,
        )
        .await;

    if let Ok(mut map) = file_op_state.0.lock() {
        map.remove(&op_id);
    }

    result
}

#[tauri::command]
pub async fn s3_batch_put_object_tags(
    state: State<'_, S3State>,
    file_op_state: State<'_, FileOpState>,
    id: String,
    op_id: String,
    keys: Vec<String>,
    tags: Vec<S3Tag>,
    merge: bool,
    channel: Channel<ProgressEvent>,
) -> Result<Vec<String>, FmError> {
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
        .batch_put_object_tags(
            &keys,
            &tags,
            merge,
            &flags.cancel,
            &|evt| { let _ = channel.send(evt); },
            &op_id,
        )
        .await;

    if let Ok(mut map) = file_op_state.0.lock() {
        map.remove(&op_id);
    }

    result
}

#[tauri::command]
pub async fn s3_list_inventory_configurations(
    state: State<'_, S3State>,
    id: String,
) -> Result<Vec<S3InventoryConfiguration>, FmError> {
    let service = get_service(&state, &id)?;
    service.list_inventory_configurations().await
}

#[tauri::command]
pub async fn s3_put_inventory_configuration(
    state: State<'_, S3State>,
    id: String,
    config: S3InventoryConfiguration,
) -> Result<(), FmError> {
    let service = get_service(&state, &id)?;
    service.put_inventory_configuration(&config).await
}

#[tauri::command]
pub async fn s3_delete_inventory_configuration(
    state: State<'_, S3State>,
    id: String,
    config_id: String,
) -> Result<(), FmError> {
    let service = get_service(&state, &id)?;
    service.delete_inventory_configuration(&config_id).await
}

// ── Replication Configuration ──────────────────────────────────────────────

#[tauri::command]
pub async fn s3_get_replication_configuration(
    state: State<'_, S3State>,
    id: String,
) -> Result<Option<S3ReplicationConfiguration>, FmError> {
    let service = get_service(&state, &id)?;
    service.get_replication_configuration().await
}

#[tauri::command]
pub async fn s3_put_replication_configuration(
    state: State<'_, S3State>,
    id: String,
    config: S3ReplicationConfiguration,
) -> Result<(), FmError> {
    let service = get_service(&state, &id)?;
    service.put_replication_configuration(&config).await
}

#[tauri::command]
pub async fn s3_delete_replication_configuration(
    state: State<'_, S3State>,
    id: String,
) -> Result<(), FmError> {
    let service = get_service(&state, &id)?;
    service.delete_replication_configuration().await
}

// ── Notification Configuration ─────────────────────────────────────────────

#[tauri::command]
pub async fn s3_get_notification_configuration(
    state: State<'_, S3State>,
    id: String,
) -> Result<S3NotificationConfiguration, FmError> {
    let service = get_service(&state, &id)?;
    service.get_notification_configuration().await
}

#[tauri::command]
pub async fn s3_put_notification_configuration(
    state: State<'_, S3State>,
    id: String,
    config: S3NotificationConfiguration,
) -> Result<(), FmError> {
    let service = get_service(&state, &id)?;
    service.put_notification_configuration(&config).await
}
