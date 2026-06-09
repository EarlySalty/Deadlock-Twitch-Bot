//! tb-raid — Raid-Subsystem des Twitch-Bots (Schritt 6).
//!
//! Saubere Trennung statt des Python-`auth.py`-Monolithen (1797 Z.): Das
//! RaidAuth-Fundament zerfällt in eigenständige Strukturen —
//!
//! - [`state_store`] — OAuth-State-Token-Lifecycle (`oauth_state_tokens`,
//!   plattform-gated auf `twitch_raid`). **6a, dieser Stand.**
//! - OAuthFlow (Authorize-URL + PKCE + Scope-Profile) — folgt.
//! - TokenRefresher (Crypto + Exchange/Refresh via [`tb_crypto`]) — folgt.
//! - TokenStore (Token-Lese-API + `twitch_token_blacklist`) — folgt.
//!
//! Plan + Slice-Schnitt: `docs/plans/2026-06-09-schritt-6-raid.md`.

pub mod state_store;

pub use state_store::{RaidOAuthState, StateStore};
