//! GET/POST `/twitch/api/v2/streamer/clip-command-settings`.
//!
//! Streamer-Selbstbedienung fuer den `!clip`-Command auf
//! `streamer_plans.clip_command_enabled` (INTEGER, Default 1).
//!
//! Plan-Grenze: der Schalter selbst bleibt bewusst ohne 403. Chat-Befehle sind
//! laut Katalog eine Free-Funktion; die Stufe entscheidet ueber die **Menge**
//! der Clips, nicht ueber den Befehl. Die Antwort traegt deshalb das
//! Monatskontingent mit, damit das Dashboard die echte Grenze anzeigen kann.

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
use crate::auth::streamer_scope::resolve_settings_target as resolve_target;

#[derive(Deserialize, Default)]
pub struct ClipCommandQuery {
    /// Nur fuer Admin/Localhost relevant; Partner nutzen ihren Session-Login.
    #[serde(default)]
    pub streamer: Option<String>,
}

#[derive(Deserialize)]
pub struct ClipCommandUpdate {
    pub clip_command_enabled: Option<bool>,
    pub clip_command_duration_seconds: Option<f64>,
}

const SELECT_SQL: &str = "SELECT COALESCE(clip_command_enabled, 1) AS enabled, \
       clip_command_duration_seconds \
       FROM streamer_plans \
      WHERE ($2 = '' AND LOWER(COALESCE(twitch_login, '')) = $1) \
         OR ($2 <> '' AND twitch_user_id = $2) \
      LIMIT 1";

