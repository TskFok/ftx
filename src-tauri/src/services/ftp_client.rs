use std::io::{Read, Seek, SeekFrom, Write};

use super::connection::{ConnectionTrait, FileEntry, CHUNK_SIZE};

pub struct FtpClient {
    host: String,
    port: u16,
    username: String,
    password: String,
    stream: Option<suppaftp::FtpStream>,
}

impl FtpClient {
    pub fn new(host: String, port: u16, username: String, password: String) -> Self {
        Self {
            host,
            port,
            username,
            password,
            stream: None,
        }
    }
}

struct ProgressReader<'a, R: Read> {
    inner: R,
    transferred: u64,
    total: u64,
    callback: Option<&'a dyn Fn(u64, u64)>,
    is_cancelled: Option<&'a dyn Fn() -> bool>,
}

impl<'a, R: Read> Read for ProgressReader<'a, R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.is_cancelled.is_some_and(|f| f()) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "Transfer cancelled",
            ));
        }
        let n = self.inner.read(buf)?;
        if n > 0 {
            self.transferred += n as u64;
            if let Some(cb) = self.callback {
                cb(self.transferred, self.total);
            }
        }
        Ok(n)
    }
}

impl ConnectionTrait for FtpClient {
    fn connect(&mut self) -> Result<(), String> {
        let addr = format!("{}:{}", self.host, self.port);
        let mut stream = suppaftp::FtpStream::connect(&addr).map_err(|e| e.to_string())?;
        stream
            .login(&self.username, &self.password)
            .map_err(|e| e.to_string())?;
        stream
            .transfer_type(suppaftp::types::FileType::Binary)
            .map_err(|e| e.to_string())?;
        self.stream = Some(stream);
        Ok(())
    }

    fn disconnect(&mut self) -> Result<(), String> {
        if let Some(ref mut stream) = self.stream {
            stream.quit().map_err(|e| e.to_string())?;
        }
        self.stream = None;
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.stream.is_some()
    }

    fn list_dir(&mut self, path: &str) -> Result<Vec<FileEntry>, String> {
        let stream = self.stream.as_mut().ok_or("Not connected")?;
        let entries = stream.list(Some(path)).map_err(|e| e.to_string())?;

        let mut files = Vec::new();
        for entry in entries {
            if let Some(file_entry) = parse_ftp_list_entry(&entry, path) {
                files.push(file_entry);
            }
        }
        Ok(files)
    }

    fn file_size(&mut self, path: &str) -> Result<u64, String> {
        let stream = self.stream.as_mut().ok_or("Not connected")?;
        stream
            .size(path)
            .map(|s| s as u64)
            .map_err(|e| e.to_string())
    }

    fn file_exists(&mut self, path: &str) -> Result<bool, String> {
        let (parent, name) = ftp_remote_parent_and_filename(path)?;
        let entries = self.list_dir(parent)?;
        Ok(entries.iter().any(|e| e.name == name && !e.is_dir))
    }

    fn upload(
        &mut self,
        local_path: &str,
        remote_path: &str,
        offset: u64,
        progress: Option<&dyn Fn(u64, u64)>,
        is_cancelled: Option<&dyn Fn() -> bool>,
    ) -> Result<u64, String> {
        let stream = self.stream.as_mut().ok_or("Not connected")?;
        let metadata = std::fs::metadata(local_path).map_err(|e| e.to_string())?;
        let total_size = metadata.len();

        let mut file = std::fs::File::open(local_path).map_err(|e| e.to_string())?;
        if offset > 0 {
            file.seek(SeekFrom::Start(offset))
                .map_err(|e| e.to_string())?;
            stream
                .resume_transfer(offset as usize)
                .map_err(|e| e.to_string())?;
        }

        let mut reader = ProgressReader {
            inner: file,
            transferred: offset,
            total: total_size,
            callback: progress,
            is_cancelled,
        };

        let _ = stream
            .put_file(remote_path, &mut reader)
            .map_err(|e| e.to_string())?;
        Ok(reader.transferred - offset)
    }

