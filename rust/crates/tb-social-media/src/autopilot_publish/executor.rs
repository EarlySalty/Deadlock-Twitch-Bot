use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Timelike, Utc};
use chrono_tz::Tz;
use serde_json::Value;
use sqlx::{PgConnection, PgPool, Row};

use super::{
    bind_on, binding, connections_on, notify, now_on, queue_status, release_slot,
    valid_approvals_on, AutopilotNotifier, PublishError, AUTOPILOT_APPROVER,
};
use crate::autopilot_settings::{self, AutopilotSettings};
use crate::credentials::CredentialManager;
use crate::produced_video;
use crate::production_jobs::autopilot_schedule;
use crate::uploaders::UploadError;

const TICK: Duration = Duration::from_secs(30);
const MAX_ATTEMPTS: i32 = 4;
const PROMPT_LATENESS_MINUTES: i64 = 15;
const MAX_LATENESS_MINUTES: i64 = 120;
const PREPARING_STALE_MINUTES: i64 = 10;
const EFFECT_STALE_MINUTES: i64 = 120;
const DEFERRAL_MINUTES: i32 = 15;
const NOT_DEFERRED: &str = "NOT COALESCE(v.autopilot_deferred_stage = $1
    AND v.autopilot_deferred_until > clock_timestamp(), FALSE)";

#[derive(Debug, Clone)]
pub struct PublishRequest {
    pub file_path: String,
    pub title: String,
    pub description: String,
    pub hashtags: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct PublishOutcome {
    pub external_id: String,
    pub visibility: Option<String>,
}

#[async_trait]
pub trait DestinationPublisher: Send + Sync {
    async fn publish(
        &self,
        owner: &str,
        platform: &str,
        request: &PublishRequest,
    ) -> Result<PublishOutcome, UploadError>;
}

pub struct PlatformPublisher {
    credentials: CredentialManager,
}

impl PlatformPublisher {
    pub fn new(credentials: CredentialManager) -> Self {
        Self { credentials }
    }
}

#[async_trait]
impl DestinationPublisher for PlatformPublisher {
    async fn publish(
        &self,
        owner: &str,
        platform: &str,
        request: &PublishRequest,
    ) -> Result<PublishOutcome, UploadError> {
        let credentials = self
            .credentials
            .get_channel_credentials_for_id(platform, owner)
            .await
            .ok_or_else(|| UploadError::not_started(UploadError::NotAuthenticated))?;
        let uploader = crate::upload_worker::build_uploader(platform, &credentials)
            .ok_or_else(|| UploadError::not_started(UploadError::NotAuthenticated))?;
        uploader.validate_video(&request.file_path)?;
        let external_id = uploader
            .upload_video(
                &request.file_path,
                &request.title,
                &request.description,
                &request.hashtags,
            )
            .await?;
        let visibility = uploader.uploaded_visibility(&external_id).await;
        Ok(PublishOutcome {
            external_id,
            visibility,
        })
    }
}

pub struct PublishExecutor {
    pool: PgPool,
    publisher: Arc<dyn DestinationPublisher>,
    notifier: Option<Arc<dyn AutopilotNotifier>>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TickReport {
    pub promoted: usize,
    pub executed: usize,
    pub rescheduled: usize,
    pub released: usize,
    pub uploads: usize,
    pub finished: usize,
    pub expired: usize,
    pub cleaned: usize,
}

pub(super) struct Claim {
    queue_id: i64,
    generation: i64,
    video_id: i64,
    owner: String,
    platform: String,
    attempts: i32,
    request: PublishRequest,
    sha256: Option<String>,
    accounts: Value,
}

enum Verdict {
    Upload(Option<String>),
    Hold(&'static str),
}

impl PublishExecutor {
    pub fn new(pool: PgPool, publisher: Arc<dyn DestinationPublisher>) -> Self {
        Self {
            pool,
            publisher,
            notifier: None,
        }
    }

    pub fn with_notifier(mut self, notifier: Arc<dyn AutopilotNotifier>) -> Self {
        self.notifier = Some(notifier);
        self
    }

    pub async fn run(self) {
        loop {
            if let Err(error) = self.tick().await {
                tracing::warn!(%error, "Auto-Pilot publication tick failed");
            }
            tokio::time::sleep(TICK).await;
        }
    }

