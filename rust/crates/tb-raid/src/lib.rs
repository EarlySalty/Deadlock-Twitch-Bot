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
//! - [`token_store`] — verschlüsselter Token-Lesepfad (`_enc`-Spalten, AAD).
//! - [`token_refresher`] — Refresh-Schreibpfad (Advisory-Lock, Lockout-Schutz).
//! - [`auth_writer`] — Onboarding-/Re-Auth-Write (UPSERT, Scope-Validierung).
//!
//! Alle 6a (RaidAuth-Fundament). Plan: `docs/plans/2026-06-09-schritt-6-raid.md`.

pub mod auth_writer;
pub mod oauth_flow;
pub mod raid_blacklist;
pub mod scope_profiles;
pub mod state_store;
pub mod token_blacklist;
pub mod token_refresher;
pub mod token_store;
pub mod util;

pub use auth_writer::{AuthWriteError, AuthWriter, NewAuth};
pub use oauth_flow::{
    build_authorize_url, build_state_info, StreamerContextResolver,
    PUBLIC_WEBSITE_ONBOARDING_LOGIN, TWITCH_AUTHORIZE_URL,
};
pub use raid_blacklist::RaidBlacklistStore;
pub use scope_profiles::{
    normalize_scope_profile, scopes_for_profile, AUTO_SCOPE_PROFILE, BASE_CRITICAL_STREAMER_SCOPES,
    BASE_SCOPE_PROFILE, BASE_STREAMER_SCOPES, DASHBOARD_REAUTH_SCOPE_PROFILE,
    DASHBOARD_UPGRADE_SCOPES, FULL_STREAMER_SCOPES,
};
pub use state_store::{RaidOAuthState, StateStore};
pub use token_blacklist::TokenBlacklistStore;
pub use token_refresher::{
    advisory_lock_pair, RaidTokenRefresher, RefreshError, RefreshOutcome, TokenBlacklist,
    TokenResponse, TwitchTokenClient,
};
pub use token_store::{RaidAuthStore, RaidTokens};
