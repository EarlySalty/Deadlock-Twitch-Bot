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
use crate::auth::resolve_streamer_scope;

#[derive(Deserialize, Default)]
pub struct ModerationQuery {
    #[serde(default)]
    pub streamer: Option<String>,
}

#[derive(Deserialize)]
pub struct ModerationUpdate {
    pub global_ban_enabled: bool,
    pub scam_pitch_enabled: bool,
    pub spam_autoban_enabled: bool,
    pub sus_invite_enabled: bool,
}

fn error_response(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(json!({ "error": code, "message": message }))).into_response()
}

async fn resolve_channel_user_id(
    auth: &DashboardAuthLevel,
    streamer: &Option<String>,
    pool: &PgPool,
) -> Result<String, Response> {
    let scope = resolve_streamer_scope(auth, streamer.as_deref(), false)?;
    match auth {
        DashboardAuthLevel::Partner { twitch_user_id, .. } => {
            let id = twitch_user_id.trim();
            if id.is_empty() {
                return Err(error_response(
                    StatusCode::BAD_REQUEST,
                    "streamer_required",
                    "streamer is required",
                ));
            }
            Ok(id.to_string())
        }
        DashboardAuthLevel::Admin { .. } => {
            let login = match scope {
                Some(login) => login,
                None => {
                    return Err(error_response(
                        StatusCode::BAD_REQUEST,
                        "streamer_required",
                        "streamer is required",
                    ))
                }
            };
            let row: Option<(Option<String>,)> = sqlx::query_as(
                "SELECT twitch_user_id FROM twitch_partners \
                  WHERE LOWER(twitch_login) = $1 AND twitch_user_id IS NOT NULL \
                  LIMIT 1",
            )
            .bind(&login)
            .fetch_optional(pool)
            .await
            .map_err(|error| {
                tracing::error!(%error, "moderation settings streamer lookup failed");
                error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "database_error",
                    "failed to resolve streamer",
                )
            })?;
            match row.and_then(|r| r.0).map(|id| id.trim().to_string()).filter(|id| !id.is_empty()) {
                Some(id) => Ok(id),
                None => Err(error_response(
                    StatusCode::NOT_FOUND,
                    "unknown_streamer",
                    "streamer not found",
                )),
            }
        }
        DashboardAuthLevel::None => Err(error_response(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "authentication required",
        )),
    }
}

pub async fn get_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<ModerationQuery>,
) -> Response {
    let channel_user_id = match resolve_channel_user_id(&auth, &query.streamer, &pool).await {
        Ok(id) => id,
        Err(response) => return response,
    };

    let row: Result<Option<(bool, bool, bool, bool)>, sqlx::Error> = sqlx::query_as(
        "SELECT global_ban_enabled, scam_pitch_enabled, spam_autoban_enabled, sus_invite_enabled \
           FROM twitch_moderation_settings \
          WHERE channel_user_id = $1",
    )
    .bind(&channel_user_id)
    .fetch_optional(&pool)
    .await;

    match row {
        Ok(Some((global_ban, scam_pitch, spam_autoban, sus_invite))) => Json(json!({
            "global_ban_enabled": global_ban,
            "scam_pitch_enabled": scam_pitch,
            "spam_autoban_enabled": spam_autoban,
            "sus_invite_enabled": sus_invite
        }))
        .into_response(),
        Ok(None) => Json(json!({
            "global_ban_enabled": true,
            "scam_pitch_enabled": true,
            "spam_autoban_enabled": true,
            "sus_invite_enabled": true
        }))
        .into_response(),
        Err(error) => {
            tracing::error!(%error, "moderation settings GET database error");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "failed to load moderation settings",
            )
        }
    }
}

