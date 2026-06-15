//! Kryptographisch sichere Zufalls-Token.
//!
//! Pendant zu Pythons `secrets.token_urlsafe(...)` (OS-CSPRNG). Ein Token aus
//! dieser Quelle ist der CSRF-Anker im OAuth-Flow — schwacher Zufall erlaubt
//! Token-Injection gegen fremde Konten. Deshalb hier zentral und ausschließlich
//! über `OsRng`, nie über selbstgebaute Mischfunktionen.

use rand::RngCore;

/// Erzeugt `n_bytes` OS-Zufall, hex-kodiert (`2 * n_bytes` Zeichen, URL-safe).
///
/// 16 Bytes entsprechen der Entropie von Pythons `secrets.token_urlsafe(16)`;
/// für OAuth-State-Tokens werden 32 Bytes verwendet.
pub fn random_hex_token(n_bytes: usize) -> String {
    use std::fmt::Write as _;
    let mut bytes = vec![0u8; n_bytes];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    let mut out = String::with_capacity(n_bytes * 2);
    for byte in bytes {
        write!(&mut out, "{byte:02x}").expect("write in String ist infallibel");
    }
    out
}

/// Byte-identisches Pendant zu Pythons `secrets.token_urlsafe(n_bytes)`:
/// `n_bytes` OS-Zufall, base64-urlsafe-kodiert OHNE `=`-Padding.
///
/// Quelle ist immer `OsRng` (OS-CSPRNG) — diese Tokens werden als Session-IDs
/// und CSRF-Anker verwendet; schwacher Zufall erlaubt Session-Übernahme. Die
/// Länge des Resultats ist `ceil(n_bytes * 4 / 3)` Zeichen (z. B. 43 für 32 Byte).
pub fn random_urlsafe_token(n_bytes: usize) -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    let mut bytes = vec![0u8; n_bytes];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(&bytes)
}

/// Konstant-zeitlicher Byte-Vergleich — Pendant zu Pythons `hmac.compare_digest`.
///
/// Verhindert Timing-Seitenkanäle beim Vergleich von Secrets/Tokens (CSRF,
/// Internal-Token, Session-IDs). Die Laufzeit hängt nur von der Länge ab, nicht
/// von der Position eines Mismatches. Ungleiche Längen → sofort `false`; die
/// Länge selbst ist kein Geheimnis (der Angreifer kennt die Token-Länge).
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn laenge_und_hex_alphabet() {
        let t = random_hex_token(32);
        assert_eq!(t.len(), 64);
        assert!(t.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn zwei_tokens_kollidieren_nicht() {
        assert_ne!(random_hex_token(32), random_hex_token(32));
    }

    #[test]
    fn urlsafe_token_laenge_und_alphabet() {
        // 32 Byte → 43 Zeichen base64-urlsafe ohne Padding (ceil(32*4/3)).
        let t = random_urlsafe_token(32);
        assert_eq!(t.len(), 43);
        assert!(!t.contains('='), "kein Padding wie secrets.token_urlsafe");
        assert!(
            t.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "nur url-safe-Alphabet"
        );
    }

    #[test]
    fn urlsafe_tokens_kollidieren_nicht() {
        assert_ne!(random_urlsafe_token(32), random_urlsafe_token(32));
    }

    #[test]
    fn constant_time_eq_gleiche_werte() {
        assert!(constant_time_eq(b"geheim-token", b"geheim-token"));
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn constant_time_eq_verschiedene_werte() {
        assert!(!constant_time_eq(b"geheim-token", b"geheim-tokeX"));
        assert!(!constant_time_eq(b"abc", b"abd"));
    }

    #[test]
    fn constant_time_eq_verschiedene_laengen() {
        assert!(!constant_time_eq(b"ab", b"abc"));
        assert!(!constant_time_eq(b"abc", b""));
    }
}