    fn download(
        &mut self,
        remote_path: &str,
        local_path: &str,
        offset: u64,
        progress: Option<&dyn Fn(u64, u64)>,
        is_cancelled: Option<&dyn Fn() -> bool>,
    ) -> Result<u64, String> {
        let stream = self.stream.as_mut().ok_or("Not connected")?;
        let total_size = stream
            .size(remote_path)
            .map(|s| s as u64)
            .map_err(|e| e.to_string())?;

        if offset > 0 {
            stream
                .resume_transfer(offset as usize)
                .map_err(|e| e.to_string())?;
        }

        let mut local_file = if offset > 0 {
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

        let mut transferred: u64 = 0;

        stream
            .retr(remote_path, |reader| {
                let mut buf = [0u8; CHUNK_SIZE];
                loop {
                    if is_cancelled.is_some_and(|f| f()) {
                        return Err(suppaftp::types::FtpError::ConnectionError(
                            std::io::Error::new(std::io::ErrorKind::Interrupted, "cancelled"),
                        ));
                    }
                    let n = reader
                        .read(&mut buf)
                        .map_err(suppaftp::types::FtpError::ConnectionError)?;
                    if n == 0 {
                        break;
                    }
                    local_file
                        .write_all(&buf[..n])
                        .map_err(suppaftp::types::FtpError::ConnectionError)?;
                    transferred += n as u64;
                    if let Some(ref cb) = progress {
                        cb(offset + transferred, total_size);
                    }
                }
                Ok(transferred)
            })
            .map_err(|e| e.to_string())
    }

    fn mkdir(&mut self, path: &str) -> Result<(), String> {
        let stream = self.stream.as_mut().ok_or("Not connected")?;
        stream.mkdir(path).map_err(|e| e.to_string())?;
        Ok(())
    }

    fn remove_file(&mut self, path: &str) -> Result<(), String> {
        let stream = self.stream.as_mut().ok_or("Not connected")?;
        stream.rm(path).map_err(|e| e.to_string())?;
        Ok(())
    }

    fn remove_dir(&mut self, path: &str) -> Result<(), String> {
        let stream = self.stream.as_mut().ok_or("Not connected")?;
        stream.rmdir(path).map_err(|e| e.to_string())?;
        Ok(())
    }

    fn rename(&mut self, from: &str, to: &str) -> Result<(), String> {
        let stream = self.stream.as_mut().ok_or("Not connected")?;
        stream.rename(from, to).map_err(|e| e.to_string())?;
        Ok(())
    }
}

fn ftp_remote_parent_and_filename(path: &str) -> Result<(&str, &str), String> {
    let path = path.trim();
    if path.is_empty() {
        return Err("路径不能为空".to_string());
    }
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return Err("路径无效".to_string());
    }
    match trimmed.rsplit_once('/') {
        Some(("", name)) => Ok(("/", name)),
        Some((parent, name)) => {
            if name.is_empty() {
                Err("路径无效".to_string())
            } else {
                Ok((parent, name))
            }
        }
        None => Ok((".", trimmed)),
    }
}

