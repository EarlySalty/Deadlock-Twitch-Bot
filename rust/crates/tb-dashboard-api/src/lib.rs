//! HTTP-Router für das Analytics-Dashboard.
//!
//! Öffentlicher Einstiegspunkt: `build_router(pool, token)`.
//! Kein Auth, kein Loopback-Gate bei public-Routen — explizit `CORS: *`.
//! Auth-Routen nutzen `AuthLevel`-Extractor aus tb-http-core.

pub mod admin_audit;
pub mod ai_state;
pub mod auth;
pub mod handlers;
pub mod obs;
pub mod process_info;
/// Strangler-Fig-Fallback-Proxy (→ Python 8765), siehe Modul-Doku.
pub mod proxy;
pub mod query_int;

pub use auth::csrf::csrf_protect;
pub use auth::discord_admin_login::{discord_admin_login_config_from_env, DiscordAdminLoginConfig};
pub use auth::level::DashboardAuthLevel;
pub use auth::oauth_login::{HelixOAuthClient, TwitchIdentity, TwitchOAuthClient};
pub use auth::security::{require_internal, RateLimiter};
pub use auth::session::{
    build_session_cookie, clear_session_cookie, DashboardAuthState, OAuthLoginState, SameSite,
    SessionCreation, ADMIN_COOKIE_NAME, OAUTH_STATE_SESSION_TYPE, OAUTH_STATE_TTL_SECS,
    PARTNER_COOKIE_NAME, SESSION_CREATE_TTL_SECS,
};
pub use handlers::affiliate::{
    affiliate_oauth_config_from_env, affiliate_stripe_config_from_env, AffiliateOAuthConfig,
    AffiliateStripeConfig,
};
pub use handlers::auth_login::{oauth_login_config_from_env, OAuthLoginConfig};
pub use handlers::billing_page::{billing_page_config_from_env, BillingPageConfig};
pub use handlers::billing_webhook::{stripe_webhook_config_from_env, StripeWebhookConfig};
pub use handlers::health_probe::{
    analytics_db_fingerprint_startup_check, AnalyticsDbFingerprintStartup,
};

pub use handlers::pause_loop::build_pause_loop_router;

use axum::{
    http::{header::HeaderName, HeaderValue},
    routing::{get, post, put},
    Extension, Router,
};
use sqlx::PgPool;
use tb_http_core::ExpectedToken;
use tb_transport_twitch::HelixClient;
use tower_http::{
    compression::CompressionLayer, cors::CorsLayer, set_header::SetResponseHeaderLayer,
    trace::TraceLayer,
};

use auth::security::{rate_limit_middleware, RateLimitLayerConfig};

/// Baut den globalen Default-Security-Header-Bundle (P2.108).
///
/// `SetResponseHeaderLayer::if_not_present` setzt jeden Header nur, wenn der
/// Handler ihn nicht selbst gesetzt hat — so überschreibt der Bundle keine
/// bewusst abweichenden Antworten. Auf ALLE Antworten von `build_router`
/// angewandt (auch Fehler/Redirects). Werte gemäß Plan P2.108:
/// - `X-Frame-Options: DENY` (Dashboard wird nirgends eingebettet)
/// - `X-Content-Type-Options: nosniff`
/// - `Referrer-Policy: strict-origin-when-cross-origin`
/// - `Cross-Origin-Opener-Policy: same-origin`
/// - `X-XSS-Protection: 0` (Legacy-Filter bewusst aus — moderne Empfehlung)
fn security_header_layers() -> [SetResponseHeaderLayer<HeaderValue>; 5] {
    fn layer(name: &'static str, value: &'static str) -> SetResponseHeaderLayer<HeaderValue> {
        SetResponseHeaderLayer::if_not_present(
            HeaderName::from_static(name),
            HeaderValue::from_static(value),
        )
    }
    [
        layer("x-frame-options", "DENY"),
        layer("x-content-type-options", "nosniff"),
        layer("referrer-policy", "strict-origin-when-cross-origin"),
        layer("cross-origin-opener-policy", "same-origin"),
        layer("x-xss-protection", "0"),
    ]
}

/// Baut den axum-Router für alle public Analytics-GET-Endpoints.
///
/// CORS-Policy: `CorsLayer::permissive()` NUR auf den Public-Routen —
/// Python setzt Access-Control-Header ausschließlich dort
/// (`api_public.py:52-58`). Authed/Admin-Routen bleiben ohne CORS-Header,
/// sonst wäre die Token-API cross-origin per Browser ansprechbar.
pub fn build_public_router(pool: PgPool) -> Router {
    use handlers::{
        bans, health_probe, network, network_stats, overlay, raids, self_explainer, social_media,
        streamer_comparison,
    };

    let public_api = Router::new()
        .route("/healthz", get(health_probe::healthz_handler))
        .route("/readyz", get(health_probe::readyz_handler))
        .route("/health", get(health_probe::readyz_handler))
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
        .route(
            "/twitch/api/v2/public/network-stats",
            get(network_stats::network_stats_handler),
        )
        .route(
            "/twitch/api/v2/public/streamer-comparison",
            get(streamer_comparison::streamer_comparison_handler),
        )
        .route(
            "/twitch/api/v2/public/overlay",
            get(overlay::overlay_api_handler),
        )
        // Roadmap (public GET + admin CRUD) liegt im eigenen build_roadmap_router,
        // damit der Admin-Write den ExpectedToken-Extractor sieht (axum erlaubt
        // denselben Pfad nicht in zwei gemergten Routern).
        .with_state(pool.clone())
        .layer(Extension(
            streamer_comparison::StreamerComparisonCache::default(),
        ))
        .layer(CorsLayer::permissive());

    // HTML, Login-Redirects und OAuth-Callbacks sind zwar öffentlich erreichbar,
    // aber keine browserübergreifende API. Wildcard-CORS auf diesen Antworten
    // vergrößert nur die Angriffsfläche und löste den ZAP-CORS-Fund aus.
    let public_pages = Router::new()
        .route("/twitch/overlay", get(overlay::overlay_html_handler))
        // Social-Media Rechtstexte — öffentlich für die Plattform-OAuth-Reviews.
        .route("/social-media/terms", get(social_media::terms_handler))
        .route("/social-media/privacy", get(social_media::privacy_handler))
        // OAuth-Callback — öffentlich (Provider-Redirect, Security via State-Token).
        .route(
            "/social-media/oauth/callback",
            get(social_media::oauth_callback_handler),
        )
        .route(
            "/social-media/oauth/callback/:platform",
            get(social_media::oauth_callback_handler),
        )
        .with_state(pool);

    public_api.merge(public_pages)
}

