use std::collections::{BTreeMap, BTreeSet};

use chrono::NaiveTime;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::{PgConnection, PgPool, Row};

use crate::ai_editor::AiEditorError;
use crate::autopilot_settings::{
    self, AutopilotSettings, DistributionPolicy, MigrationDisposition, PipelineMigrationMapping,
    PipelineMode, PipelinePolicy, PolicyScope, PublishMode, ReviewPolicy, ScheduleSlot,
    SettingsError,
};
use crate::posting_plan::PLATFORMS;
use crate::production_jobs::UNSUITABLE_REASON;

pub const MIGRATION_VERSION: i32 = 1;

pub const APPROVALS: &str = "social_media_clip_approval";
pub const QUEUE: &str = "twitch_clips_upload_queue";
pub const ENRICHMENT: &str = "social_media_clip_enrichment";
pub const CLIPS: &str = "twitch_clips_social_media";
pub const STREAMER_SETTINGS: &str = "social_media_streamer_settings";
pub const SCHEDULES: &str = "social_media_platform_schedule";
pub const CATEGORIES: &str = "social_media_category_settings";
pub const SETTINGS: &str = "social_media_settings";

const LEGACY_RUNNER_FAILURES: [&str; 4] = [
    "static_production_failed",
    "resource_unavailable",
    "ai_editor_unavailable",
    "ai_production_failed",
];

const FINAL_QUEUE: [&str; 5] = [
    "completed",
    "failed",
    "cancelled",
    "autopilot_discarded",
    "autopilot_failed",
];
const LIVE_QUEUE: [&str; 4] = [
    "pending",
    "waiting_connection",
    "waiting_schedule",
    "waiting_tiktok_approval",
];
const INBOX_QUEUE: [&str; 2] = ["inbox", "inbox_pending"];
const DRAFT_FIELDS: [&str; 12] = [
    "caption",
    "standard_description",
    "privacy_level",
    "allow_comment",
    "allow_duet",
    "allow_stitch",
    "commercial_content",
    "brand_organic_toggle",
    "brand_content_toggle",
    "music_consent",
    "music_consent_at",
    "music_consent_platform_user_id",
];

