//! Handler für `/twitch/api/v2/chat-deep-minimax`.
//!
//! Port von `bot/analytics/api_chat_deep.py:_api_v2_chat_minimax_deep`.
//! Auth + Extended-Plan-Gate + Extern-LLM-Consent (403 ohne), `streamer` +
//! `session_id` Pflicht (400). Holt die Session-Chat-Nachrichten und lässt sie
//! von MiniMax (LLM) in Kategorien/Chat-Tiefe/Top-Themen klassifizieren.

use std::time::Duration;

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
use tb_analytics::chat_deep_minimax::{
    build_deep_prompt, extract_json_object, fetch_session_messages,
};
use tb_engagement::minimax_chat::EngagementMinimaxClient;

/// Python setzt kein `max_tokens` (MiniMax-Server-Default). Der Engagement-
/// Client verlangt einen Wert → großzügig, damit das kleine Antwort-JSON
/// (Counts + kurze Begründung + 3 Themen) nicht abgeschnitten wird.
const MAX_DEEP_TOKENS: i64 = 8192;

#[derive(Deserialize)]
pub struct ChatDeepQuery {
    #[serde(default)]
    pub streamer: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
}

fn analytics_error(msg: String) -> axum::response::Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": msg, "code": "analytics_request_failed" })),
    )
        .into_response()
}

/// `GET /twitch/api/v2/chat-deep-minimax?streamer=&session_id=`
pub async fn chat_deep_minimax_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<ChatDeepQuery>,
) -> impl IntoResponse {
    if let Some(resp) = crate::auth::extended_gate(&pool, &auth).await {
        return resp;
    }
    if !tb_social_media::settings::external_llm_consent(&pool).await {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "External LLM consent not given" })),
        )
            .into_response();
    }
    // IDOR-Guard: streamer ist Pflicht und wird (wie in Python) für die Query
    // nicht direkt genutzt, aber Partner dürfen nur ihren eigenen Login angeben
    // (Cross-Account → 403); Admin/Localhost frei. `required=true`.
    let _streamer =
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
    let session_id = match params
        .session_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(s) => s.to_string(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "Session ID required" })),
            )
                .into_response();
        }
    };

    let messages = match fetch_session_messages(&pool, &session_id).await {
        Ok(m) => m,
        Err(e) => {
            tracing::error!("chat-deep-minimax Fetch-Fehler: {e}");
            return analytics_error(format!("MiniMax Analyse fehlgeschlagen: {e}"));
        }
    };
    if messages.is_empty() {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "No messages found for this session" })),
        )
            .into_response();
    }

    let prompt = build_deep_prompt(&messages);
    // Wie Python: einzelne User-Message, temperature 0.1, 240s-Timeout.
    let client = EngagementMinimaxClient::new(None, None, None, Some(Duration::from_secs(240)));
    match client
        .raw_completion("", &prompt, MAX_DEEP_TOKENS, 0.1)
        .await
    {
        Ok(content) => {
            let json_str = extract_json_object(&content);
            match serde_json::from_str::<serde_json::Value>(json_str) {
                Ok(v) => Json(v).into_response(),
                Err(e) => analytics_error(format!("MiniMax Analyse fehlgeschlagen: {e}")),
            }
        }
        Err(e) => analytics_error(format!("MiniMax Analyse fehlgeschlagen: {e}")),
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
        sqlx::query("CREATE TABLE social_media_settings (key TEXT PRIMARY KEY, value JSONB, updated_at TIMESTAMPTZ, updated_by TEXT)")
            .execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE twitch_chat_messages (id BIGSERIAL PRIMARY KEY, session_id BIGINT, chatter_login TEXT, content TEXT, message_ts TIMESTAMPTZ)")
            .execute(&pool).await.unwrap();
        Some(pool)
    }

    async fn set_consent(pool: &PgPool, on: bool) {
        tb_social_media::settings::set_setting(pool, "external_llm_consent", &json!(on), None)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn ohne_consent_403() {
        let Some(pool) = make_pool("t_deep_h_403").await else {
            return;
        };
        // Consent nicht gesetzt → false.
        let resp = chat_deep_minimax_handler(
            DashboardAuthLevel::admin(),
            State(pool),
            Query(ChatDeepQuery {
                streamer: Some("nani".into()),
                session_id: Some("5".into()),
            }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    fn partner(login: &str) -> DashboardAuthLevel {
        DashboardAuthLevel::Partner {
            twitch_login: login.to_string(),
            twitch_user_id: "42".to_string(),
            display_name: login.to_string(),
        }
    }

    /// IDOR-Guard: Partner mit fremdem ?streamer= → 403, auch mit Consent.
    /// (Plan-Gate ODER Scope-Guard greift — fremde Session-Daten bleiben dicht.)
    #[tokio::test]
    async fn partner_fremder_streamer_403() {
        let Some(pool) = make_pool("t_deep_h_idor").await else {
            return;
        };
        set_consent(&pool, true).await;
        sqlx::query("CREATE TABLE IF NOT EXISTS streamer_plans (twitch_login TEXT, plan_id TEXT)")
            .execute(&pool)
            .await
            .ok();
        let resp = chat_deep_minimax_handler(
            partner("earlysalty"),
            State(pool),
            Query(ChatDeepQuery {
                streamer: Some("ismile_e".into()),
                session_id: Some("5".into()),
            }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn streamer_pflicht_400() {
        let Some(pool) = make_pool("t_deep_h_str").await else {
            return;
        };
        set_consent(&pool, true).await;
        let resp = chat_deep_minimax_handler(
            DashboardAuthLevel::admin(),
            State(pool),
            Query(ChatDeepQuery {
                streamer: None,
                session_id: Some("5".into()),
            }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn session_pflicht_400() {
        let Some(pool) = make_pool("t_deep_h_sess").await else {
            return;
        };
        set_consent(&pool, true).await;
        let resp = chat_deep_minimax_handler(
            DashboardAuthLevel::admin(),
            State(pool),
            Query(ChatDeepQuery {
                streamer: Some("nani".into()),
                session_id: None,
            }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn keine_nachrichten_404() {
        let Some(pool) = make_pool("t_deep_h_404").await else {
            return;
        };
        set_consent(&pool, true).await;
        // Consent ok, streamer+session da, aber keine Nachrichten → 404 (kein LLM-Call).
        let resp = chat_deep_minimax_handler(
            DashboardAuthLevel::admin(),
            State(pool),
            Query(ChatDeepQuery {
                streamer: Some("nani".into()),
                session_id: Some("5".into()),
            }),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