/// Baut den Router für Admin-geschützte Routes.
///
/// Auth-Level wird per Extension eingesetzt — `AuthLevel` als `FromRequestParts`
/// liest den Token selbst aus der Extension.
pub fn build_authed_router(pool: PgPool, token: String, rate_limiter: RateLimiter) -> Router {
    use handlers::scam_guard_enforce;
    use handlers::{
        ads_schedule, affiliate_portal, ai_analysis, ai_chat, ai_history, audience,
        audience_demographics, auth_status, billing, category_activity, category_comparison,
        category_leaderboard, category_timings, chat_analytics, chat_content_analysis,
        chat_deep_minimax, chat_hype_timeline, chat_social_graph, clip_command_settings, coaching,
        engagement_mode, engagement_settings, exp_analytics, follower_funnel, greeting_settings,
        internal_home, leaderboard, loyalty_curve, lurk_command_settings, lurker_analysis,
        lurker_tax_settings, monetization, onboarding, overview, performance, raid_analytics,
        raid_history, rankings, retention_curve, scam_guard_queue, scam_guard_settings,
        session_detail, silent_settings, social_media, spa, stream_report, streamer_disconnect,
        streamers, tag_analysis, tip_settings, title, title_performance, uplink, viewer_timeline,
        viewers, watch_time,
    };

    // P2.86: Rate-Limit-Layer für die gebündelte Internal-Home-Startseite (GET +
    // Changelog-POST). Bucket "internal_home", 60 Requests/60 s pro Client-IP.
    let internal_home_rl = RateLimitLayerConfig::new(rate_limiter.clone(), "internal_home", 60, 60);
    // Verbinden-Flow wie der Login: 30 Aufrufe pro Minute und Client-IP.

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
        .route(
            "/social-media/api/last-hashtags",
            get(social_media::last_hashtags_handler),
        )
        // Partner-Freigabe: Liste + Setzen/Entfernen (Admin-only).
        .route(
            "/social-media/api/access",
            get(social_media::partner_access_get_handler)
                .put(social_media::partner_access_put_handler),
        )
        // Was die eigene Session darf (Partner und Admin).
        .route(
            "/social-media/api/access/me",
            get(social_media::my_access_handler),
        )
        // Analytics-Ansicht (identisch zu stats) + Queue-Upload.
        .route(
            "/social-media/api/analytics",
            get(social_media::stats_handler),
        )
        .route(
            "/social-media/api/upload",
            post(social_media::queue_upload_handler),
        )
        .route(
            "/social-media/api/mark-uploaded",
            post(social_media::mark_uploaded_handler),
        )
        .route(
            "/social-media/api/batch-upload",
            post(social_media::batch_upload_handler),
        )
        .route(
            "/social-media/api/fetch-clips",
            post(social_media::fetch_clips_handler),
        )
        // Multipart-Datei-Upload — eigenes 201MB-Body-Limit (Default ist 2MB).
        .route(
            "/social-media/api/clips/upload",
            post(social_media::upload_clip_handler)
                .layer(axum::extract::DefaultBodyLimit::max(201 * 1024 * 1024)),
        )
        // Templates: globale + Streamer-Listen (GET), anlegen + anwenden (POST).
        .route(
            "/social-media/api/templates/global",
            get(social_media::templates_global_handler),
        )
        .route(
            "/social-media/api/templates/streamer",
            get(social_media::templates_streamer_handler)
                .post(social_media::create_template_handler),
        )
        .route(
            "/social-media/api/templates/apply",
            post(social_media::apply_template_handler),
        )
        // Layout-CRUD (Admin): Streamer-Default + Clip-Override.
        .route(
            "/social-media/api/admin/streamer-layout",
            get(social_media::streamer_layout_get_handler)
                .put(social_media::streamer_layout_put_handler),
        )
        .route(
            "/social-media/api/admin/clips/:clip_db_id/layout",
            axum::routing::put(social_media::clip_layout_put_handler),
        )
        // Vocab-CRUD (Admin): Liste/Upsert, Löschen, Seed.
        .route(
            "/social-media/api/admin/vocab",
            get(social_media::vocab_list_handler).post(social_media::vocab_upsert_handler),
        )
        .route(
            "/social-media/api/admin/vocab/seed",
            post(social_media::vocab_seed_handler),
        )
        .route(
            "/social-media/api/admin/vocab/:term",
            axum::routing::delete(social_media::vocab_delete_handler),
        )
        // Plattform-Verbindungsstatus (verschlüsselte Credentials).
        .route(
            "/social-media/api/platforms/status",
            get(social_media::platforms_status_handler),
        )
        // Admin-Clips: paginierte Liste, Detail, Verwerfen.
        .route(
            "/social-media/api/admin/clips",
            get(social_media::admin_clips_handler),
        )
        .route(
            "/social-media/api/admin/clips/:clip_db_id",
            get(social_media::admin_clip_detail_handler),
        )
        .route(
            "/social-media/api/admin/clips/:clip_db_id/discard",
            post(social_media::admin_clip_discard_handler),
        )
        // Approval-State + Entscheidung, Auto-Approve-Settings (Admin).
        .route(
            "/social-media/api/admin/approval/:clip_db_id",
            get(social_media::approval_get_handler),
        )
        .route(
            "/social-media/api/admin/approval/:clip_db_id/decision",
            post(social_media::approval_decision_handler),
        )
        // Eingeplante Uploads wieder stoppen (Veto-Fenster).
        .route(
            "/social-media/api/approval/:clip_db_id/cancel",
            post(social_media::approval_cancel_handler),
        )
        // Zeitplan, Freigabe-Modus, Kategorien und Vorratsrechnung je Kanal.
        // Loest die frueheren globalen Auto-Approve-Flags ab.
        .route(
            "/social-media/api/admin/settings/posting-plan",
            get(social_media::posting_plan_get_handler).put(social_media::posting_plan_put_handler),
        )
        .route(
            "/social-media/api/admin/settings/posting-plan/platform/:platform",
            put(social_media::posting_plan_platform_put_handler),
        )
        .route(
            "/social-media/api/admin/settings/posting-plan/category/:category_key",
            put(social_media::posting_plan_category_put_handler),
        )
        .route(
            "/social-media/api/admin/settings/vod-archive",
            get(social_media::vod_archive_get_handler).put(social_media::vod_archive_put_handler),
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
        .route(
            "/social-media/api/admin/analytics/clips/:clip_db_id",
            get(social_media::clip_analytics_get_handler),
        )
        .route(
            "/social-media/api/admin/reports",
            get(social_media::reports_list_handler),
        )
        .route(
            "/social-media/api/admin/reports/run",
            post(social_media::reports_run_handler),
        )
        // OAuth-Start + Disconnect (Auth erforderlich).
        .route(
            "/social-media/oauth/start/:platform",
            get(social_media::oauth_start_handler),
        )
        .route(
            "/social-media/oauth/disconnect/:platform",
            post(social_media::oauth_disconnect_handler),
        )
        // Internal-Home: gebündelte Dashboard-Startseite (Profil, KPIs, Bot-Events,
        // Changelog). GET liest, POST legt einen Changelog-Eintrag an (Admin-only).
        .route(
            "/twitch/api/v2/internal-home",
            get(internal_home::get_handler).layer(axum::middleware::from_fn_with_state(
                internal_home_rl.clone(),
                rate_limit_middleware,
            )),
        )
        .route(
            "/twitch/api/v2/internal-home/changelog",
            post(internal_home::changelog_handler).layer(axum::middleware::from_fn_with_state(
                internal_home_rl,
                rate_limit_middleware,
            )),
        )
        // Streamer-Selbstbedienung: Silent-Notification-Flags (sync zu !silentban/
        // !silentraid). GET liest, POST setzt beide Flags auf twitch_partners.
        .route(
            "/twitch/api/v2/streamer/silent-settings",
            get(silent_settings::get_handler).post(silent_settings::post_handler),
        )
        // Streamer-Selbstbedienung: Go-Live-Tipp-Opt-out. Partner setzen nur
        // die eigene `twitch_tip_settings`-Zeile über ihre Session-User-ID.
        .route(
            "/twitch/api/v2/streamer/tip-settings",
            get(tip_settings::get_handler).post(tip_settings::post_handler),
        )
        // Streamer-Selbstbedienung: resumierbarer Onboarding-Wizard inklusive
        // Discord-/Steam-Link-Status.
        .route(
            "/twitch/api/v2/streamer/onboarding",
            get(onboarding::get_status).post(onboarding::post_status),
        )
        // Streamer-Selbstbedienung: Conversation-Scam-Guard konfigurieren
        // (enabled, mode, threshold, suggestion_floor).
        .route(
            "/twitch/api/v2/streamer/scam-guard/settings",
            get(scam_guard_settings::get_handler).post(scam_guard_settings::post_handler),
        )
        // Scam-Guard: Vorschlags-Queue lesen, Verdict-Detail, Vorschlag verwerfen.
        .route(
            "/twitch/api/v2/streamer/scam-guard/queue",
            get(scam_guard_queue::queue_handler),
        )
        .route(
            "/twitch/api/v2/streamer/scam-guard/verdicts/:id",
            get(scam_guard_queue::detail_handler),
        )
        .route(
            "/twitch/api/v2/streamer/scam-guard/queue/:id/ignore",
            post(scam_guard_queue::ignore_handler),
        )
        .route(
            "/twitch/api/v2/streamer/scam-guard/queue/:id/ban",
            post(scam_guard_enforce::ban_handler),
        )
        .route(
            "/twitch/api/v2/streamer/scam-guard/verdicts/:id/revoke",
            post(scam_guard_enforce::revoke_handler),
        )
        // Streamer-Selbstbedienung: Lurker-Steuer-Toggle (B9, sync zu
        // !lurkersteuer_off). Default deaktiviert, alle Partner. Spalte
        // streamer_plans.lurker_tax_enabled.
        .route(
            "/twitch/api/v2/streamer/lurker-tax-settings",
            get(lurker_tax_settings::get_handler).post(lurker_tax_settings::post_handler),
        )
        // Streamer-Selbstbedienung: !lurk-Command-Toggle. Default aktiviert
        // (bestehendes Verhalten), Spalte streamer_plans.lurk_command_enabled.
        .route(
            "/twitch/api/v2/streamer/lurk-command-settings",
            get(lurk_command_settings::get_handler).post(lurk_command_settings::post_handler),
        )
        // Streamer-Selbstbedienung: !clip-Command-Toggle. Default aktiviert
        // (bestehendes Verhalten), Spalte streamer_plans.clip_command_enabled.
        .route(
            "/twitch/api/v2/streamer/clip-command-settings",
            get(clip_command_settings::get_handler).post(clip_command_settings::post_handler),
        )
        // Streamer-Selbstbedienung: automatischer Rückgruß im Chat. Default
        // aktiviert (bestehendes Verhalten), Spalte
        // streamer_plans.greeting_reply_enabled.
        .route(
            "/twitch/api/v2/streamer/greeting-settings",
            get(greeting_settings::get_handler).post(greeting_settings::post_handler),
        )
        // Streamer-Selbstbedienung: Bot bewusst vom eigenen Kanal trennen.
        // Gleiche Kette wie die Admin-Route, Login kommt aber aus der Session.
        .route(
            "/twitch/api/v2/streamer/disconnect-bot",
            post(streamer_disconnect::post_handler),
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
        .route("/twitch/api/v2/uplink/me", get(uplink::me_handler))
        .route(
            "/twitch/api/v2/uplink/reconnect-wait",
            put(uplink::put_reconnect_wait_handler),
        )
        .route(
            "/twitch/api/v2/uplink/waitlist",
            post(uplink::waitlist_handler),
        )
        .route(
            "/twitch/api/v2/uplink/admin/waitlist",
            get(uplink::admin_waitlist_handler),
        )
        .route(
            "/twitch/api/v2/uplink/admin/waitlist/:streamer_id",
            axum::routing::delete(uplink::admin_ablehnen_handler),
        )
        .route(
            "/twitch/api/v2/uplink/admin/users",
            post(uplink::admin_freischalten_handler),
        )
        .route(
            "/twitch/api/v2/uplink/destinations",
            get(uplink::destinations_handler).put(uplink::put_destination_handler),
        )
        .route("/twitch/api/v2/uplink/caps", get(uplink::caps_handler))
        // Uplink Multi-Chat: Verbinden laeuft ueber den bestehenden
        // Streamer-OAuth (`/twitch/raid/auth?scope_profile=uplink`), es gibt
        // also keinen eigenen Start und keinen eigenen Callback mehr. Was
        // bleibt, ist das Trennen und die neue Dock-Adresse.
        .route(
            "/twitch/api/v2/uplink/connect/:platform/disconnect",
            post(uplink::disconnect_handler),
        )
        .route(
            "/twitch/api/v2/uplink/connect/:platform/streamkey",
            post(uplink::streamkey_handler),
        )
        .route(
            "/twitch/api/v2/uplink/dock-token/rotate",
            post(uplink::dock_token_rotate_handler),
        )
        // P3.5: Admin-Raid-Historie (Login-Filter `from`/`from_broadcaster`,
        // `limit` 1..=500, Default 50). Admin-Gate via DashboardAuthLevel.
        .route(
            "/twitch/raid/history",
            get(raid_history::raid_history_handler),
        )
        .route("/twitch/api/v2/title/suggest", post(title::suggest_handler))
        .route(
            "/twitch/api/v2/title/insights",
            get(title::insights_handler),
        )
        .route(
            "/twitch/api/v2/channel/title",
            axum::routing::patch(title::update_channel_title_handler),
        )
        .route(
            "/twitch/api/v2/affiliate/portal",
            get(affiliate_portal::portal_handler),
        )
        .route(
            "/twitch/api/affiliate/commissions",
            get(affiliate_portal::commissions_handler),
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
            "/twitch/api/v2/viewer-timeline",
            get(performance::viewer_count_timeline_handler),
        )
        .route("/twitch/api/v2/rankings", get(rankings::rankings_handler))
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
        .route("/twitch/api/v2/coaching", get(coaching::coaching_handler))
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
        .route("/twitch/api/v2/ai/chat", post(ai_chat::ai_chat_handler))
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
        .layer(axum::middleware::from_fn(
            crate::auth::partner_gate::partner_status_gate,
        ))
        // CSRF auf allen Write-Actions des authed-Routers (Grillme-Direktive
        // "CSRF auf allen Write-Actions"). Header-basierter csrf_protect lässt
        // GET/HEAD + Localhost (interne Loopback-Tools, z. B. Changelog-Spiegelung)
        // passieren, verlangt sonst den X-CSRF-Token; schützt lurker-tax/
        // engagement-mode/silent + social-media-/billing-Writes. Body-CSRF-Forms
        // (manual-plan) liegen im separaten build_admin_legacy_forms_router und
        // sind daher NICHT betroffen.
        .layer(axum::middleware::from_fn(crate::auth::csrf::csrf_protect))
}

/// Baut den Router für Admin-System-Endpoints.
pub fn build_admin_system_router(pool: PgPool, token: String) -> Router {
    use handlers::system::{database, errors, eventsub, health, oauth_scopes, query};

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
        // P2.74: OAuth-Scope-Audit (Scope-Diff-Panel).
        .route(
            "/twitch/api/admin/system/oauth-scopes",
            get(oauth_scopes::oauth_scopes_handler),
        )
        // P2.77/P2.81: Read-only Admin-SQL-Konsole (SELECT-only, Blocklist,
        // READ-ONLY-Transaktion, LIMIT 200).
        .route("/twitch/api/admin/system/query", get(query::query_handler))
        .with_state(pool)
        .layer(Extension(ExpectedToken(token)))
        .layer(axum::middleware::from_fn(
            crate::auth::level::promote_dashboard_admin_session,
        ))
}

/// Baut den Router für Admin-Streamer-Endpoints.
///
/// Lesend: Liste + Detail. Schreibend (B11-PR-4): verify/archive/block/
/// discord-flag als POST-Routen — CRUD in [`tb_analytics::streamers_crud`]. Die
/// Writes laufen wie der Admin-Config-Router durch den CSRF-Schutz (GET/HEAD
/// passieren, Localhost-Bypass für interne Tools).
pub fn build_admin_streamers_router(pool: PgPool, token: String) -> Router {
    use handlers::{admin_research, admin_scout, admin_streamers, social_media};

    Router::new()
        .route(
            "/twitch/api/admin/research/suggestions",
            get(admin_research::suggestions_handler),
        )
        .route(
            "/twitch/api/admin/research/:login",
            get(admin_research::handler),
        )
        // Scout-Freigaben: Kandidatenliste (mit Erkennungs-Lauf) und
        // Admin-Entscheidung; dieselben Schutzschichten wie die Research- und
        // Streamer-Routen oben (require_admin_before_csrf + csrf_protect).
        .route(
            "/twitch/api/admin/scout/candidates",
            get(admin_scout::candidates_handler),
        )
        .route(
            "/twitch/api/admin/scout/candidates/:login/decision",
            post(admin_scout::decision_handler),
        )
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
            "/twitch/api/admin/streamers/:login/disconnect-bot",
            post(admin_streamers::disconnect_bot_handler),
        )
        .route(
            "/twitch/api/admin/streamers/:login/discord-flag",
            post(admin_streamers::discord_flag_handler),
        )
        // Partner-Freigabe unter dem Admin-Prefix: dieselben Handler wie
        // `/social-media/api/access`, aber auf einem Pfad, den die
        // Admin-Subdomain durchlässt. `/social-media/*` ist dort laut
        // HOST_ROUTING_CONTRACT.md bewusst 404, und die Admin-SPA läuft
        // ausschließlich auf dieser Subdomain.
        .route(
            "/twitch/api/admin/partner-access",
            get(social_media::partner_access_get_handler)
                .put(social_media::partner_access_put_handler),
        )
        .with_state(pool)
        .layer(axum::middleware::from_fn(crate::auth::csrf::csrf_protect))
        .layer(axum::middleware::from_fn(
            crate::auth::require_admin_before_csrf,
        ))
        .layer(axum::middleware::from_fn(
            crate::auth::level::promote_dashboard_admin_session,
        ))
        .layer(Extension(ExpectedToken(token)))
}

/// Baut den Router für Admin-Config-Endpoints (Schreib-Seite).
pub fn build_admin_config_router(pool: PgPool, token: String) -> Router {
    use handlers::{
        admin_affiliate, admin_announcements, admin_audit_log, admin_billing, admin_config,
        admin_global_ban, admin_legal, admin_partner_signup_block, admin_promo_mode, admin_roadmap,
    };

    Router::new()
        .route(
            "/twitch/api/admin/config/overview",
            get(admin_config::config_overview_handler),
        )
        .route("/twitch/api/admin/audit-log", get(admin_audit_log::handler))
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
            "/twitch/api/admin/affiliates/generate-gutschriften",
            post(admin_affiliate::generate_gutschriften_handler),
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
            "/twitch/api/admin/affiliates/:login/commission-rate",
            post(admin_affiliate::set_commission_rate_handler),
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
            "/twitch/api/admin/global-bans",
            get(admin_global_ban::list_handler),
        )
        .route(
            "/twitch/api/admin/global-bans/add",
            post(admin_global_ban::add_handler),
        )
        .route(
            "/twitch/api/admin/global-bans/remove",
            post(admin_global_ban::remove_handler),
        )
        .route(
            "/twitch/api/admin/global-bans/channels/:login",
            post(admin_global_ban::set_channel_handler),
        )
        .route(
            "/twitch/api/admin/partner-signup-blocks",
            get(admin_partner_signup_block::list_handler)
                .post(admin_partner_signup_block::add_handler),
        )
        .route(
            "/twitch/api/admin/partner-signup-blocks/remove",
            post(admin_partner_signup_block::remove_handler),
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
        // B3-7: CSRF-Schutz auf alle Admin-JSON-Writes (announcements/legal/roadmap/
        // promo/config-POST). Die Middleware lässt GET/HEAD durch, prüft Writes
        // gegen das sessiongebundene Token (Localhost-Bypass für interne Tools).
        .layer(axum::middleware::from_fn(crate::auth::csrf::csrf_protect))
        .layer(axum::middleware::from_fn(
            crate::auth::require_admin_before_csrf,
        ))
        .layer(axum::middleware::from_fn(
            crate::auth::level::promote_dashboard_admin_session,
        ))
        .layer(Extension(ExpectedToken(token)))
}

/// Baut den Router für den nativen Twitch-OAuth-Dashboard-Login (B3-2).
///
/// Die drei GET-Routen werden NATIV registriert (vor dem Strangler-Proxy-
/// Fallback) und greifen damit, statt in den toten Python-Service (502) zu
/// fallen. `DashboardAuthState` + `OAuthLoginConfig` kommen als globale
/// Extensions aus der `tb-dashboard`-main (s. dort).
pub fn build_auth_router(rate_limiter: RateLimiter) -> Router {
    use auth::discord_admin_login;
    use handlers::auth_login;

    // P2.138: Login-Bucket (30/60 s). P2.140: Callback-Bucket (30/60 s) — die
    // beiden Callback-Routen teilen sich denselben Bucket. Layer pro Route, damit
    // Login und Callback getrennte Kontingente haben.
    let login_rl = RateLimitLayerConfig::new(rate_limiter.clone(), "auth_login", 30, 60);
    let callback_rl = RateLimitLayerConfig::new(rate_limiter.clone(), "auth_callback", 30, 60);
    let discord_login_rl =
        RateLimitLayerConfig::new(rate_limiter.clone(), "discord_admin_login", 10, 60);
    let discord_callback_rl =
        RateLimitLayerConfig::new(rate_limiter, "discord_admin_callback", 20, 60);

    Router::new()
        .route(
            "/twitch/auth/login",
            get(auth_login::login_handler).layer(axum::middleware::from_fn_with_state(
                login_rl,
                rate_limit_middleware,
            )),
        )
        .route(
            "/twitch/auth/callback",
            get(auth_login::callback_handler).layer(axum::middleware::from_fn_with_state(
                callback_rl.clone(),
                rate_limit_middleware,
            )),
        )
        .route(
            "/callback/twitch",
            get(auth_login::shared_callback_handler).layer(axum::middleware::from_fn_with_state(
                callback_rl,
                rate_limit_middleware,
            )),
        )
        .route("/twitch/auth/logout", get(auth_login::logout_handler))
        .route(
            "/twitch/auth/discord/login",
            get(discord_admin_login::login_handler).layer(axum::middleware::from_fn_with_state(
                discord_login_rl,
                rate_limit_middleware,
            )),
        )
        .route(
            "/callback/discord",
            get(discord_admin_login::shared_callback_handler).layer(
                axum::middleware::from_fn_with_state(
                    discord_callback_rl.clone(),
                    rate_limit_middleware,
                ),
            ),
        )
        .route(
            "/twitch/auth/discord/complete",
            get(discord_admin_login::complete_handler).layer(axum::middleware::from_fn_with_state(
                discord_callback_rl.clone(),
                rate_limit_middleware,
            )),
        )
        .route(
            "/twitch/auth/discord/callback",
            get(discord_admin_login::complete_handler).layer(axum::middleware::from_fn_with_state(
                discord_callback_rl,
                rate_limit_middleware,
            )),
        )
        .route(
            "/twitch/auth/discord/logout",
            get(discord_admin_login::logout_handler),
        )
        .route(
            "/twitch/auth/fingerprint",
            get(discord_admin_login::fingerprint_page_handler)
                .post(discord_admin_login::fingerprint_submit_handler),
        )
}

/// Baut den Router für Affiliate-Onboarding (separate Affiliate-Session,
/// `session_type='affiliate'`, kein Partner-Gate).
pub fn build_affiliate_router(pool: PgPool, rate_limiter: RateLimiter) -> Router {
    use handlers::{admin_affiliate, affiliate};

    let login_rl = RateLimitLayerConfig::new(rate_limiter.clone(), "affiliate_auth_login", 30, 60);
    let callback_rl =
        RateLimitLayerConfig::new(rate_limiter.clone(), "affiliate_auth_callback", 30, 60);
    let stripe_rl = RateLimitLayerConfig::new(rate_limiter, "affiliate_stripe_connect", 30, 60);

    Router::new()
        .route(
            "/twitch/auth/affiliate/login",
            get(affiliate::auth_login_handler).layer(axum::middleware::from_fn_with_state(
                login_rl,
                rate_limit_middleware,
            )),
        )
        .route(
            "/twitch/auth/affiliate/callback",
            get(affiliate::auth_callback_handler).layer(axum::middleware::from_fn_with_state(
                callback_rl,
                rate_limit_middleware,
            )),
        )
        .route(
            "/twitch/affiliate/connect/stripe",
            get(affiliate::connect_stripe_handler).layer(axum::middleware::from_fn_with_state(
                stripe_rl.clone(),
                rate_limit_middleware,
            )),
        )
        .route(
            "/twitch/affiliate/connect/stripe/callback",
            get(affiliate::connect_stripe_callback_handler).layer(
                axum::middleware::from_fn_with_state(stripe_rl, rate_limit_middleware),
            ),
        )
        .route("/twitch/affiliate/claim", post(affiliate::claim_handler))
        .route("/twitch/api/affiliate/me", get(affiliate::api_me_handler))
        .route(
            "/twitch/api/affiliate/profile",
            put(affiliate::api_profile_update_handler),
        )
        .route(
            "/twitch/api/affiliate/claims",
            get(affiliate::api_claims_handler),
        )
        .route(
            "/twitch/api/affiliate/gutschriften",
            get(affiliate::api_gutschriften_handler),
        )
        .route(
            "/twitch/api/affiliate/gutschriften/:gutschrift_id/pdf",
            get(affiliate::api_gutschrift_pdf_handler),
        )
        .route(
            "/twitch/api/affiliate/gutschriften/trigger",
            post(admin_affiliate::generate_gutschriften_trigger_handler),
        )
        .route(
            "/twitch/api/affiliate/admin/generate-gutschriften",
            post(admin_affiliate::generate_gutschriften_trigger_handler),
        )
        .with_state(pool)
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
pub fn build_partner_login_router(pool: PgPool, rate_limiter: RateLimiter) -> Router {
    use handlers::partner_login;

    // P2.133: getrennte Buckets — Link-Ausstellung (10/60 s) und Token-Verbrauch
    // (20/60 s) je Client-IP.
    let link_rl = RateLimitLayerConfig::new(rate_limiter.clone(), "partner_link", 10, 60);
    let login_rl = RateLimitLayerConfig::new(rate_limiter, "partner_login", 20, 60);

    Router::new()
        .route(
            "/twitch/auth/partner/link",
            post(partner_login::link_handler).layer(axum::middleware::from_fn_with_state(
                link_rl,
                rate_limit_middleware,
            )),
        )
        .route(
            "/twitch/auth/partner/login",
            post(partner_login::login_handler).layer(axum::middleware::from_fn_with_state(
                login_rl,
                rate_limit_middleware,
            )),
        )
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
    use handlers::{
        admin_chat_action, admin_form_aliases, admin_legacy_streamers, admin_manual_plan,
    };

    Router::new()
        .route(
            "/twitch/admin/manual-plan",
            post(admin_manual_plan::save_handler),
        )
        .route(
            "/twitch/admin/manual-plan/clear",
            post(admin_manual_plan::clear_handler),
        )
        // Welle-2-A1: native Admin-Streamer-Verwaltung (P0.1/P1.46/P2.112/P2.121).
        // Vor dem Strangler-Proxy-Fallback registriert (s. build_router), damit
        // diese Form-POSTs nativ greifen statt in den toten Python 8765 (502).
        .route(
            "/twitch/add_streamer",
            post(admin_legacy_streamers::add_streamer_handler),
        )
        .route(
            "/twitch/add_url",
            post(admin_legacy_streamers::add_any_handler),
        )
        .route(
            "/twitch/add_login",
            post(admin_legacy_streamers::add_any_handler),
        )
        .route(
            "/twitch/add_any",
            post(admin_legacy_streamers::add_any_handler),
        )
        .route(
            "/twitch/remove",
            post(admin_legacy_streamers::remove_handler),
        )
        .route(
            "/twitch/discord_link",
            post(admin_legacy_streamers::discord_link_handler),
        )
        // Admin-Bare-Form-Pfade aus der Python-Live-Tabelle
        // (`bot/dashboard/live/live.py`): weiterhin von Legacy-HTML/Client genutzt.
        .route("/twitch/verify", post(admin_form_aliases::verify_handler))
        .route("/twitch/archive", post(admin_form_aliases::archive_handler))
        .route(
            "/twitch/discord_flag",
            post(admin_form_aliases::discord_flag_handler),
        )
        // Welle-2-A1: native Admin-Partner-Chat-Aktion (P2.120) mit Owner-Gate
        // (P2.119); Send wird über die Bot-internal-API gebrückt.
        .route(
            "/twitch/admin/chat_action",
            post(admin_chat_action::chat_action_handler),
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
        .route(
            "/twitch/dashboards",
            get(admin_spa::dashboard_redirect_handler),
        )
        .route(
            "/twitch/dashboads",
            get(admin_spa::dashboard_redirect_handler),
        )
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
/// - `GET /twitch/abbo/rechnungen` → Stripe-Customer-Portal oder Pricing-Redirect.
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
        .route(
            "/twitch/abbo/rechnungen",
            get(billing_page::legacy_invoices_redirect_handler),
        )
        // B2-P1: Rechnungsempfänger-Profil speichern (Legacy-Form-POST, CSRF im
        // Body → in-handler validiert, daher KEIN Header-CSRF-Layer hier).
        .route(
            "/twitch/abbo/rechnungsdaten",
            post(handlers::billing_profile::profile_save_handler),
        )
        .route(
            "/twitch/abbo/promo-message",
            post(billing_page::promo_message_handler),
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
        // Checkout-Vorschau: meldet, ob ein Plan checkout-bereit ist (ohne Stripe-
        // Session anzulegen). CSRF im Body → in-handler validiert.
        .route(
            "/twitch/api/billing/checkout-preview",
            post(billing_page::checkout_preview_handler),
        )
        // B2-P1: Stripe Product/Price-Sync (Admin-only JSON-API).
        .route(
            "/twitch/api/billing/stripe/sync-products",
            post(handlers::billing_stripe_sync::sync_products_handler),
        )
        .with_state(pool)
}

/// Baut den Router für die Roadmap (public GET + admin CRUD, P1.31).
///
/// Aus dem Public-Router herausgelöst, weil das Admin-CRUD den
/// `AuthLevel`-Extractor (und damit die `ExpectedToken`-Extension) braucht;
/// axum erlaubt denselben Pfad nicht in zwei gemergten Routern. GET bleibt
/// öffentlich lesbar (CORS), POST/PATCH/DELETE laufen durch den CSRF-Layer
/// (GET/HEAD passieren) und sind admin-gegated (Handler-`is_privileged`).
pub fn build_roadmap_router(pool: PgPool, token: String) -> Router {
    use handlers::roadmap;

    Router::new()
        .route(
            "/twitch/api/v2/roadmap",
            get(roadmap::get_handler).post(roadmap::create_handler),
        )
        .route(
            "/twitch/api/v2/roadmap/:id",
            axum::routing::patch(roadmap::update_handler).delete(roadmap::delete_handler),
        )
        .with_state(pool)
        .layer(Extension(ExpectedToken(token)))
        .layer(axum::middleware::from_fn(
            crate::auth::level::promote_dashboard_admin_session,
        ))
        .layer(axum::middleware::from_fn(crate::auth::csrf::csrf_protect))
        .layer(CorsLayer::permissive())
}

/// Baut den Router für die Market-Research-Endpunkte (P2.104/105/106).
///
/// - `GET /twitch/market` — gerenderte Market-Research-HTML-Seite.
/// - `GET /twitch/api/market_data` — aggregierte Markt-Daten (JSON).
/// - `GET /twitch/api/v2/market-share` — Admin-Wrapper auf native Analytics.
///
/// Alle drei sind admin/localhost-gegated (Handler-intern via
/// `DashboardAuthLevel`). `market-share` nutzt direkt `tb_analytics::market`
/// und braucht keinen HTTP-Hop zum internen Worker.
pub fn build_market_router(pool: PgPool, token: String) -> Router {
    use handlers::market;

    Router::new()
        .route("/twitch/market", get(market::market_research_handler))
        .route(
            "/twitch/api/market_data",
            get(market::api_market_data_handler),
        )
        .route(
            "/twitch/api/v2/market-share",
            get(market::api_market_share_handler),
        )
        .with_state(pool)
        .layer(Extension(ExpectedToken(token)))
}

/// Baut den Router für die nativen Raid-Dashboard-Routen (P1.51/P1.52).
///
/// - `GET /twitch/raid/auth` — startet den Raid-OAuth-Flow (302 → Twitch).
/// - `GET /twitch/raid/go`   — Kurz-Redirect für Discord-Buttons (302 → Twitch).
/// - `GET /twitch/api/raid/analytics` — Admin-JSON für Raid-Netzwerk-Analytics.
///
/// `DashboardAuthLevel` kommt aus der globalen `DashboardAuthState`-Extension;
/// die Handler bridgen über die Internal-API (`X-Internal-Token`).
pub fn build_raid_pages_router(pool: PgPool) -> Router {
    use handlers::{obsolete_routes, raid_network_analytics, raid_pages, raid_requirements};

    Router::new()
        .route("/twitch/raid/auth", get(raid_pages::raid_auth_handler))
        .route("/twitch/raid/go", get(raid_pages::raid_go_handler))
        .route(
            "/twitch/raid/callback",
            get(obsolete_routes::raid_callback_gone_handler),
        )
        .route(
            "/twitch/raid/requirements",
            get(raid_requirements::raid_requirements_handler),
        )
        .route(
            "/twitch/api/raid/analytics",
            get(raid_network_analytics::raid_network_analytics_handler),
        )
        .with_state(pool)
}

/// Baut den Router für die Affiliate-Portal-HTML-Seite (P1.26).
///
/// `GET /twitch/affiliate/portal` — serviert das dedizierte Affiliate-Portal-
/// Bundle aus `website/dist/affiliate-portal`. Dessen Assets werden bereits
/// über die bestehende `/streamer/*`-Route (`website::streamer_asset_handler`)
/// ausgeliefert; eine zusätzliche Asset-Route ist nicht nötig. Die JSON-API
/// unter `/twitch/api/v2/affiliate/portal` bleibt im authed-Router.
pub fn build_affiliate_portal_router() -> Router {
    use handlers::affiliate_portal;

    Router::new().route(
        "/twitch/affiliate/portal",
        get(affiliate_portal::portal_page_handler),
    )
}

/// Baut den Router für die Social-Media-Admin-SPA (P2.66).
///
/// Die Handler nutzen denselben Auth-/Host-Gate-Pfad wie `/analyse`: der
/// `DashboardAuthLevel`-Extractor liest `DashboardAuthState` aus der globalen
/// Extension, und der `PgPool`-State wird fuer Partner-Access-Checks benötigt.
pub fn build_social_media_admin_router(pool: PgPool) -> Router {
    use handlers::spa;

    Router::new()
        .route("/social-media-admin", get(spa::social_media_admin_handler))
        .route(
            "/social-media-admin/*path",
            get(spa::social_media_admin_assets_handler),
        )
        .with_state(pool)
}

/// Baut die nativen Main-Domain-SPA-Seiten, die bisher vom Python-Fallback
/// bedient wurden.
///
/// Nur `/twitch/pricing` ist oeffentlich (Marketing-Seite). Die
/// eingeloggten Shells `/twitch/dashboard`, `/twitch/verwaltung` und
/// `/twitch/uplink` sind serverseitig gegated: ohne Session 303 auf den Login,
/// gesperrte Partner bekommen 403. Clientseitige Gates allein reichten nicht,
/// die komplette Dashboard-Navigation war sonst fuer jeden Besucher sichtbar.
///
/// Braucht deshalb den Pool: der Gate liest den Partner-Access-State.
pub fn build_v2_spa_pages_router(pool: PgPool) -> Router {
    use handlers::{obsolete_routes, spa};

    Router::new()
        .route(
            "/twitch/dashboard",
            get(spa::main_domain_spa_shell_gated_handler),
        )
        .route(
            "/twitch/verwaltung",
            get(spa::main_domain_spa_shell_gated_handler),
        )
        .route(
            "/twitch/uplink",
            get(spa::main_domain_spa_shell_gated_handler),
        )
        .route("/twitch/pricing", get(spa::main_domain_spa_shell_handler))
        .route(
            "/twitch/analyse",
            get(spa::legacy_analyse_root_redirect_handler),
        )
        .route(
            "/twitch/analyse/*path",
            get(spa::legacy_analyse_path_redirect_handler),
        )
        .route(
            "/twitch/dashboard-v2",
            get(spa::analyse_root_redirect_handler),
        )
        .route(
            "/twitch/dashboard-v2/*path",
            get(spa::dashboard_v2_public_assets_handler),
        )
        .route("/twitch/partners", get(spa::analyse_root_redirect_handler))
        .route(
            "/twitch/raid/analytics",
            get(spa::analyse_root_redirect_handler),
        )
        // Go-Live-Builder entfernt (User 2026-06-23): Seite -> Dashboard-Redirect.
        .route(
            "/twitch/live-announcement",
            get(spa::analyse_root_redirect_handler),
        )
        .route(
            "/twitch/api/live-announcement/config",
            get(obsolete_routes::live_announcement_builder_gone_handler)
                .post(obsolete_routes::live_announcement_builder_gone_handler),
        )
        .route(
            "/twitch/api/live-announcement/preview",
            get(obsolete_routes::live_announcement_builder_gone_handler),
        )
        .route(
            "/twitch/api/live-announcement/test",
            post(obsolete_routes::live_announcement_builder_gone_handler),
        )
        .with_state(pool)
}

/// Interne Routen fuer rs-relay: `platform-token`, `stream-kennzahlen` und
/// `chatter-verlauf` unter `/twitch/api/v2/internal/`.
/// Loopback plus `X-Internal-Token` (derselbe Token wie auf den Admin-Routen),
/// kein Cookie, kein CSRF.
pub fn build_platform_token_router(pool: PgPool, token: String) -> Router {
    Router::new()
        .route(
            "/twitch/api/v2/internal/platform-token",
            get(handlers::platform_token::internal_platform_token_handler),
        )
        // Kennzahlen des laufenden Streams fuer das Chat-Dock. Gleiche Tuer,
        // gleicher Token: das Relay hat genau einen internen Zugang.
        .route(
            "/twitch/api/v2/internal/stream-kennzahlen",
            get(handlers::stream_kennzahlen::internal_stream_kennzahlen_handler),
        )
        // Wer schreibt zum ersten Mal in diesem Kanal: fuer die
        // Erstchatter-Hervorhebung im Dock.
        .route(
            "/twitch/api/v2/internal/chatter-verlauf",
            get(handlers::chatter_verlauf::internal_chatter_verlauf_handler),
        )
        .layer(Extension(ExpectedToken(token)))
        .with_state(pool)
}

/// Baut die WebSocket-Route der eigenen OBS-Docks (Plan Abschnitt 2.3).
///
/// `GET /obs/ws` traegt den Chat-, Activity- und Stream-Info-Strom eines
/// Kanals. Die Auth laeuft ueber denselben `DashboardAuthLevel`-Extractor wie
/// alle Dashboard-Seiten, aber ohne Redirect: ein Socket bekommt 401 statt
/// einer Login-HTML-Seite.
///
/// Der Verteiler ist ein Prozess-Singleton
/// ([`obs::bus::ObsDockBus::gemeinsam`]) und haengt als Extension am Router.
/// Sein `PgListener` startet erst, wenn das erste Dock verbindet.
///
/// Das Singleton friert den Pool des **ersten** Aufrufers prozessweit ein. In
/// der Produktion ist das genau ein Aufruf (`build_router_with_helix` aus
/// `bin/tb-dashboard/src/main.rs`), also unkritisch. Im Testbinary bauen
/// mehrere Tests Router mit eigenen, teils toten Pools; wer dort den Live-Weg
/// des Busses pruefen will, baut sich mit [`obs::bus::ObsDockBus::neu`] einen
/// eigenen Bus, statt sich auf den Singleton zu verlassen.
///
/// `ExpectedToken` liegt mit auf der Route, weil der `DashboardAuthLevel`-
/// Extractor sonst den `X-Internal-Token`-Weg gar nicht sehen kann: ohne die
/// Extension faellt jeder interne Aufrufer auf `None` und damit auf 401. Es ist
/// derselbe Token wie auf den anderen Admin-Routen.
pub fn build_obs_ws_router(pool: PgPool, token: String) -> Router {
    let bus = obs::bus::ObsDockBus::gemeinsam(pool.clone());

    Router::new()
        .route("/obs/ws", get(obs::ws::obs_ws_handler))
        .layer(Extension(bus))
        .layer(Extension(ExpectedToken(token)))
        .with_state(pool)
}

/// Baut den Router für die öffentliche Website (`/streamer`) + Legacy-Redirect
/// (`/website`) — P2.67.
///
/// - `GET /streamer` → 301 auf `/streamer/`; `GET /streamer/{path}` → statische
///   Datei aus `website/dist` (Verzeichnis → `index.html`).
/// - `GET /website` (+ `/{path}`) → 301 auf `/streamer(/path)` (Query erhalten).
///
/// Öffentlich (kein Login). Nativ registriert (vor dem Strangler-Fallback).
pub fn build_website_router() -> Router {
    use handlers::website;

    Router::new()
        .route("/streamer", get(website::streamer_root_handler))
        .route("/streamer/help", get(handlers::help_page::help_page))
        .route(
            "/streamer/commands",
            get(handlers::help_page::commands_page),
        )
        .route("/streamer/faq", get(handlers::help_page::faq_redirect))
        .route("/streamer/*path", get(website::streamer_asset_handler))
        .route("/website", get(website::website_root_redirect_handler))
        .route(
            "/website/*path",
            get(website::website_path_redirect_handler),
        )
}

/// Zusammengeführter Router: public + auth (Login) + billing-webhook + authed +
/// admin-system + admin-streamers + admin-config + Legal-Seiten (HTML, statuslos).
///
/// CORS nur auf dem Public-Sub-Router (s. oben).
pub fn build_router(pool: PgPool, token: String) -> Router {
    build_router_with_helix(pool, token, None)
}

/// Wie [`build_router`], aber mit optionalem Helix-Client fuer den OBS-Pause-Loop.
pub fn build_router_with_helix(pool: PgPool, token: String, helix: Option<HelixClient>) -> Router {
    // P2.86/133/138/140: gemeinsamer Rate-Limiter (atomares Sliding-Window auf
    // dashboard_sessions). Der Fernet-Key wird aus der Env gelesen (gleiche
    // Quelle wie die Session-Verschlüsselung). Fehlt er, läuft der Limiter mit
    // leerem Key — die Hit-Rows sind trotzdem konsistent verschlüsselt; bei
    // DB-Fehlern ist der Limiter fail-open (siehe RateLimiter::allow).
    let fernet_key = DashboardAuthState::fernet_key_from_env().unwrap_or_default();
    let rate_limiter = RateLimiter::new(pool.clone(), fernet_key);
    let pause_loop_router = match helix {
        Some(helix) => build_pause_loop_router(pool.clone(), helix),
        None => handlers::pause_loop::build_unavailable_pause_loop_router(),
    };

    let mut app = build_public_router(pool.clone())
        .merge(pause_loop_router)
        .merge(build_auth_router(rate_limiter.clone()))
        .merge(build_partner_login_router(
            pool.clone(),
            rate_limiter.clone(),
        ))
        .merge(build_affiliate_router(pool.clone(), rate_limiter.clone()))
        .merge(build_roadmap_router(pool.clone(), token.clone()))
        .merge(build_market_router(pool.clone(), token.clone()))
        .merge(build_raid_pages_router(pool.clone()))
        .merge(build_affiliate_portal_router())
        .merge(build_social_media_admin_router(pool.clone()))
        .merge(build_v2_spa_pages_router(pool.clone()))
        .merge(build_obs_ws_router(pool.clone(), token.clone()))
        .merge(build_platform_token_router(pool.clone(), token.clone()))
        .merge(build_website_router())
        .merge(handlers::discord_link::build_discord_link_router(
            pool.clone(),
        ))
        .merge(handlers::demo::build_demo_router())
        .merge(build_entry_admin_router())
        .merge(build_billing_webhook_router(pool.clone()))
        .merge(build_billing_page_router(pool.clone()))
        .merge(build_admin_legacy_forms_router(pool.clone()))
        .merge(build_authed_router(
            pool.clone(),
            token.clone(),
            rate_limiter,
        ))
        .merge(build_admin_system_router(pool.clone(), token.clone()))
        .merge(build_admin_streamers_router(pool.clone(), token.clone()))
        .merge(build_admin_config_router(pool.clone(), token))
        .merge(handlers::admin_mode::build_admin_mode_router())
        .merge(handlers::legal::build_legal_router())
        .merge(handlers::roadmap_page::build_roadmap_page_router())
        .layer(axum::middleware::from_fn_with_state(
            pool,
            admin_audit::audit_admin_mutations,
        ))
        .layer(
            TraceLayer::new_for_http()
                .on_request(|request: &axum::http::Request<_>, _span: &tracing::Span| {
                    tracing::info!(
                        method = %request.method(),
                        path = %request.uri().path(),
                        "HTTP Request gestartet"
                    );
                })
                .on_response(
                    |response: &axum::http::Response<_>,
                     latency: std::time::Duration,
                     _span: &tracing::Span| {
                        tracing::info!(
                            status = response.status().as_u16(),
                            latency_ms = latency.as_millis(),
                            "HTTP Request abgeschlossen"
                        );
                    },
                ),
        )
        .layer(CompressionLayer::new());

    // Uplink Multi-Chat: Token-Lesepfad fuer rs-relay, Stream-Key-Nachlauf und
    // Trennen. Kein eigener Refresh-Job mehr: erneuert wird ueber denselben
    // Schreibpfad wie beim Raid-Bot, sonst streiten sich zwei Jobs um denselben
    // Refresh-Token. Ohne Config (Client-ID/Secret, Feldschluessel) bleiben die
    // Routen mit 503 zu und /uplink/me meldet alle Plattformen als getrennt.
    if let Some(config) = handlers::platform_token::platform_token_config_from_env() {
        app = app.layer(Extension(config));
        tracing::info!("Uplink Multi-Chat: Twitch-Token-Weg aktiv");
    }

    // P2.108: globaler Default-Security-Header-Bundle auf ALLE Antworten
    // (if_not_present überschreibt keine handler-eigenen Header).
    for layer in security_header_layers() {
        app = app.layer(layer);
    }
    app
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
        PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .ok()
    }

    /// Nicht-Loopback-POST ohne Admin-Auth → 401 vor CSRF (Python-Parität).
    #[tokio::test]
    async fn admin_write_ohne_auth_401_vor_csrf() {
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
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let body = axum::body::to_bytes(resp.into_body(), 65536).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "auth_required");
        assert_eq!(json["required"], "admin");
    }

    /// Authentifizierter Admin-POST ohne CSRF-State bleibt 403 invalid_csrf.
    #[tokio::test]
    async fn admin_write_mit_auth_ohne_csrf_403() {
        let Some(pool) = pool().await else { return };
        let app = build_admin_config_router(pool, "tok".into());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/twitch/api/admin/announcements")
                    .header("host", "dashboard.example.com")
                    .header(tb_http_core::INTERNAL_TOKEN_HEADER, "tok")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        let body = axum::body::to_bytes(resp.into_body(), 65536).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "invalid_csrf");
    }

    #[tokio::test]
    async fn affiliate_generate_ohne_auth_shape_401() {
        let Some(pool) = pool().await else { return };
        let app = build_admin_config_router(pool, "tok".into());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/twitch/api/admin/affiliates/generate-gutschriften")
                    .header("host", "dashboard.example.com")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let body = axum::body::to_bytes(resp.into_body(), 65536).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "auth_required");
        assert_eq!(json["required"], "admin");
    }

    /// GET passiert den CSRF-Layer (Safe-Methode); ohne Auth liefert der Handler
    /// 401 auth_required, aber NICHT invalid_csrf — beweist, dass GET nicht vom Layer geblockt wird.
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
        let body = axum::body::to_bytes(resp.into_body(), 65536).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "auth_required");
    }
}

