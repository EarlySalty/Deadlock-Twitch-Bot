//! Zentrale Fehlertypen des Rust-Twitch-Bots.
//!
//! Phase 0a: nur [`CryptoError`] (Feldverschlüsselung). Weitere Domänen-Fehler
//! (DB, HTTP, Transport) folgen in späteren Phasen jeweils in eigenen `thiserror`-Enums.

use thiserror::Error;

/// Fehler der Feldverschlüsselung (`tb-crypto`).
///
/// Spiegelt die Python-Fehlerklassen `KeyMissing` / `InvalidPayload` /
/// `DecryptFailed` / `CryptoError` wider. Die Varianten tragen **keine**
/// Klartext- oder Key-Werte, damit nichts Geheimes in Logs landet.
#[derive(Debug, Error)]
pub enum CryptoError {
    /// Master-Key fehlt, ist kein gültiges Hex oder hat nicht exakt 32 Byte.
    #[error("encryption key missing or invalid")]
    KeyMissing,

    /// Der verschlüsselte Blob ist strukturell ungültig (zu kurz, falsche Version, …).
    #[error("invalid encrypted payload: {0}")]
    InvalidPayload(String),

    /// Entschlüsselung schlug fehl (AAD-/Tag-Mismatch oder defekter Ciphertext).
    #[error("decryption failed")]
    DecryptFailed,

    /// Verschlüsselung schlug fehl.
    #[error("encryption failed")]
    EncryptFailed,
}
