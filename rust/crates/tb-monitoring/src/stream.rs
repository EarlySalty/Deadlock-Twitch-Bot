//! Domänen-Sicht auf einen laufenden Twitch-Stream (Helix-Payload) plus
//! Zeit-Helfer. tb-monitoring besitzt sein eigenes Modell — der Transport
//! (Helix-Client) mappt darauf, nicht umgekehrt.

use chrono::{DateTime, NaiveDateTime, SecondsFormat, Utc};

/// Ein Stream, wie ihn der Poll-Loop bzw. EventSub sieht.
#[derive(Debug, Clone, Default)]
pub struct StreamSnapshot {
    /// Helix-Stream-ID (pro Broadcast eindeutig).
    pub id: Option<String>,
    pub user_login: String,
    /// Helix `user_id` des Broadcasters (für Partner-Recruiting-Outreach-Send).
    pub user_id: String,
    /// Anzeigename (Helix `user_name`) — für Embeds/Offline-Posting.
    pub user_name: String,
    pub title: String,
    pub game_name: String,
    pub language: String,
    pub viewer_count: i32,
    pub is_mature: bool,
    pub tags: Vec<String>,
    /// Stream-Start laut Helix (RFC3339).
    pub started_at: Option<String>,
    /// Vorschaubild-Template laut Helix (`{width}`/`{height}`-Platzhalter).
    pub thumbnail_url: Option<String>,
    /// Optionales Kanalprofilbild fuer das kleine Embed-Thumbnail.
    /// Der Poll-Pfad ruft aus Python-Paritaet kein `/users` auf und liefert
    /// hier `None`; andere Pfade koennen ein bereits vorliegendes Profilbild setzen.
    pub profile_image_url: Option<String>,
}

impl StreamSnapshot {
    /// Läuft der Stream in der Ziel-Kategorie?
    /// (Python: `_stream_is_in_target_category` — Name-Vergleich, lowercase.)
    pub fn is_in_target_category(&self, target_game_lower: &str) -> bool {
        if target_game_lower.is_empty() {
            return false;
        }
        self.game_name.trim().to_lowercase() == target_game_lower
    }

    /// Tags als kompaktes JSON-Array (Python: `_normalize_stream_meta`);
    /// `None`, wenn keine nicht-leeren Tags vorhanden sind.
    pub fn tags_json(&self) -> Option<String> {
        let clean: Vec<&str> = self
            .tags
            .iter()
            .map(|t| t.trim())
            .filter(|t| !t.is_empty())
            .collect();
        if clean.is_empty() {
            None
        } else {
            serde_json::to_string(&clean).ok()
        }
    }

    /// Komma-Liste der Tags fürs Session-Feld (Python: `",".join(tags)`).
    pub fn tags_joined(&self) -> String {
        self.tags.join(",")
    }

    /// Spielname getrimmt, leer → `None`.
    pub fn game_name_opt(&self) -> Option<String> {
        let g = self.game_name.trim();
        (!g.is_empty()).then(|| g.to_string())
    }

    /// Titel getrimmt, leer → `None`.
    pub fn title_opt(&self) -> Option<String> {
        let t = self.title.trim();
        (!t.is_empty()).then(|| t.to_string())
    }
}

/// ISO-Timestamp mit Sekunden-Präzision und `+00:00`-Suffix — byte-kompatibel
/// zu Pythons `datetime.now(UTC).isoformat(timespec="seconds")`. Wird für die
/// TEXT-Timestamp-Spalten (`twitch_live_state`, `exp_*`) verwendet.
pub fn iso_seconds(dt: DateTime<Utc>) -> String {
    dt.to_rfc3339_opts(SecondsFormat::Secs, false)
}

/// Toleranter ISO-Parser (Python `_parse_dt`): `Z` → `+00:00`,
/// naive Timestamps gelten als UTC. Liefert `None` statt Fehler.
pub fn parse_dt_utc(raw: &str) -> Option<DateTime<Utc>> {
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

/// Stream-Start bestimmen (Python `_extract_stream_start`): Helix-`started_at`,
/// sonst der letzte bekannte Start aus dem Live-State.
pub fn extract_stream_start(
    stream_started_at: Option<&str>,
    previous_started_at: Option<&str>,
) -> Option<DateTime<Utc>> {
    stream_started_at
        .and_then(parse_dt_utc)
        .or_else(|| previous_started_at.and_then(parse_dt_utc))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_seconds_entspricht_python_isoformat() {
        let dt = DateTime::parse_from_rfc3339("2026-06-09T18:00:05.789Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(iso_seconds(dt), "2026-06-09T18:00:05+00:00");
    }

    #[test]
    fn parse_dt_utc_versteht_z_offset_und_naive() {
        assert!(parse_dt_utc("2026-06-09T18:00:00Z").is_some());
        assert!(parse_dt_utc("2026-06-09T18:00:00+00:00").is_some());
        assert!(parse_dt_utc("2026-06-09T18:00:00").is_some());
        assert!(parse_dt_utc("2026-06-09 18:00:00").is_some());
        assert!(parse_dt_utc("").is_none());
        assert!(parse_dt_utc("kein-datum").is_none());
    }

    #[test]
    fn kategorie_check_und_tags() {
        let stream = StreamSnapshot {
            game_name: " Deadlock ".to_string(),
            tags: vec!["DE ".to_string(), "".to_string(), "chill".to_string()],
            ..Default::default()
        };
        assert!(stream.is_in_target_category("deadlock"));
        assert!(!stream.is_in_target_category(""));
        assert_eq!(stream.tags_json().as_deref(), Some(r#"["DE","chill"]"#));
    }
}