#[derive(Debug, thiserror::Error)]
pub enum MigrationError {
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Settings(#[from] SettingsError),
    #[error("{0}")]
    Schedule(String),
    #[error("Schreibsperre der Umstellung fehlt, Migration 20261016120000 zuerst anwenden")]
    BarrierMissing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CutoverOutcome {
    Activated,
    AlreadyClosed,
    NotShadow,
    NotReady,
    Paused,
    Resumed,
    NotUnified,
    NotPaused,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CutoverReport {
    pub twitch_user_id: String,
    pub outcome: CutoverOutcome,
    pub unresolved: usize,
    pub autopilot_enabled: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct ProposedPolicy {
    policy: PipelinePolicy,
    timezone: Option<String>,
    target_platforms: Vec<String>,
    schedule_slots: Vec<ScheduleSlot>,
}

#[derive(Debug, Clone)]
pub struct LegacyRow {
    pub owner: Option<String>,
    pub key: String,
    pub raw: String,
    pub data: Value,
}

#[derive(Debug, Clone, Default)]
pub struct CanonicalState {
    pub configured: bool,
    pub policy_saved: bool,
    pub settings: Option<(AutopilotSettings, PipelinePolicy)>,
}

#[derive(Debug, Clone, Default)]
pub struct Inventory {
    pub streamers: BTreeSet<String>,
    pub logins: BTreeMap<String, String>,
    pub cutover: BTreeMap<String, String>,
    pub canonical: BTreeMap<String, CanonicalState>,
    pub pool_clips: BTreeSet<i64>,
    pub clips: Vec<LegacyRow>,
    pub approvals: Vec<LegacyRow>,
    pub queue: Vec<LegacyRow>,
    pub enrichment: Vec<LegacyRow>,
    pub streamer_settings: Vec<LegacyRow>,
    pub schedules: Vec<LegacyRow>,
    pub categories: Vec<LegacyRow>,
    pub settings: Vec<LegacyRow>,
}

#[derive(Debug, Clone)]
pub struct PlannedRow {
    pub mapping: PipelineMigrationMapping,
    pub raw_snapshot: String,
}

#[derive(Debug, Clone, Default)]
pub struct Plan {
    pub rows: Vec<PlannedRow>,
    pub orphans: BTreeMap<String, usize>,
    pub skipped_unified: BTreeSet<String>,
    pub reconciliation: BTreeMap<String, Value>,
    pub legacy_awaiting: usize,
    pub legacy_approved: usize,
    pub logins: BTreeMap<String, String>,
    pub open_legacy: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct DispositionCounts {
    pub mapped: usize,
    pub history: usize,
    pub unresolved: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Conflict {
    pub twitch_user_id: String,
    pub legacy_table: String,
    pub legacy_key: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct MigrationReport {
    pub migration_version: i32,
    pub applied: bool,
    pub owners: usize,
    pub skipped_unified: usize,
    pub rows: usize,
    pub totals: DispositionCounts,
    pub per_table: BTreeMap<String, DispositionCounts>,
    pub legacy_awaiting: usize,
    pub legacy_approved: usize,
    pub preserved_awaiting: usize,
    pub preserved_approved: usize,
    pub inserted: usize,
    pub updated: usize,
    pub unchanged: usize,
    pub stale: usize,
    pub cutover_changed: usize,
    pub orphans: BTreeMap<String, usize>,
    pub conflicts: Vec<Conflict>,
    pub channel_policies: BTreeMap<String, Value>,
    pub channels: Vec<ChannelDecision>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChannelDecision {
    pub twitch_user_id: String,
    pub login: String,
    pub pipeline_mode: String,
    pub review_policy: String,
    pub source: String,
    pub open_legacy_rows: usize,
}

impl ChannelDecision {
    pub fn line(&self) -> String {
        format!(
            "owner={} login={} mode={} review={} source={} open_legacy_rows={}",
            self.twitch_user_id,
            self.login,
            self.pipeline_mode,
            self.review_policy,
            self.source,
            self.open_legacy_rows
        )
    }
}

type MapKey = (String, String, String);

#[derive(Debug, Clone, PartialEq)]
struct StoredRow {
    digest: String,
    disposition: String,
    canonical: Option<Value>,
    reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CutoverCommand {
    DryRun,
    Apply,
    Pause(String),
    Resume(String),
    Resolve { queue_id: i64, published: bool },
}

impl CutoverCommand {
    pub fn parse(arguments: &[&str]) -> Option<Self> {
        match arguments {
            ["--social-cutover", "--dry-run"] => Some(Self::DryRun),
            ["--social-cutover", "--apply"] => Some(Self::Apply),
            ["--social-cutover", "--pause", owner] if !owner.is_empty() => {
                Some(Self::Pause((*owner).to_string()))
            }
            ["--social-cutover", "--resume", owner] if !owner.is_empty() => {
                Some(Self::Resume((*owner).to_string()))
            }
            ["--social-cutover", "--resolve", queue_id, outcome] => Some(Self::Resolve {
                queue_id: queue_id.parse().ok()?,
                published: match *outcome {
                    "--published" => true,
                    "--failed" => false,
                    _ => return None,
                },
            }),
            _ => None,
        }
    }
}

pub async fn run_command(pool: &PgPool, command: &CutoverCommand) -> Result<Value, MigrationError> {
    if *command != CutoverCommand::DryRun
        && !write_barrier_installed(&mut *pool.acquire().await?).await?
    {
        return Err(MigrationError::BarrierMissing);
    }
    Ok(match command {
        CutoverCommand::DryRun => {
            let migration = dry_run(pool).await?;
            let lines: Vec<String> = migration
                .channels
                .iter()
                .map(ChannelDecision::line)
                .collect();
            json!({ "channel_lines": lines, "migration": migration })
        }
        CutoverCommand::Apply => {
            let migration = apply(pool).await?;
            let cutover = activate_ready(pool).await?;
            json!({ "migration": migration, "cutover": cutover })
        }
        CutoverCommand::Pause(owner) => json!({ "cutover": [pause(pool, owner).await?] }),
        CutoverCommand::Resume(owner) => json!({ "cutover": [resume(pool, owner).await?] }),
        CutoverCommand::Resolve {
            queue_id,
            published,
        } => json!({ "resolved": resolve_uncertain(pool, *queue_id, *published).await? }),
    })
}

pub async fn resolve_uncertain(
    pool: &PgPool,
    queue_id: i64,
    published: bool,
) -> Result<bool, MigrationError> {
    let mut tx = pool.begin().await?;
    let platform: Option<(i64, String)> = sqlx::query_as(
        "UPDATE twitch_clips_upload_queue SET status = CASE WHEN $2 THEN 'completed' ELSE 'failed' END,
             outcome_reason = 'operator_resolved', completed_at = NOW(),
             last_error = CASE WHEN $2 THEN NULL ELSE last_error END
         WHERE id = $1 AND status = 'uncertain' AND produced_video_id IS NULL
         RETURNING clip_id, platform",
    )
    .bind(queue_id)
    .bind(published)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((clip_id, platform)) = platform else {
        tx.commit().await?;
        return Ok(false);
    };
    if published {
        sqlx::query(
            "UPDATE twitch_clips_social_media SET
                 uploaded_tiktok = uploaded_tiktok OR $2 = 'tiktok',
                 uploaded_youtube = uploaded_youtube OR $2 = 'youtube',
                 uploaded_instagram = uploaded_instagram OR $2 = 'instagram',
                 tiktok_uploaded_at = CASE WHEN $2 = 'tiktok' THEN NOW() ELSE tiktok_uploaded_at END,
                 youtube_uploaded_at = CASE WHEN $2 = 'youtube' THEN NOW() ELSE youtube_uploaded_at END,
                 instagram_uploaded_at = CASE WHEN $2 = 'instagram' THEN NOW() ELSE instagram_uploaded_at END
             WHERE id = $1",
        )
        .bind(clip_id)
        .bind(&platform)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    if published {
        crate::retention::refresh_clip_publication_status(pool, clip_id).await;
    }
    Ok(true)
}

pub async fn dry_run(pool: &PgPool) -> Result<MigrationReport, MigrationError> {
    let mut tx = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *tx)
        .await?;
    sqlx::query("SET LOCAL TimeZone = 'UTC'")
        .execute(&mut *tx)
        .await?;
    let inventory = load_inventory(&mut tx).await?;
    let plan = plan(&inventory);
    let stored = load_stored(&mut tx).await?;
    let report = report(&plan, &stored, false);
    tx.rollback().await?;
    Ok(report)
}

pub async fn apply(pool: &PgPool) -> Result<MigrationReport, MigrationError> {
    let mut tx = pool.begin().await?;
    sqlx::query("SET LOCAL TimeZone = 'UTC'")
        .execute(&mut *tx)
        .await?;
    let discovered = plan(&load_inventory(&mut tx).await?);
    for owner in plan_owners(&discovered) {
        autopilot_settings::lock_owner(&mut tx, &owner).await?;
    }
    let inventory = load_inventory(&mut tx).await?;
    let plan = plan(&inventory);
    let stored = load_stored(&mut tx).await?;
    let mut report = report(&plan, &stored, true);
    for row in &plan.rows {
        let mapping = &row.mapping;
        let key = map_key(mapping);
        if stored.get(&key) != Some(&stored_row(mapping)) {
            upsert(&mut tx, row).await?;
        }
    }
    for (owner, reconciliation) in &plan.reconciliation {
        if upsert_cutover(&mut tx, owner, reconciliation).await? {
            report.cutover_changed += 1;
        }
    }
    tx.commit().await?;
    Ok(report)
}

pub async fn activate(pool: &PgPool, owner: &str) -> Result<CutoverReport, MigrationError> {
    let mut tx = pool.begin().await?;
    sqlx::query("SET LOCAL TimeZone = 'UTC'")
        .execute(&mut *tx)
        .await?;
    autopilot_settings::lock_owner(&mut tx, owner).await?;
    let report = match cutover_state(&mut tx, owner).await?.as_deref() {
        Some("shadow") => activate_on(&mut tx, owner).await?,
        Some("unified" | "paused") => outcome(owner, CutoverOutcome::AlreadyClosed, 0),
        _ => outcome(owner, CutoverOutcome::NotShadow, 0),
    };
    tx.commit().await?;
    Ok(report)
}

pub async fn activate_ready(pool: &PgPool) -> Result<Vec<CutoverReport>, MigrationError> {
    let owners: Vec<String> = sqlx::query_scalar(
        "SELECT twitch_user_id FROM social_pipeline_cutover WHERE state = 'shadow' ORDER BY twitch_user_id",
    )
    .fetch_all(pool)
    .await?;
    let mut reports = Vec::with_capacity(owners.len());
    for owner in owners {
        reports.push(activate(pool, &owner).await?);
    }
    Ok(reports)
}

pub type OwnerCutover = (String, Result<CutoverReport, MigrationError>);

pub async fn cutover_new_owners(pool: &PgPool) -> Result<Vec<OwnerCutover>, MigrationError> {
    if !write_barrier_installed(&mut *pool.acquire().await?).await? {
        return Err(MigrationError::BarrierMissing);
    }
    let planned = dry_run(pool).await?;
    let candidates = owners_to_adopt(pool, &planned.channels).await?;
    let mut results = Vec::with_capacity(candidates.len());
    for owner in candidates {
        let result = adopt_owner(pool, &owner).await;
        results.push((owner, result));
    }
    Ok(results)
}

async fn adopt_owner(pool: &PgPool, owner: &str) -> Result<CutoverReport, MigrationError> {
    let mut tx = pool.begin().await?;
    sqlx::query("SET LOCAL TimeZone = 'UTC'")
        .execute(&mut *tx)
        .await?;
    autopilot_settings::lock_owner(&mut tx, owner).await?;
    let report = match cutover_state(&mut tx, owner).await?.as_deref() {
        None | Some("shadow") => activate_on(&mut tx, owner).await?,
        Some("unified" | "paused") => outcome(owner, CutoverOutcome::AlreadyClosed, 0),
        Some(_) => outcome(owner, CutoverOutcome::NotShadow, 0),
    };
    tx.commit().await?;
    Ok(report)
}

async fn owners_to_adopt(
    pool: &PgPool,
    channels: &[ChannelDecision],
) -> Result<Vec<String>, MigrationError> {
    let owners: Vec<String> = channels
        .iter()
        .map(|channel| channel.twitch_user_id.clone())
        .collect();
    if owners.is_empty() {
        return Ok(Vec::new());
    }
    Ok(sqlx::query_scalar(
        "SELECT owner FROM unnest($1::text[]) AS owner
         WHERE NOT EXISTS (
             SELECT 1 FROM social_pipeline_cutover c
             WHERE c.twitch_user_id = owner AND c.state <> 'shadow'
         )
         ORDER BY owner",
    )
    .bind(&owners)
    .fetch_all(pool)
    .await?)
}

pub async fn pause(pool: &PgPool, owner: &str) -> Result<CutoverReport, MigrationError> {
    let mut tx = pool.begin().await?;
    autopilot_settings::lock_owner(&mut tx, owner).await?;
    if cutover_state(&mut tx, owner).await?.as_deref() != Some("unified") {
        tx.commit().await?;
        return Ok(outcome(owner, CutoverOutcome::NotUnified, 0));
    }
    sqlx::query(
        "UPDATE social_pipeline_cutover c SET streamer_paused = s.paused, updated_at = NOW()
         FROM social_media_autopilot_settings s
         WHERE c.twitch_user_id = $1 AND s.twitch_user_id = c.twitch_user_id",
    )
    .bind(owner)
    .execute(&mut *tx)
    .await?;
    let enabled: Option<bool> = sqlx::query_scalar(
        "UPDATE social_media_autopilot_settings SET paused = TRUE, updated_at = NOW()
         WHERE twitch_user_id = $1 RETURNING enabled",
    )
    .bind(owner)
    .fetch_optional(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE social_pipeline_cutover SET state = 'paused', updated_at = NOW() WHERE twitch_user_id = $1",
    )
    .bind(owner)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(switched(owner, CutoverOutcome::Paused, enabled))
}

pub async fn resume(pool: &PgPool, owner: &str) -> Result<CutoverReport, MigrationError> {
    let mut tx = pool.begin().await?;
    autopilot_settings::lock_owner(&mut tx, owner).await?;
    if cutover_state(&mut tx, owner).await?.as_deref() != Some("paused") {
        tx.commit().await?;
        return Ok(outcome(owner, CutoverOutcome::NotPaused, 0));
    }
    let streamer_paused: Option<bool> = sqlx::query_scalar(
        "UPDATE social_pipeline_cutover c SET state = 'unified', streamer_paused = NULL, updated_at = NOW()
         FROM social_pipeline_cutover previous
         WHERE c.twitch_user_id = $1 AND previous.twitch_user_id = c.twitch_user_id
         RETURNING previous.streamer_paused",
    )
    .bind(owner)
    .fetch_one(&mut *tx)
    .await?;
    let enabled: Option<bool> = sqlx::query_scalar(
        "UPDATE social_media_autopilot_settings SET paused = COALESCE($2, paused), updated_at = NOW()
         WHERE twitch_user_id = $1 RETURNING enabled",
    )
    .bind(owner)
    .bind(streamer_paused)
    .fetch_optional(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(switched(owner, CutoverOutcome::Resumed, enabled))
}

fn switched(owner: &str, done: CutoverOutcome, enabled: Option<bool>) -> CutoverReport {
    let mut report = outcome(owner, done, 0);
    report.autopilot_enabled = enabled.unwrap_or(false);
    report
}

async fn activate_on(
    connection: &mut PgConnection,
    owner: &str,
) -> Result<CutoverReport, MigrationError> {
    let plan = plan(&load_inventory(connection).await?);
    let reconciliation = plan.reconciliation.get(owner).cloned().unwrap_or_else(
        || json!({"migration_version": MIGRATION_VERSION, "unresolved": 0, "ready": true}),
    );
    let rows: Vec<&PlannedRow> = plan
        .rows
        .iter()
        .filter(|row| row.mapping.twitch_user_id == owner)
        .collect();
    for row in &rows {
        upsert(connection, row).await?;
    }
    upsert_cutover(connection, owner, &reconciliation).await?;
    let unresolved = rows
        .iter()
        .filter(|row| row.mapping.disposition == MigrationDisposition::Unresolved)
        .count();
    let blocking = rows
        .iter()
        .filter(|row| blocks_activation(&row.mapping))
        .count();
    if blocking > 0 {
        return Ok(outcome(owner, CutoverOutcome::NotReady, unresolved));
    }
    let enabled = match reconciliation.get("proposed_policy") {
        Some(proposed) => {
            apply_policy(connection, owner, serde_json::from_value(proposed.clone())?).await?
        }
        None => {
            autopilot_settings::load_on(connection, owner)
                .await?
                .enabled
        }
    };
    sqlx::query(
        "UPDATE social_pipeline_cutover SET state = 'unified', updated_at = NOW()
         WHERE twitch_user_id = $1",
    )
    .bind(owner)
    .execute(&mut *connection)
    .await?;
    let mut report = outcome(owner, CutoverOutcome::Activated, unresolved);
    report.autopilot_enabled = enabled;
    Ok(report)
}

fn blocks_activation(mapping: &PipelineMigrationMapping) -> bool {
    mapping.disposition == MigrationDisposition::Unresolved
        && matches!(
            mapping.unresolved_reason.as_deref(),
            Some("in_flight_claim" | "uncertain_effect")
        )
}

async fn apply_policy(
    connection: &mut PgConnection,
    owner: &str,
    proposed: ProposedPolicy,
) -> Result<bool, MigrationError> {
    let mut settings = autopilot_settings::load_on(connection, owner).await?;
    if let Some(timezone) = proposed.timezone {
        settings.timezone = timezone;
    }
    settings.target_platforms =
        autopilot_settings::normalize_platforms(&proposed.target_platforms)?;
    settings.schedule_slots = sorted(proposed.schedule_slots);
    settings.enabled = settings.enabled || !settings.target_platforms.is_empty();
    let mut policy = proposed.policy;
    for slots in policy.platform_schedule_slots.values_mut() {
        *slots = sorted(std::mem::take(slots));
    }
    settings.publish_mode = if policy.review_policy == ReviewPolicy::Off {
        PublishMode::FullAuto
    } else {
        PublishMode::Review
    };
    settings.validate()?;
    policy.validate()?;
    sqlx::query(
        "INSERT INTO social_media_autopilot_settings (twitch_user_id, enabled, publish_mode,
            timezone, schedule_slots, target_platforms, pipeline_mode, review_policy,
            metadata_scope, text_defaults, schedule_scope, platform_schedule_slots,
            distribution_policy, policy_revision)
         SELECT $1, s.enabled, s.publish_mode, s.timezone, s.schedule_slots, s.target_platforms,
            p.pipeline_mode, p.review_policy, p.metadata_scope, p.text_defaults, p.schedule_scope,
            p.platform_schedule_slots, p.distribution_policy, 1
         FROM jsonb_populate_record(NULL::social_media_autopilot_settings, $2::text::jsonb) s,
            jsonb_populate_record(NULL::social_media_autopilot_settings, $3::text::jsonb) p
         ON CONFLICT (twitch_user_id) DO UPDATE SET enabled = EXCLUDED.enabled,
            publish_mode = EXCLUDED.publish_mode, timezone = EXCLUDED.timezone,
            schedule_slots = EXCLUDED.schedule_slots, target_platforms = EXCLUDED.target_platforms,
            pipeline_mode = EXCLUDED.pipeline_mode, review_policy = EXCLUDED.review_policy,
            metadata_scope = EXCLUDED.metadata_scope, text_defaults = EXCLUDED.text_defaults,
            schedule_scope = EXCLUDED.schedule_scope,
            platform_schedule_slots = EXCLUDED.platform_schedule_slots,
            distribution_policy = EXCLUDED.distribution_policy,
            policy_revision = social_media_autopilot_settings.policy_revision + 1,
            updated_at = NOW()",
    )
    .bind(owner)
    .bind(serde_json::to_string(&settings)?)
    .bind(serde_json::to_string(&policy)?)
    .execute(&mut *connection)
    .await?;
    crate::production_jobs::autopilot_schedule::retime_on(connection, owner, &settings)
        .await
        .map_err(|error| MigrationError::Schedule(error.to_string()))?;
    Ok(settings.enabled)
}

async fn cutover_state(
    connection: &mut PgConnection,
    owner: &str,
) -> Result<Option<String>, MigrationError> {
    Ok(
        sqlx::query_scalar("SELECT state FROM social_pipeline_cutover WHERE twitch_user_id = $1")
            .bind(owner)
            .fetch_optional(connection)
            .await?,
    )
}

fn outcome(owner: &str, outcome: CutoverOutcome, unresolved: usize) -> CutoverReport {
    CutoverReport {
        twitch_user_id: owner.to_string(),
        outcome,
        unresolved,
        autopilot_enabled: false,
    }
}

pub async fn write_barrier_installed(connection: &mut PgConnection) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT to_regprocedure('social_pipeline_legacy_closed(text)') IS NOT NULL")
        .fetch_one(connection)
        .await
}

pub async fn legacy_writer_open(
    connection: &mut PgConnection,
    clip_db_id: i64,
) -> Result<bool, sqlx::Error> {
    if !write_barrier_installed(connection).await? {
        return Ok(true);
    }
    sqlx::query_scalar("SELECT NOT social_pipeline_legacy_closed(social_clip_pipeline_owner($1))")
        .bind(clip_db_id)
        .fetch_one(connection)
        .await
}

fn plan_owners(plan: &Plan) -> BTreeSet<String> {
    plan.rows
        .iter()
        .map(|row| row.mapping.twitch_user_id.clone())
        .collect()
}

fn map_key(mapping: &PipelineMigrationMapping) -> MapKey {
    (
        mapping.twitch_user_id.clone(),
        mapping.legacy_table.clone(),
        mapping.legacy_key.clone(),
    )
}

fn stored_row(mapping: &PipelineMigrationMapping) -> StoredRow {
    StoredRow {
        digest: mapping.input_digest.clone(),
        disposition: disposition_name(mapping.disposition).to_string(),
        canonical: mapping.canonical_subject.clone(),
        reason: mapping.unresolved_reason.clone(),
    }
}

fn disposition_name(disposition: MigrationDisposition) -> &'static str {
    match disposition {
        MigrationDisposition::Mapped => "mapped",
        MigrationDisposition::History => "history",
        MigrationDisposition::Unresolved => "unresolved",
    }
}

fn report(plan: &Plan, stored: &BTreeMap<MapKey, StoredRow>, applied: bool) -> MigrationReport {
    let mut report = MigrationReport {
        migration_version: MIGRATION_VERSION,
        applied,
        owners: plan.reconciliation.len(),
        skipped_unified: plan.skipped_unified.len(),
        rows: plan.rows.len(),
        legacy_awaiting: plan.legacy_awaiting,
        legacy_approved: plan.legacy_approved,
        orphans: plan.orphans.clone(),
        ..MigrationReport::default()
    };
    let mut planned = BTreeSet::new();
    for row in &plan.rows {
        let mapping = &row.mapping;
        count(&mut report.totals, mapping.disposition);
        count(
            report
                .per_table
                .entry(mapping.legacy_table.clone())
                .or_default(),
            mapping.disposition,
        );
        if let Some(review) = review_decision(mapping) {
            channel_policy(&mut report, &mapping.twitch_user_id)["review_policy"] = review;
        }
        if let Some(reason) = &mapping.unresolved_reason {
            report.conflicts.push(Conflict {
                twitch_user_id: mapping.twitch_user_id.clone(),
                legacy_table: mapping.legacy_table.clone(),
                legacy_key: mapping.legacy_key.clone(),
                reason: reason.clone(),
            });
        }
        if mapping.legacy_table == APPROVALS
            && mapping.disposition != MigrationDisposition::Unresolved
        {
            match text(&mapping.original_snapshot, "state") {
                Some("awaiting_approval") => report.preserved_awaiting += 1,
                Some("approved") => report.preserved_approved += 1,
                _ => {}
            }
        }
        let key = map_key(mapping);
        match stored.get(&key) {
            None => report.inserted += 1,
            Some(existing) if *existing == stored_row(mapping) => report.unchanged += 1,
            Some(_) => report.updated += 1,
        }
        planned.insert(key);
    }
    for (owner, reconciliation) in &plan.reconciliation {
        let proposed = &reconciliation["proposed_policy"];
        channel_policy(&mut report, owner)["pipeline_mode"] = json!({
            "applied": proposed["policy"]["pipeline_mode"],
            "source": proposed["policy_source"],
        });
        report
            .channels
            .push(channel_decision(plan, owner, proposed));
    }
    let owners: BTreeSet<&String> = plan.reconciliation.keys().collect();
    report.stale = stored
        .keys()
        .filter(|key| owners.contains(&key.0) && !planned.contains(*key))
        .count();
    report
}

fn channel_decision(plan: &Plan, owner: &str, proposed: &Value) -> ChannelDecision {
    let field = |value: &Value| value.as_str().unwrap_or("unknown").to_string();
    ChannelDecision {
        twitch_user_id: owner.to_string(),
        login: plan
            .logins
            .get(owner)
            .cloned()
            .unwrap_or_else(|| "-".to_string()),
        pipeline_mode: field(&proposed["policy"]["pipeline_mode"]),
        review_policy: field(&proposed["policy"]["review_policy"]),
        source: match text(proposed, "policy_source") {
            Some("planen") => "explicit-new",
            Some("legacy") => "legacy-mapped",
            _ => "unknown",
        }
        .to_string(),
        open_legacy_rows: plan.open_legacy.get(owner).copied().unwrap_or(0),
    }
}

fn channel_policy<'r>(report: &'r mut MigrationReport, owner: &str) -> &'r mut Value {
    report
        .channel_policies
        .entry(owner.to_string())
        .or_insert_with(|| json!({}))
}

fn review_decision(mapping: &PipelineMigrationMapping) -> Option<Value> {
    let subject = mapping.canonical_subject.as_ref()?;
    if mapping.legacy_table != STREAMER_SETTINGS || text(subject, "kind") != Some("channel_review")
    {
        return None;
    }
    Some(json!({
        "legacy": subject.get("legacy_review_policy"),
        "applied": subject.get("review_policy"),
        "source": subject.get("review_source"),
    }))
}

fn count(counts: &mut DispositionCounts, disposition: MigrationDisposition) {
    match disposition {
        MigrationDisposition::Mapped => counts.mapped += 1,
        MigrationDisposition::History => counts.history += 1,
        MigrationDisposition::Unresolved => counts.unresolved += 1,
    }
}

async fn upsert(connection: &mut PgConnection, row: &PlannedRow) -> Result<(), MigrationError> {
    let mapping = &row.mapping;
    let canonical = mapping
        .canonical_subject
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;
    sqlx::query(
        "INSERT INTO social_pipeline_migration_map (twitch_user_id, legacy_table, legacy_key,
            migration_version, input_digest, canonical_subject, disposition, unresolved_reason,
            original_snapshot)
         VALUES ($1, $2, $3, $4, $5, $6::text::jsonb, $7, $8, $9::text::jsonb)
         ON CONFLICT (twitch_user_id, legacy_table, legacy_key, migration_version) DO UPDATE SET
            input_digest = EXCLUDED.input_digest, canonical_subject = EXCLUDED.canonical_subject,
            disposition = EXCLUDED.disposition, unresolved_reason = EXCLUDED.unresolved_reason,
            original_snapshot = EXCLUDED.original_snapshot",
    )
    .bind(&mapping.twitch_user_id)
    .bind(&mapping.legacy_table)
    .bind(&mapping.legacy_key)
    .bind(mapping.migration_version)
    .bind(&mapping.input_digest)
    .bind(canonical)
    .bind(disposition_name(mapping.disposition))
    .bind(&mapping.unresolved_reason)
    .bind(&row.raw_snapshot)
    .execute(connection)
    .await?;
    Ok(())
}

async fn upsert_cutover(
    connection: &mut PgConnection,
    owner: &str,
    reconciliation: &Value,
) -> Result<bool, MigrationError> {
    let changed = sqlx::query(
        "INSERT INTO social_pipeline_cutover AS c (twitch_user_id, state, migration_version, reconciliation)
         VALUES ($1, 'shadow', $2, $3::text::jsonb)
         ON CONFLICT (twitch_user_id) DO UPDATE SET state = 'shadow',
            migration_version = EXCLUDED.migration_version,
            reconciliation = EXCLUDED.reconciliation, updated_at = NOW()
         WHERE c.state IN ('legacy', 'shadow')
           AND (c.state, c.migration_version, c.reconciliation)
               IS DISTINCT FROM ('shadow', EXCLUDED.migration_version, EXCLUDED.reconciliation)",
    )
    .bind(owner)
    .bind(MIGRATION_VERSION)
    .bind(serde_json::to_string(reconciliation)?)
    .execute(connection)
    .await?
    .rows_affected();
    Ok(changed > 0)
}

async fn load_stored(
    connection: &mut PgConnection,
) -> Result<BTreeMap<MapKey, StoredRow>, MigrationError> {
    let rows = sqlx::query(
        "SELECT twitch_user_id, legacy_table, legacy_key, input_digest, disposition,
            canonical_subject::text AS canonical, unresolved_reason
         FROM social_pipeline_migration_map WHERE migration_version = $1",
    )
    .bind(MIGRATION_VERSION)
    .fetch_all(connection)
    .await?;
    let mut stored = BTreeMap::new();
    for row in rows {
        let canonical: Option<String> = row.try_get("canonical")?;
        stored.insert(
            (
                row.try_get("twitch_user_id")?,
                row.try_get("legacy_table")?,
                row.try_get("legacy_key")?,
            ),
            StoredRow {
                digest: row.try_get("input_digest")?,
                disposition: row.try_get("disposition")?,
                canonical: canonical.as_deref().map(serde_json::from_str).transpose()?,
                reason: row.try_get("unresolved_reason")?,
            },
        );
    }
    Ok(stored)
}

const LOGIN_OWNER: &str = "(SELECT s.twitch_user_id FROM twitch_streamers s WHERE lower(s.twitch_login) = lower(LOGIN) ORDER BY s.twitch_user_id LIMIT 1)";

fn owner_by_login(column: &str) -> String {
    LOGIN_OWNER.replace("LOGIN", column)
}

pub async fn load_inventory(connection: &mut PgConnection) -> Result<Inventory, MigrationError> {
    let clip_owner = format!(
        "COALESCE(c.twitch_user_id, {})",
        owner_by_login("c.streamer_login")
    );
    let login_owner = format!(
        "COALESCE(t.twitch_user_id, {})",
        owner_by_login("t.streamer_login")
    );
    let mut inventory = Inventory {
        streamers: sqlx::query_scalar(
            "SELECT twitch_user_id FROM twitch_streamers WHERE twitch_user_id IS NOT NULL",
        )
        .fetch_all(&mut *connection)
        .await?
        .into_iter()
        .collect(),
        logins: sqlx::query_as(
            "SELECT twitch_user_id, twitch_login FROM twitch_streamers
             WHERE twitch_user_id IS NOT NULL AND twitch_login IS NOT NULL",
        )
        .fetch_all(&mut *connection)
        .await?
        .into_iter()
        .collect(),
        pool_clips: sqlx::query_scalar(
            "SELECT clip_db_id FROM social_clip_pool WHERE clip_db_id IS NOT NULL",
        )
        .fetch_all(&mut *connection)
        .await?
        .into_iter()
        .collect(),
        cutover: sqlx::query_as("SELECT twitch_user_id, state FROM social_pipeline_cutover")
            .fetch_all(&mut *connection)
            .await?
            .into_iter()
            .collect(),
        ..Inventory::default()
    };
    inventory.clips = load_rows(connection, &format!(
        "SELECT c.id::text AS key, {clip_owner} AS owner, to_jsonb(c)::text AS raw
         FROM twitch_clips_social_media c
         WHERE EXISTS (SELECT 1 FROM social_media_clip_approval a WHERE a.clip_db_id = c.id)
            OR EXISTS (SELECT 1 FROM twitch_clips_upload_queue q WHERE q.clip_id = c.id AND q.produced_video_id IS NULL)
            OR EXISTS (SELECT 1 FROM social_media_clip_enrichment e WHERE e.clip_db_id = c.id)
            OR EXISTS (SELECT 1 FROM social_media_settings s WHERE s.key LIKE 'tiktok\\_draft:%' AND split_part(s.key, ':', 3) = c.id::text)
            OR ((c.pool_managed_at IS NULL OR c.pool_legacy_reserved)
                AND (COALESCE(c.uploaded_tiktok, FALSE) OR COALESCE(c.uploaded_youtube, FALSE)
                     OR COALESCE(c.uploaded_instagram, FALSE) OR c.tiktok_video_id IS NOT NULL
                     OR c.youtube_video_id IS NOT NULL OR c.instagram_media_id IS NOT NULL))
         ORDER BY c.id"
    )).await?;
    inventory.approvals = load_rows(connection, &format!(
        "SELECT a.approval_id::text AS key, {clip_owner} AS owner, to_jsonb(a)::text AS raw
         FROM social_media_clip_approval a LEFT JOIN twitch_clips_social_media c ON c.id = a.clip_db_id
         WHERE a.clip_db_id IS NOT NULL AND a.produced_video_id IS NULL ORDER BY a.approval_id"
    )).await?;
    inventory.queue = load_rows(
        connection,
        &format!(
            "SELECT q.id::text AS key, {clip_owner} AS owner, to_jsonb(q)::text AS raw
         FROM twitch_clips_upload_queue q LEFT JOIN twitch_clips_social_media c ON c.id = q.clip_id
         WHERE q.produced_video_id IS NULL ORDER BY q.id"
        ),
    )
    .await?;
    inventory.enrichment = load_rows(connection, &format!(
        "SELECT e.clip_db_id::text AS key, {clip_owner} AS owner, to_jsonb(e)::text AS raw
         FROM social_media_clip_enrichment e LEFT JOIN twitch_clips_social_media c ON c.id = e.clip_db_id
         ORDER BY e.clip_db_id"
    )).await?;
    inventory.streamer_settings = load_rows(
        connection,
        &format!(
            "SELECT t.streamer_login AS key, {login_owner} AS owner, to_jsonb(t)::text AS raw
         FROM social_media_streamer_settings t ORDER BY t.streamer_login"
        ),
    )
    .await?;
    inventory.schedules = load_rows(connection, &format!(
        "SELECT t.streamer_login || ':' || t.platform AS key, {login_owner} AS owner, to_jsonb(t)::text AS raw
         FROM social_media_platform_schedule t
         ORDER BY t.streamer_login, t.platform, t.twitch_user_id NULLS LAST, t.updated_at"
    )).await?;
    inventory.categories = load_rows(connection, &format!(
        "SELECT t.streamer_login || ':' || t.category_key AS key, {login_owner} AS owner, to_jsonb(t)::text AS raw
         FROM social_media_category_settings t
         ORDER BY t.streamer_login, t.category_key, t.twitch_user_id NULLS LAST, t.updated_at"
    )).await?;
    inventory.settings = load_rows(
        connection,
        "SELECT s.key, split_part(s.key, ':', 2) AS owner, to_jsonb(s)::text AS raw
         FROM social_media_settings s
         WHERE s.key LIKE 'tiktok\\_draft:%' OR s.key LIKE 'tiktok\\_defaults:%' ORDER BY s.key",
    )
    .await?;
    let owners = inventory_owners(&inventory);
    for owner in owners {
        let state = load_canonical(connection, &owner).await?;
        inventory.canonical.insert(owner, state);
    }
    Ok(inventory)
}

fn inventory_owners(inventory: &Inventory) -> BTreeSet<String> {
    [
        &inventory.clips,
        &inventory.approvals,
        &inventory.queue,
        &inventory.enrichment,
        &inventory.streamer_settings,
        &inventory.schedules,
        &inventory.categories,
        &inventory.settings,
    ]
    .into_iter()
    .flatten()
    .filter_map(|row| row.owner.clone())
    .filter(|owner| inventory.streamers.contains(owner))
    .collect()
}

async fn load_rows(
    connection: &mut PgConnection,
    query: &str,
) -> Result<Vec<LegacyRow>, MigrationError> {
    let rows = sqlx::query(sqlx::AssertSqlSafe(query.to_string()))
        .fetch_all(&mut *connection)
        .await?;
    let mut loaded = Vec::with_capacity(rows.len());
    for row in rows {
        let raw: String = row.try_get("raw")?;
        let data = serde_json::from_str(&raw)?;
        loaded.push(LegacyRow {
            owner: row.try_get("owner")?,
            key: row.try_get("key")?,
            raw,
            data,
        });
    }
    Ok(loaded)
}

async fn load_canonical(
    connection: &mut PgConnection,
    owner: &str,
) -> Result<CanonicalState, MigrationError> {
    let flags: Option<(bool, bool)> = sqlx::query_as(
        "SELECT COALESCE(enabled OR pipeline_mode IS NOT NULL OR schedule_slots <> '[]'::jsonb
            OR target_platforms <> '[]'::jsonb, FALSE), policy_revision > 0
         FROM social_media_autopilot_settings WHERE twitch_user_id = $1",
    )
    .bind(owner)
    .fetch_optional(&mut *connection)
    .await?;
    let (configured, policy_saved) = flags.unwrap_or((false, false));
    let settings = readable(autopilot_settings::load_on(connection, owner).await)?;
    let policy = readable(autopilot_settings::load_policy_on(connection, owner).await)?;
    Ok(CanonicalState {
        configured,
        policy_saved,
        settings: settings.zip(policy),
    })
}

fn readable<T>(result: Result<T, SettingsError>) -> Result<Option<T>, MigrationError> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(SettingsError::Database(error)) => Err(error.into()),
        Err(_) => Ok(None),
    }
}

pub fn plan(inventory: &Inventory) -> Plan {
    let context = Context::new(inventory);
    let mut plan = Plan {
        logins: inventory.logins.clone(),
        ..Plan::default()
    };
    let tables: [(&str, &[LegacyRow]); 8] = [
        (CLIPS, &inventory.clips),
        (APPROVALS, &inventory.approvals),
        (QUEUE, &inventory.queue),
        (ENRICHMENT, &inventory.enrichment),
        (STREAMER_SETTINGS, &inventory.streamer_settings),
        (SCHEDULES, &inventory.schedules),
        (CATEGORIES, &inventory.categories),
        (SETTINGS, &inventory.settings),
    ];
    for (table, rows) in tables {
        let mut seen = BTreeMap::<(String, String), usize>::new();
        for row in rows {
            if table == APPROVALS {
                match text(&row.data, "state") {
                    Some("awaiting_approval") => plan.legacy_awaiting += 1,
                    Some("approved") => plan.legacy_approved += 1,
                    _ => {}
                }
            }
            let Some(owner) = row
                .owner
                .as_ref()
                .filter(|owner| inventory.streamers.contains(*owner))
            else {
                *plan.orphans.entry(table.to_string()).or_default() += 1;
                continue;
            };
            if matches!(
                inventory.cutover.get(owner).map(String::as_str),
                Some("unified" | "paused")
            ) {
                plan.skipped_unified.insert(owner.clone());
                continue;
            }
            if table == QUEUE
                && text(&row.data, "status").is_none_or(|status| !FINAL_QUEUE.contains(&status))
            {
                *plan.open_legacy.entry(owner.clone()).or_default() += 1;
            }
            let occurrence = {
                let seen = seen.entry((owner.clone(), row.key.clone())).or_default();
                *seen += 1;
                *seen
            };
            let outcome = if occurrence > 1 {
                Outcome::unresolved("duplicate_legacy_row", None)
            } else {
                context.map(table, owner, row)
            };
            let key = if occurrence > 1 {
                format!("{}#{}", row.key, occurrence)
            } else {
                row.key.clone()
            };
            plan.rows.push(planned(table, owner, key, row, outcome));
        }
    }
    let mut by_owner = BTreeMap::<&str, Vec<&PipelineMigrationMapping>>::new();
    for row in &plan.rows {
        by_owner
            .entry(row.mapping.twitch_user_id.as_str())
            .or_default()
            .push(&row.mapping);
    }
    let reconciliation: BTreeMap<String, Value> = by_owner
        .into_iter()
        .map(|(owner, rows)| (owner.to_string(), context.reconciliation(owner, &rows)))
        .collect();
    plan.reconciliation.extend(reconciliation);
    plan
}

fn planned(table: &str, owner: &str, key: String, row: &LegacyRow, outcome: Outcome) -> PlannedRow {
    PlannedRow {
        mapping: PipelineMigrationMapping {
            twitch_user_id: owner.to_string(),
            legacy_table: table.to_string(),
            legacy_key: key,
            migration_version: MIGRATION_VERSION,
            input_digest: digest(&row.raw),
            canonical_subject: outcome.subject,
            disposition: outcome.disposition,
            unresolved_reason: outcome.reason,
            original_snapshot: row.data.clone(),
        },
        raw_snapshot: row.raw.clone(),
    }
}

fn digest(raw: &str) -> String {
    Sha256::digest(raw.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

struct Outcome {
    disposition: MigrationDisposition,
    subject: Option<Value>,
    reason: Option<String>,
}

impl Outcome {
    fn mapped(subject: Value) -> Self {
        Self {
            disposition: MigrationDisposition::Mapped,
            subject: Some(subject),
            reason: None,
        }
    }

    fn history(subject: Value) -> Self {
        Self {
            disposition: MigrationDisposition::History,
            subject: Some(subject),
            reason: None,
        }
    }

    fn unresolved(reason: &str, subject: Option<Value>) -> Self {
        Self {
            disposition: MigrationDisposition::Unresolved,
            subject,
            reason: Some(reason.to_string()),
        }
    }
}

struct Context<'a> {
    inventory: &'a Inventory,
    clips: BTreeMap<i64, &'a LegacyRow>,
    queue_by_clip: BTreeMap<i64, Vec<&'a LegacyRow>>,
    approvals_by_clip: BTreeMap<i64, Vec<&'a LegacyRow>>,
    enrichment_by_clip: BTreeMap<i64, &'a LegacyRow>,
}

impl<'a> Context<'a> {
    fn new(inventory: &'a Inventory) -> Self {
        let mut context = Self {
            inventory,
            clips: BTreeMap::new(),
            queue_by_clip: BTreeMap::new(),
            approvals_by_clip: BTreeMap::new(),
            enrichment_by_clip: BTreeMap::new(),
        };
        for row in &inventory.clips {
            if let Some(id) = int(&row.data, "id") {
                context.clips.insert(id, row);
            }
        }
        for row in &inventory.queue {
            if let Some(id) = int(&row.data, "clip_id") {
                context.queue_by_clip.entry(id).or_default().push(row);
            }
        }
        for row in &inventory.approvals {
            if let Some(id) = int(&row.data, "clip_db_id") {
                context.approvals_by_clip.entry(id).or_default().push(row);
            }
        }
        for row in &inventory.enrichment {
            if let Some(id) = int(&row.data, "clip_db_id") {
                context.enrichment_by_clip.insert(id, row);
            }
        }
        context
    }

    fn map(&self, table: &str, owner: &str, row: &LegacyRow) -> Outcome {
        match table {
            CLIPS => self.map_clip(row),
            APPROVALS => self.map_approval(row),
            QUEUE => map_queue(row),
            ENRICHMENT => map_enrichment(row),
            STREAMER_SETTINGS => self.map_streamer_settings(owner, row),
            SCHEDULES => self.map_schedule(owner, row),
            CATEGORIES => map_category(row),
            _ => self.map_setting(owner, row),
        }
    }

    fn canonical(&self, owner: &str) -> Option<&CanonicalState> {
        self.inventory.canonical.get(owner)
    }

    fn policy_saved(&self, owner: &str) -> bool {
        self.canonical(owner)
            .is_some_and(|state| state.configured && state.policy_saved && state.settings.is_some())
    }

    fn live_queue(&self, clip: i64) -> Vec<&'a LegacyRow> {
        self.queue_by_clip
            .get(&clip)
            .into_iter()
            .flatten()
            .copied()
            .filter(|row| {
                text(&row.data, "status").is_some_and(|status| !FINAL_QUEUE.contains(&status))
            })
            .collect()
    }

    fn pending_work(&self, clip: i64) -> bool {
        let open_decision = self
            .approvals_by_clip
            .get(&clip)
            .into_iter()
            .flatten()
            .any(|row| {
                matches!(
                    text(&row.data, "state"),
                    Some("awaiting_approval" | "editing")
                )
            });
        open_decision || !self.live_queue(clip).is_empty()
    }

    fn eligibility(&self, clip: Option<&LegacyRow>) -> &'static str {
        let Some(clip) = clip else {
            return "unknown";
        };
        match (
            text(&clip.data, "source_kind"),
            text(&clip.data, "category_key"),
        ) {
            (Some("manual_upload"), _) => "manual",
            (_, Some("deadlock")) => "automatic",
            _ => "legacy_non_deadlock_visible",
        }
    }

