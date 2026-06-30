//! Partner-Raid-Score Refresh-Pipeline (compute + upsert + periodic refresh).
//!
//! Port von `bot/raid/partner_scores.py` `_build_score` (Z. 656–797),
//! `_upsert_scores` (Z. 799–853) und `_refresh_scores_for_ids` (Z. 336–388).
//! Diese Slice schließt die P1.9-Lücke: In Rust las
//! `auto_raid_pipeline.rs` zwar `twitch_partner_raid_scores`, aber NICHTS
//! schrieb/aktualisierte den Cache je — nach dem Cutover fror er ein.
//!
//! ## Aufbau (tiefe Module, schmale Schnittstelle)
//!
//! - [`build_score_upsert`] — REINE Aggregation: aus Roh-Sessions, Raid-Timestamps,
//!   Internal-Metriken, Live-State und Cache wird ein [`PartnerRaidScoreUpsert`].
//!   Voll unit-testbar ohne DB (Berlin-TZ-Zeitraster, Today-Count, avg-Dauer,
//!   live/cache/offline-Verzweigung wie Python `_build_score`).
//! - [`PartnerScoreRefresher`] — DB-Orchestrator: lädt Partner + Begleitdaten,
//!   ruft [`build_score_upsert`] und schreibt via [`ScoreStore::upsert`].
//!   [`PartnerScoreRefresher::refresh_all`] ist der periodische Einstieg
//!   (Python `refresh_all_partner_raid_scores`, alle 300 s aus
//!   `bot/monitoring/partner_ops.py`).
//!
//! WIRING-TODO(P1.9): `bin/tb-bot/src/main.rs` muss neben den anderen 300s-Tasks
//! `PartnerScoreRefresher::new(pool).refresh_all(Utc::now()).await` periodisch
//! spawnen (Mirror von partner_ops.py:19/23). Diese Crate exponiert dafür den
//! Refresher; das eigentliche `tokio::spawn` + Intervall gehört in den
//! Composition-Root (bin/tb-bot), nicht in die Lib.

use chrono::{DateTime, Datelike, Timelike, Utc};
use chrono_tz::Europe::Berlin;
use sqlx::PgPool;

use crate::score_store::{PartnerRaidScoreUpsert, ScoreStore};
use crate::scoring::{
    compute_base_score, compute_fairness_score, compute_final_score,
    compute_new_partner_multiplier, compute_raid_boost_multiplier, compute_readiness_score,
    round_score, NEUTRAL_SCORE, NEW_PARTNER_RAID_THRESHOLD,
};

/// Lookback-Fenster für Sessions (Python `LOOKBACK_DAYS = 45`).
pub const LOOKBACK_DAYS: i64 = 45;
/// Ab so vielen Sessions gilt die History als zuverlässig
/// (Python `MIN_RELIABLE_SESSIONS = 3`).
pub const MIN_RELIABLE_SESSIONS: usize = 3;

// ---------------------------------------------------------------------------
// Roh-Eingaben für die reine Aggregation
// ---------------------------------------------------------------------------

/// Eine Stream-Session-Zeile (aus `twitch_stream_sessions`).
#[derive(Debug, Clone)]
pub struct SessionRow {
    /// Start der Session (UTC). `None` = unparsebar → wird übersprungen.
    pub started_at: Option<DateTime<Utc>>,
    /// Dauer in Sekunden (0 wenn unbekannt; nur > 0 zählt für avg).
    pub duration_seconds: i64,
}

/// Live-Zustand eines Partners (aus `twitch_live_state`).
#[derive(Debug, Clone, Default)]
pub struct LiveState {
    pub is_live: bool,
    /// Start des aktuellen Streams (UTC), wenn live.
    pub last_started_at: Option<DateTime<Utc>>,
}

/// Bestehende Cache-Zeile (für den offline-mit-Cache-Pfad, Python Z. 757–772).
#[derive(Debug, Clone)]
pub struct ExistingCache {
    pub current_started_at: Option<String>,
    pub current_uptime_sec: i32,
    pub duration_score: f64,
    pub time_pattern_score: f64,
    pub readiness_score: f64,
    pub fairness_score: f64,
    pub base_score: f64,
    pub final_score: f64,
}

