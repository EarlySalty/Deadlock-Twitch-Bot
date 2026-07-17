//! Handler für die 4 Global-Ban-Endpoints.
//!
//! Vertrag (Parität zu `bot/internal_api/routes/global_ban.py`):
//! - `POST /globalban/add`    → `{ok, login, reason}`
//! - `POST /globalban/remove` → `{ok, login, removed}`
//! - `GET  /globalban/check`  → `{ok, login, banned}`
//! - `GET  /globalban`        → `{ok, entries: [{chatter_login, chatter_id,
//!   reason, added_by, added_at}]}` (snake_case wie Python)
//!
//! Login kommt als `login` oder `twitch_login` (Python: `or`-Koaleszenz) und
//! wird via `tb_domain::normalize_twitch_login` kanonisiert; leer/ungültig →
//! 400 `{"error":"bad_request","message":"invalid or missing login"}`.
//! `added_by` ist serverseitig fest `"internal_api"` — Python ignoriert
//! Body-Werte dafür genauso (`global_ban.py:41`).

use axum::{
    extract::{Path, Query, State},
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tb_analytics::global_ban as db;
use tb_domain::normalize_twitch_login;
use tb_http_core::{ApiError, AuthLevel};

use super::common::{pick_first_truthy, resolve_reason};

/// Python setzt den Urheber hart auf `"internal_api"` (`global_ban.py:41`).
const ADDED_BY: &str = "internal_api";

// ── Request/Response-Typen ────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct AddBanRequest {
    #[serde(default)]
    pub login: Option<String>,
    #[serde(default)]
    pub twitch_login: Option<String>,
    #[serde(default)]
    pub chatter_id: Option<String>,
    #[serde(default)]
    pub twitch_user_id: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Deserialize)]
pub struct RemoveBanRequest {
    #[serde(default)]
    pub login: Option<String>,
    #[serde(default)]
    pub twitch_login: Option<String>,
}

#[derive(Deserialize)]
pub struct CheckBanQuery {
    #[serde(default)]
    pub login: Option<String>,
}

#[derive(Serialize)]
pub struct AddResponse {
    pub ok: bool,
    pub login: String,
    pub reason: String,
}

#[derive(Serialize)]
pub struct RemoveResponse {
    pub ok: bool,
    pub login: String,
    pub removed: bool,
}

#[derive(Serialize)]
pub struct CheckResponse {
    pub ok: bool,
    pub login: String,
    pub banned: bool,
}

/// Feldnamen snake_case wie `pg.list_chatter_global_bans()` (Python liefert
/// die DB-Spaltennamen unverändert aus).
#[derive(Serialize)]
pub struct BanEntryResponse {
    pub chatter_login: String,
    pub chatter_id: Option<String>,
    pub reason: Option<String>,
    pub added_by: Option<String>,
    pub added_at: Option<String>,
}

#[derive(Serialize)]
pub struct ListResponse {
    pub ok: bool,
    pub entries: Vec<BanEntryResponse>,
}

#[derive(Deserialize)]
pub struct SetChannelRequest {
    pub enabled: bool,
}

#[derive(Serialize)]
pub struct ChannelResponse {
    pub twitch_login: String,
    pub global_ban_enforcement_enabled: bool,
}

#[derive(Serialize)]
pub struct ChannelListResponse {
    pub ok: bool,
    pub channels: Vec<ChannelResponse>,
}

#[derive(Serialize)]
pub struct SetChannelResponse {
    pub ok: bool,
    pub channel: ChannelResponse,
}

// ── Handler ───────────────────────────────────────────────────────────────────

/// Login aus `login` | `twitch_login` koalesziert + kanonisiert; `None` →
/// der Aufrufer antwortet 400 wie Python (`_normalize_login` leer → 400).
fn required_login(login: Option<String>, twitch_login: Option<String>) -> Result<String, ApiError> {
    normalize_twitch_login(&pick_first_truthy(login, twitch_login))
        .ok_or_else(|| ApiError::bad_request("invalid or missing login"))
}

/// `POST /internal/twitch/v1/globalban/add`
pub async fn add_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Json(body): Json<AddBanRequest>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    let login = required_login(body.login, body.twitch_login)?;
    let reason = resolve_reason(body.reason);
    // Python: `str(body.get("chatter_id") or body.get("twitch_user_id") or "").strip() or None`
    let chatter_id = {
        let raw = pick_first_truthy(body.chatter_id, body.twitch_user_id);
        let trimmed = raw.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    };

    db::add_ban(
        &pool,
        &login,
        chatter_id.as_deref(),
        Some(&reason),
        Some(ADDED_BY),
    )
    .await
    .map_err(|e| {
        tracing::error!("add_ban DB-Fehler: {e}");
        ApiError::internal()
    })?;

    Ok(Json(AddResponse {
        ok: true,
        login,
        reason,
    }))
}

