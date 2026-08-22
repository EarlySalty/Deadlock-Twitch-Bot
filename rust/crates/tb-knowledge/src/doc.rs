//! Ein Wissens-Dokument: kontrolliertes Frontmatter + Markdown-Body.

/// Darf ein Doc an eine ungeschuetzte Oberflaeche?
///
/// Bewusst eine Erlaubnisliste. Ein Doc ohne `audience` ist Streamer-Wissen und
/// darf raus; alles mit eigener Zielgruppe (`concierge`, `intern`, was noch
/// kommt) bleibt drin, ohne dass jemand daran denken muss. Der Fehlerfall waere
/// sonst ein Leak: `uplink-concierge.md` zaehlt zum Beispiel auf, welche
/// Admin-Funktionen, Freischalt-Wege und Lastgrenzen es gibt, und seine
/// Body-Zeilen sind Anweisungen an einen Agenten, keine Nutzer-Antworten.
///
/// `viewer` gehoert dazu: der Deadlock-Namespace nutzt es fuer Wissen, das im
/// Chat an jeden geht.
pub fn ist_oeffentlich(audience: &str) -> bool {
    matches!(audience.trim(), "" | "streamer" | "public" | "viewer")
}

use std::str::FromStr;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum KnowledgeError {
    #[error("doc '{0}': kein Frontmatter-Block (--- … ---) am Dateianfang")]
    MissingFrontmatter(String),
    #[error("doc '{slug}': Pflichtfeld '{field}' fehlt")]
    MissingField { slug: String, field: &'static str },
    #[error("unbekannter namespace: '{0}' (erlaubt: bot|deadlock)")]
    BadNamespace(String),
    #[error("doc '{slug}': Feld '{field}' ungültig: '{value}'")]
    BadField {
        slug: String,
        field: &'static str,
        value: String,
    },
    #[error("io für '{path}': {msg}")]
    Io { path: String, msg: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Namespace {
    Bot,
    Deadlock,
}

impl Namespace {
    pub fn as_str(&self) -> &'static str {
        match self {
            Namespace::Bot => "bot",
            Namespace::Deadlock => "deadlock",
        }
    }
}

impl FromStr for Namespace {
    type Err = KnowledgeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim() {
            "bot" => Ok(Namespace::Bot),
            "deadlock" => Ok(Namespace::Deadlock),
            other => Err(KnowledgeError::BadNamespace(other.to_string())),
        }
    }
}

#[derive(Debug, Clone)]
pub struct KnowledgeDoc {
    pub slug: String,
    pub title: String,
    pub namespace: Namespace,
    pub category: String,
    pub audience: String,
    pub last_updated: String,
    pub source: String,
    pub tip_eligible: bool,
    pub tip_text: String,
    pub tip_flags: Vec<String>,
    pub time_to_value: u8,
    pub body: String,
}

/// Frontmatter-Format (kontrolliertes Eigenformat, KEIN allgemeines YAML):
/// Datei beginnt mit einer Zeile `---`, dann `key: value`-Zeilen, dann eine
/// Zeile `---`, danach der Markdown-Body. `tip_flags` als `[a, b]` oder leer `[]`.
pub fn parse_doc(raw: &str, slug: &str) -> Result<KnowledgeDoc, KnowledgeError> {
    let normalized = raw.replace("\r\n", "\n");
    let rest = normalized
        .strip_prefix("---\n")
        .ok_or_else(|| KnowledgeError::MissingFrontmatter(slug.to_string()))?;
    let end = rest
        .find("\n---\n")
        .or_else(|| rest.strip_suffix("\n---").map(|_| rest.len() - 4))
        .ok_or_else(|| KnowledgeError::MissingFrontmatter(slug.to_string()))?;
    let (front, after) = rest.split_at(end);
    let body = after.strip_prefix("\n---\n").unwrap_or("").to_string();

    let mut title = None;
    let mut namespace = None;
    let mut category = String::new();
    let mut audience = String::new();
    let mut last_updated = String::new();
    let mut source = String::new();
    let mut tip_eligible = false;
    let mut tip_text = String::new();
    let mut tip_flags: Vec<String> = Vec::new();
    let mut time_to_value: u8 = 3;

    for line in front.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        match key {
            "title" => title = Some(value.to_string()),
            "namespace" => namespace = Some(value.parse::<Namespace>()?),
            "category" => category = value.to_string(),
            "audience" => audience = value.to_string(),
            "last_updated" => last_updated = value.to_string(),
            "source" => source = value.to_string(),
            "tip_eligible" => tip_eligible = matches!(value, "true" | "yes" | "1"),
            "tip_text" => tip_text = value.to_string(),
            "tip_flags" => tip_flags = parse_flags(value),
            "time_to_value" => {
                time_to_value = value.parse::<u8>().map_err(|_| KnowledgeError::BadField {
                    slug: slug.to_string(),
                    field: "time_to_value",
                    value: value.to_string(),
                })?
            }
            _ => {}
        }
    }

    Ok(KnowledgeDoc {
        slug: slug.to_string(),
        title: title.ok_or(KnowledgeError::MissingField {
            slug: slug.to_string(),
            field: "title",
        })?,
        namespace: namespace.ok_or(KnowledgeError::MissingField {
            slug: slug.to_string(),
            field: "namespace",
        })?,
        category,
        audience,
        last_updated,
        source,
        tip_eligible,
        tip_text,
        tip_flags,
        time_to_value,
        body,
    })
}

