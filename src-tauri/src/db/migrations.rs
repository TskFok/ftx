use rusqlite::Connection;

use super::schema;

pub fn run_all(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(schema::CREATE_HOSTS_TABLE)?;
    conn.execute_batch(schema::CREATE_TRANSFER_HISTORY_TABLE)?;
    conn.execute_batch(schema::CREATE_DIRECTORY_BOOKMARKS_TABLE)?;
    conn.execute_batch(schema::CREATE_RESUME_RECORDS_TABLE)?;
    conn.execute_batch(schema::CREATE_SETTINGS_TABLE)?;
    conn.execute_batch(schema::CREATE_INDICES)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_migrations_run_successfully() {
        let conn = Connection::open_in_memory().unwrap();
        run_all(&conn).unwrap();

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
        assert!(tables.contains(&"settings".to_string()));
    }

    #[test]
    fn test_migrations_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        run_all(&conn).unwrap();
        run_all(&conn).unwrap();
    }
}
