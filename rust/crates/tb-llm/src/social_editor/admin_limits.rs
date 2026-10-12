use serde::{Deserialize, Serialize};
use sqlx::{PgConnection, PgPool, Postgres, Row, Transaction};

use super::{usage, EditorError, ErrorKind, BUDGET_LOCK};

pub const INITIAL_TOTAL_CAP_MICRO_USD: i64 = 2_000_000;
pub(crate) const SAFE_INTEGER: i64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitError {
    Invalid,
    Conflict,
    UnknownUser,
    Unavailable,
}

impl From<sqlx::Error> for LimitError {
    fn from(_: sqlx::Error) -> Self {
        Self::Unavailable
    }
}

#[derive(Debug, Serialize)]
pub struct ProcessingControl {
    pub revision: i64,
    pub paused: bool,
    pub initial_total_cap_micro_usd: i64,
    pub updated_at: String,
    pub updated_by: String,
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct UserLimit {
    pub twitch_user_id: String,
    pub revision: i64,
    pub paused: bool,
    pub monthly_cap_micro_usd: Option<i64>,
    pub updated_at: String,
    pub updated_by: String,
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct PlanLimit {
    pub plan_id: String,
    pub name: String,
    pub revision: i64,
    pub monthly_cap_micro_usd: Option<i64>,
    pub monthly_ai_jobs: Option<i64>,
    pub monthly_revisions: Option<i64>,
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct ProcessingLimits {
    pub enforcement_available: bool,
    pub plan_configuration_available: bool,
    pub plan_policies_active: bool,
    pub global: ProcessingControl,
    pub users: Vec<UserLimit>,
    pub plans: Vec<PlanLimit>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GlobalControlChange {
    pub expected_revision: i64,
    pub paused: bool,
    pub reason: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserLimitChange {
    pub twitch_user_id: String,
    pub expected_revision: i64,
    pub paused: bool,
    pub monthly_cap_micro_usd: Option<i64>,
    pub reason: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanLimitChange {
    pub plan_id: String,
    pub expected_revision: i64,
    pub monthly_cap_micro_usd: Option<i64>,
    pub monthly_ai_jobs: Option<i64>,
    pub monthly_revisions: Option<i64>,
    pub reason: String,
}

fn validate_change(revision: i64, reason: &str, actor: &str) -> Result<(), LimitError> {
    if !(0..SAFE_INTEGER).contains(&revision)
        || reason.trim().is_empty()
        || reason.chars().count() > 1000
        || actor.is_empty()
        || actor.len() > 128
    {
        return Err(LimitError::Invalid);
    }
    Ok(())
}

fn validate_cap(cap: Option<i64>, maximum: i64) -> Result<(), LimitError> {
    if cap.is_some_and(|cap| !(0..=maximum).contains(&cap)) {
        return Err(LimitError::Invalid);
    }
    Ok(())
}

async fn locked_transaction(pool: &PgPool) -> Result<Transaction<'_, Postgres>, LimitError> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(BUDGET_LOCK)
        .execute(&mut *tx)
        .await?;
    Ok(tx)
}

pub async fn processing_limits(
    pool: &PgPool,
    catalog: &[(&str, &str)],
) -> Result<ProcessingLimits, LimitError> {
    let mut tx = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *tx)
        .await?;
    let row = sqlx::query("SELECT revision, paused, updated_at::text, updated_by, reason FROM public.social_ai_processing_control WHERE singleton")
        .fetch_one(&mut *tx).await?;
    let global = ProcessingControl {
        revision: row.try_get("revision")?,
        paused: row.try_get("paused")?,
        initial_total_cap_micro_usd: INITIAL_TOTAL_CAP_MICRO_USD,
        updated_at: row.try_get("updated_at")?,
        updated_by: row.try_get("updated_by")?,
        reason: row.try_get("reason")?,
    };
    let rows = sqlx::query("SELECT twitch_user_id, revision, paused, monthly_cap_micro_usd, updated_at::text, updated_by, reason FROM public.social_ai_user_limits ORDER BY twitch_user_id")
        .fetch_all(&mut *tx).await?;
    let users = rows
        .iter()
        .map(|row| {
            Ok(UserLimit {
                twitch_user_id: row.try_get("twitch_user_id")?,
                revision: row.try_get("revision")?,
                paused: row.try_get("paused")?,
                monthly_cap_micro_usd: row.try_get("monthly_cap_micro_usd")?,
                updated_at: row.try_get("updated_at")?,
                updated_by: row.try_get("updated_by")?,
                reason: row.try_get("reason")?,
            })
        })
        .collect::<Result<Vec<_>, LimitError>>()?;
    let drafts = sqlx::query("SELECT plan_id, revision, monthly_cap_micro_usd, monthly_ai_jobs, monthly_revisions, reason FROM public.social_ai_plan_limit_drafts")
        .fetch_all(&mut *tx).await?;
    let mut plans = Vec::with_capacity(catalog.len());
    for &(id, name) in catalog {
        let draft = drafts.iter().find(|row| {
            row.try_get::<String, _>("plan_id")
                .is_ok_and(|value| value == id)
        });
        plans.push(match draft {
            Some(row) => PlanLimit {
                plan_id: id.to_owned(),
                name: name.to_owned(),
                revision: row.try_get("revision")?,
                monthly_cap_micro_usd: row.try_get("monthly_cap_micro_usd")?,
                monthly_ai_jobs: row.try_get("monthly_ai_jobs")?,
                monthly_revisions: row.try_get("monthly_revisions")?,
                reason: row.try_get("reason")?,
            },
            None => PlanLimit {
                plan_id: id.to_owned(),
                name: name.to_owned(),
                revision: 0,
                monthly_cap_micro_usd: None,
                monthly_ai_jobs: None,
                monthly_revisions: None,
                reason: String::new(),
            },
        });
    }
    tx.commit().await?;
    Ok(ProcessingLimits {
        enforcement_available: true,
        plan_configuration_available: !catalog.is_empty(),
        plan_policies_active: false,
        global,
        users,
        plans,
    })
}

pub async fn save_global_control(
    pool: &PgPool,
    change: &GlobalControlChange,
    actor: &str,
) -> Result<(), LimitError> {
    validate_change(change.expected_revision, &change.reason, actor)?;
    let mut tx = locked_transaction(pool).await?;
    let result = sqlx::query("UPDATE public.social_ai_processing_control SET paused=$1, revision=revision+1, updated_at=now(), updated_by=$2, reason=$3 WHERE singleton AND revision=$4")
        .bind(change.paused).bind(actor).bind(change.reason.trim()).bind(change.expected_revision)
        .execute(&mut *tx).await?;
    if result.rows_affected() != 1 {
        return Err(LimitError::Conflict);
    }
    tx.commit().await?;
    Ok(())
}

pub async fn save_user_limit(
    pool: &PgPool,
    change: &UserLimitChange,
    actor: &str,
) -> Result<(), LimitError> {
    validate_change(change.expected_revision, &change.reason, actor)?;
    validate_cap(change.monthly_cap_micro_usd, INITIAL_TOTAL_CAP_MICRO_USD)?;
    if change.twitch_user_id.is_empty()
        || change.twitch_user_id.len() > 32
        || !change
            .twitch_user_id
            .bytes()
            .all(|byte| byte.is_ascii_digit())
        || !change.twitch_user_id.parse::<u64>().is_ok_and(|id| id > 0)
    {
        return Err(LimitError::Invalid);
    }
    let mut tx = locked_transaction(pool).await?;
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM public.twitch_streamers WHERE twitch_user_id=$1)",
    )
    .bind(&change.twitch_user_id)
    .fetch_one(&mut *tx)
    .await?;
    if !exists {
        return Err(LimitError::UnknownUser);
    }
    let current: Option<i64> = sqlx::query_scalar(
        "SELECT revision FROM public.social_ai_user_limits WHERE twitch_user_id=$1",
    )
    .bind(&change.twitch_user_id)
    .fetch_optional(&mut *tx)
    .await?;
    if current.unwrap_or(0) != change.expected_revision {
        return Err(LimitError::Conflict);
    }
    sqlx::query("INSERT INTO public.social_ai_user_limits (twitch_user_id, revision, paused, monthly_cap_micro_usd, updated_by, reason) VALUES ($1,$2,$3,$4,$5,$6) ON CONFLICT (twitch_user_id) DO UPDATE SET revision=EXCLUDED.revision, paused=EXCLUDED.paused, monthly_cap_micro_usd=EXCLUDED.monthly_cap_micro_usd, updated_by=EXCLUDED.updated_by, reason=EXCLUDED.reason, updated_at=now()")
        .bind(&change.twitch_user_id).bind(change.expected_revision + 1).bind(change.paused)
        .bind(change.monthly_cap_micro_usd).bind(actor).bind(change.reason.trim()).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

pub async fn save_plan_limit(
    pool: &PgPool,
    change: &PlanLimitChange,
    actor: &str,
    catalog: &[(&str, &str)],
) -> Result<(), LimitError> {
    validate_change(change.expected_revision, &change.reason, actor)?;
    validate_cap(change.monthly_cap_micro_usd, INITIAL_TOTAL_CAP_MICRO_USD)?;
    validate_cap(change.monthly_ai_jobs, SAFE_INTEGER)?;
    validate_cap(change.monthly_revisions, SAFE_INTEGER)?;
    if !catalog.iter().any(|&(id, _)| id == change.plan_id) {
        return Err(LimitError::Invalid);
    }
    let mut tx = locked_transaction(pool).await?;
    let current: Option<i64> = sqlx::query_scalar(
        "SELECT revision FROM public.social_ai_plan_limit_drafts WHERE plan_id=$1",
    )
    .bind(&change.plan_id)
    .fetch_optional(&mut *tx)
    .await?;
    if current.unwrap_or(0) != change.expected_revision {
        return Err(LimitError::Conflict);
    }
    sqlx::query("INSERT INTO public.social_ai_plan_limit_drafts (plan_id, revision, monthly_cap_micro_usd, monthly_ai_jobs, monthly_revisions, updated_by, reason) VALUES ($1,$2,$3,$4,$5,$6,$7) ON CONFLICT (plan_id) DO UPDATE SET revision=EXCLUDED.revision, monthly_cap_micro_usd=EXCLUDED.monthly_cap_micro_usd, monthly_ai_jobs=EXCLUDED.monthly_ai_jobs, monthly_revisions=EXCLUDED.monthly_revisions, updated_by=EXCLUDED.updated_by, reason=EXCLUDED.reason, updated_at=now()")
        .bind(&change.plan_id).bind(change.expected_revision + 1).bind(change.monthly_cap_micro_usd)
        .bind(change.monthly_ai_jobs).bind(change.monthly_revisions).bind(actor).bind(change.reason.trim())
        .execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

pub(crate) async fn admit_on(
    connection: &mut PgConnection,
    user: &str,
    reservation: i64,
) -> Result<(), EditorError> {
    let ledger = || EditorError::new(ErrorKind::Ledger);
    let paused: bool = sqlx::query_scalar(
        "SELECT paused FROM public.social_ai_processing_control WHERE singleton",
    )
    .fetch_one(&mut *connection)
    .await
    .map_err(|_| ledger())?;
    let policy: Option<(bool, Option<i64>)> = sqlx::query_as("SELECT paused, monthly_cap_micro_usd FROM public.social_ai_user_limits WHERE twitch_user_id=$1")
        .bind(user).fetch_optional(&mut *connection).await.map_err(|_| ledger())?;
    if paused || policy.is_some_and(|(paused, _)| paused) {
        return Err(EditorError::new(ErrorKind::Budget));
    }
    if let Some((_, Some(cap))) = policy {
        let used = usage::consumption_on(connection, Some(user)).await?;
        if used.checked_add(reservation).is_none_or(|sum| sum > cap) {
            return Err(EditorError::new(ErrorKind::Budget));
        }
    }
    Ok(())
}
