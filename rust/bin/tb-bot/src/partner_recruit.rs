//! Partner-Recruiting (B11/Community): erkennt häufige Deadlock-Streamer und
//! spricht sie mit einem Partner-Angebot im Chat an. Port von
//! `bot/community/partner_recruit.py`.
//!
//! Slice 1 (diese Datei): die Datenschicht — Kandidaten-Detection, Tageszähler,
//! Outreach-Record + Message-Template. Die Orchestrierung (`run_partner_recruit`
//! + Send via ChatApi) + Verdrahtung in den Monitoring-Tick folgt als Slice 2.

use sqlx::PgPool;

/// Zeitraum für die Erkennung (Python `RECRUIT_LOOKBACK_DAYS`).
pub const RECRUIT_LOOKBACK_DAYS: i64 = 28;
/// Mindestanzahl Streaming-Tage im Zeitraum (Python `RECRUIT_MIN_DAYS`).
pub const RECRUIT_MIN_DAYS: i64 = 4;
/// ≈ 2 h bei 15 s-Sample-Intervall (Python `RECRUIT_MIN_AVG_SAMPLES_PER_DAY`).
pub const RECRUIT_MIN_AVG_SAMPLES_PER_DAY: f64 = 480.0;
/// Pause zwischen Kontaktversuchen in Tagen (Python `RECRUIT_COOLDOWN_DAYS`).
pub const RECRUIT_COOLDOWN_DAYS: i64 = 30;
/// Obergrenze Avg-Viewer (Python `RECRUIT_MAX_AVG_VIEWERS`).
pub const RECRUIT_MAX_AVG_VIEWERS: f64 = 40.0;

/// Erstkontakt-Outreach-Text (Python `_OUTREACH_MSG`). `{login}` eingesetzt.
pub fn build_outreach_message(login: &str) -> String {
    format!(
        "Hey @{login}, bin gerade über deinen Stream gestolpert — \
         wir sind die größte aktive deutsche Deadlock-Community und ziehen die \
         Streamer zusammen, die dranbleiben. Dich hätten wir gern als Partner mit \
         dabei — wie das läuft, steht in der Bio."
    )
}

/// Ein erkannter Recruiting-Kandidat (Python `_detect_recruit_candidates`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecruitCandidate {
    pub streamer: String,
    pub distinct_days: i64,
}

/// Erkennt Kandidaten: Streamer mit ≥ MIN_DAYS Tagen im Lookback, im Schnitt
/// ≥ MIN_AVG_SAMPLES_PER_DAY Samples/Tag, Avg-Viewer ≤ MAX_AVG_VIEWERS, die NICHT
/// bereits bekannt (twitch_streamer_identities), nicht im aktiven Cooldown
/// (twitch_partner_outreach) und nicht raid-blacklisted sind. Query-Fehler →
/// leere Liste (Python try/except → []).
pub async fn detect_recruit_candidates(pool: &PgPool) -> Vec<RecruitCandidate> {
    let rows = sqlx::query_as::<_, (String, i64)>(
        "SELECT streamer, COUNT(DISTINCT DATE(ts_utc)) AS distinct_days \
         FROM twitch_stats_category \
         WHERE ts_utc > NOW() + ($1 || ' days')::interval \
           AND LOWER(streamer) NOT IN ( \
                 SELECT LOWER(twitch_login) FROM twitch_streamer_identities) \
           AND LOWER(streamer) NOT IN ( \
                 SELECT streamer_login FROM twitch_partner_outreach WHERE cooldown_until > NOW()) \
           AND LOWER(streamer) NOT IN ( \
                 SELECT LOWER(target_login) FROM twitch_raid_blacklist) \
         GROUP BY streamer \
         HAVING COUNT(DISTINCT DATE(ts_utc)) >= $2 \
            AND CAST(COUNT(*) AS REAL) / COUNT(DISTINCT DATE(ts_utc)) >= $3 \
            AND AVG(viewer_count) <= $4 \
         ORDER BY distinct_days DESC",
    )
    .bind(format!("-{RECRUIT_LOOKBACK_DAYS}"))
    .bind(RECRUIT_MIN_DAYS)
    .bind(RECRUIT_MIN_AVG_SAMPLES_PER_DAY)
    .bind(RECRUIT_MAX_AVG_VIEWERS)
    .fetch_all(pool)
    .await;
    match rows {
        Ok(rows) => rows
            .into_iter()
            .map(|(streamer, distinct_days)| RecruitCandidate {
                streamer: streamer.to_lowercase(),
                distinct_days,
            })
            .collect(),
        Err(error) => {
            tracing::debug!(%error, "detect_recruit_candidates fehlgeschlagen");
            Vec::new()
        }
    }
}

/// Zählt heute (seit Mitternacht UTC) bereits gesendete Outreach-Nachrichten
/// (Python `_count_outreach_sent_today`). Fehler → 0.
pub async fn count_outreach_sent_today(pool: &PgPool) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM twitch_partner_outreach \
         WHERE status = 'sent' AND contacted_at IS NOT NULL \
           AND contacted_at::timestamptz >= date_trunc('day', NOW())",
    )
    .fetch_one(pool)
    .await
    .unwrap_or(0)
}