    fn map_clip(&self, row: &LegacyRow) -> Outcome {
        let data = &row.data;
        let Some(id) = int(data, "id") else {
            return Outcome::unresolved("missing_clip_id", None);
        };
        let eligibility = self.eligibility(Some(row));
        let pool_member = self.inventory.pool_clips.contains(&id);
        let pending = self.pending_work(id);
        let (text_subject, conflicts) = self.clip_text(row, id);
        let subject = json!({
            "kind": "source",
            "clip_db_id": id,
            "twitch_clip_id": data.get("clip_id"),
            "source_kind": data.get("source_kind"),
            "category_key": data.get("category_key"),
            "game_name": data.get("game_name"),
            "eligibility": eligibility,
            "auto_post": eligibility == "automatic",
            "pool_member": pool_member,
            "needs_pool_backfill": eligibility == "automatic" && !pool_member && pending,
            "pending_work": pending,
            "discarded": !data.get("discarded_at").is_none_or(Value::is_null),
            "published": {
                "youtube": data.get("youtube_video_id"),
                "tiktok": data.get("tiktok_video_id"),
                "instagram": data.get("instagram_media_id"),
            },
            "tiktok_post_options": data.get("tiktok_post_options"),
            "text": text_subject,
            "text_conflicts": conflicts,
        });
        if subject["discarded"] == Value::Bool(true) && !pending {
            Outcome::history(subject)
        } else {
            Outcome::mapped(subject)
        }
    }

