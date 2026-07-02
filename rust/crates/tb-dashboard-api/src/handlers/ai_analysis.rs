//! Handler für `GET /twitch/api/v2/ai/analysis`.
//!
//! Port von `api_ai.py:_api_v2_ai_analysis`. Erstellt eine tiefe, daten-basierte
//! KI-Analyse (10-Punkte-Plan) via Claude Opus (Admin/Localhost ODER ein Plan mit
//! dem konsolidierten `analytics`-Flag). Verdrahtet die
//! Bausteine aus tb-analytics (collect_ai_context/build_prompt/parse) + die
//! LLM-Clients (tb-engagement) + den globalen [`crate::ai_state`].

use std::time::Duration;

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::{SecondsFormat, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgPool;

use crate::ai_state::{chat_session_key, ChatSession, AI_MODEL_OPUS, AI_STATE};
use crate::auth::level::DashboardAuthLevel;
use tb_analytics::ai_analysis::{
    build_ai_analysis_prompt, collect_ai_context, extract_text_response, model_name_for,
    parse_ai_analysis_points_with_context, plan_ai_model,
};
use tb_analytics::ai_history::save_analysis;
use tb_engagement::claude_chat::ClaudeClient;
use tb_engagement::minimax_chat::EngagementMinimaxClient;

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
pub async fn ai_analysis_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<AnalysisQuery>,
) -> impl IntoResponse {
    // _require_v2_auth: jede gültige Auth genügt, None → 401.
    if matches!(auth, DashboardAuthLevel::None) {
        return crate::auth::unauthorized_v2_response();
    }
    AI_STATE.lock().unwrap().cleanup(Utc::now());

    // IDOR-Guard: Partner werden auf den eigenen Login geklemmt (Cross-Account →
    // 403); Admin/Localhost dürfen `streamer` frei wählen.
    let streamer =
        match crate::auth::resolve_streamer_scope(&auth, params.streamer.as_deref(), true) {
            Ok(Some(s)) => s,
            Ok(None) => {
                return json_err(
                    StatusCode::BAD_REQUEST,
                    json!({ "error": "streamer parameter required" }),
                )
            }
            Err(resp) => return resp,
        };
    if AI_STATE.lock().unwrap().in_progress_contains(&streamer) {
        return json_err(
            StatusCode::CONFLICT,
            json!({ "error": "Analyse läuft bereits für diesen Streamer. Bitte warte bis sie abgeschlossen ist." }),
        );
    }
    // days: parse-or-30, clamp 7..365 (Python int()-ValueError → 30).
    let days = params
        .days
        .as_deref()
        .and_then(|d| d.trim().parse::<i64>().ok())
        .map(|d| d.clamp(7, 365))
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
                return json_err(
                    StatusCode::FORBIDDEN,
                    json!({
                        "error": "plan_required",
                        "required_entitlements": ["analytics"],
                        "required_plans": ["analysis_dashboard", "analytics_trial", "bundle_analysis_raid_boost", "bundle_komplett", "bundle_werbefrei_analyse"],
                    }),
                );
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

    // in_progress-Guard um den Lauf (Python try/finally).
    AI_STATE.lock().unwrap().in_progress_add(&streamer);
    let resp = run_ai_analysis(&pool, &streamer, days, game_filter, ai_model, &user_context).await;
    AI_STATE.lock().unwrap().in_progress_remove(&streamer);
    resp
}

/// LLM-Dispatch (Python `_call_ai_analysis`): Opus via ClaudeClient (max_tokens
/// 60000), MiniMax via raw_completion (temp 0.5, max_tokens 60000). Fehler als
/// String (Aufrufer prüft „credit balance is too low").
async fn call_ai_analysis(ai_model: &str, prompt: &str) -> Result<Vec<Value>, String> {
    if ai_model == AI_MODEL_OPUS {
        let client = ClaudeClient::new(None, None, None, None);
        let content = client
            .create_message(None, json!([{ "role": "user", "content": prompt }]), 60000)
            .await
            .map_err(|e| e.to_string())?;
        Ok(parse_ai_analysis_points_with_context(
            &extract_text_response(&content),
            ai_model,
            "ai-analysis",
        ))
    } else {
        let client = EngagementMinimaxClient::new(None, None, None, Some(Duration::from_secs(240)));
        let raw = client
            .raw_completion("", prompt, 60000, 0.5)
            .await
            .map_err(|e| e.to_string())?;
        Ok(parse_ai_analysis_points_with_context(&raw, ai_model, "ai-analysis"))
    }
}

async fn run_ai_analysis(
    pool: &PgPool,
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

    // Step 3: persistieren (best-effort) + Session anlegen.
    let record_id = save_analysis(
        pool,
        streamer,
        days,
        model_name_for(ai_model),
        generated_at,
        &summary,
        &points_value,
    )
    .await;

    let (session_key, follow_ups_remaining) = match record_id {
        Some(id) => {
            let key = chat_session_key(streamer, id);
            let session = ChatSession {
                model: ai_model.to_string(),
                streamer: streamer.to_string(),
                analysis_id: id,
                days,
                game_filter: game_filter.to_string(),
                user_context: user_context.to_string(),
                ctx: ctx.clone(),
                points: points_value.clone(),
                history: Vec::new(),
                follow_up_count: 0,
                created_at: generated_at,
            };
            let mut st = AI_STATE.lock().unwrap();
            st.insert_session(key.clone(), session);
            let (rem, _) = st.remaining_follow_ups(streamer, ai_model, 0, generated_at);
            (Value::String(key), rem)
        }
        None => (Value::Null, 0),
    };

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
        Some(
            PgPoolOptions::new()
                .max_connections(2)
                .connect_with(opts)
                .await
                .unwrap(),
        )
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
        let Some(pool) = make_pool("t_ai_an_401").await else {
            return;
        };
        let resp = ai_analysis_handler(
            DashboardAuthLevel::None,
            State(pool),
            Query(query(Some("nani"), None)),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn streamer_required_400() {
        let Some(pool) = make_pool("t_ai_an_str").await else {
            return;
        };
        let resp = ai_analysis_handler(
            DashboardAuthLevel::admin(),
            State(pool),
            Query(query(None, None)),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn in_progress_409() {
        let Some(pool) = make_pool("t_ai_an_409").await else {
            return;
        };
        // Eindeutiger Streamer-Name (globaler State) → vorbelegen.
        AI_STATE.lock().unwrap().in_progress_add("t6inprogstreamer");
        let resp = ai_analysis_handler(
            DashboardAuthLevel::admin(),
            State(pool),
            Query(query(Some("t6inprogstreamer"), None)),
        )
        .await
        .into_response();
        AI_STATE
            .lock()
            .unwrap()
            .in_progress_remove("t6inprogstreamer");
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
        let Some(pool) = make_pool("t_ai_an_idor").await else {
            return;
        };
        let resp = ai_analysis_handler(
            partner("earlysalty"),
            State(pool),
            Query(query(Some("ismile_e"), None)),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn user_context_too_long_400() {
        let Some(pool) = make_pool("t_ai_an_uc").await else {
            return;
        };
        let long = "x".repeat(2001);
        let resp = ai_analysis_handler(
            DashboardAuthLevel::admin(),
            State(pool),
            Query(query(Some("t6uctxstreamer"), Some(&long))),
        )
        .await
        .into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
}
