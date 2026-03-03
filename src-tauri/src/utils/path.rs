//! 路径规范化与校验，防止路径遍历和符号链接攻击

use std::path::{Path, PathBuf};

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
}
