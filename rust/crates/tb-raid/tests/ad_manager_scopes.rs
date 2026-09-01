//! OAuth-Vertrag des Werbemanagers.
//!
//! Der Werbemanager verwendet den bestehenden Streamer-OAuth und den bereits
//! vorhandenen Token-Speicher. Das kleine `base`-Profil bleibt bewusst
//! read-only. Erst der ausdrückliche Dashboard-Reauth erweitert den Token um
//! Snooze und Commercial. Ein nur teilweise erweiterter Token wäre besonders
//! tückisch: Schedule-Lesen wäre möglich, Snooze oder ein geplanter Break aber
//! würden erst live mit 401/403 scheitern.

use std::collections::BTreeSet;

use tb_raid::scope_profiles::scopes_for_profile;

const AD_MANAGER_SCOPES: [&str; 3] = [
    "channel:read:ads",
    "channel:manage:ads",
    "channel:edit:commercial",
];

#[test]
fn base_bleibt_read_only_und_dashboard_profile_sind_vollstaendig() {
    let base = scopes_for_profile("base");
    assert_eq!(base.len(), 7, "das Basis-Profil darf nicht still wachsen");
    assert!(base.contains(&"channel:read:ads"));
    for write_scope in ["channel:manage:ads", "channel:edit:commercial"] {
        assert!(
            !base.contains(&write_scope),
            "Write-Scope {write_scope} gehört erst in den bewussten Dashboard-Reauth"
        );
    }

    for profile in ["dashboard_reauth", "uplink"] {
        let scopes = scopes_for_profile(profile);
        for required in AD_MANAGER_SCOPES {
            assert!(
                scopes.contains(&required),
                "OAuth-Profil {profile} fehlt Werbemanager-Scope {required}"
            );
        }
    }
}

#[test]
fn werbemanager_scopes_stehen_in_jedem_profil_genau_einmal() {
    for profile in ["dashboard_reauth", "uplink"] {
        let scopes = scopes_for_profile(profile);
        let unique: BTreeSet<_> = scopes.iter().copied().collect();
        assert_eq!(
            unique.len(),
            scopes.len(),
            "OAuth-Profil {profile} enthält doppelte Scopes"
        );
        assert_eq!(
            AD_MANAGER_SCOPES
                .iter()
                .filter(|required| unique.contains(**required))
                .count(),
            AD_MANAGER_SCOPES.len(),
            "OAuth-Profil {profile} hat keinen vollständigen Werbemanager-Scope-Satz"
        );
    }
}