/// `POST /internal/twitch/v1/globalban/remove`
pub async fn remove_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Json(body): Json<RemoveBanRequest>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    let login = required_login(body.login, body.twitch_login)?;

    let removed = db::remove_ban(&pool, &login).await.map_err(|e| {
        tracing::error!("remove_ban DB-Fehler: {e}");
        ApiError::internal()
    })?;

    Ok(Json(RemoveResponse {
        ok: true,
        login,
        removed,
    }))
}

/// `GET /internal/twitch/v1/globalban/check?login=<login>`
pub async fn check_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<CheckBanQuery>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    let login = required_login(params.login, None)?;

    let banned = db::check_ban(&pool, &login, "").await.map_err(|e| {
        tracing::error!("check_ban DB-Fehler: {e}");
        ApiError::internal()
    })?;

    Ok(Json(CheckResponse {
        ok: true,
        login,
        banned,
    }))
}

/// `GET /internal/twitch/v1/globalban`
pub async fn list_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    let entries = db::list_bans(&pool).await.map_err(|e| {
        tracing::error!("list_bans DB-Fehler: {e}");
        ApiError::internal()
    })?;

    let response_entries = entries
        .into_iter()
        .map(|e| BanEntryResponse {
            chatter_login: e.chatter_login,
            chatter_id: e.chatter_id,
            reason: e.reason,
            added_by: e.added_by,
            added_at: e.added_at.map(crate::security::datetime_to_iso),
        })
        .collect();

    Ok(Json(ListResponse {
        ok: true,
        entries: response_entries,
    }))
}

/// `GET /internal/twitch/v1/globalban/channels`
pub async fn list_channels_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    let channels = db::list_channel_enforcement(&pool)
        .await
        .map_err(|error| {
            tracing::error!(%error, "list_channel_enforcement DB-Fehler");
            ApiError::internal()
        })?
        .into_iter()
        .map(|channel| ChannelResponse {
            twitch_login: channel.twitch_login,
            global_ban_enforcement_enabled: channel.global_ban_enforcement_enabled,
        })
        .collect();

    Ok(Json(ChannelListResponse { ok: true, channels }))
}

/// `POST /internal/twitch/v1/globalban/channels/:login`
pub async fn set_channel_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Path(login): Path<String>,
    Json(body): Json<SetChannelRequest>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    let login = required_login(Some(login), None)?;
    let channel = db::set_channel_enforcement(&pool, &login, body.enabled)
        .await
        .map_err(|error| {
            tracing::error!(channel = login, %error, "set_channel_enforcement DB-Fehler");
            ApiError::internal()
        })?
        .ok_or_else(|| ApiError::not_found_with("channel not found"))?;

    tracing::info!(
        channel = channel.twitch_login,
        urteil = if channel.global_ban_enforcement_enabled {
            "anwenden"
        } else {
            "übersprungen"
        },
        grund = "global_ban_channel_setting_updated",
        "GlobalBan-Enforcement-Einstellung geändert"
    );

    Ok(Json(SetChannelResponse {
        ok: true,
        channel: ChannelResponse {
            twitch_login: channel.twitch_login,
            global_ban_enforcement_enabled: channel.global_ban_enforcement_enabled,
        },
    }))
}

