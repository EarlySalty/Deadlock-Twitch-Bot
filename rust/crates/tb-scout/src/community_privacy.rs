//! Export und Löschung ausschließlich der neuen Community-Vorschlagskopien.
use crate::community::{format_cursor, lock, naechster_stempel, VorschlagFehler};
use chrono::{DateTime, Utc};
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

fn hash(domain: &[u8], value: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(domain);
    hash.update(value.as_bytes());
    format!("{:x}", hash.finalize())
}
fn identity_hash(discord_id: &str) -> String {
    hash(b"community-scout-privacy-v1\0", discord_id)
}
fn key_hash(key: &str) -> String {
    hash(b"community-scout-erased-replay-v1\0", key)
}

pub(crate) async fn require_submission(
    tx: &mut Transaction<'_, Postgres>,
    discord_id: &str,
    key: &str,
    submitted_at: Option<DateTime<Utc>>,
    epoch: Option<i64>,
) -> Result<(), VorschlagFehler> {
    let erased: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM twitch_scout_erased_replay_keys WHERE key_hash=$1)",
    )
    .bind(key_hash(key))
    .fetch_one(&mut **tx)
    .await?;
    if erased {
        return Err(VorschlagFehler::PrivacyGesperrt);
    }
    let barrier: Option<(i64, Option<DateTime<Utc>>)> = sqlx::query_as(
        "SELECT epoch,activity_since FROM twitch_scout_community_privacy WHERE identity_hash=$1",
    )
    .bind(identity_hash(discord_id))
    .fetch_optional(&mut **tx)
    .await?;
    if let Some((current, floor)) = barrier {
        if epoch != Some(current)
            || !floor
                .zip(submitted_at)
                .is_some_and(|(floor, at)| at >= floor)
        {
            return Err(VorschlagFehler::PrivacyGesperrt);
        }
    } else if epoch.is_some_and(|epoch| epoch != 0) {
        return Err(VorschlagFehler::PrivacyGesperrt);
    }
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct PrivacyResult {
    pub discord_user_id: String,
    pub operation_id: String,
    pub epoch: i64,
    pub status: &'static str,
    pub activity_since: Option<String>,
}