    pub async fn tick(&self) -> Result<TickReport, PublishError> {
        let mut report = TickReport::default();
        self.recover_stale().await?;
        report.promoted = self.promote().await?;
        report.promoted += self.schedule_slotless().await?;
        self.execute_due(&mut report).await?;
        report.uploads = self.upload_due().await?;
        report.finished = self.finalize().await?;
        report.cleaned = self.cleanup_published().await?;
        report.expired = self.expire_reviews().await?;
        notify::collect(&self.pool).await?;
        if let Some(notifier) = &self.notifier {
            notify::dispatch(&self.pool, notifier.as_ref()).await?;
        }
        Ok(report)
    }

    pub(super) async fn recover_stale(&self) -> Result<(), PublishError> {
        sqlx::query(
            "UPDATE twitch_clips_upload_queue SET status = $1
             WHERE produced_video_id IS NOT NULL AND status = $2
             AND last_attempt_at < clock_timestamp() - make_interval(mins => $3)",
        )
        .bind(queue_status::PENDING)
        .bind(queue_status::PREPARING)
        .bind(PREPARING_STALE_MINUTES as i32)
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "UPDATE twitch_clips_upload_queue SET status = $1, outcome_reason = 'interrupted'
             WHERE produced_video_id IS NOT NULL AND status = $2
             AND effect_started_at < clock_timestamp() - make_interval(mins => $3)",
        )
        .bind(queue_status::UNCERTAIN)
        .bind(queue_status::EFFECT_STARTED)
        .bind(EFFECT_STALE_MINUTES as i32)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn promote(&self) -> Result<usize, PublishError> {
        let candidates: Vec<(i64, String)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT v.id, v.twitch_user_id FROM social_produced_videos v
             JOIN social_production_jobs j ON j.id = v.job_id AND j.source = 'autopilot'
             JOIN social_media_autopilot_settings s ON s.twitch_user_id = v.twitch_user_id
             WHERE s.publish_mode = 'full_auto' AND NOT s.paused AND v.status = 'review'
             AND NOT v.needs_review AND v.review_reasons = '[]'::jsonb
             AND v.expires_at > clock_timestamp() AND v.publication_executed_at IS NULL
             AND v.artifact_deleted_at IS NULL AND v.file_path IS NOT NULL
             AND NOT (v.metadata ? 'manual_review') AND NOT (v.metadata ? 'manual_ai_job_id')
             AND NOT EXISTS (SELECT 1 FROM social_media_clip_approval a WHERE a.produced_video_id = v.id)
             AND {NOT_DEFERRED} ORDER BY v.created_at, v.id LIMIT 20"
        )))
        .bind(Stage::Promote.name())
        .fetch_all(&self.pool)
        .await?;
        let mut promoted = 0;
        for (id, owner) in candidates {
            match self.promote_video(id, &owner).await {
                Ok(true) => promoted += 1,
                Ok(false) => self.defer(id, Stage::Promote).await?,
                Err(error) => {
                    tracing::warn!(%error, video = id, "Auto-Pilot approval could not be prepared");
                    self.defer(id, Stage::Promote).await?;
                }
            }
        }
        Ok(promoted)
    }

