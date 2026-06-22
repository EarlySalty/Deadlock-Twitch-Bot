//! DB-Zugriff auf `twitch_stream_sessions` / `twitch_session_viewers` /
//! `twitch_session_chatters` (nur Count-Read). Prod-Typen (Baseline-Schema):
//! `id` bigint, `started_at`/`ended_at` **TEXT** (ISO, SQLite-Erbe — Bind als
//! ISO-String via `iso_seconds`, Decode via `::text`-Cast + `parse_dt_utc`,
//! P2.38), `is_mature`/`had_deadlock_in_session` INTEGER (0/1, Bind/Decode als
//! `i32`, nicht bool), `avg_viewers` double precision. SQL-Vergleiche gegen
//! `NOW()`/Intervalle casten `started_at::timestamptz`, was sowohl für TEXT
//! (ISO) als auch für eine TIMESTAMPTZ-Spalte gültig ist.
//!
//! Bewusste Fixes gegenüber Python (Plan-Doc Schritt 4):
//! - `start_session` hält einen Advisory-Lock pro Login und prüft offene
//!   Sessions im selben Tx — der latente Doppel-Insert (Python sichert nur
//!   über einen In-Memory-Cache) ist damit DB-seitig ausgeschlossen.
//! - Chatter-Counts via `COUNT(*) FILTER (WHERE …)` — Pythons
//!   `SUM(boolean)` wirft seit der Boolean-Migration still einen DB-Fehler,
//!   wodurch Prod-Sessions immer 0 Chatter geschrieben haben.

use chrono::{DateTime, Utc};
use sqlx::PgPool;

use super::metrics::ViewerSample;
use crate::stream::{iso_seconds, parse_dt_utc};

/// Neue Session (Python `_start_stream_session`-Parameter).
#[derive(Debug, Clone)]
pub struct NewSession {
    pub streamer_login: String,
    pub stream_id: Option<String>,
    pub started_at: DateTime<Utc>,
    pub viewer_count: i32,
    pub followers_start: Option<i32>,
    pub title: String,
    pub language: String,
    pub is_mature: bool,
    pub tags: String,
    pub game_name: Option<String>,
    pub had_deadlock: bool,
}

/// Ergebnis von [`SessionStore::start_session`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartOutcome {
    Created(i64),
    /// Es gab bereits eine offene Session (Race mit anderem Prozess/Worker).
    AlreadyOpen(i64),
}

impl StartOutcome {
    pub fn session_id(self) -> i64 {
        match self {
            StartOutcome::Created(id) | StartOutcome::AlreadyOpen(id) => id,
        }
    }
}

/// Session-Felder, die das Finalize als Ausgangsbasis braucht.
#[derive(Debug, Clone)]
pub struct FinalizeSource {
    pub started_at: DateTime<Utc>,
    pub start_viewers: Option<i32>,
    pub end_viewers: Option<i32>,
    pub peak_viewers: Option<i32>,
    pub avg_viewers: Option<f64>,
    pub samples: Option<i32>,
    pub followers_start: Option<i32>,
}

/// Vollständiges Finalize-Update (Python-`UPDATE` 1:1).
#[derive(Debug, Clone)]
pub struct FinalizeUpdate {
    pub session_id: i64,
    pub streamer_login: String,
    pub ended_at: DateTime<Utc>,
    pub duration_seconds: i32,
    pub end_viewers: i32,
    pub peak_viewers: i32,
    pub avg_viewers: f64,
    pub samples: i32,
    pub retention_5m: Option<f64>,
    pub retention_10m: Option<f64>,
    pub retention_20m: Option<f64>,
    pub dropoff_pct: Option<f64>,
    pub dropoff_label: String,
    pub unique_chatters: i32,
    pub first_time_chatters: i32,
    pub returning_chatters: i32,
    pub followers_end: Option<i32>,
    pub follower_delta: Option<i32>,
    pub notes: String,
    pub had_deadlock_in_session: bool,
    /// `game_name = COALESCE(game_name, $fallback)`.
    pub fallback_game_name: Option<String>,
}

/// Kandidat für den Orphan-Cleanup.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct OrphanCandidate {
    pub id: i64,
    pub streamer_login: String,
    pub finalized_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct OpenSession {
    pub id: i64,
    pub streamer_login: String,
}

#[derive(Clone)]
pub struct SessionStore {
    pool: PgPool,
}

