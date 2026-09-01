//! Zeitplan, Freigabe-Modi und Kategorien pro Streamer.
//!
//! Loest die frueheren globalen `auto_approve_*`-Keys aus `social_media_settings`
//! ab (Migration `20260815120000_social_media_scheduling.sql`). Global war doppelt
//! falsch: die Flags galten fuer die ganze Instanz, und jeder freigegebene Partner
//! konnte sie umschalten.
//!
//! JSONB wird wie im Rest des Crates ueber `::text` gelesen und als `$N::text::jsonb`
//! geschrieben, weil das sqlx-`json`-Feature bewusst nicht aktiv ist.

use chrono::{DateTime, Utc};
use sqlx::{PgConnection, PgPool};

use crate::scheduler::{next_cadence_slot, CadenceLimits};
use crate::settings::PostingSchedule;

/// Zielplattformen der Pipeline, in Anzeigereihenfolge.
pub const PLATFORMS: [&str; 3] = ["youtube", "tiktok", "instagram"];

/// Kategorie, in der alles landet, was keiner gepflegten Kategorie zuzuordnen ist.
pub const CATEGORY_FALLBACK: &str = "other";

/// Kategorie mit LLM-Anreicherung. Alles andere bekommt nacktes Auto-Posting.
pub const CATEGORY_DEADLOCK: &str = "deadlock";

/// Unter dieser Reichweite warnt das Dashboard vor leerem Clip-Pool.
pub const VORRAT_WARNUNG_TAGE: i64 = 7;

/// Freigabe-Modus eines Streamers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalMode {
    /// Jeder Clip braucht eine ausdrueckliche Freigabe.
    Manual,
    /// Clip wird eingeplant und geht raus, wenn bis zum Termin niemand widerspricht.
    VetoWindow,
    /// Clip wird ohne Sichtung eingeplant.
    FullAuto,
}

/// Ob aufbereitete Clips die Plattformgrenze überhaupt überschreiten dürfen.
/// Der sichere Default bleibt so lange `PrepareOnly`, bis die Plattformen und
/// ihre Freigaben produktiv bestätigt sind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseMode {
    PrepareOnly,
    Live,
}

impl ReleaseMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PrepareOnly => "prepare_only",
            Self::Live => "live",
        }
    }

    pub fn parse(raw: &str) -> Self {
        match raw.trim() {
            "live" => Self::Live,
            _ => Self::PrepareOnly,
        }
    }

    pub fn release_enabled(self) -> bool {
        matches!(self, Self::Live)
    }
}

impl ApprovalMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::VetoWindow => "veto_window",
            Self::FullAuto => "full_auto",
        }
    }

    /// Unbekannte Werte fallen auf `Manual` zurueck: im Zweifel nicht posten.
    pub fn parse(raw: &str) -> Self {
        match raw.trim() {
            "veto_window" => Self::VetoWindow,
            "full_auto" => Self::FullAuto,
            _ => Self::Manual,
        }
    }

    /// `true`, wenn ein Clip ohne menschliche Sichtung eingeplant werden darf.
    ///
    /// `VetoWindow` und `FullAuto` planen beide ohne Sichtung ein, hier gibt es
    /// bewusst keinen Unterschied. Der Unterschied liegt danach: im
    /// Veto-Fenster laesst sich ein eingeplanter Clip bis zum Termin wieder
    /// stoppen, ueber `approval::cancel_scheduled_uploads` und die Route
    /// `POST /social-media/api/approval/:clip_db_id/cancel`.
    pub fn schedules_without_review(self) -> bool {
        !matches!(self, Self::Manual)
    }
}

/// Einstellungen, die fuer den ganzen Kanal gelten.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamerSettings {
    pub approval_mode: ApprovalMode,
    pub timezone: String,
    pub release_mode: ReleaseMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderInFlight {
    pub queue_id: i64,
    pub platform: String,
    pub status: String,
    pub provider_external_id: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum StreamerSettingsSaveError {
    #[error("Provider-Aufruf läuft oder muss abgeglichen werden")]
    ProviderInFlight(Vec<ProviderInFlight>),
    #[error("Offener Upload-Vorrat konnte nicht sicher neu terminiert werden")]
    BacklogCannotBeScheduled { platform: String },
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

impl Default for StreamerSettings {
    fn default() -> Self {
        Self {
            approval_mode: ApprovalMode::Manual,
            timezone: "Europe/Berlin".to_string(),
            release_mode: ReleaseMode::PrepareOnly,
        }
    }
}

/// Kadenz und Auto-Posting einer Plattform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformSchedule {
    pub platform: String,
    pub auto_post: bool,
    pub posts_per_week: i32,
    pub max_posts_per_day: i32,
    pub post_times: Vec<String>,
}

impl PlatformSchedule {
    fn default_for(platform: &str) -> Self {
        let limits = CadenceLimits::default();
        Self {
            platform: platform.to_string(),
            auto_post: false,
            posts_per_week: limits.posts_per_week as i32,
            max_posts_per_day: limits.max_posts_per_day as i32,
            post_times: vec!["18:00".to_string()],
        }
    }

    /// Kadenz-Grenzen fuer den Scheduler. Negative Werte gelten als null.
    pub fn limits(&self) -> CadenceLimits {
        CadenceLimits {
            posts_per_week: self.posts_per_week.max(0) as u32,
            max_posts_per_day: self.max_posts_per_day.max(0) as u32,
        }
    }

    /// Slot-Zeiten in der Zeitzone des Streamers.
    pub fn posting_schedule(&self, timezone: &str) -> PostingSchedule {
        PostingSchedule {
            times: self.post_times.clone(),
            timezone: timezone.to_string(),
        }
    }
}

/// Eine Spielkategorie samt Schalter des Streamers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategoryOption {
    pub category_key: String,
    pub display_name: String,
    /// LLM-Anreicherung erlaubt. Steht am Katalog, nicht am Streamer.
    pub enrichment_enabled: bool,
    pub auto_post: bool,
    pub sort_order: i32,
}

