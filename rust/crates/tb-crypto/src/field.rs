//! AES-256-GCM-Feldverschlüsselung — byte-identisch zu `bot/compat/field_crypto.py`.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use rand::RngCore;
use tb_error::CryptoError;
use zeroize::Zeroizing;

/// Format-Version (erstes Blob-Byte). Entspricht `FieldCrypto.VERSION = 1`.
pub const VERSION: u8 = 1;
/// GCM-Nonce-Länge in Byte. Entspricht `NONCE_SIZE = 12`.
pub const NONCE_SIZE: usize = 12;
/// AES-256-Schlüssellänge in Byte. Entspricht `KEY_SIZE = 32`.
pub const KEY_SIZE: usize = 32;
/// Einziger Key-Slot. Entspricht `self._keys['v1']`.
pub const KID: &str = "v1";

/// AES-256-GCM-Feldchiffre mit dem byte-genauen Blob-Format des Python-`FieldCrypto`.
pub struct FieldCipher {
    cipher: Aes256Gcm,
    kid: String,
}

impl FieldCipher {
    /// Lädt den Master-Key aus `DB_MASTER_KEY_V1` (Hex, exakt 32 Byte).
    /// Kein KDF, kein base64 — byte-identisch zu `FieldCrypto._load_keys`.
    pub fn from_env() -> Result<Self, CryptoError> {
        let raw = std::env::var("DB_MASTER_KEY_V1").map_err(|_| CryptoError::KeyMissing)?;
        Self::from_hex_key(raw.trim(), KID)
    }

    /// Baut die Chiffre aus einem Hex-Schlüssel. Die dekodierten Key-Bytes werden
    /// nach der Übergabe an die Chiffre wieder genullt (`Zeroizing`).
    pub fn from_hex_key(hex_key: &str, kid: &str) -> Result<Self, CryptoError> {
        let bytes = Zeroizing::new(hex::decode(hex_key).map_err(|_| CryptoError::KeyMissing)?);
        if bytes.len() != KEY_SIZE {
            return Err(CryptoError::KeyMissing);
        }
        let key = Key::<Aes256Gcm>::from_slice(bytes.as_slice());
        Ok(Self {
            cipher: Aes256Gcm::new(key),
            kid: kid.to_string(),
        })
    }

    /// Verschlüsselt `plaintext` unter `aad` und baut den BYTEA-Blob:
    /// `version[1] ‖ kid_len[1] ‖ kid ‖ nonce[12] ‖ ciphertext‖tag[16]`.
    pub fn encrypt_field(&self, plaintext: &str, aad: &str) -> Result<Vec<u8>, CryptoError> {
        let mut nonce_bytes = [0u8; NONCE_SIZE];
        rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
        self.encrypt_field_with_nonce(plaintext, aad, &nonce_bytes)
    }

    /// Wie [`FieldCipher::encrypt_field`], aber mit fester Nonce — für deterministische
    /// Testvektoren.
    pub fn encrypt_field_with_nonce(
        &self,
        plaintext: &str,
        aad: &str,
        nonce_bytes: &[u8; NONCE_SIZE],
    ) -> Result<Vec<u8>, CryptoError> {
        let nonce = Nonce::from_slice(nonce_bytes);
        let ct = self
            .cipher
            .encrypt(
                nonce,
                Payload {
                    msg: plaintext.as_bytes(),
                    aad: aad.as_bytes(),
                },
            )
            .map_err(|_| CryptoError::EncryptFailed)?;
        let kid = self.kid.as_bytes();
        let mut out = Vec::with_capacity(2 + kid.len() + NONCE_SIZE + ct.len());
        out.push(VERSION);
        out.push(kid.len() as u8);
        out.extend_from_slice(kid);
        out.extend_from_slice(nonce_bytes);
        out.extend_from_slice(&ct);
        Ok(out)
    }

