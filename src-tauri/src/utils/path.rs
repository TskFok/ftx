//! 路径规范化与校验，防止路径遍历和符号链接攻击

use std::path::{Path, PathBuf};

/// 将路径开头的 `~/`（及 Windows 下的 `~\`）或单独的 `~` 展开为用户主目录。
pub fn expand_tilde_path(path: &str) -> Result<PathBuf, String> {
    let path = path.trim();
    if path.is_empty() {
        return Err("路径不能为空".to_string());
    }
    let home = || dirs::home_dir().ok_or_else(|| "无法获取用户主目录".to_string());
    if path == "~" {
        return home();
    }
    if path.starts_with("~/") {
        return Ok(home()?.join(&path[2..]));
    }
    #[cfg(windows)]
    if path.starts_with("~\\") {
        return Ok(home()?.join(&path[2..]));
    }
    Ok(PathBuf::from(path))
}

/// 规范化并校验路径（用于读取操作，路径必须存在）
pub fn normalize_and_validate(path: &str) -> Result<PathBuf, String> {
    let path = path.trim();
    if path.is_empty() {
        return Err("路径不能为空".to_string());
    }
    let p = Path::new(path);
    p.canonicalize().map_err(|e| format!("路径无效: {}", e))
}

/// 校验路径（用于创建操作，路径可能不存在）
/// 找到最长的已存在祖先目录，确保最终路径不会逃逸
pub fn normalize_path_for_create(path: &str) -> Result<PathBuf, String> {
    let path = path.trim();
    if path.is_empty() {
        return Err("路径不能为空".to_string());
    }
    let p = Path::new(path);
    if p.exists() {
        return p.canonicalize().map_err(|e| format!("路径无效: {}", e));
    }
    // 找到已存在的祖先
    let mut current = p.to_path_buf();
    loop {
        if let Some(parent) = current.parent() {
            if parent.exists() {
                let base = parent
                    .canonicalize()
                    .map_err(|e| format!("路径无效: {}", e))?;
                let remainder = p
                    .strip_prefix(parent)
                    .map_err(|_| "路径无效".to_string())?;
                return Ok(base.join(remainder));
            }
            current = parent.to_path_buf();
        } else {
            return Err("无法解析路径".to_string());
        }
    }
}

/// 拼接远程 POSIX 路径。父目录为 `/` 时不能用 `trim_end_matches('/')` 后直接判空，否则会丢掉根前缀，
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

/// 远端 `LIST`/`ls`/`readdir` 返回的名称有时为绝对路径或含 `/`；列表中应展示最后一段。
/// `ls -l` 形式的符号链接行含 `" -> "`，此时保留整段展示名以免误截取目标路径。
pub fn remote_list_entry_display_name(raw: &str) -> String {
    let s = raw.trim().trim_end_matches(['/', '\\']);
    if s.is_empty() {
        return String::new();
    }
    if s.contains(" -> ") {
        return s.to_string();
    }
    if s.starts_with('/') || s.contains('/') {
        return s
            .rsplit_once('/')
            .map(|(_, last)| last.to_string())
            .unwrap_or_else(|| s.to_string());
    }
    if s.contains('\\') {
        return s
            .rsplit_once('\\')
            .map(|(_, last)| last.to_string())
            .unwrap_or_else(|| s.to_string());
    }
    s.to_string()
}

/// 校验文件名，防止路径遍历（拒绝 ".."、"/"、"\"）
pub fn sanitize_filename(name: &str) -> Result<String, String> {
    if name.is_empty() {
        return Err("文件名不能为空".to_string());
    }
    if name.contains("..") || name.contains('/') || name.contains('\\') {
        return Err(format!("非法文件名: {}", name));
    }
    if name == "." || name == ".." {
        return Err(format!("非法文件名: {}", name));
    }
    Ok(name.to_string())
}

