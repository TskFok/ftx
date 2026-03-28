use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::sync::Arc;

use crate::utils::path::expand_tilde_path;
use russh::client::{self, Config, Handle};
use russh::keys::known_hosts::learn_known_hosts;
use russh::keys::{
    check_known_hosts, load_secret_key, PrivateKeyWithHashAlg, PublicKey,
};
use russh::Disconnect;
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::OpenFlags;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

use super::connection::{ConnectionTrait, FileEntry, CHUNK_SIZE};

struct RusshHandler {
    host: String,
    port: u16,
}

impl client::Handler for RusshHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKey,
    ) -> Result<bool, Self::Error> {
        match check_known_hosts(&self.host, self.port, server_public_key) {
            Ok(true) => Ok(true),
            Ok(false) => {
                learn_known_hosts(&self.host, self.port, server_public_key)?;
                Ok(true)
            }
            Err(russh::keys::Error::KeyChanged { .. }) => Err(russh::Error::InvalidConfig(
                "主机密钥不匹配，可能存在中间人攻击。请检查 ~/.ssh/known_hosts".into(),
            )),
            Err(e) => Err(russh::Error::Keys(e)),
        }
    }
}

/// 拼接远程路径。父目录为 `/` 时不能用 `trim_end_matches('/')` 后直接判空，否则会丢掉根前缀，
/// 变成相对路径（如只得到 `home` 而非 `/home`），导致 SFTP 列目录失败。
fn join_remote_path(parent: &str, name: &str) -> String {
    let name = name.trim_start_matches('/');
    let parent = parent.trim_end_matches('/');
    if parent.is_empty() {
        format!("/{}", name)
    } else {
        format!("{}/{}", parent, name)
    }
}

fn fmt_mtime(mtime: Option<u32>) -> Option<String> {
    mtime.and_then(|t| {
        chrono::DateTime::<chrono::Utc>::from_timestamp(i64::from(t), 0)
            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
    })
}

/// SFTP 客户端（基于 russh，纯 Rust 协议栈，避免 libssh2/WINCNG 的密钥交换问题）。
pub struct SftpClient {
    host: String,
    port: u16,
    username: String,
    password: Option<String>,
    key_path: Option<String>,
    runtime: tokio::runtime::Runtime,
    handle: Option<Handle<RusshHandler>>,
    sftp: Option<SftpSession>,
}

impl SftpClient {
    pub fn new(
        host: String,
        port: u16,
        username: String,
        password: Option<String>,
        key_path: Option<String>,
    ) -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("tokio runtime for SFTP");
        Self {
            host,
            port,
            username,
            password,
            key_path,
            runtime,
            handle: None,
            sftp: None,
        }
    }

    fn require_sftp(&self) -> Result<&SftpSession, String> {
        self.sftp
            .as_ref()
            .ok_or_else(|| "Not connected".to_string())
    }

}

impl ConnectionTrait for SftpClient {
    fn connect(&mut self) -> Result<(), String> {
        let host = self.host.clone();
        let port = self.port;
        let username = self.username.clone();
        let password = self.password.clone();
        let key_path = self.key_path.clone();

        let config = Arc::new(Config::default());
        let handler = RusshHandler {
            host: host.clone(),
            port,
        };

        let mut handle = self
            .runtime
            .block_on(client::connect(
                config,
                (host.as_str(), port),
                handler,
            ))
            .map_err(|e| e.to_string())?;

        if let Some(ref kp) = key_path {
            let path = expand_tilde_path(kp).map_err(|e| e.to_string())?;
            let key = load_secret_key(&path, password.as_deref()).map_err(|e| e.to_string())?;
            let key = Arc::new(key);
            let rsa_hash = self
                .runtime
                .block_on(handle.best_supported_rsa_hash())
                .map_err(|e| e.to_string())?;
            let auth = self
                .runtime
                .block_on(handle.authenticate_publickey(
                    username.clone(),
                    PrivateKeyWithHashAlg::new(key, rsa_hash.flatten()),
                ))
                .map_err(|e| e.to_string())?;
            if !auth.success() {
                return Err("Authentication failed".to_string());
            }
        } else if let Some(ref pw) = password {
            let auth = self
                .runtime
                .block_on(handle.authenticate_password(username.clone(), pw.clone()))
                .map_err(|e| e.to_string())?;
            if !auth.success() {
                return Err("Authentication failed".to_string());
            }
        } else {
            return Err("No authentication method provided".to_string());
        }

        let channel = self
            .runtime
            .block_on(handle.channel_open_session())
            .map_err(|e| e.to_string())?;
        self.runtime
            .block_on(channel.request_subsystem(true, "sftp"))
            .map_err(|e| e.to_string())?;

        let sftp = self
            .runtime
            .block_on(SftpSession::new(channel.into_stream()))
            .map_err(|e| e.to_string())?;

        self.handle = Some(handle);
        self.sftp = Some(sftp);
        Ok(())
    }

