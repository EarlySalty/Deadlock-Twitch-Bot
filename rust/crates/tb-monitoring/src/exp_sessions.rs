//! Dünner Port der `exp_*`-Hooks (Experimental-Analytics-Schreibpfad).
//!
//! Fork-Entscheid (Plan-Doc Schritt 4): nur die 4 Write-Hooks, keine
//! Erweiterung — die Tabellen haben echte Konsumenten (AI-Reports,
//! `/exp/game-transitions`), das Doppelsystem wird nach dem Cutover
//! konsolidiert (`05-cleanup-decisions.md` #12). Alle Hooks sind
//! best-effort: Fehler werden debug-geloggt, nie propagiert (wie Python).
//!
//! Prod-Typen: IDs bigint, Timestamps TEXT (ISO), `avg_viewers`/
//! `minutes_from_start`/`duration_min` REAL (f32).

use std::collections::HashMap;
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use sqlx::PgPool;

use crate::stream::{iso_seconds, parse_dt_utc, StreamSnapshot};

#[derive(Clone)]
pub struct ExpSessionStore {
    pool: PgPool,
}

impl ExpSessionStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    async fn find_open_by_stream_id(&self, stream_id: &str) -> Result<Option<i64>, sqlx::Error> {
        sqlx::query_scalar(
            "SELECT id FROM exp_sessions WHERE stream_id = $1 AND ended_at IS NULL LIMIT 1",
        )
        .bind(stream_id)
        .fetch_optional(&self.pool)
        .await
    }

    /// Insert mit Idempotenz über den partiellen Unique-Index auf `stream_id`:
    /// `ON CONFLICT … DO NOTHING` + Nachschlagen ersetzt Pythons
    /// UniqueViolation-Fallback.
    async fn insert_session(
        &self,
        login: &str,
        stream_id: Option<&str>,
        started_at_text: &str,
        game_name: Option<&str>,
        stream_title: Option<&str>,
        viewer_count: i32,
    ) -> Result<Option<i64>, sqlx::Error> {
        let inserted: Option<i64> = sqlx::query_scalar(
            r#"
            INSERT INTO exp_sessions (
                streamer, stream_id, started_at, game_name, stream_title,
                peak_viewers, avg_viewers, samples
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, 0)
            ON CONFLICT (stream_id) WHERE stream_id IS NOT NULL DO NOTHING
            RETURNING id
            "#,
        )
        .bind(login)
        .bind(stream_id)
        .bind(started_at_text)
        .bind(game_name)
        .bind(stream_title)
        .bind(viewer_count)
        .bind(viewer_count as f32)
        .fetch_optional(&self.pool)
        .await?;
        if inserted.is_some() {
            return Ok(inserted);
        }
        match stream_id {
            Some(stream_id) => self.find_open_by_stream_id(stream_id).await,
            None => Ok(None),
        }
    }

    async fn record_snapshot(
        &self,
        exp_session_id: i64,
        now: DateTime<Utc>,
        viewer_count: i32,
    ) -> Result<(), sqlx::Error> {
        #[derive(sqlx::FromRow)]
        struct AggregateRow {
            started_at: String,
            samples: Option<i32>,
            avg_viewers: Option<f32>,
            peak_viewers: Option<i32>,
        }
        let mut tx = self.pool.begin().await?;
        let row: Option<AggregateRow> = sqlx::query_as(
            "SELECT started_at, samples, avg_viewers, peak_viewers
               FROM exp_sessions WHERE id = $1",
        )
        .bind(exp_session_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(AggregateRow {
            started_at: started_at_raw,
            samples,
            avg_viewers: avg_prev,
            peak_viewers: peak_prev,
        }) = row
        else {
            return Ok(());
        };
        let start = parse_dt_utc(&started_at_raw).unwrap_or(now);
        let minutes_from_start =
            (((now - start).num_seconds().max(0) as f64 / 60.0) * 100.0).round() as f32 / 100.0;

        let inserted: Option<i64> = sqlx::query_scalar(
            r#"
            INSERT INTO exp_snapshots (exp_session_id, ts_utc, viewer_count, minutes_from_start)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (exp_session_id, ts_utc) DO NOTHING
            RETURNING id
            "#,
        )
        .bind(exp_session_id)
        .bind(iso_seconds(now))
        .bind(viewer_count)
        .bind(minutes_from_start)
        .fetch_optional(&mut *tx)
        .await?;
        if inserted.is_none() {
            // Gleiche Sekunde schon erfasst → Aggregate unverändert lassen.
            tx.commit().await?;
            return Ok(());
        }

        let samples = samples.unwrap_or(0);
        let new_samples = samples + 1;
        let new_avg = ((f64::from(avg_prev.unwrap_or(0.0)) * f64::from(samples))
            + f64::from(viewer_count))
            / f64::from(new_samples.max(1));
        let new_peak = peak_prev.unwrap_or(0).max(viewer_count);
        sqlx::query(
            "UPDATE exp_sessions SET samples = $1, avg_viewers = $2, peak_viewers = $3 WHERE id = $4",
        )
        .bind(new_samples)
        .bind(new_avg as f32)
        .bind(new_peak)
        .bind(exp_session_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn record_transition(
        &self,
        exp_session_id: i64,
        login: &str,
        from_game: Option<&str>,
        to_game: Option<&str>,
        viewer_count: i32,
        now: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO exp_game_transitions
                (exp_session_id, streamer, ts_utc, from_game, to_game, viewer_count)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(exp_session_id)
        .bind(login)
        .bind(iso_seconds(now))
        .bind(from_game)
        .bind(to_game)
        .bind(viewer_count)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn finalize(
        &self,
        exp_session_id: i64,
        now: DateTime<Utc>,
        follower_delta: Option<i32>,
    ) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let started_at_raw: Option<String> =
            sqlx::query_scalar("SELECT started_at FROM exp_sessions WHERE id = $1")
                .bind(exp_session_id)
                .fetch_optional(&mut *tx)
                .await?;
        let Some(started_at_raw) = started_at_raw else {
            return Ok(());
        };
        let duration_min: Option<f32> = parse_dt_utc(&started_at_raw)
            .map(|start| ((now - start).num_seconds().max(0) as f64 / 60.0) as f32);
        sqlx::query(
            "UPDATE exp_sessions SET ended_at = $1, follower_delta = $2, duration_min = $3 WHERE id = $4",
        )
        .bind(iso_seconds(now))
        .bind(follower_delta)
        .bind(duration_min)
        .bind(exp_session_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }
}

/// Hook-Fassade mit In-Memory-Cache `login → exp_session_id` (wie Python).
pub struct ExpSessionTracker {
    store: ExpSessionStore,
    cache: Mutex<HashMap<String, i64>>,
}

impl ExpSessionTracker {
    pub fn new(store: ExpSessionStore) -> Self {
        Self {
            store,
            cache: Mutex::new(HashMap::new()),
        }
    }

    fn cached_id(&self, login: &str) -> Option<i64> {
        self.cache
            .lock()
            .expect("exp cache lock")
            .get(login)
            .copied()
    }

    /// Hook: Session gestartet. Legt (idempotent über `stream_id`) einen
    /// `exp_sessions`-Eintrag an und merkt sich die ID.
    pub async fn on_session_start(
        &self,
        login: &str,
        stream: &StreamSnapshot,
        started_at: DateTime<Utc>,
    ) {
        let login = login.to_lowercase();
        let stream_id = stream
            .id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        // Idempotenz-Vorprüfung wie Python: offene Session zur stream_id?
        if let Some(stream_id) = stream_id {
            match self.store.find_open_by_stream_id(stream_id).await {
                Ok(Some(id)) => {
                    self.cache.lock().expect("exp cache lock").insert(login, id);
                    return;
                }
                Ok(None) => {}
                Err(error) => {
                    tracing::debug!(%error, login, "exp: Konnte offene Session nicht prüfen");
                }
            }
        }
        match self
            .store
            .insert_session(
                &login,
                stream_id,
                &iso_seconds(started_at),
                stream.game_name_opt().as_deref(),
                stream.title_opt().as_deref(),
                stream.viewer_count,
            )
            .await
        {
            Ok(Some(id)) => {
                self.cache.lock().expect("exp cache lock").insert(login, id);
            }
            Ok(None) => {}
            Err(error) => {
                tracing::debug!(%error, login, "exp: Konnte exp_session nicht anlegen");
            }
        }
    }

    /// Hook: Viewer-Sample.
    pub async fn on_session_sample(
        &self,
        login: &str,
        stream: &StreamSnapshot,
        now: DateTime<Utc>,
    ) {
        let login = login.to_lowercase();
        let Some(exp_id) = self.cached_id(&login) else {
            return;
        };
        if let Err(error) = self
            .store
            .record_snapshot(exp_id, now, stream.viewer_count)
            .await
        {
            tracing::debug!(%error, login, "exp: Konnte Sample nicht schreiben");
        }
    }

    /// Hook: Spielwechsel.
    pub async fn on_game_transition(
        &self,
        login: &str,
        from_game: &str,
        to_game: &str,
        viewer_count: i32,
        now: DateTime<Utc>,
    ) {
        let login = login.to_lowercase();
        let Some(exp_id) = self.cached_id(&login) else {
            return;
        };
        let from_game = Some(from_game.trim()).filter(|g| !g.is_empty());
        let to_game = Some(to_game.trim()).filter(|g| !g.is_empty());
        if let Err(error) = self
            .store
            .record_transition(exp_id, &login, from_game, to_game, viewer_count, now)
            .await
        {
            tracing::debug!(%error, login, "exp: Konnte game_transition nicht schreiben");
        }
    }

    /// Hook: Session abgeschlossen. Räumt den Cache-Eintrag immer ab.
    pub async fn on_session_finalize(
        &self,
        login: &str,
        follower_delta: Option<i32>,
        now: DateTime<Utc>,
    ) {
        let login = login.to_lowercase();
        if let Some(exp_id) = self.cached_id(&login) {
            if let Err(error) = self.store.finalize(exp_id, now, follower_delta).await {
                tracing::debug!(%error, login, "exp: Konnte Session nicht finalisieren");
            }
        }
        self.cache.lock().expect("exp cache lock").remove(&login);
    }
}
