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
//! - [`scoring`] — reine Score-Berechnung (kein DB-Zugriff, voll unit-testbar).
//!   **6c, Partner-Raid-Scoring.**
//! - [`score_store`] — DB-Store für `twitch_partner_raid_scores` (Lesen/Schreiben).
//!   **6c, Partner-Raid-Scoring.**
//! - [`raid_history_store`] — Raid-History schreiben + recent-targets laden.
//!   **6d, Raid-Ausführung.**
//! - [`strikes_store`] — Raid-Disabled-Strikes UPSERT. **6d.**
//! - [`candidate_selection`] — reine Kandidaten-Auswahl-Logik (kein DB).
//!   **6d.**
//! - [`pending_raids`] — In-Memory-Store für ausstehende Raids + Key-Normalisierung.
//!   **6e.**
//! - [`arrival_tracking_store`] — DB-Store für `twitch_raid_arrival_tracking`
//!   (INSERT / UPDATE / Unraid-Markierung). **6e.**
//!
//! Alle 6a (RaidAuth-Fundament). Plan: `docs/plans/2026-06-09-schritt-6-raid.md`.

pub mod arrival_confirmation;
pub mod arrival_runtime;
pub mod arrival_tracking_store;
pub mod auth_writer;
pub mod auto_raid_pipeline;
pub mod candidate_selection;
pub mod eligibility;
pub mod manual_suppression;
pub mod oauth_flow;
pub mod offline_eligibility;
pub mod partner_roster;
pub mod pending_raids;
pub mod raid_blacklist;
pub mod raid_executor;
pub mod raid_history_store;
pub mod scope_profiles;
pub mod score_store;
pub mod score_tracking_store;
pub mod scoring;
pub mod signal_correlation;
pub mod state_store;
pub mod strikes_store;
pub mod target_resolution;
pub mod token_blacklist;
pub mod token_provider;
pub mod token_refresher;
pub mod token_store;
pub mod util;

pub use arrival_confirmation::{
    classify_partner_raid_arrival, ArrivalConfirmationDecision, ArrivalConfirmationService,
    ArrivalSignalContext, FollowUpKind, KnownStreamerLookup, PartnerLookup,
    PartnerRaidArrivalResolution,
};
pub use arrival_runtime::{RaidArrivalRuntime, RaidArrivalSink};
pub use arrival_tracking_store::{ArrivalTrackingStore, RecordArrivalInput};
pub use auth_writer::{AuthWriteError, AuthWriter, NewAuth};
pub use auto_raid_pipeline::{
    ArrivalReadiness, AutoRaidPipeline, AutoRaidPipelineOutcome, AutoRaidRequest,
    FallbackStreamSource,
};
pub use candidate_selection::{
    is_retryable_raid_error, select_by_score, select_fairest, FairnessCandidate, ScoredCandidate,
    SelectionReason, SelectionResult, DAILY_RAID_SOFT_CAP, PARTNER_SCORE_THRESHOLD,
};
pub use eligibility::{
    classify_eligibility, filter_eligible, is_deadlock_eligible, is_recent_deadlock,
    DeadlockEvalInput, EligibilityBucket, DEADLOCK_RECENCY_CAP_SECONDS,
};
pub use manual_suppression::ManualRaidSuppression;
pub use oauth_flow::{
    build_authorize_url, build_state_info, StreamerContextResolver,
    PUBLIC_WEBSITE_ONBOARDING_LOGIN, TWITCH_AUTHORIZE_URL,
};
pub use offline_eligibility::{OfflineAutoRaidEligibility, OfflineEligibilityStore};
pub use partner_roster::{
    build_online_candidates, OnlineCandidate, PartnerRosterEntry, PartnerRosterStore, StreamData,
};
pub use pending_raids::{
    normalize_broadcaster_login, normalize_pending_raid_key, PendingRaid, PendingRaidStore,
};
pub use raid_blacklist::RaidBlacklistStore;
pub use raid_executor::{RaidApi, RaidExecutor, RaidOutcome, RaidRequest};
pub use raid_history_store::{RaidHistoryStore, RecordRaidInput};
pub use scope_profiles::{
    normalize_scope_profile, scopes_for_profile, AUTO_SCOPE_PROFILE, BASE_CRITICAL_STREAMER_SCOPES,
    BASE_SCOPE_PROFILE, BASE_STREAMER_SCOPES, DASHBOARD_REAUTH_SCOPE_PROFILE,
    DASHBOARD_UPGRADE_SCOPES, FULL_STREAMER_SCOPES,
};
pub use score_store::{PartnerRaidScoreRow, PartnerRaidScoreUpsert, ScoreStore};
pub use score_tracking_store::{ScoreTrackingStore, TrackConfirmedInput};
pub use scoring::{
    compute_base_score, compute_duration_score, compute_fairness_score, compute_final_score,
    compute_new_partner_multiplier, compute_raid_boost_multiplier, compute_readiness_score,
    compute_scores, compute_time_pattern_score, ScoreComponents, ScoringInputs,
    DEFAULT_RAID_BOOST_MULTIPLIER, NEUTRAL_SCORE, NEW_PARTNER_MAX_MULTIPLIER,
    NEW_PARTNER_RAID_THRESHOLD, RAID_BOOST_MULTIPLIER,
};
pub use signal_correlation::{
    ActionData, RaidArrivalInput, RaidSignalAction, RaidSignalActionKind,
    RaidSignalCorrelationService, RaidSignalOutcome, RaidSignalPlan, RaidSignalType,
};
pub use state_store::{RaidOAuthState, StateStore};
pub use strikes_store::StrikesStore;
pub use target_resolution::{
    resolve_fallback_target, resolve_partner_target, PartnerResolution, PartnerResolutionStats,
    ResolvedTarget,
};
pub use token_blacklist::TokenBlacklistStore;
pub use token_provider::TokenProvider;
pub use token_refresher::{
    advisory_lock_pair, RaidTokenRefresher, RefreshError, RefreshOutcome, TokenBlacklist,
    TokenResponse, TwitchTokenClient,
};
pub use token_store::{RaidAuthStore, RaidTokens};
pub use util::parse_iso_utc;
