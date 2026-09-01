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
/// Uplink-Profil — voller Streamer-Satz plus Chat und Stream-Key für den
/// Multi-Chat und das automatische Uplink-Ziel.
///
/// Bewusst ein eigenes Profil statt einer Erweiterung von
/// [`FULL_STREAMER_SCOPES`]: die Re-Autorisierung über Discord läuft mit
/// `dashboard_reauth` und soll weiterhin nur die bewusst erweiterten Rechte anfragen. Wer den
/// Uplink verbindet, sagt bewusst Ja zu mehr.
pub const UPLINK_SCOPE_PROFILE: &str = "uplink";

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
pub const DASHBOARD_UPGRADE_SCOPES: &[&str] = &[
    "channel:read:subscriptions",
    "channel:read:hype_train",
    "channel:manage:broadcast",
    "channel:manage:ads",
    "channel:edit:commercial",
];

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
    "channel:manage:broadcast",
    "channel:manage:ads",
    "channel:edit:commercial",
];

/// Kritische Basis-Scopes, deren Fehlen den Bot-Betrieb verhindert
/// (Python: `BASE_CRITICAL_STREAMER_SCOPES`).
pub const BASE_CRITICAL_STREAMER_SCOPES: &[&str] = &["bits:read", "channel:read:redemptions"];

/// Was der Uplink über [`FULL_STREAMER_SCOPES`] hinaus braucht.
///
/// - `user:read:chat` und `user:write:chat`: Chat lesen (EventSub
///   `channel.chat.message`) und im Namen des Streamers antworten.
/// - `channel:read:stream_key`: den Stream-Key einmalig holen, damit
///   "Verbinden" das Uplink-Ziel ohne Abtippen anlegen kann.
/// - `moderator:read:followers`: Follows im Aktivitäts-Fenster.
/// - `channel:manage:redemptions`: Kanalpunkt-Einlösungen im Dock abhaken.
///
/// Eigene Liste, weil genau diese fünf über einen alten Raid-Grant
/// hinausgehen: fehlt eine davon, meldet `/uplink/me` `neu_verbinden`.
pub const UPLINK_ONLY_SCOPES: &[&str] = &[
    "user:read:chat",
    "user:write:chat",
    "channel:read:stream_key",
    "moderator:read:followers",
    "channel:manage:redemptions",
];

/// Voller Satz des Uplink-Profils = [`FULL_STREAMER_SCOPES`] +
/// [`UPLINK_ONLY_SCOPES`]. Ein Uplink-Grant ist damit ein echtes Superset des
/// Raid-Grants und darf ihn überschreiben.
pub const UPLINK_SCOPES: &[&str] = &[
    "channel:manage:raids",
    "channel:manage:moderators",
    "channel:bot",
    "clips:edit",
    "channel:read:ads",
    "bits:read",
    "channel:read:redemptions",
    "channel:read:subscriptions",
    "channel:read:hype_train",
    "channel:manage:broadcast",
    "channel:manage:ads",
    "channel:edit:commercial",
    "user:read:chat",
    "user:write:chat",
    "channel:read:stream_key",
    "moderator:read:followers",
    "channel:manage:redemptions",
];

// ---------------------------------------------------------------------------
// Normalisierung
// ---------------------------------------------------------------------------

/// Normalisiert einen Roh-Profilwert auf einen der drei gültigen Discriminatoren.
///
/// - `"base"`, `"dashboard_reauth"` und `"uplink"` werden direkt durchgereicht.
/// - `"auto"` bleibt `"auto"` (wird erst in [`super::oauth_flow`] aufgelöst).
/// - Jeder andere Wert (leer, unbekannt) → `"base"`.
///
/// Python-Äquivalent: `normalize_scope_profile`.
pub fn normalize_scope_profile(raw: &str) -> &'static str {
    match raw.trim().to_ascii_lowercase().as_str() {
        BASE_SCOPE_PROFILE => BASE_SCOPE_PROFILE,
        DASHBOARD_REAUTH_SCOPE_PROFILE => DASHBOARD_REAUTH_SCOPE_PROFILE,
        AUTO_SCOPE_PROFILE => AUTO_SCOPE_PROFILE,
        UPLINK_SCOPE_PROFILE => UPLINK_SCOPE_PROFILE,
        _ => BASE_SCOPE_PROFILE,
    }
}

/// Gibt die Scope-Liste für ein (bereits normalisiertes oder Roh-)Profil zurück.
///
/// `"dashboard_reauth"` → [`FULL_STREAMER_SCOPES`], `"uplink"` →
/// [`UPLINK_SCOPES`]; alle anderen (inkl. `"auto"`) → [`BASE_STREAMER_SCOPES`].
/// Das entspricht dem Python-Verhalten: `auto` wird hier noch *nicht*
/// aufgelöst, sondern in `build_state_info` behandelt.
///
/// Python-Äquivalent: `scopes_for_profile`.
pub fn scopes_for_profile(scope_profile: &str) -> &'static [&'static str] {
    match normalize_scope_profile(scope_profile) {
        DASHBOARD_REAUTH_SCOPE_PROFILE => FULL_STREAMER_SCOPES,
        UPLINK_SCOPE_PROFILE => UPLINK_SCOPES,
        _ => BASE_STREAMER_SCOPES,
    }
}

