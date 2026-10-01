//! Zentraler Schlüssel-Resolver für Fireworks.
//!
//! Secrets werden nie geloggt und ausschließlich durch Infisical/systemd in
//! den Prozess gegeben. Schlüssel früherer Anbieter werden nicht aufgelöst.

use std::sync::OnceLock;

type PrivateGetter = fn(&str) -> Option<String>;
static PRIVATE: OnceLock<PrivateGetter> = OnceLock::new();

/// Bot und Dashboard registrieren ihren vorhandenen Snapshot vor Hintergrundarbeit.
/// Nach Registrierung bleiben fehlende Werte fehlend, ohne zweite Quelle.
pub fn install_private_getter(get: PrivateGetter) -> Result<(), &'static str> {
    PRIVATE
        .set(get)
        .map_err(|_| "Privater LLM-Schlüsselgetter wurde bereits installiert.")
}

pub(crate) fn runtime_value(name: &str) -> Option<Option<String>> {
    PRIVATE.get().map(|get| get(name))
}

fn private_key(get: PrivateGetter) -> Option<String> {
    ["FIREWORK_API_KEY", "FIREWORKS_API_KEY"]
        .iter()
        .find_map(|name| get(name).filter(|value| !value.trim().is_empty()))
}

fn nonempty_env(var: &str) -> Option<String> {
    std::env::var(var)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

pub fn fireworks_api_key() -> Option<String> {
    if let Some(get) = PRIVATE.get() {
        return private_key(*get);
    }
    ["FIREWORK_API_KEY", "FIREWORKS_API_KEY"]
        .iter()
        .find_map(|name| nonempty_env(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn private_missing_key_stays_missing_and_alias_order_is_preserved() {
        assert!(private_key(|_| None).is_none());
        assert!(private_key(|_| Some("  ".to_owned())).is_none());
        assert_eq!(
            private_key(|name| Some(
                if name == "FIREWORK_API_KEY" {
                    "singular"
                } else {
                    "plural"
                }
                .to_owned()
            ))
            .as_deref(),
            Some("singular")
        );
    }

    #[test]
    fn singularer_fireworks_name_hat_vorrang() {
        let _guard = ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        std::env::remove_var("FIREWORK_API_KEY");
        std::env::remove_var("FIREWORKS_API_KEY");
        assert_eq!(fireworks_api_key(), None);
        std::env::set_var("FIREWORKS_API_KEY", "plural");
        assert_eq!(fireworks_api_key().as_deref(), Some("plural"));
        std::env::set_var("FIREWORK_API_KEY", "singular");
        assert_eq!(fireworks_api_key().as_deref(), Some("singular"));
        std::env::remove_var("FIREWORK_API_KEY");
        std::env::remove_var("FIREWORKS_API_KEY");
    }
}
