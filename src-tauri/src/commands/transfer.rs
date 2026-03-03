use crate::db::transfer_repo;
use crate::models::transfer::{TransferDirection, TransferHistory};
use crate::services::connection::ConnectionManager;
use crate::services::transfer_engine::{TransferEngine, TransferTask};
use crate::utils::path::{normalize_and_validate, normalize_path_for_create, sanitize_filename};
use crate::SharedDatabase;
use tauri::State;

struct DirWalkResult {
    files: Vec<(String, String, String, u64)>,
    dirs: Vec<String>,
}

fn collect_local_dir_entries(local_dir: &str, remote_dir: &str) -> Result<DirWalkResult, String> {
    let safe_local = normalize_and_validate(local_dir)?;
    let mut files = Vec::new();
    let mut dirs = Vec::new();
    let mut queue = vec![(safe_local.to_string_lossy().to_string(), remote_dir.to_string())];

    while let Some((local, remote)) = queue.pop() {
        dirs.push(remote.clone());
        let entries = std::fs::read_dir(&local)
            .map_err(|e| format!("读取目录失败 {}: {}", local, e))?;
        for entry in entries {
            let entry = entry.map_err(|e| e.to_string())?;
            let metadata = entry.metadata().map_err(|e| e.to_string())?;
            let name = entry.file_name().to_string_lossy().to_string();
            let entry_local = entry.path().to_string_lossy().to_string();
            let entry_remote = format!("{}/{}", remote.trim_end_matches('/'), name);
            if metadata.is_dir() {
                queue.push((entry_local, entry_remote));
            } else {
                files.push((entry_local, entry_remote, name, metadata.len()));
            }
        }
    }

    Ok(DirWalkResult { files, dirs })
}

#[tauri::command]
pub fn get_transfer_history(
    db: State<'_, SharedDatabase>,
    host_id: Option<i64>,
) -> Result<Vec<TransferHistory>, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    match host_id {
        Some(hid) => transfer_repo::get_history_by_host(&conn, hid),
        None => transfer_repo::get_all_history(&conn),
    }
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn clear_transfer_history(db: State<'_, SharedDatabase>) -> Result<(), String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    transfer_repo::clear_history(&conn)
        .map_err(|e| e.to_string())
        .map(|_| ())
}

#[tauri::command]
pub fn clear_transfer_history_by_host(
    host_id: i64,
    db: State<'_, SharedDatabase>,
) -> Result<(), String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    transfer_repo::clear_history_by_host(&conn, host_id)
        .map_err(|e| e.to_string())
        .map(|_| ())
}

#[tauri::command]
pub fn start_upload(
    host_id: i64,
    local_path: String,
    remote_path: String,
    filename: String,
    file_size: u64,
    engine: State<'_, TransferEngine>,
) -> Result<String, String> {
    let _ = normalize_and_validate(&local_path)?;
    let task = TransferTask::new(
        host_id,
        filename,
        local_path,
        remote_path,
        "upload".to_string(),
        file_size,
    );
    engine.submit_task(task)
}

#[tauri::command]
pub fn start_download(
    host_id: i64,
    remote_path: String,
    local_path: String,
    filename: String,
    file_size: u64,
    engine: State<'_, TransferEngine>,
) -> Result<String, String> {
    let safe_local = normalize_path_for_create(&local_path)?;
    let task = TransferTask::new(
        host_id,
        filename,
        safe_local.to_string_lossy().to_string(),
        remote_path,
        "download".to_string(),
        file_size,
    );
    engine.submit_task(task)
}

#[tauri::command]
pub fn cancel_transfer(
    transfer_id: String,
    engine: State<'_, TransferEngine>,
) -> Result<(), String> {
    engine.cancel_task(&transfer_id)
}

#[tauri::command]
pub fn retry_transfer(
    history_id: i64,
    db: State<'_, SharedDatabase>,
    engine: State<'_, TransferEngine>,
) -> Result<String, String> {
    let history = {
        let conn = db.conn.lock().map_err(|e| e.to_string())?;
        transfer_repo::get_history_by_id(&conn, history_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("History {} not found", history_id))?
    };

    let direction = match history.direction {
        TransferDirection::Upload => "upload",
        TransferDirection::Download => "download",
    };

    let task = TransferTask::new(
        history.host_id,
        history.filename,
        history.local_path,
        history.remote_path,
        direction.to_string(),
        history.file_size,
    );
    engine.submit_task(task)
}