#[cfg(test)]
mod router_wiring_tests {
    //! Welle-2-A3: Verifiziert, dass `build_router` ohne Router-Overlap-Panic
    //! konstruierbar ist (alle gemergten Sub-Router sind pfad-disjunkt) und dass
    //! der Security-Header-Bundle auf den Antworten liegt. Braucht KEINE echte
    //! DB — ein lazy PgPool reicht für den reinen Router-Aufbau.
    use super::*;
    use axum::body::Body;
    use axum::http::{header, Request, StatusCode};
    use sqlx::postgres::PgPoolOptions;
    use tower::ServiceExt;

    fn lazy_pool() -> PgPool {
        // connect_lazy baut KEINE Verbindung auf — perfekt, um den Router-
        // Zusammenbau (Overlap-Schutz) ohne laufende DB zu prüfen.
        PgPoolOptions::new()
            .max_connections(1)
            .connect_lazy("postgres://invalid:invalid@127.0.0.1:1/none")
            .expect("lazy pool")
    }

    /// build_router darf NICHT paniccen (kein doppelter Pfad in zwei Routern).
    /// Async, weil der lazy PgPool einen Tokio-Kontext für seinen Reaper braucht.
    #[tokio::test]
    async fn build_router_konstruiert_ohne_overlap_panic() {
        let _app = build_router(lazy_pool(), "smoke-token".into());
    }

