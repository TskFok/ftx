/// 拼接远程路径。父目录为 `/` 时不能用 `trim_end_matches('/')` 后直接判空，否则会丢掉根前缀，
/// 变成相对路径（如只得到 `home` 而非 `/home`），导致 SFTP 列目录失败。
pub fn join_remote_path(parent: &str, name: &str) -> String {
    let name = name.trim_start_matches('/');
    let parent = parent.trim_end_matches('/');
    if parent.is_empty() {
        format!("/{}", name)
    } else {
        format!("{}/{}", parent, name)
    }
}

pub fn fmt_mtime(mtime: Option<u32>) -> Option<String> {
    mtime.and_then(|t| {
        chrono::DateTime::<chrono::Utc>::from_timestamp(i64::from(t), 0)
            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
    })
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
}
