//! Gemeinsame LLM-Client-Schicht des Twitch-Bots (Phase-0-Foundation).
//!
//! Genau ZWEI Provider hinter einem gemeinsamen Port ([`provider::LlmProvider`]):
//! - [`minimax::MiniMaxClient`] — **Primär** („alles über MiniMax betreiben").
//! - [`anthropic::AnthropicClient`] — **Premium/`ai_full`** (Opus).
//!
//! **OpenAI ist raus** — diese Crate enthält keinen OpenAI-Client und keine
//! OpenAI-Pfade (Querschnitts-Direktive 2 des Grillme-Audits).
//!
//! Jeder erfolgreiche Completion-Call verbucht die echten Token-Zahlen
//! best-effort ins gemeinsame MiniMax-Usage-Ledger (geteiltes SQLite,
//! `source='twitch-bot'`, `purpose=…`) — siehe [`ledger`]. DB-Fehler kippen den
//! Call NIE.
//!
//! Secrets kommen ausschließlich aus der Umgebung (Infisical/systemd) über den
//! konsolidierten Resolver [`keys`] und werden NIE geloggt.
//!
//! # Aufrufer
//!
//! Bestehende, verstreute MiniMax-/Anthropic-Aufrufe (z. B. `scam_pitch`,
//! `title_ai`, `post_stream`, die Dashboard-AI-Handler) können gegen
//! [`provider::LlmProvider`] programmieren, ohne ihre Fachlogik umzubauen — diese
//! Crate stellt nur die Foundation (Clients + Ledger) bereit.

pub mod anthropic;
pub mod keys;
pub mod ledger;
pub mod minimax;
pub mod provider;

pub use anthropic::AnthropicClient;
pub use minimax::MiniMaxClient;
pub use provider::{CompletionRequest, CompletionResponse, LlmError, LlmProvider, Message};