pub async fn post_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<ModerationQuery>,
    Json(body): Json<ModerationUpdate>,
) -> Response {
    let channel_user_id = match resolve_channel_user_id(&auth, &query.streamer, &pool).await {
        Ok(id) => id,
        Err(response) => return response,
    };

    let result = sqlx::query(
        "INSERT INTO twitch_moderation_settings \
             (channel_user_id, global_ban_enabled, scam_pitch_enabled, spam_autoban_enabled, sus_invite_enabled) \
         VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT (channel_user_id) DO UPDATE SET \
             global_ban_enabled = EXCLUDED.global_ban_enabled, \
             scam_pitch_enabled = EXCLUDED.scam_pitch_enabled, \
             spam_autoban_enabled = EXCLUDED.spam_autoban_enabled, \
             sus_invite_enabled = EXCLUDED.sus_invite_enabled",
    )
    .bind(&channel_user_id)
    .bind(body.global_ban_enabled)
    .bind(body.scam_pitch_enabled)
    .bind(body.spam_autoban_enabled)
    .bind(body.sus_invite_enabled)
    .execute(&pool)
    .await;

    match result {
        Ok(_) => Json(json!({
            "ok": true,
            "global_ban_enabled": body.global_ban_enabled,
            "scam_pitch_enabled": body.scam_pitch_enabled,
            "spam_autoban_enabled": body.spam_autoban_enabled,
            "sus_invite_enabled": body.sus_invite_enabled
        }))
        .into_response(),
        Err(error) => {
            tracing::error!(%error, "moderation settings POST database error");
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "database_error",
                "failed to save moderation settings",
            )
        }
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
        sqlx::query(
            "CREATE TABLE twitch_moderation_settings (\
                channel_user_id TEXT PRIMARY KEY,\
                global_ban_enabled BOOLEAN NOT NULL DEFAULT TRUE,\
                scam_pitch_enabled BOOLEAN NOT NULL DEFAULT TRUE,\
                spam_autoban_enabled BOOLEAN NOT NULL DEFAULT TRUE,\
                sus_invite_enabled BOOLEAN NOT NULL DEFAULT TRUE\
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("CREATE TABLE twitch_partners (twitch_login TEXT, twitch_user_id TEXT)")
            .execute(&pool)
            .await
            .unwrap();
        Some(pool)
    }

    fn partner(user_id: &str) -> DashboardAuthLevel {
        DashboardAuthLevel::Partner {
            twitch_login: "nani".into(),
            twitch_user_id: user_id.into(),
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
        let Some(pool) = make_pool("t_moderation_defaults").await else {
            return;
        };

        let (status, body) = body_of(
            get_handler(
                partner("100"),
                State(pool),
                Query(ModerationQuery::default()),
            )
            .await,
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body,
            json!({
                "global_ban_enabled": true,
                "scam_pitch_enabled": true,
                "spam_autoban_enabled": true,
                "sus_invite_enabled": true
            })
        );
    }

    #[tokio::test]
    async fn post_upserts_and_get_returns_values() {
        let Some(pool) = make_pool("t_moderation_roundtrip").await else {
            return;
        };

        let update = ModerationUpdate {
            global_ban_enabled: true,
            scam_pitch_enabled: false,
            spam_autoban_enabled: false,
            sus_invite_enabled: true,
        };
        let (status, body) = body_of(
            post_handler(
                partner("100"),
                State(pool.clone()),
                Query(ModerationQuery::default()),
                Json(update),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["ok"], true);
        assert_eq!(body["scam_pitch_enabled"], false);
        assert_eq!(body["spam_autoban_enabled"], false);

        let (status, body) = body_of(
            get_handler(
                partner("100"),
                State(pool),
                Query(ModerationQuery::default()),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["global_ban_enabled"], true);
        assert_eq!(body["scam_pitch_enabled"], false);
        assert_eq!(body["spam_autoban_enabled"], false);
        assert_eq!(body["sus_invite_enabled"], true);
    }

    #[tokio::test]
    async fn admin_resolves_streamer_login_to_user_id() {
        let Some(pool) = make_pool("t_moderation_admin").await else {
            return;
        };
        sqlx::query("INSERT INTO twitch_partners (twitch_login, twitch_user_id) VALUES ('nani', '555')")
            .execute(&pool)
            .await
            .unwrap();

        let (status, _) = body_of(
            post_handler(
                DashboardAuthLevel::admin(),
                State(pool.clone()),
                Query(ModerationQuery {
                    streamer: Some("NaNi".into()),
                }),
                Json(ModerationUpdate {
                    global_ban_enabled: false,
                    scam_pitch_enabled: true,
                    spam_autoban_enabled: true,
                    sus_invite_enabled: true,
                }),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK);

        let stored: (bool,) = sqlx::query_as(
            "SELECT global_ban_enabled FROM twitch_moderation_settings WHERE channel_user_id = '555'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(!stored.0);
    }

    #[tokio::test]
    async fn partner_fremder_streamer_ist_forbidden() {
        let Some(pool) = make_pool("t_moderation_fremd").await else {
            return;
        };
        let (get_status, _) = body_of(
            get_handler(
                partner("100"),
                State(pool.clone()),
                Query(ModerationQuery {
                    streamer: Some("fremderkanal".into()),
                }),
            )
            .await,
        )
        .await;
        assert_eq!(get_status, StatusCode::FORBIDDEN);

        let (post_status, _) = body_of(
            post_handler(
                partner("100"),
                State(pool),
                Query(ModerationQuery {
                    streamer: Some("fremderkanal".into()),
                }),
                Json(ModerationUpdate {
                    global_ban_enabled: false,
                    scam_pitch_enabled: false,
                    spam_autoban_enabled: false,
                    sus_invite_enabled: false,
                }),
            )
            .await,
        )
        .await;
        assert_eq!(post_status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn admin_without_streamer_is_bad_request() {
        let Some(pool) = make_pool("t_moderation_admin_missing").await else {
            return;
        };
        let (status, body) = body_of(
            get_handler(
                DashboardAuthLevel::admin(),
                State(pool),
                Query(ModerationQuery::default()),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"], "streamer_required");
    }
}
