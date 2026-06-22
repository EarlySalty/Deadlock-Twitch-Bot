//! Helix-Endpoints rund um Streams und Kategorien (für das Monitoring):
//! `/streams` (Login-Batches + Kategorie-Pagination), `/search/categories`,
//! `/channels` und `/channels/followers`. Semantik wie der Python-`TwitchAPI`-Wrapper.

use serde::Deserialize;

use crate::client::{check_status_and_json, HelixClient, HelixError};

/// Ein Live-Stream aus Helix `/streams`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct HelixStream {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub user_id: String,
    #[serde(default)]
    pub user_login: String,
    #[serde(default)]
    pub user_name: String,
    #[serde(default)]
    pub game_id: String,
    #[serde(default)]
    pub game_name: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub viewer_count: i64,
    #[serde(default)]
    pub is_mature: bool,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub started_at: String,
}

#[derive(Debug, Deserialize)]
struct StreamsResponse {
    #[serde(default)]
    data: Vec<HelixStream>,
    #[serde(default)]
    pagination: Pagination,
}

#[derive(Debug, Default, Deserialize)]
struct Pagination {
    #[serde(default)]
    cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CategorySearchResponse {
    #[serde(default)]
    data: Vec<CategoryEntry>,
}

#[derive(Debug, Deserialize)]
struct CategoryEntry {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
}

#[derive(Debug, Deserialize)]
struct FollowersResponse {
    #[serde(default)]
    total: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct FollowersTotalFetch {
    pub total: Option<i64>,
    pub http_status: Option<u16>,
    pub error_code: Option<String>,
}

fn http_error_code(status: reqwest::StatusCode) -> String {
    match status.as_u16() {
        401 => "unauthorized".to_string(),
        403 => "forbidden".to_string(),
        404 => "not_found".to_string(),
        429 => "rate_limited".to_string(),
        code => format!("http_{code}"),
    }
}

/// Kanal-Metadaten aus Helix `/channels` (auch offline verfügbar).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct HelixChannelInfo {
    #[serde(default)]
    pub broadcaster_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub game_name: String,
    #[serde(default)]
    pub broadcaster_language: String,
}

#[derive(Debug, Deserialize)]
struct ChannelsResponse {
    #[serde(default)]
    data: Vec<HelixChannelInfo>,
}

/// Hartes Limit der Kategorie-Pagination (Python: `min(limit, 1200)`).
const CATEGORY_HARD_CAP: usize = 1200;

impl HelixClient {
    /// Live-Streams für die gegebenen Logins (gebatcht à 100, wie Python).
    pub async fn get_streams_by_logins(
        &self,
        logins: &[String],
        language: Option<&str>,
    ) -> Result<Vec<HelixStream>, HelixError> {
        let mut out = Vec::new();
        let clean: Vec<&String> = logins.iter().filter(|l| !l.trim().is_empty()).collect();
        for chunk in clean.chunks(100) {
            let mut params: Vec<(&str, &str)> =
                chunk.iter().map(|l| ("user_login", l.as_str())).collect();
            if let Some(language) = language {
                params.push(("language", language));
            }
            let resp = self.get("/streams").await?.query(&params).send().await?;
            let body: StreamsResponse = check_status_and_json(resp).await?;
            out.extend(body.data);
        }
        Ok(out)
    }

    /// Kanal-Metadaten (Titel/Kategorie) eines Broadcasters über `/channels`
    /// — liefert im Gegensatz zu `/streams` beim Go-Live sofort (kein
    /// Propagations-Lag) und unabhängig von der Kanal-Sprache.
    pub async fn get_channel_information(
        &self,
        broadcaster_id: &str,
    ) -> Result<Option<HelixChannelInfo>, HelixError> {
        let broadcaster_id = broadcaster_id.trim();
        if broadcaster_id.is_empty() {
            return Ok(None);
        }
        let params = [("broadcaster_id", broadcaster_id)];
        let resp = self.get("/channels").await?.query(&params).send().await?;
        let body: ChannelsResponse = check_status_and_json(resp).await?;
        Ok(body.data.into_iter().next())
    }

