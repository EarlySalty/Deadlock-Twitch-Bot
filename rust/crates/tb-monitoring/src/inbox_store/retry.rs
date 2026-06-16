//! Retry-Wrapper für die Schreiboperationen der Processing-Inbox.
//!
//! Parität zum Python-Orakel (`bot/storage/pg.py`): jede Inbox-Schreibung läuft
//! dort durch `storage.transaction()`, das bei den SQLSTATEs `40001`
//! (serialization_failure) und `40P01` (deadlock_detected) mit beschränktem
//! Backoff erneut versucht. Der native Inbox-Store schrieb bisher direkt gegen
//! den Pool ohne diesen Schutz — ein durch Concurrency (Python- und Rust-Worker
//! leasen denselben Tisch) ausgelöster Serialisierungs-Abbruch ließ die
//! Schreibung hart scheitern. [`with_write_retry`] schließt diese Lücke.
//!
//! Knöpfe + Defaults spiegeln Python (`TWITCH_ANALYTICS_TX_RETRY_*`): 3 Versuche,
//! Backoff `0.10s..=0.75s` (verdoppelnd). Pure und ohne Env-Lesen testbar.

use std::time::Duration;

/// Postgres-SQLSTATEs, bei denen ein erneuter Versuch erfolgversprechend ist.
const SERIALIZATION_FAILURE: &str = "40001";
const DEADLOCK_DETECTED: &str = "40P01";

/// Retry-Politik. Pure; liest selbst keine Env (siehe [`RetryPolicy::from_env`]).
#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    /// Maximale Anzahl Versuche (inkl. Erstversuch); minimal 1.
    pub max_attempts: u32,
    /// Basis-Verzögerung des ersten Retry-Schlafs.
    pub base_delay: Duration,
    /// Obergrenze einer einzelnen Verzögerung.
    pub max_delay: Duration,
}

impl Default for RetryPolicy {
    /// Werte des Python-Orakels: 3 Versuche, Backoff `0.10s..=0.75s`.
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_millis(750),
        }
    }
}

impl RetryPolicy {
    /// Lädt die Politik aus denselben Env-Variablen wie Python; fehlende oder
    /// ungültige Werte fallen auf [`RetryPolicy::default`] zurück.
    pub fn from_env() -> Self {
        let d = Self::default();
        Self {
            max_attempts: env_u32("TWITCH_ANALYTICS_TX_RETRY_ATTEMPTS")
                .map(|v| v.max(1))
                .unwrap_or(d.max_attempts),
            base_delay: env_secs_f64("TWITCH_ANALYTICS_TX_RETRY_BASE_DELAY_SECONDS")
                .map(|s| Duration::from_secs_f64(s.max(0.01)))
                .unwrap_or(d.base_delay),
            max_delay: env_secs_f64("TWITCH_ANALYTICS_TX_RETRY_MAX_DELAY_SECONDS")
                .map(|s| Duration::from_secs_f64(s.max(0.05)))
                .unwrap_or(d.max_delay),
        }
    }

    /// Backoff für den Schlaf **nach** dem `attempt`-ten fehlgeschlagenen Versuch
    /// (`attempt` 1-basiert). Verdoppelt ab `base_delay`, gedeckelt durch
    /// `max_delay` — identisch zu `_transaction_retry_sleep` in Python.
    pub fn backoff(&self, attempt: u32) -> Duration {
        let factor = 1u32.checked_shl(attempt.saturating_sub(1)).unwrap_or(u32::MAX);
        self.base_delay.saturating_mul(factor).min(self.max_delay)
    }
}

/// `true`, wenn der Fehler ein retrybarer Serialisierungs-/Deadlock-Abbruch ist.
pub fn is_retryable(err: &sqlx::Error) -> bool {
    matches!(
        err.as_database_error().and_then(|db| db.code()).as_deref(),
        Some(SERIALIZATION_FAILURE | DEADLOCK_DETECTED)
    )
}

