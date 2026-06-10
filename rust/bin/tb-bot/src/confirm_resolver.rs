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
use sqlx::PgPool;
use tb_raid::{ScoreStore, TrackConfirmedInput};

/// Eingabe-Kontext aus dem Bestätigungs-Signal.
pub struct ConfirmContext<'a> {
    pub signal_type: &'a str,
    pub to_broadcaster_id: &'a str,
    pub to_broadcaster_login: &'a str,
    pub from_broadcaster_login: &'a str,
    pub from_broadcaster_id: Option<&'a str>,
    pub viewer_count: i32,
}

#[derive(sqlx::FromRow)]
struct LiveStateBits {
    active_session_id: Option<i64>,
    /// TEXT-Timestamp in twitch_live_state.
    last_started_at: Option<String>,
    last_game: Option<String>,
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
            "SELECT active_session_id, last_started_at, last_game
             FROM twitch_live_state WHERE twitch_user_id = $1",
        )
        .bind(ctx.to_broadcaster_id)
        .fetch_optional(&self.pool)
        .await?;
        let (target_session_id, target_stream_started_at, was_deadlock) = match &live {
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

        // 2. Score-Snapshot (Defaults wie Python `_score_payload`).
        let snapshot = self.score_store.load(ctx.to_broadcaster_id).await?;

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
            score_last_computed_at: snapshot.as_ref().map(|s| s.last_computed_at.clone()),
            final_score: snapshot.as_ref().map(|s| s.final_score),
            base_score: snapshot.as_ref().map(|s| s.base_score),
            duration_score: snapshot.as_ref().map(|s| s.duration_score),
            time_pattern_score: snapshot.as_ref().map(|s| s.time_pattern_score),
            readiness_score: snapshot.as_ref().map(|s| s.readiness_score),
            fairness_score: snapshot.as_ref().map(|s| s.fairness_score),
            new_partner_multiplier: snapshot.as_ref().map(|s| s.new_partner_multiplier),
            raid_boost_multiplier: snapshot.as_ref().map(|s| s.raid_boost_multiplier),
            today_received_raids: snapshot.as_ref().map(|s| s.today_received_raids),
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
            "CREATE TABLE twitch_live_state (twitch_user_id TEXT PRIMARY KEY, last_started_at TEXT,
                last_game TEXT, active_session_id BIGINT)",
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

        sqlx::query("INSERT INTO twitch_live_state (twitch_user_id, last_started_at, last_game, active_session_id)
                     VALUES ('200', '2026-06-10T16:00:00+00:00', 'Deadlock', 77)")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_partner_raid_scores (twitch_user_id, final_score, today_received_raids, last_computed_at)
                     VALUES ('200', 0.87, 3, 'gestern')")
            .execute(&pool).await.unwrap();
        // Erfolgreicher Raid 100->200 vor confirmed_at.
        sqlx::query(
            "INSERT INTO twitch_raid_history (from_broadcaster_id, from_broadcaster_login,
                     to_broadcaster_id, to_broadcaster_login, executed_at, success)
                     VALUES ('100', 'src', '200', 'dst', NOW() - INTERVAL '1 minute', TRUE)",
        )
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
}
