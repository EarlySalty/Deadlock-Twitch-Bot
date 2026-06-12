//! Periodischer Scout-Task für Live-Deadlock-Streams.
//!
//! Port von `bot/base.py:_scout_deadlock_channels` (945–1180 Z.).
//!
//! # Aufgabe
//!
//! Entdeckt live Streamer der Ziel-Kategorie (Deadlock, Sprache "de") und
//! registriert sie als `is_monitored_only = 1` in `twitch_streamers`. Streamer,
//! die 2 aufeinanderfolgende Zyklen abwesend sind, werden wieder entfernt
//! (Sessions geschlossen, Live-State gelöscht, Datenbankzeile weg).
//!
//! # Design
//!
//! - **Repository** kapselt alle DB-Zugriffe; kennt kein HTTP.
//! - **ScoutTask** hält den Absent-Cycle-Counter im Arbeitsspeicher (`HashMap`)
//!   — er ist bewusst transient (kein DB-Overhead, kein Schema-Bloat, verloren bei
//!   Neustart ist akzeptabel: nach 2 Zyklen wäre der Streamer ohnehin weg).
//! - Deaktiviert bis `TB_SCOUT_ENABLED=1` gesetzt ist.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use sqlx::PgPool;
use tb_transport_twitch::HelixClient;

// ── Konstanten ─────────────────────────────────────────────────────────────────

/// Anzahl aufeinanderfolgender abwesender Zyklen bevor ein monitoring-only
/// Streamer entfernt wird. Python: `if missed_cycles >= 2`.
const ABSENT_CYCLES_BEFORE_REMOVE: u32 = 2;

/// Standard-Intervall zwischen Scout-Zyklen.
const DEFAULT_INTERVAL: Duration = Duration::from_secs(90);

/// Initiale Wartezeit nach Prozessstart (lässt den Bot erst hochfahren).
const INITIAL_DELAY: Duration = Duration::from_secs(30);

// ── Repository ─────────────────────────────────────────────────────────────────

/// Alle DB-Zugriffe des Scout-Tasks gebündelt.
#[derive(Clone)]
pub struct ScoutRepository {
    pool: PgPool,
}

impl ScoutRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Gibt alle aktuell als `is_monitored_only = 1` markierten Logins zurück.
    pub async fn load_monitored_only_logins(&self) -> Result<Vec<String>, sqlx::Error> {
        sqlx::query_scalar(
            "SELECT twitch_login FROM twitch_streamers WHERE COALESCE(is_monitored_only, 0) = 1",
        )
        .fetch_all(&self.pool)
        .await
    }

    /// Trägt einen neuen Monitoring-only-Streamer ein. Gibt `true` zurück wenn
    /// er tatsächlich neu war (nicht nur ein Konflikt-Update).
    pub async fn upsert_monitored(
        &self,
        login: &str,
        user_id: &str,
    ) -> Result<bool, sqlx::Error> {
        let existing: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM twitch_streamers WHERE LOWER(twitch_login) = LOWER($1) LIMIT 1",
        )
        .bind(login)
        .fetch_optional(&self.pool)
        .await?;

        if existing.is_some() {
            return Ok(false);
        }

        sqlx::query(
            r#"
            INSERT INTO twitch_streamers (twitch_login, twitch_user_id, is_monitored_only)
            VALUES ($1, $2, 1)
            ON CONFLICT (twitch_login) DO NOTHING
            "#,
        )
        .bind(login)
        .bind(user_id)
        .execute(&self.pool)
        .await?;

        Ok(true)
    }

    /// Schließt offene Stream-Sessions für einen Streamer (auto-closed: scout-removed).
    pub async fn close_open_sessions(&self, login: &str) {
        let result = sqlx::query(
            r#"
            UPDATE twitch_stream_sessions
            SET ended_at = NOW(),
                duration_seconds = EXTRACT(EPOCH FROM (NOW() - started_at))::int,
                notes = COALESCE(notes || '; ', '') || 'auto-closed: scout-removed'
            WHERE LOWER(streamer_login) = LOWER($1)
              AND ended_at IS NULL
            "#,
        )
        .bind(login)
        .execute(&self.pool)
        .await;

        if let Err(e) = result {
            tracing::debug!("scout: Session-Close für {login} fehlgeschlagen: {e}");
        }
    }

    /// Löscht den Live-State-Eintrag eines Streamers.
    pub async fn delete_live_state(&self, login: &str) {
        let result = sqlx::query(
            "DELETE FROM twitch_live_state WHERE LOWER(streamer_login) = LOWER($1)",
        )
        .bind(login)
        .execute(&self.pool)
        .await;

        if let Err(e) = result {
            tracing::debug!("scout: Live-State-Delete für {login} fehlgeschlagen: {e}");
        }
    }

    /// Löscht einen Monitoring-only-Streamer und seine kaskadierenden Einträge.
    ///
    /// Safety-Guard: löscht nur wenn `is_monitored_only = 1` — Partner bleiben
    /// immer unberührt.
    pub async fn delete_monitored_streamer(&self, login: &str) -> Result<bool, sqlx::Error> {
        // Kaskadierende Clip-Tabellen (meist no-op für monitoring-only Streamer,
        // aber korrekt für den Fall dass Clips existieren).
        for sql in [
            "DELETE FROM twitch_clips_social_analytics WHERE clip_id IN \
             (SELECT id FROM twitch_clips_social_media WHERE LOWER(streamer_login) = LOWER($1))",
            "DELETE FROM twitch_clips_upload_queue WHERE clip_id IN \
             (SELECT id FROM twitch_clips_social_media WHERE LOWER(streamer_login) = LOWER($1))",
            "DELETE FROM twitch_clips_social_media WHERE LOWER(streamer_login) = LOWER($1)",
            "DELETE FROM clip_fetch_history WHERE LOWER(streamer_login) = LOWER($1)",
        ] {
            if let Err(e) = sqlx::query(sql).bind(login).execute(&self.pool).await {
                tracing::debug!("scout: Kaskaden-Delete für {login} fehlgeschlagen: {e}");
            }
        }

        let rows = sqlx::query(
            "DELETE FROM twitch_streamers WHERE LOWER(twitch_login) = LOWER($1) \
             AND COALESCE(is_monitored_only, 0) = 1",
        )
        .bind(login)
        .execute(&self.pool)
        .await?
        .rows_affected();

        Ok(rows > 0)
    }
}