/// Vorratsrechnung fuer den Clip-Pool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolForecast {
    /// Clips, die noch nicht veroeffentlicht oder verworfen sind.
    pub verfuegbare_clips: i64,
    /// Plattformen mit eingeschaltetem Auto-Posting.
    pub aktive_plattformen: i64,
    /// Wie viele Posts sich aus dem Vorrat noch machen lassen.
    pub reicht_fuer_posts: i64,
    /// Geplante Posts pro Woche ueber alle aktiven Plattformen.
    pub posts_pro_woche: i64,
    /// Wie lange der Vorrat bei dieser Kadenz traegt. `None` ohne aktive Kadenz.
    pub reicht_fuer_tage: Option<i64>,
    /// Der Vorrat traegt keine volle Woche mehr.
    pub warnung: bool,
}

/// Rechnet den Vorrat aus Bestand und Kadenz aus, ohne IO.
pub fn berechne_vorrat(verfuegbare_clips: i64, schedules: &[PlatformSchedule]) -> PoolForecast {
    let aktive: Vec<&PlatformSchedule> = schedules
        .iter()
        .filter(|s| s.auto_post && !s.limits().blocks_everything())
        .collect();
    let aktive_plattformen = aktive.len() as i64;
    let posts_pro_woche: i64 = aktive
        .iter()
        .map(|s| i64::from(s.posts_per_week.max(0)))
        .sum();
    let reicht_fuer_posts = verfuegbare_clips.max(0) * aktive_plattformen;
    let reicht_fuer_tage = if posts_pro_woche > 0 {
        Some(reicht_fuer_posts * 7 / posts_pro_woche)
    } else {
        None
    };
    PoolForecast {
        verfuegbare_clips,
        aktive_plattformen,
        reicht_fuer_posts,
        posts_pro_woche,
        reicht_fuer_tage,
        warnung: reicht_fuer_tage.is_some_and(|tage| tage < VORRAT_WARNUNG_TAGE),
    }
}

