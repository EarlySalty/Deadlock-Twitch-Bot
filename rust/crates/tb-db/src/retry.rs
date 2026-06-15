//! Write-Transaktionen mit beschränktem Retry bei Serialisierungs-/Deadlock-Fehlern.
//!
//! Tiefes Modul: nach außen genügt [`run_transaction`] (plus die Komfort-Wrapper
//! [`repeatable_read_transaction`] / [`serializable_transaction`]). Intern kapselt
//! es Isolation-Level-Wahl, SQLSTATE-Klassifikation und exponentielles Backoff.
//!
//! Parität zum Python-Orakel (`bot/storage/pg.py`): retrybar sind die SQLSTATEs
//! `40001` (serialization_failure) und `40P01` (deadlock_detected); Default sind
//! 3 Versuche mit Backoff `0.10s..=0.75s` (verdoppelnd). Die Knöpfe spiegeln die
//! gleichen Env-Variablen wie Python, damit der An/Aus-Zustand identisch bleibt.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use sqlx::postgres::PgPool;
use sqlx::{Postgres, Transaction};

use crate::error::DbError;

/// Eine laufende Schreib-Transaktion, wie sie an die Operation übergeben wird.
pub type Tx = Transaction<'static, Postgres>;

/// Rückgabe der Transaktions-Operation: ein an die `&mut Tx`-Leihe gebundenes
/// Future. Boxed, weil ein Closure, das ein sein Argument borgendes Future
/// liefert, sich in Rust nicht mit einem freien Generic-Future ausdrücken lässt
/// (HRTB über die Leih-Lifetime). Entspricht dem Muster von `sqlx`-Helfern.
pub type TxFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, DbError>> + Send + 'a>>;

/// SQLSTATE-Codes, bei denen Postgres die Transaktion serverseitig abbricht und
/// ein erneuter Versuch erfolgversprechend ist.
const SERIALIZATION_FAILURE: &str = "40001";
const DEADLOCK_DETECTED: &str = "40P01";

/// Isolationsstufe der Transaktion. Mappt 1:1 auf die Python-Helfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IsolationLevel {
    /// Postgres-Default; entspricht `run_transaction` ohne Override.
    #[default]
    ReadCommitted,
    RepeatableRead,
    Serializable,
}

impl IsolationLevel {
    /// `BEGIN`-Statement, das die Stufe direkt beim Transaktionsstart setzt
    /// (entspricht Pythons `SET TRANSACTION ISOLATION LEVEL` vor dem ersten Befehl).
    fn begin_statement(self) -> &'static str {
        match self {
            Self::ReadCommitted => "BEGIN ISOLATION LEVEL READ COMMITTED",
            Self::RepeatableRead => "BEGIN ISOLATION LEVEL REPEATABLE READ",
            Self::Serializable => "BEGIN ISOLATION LEVEL SERIALIZABLE",
        }
    }
}

/// Retry-Politik. Pure und testbar; liest selbst keine Env (siehe [`Self::from_env`]).
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
    /// ungültige Werte fallen auf [`RetryPolicy::default`] zurück (fail-open auf
    /// die bewährten Defaults statt fail-closed).
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
    /// (`attempt` ist 1-basiert). Verdoppelt ausgehend von `base_delay`, gedeckelt
    /// durch `max_delay` — identisch zu `_transaction_retry_sleep` in Python.
    fn backoff(&self, attempt: u32) -> Duration {
        let factor = 1u32.checked_shl(attempt.saturating_sub(1)).unwrap_or(u32::MAX);
        let delay = self.base_delay.saturating_mul(factor);
        delay.min(self.max_delay)
    }
}

/// `true`, wenn der Fehler ein retrybarer Serialisierungs-/Deadlock-Abbruch ist.
fn is_retryable(err: &DbError) -> bool {
    let DbError::Sqlx(sqlx_err) = err else {
        return false;
    };
    matches!(
        sqlx_err.as_database_error().and_then(|db| db.code()).as_deref(),
        Some(SERIALIZATION_FAILURE | DEADLOCK_DETECTED)
    )
}

