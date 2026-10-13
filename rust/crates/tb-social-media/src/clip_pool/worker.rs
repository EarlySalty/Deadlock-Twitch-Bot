use std::time::Duration;

use chrono::Utc;
use sqlx::{PgPool, Row};

use super::{eligible, imports, repository, ImportWindow, PoolError};
use crate::autopilot_settings;
use crate::clip::helix::{HelixClipSource, HistoricalClipPage, HistoricalFetchError};

pub struct ClipImportWorker {
    pool: PgPool,
    source: HelixClipSource,
}

struct Claim {
    job: imports::ImportJob,
    login: String,
    generation: i64,
    cursor: Option<String>,
    window: ImportWindow,
}

impl ClipImportWorker {
    pub fn new(pool: PgPool, source: HelixClipSource) -> Self {
        Self { pool, source }
    }

    async fn claim(&self) -> Result<Option<Claim>, PoolError> {
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query("SELECT * FROM social_clip_import_jobs WHERE status IN ('queued', 'running', 'retry')
            AND (next_retry_at IS NULL OR next_retry_at <= clock_timestamp())
            AND (lease_until IS NULL OR lease_until <= clock_timestamp()) ORDER BY id FOR UPDATE SKIP LOCKED LIMIT 1")
            .fetch_optional(&mut *tx).await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let job = imports::from_row(&row)?;
        let mut pending: Vec<ImportWindow> =
            serde_json::from_value(row.try_get("pending_windows")?)?;
        let window = match job.active_window.clone() {
            Some(window) => window,
            None if !pending.is_empty() => pending.remove(0),
            None => return Err(PoolError::InvalidSource),
        };
        let generation: i64 = sqlx::query_scalar("UPDATE social_clip_import_jobs SET status = 'running',
            active_window = $2, pending_windows = $3, lease_generation = lease_generation + 1,
            lease_until = clock_timestamp() + INTERVAL '2 minutes', attempts = attempts + 1, total_attempts = total_attempts + 1
            WHERE id = $1 RETURNING lease_generation")
            .bind(job.id).bind(serde_json::to_value(&window)?).bind(serde_json::to_value(&pending)?)
            .fetch_one(&mut *tx).await?;
        let claim = Claim {
            job,
            login: row.try_get("streamer_login")?,
            generation,
            cursor: row.try_get("cursor")?,
            window,
        };
        tx.commit().await?;
        Ok(Some(claim))
    }

    async fn commit_page(&self, claim: &Claim, page: HistoricalClipPage) -> Result<(), PoolError> {
        let mut tx = self.pool.begin().await?;
        autopilot_settings::lock_owner(&mut tx, &claim.job.twitch_user_id).await?;
        let row = sqlx::query(
            "SELECT * FROM social_clip_import_jobs WHERE id = $1 AND lease_generation = $2
            AND status = 'running' AND lease_until > clock_timestamp() FOR UPDATE",
        )
        .bind(claim.job.id)
        .bind(claim.generation)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(PoolError::LeaseLost)?;
        let frozen = ImportWindow::new(claim.job.started_at, claim.job.ended_at);
        let mut eligible_count = 0i64;
        let mut persisted = 0i64;
        for mut clip in page.clips {
            if !eligible(&claim.job.twitch_user_id, &clip, &frozen)? {
                continue;
            }
            clip.streamer_login.clone_from(&claim.login);
            if let Some((_, inserted)) = repository::register_on(&mut tx, &clip, &frozen).await? {
                eligible_count += 1;
                persisted += i64::from(inserted);
            }
        }
        let mut pending: Vec<ImportWindow> =
            serde_json::from_value(row.try_get("pending_windows")?)?;
        let mut incomplete: Vec<ImportWindow> =
            serde_json::from_value(row.try_get("incomplete_windows")?)?;
        let mut window = claim.window.clone();
        window.fetched += i64::try_from(page.raw_count).map_err(|_| PoolError::InvalidSource)?;
        window.pages += 1;
        let saturated = window.fetched >= 900 || window.pages >= 10;
        let mut completed_windows = 0;
        let (active, cursor) = if saturated {
            match window.split() {
                Some(children) => {
                    pending.splice(0..0, children);
                }
                None => incomplete.push(window),
            }
            (None, None)
        } else if page.next_cursor.is_none() {
            completed_windows = 1;
            (None, None)
        } else {
            (Some(window), page.next_cursor)
        };
        let finished = active.is_none() && pending.is_empty();
        let status = if finished {
            if incomplete.is_empty() {
                "completed"
            } else {
                "incomplete"
            }
        } else {
            "queued"
        };
        sqlx::query("UPDATE social_clip_import_jobs SET pending_windows = $3, active_window = $4,
            incomplete_windows = $5, cursor = $6, raw_fetched = raw_fetched + $7,
            eligible_fetched = eligible_fetched + $8, persisted_count = persisted_count + $9,
            completed_windows = completed_windows + $10, status = $11, lease_until = NULL,
            attempts = 0, next_retry_at = NULL, last_error = CASE WHEN $11 = 'incomplete' THEN 'In einem dichten Zeitraum konnten nicht alle Clips abgerufen werden' END,
            completed_at = CASE WHEN $12 THEN NOW() END
            WHERE id = $1 AND lease_generation = $2")
            .bind(claim.job.id).bind(claim.generation).bind(serde_json::to_value(pending)?)
            .bind(active.map(serde_json::to_value).transpose()?).bind(serde_json::to_value(incomplete)?)
            .bind(cursor).bind(i64::try_from(page.raw_count).map_err(|_| PoolError::InvalidSource)?)
            .bind(eligible_count).bind(persisted).bind(completed_windows).bind(status).bind(finished)
            .execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn retry(
        &self,
        claim: &Claim,
        error: Option<HistoricalFetchError>,
    ) -> Result<(), PoolError> {
        let now = Utc::now();
        let attempts = claim.job.attempts + 1;
        let exhausted = attempts >= 5;
        let (retry_at, message) = match &error {
            Some(HistoricalFetchError::RateLimited(reset)) => (
                (*reset).max(now + chrono::Duration::seconds(1)),
                "Twitch begrenzt den Abruf. Der Import wird später fortgesetzt",
            ),
            Some(HistoricalFetchError::InvalidData) => (
                now + chrono::Duration::minutes(1),
                "Twitch hat unvollständige Clipdaten geliefert",
            ),
            Some(HistoricalFetchError::InvalidCursor) => (
                now,
                "Die Twitch-Seitenmarkierung konnte nicht erneuert werden",
            ),
            Some(HistoricalFetchError::Upstream(_)) => (
                now + chrono::Duration::seconds(30 * i64::from(attempts)),
                "Der Twitch-Abruf ist fehlgeschlagen. Der Import wird erneut versucht",
            ),
            None => (
                now + chrono::Duration::minutes(1),
                "Die Clipseite konnte nicht gespeichert werden. Der Import wird erneut versucht",
            ),
        };
        let restart_cursor = matches!(error, Some(HistoricalFetchError::InvalidCursor))
            && claim.window.cursor_restarts < 3;
        let status = if exhausted
            || (matches!(error, Some(HistoricalFetchError::InvalidCursor)) && !restart_cursor)
        {
            "error"
        } else {
            "retry"
        };
        let mut window = claim.window.clone();
        if restart_cursor {
            window.fetched = 0;
            window.pages = 0;
            window.cursor_restarts += 1;
        }
        let changed = sqlx::query(
            "UPDATE social_clip_import_jobs SET status = $3, lease_until = NULL,
            next_retry_at = CASE WHEN $3 = 'retry' THEN $4 END, last_error = $5,
            cursor = CASE WHEN $6 THEN NULL ELSE cursor END, active_window = $7
            WHERE id = $1 AND lease_generation = $2 AND status = 'running'",
        )
        .bind(claim.job.id)
        .bind(claim.generation)
        .bind(status)
        .bind(retry_at)
        .bind(message)
        .bind(restart_cursor)
        .bind(serde_json::to_value(window)?)
        .execute(&self.pool)
        .await?
        .rows_affected();
        if changed == 0 {
            return Err(PoolError::LeaseLost);
        }
        Ok(())
    }

    pub async fn run_once(&self) -> Result<bool, PoolError> {
        let Some(claim) = self.claim().await? else {
            return Ok(false);
        };
        match self
            .source
            .fetch_historical_page(
                &claim.job.twitch_user_id,
                claim.window.start,
                claim.window.end,
                claim.cursor.as_deref(),
            )
            .await
        {
            Ok(page) if page.next_cursor.is_some() && page.next_cursor == claim.cursor => {
                self.retry(&claim, Some(HistoricalFetchError::InvalidCursor))
                    .await?;
            }
            Ok(page) => {
                if let Err(error) = self.commit_page(&claim, page).await {
                    if matches!(error, PoolError::LeaseLost) {
                        return Err(error);
                    }
                    self.retry(&claim, None).await?;
                    return Err(error);
                }
            }
            Err(error) => self.retry(&claim, Some(error)).await?,
        }
        Ok(true)
    }

    pub async fn run(&self) {
        loop {
            let delay = match self.run_once().await {
                Ok(true) => Duration::from_secs(1),
                Ok(false) => Duration::from_secs(15),
                Err(_) => {
                    tracing::warn!("Clip-Pool: Importseite konnte nicht verarbeitet werden");
                    Duration::from_secs(15)
                }
            };
            tokio::time::sleep(delay).await;
        }
    }
}
