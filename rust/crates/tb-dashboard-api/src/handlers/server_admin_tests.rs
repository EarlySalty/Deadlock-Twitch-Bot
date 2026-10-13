//! Ende-zu-Ende-Tests der Server-Admin-Rechte-Schranke gegen eine echte,
//! isolierte PostgreSQL-Instanz (`crate::test_database`). Geprüft wird der
//! komplette Request-Fluss: Cookie → Session → Schranke → Handler → SQL.

use std::net::SocketAddr;
use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    body::Body,
    extract::ConnectInfo,
    http::{header, Method, Request, StatusCode},
    Extension, Router,
};
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;

use crate::auth::security::RateLimiter;
use crate::auth::server_admin_login::{
    DiscordGuildAdminIdentity, DiscordUserGuild, ServerAdminDiscordClient, ServerAdminLoginConfig,
    ServerAdminLoginError, PERMISSION_ADMINISTRATOR, PERMISSION_MANAGE_GUILD,
};
use crate::auth::session::{
    DashboardAuthState, ADMIN_COOKIE_NAME, PARTNER_COOKIE_NAME, SERVER_ADMIN_COOKIE_NAME,
};

const KEY: &str = "dGVzdGtleTEyMzQ1Njc4OTAxMjM0NTY3ODkwMTIzNDU=";
const HOST: &str = "dashboard.example.com";
const ORIGIN: &str = "https://dashboard.example.com";
const OWN: i64 = 1_111_111_111_111_111_111;
const FOREIGN: i64 = 2_222_222_222_222_222_222;
const NOT_INSTALLED: i64 = 3_333_333_333_333_333_333;
const MIGRATION: &str =
    include_str!("../../../../migrations/20261012120000_server_admin_guilds.sql");

struct Env {
    _db: crate::test_database::Database,
    pool: PgPool,
    state: DashboardAuthState,
}

async fn env() -> Env {
    let db = crate::test_database::Database::new().await;
    let schema = crate::auth::session::test_schema_name("server_admin");
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .execute(&db.pool)
        .await
        .unwrap();
    let opts = db
        .pool
        .connect_options()
        .as_ref()
        .clone()
        .options([("search_path", schema.as_str())]);
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .connect_with(opts)
        .await
        .unwrap();
    for ddl in [
        "CREATE TABLE dashboard_sessions (
            session_id TEXT NOT NULL PRIMARY KEY,
            session_type TEXT NOT NULL,
            payload_enc BYTEA NOT NULL,
            created_at DOUBLE PRECISION NOT NULL,
            expires_at DOUBLE PRECISION NOT NULL)",
        "CREATE TABLE twitch_streamers (
            twitch_login TEXT NOT NULL,
            twitch_user_id TEXT,
            created_at TEXT DEFAULT CURRENT_TIMESTAMP)",
    ] {
        sqlx::query(ddl).execute(&pool).await.unwrap();
    }
    // Die echte Migration, nur ohne das feste `public.`-Schema.
    sqlx::raw_sql(sqlx::AssertSqlSafe(MIGRATION.replace("public.", "")))
        .execute(&pool)
        .await
        .unwrap();
    for (id, name) in [(OWN, "Own Server"), (FOREIGN, "Foreign Server")] {
        sqlx::query("INSERT INTO discord_guilds (guild_id, name) VALUES ($1, $2)")
            .bind(id)
            .bind(name)
            .execute(&pool)
            .await
            .unwrap();
    }
    for (user, guild, login) in [
        ("101", OWN, "own_streamer"),
        ("202", FOREIGN, "foreign_streamer"),
        ("303", OWN, "shared_streamer"),
        ("303", FOREIGN, "shared_streamer"),
    ] {
        sqlx::query("INSERT INTO streamer_guilds (twitch_user_id, guild_id) VALUES ($1, $2)")
            .bind(user)
            .bind(guild)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO twitch_streamers (twitch_login, twitch_user_id) VALUES ($1, $2)")
            .bind(login)
            .bind(user)
            .execute(&pool)
            .await
            .unwrap();
    }
    let state = DashboardAuthState::new(pool.clone(), KEY.to_string());
    Env {
        _db: db,
        pool,
        state,
    }
}

