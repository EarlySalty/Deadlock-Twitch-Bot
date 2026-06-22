//! Observability-Event-Modell + Persistenz-Payload.
//!
//! Parität zu Pythons `RaidObservabilityEvent`
//! (`bot/raid/observability.py:12-53`): strukturiertes Per-Step-Event mit
//! Entscheidung, From-/To-Broadcaster und Detail-Map.

use std::collections::BTreeMap;

use serde_json::Value;

/// Ein einzelnes Observability-Event eines Flows (raid/analytics).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservabilityEvent {
    pub flow_type: String,
    pub flow_id: String,
    pub step: String,
    pub decision: String,
    pub from_broadcaster_login: Option<String>,
    pub from_broadcaster_id: Option<String>,
    pub to_broadcaster_login: Option<String>,
    pub to_broadcaster_id: Option<String>,
    /// Detail-Felder (sortiert für deterministische Serialisierung).
    pub details: BTreeMap<String, Value>,
}

impl ObservabilityEvent {
    /// Bevorzugt das Ziel-Login, fällt auf das Quell-Login zurück
    /// (Python `entity_login`).
    pub fn entity_login(&self) -> String {
        self.to_broadcaster_login
            .clone()
            .or_else(|| self.from_broadcaster_login.clone())
            .unwrap_or_default()
    }

    /// Bevorzugt die Ziel-ID, fällt auf die Quell-ID zurück (Python `entity_id`).
    pub fn entity_id(&self) -> String {
        self.to_broadcaster_id
            .clone()
            .or_else(|| self.from_broadcaster_id.clone())
            .unwrap_or_default()
    }

    /// Persistenz-Payload für `twitch_observability_events`
    /// (Python `as_storage_payload`).
    pub fn as_storage_payload(&self) -> StoragePayload {
        StoragePayload {
            flow_type: self.flow_type.clone(),
            flow_id: self.flow_id.clone(),
            entity_login: self.entity_login(),
            entity_id: self.entity_id(),
            step: self.step.clone(),
            decision: self.decision.clone(),
            details: self.details.clone(),
        }
    }
}

/// Flach strukturierte Persistenz-Repräsentation eines Events; entspricht den
/// Spalten von `twitch_observability_events`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoragePayload {
    pub flow_type: String,
    pub flow_id: String,
    pub entity_login: String,
    pub entity_id: String,
    pub step: String,
    pub decision: String,
    pub details: BTreeMap<String, Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn event() -> ObservabilityEvent {
        let mut details = BTreeMap::new();
        details.insert("reason".to_string(), json!("ok"));
        ObservabilityEvent {
            flow_type: "raid".into(),
            flow_id: "raid-1-1".into(),
            step: "execute".into(),
            decision: "success".into(),
            from_broadcaster_login: Some("from_login".into()),
            from_broadcaster_id: Some("111".into()),
            to_broadcaster_login: Some("to_login".into()),
            to_broadcaster_id: Some("222".into()),
            details,
        }
    }

    #[test]
    fn entity_prefers_target() {
        let e = event();
        assert_eq!(e.entity_login(), "to_login");
        assert_eq!(e.entity_id(), "222");
    }

    #[test]
    fn entity_falls_back_to_source() {
        let mut e = event();
        e.to_broadcaster_login = None;
        e.to_broadcaster_id = None;
        assert_eq!(e.entity_login(), "from_login");
        assert_eq!(e.entity_id(), "111");
    }

    #[test]
    fn storage_payload_maps_fields() {
        let p = event().as_storage_payload();
        assert_eq!(p.flow_type, "raid");
        assert_eq!(p.entity_login, "to_login");
        assert_eq!(p.entity_id, "222");
        assert_eq!(p.step, "execute");
        assert_eq!(p.decision, "success");
        assert_eq!(p.details.get("reason"), Some(&json!("ok")));
    }
}