#[tauri::command]
pub fn get_resume_records(
    host_id: i64,
    db: State<'_, SharedDatabase>,
) -> Result<Vec<crate::models::transfer::ResumeRecord>, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT id, transfer_id, host_id, remote_path, local_path, direction,
                    file_size, transferred_bytes, checksum, created_at
             FROM resume_records WHERE host_id = ?1 ORDER BY created_at DESC",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map(rusqlite::params![host_id], |row| {
            let dir_str: String = row.get(5)?;
            let direction =
                TransferDirection::from_str(&dir_str).unwrap_or(TransferDirection::Upload);
            Ok(crate::models::transfer::ResumeRecord {
                id: row.get(0)?,
                transfer_id: row.get(1)?,
                host_id: row.get(2)?,
                remote_path: row.get(3)?,
                local_path: row.get(4)?,
                direction,
                file_size: row.get(6)?,
                transferred_bytes: row.get(7)?,
                checksum: row.get(8)?,
                created_at: row.get(9)?,
            })
        })
        .map_err(|e| e.to_string())?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn check_local_file_exists(path: String) -> Result<bool, String> {
    let safe_path = normalize_path_for_create(&path)?;
    Ok(safe_path.exists())
}

#[tauri::command]
pub fn get_local_file_size(path: String) -> Result<u64, String> {
    let safe_path = normalize_and_validate(&path)?;
    let metadata = std::fs::metadata(&safe_path).map_err(|e| e.to_string())?;
    Ok(metadata.len())
}

#[tauri::command]
pub async fn start_directory_upload(
    host_id: i64,
    local_dir: String,
    remote_dir: String,
    manager: State<'_, ConnectionManager>,
    engine: State<'_, TransferEngine>,
) -> Result<Vec<String>, String> {
    let _ = normalize_and_validate(&local_dir)?;
    let entries = collect_local_dir_entries(&local_dir, &remote_dir)?;

    let conn_arc = manager.get_connection(host_id)?;
    let engine = engine.inner().clone();
    let dirs = entries.dirs;
    tokio::task::spawn_blocking(move || {
        let mut conn = conn_arc.lock().map_err(|e| e.to_string())?;
        for dir in &dirs {
            let _ = conn.mkdir(dir);
        }
        Ok::<(), String>(())
    })
    .await
    .map_err(|e| e.to_string())??;

    let mut transfer_ids = Vec::new();
    for (local_path, remote_path, filename, file_size) in entries.files {
        let task = TransferTask::new(
            host_id,
            filename,
            local_path,
            remote_path,
            "upload".to_string(),
            file_size,
        );
        transfer_ids.push(engine.submit_task(task)?);
    }

    Ok(transfer_ids)
}

