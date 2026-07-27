//! Provider-Auswahl: welches Modell bedient welchen Anwendungsfall.
//!
//! Vorbild ist der Discord-Bot (`dl-ai`): ein Default plus Overrides pro
//! Anwendungsfall, alles über Umgebungsvariablen. So lässt sich ein einzelnes
//! Feature auf einen anderen Anbieter ziehen, ohne den Rest anzufassen.
//!
//! - `TB_LLM_PROVIDER_DEFAULT` — Basis für alles.
//! - `TB_LLM_PROVIDER_<USE_CASE>` — überschreibt einzeln, z. B.
//!   `TB_LLM_PROVIDER_INVITE_QUESTION=minimax`.
//!
//! Ohne jede Konfiguration gilt: Fireworks/DeepSeek, wenn ein Key da ist,
//! sonst MiniMax. Ein unbekannter Name fällt ebenfalls auf diesen Weg zurück
//! und wird geloggt — eine Namensverwechslung darf den Bot nicht stumm
//! schalten.
//!
//! [`endpoint_for`] ist die gemeinsame Basis: Sie liefert Adresse, Modell und
//! Key. Die fachlichen Aufrufer behalten ihren jeweiligen HTTP-, Retry- und
//! Fehlerpfad und beziehen nur diese drei Werte zentral.

use tracing::{info, warn};

use crate::minimax;

const PROVIDER_DEFAULT_ENV: &str = "TB_LLM_PROVIDER_DEFAULT";
/// Fireworks-Endpunkt (OpenAI-kompatibel), identisch zum Discord-Bot.
pub const FIREWORKS_BASE_URL: &str = "https://api.fireworks.ai/inference/v1";
/// DeepSeek über Fireworks. Benchmark 11.07.2026: 56/56 auf echten
/// Produktionsfällen (30 obfuskierter Spam, 26 harmlose Viewer-Sätze).
pub const FIREWORKS_DEFAULT_MODEL: &str = "accounts/fireworks/models/deepseek-v4-flash";

/// Adresse, Modell und Key eines Anbieters — alles, was ein
/// OpenAI-kompatibler Call braucht. Der Key wird nie geloggt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmEndpoint {
    pub provider: &'static str,
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
}

/// Endpunkt für einen Anwendungsfall (z. B. `"invite_question"`).
pub fn endpoint_for(use_case: &str) -> LlmEndpoint {
    let env_name = format!("TB_LLM_PROVIDER_{}", use_case.to_uppercase());
    let configured = nonempty_env(&env_name).or_else(|| nonempty_env(PROVIDER_DEFAULT_ENV));
    resolve(configured.as_deref(), use_case)
}

/// Endpunkt-Kette für einen Anwendungsfall: bevorzugter Anbieter zuerst, der
/// andere als Ausweichweg. Aufrufer mit Failover (der Spam-Judge) arbeiten die
/// Kette ab, statt beim ersten Fehler aufzugeben. Einträge ohne Key entfallen.
pub fn endpoint_chain(use_case: &str) -> Vec<LlmEndpoint> {
    let primary = endpoint_for(use_case);
    let fallback = if primary.provider == "fireworks" {
        minimax_endpoint()
    } else {
        fireworks_endpoint()
    };
    [primary, fallback]
        .into_iter()
        .filter(|endpoint| endpoint.api_key.is_some())
        .collect()
}

fn resolve(configured: Option<&str>, use_case: &str) -> LlmEndpoint {
    match configured.map(str::trim).map(str::to_lowercase).as_deref() {
        Some("minimax") => minimax_endpoint(),
        Some("fireworks") | Some("deepseek") => fireworks_endpoint(),
        Some(other) => {
            warn!(
                provider = other,
                use_case, "unbekannter LLM-Provider konfiguriert, nutze Auto-Auswahl"
            );
            auto(use_case)
        }
        None => auto(use_case),
    }
}

/// Fireworks, wenn ein Key vorliegt — sonst MiniMax. Ohne diesen Check würde
/// eine fehlende Fireworks-Konfiguration erst beim ersten Call auffallen.
fn auto(use_case: &str) -> LlmEndpoint {
    let fireworks = fireworks_endpoint();
    if fireworks.api_key.is_some() {
        info!(
            use_case,
            model = %fireworks.model,
            "LLM-Provider: Fireworks"
        );
        return fireworks;
    }
    let fallback = minimax_endpoint();
    info!(use_case, model = %fallback.model, "LLM-Provider: MiniMax");
    fallback
}

fn fireworks_endpoint() -> LlmEndpoint {
    LlmEndpoint {
        provider: "fireworks",
        base_url: nonempty_env("FIREWORK_BASE_URL")
            .or_else(|| nonempty_env("FIREWORKS_BASE_URL"))
            .unwrap_or_else(|| FIREWORKS_BASE_URL.to_string()),
        model: nonempty_env("FIREWORK_MODEL")
            .or_else(|| nonempty_env("FIREWORKS_MODEL"))
            .unwrap_or_else(|| FIREWORKS_DEFAULT_MODEL.to_string()),
        api_key: crate::keys::fireworks_api_key(),
    }
}

