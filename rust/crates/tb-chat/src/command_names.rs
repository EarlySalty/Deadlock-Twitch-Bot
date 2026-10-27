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
pub const MAX_EXTRA_NAMES: usize = 5;

pub type ExtraNames = BTreeMap<String, Vec<String>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandNameValidationError {
    Empty,
    TooLong,
    InvalidCharacters,
}

pub fn key(entry: &CommandInfo) -> &'static str {
    // Die gespeicherte Kommandoidentität bleibt bei einem neuen Standardnamen erhalten.
    match entry.name {
        "!dldc" => "discord",
        name => name.strip_prefix('!').unwrap_or(name),
    }
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
/// Erlaubt werden bewusst nur ASCII-Buchstaben und Ziffern.
/// Das hält die Auflösung exakt zu `commands.rs`, das Commands ebenfalls
/// ASCII-case-insensitiv behandelt.
pub fn normalize_custom_name(raw: &str) -> Result<String, CommandNameValidationError> {
    normalize_name(raw, false)
}

/// Liest Namen, die vor der strengeren Eingabeprüfung gespeichert wurden.
/// Neue Änderungen laufen ausschließlich über `normalize_custom_name`.
pub fn normalize_saved_name(raw: &str) -> Result<String, CommandNameValidationError> {
    normalize_name(raw, true)
}

fn normalize_name(raw: &str, allow_legacy: bool) -> Result<String, CommandNameValidationError> {
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
    if !normalized.bytes().all(|byte| {
        byte.is_ascii_alphanumeric() || (allow_legacy && (byte == b'_' || byte == b'-'))
    }) {
        return Err(CommandNameValidationError::InvalidCharacters);
    }
    Ok(format!("!{normalized}"))
}

fn valid_override(overrides: &BTreeMap<String, String>, entry: &CommandInfo) -> Option<String> {
    overrides
        .get(key(entry))
        .and_then(|name| normalize_saved_name(name).ok())
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

fn primary_names(entry: &CommandInfo, overrides: &BTreeMap<String, String>) -> Vec<String> {
    match valid_override(overrides, entry) {
        Some(custom) => vec![custom],
        None => std::iter::once(entry.name)
            .chain(entry.aliases.iter().copied())
            .map(str::to_string)
            .collect(),
    }
}

pub fn extra_names(
    entry: &CommandInfo,
    overrides: &BTreeMap<String, String>,
    extras: &ExtraNames,
) -> Vec<String> {
    let primary = primary_names(entry, overrides);
    let mut names: Vec<String> = Vec::new();
    for name in extras.get(key(entry)).into_iter().flatten() {
        let Ok(name) = normalize_saved_name(name) else {
            continue;
        };
        if !primary.contains(&name) && !names.contains(&name) && names.len() < MAX_EXTRA_NAMES {
            names.push(name);
        }
    }
    names
}

pub fn resolve_command(
    typed: &str,
    overrides: &BTreeMap<String, String>,
    extras: &ExtraNames,
) -> Option<&'static CommandInfo> {
    let typed = typed.to_ascii_lowercase();
    catalog()
        .iter()
        .find(|entry| primary_names(entry, overrides).contains(&typed))
        .or_else(|| {
            catalog()
                .iter()
                .find(|entry| extra_names(entry, overrides, extras).contains(&typed))
        })
}

