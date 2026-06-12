//! HTTP-Router für das Analytics-Dashboard.
//!
//! Öffentlicher Einstiegspunkt: `build_router(pool, token)`.
//! Kein Auth, kein Loopback-Gate bei public-Routen — explizit `CORS: *`.
//! Auth-Routen nutzen `AuthLevel`-Extractor aus tb-http-core.

pub mod auth;
pub mod handlers;
pub mod process_info;
/// Strangler-Fig-Fallback-Proxy (→ Python 8765), siehe Modul-Doku.
pub mod proxy;

pub use auth::level::DashboardAuthLevel;
pub use auth::session::DashboardAuthState;

use axum::{routing::get, Extension, Router};
use sqlx::PgPool;
use tb_http_core::ExpectedToken;
use tower_http::cors::CorsLayer;

/// Baut den axum-Router für alle public Analytics-GET-Endpoints.
///
/// CORS-Policy: `CorsLayer::permissive()` NUR auf den Public-Routen —
/// Python setzt Access-Control-Header ausschließlich dort
/// (`api_public.py:52-58`). Authed/Admin-Routen bleiben ohne CORS-Header,
/// sonst wäre die Token-API cross-origin per Browser ansprechbar.
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
    use handlers::{ads_schedule, audience, auth_status, category_leaderboard, follower_funnel, loyalty_curve, lurker_analysis, overview, performance, raid_analytics, rankings, retention_curve, session_detail, spa, streamers, title_performance, viewer_timeline, viewers};

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
        // Performance-Analytics (lesen aus twitch_stream_sessions in Postgres)
        .route(
            "/twitch/api/v2/monthly-stats",
            get(performance::monthly_stats_handler),
        )
        .route(
            "/twitch/api/v2/weekly-stats",
            get(performance::weekly_stats_handler),
        )
        .route(
            "/twitch/api/v2/hourly-heatmap",
            get(performance::hourly_heatmap_handler),
        )
        .route(
            "/twitch/api/v2/calendar-heatmap",
            get(performance::calendar_heatmap_handler),
        )
        .route(
            "/twitch/api/v2/rankings",
            get(rankings::rankings_handler),
        )
        .route(
            "/twitch/api/v2/follower-funnel",
            get(follower_funnel::follower_funnel_handler),
        )
        .route(
            "/twitch/api/v2/tag-analysis",
            get(audience::tag_analysis_handler),
        )
        .route(
            "/twitch/api/v2/viewer-overlap",
            get(audience::viewer_overlap_handler),
        )
        .route(
            "/twitch/api/v2/viewer-profiles",
            get(audience::viewer_profiles_handler),
        )
        .route(
            "/twitch/api/v2/audience-sharing",
            get(audience::audience_sharing_handler),
        )
        .route(
            "/twitch/api/v2/audience-insights",
            get(audience::audience_insights_handler),
        )
        .route(
            "/twitch/api/v2/lurker-analysis",
            get(lurker_analysis::lurker_analysis_handler),
        )
        .route(
            "/twitch/api/v2/category-leaderboard",
            get(category_leaderboard::category_leaderboard_handler),
        )
        .route(
            "/twitch/api/v2/title-performance",
            get(title_performance::title_performance_handler),
        )
        .route(
            "/twitch/api/v2/ads-schedule",
            get(ads_schedule::ads_schedule_handler),
        )
        .route(
            "/twitch/api/v2/retention-curve",
            get(retention_curve::retention_curve_handler),
        )
        .route(
            "/twitch/api/v2/loyalty-curve",
            get(loyalty_curve::loyalty_curve_handler),
        )
        .route(
            "/twitch/api/v2/:streamer/viewer-timeline",
            get(viewer_timeline::viewer_timeline_handler),
        )
        .route(
            "/twitch/api/v2/:streamer/viewer-timeline/profile",
            get(viewer_timeline::viewer_timeline_profile_handler),
        )
        .route(
            "/twitch/api/v2/raid-retention",
            get(raid_analytics::raid_retention_handler),
        )
        .route(
            "/twitch/api/v2/raid-analytics",
            get(raid_analytics::raid_analytics_handler),
        )
        .route(
            "/twitch/api/v2/viewer-directory",
            get(viewers::viewer_directory_handler),
        )
        .route(
            "/twitch/api/v2/viewer-detail",
            get(viewers::viewer_detail_handler),
        )
        .route(
            "/twitch/api/v2/viewer-segments",
            get(viewers::viewer_segments_handler),
        )
        .route(
            "/twitch/api/v2/session/:id",
            get(session_detail::session_detail_handler),
        )
        .route(
            "/twitch/api/v2/session/:id/events",
            get(session_detail::session_events_handler),
        )
        // SPA: Haupt-HTML + statische Assets
        .route("/analyse", get(spa::analyse_handler))
        .route("/analyse/*path", get(spa::analyse_assets_handler))
        .with_state(pool)
        .layer(Extension(ExpectedToken(token)))
}

/// Baut den Router für Admin-System-Endpoints.
pub fn build_admin_system_router(pool: PgPool, token: String) -> Router {
    use handlers::system::{database, errors, eventsub, health};

    Router::new()
        .route(
            "/twitch/api/admin/system/health",
            get(health::health_handler),
        )
        .route(
            "/twitch/api/admin/system/database",
            get(database::database_handler),
        )
        .route(
            "/twitch/api/admin/system/eventsub",
            get(eventsub::eventsub_handler),
        )
        .route(
            "/twitch/api/admin/system/errors",
            get(errors::errors_handler),
        )
        .with_state(pool)
        .layer(Extension(ExpectedToken(token)))
}

/// Baut den Router für Admin-Streamer-Endpoints.
pub fn build_admin_streamers_router(pool: PgPool, token: String) -> Router {
    use handlers::admin_streamers;

    Router::new()
        .route(
            "/twitch/api/admin/streamers",
            get(admin_streamers::list_handler),
        )
        .route(
            "/twitch/api/admin/streamers/:login",
            get(admin_streamers::detail_handler),
        )
        .with_state(pool)
        .layer(Extension(ExpectedToken(token)))
}

/// Zusammengeführter Router: public + authed + admin-system + admin-streamers
/// + Legal-Seiten (HTML, statuslos).
///
/// CORS nur auf dem Public-Sub-Router (s. oben).
pub fn build_router(pool: PgPool, token: String) -> Router {
    build_public_router(pool.clone())
        .merge(build_authed_router(pool.clone(), token.clone()))
        .merge(build_admin_system_router(pool.clone(), token.clone()))
        .merge(build_admin_streamers_router(pool, token))
        .merge(handlers::legal::build_legal_router())
}
