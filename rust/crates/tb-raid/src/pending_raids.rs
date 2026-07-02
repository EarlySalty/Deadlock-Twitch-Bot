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
//! | target_stream_data          | target_stream_data          | Option<serde_json::Value> |
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

use serde::{Deserialize, Serialize};

use crate::util::unix_now;

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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingRaid {
    /// Login-Name des Quell-Streamers (normalisiert).
    pub from_broadcaster_login: String,
    /// Broadcaster-ID des Ziel-Kanals (getrimmt).
    pub to_broadcaster_id: String,
    /// Eingefrorener Ziel-Stream-Snapshot zur Raid-Sendezeit (z. B. `_partner_score`).
    ///
    /// Port von `PendingRaid.target_stream_data` (pending_raids.py Z. 102). Python hält hier
    /// ein freies `dict[str, Any]` (nur falls Mapping, sonst `None`); in Rust als beliebiger
    /// JSON-Wert. Wird bei der Arrival-Bestätigung dem frischen DB-Score vorgezogen.
    #[serde(default)]
    pub target_stream_data: Option<serde_json::Value>,
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
            target_stream_data: None,
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
// Timeout-Diagnose (P2.31)
// ---------------------------------------------------------------------------

/// Synthetisiert den menschenlesbaren `Timeout detail: …`-String aus den
/// Signal-Beobachtungen eines abgelaufenen Pendings.
///
/// Port von `RaidTrackingRuntimeService.build_pending_timeout_detail`
/// (raid_tracking_runtime.py Z. 518–553): zuerst je Signal
/// (`channel.raid`, `channel.chat.notification`) `status (reason) [detail]`;
/// fehlen Beobachtungen, dann Fallback aus `channel_raid_ready` und
/// `chat_notification_state`/`-detail`.
pub fn build_pending_timeout_detail(pending: &PendingRaid) -> String {
    let mut parts: Vec<String> = Vec::new();
    for signal_type in ["channel.raid", "channel.chat.notification"] {
        let Some(obs) = pending.signal_observations.get(signal_type) else {
            continue;
        };
        let status = obs.get("status").map(|s| s.trim()).unwrap_or("");
        let reason = obs.get("reason").map(|s| s.trim()).unwrap_or("");
        let detail = obs.get("detail").map(|s| s.trim()).unwrap_or("");
        let mut text = if status.is_empty() {
            signal_type.to_string()
        } else {
            format!("{signal_type}:{status}")
        };
        if !reason.is_empty() {
            text.push_str(&format!(" ({reason})"));
        }
        if !detail.is_empty() {
            text.push_str(&format!(" [{detail}]"));
        }
        parts.push(text);
    }

    if parts.is_empty() {
        // Fallback: channel_raid_ready (None/true → "ready", false → "subscription_not_ready").
        let channel_raid_detail = if pending.channel_raid_ready == Some(false) {
            "subscription_not_ready"
        } else {
            "ready"
        };
        let chat_state = pending
            .chat_notification_state
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("missing");
        let chat_detail = pending
            .chat_notification_detail
            .as_deref()
            .map(str::trim)
            .unwrap_or("");
        let mut chat_text = format!("channel.chat.notification:{chat_state}");
        if !chat_detail.is_empty() {
            chat_text.push_str(&format!(" [{chat_detail}]"));
        }
        parts.push(format!("channel.raid:{channel_raid_detail}"));
        parts.push(chat_text);
    }

    format!("Timeout detail: {}", parts.join("; "))
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

    /// Sweep für veraltete Pending-Raids inkl. Timeout-Diagnose (P2.29 + P2.31).
    ///
    /// Port von `RaidTrackingRuntimeService.cleanup_stale_pending_raids`
    /// (raid_tracking_runtime.py Z. 74–120): entfernt alle abgelaufenen Pendings
    /// (`cleanup_stale`), erzeugt je Eintrag den menschenlesbaren
    /// `build_pending_timeout_detail`-String, loggt eine Warnung mit Alter +
    /// Detail und gibt `(PendingRaid, timeout_detail)`-Paare zurück, damit der
    /// Aufrufer Observability-Events/Counter emittieren kann.
    ///
    /// Der periodische Aufruf (300 s, `RaidTrackingRuntimeConfig.cleanup_timeout_seconds`)
    /// wird im Composition-Root verdrahtet — siehe WIRING-TODO.
    pub fn sweep_stale(
        &mut self,
        timeout_seconds: f64,
        now: Option<f64>,
    ) -> Vec<(PendingRaid, String)> {
        let current = now.unwrap_or_else(unix_now);
        let stale = self.cleanup_stale(timeout_seconds, Some(current));
        stale
            .into_iter()
            .map(|pending| {
                let detail = build_pending_timeout_detail(&pending);
                let age = current - pending.registered_ts;
                let from = if pending.from_broadcaster_login.is_empty() {
                    "<unknown>"
                } else {
                    pending.from_broadcaster_login.as_str()
                };
                tracing::warn!(
                    age_seconds = age.round() as i64,
                    from = %from,
                    to_broadcaster_id = %pending.to_broadcaster_id,
                    timeout_detail = %detail,
                    "Pending raid timed out"
                );
                (pending, detail)
            })
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

    /// Read-only-Iteration über alle Einträge als `(&key, &PendingRaid)`.
    ///
    /// Port von `PendingRaidStore.iter_entries` (pending_raids.py Z. 306) — ohne
    /// Mutation. Genutzt vom Source-Unraid-Cancel-Pfad, der alle Raids eines
    /// Quell-Streamers finden muss, ohne den Store zu verändern.
    pub fn iter(&self) -> impl Iterator<Item = (&(String, String), &PendingRaid)> {
        self.raids.iter()
    }

    /// Read-only-Iteration über alle Raids (`&PendingRaid`), ohne Keys.
    pub fn values(&self) -> impl Iterator<Item = &PendingRaid> {
        self.raids.values()
    }

    /// Storniert alle ausstehenden Raids eines Quell-Streamers (B7-03,
    /// Source-Self-Unraid). Einziges Match-Kriterium ist
    /// `from_broadcaster_login` (normalisiert) — wie Python
    /// `cancel_pending_raids_for_source_unraid` (raid_tracking_runtime.py
    /// Z. 160–220). Nutzt die [`Self::iter`]-API zum read-only Sammeln der
    /// Treffer-Keys (kein Borrow-Konflikt mit `pop`) und entfernt sie dann.
    /// Fremde Pendings bleiben unangetastet. Gibt die entfernten Raids zurück.
    pub fn cancel_from_source(&mut self, from_broadcaster_login: &str) -> Vec<PendingRaid> {
        let normalized_from = normalize_broadcaster_login(from_broadcaster_login);
        if normalized_from.is_empty() {
            return vec![];
        }
        let matching_keys: Vec<(String, String)> = self
            .iter()
            .filter(|(_, raid)| {
                normalize_broadcaster_login(&raid.from_broadcaster_login) == normalized_from
            })
            .map(|(key, _)| key.clone())
            .collect();
        matching_keys
            .into_iter()
            .filter_map(|(to_id, from)| self.pop(&to_id, Some(&from)))
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

    // --- P2.29: Sweep entfernt abgelaufene Pendings ---

    #[test]
    fn sweep_stale_entfernt_und_liefert_detail() {
        let mut s = PendingRaidStore::new();
        let mut old = PendingRaid::new("old_src", "old_tgt");
        old.registered_ts = 1.0; // uralt
        s.store(old);
        s.store(PendingRaid::new("new_src", "new_tgt")); // frisch

        let swept = s.sweep_stale(300.0, None);
        assert_eq!(swept.len(), 1, "genau der alte Pending wird gesweept");
        let (raid, detail) = &swept[0];
        assert_eq!(raid.from_broadcaster_login, "old_src");
        assert!(
            detail.starts_with("Timeout detail:"),
            "Timeout-Detail-String erzeugt: {detail}"
        );
        assert_eq!(s.len(), 1, "frischer Pending bleibt im Store");
    }

    #[test]
    fn sweep_stale_leer_wenn_nichts_abgelaufen() {
        let mut s = PendingRaidStore::new();
        s.store(PendingRaid::new("src", "tgt"));
        assert!(s.sweep_stale(300.0, None).is_empty());
        assert_eq!(s.len(), 1);
    }

    // --- P2.31: build_pending_timeout_detail ---

    #[test]
    fn timeout_detail_aus_signal_observations() {
        let mut raid = PendingRaid::new("src", "tgt");
        raid.record_signal_observation(
            "channel.raid",
            "ready",
            Some("subscribed".to_string()),
            None,
        );
        raid.record_signal_observation(
            "channel.chat.notification",
            "missing",
            None,
            Some("no_signal".to_string()),
        );
        let detail = build_pending_timeout_detail(&raid);
        assert_eq!(
            detail,
            "Timeout detail: channel.raid:ready (subscribed); \
             channel.chat.notification:missing [no_signal]"
        );
    }

    #[test]
    fn timeout_detail_fallback_ohne_observations() {
        let mut raid = PendingRaid::new("src", "tgt");
        raid.channel_raid_ready = Some(false);
        raid.chat_notification_state = Some("sent".to_string());
        raid.chat_notification_detail = Some("delayed".to_string());
        let detail = build_pending_timeout_detail(&raid);
        assert_eq!(
            detail,
            "Timeout detail: channel.raid:subscription_not_ready; \
             channel.chat.notification:sent [delayed]"
        );
    }

    #[test]
    fn timeout_detail_fallback_leerer_chat_state_ist_missing() {
        let raid = PendingRaid::new("src", "tgt"); // channel_raid_ready=None → "ready"
        let detail = build_pending_timeout_detail(&raid);
        assert_eq!(
            detail,
            "Timeout detail: channel.raid:ready; channel.chat.notification:missing"
        );
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

    // --- B7-05: target_stream_data ---

    #[test]
    fn pending_raid_new_target_stream_data_default_none() {
        let raid = PendingRaid::new("src", "dst");
        assert!(raid.target_stream_data.is_none());
    }

    #[test]
    fn pending_raid_target_stream_data_round_trip() {
        let mut raid = PendingRaid::new("src", "dst");
        raid.target_stream_data = Some(serde_json::json!({
            "_partner_score": { "final_score": 12.5, "base_score": 10.0 },
            "viewer_count": 42,
        }));

        let json = serde_json::to_string(&raid).expect("serialize");
        let back: PendingRaid = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(back.from_broadcaster_login, "src");
        assert_eq!(back.to_broadcaster_id, "dst");
        assert_eq!(back.target_stream_data, raid.target_stream_data);
        // Python-Zugriff: target_stream_data.get("_partner_score").get("final_score")
        let tsd = back.target_stream_data.expect("target_stream_data present");
        assert_eq!(
            tsd["_partner_score"]["final_score"],
            serde_json::json!(12.5)
        );
    }

    // --- B7-08: read-only Iterations-API ---

    #[test]
    fn store_values_liefert_alle_eintraege() {
        let mut s = PendingRaidStore::new();
        s.store(PendingRaid::new("src_a", "tgt_1"));
        s.store(PendingRaid::new("src_b", "tgt_2"));

        let mut froms: Vec<String> = s
            .values()
            .map(|r| r.from_broadcaster_login.clone())
            .collect();
        froms.sort();
        assert_eq!(froms, vec!["src_a".to_string(), "src_b".to_string()]);
        assert_eq!(s.values().count(), s.len());
    }

    // --- B7-03: Source-Self-Unraid-Cancel über iter() ---

    #[test]
    fn cancel_from_source_storniert_nur_eigene_quelle() {
        let mut s = PendingRaidStore::new();
        // Zwei Auto-Raids desselben Quell-Streamers auf verschiedene Ziele.
        s.store(PendingRaid::new("Raider", "tgt_1"));
        s.store(PendingRaid::new("raider", "tgt_2"));
        // Fremder Quell-Streamer bleibt unangetastet.
        s.store(PendingRaid::new("anderer", "tgt_3"));

        // Unnormalisierter Input trifft trotzdem (trim + lowercase).
        let canceled = s.cancel_from_source("  RAIDER  ");
        assert_eq!(canceled.len(), 2, "beide Raids der Quelle storniert");
        let mut targets: Vec<String> = canceled
            .iter()
            .map(|r| r.to_broadcaster_id.clone())
            .collect();
        targets.sort();
        assert_eq!(targets, vec!["tgt_1".to_string(), "tgt_2".to_string()]);

        // Fremder Pending bleibt im Store.
        assert_eq!(s.len(), 1);
        assert!(s.get("tgt_3", Some("anderer")).is_some());
    }

    #[test]
    fn cancel_from_source_leerer_login_macht_nichts() {
        let mut s = PendingRaidStore::new();
        s.store(PendingRaid::new("raider", "tgt_1"));
        assert!(s.cancel_from_source("   ").is_empty());
        assert_eq!(s.len(), 1, "kein Pending entfernt bei leerem Login");
    }

    #[test]
    fn store_iter_liefert_key_und_raid() {
        let mut s = PendingRaidStore::new();
        s.store(PendingRaid::new("src_a", "tgt_1"));
        s.store(PendingRaid::new("src_b", "tgt_2"));

        let mut keys: Vec<(String, String)> = s.iter().map(|(k, _)| k.clone()).collect();
        keys.sort();
        assert_eq!(
            keys,
            vec![
                ("tgt_1".to_string(), "src_a".to_string()),
                ("tgt_2".to_string(), "src_b".to_string()),
            ]
        );
        // Source-Unraid-Cancel-Pfad: über &PendingRaid filtern, ohne Mutation
        let from_src_a: Vec<&PendingRaid> = s
            .iter()
            .filter(|(_, r)| r.from_broadcaster_login == "src_a")
            .map(|(_, r)| r)
            .collect();
        assert_eq!(from_src_a.len(), 1);
        assert_eq!(from_src_a[0].to_broadcaster_id, "tgt_1");
        assert_eq!(s.iter().count(), s.len());
    }
}
