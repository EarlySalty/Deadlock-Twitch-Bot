use serde::Serialize;
use sqlx::{PgPool, Row};

use super::{check_consent, EditorError, ErrorKind};

const SAFE_INTEGER: i64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, Default, Serialize)]
pub struct JobUsage {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub estimated_provider_cost_usd: Option<f64>,
    pub provider_latency_ms: Option<i64>,
}

pub async fn consent_allowed(pool: &PgPool, twitch_user_id: &str) -> Result<(), EditorError> {
    check_consent(pool, twitch_user_id).await
}

pub async fn global_consent(pool: &PgPool) -> Result<bool, EditorError> {
    let value: Option<String> = sqlx::query_scalar(
        "SELECT value::text FROM public.social_media_settings WHERE key = 'external_llm_consent'",
    )
    .fetch_optional(pool)
    .await
    .map_err(|_| EditorError::new(ErrorKind::Consent))?;
    Ok(value
        .as_deref()
        .and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok())
        == Some(serde_json::Value::Bool(true)))
}

fn bounded_sum(
    row: &sqlx::postgres::PgRow,
    name: &str,
    complete: bool,
) -> Result<Option<i64>, EditorError> {
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

pub async fn job_usage(
    pool: &PgPool,
    twitch_user_id: &str,
    job_id: &str,
) -> Result<JobUsage, EditorError> {
    let row = sqlx::query("SELECT COUNT(*)::bigint AS attempts,
        COUNT(*) FILTER (WHERE attempt_state NOT IN ('succeeded','failed') OR attempt_state IS NULL)::bigint AS unfinished,
        COUNT(*) FILTER (WHERE tokens_in IS NULL OR tokens_in < 0 OR tokens_out IS NULL OR tokens_out < 0)::bigint AS unknown_tokens,
        COUNT(*) FILTER (WHERE estimated_micro_usd IS NULL OR estimated_micro_usd < 0 OR cost_basis IS NULL OR cost_basis NOT IN ('usage_priced_estimate','provider_rejected_unbilled','subscription_included'))::bigint AS unknown_cost,
        COUNT(*) FILTER (WHERE latency_ms IS NULL OR latency_ms < 0)::bigint AS unknown_latency,
        CASE WHEN COUNT(DISTINCT provider) = 1 AND COUNT(provider) = COUNT(*) THEN MIN(provider) END AS provider,
        CASE WHEN COUNT(DISTINCT model) = 1 AND COUNT(model) = COUNT(*) THEN MIN(model) END AS model,
        SUM(tokens_in::numeric)::text AS input_tokens,
        SUM(tokens_out::numeric)::text AS output_tokens,
        SUM(estimated_micro_usd::numeric)::text AS estimated,
        SUM(latency_ms::numeric)::text AS latency
        FROM public.llm_usage WHERE purpose = 'social_editor' AND social_ai_twitch_user_id = $1 AND social_ai_job_id = $2")
        .bind(twitch_user_id).bind(job_id).fetch_one(pool).await.map_err(|_| EditorError::new(ErrorKind::Ledger))?;
    let count: i64 = row
        .try_get("attempts")
        .map_err(|_| EditorError::new(ErrorKind::Ledger))?;
    let unfinished: i64 = row
        .try_get("unfinished")
        .map_err(|_| EditorError::new(ErrorKind::Ledger))?;
    let complete = count > 0 && unfinished == 0;
    let unknown = |column| {
        row.try_get::<i64, _>(column)
            .map_err(|_| EditorError::new(ErrorKind::Ledger))
    };
    Ok(JobUsage {
        provider: if count > 0 {
            row.try_get("provider")
                .map_err(|_| EditorError::new(ErrorKind::Ledger))?
        } else {
            None
        },
        model: if count > 0 {
            row.try_get("model")
                .map_err(|_| EditorError::new(ErrorKind::Ledger))?
        } else {
            None
        },
        input_tokens: bounded_sum(
            &row,
            "input_tokens",
            complete && unknown("unknown_tokens")? == 0,
        )?,
        output_tokens: bounded_sum(
            &row,
            "output_tokens",
            complete && unknown("unknown_tokens")? == 0,
        )?,
        estimated_provider_cost_usd: bounded_sum(
            &row,
            "estimated",
            complete && unknown("unknown_cost")? == 0,
        )?
        .map(|cost| cost as f64 / 1_000_000.0),
        provider_latency_ms: bounded_sum(
            &row,
            "latency",
            complete && unknown("unknown_latency")? == 0,
        )?,
    })
}
