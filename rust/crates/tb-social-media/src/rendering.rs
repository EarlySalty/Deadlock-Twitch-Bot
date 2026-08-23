//! HTML-Template-Rendering für das Social-Media-Dashboard + Legal-Seiten (Port
//! von `bot/social_media/rendering.py`).
//!
//! Simple `__KEY__`-Substitution. Die Templates (dashboard/terms/privacy) sind
//! statisch und werden via `include_str!` in die Binary eingebettet — kein
//! Runtime-Pfad nötig (Python liest sie aus `templates/`).

const TERMS_HTML: &str = include_str!("../templates/terms.html");
const PRIVACY_HTML: &str = include_str!("../templates/privacy.html");

/// Ersetzt `__KEY__`-Platzhalter (Key großgeschrieben) durch die Werte
/// (Python `_apply_substitutions`).
pub fn apply_substitutions(template: &str, substitutions: &[(&str, &str)]) -> String {
    let mut rendered = template.to_string();
    for (key, value) in substitutions {
        rendered = rendered.replace(&format!("__{}__", key.to_uppercase()), value);
    }
    rendered
}

/// Rendert ein Template: BOM + führende Leerzeilen entfernen, dann substituieren
/// (Python `render_social_media_template`).
fn render(template: &str, substitutions: &[(&str, &str)]) -> String {
    let trimmed = template.trim_start_matches(['\u{feff}', '\n']);
    apply_substitutions(trimmed, substitutions)
}


/// Rendert die Terms-Seite.
pub fn render_terms() -> String {
    render(TERMS_HTML, &[])
}

/// Rendert die Privacy-Seite.
pub fn render_privacy() -> String {
    render(PRIVACY_HTML, &[])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substitution_ersetzt_grossgeschriebene_keys() {
        let t = "Hallo __NAME__, Daten: __PAYLOAD__ (__NAME__).";
        let out = apply_substitutions(t, &[("name", "Nani"), ("payload", "{}")]);
        assert_eq!(out, "Hallo Nani, Daten: {} (Nani).");
        // Unbekannte Platzhalter bleiben stehen.
        assert!(apply_substitutions("__FEHLT__", &[]).contains("__FEHLT__"));
    }

    #[test]
    fn render_strippt_bom_und_leerzeilen() {
        let out = render("\u{feff}\n\nInhalt __X__", &[("x", "Y")]);
        assert_eq!(out, "Inhalt Y");
    }

    #[test]
    fn terms_und_privacy_rendern() {
        assert!(render_terms().len() > 100);
        assert!(render_privacy().len() > 100);
    }
}
