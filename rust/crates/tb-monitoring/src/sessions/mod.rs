//! Stream-Session-Subsystem: reine Kennzahlen ([`metrics`]), DB-Zugriff
//! ([`store`]) und Lebenszyklus-Orchestrierung ([`tracker`]).

pub mod metrics;
pub mod store;
pub mod tracker;

pub use metrics::{Aggregates, Dropoff, ViewerSample};
pub use store::{
    FinalizeSource, FinalizeUpdate, NewSession, OpenSession, OrphanCandidate, SessionStore,
    StartOutcome,
};
pub use tracker::{
    FollowerCountSource, FollowerFetch, NoFollowerSource, NoRaidTrackingResolver,
    RaidTrackingResolver, SessionTracker,
};

/// Leere oder nur aus Leerzeichen bestehende `twitch_user_id` als "keine ID"
/// behandeln.
///
/// Das Poller-Roster liefert für Kanäle ohne aufgelöste ID `Some("")`. Ohne
/// diesen Filter verdrängt der leere String die echte ID aus dem Fallback und
/// jede ID-Abfrage läuft ins Leere — die Regel stand viermal im Subsystem und
/// fehlte an der vierten Stelle (Merge-Kritiker 10.08.2026).
pub fn echte_twitch_user_id(id: Option<&str>) -> Option<&str> {
    id.map(str::trim).filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::echte_twitch_user_id;

    #[test]
    fn leere_und_blanke_ids_gelten_als_keine_id() {
        assert_eq!(echte_twitch_user_id(None), None);
        assert_eq!(echte_twitch_user_id(Some("")), None);
        assert_eq!(echte_twitch_user_id(Some("   ")), None);
        assert_eq!(echte_twitch_user_id(Some("\t\n")), None);
    }

    #[test]
    fn echte_id_kommt_getrimmt_durch() {
        assert_eq!(echte_twitch_user_id(Some("520300019")), Some("520300019"));
        assert_eq!(echte_twitch_user_id(Some(" 520300019 ")), Some("520300019"));
    }
}
