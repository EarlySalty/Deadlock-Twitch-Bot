//! HTTP-Router für das public Analytics-Dashboard.
//!
//! Öffentlicher Einstiegspunkt: `build_public_router(pool)`.
//! Kein Auth, kein Loopback-Gate — diese Routen sind explizit public (`CORS: *`).

pub mod handlers;

use axum::Router;
use sqlx::PgPool;
use tower_http::cors::CorsLayer;

/// Baut den axum-Router für alle public Analytics-GET-Endpoints.
///
/// CORS-Policy: `CorsLayer::permissive()` (entspricht Python-`Access-Control-Allow-Origin: *`).
/// Auth-Routen kommen in `build_authed_router` (Slice 1b).
pub fn build_public_router(pool: PgPool) -> Router {
    use axum::routing::get;
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
