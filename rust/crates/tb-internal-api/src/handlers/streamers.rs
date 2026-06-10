//! Handler für 7 Streamer-CRUD-Endpoints.
//!
//! Alle Endpoints: `auth.is_privileged()` → 401.
//! Kein Idempotency-Caching (kommt später).
//! Discord-Nebeneffekte: deferred bis Schritt 5/6.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Extension, Json,
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::sync::Arc;
use tb_analytics::streamers_crud as db;
use tb_http_core::{ApiError, AuthLevel};
use tb_transport_twitch::HelixClient;

// ── Response-Typen ────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct OkResponse {
    pub ok: bool,
}

#[derive(Serialize)]
pub struct OkMessageResponse {
    pub ok: bool,
    pub message: String,
}

#[derive(Serialize)]
pub struct StreamersListResponse {
    pub ok: bool,
    pub streamers: Vec<tb_analytics::streamers_crud::StreamerListRow>,
}

// ── Request-Typen ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddStreamerRequest {
    pub login: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatActionRequest {
    /// "chat" | "action" | "announcement"
    pub mode: String,
    pub message: String,
    #[serde(default)]
    pub color: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveRequest {
    /// "archive" | "unarchive" | "block" | "unblock"
    pub mode: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscordFlagRequest {
    pub is_on_discord: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscordProfileRequest {
    pub discord_user_id: Option<String>,
    pub discord_display_name: Option<String>,
    /// Wenn true: is_on_discord = 1
    #[serde(default)]
    pub mark_member: bool,
}

// ── Handler ───────────────────────────────────────────────────────────────────

/// `GET /internal/twitch/v1/streamers`
pub async fn list_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    // Ziel-Spiel wie Python (`os.getenv` mit "Deadlock"-Default) für die
    // last_deadlock_stream_at-Erkennung.
    let target_game = std::env::var("TWITCH_TARGET_GAME_NAME")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "Deadlock".to_string());
    let streamers = db::list_streamers(&pool, &target_game).await.map_err(|e| {
        tracing::error!("list_streamers DB-Fehler: {e}");
        ApiError::internal()
    })?;

    Ok(Json(StreamersListResponse {
        ok: true,
        streamers,
    }))
}

/// `POST /internal/twitch/v1/streamers`
///
/// Wenn HelixClient nicht konfiguriert: 503.
/// Wenn Helix den Login nicht kennt: 422 `{"ok": false, "error": "unknown_login"}`.
/// Wenn bereits aktiver Partner: 200 `{"ok": true, "message": "already_active_partner"}`.
pub async fn add_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Extension(helix): Extension<Arc<Option<HelixClient>>>,
    Json(body): Json<AddStreamerRequest>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    let login = body.login.trim().to_lowercase();
    if login.is_empty() {
        return Err(ApiError::bad_request_with_body(
            serde_json::json!({"ok": false, "error": "login_required"}),
        ));
    }

    // Helix-Lookup: user_id auflösen und Login validieren
    // `helix` ist Arc<Option<HelixClient>> — `(*helix).as_ref()` gibt Option<&HelixClient>
    let user_id: Option<String> = match (*helix).as_ref() {
        None => {
            // HelixClient nicht konfiguriert → 503
            return Ok((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"ok": false, "error": "helix_unavailable"})),
            )
                .into_response());
        }
        Some(client) => {
            match client.get_users(&[login.as_str()]).await {
                Ok(map) => {
                    if map.contains_key(&login) {
                        map.get(&login).map(|u| u.id.clone())
                    } else {
                        // Helix kennt den Login nicht → 422
                        return Ok((
                            StatusCode::UNPROCESSABLE_ENTITY,
                            Json(serde_json::json!({"ok": false, "error": "unknown_login"})),
                        )
                            .into_response());
                    }
                }
                Err(e) => {
                    tracing::warn!("Helix-Lookup für {login} fehlgeschlagen: {e}");
                    // Helix-Fehler → trotzdem fortfahren ohne user_id (graceful degradation)
                    None
                }
            }
        }
    };

    use db::AddStreamerResult;
    match db::add_streamer(&pool, &login, user_id.as_deref()).await {
        Ok(AddStreamerResult::AlreadyExists) => Ok((
            StatusCode::OK,
            Json(serde_json::json!({"ok": true, "login": login, "message": "already_active_partner"})),
        )
            .into_response()),
        Ok(AddStreamerResult::Added) => Ok((
            StatusCode::OK,
            Json(serde_json::json!({"ok": true, "message": format!("{login} hinzugefügt")})),
        )
            .into_response()),
        Err(e) => {
            tracing::error!("add_streamer DB-Fehler: {e}");
            Err(ApiError::internal())
        }
    }
}

