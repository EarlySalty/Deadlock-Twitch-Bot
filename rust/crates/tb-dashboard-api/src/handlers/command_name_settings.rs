//! Kanalbezogene Chat-Command-Namen für Partner und Admins.
use crate::auth::level::DashboardAuthLevel;
use crate::auth::streamer_scope::resolve_settings_target as resolve_target;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{types::Json as DbJson, PgPool};
use std::collections::BTreeMap;
use tb_chat::catalog::catalog;
use tb_chat::command_names::{
    conflicting_command, effective_aliases, effective_name, entry_by_key, key,
    normalize_custom_name, CommandNameValidationError,
};

#[derive(Deserialize, Default)]
pub struct CommandNameQuery {
    pub streamer: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandNameUpdate {
    pub command: String,
    pub name: Option<String>,
}

#[derive(Serialize)]
struct CommandNameView {
    command: &'static str,
    default_name: &'static str,
    default_aliases: &'static [&'static str],
    custom_name: Option<String>,
    effective_name: String,
    effective_aliases: &'static [&'static str],
    group: &'static str,
    group_label: &'static str,
    summary: &'static str,
}

fn db_error(error: sqlx::Error) -> Response {
    tracing::error!(%error, "command-name-settings DB-Fehler");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"error":"db"})),
    )
        .into_response()
}

fn validation_error(error: CommandNameValidationError) -> Response {
    let message = match error {
        CommandNameValidationError::Empty => "Bitte einen Befehlsnamen eingeben.",
        CommandNameValidationError::TooLong => "Der Befehlsname ist zu lang.",
        CommandNameValidationError::InvalidCharacters => {
            "Erlaubt sind nur Buchstaben, Zahlen, _ und -."
        }
    };
    (
        StatusCode::BAD_REQUEST,
        Json(json!({"error":"invalid_name","message":message})),
    )
        .into_response()
}

fn conflict_error(candidate: &str, conflict: &tb_chat::catalog::CommandInfo) -> Response {
    (
        StatusCode::CONFLICT,
        Json(json!({
            "error":"name_conflict",
            "message": format!(
                "{} wird in deinem Kanal bereits für {} verwendet.",
                candidate,
                conflict.name
            ),
            "conflict_command": key(conflict)
        })),
    )
        .into_response()
}

fn views(overrides: &BTreeMap<String, String>) -> Vec<CommandNameView> {
    catalog()
        .iter()
        .map(|entry| {
            let custom_name = overrides
                .get(key(entry))
                .and_then(|value| normalize_custom_name(value).ok());
            CommandNameView {
                command: key(entry),
                default_name: entry.name,
                default_aliases: entry.aliases,
                custom_name,
                effective_name: effective_name(entry, overrides),
                effective_aliases: effective_aliases(entry, overrides),
                group: entry.group.as_str(),
                group_label: entry.group.label(),
                summary: entry.summary,
            }
        })
        .collect()
}

pub async fn get_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<CommandNameQuery>,
) -> Response {
    let (login, user_id) = match resolve_target(&auth, &query.streamer) {
        Ok(target) => target,
        Err(response) => return response,
    };
    let saved = sqlx::query_scalar::<_, DbJson<BTreeMap<String, String>>>(
        "SELECT command_name_overrides FROM streamer_plans
         WHERE ($2 <> '' AND twitch_user_id = $2)
            OR ($2 = '' AND LOWER(COALESCE(twitch_login, '')) = $1)
         LIMIT 1",
    )
    .bind(login)
    .bind(user_id)
    .fetch_optional(&pool)
    .await;

    match saved {
        Ok(saved) => {
            let overrides = saved.map(|value| value.0).unwrap_or_default();
            Json(json!({"commands": views(&overrides)})).into_response()
        }
        Err(error) => db_error(error),
    }
}

