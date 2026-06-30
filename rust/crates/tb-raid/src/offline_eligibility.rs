//! Quell-Eligibility fürs Auto-Raid: darf für diesen Broadcaster beim
//! Offline-Gehen überhaupt ein Auto-Raid laufen? Port von
//! `storage/partner_registry.py` `load_offline_auto_raid_eligibility` +
//! der Skip-Kaskade in `offline_raid_orchestrator.handle_streamer_offline`.
//!
//! Python lud dafür die komplette Partner-Zeile (20 Spalten) und griff per
//! Index `row[13]` auf `raid_bot_enabled` zu — hier werden genau die zwei
//! benötigten Werte selektiert. `twitch_partners.raid_bot_enabled` ist
//! **INTEGER** (0/1), `twitch_raid_auth.raid_enabled` **BOOLEAN**.

use sqlx::PgPool;

/// Ergebnis der Eligibility-Prüfung (Python `OfflineAutoRaidEligibility`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfflineAutoRaidEligibility {
    pub active_partner: bool,
    pub auth_row_found: bool,
    pub raid_bot_enabled: bool,
    pub raid_auth_enabled: bool,
}

impl OfflineAutoRaidEligibility {
    pub fn can_auto_raid(&self) -> bool {
        self.active_partner && self.raid_bot_enabled && self.raid_auth_enabled
    }

    /// Erster greifender Skip-Grund in Python-Reihenfolge, `None` wenn erlaubt.
    pub fn skip_reason(&self) -> Option<&'static str> {
        if !self.active_partner && !self.auth_row_found {
            return Some("not_found");
        }
        if !self.active_partner {
            return Some("not_active_partner");
        }
        if !self.raid_bot_enabled {
            return Some("setting_disabled");
        }
        if !self.raid_auth_enabled {
            return Some("no_auth");
        }
        None
    }
}

#[derive(Clone)]
pub struct OfflineEligibilityStore {
    pool: PgPool,
}

impl OfflineEligibilityStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Lädt die Eligibility für einen Broadcaster. Leere ID → alles `false`.
    pub async fn load(
        &self,
        twitch_user_id: &str,
    ) -> Result<OfflineAutoRaidEligibility, sqlx::Error> {
        let user_id = twitch_user_id.trim();
        if user_id.is_empty() {
            return Ok(OfflineAutoRaidEligibility {
                active_partner: false,
                auth_row_found: false,
                raid_bot_enabled: false,
                raid_auth_enabled: false,
            });
        }

        // Neueste aktive Partner-Zeile (Python: status='active', ORDER BY id DESC).
        let partner_row: Option<Option<i32>> = sqlx::query_scalar!(
            r#"SELECT raid_bot_enabled AS "raid_bot_enabled?" FROM twitch_partners
              WHERE twitch_user_id = $1 AND status = 'active'
              ORDER BY id DESC LIMIT 1"#,
            user_id
        )
        .fetch_optional(&self.pool)
        .await?;

        let auth_row: Option<Option<bool>> = sqlx::query_scalar!(
            r#"SELECT raid_enabled AS "raid_enabled?" FROM twitch_raid_auth WHERE twitch_user_id = $1 LIMIT 1"#,
            user_id
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(OfflineAutoRaidEligibility {
            active_partner: partner_row.is_some(),
            auth_row_found: auth_row.is_some(),
            raid_bot_enabled: partner_row.flatten().unwrap_or(0) != 0,
            raid_auth_enabled: auth_row.flatten().unwrap_or(false),
        })
    }
}
