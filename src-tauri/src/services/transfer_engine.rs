use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use tauri::{AppHandle, Emitter};

use crate::db::{transfer_repo, Database};
use crate::models::transfer::{
    ResumeRecord, TransferDirection, TransferHistory, TransferProgress, TransferStatus,
};
use crate::services::connection::ConnectionManager;
use crate::services::resume;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferTask {
    pub id: String,
    pub host_id: i64,
    pub filename: String,
    pub local_path: String,
    pub remote_path: String,
    pub direction: String,
    pub file_size: u64,
}

impl TransferTask {
    pub fn new(
        host_id: i64,
        filename: String,
        local_path: String,
        remote_path: String,
        direction: String,
        file_size: u64,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            host_id,
            filename,
            local_path,
            remote_path,
            direction,
            file_size,
        }
    }
}

#[derive(Clone)]
pub struct TransferEngine {
    conn_manager: ConnectionManager,
    db: Arc<Database>,
    app_handle: Arc<Mutex<Option<AppHandle>>>,
    active_tasks: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
    task_handles: Arc<Mutex<HashMap<String, std::thread::JoinHandle<()>>>>,
}

impl TransferEngine {
    pub fn new(conn_manager: ConnectionManager, db: Arc<Database>) -> Self {
        Self {
            conn_manager,
            db,
            app_handle: Arc::new(Mutex::new(None)),
            active_tasks: Arc::new(Mutex::new(HashMap::new())),
            task_handles: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn set_app_handle(&self, handle: AppHandle) {
        let mut h = self.app_handle.lock().unwrap();
        *h = Some(handle);
    }

    pub fn submit_task(&self, task: TransferTask) -> Result<String, String> {
        let task_id = task.id.clone();
        let cancel_flag = Arc::new(AtomicBool::new(false));

        {
            let mut active = self.active_tasks.lock().map_err(|e| e.to_string())?;
            active.insert(task_id.clone(), cancel_flag.clone());
        }

        let engine = self.clone();
        let tid = task_id.clone();
        let handle = std::thread::spawn(move || {
            engine.execute_task(task);
            let mut handles = engine.task_handles.lock().unwrap();
            handles.remove(&tid);
        });

        {
            let mut handles = self.task_handles.lock().map_err(|e| e.to_string())?;
            handles.insert(task_id.clone(), handle);
        }

        Ok(task_id)
    }

    pub fn cancel_task(&self, transfer_id: &str) -> Result<(), String> {
        let flag = {
            let active = self.active_tasks.lock().map_err(|e| e.to_string())?;
            active.get(transfer_id).cloned()
        };
        if let Some(flag) = flag {
            flag.store(true, Ordering::Relaxed);
            Ok(())
        } else {
            Err(format!("Transfer {} not found", transfer_id))
        }
    }

    pub fn get_active_task_ids(&self) -> Result<Vec<String>, String> {
        let active = self.active_tasks.lock().map_err(|e| e.to_string())?;
        Ok(active.keys().cloned().collect())
    }

    fn execute_task(&self, task: TransferTask) {
        let direction = match task.direction.as_str() {
            "upload" => TransferDirection::Upload,
            _ => TransferDirection::Download,
        };

        let history = TransferHistory::new(
            task.host_id,
            task.filename.clone(),
            task.remote_path.clone(),
            task.local_path.clone(),
            direction.clone(),
            task.file_size,
        );

        let history_id = {
            let conn = match self.db.conn.lock() {
                Ok(c) => c,
                Err(_) => {
                    self.emit_failed(&task.id, &task.filename, "Database lock failed");
                    self.cleanup_active(&task.id);
                    return;
                }
            };
            match transfer_repo::insert_history(&conn, &history) {
                Ok(h) => h.id.unwrap(),
                Err(e) => {
                    self.emit_failed(&task.id, &task.filename, &e.to_string());
                    self.cleanup_active(&task.id);
                    return;
                }
            }
        };

        {
            let conn = self.db.conn.lock().unwrap();
            let _ = transfer_repo::update_history_status(
                &conn,
                history_id,
                &TransferStatus::Transferring,
                0,
                None,
                None,
            );
        }

        let resume_offset = match resume::find_resume_record(
            &self.db,
            task.host_id,
            &task.remote_path,
            &task.local_path,
            direction.as_str(),
        ) {
            Ok(Some(r)) => r.transferred_bytes,
            _ => 0,
        };

        let conn_arc = match self.conn_manager.get_connection(task.host_id) {
            Ok(c) => c,
            Err(e) => {
                self.finish_task_failed(&task, history_id, &e);
                return;
            }
        };

        let cancel_flag = {
            let active = self.active_tasks.lock().unwrap();
            active.get(&task.id).cloned()
        };

        let cancel_flag = match cancel_flag {
            Some(f) => f,
            None => {
                self.finish_task_failed(&task, history_id, "Task was removed");
                return;
            }
        };

        let app_handle = self.app_handle.lock().unwrap().clone();
        let task_id = task.id.clone();
        let filename = task.filename.clone();
        let total_bytes = task.file_size;
        let start_time = Instant::now();
        let last_resume_save = Arc::new(Mutex::new(Instant::now()));
        let db_for_progress = self.db.clone();
        let host_id = task.host_id;
        let remote_path_c = task.remote_path.clone();
        let local_path_c = task.local_path.clone();
        let direction_c = direction.clone();
        let cancel_for_progress = cancel_flag.clone();

        // 各连接实现传入的 transferred 为「文件内绝对已传输位置」（含断点 offset），非仅本次会话增量。
        let cf_for_poll = cancel_flag.clone();
        let check_cancel_for_conn = move || cf_for_poll.load(Ordering::Relaxed);

        let progress_fn = move |transferred: u64, _total: u64| {
            if cancel_for_progress.load(Ordering::Relaxed) {
                return;
            }

            let elapsed = start_time.elapsed().as_secs_f64();
            let effective_transferred = transferred.min(total_bytes);
            let session_bytes = effective_transferred.saturating_sub(resume_offset);
            let speed = if elapsed > 0.0 {
                session_bytes as f64 / elapsed
            } else {
                0.0
            };
            let remaining = if speed > 0.0 && total_bytes > effective_transferred {
                (total_bytes - effective_transferred) as f64 / speed
            } else {
                0.0
            };
            let percentage = if total_bytes > 0 {
                (effective_transferred as f64 / total_bytes as f64) * 100.0
            } else {
                0.0
            };

            let progress = TransferProgress {
                transfer_id: task_id.clone(),
                filename: filename.clone(),
                total_bytes,
                transferred_bytes: effective_transferred,
                speed_bytes_per_sec: speed,
                eta_seconds: remaining,
                percentage,
            };

            if let Some(ref handle) = app_handle {
                let _ = handle.emit("transfer-progress", &progress);
            }

            let mut last = last_resume_save.lock().unwrap();
            if last.elapsed().as_secs() >= 3 {
                *last = Instant::now();
                let record = ResumeRecord::new(
                    task_id.clone(),
                    host_id,
                    remote_path_c.clone(),
                    local_path_c.clone(),
                    direction_c.clone(),
                    total_bytes,
                );
                let mut record = record;
                record.transferred_bytes = effective_transferred;
                let _ = resume::save_resume_record(&db_for_progress, &record);
            }
        };

        let result = {
            let mut conn_guard = match conn_arc.lock() {
                Ok(g) => g,
                Err(e) => {
                    self.finish_task_failed(&task, history_id, &e.to_string());
                    return;
                }
            };

            match direction {
                TransferDirection::Upload => conn_guard.upload(
                    &task.local_path,
                    &task.remote_path,
                    resume_offset,
                    Some(&progress_fn),
                    Some(&check_cancel_for_conn),
                ),
                TransferDirection::Download => conn_guard.download(
                    &task.remote_path,
                    &task.local_path,
                    resume_offset,
                    Some(&progress_fn),
                    Some(&check_cancel_for_conn),
                ),
            }
        };

        if cancel_flag.load(Ordering::Relaxed) {
            let conn = self.db.conn.lock().unwrap();
            let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
            let _ = transfer_repo::update_history_status(
                &conn,
                history_id,
                &TransferStatus::Cancelled,
                0,
                None,
                Some(&now),
            );
            drop(conn);
            self.emit_event("transfer-cancelled", &task.id, &task.filename);
            self.cleanup_active(&task.id);
            return;
        }

        match result {
            Ok(bytes) => {
                let total_transferred = resume_offset + bytes;
                let conn = self.db.conn.lock().unwrap();
                let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
                let _ = transfer_repo::update_history_status(
                    &conn,
                    history_id,
                    &TransferStatus::Success,
                    total_transferred,
                    None,
                    Some(&now),
                );
                drop(conn);
                let _ = resume::delete_resume_record(&self.db, &task.id);

                self.emit_event("transfer-complete", &task.id, &task.filename);
            }
            Err(e) => {
                self.finish_task_failed(&task, history_id, &e);
            }
        }

        self.cleanup_active(&task.id);
    }

    fn finish_task_failed(&self, task: &TransferTask, history_id: i64, error: &str) {
        let conn = self.db.conn.lock().unwrap();
        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let _ = transfer_repo::update_history_status(
            &conn,
            history_id,
            &TransferStatus::Failed,
            0,
            Some(error),
            Some(&now),
        );
        drop(conn);

        let direction = match task.direction.as_str() {
            "upload" => TransferDirection::Upload,
            _ => TransferDirection::Download,
        };
        let record = ResumeRecord::new(
            task.id.clone(),
            task.host_id,
            task.remote_path.clone(),
            task.local_path.clone(),
            direction,
            task.file_size,
        );
        let _ = resume::save_resume_record(&self.db, &record);

        self.emit_failed(&task.id, &task.filename, error);
        self.cleanup_active(&task.id);
    }

    fn cleanup_active(&self, task_id: &str) {
        if let Ok(mut active) = self.active_tasks.lock() {
            active.remove(task_id);
        }
    }

    fn emit_event(&self, event: &str, transfer_id: &str, filename: &str) {
        if let Some(ref handle) = *self.app_handle.lock().unwrap() {
            #[derive(Serialize, Clone)]
            struct TransferEvent {
                transfer_id: String,
                filename: String,
            }
            let _ = handle.emit(
                event,
                TransferEvent {
                    transfer_id: transfer_id.to_string(),
                    filename: filename.to_string(),
                },
            );
        }
    }

    fn emit_failed(&self, transfer_id: &str, filename: &str, error: &str) {
        if let Some(ref handle) = *self.app_handle.lock().unwrap() {
            #[derive(Serialize, Clone)]
            struct TransferFailedEvent {
                transfer_id: String,
                filename: String,
                error: String,
            }
            let _ = handle.emit(
                "transfer-failed",
                TransferFailedEvent {
                    transfer_id: transfer_id.to_string(),
                    filename: filename.to_string(),
                    error: error.to_string(),
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrations;
    use crate::services::connection::ConnectionTrait;
    use rusqlite::Connection;
    use std::io::Write;
    use tempfile::NamedTempFile;

    struct MockClient {
        connected: bool,
    }

    impl MockClient {
        fn new() -> Self {
            Self { connected: true }
        }
    }

    impl ConnectionTrait for MockClient {
        fn connect(&mut self) -> Result<(), String> {
            self.connected = true;
            Ok(())
        }
        fn disconnect(&mut self) -> Result<(), String> {
            self.connected = false;
            Ok(())
        }
        fn is_connected(&self) -> bool {
            self.connected
        }
        fn list_dir(
            &mut self,
            _path: &str,
        ) -> Result<Vec<crate::services::connection::FileEntry>, String> {
            Ok(vec![])
        }
        fn file_size(&mut self, _path: &str) -> Result<u64, String> {
            Ok(0)
        }
        fn file_exists(&mut self, _path: &str) -> Result<bool, String> {
            Ok(true)
        }
        fn upload(
            &mut self,
            _local_path: &str,
            _remote_path: &str,
            _offset: u64,
            progress: Option<&dyn Fn(u64, u64)>,
            is_cancelled: Option<&dyn Fn() -> bool>,
        ) -> Result<u64, String> {
            if is_cancelled.is_some_and(|f| f()) {
                return Err("Transfer cancelled".to_string());
            }
            if let Some(cb) = progress {
                cb(100, 100);
            }
            Ok(100)
        }
        fn download(
            &mut self,
            _remote_path: &str,
            _local_path: &str,
            _offset: u64,
            progress: Option<&dyn Fn(u64, u64)>,
            is_cancelled: Option<&dyn Fn() -> bool>,
        ) -> Result<u64, String> {
            if is_cancelled.is_some_and(|f| f()) {
                return Err("Transfer cancelled".to_string());
            }
            if let Some(cb) = progress {
                cb(100, 100);
            }
            Ok(100)
        }
        fn mkdir(&mut self, _path: &str) -> Result<(), String> {
            Ok(())
        }
        fn remove_file(&mut self, _path: &str) -> Result<(), String> {
            Ok(())
        }
        fn remove_dir(&mut self, _path: &str) -> Result<(), String> {
            Ok(())
        }
        fn rename(&mut self, _from: &str, _to: &str) -> Result<(), String> {
            Ok(())
        }
    }

    struct SlowUploadMockClient {
        connected: bool,
    }

    impl SlowUploadMockClient {
        fn new() -> Self {
            Self { connected: true }
        }
    }

    impl ConnectionTrait for SlowUploadMockClient {
        fn connect(&mut self) -> Result<(), String> {
            self.connected = true;
            Ok(())
        }
        fn disconnect(&mut self) -> Result<(), String> {
            self.connected = false;
            Ok(())
        }
        fn is_connected(&self) -> bool {
            self.connected
        }
        fn list_dir(
            &mut self,
            _path: &str,
        ) -> Result<Vec<crate::services::connection::FileEntry>, String> {
            Ok(vec![])
        }
        fn file_size(&mut self, _path: &str) -> Result<u64, String> {
            Ok(0)
        }
        fn file_exists(&mut self, _path: &str) -> Result<bool, String> {
            Ok(true)
        }
        fn upload(
            &mut self,
            _local_path: &str,
            _remote_path: &str,
            _offset: u64,
            progress: Option<&dyn Fn(u64, u64)>,
            is_cancelled: Option<&dyn Fn() -> bool>,
        ) -> Result<u64, String> {
            const CHUNK: u64 = 8192;
            let total_size = 512 * 1024_u64;
            let mut pos = 0_u64;
            while pos < total_size {
                if is_cancelled.is_some_and(|f| f()) {
                    return Err("Transfer cancelled".to_string());
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
                pos = (pos + CHUNK).min(total_size);
                if let Some(cb) = progress {
                    cb(pos, total_size);
                }
            }
            Ok(total_size)
        }
        fn download(
            &mut self,
            _remote_path: &str,
            _local_path: &str,
            _offset: u64,
            _progress: Option<&dyn Fn(u64, u64)>,
            _is_cancelled: Option<&dyn Fn() -> bool>,
        ) -> Result<u64, String> {
            Err("SlowUploadMockClient only implements upload".to_string())
        }
        fn mkdir(&mut self, _path: &str) -> Result<(), String> {
            Ok(())
        }
        fn remove_file(&mut self, _path: &str) -> Result<(), String> {
            Ok(())
        }
        fn remove_dir(&mut self, _path: &str) -> Result<(), String> {
            Ok(())
        }
        fn rename(&mut self, _from: &str, _to: &str) -> Result<(), String> {
            Ok(())
        }
    }

    fn setup_test_db() -> Arc<Database> {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        migrations::run_all(&conn).unwrap();
        conn.execute(
            "INSERT INTO hosts (name, host, port, protocol, username) VALUES ('test', 'localhost', 22, 'sftp', 'user')",
            [],
        )
        .unwrap();
        Arc::new(Database::new_test(conn).unwrap())
    }

    fn setup_engine() -> TransferEngine {
        let db = setup_test_db();
        let conn_manager = ConnectionManager::new();
        conn_manager
            .insert_mock_connection(1, Box::new(MockClient::new()))
            .unwrap();
        TransferEngine::new(conn_manager, db)
    }

    fn setup_engine_slow_upload() -> TransferEngine {
        let db = setup_test_db();
        let conn_manager = ConnectionManager::new();
        conn_manager
            .insert_mock_connection(1, Box::new(SlowUploadMockClient::new()))
            .unwrap();
        TransferEngine::new(conn_manager, db)
    }

    fn create_temp_file() -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(&[0u8; 100]).unwrap();
        f
    }

    #[test]
    fn test_transfer_task_creation() {
        let task = TransferTask::new(
            1,
            "test.txt".to_string(),
            "/local/test.txt".to_string(),
            "/remote/test.txt".to_string(),
            "upload".to_string(),
            1024,
        );
        assert!(!task.id.is_empty());
        assert_eq!(task.host_id, 1);
        assert_eq!(task.filename, "test.txt");
        assert_eq!(task.file_size, 1024);
    }

    #[test]
    fn test_transfer_task_unique_ids() {
        let task1 =
            TransferTask::new(1, "a.txt".into(), "/a".into(), "/a".into(), "upload".into(), 0);
        let task2 =
            TransferTask::new(1, "b.txt".into(), "/b".into(), "/b".into(), "upload".into(), 0);
        assert_ne!(task1.id, task2.id);
    }

    #[test]
    fn test_upload_completes_without_deadlock() {
        let engine = setup_engine();
        let tmp = create_temp_file();
        let local_path = tmp.path().to_str().unwrap().to_string();

        let task = TransferTask::new(
            1,
            "test.txt".into(),
            local_path,
            "/remote/test.txt".into(),
            "upload".into(),
            100,
        );
        let task_id = task.id.clone();
        engine.submit_task(task).unwrap();

        let deadline = Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let ids = engine.get_active_task_ids().unwrap();
            if !ids.contains(&task_id) {
                break;
            }
            if Instant::now() > deadline {
                panic!("deadlock detected: task not cleaned up within 5s");
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        assert!(
            engine.get_active_task_ids().unwrap().is_empty(),
            "active_tasks should be empty after upload completes"
        );
    }

    #[test]
    fn test_download_completes_without_deadlock() {
        let engine = setup_engine();
        let tmp = create_temp_file();
        let local_path = tmp.path().to_str().unwrap().to_string();

        let task = TransferTask::new(
            1,
            "test.txt".into(),
            local_path,
            "/remote/test.txt".into(),
            "download".into(),
            100,
        );
        let task_id = task.id.clone();
        engine.submit_task(task).unwrap();

        let deadline = Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let ids = engine.get_active_task_ids().unwrap();
            if !ids.contains(&task_id) {
                break;
            }
            if Instant::now() > deadline {
                panic!("deadlock detected: task not cleaned up within 5s");
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        assert!(engine.get_active_task_ids().unwrap().is_empty());
    }

    #[test]
    fn test_multiple_uploads_complete_sequentially() {
        let engine = setup_engine();

        for i in 0..3 {
            let tmp = create_temp_file();
            let local_path = tmp.path().to_str().unwrap().to_string();
            let task = TransferTask::new(
                1,
                format!("file_{}.txt", i),
                local_path,
                format!("/remote/file_{}.txt", i),
                "upload".into(),
                100,
            );
            let task_id = task.id.clone();
            engine.submit_task(task).unwrap();

            let deadline = Instant::now() + std::time::Duration::from_secs(5);
            loop {
                let ids = engine.get_active_task_ids().unwrap();
                if !ids.contains(&task_id) {
                    break;
                }
                if Instant::now() > deadline {
                    panic!("deadlock detected on task {}", i);
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }

        assert!(engine.get_active_task_ids().unwrap().is_empty());
    }

    #[test]
    fn test_cancel_task_cleans_up() {
        let engine = setup_engine();
        let tmp = create_temp_file();
        let local_path = tmp.path().to_str().unwrap().to_string();

        let task = TransferTask::new(
            1,
            "cancel_me.txt".into(),
            local_path,
            "/remote/cancel_me.txt".into(),
            "upload".into(),
            100,
        );
        let task_id = task.id.clone();
        engine.submit_task(task).unwrap();

        let _ = engine.cancel_task(&task_id);

        let deadline = Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let ids = engine.get_active_task_ids().unwrap();
            if !ids.contains(&task_id) {
                break;
            }
            if Instant::now() > deadline {
                panic!("deadlock detected: cancelled task not cleaned up within 5s");
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        assert!(engine.get_active_task_ids().unwrap().is_empty());
    }

    #[test]
    fn test_cancel_mid_slow_upload_stops_transfer() {
        let engine = setup_engine_slow_upload();
        let tmp = create_temp_file();
        let local_path = tmp.path().to_str().unwrap().to_string();

        let task = TransferTask::new(
            1,
            "slow.bin".into(),
            local_path,
            "/remote/slow.bin".into(),
            "upload".into(),
            512 * 1024,
        );
        let task_id = task.id.clone();
        engine.submit_task(task).unwrap();

        std::thread::sleep(std::time::Duration::from_millis(150));
        let _ = engine.cancel_task(&task_id);

        let deadline = Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let ids = engine.get_active_task_ids().unwrap();
            if !ids.contains(&task_id) {
                break;
            }
            if Instant::now() > deadline {
                panic!("cancelled slow upload did not finish within 10s");
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        assert!(engine.get_active_task_ids().unwrap().is_empty());
    }

    /// 与 execute_task 内 progress 回调一致：progress 传入绝对偏移，速度按本次会话字节数估算。
    #[test]
    fn test_progress_absolute_offset_session_speed() {
        let total_bytes = 1000_u64;
        let resume_offset = 200_u64;
        let transferred_abs = 500_u64;
        let elapsed = 2.0_f64;

        let effective_transferred = transferred_abs.min(total_bytes);
        let session_bytes = effective_transferred.saturating_sub(resume_offset);
        let speed = session_bytes as f64 / elapsed;

        assert_eq!(effective_transferred, 500);
        assert_eq!(session_bytes, 300);
        assert_eq!(speed, 150.0);

        let remaining = if speed > 0.0 && total_bytes > effective_transferred {
            (total_bytes - effective_transferred) as f64 / speed
        } else {
            0.0
        };
        assert!((remaining - 500.0 / 150.0).abs() < f64::EPSILON);
    }
}
