//! HTTP-Router für die interne Twitch-Bot-API.
//!
//! Öffentlicher Einstiegspunkt: `build_internal_router(pool, token)`.
//! Alle Endpoints liegen unter `/internal/twitch/v1`.
//! Auth: X-Internal-Token-Header + Loopback-Guard (Defense-in-Depth).

pub mod handlers;

use axum::{middleware, routing::{get, post}, Extension, Router};
use sqlx::PgPool;
use tb_http_core::{
    loopback_only, internal_auth, ExpectedToken, INTERNAL_API_BASE_PATH,
};

/// Baut den axum-Router für alle internen Endpoints.
///
/// `token` wird als `ExpectedToken`-Extension eingesetzt.
/// `loopback_only` + `internal_auth` werden als Layer gestapelt.
pub fn build_internal_router(pool: PgPool, token: String) -> Router {
    use handlers::{global_ban, healthz};

    let base = INTERNAL_API_BASE_PATH; // "/internal/twitch/v1"

    Router::new()
        .route(&format!("{base}/healthz"), get(healthz::healthz_handler))
        .route(
            &format!("{base}/globalban"),
            get(global_ban::list_handler),
        )
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
        .with_state(pool)
        .layer(Extension(ExpectedToken(token.clone())))
        .layer(middleware::from_fn_with_state(token, internal_auth))
        .layer(middleware::from_fn(loopback_only))
}