/// Ob ein gespeicherter Scope-Satz für den Uplink reicht.
///
/// Reine Mengenprüfung gegen [`UPLINK_SCOPES`], damit `/uplink/me` einen alten
/// Raid-Grant von einem Uplink-Grant unterscheiden kann, ohne die Spalte
/// `scopes` an zwei Stellen zu deuten. Zusätzliche Scopes stören nicht.
pub fn hat_alle_uplink_scopes(gespeichert: &[String]) -> bool {
    UPLINK_SCOPES
        .iter()
        .all(|noetig| gespeichert.iter().any(|s| s.trim() == *noetig))
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
        assert!(FULL_STREAMER_SCOPES.contains(&"channel:manage:broadcast"));
    }

    #[test]
    fn scopes_for_uplink_enthaelt_full_und_chat_und_stream_key() {
        let scopes = scopes_for_profile("uplink");
        for scope in FULL_STREAMER_SCOPES {
            assert!(scopes.contains(scope), "uplink fehlt Voll-Scope: {scope}");
        }
        for scope in UPLINK_ONLY_SCOPES {
            assert!(scopes.contains(scope), "uplink fehlt Zusatz-Scope: {scope}");
        }
        assert!(scopes.contains(&"user:read:chat"));
        assert!(scopes.contains(&"user:write:chat"));
        assert!(scopes.contains(&"channel:read:stream_key"));
        assert_eq!(
            scopes.len(),
            FULL_STREAMER_SCOPES.len() + UPLINK_ONLY_SCOPES.len()
        );
        // Keine Dublette: sonst fragt der Dialog ein Recht zweimal an und die
        // Mengengleichheit im AuthWriter wackelt.
        let mut sortiert: Vec<&str> = scopes.to_vec();
        sortiert.sort_unstable();
        let vorher = sortiert.len();
        sortiert.dedup();
        assert_eq!(sortiert.len(), vorher);
    }

    #[test]
    fn normalize_uplink_bleibt_uplink() {
        assert_eq!(normalize_scope_profile("uplink"), "uplink");
        assert_eq!(normalize_scope_profile("  UPLINK "), "uplink");
        // Und die anderen Profile bleiben, wo sie waren.
        assert_eq!(normalize_scope_profile("base"), "base");
        assert_eq!(normalize_scope_profile("uplinks"), "base");
    }

    #[test]
    fn die_uplink_zusatzrechte_stehen_nicht_schon_im_vollen_satz() {
        // Sonst wäre die Prüfmenge für "neu verbinden" wertlos: ein alter
        // Raid-Grant erfüllte sie dann bereits.
        for scope in UPLINK_ONLY_SCOPES {
            assert!(
                !FULL_STREAMER_SCOPES.contains(scope),
                "{scope} steht schon im vollen Raid-Satz"
            );
        }
    }

    /// Die Zusatzrechte gehoeren ausschliesslich dem bewussten Klick "Mit
    /// Twitch verbinden". Wer nur den Raid-Bot autorisiert, soll im
    /// Twitch-Dialog nichts von Stream-Key, Chat oder Kanalpunkten lesen.
    #[test]
    fn auto_und_base_enthalten_keine_uplink_scopes() {
        for profil in ["base", "auto", "dashboard_reauth", "", "unbekannt"] {
            let scopes = scopes_for_profile(profil);
            for zusatz in UPLINK_ONLY_SCOPES {
                assert!(
                    !scopes.contains(zusatz),
                    "Profil {profil} fragt {zusatz} an, das gehoert nur zum Uplink"
                );
            }
        }
        // Das Basisprofil bleibt absichtlich klein; Schreibrechte gibt es nur
        // nach der bewussten Dashboard-Re-Autorisierung.
        assert_eq!(scopes_for_profile("base").len(), 7);
        assert_eq!(scopes_for_profile("dashboard_reauth").len(), 12);
        assert_eq!(scopes_for_profile("auto").len(), 7);
    }

    #[test]
    fn alter_raid_grant_erfuellt_die_uplink_scopes_nicht() {
        let alt: Vec<String> = FULL_STREAMER_SCOPES.iter().map(|s| s.to_string()).collect();
        assert!(!hat_alle_uplink_scopes(&alt));
        let neu: Vec<String> = UPLINK_SCOPES.iter().map(|s| s.to_string()).collect();
        assert!(hat_alle_uplink_scopes(&neu));
        // Reihenfolge und Zusatzrechte spielen keine Rolle.
        let mut gedreht = neu.clone();
        gedreht.reverse();
        gedreht.push("channel:read:goals".to_string());
        assert!(hat_alle_uplink_scopes(&gedreht));
        assert!(!hat_alle_uplink_scopes(&[]));
    }
}
