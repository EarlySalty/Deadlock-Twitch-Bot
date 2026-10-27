use std::time::Instant;

use super::{
    helix::HelixClipSource,
    model::{FetchStats, StreamerFetchResult},
    repository::ClipRepository,
};

const GAME_LOOKUP_BATCH: i64 = 100;

/// Orchestriert einen Clip-Fetch-Lauf.
///
/// Kennt weder HTTP noch SQL direkt — delegiert an `ClipRepository` und
/// `HelixClipSource`. Enthält ausschließlich Business-Logik.
pub struct ClipFetchService {
    repo: ClipRepository,
    helix: HelixClipSource,
    clip_limit: u32,
    rate_limit_ms: u64,
    kontingent_verbrauch: bool,
}

impl ClipFetchService {
    pub fn new(repo: ClipRepository, helix: HelixClipSource) -> Self {
        Self {
            repo,
            helix,
            clip_limit: 20,
            rate_limit_ms: 1_000,
            // Standard ist der Hintergrundlauf, und der verbraucht nichts:
            // dass Zuschauer den Kanal clippen, ist keine Kontingentnutzung.
            kontingent_verbrauch: false,
        }
    }

    /// Überschreibt die Clip-Anzahl pro Fetch (für den manuellen Dashboard-Fetch).
    pub fn with_clip_limit(mut self, limit: u32) -> Self {
        self.clip_limit = limit.max(1);
        self
    }

    /// Bucht neu aufgenommene Clips auf das Monatskontingent des Streamers.
    ///
    /// Nur fuer den vom Streamer selbst ausgeloesten Dashboard-Fetch. Der
    /// Hintergrundlauf bleibt bei `false`, sonst haette ein Free-Streamer sein
    /// Kontingent aufgebraucht, ohne je etwas angeklickt zu haben.
    pub fn mit_kontingent_verbrauch(mut self, verbraucht: bool) -> Self {
        self.kontingent_verbrauch = verbraucht;
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

        self.fetch_for_broadcaster(&user_id, login).await
    }

    /// Manueller Dashboardpfad: die autorisierte ID wird nie erneut aus dem Namen ermittelt.
    pub async fn fetch_for_broadcaster(&self, user_id: &str, login: &str) -> StreamerFetchResult {
        let started = Instant::now();
        if user_id.is_empty() || !user_id.bytes().all(|byte| byte.is_ascii_digit()) {
            return error_result(login, "Ungültige Twitch-ID", started);
        }
        match self.repo.pool_managed(user_id).await {
            Ok(true) => return self.fetch_managed_recent(user_id, login, started).await,
            Ok(false) => {}
            Err(error) => return error_result(login, &error.to_string(), started),
        }
        match self.repo.social_access_granted(user_id).await {
            Ok(true) => return self.start_pool(user_id, login, started).await,
            Ok(false) => {}
            Err(error) => return error_result(login, &error.to_string(), started),
        }
        let clips = match self
            .helix
            .fetch_clips(user_id, login, self.clip_limit)
            .await
        {
            Ok(c) => c,
            Err(e) => {
                return error_result(login, &e.to_string(), started);
            }
        };

        // FK sicherstellen bevor Clips geschrieben werden.
        if let Err(e) = self.repo.ensure_monitored_streamer(login, user_id).await {
            return error_result(login, &e.to_string(), started);
        }

        let mut clips_new = 0i32;
        for clip in &clips {
            match self.repo.register_clip(clip).await {
                Ok((clip_db_id, is_new)) => {
                    if is_new {
                        clips_new += 1;
                        // Nur neue Zeilen buchen: ein zweiter Fetch derselben
                        // Clips darf das Kontingent nicht erneut belasten.
                        if self.kontingent_verbrauch {
                            if let Err(e) = self.repo.mark_kontingent_verbrauch(clip_db_id).await {
                                tracing::warn!(
                                    "clip_fetch: Kontingent-Buchung fuer Clip {} fehlgeschlagen: {e}",
                                    clip.clip_id
                                );
                            }
                        }
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

    async fn start_pool(&self, owner: &str, login: &str, started: Instant) -> StreamerFetchResult {
        match self.repo.start_pool_import(owner, login).await {
            Ok(job) => tracing::info!(
                login,
                job = job.id,
                "clip_fetch: Clip-Pool-Import automatisch gestartet"
            ),
            Err(error) => return error_result(login, &error.to_string(), started),
        }
        self.fetch_managed_recent(owner, login, started).await
    }

    async fn fetch_managed_recent(
        &self,
        owner: &str,
        login: &str,
        started: Instant,
    ) -> StreamerFetchResult {
        let now = chrono::Utc::now();
        let start = now - chrono::Duration::days(14);
        let mut cursor = None;
        let mut clips = Vec::new();
        while clips.len() < self.clip_limit as usize {
            let page = match self
                .helix
                .fetch_historical_page(owner, start, now, cursor.as_deref())
                .await
            {
                Ok(page) => page,
                Err(error) => return error_result(login, &error.to_string(), started),
            };
            clips.extend(page.clips.into_iter().map(|mut clip| {
                clip.streamer_login = login.to_owned();
                clip
            }));
            if page.next_cursor.is_none() || page.next_cursor == cursor {
                break;
            }
            cursor = page.next_cursor;
        }
        clips.truncate(self.clip_limit as usize);
        let (clips_found, clips_new) = match self.repo.register_recent_pool(owner, &clips).await {
            Ok(counts) => counts,
            Err(error) => return error_result(login, &error.to_string(), started),
        };
        let result = StreamerFetchResult {
            login: login.to_owned(),
            clips_found,
            clips_new,
            duration_ms: started.elapsed().as_millis() as i64,
            error: None,
        };
        if self.repo.record_history(&result).await.is_err() {
            return error_result(
                login,
                "Der Abrufverlauf konnte nicht gespeichert werden",
                started,
            );
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

        self.fill_missing_game_names().await;

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

    pub async fn fill_missing_game_names(&self) {
        let ids = match self.repo.game_ids_without_name(GAME_LOOKUP_BATCH).await {
            Ok(ids) => ids,
            Err(error) => {
                tracing::warn!("clip_fetch: Spiele ohne Namen nicht lesbar: {error}");
                return;
            }
        };
        let names = match self.helix.fetch_game_names(&ids).await {
            Ok(names) => names,
            Err(error) => {
                tracing::warn!("clip_fetch: Spielnamen nicht abrufbar: {error}");
                return;
            }
        };
        let mut filled = 0u64;
        for (game_id, game_name) in &names {
            match self.repo.fill_game_name(game_id, game_name).await {
                Ok(rows) => filled += rows,
                Err(error) => {
                    tracing::warn!("clip_fetch: Spielname für {game_id} nicht gespeichert: {error}")
                }
            }
        }
        if filled > 0 {
            tracing::info!("clip_fetch: Spielnamen für {filled} Clips ergänzt");
        }
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
