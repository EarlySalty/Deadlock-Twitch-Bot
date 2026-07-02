//! Approval-Workflow (Port der DB-/State-Logik aus
//! `bot/social_media/approval/approval_service.py`).
//!
//! Zustandsmaschine je Clip in `social_media_clip_approval`
//! (awaiting_approval → approved/skipped/editing) + das Einreihen freigegebener
//! Plattformen in die Upload-Queue. Der Discord-DM-/UI-Teil (Approval-Buttons,
//! DM-Versand) ist **B10 (von Nani ausgeschlossen)** und nicht portiert.

use serde_json::{json, Value};
use sqlx::PgPool;

use crate::clip_queue::queue_upload;
use crate::enrichment::get_enrichment;
use crate::settings::{get_auto_approve_settings, AutoApprove};

pub const STATE_AWAITING: &str = "awaiting_approval";
pub const STATE_APPROVED: &str = "approved";
pub const STATE_SKIPPED: &str = "skipped";
pub const STATE_EDITING: &str = "editing";

pub const DECISION_APPROVE: &str = "approve";
pub const DECISION_SKIP: &str = "skip";
pub const DECISION_EDIT: &str = "edit";

pub const SUPPORTED_PLATFORMS: [&str; 3] = ["youtube", "tiktok", "instagram"];

#[derive(Debug, thiserror::Error)]
pub enum ApprovalError {
    #[error("clip_db_id {0} not found")]
    ClipNotFound(i32),
    #[error("clip_db_id {0} is out of range for social_media_clip_approval.clip_db_id")]
    ClipIdOutOfRange(i64),
    #[error("at least one platform must be approved")]
    NoPlatform,
    #[error("db: {0}")]
    Db(#[from] sqlx::Error),
}

/// Approval-Zustand eines Clips.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalRecord {
    pub clip_db_id: i32,
    pub state: String,
    pub approved_platforms: Vec<String>,
    pub approver_user_id: Option<String>,
    pub decided_at: Option<String>,
    pub dm_message_id: Option<String>,
    pub dm_channel_id: Option<String>,
    pub last_sent_at: Option<String>,
}

/// Trimmt, kleinschreibt, dedupliziert, nur unterstützte Plattformen.
fn normalize_platforms<I: IntoIterator<Item = String>>(platforms: I) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for p in platforms {
        let p = p.trim().to_lowercase();
        if SUPPORTED_PLATFORMS.contains(&p.as_str()) && seen.insert(p.clone()) {
            out.push(p);
        }
    }
    out
}

/// Mappt Eingabe-Decision auf den kanonischen Wert.
fn normalize_decision(decision: &str) -> &'static str {
    match decision.trim().to_lowercase().as_str() {
        "approve" | "approved" => DECISION_APPROVE,
        "skip" | "skipped" => DECISION_SKIP,
        "edit" | "editing" => DECISION_EDIT,
        _ => DECISION_APPROVE, // Python-Default
    }
}

fn decode_platforms(raw: Option<&str>) -> Vec<String> {
    let parsed: Vec<String> = raw
        .and_then(|s| serde_json::from_str::<Vec<String>>(s).ok())
        .unwrap_or_default();
    normalize_platforms(parsed)
}

