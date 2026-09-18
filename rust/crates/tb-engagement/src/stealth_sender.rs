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
        if !crate::gate::is_operational_partner(&self.pool, channel_login).await {
            tracing::warn!(
                channel_login,
                broadcaster_id,
                reason = "lurker_write_denied_non_partner",
                "StealthSender: Nicht-Partner-Schreibversuch aus dem Engagement-Pfad blockiert"
            );
            return Some(false);
        }
        self.send_after_gate(broadcaster_id, text, None).await
    }

    /// Eng begrenzter Nicht-Partner-Pfad fuer den Live-Smalltalk-Test.
    ///
    /// Er darf nur in den exakt offenen Smalltalk-Kanal schreiben. Die
    /// Follower-Grenze wird hier unmittelbar vor dem HTTP-Call erneut aus den
    /// Monitoring-Daten geprueft, damit ein Auswahlfehler nie zum Versand wird.
    pub async fn send_smalltalk_live(
        &self,
        broadcaster_id: &str,
        channel_login: &str,
        text: &str,
    ) -> Option<bool> {
        let broadcaster_id = broadcaster_id.trim();
        let channel_login = channel_login.trim().to_lowercase();
        if broadcaster_id.is_empty() || channel_login.is_empty() {
            return Some(false);
        }
        let Ok(text) = crate::llm_chat::sanitize_test_mode_text(text) else {
            return Some(false);
        };
        self.send_after_gate(broadcaster_id, &text, Some(&channel_login))
            .await
    }

    async fn send_after_gate(
        &self,
        broadcaster_id: &str,
        text: &str,
        smalltalk_login: Option<&str>,
    ) -> Option<bool> {
        let broadcaster_id = broadcaster_id.trim();
        let Some(text) = sanitize_chat_text(text, 120) else {
            return Some(false);
        };
        if broadcaster_id.is_empty() {
            return Some(false);
        }

        let (access_token, sender_id) = match self.auth.get_valid_access_token().await {
            Ok(Some(token)) => token,
            Ok(None) => return None,
            Err(error) => {
                tracing::error!(%error, "StealthSender: Sende-Token nicht verfügbar");
                return Some(false);
            }
        };

        let body = serde_json::json!({
            "broadcaster_id": broadcaster_id,
            "sender_id": sender_id,
            "message": text,
        });
        // Token refresh above may await the network. Re-read every live gate
        // afterwards; there is no intervening await before the chat HTTP call.
        if let Some(login) = smalltalk_login {
            match crate::smalltalk_loop_store::live_send_allowed(
                &self.pool,
                broadcaster_id,
                login,
                chrono::Utc::now(),
            )
            .await
            {
                Ok(true) => {}
                Ok(false) => {
                    tracing::warn!(
                        event = "smalltalk_loop.send_blocked",
                        channel = login,
                        broadcaster_id,
                        reason = "live_eligibility"
                    );
                    return Some(false);
                }
                Err(error) => {
                    tracing::warn!(event = "smalltalk_loop.send_blocked", channel = login,
                        broadcaster_id, reason = "database", %error);
                    return Some(false);
                }
            }
        }
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
        if status != 200 && status != 204 {
            let body = resp.text().await.unwrap_or_default();
            tracing::warn!(status, body = %truncate(&body, 200), "StealthSender: Helix-Fehler");
            return Some(false);
        }
        if status == 204 {
            return Some(true);
        }

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
            None => Some(true),
        }
    }
}