fn minimax_endpoint() -> LlmEndpoint {
    LlmEndpoint {
        provider: "minimax",
        base_url: nonempty_env("MINIMAX_BASE_URL")
            .unwrap_or_else(|| minimax::DEFAULT_BASE_URL.to_string()),
        model: nonempty_env("MINIMAX_MODEL").unwrap_or_else(|| minimax::DEFAULT_MODEL.to_string()),
        api_key: crate::keys::minimax_api_key(),
    }
}

fn nonempty_env(var: &str) -> Option<String> {
    std::env::var(var).ok().filter(|v| !v.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn clear() {
        for v in [
            PROVIDER_DEFAULT_ENV,
            "TB_LLM_PROVIDER_INVITE_QUESTION",
            "FIREWORK_API_KEY",
            "FIREWORKS_API_KEY",
            "FIREWORK_BASE_URL",
            "FIREWORKS_BASE_URL",
            "FIREWORKS_MODEL",
            "FIREWORK_MODEL",
            "MINIMAX_API_KEY",
            "MINIMAX_TOKEN_PLAN_KEY",
            "MINIMAX_MODEL",
            "MINMAX",
        ] {
            std::env::remove_var(v);
        }
    }

    #[test]
    fn use_case_override_schlaegt_default() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear();
        std::env::set_var(PROVIDER_DEFAULT_ENV, "minimax");
        std::env::set_var("TB_LLM_PROVIDER_INVITE_QUESTION", "fireworks");

        assert_eq!(endpoint_for("invite_question").provider, "fireworks");
        assert_eq!(endpoint_for("title_ai").provider, "minimax");
        clear();
    }

    #[test]
    fn ohne_konfiguration_entscheidet_der_key() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear();
        assert_eq!(endpoint_for("default").provider, "minimax");

        std::env::set_var("FIREWORK_API_KEY", "k");
        let endpoint = endpoint_for("default");
        assert_eq!(endpoint.provider, "fireworks");
        assert_eq!(endpoint.model, FIREWORKS_DEFAULT_MODEL);
        assert_eq!(endpoint.base_url, FIREWORKS_BASE_URL);
        clear();
    }

    #[test]
    fn deepseek_ist_ein_alias_fuer_fireworks() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear();
        std::env::set_var(PROVIDER_DEFAULT_ENV, "deepseek");
        assert_eq!(endpoint_for("default").provider, "fireworks");
        clear();
    }

    #[test]
    fn modell_ist_ueberschreibbar() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear();
        std::env::set_var(PROVIDER_DEFAULT_ENV, "fireworks");
        std::env::set_var("FIREWORKS_MODEL", "accounts/fireworks/models/eigenes");
        assert_eq!(
            endpoint_for("default").model,
            "accounts/fireworks/models/eigenes"
        );
        clear();
    }

    #[test]
    fn fireworks_base_url_bevorzugt_singular_alias() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear();
        std::env::set_var(PROVIDER_DEFAULT_ENV, "fireworks");
        std::env::set_var("FIREWORK_BASE_URL", "https://singular.example/v1");
        std::env::set_var("FIREWORKS_BASE_URL", "https://plural.example/v1");
        std::env::set_var("FIREWORK_MODEL", "singular-model");
        std::env::set_var("FIREWORKS_MODEL", "plural-model");

        let endpoint = endpoint_for("default");
        assert_eq!(endpoint.base_url, "https://singular.example/v1");
        assert_eq!(endpoint.model, "singular-model");
        clear();
    }

    #[test]
    fn kette_stellt_den_gewaehlten_anbieter_nach_vorn() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear();
        // Nur MiniMax konfiguriert: kein Fireworks-Eintrag ohne Key.
        std::env::set_var("MINIMAX_API_KEY", "m");
        let chain = endpoint_chain("default");
        assert_eq!(chain.len(), 1);
        assert_eq!(chain[0].provider, "minimax");

        // Beide Keys: Fireworks führt, MiniMax bleibt als Ausweichweg.
        std::env::set_var("FIREWORK_API_KEY", "f");
        let chain = endpoint_chain("default");
        assert_eq!(chain.len(), 2);
        assert_eq!(chain[0].provider, "fireworks");
        assert_eq!(chain[1].provider, "minimax");

        // Umgeschaltet: MiniMax führt, Fireworks weicht aus.
        std::env::set_var(PROVIDER_DEFAULT_ENV, "minimax");
        let chain = endpoint_chain("default");
        assert_eq!(chain[0].provider, "minimax");
        assert_eq!(chain[1].provider, "fireworks");
        clear();
    }

    #[test]
    fn unbekannter_name_faellt_zurueck_statt_zu_scheitern() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear();
        std::env::set_var(PROVIDER_DEFAULT_ENV, "gibtsnicht");
        assert_eq!(endpoint_for("default").provider, "minimax");
        clear();
    }
}
