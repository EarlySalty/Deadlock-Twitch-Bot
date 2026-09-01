//! Feste LLM-Auswahl des Twitch-Bots.
//!
//! Jeder Anwendungsfall läuft ausschließlich über DeepSeek V4 Flash bei
//! Fireworks. Frühere Provider- und Modell-Overrides werden bewusst ignoriert.

use std::sync::OnceLock;
use tracing::warn;

pub const FIREWORKS_BASE_URL: &str = "https://api.fireworks.ai/inference/v1";
pub const FIREWORKS_DEFAULT_MODEL: &str = "accounts/fireworks/models/deepseek-v4-flash-0731";

/// Bestehende öffentliche Konstante für fachliche Guard-Tests. Inzwischen gilt
/// dieselbe Fireworks-Bindung für jeden Anwendungsfall.
pub const FIREWORKS_ONLY_USE_CASES: &[&str] = &["ricky_crew_review", "outreach_shadow"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmEndpoint {
    pub provider: &'static str,
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
}

/// Anbieter und Modell sind absichtlich nicht mehr pro Anwendungsfall
/// konfigurierbar. `use_case` bleibt für Ledger und Warnungen erhalten.
pub fn endpoint_for(use_case: &str) -> LlmEndpoint {
    warne_ignorierte_altanbieter(use_case);
    fireworks_endpoint()
}

/// Ohne Fireworks-Schlüssel bleibt die Kette leer. Es gibt keinen Rückfall auf
/// einen Altanbieter.
pub fn endpoint_chain(use_case: &str) -> Vec<LlmEndpoint> {
    let endpoint = endpoint_for(use_case);
    if endpoint.api_key.is_some() {
        vec![endpoint]
    } else {
        Vec::new()
    }
}

/// Eine umgebogene Basis-URL bleibt für lokale Mock- und Proxy-Tests möglich.
/// Der Modellname selbst bleibt fest.
pub(crate) fn fireworks_base_url() -> String {
    nonempty_env("FIREWORK_BASE_URL")
        .or_else(|| nonempty_env("FIREWORKS_BASE_URL"))
        .unwrap_or_else(|| FIREWORKS_BASE_URL.to_string())
}

fn fireworks_endpoint() -> LlmEndpoint {
    LlmEndpoint {
        provider: "fireworks",
        base_url: fireworks_base_url(),
        model: FIREWORKS_DEFAULT_MODEL.to_string(),
        api_key: crate::keys::fireworks_api_key(),
    }
}

fn nonempty_env(var: &str) -> Option<String> {
    std::env::var(var)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

fn warne_ignorierte_altanbieter(use_case: &str) {
    static GEWARNT: OnceLock<()> = OnceLock::new();
    let altanbieter_gesetzt = [
        "MINIMAX_TOKEN_PLAN_KEY",
        "MINIMAX_API_KEY",
        "MINMAX",
        "ANTHROPIC_API_KEY",
        "TB_LLM_PROVIDER_DEFAULT",
    ]
    .iter()
    .any(|name| nonempty_env(name).is_some())
        || nonempty_env(&format!("TB_LLM_PROVIDER_{}", use_case.to_uppercase())).is_some()
        || nonempty_env(&format!("TB_LLM_MODEL_{}", use_case.to_uppercase())).is_some();

    if altanbieter_gesetzt {
        GEWARNT.get_or_init(|| {
            warn!(
                "LLM-Altanbieter- oder Modell-Override ignoriert; der Twitch-Bot nutzt ausschließlich das freigegebene Fireworks-Modell"
            );
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn clear() {
        for name in [
            "FIREWORK_API_KEY",
            "FIREWORKS_API_KEY",
            "FIREWORK_BASE_URL",
            "FIREWORKS_BASE_URL",
            "FIREWORK_MODEL",
            "FIREWORKS_MODEL",
            "MINIMAX_TOKEN_PLAN_KEY",
            "MINIMAX_API_KEY",
            "MINMAX",
            "ANTHROPIC_API_KEY",
            "TB_LLM_PROVIDER_DEFAULT",
            "TB_LLM_PROVIDER_AI_CHAT",
            "TB_LLM_MODEL_AI_CHAT",
        ] {
            std::env::remove_var(name);
        }
    }

    #[test]
    fn jeder_anwendungsfall_ist_fireworks_mit_festem_modell() {
        let _guard = ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        clear();
        for use_case in ["dashboard_self_explainer", "ai_chat", "spam_judge"] {
            let endpoint = endpoint_for(use_case);
            assert_eq!(endpoint.provider, "fireworks");
            assert_eq!(endpoint.model, FIREWORKS_DEFAULT_MODEL);
        }
        clear();
    }

    #[test]
    fn altanbieter_und_modell_override_haben_keine_wirkung() {
        let _guard = ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        clear();
        std::env::set_var("MINIMAX_API_KEY", "alt-key");
        std::env::set_var("ANTHROPIC_API_KEY", "alt-key");
        std::env::set_var("TB_LLM_PROVIDER_AI_CHAT", "minimax");
        std::env::set_var("TB_LLM_MODEL_AI_CHAT", "fremdes-modell");

        let endpoint = endpoint_for("ai_chat");
        assert_eq!(endpoint.provider, "fireworks");
        assert_eq!(endpoint.model, FIREWORKS_DEFAULT_MODEL);
        assert!(endpoint.api_key.is_none());
        assert!(endpoint_chain("ai_chat").is_empty());
        clear();
    }

    #[test]
    fn kette_enthaelt_nur_fireworks_wenn_der_schluessel_da_ist() {
        let _guard = ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        clear();
        std::env::set_var("FIREWORK_API_KEY", "test-key");
        let chain = endpoint_chain("spam_judge");
        assert_eq!(chain.len(), 1);
        assert_eq!(chain[0].provider, "fireworks");
        assert_eq!(chain[0].model, FIREWORKS_DEFAULT_MODEL);
        clear();
    }
}
