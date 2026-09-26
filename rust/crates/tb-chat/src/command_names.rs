//! Kanalbezogene Namen für alle sichtbaren Chat-Befehle.
//!
//! Ohne Override gelten der kanonische Name und seine eingebauten Kurzformen.
//! Sobald ein Streamer einen eigenen Namen setzt, ersetzt dieser Name für
//! seinen Kanal den gesamten Standard-Trigger-Satz dieses Befehls. So kann
//! z. B. ein bereits vorhandenes `!raid` frei bleiben, während der
//! Deadlock-Bot nur noch auf `!dachraid` reagiert.

use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::catalog::{catalog, CommandInfo};

pub const MAX_COMMAND_NAME_LEN: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandNameValidationError {
    Empty,
    TooLong,
    InvalidCharacters,
}

pub fn key(entry: &CommandInfo) -> &'static str {
    entry.name.strip_prefix('!').unwrap_or(entry.name)
}

pub fn entry_by_key(command_key: &str) -> Option<&'static CommandInfo> {
    let normalized = command_key
        .trim()
        .trim_start_matches('!')
        .to_ascii_lowercase();
    catalog()
        .iter()
        .find(|entry| key(entry) == normalized.as_str())
}

/// Normalisiert Nutzereingaben auf `!name`.
///
/// Erlaubt werden bewusst nur ASCII-Buchstaben, Ziffern, `_` und `-`.
/// Das hält die Auflösung exakt zu `commands.rs`, das Commands ebenfalls
/// ASCII-case-insensitiv behandelt.
pub fn normalize_custom_name(raw: &str) -> Result<String, CommandNameValidationError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(CommandNameValidationError::Empty);
    }
    let without_bang = trimmed.strip_prefix('!').unwrap_or(trimmed);
    if without_bang.is_empty() {
        return Err(CommandNameValidationError::Empty);
    }
    let normalized = without_bang.to_ascii_lowercase();
    let full_len = normalized.len() + 1;
    if full_len > MAX_COMMAND_NAME_LEN {
        return Err(CommandNameValidationError::TooLong);
    }
    if !normalized
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        return Err(CommandNameValidationError::InvalidCharacters);
    }
    Ok(format!("!{normalized}"))
}

fn valid_override(overrides: &BTreeMap<String, String>, entry: &CommandInfo) -> Option<String> {
    overrides
        .get(key(entry))
        .and_then(|name| normalize_custom_name(name).ok())
}

/// Liefert den tatsächlich aktiven Hauptnamen eines Befehls.
pub fn effective_name(entry: &CommandInfo, overrides: &BTreeMap<String, String>) -> String {
    valid_override(overrides, entry).unwrap_or_else(|| entry.name.to_string())
}

/// Standard-Aliase gelten nur solange kein eigener Name gesetzt ist.
pub fn effective_aliases(
    entry: &CommandInfo,
    overrides: &BTreeMap<String, String>,
) -> &'static [&'static str] {
    if valid_override(overrides, entry).is_some() {
        &[]
    } else {
        entry.aliases
    }
}

/// Löst den im Chat geschriebenen Trigger auf den kanonischen Katalogeintrag
/// auf. Ein eigener Name ersetzt kanonischen Namen + Standard-Aliase nur für
/// genau diesen Kanal.
pub fn resolve_command(
    typed: &str,
    overrides: &BTreeMap<String, String>,
) -> Option<&'static CommandInfo> {
    let typed = typed.to_ascii_lowercase();
    catalog().iter().find(|entry| {
        if let Some(custom) = valid_override(overrides, entry) {
            typed == custom
        } else {
            typed == entry.name || entry.aliases.contains(&typed.as_str())
        }
    })
}

/// Prüft, ob ein neuer eigener Name mit einem aktuell aktiven Namen eines
/// anderen Befehls kollidiert. Der eigene bisherige Eintrag wird ignoriert.
pub fn conflicting_command(
    command_key: &str,
    candidate: &str,
    overrides: &BTreeMap<String, String>,
) -> Option<&'static CommandInfo> {
    let candidate = normalize_custom_name(candidate).ok()?;
    catalog().iter().find(|entry| {
        if key(entry) == command_key {
            return false;
        }
        if let Some(custom) = valid_override(overrides, entry) {
            candidate == custom
        } else {
            candidate == entry.name || entry.aliases.contains(&candidate.as_str())
        }
    })
}

