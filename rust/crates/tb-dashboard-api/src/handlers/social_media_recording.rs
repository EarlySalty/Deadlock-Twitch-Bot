use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::{PgPool, Postgres, Row, Transaction};
use tb_social_media::partner_access::is_partner_id_granted;
use uuid::Uuid;

use crate::auth::level::DashboardAuthLevel;

pub const CHUNK_MAX_BYTES: usize = 2 * 1024 * 1024;
pub const SESSION_MAX_BYTES: i64 = 20 * 1024 * 1024;
pub const DAILY_MAX_BYTES: i64 = 60 * 1024 * 1024;
const CHUNK_BODY_LIMIT: usize = CHUNK_MAX_BYTES + 64 * 1024;
const CONSENT_BODY_LIMIT: usize = 4096;
const MAX_SEQ: i32 = 5000;
const MAX_EVENTS_PER_CHUNK: usize = 50_000;
const USER_AGENT_MAX_CHARS: usize = 200;
const ADMIN_LIST_LIMIT: i64 = 500;
const RETENTION_SQL: &str =
    "DELETE FROM social_media_recording_sessions WHERE started_at < now() - interval '30 days'";

pub fn partner_routes() -> Router<PgPool> {
    Router::new()
        .route(
            "/social-media/api/recording/consent",
            get(consent_get_handler)
                .post(consent_post_handler)
                .layer(DefaultBodyLimit::max(CONSENT_BODY_LIMIT)),
        )
        .route(
            "/social-media/api/recording/chunk",
            post(chunk_post_handler).layer(DefaultBodyLimit::max(CHUNK_BODY_LIMIT)),
        )
}

pub fn admin_routes() -> Router<PgPool> {
    Router::new()
        .route(
            "/twitch/api/admin/social-media/recordings",
            get(admin_list_handler),
        )
        .route(
            "/twitch/api/admin/social-media/recordings/{session_id}",
            delete(admin_delete_handler),
        )
        .route(
            "/twitch/api/admin/social-media/recordings/{session_id}/events",
            get(admin_events_handler),
        )
}

pub async fn purge_expired(pool: &PgPool) -> Result<u64, sqlx::Error> {
    Ok(sqlx::query(RETENTION_SQL)
        .execute(pool)
        .await?
        .rows_affected())
}

pub fn spawn_retention(pool: PgPool) {
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        tracing::warn!("Social-Media-Aufzeichnungen: keine Laufzeit für die Aufbewahrung");
        return;
    };
    runtime.spawn(async move {
        let start = tokio::time::Instant::now() + std::time::Duration::from_secs(120);
        let mut tick = tokio::time::interval_at(start, std::time::Duration::from_secs(3600));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            match purge_expired(&pool).await {
                Ok(deleted) => {
                    tracing::info!(deleted, "Social-Media-Aufzeichnungen: Aufbewahrung geprüft")
                }
                Err(error) => {
                    tracing::warn!(%error, "Social-Media-Aufzeichnungen: Aufbewahrung fehlgeschlagen")
                }
            }
        }
    });
}

struct Recorder {
    twitch_user_id: String,
    twitch_login: String,
}

fn recorder(auth: &DashboardAuthLevel) -> Option<Recorder> {
    match auth {
        DashboardAuthLevel::Partner {
            twitch_login,
            twitch_user_id,
            ..
        } => Some(Recorder {
            twitch_user_id: twitch_user_id.clone(),
            twitch_login: twitch_login.to_lowercase(),
        }),
        _ => None,
    }
}

fn error(status: StatusCode, code: &'static str) -> Response {
    (status, Json(json!({ "error": code }))).into_response()
}

fn db_error(context: &'static str, e: sqlx::Error) -> Response {
    tracing::error!(error = %e, context, "social media recording: DB-Fehler");
    error(StatusCode::INTERNAL_SERVER_ERROR, "db_error")
}

fn partner_required() -> Response {
    error(StatusCode::FORBIDDEN, "partner_session_required")
}

