//! Twitch OAuth Scope-Profile (Python: `bot/raid/scope_profiles.py`).
//!
//! Reines, I/O-freies Modul — kein DB-Zugriff, keine Netzwerkaufrufe.
//! Enthält alle Scope-Konstanten, Normalisierung und Profil-Auflösung
//! 1:1 zum Python-Original.

// ---------------------------------------------------------------------------
// Profil-Discriminatoren
// ---------------------------------------------------------------------------

/// Basis-Profil — minimaler Scope-Satz für Raid-Funktionalität.
pub const BASE_SCOPE_PROFILE: &str = "base";
/// Dashboard-Re-Auth-Profil — erweiterter Satz inkl. Abo- und Hype-Train-Daten.
pub const DASHBOARD_REAUTH_SCOPE_PROFILE: &str = "dashboard_reauth";
/// Auto-Profil — wird zur Laufzeit über Streamer-Kontext aufgelöst.
pub const AUTO_SCOPE_PROFILE: &str = "auto";

// ---------------------------------------------------------------------------
// Scope-Listen
// ---------------------------------------------------------------------------

/// Minimaler Scope-Satz, der für Raid + Moderation benötigt wird
/// (Python: `BASE_STREAMER_SCOPES`).
pub const BASE_STREAMER_SCOPES: &[&str] = &[
    "channel:manage:raids",
    "channel:manage:moderators",
    "channel:bot",
    "clips:edit",
    "channel:read:ads",
    "bits:read",
    "channel:read:redemptions",
];

/// Zusätzliche Scopes für Dashboard-Features (Python: `DASHBOARD_UPGRADE_SCOPES`).
pub const DASHBOARD_UPGRADE_SCOPES: &[&str] =
    &["channel:read:subscriptions", "channel:read:hype_train"];

/// Vollständiger Satz = Basis + Dashboard-Upgrade
/// (Python: `FULL_STREAMER_SCOPES`).
pub const FULL_STREAMER_SCOPES: &[&str] = &[
    "channel:manage:raids",
    "channel:manage:moderators",
    "channel:bot",
    "clips:edit",
    "channel:read:ads",
    "bits:read",
    "channel:read:redemptions",
    "channel:read:subscriptions",
    "channel:read:hype_train",
];

/// Kritische Basis-Scopes, deren Fehlen den Bot-Betrieb verhindert
/// (Python: `BASE_CRITICAL_STREAMER_SCOPES`).
pub const BASE_CRITICAL_STREAMER_SCOPES: &[&str] = &["bits:read", "channel:read:redemptions"];

// ---------------------------------------------------------------------------
// Normalisierung
// ---------------------------------------------------------------------------

/// Normalisiert einen Roh-Profilwert auf einen der drei gültigen Discriminatoren.
///
/// - `"base"` und `"dashboard_reauth"` werden direkt durchgereicht.
/// - `"auto"` bleibt `"auto"` (wird erst in [`super::oauth_flow`] aufgelöst).
/// - Jeder andere Wert (leer, unbekannt) → `"base"`.
///
/// Python-Äquivalent: `normalize_scope_profile`.
pub fn normalize_scope_profile(raw: &str) -> &'static str {
    match raw.trim().to_ascii_lowercase().as_str() {
        BASE_SCOPE_PROFILE => BASE_SCOPE_PROFILE,
        DASHBOARD_REAUTH_SCOPE_PROFILE => DASHBOARD_REAUTH_SCOPE_PROFILE,
        AUTO_SCOPE_PROFILE => AUTO_SCOPE_PROFILE,
        _ => BASE_SCOPE_PROFILE,
    }
}

