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

use crate::util::parse_iso_utc;

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
    /// `None` (wie Python). Bei Deadlock-Raids starten `deadlock_continued_*`/
    /// `resolved_*` NULL (werden später beim Auflösen gesetzt); Nicht-Deadlock-Raids
    /// werden sofort als aufgelöst eingetragen. Liefert die neue `id`.
    pub async fn track_confirmed(
        &self,
        input: &TrackConfirmedInput,
    ) -> Result<Option<i64>, sqlx::Error> {
        if input.to_broadcaster_id.trim().is_empty() {
            return Ok(None);
        }
        // Nicht-Deadlock-Raids (was_deadlock_at_raid=false) werden sofort als
        // aufgelöst eingetragen (Dauer 0, Grund "not_deadlock_at_raid") — Python
        // partner_raid_score_tracking.py:421-481. Sonst NULL (späteres Auflösen).
        let (continued_until, continued_sec, resolved_at, resolution_reason): (
            Option<&str>,
            Option<i32>,
            Option<&str>,
            Option<&str>,
        ) = if input.was_deadlock_at_raid {
            (None, None, None, None)
        } else {
            (
                Some(input.confirmed_at.as_str()),
                Some(0),
                Some(input.confirmed_at.as_str()),
                Some("not_deadlock_at_raid"),
            )
        };
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
                raid_boost_multiplier, today_received_raids, was_deadlock_at_raid,
                deadlock_continued_until, deadlock_continued_sec, resolved_at, resolution_reason
            ) VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11,
                $12, $13, $14, $15, $16, $17, $18, $19, $20, $21,
                $22, $23, $24, $25
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
        .bind(continued_until)
        .bind(continued_sec)
        .bind(resolved_at)
        .bind(resolution_reason)
        .fetch_one(&self.pool)
        .await?;
        Ok(Some(i64::from(id.0)))
    }

    /// Löst die noch offenen Tracking-Zeilen einer abgeschlossenen Session auf.
    /// Port von `resolve_partner_raid_tracking_for_session`
    /// (partner_raid_score_tracking.py Z. 521–628). Wird beim Session-Finalize
    /// aufgerufen (über den [`crate`]-fremden Resolver-Port in tb-monitoring).
    ///
    /// Ablauf je Zeile (`resolved_at IS NULL`):
    /// - `was_deadlock_at_raid` → `twitch_channel_updates` im Fenster
    ///   `[confirmed_at, ended_at]` scannen; erstes Nicht-Ziel-Spiel setzt
    ///   `resolution_dt` = dessen `recorded_at` (`channel_update_non_deadlock`),
    ///   sonst Session-Ende (`session_ended`).
    /// - sonst sofort Session-Ende mit `not_deadlock_at_raid`.
    ///
    /// `deadlock_continued_sec = max(0, resolution_dt - confirmed_at)`,
    /// `resolved_at = ended_at`. Gibt die Anzahl aufgelöster Zeilen zurück;
    /// `0` bei fehlender `session_id`/`ended_at` oder DB-Fehler (best-effort wie
    /// Python). `target_game_lower` ist das normalisierte Ziel-Spiel
    /// (Python `_target_game_lower`), vom Aufrufer durchgereicht statt aus env.
    pub async fn resolve_for_session(
        &self,
        twitch_user_id: Option<&str>,
        streamer_login: &str,
        session_id: Option<i64>,
        session_ended_at: Option<DateTime<Utc>>,
        target_game_lower: &str,
    ) -> i64 {
        let Some(session_id) = session_id else {
            return 0;
        };
        let Some(ended_at) = session_ended_at else {
            return 0;
        };
        match self
            .resolve_for_session_inner(
                twitch_user_id,
                streamer_login,
                session_id,
                ended_at,
                target_game_lower,
            )
            .await
        {
            Ok(resolved) => {
                if resolved > 0 {
                    tracing::info!(
                        streamer = streamer_login.trim().to_lowercase(),
                        session_id,
                        rows = resolved,
                        "Partner-Raid-Score-Tracking für Session aufgelöst"
                    );
                }
                resolved
            }
            Err(error) => {
                // best-effort wie Python (Z. 619–626): Fehler loggen, 0 melden.
                tracing::debug!(%error, session_id, "Partner-Raid-Score-Tracking-Resolve fehlgeschlagen");
                0
            }
        }
    }

    async fn resolve_for_session_inner(
        &self,
        twitch_user_id: Option<&str>,
        streamer_login: &str,
        session_id: i64,
        ended_at: DateTime<Utc>,
        target_game_lower: &str,
    ) -> Result<i64, sqlx::Error> {
        let target_id = twitch_user_id.map(str::trim).unwrap_or("").to_string();
        let login_lower = streamer_login.trim().to_lowercase();
        let target_game_lower = target_game_lower.trim().to_lowercase();

        // Session-Start (TIMESTAMPTZ) für den Fallback-Lade-Zweig.
        let session_started_at: Option<DateTime<Utc>> =
            sqlx::query_scalar("SELECT started_at FROM twitch_stream_sessions WHERE id = $1 LIMIT 1")
                .bind(session_id)
                .fetch_optional(&self.pool)
                .await?
                .flatten();

        let rows = self
            .load_unresolved_rows(
                session_id,
                &target_id,
                &login_lower,
                session_started_at,
                ended_at,
            )
            .await?;
        if rows.is_empty() {
            return Ok(0);
        }

        let mut resolved = 0i64;
        for row in rows {
            let TrackingRow {
                id: tracking_id,
                confirmed_at,
                to_broadcaster_id,
                was_deadlock_at_raid,
            } = row;
            let Some(confirmed_at_dt) = parse_iso_utc(&confirmed_at) else {
                continue;
            };

            let mut resolution_dt = ended_at;
            let mut resolution_reason = "session_ended";
            if was_deadlock_at_raid {
                // Ziel-User für die Channel-Update-Suche: Zeilen-ID, sonst session-Target.
                let tracked_user_id = if to_broadcaster_id.trim().is_empty() {
                    target_id.clone()
                } else {
                    to_broadcaster_id.trim().to_string()
                };
                let updates: Vec<(Option<String>, DateTime<Utc>)> = sqlx::query_as(
                    "SELECT game_name, recorded_at FROM twitch_channel_updates \
                     WHERE twitch_user_id = $1 AND recorded_at >= $2 AND recorded_at <= $3 \
                     ORDER BY recorded_at ASC",
                )
                .bind(if tracked_user_id.is_empty() {
                    target_id.clone()
                } else {
                    tracked_user_id
                })
                .bind(confirmed_at_dt)
                .bind(ended_at)
                .fetch_all(&self.pool)
                .await?;
                for (game_name, recorded_at) in updates {
                    let game = game_name.unwrap_or_default().trim().to_lowercase();
                    if game.is_empty() {
                        continue;
                    }
                    if game != target_game_lower {
                        resolution_dt = recorded_at;
                        resolution_reason = "channel_update_non_deadlock";
                        break;
                    }
                }
            } else {
                resolution_reason = "not_deadlock_at_raid";
            }

            let duration_sec = (resolution_dt - confirmed_at_dt).num_seconds().max(0) as i32;
            sqlx::query(
                "UPDATE twitch_partner_raid_score_tracking \
                 SET deadlock_continued_until = $1, deadlock_continued_sec = $2, \
                     resolved_at = $3, resolution_reason = $4 \
                 WHERE id = $5",
            )
            .bind(iso_utc(resolution_dt))
            .bind(duration_sec)
            .bind(iso_utc(ended_at))
            .bind(resolution_reason)
            .bind(tracking_id)
            .execute(&self.pool)
            .await?;
            resolved += 1;
        }
        Ok(resolved)
    }

    /// Lädt die offenen Tracking-Zeilen (`resolved_at IS NULL`) für eine Session.
    /// Primär über `target_session_id`, plus Fallback über
    /// `target_session_id IS NULL` + Ziel-Identität + Confirmed-Zeitfenster
    /// (Python `_load_unresolved_tracking_rows_for_session` Z. 279–356).
    /// Beide Quellen werden über `id` dedupliziert und nach `(confirmed_at, id)`
    /// sortiert.
    async fn load_unresolved_rows(
        &self,
        session_id: i64,
        target_id: &str,
        login_lower: &str,
        session_started_at: Option<DateTime<Utc>>,
        session_ended_at: DateTime<Utc>,
    ) -> Result<Vec<TrackingRow>, sqlx::Error> {
        let primary: Vec<TrackingRowRaw> = sqlx::query_as(
            "SELECT id, confirmed_at, to_broadcaster_id, was_deadlock_at_raid \
             FROM twitch_partner_raid_score_tracking \
             WHERE target_session_id = $1 AND resolved_at IS NULL \
             ORDER BY confirmed_at ASC, id ASC",
        )
        .bind(session_id as i32)
        .fetch_all(&self.pool)
        .await?;

        let Some(started_at) = session_started_at else {
            return Ok(primary.into_iter().map(TrackingRowRaw::into_row).collect());
        };

        // Fallback nur mit auflösbarer Ziel-Identität (Python Z. 303–310).
        let fallback: Vec<TrackingRowRaw> = if !target_id.trim().is_empty() {
            sqlx::query_as(
                "SELECT id, confirmed_at, to_broadcaster_id, was_deadlock_at_raid \
                 FROM twitch_partner_raid_score_tracking \
                 WHERE target_session_id IS NULL AND resolved_at IS NULL \
                   AND to_broadcaster_id = $1 \
                   AND confirmed_at >= $2 AND confirmed_at <= $3 \
                   AND (target_stream_started_at IS NULL OR target_stream_started_at = $4) \
                 ORDER BY confirmed_at ASC, id ASC",
            )
            .bind(target_id.trim())
            .bind(iso_utc(started_at))
            .bind(iso_utc(session_ended_at))
            .bind(iso_utc(started_at))
            .fetch_all(&self.pool)
            .await?
        } else if !login_lower.is_empty() {
            sqlx::query_as(
                "SELECT id, confirmed_at, to_broadcaster_id, was_deadlock_at_raid \
                 FROM twitch_partner_raid_score_tracking \
                 WHERE target_session_id IS NULL AND resolved_at IS NULL \
                   AND LOWER(to_broadcaster_login) = LOWER($1) \
                   AND confirmed_at >= $2 AND confirmed_at <= $3 \
                   AND (target_stream_started_at IS NULL OR target_stream_started_at = $4) \
                 ORDER BY confirmed_at ASC, id ASC",
            )
            .bind(login_lower)
            .bind(iso_utc(started_at))
            .bind(iso_utc(session_ended_at))
            .bind(iso_utc(started_at))
            .fetch_all(&self.pool)
            .await?
        } else {
            // Weder Ziel-ID noch Login → keine Fallback-Zeilen (Python Z. 309–310).
            return Ok(Vec::new());
        };

        if fallback.is_empty() {
            return Ok(primary.into_iter().map(TrackingRowRaw::into_row).collect());
        }

        // Über id dedupizieren, primäre Zeilen gewinnen (Python Z. 340–356).
        let mut combined: std::collections::HashMap<i32, TrackingRow> =
            std::collections::HashMap::new();
        for row in primary.into_iter().chain(fallback) {
            combined.entry(row.id).or_insert_with(|| row.into_row());
        }
        let mut rows: Vec<TrackingRow> = combined.into_values().collect();
        rows.sort_by(|a, b| {
            let a_dt = parse_iso_utc(&a.confirmed_at);
            let b_dt = parse_iso_utc(&b.confirmed_at);
            a_dt.cmp(&b_dt).then(a.id.cmp(&b.id))
        });
        Ok(rows)
    }
}

