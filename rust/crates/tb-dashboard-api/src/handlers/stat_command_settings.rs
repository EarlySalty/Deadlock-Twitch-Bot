//! Je Statistikbefehl ein Schalter im bestehenden streamer_plans-Datensatz.
use crate::auth::level::DashboardAuthLevel;
use crate::auth::streamer_scope::resolve_settings_target as resolve_target;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::json;
use sqlx::{types::Json as DbJson, PgPool};
use std::collections::BTreeMap;
use tb_chat::stat_commands::StatCommand;

#[derive(Deserialize, Default)]
pub struct StatCommandQuery {
    pub streamer: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatCommandUpdate {
    pub command: StatCommand,
    pub enabled: bool,
}

fn db_error(error: sqlx::Error) -> Response {
    tracing::error!(%error, "stat-command-settings DB-Fehler");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"error":"db"})),
    )
        .into_response()
}

pub async fn get_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<StatCommandQuery>,
) -> Response {
    let (login, user_id) = match resolve_target(&auth, &query.streamer) {
        Ok(t) => t,
        Err(response) => return response,
    };
    let result = sqlx::query_scalar::<_, DbJson<BTreeMap<StatCommand, bool>>>(
        "SELECT stat_command_settings FROM streamer_plans WHERE ($2 <> '' AND twitch_user_id = $2) OR ($2 = '' AND LOWER(COALESCE(twitch_login, '')) = $1) LIMIT 1"
    ).bind(login).bind(user_id).fetch_optional(&pool).await;
    match result {
        Ok(saved) => {
            let mut settings = saved.map(|v| v.0).unwrap_or_default();
            for command in StatCommand::ALL {
                settings.entry(command).or_insert(true);
            }
            Json(json!({"commands": settings})).into_response()
        }
        Err(error) => db_error(error),
    }
}

