use std::time::Duration;

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;

use crate::auth::level::DashboardAuthLevel;

const ENFORCE_PATH: &str = "/internal/twitch/v1/scam-guard/enforce";
const REVOKE_PATH: &str = "/internal/twitch/v1/scam-guard/revoke";

#[derive(Deserialize, Default)]
pub struct ScamGuardQuery {
    #[serde(default)]
    pub streamer: Option<String>,
}

#[allow(clippy::result_large_err)]
fn resolve_login(auth: &DashboardAuthLevel, streamer: &Option<String>) -> Result<String, Response> {
    match auth {
        DashboardAuthLevel::Partner { twitch_login, .. } => Ok(twitch_login.to_lowercase()),
        DashboardAuthLevel::Admin { .. } | DashboardAuthLevel::Localhost => {
            match streamer.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                Some(s) => Ok(s.to_lowercase()),
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

fn error_response(status: StatusCode, code: &str) -> Response {
    (status, Json(json!({ "error": code }))).into_response()
}

fn nonempty_env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn worker_internal_base_url() -> String {
    if let Some(explicit) = nonempty_env("TWITCH_INTERNAL_API_BASE_URL") {
        return explicit.trim_end_matches('/').to_string();
    }
    let host = nonempty_env("TWITCH_INTERNAL_API_HOST").unwrap_or_else(|| "127.0.0.1".to_string());
    let port = nonempty_env("TWITCH_INTERNAL_API_PORT").unwrap_or_else(|| "8776".to_string());
    format!("http://{host}:{port}")
}

async fn proxy_for_owned_verdict(
    auth: DashboardAuthLevel,
    pool: PgPool,
    query: ScamGuardQuery,
    id: i64,
    path: &'static str,
) -> Response {
    let login = match resolve_login(&auth, &query.streamer) {
        Ok(login) => login,
        Err(response) => return response,
    };

    let owner = match sqlx::query_scalar::<_, String>(
        "SELECT channel_login FROM twitch_scam_guard_verdicts WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&pool)
    .await
    {
        Ok(owner) => owner,
        Err(error) => {
            tracing::error!(%error, "scam-guard enforce ownership database error");
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, "db");
        }
    };
    if owner.as_deref() != Some(login.as_str()) {
        return error_response(StatusCode::NOT_FOUND, "not found");
    }

    let Some(token) = nonempty_env("TWITCH_INTERNAL_API_TOKEN") else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let url = format!("{}{path}", worker_internal_base_url());
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_default();
    let upstream = match client
        .post(url)
        .header("X-Internal-Token", token)
        .json(&json!({ "verdictId": id }))
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            tracing::warn!(%error, "scam-guard internal proxy transport error");
            return error_response(StatusCode::BAD_GATEWAY, "upstream");
        }
    };
    let status = StatusCode::from_u16(upstream.status().as_u16())
        .unwrap_or(StatusCode::BAD_GATEWAY);
    let body = match upstream.bytes().await {
        Ok(body) => body,
        Err(error) => {
            tracing::warn!(%error, "scam-guard internal proxy body error");
            return error_response(StatusCode::BAD_GATEWAY, "upstream");
        }
    };
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body))
        .unwrap_or_else(|_| error_response(StatusCode::BAD_GATEWAY, "upstream"))
}

pub async fn ban_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<ScamGuardQuery>,
    Path(id): Path<i64>,
) -> Response {
    proxy_for_owned_verdict(auth, pool, query, id, ENFORCE_PATH).await
}

pub async fn revoke_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<ScamGuardQuery>,
    Path(id): Path<i64>,
) -> Response {
    proxy_for_owned_verdict(auth, pool, query, id, REVOKE_PATH).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use serde_json::{json, Value};
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
        sqlx::query(
            "CREATE TABLE twitch_scam_guard_verdicts (\
                id BIGSERIAL PRIMARY KEY,\
                channel_login TEXT NOT NULL\
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    fn partner(login: &str) -> DashboardAuthLevel {
        DashboardAuthLevel::Partner {
            twitch_login: login.into(),
            twitch_user_id: "42".into(),
            display_name: String::new(),
        }
    }

    async fn body_of(response: Response) -> (StatusCode, Value) {
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 65536).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    #[tokio::test]
    async fn ban_und_revoke_verbergen_fremde_und_unbekannte_ids() {
        let Some(pool) = make_pool("t_scam_guard_enforce_ownership").await else {
            return;
        };
        let foreign_id: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_scam_guard_verdicts (channel_login) \
             VALUES ('other') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        for id in [foreign_id, 9_999_999] {
            let (status, body) = body_of(
                ban_handler(
                    partner("nani"),
                    State(pool.clone()),
                    Query(ScamGuardQuery::default()),
                    Path(id),
                )
                .await,
            )
            .await;
            assert_eq!(status, StatusCode::NOT_FOUND);
            assert_eq!(body, json!({ "error": "not found" }));

            let (status, body) = body_of(
                revoke_handler(
                    partner("nani"),
                    State(pool.clone()),
                    Query(ScamGuardQuery::default()),
                    Path(id),
                )
                .await,
            )
            .await;
            assert_eq!(status, StatusCode::NOT_FOUND);
            assert_eq!(body, json!({ "error": "not found" }));
        }
    }
}
