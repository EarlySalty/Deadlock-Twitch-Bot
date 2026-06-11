//! Handler für `GET /twitch/api/admin/streamers` und
//! `GET /twitch/api/admin/streamers/{login}`.

use axum::{
    extract::{Path, Query, State},
    response::IntoResponse,
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tb_analytics::admin_streamers::{
    list_streamers, partner_status, scope_snapshot, streamer_detail, streamer_stats_and_sessions,
    StreamerView,
};
use tb_http_core::{ApiError, AuthLevel};

// ── Query-Parameter ──────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ListQuery {
    pub view: Option<String>,
}

// ── List-Response ────────────────────────────────────────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdminStreamersResponse {
    pub items: Vec<AdminStreamerItem>,
    pub count: usize,
    pub view: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdminStreamerItem {
    pub login: String,
    pub display_name: String,
    pub twitch_user_id: Option<String>,
    pub discord_user_id: Option<String>,
    pub discord_display_name: Option<String>,
    pub verified: bool,
    pub archived: bool,
    pub archived_at: Option<String>,
    pub created_at: Option<String>,
    pub is_live: bool,
    pub is_on_discord: bool,
    pub manual_partner_opt_out: bool,
    pub partner_status: String,
    pub viewer_count: i64,
    pub active_session_id: Option<i64>,
    pub last_seen_at: Option<String>,
    pub last_game: Option<String>,
    pub last_stream_at: Option<String>,
    pub plan_id: Option<String>,
    pub billing_status: Option<String>,
    pub oauth_connected: bool,
    pub oauth_needs_reauth: bool,
    pub oauth_status: String,
    pub granted_scopes: Vec<String>,
    pub missing_scopes: Vec<String>,
    pub oauth_authorized_at: Option<String>,
    pub promo_disabled: bool,
    pub notes: Option<String>,
    pub technical_pause_reason: Option<String>,
    pub operational_state: Option<String>,
    /// Abgeleiteter Anzeige-Status: "live" | "verified" | "offline" | "archived" |
    /// "departnered" | "blocked" | "non_partner" | "token_error"
    pub status: String,
}

