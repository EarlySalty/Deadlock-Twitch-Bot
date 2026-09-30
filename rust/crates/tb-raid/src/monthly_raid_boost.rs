//! Monatlicher Effort-Sieger und dessen zeitlich begrenzter Raid-Ziel-Boost.
//!
//! Die Effort-Engine selbst lebt außerhalb von tb-raid. Dieses Modul konsumiert
//! ausschließlich ihr append-only Event-Ledger partner_effort_events,
//! friert den abgeschlossenen Kalendermonat ein und verwaltet den daraus
//! entstehenden Zwei-Stream-Boost.
//!
//! Semantik:
//! - Monatsgrenze ist Europe/Berlin.
//! - Der Grant gilt 30 Tage ab Vergabe und für zwei qualifizierende Streams.
//! - Ein Stream reserviert den Grant beim Start. Deshalb bleibt der Boost für
//!   diesen Stream bis zum Ende aktiv, auch wenn die 30 Tage währenddessen
//!   ablaufen.
//! - Erst nach mindestens 30 Minuten Deadlock innerhalb desselben Streams wird
//!   ein Stream abgezogen.
//! - Plan- und Monatsboost stapeln nicht; beide ergeben denselben 1.15-Faktor.

use chrono::{DateTime, Datelike, Duration, TimeZone, Utc};
use chrono_tz::Europe::Berlin;
use sqlx::{Connection, PgConnection, PgPool, Postgres, Transaction};

use crate::scoring::RAID_BOOST_MULTIPLIER;