type Rejection = (StatusCode, &'static str);

fn require_admin(auth: &DashboardAuthLevel) -> Result<(), Rejection> {
    match auth {
        DashboardAuthLevel::Admin { .. } => Ok(()),
        DashboardAuthLevel::Partner { .. } => Err((StatusCode::FORBIDDEN, "admin_required")),
        DashboardAuthLevel::None => Err((StatusCode::UNAUTHORIZED, "auth_required")),
    }
}

async fn stored_decision(
    pool: &PgPool,
    twitch_user_id: &str,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar::<_, String>(
        "SELECT decision FROM social_media_recording_consent WHERE twitch_user_id = $1",
    )
    .bind(twitch_user_id)
    .fetch_optional(pool)
    .await
}

pub async fn consent_get_handler(auth: DashboardAuthLevel, State(pool): State<PgPool>) -> Response {
    let Some(recorder) = recorder(&auth) else {
        return Json(json!({ "eligible": false, "decision": null })).into_response();
    };
    let eligible = is_partner_id_granted(&pool, &recorder.twitch_user_id).await;
    match stored_decision(&pool, &recorder.twitch_user_id).await {
        Ok(decision) => Json(json!({ "eligible": eligible, "decision": decision })).into_response(),
        Err(e) => db_error("consent_get", e),
    }
}

#[derive(Deserialize)]
struct ConsentBody {
    decision: String,
}

fn parse_decision(body: &[u8]) -> Option<&'static str> {
    let parsed: ConsentBody = serde_json::from_slice(body).ok()?;
    match parsed.decision.as_str() {
        "granted" => Some("granted"),
        "declined" => Some("declined"),
        _ => None,
    }
}

async fn save_decision(
    pool: &PgPool,
    recorder: &Recorder,
    decision: &str,
) -> Result<u64, sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO social_media_recording_consent (twitch_user_id, twitch_login, decision, decided_at)
         VALUES ($1, $2, $3, now())
         ON CONFLICT (twitch_user_id) DO UPDATE
         SET twitch_login = EXCLUDED.twitch_login, decision = EXCLUDED.decision, decided_at = now()",
    )
    .bind(&recorder.twitch_user_id)
    .bind(&recorder.twitch_login)
    .bind(decision)
    .execute(&mut *tx)
    .await?;
    let deleted = if decision == "declined" {
        sqlx::query("DELETE FROM social_media_recording_sessions WHERE twitch_user_id = $1")
            .bind(&recorder.twitch_user_id)
            .execute(&mut *tx)
            .await?
            .rows_affected()
    } else {
        0
    };
    tx.commit().await?;
    Ok(deleted)
}

pub async fn consent_post_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    body: Bytes,
) -> Response {
    let Some(recorder) = recorder(&auth) else {
        return partner_required();
    };
    let Some(decision) = parse_decision(&body) else {
        return error(StatusCode::BAD_REQUEST, "invalid_decision");
    };
    match save_decision(&pool, &recorder, decision).await {
        Ok(deleted) => {
            Json(json!({ "decision": decision, "deleted_sessions": deleted })).into_response()
        }
        Err(e) => db_error("consent_post", e),
    }
}

#[derive(Deserialize)]
struct ChunkBody {
    session_id: String,
    seq: i32,
    events: Vec<Value>,
    #[serde(default)]
    meta: Option<ChunkMeta>,
}

#[derive(Deserialize, Default)]
struct ChunkMeta {
    viewport_width: Option<i32>,
    viewport_height: Option<i32>,
}

struct Chunk {
    session_id: Uuid,
    seq: i32,
    payload: String,
    meta: ChunkMeta,
    user_agent: String,
}

