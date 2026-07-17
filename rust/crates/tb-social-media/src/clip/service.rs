use std::time::Instant;

use super::{
    helix::HelixClipSource,
    model::{FetchStats, StreamerFetchResult},
    repository::ClipRepository,
};

/// Orchestriert einen Clip-Fetch-Lauf.
///
/// Kennt weder HTTP noch SQL direkt — delegiert an `ClipRepository` und
/// `HelixClipSource`. Enthält ausschließlich Business-Logik.
pub struct ClipFetchService {
    repo: ClipRepository,
    helix: HelixClipSource,
    clip_limit: u32,
    rate_limit_ms: u64,
}

impl ClipFetchService {
    pub fn new(repo: ClipRepository, helix: HelixClipSource) -> Self {
        Self {
            repo,
            helix,
            clip_limit: 20,
            rate_limit_ms: 1_000,
        }
    }

    /// Überschreibt die Clip-Anzahl pro Fetch (für den manuellen Dashboard-Fetch).
    pub fn with_clip_limit(mut self, limit: u32) -> Self {
        self.clip_limit = limit.max(1);
        self
    }

    /// Fetcht Clips für einen einzelnen Streamer und schreibt Verlaufseintrag.
    pub async fn fetch_for_streamer(&self, login: &str) -> StreamerFetchResult {
        let started = Instant::now();

        let user_id = match self.helix.fetch_user_id(login).await {
            Ok(Some(id)) => id,
            Ok(None) => {
                tracing::debug!("clip_fetch: User nicht gefunden: {login}");
                return StreamerFetchResult {
                    login: login.to_string(),
                    error: Some(format!("User '{login}' nicht in Helix gefunden")),
                    ..Default::default()
                };
            }
            Err(e) => {
                return error_result(login, &e.to_string(), started);
            }
        };

        let clips = match self
            .helix
            .fetch_clips(&user_id, login, self.clip_limit)
            .await
        {
            Ok(c) => c,
            Err(e) => {
                return error_result(login, &e.to_string(), started);
            }
        };

        // FK sicherstellen bevor Clips geschrieben werden.
        if let Err(e) = self.repo.ensure_monitored_streamer(login, &user_id).await {
            return error_result(login, &e.to_string(), started);
        }

        let mut clips_new = 0i32;
        for clip in &clips {
            match self.repo.register_clip(clip).await {
                Ok((_, is_new)) => {
                    if is_new {
                        clips_new += 1;
                    }
                }
                Err(e) => {
                    tracing::warn!("clip_fetch: DB-Fehler bei Clip {}: {e}", clip.clip_id);
                }
            }
        }

        let duration_ms = started.elapsed().as_millis() as i64;

        let result = StreamerFetchResult {
            login: login.to_string(),
            clips_found: clips.len() as i32,
            clips_new,
            duration_ms,
            error: None,
        };

        if let Err(e) = self.repo.record_history(&result).await {
            tracing::warn!("clip_fetch: History-Write für {login} fehlgeschlagen: {e}");
        }

        result
    }

    /// Fetcht Clips für alle aktiven Partner sequenziell (Rate-Limit-freundlich).
    pub async fn fetch_all_active_partners(&self) -> FetchStats {
        let started = Instant::now();
        let mut stats = FetchStats::default();

        let logins = match self.repo.active_partner_logins().await {
            Ok(l) => l,
            Err(e) => {
                tracing::error!("clip_fetch: Partner-List-Fehler: {e}");
                return stats;
            }
        };

        tracing::info!("clip_fetch: Starte Lauf für {} Partner", logins.len());

        for login in &logins {
            let result = self.fetch_for_streamer(login).await;

            if result.error.is_some() {
                stats.errors += 1;
            } else {
                stats.streamers += 1;
                stats.clips_total += result.clips_found as u32;
                stats.clips_new += result.clips_new as u32;
            }

            // Rate-Limit: 1 s zwischen Streamern (Helix-Schutz).
            tokio::time::sleep(tokio::time::Duration::from_millis(self.rate_limit_ms)).await;
        }

        stats.duration_ms = started.elapsed().as_millis() as u64;

        tracing::info!(
            "clip_fetch: Lauf fertig — {} Streamer, {} Clips ({} neu), {} Fehler, {}ms",
            stats.streamers,
            stats.clips_total,
            stats.clips_new,
            stats.errors,
            stats.duration_ms,
        );

        stats
    }
}

fn error_result(login: &str, msg: &str, started: Instant) -> StreamerFetchResult {
    tracing::warn!("clip_fetch: Fehler für {login}: {msg}");
    StreamerFetchResult {
        login: login.to_string(),
        error: Some(msg.to_string()),
        duration_ms: started.elapsed().as_millis() as i64,
        ..Default::default()
    }
}
