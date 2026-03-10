pub mod commands;
pub mod crypto;
pub mod db;
pub mod models;
pub mod services;
pub mod utils;
pub mod validation;

use db::Database;
use services::connection::ConnectionManager;
use services::transfer_engine::TransferEngine;
use std::sync::Arc;
use tauri::Manager;

/// Wrapper so we can put Arc<Database> into Tauri's managed state
/// while also sharing it with TransferEngine.
pub struct SharedDatabase(pub Arc<Database>);

impl std::ops::Deref for SharedDatabase {
    type Target = Database;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let app_data_dir = app
                .path()
                .app_data_dir()
                .expect("Failed to get app data dir");
            let database = Database::new(app_data_dir)
                .map_err(|e| e.to_string())
                .expect("Failed to initialize database");
            let db_arc = Arc::new(database);

            let idle_timeout = {
                let conn = db_arc.conn.lock().unwrap();
                db::settings_repo::get_idle_timeout_secs(&conn)
                    .unwrap_or(services::connection::DEFAULT_IDLE_TIMEOUT_SECS)
            };
            let conn_manager = ConnectionManager::with_idle_timeout(idle_timeout);
            let engine = TransferEngine::new(conn_manager.clone(), db_arc.clone());
            engine.set_app_handle(app.handle().clone());

            app.manage(SharedDatabase(db_arc));
            app.manage(conn_manager);
            app.manage(engine);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::host::get_hosts,
            commands::host::create_host,
            commands::host::update_host,
            commands::host::delete_host,
            commands::transfer::get_transfer_history,
            commands::transfer::clear_transfer_history,
            commands::transfer::clear_transfer_history_by_host,
            commands::transfer::start_upload,
            commands::transfer::start_download,
            commands::transfer::cancel_transfer,
            commands::transfer::retry_transfer,
            commands::transfer::get_resume_records,
            commands::transfer::check_local_file_exists,
            commands::transfer::get_local_file_size,
            commands::transfer::start_directory_upload,
            commands::transfer::start_directory_download,
            commands::file_browser::list_local_dir,
            commands::bookmark::get_bookmarks,
            commands::bookmark::get_all_bookmarks,
            commands::bookmark::create_bookmark,
            commands::bookmark::delete_bookmark,
            commands::bookmark::touch_bookmark,
            commands::connection::connect_host,
            commands::connection::disconnect_host,
            commands::connection::test_connection,
            commands::connection::test_connection_by_id,
            commands::connection::connection_status,
            commands::connection::active_connections,
            commands::connection::list_remote_dir,
            commands::connection::create_remote_dir,
            commands::connection::delete_remote_file,
            commands::connection::delete_remote_dir,
            commands::connection::rename_remote,
            commands::connection::remote_file_exists,
            commands::connection::remote_file_size,
            commands::settings::get_idle_timeout_secs,
            commands::settings::set_idle_timeout_secs,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
