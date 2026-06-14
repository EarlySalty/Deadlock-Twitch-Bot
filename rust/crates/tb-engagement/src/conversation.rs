//! Rolling Multi-Turn-Konversations-Buffer pro Channel (Port von
//! `bot/engagement/conversation.py`).
//!
//! Hält die letzten ~100 Turns in `twitch_engagement_conversation`. User- und
//! Bot-Turns werden abwechselnd persistiert, wie es OpenAI-/MiniMax-kompatible
//! Chat-Completion-APIs erwarten. `load_recent_buffer` liefert chronologisch
//! (älteste zuerst) — die DB-Reihenfolge `ts DESC` wird dafür umgedreht.

use chrono::{DateTime, Utc};
use sqlx::PgPool;

/// Ein einzelner Gesprächs-Turn aus dem Buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationTurn {
    pub role: String,
    pub twitch_user_id: Option<String>,
    pub twitch_login: Option<String>,
    pub content: String,
    pub message_id: Option<String>,
    pub ts: DateTime<Utc>,
}

type Row = (
    String,
    Option<String>,
    Option<String>,
    String,
    Option<String>,
    DateTime<Utc>,
);

/// Persistenter Multi-Turn-Buffer pro Channel.
pub struct ConversationBuffer {
    pool: PgPool,
}

impl ConversationBuffer {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Hängt einen User-Turn an.
    pub async fn append_user_turn(
        &self,
        channel_login: &str,
        twitch_user_id: &str,
        twitch_login: &str,
        content: &str,
        message_id: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO twitch_engagement_conversation \
             (channel_login, role, twitch_user_id, twitch_login, content, message_id) \
             VALUES ($1, 'user', $2, $3, $4, $5)",
        )
        .bind(channel_login)
        .bind(twitch_user_id)
        .bind(twitch_login)
        .bind(content)
        .bind(message_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Hängt einen Assistant-(Bot-)Turn an.
    pub async fn append_assistant_turn(
        &self,
        channel_login: &str,
        content: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO twitch_engagement_conversation (channel_login, role, content) \
             VALUES ($1, 'assistant', $2)",
        )
        .bind(channel_login)
        .bind(content)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Lädt die jüngsten `limit` Turns eines Channels in chronologischer
    /// Reihenfolge (älteste zuerst).
    pub async fn load_recent_buffer(
        &self,
        channel_login: &str,
        limit: i64,
    ) -> Result<Vec<ConversationTurn>, sqlx::Error> {
        let rows = sqlx::query_as::<_, Row>(
            "SELECT role, twitch_user_id, twitch_login, content, message_id, ts \
             FROM twitch_engagement_conversation \
             WHERE channel_login = $1 \
             ORDER BY ts DESC \
             LIMIT $2",
        )
        .bind(channel_login)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .rev()
            .map(|(role, twitch_user_id, twitch_login, content, message_id, ts)| ConversationTurn {
                role,
                twitch_user_id,
                twitch_login,
                content,
                message_id,
                ts,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new().max_connections(1).connect(&dsn).await.unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).execute(&admin).await.unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&admin).await.unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn).unwrap().options([("search_path", schema)]);
        let pool = PgPoolOptions::new().max_connections(2).connect_with(opts).await.unwrap();
        sqlx::query(
            "CREATE TABLE twitch_engagement_conversation (\
             id BIGSERIAL PRIMARY KEY, channel_login TEXT NOT NULL, role TEXT NOT NULL, \
             twitch_user_id TEXT, twitch_login TEXT, content TEXT NOT NULL, \
             message_id TEXT, ts TIMESTAMPTZ NOT NULL DEFAULT NOW())",
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn append_rollen_und_nullables() {
        let Some(pool) = make_pool("t_eng_conv_append").await else { return };
        let buf = ConversationBuffer::new(pool.clone());
        buf.append_user_turn("nani", "u1", "chatter1", "frage", Some("m1")).await.unwrap();
        buf.append_assistant_turn("nani", "antwort").await.unwrap();

        let turns = buf.load_recent_buffer("nani", 100).await.unwrap();
        assert_eq!(turns.len(), 2);
        // Assistant-Turn hat keine User-Felder.
        let assistant = turns.iter().find(|t| t.role == "assistant").unwrap();
        assert_eq!(assistant.content, "antwort");
        assert!(assistant.twitch_user_id.is_none());
        assert!(assistant.twitch_login.is_none());
        // User-Turn behält seine Felder.
        let user = turns.iter().find(|t| t.role == "user").unwrap();
        assert_eq!(user.twitch_login.as_deref(), Some("chatter1"));
        assert_eq!(user.message_id.as_deref(), Some("m1"));
    }

    #[tokio::test]
    async fn load_chronologisch_und_limit() {
        let Some(pool) = make_pool("t_eng_conv_order").await else { return };
        let buf = ConversationBuffer::new(pool.clone());
        // Kontrollierte ts für deterministische Reihenfolge.
        sqlx::query(
            "INSERT INTO twitch_engagement_conversation (channel_login, role, content, ts) VALUES \
             ('nani','user','alt', NOW() - INTERVAL '20 seconds'), \
             ('nani','assistant','mittel', NOW() - INTERVAL '10 seconds'), \
             ('nani','user','neu', NOW())",
        )
        .execute(&pool)
        .await
        .unwrap();

        // Voll: chronologisch (älteste zuerst).
        let all = buf.load_recent_buffer("nani", 100).await.unwrap();
        assert_eq!(
            all.iter().map(|t| t.content.as_str()).collect::<Vec<_>>(),
            vec!["alt", "mittel", "neu"]
        );
        // Limit 2 → die zwei JÜNGSTEN (mittel, neu), chronologisch sortiert.
        let limited = buf.load_recent_buffer("nani", 2).await.unwrap();
        assert_eq!(
            limited.iter().map(|t| t.content.as_str()).collect::<Vec<_>>(),
            vec!["mittel", "neu"]
        );
    }
}
