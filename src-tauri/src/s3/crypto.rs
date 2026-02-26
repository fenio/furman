use std::collections::HashMap;
use std::path::{Path, PathBuf};

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce as AesNonce};
use chacha20poly1305::{ChaCha20Poly1305, Nonce as ChaChaNonce};
use argon2::Argon2;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use rand::RngCore;

use crate::models::FmError;

// ── Encryption config (passed from frontend) ────────────────────────────────

/// User-configurable encryption settings (per-profile).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct EncryptionConfig {
    /// "aes-256-gcm" (default) or "chacha20-poly1305"
    #[serde(default = "default_algorithm")]
    pub algorithm: String,
    /// Argon2id memory cost in KiB (default: 19456 = ~19 MiB)
    #[serde(default = "default_kdf_memory")]
    pub kdf_memory_cost: u32,
    /// Argon2id time cost / iterations (default: 2)
    #[serde(default = "default_kdf_time")]
    pub kdf_time_cost: u32,
    /// Argon2id parallelism (default: 1)
    #[serde(default = "default_kdf_parallelism")]
    pub kdf_parallelism: u32,
    /// Overwrite temp files with zeros before deleting
    #[serde(default)]
    pub secure_temp_cleanup: bool,
}

fn default_algorithm() -> String { "aes-256-gcm".to_string() }
fn default_kdf_memory() -> u32 { 19456 }
fn default_kdf_time() -> u32 { 2 }
fn default_kdf_parallelism() -> u32 { 1 }

impl Default for EncryptionConfig {
    fn default() -> Self {
        Self {
            algorithm: default_algorithm(),
            kdf_memory_cost: default_kdf_memory(),
            kdf_time_cost: default_kdf_time(),
            kdf_parallelism: default_kdf_parallelism(),
            secure_temp_cleanup: false,
        }
    }
}

// ── Encryption parameters (stored in S3 metadata) ───────────────────────────

pub struct EncryptionParams {
    pub algorithm: String,    // "aes-256-gcm" or "chacha20-poly1305"
    pub kdf: String,          // "argon2id"
    pub salt: Vec<u8>,        // 16 bytes
    pub nonce: Vec<u8>,       // 12 bytes
    pub original_size: u64,
    pub kdf_memory_cost: u32,
    pub kdf_time_cost: u32,
    pub kdf_parallelism: u32,
}

impl EncryptionParams {
    pub fn to_metadata(&self) -> HashMap<String, String> {
        let mut m = HashMap::new();
        m.insert("furman-encrypted".to_string(), self.algorithm.clone());
        m.insert("furman-kdf".to_string(), self.kdf.clone());
        m.insert("furman-salt".to_string(), BASE64.encode(&self.salt));
        m.insert("furman-nonce".to_string(), BASE64.encode(&self.nonce));
        m.insert("furman-original-size".to_string(), self.original_size.to_string());
        m.insert("furman-kdf-m".to_string(), self.kdf_memory_cost.to_string());
        m.insert("furman-kdf-t".to_string(), self.kdf_time_cost.to_string());
        m.insert("furman-kdf-p".to_string(), self.kdf_parallelism.to_string());
        m
    }

    pub fn from_metadata(meta: &HashMap<String, String>) -> Option<Self> {
        let algorithm = meta.get("furman-encrypted")?;
        let kdf = meta.get("furman-kdf")?;
        let salt = BASE64.decode(meta.get("furman-salt")?).ok()?;
        let nonce = BASE64.decode(meta.get("furman-nonce")?).ok()?;
        let original_size: u64 = meta.get("furman-original-size")?.parse().ok()?;
        // KDF params — default to OWASP-recommended values for backward compat
        let kdf_memory_cost = meta.get("furman-kdf-m").and_then(|v| v.parse().ok()).unwrap_or(19456);
        let kdf_time_cost = meta.get("furman-kdf-t").and_then(|v| v.parse().ok()).unwrap_or(2);
        let kdf_parallelism = meta.get("furman-kdf-p").and_then(|v| v.parse().ok()).unwrap_or(1);
        Some(Self {
            algorithm: algorithm.clone(),
            kdf: kdf.clone(),
            salt,
            nonce,
            original_size,
            kdf_memory_cost,
            kdf_time_cost,
            kdf_parallelism,
        })
    }

    pub fn is_encrypted(meta: &HashMap<String, String>) -> bool {
        meta.contains_key("furman-encrypted")
    }
}

// ── Key derivation ───────────────────────────────────────────────────────────

fn derive_key(password: &str, salt: &[u8], m_cost: u32, t_cost: u32, p_cost: u32) -> Result<[u8; 32], FmError> {
    let params = argon2::Params::new(m_cost, t_cost, p_cost, Some(32))
        .map_err(|e| FmError::Other(format!("Argon2 params error: {}", e)))?;
    let argon2 = Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);

    let mut key = [0u8; 32];
    argon2
        .hash_password_into(password.as_bytes(), salt, &mut key)
        .map_err(|e| FmError::Other(format!("Key derivation failed: {}", e)))?;
    Ok(key)
}

// ── Encrypt / Decrypt ────────────────────────────────────────────────────────

