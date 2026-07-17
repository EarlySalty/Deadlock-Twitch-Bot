//! HMAC-signierter Partner-Einmal-Login-Token (B3-8 / auth-core-4).
//!
//! Port von `bot/dashboard/auth/services.py:PartnerLoginTokenService`
//! (`_serialize_token`/`_deserialize_token`). Format auf der Leitung:
//!
//! ```text
//! base64url(payload_json).base64url(hmac_sha256(payload_b64))
//! ```
//!
//! - `payload_json`: kompaktes JSON (`separators=(",",":")`, `sort_keys=True`)
//!   mit den Feldern `{ v, sid, next, iat, exp }`.
//! - Signatur: HMAC-SHA256 über den **base64url-kodierten Payload** (nicht das
//!   rohe JSON), Key = `TWITCH_PARTNER_TOKEN` (Infisical/Env).
//! - base64url **ohne** `=`-Padding (URL_SAFE_NO_PAD), wie Python `rstrip("=")`.
//!
//! Einmaligkeit wird NICHT hier erzwungen, sondern beim Verbrauch über den
//! atomaren `DELETE … RETURNING` der State-Row (`consume_partner_login_state`).

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Token-Version (Python `_TOKEN_VERSION = 1`). Mismatch beim Verify → Fehler.
const TOKEN_VERSION: i64 = 1;

/// Default-TTL des Login-Tokens in Sekunden (Python `_token_ttl_seconds`,
/// Default 180, clamp 30..=600).
pub const PARTNER_LOGIN_TOKEN_TTL_SECS: u64 = 180;

/// Entschlüsselter Token-Payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PartnerLoginToken {
    /// Token-Version.
    pub v: i64,
    /// State-ID (Primärschlüssel der State-Row).
    pub sid: String,
    /// Normalisierter Redirect-Pfad nach dem Login.
    pub next: String,
    /// issued-at (epoch sec).
    pub iat: i64,
    /// expires-at (epoch sec).
    pub exp: i64,
}

/// Fehler bei Token-Verifikation.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TokenError {
    #[error("malformed token")]
    Malformed,
    #[error("bad signature")]
    BadSignature,
    #[error("unsupported version")]
    Version,
    #[error("expired or not yet valid")]
    Timing,
}

impl PartnerLoginToken {
    /// Erzeugt einen Token mit `sid`/`next` und TTL ab `now` (epoch sec).
    pub fn new(sid: String, next: String, now: i64, ttl_secs: u64) -> Self {
        Self {
            v: TOKEN_VERSION,
            sid,
            next,
            iat: now,
            exp: now + ttl_secs as i64,
        }
    }

    /// Serialisiert + signiert: `base64url(payload).base64url(sig)`.
    ///
    /// Kompaktes JSON mit sortierten Keys (serde_json::Value::Object ist eine
    /// BTreeMap → sortiert; `to_string` nutzt kompakte Separatoren) — bit-genau
    /// zu Pythons `json.dumps(..., separators=(",",":"), sort_keys=True)`.
    pub fn sign(&self, secret: &[u8]) -> String {
        // Über serde_json::Value serialisieren → Map sortiert die Keys (BTreeMap).
        let value = serde_json::json!({
            "v": self.v,
            "sid": self.sid,
            "next": self.next,
            "iat": self.iat,
            "exp": self.exp,
        });
        let payload_json = value.to_string();
        let payload_b64 = URL_SAFE_NO_PAD.encode(payload_json.as_bytes());
        let sig = hmac_sign(secret, payload_b64.as_bytes());
        let sig_b64 = URL_SAFE_NO_PAD.encode(sig);
        format!("{payload_b64}.{sig_b64}")
    }

    /// Verifiziert Signatur + Version + Zeitfenster und gibt den Payload zurück.
    pub fn verify(token: &str, secret: &[u8], now: i64) -> Result<Self, TokenError> {
        let (payload_b64, sig_b64) = token.split_once('.').ok_or(TokenError::Malformed)?;
        let expected = hmac_sign(secret, payload_b64.as_bytes());
        let presented = URL_SAFE_NO_PAD
            .decode(sig_b64)
            .map_err(|_| TokenError::Malformed)?;
        // Konstant-zeitlicher Vergleich.
        if !tb_crypto::constant_time_eq(&expected, &presented) {
            return Err(TokenError::BadSignature);
        }
        let payload_bytes = URL_SAFE_NO_PAD
            .decode(payload_b64)
            .map_err(|_| TokenError::Malformed)?;
        let parsed: PartnerLoginToken =
            serde_json::from_slice(&payload_bytes).map_err(|_| TokenError::Malformed)?;
        if parsed.v != TOKEN_VERSION {
            return Err(TokenError::Version);
        }
        if parsed.sid.is_empty() || parsed.iat <= 0 || parsed.exp <= parsed.iat || parsed.exp <= now
        {
            return Err(TokenError::Timing);
        }
        Ok(parsed)
    }
}

fn hmac_sign(secret: &[u8], message: &[u8]) -> Vec<u8> {
    // HMAC akzeptiert beliebige Key-Längen — `new_from_slice` kann nicht failen.
    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC akzeptiert jede Key-Länge");
    mac.update(message);
    mac.finalize().into_bytes().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &[u8] = b"super-secret-partner-token";

    #[test]
    fn roundtrip_sign_verify() {
        let tok = PartnerLoginToken::new("state123".into(), "/analyse".into(), 1000, 180);
        let wire = tok.sign(SECRET);
        assert!(wire.contains('.'));
        let back = PartnerLoginToken::verify(&wire, SECRET, 1100).unwrap();
        assert_eq!(back, tok);
        assert_eq!(back.next, "/analyse");
    }

    #[test]
    fn falscher_secret_schlaegt_fehl() {
        let wire = PartnerLoginToken::new("s".into(), "/x".into(), 1000, 180).sign(SECRET);
        assert_eq!(
            PartnerLoginToken::verify(&wire, b"other", 1100).unwrap_err(),
            TokenError::BadSignature
        );
    }

    #[test]
    fn abgelaufen_schlaegt_fehl() {
        let wire = PartnerLoginToken::new("s".into(), "/x".into(), 1000, 180).sign(SECRET);
        // now > exp (1000+180=1180).
        assert_eq!(
            PartnerLoginToken::verify(&wire, SECRET, 2000).unwrap_err(),
            TokenError::Timing
        );
    }

    #[test]
    fn manipulierter_payload_schlaegt_fehl() {
        let wire = PartnerLoginToken::new("s".into(), "/x".into(), 1000, 180).sign(SECRET);
        let (_p, sig) = wire.split_once('.').unwrap();
        // Anderen Payload mit der alten Signatur kombinieren → BadSignature.
        let forged_payload =
            URL_SAFE_NO_PAD.encode(br#"{"v":1,"sid":"evil","next":"/x","iat":1000,"exp":1180}"#);
        let forged = format!("{forged_payload}.{sig}");
        assert_eq!(
            PartnerLoginToken::verify(&forged, SECRET, 1100).unwrap_err(),
            TokenError::BadSignature
        );
    }

    #[test]
    fn kaputtes_format_malformed() {
        assert_eq!(
            PartnerLoginToken::verify("no-dot-here", SECRET, 1100).unwrap_err(),
            TokenError::Malformed
        );
    }
}