/// Legt fehlende Zeilen fuer einen Streamer an, damit alle Leser vollstaendige
/// Daten sehen. Idempotent und nur additiv.
pub async fn ensure_streamer_rows(pool: &PgPool, streamer_login: &str) -> Result<(), sqlx::Error> {
    let login = streamer_login.trim().to_lowercase();
    if login.is_empty() {
        return Ok(());
    }
    let defaults = StreamerSettings::default();
    sqlx::query!(
        "INSERT INTO social_media_streamer_settings (streamer_login, approval_mode, timezone, updated_by) \
         SELECT $1, $2, $3, 'auto_default' \
          WHERE EXISTS (SELECT 1 FROM twitch_streamers WHERE twitch_login = $1) \
         ON CONFLICT (streamer_login) DO NOTHING",
        login,
        defaults.approval_mode.as_str(),
        defaults.timezone
    )
    .execute(pool)
    .await?;

    for platform in PLATFORMS {
        let row = PlatformSchedule::default_for(platform);
        let times = serde_json::to_string(&row.post_times).unwrap_or_else(|_| "[]".to_string());
        sqlx::query!(
            "INSERT INTO social_media_platform_schedule \
                 (streamer_login, platform, auto_post, posts_per_week, max_posts_per_day, post_times, updated_by) \
             SELECT $1, $2, FALSE, $3, $4, $5::text::jsonb, 'auto_default' \
              WHERE EXISTS (SELECT 1 FROM twitch_streamers WHERE twitch_login = $1) \
             ON CONFLICT (streamer_login, platform) DO NOTHING",
            login,
            platform,
            row.posts_per_week,
            row.max_posts_per_day,
            times
        )
        .execute(pool)
        .await?;
    }

    // Deadlock ist die einzige aktive Kategorie und startet eingeschaltet; der
    // scharfe Schalter bleibt das Auto-Posting der Plattform.
    sqlx::query!(
        "INSERT INTO social_media_category_settings (streamer_login, category_key, auto_post, updated_by) \
         SELECT $1, k.category_key, (k.category_key = $2), 'auto_default' \
           FROM social_media_category k \
          WHERE EXISTS (SELECT 1 FROM twitch_streamers WHERE twitch_login = $1) \
         ON CONFLICT (streamer_login, category_key) DO NOTHING",
        login,
        CATEGORY_DEADLOCK
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Liest die Kanal-Einstellungen; fehlende Zeile ergibt den Default.
pub async fn load_streamer_settings(pool: &PgPool, streamer_login: &str) -> StreamerSettings {
    match load_streamer_settings_checked(pool, streamer_login).await {
        Ok(settings) => settings,
        Err(error) => {
            tracing::error!(
                %error,
                streamer = %streamer_login,
                code = "streamer_settings_read_failed",
                "Social-Media-Kanaleinstellungen konnten nicht gelesen werden"
            );
            StreamerSettings::default()
        }
    }
}

pub async fn load_streamer_settings_checked(
    pool: &PgPool,
    streamer_login: &str,
) -> Result<StreamerSettings, sqlx::Error> {
    let login = streamer_login.trim().to_lowercase();
    let row: Option<(String, String, String)> = sqlx::query_as(
        "SELECT approval_mode, timezone, release_mode \
         FROM social_media_streamer_settings WHERE streamer_login = $1",
    )
    .bind(login)
    .fetch_optional(pool)
    .await?;
    Ok(match row {
        Some((approval_mode, timezone, release_mode)) => StreamerSettings {
            approval_mode: ApprovalMode::parse(&approval_mode),
            timezone,
            release_mode: ReleaseMode::parse(&release_mode),
        },
        None => StreamerSettings::default(),
    })
}

/// Setzt Freigabe-Modus und Zeitzone.
pub async fn save_streamer_settings(
    pool: &PgPool,
    streamer_login: &str,
    settings: &StreamerSettings,
    updated_by: Option<&str>,
) -> Result<StreamerSettings, StreamerSettingsSaveError> {
    let login = streamer_login.trim().to_lowercase();
    let updated_by = updated_by.map(str::trim).filter(|s| !s.is_empty());
    let mut transaction = pool.begin().await?;
    acquire_release_lock(transaction.as_mut(), &login).await?;
    let previous_release_mode: Option<String> = sqlx::query_scalar(
        "SELECT release_mode FROM social_media_streamer_settings \
         WHERE streamer_login = $1 FOR UPDATE",
    )
    .bind(&login)
    .fetch_optional(transaction.as_mut())
    .await?;
    let provider_in_flight = if settings.release_mode == ReleaseMode::PrepareOnly {
        let rows: Vec<(i64, String, String, Option<String>)> = sqlx::query_as(
            "SELECT q.id, q.platform, q.status, q.provider_external_id \
             FROM twitch_clips_upload_queue q \
             JOIN twitch_clips_social_media c ON c.id = q.clip_id \
             WHERE LOWER(c.streamer_login) = LOWER($1) \
               AND q.provider_started_at IS NOT NULL \
               AND (q.status IN ('processing', 'reconciliation_required') \
                    OR (q.status IN ('completed', 'failed') \
                        AND q.completed_at >= transaction_timestamp())) \
             ORDER BY q.id FOR UPDATE OF q",
        )
        .bind(&login)
        .fetch_all(transaction.as_mut())
        .await?;
        rows.into_iter()
            .map(
                |(queue_id, platform, status, provider_external_id)| ProviderInFlight {
                    queue_id,
                    platform,
                    status,
                    provider_external_id,
                },
            )
            .collect()
    } else {
        Vec::new()
    };
    if settings.release_mode == ReleaseMode::Live
        && previous_release_mode.as_deref() != Some(ReleaseMode::Live.as_str())
    {
        replan_pending_for_live(transaction.as_mut(), &login, &settings.timezone).await?;
    }
    sqlx::query(
        "INSERT INTO social_media_streamer_settings \
             (streamer_login, approval_mode, timezone, release_mode, updated_at, updated_by) \
         VALUES ($1, $2, $3, $4, CURRENT_TIMESTAMP, $5) \
         ON CONFLICT (streamer_login) DO UPDATE SET \
             approval_mode = EXCLUDED.approval_mode, \
             timezone = EXCLUDED.timezone, \
             release_mode = EXCLUDED.release_mode, \
             updated_at = CURRENT_TIMESTAMP, \
             updated_by = EXCLUDED.updated_by",
    )
    .bind(&login)
    .bind(settings.approval_mode.as_str())
    .bind(&settings.timezone)
    .bind(settings.release_mode.as_str())
    .bind(updated_by)
    .execute(transaction.as_mut())
    .await?;
    transaction.commit().await?;
    if !provider_in_flight.is_empty() {
        return Err(StreamerSettingsSaveError::ProviderInFlight(
            provider_in_flight,
        ));
    }
    Ok(settings.clone())
}

async fn replan_pending_for_live(
    connection: &mut PgConnection,
    streamer_login: &str,
    timezone: &str,
) -> Result<(), StreamerSettingsSaveError> {
    let pending: Vec<(i64, String)> = sqlx::query_as(
        "SELECT q.id, q.platform FROM twitch_clips_upload_queue q \
         JOIN twitch_clips_social_media c ON c.id = q.clip_id \
         WHERE LOWER(c.streamer_login) = LOWER($1) \
           AND q.status = 'pending' AND q.provider_started_at IS NULL \
         ORDER BY q.platform, q.priority DESC, q.scheduled_at NULLS FIRST, q.created_at, q.id \
         FOR UPDATE OF q",
    )
    .bind(streamer_login)
    .fetch_all(&mut *connection)
    .await?;
    if pending.is_empty() {
        return Ok(());
    }

    let rows: Vec<(String, bool, i32, i32, Option<String>)> = sqlx::query_as(
        "SELECT platform, auto_post, posts_per_week, max_posts_per_day, post_times::text \
         FROM social_media_platform_schedule WHERE streamer_login = $1",
    )
    .bind(streamer_login)
    .fetch_all(&mut *connection)
    .await?;
    let schedules: Vec<PlatformSchedule> = PLATFORMS
        .iter()
        .map(|platform| {
            rows.iter()
                .find(|row| row.0 == *platform)
                .map(|row| PlatformSchedule {
                    platform: row.0.clone(),
                    auto_post: row.1 && row.0 != "tiktok",
                    posts_per_week: row.2,
                    max_posts_per_day: row.3,
                    post_times: row
                        .4
                        .as_deref()
                        .and_then(|raw| serde_json::from_str::<Vec<String>>(raw).ok())
                        .unwrap_or_else(|| PlatformSchedule::default_for(platform).post_times),
                })
                .unwrap_or_else(|| PlatformSchedule::default_for(platform))
        })
        .collect();
    let pending_ids: Vec<i64> = pending.iter().map(|(id, _)| *id).collect();
    let existing: Vec<(String, DateTime<Utc>)> = sqlx::query_as(
        "SELECT q.platform, q.scheduled_at FROM twitch_clips_upload_queue q \
         JOIN twitch_clips_social_media c ON c.id = q.clip_id \
         WHERE LOWER(c.streamer_login) = LOWER($1) \
           AND q.id <> ALL($2::bigint[]) \
           AND q.status NOT IN ('failed') AND q.scheduled_at IS NOT NULL \
           AND q.scheduled_at > CURRENT_TIMESTAMP - INTERVAL '7 days'",
    )
    .bind(streamer_login)
    .bind(&pending_ids)
    .fetch_all(&mut *connection)
    .await?;
    let now = Utc::now();
    let mut taken_by_platform: std::collections::HashMap<String, Vec<DateTime<Utc>>> =
        std::collections::HashMap::new();
    for (platform, scheduled_at) in existing {
        taken_by_platform
            .entry(platform)
            .or_default()
            .push(scheduled_at);
    }
    for (queue_id, platform) in pending {
        let Some(schedule) = schedules
            .iter()
            .find(|schedule| schedule.platform == platform)
        else {
            return Err(StreamerSettingsSaveError::BacklogCannotBeScheduled { platform });
        };
        let taken = taken_by_platform.entry(platform.clone()).or_default();
        let Some(slot) = next_cadence_slot(
            now,
            taken,
            &schedule.posting_schedule(timezone),
            &schedule.limits(),
        ) else {
            return Err(StreamerSettingsSaveError::BacklogCannotBeScheduled { platform });
        };
        sqlx::query(
            "UPDATE twitch_clips_upload_queue SET scheduled_at = $2, last_error = NULL \
             WHERE id = $1 AND status = 'pending' AND provider_started_at IS NULL",
        )
        .bind(queue_id)
        .bind(slot)
        .execute(&mut *connection)
        .await?;
        taken.push(slot);
    }
    Ok(())
}

/// Serialisiert den letzten Release-Check/Provider-Start mit Änderungen am
/// Kill-Switch desselben Streamers. Die Sperre wird nur bis zum commitbaren
/// Provider-Startmarker gehalten, nie über einen Netzwerkaufruf.
pub(crate) async fn acquire_release_lock(
    connection: &mut PgConnection,
    streamer_login: &str,
) -> Result<(), sqlx::Error> {
    let key = format!(
        "tb-social-media-release:{}",
        streamer_login.trim().to_lowercase()
    );
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(key)
        .execute(&mut *connection)
        .await?;
    Ok(())
}

/// Hartes Release-Gate für den Upload-Pfad. Fehlende Settings und unbekannte
/// Werte sind absichtlich gesperrt.
pub async fn release_enabled_for_clip(pool: &PgPool, clip_db_id: i64) -> bool {
    match sqlx::query_scalar::<_, bool>(
        "SELECT COALESCE(s.release_mode = 'live', FALSE) \
           FROM twitch_clips_social_media c \
           LEFT JOIN social_media_streamer_settings s \
             ON LOWER(s.streamer_login) = LOWER(c.streamer_login) \
          WHERE c.id = $1 AND c.discarded_at IS NULL",
    )
    .bind(clip_db_id)
    .fetch_optional(pool)
    .await
    {
        Ok(Some(enabled)) => enabled,
        Ok(None) => false,
        Err(error) => {
            tracing::error!(
                %error,
                clip_db_id,
                code = "release_gate_read_failed",
                "Social-Media-Release-Gate konnte nicht gelesen werden"
            );
            false
        }
    }
}

/// Kadenz aller Plattformen, fehlende Zeilen als Default aufgefuellt.
pub async fn load_platform_schedules(pool: &PgPool, streamer_login: &str) -> Vec<PlatformSchedule> {
    match load_platform_schedules_checked(pool, streamer_login).await {
        Ok(schedules) => schedules,
        Err(error) => {
            tracing::error!(
                %error,
                streamer = %streamer_login,
                code = "platform_schedules_read_failed",
                "Social-Media-Plattformzeitpläne konnten nicht gelesen werden"
            );
            Vec::new()
        }
    }
}

pub async fn load_platform_schedules_checked(
    pool: &PgPool,
    streamer_login: &str,
) -> Result<Vec<PlatformSchedule>, sqlx::Error> {
    let login = streamer_login.trim().to_lowercase();
    let rows = sqlx::query!(
        "SELECT platform, auto_post, posts_per_week, max_posts_per_day, \
                post_times::text AS \"post_times?\" \
           FROM social_media_platform_schedule WHERE streamer_login = $1",
        login
    )
    .fetch_all(pool)
    .await?;

    Ok(PLATFORMS
        .iter()
        .map(|platform| {
            rows.iter()
                .find(|row| row.platform == *platform)
                .map(|row| PlatformSchedule {
                    platform: row.platform.clone(),
                    auto_post: row.auto_post,
                    posts_per_week: row.posts_per_week,
                    max_posts_per_day: row.max_posts_per_day,
                    post_times: row
                        .post_times
                        .as_deref()
                        .and_then(|raw| serde_json::from_str::<Vec<String>>(raw).ok())
                        .unwrap_or_else(|| PlatformSchedule::default_for(platform).post_times),
                })
                .unwrap_or_else(|| PlatformSchedule::default_for(platform))
        })
        .collect())
}

/// Schreibt die Kadenz einer Plattform.
pub async fn save_platform_schedule(
    pool: &PgPool,
    streamer_login: &str,
    schedule: &PlatformSchedule,
    updated_by: Option<&str>,
) -> Result<(), sqlx::Error> {
    let login = streamer_login.trim().to_lowercase();
    let updated_by = updated_by.map(str::trim).filter(|s| !s.is_empty());
    let times = serde_json::to_string(&schedule.post_times).unwrap_or_else(|_| "[]".to_string());
    // Auch interne Aufrufer dürfen den allgemeinen Schalter nicht als
    // TikTok-Direct-Post-Zustimmung missverstehen.
    let auto_post = schedule.auto_post && schedule.platform != "tiktok";
    let mut transaction = pool.begin().await?;
    acquire_release_lock(transaction.as_mut(), &login).await?;
    sqlx::query(
        "INSERT INTO social_media_platform_schedule \
             (streamer_login, platform, auto_post, posts_per_week, max_posts_per_day, \
              post_times, updated_at, updated_by) \
         VALUES ($1, $2, $3, $4, $5, $6::text::jsonb, CURRENT_TIMESTAMP, $7) \
         ON CONFLICT (streamer_login, platform) DO UPDATE SET \
             auto_post = EXCLUDED.auto_post, \
             posts_per_week = EXCLUDED.posts_per_week, \
             max_posts_per_day = EXCLUDED.max_posts_per_day, \
             post_times = EXCLUDED.post_times, \
             updated_at = CURRENT_TIMESTAMP, \
             updated_by = EXCLUDED.updated_by",
    )
    .bind(&login)
    .bind(&schedule.platform)
    .bind(auto_post)
    .bind(schedule.posts_per_week)
    .bind(schedule.max_posts_per_day)
    .bind(times)
    .bind(updated_by)
    .execute(transaction.as_mut())
    .await?;
    if !auto_post || schedule.limits().blocks_everything() {
        sqlx::query(
            "UPDATE twitch_clips_upload_queue q \
                SET status = 'failed', last_error = 'platform_paused', \
                    last_attempt_at = CURRENT_TIMESTAMP \
               FROM twitch_clips_social_media c \
              WHERE c.id = q.clip_id \
                AND LOWER(c.streamer_login) = LOWER($1) \
                AND q.platform = $2 AND q.status = 'pending' \
                AND q.provider_started_at IS NULL",
        )
        .bind(&login)
        .bind(&schedule.platform)
        .execute(transaction.as_mut())
        .await?;
    }
    transaction.commit().await?;
    Ok(())
}

/// Kategorien samt Schalter des Streamers.
pub async fn load_categories(pool: &PgPool, streamer_login: &str) -> Vec<CategoryOption> {
    match load_categories_checked(pool, streamer_login).await {
        Ok(categories) => categories,
        Err(error) => {
            tracing::error!(
                %error,
                streamer = %streamer_login,
                code = "categories_read_failed",
                "Social-Media-Kategorien konnten nicht gelesen werden"
            );
            Vec::new()
        }
    }
}

pub async fn load_categories_checked(
    pool: &PgPool,
    streamer_login: &str,
) -> Result<Vec<CategoryOption>, sqlx::Error> {
    let login = streamer_login.trim().to_lowercase();
    let rows = sqlx::query!(
        "SELECT k.category_key, k.display_name, k.enrichment_enabled, k.sort_order, \
                COALESCE(s.auto_post, FALSE) AS \"auto_post!\" \
           FROM social_media_category k \
           LEFT JOIN social_media_category_settings s \
                  ON s.category_key = k.category_key AND s.streamer_login = $1 \
          ORDER BY k.sort_order, k.category_key",
        login
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| CategoryOption {
            category_key: row.category_key,
            display_name: row.display_name,
            enrichment_enabled: row.enrichment_enabled,
            auto_post: row.auto_post,
            sort_order: row.sort_order,
        })
        .collect())
}

/// Schaltet Auto-Posting fuer eine Kategorie.
pub async fn save_category_setting(
    pool: &PgPool,
    streamer_login: &str,
    category_key: &str,
    auto_post: bool,
    updated_by: Option<&str>,
) -> Result<(), sqlx::Error> {
    let login = streamer_login.trim().to_lowercase();
    let updated_by = updated_by.map(str::trim).filter(|s| !s.is_empty());
    sqlx::query!(
        "INSERT INTO social_media_category_settings \
             (streamer_login, category_key, auto_post, updated_at, updated_by) \
         VALUES ($1, $2, $3, CURRENT_TIMESTAMP, $4) \
         ON CONFLICT (streamer_login, category_key) DO UPDATE SET \
             auto_post = EXCLUDED.auto_post, \
             updated_at = CURRENT_TIMESTAMP, \
             updated_by = EXCLUDED.updated_by",
        login,
        category_key,
        auto_post,
        updated_by
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Ordnet einen Clip einer Kategorie zu. Die Helix-`game_id` schlaegt den Namen;
/// ohne Treffer landet der Clip in [`CATEGORY_FALLBACK`].
pub async fn resolve_category(
    pool: &PgPool,
    game_id: Option<&str>,
    game_name: Option<&str>,
) -> String {
    let game_id = game_id.map(str::trim).filter(|s| !s.is_empty());
    let game_name = game_name
        .map(|name| name.trim().to_lowercase())
        .filter(|s| !s.is_empty());
    sqlx::query_scalar!(
        "SELECT category_key FROM social_media_category \
          WHERE ($1::text IS NOT NULL AND twitch_game_id = $1) \
             OR ($2::text IS NOT NULL AND $2 = ANY (match_game_names)) \
          ORDER BY ($1::text IS NOT NULL AND twitch_game_id = $1) DESC, sort_order \
          LIMIT 1",
        game_id,
        game_name
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .unwrap_or_else(|| CATEGORY_FALLBACK.to_string())
}

/// `true`, wenn die Kategorie dieses Clips LLM-Anreicherung bekommen darf.
pub async fn enrichment_allowed_for_clip(pool: &PgPool, clip_db_id: i64) -> bool {
    sqlx::query_scalar!(
        "SELECT k.enrichment_enabled FROM twitch_clips_social_media c \
           JOIN social_media_category k ON k.category_key = c.category_key \
          WHERE c.id = $1",
        clip_db_id
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .unwrap_or(false)
}

/// `true`, wenn dieser Clip ohne Sichtung eingeplant werden darf: Freigabe-Modus
/// laesst es zu und die Kategorie des Clips ist eingeschaltet.
pub async fn auto_schedule_allowed(pool: &PgPool, clip_db_id: i64) -> bool {
    let Some(row) = sqlx::query!(
        "SELECT c.streamer_login, COALESCE(s.auto_post, FALSE) AS \"auto_post!\" \
           FROM twitch_clips_social_media c \
           LEFT JOIN social_media_category_settings s \
                  ON s.category_key = c.category_key AND s.streamer_login = c.streamer_login \
          WHERE c.id = $1",
        clip_db_id
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten() else {
        return false;
    };
    if !row.auto_post {
        return false;
    }
    load_streamer_settings(pool, &row.streamer_login)
        .await
        .approval_mode
        .schedules_without_review()
}

/// Plattformen, auf denen dieser Streamer automatisch posten laesst.
pub async fn auto_post_platforms(pool: &PgPool, streamer_login: &str) -> Vec<String> {
    load_platform_schedules(pool, streamer_login)
        .await
        .into_iter()
        // TikTok Direct Post verlangt pro Beitrag eine ausdrückliche
        // Creator-Zustimmung sowie eine bewusst gewählte Sichtbarkeit und
        // Interaktionsoptionen. Diese per-Clip-Daten gibt es noch nicht; ein
        // allgemeiner Kanal-Schalter darf daher niemals als Zustimmung gelten.
        .filter(|s| s.platform != "tiktok" && s.auto_post && !s.limits().blocks_everything())
        .map(|s| s.platform)
        .collect()
}

/// Plattformen dieses Kanals, deren Kadenz auf null steht.
///
/// Eine solche Plattform ist ausgeschaltet: sie bekommt nie einen Termin, also
/// darf auch keine Freigabe auf ihr landen.
pub async fn pausierte_plattformen(pool: &PgPool, streamer_login: &str) -> Vec<String> {
    load_platform_schedules(pool, streamer_login)
        .await
        .into_iter()
        .filter(|s| s.limits().blocks_everything())
        .map(|s| s.platform)
        .collect()
}

/// Ergebnis der Terminsuche fuer eine Plattform.
///
/// Ein blosses `Option<DateTime<Utc>>` warf drei sehr verschiedene Faelle in
/// dasselbe `None`. Das ist beim Einreihen in die Warteschlange gefaehrlich,
/// weil ein leeres `scheduled_at` dort "sofort faellig" bedeutet: aus "kein
/// Termin frei" wurde damit still "sofort posten".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotPlan {
    /// Naechster freier Termin.
    Termin(DateTime<Utc>),
    /// Die Kadenz der Plattform steht auf null. Die Plattform ist damit
    /// ausgeschaltet, egal was sonst eingestellt ist.
    Ausgeschaltet,
    /// Die Kadenz laesst Posts zu, im Planungshorizont ist aber jeder Termin
    /// schon vergeben.
    HorizontVoll,
}

/// Naechster freier Termin fuer diese Plattform, unter Beachtung der Kadenz und
/// der schon eingeplanten Uploads.
pub async fn plan_next_slot(
    pool: &PgPool,
    streamer_login: &str,
    platform: &str,
    now: DateTime<Utc>,
) -> Result<SlotPlan, sqlx::Error> {
    let login = streamer_login.trim().to_lowercase();
    let schedule = load_platform_schedules_checked(pool, &login)
        .await?
        .into_iter()
        .find(|s| s.platform == platform)
        // `load_platform_schedules` fuellt jede Plattform aus `PLATFORMS` auf,
        // die Zeile greift also nur, wenn jemand einen Namen ausserhalb der
        // Liste hereinreicht. Auch dann gilt eine echte Kadenz: einen Pfad
        // "kein Plan, also sofort" soll es hier bewusst nicht geben.
        .unwrap_or_else(|| PlatformSchedule::default_for(platform));
    let limits = schedule.limits();
    if limits.blocks_everything() {
        return Ok(SlotPlan::Ausgeschaltet);
    }
    let settings = load_streamer_settings_checked(pool, &login).await?;
    let taken = belegte_termine(pool, &login, platform).await?;
    Ok(
        match next_cadence_slot(
            now,
            &taken,
            &schedule.posting_schedule(&settings.timezone),
            &limits,
        ) {
            Some(termin) => SlotPlan::Termin(termin),
            None => SlotPlan::HorizontVoll,
        },
    )
}

/// Schon vergebene Termine dieser Plattform: eingeplante und erledigte Uploads
/// im relevanten Zeitfenster.
async fn belegte_termine(
    pool: &PgPool,
    streamer_login: &str,
    platform: &str,
) -> Result<Vec<DateTime<Utc>>, sqlx::Error> {
    sqlx::query_scalar!(
        "SELECT COALESCE(q.scheduled_at, q.completed_at) AS \"termin!\" \
           FROM twitch_clips_upload_queue q \
           JOIN twitch_clips_social_media c ON c.id = q.clip_id \
          WHERE LOWER(c.streamer_login) = $1 \
            AND q.platform = $2 \
            AND q.status <> 'failed' \
            AND COALESCE(q.scheduled_at, q.completed_at) IS NOT NULL \
            AND COALESCE(q.scheduled_at, q.completed_at) > CURRENT_TIMESTAMP - INTERVAL '14 days'",
        streamer_login,
        platform
    )
    .fetch_all(pool)
    .await
}

/// Zaehlt die Clips, die noch fuer Posts zur Verfuegung stehen: nicht verworfen,
/// nicht schon ueberall veroeffentlicht, und in einer eingeschalteten Kategorie.
pub async fn verfuegbare_clips(pool: &PgPool, streamer_login: &str) -> Result<i64, sqlx::Error> {
    let login = streamer_login.trim().to_lowercase();
    sqlx::query_scalar!(
        "SELECT COUNT(*) AS \"anzahl!\" FROM twitch_clips_social_media c \
           JOIN social_media_category_settings s \
             ON s.category_key = c.category_key AND s.streamer_login = c.streamer_login \
          WHERE LOWER(c.streamer_login) = $1 \
            AND s.auto_post \
            AND c.discarded_at IS NULL \
            AND COALESCE(c.status, 'pending') NOT IN ('published_all', 'discarded', 'skipped')",
        login
    )
    .fetch_one(pool)
    .await
}

/// Vorratsrechnung fuer das Dashboard.
pub async fn pool_forecast(
    pool: &PgPool,
    streamer_login: &str,
) -> Result<PoolForecast, sqlx::Error> {
    let clips = verfuegbare_clips(pool, streamer_login).await?;
    let schedules = load_platform_schedules_checked(pool, streamer_login).await?;
    Ok(berechne_vorrat(clips, &schedules))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = crate::test_support::test_dsn()?;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
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
        let options = PgConnectOptions::from_str(&dsn)
            .unwrap()
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await
            .unwrap();
        for ddl in [
            "CREATE TABLE social_media_streamer_settings (streamer_login TEXT PRIMARY KEY, approval_mode TEXT NOT NULL DEFAULT 'manual', timezone TEXT NOT NULL DEFAULT 'Europe/Berlin', release_mode TEXT NOT NULL DEFAULT 'prepare_only', updated_at TIMESTAMPTZ DEFAULT NOW(), updated_by TEXT)",
            "CREATE TABLE social_media_platform_schedule (streamer_login TEXT NOT NULL, platform TEXT NOT NULL, auto_post BOOLEAN NOT NULL DEFAULT FALSE, posts_per_week INTEGER NOT NULL DEFAULT 4, max_posts_per_day INTEGER NOT NULL DEFAULT 1, post_times JSONB NOT NULL DEFAULT '[\"18:00\"]'::jsonb, updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), updated_by TEXT, PRIMARY KEY (streamer_login, platform))",
            "CREATE TABLE social_media_platform_auth (id BIGSERIAL PRIMARY KEY, platform TEXT NOT NULL, streamer_login TEXT, enabled INTEGER NOT NULL DEFAULT 1)",
            "CREATE TABLE twitch_clips_social_media (id BIGSERIAL PRIMARY KEY, clip_id TEXT NOT NULL, streamer_login TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'pending', discarded_at TIMESTAMPTZ, uploaded_tiktok BOOLEAN NOT NULL DEFAULT FALSE, uploaded_youtube BOOLEAN NOT NULL DEFAULT FALSE, uploaded_instagram BOOLEAN NOT NULL DEFAULT FALSE, tiktok_video_id TEXT, youtube_video_id TEXT, instagram_media_id TEXT, tiktok_uploaded_at TIMESTAMPTZ, youtube_uploaded_at TIMESTAMPTZ, instagram_uploaded_at TIMESTAMPTZ)",
            "CREATE TABLE twitch_clips_upload_queue (id BIGSERIAL PRIMARY KEY, clip_id BIGINT NOT NULL, platform TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'pending', priority INTEGER NOT NULL DEFAULT 0, scheduled_at TIMESTAMPTZ, created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), completed_at TIMESTAMPTZ, provider_started_at TIMESTAMPTZ, provider_lease_token TEXT, provider_external_id TEXT, provider_accepted_at TIMESTAMPTZ, last_error TEXT)",
            "CREATE TABLE social_media_clip_preparation (clip_db_id BIGINT PRIMARY KEY)",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        Some(pool)
    }

    fn plan(platform: &str, auto_post: bool, pro_woche: i32) -> PlatformSchedule {
        PlatformSchedule {
            auto_post,
            posts_per_week: pro_woche,
            ..PlatformSchedule::default_for(platform)
        }
    }

    #[test]
    fn modus_parst_und_faellt_sicher_zurueck() {
        assert_eq!(ApprovalMode::parse("manual"), ApprovalMode::Manual);
        assert_eq!(ApprovalMode::parse("veto_window"), ApprovalMode::VetoWindow);
        assert_eq!(ApprovalMode::parse("full_auto"), ApprovalMode::FullAuto);
        // Unbekanntes und Muell landen im sichersten Modus.
        assert_eq!(ApprovalMode::parse("quatsch"), ApprovalMode::Manual);
        assert_eq!(ApprovalMode::parse(""), ApprovalMode::Manual);
    }

    #[test]
    fn release_modus_faellt_sicher_auf_prepare_only_zurueck() {
        assert_eq!(ReleaseMode::parse("live"), ReleaseMode::Live);
        assert!(ReleaseMode::parse("live").release_enabled());
        assert_eq!(ReleaseMode::parse("kaputt"), ReleaseMode::PrepareOnly);
        assert!(!ReleaseMode::parse("").release_enabled());
        assert_eq!(
            StreamerSettings::default().release_mode,
            ReleaseMode::PrepareOnly
        );
    }

    #[test]
    fn nur_manual_verlangt_sichtung() {
        assert!(!ApprovalMode::Manual.schedules_without_review());
        assert!(ApprovalMode::VetoWindow.schedules_without_review());
        assert!(ApprovalMode::FullAuto.schedules_without_review());
    }

    #[test]
    fn modus_roundtrip_ueber_text() {
        for modus in [
            ApprovalMode::Manual,
            ApprovalMode::VetoWindow,
            ApprovalMode::FullAuto,
        ] {
            assert_eq!(ApprovalMode::parse(modus.as_str()), modus);
        }
    }

    #[test]
    fn vorrat_ohne_aktive_plattform_warnt_nicht() {
        let vorrat = berechne_vorrat(12, &[plan("youtube", false, 4)]);
        assert_eq!(vorrat.aktive_plattformen, 0);
        assert_eq!(vorrat.reicht_fuer_posts, 0);
        assert_eq!(vorrat.reicht_fuer_tage, None);
        assert!(!vorrat.warnung);
    }

    #[test]
    fn vorrat_rechnet_posts_pro_plattform() {
        // 10 Clips auf zwei aktiven Plattformen sind 20 Posts, bei 8 Posts pro
        // Woche also 17 Tage Reichweite.
        let vorrat = berechne_vorrat(
            10,
            &[
                plan("youtube", true, 4),
                plan("tiktok", true, 4),
                plan("instagram", false, 4),
            ],
        );
        assert_eq!(vorrat.aktive_plattformen, 2);
        assert_eq!(vorrat.reicht_fuer_posts, 20);
        assert_eq!(vorrat.posts_pro_woche, 8);
        assert_eq!(vorrat.reicht_fuer_tage, Some(17));
        assert!(!vorrat.warnung);
    }

    #[test]
    fn knapper_vorrat_loest_die_warnung_aus() {
        // 2 Clips, eine Plattform, 4 Posts pro Woche: reicht 3 Tage.
        let vorrat = berechne_vorrat(2, &[plan("youtube", true, 4)]);
        assert_eq!(vorrat.reicht_fuer_posts, 2);
        assert_eq!(vorrat.reicht_fuer_tage, Some(3));
        assert!(vorrat.warnung);
    }

    #[test]
    fn leerer_pool_warnt() {
        let vorrat = berechne_vorrat(0, &[plan("youtube", true, 4)]);
        assert_eq!(vorrat.reicht_fuer_posts, 0);
        assert_eq!(vorrat.reicht_fuer_tage, Some(0));
        assert!(vorrat.warnung);
    }

    #[test]
    fn kadenz_null_zaehlt_nicht_als_aktive_plattform() {
        let vorrat = berechne_vorrat(10, &[plan("youtube", true, 0)]);
        assert_eq!(vorrat.aktive_plattformen, 0);
        assert_eq!(vorrat.reicht_fuer_tage, None);
        assert!(!vorrat.warnung);
    }

    #[test]
    fn grenzen_kappen_negative_werte() {
        let kaputt = PlatformSchedule {
            posts_per_week: -5,
            max_posts_per_day: -1,
            ..PlatformSchedule::default_for("youtube")
        };
        assert_eq!(kaputt.limits().posts_per_week, 0);
        assert_eq!(kaputt.limits().max_posts_per_day, 0);
        assert!(kaputt.limits().blocks_everything());
    }

    #[tokio::test]
    async fn tiktok_auto_post_bleibt_ohne_per_clip_consent_aus() {
        let Some(pool) = make_pool("t_sm_posting_plan_tiktok_consent").await else {
            return;
        };
        let schedule = PlatformSchedule {
            platform: "tiktok".to_string(),
            auto_post: true,
            posts_per_week: 4,
            max_posts_per_day: 1,
            post_times: vec!["18:00".to_string()],
        };
        save_platform_schedule(&pool, "nani", &schedule, Some("test"))
            .await
            .unwrap();
        let stored: bool = sqlx::query_scalar(
            "SELECT auto_post FROM social_media_platform_schedule \
             WHERE streamer_login = 'nani' AND platform = 'tiktok'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(!stored);
        assert!(auto_post_platforms(&pool, "nani").await.is_empty());
    }

    #[tokio::test]
    async fn live_aktivierung_staffelt_ueberfaelligen_vorrat_neu() {
        let Some(pool) = make_pool("t_sm_posting_plan_release_replan").await else {
            return;
        };
        sqlx::query(
            "INSERT INTO social_media_streamer_settings \
             (streamer_login, approval_mode, timezone, release_mode) \
             VALUES ('nani', 'manual', 'Europe/Berlin', 'prepare_only')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO social_media_platform_schedule \
             (streamer_login, platform, auto_post, posts_per_week, max_posts_per_day, post_times) \
             VALUES ('nani', 'youtube', TRUE, 2, 1, '[\"18:00\"]'::jsonb)",
        )
        .execute(&pool)
        .await
        .unwrap();
        for index in 0..3 {
            let clip: i64 = sqlx::query_scalar(
                "INSERT INTO twitch_clips_social_media (clip_id, streamer_login) \
                 VALUES ($1, 'nani') RETURNING id",
            )
            .bind(format!("clip-{index}"))
            .fetch_one(&pool)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO twitch_clips_upload_queue \
                 (clip_id, platform, status, scheduled_at) \
                 VALUES ($1, 'youtube', 'pending', NOW() - INTERVAL '14 days')",
            )
            .bind(clip)
            .execute(&pool)
            .await
            .unwrap();
        }
        let before = Utc::now();
        save_streamer_settings(
            &pool,
            "nani",
            &StreamerSettings {
                approval_mode: ApprovalMode::Manual,
                timezone: "Europe/Berlin".into(),
                release_mode: ReleaseMode::Live,
            },
            Some("test"),
        )
        .await
        .unwrap();
        let scheduled: Vec<DateTime<Utc>> = sqlx::query_scalar(
            "SELECT scheduled_at FROM twitch_clips_upload_queue ORDER BY scheduled_at",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(scheduled.len(), 3);
        assert!(scheduled.iter().all(|slot| *slot > before));
        for (index, candidate) in scheduled.iter().enumerate() {
            let same_day = scheduled[..index]
                .iter()
                .filter(|slot| slot.date_naive() == candidate.date_naive())
                .count();
            assert_eq!(same_day, 0, "max_posts_per_day muss erhalten bleiben");
            let in_week = scheduled[..index]
                .iter()
                .filter(|slot| **slot > *candidate - chrono::Duration::days(7))
                .count();
            assert!(in_week < 2, "posts_per_week muss erhalten bleiben");
        }
        let mode: String = sqlx::query_scalar(
            "SELECT release_mode FROM social_media_streamer_settings WHERE streamer_login = 'nani'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(mode, "live");
    }

    async fn wait_for_advisory_waiters(pool: &PgPool, minimum: i64) {
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                let waiting: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*) FROM pg_stat_activity \
                     WHERE datname = current_database() \
                       AND wait_event_type = 'Lock' AND wait_event = 'advisory' \
                       AND query LIKE 'SELECT pg_advisory_xact_lock%'",
                )
                .fetch_one(pool)
                .await
                .unwrap();
                if waiting >= minimum {
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("Provider-/Settings-Transaktion muss sichtbar auf der Release-Sperre warten");
    }

    #[tokio::test]
    async fn providerabschluss_an_abschaltgrenze_bleibt_als_outcome_sichtbar() {
        let Some(pool) = make_pool("t_sm_posting_plan_release_boundary").await else {
            return;
        };
        sqlx::query(
            "INSERT INTO social_media_streamer_settings \
             (streamer_login, approval_mode, timezone, release_mode) \
             VALUES ('nani', 'manual', 'Europe/Berlin', 'live')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let clip_id: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media (clip_id, streamer_login) \
             VALUES ('boundary-clip', 'nani') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let queue_id: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_upload_queue \
             (clip_id, platform, status, provider_started_at, provider_lease_token) \
             VALUES ($1, 'youtube', 'processing', NOW(), 'lease-boundary') RETURNING id",
        )
        .bind(clip_id)
        .fetch_one(&pool)
        .await
        .unwrap();

        // Erst beide echten Produktivpfade hinter derselben Sperre aufreihen.
        // Completion steht bewusst zuerst; die Settings-Transaktion beginnt
        // danach, aber noch bevor der Providerabschluss geschrieben wird.
        let mut blocker = pool.begin().await.unwrap();
        acquire_release_lock(blocker.as_mut(), "nani")
            .await
            .unwrap();
        let baseline: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pg_stat_activity \
             WHERE datname = current_database() \
               AND wait_event_type = 'Lock' AND wait_event = 'advisory' \
               AND query LIKE 'SELECT pg_advisory_xact_lock%'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let completion_pool = pool.clone();
        let completion = tokio::spawn(async move {
            crate::clip_queue::complete_provider_upload(
                &completion_pool,
                queue_id,
                "lease-boundary",
                Some("youtube-boundary-id"),
            )
            .await
        });
        wait_for_advisory_waiters(&pool, baseline + 1).await;
        let settings_pool = pool.clone();
        let disable = tokio::spawn(async move {
            save_streamer_settings(
                &settings_pool,
                "nani",
                &StreamerSettings {
                    approval_mode: ApprovalMode::Manual,
                    timezone: "Europe/Berlin".into(),
                    release_mode: ReleaseMode::PrepareOnly,
                },
                Some("test"),
            )
            .await
        });
        wait_for_advisory_waiters(&pool, baseline + 2).await;
        blocker.rollback().await.unwrap();

        assert!(completion.await.unwrap().unwrap());
        let conflict = disable.await.unwrap().unwrap_err();
        let StreamerSettingsSaveError::ProviderInFlight(outcomes) = conflict else {
            panic!("Abschalten muss den gleichzeitig abgeschlossenen Provider-Versuch melden");
        };
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].queue_id, queue_id);
        assert_eq!(outcomes[0].status, "completed");
        assert_eq!(
            outcomes[0].provider_external_id.as_deref(),
            Some("youtube-boundary-id")
        );
        let mode: String = sqlx::query_scalar(
            "SELECT release_mode FROM social_media_streamer_settings WHERE streamer_login = 'nani'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            mode, "prepare_only",
            "Kill-Switch wird trotz 409 gespeichert"
        );
    }
}
