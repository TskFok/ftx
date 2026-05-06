use std::fs;
use std::future::Future;
use std::io::{Read, Seek, SeekFrom, Write};
use std::pin::Pin;
use std::sync::Arc;

use crate::utils::path::{expand_tilde_path, join_remote_path, remote_list_entry_display_name};
use russh::client::{self, Config, Handle};
use russh::keys::known_hosts::learn_known_hosts;
use russh::keys::{
    check_known_hosts, load_secret_key, PrivateKeyWithHashAlg, PublicKey,
};
use russh::Disconnect;
use russh_sftp::client::error::Error as SftpError;
use russh_sftp::client::rawsession::{Limits, SftpResult};
use russh_sftp::client::RawSftpSession;
use russh_sftp::extensions;
use russh_sftp::protocol::{Data, FileAttributes, OpenFlags, Status, StatusCode};

use super::common::fmt_mtime;
use crate::services::connection::{ConnectionTrait, FileEntry};

/// SFTP 单包读/写上限（与 russh_sftp 内置默认值对齐；可通过 limits@openssh.com 下调）。
const SFTP_DEFAULT_PAYLOAD_MAX: u32 = 261_120;
/// 单包绝对上限，避免异常偏大 allocation。
const SFTP_ABS_PAYLOAD_MAX: u32 = 1024 * 1024;

/// SSH 会话：禁用 Nagle、增大 channel 窗口与包长，减轻高延迟链路上的阻塞。
fn ssh_client_config() -> Config {
    let mut c = Config::default();
    c.nodelay = true;
    c.window_size = 16 * 1024 * 1024;
    c.maximum_packet_size = 256 * 1024;
    c.channel_buffer_size = c.channel_buffer_size.max(256);
    c
}

fn effective_sftp_payload_max(limits_read_len: Option<u64>, limits_write_len: Option<u64>) -> u32 {
    let cap = limits_read_len
        .into_iter()
        .chain(limits_write_len)
        .filter(|&x| x > 0)
        .map(|x| (x as u32).min(SFTP_ABS_PAYLOAD_MAX))
        .min()
        .unwrap_or(SFTP_DEFAULT_PAYLOAD_MAX);
    cap.max(8192)
}

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

/// SFTP 客户端（基于 russh，纯 Rust 协议栈；用于 Windows / Linux 等非 macOS 平台）。
pub struct SftpClient {
    host: String,
    port: u16,
    username: String,
    password: Option<String>,
    key_path: Option<String>,
    runtime: tokio::runtime::Runtime,
    handle: Option<Handle<RusshHandler>>,
    raw: Option<Arc<RawSftpSession>>,
    max_payload: u32,
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
            raw: None,
            max_payload: SFTP_DEFAULT_PAYLOAD_MAX,
        }
    }

    fn require_session(&self) -> Result<&Arc<RawSftpSession>, String> {
        self.raw
            .as_ref()
            .ok_or_else(|| "Not connected".to_string())
    }
}

async fn pipeline_download(
    session: Arc<RawSftpSession>,
    handle: String,
    mut local_file: fs::File,
    start_offset: u64,
    total_size: u64,
    max_read: u32,
    progress: Option<&dyn Fn(u64, u64)>,
    is_cancelled: Option<&dyn Fn() -> bool>,
) -> Result<u64, String> {
    let mut off = start_offset;
    let mut transferred: u64 = 0;
    let mut pending: Option<Pin<Box<dyn Future<Output = SftpResult<Data>> + Send>>> =
        Some(Box::pin(session.read(handle.clone(), off, max_read)));

    while let Some(fut) = pending.take() {
        if is_cancelled.is_some_and(|f| f()) {
            return Err("Transfer cancelled".to_string());
        }
        let packet = fut.await.map_err(|e| e.to_string())?;
        let data = packet.data;
        let n = data.len() as u32;
        if n == 0 {
            break;
        }
        off += u64::from(n);
        transferred += u64::from(n);
        let more = n == max_read && off < total_size;

        pending = if more {
            Some(Box::pin(session.read(handle.clone(), off, max_read)))
        } else {
            None
        };

        local_file
            .write_all(&data)
            .map_err(|e| e.to_string())?;

        if let Some(cb) = progress {
            cb(off, total_size);
        }

        if !more {
            break;
        }
    }

    Ok(transferred)
}

