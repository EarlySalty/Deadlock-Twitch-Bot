use std::collections::{BTreeSet, HashMap};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{PgConnection, PgPool, Row};

use crate::autopilot_settings::{self, NotificationPreferences, PublishMode, SettingsError};
use crate::produced_video::{self, ProducedVideoError};
use crate::production_jobs::autopilot_schedule::{self, ScheduleError, SchedulePatch};
use crate::uploaders::tiktok::TikTokPostOptions;

mod executor;
mod notify;
#[cfg(test)]
mod tests;
mod tiktok_delivery;

pub use executor::{
    DestinationPublisher, PlatformPublisher, PublishExecutor, PublishOutcome, PublishRequest,
};
pub use notify::{AutopilotNotifier, DiscordDelivery};

pub const AUTOPILOT_APPROVER: &str = "autopilot";
pub(crate) const EFFECTIVE_REVIEW: &str = "(CASE WHEN s.review_policy = 'veto_until_deadline'
        THEN 'veto_until_deadline'
    WHEN s.pipeline_mode IS NOT NULL THEN s.review_policy
    WHEN s.publish_mode = 'full_auto' THEN 'off'
    ELSE 'required' END)";
pub const DASHBOARD_LINK: &str =
    "https://deutsche-deadlock-community.de/twitch/social-media?view=plan";
const RECENT_FINISHED_VIDEOS: i64 = 50;
const RETRY_BLOCKING_REASONS: [&str; 11] = [
    "account_changed",
    "account_missing",
    "exact_file_changed",
    "tiktok_effect_boundary_pending",
    "tiktok_consent_missing",
    "tiktok_caption_changed",
    "tiktok_options_invalid",
    "tiktok_file_invalid",
    "tiktok_music_consent_missing",
    "tiktok_already_published",
    "tiktok_transfer_active",
];
const REAPPROVAL_REASONS: [&str; 11] = [
    "account_changed",
    "account_missing",
    "exact_file_changed",
    "review_required",
    "tiktok_effect_boundary_pending",
    "tiktok_consent_missing",
    "tiktok_caption_changed",
    "tiktok_options_invalid",
    "tiktok_file_invalid",
    "tiktok_music_consent_missing",
    "tiktok_transfer_active",
];
const TIKTOK_PRIVACY_LEVELS: [&str; 4] = [
    "PUBLIC_TO_EVERYONE",
    "MUTUAL_FOLLOW_FRIENDS",
    "FOLLOWER_OF_CREATOR",
    "SELF_ONLY",
];

pub(crate) mod queue_status {
    pub const PENDING: &str = "waiting_exact_file";
    pub const PREPARING: &str = "autopilot_preparing";
    pub const EFFECT_STARTED: &str = "autopilot_effect_started";
    pub const PUBLISHED: &str = "completed";
    pub const RETRY: &str = "autopilot_retry";
    pub const HELD: &str = "autopilot_held";
    pub const PROCESSING: &str = "autopilot_processing";
    pub const UNCERTAIN: &str = "autopilot_uncertain";
    pub const DISCARDED: &str = "autopilot_discarded";
    pub const FAILED: &str = "autopilot_failed";
}

