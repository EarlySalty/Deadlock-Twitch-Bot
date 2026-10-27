use axum::{extract::State, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tb_http_core::ApiError;

use crate::auth::level::DashboardAuthLevel;

#[derive(Serialize, sqlx::FromRow)]
pub struct GuildRaidSetting {
    guild_id: String,
    name: Option<String>,
    cross_server_raids: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Change {
    guild_id: String,
    cross_server_raids: bool,
    #[serde(default, rename = "csrf_token")]
    _csrf_token: Option<String>,
}

pub async fn list(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
) -> Result<impl IntoResponse, ApiError> {
    if let Some(error) = crate::auth::require_admin(&auth) {
        return Err(error);
    }
    let settings = sqlx::query_as::<_, GuildRaidSetting>(
        "SELECT guild_id::text, name, cross_server_raids FROM discord_guilds
         WHERE status='active' AND removed_at IS NULL ORDER BY name, guild_id",
    )
    .fetch_all(&pool)
    .await
    .map_err(|_| ApiError::internal())?;
    Ok((
        [(axum::http::header::CACHE_CONTROL, "no-store")],
        Json(settings),
    ))
}

pub async fn save(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Json(change): Json<Change>,
) -> Result<impl IntoResponse, ApiError> {
    if let Some(error) = crate::auth::require_admin(&auth) {
        return Err(error);
    }
    let id = change
        .guild_id
        .parse::<i64>()
        .ok()
        .filter(|id| *id > 0)
        .ok_or_else(|| ApiError::bad_request("Der Server ist ungültig."))?;
    let setting = sqlx::query_as::<_, GuildRaidSetting>(
        "UPDATE discord_guilds SET cross_server_raids=$2, updated_at=now(), updated_by='admin'
         WHERE guild_id=$1 AND status='active' AND removed_at IS NULL
         RETURNING guild_id::text, name, cross_server_raids",
    )
    .bind(id)
    .bind(change.cross_server_raids)
    .fetch_optional(&pool)
    .await
    .map_err(|_| ApiError::internal())?
    .ok_or_else(|| ApiError::bad_request("Der Server ist nicht aktiv."))?;
    Ok(Json(setting))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn switch_defaults_on_persists_and_rejects_non_admins() {
        let db = crate::guild_onboarding::tests::fixture().await;
        sqlx::query("INSERT INTO discord_guilds(guild_id,name,status) VALUES(1,'Test','active')")
            .execute(&db.pool)
            .await
            .unwrap();
        let initial: bool =
            sqlx::query_scalar("SELECT cross_server_raids FROM discord_guilds WHERE guild_id=1")
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert!(initial);
        assert!(list(DashboardAuthLevel::None, State(db.pool.clone()))
            .await
            .is_err());
        assert!(save(
            DashboardAuthLevel::None,
            State(db.pool.clone()),
            Json(Change {
                guild_id: "1".into(),
                cross_server_raids: false,
                _csrf_token: None,
            })
        )
        .await
        .is_err());
        save(
            DashboardAuthLevel::admin(),
            State(db.pool.clone()),
            Json(Change {
                guild_id: "1".into(),
                cross_server_raids: false,
                _csrf_token: None,
            }),
        )
        .await
        .unwrap();
        let saved: bool =
            sqlx::query_scalar("SELECT cross_server_raids FROM discord_guilds WHERE guild_id=1")
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert!(!saved);
        assert!(list(DashboardAuthLevel::admin(), State(db.pool.clone()))
            .await
            .is_ok());
    }
}