async fn pipeline_upload(
    session: Arc<RawSftpSession>,
    handle: String,
    mut local_file: fs::File,
    start_offset: u64,
    total_size: u64,
    max_write: u32,
    progress: Option<&dyn Fn(u64, u64)>,
    is_cancelled: Option<&dyn Fn() -> bool>,
) -> Result<u64, String> {
    if is_cancelled.is_some_and(|f| f()) {
        return Err("Transfer cancelled".to_string());
    }
    let mut write_off = start_offset;
    let mut buf = vec![0u8; max_write as usize];
    let n = local_file
        .read(&mut buf)
        .map_err(|e| e.to_string())?;
    if n == 0 {
        return Ok(0);
    }
    let mut pending: Option<Pin<Box<dyn Future<Output = SftpResult<Status>> + Send>>> =
        Some(Box::pin(session.write(
            handle.clone(),
            write_off,
            buf[..n].to_vec(),
        )));
    write_off += n as u64;

    if let Some(cb) = progress {
        cb(write_off, total_size);
    }

    while let Some(fut) = pending.take() {
        if is_cancelled.is_some_and(|f| f()) {
            return Err("Transfer cancelled".to_string());
        }
        let mut next_buf = vec![0u8; max_write as usize];
        let n2 = local_file
            .read(&mut next_buf)
            .map_err(|e| e.to_string())?;

        let next_fut: Option<Pin<Box<dyn Future<Output = SftpResult<Status>> + Send>>> = if n2 > 0 {
            Some(Box::pin(session.write(
                handle.clone(),
                write_off,
                next_buf[..n2].to_vec(),
            )))
        } else {
            None
        };

        fut.await.map_err(|e| e.to_string())?;

        if is_cancelled.is_some_and(|f| f()) {
            return Err("Transfer cancelled".to_string());
        }

        if let Some(f) = next_fut {
            pending = Some(f);
            write_off += n2 as u64;
            if let Some(cb) = progress {
                cb(write_off, total_size);
            }
        } else {
            break;
        }
    }

    Ok(write_off - start_offset)
}

impl ConnectionTrait for SftpClient {
    fn connect(&mut self) -> Result<(), String> {
        let host = self.host.clone();
        let port = self.port;
        let username = self.username.clone();
        let password = self.password.clone();
        let key_path = self.key_path.clone();

        let config = Arc::new(ssh_client_config());
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

        let stream = channel.into_stream();
        let mut raw_session = RawSftpSession::new(stream);
        let version = self
            .runtime
            .block_on(raw_session.init())
            .map_err(|e| e.to_string())?;

        let mut limits_read = None::<u64>;
        let mut limits_write = None::<u64>;
        if version
            .extensions
            .get(extensions::LIMITS)
            .is_some_and(|e| e == "1")
        {
            let lim = self
                .runtime
                .block_on(raw_session.limits())
                .map_err(|e| e.to_string())?;
            let arc_lim = Arc::new(Limits::from(lim));
            limits_read = arc_lim.read_len;
            limits_write = arc_lim.write_len;
            raw_session.set_limits(arc_lim);
        }

        self.max_payload = effective_sftp_payload_max(limits_read, limits_write);
        self.raw = Some(Arc::new(raw_session));
        self.handle = Some(handle);
        Ok(())
    }

    fn disconnect(&mut self) -> Result<(), String> {
        if let Some(raw) = self.raw.take() {
            let _ = raw.close_session();
        }
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
        self.raw.is_some()
            && self
                .handle
                .as_ref()
                .map(|h| !h.is_closed())
                .unwrap_or(false)
    }

    fn list_dir(&mut self, path: &str) -> Result<Vec<FileEntry>, String> {
        let session = self.require_session()?;
        let h = self
            .runtime
            .block_on(session.opendir(path))
            .map_err(|e| e.to_string())?
            .handle;

        let mut files = Vec::new();
        loop {
            match self.runtime.block_on(session.readdir(&h)) {
                Ok(name) => {
                    for f in name.files {
                        let fname = f.filename;
                        if fname == "." || fname == ".." {
                            continue;
                        }
                        let display_name = remote_list_entry_display_name(&fname);
                        if display_name == "." || display_name == ".." || display_name.is_empty() {
                            continue;
                        }
                        let full_path = join_remote_path(path, &display_name);
                        let is_dir = f.attrs.is_dir();
                        files.push(FileEntry {
                            name: display_name,
                            path: full_path,
                            is_dir,
                            size: f.attrs.size.unwrap_or(0),
                            modified: fmt_mtime(f.attrs.mtime),
                        });
                    }
                }
                Err(SftpError::Status(s)) if s.status_code == StatusCode::Eof => break,
                Err(e) => return Err(e.to_string()),
            }
        }

        self.runtime
            .block_on(session.close(h))
            .map_err(|e| e.to_string())?;
        Ok(files)
    }

    fn file_size(&mut self, path: &str) -> Result<u64, String> {
        let session = self.require_session()?;
        let meta = self
            .runtime
            .block_on(session.stat(path))
            .map_err(|e| e.to_string())?;
        meta.attrs
            .size
            .ok_or_else(|| "Unable to determine file size".to_string())
    }

    fn file_exists(&mut self, path: &str) -> Result<bool, String> {
        let session = self.require_session()?;
        match self.runtime.block_on(session.stat(path)) {
            Ok(meta) => Ok(!meta.attrs.is_dir()),
            Err(SftpError::Status(s)) if s.status_code == StatusCode::NoSuchFile => Ok(false),
            Err(e) => Err(e.to_string()),
        }
    }

