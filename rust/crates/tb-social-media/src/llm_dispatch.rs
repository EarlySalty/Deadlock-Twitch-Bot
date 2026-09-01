//! Zentraler Fireworks-Dispatcher für die Social-Media-Anreicherung.
//!
//! Frühere Ollama-, MiniMax- und Claude-Bahnen sind entfernt. Auch dieser
//! Bereich nutzt ausschließlich [`tb_llm::complete`].

use async_trait::async_trait;
use sqlx::PgPool;

use crate::llm::{
    parse_llm_payload, render_user_prompt, LlmError, LlmRequest, LlmResponse, LlmTextResponse,
    SYSTEM_PROMPT,
};
use crate::settings;

const USE_CASE: &str = "social_media";
const TIMEOUT_SECONDS: u64 = 60;
const FIREWORKS_PRICE_INPUT_PER_1K: f64 = 0.0009;
const FIREWORKS_PRICE_OUTPUT_PER_1K: f64 = 0.0009;

#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn name(&self) -> &str;
    async fn generate(&self, request: &LlmRequest) -> Result<LlmResponse, LlmError>;
    async fn generate_text(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        max_tokens: i64,
        temperature: f64,
    ) -> Result<LlmTextResponse, LlmError>;
}

fn estimate_cost(prompt_tokens: i64, completion_tokens: i64) -> f64 {
    let input_rate = env_rate("FIREWORKS_PRICE_INPUT_PER_1K", FIREWORKS_PRICE_INPUT_PER_1K);
    let output_rate = env_rate(
        "FIREWORKS_PRICE_OUTPUT_PER_1K",
        FIREWORKS_PRICE_OUTPUT_PER_1K,
    );
    let raw = (prompt_tokens as f64 / 1000.0) * input_rate
        + (completion_tokens as f64 / 1000.0) * output_rate;
    (raw * 1_000_000.0).round() / 1_000_000.0
}

fn env_rate(var: &str, default: f64) -> f64 {
    std::env::var(var)
        .ok()
        .and_then(|value| value.trim().parse::<f64>().ok())
        .filter(|rate| *rate >= 0.0)
        .unwrap_or(default)
}

fn dispatch_error(error: tb_llm::LlmError) -> LlmError {
    match error {
        tb_llm::LlmError::Unavailable(detail) => LlmError::ProviderUnavailable(detail),
        other => LlmError::ProviderError(format!("Fireworks-Fehler: {other}")),
    }
}

struct FireworksProvider {
    endpoint: tb_llm::LlmEndpoint,
    temperature: f64,
}

impl FireworksProvider {
    fn from_connector() -> Result<Self, LlmError> {
        let endpoint = tb_llm::endpoint_for(USE_CASE);
        if endpoint.api_key.is_none() {
            return Err(LlmError::ProviderUnavailable(
                "kein Fireworks-Schlüssel für social_media".to_string(),
            ));
        }
        Ok(Self {
            endpoint,
            temperature: 0.4,
        })
    }

    #[cfg(test)]
    fn for_test(endpoint: tb_llm::LlmEndpoint) -> Self {
        Self {
            endpoint,
            temperature: 0.4,
        }
    }
}

#[async_trait]
impl LlmProvider for FireworksProvider {
    fn name(&self) -> &str {
        "fireworks"
    }

    async fn generate(&self, request: &LlmRequest) -> Result<LlmResponse, LlmError> {
        let prompt = render_user_prompt(request);
        let text = self
            .generate_text(SYSTEM_PROMPT, &prompt, 600, self.temperature)
            .await?;
        parse_llm_payload(
            &text.content,
            &text.provider,
            &text.model,
            text.cost_usd_estimate,
        )
    }

    async fn generate_text(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        max_tokens: i64,
        temperature: f64,
    ) -> Result<LlmTextResponse, LlmError> {
        let mut request = tb_llm::Request::simple(system_prompt, user_prompt)
            .max_tokens(max_tokens)
            .temperature(temperature)
            .timeout(std::time::Duration::from_secs(TIMEOUT_SECONDS))
            .ledger_purpose("social-media-fireworks")
            .endpoint(self.endpoint.clone());
        if system_prompt.to_lowercase().contains("strict json") {
            request = request.json_object();
        }

        let response = tb_llm::complete(USE_CASE, request)
            .await
            .map_err(dispatch_error)?;
        if response.text.trim().is_empty() {
            return Err(LlmError::ProviderError(
                "Fireworks lieferte leeren Inhalt".to_string(),
            ));
        }
        Ok(LlmTextResponse {
            content: response.text,
            provider: response.provider,
            model: response.model,
            cost_usd_estimate: Some(estimate_cost(
                response.prompt_tokens.unwrap_or(0),
                response.completion_tokens.unwrap_or(0),
            )),
        })
    }
}

/// Der Pool bleibt Teil des Konstruktors, damit die bestehende Verdrahtung
/// stabil bleibt. Anbieter- und Consent-Auswahl aus der Datenbank gibt es nicht
/// mehr: Fireworks ist der einzige freigegebene Weg.
pub struct LlmDispatcher {
    pool: PgPool,
}

impl LlmDispatcher {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    async fn ensure_consent(&self) -> Result<(), LlmError> {
        if settings::external_llm_consent(&self.pool).await {
            Ok(())
        } else {
            Err(LlmError::ProviderUnavailable(
                "keine Einwilligung für externe Social-Media-Anreicherung".to_string(),
            ))
        }
    }

    pub async fn generate(&self, request: &LlmRequest) -> Result<LlmResponse, LlmError> {
        self.ensure_consent().await?;
        FireworksProvider::from_connector()?.generate(request).await
    }

    pub async fn generate_text(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        max_tokens: i64,
        temperature: f64,
    ) -> Result<LlmTextResponse, LlmError> {
        self.ensure_consent().await?;
        FireworksProvider::from_connector()?
            .generate_text(system_prompt, user_prompt, max_tokens, temperature)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn inner_json() -> &'static str {
        "{\"youtube\":{\"title\":\"YT\",\"description\":\"d\",\"hashtags\":[\"haze\"]},\"tiktok\":{\"title\":\"TK\",\"description\":\"d\",\"hashtags\":[]},\"instagram\":{\"title\":\"IG\",\"description\":\"d\",\"hashtags\":[]}}"
    }

    #[tokio::test]
    async fn social_media_laeuft_ueber_fireworks_und_rechnet_fireworks_preis() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(body_string_contains(
                tb_llm::selection::FIREWORKS_DEFAULT_MODEL,
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{"message": {"content": inner_json()}}],
                "usage": {"prompt_tokens": 1000, "completion_tokens": 1000}
            })))
            .mount(&server)
            .await;
        let provider = FireworksProvider::for_test(tb_llm::LlmEndpoint {
            provider: "fireworks",
            base_url: server.uri(),
            model: tb_llm::selection::FIREWORKS_DEFAULT_MODEL.to_string(),
            api_key: Some("test-key".to_string()),
        });

        let response = provider.generate(&LlmRequest::default()).await.unwrap();
        assert_eq!(response.provider, "fireworks");
        assert_eq!(response.model, tb_llm::selection::FIREWORKS_DEFAULT_MODEL);
        assert_eq!(response.youtube.title.as_deref(), Some("YT"));
        assert_eq!(response.cost_usd_estimate, Some(0.0018));
    }
}
