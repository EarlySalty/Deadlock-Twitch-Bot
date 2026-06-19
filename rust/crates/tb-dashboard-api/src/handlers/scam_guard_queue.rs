use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;

use crate::auth::level::DashboardAuthLevel;

#[derive(Deserialize, Default)]
pub struct ScamGuardQuery {
    #[serde(default)]
    pub streamer: Option<String>,
}

#[derive(sqlx::FromRow)]
struct QueueRow {
    id: i64,
    chatter_login: String,
    chatter_id: Option<String>,
    confidence: f64,
    category: String,
    reasoning: String,
    created_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct DetailRow {
    id: i64,
    chatter_login: String,
    chatter_id: Option<String>,
    verdict: String,
    confidence: f64,
    category: String,
    reasoning: String,
    transcript_snapshot: String,
    action_taken: String,
    created_at: DateTime<Utc>,
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

pub async fn queue_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<ScamGuardQuery>,
) -> Response {
    let login = match resolve_login(&auth, &query.streamer) {
        Ok(login) => login,
        Err(response) => return response,
    };

    match sqlx::query_as::<_, QueueRow>(
        "SELECT id, chatter_login, chatter_id, confidence::float8 AS confidence, \
                category, reasoning, created_at \
           FROM twitch_scam_guard_verdicts \
          WHERE channel_login = $1 AND action_taken = 'suggested' \
          ORDER BY created_at DESC \
          LIMIT 100",
    )
    .bind(&login)
    .fetch_all(&pool)
    .await
    {
        Ok(rows) => {
            let queue = rows
                .into_iter()
                .map(|row| {
                    json!({
                        "id": row.id,
                        "chatter_login": row.chatter_login,
                        "chatter_id": row.chatter_id,
                        "confidence": row.confidence,
                        "category": row.category,
                        "reasoning": row.reasoning,
                        "created_at": row.created_at.to_rfc3339()
                    })
                })
                .collect::<Vec<_>>();
            Json(json!({ "queue": queue })).into_response()
        }
        Err(error) => {
            tracing::error!(%error, "scam-guard queue GET database error");
            error_response(StatusCode::INTERNAL_SERVER_ERROR, "db")
        }
    }
}

pub async fn detail_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<ScamGuardQuery>,
    Path(id): Path<i64>,
) -> Response {
    let login = match resolve_login(&auth, &query.streamer) {
        Ok(login) => login,
        Err(response) => return response,
    };

    match sqlx::query_as::<_, DetailRow>(
        "SELECT id, chatter_login, chatter_id, verdict, \
                confidence::float8 AS confidence, category, reasoning, \
                transcript_snapshot, action_taken, created_at \
           FROM twitch_scam_guard_verdicts \
          WHERE id = $1 AND channel_login = $2",
    )
    .bind(id)
    .bind(&login)
    .fetch_optional(&pool)
    .await
    {
        Ok(Some(row)) => Json(json!({
            "id": row.id,
            "chatter_login": row.chatter_login,
            "chatter_id": row.chatter_id,
            "verdict": row.verdict,
            "confidence": row.confidence,
            "category": row.category,
            "reasoning": row.reasoning,
            "transcript_snapshot": row.transcript_snapshot,
            "action_taken": row.action_taken,
            "created_at": row.created_at.to_rfc3339()
        }))
        .into_response(),
        Ok(None) => error_response(StatusCode::NOT_FOUND, "not found"),
        Err(error) => {
            tracing::error!(%error, "scam-guard verdict detail GET database error");
            error_response(StatusCode::INTERNAL_SERVER_ERROR, "db")
        }
    }
}