/// Gebündelte Roh-Eingaben für [`build_score_upsert`].
#[derive(Debug, Clone)]
pub struct ScoreBuildInput {
    pub twitch_user_id: String,
    pub twitch_login: String,
    pub sessions: Vec<SessionRow>,
    /// Erfolgreiche Raid-Timestamps ZU diesem Partner (UTC).
    pub raid_timestamps: Vec<DateTime<Utc>>,
    /// (sent_30d, received_30d, received_7d) — interne Netzwerk-Metriken.
    pub internal_metrics: (i64, i64, i64),
    pub raid_boost_enabled: bool,
    pub live_state: LiveState,
    pub existing_cache: Option<ExistingCache>,
}

/// `value.astimezone(UTC).isoformat(timespec="seconds")` — Python `_iso_utc`.
fn iso_utc_seconds(value: DateTime<Utc>) -> String {
    value.format("%Y-%m-%dT%H:%M:%S+00:00").to_string()
}

// ---------------------------------------------------------------------------
// Reine Aggregation (Port von _build_score)
// ---------------------------------------------------------------------------

/// Baut aus den Roh-Eingaben eine vollständige Upsert-Zeile.
///
/// Faithful-Port von `PartnerRaidScoreService._build_score` (Z. 656–797):
/// avg-Dauer + Zuverlässigkeit, Berlin-Zeitraster (weekday+hour-Match),
/// today_received_raids (Berlin-Datum), live/cache/offline-Verzweigung,
/// Multiplikatoren und final_score.
pub fn build_score_upsert(
    input: &ScoreBuildInput,
    now_utc: DateTime<Utc>,
) -> PartnerRaidScoreUpsert {
    let lookback_cutoff = now_utc - chrono::Duration::days(LOOKBACK_DAYS);

    // recent_started / recent_durations (Python Z. 682–692).
    let mut recent_started: Vec<DateTime<Utc>> = Vec::new();
    let mut recent_durations: Vec<i64> = Vec::new();
    for row in &input.sessions {
        let Some(started_at) = row.started_at else {
            continue;
        };
        if started_at < lookback_cutoff {
            continue;
        }
        recent_started.push(started_at);
        if row.duration_seconds > 0 {
            recent_durations.push(row.duration_seconds);
        }
    }

    let avg_duration_sec: i64 = if recent_durations.is_empty() {
        0
    } else {
        let sum: i64 = recent_durations.iter().sum();
        // int(round(sum/len)) — Python round() ist banker's rounding, aber für
        // positive Mittelwerte ist .round() (half-away) praktisch deckungsgleich.
        (sum as f64 / recent_durations.len() as f64).round() as i64
    };
    let duration_history_reliable =
        recent_durations.len() >= MIN_RELIABLE_SESSIONS && avg_duration_sec > 0;

    // Berlin-Zeitraster: weekday+hour-Match (Python Z. 700–712).
    let now_berlin = now_utc.with_timezone(&Berlin);
    let (time_pattern_score_base, time_pattern_reliable) =
        if recent_started.len() >= MIN_RELIABLE_SESSIONS {
            let matching = recent_started
                .iter()
                .filter(|started| {
                    let b = started.with_timezone(&Berlin);
                    b.weekday() == now_berlin.weekday() && b.hour() == now_berlin.hour()
                })
                .count();
            (
                round_score(matching as f64 / recent_started.len() as f64),
                true,
            )
        } else {
            (NEUTRAL_SCORE, false)
        };

    // today_received_raids (Berlin-Datum, Python Z. 714–719).
    let today_berlin = now_berlin.date_naive();
    let raid_total = input.raid_timestamps.len() as i64;
    let today_received_raids = input
        .raid_timestamps
        .iter()
        .filter(|ts| ts.with_timezone(&Berlin).date_naive() == today_berlin)
        .count() as i64;

    let (sent_30d, received_30d, received_7d) = input.internal_metrics;
    let is_new_partner_preferred = raid_total < NEW_PARTNER_RAID_THRESHOLD;
    let new_partner_multiplier = compute_new_partner_multiplier(raid_total);
    let raid_boost_multiplier = compute_raid_boost_multiplier(input.raid_boost_enabled);

    let is_live = input.live_state.is_live;
    let started_at_live = input.live_state.last_started_at;

    // Verzweigung live / cache / offline (Python Z. 730–772).
    let current_started_at: Option<String>;
    let current_uptime_sec: i32;
    let duration_score: f64;
    let time_pattern_score: f64;
    let readiness_score: f64;
    let fairness_score: f64;
    let base_score: f64;
    let final_score: f64;

    if let (true, Some(started)) = (is_live, started_at_live) {
        current_started_at = Some(iso_utc_seconds(started));
        current_uptime_sec = (now_utc - started).num_seconds().max(0) as i32;
        duration_score = if duration_history_reliable && avg_duration_sec > 0 {
            round_score(
                ((avg_duration_sec - current_uptime_sec as i64) as f64 / avg_duration_sec as f64)
                    .clamp(0.0, 1.0),
            )
        } else {
            NEUTRAL_SCORE
        };
        time_pattern_score = if time_pattern_reliable {
            time_pattern_score_base
        } else {
            NEUTRAL_SCORE
        };
        readiness_score = compute_readiness_score(duration_score, time_pattern_score);
        fairness_score =
            compute_fairness_score(sent_30d, received_30d, received_7d, today_received_raids);
        base_score = compute_base_score(readiness_score, fairness_score);
        final_score =
            compute_final_score(base_score, new_partner_multiplier, raid_boost_multiplier);
    } else if let Some(cache) = &input.existing_cache {
        current_started_at = cache
            .current_started_at
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        current_uptime_sec = cache.current_uptime_sec;
        duration_score = round_score(cache.duration_score);
        time_pattern_score = round_score(cache.time_pattern_score);
        readiness_score = round_score(cache.readiness_score);
        fairness_score = round_score(cache.fairness_score);
        base_score = round_score(cache.base_score);
        final_score = round_score(cache.final_score);
    } else {
        current_started_at = None;
        current_uptime_sec = 0;
        duration_score = NEUTRAL_SCORE;
        time_pattern_score = if time_pattern_reliable {
            time_pattern_score_base
        } else {
            NEUTRAL_SCORE
        };
        readiness_score = compute_readiness_score(duration_score, time_pattern_score);
        fairness_score =
            compute_fairness_score(sent_30d, received_30d, received_7d, today_received_raids);
        base_score = compute_base_score(readiness_score, fairness_score);
        final_score =
            compute_final_score(base_score, new_partner_multiplier, raid_boost_multiplier);
    }

    PartnerRaidScoreUpsert {
        twitch_user_id: input.twitch_user_id.clone(),
        twitch_login: input.twitch_login.clone(),
        avg_duration_sec: avg_duration_sec as i32,
        time_pattern_score_base: round_score(time_pattern_score_base),
        received_successful_raids_total: raid_total as i32,
        is_new_partner_preferred: i32::from(is_new_partner_preferred),
        new_partner_multiplier,
        raid_boost_multiplier: round_score(raid_boost_multiplier),
        is_live: i32::from(is_live),
        current_started_at,
        current_uptime_sec,
        duration_score: round_score(duration_score),
        time_pattern_score: round_score(time_pattern_score),
        readiness_score: round_score(readiness_score),
        fairness_score: round_score(fairness_score),
        base_score: round_score(base_score),
        final_score: round_score(final_score),
        internal_sent_raids_30d: sent_30d.max(0) as i32,
        internal_received_raids_30d: received_30d.max(0) as i32,
        internal_received_raids_7d: received_7d.max(0) as i32,
        today_received_raids: today_received_raids as i32,
        last_computed_at: iso_utc_seconds(now_utc),
    }
}