pub fn conflicting_command(
    command_key: &str,
    candidate: &str,
    overrides: &BTreeMap<String, String>,
    extras: &ExtraNames,
) -> Option<&'static CommandInfo> {
    let candidate = normalize_saved_name(candidate).ok()?;
    catalog().iter().find(|entry| {
        if key(entry) == command_key {
            return false;
        }
        if entry.name == "!dldc" && candidate == "!dldc" {
            return true;
        }
        primary_names(entry, overrides).contains(&candidate)
            || extra_names(entry, overrides, extras).contains(&candidate)
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

    fn none() -> ExtraNames {
        ExtraNames::new()
    }

    #[test]
    fn stat_commands_custom_names_normalisiert_ergonomisch_und_lehnt_unsichere_namen_ab() {
        assert_eq!(
            normalize_custom_name(" DACHRaid7 "),
            Ok("!dachraid7".into())
        );
        assert_eq!(normalize_custom_name("!meinraid"), Ok("!meinraid".into()));
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
        for invalid in ["!!raid", "#raid", "!raid#", "!mein_raid", "!mein-raid"] {
            assert_eq!(
                normalize_custom_name(invalid),
                Err(CommandNameValidationError::InvalidCharacters),
                "{invalid}"
            );
        }
        let too_long = format!("!{}", "a".repeat(MAX_COMMAND_NAME_LEN));
        assert_eq!(
            normalize_custom_name(&too_long),
            Err(CommandNameValidationError::TooLong)
        );
    }

    #[test]
    fn stat_commands_custom_names_ersetzen_standardnamen_und_aliase() {
        let mut overrides = BTreeMap::new();
        let community_command = entry_by_key("discord").unwrap();
        assert_eq!(community_command.name, "!dldc");
        assert_eq!(effective_name(community_command, &overrides), "!dldc");
        assert_eq!(effective_aliases(community_command, &overrides), &["!dlde"]);
        assert!(resolve_command("!discord", &overrides, &none()).is_none());
        assert!(conflicting_command("raid", "!discord", &overrides, &none()).is_none());
        overrides.insert("discord".into(), "!community".into());
        assert_eq!(effective_name(community_command, &overrides), "!community");
        assert!(effective_aliases(community_command, &overrides).is_empty());
        assert_eq!(
            resolve_command("!community", &overrides, &none()).map(|entry| entry.name),
            Some("!dldc")
        );
        for trigger in ["!discord", "!dldc", "!dlde"] {
            assert!(resolve_command(trigger, &overrides, &none()).is_none());
        }
        assert!(conflicting_command("discord", "!community", &overrides, &none()).is_none());
        assert_eq!(
            conflicting_command("raid", "!dldc", &overrides, &none()).map(key),
            Some("discord")
        );
        assert!(conflicting_command("discord", "!dldc", &overrides, &none()).is_none());
        assert_eq!(
            conflicting_command("raid", "!community", &overrides, &none()).map(key),
            Some("discord")
        );
        assert_eq!(
            resolve_command("!raid", &overrides, &none()).map(|c| c.name),
            Some("!raid")
        );
        assert_eq!(
            resolve_command("!traid", &overrides, &none()).map(|c| c.name),
            Some("!raid")
        );

        overrides.insert("raid".into(), "!dachraid".into());
        assert_eq!(
            resolve_command("!dachraid", &overrides, &none()).map(|c| c.name),
            Some("!raid")
        );
        assert!(resolve_command("!raid", &overrides, &none()).is_none());
        assert!(resolve_command("!traid", &overrides, &none()).is_none());
        assert_eq!(
            resolve_command("!ping", &overrides, &none()).map(|c| c.name),
            Some("!ping")
        );
    }

    #[test]
    fn zusatznamen_loesen_auf_und_blockieren_fremde_namen() {
        let mut overrides = BTreeMap::new();
        let mut extras = ExtraNames::new();
        extras.insert(
            "discord".into(),
            vec![
                "!dc".into(),
                "Discord".into(),
                "!dldc".into(),
                "kaputt name".into(),
            ],
        );
        let community = entry_by_key("discord").unwrap();
        assert_eq!(
            extra_names(community, &overrides, &extras),
            ["!dc", "!discord"]
        );
        for trigger in ["!dc", "!discord", "!dldc", "!dlde"] {
            assert_eq!(
                resolve_command(trigger, &overrides, &extras).map(key),
                Some("discord"),
                "{trigger}"
            );
        }
        assert_eq!(
            conflicting_command("raid", "!dc", &overrides, &extras).map(key),
            Some("discord")
        );
        assert!(conflicting_command("discord", "!dc", &overrides, &extras).is_none());
        overrides.insert("ping".into(), "!dc".into());
        assert_eq!(
            resolve_command("!dc", &overrides, &extras).map(key),
            Some("ping")
        );
    }

    #[test]
    fn stat_commands_custom_names_kaputter_db_override_deaktiviert_keinen_standardbefehl() {
        let mut overrides = BTreeMap::new();
        overrides.insert("raid".into(), "kaputt name".into());
        assert_eq!(
            resolve_command("!raid", &overrides, &none()).map(|c| c.name),
            Some("!raid")
        );
        assert_eq!(
            resolve_command("!traid", &overrides, &none()).map(|c| c.name),
            Some("!raid")
        );
    }

    #[test]
    fn alte_namen_mit_unterstrich_oder_bindestrich_bleiben_aktiv() {
        let mut overrides = BTreeMap::new();
        overrides.insert("raid".into(), "!mein_raid".into());
        assert_eq!(normalize_saved_name("!mein_raid"), Ok("!mein_raid".into()));
        assert_eq!(
            normalize_custom_name("!mein_raid"),
            Err(CommandNameValidationError::InvalidCharacters)
        );
        assert_eq!(
            resolve_command("!mein_raid", &overrides, &none()).map(|c| c.name),
            Some("!raid")
        );
        assert!(resolve_command("!raid", &overrides, &none()).is_none());
        overrides.insert("raid".into(), "!mein-raid".into());
        assert_eq!(
            resolve_command("!mein-raid", &overrides, &none()).map(|c| c.name),
            Some("!raid")
        );
    }

    #[test]
    fn stat_commands_custom_names_kollisionspruefung_beruecksichtigt_effektive_namen() {
        let mut overrides = BTreeMap::new();
        assert_eq!(
            conflicting_command("raid", "!ping", &overrides, &none()).map(|c| c.name),
            Some("!ping")
        );
        overrides.insert("ping".into(), "!pong".into());
        assert!(conflicting_command("raid", "!ping", &overrides, &none()).is_none());
        overrides.insert("wins".into(), "!siege".into());
        assert_eq!(
            conflicting_command("raid", "!siege", &overrides, &none()).map(|c| c.name),
            Some("!wins")
        );
    }

    #[test]
    fn reset_eines_katalogbefehls_mit_unterstrich_erkennt_legacy_kollision() {
        let mut overrides = BTreeMap::new();
        overrides.insert("connect".into(), "!raid_history".into());
        overrides.insert("raid_history".into(), "!meinehistorie".into());
        assert_eq!(
            conflicting_command("raid_history", "!raid_history", &overrides, &none())
                .map(|c| c.name),
            Some("!connect")
        );
    }
}