/// Eine offene Tracking-Zeile (Subset der Spalten, das der Resolve braucht).
/// `to_broadcaster_id`/`was_deadlock_at_raid` können in der Prod-Tabelle NULL
/// sein → `Option`, beim Lesen auf den Default abgebildet.
#[derive(Debug, Clone, sqlx::FromRow)]
struct TrackingRowRaw {
    id: i32,
    confirmed_at: Option<String>,
    to_broadcaster_id: Option<String>,
    /// INTEGER-Flag (0/1).
    was_deadlock_at_raid: Option<i32>,
}

/// Normalisierte Tracking-Zeile (nach Default-Auflösung der NULL-Felder).
struct TrackingRow {
    id: i32,
    confirmed_at: String,
    to_broadcaster_id: String,
    was_deadlock_at_raid: bool,
}

impl TrackingRowRaw {
    fn into_row(self) -> TrackingRow {
        TrackingRow {
            id: self.id,
            confirmed_at: self.confirmed_at.unwrap_or_default(),
            to_broadcaster_id: self.to_broadcaster_id.unwrap_or_default(),
            was_deadlock_at_raid: self.was_deadlock_at_raid.unwrap_or(0) != 0,
        }
    }
}

/// ISO-UTC-Text (sekundengenau, `+00:00`-Offset) für die TEXT-Spalten —
/// Python `_iso_utc` (`isoformat(timespec="seconds")`).
fn iso_utc(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(chrono::SecondsFormat::Secs, false)
}