// ---------------------------------------------------------------------------
// DB-Orchestrator
// ---------------------------------------------------------------------------

/// Ein aktiver Partner (aus `twitch_streamers_partner_state`).
#[derive(Debug, Clone, sqlx::FromRow)]
struct PartnerRow {
    twitch_user_id: String,
    twitch_login: String,
}

/// Orchestriert den Score-Refresh: lädt Partner + Begleitdaten, aggregiert,
/// upsertet. Port von `PartnerRaidScoreService._refresh_scores_for_ids`.
#[derive(Clone)]
pub struct PartnerScoreRefresher {
    pool: PgPool,
    store: ScoreStore,
}

impl PartnerScoreRefresher {
    pub fn new(pool: PgPool) -> Self {
        let store = ScoreStore::new(pool.clone());
        Self { pool, store }
    }

    /// Refresht alle AKTIVEN Partner (Python `refresh_all_partner_scores`).
    /// Gibt die Anzahl geschriebener Zeilen zurück.
    pub async fn refresh_all(&self, now_utc: DateTime<Utc>) -> Result<usize, sqlx::Error> {
        let partners = self.load_active_partners().await?;
        self.refresh_partners(&partners, now_utc).await
    }

    /// Refresht eine konkrete Liste von Partner-User-IDs (aktiv ODER inaktiv),
    /// Port von `refresh_partner_score`/`_refresh_scores_for_ids(active_only=false)`.
    pub async fn refresh_for_ids(
        &self,
        twitch_user_ids: &[String],
        now_utc: DateTime<Utc>,
    ) -> Result<usize, sqlx::Error> {
        if twitch_user_ids.is_empty() {
            return Ok(0);
        }
        let partners = self.load_partners_by_ids(twitch_user_ids).await?;
        self.refresh_partners(&partners, now_utc).await
    }