    /// Eine öffentliche Route (Legacy-Redirect, kein DB-Zugriff) trägt den
    /// Security-Header-Bundle (P2.108) und liefert kein 500.
    #[tokio::test]
    async fn security_header_bundle_auf_antwort() {
        let app = build_router(lazy_pool(), "smoke-token".into());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/website/foo")
                    .header("host", "dashboard.example.com")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        // /website/foo → 301 auf /streamer/foo (kein DB-Zugriff).
        assert_eq!(resp.status(), StatusCode::PERMANENT_REDIRECT);
        let h = resp.headers();
        assert_eq!(h.get("x-frame-options").unwrap(), "DENY");
        assert_eq!(h.get("x-content-type-options").unwrap(), "nosniff");
        assert_eq!(
            h.get("referrer-policy").unwrap(),
            "strict-origin-when-cross-origin"
        );
        assert_eq!(h.get("cross-origin-opener-policy").unwrap(), "same-origin");
        assert_eq!(h.get("x-xss-protection").unwrap(), "0");
        assert_eq!(h.get(header::LOCATION).unwrap(), "/streamer/foo");
    }

    #[tokio::test]
    async fn cors_gilt_nur_fuer_public_api_nicht_fuer_html_redirects() {
        let app = build_public_router(lazy_pool());
        let html = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/twitch/overlay")
                    .header("host", "deutsche-deadlock-community.de")
                    .header(header::ORIGIN, "https://evil.example")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(html.status(), StatusCode::SEE_OTHER);
        assert!(
            html.headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .is_none(),
            "HTML- und Login-Antworten dürfen kein Wildcard-CORS tragen"
        );

        let api = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/healthz")
                    .header("host", "deutsche-deadlock-community.de")
                    .header(header::ORIGIN, "https://example.org")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(api.status(), StatusCode::OK);
        assert_eq!(
            api.headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .and_then(|value| value.to_str().ok()),
            Some("*")
        );
    }

