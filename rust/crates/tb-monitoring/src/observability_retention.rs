//! Retention-Cleanup fuer `twitch_observability_events`.
//!
//! Python hat fuer die EventSub-Capacity-Zeitreihe denselben Betriebsvertrag:
//! stuendlicher Cleanup, Default 45 Tage, Clamp 7..365
//! (`bot/monitoring/eventsub_mixin.py:_eventsub_capacity_retention_days`).
//! Observability-Events nutzen diese Grenzen, damit die Flow-Tabelle nicht
//! unbegrenzt waechst.

use chrono::{DateTime, Duration, Utc};
use sqlx::PgPool;

pub const OBSERVABILITY_RETENTION_DEFAULT_DAYS: i64 = 45;
const OBSERVABILITY_RETENTION_MIN_DAYS: i64 = 7;
const OBSERVABILITY_RETENTION_MAX_DAYS: i64 = 365;

pub fn observability_retention_days() -> i64 {
    parse_env_clamped(
        "TWITCH_OBSERVABILITY_RETENTION_DAYS",
        OBSERVABILITY_RETENTION_DEFAULT_DAYS,
        OBSERVABILITY_RETENTION_MIN_DAYS,
        OBSERVABILITY_RETENTION_MAX_DAYS,
    )
}

pub async fn cleanup_observability_events(pool: &PgPool) -> Result<u64, sqlx::Error> {
    let cutoff = Utc::now() - Duration::days(observability_retention_days());
    cleanup_observability_events_before(pool, cutoff).await
}

pub async fn cleanup_observability_events_before(
    pool: &PgPool,
    cutoff: DateTime<Utc>,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query("DELETE FROM twitch_observability_events WHERE created_at < $1")
        .bind(cutoff)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

fn parse_env_clamped(key: &str, default: i64, min: i64, max: i64) -> i64 {
    match std::env::var(key) {
        Ok(raw) if raw.trim().is_empty() => default,
        Ok(raw) => match raw.trim().parse::<i64>() {
            Ok(value) => {
                let clamped = value.clamp(min, max);
                if clamped != value {
                    tracing::warn!(
                        setting = key,
                        value,
                        minimum = min,
                        maximum = max,
                        "Optionaler Observability-Retention-Env-Wert ausserhalb des Bereichs; Clamp wird verwendet"
                    );
                }
                clamped
            }
            Err(_) => {
                tracing::warn!(
                    setting = key,
                    value = %raw,
                    default,
                    "Ungültiger optionaler Observability-Retention-Env-Wert; Default wird verwendet"
                );
                default
            }
        },
        Err(_) => default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retention_bounds_match_capacity_reference() {
        assert_eq!(OBSERVABILITY_RETENTION_DEFAULT_DAYS, 45);
        assert_eq!(OBSERVABILITY_RETENTION_MIN_DAYS, 7);
        assert_eq!(OBSERVABILITY_RETENTION_MAX_DAYS, 365);
    }
}