    async fn defer(&self, id: i64, stage: Stage) -> Result<(), PublishError> {
        sqlx::query(
            "UPDATE social_produced_videos SET autopilot_deferred_stage = $2,
             autopilot_deferred_until = clock_timestamp() + make_interval(mins => $3) WHERE id = $1",
        )
        .bind(id)
        .bind(stage.name())
        .bind(DEFERRAL_MINUTES)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn promote_video(&self, id: i64, owner: &str) -> Result<bool, PublishError> {
        let settings = autopilot_settings::load(&self.pool, owner).await?;
        let mut connection = self.pool.acquire().await?;
        let accounts = connections_on(&mut connection, owner).await?;
        drop(connection);
        let platforms: Vec<String> = settings
            .target_platforms
            .iter()
            .filter(|platform| accounts.contains_key(platform.as_str()))
            .cloned()
            .collect();
        if platforms.is_empty() {
            return Ok(false);
        }
        let binding = binding(&self.pool, owner, id).await?;
        let mut tx = self.pool.begin().await?;
        autopilot_settings::lock_owner(&mut tx, owner).await?;
        let mode: Option<String> = sqlx::query_scalar(
            "SELECT publish_mode FROM social_media_autopilot_settings WHERE twitch_user_id = $1",
        )
        .bind(owner)
        .fetch_optional(&mut *tx)
        .await?;
        let still_eligible: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM social_produced_videos WHERE id = $1 AND status = 'review'
             AND NOT needs_review AND NOT (metadata ? 'manual_review'))",
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        if mode.as_deref() != Some("full_auto") || !still_eligible {
            return Ok(false);
        }
        bind_on(&mut tx, owner, id, &binding, &platforms).await?;
        for platform in &platforms {
            sqlx::query(
                "INSERT INTO social_media_clip_approval (produced_video_id, platform, state, approved_platforms, approver_user_id, decided_at)
                 VALUES ($1, $2, 'approved', '[]'::jsonb, $3, NOW()) ON CONFLICT (produced_video_id, platform) DO NOTHING",
            )
            .bind(id)
            .bind(platform)
            .bind(AUTOPILOT_APPROVER)
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query("UPDATE social_produced_videos SET status = 'approved' WHERE id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        let now = now_on(&mut tx).await?;
        let slot: Option<DateTime<Utc>> =
            sqlx::query_scalar("SELECT scheduled_at FROM social_produced_videos WHERE id = $1")
                .bind(id)
                .fetch_one(&mut *tx)
                .await?;
        if slot.is_some_and(|slot| slot <= now) {
            release_slot(&mut tx, id).await?;
        }
        tx.commit().await?;
        match produced_video::enqueue_for_publishing_for_user(&self.pool, owner, id, &platforms)
            .await
        {
            Ok(_) | Err(produced_video::ProducedVideoError::NoSlot) => Ok(true),
            Err(error) => Err(error.into()),
        }
    }

    async fn schedule_slotless(&self) -> Result<usize, PublishError> {
        let candidates: Vec<(i64, String, Vec<String>)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT v.id, v.twitch_user_id, array_agg(a.platform ORDER BY a.platform)
             FROM social_produced_videos v
             JOIN social_media_clip_approval a ON a.produced_video_id = v.id AND a.state = 'approved'
             LEFT JOIN social_media_autopilot_settings s ON s.twitch_user_id = v.twitch_user_id
             WHERE v.status = 'approved' AND v.scheduled_at IS NULL AND v.publication_executed_at IS NULL
             AND v.artifact_deleted_at IS NULL AND v.file_path IS NOT NULL
             AND v.metadata ? 'publication' AND NOT COALESCE(s.paused, FALSE) AND {NOT_DEFERRED}
             GROUP BY v.id, v.twitch_user_id ORDER BY v.id LIMIT 20"
        )))
        .bind(Stage::Schedule.name())
        .fetch_all(&self.pool)
        .await?;
        let mut scheduled = 0;
        for (id, owner, platforms) in candidates {
            match super::schedule(&self.pool, &owner, id, &platforms).await {
                Ok(()) => scheduled += 1,
                Err(PublishError::NoSlot) | Err(PublishError::ApprovalRequired) => {
                    self.defer(id, Stage::Schedule).await?
                }
                Err(error) => {
                    tracing::warn!(%error, video = id, "Auto-Pilot publication could not be rescheduled");
                    self.defer(id, Stage::Schedule).await?;
                }
            }
        }
        Ok(scheduled)
    }

    async fn execute_due(&self, report: &mut TickReport) -> Result<(), PublishError> {
        let due: Vec<(i64, String)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT v.id, v.twitch_user_id FROM social_produced_videos v
             LEFT JOIN social_media_autopilot_settings s ON s.twitch_user_id = v.twitch_user_id
             WHERE v.status IN ('review', 'approved', 'scheduled') AND v.publication_executed_at IS NULL
             AND v.scheduled_at <= clock_timestamp() AND NOT COALESCE(s.paused, FALSE) AND {NOT_DEFERRED}
             ORDER BY v.scheduled_at, v.id LIMIT 20"
        )))
        .bind(Stage::Execute.name())
        .fetch_all(&self.pool)
        .await?;
        for (id, owner) in due {
            if let Err(error) = self.execute_video(id, &owner, report).await {
                tracing::warn!(%error, video = id, "Auto-Pilot publication event failed");
                self.defer(id, Stage::Execute).await?;
            }
        }
        Ok(())
    }

    async fn execute_video(
        &self,
        id: i64,
        owner: &str,
        report: &mut TickReport,
    ) -> Result<(), PublishError> {
        let mut tx = self.pool.begin().await?;
        autopilot_settings::lock_owner(&mut tx, owner).await?;
        let settings = autopilot_settings::load_on(&mut tx, owner).await?;
        let row: Option<(Option<DateTime<Utc>>, String, i64)> = sqlx::query_as(
            "SELECT scheduled_at, status, clip_id FROM social_produced_videos WHERE id = $1
             AND twitch_user_id = $2 AND publication_executed_at IS NULL FOR UPDATE",
        )
        .bind(id)
        .bind(owner)
        .fetch_optional(&mut *tx)
        .await?;
        let Some((Some(slot), status, clip_id)) = row else {
            return Ok(());
        };
        let now = now_on(&mut tx).await?;
        if slot > now || settings.paused {
            return Ok(());
        }
        let approved = valid_approvals_on(&mut tx, id, owner, settings.publish_mode).await?;
        if approved.is_empty() {
            release_slot(&mut tx, id).await?;
            if status != "review" {
                sqlx::query("UPDATE social_produced_videos SET status = 'review' WHERE id = $1 AND status = 'approved'")
                    .bind(id)
                    .execute(&mut *tx)
                    .await?;
            }
            tx.commit().await?;
            report.released += 1;
            return Ok(());
        }
        if !on_time(&settings, slot, now) {
            let taken = autopilot_schedule::taken_on(&mut tx, owner).await?;
            match autopilot_settings::next_shared_slot(now, &taken, &settings)? {
                Some(next) => move_slot(&mut tx, id, next).await?,
                None => release_slot(&mut tx, id).await?,
            }
            tx.commit().await?;
            report.rescheduled += 1;
            return Ok(());
        }
        let existing: Vec<String> = sqlx::query_scalar(
            "SELECT platform FROM twitch_clips_upload_queue WHERE produced_video_id = $1",
        )
        .bind(id)
        .fetch_all(&mut *tx)
        .await?;
        let existing: BTreeSet<String> = existing.into_iter().collect();
        let metadata: String =
            sqlx::query_scalar("SELECT metadata::text FROM social_produced_videos WHERE id = $1")
                .bind(id)
                .fetch_one(&mut *tx)
                .await?;
        let event = EventRef {
            clip_id,
            video_id: id,
            slot,
            metadata: &metadata,
        };
        for platform in approved
            .iter()
            .filter(|platform| !existing.contains(*platform))
        {
            insert_destination(&mut tx, &event, platform, queue_status::PENDING, None).await?;
        }
        sqlx::query(
            "UPDATE twitch_clips_upload_queue SET status = $2, outcome_reason = 'not_approved_at_execution'
             WHERE produced_video_id = $1 AND status = $3 AND NOT (platform = ANY($4))",
        )
        .bind(id)
        .bind(queue_status::DISCARDED)
        .bind(queue_status::PENDING)
        .bind(approved.iter().cloned().collect::<Vec<_>>())
        .execute(&mut *tx)
        .await?;
        let mut discarded: BTreeSet<String> = settings.target_platforms.iter().cloned().collect();
        let skipped: Vec<String> = sqlx::query_scalar(
            "SELECT platform FROM social_media_clip_approval WHERE produced_video_id = $1",
        )
        .bind(id)
        .fetch_all(&mut *tx)
        .await?;
        discarded.extend(skipped);
        for platform in discarded
            .iter()
            .filter(|platform| !approved.contains(*platform) && !existing.contains(*platform))
        {
            insert_destination(
                &mut tx,
                &event,
                platform,
                queue_status::DISCARDED,
                Some("not_approved_at_execution"),
            )
            .await?;
        }
        sqlx::query(
            "UPDATE social_produced_videos SET publication_executed_at = $2,
             status = CASE WHEN status = 'review' THEN 'approved' ELSE status END WHERE id = $1",
        )
        .bind(id)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        report.executed += 1;
        Ok(())
    }

    async fn upload_due(&self) -> Result<usize, PublishError> {
        let mut uploads = 0;
        for _ in 0..10 {
            let Some(claim) = self.claim().await? else {
                break;
            };
            self.process(claim).await?;
            uploads += 1;
        }
        Ok(uploads)
    }

    pub(super) async fn claim(&self) -> Result<Option<Claim>, PublishError> {
        let row = sqlx::query(
            "UPDATE twitch_clips_upload_queue q SET status = $1, last_attempt_at = clock_timestamp(),
                 claim_generation = q.claim_generation + 1
             FROM social_produced_videos v, twitch_clips_social_media c
             WHERE q.id = (SELECT q2.id FROM twitch_clips_upload_queue q2
                 JOIN social_produced_videos v2 ON v2.id = q2.produced_video_id
                 LEFT JOIN social_media_autopilot_settings s ON s.twitch_user_id = v2.twitch_user_id
                 WHERE v2.publication_executed_at IS NOT NULL AND NOT COALESCE(s.paused, FALSE)
                 AND v2.artifact_deleted_at IS NULL AND v2.file_path IS NOT NULL
                 AND (q2.status = $2 OR (q2.status = $3 AND q2.next_retry_at <= clock_timestamp()))
                 ORDER BY q2.scheduled_at, q2.id LIMIT 1 FOR UPDATE OF q2 SKIP LOCKED)
             AND v.id = q.produced_video_id AND c.id = q.clip_id
             RETURNING q.id::bigint AS queue_id, q.claim_generation, v.id AS video_id, v.twitch_user_id,
                 q.platform, q.attempts, v.file_path, q.title, q.description, q.hashtags, c.clip_title,
                 v.metadata#>>ARRAY['publication', 'hashes', q.platform] AS sha256,
                 COALESCE(v.metadata#>'{publication,accounts}', '{}'::jsonb)::text AS accounts",
        )
        .bind(queue_status::PREPARING)
        .bind(queue_status::PENDING)
        .bind(queue_status::RETRY)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let clip_title: Option<String> = row.try_get("clip_title")?;
        let title: Option<String> = row.try_get("title")?;
        let hashtags: Option<String> = row.try_get("hashtags")?;
        Ok(Some(Claim {
            queue_id: row.try_get("queue_id")?,
            generation: row.try_get("claim_generation")?,
            video_id: row.try_get("video_id")?,
            owner: row.try_get("twitch_user_id")?,
            platform: row.try_get("platform")?,
            attempts: row.try_get::<Option<i32>, _>("attempts")?.unwrap_or(0),
            request: PublishRequest {
                file_path: row
                    .try_get::<Option<String>, _>("file_path")?
                    .unwrap_or_default(),
                title: title
                    .filter(|value| !value.trim().is_empty())
                    .or(clip_title)
                    .unwrap_or_default(),
                description: row
                    .try_get::<Option<String>, _>("description")?
                    .unwrap_or_default(),
                hashtags: hashtags
                    .and_then(|raw| serde_json::from_str::<Vec<String>>(&raw).ok())
                    .unwrap_or_default(),
            },
            sha256: row.try_get("sha256")?,
            accounts: serde_json::from_str(&row.try_get::<String, _>("accounts")?)?,
        }))
    }

    async fn verdict(&self, claim: &Claim) -> Result<Verdict, PublishError> {
        let settings = autopilot_settings::load(&self.pool, &claim.owner).await?;
        let mut connection = self.pool.acquire().await?;
        let approved = valid_approvals_on(
            &mut connection,
            claim.video_id,
            &claim.owner,
            settings.publish_mode,
        )
        .await?;
        if !approved.contains(&claim.platform) {
            return Ok(Verdict::Hold("review_required"));
        }
        if claim.platform == "tiktok" {
            return Ok(Verdict::Hold("tiktok_effect_boundary_pending"));
        }
        let accounts = connections_on(&mut connection, &claim.owner).await?;
        let Some(current) = accounts.get(&claim.platform) else {
            return Ok(Verdict::Hold("account_missing"));
        };
        let Some(bound) = claim.accounts.get(&claim.platform) else {
            return Ok(Verdict::Hold("account_changed"));
        };
        if bound.as_str() != current.as_deref() {
            return Ok(Verdict::Hold("account_changed"));
        }
        let Some(expected) = claim.sha256.as_deref() else {
            return Ok(Verdict::Hold("exact_file_changed"));
        };
        match crate::uploaders::tiktok::video_digest(&claim.request.file_path).await {
            Ok(actual) if actual == expected => Ok(Verdict::Upload(current.clone())),
            _ => Ok(Verdict::Hold("exact_file_changed")),
        }
    }

    pub(super) async fn process(&self, claim: Claim) -> Result<(), PublishError> {
        let account = match self.verdict(&claim).await? {
            Verdict::Hold(reason) => {
                self.settle(&claim, queue_status::HELD, Some(reason), None)
                    .await?;
                return Ok(());
            }
            Verdict::Upload(account) => account,
        };
        if !self.start_effect(&claim, account.as_deref()).await? {
            return Ok(());
        }
        let attempts = claim.attempts.saturating_add(1);
        match self
            .publisher
            .publish(&claim.owner, &claim.platform, &claim.request)
            .await
        {
            Ok(outcome) => self.complete(&claim, &outcome).await,
            Err(UploadError::Request(_)) => {
                self.settle(
                    &claim,
                    queue_status::UNCERTAIN,
                    Some("request_uncertain"),
                    None,
                )
                .await
            }
            Err(UploadError::Validation(_)) | Err(UploadError::NotImplemented(_)) => {
                self.settle(
                    &claim,
                    queue_status::FAILED,
                    Some("validation_failed"),
                    None,
                )
                .await
            }
            Err(error) => {
                let reason = match &error {
                    UploadError::QuotaExceeded(_) => "quota_exceeded",
                    UploadError::NotAuthenticated => "not_authenticated",
                    UploadError::NotStarted(_) => "not_started",
                    _ => "platform_error",
                };
                if attempts >= MAX_ATTEMPTS {
                    self.settle(&claim, queue_status::FAILED, Some(reason), None)
                        .await
                } else {
                    let delay =
                        chrono::Duration::minutes(15 * (1_i64 << (attempts - 1).clamp(0, 6)));
                    self.settle(&claim, queue_status::RETRY, Some(reason), Some(delay))
                        .await
                }
            }
        }
    }

    pub(super) async fn start_effect(
        &self,
        claim: &Claim,
        account: Option<&str>,
    ) -> Result<bool, PublishError> {
        let mut tx = self.pool.begin().await?;
        autopilot_settings::lock_owner(&mut tx, &claim.owner).await?;
        let settings = autopilot_settings::load_on(&mut tx, &claim.owner).await?;
        let approved =
            valid_approvals_on(&mut tx, claim.video_id, &claim.owner, settings.publish_mode)
                .await?;
        let (status, reason) = if approved.contains(&claim.platform) {
            (queue_status::EFFECT_STARTED, None)
        } else {
            (queue_status::HELD, Some("review_required"))
        };
        let changed = sqlx::query(
            "UPDATE twitch_clips_upload_queue SET status = $2,
             outcome_reason = CASE WHEN $2 = $6 THEN outcome_reason ELSE $5 END,
             effect_started_at = CASE WHEN $2 = $6 THEN clock_timestamp() ELSE effect_started_at END,
             attempts = CASE WHEN $2 = $6 THEN attempts + 1 ELSE attempts END,
             account_id = CASE WHEN $2 = $6 THEN $3 ELSE account_id END,
             next_retry_at = NULL WHERE id = $1 AND status = $4 AND claim_generation = $7",
        )
        .bind(claim.queue_id)
        .bind(status)
        .bind(account)
        .bind(queue_status::PREPARING)
        .bind(reason)
        .bind(queue_status::EFFECT_STARTED)
        .bind(claim.generation)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        tx.commit().await?;
        Ok(changed == 1 && reason.is_none())
    }

    pub(super) async fn settle(
        &self,
        claim: &Claim,
        status: &str,
        reason: Option<&str>,
        delay: Option<chrono::Duration>,
    ) -> Result<(), PublishError> {
        sqlx::query(
            "UPDATE twitch_clips_upload_queue SET status = $2, outcome_reason = $3,
             next_retry_at = CASE WHEN $4::bigint IS NULL THEN NULL ELSE clock_timestamp() + make_interval(secs => $4::bigint::double precision) END,
             last_error = $3 WHERE id = $1 AND claim_generation = $5 AND status = ANY($6)",
        )
        .bind(claim.queue_id)
        .bind(status)
        .bind(reason)
        .bind(delay.map(|delay| delay.num_seconds()))
        .bind(claim.generation)
        .bind(claimed_states(status))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub(super) async fn complete(
        &self,
        claim: &Claim,
        outcome: &PublishOutcome,
    ) -> Result<(), PublishError> {
        let mut tx = self.pool.begin().await?;
        let changed = sqlx::query(
            "UPDATE twitch_clips_upload_queue SET status = $2, external_id = $3, completed_at = clock_timestamp(),
             outcome_reason = NULL, last_error = NULL, next_retry_at = NULL,
             youtube_visibility = CASE WHEN platform = 'youtube' THEN COALESCE($4, youtube_visibility) ELSE youtube_visibility END
             WHERE id = $1 AND claim_generation = $5 AND status = ANY($6)",
        )
        .bind(claim.queue_id)
        .bind(queue_status::PUBLISHED)
        .bind(&outcome.external_id)
        .bind(&outcome.visibility)
        .bind(claim.generation)
        .bind(claimed_states(queue_status::PUBLISHED))
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if changed != 1 {
            tx.commit().await?;
            return Ok(());
        }
        let clip_sql = match claim.platform.as_str() {
            "youtube" => Some("UPDATE twitch_clips_social_media c SET uploaded_youtube = TRUE, youtube_video_id = $1, youtube_uploaded_at = clock_timestamp() FROM twitch_clips_upload_queue q WHERE q.id = $2 AND c.id = q.clip_id"),
            "instagram" => Some("UPDATE twitch_clips_social_media c SET uploaded_instagram = TRUE, instagram_media_id = $1, instagram_uploaded_at = clock_timestamp() FROM twitch_clips_upload_queue q WHERE q.id = $2 AND c.id = q.clip_id"),
            "tiktok" => Some("UPDATE twitch_clips_social_media c SET uploaded_tiktok = TRUE, tiktok_video_id = $1, tiktok_uploaded_at = clock_timestamp() FROM twitch_clips_upload_queue q WHERE q.id = $2 AND c.id = q.clip_id"),
            _ => None,
        };
        if let Some(sql) = clip_sql {
            sqlx::query(sql)
                .bind(&outcome.external_id)
                .bind(claim.queue_id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    async fn finalize(&self) -> Result<usize, PublishError> {
        let finished = sqlx::query(
            "UPDATE social_produced_videos v SET publication_completed_at = clock_timestamp(),
             status = CASE WHEN EXISTS (SELECT 1 FROM twitch_clips_upload_queue q WHERE q.produced_video_id = v.id AND q.status = $1)
                 THEN 'published' ELSE v.status END
             WHERE v.publication_executed_at IS NOT NULL AND v.publication_completed_at IS NULL
             AND NOT EXISTS (SELECT 1 FROM twitch_clips_upload_queue q WHERE q.produced_video_id = v.id
                 AND q.status NOT IN ($1, $2, $3))",
        )
        .bind(queue_status::PUBLISHED)
        .bind(queue_status::DISCARDED)
        .bind(queue_status::FAILED)
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(usize::try_from(finished).unwrap_or(usize::MAX))
    }

    async fn cleanup_published(&self) -> Result<usize, PublishError> {
        let rows: Vec<(i64, String)> = sqlx::query_as(
            "SELECT v.id, v.file_path FROM social_produced_videos v
             WHERE v.file_path IS NOT NULL AND v.artifact_deleted_at IS NULL AND v.cleanup_error IS NULL
             AND v.publication_executed_at IS NOT NULL
             AND NOT EXISTS (SELECT 1 FROM twitch_clips_upload_queue q WHERE q.produced_video_id = v.id
                 AND q.status IS DISTINCT FROM $1 AND q.status IS DISTINCT FROM $2)
             LIMIT 20",
        )
        .bind(queue_status::PUBLISHED)
        .bind(queue_status::DISCARDED)
        .fetch_all(&self.pool)
        .await?;
        let mut cleaned = 0;
        for (id, path) in rows {
            if self.delete_artifact(id, &path).await? {
                cleaned += 1;
            }
        }
        Ok(cleaned)
    }

    async fn expire_reviews(&self) -> Result<usize, PublishError> {
        let rows: Vec<(i64, String, Option<String>)> = sqlx::query_as(
            "SELECT id, twitch_user_id, file_path FROM social_produced_videos
             WHERE status = 'review' AND expires_at <= clock_timestamp() AND publication_executed_at IS NULL
             AND NOT (metadata ? 'manual_ai_job_id') AND NOT (metadata ? 'production_ai_job_id')
             ORDER BY expires_at, id LIMIT 20",
        )
        .fetch_all(&self.pool)
        .await?;
        let mut expired = 0;
        for (id, owner, path) in rows {
            let mut tx = self.pool.begin().await?;
            autopilot_settings::lock_owner(&mut tx, &owner).await?;
            sqlx::query("DELETE FROM twitch_clips_upload_queue WHERE produced_video_id = $1 AND status = $2")
                .bind(id)
                .bind(queue_status::PENDING)
                .execute(&mut *tx)
                .await?;
            let changed = sqlx::query(
                "UPDATE social_produced_videos SET status = 'expired', scheduled_at = NULL
                 WHERE id = $1 AND status = 'review' AND expires_at <= clock_timestamp()",
            )
            .bind(id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
            tx.commit().await?;
            if changed != 1 {
                continue;
            }
            expired += 1;
            if let Some(path) = path {
                self.delete_artifact(id, &path).await?;
            }
        }
        Ok(expired)
    }

    async fn delete_artifact(&self, id: i64, path: &str) -> Result<bool, PublishError> {
        let result = match tokio::fs::remove_file(path).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        };
        match result {
            Ok(()) => {
                sqlx::query("UPDATE social_produced_videos SET artifact_deleted_at = clock_timestamp(), cleanup_error = NULL WHERE id = $1")
                    .bind(id)
                    .execute(&self.pool)
                    .await?;
                Ok(true)
            }
            Err(error) => {
                tracing::warn!(%error, video = id, "Auto-Pilot video file could not be removed");
                sqlx::query("UPDATE social_produced_videos SET cleanup_error = 'Die Videodatei konnte nicht gelöscht werden. Wir versuchen es nicht automatisch erneut.' WHERE id = $1")
                    .bind(id)
                    .execute(&self.pool)
                    .await?;
                Ok(false)
            }
        }
    }
}

#[derive(Clone, Copy)]
enum Stage {
    Promote,
    Schedule,
    Execute,
}

impl Stage {
    fn name(self) -> &'static str {
        match self {
            Self::Promote => "promote",
            Self::Schedule => "schedule",
            Self::Execute => "execute",
        }
    }
}

fn claimed_states(target: &str) -> Vec<&'static str> {
    if target == queue_status::HELD {
        vec![queue_status::PREPARING]
    } else {
        vec![queue_status::EFFECT_STARTED, queue_status::UNCERTAIN]
    }
}

fn on_time(settings: &AutopilotSettings, slot: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    let late = now - slot;
    if late <= chrono::Duration::minutes(PROMPT_LATENESS_MINUTES) {
        return true;
    }
    if late > chrono::Duration::minutes(MAX_LATENESS_MINUTES) {
        return false;
    }
    let hour = settings
        .timezone
        .parse::<Tz>()
        .map(|zone| now.with_timezone(&zone).hour())
        .unwrap_or_else(|_| now.hour());
    (7..23).contains(&hour)
}

async fn move_slot(tx: &mut PgConnection, id: i64, next: DateTime<Utc>) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE social_produced_videos SET scheduled_at = $2 WHERE id = $1")
        .bind(id)
        .bind(next)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "UPDATE twitch_clips_upload_queue SET scheduled_at = $2 WHERE produced_video_id = $1",
    )
    .bind(id)
    .bind(next)
    .execute(&mut *tx)
    .await?;
    Ok(())
}

struct EventRef<'a> {
    clip_id: i64,
    video_id: i64,
    slot: DateTime<Utc>,
    metadata: &'a str,
}

async fn insert_destination(
    tx: &mut PgConnection,
    event: &EventRef<'_>,
    platform: &str,
    status: &str,
    reason: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO twitch_clips_upload_queue (clip_id, produced_video_id, platform, status, scheduled_at,
             outcome_reason, title, description, hashtags)
         VALUES ($1, $2, $3, $4, $5, $6, $7::text::jsonb->'platforms'->$3->>'title',
             $7::text::jsonb->'platforms'->$3->>'description', ($7::text::jsonb->'platforms'->$3->'hashtags')::text)
         ON CONFLICT DO NOTHING",
    )
    .bind(event.clip_id)
    .bind(event.video_id)
    .bind(platform)
    .bind(status)
    .bind(event.slot)
    .bind(reason)
    .bind(event.metadata)
    .execute(tx)
    .await?;
    Ok(())
}
