//! Helix `GET /clips` fuer Broadcaster-Clips.
//!
//! Twitch liefert Clips sortiert nach View Count; dieser Client veraendert diese
//! Reihenfolge nicht.

use std::collections::HashSet;

use serde::Deserialize;

use crate::client::{check_status_and_json, HelixClient, HelixError};

const CLIPS_HARD_CAP: usize = 1_000;
const CLIPS_PAGE_SIZE: usize = 100;

/// Clip-Daten aus Helix `GET /clips`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct HelixClip {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub broadcaster_name: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub duration: f64,
    #[serde(default)]
    pub game_id: String,
}

fn capped_clip_limit(limit: usize) -> usize {
    limit.min(CLIPS_HARD_CAP)
}

fn clips_page_size(remaining: usize) -> usize {
    remaining.min(CLIPS_PAGE_SIZE)
}

#[derive(Debug, Default, Deserialize)]
struct ClipsResponse {
    #[serde(default)]
    data: Vec<HelixClip>,
    #[serde(default)]
    pagination: Pagination,
}

#[derive(Debug, Default, Deserialize)]
struct Pagination {
    #[serde(default)]
    cursor: Option<String>,
}

impl HelixClient {
    /// Holt Clips eines Broadcasters via Helix `GET /clips`.
    ///
    /// Twitch sortiert die Antwort nach View Count. `limit` wird auf 1000 gekappt.
    pub async fn get_clips_by_broadcaster(
        &self,
        broadcaster_id: &str,
        limit: usize,
    ) -> Result<Vec<HelixClip>, HelixError> {
        let broadcaster_id = broadcaster_id.trim();
        let limit = capped_clip_limit(limit);
        if broadcaster_id.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }

        let mut out = Vec::new();
        let mut after: Option<String> = None;
        let mut seen_cursors = HashSet::new();

        while out.len() < limit {
            let remaining = limit - out.len();
            let first = clips_page_size(remaining).to_string();
            let mut params = vec![
                ("broadcaster_id", broadcaster_id.to_string()),
                ("first", first),
            ];
            if let Some(cursor) = &after {
                params.push(("after", cursor.clone()));
            }

            let request = self.get("/clips").await?.query(&params);
            let response = self.send_with_retry(request).await?;
            let body: ClipsResponse = check_status_and_json(response).await?;
            let page_empty = body.data.is_empty();
            out.extend(body.data);

            let next_cursor = body
                .pagination
                .cursor
                .map(|cursor| cursor.trim().to_string())
                .filter(|cursor| !cursor.is_empty());
            let Some(next_cursor) = next_cursor else {
                break;
            };
            if page_empty || !seen_cursors.insert(next_cursor.clone()) {
                break;
            }
            after = Some(next_cursor);
        }

        out.truncate(limit);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::HelixConfig;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn bare_client(server: &MockServer) -> HelixClient {
        HelixClient::new(HelixConfig {
            client_id: "cid".to_string(),
            client_secret: "sec".to_string(),
            token_url: format!("{}/oauth2/token", server.uri()),
            helix_base: format!("{}/helix", server.uri()),
        })
        .unwrap()
    }

