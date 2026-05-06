use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::models::host::{Host, Protocol};

use super::ftp_client::FtpClient;
use super::sftp_client::SftpClient;

pub const CHUNK_SIZE: usize = 32768;

/// 默认空闲超时时间（秒），超过此时间未使用的连接将被断开
pub const DEFAULT_IDLE_TIMEOUT_SECS: u64 = 300;

/// 包装连接，在释放时更新最后活动时间
pub struct ConnectionGuard {
    client: Arc<Mutex<Box<dyn ConnectionTrait>>>,
    last_activity: Arc<Mutex<Instant>>,
}

impl ConnectionGuard {
    fn new(client: Arc<Mutex<Box<dyn ConnectionTrait>>>, last_activity: Arc<Mutex<Instant>>) -> Self {
        *last_activity.lock().unwrap() = Instant::now();
        Self { client, last_activity }
    }
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        if let Ok(mut last) = self.last_activity.lock() {
            *last = Instant::now();
        }
    }
}

impl std::ops::Deref for ConnectionGuard {
    type Target = Arc<Mutex<Box<dyn ConnectionTrait>>>;
    fn deref(&self) -> &Self::Target {
        &self.client
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<String>,
}

pub trait ConnectionTrait: Send {
    fn connect(&mut self) -> Result<(), String>;
    fn disconnect(&mut self) -> Result<(), String>;
    fn is_connected(&self) -> bool;

    fn list_dir(&mut self, path: &str) -> Result<Vec<FileEntry>, String>;
    fn file_size(&mut self, path: &str) -> Result<u64, String>;
    fn file_exists(&mut self, path: &str) -> Result<bool, String>;

    /// Upload a file with optional resume offset and progress reporting.
    /// `progress` 的第一个参数须为文件中已传输的**绝对字节位置**（与本次调用的 `offset`/续传起点一致）。
    /// Returns the number of bytes transferred in this call.
    fn upload(
        &mut self,
        local_path: &str,
        remote_path: &str,
        offset: u64,
        progress: Option<&dyn Fn(u64, u64)>,
    ) -> Result<u64, String>;

    /// Download a file with optional resume offset and progress reporting.
    /// `progress` 的第一个参数须为文件中已传输的**绝对字节位置**。
    /// Returns the number of bytes transferred in this call.
    fn download(
        &mut self,
        remote_path: &str,
        local_path: &str,
        offset: u64,
        progress: Option<&dyn Fn(u64, u64)>,
    ) -> Result<u64, String>;

    fn mkdir(&mut self, path: &str) -> Result<(), String>;
    fn remove_file(&mut self, path: &str) -> Result<(), String>;
    fn remove_dir(&mut self, path: &str) -> Result<(), String>;
    fn rename(&mut self, from: &str, to: &str) -> Result<(), String>;
}

fn create_client(host: &Host) -> Box<dyn ConnectionTrait> {
    match host.protocol {
        Protocol::Ftp => Box::new(FtpClient::new(
            host.host.clone(),
            host.port,
            host.username.clone(),
            host.password.clone().unwrap_or_default(),
        )),
        Protocol::Sftp => Box::new(SftpClient::new(
            host.host.clone(),
            host.port,
            host.username.clone(),
            host.password.clone(),
            host.key_path.clone(),
        )),
    }
}

struct ConnectionEntry {
    client: Arc<Mutex<Box<dyn ConnectionTrait>>>,
    last_activity: Arc<Mutex<Instant>>,
}

/// Thread-safe connection pool that manages active FTP/SFTP connections keyed by host ID.
/// Each connection is independently locked so operations on different hosts don't block each other.
/// Connections are automatically disconnected when idle for longer than idle_timeout_secs.
#[derive(Clone)]
pub struct ConnectionManager {
    connections: Arc<Mutex<HashMap<i64, ConnectionEntry>>>,
    idle_timeout_secs: Arc<AtomicU64>,
}

impl ConnectionManager {
    pub fn new() -> Self {
        Self::with_idle_timeout(DEFAULT_IDLE_TIMEOUT_SECS)
    }

    pub fn with_idle_timeout(idle_timeout_secs: u64) -> Self {
        Self {
            connections: Arc::new(Mutex::new(HashMap::new())),
            idle_timeout_secs: Arc::new(AtomicU64::new(idle_timeout_secs)),
        }
    }

    pub fn set_idle_timeout(&self, secs: u64) {
        self.idle_timeout_secs.store(secs, Ordering::Relaxed);
    }

    pub fn idle_timeout_secs(&self) -> u64 {
        self.idle_timeout_secs.load(Ordering::Relaxed)
    }

    pub fn connect(&self, host: &Host) -> Result<(), String> {
        let host_id = host.id.ok_or("Host has no ID")?;

        {
            let conns = self.connections.lock().map_err(|e| e.to_string())?;
            if conns.contains_key(&host_id) {
                return Ok(());
            }
        }

        let mut client = create_client(host);
        client.connect()?;

        let mut conns = self.connections.lock().map_err(|e| e.to_string())?;
        conns.insert(
            host_id,
            ConnectionEntry {
                client: Arc::new(Mutex::new(client)),
                last_activity: Arc::new(Mutex::new(Instant::now())),
            },
        );
        Ok(())
    }

