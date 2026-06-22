//! Kleine crate-interne Helfer.

use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, NaiveDateTime, Utc};

/// Aktuelle Unix-Zeit in Sekunden (f64, wie Pythons `time.time()`).
pub(crate) fn unix_now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

/// Parst einen ISO-Timestamp (toleriert `Z`-Suffix); naive Timestamps gelten
/// als UTC. `None` bei leerem/kaputtem Wert. Für die TEXT-Timestamp-Spalten
/// (`twitch_token_blacklist` u. a.).
pub fn parse_iso_utc(raw: &str) -> Option<DateTime<Utc>> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    // RFC3339 verlangt `T` als Trenner und `+HH:MM` als Offset. Python-ISO und
    // der Postgres-`timestamptz::text`-Render (`2026-06-15 18:00:00+00`) weichen
    // ab: Leerzeichen statt `T`, Offset ohne Minuten. Beides hier normalisieren,
    // damit der robuste RFC3339-Parser greift (P2.38: TEXT/TIMESTAMPTZ-tolerant).
    let normalized = normalize_for_rfc3339(raw);
    if let Ok(dt) = DateTime::parse_from_rfc3339(&normalized) {
        return Some(dt.with_timezone(&Utc));
    }
    for fmt in [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S%.f%:z",
        "%Y-%m-%d %H:%M:%S%.f%:z",
    ] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(raw, fmt) {
            return Some(DateTime::from_naive_utc_and_offset(naive, Utc));
        }
        if let Ok(dt) = DateTime::parse_from_str(raw, fmt) {
            return Some(dt.with_timezone(&Utc));
        }
    }
    None
}

/// Normalisiert einen ISO-/Postgres-Timestamp auf strenges RFC3339:
/// `Z` → `+00:00`, Leerzeichen-Trenner → `T`, kurzer Offset `+HH`/`-HH` (ohne
/// Minuten, wie `timestamptz::text` ihn rendert) → `+HH:00`.
fn normalize_for_rfc3339(raw: &str) -> String {
    let mut s = raw.replace('Z', "+00:00");
    // Leerzeichen-Trenner zwischen Datum und Zeit zu `T`.
    if let Some(idx) = s.find(' ') {
        // Nur den ersten Trenner ersetzen (Datum<sp>Zeit).
        s.replace_range(idx..=idx, "T");
    }
    // Kurzen Offset `+HH`/`-HH` am Ende auf `+HH:00` erweitern.
    let bytes = s.as_bytes();
    let len = bytes.len();
    if len >= 3 {
        let sign = bytes[len - 3];
        let has_short_offset = (sign == b'+' || sign == b'-')
            && bytes[len - 2].is_ascii_digit()
            && bytes[len - 1].is_ascii_digit();
        if has_short_offset {
            s.push_str(":00");
        }
    }
    s
}

/// Maskiert eine Kennung fürs Logging (Python `_mask_log_identifier`): erste und
/// letzte 2 Zeichen, Mitte als `…`. Verhindert volle ID-Disclosure im Log; sehr
/// kurze IDs (≤ 4 Zeichen) werden komplett zu `…`.
pub(crate) fn mask_log_identifier(identifier: &str) -> String {
    let chars: Vec<char> = identifier.chars().collect();
    if chars.len() <= 4 {
        return "…".to_string();
    }
    let head: String = chars.iter().take(2).collect();
    let tail: String = chars
        .iter()
        .rev()
        .take(2)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{head}…{tail}")
}

#[cfg(test)]
mod tests {
    use super::{mask_log_identifier, parse_iso_utc};

    #[test]
    fn maskiert_lang_und_schuetzt_kurz() {
        assert_eq!(mask_log_identifier("123456789"), "12…89");
        assert_eq!(mask_log_identifier("ab"), "…");
    }

    #[test]
    fn parse_iso_utc_rfc3339_und_z() {
        let a = parse_iso_utc("2026-06-15T18:00:00+00:00").unwrap();
        let b = parse_iso_utc("2026-06-15T18:00:00Z").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn parse_iso_utc_naiv_gilt_als_utc() {
        let dt = parse_iso_utc("2026-06-15T18:00:00").unwrap();
        assert_eq!(dt.to_rfc3339(), "2026-06-15T18:00:00+00:00");
    }

    /// P2.38: Postgres rendert `timestamptz::text` als `2026-06-15 18:00:00+00`
    /// (Leerzeichen-Trenner, Offset ohne Minuten). Muss parsebar sein.
    #[test]
    fn parse_iso_utc_postgres_timestamptz_text_render() {
        let dt = parse_iso_utc("2026-06-15 18:00:00+00").unwrap();
        assert_eq!(dt.to_rfc3339(), "2026-06-15T18:00:00+00:00");
    }

    #[test]
    fn parse_iso_utc_postgres_text_mit_offset_und_subsekunden() {
        let dt = parse_iso_utc("2026-06-15 20:30:00.5+02").unwrap();
        // +02 → UTC 18:30:00.5
        assert_eq!(dt.to_rfc3339(), "2026-06-15T18:30:00.500+00:00");
    }

    #[test]
    fn parse_iso_utc_leer_und_kaputt() {
        assert!(parse_iso_utc("").is_none());
        assert!(parse_iso_utc("   ").is_none());
        assert!(parse_iso_utc("not-a-date").is_none());
    }
}
