//! Handler für `/twitch/api/v2/watch-time-distribution`.
//!
//! Port von `bot/analytics/api_audience.py:_api_v2_watch_time_distribution`.
//! Auth: nur „eingeloggt" (kein Extended-Plan-Gate); stattdessen narrowed ein
//! Lesefenster die Sichtbarkeit (Free → nur letzter Stream). `streamer` Pflicht,
//! `days` 7..365.
//!
//! Lesefenster (Python `_resolve_read_window`): Admin → `full`; sonst
//! entscheidet der Plan des **abgefragten** Streamers — das konsolidierte
//! `analytics`-Flag → `full`, sonst (kein Flag) → `last_stream`.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;

use crate::auth::level::DashboardAuthLevel;

#[derive(Deserialize)]
pub struct WatchTimeQuery {
    #[serde(default)]
    pub streamer: Option<String>,
    #[serde(default)]
    pub days: Option<i32>,
}

/// Pures Fenster-Mapping anhand der Plan-Entitlements (Python `_plan_has_entitlement`).
fn window_for_entitlements(entitlements: &[&str]) -> &'static str {
    if entitlements.contains(&"analytics") {
        "full"
    } else {
        "last_stream"
    }
}

/// Lesefenster auflösen: Admin → `full`; sonst Plan des Streamers prüfen.
/// Bei Plan-Lookup-Fehler konservativ `last_stream` (nie mehr Daten zeigen als erlaubt).
async fn resolve_read_window(
    pool: &PgPool,
    auth: &DashboardAuthLevel,
    streamer: &str,
) -> &'static str {
    if matches!(auth, DashboardAuthLevel::Admin { .. }) {
        return "full";
    }
    match tb_analytics::plan::resolve_plan_snapshot(pool, streamer, "").await {
        Ok(snap) => window_for_entitlements(&snap.entitlements),
        Err(_) => "last_stream",
    }
}

/// `GET /twitch/api/v2/watch-time-distribution?streamer=&days=30`
pub async fn watch_time_distribution_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<WatchTimeQuery>,
) -> impl IntoResponse {
    // _require_v2_auth: jede gültige v2-Auth genügt, None → 401.
    if matches!(auth, DashboardAuthLevel::None) {
        return crate::auth::unauthorized_v2_response();
    }
    // IDOR-Guard: Partner werden auf den eigenen Login geklemmt (fremder
    // ?streamer= → 403); Admin/Localhost dürfen frei wählen. streamer Pflicht.
    let streamer =
        match crate::auth::resolve_streamer_scope(&auth, params.streamer.as_deref(), true) {
            Ok(Some(s)) => s,
            Ok(None) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "Streamer required" })),
                )
                    .into_response();
            }
            Err(resp) => return resp,
        };
    let days = params.days.unwrap_or(30).clamp(7, 365) as i64;
    let window = resolve_read_window(&pool, &auth, &streamer).await;

    match tb_analytics::watch_time::load_watch_time_distribution(&pool, &streamer, days, window)
        .await
    {
        Ok(v) => Json(v).into_response(),
        Err(e) => {
            tracing::error!("watch-time-distribution Fehler: {e}");
            crate::auth::analytics_request_failed_json().into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn)
            .unwrap()
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(opts)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE twitch_stream_sessions (id BIGSERIAL PRIMARY KEY, streamer_login TEXT, started_at TIMESTAMPTZ, ended_at TIMESTAMPTZ)").execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE twitch_session_chatters (session_id BIGINT, chatter_login TEXT, chatter_id TEXT, first_message_at TIMESTAMPTZ, last_seen_at TIMESTAMPTZ)").execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE twitch_chat_messages (id BIGSERIAL PRIMARY KEY, session_id BIGINT, chatter_login TEXT, chatter_id TEXT, message_ts TIMESTAMPTZ)").execute(&pool).await.unwrap();
        Some(pool)
    }

    #[test]
    fn fenster_mapping() {
        assert_eq!(
            window_for_entitlements(&["analytics", "chat.lurker_tax"]),
            "full"
        );
        assert_eq!(window_for_entitlements(&["analytics"]), "full");
        assert_eq!(window_for_entitlements(&["chat.lurker_tax"]), "last_stream"); // kein Flag
        assert_eq!(window_for_entitlements(&[]), "last_stream");
    }

    #[tokio::test]
    async fn streamer_pflicht_400() {
        let Some(pool) = make_pool("t_wt_handler1").await else {
            return;
        };
        let resp = watch_time_distribution_handler(
            DashboardAuthLevel::admin(),
            State(pool),
            Query(WatchTimeQuery {
                streamer: None,
                days: None,
            }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn none_auth_401() {
        let Some(pool) = make_pool("t_wt_handler2").await else {
            return;
        };
        let resp = watch_time_distribution_handler(
            DashboardAuthLevel::None,
            State(pool),
            Query(WatchTimeQuery {
                streamer: Some("nani".into()),
                days: None,
            }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    /// IDOR-Guard: dieser Handler hat KEIN extended_gate, der Scope-Guard ist
    /// also die einzige 403-Quelle. Ein Partner, der per ?streamer= einen
    /// fremden Login abfragt, muss eindeutig durch den Scope-Guard 403 erhalten.
    #[tokio::test]
    async fn partner_fremder_streamer_403() {
        let Some(pool) = make_pool("t_wt_handler_idor").await else {
            return;
        };
        let resp = watch_time_distribution_handler(
            DashboardAuthLevel::Partner {
                twitch_login: "earlysalty".into(),
                twitch_user_id: "42".into(),
                display_name: "earlysalty".into(),
            },
            State(pool),
            Query(WatchTimeQuery {
                streamer: Some("ismile_e".into()),
                days: Some(30),
            }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    /// Gegenprobe: derselbe Partner darf den EIGENEN Login lesen (200), trotz
    /// Case-Mismatch im Param (Helper lowercased beide Seiten).
    #[tokio::test]
    async fn partner_eigener_streamer_200() {
        let Some(pool) = make_pool("t_wt_handler_own").await else {
            return;
        };
        let resp = watch_time_distribution_handler(
            DashboardAuthLevel::Partner {
                twitch_login: "earlysalty".into(),
                twitch_user_id: "42".into(),
                display_name: "earlysalty".into(),
            },
            State(pool),
            Query(WatchTimeQuery {
                streamer: Some("EarlySalty".into()),
                days: Some(30),
            }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn localhost_200() {
        let Some(pool) = make_pool("t_wt_handler3").await else {
            return;
        };
        let resp = watch_time_distribution_handler(
            DashboardAuthLevel::admin(),
            State(pool),
            Query(WatchTimeQuery {
                streamer: Some("nani".into()),
                days: Some(30),
            }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
