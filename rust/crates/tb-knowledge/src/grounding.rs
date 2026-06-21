//! Baut aus selektierten Dokumenten den Grounding-Block + die Pflicht-Quellen.

use crate::doc::KnowledgeDoc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grounding {
    pub facts: String,
    pub sources: Vec<String>,
}

pub fn assemble_grounding(docs: &[&KnowledgeDoc]) -> Grounding {
    let mut facts = String::new();
    let mut sources: Vec<String> = Vec::new();
    for d in docs {
        if !facts.is_empty() {
            facts.push_str("\n\n");
        }
        facts.push_str("## ");
        facts.push_str(&d.title);
        facts.push('\n');
        facts.push_str(d.body.trim());
        if !sources.contains(&d.title) {
            sources.push(d.title.clone());
        }
    }
    Grounding { facts, sources }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::parse_doc;

    #[test]
    fn baut_fakten_und_quellen() {
        let a = parse_doc(
            "---\ntitle: Auto-Raid\nnamespace: bot\n---\nRaidet weiter.",
            "auto-raid",
        )
        .unwrap();
        let b = parse_doc(
            "---\ntitle: Einrichtung\nnamespace: bot\n---\nTwitch verbinden.",
            "einrichtung",
        )
        .unwrap();
        let g = assemble_grounding(&[&a, &b]);
        assert!(g.facts.contains("## Auto-Raid"));
        assert!(g.facts.contains("Raidet weiter."));
        assert!(g.facts.contains("## Einrichtung"));
        assert_eq!(
            g.sources,
            vec!["Auto-Raid".to_string(), "Einrichtung".to_string()]
        );
    }

    #[test]
    fn leere_auswahl_leeres_grounding() {
        let g = assemble_grounding(&[]);
        assert!(g.facts.is_empty());
        assert!(g.sources.is_empty());
    }
}
