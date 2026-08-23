//! Guard gegen Migrations-Versionskollisionen.
//!
//! sqlx identifiziert Migrationen allein ueber die Versionsnummer und bettet
//! zwei gleichversionierte Dateien kommentarlos beide ein. Zur Laufzeit endet
//! das in einer Restart-Schleife ("previously applied but has been modified"
//! bzw. duplicate key), wie am 2026-08-15 live passiert, als zwei Branches
//! unabhaengig die Version 20260815120000 vergeben hatten. Dieser Test macht
//! jeden Build mit einer Kollision sofort rot, ohne Datenbank.

#[test]
fn migrationsversionen_sind_eindeutig_und_sortiert() {
    let migrations = &tb_db::MIGRATOR.migrations;
    assert!(!migrations.is_empty(), "Migrator ist leer, Embed kaputt?");
    for fenster in migrations.windows(2) {
        let (a, b) = (&fenster[0], &fenster[1]);
        assert!(
            a.version < b.version,
            "Migrations-Versionskollision oder falsche Reihenfolge: \
             {} ({}) vs {} ({}) — Versionsnummer muss strikt eindeutig sein",
            a.version,
            a.description,
            b.version,
            b.description,
        );
    }
}
