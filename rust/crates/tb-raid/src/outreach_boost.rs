//! Outreach-Boost-Ziele (Phase 6g): frisch kontaktierte Outreach-Empfänger
//! bekommen höchstens EINEN bevorzugten Raid. Port von
//! `raid/services/outreach_boost_targets.py`.
//!
//! Prod-Schema `twitch_partner_outreach`: alle Spalten **TEXT** (auch
//! `raid_used_at` und `contacted_at` — daher die Casts).

use std::collections::HashSet;

use sqlx::PgPool;

/// Frische-Fenster für Boost-Ziele (Python `lookback_hours = 48`).
pub const OUTREACH_BOOST_LOOKBACK_HOURS: i32 = 48;

#[derive(Clone)]
pub struct OutreachBoostStore {
    pool: PgPool,
}

impl OutreachBoostStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Logins (lowercase) aller frischen, noch nicht boost-geraidten
    /// Outreach-Empfänger (Python `load_outreach_boost_logins`).
    pub async fn load_boost_logins(
        &self,
        lookback_hours: i32,
    ) -> Result<HashSet<String>, sqlx::Error> {
        if lookback_hours <= 0 {
            return Ok(HashSet::new());
        }
        let rows: Vec<(Option<String>,)> = sqlx::query_as(
            r#"
            SELECT streamer_login
            FROM twitch_partner_outreach
            WHERE status = 'sent'
              AND raid_used_at IS NULL
              AND contacted_at IS NOT NULL
              AND contacted_at::timestamptz >= NOW() - (($1 || ' hours')::interval)
            "#,
        )
        .bind(lookback_hours.to_string())
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .filter_map(|(login,)| login)
            .map(|l| l.trim().to_lowercase())
            .filter(|l| !l.is_empty())
            .collect())
    }

    /// Markiert den Boost als verbraucht — CAS auf `raid_used_at IS NULL`,
    /// damit jeder Empfänger höchstens einen Boost-Raid bekommt (Python
    /// `mark_outreach_boost_used`). `true` = wirklich markiert.
    pub async fn mark_used(&self, login: &str) -> Result<bool, sqlx::Error> {
        let normalized = login.trim().to_lowercase();
        if normalized.is_empty() {
            return Ok(false);
        }
        let result = sqlx::query(
            "UPDATE twitch_partner_outreach
                SET raid_used_at = NOW()::text
              WHERE streamer_login = $1 AND raid_used_at IS NULL",
        )
        .bind(&normalized)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }
}
