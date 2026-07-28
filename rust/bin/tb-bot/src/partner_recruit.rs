//! Partner-Recruiting (B11/Community): erkennt häufige Deadlock-Streamer und
//! reiht sie in die raid-basierte Outreach-Trust-Leiter ein.
//!
//! Kein kalter Chat-Erstkontakt: Kandidaten werden in `twitch_partner_outreach`
//! mit Cooldown vorgemerkt und danach über den Outreach-Boost/Raid-Arrival-Pfad
//! kontaktiert.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use sqlx::PgPool;
use tb_chat::moderation::{OutboundSuppressionCheck, OutboundSuppressionStore};
use tb_chat::ChatApi;
use tb_monitoring::StreamSnapshot;
use tb_raid::RaidBlacklistStore;

/// Maximale Outreach-Sends pro Tag über alle Ticks (Python `RECRUIT_MAX_PER_DAY`).
pub const RECRUIT_MAX_PER_DAY: i64 = 8;
/// Maximale Sends pro Prüfzyklus (Python `RECRUIT_MAX_PER_TICK`).
pub const RECRUIT_MAX_PER_TICK: usize = 3;
/// Pause zwischen Sends innerhalb eines Ticks (Python `RECRUIT_THROTTLE_SECONDS`).
pub const RECRUIT_THROTTLE_SECONDS: u64 = 60;

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
           AND NOT EXISTS ( \
                 SELECT 1 FROM twitch_partners p \
                 WHERE p.status = 'active' \
                   AND LOWER(NULLIF(TRIM(p.twitch_login), '')) = LOWER(NULLIF(TRIM(streamer), ''))) \
           AND LOWER(streamer) NOT IN ( \
                 SELECT streamer_login FROM twitch_partner_outreach WHERE cooldown_until > NOW()) \
           AND LOWER(streamer) NOT IN ( \
                 SELECT LOWER(target_login) FROM twitch_raid_blacklist) \
           AND NOT EXISTS ( \
                 SELECT 1 FROM twitch_chatter_global_ban gb \
                 WHERE LOWER(gb.chatter_login) = LOWER(streamer)) \
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

/// Zählt heute (seit Mitternacht UTC) bereits für die Raid-Leiter vorgemerkte
/// oder historisch kalt kontaktierte Kandidaten. Fehler → Tageslimit erreicht.
pub async fn count_outreach_enqueued_today(pool: &PgPool) -> i64 {
    let count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM twitch_partner_outreach \
         WHERE status IN ('queued', 'sent') \
           AND COALESCE(NULLIF(contacted_at::text, '')::timestamptz, NULLIF(detected_at::text, '')::timestamptz) >= date_trunc('day', NOW())",
    )
    .fetch_one(pool)
    .await;
    match count {
        Ok(count) => count,
        Err(error) => {
            tracing::error!(
                %error,
                "PartnerRecruit: Tageslimit-Zählung fehlgeschlagen; fail-closed"
            );
            RECRUIT_MAX_PER_DAY
        }
    }
}