// ── Typen ──────────────────────────────────────────────────────────────────────

/// Kompakte Statistik eines abgeschlossenen Scout-Zyklus (für tracing).
#[derive(Debug, Default)]
pub struct ScoutStats {
    pub streams_seen: u32,
    pub new_streamers: u32,
    pub removed_streamers: u32,
}

// ── Task ───────────────────────────────────────────────────────────────────────

/// Periodischer Scout-Task. Hält den Absent-Cycle-Counter in-memory.
pub struct ScoutTask {
    repo: ScoutRepository,
    helix: Arc<HelixClient>,
    game_name: String,
    language_filters: Vec<String>,
    interval: Duration,
    absent_cycles: HashMap<String, u32>,
}

impl ScoutTask {
    pub fn new(
        repo: ScoutRepository,
        helix: Arc<HelixClient>,
        game_name: impl Into<String>,
        language_filters: Vec<String>,
    ) -> Self {
        Self {
            repo,
            helix,
            game_name: game_name.into(),
            language_filters,
            interval: DEFAULT_INTERVAL,
            absent_cycles: HashMap::new(),
        }
    }

    /// Startet den Task wenn `TB_SCOUT_ENABLED=1` gesetzt ist.
    ///
    /// Gibt `true` zurück wenn tatsächlich gestartet.
    pub fn start_if_enabled(self) -> bool {
        let enabled = std::env::var("TB_SCOUT_ENABLED")
            .map(|v| v.trim() == "1")
            .unwrap_or(false);

        if !enabled {
            tracing::info!("scout: Task deaktiviert (TB_SCOUT_ENABLED≠1)");
            return false;
        }

        tracing::info!(
            "scout: Task startet (game={}, lang={:?}, interval={}s)",
            self.game_name,
            self.language_filters,
            self.interval.as_secs(),
        );

        tokio::spawn(self.run());
        true
    }

    async fn run(mut self) {
        tokio::time::sleep(INITIAL_DELAY).await;

        loop {
            let stats = self.run_once().await;
            tracing::info!(
                "scout: Zyklus — {} Streams gesehen, {} neu, {} entfernt",
                stats.streams_seen,
                stats.new_streamers,
                stats.removed_streamers,
            );
            tokio::time::sleep(self.interval).await;
        }
    }

