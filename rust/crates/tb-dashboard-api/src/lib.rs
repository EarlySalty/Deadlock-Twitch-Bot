//! HTTP-Router für das Analytics-Dashboard.
//!
//! Öffentlicher Einstiegspunkt: `build_router(pool, token)`.
//! Kein Auth, kein Loopback-Gate bei public-Routen — explizit `CORS: *`.
//! Auth-Routen nutzen `AuthLevel`-Extractor aus tb-http-core.

pub mod auth;
pub mod handlers;

use axum::{routing::get, Extension, Router};
use sqlx::PgPool;
use tb_http_core::ExpectedToken;
use tower_http::cors::CorsLayer;

/// Baut den axum-Router für alle public Analytics-GET-Endpoints.
///
/// CORS-Policy: `CorsLayer::permissive()`.
pub fn build_public_router(pool: PgPool) -> Router {
    use handlers::{bans, network, raids};

    Router::new()
        .route(
            "/twitch/api/v2/public/recent-bans",
            get(bans::recent_bans_handler),
        )
        .route(
            "/twitch/api/v2/public/recent-raids",
            get(raids::recent_raids_handler),
        )
        .route(
            "/twitch/api/v2/public/network",
            get(network::network_handler),
        )
        .with_state(pool)
        .layer(CorsLayer::permissive())
}

/// Baut den Router für Admin-geschützte Routes.
///
/// Auth-Level wird per Extension eingesetzt — `AuthLevel` als `FromRequestParts`
/// liest den Token selbst aus der Extension.
pub fn build_authed_router(pool: PgPool, token: String) -> Router {
    use handlers::{auth_status, overview, streamers};

    Router::new()
        .route(
            "/twitch/api/v2/auth-status",
            get(auth_status::auth_status_handler),
        )
        .route(
            "/twitch/api/v2/streamers",
            get(streamers::streamers_handler),
        )
        .route("/twitch/api/v2/overview", get(overview::overview_handler))
        .with_state(pool)
        .layer(Extension(ExpectedToken(token)))
        .layer(CorsLayer::permissive())
}

/// Zusammengeführter Router: public + authed.
///
/// Kein doppelter CorsLayer — jeder Sub-Router hat seinen eigenen.
pub fn build_router(pool: PgPool, token: String) -> Router {
    build_public_router(pool.clone()).merge(build_authed_router(pool, token))
}