/// Führt eine Inbox-Schreiboperation aus und versucht sie bei retrybaren
/// Fehlern (`40001`/`40P01`) mit exponentiellem Backoff erneut. Nicht-retrybare
/// Fehler und der letzte erschöpfte Versuch werden unverändert weitergereicht.
///
/// `operation` muss bei jedem Versuch erneut aufrufbar sein (`FnMut`), da ein
/// Serialisierungs-Abbruch die Transaktion serverseitig zurückrollt.
pub async fn with_write_retry<F, Fut, T>(policy: RetryPolicy, mut operation: F) -> Result<T, sqlx::Error>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, sqlx::Error>>,
{
    let max_attempts = policy.max_attempts.max(1);
    for attempt in 1..=max_attempts {
        match operation().await {
            Ok(value) => return Ok(value),
            Err(err) => {
                if attempt >= max_attempts || !is_retryable(&err) {
                    return Err(err);
                }
                tracing::warn!(
                    sqlstate = err.as_database_error().and_then(|db| db.code()).as_deref(),
                    attempt = attempt + 1,
                    max_attempts,
                    "Inbox-Schreibung nach Serialisierungs-/Deadlock-Abbruch erneut versuchen"
                );
                tokio::time::sleep(policy.backoff(attempt)).await;
            }
        }
    }
    unreachable!("retry loop must return within max_attempts")
}

fn env_u32(key: &str) -> Option<u32> {
    std::env::var(key).ok()?.trim().parse().ok()
}

fn env_secs_f64(key: &str) -> Option<f64> {
    std::env::var(key).ok()?.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn default_policy_matches_python_oracle() {
        let p = RetryPolicy::default();
        assert_eq!(p.max_attempts, 3);
        assert_eq!(p.base_delay, Duration::from_millis(100));
        assert_eq!(p.max_delay, Duration::from_millis(750));
    }

    #[test]
    fn backoff_doubles_then_caps_at_max() {
        let p = RetryPolicy::default();
        assert_eq!(p.backoff(1), Duration::from_millis(100));
        assert_eq!(p.backoff(2), Duration::from_millis(200));
        assert_eq!(p.backoff(3), Duration::from_millis(400));
        // weit jenseits → auf max_delay gedeckelt
        assert_eq!(p.backoff(10), Duration::from_millis(750));
    }

    #[test]
    fn backoff_does_not_overflow_on_huge_attempt() {
        let p = RetryPolicy::default();
        assert_eq!(p.backoff(u32::MAX), Duration::from_millis(750));
    }

    #[test]
    fn pool_closed_is_not_retryable() {
        // Kein DB-Error → trägt keinen SQLSTATE → nicht retrybar.
        assert!(!is_retryable(&sqlx::Error::PoolClosed));
    }

    #[tokio::test]
    async fn ok_on_first_try_runs_once() {
        let calls = Cell::new(0u32);
        let out = with_write_retry(RetryPolicy::default(), || async {
            calls.set(calls.get() + 1);
            Ok::<_, sqlx::Error>(42)
        })
        .await;
        assert_eq!(out.unwrap(), 42);
        assert_eq!(calls.get(), 1);
    }

    #[tokio::test]
    async fn non_retryable_error_is_not_retried() {
        let calls = Cell::new(0u32);
        let out: Result<(), _> = with_write_retry(RetryPolicy::default(), || async {
            calls.set(calls.get() + 1);
            Err(sqlx::Error::PoolClosed)
        })
        .await;
        assert!(out.is_err());
        assert_eq!(calls.get(), 1, "nicht-retrybar darf nur einmal laufen");
    }

    #[tokio::test]
    async fn single_attempt_policy_runs_exactly_once() {
        // Ohne echten Postgres lässt sich kein `40001`-SQLSTATE bauen — der
        // retrybare Pfad ist über die DB-Tests abgedeckt. Hier prüfen wir die
        // Versuchsobergrenze: max_attempts=1 läuft genau einmal, dann Fehler raus.
        let policy = RetryPolicy {
            max_attempts: 1,
            ..RetryPolicy::default()
        };
        let calls = Cell::new(0u32);
        let out: Result<(), _> = with_write_retry(policy, || async {
            calls.set(calls.get() + 1);
            Err(sqlx::Error::PoolClosed)
        })
        .await;
        assert!(out.is_err());
        assert_eq!(calls.get(), 1);
    }
}
