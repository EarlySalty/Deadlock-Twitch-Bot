//! Fernet-kompatible Ver- und Entschlüsselung für Python `cryptography.fernet`.
//!
//! Python-Referenz: `bot/storage/sessions_db.py` (`_encrypt`/`_decrypt`).
//! Kanonische Key-Quelle ist die Env-Var `SESSIONS_ENCRYPTION_KEY` (Infisical,
//! base64-urlsafe wie `Fernet.generate_key()`) — Python liest sie seit dem
//! Linux-Fix in `_load_or_create_key()` ebenfalls zuerst; der Windows-Keyring
//! ist nur noch Alt-Fallback. `encrypt()` wird für den Sliding-Session-Refresh
//! gebraucht (Payload muss mit aktualisiertem `expires_at` neu verschlüsselt
//! werden, Python-Pendant: `services.py` Session-Refresh).
//!
//! # Fernet-Token-Format (https://github.com/fernet/spec/blob/master/Spec.md)
//! ```text
//! base64url( version(1) || timestamp_be_u64(8) || iv(16) || ciphertext(n) || hmac_sha256(32) )
//! ```
//! - `version`: immer `0x80`
//! - `timestamp`: Unix-Sekunden, Big-Endian uint64
//! - `iv`: AES-128-CBC-IV (16 Bytes)
//! - `ciphertext`: PKCS#7-padded AES-128-CBC
//! - `hmac`: HMAC-SHA256 über `version || timestamp || iv || ciphertext`
//!
//! Key-Split (32 Bytes):
//! - Bytes 0..16 = HMAC-Signing-Key
//! - Bytes 16..32 = AES-128-CBC-Verschlüsselungs-Key
//!
//! TTL-Check: wird **nicht** hier durchgeführt — TTL-Prüfung erfolgt über
//! `expires_at`-Feld in der DB-Session (Python: `services.py:213-220`).
//! Fernet-interner Timestamp ist redundant mit dem DB-`expires_at`.

use std::time::{SystemTime, UNIX_EPOCH};

use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use hmac::{Hmac, Mac};
use sha2::Sha256;

type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;
type Aes128CbcEnc = cbc::Encryptor<aes::Aes128>;
type HmacSha256 = Hmac<Sha256>;

/// Fernet-Fehler.
#[derive(Debug, thiserror::Error)]
pub enum FernetError {
    #[error("base64-Dekodierung fehlgeschlagen")]
    Base64,
    #[error("Token zu kurz (mind. 57 Bytes)")]
    TooShort,
    #[error("Ungültige Fernet-Version (erwartet 0x80)")]
    BadVersion,
    #[error("HMAC-Verifikation fehlgeschlagen")]
    HmacMismatch,
    #[error("AES-Entschlüsselung fehlgeschlagen (Padding-Fehler)")]
    AesDecrypt,
    #[error("Token abgelaufen (TTL überschritten)")]
    Expired,
    #[error("Key-Länge ungültig (erwartet 32 Bytes nach base64-Dekodierung)")]
    BadKeyLength,
}

/// Aufgeteilter Fernet-Key: erste 16 Bytes signieren (HMAC), letzte 16 verschlüsseln (AES).
struct FernetKey {
    signing: [u8; 16],
    encryption: [u8; 16],
}

/// Dekodiert den base64-urlsafe-Key und teilt ihn in Signing-/Encryption-Hälfte.
fn parse_key(key_b64: &str) -> Result<FernetKey, FernetError> {
    let key_bytes = base64_urlsafe_decode(key_b64).map_err(|_| FernetError::Base64)?;
    if key_bytes.len() != 32 {
        return Err(FernetError::BadKeyLength);
    }
    Ok(FernetKey {
        signing: key_bytes[..16].try_into().unwrap(),
        encryption: key_bytes[16..32].try_into().unwrap(),
    })
}