/// Führt `operation` in einer Schreib-Transaktion aus und committet bei Erfolg.
///
/// Bei retrybaren Fehlern (`40001`/`40P01`) wird mit exponentiellem Backoff bis
/// zur Politik-Obergrenze erneut versucht; jeder Versuch öffnet eine frische
/// Transaktion in der gewünschten [`IsolationLevel`]. Nicht-retrybare Fehler und
/// der letzte erschöpfte Versuch werden unverändert weitergereicht. Ein Rollback
/// bei Fehler erfolgt implizit durch `Drop` der nicht-committeten Transaktion.
///
/// `operation` muss bei jedem Versuch erneut aufrufbar sein, daher `FnMut`.
pub async fn run_transaction<F, T>(
    pool: &PgPool,
    isolation: IsolationLevel,
    policy: RetryPolicy,
    mut operation: F,
) -> Result<T, DbError>
where
    F: for<'a> FnMut(&'a mut Tx) -> TxFuture<'a, T>,
{
    let max_attempts = policy.max_attempts.max(1);

    for attempt in 1..=max_attempts {
        let mut tx = pool.begin_with(isolation.begin_statement()).await?;
        match operation(&mut tx).await {
            Ok(value) => {
                tx.commit().await?;
                return Ok(value);
            }
            Err(err) => {
                // tx wird durch Drop zurückgerollt.
                drop(tx);
                if attempt >= max_attempts || !is_retryable(&err) {
                    return Err(err);
                }
                tokio::time::sleep(policy.backoff(attempt)).await;
            }
        }
    }

    // Unerreichbar: die Schleife kehrt in jeder Iteration zurück.
    unreachable!("retry loop must return within max_attempts")
}

/// `run_transaction` mit `REPEATABLE READ` und Default-Politik.
pub async fn repeatable_read_transaction<F, T>(pool: &PgPool, operation: F) -> Result<T, DbError>
where
    F: for<'a> FnMut(&'a mut Tx) -> TxFuture<'a, T>,
{
    run_transaction(
        pool,
        IsolationLevel::RepeatableRead,
        RetryPolicy::default(),
        operation,
    )
    .await
}

/// `run_transaction` mit `SERIALIZABLE` und Default-Politik.
pub async fn serializable_transaction<F, T>(pool: &PgPool, operation: F) -> Result<T, DbError>
where
    F: for<'a> FnMut(&'a mut Tx) -> TxFuture<'a, T>,
{
    run_transaction(
        pool,
        IsolationLevel::Serializable,
        RetryPolicy::default(),
        operation,
    )
    .await
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
        // attempt 1 → base, attempt 2 → 2*base, attempt 3 → 4*base (gedeckelt).
        assert_eq!(p.backoff(1), Duration::from_millis(100));
        assert_eq!(p.backoff(2), Duration::from_millis(200));
        // 4*100ms = 400ms < 750ms cap
        assert_eq!(p.backoff(3), Duration::from_millis(400));
        // weit jenseits → auf max_delay gedeckelt
        assert_eq!(p.backoff(10), Duration::from_millis(750));
    }

    #[test]
    fn backoff_does_not_overflow_on_huge_attempt() {
        let p = RetryPolicy::default();
        // saturating shift darf nicht paniken
        assert_eq!(p.backoff(u32::MAX), Duration::from_millis(750));
    }

    #[test]
    fn isolation_begin_statements() {
        assert_eq!(
            IsolationLevel::default().begin_statement(),
            "BEGIN ISOLATION LEVEL READ COMMITTED"
        );
        assert_eq!(
            IsolationLevel::RepeatableRead.begin_statement(),
            "BEGIN ISOLATION LEVEL REPEATABLE READ"
        );
        assert_eq!(
            IsolationLevel::Serializable.begin_statement(),
            "BEGIN ISOLATION LEVEL SERIALIZABLE"
        );
    }

    #[test]
    fn serialization_and_deadlock_codes_are_retryable() {
        // Stellt sicher, dass die Konstanten den Python-SQLSTATEs entsprechen.
        assert_eq!(SERIALIZATION_FAILURE, "40001");
        assert_eq!(DEADLOCK_DETECTED, "40P01");
    }

    #[test]
    fn non_database_errors_are_not_retryable() {
        // Pool-Closed o.ä. trägt keine SQLSTATE → nicht retrybar.
        let err = DbError::Sqlx(sqlx::Error::PoolClosed);
        assert!(!is_retryable(&err));
    }
}
