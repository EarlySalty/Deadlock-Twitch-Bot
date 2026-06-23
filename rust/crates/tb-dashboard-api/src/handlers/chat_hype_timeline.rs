//! Handler für `/twitch/api/v2/chat-hype-timeline`.
//!
//! Port von `bot/analytics/api_chat_deep.py:_api_v2_chat_hype_timeline`.
//! Auth + Extended-Plan-Gate, `streamer` (Pflicht), optionale `session_id`
//! (sonst letzte Session). Der Loader liefert den HTTP-Status (200/400/404) mit.

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
use tb_analytics::chat_hype_timeline::{load_chat_hype_timeline, HypeTimeline};

#[derive(Deserialize)]
pub struct HypeTimelineQuery {
    #[serde(default)]
    pub streamer: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
}

/// `GET /twitch/api/v2/chat-hype-timeline?streamer=&session_id=`
pub async fn chat_hype_timeline_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<HypeTimelineQuery>,
) -> impl IntoResponse {
    if let Some(resp) = crate::auth::extended_gate(&pool, &auth).await {
        return resp;
    }
    let streamer = match params.streamer.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => s.to_lowercase(),
        None => {
            return (StatusCode::BAD_REQUEST, Json(json!({ "error": "Streamer required" }))).into_response();
        }
    };
    let session_id_raw = params.session_id.as_deref().unwrap_or("").trim().to_string();

    match load_chat_hype_timeline(&pool, &streamer, &session_id_raw).await {
        Ok(HypeTimeline::Ok(v)) => Json(v).into_response(),
        Ok(HypeTimeline::BadRequest(v)) => (StatusCode::BAD_REQUEST, Json(v)).into_response(),
        Ok(HypeTimeline::NotFound(v)) => (StatusCode::NOT_FOUND, Json(v)).into_response(),
        Err(e) => {
            tracing::error!("chat-hype-timeline Fehler: {e}");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "internal" }))).into_response()
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
        let admin = PgPoolOptions::new().max_connections(1).connect(&dsn).await.unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).execute(&admin).await.unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&admin).await.unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn).unwrap().options([("search_path", schema)]);
        let pool = PgPoolOptions::new().max_connections(2).connect_with(opts).await.unwrap();
        sqlx::query("CREATE TABLE twitch_stream_sessions (id BIGSERIAL PRIMARY KEY, streamer_login TEXT, started_at TIMESTAMPTZ, duration_seconds INTEGER, stream_title TEXT)").execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE twitch_chat_messages (id BIGSERIAL PRIMARY KEY, session_id BIGINT, streamer_login TEXT, chatter_login TEXT, content TEXT, message_ts TIMESTAMPTZ)").execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE twitch_session_viewers (session_id BIGINT, minutes_from_start INTEGER, viewer_count INTEGER)").execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE twitch_session_chatters (session_id BIGINT, streamer_login TEXT, chatter_login TEXT, messages INTEGER DEFAULT 0)").execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE twitch_raw_chat_ingest_health (streamer_login TEXT PRIMARY KEY, last_raw_chat_message_at TEXT, last_raw_chat_insert_ok_at TEXT, last_raw_chat_insert_error_at TEXT, last_raw_chat_error TEXT)").execute(&pool).await.unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn streamer_pflicht_400() {
        let Some(pool) = make_pool("t_hype_h1").await else { return };
        let resp = chat_hype_timeline_handler(
            DashboardAuthLevel::admin(),
            State(pool),
            Query(HypeTimelineQuery { streamer: None, session_id: None }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn keine_session_404() {
        let Some(pool) = make_pool("t_hype_h2").await else { return };
        let resp = chat_hype_timeline_handler(
            DashboardAuthLevel::admin(),
            State(pool),
            Query(HypeTimelineQuery { streamer: Some("nani".into()), session_id: None }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
