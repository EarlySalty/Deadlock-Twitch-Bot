//! Handler für `GET /twitch/api/v2/ai/analysis`.
//!
//! Port von `api_ai.py:_api_v2_ai_analysis`. Erstellt eine tiefe, daten-basierte
//! KI-Analyse (10-Punkte-Plan) via Claude Opus (Admin/Localhost ODER ein Plan mit
//! dem konsolidierten `analytics`-Flag). Verdrahtet die
//! Bausteine aus tb-analytics (collect_ai_context/build_prompt/parse) + die
//! Bestehende KI-Aufrufe und den persistenten Analyse-Folgechat.

use std::time::Duration;

use axum::{
    extract::{Extension, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::{SecondsFormat, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgPool;

use crate::ai_state::{chat_session_key, ChatSession, AI_MODEL_OPUS};
use crate::ai_store::{AiStore, SessionGuard, StoreError};
use crate::auth::level::DashboardAuthLevel;
use tb_analytics::ai_analysis::{
    build_ai_analysis_prompt, collect_ai_context, extract_text_response, model_name_for,
    parse_ai_analysis_points_with_context, plan_ai_model,
};
use tb_analytics::ai_history::save_analysis;
use tb_engagement::llm_chat::EngagementLlmClient;

const MAX_USER_CONTEXT_CHARS: usize = 2000;

#[derive(Deserialize)]
pub struct AnalysisQuery {
    #[serde(default)]
    pub streamer: Option<String>,
    #[serde(default)]
    pub days: Option<String>,
    #[serde(default)]
    pub game_filter: Option<String>,
    #[serde(default)]
    pub user_context: Option<String>,
}

fn json_err(status: StatusCode, body: Value) -> Response {
    (status, Json(body)).into_response()
}

/// `GET /twitch/api/v2/ai/analysis?streamer=&days=&game_filter=&user_context=`
pub(crate) async fn ai_analysis_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Extension(store): Extension<AiStore>,
    Query(params): Query<AnalysisQuery>,
) -> impl IntoResponse {
    // _require_v2_auth: jede gültige Auth genügt, None → 401.
    if matches!(auth, DashboardAuthLevel::None) {
        return crate::auth::unauthorized_v2_response();
    }

    // IDOR-Guard: Partner werden auf den eigenen Login geklemmt (Cross-Account →
    // 403); Admin/Localhost dürfen `streamer` frei wählen.
    let (streamer, twitch_user_id) =
        match crate::auth::streamer_scope::resolve_analysis_target(&pool, &auth, params.streamer.as_deref()).await {
            Ok(target) => target,
            Err(resp) => return resp,
        };
    // days: parse-or-30, clamp 7..3650 (Python int()-ValueError → 30).
    let days = params
        .days
        .as_deref()
        .and_then(|d| d.trim().parse::<i64>().ok())
        .map(|d| d.clamp(7, 3650))
        .unwrap_or(30);
    // game_filter: deadlock|all, sonst all.
    let gf = params
        .game_filter
        .as_deref()
        .unwrap_or("all")
        .trim()
        .to_lowercase();
    let game_filter = if gf == "deadlock" { "deadlock" } else { "all" };
    let user_context = params
        .user_context
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_string();
    if user_context.chars().count() > MAX_USER_CONTEXT_CHARS {
        return json_err(
            StatusCode::BAD_REQUEST,
            json!({ "error": format!("user_context darf maximal {MAX_USER_CONTEXT_CHARS} Zeichen lang sein") }),
        );
    }

    // Modellwahl: Localhost/Admin → Opus; sonst Plan des Streamers.
    let ai_model: &str = if matches!(auth, DashboardAuthLevel::Admin { .. }) {
        AI_MODEL_OPUS
    } else {
        match plan_ai_model(&pool, &streamer).await {
            Ok(Some(m)) => m,
            Ok(None) => {
                // Eine Quelle fuer den 403-Body: derselbe Aufbau wie bei allen
                // anderen Plan-Sperren (vorher stand hier die alte Plan-Liste fest).
                return crate::auth::stufe_required_response(tb_analytics::stufe::Stufe::Plus);
            }
            Err(e) => {
                tracing::error!("ai/analysis plan-Auflösung fehlgeschlagen: {e}");
                return json_err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    json!({ "error": "KI-Analyse konnte nicht geladen werden.", "code": "ai_analysis_failed" }),
                );
            }
        }
    };

    let mut guard = match SessionGuard::acquire(&store.lock_pool, &format!("ai-analysis:{twitch_user_id}")).await {
        Ok(guard) => guard,
        Err(StoreError::Busy) => return json_err(StatusCode::CONFLICT, json!({"error":"analysis_in_progress"})),
        Err(error) => {
            tracing::error!(%error, "Analyse-Speicher nicht verfügbar");
            return json_err(StatusCode::SERVICE_UNAVAILABLE, json!({"error":"analysis_store_unavailable"}));
        }
    };
    run_ai_analysis(&pool, &mut guard, &twitch_user_id, &streamer, days, game_filter, ai_model, &user_context).await
}

/// LLM-Dispatch (Python `_call_ai_analysis`): Opus ueber den zentralen Eingang
/// `tb_llm::complete` mit Use-Case `ai_analysis` (max_tokens 60000), KI
/// via raw_completion (temp 0.5, max_tokens 60000). Fehler als String
/// (Aufrufer prüft „credit balance is too low").
async fn call_ai_analysis(ai_model: &str, prompt: &str) -> Result<Vec<Value>, String> {
    if ai_model == AI_MODEL_OPUS {
        let response = tb_llm::complete(
            "ai_analysis",
            tb_llm::Request::prompt(prompt)
                .max_tokens(60000)
                .ledger_purpose("ai-analysis"),
        )
        .await
        .map_err(|e| e.to_string())?;
        Ok(parse_ai_analysis_points_with_context(
            &extract_text_response(&Value::String(response.text)),
            ai_model,
            "ai-analysis",
        ))
    } else {
        let client = EngagementLlmClient::new(None, None, None, Some(Duration::from_secs(240)));
        let raw = client
            .raw_completion("", prompt, 60000, 0.5)
            .await
            .map_err(|e| e.to_string())?;
        Ok(parse_ai_analysis_points_with_context(
            &raw,
            ai_model,
            "ai-analysis",
        ))
    }
}

async fn run_ai_analysis(
    pool: &PgPool,
    guard: &mut SessionGuard,
    twitch_user_id: &str,
    streamer: &str,
    days: i64,
    game_filter: &str,
    ai_model: &str,
    user_context: &str,
) -> Response {
    let since = Utc::now() - chrono::Duration::days(days);

    // Step 1: Kontext sammeln.
    let ctx = match collect_ai_context(pool, streamer, since, game_filter).await {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("ai/analysis collect_ai_context Fehler: {e}");
            return json_err(
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({ "error": "Analyse-Daten konnten nicht gesammelt werden.", "code": "ai_context_collection_failed" }),
            );
        }
    };

    // Step 2: Modell aufrufen.
    let prompt = build_ai_analysis_prompt(streamer, days, &ctx, game_filter, user_context);
    let points = match call_ai_analysis(ai_model, &prompt).await {
        Ok(p) => p,
        Err(msg) => {
            tracing::error!("ai/analysis Modell-Fehler ({ai_model}): {msg}");
            if msg.contains("credit balance is too low") {
                return json_err(
                    StatusCode::SERVICE_UNAVAILABLE,
                    json!({ "error": "Kein Guthaben auf dem Anthropic-Konto. Bitte auf console.anthropic.com/billing Credits kaufen." }),
                );
            }
            return json_err(
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({ "error": "KI-Analyse konnte nicht abgeschlossen werden.", "code": "ai_analysis_failed" }),
            );
        }
    };

    let generated_at = Utc::now();
    let points_value = Value::Array(points);
    let summary = ctx.get("summary").cloned().unwrap_or_else(|| json!({}));

    // Beide Schreibpfade müssen vor der Erfolgsmeldung abgeschlossen sein.
    let record_id = match save_analysis(pool, streamer, days, model_name_for(ai_model), generated_at, &summary, &points_value).await {
        Some(id) => id,
        None => return json_err(StatusCode::SERVICE_UNAVAILABLE, json!({"error":"analysis_store_unavailable"})),
    };
    let session = ChatSession {
                model: ai_model.to_string(),
                streamer: streamer.to_string(),
                analysis_id: record_id,
                days,
                game_filter: game_filter.to_string(),
                user_context: user_context.to_string(),
                ctx: ctx.clone(),
                points: points_value.clone(),
                history: Vec::new(),
                follow_up_count: 0,
                created_at: generated_at,
    };
    let (follow_ups_remaining, _) = match crate::ai_store::save_session(guard, twitch_user_id, &session).await {
        Ok(remaining) => remaining,
        Err(error) => {
            tracing::error!(%error, "Analyse und Unterhaltung konnten nicht gespeichert werden");
            return json_err(StatusCode::SERVICE_UNAVAILABLE, json!({"error":"analysis_store_unavailable"}));
        }
    };
    let record_id = session.analysis_id;
    let session_key = chat_session_key(streamer, record_id);

    Json(json!({
        "id": record_id,
        "streamer": streamer,
        "days": days,
        "gameFilter": game_filter,
        "model": ai_model,
        "sessionKey": session_key,
        "followUpsRemaining": follow_ups_remaining,
        "generatedAt": generated_at.to_rfc3339_opts(SecondsFormat::Micros, false),
        "points": points_value,
        "dataSnapshot": summary,
    }))
    .into_response()
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

    fn query(streamer: Option<&str>, user_context: Option<&str>) -> AnalysisQuery {
        AnalysisQuery {
            streamer: streamer.map(String::from),
            days: None,
            game_filter: None,
            user_context: user_context.map(String::from),
        }
    }

    #[tokio::test]
    async fn none_auth_401() {
        let database = make_pool("t_ai_an_401").await;
        let pool = database.pool.clone();
        let resp = ai_analysis_handler(
            DashboardAuthLevel::None,
            State(pool.clone()), Extension(AiStore::new(&pool)),
            Query(query(Some("nani"), None)),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn streamer_required_400() {
        let database = make_pool("t_ai_an_str").await;
        let pool = database.pool.clone();
        let resp = ai_analysis_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()), Extension(AiStore::new(&pool)),
            Query(query(None, None)),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn in_progress_409() {
        let database = make_pool("t_ai_an_409").await;
        let pool = database.pool.clone();
        let store = AiStore::new(&pool);
        let guard = SessionGuard::acquire(&store.lock_pool, "ai-analysis:42").await.unwrap();
        let resp = ai_analysis_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()), Extension(AiStore::new(&pool)),
            Query(query(Some("t6inprogstreamer"), None)),
        )
        .await
        .into_response();
        drop(guard);
        assert_eq!(resp.status(), StatusCode::CONFLICT);
    }

    fn partner(login: &str) -> DashboardAuthLevel {
        DashboardAuthLevel::Partner {
            twitch_login: login.to_string(),
            twitch_user_id: "42".to_string(),
            display_name: login.to_string(),
        }
    }

    // IDOR-Guard: Partner mit fremdem ?streamer= → 403 (vor jedem DB-/LLM-Zugriff).
    #[tokio::test]
    async fn partner_fremder_streamer_403() {
        let database = make_pool("t_ai_an_idor").await;
        let pool = database.pool.clone();
        let resp = ai_analysis_handler(
            partner("earlysalty"),
            State(pool.clone()), Extension(AiStore::new(&pool)),
            Query(query(Some("ismile_e"), None)),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn user_context_too_long_400() {
        let database = make_pool("t_ai_an_uc").await;
        let pool = database.pool.clone();
        let long = "x".repeat(2001);
        let resp = ai_analysis_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()), Extension(AiStore::new(&pool)),
            Query(query(Some("t6uctxstreamer"), Some(&long))),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
}
