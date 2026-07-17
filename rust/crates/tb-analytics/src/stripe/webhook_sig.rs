//! Verifikation von Stripe-Webhook-Signaturen (Ersatz für
//! `stripe.Webhook.construct_event`).
//!
//! Stripe signiert jeden Webhook mit HMAC-SHA256 über `"{timestamp}.{payload}"`
//! und liefert das Ergebnis im `Stripe-Signature`-Header als
//! `t=<unix>,v1=<hex>[,v1=<hex>...]`. Diese Funktion prüft (1) das Alter des
//! Timestamps gegen ein Toleranzfenster und (2) ob mindestens eine `v1`-Signatur
//! konstant-zeitlich gegen den erwarteten HMAC matcht.
//!
//! Das Webhook-Secret (`whsec_…`) wird ausschließlich für die HMAC-Berechnung
//! verwendet und niemals geloggt oder in Fehlern transportiert.

use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Stripes Standard-Toleranzfenster für Webhook-Timestamps (5 Minuten).
pub const DEFAULT_TOLERANCE_SECONDS: i64 = 300;

/// Fehlerursachen der Signatur-Verifikation. Tragen **keine** Geheimnisse.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum WebhookError {
    /// Header fehlt der `t=`-Timestamp oder ist strukturell unbrauchbar.
    #[error("invalid stripe signature header")]
    InvalidHeader,
    /// Header enthält keine `v1`-Signatur.
    #[error("no v1 signature in stripe header")]
    NoSignatures,
    /// Timestamp liegt außerhalb des erlaubten Toleranzfensters (Replay-Schutz).
    #[error("stripe signature timestamp outside tolerance")]
    TimestampOutsideTolerance,
    /// Keine der gelieferten Signaturen matcht den erwarteten HMAC.
    #[error("stripe signature mismatch")]
    NoMatch,
}

struct ParsedHeader {
    timestamp: i64,
    v1: Vec<String>,
}

fn parse_header(header: &str) -> Result<ParsedHeader, WebhookError> {
    let mut timestamp: Option<i64> = None;
    let mut v1: Vec<String> = Vec::new();
    for part in header.split(',') {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        match key.trim() {
            "t" => timestamp = value.trim().parse::<i64>().ok(),
            "v1" => {
                let sig = value.trim();
                if !sig.is_empty() {
                    v1.push(sig.to_string());
                }
            }
            _ => {}
        }
    }
    let timestamp = timestamp.ok_or(WebhookError::InvalidHeader)?;
    if v1.is_empty() {
        return Err(WebhookError::NoSignatures);
    }
    Ok(ParsedHeader { timestamp, v1 })
}

/// HMAC akzeptiert Schlüssel beliebiger Länge — `new_from_slice` kann für HMAC
/// nicht fehlschlagen (im Gegensatz zu Fixed-Key-MACs).
fn new_mac(secret: &[u8]) -> HmacSha256 {
    HmacSha256::new_from_slice(secret).expect("HMAC keys may be of any length")
}