    pub fn disconnect(&self, host_id: i64) -> Result<(), String> {
        let entry = {
            let mut conns = self.connections.lock().map_err(|e| e.to_string())?;
            conns.remove(&host_id)
        };
        if let Some(entry) = entry {
            let mut client = entry.client.lock().map_err(|e| e.to_string())?;
            client.disconnect()?;
        }
        Ok(())
    }

    pub fn get_connection(&self, host_id: i64) -> Result<ConnectionGuard, String> {
        let timeout_secs = self.idle_timeout_secs.load(Ordering::Relaxed)
            as u128;

        let mut conns = self.connections.lock().map_err(|e| e.to_string())?;
        let entry = conns
            .get_mut(&host_id)
            .ok_or_else(|| format!("No active connection for host {}", host_id))?;

        let idle_secs = entry.last_activity.lock().unwrap().elapsed().as_secs() as u128;
        if timeout_secs > 0 && idle_secs >= timeout_secs {
            let entry = conns.remove(&host_id).unwrap();
            drop(conns);
            let mut client = entry.client.lock().map_err(|e| e.to_string())?;
            let _ = client.disconnect();
            return Err(format!(
                "Connection closed due to idle timeout ({} seconds)",
                timeout_secs
            ));
        }

        Ok(ConnectionGuard::new(
            entry.client.clone(),
            entry.last_activity.clone(),
        ))
    }

    pub fn is_connected(&self, host_id: i64) -> bool {
        self.connections
            .lock()
            .ok()
            .map(|c| c.contains_key(&host_id))
            .unwrap_or(false)
    }

    pub fn test_connection(host: &Host) -> Result<(), String> {
        let mut client = create_client(host);
        client.connect()?;
        client.disconnect()?;
        Ok(())
    }

    pub fn disconnect_all(&self) -> Result<(), String> {
        let entries: Vec<_> = {
            let mut conns = self.connections.lock().map_err(|e| e.to_string())?;
            conns.drain().collect()
        };
        for (_, entry) in entries {
            if let Ok(mut client) = entry.client.lock() {
                let _ = client.disconnect();
            }
        }
        Ok(())
    }

    pub fn active_connections(&self) -> Result<Vec<i64>, String> {
        let conns = self.connections.lock().map_err(|e| e.to_string())?;
        Ok(conns.keys().cloned().collect())
    }