    /// Die eingeloggten Main-Domain-Shells duerfen ausgeloggt keine Shell
    /// zeigen: 303 auf den Login mit dem eigenen Pfad als `next`.
    /// `/twitch/pricing` bleibt als Marketing-Seite offen.
    #[tokio::test]
    async fn spa_shells_sind_ohne_session_gegated_pricing_bleibt_offen() {
        let app = build_router(lazy_pool(), "smoke-token".into());
        let gegated = [
            (
                "/twitch/dashboard",
                "/twitch/auth/login?next=%2Ftwitch%2Fdashboard",
            ),
            (
                "/twitch/verwaltung",
                "/twitch/auth/login?next=%2Ftwitch%2Fverwaltung",
            ),
            (
                "/twitch/uplink",
                "/twitch/auth/login?next=%2Ftwitch%2Fuplink",
            ),
        ];
        for (pfad, ziel) in gegated {
            let resp = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("GET")
                        .uri(pfad)
                        .header("host", "deutsche-deadlock-community.de")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::SEE_OTHER, "{pfad}");
            assert_eq!(
                resp.headers().get(header::LOCATION).unwrap(),
                ziel,
                "{pfad}"
            );
        }

        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/twitch/pricing")
                    .header("host", "deutsche-deadlock-community.de")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        // Ohne gebautes Bundle 404 "Dashboard not built", mit Bundle 200 —
        // aber niemals ein Login-Redirect.
        assert_ne!(resp.status(), StatusCode::SEE_OTHER);
        assert!(resp.headers().get(header::LOCATION).is_none());
    }

    #[tokio::test]
    async fn full_router_registriert_pause_loop_unavailable_vor_globalen_layern() {
        let app = build_router(lazy_pool(), "smoke-token".into());
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/twitch/pause-loop")
                    .header("host", "dashboard.example.com")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let h = resp.headers();
        assert_eq!(h.get("x-frame-options").unwrap(), "SAMEORIGIN");
        assert!(h.get("content-security-policy").is_some());
        assert_eq!(h.get("x-content-type-options").unwrap(), "nosniff");
        assert_eq!(
            h.get("referrer-policy").unwrap(),
            "strict-origin-when-cross-origin"
        );
        assert_eq!(h.get("cross-origin-opener-policy").unwrap(), "same-origin");

        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/twitch/api/v2/public/pause-loop-clips")
                    .header("host", "dashboard.example.com")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "pause_loop_unavailable");
    }

    #[tokio::test]
    async fn live_announcement_legacy_api_pfade_liefern_410_json() {
        let app = build_router(lazy_pool(), "smoke-token".into());
        for (method, uri) in [
            ("GET", "/twitch/api/live-announcement/config"),
            ("POST", "/twitch/api/live-announcement/config"),
            ("GET", "/twitch/api/live-announcement/preview"),
            ("POST", "/twitch/api/live-announcement/test"),
        ] {
            let resp = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(uri)
                        .header("host", "dashboard.example.com")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::GONE, "{method} {uri}");
            let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
            let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(json["error"], "live_announcement_builder_removed");
            assert!(json.get("message").and_then(|v| v.as_str()).is_some());
        }
    }
}
