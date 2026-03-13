use tauri::State;

use crate::{
    local_fs::LocalFilesystem,
    models::{AppBootstrap, PaneSet, PaneSnapshot},
};

#[tauri::command]
pub fn bootstrap_app_state(local_fs: State<'_, LocalFilesystem>) -> Result<AppBootstrap, String> {
    let local = local_fs
        .list_directory(None)
        .map_err(|error| error.to_string())?;

    Ok(AppBootstrap {
        connection_profiles: AppBootstrap::sample_profiles(),
        session: AppBootstrap::sample_session(),
        panes: PaneSet {
            local,
            remote: AppBootstrap::remote_mock(),
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