    fn clip_text(&self, clip: &LegacyRow, id: i64) -> (Value, Vec<Value>) {
        let live = self.live_queue(id);
        let enrichment = self.enrichment_by_clip.get(&id).map(|row| &row.data);
        let mut platforms = serde_json::Map::new();
        let mut conflicts = Vec::new();
        let mut chosen = Vec::new();
        for platform in PLATFORMS {
            let queued = live
                .iter()
                .filter(|row| text(&row.data, "platform") == Some(platform))
                .max_by_key(|row| int(&row.data, "id"))
                .map(|row| &row.data);
            let mut fields = serde_json::Map::new();
            for field in ["title", "description", "hashtags"] {
                let candidates = [
                    ("legacy_queue", queued.and_then(|q| queue_text(q, field))),
                    (
                        "enrichment",
                        enrichment.and_then(|e| enrichment_text(e, field, platform)),
                    ),
                    ("clip", clip_field_text(&clip.data, field)),
                ];
                let (value, conflict) = resolve_text(&candidates);
                if let Some(origins) = conflict {
                    conflicts
                        .push(json!({"platform": platform, "field": field, "origins": origins}));
                }
                chosen.push((platform, field, value.as_ref().map(|(_, v)| v.clone())));
                fields.insert(
                    field.to_string(),
                    value.map_or(
                        Value::Null,
                        |(origin, value)| json!({"value": value, "origin": origin}),
                    ),
                );
            }
            platforms.insert(platform.to_string(), Value::Object(fields));
        }
        let shared = ["title", "description", "hashtags"].iter().all(|field| {
            let mut values = chosen
                .iter()
                .filter(|(_, f, _)| f == field)
                .map(|(_, _, value)| value);
            let first = values.next();
            values.all(|value| Some(value) == first)
        });
        let scope = if shared { "shared" } else { "per_platform" };
        (json!({"scope": scope, "platforms": platforms}), conflicts)
    }

