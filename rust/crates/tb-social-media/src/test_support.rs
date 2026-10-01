#[path = "../../../test-support/database.rs"]
mod test_database;

pub(crate) fn test_dsn() -> Option<String> {
    match test_database::database_url() {
        Some(dsn) if !dsn.trim().is_empty() => Some(dsn),
        _ => {
            assert!(
                !test_database::required(),
                "Die erforderliche isolierte Testdatenbank ist nicht konfiguriert"
            );
            None
        }
    }
}