pub async fn post_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<CommandNameQuery>,
    body: Result<Json<CommandNameUpdate>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let (login, user_id) = match resolve_target(&auth, &query.streamer) {
        Ok(target) => target,
        Err(response) => return response,
    };
    let body = match body {
        Ok(Json(body)) => body,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error":"invalid command name settings"})),
            )
                .into_response()
        }
    };
    let Some(entry) = entry_by_key(&body.command) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"unknown_command"})),
        )
            .into_response();
    };
    let command_key = key(entry);

    let requested_name = match body.name.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(value) => match normalize_custom_name(value) {
            Ok(value) if value == entry.name => None,
            Ok(value) => Some(value),
            Err(error) => return validation_error(error),
        },
    };

    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(error) => return db_error(error),
    };

    if !user_id.is_empty() {
        if let Err(error) = sqlx::query(
            "INSERT INTO streamer_plans (twitch_user_id, twitch_login)
             VALUES ($1, $2)
             ON CONFLICT (twitch_user_id) DO UPDATE
             SET twitch_login = COALESCE(streamer_plans.twitch_login, EXCLUDED.twitch_login)",
        )
        .bind(&user_id)
        .bind(&login)
        .execute(&mut *tx)
        .await
        {
            return db_error(error);
        }
    }

    let saved = sqlx::query_scalar::<_, DbJson<BTreeMap<String, String>>>(
        "SELECT command_name_overrides FROM streamer_plans
         WHERE ($2 <> '' AND twitch_user_id = $2)
            OR ($2 = '' AND LOWER(COALESCE(twitch_login, '')) = $1)
         LIMIT 1
         FOR UPDATE",
    )
    .bind(&login)
    .bind(&user_id)
    .fetch_optional(&mut *tx)
    .await;

    let mut overrides = match saved {
        Ok(Some(saved)) => saved.0,
        Ok(None) => {
            return (StatusCode::NOT_FOUND, Json(json!({"error":"no plan row"}))).into_response()
        }
        Err(error) => return db_error(error),
    };

    if let Some(candidate) = requested_name.as_deref() {
        if let Some(conflict) = conflicting_command(command_key, candidate, &overrides) {
            return conflict_error(candidate, conflict);
        }
        overrides.insert(command_key.to_string(), candidate.to_string());
    } else {
        for candidate in std::iter::once(entry.name).chain(entry.aliases.iter().copied()) {
            if let Some(conflict) = conflicting_command(command_key, candidate, &overrides) {
                return conflict_error(candidate, conflict);
            }
        }
        overrides.remove(command_key);
    }

    let result = if !user_id.is_empty() {
        sqlx::query(
            "UPDATE streamer_plans
             SET command_name_overrides = $2
             WHERE twitch_user_id = $1",
        )
        .bind(&user_id)
        .bind(DbJson(overrides.clone()))
        .execute(&mut *tx)
        .await
    } else {
        sqlx::query(
            "UPDATE streamer_plans
             SET command_name_overrides = $2
             WHERE LOWER(COALESCE(twitch_login, '')) = $1",
        )
        .bind(&login)
        .bind(DbJson(overrides.clone()))
        .execute(&mut *tx)
        .await
    };

    match result {
        Ok(result) if result.rows_affected() > 0 => {
            if let Err(error) = tx.commit().await {
                return db_error(error);
            }
            Json(json!({
                "ok": true,
                "command": command_key,
                "custom_name": requested_name,
                "effective_name": effective_name(entry, &overrides),
                "effective_aliases": effective_aliases(entry, &overrides),
            }))
            .into_response()
        }
        Ok(_) => (StatusCode::NOT_FOUND, Json(json!({"error":"no plan row"}))).into_response(),
        Err(error) => db_error(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::level::DashboardAuthLevel;
    use axum::{body::Body, extract::FromRequest, http::Request};

    async fn database() -> crate::test_postgres::TestPostgres {
        let database = crate::test_postgres::TestPostgres::start().await;
        sqlx::query(
            "CREATE TABLE streamer_plans (
                twitch_user_id TEXT PRIMARY KEY,
                twitch_login TEXT
            )",
        )
        .execute(&database.pool)
        .await
        .unwrap();
        sqlx::raw_sql(include_str!(
            "../../../../migrations/20260925191500_add_command_name_overrides.sql"
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
        let bytes = axum::body::to_bytes(response.into_body(), 131072)
            .await
            .unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    #[tokio::test]
    async fn stat_command_settings_defaults_und_kanalbezogenes_raid_override() {
        let database = database().await;
        let pool = &database.pool;

        let (status, initial) = body(
            get_handler(
                partner("42"),
                State(pool.clone()),
                Query(CommandNameQuery::default()),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            initial["commands"].as_array().unwrap().len(),
            catalog().len()
        );

        let response = post_handler(
            partner("42"),
            State(pool.clone()),
            Query(CommandNameQuery::default()),
            Ok(Json(CommandNameUpdate {
                command: "raid".into(),
                name: Some("DACHRAID".into()),
            })),
        )
        .await;
        let (status, saved) = body(response).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(saved["custom_name"], "!dachraid");
        assert_eq!(saved["effective_aliases"], json!([]));

        let (status, current) = body(
            get_handler(
                partner("42"),
                State(pool.clone()),
                Query(CommandNameQuery::default()),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let raid = current["commands"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["command"] == "raid")
            .unwrap();
        assert_eq!(raid["effective_name"], "!dachraid");
        assert_eq!(raid["default_name"], "!raid");
        assert_eq!(raid["default_aliases"], json!(["!traid"]));
        assert_eq!(raid["effective_aliases"], json!([]));

        let reset = post_handler(
            partner("42"),
            State(pool.clone()),
            Query(CommandNameQuery::default()),
            Ok(Json(CommandNameUpdate {
                command: "raid".into(),
                name: None,
            })),
        )
        .await;
        let (_, reset) = body(reset).await;
        assert_eq!(reset["custom_name"], serde_json::Value::Null);
        assert_eq!(reset["effective_name"], "!raid");
        assert_eq!(reset["effective_aliases"], json!(["!traid"]));
    }

    #[tokio::test]
    async fn stat_command_settings_validierung_und_kollisionen() {
        let database = database().await;
        let pool = &database.pool;

        for name in ["!", "!raid bitte", "!räid"] {
            let response = post_handler(
                partner("42"),
                State(pool.clone()),
                Query(CommandNameQuery::default()),
                Ok(Json(CommandNameUpdate {
                    command: "raid".into(),
                    name: Some(name.into()),
                })),
            )
            .await;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        }

        let unknown = post_handler(
            partner("42"),
            State(pool.clone()),
            Query(CommandNameQuery::default()),
            Ok(Json(CommandNameUpdate {
                command: "gibt-es-nicht".into(),
                name: Some("!x".into()),
            })),
        )
        .await;
        assert_eq!(unknown.status(), StatusCode::BAD_REQUEST);

        let collision = post_handler(
            partner("42"),
            State(pool.clone()),
            Query(CommandNameQuery::default()),
            Ok(Json(CommandNameUpdate {
                command: "raid".into(),
                name: Some("!ping".into()),
            })),
        )
        .await;
        let (status, collision) = body(collision).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(collision["conflict_command"], "ping");
    }

    #[tokio::test]
    async fn stat_command_settings_reset_lehnt_kollision_mit_freiem_standardnamen_ab() {
        let database = database().await;
        let pool = &database.pool;

        for (command, name) in [("ping", "!pong"), ("raid", "!ping")] {
            assert_eq!(
                post_handler(
                    partner("42"),
                    State(pool.clone()),
                    Query(CommandNameQuery::default()),
                    Ok(Json(CommandNameUpdate {
                        command: command.into(),
                        name: Some(name.into()),
                    })),
                )
                .await
                .status(),
                StatusCode::OK
            );
        }

        let response = post_handler(
            partner("42"),
            State(pool.clone()),
            Query(CommandNameQuery::default()),
            Ok(Json(CommandNameUpdate {
                command: "ping".into(),
                name: None,
            })),
        )
        .await;
        let (status, payload) = body(response).await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(payload["conflict_command"], "raid");
    }

    #[tokio::test]
    async fn zwei_streamer_bleiben_getrennt_und_admin_darf_ziel_waehlen() {
        let database = database().await;
        let pool = &database.pool;
        sqlx::query(
            "INSERT INTO streamer_plans (twitch_user_id,twitch_login)
             VALUES ('42','nani'),('99','anderer')",
        )
        .execute(pool)
        .await
        .unwrap();

        assert_eq!(
            post_handler(
                partner("42"),
                State(pool.clone()),
                Query(CommandNameQuery::default()),
                Ok(Json(CommandNameUpdate {
                    command: "raid".into(),
                    name: Some("!meinraid".into()),
                })),
            )
            .await
            .status(),
            StatusCode::OK
        );
        let foreign: serde_json::Value = sqlx::query_scalar(
            "SELECT command_name_overrides FROM streamer_plans WHERE twitch_user_id='99'",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(foreign, json!({}));

        assert_eq!(
            post_handler(
                DashboardAuthLevel::admin(),
                State(pool.clone()),
                Query(CommandNameQuery {
                    streamer: Some("anderer".into()),
                }),
                Ok(Json(CommandNameUpdate {
                    command: "raid".into(),
                    name: Some("!andererraid".into()),
                })),
            )
            .await
            .status(),
            StatusCode::OK
        );
        let foreign: serde_json::Value = sqlx::query_scalar(
            "SELECT command_name_overrides FROM streamer_plans WHERE twitch_user_id='99'",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(foreign["raid"], "!andererraid");
    }

    #[tokio::test]
    async fn ungueltiges_json_gibt_400() {
        let database = database().await;
        let request = Request::builder()
            .header("content-type", "application/json")
            .body(Body::from(r#"{"command":"raid","name":"!x","extra":true}"#))
            .unwrap();
        let payload = Json::<CommandNameUpdate>::from_request(request, &()).await;
        assert_eq!(
            post_handler(
                partner("42"),
                State(database.pool),
                Query(CommandNameQuery::default()),
                payload,
            )
            .await
            .status(),
            StatusCode::BAD_REQUEST
        );
    }
}