/// Merkt einen Kandidaten für den raid-basierten Outreach-Boost vor und setzt
/// den Cooldown. Es wird keine kalte Chat-Nachricht gesendet.
pub async fn record_outreach_enqueued(pool: &PgPool, login: &str, user_id: &str) {
    let res = sqlx::query(
        "INSERT INTO twitch_partner_outreach \
           (streamer_login, streamer_user_id, detected_at, contacted_at, status, cooldown_until, raid_used_at) \
         VALUES (LOWER($1), $2, NOW(), NULL, 'queued', NOW() + ($3 || ' days')::interval, NULL) \
         ON CONFLICT (streamer_login) DO UPDATE SET \
           streamer_user_id = EXCLUDED.streamer_user_id, \
           detected_at = EXCLUDED.detected_at, \
           contacted_at = NULL, \
           status = 'queued', \
           cooldown_until = EXCLUDED.cooldown_until, \
           raid_used_at = NULL",
    )
    .bind(login)
    .bind(user_id)
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

/// Merkt einen Kandidaten als Outreach-Boost-Ziel vor. Suppression-Check
/// (source=recruitment) bleibt als Opt-out-Stop erhalten.
async fn enqueue_partner_outreach(pool: &PgPool, login: &str, user_id: &str) {
    match RaidBlacklistStore::new(pool.clone())
        .is_hard_banned(Some(user_id), login)
        .await
    {
        Ok(true) => {
            tracing::warn!(
                login,
                user_id,
                "PartnerRecruit: Kandidat hart ausgeschlossen (globaler Ban)"
            );
            return;
        }
        Ok(false) => {}
        Err(error) => {
            tracing::error!(
                %error,
                login,
                user_id,
                "PartnerRecruit: Global-Ban-Prüfung fehlgeschlagen; fail-closed"
            );
            return;
        }
    }

    if OutboundSuppressionStore::new(pool.clone())
        .check_suppression(login, "recruitment")
        .await
        .is_some()
    {
        tracing::info!(
            login,
            "PartnerRecruit: Outreach übersprungen (Chat-Suppression)"
        );
        return;
    }

    record_outreach_enqueued(pool, login, user_id).await;
    tracing::info!(login, "PartnerRecruit: Kandidat für Raid-Leiter vorgemerkt");
}

/// Haupt-Entry-Point: erkennt Kandidaten, respektiert das Tageslimit und merkt
/// aktuell live Kandidaten für die Raid-Leiter vor (max pro Tick, 60 s Throttle
/// dazwischen). Wird vom Monitoring-after_tick gespawnt; die 30-min-Drosselung
/// liegt beim Aufrufer.
pub async fn run_partner_recruit(
    pool: &PgPool,
    _chat_api: &Arc<dyn ChatApi>,
    category_streams: &[StreamSnapshot],
) {
    let candidates = detect_recruit_candidates(pool).await;
    if candidates.is_empty() {
        return;
    }
    let enqueued_today = count_outreach_enqueued_today(pool).await;
    let remaining_today = RECRUIT_MAX_PER_DAY - enqueued_today;
    if remaining_today <= 0 {
        tracing::info!(enqueued_today, "PartnerRecruit: Tageslimit erreicht");
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
        enqueue_partner_outreach(pool, login, user_id).await;
    }
    if !targets.is_empty() {
        tracing::info!(
            count = targets.len(),
            "PartnerRecruit: Queue-Tick abgeschlossen"
        );
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
            RecruitCandidate {
                streamer: "live_a".into(),
                distinct_days: 5,
            },
            RecruitCandidate {
                streamer: "offline_b".into(),
                distinct_days: 5,
            },
            RecruitCandidate {
                streamer: "live_c".into(),
                distinct_days: 5,
            },
        ];
        let streams = vec![
            snap("Live_A", "111"),
            snap("Live_C", "333"),
            snap("ohne_id", ""),
        ];

        // offline_b nicht live → raus; ohne_id leere user_id (nicht Kandidat) → egal.
        let t = select_outreach_targets(&candidates, &streams, 5, 3);
        assert_eq!(t, vec![("live_a", "111"), ("live_c", "333")]);

        // max_per_tick = 1 → nur der erste Kandidat in Reihenfolge.
        let t1 = select_outreach_targets(&candidates, &streams, 5, 1);
        assert_eq!(t1, vec![("live_a", "111")]);

        // remaining_today 0 → keine neuen Queue-Einträge.
        assert!(select_outreach_targets(&candidates, &streams, 0, 3).is_empty());
    }

    #[test]
    fn select_targets_leere_user_id_kandidat_uebersprungen() {
        // Kandidat ist live, aber sein Stream-Snapshot hat keine user_id.
        let candidates = vec![RecruitCandidate {
            streamer: "noid".into(),
            distinct_days: 5,
        }];
        let streams = vec![snap("noid", "")];
        assert!(select_outreach_targets(&candidates, &streams, 5, 3).is_empty());
    }
}