fn parse_ftp_list_entry(line: &str, parent_path: &str) -> Option<FileEntry> {
    use crate::utils::path::{join_remote_path, remote_list_entry_display_name};

    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 9 {
        return None;
    }

    let raw_name = parts[8..].join(" ");
    let name = remote_list_entry_display_name(&raw_name);
    if name.is_empty() || name == "." || name == ".." {
        return None;
    }

    let is_dir = line.starts_with('d');
    let size: u64 = parts[4].parse().unwrap_or(0);
    let path = join_remote_path(parent_path, &name);

    Some(FileEntry {
        name,
        path,
        is_dir,
        size,
        modified: Some(format!("{} {} {}", parts[5], parts[6], parts[7])),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ftp_remote_parent_and_filename() {
        assert_eq!(
            ftp_remote_parent_and_filename("/home/u/a.txt").unwrap(),
            ("/home/u", "a.txt")
        );
        assert_eq!(
            ftp_remote_parent_and_filename("/a.txt").unwrap(),
            ("/", "a.txt")
        );
        assert_eq!(
            ftp_remote_parent_and_filename("rel/x.bin").unwrap(),
            ("rel", "x.bin")
        );
        assert_eq!(
            ftp_remote_parent_and_filename("/home/u/a.txt/").unwrap(),
            ("/home/u", "a.txt")
        );
        assert!(ftp_remote_parent_and_filename("").is_err());
    }

    #[test]
    fn test_ftp_client_new() {
        let client = FtpClient::new(
            "127.0.0.1".into(),
            21,
            "user".into(),
            "pass".into(),
        );
        assert!(!client.is_connected());
        assert_eq!(client.host, "127.0.0.1");
        assert_eq!(client.port, 21);
    }

    #[test]
    fn test_parse_ftp_list_entry_file() {
        let line = "-rw-r--r--   1 user group   1024 Jan 01 12:00 test.txt";
        let entry = parse_ftp_list_entry(line, "/home").unwrap();
        assert_eq!(entry.name, "test.txt");
        assert_eq!(entry.path, "/home/test.txt");
        assert!(!entry.is_dir);
        assert_eq!(entry.size, 1024);
    }

    #[test]
    fn test_parse_ftp_list_entry_dir() {
        let line = "drwxr-xr-x   2 user group   4096 Jan 01 12:00 subdir";
        let entry = parse_ftp_list_entry(line, "/home/").unwrap();
        assert_eq!(entry.name, "subdir");
        assert_eq!(entry.path, "/home/subdir");
        assert!(entry.is_dir);
    }

    #[test]
    fn test_parse_ftp_list_entry_skips_dots() {
        let line = "drwxr-xr-x   2 user group   4096 Jan 01 12:00 .";
        assert!(parse_ftp_list_entry(line, "/").is_none());

        let line = "drwxr-xr-x   2 user group   4096 Jan 01 12:00 ..";
        assert!(parse_ftp_list_entry(line, "/").is_none());
    }

    #[test]
    fn test_parse_ftp_list_entry_dir_full_path_name() {
        let line = "drwxr-xr-x   2 user group   4096 Jan 01 12:00 /home/user/mydir";
        let entry = parse_ftp_list_entry(line, "/home/user").unwrap();
        assert_eq!(entry.name, "mydir");
        assert_eq!(entry.path, "/home/user/mydir");
        assert!(entry.is_dir);
    }

    #[test]
    fn test_parse_ftp_list_entry_invalid() {
        let line = "short line";
        assert!(parse_ftp_list_entry(line, "/").is_none());
    }

    #[test]
    fn test_parse_ftp_list_entry_filename_with_spaces() {
        let line = "-rw-r--r--   1 user group   2048 Feb 15 09:30 my file name.txt";
        let entry = parse_ftp_list_entry(line, "/data").unwrap();
        assert_eq!(entry.name, "my file name.txt");
        assert_eq!(entry.path, "/data/my file name.txt");
        assert_eq!(entry.size, 2048);
    }

    #[test]
    fn test_progress_reader() {
        let data = b"hello world";
        let progress_values = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let pv = progress_values.clone();

        let callback = move |transferred: u64, total: u64| {
            pv.lock().unwrap().push((transferred, total));
        };

        let mut reader = ProgressReader {
            inner: &data[..],
            transferred: 0,
            total: data.len() as u64,
            callback: Some(&callback),
            is_cancelled: None,
        };

        let mut buf = [0u8; 5];
        let n = reader.read(&mut buf).unwrap();
        assert_eq!(n, 5);
        assert_eq!(reader.transferred, 5);

        let n = reader.read(&mut buf).unwrap();
        assert_eq!(n, 5);
        assert_eq!(reader.transferred, 10);

        let n = reader.read(&mut buf).unwrap();
        assert_eq!(n, 1);
        assert_eq!(reader.transferred, 11);

        let values = progress_values.lock().unwrap();
        assert_eq!(values.len(), 3);
        assert_eq!(values[0], (5, 11));
        assert_eq!(values[1], (10, 11));
        assert_eq!(values[2], (11, 11));
    }

    #[test]
    fn test_progress_reader_aborts_when_cancelled() {
        let data = b"hello world";
        let cancel = || true;

        let mut reader = ProgressReader {
            inner: &data[..],
            transferred: 0,
            total: data.len() as u64,
            callback: None,
            is_cancelled: Some(&cancel),
        };

        let mut buf = [0u8; 5];
        let err = reader.read(&mut buf).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::Interrupted);
    }

    #[test]
    fn test_progress_reader_no_callback() {
        let data = b"test";
        let mut reader = ProgressReader {
            inner: &data[..],
            transferred: 0,
            total: data.len() as u64,
            callback: None,
            is_cancelled: None,
        };

        let mut buf = [0u8; 10];
        let n = reader.read(&mut buf).unwrap();
        assert_eq!(n, 4);
        assert_eq!(reader.transferred, 4);
    }

    #[test]
    fn test_progress_reader_with_offset() {
        let data = b"remaining data";
        let mut reader = ProgressReader {
            inner: &data[..],
            transferred: 100,
            total: 114,
            callback: None,
            is_cancelled: None,
        };

        let mut buf = [0u8; 256];
        let n = reader.read(&mut buf).unwrap();
        assert_eq!(n, 14);
        assert_eq!(reader.transferred, 114);
    }
}
