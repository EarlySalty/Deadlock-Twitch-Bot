//! Global-Chatter-Ban-Erkennung — Port von `_is_globally_banned_cached` +
//! `_enforce_global_chatter_ban` (moderation.py Z. 717–772).
//!
//! Dieses Modul trifft nur die ENTSCHEIDUNG (Cache + DB-Lookup). Die AKTION
//! (Delete + Ban + Notice + Records + Discord-Alert) läuft in der Pipeline
//! über [`crate::moderation::ModerationEngine::auto_ban_and_cleanup`] — exakt
//! wie Python, wo `_enforce_global_chatter_ban` an `_auto_ban_and_cleanup`
//! delegiert (Z. 764–772, mit eigenem reason_text/notice_text/alert_kind).
//!
//! Cache: Positive Treffer bleiben 300 Sekunden im Speicher (moderation.py
//! Z. 737: `< 300.0`). Negative Treffer werden NICHT gecacht — neue Bans
//! wirken sofort bei der nächsten Nachricht. Cache-Größe: max 500 Einträge.

use crate::types::ChatMessageEvent;
use dashmap::DashMap;
use sqlx::PgPool;
use std::time::Instant;
use tracing::warn;

// ---------------------------------------------------------------------------
// Konstanten
// ---------------------------------------------------------------------------

/// Cache-TTL für positive Ban-Treffer in Sekunden (moderation.py Z. 737: `< 300.0`).
const BAN_CACHE_TTL_SECS: u64 = 300;

/// Maximale Cache-Größe vor Eviction-Lauf (moderation.py Z. 746: `> 500`).
const BAN_CACHE_MAX_ENTRIES: usize = 500;

// ---------------------------------------------------------------------------
// GlobalChatterBanEnforcer
// ---------------------------------------------------------------------------

/// Entscheidet ob ein Chatter auf der globalen Bannliste steht.
pub struct GlobalChatterBanEnforcer {
    pool: PgPool,
    /// Positiver Cache: chatter_login (lowercase) → Instant des Eintragszeitpunkts.
    cache: DashMap<String, Instant>,
}

impl GlobalChatterBanEnforcer {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            cache: DashMap::new(),
        }
    }

    /// Prüft ob der Chatter global gebannt ist (Cache → DB).
    ///
    /// Fehler werden geloggt und geben `false` zurück (fail-safe: kein
    /// falscher Ban — Python Z. 751: `except: return False`).
    pub async fn is_banned(&self, event: &ChatMessageEvent) -> bool {
        let chatter_login = event.chatter_user_login.to_lowercase();
        if chatter_login.is_empty() {
            return false;
        }

        // Cache-Prüfung: positiver Treffer < 300s (moderation.py Z. 736–738)
        if let Some(entry) = self.cache.get(&chatter_login) {
            if entry.elapsed().as_secs() < BAN_CACHE_TTL_SECS {
                return true;
            }
            // Abgelaufen → entfernen und DB befragen
            drop(entry);
            self.cache.remove(&chatter_login);
        }

        // DB-Check (pg.py Z. 4119–4134: Login ODER ID)
        match self
            .is_globally_banned(&chatter_login, &event.chatter_user_id)
            .await
        {
            Ok(true) => {
                // Positiven Treffer cachen (moderation.py Z. 744–749)
                self.cache.insert(chatter_login, Instant::now());
                self.evict_if_needed();
                true
            }
            Ok(false) => false,
            Err(e) => {
                warn!("global_chatter_ban: DB-Fehler — {}", e);
                false
            }
        }
    }

    /// DB-Abfrage: Login ODER ID — identisch mit `is_chatter_globally_banned`
    /// (pg.py Z. 4119–4134).
    async fn is_globally_banned(
        &self,
        chatter_login: &str,
        chatter_id: &str,
    ) -> Result<bool, sqlx::Error> {
        let row = sqlx::query_scalar!(
            "SELECT 1 FROM twitch_chatter_global_ban \
             WHERE chatter_login = $1 \
                OR (chatter_id IS NOT NULL AND chatter_id = $2 AND chatter_id <> '') \
             LIMIT 1",
            chatter_login,
            chatter_id,
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.is_some())
    }

    /// Entfernt abgelaufene Cache-Einträge wenn der Cache zu groß wird
    /// (moderation.py Z. 746–749: `len(cache) > 500`).
    fn evict_if_needed(&self) {
        if self.cache.len() <= BAN_CACHE_MAX_ENTRIES {
            return;
        }
        let now = Instant::now();
        let stale_keys: Vec<String> = self
            .cache
            .iter()
            .filter(|e| now.duration_since(*e.value()).as_secs() >= BAN_CACHE_TTL_SECS)
            .map(|e| e.key().clone())
            .collect();
        for key in stale_keys {
            self.cache.remove(&key);
        }
    }
}

// ---------------------------------------------------------------------------
// Unit-Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ban_cache_ttl_konstante() {
        // moderation.py Z. 737: < 300.0
        assert_eq!(BAN_CACHE_TTL_SECS, 300);
    }

    #[test]
    fn ban_cache_max_entries_konstante() {
        // moderation.py Z. 746: > 500
        assert_eq!(BAN_CACHE_MAX_ENTRIES, 500);
    }

    #[test]
    fn global_ban_texte_kommen_aus_moderation() {
        // Texte leben kanonisch in moderation.rs (Z. 35/44) — hier nur der
        // Wortlaut-Check gegen moderation.py Z. 766–770.
        assert_eq!(
            crate::moderation::BAN_REASON_GLOBAL,
            "Netzwerkweiter Ban: Verstoß gegen Community-Richtlinien"
        );
        assert!(crate::moderation::NOTICE_GLOBAL_BAN
            .contains("{login} steht netzwerkweit auf der Bannliste"));
    }
}