    async fn client_with_token(server: &MockServer) -> HelixClient {
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "tok",
                "expires_in": 3600
            })))
            .expect(1)
            .mount(server)
            .await;
        bare_client(server)
    }

    fn clip_json(id: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "broadcaster_name": "Nani",
            "title": format!("Clip {id}"),
            "duration": 12.5,
            "game_id": "142"
        })
    }

    fn clip_items(count: usize, start: usize) -> Vec<serde_json::Value> {
        (start..start + count)
            .map(|idx| clip_json(&format!("clip-{idx}")))
            .collect()
    }

    #[tokio::test]
    async fn clips_parst_einseiten_response_inkl_auth_headers() {
        let server = MockServer::start().await;
        let client = client_with_token(&server).await;
        Mock::given(method("GET"))
            .and(path("/helix/clips"))
            .and(query_param("broadcaster_id", "42"))
            .and(query_param("first", "2"))
            .and(header("Client-Id", "cid"))
            .and(header("Authorization", "Bearer tok"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [clip_json("abc")],
                "pagination": {}
            })))
            .expect(1)
            .mount(&server)
            .await;

        let clips = client.get_clips_by_broadcaster("42", 2).await.unwrap();

        assert_eq!(
            clips,
            vec![HelixClip {
                id: "abc".to_string(),
                broadcaster_name: "Nani".to_string(),
                title: "Clip abc".to_string(),
                duration: 12.5,
                game_id: "142".to_string(),
            }]
        );
        server.verify().await;
    }

    #[tokio::test]
    async fn clips_folgt_cursor_ueber_zwei_seiten_und_nutzt_remaining_first() {
        let server = MockServer::start().await;
        let client = client_with_token(&server).await;
        Mock::given(method("GET"))
            .and(path("/helix/clips"))
            .and(query_param("broadcaster_id", "42"))
            .and(query_param("first", "100"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": clip_items(100, 0),
                "pagination": {"cursor": "cursor-1"}
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/helix/clips"))
            .and(query_param("broadcaster_id", "42"))
            .and(query_param("first", "1"))
            .and(query_param("after", "cursor-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": clip_items(1, 100),
                "pagination": {}
            })))
            .expect(1)
            .mount(&server)
            .await;

        let clips = client.get_clips_by_broadcaster("42", 101).await.unwrap();

        assert_eq!(clips.len(), 101);
        assert_eq!(clips[0].id, "clip-0");
        assert_eq!(clips[100].id, "clip-100");
        server.verify().await;
    }

    #[tokio::test]
    async fn clips_leere_id_oder_limit_null_ohne_netzwerk() {
        let server = MockServer::start().await;
        let client = bare_client(&server);

        assert!(client
            .get_clips_by_broadcaster("   ", 10)
            .await
            .unwrap()
            .is_empty());
        assert!(client
            .get_clips_by_broadcaster("42", 0)
            .await
            .unwrap()
            .is_empty());

        let requests = server.received_requests().await.expect("requests");
        assert!(
            requests.is_empty(),
            "kein Token- oder Helix-Request erwartet"
        );
    }

    #[tokio::test]
    async fn clips_non_2xx_ergibt_status_error() {
        let server = MockServer::start().await;
        let client = client_with_token(&server).await;
        Mock::given(method("GET"))
            .and(path("/helix/clips"))
            .and(query_param("broadcaster_id", "42"))
            .and(query_param("first", "1"))
            .respond_with(ResponseTemplate::new(403))
            .expect(1)
            .mount(&server)
            .await;

        let err = client.get_clips_by_broadcaster("42", 1).await.unwrap_err();

        assert!(matches!(err, HelixError::Status { status: 403 }), "{err:?}");
        server.verify().await;
    }

    #[tokio::test]
    async fn clips_bricht_bei_wiederholtem_cursor_ab() {
        let server = MockServer::start().await;
        let client = client_with_token(&server).await;
        Mock::given(method("GET"))
            .and(path("/helix/clips"))
            .and(query_param("first", "100"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [clip_json("first")],
                "pagination": {"cursor": "same"}
            })))
            .expect(1)
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/helix/clips"))
            .and(query_param("first", "100"))
            .and(query_param("after", "same"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [clip_json("second")],
                "pagination": {"cursor": "same"}
            })))
            .expect(1)
            .mount(&server)
            .await;

        let clips = client.get_clips_by_broadcaster("42", 250).await.unwrap();

        assert_eq!(
            clips
                .iter()
                .map(|clip| clip.id.as_str())
                .collect::<Vec<_>>(),
            ["first", "second"]
        );
        server.verify().await;
    }

    #[test]
    fn clips_limit_cap_und_page_size() {
        assert_eq!(capped_clip_limit(0), 0);
        assert_eq!(capped_clip_limit(999), 999);
        assert_eq!(capped_clip_limit(1_500), CLIPS_HARD_CAP);
        assert_eq!(clips_page_size(1), 1);
        assert_eq!(clips_page_size(250), CLIPS_PAGE_SIZE);
    }
}
