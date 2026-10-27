use serde::{Deserialize, Serialize};
use sqlx::{PgConnection, PgPool, Postgres, Row, Transaction};

use super::{
    usage::{self, DEBIT_SQL, INVALID_DEBIT_SQL},
    EditorContext, EditorError, ErrorKind, BUDGET_LOCK,
};

pub const INITIAL_TOTAL_CAP_MICRO_USD: i64 = 20_000_000;
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
    pub monthly_cap_micro_usd: Option<i64>,
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
    pub monthly_ai_jobs: Option<i64>,
    pub monthly_revisions: Option<i64>,
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
    pub monthly_cap_micro_usd: Option<i64>,
    pub reason: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserLimitChange {
    pub twitch_user_id: String,
    pub expected_revision: i64,
    pub paused: bool,
    pub monthly_cap_micro_usd: Option<i64>,
    pub monthly_ai_jobs: Option<i64>,
    pub monthly_revisions: Option<i64>,
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
    let row = sqlx::query("SELECT revision, paused, monthly_cap_micro_usd, updated_at::text, updated_by, reason FROM public.social_ai_processing_control WHERE singleton")
        .fetch_one(&mut *tx).await?;
    let global = ProcessingControl {
        revision: row.try_get("revision")?,
        paused: row.try_get("paused")?,
        initial_total_cap_micro_usd: INITIAL_TOTAL_CAP_MICRO_USD,
        monthly_cap_micro_usd: row.try_get("monthly_cap_micro_usd")?,
        updated_at: row.try_get("updated_at")?,
        updated_by: row.try_get("updated_by")?,
        reason: row.try_get("reason")?,
    };
    let rows = sqlx::query("SELECT twitch_user_id, revision, paused, monthly_cap_micro_usd, monthly_ai_jobs, monthly_revisions, updated_at::text, updated_by, reason FROM public.social_ai_user_limits ORDER BY twitch_user_id")
        .fetch_all(&mut *tx).await?;
    let users = rows
        .iter()
        .map(|row| {
            Ok(UserLimit {
                twitch_user_id: row.try_get("twitch_user_id")?,
                revision: row.try_get("revision")?,
                paused: row.try_get("paused")?,
                monthly_cap_micro_usd: row.try_get("monthly_cap_micro_usd")?,
                monthly_ai_jobs: row.try_get("monthly_ai_jobs")?,
                monthly_revisions: row.try_get("monthly_revisions")?,
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
    validate_cap(change.monthly_cap_micro_usd, INITIAL_TOTAL_CAP_MICRO_USD)?;
    let mut tx = locked_transaction(pool).await?;
    let result = sqlx::query("UPDATE public.social_ai_processing_control SET paused=$1, monthly_cap_micro_usd=$2, revision=revision+1, updated_at=now(), updated_by=$3, reason=$4 WHERE singleton AND revision=$5")
        .bind(change.paused).bind(change.monthly_cap_micro_usd).bind(actor).bind(change.reason.trim()).bind(change.expected_revision)
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
    validate_cap(change.monthly_ai_jobs, SAFE_INTEGER)?;
    validate_cap(change.monthly_revisions, SAFE_INTEGER)?;
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
    sqlx::query("INSERT INTO public.social_ai_user_limits (twitch_user_id, revision, paused, monthly_cap_micro_usd, monthly_ai_jobs, monthly_revisions, updated_by, reason) VALUES ($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT (twitch_user_id) DO UPDATE SET revision=EXCLUDED.revision, paused=EXCLUDED.paused, monthly_cap_micro_usd=EXCLUDED.monthly_cap_micro_usd, monthly_ai_jobs=EXCLUDED.monthly_ai_jobs, monthly_revisions=EXCLUDED.monthly_revisions, updated_by=EXCLUDED.updated_by, reason=EXCLUDED.reason, updated_at=now()")
        .bind(&change.twitch_user_id).bind(change.expected_revision + 1).bind(change.paused)
        .bind(change.monthly_cap_micro_usd).bind(change.monthly_ai_jobs).bind(change.monthly_revisions)
        .bind(actor).bind(change.reason.trim()).execute(&mut *tx).await?;
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

const CURRENT_MONTH_SQL: &str =
    "to_char(date_trunc('month', now() AT TIME ZONE 'UTC'), 'YYYY-MM-DD')";

struct UserPolicy {
    paused: bool,
    monthly_cap_micro_usd: Option<i64>,
    monthly_ai_jobs: Option<i64>,
    monthly_revisions: Option<i64>,
}

fn ledger() -> EditorError {
    EditorError::new(ErrorKind::Ledger)
}

fn budget() -> EditorError {
    EditorError::new(ErrorKind::Budget)
}

async fn user_policy(
    connection: &mut PgConnection,
    user: &str,
) -> Result<Option<UserPolicy>, EditorError> {
    let row = sqlx::query("SELECT paused, monthly_cap_micro_usd, monthly_ai_jobs, monthly_revisions FROM public.social_ai_user_limits WHERE twitch_user_id=$1")
        .bind(user)
        .fetch_optional(&mut *connection)
        .await
        .map_err(|_| ledger())?;
    row.map(|row| {
        Ok(UserPolicy {
            paused: row.try_get("paused")?,
            monthly_cap_micro_usd: row.try_get("monthly_cap_micro_usd")?,
            monthly_ai_jobs: row.try_get("monthly_ai_jobs")?,
            monthly_revisions: row.try_get("monthly_revisions")?,
        })
    })
    .transpose()
    .map_err(|_: sqlx::Error| ledger())
}

pub(crate) async fn monthly_consumption(connection: &mut PgConnection) -> Result<i64, EditorError> {
    let query = format!("SELECT COALESCE(SUM(({DEBIT_SQL})::numeric),0)::text AS used, COUNT(*) FILTER (WHERE {INVALID_DEBIT_SQL})::bigint AS invalid FROM public.llm_usage WHERE purpose='social_editor' AND (ts >= {CURRENT_MONTH_SQL} OR ts !~ '^[0-9]{{4}}-[0-9]{{2}}-[0-9]{{2}}T' OR attempt_state='started' OR estimated_micro_usd IS NULL OR cost_basis IS DISTINCT FROM 'usage_priced_estimate')");
    let row = sqlx::query(sqlx::AssertSqlSafe(query))
        .fetch_one(connection)
        .await
        .map_err(|_| ledger())?;
    let invalid: i64 = row.try_get("invalid").map_err(|_| ledger())?;
    let used: String = row.try_get("used").map_err(|_| ledger())?;
    if invalid != 0 {
        return Err(ledger());
    }
    used.parse::<i64>()
        .ok()
        .filter(|used| *used >= 0)
        .ok_or_else(ledger)
}

fn within(used: i64, reservation: i64, cap: i64) -> Result<(), EditorError> {
    if used.checked_add(reservation).is_none_or(|sum| sum > cap) {
        return Err(budget());
    }
    Ok(())
}

async fn already_admitted(
    connection: &mut PgConnection,
    context: &EditorContext,
) -> Result<bool, EditorError> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM public.llm_usage WHERE purpose='social_editor' AND social_ai_twitch_user_id=$1 AND social_ai_job_id=$2)")
        .bind(&context.twitch_user_id)
        .bind(&context.job_id)
        .fetch_one(connection)
        .await
        .map_err(|_| ledger())
}

async fn is_revision(connection: &mut PgConnection, job_id: &str) -> Result<bool, EditorError> {
    let Some(id) = job_id
        .strip_prefix("manual:")
        .and_then(|id| id.parse::<i64>().ok())
    else {
        return Ok(false);
    };
    let revision: Option<bool> =
        sqlx::query_scalar("SELECT root_job_id IS NOT NULL FROM public.social_ai_jobs WHERE id=$1")
            .bind(id)
            .fetch_optional(connection)
            .await
            .map_err(|_| ledger())?;
    Ok(revision.unwrap_or(false))
}

async fn monthly_jobs(
    connection: &mut PgConnection,
    user: &str,
    revisions: bool,
) -> Result<i64, EditorError> {
    let query = format!("SELECT COUNT(*) FROM (SELECT social_ai_job_id FROM public.llm_usage WHERE purpose='social_editor' AND social_ai_twitch_user_id=$1 AND social_ai_job_id IS NOT NULL GROUP BY social_ai_job_id HAVING MIN(ts) >= {CURRENT_MONTH_SQL}) started LEFT JOIN public.social_ai_jobs j ON started.social_ai_job_id = 'manual:' || j.id::text WHERE (j.root_job_id IS NOT NULL) = $2");
    sqlx::query_scalar(sqlx::AssertSqlSafe(query))
        .bind(user)
        .bind(revisions)
        .fetch_one(connection)
        .await
        .map_err(|_| ledger())
}

async fn admit_job(
    connection: &mut PgConnection,
    context: &EditorContext,
    policy: &UserPolicy,
) -> Result<(), EditorError> {
    if policy.monthly_ai_jobs.is_none() && policy.monthly_revisions.is_none() {
        return Ok(());
    }
    if already_admitted(connection, context).await? {
        return Ok(());
    }
    let revision = is_revision(connection, &context.job_id).await?;
    let limit = if revision {
        policy.monthly_revisions
    } else {
        policy.monthly_ai_jobs
    };
    let Some(limit) = limit else {
        return Ok(());
    };
    if monthly_jobs(connection, &context.twitch_user_id, revision).await? >= limit {
        return Err(budget());
    }
    Ok(())
}

pub(crate) async fn admit_on(
    connection: &mut PgConnection,
    context: &EditorContext,
    reservation: i64,
) -> Result<(), EditorError> {
    let (paused, global_cap): (bool, Option<i64>) = sqlx::query_as(
        "SELECT paused, monthly_cap_micro_usd FROM public.social_ai_processing_control WHERE singleton",
    )
    .fetch_one(&mut *connection)
    .await
    .map_err(|_| ledger())?;
    let policy = user_policy(connection, &context.twitch_user_id).await?;
    if paused || policy.as_ref().is_some_and(|policy| policy.paused) {
        return Err(budget());
    }
    if let Some(cap) = global_cap {
        within(monthly_consumption(connection).await?, reservation, cap)?;
    }
    let Some(policy) = policy else {
        return Ok(());
    };
    if let Some(cap) = policy.monthly_cap_micro_usd {
        let used = usage::consumption_on(connection, Some(&context.twitch_user_id)).await?;
        within(used, reservation, cap)?;
    }
    admit_job(connection, context, &policy).await
}
