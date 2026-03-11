use ssh2::{CheckResult, KnownHostFileKind, Session};
use std::io::{Read, Seek, SeekFrom, Write};
use std::net::TcpStream;
use std::path::Path;

use super::connection::{ConnectionTrait, FileEntry, CHUNK_SIZE};

fn verify_host_key(session: &mut Session, host: &str, port: u16) -> Result<(), String> {
    let ssh_dir = dirs::home_dir()
        .ok_or_else(|| "无法获取用户主目录".to_string())?
        .join(".ssh");
    let known_hosts_path = ssh_dir.join("known_hosts");

    let mut known_hosts = session.known_hosts().map_err(|e| e.to_string())?;
    if known_hosts_path.exists() {
        let _ = known_hosts.read_file(&known_hosts_path, KnownHostFileKind::OpenSSH);
    }

    let (key, key_type) = session
        .host_key()
        .ok_or_else(|| "无法获取主机密钥".to_string())?;
    let host_str = if port == 22 {
        host.to_string()
    } else {
        format!("[{}]:{}", host, port)
    };

    match known_hosts.check(&host_str, key) {
        CheckResult::Match => Ok(()),
        CheckResult::Mismatch => Err(format!(
            "主机密钥不匹配，可能存在中间人攻击。请检查 ~/.ssh/known_hosts 中的 {} 条目",
            host_str
        )),
        CheckResult::NotFound => {
            // 首次连接时自动将主机密钥添加到 known_hosts
            known_hosts
                .add(&host_str, key, &host_str, key_type.into())
                .map_err(|e| e.to_string())?;
            std::fs::create_dir_all(&ssh_dir).map_err(|e| e.to_string())?;
            known_hosts
                .write_file(&known_hosts_path, KnownHostFileKind::OpenSSH)
                .map_err(|e| e.to_string())?;
            Ok(())
        }
        CheckResult::Failure => Err("主机密钥验证失败".to_string()),
    }
}

pub struct SftpClient {
    host: String,
    port: u16,
    username: String,
    password: Option<String>,
    key_path: Option<String>,
    session: Option<Session>,
}

impl SftpClient {
    pub fn new(
        host: String,
        port: u16,
        username: String,
        password: Option<String>,
        key_path: Option<String>,
    ) -> Self {
        Self {
            host,
            port,
            username,
            password,
            key_path,
            session: None,
        }
    }

    fn sftp(&self) -> Result<ssh2::Sftp, String> {
        self.session
            .as_ref()
            .ok_or_else(|| "Not connected".to_string())?
            .sftp()
            .map_err(|e| e.to_string())
    }
}

impl ConnectionTrait for SftpClient {
    fn connect(&mut self) -> Result<(), String> {
        let addr = format!("{}:{}", self.host, self.port);
        let tcp = TcpStream::connect(&addr).map_err(|e| e.to_string())?;
        let mut session = Session::new().map_err(|e| e.to_string())?;
        session.set_tcp_stream(tcp);
        session.handshake().map_err(|e| e.to_string())?;

        verify_host_key(&mut session, &self.host, self.port)?;

        if let Some(ref key_path) = self.key_path {
            session
                .userauth_pubkey_file(
                    &self.username,
                    None,
                    Path::new(key_path),
                    self.password.as_deref(),
                )
                .map_err(|e| e.to_string())?;
        } else if let Some(ref password) = self.password {
            session
                .userauth_password(&self.username, password)
                .map_err(|e| e.to_string())?;
        } else {
            return Err("No authentication method provided".to_string());
        }

        if !session.authenticated() {
            return Err("Authentication failed".to_string());
        }

        self.session = Some(session);
        Ok(())
    }

    fn disconnect(&mut self) -> Result<(), String> {
        if let Some(ref session) = self.session {
            session
                .disconnect(None, "bye", None)
                .map_err(|e| e.to_string())?;
        }
        self.session = None;
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.session
            .as_ref()
            .map(|s| s.authenticated())
            .unwrap_or(false)
    }

