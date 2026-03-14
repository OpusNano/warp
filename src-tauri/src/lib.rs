#![allow(dead_code)]

use std::sync::Arc;

use tauri::Manager;

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
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir()?;
            let session_manager = Arc::new(session::SessionManager::new(app.handle().clone(), app_data_dir));
            let transfer_manager = Arc::new(transfer::TransferManager::new(
                app.handle().clone(),
                session_manager.clone(),
            ));
            app.manage(session_manager);
            app.manage(transfer_manager);
            Ok(())
        })
        .manage(local_fs::LocalFilesystem::new())
        .invoke_handler(tauri::generate_handler![
            app::bootstrap_app_state,
            app::list_local_directory,
            app::open_local_directory,
            app::go_up_local_directory,
            app::rename_local_entry,
            app::delete_local_entry,
            app::connect_remote,
            app::resolve_remote_trust,
            app::disconnect_remote,
            app::refresh_remote_directory,
            app::open_remote_directory,
            app::go_up_remote_directory,
            app::create_remote_directory,
            app::rename_remote_entry,
            app::delete_remote_entry,
            app::queue_download,
            app::queue_upload,
            app::list_transfer_jobs,
            app::cancel_transfer,
            app::resolve_transfer_conflict,
            app::clear_completed_transfers
        ])
        .run(tauri::generate_context!())
        .expect("error while running warp")
}
