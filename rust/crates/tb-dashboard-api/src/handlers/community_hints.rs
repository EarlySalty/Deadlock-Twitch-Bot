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
pub struct CommunityHintsQuery {
    #[serde(default)]
    pub streamer: Option<String>,
}

#[derive(Deserialize)]
pub struct CommunityHintsUpdate {
    pub enabled: bool,
}

const SELECT_SQL: &str = "SELECT COALESCE(promo_disabled, 0) AS promo_disabled \
       FROM streamer_plans \
      WHERE ($2 = '' AND LOWER(COALESCE(twitch_login, '')) = $1) \
         OR ($2 <> '' AND twitch_user_id = $2) \
      LIMIT 1";

async fn darf_ausschalten(pool: &PgPool, login: &str, user_id: &str) -> bool {
    match tb_analytics::plan::resolve_plan_snapshot(pool, login, user_id).await {
        Ok(snapshot) => snapshot.entitlements.contains(&"chat.promos.disable"),
        Err(error) => {
            tracing::warn!(%error, login, "Community-Hinweise: Plan nicht auflösbar");
            false
        }
    }
}

fn antwort(enabled: bool, can_disable: bool) -> Response {
    Json(json!({
        "enabled": enabled,
        "can_disable": can_disable,
    }))
    .into_response()
}

pub async fn get_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<CommunityHintsQuery>,
) -> Response {
    let (login, user_id) = match resolve_target(&auth, &query.streamer) {
        Ok(t) => t,
        Err(resp) => return resp,
    };
    let can_disable = darf_ausschalten(&pool, &login, &user_id).await;
    match sqlx::query(SELECT_SQL)
        .bind(&login)
        .bind(&user_id)
        .fetch_optional(&pool)
        .await
    {
        Ok(Some(row)) => {
            let promo_disabled: i32 = row.try_get("promo_disabled").unwrap_or(0);
            antwort(promo_disabled == 0, can_disable)
        }
        Ok(None) => antwort(true, can_disable),
        Err(error) => {
            tracing::error!(%error, "community-hints GET DB-Fehler");
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
    Query(query): Query<CommunityHintsQuery>,
    Json(body): Json<CommunityHintsUpdate>,
) -> Response {
    let (login, user_id) = match resolve_target(&auth, &query.streamer) {
        Ok(t) => t,
        Err(resp) => return resp,
    };
    let can_disable = darf_ausschalten(&pool, &login, &user_id).await;
    if !body.enabled && !can_disable {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "plan_required",
                "required_entitlements": ["chat.promos.disable"],
            })),
        )
            .into_response();
    }
    let promo_disabled = i32::from(!body.enabled);
    let result = if !user_id.is_empty() {
        sqlx::query(
            "INSERT INTO streamer_plans (twitch_user_id, twitch_login, promo_disabled) \
             VALUES ($1, $2, $3) \
             ON CONFLICT (twitch_user_id) \
             DO UPDATE SET promo_disabled = EXCLUDED.promo_disabled, \
                           twitch_login = COALESCE(streamer_plans.twitch_login, EXCLUDED.twitch_login)",
        )
        .bind(&user_id)
        .bind(&login)
        .bind(promo_disabled)
        .execute(&pool)
        .await
    } else {
        sqlx::query(
            "UPDATE streamer_plans SET promo_disabled = $2 \
              WHERE LOWER(COALESCE(twitch_login, '')) = $1",
        )
        .bind(&login)
        .bind(promo_disabled)
        .execute(&pool)
        .await
    };
    match result {
        Ok(res) if res.rows_affected() > 0 => Json(json!({
            "ok": true,
            "enabled": body.enabled,
            "can_disable": can_disable,
        }))
        .into_response(),
        Ok(_) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "no plan row" })),
        )
            .into_response(),
        Err(error) => {
            tracing::error!(%error, "community-hints POST DB-Fehler");
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
            "CREATE TABLE streamer_plans ( \
                twitch_user_id TEXT PRIMARY KEY, \
                twitch_login TEXT, \
                plan_name TEXT DEFAULT 'free' NOT NULL, \
                promo_disabled INTEGER DEFAULT 0 NOT NULL, \
                manual_plan_id TEXT, \
                manual_plan_expires_at TEXT, \
                manual_plan_notes TEXT DEFAULT '' NOT NULL, \
                manual_plan_updated_at TEXT, \
                trial_ever_granted INTEGER DEFAULT 0 NOT NULL, \
                first_login_at TEXT \
             )",
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
    async fn aus_nur_mit_entitlement() {
        let Some(pool) = make_pool("t_community_hints").await else {
            return;
        };
        sqlx::query(
            "INSERT INTO streamer_plans (twitch_user_id, twitch_login, manual_plan_id, promo_disabled) \
             VALUES ('42', 'frei', 'free', 0), ('43', 'pluskanal', 'plus', 0)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let (status, body) = body_of(
            post_handler(
                partner("frei", "42"),
                State(pool.clone()),
                Query(CommunityHintsQuery::default()),
                Json(CommunityHintsUpdate { enabled: false }),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["required_entitlements"][0], "chat.promos.disable");

        let (status, body) = body_of(
            post_handler(
                partner("pluskanal", "43"),
                State(pool.clone()),
                Query(CommunityHintsQuery::default()),
                Json(CommunityHintsUpdate { enabled: false }),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["enabled"], false);

        let flag: i32 = sqlx::query_scalar(
            "SELECT promo_disabled FROM streamer_plans WHERE twitch_user_id = '43'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(flag, 1);

        let (status, body) = body_of(
            get_handler(
                partner("frei", "42"),
                State(pool),
                Query(CommunityHintsQuery::default()),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["enabled"], true);
        assert_eq!(body["can_disable"], false);
    }
}
