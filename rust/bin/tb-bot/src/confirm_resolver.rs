//! Confirm-Resolver — beschafft die Cross-Subsystem-Daten für einen bestätigten
//! Partner-Raid und baut daraus `TrackConfirmedInput`. Port der Lese-/
//! Auflösungslogik aus `partner_raid_score_tracking.py track_confirmed_partner_raid`
//! (Z. 359–470) + `_load_raid_history_reference` (Z. 214–263).
//!
//! Gehört in `tb-bot` (Composition-Root), weil es `twitch_live_state`
//! (Monitoring) + `twitch_partner_raid_scores` + `twitch_raid_history` liest und
//! daraus die tb-raid-Eingabe baut. So bleibt `tb-raid` monitoring-frei.
//!
//! **Sicherheitskritisch:** Das Ergebnis entscheidet, was als bestätigter
//! Partner-Raid getrackt wird → fließt ins Scoring → in künftige Raids.
//!
//! Noch nicht aus `main.rs` aufgerufen (Cutover-Gate).
#![allow(dead_code)]

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use serde_json::Value;
use sqlx::PgPool;
use tb_raid::{PartnerRaidScoreRow, ScoreStore, TrackConfirmedInput};

/// Eingabe-Kontext aus dem Bestätigungs-Signal.
pub struct ConfirmContext<'a> {
    pub signal_type: &'a str,
    pub to_broadcaster_id: &'a str,
    pub to_broadcaster_login: &'a str,
    pub from_broadcaster_login: &'a str,
    pub from_broadcaster_id: Option<&'a str>,
    pub viewer_count: i32,
    pub pending_target_stream_data: Option<&'a Value>,
}

struct ScoreSnapshot {
    last_computed_at: Option<String>,
    final_score: f64,
    base_score: f64,
    duration_score: f64,
    time_pattern_score: f64,
    readiness_score: f64,
    fairness_score: f64,
    new_partner_multiplier: f64,
    raid_boost_multiplier: f64,
    today_received_raids: i32,
}

impl ScoreSnapshot {
    fn defaults() -> Self {
        Self {
            last_computed_at: None,
            final_score: 0.0,
            base_score: 0.0,
            duration_score: 0.5,
            time_pattern_score: 0.5,
            readiness_score: 0.5,
            fairness_score: 0.5,
            new_partner_multiplier: 1.0,
            raid_boost_multiplier: 1.0,
            today_received_raids: 0,
        }
    }

    fn from_row(row: Option<&PartnerRaidScoreRow>) -> Self {
        let Some(row) = row else {
            return Self::defaults();
        };
        Self {
            last_computed_at: Some(row.last_computed_at.clone()),
            final_score: row.final_score,
            base_score: row.base_score,
            duration_score: row.duration_score,
            time_pattern_score: row.time_pattern_score,
            readiness_score: row.readiness_score,
            fairness_score: row.fairness_score,
            new_partner_multiplier: row.new_partner_multiplier,
            raid_boost_multiplier: row.raid_boost_multiplier,
            today_received_raids: row.today_received_raids,
        }
    }
}

fn score_f64(score: &serde_json::Map<String, Value>, key: &str, default: f64) -> f64 {
    score.get(key).and_then(Value::as_f64).unwrap_or(default)
}

fn score_i32(score: &serde_json::Map<String, Value>, key: &str, default: i32) -> i32 {
    score
        .get(key)
        .and_then(Value::as_i64)
        .and_then(|v| i32::try_from(v).ok())
        .unwrap_or(default)
}