    async fn refresh_partners(
        &self,
        partners: &[PartnerRow],
        now_utc: DateTime<Utc>,
    ) -> Result<usize, sqlx::Error> {
        let mut written = 0usize;
        for partner in partners {
            let input = self.build_input_for(partner, now_utc).await?;
            let upsert = build_score_upsert(&input, now_utc);
            self.store.upsert(&upsert).await?;
            written += 1;
        }
        Ok(written)
    }

    async fn build_input_for(
        &self,
        partner: &PartnerRow,
        now_utc: DateTime<Utc>,
    ) -> Result<ScoreBuildInput, sqlx::Error> {
        let sessions = self.load_sessions(&partner.twitch_login).await?;
        let raid_timestamps = self.load_raid_timestamps(&partner.twitch_user_id).await?;
        let internal_metrics = self
            .load_internal_metrics(&partner.twitch_user_id, now_utc)
            .await?;
        let raid_boost_enabled = self.load_boost_flag(&partner.twitch_user_id).await?;
        let live_state = self.load_live_state(&partner.twitch_user_id).await?;
        let existing_cache = self.load_existing_cache(&partner.twitch_user_id).await?;
        Ok(ScoreBuildInput {
            twitch_user_id: partner.twitch_user_id.clone(),
            twitch_login: partner.twitch_login.clone(),
            sessions,
            raid_timestamps,
            internal_metrics,
            raid_boost_enabled,
            live_state,
            existing_cache,
        })
    }

    async fn load_active_partners(&self) -> Result<Vec<PartnerRow>, sqlx::Error> {
        sqlx::query_as!(
            PartnerRow,
            r#"
            SELECT twitch_user_id AS "twitch_user_id!",
                   LOWER(twitch_login) AS "twitch_login!"
            FROM twitch_streamers_partner_state
            WHERE twitch_user_id IS NOT NULL
              AND twitch_login IS NOT NULL
              AND COALESCE(is_partner_active, 0) = 1
            ORDER BY LOWER(twitch_login)
            "#,
        )
        .fetch_all(&self.pool)
        .await
    }

    async fn load_partners_by_ids(&self, ids: &[String]) -> Result<Vec<PartnerRow>, sqlx::Error> {
        sqlx::query_as!(
            PartnerRow,
            r#"
            SELECT twitch_user_id AS "twitch_user_id!",
                   LOWER(twitch_login) AS "twitch_login!"
            FROM twitch_streamers_partner_state
            WHERE twitch_user_id IS NOT NULL
              AND twitch_login IS NOT NULL
              AND twitch_user_id = ANY($1)
            ORDER BY LOWER(twitch_login)
            "#,
            ids
        )
        .fetch_all(&self.pool)
        .await
    }

