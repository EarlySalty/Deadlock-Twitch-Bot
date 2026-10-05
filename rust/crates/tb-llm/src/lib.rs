//! Die LLM-Schicht des Twitch-Bots: ein Eingang, dahinter alles.
//!
//! [`complete`] ist die einzige Stelle im ganzen Repo, die HTTP gegen ein
//! Sprachmodell spricht. Aufrufer nennen ihren Anwendungsfall und schicken
//! einen [`Request`]; Zeitgrenze, Wiederholung bei
//! 429, Verbuchung im Ledger und die Einordnung des Fehlers passieren hier.
//!
//! ```ignore
//! let antwort = tb_llm::complete(
//!     "title_ai",
//!     tb_llm::Request::prompt(prompt).temperature(0.35).max_tokens(2000),
//! )
//! .await?;
//! ```
//!
//! # Anbieter
//!
//! Fireworks verwendet die neueste geprüfte **DeepSeek-Flash-Version** gemäß
//! der zentralen lokalen Auswahl. Auch `title_ai` nutzt diese Familie.
//! Ohne Schlüssel oder geprüftes Modell schlägt der
//! Connector geschlossen fehl. Modell-ENV-Overrides werden nicht verwendet.
//!
//! # Ledger
//!
//! Jeder HTTP-Versuch steht vor dem Senden im Postgres-Ledger mit Projekt,
//! Dienst und Zweck. Fehlende Tokenzahlen bleiben unbekannt. Ohne gespeicherten
//! Start findet kein kostenpflichtiger Aufruf statt. Abschlüsse werden bei
//! Schreibfehlern über das strukturierte Journal nachgeliefert.
//!
//! # Secrets
//!
//! Schlüssel kommen ausschließlich aus der Umgebung (Infisical/systemd) über
//! den konsolidierten Resolver [`keys`] und werden NIE geloggt.

pub mod daily_model_resolver;
pub mod hub;
pub mod keys;
pub mod ledger;
pub mod model_resolver;
pub mod selection;

#[cfg(feature = "local-eval")]
pub mod local_eval;

pub use hub::{
    complete, complete_detailed, strip_think, Accept, Ledger, LlmError, LlmFailure, Message,
    Request, Response,
};
pub use selection::{endpoint_chain, endpoint_for, LlmEndpoint};
