//! Queries für `GET /twitch/api/v2/public/recent-bans`.

use sqlx::PgPool;

/// Eine Zeile aus `twitch_ban_events`.
///
/// `received_at` wird via `received_at::text` als `Option<String>` gelesen, damit
/// das Postgres-Textformat (`2024-01-15 10:23:45+00`) erhalten bleibt und keine
/// chrono-Dep nötig ist. Python-`.isoformat()` weicht im `T`-Trennzeichen ab —
/// das ist ein dokumentierter Shadow-Diff-Toleranzpunkt.
#[derive(Debug, sqlx::FromRow)]
pub struct BanRow {
    pub target_login: String,
    pub moderator_login: Option<String>,
    pub reason: Option<String>,
    pub received_at: Option<String>,
}

/// Stats aus `twitch_ban_events` (30-Tage-Fenster).
#[derive(Debug)]
pub struct BanStats {
    pub today: i64,
    pub total_30d: i64,
    pub channels_protected: i64,
}

/// Kombiniertes Ergebnis für den `/recent-bans`-Endpoint.
#[derive(Debug)]
pub struct RecentBansResult {
    pub bans: Vec<BanRow>,
    pub stats: BanStats,
}

/// Lädt die letzten 20 Bans + 30-Tage-Stats.
///
/// Beide Queries laufen sequenziell auf demselben Pool; kein Transaktions-Snapshot
/// nötig (read-only, kleine Drift zwischen den Queries ist akzeptabel).
///
/// `channels_protected` zählt `DISTINCT twitch_user_id` — die Tabelle hat keine
/// `channel_login`-Spalte; `twitch_user_id` entspricht dem Kanal-Owner.
pub async fn recent_bans(pool: &PgPool) -> Result<RecentBansResult, sqlx::Error> {
    let bans: Vec<BanRow> = sqlx::query_as(
        r#"
        SELECT
            target_login,
            moderator_login,
            reason,
            received_at::text AS received_at
        FROM twitch_ban_events
        ORDER BY received_at DESC
        LIMIT 20
        "#,
    )
    .fetch_all(pool)
    .await?;

    let row: (Option<i64>, Option<i64>, Option<i64>) = sqlx::query_as(
        r#"
        SELECT
            COUNT(*) FILTER (WHERE received_at >= CURRENT_DATE)               AS today,
            COUNT(*) FILTER (WHERE received_at >= NOW() - INTERVAL '30 days') AS total_30d,
            COUNT(DISTINCT twitch_user_id)                                     AS channels_protected
        FROM twitch_ban_events
        WHERE received_at >= NOW() - INTERVAL '30 days'
        "#,
    )
    .fetch_one(pool)
    .await?;

    Ok(RecentBansResult {
        bans,
        stats: BanStats {
            today: row.0.unwrap_or(0),
            total_30d: row.1.unwrap_or(0),
            channels_protected: row.2.unwrap_or(0),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    /// DSN aus `TB_TEST_DATABASE_URL` (via `rust/scripts/test_db.sh up`).
    /// Test überspringt sich, wenn Variable nicht gesetzt.
    fn test_dsn() -> Option<String> {
        std::env::var("TB_TEST_DATABASE_URL").ok()
    }

    /// Pool mit max 1 Connection + eigenem Schema — parallele Tests kollidieren nicht,
    /// weil `SET search_path` nur auf der einen Verbindung wirkt und dieser Pool sie
    /// nicht mit anderen Tests teilt.
    async fn make_pool(dsn: &str, schema: &str) -> PgPool {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(dsn)
            .await
            .expect("connect test-db");
        sqlx::query(&format!("CREATE SCHEMA IF NOT EXISTS {schema}"))
            .execute(&pool)
            .await
            .expect("Schema anlegen fehlgeschlagen");
        sqlx::query(&format!("SET search_path TO {schema}"))
            .execute(&pool)
            .await
            .expect("search_path setzen fehlgeschlagen");
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_ban_events (
                id              BIGSERIAL PRIMARY KEY,
                twitch_user_id  TEXT NOT NULL DEFAULT 'default_uid',
                target_login    TEXT NOT NULL,
                moderator_login TEXT,
                reason          TEXT,
                received_at     TIMESTAMPTZ
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL fehlgeschlagen");
        pool
    }

    #[tokio::test]
    async fn recent_bans_leere_tabelle() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_bans_leer").await;
        sqlx::query("TRUNCATE twitch_ban_events")
            .execute(&pool)
            .await
            .unwrap();

        let result = recent_bans(&pool).await.unwrap();
        assert!(result.bans.is_empty(), "erwartet leere Liste");
        assert_eq!(result.stats.today, 0);
        assert_eq!(result.stats.total_30d, 0);
        assert_eq!(result.stats.channels_protected, 0);
    }

    #[tokio::test]
    async fn recent_bans_fixture_und_stats() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_bans_fixture").await;
        sqlx::query("TRUNCATE twitch_ban_events")
            .execute(&pool)
            .await
            .unwrap();

        // 2 aktuelle Bans in 2 verschiedenen Kanälen (twitch_user_id = Kanal-Owner)
        sqlx::query(
            r#"
            INSERT INTO twitch_ban_events
                (twitch_user_id, target_login, moderator_login, reason, received_at)
            VALUES
                ('uid_a', 'spammer1', 'mod_a', 'Spam',    NOW()),
                ('uid_b', 'spammer2', NULL,    NULL,       NOW()),
                ('uid_a', 'alter_ban', 'mod_c', 'Werbung', NOW() - INTERVAL '60 days')
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        let result = recent_bans(&pool).await.unwrap();

        // Nur 2 Bans im 30-Tage-Fenster (alter_ban ist 60 Tage alt)
        assert_eq!(result.stats.total_30d, 2);
        // 2 distinct twitch_user_ids im 30-Tage-Fenster
        assert_eq!(result.stats.channels_protected, 2);
        // today ≥ 2 (beide frisch)
        assert!(result.stats.today >= 2);

        // Alle 3 Einträge kommen zurück (LIMIT 20, ORDER BY DESC)
        assert_eq!(result.bans.len(), 3);
        // NULL-Felder erhalten
        let null_ban = result
            .bans
            .iter()
            .find(|b| b.target_login == "spammer2")
            .unwrap();
        assert!(null_ban.moderator_login.is_none());
        assert!(null_ban.reason.is_none());
        // received_at ist ein nicht-leerer String
        assert!(result.bans[0]
            .received_at
            .as_deref()
            .map(|s| !s.is_empty())
            .unwrap_or(false));
    }
}
