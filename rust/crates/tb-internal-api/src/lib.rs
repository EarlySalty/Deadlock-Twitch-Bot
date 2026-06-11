//! HTTP-Router für die interne Twitch-Bot-API.
//!
//! Öffentlicher Einstiegspunkt: `build_internal_router(pool, token, helix)`.
//! Alle Endpoints liegen unter `/internal/twitch/v1`.
//! Auth: X-Internal-Token-Header + Loopback-Guard (Defense-in-Depth).

pub mod handlers;
pub mod idempotency;

use axum::{
    middleware,
    routing::{get, post},
    Extension, Router,
};
use sqlx::PgPool;
use std::sync::Arc;
use tb_http_core::{internal_auth, loopback_only, ExpectedToken, INTERNAL_API_BASE_PATH};
use tb_monitoring::EventSubDispatcher;
use tb_transport_twitch::HelixClient;

pub use handlers::eventsub::EventSubDispatcherExt;
pub use handlers::legacy_proxy::{LegacyProxy, LegacyProxyExt};
pub use handlers::raid::{ManualRaidExt, ManualRaidPort};
pub use handlers::raid_oauth::{RaidOAuthExt, RaidOAuthPort};
pub use idempotency::IdempotencyState;

/// Baut den axum-Router für alle internen Endpoints.
///
/// `token` wird als `ExpectedToken`-Extension eingesetzt.
/// `helix` wird als `Extension<Arc<Option<HelixClient>>>` eingesetzt.
/// `dispatcher` bedient `POST /eventsub/dispatch`; `None` → 503 (Bridge puffert).
/// `raid_oauth` bedient die `/raid/*`-OAuth-Strecke; `None` → 503.
/// `legacy_proxy` reicht unbekannte Routen an die Legacy-Python-API weiter;
/// `None` → unbekannte Routen antworten 404 (Strangler-Fig, s. `legacy_proxy`).
/// `loopback_only` + `internal_auth` werden als Layer gestapelt.
pub fn build_internal_router(
    pool: PgPool,
    token: String,
    helix: Arc<Option<HelixClient>>,
    dispatcher: Option<Arc<EventSubDispatcher>>,
    manual_raid: Option<Arc<dyn handlers::raid::ManualRaidPort>>,
    raid_oauth: Option<Arc<dyn handlers::raid_oauth::RaidOAuthPort>>,
    legacy_proxy: Option<Arc<LegacyProxy>>,
) -> Router {
    use handlers::{
        chat_command, discord_invite, eventsub, global_ban, healthz, market_share, raid,
        raid_blacklist, raid_oauth as oauth, self_explainer_log, streamer_link, streamers,
        telemetry_routes,
    };

    let base = INTERNAL_API_BASE_PATH; // "/internal/twitch/v1"

    Router::new()
        .route(&format!("{base}/healthz"), get(healthz::healthz_handler))
        .route(
            &format!("{base}/eventsub/dispatch"),
            post(eventsub::dispatch_handler),
        )
        .route(
            &format!("{base}/raid/manual"),
            post(raid::manual_raid_handler),
        )
        .route(
            &format!("{base}/streamer/:login/discord-invite"),
            get(discord_invite::handler),
        )
        .route(
            &format!("{base}/chat/command"),
            post(chat_command::handler),
        )
        .route(&format!("{base}/globalban"), get(global_ban::list_handler))
        .route(
            &format!("{base}/globalban/add"),
            post(global_ban::add_handler),
        )
        .route(
            &format!("{base}/globalban/remove"),
            post(global_ban::remove_handler),
        )
        .route(
            &format!("{base}/globalban/check"),
            get(global_ban::check_handler),
        )
        // Raid-Blacklist: nativer Port der bislang an Python 8779 proxied
        // CRUD-Routen. Distinkte Pfade je Methode → kein 405-vs-Fallback-
        // Konflikt. Login via tb_domain::normalize_twitch_login kanonisiert;
        // Byte-Parität durch identisches SQL (s. tb_analytics::raid_blacklist).
        .route(
            &format!("{base}/raid/blacklist"),
            get(raid_blacklist::list_handler),
        )
        .route(
            &format!("{base}/raid/blacklist/add"),
            post(raid_blacklist::add_handler),
        )
        .route(
            &format!("{base}/raid/blacklist/remove"),
            post(raid_blacklist::remove_handler),
        )
        .route(
            &format!("{base}/raid/blacklist/check"),
            get(raid_blacklist::check_handler),
        )
        // Streamer-Link-Kandidaten (nativer Port, reiner GET-Read; kein POST auf
        // demselben Pfad → kein 405-vs-Fallback-Konflikt).
        .route(
            &format!("{base}/streamers/link-candidates"),
            get(streamer_link::list_handler),
        )
        // Markt-Dominanz fürs Admin-Dashboard: nativer GET-Read auf
        // twitch_stats_category; das Python-Dashboard (8765) proxied
        // /twitch/api/v2/market-share hierher.
        .route(
            &format!("{base}/market-share"),
            get(market_share::market_share_handler),
        )
        // Self-Explainer-Discord-Log: reiner Relay an den Master-Broker (8770),
        // kein DB-Zugriff. Token-Fallback-Kette inkl. TWITCH_INTERNAL_API_TOKEN.
        .route(
            &format!("{base}/discord/self-explainer-log"),
            post(self_explainer_log::handler),
        )
        // Raid-OAuth-Strecke (Welle B): nativ via RaidOAuthPort +
        // Composition-Root in tb-bot (raid_oauth_impl.rs). auth-url schreibt
        // den State in oauth_state_tokens mit IDENTISCHEM SQL wie Python.
        // oauth-callback ist seit dem Followup-Port (12.6.) komplett nativ:
        // Token-Exchange + Helix-Owner-Lookup + Mismatch-/Scope-Checks +
        // verschlüsselter Persist + Background-Followups (complete_setup /
        // sync_partner_state via PartnerSetupService); Idempotenz cacht das
        // Ergebnis als HTTP 200 wie Python.
        // BEWUSST NICHT nativ:
        // - POST /raid/requirements — sendet in Python eine echte Discord-DM;
        //   ohne Discord-Bridge bleibt die Route über den Legacy-Proxy.
        .route(
            &format!("{base}/raid/auth-url"),
            get(oauth::auth_url_handler),
        )
        .route(
            &format!("{base}/raid/oauth-callback"),
            post(oauth::oauth_callback_handler),
        )
        .route(
            &format!("{base}/raid/auth-state"),
            get(oauth::auth_state_handler),
        )
        .route(
            &format!("{base}/raid/block-state"),
            get(oauth::block_state_handler),
        )
        .route(&format!("{base}/raid/go-url"), get(oauth::go_url_handler))
        // Telemetrie (Welle B): announcements = reiner DB-Read; link-click =
        // Write mit geteiltem Idempotenz-Layer (Scope-Key, Fingerprint→409,
        // Inflight-Dedup, Replay-Header — voller Python-Vertrag).
        .route(
            &format!("{base}/live/active-announcements"),
            get(telemetry_routes::live_active_announcements_handler),
        )
        .route(
            &format!("{base}/live/link-click"),
            post(telemetry_routes::live_link_click_handler),
        )
        // Analytics-Reads (Welle B). Nur /analytics/comparison ist nativ —
        // Shape im Live-Diff gegen Python 8779 verifiziert.
        // BEWUSST NICHT nativ (Live-Diff zeigte echte Shape-Lücken):
        // - GET /stats — Python liefert {tracked:{top,hourly,weekday},
        //   category, avg_viewers_all, avg_viewers_tracked}; der Rust-Handler
        //   eine andere Aggregation. Der Dashboard-Split-Mode liest exakt
        //   die Python-Felder.
        // - GET /analytics/streamer/:login — Python delegiert an
        //   AnalyticsBackendExtended.get_comprehensive_analytics; der
        //   Rust-Handler baut nur {stats, recent_sessions}.
        // - GET /sessions/:session_id — Python macht SELECT * plus
        //   berechnete Felder (retention_5m/10m/20m, dropoff_label,
        //   start/end_viewers, …); dem Rust-Port fehlen ~15 Felder.
        .route(
            &format!("{base}/analytics/comparison"),
            get(streamers::analytics_comparison_handler),
        )
        // Streamer-Endpoints: kompletter /streamers-Baum läuft bewusst über
        // den Legacy-Fallback-Proxy zum Python-Worker, bis Partner-Lifecycle
        // (promote/departner) + Discord-Bridge nativ sind: verify
        // mode=clear/failed departnern in Python KOMPLETT inkl. Discord-DM,
        // add/remove brauchen Promote-Logik + Helix-Verhalten, discord-flag/
        // -profile brauchen Rollen-Sync, chat-action braucht den live
        // rotierten Bot-Token des Python-Chats. Auch der Lesepfad
        // (GET /streamers) bleibt drüben: ein nativer GET würde für POST auf
        // demselben Pfad 405 statt Fallback liefern (axum matcht den Pfad,
        // dann erst die Methode). Ebenfalls proxied: GET /debug/observability
        // + /debug/chatters/:login (brauchen Python-In-Process-Bot-State) und
        // POST /eventsub/processing/requeue. Re-Aktivierung pro Route nur mit
        // Vertragstests gegen die Python-Antworten
        // (siehe rust/docs/04-cutover-plan.md, Kopplung 8).
        .fallback(handlers::legacy_proxy::legacy_fallback_handler)
        .with_state(pool)
        .layer(Extension(helix))
        .layer(Extension(EventSubDispatcherExt(dispatcher)))
        .layer(Extension(handlers::raid::ManualRaidExt(manual_raid)))
        .layer(Extension(handlers::raid_oauth::RaidOAuthExt(raid_oauth)))
        .layer(Extension(idempotency::IdempotencyState::new()))
        .layer(Extension(LegacyProxyExt(legacy_proxy)))
        .layer(Extension(ExpectedToken(token.clone())))
        .layer(middleware::from_fn_with_state(token, internal_auth))
        .layer(middleware::from_fn(loopback_only))
}
