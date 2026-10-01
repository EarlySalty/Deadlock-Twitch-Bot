use std::collections::HashSet;

use serde::Deserialize;
use tb_transport_twitch::{HelixClient, HelixError};

use crate::error::VodArchiveError;
use crate::twitch::VodEintrag;

#[derive(Deserialize)]
struct Video {
    id: String,
    user_id: String,
    title: String,
    duration: String,
}

#[derive(Default, Deserialize)]
struct Pagination {
    cursor: Option<String>,
}

#[derive(Deserialize)]
struct VideosResponse {
    data: Vec<Video>,
    #[serde(default)]
    pagination: Pagination,
}

fn duration_seconds(value: &str) -> Result<i64, HelixError> {
    let mut seconds = 0_i64;
    let mut digits = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_digit() {
            digits.push(char::from(byte));
            continue;
        }
        let factor = match byte {
            b'h' => 3600,
            b'm' => 60,
            b's' => 1,
            _ => return Err(HelixError::InvalidResponse("Ungültige VOD-Dauer")),
        };
        let part = digits
            .parse::<i64>()
            .ok()
            .and_then(|part| part.checked_mul(factor));
        seconds = part
            .and_then(|part| seconds.checked_add(part))
            .ok_or(HelixError::InvalidResponse("Ungültige VOD-Dauer"))?;
        digits.clear();
    }
    if value.is_empty() || !digits.is_empty() {
        return Err(HelixError::InvalidResponse("Ungültige VOD-Dauer"));
    }
    Ok(seconds)
}

pub async fn liste_vods(
    client: &HelixClient,
    twitch_user_id: &str,
) -> Result<Vec<VodEintrag>, VodArchiveError> {
    if twitch_user_id.is_empty() || !twitch_user_id.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(HelixError::InvalidResponse("Ungültige Twitch-ID").into());
    }
    let mut vods = Vec::new();
    let mut cursor = None;
    let mut cursors = HashSet::new();
    loop {
        let mut request = client.get("/videos").await?.query(&[
            ("user_id", twitch_user_id),
            ("type", "archive"),
            ("first", "100"),
            ("sort", "time"),
        ]);
        if let Some(after) = &cursor {
            request = request.query(&[("after", after)]);
        }
        let response = request.send().await.map_err(HelixError::from)?;
        if !response.status().is_success() {
            return Err(HelixError::Status {
                status: response.status().as_u16(),
            }
            .into());
        }
        let page: VideosResponse = response.json().await.map_err(HelixError::from)?;
        for video in page.data {
            if video.user_id != twitch_user_id
                || video.id.is_empty()
                || !video.id.bytes().all(|byte| byte.is_ascii_digit())
            {
                return Err(HelixError::InvalidResponse(
                    "VOD-Antwort enthält eine fremde oder ungültige ID",
                )
                .into());
            }
            vods.push(VodEintrag {
                twitch_id: format!("v{}", video.id),
                title: video.title,
                duration_sec: duration_seconds(&video.duration)?,
            });
        }
        cursor = page.pagination.cursor.filter(|cursor| !cursor.is_empty());
        let Some(after) = &cursor else {
            return Ok(vods);
        };
        if !cursors.insert(after.clone()) || cursors.len() > 1000 {
            return Err(
                HelixError::InvalidResponse("VOD-Pagination wird nicht abgeschlossen").into(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tb_transport_twitch::HelixConfig;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn client(server: &MockServer) -> HelixClient {
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "test", "expires_in": 3600
            })))
            .mount(server)
            .await;
        HelixClient::new(HelixConfig {
            client_id: "test".into(),
            client_secret: "test".into(),
            token_url: format!("{}/token", server.uri()),
            helix_base: format!("{}/helix", server.uri()),
        })
        .unwrap()
    }

    #[tokio::test]
    async fn archiv_liste_bleibt_auf_id_und_paginiert() {
        let server = MockServer::start().await;
        let client = client(&server).await;
        Mock::given(method("GET"))
            .and(path("/helix/videos"))
            .and(query_param("user_id", "42"))
            .and(query_param("type", "archive"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"id":"7", "user_id":"42", "title":"Umbenannt", "duration":"1h2m3s"}],
                "pagination": {"cursor":"next"}
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/helix/videos"))
            .and(query_param("user_id", "42"))
            .and(query_param("after", "next"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"id":"6", "user_id":"42", "title":"Alter Titel", "duration":"42s"}]
            })))
            .expect(1)
            .with_priority(1)
            .mount(&server)
            .await;
        let vods = liste_vods(&client, "42").await.unwrap();
        assert_eq!(
            vods.iter()
                .map(|vod| vod.twitch_id.as_str())
                .collect::<Vec<_>>(),
            ["v7", "v6"]
        );
        assert_eq!(vods[0].duration_sec, 3723);
        let requests = server.received_requests().await.unwrap();
        assert!(requests.iter().all(|request| !request
            .url
            .query_pairs()
            .any(|(key, _)| key.contains("login"))));
    }

    #[tokio::test]
    async fn fremde_fehlende_id_und_api_fehler_geben_keine_vods_frei() {
        for (status, video) in [
            (
                200,
                serde_json::json!({"id":"7", "user_id":"99", "title":"Fremd", "duration":"1h"}),
            ),
            (
                200,
                serde_json::json!({"id":"7", "title":"Ohne ID", "duration":"1h"}),
            ),
            (403, serde_json::json!({})),
        ] {
            let server = MockServer::start().await;
            let client = client(&server).await;
            Mock::given(method("GET"))
                .and(path("/helix/videos"))
                .respond_with(
                    ResponseTemplate::new(status)
                        .set_body_json(serde_json::json!({"data":[video]})),
                )
                .mount(&server)
                .await;
            assert!(liste_vods(&client, "42").await.is_err());
        }
    }

    #[tokio::test]
    async fn wiederholter_cursor_und_ungueltige_id_brechen_ab() {
        let server = MockServer::start().await;
        let client = client(&server).await;
        Mock::given(method("GET"))
            .and(path("/helix/videos"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [], "pagination":{"cursor":"same"}
            })))
            .expect(2)
            .mount(&server)
            .await;
        assert!(liste_vods(&client, "42").await.is_err());
        assert!(liste_vods(&client, "name").await.is_err());
    }

    #[test]
    fn vod_dauer_ist_geprueft() {
        assert_eq!(duration_seconds("12h0m0s").unwrap(), 43200);
        for invalid in ["", "12", "1d", "h", "9223372036854775807h"] {
            assert!(duration_seconds(invalid).is_err());
        }
    }
}