/// Gibt die Scope-Liste für ein (bereits normalisiertes oder Roh-)Profil zurück.
///
/// `"dashboard_reauth"` → [`FULL_STREAMER_SCOPES`]; alle anderen (inkl. `"auto"`)
/// → [`BASE_STREAMER_SCOPES`]. Das entspricht dem Python-Verhalten: `auto` wird
/// hier noch *nicht* aufgelöst, sondern in `build_state_info` behandelt.
///
/// Python-Äquivalent: `scopes_for_profile`.
pub fn scopes_for_profile(scope_profile: &str) -> &'static [&'static str] {
    if normalize_scope_profile(scope_profile) == DASHBOARD_REAUTH_SCOPE_PROFILE {
        FULL_STREAMER_SCOPES
    } else {
        BASE_STREAMER_SCOPES
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_bekannte_profile_bleiben_erhalten() {
        assert_eq!(normalize_scope_profile("base"), "base");
        assert_eq!(
            normalize_scope_profile("dashboard_reauth"),
            "dashboard_reauth"
        );
        assert_eq!(normalize_scope_profile("auto"), "auto");
    }

    #[test]
    fn normalize_gross_und_leerzeichen_werden_normalisiert() {
        assert_eq!(normalize_scope_profile("  BASE  "), "base");
        assert_eq!(
            normalize_scope_profile("DASHBOARD_REAUTH"),
            "dashboard_reauth"
        );
        assert_eq!(normalize_scope_profile("Auto"), "auto");
    }

    #[test]
    fn normalize_ungueltige_werte_fallen_auf_base_zurueck() {
        assert_eq!(normalize_scope_profile(""), "base");
        assert_eq!(normalize_scope_profile("unbekannt"), "base");
        assert_eq!(normalize_scope_profile("full"), "base");
        assert_eq!(normalize_scope_profile("  "), "base");
    }

    #[test]
    fn normalize_auto_bleibt_auto_nicht_base() {
        // Sicherstellen: auto wird nicht still zu base degradiert.
        let result = normalize_scope_profile("auto");
        assert_eq!(result, "auto");
        assert_ne!(result, "base");
    }

    #[test]
    fn scopes_for_base_sind_korrekt() {
        let scopes = scopes_for_profile("base");
        assert!(scopes.contains(&"channel:manage:raids"));
        assert!(scopes.contains(&"bits:read"));
        assert!(scopes.contains(&"channel:read:redemptions"));
        // Dashboard-Scopes dürfen NICHT enthalten sein.
        assert!(!scopes.contains(&"channel:read:subscriptions"));
        assert!(!scopes.contains(&"channel:read:hype_train"));
        assert_eq!(scopes.len(), BASE_STREAMER_SCOPES.len());
    }

    #[test]
    fn scopes_for_dashboard_reauth_enthalten_vollen_satz() {
        let scopes = scopes_for_profile("dashboard_reauth");
        assert!(scopes.contains(&"channel:manage:raids"));
        assert!(scopes.contains(&"bits:read"));
        assert!(scopes.contains(&"channel:read:subscriptions"));
        assert!(scopes.contains(&"channel:read:hype_train"));
        assert_eq!(scopes.len(), FULL_STREAMER_SCOPES.len());
    }

    #[test]
    fn scopes_for_auto_liefert_base_satz() {
        // auto wird erst in build_state_info aufgelöst — hier Basis-Fallback.
        assert_eq!(scopes_for_profile("auto"), BASE_STREAMER_SCOPES);
    }

    #[test]
    fn full_streamer_scopes_enthaelt_alle_basis_scopes() {
        for scope in BASE_STREAMER_SCOPES {
            assert!(
                FULL_STREAMER_SCOPES.contains(scope),
                "FULL_STREAMER_SCOPES fehlt Basis-Scope: {scope}"
            );
        }
        for scope in DASHBOARD_UPGRADE_SCOPES {
            assert!(
                FULL_STREAMER_SCOPES.contains(scope),
                "FULL_STREAMER_SCOPES fehlt Upgrade-Scope: {scope}"
            );
        }
        assert_eq!(
            FULL_STREAMER_SCOPES.len(),
            BASE_STREAMER_SCOPES.len() + DASHBOARD_UPGRADE_SCOPES.len()
        );
    }
}
