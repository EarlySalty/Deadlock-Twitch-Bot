//! Partner-Recruiting (B11/Community): erkennt häufige Deadlock-Streamer und
//! spricht sie mit einem Partner-Angebot im Chat an. Port von
//! `bot/community/partner_recruit.py`.
//!
//! Slice 1 (diese Datei): die Datenschicht — Kandidaten-Detection, Tageszähler,
//! Outreach-Record + Message-Template. Die Orchestrierung (`run_partner_recruit`
//! + Send via ChatApi) + Verdrahtung in den Monitoring-Tick folgt als Slice 2.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use sqlx::PgPool;
use tb_chat::moderation::{OutboundSuppressionCheck, OutboundSuppressionStore};
use tb_chat::types::SendOutcome;
use tb_chat::ChatApi;
use tb_monitoring::StreamSnapshot;
use tb_raid::ExternalRecruitmentStore;

/// Maximale Outreach-Sends pro Tag über alle Ticks (Python `RECRUIT_MAX_PER_DAY`).
pub const RECRUIT_MAX_PER_DAY: i64 = 8;
/// Maximale Sends pro Prüfzyklus (Python `RECRUIT_MAX_PER_TICK`).
pub const RECRUIT_MAX_PER_TICK: usize = 3;
/// Pause zwischen Sends innerhalb eines Ticks (Python `RECRUIT_THROTTLE_SECONDS`).
pub const RECRUIT_THROTTLE_SECONDS: u64 = 60;
/// Verzögerter Bot-Ban-Check nach erfolgreichem Outreach (wie Recruitment-Message).
const RECRUIT_BAN_CHECK_DELAY_SECONDS: i64 = 3600;

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

/// Wählt die zu kontaktierenden Live-Kandidaten (Python-Auswahllogik in
/// `_run_partner_recruit`): nur aktuell live (im category_streams-Snapshot),
/// mit user_id, gedeckelt auf `min(max_per_tick, remaining_today)`. Pure.
pub fn select_outreach_targets<'a>(
    candidates: &'a [RecruitCandidate],
    category_streams: &'a [StreamSnapshot],
    remaining_today: i64,
    max_per_tick: usize,
) -> Vec<(&'a str, &'a str)> {
    let mut live: HashMap<String, &StreamSnapshot> = HashMap::new();
    for stream in category_streams {
        let login = stream.user_login.to_lowercase();
        if !login.is_empty() {
            live.insert(login, stream);
        }
    }
    let max_sends = max_per_tick.min(remaining_today.max(0) as usize);
    let mut targets: Vec<(&str, &str)> = Vec::new();
    for cand in candidates {
        if targets.len() >= max_sends {
            break;
        }
        let Some(stream) = live.get(&cand.streamer) else {
            continue;
        };
        if stream.user_id.is_empty() {
            continue;
        }
        targets.push((cand.streamer.as_str(), stream.user_id.as_str()));
    }
    targets
}

/// Sendet eine Outreach-Nachricht an einen Kandidaten (Python
/// `_send_partner_outreach`, OHNE IRC-Follow/Join und OHNE die ausgeschlossene
/// Voice-Reaction-Konversation). Suppression-Check (source=recruitment) →
/// ChatApi-Send → record_outreach (Cooldown auch bei Fehlschlag) → bei Erfolg
/// verzögerter Bot-Ban-Check.
async fn send_partner_outreach(
    pool: &PgPool,
    chat_api: &Arc<dyn ChatApi>,
    login: &str,
    user_id: &str,
) {
    if OutboundSuppressionStore::new(pool.clone())
        .check_suppression(login, "recruitment")
        .await
        .is_some()
    {
        tracing::info!(login, "PartnerRecruit: Outreach übersprungen (Chat-Suppression)");
        return;
    }

    let message = build_outreach_message(login);
    let success = matches!(
        chat_api.send_message(user_id, &message).await,
        Ok(SendOutcome::Sent)
    );
    record_outreach(pool, login, user_id, success).await;

    if success {
        if let Err(error) = ExternalRecruitmentStore::new(pool.clone())
            .schedule_bot_ban_check(user_id, login, "recruitment", RECRUIT_BAN_CHECK_DELAY_SECONDS)
            .await
        {
            tracing::debug!(%error, login, "PartnerRecruit: Ban-Check-Schedule fehlgeschlagen");
        }
        tracing::info!(login, "PartnerRecruit: Outreach gesendet");
    } else {
        tracing::warn!(login, "PartnerRecruit: Outreach fehlgeschlagen");
    }
}