    fn upload(
        &mut self,
        local_path: &str,
        remote_path: &str,
        offset: u64,
        progress: Option<&dyn Fn(u64, u64)>,
        is_cancelled: Option<&dyn Fn() -> bool>,
    ) -> Result<u64, String> {
        let session = self.require_session()?.clone();
        let metadata = fs::metadata(local_path).map_err(|e| e.to_string())?;
        let total_size = metadata.len();

        let mut local_file = fs::File::open(local_path).map_err(|e| e.to_string())?;

        let (handle, max_w) = if offset > 0 {
            local_file
                .seek(SeekFrom::Start(offset))
                .map_err(|e| e.to_string())?;
            let h = self
                .runtime
                .block_on(session.open(
                    remote_path,
                    OpenFlags::READ | OpenFlags::WRITE,
                    FileAttributes::empty(),
                ))
                .map_err(|e| e.to_string())?
                .handle;
            (h, self.max_payload)
        } else {
            let h = self
                .runtime
                .block_on(session.open(
                    remote_path,
                    OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE | OpenFlags::READ,
                    FileAttributes::empty(),
                ))
                .map_err(|e| e.to_string())?
                .handle;
            (h, self.max_payload)
        };

        let transferred = self.runtime.block_on(pipeline_upload(
            session.clone(),
            handle.clone(),
            local_file,
            offset,
            total_size,
            max_w,
            progress,
            is_cancelled,
        ))?;

        self.runtime
            .block_on(session.close(handle))
            .map_err(|e| e.to_string())?;

        Ok(transferred)
    }

    fn download(
        &mut self,
        remote_path: &str,
        local_path: &str,
        offset: u64,
        progress: Option<&dyn Fn(u64, u64)>,
        is_cancelled: Option<&dyn Fn() -> bool>,
    ) -> Result<u64, String> {
        let session = self.require_session()?.clone();
        let meta = self
            .runtime
            .block_on(session.stat(remote_path))
            .map_err(|e| e.to_string())?;
        let total_size = meta.attrs.size.unwrap_or(0);

        let handle = self
            .runtime
            .block_on(session.open(
                remote_path,
                OpenFlags::READ,
                FileAttributes::empty(),
            ))
            .map_err(|e| e.to_string())?
            .handle;

        let local_file = if offset > 0 {
            let mut f = fs::OpenOptions::new()
                .write(true)
                .open(local_path)
                .map_err(|e| e.to_string())?;
            f.seek(SeekFrom::Start(offset))
                .map_err(|e| e.to_string())?;
            f
        } else {
            fs::File::create(local_path).map_err(|e| e.to_string())?
        };

        let transferred = self.runtime.block_on(pipeline_download(
            session.clone(),
            handle.clone(),
            local_file,
            offset,
            total_size,
            self.max_payload,
            progress,
            is_cancelled,
        ))?;

        self.runtime
            .block_on(session.close(handle))
            .map_err(|e| e.to_string())?;

        Ok(transferred)
    }

    fn mkdir(&mut self, path: &str) -> Result<(), String> {
        let session = self.require_session()?;
        self.runtime
            .block_on(session.mkdir(path, FileAttributes::empty()))
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn remove_file(&mut self, path: &str) -> Result<(), String> {
        let session = self.require_session()?;
        self.runtime
            .block_on(session.remove(path))
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn remove_dir(&mut self, path: &str) -> Result<(), String> {
        let session = self.require_session()?;
        self.runtime
            .block_on(session.rmdir(path))
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn rename(&mut self, from: &str, to: &str) -> Result<(), String> {
        let session = self.require_session()?;
        self.runtime
            .block_on(session.rename(from, to))
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(client.require_session().is_err());
    }

    #[test]
    fn test_ssh_config_nodelay_and_window() {
        let c = ssh_client_config();
        assert!(c.nodelay);
        assert!(c.window_size >= 16 * 1024 * 1024);
        assert!(c.maximum_packet_size >= 256 * 1024);
        assert!(c.channel_buffer_size >= 256);
    }

    #[test]
    fn test_effective_sftp_payload_max() {
        assert_eq!(
            effective_sftp_payload_max(None, None),
            SFTP_DEFAULT_PAYLOAD_MAX
        );
        assert_eq!(
            effective_sftp_payload_max(Some(0), Some(0)),
            SFTP_DEFAULT_PAYLOAD_MAX
        );
        assert_eq!(effective_sftp_payload_max(Some(16 * 1024), None), 16 * 1024);
        assert_eq!(
            effective_sftp_payload_max(Some(2 * 1024 * 1024), None),
            SFTP_ABS_PAYLOAD_MAX
        );
        assert_eq!(
            effective_sftp_payload_max(Some(2 * 1024 * 1024), Some(32 * 1024)),
            32 * 1024
        );
    }
}