    async fn load_sessions(&self, login: &str) -> Result<Vec<SessionRow>, sqlx::Error> {
        let rows = sqlx::query!(
            r#"
            SELECT NULLIF(started_at::text, '')::timestamptz AS "started_at?",
                   duration_seconds AS "duration_seconds?"
            FROM twitch_stream_sessions
            WHERE LOWER(streamer_login) = LOWER($1)
            "#,
            login
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| SessionRow {
                started_at: row.started_at,
                duration_seconds: i64::from(row.duration_seconds.unwrap_or(0)),
            })
            .collect())
    }

    async fn load_raid_timestamps(&self, user_id: &str) -> Result<Vec<DateTime<Utc>>, sqlx::Error> {
        let rows = sqlx::query!(
            r#"
            SELECT executed_at AS "executed_at?"
            FROM twitch_raid_history
            WHERE to_broadcaster_id = $1
              AND COALESCE(success, FALSE) IS TRUE
            "#,
            user_id
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().filter_map(|row| row.executed_at).collect())
    }

    async fn load_internal_metrics(
        &self,
        user_id: &str,
        now_utc: DateTime<Utc>,
    ) -> Result<(i64, i64, i64), sqlx::Error> {
        let cutoff_30d = now_utc - chrono::Duration::days(30);
        let cutoff_7d = now_utc - chrono::Duration::days(7);

        // sent_30d: dieser Partner als Quelle, anderer Partner als Ziel.
        let sent_30d: i64 = sqlx::query_scalar!(
            r#"
            SELECT COUNT(*)::bigint AS "count!"
            FROM twitch_raid_history h
            WHERE COALESCE(h.success, FALSE) IS TRUE
              AND h.from_broadcaster_id = $1
              AND h.executed_at >= $2
              AND EXISTS (
                  SELECT 1 FROM twitch_streamers_partner_state p
                  WHERE p.twitch_user_id = h.to_broadcaster_id
              )
            "#,
            user_id,
            cutoff_30d
        )
        .fetch_one(&self.pool)
        .await?;

        let received_30d: i64 = sqlx::query_scalar!(
            r#"
            SELECT COUNT(*)::bigint AS "count!"
            FROM twitch_raid_history h
            WHERE COALESCE(h.success, FALSE) IS TRUE
              AND h.to_broadcaster_id = $1
              AND h.executed_at >= $2
              AND EXISTS (
                  SELECT 1 FROM twitch_streamers_partner_state p
                  WHERE p.twitch_user_id = h.from_broadcaster_id
              )
            "#,
            user_id,
            cutoff_30d
        )
        .fetch_one(&self.pool)
        .await?;

        let received_7d: i64 = sqlx::query_scalar!(
            r#"
            SELECT COUNT(*)::bigint AS "count!"
            FROM twitch_raid_history h
            WHERE COALESCE(h.success, FALSE) IS TRUE
              AND h.to_broadcaster_id = $1
              AND h.executed_at >= $2
              AND EXISTS (
                  SELECT 1 FROM twitch_streamers_partner_state p
                  WHERE p.twitch_user_id = h.from_broadcaster_id
              )
            "#,
            user_id,
            cutoff_7d
        )
        .fetch_one(&self.pool)
        .await?;

        Ok((sent_30d, received_30d, received_7d))
    }

    /// Lädt den Raid-Boost-Flag (vereinfachter Pfad: `raid_boost_enabled`-Spalte
    /// in `streamer_plans`). Die vollständige Entitlement-Auflösung
    /// (plan_name/manual_plan_id) aus Python `_load_boost_flags` ist eine eigene
    /// Slice (Entitlement-Katalog noch nicht in tb-raid portiert).
    async fn load_boost_flag(&self, user_id: &str) -> Result<bool, sqlx::Error> {
        let row: Option<i32> = sqlx::query_scalar!(
            r#"
            SELECT COALESCE(raid_boost_enabled, 0) AS "raid_boost_enabled!"
            FROM streamer_plans
            WHERE twitch_user_id = $1
            "#,
            user_id
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|v| v != 0).unwrap_or(false))
    }

    async fn load_live_state(&self, user_id: &str) -> Result<LiveState, sqlx::Error> {
        let row = sqlx::query!(
            r#"
            SELECT COALESCE(is_live, 0) AS "is_live!",
                   NULLIF(last_started_at::text, '')::timestamptz AS "last_started_at?"
            FROM twitch_live_state
            WHERE twitch_user_id = $1
            "#,
            user_id
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(match row {
            Some(row) => LiveState {
                is_live: row.is_live != 0,
                last_started_at: row.last_started_at,
            },
            None => LiveState::default(),
        })
    }

    async fn load_existing_cache(
        &self,
        user_id: &str,
    ) -> Result<Option<ExistingCache>, sqlx::Error> {
        let row = self.store.load(user_id).await?;
        Ok(row.map(|r| ExistingCache {
            current_started_at: r.current_started_at,
            current_uptime_sec: r.current_uptime_sec,
            duration_score: r.duration_score,
            time_pattern_score: r.time_pattern_score,
            readiness_score: r.readiness_score,
            fairness_score: r.fairness_score,
            base_score: r.base_score,
            final_score: r.final_score,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn base_input(user_id: &str) -> ScoreBuildInput {
        ScoreBuildInput {
            twitch_user_id: user_id.to_string(),
            twitch_login: "partner".to_string(),
            sessions: vec![],
            raid_timestamps: vec![],
            internal_metrics: (0, 0, 0),
            raid_boost_enabled: false,
            live_state: LiveState::default(),
            existing_cache: None,
        }
    }

    #[test]
    fn build_offline_neuer_partner_neutral_scores() {
        // Keine Sessions, keine Raids, nicht live, kein Cache → offline-Pfad mit
        // NEUTRAL duration, fairness aus (0,0,0,0)=0.75, base/final daraus.
        let now = Utc.with_ymd_and_hms(2026, 6, 21, 12, 0, 0).unwrap();
        let upsert = build_score_upsert(&base_input("uid_off"), now);
        assert_eq!(upsert.is_live, 0);
        assert_eq!(upsert.received_successful_raids_total, 0);
        assert_eq!(upsert.is_new_partner_preferred, 1);
        assert!((upsert.duration_score - 0.5).abs() < 1e-9);
        assert!((upsert.fairness_score - 0.75).abs() < 1e-9);
        // new_partner_multiplier = 1.25 (0 Raids); base=0.5*0.65+0.75*0.35=0.5875.
        assert!((upsert.new_partner_multiplier - 1.25).abs() < 1e-9);
        assert!((upsert.base_score - round_score(0.5875)).abs() < 1e-9);
        assert!((upsert.final_score - round_score(0.5875 * 1.25)).abs() < 1e-9);
        assert_eq!(upsert.last_computed_at, "2026-06-21T12:00:00+00:00");
        assert!(upsert.current_started_at.is_none());
    }

    #[test]
    fn build_live_partner_uptime_und_duration_score() {
        // Live seit 1h, avg-Dauer 2h aus >=3 zuverlässigen Sessions →
        // duration_score = (7200-3600)/7200 = 0.5.
        let now = Utc.with_ymd_and_hms(2026, 6, 21, 12, 0, 0).unwrap();
        let started = now - chrono::Duration::seconds(3600);
        let mut input = base_input("uid_live");
        input.live_state = LiveState {
            is_live: true,
            last_started_at: Some(started),
        };
        input.sessions = (0..3)
            .map(|i| SessionRow {
                started_at: Some(now - chrono::Duration::days(i + 1)),
                duration_seconds: 7200,
            })
            .collect();
        let upsert = build_score_upsert(&input, now);
        assert_eq!(upsert.is_live, 1);
        assert_eq!(upsert.avg_duration_sec, 7200);
        assert_eq!(upsert.current_uptime_sec, 3600);
        assert!((upsert.duration_score - 0.5).abs() < 1e-9);
        assert_eq!(
            upsert.current_started_at.as_deref(),
            Some(iso_utc_seconds(started).as_str())
        );
    }

    #[test]
    fn build_today_received_raids_zaehlt_berlin_datum() {
        // now = 21.6.2026 12:00 UTC (= 14:00 Berlin). Ein Raid heute, einer gestern.
        let now = Utc.with_ymd_and_hms(2026, 6, 21, 12, 0, 0).unwrap();
        let mut input = base_input("uid_today");
        input.raid_timestamps = vec![
            now - chrono::Duration::hours(2), // heute (Berlin)
            now - chrono::Duration::days(2),  // vor 2 Tagen
        ];
        let upsert = build_score_upsert(&input, now);
        assert_eq!(upsert.received_successful_raids_total, 2);
        assert_eq!(upsert.today_received_raids, 1);
    }

    #[test]
    fn build_offline_mit_cache_uebernimmt_cache_scores() {
        let now = Utc.with_ymd_and_hms(2026, 6, 21, 12, 0, 0).unwrap();
        let mut input = base_input("uid_cache");
        input.existing_cache = Some(ExistingCache {
            current_started_at: Some("2026-06-20T10:00:00+00:00".to_string()),
            current_uptime_sec: 1234,
            duration_score: 0.42,
            time_pattern_score: 0.6,
            readiness_score: 0.5,
            fairness_score: 0.7,
            base_score: 0.55,
            final_score: 0.61,
        });
        let upsert = build_score_upsert(&input, now);
        assert_eq!(upsert.is_live, 0);
        assert_eq!(upsert.current_uptime_sec, 1234);
        assert!((upsert.duration_score - 0.42).abs() < 1e-9);
        assert!((upsert.final_score - 0.61).abs() < 1e-9);
        assert_eq!(
            upsert.current_started_at.as_deref(),
            Some("2026-06-20T10:00:00+00:00")
        );
    }
}