/// Encrypt a file to a temp file. Returns (temp_path, params).
pub fn encrypt_file(source: &Path, password: &str, config: &EncryptionConfig) -> Result<(PathBuf, EncryptionParams), FmError> {
    let plaintext = std::fs::read(source)?;
    let original_size = plaintext.len() as u64;

    let mut salt = [0u8; 16];
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut salt);
    rand::thread_rng().fill_bytes(&mut nonce_bytes);

    let key = derive_key(password, &salt, config.kdf_memory_cost, config.kdf_time_cost, config.kdf_parallelism)?;

    let ciphertext = match config.algorithm.as_str() {
        "chacha20-poly1305" => {
            let cipher = ChaCha20Poly1305::new_from_slice(&key)
                .map_err(|e| FmError::Other(format!("Cipher init failed: {}", e)))?;
            let nonce = ChaChaNonce::from_slice(&nonce_bytes);
            cipher
                .encrypt(nonce, plaintext.as_ref())
                .map_err(|e| FmError::Other(format!("Encryption failed: {}", e)))?
        }
        _ => {
            // Default: AES-256-GCM
            let cipher = Aes256Gcm::new_from_slice(&key)
                .map_err(|e| FmError::Other(format!("Cipher init failed: {}", e)))?;
            let nonce = AesNonce::from_slice(&nonce_bytes);
            cipher
                .encrypt(nonce, plaintext.as_ref())
                .map_err(|e| FmError::Other(format!("Encryption failed: {}", e)))?
        }
    };

    // Write to temp file
    let filename = source
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "encrypted".to_string());
    let temp_path = std::env::temp_dir()
        .join("furman-encrypt")
        .join(format!("{}-{}", &BASE64.encode(&salt)[..8], filename));

    if let Some(parent) = temp_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&temp_path, &ciphertext)?;

    let algorithm = match config.algorithm.as_str() {
        "chacha20-poly1305" => "chacha20-poly1305",
        _ => "aes-256-gcm",
    };

    let params = EncryptionParams {
        algorithm: algorithm.to_string(),
        kdf: "argon2id".to_string(),
        salt: salt.to_vec(),
        nonce: nonce_bytes.to_vec(),
        original_size,
        kdf_memory_cost: config.kdf_memory_cost,
        kdf_time_cost: config.kdf_time_cost,
        kdf_parallelism: config.kdf_parallelism,
    };

    Ok((temp_path, params))
}

/// Decrypt a file in-place. Reads ciphertext, decrypts, overwrites with plaintext.
/// Algorithm and KDF params are read from the EncryptionParams (stored in S3 metadata).
pub fn decrypt_file(path: &Path, password: &str, params: &EncryptionParams) -> Result<(), FmError> {
    let ciphertext = std::fs::read(path)?;

    let key = derive_key(password, &params.salt, params.kdf_memory_cost, params.kdf_time_cost, params.kdf_parallelism)?;

    let plaintext = match params.algorithm.as_str() {
        "chacha20-poly1305" => {
            let cipher = ChaCha20Poly1305::new_from_slice(&key)
                .map_err(|e| FmError::Other(format!("Cipher init failed: {}", e)))?;
            let nonce = ChaChaNonce::from_slice(&params.nonce);
            cipher
                .decrypt(nonce, ciphertext.as_ref())
                .map_err(|_| FmError::Other("Decryption failed — wrong password or corrupted data".to_string()))?
        }
        _ => {
            let cipher = Aes256Gcm::new_from_slice(&key)
                .map_err(|e| FmError::Other(format!("Cipher init failed: {}", e)))?;
            let nonce = AesNonce::from_slice(&params.nonce);
            cipher
                .decrypt(nonce, ciphertext.as_ref())
                .map_err(|_| FmError::Other("Decryption failed — wrong password or corrupted data".to_string()))?
        }
    };

    std::fs::write(path, &plaintext)?;
    Ok(())
}

/// Securely delete a file by overwriting with zeros before removal.
pub fn secure_delete(path: &Path) -> std::io::Result<()> {
    if let Ok(meta) = std::fs::metadata(path) {
        let size = meta.len() as usize;
        if size > 0 {
            std::fs::write(path, &vec![0u8; size])?;
        }
    }
    std::fs::remove_file(path)
}

