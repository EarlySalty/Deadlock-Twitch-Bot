//! Chat-Send über den Engagement-Sende-Account (Port von
//! `bot/engagement/stealth_sender.py`).
//!
//! Spiegelt den Helix-Send-Pfad, nutzt aber NICHT die zentrale Bot-Identität,
//! sondern Token + User-ID des separaten Smoke-Accounts (siehe [`SenderAuthStore`]).
//! Damit erscheint die AI-Antwort im Chat als unauffälliger Zuschauer statt als
//! „der Bot".
//!
//! [`StealthSender::send`] ist best-effort: `Some(true)` nur bei bestätigtem
//! Versand (`is_sent`), `Some(false)` bei Drop/HTTP-Fehler, `None` wenn kein
//! Account onboarded ist (Aufrufer fällt auf Default zurück).

use std::sync::Arc;

use crate::llm_chat::sanitize_chat_text;
use crate::sender_auth::SenderAuthStore;

const DEFAULT_HELIX_URL: &str = "https://api.twitch.tv/helix/chat/messages";

/// Versendet Engagement-Antworten über den Smoke-Account.
pub struct StealthSender {
    auth: Arc<SenderAuthStore>,
    pool: sqlx::PgPool,
    http: reqwest::Client,
    client_id: String,
    helix_url: String,
}

impl StealthSender {
    pub fn new(auth: Arc<SenderAuthStore>, client_id: String, pool: sqlx::PgPool) -> Self {
        Self {
            auth,
            pool,
            http: reqwest::Client::new(),
            client_id,
            helix_url: DEFAULT_HELIX_URL.to_string(),
        }
    }

    /// Setzt den Helix-Endpoint (für Tests; produktiv bleibt der Default).
    pub fn with_helix_url(mut self, url: impl Into<String>) -> Self {
        self.helix_url = url.into();
        self
    }

    /// Sendet `text` als Smoke-Account nur in operativ aktive Partner-Kanäle.
    ///
    /// `Some(true)` – Nachricht bestätigt versendet.
    /// `Some(false)` – kein aktiver Partner (ohne HTTP-Call) oder Versand fehlgeschlagen/gedroppt.
    /// `None` – kein Sende-Account onboarded (Aufrufer soll Fallback nutzen).
    pub async fn send(
        &self,
        broadcaster_id: &str,
        channel_login: &str,
        text: &str,
    ) -> Option<bool> {
        let broadcaster_id = broadcaster_id.trim();
        if !crate::gate::is_operational_partner(&self.pool, channel_login).await {
            tracing::warn!(
                channel_login,
                broadcaster_id,
                reason = "lurker_write_denied_non_partner",
                "StealthSender: Nicht-Partner-Schreibversuch aus dem Engagement-Pfad blockiert"
            );
            return Some(false);
        }
        let Some(text) = sanitize_chat_text(text, 120) else {
            return Some(false);
        };
        if broadcaster_id.is_empty() {
            return Some(false);
        }

        let (access_token, sender_id) = self.auth.get_valid_access_token().await?;

        let body = serde_json::json!({
            "broadcaster_id": broadcaster_id,
            "sender_id": sender_id,
            "message": text,
        });
        let resp = match self
            .http
            .post(&self.helix_url)
            .header("Client-ID", &self.client_id)
            .header("Authorization", format!("Bearer {access_token}"))
            .json(&body)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error = %e, "StealthSender: Send fehlgeschlagen");
                return Some(false);
            }
        };

        let status = resp.status().as_u16();
        // Alles außer 200/204 → Fehlschlag (Python `if r.status not in {200, 204}`).
        if status != 200 && status != 204 {
            let body = resp.text().await.unwrap_or_default();
            tracing::warn!(status, body = %truncate(&body, 200), "StealthSender: Helix-Fehler");
            return Some(false);
        }
        if status == 204 {
            return Some(true);
        }

        // HTTP 200 kann trotzdem einen serverseitigen Drop bedeuten.
        let payload: serde_json::Value = resp.json().await.unwrap_or(serde_json::Value::Null);
        let first = payload
            .get("data")
            .and_then(serde_json::Value::as_array)
            .and_then(|a| a.first());
        match first
            .and_then(|d| d.get("is_sent"))
            .and_then(serde_json::Value::as_bool)
        {
            Some(true) => Some(true),
            Some(false) => {
                let drop = first
                    .and_then(|d| d.get("drop_reason"))
                    .map(ToString::to_string)
                    .unwrap_or_default();
                tracing::warn!(drop = %drop, "StealthSender: Nachricht gedroppt");
                Some(false)
            }
            // Kein eindeutiges is_sent → optimistisch True (Helix-Erfolg).
            None => Some(true),
        }
    }
}

