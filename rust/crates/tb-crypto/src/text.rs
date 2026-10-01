//! Textspalten nutzen dasselbe authentifizierte Feldformat wie BYTEA-Spalten.
//! Base64 allein wäre keine Verschlüsselung. Unmarkierter Klartext wird nie
//! still akzeptiert; Altbestände müssen ausdrücklich migriert werden.
use crate::FieldCipher;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use tb_error::CryptoError;

pub const PREFIX: &str = "enc:v1:";

pub fn encrypt(cipher: &FieldCipher, raw: &str, context: &str) -> Result<String, CryptoError> {
    Ok(format!(
        "{PREFIX}{}",
        URL_SAFE_NO_PAD.encode(cipher.encrypt_field(raw, context)?)
    ))
}

pub fn decrypt(cipher: &FieldCipher, encoded: &str, context: &str) -> Result<String, CryptoError> {
    let payload = encoded
        .strip_prefix(PREFIX)
        .ok_or(CryptoError::DecryptFailed)?;
    if payload.len() > 128 * 1024 {
        return Err(CryptoError::DecryptFailed);
    }
    let blob = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| CryptoError::DecryptFailed)?;
    cipher.decrypt_field(&blob, context)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn cipher() -> FieldCipher {
        FieldCipher::from_hex_key(&"11".repeat(32), "v1").unwrap()
    }
    #[test]
    fn account_and_field_are_authenticated_and_ciphertext_is_randomized() {
        let cipher = cipher();
        let raw = "synthetic-refresh-credential";
        let a = encrypt(&cipher, raw, "provider|account1|refresh|1").unwrap();
        let b = encrypt(&cipher, raw, "provider|account1|refresh|1").unwrap();
        assert_ne!(a, b);
        assert!(!a.contains(raw));
        assert_eq!(
            decrypt(&cipher, &a, "provider|account1|refresh|1").unwrap(),
            raw
        );
        assert!(decrypt(&cipher, &a, "provider|account2|refresh|1").is_err());
        assert!(decrypt(&cipher, &a, "provider|account1|access|1").is_err());
        assert!(decrypt(&cipher, raw, "provider|account1|refresh|1").is_err());
        let wrong = FieldCipher::from_hex_key(&"22".repeat(32), "v1").unwrap();
        assert!(decrypt(&wrong, &a, "provider|account1|refresh|1").is_err());
    }
}
