//! Sobre cifrado de exportación: contraseña → PBKDF2-HMAC-SHA256 → AES-256-GCM.
//! Compatible con el formato de la versión Swift (`jterm-encrypted`, versión 2).

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::Sha256;

pub const FORMAT: &str = "jterm-encrypted";
pub const ITERATIONS: u32 = 600_000;
const NONCE_LEN: usize = 12;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportEnvelope {
    pub format: String,
    pub version: u32,
    pub kdf: String,
    pub iterations: u32,
    /// base64
    pub salt: String,
    /// base64 de nonce + texto cifrado + etiqueta GCM
    pub data: String,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CryptoError {
    #[error("Contraseña incorrecta o archivo dañado.")]
    BadPassphrase,
    #[error("Formato de archivo no compatible.")]
    Unsupported,
    #[error("No se pudo derivar la clave de cifrado.")]
    Derivation,
}

fn derive(passphrase: &str, salt: &[u8], iterations: u32) -> Result<[u8; 32], CryptoError> {
    if passphrase.is_empty() || salt.is_empty() {
        return Err(CryptoError::Derivation);
    }
    let mut key = [0u8; 32];
    pbkdf2::pbkdf2_hmac::<Sha256>(passphrase.as_bytes(), salt, iterations, &mut key);
    Ok(key)
}

/// Cifra con las iteraciones estándar y devuelve el JSON del sobre.
pub fn encrypt(plain: &[u8], passphrase: &str) -> Result<Vec<u8>, CryptoError> {
    encrypt_with(plain, passphrase, ITERATIONS)
}

pub fn encrypt_with(plain: &[u8], passphrase: &str, iterations: u32) -> Result<Vec<u8>, CryptoError> {
    let mut salt = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut salt);
    let key = derive(passphrase, &salt, iterations)?;
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
    let mut nonce = [0u8; NONCE_LEN];
    rand::rngs::OsRng.fill_bytes(&mut nonce);
    let ct = cipher.encrypt(Nonce::from_slice(&nonce), plain).map_err(|_| CryptoError::Derivation)?;
    let mut combined = nonce.to_vec();
    combined.extend_from_slice(&ct);
    let env = ExportEnvelope {
        format: FORMAT.into(),
        version: 2,
        kdf: "pbkdf2-sha256".into(),
        iterations,
        salt: B64.encode(salt),
        data: B64.encode(combined),
    };
    serde_json::to_vec_pretty(&env).map_err(|_| CryptoError::Derivation)
}

/// ¿El archivo es un sobre cifrado de jTerm?
pub fn envelope(data: &[u8]) -> Option<ExportEnvelope> {
    serde_json::from_slice::<ExportEnvelope>(data).ok().filter(|e| e.format == FORMAT)
}

pub fn decrypt(env: &ExportEnvelope, passphrase: &str) -> Result<Vec<u8>, CryptoError> {
    if env.kdf != "pbkdf2-sha256" || !(1..=10_000_000).contains(&env.iterations) {
        return Err(CryptoError::Unsupported);
    }
    let salt = B64.decode(&env.salt).map_err(|_| CryptoError::Unsupported)?;
    let blob = B64.decode(&env.data).map_err(|_| CryptoError::Unsupported)?;
    if blob.len() < NONCE_LEN + 16 {
        return Err(CryptoError::BadPassphrase);
    }
    let key = derive(passphrase, &salt, env.iterations)?;
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
    let (nonce, ct) = blob.split_at(NONCE_LEN);
    cipher.decrypt(Nonce::from_slice(nonce), ct).map_err(|_| CryptoError::BadPassphrase)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let enc = encrypt_with(b"hola mundo", "clave", 1000).unwrap();
        let env = envelope(&enc).unwrap();
        assert_eq!(decrypt(&env, "clave").unwrap(), b"hola mundo");
        assert_eq!(decrypt(&env, "otra"), Err(CryptoError::BadPassphrase));
    }

    #[test]
    fn rejects_foreign() {
        assert!(envelope(b"{\"format\":\"x\"}").is_none());
        assert!(envelope(b"no json").is_none());
        assert_eq!(encrypt_with(b"x", "", 10), Err(CryptoError::Derivation));
    }

    #[test]
    fn bad_iterations() {
        let mut env = envelope(&encrypt_with(b"x", "k", 10).unwrap()).unwrap();
        env.iterations = 0;
        assert_eq!(decrypt(&env, "k"), Err(CryptoError::Unsupported));
    }
}