    /// Entschlüsselt einen Python-/Rust-erzeugten Blob unter demselben `aad`.
    pub fn decrypt_field(&self, blob: &[u8], aad: &str) -> Result<String, CryptoError> {
        if blob.len() < 15 {
            return Err(CryptoError::InvalidPayload("blob too short".into()));
        }
        let version = blob[0];
        let kid_len = blob[1] as usize;
        if version != VERSION {
            return Err(CryptoError::InvalidPayload(format!("unknown version: {version}")));
        }
        let kid_end = 2 + kid_len;
        if blob.len() < kid_end + NONCE_SIZE {
            return Err(CryptoError::InvalidPayload("blob truncated (missing nonce)".into()));
        }
        let kid = std::str::from_utf8(&blob[2..kid_end])
            .map_err(|_| CryptoError::InvalidPayload("invalid key id encoding".into()))?;
        if kid != self.kid {
            return Err(CryptoError::KeyMissing);
        }
        let nonce_end = kid_end + NONCE_SIZE;
        let nonce = Nonce::from_slice(&blob[kid_end..nonce_end]);
        let ct = &blob[nonce_end..];
        if ct.is_empty() {
            return Err(CryptoError::InvalidPayload(
                "blob truncated (missing ciphertext)".into(),
            ));
        }
        let pt = self
            .cipher
            .decrypt(nonce, Payload { msg: ct, aad: aad.as_bytes() })
            .map_err(|_| CryptoError::DecryptFailed)?;
        String::from_utf8(pt).map_err(|_| CryptoError::DecryptFailed)
    }

    /// Der Key-Slot dieser Chiffre (`"v1"`).
    pub fn kid(&self) -> &str {
        &self.kid
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Fester 32-Byte-Testschlüssel (Hex). NUR für Tests — niemals ein Prod-Key.
    const TEST_KEY_HEX: &str =
        "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

    #[test]
    fn round_trip_recovers_plaintext() {
        let c = FieldCipher::from_hex_key(TEST_KEY_HEX, KID).unwrap();
        let aad = crate::aad::raid_auth("access_token", "555", 1);
        let blob = c.encrypt_field("twitch-token-xyz", &aad).unwrap();
        let back = c.decrypt_field(&blob, &aad).unwrap();
        assert_eq!(back, "twitch-token-xyz");
    }

    #[test]
    fn blob_has_expected_framing() {
        let c = FieldCipher::from_hex_key(TEST_KEY_HEX, KID).unwrap();
        let aad = crate::aad::raid_auth("access_token", "555", 1);
        let blob = c.encrypt_field("x", &aad).unwrap();
        assert_eq!(blob[0], VERSION); // 0x01
        assert_eq!(blob[1] as usize, KID.len()); // kid_len = 2
        assert_eq!(&blob[2..2 + KID.len()], KID.as_bytes()); // "v1"
        // Header(4) + nonce(12) + 1 Byte ct + 16 Byte tag = 33
        assert_eq!(blob.len(), 2 + KID.len() + NONCE_SIZE + 1 + 16);
    }

    #[test]
    fn wrong_aad_fails_decrypt() {
        let c = FieldCipher::from_hex_key(TEST_KEY_HEX, KID).unwrap();
        let blob = c
            .encrypt_field("secret", &crate::aad::raid_auth("access_token", "1", 1))
            .unwrap();
        let err = c.decrypt_field(&blob, &crate::aad::raid_auth("access_token", "2", 1));
        assert!(err.is_err(), "AAD-Mismatch muss fehlschlagen");
    }

    #[test]
    fn tampered_ciphertext_fails() {
        let c = FieldCipher::from_hex_key(TEST_KEY_HEX, KID).unwrap();
        let aad = crate::aad::raid_auth("access_token", "1", 1);
        let mut blob = c.encrypt_field("secret", &aad).unwrap();
        *blob.last_mut().unwrap() ^= 0xff; // Tag kippen
        assert!(c.decrypt_field(&blob, &aad).is_err());
    }

    #[test]
    fn rejects_non_32_byte_key() {
        assert!(FieldCipher::from_hex_key("00112233", KID).is_err());
    }
}
