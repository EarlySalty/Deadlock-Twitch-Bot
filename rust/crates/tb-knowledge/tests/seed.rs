//! Verifiziert, dass die PRODUKTIVE Wissensbasis (rust/knowledge) lädt und
//! die Kernfragen die erwarteten Dokumente selektieren.

use std::path::Path;

use tb_knowledge::{KnowledgeBase, Namespace};

fn knowledge_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../knowledge")
}

#[test]
fn produktive_basis_laedt() {
    let kb = KnowledgeBase::load_from_dir(&knowledge_root()).expect("knowledge lädt fehlerfrei");
    assert!(
        kb.len() >= 16,
        "mindestens 6 bot-Docs + 9 FAQ-Docs + 1 deadlock-Platzhalter"
    );
}

#[test]
fn keine_internen_mechanismen_in_hilfe_tipps_oder_grounding() {
    let kb =
        KnowledgeBase::load_from_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures"))
            .unwrap();
    for audience in [None, Some("streamer"), Some("intern"), Some("concierge")] {
        let hits = kb.select("Raid Score Mechanik", Namespace::Bot, audience, 100);
        assert!(hits
            .iter()
            .all(|doc| tb_knowledge::ist_oeffentlich(&doc.audience)));
    }
    assert!(kb
        .eligible_tips()
        .iter()
        .all(|doc| tb_knowledge::ist_oeffentlich(&doc.audience)));
    let all: Vec<_> = kb.docs().iter().collect();
    let grounding = tb_knowledge::assemble_grounding(&all);
    assert!(!grounding.facts.contains("NICHT_FREIGEGEBENE"));
    assert!(!grounding.facts.contains("INTERNER_STREAMER"));
    assert!(!grounding.facts.contains("GEHEIMES_INTERNES_WISSEN"));
    assert!(!grounding
        .sources
        .iter()
        .any(|title| title.contains("Interne")));
}

#[test]
fn raid_frage_findet_auto_raid() {
    let kb = KnowledgeBase::load_from_dir(&knowledge_root()).unwrap();
    let hits = kb.select(
        "Auto-Raid Zuschauer Deadlock-Streamer offline",
        Namespace::Bot,
        None,
        3,
    );
    assert!(hits.iter().any(|d| d.slug == "auto-raid"));
}

#[test]
fn einrichtungs_frage_findet_setup() {
    let kb = KnowledgeBase::load_from_dir(&knowledge_root()).unwrap();
    let hits = kb.select(
        "Einrichtung Twitch-Konto verbinden speichern manuell einstellen",
        Namespace::Bot,
        None,
        3,
    );
    assert!(hits.iter().any(|d| d.slug == "einrichtung"));
}

#[test]
fn faq_frage_findet_migrierte_faq() {
    let kb = KnowledgeBase::load_from_dir(&knowledge_root()).unwrap();
    let hits = kb.select(
        "Kostet Deutsche Deadlock Community etwas kostenlos Abo",
        Namespace::Bot,
        None,
        5,
    );
    assert!(hits.iter().any(|d| d.slug == "faq-einstieg"));
}

#[test]
fn stoerung_stream_info_felder_findet_uplink_stoerungen() {
    let kb = KnowledgeBase::load_from_dir(&knowledge_root()).unwrap();
    let hits = kb.select(
        "Live-Benachrichtigung Zuschauer Wiederholen Stream-Infos OBS Fenster fehlt",
        Namespace::Bot,
        None,
        5,
    );
    assert!(hits.iter().any(|d| d.slug == "uplink-stoerungen"));
}
