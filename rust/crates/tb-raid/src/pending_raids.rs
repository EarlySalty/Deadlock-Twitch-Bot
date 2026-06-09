//! In-Memory-Store für ausstehende Raids (`PendingRaid`) + reine Key-Helfer.
//!
//! Port von `bot/raid/pending_raids.py` — Datenschicht, kein Algorithmus.
//!
//! Schema-Vertrag:
//!
//! | Python-Feld                 | Rust-Feld                   | Typ                     |
//! |-----------------------------|-----------------------------|-------------------------|
//! | from_broadcaster_login      | from_broadcaster_login      | String                  |
//! | to_broadcaster_id           | to_broadcaster_id           | String                  |
//! | registered_ts               | registered_ts               | f64 (Unix-Sekunden)     |
//! | is_partner_raid             | is_partner_raid             | bool                    |
//! | registered_viewer_count     | registered_viewer_count     | i32                     |
//! | offline_trigger_ts          | offline_trigger_ts          | Option<f64>             |
//! | raid_flow_id                | raid_flow_id                | Option<String>          |
//! | channel_raid_ready          | channel_raid_ready          | Option<bool>            |
//! | channel_raid_ready_detail   | channel_raid_ready_detail   | Option<String>          |
//! | chat_notification_state     | chat_notification_state     | Option<String>          |
//! | chat_notification_detail    | chat_notification_detail    | Option<String>          |
//! | signal_observations         | signal_observations         | HashMap<String, HashMap<String, String>> |

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------------------
// Reine Key-Helfer (Port von pending_raids.py Z. 12–24)
// ---------------------------------------------------------------------------

/// Normalisiert einen Broadcaster-Login-Namen: trim + lowercase.
///
/// Port von `normalize_broadcaster_login` (pending_raids.py Z. 12–13).
pub fn normalize_broadcaster_login(raw: &str) -> String {
    raw.trim().to_lowercase()
}

/// Normalisiert den zusammengesetzten Pending-Raid-Key:
/// `(to_broadcaster_id.trim(), normalize_broadcaster_login(from_broadcaster_login))`.
///
/// Port von `normalize_pending_raid_key` (pending_raids.py Z. 16–24).
pub fn normalize_pending_raid_key(
    to_broadcaster_id: &str,
    from_broadcaster_login: &str,
) -> (String, String) {
    (
        to_broadcaster_id.trim().to_string(),
        normalize_broadcaster_login(from_broadcaster_login),
    )
}

// ---------------------------------------------------------------------------
// PendingRaid-Datenstruktur
// ---------------------------------------------------------------------------

/// Ein ausstehender Raid, der noch nicht als Arrival bestätigt wurde.
///
/// Port von `PendingRaid` (pending_raids.py Z. 98–112).
/// `signal_observations` ist eine Map `signal_type → {field → value}`.
#[derive(Debug, Clone)]
pub struct PendingRaid {
    /// Login-Name des Quell-Streamers (normalisiert).
    pub from_broadcaster_login: String,
    /// Broadcaster-ID des Ziel-Kanals (getrimmt).
    pub to_broadcaster_id: String,
    /// Unix-Timestamp (Sekunden), wann der Raid registriert wurde.
    pub registered_ts: f64,
    /// Ob der Quell-Kanal als Partner-Kanal bekannt ist.
    pub is_partner_raid: bool,
    /// Viewer-Anzahl zum Registrierzeitpunkt.
    pub registered_viewer_count: i32,
    /// Unix-Timestamp, wann der Offline-Trigger ausgelöst wurde (falls vorhanden).
    pub offline_trigger_ts: Option<f64>,
    /// Optionale Flow-ID zur internen Nachverfolgung.
    pub raid_flow_id: Option<String>,
    /// Ob der Ziel-Kanal raid-bereit ist (None = unbekannt).
    pub channel_raid_ready: Option<bool>,
    /// Optionales Detail zum raid_ready-Status.
    pub channel_raid_ready_detail: Option<String>,
    /// Status der Chat-Benachrichtigung (None = noch nicht gesendet).
    pub chat_notification_state: Option<String>,
    /// Optionales Detail zur Chat-Benachrichtigung.
    pub chat_notification_detail: Option<String>,
    /// Beobachtete Signale pro Signaltyp: `signal_type → {field → value}`.
    pub signal_observations: HashMap<String, HashMap<String, String>>,
}

