mod config;
mod schema;
mod usage;

pub use config::{EditorConfig, Effort, Pricing, ProviderConfig, ProviderKind};
pub use usage::Usage;

use base64::{engine::general_purpose::STANDARD, Engine};
use chrono::{SecondsFormat, Utc};
use reqwest::{redirect::Policy, Client, StatusCode};
use serde_json::{json, Value};
use sqlx::{PgConnection, PgPool, Postgres, Row, Transaction};
use std::time::{Duration, Instant};

const PROVIDER: &str = "anthropic";
const PURPOSE: &str = "social_editor";
const BUDGET_LOCK: i64 = 745_001_322_990;
const MAX_RESPONSE_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Configuration,
    InvalidRequest,
    Consent,
    Budget,
    Ledger,
    Unavailable,
    Transport,
    Provider,
    Refusal,
    Truncated,
    InvalidResponse,
}

#[derive(Debug)]
pub struct EditorError {
    pub kind: ErrorKind,
    pub ledger_id: Option<i64>,
    pub request_id: Option<Box<str>>,
    pub http_status: Option<u16>,
    pub provider_error_code: Option<Box<str>>,
    pub usage: Option<Box<Usage>>,
    pub latency_ms: Option<u32>,
    pub estimated_micro_usd: Option<i64>,
    pub reserved_micro_usd: Option<i64>,
    pub cost_basis: Option<&'static str>,
}

impl EditorError {
    fn new(kind: ErrorKind) -> Self {
        Self {
            kind,
            ledger_id: None,
            request_id: None,
            http_status: None,
            provider_error_code: None,
            usage: None,
            latency_ms: None,
            estimated_micro_usd: None,
            reserved_micro_usd: None,
            cost_basis: None,
        }
    }
}

impl std::fmt::Display for EditorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self.kind {
            ErrorKind::Configuration => "KI-Konfiguration ist ungültig",
            ErrorKind::InvalidRequest => "KI-Anfrage überschreitet die freigegebenen Grenzen",
            ErrorKind::Consent => "Die erforderliche KI-Einwilligung fehlt",
            ErrorKind::Budget => "Das freigegebene KI-Budget ist ausgeschöpft",
            ErrorKind::Ledger => "Die KI-Verbrauchserfassung ist nicht verfügbar",
            ErrorKind::Unavailable => "Der KI-Anbieter ist nicht verfügbar",
            ErrorKind::Transport => "Die KI-Anfrage konnte nicht zugestellt werden",
            ErrorKind::Provider => "Der KI-Anbieter hat die Anfrage abgelehnt",
            ErrorKind::Refusal => "Die KI hat die Anfrage abgelehnt",
            ErrorKind::Truncated => "Die KI-Antwort wurde abgeschnitten",
            ErrorKind::InvalidResponse => "Die KI-Antwort ist ungültig",
        })
    }
}

impl std::error::Error for EditorError {}

#[derive(Debug, Clone)]
pub struct EditorContext {
    pub twitch_user_id: String,
    pub job_id: String,
    pub stage: String,
}

#[derive(Debug, Clone, Copy)]
pub enum ImageMediaType {
    Jpeg,
    Png,
}

impl ImageMediaType {
    fn as_str(self) -> &'static str {
        match self {
            Self::Jpeg => "image/jpeg",
            Self::Png => "image/png",
        }
    }
}

#[derive(Debug)]
pub struct Frame {
    pub timestamp_ms: u64,
    pub media_type: ImageMediaType,
    pub bytes: Vec<u8>,
}

#[derive(Debug)]
pub struct EditorRequest {
    pub system: String,
    pub text: String,
    pub frames: Vec<Frame>,
    pub json_schema: Value,
}

#[derive(Debug)]
pub struct EditorResponse {
    pub provider: String,
    pub model: String,
    pub content: Value,
    pub usage: Option<Usage>,
    pub latency_ms: u64,
    pub request_id: Option<String>,
    pub ledger_id: i64,
    pub estimated_micro_usd: Option<i64>,
    pub reserved_micro_usd: i64,
    pub charge_micro_usd: i64,
    pub cost_basis: &'static str,
}