fn user_agent(headers: &HeaderMap) -> String {
    headers
        .get(header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .chars()
        .take(USER_AGENT_MAX_CHARS)
        .collect()
}

fn viewport_value(value: Option<i32>) -> Option<i32> {
    value.filter(|v| (1..=20_000).contains(v))
}

fn parse_chunk(body: &[u8], headers: &HeaderMap) -> Result<Chunk, Rejection> {
    let parsed: ChunkBody =
        serde_json::from_slice(body).map_err(|_| (StatusCode::BAD_REQUEST, "invalid_body"))?;
    let session_id = Uuid::parse_str(&parsed.session_id)
        .map_err(|_| (StatusCode::BAD_REQUEST, "invalid_session_id"))?;
    if !(0..=MAX_SEQ).contains(&parsed.seq) {
        return Err((StatusCode::BAD_REQUEST, "invalid_seq"));
    }
    if parsed.events.is_empty()
        || parsed.events.len() > MAX_EVENTS_PER_CHUNK
        || !parsed.events.iter().all(Value::is_object)
    {
        return Err((StatusCode::BAD_REQUEST, "invalid_events"));
    }
    let payload = serde_json::to_string(&parsed.events)
        .map_err(|_| (StatusCode::BAD_REQUEST, "invalid_events"))?;
    if payload.len() > CHUNK_MAX_BYTES {
        return Err((StatusCode::PAYLOAD_TOO_LARGE, "chunk_too_large"));
    }
    Ok(Chunk {
        session_id,
        seq: parsed.seq,
        payload,
        meta: parsed.meta.unwrap_or_default(),
        user_agent: user_agent(headers),
    })
}

enum IngestOutcome {
    Stored { remaining: i64 },
    Rejected(StatusCode, &'static str),
}

async fn consent_granted(
    tx: &mut Transaction<'_, Postgres>,
    twitch_user_id: &str,
) -> Result<bool, sqlx::Error> {
    let decision = sqlx::query_scalar::<_, String>(
        "SELECT decision FROM social_media_recording_consent WHERE twitch_user_id = $1 FOR SHARE",
    )
    .bind(twitch_user_id)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(decision.as_deref() == Some("granted"))
}

async fn lock_session(
    tx: &mut Transaction<'_, Postgres>,
    recorder: &Recorder,
    chunk: &Chunk,
) -> Result<(String, i64), sqlx::Error> {
    sqlx::query(
        "INSERT INTO social_media_recording_sessions
             (id, twitch_user_id, twitch_login, user_agent, viewport_width, viewport_height)
         VALUES ($1::uuid, $2, $3, $4, $5, $6)
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(chunk.session_id.to_string())
    .bind(&recorder.twitch_user_id)
    .bind(&recorder.twitch_login)
    .bind(&chunk.user_agent)
    .bind(viewport_value(chunk.meta.viewport_width))
    .bind(viewport_value(chunk.meta.viewport_height))
    .execute(&mut **tx)
    .await?;
    let row = sqlx::query(
        "SELECT twitch_user_id, total_bytes FROM social_media_recording_sessions
         WHERE id = $1::uuid FOR UPDATE",
    )
    .bind(chunk.session_id.to_string())
    .fetch_one(&mut **tx)
    .await?;
    Ok((row.try_get(0)?, row.try_get(1)?))
}

async fn daily_bytes(
    tx: &mut Transaction<'_, Postgres>,
    twitch_user_id: &str,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar::<_, i64>(
        "SELECT COALESCE(SUM(total_bytes), 0)::bigint FROM social_media_recording_sessions
         WHERE twitch_user_id = $1 AND started_at > now() - interval '1 day'",
    )
    .bind(twitch_user_id)
    .fetch_one(&mut **tx)
    .await
}

async fn insert_chunk(
    tx: &mut Transaction<'_, Postgres>,
    chunk: &Chunk,
    size: i32,
) -> Result<bool, sqlx::Error> {
    let inserted = sqlx::query(
        "INSERT INTO social_media_recording_chunks (session_id, seq, payload, byte_size)
         VALUES ($1::uuid, $2, $3::text::jsonb, $4)
         ON CONFLICT (session_id, seq) DO NOTHING",
    )
    .bind(chunk.session_id.to_string())
    .bind(chunk.seq)
    .bind(&chunk.payload)
    .bind(size)
    .execute(&mut **tx)
    .await?
    .rows_affected();
    if inserted == 0 {
        return Ok(false);
    }
    sqlx::query(
        "UPDATE social_media_recording_sessions
         SET total_bytes = total_bytes + $2, chunk_count = chunk_count + 1, last_chunk_at = now()
         WHERE id = $1::uuid",
    )
    .bind(chunk.session_id.to_string())
    .bind(size)
    .execute(&mut **tx)
    .await?;
    Ok(true)
}

async fn ingest(
    pool: &PgPool,
    recorder: &Recorder,
    chunk: &Chunk,
) -> Result<IngestOutcome, sqlx::Error> {
    let Ok(size) = i32::try_from(chunk.payload.len()) else {
        return Ok(IngestOutcome::Rejected(
            StatusCode::PAYLOAD_TOO_LARGE,
            "chunk_too_large",
        ));
    };
    let size_bytes = i64::from(size);
    let mut tx = pool.begin().await?;
    if !consent_granted(&mut tx, &recorder.twitch_user_id).await? {
        return Ok(IngestOutcome::Rejected(
            StatusCode::FORBIDDEN,
            "consent_required",
        ));
    }
    let (owner, session_bytes) = lock_session(&mut tx, recorder, chunk).await?;
    if owner != recorder.twitch_user_id {
        return Ok(IngestOutcome::Rejected(
            StatusCode::FORBIDDEN,
            "session_owner_mismatch",
        ));
    }
    if session_bytes.saturating_add(size_bytes) > SESSION_MAX_BYTES {
        return Ok(IngestOutcome::Rejected(
            StatusCode::PAYLOAD_TOO_LARGE,
            "session_cap_reached",
        ));
    }
    if daily_bytes(&mut tx, &recorder.twitch_user_id)
        .await?
        .saturating_add(size_bytes)
        > DAILY_MAX_BYTES
    {
        return Ok(IngestOutcome::Rejected(
            StatusCode::TOO_MANY_REQUESTS,
            "daily_cap_reached",
        ));
    }
    if !insert_chunk(&mut tx, chunk, size).await? {
        return Ok(IngestOutcome::Rejected(
            StatusCode::CONFLICT,
            "duplicate_chunk",
        ));
    }
    tx.commit().await?;
    Ok(IngestOutcome::Stored {
        remaining: SESSION_MAX_BYTES - session_bytes - size_bytes,
    })
}

pub async fn chunk_post_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(recorder) = recorder(&auth) else {
        return partner_required();
    };
    let chunk = match parse_chunk(&body, &headers) {
        Ok(chunk) => chunk,
        Err((status, code)) => return error(status, code),
    };
    if !is_partner_id_granted(&pool, &recorder.twitch_user_id).await {
        return error(StatusCode::FORBIDDEN, "social_media_access_required");
    }
    match ingest(&pool, &recorder, &chunk).await {
        Ok(IngestOutcome::Stored { remaining }) => {
            Json(json!({ "ok": true, "remaining_session_bytes": remaining })).into_response()
        }
        Ok(IngestOutcome::Rejected(status, code)) => error(status, code),
        Err(e) => db_error("chunk_post", e),
    }
}

fn session_json(row: &sqlx::postgres::PgRow) -> Result<Value, sqlx::Error> {
    let started_at: DateTime<Utc> = row.try_get("started_at")?;
    let last_chunk_at: DateTime<Utc> = row.try_get("last_chunk_at")?;
    Ok(json!({
        "id": row.try_get::<String, _>("id")?,
        "twitch_user_id": row.try_get::<String, _>("twitch_user_id")?,
        "twitch_login": row.try_get::<String, _>("twitch_login")?,
        "started_at": started_at.to_rfc3339(),
        "last_chunk_at": last_chunk_at.to_rfc3339(),
        "duration_seconds": (last_chunk_at - started_at).num_seconds().max(0),
        "total_bytes": row.try_get::<i64, _>("total_bytes")?,
        "chunk_count": row.try_get::<i32, _>("chunk_count")?,
        "viewport_width": row.try_get::<Option<i32>, _>("viewport_width")?,
        "viewport_height": row.try_get::<Option<i32>, _>("viewport_height")?,
        "user_agent": row.try_get::<String, _>("user_agent")?,
    }))
}

async fn list_sessions(pool: &PgPool) -> Result<Vec<Value>, sqlx::Error> {
    purge_expired(pool).await?;
    let rows = sqlx::query(
        "SELECT id::text AS id, twitch_user_id, twitch_login, started_at, last_chunk_at,
                total_bytes, chunk_count, viewport_width, viewport_height, user_agent
         FROM social_media_recording_sessions
         WHERE chunk_count > 0
         ORDER BY started_at DESC
         LIMIT $1",
    )
    .bind(ADMIN_LIST_LIMIT)
    .fetch_all(pool)
    .await?;
    rows.iter().map(session_json).collect()
}

pub async fn admin_list_handler(auth: DashboardAuthLevel, State(pool): State<PgPool>) -> Response {
    if let Err((status, code)) = require_admin(&auth) {
        return error(status, code);
    }
    match list_sessions(&pool).await {
        Ok(items) => Json(json!({ "items": items })).into_response(),
        Err(e) => db_error("admin_list", e),
    }
}

async fn session_payloads(pool: &PgPool, session_id: Uuid) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar::<_, String>(
        "SELECT payload::text FROM social_media_recording_chunks
         WHERE session_id = $1::uuid ORDER BY seq",
    )
    .bind(session_id.to_string())
    .fetch_all(pool)
    .await
}

fn chunks_response(payloads: &[String]) -> Response {
    let body = format!("[{}]", payloads.join(","));
    (
        [
            (header::CONTENT_TYPE, "application/json"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        body,
    )
        .into_response()
}

pub async fn admin_events_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Path(session_id): Path<String>,
) -> Response {
    if let Err((status, code)) = require_admin(&auth) {
        return error(status, code);
    }
    let Ok(session_id) = Uuid::parse_str(&session_id) else {
        return error(StatusCode::BAD_REQUEST, "invalid_session_id");
    };
    match session_payloads(&pool, session_id).await {
        Ok(payloads) if payloads.is_empty() => error(StatusCode::NOT_FOUND, "not_found"),
        Ok(payloads) => chunks_response(&payloads),
        Err(e) => db_error("admin_events", e),
    }
}

pub async fn admin_delete_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Path(session_id): Path<String>,
) -> Response {
    if let Err((status, code)) = require_admin(&auth) {
        return error(status, code);
    }
    let Ok(session_id) = Uuid::parse_str(&session_id) else {
        return error(StatusCode::BAD_REQUEST, "invalid_session_id");
    };
    let result = sqlx::query("DELETE FROM social_media_recording_sessions WHERE id = $1::uuid")
        .bind(session_id.to_string())
        .execute(&pool)
        .await;
    match result {
        Ok(done) if done.rows_affected() > 0 => Json(json!({ "ok": true })).into_response(),
        Ok(_) => error(StatusCode::NOT_FOUND, "not_found"),
        Err(e) => db_error("admin_delete", e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    const MIGRATION: &str =
        include_str!("../../../../migrations/20261025120000_social_media_recordings.sql");
    const SESSION: &str = "6f1d2c3b-4a5e-4f60-8a7b-9c0d1e2f3a4b";

    async fn pool_or_skip(schema: &str) -> Option<PgPool> {
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
            "CREATE TABLE social_media_partner_access (twitch_user_id TEXT PRIMARY KEY, granted BOOLEAN NOT NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO social_media_partner_access VALUES ('111', TRUE), ('222', TRUE)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::raw_sql(sqlx::AssertSqlSafe(MIGRATION.replace("public.", "")))
            .execute(&pool)
            .await
            .unwrap();
        Some(pool)
    }

    fn partner(id: &str) -> DashboardAuthLevel {
        DashboardAuthLevel::Partner {
            twitch_login: format!("user{id}"),
            twitch_user_id: id.to_string(),
            display_name: String::new(),
        }
    }

    fn chunk_body(seq: i32) -> Bytes {
        Bytes::from(
            json!({ "session_id": SESSION, "seq": seq, "events": [{ "type": 4, "timestamp": 1 }] })
                .to_string(),
        )
    }

    async fn send_chunk(pool: &PgPool, id: &str, seq: i32) -> StatusCode {
        chunk_post_handler(
            partner(id),
            State(pool.clone()),
            HeaderMap::new(),
            chunk_body(seq),
        )
        .await
        .status()
    }

    async fn decide(pool: &PgPool, id: &str, decision: &str) -> StatusCode {
        let body = Bytes::from(json!({ "decision": decision }).to_string());
        consent_post_handler(partner(id), State(pool.clone()), body)
            .await
            .status()
    }

    async fn chunk_rows(pool: &PgPool) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM social_media_recording_chunks")
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn ingest_requires_consent_and_revoke_deletes_recordings() {
        let Some(pool) = pool_or_skip("social_media_recording_consent").await else {
            return;
        };
        assert_eq!(send_chunk(&pool, "111", 0).await, StatusCode::FORBIDDEN);
        assert_eq!(decide(&pool, "111", "granted").await, StatusCode::OK);
        assert_eq!(send_chunk(&pool, "111", 0).await, StatusCode::OK);
        assert_eq!(send_chunk(&pool, "111", 0).await, StatusCode::CONFLICT);
        assert_eq!(send_chunk(&pool, "111", 1).await, StatusCode::OK);
        assert_eq!(decide(&pool, "222", "granted").await, StatusCode::OK);
        assert_eq!(send_chunk(&pool, "222", 2).await, StatusCode::FORBIDDEN);
        assert_eq!(chunk_rows(&pool).await, 2);
        assert_eq!(decide(&pool, "111", "declined").await, StatusCode::OK);
        assert_eq!(chunk_rows(&pool).await, 0);
        assert_eq!(send_chunk(&pool, "111", 3).await, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn retention_removes_sessions_older_than_thirty_days() {
        let Some(pool) = pool_or_skip("social_media_recording_retention").await else {
            return;
        };
        sqlx::query(
            "INSERT INTO social_media_recording_sessions (id, twitch_user_id, twitch_login, started_at, last_chunk_at)
             VALUES ($1::uuid, '111', 'user111', now() - interval '31 days', now() - interval '31 days'),
                    (gen_random_uuid(), '111', 'user111', now() - interval '29 days', now() - interval '29 days')",
        )
        .bind(SESSION)
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(purge_expired(&pool).await.unwrap(), 1);
        let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM social_media_recording_sessions")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(left, 1);
    }

    #[test]
    fn parse_chunk_rejects_bad_input() {
        let headers = HeaderMap::new();
        let reject = |body: Value| parse_chunk(body.to_string().as_bytes(), &headers).err();
        assert_eq!(
            reject(json!({ "session_id": "nope", "seq": 0, "events": [{}] })),
            Some((StatusCode::BAD_REQUEST, "invalid_session_id"))
        );
        assert_eq!(
            reject(json!({ "session_id": SESSION, "seq": -1, "events": [{}] })),
            Some((StatusCode::BAD_REQUEST, "invalid_seq"))
        );
        assert_eq!(
            reject(json!({ "session_id": SESSION, "seq": 0, "events": [1] })),
            Some((StatusCode::BAD_REQUEST, "invalid_events"))
        );
        let big = "x".repeat(CHUNK_MAX_BYTES);
        assert_eq!(
            reject(json!({ "session_id": SESSION, "seq": 0, "events": [{ "d": big }] })),
            Some((StatusCode::PAYLOAD_TOO_LARGE, "chunk_too_large"))
        );
    }
}
