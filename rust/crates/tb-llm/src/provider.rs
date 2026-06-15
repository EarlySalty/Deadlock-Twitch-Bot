//! Gemeinsamer Provider-Port und die geteilten Request-/Response-Typen.
//!
//! Genau zwei Implementierungen: [`crate::minimax::MiniMaxClient`] (Primär) und
//! [`crate::anthropic::AnthropicClient`] (Premium/`ai_full`). Beide sprechen
//! diesen Port, sodass Aufrufer austauschbar gegen das Trait programmieren können,
//! ohne ihre eigene Logik umzubauen.
//!
//! **Provider-AUSWAHL** (welcher Provider für welches Feature/Entitlement) ist
//! bewusst NICHT Teil dieser Schicht — sie liegt im Aufrufer (Python-Orakel:
//! `bot/analytics/api_ai.py:_plan_ai_model`, `analytics.ai_full` → Anthropic,
//! `analytics.ai_mini` → MiniMax). Diese Foundation liefert nur die Clients.

use async_trait::async_trait;

/// Eine Chat-Nachricht für einen Completion-Call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// `system` | `user` | `assistant`.
    pub role: String,
    pub content: String,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self { role: "system".to_string(), content: content.into() }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self { role: "user".to_string(), content: content.into() }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self { role: "assistant".to_string(), content: content.into() }
    }
}

/// Parameter eines Completion-Calls. `system` wird vom jeweiligen Provider
/// passend platziert (MiniMax: als `system`-Message; Anthropic: als top-level
/// `system`-Feld).
#[derive(Debug, Clone)]
pub struct CompletionRequest {
    pub system: Option<String>,
    pub messages: Vec<Message>,
    pub max_tokens: i64,
    pub temperature: Option<f64>,
}

impl CompletionRequest {
    /// Bequemer Konstruktor für den häufigen system+user-Fall.
    pub fn simple(system: impl Into<String>, user: impl Into<String>, max_tokens: i64) -> Self {
        Self {
            system: Some(system.into()),
            messages: vec![Message::user(user)],
            max_tokens,
            temperature: None,
        }
    }
}

/// Antwort eines Completion-Calls inkl. Token-Telemetrie. Die Telemetrie ist
/// bereits ins Ledger verbucht (best-effort) — die Felder dienen dem Aufrufer für
/// eigenes Logging/Audit (z. B. `twitch_engagement_log`).
#[derive(Debug, Clone, PartialEq)]
pub struct CompletionResponse {
    pub text: String,
    pub model: String,
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub latency_ms: i64,
}

/// Fehler eines Provider-Calls.
#[derive(Debug)]
pub enum LlmError {
    /// Kein API-Key konfiguriert (Python `LLMSecretNotFoundError`).
    Unavailable(String),
    /// Transport-/HTTP-/Parse-Fehler. Die Message trägt ggf. den Response-Body,
    /// damit der Aufrufer Provider-Fehlertexte (z. B. „credit balance too low")
    /// auswerten kann.
    Http(String),
}

impl std::fmt::Display for LlmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LlmError::Unavailable(e) => write!(f, "LLM-Provider nicht verfügbar: {e}"),
            LlmError::Http(e) => write!(f, "LLM-Call fehlgeschlagen: {e}"),
        }
    }
}

impl std::error::Error for LlmError {}

/// Gemeinsamer Port für die zwei LLM-Provider.
///
/// Eine erfolgreiche [`Self::complete`]-Implementierung verbucht die Tokens
/// best-effort ins gemeinsame MiniMax-Usage-Ledger (`source='twitch-bot'`,
/// `purpose=…`) — ein DB-Fehler ist NIE ein harter Fehler des Calls.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Stabiler Provider-Name (`minimax` | `anthropic`).
    fn name(&self) -> &'static str;

    /// Das gelockte Modell (für Logging/Persistenz).
    fn model(&self) -> &str;

    /// Führt einen Completion-Call aus und verbucht bei Erfolg die Tokens unter
    /// `purpose` ins Ledger.
    async fn complete(
        &self,
        request: &CompletionRequest,
        purpose: &str,
    ) -> Result<CompletionResponse, LlmError>;
}
