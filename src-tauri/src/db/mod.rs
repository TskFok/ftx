pub mod bookmark_repo;
pub mod host_repo;
pub mod migrations;
pub mod schema;
pub mod transfer_repo;

use crate::crypto;
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::Mutex;

pub struct Database {
    pub conn: Mutex<Connection>,
    encryption_key: Option<[u8; 32]>,
}

impl Database {
    pub fn new(app_data_dir: PathBuf) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        std::fs::create_dir_all(&app_data_dir).ok();
        let db_path = app_data_dir.join("ftx_tool.db");
        let conn = Connection::open(&db_path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        let encryption_key = crypto::load_or_create_key(&app_data_dir).ok();
        let db = Self {
            conn: Mutex::new(conn),
            encryption_key,
        };
        db.run_migrations()?;
        Ok(db)
    }

    #[cfg(test)]
    pub fn new_test(conn: Connection) -> Result<Self, rusqlite::Error> {
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        let db = Self {
            conn: Mutex::new(conn),
            encryption_key: None,
        };
        db.run_migrations()?;
        Ok(db)
    }

    pub fn encryption_key(&self) -> Option<&[u8; 32]> {
        self.encryption_key.as_ref()
    }

    fn run_migrations(&self) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        migrations::run_all(&conn)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_database_creation_in_memory() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .unwrap();
        migrations::run_all(&conn).unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM hosts", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_all_tables_created() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        migrations::run_all(&conn).unwrap();

        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert!(tables.contains(&"hosts".to_string()));
        assert!(tables.contains(&"transfer_history".to_string()));
        assert!(tables.contains(&"directory_bookmarks".to_string()));
        assert!(tables.contains(&"resume_records".to_string()));
    }

    #[test]
    fn test_indices_created() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        migrations::run_all(&conn).unwrap();

        let indices: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='index' AND name LIKE 'idx_%' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert!(indices.contains(&"idx_transfer_history_host_id".to_string()));
        assert!(indices.contains(&"idx_transfer_history_status".to_string()));
        assert!(indices.contains(&"idx_transfer_history_started_at".to_string()));
        assert!(indices.contains(&"idx_directory_bookmarks_host_id".to_string()));
        assert!(indices.contains(&"idx_resume_records_host_id".to_string()));
        assert!(indices.contains(&"idx_resume_records_transfer_id".to_string()));
    }

    #[test]
    fn test_foreign_keys_enabled() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        migrations::run_all(&conn).unwrap();

        let fk_enabled: i32 = conn
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .unwrap();
        assert_eq!(fk_enabled, 1);
    }
}
