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
use sqlx::PgPool;

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
}

impl Default for StreamerSettings {
    fn default() -> Self {
        Self {
            approval_mode: ApprovalMode::Manual,
            timezone: "Europe/Berlin".to_string(),
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
    let login = streamer_login.trim().to_lowercase();
    let row = sqlx::query!(
        "SELECT approval_mode, timezone FROM social_media_streamer_settings WHERE streamer_login = $1",
        login
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    match row {
        Some(row) => StreamerSettings {
            approval_mode: ApprovalMode::parse(&row.approval_mode),
            timezone: row.timezone,
        },
        None => StreamerSettings::default(),
    }
}

/// Setzt Freigabe-Modus und Zeitzone.
pub async fn save_streamer_settings(
    pool: &PgPool,
    streamer_login: &str,
    settings: &StreamerSettings,
    updated_by: Option<&str>,
) -> Result<StreamerSettings, sqlx::Error> {
    let login = streamer_login.trim().to_lowercase();
    let updated_by = updated_by.map(str::trim).filter(|s| !s.is_empty());
    sqlx::query!(
        "INSERT INTO social_media_streamer_settings \
             (streamer_login, approval_mode, timezone, updated_at, updated_by) \
         VALUES ($1, $2, $3, CURRENT_TIMESTAMP, $4) \
         ON CONFLICT (streamer_login) DO UPDATE SET \
             approval_mode = EXCLUDED.approval_mode, \
             timezone = EXCLUDED.timezone, \
             updated_at = CURRENT_TIMESTAMP, \
             updated_by = EXCLUDED.updated_by",
        login,
        settings.approval_mode.as_str(),
        settings.timezone,
        updated_by
    )
    .execute(pool)
    .await?;
    Ok(settings.clone())
}

/// Kadenz aller Plattformen, fehlende Zeilen als Default aufgefuellt.
pub async fn load_platform_schedules(pool: &PgPool, streamer_login: &str) -> Vec<PlatformSchedule> {
    let login = streamer_login.trim().to_lowercase();
    let rows = sqlx::query!(
        "SELECT platform, auto_post, posts_per_week, max_posts_per_day, \
                post_times::text AS \"post_times?\" \
           FROM social_media_platform_schedule WHERE streamer_login = $1",
        login
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    PLATFORMS
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
        .collect()
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
    sqlx::query!(
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
        login,
        schedule.platform,
        schedule.auto_post,
        schedule.posts_per_week,
        schedule.max_posts_per_day,
        times,
        updated_by
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Kategorien samt Schalter des Streamers.
pub async fn load_categories(pool: &PgPool, streamer_login: &str) -> Vec<CategoryOption> {
    let login = streamer_login.trim().to_lowercase();
    sqlx::query!(
        "SELECT k.category_key, k.display_name, k.enrichment_enabled, k.sort_order, \
                COALESCE(s.auto_post, FALSE) AS \"auto_post!\" \
           FROM social_media_category k \
           LEFT JOIN social_media_category_settings s \
                  ON s.category_key = k.category_key AND s.streamer_login = $1 \
          ORDER BY k.sort_order, k.category_key",
        login
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default()
    .into_iter()
    .map(|row| CategoryOption {
        category_key: row.category_key,
        display_name: row.display_name,
        enrichment_enabled: row.enrichment_enabled,
        auto_post: row.auto_post,
        sort_order: row.sort_order,
    })
    .collect()
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
        .filter(|s| s.auto_post && !s.limits().blocks_everything())
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
    /// Fuer diese Plattform gibt es ueberhaupt keinen Zeitplan, etwa weil der
    /// Name nicht zu den bekannten Plattformen gehoert.
    OhnePlan,
}

/// Naechster freier Termin fuer diese Plattform, unter Beachtung der Kadenz und
/// der schon eingeplanten Uploads.
pub async fn plan_next_slot(
    pool: &PgPool,
    streamer_login: &str,
    platform: &str,
    now: DateTime<Utc>,
) -> SlotPlan {
    let login = streamer_login.trim().to_lowercase();
    let Some(schedule) = load_platform_schedules(pool, &login)
        .await
        .into_iter()
        .find(|s| s.platform == platform)
    else {
        return SlotPlan::OhnePlan;
    };
    let limits = schedule.limits();
    if limits.blocks_everything() {
        return SlotPlan::Ausgeschaltet;
    }
    let settings = load_streamer_settings(pool, &login).await;
    let taken = belegte_termine(pool, &login, platform).await;
    match next_cadence_slot(
        now,
        &taken,
        &schedule.posting_schedule(&settings.timezone),
        &limits,
    ) {
        Some(termin) => SlotPlan::Termin(termin),
        None => SlotPlan::HorizontVoll,
    }
}

/// Schon vergebene Termine dieser Plattform: eingeplante und erledigte Uploads
/// im relevanten Zeitfenster.
async fn belegte_termine(
    pool: &PgPool,
    streamer_login: &str,
    platform: &str,
) -> Vec<DateTime<Utc>> {
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
    .unwrap_or_default()
}

/// Zaehlt die Clips, die noch fuer Posts zur Verfuegung stehen: nicht verworfen,
/// nicht schon ueberall veroeffentlicht, und in einer eingeschalteten Kategorie.
pub async fn verfuegbare_clips(pool: &PgPool, streamer_login: &str) -> i64 {
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
    .unwrap_or(0)
}

/// Vorratsrechnung fuer das Dashboard.
pub async fn pool_forecast(pool: &PgPool, streamer_login: &str) -> PoolForecast {
    let clips = verfuegbare_clips(pool, streamer_login).await;
    let schedules = load_platform_schedules(pool, streamer_login).await;
    berechne_vorrat(clips, &schedules)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