pub async fn get_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<ClipCommandQuery>,
) -> Response {
    let (login, user_id) = match resolve_target(&auth, &query.streamer) {
        Ok(t) => t,
        Err(resp) => return resp,
    };
    // Kontingent der Stufe: Free 3 Clips im Monat, Plus 10, Pro unbegrenzt.
    // Hier steht nur der Zaehlerstand, damit die Einstellungsseite ehrlich ist.
    //
    // Gezaehlt wird, was der Streamer selbst in unsere DB holt: eigener Upload
    // und eigener Clip-Fetch im Dashboard, beide ueber `clip_kontingent_guard`
    // in `social_media.rs`. Der Hintergrund-Fetcher
    // (`tb_social_media::ClipFetchTask`) und der `!clip`-Chat-Befehl, den auch
    // Zuschauer ausloesen, buchen nichts und werden auch nicht gesperrt. Das
    // Kontingent ist eine Grenze fuer die Clip-Werkzeuge, keine Strafe dafuer,
    // dass andere Leute den Kanal clippen.
    let stufe = crate::auth::stufe_fuer_auth(&pool, &auth).await;
    let kontingent = tb_analytics::stufe::clip_kontingent(&pool, stufe, &login).await;

    match sqlx::query(SELECT_SQL)
        .bind(&login)
        .bind(&user_id)
        .fetch_optional(&pool)
        .await
    {
        Ok(Some(row)) => {
            let enabled: i32 = row.get("enabled");
            let duration: Option<f64> = row.get("clip_command_duration_seconds");
            Json(json!({
                "clip_command_enabled": enabled != 0,
                "clip_command_duration_seconds": duration.unwrap_or(tb_domain::clip::DEFAULT_DURATION_SECONDS),
                "clip_command_duration_configured": duration.is_some(),
                "kontingent": kontingent.als_json(),
            }))
            .into_response()
        }
        Ok(None) => Json(json!({
            "clip_command_enabled": true,
            "clip_command_duration_seconds": tb_domain::clip::DEFAULT_DURATION_SECONDS,
            "clip_command_duration_configured": false,
            "kontingent": kontingent.als_json(),
        }))
        .into_response(),
        Err(error) => {
            tracing::error!(%error, "clip-command-settings GET DB-Fehler");
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
    Query(query): Query<ClipCommandQuery>,
    Json(body): Json<ClipCommandUpdate>,
) -> Response {
    let (login, user_id) = match resolve_target(&auth, &query.streamer) {
        Ok(t) => t,
        Err(resp) => return resp,
    };
    if body.clip_command_enabled.is_none() && body.clip_command_duration_seconds.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "Bitte eine Einstellung angeben."})),
        )
            .into_response();
    }
    if body
        .clip_command_duration_seconds
        .is_some_and(|value| !tb_domain::clip::valid_duration(value))
    {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": "Die Clip-Dauer muss zwischen 5 und 60 Sekunden liegen, in Schritten von 0,1 Sekunden."}))).into_response();
    }
    let flag = body.clip_command_enabled.map(i32::from);

    let result = if !user_id.is_empty() {
        sqlx::query_as::<_, (i32, Option<f64>)>(
            "INSERT INTO streamer_plans (twitch_user_id, twitch_login, clip_command_enabled, clip_command_duration_seconds) \
             VALUES ($1, $2, COALESCE($3, 1), $4) \
             ON CONFLICT (twitch_user_id) \
             DO UPDATE SET clip_command_enabled = COALESCE($3, streamer_plans.clip_command_enabled), \
                           clip_command_duration_seconds = COALESCE($4, streamer_plans.clip_command_duration_seconds), \
                           twitch_login = COALESCE(streamer_plans.twitch_login, EXCLUDED.twitch_login) \
             RETURNING COALESCE(clip_command_enabled, 1), clip_command_duration_seconds",
        )
        .bind(&user_id)
        .bind(&login)
        .bind(flag)
        .bind(body.clip_command_duration_seconds)
        .fetch_optional(&pool)
        .await
    } else {
        sqlx::query_as::<_, (i32, Option<f64>)>(
            "UPDATE streamer_plans SET clip_command_enabled = COALESCE($2, clip_command_enabled), \
             clip_command_duration_seconds = COALESCE($3, clip_command_duration_seconds) \
             WHERE LOWER(COALESCE(twitch_login, '')) = $1 \
             RETURNING COALESCE(clip_command_enabled, 1), clip_command_duration_seconds",
        )
        .bind(&login)
        .bind(flag)
        .bind(body.clip_command_duration_seconds)
        .fetch_optional(&pool)
        .await
    };

    match result {
        Ok(Some((enabled, duration))) => {
            let stufe = crate::auth::stufe_fuer_auth(&pool, &auth).await;
            let kontingent = tb_analytics::stufe::clip_kontingent(&pool, stufe, &login).await;
            Json(json!({
                "ok": true,
                "clip_command_enabled": enabled != 0,
                "clip_command_duration_seconds": duration.unwrap_or(tb_domain::clip::DEFAULT_DURATION_SECONDS),
                "clip_command_duration_configured": duration.is_some(),
                "kontingent": kontingent.als_json(),
            }))
            .into_response()
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Keine Kanaleinstellung gefunden." })),
        )
            .into_response(),
        Err(error) => {
            tracing::error!(%error, "clip-command-settings POST DB-Fehler");
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
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DROP SCHEMA IF EXISTS {schema} CASCADE"
        )))
        .execute(&admin)
        .await
        .unwrap();
        sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
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
             plan_name TEXT DEFAULT 'free' NOT NULL, clip_command_enabled INTEGER DEFAULT 1 NOT NULL, \
             clip_command_duration_seconds DOUBLE PRECISION)",
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
        let Some(pool) = make_pool("t_clipcmd_partner").await else {
            return;
        };

        let (s, j) = body_of(
            get_handler(
                partner("nani", "42"),
                State(pool.clone()),
                Query(ClipCommandQuery::default()),
            )
            .await,
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(j["clip_command_enabled"], true);

        let (s, j) = body_of(
            post_handler(
                partner("nani", "42"),
                State(pool.clone()),
                Query(ClipCommandQuery::default()),
                Json(ClipCommandUpdate {
                    clip_command_enabled: Some(false),
                    clip_command_duration_seconds: None,
                }),
            )
            .await,
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(j["clip_command_enabled"], false);

        let (_s, j) = body_of(
            get_handler(
                partner("nani", "42"),
                State(pool),
                Query(ClipCommandQuery::default()),
            )
            .await,
        )
        .await;
        assert_eq!(j["clip_command_enabled"], false);
    }

    #[tokio::test]
    async fn admin_toggle_roundtrip_per_streamer_query() {
        let Some(pool) = make_pool("t_clipcmd_admin").await else {
            return;
        };
        sqlx::query(
            "INSERT INTO streamer_plans (twitch_user_id, twitch_login, clip_command_enabled) \
             VALUES ('99', 'target', 1)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let query = ClipCommandQuery {
            streamer: Some("target".to_string()),
        };
        let (s, j) = body_of(
            post_handler(
                DashboardAuthLevel::admin(),
                State(pool.clone()),
                Query(query),
                Json(ClipCommandUpdate {
                    clip_command_enabled: Some(false),
                    clip_command_duration_seconds: None,
                }),
            )
            .await,
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(j["clip_command_enabled"], false);

        let (s, j) = body_of(
            get_handler(
                DashboardAuthLevel::admin(),
                State(pool),
                Query(ClipCommandQuery {
                    streamer: Some("target".to_string()),
                }),
            )
            .await,
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(j["clip_command_enabled"], false);
    }

    #[tokio::test]
    async fn duration_roundtrip_preserves_explicit_choice_and_channel_scope() {
        let Some(pool) = make_pool("t_clipcmd_duration").await else {
            return;
        };
        let (_, defaults) = body_of(
            get_handler(
                partner("nani", "42"),
                State(pool.clone()),
                Query(ClipCommandQuery::default()),
            )
            .await,
        )
        .await;
        assert_eq!(defaults["clip_command_duration_seconds"], 60.0);
        assert_eq!(defaults["clip_command_duration_configured"], false);
        for duration in [Some(30.0), None, Some(59.9)] {
            let (status, body) = body_of(
                post_handler(
                    partner("nani", "42"),
                    State(pool.clone()),
                    Query(ClipCommandQuery {
                        streamer: Some("foreign".into()),
                    }),
                    Json(ClipCommandUpdate {
                        clip_command_enabled: if duration.is_none() {
                            Some(false)
                        } else {
                            None
                        },
                        clip_command_duration_seconds: duration,
                    }),
                )
                .await,
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(
                body["clip_command_duration_seconds"],
                duration.unwrap_or(30.0)
            );
            assert_eq!(body["clip_command_duration_configured"], true);
        }
        let (_, reloaded) = body_of(
            get_handler(
                partner("nani", "42"),
                State(pool.clone()),
                Query(ClipCommandQuery::default()),
            )
            .await,
        )
        .await;
        assert_eq!(reloaded["clip_command_duration_seconds"], 59.9);
        assert_eq!(reloaded["clip_command_enabled"], false);
        let (_, foreign) = body_of(
            get_handler(
                partner("foreign", "99"),
                State(pool),
                Query(ClipCommandQuery::default()),
            )
            .await,
        )
        .await;
        assert_eq!(foreign["clip_command_duration_seconds"], 60.0);
        assert_eq!(foreign["clip_command_duration_configured"], false);
    }

    #[tokio::test]
    async fn invalid_duration_does_not_write() {
        let Some(pool) = make_pool("t_clipcmd_invalid").await else {
            return;
        };
        for duration in [4.9, 60.1, 30.01, f64::NAN, f64::INFINITY] {
            let (status, _) = body_of(
                post_handler(
                    partner("nani", "42"),
                    State(pool.clone()),
                    Query(ClipCommandQuery::default()),
                    Json(ClipCommandUpdate {
                        clip_command_enabled: Some(false),
                        clip_command_duration_seconds: Some(duration),
                    }),
                )
                .await,
            )
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST);
        }
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM streamer_plans")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn admin_ohne_streamer_ist_bad_request() {
        let Some(pool) = make_pool("t_clipcmd_admin_missing").await else {
            return;
        };
        let (s, _) = body_of(
            get_handler(
                DashboardAuthLevel::admin(),
                State(pool),
                Query(ClipCommandQuery::default()),
            )
            .await,
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn unauthorized_returns_401() {
        let Some(pool) = make_pool("t_clipcmd_auth").await else {
            return;
        };
        let (s, _) = body_of(
            get_handler(
                DashboardAuthLevel::None,
                State(pool),
                Query(ClipCommandQuery::default()),
            )
            .await,
        )
        .await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
    }
}