/// `[a, b, c]` / `a, b, c` / `[]` → `["a","b","c"]` / `[]`.
fn parse_flags(value: &str) -> Vec<String> {
    value
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "---\n\
title: Auto-Raid\n\
namespace: bot\n\
category: feature\n\
audience: streamer\n\
last_updated: 2026-06-21\n\
source: manual\n\
tip_eligible: true\n\
tip_flags: [feature, costream]\n\
time_to_value: 2\n\
---\n\
Der Bot raidet Zuschauer weiter.\n";

    #[test]
    fn parst_frontmatter_und_body() {
        let d = parse_doc(SAMPLE, "auto-raid").expect("parst");
        assert_eq!(d.slug, "auto-raid");
        assert_eq!(d.title, "Auto-Raid");
        assert_eq!(d.namespace, Namespace::Bot);
        assert_eq!(d.category, "feature");
        assert_eq!(d.audience, "streamer");
        assert!(d.tip_eligible);
        assert_eq!(d.tip_text, "");
        assert_eq!(
            d.tip_flags,
            vec!["feature".to_string(), "costream".to_string()]
        );
        assert_eq!(d.time_to_value, 2);
        assert_eq!(d.body.trim(), "Der Bot raidet Zuschauer weiter.");
    }

    #[test]
    fn fehlendes_frontmatter_ist_fehler() {
        let err = parse_doc("kein frontmatter hier", "x").unwrap_err();
        assert!(matches!(err, KnowledgeError::MissingFrontmatter(_)));
    }

    #[test]
    fn fehlender_title_ist_fehler() {
        let raw = "---\nnamespace: bot\n---\nbody";
        let err = parse_doc(raw, "x").unwrap_err();
        assert!(matches!(
            err,
            KnowledgeError::MissingField { field: "title", .. }
        ));
    }

    #[test]
    fn unbekannter_namespace_ist_fehler() {
        let raw = "---\ntitle: T\nnamespace: foo\n---\nbody";
        let err = parse_doc(raw, "x").unwrap_err();
        assert!(matches!(err, KnowledgeError::BadNamespace(_)));
    }

    #[test]
    fn defaults_wenn_optionale_felder_fehlen() {
        let raw = "---\ntitle: T\nnamespace: deadlock\n---\ninhalt";
        let d = parse_doc(raw, "t").unwrap();
        assert_eq!(d.namespace, Namespace::Deadlock);
        assert_eq!(d.category, "");
        assert!(!d.tip_eligible);
        assert_eq!(d.tip_text, "");
        assert!(d.tip_flags.is_empty());
        assert_eq!(d.time_to_value, 3);
    }

    #[test]
    fn parst_tip_text() {
        let raw = "---\ntitle: Auto-Raid\nnamespace: bot\ntip_eligible: true\ntip_text: Du gehst offline? Der Bot raidet deine Zuschauer automatisch weiter.\n---\nbody";
        let d = parse_doc(raw, "auto-raid").unwrap();
        assert_eq!(
            d.tip_text,
            "Du gehst offline? Der Bot raidet deine Zuschauer automatisch weiter."
        );
    }
}