/// Kürzt einen String byte-sicher auf `max` Zeichen (für Log-Bodies).
fn truncate(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

#[cfg(test)]
#[path = "../../../test-support/postgres.rs"]
mod isolated_postgres;

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::PgPool;
    use tb_crypto::{aad, FieldCipher};
    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn cipher() -> Arc<FieldCipher> {
        Arc::new(FieldCipher::from_hex_key(&"cd".repeat(32), "k1").unwrap())
    }

    async fn make_pool(_schema: &str) -> Option<(isolated_postgres::TestPostgres, PgPool)> {
        let db = isolated_postgres::TestPostgres::start().await;
        let pool = db.pool.clone();
        sqlx::raw_sql(
            "CREATE TABLE twitch_streamers_partner_state (twitch_user_id TEXT, twitch_login TEXT, is_partner_active INTEGER);
             CREATE TABLE twitch_live_state (twitch_user_id TEXT PRIMARY KEY, streamer_login TEXT,
                 is_live INTEGER, last_game TEXT, last_viewer_count INTEGER);
             CREATE TABLE twitch_partners (twitch_user_id TEXT, twitch_login TEXT, status TEXT);
             CREATE TABLE twitch_partner_outreach (streamer_user_id TEXT, streamer_login TEXT, cooldown_until TEXT);
             CREATE TABLE twitch_raid_blacklist (target_id TEXT, target_login TEXT);
             CREATE TABLE twitch_smalltalk_sessions (
                 id UUID PRIMARY KEY, channel_login TEXT NOT NULL, streamer_user_id TEXT NOT NULL,
                 started_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), ended_at TIMESTAMPTZ
             );
             CREATE TABLE twitch_engagement_settings (
                 channel_login TEXT PRIMARY KEY, enabled BOOLEAN NOT NULL, irc_read BOOLEAN NOT NULL,
                 output_mode TEXT NOT NULL
             );
             CREATE TABLE twitch_stream_sessions (
                 streamer_login TEXT NOT NULL, followers_start INTEGER, followers_end INTEGER,
                 started_at TIMESTAMPTZ
             )",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::raw_sql(include_str!(
            "../../../migrations/20260918024500_smalltalk_candidate_state.sql"
        ))
        .execute(&pool)
        .await
        .unwrap();
        Some((db, pool))
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

    async fn seed_smalltalk_live(pool: &PgPool, followers: i32) {
        sqlx::query(
            "INSERT INTO twitch_smalltalk_sessions (id, channel_login, streamer_user_id, ended_at, live_test)
             VALUES ($1, 'tiny', '123', NULL, TRUE)",
        )
        .bind(uuid::Uuid::new_v4())
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_engagement_settings (channel_login, enabled, irc_read, output_mode)
             VALUES ('tiny', TRUE, TRUE, 'smalltalk_live')",
        )
        .execute(pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO twitch_live_state VALUES ('123', 'tiny', 1, 'Deadlock', 2)")
            .execute(pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO twitch_smalltalk_candidate_state
                (twitch_user_id, channel_login, follower_count, checked_at, source, live_deadlock, live_checked_at)
             VALUES ('123', 'tiny', $1, NOW(), 'helix', TRUE, NOW())",
        )
        .bind(followers)
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn smalltalk_live_sendet_nur_unter_50_follower() {
        let Some((_db, pool)) = make_pool("t_eng_stealth_smalltalk_49").await else {
            return;
        };
        seed_smalltalk_live(&pool, 49).await;
        let auth = auth_with_token(pool.clone(), cipher()).await;
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/helix/chat/messages"))
            .and(header("Client-ID", "cid"))
            .and(body_json(serde_json::json!({
                "broadcaster_id": "123",
                "sender_id": "77",
                "message": "hi"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"is_sent": true}]
            })))
            .expect(1)
            .mount(&server)
            .await;
        let sender = StealthSender::new(auth, "cid".into(), pool)
            .with_helix_url(format!("{}/helix/chat/messages", server.uri()));
        assert_eq!(
            sender.send_smalltalk_live("123", "tiny", "hi").await,
            Some(true)
        );
    }

    #[tokio::test]
    async fn smalltalk_live_blockiert_ab_50_follower_vor_http() {
        let Some((_db, pool)) = make_pool("t_eng_stealth_smalltalk_50").await else {
            return;
        };
        seed_smalltalk_live(&pool, 50).await;
        let auth = auth_with_token(pool.clone(), cipher()).await;
        let server = MockServer::start().await;
        let sender = StealthSender::new(auth, "cid".into(), pool)
            .with_helix_url(format!("{}/helix/chat/messages", server.uri()));
        assert_eq!(
            sender.send_smalltalk_live("123", "tiny", "hi").await,
            Some(false)
        );
        assert_eq!(server.received_requests().await.unwrap().len(), 0);
    }

    #[tokio::test]
    async fn live_send_revalidiert_alle_gates_ohne_chat_http() {
        let (_db, pool) = make_pool("live_gate_matrix").await.unwrap();
        let auth = auth_with_token(pool.clone(), cipher()).await;
        let server = MockServer::start().await;
        let sender = StealthSender::new(auth, "cid".into(), pool.clone())
            .with_helix_url(format!("{}/helix/chat/messages", server.uri()));
        let mutations = [
            "UPDATE twitch_smalltalk_candidate_state SET follower_count=NULL",
            "UPDATE twitch_smalltalk_candidate_state SET follower_count=50",
            "UPDATE twitch_smalltalk_candidate_state SET checked_at=NOW()-INTERVAL '12 hours 1 second'",
            "UPDATE twitch_smalltalk_candidate_state SET checked_at=NOW()+INTERVAL '1 minute'",
            "UPDATE twitch_smalltalk_candidate_state SET live_checked_at=NOW()-INTERVAL '91 seconds'",
            "UPDATE twitch_smalltalk_candidate_state SET live_checked_at=NOW()+INTERVAL '1 minute'",
            "UPDATE twitch_smalltalk_candidate_state SET live_deadlock=FALSE",
            "UPDATE twitch_smalltalk_candidate_state SET channel_login='anderer'",
            "UPDATE twitch_smalltalk_candidate_state SET cooldown_until=NOW()+INTERVAL '1 hour'",
            "UPDATE twitch_live_state SET is_live=0",
            "UPDATE twitch_live_state SET last_game='Just Chatting'",
            "UPDATE twitch_live_state SET twitch_user_id='999'",
            "UPDATE twitch_live_state SET streamer_login='umbenannt'",
            "UPDATE twitch_smalltalk_sessions SET ended_at=NOW()",
            "UPDATE twitch_smalltalk_sessions SET streamer_user_id='999'",
            "UPDATE twitch_smalltalk_sessions SET started_at=NOW()-INTERVAL '61 minutes'",
            "UPDATE twitch_smalltalk_sessions SET started_at=NOW()+INTERVAL '1 minute'",
            "UPDATE twitch_smalltalk_sessions SET live_test=FALSE",
            "UPDATE twitch_engagement_settings SET enabled=FALSE",
            "UPDATE twitch_engagement_settings SET irc_read=FALSE",
            "UPDATE twitch_engagement_settings SET output_mode='live'",
            "INSERT INTO twitch_raid_blacklist VALUES ('123', 'anderer')",
            "INSERT INTO twitch_raid_blacklist VALUES ('999', 'tiny')",
            "INSERT INTO twitch_partners VALUES ('123', 'tiny', 'active')",
            "INSERT INTO twitch_streamers_partner_state VALUES ('123', 'tiny', 1)",
            "INSERT INTO twitch_partner_outreach VALUES ('123', 'tiny', (NOW()+INTERVAL '1 hour')::text)",
            "INSERT INTO twitch_partner_outreach VALUES ('123', 'tiny', 'invalid-date')",
        ];
        for mutation in mutations {
            sqlx::raw_sql("TRUNCATE twitch_smalltalk_sessions, twitch_engagement_settings, twitch_smalltalk_candidate_state, twitch_live_state, twitch_raid_blacklist, twitch_partners, twitch_streamers_partner_state, twitch_partner_outreach CASCADE")
                .execute(&pool).await.unwrap();
            seed_smalltalk_live(&pool, 49).await;
            sqlx::raw_sql(mutation).execute(&pool).await.unwrap();
            assert_eq!(
                sender.send_smalltalk_live("123", "tiny", "hi").await,
                Some(false),
                "{mutation}"
            );
            assert!(
                server.received_requests().await.unwrap().is_empty(),
                "HTTP leaked: {mutation}"
            );
        }
    }

    #[tokio::test]
    async fn live_send_blockiert_werbung_auch_bei_direktem_senderaufruf() {
        let (_db, pool) = make_pool("live_no_pitch").await.unwrap();
        seed_smalltalk_live(&pool, 0).await;
        let auth = auth_with_token(pool.clone(), cipher()).await;
        let server = MockServer::start().await;
        let sender = StealthSender::new(auth, "cid".into(), pool)
            .with_helix_url(format!("{}/helix/chat/messages", server.uri()));
        for text in [
            "komm in unseren Discord",
            "https://example.org",
            "werde unser Partner",
        ] {
            assert_eq!(
                sender.send_smalltalk_live("123", "tiny", text).await,
                Some(false)
            );
        }
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn follower_aenderung_waehrend_token_refresh_blockiert_chat_http() {
        let (_db, pool) = make_pool("live_token_race").await.unwrap();
        seed_smalltalk_live(&pool, 49).await;
        let c = cipher();
        auth_with_token(pool.clone(), c.clone()).await;
        sqlx::query("UPDATE twitch_engagement_sender_auth SET token_expires_at=1")
            .execute(&pool)
            .await
            .unwrap();
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/token"))
            .respond_with(ResponseTemplate::new(200)
                .set_delay(std::time::Duration::from_millis(500))
                .set_body_json(serde_json::json!({"access_token":"acc", "refresh_token":"ref", "expires_in":3600, "token_type":"bearer"})))
            .expect(1).mount(&server).await;
        let auth = Arc::new(
            SenderAuthStore::new(pool.clone(), c, "cid".into(), "csec".into())
                .with_token_url(format!("{}/token", server.uri())),
        );
        let sender = StealthSender::new(auth, "cid".into(), pool.clone())
            .with_helix_url(format!("{}/helix/chat/messages", server.uri()));
        let invalidate = async {
            tokio::time::timeout(std::time::Duration::from_secs(2), async {
                while server.received_requests().await.unwrap().is_empty() {
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                }
            })
            .await
            .unwrap();
            sqlx::query("UPDATE twitch_smalltalk_candidate_state SET follower_count=50")
                .execute(&pool)
                .await
                .unwrap();
        };
        let (sent, _) = tokio::join!(sender.send_smalltalk_live("123", "tiny", "hi"), invalidate);
        assert_eq!(sent, Some(false));
        assert_eq!(
            server.received_requests().await.unwrap().len(),
            1,
            "only token refresh, never chat"
        );
    }

    #[tokio::test]
    async fn none_ohne_account() {
        let Some((_db, pool)) = make_pool("t_eng_stealth_none").await else {
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
        let Some((_db, pool)) = make_pool("t_eng_stealth_empty").await else {
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
        let Some((_db, pool)) = make_pool("t_eng_stealth_non_partner").await else {
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
        let Some((_db, pool)) = make_pool("t_eng_stealth_sent").await else {
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
        let Some((_db, pool)) = make_pool("t_eng_stealth_drop").await else {
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
        let Some((_db, pool)) = make_pool("t_eng_stealth_err").await else {
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
