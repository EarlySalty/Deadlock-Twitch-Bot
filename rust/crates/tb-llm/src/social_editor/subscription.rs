use std::{
    path::Path,
    time::{Duration, Instant},
};

use serde_json::{json, Value};
use sqlx::PgPool;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::{
    begin_entry, content_blocks, schema, validate_request, AttemptEntry, EditorConfig,
    EditorContext, EditorError, EditorRequest, EditorResponse, ErrorKind, SubscriptionRelay, Usage,
    MAX_RESPONSE_BYTES,
};
use crate::ledger::{SUBSCRIPTION_INCLUDED, SUBSCRIPTION_PROVIDER};

pub(super) enum Outcome {
    Stop(EditorError),
    Fallback(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Failure {
    Unreachable,
    Timeout,
    Limit,
    Busy,
    CliError,
    Rejected,
    InvalidResponse,
    ModelMismatch,
}

impl Failure {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Unreachable => "unreachable",
            Self::Timeout => "timeout",
            Self::Limit => "limit",
            Self::Busy => "busy",
            Self::CliError => "cli_error",
            Self::Rejected => "invalid_request",
            Self::InvalidResponse => "invalid_response",
            Self::ModelMismatch => "model_mismatch",
        }
    }
}

#[derive(Debug)]
pub(super) struct Reply {
    pub(super) model: String,
    pub(super) content: Value,
    pub(super) usage: Usage,
}

fn payload(
    configuration: &EditorConfig,
    relay: &SubscriptionRelay,
    request: &EditorRequest,
) -> Result<Vec<u8>, EditorError> {
    let body = json!({
        "model": configuration.provider.model,
        "system": request.system,
        "json_schema": schema::conformant_wire(&request.json_schema)?,
        "timeout_ms": relay.request_timeout_ms,
        "content": content_blocks(request),
    });
    let mut line =
        serde_json::to_vec(&body).map_err(|_| EditorError::new(ErrorKind::InvalidRequest))?;
    line.push(b'\n');
    Ok(line)
}

async fn exchange(socket: &Path, line: &[u8]) -> Result<Vec<u8>, Failure> {
    let mut stream = tokio::net::UnixStream::connect(socket)
        .await
        .map_err(|_| Failure::Unreachable)?;
    stream
        .write_all(line)
        .await
        .map_err(|_| Failure::Unreachable)?;
    stream.shutdown().await.map_err(|_| Failure::Unreachable)?;
    let mut reply = Vec::new();
    (&mut stream)
        .take(MAX_RESPONSE_BYTES as u64 + 1)
        .read_to_end(&mut reply)
        .await
        .map_err(|_| Failure::InvalidResponse)?;
    if reply.len() > MAX_RESPONSE_BYTES {
        return Err(Failure::InvalidResponse);
    }
    Ok(reply)
}

pub(super) fn interpret(
    body: &[u8],
    validator: &jsonschema::Validator,
    model: &str,
) -> Result<Reply, Failure> {
    let reply: Value = serde_json::from_slice(body).map_err(|_| Failure::InvalidResponse)?;
    match reply.get("ok").and_then(Value::as_bool) {
        Some(true) => (),
        Some(false) => {
            return Err(match reply.get("kind").and_then(Value::as_str) {
                Some("limit") => Failure::Limit,
                Some("timeout") => Failure::Timeout,
                Some("busy") => Failure::Busy,
                Some("invalid_request") => Failure::Rejected,
                _ => Failure::CliError,
            })
        }
        None => return Err(Failure::InvalidResponse),
    }
    let served = reply
        .get("model")
        .and_then(Value::as_str)
        .ok_or(Failure::InvalidResponse)?;
    if served != model {
        return Err(Failure::ModelMismatch);
    }
    let content = reply
        .get("structured_output")
        .filter(|content| validator.is_valid(content))
        .cloned()
        .ok_or(Failure::InvalidResponse)?;
    let usage = reply
        .get("usage")
        .and_then(Usage::from_value)
        .ok_or(Failure::InvalidResponse)?;
    Ok(Reply {
        model: served.to_owned(),
        content,
        usage,
    })
}

const CLIENT_TIMEOUT_MARGIN_MS: u64 = 15_000;
const ABANDON_GRACE_MS: u64 = 60_000;

fn client_timeout_ms(relay: &SubscriptionRelay) -> u64 {
    relay
        .request_timeout_ms
        .saturating_add(CLIENT_TIMEOUT_MARGIN_MS)
}

