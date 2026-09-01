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
//! Ausschließlich **DeepSeek V4 Flash bei Fireworks**. Ohne Fireworks-Schlüssel
//! schlägt der Connector geschlossen fehl. Altanbieter, Provider-Overrides und
//! Modell-Overrides werden nicht verwendet.
//!
//! # Ledger
//!
//! Jeder erfolgreiche Aufruf verbucht die echten Token-Zahlen best-effort ins
//! gemeinsame Usage-Ledger (`source='twitch-bot'`, `purpose=` Name des
//! Anwendungsfalls, falls der Aufrufer keinen eigenen Zweck nennt) — siehe
//! [`ledger`]. Ein DB-Fehler kippt den Aufruf NIE.
//!
//! # Secrets
//!
//! Schlüssel kommen ausschließlich aus der Umgebung (Infisical/systemd) über
//! den konsolidierten Resolver [`keys`] und werden NIE geloggt.

pub mod hub;
pub mod keys;
pub mod ledger;
pub mod selection;

pub use hub::{
    complete, complete_detailed, strip_think, Accept, Ledger, LlmError, LlmFailure, Message,
    Request, Response,
};
pub use selection::{endpoint_chain, endpoint_for, LlmEndpoint};
