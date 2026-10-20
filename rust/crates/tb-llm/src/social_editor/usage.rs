use serde_json::Value;

use sqlx::{PgConnection, Row};

use super::{EditorError, ErrorKind, Pricing};

pub(crate) const DEBIT_SQL: &str = "CASE WHEN attempt_state='succeeded' THEN COALESCE(NULLIF(estimated_micro_usd, 0), reserved_micro_usd) WHEN attempt_state='failed' AND cost_basis='provider_rejected_unbilled' THEN 0 ELSE GREATEST(reserved_micro_usd, COALESCE(estimated_micro_usd, reserved_micro_usd)) END";
pub(crate) const INVALID_DEBIT_SQL: &str = "reserved_micro_usd IS NULL OR reserved_micro_usd <= 0 OR attempt_state IS NULL OR attempt_state NOT IN ('started','succeeded','failed') OR (estimated_micro_usd IS NOT NULL AND estimated_micro_usd < 0) OR (cost_basis='provider_rejected_unbilled' AND (attempt_state<>'failed' OR estimated_micro_usd IS DISTINCT FROM 0 OR tokens_in IS NOT NULL OR tokens_out IS NOT NULL OR cache_creation_input_tokens IS NOT NULL OR cache_read_input_tokens IS NOT NULL OR http_status IS NULL OR http_status NOT BETWEEN 400 AND 499 OR http_status IN (408,429)))";

pub(crate) async fn consumption_on(
    connection: &mut PgConnection,
    user: Option<&str>,
) -> Result<i64, EditorError> {
    let query = format!("SELECT COALESCE(SUM(({DEBIT_SQL})::numeric),0)::text AS used, COUNT(*) FILTER (WHERE {INVALID_DEBIT_SQL})::bigint AS invalid FROM public.llm_usage WHERE purpose='social_editor' AND ($1::text IS NULL OR (social_ai_twitch_user_id=$1 AND (ts >= to_char(date_trunc('month', now() AT TIME ZONE 'UTC'), 'YYYY-MM-DD') OR ts !~ '^[0-9]{{4}}-[0-9]{{2}}-[0-9]{{2}}T' OR attempt_state='started' OR estimated_micro_usd IS NULL OR cost_basis IS DISTINCT FROM 'usage_priced_estimate'))) ");
    let row = sqlx::query(sqlx::AssertSqlSafe(query))
        .bind(user)
        .fetch_one(connection)
        .await
        .map_err(|_| EditorError::new(ErrorKind::Ledger))?;
    let invalid: i64 = row
        .try_get("invalid")
        .map_err(|_| EditorError::new(ErrorKind::Ledger))?;
    let used: String = row
        .try_get("used")
        .map_err(|_| EditorError::new(ErrorKind::Ledger))?;
    if invalid != 0 {
        return Err(EditorError::new(ErrorKind::Ledger));
    }
    used.parse::<i64>()
        .ok()
        .filter(|used| *used >= 0)
        .ok_or_else(|| EditorError::new(ErrorKind::Ledger))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Usage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_creation_input_tokens: Option<u64>,
    pub cache_read_input_tokens: Option<u64>,
}

impl Usage {
    pub(crate) fn from_value(value: &Value) -> Option<Self> {
        let object = value.as_object()?;
        let get = |key: &str| object.get(key).and_then(Value::as_u64);
        Some(Self {
            input_tokens: get("input_tokens"),
            output_tokens: get("output_tokens"),
            cache_creation_input_tokens: get("cache_creation_input_tokens"),
            cache_read_input_tokens: get("cache_read_input_tokens"),
        })
    }

    pub(crate) fn estimated_cost(&self, pricing: &Pricing) -> Option<i64> {
        pricing.measured(
            self.input_tokens?,
            self.output_tokens?,
            self.cache_creation_input_tokens?,
            self.cache_read_input_tokens?,
        )
    }
}
