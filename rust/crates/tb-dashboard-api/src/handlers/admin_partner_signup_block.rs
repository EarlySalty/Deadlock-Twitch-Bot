//! Admin-Endpoints für die Partneraufnahme-Sperrliste
//! (`twitch_partner_signup_denylist`).
//!
//! Vertrag:
//! - `GET  /twitch/api/admin/partner-signup-blocks`        → `{items: [...]}`
//! - `POST /twitch/api/admin/partner-signup-blocks`        → Add + Outcome
//! - `POST /twitch/api/admin/partner-signup-blocks/remove` → Remove + Outcome
//!
//! Die fachliche Logik liegt unverändert in
//! [`tb_analytics::partner_signup_block`]; hier steht nur die Dashboard-Hülle.
//! Bewusst getrennt von der Audio-Archiv-Ausschlussliste und vom
//! Admin-Block der Streamer-Verwaltung.
//!
//! Der Browser darf weder eine `twitch_user_id` noch einen `added_by`-Wert
//! setzen: die ID löst der Server auf (erst lokaler Bestand, dann Helix
//! `get_users`), der Bearbeiter kommt aus der Admin-Session. Ein Login ohne
//! auflösbare ID wird abgelehnt, sonst hebelt eine Umbenennung den Block aus.
//!
//! Schutzschichten: die drei Routen hängen in `build_admin_config_router`
//! (`lib.rs`), der `crate::auth::require_admin_before_csrf` und
//! `crate::auth::csrf::csrf_protect` als Layer trägt. Schreibende Aufrufe
//! brauchen also zusätzlich zur Session-Auth über [`DashboardAuthLevel`] ein
//! gültiges CSRF-Token (`csrfToken` aus `auth_status.rs`, Localhost-Bypass für
//! interne Werkzeuge). Der `require_admin`-Aufruf in jedem Handler ist die
//! zweite, vom Router unabhängige Sperre und kein Ersatz für die Layer.

use axum::{
    extract::{Extension, State},
    http::HeaderMap,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tb_analytics::partner_signup_block as db;
use tb_domain::normalize_twitch_login;
use tb_http_core::ApiError;
use tb_transport_twitch::{HelixClient, HelixConfig};

use crate::auth::level::DashboardAuthLevel;
use crate::auth::session::DashboardAuthState;
use crate::handlers::admin_actor;

/// Fallback-Grund, wenn der Admin keinen mitgibt. Landet als
/// `signup_block:<reason>` auch in der Raid-Blacklist.
const DEFAULT_REASON: &str = "owner_decision";

// ── Request-Typen ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct AddRequest {
    pub login: String,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default, alias = "publicMessage")]
    pub public_message: Option<String>,
}

#[derive(Deserialize)]
pub struct RemoveRequest {
    pub login: String,
    #[serde(default, alias = "twitchUserId")]
    pub twitch_user_id: Option<String>,
}

// ── Response-Typen ────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct EntryResponse {
    pub twitch_user_id: String,
    pub login: String,
    pub reason: String,
    pub public_message: Option<String>,
    pub added_by: String,
    pub added_at: String,
}