    fn disconnect(&mut self) -> Result<(), String> {
        if let Some(ref sftp) = self.sftp {
            let _ = self.runtime.block_on(sftp.close());
        }
        self.sftp = None;
        if let Some(h) = self.handle.take() {
            let _ = self.runtime.block_on(h.disconnect(
                Disconnect::ByApplication,
                "bye",
                "English",
            ));
        }
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.sftp.is_some()
            && self
                .handle
                .as_ref()
                .map(|h| !h.is_closed())
                .unwrap_or(false)
    }

    fn list_dir(&mut self, path: &str) -> Result<Vec<FileEntry>, String> {
        let sftp = self.require_sftp()?;
        let mut dir = self
            .runtime
            .block_on(sftp.read_dir(path))
            .map_err(|e| e.to_string())?;

        let mut files = Vec::new();
        while let Some(entry) = dir.next() {
            let name = entry.file_name();
            if name == "." || name == ".." {
                continue;
            }
            let meta = entry.metadata();
            let full_path = join_remote_path(path, &name);
            files.push(FileEntry {
                name,
                path: full_path,
                is_dir: meta.is_dir(),
                size: meta.size.unwrap_or(0),
                modified: fmt_mtime(meta.mtime),
            });
        }
        Ok(files)
    }

    fn file_size(&mut self, path: &str) -> Result<u64, String> {
        let sftp = self.require_sftp()?;
        let meta = self
            .runtime
            .block_on(sftp.metadata(path))
            .map_err(|e| e.to_string())?;
        meta.size
            .ok_or_else(|| "Unable to determine file size".to_string())
    }

    fn file_exists(&mut self, path: &str) -> Result<bool, String> {
        let sftp = self.require_sftp()?;
        self.runtime
            .block_on(sftp.try_exists(path))
            .map_err(|e| e.to_string())
    }

    fn upload(
        &mut self,
        local_path: &str,
        remote_path: &str,
        offset: u64,
        progress: Option<&dyn Fn(u64, u64)>,
    ) -> Result<u64, String> {
        let sftp = self.require_sftp()?;
        let metadata = fs::metadata(local_path).map_err(|e| e.to_string())?;
        let total_size = metadata.len();

        let mut local_file = fs::File::open(local_path).map_err(|e| e.to_string())?;

        let mut remote_file = if offset > 0 {
            local_file
                .seek(SeekFrom::Start(offset))
                .map_err(|e| e.to_string())?;
            let mut f = self
                .runtime
                .block_on(sftp.open_with_flags(
                    remote_path,
                    OpenFlags::READ | OpenFlags::WRITE,
                ))
                .map_err(|e| e.to_string())?;
            self.runtime
                .block_on(f.seek(SeekFrom::Start(offset)))
                .map_err(|e| e.to_string())?;
            f
        } else {
            self.runtime
                .block_on(sftp.open_with_flags(
                    remote_path,
                    OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE | OpenFlags::READ,
                ))
                .map_err(|e| e.to_string())?
        };

        let mut buf = [0u8; CHUNK_SIZE];
        let mut transferred = offset;

        loop {
            let n = local_file.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            self.runtime
                .block_on(remote_file.write_all(&buf[..n]))
                .map_err(|e| e.to_string())?;
            transferred += n as u64;
            if let Some(cb) = progress {
                cb(transferred, total_size);
            }
        }

        let _ = self.runtime.block_on(remote_file.shutdown());
        Ok(transferred - offset)
    }

