//! Geteilte Body-Koaleszenz-Helfer der internen API-Handler.
//!
//! Mehrere Python-Routen (`global_ban.py`, `raid.py`-Blacklist) nutzen
//! identische Muster — hier einmal zentral, damit die Parität nicht pro
//! Handler driftet:
//! - `body.get("login") or body.get("twitch_login")` → [`pick_first_truthy`]
//! - `str(body.get("reason") or DEFAULT).strip() or DEFAULT` → [`resolve_reason`]

/// Default-Begründung für Bann-Einträge (Python: `"manual_ban:absolut"` in
/// `global_ban.py` und `raid.py`).
pub const DEFAULT_REASON: &str = "manual_ban:absolut";

/// Python: `body.get(a) or body.get(b)` — `a` gewinnt, wenn truthy.
///
/// Truthy heißt in Python: nicht-leerer String — Whitespace-only zählt als
/// truthy (die Normalisierung dahinter weist ihn dann ab). Deshalb hier
/// bewusst `is_empty()` statt `trim().is_empty()`.
pub fn pick_first_truthy(primary: Option<String>, fallback: Option<String>) -> String {
    let primary = primary.unwrap_or_default();
    if primary.is_empty() {
        fallback.unwrap_or_default()
    } else {
        primary
    }
}

/// `reason` defaulten: fehlend/leer → Default, sonst getrimmt; leer nach Trim
/// → Default (Python: `str(body.get("reason") or DEFAULT).strip() or DEFAULT`).
pub fn resolve_reason(reason: Option<String>) -> String {
    let reason = reason.unwrap_or_default();
    let trimmed = reason.trim();
    if trimmed.is_empty() {
        DEFAULT_REASON.to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_primary_gewinnt_wenn_nicht_leer() {
        assert_eq!(pick_first_truthy(Some("a".into()), Some("b".into())), "a");
    }

    #[test]
    fn pick_fallback_bei_leerem_primary() {
        assert_eq!(
            pick_first_truthy(Some(String::new()), Some("b".into())),
            "b"
        );
        assert_eq!(pick_first_truthy(None, Some("b".into())), "b");
    }

    #[test]
    fn pick_whitespace_primary_ist_truthy_wie_in_python() {
        // Python: " " or "b" → " " — die Normalisierung weist ihn danach ab.
        assert_eq!(pick_first_truthy(Some(" ".into()), Some("b".into())), " ");
    }

    #[test]
    fn reason_default_bei_fehlend_leer_oder_whitespace() {
        assert_eq!(resolve_reason(None), DEFAULT_REASON);
        assert_eq!(resolve_reason(Some(String::new())), DEFAULT_REASON);
        assert_eq!(resolve_reason(Some("   ".into())), DEFAULT_REASON);
    }

    #[test]
    fn reason_wird_getrimmt() {
        assert_eq!(resolve_reason(Some("  Spam  ".into())), "Spam");
    }
}
