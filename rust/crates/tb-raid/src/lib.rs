//! tb-raid — Raid-Subsystem des Twitch-Bots (Schritt 6).
//!
//! Saubere Trennung statt des Python-`auth.py`-Monolithen (1797 Z.): Das
//! RaidAuth-Fundament zerfällt in eigenständige Strukturen —
//!
//! - [`state_store`] — OAuth-State-Token-Lifecycle (`oauth_state_tokens`,
//!   plattform-gated auf `twitch_raid`). **6a, dieser Stand.**
//! - [`scope_profiles`] — Scope-Konstanten, Normalisierung und Profil-Auflösung.
//!   **6a, dieser Stand.**
//! - [`oauth_flow`] — Authorize-URL-Bau, State-Info-Aufbau, DB-Abstraktions-Trait.
//!   **6a, dieser Stand.**
//! - TokenRefresher (Crypto + Exchange/Refresh via [`tb_crypto`]) — folgt.
//! - TokenStore (Token-Lese-API + `twitch_token_blacklist`) — folgt.
//!
//! Plan + Slice-Schnitt: `docs/plans/2026-06-09-schritt-6-raid.md`.

pub mod oauth_flow;
pub mod scope_profiles;
pub mod state_store;

pub use oauth_flow::{
    build_authorize_url, build_state_info, StreamerContextResolver,
    PUBLIC_WEBSITE_ONBOARDING_LOGIN, TWITCH_AUTHORIZE_URL,
};
pub use scope_profiles::{
    normalize_scope_profile, scopes_for_profile, AUTO_SCOPE_PROFILE, BASE_CRITICAL_STREAMER_SCOPES,
    BASE_SCOPE_PROFILE, BASE_STREAMER_SCOPES, DASHBOARD_REAUTH_SCOPE_PROFILE,
    DASHBOARD_UPGRADE_SCOPES, FULL_STREAMER_SCOPES,
};
pub use state_store::{RaidOAuthState, StateStore};