#[tauri::command]
pub async fn start_directory_download(
    host_id: i64,
    remote_dir: String,
    local_dir: String,
    manager: State<'_, ConnectionManager>,
    engine: State<'_, TransferEngine>,
) -> Result<Vec<String>, String> {
    let safe_local_dir = normalize_path_for_create(&local_dir)?;
    let safe_local_str = safe_local_dir.to_string_lossy().to_string();
    let conn_arc = manager.get_connection(host_id)?;
    let engine = engine.inner().clone();

    let (files, dirs_to_create) = tokio::task::spawn_blocking(move || {
        let mut conn = conn_arc.lock().map_err(|e| e.to_string())?;
        let mut files: Vec<(String, String, String, u64)> = Vec::new();
        let mut dirs: Vec<String> = Vec::new();
        let mut queue = vec![(remote_dir, safe_local_str)];

        while let Some((remote, local)) = queue.pop() {
            dirs.push(local.clone());
            let entries = conn.list_dir(&remote)?;
            for entry in entries {
                if entry.name == "." || entry.name == ".." {
                    continue;
                }
                let safe_name = sanitize_filename(&entry.name)
                    .map_err(|e| e.to_string())?;
                let entry_local = std::path::Path::new(&local)
                    .join(&safe_name)
                    .to_string_lossy()
                    .to_string();
                if entry.is_dir {
                    let entry_remote =
                        format!("{}/{}", remote.trim_end_matches('/'), entry.name);
                    queue.push((entry_remote, entry_local));
                } else {
                    files.push((entry.path, entry_local, entry.name, entry.size));
                }
            }
        }

        Ok::<_, String>((files, dirs))
    })
    .await
    .map_err(|e| e.to_string())??;

    for dir in &dirs_to_create {
        let _ = normalize_path_for_create(dir)?;
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("创建本地目录失败 {}: {}", dir, e))?;
    }

    let mut transfer_ids = Vec::new();
    for (remote_path, local_path, filename, file_size) in files {
        let task = TransferTask::new(
            host_id,
            filename,
            local_path,
            remote_path,
            "download".to_string(),
            file_size,
        );
        transfer_ids.push(engine.submit_task(task)?);
    }

    Ok(transfer_ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collect_local_dir_entries_flat() {
        let temp = std::env::temp_dir().join("ftx_test_dir_collect_flat");
        let _ = std::fs::remove_dir_all(&temp);
        std::fs::create_dir_all(&temp).unwrap();
        std::fs::write(temp.join("a.txt"), "hello").unwrap();
        std::fs::write(temp.join("b.txt"), "world").unwrap();

        let result =
            collect_local_dir_entries(&temp.to_string_lossy(), "/remote/testdir").unwrap();

        assert_eq!(result.dirs.len(), 1);
        assert_eq!(result.dirs[0], "/remote/testdir");
        assert_eq!(result.files.len(), 2);
        let filenames: Vec<&str> = result.files.iter().map(|f| f.2.as_str()).collect();
        assert!(filenames.contains(&"a.txt"));
        assert!(filenames.contains(&"b.txt"));

        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn test_collect_local_dir_entries_nested() {
        let temp = std::env::temp_dir().join("ftx_test_dir_collect_nested");
        let _ = std::fs::remove_dir_all(&temp);
        std::fs::create_dir_all(temp.join("sub1/sub2")).unwrap();
        std::fs::write(temp.join("root.txt"), "root").unwrap();
        std::fs::write(temp.join("sub1/mid.txt"), "mid").unwrap();
        std::fs::write(temp.join("sub1/sub2/deep.txt"), "deep").unwrap();

        let result =
            collect_local_dir_entries(&temp.to_string_lossy(), "/remote/nested").unwrap();

        assert_eq!(result.dirs.len(), 3);
        assert!(result.dirs.contains(&"/remote/nested".to_string()));
        assert!(result.dirs.contains(&"/remote/nested/sub1".to_string()));
        assert!(result.dirs.contains(&"/remote/nested/sub1/sub2".to_string()));

        assert_eq!(result.files.len(), 3);
        let filenames: Vec<&str> = result.files.iter().map(|f| f.2.as_str()).collect();
        assert!(filenames.contains(&"root.txt"));
        assert!(filenames.contains(&"mid.txt"));
        assert!(filenames.contains(&"deep.txt"));

        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn test_collect_local_dir_entries_empty() {
        let temp = std::env::temp_dir().join("ftx_test_dir_collect_empty");
        let _ = std::fs::remove_dir_all(&temp);
        std::fs::create_dir_all(&temp).unwrap();

        let result =
            collect_local_dir_entries(&temp.to_string_lossy(), "/remote/empty").unwrap();

        assert_eq!(result.dirs.len(), 1);
        assert_eq!(result.dirs[0], "/remote/empty");
        assert_eq!(result.files.len(), 0);

        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn test_collect_local_dir_entries_trailing_slash() {
        let temp = std::env::temp_dir().join("ftx_test_dir_collect_slash");
        let _ = std::fs::remove_dir_all(&temp);
        std::fs::create_dir_all(&temp).unwrap();
        std::fs::write(temp.join("f.txt"), "data").unwrap();

        let result =
            collect_local_dir_entries(&temp.to_string_lossy(), "/remote/dir/").unwrap();

        let file_remote = &result.files[0].1;
        assert!(
            file_remote == "/remote/dir/f.txt",
            "Expected /remote/dir/f.txt, got {}",
            file_remote
        );

        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn test_collect_local_dir_entries_nonexistent() {
        let result = collect_local_dir_entries("/nonexistent/path/xyz", "/remote/dir");
        assert!(result.is_err());
    }
}
