//! Zentraler Schlüssel-Resolver für Fireworks.
//!
//! Secrets werden nie geloggt und ausschließlich durch Infisical/systemd in
//! den Prozess gegeben. Schlüssel früherer Anbieter werden nicht aufgelöst.

fn nonempty_env(var: &str) -> Option<String> {
    std::env::var(var)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

pub fn fireworks_api_key() -> Option<String> {
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