impl From<db::SignupBlockEntry> for EntryResponse {
    fn from(entry: db::SignupBlockEntry) -> Self {
        Self {
            twitch_user_id: entry.twitch_user_id,
            login: entry.twitch_login,
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
    pub login: String,
    pub twitch_user_id: String,
    pub reason: String,
    /// Der Eintrag war neu (`false` = bestehender Eintrag aktualisiert).
    pub inserted: bool,
    pub raid_blacklisted: bool,
    pub credentials_deleted: bool,
    pub active_partner_paused: bool,
}

#[derive(Serialize)]
pub struct RemoveResponse {
    pub ok: bool,
    pub login: String,
    pub removed: bool,
    pub raid_entries_removed: u64,
    pub partner_pause_cleared: bool,
}

// ── Hilfen ────────────────────────────────────────────────────────────────────

fn db_error(error: sqlx::Error) -> ApiError {
    tracing::error!(%error, "admin_partner_signup_block DB-Fehler");
    ApiError::internal()
}

fn require_login(login: &str) -> Result<String, ApiError> {
    normalize_twitch_login(login).ok_or_else(|| ApiError::bad_request("invalid login"))
}

/// Baut den Helix-Client aus der Prozess-Umgebung des Dashboards (gleiches
/// Muster wie der Avatar-Cache in `internal_home.rs`). `None` heißt: keine
/// Twitch-Credentials, dann bleibt nur der lokale Bestand.
fn helix_client() -> Option<HelixClient> {
    let client_id = std::env::var("TWITCH_CLIENT_ID")
        .ok()
        .filter(|value| !value.is_empty());
    let client_secret = std::env::var("TWITCH_CLIENT_SECRET")
        .ok()
        .filter(|value| !value.is_empty());
    // Beide Flags einzeln rechnen: ein hart verdrahtetes `false` meldete auch
    // dann eine fehlende Client-ID, wenn nur das Secret fehlte. Nur die
    // Presence loggen, NIE die Werte selbst.
    let client_id_present = client_id.is_some();
    let client_secret_present = client_secret.is_some();
    let (Some(client_id), Some(client_secret)) = (client_id, client_secret) else {
        tracing::warn!(
            client_id_present,
            client_secret_present,
            "Signup-Block: Helix-Auflösung deaktiviert, TWITCH_CLIENT_ID/SECRET fehlen"
        );
        return None;
    };
    match HelixClient::new(HelixConfig::new(client_id, client_secret)) {
        Ok(client) => Some(client),
        Err(error) => {
            tracing::warn!(%error, "Signup-Block: HelixClient-Konstruktion fehlgeschlagen");
            None
        }
    }
}

/// Löst die stabile `twitch_user_id` auf: erst lokaler Bestand, dann Helix.
/// Ein unbekannter Kanal ergibt 400 und keinen Eintrag (INV-02).
///
/// Der Helix-Client kommt als Parameter statt aus [`helix_client`] direkt,
/// damit der Auflösungspfad ohne Prozess-Umgebung prüfbar ist.
async fn resolve_user_id(
    pool: &PgPool,
    login: &str,
    helix: Option<&HelixClient>,
) -> Result<String, ApiError> {
    if let Some(id) = db::resolve_user_id(pool, login).await.map_err(db_error)? {
        return Ok(id);
    }

    let Some(helix) = helix else {
        return Err(ApiError::bad_request(
            "Kanal ist lokal unbekannt und Twitch-Auflösung ist nicht verfügbar",
        ));
    };

    match helix.get_users(&[login]).await {
        Ok(users) => match users.get(login) {
            Some(user) if !user.id.trim().is_empty() => Ok(user.id.clone()),
            _ => Err(ApiError::bad_request("Twitch kennt diesen Kanal nicht")),
        },
        Err(error) => {
            tracing::error!(%login, %error, "Signup-Block: Helix get_users fehlgeschlagen");
            Err(ApiError::unavailable())
        }
    }
}

// ── Handler ───────────────────────────────────────────────────────────────────

/// `GET /twitch/api/admin/partner-signup-blocks`
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

/// `POST /twitch/api/admin/partner-signup-blocks`
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
    let helix = helix_client();
    add_entry(&pool, body, &actor, helix.as_ref()).await
}

/// Der Rumpf von [`add_handler`] ohne Axum-Extraktoren und ohne Prozess-Env.
async fn add_entry(
    pool: &PgPool,
    body: AddRequest,
    actor: &str,
    helix: Option<&HelixClient>,
) -> Result<Json<AddResponse>, ApiError> {
    let login = require_login(&body.login)?;
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

    let twitch_user_id = resolve_user_id(pool, &login, helix).await?;

    let outcome = db::add(
        pool,
        &twitch_user_id,
        &login,
        &reason,
        public_message,
        actor,
    )
    .await
    .map_err(db_error)?;

    Ok(Json(AddResponse {
        ok: true,
        login,
        twitch_user_id,
        reason,
        inserted: outcome.inserted,
        raid_blacklisted: outcome.raid_blacklisted,
        credentials_deleted: outcome.credentials_deleted,
        active_partner_paused: outcome.active_partner_paused,
    }))
}

/// `POST /twitch/api/admin/partner-signup-blocks/remove`
pub async fn remove_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Json(body): Json<RemoveRequest>,
) -> Result<impl IntoResponse, ApiError> {
    if let Some(error) = crate::auth::require_admin(&auth) {
        return Err(error);
    }

    let user_id = body
        .twitch_user_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    // Ohne gültigen Login reicht die ID allein (Kanal umbenannt oder gelöscht).
    let login = match normalize_twitch_login(&body.login) {
        Some(login) => login,
        None if user_id.is_some() => String::new(),
        None => return Err(ApiError::bad_request("invalid login")),
    };

    let outcome = db::remove(&pool, user_id, &login).await.map_err(db_error)?;

    Ok(Json(RemoveResponse {
        ok: true,
        login,
        removed: outcome.removed,
        raid_entries_removed: outcome.raid_entries_removed,
        partner_pause_cleared: outcome.partner_pause_cleared,
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

    /// Wie `db_dsn_or_skip!` in `tb_analytics::partner_signup_block`: ohne DSN
    /// wird übersprungen, aber `TB_TEST_REQUIRE_DB=1` macht daraus eine Panik,
    /// damit ein DB-Test in CI nicht still grün meldet.
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

    /// Legt das Test-Schema neu an und liefert einen Pool darauf.
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
            CREATE TABLE twitch_partner_signup_denylist (
                twitch_user_id TEXT PRIMARY KEY,
                twitch_login TEXT NOT NULL,
                reason TEXT NOT NULL,
                public_message TEXT,
                added_by TEXT NOT NULL,
                added_at TIMESTAMPTZ NOT NULL DEFAULT now(),
                partner_paused_by_block BOOLEAN NOT NULL DEFAULT false
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        // Wie die Migration 20260806060000_partner_signup_denylist.sql: der
        // Login ist zusätzlich case-insensitiv eindeutig.
        sqlx::query(
            "CREATE UNIQUE INDEX idx_partner_signup_denylist_login
                 ON twitch_partner_signup_denylist (lower(twitch_login))",
        )
        .execute(&pool)
        .await
        .unwrap();
        // Leere Quellen für `resolve_user_id`: erst damit prüft der Test den
        // echten Auflösungspfad statt eines DB-Fehlers.
        for ddl in [
            "CREATE TABLE twitch_partners (id BIGSERIAL PRIMARY KEY, twitch_login TEXT, twitch_user_id TEXT)",
            "CREATE TABLE twitch_streamer_identities (twitch_login TEXT, twitch_user_id TEXT)",
            "CREATE TABLE twitch_streamers (twitch_login TEXT, twitch_user_id TEXT)",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        pool
    }

    /// Räumt das Test-Schema wieder ab, damit die geteilte Test-DB nicht
    /// zwischen Läufen mit Leichen vollläuft.
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
                login: "beispiel".into(),
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
            Json(RemoveRequest {
                login: "beispiel".into(),
                twitch_user_id: None,
            }),
        )
        .await;
        let (status, _) = body_json(response).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn remove_ohne_login_und_ohne_id_ist_400() {
        let response = remove_handler(
            DashboardAuthLevel::admin(),
            State(lazy_pool()),
            Json(RemoveRequest {
                login: "   ".into(),
                twitch_user_id: None,
            }),
        )
        .await;
        let (status, _) = body_json(response).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn add_mit_unbekanntem_login_legt_keinen_eintrag_an() {
        let dsn = db_dsn_or_skip!();
        let schema = "t_signup_block_unbekannt";
        let pool = make_pool(&dsn, schema).await;
        // Ohne Helix-Client bleibt nur der lokale Bestand; ein unbekannter
        // Login muss 4xx geben statt login-only zu speichern. Der Client kommt
        // als Parameter, deshalb hängt der Test nicht mehr an TWITCH_CLIENT_ID
        // und kann nicht mehr still an der Prüfung vorbeilaufen.
        let response = add_entry(
            &pool,
            AddRequest {
                login: "gibtesnichtxyz".into(),
                reason: Some("test".into()),
                public_message: None,
            },
            "test",
            None,
        )
        .await;
        let (status, _) = body_json(response).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM twitch_partner_signup_denylist")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 0, "abgelehnter Add darf keinen Eintrag hinterlassen");
        drop_schema(pool, &dsn, schema).await;
    }

    #[tokio::test]
    async fn liste_gibt_eintraege_als_items_zurueck() {
        let dsn = db_dsn_or_skip!();
        let schema = "t_signup_block_liste";
        let pool = make_pool(&dsn, schema).await;
        sqlx::query(
            "INSERT INTO twitch_partner_signup_denylist
                 (twitch_user_id, twitch_login, reason, public_message, added_by)
             VALUES ('4711', 'beispiel', 'owner_decision', 'kein Interesse', 'discord:1')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let response = list_handler(DashboardAuthLevel::admin(), State(pool.clone())).await;
        let (status, body) = body_json(response).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["items"][0]["login"], "beispiel");
        assert_eq!(body["items"][0]["twitch_user_id"], "4711");
        assert_eq!(body["items"][0]["public_message"], "kein Interesse");
        assert_eq!(body["items"][0]["added_by"], "discord:1");
        drop_schema(pool, &dsn, schema).await;
    }

    /// Beweisziel: die Test-DDL trägt den UNIQUE-Index auf `lower(login)` aus
    /// der Migration wirklich mit; ohne ihn liefe der zweite Insert durch.
    #[tokio::test]
    async fn login_ist_case_insensitiv_eindeutig() {
        let dsn = db_dsn_or_skip!();
        let schema = "t_signup_block_unique";
        let pool = make_pool(&dsn, schema).await;
        sqlx::query(
            "INSERT INTO twitch_partner_signup_denylist
                 (twitch_user_id, twitch_login, reason, added_by)
             VALUES ('4711', 'beispiel', 'owner_decision', 'discord:1')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let konflikt = sqlx::query(
            "INSERT INTO twitch_partner_signup_denylist
                 (twitch_user_id, twitch_login, reason, added_by)
             VALUES ('4712', 'BeIsPiEl', 'owner_decision', 'discord:1')",
        )
        .execute(&pool)
        .await;
        assert!(
            konflikt.is_err(),
            "zweiter Login in anderer Schreibweise muss am UNIQUE-Index scheitern"
        );
        drop_schema(pool, &dsn, schema).await;
    }
}