/// UUID und Epoche stammen aus dem dauerhaft gespeicherten zentralen Auftrag.
/// Ein Retry benutzt dieselben Werte. Eine alte Operation verändert keinen neueren Stand.
pub async fn apply_operation(
    pool: &PgPool,
    discord_id: &str,
    operation_id: Uuid,
    epoch: i64,
    activity_since: Option<DateTime<Utc>>,
) -> Result<PrivacyResult, VorschlagFehler> {
    if epoch <= 0 {
        return Err(VorschlagFehler::Konflikt);
    }
    let mut tx = pool.begin().await?;
    lock(&mut tx).await?;
    let identity = identity_hash(discord_id);
    let previous: Option<(i64,Uuid,Option<DateTime<Utc>>)> = sqlx::query_as("SELECT epoch,operation_id,activity_since FROM twitch_scout_community_privacy WHERE identity_hash=$1")
        .bind(&identity).fetch_optional(&mut *tx).await?;
    let response = |epoch, status, floor: Option<DateTime<Utc>>| PrivacyResult {
        discord_user_id: discord_id.into(),
        operation_id: operation_id.to_string(),
        epoch,
        status,
        activity_since: floor.map(format_cursor),
    };
    if let Some((old_epoch, old_id, old_floor)) = previous {
        if epoch < old_epoch {
            return Ok(response(old_epoch, "stale", old_floor));
        }
        if epoch == old_epoch {
            if operation_id != old_id || activity_since != old_floor {
                return Err(VorschlagFehler::Konflikt);
            }
            return Ok(response(epoch, "replayed", old_floor));
        }
        if old_floor
            .zip(activity_since)
            .is_some_and(|(old, new)| new < old)
        {
            return Err(VorschlagFehler::Konflikt);
        }
    }
    if activity_since.is_none() {
        let keys: Vec<String> = sqlx::query_scalar("SELECT idempotency_key FROM twitch_scout_community_suggestions WHERE suggested_by_discord_id=$1")
            .bind(discord_id).fetch_all(&mut *tx).await?;
        let hashes: Vec<String> = keys.iter().map(|key| key_hash(key)).collect();
        sqlx::query("INSERT INTO twitch_scout_erased_replay_keys(key_hash) SELECT UNNEST($1::text[]) ON CONFLICT DO NOTHING")
            .bind(&hashes).execute(&mut *tx).await?;
        let channels: Vec<String> = sqlx::query_scalar("DELETE FROM twitch_scout_community_suggestions WHERE suggested_by_discord_id=$1 RETURNING twitch_user_id")
            .bind(discord_id).fetch_all(&mut *tx).await?;
        let stamp = naechster_stempel(&mut tx).await?;
        sqlx::query("WITH affected AS (SELECT c.streamer_login, ROW_NUMBER() OVER(ORDER BY c.streamer_login) AS n FROM twitch_scout_candidates c WHERE c.suggested_by_discord_id=$1 OR c.twitch_user_id=ANY($2))
            UPDATE twitch_scout_candidates c SET
            suggested_by_discord_id=CASE WHEN c.suggested_by_discord_id=$1 THEN NULL ELSE c.suggested_by_discord_id END,
            suggestion_reason=CASE WHEN c.suggested_by_discord_id=$1 THEN NULL ELSE c.suggestion_reason END,
            suggested_at=CASE WHEN c.suggested_by_discord_id=$1 THEN NULL ELSE c.suggested_at END,
            suggestion_count=(SELECT COUNT(DISTINCT s.suggested_by_discord_id)::int FROM twitch_scout_community_suggestions s WHERE s.twitch_user_id=c.twitch_user_id AND s.result_status IN ('created','already_known')),
            community_updated_at=CASE WHEN c.source='community' THEN $3 + affected.n * INTERVAL '1 microsecond' ELSE c.community_updated_at END
            FROM affected WHERE c.streamer_login=affected.streamer_login")
            .bind(discord_id).bind(&channels).bind(stamp).execute(&mut *tx).await?;
    }
    sqlx::query("INSERT INTO twitch_scout_community_privacy(identity_hash,epoch,operation_id,activity_since) VALUES ($1,$2,$3,$4)
        ON CONFLICT(identity_hash) DO UPDATE SET epoch=EXCLUDED.epoch,operation_id=EXCLUDED.operation_id,activity_since=EXCLUDED.activity_since")
        .bind(identity).bind(epoch).bind(operation_id).bind(activity_since).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(response(epoch, "applied", activity_since))
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ExportSuggestion {
    pub idempotency_key: String,
    pub twitch_user_id: String,
    pub twitch_login: String,
    pub reason: Option<String>,
    pub result_status: String,
    pub created_at: DateTime<Utc>,
    pub submitted_at: Option<DateTime<Utc>>,
}
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ExportCandidate {
    pub twitch_user_id: Option<String>,
    pub streamer_login: String,
    pub suggestion_reason: Option<String>,
    pub suggested_at: Option<DateTime<Utc>>,
    pub status: String,
}
#[derive(Debug, Serialize)]
pub struct PrivacyExport {
    pub discord_user_id: String,
    pub epoch: i64,
    pub activity_since: Option<String>,
    pub suggestions: Vec<ExportSuggestion>,
    pub candidates: Vec<ExportCandidate>,
}
pub async fn export(pool: &PgPool, discord_id: &str) -> Result<PrivacyExport, sqlx::Error> {
    let mut tx = pool.begin().await?;
    lock(&mut tx).await?;
    let state: Option<(i64, Option<DateTime<Utc>>)> = sqlx::query_as(
        "SELECT epoch,activity_since FROM twitch_scout_community_privacy WHERE identity_hash=$1",
    )
    .bind(identity_hash(discord_id))
    .fetch_optional(&mut *tx)
    .await?;
    let suggestions=sqlx::query_as("SELECT idempotency_key,twitch_user_id,twitch_login,reason,result_status,created_at,submitted_at FROM twitch_scout_community_suggestions WHERE suggested_by_discord_id=$1 ORDER BY id")
        .bind(discord_id).fetch_all(&mut *tx).await?;
    let candidates=sqlx::query_as("SELECT twitch_user_id,streamer_login,suggestion_reason,suggested_at,status FROM twitch_scout_candidates WHERE suggested_by_discord_id=$1 ORDER BY streamer_login")
        .bind(discord_id).fetch_all(&mut *tx).await?;
    tx.commit().await?;
    let (epoch, floor) = state.unwrap_or((0, None));
    Ok(PrivacyExport {
        discord_user_id: discord_id.into(),
        epoch,
        activity_since: floor.map(format_cursor),
        suggestions,
        candidates,
    })
}
