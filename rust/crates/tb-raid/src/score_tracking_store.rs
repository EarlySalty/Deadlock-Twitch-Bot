//! Store für bestätigte Partner-Raids (`twitch_partner_raid_score_tracking`).
//! Port des INSERT aus `partner_raid_score_tracking.py` `track_confirmed_partner_raid`.
//!
//! **Bewusste Trennung:** Das Python-Original liest quer über Subsysteme
//! (`twitch_live_state`, Score-Cache, Raid-History) und berechnet
//! `was_deadlock_at_raid`. Damit `tb-raid` nicht an die Monitoring-Tabellen
//! koppelt, nimmt dieser Store einen **bereits aufgelösten** [`TrackConfirmedInput`]
//! entgegen — die Cross-Table-Reads erledigt der Aufrufer (Composition-Root /
//! Arrival-Runtime). So bleibt der Store rein und testbar.
//!
//! Prod-Schema-Eigenheit (verifiziert): TEXT-Timestamps (`confirmed_at`,
//! `target_stream_started_at`, `score_last_computed_at`, `deadlock_continued_until`,
//! `resolved_at`), `was_deadlock_at_raid` INTEGER (nicht boolean!),
//! `raid_history_executed_at` timestamptz, Scores double precision.

use chrono::{DateTime, Utc};
use sqlx::PgPool;

/// Vollständig aufgelöste Eingabe für einen bestätigten Partner-Raid.
#[derive(Debug, Clone, Default)]
pub struct TrackConfirmedInput {
    pub raid_history_id: Option<i64>,
    pub raid_history_executed_at: Option<DateTime<Utc>>,
    pub from_broadcaster_id: Option<String>,
    pub from_broadcaster_login: String,
    pub to_broadcaster_id: String,
    pub to_broadcaster_login: String,
    pub viewer_count: i32,
    /// ISO-Text (TEXT-Spalte).
    pub confirmed_at: String,
    pub target_session_id: Option<i32>,
    pub target_stream_started_at: Option<String>,
    pub score_last_computed_at: Option<String>,
    pub final_score: Option<f64>,
    pub base_score: Option<f64>,
    pub duration_score: Option<f64>,
    pub time_pattern_score: Option<f64>,
    pub readiness_score: Option<f64>,
    pub fairness_score: Option<f64>,
    pub new_partner_multiplier: Option<f64>,
    pub raid_boost_multiplier: Option<f64>,
    pub today_received_raids: Option<i32>,
    /// INTEGER-Flag (0/1), nicht boolean.
    pub was_deadlock_at_raid: bool,
}

#[derive(Clone)]
pub struct ScoreTrackingStore {
    pool: PgPool,
}

impl ScoreTrackingStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Trägt einen bestätigten Partner-Raid ein. Leere `to_broadcaster_id` →
    /// `None` (wie Python). `deadlock_continued_*`/`resolved_*` starten NULL
    /// (werden später beim Auflösen gesetzt). Liefert die neue `id`.
    pub async fn track_confirmed(
        &self,
        input: &TrackConfirmedInput,
    ) -> Result<Option<i64>, sqlx::Error> {
        if input.to_broadcaster_id.trim().is_empty() {
            return Ok(None);
        }
        let id: (i32,) = sqlx::query_as(
            r#"
            INSERT INTO twitch_partner_raid_score_tracking (
                raid_history_id, raid_history_executed_at,
                from_broadcaster_id, from_broadcaster_login,
                to_broadcaster_id, to_broadcaster_login,
                viewer_count, confirmed_at, target_session_id,
                target_stream_started_at, score_last_computed_at,
                final_score, base_score, duration_score, time_pattern_score,
                readiness_score, fairness_score, new_partner_multiplier,
                raid_boost_multiplier, today_received_raids, was_deadlock_at_raid
            ) VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11,
                $12, $13, $14, $15, $16, $17, $18, $19, $20, $21
            )
            RETURNING id
            "#,
        )
        .bind(input.raid_history_id)
        .bind(input.raid_history_executed_at)
        .bind(
            input
                .from_broadcaster_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty()),
        )
        .bind(input.from_broadcaster_login.trim().to_lowercase())
        .bind(input.to_broadcaster_id.trim())
        .bind(input.to_broadcaster_login.trim().to_lowercase())
        .bind(input.viewer_count)
        .bind(&input.confirmed_at)
        .bind(input.target_session_id)
        .bind(&input.target_stream_started_at)
        .bind(&input.score_last_computed_at)
        .bind(input.final_score)
        .bind(input.base_score)
        .bind(input.duration_score)
        .bind(input.time_pattern_score)
        .bind(input.readiness_score)
        .bind(input.fairness_score)
        .bind(input.new_partner_multiplier)
        .bind(input.raid_boost_multiplier)
        .bind(input.today_received_raids)
        .bind(i32::from(input.was_deadlock_at_raid))
        .fetch_one(&self.pool)
        .await?;
        Ok(Some(i64::from(id.0)))
    }
}
