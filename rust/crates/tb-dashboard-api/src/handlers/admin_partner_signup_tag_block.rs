use axum::{
    extract::{Extension, State},
    http::HeaderMap,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tb_analytics::partner_signup_tag_block as db;
use tb_http_core::ApiError;

use crate::auth::level::DashboardAuthLevel;
use crate::auth::session::DashboardAuthState;
use crate::handlers::admin_actor;

const DEFAULT_REASON: &str = "tag_block";

#[derive(Deserialize)]
pub struct AddRequest {
    pub tag: String,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default, alias = "publicMessage")]
    pub public_message: Option<String>,
}

#[derive(Deserialize)]
pub struct RemoveRequest {
    pub tag: String,
}

#[derive(Serialize)]
pub struct EntryResponse {
    pub tag: String,
    pub display_tag: String,
    pub reason: String,
    pub public_message: Option<String>,
    pub added_by: String,
    pub added_at: String,
}

impl From<db::TagBlockEntry> for EntryResponse {
    fn from(entry: db::TagBlockEntry) -> Self {
        Self {
            tag: entry.tag,
            display_tag: entry.display_tag,
            reason: entry.reason,
            public_message: entry.public_message,
            added_by: entry.added_by,
            added_at: entry.added_at.to_rfc3339(),
        }
    }
}

#[derive(Serialize)]
pub struct ListResponse {
    pub items: Vec<EntryResponse>,
}

#[derive(Serialize)]
pub struct AddResponse {
    pub ok: bool,
    pub tag: String,
    pub display_tag: String,
    pub inserted: bool,
}

#[derive(Serialize)]
pub struct RemoveResponse {
    pub ok: bool,
    pub tag: String,
    pub removed: bool,
}

fn db_error(error: sqlx::Error) -> ApiError {
    tracing::error!(%error, "admin_partner_signup_tag_block DB-Fehler");
    ApiError::internal()
}

fn require_tag(tag: &str) -> Result<String, ApiError> {
    db::normalize_tag(tag).ok_or_else(|| ApiError::bad_request("invalid tag"))
}

pub async fn list_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
) -> Result<impl IntoResponse, ApiError> {
    if let Some(error) = crate::auth::require_admin(&auth) {
        return Err(error);
    }

    let items = db::list_entries(&pool)
        .await
        .map_err(db_error)?
        .into_iter()
        .map(EntryResponse::from)
        .collect();

    Ok(Json(ListResponse { items }))
}

pub async fn add_handler(
    auth: DashboardAuthLevel,
    config: Option<Extension<DashboardAuthState>>,
    headers: HeaderMap,
    State(pool): State<PgPool>,
    Json(body): Json<AddRequest>,
) -> Result<impl IntoResponse, ApiError> {
    if let Some(error) = crate::auth::require_admin(&auth) {
        return Err(error);
    }

    let actor = admin_actor::admin_actor_label(config.as_ref(), &headers).await;
    add_entry(&pool, body, &actor).await
}

async fn add_entry(
    pool: &PgPool,
    body: AddRequest,
    actor: &str,
) -> Result<Json<AddResponse>, ApiError> {
    let tag = require_tag(&body.tag)?;
    let display_tag = body.tag.trim().to_string();
    let reason = body
        .reason
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_REASON)
        .to_string();
    let public_message = body
        .public_message
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let outcome = db::add(pool, &display_tag, &reason, public_message, actor)
        .await
        .map_err(db_error)?;
    match db::backfill(pool, &tag).await {
        Ok(backfilled) => {
            tracing::info!(%tag, backfilled, "Tag-Backfill nach Admin-Write");
        }
        Err(error) => {
            tracing::warn!(
                %error,
                %tag,
                "Tag-Regel gespeichert, unmittelbarer Backfill fehlgeschlagen; Bot-Sweep versucht erneut"
            );
        }
    }

    Ok(Json(AddResponse {
        ok: true,
        tag,
        display_tag,
        inserted: outcome.inserted,
    }))
}