/// 在已验证的基路径下安全地 join 子路径
pub fn safe_join(base: &PathBuf, name: &str) -> Result<PathBuf, String> {
    let sanitized = sanitize_filename(name)?;
    let result = base.join(&sanitized);
    if result.starts_with(base) {
        Ok(result)
    } else {
        Err(format!("路径逃逸: {}", name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_expand_tilde_path_home_prefix() {
        let home = dirs::home_dir().expect("home dir");
        assert_eq!(expand_tilde_path("~").unwrap(), home);
        assert_eq!(expand_tilde_path("~/").unwrap(), home);
        assert_eq!(
            expand_tilde_path("~/Documents").unwrap(),
            home.join("Documents")
        );
        #[cfg(windows)]
        assert_eq!(
            expand_tilde_path(r"~\Documents").unwrap(),
            home.join("Documents")
        );
    }

    #[test]
    fn test_expand_tilde_path_absolute_unchanged() {
        let p = if cfg!(windows) {
            r"C:\absolute\id_rsa"
        } else {
            "/absolute/id_rsa"
        };
        assert_eq!(expand_tilde_path(p).unwrap(), PathBuf::from(p));
    }

    #[test]
    fn test_expand_tilde_path_empty_err() {
        assert!(expand_tilde_path("").is_err());
        assert!(expand_tilde_path("   ").is_err());
    }

    #[test]
    fn test_normalize_and_validate_exists() {
        let temp = std::env::temp_dir().join("ftx_test_normalize_exists");
        let _ = fs::remove_dir_all(&temp);
        fs::create_dir_all(&temp).unwrap();

        let result = normalize_and_validate(temp.to_string_lossy().as_ref()).unwrap();
        assert!(result.ends_with("ftx_test_normalize_exists"));

        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn test_normalize_and_validate_empty() {
        assert!(normalize_and_validate("").is_err());
        assert!(normalize_and_validate("   ").is_err());
    }

    #[test]
    fn test_normalize_and_validate_nonexistent() {
        let result = normalize_and_validate("/nonexistent/path/xyz");
        assert!(result.is_err());
    }

    #[test]
    fn test_normalize_and_validate_traversal() {
        let temp = std::env::temp_dir().join("ftx_test_normalize_traversal");
        let _ = fs::remove_dir_all(&temp);
        fs::create_dir_all(temp.join("sub")).unwrap();

        // canonicalize 会解析 .. 到真实路径
        let traversal = temp.join("sub/../sub").to_string_lossy().to_string();
        let result = normalize_and_validate(&traversal);
        assert!(result.is_ok());

        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn test_sanitize_filename_ok() {
        assert_eq!(sanitize_filename("a.txt").unwrap(), "a.txt");
        assert_eq!(sanitize_filename("my file").unwrap(), "my file");
    }

    #[test]
    fn test_sanitize_filename_rejects_traversal() {
        assert!(sanitize_filename("..").is_err());
        assert!(sanitize_filename("../etc").is_err());
        assert!(sanitize_filename("a/b").is_err());
        assert!(sanitize_filename("a\\b").is_err());
        assert!(sanitize_filename(".").is_err());
        assert!(sanitize_filename("").is_err());
    }

    #[test]
    fn test_normalize_path_for_create_existing() {
        let temp = std::env::temp_dir().join("ftx_test_create_existing");
        let _ = fs::remove_dir_all(&temp);
        fs::create_dir_all(&temp).unwrap();

        let result = normalize_path_for_create(temp.to_string_lossy().as_ref()).unwrap();
        assert!(result.ends_with("ftx_test_create_existing"));

        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn test_normalize_path_for_create_new() {
        let temp = std::env::temp_dir().join("ftx_test_create_new");
        let _ = fs::remove_dir_all(&temp);
        fs::create_dir_all(&temp).unwrap();

        let new_path = temp.join("subdir/newdir");
        let result = normalize_path_for_create(new_path.to_string_lossy().as_ref()).unwrap();
        assert!(result.ends_with("newdir"));

        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn test_safe_join() {
        let base = PathBuf::from("/tmp/ftx_test");
        let result = safe_join(&base, "file.txt").unwrap();
        assert_eq!(result, PathBuf::from("/tmp/ftx_test/file.txt"));

        assert!(safe_join(&base, "../etc").is_err());
        assert!(safe_join(&base, "..").is_err());
    }

    #[test]
    fn test_join_remote_path() {
        assert_eq!(join_remote_path("/home/u", "a.txt"), "/home/u/a.txt");
        assert_eq!(join_remote_path("/home/u/", "b"), "/home/u/b");
        assert_eq!(join_remote_path("/", "etc"), "/etc");
        assert_eq!(join_remote_path("//", "tmp"), "/tmp");
        assert_eq!(join_remote_path("", "x"), "/x");
    }

    #[test]
    fn test_remote_list_entry_display_name_plain() {
        assert_eq!(remote_list_entry_display_name("subdir"), "subdir");
        assert_eq!(remote_list_entry_display_name("my file.txt"), "my file.txt");
    }

    #[test]
    fn test_remote_list_entry_display_name_absolute() {
        assert_eq!(
            remote_list_entry_display_name("/home/user/myproject"),
            "myproject"
        );
    }

    #[test]
    fn test_remote_list_entry_display_name_relative_segments() {
        assert_eq!(remote_list_entry_display_name("a/b"), "b");
    }

    #[test]
    fn test_remote_list_entry_display_name_symlink_preserves_arrow() {
        assert_eq!(
            remote_list_entry_display_name("link -> /abs/target"),
            "link -> /abs/target"
        );
    }
}