#[cfg(test)]
mod no_cold_send_tests {
    use super::*;
    use async_trait::async_trait;
    use chrono::{DateTime, Utc};
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tb_chat::api::BanOutcome;
    use tb_chat::types::SendOutcome;
    use tb_raid::OutreachBoostStore;

    struct CountingChatApi {
        sends: AtomicUsize,
    }

    impl CountingChatApi {
        fn new() -> Self {
            Self {
                sends: AtomicUsize::new(0),
            }
        }

        fn send_count(&self) -> usize {
            self.sends.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl ChatApi for CountingChatApi {
        async fn send_message(
            &self,
            _broadcaster_id: &str,
            _message: &str,
        ) -> Result<SendOutcome, String> {
            self.sends.fetch_add(1, Ordering::SeqCst);
            Ok(SendOutcome::Sent)
        }

        async fn send_announcement(
            &self,
            _broadcaster_id: &str,
            _message: &str,
            _color: &str,
        ) -> Result<bool, String> {
            Ok(true)
        }

        async fn ban_user(
            &self,
            _broadcaster_id: &str,
            _target_user_id: &str,
            _reason: &str,
        ) -> Result<BanOutcome, String> {
            Ok(BanOutcome::Banned)
        }

        async fn timeout_user(
            &self,
            _broadcaster_id: &str,
            _target_user_id: &str,
            _duration_secs: u32,
            _reason: &str,
        ) -> Result<BanOutcome, String> {
            Ok(BanOutcome::Banned)
        }

        async fn unban_user(
            &self,
            _broadcaster_id: &str,
            _target_user_id: &str,
        ) -> Result<bool, String> {
            Ok(true)
        }

        async fn delete_message(
            &self,
            _broadcaster_id: &str,
            _message_id: &str,
        ) -> Result<bool, String> {
            Ok(true)
        }

        async fn user_created_at(&self, _user_id: &str) -> Result<Option<DateTime<Utc>>, String> {
            Ok(None)
        }

        async fn resolve_user_id(&self, _login: &str) -> Result<Option<String>, String> {
            Ok(None)
        }

        async fn bot_user_id(&self) -> String {
            "bot".to_string()
        }
    }

    fn snap(login: &str, user_id: &str) -> StreamSnapshot {
        StreamSnapshot {
            user_login: login.into(),
            user_id: user_id.into(),
            ..Default::default()
        }
    }

    async fn setup(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .ok()?;
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .ok()?;
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .ok()?;
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn)
            .ok()?
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect_with(opts)
            .await
            .ok()?;
        for ddl in [
            "CREATE TABLE twitch_stats_category (streamer TEXT, ts_utc TIMESTAMPTZ, viewer_count INTEGER)",
            "CREATE TABLE twitch_streamer_identities (twitch_user_id TEXT, twitch_login TEXT)",
            "CREATE TABLE twitch_partners (twitch_user_id TEXT, twitch_login TEXT, status TEXT)",
            "CREATE TABLE twitch_raid_blacklist (target_login TEXT)",
            "CREATE TABLE twitch_chatter_global_ban (chatter_login TEXT, chatter_id TEXT)",
            "CREATE TABLE twitch_partner_outreach (streamer_login TEXT PRIMARY KEY, streamer_user_id TEXT, \
             detected_at TIMESTAMPTZ, contacted_at TIMESTAMPTZ, status TEXT, cooldown_until TIMESTAMPTZ, raid_used_at TIMESTAMPTZ)",
        ] {
            sqlx::query(ddl).execute(&pool).await.ok()?;
        }
        Some(pool)
    }

