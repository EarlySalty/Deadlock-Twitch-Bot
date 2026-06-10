//! Helix-Endpoints rund um Streams und Kategorien (für das Monitoring):
//! `/streams` (Login-Batches + Kategorie-Pagination), `/search/categories`
//! und `/channels/followers`. Semantik wie der Python-`TwitchAPI`-Wrapper.

use serde::Deserialize;

use crate::client::{HelixClient, HelixError};

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
            let body: StreamsResponse = resp.json().await?;
            out.extend(body.data);
        }
        Ok(out)
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
            let body: StreamsResponse = resp.json().await?;
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
        let body: CategorySearchResponse = resp.json().await?;
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
    /// `total`-Wert verlangt einen Moderator-Token — mit App-Token antwortet
    /// Twitch 401/403, dann (und bei jedem non-200) kommt `None` zurück.
    pub async fn get_followers_total(
        &self,
        broadcaster_id: &str,
    ) -> Result<Option<i64>, HelixError> {
        let broadcaster_id = broadcaster_id.trim();
        if broadcaster_id.is_empty() {
            return Ok(None);
        }
        let resp = self
            .get("/channels/followers")
            .await?
            .query(&[("broadcaster_id", broadcaster_id), ("first", "1")])
            .send()
            .await?;
        if !resp.status().is_success() {
            tracing::debug!(
                status = %resp.status(),
                "followers-total nicht verfügbar (App-Token ohne Moderator-Scope?)"
            );
            return Ok(None);
        }
        let body: FollowersResponse = resp.json().await?;
        Ok(body.total)
    }
}

#[cfg(test)]
mod tests {
    use crate::client::{HelixClient, HelixConfig};
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

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
        let total = client.get_followers_total("42").await.unwrap();
        assert_eq!(total, None);
    }
}
