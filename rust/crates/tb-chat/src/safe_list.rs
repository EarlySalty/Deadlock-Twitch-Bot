//! Safe-List — Konten, die NIE automatisch moderiert werden.
//!
//! Der Gegenpol zur `crew_guard::CREW_REGISTRY`. Diese Konten reden
//! nachweislich ÜBER die Ricky-Kampagne (und treffen damit die bewusst
//! mehrdeutigen Trigger-Wörter wie „ricky" oder „bannliste"), gehören ihr aber
//! ausdrücklich nicht an. Sie lösen deshalb weder Auto-Ban noch
//! Nachrichten-Löschung noch einen Crew-Guard-Alarm aus.
//!
//! Die Liste wirkt vor **jeder** Moderations-Aktion (Ban, Timeout,
//! Message-Delete). Es gibt fünf Stellen, an denen der Bot gegen einen Chatter
//! handelt, und jede prüft [`is_safe`]:
//!   1. [`crate::moderation::ModerationEngine::auto_ban_and_cleanup`] — Ban +
//!      Delete für Spam, Scam und Global-Ban aus der Chat-Pipeline.
//!   2. [`crate::moderation::ModerationEngine::timeout_and_cleanup`] — Timeout
//!      + Delete. Ein Timeout ist ebenfalls Moderation.
//!   3. `conversation_scam::try_ban` — bannt direkt, an (1) vorbei.
//!   4. `pipeline::handle_strong_timeout` — timeoutet direkt, an (2) vorbei.
//!   5. `global_ban_sweep::list_bans` — filtert die Banliste, wirkt daher auch
//!      bei einem direkten DB-Eintrag.
//!
//! Zusätzlich unterdrückt [`crate::crew_guard`] jede Meldung zu diesen Konten.
//!
//! Wer hier einen neuen Ban-/Timeout-/Delete-Pfad ergänzt, prüft [`is_safe`]
//! davor. Suche nach `api.ban_user`, `api.timeout_user`, `api.delete_message`.

/// Ein Konto, das von automatischer Moderation ausgenommen ist.
pub struct SafeAccount {
    pub twitch_user_id: &'static str,
    /// Lowercase-Login. Nur relevant, wenn zu einer Nachricht KEINE ID vorliegt.
    pub login: &'static str,
}

/// Ausgenommene Konten (Ansage nani, 2026-07-10).
pub const SAFE_ACCOUNTS: &[SafeAccount] = &[
    SafeAccount {
        twitch_user_id: "455311800",
        login: "fr4gm1nt",
    },
    SafeAccount {
        twitch_user_id: "19123804",
        login: "kubi_kubi_kubi",
    },
];

/// `true`, wenn dieses Konto von jeder Auto-Moderation ausgenommen ist.
///
/// Liegt eine `chatter_id` vor, entscheidet **ausschliesslich** sie. Der Login
/// ist nur der Notnagel für Alt-Nachrichten ohne ID (IRC-Backfill): Twitch gibt
/// aufgegebene Logins wieder frei, ein „ID ODER Login"-Match wäre also eine
/// Hintertür — wer den freien Namen registriert, wäre sonst bann-immun.
pub fn is_safe(chatter_id: Option<&str>, chatter_login: &str) -> bool {
    if let Some(id) = chatter_id.map(str::trim).filter(|id| !id.is_empty()) {
        return SAFE_ACCOUNTS.iter().any(|acc| acc.twitch_user_id == id);
    }

    let login = chatter_login.trim().to_lowercase();
    !login.is_empty() && SAFE_ACCOUNTS.iter().any(|acc| acc.login == login)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn erkennt_safe_konto_per_id() {
        assert!(is_safe(Some("19123804"), "kubi_kubi_kubi"));
        assert!(is_safe(Some("455311800"), "fr4gm1nt"));
    }

    #[test]
    fn id_gewinnt_gegen_login_bei_namensuebernahme() {
        // Fremde ID, aber der Login eines Safe-Kontos: NICHT geschützt.
        assert!(!is_safe(Some("999999"), "kubi_kubi_kubi"));
    }

    #[test]
    fn faellt_ohne_id_auf_login_zurueck() {
        // Alt-Nachrichten aus dem IRC-Backfill haben keine chatter_id.
        assert!(is_safe(None, "kubi_kubi_kubi"));
        assert!(is_safe(Some(""), "KUBI_KUBI_KUBI"));
        assert!(is_safe(Some("   "), "  Fr4gm1nt  "));
    }

    #[test]
    fn fremde_konten_sind_nicht_safe() {
        assert!(!is_safe(Some("147713656"), "helmbombenricky"));
        assert!(!is_safe(None, "irgendwer"));
        assert!(!is_safe(None, ""));
        assert!(!is_safe(Some(""), ""));
    }

    #[test]
    fn safe_konten_stehen_nicht_in_der_crew_registry() {
        // Ein Konto darf nie gleichzeitig Crew und Safe sein.
        for safe in SAFE_ACCOUNTS {
            assert_eq!(
                crate::crew_guard::screen("harmlos", Some(safe.twitch_user_id)),
                crate::crew_guard::CrewSignal::None,
                "Safe-Konto {} darf kein Crew-Signal erzeugen",
                safe.login
            );
        }
    }
}