    fn download(
        &mut self,
        remote_path: &str,
        local_path: &str,
        offset: u64,
        progress: Option<&dyn Fn(u64, u64)>,
    ) -> Result<u64, String> {
        let sftp = self.require_sftp()?;
        let meta = self
            .runtime
            .block_on(sftp.metadata(remote_path))
            .map_err(|e| e.to_string())?;
        let total_size = meta.size.unwrap_or(0);

        let mut remote_file = self
            .runtime
            .block_on(sftp.open(remote_path))
            .map_err(|e| e.to_string())?;

        let mut local_file = if offset > 0 {
            self.runtime
                .block_on(remote_file.seek(SeekFrom::Start(offset)))
                .map_err(|e| e.to_string())?;
            let mut f = fs::OpenOptions::new()
                .write(true)
                .open(local_path)
                .map_err(|e| e.to_string())?;
            f.seek(SeekFrom::Start(offset)).map_err(|e| e.to_string())?;
            f
        } else {
            fs::File::create(local_path).map_err(|e| e.to_string())?
        };

        let mut buf = [0u8; CHUNK_SIZE];
        let mut transferred: u64 = 0;

        loop {
            let n = self
                .runtime
                .block_on(remote_file.read(&mut buf))
                .map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            local_file
                .write_all(&buf[..n])
                .map_err(|e| e.to_string())?;
            transferred += n as u64;
            if let Some(cb) = progress {
                cb(offset + transferred, total_size);
            }
        }

        Ok(transferred)
    }

    fn mkdir(&mut self, path: &str) -> Result<(), String> {
        let sftp = self.require_sftp()?;
        self.runtime
            .block_on(sftp.create_dir(path))
            .map_err(|e| e.to_string())
    }

    fn remove_file(&mut self, path: &str) -> Result<(), String> {
        let sftp = self.require_sftp()?;
        self.runtime
            .block_on(sftp.remove_file(path))
            .map_err(|e| e.to_string())
    }

    fn remove_dir(&mut self, path: &str) -> Result<(), String> {
        let sftp = self.require_sftp()?;
        self.runtime
            .block_on(sftp.remove_dir(path))
            .map_err(|e| e.to_string())
    }

    fn rename(&mut self, from: &str, to: &str) -> Result<(), String> {
        let sftp = self.require_sftp()?;
        self.runtime
            .block_on(sftp.rename(from, to))
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_join_remote_path() {
        assert_eq!(join_remote_path("/home/u", "a.txt"), "/home/u/a.txt");
        assert_eq!(join_remote_path("/home/u/", "b"), "/home/u/b");
        assert_eq!(join_remote_path("/", "etc"), "/etc");
        assert_eq!(join_remote_path("//", "tmp"), "/tmp");
        assert_eq!(join_remote_path("", "x"), "/x");
    }

    #[test]
    fn test_sftp_client_new() {
        let client = SftpClient::new(
            "127.0.0.1".into(),
            22,
            "user".into(),
            Some("pass".into()),
            None,
        );
        assert!(!client.is_connected());
        assert_eq!(client.host, "127.0.0.1");
        assert_eq!(client.port, 22);
    }

    #[test]
    fn test_sftp_client_new_with_key() {
        let client = SftpClient::new(
            "example.com".into(),
            22,
            "admin".into(),
            None,
            Some("/home/admin/.ssh/id_rsa".into()),
        );
        assert!(!client.is_connected());
        assert!(client.key_path.is_some());
        assert!(client.password.is_none());
    }

    #[test]
    fn test_sftp_not_connected_errors() {
        let client = SftpClient::new(
            "127.0.0.1".into(),
            22,
            "user".into(),
            Some("pass".into()),
            None,
        );
        assert!(client.require_sftp().is_err());
    }
}
