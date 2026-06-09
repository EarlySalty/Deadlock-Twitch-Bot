//! Runtime-verstellbares Poll-Intervall aus `twitch_global_settings`
//! (Key `poll_interval_seconds`, gültig 5–3600 s, Default 15 s) —
//! wird wie in Python vor jedem Tick gelesen.

use sqlx::PgPool;

pub const POLL_INTERVAL_DEFAULT_SECONDS: u64 = 15;
pub const POLL_INTERVAL_MIN_SECONDS: u64 = 5;
pub const POLL_INTERVAL_MAX_SECONDS: u64 = 3600;
const SETTING_KEY: &str = "poll_interval_seconds";

#[derive(Clone)]
pub struct PollIntervalStore {
    pool: PgPool,
}

impl PollIntervalStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Aktuelles Intervall: DB-Wert wenn gültig, sonst Default.
    /// Lesefehler sind nicht fatal (Default, debug-Log).
    pub async fn current_seconds(&self) -> u64 {
        let raw: Result<Option<String>, sqlx::Error> = sqlx::query_scalar(
            "SELECT setting_value FROM twitch_global_settings WHERE setting_key = $1 LIMIT 1",
        )
        .bind(SETTING_KEY)
        .fetch_optional(&self.pool)
        .await;
        match raw {
            Ok(Some(value)) => normalize(&value).unwrap_or_else(|| {
                tracing::debug!(value, "Ungültiges Poll-Intervall in DB — Default");
                POLL_INTERVAL_DEFAULT_SECONDS
            }),
            Ok(None) => POLL_INTERVAL_DEFAULT_SECONDS,
            Err(error) => {
                tracing::debug!(%error, "Poll-Intervall nicht lesbar — Default");
                POLL_INTERVAL_DEFAULT_SECONDS
            }
        }
    }
}

fn normalize(raw: &str) -> Option<u64> {
    let value: u64 = raw.trim().parse().ok()?;
    (POLL_INTERVAL_MIN_SECONDS..=POLL_INTERVAL_MAX_SECONDS)
        .contains(&value)
        .then_some(value)
}

#[cfg(test)]
mod tests {
    use super::normalize;

    #[test]
    fn normalize_klemmt_auf_gueltigen_bereich() {
        assert_eq!(normalize("15"), Some(15));
        assert_eq!(normalize(" 3600 "), Some(3600));
        assert_eq!(normalize("4"), None);
        assert_eq!(normalize("3601"), None);
        assert_eq!(normalize("abc"), None);
        assert_eq!(normalize(""), None);
    }
}