// ── Handler-Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        extract::ConnectInfo,
        http::{Request, StatusCode},
        middleware,
        routing::{get, post},
        Extension, Router,
    };
    use sqlx::postgres::PgPoolOptions;
    use std::net::SocketAddr;
    use tb_http_core::{internal_auth, loopback_only, ExpectedToken};
    use tower::ServiceExt;

    fn test_dsn() -> Option<String> {
        std::env::var("TB_TEST_DATABASE_URL").ok()
    }

    /// Gibt die DSN zurück oder bricht den Test ab.
    /// Mit `TB_TEST_REQUIRE_DB=1` wird statt des stillen Skips ein panic ausgelöst.
    macro_rules! db_dsn_or_skip {
        () => {
            match test_dsn() {
                Some(d) => d,
                None => {
                    if std::env::var("TB_TEST_REQUIRE_DB").as_deref() == Ok("1") {
                        panic!("TB_TEST_REQUIRE_DB=1 ist gesetzt, aber TB_TEST_DATABASE_URL fehlt");
                    }
                    eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                    return;
                }
            }
        };
    }

    async fn make_pool(dsn: &str, schema: &str) -> PgPool {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(dsn)
            .await
            .expect("connect");

        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&pool)
            .await
            .expect("Schema droppen");
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&pool)
            .await
            .expect("Schema anlegen");
        sqlx::query(&format!("SET search_path TO {schema}"))
            .execute(&pool)
            .await
            .expect("search_path");

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_chatter_global_ban (
                chatter_login  TEXT PRIMARY KEY,
                chatter_id     TEXT,
                reason         TEXT,
                added_by       TEXT,
                added_at       TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL ban");

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_chatter_global_ban_applied (
                id            BIGSERIAL PRIMARY KEY,
                chatter_login TEXT NOT NULL,
                applied_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL ban_applied");

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_raid_blacklist (
                id           BIGSERIAL PRIMARY KEY,
                target_id    TEXT,
                target_login TEXT NOT NULL,
                reason       TEXT,
                UNIQUE (target_login)
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL blacklist");

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_partners (
                twitch_login TEXT PRIMARY KEY,
                status TEXT NOT NULL DEFAULT 'active',
                admin_archived_at TIMESTAMPTZ,
                departnered_at TIMESTAMPTZ,
                global_ban_enforcement_enabled BOOLEAN NOT NULL DEFAULT TRUE
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL partners");

        sqlx::query(
            "TRUNCATE twitch_chatter_global_ban, twitch_chatter_global_ban_applied, twitch_raid_blacklist",
        )
        .execute(&pool)
        .await
        .expect("TRUNCATE");

        pool
    }

    /// Baut Test-Router mit Loopback-Adresse als ConnectInfo.
    fn make_router(pool: PgPool, token: &str) -> Router {
        let base = tb_http_core::INTERNAL_API_BASE_PATH;
        Router::new()
            .route(&format!("{base}/globalban"), get(list_handler))
            .route(&format!("{base}/globalban/add"), post(add_handler))
            .route(&format!("{base}/globalban/remove"), post(remove_handler))
            .route(&format!("{base}/globalban/check"), get(check_handler))
            .route(
                &format!("{base}/globalban/channels"),
                get(list_channels_handler),
            )
            .route(
                &format!("{base}/globalban/channels/:login"),
                post(set_channel_handler),
            )
            .with_state(pool)
            .layer(Extension(ExpectedToken(token.to_string())))
            .layer(middleware::from_fn_with_state(
                token.to_string(),
                internal_auth,
            ))
            .layer(middleware::from_fn(loopback_only))
    }

    fn loopback_req_json(
        method: &str,
        uri: &str,
        body: &str,
        token: Option<&str>,
    ) -> Request<Body> {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .extension(ConnectInfo(
                "127.0.0.1:55555".parse::<SocketAddr>().unwrap(),
            ));
        if let Some(t) = token {
            builder = builder.header("x-internal-token", t);
        }
        builder.body(Body::from(body.to_string())).unwrap()
    }

    #[tokio::test]
    async fn returns_401_ohne_auth() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_handler_gb_401").await;
        let app = make_router(pool, "secret");

        let base = tb_http_core::INTERNAL_API_BASE_PATH;
        let req = loopback_req_json("GET", &format!("{base}/globalban"), "", None);
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn channel_toggle_route_erfordert_auth_ohne_db_zugriff() {
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://unused:unused@127.0.0.1/unused")
            .expect("lazy pool");
        let app = make_router(pool, "secret");
        let base = tb_http_core::INTERNAL_API_BASE_PATH;
        let req = loopback_req_json(
            "POST",
            &format!("{base}/globalban/channels/kanal"),
            r#"{"enabled":false}"#,
            None,
        );

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn channel_toggle_router_setzt_flag_und_liste_liefert_es() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_handler_gb_channel_toggle").await;
        sqlx::query("INSERT INTO twitch_partners (twitch_login) VALUES ('kanal')")
            .execute(&pool)
            .await
            .unwrap();
        let app = make_router(pool, "secret");
        let base = tb_http_core::INTERNAL_API_BASE_PATH;

        let req = loopback_req_json(
            "POST",
            &format!("{base}/globalban/channels/kanal"),
            r#"{"enabled":false}"#,
            Some("secret"),
        );
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let req = loopback_req_json(
            "GET",
            &format!("{base}/globalban/channels"),
            "",
            Some("secret"),
        );
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["channels"][0]["twitch_login"], "kanal");
        assert_eq!(json["channels"][0]["global_ban_enforcement_enabled"], false);
    }

    #[tokio::test]
    async fn add_normalisiert_login_und_liefert_login_reason_zurueck() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_handler_gb_add").await;
        let app = make_router(pool, "secret");

        let base = tb_http_core::INTERNAL_API_BASE_PATH;
        // Python: _normalize_login("@TestUser") → "testuser"; reason wird getrimmt.
        let body = r#"{"login":"@TestUser","reason":" Spam "}"#;
        let req = loopback_req_json(
            "POST",
            &format!("{base}/globalban/add"),
            body,
            Some("secret"),
        );
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["ok"], true);
        assert_eq!(json["login"], "testuser");
        assert_eq!(json["reason"], "Spam");
    }

    #[tokio::test]
    async fn add_400_bei_fehlendem_login_mit_python_envelope() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_handler_gb_add_400").await;
        let app = make_router(pool, "secret");

        let base = tb_http_core::INTERNAL_API_BASE_PATH;
        let req = loopback_req_json(
            "POST",
            &format!("{base}/globalban/add"),
            "{}",
            Some("secret"),
        );
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["error"], "bad_request");
        assert_eq!(json["message"], "invalid or missing login");
    }

    #[tokio::test]
    async fn add_akzeptiert_twitch_login_und_twitch_user_id_fallbacks() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_handler_gb_add_fallback").await;
        let app = make_router(pool.clone(), "secret");

        let base = tb_http_core::INTERNAL_API_BASE_PATH;
        // Python: body.get("login") or body.get("twitch_login") bzw.
        // chatter_id or twitch_user_id.
        let body = r#"{"twitch_login":"AltUser","twitch_user_id":"12345"}"#;
        let req = loopback_req_json(
            "POST",
            &format!("{base}/globalban/add"),
            body,
            Some("secret"),
        );
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["login"], "altuser");
        // Default-Reason wie Python wenn keiner angegeben.
        assert_eq!(json["reason"], "manual_ban:absolut");

        // chatter_id aus twitch_user_id gelandet → Check per ID trifft.
        let banned = db::check_ban(&pool, "anderername", "12345").await.unwrap();
        assert!(
            banned,
            "twitch_user_id muss als chatter_id gespeichert sein"
        );
    }

    #[tokio::test]
    async fn check_normalisiert_login_und_400_bei_fehlendem() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_handler_gb_check_norm").await;
        let app = make_router(pool, "secret");
        let base = tb_http_core::INTERNAL_API_BASE_PATH;

        // @-Form wird kanonisiert beantwortet.
        let req = loopback_req_json(
            "GET",
            &format!("{base}/globalban/check?login=%40Foo"),
            "",
            Some("secret"),
        );
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["login"], "foo");

        // Fehlender login-Parameter → 400 mit Python-Envelope (kein 422/Query-Reject).
        let req = loopback_req_json(
            "GET",
            &format!("{base}/globalban/check"),
            "",
            Some("secret"),
        );
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["error"], "bad_request");
    }

    #[tokio::test]
    async fn list_liefert_snake_case_felder_wie_python() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_handler_gb_list_shape").await;
        db::add_ban(
            &pool,
            "shapeuser",
            Some("99"),
            Some("Test"),
            Some("internal_api"),
        )
        .await
        .unwrap();
        let app = make_router(pool, "secret");

        let base = tb_http_core::INTERNAL_API_BASE_PATH;
        let req = loopback_req_json("GET", &format!("{base}/globalban"), "", Some("secret"));
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let bytes = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let entry = &json["entries"][0];
        // Python liefert die DB-Spaltennamen snake_case — kein camelCase.
        assert_eq!(entry["chatter_login"], "shapeuser");
        assert_eq!(entry["chatter_id"], "99");
        assert_eq!(entry["added_by"], "internal_api");
        assert!(entry.get("chatterLogin").is_none());
        assert!(entry["added_at"].is_string());
    }

    #[tokio::test]
    async fn remove_returns_200_mit_removed_false_bei_unbekanntem_login() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_handler_gb_remove").await;
        let app = make_router(pool, "secret");

        let base = tb_http_core::INTERNAL_API_BASE_PATH;
        let body = r#"{"login":"nichtvorhanden"}"#;
        let req = loopback_req_json(
            "POST",
            &format!("{base}/globalban/remove"),
            body,
            Some("secret"),
        );
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["removed"], false);
    }

    #[tokio::test]
    async fn check_returns_banned_false_bei_unbekanntem_login() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_handler_gb_check").await;
        let app = make_router(pool, "secret");

        let base = tb_http_core::INTERNAL_API_BASE_PATH;
        let req = loopback_req_json(
            "GET",
            &format!("{base}/globalban/check?login=niemand"),
            "",
            Some("secret"),
        );
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["banned"], false);
    }

    #[tokio::test]
    async fn list_returns_leeres_array() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_handler_gb_list").await;
        let app = make_router(pool, "secret");

        let base = tb_http_core::INTERNAL_API_BASE_PATH;
        let req = loopback_req_json("GET", &format!("{base}/globalban"), "", Some("secret"));
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["entries"], serde_json::json!([]));
    }
}
