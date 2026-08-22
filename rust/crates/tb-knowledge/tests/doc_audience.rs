//! Regression: die Zielgruppen-Erlaubnisliste gilt fuer alle Ausgaenge der
//! Wissensbasis, nicht nur fuer den HTML-Renderer der Hilfeseite.

use tb_knowledge::{ist_oeffentlich, KnowledgeBase, Namespace};

#[test]
fn erlaubnisliste_kennt_nur_oeffentliche_zielgruppen() {
    assert!(ist_oeffentlich(""));
    assert!(ist_oeffentlich("streamer"));
    assert!(ist_oeffentlich(" public "));
    assert!(!ist_oeffentlich("concierge"));
    assert!(!ist_oeffentlich("intern"));
    // Unbekannte Zielgruppen bleiben drin, nicht draussen.
    assert!(!ist_oeffentlich("was-auch-immer-morgen-dazukommt"));
}

#[test]
fn select_liefert_kein_concierge_doc_mehr() {
    let kb = KnowledgeBase::load_from_dir(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures"),
    )
    .expect("fixtures laden");
    // Das Concierge-Doc matcht „GEHEIMES_INTERNES_WISSEN“ lexikalisch am besten;
    // ohne den Audience-Filter stuende es in der Auswahl.
    let hits = kb.select("GEHEIMES_INTERNES_WISSEN", Namespace::Bot, None, 4);
    assert!(
        !hits.iter().any(|d| d.slug == "nur-concierge"),
        "Concierge-Doc in der Auswahl: {:?}",
        hits.iter().map(|d| d.slug.as_str()).collect::<Vec<_>>()
    );
    let hits = kb.select("auto raid", Namespace::Bot, None, 4);
    assert!(
        hits.iter().any(|d| d.slug == "auto-raid"),
        "Streamer-Docs muessen weiter auffindbar sein"
    );
}
