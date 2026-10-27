use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;
use tb_chat::follow_hint::{
    normalize_follow_hint_message, FOLLOW_HINT_DEFAULT_MESSAGE, FOLLOW_HINT_MAX_CHARS,
};

use crate::auth::level::DashboardAuthLevel;
use crate::auth::streamer_scope::resolve_settings_target as resolve_target;

#[derive(Deserialize, Default)]
pub struct FollowHintQuery {
    #[serde(default)]
    pub streamer: Option<String>,
}

#[derive(Deserialize)]
pub struct FollowHintUpdate {
    pub enabled: bool,
    #[serde(default)]
    pub message: Option<String>,
}

const SELECT_SQL: &str = "SELECT COALESCE(follow_hint_enabled, 0), follow_hint_message \
       FROM streamer_plans \
      WHERE ($2 = '' AND LOWER(COALESCE(twitch_login, '')) = $1) \
         OR ($2 <> '' AND twitch_user_id = $2) \
      LIMIT 1";

fn settings_json(enabled: bool, message: Option<String>) -> serde_json::Value {
    json!({
        "enabled": enabled,
        "message": message.unwrap_or_default(),
        "default_message": FOLLOW_HINT_DEFAULT_MESSAGE,
        "max_chars": FOLLOW_HINT_MAX_CHARS,
    })
}

fn db_error(error: sqlx::Error, action: &str) -> Response {
    tracing::error!(%error, action, "follow-hint-settings DB error");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": "db" })),
    )
        .into_response()
}

pub async fn get_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<FollowHintQuery>,
) -> Response {
    let (login, user_id) = match resolve_target(&auth, &query.streamer) {
        Ok(t) => t,
        Err(resp) => return resp,
    };
    match sqlx::query_as::<_, (i32, Option<String>)>(SELECT_SQL)
        .bind(&login)
        .bind(&user_id)
        .fetch_optional(&pool)
        .await
    {
        Ok(Some((enabled, message))) => Json(settings_json(enabled != 0, message)).into_response(),
        Ok(None) => Json(settings_json(false, None)).into_response(),
        Err(error) => db_error(error, "get"),
    }
}

pub async fn post_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<FollowHintQuery>,
    Json(body): Json<FollowHintUpdate>,
) -> Response {
    let (login, user_id) = match resolve_target(&auth, &query.streamer) {
        Ok(t) => t,
        Err(resp) => return resp,
    };
    let message = match normalize_follow_hint_message(body.message.as_deref().unwrap_or("")) {
        Ok(message) => message,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "message_too_long", "max_chars": FOLLOW_HINT_MAX_CHARS })),
            )
                .into_response()
        }
    };
    let flag = i32::from(body.enabled);

    let result = if !user_id.is_empty() {
        sqlx::query(
            "INSERT INTO streamer_plans (twitch_user_id, twitch_login, follow_hint_enabled, follow_hint_message) \
             VALUES ($1, $2, $3, $4) \
             ON CONFLICT (twitch_user_id) \
             DO UPDATE SET follow_hint_enabled = EXCLUDED.follow_hint_enabled, \
                           follow_hint_message = EXCLUDED.follow_hint_message, \
                           twitch_login = COALESCE(streamer_plans.twitch_login, EXCLUDED.twitch_login)",
        )
        .bind(&user_id)
        .bind(&login)
        .bind(flag)
        .bind(&message)
        .execute(&pool)
        .await
    } else {
        sqlx::query(
            "UPDATE streamer_plans SET follow_hint_enabled = $2, follow_hint_message = $3 \
              WHERE LOWER(COALESCE(twitch_login, '')) = $1",
        )
        .bind(&login)
        .bind(flag)
        .bind(&message)
        .execute(&pool)
        .await
    };

    match result {
        Ok(res) if res.rows_affected() > 0 => {
            let mut payload = settings_json(body.enabled, message);
            payload["ok"] = json!(true);
            Json(payload).into_response()
        }
        Ok(_) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "no plan row" })),
        )
            .into_response(),
        Err(error) => db_error(error, "post"),
    }
}
