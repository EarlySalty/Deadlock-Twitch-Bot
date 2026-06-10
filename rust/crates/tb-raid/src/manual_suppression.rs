//! Auto-Raid-Unterdrückung nach manuellem/externem Raid. Port von
//! `raid/services/manual_raid_suppression.py` (`mark_manual_raid_started` +
//! `is_offline_auto_raid_suppressed`).
//!
//! In-Memory-TTL-Map `broadcaster_id → bis-Unix-Zeitpunkt`. Python hängte den
//! Zustand per `getattr`-Magie an ein Owner-Objekt — hier ist er ein expliziter
//! Store, geteilt via `Arc<Mutex<…>>` in der Composition-Root.

use std::collections::HashMap;

use crate::util::unix_now;

/// Minimale TTL in Sekunden (Python: `max(30.0, ttl_seconds)`).
const MIN_TTL_SECONDS: f64 = 30.0;

#[derive(Debug, Default)]
pub struct ManualRaidSuppression {
    /// `broadcaster_id → Unix-Zeitpunkt, bis zu dem Auto-Raids unterdrückt sind`.
    until_by_id: HashMap<String, f64>,
}

impl ManualRaidSuppression {
    pub fn new() -> Self {
        Self::default()
    }

    /// Merkt einen manuellen Raid vor: Auto-Raids für diesen Broadcaster sind
    /// `ttl_seconds` (min. 30 s) unterdrückt. Leere ID → no-op.
    /// `now`: Unix-Sekunden; `None` = Systemzeit.
    pub fn mark(&mut self, broadcaster_id: &str, ttl_seconds: f64, now: Option<f64>) {
        let key = broadcaster_id.trim();
        if key.is_empty() {
            return;
        }
        let now = now.unwrap_or_else(unix_now);
        let ttl = ttl_seconds.max(MIN_TTL_SECONDS);
        self.until_by_id.insert(key.to_string(), now + ttl);
    }

    /// Ist der Auto-Raid für diesen Broadcaster gerade unterdrückt?
    /// Abgelaufene Einträge werden dabei entfernt (Python-Verhalten).
    pub fn is_suppressed(&mut self, broadcaster_id: &str, now: Option<f64>) -> bool {
        let key = broadcaster_id.trim();
        if key.is_empty() {
            return false;
        }
        let now = now.unwrap_or_else(unix_now);
        match self.until_by_id.get(key) {
            Some(&until) if now <= until => true,
            Some(_) => {
                self.until_by_id.remove(key);
                false
            }
            None => false,
        }
    }

    /// Entfernt alle abgelaufenen Einträge (Python `cleanup_expired_manual_raid_suppressions`).
    pub fn cleanup_expired(&mut self, now: Option<f64>) {
        let now = now.unwrap_or_else(unix_now);
        self.until_by_id.retain(|_, &mut until| now <= until);
    }

    pub fn len(&self) -> usize {
        self.until_by_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.until_by_id.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mark_unterdrueckt_innerhalb_der_ttl() {
        let mut s = ManualRaidSuppression::new();
        s.mark("42", 180.0, Some(1000.0));
        assert!(s.is_suppressed("42", Some(1100.0)));
        assert!(s.is_suppressed("42", Some(1180.0)), "Grenze inklusive");
        assert!(!s.is_suppressed("99", Some(1100.0)), "fremde ID frei");
    }

    #[test]
    fn abgelaufener_eintrag_wird_entfernt() {
        let mut s = ManualRaidSuppression::new();
        s.mark("42", 60.0, Some(1000.0));
        assert!(!s.is_suppressed("42", Some(1061.0)));
        assert!(s.is_empty(), "abgelaufener Eintrag aufgeräumt");
    }

    #[test]
    fn ttl_wird_auf_minimum_geklemmt() {
        let mut s = ManualRaidSuppression::new();
        s.mark("42", 1.0, Some(1000.0));
        assert!(s.is_suppressed("42", Some(1029.0)), "min. 30s TTL");
    }

    #[test]
    fn leere_id_ist_noop() {
        let mut s = ManualRaidSuppression::new();
        s.mark("  ", 180.0, Some(1000.0));
        assert!(s.is_empty());
        assert!(!s.is_suppressed("", Some(1000.0)));
    }

    #[test]
    fn cleanup_entfernt_nur_abgelaufene() {
        let mut s = ManualRaidSuppression::new();
        s.mark("alt", 60.0, Some(1000.0));
        s.mark("frisch", 600.0, Some(1000.0));
        s.cleanup_expired(Some(1100.0));
        assert_eq!(s.len(), 1);
        assert!(s.is_suppressed("frisch", Some(1100.0)));
    }
}