impl PendingRaid {
    /// Erstellt einen neuen `PendingRaid` mit Pflichtfeldern.
    /// `registered_ts` wird auf die aktuelle Systemzeit gesetzt.
    pub fn new(
        from_broadcaster_login: impl Into<String>,
        to_broadcaster_id: impl Into<String>,
    ) -> Self {
        Self {
            from_broadcaster_login: normalize_broadcaster_login(&from_broadcaster_login.into()),
            to_broadcaster_id: to_broadcaster_id.into().trim().to_string(),
            registered_ts: unix_now(),
            is_partner_raid: false,
            registered_viewer_count: 0,
            offline_trigger_ts: None,
            raid_flow_id: None,
            channel_raid_ready: None,
            channel_raid_ready_detail: None,
            chat_notification_state: None,
            chat_notification_detail: None,
            signal_observations: HashMap::new(),
        }
    }

    /// Gibt den normalisierten Key zurück (analog zu `PendingRaid.key` in Python).
    pub fn key(&self) -> (String, String) {
        normalize_pending_raid_key(&self.to_broadcaster_id, &self.from_broadcaster_login)
    }

    /// Trägt eine Signal-Beobachtung ein.
    ///
    /// Port von `PendingRaid.record_signal_observation` (pending_raids.py Z. 146).
    pub fn record_signal_observation(
        &mut self,
        signal_type: impl Into<String>,
        status: impl Into<String>,
        reason: Option<String>,
        detail: Option<String>,
    ) {
        let mut obs = HashMap::new();
        obs.insert("status".to_string(), status.into());
        if let Some(r) = reason {
            if !r.is_empty() {
                obs.insert("reason".to_string(), r);
            }
        }
        if let Some(d) = detail {
            if !d.is_empty() {
                obs.insert("detail".to_string(), d);
            }
        }
        self.signal_observations.insert(signal_type.into(), obs);
    }
}

// ---------------------------------------------------------------------------
// PendingRaidStore — In-Memory-Store mit TTL-Ablauf
// ---------------------------------------------------------------------------

/// In-Memory-Store für ausstehende Raids.
///
/// Port von `PendingRaidStore` (pending_raids.py Z. 282–460).
/// Key: `(to_broadcaster_id, from_broadcaster_login)` — beide normalisiert.
#[derive(Debug, Default)]
pub struct PendingRaidStore {
    raids: HashMap<(String, String), PendingRaid>,
}