fn build_body(config: &ProviderConfig, request: &EditorRequest) -> Result<Value, EditorError> {
    let mut blocks = Vec::with_capacity(request.frames.len() * 2 + 1);
    for frame in &request.frames {
        blocks.push(json!({"type":"text","text":format!("Frame bei {} ms", frame.timestamp_ms)}));
        blocks.push(json!({"type":"image","source":{
            "type":"base64","media_type":frame.media_type.as_str(),"data":STANDARD.encode(&frame.bytes)
        }}));
    }
    blocks.push(json!({"type":"text","text":request.text}));
    let effort = serde_json::to_value(config.effort)
        .map_err(|_| EditorError::new(ErrorKind::Configuration))?;
    Ok(json!({
        "model":config.model,
        "max_tokens":config.max_output_tokens,
        "thinking":{"type":"adaptive"},
        "output_config":{"effort":effort,"format":{"type":"json_schema","schema":request.json_schema}},
        "system":request.system,
        "messages":[{"role":"user","content":blocks}]
    }))
}

fn image_signature_matches(frame: &Frame) -> bool {
    match frame.media_type {
        ImageMediaType::Png => {
            let bytes = &frame.bytes;
            bytes.len() >= 24
                && bytes.starts_with(b"\x89PNG\r\n\x1a\n")
                && &bytes[12..16] == b"IHDR"
                && (1..=8_000).contains(&u32::from_be_bytes([
                    bytes[16], bytes[17], bytes[18], bytes[19],
                ]))
                && (1..=8_000).contains(&u32::from_be_bytes([
                    bytes[20], bytes[21], bytes[22], bytes[23],
                ]))
        }
        ImageMediaType::Jpeg => {
            frame.bytes.starts_with(&[0xff, 0xd8]) && frame.bytes.ends_with(&[0xff, 0xd9])
        }
    }
}

fn validate_request(
    config: &ProviderConfig,
    context: &EditorContext,
    request: &EditorRequest,
) -> Result<jsonschema::Validator, EditorError> {
    let bad = || EditorError::new(ErrorKind::InvalidRequest);
    if context.twitch_user_id.trim().is_empty()
        || context.twitch_user_id.len() > 128
        || context.job_id.trim().is_empty()
        || context.job_id.len() > 128
        || context.stage.trim().is_empty()
        || context.stage.len() > 64
        || request.system.is_empty()
        || request.text.is_empty()
        || request.system.len().saturating_add(request.text.len()) > config.max_text_bytes
        || request.frames.len() > config.max_frames
        || request.frames.iter().any(|frame| {
            frame.bytes.is_empty()
                || frame.bytes.len() > config.max_frame_bytes
                || !image_signature_matches(frame)
        })
    {
        return Err(bad());
    }
    let schema = serde_json::to_vec(&request.json_schema).map_err(|_| bad())?;
    if schema.len() > config.max_schema_bytes {
        return Err(bad());
    }
    let validator = schema::compile(&request.json_schema)?;
    let conservative_input_tokens = 8_192_usize
        .saturating_add(request.system.len())
        .saturating_add(request.text.len())
        .saturating_add(schema.len())
        .saturating_add(request.frames.len().saturating_mul(4_784));
    if conservative_input_tokens > config.max_input_tokens as usize {
        return Err(bad());
    }
    Ok(validator)
}

async fn check_consent_on(
    connection: &mut PgConnection,
    twitch_user_id: &str,
) -> Result<(), EditorError> {
    let row = sqlx::query("SELECT (SELECT external_ai_consent FROM public.social_ai_user_consent WHERE twitch_user_id=$1) AS person, (SELECT value::text FROM public.social_media_settings WHERE key='external_llm_consent') AS global")
        .bind(twitch_user_id)
        .fetch_one(connection).await.map_err(|_| EditorError::new(ErrorKind::Consent))?;
    let person: Option<bool> = row
        .try_get("person")
        .map_err(|_| EditorError::new(ErrorKind::Consent))?;
    let global: Option<String> = row
        .try_get("global")
        .map_err(|_| EditorError::new(ErrorKind::Consent))?;
    if person != Some(true)
        || global
            .as_deref()
            .and_then(|v| serde_json::from_str::<Value>(v).ok())
            != Some(Value::Bool(true))
    {
        return Err(EditorError::new(ErrorKind::Consent));
    }
    Ok(())
}

