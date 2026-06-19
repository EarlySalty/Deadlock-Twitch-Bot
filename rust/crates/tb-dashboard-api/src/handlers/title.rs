//! Titelgenerator, Insights und Twitch-Kanaltitel-Update.

use std::sync::{Arc, OnceLock};

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;
use tb_chat::title_ai::{
    generate_title, GenerateTitleError, PromptHistoryItem, PromptKnowledgeItem, TitleRateLimiter,
};
use tb_chat::title_db;
use tb_crypto::FieldCipher;
use tb_raid::RaidAuthStore;

use crate::auth::level::DashboardAuthLevel;

static TITLE_RATE_LIMITER: OnceLock<TitleRateLimiter> = OnceLock::new();

#[derive(Deserialize)]
pub struct TitleSuggestBody {
    pub keywords: String,
    #[serde(default)]
    pub streamer: Option<String>,
    #[serde(default)]
    pub include_live: bool,
}

#[derive(Deserialize, Default)]
pub struct TitleQuery {
    #[serde(default)]
    pub streamer: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateTitleBody {
    pub title: String,
}

fn requested_login(
    auth: &DashboardAuthLevel,
    requested: Option<&str>,
) -> Result<String, (StatusCode, Json<serde_json::Value>)> {
    let requested = requested.unwrap_or("").trim().to_lowercase();
    match auth {
        DashboardAuthLevel::None => Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({"error":"unauthorized"})),
        )),
        DashboardAuthLevel::Partner { twitch_login, .. } => {
            let own = twitch_login.trim().to_lowercase();
            if !requested.is_empty() && requested != own {
                Err((
                    StatusCode::FORBIDDEN,
                    Json(
                        json!({"error":"Du kannst nur auf deinen eigenen Twitch-Account zugreifen."}),
                    ),
                ))
            } else {
                Ok(own)
            }
        }
        DashboardAuthLevel::Admin { actor } => {
            let actor_login = actor
                .as_ref()
                .map(|a| a.twitch_login.trim().to_lowercase())
                .unwrap_or_default();
            let login = if requested.is_empty() {
                actor_login
            } else {
                requested
            };
            if login.is_empty() {
                Err((
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error":"streamer required"})),
                ))
            } else {
                Ok(login)
            }
        }
        DashboardAuthLevel::Localhost => {
            if requested.is_empty() {
                Err((
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error":"streamer required"})),
                ))
            } else {
                Ok(requested)
            }
        }
    }
}

async fn resolve_user_id(pool: &PgPool, login: &str) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT twitch_user_id FROM twitch_streamers \
         WHERE LOWER(twitch_login) = $1 AND COALESCE(twitch_user_id, '') <> '' LIMIT 1",
    )
    .bind(login)
    .fetch_optional(pool)
    .await
}

pub async fn suggest_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Json(body): Json<TitleSuggestBody>,
) -> impl IntoResponse {
    let keywords = body.keywords.trim();
    if keywords.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"keywords required"})),
        )
            .into_response();
    }
    let login = match requested_login(&auth, body.streamer.as_deref()) {
        Ok(login) => login,
        Err(resp) => return resp.into_response(),
    };
    let user_id = match resolve_user_id(&pool, &login).await {
        Ok(Some(id)) => id,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error":"streamer not found"})),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!("title user-id lookup fehlgeschlagen: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error":"internal_error"})),
            )
                .into_response();
        }
    };

    let history = title_db::get_streamer_title_history(&pool, &user_id, 30).await;
    let own_avg = title_db::get_streamer_avg_viewers(&pool, &user_id).await;
    let analysis: Vec<serde_json::Value> = history
        .iter()
        .map(|item| {
            let avg = item.avg_viewers.unwrap_or(0.0);
            let followers = item.followers_start.unwrap_or(1).max(1) as f64;
            json!({
                "title": item.title,
                "avg_viewers": avg,
                "peak_viewers": item.peak_viewers.unwrap_or(0),
                "relative_perf": if own_avg > 0.0 { avg / own_avg } else { 0.0 },
                "engagement_rate": avg / followers,
            })
        })
        .collect();
    let prompt_history: Vec<PromptHistoryItem> = analysis
        .iter()
        .map(|item| PromptHistoryItem {
            title: item["title"].as_str().unwrap_or_default().to_string(),
            relative_perf: item["relative_perf"].as_f64(),
            engagement_rate: item["engagement_rate"].as_f64(),
        })
        .collect();
    let knowledge = title_db::get_top_knowledge_titles(&pool, 30).await;
    let prompt_knowledge: Vec<PromptKnowledgeItem> = knowledge
        .into_iter()
        .map(|item| PromptKnowledgeItem {
            title: item.title,
            normalized_score: item.normalized_score,
        })
        .collect();

    let limiter = TITLE_RATE_LIMITER.get_or_init(TitleRateLimiter::default);
    match generate_title(
        limiter,
        &user_id,
        keywords,
        &prompt_history,
        &prompt_knowledge,
        None,
        None,
        "dashboard",
    )
    .await
    {
        Ok(result) => Json(json!({
            "primary": result.primary,
            "alternatives": result.alternatives,
            "title_analysis": analysis.into_iter().take(20).collect::<Vec<_>>(),
            "live_context_used": body.include_live,
        }))
        .into_response(),
        Err(GenerateTitleError::RateLimit(rate)) => (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({"error":"rate_limit","retry_after":rate.retry_after})),
        )
            .into_response(),
        Err(GenerateTitleError::NoApiKey) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":"title_ai_unavailable"})),
        )
            .into_response(),
        Err(GenerateTitleError::Http(e)) => {
            tracing::error!("title generation fehlgeschlagen: {e}");
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({"error":"title_generation_failed"})),
            )
                .into_response()
        }
    }
}

