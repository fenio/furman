use std::collections::HashMap;
use std::path::{Path, PathBuf};

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use argon2::Argon2;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use rand::RngCore;

use crate::models::FmError;

// ── Encryption parameters ────────────────────────────────────────────────────

pub struct EncryptionParams {
    pub algorithm: String,    // "aes-256-gcm"
    pub kdf: String,          // "argon2id"
    pub salt: Vec<u8>,        // 16 bytes
    pub nonce: Vec<u8>,       // 12 bytes
    pub original_size: u64,
}

impl EncryptionParams {
    pub fn to_metadata(&self) -> HashMap<String, String> {
        let mut m = HashMap::new();
        m.insert("furman-encrypted".to_string(), self.algorithm.clone());
        m.insert("furman-kdf".to_string(), self.kdf.clone());
        m.insert("furman-salt".to_string(), BASE64.encode(&self.salt));
        m.insert("furman-nonce".to_string(), BASE64.encode(&self.nonce));
        m.insert("furman-original-size".to_string(), self.original_size.to_string());
        m
    }

    pub fn from_metadata(meta: &HashMap<String, String>) -> Option<Self> {
        let algorithm = meta.get("furman-encrypted")?;
        let kdf = meta.get("furman-kdf")?;
        let salt = BASE64.decode(meta.get("furman-salt")?).ok()?;
        let nonce = BASE64.decode(meta.get("furman-nonce")?).ok()?;
        let original_size: u64 = meta.get("furman-original-size")?.parse().ok()?;
        Some(Self {
            algorithm: algorithm.clone(),
            kdf: kdf.clone(),
            salt,
            nonce,
            original_size,
        })
    }

    pub fn is_encrypted(meta: &HashMap<String, String>) -> bool {
        meta.contains_key("furman-encrypted")
    }
}

// ── Key derivation ───────────────────────────────────────────────────────────

fn derive_key(password: &str, salt: &[u8]) -> Result<[u8; 32], FmError> {
    // OWASP-recommended Argon2id params: m=19456 KiB (19 MiB), t=2, p=1
    let params = argon2::Params::new(19456, 2, 1, Some(32))
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
pub fn encrypt_file(source: &Path, password: &str) -> Result<(PathBuf, EncryptionParams), FmError> {
    let plaintext = std::fs::read(source)?;
    let original_size = plaintext.len() as u64;

    let mut salt = [0u8; 16];
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut salt);
    rand::thread_rng().fill_bytes(&mut nonce_bytes);

    let key = derive_key(password, &salt)?;
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| FmError::Other(format!("Cipher init failed: {}", e)))?;
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_ref())
        .map_err(|e| FmError::Other(format!("Encryption failed: {}", e)))?;

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

    let params = EncryptionParams {
        algorithm: "aes-256-gcm".to_string(),
        kdf: "argon2id".to_string(),
        salt: salt.to_vec(),
        nonce: nonce_bytes.to_vec(),
        original_size,
    };

    Ok((temp_path, params))
}

/// Decrypt a file in-place. Reads ciphertext, decrypts, overwrites with plaintext.
pub fn decrypt_file(path: &Path, password: &str, params: &EncryptionParams) -> Result<(), FmError> {
    let ciphertext = std::fs::read(path)?;

    let key = derive_key(password, &params.salt)?;
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| FmError::Other(format!("Cipher init failed: {}", e)))?;
    let nonce = Nonce::from_slice(&params.nonce);

    let plaintext = cipher
        .decrypt(nonce, ciphertext.as_ref())
        .map_err(|_| FmError::Other("Decryption failed — wrong password or corrupted data".to_string()))?;

    std::fs::write(path, &plaintext)?;
    Ok(())
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_roundtrip() {
        let dir = std::env::temp_dir().join("furman-crypto-test");
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("test.txt");
        let mut f = std::fs::File::create(&src).unwrap();
        f.write_all(b"Hello, encryption!").unwrap();
        drop(f);

        let (enc_path, params) = encrypt_file(&src, "testpassword123").unwrap();

        // Encrypted file should differ from original
        let enc_data = std::fs::read(&enc_path).unwrap();
        assert_ne!(enc_data, b"Hello, encryption!");

        // Decrypt
        decrypt_file(&enc_path, "testpassword123", &params).unwrap();
        let dec_data = std::fs::read(&enc_path).unwrap();
        assert_eq!(dec_data, b"Hello, encryption!");
        assert_eq!(params.original_size, 18);

        // Cleanup
        let _ = std::fs::remove_file(&src);
        let _ = std::fs::remove_file(&enc_path);
    }

    #[test]
    fn test_wrong_password() {
        let dir = std::env::temp_dir().join("furman-crypto-test");
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("test_wrong.txt");
        std::fs::write(&src, b"secret data").unwrap();

        let (enc_path, params) = encrypt_file(&src, "correct").unwrap();

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
        };

        let meta = params.to_metadata();
        assert!(EncryptionParams::is_encrypted(&meta));

        let restored = EncryptionParams::from_metadata(&meta).unwrap();
        assert_eq!(restored.algorithm, "aes-256-gcm");
        assert_eq!(restored.kdf, "argon2id");
        assert_eq!(restored.salt, params.salt);
        assert_eq!(restored.nonce, params.nonce);
        assert_eq!(restored.original_size, 42);
    }

    #[test]
    fn test_not_encrypted_metadata() {
        let meta = HashMap::new();
        assert!(!EncryptionParams::is_encrypted(&meta));
        assert!(EncryptionParams::from_metadata(&meta).is_none());
    }
}
