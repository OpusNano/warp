#![allow(dead_code)]

mod app;
mod events;
mod local_fs;
mod logging;
mod models;
mod profiles;
mod remote_sftp;
mod scp_compat;
mod session;
mod store;
mod transfer;
mod trust;

pub fn run() {
    logging::init();

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![app::bootstrap_app_state])
        .run(tauri::generate_context!())
        .expect("error while running warp")
}