    /// Bis zu `limit` Live-Streams einer Kategorie (Cursor-Pagination à 100).
    pub async fn get_streams_by_category(
        &self,
        game_id: &str,
        language: Option<&str>,
        limit: usize,
    ) -> Result<Vec<HelixStream>, HelixError> {
        let limit = limit.clamp(1, CATEGORY_HARD_CAP);
        let mut out: Vec<HelixStream> = Vec::new();
        let mut after: Option<String> = None;
        while out.len() < limit {
            let mut params: Vec<(&str, String)> = vec![
                ("game_id", game_id.to_string()),
                ("first", "100".to_string()),
            ];
            if let Some(language) = language {
                params.push(("language", language.to_string()));
            }
            if let Some(cursor) = &after {
                params.push(("after", cursor.clone()));
            }
            let resp = self.get("/streams").await?.query(&params).send().await?;
            let body: StreamsResponse = check_status_and_json(resp).await?;
            let empty = body.data.is_empty();
            out.extend(body.data);
            after = body.pagination.cursor;
            if after.is_none() || empty {
                break;
            }
        }
        out.truncate(limit);
        Ok(out)
    }

    /// game_id einer Kategorie über `/search/categories` (exakter Treffer
    /// bevorzugt, sonst Präfix — wie Python `search_category_id`). Gecacht.
    pub async fn search_category_id(&self, query: &str) -> Result<Option<String>, HelixError> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(None);
        }
        let key = query.to_lowercase();
        if let Some(cached) = self.category_cache.lock().await.get(&key) {
            return Ok(Some(cached.clone()));
        }
        let resp = self
            .get("/search/categories")
            .await?
            .query(&[("query", query), ("first", "25")])
            .send()
            .await?;
        let body: CategorySearchResponse = check_status_and_json(resp).await?;
        let mut best: Option<String> = None;
        for entry in body.data {
            let name = entry.name.trim().to_lowercase();
            if name == key {
                best = Some(entry.id);
                break;
            }
            if best.is_none() && name.starts_with(&key) {
                best = Some(entry.id);
            }
        }
        if let Some(id) = &best {
            self.category_cache.lock().await.insert(key, id.clone());
        }
        Ok(best)
    }

    /// Thumbnail des neuesten VOD (`type=archive`) als 1280x720-URL mit
    /// `rand`-Cache-Buster (Python `get_latest_vod_thumbnail`). Best-effort.
    pub async fn get_latest_vod_thumbnail(
        &self,
        user_id: &str,
    ) -> Result<Option<String>, HelixError> {
        let user_id = user_id.trim();
        if user_id.is_empty() {
            return Ok(None);
        }
        let resp = self
            .get("/videos")
            .await?
            .query(&[("user_id", user_id), ("type", "archive"), ("first", "1")])
            .send()
            .await?;
        if !resp.status().is_success() {
            return Ok(None);
        }
        #[derive(Deserialize)]
        struct VideosResponse {
            #[serde(default)]
            data: Vec<VideoEntry>,
        }
        #[derive(Deserialize)]
        struct VideoEntry {
            #[serde(default)]
            thumbnail_url: String,
        }
        let body: VideosResponse = resp.json().await?;
        let thumb = body
            .data
            .first()
            .map(|v| v.thumbnail_url.trim().to_string())
            .filter(|t| !t.is_empty());
        Ok(thumb.map(|t| {
            let resolved = t.replace("{width}", "1280").replace("{height}", "720");
            let rand = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            format!("{resolved}?rand={rand}")
        }))
    }

    /// Follower-Gesamtzahl via `/channels/followers`. Best-effort: Der
    /// `total`-Wert verlangt einen **Moderator-Token mit `moderator:read:followers`**
    /// für genau diesen Broadcaster.
    ///
    /// `user_token`:
    /// - `Some(tok)` → Request mit diesem Bearer (Streamer- oder zentraler
    ///   Bot-Token, der den Kanal moderiert). Liefert die echte Zahl.
    /// - `None` → App-Token-Pfad wie bisher; Twitch antwortet ohne Scope
    ///   401/403 und es kommt `total = None` mit Diagnosefeldern zurück.
    ///
    /// Port: Python `twitch_api.get_followers_total(broadcaster_id, user_token=…)`.
    pub async fn get_followers_total(
        &self,
        broadcaster_id: &str,
        user_token: Option<&str>,
    ) -> Result<FollowersTotalFetch, HelixError> {
        let broadcaster_id = broadcaster_id.trim();
        if broadcaster_id.is_empty() {
            return Ok(FollowersTotalFetch::default());
        }
        let token = user_token.map(str::trim).filter(|t| !t.is_empty());
        let request = match token {
            Some(token) => self.get_with_user_token("/channels/followers", token),
            None => self.get("/channels/followers").await?,
        };
        let resp = request
            .query(&[("broadcaster_id", broadcaster_id), ("first", "1")])
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            tracing::debug!(
                status = %status,
                with_user_token = token.is_some(),
                "followers-total nicht verfügbar (Token ohne moderator:read:followers?)"
            );
            return Ok(FollowersTotalFetch {
                total: None,
                http_status: Some(status.as_u16()),
                error_code: Some(http_error_code(status)),
            });
        }
        let body: FollowersResponse = resp.json().await?;
        Ok(FollowersTotalFetch {
            total: body.total,
            http_status: Some(200),
            error_code: None,
        })
    }

    /// Die jüngsten Archiv-VODs eines Kanals (`type=archive`) mit den Feldern,
    /// die der Highlight-Clipper braucht (id/created_at/duration). Best-effort:
    /// leerer Login oder non-200 → leere Liste.
    pub async fn get_archive_videos(
        &self,
        user_id: &str,
        first: u32,
    ) -> Result<Vec<ArchiveVideo>, HelixError> {
        let user_id = user_id.trim();
        if user_id.is_empty() {
            return Ok(Vec::new());
        }
        let first = first.to_string();
        let resp = self
            .get("/videos")
            .await?
            .query(&[("user_id", user_id), ("type", "archive"), ("first", first.as_str())])
            .send()
            .await?;
        if !resp.status().is_success() {
            return Ok(Vec::new());
        }
        #[derive(Deserialize)]
        struct VideosResponse {
            #[serde(default)]
            data: Vec<ArchiveVideo>,
        }
        let body: VideosResponse = resp.json().await?;
        Ok(body.data)
    }

    /// Subscription-Übersicht eines Broadcasters via `GET /subscriptions`
    /// (`total`/`points` aus der Wurzel, Datenquelle des Block-6-Snapshot-
    /// Pollers). Braucht ein **User-Token** des Broadcasters.
    ///
    /// Port: `twitch_api.py:get_broadcaster_subscriptions_result`.
    /// Scope: `channel:read:subscriptions`.
    pub async fn get_broadcaster_subscriptions(
        &self,
        broadcaster_id: &str,
        user_token: &str,
    ) -> Result<BroadcasterSubscriptions, HelixError> {
        let resp = self
            .get_with_user_token("/subscriptions", user_token)
            .query(&[("broadcaster_id", broadcaster_id), ("first", "1")])
            .send()
            .await?;
        check_status_and_json(resp).await
    }

    /// Werbe-Schedule eines Broadcasters via `GET /channels/ads` (`data[0]`).
    /// Braucht ein **User-Token** des Broadcasters. `Ok(None)` = leeres
    /// `data`-Array (Helix lieferte keinen Schedule).
    ///
    /// Port: `twitch_api.py:get_ad_schedule_result`.
    /// Scope: `channel:read:ads`.
    pub async fn get_ad_schedule(
        &self,
        broadcaster_id: &str,
        user_token: &str,
    ) -> Result<Option<AdSchedule>, HelixError> {
        let resp = self
            .get_with_user_token("/channels/ads", user_token)
            .query(&[("broadcaster_id", broadcaster_id)])
            .send()
            .await?;
        let body: AdScheduleResponse = check_status_and_json(resp).await?;
        Ok(body.data.into_iter().next())
    }
}