    fn map_approval(&self, row: &LegacyRow) -> Outcome {
        let data = &row.data;
        let Some(clip) = int(data, "clip_db_id") else {
            return Outcome::unresolved("missing_clip_reference", None);
        };
        let eligibility = self.eligibility(self.clips.get(&clip).copied());
        let queue: Vec<Value> = self
            .queue_by_clip
            .get(&clip)
            .into_iter()
            .flatten()
            .map(|q| {
                json!({
                    "queue_id": q.data.get("id"),
                    "platform": q.data.get("platform"),
                    "status": q.data.get("status"),
                    "scheduled_at": q.data.get("scheduled_at"),
                })
            })
            .collect();
        let pending_work = !self.live_queue(clip).is_empty();
        let base = json!({
            "kind": "review_decision",
            "clip_db_id": clip,
            "legacy_state": data.get("state"),
            "selected_platforms": data.get("approved_platforms"),
            "eligibility": eligibility,
            "dm_message_id": data.get("dm_message_id"),
            "dm_channel_id": data.get("dm_channel_id"),
            "last_sent_at": data.get("last_sent_at"),
        });
        match text(data, "state") {
            Some("awaiting_approval" | "editing") => Outcome::mapped(merge(
                base,
                json!({"decision": "pending", "publishable": false, "requires_exact_version_approval": true}),
            )),
            Some("approved") => Outcome::mapped(merge(
                base,
                json!({
                    "decision": "approved_legacy",
                    "approver_user_id": data.get("approver_user_id"),
                    "decided_at": data.get("decided_at"),
                    "legacy_queue": queue,
                    "pending_work": pending_work,
                    "exact_version": null,
                    "requires_exact_version_approval": pending_work,
                }),
            )),
            Some("skipped") => Outcome::history(merge(
                base,
                json!({"decision": "skipped", "decided_at": data.get("decided_at")}),
            )),
            _ => Outcome::unresolved("unknown_approval_state", Some(base)),
        }
    }