fn score_string(score: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
    score
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn score_snapshot_from_pending(target_stream_data: Option<&Value>) -> Option<ScoreSnapshot> {
    let score = target_stream_data?.get("_partner_score")?.as_object()?;
    Some(ScoreSnapshot {
        last_computed_at: score_string(score, "last_computed_at"),
        final_score: score_f64(score, "final_score", 0.0),
        base_score: score_f64(score, "base_score", 0.0),
        duration_score: score_f64(score, "duration_score", 0.5),
        time_pattern_score: score_f64(score, "time_pattern_score", 0.5),
        readiness_score: score_f64(score, "readiness_score", 0.5),
        fairness_score: score_f64(score, "fairness_score", 0.5),
        new_partner_multiplier: score_f64(score, "new_partner_multiplier", 1.0),
        raid_boost_multiplier: score_f64(score, "raid_boost_multiplier", 1.0),
        today_received_raids: score_i32(score, "today_received_raids", 0),
    })
}

#[derive(sqlx::FromRow)]
struct LiveStateBits {
    active_session_id: Option<i64>,
    /// TEXT-Timestamp in twitch_live_state.
    last_started_at: Option<String>,
    last_game: Option<String>,
    /// Streamer-Login für den Open-Session-Fallback (P2.36/P2.37).
    streamer_login: Option<String>,
}

#[derive(Clone)]
pub struct ConfirmResolver {
    pool: PgPool,
    score_store: ScoreStore,
    /// Ziel-Spiel (lowercase), das `was_deadlock_at_raid` bestimmt — Python
    /// `_target_game_lower()` (= TWITCH_TARGET_GAME_NAME, default "deadlock").
    target_game_lower: String,
}

impl ConfirmResolver {
    pub fn new(pool: PgPool, target_game_lower: impl Into<String>) -> Self {
        let score_store = ScoreStore::new(pool.clone());
        Self {
            pool,
            score_store,
            target_game_lower: target_game_lower.into(),
        }
    }

    /// Baut `TrackConfirmedInput` aus live_state + Score-Snapshot + Raid-History.
    pub async fn resolve(
        &self,
        ctx: &ConfirmContext<'_>,
        now: DateTime<Utc>,
    ) -> Result<TrackConfirmedInput, sqlx::Error> {
        // 1. live_state: Session-ID, Stream-Start, Spiel (→ was_deadlock).
        let live: Option<LiveStateBits> = sqlx::query_as(
            "SELECT active_session_id, last_started_at, last_game, streamer_login
             FROM twitch_live_state WHERE twitch_user_id = $1",
        )
        .bind(ctx.to_broadcaster_id)
        .fetch_optional(&self.pool)
        .await?;
        let (mut target_session_id, target_stream_started_at, was_deadlock) = match &live {
            Some(l) => {
                let session = l
                    .active_session_id
                    .and_then(|v| i32::try_from(v).ok())
                    .filter(|v| *v != 0);
                let started = l
                    .last_started_at
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string);
                let deadlock = l
                    .last_game
                    .as_deref()
                    .map(|g| g.trim().to_lowercase() == self.target_game_lower)
                    .unwrap_or(false);
                (session, started, deadlock)
            }
            None => (None, None, false),
        };

        // P2.36/P2.37: Open-Session-Fallback. Ist active_session_id leer (NULL/0)
        // — Race direkt nach Stream-Start, bevor live_state die Session-ID
        // zurückschreibt — verknüpft Python (partner_raid_score_tracking.py:403-409
        // via _lookup_open_session_id) den getrackten Raid trotzdem über
        // streamer_login + ended_at IS NULL. Sonst bliebe target_session_id NULL
        // und die Zeile resolved nur über den schwächeren Confirmed-Window-Pfad.
        // Login-Quelle wie Python: live_state.streamer_login, sonst to_broadcaster_login.
        if target_session_id.is_none() {
            let login = live
                .as_ref()
                .and_then(|l| l.streamer_login.as_deref())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(ctx.to_broadcaster_login);
            target_session_id = self
                .lookup_open_session_id(login, target_stream_started_at.as_deref())
                .await?;
        }

        // 2. Score-Snapshot (Defaults wie Python `_score_payload`).
        let snapshot = match score_snapshot_from_pending(ctx.pending_target_stream_data) {
            Some(snapshot) => snapshot,
            None => {
                let row = self.score_store.load(ctx.to_broadcaster_id).await?;
                ScoreSnapshot::from_row(row.as_ref())
            }
        };

        // 3. Raid-History-Referenz (jüngster erfolgreicher Raid source→target
        //    bis confirmed_at + 10 min).
        let (raid_history_id, raid_history_executed_at) = self
            .load_raid_history_reference(ctx, now + Duration::minutes(10))
            .await?;

        Ok(TrackConfirmedInput {
            raid_history_id,
            raid_history_executed_at,
            from_broadcaster_id: ctx.from_broadcaster_id.map(str::to_string),
            from_broadcaster_login: ctx.from_broadcaster_login.to_string(),
            to_broadcaster_id: ctx.to_broadcaster_id.to_string(),
            to_broadcaster_login: ctx.to_broadcaster_login.to_string(),
            viewer_count: ctx.viewer_count,
            confirmed_at: now.to_rfc3339_opts(SecondsFormat::Secs, false),
            target_session_id,
            target_stream_started_at,
            // Ohne Score-Cache-Zeile schreibt Python (_score_payload({})) feste
            // neutrale Defaults statt NULL — sonst verzerren NULLs die AVG-
            // Aggregationen über das Tracking. final/base=0.0, *_score=0.5,
            // multipliers=1.0, today=0. score_last_computed_at bleibt NULL.
            score_last_computed_at: snapshot.last_computed_at,
            final_score: Some(snapshot.final_score),
            base_score: Some(snapshot.base_score),
            duration_score: Some(snapshot.duration_score),
            time_pattern_score: Some(snapshot.time_pattern_score),
            readiness_score: Some(snapshot.readiness_score),
            fairness_score: Some(snapshot.fairness_score),
            new_partner_multiplier: Some(snapshot.new_partner_multiplier),
            raid_boost_multiplier: Some(snapshot.raid_boost_multiplier),
            today_received_raids: Some(snapshot.today_received_raids),
            was_deadlock_at_raid: was_deadlock,
        })
    }

    /// Port von `_load_raid_history_reference` (Z. 214–263): jüngster
    /// erfolgreicher Raid source→target, `executed_at <= upper_bound`.
    async fn load_raid_history_reference(
        &self,
        ctx: &ConfirmContext<'_>,
        upper_bound: DateTime<Utc>,
    ) -> Result<(Option<i64>, Option<DateTime<Utc>>), sqlx::Error> {
        let row: Option<(i64, DateTime<Utc>)> = match ctx.from_broadcaster_id {
            Some(from_id) if !from_id.trim().is_empty() => {
                sqlx::query_as(
                    "SELECT id, executed_at FROM twitch_raid_history
                      WHERE to_broadcaster_id = $1 AND LOWER(to_broadcaster_login) = LOWER($2)
                        AND from_broadcaster_id = $3 AND LOWER(from_broadcaster_login) = LOWER($4)
                        AND COALESCE(success, FALSE) IS TRUE AND executed_at <= $5
                      ORDER BY executed_at DESC, id DESC LIMIT 1",
                )
                .bind(ctx.to_broadcaster_id)
                .bind(ctx.to_broadcaster_login)
                .bind(from_id)
                .bind(ctx.from_broadcaster_login)
                .bind(upper_bound)
                .fetch_optional(&self.pool)
                .await?
            }
            _ => {
                sqlx::query_as(
                    "SELECT id, executed_at FROM twitch_raid_history
                      WHERE to_broadcaster_id = $1 AND LOWER(to_broadcaster_login) = LOWER($2)
                        AND LOWER(from_broadcaster_login) = LOWER($3)
                        AND COALESCE(success, FALSE) IS TRUE AND executed_at <= $4
                      ORDER BY executed_at DESC, id DESC LIMIT 1",
                )
                .bind(ctx.to_broadcaster_id)
                .bind(ctx.to_broadcaster_login)
                .bind(ctx.from_broadcaster_login)
                .bind(upper_bound)
                .fetch_optional(&self.pool)
                .await?
            }
        };
        Ok(match row {
            Some((id, executed_at)) => (Some(id).filter(|v| *v != 0), Some(executed_at)),
            None => (None, None),
        })
    }

    /// Port von `_lookup_open_session_id` (partner_raid_score_tracking.py:138-161):
    /// jüngste offene Session (`ended_at IS NULL`) des Streamers; bevorzugt die,
    /// deren `started_at` exakt zu `target_stream_started_at` passt.
    async fn lookup_open_session_id(
        &self,
        streamer_login: &str,
        target_stream_started_at: Option<&str>,
    ) -> Result<Option<i32>, sqlx::Error> {
        let login = streamer_login.trim();
        if login.is_empty() {
            return Ok(None);
        }
        let id: Option<i64> = sqlx::query_scalar(
            "SELECT id FROM twitch_stream_sessions
              WHERE LOWER(streamer_login) = LOWER($1)
                AND ended_at IS NULL
              ORDER BY
                CASE WHEN COALESCE(started_at, '') = COALESCE($2, '') THEN 0 ELSE 1 END,
                started_at DESC,
                id DESC
              LIMIT 1",
        )
        .bind(login)
        .bind(target_stream_started_at)
        .fetch_optional(&self.pool)
        .await?;
        Ok(id.and_then(|v| i32::try_from(v).ok()).filter(|v| *v != 0))
    }
}

