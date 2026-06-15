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

use axum::{
    routing::{get, post},
    Extension, Router,
};
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
    use handlers::{bans, network, raids, self_explainer, social_media};

    Router::new()
        .route(
            "/twitch/api/v2/public/recent-bans",
            get(bans::recent_bans_handler),
        )
        // Frage-Box auf /streamer: erklärt den Bot grounded (öffentlich, rate-limitiert).
        .route(
            "/twitch/api/v2/self-explainer/ask",
            post(self_explainer::self_explainer_ask),
        )
        .route(
            "/twitch/api/v2/public/recent-raids",
            get(raids::recent_raids_handler),
        )
        .route(
            "/twitch/api/v2/public/network",
            get(network::network_handler),
        )
        // Social-Media Rechtstexte — öffentlich für die Plattform-OAuth-Reviews.
        .route("/social-media/terms", get(social_media::terms_handler))
        .route("/social-media/privacy", get(social_media::privacy_handler))
        // OAuth-Callback — öffentlich (Provider-Redirect, Security via State-Token).
        .route("/social-media/oauth/callback", get(social_media::oauth_callback_handler))
        .route("/social-media/oauth/callback/:platform", get(social_media::oauth_callback_handler))
        .with_state(pool)
        .layer(CorsLayer::permissive())
}