async fn check_consent(pool: &PgPool, twitch_user_id: &str) -> Result<(), EditorError> {
    let mut connection = pool
        .acquire()
        .await
        .map_err(|_| EditorError::new(ErrorKind::Consent))?;
    check_consent_on(&mut connection, twitch_user_id).await
}

async fn begin_attempt(
    pool: &PgPool,
    config: &ProviderConfig,
    context: &EditorContext,
) -> Result<i64, EditorError> {
    let (project, service) =
        crate::ledger::identity().ok_or_else(|| EditorError::new(ErrorKind::Ledger))?;
    let reservation = config
        .pricing
        .reservation(
            u64::from(config.max_input_tokens),
            u64::from(config.max_output_tokens),
        )
        .ok_or_else(|| EditorError::new(ErrorKind::Configuration))?;
    let mut tx: Transaction<'_, Postgres> = pool
        .begin()
        .await
        .map_err(|_| EditorError::new(ErrorKind::Ledger))?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(BUDGET_LOCK)
        .execute(&mut *tx)
        .await
        .map_err(|_| EditorError::new(ErrorKind::Ledger))?;
    check_consent_on(&mut tx, &context.twitch_user_id).await?;
    let (used, total, invalid): (Option<i64>, i64, i64) = sqlx::query_as("SELECT SUM(CASE WHEN attempt_state='succeeded' THEN COALESCE(NULLIF(estimated_micro_usd, 0), reserved_micro_usd) ELSE GREATEST(reserved_micro_usd, COALESCE(estimated_micro_usd, reserved_micro_usd)) END)::bigint, COUNT(*)::bigint, COUNT(*) FILTER (WHERE reserved_micro_usd IS NULL OR reserved_micro_usd <= 0 OR attempt_state NOT IN ('started','succeeded','failed') OR (estimated_micro_usd IS NOT NULL AND estimated_micro_usd < 0))::bigint FROM public.llm_usage WHERE purpose=$1 AND provider=$2")
        .bind(PURPOSE).bind(PROVIDER).fetch_one(&mut *tx).await.map_err(|_| EditorError::new(ErrorKind::Ledger))?;
    if invalid != 0 || (total != 0 && used.is_none()) {
        return Err(EditorError::new(ErrorKind::Ledger));
    }
    if used
        .unwrap_or(0)
        .checked_add(reservation)
        .is_none_or(|sum| sum > config.initial_total_cap_micro_usd)
    {
        return Err(EditorError::new(ErrorKind::Budget));
    }
    let id: i64 = sqlx::query_scalar("INSERT INTO public.llm_usage (ts,source,purpose,model,tokens_in,tokens_out,total,success,project,service,provider,attempt_state,social_ai_twitch_user_id,social_ai_job_id,social_ai_stage,reserved_micro_usd,cost_basis) VALUES ($1,$2,$3,$4,NULL,NULL,NULL,0,$5,$6,$7,'started',$8,$9,$10,$11,'reserved_upper_bound') RETURNING id")
        .bind(Utc::now().to_rfc3339_opts(SecondsFormat::Secs, false))
        .bind(crate::ledger::SOURCE).bind(PURPOSE).bind(&config.model).bind(project).bind(service).bind(PROVIDER)
        .bind(&context.twitch_user_id).bind(&context.job_id).bind(&context.stage).bind(reservation)
        .fetch_one(&mut *tx).await.map_err(|_| EditorError::new(ErrorKind::Ledger))?;
    tx.commit()
        .await
        .map_err(|_| EditorError::new(ErrorKind::Ledger))?;
    Ok(id)
}

fn parse_reply(
    body: &[u8],
    validator: &jsonschema::Validator,
) -> Result<(String, Value), ErrorKind> {
    let response: Value = serde_json::from_slice(body).map_err(|_| ErrorKind::InvalidResponse)?;
    match response.get("stop_reason").and_then(Value::as_str) {
        Some("end_turn") => (),
        Some("refusal") => return Err(ErrorKind::Refusal),
        Some("max_tokens") => return Err(ErrorKind::Truncated),
        _ => return Err(ErrorKind::InvalidResponse),
    }
    let model = response
        .get("model")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or(ErrorKind::InvalidResponse)?
        .to_owned();
    let blocks = response
        .get("content")
        .and_then(Value::as_array)
        .ok_or(ErrorKind::InvalidResponse)?;
    let mut texts =
        blocks
            .iter()
            .filter_map(|block| match block.get("type").and_then(Value::as_str) {
                Some("text") => Some(block.get("text").and_then(Value::as_str)),
                Some("thinking" | "redacted_thinking") => None,
                _ => Some(None),
            });
    let text = texts.next().flatten().ok_or(ErrorKind::InvalidResponse)?;
    if texts.next().is_some() {
        return Err(ErrorKind::InvalidResponse);
    }
    let content: Value = serde_json::from_str(text).map_err(|_| ErrorKind::InvalidResponse)?;
    if !validator.is_valid(&content) {
        return Err(ErrorKind::InvalidResponse);
    }
    Ok((model, content))
}