pub async fn remove_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Json(body): Json<RemoveRequest>,
) -> Result<impl IntoResponse, ApiError> {
    if let Some(error) = crate::auth::require_admin(&auth) {
        return Err(error);
    }

    let tag = require_tag(&body.tag)?;
    let outcome = db::remove(&pool, &tag).await.map_err(db_error)?;

    Ok(Json(RemoveResponse {
        ok: true,
        tag,
        removed: outcome.removed,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;
    use serde_json::Value;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    fn lazy_pool() -> PgPool {
        PgPoolOptions::new()
            .connect_lazy("postgres://unused:unused@127.0.0.1/unused")
            .expect("lazy pool")
    }

    macro_rules! db_dsn_or_skip {
        () => {
            match std::env::var("TB_TEST_DATABASE_URL").ok() {
                Some(dsn) => dsn,
                None => {
                    if std::env::var("TB_TEST_REQUIRE_DB").as_deref() == Ok("1") {
                        panic!("TB_TEST_REQUIRE_DB=1 gesetzt, aber TB_TEST_DATABASE_URL fehlt");
                    }
                    eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                    return;
                }
            }
        };
    }

    async fn make_pool(dsn: &str, schema: &str) -> PgPool {
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(dsn)
            .await
            .expect("connect test-db");
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(dsn)
            .unwrap()
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(opts)
            .await
            .unwrap();
        sqlx::query(
            r#"
            CREATE TABLE twitch_partner_signup_tag_blocks (
                tag            TEXT PRIMARY KEY,
                display_tag    TEXT NOT NULL,
                reason         TEXT NOT NULL,
                public_message TEXT,
                added_by       TEXT NOT NULL,
                added_at       TIMESTAMPTZ NOT NULL DEFAULT now()
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE twitch_stream_sessions (
                id BIGSERIAL PRIMARY KEY,
                streamer_login TEXT NOT NULL,
                twitch_user_id TEXT,
                tags TEXT
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            r#"
            CREATE TABLE twitch_partner_signup_denylist (
                twitch_user_id TEXT PRIMARY KEY,
                twitch_login   TEXT NOT NULL,
                reason         TEXT NOT NULL,
                public_message TEXT,
                added_by       TEXT NOT NULL,
                added_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
                partner_paused_by_block BOOLEAN NOT NULL DEFAULT false
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        for ddl in [
            "CREATE TABLE twitch_raid_blacklist (
                target_id    TEXT,
                target_login TEXT NOT NULL PRIMARY KEY,
                reason       TEXT,
                added_at     TEXT DEFAULT CURRENT_TIMESTAMP
            )",
            "CREATE TABLE twitch_raid_auth (twitch_user_id TEXT PRIMARY KEY, twitch_login TEXT)",
            "CREATE TABLE twitch_partners (
                id BIGSERIAL PRIMARY KEY,
                twitch_user_id TEXT,
                twitch_login TEXT,
                status TEXT,
                raid_bot_enabled INTEGER,
                technical_pause_reason TEXT,
                manual_partner_opt_out INTEGER DEFAULT 0
            )",
            "CREATE TABLE twitch_streamer_identities (twitch_user_id TEXT PRIMARY KEY, twitch_login TEXT)",
            "CREATE TABLE twitch_streamers (id BIGSERIAL PRIMARY KEY, twitch_login TEXT UNIQUE NOT NULL, twitch_user_id TEXT)",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        pool
    }

    async fn drop_schema(pool: PgPool, dsn: &str, schema: &str) {
        pool.close().await;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(dsn)
            .await
            .expect("connect test-db");
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
    }

    async fn body_json(result: Result<impl IntoResponse, ApiError>) -> (StatusCode, Value) {
        let response = result.into_response();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 65536)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    #[tokio::test]
    async fn list_verlangt_admin_ohne_db_zugriff() {
        let response = list_handler(DashboardAuthLevel::None, State(lazy_pool())).await;
        let (status, _) = body_json(response).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn add_verlangt_admin_ohne_db_zugriff() {
        let response = add_handler(
            DashboardAuthLevel::None,
            None,
            HeaderMap::new(),
            State(lazy_pool()),
            Json(AddRequest {
                tag: "german".into(),
                reason: None,
                public_message: None,
            }),
        )
        .await;
        let (status, _) = body_json(response).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn remove_verlangt_admin_ohne_db_zugriff() {
        let response = remove_handler(
            DashboardAuthLevel::None,
            State(lazy_pool()),
            Json(RemoveRequest { tag: "german".into() }),
        )
        .await;
        let (status, _) = body_json(response).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn add_mit_leerem_tag_ist_400_ohne_schreibzugriff() {
        let response = add_entry(
            &lazy_pool(),
            AddRequest {
                tag: "   ".into(),
                reason: None,
                public_message: None,
            },
            "test",
        )
        .await;
        let (status, _) = body_json(response).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn remove_mit_leerem_tag_ist_400() {
        let response = remove_handler(
            DashboardAuthLevel::admin(),
            State(lazy_pool()),
            Json(RemoveRequest { tag: "  ".into() }),
        )
        .await;
        let (status, _) = body_json(response).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn add_entry_speichert_mit_default_grund_und_actor() {
        let dsn = db_dsn_or_skip!();
        let schema = "t_tag_block_api_add";
        let pool = make_pool(&dsn, schema).await;

        let response = add_entry(
            &pool,
            AddRequest {
                tag: "  German ".into(),
                reason: None,
                public_message: Some("Kein Interesse".into()),
            },
            "discord:4711",
        )
        .await;
        let (status, body) = body_json(response).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["ok"], true);
        assert_eq!(body["tag"], "german");
        assert_eq!(body["display_tag"], "German");
        assert_eq!(body["inserted"], true);

        let row: (String, String, Option<String>) = sqlx::query_as(
            "SELECT reason, added_by, public_message FROM twitch_partner_signup_tag_blocks
              WHERE tag = 'german'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.0, "tag_block");
        assert_eq!(row.1, "discord:4711");
        assert_eq!(row.2.as_deref(), Some("Kein Interesse"));
        drop_schema(pool, &dsn, schema).await;
    }

    #[tokio::test]
    async fn add_entry_backfillt_historische_sessions_sofort() {
        let dsn = db_dsn_or_skip!();
        let schema = "t_tag_block_api_backfill";
        let pool = make_pool(&dsn, schema).await;
        sqlx::query(
            "INSERT INTO twitch_stream_sessions (streamer_login, twitch_user_id, tags)
             VALUES ('kanal_alt', '77', 'Deutsch')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let response = add_entry(
            &pool,
            AddRequest {
                tag: "Deutsch".into(),
                reason: None,
                public_message: None,
            },
            "discord:4711",
        )
        .await;
        let (status, _) = body_json(response).await;
        assert_eq!(status, StatusCode::OK);

        let (reason, added_by): (String, String) = sqlx::query_as(
            "SELECT reason, added_by FROM twitch_partner_signup_denylist
              WHERE twitch_user_id = '77'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(reason, "tag_block:deutsch");
        assert_eq!(added_by, "tag_block");
        drop_schema(pool, &dsn, schema).await;
    }

    #[tokio::test]
    async fn add_entry_bleibt_erfolgreich_wenn_backfill_fehlschlaegt() {
        let dsn = db_dsn_or_skip!();
        let schema = "t_tag_block_api_backfill_fail";
        let pool = make_pool(&dsn, schema).await;
        sqlx::query("DROP TABLE twitch_stream_sessions")
            .execute(&pool)
            .await
            .unwrap();

        let response = add_entry(
            &pool,
            AddRequest {
                tag: "Deutsch".into(),
                reason: None,
                public_message: None,
            },
            "discord:4711",
        )
        .await;
        let (status, body) = body_json(response).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["ok"], true);

        let stored: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM twitch_partner_signup_tag_blocks WHERE tag = 'deutsch')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(stored);
        drop_schema(pool, &dsn, schema).await;
    }

    #[tokio::test]
    async fn liste_gibt_eintraege_als_items_zurueck() {
        let dsn = db_dsn_or_skip!();
        let schema = "t_tag_block_api_liste";
        let pool = make_pool(&dsn, schema).await;
        sqlx::query(
            "INSERT INTO twitch_partner_signup_tag_blocks
                 (tag, display_tag, reason, public_message, added_by)
             VALUES ('german', 'German', 'tag_block', NULL, 'discord:1')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let response = list_handler(DashboardAuthLevel::admin(), State(pool.clone())).await;
        let (status, body) = body_json(response).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["items"][0]["tag"], "german");
        assert_eq!(body["items"][0]["display_tag"], "German");
        assert_eq!(body["items"][0]["added_by"], "discord:1");
        drop_schema(pool, &dsn, schema).await;
    }

    #[tokio::test]
    async fn remove_meldet_entfernung_oder_fehlen() {
        let dsn = db_dsn_or_skip!();
        let schema = "t_tag_block_api_remove";
        let pool = make_pool(&dsn, schema).await;
        sqlx::query(
            "INSERT INTO twitch_partner_signup_tag_blocks
                 (tag, display_tag, reason, added_by)
             VALUES ('german', 'German', 'tag_block', 'discord:1')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let response = remove_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Json(RemoveRequest { tag: "German".into() }),
        )
        .await;
        let (status, body) = body_json(response).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["removed"], true);

        let response = remove_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Json(RemoveRequest { tag: "german".into() }),
        )
        .await;
        let (status, body) = body_json(response).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["removed"], false);
        drop_schema(pool, &dsn, schema).await;
    }
}