#[derive(Debug, thiserror::Error)]
pub enum PublishError {
    #[error("Das Video wurde nicht gefunden")]
    NotFound,
    #[error("Das Video ist in diesem Zustand nicht verfügbar")]
    State,
    #[error("Bitte bestätige den Wechsel ausdrücklich")]
    Confirmation,
    #[error("Bitte wähle aus, was mit bereits wartenden Videos passieren soll")]
    PendingChoice,
    #[error("Mindestens eine Zielplattform muss ausgewählt werden")]
    NoPlatforms,
    #[error("{0}")]
    Invalid(String),
    #[error("Für diese Plattform fehlt die Videofreigabe")]
    ApprovalRequired,
    #[error("Dieses Ziel kann nicht erneut versucht werden")]
    RetryBlocked,
    #[error("Die Videodatei wurde bereits gelöscht. Ein neuer Versuch ist nicht möglich")]
    ArtifactDeleted,
    #[error("Im gemeinsamen Zeitplan ist kein freier Termin verfügbar")]
    NoSlot,
    #[error(transparent)]
    Video(#[from] ProducedVideoError),
    #[error(transparent)]
    Settings(#[from] SettingsError),
    #[error(transparent)]
    Schedule(#[from] ScheduleError),
    #[error("Die Veröffentlichungsdaten konnten nicht gespeichert oder geladen werden")]
    Database(#[from] sqlx::Error),
    #[error("Die Veröffentlichungsdaten sind ungültig")]
    Json(#[from] serde_json::Error),
    #[error("Die Videodatei konnte nicht gelesen werden")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PendingHandling {
    KeepReview,
    TransferEligible,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModeTransition {
    pub mode: PublishMode,
    pub confirmed: bool,
    #[serde(default)]
    pub pending: Option<PendingHandling>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DestinationDecision {
    pub platform: String,
    #[serde(default)]
    pub tiktok_options: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewMode {
    Review,
    FullAuto,
    VetoUntilDeadline,
}

#[derive(Debug, Clone, Serialize)]
pub struct SnapshotSettings {
    pub publish_mode: PublishMode,
    pub review_mode: ReviewMode,
    pub paused: bool,
    pub notifications: NotificationPreferences,
}

#[derive(Debug, Clone, Serialize)]
pub struct Connection {
    pub platform: String,
    pub connected: bool,
    pub account_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Approval {
    pub platform: String,
    pub state: String,
    pub approver_user_id: Option<String>,
    pub decided_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Destination {
    pub platform: String,
    pub state: &'static str,
    pub attempts: i32,
    pub next_retry_at: Option<DateTime<Utc>>,
    pub external_id: Option<String>,
    pub reason: Option<String>,
    pub account_id: Option<String>,
    pub visibility: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PublicationEvent {
    pub id: i64,
    pub produced_video_id: i64,
    pub scheduled_at: DateTime<Utc>,
    pub executed_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub destinations: Vec<Destination>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PublishVideo {
    pub id: i64,
    pub clip_id: i64,
    pub version_no: i32,
    pub variant_index: i32,
    pub mode: String,
    pub status: String,
    pub duration_ms: Option<i64>,
    pub expires_at: Option<DateTime<Utc>>,
    pub needs_review: bool,
    pub review_reasons: Vec<String>,
    pub artifact_state: &'static str,
    pub cleanup_error: Option<String>,
    pub approvals: Vec<Approval>,
    pub event: Option<PublicationEvent>,
    pub test_job_id: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TikTokBindingIssue {
    ConsentMissing,
    AccountMissing,
    AccountChanged,
    FileChanged,
    CaptionChanged,
    OptionsInvalid,
    MusicConsentMissing,
}

impl TikTokBindingIssue {
    pub fn reason(self) -> &'static str {
        match self {
            Self::ConsentMissing => "tiktok_consent_missing",
            Self::AccountMissing => "account_missing",
            Self::AccountChanged => "account_changed",
            Self::FileChanged => "exact_file_changed",
            Self::CaptionChanged => "tiktok_caption_changed",
            Self::OptionsInvalid => "tiktok_options_invalid",
            Self::MusicConsentMissing => "tiktok_music_consent_missing",
        }
    }

    pub fn message(self) -> &'static str {
        match self {
            Self::ConsentMissing => "Bitte bestätige die TikTok-Freigabe für genau diese Version.",
            Self::AccountMissing => "TikTok ist nicht verbunden. Bitte verbinde das Konto zuerst.",
            Self::AccountChanged => {
                "Das TikTok-Konto hat sich geändert. Bitte öffne die TikTok-Freigabe erneut."
            }
            Self::FileChanged => {
                "Die Videodatei hat sich geändert. Bitte öffne die TikTok-Freigabe erneut."
            }
            Self::CaptionChanged => {
                "Die TikTok-Beschreibung wurde geändert. Bitte bestätige die Freigabe erneut."
            }
            Self::OptionsInvalid => {
                "Bitte prüfe Sichtbarkeit und Werbekennzeichnung in der TikTok-Freigabe."
            }
            Self::MusicConsentMissing => {
                "Bitte bestätige zuerst einmalig die Musiknutzung für dieses TikTok-Konto."
            }
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TikTokSubject {
    pub video_id: i64,
    pub clip_id: i64,
    #[serde(skip)]
    pub file_path: String,
    pub approved_video_sha256: String,
    pub caption: String,
    pub duration_ms: Option<i64>,
    pub credential_id: Option<i32>,
    pub platform_user_id: Option<String>,
    pub music_consent: bool,
    #[serde(skip)]
    pub bound_options: Option<TikTokPostOptions>,
}

impl TikTokSubject {
    pub fn issue(&self, options: &TikTokPostOptions) -> Option<TikTokBindingIssue> {
        let account = self.platform_user_id.as_deref();
        if !options.consent || options.creator_username.trim().is_empty() {
            Some(TikTokBindingIssue::ConsentMissing)
        } else if self.credential_id.is_none() || account.is_none() {
            Some(TikTokBindingIssue::AccountMissing)
        } else if Some(options.credential_id) != self.credential_id
            || Some(options.platform_user_id.as_str()) != account
        {
            Some(TikTokBindingIssue::AccountChanged)
        } else if options.approved_video_sha256 != self.approved_video_sha256
            || options.video_path != self.file_path
        {
            Some(TikTokBindingIssue::FileChanged)
        } else if options.caption != self.caption {
            Some(TikTokBindingIssue::CaptionChanged)
        } else if !valid_tiktok_choice(options) {
            Some(TikTokBindingIssue::OptionsInvalid)
        } else if !self.music_consent {
            Some(TikTokBindingIssue::MusicConsentMissing)
        } else {
            None
        }
    }

    pub fn binding(&self) -> Result<&TikTokPostOptions, TikTokBindingIssue> {
        let options = self
            .bound_options
            .as_ref()
            .ok_or(TikTokBindingIssue::ConsentMissing)?;
        match self.issue(options) {
            Some(issue) => Err(issue),
            None => Ok(options),
        }
    }
}

fn valid_tiktok_choice(options: &TikTokPostOptions) -> bool {
    TIKTOK_PRIVACY_LEVELS.contains(&options.privacy_level.as_str())
        && options.commercial_content
            == (options.brand_organic_toggle || options.brand_content_toggle)
        && !(options.brand_content_toggle && options.privacy_level == "SELF_ONLY")
}

fn stored_tiktok_options(metadata: &Value) -> Option<TikTokPostOptions> {
    metadata
        .pointer("/publication/tiktok_options")
        .cloned()
        .and_then(|options| serde_json::from_value(options).ok())
}

#[derive(Debug, Clone, Default)]
pub(crate) struct TikTokAccount {
    pub credential_id: Option<i32>,
    pub platform_user_id: Option<String>,
    pub music_consent: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct NotificationView {
    pub id: i64,
    pub kind: String,
    pub content: String,
    pub created_at: DateTime<Utc>,
    pub discord_state: String,
    pub read_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Capabilities {
    pub discord: bool,
    pub email: bool,
    pub tiktok_effect_boundary: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PublishSnapshot {
    pub settings: SnapshotSettings,
    pub target_platforms: Vec<String>,
    pub connections: Vec<Connection>,
    pub videos: Vec<PublishVideo>,
    pub notifications: Vec<NotificationView>,
    pub capabilities: Capabilities,
}

pub(crate) fn destination_state(status: &str) -> &'static str {
    match status {
        queue_status::PREPARING => "preparing",
        queue_status::EFFECT_STARTED => "effect_started",
        queue_status::PUBLISHED => "published",
        queue_status::RETRY => "retry",
        queue_status::HELD => "held",
        queue_status::PROCESSING => "processing",
        queue_status::UNCERTAIN => "uncertain",
        queue_status::DISCARDED => "discarded",
        queue_status::FAILED => "failed",
        _ => "pending",
    }
}

fn artifact_state(
    file_path: Option<&str>,
    deleted: Option<DateTime<Utc>>,
    error: Option<&str>,
) -> &'static str {
    if error.is_some() {
        "error"
    } else if deleted.is_some() {
        "deleted"
    } else if file_path.is_none() {
        "missing"
    } else {
        "present"
    }
}

pub(crate) async fn now_on(connection: &mut PgConnection) -> Result<DateTime<Utc>, sqlx::Error> {
    sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(connection)
        .await
}

pub(crate) async fn connections_on(
    connection: &mut PgConnection,
    owner: &str,
) -> Result<HashMap<String, Option<String>>, sqlx::Error> {
    let rows: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT DISTINCT ON (platform) platform, platform_user_id FROM social_media_platform_auth
         WHERE twitch_user_id = $1 AND enabled = 1
         ORDER BY platform, authorized_at DESC, id DESC",
    )
    .bind(owner)
    .fetch_all(connection)
    .await?;
    Ok(rows.into_iter().collect())
}

pub(crate) async fn valid_approvals_on(
    connection: &mut PgConnection,
    video_id: i64,
    owner: &str,
    mode: ReviewMode,
) -> Result<BTreeSet<String>, sqlx::Error> {
    let rows: Vec<String> = sqlx::query_scalar(
        "SELECT platform FROM social_media_clip_approval WHERE produced_video_id = $1
         AND state = 'approved' AND (approver_user_id = $2 OR ($3 AND approver_user_id = $4))",
    )
    .bind(video_id)
    .bind(owner)
    .bind(mode != ReviewMode::Review)
    .bind(AUTOPILOT_APPROVER)
    .fetch_all(connection)
    .await?;
    Ok(rows.into_iter().collect())
}

pub(crate) async fn review_mode_on(
    connection: &mut PgConnection,
    owner: &str,
) -> Result<ReviewMode, sqlx::Error> {
    let policy: Option<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT {EFFECTIVE_REVIEW} FROM social_media_autopilot_settings s WHERE s.twitch_user_id = $1"
    )))
    .bind(owner)
    .fetch_optional(connection)
    .await?;
    Ok(match policy.as_deref() {
        Some("veto_until_deadline") => ReviewMode::VetoUntilDeadline,
        Some("off") => ReviewMode::FullAuto,
        _ => ReviewMode::Review,
    })
}

pub async fn snapshot(pool: &PgPool, owner: &str) -> Result<PublishSnapshot, PublishError> {
    let mut connection = pool.acquire().await?;
    let settings = autopilot_settings::load_on(&mut connection, owner).await?;
    let accounts = connections_on(&mut connection, owner).await?;
    let connections = ["youtube", "tiktok", "instagram"]
        .into_iter()
        .map(|platform| Connection {
            platform: platform.into(),
            connected: accounts.contains_key(platform),
            account_id: accounts.get(platform).cloned().flatten(),
        })
        .collect();
    let videos = videos_on(&mut connection, owner).await?;
    let notifications = notifications_on(&mut connection, owner).await?;
    let review_mode = review_mode_on(&mut connection, owner).await?;
    let discord: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM twitch_streamer_identities WHERE twitch_user_id = $1
         AND COALESCE(discord_user_id, '') <> '')",
    )
    .bind(owner)
    .fetch_one(&mut *connection)
    .await?;
    Ok(PublishSnapshot {
        target_platforms: settings.target_platforms.clone(),
        settings: SnapshotSettings {
            publish_mode: settings.publish_mode,
            review_mode,
            paused: settings.paused,
            notifications: settings.notifications,
        },
        connections,
        videos,
        notifications,
        capabilities: Capabilities {
            discord,
            email: false,
            tiktok_effect_boundary: false,
        },
    })
}

async fn videos_on(
    connection: &mut PgConnection,
    owner: &str,
) -> Result<Vec<PublishVideo>, PublishError> {
    let rows = sqlx::query(
        "WITH listed AS (
            SELECT v.*, v.status IN ('review', 'approved', 'scheduled') OR EXISTS (
                SELECT 1 FROM twitch_clips_upload_queue q
                WHERE q.produced_video_id = v.id AND q.status = ANY($2)) AS unresolved
            FROM social_produced_videos v WHERE v.twitch_user_id = $1 AND v.status <> 'rendering'
         ), ranked AS (
            SELECT listed.*, ROW_NUMBER() OVER (PARTITION BY unresolved ORDER BY created_at DESC, id DESC) AS position
            FROM listed
         )
         SELECT id, clip_id, version_no, variant_index, mode, status, duration_ms, expires_at,
            needs_review, review_reasons::text AS review_reasons, file_path, artifact_deleted_at,
            cleanup_error, scheduled_at, publication_executed_at, publication_completed_at,
            metadata::text AS metadata,
            (SELECT j.id FROM social_production_jobs j
                WHERE j.id = social_video_job(ranked.job_id, ranked.metadata)
                AND j.source = 'test') AS test_job_id
         FROM ranked WHERE unresolved OR position <= $3
         ORDER BY created_at DESC, id DESC",
    )
    .bind(owner)
    .bind([
        queue_status::PENDING,
        queue_status::PREPARING,
        queue_status::EFFECT_STARTED,
        queue_status::RETRY,
        queue_status::HELD,
        queue_status::PROCESSING,
        queue_status::UNCERTAIN,
        queue_status::FAILED,
    ])
    .bind(RECENT_FINISHED_VIDEOS)
    .fetch_all(&mut *connection)
    .await?;
    let ids: Vec<i64> = rows
        .iter()
        .map(|row| row.try_get("id"))
        .collect::<Result<_, _>>()?;
    let mut approvals: HashMap<i64, Vec<Approval>> = HashMap::new();
    for row in sqlx::query(
        "SELECT produced_video_id, platform, state, approver_user_id, decided_at
         FROM social_media_clip_approval WHERE produced_video_id = ANY($1) ORDER BY platform",
    )
    .bind(&ids)
    .fetch_all(&mut *connection)
    .await?
    {
        approvals
            .entry(row.try_get("produced_video_id")?)
            .or_default()
            .push(Approval {
                platform: row.try_get("platform")?,
                state: row.try_get("state")?,
                approver_user_id: row.try_get("approver_user_id")?,
                decided_at: row.try_get("decided_at")?,
            });
    }
    let mut destinations: HashMap<i64, Vec<Destination>> = HashMap::new();
    for row in sqlx::query(
        "SELECT produced_video_id, platform, status, attempts, next_retry_at, external_id,
            outcome_reason, account_id, youtube_visibility
         FROM twitch_clips_upload_queue WHERE produced_video_id = ANY($1) ORDER BY platform",
    )
    .bind(&ids)
    .fetch_all(&mut *connection)
    .await?
    {
        let status: Option<String> = row.try_get("status")?;
        destinations
            .entry(row.try_get("produced_video_id")?)
            .or_default()
            .push(Destination {
                platform: row.try_get("platform")?,
                state: destination_state(status.as_deref().unwrap_or_default()),
                attempts: row.try_get::<Option<i32>, _>("attempts")?.unwrap_or(0),
                next_retry_at: row.try_get("next_retry_at")?,
                external_id: row.try_get("external_id")?,
                reason: row.try_get("outcome_reason")?,
                account_id: row.try_get("account_id")?,
                visibility: row.try_get("youtube_visibility")?,
            });
    }
    rows.into_iter()
        .map(|row| {
            let id: i64 = row.try_get("id")?;
            let reasons: Value =
                serde_json::from_str(&row.try_get::<String, _>("review_reasons")?)?;
            let file_path: Option<String> = row.try_get("file_path")?;
            let cleanup_error: Option<String> = row.try_get("cleanup_error")?;
            let scheduled_at: Option<DateTime<Utc>> = row.try_get("scheduled_at")?;
            Ok(PublishVideo {
                id,
                clip_id: row.try_get("clip_id")?,
                version_no: row.try_get("version_no")?,
                variant_index: row.try_get("variant_index")?,
                mode: row.try_get("mode")?,
                status: row.try_get("status")?,
                duration_ms: row.try_get("duration_ms")?,
                expires_at: row.try_get("expires_at")?,
                needs_review: row.try_get("needs_review")?,
                review_reasons: reasons
                    .as_array()
                    .map(|values| {
                        values
                            .iter()
                            .filter_map(|value| value.as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default(),
                artifact_state: artifact_state(
                    file_path.as_deref(),
                    row.try_get("artifact_deleted_at")?,
                    cleanup_error.as_deref(),
                ),
                cleanup_error,
                approvals: approvals.remove(&id).unwrap_or_default(),
                event: match scheduled_at {
                    Some(scheduled_at) => Some(PublicationEvent {
                        id,
                        produced_video_id: id,
                        scheduled_at,
                        executed_at: row.try_get("publication_executed_at")?,
                        completed_at: row.try_get("publication_completed_at")?,
                        destinations: destinations.remove(&id).unwrap_or_default(),
                    }),
                    None => None,
                },
                test_job_id: row.try_get("test_job_id")?,
            })
        })
        .collect()
}

async fn notifications_on(
    connection: &mut PgConnection,
    owner: &str,
) -> Result<Vec<NotificationView>, sqlx::Error> {
    sqlx::query(
        "SELECT id, kind, content, created_at, discord_state, read_at FROM social_autopilot_notifications
         WHERE twitch_user_id = $1 AND show_in_dashboard ORDER BY created_at DESC, id DESC LIMIT 20",
    )
    .bind(owner)
    .fetch_all(connection)
    .await?
    .into_iter()
    .map(|row| {
        Ok(NotificationView {
            id: row.try_get("id")?,
            kind: row.try_get("kind")?,
            content: row.try_get("content")?,
            created_at: row.try_get("created_at")?,
            discord_state: row.try_get("discord_state")?,
            read_at: row.try_get("read_at")?,
        })
    })
    .collect()
}

pub async fn transition_mode(
    pool: &PgPool,
    owner: &str,
    transition: &ModeTransition,
) -> Result<(), PublishError> {
    if !transition.confirmed {
        return Err(PublishError::Confirmation);
    }
    let mut tx = pool.begin().await?;
    autopilot_settings::lock_owner(&mut tx, owner).await?;
    let settings = autopilot_settings::load_on(&mut tx, owner).await?;
    if settings.publish_mode == transition.mode {
        tx.commit().await?;
        return Ok(());
    }
    let now = now_on(&mut tx).await?;
    match transition.mode {
        PublishMode::FullAuto => enter_full_auto(&mut tx, owner, transition.pending, now).await?,
        PublishMode::Review => enter_review(&mut tx, owner, now).await?,
    }
    sqlx::query(
        "INSERT INTO social_media_autopilot_settings (twitch_user_id, publish_mode) VALUES ($1, $2)
         ON CONFLICT (twitch_user_id) DO UPDATE SET publish_mode = EXCLUDED.publish_mode, updated_at = NOW()",
    )
    .bind(owner)
    .bind(match transition.mode {
        PublishMode::Review => "review",
        PublishMode::FullAuto => "full_auto",
    })
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

async fn enter_full_auto(
    tx: &mut PgConnection,
    owner: &str,
    pending: Option<PendingHandling>,
    now: DateTime<Utc>,
) -> Result<(), PublishError> {
    match pending.ok_or(PublishError::PendingChoice)? {
        PendingHandling::KeepReview => {
            sqlx::query(
                "UPDATE social_produced_videos SET metadata = metadata || '{\"manual_review\":true}'::jsonb
                 WHERE twitch_user_id = $1 AND status = 'review'",
            )
            .bind(owner)
            .execute(&mut *tx)
            .await?;
        }
        PendingHandling::TransferEligible => {
            sqlx::query(
                "UPDATE social_produced_videos v SET needs_review = FALSE
                 FROM social_production_jobs j
                 WHERE v.twitch_user_id = $1 AND v.status = 'review' AND v.mode = 'static'
                 AND v.expires_at > $2 AND v.review_reasons = '[]'::jsonb
                 AND j.id = v.job_id AND j.source = 'autopilot'
                 AND NOT (v.metadata ? 'manual_ai_job_id') AND NOT (v.metadata ? 'manual_review')",
            )
            .bind(owner)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        }
    }
    Ok(())
}

async fn enter_review(
    tx: &mut PgConnection,
    owner: &str,
    now: DateTime<Utc>,
) -> Result<(), PublishError> {
    let automatic: Vec<i64> = sqlx::query_scalar(
        "SELECT DISTINCT v.id FROM social_produced_videos v JOIN social_media_clip_approval a
         ON a.produced_video_id = v.id WHERE v.twitch_user_id = $1 AND a.approver_user_id = $2",
    )
    .bind(owner)
    .bind(AUTOPILOT_APPROVER)
    .fetch_all(&mut *tx)
    .await?;
    sqlx::query(
        "DELETE FROM social_media_clip_approval a USING social_produced_videos v
         WHERE a.produced_video_id = v.id AND v.twitch_user_id = $1 AND a.approver_user_id = $2",
    )
    .bind(owner)
    .bind(AUTOPILOT_APPROVER)
    .execute(&mut *tx)
    .await?;
    for id in automatic {
        let explicit = valid_approvals_on(tx, id, owner, ReviewMode::Review).await?;
        let explicit: Vec<String> = explicit.into_iter().collect();
        sqlx::query(
            "DELETE FROM twitch_clips_upload_queue q USING social_produced_videos v
             WHERE v.id = q.produced_video_id AND q.produced_video_id = $1 AND q.status = $2
             AND v.publication_executed_at IS NULL AND NOT (q.platform = ANY($3))",
        )
        .bind(id)
        .bind(queue_status::PENDING)
        .bind(&explicit)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE twitch_clips_upload_queue SET status = $2, outcome_reason = 'review_required', next_retry_at = NULL
             WHERE produced_video_id = $1 AND status = ANY($3) AND NOT (platform = ANY($4))",
        )
        .bind(id)
        .bind(queue_status::HELD)
        .bind([queue_status::RETRY, queue_status::PENDING])
        .bind(&explicit)
        .execute(&mut *tx)
        .await?;
        if explicit.is_empty() {
            sqlx::query(
                "UPDATE social_produced_videos SET status = 'review', needs_review = TRUE,
                 expires_at = GREATEST(COALESCE(expires_at, $2), $2 + INTERVAL '7 days')
                 WHERE id = $1 AND status IN ('approved', 'scheduled') AND publication_executed_at IS NULL
                 AND file_path IS NOT NULL AND artifact_deleted_at IS NULL",
            )
            .bind(id)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        }
    }
    sqlx::query(
        "UPDATE social_produced_videos SET needs_review = TRUE
         WHERE twitch_user_id = $1 AND status = 'review' AND NOT needs_review",
    )
    .bind(owner)
    .execute(&mut *tx)
    .await?;
    Ok(())
}

pub async fn set_paused(pool: &PgPool, owner: &str, paused: bool) -> Result<(), PublishError> {
    autopilot_schedule::save_schedule(
        pool,
        owner,
        &SchedulePatch {
            paused: Some(paused),
            ..Default::default()
        },
    )
    .await?;
    Ok(())
}

pub async fn save_notifications(
    pool: &PgPool,
    owner: &str,
    preferences: &NotificationPreferences,
) -> Result<NotificationPreferences, PublishError> {
    if !(1..=168).contains(&preferences.reminder_hours) {
        return Err(PublishError::Invalid(
            "Die Vorwarnung muss zwischen 1 und 168 Stunden liegen".into(),
        ));
    }
    let mut preferences = preferences.clone();
    preferences.email = false;
    sqlx::query(
        "INSERT INTO social_media_autopilot_settings (twitch_user_id, notifications) VALUES ($1, $2::text::jsonb)
         ON CONFLICT (twitch_user_id) DO UPDATE SET notifications = EXCLUDED.notifications, updated_at = NOW()",
    )
    .bind(owner)
    .bind(serde_json::to_string(&preferences)?)
    .execute(pool)
    .await?;
    Ok(preferences)
}

pub(crate) struct Binding {
    path: String,
    digest: String,
}

pub(crate) async fn binding(pool: &PgPool, owner: &str, id: i64) -> Result<Binding, PublishError> {
    let video = produced_video::get(pool, owner, id)
        .await?
        .ok_or(PublishError::NotFound)?;
    let path = video.file_path.ok_or(PublishError::State)?;
    let digest = crate::uploaders::tiktok::video_digest(&path).await?;
    Ok(Binding { path, digest })
}

pub(crate) async fn bind_on(
    connection: &mut PgConnection,
    owner: &str,
    id: i64,
    binding: &Binding,
    platforms: &[String],
) -> Result<(), PublishError> {
    let accounts = connections_on(connection, owner).await?;
    let mut bound = serde_json::Map::new();
    let mut hashes = serde_json::Map::new();
    for platform in platforms {
        let account = accounts.get(platform).ok_or_else(|| {
            PublishError::Invalid(format!(
                "{} ist nicht verbunden. Bitte verbinde das Konto zuerst.",
                platform_name(platform)
            ))
        })?;
        bound.insert(platform.clone(), json!(account));
        hashes.insert(platform.clone(), json!(binding.digest));
    }
    let changed = sqlx::query(
        "UPDATE social_produced_videos SET metadata = jsonb_set(metadata, '{publication}',
            COALESCE(metadata->'publication', '{}'::jsonb) || jsonb_build_object(
                'hashes', COALESCE(metadata#>'{publication,hashes}', '{}'::jsonb) || $3::text::jsonb,
                'accounts', COALESCE(metadata#>'{publication,accounts}', '{}'::jsonb) || $4::text::jsonb))
         WHERE id = $1 AND twitch_user_id = $2 AND file_path = $5 AND artifact_deleted_at IS NULL",
    )
    .bind(id)
    .bind(owner)
    .bind(serde_json::to_string(&hashes)?)
    .bind(serde_json::to_string(&bound)?)
    .bind(&binding.path)
    .execute(&mut *connection)
    .await?
    .rows_affected();
    if changed != 1 {
        return Err(PublishError::State);
    }
    Ok(())
}

pub(crate) fn platform_name(platform: &str) -> &'static str {
    match platform {
        "youtube" => "YouTube",
        "tiktok" => "TikTok",
        "instagram" => "Instagram",
        _ => "Die Plattform",
    }
}

pub async fn approve(
    pool: &PgPool,
    owner: &str,
    id: i64,
    decisions: &[DestinationDecision],
) -> Result<(), PublishError> {
    let selected: Vec<String> = decisions
        .iter()
        .map(|decision| decision.platform.clone())
        .collect();
    let platforms = autopilot_settings::normalize_platforms(&selected)?;
    if platforms.is_empty() {
        return Err(PublishError::NoPlatforms);
    }
    let binding = binding(pool, owner, id).await?;
    let mut tx = pool.begin().await?;
    autopilot_settings::lock_owner(&mut tx, owner).await?;
    produced_video::approve_on(&mut tx, owner, id, &platforms).await?;
    bind_on(&mut tx, owner, id, &binding, &platforms).await?;
    sqlx::query(
        "UPDATE twitch_clips_upload_queue SET status = $3, next_retry_at = clock_timestamp(), outcome_reason = NULL
         WHERE produced_video_id = $1 AND platform = ANY($2) AND status = $4
         AND outcome_reason = ANY($5)",
    )
    .bind(id)
    .bind(&platforms)
    .bind(queue_status::RETRY)
    .bind(queue_status::HELD)
    .bind(REAPPROVAL_REASONS)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    schedule(pool, owner, id, &platforms).await
}

pub async fn tiktok_subject(
    pool: &PgPool,
    owner: &str,
    id: i64,
) -> Result<TikTokSubject, PublishError> {
    let binding = binding(pool, owner, id).await?;
    let mut connection = pool.acquire().await?;
    tiktok_subject_on(&mut connection, owner, id, &binding).await
}

async fn tiktok_subject_on(
    connection: &mut PgConnection,
    owner: &str,
    id: i64,
    binding: &Binding,
) -> Result<TikTokSubject, PublishError> {
    let video: Option<(i64, Option<i64>, String)> = sqlx::query_as(
        "SELECT clip_id, duration_ms, metadata::text FROM social_produced_videos
         WHERE id = $1 AND twitch_user_id = $2 AND file_path = $3 AND artifact_deleted_at IS NULL",
    )
    .bind(id)
    .bind(owner)
    .bind(&binding.path)
    .fetch_optional(&mut *connection)
    .await?;
    let (clip_id, duration_ms, metadata) = video.ok_or(PublishError::State)?;
    let metadata: Value = serde_json::from_str(&metadata)?;
    let account = tiktok_account_on(connection, owner).await?;
    Ok(TikTokSubject {
        video_id: id,
        clip_id,
        file_path: binding.path.clone(),
        approved_video_sha256: binding.digest.clone(),
        caption: crate::ai_editor::metadata::tiktok_caption(&metadata),
        duration_ms,
        credential_id: account.credential_id,
        platform_user_id: account.platform_user_id,
        music_consent: account.music_consent,
        bound_options: stored_tiktok_options(&metadata),
    })
}

pub(crate) async fn tiktok_account_on(
    connection: &mut PgConnection,
    owner: &str,
) -> Result<TikTokAccount, sqlx::Error> {
    let account: Option<(i32, Option<String>)> = sqlx::query_as(
        "SELECT id, platform_user_id FROM social_media_platform_auth
         WHERE twitch_user_id = $1 AND platform = 'tiktok' AND enabled = 1
         ORDER BY authorized_at DESC, id DESC LIMIT 1",
    )
    .bind(owner)
    .fetch_optional(&mut *connection)
    .await?;
    let credential_id = account.as_ref().map(|(credential, _)| *credential);
    let platform_user_id = account
        .and_then(|(_, account)| account)
        .filter(|account| !account.is_empty());
    let music_consent = match platform_user_id.as_deref() {
        Some(account) => music_consent_on(connection, owner, account).await?,
        None => false,
    };
    Ok(TikTokAccount {
        credential_id,
        platform_user_id,
        music_consent,
    })
}

async fn music_consent_on(
    connection: &mut PgConnection,
    owner: &str,
    account: &str,
) -> Result<bool, sqlx::Error> {
    let consent_key = format!("tiktok_music_consent:{owner}:{account}");
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT key, value::text FROM social_media_settings WHERE key = ANY($1)")
            .bind([consent_key.clone(), format!("tiktok_defaults:{owner}")])
            .fetch_all(connection)
            .await?;
    Ok(rows.iter().any(|(key, raw)| {
        let Ok(value) = serde_json::from_str::<Value>(raw) else {
            return false;
        };
        let text = |field: &str| value.get(field).and_then(Value::as_str);
        if *key == consent_key {
            text("platform_user_id") == Some(account) && text("consented_at").is_some()
        } else {
            value.get("music_consent").and_then(Value::as_bool) == Some(true)
                && text("music_consent_at").is_some()
                && text("music_consent_platform_user_id") == Some(account)
        }
    }))
}

type SlotState = (Option<DateTime<Utc>>, Option<DateTime<Utc>>);

pub async fn schedule(
    pool: &PgPool,
    owner: &str,
    id: i64,
    platforms: &[String],
) -> Result<(), PublishError> {
    let platforms = autopilot_settings::normalize_platforms(platforms)?;
    let mut tx = pool.begin().await?;
    autopilot_settings::lock_owner(&mut tx, owner).await?;
    let video: Option<SlotState> = sqlx::query_as(
        "SELECT scheduled_at, publication_executed_at FROM social_produced_videos
         WHERE id = $1 AND twitch_user_id = $2 FOR UPDATE",
    )
    .bind(id)
    .bind(owner)
    .fetch_optional(&mut *tx)
    .await?;
    let (slot, executed) = video.ok_or(PublishError::NotFound)?;
    if executed.is_some() {
        tx.commit().await?;
        return Ok(());
    }
    let mode = review_mode_on(&mut tx, owner).await?;
    let approved = valid_approvals_on(&mut tx, id, owner, mode).await?;
    let platforms: Vec<String> = platforms
        .into_iter()
        .filter(|platform| approved.contains(platform))
        .collect();
    if platforms.is_empty() {
        return Err(PublishError::ApprovalRequired);
    }
    let now = now_on(&mut tx).await?;
    if slot.is_some_and(|slot| slot <= now) {
        release_slot(&mut tx, id).await?;
    }
    tx.commit().await?;
    match produced_video::enqueue_for_publishing_for_user(pool, owner, id, &platforms).await {
        Ok(_) => Ok(()),
        Err(ProducedVideoError::NoSlot) => Err(PublishError::NoSlot),
        Err(error) => Err(error.into()),
    }
}

pub(crate) async fn release_slot(tx: &mut PgConnection, id: i64) -> Result<(), sqlx::Error> {
    sqlx::query(
        "DELETE FROM twitch_clips_upload_queue WHERE produced_video_id = $1 AND status = $2",
    )
    .bind(id)
    .bind(queue_status::PENDING)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE social_produced_videos SET scheduled_at = NULL,
         status = CASE WHEN status = 'scheduled' THEN 'approved' ELSE status END
         WHERE id = $1 AND publication_executed_at IS NULL",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;
    Ok(())
}

pub async fn retry(
    pool: &PgPool,
    owner: &str,
    id: i64,
    platform: &str,
) -> Result<(), PublishError> {
    let mut tx = pool.begin().await?;
    autopilot_settings::lock_owner(&mut tx, owner).await?;
    let row: Option<(i64, Option<String>, Option<String>, bool)> = sqlx::query_as(
        "SELECT q.id::bigint, q.status, q.outcome_reason,
            v.artifact_deleted_at IS NULL AND v.file_path IS NOT NULL
         FROM twitch_clips_upload_queue q
         JOIN social_produced_videos v ON v.id = q.produced_video_id
         WHERE q.produced_video_id = $1 AND q.platform = $2 AND v.twitch_user_id = $3 FOR UPDATE OF q",
    )
    .bind(id)
    .bind(platform)
    .bind(owner)
    .fetch_optional(&mut *tx)
    .await?;
    let (queue_id, status, reason, artifact_present) = row.ok_or(PublishError::NotFound)?;
    let status = status.unwrap_or_default();
    if status != queue_status::RETRY && status != queue_status::HELD {
        return Err(PublishError::RetryBlocked);
    }
    if reason
        .as_deref()
        .is_some_and(|reason| RETRY_BLOCKING_REASONS.contains(&reason))
    {
        return Err(PublishError::RetryBlocked);
    }
    if !artifact_present {
        return Err(PublishError::ArtifactDeleted);
    }
    let mode = review_mode_on(&mut tx, owner).await?;
    if !valid_approvals_on(&mut tx, id, owner, mode)
        .await?
        .contains(platform)
    {
        return Err(PublishError::ApprovalRequired);
    }
    sqlx::query(
        "UPDATE twitch_clips_upload_queue SET status = $2, next_retry_at = clock_timestamp(),
         outcome_reason = 'manual_retry' WHERE id = $1",
    )
    .bind(queue_id)
    .bind(queue_status::RETRY)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}