/// Ein Archiv-VOD (Teilmenge der Helix-`/videos`-Felder).
#[derive(Debug, Clone, Deserialize)]
pub struct ArchiveVideo {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub duration: String,
}

/// Ein einzelner Abonnent aus `GET /subscriptions`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Subscription {
    #[serde(default)]
    pub user_id: String,
    #[serde(default)]
    pub user_login: String,
    #[serde(default)]
    pub user_name: String,
    /// `"1000"` / `"2000"` / `"3000"` (Tier 1/2/3).
    #[serde(default)]
    pub tier: String,
    #[serde(default)]
    pub is_gift: bool,
}

/// Antwort auf `GET /subscriptions` (Datenquelle des Block-6-Snapshot-Pollers).
///
/// `total`/`points` stehen im Wurzel-Objekt (nicht je Eintrag); der Poller
/// schreibt beide. Port: `mixin.py:_collect_subs_for_user`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct BroadcasterSubscriptions {
    #[serde(default)]
    pub data: Vec<Subscription>,
    /// Gesamtzahl aktiver Subs (Wurzel-Feld der Helix-Antwort).
    #[serde(default)]
    pub total: i64,
    /// Sub-Punkte des Channels (Wurzel-Feld).
    #[serde(default)]
    pub points: i64,
}

