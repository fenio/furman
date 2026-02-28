use crate::commands::file::{FileOpState, OpFlags};
use crate::models::{DirListing, FmError, ProgressEvent, TransferCheckpoint};
use crate::sftp::{self, sftperr, SftpService, SftpState};
use crate::sftp::helpers::strip_sftp_prefix;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tauri::ipc::Channel;
use tauri::State;

// ── Helper ───────────────────────────────────────────────────────────────────

/// Extract an owned SftpService from the state, dropping the MutexGuard
/// before any async work (same pattern as S3's get_service).
fn get_service(state: &State<'_, SftpState>, id: &str) -> Result<SftpService, FmError> {
    let map = state.0.lock().map_err(|e| sftperr(e.to_string()))?;
    let conn = map.get(id).ok_or_else(|| sftperr("SFTP connection not found"))?;
    Ok(SftpService::new(
        conn.session.clone(),
        conn.host.clone(),
        conn.port,
    ))
}

// ── Commands ────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn sftp_connect(
    state: State<'_, SftpState>,
    id: String,
    host: String,
    port: u16,
    username: String,
    auth_method: String,
    password: Option<String>,
    key_path: Option<String>,
    key_passphrase: Option<String>,
) -> Result<String, FmError> {
    let conn = sftp::client::build_sftp_client(
        &host,
        port,
        &username,
        &auth_method,
        password.as_deref(),
        key_path.as_deref(),
        key_passphrase.as_deref(),
    )
    .await?;

    let home_dir = conn.home_dir.clone();
    let mut map = state.0.lock().map_err(|e| sftperr(e.to_string()))?;
    map.insert(id, conn);
    Ok(home_dir)
}

#[tauri::command]
pub async fn sftp_disconnect(state: State<'_, SftpState>, id: String) -> Result<(), FmError> {
    let conn = {
        let mut map = state.0.lock().map_err(|e| sftperr(e.to_string()))?;
        map.remove(&id)
    };
    if let Some(conn) = conn {
        let _ = conn.session.close().await;
    }
    Ok(())
}

#[tauri::command]
pub async fn sftp_list_objects(
    state: State<'_, SftpState>,
    id: String,
    path: String,
) -> Result<DirListing, FmError> {
    let svc = get_service(&state, &id)?;
    let remote_path = strip_sftp_prefix(&path);
    svc.list_objects(remote_path).await
}

#[tauri::command]
pub async fn sftp_delete(
    state: State<'_, SftpState>,
    id: String,
    paths: Vec<String>,
) -> Result<(), FmError> {
    let svc = get_service(&state, &id)?;
    let remote_paths: Vec<String> = paths.iter().map(|p| strip_sftp_prefix(p).to_string()).collect();
    svc.delete(&remote_paths).await
}

#[tauri::command]
pub async fn sftp_rename(
    state: State<'_, SftpState>,
    id: String,
    path: String,
    new_name: String,
) -> Result<(), FmError> {
    let svc = get_service(&state, &id)?;
    let remote_path = strip_sftp_prefix(&path);
    svc.rename(remote_path, &new_name).await
}

#[tauri::command]
pub async fn sftp_create_folder(
    state: State<'_, SftpState>,
    id: String,
    path: String,
) -> Result<(), FmError> {
    let svc = get_service(&state, &id)?;
    let remote_path = strip_sftp_prefix(&path);
    svc.create_folder(remote_path).await
}

#[tauri::command]
pub async fn sftp_download(
    state: State<'_, SftpState>,
    file_op_state: State<'_, FileOpState>,
    id: String,
    op_id: String,
    keys: Vec<String>,
    destination: String,
    channel: Channel<ProgressEvent>,
) -> Result<Option<TransferCheckpoint>, FmError> {
    let flags = {
        let mut ops = file_op_state
            .0
            .lock()
            .map_err(|e| sftperr(e.to_string()))?;
        let flags = Arc::new(OpFlags {
            cancel: AtomicBool::new(false),
            pause: AtomicBool::new(false),
        });
        ops.insert(op_id.clone(), flags.clone());
        flags
    };

    let svc = get_service(&state, &id)?;
    let remote_paths: Vec<String> = keys.iter().map(|p| strip_sftp_prefix(p).to_string()).collect();

    let result = svc
        .download(
            &remote_paths,
            &destination,
            &op_id,
            &flags.cancel,
            &|evt| { let _ = channel.send(evt); },
        )
        .await;

    // Clean up
    file_op_state
        .0
        .lock()
        .map_err(|e| sftperr(e.to_string()))?
        .remove(&op_id);

    result
}

#[tauri::command]
pub async fn sftp_upload(
    state: State<'_, SftpState>,
    file_op_state: State<'_, FileOpState>,
    id: String,
    op_id: String,
    sources: Vec<String>,
    remote_prefix: String,
    channel: Channel<ProgressEvent>,
) -> Result<Option<TransferCheckpoint>, FmError> {
    let flags = {
        let mut ops = file_op_state
            .0
            .lock()
            .map_err(|e| sftperr(e.to_string()))?;
        let flags = Arc::new(OpFlags {
            cancel: AtomicBool::new(false),
            pause: AtomicBool::new(false),
        });
        ops.insert(op_id.clone(), flags.clone());
        flags
    };

    let svc = get_service(&state, &id)?;
    let remote_dest = strip_sftp_prefix(&remote_prefix);

    let result = svc
        .upload(
            &sources,
            remote_dest,
            &op_id,
            &flags.cancel,
            &|evt| { let _ = channel.send(evt); },
        )
        .await;

    // Clean up
    file_op_state
        .0
        .lock()
        .map_err(|e| sftperr(e.to_string()))?
        .remove(&op_id);

    result
}

#[tauri::command]
pub async fn sftp_download_temp(
    state: State<'_, SftpState>,
    id: String,
    path: String,
) -> Result<String, FmError> {
    let svc = get_service(&state, &id)?;
    let remote_path = strip_sftp_prefix(&path);
    svc.download_temp(remote_path).await
}

#[tauri::command]
pub async fn sftp_put_text(
    state: State<'_, SftpState>,
    id: String,
    path: String,
    content: String,
) -> Result<(), FmError> {
    let svc = get_service(&state, &id)?;
    let remote_path = strip_sftp_prefix(&path);
    svc.put_text(remote_path, &content).await
}

#[tauri::command]
pub async fn sftp_head(
    state: State<'_, SftpState>,
    id: String,
    path: String,
) -> Result<crate::models::FileProperties, FmError> {
    let svc = get_service(&state, &id)?;
    let remote_path = strip_sftp_prefix(&path);
    let meta = svc.stat(remote_path).await?;
    let name = remote_path.rsplit('/').next().unwrap_or(remote_path);
    Ok(crate::models::FileProperties {
        name: name.to_string(),
        path: path.clone(),
        size: meta.size.unwrap_or(0),
        is_dir: meta.is_dir(),
        is_symlink: meta.is_symlink(),
        symlink_target: None,
        created: 0,
        modified: meta.mtime.map(|t| t as i64 * 1000).unwrap_or(0),
        accessed: meta.atime.map(|t| t as i64 * 1000).unwrap_or(0),
        permissions: meta.permissions.unwrap_or(0),
        owner: meta.uid.map(|u| u.to_string()).unwrap_or_default(),
        group: meta.gid.map(|g| g.to_string()).unwrap_or_default(),
        kind: if meta.is_dir() {
            "Directory"
        } else if meta.is_symlink() {
            "Symlink"
        } else {
            "File"
        }
        .to_string(),
    })
}
