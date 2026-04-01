//! macOS：通过系统自带的 OpenSSH `sftp` 子进程实现 SFTP。
//! 密码或加密私钥口令在非 TTY 环境下通过 `SSH_ASKPASS` + 临时脚本提供。

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use uuid::Uuid;

use crate::utils::path::expand_tilde_path;

use super::common::{join_remote_path};
use crate::services::connection::{ConnectionTrait, FileEntry};

const ASKPASS_ENV: &str = "SSH_PASSPHRASE_FTX";

/// OpenSSH `ls -l` 风格行解析（mode nlink user group size mon day time name...）
fn parse_long_ls_line(line: &str) -> Option<(String, bool, u64, Option<String>)> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let first = line.chars().next()?;
    if first != 'd' && first != '-' && first != 'l' {
        return None;
    }
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 9 {
        return None;
    }
    let is_dir = parts[0].starts_with('d');
    let size: u64 = parts[4].parse().ok()?;
    let name = parts[8..].join(" ");
    if name == "." || name == ".." {
        return None;
    }
    let modified = Some(format!("{} {} {}", parts[5], parts[6], parts[7]));
    Some((name, is_dir, size, modified))
}

fn quote_sftp_token(p: &str) -> String {
    let escaped = p.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{}\"", escaped)
}

fn sftp_endpoint(username: &str, host: &str) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("{}@[{}]", username, host)
    } else {
        format!("{}@{}", username, host)
    }
}

struct AskpassScript {
    path: std::path::PathBuf,
}

impl AskpassScript {
    fn new() -> Result<Self, String> {
        let path = std::env::temp_dir().join(format!("ftx-askpass-{}.sh", Uuid::new_v4()));
        fs::write(
            &path,
            "#!/bin/sh\nprintf '%s\\n' \"$SSH_PASSPHRASE_FTX\"\n",
        )
        .map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
        }
        Ok(Self { path })
    }

    fn apply(&self, cmd: &mut Command, passphrase: &str) {
        cmd.env(ASKPASS_ENV, passphrase);
        cmd.env("SSH_ASKPASS", &self.path);
        cmd.env("SSH_ASKPASS_REQUIRE", "force");
        cmd.env("DISPLAY", ":0");
    }
}

impl Drop for AskpassScript {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// macOS SFTP：基于系统 `/usr/bin/sftp`。
pub struct SftpClient {
    host: String,
    port: u16,
    username: String,
    password: Option<String>,
    key_path: Option<String>,
    connected: bool,
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
            connected: false,
        }
    }

    fn require_connected(&self) -> Result<(), String> {
        if self.connected {
            Ok(())
        } else {
            Err("Not connected".to_string())
        }
    }

    fn sftp_bin() -> &'static str {
        if Path::new("/usr/bin/sftp").is_file() {
            "/usr/bin/sftp"
        } else {
            "sftp"
        }
    }

    /// 执行一批非交互 `sftp -b -` 命令，返回合并后的 stdout + stderr（便于报错）。
    fn run_batch(&self, batch: &str) -> Result<String, String> {
        if self.key_path.is_none() && self.password.is_none() {
            return Err("No authentication method provided".to_string());
        }

        let mut cmd = Command::new(Self::sftp_bin());
        cmd.args(["-o", "StrictHostKeyChecking=accept-new"]);
        cmd.args(["-o", "ConnectTimeout=30"]);
        cmd.arg("-P").arg(self.port.to_string());

        let _ask = if let Some(ref pw) = self.password {
            let s = AskpassScript::new()?;
            s.apply(&mut cmd, pw);
            Some(s)
        } else {
            cmd.args(["-o", "BatchMode=yes"]);
            None
        };

        if let Some(ref kp) = self.key_path {
            let path = expand_tilde_path(kp).map_err(|e| e.to_string())?;
            cmd.arg("-i").arg(path);
        }

        cmd.arg(sftp_endpoint(&self.username, &self.host));
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| {
            format!(
                "无法启动系统 sftp（需安装 Xcode CLT 或自带 OpenSSH）: {}",
                e
            )
        })?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(batch.as_bytes())
                .map_err(|e| e.to_string())?;
        }

        let out = child.wait_with_output().map_err(|e| e.to_string())?;
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        let combined = format!("{stderr}{stdout}");

        if !out.status.success() {
            return Err(if combined.trim().is_empty() {
                format!("sftp 失败，退出码 {:?}", out.status.code())
            } else {
                combined.trim().to_string()
            });
        }

        Ok(combined)
    }

    fn parse_ls_output(&self, output: &str, parent: &str) -> Result<Vec<FileEntry>, String> {
        let mut files = Vec::new();
        for line in output.lines() {
            if let Some((name, is_dir, size, modified)) = parse_long_ls_line(line) {
                let full_path = join_remote_path(parent, &name);
                files.push(FileEntry {
                    name,
                    path: full_path,
                    is_dir,
                    size,
                    modified,
                });
            }
        }
        Ok(files)
    }

    fn stat_line_remote(&self, path: &str) -> Result<String, String> {
        let q = quote_sftp_token(path);
        let out = self.run_batch(&format!("ls -ld {q}\n"))?;
        for line in out.lines() {
            if let Some(_) = parse_long_ls_line(line) {
                return Ok(line.trim().to_string());
            }
        }
        Err("Unable to stat remote path".to_string())
    }
}