/// Verschlüsselt `plaintext` zu einem Fernet-Token kompatibel zu Python
/// `cryptography.fernet` (Python kann das Ergebnis 1:1 entschlüsseln).
///
/// Gibt den base64-urlsafe-kodierten Token-String zurück (mit `=`-Padding,
/// wie Python ihn erzeugt und beim Dekodieren auch verlangt).
pub fn encrypt(key_b64: &str, plaintext: &[u8]) -> Result<String, FernetError> {
    use base64::{engine::general_purpose::URL_SAFE, Engine};

    let key = parse_key(key_b64)?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let iv: [u8; 16] = rand::random();

    // PKCS#7-Padding: immer mindestens 1 Padding-Byte, auf 16er-Block aufgefüllt
    let padded_len = (plaintext.len() / 16 + 1) * 16;
    let mut buf = vec![0u8; padded_len];
    buf[..plaintext.len()].copy_from_slice(plaintext);
    let ct_len = Aes128CbcEnc::new(&key.encryption.into(), &iv.into())
        .encrypt_padded_mut::<Pkcs7>(&mut buf, plaintext.len())
        .map_err(|_| FernetError::AesDecrypt)?
        .len();
    buf.truncate(ct_len);

    // Token: version || timestamp_be || iv || ciphertext || hmac
    let mut raw = Vec::with_capacity(1 + 8 + 16 + buf.len() + 32);
    raw.push(0x80);
    raw.extend_from_slice(&timestamp.to_be_bytes());
    raw.extend_from_slice(&iv);
    raw.extend_from_slice(&buf);

    let mut mac =
        <HmacSha256 as Mac>::new_from_slice(&key.signing).expect("HMAC-Key immer gültig");
    mac.update(&raw);
    raw.extend_from_slice(&mac.finalize().into_bytes());

    Ok(URL_SAFE.encode(&raw))
}

/// Entschlüsselt ein Fernet-Token kompatibel zu Python `cryptography.fernet`.
///
/// `key_b64` ist der base64-urlsafe-kodierte 32-Byte-Key (wie `Fernet.generate_key()`).
/// `token` ist der base64-urlsafe-kodierte Fernet-Token-String.
/// `ttl_secs`: wenn `Some(n)`, wird der Token abgelehnt wenn älter als `n` Sekunden.
///
/// Gibt den entschlüsselten Klartext zurück.
pub fn decrypt(key_b64: &str, token: &str, ttl_secs: Option<u64>) -> Result<Vec<u8>, FernetError> {
    // 1. Key dekodieren
    let key = parse_key(key_b64)?;
    let signing_key = &key.signing;
    let encryption_key = key.encryption;

    // 2. Token dekodieren
    let raw = base64_urlsafe_decode(token).map_err(|_| FernetError::Base64)?;
    // Minimum: 1 (version) + 8 (ts) + 16 (iv) + 16 (min 1 PKCS7-Block) + 32 (hmac) = 73
    // Spec sagt >= 57, wir erzwingen 57 als untere Grenze
    if raw.len() < 57 {
        return Err(FernetError::TooShort);
    }

    // 3. Version prüfen
    if raw[0] != 0x80 {
        return Err(FernetError::BadVersion);
    }

    // 4. Timestamp lesen
    let timestamp_bytes: [u8; 8] = raw[1..9].try_into().unwrap();
    let timestamp = u64::from_be_bytes(timestamp_bytes);

    // 5. TTL prüfen (optional)
    if let Some(ttl) = ttl_secs {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if now.saturating_sub(timestamp) > ttl {
            return Err(FernetError::Expired);
        }
    }

    // 6. HMAC prüfen: über alles außer dem letzten 32-Byte-HMAC
    let hmac_offset = raw.len() - 32;
    let signed_data = &raw[..hmac_offset];
    let presented_hmac = &raw[hmac_offset..];

    let mut mac =
        <HmacSha256 as Mac>::new_from_slice(signing_key).expect("HMAC-Key immer gültig");
    mac.update(signed_data);
    mac.verify_slice(presented_hmac)
        .map_err(|_| FernetError::HmacMismatch)?;

    // 7. IV + Ciphertext extrahieren
    let iv: [u8; 16] = raw[9..25].try_into().unwrap();
    let ciphertext = &raw[25..hmac_offset];

    // 8. AES-128-CBC entschlüsseln
    let mut buf = ciphertext.to_vec();
    let plaintext_len = Aes128CbcDec::new(&encryption_key.into(), &iv.into())
        .decrypt_padded_mut::<Pkcs7>(&mut buf)
        .map_err(|_| FernetError::AesDecrypt)?
        .len();
    buf.truncate(plaintext_len);

    Ok(buf)
}