fn router(env: &Env) -> Router {
    crate::build_server_admin_router(RateLimiter::new(env.pool.clone(), KEY.to_string()))
        .layer(Extension(env.state.clone()))
}

/// Guild-Admin-Session für `OWN` (Snapshot wie nach dem Discord-Login).
async fn guild_admin(env: &Env) -> (String, String) {
    let created = env
        .state
        .create_server_admin_session(42, "Mod", &[OWN])
        .await
        .unwrap();
    (created.session_id, created.csrf_token)
}

fn request(
    method: Method,
    uri: &str,
    cookie: Option<(&str, &str)>,
    csrf: Option<&str>,
    body: Option<Value>,
) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::HOST, HOST)
        .header(header::ORIGIN, ORIGIN);
    if let Some((name, value)) = cookie {
        builder = builder.header(header::COOKIE, format!("{name}={value}"));
    }
    if let Some(csrf) = csrf {
        builder = builder.header("x-csrf-token", csrf);
    }
    let body = match body {
        Some(body) => {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
            Body::from(body.to_string())
        }
        None => Body::empty(),
    };
    let mut request = builder.body(body).unwrap();
    request.extensions_mut().insert(ConnectInfo(
        "203.0.113.9:4000".parse::<SocketAddr>().unwrap(),
    ));
    request
}