impl ConnectionTrait for SftpClient {
    fn connect(&mut self) -> Result<(), String> {
        let _ = self.run_batch("pwd\n")?;
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

    fn list_dir(&mut self, path: &str) -> Result<Vec<FileEntry>, String> {
        self.require_connected()?;
        let q = quote_sftp_token(path);
        let out = self.run_batch(&format!("ls -la {q}\n"))?;
        self.parse_ls_output(&out, path)
    }

    fn file_size(&mut self, path: &str) -> Result<u64, String> {
        self.require_connected()?;
        let line = self.stat_line_remote(path)?;
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 9 {
            return Err("Unable to determine file size".to_string());
        }
        parts[4]
            .parse::<u64>()
            .map_err(|_| "Unable to determine file size".to_string())
    }

    fn file_exists(&mut self, path: &str) -> Result<bool, String> {
        self.require_connected()?;
        let q = quote_sftp_token(path);
        match self.run_batch(&format!("ls -ld {q}\n")) {
            Ok(out) => {
                if out.contains("No such file")
                    || out.contains("not found")
                    || out.contains("Couldn't stat")
                {
                    Ok(false)
                } else {
                    Ok(true)
                }
            }
            Err(e) => {
                let el = e.to_lowercase();
                if el.contains("no such file") || el.contains("couldn't stat") {
                    Ok(false)
                } else {
                    Err(e)
                }
            }
        }
    }

    fn upload(
        &mut self,
        local_path: &str,
        remote_path: &str,
        offset: u64,
        progress: Option<&dyn Fn(u64, u64)>,
    ) -> Result<u64, String> {
        self.require_connected()?;
        let metadata = fs::metadata(local_path).map_err(|e| e.to_string())?;
        let total_size = metadata.len();
        let rl = quote_sftp_token(remote_path);
        let ll = quote_sftp_token(local_path);

        let cmd = if offset > 0 {
            format!("reput {ll} {rl}\n")
        } else {
            format!("put -f {ll} {rl}\n")
        };

        if let Some(cb) = progress {
            cb(offset, total_size);
        }

        self.run_batch(&cmd)?;

        if let Some(cb) = progress {
            cb(total_size, total_size);
        }

        Ok(if offset > 0 {
            total_size.saturating_sub(offset)
        } else {
            total_size
        })
    }

    fn download(
        &mut self,
        remote_path: &str,
        local_path: &str,
        offset: u64,
        progress: Option<&dyn Fn(u64, u64)>,
    ) -> Result<u64, String> {
        self.require_connected()?;
        let total_size = self.file_size(remote_path)?;
        let rl = quote_sftp_token(remote_path);
        let ll = quote_sftp_token(local_path);

        if offset > 0 {
            let _ = fs::OpenOptions::new()
                .append(true)
                .open(local_path)
                .map_err(|e| e.to_string())?;
        } else {
            let _ = fs::remove_file(local_path);
        }

        let cmd = if offset > 0 {
            format!("reget {rl} {ll}\n")
        } else {
            format!("get -f {rl} {ll}\n")
        };

        if let Some(cb) = progress {
            cb(offset, total_size);
        }

        self.run_batch(&cmd)?;

        let end_len = fs::metadata(local_path)
            .map_err(|e| e.to_string())?
            .len();

        if let Some(cb) = progress {
            cb(end_len.min(total_size), total_size);
        }

        Ok(end_len.saturating_sub(offset))
    }

    fn mkdir(&mut self, path: &str) -> Result<(), String> {
        self.require_connected()?;
        let q = quote_sftp_token(path);
        self.run_batch(&format!("mkdir {q}\n"))?;
        Ok(())
    }

    fn remove_file(&mut self, path: &str) -> Result<(), String> {
        self.require_connected()?;
        let q = quote_sftp_token(path);
        self.run_batch(&format!("rm {q}\n"))?;
        Ok(())
    }

    fn remove_dir(&mut self, path: &str) -> Result<(), String> {
        self.require_connected()?;
        let q = quote_sftp_token(path);
        self.run_batch(&format!("rmdir {q}\n"))?;
        Ok(())
    }

    fn rename(&mut self, from: &str, to: &str) -> Result<(), String> {
        self.require_connected()?;
        let a = quote_sftp_token(from);
        let b = quote_sftp_token(to);
        self.run_batch(&format!("rename {a} {b}\n"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::parse_long_ls_line;

    #[test]
    fn test_parse_long_ls_file() {
        let line = "-rw-r--r--    1 user  staff   12345 Jan 15 10:30 foo.txt";
        let (name, is_dir, size, m) = parse_long_ls_line(line).unwrap();
        assert_eq!(name, "foo.txt");
        assert!(!is_dir);
        assert_eq!(size, 12345);
        assert!(m.is_some());
    }

    #[test]
    fn test_parse_long_ls_dir() {
        let line = "drwxr-xr-x    4 user  staff   128 Jan  1  2020 mydir";
        let (name, is_dir, _, _) = parse_long_ls_line(line).unwrap();
        assert_eq!(name, "mydir");
        assert!(is_dir);
    }

    #[test]
    fn test_parse_long_ls_name_with_spaces() {
        let line = "-rw-r--r--    1 user  staff   1 Jan  1 12:00 a b c.txt";
        let (name, _, size, _) = parse_long_ls_line(line).unwrap();
        assert_eq!(name, "a b c.txt");
        assert_eq!(size, 1);
    }

    #[test]
    fn test_quote_sftp_token() {
        assert_eq!(super::quote_sftp_token("/tmp/a b"), "\"/tmp/a b\"");
    }
}