/// Antwort auf `GET /channels/ads` (`data[0]`).
///
/// Felder wie vom Snapshot-Poller geschrieben (Port: `mixin.py:889–894`). Die
/// Zeit-Felder kommen von Helix als Unix-Sekunden-Zahl ODER als String —
/// beides wird tolerant zu `Option<String>` geparst (analog `_safe_time_text`);
/// die Normalisierung zu ISO-8601 macht der Poller (separates Ticket).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AdSchedule {
    /// Sekunden bis zur nächsten geplanten Werbung.
    #[serde(default)]
    pub duration: i64,
    #[serde(default, deserialize_with = "deserialize_string_or_number")]
    pub next_ad_at: Option<String>,
    #[serde(default, deserialize_with = "deserialize_string_or_number")]
    pub last_ad_at: Option<String>,
    /// Verbleibende Preroll-freie Zeit in Sekunden.
    #[serde(default)]
    pub preroll_free_time: i64,
    #[serde(default)]
    pub snooze_count: i64,
    #[serde(default, deserialize_with = "deserialize_string_or_number")]
    pub snooze_refresh_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AdScheduleResponse {
    #[serde(default)]
    data: Vec<AdSchedule>,
}

/// Deserialisiert ein Helix-Zeitfeld, das als String **oder** als Zahl kommen
/// kann (Unix-Sekunden), zu `Option<String>`. `null`/leerer String → `None`.
fn deserialize_string_or_number<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde_json::Value;
    let value = Option::<Value>::deserialize(deserializer)?;
    Ok(match value {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        Some(Value::Number(n)) => Some(n.to_string()),
        Some(other) => Some(other.to_string()),
    })
}

