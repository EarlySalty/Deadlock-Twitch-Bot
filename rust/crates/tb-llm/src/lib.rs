//! Die LLM-Schicht des Twitch-Bots: ein Eingang, dahinter alles.
//!
//! [`complete`] ist die einzige Stelle im ganzen Repo, die HTTP gegen ein
//! Sprachmodell spricht. Aufrufer nennen ihren Anwendungsfall und schicken
//! einen [`Request`]; Anbieterwahl, Ausweichkette, Zeitgrenze, Wiederholung bei
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
//! Drei, alle in [`selection`] konstant gehalten:
//! - **Fireworks/DeepSeek** — Standard, solange ein Fireworks-Key gesetzt ist.
//! - **MiniMax** — Rückfall ohne Fireworks-Key und Ausweichweg der Kette.
//! - **Anthropic** — nur für die Anwendungsfälle in
//!   [`selection::ANTHROPIC_USE_CASES`] (Dashboard-KI, Opus-Report,
//!   Clip-Anreicherung).
//!
//! **OpenAI ist raus** — diese Crate enthält keinen OpenAI-Client, keine
//! OpenAI-Konstante und keinen OpenAI-Pfad (Querschnitts-Direktive 2 des
//! Grillme-Audits). Neue Anbieter kommen nicht dazu, ohne dass diese Direktive
//! ausdrücklich aufgehoben wird.
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
    complete, extract_anthropic_text, strip_think, Accept, Ledger, LlmError, Message, Request,
    Response,
};
pub use selection::{endpoint_chain, endpoint_for, LlmEndpoint};
