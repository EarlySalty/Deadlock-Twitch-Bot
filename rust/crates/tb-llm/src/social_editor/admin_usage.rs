use serde::{Deserialize, Serialize};
use sqlx::{postgres::PgRow, PgPool, Row};

use super::{
    admin_limits::SAFE_INTEGER,
    usage::{DEBIT_SQL, INVALID_DEBIT_SQL},
    EditorError, ErrorKind,
};

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UsageGrouping {
    #[default]
    User,
    Provider,
    Job,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UsageFilter {
    pub group_by: UsageGrouping,
    pub offset: i64,
    pub limit: Option<i64>,
    pub twitch_user_id: Option<String>,
    pub provider: Option<String>,
    pub job_id: Option<String>,
}

impl UsageFilter {
    pub fn validate(&self) -> bool {
        (0..=100_000).contains(&self.offset)
            && (1..=100).contains(&self.limit.unwrap_or(25))
            && [
                self.twitch_user_id.as_deref(),
                self.provider.as_deref(),
                self.job_id.as_deref(),
            ]
            .into_iter()
            .flatten()
            .all(|value| {
                !value.is_empty() && value.len() <= 128 && !value.chars().any(char::is_control)
            })
    }
}

#[derive(Debug, Serialize)]
pub struct UsageTotals {
    pub attempts: i64,
    pub incomplete_attempts: i64,
    pub unknown_cost_attempts: i64,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub cache_creation_input_tokens: Option<i64>,
    pub cache_read_input_tokens: Option<i64>,
    pub estimated_micro_usd: Option<i64>,
    pub measured_micro_usd: Option<i64>,
    pub invoiced_micro_usd: Option<i64>,
    pub held_micro_usd: Option<i64>,
    pub charged_micro_usd: Option<i64>,
    pub provider_latency_ms: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct UsageGroup {
    #[serde(flatten)]
    pub totals: UsageTotals,
    pub twitch_user_id: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub job_id: Option<String>,
    pub processing_ms: Option<i64>,
    pub rendering_ms: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct UsageReport {
    pub totals: UsageTotals,
    pub groups: Vec<UsageGroup>,
    pub group_by: UsageGrouping,
    pub offset: i64,
    pub has_more: bool,
    pub period_start: Option<String>,
    pub period_end: Option<String>,
}

const FILTERED: &str = "WITH filtered AS (SELECT * FROM public.llm_usage WHERE purpose='social_editor' AND ($1::text IS NULL OR social_ai_twitch_user_id=$1) AND ($2::text IS NULL OR provider=$2) AND ($3::text IS NULL OR social_ai_job_id=$3))";
const UNFINISHED: &str = "attempt_state IS NULL OR attempt_state NOT IN ('succeeded','failed')";
const UNKNOWN_COST: &str = "estimated_micro_usd IS NULL OR estimated_micro_usd < 0 OR cost_basis IS DISTINCT FROM 'usage_priced_estimate'";

fn aggregate_columns() -> String {
    let mut columns = format!("COUNT(*)::bigint AS attempts, COUNT(*) FILTER (WHERE {UNFINISHED})::bigint AS incomplete_attempts, COUNT(*) FILTER (WHERE {UNKNOWN_COST})::bigint AS unknown_cost_attempts, COUNT(*) FILTER (WHERE {INVALID_DEBIT_SQL})::bigint AS invalid_debits, SUM(({DEBIT_SQL})::numeric)::text AS charged_micro_usd, COALESCE(SUM(({DEBIT_SQL})::numeric) FILTER (WHERE {UNFINISHED} OR {UNKNOWN_COST}),0)::text AS held_micro_usd, MIN(ts) AS period_start, MAX(ts) AS period_end");
    for (column, alias) in [
        ("tokens_in", "input_tokens"),
        ("tokens_out", "output_tokens"),
        ("cache_creation_input_tokens", "cache_creation_input_tokens"),
        ("cache_read_input_tokens", "cache_read_input_tokens"),
        ("latency_ms", "provider_latency_ms"),
        ("estimated_micro_usd", "estimated_micro_usd"),
    ] {
        columns.push_str(&format!(", COUNT(*) FILTER (WHERE {column} IS NULL OR {column} < 0)::bigint AS unknown_{alias}, SUM({column}::numeric)::text AS {alias}"));
    }
    columns
}

fn count(row: &PgRow, name: &str) -> Result<i64, EditorError> {
    row.try_get::<i64, _>(name)
        .ok()
        .filter(|value| (0..=SAFE_INTEGER).contains(value))
        .ok_or_else(|| EditorError::new(ErrorKind::Ledger))
}

fn safe_sum(row: &PgRow, name: &str, complete: bool) -> Result<Option<i64>, EditorError> {
    if !complete {
        return Ok(None);
    }
    let value: Option<String> = row
        .try_get(name)
        .map_err(|_| EditorError::new(ErrorKind::Ledger))?;
    Ok(value
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|value| (0..=SAFE_INTEGER).contains(value)))
}

fn totals(row: &PgRow) -> Result<UsageTotals, EditorError> {
    let attempts = count(row, "attempts")?;
    let incomplete = count(row, "incomplete_attempts")?;
    let unknown_cost = count(row, "unknown_cost_attempts")?;
    let complete = attempts > 0 && incomplete == 0;
    let value = |name: &str| {
        safe_sum(
            row,
            name,
            complete && count(row, &format!("unknown_{name}"))? == 0,
        )
    };
    let valid_debit = count(row, "invalid_debits")? == 0;
    Ok(UsageTotals {
        attempts,
        incomplete_attempts: incomplete,
        unknown_cost_attempts: unknown_cost,
        input_tokens: value("input_tokens")?,
        output_tokens: value("output_tokens")?,
        cache_creation_input_tokens: value("cache_creation_input_tokens")?,
        cache_read_input_tokens: value("cache_read_input_tokens")?,
        estimated_micro_usd: safe_sum(row, "estimated_micro_usd", complete && unknown_cost == 0)?,
        measured_micro_usd: None,
        invoiced_micro_usd: None,
        held_micro_usd: safe_sum(row, "held_micro_usd", valid_debit)?,
        charged_micro_usd: safe_sum(row, "charged_micro_usd", valid_debit && attempts > 0)?,
        provider_latency_ms: value("provider_latency_ms")?,
    })
}

fn identifier(row: &PgRow, name: &str) -> Result<Option<String>, EditorError> {
    let value: Option<String> = row
        .try_get(name)
        .map_err(|_| EditorError::new(ErrorKind::Ledger))?;
    Ok(value.filter(|value| {
        !value.is_empty()
            && value.len() <= 128
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte))
    }))
}

