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
}
