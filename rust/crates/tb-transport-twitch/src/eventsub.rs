//! Helix-EventSub-Subscription-Verwaltung (Webhook-Transport):
//! Anlegen (409 = already exists = Erfolg, wie Python
//! `subscribe_eventsub_webhook`), Auflisten (Cursor-Pagination) und Löschen.

use serde::Deserialize;
use serde_json::Value;

use crate::client::{HelixClient, HelixError};

/// Eine bei Twitch registrierte EventSub-Subscription.
#[derive(Debug, Clone, Deserialize)]
pub struct EventSubSubscription {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub status: String,
    #[serde(rename = "type", default)]
    pub sub_type: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub condition: Value,
    #[serde(default)]
    pub transport: TransportInfo,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct TransportInfo {
    #[serde(default)]
    pub method: String,
    #[serde(default)]
    pub callback: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SubscriptionsResponse {
    #[serde(default)]
    data: Vec<EventSubSubscription>,
    #[serde(default)]
    pagination: Pagination,
}

#[derive(Debug, Default, Deserialize)]
struct Pagination {
    #[serde(default)]
    cursor: Option<String>,
}

/// Ergebnis des Anlegens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateOutcome {
    Created,
    /// 409 — Subscription existiert bereits (als Erfolg behandelt).
    AlreadyExists,
}

impl HelixClient {
    /// Legt eine Webhook-Subscription an. `bearer_override` erlaubt
    /// User-/Bot-Tokens (Python-Parität); `None` = App-Token.
    /// Non-2xx außer 409 → [`HelixError::Status`].
    pub async fn create_eventsub_webhook_subscription(
        &self,
        sub_type: &str,
        version: &str,
        condition: &Value,
        callback: &str,
        secret: &str,
        bearer_override: Option<&str>,
    ) -> Result<CreateOutcome, HelixError> {
        let payload = serde_json::json!({
            "type": sub_type,
            "version": version,
            "condition": condition,
            "transport": {
                "method": "webhook",
                "callback": callback,
                "secret": secret,
            },
        });
        let mut builder = self.post("/eventsub/subscriptions").await?;
        if let Some(token) = bearer_override.map(str::trim).filter(|t| !t.is_empty()) {
            builder = builder.header("Authorization", format!("Bearer {token}"));
        }
        let resp = builder.json(&payload).send().await?;
        let status = resp.status();
        if status.as_u16() == 409 {
            return Ok(CreateOutcome::AlreadyExists);
        }
        if !status.is_success() {
            return Err(HelixError::Status {
                status: status.as_u16(),
            });
        }
        Ok(CreateOutcome::Created)
    }

    /// Listet alle Subscriptions (optional nach Status gefiltert),
    /// folgt der Cursor-Pagination. Obergrenze: 100 Seiten (= 10 000 Subs).
    pub async fn list_eventsub_subscriptions(
        &self,
        status: Option<&str>,
    ) -> Result<Vec<EventSubSubscription>, HelixError> {
        /// Max. Seiten, um eine Endlosschleife bei defektem API-Cursor auszuschließen.
        const MAX_PAGES: u32 = 100;
        let mut out = Vec::new();
        let mut after: Option<String> = None;
        let mut pages = 0u32;
        loop {
            if pages >= MAX_PAGES {
                tracing::warn!(
                    "list_eventsub_subscriptions: Seitenlimit ({MAX_PAGES}) erreicht, \
                     Pagination abgebrochen"
                );
                break;
            }
            pages += 1;
            let mut params: Vec<(&str, String)> = Vec::new();
            if let Some(status) = status.map(str::trim).filter(|s| !s.is_empty()) {
                params.push(("status", status.to_string()));
            }
            if let Some(cursor) = &after {
                params.push(("after", cursor.clone()));
            }
            let resp = self
                .get("/eventsub/subscriptions")
                .await?
                .query(&params)
                .send()
                .await?;
            if !resp.status().is_success() {
                return Err(HelixError::Status {
                    status: resp.status().as_u16(),
                });
            }
            let body: SubscriptionsResponse = resp.json().await?;
            let empty = body.data.is_empty();
            out.extend(body.data);
            after = body.pagination.cursor;
            if after.is_none() || empty {
                break;
            }
        }
        Ok(out)
    }

    /// Löscht eine Subscription. `true` = gelöscht, `false` = unbekannt (404).
    pub async fn delete_eventsub_subscription(&self, id: &str) -> Result<bool, HelixError> {
        let resp = self
            .delete("/eventsub/subscriptions")
            .await?
            .query(&[("id", id)])
            .send()
            .await?;
        match resp.status().as_u16() {
            204 => Ok(true),
            404 => Ok(false),
            status if resp.status().is_success() => {
                tracing::debug!(status, "EventSub-Delete mit unerwartetem Erfolgsstatus");
                Ok(true)
            }
            status => Err(HelixError::Status { status }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CreateOutcome;
    use crate::client::{HelixClient, HelixConfig};
    use wiremock::matchers::{body_partial_json, method, path, query_param};
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
    async fn create_sendet_webhook_transport_und_behandelt_409() {
        let server = MockServer::start().await;
        let client = client_with(&server).await;
        Mock::given(method("POST"))
            .and(path("/helix/eventsub/subscriptions"))
            .and(body_partial_json(serde_json::json!({
                "type": "stream.offline",
                "transport": {"method": "webhook", "callback": "https://cb/x"}
            })))
            .respond_with(ResponseTemplate::new(202))
            .expect(1)
            .up_to_n_times(1)
            .mount(&server)
            .await;
        let outcome = client
            .create_eventsub_webhook_subscription(
                "stream.offline",
                "1",
                &serde_json::json!({"broadcaster_user_id": "42"}),
                "https://cb/x",
                "geheim",
                None,
            )
            .await
            .unwrap();
        assert_eq!(outcome, CreateOutcome::Created);

        // 409 → AlreadyExists statt Fehler.
        Mock::given(method("POST"))
            .and(path("/helix/eventsub/subscriptions"))
            .respond_with(ResponseTemplate::new(409))
            .mount(&server)
            .await;
        let outcome = client
            .create_eventsub_webhook_subscription(
                "stream.offline",
                "1",
                &serde_json::json!({"broadcaster_user_id": "42"}),
                "https://cb/x",
                "geheim",
                None,
            )
            .await
            .unwrap();
        assert_eq!(outcome, CreateOutcome::AlreadyExists);
    }

    #[tokio::test]
    async fn list_und_delete() {
        let server = MockServer::start().await;
        let client = client_with(&server).await;
        Mock::given(method("GET"))
            .and(path("/helix/eventsub/subscriptions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{
                    "id": "sub-1", "status": "enabled", "type": "stream.offline",
                    "version": "1",
                    "condition": {"broadcaster_user_id": "42"},
                    "transport": {"method": "webhook", "callback": "https://cb/x"}
                }],
                "pagination": {}
            })))
            .mount(&server)
            .await;
        let subs = client.list_eventsub_subscriptions(None).await.unwrap();
        assert_eq!(subs.len(), 1);
        assert_eq!(subs[0].sub_type, "stream.offline");
        assert_eq!(subs[0].transport.callback.as_deref(), Some("https://cb/x"));

        Mock::given(method("DELETE"))
            .and(path("/helix/eventsub/subscriptions"))
            .and(query_param("id", "sub-1"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;
        assert!(client.delete_eventsub_subscription("sub-1").await.unwrap());
    }
}
