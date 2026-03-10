use crate::db::settings_repo;
use crate::services::connection::ConnectionManager;
use crate::SharedDatabase;
use tauri::State;

#[tauri::command]
pub fn get_idle_timeout_secs(
    db: State<'_, SharedDatabase>,
    manager: State<'_, ConnectionManager>,
) -> Result<u64, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    match settings_repo::get_idle_timeout_secs(&conn) {
        Ok(secs) => Ok(secs),
        Err(_) => Ok(manager.idle_timeout_secs()),
    }
}

#[tauri::command]
pub fn set_idle_timeout_secs(
    secs: u64,
    db: State<'_, SharedDatabase>,
    manager: State<'_, ConnectionManager>,
) -> Result<(), String> {
    if secs > 86400 {
        return Err("空闲超时时间不能超过 24 小时 (86400 秒)".to_string());
    }
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    settings_repo::set_idle_timeout_secs(&conn, secs)?;
    manager.set_idle_timeout(secs);
    Ok(())
}