/// Haupt-Entry-Point (Python `_run_partner_recruit`): erkennt Kandidaten,
/// respektiert das Tageslimit, sendet an aktuell live Kandidaten (max pro Tick,
/// 60 s Throttle dazwischen). Wird vom Monitoring-after_tick gespawnt; die
/// 30-min-Drosselung liegt beim Aufrufer.
pub async fn run_partner_recruit(
    pool: &PgPool,
    chat_api: &Arc<dyn ChatApi>,
    category_streams: &[StreamSnapshot],
) {
    let candidates = detect_recruit_candidates(pool).await;
    if candidates.is_empty() {
        return;
    }
    let sent_today = count_outreach_sent_today(pool).await;
    let remaining_today = RECRUIT_MAX_PER_DAY - sent_today;
    if remaining_today <= 0 {
        tracing::info!(sent_today, "PartnerRecruit: Tageslimit erreicht");
        return;
    }

    let targets = select_outreach_targets(
        &candidates,
        category_streams,
        remaining_today,
        RECRUIT_MAX_PER_TICK,
    );
    for (i, (login, user_id)) in targets.iter().enumerate() {
        if i > 0 {
            tokio::time::sleep(Duration::from_secs(RECRUIT_THROTTLE_SECONDS)).await;
        }
        send_partner_outreach(pool, chat_api, login, user_id).await;
    }
    if !targets.is_empty() {
        tracing::info!(count = targets.len(), "PartnerRecruit: Outreach-Tick abgeschlossen");
    }
}

#[cfg(test)]
mod pure_tests {
    use super::*;

    fn snap(login: &str, user_id: &str) -> StreamSnapshot {
        StreamSnapshot {
            user_login: login.into(),
            user_id: user_id.into(),
            ..Default::default()
        }
    }

    #[test]
    fn select_targets_live_filter_cap_und_remaining() {
        let candidates = vec![
            RecruitCandidate { streamer: "live_a".into(), distinct_days: 5 },
            RecruitCandidate { streamer: "offline_b".into(), distinct_days: 5 },
            RecruitCandidate { streamer: "live_c".into(), distinct_days: 5 },
        ];
        let streams = vec![snap("Live_A", "111"), snap("Live_C", "333"), snap("ohne_id", "")];

        // offline_b nicht live → raus; ohne_id leere user_id (nicht Kandidat) → egal.
        let t = select_outreach_targets(&candidates, &streams, 5, 3);
        assert_eq!(t, vec![("live_a", "111"), ("live_c", "333")]);

        // max_per_tick = 1 → nur der erste Kandidat in Reihenfolge.
        let t1 = select_outreach_targets(&candidates, &streams, 5, 1);
        assert_eq!(t1, vec![("live_a", "111")]);

        // remaining_today 0 → keine Sends.
        assert!(select_outreach_targets(&candidates, &streams, 0, 3).is_empty());
    }

    #[test]
    fn select_targets_leere_user_id_kandidat_uebersprungen() {
        // Kandidat ist live, aber sein Stream-Snapshot hat keine user_id.
        let candidates = vec![RecruitCandidate { streamer: "noid".into(), distinct_days: 5 }];
        let streams = vec![snap("noid", "")];
        assert!(select_outreach_targets(&candidates, &streams, 5, 3).is_empty());
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
