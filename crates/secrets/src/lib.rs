//! `secrets` — small cryptographic helpers, modelled on tracehub-edge's
//! `tracehub-secrets` (generic, no project coupling):
//!
//! - [`SealingKey`] + [`seal`]/[`open`] — AES-256-GCM authenticated encryption
//!   (pure-Rust `RustCrypto`, no `OpenSSL`). The 96-bit nonce is random per seal
//!   and prepended to the ciphertext.
//! - [`SecretString`] / [`SecretBox`] (re-exported from `secrecy`) — values
//!   that render as `[REDACTED]` in `Debug`, so an accidental
//!   `tracing::debug!(?token)` can't leak them.
//! - [`SecretCache`] — a lock-free (`arc-swap`) cache of decrypted secrets,
//!   hot-swappable at runtime (the same pattern as a config handle).
//! - [`constant_time_eq`] — length-checked constant-time byte comparison for
//!   token/HMAC checks.
//!
//! ```
//! use secrets::{open, seal, SealingKey};
//!
//! let key = SealingKey::from_bytes([7u8; 32]);
//! let sealed = seal(&key, b"hunter2");
//! assert_eq!(open(&key, &sealed).unwrap(), b"hunter2");
//! assert!(open(&SealingKey::from_bytes([0u8; 32]), &sealed).is_err()); // wrong key
//! ```

#![allow(clippy::must_use_candidate, clippy::missing_errors_doc)]

mod cache;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use thiserror::Error;

pub use cache::SecretCache;
pub use secrecy::{ExposeSecret, SecretBox, SecretString};

const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;

/// Errors from sealing/opening and key loading.
#[derive(Debug, Error)]
pub enum SecretsError {
    /// AEAD authentication / decryption failed (wrong key or tampered data).
    #[error("decryption failed (wrong key or corrupted ciphertext)")]
    Decrypt,
    /// Sealed blob is too short to contain a nonce.
    #[error("malformed sealed data: {0}")]
    Malformed(&'static str),
    /// Key material was not exactly 32 bytes.
    #[error("invalid key length: expected {KEY_LEN} bytes, got {0}")]
    KeyLength(usize),
    /// Base64 decode failure.
    #[error("base64 decode: {0}")]
    Base64(#[from] base64::DecodeError),
    /// I/O error reading a key file.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// A 256-bit AES-GCM key.
#[derive(Clone)]
pub struct SealingKey([u8; KEY_LEN]);

impl SealingKey {
    /// Construct from raw 32 bytes.
    pub fn from_bytes(bytes: [u8; KEY_LEN]) -> Self {
        Self(bytes)
    }

    /// Construct from a base64 (standard alphabet) string of 32 bytes.
    pub fn from_base64(s: &str) -> Result<Self, SecretsError> {
        let bytes = BASE64.decode(s.trim())?;
        let len = bytes.len();
        let arr: [u8; KEY_LEN] = bytes.try_into().map_err(|_| SecretsError::KeyLength(len))?;
        Ok(Self(arr))
    }

    /// Load a base64-encoded key from a file (async).
    pub async fn from_file(path: impl AsRef<std::path::Path>) -> Result<Self, SecretsError> {
        let contents = tokio::fs::read_to_string(path).await?;
        Self::from_base64(&contents)
    }

    fn cipher(&self) -> Aes256Gcm {
        Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&self.0))
    }
}

/// Encrypt `plaintext` with AES-256-GCM. Output is `nonce(12) || ciphertext`.
///
/// # Panics
/// Panics only if the AEAD encrypt fails, which for AES-GCM with a valid key
/// and in-range plaintext does not happen.
pub fn seal(key: &SealingKey, plaintext: &[u8]) -> Vec<u8> {
    let nonce_bytes: [u8; NONCE_LEN] = rand::random();
    let ciphertext = key
        .cipher()
        .encrypt(Nonce::from_slice(&nonce_bytes), plaintext)
        .expect("AES-256-GCM encryption is infallible for valid inputs");

    let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    out
}

/// Decrypt a `nonce(12) || ciphertext` blob produced by [`seal`].
pub fn open(key: &SealingKey, sealed: &[u8]) -> Result<Vec<u8>, SecretsError> {
    if sealed.len() < NONCE_LEN {
        return Err(SecretsError::Malformed("shorter than nonce"));
    }
    let (nonce, ciphertext) = sealed.split_at(NONCE_LEN);
    key.cipher()
        .decrypt(Nonce::from_slice(nonce), ciphertext)
        .map_err(|_| SecretsError::Decrypt)
}

/// Length-checked constant-time equality. Use for comparing secret tokens /
/// HMAC tags so the comparison time doesn't leak how many bytes matched.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_open_round_trip() {
        let key = SealingKey::from_bytes([42u8; 32]);
        let sealed = seal(&key, b"top secret");
        assert_ne!(&sealed[12..], b"top secret", "ciphertext is not plaintext");
        assert_eq!(open(&key, &sealed).unwrap(), b"top secret");
    }

    #[test]
    fn wrong_key_fails_to_open() {
        let sealed = seal(&SealingKey::from_bytes([1u8; 32]), b"x");
        assert!(matches!(
            open(&SealingKey::from_bytes([2u8; 32]), &sealed),
            Err(SecretsError::Decrypt)
        ));
    }

    #[test]
    fn distinct_nonces_make_distinct_ciphertexts() {
        let key = SealingKey::from_bytes([9u8; 32]);
        assert_ne!(
            seal(&key, b"same"),
            seal(&key, b"same"),
            "random nonce per seal"
        );
    }

    #[test]
    fn base64_key_round_trips() {
        let raw = [3u8; 32];
        let b64 = BASE64.encode(raw);
        let key = SealingKey::from_base64(&b64).unwrap();
        let sealed = seal(&key, b"hi");
        assert_eq!(open(&SealingKey::from_bytes(raw), &sealed).unwrap(), b"hi");
    }

    #[test]
    fn bad_key_length_is_rejected() {
        assert!(matches!(
            SealingKey::from_base64(&BASE64.encode([0u8; 16])),
            Err(SecretsError::KeyLength(16))
        ));
    }

    #[test]
    fn constant_time_eq_matches_semantics() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
    }

    #[test]
    fn secret_string_is_redacted_in_debug() {
        let s = SecretString::from("hunter2".to_owned());
        let rendered = format!("{s:?}");
        assert!(
            !rendered.contains("hunter2"),
            "Debug leaked the secret: {rendered}"
        );
        assert_eq!(s.expose_secret(), "hunter2");
    }
}