pub async fn post_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<StatCommandQuery>,
    body: Result<Json<StatCommandUpdate>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let (login, user_id) = match resolve_target(&auth, &query.streamer) {
        Ok(t) => t,
        Err(response) => return response,
    };
    let body = match body {
        Ok(Json(body)) => body,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error":"invalid command settings"})),
            )
                .into_response()
        }
    };
    let key = body.command.key();
    let result = if !user_id.is_empty() {
        sqlx::query("INSERT INTO streamer_plans (twitch_user_id, twitch_login, stat_command_settings) VALUES ($1, $2, jsonb_build_object($3::text, $4::boolean)) ON CONFLICT (twitch_user_id) DO UPDATE SET stat_command_settings = jsonb_set(streamer_plans.stat_command_settings, ARRAY[$3::text], to_jsonb($4::boolean)), twitch_login = COALESCE(streamer_plans.twitch_login, EXCLUDED.twitch_login)")
            .bind(user_id).bind(login).bind(key).bind(body.enabled).execute(&pool).await
    } else {
        sqlx::query("UPDATE streamer_plans SET stat_command_settings = jsonb_set(stat_command_settings, ARRAY[$2::text], to_jsonb($3::boolean)) WHERE LOWER(COALESCE(twitch_login, '')) = $1")
            .bind(login).bind(key).bind(body.enabled).execute(&pool).await
    };
    match result {
        Ok(result) if result.rows_affected() > 0 => {
            Json(json!({"ok":true, "command":body.command, "enabled":body.enabled})).into_response()
        }
        Ok(_) => (StatusCode::NOT_FOUND, Json(json!({"error":"no plan row"}))).into_response(),
        Err(error) => db_error(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, extract::FromRequest, http::Request};

    async fn database() -> crate::test_postgres::TestPostgres {
        let database = crate::test_postgres::TestPostgres::start().await;
        sqlx::query(
            "CREATE TABLE streamer_plans (twitch_user_id TEXT PRIMARY KEY, twitch_login TEXT)",
        )
        .execute(&database.pool)
        .await
        .unwrap();
        sqlx::raw_sql(include_str!(
            "../../../../migrations/20260909210000_add_stat_command_settings.sql"
        ))
        .execute(&database.pool)
        .await
        .unwrap();
        database
    }

    fn partner(id: &str) -> DashboardAuthLevel {
        DashboardAuthLevel::Partner {
            twitch_login: "nani".into(),
            twitch_user_id: id.into(),
            display_name: String::new(),
        }
    }

    async fn body(response: Response) -> (StatusCode, serde_json::Value) {
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 65536)
            .await
            .unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    async fn settings(pool: &PgPool) -> serde_json::Value {
        let (status, value) = body(
            get_handler(
                partner("42"),
                State(pool.clone()),
                Query(StatCommandQuery::default()),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        value["commands"].clone()
    }

    #[tokio::test]
    async fn stat_settings_acht_defaults_parallele_updates_und_wieder_an() {
        let database = database().await;
        let pool = &database.pool;
        let defaults = settings(pool).await;
        assert_eq!(defaults.as_object().unwrap().len(), 8);
        for command in StatCommand::ALL {
            assert_eq!(defaults[command.key()], true);
        }
        let mut tasks = tokio::task::JoinSet::new();
        for command in StatCommand::ALL {
            let pool = pool.clone();
            tasks.spawn(async move {
                post_handler(
                    partner("42"),
                    State(pool),
                    Query(StatCommandQuery::default()),
                    Ok(Json(StatCommandUpdate {
                        command,
                        enabled: false,
                    })),
                )
                .await
                .status()
            });
        }
        while let Some(status) = tasks.join_next().await {
            assert_eq!(status.unwrap(), StatusCode::OK);
        }
        let disabled = settings(pool).await;
        for command in StatCommand::ALL {
            assert_eq!(disabled[command.key()], false);
        }
        for command in StatCommand::ALL {
            let response = post_handler(
                partner("42"),
                State(pool.clone()),
                Query(StatCommandQuery::default()),
                Ok(Json(StatCommandUpdate {
                    command,
                    enabled: true,
                })),
            )
            .await;
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(settings(pool).await[command.key()], true);
        }
    }

    #[tokio::test]
    async fn stat_settings_identitaet_und_admin_ziel() {
        let database = database().await;
        let pool = &database.pool;
        sqlx::query("INSERT INTO streamer_plans VALUES ('42', 'alter_name', '{\"rank\":false}'), ('99', 'nani', '{\"rank\":true}')").execute(pool).await.unwrap();
        assert_eq!(settings(pool).await["rank"], false);
        let query = || {
            Query(StatCommandQuery {
                streamer: Some("nani".into()),
            })
        };
        let response = post_handler(
            partner("42"),
            State(pool.clone()),
            query(),
            Ok(Json(StatCommandUpdate {
                command: StatCommand::Rank,
                enabled: false,
            })),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let foreign: bool = sqlx::query_scalar("SELECT (stat_command_settings->>'rank')::bool FROM streamer_plans WHERE twitch_user_id='99'").fetch_one(pool).await.unwrap();
        assert!(foreign);
        let response = post_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            query(),
            Ok(Json(StatCommandUpdate {
                command: StatCommand::Rank,
                enabled: false,
            })),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let foreign: bool = sqlx::query_scalar("SELECT (stat_command_settings->>'rank')::bool FROM streamer_plans WHERE twitch_user_id='99'").fetch_one(pool).await.unwrap();
        assert!(!foreign);
        for auth in [DashboardAuthLevel::None, partner("")] {
            assert_eq!(
                get_handler(auth.clone(), State(pool.clone()), query())
                    .await
                    .status(),
                StatusCode::UNAUTHORIZED
            );
            assert_eq!(
                post_handler(
                    auth,
                    State(pool.clone()),
                    query(),
                    Ok(Json(StatCommandUpdate {
                        command: StatCommand::Rank,
                        enabled: false
                    }))
                )
                .await
                .status(),
                StatusCode::UNAUTHORIZED
            );
        }
    }

    #[tokio::test]
    async fn stat_settings_ungueltige_payloads_400_und_db_fehler_500() {
        let database = database().await;
        let pool = &database.pool;
        for payload in [
            json!(null),
            json!({"command":"raid","enabled":false}),
            json!({"command":"rank","enabled":null}),
            json!({"command":"rank","enabled":"false"}),
            json!({"command":"rank","enabled":false,"extra":true}),
            json!({"enabled":false}),
        ] {
            let request = Request::builder()
                .header("content-type", "application/json")
                .body(Body::from(payload.to_string()))
                .unwrap();
            let payload = Json::<StatCommandUpdate>::from_request(request, &()).await;
            let response = post_handler(
                partner("42"),
                State(pool.clone()),
                Query(StatCommandQuery::default()),
                payload,
            )
            .await;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        }
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM streamer_plans")
                .fetch_one(pool)
                .await
                .unwrap(),
            0
        );
        sqlx::query("DROP TABLE streamer_plans")
            .execute(pool)
            .await
            .unwrap();
        assert_eq!(
            get_handler(
                partner("42"),
                State(pool.clone()),
                Query(StatCommandQuery::default())
            )
            .await
            .status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
        assert_eq!(
            post_handler(
                partner("42"),
                State(pool.clone()),
                Query(StatCommandQuery::default()),
                Ok(Json(StatCommandUpdate {
                    command: StatCommand::Rank,
                    enabled: true
                }))
            )
            .await
            .status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }
}
