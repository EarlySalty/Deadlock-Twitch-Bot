use async_trait::async_trait;
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use sqlx::{PgConnection, PgPool};

use super::{platform_name, queue_status, PublishError, DASHBOARD_LINK};
use crate::autopilot_settings::{self, AutopilotSettings, NotificationPreferences};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscordDelivery {
    Sent,
    Failed,
    Unavailable,
}

#[async_trait]
pub trait AutopilotNotifier: Send + Sync {
    async fn send(&self, twitch_user_id: &str, content: &str) -> DiscordDelivery;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    ReadyForReview,
    PublicationDeadline,
    ProcessingFailed,
    PublicationFailed,
    EmptyPool,
    ReviewExpiring,
}

impl Kind {
    fn code(self) -> &'static str {
        match self {
            Self::ReadyForReview => "ready_for_review",
            Self::PublicationDeadline => "publication_deadline",
            Self::ProcessingFailed => "processing_failed",
            Self::PublicationFailed => "publication_failed",
            Self::EmptyPool => "empty_pool",
            Self::ReviewExpiring => "review_expiring",
        }
    }

    fn enabled(self, preferences: &NotificationPreferences) -> bool {
        match self {
            Self::ReadyForReview => preferences.ready_for_review,
            Self::PublicationDeadline => preferences.publication_deadline,
            Self::ProcessingFailed => preferences.processing_failed,
            Self::PublicationFailed => preferences.publication_failed,
            Self::EmptyPool => preferences.empty_pool,
            Self::ReviewExpiring => preferences.review_expiring,
        }
    }
}

struct Owner<'a> {
    id: &'a str,
    settings: &'a AutopilotSettings,
}

impl Owner<'_> {
    fn local(&self, value: DateTime<Utc>) -> String {
        let format = "%d.%m.%Y um %H:%M Uhr";
        match self.settings.timezone.parse::<Tz>() {
            Ok(zone) => value.with_timezone(&zone).format(format).to_string(),
            Err(_) => value.format(format).to_string(),
        }
    }

    async fn record(
        &self,
        connection: &mut PgConnection,
        kind: Kind,
        key: &str,
        content: &str,
    ) -> Result<(), sqlx::Error> {
        let preferences = &self.settings.notifications;
        if !kind.enabled(preferences) || (!preferences.dashboard && !preferences.discord) {
            return Ok(());
        }
        sqlx::query(
            "INSERT INTO social_autopilot_notifications (twitch_user_id, kind, content, link, dedupe_key, show_in_dashboard, discord_state)
             VALUES ($1, $2, $3, $4, $5, $6, $7) ON CONFLICT (dedupe_key) DO NOTHING",
        )
        .bind(self.id)
        .bind(kind.code())
        .bind(content)
        .bind(DASHBOARD_LINK)
        .bind(format!("{}:{}:{key}", kind.code(), self.id))
        .bind(preferences.dashboard)
        .bind(if preferences.discord { "pending" } else { "disabled" })
        .execute(connection)
        .await?;
        Ok(())
    }
}

pub(crate) async fn collect(pool: &PgPool) -> Result<(), PublishError> {
    let owners: Vec<String> = sqlx::query_scalar(
        "SELECT twitch_user_id FROM social_media_autopilot_settings
         UNION SELECT DISTINCT twitch_user_id FROM social_produced_videos
         WHERE created_at > clock_timestamp() - INTERVAL '14 days'",
    )
    .fetch_all(pool)
    .await?;
    let mut connection = pool.acquire().await?;
    for owner in owners {
        let settings = match autopilot_settings::load_on(&mut connection, &owner).await {
            Ok(settings) => settings,
            Err(error) => {
                tracing::warn!(%error, owner, "Auto-Pilot notification settings are invalid");
                continue;
            }
        };
        collect_owner(
            &mut connection,
            &Owner {
                id: &owner,
                settings: &settings,
            },
        )
        .await?;
    }
    Ok(())
}

