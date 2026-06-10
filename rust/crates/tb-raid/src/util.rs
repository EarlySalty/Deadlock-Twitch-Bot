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
pub(crate) fn parse_iso_utc(raw: &str) -> Option<DateTime<Utc>> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let normalized = raw.replace('Z', "+00:00");
    if let Ok(dt) = DateTime::parse_from_rfc3339(&normalized) {
        return Some(dt.with_timezone(&Utc));
    }
    for fmt in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M:%S%.f"] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(raw, fmt) {
            return Some(DateTime::from_naive_utc_and_offset(naive, Utc));
        }
    }
    None
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
    use super::mask_log_identifier;

    #[test]
    fn maskiert_lang_und_schuetzt_kurz() {
        assert_eq!(mask_log_identifier("123456789"), "12…89");
        assert_eq!(mask_log_identifier("ab"), "…");
    }
}
