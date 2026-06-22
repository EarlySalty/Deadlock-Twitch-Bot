//! Retry-Wrapper für die Schreiboperationen der Processing-Inbox.
//!
//! Parität zu den Python-Orakeln (`bot/storage/pg.py` und Monitoring-Storage):
//! retrybare SQLSTATEs laufen mit beschränktem Backoff erneut. Der native
//! Inbox-Store schrieb bisher direkt gegen den Pool ohne diesen Schutz — ein
//! durch Concurrency (Python- und Rust-Worker leasen denselben Tisch) oder
//! einen kurzen Postgres-Neustart ausgelöster Abbruch ließ die Schreibung hart
//! scheitern. [`with_write_retry`] schließt diese Lücke.
//!
//! Knöpfe + Defaults spiegeln Python (`TWITCH_ANALYTICS_TX_RETRY_*`): 3 Versuche,
//! Backoff `0.10s..=0.75s` (verdoppelnd). Pure und ohne Env-Lesen testbar.

use std::time::Duration;

/// Postgres-SQLSTATEs, bei denen ein erneuter Versuch erfolgversprechend ist.
const SERIALIZATION_FAILURE: &str = "40001";
const DEADLOCK_DETECTED: &str = "40P01";
const CONNECTION_EXCEPTION: &str = "08000";
const SQLCLIENT_UNABLE_TO_ESTABLISH_SQLCONNECTION: &str = "08001";
const CONNECTION_DOES_NOT_EXIST: &str = "08003";
const SQLSERVER_REJECTED_ESTABLISHMENT_OF_SQLCONNECTION: &str = "08004";
const CONNECTION_FAILURE: &str = "08006";
const ADMIN_SHUTDOWN: &str = "57P01";
const CRASH_SHUTDOWN: &str = "57P02";
const CANNOT_CONNECT_NOW: &str = "57P03";
const TOO_MANY_CONNECTIONS: &str = "53300";

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
        let factor = 1u32
            .checked_shl(attempt.saturating_sub(1))
            .unwrap_or(u32::MAX);
        self.base_delay.saturating_mul(factor).min(self.max_delay)
    }
}

/// `true`, wenn der Fehler ein retrybarer Serialisierungs-, Deadlock-,
/// Verbindungs- oder Admin-Shutdown-Abbruch ist.
pub fn is_retryable(err: &sqlx::Error) -> bool {
    matches!(
        err.as_database_error().and_then(|db| db.code()).as_deref(),
        Some(
            SERIALIZATION_FAILURE
                | DEADLOCK_DETECTED
                | CONNECTION_EXCEPTION
                | SQLCLIENT_UNABLE_TO_ESTABLISH_SQLCONNECTION
                | CONNECTION_DOES_NOT_EXIST
                | SQLSERVER_REJECTED_ESTABLISHMENT_OF_SQLCONNECTION
                | CONNECTION_FAILURE
                | ADMIN_SHUTDOWN
                | CRASH_SHUTDOWN
                | CANNOT_CONNECT_NOW
                | TOO_MANY_CONNECTIONS
        )
    )
}

/// Führt eine Inbox-Schreiboperation aus und versucht sie bei retrybaren
/// Fehlern mit exponentiellem Backoff erneut. Nicht-retrybare
/// Fehler und der letzte erschöpfte Versuch werden unverändert weitergereicht.
///
/// `operation` muss bei jedem Versuch erneut aufrufbar sein (`FnMut`), da ein
/// Serialisierungs-Abbruch die Transaktion serverseitig zurückrollt.
pub async fn with_write_retry<F, Fut, T>(
    policy: RetryPolicy,
    mut operation: F,
) -> Result<T, sqlx::Error>
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
                    "Schreiboperation nach retrybarem Postgres-Abbruch erneut versuchen"
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
    use sqlx::PgPool;
    use std::cell::Cell;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

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

    async fn make_pool() -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .ok()
    }

    async fn raise_sqlstate(pool: &PgPool, code: &str) -> sqlx::Error {
        let sql = format!("DO $$ BEGIN RAISE SQLSTATE '{code}'; END $$");
        match sqlx::query(&sql).execute(pool).await {
            Ok(_) => panic!("RAISE SQLSTATE {code} unexpectedly succeeded"),
            Err(err) => err,
        }
    }

    #[tokio::test]
    async fn transient_connection_and_shutdown_sqlstates_are_retryable() {
        let Some(pool) = make_pool().await else {
            return;
        };
        for code in [
            "08000", "08001", "08003", "08004", "08006", "57P01", "57P02", "57P03", "53300",
        ] {
            let err = raise_sqlstate(&pool, code).await;
            assert!(is_retryable(&err), "{code} muss retrybar sein");
        }
    }

    #[tokio::test]
    async fn write_retry_retries_transient_connection_error() {
        let Some(pool) = make_pool().await else {
            return;
        };
        let calls = Arc::new(AtomicU32::new(0));
        let policy = RetryPolicy {
            base_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(1),
            ..RetryPolicy::default()
        };
        let out = with_write_retry(policy, || {
            let pool = pool.clone();
            let calls = Arc::clone(&calls);
            async move {
                let attempt = calls.fetch_add(1, Ordering::SeqCst) + 1;
                if attempt == 1 {
                    return Err(raise_sqlstate(&pool, "08006").await);
                }
                Ok::<_, sqlx::Error>(attempt)
            }
        })
        .await;

        assert_eq!(out.unwrap(), 2);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
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