async fn send(router: &Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn guild_row(pool: &PgPool, guild: i64) -> (String, bool, bool) {
    sqlx::query_as(
        "SELECT language, cross_server_raids, ai_features FROM discord_guilds WHERE guild_id = $1",
    )
    .bind(guild)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn streamer_active(pool: &PgPool, user: &str, guild: i64) -> bool {
    sqlx::query_scalar(
        "SELECT active FROM streamer_guilds WHERE twitch_user_id = $1 AND guild_id = $2",
    )
    .bind(user)
    .bind(guild)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn ohne_login_gibt_es_nichts() {
    let env = env().await;
    let app = router(&env);
    for uri in [
        "/twitch/api/server-admin/me".to_string(),
        format!("/twitch/api/server-admin/guilds/{OWN}"),
        format!("/twitch/api/server-admin/guilds/{OWN}/streamers"),
        format!("/twitch/api/server-admin/guilds/{OWN}/tags"),
    ] {
        let (status, body) = send(&app, request(Method::GET, &uri, None, None, None)).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{uri}");
        assert_eq!(body["error"], "server_admin_login_required");
    }
}

#[tokio::test]
async fn guild_admin_sieht_nur_die_eigene_guild() {
    let env = env().await;
    let app = router(&env);
    let (sid, _) = guild_admin(&env).await;
    let cookie = Some((SERVER_ADMIN_COOKIE_NAME, sid.as_str()));

    let (status, me) = send(
        &app,
        request(
            Method::GET,
            "/twitch/api/server-admin/me",
            cookie,
            None,
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["principal"], "guild_admin");
    let ids: Vec<&str> = me["guilds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["guildId"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec![OWN.to_string().as_str()]);

    let (status, own) = send(
        &app,
        request(
            Method::GET,
            &format!("/twitch/api/server-admin/guilds/{OWN}"),
            cookie,
            None,
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(own["activeStreamers"], 2);

    // Fremde, nicht installierte und kaputte Guild-IDs: einheitlich 403.
    for guild in [
        FOREIGN.to_string(),
        NOT_INSTALLED.to_string(),
        "abc".to_string(),
        "0".to_string(),
    ] {
        let (status, body) = send(
            &app,
            request(
                Method::GET,
                &format!("/twitch/api/server-admin/guilds/{guild}"),
                cookie,
                None,
                None,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{guild}");
        assert_eq!(body["error"], "guild_forbidden");
        let (status, _) = send(
            &app,
            request(
                Method::GET,
                &format!("/twitch/api/server-admin/guilds/{guild}/streamers"),
                cookie,
                None,
                None,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{guild}");
    }

    let (status, streamers) = send(
        &app,
        request(
            Method::GET,
            &format!("/twitch/api/server-admin/guilds/{OWN}/streamers"),
            cookie,
            None,
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let logins: Vec<&str> = streamers["streamers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["twitchLogin"].as_str().unwrap())
        .collect();
    assert_eq!(logins, vec!["own_streamer", "shared_streamer"]);
}

#[tokio::test]
async fn guild_admin_kann_fremde_guild_nicht_aendern() {
    let env = env().await;
    let app = router(&env);
    let (sid, csrf) = guild_admin(&env).await;
    let cookie = Some((SERVER_ADMIN_COOKIE_NAME, sid.as_str()));

    let (status, _) = send(
        &app,
        request(
            Method::PATCH,
            &format!("/twitch/api/server-admin/guilds/{FOREIGN}/settings"),
            cookie,
            Some(&csrf),
            Some(json!({"language": "de"})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(guild_row(&env.pool, FOREIGN).await.0, "en");

    let (status, _) = send(
        &app,
        request(
            Method::POST,
            &format!("/twitch/api/server-admin/guilds/{FOREIGN}/streamers/202"),
            cookie,
            Some(&csrf),
            Some(json!({"active": false})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(streamer_active(&env.pool, "202", FOREIGN).await);

    // Streamer einer fremden Guild über die eigene Guild-ID: nicht vorhanden.
    let (status, _) = send(
        &app,
        request(
            Method::POST,
            &format!("/twitch/api/server-admin/guilds/{OWN}/streamers/202"),
            cookie,
            Some(&csrf),
            Some(json!({"active": false})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(streamer_active(&env.pool, "202", FOREIGN).await);
}

#[tokio::test]
async fn guild_admin_darf_erlaubte_einstellungen_aendern() {
    let env = env().await;
    let app = router(&env);
    let (sid, csrf) = guild_admin(&env).await;
    let cookie = Some((SERVER_ADMIN_COOKIE_NAME, sid.as_str()));

    let (status, body) = send(
        &app,
        request(
            Method::PATCH,
            &format!("/twitch/api/server-admin/guilds/{OWN}/settings"),
            cookie,
            Some(&csrf),
            Some(json!({
                "language": "de",
                "crossServerRaids": false,
                "autoPartnerDefault": false,
                "announceChannelId": "123456789012345678",
                "pingRoleId": "223456789012345678",
                "promoInviteUrl": "discord.gg/deadlock",
            })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["language"], "de");
    assert_eq!(body["crossServerRaids"], false);
    assert_eq!(body["autoPartnerDefault"], false);
    assert_eq!(body["announceChannelId"], "123456789012345678");
    assert_eq!(body["promoInviteUrl"], "https://discord.gg/deadlock");
    // Nicht mitgeschickte Felder bleiben, fremde Guild unverändert.
    assert_eq!(body["streamerRoleId"], Value::Null);
    assert_eq!(
        guild_row(&env.pool, FOREIGN).await,
        ("en".to_string(), true, false)
    );

    // Kanal leeren.
    let (status, body) = send(
        &app,
        request(
            Method::PATCH,
            &format!("/twitch/api/server-admin/guilds/{OWN}/settings"),
            cookie,
            Some(&csrf),
            Some(json!({"announceChannelId": null})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["announceChannelId"], Value::Null);
    assert_eq!(body["pingRoleId"], "223456789012345678");

    // Streamer der eigenen Guild deaktivieren: nur diese Zeile ändert sich.
    let (status, _) = send(
        &app,
        request(
            Method::POST,
            &format!("/twitch/api/server-admin/guilds/{OWN}/streamers/303"),
            cookie,
            Some(&csrf),
            Some(json!({"active": false})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!streamer_active(&env.pool, "303", OWN).await);
    assert!(streamer_active(&env.pool, "303", FOREIGN).await);

    let audit: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM server_admin_audit_log WHERE guild_id = $1 AND actor = 'discord:42'",
    )
    .bind(OWN)
    .fetch_one(&env.pool)
    .await
    .unwrap();
    assert_eq!(audit, 3);
}

#[tokio::test]
async fn gesperrte_bereiche_sind_ausdruecklich_verboten() {
    let env = env().await;
    let app = router(&env);
    let (sid, csrf) = guild_admin(&env).await;
    let cookie = Some((SERVER_ADMIN_COOKIE_NAME, sid.as_str()));

    for (field, area) in [
        ("aiFeatures", "ai_features"),
        ("ai_features", "ai_features"),
        ("tags", "tags"),
        ("globalBans", "global_bans"),
        ("blacklist", "blacklist"),
        ("billing", "billing"),
    ] {
        // Auch zusammen mit einem erlaubten Feld wird nichts geschrieben.
        let (status, body) = send(
            &app,
            request(
                Method::PATCH,
                &format!("/twitch/api/server-admin/guilds/{OWN}/settings"),
                cookie,
                Some(&csrf),
                Some(json!({"language": "de", field: true})),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{field}");
        assert_eq!(body["error"], "area_forbidden");
        assert_eq!(body["area"], area);
    }
    assert_eq!(
        guild_row(&env.pool, OWN).await,
        ("en".to_string(), true, false)
    );

    for segment in ["tags", "global-bans", "blacklist", "billing", "ai-features"] {
        for method in [Method::GET, Method::POST, Method::DELETE] {
            let (status, body) = send(
                &app,
                request(
                    method.clone(),
                    &format!("/twitch/api/server-admin/guilds/{OWN}/{segment}"),
                    cookie,
                    Some(&csrf),
                    None,
                ),
            )
            .await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{method} {segment}");
            assert_eq!(body["error"], "area_forbidden");
        }
    }

    // Unbekannte Felder: 400, nichts geschrieben.
    let (status, body) = send(
        &app,
        request(
            Method::PATCH,
            &format!("/twitch/api/server-admin/guilds/{OWN}/settings"),
            cookie,
            Some(&csrf),
            Some(json!({"guildId": "1"})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "unknown_field");
}

#[tokio::test]
async fn schreiben_braucht_csrf_token_und_gleichen_ursprung() {
    let env = env().await;
    let app = router(&env);
    let (sid, csrf) = guild_admin(&env).await;
    let cookie = Some((SERVER_ADMIN_COOKIE_NAME, sid.as_str()));
    let uri = format!("/twitch/api/server-admin/guilds/{OWN}/settings");

    let (status, body) = send(
        &app,
        request(
            Method::PATCH,
            &uri,
            cookie,
            None,
            Some(json!({"language": "de"})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"], "invalid_csrf");
    let (status, _) = send(
        &app,
        request(
            Method::PATCH,
            &uri,
            cookie,
            Some("falsch"),
            Some(json!({"language": "de"})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let mut cross = request(
        Method::PATCH,
        &uri,
        cookie,
        Some(&csrf),
        Some(json!({"language": "de"})),
    );
    cross
        .headers_mut()
        .insert(header::ORIGIN, "https://evil.example".parse().unwrap());
    let (status, _) = send(&app, cross).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(guild_row(&env.pool, OWN).await.0, "en");
}

#[tokio::test]
async fn server_admin_session_ist_kein_globaler_admin_und_umgekehrt() {
    let env = env().await;
    let (sid, _) = guild_admin(&env).await;

    // Die Server-Admin-Session unter einem Admin- oder Partner-Cookie gilt nicht.
    for cookie in [ADMIN_COOKIE_NAME, PARTNER_COOKIE_NAME] {
        assert_eq!(env.state.load_admin_session(&sid).await.unwrap(), None);
        let app = router(&env);
        let (status, _) = send(
            &app,
            request(
                Method::GET,
                "/twitch/api/server-admin/me",
                Some((cookie, sid.as_str())),
                None,
                None,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{cookie}");
    }

    // Der DashboardAuthLevel-Extractor (alle globalen Admin-/Partner-Routen)
    // erkennt die Server-Admin-Session nicht.
    use axum::extract::FromRequestParts;
    let mut parts = request(
        Method::GET,
        "/twitch/api/v2/auth-status",
        Some((SERVER_ADMIN_COOKIE_NAME, sid.as_str())),
        None,
        None,
    )
    .into_parts()
    .0;
    parts.extensions.insert(env.state.clone());
    let Ok(level) =
        crate::auth::level::DashboardAuthLevel::from_request_parts(&mut parts, &()).await;
    assert_eq!(level, crate::auth::level::DashboardAuthLevel::None);
}

/// Der globale Admin-Bereich (Betreiber-Session, Admin-Modus) ist kein Weg in
/// das Server-Admin-Dashboard: Das Panel kennt nur den eigenen Discord-Login.
#[tokio::test]
async fn globale_admin_session_oeffnet_das_server_admin_dashboard_nicht() {
    let env = env().await;
    let admin = env.state.create_admin_session("1", "owner").await.unwrap();
    let app = router(&env).layer(Extension(tb_http_core::AuthLevel::Admin));

    for (method, uri, body) in [
        (Method::GET, "/twitch/api/server-admin/me".to_string(), None),
        (
            Method::GET,
            format!("/twitch/api/server-admin/guilds/{OWN}"),
            None,
        ),
        (
            Method::GET,
            format!("/twitch/api/server-admin/guilds/{FOREIGN}/streamers"),
            None,
        ),
        (
            Method::PATCH,
            format!("/twitch/api/server-admin/guilds/{OWN}/settings"),
            Some(json!({"language": "de"})),
        ),
    ] {
        let (status, body) = send(
            &app,
            request(
                method.clone(),
                &uri,
                Some((ADMIN_COOKIE_NAME, admin.session_id.as_str())),
                Some(&admin.csrf_token),
                body,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {uri}");
        assert_eq!(body["error"], "server_admin_login_required");
    }
    assert_eq!(guild_row(&env.pool, OWN).await.0, "en");
}

struct FakeDiscord(DiscordGuildAdminIdentity);

#[async_trait]
impl ServerAdminDiscordClient for FakeDiscord {
    async fn exchange_code(
        &self,
        code: &str,
        _redirect_uri: &str,
    ) -> Result<DiscordGuildAdminIdentity, ServerAdminLoginError> {
        if code == "good" {
            Ok(self.0.clone())
        } else {
            Err(ServerAdminLoginError)
        }
    }
}

#[tokio::test]
async fn login_nimmt_nur_guilds_mit_server_verwalten_und_installiertem_bot() {
    let env = env().await;
    let identity = DiscordGuildAdminIdentity {
        user_id: 77,
        display_name: "Owner".into(),
        guilds: vec![
            DiscordUserGuild {
                id: OWN,
                name: "Own".into(),
                owner: false,
                permissions: PERMISSION_MANAGE_GUILD,
            },
            // Mitglied ohne Rechte auf der fremden Guild.
            DiscordUserGuild {
                id: FOREIGN,
                name: "Foreign".into(),
                owner: false,
                permissions: 1 << 11,
            },
            // Admin, aber Bot nicht installiert.
            DiscordUserGuild {
                id: NOT_INSTALLED,
                name: "Other".into(),
                owner: false,
                permissions: PERMISSION_ADMINISTRATOR,
            },
        ],
    };
    let config = ServerAdminLoginConfig {
        client_id: "cid".into(),
        redirect_uri: "https://dashboard.example.com/twitch/server-admin/auth/callback".into(),
        cookie_secure: true,
        client: Arc::new(FakeDiscord(identity)),
    };
    let app = router(&env).layer(Extension(config));

    let response = app
        .clone()
        .oneshot(request(
            Method::GET,
            "/twitch/server-admin/auth/login",
            None,
            None,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let location = response.headers()[header::LOCATION]
        .to_str()
        .unwrap()
        .to_string();
    assert!(location.starts_with("https://discord.com/oauth2/authorize"));
    let state = url::Url::parse(&location)
        .unwrap()
        .query_pairs()
        .find(|(k, _)| k == "state")
        .unwrap()
        .1
        .to_string();

    // Ohne passenden State-Cookie (Login-CSRF) kein Login.
    let (status, _) = send(
        &app,
        request(
            Method::GET,
            &format!("/twitch/server-admin/auth/callback?code=good&state={state}"),
            Some(("tb_server_admin_oauth", "other")),
            None,
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let response = app
        .clone()
        .oneshot(request(
            Method::GET,
            &format!("/twitch/server-admin/auth/callback?code=good&state={state}"),
            Some(("tb_server_admin_oauth", state.as_str())),
            None,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let session = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find_map(|v| v.strip_prefix(&format!("{SERVER_ADMIN_COOKIE_NAME}=")))
        .and_then(|v| v.split(';').next())
        .unwrap()
        .to_string();
    let loaded = env
        .state
        .load_server_admin_session(&session)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.managed_guild_ids, vec![OWN]);
    assert_eq!(loaded.discord_user_id, "77");

    // State ist verbraucht (Replay).
    let (status, _) = send(
        &app,
        request(
            Method::GET,
            &format!("/twitch/server-admin/auth/callback?code=good&state={state}"),
            Some(("tb_server_admin_oauth", state.as_str())),
            None,
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
