//! Geteilte Definition der „letzten beendeten Session" eines Streamers.
//!
//! Sowohl das Lesefenster (`overview::window_since_dates`, Variante `LastStream`)
//! als auch der Paywall-Clamp in `session_detail` müssen exakt dieselbe Session
//! als „die letzte" verstehen — sonst driften Lesefenster und Zugriffsgrenze
//! auseinander. Diese eine Quelle garantiert die Gleichheit by construction:
//! Overview projiziert auf `started_at`, session_detail auf `id`.

use chrono::{DateTime, Utc};
use sqlx::PgPool;

/// Die zuletzt **beendete** Session eines Streamers.
pub struct LastEndedSession {
    pub id: i64,
    pub started_at: DateTime<Utc>,
}

/// Liefert die zuletzt beendete Session für `login` (case-insensitiv).
///
/// - `login` leer (`""`) → globale letzte beendete Session (Wildcard; nur der
///   privilegierte Overview-Pfad nutzt das, normale Partner haben stets einen
///   konkreten Login).
/// - `None`, wenn keine beendete Session existiert.
///
/// Die Abfrage selbst steht in [`tb_analytics::stufe::letzte_beendete_session`]:
/// dort haengt auch das Gratis-Fenster dran, und "letzter Stream" darf in der
/// Paywall nur einmal definiert sein.
pub async fn latest_ended_session(pool: &PgPool, login: &str) -> Option<LastEndedSession> {
    tb_analytics::stufe::letzte_beendete_session(pool, login)
        .await
        .map(|(id, started_at)| LastEndedSession { id, started_at })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    async fn pool_or_skip(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(&format!("SET search_path TO {schema}"))
            .execute(&pool)
            .await
            .unwrap();
        Some(pool)
    }

    async fn create_sessions_table(pool: &PgPool) {
        sqlx::query(
            "CREATE TABLE twitch_stream_sessions (\
                id BIGINT PRIMARY KEY, \
                streamer_login TEXT NOT NULL, \
                started_at TIMESTAMPTZ NOT NULL, \
                ended_at TIMESTAMPTZ, \
                peak_viewers INTEGER, \
                start_viewers INTEGER)",
        )
        .execute(pool)
        .await
        .unwrap();
    }

    /// Neueste BEENDETE Session pro Streamer; case-insensitiv; laufende Session
    /// (ended_at NULL) und fremder Streamer werden ignoriert.
    #[tokio::test]
    async fn nimmt_neueste_beendete_session_pro_streamer() {
        let Some(pool) = pool_or_skip("t_last_session_pick").await else {
            return;
        };
        create_sessions_table(&pool).await;
        sqlx::query(
            "INSERT INTO twitch_stream_sessions (id, streamer_login, started_at, ended_at) VALUES \
             (1, 'EarlySalty', NOW() - INTERVAL '3 days', NOW() - INTERVAL '3 days' + INTERVAL '2 hours'), \
             (2, 'earlysalty', NOW() - INTERVAL '1 day',  NOW() - INTERVAL '1 day'  + INTERVAL '2 hours'), \
             (3, 'earlysalty', NOW() - INTERVAL '1 hour', NULL), \
             (4, 'someoneelse', NOW(),                    NOW())",
        )
        .execute(&pool)
        .await
        .unwrap();

        let got = latest_ended_session(&pool, "earlysalty")
            .await
            .expect("beendete Session vorhanden");
        assert_eq!(
            got.id, 2,
            "neueste beendete Session, laufende #3 + fremder #4 ignoriert"
        );
    }

    /// Keine beendete Session → None (laufende Session zählt nicht).
    #[tokio::test]
    async fn none_wenn_nur_laufende_session() {
        let Some(pool) = pool_or_skip("t_last_session_none").await else {
            return;
        };
        create_sessions_table(&pool).await;
        sqlx::query(
            "INSERT INTO twitch_stream_sessions (id, streamer_login, started_at, ended_at) \
             VALUES (1, 'streamerx', NOW(), NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert!(latest_ended_session(&pool, "streamerx").await.is_none());
    }

    /// Leerer Login = Wildcard → globale letzte beendete Session.
    #[tokio::test]
    async fn wildcard_leerer_login_nimmt_globale_letzte() {
        let Some(pool) = pool_or_skip("t_last_session_wild").await else {
            return;
        };
        create_sessions_table(&pool).await;
        sqlx::query(
            "INSERT INTO twitch_stream_sessions (id, streamer_login, started_at, ended_at) VALUES \
             (1, 'a', NOW() - INTERVAL '2 days', NOW() - INTERVAL '2 days' + INTERVAL '1 hour'), \
             (2, 'b', NOW() - INTERVAL '1 day',  NOW() - INTERVAL '1 day'  + INTERVAL '1 hour')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let got = latest_ended_session(&pool, "")
            .await
            .expect("globale Session");
        assert_eq!(got.id, 2);
    }
}
