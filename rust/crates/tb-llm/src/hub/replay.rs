use super::*;
use crate::local_eval::{EvalError, Result};
use serde::{Deserialize, Serialize};

pub struct LocalClient {
    client: reqwest::Client,
    endpoint: LlmEndpoint,
}

pub(super) async fn finish_local(
    mut response: reqwest::Response,
) -> std::result::Result<Value, RawError> {
    if !response.status().is_success() {
        return Err(RawError::Fehler(LlmError::Http {
            status: response.status().as_u16(),
            body: String::new(),
        }));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| RawError::Fehler(LlmError::Transport("local_body".into())))?
    {
        if bytes.len() + chunk.len() > 65536 {
            return Err(RawError::Fehler(LlmError::Unparsable(
                "local_body_size".into(),
            )));
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes)
        .map_err(|_| RawError::Fehler(LlmError::Unparsable("local_json".into())))
}

#[derive(Deserialize, Serialize)]
pub struct EvalResponse {
    pub text: String,
    pub model: String,
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub latency_ms: u64,
    pub finish_reason: String,
    pub server_generation_tokens_per_second: Option<f64>,
    pub error_code: Option<String>,
    pub returned_model: Option<String>,
}

impl LocalClient {
    pub fn new(scope: &str, base_url: &str, model: &str) -> Result<Self> {
        if scope != "twitch" || !matches!(model, "qwen3.5-4b-local" | "qwen3.5-9b-local") {
            return Err(EvalError("scope_or_model"));
        }
        let url = reqwest::Url::parse(base_url).map_err(|_| EvalError("local_endpoint"))?;
        let port = url.port().ok_or(EvalError("local_endpoint"))?;
        let host = match url.host_str() {
            Some("127.0.0.1") => "127.0.0.1",
            Some("[::1]") => "[::1]",
            _ => return Err(EvalError("local_endpoint")),
        };
        if port == 0 || base_url != format!("http://{host}:{port}/v1") {
            return Err(EvalError("local_endpoint"));
        }
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .build()
            .map_err(|_| EvalError("client_initialization"))?;
        Ok(Self {
            client,
            endpoint: LlmEndpoint {
                provider: "local-eval",
                model: model.into(),
                base_url: base_url.into(),
                api_key: None,
            },
        })
    }

    pub async fn complete(&self, request: Request) -> Result<EvalResponse> {
        if request.max_tokens.is_none_or(|n| !(1..=512).contains(&n))
            || request
                .timeout
                .is_none_or(|t| t.is_zero() || t > Duration::from_secs(600))
            || request.failover
            || request.retry_on_429 != 0
            || request.allow_reasoning_content
            || request.endpoint.is_some()
            || !request.reasoning_off
        {
            return Err(EvalError("request_limits"));
        }
        let started = Instant::now();
        let payload = tokio::time::timeout(
            request.timeout.ok_or(EvalError("request_limits"))?,
            send_openai_compatible(&self.client, &self.endpoint, None, &request),
        )
        .await
        .map_err(|_| EvalError("timeout"))?
        .map_err(|error| match error {
            RawError::TooManyRequests { .. } => EvalError("rate_limit"),
            RawError::Fehler(error) => EvalError(error.code()),
        })?;
        Ok(parse_response(
            &payload,
            &self.endpoint.model,
            started.elapsed().as_millis() as u64,
        ))
    }
}

pub(super) fn parse_response(
    payload: &Value,
    expected_model: &str,
    latency_ms: u64,
) -> EvalResponse {
    let returned_model = payload.get("model").and_then(Value::as_str);
    let finish_reason = payload["choices"][0]["finish_reason"]
        .as_str()
        .unwrap_or("");
    let raw = extract_openai_text(payload, false);
    let text = strip_think(&raw);
    let error = if returned_model != Some(expected_model) {
        Some("model_mismatch")
    } else if finish_reason.is_empty() {
        Some("finish_reason_missing")
    } else if !matches!(finish_reason, "stop" | "length") {
        Some("finish_reason_unexpected")
    } else if text.is_empty() {
        Some("empty_answer")
    } else if text.to_lowercase().contains("<think") || text.to_lowercase().contains("</think") {
        Some("unfinished_thinking")
    } else {
        None
    };
    EvalResponse {
        text: if error.is_none() { text } else { String::new() },
        model: expected_model.into(),
        latency_ms,
        finish_reason: finish_reason.into(),
        prompt_tokens: usage_field(payload.get("usage"), &["prompt_tokens"])
            .filter(|n| *n <= 1_000_000),
        completion_tokens: usage_field(payload.get("usage"), &["completion_tokens"])
            .filter(|n| *n <= 1_000_000),
        server_generation_tokens_per_second: payload["timings"]["predicted_per_second"]
            .as_f64()
            .filter(|n| n.is_finite() && *n >= 0.0),
        error_code: error.map(str::to_string),
        returned_model: returned_model.map(|s| s.chars().take(200).collect()),
    }
}
