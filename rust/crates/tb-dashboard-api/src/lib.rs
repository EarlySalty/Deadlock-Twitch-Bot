//! HTTP-Router für das Analytics-Dashboard.
//!
//! Öffentlicher Einstiegspunkt: `build_router(pool, token)`.
//! Kein Auth, kein Loopback-Gate bei public-Routen — explizit `CORS: *`.
//! Auth-Routen nutzen `AuthLevel`-Extractor aus tb-http-core.

pub mod ai_state;
pub mod auth;
pub mod handlers;
pub mod process_info;
pub mod query_int;
/// Strangler-Fig-Fallback-Proxy (→ Python 8765), siehe Modul-Doku.
pub mod proxy;

pub use auth::csrf::csrf_protect;
pub use auth::level::DashboardAuthLevel;
pub use auth::oauth_login::{HelixOAuthClient, TwitchIdentity, TwitchOAuthClient};
pub use auth::security::{require_internal, RateLimiter};
pub use auth::session::{
    build_session_cookie, clear_session_cookie, DashboardAuthState, OAuthLoginState, SameSite,
    SessionCreation, ADMIN_COOKIE_NAME, OAUTH_STATE_SESSION_TYPE, OAUTH_STATE_TTL_SECS,
    PARTNER_COOKIE_NAME, SESSION_CREATE_TTL_SECS,
};
pub use handlers::auth_login::{oauth_login_config_from_env, OAuthLoginConfig};
pub use handlers::billing_page::{billing_page_config_from_env, BillingPageConfig};
pub use handlers::billing_webhook::{stripe_webhook_config_from_env, StripeWebhookConfig};

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
    use handlers::{ads_schedule, ai_analysis, ai_chat, ai_history, audience, audience_demographics, auth_status, billing, category_activity, category_comparison, category_leaderboard, category_timings, chat_analytics, chat_content_analysis, chat_deep_minimax, chat_hype_timeline, chat_social_graph, coaching, engagement_mode, engagement_settings, exp_analytics, follower_funnel, internal_home, leaderboard, loyalty_curve, lurker_analysis, lurker_tax_settings, monetization, overview, performance, raid_analytics, rankings, retention_curve, session_detail, silent_settings, social_media, spa, stream_report, streamers, tag_analysis, title_performance, viewer_timeline, viewers, watch_time};

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
        // Streamer-Selbstbedienung: Lurker-Steuer-Toggle (B9, sync zu
        // !lurkersteuer_off). Default deaktiviert, alle Partner. Spalte
        // streamer_plans.lurker_tax_enabled.
        .route(
            "/twitch/api/v2/streamer/lurker-tax-settings",
            get(lurker_tax_settings::get_handler).post(lurker_tax_settings::post_handler),
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
        // B19-dash-mode-toggle: Output-Modus der Engagement-KI (off/shadow/live) auf
        // twitch_engagement_settings.output_mode. Partner setzt den eigenen Kanal,
        // Admin/Localhost via ?channel=. Orthogonal zum enabled-Toggle (shadow/live
        // greift erst bei enabled=TRUE).
        .route(
            "/twitch/api/v2/engagement/mode",
            get(engagement_mode::get_handler).post(engagement_mode::post_handler),
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
        // B13-2: Web-Leaderboard (Ersatz für den gedroppten Discord-!twl).
        .route(
            "/twitch/api/v2/leaderboard",
            get(leaderboard::leaderboard_handler),
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
            "/twitch/api/v2/category-activity-series",
            get(category_activity::category_activity_series_handler),
        )
        .route(
            "/twitch/api/v2/exp/overview",
            get(exp_analytics::exp_overview_handler),
        )
        .route(
            "/twitch/api/v2/exp/game-breakdown",
            get(exp_analytics::exp_game_breakdown_handler),
        )
        .route(
            "/twitch/api/v2/exp/game-transitions",
            get(exp_analytics::exp_game_transitions_handler),
        )
        .route(
            "/twitch/api/v2/exp/growth-curves",
            get(exp_analytics::exp_growth_curves_handler),
        )
        .route(
            "/twitch/api/v2/tag-analysis-extended",
            get(tag_analysis::tag_analysis_extended_handler),
        )
        .route(
            "/twitch/api/v2/monetization",
            get(monetization::monetization_handler),
        )
        .route(
            "/twitch/api/v2/watch-time-distribution",
            get(watch_time::watch_time_distribution_handler),
        )
        .route(
            "/twitch/api/v2/chat-social-graph",
            get(chat_social_graph::chat_social_graph_handler),
        )
        .route(
            "/twitch/api/v2/chat-hype-timeline",
            get(chat_hype_timeline::chat_hype_timeline_handler),
        )
        .route(
            "/twitch/api/v2/chat-content-analysis",
            get(chat_content_analysis::chat_content_analysis_handler),
        )
        .route(
            "/twitch/api/v2/coaching",
            get(coaching::coaching_handler),
        )
        .route(
            "/twitch/api/v2/chat-deep-minimax",
            get(chat_deep_minimax::chat_deep_minimax_handler),
        )
        .route(
            "/twitch/api/v2/chat-analytics",
            get(chat_analytics::chat_analytics_handler),
        )
        .route(
            "/twitch/api/v2/ai/history",
            get(ai_history::ai_history_handler),
        )
        .route(
            "/twitch/api/v2/ai/analysis",
            get(ai_analysis::ai_analysis_handler),
        )
        .route(
            "/twitch/api/v2/ai/chat",
            post(ai_chat::ai_chat_handler),
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
///
/// Lesend: Liste + Detail. Schreibend (B11-PR-4): verify/archive/block/
/// discord-flag als POST-Routen — CRUD in [`tb_analytics::streamers_crud`]. Die
/// Writes laufen wie der Admin-Config-Router durch den CSRF-Schutz (GET/HEAD
/// passieren, Localhost-Bypass für interne Tools).
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
        .route(
            "/twitch/api/admin/streamers/:login/verify",
            post(admin_streamers::verify_handler),
        )
        .route(
            "/twitch/api/admin/streamers/:login/archive",
            post(admin_streamers::archive_handler),
        )
        .route(
            "/twitch/api/admin/streamers/:login/block",
            post(admin_streamers::block_handler),
        )
        .route(
            "/twitch/api/admin/streamers/:login/discord-flag",
            post(admin_streamers::discord_flag_handler),
        )
        .with_state(pool)
        .layer(Extension(ExpectedToken(token)))
        .layer(axum::middleware::from_fn(crate::auth::csrf::csrf_protect))
}

/// Baut den Router für Admin-Config-Endpoints (Schreib-Seite).
pub fn build_admin_config_router(pool: PgPool, token: String) -> Router {
    use handlers::{
        admin_affiliate, admin_announcements, admin_audit_log, admin_billing, admin_config,
        admin_legal, admin_promo_mode, admin_roadmap,
    };

    Router::new()
        .route(
            "/twitch/api/admin/config/overview",
            get(admin_config::config_overview_handler),
        )
        .route(
            "/twitch/api/admin/audit-log",
            get(admin_audit_log::handler),
        )
        .route(
            "/twitch/api/admin/affiliates/stats",
            get(admin_affiliate::stats_handler),
        )
        .route(
            "/twitch/api/admin/affiliates",
            get(admin_affiliate::list_handler),
        )
        .route(
            "/twitch/api/admin/affiliates/gutschriften",
            get(admin_affiliate::gutschriften_handler),
        )
        .route(
            "/twitch/api/admin/affiliates/gutschriften/:gutschrift_id/pdf",
            get(admin_affiliate::gutschrift_pdf_handler),
        )
        .route(
            "/twitch/api/admin/affiliates/:login/gutschriften",
            get(admin_affiliate::gutschriften_for_login_handler),
        )
        .route(
            "/twitch/api/admin/affiliates/:login/toggle",
            post(admin_affiliate::toggle_handler),
        )
        .route(
            "/twitch/api/admin/affiliates/:login",
            get(admin_affiliate::detail_handler),
        )
        .route(
            "/twitch/api/admin/billing/subscriptions",
            get(admin_billing::subscriptions_handler),
        )
        .route(
            "/twitch/api/admin/billing/affiliates",
            get(admin_billing::affiliates_handler),
        )
        .route(
            "/twitch/api/admin/announcements",
            get(admin_announcements::get_handler).post(admin_announcements::save_handler),
        )
        .route(
            "/twitch/api/admin/roadmap",
            get(admin_roadmap::get_handler).post(admin_roadmap::save_handler),
        )
        .route(
            "/twitch/api/admin/legal/:slug",
            get(admin_legal::get_handler).post(admin_legal::save_handler),
        )
        .route(
            "/twitch/api/admin/config/promo",
            post(admin_promo_mode::set_promo_handler),
        )
        .route(
            "/twitch/api/admin/config/raids",
            post(admin_config::config_raids_handler),
        )
        .route(
            "/twitch/api/admin/config/chat",
            post(admin_config::config_chat_handler),
        )
        .with_state(pool)
        .layer(Extension(ExpectedToken(token)))
        // B3-7: CSRF-Schutz auf alle Admin-JSON-Writes (announcements/legal/roadmap/
        // promo/config-POST). Die Middleware lässt GET/HEAD durch, prüft Writes
        // gegen das sessiongebundene Token (Localhost-Bypass für interne Tools).
        .layer(axum::middleware::from_fn(crate::auth::csrf::csrf_protect))
}

/// Baut den Router für den nativen Twitch-OAuth-Dashboard-Login (B3-2).
///
/// Die drei GET-Routen werden NATIV registriert (vor dem Strangler-Proxy-
/// Fallback) und greifen damit, statt in den toten Python-Service (502) zu
/// fallen. `DashboardAuthState` + `OAuthLoginConfig` kommen als globale
/// Extensions aus der `tb-dashboard`-main (s. dort).
pub fn build_auth_router() -> Router {
    use handlers::auth_login;

    Router::new()
        .route("/twitch/auth/login", get(auth_login::login_handler))
        .route("/twitch/auth/callback", get(auth_login::callback_handler))
        .route("/twitch/auth/logout", get(auth_login::logout_handler))
}

/// Baut den Router für den Partner-Einmal-Login via HMAC One-Time-Token (B3-8).
///
/// - `POST /twitch/auth/partner/link` — Admin/Localhost stellt einen Einmal-Link
///   für einen Partner aus (HMAC-Token, State persistiert).
/// - `POST /twitch/auth/partner/login` — verbraucht den Token (atomar einmalig),
///   legt die Partner-Session an + setzt das Cookie + 302 ins Dashboard.
///
/// `DashboardAuthState`/`OAuthLoginConfig` kommen als globale Extensions aus der
/// `tb-dashboard`-main. Das HMAC-Secret (`TWITCH_PARTNER_TOKEN`) liest der Handler
/// aus dem Prozess-Env (Infisical); fehlt es, liefern beide Routen 503.
pub fn build_partner_login_router(pool: PgPool) -> Router {
    use handlers::partner_login;

    Router::new()
        .route("/twitch/auth/partner/link", post(partner_login::link_handler))
        .route("/twitch/auth/partner/login", post(partner_login::login_handler))
        .with_state(pool)
}

/// Baut den Router für die Admin-Legacy-Form-POST-Writes (B2-P1).
///
/// Diese Routen werden vom Legacy-Admin-Client (`submitLegacyAction`,
/// `admin_dashboard/.../client.ts`) als `application/x-www-form-urlencoded`
/// aufgerufen — inkl. `csrf_token` IM BODY (nicht im `X-CSRF-Token`-Header). Sie
/// dürfen daher NICHT durch die Header-basierte `csrf_protect`-Middleware laufen
/// (die würde sie immer ablehnen); die Handler validieren den Body-CSRF selbst
/// gegen die Session (Localhost-Bypass), genau wie Pythons `_read_post_with_csrf`.
///
/// - `POST /twitch/admin/manual-plan`       — Plan-Override setzen.
/// - `POST /twitch/admin/manual-plan/clear` — Plan-Override entfernen.
///
/// Antwort: `302` auf `/twitch/admin?ok=…`/`?err=…` (der Client folgt dem
/// Redirect und liest den Status aus dem Query). `DashboardAuthLevel` +
/// `DashboardAuthState` kommen als globale Extensions aus der `tb-dashboard`-main.
pub fn build_admin_legacy_forms_router(pool: PgPool) -> Router {
    use handlers::admin_manual_plan;

    Router::new()
        .route(
            "/twitch/admin/manual-plan",
            post(admin_manual_plan::save_handler),
        )
        .route(
            "/twitch/admin/manual-plan/clear",
            post(admin_manual_plan::clear_handler),
        )
        .with_state(pool)
}

/// Baut den Router für die Entry-/Redirect-Routen + die Admin-SPA + den
/// Forward-Auth-Endpoint (B1-ENTRY / B1-ADMIN-SPA / B1-ADMIN-FORWARD-AUTH).
///
/// Diese Routen werden NATIV registriert (vor dem Strangler-Proxy-Fallback),
/// damit der Admin-Host nativ lädt (kein 502 vom toten Python 8765 mehr). Der
/// `DashboardAuthLevel`-Extractor liest `DashboardAuthState` aus der globalen
/// Extension (in der `tb-dashboard`-main injiziert); ohne sie sind alle Requests
/// `None`/`Localhost` (fail-closed) — Admin-SPA + forward_auth liefern dann 401.
///
/// **Scope:** NUR Host + Auth + SPA-Shell + Entry-Redirects. Die Admin-Schreib-
/// Aktionen (add_streamer/verify/archive/discord_flag/manual-plan) sind separate
/// Folge-Tickets (B1-ADD-STREAMER/B1-VERIFY/B1-ARCHIVE/B1-DISCORD-LINK/
/// B1-MANUAL-PLAN) und hier bewusst NICHT enthalten.
pub fn build_entry_admin_router() -> Router {
    use handlers::{admin_spa, forward_auth};

    Router::new()
        // Forward-Auth für Caddys forward_auth auf dem Admin-Host.
        .route(
            "/twitch/auth/validate",
            get(forward_auth::validate_admin_session),
        )
        // Entry-/Redirect-Routen.
        .route("/", get(admin_spa::root_handler))
        .route("/twitch", get(admin_spa::twitch_index_handler))
        .route("/twitch/", get(admin_spa::twitch_index_handler))
        .route("/twitch/stats", get(admin_spa::dashboard_redirect_handler))
        .route("/twitch/dashboards", get(admin_spa::dashboard_redirect_handler))
        .route("/twitch/dashboads", get(admin_spa::dashboard_redirect_handler))
        .route("/dashboards", get(admin_spa::dashboard_redirect_handler))
        .route("/dashboads", get(admin_spa::dashboard_redirect_handler))
        // Admin-SPA: Shell + Deep-Link-Fallback + Assets.
        .route("/twitch/admin", get(admin_spa::admin_index_handler))
        .route("/twitch/admin/*path", get(admin_spa::admin_path_handler))
}

/// Baut den Router für den nativen Stripe-Webhook (B2-P0).
///
/// `POST /twitch/api/billing/stripe/webhook` — unauthentifiziert (Stripe ist der
/// Aufrufer; die `Stripe-Signature`-HMAC IST die Authentifizierung). NATIV
/// registriert (vor dem Strangler-Fallback), damit der umsatzkritische Pfad
/// nicht mehr in den toten Python-Service (502) läuft. Die `StripeWebhookConfig`
/// (Webhook-Secret + optionaler Stripe-Client) kommt als globale Extension aus
/// der `tb-dashboard`-main; fehlt sie, liefert der Handler 503.
pub fn build_billing_webhook_router(pool: PgPool) -> Router {
    use handlers::billing_webhook;

    Router::new()
        .route(
            "/twitch/api/billing/stripe/webhook",
            post(billing_webhook::stripe_webhook_handler),
        )
        .with_state(pool)
}

/// Baut den Router für den nativen Abo-/Billing-Bezahlpfad (Block 2A).
///
/// Routen werden NATIV (vor dem Strangler-Fallback) registriert, damit der
/// umsatzkritische Pfad nicht in den toten Python-Service (502) läuft:
/// - `GET /twitch/abbo` (+ `/abo`/`/abos`) → 301 auf `/twitch/pricing`.
/// - `GET /twitch/abbo/bezahlen` → Stripe-Checkout-Session + 302 zur hosted URL.
/// - `GET|POST /twitch/abbo/kündigen` → Stripe-Customer-Portal / cancel_at_period_end.
/// - `GET /twitch/api/billing/catalog` (+ `/v2/`) → Plan-Katalog + aktueller Plan.
/// - `GET /twitch/api/billing/readiness` → Stripe-Readiness (keine Secrets).
///
/// `DashboardAuthLevel` kommt aus der globalen `DashboardAuthState`-Extension
/// (Login-Gate). `BillingPageConfig` (Stripe-Client + Public-Origin) wird als
/// globale Extension in der `tb-dashboard`-main injiziert; fehlt sie, leiten
/// Checkout/Cancel mit `reason=...` um (kein 500), Katalog/Readiness melden
/// `checkout_ready=false`.
pub fn build_billing_page_router(pool: PgPool) -> Router {
    use handlers::billing_page;

    Router::new()
        .route("/twitch/abbo", get(billing_page::abbo_redirect_handler))
        .route("/twitch/abo", get(billing_page::abbo_redirect_handler))
        .route("/twitch/abos", get(billing_page::abbo_redirect_handler))
        .route(
            "/twitch/abbo/bezahlen",
            get(billing_page::checkout_start_handler),
        )
        .route(
            "/twitch/abbo/kündigen",
            get(billing_page::cancel_handler).post(billing_page::cancel_handler),
        )
        // B2-P1: Rechnungsempfänger-Profil speichern (Legacy-Form-POST, CSRF im
        // Body → in-handler validiert, daher KEIN Header-CSRF-Layer hier).
        .route(
            "/twitch/abbo/rechnungsdaten",
            post(handlers::billing_profile::profile_save_handler),
        )
        .route(
            "/twitch/api/billing/catalog",
            get(billing_page::catalog_handler),
        )
        .route(
            "/twitch/api/v2/billing/catalog",
            get(billing_page::catalog_handler),
        )
        .route(
            "/twitch/api/billing/readiness",
            get(billing_page::readiness_handler),
        )
        // B2-P1: Stripe Product/Price-Sync (Admin-only JSON-API).
        .route(
            "/twitch/api/billing/stripe/sync-products",
            post(handlers::billing_stripe_sync::sync_products_handler),
        )
        .with_state(pool)
}

/// Zusammengeführter Router: public + auth (Login) + billing-webhook + authed +
/// admin-system + admin-streamers + admin-config + Legal-Seiten (HTML, statuslos).
///
/// CORS nur auf dem Public-Sub-Router (s. oben).
pub fn build_router(pool: PgPool, token: String) -> Router {
    build_public_router(pool.clone())
        .merge(build_auth_router())
        .merge(build_partner_login_router(pool.clone()))
        .merge(handlers::discord_link::build_discord_link_router(pool.clone()))
        .merge(handlers::demo::build_demo_router())
        .merge(build_entry_admin_router())
        .merge(build_billing_webhook_router(pool.clone()))
        .merge(build_billing_page_router(pool.clone()))
        .merge(build_admin_legacy_forms_router(pool.clone()))
        .merge(build_authed_router(pool.clone(), token.clone()))
        .merge(build_admin_system_router(pool.clone(), token.clone()))
        .merge(build_admin_streamers_router(pool.clone(), token.clone()))
        .merge(build_admin_config_router(pool, token))
        .merge(handlers::legal::build_legal_router())
        .merge(handlers::roadmap_page::build_roadmap_page_router())
}

#[cfg(test)]
mod csrf_wiring_tests {
    //! B3-7: Verifiziert, dass der CSRF-Layer auf dem Admin-Config-Router liegt.
    //! DB-env-gated über `TB_TEST_DATABASE_URL` (echter PgPool nötig).
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use sqlx::postgres::PgPoolOptions;
    use tower::ServiceExt;

    async fn pool() -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        PgPoolOptions::new().max_connections(1).connect(&dsn).await.ok()
    }

    /// Nicht-Loopback-POST auf einen Admin-Write ohne CSRF-Token → 403 vom Layer
    /// (fail-closed, da keine DashboardAuthState-Extension). Beweist: Layer greift.
    #[tokio::test]
    async fn admin_write_ohne_csrf_403() {
        let Some(pool) = pool().await else { return };
        let app = build_admin_config_router(pool, "tok".into());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/twitch/api/admin/announcements")
                    .header("host", "dashboard.example.com")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    /// GET passiert den CSRF-Layer (Safe-Methode); ohne Auth liefert der Handler
    /// 401, aber NICHT das CSRF-403 — beweist, dass GET nicht vom Layer geblockt wird.
    #[tokio::test]
    async fn admin_read_passiert_csrf_layer() {
        let Some(pool) = pool().await else { return };
        let app = build_admin_config_router(pool, "tok".into());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/twitch/api/admin/announcements")
                    .header("host", "dashboard.example.com")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}