    async fn seed_qualifying(pool: &PgPool, streamer: &str) {
        sqlx::query(
            "INSERT INTO twitch_stats_category (streamer, ts_utc, viewer_count) \
             SELECT $1, NOW() - (d || ' days')::interval - (s || ' seconds')::interval, 10 \
             FROM generate_series(0,3) AS d, generate_series(1,480) AS s",
        )
        .bind(streamer)
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn recruit_tick_queued_ohne_kalten_chat_send() {
        let Some(pool) = setup("trust_ladder_no_cold_send").await else {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt oder nicht erreichbar");
            return;
        };
        seed_qualifying(&pool, "kandidat").await;
        let chat_api = Arc::new(CountingChatApi::new());
        let chat_api_trait: Arc<dyn ChatApi> = chat_api.clone();

        run_partner_recruit(&pool, &chat_api_trait, &[snap("kandidat", "555")]).await;

        assert_eq!(chat_api.send_count(), 0, "kein kalter Chat-Send");
        let (status, contacted_at_is_null): (String, bool) = sqlx::query_as(
            "SELECT status, contacted_at IS NULL FROM twitch_partner_outreach WHERE streamer_login='kandidat'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(status, "queued");
        assert!(contacted_at_is_null);
        assert_eq!(count_outreach_enqueued_today(&pool).await, 1);
    }

    #[tokio::test]
    async fn recruit_tick_schliesst_global_gebannte_id_fail_closed_aus() {
        let Some(pool) = setup("trust_ladder_global_ban_id").await else {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt oder nicht erreichbar");
            return;
        };
        seed_qualifying(&pool, "kandidat").await;
        sqlx::query(
            "INSERT INTO twitch_chatter_global_ban (chatter_login, chatter_id) \
             VALUES ('anderer_login', '555')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let chat_api = Arc::new(CountingChatApi::new());
        let chat_api_trait: Arc<dyn ChatApi> = chat_api.clone();

        run_partner_recruit(&pool, &chat_api_trait, &[snap("kandidat", "555")]).await;

        assert_eq!(chat_api.send_count(), 0, "kein kalter Chat-Send");
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_partner_outreach")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0, "global gebannter Kandidat wird nicht vorgemerkt");
    }

    #[tokio::test]
    async fn requeue_setzt_raid_used_at_zurueck_aber_boost_wartet_auf_send() {
        let Some(pool) = setup("trust_ladder_requeue_resets_raid_used").await else {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt oder nicht erreichbar");
            return;
        };
        sqlx::query(
            "INSERT INTO twitch_partner_outreach \
               (streamer_login, streamer_user_id, detected_at, contacted_at, status, cooldown_until, raid_used_at) \
             VALUES ('kandidat', 'alt', NOW() - INTERVAL '3 hours', NOW() - INTERVAL '2 hours', \
                     'sent', NOW(), NOW())",
        )
        .execute(&pool)
        .await
        .unwrap();

        record_outreach_enqueued(&pool, "Kandidat", "555").await;

        let (status, user_id, raid_used_at_is_null): (String, String, bool) = sqlx::query_as(
            "SELECT status, streamer_user_id, raid_used_at IS NULL \
             FROM twitch_partner_outreach WHERE streamer_login='kandidat'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(status, "queued");
        assert_eq!(user_id, "555");
        assert!(raid_used_at_is_null, "Re-Queue setzt Verbrauch zurück");

        let logins = OutreachBoostStore::new(pool)
            .load_boost_logins(48)
            .await
            .unwrap();
        assert!(
            !logins.contains("kandidat"),
            "queued Ziel ist erst nach sent/contacted_at im Boost-Pfad"
        );
    }

    #[tokio::test]
    async fn count_cast_fehler_behandelt_tageslimit_als_erreicht() {
        let Some(pool) = setup("trust_ladder_count_fail_closed").await else {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt oder nicht erreichbar");
            return;
        };
        sqlx::query("DROP TABLE twitch_partner_outreach")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE twitch_partner_outreach (streamer_login TEXT PRIMARY KEY, streamer_user_id TEXT, \
             detected_at TEXT, contacted_at TEXT, status TEXT, cooldown_until TEXT, raid_used_at TEXT)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_partner_outreach (streamer_login, detected_at, contacted_at, status) \
             VALUES ('kaputt', 'not-a-timestamp', NULL, 'queued')",
        )
        .execute(&pool)
        .await
        .unwrap();

        assert_eq!(
            count_outreach_enqueued_today(&pool).await,
            RECRUIT_MAX_PER_DAY
        );
    }

    #[tokio::test]
    async fn active_partner_mit_null_login_leert_kandidatenliste_nicht() {
        let Some(pool) = setup("trust_ladder_null_partner_login").await else {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt oder nicht erreichbar");
            return;
        };
        seed_qualifying(&pool, "kandidat").await;
        sqlx::query(
            "INSERT INTO twitch_partners (twitch_user_id, twitch_login, status) \
             VALUES ('p-null', NULL, 'active')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let candidates = detect_recruit_candidates(&pool).await;
        let logins: Vec<&str> = candidates.iter().map(|c| c.streamer.as_str()).collect();
        assert_eq!(logins, vec!["kandidat"]);
    }
}

#[cfg(all(test, feature = "integration"))]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    async fn setup(schema: &str) -> PgPool {
        let dsn = std::env::var("TB_TEST_DATABASE_URL")
            .expect("TB_TEST_DATABASE_URL fehlt — `rust/scripts/test_db.sh up` und die URL exportieren");
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
            .max_connections(4)
            .connect_with(opts)
            .await
            .unwrap();
        for ddl in [
            "CREATE TABLE twitch_stats_category (streamer TEXT, ts_utc TIMESTAMPTZ, viewer_count INTEGER)",
            "CREATE TABLE twitch_streamer_identities (twitch_user_id TEXT, twitch_login TEXT)",
            "CREATE TABLE twitch_partners (twitch_user_id TEXT, twitch_login TEXT, status TEXT)",
            "CREATE TABLE twitch_raid_blacklist (target_login TEXT)",
            "CREATE TABLE twitch_chatter_global_ban (chatter_login TEXT, chatter_id TEXT)",
            "CREATE TABLE twitch_partner_outreach (streamer_login TEXT PRIMARY KEY, streamer_user_id TEXT, \
             detected_at TIMESTAMPTZ, contacted_at TIMESTAMPTZ, status TEXT, cooldown_until TIMESTAMPTZ, raid_used_at TIMESTAMPTZ)",
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
        // Aktiver Partner ohne Identity-Zeile → ausgeschlossen.
        seed_qualifying(&pool, "schonpartner", 10).await;
        sqlx::query(
            "INSERT INTO twitch_partners (twitch_user_id, twitch_login, status) \
             VALUES ('p1','schonpartner','active')",
        )
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
        // Global gebannt → ausgeschlossen.
        seed_qualifying(&pool, "globalban", 10).await;
        sqlx::query(
            "INSERT INTO twitch_chatter_global_ban (chatter_login, chatter_id) \
             VALUES ('globalban', 'gb-1')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let candidates = detect_recruit_candidates(&pool).await;
        let logins: Vec<&str> = candidates.iter().map(|c| c.streamer.as_str()).collect();
        assert_eq!(
            logins,
            vec!["kandidat"],
            "nur der nicht-ausgeschlossene Kandidat"
        );
        assert_eq!(candidates[0].distinct_days, 4);
    }

    #[tokio::test]
    async fn count_und_record_outreach_queue() {
        let pool = setup("t6e_recruit_record").await;
        assert_eq!(count_outreach_enqueued_today(&pool).await, 0);

        record_outreach_enqueued(&pool, "NeuerStreamer", "555").await;
        assert_eq!(count_outreach_enqueued_today(&pool).await, 1);

        record_outreach_enqueued(&pool, "anderer", "556").await;
        assert_eq!(count_outreach_enqueued_today(&pool).await, 2);

        // Cooldown gesetzt (in der Zukunft), Login lowercased, nicht kalt kontaktiert.
        let (login, status, contacted_at_is_null, cd_future): (String, String, bool, bool) = sqlx::query_as(
            "SELECT streamer_login, status, contacted_at IS NULL, cooldown_until > NOW() FROM twitch_partner_outreach WHERE streamer_login='neuerstreamer'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(login, "neuerstreamer");
        assert_eq!(status, "queued");
        assert!(contacted_at_is_null);
        assert!(cd_future);
    }
}
