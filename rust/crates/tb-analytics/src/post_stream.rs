//! Post-Stream-Analyse (B11-Post-Stream-Trigger). Port von
//! `bot/analytics/api_post_stream.py` + `post_stream/report_builder.py`.
//!
//! GROSSES Subsystem, Bottom-up portiert. Slice 1 (diese Datei zu Beginn): die
//! fundamentale Datenquelle `load_session_chat_data` — Session-Metadaten +
//! Chat-Nachrichten einer abgeschlossenen Session. Sie speist später die
//! KI-Wortgruppen und den Report-Builder. Weitere Slices: Wortgruppen-AI,
//! Report-Snapshot-Builder, Report-AI, Trigger-Wiring in on_stream_offline.

use sqlx::PgPool;

/// Session-Metadaten einer abgeschlossenen Stream-Session
/// (Python `_load_session_chat_data` → `session`).
#[derive(Debug, Clone)]
pub struct PostStreamSession {
    pub streamer_login: String,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub duration_seconds: i64,
    pub avg_viewers: f64,
    pub peak_viewers: i64,
    pub followers_delta: i64,
}

/// Geladene Chat-/Session-Daten für die Post-Stream-Analyse
/// (Python `_load_session_chat_data`-Rückgabe).
#[derive(Debug, Clone)]
pub struct SessionChatData {
    pub session: PostStreamSession,
    /// Nicht-Command-Nachrichten der Session (≤ 1500, nach Zeit sortiert).
    pub messages: Vec<String>,
    /// Dauer in Minuten, mindestens 1 (Python `duration_min`).
    pub duration_min: i64,
    pub unique_chatters: i64,
}

/// Lädt Session-Metadaten + Chat-Nachrichten einer Session (Python
/// `_load_session_chat_data`). `None`, wenn die Session nicht existiert.
pub async fn load_session_chat_data(pool: &PgPool, session_id: i64) -> Option<SessionChatData> {
    let row = sqlx::query_as::<
        _,
        (
            String,
            Option<String>,
            Option<String>,
            Option<i64>,
            Option<f64>,
            Option<i64>,
            Option<i64>,
        ),
    >(
        "SELECT s.streamer_login, \
                s.started_at::text, \
                s.ended_at::text, \
                s.duration_seconds::int8, \
                COALESCE(s.avg_viewers, 0)::float8, \
                COALESCE(s.peak_viewers, 0)::int8, \
                COALESCE(s.follower_delta, 0)::int8 \
         FROM twitch_stream_sessions s \
         WHERE s.id = $1",
    )
    .bind(session_id)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()?;

    let (streamer_login, started_at, ended_at, duration_seconds, avg_viewers, peak_viewers, followers_delta) =
        row;
    let duration_seconds = duration_seconds.unwrap_or(0);

    let messages: Vec<String> = sqlx::query_scalar::<_, String>(
        "SELECT content FROM twitch_chat_messages \
         WHERE session_id = $1 \
           AND is_command = FALSE \
           AND content IS NOT NULL \
           AND length(content) > 1 \
         ORDER BY message_ts \
         LIMIT 1500",
    )
    .bind(session_id)
    .fetch_all(pool)
    .await
    .unwrap_or_default()
    .into_iter()
    .map(|c| c.trim().to_string())
    .filter(|c| !c.is_empty())
    .collect();

    let unique_chatters: i64 = sqlx::query_scalar(
        "SELECT COUNT(DISTINCT chatter_login) FROM twitch_chat_messages \
         WHERE session_id = $1 AND chatter_login IS NOT NULL",
    )
    .bind(session_id)
    .fetch_one(pool)
    .await
    .unwrap_or(0);

    // Python: max(1, duration_seconds // 60).
    let duration_min = (duration_seconds / 60).max(1);

    Some(SessionChatData {
        session: PostStreamSession {
            streamer_login,
            started_at,
            ended_at,
            duration_seconds,
            avg_viewers: avg_viewers.unwrap_or(0.0),
            peak_viewers: peak_viewers.unwrap_or(0),
            followers_delta: followers_delta.unwrap_or(0),
        },
        messages,
        duration_min,
        unique_chatters,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    async fn pool_or_skip(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let pool = PgPoolOptions::new().max_connections(1).connect(&dsn).await.unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).execute(&pool).await.unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&pool).await.unwrap();
        sqlx::query(&format!("SET search_path TO {schema}")).execute(&pool).await.unwrap();
        sqlx::query(
            "CREATE TABLE twitch_stream_sessions (id BIGINT PRIMARY KEY, streamer_login TEXT, \
             started_at TIMESTAMPTZ, ended_at TIMESTAMPTZ, duration_seconds INTEGER, \
             avg_viewers DOUBLE PRECISION, peak_viewers INTEGER, follower_delta INTEGER)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE twitch_chat_messages (session_id BIGINT, content TEXT, is_command BOOLEAN, \
             message_ts TIMESTAMPTZ, chatter_login TEXT)",
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn laedt_session_metadaten_und_gefilterte_messages() {
        let Some(pool) = pool_or_skip("t6e_post_stream").await else { return };
        sqlx::query(
            "INSERT INTO twitch_stream_sessions \
             (id, streamer_login, started_at, ended_at, duration_seconds, avg_viewers, peak_viewers, follower_delta) \
             VALUES (1,'streamer','2026-06-10T18:00:00+00','2026-06-10T20:00:00+00',7200,12.5,40,7)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_chat_messages (session_id, content, is_command, message_ts, chatter_login) VALUES \
             (1,'hallo zusammen', FALSE, '2026-06-10T18:05:00+00','a'), \
             (1,'!ping', TRUE, '2026-06-10T18:06:00+00','b'), \
             (1,'x', FALSE, '2026-06-10T18:07:00+00','c'), \
             (1,'gutes spiel', FALSE, '2026-06-10T18:08:00+00','a'), \
             (1,NULL, FALSE, '2026-06-10T18:09:00+00','d')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let data = load_session_chat_data(&pool, 1).await.expect("session vorhanden");
        assert_eq!(data.session.streamer_login, "streamer");
        assert_eq!(data.session.duration_seconds, 7200);
        assert_eq!(data.session.avg_viewers, 12.5);
        assert_eq!(data.session.peak_viewers, 40);
        assert_eq!(data.session.followers_delta, 7);
        assert_eq!(data.duration_min, 120); // 7200/60
        // Command (!ping), zu kurz (x) und NULL gefiltert → nur 2 echte Nachrichten.
        assert_eq!(data.messages, vec!["hallo zusammen".to_string(), "gutes spiel".to_string()]);
        // 4 distinkte Chatter (a,b,c,d).
        assert_eq!(data.unique_chatters, 4);

        // Unbekannte Session → None.
        assert!(load_session_chat_data(&pool, 999).await.is_none());
    }
}