impl SessionStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Alle offenen Sessions (Cache-Rehydrierung beim Start).
    pub async fn list_open(&self) -> Result<Vec<OpenSession>, sqlx::Error> {
        sqlx::query_as(
            "SELECT id, streamer_login FROM twitch_stream_sessions WHERE ended_at IS NULL",
        )
        .fetch_all(&self.pool)
        .await
    }

    /// Jüngste offene Session eines Logins.
    pub async fn find_open_id(&self, login: &str) -> Result<Option<i64>, sqlx::Error> {
        sqlx::query_scalar(
            "SELECT id FROM twitch_stream_sessions
              WHERE streamer_login = $1 AND ended_at IS NULL
              ORDER BY started_at DESC LIMIT 1",
        )
        .bind(login)
        .fetch_optional(&self.pool)
        .await
    }

    pub async fn stream_id_of(&self, session_id: i64) -> Result<Option<String>, sqlx::Error> {
        let row: Option<Option<String>> =
            sqlx::query_scalar("SELECT stream_id FROM twitch_stream_sessions WHERE id = $1")
                .bind(session_id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.flatten())
    }

    /// Legt eine Session an — race-sicher über einen Advisory-Lock pro Login:
    /// existiert im selben Moment schon eine offene Session, wird deren ID
    /// zurückgegeben statt eine zweite anzulegen.
    pub async fn start_session(&self, new: &NewSession) -> Result<StartOutcome, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "SELECT pg_advisory_xact_lock(hashtextextended('twitch_stream_session:' || $1, 0))",
        )
        .bind(&new.streamer_login)
        .execute(&mut *tx)
        .await?;
        let existing: Option<i64> = sqlx::query_scalar(
            "SELECT id FROM twitch_stream_sessions
              WHERE streamer_login = $1 AND ended_at IS NULL
              ORDER BY started_at DESC LIMIT 1",
        )
        .bind(&new.streamer_login)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(id) = existing {
            tx.commit().await?;
            return Ok(StartOutcome::AlreadyOpen(id));
        }
        let session_id: i64 = sqlx::query_scalar(
            r#"
            INSERT INTO twitch_stream_sessions (
                streamer_login, stream_id, started_at, start_viewers, peak_viewers,
                end_viewers, avg_viewers, samples, followers_start, stream_title,
                language, is_mature, tags, game_name, had_deadlock_in_session
            ) VALUES ($1, $2, $3, $4, $4, $4, $5, 0, $6, $7, $8, $9, $10, $11, $12)
            RETURNING id
            "#,
        )
        .bind(&new.streamer_login)
        .bind(&new.stream_id)
        // P2.38: started_at als ISO-TEXT binden (Prod-Spalte ist TEXT).
        .bind(iso_seconds(new.started_at))
        .bind(new.viewer_count)
        .bind(f64::from(new.viewer_count))
        .bind(new.followers_start)
        .bind(&new.title)
        .bind(&new.language)
        .bind(i32::from(new.is_mature))
        .bind(&new.tags)
        .bind(&new.game_name)
        .bind(i32::from(new.had_deadlock))
        .fetch_one(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE twitch_live_state SET active_session_id = $1 WHERE streamer_login = $2",
        )
        .bind(session_id)
        .bind(&new.streamer_login)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(StartOutcome::Created(session_id))
    }

    /// Viewer-Snapshot + Aggregat-Update in einer Transaktion
    /// (Python `_record_session_sample`). `false` = Session unbekannt.
    pub async fn record_sample(
        &self,
        session_id: i64,
        viewer_count: i32,
        now: DateTime<Utc>,
    ) -> Result<bool, sqlx::Error> {
        #[derive(sqlx::FromRow)]
        struct SampleSource {
            // P2.38: started_at als TEXT lesen (::text-Cast) und ISO-parsen —
            // die Prod-Spalte ist TEXT (Baseline-Schema), ein Decode nach
            // DateTime<Utc> würde dort werfen. Der Cast ist auch für eine
            // TIMESTAMPTZ-Spalte gültig, daher typ-unabhängig.
            started_at: Option<String>,
            samples: Option<i32>,
            avg_viewers: Option<f64>,
            start_viewers: Option<i32>,
            peak_viewers: Option<i32>,
        }
        let mut tx = self.pool.begin().await?;
        let row: Option<SampleSource> = sqlx::query_as(
            "SELECT started_at::text AS started_at, samples, avg_viewers, start_viewers, peak_viewers
               FROM twitch_stream_sessions WHERE id = $1",
        )
        .bind(session_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(SampleSource {
            started_at: started_at_raw,
            samples,
            avg_viewers: avg_prev,
            start_viewers,
            peak_viewers,
        }) = row
        else {
            return Ok(false);
        };
        // Unparsebarer/fehlender Start → minutes_from_start = 0 (defensiv;
        // Python würde hier ebenfalls 0 setzen statt zu crashen).
        let started_at = started_at_raw
            .as_deref()
            .and_then(parse_dt_utc)
            .unwrap_or(now);
        let minutes_from_start = ((now - started_at).num_seconds().max(0) / 60) as i32;
        sqlx::query(
            r#"
            INSERT INTO twitch_session_viewers (session_id, ts_utc, minutes_from_start, viewer_count)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (session_id, ts_utc) DO UPDATE SET
                minutes_from_start = EXCLUDED.minutes_from_start,
                viewer_count = EXCLUDED.viewer_count
            "#,
        )
        .bind(session_id)
        .bind(now)
        .bind(minutes_from_start)
        .bind(viewer_count)
        .execute(&mut *tx)
        .await?;

        let samples = samples.unwrap_or(0);
        let avg_prev = avg_prev.unwrap_or(0.0);
        let new_samples = samples + 1;
        let new_avg = ((avg_prev * f64::from(samples)) + f64::from(viewer_count))
            / f64::from(new_samples.max(1));
        let start_viewers = match start_viewers.unwrap_or(0) {
            0 => viewer_count,
            v => v,
        };
        let peak_viewers = peak_viewers.unwrap_or(0).max(viewer_count);
        sqlx::query(
            "UPDATE twitch_stream_sessions
                SET samples = $1, avg_viewers = $2, peak_viewers = $3,
                    end_viewers = $4, start_viewers = $5
              WHERE id = $6",
        )
        .bind(new_samples)
        .bind(new_avg)
        .bind(peak_viewers)
        .bind(viewer_count)
        .bind(start_viewers)
        .bind(session_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(true)
    }

    /// Backfill für vom Scout unvollständig angelegte Sessions
    /// (Python `_adopt_incomplete_session`) — atomar über die WHERE-Klausel.
    pub async fn adopt_incomplete(
        &self,
        session_id: i64,
        viewer_count: i32,
        game_name: Option<&str>,
        had_deadlock: bool,
        stream_title: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            UPDATE twitch_stream_sessions
               SET start_viewers = $1,
                   peak_viewers = GREATEST(peak_viewers, $1),
                   had_deadlock_in_session = GREATEST(COALESCE(had_deadlock_in_session, 0), $2),
                   game_name = COALESCE(game_name, $3),
                   stream_title = COALESCE(stream_title, $4)
             WHERE id = $5 AND samples = 0 AND start_viewers = 0
            "#,
        )
        .bind(viewer_count)
        .bind(i32::from(had_deadlock))
        .bind(game_name)
        .bind(stream_title)
        .bind(session_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Speichert den gesendeten Announcement-Text an der Session
    /// (Python: UPDATE `notification_text` nach erfolgreichem Posting).
    pub async fn set_notification_text(
        &self,
        session_id: i64,
        text: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE twitch_stream_sessions SET notification_text = $1 WHERE id = $2")
            .bind(text)
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn finalize_source(
        &self,
        session_id: i64,
    ) -> Result<Option<FinalizeSource>, sqlx::Error> {
        // P2.38: started_at als TEXT (::text-Cast) lesen und ISO-parsen, statt
        // direkt nach DateTime<Utc> zu decodieren (Prod-Spalte ist TEXT).
        #[derive(sqlx::FromRow)]
        struct Raw {
            started_at: Option<String>,
            start_viewers: Option<i32>,
            end_viewers: Option<i32>,
            peak_viewers: Option<i32>,
            avg_viewers: Option<f64>,
            samples: Option<i32>,
            followers_start: Option<i32>,
        }
        let raw: Option<Raw> = sqlx::query_as(
            "SELECT started_at::text AS started_at, start_viewers, end_viewers, peak_viewers,
                    avg_viewers, samples, followers_start
               FROM twitch_stream_sessions WHERE id = $1",
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(raw.map(|r| FinalizeSource {
            // Unparsebar/fehlend → Unix-Epoch (Finalize berechnet daraus eine
            // große, geklammerte Dauer; verhindert Crash bei Alt-/Defektdaten).
            started_at: r
                .started_at
                .as_deref()
                .and_then(parse_dt_utc)
                .unwrap_or_else(|| DateTime::<Utc>::from_timestamp(0, 0).unwrap_or_default()),
            start_viewers: r.start_viewers,
            end_viewers: r.end_viewers,
            peak_viewers: r.peak_viewers,
            avg_viewers: r.avg_viewers,
            samples: r.samples,
            followers_start: r.followers_start,
        }))
    }

    /// Viewer-Samples chronologisch (Python sortiert nach `ts_utc`).
    pub async fn viewer_samples(&self, session_id: i64) -> Result<Vec<ViewerSample>, sqlx::Error> {
        let rows: Vec<(Option<i32>, i32)> = sqlx::query_as(
            "SELECT minutes_from_start, viewer_count
               FROM twitch_session_viewers WHERE session_id = $1 ORDER BY ts_utc",
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(minutes, viewers)| ViewerSample {
                minutes_from_start: minutes.unwrap_or(0),
                viewer_count: viewers,
            })
            .collect())
    }

    /// Chatter-Zählung fürs Finalize — mit `FILTER` statt Pythons kaputtem
    /// `SUM(boolean)`. Liefert (unique, first_time).
    pub async fn chatter_counts(&self, session_id: i64) -> Result<(i64, i64), sqlx::Error> {
        let row: (i64, i64) = sqlx::query_as(
            "SELECT COUNT(*),
                    COUNT(*) FILTER (WHERE is_first_time_streamer)
               FROM twitch_session_chatters WHERE session_id = $1",
        )
        .bind(session_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    /// Session abschließen + Live-State-Verknüpfung lösen (eine Transaktion).
    /// `ended_at IS NULL`-Guard härtet gegen Doppel-Finalize (Race
    /// Poll-Offline vs. EventSub-Offline) — Python fehlt der Guard, ein
    /// zweiter Abschluss würde dort die fertigen Kennzahlen überschreiben.
    /// `false` = Session war bereits abgeschlossen.
    pub async fn apply_finalize(&self, update: &FinalizeUpdate) -> Result<bool, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let updated = sqlx::query(
            r#"
            UPDATE twitch_stream_sessions
               SET ended_at = $1,
                   duration_seconds = $2,
                   end_viewers = $3,
                   peak_viewers = $4,
                   avg_viewers = $5,
                   samples = $6,
                   retention_5m = $7,
                   retention_10m = $8,
                   retention_20m = $9,
                   dropoff_pct = $10,
                   dropoff_label = $11,
                   unique_chatters = $12,
                   first_time_chatters = $13,
                   returning_chatters = $14,
                   followers_end = $15,
                   follower_delta = $16,
                   notes = $17,
                   had_deadlock_in_session = $18,
                   game_name = COALESCE(game_name, $19)
             WHERE id = $20 AND ended_at IS NULL
            "#,
        )
        // P2.38: ended_at als ISO-TEXT binden (Prod-Spalte ist TEXT).
        .bind(iso_seconds(update.ended_at))
        .bind(update.duration_seconds)
        .bind(update.end_viewers)
        .bind(update.peak_viewers)
        .bind(update.avg_viewers)
        .bind(update.samples)
        .bind(update.retention_5m)
        .bind(update.retention_10m)
        .bind(update.retention_20m)
        .bind(update.dropoff_pct)
        .bind(&update.dropoff_label)
        .bind(update.unique_chatters)
        .bind(update.first_time_chatters)
        .bind(update.returning_chatters)
        .bind(update.followers_end)
        .bind(update.follower_delta)
        .bind(&update.notes)
        .bind(i32::from(update.had_deadlock_in_session))
        .bind(&update.fallback_game_name)
        .bind(update.session_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE twitch_live_state SET active_session_id = NULL WHERE streamer_login = $1",
        )
        .bind(&update.streamer_login)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(updated.rows_affected() > 0)
    }

    /// Verwaiste offene Sessions: (1) Scout-Sessions ohne Samples > 24 h,
    /// (2) Sessions mit Samples, deren letzter Viewer-Eintrag > 1 h alt ist.
    pub async fn orphan_candidates(
        &self,
    ) -> Result<(Vec<OrphanCandidate>, Vec<OrphanCandidate>), sqlx::Error> {
        let zero_sample: Vec<OrphanCandidate> = sqlx::query_as(
            r#"
            SELECT id, streamer_login,
                   COALESCE(started_at::timestamptz, NOW()) AS finalized_at
            FROM twitch_stream_sessions
            WHERE ended_at IS NULL
              AND samples = 0
              AND started_at::timestamptz < NOW() - INTERVAL '24 hours'
            "#,
        )
        .fetch_all(&self.pool)
        .await?;
        let stale: Vec<OrphanCandidate> = sqlx::query_as(
            r#"
            SELECT ss.id, ss.streamer_login, MAX(sv.ts_utc) AS finalized_at
            FROM twitch_session_viewers sv
            JOIN twitch_stream_sessions ss ON ss.id = sv.session_id
            WHERE ss.ended_at IS NULL
              AND ss.samples > 0
            GROUP BY ss.id, ss.streamer_login
            HAVING MAX(sv.ts_utc) < NOW() - INTERVAL '1 hour'
            "#,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok((zero_sample, stale))
    }
}
