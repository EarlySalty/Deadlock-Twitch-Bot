//! Konsolidierter API-Key-Resolver für die zwei Provider.
//!
//! Eine einzige Stelle für die Key-Auflösung — verhindert die in der Python-
//! Codebase und den verstreuten Rust-Clients dreifach duplizierte
//! Env-Fallback-Kette. **Secrets werden NIE geloggt**; Keys kommen ausschließlich
//! aus der Umgebung (Infisical/systemd injiziert sie). Keyring-Fallback bewusst
//! weggelassen (Grillme-Entscheidung `fernet-crypto-5`: Keys aus dem Tresor).
//!
//! Python-Orakel (`bot/core/llm_providers.py`):
//! - MiniMax: `MINIMAX_TOKEN_PLAN_KEY` → `MINIMAX_API_KEY` → `MINMAX`
//! - Anthropic: `ANTHROPIC_API_KEY`

/// Env-Var nur, wenn gesetzt UND nicht leer (mirror von Pythons `or`-Kette).
fn nonempty_env(var: &str) -> Option<String> {
    std::env::var(var).ok().filter(|v| !v.trim().is_empty())
}

/// Liefert den ersten nicht-leeren Wert aus der Var-Reihenfolge.
fn first_nonempty(vars: &[&str]) -> Option<String> {
    vars.iter().find_map(|v| nonempty_env(v))
}

/// MiniMax-Key: `MINIMAX_TOKEN_PLAN_KEY` → `MINIMAX_API_KEY` → `MINMAX`.
/// Reihenfolge 1:1 aus `get_minimax_client` (llm_providers.py).
pub fn minimax_api_key() -> Option<String> {
    first_nonempty(&["MINIMAX_TOKEN_PLAN_KEY", "MINIMAX_API_KEY", "MINMAX"])
}

/// Fireworks-Key: `FIREWORK_API_KEY` → `FIREWORKS_API_KEY`. Reihenfolge wie
/// im Discord-Bot (`dl-ai`), wo der Singular-Name der etablierte ist.
pub fn fireworks_api_key() -> Option<String> {
    first_nonempty(&["FIREWORK_API_KEY", "FIREWORKS_API_KEY"])
}

/// Anthropic-Key: `ANTHROPIC_API_KEY` (einzige Quelle, Python-Parität).
pub fn anthropic_api_key() -> Option<String> {
    nonempty_env("ANTHROPIC_API_KEY")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Serialisiert die Env-Mutationen über die Tests dieses Moduls.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn clear() {
        for v in [
            "MINIMAX_TOKEN_PLAN_KEY",
            "MINIMAX_API_KEY",
            "MINMAX",
            "ANTHROPIC_API_KEY",
            "FIREWORK_API_KEY",
            "FIREWORKS_API_KEY",
        ] {
            std::env::remove_var(v);
        }
    }

    #[test]
    fn minimax_key_reihenfolge_und_leer_uebersprungen() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear();
        assert_eq!(minimax_api_key(), None);

        // Leerer Plan-Key wird übersprungen, fällt auf API_KEY.
        std::env::set_var("MINIMAX_TOKEN_PLAN_KEY", "  ");
        std::env::set_var("MINIMAX_API_KEY", "k-api");
        assert_eq!(minimax_api_key().as_deref(), Some("k-api"));

        // Plan-Key hat Vorrang, wenn gesetzt.
        std::env::set_var("MINIMAX_TOKEN_PLAN_KEY", "k-plan");
        assert_eq!(minimax_api_key().as_deref(), Some("k-plan"));

        // Drittes Fallback MINMAX.
        clear();
        std::env::set_var("MINMAX", "k-legacy");
        assert_eq!(minimax_api_key().as_deref(), Some("k-legacy"));
        clear();
    }

    #[test]
    fn fireworks_key_bevorzugt_singular() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear();
        assert_eq!(fireworks_api_key(), None);

        std::env::set_var("FIREWORKS_API_KEY", "k-plural");
        assert_eq!(fireworks_api_key().as_deref(), Some("k-plural"));

        std::env::set_var("FIREWORK_API_KEY", "k-singular");
        assert_eq!(fireworks_api_key().as_deref(), Some("k-singular"));
        clear();
    }

    #[test]
    fn anthropic_key_aus_env() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        clear();
        assert_eq!(anthropic_api_key(), None);
        std::env::set_var("ANTHROPIC_API_KEY", "k-anthropic");
        assert_eq!(anthropic_api_key().as_deref(), Some("k-anthropic"));
        clear();
    }
}
