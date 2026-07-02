//! GET/POST `/twitch/api/v2/streamer/scam-guard/settings`.
//!
//! Partner verwalten die Einstellungen ihres eigenen Kanals. Admin adressiert
//! einen Kanal über `?streamer=`.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;

use crate::auth::level::DashboardAuthLevel;

const VALID_MODES: [&str; 3] = ["auto_ban", "timeout", "alert_only"];

#[derive(Deserialize, Default)]
pub struct ScamGuardQuery {
    /// Nur für Admin relevant; Partner nutzen ihren Session-Login.
    #[serde(default)]
    pub streamer: Option<String>,
}

#[derive(Deserialize)]
pub struct ScamGuardUpdate {
    pub enabled: bool,
    pub mode: String,
    pub threshold: f32,
    pub suggestion_floor: f32,
}

#[allow(clippy::result_large_err)]
fn resolve_login(auth: &DashboardAuthLevel, streamer: &Option<String>) -> Result<String, Response> {
    match auth {
        DashboardAuthLevel::Partner { twitch_login, .. } => Ok(twitch_login.to_lowercase()),
        DashboardAuthLevel::Admin { .. } => {
            match streamer.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                Some(s) => Ok(s.to_lowercase()),
                None => Err((
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": "streamer_required",
                        "message": "streamer is required"
                    })),
                )
                    .into_response()),
            }
        }
        DashboardAuthLevel::None => Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": "unauthorized",
                "message": "authentication required"
            })),
        )
            .into_response()),
    }
}

fn error_response(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(json!({ "error": code, "message": message }))).into_response()
}

fn valid_thresholds(threshold: f32, suggestion_floor: f32) -> bool {
    0.0 <= suggestion_floor && suggestion_floor <= threshold && threshold <= 1.0
}

pub async fn get_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<ScamGuardQuery>,
) -> Response {
    let login = match resolve_login(&auth, &query.streamer) {
        Ok(login) => login,
        Err(response) => return response,
    };

    match sqlx::query!(
        "SELECT enabled, mode, threshold::float8 AS \"threshold!\", \
                suggestion_floor::float8 AS \"suggestion_floor!\" \
           FROM twitch_scam_guard_settings \
          WHERE channel_login = $1",
        login
    )
    .fetch_optional(&pool)
    .await
    {
        Ok(Some(row)) => Json(json!({
            "enabled": row.enabled,
            "mode": row.mode,
            "threshold": row.threshold,
            "suggestion_floor": row.suggestion_floor
        }))
        .into_response(),
        Ok(None) => Json(json!({
            "enabled": true,
            "mode": "auto_ban",
            "threshold": 0.90,
            "suggestion_floor": 0.70
        }))
        .into_response(),
        Err(error) => {
            tracing::error!(%error, "scam-guard settings GET database error");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "failed to load scam-guard settings",
            )
        }
    }
}