    fn list_dir(&mut self, path: &str) -> Result<Vec<FileEntry>, String> {
        let sftp = self.sftp()?;
        let entries = sftp
            .readdir(Path::new(path))
            .map_err(|e| e.to_string())?;

        let mut files = Vec::new();
        for (pathbuf, stat) in entries {
            let name = pathbuf
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            if name == "." || name == ".." {
                continue;
            }
            files.push(FileEntry {
                name,
                path: pathbuf.to_string_lossy().to_string(),
                is_dir: stat.is_dir(),
                size: stat.size.unwrap_or(0),
                modified: stat.mtime.map(|t| t.to_string()),
            });
        }
        Ok(files)
    }

    fn file_size(&mut self, path: &str) -> Result<u64, String> {
        let sftp = self.sftp()?;
        let stat = sftp.stat(Path::new(path)).map_err(|e| e.to_string())?;
        stat.size
            .ok_or_else(|| "Unable to determine file size".to_string())
    }

    fn file_exists(&mut self, path: &str) -> Result<bool, String> {
        let sftp = self.sftp()?;
        match sftp.stat(Path::new(path)) {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    fn upload(
        &mut self,
        local_path: &str,
        remote_path: &str,
        offset: u64,
        progress: Option<&dyn Fn(u64, u64)>,
    ) -> Result<u64, String> {
        let sftp = self.sftp()?;
        let metadata = std::fs::metadata(local_path).map_err(|e| e.to_string())?;
        let total_size = metadata.len();

        let mut local_file =
            std::fs::File::open(local_path).map_err(|e| e.to_string())?;

        let mut remote_file = if offset > 0 {
            local_file
                .seek(SeekFrom::Start(offset))
                .map_err(|e| e.to_string())?;
            let mut f = sftp
                .open_mode(
                    Path::new(remote_path),
                    ssh2::OpenFlags::WRITE,
                    0o644,
                    ssh2::OpenType::File,
                )
                .map_err(|e| e.to_string())?;
            f.seek(SeekFrom::Start(offset))
                .map_err(|e| e.to_string())?;
            f
        } else {
            sftp.create(Path::new(remote_path))
                .map_err(|e| e.to_string())?
        };

        let mut buf = [0u8; CHUNK_SIZE];
        let mut transferred = offset;

        loop {
            let n = local_file.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            remote_file
                .write_all(&buf[..n])
                .map_err(|e| e.to_string())?;
            transferred += n as u64;
            if let Some(cb) = progress {
                cb(transferred, total_size);
            }
        }

        Ok(transferred - offset)
    }

    fn download(
        &mut self,
        remote_path: &str,
        local_path: &str,
        offset: u64,
        progress: Option<&dyn Fn(u64, u64)>,
    ) -> Result<u64, String> {
        let sftp = self.sftp()?;
        let stat = sftp
            .stat(Path::new(remote_path))
            .map_err(|e| e.to_string())?;
        let total_size = stat.size.unwrap_or(0);

        let mut remote_file = sftp
            .open(Path::new(remote_path))
            .map_err(|e| e.to_string())?;

        let mut local_file = if offset > 0 {
            remote_file
                .seek(SeekFrom::Start(offset))
                .map_err(|e| e.to_string())?;
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .open(local_path)
                .map_err(|e| e.to_string())?;
            f.seek(SeekFrom::Start(offset))
                .map_err(|e| e.to_string())?;
            f
        } else {
            std::fs::File::create(local_path).map_err(|e| e.to_string())?
        };

        let mut buf = [0u8; CHUNK_SIZE];
        let mut transferred: u64 = 0;

        loop {
            let n = remote_file.read(&mut buf).map_err(|e| e.to_string())?;
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
        let sftp = self.sftp()?;
        sftp.mkdir(Path::new(path), 0o755)
            .map_err(|e| e.to_string())
    }

    fn remove_file(&mut self, path: &str) -> Result<(), String> {
        let sftp = self.sftp()?;
        sftp.unlink(Path::new(path)).map_err(|e| e.to_string())
    }

    fn remove_dir(&mut self, path: &str) -> Result<(), String> {
        let sftp = self.sftp()?;
        sftp.rmdir(Path::new(path)).map_err(|e| e.to_string())
    }

    fn rename(&mut self, from: &str, to: &str) -> Result<(), String> {
        let sftp = self.sftp()?;
        sftp.rename(Path::new(from), Path::new(to), None)
            .map_err(|e| e.to_string())
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
        assert!(client.sftp().is_err());
    }
}
