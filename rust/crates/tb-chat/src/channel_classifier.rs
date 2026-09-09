//! Kanal-Klassifizierung — Port von bot.py L1559–1572 + Helfer-Methoden.
//!
//! Drei Flags pro Channel:
//! - `is_partner`: echter Partner (not monitored-only) — bot.py Z. 1568–1572
//! - `is_monitored_only`: nur Datensammlung, keine Bot-Funktionen — bot.py Z. 1567
//! - `is_deadlock_live`: Channel streamt gerade Deadlock — bot.py Z. 1560
//!
//! Quellen:
//! - `is_partner_channel_for_chat_tracking` → `twitch_streamers_partner_state.is_partner_active`
//!   (partner_utils.py Z. 153–181; in bot.py L746 überschrieben: monitored-only → True)
//! - `_is_monitored_only` → `twitch_streamers` ohne `twitch_partners`-Eintrag
//! - `_is_deadlock_live` → `_is_target_game_live_for_chat` via `twitch_live_state`
//!   (bot.py Z. 755–761, moderation.py Z. 2008–2080)
//!
//! Cache: Live-State wird 60 Sekunden gecacht. Python cached `_chat_category_cache`
//! mit 15s TTL (moderation.py Z. 2016), Partner/Monitored-Only haben keinen eigenen
//! Cache im Python-Code (werden per Call gelesen). Rust cached alle drei Felder zusammen
//! für 60s, was dem Bot-Join-Verhalten entspricht (Kanalliste ändert sich selten).

use chrono::Utc;
use dashmap::DashMap;
use sqlx::PgPool;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Typen
// ---------------------------------------------------------------------------

/// Klassifizierung eines Twitch-Channels für die Chat-Pipeline.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ChannelClass {
    /// Echter Partner (nicht monitored-only) — volle Bot-Funktionen (bot.py Z. 1568–1572).
    pub is_partner: bool,
    /// Nur Datensammlung, keine Bot-Aktionen (bot.py Z. 1567, Z. 1574–1585).
    pub is_monitored_only: bool,
    /// Channel streamt gerade Deadlock (bot.py Z. 1560, moderation.py Z. 2008–2080).
    pub is_deadlock_live: bool,
}

/// Cache-Eintrag mit monotoner Zeitstempel-Sekunde.
#[derive(Clone)]
struct CacheEntry {
    class: ChannelClass,
    /// Unix-Timestamp der Eintragserstellung (Sekunden, für TTL-Vergleich).
    inserted_at_secs: i64,
}

// ---------------------------------------------------------------------------
// ChannelClassifier
// ---------------------------------------------------------------------------

pub struct ChannelClassifier {
    pool: PgPool,
    /// In-Memory-Cache: broadcaster_id → CacheEntry.
    /// TTL: 60 Sekunden.
    cache: Arc<DashMap<String, CacheEntry>>,
}

/// Cache-TTL in Sekunden. Python cached Live-State 15s (moderation.py Z. 2016);
/// Partner/Monitored-Only haben keinen expliziten Cache → wir nehmen 60s als
/// pragmatischen Kompromiss für die ganzheitliche Klassifizierung.
const CACHE_TTL_SECS: i64 = 60;

impl ChannelClassifier {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            cache: Arc::new(DashMap::new()),
        }
    }

    /// Klassifiziert einen Channel. Ergebnis wird 60s gecacht.
    pub async fn classify(&self, _broadcaster_login: &str, broadcaster_id: &str) -> ChannelClass {
        if broadcaster_id.trim().is_empty() {
            return ChannelClass::default();
        }
        let now_secs = Utc::now().timestamp();
        self.cache
            .retain(|_, entry| now_secs - entry.inserted_at_secs < CACHE_TTL_SECS);
        if let Some(entry) = self.cache.get(broadcaster_id) {
            return entry.class.clone();
        }
        let class = match self.classify_from_db(broadcaster_id).await {
            Ok(class) => class,
            Err(error) => {
                tracing::warn!(%error, broadcaster_id, "Kanalstatus nicht abrufbar");
                return ChannelClass::default();
            }
        };
        self.cache.insert(
            broadcaster_id.into(),
            CacheEntry {
                class: class.clone(),
                inserted_at_secs: now_secs,
            },
        );
        class
    }

    async fn classify_from_db(&self, broadcaster_id: &str) -> Result<ChannelClass, sqlx::Error> {
        let (active, monitored, live): (bool, bool, bool) = sqlx::query_as(
            "SELECT EXISTS (SELECT 1 FROM twitch_streamers_partner_state WHERE twitch_user_id=$1 AND is_partner_active=1),
                    EXISTS (SELECT 1 FROM twitch_streamers s WHERE s.twitch_user_id=$1
                            AND NOT EXISTS (SELECT 1 FROM twitch_partners p WHERE p.twitch_user_id=s.twitch_user_id)),
                    EXISTS (SELECT 1 FROM twitch_live_state WHERE twitch_user_id=$1 AND is_live=1 AND LOWER(TRIM(last_game))='deadlock')"
        ).bind(broadcaster_id).fetch_one(&self.pool).await?;
        Ok(ChannelClass {
            is_partner: active && !monitored,
            is_monitored_only: monitored,
            is_deadlock_live: live,
        })
    }

    /// Cache für einen Channel invalidieren (z.B. nach Konfigurationsänderung).
    pub fn invalidate(&self, broadcaster_id: &str) {
        self.cache.remove(broadcaster_id);
    }
}

// ---------------------------------------------------------------------------
// Unit-Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_class_felder() {
        let c = ChannelClass {
            is_partner: true,
            is_monitored_only: false,
            is_deadlock_live: true,
        };
        assert!(c.is_partner);
        assert!(!c.is_monitored_only);
        assert!(c.is_deadlock_live);
    }

    #[test]
    fn monitored_only_ist_kein_partner() {
        // Invariante: monitored-only Channels sind NIE is_partner (bot.py Z. 1568–1572)
        let is_monitored_only = true;
        let is_partner_active = true; // technisch wäre es aktiv
        let is_partner = is_partner_active && !is_monitored_only;
        assert!(!is_partner);
    }

    #[test]
    fn cache_ttl_logik() {
        let now = Utc::now().timestamp();
        let entry = CacheEntry {
            class: ChannelClass {
                is_partner: false,
                is_monitored_only: false,
                is_deadlock_live: false,
            },
            inserted_at_secs: now - 30,
        };
        // 30s alt → noch gültig
        assert!(now - entry.inserted_at_secs < CACHE_TTL_SECS);

        let stale = CacheEntry {
            inserted_at_secs: now - 61,
            ..entry
        };
        // 61s alt → abgelaufen
        assert!(now - stale.inserted_at_secs >= CACHE_TTL_SECS);
    }
}