pub async fn post_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<ScamGuardQuery>,
    Json(body): Json<ScamGuardUpdate>,
) -> Response {
    let login = match resolve_login(&auth, &query.streamer) {
        Ok(login) => login,
        Err(response) => return response,
    };

    if !VALID_MODES.contains(&body.mode.as_str()) {
        return error_response(StatusCode::BAD_REQUEST, "invalid_mode", "invalid mode");
    }
    if !valid_thresholds(body.threshold, body.suggestion_floor) {
        return error_response(
            StatusCode::BAD_REQUEST,
            "invalid_thresholds",
            "invalid thresholds",
        );
    }

    let result = sqlx::query!(
        "INSERT INTO twitch_scam_guard_settings \
             (channel_login, enabled, mode, threshold, suggestion_floor) \
         VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT (channel_login) DO UPDATE SET \
             enabled = EXCLUDED.enabled, \
             mode = EXCLUDED.mode, \
             threshold = EXCLUDED.threshold, \
             suggestion_floor = EXCLUDED.suggestion_floor",
        login,
        body.enabled,
        &body.mode,
        body.threshold,
        body.suggestion_floor
    )
    .execute(&pool)
    .await;

    match result {
        Ok(_) => Json(json!({
            "ok": true,
            "enabled": body.enabled,
            "mode": body.mode,
            "threshold": body.threshold,
            "suggestion_floor": body.suggestion_floor
        }))
        .into_response(),
        Err(error) => {
            tracing::error!(%error, "scam-guard settings POST database error");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "failed to save scam-guard settings",
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;
    use serde_json::json;
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
            "CREATE TABLE twitch_scam_guard_settings (\
                channel_login TEXT PRIMARY KEY,\
                enabled BOOLEAN NOT NULL DEFAULT TRUE,\
                mode TEXT NOT NULL DEFAULT 'auto_ban',\
                threshold REAL NOT NULL DEFAULT 0.90,\
                suggestion_floor REAL NOT NULL DEFAULT 0.70\
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

    async fn body_of(resp: Response) -> (StatusCode, serde_json::Value) {
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), 65536).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        )
    }

    #[tokio::test]
    async fn get_without_row_returns_defaults() {
        let Some(pool) = make_pool("t_scam_guard_defaults").await else {
            return;
        };

        let (status, body) = body_of(
            get_handler(
                partner("NaNi"),
                State(pool),
                Query(ScamGuardQuery::default()),
            )
            .await,
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body,
            json!({
                "enabled": true,
                "mode": "auto_ban",
                "threshold": 0.90,
                "suggestion_floor": 0.70
            })
        );
    }

    #[tokio::test]
    async fn valid_post_upserts_and_get_returns_values() {
        let Some(pool) = make_pool("t_scam_guard_roundtrip").await else {
            return;
        };

        let update = ScamGuardUpdate {
            enabled: false,
            mode: "timeout".into(),
            threshold: 0.82,
            suggestion_floor: 0.61,
        };
        let (status, body) = body_of(
            post_handler(
                partner("NaNi"),
                State(pool.clone()),
                Query(ScamGuardQuery::default()),
                Json(update),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["ok"], true);
        assert_eq!(body["enabled"], false);
        assert_eq!(body["mode"], "timeout");
        assert!((body["threshold"].as_f64().unwrap() - 0.82).abs() < 0.000_001);
        assert!((body["suggestion_floor"].as_f64().unwrap() - 0.61).abs() < 0.000_001);

        let (status, body) = body_of(
            get_handler(
                partner("NANI"),
                State(pool),
                Query(ScamGuardQuery::default()),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["enabled"], false);
        assert_eq!(body["mode"], "timeout");
        assert!((body["threshold"].as_f64().unwrap() - 0.82).abs() < 0.000_001);
        assert!((body["suggestion_floor"].as_f64().unwrap() - 0.61).abs() < 0.000_001);
    }

    #[tokio::test]
    async fn post_rejects_invalid_mode() {
        let Some(pool) = make_pool("t_scam_guard_invalid_mode").await else {
            return;
        };

        let (status, body) = body_of(
            post_handler(
                partner("nani"),
                State(pool),
                Query(ScamGuardQuery::default()),
                Json(ScamGuardUpdate {
                    enabled: true,
                    mode: "block".into(),
                    threshold: 0.90,
                    suggestion_floor: 0.70,
                }),
            )
            .await,
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            body,
            json!({ "error": "invalid_mode", "message": "invalid mode" })
        );
    }

    #[tokio::test]
    async fn post_rejects_suggestion_floor_above_threshold() {
        let Some(pool) = make_pool("t_scam_guard_invalid_thresholds").await else {
            return;
        };

        let (status, body) = body_of(
            post_handler(
                partner("nani"),
                State(pool),
                Query(ScamGuardQuery::default()),
                Json(ScamGuardUpdate {
                    enabled: true,
                    mode: "alert_only".into(),
                    threshold: 0.70,
                    suggestion_floor: 0.71,
                }),
            )
            .await,
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            body,
            json!({ "error": "invalid_thresholds", "message": "invalid thresholds" })
        );
    }
}
