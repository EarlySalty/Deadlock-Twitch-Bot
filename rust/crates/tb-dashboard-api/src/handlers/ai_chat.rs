//! Handler für `POST /twitch/api/v2/ai/chat`.
//!
//! Port von `api_ai.py:_api_v2_ai_chat`. Beantwortet Folgefragen zu einer bereits
//! erstellten KI-Analyse (Session aus [`crate::ai_state`]) via Claude Opus, mit
//! History-Kontext und Follow-up-Ratelimit. (Der KI-Pfad bleibt im Dispatch
//! für historische Sessions erhalten; neue Analysen nutzen ausschließlich Opus.)

use std::time::Duration;

use axum::{
    extract::{Extension, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::Utc;
use serde_json::{json, Value};
use sqlx::PgPool;

use crate::ai_state::{ChatSession, AI_MODEL_LLM, AI_MODEL_OPUS};
use crate::ai_store::{AiStore, StoreError};
use crate::auth::level::DashboardAuthLevel;
use tb_analytics::ai_analysis::{extract_text_response, plan_ai_model};
use tb_engagement::llm_chat::EngagementLlmClient;

fn json_err(status: StatusCode, body: Value) -> Response {
    (status, Json(body)).into_response()
}

/// System-Prompt des Folgechats (Python `_build_chat_system_prompt`): bündelt
/// den Analysekontext der Session zu einem JSON-Payload. Key-Order/Whitespace
/// des `json.dumps` weicht ab (serde kompakt+alphabetisch) — nur Prompt, nicht
/// beobachtbar (nicht-deterministische LLM-Antwort).
fn build_chat_system_prompt(session: &ChatSession) -> String {
    let ctx = &session.ctx;
    let sub = |k: &str, default: Value| ctx.get(k).cloned().unwrap_or(default);
    let payload = json!({
        "summary": sub("summary", json!({})),
        "recentSessions": sub("recentSessions", json!([])),
        "weekdayPerformance": sub("weekdayPerformance", json!([])),
        "bestSessions": sub("bestSessions", json!([])),
        "worstSessions": sub("worstSessions", json!([])),
        "gamePerformance": sub("gamePerformance", json!([])),
        "weeklyTrend": sub("weeklyTrend", json!([])),
        "deadlockSummary": sub("deadlockSummary", json!({})),
        "gameBreakdown": sub("gameBreakdown", json!([])),
        "analysisPoints": session.points.clone(),
        "userContext": session.user_context.clone(),
        "gameFilter": session.game_filter.clone(),
        "days": session.days,
        "streamer": session.streamer.clone(),
    });
    format!(
        "Du beantwortest Rueckfragen zu einer bereits erstellten Twitch-KI-Analyse. \
Nutze nur den folgenden Analysekontext und die bisherige Unterhaltung. \
Antworte praezise, konkret und datenbasiert. Erfinde keine zusaetzlichen Kennzahlen.\n\n\
=== ANALYSEKONTEXT ===\n{}",
        serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string())
    )
}

/// Gefilterte History-Messages (role ∈ {user, assistant} + nicht-leerer Content).
fn history_messages(session: &ChatSession) -> Vec<Value> {
    session
        .history
        .iter()
        .filter_map(|e| {
            let role = e.get("role").and_then(Value::as_str)?;
            if role != "user" && role != "assistant" {
                return None;
            }
            let content = e
                .get("content")
                .and_then(Value::as_str)
                .filter(|c| !c.is_empty())?;
            Some(json!({ "role": role, "content": content }))
        })
        .collect()
}

/// LLM-Dispatch des Folgechats (Python `_call_ai_chat`). Fehler als String.
struct ChatCallError { known_rejection: bool }
impl From<tb_llm::LlmError> for ChatCallError {
    fn from(error: tb_llm::LlmError) -> Self {
        let known_rejection = matches!(error, tb_llm::LlmError::Unavailable(_) |
            tb_llm::LlmError::Http { status: 400 | 401 | 403 | 404 | 409 | 422 | 429, .. });
        Self { known_rejection }
    }
}
impl From<tb_engagement::llm_chat::GenerateError> for ChatCallError {
    fn from(error: tb_engagement::llm_chat::GenerateError) -> Self {
        use tb_engagement::llm_chat::GenerateError;
        let known_rejection = match error {
            GenerateError::Unavailable(_) => true,
            GenerateError::Http(text) => text.strip_prefix("LLM-Aufruf fehlgeschlagen: HTTP ")
                .and_then(|rest| rest.split_whitespace().next())
                .and_then(|status| status.trim_end_matches(':').parse::<u16>().ok())
                .is_some_and(|status| matches!(status, 400 | 401 | 403 | 404 | 409 | 422 | 429)),
            _ => false,
        };
        Self { known_rejection }
    }
}
async fn call_ai_chat(session: &ChatSession, message: &str) -> Result<String, ChatCallError> {
    let system_prompt = build_chat_system_prompt(session);
    let history = history_messages(session);

    if session.model == AI_MODEL_OPUS {
        // Opus: system separat, messages = History + neue User-Message,
        // direkt als `Message` gebaut statt ueber ein JSON-Zwischenarray.
        let mut messages: Vec<tb_llm::Message> = history
            .iter()
            .map(|m| tb_llm::Message {
                role: m["role"].as_str().unwrap_or("user").to_string(),
                content: m["content"].as_str().unwrap_or_default().to_string(),
            })
            .collect();
        messages.push(tb_llm::Message::user(message));
        let response = tb_llm::complete(
            "ai_chat",
            tb_llm::Request::history(messages)
                .system(&system_prompt)
                .max_tokens(4000)
                .ledger_purpose("ai-chat"),
        )
        .await
        .map_err(ChatCallError::from)?;
        // Der Hub hat die Text-Bloecke bereits zusammengesetzt; der Aufruf
        // haelt die Paritaet zur frueheren Auswertung des content-Arrays.
        Ok(extract_text_response(&Value::String(response.text)))
    } else {
        // KI: messages = [system] + History + neue User-Message.
        let mut messages = vec![json!({ "role": "system", "content": system_prompt })];
        messages.extend(history);
        messages.push(json!({ "role": "user", "content": message }));
        let client = EngagementLlmClient::new(None, None, None, Some(Duration::from_secs(240)));
        // raw_text bereits getrimmt (= extract_text_response auf String).
        client
            .messages_completion(Value::Array(messages), 4000, 0.5)
            .await
            .map_err(ChatCallError::from)
    }
}

/// `int(payload["analysis_id"])` — Number (inkl. Float-Trunkierung) ODER
/// numerischer String; sonst `None`.
fn parse_analysis_id(v: Option<&Value>) -> Option<i64> {
    match v {
        Some(Value::Number(n)) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Some(Value::String(s)) => s.trim().parse::<i64>().ok(),
        _ => None,
    }
}

/// `POST /twitch/api/v2/ai/chat`  Body: `{streamer, analysis_id, message}`
pub(crate) async fn ai_chat_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Extension(store): Extension<AiStore>,
    body: String,
) -> impl IntoResponse {
    if matches!(auth, DashboardAuthLevel::None) {
        return crate::auth::unauthorized_v2_response();
    }

    let payload: Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => return json_err(StatusCode::BAD_REQUEST, json!({ "error": "invalid_json" })),
    };

    let requested_streamer = payload
        .get("streamer")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if requested_streamer.is_empty() {
        return json_err(
            StatusCode::BAD_REQUEST,
            json!({ "error": "streamer required" }),
        );
    }
    if let Err(response) = crate::auth::streamer_scope::resolve_streamer_scope(&auth, Some(requested_streamer), true) {
        return response;
    }
    let analysis_id = match parse_analysis_id(payload.get("analysis_id")) {
        Some(id) => id,
        None => {
            return json_err(
                StatusCode::BAD_REQUEST,
                json!({ "error": "analysis_id required" }),
            )
        }
    };
    let message = payload
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if message.is_empty() {
        return json_err(
            StatusCode::BAD_REQUEST,
            json!({ "error": "message required" }),
        );
    }

    let (streamer, twitch_user_id) = match crate::auth::streamer_scope::resolve_analysis_target(&pool, &auth, Some(requested_streamer)).await {
        Ok(target) => target,
        Err(resp) => return resp,
    };

    // Plan-Gate für Nicht-Admin/Localhost.
    if !matches!(auth, DashboardAuthLevel::Admin { .. }) {
        match plan_ai_model(&pool, &streamer).await {
            Ok(Some(_)) => {}
            Ok(None) => {
                return json_err(
                    StatusCode::FORBIDDEN,
                    json!({ "error": "plan_required", "required_entitlements": ["analytics"] }),
                );
            }
            Err(e) => {
                tracing::error!("ai/chat plan-Auflösung fehlgeschlagen: {e}");
                return json_err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    json!({ "error": "KI-Chat konnte nicht geladen werden.", "code": "ai_chat_failed" }),
                );
            }
        }
    }

    let mut turn = match store.reserve(&twitch_user_id, analysis_id).await {
        Ok(turn) => turn,
        Err(StoreError::NotFound) => return json_err(StatusCode::NOT_FOUND, json!({"error":"chat_session_not_found"})),
        Err(StoreError::Busy) => return json_err(StatusCode::CONFLICT, json!({"error":"chat_turn_in_progress"})),
        Err(StoreError::Limit { reset }) => return json_err(StatusCode::TOO_MANY_REQUESTS,
            json!({"error":"follow_up_limit_reached", "followUpsRemaining":0,
                "retry_after":reset.map(|at| (at - Utc::now().timestamp()).max(0)), "rateLimitReset":reset})),
        Err(error) => {
            tracing::error!(%error, "Analyse-Unterhaltung nicht verfügbar");
            return json_err(StatusCode::SERVICE_UNAVAILABLE, json!({"error":"analysis_store_unavailable"}));
        }
    };
    let model = turn.session.model.clone();
    let reply = match call_ai_chat(&turn.session, &message).await {
        Ok(r) if !r.trim().is_empty() => r,
        Ok(_) => {
            if let Err(error) = turn.fail(false).await {
                tracing::error!(%error, "Analyse-Anfragestatus konnte nicht gespeichert werden");
                return json_err(StatusCode::SERVICE_UNAVAILABLE, json!({"error":"analysis_store_unavailable"}));
            }
            return json_err(StatusCode::BAD_GATEWAY, json!({"error":"KI-Chat konnte nicht geladen werden.", "code":"ai_chat_failed", "outcomeUnknown":true}));
        }
        Err(error) => {
            let known_rejection = error.known_rejection;
            if let Err(error) = turn.fail(known_rejection).await {
                tracing::error!(%error, "Analyse-Anfragestatus konnte nicht gespeichert werden");
                return json_err(StatusCode::SERVICE_UNAVAILABLE, json!({"error":"analysis_store_unavailable"}));
            }
            return json_err(
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({ "error": "KI-Chat konnte nicht geladen werden.", "code": "ai_chat_failed", "outcomeUnknown":!known_rejection }),
            );
        }
    };
    let (remaining_after, reset_ts2) = match turn.complete(&message, &reply).await {
        Ok(remaining) => remaining,
        Err(error) => {
            tracing::error!(%error, "Antwort und Analyse-Kontingent konnten nicht bestätigt werden");
            return json_err(StatusCode::SERVICE_UNAVAILABLE, json!({"error":"analysis_store_unavailable"}));
        }
    };

    let mut response = json!({ "message": reply, "followUpsRemaining": remaining_after });
    if model == AI_MODEL_LLM {
        if let Some(reset) = reset_ts2 {
            response["rateLimitReset"] = json!(reset);
        }
    }
    Json(response).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_postgres as postgres;
    async fn make_pool(_schema: &str) -> postgres::TestPostgres {
        let database = postgres::TestPostgres::start().await;
        sqlx::raw_sql("CREATE TABLE ai_analyses(id BIGINT PRIMARY KEY); INSERT INTO ai_analyses VALUES (7); CREATE TABLE twitch_streamers(twitch_login TEXT, twitch_user_id TEXT); INSERT INTO twitch_streamers VALUES ('nani','42'),('t6inprogstreamer','42'),('t6uctxstreamer','42'),('t6chatnosession','42'),('t6chat429','42');")
            .execute(&database.pool).await.unwrap();
        sqlx::raw_sql(include_str!("../../../../migrations/20261003010000_ai_chat_persistenz.sql"))
            .execute(&database.pool).await.unwrap();
        database
    }

    fn session(model: &str) -> ChatSession {
        ChatSession {
            model: model.to_string(),
            streamer: "nani".into(),
            analysis_id: 7,
            days: 30,
            game_filter: "all".into(),
            user_context: "Kontext".into(),
            ctx: json!({"summary": {"streamCount": 5}}),
            points: json!([{"number": 1}]),
            history: Vec::new(),
            follow_up_count: 0,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn system_prompt_enthaelt_kontext() {
        let p = build_chat_system_prompt(&session(AI_MODEL_OPUS));
        assert!(p.starts_with("Du beantwortest Rueckfragen"));
        assert!(p.contains("=== ANALYSEKONTEXT ==="));
        assert!(p.contains("\"streamCount\":5"));
        assert!(p.contains("\"userContext\":\"Kontext\""));
        assert!(p.contains("\"streamer\":\"nani\""));
    }

    #[test]
    fn history_filtert_rollen_und_leer() {
        let mut s = session(AI_MODEL_OPUS);
        s.history = vec![
            json!({"role": "user", "content": "frage"}),
            json!({"role": "system", "content": "ignorier mich"}),
            json!({"role": "assistant", "content": ""}),
            json!({"role": "assistant", "content": "antwort"}),
        ];
        let msgs = history_messages(&s);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0]["content"], "frage");
        assert_eq!(msgs[1]["content"], "antwort");
    }

    #[test]
    fn analysis_id_parsing() {
        assert_eq!(parse_analysis_id(Some(&json!(7))), Some(7));
        assert_eq!(parse_analysis_id(Some(&json!("12"))), Some(12));
        assert_eq!(parse_analysis_id(Some(&json!(3.9))), Some(3)); // int(3.9)=3
        assert_eq!(parse_analysis_id(Some(&json!("abc"))), None);
        assert_eq!(parse_analysis_id(None), None);
    }

    #[tokio::test]
    async fn none_auth_401() {
        let database = make_pool("t_ai_chat_401").await;
        let pool = database.pool.clone();
        let resp = ai_chat_handler(DashboardAuthLevel::None, State(pool.clone()), Extension(AiStore::new(&pool)), "{}".into())
            .await
            .into_response();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn invalid_json_400() {
        let database = make_pool("t_ai_chat_json").await;
        let pool = database.pool.clone();
        let resp = ai_chat_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()), Extension(AiStore::new(&pool)),
            "nicht json".into(),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn fehlende_felder_400() {
        let database = make_pool("t_ai_chat_fields").await;
        let pool = database.pool.clone();
        // analysis_id fehlt.
        let resp = ai_chat_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()), Extension(AiStore::new(&pool)),
            json!({"streamer": "nani", "message": "hi"}).to_string(),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn partner_fremder_streamer_403() {
        let database = make_pool("t_ai_chat_owner_mismatch").await;
        let pool = database.pool.clone();
        let resp = ai_chat_handler(
            DashboardAuthLevel::Partner {
                twitch_login: "owner".into(),
                twitch_user_id: "42".into(),
                display_name: "Owner".into(),
            },
            State(pool.clone()), Extension(AiStore::new(&pool)),
            json!({"streamer": "other", "analysis_id": 7, "message": "hi"}).to_string(),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn session_not_found_404() {
        let database = make_pool("t_ai_chat_404").await;
        let pool = database.pool.clone();
        // Localhost (kein Plan-Gate), gültige Felder, aber keine Session.
        let resp = ai_chat_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()), Extension(AiStore::new(&pool)),
            json!({"streamer": "t6chatnosession", "analysis_id": 999999, "message": "hi"})
                .to_string(),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn opus_limit_erschoepft_429() {
        let database = make_pool("t_ai_chat_429").await;
        let pool = database.pool.clone();
        // Session mit aufgebrauchtem Opus-Limit (follow_up_count = 3).
        let mut s = session(AI_MODEL_OPUS);
        s.streamer = "t6chat429".into();
        s.follow_up_count = 3;
        let store = AiStore::new(&pool);
        let mut guard = crate::ai_store::SessionGuard::acquire(&store.lock_pool, "test-save").await.unwrap();
        crate::ai_store::save_session(&mut guard, "42", &s).await.unwrap();
        sqlx::query("UPDATE twitch_ai_chat_sessions SET follow_up_count = 3").execute(&pool).await.unwrap();
        drop(guard);
        let resp = ai_chat_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()), Extension(AiStore::new(&pool)),
            json!({"streamer": "t6chat429", "analysis_id": 7, "message": "hi"}).to_string(),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
    }
}