/// Loggt einen Outreach-Versuch mit Cooldown (Python `_record_outreach`).
/// Cooldown wird auch bei Fehlschlag gesetzt. Status `sent`/`failed`.
pub async fn record_outreach(pool: &PgPool, login: &str, user_id: &str, success: bool) {
    let status = if success { "sent" } else { "failed" };
    let res = sqlx::query(
        "INSERT INTO twitch_partner_outreach \
           (streamer_login, streamer_user_id, detected_at, contacted_at, status, cooldown_until) \
         VALUES (LOWER($1), $2, NOW(), NOW(), $3, NOW() + ($4 || ' days')::interval) \
         ON CONFLICT (streamer_login) DO UPDATE SET \
           streamer_user_id = EXCLUDED.streamer_user_id, \
           detected_at = EXCLUDED.detected_at, \
           contacted_at = EXCLUDED.contacted_at, \
           status = EXCLUDED.status, \
           cooldown_until = EXCLUDED.cooldown_until",
    )
    .bind(login)
    .bind(user_id)
    .bind(status)
    .bind(format!("{RECRUIT_COOLDOWN_DAYS}"))
    .execute(pool)
    .await;
    if let Err(error) = res {
        tracing::debug!(%error, login, "record_outreach fehlgeschlagen");
    }
}

#[cfg(all(test, feature = "integration"))]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    async fn setup(schema: &str) -> PgPool {
        let dsn = std::env::var("TB_TEST_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://postgres:tbtest@127.0.0.1:5434/postgres".to_string());
        let admin = PgPoolOptions::new().max_connections(1).connect(&dsn).await.unwrap();
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
            .max_connections(4)
            .connect_with(opts)
            .await
            .unwrap();
        for ddl in [
            "CREATE TABLE twitch_stats_category (streamer TEXT, ts_utc TIMESTAMPTZ, viewer_count INTEGER)",
            "CREATE TABLE twitch_streamer_identities (twitch_user_id TEXT, twitch_login TEXT)",
            "CREATE TABLE twitch_raid_blacklist (target_login TEXT)",
            "CREATE TABLE twitch_partner_outreach (streamer_login TEXT PRIMARY KEY, streamer_user_id TEXT, \
             detected_at TIMESTAMPTZ, contacted_at TIMESTAMPTZ, status TEXT, cooldown_until TIMESTAMPTZ)",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        pool
    }

    async fn seed_qualifying(pool: &PgPool, streamer: &str, viewer_count: i32) {
        // 4 distinkte Tage × 480 Samples/Tag, fixe Basiszeit (innerhalb 28 Tage).
        sqlx::query(
            "INSERT INTO twitch_stats_category (streamer, ts_utc, viewer_count) \
             SELECT $1, \
                    '2026-06-10 12:00:00+00'::timestamptz - (d || ' days')::interval - (s || ' seconds')::interval, \
                    $2 \
             FROM generate_series(0,3) AS d, generate_series(1,480) AS s",
        )
        .bind(streamer)
        .bind(viewer_count)
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn detect_qualifizierten_kandidaten_und_ausschluesse() {
        let pool = setup("t6e_recruit_detect").await;
        seed_qualifying(&pool, "kandidat", 10).await;
        // Bereits bekannter Streamer → ausgeschlossen.
        seed_qualifying(&pool, "bekannt", 10).await;
        sqlx::query("INSERT INTO twitch_streamer_identities (twitch_user_id, twitch_login) VALUES ('1','bekannt')")
            .execute(&pool)
            .await
            .unwrap();
        // Zu großer Streamer (avg viewers > 40) → ausgeschlossen.
        seed_qualifying(&pool, "zugross", 100).await;
        // Im aktiven Cooldown → ausgeschlossen.
        seed_qualifying(&pool, "cooldown", 10).await;
        sqlx::query(
            "INSERT INTO twitch_partner_outreach (streamer_login, status, cooldown_until) \
             VALUES ('cooldown','sent', NOW() + INTERVAL '10 days')",
        )
        .execute(&pool)
        .await
        .unwrap();
        // Raid-blacklisted → ausgeschlossen.
        seed_qualifying(&pool, "geblockt", 10).await;
        sqlx::query("INSERT INTO twitch_raid_blacklist (target_login) VALUES ('geblockt')")
            .execute(&pool)
            .await
            .unwrap();

        let candidates = detect_recruit_candidates(&pool).await;
        let logins: Vec<&str> = candidates.iter().map(|c| c.streamer.as_str()).collect();
        assert_eq!(logins, vec!["kandidat"], "nur der nicht-ausgeschlossene Kandidat");
        assert_eq!(candidates[0].distinct_days, 4);
    }

    #[tokio::test]
    async fn count_und_record_outreach() {
        let pool = setup("t6e_recruit_record").await;
        assert_eq!(count_outreach_sent_today(&pool).await, 0);

        record_outreach(&pool, "NeuerStreamer", "555", true).await;
        assert_eq!(count_outreach_sent_today(&pool).await, 1);

        // failed zählt NICHT als sent-today.
        record_outreach(&pool, "anderer", "556", false).await;
        assert_eq!(count_outreach_sent_today(&pool).await, 1);

        // Cooldown gesetzt (in der Zukunft) + Login lowercased + UPSERT.
        let (login, cd_future): (String, bool) = sqlx::query_as(
            "SELECT streamer_login, cooldown_until > NOW() FROM twitch_partner_outreach WHERE streamer_login='neuerstreamer'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(login, "neuerstreamer");
        assert!(cd_future);
    }
}