async fn collect_owner(
    connection: &mut PgConnection,
    owner: &Owner<'_>,
) -> Result<(), PublishError> {
    let reminder = i32::try_from(owner.settings.notifications.reminder_hours).unwrap_or(24);
    let ready: Vec<i64> = sqlx::query_scalar(
        "SELECT id FROM social_produced_videos WHERE twitch_user_id = $1 AND status = 'review'
         AND needs_review AND artifact_deleted_at IS NULL AND expires_at > clock_timestamp() + INTERVAL '6 days'",
    )
    .bind(owner.id)
    .fetch_all(&mut *connection)
    .await?;
    for id in ready {
        owner
            .record(
                connection,
                Kind::ReadyForReview,
                &id.to_string(),
                "Ein neues Video wartet auf deine Prüfung. Ohne deine Freigabe wird es nicht veröffentlicht.",
            )
            .await?;
    }
    let expiring: Vec<(i64, DateTime<Utc>)> = sqlx::query_as(
        "SELECT id, expires_at FROM social_produced_videos WHERE twitch_user_id = $1 AND status = 'review'
         AND artifact_deleted_at IS NULL AND expires_at > clock_timestamp()
         AND expires_at <= clock_timestamp() + make_interval(hours => $2)",
    )
    .bind(owner.id)
    .bind(reminder)
    .fetch_all(&mut *connection)
    .await?;
    for (id, expires_at) in expiring {
        let content = format!(
            "Ein ungeprüftes Video wird am {} gelöscht, wenn du es bis dahin nicht freigibst.",
            owner.local(expires_at)
        );
        owner
            .record(connection, Kind::ReviewExpiring, &id.to_string(), &content)
            .await?;
    }
    let deadlines: Vec<(i64, DateTime<Utc>)> = sqlx::query_as(
        "SELECT v.id, v.scheduled_at FROM social_produced_videos v WHERE v.twitch_user_id = $1
         AND v.status = 'review' AND v.publication_executed_at IS NULL AND v.artifact_deleted_at IS NULL
         AND v.scheduled_at > clock_timestamp() AND v.scheduled_at <= clock_timestamp() + make_interval(hours => $2)
         AND NOT EXISTS (SELECT 1 FROM social_media_clip_approval a WHERE a.produced_video_id = v.id AND a.state = 'approved')",
    )
    .bind(owner.id)
    .bind(reminder)
    .fetch_all(&mut *connection)
    .await?;
    for (id, slot) in deadlines {
        let content = format!(
            "Für den Termin am {} fehlt noch deine Freigabe. Ohne Freigabe wird dieses Video nicht veröffentlicht.",
            owner.local(slot)
        );
        owner
            .record(
                connection,
                Kind::PublicationDeadline,
                &format!("{id}:{}", slot.timestamp()),
                &content,
            )
            .await?;
    }
    let failed_jobs: Vec<i64> = sqlx::query_scalar(
        "SELECT id FROM social_production_jobs WHERE twitch_user_id = $1 AND status = 'failed'
         AND finished_at > clock_timestamp() - INTERVAL '2 days'",
    )
    .bind(owner.id)
    .fetch_all(&mut *connection)
    .await?;
    for id in failed_jobs {
        owner
            .record(
                connection,
                Kind::ProcessingFailed,
                &format!("job:{id}"),
                "Ein Video konnte nicht erstellt werden. Der Clip wird nicht automatisch erneut verarbeitet.",
            )
            .await?;
    }
    let failed_destinations: Vec<(i64, String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT q.id::bigint, q.platform, q.status, q.outcome_reason FROM twitch_clips_upload_queue q
         JOIN social_produced_videos v ON v.id = q.produced_video_id
         WHERE v.twitch_user_id = $1 AND q.last_attempt_at > clock_timestamp() - INTERVAL '2 days'
         AND (q.status IN ($2, $3) OR (q.status = $4 AND q.outcome_reason IN ('account_changed', 'account_missing', 'exact_file_changed')))",
    )
    .bind(owner.id)
    .bind(queue_status::FAILED)
    .bind(queue_status::UNCERTAIN)
    .bind(queue_status::HELD)
    .fetch_all(&mut *connection)
    .await?;
    for (id, platform, status, reason) in failed_destinations {
        let name = platform_name(&platform);
        let content = match status.as_deref() {
            Some(queue_status::UNCERTAIN) => format!(
                "Der Ausgang der Veröffentlichung auf {name} ist unklar. Bitte prüfe dein Konto. Es gibt keinen automatischen zweiten Upload."
            ),
            Some(queue_status::HELD) => format!(
                "Die Veröffentlichung auf {name} wurde angehalten. Bitte prüfe die Verbindung und gib das Video erneut frei."
            ),
            _ => format!("Die Veröffentlichung auf {name} ist fehlgeschlagen."),
        };
        owner
            .record(
                connection,
                Kind::PublicationFailed,
                &format!(
                    "{id}:{}:{}",
                    status.unwrap_or_default(),
                    reason.unwrap_or_default()
                ),
                &content,
            )
            .await?;
    }
    let empty: Option<DateTime<Utc>> = sqlx::query_scalar(
        "SELECT planned_at FROM social_autopilot_schedule_state WHERE twitch_user_id = $1
         AND production_hold = 'empty_pool' AND planned_at > clock_timestamp() - INTERVAL '1 hour'",
    )
    .bind(owner.id)
    .fetch_optional(&mut *connection)
    .await?;
    if let Some(planned_at) = empty {
        owner
            .record(
                connection,
                Kind::EmptyPool,
                &planned_at.format("%Y-%m-%d").to_string(),
                "Im Clip-Pool sind keine passenden Clips mehr. Der Auto-Pilot produziert erst weiter, wenn neue Clips verfügbar sind.",
            )
            .await?;
    }
    Ok(())
}

pub(crate) async fn dispatch(
    pool: &PgPool,
    notifier: &dyn AutopilotNotifier,
) -> Result<(), PublishError> {
    let pending: Vec<(i64, String, String, String)> = sqlx::query_as(
        "SELECT id, twitch_user_id, content, link FROM social_autopilot_notifications
         WHERE discord_state = 'pending' AND created_at > clock_timestamp() - INTERVAL '1 day'
         ORDER BY created_at, id LIMIT 20",
    )
    .fetch_all(pool)
    .await?;
    for (id, owner, content, link) in pending {
        let claimed = sqlx::query(
            "UPDATE social_autopilot_notifications SET discord_state = 'sending' WHERE id = $1 AND discord_state = 'pending'",
        )
        .bind(id)
        .execute(pool)
        .await?
        .rows_affected();
        if claimed != 1 {
            continue;
        }
        let state = match notifier.send(&owner, &format!("{content}\n{link}")).await {
            DiscordDelivery::Sent => "sent",
            DiscordDelivery::Failed => "failed",
            DiscordDelivery::Unavailable => "unavailable",
        };
        sqlx::query(
            "UPDATE social_autopilot_notifications SET discord_state = $2,
             discord_sent_at = CASE WHEN $2 = 'sent' THEN clock_timestamp() ELSE NULL END WHERE id = $1",
        )
        .bind(id)
        .bind(state)
        .execute(pool)
        .await?;
    }
    Ok(())
}
