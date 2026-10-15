use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

pub async fn blocked(
    pool: &sqlx::PgPool,
    id: Option<&str>,
    login: Option<&str>,
) -> Option<Response> {
    match tb_llm::ai_guard::allowed_with_pool(pool, id, login).await {
        Ok(true) => None,
        Ok(false) => Some(
            (
                StatusCode::FORBIDDEN,
                Json(json!({
                    "error": "KI-Funktionen sind für den Server dieses Streamers ausgeschaltet.",
                    "code": "ai_disabled"
                })),
            )
                .into_response(),
        ),
        Err(_) => Some(
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({
                    "error": "KI-Schalter konnte nicht geprüft werden.",
                    "code": "ai_policy_unavailable"
                })),
            )
                .into_response(),
        ),
    }
}

#[derive(serde::Deserialize)]
pub struct CapabilitiesQuery {
    streamer: Option<String>,
}

pub async fn capabilities(
    auth: crate::auth::DashboardAuthLevel,
    axum::extract::State(pool): axum::extract::State<sqlx::PgPool>,
    axum::extract::Query(query): axum::extract::Query<CapabilitiesQuery>,
) -> Response {
    use crate::auth::DashboardAuthLevel;
    let requested = query
        .streamer
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let (id, login) = if let Some(id) =
        requested.filter(|value| value.bytes().all(|byte| byte.is_ascii_digit()))
    {
        match &auth {
            DashboardAuthLevel::Partner { twitch_user_id, .. } if twitch_user_id != id => {
                return StatusCode::FORBIDDEN.into_response()
            }
            DashboardAuthLevel::None => return crate::auth::unauthorized_v2_response(),
            _ => (Some(id.to_string()), None),
        }
    } else {
        if requested.is_some() {
            if let Err(response) =
                crate::auth::streamer_scope::resolve_streamer_scope(&auth, requested, true)
            {
                return response;
            }
        }
        let (login, id) =
            match crate::auth::streamer_scope::resolve_settings_target(&auth, &query.streamer) {
                Ok(target) => target,
                Err(response) => return response,
            };
        (Some(id), Some(login))
    };
    match tb_llm::ai_guard::allowed_with_pool(&pool, id.as_deref(), login.as_deref()).await {
        Ok(enabled) => Json(json!({"aiFeatures": enabled})).into_response(),
        Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}

#[cfg(test)]
pub(crate) async fn test_policy(pool: &sqlx::PgPool, login: &str) {
    sqlx::raw_sql("CREATE TABLE twitch_streamer_identities(twitch_user_id TEXT PRIMARY KEY,twitch_login TEXT NOT NULL)")
        .execute(pool).await.unwrap();
    sqlx::query("INSERT INTO twitch_streamer_identities VALUES ('42',$1)")
        .bind(login)
        .execute(pool)
        .await
        .unwrap();
    sqlx::raw_sql(include_str!(
        "../../../../migrations/20261015120000_streamer_ai_policy.sql"
    ))
    .execute(pool)
    .await
    .unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::DashboardAuthLevel;
    use axum::extract::{Query, State};

    #[tokio::test]
    async fn foreign_capability_and_preflight_are_disabled() {
        let db = crate::test_postgres::TestPostgres::start().await;
        test_policy(&db.pool, "home").await;
        assert!(blocked(&db.pool, Some("42"), Some("home")).await.is_none());
        let partner = DashboardAuthLevel::Partner {
            twitch_login: "home".into(),
            twitch_user_id: "42".into(),
            display_name: "Home".into(),
        };
        for streamer in ["foreign", "99"] {
            let response = capabilities(
                partner.clone(),
                State(db.pool.clone()),
                Query(CapabilitiesQuery {
                    streamer: Some(streamer.into()),
                }),
            )
            .await;
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
        }
        sqlx::query("UPDATE twitch_streamer_ai_policy SET guild_id=987654321,ai_features=false")
            .execute(&db.pool)
            .await
            .unwrap();
        let auth = DashboardAuthLevel::Partner {
            twitch_login: "home".into(),
            twitch_user_id: "42".into(),
            display_name: "Home".into(),
        };
        let response = capabilities(
            auth,
            State(db.pool.clone()),
            Query(CapabilitiesQuery { streamer: None }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).unwrap()["aiFeatures"],
            false
        );
        assert_eq!(
            blocked(&db.pool, Some("42"), None).await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
    }
}