/// Baut den Router für Admin-geschützte Routes.
///
/// Auth-Level wird per Extension eingesetzt — `AuthLevel` als `FromRequestParts`
/// liest den Token selbst aus der Extension.
pub fn build_authed_router(pool: PgPool, token: String) -> Router {
    use handlers::{ads_schedule, audience, audience_demographics, auth_status, billing, category_comparison, category_leaderboard, category_timings, engagement_settings, follower_funnel, internal_home, loyalty_curve, lurker_analysis, overview, performance, raid_analytics, rankings, retention_curve, session_detail, silent_settings, social_media, spa, stream_report, streamers, title_performance, viewer_timeline, viewers};

    Router::new()
        .route(
            "/twitch/api/v2/auth-status",
            get(auth_status::auth_status_handler),
        )
        // Social-Media-Dashboard-SPA (Auth erforderlich).
        .route("/social-media", get(social_media::index_handler))
        // Social-Media Read-API (scope-gefiltert).
        .route("/social-media/api/stats", get(social_media::stats_handler))
        .route("/social-media/api/clips", get(social_media::clips_handler))
        .route("/social-media/api/last-hashtags", get(social_media::last_hashtags_handler))
        // Analytics-Ansicht (identisch zu stats) + Queue-Upload.
        .route("/social-media/api/analytics", get(social_media::stats_handler))
        .route("/social-media/api/upload", post(social_media::queue_upload_handler))
        .route("/social-media/api/mark-uploaded", post(social_media::mark_uploaded_handler))
        .route("/social-media/api/batch-upload", post(social_media::batch_upload_handler))
        .route("/social-media/api/fetch-clips", post(social_media::fetch_clips_handler))
        // Multipart-Datei-Upload — eigenes 201MB-Body-Limit (Default ist 2MB).
        .route(
            "/social-media/api/clips/upload",
            post(social_media::upload_clip_handler).layer(axum::extract::DefaultBodyLimit::max(201 * 1024 * 1024)),
        )
        // Templates: globale + Streamer-Listen (GET), anlegen + anwenden (POST).
        .route("/social-media/api/templates/global", get(social_media::templates_global_handler))
        .route("/social-media/api/templates/streamer", get(social_media::templates_streamer_handler).post(social_media::create_template_handler))
        .route("/social-media/api/templates/apply", post(social_media::apply_template_handler))
        // Layout-CRUD (Admin): Streamer-Default + Clip-Override.
        .route(
            "/social-media/api/admin/streamer-layout",
            get(social_media::streamer_layout_get_handler).put(social_media::streamer_layout_put_handler),
        )
        .route("/social-media/api/admin/clips/:clip_db_id/layout", axum::routing::put(social_media::clip_layout_put_handler))
        // Vocab-CRUD (Admin): Liste/Upsert, Löschen, Seed.
        .route(
            "/social-media/api/admin/vocab",
            get(social_media::vocab_list_handler).post(social_media::vocab_upsert_handler),
        )
        .route("/social-media/api/admin/vocab/seed", post(social_media::vocab_seed_handler))
        .route("/social-media/api/admin/vocab/:term", axum::routing::delete(social_media::vocab_delete_handler))
        // Plattform-Verbindungsstatus (verschlüsselte Credentials).
        .route("/social-media/api/platforms/status", get(social_media::platforms_status_handler))
        // Admin-Clips: paginierte Liste, Detail, Verwerfen.
        .route("/social-media/api/admin/clips", get(social_media::admin_clips_handler))
        .route("/social-media/api/admin/clips/:clip_db_id", get(social_media::admin_clip_detail_handler))
        .route("/social-media/api/admin/clips/:clip_db_id/discard", post(social_media::admin_clip_discard_handler))
        // Approval-State + Entscheidung, Auto-Approve-Settings (Admin).
        .route("/social-media/api/admin/approval/:clip_db_id", get(social_media::approval_get_handler))
        .route("/social-media/api/admin/approval/:clip_db_id/decision", post(social_media::approval_decision_handler))
        .route(
            "/social-media/api/admin/settings/auto-approve",
            get(social_media::auto_approve_get_handler).put(social_media::auto_approve_put_handler),
        )
        // Enrichment-Detail, Clip-Analytics, Report-Liste (Admin, lesend).
        .route(
            "/social-media/api/admin/clips/:clip_db_id/enrichment",
            get(social_media::enrichment_get_handler).put(social_media::enrichment_put_handler),
        )
        .route(
            "/social-media/api/admin/clips/:clip_db_id/enrichment/run",
            post(social_media::enrichment_run_handler),
        )
        .route("/social-media/api/admin/analytics/clips/:clip_db_id", get(social_media::clip_analytics_get_handler))
        .route("/social-media/api/admin/reports", get(social_media::reports_list_handler))
        .route("/social-media/api/admin/reports/run", post(social_media::reports_run_handler))
        // OAuth-Start + Disconnect (Auth erforderlich).
        .route("/social-media/oauth/start/:platform", get(social_media::oauth_start_handler))
        .route("/social-media/oauth/disconnect/:platform", post(social_media::oauth_disconnect_handler))
        // Internal-Home: gebündelte Dashboard-Startseite (Profil, KPIs, Bot-Events,
        // Changelog). GET liest, POST legt einen Changelog-Eintrag an (Admin-only).
        .route(
            "/twitch/api/v2/internal-home",
            get(internal_home::get_handler),
        )
        .route(
            "/twitch/api/v2/internal-home/changelog",
            post(internal_home::changelog_handler),
        )
        // Streamer-Selbstbedienung: Silent-Notification-Flags (sync zu !silentban/
        // !silentraid). GET liest, POST setzt beide Flags auf twitch_partners.
        .route(
            "/twitch/api/v2/streamer/silent-settings",
            get(silent_settings::get_handler).post(silent_settings::post_handler),
        )
        // AI-Engagement-Dashboard: Admin/Super-Mod sieht alle Kanäle, Partner nur
        // den eigenen. settings (Liste), toggle (an/aus), update (steam/persona/
        // tabu), log (Decision-Historie).
        .route(
            "/twitch/api/v2/engagement/settings",
            get(engagement_settings::get_settings_handler),
        )
        .route(
            "/twitch/api/v2/engagement/toggle",
            post(engagement_settings::post_toggle_handler),
        )
        .route(
            "/twitch/api/v2/engagement/update",
            post(engagement_settings::post_update_handler),
        )
        .route(
            "/twitch/api/v2/engagement/log",
            get(engagement_settings::get_log_handler),
        )
        // Onboarding des Engagement-Sende-Accounts (Smoke-Account): start =
        // Admin-only Authorize-Link, callback = öffentlich (Security via
        // State-Token). /callback/engagement-sender ist die in der Twitch-App
        // registrierte Redirect-URI (Caddy-Pfad-Freigabe nötig).
        .route(
            "/twitch/api/v2/engagement/sender-auth",
            get(engagement_settings::sender_auth_start_handler),
        )
        .route(
            "/twitch/api/v2/engagement/sender-callback",
            get(engagement_settings::sender_auth_callback_handler),
        )
        .route(
            "/callback/engagement-sender",
            get(engagement_settings::sender_auth_callback_handler),
        )
        .route(
            "/twitch/api/v2/streamers",
            get(streamers::streamers_handler),
        )
        .route("/twitch/api/v2/overview", get(overview::overview_handler))
        // Post-Stream-A/B-Report (B11): liest twitch_stream_ai_reports für die
        // Dashboard-Anzeige (Partner → eigener Login, Admin/Localhost → frei).
        .route(
            "/twitch/api/v2/stream-report",
            get(stream_report::stream_report_handler),
        )
        .route(
            "/twitch/api/v2/stream-report/rate",
            post(stream_report::stream_report_rate_handler),
        )
        .route(
            "/twitch/api/v2/stream-report/ab-vote",
            get(stream_report::stream_report_ab_vote_get)
                .post(stream_report::stream_report_ab_vote_post),
        )
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
            "/twitch/api/billing/trial/start",
            post(billing::start_trial_handler),
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
            "/twitch/api/v2/category-comparison",
            get(category_comparison::category_comparison_handler),
        )
        .route(
            "/twitch/api/v2/audience-demographics",
            get(audience_demographics::audience_demographics_handler),
        )
        .route(
            "/twitch/api/v2/category-timings",
            get(category_timings::category_timings_handler),
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
        .layer(axum::middleware::from_fn(crate::auth::partner_gate::partner_status_gate))
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

/// Baut den Router für Admin-Config-Endpoints (Schreib-Seite).
pub fn build_admin_config_router(pool: PgPool, token: String) -> Router {
    use handlers::admin_promo_mode;

    Router::new()
        .route(
            "/twitch/api/admin/config/promo",
            post(admin_promo_mode::set_promo_handler),
        )
        .with_state(pool)
        .layer(Extension(ExpectedToken(token)))
}

/// Zusammengeführter Router: public + authed + admin-system + admin-streamers
/// + admin-config + Legal-Seiten (HTML, statuslos).
///
/// CORS nur auf dem Public-Sub-Router (s. oben).
pub fn build_router(pool: PgPool, token: String) -> Router {
    build_public_router(pool.clone())
        .merge(build_authed_router(pool.clone(), token.clone()))
        .merge(build_admin_system_router(pool.clone(), token.clone()))
        .merge(build_admin_streamers_router(pool.clone(), token.clone()))
        .merge(build_admin_config_router(pool, token))
        .merge(handlers::legal::build_legal_router())
}