type Row = (
    i32,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

fn row_to_record(r: Row) -> ApprovalRecord {
    ApprovalRecord {
        clip_db_id: r.0,
        state: r.1.unwrap_or_else(|| STATE_AWAITING.to_string()),
        approved_platforms: decode_platforms(r.2.as_deref()),
        approver_user_id: r.3,
        decided_at: r.4,
        dm_message_id: r.5,
        dm_channel_id: r.6,
        last_sent_at: r.7,
    }
}

const SELECT_SQL: &str = "SELECT clip_db_id, state, approved_platforms::text, approver_user_id, \
    decided_at::text, dm_message_id, dm_channel_id, last_sent_at::text \
    FROM social_media_clip_approval WHERE clip_db_id = $1 LIMIT 1";

/// Approval-Zeile eines Clips.
pub async fn get_approval_record(pool: &PgPool, clip_db_id: i32) -> Option<ApprovalRecord> {
    match fetch_approval_record(pool, clip_db_id).await {
        Ok(record) => record,
        Err(error) => {
            tracing::warn!(
                %error,
                clip_db_id,
                "Social-Media-Approval: Approval-Zeile nicht ladbar"
            );
            None
        }
    }
}

async fn fetch_approval_record(
    pool: &PgPool,
    clip_db_id: i32,
) -> Result<Option<ApprovalRecord>, sqlx::Error> {
    let row: Option<Row> = sqlx::query_as(SELECT_SQL)
        .bind(clip_db_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(row_to_record))
}

/// Stellt eine (awaiting-)Approval-Zeile sicher.
pub async fn ensure_approval_row(pool: &PgPool, clip_db_id: i32) -> ApprovalRecord {
    if let Some(r) = get_approval_record(pool, clip_db_id).await {
        return r;
    }
    if let Err(error) = sqlx::query!(
        "INSERT INTO social_media_clip_approval (clip_db_id, state, approved_platforms) \
         VALUES ($1, $2, '[]'::jsonb) ON CONFLICT (clip_db_id) DO NOTHING",
        clip_db_id,
        STATE_AWAITING
    )
    .execute(pool)
    .await
    {
        tracing::warn!(
            %error,
            clip_db_id,
            "Social-Media-Approval: Awaiting-Zeile konnte nicht sichergestellt werden"
        );
    }
    get_approval_record(pool, clip_db_id)
        .await
        .unwrap_or(ApprovalRecord {
            clip_db_id,
            state: STATE_AWAITING.to_string(),
            approved_platforms: Vec::new(),
            approver_user_id: None,
            decided_at: None,
            dm_message_id: None,
            dm_channel_id: None,
            last_sent_at: None,
        })
}

/// Setzt den Clip (zurück) auf „wartet auf Freigabe" (Python
/// `mark_clip_awaiting_approval`; ersetzt den Orchestrator-Stub).
pub async fn mark_clip_awaiting_approval(pool: &PgPool, clip_db_id: i32) {
    ensure_approval_row(pool, clip_db_id).await;
    if let Err(error) = sqlx::query!(
        "UPDATE social_media_clip_approval SET state = $1, approver_user_id = NULL, \
         decided_at = NULL, approved_platforms = '[]'::jsonb, dm_message_id = NULL, \
         dm_channel_id = NULL, last_sent_at = NULL WHERE clip_db_id = $2 AND state <> $1",
        STATE_AWAITING,
        clip_db_id
    )
    .execute(pool)
    .await
    {
        tracing::warn!(
            %error,
            clip_db_id,
            "Social-Media-Approval: Awaiting-Status konnte nicht gesetzt werden"
        );
    }
    if let Err(error) = sqlx::query!(
        "UPDATE twitch_clips_social_media SET status = $1 WHERE id = $2 \
         AND COALESCE(status, '') NOT IN ('published_all', 'published_partial', 'discarded')",
        STATE_AWAITING,
        clip_db_id as i64
    )
    .execute(pool)
    .await
    {
        tracing::warn!(
            %error,
            clip_db_id,
            "Social-Media-Approval: Clip-Status konnte nicht auf awaiting gesetzt werden"
        );
    }
}

/// Freigegebene Clips, die noch in die Queue müssen.
pub async fn iter_approved_clips_pending_queue(pool: &PgPool, limit: i64) -> Vec<i32> {
    sqlx::query_scalar!(
        "SELECT clip_db_id FROM social_media_clip_approval WHERE state = $1 \
         ORDER BY CASE WHEN decided_at IS NULL THEN 1 ELSE 0 END, decided_at DESC, clip_db_id DESC \
         LIMIT $2",
        STATE_APPROVED,
        limit.max(1)
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default()
}

/// `true`, wenn der Clip für diese Plattform freigegeben ist.
pub async fn is_clip_approved_for(
    pool: &PgPool,
    clip_db_id: i64,
    platform: &str,
) -> Result<bool, ApprovalError> {
    let approval_clip_db_id =
        i32::try_from(clip_db_id).map_err(|_| ApprovalError::ClipIdOutOfRange(clip_db_id))?;
    let platform = platform.trim().to_lowercase();
    if !SUPPORTED_PLATFORMS.contains(&platform.as_str()) {
        return Ok(false);
    }
    Ok(
        match fetch_approval_record(pool, approval_clip_db_id).await? {
            Some(r) if r.state == STATE_APPROVED => r.approved_platforms.contains(&platform),
            _ => false,
        },
    )
}

/// Serialisiert den Record fürs Dashboard.
pub fn serialize_approval_record(record: &ApprovalRecord) -> Value {
    json!({
        "clip_db_id": record.clip_db_id,
        "state": record.state,
        "approved_platforms": record.approved_platforms,
        "approver_user_id": record.approver_user_id,
        "decided_at": record.decided_at,
        "dm_message_id": record.dm_message_id,
        "dm_channel_id": record.dm_channel_id,
        "last_sent_at": record.last_sent_at,
    })
}

fn auto_platforms(settings: AutoApprove) -> Vec<String> {
    let mut out = Vec::new();
    if settings.youtube {
        out.push("youtube".to_string());
    }
    if settings.tiktok {
        out.push("tiktok".to_string());
    }
    if settings.instagram {
        out.push("instagram".to_string());
    }
    out
}

/// Verarbeitet eine Approval-Entscheidung (Python `handle_decision`): setzt
/// State + Plattformen + Clip-Status; bei approve werden die Uploads eingereiht.
pub async fn handle_decision(
    pool: &PgPool,
    clip_db_id: i32,
    decision: &str,
    approved_platforms: &[String],
    user_id: Option<&str>,
) -> Result<ApprovalRecord, ApprovalError> {
    if !clip_exists(pool, clip_db_id).await {
        return Err(ApprovalError::ClipNotFound(clip_db_id));
    }
    let decision = normalize_decision(decision);
    let selected = normalize_platforms(approved_platforms.iter().cloned());

    let (final_platforms, next_state) = match decision {
        DECISION_APPROVE => {
            let mut combined = selected;
            combined.extend(auto_platforms(get_auto_approve_settings(pool).await));
            let final_platforms = normalize_platforms(combined);
            if final_platforms.is_empty() {
                return Err(ApprovalError::NoPlatform);
            }
            (final_platforms, STATE_APPROVED)
        }
        DECISION_SKIP => (Vec::new(), STATE_SKIPPED),
        _ => (selected, STATE_EDITING),
    };

    ensure_approval_row(pool, clip_db_id).await;
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query!(
        "UPDATE social_media_clip_approval SET state = $1, approved_platforms = $2::text::jsonb, \
         approver_user_id = $3, decided_at = $4::text::timestamptz WHERE clip_db_id = $5",
        next_state,
        serde_json::to_string(&final_platforms).unwrap_or_else(|_| "[]".to_string()),
        user_id.map(str::trim).filter(|s| !s.is_empty()),
        &now,
        clip_db_id
    )
    .execute(pool)
    .await?;
    sqlx::query!(
        "UPDATE twitch_clips_social_media SET status = $1 WHERE id = $2",
        next_state,
        clip_db_id as i64
    )
    .execute(pool)
    .await?;

    if decision == DECISION_APPROVE {
        ensure_queued_uploads(pool, clip_db_id).await;
    }
    Ok(ensure_approval_row(pool, clip_db_id).await)
}

/// Reiht die freigegebenen Plattformen eines Clips in die Upload-Queue
/// (Python `ensure_queued_uploads`). Nutzt die Enrichment-Texte je Plattform.
pub async fn ensure_queued_uploads(pool: &PgPool, clip_db_id: i32) -> Vec<(String, i64)> {
    if !clip_exists(pool, clip_db_id).await {
        return Vec::new();
    }
    let record = ensure_approval_row(pool, clip_db_id).await;
    if record.state != STATE_APPROVED {
        return Vec::new();
    }
    let enrichment = get_enrichment(pool, clip_db_id).await;
    let mut queued = Vec::new();
    for platform in &record.approved_platforms {
        match is_clip_approved_for(pool, i64::from(clip_db_id), platform).await {
            Ok(true) => {}
            Ok(false) => continue,
            Err(e) => {
                tracing::warn!(clip_db_id, platform = %platform, %e, "approval check failed while queuing uploads");
                continue;
            }
        }
        if upload_already_exists(pool, clip_db_id, platform).await {
            continue;
        }
        let (title, description, hashtags) =
            enrichment
                .as_ref()
                .map_or((None, None, Vec::new()), |e| match platform.as_str() {
                    "youtube" => (
                        e.title_youtube.clone(),
                        e.description_youtube.clone(),
                        e.hashtags_youtube.clone(),
                    ),
                    "tiktok" => (
                        e.title_tiktok.clone(),
                        e.description_tiktok.clone(),
                        e.hashtags_tiktok.clone(),
                    ),
                    _ => (
                        e.title_instagram.clone(),
                        e.description_instagram.clone(),
                        e.hashtags_instagram.clone(),
                    ),
                });
        match queue_upload(
            pool,
            clip_db_id,
            platform,
            title.as_deref(),
            description.as_deref(),
            Some(&hashtags),
            None,
            0,
        )
        .await
        {
            Ok(queue_id) => queued.push((platform.clone(), queue_id)),
            Err(error) => {
                tracing::warn!(
                    %error,
                    clip_db_id,
                    platform = %platform,
                    "Social-Media-Approval: Upload konnte nicht eingereiht werden"
                );
            }
        }
    }
    queued
}

async fn clip_exists(pool: &PgPool, clip_db_id: i32) -> bool {
    sqlx::query_scalar!(
        "SELECT 1 AS \"exists!\" FROM twitch_clips_social_media WHERE id = $1",
        clip_db_id as i64
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .is_some()
}

/// `true`, wenn die Plattform schon hochgeladen ODER eine nicht-failed
/// Queue-Zeile existiert (Python `_upload_already_exists`).
async fn upload_already_exists(pool: &PgPool, clip_db_id: i32, platform: &str) -> bool {
    let column = match platform {
        "youtube" => "uploaded_youtube",
        "tiktok" => "uploaded_tiktok",
        "instagram" => "uploaded_instagram",
        _ => return true,
    };
    let row: Option<(Option<bool>, bool)> = sqlx::query_as(&format!(
        "SELECT {column}, EXISTS(SELECT 1 FROM twitch_clips_upload_queue \
         WHERE clip_id = $1 AND platform = $2 AND status <> 'failed') \
         FROM twitch_clips_social_media WHERE id = $3 LIMIT 1"
    ))
    .bind(clip_db_id as i64)
    .bind(platform)
    .bind(clip_db_id as i64)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    match row {
        Some((uploaded, has_queue)) => uploaded.unwrap_or(false) || has_queue,
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
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
        let opts = PgConnectOptions::from_str(&dsn)
            .unwrap()
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(3)
            .connect_with(opts)
            .await
            .unwrap();
        for ddl in [
            "CREATE TABLE twitch_clips_social_media (id SERIAL PRIMARY KEY, status TEXT DEFAULT 'pending', uploaded_tiktok BOOLEAN DEFAULT FALSE, uploaded_youtube BOOLEAN DEFAULT FALSE, uploaded_instagram BOOLEAN DEFAULT FALSE)",
            "CREATE TABLE social_media_clip_approval (clip_db_id INTEGER PRIMARY KEY, state TEXT NOT NULL DEFAULT 'awaiting_approval', approved_platforms JSONB NOT NULL DEFAULT '[]'::jsonb, approver_user_id TEXT, decided_at TIMESTAMPTZ, dm_message_id TEXT, dm_channel_id TEXT, last_sent_at TIMESTAMPTZ)",
            "CREATE TABLE twitch_clips_upload_queue (id SERIAL PRIMARY KEY, clip_id INTEGER, platform TEXT, status TEXT DEFAULT 'pending', priority INTEGER DEFAULT 0, title TEXT, description TEXT, hashtags TEXT, scheduled_at TIMESTAMPTZ, attempts INTEGER DEFAULT 0, last_error TEXT, last_attempt_at TIMESTAMPTZ, created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP, completed_at TIMESTAMPTZ)",
            "CREATE TABLE social_media_settings (key TEXT PRIMARY KEY, value JSONB, updated_at TIMESTAMPTZ, updated_by TEXT)",
            "CREATE TABLE social_media_clip_enrichment (clip_db_id INTEGER PRIMARY KEY, transcript_raw TEXT, transcript_corrected TEXT, transcript_segments JSONB, transcript_lang TEXT, detected_terms JSONB DEFAULT '[]'::jsonb, title_youtube TEXT, title_tiktok TEXT, title_instagram TEXT, description_youtube TEXT, description_tiktok TEXT, description_instagram TEXT, hashtags_youtube JSONB DEFAULT '[]'::jsonb, hashtags_tiktok JSONB DEFAULT '[]'::jsonb, hashtags_instagram JSONB DEFAULT '[]'::jsonb, llm_provider TEXT, llm_model TEXT, cost_usd_estimate NUMERIC(10,6), status TEXT DEFAULT 'pending', error_message TEXT, started_at TIMESTAMPTZ, completed_at TIMESTAMPTZ, edited_by TEXT, updated_at TIMESTAMPTZ DEFAULT NOW())",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        Some(pool)
    }

    async fn seed_clip(pool: &PgPool) -> i32 {
        sqlx::query_scalar("INSERT INTO twitch_clips_social_media DEFAULT VALUES RETURNING id")
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn approve_setzt_state_und_queued_uploads() {
        let Some(pool) = make_pool("t_sm_approval").await else {
            return;
        };
        let clip = seed_clip(&pool).await;
        sqlx::query("INSERT INTO social_media_clip_enrichment (clip_db_id, title_youtube, hashtags_youtube) VALUES ($1, 'YT-Titel', '[\"#deadlock\"]'::jsonb)").bind(clip).execute(&pool).await.unwrap();

        let rec = handle_decision(
            &pool,
            clip,
            "approve",
            &["youtube".to_string()],
            Some("admin"),
        )
        .await
        .unwrap();
        assert_eq!(rec.state, "approved");
        assert_eq!(rec.approved_platforms, vec!["youtube".to_string()]);
        assert_eq!(rec.approver_user_id.as_deref(), Some("admin"));
        // Clip-Status approved.
        let cstatus: String =
            sqlx::query_scalar("SELECT status FROM twitch_clips_social_media WHERE id = $1")
                .bind(clip)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(cstatus, "approved");
        // Upload eingereiht mit Enrichment-Titel.
        let (platform, title): (String, Option<String>) = sqlx::query_as(
            "SELECT platform, title FROM twitch_clips_upload_queue WHERE clip_id = $1",
        )
        .bind(clip)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(platform, "youtube");
        assert_eq!(title.as_deref(), Some("YT-Titel"));
        assert!(is_clip_approved_for(&pool, i64::from(clip), "youtube")
            .await
            .unwrap());
        assert!(!is_clip_approved_for(&pool, i64::from(clip), "tiktok")
            .await
            .unwrap());

        // ensure_queued_uploads erneut → idempotent (kein zweiter Job).
        ensure_queued_uploads(&pool, clip).await;
        let n: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM twitch_clips_upload_queue WHERE clip_id = $1")
                .bind(clip)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(n, 1);

        // iter_approved findet den Clip.
        assert_eq!(
            iter_approved_clips_pending_queue(&pool, 10).await,
            vec![clip]
        );
    }

    #[tokio::test]
    async fn skip_und_approve_ohne_plattform() {
        let Some(pool) = make_pool("t_sm_approval_skip").await else {
            return;
        };
        let clip = seed_clip(&pool).await;
        let rec = handle_decision(&pool, clip, "skip", &[], None)
            .await
            .unwrap();
        assert_eq!(rec.state, "skipped");
        assert!(rec.approved_platforms.is_empty());
        // approve ohne ausgewählte Plattform + ohne Auto-Approve → Fehler.
        assert!(matches!(
            handle_decision(&pool, clip, "approve", &[], None).await,
            Err(ApprovalError::NoPlatform)
        ));
        // Unbekannter Clip → ClipNotFound.
        assert!(matches!(
            handle_decision(&pool, 999, "approve", &["youtube".to_string()], None).await,
            Err(ApprovalError::ClipNotFound(999))
        ));
    }

    #[tokio::test]
    async fn mark_awaiting_und_record() {
        let Some(pool) = make_pool("t_sm_approval_mark").await else {
            return;
        };
        let clip = seed_clip(&pool).await;
        handle_decision(&pool, clip, "approve", &["tiktok".to_string()], None)
            .await
            .unwrap();
        // mark_awaiting setzt zurück.
        mark_clip_awaiting_approval(&pool, clip).await;
        let rec = get_approval_record(&pool, clip).await.unwrap();
        assert_eq!(rec.state, "awaiting_approval");
        assert!(rec.approved_platforms.is_empty());
        let cstatus: String =
            sqlx::query_scalar("SELECT status FROM twitch_clips_social_media WHERE id = $1")
                .bind(clip)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(cstatus, "awaiting_approval");
        // serialize.
        let v = serialize_approval_record(&rec);
        assert_eq!(v["state"], "awaiting_approval");
    }
}
