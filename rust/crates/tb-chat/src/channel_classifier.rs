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
use tracing::debug;

// ---------------------------------------------------------------------------
// Typen
// ---------------------------------------------------------------------------

/// Klassifizierung eines Twitch-Channels für die Chat-Pipeline.
#[derive(Debug, Clone, PartialEq)]
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
    /// In-Memory-Cache: broadcaster_login (lowercase) → CacheEntry.
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
    pub async fn classify(&self, broadcaster_login: &str, _broadcaster_id: &str) -> ChannelClass {
        let login = broadcaster_login.to_lowercase();
        let now_secs = Utc::now().timestamp();

        // Cache-Treffer prüfen
        if let Some(entry) = self.cache.get(&login) {
            if now_secs - entry.inserted_at_secs < CACHE_TTL_SECS {
                debug!(channel = %login, "channel_classifier: cache hit");
                return entry.class.clone();
            }
        }

        let class = self.classify_from_db(&login).await;
        self.cache.insert(
            login.clone(),
            CacheEntry {
                class: class.clone(),
                inserted_at_secs: now_secs,
            },
        );
        class
    }

    async fn classify_from_db(&self, login: &str) -> ChannelClass {
        // --- is_monitored_only: Streamer ohne Partner-Eintrag ---
        let is_monitored_only = sqlx::query_scalar::<_, bool>(
            "SELECT NOT EXISTS ( \
                 SELECT 1 FROM twitch_partners p \
                 WHERE p.twitch_user_id = s.twitch_user_id \
                    OR LOWER(p.twitch_login) = LOWER(s.twitch_login) \
             ) \
             FROM twitch_streamers s \
             WHERE LOWER(s.twitch_login) = $1",
        )
        .bind(login)
        .fetch_optional(&self.pool)
        .await
        .unwrap_or(None)
        .unwrap_or(false);

        // --- is_partner_channel_for_chat_tracking (partner_utils.py Z. 153–181)
        //     bot.py Z. 746–753: monitored-only → True für Tracking-Gate, aber KEIN Partner
        //     is_partner = aktiver Partner UND NICHT monitored-only (bot.py Z. 1568–1572) ---
        let is_partner_active = sqlx::query_scalar::<_, i32>(
            "SELECT COALESCE(is_partner_active, 0) \
             FROM twitch_streamers_partner_state \
             WHERE LOWER(twitch_login) = $1",
        )
        .bind(login)
        .fetch_optional(&self.pool)
        .await
        .unwrap_or(None)
        .unwrap_or(0)
            != 0;

        // Partner = is_partner_active UND nicht monitored-only (bot.py Z. 1568–1572)
        let is_partner = is_partner_active && !is_monitored_only;

        // --- is_deadlock_live (bot.py Z. 755–761, moderation.py Z. 2008–2080)
        //     is_live = integer, last_game = text (Prod-Schema) ---
        let is_deadlock_live = match sqlx::query_as::<_, (i32, Option<String>)>(
            "SELECT is_live, last_game FROM twitch_live_state WHERE streamer_login = $1",
        )
        .bind(login)
        .fetch_optional(&self.pool)
        .await
        {
            Ok(Some((is_live, Some(game)))) => {
                is_live != 0 && game.trim().to_lowercase() == "deadlock"
            }
            _ => false,
        };

        debug!(
            channel = %login,
            is_partner,
            is_monitored_only,
            is_deadlock_live,
            "channel_classifier: DB-Lookup"
        );

        ChannelClass {
            is_partner,
            is_monitored_only,
            is_deadlock_live,
        }
    }

    /// Cache für einen Channel invalidieren (z.B. nach Konfigurationsänderung).
    pub fn invalidate(&self, broadcaster_login: &str) {
        self.cache.remove(&broadcaster_login.to_lowercase());
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
