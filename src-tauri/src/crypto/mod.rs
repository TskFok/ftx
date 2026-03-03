//! 敏感数据加密，使用 AES-GCM

use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use rand::RngCore;

const NONCE_LEN: usize = 12;
const ENC_PREFIX: &str = "enc:";

/// 加密字符串
pub fn encrypt(plaintext: &str, key: &[u8; 32]) -> Result<String, String> {
    if plaintext.is_empty() {
        return Ok(String::new());
    }
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| e.to_string())?;
    let mut nonce = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce);
    let ciphertext = cipher
        .encrypt(&nonce.into(), plaintext.as_bytes())
        .map_err(|e| e.to_string())?;
    let mut blob = nonce.to_vec();
    blob.extend_from_slice(&ciphertext);
    Ok(format!("{}{}", ENC_PREFIX, BASE64.encode(&blob)))
}

/// 解密字符串
pub fn decrypt(encoded: &str, key: &[u8; 32]) -> Result<String, String> {
    if encoded.is_empty() {
        return Ok(String::new());
    }
    if !encoded.starts_with(ENC_PREFIX) {
        return Ok(encoded.to_string());
    }
    let b64 = encoded
        .strip_prefix(ENC_PREFIX)
        .ok_or("无效的加密格式")?;
    let blob = BASE64.decode(b64).map_err(|e| format!("解码失败: {}", e))?;
    if blob.len() < NONCE_LEN {
        return Err("数据过短".to_string());
    }
    let (nonce_bytes, ciphertext) = blob.split_at(NONCE_LEN);
    let nonce: [u8; NONCE_LEN] = nonce_bytes
        .try_into()
        .map_err(|_| "nonce 长度错误")?;
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| e.to_string())?;
    let plaintext = cipher
        .decrypt((&nonce).into(), ciphertext)
        .map_err(|e| format!("解密失败: {}", e))?;
    String::from_utf8(plaintext).map_err(|e| format!("UTF-8 错误: {}", e))
}

/// 从应用数据目录加载或创建加密密钥
pub fn load_or_create_key(app_data_dir: &std::path::Path) -> Result<[u8; 32], String> {
    std::fs::create_dir_all(app_data_dir).map_err(|e| e.to_string())?;
    let key_path = app_data_dir.join(".ftx_encryption_key");
    if key_path.exists() {
        let bytes = std::fs::read(&key_path).map_err(|e| format!("读取密钥失败: {}", e))?;
        let arr: [u8; 32] = bytes
            .try_into()
            .map_err(|_| "密钥文件格式错误")?;
        return Ok(arr);
    }
    let mut key = [0u8; 32];
    OsRng.fill_bytes(&mut key);
    std::fs::write(&key_path, &key).map_err(|e| format!("写入密钥失败: {}", e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("设置密钥权限失败: {}", e))?;
    }
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let key = [42u8; 32];
        let plain = "secret password 123";
        let enc = encrypt(plain, &key).unwrap();
        assert!(enc.starts_with("enc:"));
        let dec = decrypt(&enc, &key).unwrap();
        assert_eq!(dec, plain);
    }

    #[test]
    fn test_decrypt_empty_returns_empty() {
        let key = [0u8; 32];
        assert_eq!(decrypt("", &key).unwrap(), "");
    }

    #[test]
    fn test_encrypt_empty_returns_empty() {
        let key = [0u8; 32];
        assert_eq!(encrypt("", &key).unwrap(), "");
    }

    #[test]
    fn test_decrypt_plaintext_passthrough() {
        let key = [0u8; 32];
        assert_eq!(decrypt("plaintext", &key).unwrap(), "plaintext");
    }

    #[test]
    fn test_decrypt_wrong_key_fails() {
        let key1 = [1u8; 32];
        let key2 = [2u8; 32];
        let enc = encrypt("secret", &key1).unwrap();
        assert!(decrypt(&enc, &key2).is_err());
    }
}