pub async fn ignore_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<ScamGuardQuery>,
    Path(id): Path<i64>,
) -> Response {
    let login = match resolve_login(&auth, &query.streamer) {
        Ok(login) => login,
        Err(response) => return response,
    };

    match sqlx::query(
        "UPDATE twitch_scam_guard_verdicts \
            SET action_taken = 'overturned' \
          WHERE id = $1 AND channel_login = $2 AND action_taken = 'suggested'",
    )
    .bind(id)
    .bind(&login)
    .execute(&pool)
    .await
    {
        Ok(result) if result.rows_affected() > 0 => {
            Json(json!({ "ok": true })).into_response()
        }
        Ok(_) => error_response(StatusCode::NOT_FOUND, "not found"),
        Err(error) => {
            tracing::error!(%error, "scam-guard queue ignore POST database error");
            error_response(StatusCode::INTERNAL_SERVER_ERROR, "db")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use serde_json::{json, Value};
    use sqlx::{
        postgres::{PgConnectOptions, PgPoolOptions},
        Row,
    };
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
                channel_login TEXT NOT NULL,\
                chatter_login TEXT NOT NULL,\
                chatter_id TEXT NULL,\
                verdict TEXT NOT NULL,\
                confidence REAL NOT NULL,\
                category TEXT NOT NULL,\
                reasoning TEXT NOT NULL,\
                transcript_snapshot TEXT NOT NULL,\
                action_taken TEXT NOT NULL,\
                created_at TIMESTAMPTZ NOT NULL\
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

    async fn insert_verdict(
        pool: &PgPool,
        channel_login: &str,
        chatter_login: &str,
        action_taken: &str,
    ) -> i64 {
        sqlx::query_scalar(
            "INSERT INTO twitch_scam_guard_verdicts (\
                channel_login, chatter_login, chatter_id, verdict, confidence, category,\
                reasoning, transcript_snapshot, action_taken, created_at\
             ) VALUES ($1, $2, $3, 'scam', 0.88, 'impersonation', 'reason',\
                       'snapshot', $4, '2026-06-19T10:00:00Z')\
             RETURNING id",
        )
        .bind(channel_login)
        .bind(chatter_login)
        .bind(format!("{chatter_login}-id"))
        .bind(action_taken)
        .fetch_one(pool)
        .await
        .unwrap()
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
    async fn queue_lists_only_own_suggested_verdicts() {
        let Some(pool) = make_pool("t_scam_guard_queue_filter").await else {
            return;
        };
        let expected_id = insert_verdict(&pool, "nani", "queued", "suggested").await;
        insert_verdict(&pool, "nani", "already_banned", "banned").await;
        insert_verdict(&pool, "other", "foreign", "suggested").await;

        let (status, body) = body_of(
            queue_handler(
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
                "queue": [{
                    "id": expected_id,
                    "chatter_login": "queued",
                    "chatter_id": "queued-id",
                    "confidence": 0.8799999952316284_f64,
                    "category": "impersonation",
                    "reasoning": "reason",
                    "created_at": "2026-06-19T10:00:00+00:00"
                }]
            })
        );
    }

    #[tokio::test]
    async fn detail_returns_verdict_for_own_channel() {
        let Some(pool) = make_pool("t_scam_guard_detail_own").await else {
            return;
        };
        let id = insert_verdict(&pool, "nani", "detail_chatter", "suggested").await;

        let (status, body) = body_of(
            detail_handler(
                partner("NANI"),
                State(pool),
                Query(ScamGuardQuery::default()),
                Path(id),
            )
            .await,
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["id"], id);
        assert_eq!(body["chatter_login"], "detail_chatter");
        assert_eq!(body["chatter_id"], "detail_chatter-id");
        assert_eq!(body["verdict"], "scam");
        assert_eq!(body["category"], "impersonation");
        assert_eq!(body["reasoning"], "reason");
        assert_eq!(body["transcript_snapshot"], "snapshot");
        assert_eq!(body["action_taken"], "suggested");
        assert_eq!(body["created_at"], "2026-06-19T10:00:00+00:00");
    }

    #[tokio::test]
    async fn detail_hides_foreign_channel() {
        let Some(pool) = make_pool("t_scam_guard_detail_foreign").await else {
            return;
        };
        let id = insert_verdict(&pool, "other", "foreign", "suggested").await;

        let (status, body) = body_of(
            detail_handler(
                partner("nani"),
                State(pool),
                Query(ScamGuardQuery::default()),
                Path(id),
            )
            .await,
        )
        .await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, json!({ "error": "not found" }));
    }

    #[tokio::test]
    async fn ignore_overturns_suggested_and_rejects_banned_or_foreign() {
        let Some(pool) = make_pool("t_scam_guard_ignore").await else {
            return;
        };
        let suggested_id = insert_verdict(&pool, "nani", "suggested", "suggested").await;
        let banned_id = insert_verdict(&pool, "nani", "banned", "banned").await;
        let foreign_id = insert_verdict(&pool, "other", "foreign", "suggested").await;

        let (status, body) = body_of(
            ignore_handler(
                partner("nani"),
                State(pool.clone()),
                Query(ScamGuardQuery::default()),
                Path(suggested_id),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, json!({ "ok": true }));

        let row = sqlx::query(
            "SELECT action_taken FROM twitch_scam_guard_verdicts WHERE id = $1",
        )
        .bind(suggested_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.get::<String, _>("action_taken"), "overturned");

        for id in [banned_id, foreign_id] {
            let (status, body) = body_of(
                ignore_handler(
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
