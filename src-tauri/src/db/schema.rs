pub const CREATE_HOSTS_TABLE: &str = "
CREATE TABLE IF NOT EXISTS hosts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    host TEXT NOT NULL,
    port INTEGER NOT NULL DEFAULT 22,
    protocol TEXT NOT NULL CHECK(protocol IN ('ftp', 'sftp')),
    username TEXT NOT NULL,
    password TEXT,
    key_path TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
)";

pub const CREATE_TRANSFER_HISTORY_TABLE: &str = "
CREATE TABLE IF NOT EXISTS transfer_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    host_id INTEGER NOT NULL,
    filename TEXT NOT NULL,
    remote_path TEXT NOT NULL,
    local_path TEXT NOT NULL,
    direction TEXT NOT NULL CHECK(direction IN ('upload', 'download')),
    file_size INTEGER NOT NULL DEFAULT 0,
    transferred_size INTEGER NOT NULL DEFAULT 0,
    status TEXT NOT NULL CHECK(status IN ('pending', 'transferring', 'success', 'failed', 'cancelled')),
    error_message TEXT,
    started_at TEXT,
    finished_at TEXT,
    FOREIGN KEY (host_id) REFERENCES hosts(id) ON DELETE CASCADE
)";

pub const CREATE_DIRECTORY_BOOKMARKS_TABLE: &str = "
CREATE TABLE IF NOT EXISTS directory_bookmarks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    host_id INTEGER NOT NULL,
    remote_dir TEXT,
    local_dir TEXT,
    label TEXT NOT NULL,
    last_used_at TEXT,
    FOREIGN KEY (host_id) REFERENCES hosts(id) ON DELETE CASCADE
)";

pub const CREATE_RESUME_RECORDS_TABLE: &str = "
CREATE TABLE IF NOT EXISTS resume_records (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    transfer_id TEXT NOT NULL,
    host_id INTEGER NOT NULL,
    remote_path TEXT NOT NULL,
    local_path TEXT NOT NULL,
    direction TEXT NOT NULL CHECK(direction IN ('upload', 'download')),
    file_size INTEGER NOT NULL DEFAULT 0,
    transferred_bytes INTEGER NOT NULL DEFAULT 0,
    checksum TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    FOREIGN KEY (host_id) REFERENCES hosts(id) ON DELETE CASCADE
)";

pub const CREATE_SETTINGS_TABLE: &str = "
CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
)";

pub const CREATE_INDICES: &str = "
CREATE INDEX IF NOT EXISTS idx_transfer_history_host_id ON transfer_history(host_id);
CREATE INDEX IF NOT EXISTS idx_transfer_history_status ON transfer_history(status);
CREATE INDEX IF NOT EXISTS idx_transfer_history_started_at ON transfer_history(started_at DESC);
CREATE INDEX IF NOT EXISTS idx_directory_bookmarks_host_id ON directory_bookmarks(host_id);
CREATE INDEX IF NOT EXISTS idx_resume_records_host_id ON resume_records(host_id);
CREATE INDEX IF NOT EXISTS idx_resume_records_transfer_id ON resume_records(transfer_id);
";