/// Verifiziert eine Stripe-Webhook-Signatur.
///
/// * `payload` — exakter, **roher** Request-Body (Byte-für-Byte wie empfangen).
/// * `sig_header` — Inhalt des `Stripe-Signature`-Headers.
/// * `secret` — Webhook-Signing-Secret (`whsec_…`).
/// * `now_unix` — aktuelle Unix-Zeit in Sekunden (für Replay-Fenster).
/// * `tolerance_seconds` — maximales Alter; `<= 0` deaktiviert die Zeitprüfung.
///
/// Gibt bei Erfolg `Ok(timestamp)` zurück (der signierte Timestamp), sonst den
/// jeweiligen [`WebhookError`].
pub fn verify_signature(
    payload: &[u8],
    sig_header: &str,
    secret: &str,
    now_unix: i64,
    tolerance_seconds: i64,
) -> Result<i64, WebhookError> {
    let parsed = parse_header(sig_header)?;

    if tolerance_seconds > 0 && now_unix.saturating_sub(parsed.timestamp) > tolerance_seconds {
        return Err(WebhookError::TimestampOutsideTolerance);
    }

    // signed_payload = "{timestamp}.{payload}"
    let mut signed_payload = parsed.timestamp.to_string().into_bytes();
    signed_payload.push(b'.');
    signed_payload.extend_from_slice(payload);

    let secret_bytes = secret.as_bytes();
    for candidate in &parsed.v1 {
        let Ok(candidate_bytes) = hex::decode(candidate) else {
            continue;
        };
        let mut mac = new_mac(secret_bytes);
        mac.update(&signed_payload);
        // `verify_slice` vergleicht konstant-zeitlich.
        if mac.verify_slice(&candidate_bytes).is_ok() {
            return Ok(parsed.timestamp);
        }
    }
    Err(WebhookError::NoMatch)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Unabhängig per `openssl dgst -sha256 -hmac` erzeugter Referenzvektor
    // (Stripes kanonisches Doku-Beispiel-Payload).
    const SECRET: &str = "whsec_test_secret";
    const TIMESTAMP: i64 = 1492774577;
    const PAYLOAD: &str = r#"{"id": "evt_test_webhook", "object": "event"}"#;
    const V1: &str = "c137b1b62277d523cf8fed4dfbd0170a9a5b8a380e00cc3711d4bf0652f2ce7a";

    fn header() -> String {
        format!("t={TIMESTAMP},v1={V1}")
    }

    #[test]
    fn reference_vector_verifies_true() {
        // now == timestamp → Toleranz spielt keine Rolle.
        let result = verify_signature(
            PAYLOAD.as_bytes(),
            &header(),
            SECRET,
            TIMESTAMP,
            DEFAULT_TOLERANCE_SECONDS,
        );
        assert_eq!(result, Ok(TIMESTAMP));
    }

    #[test]
    fn tampered_payload_verifies_false() {
        let tampered = r#"{"id": "evt_test_webhook", "object": "event!"}"#;
        let result = verify_signature(
            tampered.as_bytes(),
            &header(),
            SECRET,
            TIMESTAMP,
            DEFAULT_TOLERANCE_SECONDS,
        );
        assert_eq!(result, Err(WebhookError::NoMatch));
    }

    #[test]
    fn wrong_secret_verifies_false() {
        let result = verify_signature(
            PAYLOAD.as_bytes(),
            &header(),
            "whsec_wrong_secret",
            TIMESTAMP,
            DEFAULT_TOLERANCE_SECONDS,
        );
        assert_eq!(result, Err(WebhookError::NoMatch));
    }

    #[test]
    fn stale_timestamp_outside_tolerance_is_rejected() {
        // now weit nach dem Timestamp → außerhalb des Fensters.
        let result = verify_signature(
            PAYLOAD.as_bytes(),
            &header(),
            SECRET,
            TIMESTAMP + DEFAULT_TOLERANCE_SECONDS + 1,
            DEFAULT_TOLERANCE_SECONDS,
        );
        assert_eq!(result, Err(WebhookError::TimestampOutsideTolerance));
    }

    #[test]
    fn disabled_tolerance_allows_old_timestamp() {
        let result = verify_signature(
            PAYLOAD.as_bytes(),
            &header(),
            SECRET,
            TIMESTAMP + 10_000_000,
            0,
        );
        assert_eq!(result, Ok(TIMESTAMP));
    }

    #[test]
    fn multiple_v1_signatures_match_when_any_is_valid() {
        let header = format!("t={TIMESTAMP},v1=deadbeef,v1={V1}");
        let result = verify_signature(
            PAYLOAD.as_bytes(),
            &header,
            SECRET,
            TIMESTAMP,
            DEFAULT_TOLERANCE_SECONDS,
        );
        assert_eq!(result, Ok(TIMESTAMP));
    }

    #[test]
    fn missing_timestamp_is_invalid_header() {
        let result = verify_signature(
            PAYLOAD.as_bytes(),
            &format!("v1={V1}"),
            SECRET,
            TIMESTAMP,
            DEFAULT_TOLERANCE_SECONDS,
        );
        assert_eq!(result, Err(WebhookError::InvalidHeader));
    }

    #[test]
    fn missing_v1_is_no_signatures() {
        let result = verify_signature(
            PAYLOAD.as_bytes(),
            &format!("t={TIMESTAMP}"),
            SECRET,
            TIMESTAMP,
            DEFAULT_TOLERANCE_SECONDS,
        );
        assert_eq!(result, Err(WebhookError::NoSignatures));
    }
}
