use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use rand::TryRng;
use tb_error::CryptoError;
use zeroize::Zeroizing;

pub const VERSION: u8 = 1;
pub const NONCE_SIZE: usize = 12;
pub const KEY_SIZE: usize = 32;
pub const KID: &str = "v1";

#[derive(Clone)]
pub struct FieldCipher {
    cipher: Aes256Gcm,
    kid: String,
}

impl FieldCipher {
    pub fn from_env() -> Result<Self, CryptoError> {
        let raw = std::env::var("DB_MASTER_KEY_V1").map_err(|_| CryptoError::KeyMissing)?;
        Self::from_hex_key(raw.trim(), KID)
    }

    pub fn from_hex_key(hex_key: &str, kid: &str) -> Result<Self, CryptoError> {
        let bytes = Zeroizing::new(hex::decode(hex_key).map_err(|_| CryptoError::KeyMissing)?);
        if bytes.len() != KEY_SIZE {
            return Err(CryptoError::KeyMissing);
        }
        let key =
            <&Key<Aes256Gcm>>::try_from(bytes.as_slice()).map_err(|_| CryptoError::KeyMissing)?;
        Ok(Self {
            cipher: Aes256Gcm::new(key),
            kid: kid.to_string(),
        })
    }

    pub fn encrypt_field(&self, plaintext: &str, aad: &str) -> Result<Vec<u8>, CryptoError> {
        let mut nonce_bytes = [0u8; NONCE_SIZE];
        rand::rngs::SysRng
            .try_fill_bytes(&mut nonce_bytes)
            .map_err(|_| CryptoError::EncryptFailed)?;
        self.encrypt_field_with_nonce(plaintext, aad, &nonce_bytes)
    }

    pub fn encrypt_field_with_nonce(
        &self,
        plaintext: &str,
        aad: &str,
        nonce_bytes: &[u8; NONCE_SIZE],
    ) -> Result<Vec<u8>, CryptoError> {
        self.encrypt_bytes_with_nonce(plaintext.as_bytes(), aad.as_bytes(), nonce_bytes)
    }

    pub fn encrypt_bytes(&self, plaintext: &[u8], aad: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let mut nonce_bytes = [0u8; NONCE_SIZE];
        rand::rngs::SysRng
            .try_fill_bytes(&mut nonce_bytes)
            .map_err(|_| CryptoError::EncryptFailed)?;
        self.encrypt_bytes_with_nonce(plaintext, aad, &nonce_bytes)
    }

    fn encrypt_bytes_with_nonce(
        &self,
        plaintext: &[u8],
        aad: &[u8],
        nonce_bytes: &[u8; NONCE_SIZE],
    ) -> Result<Vec<u8>, CryptoError> {
        let nonce = Nonce::from(*nonce_bytes);
        let ct = self
            .cipher
            .encrypt(
                &nonce,
                Payload {
                    msg: plaintext,
                    aad,
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

    pub fn decrypt_field(&self, blob: &[u8], aad: &str) -> Result<String, CryptoError> {
        String::from_utf8(self.decrypt_bytes(blob, aad.as_bytes())?)
            .map_err(|_| CryptoError::DecryptFailed)
    }

    pub fn decrypt_bytes(&self, blob: &[u8], aad: &[u8]) -> Result<Vec<u8>, CryptoError> {
        if blob.len() < 15 {
            return Err(CryptoError::InvalidPayload("blob too short".into()));
        }
        let version = blob[0];
        let kid_len = blob[1] as usize;
        if version != VERSION {
            return Err(CryptoError::InvalidPayload(format!(
                "unknown version: {version}"
            )));
        }
        let kid_end = 2 + kid_len;
        if blob.len() < kid_end + NONCE_SIZE {
            return Err(CryptoError::InvalidPayload(
                "blob truncated (missing nonce)".into(),
            ));
        }
        let kid = std::str::from_utf8(&blob[2..kid_end])
            .map_err(|_| CryptoError::InvalidPayload("invalid key id encoding".into()))?;
        if kid != self.kid {
            return Err(CryptoError::KeyMissing);
        }
        let nonce_end = kid_end + NONCE_SIZE;
        let nonce = <&Nonce<_>>::try_from(&blob[kid_end..nonce_end])
            .map_err(|_| CryptoError::DecryptFailed)?;
        let ct = &blob[nonce_end..];
        if ct.is_empty() {
            return Err(CryptoError::InvalidPayload(
                "blob truncated (missing ciphertext)".into(),
            ));
        }
        let pt = self
            .cipher
            .decrypt(nonce, Payload { msg: ct, aad })
            .map_err(|_| CryptoError::DecryptFailed)?;
        Ok(pt)
    }

    pub fn kid(&self) -> &str {
        &self.kid
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_KEY_HEX: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

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
        assert_eq!(blob[0], VERSION);
        assert_eq!(blob[1] as usize, KID.len());
        assert_eq!(&blob[2..2 + KID.len()], KID.as_bytes());
        let expected_len = 2 + KID.len() + NONCE_SIZE + 1 + 16;
        assert_eq!(blob.len(), expected_len);
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
        *blob.last_mut().unwrap() ^= 0xff;
        assert!(c.decrypt_field(&blob, &aad).is_err());
    }

    #[test]
    fn rejects_non_32_byte_key() {
        assert!(FieldCipher::from_hex_key("00112233", KID).is_err());
    }
}