async fn call(
    relay: &SubscriptionRelay,
    line: &[u8],
    validator: &jsonschema::Validator,
    model: &str,
) -> Result<Reply, Failure> {
    let body = tokio::time::timeout(
        Duration::from_millis(client_timeout_ms(relay)),
        exchange(&relay.socket, line),
    )
    .await
    .map_err(|_| Failure::Timeout)??;
    interpret(&body, validator, model)
}

fn count(value: Option<u64>) -> Option<i64> {
    value.and_then(|n| i64::try_from(n).ok())
}

pub(super) fn completion(
    id: i64,
    usage: Option<&Usage>,
    latency_ms: u64,
    failure: Option<Failure>,
) -> crate::ledger::Completion {
    let tokens_in = count(usage.and_then(|u| u.input_tokens));
    let tokens_out = count(usage.and_then(|u| u.output_tokens));
    crate::ledger::Completion {
        id,
        tokens_in,
        tokens_out,
        total: tokens_in.and_then(|a| tokens_out.and_then(|b| a.checked_add(b))),
        request_id: None,
        success: failure.is_none(),
        error_code: failure.map(|failure| format!("Subscription/{}", failure.as_str())),
        http_status: None,
        latency_ms: i64::try_from(latency_ms).unwrap_or(i64::MAX),
        model: None,
        social_accounting: Some(crate::ledger::SocialAccounting {
            reserved_micro_usd: 0,
            estimated_micro_usd: Some(0),
            cost_basis: SUBSCRIPTION_INCLUDED.to_owned(),
            cache_creation_input_tokens: count(usage.and_then(|u| u.cache_creation_input_tokens)),
            cache_read_input_tokens: count(usage.and_then(|u| u.cache_read_input_tokens)),
        }),
    }
}

fn stops(kind: ErrorKind) -> bool {
    matches!(
        kind,
        ErrorKind::Configuration
            | ErrorKind::InvalidRequest
            | ErrorKind::Consent
            | ErrorKind::Budget
    )
}

pub(super) async fn attempt(
    pool: &PgPool,
    context: &EditorContext,
    configuration: &EditorConfig,
    request: &EditorRequest,
    relay: &SubscriptionRelay,
) -> Result<EditorResponse, Outcome> {
    configuration.validate().map_err(Outcome::Stop)?;
    let config = &configuration.provider;
    let validator = validate_request(config, context, request).map_err(Outcome::Stop)?;
    let line = payload(configuration, relay, request).map_err(Outcome::Stop)?;
    let entry = AttemptEntry {
        provider: SUBSCRIPTION_PROVIDER,
        reservation: 0,
        estimated: Some(0),
        cost_basis: SUBSCRIPTION_INCLUDED,
        abandon_after_ms: Some(client_timeout_ms(relay).saturating_add(ABANDON_GRACE_MS)),
    };
    let reserved = match begin_entry(pool, config, context, &entry).await {
        Ok(reserved) => reserved,
        Err(error) if stops(error.kind) => return Err(Outcome::Stop(error)),
        Err(error) => {
            tracing::warn!(stage = %context.stage, kind = ?error.kind, "Social AI subscription ledger unavailable");
            return Err(Outcome::Fallback("ledger"));
        }
    };
    let id = reserved.id;
    let start = Instant::now();
    let result = call(relay, &line, &validator, &config.model).await;
    let latency_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
    let completed = completion(
        id,
        result.as_ref().ok().map(|reply| &reply.usage),
        latency_ms,
        result.as_ref().err().copied(),
    );
    if let Err(mut error) = reserved.finalize(pool, &completed).await {
        crate::ledger::journal_completion(&completed);
        error.ledger_id = Some(id);
        return Err(Outcome::Stop(error));
    }
    match result {
        Ok(reply) => Ok(EditorResponse {
            provider: SUBSCRIPTION_PROVIDER.to_owned(),
            model: reply.model,
            content: reply.content,
            usage: Some(reply.usage),
            latency_ms,
            request_id: None,
            ledger_id: id,
            estimated_micro_usd: Some(0),
            reserved_micro_usd: 0,
            charge_micro_usd: 0,
            cost_basis: SUBSCRIPTION_INCLUDED,
        }),
        Err(failure) => {
            tracing::warn!(
                stage = %context.stage,
                ledger_id = id,
                reason = failure.as_str(),
                "Social AI subscription relay failed"
            );
            Err(Outcome::Fallback(failure.as_str()))
        }
    }
}
