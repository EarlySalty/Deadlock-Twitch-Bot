use std::sync::Arc;

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use tb_transport_twitch::{HelixClient, HelixError};

use super::model::ClipRecord;

/// Maximale Clips pro API-Seite (Helix-Limit).
const HELIX_PAGE_SIZE: u32 = 100;

/// Fetch-Fenster in Tagen. Muss zur 14-Tage-Retention des DB-Triggers
/// `social_media_set_retention_until` (schema.rs) passen: der Trigger setzt
/// `retention_until = created_at + 14 Tage` auf die Twitch-Erstellzeit —
/// ältere Clips wären beim INSERT sofort expired und der Retention-Worker
/// löscht sie im nächsten Lauf wieder (Fetch-Kreislauf).
const FETCH_WINDOW_DAYS: i64 = 14;

/// Baut die Query-Parameter für GET /clips — pur, damit das Zeitfenster
/// ohne HTTP testbar ist.
fn clip_query_params(
    broadcaster_id: &str,
    per_page: u32,
    cursor: Option<&str>,
    now: DateTime<Utc>,
) -> Vec<(String, String)> {
    let started_at =
        (now - Duration::days(FETCH_WINDOW_DAYS)).to_rfc3339_opts(SecondsFormat::Secs, true);
    let ended_at = now.to_rfc3339_opts(SecondsFormat::Secs, true);

    let mut params = vec![
        ("broadcaster_id".to_string(), broadcaster_id.to_string()),
        ("first".to_string(), per_page.to_string()),
        ("started_at".to_string(), started_at),
        ("ended_at".to_string(), ended_at),
    ];
    if let Some(c) = cursor {
        params.push(("after".to_string(), c.to_string()));
    }
    params
}

/// Eine Seite Clips aus der Helix-API mit optionalem Pagination-Cursor.
#[derive(Debug)]
pub struct ClipPage {
    pub clips: Vec<ClipRecord>,
    pub next_cursor: Option<String>,
}

/// Fetcht Clips über die Twitch Helix-API.
///
/// Kapselt alle HTTP-Kommunikation — der Rest des Crates kennt kein reqwest.
#[derive(Clone)]
pub struct HelixClipSource {
    client: Arc<HelixClient>,
}

impl HelixClipSource {
    pub fn new(client: Arc<HelixClient>) -> Self {
        Self { client }
    }

    /// Holt die Twitch-User-ID für einen Login-Namen.
    pub async fn fetch_user_id(&self, login: &str) -> Result<Option<String>, HelixError> {
        let users = self.client.get_users(&[login]).await?;
        Ok(users.get(&login.to_lowercase()).map(|u| u.id.clone()))
    }

    /// Fetcht eine Seite Clips für einen Broadcaster im 14-Tage-Fenster.
    pub async fn fetch_clips_page(
        &self,
        broadcaster_id: &str,
        limit: u32,
        cursor: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<ClipPage, HelixError> {
        let per_page = HELIX_PAGE_SIZE.min(limit);
        let params = clip_query_params(broadcaster_id, per_page, cursor, now);
        let req = self.client.get("/clips").await?.query(&params);

        let resp: serde_json::Value = req.send().await?.json().await?;

        let raw_clips = resp
            .get("data")
            .and_then(|d| d.as_array())
            .cloned()
            .unwrap_or_default();

        let next_cursor = resp
            .pointer("/pagination/cursor")
            .and_then(|v| v.as_str())
            .map(str::to_string);

        let clips = raw_clips
            .iter()
            .filter_map(|c| parse_clip(c, broadcaster_id))
            .collect();

        Ok(ClipPage { clips, next_cursor })
    }

    /// Fetcht alle Clips bis zum angegebenen Limit (paginiert automatisch).
    pub async fn fetch_clips(
        &self,
        broadcaster_id: &str,
        streamer_login: &str,
        limit: u32,
    ) -> Result<Vec<ClipRecord>, HelixError> {
        let mut all = Vec::new();
        let mut cursor: Option<String> = None;
        // Ein Zeitstempel für den ganzen Lauf, damit alle Seiten dasselbe
        // Fenster sehen und die Pagination konsistent bleibt.
        let now = Utc::now();

        loop {
            let remaining = limit.saturating_sub(all.len() as u32);
            if remaining == 0 {
                break;
            }

            let page = self
                .fetch_clips_page(broadcaster_id, remaining, cursor.as_deref(), now)
                .await?;

            if page.clips.is_empty() {
                break;
            }

            // Streamer-Login im Clip setzen — die Helix-Antwort enthält nur den
            // broadcaster_id, nicht den Login. Wir kennen ihn aus dem Kontext.
            all.extend(page.clips.into_iter().map(|mut c| {
                c.streamer_login = streamer_login.to_string();
                c
            }));

            cursor = page.next_cursor;
            if cursor.is_none() {
                break;
            }
        }

        Ok(all)
    }
}

fn parse_clip(v: &serde_json::Value, broadcaster_id: &str) -> Option<ClipRecord> {
    let clip_id = v.get("id")?.as_str()?.to_string();
    let clip_url = v.get("url")?.as_str()?.to_string();
    let clip_title = v
        .get("title")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string();
    let thumbnail_url = v
        .get("thumbnail_url")
        .and_then(|t| t.as_str())
        .map(str::to_string);
    let created_at = v
        .get("created_at")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string();
    let duration_seconds = v
        .get("duration")
        .and_then(|d| d.as_f64())
        .unwrap_or(0.0);
    let view_count = v
        .get("view_count")
        .and_then(|vc| vc.as_i64())
        .unwrap_or(0);
    let game_name = v
        .get("game_name")
        .and_then(|g| g.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    Some(ClipRecord {
        clip_id,
        clip_url,
        clip_title,
        thumbnail_url,
        streamer_login: String::new(), // wird vom Aufrufer gesetzt
        twitch_user_id: broadcaster_id.to_string(),
        created_at,
        duration_seconds,
        view_count,
        game_name,
    })
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    #[test]
    fn query_traegt_14_tage_fenster() {
        let now = Utc.with_ymd_and_hms(2026, 8, 6, 12, 0, 0).unwrap();
        let params = clip_query_params("123", 100, None, now);

        assert!(
            params.contains(&("started_at".to_string(), "2026-07-23T12:00:00Z".to_string())),
            "started_at fehlt oder falsch: {params:?}"
        );
        assert!(
            params.contains(&("ended_at".to_string(), "2026-08-06T12:00:00Z".to_string())),
            "ended_at fehlt oder falsch: {params:?}"
        );
        assert!(params.contains(&("broadcaster_id".to_string(), "123".to_string())));
        assert!(params.contains(&("first".to_string(), "100".to_string())));
    }

    #[test]
    fn fenster_passt_zur_trigger_retention() {
        // Muss mit dem 14-Tage-Intervall des DB-Triggers
        // social_media_set_retention_until (schema.rs) übereinstimmen.
        assert_eq!(FETCH_WINDOW_DAYS, 14);
    }

    #[test]
    fn cursor_landet_als_after() {
        let now = Utc.with_ymd_and_hms(2026, 8, 6, 12, 0, 0).unwrap();
        let params = clip_query_params("123", 50, Some("abc"), now);
        assert!(params.contains(&("after".to_string(), "abc".to_string())));

        let ohne = clip_query_params("123", 50, None, now);
        assert!(!ohne.iter().any(|(k, _)| k == "after"));
    }
}