struct AttemptOutcome<'a> {
    status: Option<StatusCode>,
    request_id: Option<&'a str>,
    usage: Option<&'a Usage>,
    estimated: Option<i64>,
    latency_ms: u64,
    kind: Option<ErrorKind>,
    provider_code: Option<&'a str>,
}

async fn finalize(
    pool: &PgPool,
    id: i64,
    reserved: i64,
    outcome: AttemptOutcome<'_>,
) -> Result<(), EditorError> {
    let AttemptOutcome {
        status,
        request_id,
        usage,
        estimated,
        latency_ms,
        kind,
        provider_code,
    } = outcome;
    let safe_estimated = estimated.filter(|_| usage.is_some());
    let tokens_in = usage
        .and_then(|u| u.input_tokens)
        .and_then(|n| i64::try_from(n).ok());
    let tokens_out = usage
        .and_then(|u| u.output_tokens)
        .and_then(|n| i64::try_from(n).ok());
    let total = tokens_in.and_then(|a| tokens_out.and_then(|b| a.checked_add(b)));
    let code = kind.map(|k| match provider_code {
        Some(provider_code) if k == ErrorKind::Provider => format!("{k:?}/{provider_code}"),
        _ => format!("{k:?}"),
    });
    let affected = sqlx::query("UPDATE public.llm_usage SET tokens_in=$2,tokens_out=$3,total=$4,success=$5,attempt_state=$6,request_id=$7,error_code=$8,http_status=$9,latency_ms=$10,finished_at=now(),estimated_micro_usd=$11,cost_basis=$12,cache_creation_input_tokens=$13,cache_read_input_tokens=$14 WHERE id=$1 AND purpose=$15 AND provider=$16 AND attempt_state='started'")
        .bind(id).bind(tokens_in).bind(tokens_out).bind(total).bind(i64::from(kind.is_none()))
        .bind(if kind.is_none() { "succeeded" } else { "failed" }).bind(request_id).bind(code)
        .bind(status.map(|s| i32::from(s.as_u16()))).bind(i64::try_from(latency_ms).unwrap_or(i64::MAX))
        .bind(safe_estimated).bind(if (kind.is_none() || safe_estimated.is_some_and(|cost| cost > reserved)) && safe_estimated.is_some() { "usage_priced_estimate" } else { "reserved_upper_bound" })
        .bind(usage.and_then(|u| u.cache_creation_input_tokens).and_then(|n| i64::try_from(n).ok()))
        .bind(usage.and_then(|u| u.cache_read_input_tokens).and_then(|n| i64::try_from(n).ok()))
        .bind(PURPOSE).bind(PROVIDER).execute(pool).await.map_err(|_| EditorError::new(ErrorKind::Ledger))?;
    if affected.rows_affected() != 1 {
        return Err(EditorError::new(ErrorKind::Ledger));
    }
    Ok(())
}

fn response_usage(body: &[u8]) -> Option<Usage> {
    let value: Value = serde_json::from_slice(body).ok()?;
    Usage::from_value(value.get("usage")?)
}

fn response_request_id(
    body: &[u8],
    header: Option<&reqwest::header::HeaderValue>,
) -> Option<String> {
    header
        .and_then(|h| h.to_str().ok())
        .map(str::to_owned)
        .or_else(|| {
            let value: Value = serde_json::from_slice(body).ok()?;
            value.get("id")?.as_str().map(str::to_owned)
        })
}

