use rusqlite::Connection;

pub const KEY_IDLE_TIMEOUT_SECS: &str = "idle_timeout_secs";

pub fn get_string(conn: &Connection, key: &str) -> Result<Option<String>, String> {
    let mut stmt = conn
        .prepare("SELECT value FROM settings WHERE key = ?1")
        .map_err(|e| e.to_string())?;
    let mut rows = stmt
        .query_map([key], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    rows.next().transpose().map_err(|e| e.to_string())
}

pub fn set_string(conn: &Connection, key: &str, value: &str) -> Result<(), String> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2) \
         ON CONFLICT(key) DO UPDATE SET value = ?2",
        [key, value],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn get_idle_timeout_secs(conn: &Connection) -> Result<u64, String> {
    match get_string(conn, KEY_IDLE_TIMEOUT_SECS)? {
        Some(s) => s.parse::<u64>().map_err(|e| e.to_string()),
        None => Ok(crate::services::connection::DEFAULT_IDLE_TIMEOUT_SECS),
    }
}

pub fn set_idle_timeout_secs(conn: &Connection, secs: u64) -> Result<(), String> {
    set_string(conn, KEY_IDLE_TIMEOUT_SECS, &secs.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn test_get_set_idle_timeout() {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::migrations::run_all(&conn).unwrap();

        assert_eq!(get_idle_timeout_secs(&conn).unwrap(), 300);

        set_idle_timeout_secs(&conn, 600).unwrap();
        assert_eq!(get_idle_timeout_secs(&conn).unwrap(), 600);

        set_idle_timeout_secs(&conn, 0).unwrap();
        assert_eq!(get_idle_timeout_secs(&conn).unwrap(), 0);
    }
}
