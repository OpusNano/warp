use tauri::State;

use crate::{
    local_fs::LocalFilesystem,
    models::{
        AppBootstrap, ConnectRequest, CreateRemoteDirectoryRequest, DeleteLocalEntriesRequest,
        DeleteRemoteEntriesRequest, DeleteRemoteEntryRequest, PaneSet, PaneSnapshot,
        QueueDownloadRequest, QueueUploadRequest, RemoteConnectionSnapshot, RemoteDeleteResponse,
        RenameRemoteEntryRequest, TransferConflictResolution, TransferQueueSnapshot, TrustDecision,
    },
    session::SessionManager,
    transfer::TransferManager,
};

#[tauri::command]
pub async fn bootstrap_app_state(
    local_fs: State<'_, LocalFilesystem>,
    session_manager: State<'_, std::sync::Arc<SessionManager>>,
    transfer_manager: State<'_, std::sync::Arc<TransferManager>>,
) -> Result<AppBootstrap, String> {
    let local = local_fs
        .list_directory(None)
        .map_err(|error| error.to_string())?;

    let remote = session_manager.snapshot().await;
    let transfers = transfer_manager.snapshot().await;

    Ok(AppBootstrap {
        connection_profiles: AppBootstrap::sample_profiles(),
        session: remote.session,
        panes: PaneSet {
            local,
            remote: remote.remote_pane,
        },
        transfers,
        shortcuts: AppBootstrap::current_shortcuts(),
    })
}

#[tauri::command]
pub fn list_local_directory(
    local_fs: State<'_, LocalFilesystem>,
    path: Option<String>,
) -> Result<PaneSnapshot, String> {
    local_fs
        .list_directory(path)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn open_local_directory(
    local_fs: State<'_, LocalFilesystem>,
    path: String,
    entry_name: String,
) -> Result<PaneSnapshot, String> {
    local_fs
        .open_directory(&path, &entry_name)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn go_up_local_directory(
    local_fs: State<'_, LocalFilesystem>,
    path: String,
) -> Result<PaneSnapshot, String> {
    local_fs
        .go_up_directory(&path)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn rename_local_entry(
    local_fs: State<'_, LocalFilesystem>,
    path: String,
    entry_name: String,
    new_name: String,
) -> Result<PaneSnapshot, String> {
    local_fs
        .rename_entry(&path, &entry_name, &new_name)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn delete_local_entries(
    local_fs: State<'_, LocalFilesystem>,
    request: DeleteLocalEntriesRequest,
) -> Result<PaneSnapshot, String> {
    local_fs
        .delete_entries(&request.path, &request.entry_names)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn connect_remote(
    session_manager: State<'_, std::sync::Arc<SessionManager>>,
    request: ConnectRequest,
) -> Result<RemoteConnectionSnapshot, String> {
    session_manager
        .connect(request)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn resolve_remote_trust(
    session_manager: State<'_, std::sync::Arc<SessionManager>>,
    decision: TrustDecision,
) -> Result<RemoteConnectionSnapshot, String> {
    session_manager
        .resolve_trust(decision)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn disconnect_remote(
    session_manager: State<'_, std::sync::Arc<SessionManager>>,
) -> Result<RemoteConnectionSnapshot, String> {
    session_manager
        .disconnect()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn refresh_remote_directory(
    session_manager: State<'_, std::sync::Arc<SessionManager>>,
) -> Result<RemoteConnectionSnapshot, String> {
    session_manager
        .refresh_remote_directory()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn open_remote_directory(
    session_manager: State<'_, std::sync::Arc<SessionManager>>,
    path: String,
) -> Result<RemoteConnectionSnapshot, String> {
    session_manager
        .open_remote_directory(path)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn go_up_remote_directory(
    session_manager: State<'_, std::sync::Arc<SessionManager>>,
) -> Result<RemoteConnectionSnapshot, String> {
    session_manager
        .go_up_remote_directory()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn create_remote_directory(
    session_manager: State<'_, std::sync::Arc<SessionManager>>,
    request: CreateRemoteDirectoryRequest,
) -> Result<RemoteConnectionSnapshot, String> {
    session_manager
        .create_remote_directory(request)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn rename_remote_entry(
    session_manager: State<'_, std::sync::Arc<SessionManager>>,
    request: RenameRemoteEntryRequest,
) -> Result<RemoteConnectionSnapshot, String> {
    session_manager
        .rename_remote_entry(request)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn delete_remote_entry(
    session_manager: State<'_, std::sync::Arc<SessionManager>>,
    request: DeleteRemoteEntryRequest,
) -> Result<RemoteDeleteResponse, String> {
    session_manager
        .delete_remote_entry(request)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn delete_remote_entries(
    session_manager: State<'_, std::sync::Arc<SessionManager>>,
    request: DeleteRemoteEntriesRequest,
) -> Result<RemoteDeleteResponse, String> {
    session_manager
        .delete_remote_entries(request)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn queue_download(
    transfer_manager: State<'_, std::sync::Arc<TransferManager>>,
    request: QueueDownloadRequest,
) -> Result<TransferQueueSnapshot, String> {
    transfer_manager
        .queue_download(request)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn queue_upload(
    transfer_manager: State<'_, std::sync::Arc<TransferManager>>,
    request: QueueUploadRequest,
) -> Result<TransferQueueSnapshot, String> {
    transfer_manager
        .queue_upload(request)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn list_transfer_jobs(
    transfer_manager: State<'_, std::sync::Arc<TransferManager>>,
) -> Result<TransferQueueSnapshot, String> {
    Ok(transfer_manager.snapshot().await)
}

#[tauri::command]
pub async fn cancel_transfer(
    transfer_manager: State<'_, std::sync::Arc<TransferManager>>,
    job_id: String,
) -> Result<TransferQueueSnapshot, String> {
    Ok(transfer_manager.cancel_transfer(&job_id).await)
}

#[tauri::command]
pub async fn retry_transfer(
    transfer_manager: State<'_, std::sync::Arc<TransferManager>>,
    job_id: String,
) -> Result<TransferQueueSnapshot, String> {
    Ok(transfer_manager.retry_transfer(&job_id).await)
}

#[tauri::command]
pub async fn resolve_transfer_conflict(
    transfer_manager: State<'_, std::sync::Arc<TransferManager>>,
    job_id: String,
    resolution: TransferConflictResolution,
) -> Result<TransferQueueSnapshot, String> {
    transfer_manager
        .resolve_conflict(&job_id, resolution)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn clear_completed_transfers(
    transfer_manager: State<'_, std::sync::Arc<TransferManager>>,
) -> Result<TransferQueueSnapshot, String> {
    Ok(transfer_manager.clear_completed().await)
}
