use std::{sync::Arc, time::Duration};

use sqlx::PgPool;
use tb_config::BrokerConfig;
use tb_transport_discord::{BrokerRelay, DeleteMessage, DiscordBackend, DiscordError};

use crate::task_supervisor::TaskSupervisor;

const RETENTION_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const RETENTION_LIMIT: i64 = 50;
const RETENTION_DELETE_REASON: &str = "Ricky-Review: Aufbewahrungsfrist abgelaufen";

pub fn start(supervisor: &TaskSupervisor, pool: PgPool, broker: &BrokerConfig) {
    let discord: Arc<dyn DiscordBackend> = match BrokerRelay::new(broker) {
        Ok(relay) => Arc::new(relay),
        Err(error) => {
            tracing::warn!(%error, "Crew-Archivpflege: Discord-Verbindung nicht verfügbar");
            return;
        }
    };
    supervisor.spawn("crew_archive_retention", async move {
        loop {
            if let Err(error) = cleanup_once(&pool, discord.as_ref()).await {
                tracing::warn!(%error, "Crew-Archivpflege fehlgeschlagen");
            }
            tokio::time::sleep(RETENTION_INTERVAL).await;
        }
    });
}

async fn cleanup_once(pool: &PgPool, discord: &dyn DiscordBackend) -> Result<(), sqlx::Error> {
    sqlx::query(
        "DELETE FROM twitch_crew_review_events
          WHERE discord_message_id IS NULL
            AND expires_at <= NOW()
            AND (model_claim_until IS NULL OR model_claim_until <= NOW())
            AND (discord_claim_until IS NULL OR discord_claim_until <= NOW())",
    )
    .execute(pool)
    .await?;
    let groups: Vec<(i64, String)> = sqlx::query_as(
        "SELECT discord_channel_id, discord_message_id
           FROM twitch_crew_review_events
          WHERE discord_channel_id IS NOT NULL
            AND discord_message_id IS NOT NULL
          GROUP BY discord_channel_id, discord_message_id
         HAVING BOOL_AND(expires_at <= NOW())
            AND BOOL_AND(model_claim_until IS NULL OR model_claim_until <= NOW())
            AND BOOL_AND(discord_claim_until IS NULL OR discord_claim_until <= NOW())
          ORDER BY MIN(expires_at), discord_channel_id, discord_message_id
          LIMIT $1",
    )
    .bind(RETENTION_LIMIT)
    .fetch_all(pool)
    .await?;
    for (channel_id, message_id) in groups {
        match discord
            .delete_message(DeleteMessage {
                channel_id,
                message_id: message_id.clone(),
                reason: RETENTION_DELETE_REASON.to_owned(),
            })
            .await
        {
            Ok(()) => {
                sqlx::query(
                    "DELETE FROM twitch_crew_review_events expired
                      WHERE expired.discord_channel_id = $1
                        AND expired.discord_message_id = $2
                        AND expired.expires_at <= NOW()
                        AND (expired.model_claim_until IS NULL OR expired.model_claim_until <= NOW())
                        AND (expired.discord_claim_until IS NULL OR expired.discord_claim_until <= NOW())
                        AND NOT EXISTS (
                            SELECT 1 FROM twitch_crew_review_events fresh
                             WHERE fresh.discord_channel_id = expired.discord_channel_id
                               AND fresh.discord_message_id = expired.discord_message_id
                               AND fresh.expires_at > NOW()
                        )",
                )
                .bind(channel_id)
                .bind(&message_id)
                .execute(pool)
                .await?;
            }
            Err(error) => {
                let error_class = match error {
                    DiscordError::BrokerError { status, .. } => format!("discord_status_{status}"),
                    DiscordError::Http(_) => "discord_http".to_owned(),
                    DiscordError::Deserialize(_) => "discord_decode".to_owned(),
                };
                sqlx::query(
                    "UPDATE twitch_crew_review_events event
                        SET content = NULL,
                            metadata = jsonb_build_object('error_class', $3::text, 'tombstoned_at', NOW()),
                            provider = NULL, model = NULL, confidence = NULL,
                            last_delete_error = $3, tombstoned_at = NOW()
                      WHERE event.discord_channel_id = $1
                        AND event.discord_message_id = $2
                        AND (event.model_claim_until IS NULL OR event.model_claim_until <= NOW())
                        AND (event.discord_claim_until IS NULL OR event.discord_claim_until <= NOW())
                        AND NOT EXISTS (
                            SELECT 1 FROM twitch_crew_review_events fresh
                             WHERE fresh.discord_channel_id = event.discord_channel_id
                               AND fresh.discord_message_id = event.discord_message_id
                               AND fresh.expires_at > NOW()
                        )",
                )
                .bind(channel_id)
                .bind(&message_id)
                .bind(error_class)
                .execute(pool)
                .await?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
use crate::test_postgres as postgres;

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        matchers::{body_partial_json, method, path},
        Mock, MockServer, ResponseTemplate,
    };

    async fn database() -> postgres::TestPostgres {
        let db = postgres::TestPostgres::start().await;
        sqlx::raw_sql(
            "CREATE TABLE twitch_crew_review_events (
            id BIGSERIAL PRIMARY KEY, discord_channel_id BIGINT, discord_message_id TEXT,
            expires_at TIMESTAMPTZ NOT NULL, model_claim_until TIMESTAMPTZ,
            discord_claim_until TIMESTAMPTZ, content TEXT DEFAULT 'archivierter Text',
            metadata JSONB DEFAULT '{\"archiviert\":true}', provider TEXT DEFAULT 'alter Anbieter',
            model TEXT DEFAULT 'altes Modell', confidence DOUBLE PRECISION DEFAULT 0.1,
            last_delete_error TEXT, tombstoned_at TIMESTAMPTZ)",
        )
        .execute(&db.pool)
        .await
        .unwrap();
        db
    }

    fn relay(server: &MockServer) -> BrokerRelay {
        BrokerRelay::new(&BrokerConfig {
            base_url: server.uri(),
            token: "test-token".into(),
        })
        .unwrap()
    }

    #[tokio::test]
    async fn archivpflege_beachtet_ablauf_gruppierung_und_bestehende_claims() {
        let db = database().await;
        sqlx::raw_sql("INSERT INTO twitch_crew_review_events (discord_channel_id, discord_message_id, expires_at, model_claim_until, discord_claim_until) VALUES
            (NULL,NULL,NOW()-INTERVAL '1 day',NULL,NULL),
            (NULL,NULL,NOW()+INTERVAL '1 day',NULL,NULL),
            (1,'expired',NOW()-INTERVAL '1 day',NULL,NULL),
            (1,'expired',NOW()-INTERVAL '2 days',NULL,NULL),
            (1,'mixed',NOW()-INTERVAL '1 day',NULL,NULL),
            (1,'mixed',NOW()+INTERVAL '1 day',NULL,NULL),
            (1,'model-claimed',NOW()-INTERVAL '1 day',NOW()+INTERVAL '1 hour',NULL),
            (1,'discord-claimed',NOW()-INTERVAL '1 day',NULL,NOW()+INTERVAL '1 hour'),
            (NULL,NULL,NOW()-INTERVAL '1 day',NOW()+INTERVAL '1 hour',NULL)")
            .execute(&db.pool).await.unwrap();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/internal/master/v1/discord/delete-message"))
            .and(body_partial_json(serde_json::json!({"channel_id":1,"message_id":"expired","reason":RETENTION_DELETE_REASON})))
            .respond_with(ResponseTemplate::new(200)).expect(1).mount(&server).await;
        cleanup_once(&db.pool, &relay(&server)).await.unwrap();
        let ids: Vec<i64> =
            sqlx::query_scalar("SELECT id FROM twitch_crew_review_events ORDER BY id")
                .fetch_all(&db.pool)
                .await
                .unwrap();
        assert_eq!(ids, vec![2, 5, 6, 7, 8, 9]);
        server.verify().await;
    }

    #[tokio::test]
    async fn fehlende_loeschrechte_redigieren_archiv_und_erlauben_spaetere_loeschung() {
        let db = database().await;
        sqlx::query("INSERT INTO twitch_crew_review_events (discord_channel_id, discord_message_id, expires_at) VALUES (1,'expired',NOW()-INTERVAL '1 day')")
            .execute(&db.pool).await.unwrap();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(403))
            .expect(1)
            .mount(&server)
            .await;
        cleanup_once(&db.pool, &relay(&server)).await.unwrap();
        let row: (bool, bool, bool, String) = sqlx::query_as("SELECT content IS NULL AND provider IS NULL AND model IS NULL AND confidence IS NULL, tombstoned_at IS NOT NULL, NOT (metadata ? 'archiviert'), last_delete_error FROM twitch_crew_review_events")
            .fetch_one(&db.pool).await.unwrap();
        assert_eq!(row, (true, true, true, "discord_status_403".into()));
        server.verify().await;
        server.reset().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;
        cleanup_once(&db.pool, &relay(&server)).await.unwrap();
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_crew_review_events")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
        server.verify().await;
    }
}