/// `DELETE /internal/twitch/v1/streamers/{login}`
pub async fn remove_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Path(login): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    use db::RemoveStreamerResult;
    match db::remove_streamer(&pool, &login).await {
        Ok(RemoveStreamerResult::NotFound) => Err(ApiError::not_found()),
        Ok(RemoveStreamerResult::Archived) => Ok(Json(OkMessageResponse {
            ok: true,
            message: format!("{login} archiviert"),
        })),
        Ok(RemoveStreamerResult::Deleted) => Ok(Json(OkMessageResponse {
            ok: true,
            message: format!("{login} gelöscht"),
        })),
        Err(e) => {
            tracing::error!("remove_streamer DB-Fehler: {e}");
            Err(ApiError::internal())
        }
    }
}

/// `POST /internal/twitch/v1/streamers/{login}/verify`
pub async fn verify_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Path(login): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    use db::VerifyStreamerResult;
    match db::verify_streamer(&pool, &login).await {
        Ok(VerifyStreamerResult::Verified) => Ok(Json(OkMessageResponse {
            ok: true,
            message: format!("{login} verifiziert"),
        })),
        Ok(VerifyStreamerResult::NotAPartner) => Err(ApiError::not_found()),
        Err(e) => {
            tracing::error!("verify_streamer DB-Fehler: {e}");
            Err(ApiError::internal())
        }
    }
}

/// `POST /internal/twitch/v1/streamers/{login}/archive`
pub async fn archive_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Path(login): Path<String>,
    Json(body): Json<ArchiveRequest>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    let mode = db::ArchiveMode::parse(&body.mode).ok_or_else(|| {
        ApiError::bad_request_with_body(serde_json::json!({
            "ok": false,
            "error": "invalid_mode",
            "message": "ungültiger mode — erwartet: archive|unarchive|block|unblock"
        }))
    })?;

    match db::archive_streamer(&pool, &login, mode).await {
        Ok(true) => Ok(Json(OkResponse { ok: true })),
        Ok(false) => Err(ApiError::not_found()),
        Err(e) => {
            tracing::error!("archive_streamer DB-Fehler: {e}");
            Err(ApiError::internal())
        }
    }
}

/// `POST /internal/twitch/v1/streamers/{login}/discord-flag`
pub async fn discord_flag_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Path(login): Path<String>,
    Json(body): Json<DiscordFlagRequest>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    match db::set_discord_flag(&pool, &login, body.is_on_discord).await {
        Ok(true) => Ok(Json(OkResponse { ok: true })),
        Ok(false) => Err(ApiError::not_found()),
        Err(e) => {
            tracing::error!("set_discord_flag DB-Fehler: {e}");
            Err(ApiError::internal())
        }
    }
}