/// Clean up temp files, optionally with secure deletion.
pub fn cleanup_temp_files(files: &[PathBuf], secure: bool) {
    for f in files {
        if secure {
            let _ = secure_delete(f);
        } else {
            let _ = std::fs::remove_file(f);
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_roundtrip_aes() {
        let dir = std::env::temp_dir().join("furman-crypto-test");
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("test_aes.txt");
        let mut f = std::fs::File::create(&src).unwrap();
        f.write_all(b"Hello, encryption!").unwrap();
        drop(f);

        let config = EncryptionConfig::default();
        let (enc_path, params) = encrypt_file(&src, "testpassword123", &config).unwrap();

        let enc_data = std::fs::read(&enc_path).unwrap();
        assert_ne!(enc_data, b"Hello, encryption!");
        assert_eq!(params.algorithm, "aes-256-gcm");

        decrypt_file(&enc_path, "testpassword123", &params).unwrap();
        let dec_data = std::fs::read(&enc_path).unwrap();
        assert_eq!(dec_data, b"Hello, encryption!");
        assert_eq!(params.original_size, 18);

        let _ = std::fs::remove_file(&src);
        let _ = std::fs::remove_file(&enc_path);
    }

    #[test]
    fn test_roundtrip_chacha() {
        let dir = std::env::temp_dir().join("furman-crypto-test");
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("test_chacha.txt");
        std::fs::write(&src, b"ChaCha20 test data").unwrap();

        let config = EncryptionConfig {
            algorithm: "chacha20-poly1305".to_string(),
            ..EncryptionConfig::default()
        };
        let (enc_path, params) = encrypt_file(&src, "mypassword", &config).unwrap();

        assert_eq!(params.algorithm, "chacha20-poly1305");

        decrypt_file(&enc_path, "mypassword", &params).unwrap();
        let dec_data = std::fs::read(&enc_path).unwrap();
        assert_eq!(dec_data, b"ChaCha20 test data");

        let _ = std::fs::remove_file(&src);
        let _ = std::fs::remove_file(&enc_path);
    }

    #[test]
    fn test_wrong_password() {
        let dir = std::env::temp_dir().join("furman-crypto-test");
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("test_wrong.txt");
        std::fs::write(&src, b"secret data").unwrap();

        let config = EncryptionConfig::default();
        let (enc_path, params) = encrypt_file(&src, "correct", &config).unwrap();

        let result = decrypt_file(&enc_path, "wrong", &params);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("wrong password"));

        let _ = std::fs::remove_file(&src);
        let _ = std::fs::remove_file(&enc_path);
    }

    #[test]
    fn test_metadata_roundtrip() {
        let params = EncryptionParams {
            algorithm: "aes-256-gcm".to_string(),
            kdf: "argon2id".to_string(),
            salt: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
            nonce: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12],
            original_size: 42,
            kdf_memory_cost: 32768,
            kdf_time_cost: 4,
            kdf_parallelism: 2,
        };

        let meta = params.to_metadata();
        assert!(EncryptionParams::is_encrypted(&meta));

        let restored = EncryptionParams::from_metadata(&meta).unwrap();
        assert_eq!(restored.algorithm, "aes-256-gcm");
        assert_eq!(restored.kdf, "argon2id");
        assert_eq!(restored.salt, params.salt);
        assert_eq!(restored.nonce, params.nonce);
        assert_eq!(restored.original_size, 42);
        assert_eq!(restored.kdf_memory_cost, 32768);
        assert_eq!(restored.kdf_time_cost, 4);
        assert_eq!(restored.kdf_parallelism, 2);
    }

    #[test]
    fn test_metadata_backward_compat() {
        // Old-style metadata without KDF params
        let mut meta = HashMap::new();
        meta.insert("furman-encrypted".to_string(), "aes-256-gcm".to_string());
        meta.insert("furman-kdf".to_string(), "argon2id".to_string());
        meta.insert("furman-salt".to_string(), BASE64.encode(&[1u8; 16]));
        meta.insert("furman-nonce".to_string(), BASE64.encode(&[2u8; 12]));
        meta.insert("furman-original-size".to_string(), "100".to_string());

        let restored = EncryptionParams::from_metadata(&meta).unwrap();
        // Should fall back to defaults
        assert_eq!(restored.kdf_memory_cost, 19456);
        assert_eq!(restored.kdf_time_cost, 2);
        assert_eq!(restored.kdf_parallelism, 1);
    }

    #[test]
    fn test_custom_kdf_params() {
        let dir = std::env::temp_dir().join("furman-crypto-test");
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("test_custom_kdf.txt");
        std::fs::write(&src, b"custom kdf test").unwrap();

        let config = EncryptionConfig {
            kdf_memory_cost: 8192,
            kdf_time_cost: 1,
            kdf_parallelism: 1,
            ..EncryptionConfig::default()
        };
        let (enc_path, params) = encrypt_file(&src, "pass", &config).unwrap();
        assert_eq!(params.kdf_memory_cost, 8192);
        assert_eq!(params.kdf_time_cost, 1);

        // Decrypt uses params from metadata, not config
        decrypt_file(&enc_path, "pass", &params).unwrap();
        let dec_data = std::fs::read(&enc_path).unwrap();
        assert_eq!(dec_data, b"custom kdf test");

        let _ = std::fs::remove_file(&src);
        let _ = std::fs::remove_file(&enc_path);
    }

    #[test]
    fn test_not_encrypted_metadata() {
        let meta = HashMap::new();
        assert!(!EncryptionParams::is_encrypted(&meta));
        assert!(EncryptionParams::from_metadata(&meta).is_none());
    }

    #[test]
    fn test_secure_delete() {
        let dir = std::env::temp_dir().join("furman-crypto-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test_secure_del.tmp");
        std::fs::write(&path, b"sensitive data here").unwrap();
        assert!(path.exists());

        secure_delete(&path).unwrap();
        assert!(!path.exists());
    }
}
