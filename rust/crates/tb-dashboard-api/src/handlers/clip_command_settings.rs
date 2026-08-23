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

#[derive(Deserialize, Default)]
pub struct ClipCommandQuery {
    /// Nur fuer Admin/Localhost relevant; Partner nutzen ihren Session-Login.
    #[serde(default)]
    pub streamer: Option<String>,
}

#[derive(Deserialize)]
pub struct ClipCommandUpdate {
    pub clip_command_enabled: bool,
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

const SELECT_SQL: &str = "SELECT COALESCE(clip_command_enabled, 1) AS enabled \
       FROM streamer_plans \
      WHERE LOWER(COALESCE(twitch_login, '')) = $1 \
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
            let enabled: i32 = row.try_get("enabled").unwrap_or(1);
            Json(json!({
                "clip_command_enabled": enabled != 0,
                "kontingent": kontingent.als_json(),
            }))
            .into_response()
        }
        Ok(None) => Json(json!({
            "clip_command_enabled": true,
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
    let flag = i32::from(body.clip_command_enabled);

    let result = if !user_id.is_empty() {
        sqlx::query(
            "INSERT INTO streamer_plans (twitch_user_id, twitch_login, clip_command_enabled) \
             VALUES ($1, $2, $3) \
             ON CONFLICT (twitch_user_id) \
             DO UPDATE SET clip_command_enabled = EXCLUDED.clip_command_enabled, \
                           twitch_login = COALESCE(streamer_plans.twitch_login, EXCLUDED.twitch_login)",
        )
        .bind(&user_id)
        .bind(&login)
        .bind(flag)
        .execute(&pool)
        .await
    } else {
        sqlx::query(
            "UPDATE streamer_plans SET clip_command_enabled = $2 \
              WHERE LOWER(COALESCE(twitch_login, '')) = $1",
        )
        .bind(&login)
        .bind(flag)
        .execute(&pool)
        .await
    };

    match result {
        Ok(res) if res.rows_affected() > 0 => {
            let stufe = crate::auth::stufe_fuer_auth(&pool, &auth).await;
            let kontingent = tb_analytics::stufe::clip_kontingent(&pool, stufe, &login).await;
            Json(json!({
                "ok": true,
                "clip_command_enabled": body.clip_command_enabled,
                "kontingent": kontingent.als_json(),
            }))
            .into_response()
        }
        Ok(_) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "no plan row" })),
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
             plan_name TEXT DEFAULT 'free' NOT NULL, clip_command_enabled INTEGER DEFAULT 1 NOT NULL)",
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
                    clip_command_enabled: false,
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
                    clip_command_enabled: false,
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
