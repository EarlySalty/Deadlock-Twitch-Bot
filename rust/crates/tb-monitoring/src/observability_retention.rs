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
#[cfg(test)]
const OBSERVABILITY_RETENTION_MIN_DAYS: i64 = 7;
#[cfg(test)]
const OBSERVABILITY_RETENTION_MAX_DAYS: i64 = 365;

pub async fn cleanup_observability_events(pool: &PgPool, retention_days: i64) -> Result<u64, sqlx::Error> {
    let cutoff = Utc::now() - Duration::days(retention_days);
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