fn period(row: &PgRow, name: &str) -> Result<Option<String>, EditorError> {
    let value: Option<String> = row
        .try_get(name)
        .map_err(|_| EditorError::new(ErrorKind::Ledger))?;
    Ok(value
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(&value).ok())
        .map(|date| date.to_rfc3339()))
}

pub async fn usage_report(pool: &PgPool, filter: &UsageFilter) -> Result<UsageReport, EditorError> {
    if !filter.validate() {
        return Err(EditorError::new(ErrorKind::InvalidRequest));
    }
    let ledger = || EditorError::new(ErrorKind::Ledger);
    let mut tx = pool.begin().await.map_err(|_| ledger())?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *tx)
        .await
        .map_err(|_| ledger())?;
    let columns = aggregate_columns();
    let all = sqlx::query(sqlx::AssertSqlSafe(format!(
        "{FILTERED} SELECT {columns} FROM filtered"
    )))
    .bind(&filter.twitch_user_id)
    .bind(&filter.provider)
    .bind(&filter.job_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| ledger())?;
    let group_by = match filter.group_by {
        UsageGrouping::User => "social_ai_twitch_user_id",
        UsageGrouping::Provider => "provider, model",
        UsageGrouping::Job => "social_ai_twitch_user_id, social_ai_job_id",
    };
    let single = |column| {
        format!("CASE WHEN COUNT(DISTINCT {column})=1 AND COUNT({column})=COUNT(*) THEN MIN({column}) END AS {column}")
    };
    let identities = [
        "social_ai_twitch_user_id",
        "provider",
        "model",
        "social_ai_job_id",
    ]
    .map(single)
    .join(", ");
    let query = format!("{FILTERED}, grouped AS (SELECT {columns}, {identities}, MIN(id) AS first_id FROM filtered GROUP BY {group_by}) SELECT grouped.*, j.processing_ms, j.rendering_ms FROM grouped LEFT JOIN public.social_ai_jobs j ON $4::text='job' AND grouped.social_ai_job_id='manual:' || j.id::text AND grouped.social_ai_twitch_user_id=j.twitch_user_id::text AND j.completed_at IS NOT NULL ORDER BY first_id LIMIT $5 OFFSET $6");
    let grouping = match filter.group_by {
        UsageGrouping::User => "user",
        UsageGrouping::Provider => "provider",
        UsageGrouping::Job => "job",
    };
    let limit = filter.limit.unwrap_or(25);
    let mut rows = sqlx::query(sqlx::AssertSqlSafe(query))
        .bind(&filter.twitch_user_id)
        .bind(&filter.provider)
        .bind(&filter.job_id)
        .bind(grouping)
        .bind(limit + 1)
        .bind(filter.offset)
        .fetch_all(&mut *tx)
        .await
        .map_err(|_| ledger())?;
    let has_more = rows.len() > limit as usize;
    rows.truncate(limit as usize);
    let mut groups = Vec::with_capacity(rows.len());
    for row in rows {
        let ms = |name| -> Result<Option<i64>, EditorError> {
            let value: Option<i64> = row.try_get(name).map_err(|_| ledger())?;
            Ok(value.filter(|value| (0..=SAFE_INTEGER).contains(value)))
        };
        groups.push(UsageGroup {
            totals: totals(&row)?,
            twitch_user_id: identifier(&row, "social_ai_twitch_user_id")?,
            provider: identifier(&row, "provider")?,
            model: identifier(&row, "model")?,
            job_id: identifier(&row, "social_ai_job_id")?,
            processing_ms: ms("processing_ms")?,
            rendering_ms: ms("rendering_ms")?,
        });
    }
    tx.commit().await.map_err(|_| ledger())?;
    Ok(UsageReport {
        totals: totals(&all)?,
        groups,
        group_by: filter.group_by,
        offset: filter.offset,
        has_more,
        period_start: period(&all, "period_start")?,
        period_end: period(&all, "period_end")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_filter_rejects_unbounded_or_control_input() {
        assert!(UsageFilter::default().validate());
        assert!(!UsageFilter {
            offset: -1,
            ..UsageFilter::default()
        }
        .validate());
        assert!(!UsageFilter {
            limit: Some(101),
            ..UsageFilter::default()
        }
        .validate());
        assert!(!UsageFilter {
            job_id: Some("private\ninput".into()),
            ..UsageFilter::default()
        }
        .validate());
        assert!(UsageFilter {
            provider: Some("' OR TRUE --".into()),
            ..UsageFilter::default()
        }
        .validate());
    }
}