/// Normalisiert ein Ad-Schedule-Zeitfeld auf ISO-8601 (UTC), wie der Python-Poller
/// (`mixin.py:_safe_time_text`, 869–886). Eingabe ist der bereits zu `String`
/// deserialisierte Wert (siehe [`deserialize_string_or_number`]); Helix liefert
/// hier Unix-Sekunden als Zahl ODER einen fertigen ISO-String.
///
/// Regeln (byte-genau wie Python):
/// - leerer Wert → `None`
/// - rein numerisch (ggf. mit Nachkommastellen): als Unix-Epoch interpretieren
///   - `ts <= 0` → `None` (verworfen)
///   - `ts > 10_000_000_000` → Millisekunden, durch 1000 teilen
///   - gültig → ISO-8601 UTC (`...+00:00`); nicht darstellbar → `str(int(ts))`
/// - nicht-numerischer String (bereits ISO o. Ä.) → unverändert durchreichen
pub fn normalize_ad_time(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Nur reine Zahlen (inkl. Float) als Epoch behandeln — sonst durchreichen.
    let Ok(parsed) = trimmed.parse::<f64>() else {
        return Some(trimmed.to_string());
    };
    let mut ts = parsed;
    if ts <= 0.0 {
        return None;
    }
    // Manche APIs liefern Millisekunden → auf Sekunden normalisieren.
    if ts > 10_000_000_000.0 {
        ts /= 1000.0;
    }
    let secs = ts.trunc() as i64;
    let nanos = ((ts - ts.trunc()) * 1_000_000_000.0).round() as u32;
    match chrono::DateTime::<chrono::Utc>::from_timestamp(secs, nanos) {
        Some(dt) => Some(dt.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, false)),
        // Nicht darstellbar (Überlauf) → ganzzahlige Sekunden als Text (Python-Fallback).
        None => Some(secs.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_ad_time;
    use crate::client::{HelixClient, HelixConfig, HelixError};
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn normalize_ad_time_epoch_sekunden_zu_iso() {
        // 1750000000 = 2025-06-15T15:06:40Z (Helix liefert es als Zahl-String).
        let iso = normalize_ad_time("1750000000").expect("ISO");
        assert_eq!(iso, "2025-06-15T15:06:40+00:00");
    }

    #[test]
    fn normalize_ad_time_millisekunden_werden_durch_1000_geteilt() {
        // > 10e9 ⇒ Millisekunden ⇒ /1000 ⇒ gleiche Sekunde wie oben.
        let iso = normalize_ad_time("1750000000000").expect("ISO");
        assert_eq!(iso, "2025-06-15T15:06:40+00:00");
    }

    #[test]
    fn normalize_ad_time_nicht_positiv_wird_verworfen() {
        assert_eq!(normalize_ad_time("0"), None);
        assert_eq!(normalize_ad_time("-5"), None);
    }

    #[test]
    fn normalize_ad_time_leer_ist_none() {
        assert_eq!(normalize_ad_time(""), None);
        assert_eq!(normalize_ad_time("   "), None);
    }

    #[test]
    fn normalize_ad_time_iso_string_wird_durchgereicht() {
        // Bereits ISO (nicht-numerisch) → unverändert.
        assert_eq!(
            normalize_ad_time("2026-06-15T12:00:00Z").as_deref(),
            Some("2026-06-15T12:00:00Z")
        );
    }

    async fn client_with(server: &MockServer) -> HelixClient {
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "tok",
                "expires_in": 3600
            })))
            .mount(server)
            .await;
        HelixClient::new(HelixConfig {
            client_id: "cid".to_string(),
            client_secret: "sec".to_string(),
            token_url: format!("{}/oauth2/token", server.uri()),
            helix_base: format!("{}/helix", server.uri()),
        })
        .unwrap()
    }

    #[tokio::test]
    async fn streams_by_logins_parst_streams() {
        let server = MockServer::start().await;
        let client = client_with(&server).await;
        Mock::given(method("GET"))
            .and(path("/helix/streams"))
            .and(query_param("user_login", "drag"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{
                    "id": "991", "user_id": "42", "user_login": "drag",
                    "user_name": "Drag", "game_id": "g1", "game_name": "Deadlock",
                    "title": "Ranked", "language": "de", "viewer_count": 12,
                    "is_mature": false, "tags": ["DE"],
                    "started_at": "2026-06-09T18:00:00Z"
                }],
                "pagination": {}
            })))
            .mount(&server)
            .await;

        let streams = client
            .get_streams_by_logins(&["drag".to_string()], Some("de"))
            .await
            .unwrap();
        assert_eq!(streams.len(), 1);
        assert_eq!(streams[0].user_login, "drag");
        assert_eq!(streams[0].viewer_count, 12);
        assert_eq!(streams[0].tags.as_deref(), Some(&["DE".to_string()][..]));
    }

    #[tokio::test]
    async fn category_pagination_folgt_cursor_und_kappt_limit() {
        let server = MockServer::start().await;
        let client = client_with(&server).await;
        let stream = |id: &str| {
            serde_json::json!({
                "id": id, "user_login": format!("u{id}"), "game_name": "Deadlock",
                "viewer_count": 1, "started_at": "2026-06-09T18:00:00Z"
            })
        };
        // Seite 1 mit Cursor …
        Mock::given(method("GET"))
            .and(path("/helix/streams"))
            .and(query_param("game_id", "g1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [stream("1"), stream("2")],
                "pagination": {"cursor": "c1"}
            })))
            .expect(1)
            .up_to_n_times(1)
            .mount(&server)
            .await;

        let streams = client.get_streams_by_category("g1", None, 2).await.unwrap();
        assert_eq!(streams.len(), 2, "Limit erreicht — kein zweiter Request");
    }

    #[tokio::test]
    async fn category_suche_bevorzugt_exakten_treffer_und_cacht() {
        let server = MockServer::start().await;
        let client = client_with(&server).await;
        Mock::given(method("GET"))
            .and(path("/helix/search/categories"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [
                    {"id": "111", "name": "Deadlock 2 Fan"},
                    {"id": "222", "name": "Deadlock"}
                ]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let id = client.search_category_id("deadlock").await.unwrap();
        assert_eq!(id.as_deref(), Some("222"));
        // Zweiter Aufruf kommt aus dem Cache (expect(1) oben).
        let cached = client.search_category_id("Deadlock").await.unwrap();
        assert_eq!(cached.as_deref(), Some("222"));
        server.verify().await;
    }

    #[tokio::test]
    async fn followers_total_none_bei_fehlendem_scope() {
        let server = MockServer::start().await;
        let client = client_with(&server).await;
        Mock::given(method("GET"))
            .and(path("/helix/channels/followers"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        let fetch = client.get_followers_total("42", None).await.unwrap();
        assert_eq!(fetch.total, None);
        assert_eq!(fetch.http_status, Some(401));
        assert_eq!(fetch.error_code.as_deref(), Some("unauthorized"));
    }

    #[tokio::test]
    async fn followers_total_mit_user_token_liefert_total() {
        let server = MockServer::start().await;
        let client = client_with(&server).await;
        // Der moderator-scoped User-Token MUSS den App-Token im Authorization-Header
        // ersetzen, sonst antwortet Twitch ohne `total`.
        Mock::given(method("GET"))
            .and(path("/helix/channels/followers"))
            .and(query_param("broadcaster_id", "42"))
            .and(header("Authorization", "Bearer usertok"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "total": 1337,
                "data": []
            })))
            .expect(1)
            .mount(&server)
            .await;
        let fetch = client
            .get_followers_total("42", Some("usertok"))
            .await
            .unwrap();
        assert_eq!(fetch.total, Some(1337));
        assert_eq!(fetch.http_status, Some(200));
        assert_eq!(fetch.error_code, None);
        server.verify().await;
    }

    #[tokio::test]
    async fn archive_videos_parst_felder() {
        let server = MockServer::start().await;
        let client = client_with(&server).await;
        Mock::given(method("GET"))
            .and(path("/helix/videos"))
            .and(query_param("user_id", "42"))
            .and(query_param("type", "archive"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [
                    {"id": "v1", "created_at": "2026-06-09T18:00:00Z", "duration": "2h3m4s"},
                    {"id": "v2", "created_at": "2026-06-08T18:00:00Z", "duration": "47m"}
                ]
            })))
            .mount(&server)
            .await;
        let vods = client.get_archive_videos("42", 20).await.unwrap();
        assert_eq!(vods.len(), 2);
        assert_eq!(vods[0].id, "v1");
        assert_eq!(vods[0].duration, "2h3m4s");
        // Leerer Login → keine Anfrage, leere Liste.
        assert!(client.get_archive_videos("  ", 20).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn subscriptions_parst_total_und_points() {
        let server = MockServer::start().await;
        let client = client_with(&server).await;
        Mock::given(method("GET"))
            .and(path("/helix/subscriptions"))
            .and(query_param("broadcaster_id", "42"))
            .and(header("Authorization", "Bearer user-tok"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [
                    {"user_id": "9", "user_login": "fan", "user_name": "Fan",
                     "tier": "1000", "is_gift": false}
                ],
                "total": 137,
                "points": 152
            })))
            .mount(&server)
            .await;
        let subs = client
            .get_broadcaster_subscriptions("42", "user-tok")
            .await
            .unwrap();
        assert_eq!(subs.total, 137);
        assert_eq!(subs.points, 152);
        assert_eq!(subs.data.len(), 1);
        assert_eq!(subs.data[0].tier, "1000");
    }

    #[tokio::test]
    async fn subscriptions_403_ergibt_status_fehler() {
        let server = MockServer::start().await;
        let client = client_with(&server).await;
        Mock::given(method("GET"))
            .and(path("/helix/subscriptions"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&server)
            .await;
        let err = client
            .get_broadcaster_subscriptions("42", "user-tok")
            .await
            .unwrap_err();
        assert!(matches!(err, HelixError::Status { status: 403 }), "{err:?}");
    }

    #[tokio::test]
    async fn ad_schedule_parst_felder_inkl_zahl_timestamps() {
        let server = MockServer::start().await;
        let client = client_with(&server).await;
        // Helix liefert die Zeit-Felder hier als Unix-Sekunden-Zahl.
        Mock::given(method("GET"))
            .and(path("/helix/channels/ads"))
            .and(query_param("broadcaster_id", "42"))
            .and(header("Authorization", "Bearer user-tok"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{
                    "next_ad_at": 1750000000,
                    "last_ad_at": 1749990000,
                    "duration": 60,
                    "preroll_free_time": 90,
                    "snooze_count": 2,
                    "snooze_refresh_at": "2026-06-15T12:00:00Z"
                }]
            })))
            .mount(&server)
            .await;
        let ad = client
            .get_ad_schedule("42", "user-tok")
            .await
            .unwrap()
            .expect("data[0] vorhanden");
        assert_eq!(ad.duration, 60);
        assert_eq!(ad.preroll_free_time, 90);
        assert_eq!(ad.snooze_count, 2);
        assert_eq!(ad.next_ad_at.as_deref(), Some("1750000000"));
        assert_eq!(ad.snooze_refresh_at.as_deref(), Some("2026-06-15T12:00:00Z"));
    }

    #[tokio::test]
    async fn ad_schedule_leeres_data_ergibt_none() {
        let server = MockServer::start().await;
        let client = client_with(&server).await;
        Mock::given(method("GET"))
            .and(path("/helix/channels/ads"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"data": []})),
            )
            .mount(&server)
            .await;
        let ad = client.get_ad_schedule("42", "user-tok").await.unwrap();
        assert!(ad.is_none());
    }
}