/// DB-Lesefehler beim Command-Namen-Lookup werden gedrosselt. Bei Fehlern
/// fällt der Command-Dispatcher auf die globalen Standardnamen zurück.
pub(crate) fn warn_read_failure(error: &sqlx::Error) {
    static WARNINGS: OnceLock<Mutex<(Option<Instant>, u64)>> = OnceLock::new();
    let mut window = WARNINGS
        .get_or_init(|| Mutex::new((None, 0)))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let now = Instant::now();
    if window
        .0
        .is_some_and(|last| now.duration_since(last) < Duration::from_secs(4 * 24 * 60 * 60))
    {
        window.1 = window.1.saturating_add(1);
        return;
    }
    tracing::warn!(
        %error,
        wiederholungen = window.1,
        "Eigene Command-Namen konnten nicht gelesen werden; Standardnamen bleiben aktiv"
    );
    *window = (Some(now), 0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stat_commands_custom_names_normalisiert_ergonomisch_und_lehnt_unsichere_namen_ab() {
        assert_eq!(
            normalize_custom_name(" DACH_Raid "),
            Ok("!dach_raid".into())
        );
        assert_eq!(normalize_custom_name("!mein-raid"), Ok("!mein-raid".into()));
        assert_eq!(
            normalize_custom_name(""),
            Err(CommandNameValidationError::Empty)
        );
        assert_eq!(
            normalize_custom_name("!"),
            Err(CommandNameValidationError::Empty)
        );
        assert_eq!(
            normalize_custom_name("!raid bitte"),
            Err(CommandNameValidationError::InvalidCharacters)
        );
        assert_eq!(
            normalize_custom_name("!räid"),
            Err(CommandNameValidationError::InvalidCharacters)
        );
        let too_long = format!("!{}", "a".repeat(MAX_COMMAND_NAME_LEN));
        assert_eq!(
            normalize_custom_name(&too_long),
            Err(CommandNameValidationError::TooLong)
        );
    }

    #[test]
    fn stat_commands_custom_names_ersetzen_standardnamen_und_aliase() {
        let mut overrides = BTreeMap::new();
        assert_eq!(
            resolve_command("!raid", &overrides).map(|c| c.name),
            Some("!raid")
        );
        assert_eq!(
            resolve_command("!traid", &overrides).map(|c| c.name),
            Some("!raid")
        );

        overrides.insert("raid".into(), "!dachraid".into());
        assert_eq!(
            resolve_command("!dachraid", &overrides).map(|c| c.name),
            Some("!raid")
        );
        assert!(resolve_command("!raid", &overrides).is_none());
        assert!(resolve_command("!traid", &overrides).is_none());
        assert_eq!(
            resolve_command("!ping", &overrides).map(|c| c.name),
            Some("!ping")
        );
    }

    #[test]
    fn stat_commands_custom_names_kaputter_db_override_deaktiviert_keinen_standardbefehl() {
        let mut overrides = BTreeMap::new();
        overrides.insert("raid".into(), "kaputt name".into());
        assert_eq!(
            resolve_command("!raid", &overrides).map(|c| c.name),
            Some("!raid")
        );
        assert_eq!(
            resolve_command("!traid", &overrides).map(|c| c.name),
            Some("!raid")
        );
    }

    #[test]
    fn stat_commands_custom_names_kollisionspruefung_beruecksichtigt_effektive_namen() {
        let mut overrides = BTreeMap::new();
        assert_eq!(
            conflicting_command("raid", "!ping", &overrides).map(|c| c.name),
            Some("!ping")
        );
        overrides.insert("ping".into(), "!pong".into());
        assert!(conflicting_command("raid", "!ping", &overrides).is_none());
        overrides.insert("wins".into(), "!siege".into());
        assert_eq!(
            conflicting_command("raid", "!siege", &overrides).map(|c| c.name),
            Some("!wins")
        );
    }
}