impl PendingRaidStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Speichert oder überschreibt einen Pending-Raid.
    ///
    /// Port von `PendingRaidStore.store` (pending_raids.py Z. 311).
    pub fn store(&mut self, raid: PendingRaid) {
        let key = raid.key();
        self.raids.insert(key, raid);
    }

    /// Gibt eine Referenz auf den Raid zurück, falls vorhanden.
    ///
    /// Port von `PendingRaidStore.get` (pending_raids.py Z. 330).
    /// Suche: Exakter Key wenn `from_broadcaster_login` angegeben, sonst
    /// Einzel-Treffer nur über `to_broadcaster_id`.
    pub fn get(
        &self,
        to_broadcaster_id: &str,
        from_broadcaster_login: Option<&str>,
    ) -> Option<&PendingRaid> {
        let target_id = to_broadcaster_id.trim().to_string();
        if let Some(from) = from_broadcaster_login {
            let key = normalize_pending_raid_key(&target_id, from);
            return self.raids.get(&key);
        }
        // Fallback: eindeutiger Treffer über target_id allein
        let matches: Vec<&PendingRaid> = self
            .raids
            .values()
            .filter(|r| r.to_broadcaster_id == target_id)
            .collect();
        if matches.len() == 1 {
            Some(matches[0])
        } else {
            None
        }
    }

    /// Entfernt einen Raid und gibt ihn zurück.
    ///
    /// Port von `PendingRaidStore.pop` (pending_raids.py Z. 370).
    /// Verhält sich identisch zu `get` bzgl. Schlüsselsuche, entfernt aber den Eintrag.
    pub fn pop(
        &mut self,
        to_broadcaster_id: &str,
        from_broadcaster_login: Option<&str>,
    ) -> Option<PendingRaid> {
        let target_id = to_broadcaster_id.trim().to_string();
        if let Some(from) = from_broadcaster_login {
            let key = normalize_pending_raid_key(&target_id, from);
            return self.raids.remove(&key);
        }
        // Fallback: eindeutiger Treffer über target_id
        let matching_key = self
            .raids
            .keys()
            .filter(|(tid, _)| tid == &target_id)
            .cloned()
            .collect::<Vec<_>>();
        if matching_key.len() == 1 {
            self.raids.remove(&matching_key[0])
        } else {
            None
        }
    }

    /// Entfernt alle Raids, deren `registered_ts` älter als `timeout_seconds` ist.
    /// Gibt die entfernten Raids zurück.
    ///
    /// Port von `PendingRaidStore.cleanup_stale` (pending_raids.py Z. 411).
    pub fn cleanup_stale(&mut self, timeout_seconds: f64, now: Option<f64>) -> Vec<PendingRaid> {
        let current = now.unwrap_or_else(unix_now);
        let stale_keys: Vec<(String, String)> = self
            .raids
            .iter()
            .filter(|(_, r)| current - r.registered_ts > timeout_seconds)
            .map(|(k, _)| k.clone())
            .collect();
        stale_keys
            .into_iter()
            .filter_map(|k| self.raids.remove(&k))
            .collect()
    }

    /// Entfernt alle Raids desselben Quell-Streamers, die NICHT auf `current_target_id` zeigen.
    /// Gibt die entfernten Raids zurück.
    ///
    /// Port von `PendingRaidStore.supersede_from_source` (pending_raids.py Z. 431).
    pub fn supersede_from_source(
        &mut self,
        from_broadcaster_login: &str,
        current_target_id: &str,
    ) -> Vec<PendingRaid> {
        let normalized_from = normalize_broadcaster_login(from_broadcaster_login);
        let current_target = current_target_id.trim().to_string();
        if normalized_from.is_empty() {
            return vec![];
        }
        let to_remove: Vec<(String, String)> = self
            .raids
            .iter()
            .filter(|(k, r)| k.0 != current_target && r.from_broadcaster_login == normalized_from)
            .map(|(k, _)| k.clone())
            .collect();
        to_remove
            .into_iter()
            .filter_map(|k| self.raids.remove(&k))
            .collect()
    }

    /// Anzahl aktuell ausstehender Raids.
    pub fn len(&self) -> usize {
        self.raids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.raids.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Hilfsfunktion
// ---------------------------------------------------------------------------

fn unix_now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- normalize_broadcaster_login ---

    #[test]
    fn normalize_login_trim_und_lowercase() {
        assert_eq!(normalize_broadcaster_login("  StreamerA  "), "streamera");
    }

    #[test]
    fn normalize_login_leer() {
        assert_eq!(normalize_broadcaster_login(""), "");
    }

    #[test]
    fn normalize_login_nur_leerzeichen() {
        assert_eq!(normalize_broadcaster_login("   "), "");
    }

    // --- normalize_pending_raid_key ---

    #[test]
    fn normalize_key_trimmt_id_und_lowercase_login() {
        let (tid, from) = normalize_pending_raid_key("  123  ", "  StreamerB  ");
        assert_eq!(tid, "123");
        assert_eq!(from, "streamerb");
    }

    #[test]
    fn normalize_key_leer_inputs() {
        let (tid, from) = normalize_pending_raid_key("", "");
        assert_eq!(tid, "");
        assert_eq!(from, "");
    }

    // --- PendingRaid::new ---

    #[test]
    fn pending_raid_new_normalisiert_felder() {
        let raid = PendingRaid::new("  StreamerC  ", "  456  ");
        assert_eq!(raid.from_broadcaster_login, "streamerc");
        assert_eq!(raid.to_broadcaster_id, "456");
        assert!(!raid.is_partner_raid);
        assert_eq!(raid.registered_viewer_count, 0);
    }

    #[test]
    fn pending_raid_key_korrekt() {
        let raid = PendingRaid::new("Streamer_X", "789");
        let (tid, from) = raid.key();
        assert_eq!(tid, "789");
        assert_eq!(from, "streamer_x");
    }

    #[test]
    fn record_signal_observation_speichert_korrekt() {
        let mut raid = PendingRaid::new("src", "dst");
        raid.record_signal_observation(
            "chat_raid",
            "confirmed",
            Some("viewer_wave".to_string()),
            None,
        );
        let obs = raid.signal_observations.get("chat_raid").unwrap();
        assert_eq!(obs.get("status").unwrap(), "confirmed");
        assert_eq!(obs.get("reason").unwrap(), "viewer_wave");
        assert!(!obs.contains_key("detail"));
    }

    // --- PendingRaidStore ---

    #[test]
    fn store_und_get_exakter_key() {
        let mut s = PendingRaidStore::new();
        let raid = PendingRaid::new("streamer_a", "to_001");
        s.store(raid);
        let got = s.get("to_001", Some("streamer_a")).unwrap();
        assert_eq!(got.from_broadcaster_login, "streamer_a");
    }

    #[test]
    fn get_normalisiert_inputs() {
        let mut s = PendingRaidStore::new();
        s.store(PendingRaid::new("Streamer_A", "To_001"));
        // Suche mit unnormalisiertem Input muss trotzdem treffen
        assert!(s.get("To_001", Some("STREAMER_A")).is_some());
    }

    #[test]
    fn get_ohne_from_einzel_treffer() {
        let mut s = PendingRaidStore::new();
        s.store(PendingRaid::new("src", "target_x"));
        assert!(s.get("target_x", None).is_some());
    }

    #[test]
    fn get_ohne_from_kein_eindeutiger_treffer() {
        let mut s = PendingRaidStore::new();
        s.store(PendingRaid::new("src_a", "same_target"));
        s.store(PendingRaid::new("src_b", "same_target"));
        // Zwei Raids auf selbes Ziel → kein eindeutiger Treffer
        assert!(s.get("same_target", None).is_none());
    }

    #[test]
    fn pop_entfernt_eintrag() {
        let mut s = PendingRaidStore::new();
        s.store(PendingRaid::new("src", "tgt"));
        assert!(s.pop("tgt", Some("src")).is_some());
        assert!(s.is_empty());
    }

    #[test]
    fn cleanup_stale_entfernt_abgelaufene() {
        let mut s = PendingRaidStore::new();
        let mut old = PendingRaid::new("old_src", "old_tgt");
        old.registered_ts = 1.0; // uralt
        s.store(old);
        s.store(PendingRaid::new("new_src", "new_tgt")); // frisch
        let removed = s.cleanup_stale(300.0, None);
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].from_broadcaster_login, "old_src");
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn supersede_from_source_entfernt_altes_ziel() {
        let mut s = PendingRaidStore::new();
        s.store(PendingRaid::new("raider", "old_target"));
        s.store(PendingRaid::new("raider", "new_target"));
        let removed = s.supersede_from_source("raider", "new_target");
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].to_broadcaster_id, "old_target");
        assert_eq!(s.len(), 1);
    }
}
