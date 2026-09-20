//! Helix `GET /schedule` für öffentliche Streampläne.

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::client::{check_status_and_json, HelixClient, HelixError};

const SCHEDULE_PAGE_SIZE: usize = 25;
const SCHEDULE_HARD_CAP: usize = 100;

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct HelixScheduleSegment {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub start_time: String,
    #[serde(default)]
    pub end_time: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub canceled_until: Option<String>,
    #[serde(default)]
    pub is_recurring: bool,
}

#[derive(Debug, Default, Deserialize)]
struct ScheduleResponse {
    #[serde(default)]
    data: ScheduleData,
    #[serde(default)]
    pagination: Pagination,
}

#[derive(Debug, Default, Deserialize)]
struct ScheduleData {
    #[serde(default)]
    segments: Vec<HelixScheduleSegment>,
}

#[derive(Debug, Default, Deserialize)]
struct Pagination {
    #[serde(default)]
    cursor: Option<String>,
}

impl HelixClient {
    /// Liest kommende Segmente aus dem Twitch Stream Schedule.
    pub async fn get_channel_stream_schedule(
        &self,
        broadcaster_id: &str,
        start_time: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<HelixScheduleSegment>, HelixError> {
        let broadcaster_id = broadcaster_id.trim();
        let limit = limit.min(SCHEDULE_HARD_CAP);
        if broadcaster_id.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }

        let mut out = Vec::new();
        let mut after: Option<String> = None;
        let mut seen_cursors = HashSet::new();

        while out.len() < limit {
            let first = (limit - out.len()).min(SCHEDULE_PAGE_SIZE).to_string();
            let mut params = vec![
                ("broadcaster_id", broadcaster_id.to_string()),
                ("first", first),
                ("start_time", start_time.to_rfc3339()),
            ];
            if let Some(cursor) = &after {
                params.push(("after", cursor.clone()));
            }

            let request = self.get("/schedule").await?.query(&params);
            let response = self.send_with_retry(request).await?;
            let body: ScheduleResponse = check_status_and_json(response).await?;
            let page_empty = body.data.segments.is_empty();
            out.extend(body.data.segments);

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

        out.retain(|segment| segment.canceled_until.is_none());
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

    async fn client(server: &MockServer) -> HelixClient {
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "tok",
                "expires_in": 3600
            })))
            .expect(1)
            .mount(server)
            .await;
        HelixClient::new(HelixConfig {
            client_id: "cid".into(),
            client_secret: "sec".into(),
            token_url: format!("{}/oauth2/token", server.uri()),
            helix_base: format!("{}/helix", server.uri()),
        })
        .unwrap()
    }

    #[tokio::test]
    async fn schedule_parst_segmente_und_filtert_abgesagte() {
        let server = MockServer::start().await;
        let client = client(&server).await;
        Mock::given(method("GET"))
            .and(path("/helix/schedule"))
            .and(query_param("broadcaster_id", "42"))
            .and(query_param("first", "3"))
            .and(header("Client-Id", "cid"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": {
                    "segments": [
                        {
                            "id": "a",
                            "start_time": "2026-09-21T18:00:00Z",
                            "end_time": "2026-09-21T20:00:00Z",
                            "title": "Deadlock",
                            "is_recurring": true
                        },
                        {
                            "id": "b",
                            "start_time": "2026-09-22T18:00:00Z",
                            "end_time": "2026-09-22T20:00:00Z",
                            "title": "Fällt aus",
                            "canceled_until": "2026-09-22T20:00:00Z"
                        }
                    ]
                },
                "pagination": {}
            })))
            .expect(1)
            .mount(&server)
            .await;

        let start = "2026-09-20T10:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let items = client
            .get_channel_stream_schedule("42", start, 3)
            .await
            .unwrap();

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "a");
        assert_eq!(items[0].title, "Deadlock");
        server.verify().await;
    }

    #[tokio::test]
    async fn schedule_leere_id_oder_limit_null_ohne_netzwerk() {
        let server = MockServer::start().await;
        let client = HelixClient::new(HelixConfig {
            client_id: "cid".into(),
            client_secret: "sec".into(),
            token_url: format!("{}/oauth2/token", server.uri()),
            helix_base: format!("{}/helix", server.uri()),
        })
        .unwrap();
        let start = Utc::now();

        assert!(client
            .get_channel_stream_schedule("", start, 10)
            .await
            .unwrap()
            .is_empty());
        assert!(client
            .get_channel_stream_schedule("42", start, 0)
            .await
            .unwrap()
            .is_empty());
        assert!(server.received_requests().await.unwrap().is_empty());
    }
}