fn provider_error_code(body: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(body).ok()?;
    let error = value.get("error")?;
    let kind = error.get("type")?.as_str()?;
    if !matches!(
        kind,
        "invalid_request_error"
            | "authentication_error"
            | "permission_error"
            | "not_found_error"
            | "request_too_large"
            | "rate_limit_error"
            | "api_error"
            | "overloaded_error"
    ) {
        return None;
    }
    let Some(code) = error.get("code").and_then(Value::as_str) else {
        return Some(kind.to_owned());
    };
    if code.is_empty()
        || code.len() > 48
        || !code
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase())
        || !code
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Some(kind.to_owned());
    }
    Some(format!("{kind}/{code}"))
}

pub async fn complete(
    pool: &PgPool,
    context: &EditorContext,
    configuration: &EditorConfig,
    request: &EditorRequest,
) -> Result<EditorResponse, EditorError> {
    let key =
        crate::keys::anthropic_api_key().ok_or_else(|| EditorError::new(ErrorKind::Unavailable))?;
    complete_with_key(pool, context, configuration, request, &key).await
}

async fn complete_with_key(
    pool: &PgPool,
    context: &EditorContext,
    configuration: &EditorConfig,
    request: &EditorRequest,
    key: &str,
) -> Result<EditorResponse, EditorError> {
    configuration.validate()?;
    let config = &configuration.provider;
    let validator = validate_request(config, context, request)?;
    let body = build_body(config, request)?;
    let reservation = config
        .pricing
        .reservation(
            u64::from(config.max_input_tokens),
            u64::from(config.max_output_tokens),
        )
        .ok_or_else(|| EditorError::new(ErrorKind::Configuration))?;
    let client = Client::builder()
        .no_proxy()
        .redirect(Policy::none())
        .timeout(Duration::from_millis(config.request_timeout_ms))
        .build()
        .map_err(|_| EditorError::new(ErrorKind::Configuration))?;
    let endpoint = format!("{}/v1/messages", config.endpoint.trim_end_matches('/'));
    let deadline = Instant::now() + Duration::from_millis(config.total_deadline_ms);
    for attempt in 0..=config.max_retries {
        if attempt > 0 {
            let delay =
                Duration::from_millis(config.retry_backoff_ms.saturating_mul(u64::from(attempt)));
            if Instant::now() + delay >= deadline {
                return Err(EditorError::new(ErrorKind::Transport));
            }
            tokio::time::sleep(delay).await;
        }
        let id = begin_attempt(pool, config, context).await?;
        if let Err(mut consent_error) = check_consent(pool, &context.twitch_user_id).await {
            if finalize(
                pool,
                id,
                reservation,
                AttemptOutcome {
                    status: None,
                    request_id: None,
                    usage: None,
                    estimated: None,
                    latency_ms: 0,
                    kind: Some(ErrorKind::Consent),
                    provider_code: None,
                },
            )
            .await
            .is_err()
            {
                crate::ledger::journal_completion(&crate::ledger::Completion {
                    id,
                    tokens_in: None,
                    tokens_out: None,
                    total: None,
                    request_id: None,
                    success: false,
                    error_code: Some("Consent".to_owned()),
                    http_status: None,
                    latency_ms: 0,
                });
            }
            consent_error.ledger_id = Some(id);
            return Err(consent_error);
        }
        let start = Instant::now();
        let remaining = deadline.saturating_duration_since(start);
        let sent = client
            .post(&endpoint)
            .header("x-api-key", key)
            .header("anthropic-version", "2023-06-01")
            .timeout(remaining.min(Duration::from_millis(config.request_timeout_ms)))
            .json(&body)
            .send()
            .await;
        let (status, request_id, bytes, kind, retry) = match sent {
            Ok(mut reply) => {
                let status = reply.status();
                let header = reply.headers().get("request-id").cloned();
                let mut bytes = Vec::new();
                loop {
                    match reply.chunk().await {
                        Ok(Some(chunk))
                            if bytes.len().saturating_add(chunk.len()) <= MAX_RESPONSE_BYTES =>
                        {
                            bytes.extend_from_slice(&chunk)
                        }
                        Ok(None) => break,
                        _ => {
                            bytes.clear();
                            break;
                        }
                    }
                }
                let request_id = response_request_id(&bytes, header.as_ref());
                let retry = status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error();
                let kind = if status.is_success() {
                    None
                } else {
                    Some(ErrorKind::Provider)
                };
                (Some(status), request_id, bytes, kind, retry)
            }
            Err(_) => (None, None, Vec::new(), Some(ErrorKind::Transport), true),
        };
        let provider_code = if kind == Some(ErrorKind::Provider) {
            provider_error_code(&bytes)
        } else {
            None
        };
        let usage = response_usage(&bytes);
        let measured = usage
            .as_ref()
            .and_then(|u| u.estimated_cost(&config.pricing));
        let estimated = measured.filter(|v| *v > 0);
        let exceeds_reservation = estimated.is_some_and(|cost| cost > reservation);
        let parsed = if exceeds_reservation {
            Err(ErrorKind::InvalidResponse)
        } else if kind.is_none() {
            parse_reply(&bytes, &validator).and_then(|(model, content)| {
                if model == config.model {
                    Ok((model, content))
                } else {
                    Err(ErrorKind::InvalidResponse)
                }
            })
        } else {
            Err(kind.unwrap_or(ErrorKind::Provider))
        };
        let error_kind = parsed.as_ref().err().copied();
        let latency_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
        if let Err(mut err) = finalize(
            pool,
            id,
            reservation,
            AttemptOutcome {
                status,
                request_id: request_id.as_deref(),
                usage: usage.as_ref(),
                estimated,
                latency_ms,
                kind: error_kind,
                provider_code: provider_code.as_deref(),
            },
        )
        .await
        {
            let tokens_in = usage
                .as_ref()
                .and_then(|u| u.input_tokens)
                .and_then(|n| i64::try_from(n).ok());
            let tokens_out = usage
                .as_ref()
                .and_then(|u| u.output_tokens)
                .and_then(|n| i64::try_from(n).ok());
            crate::ledger::journal_completion(&crate::ledger::Completion {
                id,
                tokens_in,
                tokens_out,
                total: tokens_in.and_then(|a| tokens_out.and_then(|b| a.checked_add(b))),
                request_id: request_id.clone(),
                success: error_kind.is_none(),
                error_code: error_kind.map(|k| match provider_code.as_deref() {
                    Some(code) if k == ErrorKind::Provider => format!("{k:?}/{code}"),
                    _ => format!("{k:?}"),
                }),
                http_status: status.map(|s| i32::from(s.as_u16())),
                latency_ms: i64::try_from(latency_ms).unwrap_or(i64::MAX),
            });
            err.ledger_id = Some(id);
            err.request_id = request_id.map(String::into_boxed_str);
            err.http_status = status.map(|s| s.as_u16());
            err.provider_error_code = provider_code.map(String::into_boxed_str);
            err.usage = usage.map(Box::new);
            err.latency_ms = Some(u32::try_from(latency_ms).unwrap_or(u32::MAX));
            err.estimated_micro_usd = estimated;
            err.reserved_micro_usd = Some(reservation);
            err.cost_basis = Some(if exceeds_reservation {
                "usage_priced_estimate"
            } else {
                "reserved_upper_bound"
            });
            return Err(err);
        }
        match parsed {
            Ok((model, content)) => {
                return Ok(EditorResponse {
                    provider: PROVIDER.to_owned(),
                    model,
                    content,
                    usage,
                    latency_ms,
                    request_id,
                    ledger_id: id,
                    estimated_micro_usd: estimated,
                    reserved_micro_usd: reservation,
                    charge_micro_usd: estimated.unwrap_or(reservation),
                    cost_basis: if estimated.is_some() {
                        "usage_priced_estimate"
                    } else {
                        "reserved_upper_bound"
                    },
                })
            }
            Err(_) if retry && !exceeds_reservation && attempt < config.max_retries => continue,
            Err(kind) => {
                return Err(EditorError {
                    kind,
                    ledger_id: Some(id),
                    request_id: request_id.map(String::into_boxed_str),
                    http_status: status.map(|s| s.as_u16()),
                    provider_error_code: provider_code.map(String::into_boxed_str),
                    usage: usage.map(Box::new),
                    latency_ms: Some(u32::try_from(latency_ms).unwrap_or(u32::MAX)),
                    estimated_micro_usd: estimated,
                    reserved_micro_usd: Some(reservation),
                    cost_basis: Some(if exceeds_reservation {
                        "usage_priced_estimate"
                    } else {
                        "reserved_upper_bound"
                    }),
                })
            }
        }
    }
    Err(EditorError::new(ErrorKind::Transport))
}

#[cfg(test)]
mod tests;
