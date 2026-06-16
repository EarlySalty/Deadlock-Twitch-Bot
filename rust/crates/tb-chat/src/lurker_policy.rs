//! Passive-Lurker-Policy — Port von `bot/chat/lurker_policy.py`.
//!
//! # Hintergrund (B8-07)
//!
//! Ein „monitored-only"-Kanal sammelt nur Daten: der Bot liest Chat, hat aber
//! keine Broadcaster-Autorisierung für ausgehende Aktionen und ist kein aktiver
//! Partner. Für solche Kanäle ist **passive Beobachtung der erwartete
//! Endzustand** — die Chat-Reconcile darf dort *nicht* versuchen, fehlende
//! Send-/Mod-Subscriptions nachzuziehen oder das Runtime-Membership zu heilen.
//! Andernfalls produziert jeder Reconcile-Durchlauf wiederkehrende
//! Subscribe-Fehlversuche gegen Kanäle, die bewusst nur beobachtet werden.
//!
//! Die zwei reinen Prädikate hier kapseln genau diese Entscheidung:
//!
//! - [`is_passive_lurker_channel`] — markiert einen Kanal als Lurker-Endzustand
//!   (`connection.py:1237`, `connection.py:1532`). Treffer → Reconcile schreibt
//!   den Subscription-State [`PASSIVE_LURKER_STATE`] mit [`PASSIVE_LURKER_DETAIL`]
//!   statt einen Subscribe-Versuch zu starten.
//! - [`should_attempt_runtime_heal`] — Scout-Heal-Gate (`base.py:1135`):
//!   monitored-only Kanäle sind **nie** Heal-Ziele; sonst heilt der Scout nur,
//!   wenn das Runtime-Membership noch nicht bereit ist.
//!
//! Beide Funktionen sind bewusst DB-frei und seiteneffektlos — die aufrufenden
//! Reconcile-/Scout-Schleifen (in `tb-monitoring`/`tb-bot`) liefern die Flags.
//!
//! Port: `bot/chat/lurker_policy.py` (vollständig, 1:1).

/// Subscription-State-Marker für Lurker-Kanäle.
///
/// Port: `lurker_policy.py:PASSIVE_LURKER_STATE`.
pub const PASSIVE_LURKER_STATE: &str = "passive_lurker";

/// Erläuterung, die zusammen mit [`PASSIVE_LURKER_STATE`] persistiert wird.
///
/// Port: `lurker_policy.py:PASSIVE_LURKER_DETAIL`.
pub const PASSIVE_LURKER_DETAIL: &str =
    "monitored-only channel without broadcaster authorization runs in passive lurker mode";

/// `true`, wenn passive Beobachtung der erwartete Endzustand des Kanals ist.
///
/// Bedingung (alle drei): Kanal ist monitored-only **und** kein aktiver Partner
/// **und** ohne Raid-Auth. Ein Treffer signalisiert der Chat-Reconcile, *keinen*
/// Subscribe-Versuch zu starten, sondern den State [`PASSIVE_LURKER_STATE`] zu
/// schreiben.
///
/// Port: `lurker_policy.py:is_passive_lurker_channel`.
pub fn is_passive_lurker_channel(
    is_monitored_only: bool,
    is_partner_active: bool,
    has_raid_auth: bool,
) -> bool {
    is_monitored_only && !is_partner_active && !has_raid_auth
}

/// `true`, wenn der Scout für diesen Kanal ein Runtime-Heal versuchen soll.
///
/// Monitored-only Kanäle sind **nie** Heal-Ziele (`false`). Für alle anderen
/// Kanäle wird nur geheilt, wenn das Runtime-Membership noch nicht bereit ist
/// (`!is_ready`).
///
/// Port: `lurker_policy.py:should_attempt_runtime_heal`.
pub fn should_attempt_runtime_heal(is_monitored_only: bool, is_ready: bool) -> bool {
    if is_monitored_only {
        return false;
    }
    !is_ready
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // is_passive_lurker_channel — vollständige Wahrheitstabelle (2^3 = 8)
    // -----------------------------------------------------------------------

    #[test]
    fn passive_lurker_nur_bei_monitored_ohne_partner_ohne_raidauth() {
        // monitored=1, partner=0, raid=0 → der EINZIGE true-Fall.
        assert!(is_passive_lurker_channel(true, false, false));
    }

    #[test]
    fn passive_lurker_falsch_bei_aktivem_partner() {
        // Aktiver Partner ist nie passiver Lurker, auch wenn monitored-only.
        assert!(!is_passive_lurker_channel(true, true, false));
    }

    #[test]
    fn passive_lurker_falsch_bei_raid_auth() {
        // Raid-Auth (Broadcaster-Token vorhanden) → kein passiver Lurker.
        assert!(!is_passive_lurker_channel(true, false, true));
    }

    #[test]
    fn passive_lurker_falsch_wenn_nicht_monitored_only() {
        // Nicht monitored-only → nie passiver Lurker, egal welche Kombination.
        assert!(!is_passive_lurker_channel(false, false, false));
        assert!(!is_passive_lurker_channel(false, true, false));
        assert!(!is_passive_lurker_channel(false, false, true));
        assert!(!is_passive_lurker_channel(false, true, true));
        // letzter Rest der Tabelle: monitored + partner + raid
        assert!(!is_passive_lurker_channel(true, true, true));
    }

    // -----------------------------------------------------------------------
    // should_attempt_runtime_heal
    // -----------------------------------------------------------------------

    #[test]
    fn heal_nie_fuer_monitored_only() {
        // monitored-only → nie heilen, egal ob bereit oder nicht.
        assert!(!should_attempt_runtime_heal(true, false));
        assert!(!should_attempt_runtime_heal(true, true));
    }

    #[test]
    fn heal_fuer_nicht_bereiten_normalen_kanal() {
        // Normaler Kanal, nicht bereit → heilen.
        assert!(should_attempt_runtime_heal(false, false));
    }

    #[test]
    fn heal_nicht_fuer_bereiten_normalen_kanal() {
        // Normaler Kanal, bereits bereit → kein Heal nötig.
        assert!(!should_attempt_runtime_heal(false, true));
    }

    // -----------------------------------------------------------------------
    // Konstanten wortgetreu zum Python-Orakel
    // -----------------------------------------------------------------------

    #[test]
    fn konstanten_wortgetreu() {
        assert_eq!(PASSIVE_LURKER_STATE, "passive_lurker");
        assert_eq!(
            PASSIVE_LURKER_DETAIL,
            "monitored-only channel without broadcaster authorization runs in passive lurker mode"
        );
    }
}
