//! tb-category-collector — Kategoriesammler für die Twitch-Kategorie Deadlock.
//!
//! Sammelt dauerhaft die gesamte Kategorie (weltweit, alle Sprachen) über
//! ausschließlich lesende Helix-Endpunkte (App-Token) und anonyme
//! `justinfan`-Chat-Verbindungen. Kein Senden, kein Bot-Token im Chat, keine
//! Partner-/Bot-Aktionslisten. Konfiguration ausschließlich in Postgres.

pub mod collector;
pub mod config;
pub mod sprache;
pub mod store;

#[cfg(test)]
#[path = "../../../test-support/postgres.rs"]
mod test_postgres;
