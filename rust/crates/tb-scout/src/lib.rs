//! tb-scout: findet kleine, erstmalig gesehene Deadlock-Twitch-Kanäle und
//! legt sie nach Admin-Freigabe in die BESTEHENDE Outreach-Kette
//! (`twitch_partner_outreach`). Kein eigener Versand, keine neue Nachrichtenart.
//!
//! Filter wirken ausschließlich auf Verhalten (Sessions, Ø Zuschauer),
//! Listenbestand (Black-/Denylists, Suppression, Cooldown) und Sendedaten
//! (first_seen, dispatched_at) — nie auf Identitätsmerkmale.

pub mod detector;
pub mod store;

/// Status eines Kandidaten in `twitch_scout_candidates`.
pub const STATUS_VORGESCHLAGEN: &str = "vorgeschlagen";
pub const STATUS_APPROVED: &str = "approved";
pub const STATUS_UEBERSPRUNGEN: &str = "uebersprungen";
pub const STATUS_PAUSIERT: &str = "pausiert";

/// Entscheidungswerte des Admin-Endpoints (POST …/decision). `approve` ist
/// der Eingabewert; gespeichert wird [`STATUS_APPROVED`].
pub const DECISION_APPROVE: &str = "approve";
pub const DECISION_UEBERSPRUNGEN: &str = "uebersprungen";
pub const DECISION_PAUSIERT: &str = "pausiert";

/// Normalisiert einen Entscheidungswert auf den gespeicherten Status.
/// Unbekannte Eingaben → `None` (kein stiller Default wie im Bestand).
pub fn normalize_entscheidung(decision: &str) -> Option<&'static str> {
    match decision.trim().to_lowercase().as_str() {
        DECISION_APPROVE | STATUS_APPROVED => Some(STATUS_APPROVED),
        DECISION_UEBERSPRUNGEN => Some(STATUS_UEBERSPRUNGEN),
        DECISION_PAUSIERT => Some(STATUS_PAUSIERT),
        _ => None,
    }
}

/// Normalisiert einen Twitch-Login (Trim + Kleinschreibung); leer → `None`.
pub fn normalisiere_login(login: &str) -> Option<String> {
    let login = login.trim().to_lowercase();
    if login.is_empty() {
        None
    } else {
        Some(login)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entscheidungen_werden_korrekt_gemappt() {
        assert_eq!(normalize_entscheidung("approve"), Some(STATUS_APPROVED));
        assert_eq!(normalize_entscheidung("Approved"), Some(STATUS_APPROVED));
        assert_eq!(
            normalize_entscheidung("uebersprungen"),
            Some(STATUS_UEBERSPRUNGEN)
        );
        assert_eq!(normalize_entscheidung("pausiert"), Some(STATUS_PAUSIERT));
        assert_eq!(normalize_entscheidung("vorgeschlagen"), None);
        assert_eq!(normalize_entscheidung("  "), None);
    }

    #[test]
    fn login_wird_kleingeschrieben_und_getrimmt() {
        assert_eq!(
            normalisiere_login("  GrosserKanal ").as_deref(),
            Some("grosserkanal")
        );
        assert_eq!(normalisiere_login("   "), None);
    }
}