pub const MONTHLY_BOOST_STREAMS: i16 = 2;
pub const MONTHLY_BOOST_TTL_DAYS: i64 = 30;
pub const QUALIFYING_DEADLOCK_SECONDS: i64 = 30 * 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeasonWindow {
    pub key: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeasonWinner {
    pub twitch_user_id: String,
    pub twitch_login: String,
    pub points: i64,
    pub qualified_invites: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeasonCloseOutcome {
    SourceUnavailable {
        season_key: String,
    },
    AlreadyClosed {
        season_key: String,
    },
    Closed {
        season_key: String,
        partners: usize,
        winner: Option<SeasonWinner>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeasonalBoostState {
    pub stream_boost_active: bool,
    pub grant_id: Option<i64>,
    pub consumed_stream: bool,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct EffortStandingRow {
    twitch_user_id: String,
    twitch_login: String,
    points: i64,
    qualified_invites: i64,
    score_reached_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct OpenUsageRow {
    id: i64,
    grant_id: i64,
    session_id: i64,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct SessionForBoost {
    id: i64,
    started_at: DateTime<Utc>,
    ended_at: Option<DateTime<Utc>>,
    game_name: Option<String>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct CurrentLiveState {
    is_live: i32,
    active_session_id: Option<i64>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct GrantRow {
    id: i64,
    granted_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    streams_remaining: i16,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct GameUpdateRow {
    recorded_at: DateTime<Utc>,
    game_name: Option<String>,
}

#[derive(Clone)]
pub struct MonthlyRaidBoostStore {
    pool: PgPool,
}

impl MonthlyRaidBoostStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Schließt den unmittelbar vorherigen Berlin-Kalendermonat.
    ///
    /// Der Quellvertrag aus der Effort-Engine ist bewusst klein:
    /// partner_effort_events(partner_twitch_user_id, partner_login, event_type, source_id, points, occurred_at).
    /// Fehlt die Tabelle noch, wird nichts geschlossen. Dadurch kann dieser
    /// Baustein vor dem Effort-PR landen, ohne einen zweiten Event-Store zu bauen.
    pub async fn close_previous_season(
        &self,
        now: DateTime<Utc>,
    ) -> Result<SeasonCloseOutcome, sqlx::Error> {
        self.close_season_ending_at(now, now).await
    }

    /// Offene Monate seit dem Start des Programms, auch nach einem längeren
    /// Ausfall. Der letzte Monat bleibt für einen ausstehenden Score-Refresh
    /// enthalten; es werden keine Monate vor dem Programmstart erfunden.
    pub async fn due_season_cutoffs(
        &self,
        now: DateTime<Utc>,
    ) -> Result<Vec<DateTime<Utc>>, sqlx::Error> {
        sqlx::query_scalar("SELECT month_end AT TIME ZONE 'Europe/Berlin' FROM partner_effort_program p CROSS JOIN LATERAL generate_series(date_trunc('month',p.started_at AT TIME ZONE 'Europe/Berlin') + interval '1 month', date_trunc('month',$1::timestamptz AT TIME ZONE 'Europe/Berlin'), interval '1 month') month_end WHERE p.singleton AND (month_end AT TIME ZONE 'Europe/Berlin') + interval '5 minutes' <= $1 AND (month_end = date_trunc('month',$1::timestamptz AT TIME ZONE 'Europe/Berlin') OR NOT EXISTS(SELECT 1 FROM twitch_partner_effort_season_closures c WHERE c.season_ended_at = month_end AT TIME ZONE 'Europe/Berlin')) ORDER BY month_end")
            .bind(now).fetch_all(&self.pool).await
    }

    pub async fn close_season_ending_at(
        &self,
        cutoff: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<SeasonCloseOutcome, sqlx::Error> {
        let window = previous_season_window(cutoff);
        let mut lock_connection = PgConnection::connect_with(&self.pool.connect_options()).await?;
        let mut lock = lock_connection.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock(713219, 27)")
            .execute(&mut *lock)
            .await?;
        let mut tx = self.pool.begin().await?;
        let closed: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM twitch_partner_effort_season_closures WHERE season_key=$1)")
            .bind(&window.key).fetch_one(&mut *tx).await?;
        if closed {
            tx.rollback().await?;
            return Ok(SeasonCloseOutcome::AlreadyClosed {
                season_key: window.key,
            });
        }

        let source_exists: Option<String> =
            sqlx::query_scalar("SELECT to_regclass('partner_effort_events')::text")
                .fetch_one(&mut *tx)
                .await?;
        if source_exists.is_none() {
            tx.rollback().await?;
            return Ok(SeasonCloseOutcome::SourceUnavailable {
                season_key: window.key,
            });
        }

        let ready: bool = sqlx::query_scalar("SELECT COUNT(*) = 7 FROM partner_effort_source_state WHERE source=ANY($1) AND healthy AND successful_at >= $2")
            .bind(vec!["invites", "referrals", "clips", "shared_chat", "steam_party", "engine", "category_collection"])
            .bind(window.ended_at)
            .fetch_one(&mut *tx).await?;
        if !ready {
            tx.rollback().await?;
            return Ok(SeasonCloseOutcome::SourceUnavailable {
                season_key: window.key,
            });
        }

        let claimed = sqlx::query(
            "INSERT INTO twitch_partner_effort_season_closures
                 (season_key, season_started_at, season_ended_at, closed_at)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (season_key) DO NOTHING",
        )
        .bind(&window.key)
        .bind(window.started_at)
        .bind(window.ended_at)
        .bind(now)
        .execute(&mut *tx)
        .await?;

        if claimed.rows_affected() == 0 {
            tx.rollback().await?;
            return Ok(SeasonCloseOutcome::AlreadyClosed {
                season_key: window.key,
            });
        }

        let standings = load_effort_standings(&mut tx, &window).await?;

        for (index, standing) in standings.iter().enumerate() {
            sqlx::query(
                "INSERT INTO twitch_partner_effort_season_results
                     (season_key, twitch_user_id, twitch_login, rank, points,
                      qualified_invites, score_reached_at, closed_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
            )
            .bind(&window.key)
            .bind(&standing.twitch_user_id)
            .bind(&standing.twitch_login)
            .bind((index + 1) as i32)
            .bind(standing.points)
            .bind(standing.qualified_invites)
            .bind(standing.score_reached_at)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        }

        let winner = standings
            .first()
            .filter(|standing| standing.points > 0)
            .map(|standing| SeasonWinner {
                twitch_user_id: standing.twitch_user_id.clone(),
                twitch_login: standing.twitch_login.clone(),
                points: standing.points,
                qualified_invites: standing.qualified_invites,
            });

        if let Some(winner) = &winner {
            sqlx::query(
                "INSERT INTO twitch_partner_raid_boost_grants
                     (season_key, twitch_user_id, twitch_login, multiplier,
                      streams_total, streams_remaining, granted_at, expires_at)
                 VALUES ($1, $2, $3, $4, $5, $5, $6, $7)
                 ON CONFLICT (season_key) DO NOTHING",
            )
            .bind(&window.key)
            .bind(&winner.twitch_user_id)
            .bind(&winner.twitch_login)
            .bind(RAID_BOOST_MULTIPLIER)
            .bind(MONTHLY_BOOST_STREAMS)
            .bind(now)
            .bind(now + Duration::days(MONTHLY_BOOST_TTL_DAYS))
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        lock.commit().await?;

        Ok(SeasonCloseOutcome::Closed {
            season_key: window.key,
            partners: standings.len(),
            winner,
        })
    }

    /// Liest ausschließlich den persistierten Boost des aktuellen Streams.
    /// Der Compute-only-/Vergleichspfad darf weder reservieren noch verbrauchen.
    pub async fn reserved_stream_boost_active(
        &self,
        twitch_user_id: &str,
    ) -> Result<bool, sqlx::Error> {
        let ready: Option<String> =
            sqlx::query_scalar("SELECT to_regclass('twitch_partner_raid_boost_streams')::text")
                .fetch_one(&self.pool)
                .await?;
        if ready.is_none() {
            return Ok(false);
        }
        sqlx::query_scalar(
            "SELECT EXISTS (
                SELECT 1 FROM twitch_partner_raid_boost_streams u
                JOIN twitch_stream_sessions s ON s.id::bigint = u.session_id
                    AND s.twitch_user_id = u.twitch_user_id
                JOIN twitch_live_state l ON l.twitch_user_id = u.twitch_user_id
                WHERE u.twitch_user_id = $1
                  AND COALESCE(l.is_live, 0) <> 0
                  AND s.ended_at IS NULL AND u.stream_ended_at IS NULL
                  AND u.session_id = COALESCE(l.active_session_id::bigint, (
                      SELECT id::bigint FROM twitch_stream_sessions
                       WHERE twitch_user_id = $1 AND ended_at IS NULL
                       ORDER BY started_at::text::timestamptz DESC LIMIT 1
                  ))
            )",
        )
        .bind(twitch_user_id)
        .fetch_one(&self.pool)
        .await
    }

    /// Gleicht Reservierungen mit dem aktuellen Streamzustand ab und liefert,
    /// ob der laufende Stream den Monatsboost tatsächlich trägt.
    ///
    /// Die Methode ist idempotent und läuft vor jedem Raid-Score-Refresh.
    /// Dadurch heilen verpasste Online/Offline-Events beim 300-Sekunden-
    /// Reconcile nach.
    pub async fn reconcile_partner(
        &self,
        twitch_user_id: &str,
        _twitch_login: &str,
        now: DateTime<Utc>,
    ) -> Result<SeasonalBoostState, sqlx::Error> {
        let tables_ready: Option<String> =
            sqlx::query_scalar("SELECT to_regclass('twitch_partner_raid_boost_grants')::text")
                .fetch_one(&self.pool)
                .await?;
        if tables_ready.is_none() {
            return Ok(SeasonalBoostState {
                stream_boost_active: false,
                grant_id: None,
                consumed_stream: false,
            });
        }

        let mut tx = self.pool.begin().await?;
        // Alle Finalize-/Reserve-Schritte eines Partners serialisieren. Ohne
        // diesen Lock kann ein schneller Restart parallel den alten Stream
        // verbrauchen und zugleich schon einen neuen Slot reservieren.
        sqlx::query("SELECT pg_advisory_xact_lock(27182, hashtext($1))")
            .bind(twitch_user_id)
            .execute(&mut *tx)
            .await?;
        let consumed_stream = finalize_finished_usages(&mut tx, twitch_user_id, now).await?;

        let live = sqlx::query_as::<_, CurrentLiveState>(
            "SELECT COALESCE(is_live, 0)::int4 AS is_live,
                    active_session_id::bigint AS active_session_id
               FROM twitch_live_state
              WHERE twitch_user_id = $1
              LIMIT 1",
        )
        .bind(twitch_user_id)
        .fetch_optional(&mut *tx)
        .await?;

        let Some(live) = live.filter(|row| row.is_live != 0) else {
            tx.commit().await?;
            return Ok(SeasonalBoostState {
                stream_boost_active: false,
                grant_id: None,
                consumed_stream,
            });
        };

        let session = match live.active_session_id {
            Some(session_id) => load_session_by_id(&mut tx, twitch_user_id, session_id).await?,
            None => load_open_session(&mut tx, twitch_user_id).await?,
        };

        let Some(session) = session else {
            tx.commit().await?;
            return Ok(SeasonalBoostState {
                stream_boost_active: false,
                grant_id: None,
                consumed_stream,
            });
        };

        if session.ended_at.is_some() {
            tx.commit().await?;
            return Ok(SeasonalBoostState {
                stream_boost_active: false,
                grant_id: None,
                consumed_stream,
            });
        }

        if let Some(grant_id) = usage_grant_for_session(&mut tx, twitch_user_id, session.id).await?
        {
            tx.commit().await?;
            return Ok(SeasonalBoostState {
                stream_boost_active: true,
                grant_id: Some(grant_id),
                consumed_stream,
            });
        }

        let grant = sqlx::query_as::<_, GrantRow>(
            "SELECT id, granted_at, expires_at, streams_remaining
               FROM twitch_partner_raid_boost_grants g
              WHERE twitch_user_id = $1
                AND streams_remaining > (
                    SELECT COUNT(*) FROM twitch_partner_raid_boost_streams u
                    WHERE u.grant_id = g.id AND u.stream_ended_at IS NULL
                )
                AND granted_at <= $2
                AND expires_at > $2
              ORDER BY expires_at ASC, granted_at ASC, id ASC
              LIMIT 1
              FOR UPDATE",
        )
        .bind(twitch_user_id)
        .bind(session.started_at)
        .fetch_optional(&mut *tx)
        .await?;

        let Some(grant) =
            grant.filter(|grant| grant_is_available_for_stream(grant, session.started_at))
        else {
            tx.commit().await?;
            return Ok(SeasonalBoostState {
                stream_boost_active: false,
                grant_id: None,
                consumed_stream,
            });
        };

        let inserted = sqlx::query(
            "INSERT INTO twitch_partner_raid_boost_streams
                 (grant_id, twitch_user_id, session_id, stream_started_at, reserved_at)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (twitch_user_id, session_id) DO NOTHING",
        )
        .bind(grant.id)
        .bind(twitch_user_id)
        .bind(session.id)
        .bind(session.started_at)
        .bind(now)
        .execute(&mut *tx)
        .await?;

        let grant_id = if inserted.rows_affected() > 0 {
            Some(grant.id)
        } else {
            usage_grant_for_session(&mut tx, twitch_user_id, session.id).await?
        };

        tx.commit().await?;
        Ok(SeasonalBoostState {
            stream_boost_active: grant_id.is_some(),
            grant_id,
            consumed_stream,
        })
    }
}

async fn load_effort_standings(
    tx: &mut Transaction<'_, Postgres>,
    window: &SeasonWindow,
) -> Result<Vec<EffortStandingRow>, sqlx::Error> {
    sqlx::query_as::<_, EffortStandingRow>(
        r#"
        WITH active AS (
            SELECT twitch_user_id, LOWER(twitch_login) AS twitch_login
              FROM twitch_partners
             WHERE status = 'active'
               AND departnered_at IS NULL
               AND admin_archived_at IS NULL
               AND COALESCE(manual_partner_opt_out, 0) = 0
               AND COALESCE(TRIM(technical_pause_reason), '') = ''
        ),
        event_running AS (
            SELECT e.partner_twitch_user_id,
                   e.event_type,
                   e.credited_at,
                   e.id,
                   SUM(e.points) OVER (
                       PARTITION BY e.partner_twitch_user_id
                       ORDER BY e.credited_at, e.id
                       ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW
                   )::bigint AS running_points,
                   SUM(e.points) OVER (
                       PARTITION BY e.partner_twitch_user_id
                   )::bigint AS final_points
              FROM partner_effort_events e
             WHERE e.credited_at >= $1
               AND e.credited_at < $2
        ),
        monthly AS (
            SELECT partner_twitch_user_id AS twitch_user_id,
                   MAX(final_points)::bigint AS points,
                   COUNT(*) FILTER (WHERE event_type = 'qualified_invite')::bigint
                       AS qualified_invites,
                   MIN(credited_at) FILTER (WHERE running_points = final_points)
                       AS score_reached_at
              FROM event_running
             GROUP BY partner_twitch_user_id
        ),
        scores AS (
            SELECT a.twitch_user_id,
                   a.twitch_login,
                   COALESCE(m.points, 0)::bigint AS points,
                   COALESCE(m.qualified_invites, 0)::bigint AS qualified_invites,
                   CASE
                       WHEN COALESCE(m.points, 0) = 0 THEN $1
                       ELSE m.score_reached_at
                   END AS score_reached_at
              FROM active a
              LEFT JOIN monthly m ON m.twitch_user_id = a.twitch_user_id
        )
        SELECT twitch_user_id,
               twitch_login,
               points,
               qualified_invites,
               score_reached_at
          FROM scores
         ORDER BY points DESC,
                  qualified_invites DESC,
                  score_reached_at ASC NULLS LAST,
                  twitch_user_id ASC
        "#,
    )
    .bind(window.started_at)
    .bind(window.ended_at)
    .fetch_all(&mut **tx)
    .await
}

async fn load_session_by_id(
    tx: &mut Transaction<'_, Postgres>,
    twitch_user_id: &str,
    session_id: i64,
) -> Result<Option<SessionForBoost>, sqlx::Error> {
    sqlx::query_as::<_, SessionForBoost>(
        "SELECT id::bigint AS id,
                started_at::text::timestamptz AS started_at,
                NULLIF(ended_at::text, '')::timestamptz AS ended_at,
                game_name
           FROM twitch_stream_sessions
          WHERE id::bigint = $1 AND twitch_user_id = $2
          LIMIT 1",
    )
    .bind(session_id)
    .bind(twitch_user_id)
    .fetch_optional(&mut **tx)
    .await
}

async fn load_open_session(
    tx: &mut Transaction<'_, Postgres>,
    twitch_user_id: &str,
) -> Result<Option<SessionForBoost>, sqlx::Error> {
    sqlx::query_as::<_, SessionForBoost>(
        "SELECT id::bigint AS id,
                started_at::text::timestamptz AS started_at,
                NULLIF(ended_at::text, '')::timestamptz AS ended_at,
                game_name
           FROM twitch_stream_sessions
          WHERE ended_at IS NULL
            AND twitch_user_id = $1
          ORDER BY started_at::text::timestamptz DESC
          LIMIT 1",
    )
    .bind(twitch_user_id)
    .fetch_optional(&mut **tx)
    .await
}

async fn usage_grant_for_session(
    tx: &mut Transaction<'_, Postgres>,
    twitch_user_id: &str,
    session_id: i64,
) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT grant_id
           FROM twitch_partner_raid_boost_streams
          WHERE twitch_user_id = $1
            AND session_id = $2
          LIMIT 1",
    )
    .bind(twitch_user_id)
    .bind(session_id)
    .fetch_optional(&mut **tx)
    .await
}

async fn finalize_finished_usages(
    tx: &mut Transaction<'_, Postgres>,
    twitch_user_id: &str,
    now: DateTime<Utc>,
) -> Result<bool, sqlx::Error> {
    let usages = sqlx::query_as::<_, OpenUsageRow>(
        "SELECT id, grant_id, session_id
           FROM twitch_partner_raid_boost_streams
          WHERE twitch_user_id = $1
            AND stream_ended_at IS NULL
          ORDER BY id
          FOR UPDATE",
    )
    .bind(twitch_user_id)
    .fetch_all(&mut **tx)
    .await?;

    let mut consumed_any = false;
    for usage in usages {
        let Some(session) = load_session_by_id(tx, twitch_user_id, usage.session_id).await? else {
            continue;
        };
        let Some(ended_at) = session.ended_at else {
            continue;
        };

        let deadlock_seconds =
            deadlock_seconds_for_session(tx, twitch_user_id, &session, ended_at).await?;
        let qualifies = stream_consumes_boost(deadlock_seconds);

        if qualifies {
            let updated = sqlx::query(
                "UPDATE twitch_partner_raid_boost_grants
                    SET streams_remaining = GREATEST(streams_remaining - 1, 0)
                  WHERE id = $1
                    AND streams_remaining > 0",
            )
            .bind(usage.grant_id)
            .execute(&mut **tx)
            .await?;
            consumed_any |= updated.rows_affected() > 0;
        }

        sqlx::query(
            "UPDATE twitch_partner_raid_boost_streams
                SET stream_ended_at = $2,
                    deadlock_seconds = $3,
                    qualified = $4,
                    consumed_at = CASE WHEN $4 THEN $5 ELSE NULL END
              WHERE id = $1
                AND stream_ended_at IS NULL",
        )
        .bind(usage.id)
        .bind(ended_at)
        .bind(deadlock_seconds as i32)
        .bind(qualifies)
        .bind(now)
        .execute(&mut **tx)
        .await?;
    }

    Ok(consumed_any)
}

async fn deadlock_seconds_for_session(
    tx: &mut Transaction<'_, Postgres>,
    twitch_user_id: &str,
    session: &SessionForBoost,
    ended_at: DateTime<Utc>,
) -> Result<i64, sqlx::Error> {
    let updates = sqlx::query_as::<_, GameUpdateRow>(
        "SELECT recorded_at, game_name
           FROM twitch_channel_updates
          WHERE twitch_user_id = $1
            AND recorded_at >= $2
            AND recorded_at < $3
          ORDER BY recorded_at ASC, id ASC",
    )
    .bind(twitch_user_id)
    .bind(session.started_at)
    .bind(ended_at)
    .fetch_all(&mut **tx)
    .await?;

    let timeline = updates
        .into_iter()
        .map(|row| (row.recorded_at, row.game_name.unwrap_or_default()))
        .collect::<Vec<_>>();

    Ok(deadlock_seconds_for_timeline(
        session.started_at,
        ended_at,
        session.game_name.as_deref().unwrap_or(""),
        &timeline,
    ))
}

pub fn previous_season_window(now_utc: DateTime<Utc>) -> SeasonWindow {
    let local = now_utc.with_timezone(&Berlin);
    let (previous_year, previous_month) = if local.month() == 1 {
        (local.year() - 1, 12)
    } else {
        (local.year(), local.month() - 1)
    };

    let previous_start = Berlin
        .with_ymd_and_hms(previous_year, previous_month, 1, 0, 0, 0)
        .single()
        .expect("Monatsanfang Europe/Berlin ist eindeutig");
    let current_start = Berlin
        .with_ymd_and_hms(local.year(), local.month(), 1, 0, 0, 0)
        .single()
        .expect("Monatsanfang Europe/Berlin ist eindeutig");

    SeasonWindow {
        key: format!("{previous_year:04}-{previous_month:02}"),
        started_at: previous_start.with_timezone(&Utc),
        ended_at: current_start.with_timezone(&Utc),
    }
}

/// Nächster regulärer Abschlusszeitpunkt, immer am 1. um 00:05 Berlin.
pub fn next_monthly_close_after(now_utc: DateTime<Utc>) -> DateTime<Utc> {
    let local = now_utc.with_timezone(&Berlin);
    let current_candidate = Berlin
        .with_ymd_and_hms(local.year(), local.month(), 1, 0, 5, 0)
        .single()
        .expect("00:05 Europe/Berlin ist eindeutig");

    if local < current_candidate {
        return current_candidate.with_timezone(&Utc);
    }

    let (year, month) = if local.month() == 12 {
        (local.year() + 1, 1)
    } else {
        (local.year(), local.month() + 1)
    };
    Berlin
        .with_ymd_and_hms(year, month, 1, 0, 5, 0)
        .single()
        .expect("00:05 Europe/Berlin ist eindeutig")
        .with_timezone(&Utc)
}

pub fn stream_consumes_boost(deadlock_seconds: i64) -> bool {
    deadlock_seconds >= QUALIFYING_DEADLOCK_SECONDS
}

pub fn remaining_after_stream(streams_remaining: i16, deadlock_seconds: i64) -> i16 {
    if streams_remaining > 0 && stream_consumes_boost(deadlock_seconds) {
        streams_remaining - 1
    } else {
        streams_remaining
    }
}

pub fn combined_raid_boost_enabled(plan_boost_active: bool, seasonal_stream_active: bool) -> bool {
    plan_boost_active || seasonal_stream_active
}

fn grant_is_available_for_stream(grant: &GrantRow, stream_started_at: DateTime<Utc>) -> bool {
    grant.streams_remaining > 0
        && grant.granted_at <= stream_started_at
        && stream_started_at < grant.expires_at
}

/// Summiert bekannte Deadlock-Segmente innerhalb eines Twitch-Streams.
///
/// initial_game stammt aus twitch_stream_sessions.game_name, spätere
/// Kategorieänderungen aus twitch_channel_updates. Unbekannte Zeit wird nicht
/// als Deadlock geraten.
pub fn deadlock_seconds_for_timeline(
    started_at: DateTime<Utc>,
    ended_at: DateTime<Utc>,
    initial_game: &str,
    updates: &[(DateTime<Utc>, String)],
) -> i64 {
    if ended_at <= started_at {
        return 0;
    }

    let mut total = Duration::zero();
    let mut cursor = started_at;
    let mut current_game = initial_game.trim().to_string();

    for (at, game) in updates {
        if *at <= started_at {
            current_game = game.trim().to_string();
            continue;
        }
        if *at >= ended_at {
            break;
        }
        if *at < cursor {
            continue;
        }
        if is_deadlock(&current_game) {
            total += *at - cursor;
        }
        cursor = *at;
        current_game = game.trim().to_string();
    }

    if is_deadlock(&current_game) {
        total += ended_at - cursor;
    }
    total.num_seconds()
}

fn is_deadlock(game: &str) -> bool {
    game.trim().eq_ignore_ascii_case("deadlock")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scoring::compute_raid_boost_multiplier;

    fn utc(y: i32, m: u32, d: u32, h: u32, min: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, h, min, 0).single().unwrap()
    }

    #[test]
    fn zwei_qualifizierende_streams_verbrauchen_den_grant() {
        let remaining = remaining_after_stream(MONTHLY_BOOST_STREAMS, 30 * 60);
        assert_eq!(remaining, 1);
        let remaining = remaining_after_stream(remaining, 75 * 60);
        assert_eq!(remaining, 0);
    }

    #[test]
    fn stream_unter_30_minuten_verbraucht_nichts() {
        assert_eq!(
            remaining_after_stream(MONTHLY_BOOST_STREAMS, 29 * 60 + 59),
            MONTHLY_BOOST_STREAMS
        );
    }

    #[test]
    fn grant_ist_nach_30_tagen_fuer_neue_streams_abgelaufen() {
        let granted_at = utc(2026, 9, 1, 22, 5);
        let grant = GrantRow {
            id: 1,
            granted_at,
            expires_at: granted_at + Duration::days(30),
            streams_remaining: 2,
        };
        assert!(grant_is_available_for_stream(
            &grant,
            grant.expires_at - Duration::seconds(1)
        ));
        assert!(!grant_is_available_for_stream(&grant, grant.expires_at));
    }

    #[test]
    fn plan_und_monatsboost_stapeln_nicht() {
        let active = combined_raid_boost_enabled(true, true);
        assert_eq!(compute_raid_boost_multiplier(active), RAID_BOOST_MULTIPLIER);
    }

    #[test]
    fn deadlock_zeit_wird_ueber_kategorie_wechsel_summiert() {
        let start = utc(2026, 9, 10, 18, 0);
        let end = utc(2026, 9, 10, 19, 0);
        let updates = vec![
            (utc(2026, 9, 10, 18, 20), "Just Chatting".to_string()),
            (utc(2026, 9, 10, 18, 35), "Deadlock".to_string()),
        ];
        assert_eq!(
            deadlock_seconds_for_timeline(start, end, "Deadlock", &updates),
            45 * 60
        );
    }

    #[test]
    fn teilsekunden_werden_erst_nach_dem_summieren_abgerundet() {
        let start = utc(2026, 9, 10, 18, 0);
        let updates = vec![
            (
                start + Duration::milliseconds(900_500),
                "Just Chatting".to_string(),
            ),
            (
                start + Duration::milliseconds(901_000),
                "Deadlock".to_string(),
            ),
        ];
        let end = start + Duration::milliseconds(1_800_500);
        assert_eq!(
            deadlock_seconds_for_timeline(start, end, "Deadlock", &updates),
            1800
        );
    }

    #[test]
    fn unbekannte_kategorie_zaehlt_nicht_als_deadlock() {
        let start = utc(2026, 9, 10, 18, 0);
        let end = start + Duration::hours(2);
        assert_eq!(deadlock_seconds_for_timeline(start, end, "", &[]), 0);
    }

    #[test]
    fn berlin_monatsfenster_beruecksichtigt_dst() {
        let window = previous_season_window(utc(2026, 10, 1, 22, 5));
        assert_eq!(window.key, "2026-09");
        assert_eq!(window.started_at, utc(2026, 8, 31, 22, 0));
        assert_eq!(window.ended_at, utc(2026, 9, 30, 22, 0));
    }

    #[test]
    fn naechster_abschluss_ist_0005_berlin() {
        let now = utc(2026, 9, 26, 20, 0);
        assert_eq!(next_monthly_close_after(now), utc(2026, 9, 30, 22, 5));
    }
}
