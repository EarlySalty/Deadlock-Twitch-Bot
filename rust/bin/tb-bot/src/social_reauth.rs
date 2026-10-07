use async_trait::async_trait;
use sqlx::PgPool;
use tb_raid::token_lifecycle::{discord_user_id_for, TokenLifecycleNotifier};
use tb_social_media::reauth::SocialReauthNotifier;

use crate::token_lifecycle_wiring::BrokerTokenLifecycleNotifier;

#[cfg(test)]
#[path = "../../../test-support/database.rs"]
mod test_database;

const SOCIAL_DASHBOARD_URL: &str = "https://deutsche-deadlock-community.de/social-media-admin";

pub(crate) struct SocialConnectionNotifier {
    pool: PgPool,
    dm: BrokerTokenLifecycleNotifier,
}

impl SocialConnectionNotifier {
    pub(crate) fn new(pool: PgPool, dm: BrokerTokenLifecycleNotifier) -> Self {
        Self { pool, dm }
    }
}

fn reconnect_text(platform: &str, incident_at: chrono::DateTime<chrono::Utc>) -> Option<String> {
    let name = match platform {
        "tiktok" => "TikTok",
        "youtube" => "YouTube",
        "instagram" => "Instagram",
        _ => return None,
    };
    Some(format!(
        "Deine Verbindung zu {name} muss erneuert werden. Bitte verbinde {name} im Social-Media-Dashboard neu: {SOCIAL_DASHBOARD_URL}?hinweis={}",
        incident_at.timestamp_micros(),
    ))
}

#[async_trait]
impl SocialReauthNotifier for SocialConnectionNotifier {
    async fn notify(
        &self,
        twitch_user_id: &str,
        platform: &str,
        _needs_reauth: bool,
        incident_at: chrono::DateTime<chrono::Utc>,
    ) -> bool {
        let Some(content) = reconnect_text(platform, incident_at) else {
            return false;
        };
        let Some(discord_user_id) = discord_user_id_for(&self.pool, twitch_user_id, "").await
        else {
            return false;
        };
        self.dm.send_user_dm(&discord_user_id, &content).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn incident() -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::parse_from_rfc3339("2026-10-07T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc)
    }

    #[tokio::test]
    async fn dm_uses_twitch_identity_and_existing_broker_port() {
        use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
        use std::str::FromStr;
        use tb_transport_discord::BrokerRelay;
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let dsn = test_database::database_url().expect("Test database required");
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .unwrap();
        sqlx::query("DROP SCHEMA IF EXISTS t_sm_social_dm_path CASCADE")
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query("CREATE SCHEMA t_sm_social_dm_path")
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
        let options = PgConnectOptions::from_str(&dsn)
            .unwrap()
            .options([("search_path", "t_sm_social_dm_path")]);
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(options)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE twitch_streamer_identities (twitch_user_id TEXT, twitch_login TEXT, discord_user_id TEXT, updated_at TIMESTAMPTZ)")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_streamer_identities VALUES ('42', 'reassigned_login', '555', '2026-10-06T00:00:00Z'), ('99', 'reassigned_login', '777', '2026-10-07T00:00:00Z')")
            .execute(&pool).await.unwrap();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/internal/master/v1/discord/send-dm"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true, "result": {"message_id": "test-dm-1"}
            })))
            .mount(&server)
            .await;
        let relay = BrokerRelay::new(&tb_config::BrokerConfig {
            base_url: server.uri(),
            token: "test-only".into(),
        })
        .unwrap();
        let dm = BrokerTokenLifecycleNotifier::from_config(
            Some(relay),
            &tb_config::discord::TokenLifecycle::default(),
        );
        let notifier = SocialConnectionNotifier::new(pool, dm);
        assert!(notifier.notify("42", "tiktok", true, incident()).await);
        assert!(!notifier.notify("404", "tiktok", true, incident()).await);
        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
        assert_eq!(body["user_id"], 555);
        assert!(body["content"]
            .as_str()
            .unwrap()
            .contains(SOCIAL_DASHBOARD_URL));
        assert!(requests[0].headers.contains_key("x-idempotency-key"));
        server.reset().await;
        Mock::given(method("POST"))
            .and(path("/internal/master/v1/discord/send-dm"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": false, "result": {"message_id": ""}
            })))
            .mount(&server)
            .await;
        assert!(!notifier.notify("42", "tiktok", false, incident()).await);
    }

    #[test]
    fn dm_contains_dashboard_link_and_no_internal_terms() {
        for platform in ["tiktok", "youtube", "instagram"] {
            for _needs_reauth in [false, true] {
                let text = reconnect_text(platform, incident()).unwrap();
                assert!(text.contains(SOCIAL_DASHBOARD_URL));
                for forbidden in ["Token", "Autorisierung", "OAuth", "\u{2014}"] {
                    assert!(!text.contains(forbidden));
                }
            }
        }
        assert!(reconnect_text("unknown", incident()).is_none());
        assert_ne!(
            reconnect_text("tiktok", incident()),
            reconnect_text("tiktok", incident() + chrono::Duration::days(1))
        );
    }
}