    #[cfg(test)]
    pub fn insert_mock_connection(
        &self,
        host_id: i64,
        client: Box<dyn ConnectionTrait>,
    ) -> Result<(), String> {
        let mut conns = self.connections.lock().map_err(|e| e.to_string())?;
        conns.insert(
            host_id,
            ConnectionEntry {
                client: Arc::new(Mutex::new(client)),
                last_activity: Arc::new(Mutex::new(Instant::now())),
            },
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockClient {
        connected: bool,
        fail_connect: bool,
    }

    impl MockClient {
        fn new(fail_connect: bool) -> Self {
            Self {
                connected: false,
                fail_connect,
            }
        }
    }

    impl ConnectionTrait for MockClient {
        fn connect(&mut self) -> Result<(), String> {
            if self.fail_connect {
                return Err("Connection refused".to_string());
            }
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

        fn list_dir(&mut self, _path: &str) -> Result<Vec<FileEntry>, String> {
            if !self.connected {
                return Err("Not connected".to_string());
            }
            Ok(vec![FileEntry {
                name: "test.txt".to_string(),
                path: "/test.txt".to_string(),
                is_dir: false,
                size: 100,
                modified: None,
            }])
        }

        fn file_size(&mut self, _path: &str) -> Result<u64, String> {
            Ok(100)
        }

        fn file_exists(&mut self, _path: &str) -> Result<bool, String> {
            Ok(true)
        }

        fn upload(
            &mut self,
            _local_path: &str,
            _remote_path: &str,
            _offset: u64,
            _progress: Option<&dyn Fn(u64, u64)>,
        ) -> Result<u64, String> {
            Ok(100)
        }

        fn download(
            &mut self,
            _remote_path: &str,
            _local_path: &str,
            _offset: u64,
            _progress: Option<&dyn Fn(u64, u64)>,
        ) -> Result<u64, String> {
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

    #[test]
    fn test_connection_manager_new() {
        let manager = ConnectionManager::new();
        assert!(manager.active_connections().unwrap().is_empty());
    }

    #[test]
    fn test_insert_and_get_connection() {
        let manager = ConnectionManager::new();
        let client = Box::new(MockClient::new(false));
        manager.insert_mock_connection(1, client).unwrap();

        assert!(manager.is_connected(1));
        assert!(!manager.is_connected(2));

        let conn = manager.get_connection(1);
        assert!(conn.is_ok());

        let conn = manager.get_connection(999);
        assert!(conn.is_err());
    }

    #[test]
    fn test_disconnect() {
        let manager = ConnectionManager::new();
        manager
            .insert_mock_connection(1, Box::new(MockClient::new(false)))
            .unwrap();

        assert!(manager.is_connected(1));
        manager.disconnect(1).unwrap();
        assert!(!manager.is_connected(1));
    }

    #[test]
    fn test_disconnect_nonexistent_is_ok() {
        let manager = ConnectionManager::new();
        assert!(manager.disconnect(999).is_ok());
    }

    #[test]
    fn test_connect_idempotent_when_already_connected() {
        let manager = ConnectionManager::new();
        manager
            .insert_mock_connection(1, Box::new(MockClient::new(false)))
            .unwrap();

        let host = Host {
            id: Some(1),
            name: "test".into(),
            host: "127.0.0.1".into(),
            port: 21,
            protocol: Protocol::Ftp,
            username: "user".into(),
            password: Some("pass".into()),
            key_path: None,
            created_at: None,
            updated_at: None,
        };

        assert!(manager.connect(&host).is_ok());
        assert!(manager.is_connected(1));
    }

    #[test]
    fn test_disconnect_all() {
        let manager = ConnectionManager::new();
        manager
            .insert_mock_connection(1, Box::new(MockClient::new(false)))
            .unwrap();
        manager
            .insert_mock_connection(2, Box::new(MockClient::new(false)))
            .unwrap();

        assert_eq!(manager.active_connections().unwrap().len(), 2);
        manager.disconnect_all().unwrap();
        assert!(manager.active_connections().unwrap().is_empty());
    }

    #[test]
    fn test_active_connections() {
        let manager = ConnectionManager::new();
        manager
            .insert_mock_connection(10, Box::new(MockClient::new(false)))
            .unwrap();
        manager
            .insert_mock_connection(20, Box::new(MockClient::new(false)))
            .unwrap();

        let mut ids = manager.active_connections().unwrap();
        ids.sort();
        assert_eq!(ids, vec![10, 20]);
    }

    #[test]
    fn test_with_connection_operations() {
        let manager = ConnectionManager::new();
        let mut client = MockClient::new(false);
        client.connected = true;
        manager
            .insert_mock_connection(1, Box::new(client))
            .unwrap();

        let conn = manager.get_connection(1).unwrap();
        let mut conn = conn.lock().unwrap();
        let entries = conn.list_dir("/").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "test.txt");
    }

    #[test]
    fn test_concurrent_connections_independent() {
        let manager = ConnectionManager::new();
        let mut c1 = MockClient::new(false);
        c1.connected = true;
        let mut c2 = MockClient::new(false);
        c2.connected = true;

        manager.insert_mock_connection(1, Box::new(c1)).unwrap();
        manager.insert_mock_connection(2, Box::new(c2)).unwrap();

        let conn1 = manager.get_connection(1).unwrap();
        let conn2 = manager.get_connection(2).unwrap();

        let mut c1 = conn1.lock().unwrap();
        let mut c2 = conn2.lock().unwrap();

        let files1 = c1.list_dir("/a").unwrap();
        let files2 = c2.list_dir("/b").unwrap();

        assert_eq!(files1.len(), 1);
        assert_eq!(files2.len(), 1);
    }

    #[test]
    fn test_clone_shares_state() {
        let manager = ConnectionManager::new();
        let clone = manager.clone();

        manager
            .insert_mock_connection(1, Box::new(MockClient::new(false)))
            .unwrap();
        assert!(clone.is_connected(1));
    }

    #[test]
    fn test_create_client_ftp() {
        let host = Host {
            id: Some(1),
            name: "test".into(),
            host: "127.0.0.1".into(),
            port: 21,
            protocol: Protocol::Ftp,
            username: "user".into(),
            password: Some("pass".into()),
            key_path: None,
            created_at: None,
            updated_at: None,
        };
        let client = create_client(&host);
        assert!(!client.is_connected());
    }

    #[test]
    fn test_idle_timeout_disconnects() {
        use std::thread;
        use std::time::Duration;

        let manager = ConnectionManager::with_idle_timeout(1);
        manager
            .insert_mock_connection(1, Box::new(MockClient::new(false)))
            .unwrap();

        let conn = manager.get_connection(1).unwrap();
        drop(conn);
        thread::sleep(Duration::from_secs(2));

        let result = manager.get_connection(1);
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(err.contains("idle timeout"));
    }

    #[test]
    fn test_idle_timeout_zero_disables() {
        use std::thread;
        use std::time::Duration;

        let manager = ConnectionManager::with_idle_timeout(0);
        manager
            .insert_mock_connection(1, Box::new(MockClient::new(false)))
            .unwrap();

        let conn = manager.get_connection(1).unwrap();
        drop(conn);
        thread::sleep(Duration::from_millis(100));

        let conn = manager.get_connection(1);
        assert!(conn.is_ok());
    }

    #[test]
    fn test_create_client_sftp() {
        let host = Host {
            id: Some(2),
            name: "test".into(),
            host: "127.0.0.1".into(),
            port: 22,
            protocol: Protocol::Sftp,
            username: "user".into(),
            password: Some("pass".into()),
            key_path: None,
            created_at: None,
            updated_at: None,
        };
        let client = create_client(&host);
        assert!(!client.is_connected());
    }
}