#[cfg(all(test, feature = "integration"))]
mod tests {
    use super::*;
    use std::str::FromStr;

    async fn setup(schema: &str) -> PgPool {
        let url = std::env::var("TB_TEST_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://postgres:tbtest@127.0.0.1:5434/postgres".to_string());
        let admin = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
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
        let opts = sqlx::postgres::PgConnectOptions::from_str(&url)
            .unwrap()
            .options([("search_path", schema)]);
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(4)
            .connect_with(opts)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE twitch_live_state (twitch_user_id TEXT PRIMARY KEY, streamer_login TEXT,
                last_started_at TEXT, last_game TEXT, active_session_id BIGINT)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE twitch_raid_history (id BIGSERIAL PRIMARY KEY, from_broadcaster_id TEXT,
                from_broadcaster_login TEXT, to_broadcaster_id TEXT, to_broadcaster_login TEXT,
                executed_at TIMESTAMPTZ, success BOOLEAN)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE twitch_stream_sessions (id BIGSERIAL PRIMARY KEY, streamer_login TEXT,
                started_at TEXT, ended_at TEXT)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE twitch_partner_raid_scores (twitch_user_id TEXT PRIMARY KEY, twitch_login TEXT DEFAULT '',
                avg_duration_sec INTEGER DEFAULT 0, time_pattern_score_base DOUBLE PRECISION DEFAULT 0.5,
                received_successful_raids_total INTEGER DEFAULT 0, is_new_partner_preferred INTEGER DEFAULT 0,
                new_partner_multiplier DOUBLE PRECISION DEFAULT 1.0, raid_boost_multiplier DOUBLE PRECISION DEFAULT 1.0,
                is_live INTEGER DEFAULT 0, current_started_at TEXT, current_uptime_sec INTEGER DEFAULT 0,
                duration_score DOUBLE PRECISION DEFAULT 0.5, time_pattern_score DOUBLE PRECISION DEFAULT 0.5,
                readiness_score DOUBLE PRECISION DEFAULT 0.5, fairness_score DOUBLE PRECISION DEFAULT 0.5,
                base_score DOUBLE PRECISION DEFAULT 0.5, final_score DOUBLE PRECISION DEFAULT 0.5,
                internal_sent_raids_30d INTEGER DEFAULT 0, internal_received_raids_30d INTEGER DEFAULT 0,
                internal_received_raids_7d INTEGER DEFAULT 0, today_received_raids INTEGER DEFAULT 0,
                last_computed_at TEXT DEFAULT '')",
        ).execute(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn resolve_zieht_session_deadlock_score_und_history() {
        let pool = setup("t6e_confirm_resolver").await;
        let now = chrono::DateTime::parse_from_rfc3339("2026-06-10T18:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc);

        sqlx::query("INSERT INTO twitch_live_state (twitch_user_id, streamer_login, last_started_at, last_game, active_session_id)
                     VALUES ('200', 'dst', '2026-06-10T16:00:00+00:00', 'Deadlock', 77)")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_partner_raid_scores (twitch_user_id, final_score, today_received_raids, last_computed_at)
                     VALUES ('200', 0.87, 3, 'gestern')")
            .execute(&pool).await.unwrap();
        // Erfolgreicher Raid 100->200 vor confirmed_at. executed_at relativ zur
        // simulierten `now` (nicht Wall-Clock-NOW()) — sonst hängt der Test am
        // Kalender: der Resolver filtert `executed_at <= now`, und ein echtes
        // NOW() liegt nach dem 2026-06-10-Stichtag.
        sqlx::query(
            "INSERT INTO twitch_raid_history (from_broadcaster_id, from_broadcaster_login,
                     to_broadcaster_id, to_broadcaster_login, executed_at, success)
                     VALUES ('100', 'src', '200', 'dst', $1, TRUE)",
        )
        .bind(now - chrono::Duration::minutes(1))
        .execute(&pool)
        .await
        .unwrap();

        let resolver = ConfirmResolver::new(pool.clone(), "deadlock");
        let ctx = ConfirmContext {
            signal_type: "channel.raid",
            to_broadcaster_id: "200",
            to_broadcaster_login: "dst",
            from_broadcaster_login: "src",
            from_broadcaster_id: Some("100"),
            viewer_count: 42,
            pending_target_stream_data: None,
        };
        let input = resolver.resolve(&ctx, now).await.unwrap();

        assert_eq!(input.target_session_id, Some(77));
        assert_eq!(
            input.target_stream_started_at.as_deref(),
            Some("2026-06-10T16:00:00+00:00")
        );
        assert!(
            input.was_deadlock_at_raid,
            "last_game=Deadlock → was_deadlock"
        );
        assert_eq!(input.final_score, Some(0.87));
        assert_eq!(input.today_received_raids, Some(3));
        assert!(
            input.raid_history_id.is_some(),
            "Raid-History-Referenz gefunden"
        );

        // Kein Deadlock + keine History/Score → Defaults/None.
        sqlx::query(
            "UPDATE twitch_live_state SET last_game='Just Chatting' WHERE twitch_user_id='200'",
        )
        .execute(&pool)
        .await
        .unwrap();
        let input2 = resolver.resolve(&ctx, now).await.unwrap();
        assert!(
            !input2.was_deadlock_at_raid,
            "Just Chatting → kein Deadlock"
        );
    }

    #[tokio::test]
    async fn resolve_nutzt_pending_score_snapshot_vor_db_score() {
        let pool = setup("t6e_confirm_pending_score_snapshot").await;
        let now = chrono::DateTime::parse_from_rfc3339("2026-06-10T18:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc);

        sqlx::query("INSERT INTO twitch_live_state (twitch_user_id, streamer_login, last_started_at, last_game, active_session_id)
                     VALUES ('200', 'dst', '2026-06-10T16:00:00+00:00', 'Deadlock', 77)")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_partner_raid_scores (twitch_user_id, final_score, today_received_raids, last_computed_at)
                     VALUES ('200', 0.11, 9, 'frisch')")
            .execute(&pool).await.unwrap();
        let pending_snapshot = serde_json::json!({
            "_partner_score": {
                "final_score": 0.91,
                "base_score": 0.81,
                "duration_score": 0.71,
                "time_pattern_score": 0.61,
                "readiness_score": 0.51,
                "fairness_score": 0.41,
                "new_partner_multiplier": 1.2,
                "raid_boost_multiplier": 1.3,
                "today_received_raids": 2,
                "last_computed_at": "eingefroren"
            }
        });

        let resolver = ConfirmResolver::new(pool.clone(), "deadlock");
        let ctx = ConfirmContext {
            signal_type: "channel.raid",
            to_broadcaster_id: "200",
            to_broadcaster_login: "dst",
            from_broadcaster_login: "src",
            from_broadcaster_id: Some("100"),
            viewer_count: 42,
            pending_target_stream_data: Some(&pending_snapshot),
        };
        let input = resolver.resolve(&ctx, now).await.unwrap();

        assert_eq!(input.final_score, Some(0.91));
        assert_eq!(input.base_score, Some(0.81));
        assert_eq!(input.today_received_raids, Some(2));
        assert_eq!(input.score_last_computed_at.as_deref(), Some("eingefroren"));
    }

    #[tokio::test]
    async fn target_session_id_faellt_auf_offene_session_zurueck() {
        // P2.36/P2.37: Ist twitch_live_state.active_session_id leer (NULL/0) zur
        // Confirm-Zeit (Race nach Stream-Start), muss der Resolver die offene
        // twitch_stream_sessions-Zeile (ended_at IS NULL) des Streamers verknüpfen
        // — sonst bleibt target_session_id NULL und der getrackte Raid resolved
        // nur über den schwächeren Confirmed-Window-Fallback.
        let pool = setup("t6e_confirm_open_session").await;
        let now = chrono::DateTime::parse_from_rfc3339("2026-06-10T18:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc);

        // live_state: active_session_id NULL.
        sqlx::query("INSERT INTO twitch_live_state (twitch_user_id, streamer_login, last_started_at, last_game, active_session_id)
                     VALUES ('200', 'dst', '2026-06-10T16:00:00+00:00', 'Deadlock', NULL)")
            .execute(&pool).await.unwrap();
        // Eine bereits beendete Session (darf NICHT gewählt werden).
        sqlx::query(
            "INSERT INTO twitch_stream_sessions (id, streamer_login, started_at, ended_at)
                     VALUES (11, 'dst', '2026-06-10T10:00:00+00:00', '2026-06-10T12:00:00+00:00')",
        )
        .execute(&pool)
        .await
        .unwrap();
        // Die offene Session, deren started_at exakt zu live_state.last_started_at passt.
        sqlx::query(
            "INSERT INTO twitch_stream_sessions (id, streamer_login, started_at, ended_at)
                     VALUES (42, 'dst', '2026-06-10T16:00:00+00:00', NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();
        // Eine weitere offene Session eines anderen Streamers (Login-Filter greift).
        sqlx::query(
            "INSERT INTO twitch_stream_sessions (id, streamer_login, started_at, ended_at)
                     VALUES (99, 'andere', '2026-06-10T17:00:00+00:00', NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let resolver = ConfirmResolver::new(pool.clone(), "deadlock");
        let ctx = ConfirmContext {
            signal_type: "channel.chat.notification",
            to_broadcaster_id: "200",
            to_broadcaster_login: "dst",
            from_broadcaster_login: "src",
            from_broadcaster_id: Some("100"),
            viewer_count: 7,
            pending_target_stream_data: None,
        };
        let input = resolver.resolve(&ctx, now).await.unwrap();
        assert_eq!(
            input.target_session_id,
            Some(42),
            "active_session_id NULL -> offene Session 42 (started_at-Match) verknüpft"
        );
    }

    #[tokio::test]
    async fn target_session_id_fallback_login_aus_ctx_wenn_live_state_login_leer() {
        // streamer_login in live_state leer -> Fallback auf ctx.to_broadcaster_login.
        let pool = setup("t6e_confirm_open_session_login_fallback").await;
        let now = chrono::DateTime::parse_from_rfc3339("2026-06-10T18:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc);

        sqlx::query("INSERT INTO twitch_live_state (twitch_user_id, streamer_login, last_started_at, last_game, active_session_id)
                     VALUES ('200', '', '2026-06-10T16:00:00+00:00', 'Deadlock', 0)")
            .execute(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO twitch_stream_sessions (id, streamer_login, started_at, ended_at)
                     VALUES (55, 'dst', '2026-06-10T15:00:00+00:00', NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let resolver = ConfirmResolver::new(pool.clone(), "deadlock");
        let ctx = ConfirmContext {
            signal_type: "channel.chat.notification",
            to_broadcaster_id: "200",
            to_broadcaster_login: "DST",
            from_broadcaster_login: "src",
            from_broadcaster_id: Some("100"),
            viewer_count: 7,
            pending_target_stream_data: None,
        };
        let input = resolver.resolve(&ctx, now).await.unwrap();
        assert_eq!(
            input.target_session_id,
            Some(55),
            "active_session_id 0 + leerer live_state-Login -> Login aus ctx, offene Session verknüpft"
        );
    }
}
