use tauri::State;

use crate::{
    local_fs::LocalFilesystem,
    models::{AppBootstrap, ConnectRequest, PaneSet, PaneSnapshot, RemoteConnectionSnapshot, TrustDecision},
    session::SessionManager,
};

#[tauri::command]
pub async fn bootstrap_app_state(
    local_fs: State<'_, LocalFilesystem>,
    session_manager: State<'_, SessionManager>,
) -> Result<AppBootstrap, String> {
    let local = local_fs
        .list_directory(None)
        .map_err(|error| error.to_string())?;

    let remote = session_manager.snapshot().await;

    Ok(AppBootstrap {
        connection_profiles: AppBootstrap::sample_profiles(),
        session: remote.session,
        panes: PaneSet {
            local,
            remote: remote.remote_pane,
        },
        transfers: Vec::new(),
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
pub fn delete_local_entry(
    local_fs: State<'_, LocalFilesystem>,
    path: String,
    entry_name: String,
) -> Result<PaneSnapshot, String> {
    local_fs
        .delete_entry(&path, &entry_name)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn connect_remote(
    session_manager: State<'_, SessionManager>,
    request: ConnectRequest,
) -> Result<RemoteConnectionSnapshot, String> {
    session_manager
        .connect(request)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn resolve_remote_trust(
    session_manager: State<'_, SessionManager>,
    decision: TrustDecision,
) -> Result<RemoteConnectionSnapshot, String> {
    session_manager
        .resolve_trust(decision)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn disconnect_remote(
    session_manager: State<'_, SessionManager>,
) -> Result<RemoteConnectionSnapshot, String> {
    session_manager
        .disconnect()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn refresh_remote_directory(
    session_manager: State<'_, SessionManager>,
) -> Result<RemoteConnectionSnapshot, String> {
    session_manager
        .refresh_remote_directory()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn open_remote_directory(
    session_manager: State<'_, SessionManager>,
    path: String,
) -> Result<RemoteConnectionSnapshot, String> {
    session_manager
        .open_remote_directory(path)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn go_up_remote_directory(
    session_manager: State<'_, SessionManager>,
) -> Result<RemoteConnectionSnapshot, String> {
    session_manager
        .go_up_remote_directory()
        .await
        .map_err(|error| error.to_string())
}
