use crate::models::AppBootstrap;

#[tauri::command]
pub fn bootstrap_app_state() -> AppBootstrap {
    AppBootstrap::sample()
}