/// `POST /internal/twitch/v1/streamers/{login}/discord-profile`
pub async fn discord_profile_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Path(login): Path<String>,
    Json(body): Json<DiscordProfileRequest>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    // Discord-ID muss numerisch sein wenn angegeben
    if let Some(ref did) = body.discord_user_id {
        let clean = did.trim();
        if !clean.is_empty() && !clean.chars().all(|c| c.is_ascii_digit()) {
            return Err(ApiError::bad_request_with_body(serde_json::json!({
                "ok": false,
                "error": "invalid_discord_id",
                "message": "discord_user_id muss numerisch sein"
            })));
        }
    }

    match db::set_discord_profile(
        &pool,
        &login,
        body.discord_user_id.as_deref(),
        body.discord_display_name.as_deref(),
        body.mark_member,
    )
    .await
    {
        Ok(true) => Ok(Json(OkResponse { ok: true })),
        Ok(false) => Err(ApiError::not_found()),
        Err(e) => {
            tracing::error!("set_discord_profile DB-Fehler: {e}");
            Err(ApiError::internal())
        }
    }
}

/// `POST /internal/twitch/v1/streamers/{login}/chat-action`
///
/// Sendet eine Chat-Nachricht oder Announcement im Kanal des Streamers.
/// Erfordert `TWITCH_BOT_TOKEN` und `TWITCH_BOT_USER_ID` als Env-Variablen.
pub async fn chat_action_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Path(login): Path<String>,
    Extension(helix): Extension<Arc<Option<HelixClient>>>,
    Json(body): Json<ChatActionRequest>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }

    let bot_token = std::env::var("TWITCH_BOT_TOKEN").unwrap_or_default();
    let bot_user_id = std::env::var("TWITCH_BOT_USER_ID").unwrap_or_default();
    if bot_token.is_empty() || bot_user_id.is_empty() {
        return Ok((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"ok": false, "error": "bot_credentials_missing"})),
        )
            .into_response());
    }

    let helix_client = match (*helix).as_ref() {
        Some(c) => c,
        None => {
            return Ok((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"ok": false, "error": "helix_unavailable"})),
            )
                .into_response())
        }
    };

    let login_lower = login.to_lowercase();
    let broadcaster_id: Option<String> = sqlx::query_scalar(
        "SELECT twitch_user_id FROM twitch_streamers WHERE LOWER(twitch_login) = LOWER($1)",
    )
    .bind(&login_lower)
    .fetch_optional(&pool)
    .await
    .map_err(|e| {
        tracing::error!("chat_action DB-Fehler für {login_lower}: {e}");
        ApiError::internal()
    })?;

    let Some(broadcaster_id) = broadcaster_id.filter(|s| !s.is_empty()) else {
        return Err(ApiError::not_found());
    };

    let mode = body.mode.trim();
    let send_message = if mode == "action" {
        format!("/me {}", body.message)
    } else {
        body.message.clone()
    };

    let resp = if mode == "announcement" {
        let color = body
            .color
            .as_deref()
            .map(|c| c.trim())
            .filter(|c| !c.is_empty())
            .unwrap_or("purple");
        helix_client
            .post_with_user_token(
                &format!(
                    "/chat/announcements?broadcaster_id={broadcaster_id}&moderator_id={bot_user_id}"
                ),
                &bot_token,
            )
            .json(&serde_json::json!({"message": send_message, "color": color}))
            .send()
            .await
    } else {
        helix_client
            .post_with_user_token("/chat/messages", &bot_token)
            .json(&serde_json::json!({
                "broadcaster_id": broadcaster_id,
                "sender_id": bot_user_id,
                "message": send_message,
            }))
            .send()
            .await
    };

    match resp {
        Ok(r) if r.status().is_success() || r.status().as_u16() == 204 => {
            Ok(Json(OkMessageResponse {
                ok: true,
                message: "gesendet".to_string(),
            })
            .into_response())
        }
        Ok(r) => {
            let status = r.status();
            let body_txt = r.text().await.unwrap_or_default();
            tracing::warn!("chat_action Helix-Fehler HTTP {status} für {login_lower}: {body_txt}");
            Ok((
                StatusCode::BAD_GATEWAY,
                Json(
                    serde_json::json!({"ok": false, "error": "helix_error", "helix_status": status.as_u16()}),
                ),
            )
                .into_response())
        }
        Err(e) => {
            tracing::error!("chat_action Request-Fehler für {login_lower}: {e}");
            Err(ApiError::internal())
        }
    }
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
        routing::{delete, get, post},
        Extension, Router,
    };
    use sqlx::postgres::PgPoolOptions;
    use std::net::SocketAddr;
    use tb_http_core::{internal_auth, loopback_only, ExpectedToken, INTERNAL_API_BASE_PATH};
    use tower::ServiceExt;

    fn test_dsn() -> Option<String> {
        std::env::var("TB_TEST_DATABASE_URL").ok()
    }

    async fn make_pool(dsn: &str, schema: &str) -> PgPool {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(dsn)
            .await
            .expect("DB-Verbindung");

        sqlx::query(&format!("CREATE SCHEMA IF NOT EXISTS {schema}"))
            .execute(&pool)
            .await
            .expect("Schema");

        sqlx::query(&format!("SET search_path TO {schema}"))
            .execute(&pool)
            .await
            .expect("search_path");

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_streamers (
                twitch_login        TEXT PRIMARY KEY,
                twitch_user_id      TEXT,
                discord_user_id     TEXT,
                discord_display_name TEXT,
                is_on_discord       INTEGER DEFAULT 0,
                is_verified         INTEGER DEFAULT 0,
                is_monitored_only   INTEGER DEFAULT 0,
                created_at          TIMESTAMPTZ DEFAULT NOW(),
                archived_at         TIMESTAMPTZ
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_streamers");

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_streamer_identities (
                twitch_user_id      TEXT PRIMARY KEY,
                twitch_login        TEXT NOT NULL,
                discord_user_id     TEXT,
                discord_display_name TEXT,
                is_on_discord       INTEGER DEFAULT 0,
                created_at          TIMESTAMPTZ DEFAULT NOW(),
                updated_at          TIMESTAMPTZ DEFAULT NOW()
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_streamer_identities");

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_live_state (
                streamer_login TEXT PRIMARY KEY,
                is_live        INTEGER DEFAULT 0
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_live_state");

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_partners (
                id                       SERIAL PRIMARY KEY,
                twitch_login             TEXT NOT NULL,
                twitch_user_id           TEXT,
                status                   TEXT DEFAULT 'active',
                manual_verified_permanent INTEGER DEFAULT 0,
                manual_verified_at       TIMESTAMPTZ,
                manual_verified_until    TIMESTAMPTZ,
                admin_archived_at        TIMESTAMPTZ,
                technical_pause_reason   TEXT,
                manual_partner_opt_out   INTEGER DEFAULT 0,
                raid_bot_enabled         INTEGER DEFAULT 1
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL twitch_partners");

        // Quellen der Listen-Query (Partner-View als Test-Tabelle + Joins).
        for ddl in [
            "CREATE TABLE IF NOT EXISTS twitch_partners_all_state (
                twitch_login TEXT, twitch_user_id TEXT,
                manual_verified_permanent INTEGER DEFAULT 0,
                manual_verified_until TEXT, manual_verified_at TEXT,
                manual_partner_opt_out INTEGER DEFAULT 0, archived_at TEXT,
                is_on_discord INTEGER DEFAULT 0, discord_user_id TEXT,
                discord_display_name TEXT, raid_bot_enabled INTEGER DEFAULT 1,
                status TEXT DEFAULT 'active' )",
            "CREATE TABLE IF NOT EXISTS twitch_raid_auth (
                twitch_user_id TEXT PRIMARY KEY, twitch_login TEXT,
                raid_enabled BOOLEAN, needs_reauth BOOLEAN,
                authorized_at TIMESTAMPTZ, token_expires_at TIMESTAMPTZ )",
            "CREATE TABLE IF NOT EXISTS twitch_stream_sessions (
                streamer_login TEXT, game_name TEXT,
                had_deadlock_in_session BOOLEAN DEFAULT FALSE,
                started_at TIMESTAMPTZ, ended_at TIMESTAMPTZ )",
        ] {
            sqlx::query(ddl)
                .execute(&pool)
                .await
                .expect("DDL Listen-Quellen");
        }

        sqlx::query(
            "TRUNCATE twitch_streamers, twitch_streamer_identities, twitch_live_state, twitch_partners, twitch_partners_all_state, twitch_raid_auth, twitch_stream_sessions RESTART IDENTITY",
        )
        .execute(&pool)
        .await
        .expect("TRUNCATE");

        pool
    }

    fn make_router(pool: PgPool, token: &str) -> Router {
        let base = INTERNAL_API_BASE_PATH;
        let helix: Arc<Option<HelixClient>> = Arc::new(None);
        Router::new()
            .route(&format!("{base}/streamers"), get(list_handler))
            .route(&format!("{base}/streamers"), post(add_handler))
            .route(&format!("{base}/streamers/:login"), delete(remove_handler))
            .route(
                &format!("{base}/streamers/:login/verify"),
                post(verify_handler),
            )
            .route(
                &format!("{base}/streamers/:login/archive"),
                post(archive_handler),
            )
            .route(
                &format!("{base}/streamers/:login/discord-flag"),
                post(discord_flag_handler),
            )
            .route(
                &format!("{base}/streamers/:login/discord-profile"),
                post(discord_profile_handler),
            )
            .with_state(pool)
            .layer(Extension(helix))
            .layer(Extension(ExpectedToken(token.to_string())))
            .layer(middleware::from_fn_with_state(
                token.to_string(),
                internal_auth,
            ))
            .layer(middleware::from_fn(loopback_only))
    }

    fn loopback_req(method: &str, uri: &str, body: &str, token: Option<&str>) -> Request<Body> {
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
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_sh_401").await;
        let app = make_router(pool, "secret");
        let base = INTERNAL_API_BASE_PATH;
        let req = loopback_req("GET", &format!("{base}/streamers"), "", None);
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn list_returns_200() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_sh_list").await;
        let app = make_router(pool, "secret");
        let base = INTERNAL_API_BASE_PATH;
        let req = loopback_req("GET", &format!("{base}/streamers"), "", Some("secret"));
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn remove_returns_404_bei_unbekanntem_login() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_sh_remove_404").await;
        let app = make_router(pool, "secret");
        let base = INTERNAL_API_BASE_PATH;
        let req = loopback_req(
            "DELETE",
            &format!("{base}/streamers/nichtvorhanden"),
            "",
            Some("secret"),
        );
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn archive_returns_400_bei_ungueltigem_mode() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_sh_archive_400").await;
        let app = make_router(pool, "secret");
        let base = INTERNAL_API_BASE_PATH;
        let req = loopback_req(
            "POST",
            &format!("{base}/streamers/testuser/archive"),
            r#"{"mode":"ungueltig"}"#,
            Some("secret"),
        );
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn discord_profile_returns_400_bei_nicht_numerischer_discord_id() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_sh_discord_val").await;
        let app = make_router(pool, "secret");
        let base = INTERNAL_API_BASE_PATH;
        let req = loopback_req(
            "POST",
            &format!("{base}/streamers/testuser/discord-profile"),
            r#"{"discordUserId":"nicht-eine-zahl"}"#,
            Some("secret"),
        );
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn add_returns_503_wenn_helix_nicht_konfiguriert() {
        let dsn = match test_dsn() {
            Some(d) => d,
            None => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            }
        };
        let pool = make_pool(&dsn, "test_sh_add_503").await;
        let app = make_router(pool, "secret"); // helix = Arc::new(None) → 503
        let base = INTERNAL_API_BASE_PATH;
        let req = loopback_req(
            "POST",
            &format!("{base}/streamers"),
            r#"{"login":"someuser"}"#,
            Some("secret"),
        );
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
}
