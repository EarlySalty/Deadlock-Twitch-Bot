//! GET/POST `/twitch/api/v2/streamer/greeting-settings`.
//!
//! Streamer-Selbstbedienung fuer den automatischen Rueckgruss im Chat auf
//! `streamer_plans.greeting_reply_enabled` (INTEGER, Default 1). Gelesen wird
//! die Spalte im Bot von `tb_chat::StandardReplies`.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::json;
use sqlx::{PgPool, Row};

use crate::auth::level::DashboardAuthLevel;

#[derive(Deserialize, Default)]
pub struct GreetingQuery {
    /// Nur fuer Admin/Localhost relevant; Partner nutzen ihren Session-Login.
    #[serde(default)]
    pub streamer: Option<String>,
}

#[derive(Deserialize)]
pub struct GreetingUpdate {
    pub greeting_reply_enabled: bool,
}

#[allow(clippy::result_large_err)]
fn resolve_target(
    auth: &DashboardAuthLevel,
    streamer: &Option<String>,
) -> Result<(String, String), Response> {
    match auth {
        DashboardAuthLevel::Partner {
            twitch_login,
            twitch_user_id,
            ..
        } => Ok((
            twitch_login.to_lowercase(),
            twitch_user_id.trim().to_string(),
        )),
        DashboardAuthLevel::Admin { .. } => {
            match streamer.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                Some(s) => Ok((s.to_lowercase(), String::new())),
                None => Err((
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "streamer required" })),
                )
                    .into_response()),
            }
        }
        DashboardAuthLevel::None => Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "unauthorized" })),
        )
            .into_response()),
    }
}

const SELECT_SQL: &str = "SELECT COALESCE(greeting_reply_enabled, 1) AS enabled \
       FROM streamer_plans \
      WHERE LOWER(COALESCE(twitch_login, '')) = $1 \
         OR ($2 <> '' AND twitch_user_id = $2) \
      LIMIT 1";

pub async fn get_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<GreetingQuery>,
) -> Response {
    let (login, user_id) = match resolve_target(&auth, &query.streamer) {
        Ok(t) => t,
        Err(resp) => return resp,
    };

    match sqlx::query(SELECT_SQL)
        .bind(&login)
        .bind(&user_id)
        .fetch_optional(&pool)
        .await
    {
        Ok(Some(row)) => {
            let enabled: i32 = row.try_get("enabled").unwrap_or(1);
            Json(json!({ "greeting_reply_enabled": enabled != 0 })).into_response()
        }
        Ok(None) => Json(json!({ "greeting_reply_enabled": true })).into_response(),
        Err(error) => {
            tracing::error!(%error, "greeting-settings GET DB-Fehler");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "db" })),
            )
                .into_response()
        }
    }
}

pub async fn post_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<GreetingQuery>,
    Json(body): Json<GreetingUpdate>,
) -> Response {
    let (login, user_id) = match resolve_target(&auth, &query.streamer) {
        Ok(t) => t,
        Err(resp) => return resp,
    };
    let flag = i32::from(body.greeting_reply_enabled);

    let result = if !user_id.is_empty() {
        sqlx::query(
            "INSERT INTO streamer_plans (twitch_user_id, twitch_login, greeting_reply_enabled) \
             VALUES ($1, $2, $3) \
             ON CONFLICT (twitch_user_id) \
             DO UPDATE SET greeting_reply_enabled = EXCLUDED.greeting_reply_enabled, \
                           twitch_login = COALESCE(streamer_plans.twitch_login, EXCLUDED.twitch_login)",
        )
        .bind(&user_id)
        .bind(&login)
        .bind(flag)
        .execute(&pool)
        .await
    } else {
        sqlx::query(
            "UPDATE streamer_plans SET greeting_reply_enabled = $2 \
              WHERE LOWER(COALESCE(twitch_login, '')) = $1",
        )
        .bind(&login)
        .bind(flag)
        .execute(&pool)
        .await
    };

    match result {
        Ok(res) if res.rows_affected() > 0 => {
            Json(json!({ "ok": true, "greeting_reply_enabled": body.greeting_reply_enabled }))
                .into_response()
        }
        Ok(_) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "no plan row" })),
        )
            .into_response(),
        Err(error) => {
            tracing::error!(%error, "greeting-settings POST DB-Fehler");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "db" })),
            )
                .into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        extract::{Query, State},
        http::StatusCode,
        response::Response,
        Json,
    };
    use serde_json::Value;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use sqlx::PgPool;
    use std::str::FromStr;

    use crate::auth::level::DashboardAuthLevel;

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
        sqlx::query(
            "CREATE TABLE streamer_plans (twitch_user_id TEXT PRIMARY KEY, twitch_login TEXT, \
             plan_name TEXT DEFAULT 'free' NOT NULL, greeting_reply_enabled INTEGER DEFAULT 1 NOT NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    fn partner(login: &str, uid: &str) -> DashboardAuthLevel {
        DashboardAuthLevel::Partner {
            twitch_login: login.into(),
            twitch_user_id: uid.into(),
            display_name: String::new(),
        }
    }

    async fn body_of(resp: Response) -> (StatusCode, Value) {
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), 65536).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    #[tokio::test]
    async fn partner_toggle_roundtrip_default_an() {
        let Some(pool) = make_pool("t_greeting_api_partner").await else {
            return;
        };

        let (s, j) = body_of(
            get_handler(
                partner("nani", "42"),
                State(pool.clone()),
                Query(GreetingQuery::default()),
            )
            .await,
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(j["greeting_reply_enabled"], true);

        let (s, j) = body_of(
            post_handler(
                partner("nani", "42"),
                State(pool.clone()),
                Query(GreetingQuery::default()),
                Json(GreetingUpdate {
                    greeting_reply_enabled: false,
                }),
            )
            .await,
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(j["greeting_reply_enabled"], false);

        let (_s, j) = body_of(
            get_handler(
                partner("nani", "42"),
                State(pool),
                Query(GreetingQuery::default()),
            )
            .await,
        )
        .await;
        assert_eq!(j["greeting_reply_enabled"], false);
    }

    #[tokio::test]
    async fn admin_toggle_roundtrip_per_streamer_query() {
        let Some(pool) = make_pool("t_greeting_api_admin").await else {
            return;
        };
        sqlx::query(
            "INSERT INTO streamer_plans (twitch_user_id, twitch_login, greeting_reply_enabled) \
             VALUES ('99', 'target', 1)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let query = GreetingQuery {
            streamer: Some("target".to_string()),
        };
        let (s, j) = body_of(
            post_handler(
                DashboardAuthLevel::admin(),
                State(pool.clone()),
                Query(query),
                Json(GreetingUpdate {
                    greeting_reply_enabled: false,
                }),
            )
            .await,
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(j["greeting_reply_enabled"], false);

        let (s, j) = body_of(
            get_handler(
                DashboardAuthLevel::admin(),
                State(pool),
                Query(GreetingQuery {
                    streamer: Some("target".to_string()),
                }),
            )
            .await,
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(j["greeting_reply_enabled"], false);
    }

    #[tokio::test]
    async fn unauthorized_returns_401() {
        let Some(pool) = make_pool("t_greeting_api_auth").await else {
            return;
        };
        let (s, _) = body_of(
            get_handler(
                DashboardAuthLevel::None,
                State(pool),
                Query(GreetingQuery::default()),
            )
            .await,
        )
        .await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
    }
}
