//! Freiwillige Abo-Erinnerungen im eigenen Twitch-Kanal.
use crate::auth::level::DashboardAuthLevel;
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Update {
    pub enabled: bool,
}

fn identity(auth: &DashboardAuthLevel) -> Option<(&str, &str)> {
    match auth {
        DashboardAuthLevel::Partner {
            twitch_user_id,
            twitch_login,
            ..
        } if !twitch_user_id.trim().is_empty() => Some((twitch_user_id, twitch_login)),
        DashboardAuthLevel::Admin { actor: Some(actor) }
            if !actor.twitch_user_id.trim().is_empty() =>
        {
            Some((&actor.twitch_user_id, &actor.twitch_login))
        }
        _ => None,
    }
}

async fn available(pool: &PgPool, id: &str) -> Result<bool, sqlx::Error> {
    let scopes: Option<String> = sqlx::query_scalar("SELECT scopes FROM twitch_raid_auth WHERE twitch_user_id=$1 AND NOT COALESCE(needs_reauth,false)")
        .bind(id).fetch_optional(pool).await?.flatten();
    Ok(scopes
        .unwrap_or_default()
        .split_whitespace()
        .any(|scope| scope == tb_chat::sub_reminder::SUB_SCOPE))
}

fn error(error: sqlx::Error) -> Response {
    tracing::warn!(%error, "Sub-Erinnerungseinstellung nicht verfügbar");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"error":"Einstellung gerade nicht verfügbar."})),
    )
        .into_response()
}

pub async fn get_handler(auth: DashboardAuthLevel, State(pool): State<PgPool>) -> Response {
    let Some((id, _)) = identity(&auth) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let result: Result<_, sqlx::Error> = async {
        let enabled = sqlx::query_scalar::<_, i32>(
            "SELECT sub_reminder_enabled FROM streamer_plans WHERE twitch_user_id=$1",
        )
        .bind(id)
        .fetch_optional(&pool)
        .await?
            == Some(1);
        Ok(json!({"enabled":enabled,"available":available(&pool,id).await?}))
    }
    .await;
    match result {
        Ok(value) => Json(value).into_response(),
        Err(err) => error(err),
    }
}

pub async fn post_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Json(body): Json<Update>,
) -> Response {
    let Some((id, login)) = identity(&auth) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let ready = match available(&pool, id).await {
        Ok(ready) => ready,
        Err(err) => return error(err),
    };
    if body.enabled && !ready {
        return (StatusCode::CONFLICT, Json(json!({"error":"Bitte Twitch zuerst in der Verwaltung neu verbinden, damit der Bot deine Abos lesen darf."}))).into_response();
    }
    let result: Result<(),sqlx::Error> = async {
        let mut tx=pool.begin().await?;
        // Gleiche Sperrreihenfolge wie Versand: Kanal, dann Zuschauerzustimmung.
        sqlx::query("INSERT INTO streamer_plans (twitch_user_id,twitch_login,sub_reminder_enabled,sub_reminder_enabled_at) VALUES ($1,$2,$3,clock_timestamp()) ON CONFLICT (twitch_user_id) DO UPDATE SET sub_reminder_enabled=EXCLUDED.sub_reminder_enabled, sub_reminder_enabled_at=CASE WHEN streamer_plans.sub_reminder_enabled=EXCLUDED.sub_reminder_enabled THEN streamer_plans.sub_reminder_enabled_at ELSE clock_timestamp() END")
            .bind(id).bind(login).bind(i32::from(body.enabled)).execute(&mut *tx).await?;
        if !body.enabled {
            sqlx::query("UPDATE twitch_sub_reminders SET ended_at=NULL,end_message_id=NULL WHERE broadcaster_user_id=$1")
                .bind(id).execute(&mut *tx).await?;
        }
        tx.commit().await
    }.await;
    match result {
        Ok(()) => Json(json!({"enabled":body.enabled,"available":ready})).into_response(),
        Err(err) => error(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn partner(id: &str) -> DashboardAuthLevel {
        DashboardAuthLevel::Partner {
            twitch_user_id: id.into(),
            twitch_login: "renamed".into(),
            display_name: String::new(),
        }
    }
    #[tokio::test]
    async fn sub_reminder_settings_default_scope_gate_identitaet_und_aus_an_grenze() {
        let db = crate::test_postgres::TestPostgres::start().await;
        sqlx::raw_sql("CREATE TABLE streamer_plans(twitch_user_id text PRIMARY KEY,twitch_login text); CREATE TABLE twitch_subscription_events(id bigint); CREATE TABLE twitch_raid_auth(twitch_user_id text PRIMARY KEY,scopes text,needs_reauth boolean);")
            .execute(&db.pool).await.unwrap();
        sqlx::raw_sql(include_str!(
            "../../../../migrations/20260909220000_sub_reminders.sql"
        ))
        .execute(&db.pool)
        .await
        .unwrap();
        let response = get_handler(partner("42"), State(db.pool.clone())).await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 65536)
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value, json!({"enabled":false,"available":false}));
        assert_eq!(
            post_handler(
                partner("42"),
                State(db.pool.clone()),
                Json(Update { enabled: true })
            )
            .await
            .status(),
            StatusCode::CONFLICT
        );
        assert_eq!(
            post_handler(
                partner(""),
                State(db.pool.clone()),
                Json(Update { enabled: true })
            )
            .await
            .status(),
            StatusCode::UNAUTHORIZED
        );
        sqlx::query("INSERT INTO twitch_raid_auth VALUES('42','channel:read:subscriptions',false)")
            .execute(&db.pool)
            .await
            .unwrap();
        assert_eq!(
            post_handler(
                partner("42"),
                State(db.pool.clone()),
                Json(Update { enabled: true })
            )
            .await
            .status(),
            StatusCode::OK
        );
        sqlx::query("INSERT INTO twitch_sub_reminders(broadcaster_user_id,viewer_user_id,enabled,end_message_id,ended_at) VALUES('42','viewer',true,'old',now())").execute(&db.pool).await.unwrap();
        assert_eq!(
            post_handler(
                partner("42"),
                State(db.pool.clone()),
                Json(Update { enabled: false })
            )
            .await
            .status(),
            StatusCode::OK
        );
        let end: Option<String> =
            sqlx::query_scalar("SELECT end_message_id FROM twitch_sub_reminders")
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert_eq!(end, None);
        let old: chrono::DateTime<chrono::Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        assert_eq!(
            post_handler(
                partner("42"),
                State(db.pool.clone()),
                Json(Update { enabled: true })
            )
            .await
            .status(),
            StatusCode::OK
        );
        let mut connection = db.pool.acquire().await.unwrap();
        tb_chat::sub_reminder::record_subscription_event(
            &mut connection,
            "42",
            "viewer",
            "during-disabled",
            old,
            true,
        )
        .await
        .unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, Option<String>>(
                "SELECT end_message_id FROM twitch_sub_reminders"
            )
            .fetch_one(&db.pool)
            .await
            .unwrap(),
            None
        );
        assert_eq!(
            get_handler(DashboardAuthLevel::None, State(db.pool.clone()))
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM streamer_plans WHERE twitch_user_id<>'42'"
            )
            .fetch_one(&db.pool)
            .await
            .unwrap(),
            0
        );
    }
}
