//! Host 输入校验，防止超长字符串、非法字符、路径遍历

use crate::models::host::Host;
use crate::utils::path::{expand_tilde_path, normalize_and_validate, normalize_path_for_create};

const MAX_NAME_LEN: usize = 128;
const MAX_HOST_LEN: usize = 256;
const MAX_USERNAME_LEN: usize = 128;
const MAX_PASSWORD_LEN: usize = 512;
const MAX_KEY_PATH_LEN: usize = 1024;

/// 校验 Host 输入
pub fn validate_host(host: &Host) -> Result<(), String> {
    validate_name(&host.name)?;
    validate_host_address(&host.host)?;
    validate_port(host.port)?;
    validate_username(&host.username)?;
    if let Some(ref p) = host.password {
        validate_password(p)?;
    }
    if let Some(ref k) = host.key_path {
        validate_key_path(k)?;
    }
    Ok(())
}

fn validate_name(name: &str) -> Result<(), String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("主机名称不能为空".to_string());
    }
    if trimmed.len() > MAX_NAME_LEN {
        return Err(format!(
            "主机名称不能超过 {} 个字符",
            MAX_NAME_LEN
        ));
    }
    Ok(())
}

fn validate_host_address(host: &str) -> Result<(), String> {
    let trimmed = host.trim();
    if trimmed.is_empty() {
        return Err("主机地址不能为空".to_string());
    }
    if trimmed.len() > MAX_HOST_LEN {
        return Err(format!(
            "主机地址不能超过 {} 个字符",
            MAX_HOST_LEN
        ));
    }
    if trimmed.starts_with("file://") {
        return Err("不允许 file:// 协议".to_string());
    }
    Ok(())
}

fn validate_port(port: u16) -> Result<(), String> {
    if port == 0 {
        return Err("端口不能为 0".to_string());
    }
    Ok(())
}

fn validate_username(username: &str) -> Result<(), String> {
    let trimmed = username.trim();
    if trimmed.is_empty() {
        return Err("用户名不能为空".to_string());
    }
    if trimmed.len() > MAX_USERNAME_LEN {
        return Err(format!(
            "用户名不能超过 {} 个字符",
            MAX_USERNAME_LEN
        ));
    }
    Ok(())
}

fn validate_password(password: &str) -> Result<(), String> {
    if password.len() > MAX_PASSWORD_LEN {
        return Err(format!(
            "密码不能超过 {} 个字符",
            MAX_PASSWORD_LEN
        ));
    }
    Ok(())
}

fn validate_key_path(key_path: &str) -> Result<(), String> {
    let trimmed = key_path.trim();
    if trimmed.is_empty() {
        return Err("密钥路径不能为空".to_string());
    }
    if trimmed.len() > MAX_KEY_PATH_LEN {
        return Err(format!(
            "密钥路径不能超过 {} 个字符",
            MAX_KEY_PATH_LEN
        ));
    }
    if trimmed.contains("..") {
        return Err("密钥路径不允许包含 ..".to_string());
    }
    let expanded = expand_tilde_path(trimmed)?;
    if !expanded.is_absolute() {
        return Err("密钥路径必须为绝对路径".to_string());
    }
    let expanded_str = expanded.to_string_lossy();
    if expanded.exists() {
        normalize_and_validate(expanded_str.as_ref())?;
    } else {
        normalize_path_for_create(expanded_str.as_ref())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::host::Protocol;

    fn valid_host() -> Host {
        Host {
            id: None,
            name: "test".into(),
            host: "192.168.1.1".into(),
            port: 22,
            protocol: Protocol::Sftp,
            username: "user".into(),
            password: Some("pass".into()),
            key_path: None,
            created_at: None,
            updated_at: None,
        }
    }

    #[test]
    fn test_validate_host_ok() {
        assert!(validate_host(&valid_host()).is_ok());
    }

    #[test]
    fn test_validate_name_empty() {
        let mut h = valid_host();
        h.name = "".into();
        assert!(validate_host(&h).is_err());
        h.name = "   ".into();
        assert!(validate_host(&h).is_err());
    }

    #[test]
    fn test_validate_name_too_long() {
        let mut h = valid_host();
        h.name = "a".repeat(MAX_NAME_LEN + 1);
        assert!(validate_host(&h).is_err());
    }

    #[test]
    fn test_validate_host_empty() {
        let mut h = valid_host();
        h.host = "".into();
        assert!(validate_host(&h).is_err());
    }

    #[test]
    fn test_validate_host_file_protocol() {
        let mut h = valid_host();
        h.host = "file:///etc/passwd".into();
        assert!(validate_host(&h).is_err());
    }

    #[test]
    fn test_validate_port_zero() {
        let mut h = valid_host();
        h.port = 0;
        assert!(validate_host(&h).is_err());
    }

    #[test]
    fn test_validate_username_empty() {
        let mut h = valid_host();
        h.username = "".into();
        assert!(validate_host(&h).is_err());
    }

    #[test]
    fn test_validate_key_path_traversal() {
        let mut h = valid_host();
        h.key_path = Some("/home/user/.ssh/../../../etc/passwd".into());
        assert!(validate_host(&h).is_err());
    }

    #[test]
    fn test_validate_key_path_tilde_under_home_nonexistent() {
        let mut h = valid_host();
        h.key_path = Some("~/ftx_validate_key_path_nonexistent_xyz123".into());
        assert!(validate_host(&h).is_ok());
    }
}