    async fn run_once(&mut self) -> ScoutStats {
        let mut stats = ScoutStats::default();

        // ── Game-ID auflösen ──────────────────────────────────────────────────
        let game_id = match self.helix.search_category_id(&self.game_name).await {
            Ok(Some(id)) => id,
            Ok(None) => {
                tracing::warn!("scout: Kategorie '{}' nicht gefunden", self.game_name);
                return stats;
            }
            Err(e) => {
                tracing::warn!("scout: Kategorie-Lookup fehlgeschlagen: {e}");
                return stats;
            }
        };

        // ── Aktuelle Live-Streams holen ───────────────────────────────────────
        // Mehrere Sprachen werden als separate Requests behandelt (Helix erlaubt
        // nur einen language-Parameter pro Request).
        let mut current_logins: HashMap<String, String> = HashMap::new(); // login → user_id

        let language_list = if self.language_filters.is_empty() {
            vec![None]
        } else {
            self.language_filters.iter().map(|l| Some(l.as_str())).collect::<Vec<_>>()
        };

        for lang in language_list {
            match self.helix.get_streams_by_category(&game_id, lang, 100).await {
                Ok(streams) => {
                    for s in streams {
                        let login = s.user_login.to_lowercase();
                        if !login.is_empty() {
                            current_logins.insert(login, s.user_id.clone());
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("scout: Streams-Fetch fehlgeschlagen (lang={lang:?}): {e}");
                }
            }
        }

        stats.streams_seen = current_logins.len() as u32;

        // ── Bestehende monitoring-only Einträge laden ─────────────────────────
        let existing_monitored: std::collections::HashSet<String> =
            match self.repo.load_monitored_only_logins().await {
                Ok(v) => v.into_iter().map(|l| l.to_lowercase()).collect(),
                Err(e) => {
                    tracing::error!("scout: DB-Fehler beim Laden der monitoring-only Streamer: {e}");
                    return stats;
                }
            };

        // ── Phase 1: Neue Streamer hinzufügen ─────────────────────────────────
        for (login, user_id) in &current_logins {
            if existing_monitored.contains(login.as_str()) {
                continue;
            }
            match self.repo.upsert_monitored(login, user_id).await {
                Ok(true) => {
                    tracing::debug!("scout: Neuer Monitoring-Streamer: {login}");
                    stats.new_streamers += 1;
                }
                Ok(false) => {} // bereits vorhanden (als Partner oder monitoring)
                Err(e) => tracing::warn!("scout: DB-Fehler bei upsert für {login}: {e}"),
            }
        }

        // ── Phase 2: Absent-Cycle-Tracking + Remove ───────────────────────────
        let mut stale_keys: Vec<String> = Vec::new();

        for login in &existing_monitored {
            if current_logins.contains_key(login.as_str()) {
                // Noch live → Zähler zurücksetzen
                self.absent_cycles.remove(login);
                continue;
            }

            let cycles = self.absent_cycles.entry(login.clone()).or_insert(0);
            *cycles += 1;

            if *cycles >= ABSENT_CYCLES_BEFORE_REMOVE {
                stale_keys.push(login.clone());
            }
        }

        // Veraltete Zähler für Logins entfernen die nicht mehr monitoring-only sind
        self.absent_cycles
            .retain(|login, _| existing_monitored.contains(login.as_str()));

        for login in stale_keys {
            self.absent_cycles.remove(&login);
            self.repo.close_open_sessions(&login).await;
            self.repo.delete_live_state(&login).await;

            match self.repo.delete_monitored_streamer(&login).await {
                Ok(true) => {
                    tracing::info!("scout: Monitoring-Streamer entfernt: {login}");
                    stats.removed_streamers += 1;
                }
                Ok(false) => {
                    // Safety-Guard hat gegriffen: Login ist Partner → nicht löschen
                    tracing::debug!("scout: Delete für {login} abgelehnt (kein is_monitored_only=1)");
                }
                Err(e) => tracing::warn!("scout: Delete für {login} fehlgeschlagen: {e}"),
            }
        }

        stats
    }
}

// ── Builder ────────────────────────────────────────────────────────────────────

/// Baut einen fertigen `ScoutTask` aus Pool + Helix.
///
/// `language_filters`: Leere Liste = keine Sprach-Einschränkung (alle Sprachen).
pub fn build_scout_task(
    pool: PgPool,
    helix: Arc<HelixClient>,
    game_name: impl Into<String>,
    language_filters: Vec<String>,
) -> ScoutTask {
    let repo = ScoutRepository::new(pool);
    ScoutTask::new(repo, helix, game_name, language_filters)
}
