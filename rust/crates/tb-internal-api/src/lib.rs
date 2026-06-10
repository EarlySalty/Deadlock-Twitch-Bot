//! HTTP-Router für die interne Twitch-Bot-API.
//!
//! Öffentlicher Einstiegspunkt: `build_internal_router(pool, token, helix)`.
//! Alle Endpoints liegen unter `/internal/twitch/v1`.
//! Auth: X-Internal-Token-Header + Loopback-Guard (Defense-in-Depth).

pub mod handlers;

use axum::{
    middleware,
    routing::{delete, get, post},
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

/// Baut den axum-Router für alle internen Endpoints.
///
/// `token` wird als `ExpectedToken`-Extension eingesetzt.
/// `helix` wird als `Extension<Arc<Option<HelixClient>>>` eingesetzt.
/// `dispatcher` bedient `POST /eventsub/dispatch`; `None` → 503 (Bridge puffert).
/// `legacy_proxy` reicht unbekannte Routen an die Legacy-Python-API weiter;
/// `None` → unbekannte Routen antworten 404 (Strangler-Fig, s. `legacy_proxy`).
/// `loopback_only` + `internal_auth` werden als Layer gestapelt.
pub fn build_internal_router(
    pool: PgPool,
    token: String,
    helix: Arc<Option<HelixClient>>,
    dispatcher: Option<Arc<EventSubDispatcher>>,
    manual_raid: Option<Arc<dyn handlers::raid::ManualRaidPort>>,
    legacy_proxy: Option<Arc<LegacyProxy>>,
) -> Router {
    use handlers::{eventsub, global_ban, healthz, raid, streamers};

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
        // Streamer-CRUD-Endpoints
        .route(&format!("{base}/streamers"), get(streamers::list_handler))
        .route(&format!("{base}/streamers"), post(streamers::add_handler))
        .route(
            &format!("{base}/streamers/:login"),
            delete(streamers::remove_handler),
        )
        .route(
            &format!("{base}/streamers/:login/verify"),
            post(streamers::verify_handler),
        )
        .route(
            &format!("{base}/streamers/:login/archive"),
            post(streamers::archive_handler),
        )
        .route(
            &format!("{base}/streamers/:login/discord-flag"),
            post(streamers::discord_flag_handler),
        )
        .route(
            &format!("{base}/streamers/:login/discord-profile"),
            post(streamers::discord_profile_handler),
        )
        .route(
            &format!("{base}/streamers/:login/chat-action"),
            post(streamers::chat_action_handler),
        )
        .fallback(handlers::legacy_proxy::legacy_fallback_handler)
        .with_state(pool)
        .layer(Extension(helix))
        .layer(Extension(EventSubDispatcherExt(dispatcher)))
        .layer(Extension(handlers::raid::ManualRaidExt(manual_raid)))
        .layer(Extension(LegacyProxyExt(legacy_proxy)))
        .layer(Extension(ExpectedToken(token.clone())))
        .layer(middleware::from_fn_with_state(token, internal_auth))
        .layer(middleware::from_fn(loopback_only))
}
