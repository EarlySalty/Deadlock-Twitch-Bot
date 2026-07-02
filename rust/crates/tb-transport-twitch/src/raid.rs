//! Helix-Raid-Endpoints (`/raids`) — Start + Cancel. Beide brauchen den
//! **User-Token des Quell-Broadcasters** (nicht den App-Token), deshalb
//! `bearer_override`. Port von `raid/executor.py` `start_raid`/`cancel_raid`.

use crate::client::{HelixClient, HelixError};

/// Ergebnis einer Raid-API-Operation (Python: `(success, error_message)`).
pub type RaidApiResult = Result<(), String>;

impl HelixClient {
    /// Startet einen Raid `from → to` (`POST /raids`). 200 = Erfolg.
    /// `user_token` ist der Access-Token des Quell-Broadcasters.
    pub async fn start_raid(
        &self,
        from_broadcaster_id: &str,
        to_broadcaster_id: &str,
        user_token: &str,
    ) -> Result<RaidApiResult, HelixError> {
        let resp = self
            .post_with_user_token("/raids", user_token)
            .query(&[
                ("from_broadcaster_id", from_broadcaster_id),
                ("to_broadcaster_id", to_broadcaster_id),
            ])
            .send()
            .await?;
        if resp.status().as_u16() == 200 {
            Ok(Ok(()))
        } else {
            let status = resp.status().as_u16();
            let body = match resp.text().await {
                Ok(body) => body,
                Err(error) => {
                    tracing::warn!(%error, status, "Twitch Raid: Fehlerbody nicht lesbar");
                    String::new()
                }
            };
            let snippet: String = body.chars().take(200).collect();
            Ok(Err(format!("Raid API failed: HTTP {status}: {snippet}")))
        }
    }

    /// Bricht einen ausstehenden Raid ab (`DELETE /raids`). 200/204 = Erfolg.
    /// Funktioniert nur während des Countdowns.
    pub async fn cancel_raid(
        &self,
        broadcaster_id: &str,
        user_token: &str,
    ) -> Result<RaidApiResult, HelixError> {
        let resp = self
            .delete_with_user_token("/raids", user_token)
            .query(&[("broadcaster_id", broadcaster_id)])
            .send()
            .await?;
        match resp.status().as_u16() {
            200 | 204 => Ok(Ok(())),
            status => {
                let body = match resp.text().await {
                    Ok(body) => body,
                    Err(error) => {
                        tracing::warn!(%error, status, "Twitch Cancel-Raid: Fehlerbody nicht lesbar");
                        String::new()
                    }
                };
                let snippet: String = body.chars().take(200).collect();
                Ok(Err(format!(
                    "Cancel-Raid API failed: HTTP {status}: {snippet}"
                )))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::client::{HelixClient, HelixConfig};
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn client_with(server: &MockServer) -> HelixClient {
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "app-tok", "expires_in": 3600
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
    async fn start_raid_nutzt_user_token_und_meldet_erfolg() {
        let server = MockServer::start().await;
        let client = client_with(&server).await;
        Mock::given(method("POST"))
            .and(path("/helix/raids"))
            .and(header("Authorization", "Bearer user-tok"))
            .and(query_param("from_broadcaster_id", "1"))
            .and(query_param("to_broadcaster_id", "2"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        let result = client.start_raid("1", "2", "user-tok").await.unwrap();
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn start_raid_meldet_fehler_bei_non_200() {
        let server = MockServer::start().await;
        let client = client_with(&server).await;
        Mock::given(method("POST"))
            .and(path("/helix/raids"))
            .respond_with(ResponseTemplate::new(429).set_body_string("rate limited"))
            .mount(&server)
            .await;
        let result = client.start_raid("1", "2", "user-tok").await.unwrap();
        assert!(result.unwrap_err().contains("HTTP 429"));
    }

    #[tokio::test]
    async fn cancel_raid_akzeptiert_204() {
        let server = MockServer::start().await;
        let client = client_with(&server).await;
        Mock::given(method("DELETE"))
            .and(path("/helix/raids"))
            .and(query_param("broadcaster_id", "1"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;
        assert!(client.cancel_raid("1", "user-tok").await.unwrap().is_ok());
    }
}
