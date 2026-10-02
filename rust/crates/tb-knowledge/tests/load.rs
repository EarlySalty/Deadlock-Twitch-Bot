use std::path::Path;

use tb_knowledge::{KnowledgeBase, Namespace};

fn fixtures() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

#[test]
fn laedt_beide_namespaces() {
    let kb = KnowledgeBase::load_from_dir(&fixtures()).expect("lädt");
    // Auch interne und nicht freigegebene Dokumente laden. Erst die
    // Zugriffsprüfung entscheidet, was die Nutzerhilfe verwenden darf.
    assert_eq!(kb.len(), 6);
    let bot = kb
        .docs()
        .iter()
        .filter(|d| d.namespace == Namespace::Bot)
        .count();
    let dl = kb
        .docs()
        .iter()
        .filter(|d| d.namespace == Namespace::Deadlock)
        .count();
    assert_eq!(bot, 5);
    assert_eq!(dl, 1);
}

#[test]
fn slug_kommt_aus_dateiname() {
    let kb = KnowledgeBase::load_from_dir(&fixtures()).unwrap();
    assert!(kb
        .docs()
        .iter()
        .any(|d| d.slug == "auto-raid" && d.title == "Auto-Raid"));
}

#[test]
fn fehlendes_verzeichnis_ist_leer_kein_fehler() {
    let kb = KnowledgeBase::load_from_dir(Path::new("/does/not/exist")).unwrap();
    assert!(kb.is_empty());
}