/// base64-urlsafe dekodieren (mit oder ohne Padding).
fn base64_urlsafe_decode(s: &str) -> Result<Vec<u8>, base64::DecodeError> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    // Padding entfernen falls vorhanden, dann ohne Padding dekodieren
    let stripped = s.trim_end_matches('=');
    URL_SAFE_NO_PAD.decode(stripped)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Testvektoren erzeugt via:
    // python3 -c "
    //   from cryptography.fernet import Fernet
    //   import base64
    //   key = b'dGVzdGtleTEyMzQ1Njc4OTAxMjM0NTY3ODkwMTIzNDU='
    //   f = Fernet(key)
    //   print(f.encrypt(b'session_data_test_123').decode())
    //   print(f.encrypt(b'hello').decode())
    // "
    // Key: dGVzdGtleTEyMzQ1Njc4OTAxMjM0NTY3ODkwMTIzNDU= (32 Bytes: testkey1234567890...345)

    const TEST_KEY: &str = "dGVzdGtleTEyMzQ1Njc4OTAxMjM0NTY3ODkwMTIzNDU=";

    /// Fester Token erzeugt via Python, Inhalt: b"session_data_test_123"
    /// (timestamp ~2025, kein TTL-Check hier)
    const TOKEN_SESSION: &str =
        "gAAAAABqK34poFELKqRkWAZ9mO4R2M8iOHLtVy0q0TlcowBWnPrV1neVPLUL6EFzj84fmJkQto_60RJ1jlsAwqGjO7qzVxDl86H5Ja8aCZq_0kJf4aEr1lY=";

    /// Fester Token: b"hello"
    const TOKEN_HELLO: &str =
        "gAAAAABqK34ji17kn2i5NvZ56azRH7L91qSYmV1HOWtyUrAVfVX5i0KiOMlcxxSpUvnQjXs2TqROsyfmuvcyQQFTkNy37K4fjA==";

    #[test]
    fn python_token_session_data_entschluesseln() {
        let result = decrypt(TEST_KEY, TOKEN_SESSION, None).expect("Entschlüsselung muss klappen");
        assert_eq!(result, b"session_data_test_123");
    }

    #[test]
    fn python_token_hello_entschluesseln() {
        let result = decrypt(TEST_KEY, TOKEN_HELLO, None).expect("Entschlüsselung muss klappen");
        assert_eq!(result, b"hello");
    }

    #[test]
    fn falscher_key_gibt_hmac_fehler() {
        let wrong_key = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
        let err = decrypt(wrong_key, TOKEN_HELLO, None).unwrap_err();
        assert!(matches!(err, FernetError::HmacMismatch));
    }

    #[test]
    fn manipulierter_token_gibt_hmac_fehler() {
        // Letztes Byte des Tokens ändern → HMAC-Mismatch
        let mut raw = base64_urlsafe_decode(TOKEN_HELLO.trim_end_matches('=')).unwrap();
        let last = raw.last_mut().unwrap();
        *last ^= 0xFF;
        let tampered = {
            use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
            URL_SAFE_NO_PAD.encode(&raw)
        };
        let err = decrypt(TEST_KEY, &tampered, None).unwrap_err();
        assert!(matches!(err, FernetError::HmacMismatch));
    }

    #[test]
    fn token_zu_kurz_gibt_fehler() {
        let err = decrypt(TEST_KEY, "gAAAAA==", None).unwrap_err();
        assert!(matches!(err, FernetError::TooShort));
    }

    #[test]
    fn key_laenge_falsch_gibt_fehler() {
        // 16-Byte-Key (zu kurz)
        let short_key = "dGVzdGtleTEyMzQ="; // 16 Bytes
        let err = decrypt(short_key, TOKEN_HELLO, None).unwrap_err();
        assert!(matches!(err, FernetError::BadKeyLength));
    }

    #[test]
    fn ttl_abgelaufen_gibt_fehler() {
        // TTL von 1 Sekunde, Token von 2025 → abgelaufen
        let err = decrypt(TEST_KEY, TOKEN_HELLO, Some(1)).unwrap_err();
        assert!(matches!(err, FernetError::Expired));
    }

    #[test]
    fn ttl_none_ignoriert_timestamp() {
        // Kein TTL → kein Expired-Fehler auch für alten Token
        let result = decrypt(TEST_KEY, TOKEN_HELLO, None);
        assert!(result.is_ok());
    }

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let plaintexts: &[&[u8]] = &[
            b"",
            b"x",
            b"genau-sechzehn-b",
            br#"{"twitch_login":"testuser","expires_at":9999999999.0}"#,
        ];
        for pt in plaintexts {
            let token = encrypt(TEST_KEY, pt).expect("encrypt muss klappen");
            let back = decrypt(TEST_KEY, &token, None).expect("decrypt muss klappen");
            assert_eq!(&back, pt);
        }
    }

    #[test]
    fn encrypt_mit_falschem_key_nicht_entschluesselbar() {
        let token = encrypt(TEST_KEY, b"geheim").unwrap();
        let err = decrypt("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=", &token, None).unwrap_err();
        assert!(matches!(err, FernetError::HmacMismatch));
    }

    /// Goldstandard-Interop: Rust-encrypt → Python `cryptography.fernet` decrypt.
    /// Läuft nur wenn python3 + cryptography verfügbar sind (auf dem Bot-Host immer).
    #[test]
    fn rust_encrypt_python_decrypt_interop() {
        let token = encrypt(TEST_KEY, b"rust-zu-python-interop").unwrap();
        let script = format!(
            "from cryptography.fernet import Fernet\n\
             print(Fernet(b'{TEST_KEY}').decrypt(b'{token}').decode(), end='')"
        );
        let out = match std::process::Command::new("python3").arg("-c").arg(&script).output() {
            Ok(o) => o,
            Err(_) => return, // kein python3 → Test übersprungen
        };
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            if stderr.contains("ModuleNotFoundError") {
                return; // cryptography fehlt → übersprungen
            }
            panic!("Python-Decrypt fehlgeschlagen: {stderr}");
        }
        assert_eq!(out.stdout, b"rust-zu-python-interop");
    }

    #[test]
    fn json_payload_python_roundtrip() {
        // Token erzeugt via Python für JSON-Payload
        // python3 -c "
        //   from cryptography.fernet import Fernet
        //   import json, time
        //   key = b'dGVzdGtleTEyMzQ1Njc4OTAxMjM0NTY3ODkwMTIzNDU='
        //   f = Fernet(key)
        //   payload = json.dumps({'twitch_login': 'testuser', 'expires_at': 9999999999.0})
        //   print(f.encrypt(payload.encode()).decode())
        // "
        const JSON_TOKEN: &str = "gAAAAABqK38fqbEmIzhS9kQSi9jD0KF-FELkYjcon3hQr4KFXhxPzodR-l7l1YOT6eP4KznLWyL9Gw_lovSBJo6A24XavZNYAJ4tFHo95s1ToarvSVmh1oWjoml3vsA7V06DtP5ExhV1QPdfIR_3jqxJXKxMdyHAzQ==";
        let raw = decrypt(TEST_KEY, JSON_TOKEN, None).expect("JSON-Token muss entschlüsselbar sein");
        let s = std::str::from_utf8(&raw).unwrap();
        let v: serde_json::Value = serde_json::from_str(s).unwrap();
        assert_eq!(v["twitch_login"], "testuser");
        assert!((v["expires_at"].as_f64().unwrap() - 9999999999.0).abs() < 1.0);
    }
}