    fn map_streamer_settings(&self, owner: &str, row: &LegacyRow) -> Outcome {
        let data = &row.data;
        let legacy = match text(data, "approval_mode") {
            Some("manual") => ReviewPolicy::Required,
            Some("veto_window") => ReviewPolicy::VetoUntilDeadline,
            Some("full_auto") => ReviewPolicy::Off,
            _ => return Outcome::unresolved("unknown_approval_mode", None),
        };
        let canonical = self.canonical(owner).filter(|state| state.configured);
        let saved = canonical
            .filter(|state| state.policy_saved)
            .and_then(|state| state.settings.as_ref())
            .map(|(_, policy)| policy.review_policy);
        let subject = json!({
            "kind": "channel_review",
            "review_policy": saved.unwrap_or(legacy),
            "legacy_review_policy": legacy,
            "review_source": if saved.is_some() { "planen" } else { "legacy" },
            "timezone": data.get("timezone"),
            "subtitles_enabled": data.get("subtitles_enabled"),
            "release_backlog": false,
        });
        match canonical {
            Some(state) => match &state.settings {
                None => Outcome::unresolved("canonical_settings_unreadable", Some(subject)),
                Some((settings, _)) => {
                    if text(data, "timezone").is_some_and(|zone| zone != settings.timezone) {
                        Outcome::unresolved("conflicts_with_canonical_timezone", Some(subject))
                    } else {
                        Outcome::mapped(subject)
                    }
                }
            },
            None => Outcome::mapped(subject),
        }
    }

