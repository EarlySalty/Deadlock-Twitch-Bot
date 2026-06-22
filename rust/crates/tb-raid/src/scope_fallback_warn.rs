//! Once-only-WARN für den Legacy-Broadcaster-Token-Fallback (P3.9).
//!
//! Port von `bot/raid/runtime_support.py:97-124`
//! (`warn_user_scope_fallback_once` / `clear_user_scope_fallback_warning`).
//!
//! Wenn der Follower-Total-Abruf den Bot-Token nicht nutzen kann (kein
//! `moderator:read:followers`) und auf den per-Streamer-/Legacy-Broadcaster-Token
//! ausweicht, soll der Operator **genau einmal** je `(area, subject)` ein
//! WARNING sehen — wiederholtes Loggen würde die Logs fluten. Nach Erholung
//! (Bot-Token greift wieder) wird der Schlüssel re-armiert, sodass ein erneuter
//! Rückfall wieder einmal warnt.
//!
//! Reines Operator-Signal: kein User-sichtbarer Text, keine persistierten
//! Effekte. Secrets werden nie geloggt (nur `area`/`subject`).

use std::collections::HashSet;
use std::sync::Mutex;

/// Dedup-Schlüssel: `(area, subject)`, beide normalisiert (getrimmt, lowercase).
type WarnKey = (String, String);

fn make_key(area: &str, subject: &str) -> WarnKey {
    let area = area.trim().to_lowercase();
    let subject = {
        let s = subject.trim().to_lowercase();
        if s.is_empty() {
            "<unknown>".to_string()
        } else {
            s
        }
    };
    (area, subject)
}

/// Hält die bereits gewarnten `(area, subject)`-Schlüssel. Thread-safe; eine
/// Instanz pro Bot-Laufzeit (entspricht Pythons `bot._user_scope_fallback_warned`).
#[derive(Default)]
pub struct ScopeFallbackWarner {
    warned: Mutex<HashSet<WarnKey>>,
}

impl ScopeFallbackWarner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Loggt **genau einmal** je `(area, subject)` ein WARNING über den
    /// Legacy-Broadcaster-Token-Fallback. Liefert `true`, wenn jetzt geloggt
    /// wurde, `false`, wenn der Schlüssel schon bekannt war (kein erneutes Log).
    pub fn warn_once(&self, area: &str, subject: &str) -> bool {
        let key = make_key(area, subject);
        {
            let mut guard = self.warned.lock().unwrap_or_else(|e| e.into_inner());
            if !guard.insert(key) {
                return false;
            }
        }
        let subject_display = if subject.trim().is_empty() {
            "<unknown>"
        } else {
            subject.trim()
        };
        tracing::warn!(
            area,
            subject = subject_display,
            "RaidBot: nutze Legacy-Broadcaster-Token fuer {} ({}). \
             Der Bot-Token sollte diesen Pfad uebernehmen.",
            area,
            subject_display,
        );
        true
    }

    /// Re-armiert den `(area, subject)`-Schlüssel: nach erfolgreicher Erholung
    /// (Bot-Token greift wieder) wird ein erneuter Rückfall wieder einmal warnen.
    /// Port von `clear_user_scope_fallback_warning`.
    pub fn clear(&self, area: &str, subject: &str) {
        let key = make_key(area, subject);
        let mut guard = self.warned.lock().unwrap_or_else(|e| e.into_inner());
        guard.remove(&key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warns_only_once_per_key() {
        let warner = ScopeFallbackWarner::new();
        assert!(warner.warn_once("followers", "channel_a"));
        // Zweiter Aufruf für denselben Schlüssel → kein erneutes Log.
        assert!(!warner.warn_once("followers", "channel_a"));
        // Anderer subject → eigenes Log.
        assert!(warner.warn_once("followers", "channel_b"));
    }

    #[test]
    fn key_normalization_dedups_case_and_whitespace() {
        let warner = ScopeFallbackWarner::new();
        assert!(warner.warn_once("Followers", "Channel_A"));
        assert!(!warner.warn_once("  followers ", " channel_a "));
    }

    #[test]
    fn clear_rearms_warning() {
        let warner = ScopeFallbackWarner::new();
        assert!(warner.warn_once("followers", "channel_a"));
        assert!(!warner.warn_once("followers", "channel_a"));
        // Nach clear: wieder einmal warnen.
        warner.clear("followers", "channel_a");
        assert!(warner.warn_once("followers", "channel_a"));
    }

    #[test]
    fn empty_subject_maps_to_unknown() {
        let warner = ScopeFallbackWarner::new();
        assert!(warner.warn_once("followers", ""));
        // "<unknown>" und leerer subject teilen sich den Schlüssel.
        assert!(!warner.warn_once("followers", "   "));
    }
}