/// Kürzt einen String byte-sicher auf `max` Zeichen (für Log-Bodies).
fn truncate(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use sqlx::PgPool;
    use std::str::FromStr;
    use tb_crypto::{aad, FieldCipher};
    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn cipher() -> Arc<FieldCipher> {
        Arc::new(FieldCipher::from_hex_key(&"cd".repeat(32), "k1").unwrap())
    }

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn)
            .unwrap()
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(opts)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE twitch_streamers_partner_state (twitch_login TEXT, is_partner_active INTEGER)",
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    async fn add_active_partner(pool: &PgPool, channel_login: &str) {
        sqlx::query(
            "INSERT INTO twitch_streamers_partner_state (twitch_login, is_partner_active) VALUES ($1, 1)",
        )
        .bind(channel_login)
        .execute(pool)
        .await
        .unwrap();
    }

    /// SenderAuthStore mit einem frischen, entschlüsselbaren Token in der DB.
    async fn auth_with_token(pool: PgPool, c: Arc<FieldCipher>) -> Arc<SenderAuthStore> {
        let s = SenderAuthStore::new(pool.clone(), c.clone(), "cid".into(), "csec".into());
        s.ensure_table().await;
        let access_enc = c
            .encrypt_field("acc", &aad::engagement_sender("access_token", "77"))
            .unwrap();
        let refresh_enc = c
            .encrypt_field("ref", &aad::engagement_sender("refresh_token", "77"))
            .unwrap();
        let future = chrono::Utc::now().timestamp() + 3600;
        sqlx::query(
            "INSERT INTO twitch_engagement_sender_auth (twitch_user_id, twitch_login, \
             access_token_enc, refresh_token_enc, scopes, token_expires_at) \
             VALUES ('77', 'smoke', $1, $2, 's', $3)",
        )
        .bind(access_enc)
        .bind(refresh_enc)
        .bind(future)
        .execute(&pool)
        .await
        .unwrap();
        Arc::new(s)
    }

    #[tokio::test]
    async fn none_ohne_account() {
        let Some(pool) = make_pool("t_eng_stealth_none").await else {
            return;
        };
        add_active_partner(&pool, "partner").await;
        let s = SenderAuthStore::new(pool.clone(), cipher(), "cid".into(), "csec".into());
        s.ensure_table().await;
        let sender = StealthSender::new(Arc::new(s), "cid".into(), pool);
        // Kein Token onboarded → None (Fallback-Signal).
        assert_eq!(sender.send("123", "partner", "hi").await, None);
    }

    #[tokio::test]
    async fn leerer_input_ist_false() {
        let Some(pool) = make_pool("t_eng_stealth_empty").await else {
            return;
        };
        add_active_partner(&pool, "partner").await;
        let auth = auth_with_token(pool.clone(), cipher()).await;
        let sender = StealthSender::new(auth, "cid".into(), pool);
        assert_eq!(sender.send("", "partner", "hi").await, Some(false));
        assert_eq!(sender.send("123", "partner", "   ").await, Some(false));
    }

    #[tokio::test]
    async fn nicht_partner_wird_blockiert_ohne_http() {
        let Some(pool) = make_pool("t_eng_stealth_non_partner").await else {
            return;
        };
        let auth = auth_with_token(pool.clone(), cipher()).await;
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/helix/chat/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"is_sent": true}]
            })))
            .expect(0)
            .mount(&server)
            .await;
        let sender = StealthSender::new(auth, "cid".into(), pool)
            .with_helix_url(format!("{}/helix/chat/messages", server.uri()));

        assert_eq!(
            sender.send("123", "kein_partner", "hallo").await,
            Some(false)
        );
    }

    #[tokio::test]
    async fn aktiver_partner_sendet() {
        let Some(pool) = make_pool("t_eng_stealth_sent").await else {
            return;
        };
        add_active_partner(&pool, "partner").await;
        let c = cipher();
        let auth = auth_with_token(pool.clone(), c).await;
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/helix/chat/messages"))
            .and(header("Client-ID", "cid"))
            .and(header("Authorization", "Bearer acc"))
            .and(body_json(serde_json::json!({
                "broadcaster_id": "123",
                "sender_id": "77",
                "message": "hallo, welt",
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"message_id": "m", "is_sent": true}]
            })))
            .expect(1)
            .mount(&server)
            .await;
        let sender = StealthSender::new(auth, "cid".into(), pool)
            .with_helix_url(format!("{}/helix/chat/messages", server.uri()));
        assert_eq!(
            sender.send("123", "partner", "hallo 😭 — welt!").await,
            Some(true)
        );
    }

    #[tokio::test]
    async fn is_sent_false_ist_drop() {
        let Some(pool) = make_pool("t_eng_stealth_drop").await else {
            return;
        };
        add_active_partner(&pool, "partner").await;
        let auth = auth_with_token(pool.clone(), cipher()).await;
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/helix/chat/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"is_sent": false, "drop_reason": {"code": "msg_rejected"}}]
            })))
            .mount(&server)
            .await;
        let sender = StealthSender::new(auth, "cid".into(), pool)
            .with_helix_url(format!("{}/helix/chat/messages", server.uri()));
        assert_eq!(sender.send("123", "partner", "hallo").await, Some(false));
    }

    #[tokio::test]
    async fn http_error_ist_false() {
        let Some(pool) = make_pool("t_eng_stealth_err").await else {
            return;
        };
        add_active_partner(&pool, "partner").await;
        let auth = auth_with_token(pool.clone(), cipher()).await;
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/helix/chat/messages"))
            .respond_with(ResponseTemplate::new(401).set_body_string("unauthorized"))
            .mount(&server)
            .await;
        let sender = StealthSender::new(auth, "cid".into(), pool)
            .with_helix_url(format!("{}/helix/chat/messages", server.uri()));
        assert_eq!(sender.send("123", "partner", "hallo").await, Some(false));
    }
}