    fn map_schedule(&self, owner: &str, row: &LegacyRow) -> Outcome {
        let data = &row.data;
        let platform = text(data, "platform").unwrap_or_default().to_string();
        let auto_post = data
            .get("auto_post")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let slots = match cadence_pattern(data) {
            Ok(slots) => slots,
            Err(reason) => return Outcome::unresolved(reason, None),
        };
        let subject = json!({
            "kind": "platform_schedule",
            "platform": platform,
            "target": auto_post,
            "slots": slots,
            "editable": true,
            "keep_committed_posts": true,
        });
        let Some(state) = self.canonical(owner).filter(|state| state.configured) else {
            return Outcome::mapped(subject);
        };
        let Some((settings, policy)) = &state.settings else {
            return Outcome::unresolved("canonical_settings_unreadable", Some(subject));
        };
        let canonical_slots = match policy.schedule_scope {
            PolicyScope::Shared => settings.schedule_slots.clone(),
            PolicyScope::PerPlatform => policy
                .platform_schedule_slots
                .get(&platform)
                .cloned()
                .unwrap_or_default(),
        };
        let slots_conflict =
            auto_post && !canonical_slots.is_empty() && sorted(canonical_slots) != slots;
        let target_conflict = !settings.target_platforms.is_empty()
            && settings.target_platforms.contains(&platform) != auto_post;
        if slots_conflict || target_conflict {
            Outcome::unresolved("conflicts_with_canonical_schedule", Some(subject))
        } else {
            Outcome::mapped(subject)
        }
    }

    fn map_setting(&self, owner: &str, row: &LegacyRow) -> Outcome {
        let Some(value) = row.data.get("value").filter(|value| valid_draft(value)) else {
            return Outcome::unresolved("invalid_tiktok_payload", None);
        };
        let mut choices = value.clone();
        if let Some(fields) = choices.as_object_mut() {
            fields.retain(|key, _| !key.starts_with("music_consent"));
        }
        if row.key.starts_with("tiktok_defaults:") {
            return self.map_defaults(owner, value, choices);
        }
        let clip = row
            .key
            .rsplit(':')
            .next()
            .and_then(|id| id.parse::<i64>().ok());
        let subject = json!({
            "kind": "tiktok_draft",
            "clip_db_id": clip,
            "choices": choices,
            "music_consent": {
                "scope": "account",
                "given": value.get("music_consent").and_then(Value::as_bool).unwrap_or(false),
                "at": value.get("music_consent_at"),
                "platform_user_id": value.get("music_consent_platform_user_id"),
            },
            "authorization": false,
        });
        let Some(clip_row) = clip.and_then(|id| self.clips.get(&id)) else {
            return Outcome::unresolved("draft_clip_missing", Some(subject));
        };
        if clip_row.owner.as_deref() != Some(owner) {
            return Outcome::unresolved("draft_owner_mismatch", Some(subject));
        }
        Outcome::mapped(subject)
    }

    fn map_defaults(&self, owner: &str, value: &Value, choices: Value) -> Outcome {
        let description = text(value, "standard_description").unwrap_or_default();
        let subject = json!({
            "kind": "tiktok_defaults",
            "choices": choices,
            "text_default_override": {"platform": "tiktok", "description": description},
            "music_consent_scope": "account",
            "authorization": false,
        });
        let existing = self
            .canonical(owner)
            .and_then(|state| state.settings.as_ref())
            .and_then(|(_, policy)| policy.text_defaults.per_platform.get("tiktok"))
            .map(|defaults| defaults.description.as_str())
            .unwrap_or_default();
        if !existing.is_empty() && existing != description {
            Outcome::unresolved("conflicts_with_canonical_text_default", Some(subject))
        } else {
            Outcome::mapped(subject)
        }
    }

    fn reconciliation(&self, owner: &str, rows: &[&PipelineMigrationMapping]) -> Value {
        let mut counts = BTreeMap::<String, DispositionCounts>::new();
        for mapping in rows {
            count(
                counts.entry(mapping.legacy_table.clone()).or_default(),
                mapping.disposition,
            );
        }
        let unresolved = rows
            .iter()
            .filter(|mapping| mapping.disposition == MigrationDisposition::Unresolved)
            .count();
        json!({
            "migration_version": MIGRATION_VERSION,
            "counts": counts,
            "unresolved": unresolved,
            "ready": unresolved == 0,
            "proposed_policy": self.proposed_policy(owner, rows),
        })
    }

    fn proposed_policy(&self, owner: &str, rows: &[&PipelineMigrationMapping]) -> Value {
        let canonical = self
            .canonical(owner)
            .and_then(|state| state.settings.as_ref());
        let mut policy = canonical
            .map(|(_, policy)| policy.clone())
            .unwrap_or_default();
        let saved = self.policy_saved(owner);
        if !saved {
            policy.pipeline_mode = PipelineMode::Upload;
        }
        let source = if saved { "planen" } else { "legacy" };
        let mut timezone = canonical.map(|(settings, _)| settings.timezone.clone());
        let mut targets: BTreeSet<String> = canonical
            .map(|(settings, _)| settings.target_platforms.iter().cloned().collect())
            .unwrap_or_default();
        let mut patterns = canonical
            .map(|(settings, policy)| canonical_patterns(settings, policy, &targets))
            .unwrap_or_default();
        let mut legacy_schedule = false;
        for mapping in rows {
            let Some(subject) = &mapping.canonical_subject else {
                continue;
            };
            if mapping.unresolved_reason.as_deref() == Some("conflicts_with_canonical_timezone") {
                if let Some(review) = subject
                    .get("review_policy")
                    .and_then(|value| serde_json::from_value(value.clone()).ok())
                {
                    policy.review_policy = review;
                }
            }
            if mapping.disposition != MigrationDisposition::Mapped {
                continue;
            }
            match text(subject, "kind") {
                Some("channel_review") => {
                    if let Some(review) = subject
                        .get("review_policy")
                        .and_then(|value| serde_json::from_value(value.clone()).ok())
                    {
                        policy.review_policy = review;
                    }
                    if let Some(zone) = text(subject, "timezone") {
                        timezone = Some(zone.to_string());
                    }
                }
                Some("platform_schedule") if subject["target"] == Value::Bool(true) => {
                    let platform = text(subject, "platform").unwrap_or_default().to_string();
                    let slots = subject
                        .get("slots")
                        .and_then(|value| serde_json::from_value(value.clone()).ok())
                        .unwrap_or_default();
                    targets.insert(platform.clone());
                    patterns.insert(platform, slots);
                    legacy_schedule = true;
                }
                Some("tiktok_defaults") => {
                    let description = subject["text_default_override"]["description"]
                        .as_str()
                        .unwrap_or_default();
                    if !description.is_empty() {
                        policy
                            .text_defaults
                            .per_platform
                            .entry("tiktok".to_string())
                            .or_default()
                            .description = description.to_string();
                    }
                }
                _ => {}
            }
        }
        if !legacy_schedule {
            return json!({
                "policy": policy,
                "policy_source": source,
                "timezone": timezone,
                "target_platforms": targets,
                "schedule_slots": canonical
                    .map(|(settings, _)| settings.schedule_slots.clone())
                    .unwrap_or_default(),
            });
        }
        let first = patterns.values().next().cloned();
        let shared_slots = if patterns.values().all(|slots| Some(slots) == first.as_ref()) {
            policy.schedule_scope = PolicyScope::Shared;
            policy.platform_schedule_slots.clear();
            first.unwrap_or_default()
        } else {
            policy.schedule_scope = PolicyScope::PerPlatform;
            policy.platform_schedule_slots = patterns.clone();
            Vec::new()
        };
        policy.distribution_policy = DistributionPolicy::for_scope(policy.schedule_scope);
        json!({
            "policy": policy,
            "policy_source": source,
            "timezone": timezone,
            "target_platforms": targets,
            "schedule_slots": shared_slots,
        })
    }
}

fn canonical_patterns(
    settings: &AutopilotSettings,
    policy: &PipelinePolicy,
    targets: &BTreeSet<String>,
) -> BTreeMap<String, Vec<ScheduleSlot>> {
    targets
        .iter()
        .map(|platform| {
            let slots = match policy.schedule_scope {
                PolicyScope::Shared => settings.schedule_slots.clone(),
                PolicyScope::PerPlatform => policy
                    .platform_schedule_slots
                    .get(platform)
                    .cloned()
                    .unwrap_or_default(),
            };
            (platform.clone(), sorted(slots))
        })
        .collect()
}

fn map_queue(row: &LegacyRow) -> Outcome {
    let data = &row.data;
    let status = text(data, "status").unwrap_or_default();
    let subject = json!({
        "kind": "publication",
        "clip_db_id": data.get("clip_id"),
        "platform": data.get("platform"),
        "legacy_status": status,
        "scheduled_at": data.get("scheduled_at"),
        "priority": data.get("priority"),
        "attempts": data.get("attempts"),
        "payload": {
            "title": data.get("title"),
            "description": data.get("description"),
            "hashtags": data.get("hashtags"),
            "tiktok_post_options": data.get("tiktok_post_options"),
            "youtube_visibility": data.get("youtube_visibility"),
        },
        "external_id": data.get("external_id"),
        "tiktok_publish_id": data.get("tiktok_publish_id"),
        "account_id": data.get("account_id"),
        "inbox_only": INBOX_QUEUE.contains(&status),
    });
    let effect_started = !data.get("effect_started_at").is_none_or(Value::is_null);
    if FINAL_QUEUE.contains(&status) {
        Outcome::history(subject)
    } else if status == "processing" {
        Outcome::unresolved("in_flight_claim", Some(subject))
    } else if effect_started {
        Outcome::unresolved("uncertain_effect", Some(subject))
    } else if LIVE_QUEUE.contains(&status) || INBOX_QUEUE.contains(&status) {
        Outcome::mapped(subject)
    } else {
        Outcome::unresolved("unknown_queue_status", Some(subject))
    }
}