// ── Detail-Response ──────────────────────────────────────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdminStreamerDetailResponse {
    pub login: String,
    pub display_name: String,
    pub twitch_user_id: Option<String>,
    pub partner_status: String,
    pub stats: StreamerStats,
    pub sessions: Vec<StreamerSession>,
    pub settings: StreamerSettings,
    pub oauth: StreamerOAuth,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamerStats {
    pub total_sessions: i64,
    pub total_duration_seconds: i64,
    pub avg_viewers: f64,
    pub peak_viewers: i64,
    pub follower_delta: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamerSession {
    pub id: i64,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub stream_title: Option<String>,
    pub game_name: Option<String>,
    pub avg_viewers: Option<f64>,
    pub peak_viewers: Option<i64>,
    pub duration_seconds: Option<i64>,
    pub follower_delta: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamerSettings {
    pub raid_bot_enabled: bool,
    pub silent_ban: bool,
    pub silent_raid: bool,
    pub is_monitored_only: bool,
    pub live_ping_enabled: bool,
    pub promo_disabled: bool,
    pub promo_message: Option<String>,
    pub raid_boost_enabled: bool,
    pub notes: Option<String>,
    pub plan_name: Option<String>,
    pub manual_plan_id: Option<String>,
    pub manual_plan_expires_at: Option<String>,
    pub manual_plan_notes: Option<String>,
    pub billing_plan_id: Option<String>,
    pub billing_status: Option<String>,
    pub is_on_discord: bool,
    pub require_discord_link: bool,
    pub discord_user_id: Option<String>,
    pub discord_display_name: Option<String>,
    pub created_at: Option<String>,
    pub archived_at: Option<String>,
    pub operational_state: Option<String>,
    pub technical_pause_reason: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamerOAuth {
    pub connected: bool,
    pub needs_reauth: bool,
    pub status: String,
    pub granted_scopes: Vec<String>,
    pub missing_scopes: Vec<String>,
    pub authorized_at: Option<String>,
    pub raid_enabled: bool,
}

// ── Hilfsfunktionen ──────────────────────────────────────────────────────────

fn fmt_dt(dt: DateTime<Utc>) -> String {
    dt.to_rfc3339()
}

// ── Handler ───────────────────────────────────────────────────────────────────

/// `GET /twitch/api/admin/streamers?view=<view>`
pub async fn list_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<ListQuery>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    let view_str = params.view.as_deref().unwrap_or("all");
    let view = StreamerView::parse(view_str).ok_or_else(|| {
        ApiError::bad_request_with_body(serde_json::json!({
            "error": "invalid_view",
            "supported": StreamerView::all_names(),
        }))
    })?;

    let rows = list_streamers(&pool, view).await.map_err(|e| {
        tracing::error!("list_streamers Fehler: {e}");
        ApiError::internal()
    })?;

    let items: Vec<AdminStreamerItem> = rows
        .into_iter()
        .map(|r| {
            let snap = scope_snapshot(r.scopes.as_deref(), r.needs_reauth.unwrap_or(0));
            let ps = partner_status(
                r.status.as_deref(),
                r.archived_at.as_deref(),
                r.manual_partner_opt_out.unwrap_or(0),
                r.technical_pause_reason.as_deref(),
            );
            // Abgeleiteter Anzeige-Status
            let display_status = if r.is_live != 0 {
                "live"
            } else if r.is_verified != 0 {
                "verified"
            } else {
                ps
            };

            AdminStreamerItem {
                login: r.twitch_login.clone(),
                display_name: r.twitch_login, // kein separates display_name in DB
                twitch_user_id: r.twitch_user_id,
                discord_user_id: r.discord_user_id,
                discord_display_name: r.discord_display_name,
                verified: r.is_verified != 0,
                archived: r.archived_at.is_some(),
                archived_at: r.archived_at,
                created_at: r.created_at,
                is_live: r.is_live != 0,
                is_on_discord: r.is_on_discord.unwrap_or(0) != 0,
                manual_partner_opt_out: r.manual_partner_opt_out.unwrap_or(0) != 0,
                partner_status: ps.to_string(),
                viewer_count: r.last_viewer_count.unwrap_or(0) as i64,
                active_session_id: r.active_session_id,
                last_seen_at: r.last_seen_at,
                last_game: r.last_game,
                last_stream_at: r.last_stream_at.map(fmt_dt),
                plan_id: r.billing_plan_id.or(r.manual_plan_id),
                billing_status: r.billing_status,
                oauth_connected: snap.connected,
                oauth_needs_reauth: snap.needs_reauth,
                oauth_status: snap.status.to_string(),
                granted_scopes: snap.granted_scopes,
                missing_scopes: snap.missing_scopes,
                oauth_authorized_at: r.authorized_at.map(fmt_dt),
                promo_disabled: r.promo_disabled.unwrap_or(0) != 0,
                notes: r.promo_message, // list zeigt promo_message als notes
                technical_pause_reason: r.technical_pause_reason,
                operational_state: r.operational_state,
                status: display_status.to_string(),
            }
        })
        .collect();

    let count = items.len();
    Ok(Json(AdminStreamersResponse {
        items,
        count,
        view: view_str.to_string(),
    }))
}

/// `GET /twitch/api/admin/streamers/{login}`
pub async fn detail_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Path(login): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    let row = streamer_detail(&pool, &login)
        .await
        .map_err(|e| {
            tracing::error!("streamer_detail Fehler für {login}: {e}");
            ApiError::internal()
        })?
        .ok_or_else(ApiError::not_found)?;

    let (stats_row, session_rows) =
        streamer_stats_and_sessions(&pool, &login)
            .await
            .map_err(|e| {
                tracing::error!("streamer_stats_and_sessions Fehler für {login}: {e}");
                ApiError::internal()
            })?;

    let snap = scope_snapshot(row.scopes.as_deref(), row.needs_reauth.unwrap_or(0));
    let ps = partner_status(
        row.status.as_deref(),
        row.archived_at.as_deref(),
        row.manual_partner_opt_out.unwrap_or(0),
        row.technical_pause_reason.as_deref(),
    );

    let sessions = session_rows
        .into_iter()
        .map(|s| StreamerSession {
            id: s.id,
            started_at: fmt_dt(s.started_at),
            ended_at: s.ended_at.map(fmt_dt),
            stream_title: s.stream_title,
            game_name: s.game_name,
            avg_viewers: s.avg_viewers,
            peak_viewers: s.peak_viewers,
            duration_seconds: s.duration_seconds,
            follower_delta: s.follower_delta,
        })
        .collect();

    Ok(Json(AdminStreamerDetailResponse {
        login: row.twitch_login.clone(),
        display_name: row.twitch_login,
        twitch_user_id: row.twitch_user_id,
        partner_status: ps.to_string(),
        stats: StreamerStats {
            total_sessions: stats_row.total_sessions,
            total_duration_seconds: stats_row.total_duration_seconds,
            avg_viewers: stats_row.avg_viewers,
            peak_viewers: stats_row.peak_viewers,
            follower_delta: stats_row.follower_delta,
        },
        sessions,
        settings: StreamerSettings {
            raid_bot_enabled: row.raid_bot_enabled.unwrap_or(1) != 0,
            silent_ban: row.silent_ban.unwrap_or(0) != 0,
            silent_raid: row.silent_raid.unwrap_or(0) != 0,
            is_monitored_only: row.is_monitored_only.unwrap_or(0) != 0,
            live_ping_enabled: row.live_ping_enabled != 0,
            promo_disabled: row.promo_disabled.unwrap_or(0) != 0,
            promo_message: row.promo_message,
            raid_boost_enabled: row.raid_boost_enabled.unwrap_or(0) != 0,
            notes: row.notes,
            plan_name: row.plan_name,
            manual_plan_id: row.manual_plan_id,
            manual_plan_expires_at: row.manual_plan_expires_at.map(fmt_dt),
            manual_plan_notes: row.manual_plan_notes,
            billing_plan_id: row.billing_plan_id,
            billing_status: row.billing_status,
            is_on_discord: row.is_on_discord.unwrap_or(0) != 0,
            require_discord_link: row.require_discord_link.unwrap_or(0) != 0,
            discord_user_id: row.discord_user_id,
            discord_display_name: row.discord_display_name,
            created_at: row.created_at,
            archived_at: row.archived_at,
            operational_state: row.operational_state,
            technical_pause_reason: row.technical_pause_reason,
        },
        oauth: StreamerOAuth {
            connected: snap.connected,
            needs_reauth: snap.needs_reauth,
            status: snap.status.to_string(),
            granted_scopes: snap.granted_scopes,
            missing_scopes: snap.missing_scopes,
            authorized_at: row.authorized_at.map(fmt_dt),
            raid_enabled: row.oauth_raid_enabled.unwrap_or(0) != 0,
        },
    }))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        extract::ConnectInfo,
        http::{Request, StatusCode},
        routing::get,
        Extension, Router,
    };
    use sqlx::postgres::PgPoolOptions;
    use std::net::SocketAddr;
    use tb_http_core::ExpectedToken;
    use tower::ServiceExt;

    fn test_dsn() -> Option<String> {
        std::env::var("TB_TEST_DATABASE_URL").ok()
    }

    async fn make_pool(dsn: &str, schema: &str) -> PgPool {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(dsn)
            .await
            .expect("connect");
        sqlx::query(&format!("CREATE SCHEMA IF NOT EXISTS {schema}"))
            .execute(&pool)
            .await
            .expect("Schema");
        sqlx::query(&format!("SET search_path TO {schema}"))
            .execute(&pool)
            .await
            .expect("search_path");

        // Gleiche DDL wie in tb-analytics-Tests (kopiert, damit Handler-Tests standalone laufen)
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_partners_all_state (
                id BIGSERIAL PRIMARY KEY, twitch_login TEXT NOT NULL, twitch_user_id TEXT,
                discord_user_id TEXT, discord_display_name TEXT, created_at TEXT,
                archived_at TEXT, require_discord_link INTEGER NOT NULL DEFAULT 0,
                is_on_discord INTEGER NOT NULL DEFAULT 0, manual_partner_opt_out INTEGER NOT NULL DEFAULT 0,
                status TEXT, raid_bot_enabled INTEGER NOT NULL DEFAULT 1, silent_ban INTEGER NOT NULL DEFAULT 0,
                silent_raid INTEGER NOT NULL DEFAULT 0, is_monitored_only INTEGER NOT NULL DEFAULT 0,
                is_verified INTEGER NOT NULL DEFAULT 0, is_partner_active INTEGER NOT NULL DEFAULT 1,
                live_ping_enabled INTEGER NOT NULL DEFAULT 1,
                technical_pause_reason TEXT, operational_state TEXT
            )
        "#,
        )
        .execute(&pool)
        .await
        .expect("DDL");
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_live_state (
                streamer_login TEXT PRIMARY KEY, twitch_user_id TEXT,
                is_live INTEGER NOT NULL DEFAULT 0, last_seen_at TEXT,
                last_started_at TEXT, last_viewer_count INTEGER,
                active_session_id BIGINT, last_game TEXT
            )
        "#,
        )
        .execute(&pool)
        .await
        .expect("DDL");
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_raid_auth (
                id BIGSERIAL PRIMARY KEY, twitch_login TEXT, twitch_user_id TEXT,
                scopes TEXT, needs_reauth INTEGER NOT NULL DEFAULT 0,
                raid_enabled INTEGER NOT NULL DEFAULT 0, authorized_at TIMESTAMPTZ
            )
        "#,
        )
        .execute(&pool)
        .await
        .expect("DDL");
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_billing_subscriptions (
                id BIGSERIAL PRIMARY KEY, customer_reference TEXT NOT NULL,
                plan_id TEXT, status TEXT, updated_at TIMESTAMPTZ
            )
        "#,
        )
        .execute(&pool)
        .await
        .expect("DDL");
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS streamer_plans (
                twitch_login TEXT PRIMARY KEY, plan_name TEXT,
                promo_disabled INTEGER NOT NULL DEFAULT 0, promo_message TEXT,
                raid_boost_enabled INTEGER NOT NULL DEFAULT 0, notes TEXT,
                manual_plan_id TEXT, manual_plan_expires_at TIMESTAMPTZ, manual_plan_notes TEXT
            )
        "#,
        )
        .execute(&pool)
        .await
        .expect("DDL");
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_stream_sessions (
                id BIGSERIAL PRIMARY KEY, streamer_login TEXT NOT NULL,
                started_at TIMESTAMPTZ NOT NULL, ended_at TIMESTAMPTZ,
                stream_title TEXT, game_name TEXT, avg_viewers DOUBLE PRECISION,
                peak_viewers BIGINT, duration_seconds BIGINT, follower_delta BIGINT
            )
        "#,
        )
        .execute(&pool)
        .await
        .expect("DDL");
        sqlx::query(
            "TRUNCATE twitch_partners_all_state, twitch_live_state, twitch_raid_auth, \
             twitch_billing_subscriptions, streamer_plans, twitch_stream_sessions",
        )
        .execute(&pool)
        .await
        .expect("TRUNCATE");
        pool
    }

    fn make_list_router(pool: PgPool, token: &str) -> Router {
        Router::new()
            .route("/twitch/api/admin/streamers", get(list_handler))
            .with_state(pool)
            .layer(Extension(ExpectedToken(token.to_string())))
    }

    fn make_detail_router(pool: PgPool, token: &str) -> Router {
        Router::new()
            .route("/twitch/api/admin/streamers/:login", get(detail_handler))
            .with_state(pool)
            .layer(Extension(ExpectedToken(token.to_string())))
    }

    fn addr() -> SocketAddr {
        "1.2.3.4:9999".parse().unwrap()
    }

    // ── List-Tests ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn list_returns_401_ohne_auth() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_admin_h_list_unauth").await;
        let req = Request::builder()
            .uri("/twitch/api/admin/streamers?view=all")
            .extension(ConnectInfo(addr()))
            .header(axum::http::header::HOST, "example.com")
            .body(Body::empty())
            .unwrap();
        let res = make_list_router(pool, "tok").oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn list_returns_400_bei_invalidem_view() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_admin_h_list_400").await;
        let req = Request::builder()
            .uri("/twitch/api/admin/streamers?view=ungueltig")
            .extension(ConnectInfo(addr()))
            .header(axum::http::header::HOST, "example.com")
            .header("x-internal-token", "tok")
            .body(Body::empty())
            .unwrap();
        let res = make_list_router(pool, "tok").oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let b = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
        assert_eq!(v["error"], "invalid_view");
        assert!(v["supported"].is_array());
    }

    #[tokio::test]
    async fn list_returns_200_mit_leerer_liste() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_admin_h_list_200_leer").await;
        let req = Request::builder()
            .uri("/twitch/api/admin/streamers?view=all")
            .extension(ConnectInfo(addr()))
            .header(axum::http::header::HOST, "example.com")
            .header("x-internal-token", "tok")
            .body(Body::empty())
            .unwrap();
        let res = make_list_router(pool, "tok").oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let b = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
        assert_eq!(v["count"], 0);
        assert!(v["items"].as_array().unwrap().is_empty());
        assert_eq!(v["view"], "all");
    }

    // ── Detail-Tests ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn detail_returns_401_ohne_auth() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_admin_h_detail_unauth").await;
        let req = Request::builder()
            .uri("/twitch/api/admin/streamers/teststreamer")
            .extension(ConnectInfo(addr()))
            .header(axum::http::header::HOST, "example.com")
            .body(Body::empty())
            .unwrap();
        let res = make_detail_router(pool, "tok").oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn detail_returns_404_fuer_unbekannten_login() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_admin_h_detail_404").await;
        let req = Request::builder()
            .uri("/twitch/api/admin/streamers/gibts_nicht")
            .extension(ConnectInfo(addr()))
            .header(axum::http::header::HOST, "example.com")
            .header("x-internal-token", "tok")
            .body(Body::empty())
            .unwrap();
        let res = make_detail_router(pool, "tok").oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn detail_returns_200_fuer_bekannten_login() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_admin_h_detail_200").await;
        sqlx::query(
            "INSERT INTO twitch_partners_all_state (twitch_login, status, created_at) \
             VALUES ('bekannter', 'active', NOW())",
        )
        .execute(&pool)
        .await
        .expect("insert");
        let req = Request::builder()
            .uri("/twitch/api/admin/streamers/Bekannter") // case-insensitive
            .extension(ConnectInfo(addr()))
            .header(axum::http::header::HOST, "example.com")
            .header("x-internal-token", "tok")
            .body(Body::empty())
            .unwrap();
        let res = make_detail_router(pool, "tok").oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let b = axum::body::to_bytes(res.into_body(), 8192).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&b).unwrap();
        assert_eq!(v["login"], "bekannter");
        assert!(v["stats"].is_object());
        assert!(v["sessions"].is_array());
        assert!(v["settings"].is_object());
        assert!(v["oauth"].is_object());
    }
}
