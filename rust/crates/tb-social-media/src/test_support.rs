//! Gemeinsame Testhilfen dieses Crates.
//!
//! Nur unter `cfg(test)` uebersetzt und deshalb kein Teil der Auslieferung.

/// Test-DSN, mit Notbremse.
///
/// Ohne `TB_TEST_DATABASE_URL` verlassen die DB-Tests ihren Rumpf und melden
/// gruen, ohne eine einzige Zusicherung geprueft zu haben. Fuer einen Lauf am
/// Arbeitsplatz ist das bequem, fuer einen Lauf, der als Nachweis gilt, ist es
/// eine Luege. `TB_TEST_REQUIRE_DB=1` macht daraus einen Abbruch.
///
/// Steht bewusst hier und nicht in einem der Testmodule: eine Notbremse, die
/// nur in einer von fuenf `make_pool`-Kopien haengt, schuetzt genau die Tests
/// nicht, die noch niemand daran gehaengt hat.
pub(crate) fn test_dsn() -> Option<String> {
    match std::env::var("TB_TEST_DATABASE_URL") {
        Ok(dsn) if !dsn.trim().is_empty() => Some(dsn),
        _ => {
            assert!(
                std::env::var("TB_TEST_REQUIRE_DB").as_deref() != Ok("1"),
                "TB_TEST_REQUIRE_DB=1 gesetzt, aber TB_TEST_DATABASE_URL fehlt: \
                 dieser Test haette nichts geprueft"
            );
            None
        }
    }
}