fn map_enrichment(row: &LegacyRow) -> Outcome {
    let data = &row.data;
    let mut platforms = serde_json::Map::new();
    for platform in PLATFORMS {
        platforms.insert(
            platform.to_string(),
            json!({
                "title": data.get(format!("title_{platform}")),
                "description": data.get(format!("description_{platform}")),
                "hashtags": data.get(format!("hashtags_{platform}")),
            }),
        );
    }
    Outcome::mapped(json!({
        "kind": "platform_text_input",
        "clip_db_id": data.get("clip_db_id"),
        "status": data.get("status"),
        "platforms": platforms,
    }))
}

fn map_category(row: &LegacyRow) -> Outcome {
    let data = &row.data;
    let auto_post = data
        .get("auto_post")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if text(data, "category_key") == Some("deadlock") {
        Outcome::mapped(json!({
            "kind": "intake",
            "category_key": "deadlock",
            "intake": if auto_post { "automatic" } else { "manual" },
        }))
    } else {
        Outcome::history(json!({
            "kind": "intake",
            "category_key": data.get("category_key"),
            "intake": "visible_only",
            "legacy_auto_post": auto_post,
        }))
    }
}

pub fn cadence_pattern(data: &Value) -> Result<Vec<ScheduleSlot>, &'static str> {
    let per_week = int(data, "posts_per_week").unwrap_or(0).max(0) as usize;
    let max_per_day = int(data, "max_posts_per_day").unwrap_or(0).max(0) as usize;
    let mut times = BTreeSet::new();
    for time in data
        .get("post_times")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let parsed = time
            .as_str()
            .and_then(|time| NaiveTime::parse_from_str(time.trim(), "%H:%M").ok())
            .ok_or("invalid_post_time")?;
        times.insert(parsed.format("%H:%M").to_string());
    }
    if per_week == 0 {
        return Ok(Vec::new());
    }
    if times.is_empty() {
        return Err("missing_post_times");
    }
    let per_day = max_per_day.min(times.len());
    if per_day == 0 {
        return Err("cadence_without_daily_capacity");
    }
    let days = per_week.div_ceil(per_day).min(7);
    let mut remaining = per_week;
    let mut slots = Vec::new();
    for day in 0..days {
        let weekday = (day * 7 / days + 1) as u8;
        for time in times.iter().take(per_day.min(remaining)) {
            slots.push(ScheduleSlot {
                weekday,
                local_time: time.clone(),
            });
        }
        remaining = remaining.saturating_sub(per_day);
    }
    Ok(sorted(slots))
}

fn sorted(mut slots: Vec<ScheduleSlot>) -> Vec<ScheduleSlot> {
    slots.sort_by(|a, b| (a.weekday, &a.local_time).cmp(&(b.weekday, &b.local_time)));
    slots.dedup();
    slots
}

fn valid_draft(value: &Value) -> bool {
    value.as_object().is_some_and(|fields| {
        fields
            .keys()
            .all(|key| DRAFT_FIELDS.contains(&key.as_str()))
    })
}

fn resolve_text(
    candidates: &[(&'static str, Option<Value>)],
) -> (Option<(&'static str, Value)>, Option<Vec<&'static str>>) {
    let present: Vec<(&'static str, Value)> = candidates
        .iter()
        .filter_map(|(origin, value)| value.clone().map(|value| (*origin, value)))
        .collect();
    let Some(first) = present.first().cloned() else {
        return (None, None);
    };
    let conflict = present.iter().any(|(_, value)| *value != first.1);
    let origins = conflict.then(|| present.iter().map(|(origin, _)| *origin).collect());
    (Some(first), origins)
}

fn queue_text(data: &Value, field: &str) -> Option<Value> {
    match field {
        "hashtags" => text_hashtags(text(data, "hashtags")),
        _ => non_empty(text(data, field)),
    }
}

fn enrichment_text(data: &Value, field: &str, platform: &str) -> Option<Value> {
    let value = data.get(format!("{field}_{platform}"))?;
    match field {
        "hashtags" => {
            let tags: Vec<Value> = value
                .as_array()?
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|tag| !tag.is_empty())
                .map(|tag| Value::String(tag.to_string()))
                .collect();
            (!tags.is_empty()).then_some(Value::Array(tags))
        }
        _ => non_empty(value.as_str()),
    }
}

fn clip_field_text(data: &Value, field: &str) -> Option<Value> {
    match field {
        "title" => non_empty(text(data, "custom_title")),
        "description" => non_empty(text(data, "custom_description")),
        _ => text_hashtags(text(data, "hashtags")),
    }
}

fn text_hashtags(raw: Option<&str>) -> Option<Value> {
    let tags: Vec<Value> = raw?
        .split(|c: char| c.is_whitespace() || c == ',')
        .map(str::trim)
        .filter(|tag| !tag.is_empty())
        .map(|tag| Value::String(tag.to_string()))
        .collect();
    (!tags.is_empty()).then_some(Value::Array(tags))
}

fn non_empty(value: Option<&str>) -> Option<Value> {
    value
        .filter(|value| !value.trim().is_empty())
        .map(|value| Value::String(value.to_string()))
}

fn merge(mut base: Value, extra: Value) -> Value {
    if let (Some(base), Value::Object(extra)) = (base.as_object_mut(), extra) {
        base.extend(extra);
    }
    base
}

fn text<'v>(value: &'v Value, key: &str) -> Option<&'v str> {
    value.get(key).and_then(Value::as_str)
}

fn int(value: &Value, key: &str) -> Option<i64> {
    value.get(key).and_then(Value::as_i64)
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct LegacyReviewClearReport {
    pub jobs_cleared: u64,
    pub clips_unpinned: u64,
    pub awaiting_returned: u64,
}

fn legacy_jam_reasons() -> Vec<String> {
    let mut reasons = AiEditorError::failure_messages();
    reasons.extend(
        LEGACY_RUNNER_FAILURES
            .iter()
            .map(|reason| reason.to_string()),
    );
    reasons
}

pub async fn clear_legacy_review_jam(
    pool: &PgPool,
) -> Result<LegacyReviewClearReport, MigrationError> {
    let mut tx = pool.begin().await?;
    let cancelled: Vec<(String, i64)> = sqlx::query_as(
        "WITH jammed AS (
            SELECT j.id FROM social_production_jobs j
            WHERE j.status = 'review'
            AND j.review_reasons ?| $1::text[]
            AND NOT j.review_reasons ? $2
            AND NOT EXISTS (
                SELECT 1 FROM social_produced_videos v
                WHERE v.twitch_user_id = j.twitch_user_id
                AND social_video_job(v.job_id, v.metadata) = j.id
                AND v.file_path IS NOT NULL
                AND v.status NOT IN ('rejected', 'expired')
            )
            FOR UPDATE OF j
        )
        UPDATE social_production_jobs j
        SET status = CASE WHEN j.source = 'test' THEN 'failed' ELSE 'cancelled' END,
            stage = CASE WHEN j.source = 'test' THEN 'failed' ELSE 'cleared_no_video' END,
            last_error = COALESCE(j.last_error, j.review_reasons->>0),
            finished_at = COALESCE(j.finished_at, NOW()), scheduled_at = NULL
        FROM jammed WHERE j.id = jammed.id
        RETURNING j.twitch_user_id, j.clip_id",
    )
    .bind(legacy_jam_reasons())
    .bind(UNSUITABLE_REASON)
    .fetch_all(&mut *tx)
    .await?;
    let clip_ids: Vec<i64> = cancelled.iter().map(|(_, clip_id)| *clip_id).collect();
    let clips_unpinned = if clip_ids.is_empty() {
        0
    } else {
        sqlx::query(
            "UPDATE social_clip_pool p SET selected_at = NULL
            WHERE p.clip_db_id = ANY($1)
            AND p.selected_at IS NOT NULL
            AND NOT EXISTS (
                SELECT 1 FROM social_production_jobs other
                WHERE other.twitch_user_id = p.twitch_user_id AND other.clip_id = p.clip_db_id
                AND other.status IN ('queued', 'running', 'review')
            )",
        )
        .bind(&clip_ids)
        .execute(&mut *tx)
        .await?
        .rows_affected()
    };
    let awaiting_returned = sqlx::query(
        "UPDATE twitch_clips_social_media c SET status = 'pool'
        WHERE c.status = 'awaiting_approval'
        AND EXISTS (
            SELECT 1 FROM social_pipeline_cutover o
            WHERE o.twitch_user_id = c.twitch_user_id AND o.state = 'unified'
        )
        AND NOT EXISTS (
            SELECT 1 FROM social_produced_videos v
            WHERE v.twitch_user_id = c.twitch_user_id AND v.clip_id = c.id
            AND v.file_path IS NOT NULL
            AND v.status NOT IN ('rejected', 'expired')
        )",
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();
    tx.commit().await?;
    Ok(LegacyReviewClearReport {
        jobs_cleared: cancelled.len() as u64,
        clips_unpinned,
        awaiting_returned,
    })
}