pub async fn insights_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<TitleQuery>,
) -> impl IntoResponse {
    let login = match requested_login(&auth, query.streamer.as_deref()) {
        Ok(login) => login,
        Err(resp) => return resp.into_response(),
    };
    let Some(user_id) = resolve_user_id(&pool, &login).await.ok().flatten() else {
        return Json(json!({"insight": null})).into_response();
    };
    match title_db::get_latest_insight(&pool, &user_id).await {
        Ok(insight) => Json(json!({"insight": insight})).into_response(),
        Err(e) => {
            tracing::error!("title insight lookup fehlgeschlagen: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error":"internal_error"})),
            )
                .into_response()
        }
    }
}

pub async fn update_channel_title_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<TitleQuery>,
    Json(body): Json<UpdateTitleBody>,
) -> impl IntoResponse {
    let title = body.title.trim();
    if title.is_empty() || title.chars().count() > 140 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"title must contain 1 to 140 characters"})),
        )
            .into_response();
    }
    let login = match requested_login(&auth, query.streamer.as_deref()) {
        Ok(login) => login,
        Err(resp) => return resp.into_response(),
    };
    let user_id = match resolve_user_id(&pool, &login).await {
        Ok(Some(id)) => id,
        _ => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error":"streamer not found"})),
            )
                .into_response()
        }
    };

    let Ok(cipher) = FieldCipher::from_env() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":"token_store_unavailable"})),
        )
            .into_response();
    };
    let store = RaidAuthStore::new(pool.clone(), Arc::new(cipher));
    let scopes = store.get_scopes(&user_id).await.unwrap_or_default();
    if !scopes
        .iter()
        .any(|scope| scope == "channel:manage:broadcast")
    {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error":"scope_missing","required_scope":"channel:manage:broadcast"})),
        )
            .into_response();
    }
    let token = match store.load_decrypted_unrestricted(&user_id).await {
        Ok(Some(tokens)) if !tokens.needs_reauth => tokens.access_token,
        _ => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({"error":"reauth_required"})),
            )
                .into_response()
        }
    };
    let client_id = std::env::var("TWITCH_CLIENT_ID")
        .or_else(|_| std::env::var("TWITCH_BOT_CLIENT_ID"))
        .unwrap_or_default();
    if client_id.trim().is_empty() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":"twitch_client_unavailable"})),
        )
            .into_response();
    }
    let base = std::env::var("TWITCH_HELIX_BASE_URL")
        .unwrap_or_else(|_| "https://api.twitch.tv/helix".to_string());
    let response = reqwest::Client::new()
        .patch(format!("{}/channels", base.trim_end_matches('/')))
        .query(&[("broadcaster_id", user_id.as_str())])
        .header("Client-Id", client_id)
        .bearer_auth(token)
        .json(&json!({"title": title}))
        .send()
        .await;

    match response {
        Ok(resp) if resp.status().is_success() => {
            Json(json!({"ok":true,"title":title})).into_response()
        }
        Ok(resp) if resp.status() == reqwest::StatusCode::UNAUTHORIZED => (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error":"reauth_required"})),
        )
            .into_response(),
        Ok(resp) if resp.status() == reqwest::StatusCode::FORBIDDEN => (
            StatusCode::FORBIDDEN,
            Json(json!({"error":"scope_missing","required_scope":"channel:manage:broadcast"})),
        )
            .into_response(),
        Ok(resp) => {
            tracing::error!(status = %resp.status(), "Twitch-Titelupdate abgelehnt");
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({"error":"twitch_update_failed"})),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!("Twitch-Titelupdate fehlgeschlagen: {e}");
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({"error":"twitch_unavailable"})),
            )
                .into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn partner() -> DashboardAuthLevel {
        DashboardAuthLevel::Partner {
            twitch_login: "nani".into(),
            twitch_user_id: "1".into(),
            display_name: "Nani".into(),
        }
    }

    #[test]
    fn partner_darf_nur_eigenen_login_nutzen() {
        assert_eq!(requested_login(&partner(), None).unwrap(), "nani");
        assert!(requested_login(&partner(), Some("other")).is_err());
    }

    #[test]
    fn admin_braucht_auswahl_oder_actor() {
        assert!(requested_login(&DashboardAuthLevel::admin(), None).is_err());
        assert_eq!(
            requested_login(&DashboardAuthLevel::admin(), Some("Nani")).unwrap(),
            "nani"
        );
    }
}
