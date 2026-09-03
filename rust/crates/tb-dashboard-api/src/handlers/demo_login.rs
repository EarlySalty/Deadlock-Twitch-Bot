use axum::{
    body::Bytes,
    http::{
        header::{HeaderValue, CACHE_CONTROL, CONTENT_TYPE, SET_COOKIE},
        StatusCode,
    },
    response::{IntoResponse, Redirect, Response},
    Extension,
};
use tracing::{info, warn};

use crate::auth::session::{
    build_session_cookie, DashboardAuthState, PartnerSession, SameSite, PARTNER_COOKIE_NAME,
    SESSION_CREATE_TTL_SECS,
};

const DASHBOARD_PATH: &str = "/twitch/dashboard";

const MAX_CONCURRENT_VERIFY: usize = 4;

static VERIFY_SLOTS: std::sync::LazyLock<std::sync::Arc<tokio::sync::Semaphore>> =
    std::sync::LazyLock::new(|| {
        std::sync::Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_VERIFY))
    });

pub struct DemoLoginConfig {
    pub username: String,
    pub password_hash: String,
    pub twitch_user_id: String,
    pub display_name: String,
    pub cookie_secure: bool,
}

fn non_empty_env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub fn demo_login_config_from_env() -> Option<DemoLoginConfig> {
    let username = non_empty_env("TWITCH_DEMO_LOGIN_USER")?;
    let password_hash = non_empty_env("TWITCH_DEMO_LOGIN_PASSWORD_HASH")?;
    let twitch_user_id = non_empty_env("TWITCH_DEMO_LOGIN_TWITCH_USER_ID")?;
    let display_name = non_empty_env("TWITCH_DEMO_LOGIN_DISPLAY_NAME").unwrap_or_default();
    let cookie_secure = std::env::var("TB_DASHBOARD_COOKIE_INSECURE").as_deref() != Ok("1");
    Some(DemoLoginConfig {
        username,
        password_hash,
        twitch_user_id,
        display_name,
        cookie_secure,
    })
}

fn no_store(mut response: Response) -> Response {
    response.headers_mut().insert(
        CACHE_CONTROL,
        HeaderValue::from_static("no-store, max-age=0"),
    );
    response
}

fn redirect_with_cookie(location: &str, cookie: &str) -> Response {
    let mut response = Redirect::to(location).into_response();
    match HeaderValue::from_str(cookie) {
        Ok(value) => {
            response.headers_mut().append(SET_COOKIE, value);
        }
        Err(error) => {
            warn!(%error, "Demo-Login: Session-Cookie konnte nicht gesetzt werden");
        }
    }
    no_store(response)
}

fn form_value(body: &Bytes, key: &str) -> Option<String> {
    let text = String::from_utf8_lossy(body);
    for (name, value) in url::form_urlencoded::parse(text.as_bytes()) {
        if name == key {
            return Some(value.into_owned());
        }
    }
    None
}

fn verify_password(password: &str, phc_hash: &str) -> bool {
    use argon2::{Argon2, PasswordHash, PasswordVerifier};
    match PasswordHash::new(phc_hash) {
        Ok(parsed) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(error) => {
            warn!(%error, "Demo-Login: konfigurierter Passwort-Hash ist kein gueltiger PHC-String");
            false
        }
    }
}

async fn resolve_active_partner_by_user_id(
    pool: &sqlx::PgPool,
    user_id: &str,
) -> Result<Option<(String, String)>, sqlx::Error> {
    let user_id = user_id.trim();
    if user_id.is_empty() {
        return Ok(None);
    }
    let row: Option<(String, String)> = sqlx::query_as(
        r#"
        SELECT p.twitch_login, p.twitch_user_id
        FROM twitch_partners p
        WHERE LOWER(COALESCE(p.technical_pause_reason, '')) <> 'blocked'
          AND COALESCE(p.status, '') = 'active'
          AND p.twitch_user_id = $1
        ORDER BY COALESCE(p.departnered_at, p.admin_archived_at, p.partnered_at) DESC
        LIMIT 1
        "#,
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn get_handler() -> Response {
    if demo_login_config_from_env().is_none() {
        return no_store(StatusCode::NOT_FOUND.into_response());
    }
    no_store(
        (
            [(CONTENT_TYPE, "text/html; charset=utf-8")],
            LOGIN_PAGE_HTML,
        )
            .into_response(),
    )
}

pub async fn post_handler(state: Option<Extension<DashboardAuthState>>, body: Bytes) -> Response {
    let Some(config) = demo_login_config_from_env() else {
        return no_store(StatusCode::NOT_FOUND.into_response());
    };

    let username = form_value(&body, "username").unwrap_or_default();
    let password = form_value(&body, "password").unwrap_or_default();

    let user_ok =
        tb_crypto::constant_time_eq(username.trim().as_bytes(), config.username.as_bytes());
    let permit = match VERIFY_SLOTS.clone().try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => {
            warn!("AUDIT demo login throttled (Verify-Slots erschoepft)");
            return no_store(
                (
                    StatusCode::TOO_MANY_REQUESTS,
                    "Zu viele Anfragen. Bitte spaeter erneut versuchen.",
                )
                    .into_response(),
            );
        }
    };
    let password_hash = config.password_hash.clone();
    let pass_ok = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        verify_password(&password, &password_hash)
    })
    .await
    .unwrap_or(false);
    if !(user_ok && pass_ok) {
        warn!("AUDIT demo login failed (Nutzername oder Passwort falsch)");
        return no_store((StatusCode::UNAUTHORIZED, "Anmeldung fehlgeschlagen.").into_response());
    }

    let Some(Extension(state)) = state else {
        return no_store(
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "Login konnte gerade nicht abgeschlossen werden. Bitte erneut versuchen.",
            )
                .into_response(),
        );
    };

    let partner =
        match resolve_active_partner_by_user_id(state.pool(), &config.twitch_user_id).await {
            Ok(Some((twitch_login, twitch_user_id))) => PartnerSession {
                twitch_login,
                twitch_user_id,
                display_name: String::new(),
            },
            Ok(None) => {
                warn!(
                    twitch_user_id = %config.twitch_user_id,
                    "AUDIT demo login denied (kein Partner fuer konfigurierte User-ID)"
                );
                return no_store(
                    (
                        StatusCode::FORBIDDEN,
                        "Kein Zugriff: Das Konto ist nicht als Streamer-Partner freigegeben.",
                    )
                        .into_response(),
                );
            }
            Err(error) => {
                warn!(%error, "Demo-Login: Partner-Lookup fehlgeschlagen");
                return no_store(
                    (
                        StatusCode::SERVICE_UNAVAILABLE,
                        "Login konnte gerade nicht abgeschlossen werden. Bitte erneut versuchen.",
                    )
                        .into_response(),
                );
            }
        };

    let display_name = if config.display_name.is_empty() {
        partner.twitch_login.clone()
    } else {
        config.display_name.clone()
    };

    let session = match state
        .create_partner_session(
            &partner.twitch_login,
            &partner.twitch_user_id,
            &display_name,
        )
        .await
    {
        Ok(session) => session,
        Err(error) => {
            warn!(%error, "Demo-Login: Session-Erstellung fehlgeschlagen");
            return no_store(
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Login konnte gerade nicht abgeschlossen werden. Bitte erneut versuchen.",
                )
                    .into_response(),
            );
        }
    };

    info!(
        twitch_login = %partner.twitch_login,
        twitch_user_id = %partner.twitch_user_id,
        "AUDIT demo login success"
    );

    let cookie = build_session_cookie(
        PARTNER_COOKIE_NAME,
        &session.session_id,
        config.cookie_secure,
        SameSite::Lax,
        SESSION_CREATE_TTL_SECS,
    );
    redirect_with_cookie(DASHBOARD_PATH, &cookie)
}

const LOGIN_PAGE_HTML: &str = r#"<!DOCTYPE html>
<html lang="de">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="robots" content="noindex, nofollow">
<title>Anmeldung, Deutsche Deadlock Community</title>
<style>
  :root {
    --gold: #f0b429;
    --gold-soft: rgba(240, 180, 41, 0.16);
    --bg: #0e0e12;
    --panel: #17171f;
    --border: #2a2a36;
    --text: #efeff5;
    --muted: #9a9aa8;
  }
  * { box-sizing: border-box; }
  body {
    margin: 0;
    min-height: 100vh;
    display: flex;
    align-items: center;
    justify-content: center;
    background: radial-gradient(1200px 600px at 50% -10%, #1d1a10 0%, var(--bg) 60%);
    color: var(--text);
    font-family: "Inter", "Segoe UI", system-ui, -apple-system, sans-serif;
    padding: 24px;
  }
  .card {
    width: 100%;
    max-width: 380px;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 16px;
    padding: 32px 28px;
    box-shadow: 0 24px 60px rgba(0, 0, 0, 0.45);
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-bottom: 6px;
  }
  .brand-dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: var(--gold);
    box-shadow: 0 0 14px var(--gold);
  }
  .brand-name {
    font-size: 13px;
    letter-spacing: 0.14em;
    text-transform: uppercase;
    color: var(--muted);
  }
  h1 {
    font-size: 22px;
    margin: 12px 0 4px;
  }
  .sub {
    margin: 0 0 22px;
    font-size: 14px;
    color: var(--muted);
    line-height: 1.5;
  }
  .sub .en {
    display: block;
    margin-top: 6px;
    font-size: 12px;
    color: #7f7f8c;
  }
  label {
    display: block;
    font-size: 13px;
    color: var(--muted);
    margin: 14px 0 6px;
  }
  input {
    width: 100%;
    padding: 12px 14px;
    background: #0f0f16;
    border: 1px solid var(--border);
    border-radius: 10px;
    color: var(--text);
    font-size: 15px;
  }
  input:focus {
    outline: none;
    border-color: var(--gold);
    box-shadow: 0 0 0 3px var(--gold-soft);
  }
  button {
    width: 100%;
    margin-top: 24px;
    padding: 13px 16px;
    border: none;
    border-radius: 10px;
    background: var(--gold);
    color: #1a1405;
    font-size: 15px;
    font-weight: 700;
    cursor: pointer;
  }
  button:hover { filter: brightness(1.06); }
  .foot {
    margin-top: 18px;
    font-size: 12px;
    color: #6f6f7c;
    text-align: center;
  }
</style>
</head>
<body>
  <main class="card">
    <div class="brand">
      <span class="brand-dot"></span>
      <span class="brand-name">Deadlock Community</span>
    </div>
    <h1>Anmeldung</h1>
    <p class="sub">
      Melde dich mit deinem Pruefer-Zugang an, um das Streamer-Dashboard zu sehen.
      <span class="en">Sign in with your reviewer account to open the streamer dashboard.</span>
    </p>
    <form method="post" action="/twitch/auth/google">
      <label for="username">Nutzername</label>
      <input id="username" name="username" type="text" autocomplete="username" autofocus required>
      <label for="password">Passwort</label>
      <input id="password" name="password" type="password" autocomplete="current-password" required>
      <button type="submit">Anmelden</button>
    </form>
    <p class="foot">Deutsche Deadlock Community</p>
  </main>
</body>
</html>"#;

#[cfg(test)]
mod unit_tests {
    use super::*;
    use argon2::password_hash::SaltString;
    use argon2::{Argon2, PasswordHasher};

    fn make_hash(password: &str) -> String {
        let salt = SaltString::from_b64("dGVzdHNhbHR0ZXN0c2E").unwrap();
        Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .unwrap()
            .to_string()
    }

    #[test]
    fn verify_password_akzeptiert_richtiges_und_lehnt_falsches_ab() {
        let good = tb_crypto::random_urlsafe_token(12);
        let bad = tb_crypto::random_urlsafe_token(12);
        let hash = make_hash(&good);
        assert!(verify_password(&good, &hash));
        assert!(!verify_password(&bad, &hash));
        assert!(!verify_password("", &hash));
    }

    #[test]
    fn verify_password_bei_kaputtem_hash_false() {
        let irgendwas = tb_crypto::random_urlsafe_token(8);
        assert!(!verify_password(&irgendwas, "kein-phc-string"));
    }

    #[test]
    fn form_value_liest_urlencoded() {
        let body = Bytes::from(format!("feld_a={}&feld_b=a%20b%26c", "pruefer"));
        assert_eq!(form_value(&body, "feld_a").as_deref(), Some("pruefer"));
        assert_eq!(form_value(&body, "feld_b").as_deref(), Some("a b&c"));
        assert_eq!(form_value(&body, "fehlt"), None);
    }

    #[test]
    fn login_seite_hat_kein_inline_script() {
        assert!(!LOGIN_PAGE_HTML.contains("<script"));
        assert!(LOGIN_PAGE_HTML.contains("action=\"/twitch/auth/google\""));
    }
}

#[cfg(test)]
mod route_tests {
    use super::*;
    use crate::auth::security::{rate_limit_middleware, RateLimitLayerConfig, RateLimiter};
    use argon2::password_hash::SaltString;
    use argon2::{Argon2, PasswordHasher};
    use axum::body::Body;
    use axum::http::Request;
    use axum::routing::get;
    use axum::Router;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use sqlx::PgPool;
    use std::str::FromStr;
    use tower::ServiceExt;

    static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    const TEST_USER: &str = "google-pruefer";
    const TEST_USER_ID: &str = "77001122";

    static TEST_PASSWORD: std::sync::LazyLock<String> =
        std::sync::LazyLock::new(|| tb_crypto::random_urlsafe_token(12));
    static WRONG_PASSWORD: std::sync::LazyLock<String> =
        std::sync::LazyLock::new(|| tb_crypto::random_urlsafe_token(12));

    fn login_body(user: &str, password: &str) -> String {
        format!("username={user}&password={password}")
    }

    fn test_fernet_key() -> String {
        "dGVzdGtleTEyMzQ1Njc4OTAxMjM0NTY3ODkwMTIzNDU=".to_string()
    }

    fn make_hash(password: &str) -> String {
        let salt = SaltString::from_b64("cnVudGltZXNhbHRydW50aQ").unwrap();
        Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .unwrap()
            .to_string()
    }

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let Some(dsn) = std::env::var("TB_TEST_DATABASE_URL").ok() else {
            if std::env::var("TB_TEST_REQUIRE_DB").as_deref() == Ok("1") {
                panic!("TB_TEST_REQUIRE_DB=1 gesetzt, aber TB_TEST_DATABASE_URL fehlt");
            }
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return None;
        };
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
            .max_connections(4)
            .connect_with(opts)
            .await
            .unwrap();
        sqlx::query(
            r#"CREATE TABLE dashboard_sessions (
                session_id TEXT PRIMARY KEY, session_type TEXT NOT NULL,
                payload_enc BYTEA NOT NULL, created_at DOUBLE PRECISION NOT NULL,
                expires_at DOUBLE PRECISION NOT NULL)"#,
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            r#"CREATE TABLE twitch_partners (
                twitch_login TEXT, twitch_user_id TEXT, status TEXT,
                technical_pause_reason TEXT, manual_partner_opt_out INTEGER,
                departnered_at TEXT, admin_archived_at TEXT,
                partnered_at TEXT)"#,
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_partners (twitch_login, twitch_user_id, status)
             VALUES ('pruefkonto', $1, 'active')",
        )
        .bind(TEST_USER_ID)
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    fn set_secrets() {
        std::env::set_var("TWITCH_DEMO_LOGIN_USER", TEST_USER);
        std::env::set_var("TWITCH_DEMO_LOGIN_PASSWORD_HASH", make_hash(&TEST_PASSWORD));
        std::env::set_var("TWITCH_DEMO_LOGIN_TWITCH_USER_ID", TEST_USER_ID);
    }

    fn clear_secrets() {
        std::env::remove_var("TWITCH_DEMO_LOGIN_USER");
        std::env::remove_var("TWITCH_DEMO_LOGIN_PASSWORD_HASH");
        std::env::remove_var("TWITCH_DEMO_LOGIN_TWITCH_USER_ID");
        std::env::remove_var("TWITCH_DEMO_LOGIN_DISPLAY_NAME");
    }

    fn app(pool: PgPool, state: DashboardAuthState, max_requests: u32) -> Router {
        let limiter = RateLimiter::new(pool.clone(), test_fernet_key());
        let rl = RateLimitLayerConfig::new(limiter, "demo_login", max_requests, 60);
        Router::new()
            .route(
                "/twitch/auth/google",
                get(super::get_handler).post(super::post_handler).layer(
                    axum::middleware::from_fn_with_state(rl, rate_limit_middleware),
                ),
            )
            .layer(Extension(state))
            .with_state(pool)
    }

    fn post_req(body: impl Into<Body>) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri("/twitch/auth/google")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(body.into())
            .unwrap()
    }

    fn get_req() -> Request<Body> {
        Request::builder()
            .method("GET")
            .uri("/twitch/auth/google")
            .body(Body::empty())
            .unwrap()
    }

    fn cookie_sid(resp: &Response) -> Option<String> {
        let cookie = resp.headers().get(SET_COOKIE)?.to_str().ok()?;
        cookie
            .strip_prefix(&format!("{PARTNER_COOKIE_NAME}="))
            .and_then(|s| s.split(';').next())
            .map(|s| s.to_string())
    }

    #[tokio::test]
    async fn ohne_secrets_get_und_post_404() {
        let Some(pool) = make_pool("t_demo_404").await else {
            return;
        };
        let _guard = ENV_LOCK.lock().await;
        clear_secrets();
        let state = DashboardAuthState::new(pool.clone(), test_fernet_key());

        let g = app(pool.clone(), state.clone(), 100)
            .oneshot(get_req())
            .await
            .unwrap();
        assert_eq!(g.status(), StatusCode::NOT_FOUND);

        let p = app(pool.clone(), state, 100)
            .oneshot(post_req(login_body("x", &WRONG_PASSWORD)))
            .await
            .unwrap();
        assert_eq!(p.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn falsches_passwort_401_keine_session() {
        let Some(pool) = make_pool("t_demo_401").await else {
            return;
        };
        let _guard = ENV_LOCK.lock().await;
        set_secrets();
        let state = DashboardAuthState::new(pool.clone(), test_fernet_key());

        let resp = app(pool.clone(), state, 100)
            .oneshot(post_req(login_body(TEST_USER, &WRONG_PASSWORD)))
            .await
            .unwrap();
        clear_secrets();

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        assert!(resp.headers().get(SET_COOKIE).is_none());
        let rows: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM dashboard_sessions WHERE session_type = 'twitch'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(rows, 0);
    }

    #[tokio::test]
    async fn unbekannter_nutzername_401() {
        let Some(pool) = make_pool("t_demo_unknown_user").await else {
            return;
        };
        let _guard = ENV_LOCK.lock().await;
        set_secrets();
        let state = DashboardAuthState::new(pool.clone(), test_fernet_key());

        let resp = app(pool.clone(), state, 100)
            .oneshot(post_req(login_body("fremd", &TEST_PASSWORD)))
            .await
            .unwrap();
        clear_secrets();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn rate_limit_greift_nach_n_versuchen() {
        let Some(pool) = make_pool("t_demo_rl").await else {
            return;
        };
        let _guard = ENV_LOCK.lock().await;
        set_secrets();
        let state = DashboardAuthState::new(pool.clone(), test_fernet_key());
        let router = app(pool.clone(), state, 3);

        let mut saw_429 = false;
        let mut saw_401 = false;
        for _ in 0..5 {
            let resp = router
                .clone()
                .oneshot(post_req(login_body(TEST_USER, &WRONG_PASSWORD)))
                .await
                .unwrap();
            match resp.status() {
                StatusCode::UNAUTHORIZED => saw_401 = true,
                StatusCode::TOO_MANY_REQUESTS => saw_429 = true,
                other => panic!("unerwarteter Status {other}"),
            }
        }
        clear_secrets();
        assert!(saw_401, "vor dem Limit muss 401 kommen");
        assert!(saw_429, "nach dem Limit muss 429 kommen");
    }

    #[tokio::test]
    async fn richtiges_passwort_praegt_session_mit_konfigurierter_user_id() {
        let Some(pool) = make_pool("t_demo_ok").await else {
            return;
        };
        let _guard = ENV_LOCK.lock().await;
        set_secrets();
        let state = DashboardAuthState::new(pool.clone(), test_fernet_key());

        let resp = app(pool.clone(), state.clone(), 100)
            .oneshot(post_req(login_body(TEST_USER, &TEST_PASSWORD)))
            .await
            .unwrap();
        clear_secrets();

        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        let location = resp
            .headers()
            .get(axum::http::header::LOCATION)
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(location, DASHBOARD_PATH);
        let sid = cookie_sid(&resp).expect("Set-Cookie twitch_dash_session fehlt");

        let session = state.load_partner_session(&sid).await.unwrap().unwrap();
        assert_eq!(session.twitch_user_id, TEST_USER_ID);
        assert_eq!(session.twitch_login, "pruefkonto");
    }

    #[tokio::test]
    async fn fremde_user_id_im_request_aendert_bindung_nicht() {
        let Some(pool) = make_pool("t_demo_binding").await else {
            return;
        };
        let _guard = ENV_LOCK.lock().await;
        set_secrets();
        let state = DashboardAuthState::new(pool.clone(), test_fernet_key());

        let resp = app(pool.clone(), state.clone(), 100)
            .oneshot(post_req(format!(
                "{}&twitch_user_id=99998888&twitch_login=angreifer",
                login_body(TEST_USER, &TEST_PASSWORD)
            )))
            .await
            .unwrap();
        clear_secrets();

        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        assert!(cookie_sid(&resp).is_some(), "Set-Cookie fehlt");

        let payload_enc: Vec<u8> = sqlx::query_scalar(
            "SELECT payload_enc FROM dashboard_sessions WHERE session_type = 'twitch'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let token = String::from_utf8(payload_enc).unwrap();
        let plaintext = crate::auth::fernet::decrypt(&test_fernet_key(), &token, None).unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&plaintext).unwrap();
        let bound_id = payload
            .get("twitch_user_id")
            .and_then(|v| v.as_str())
            .unwrap();
        let bound_login = payload
            .get("twitch_login")
            .and_then(|v| v.as_str())
            .unwrap();

        assert_eq!(
            bound_id, TEST_USER_ID,
            "gepraegte Session muss an die konfigurierte User-ID gebunden sein, nie an die aus dem Formular"
        );
        assert_ne!(bound_id, "99998888");
        assert_eq!(bound_login, "pruefkonto");
        assert_ne!(bound_login, "angreifer");
    }

    #[tokio::test]
    async fn keine_aktive_partnerzeile_fuer_konfigurierte_id_ergibt_403() {
        let Some(pool) = make_pool("t_demo_poison").await else {
            return;
        };
        let _guard = ENV_LOCK.lock().await;
        set_secrets();
        sqlx::query("DELETE FROM twitch_partners")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO twitch_partners (twitch_login, twitch_user_id, status)
             VALUES ('', '00000000', 'active')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let state = DashboardAuthState::new(pool.clone(), test_fernet_key());

        let resp = app(pool.clone(), state, 100)
            .oneshot(post_req(login_body(TEST_USER, &TEST_PASSWORD)))
            .await
            .unwrap();
        clear_secrets();

        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        assert!(resp.headers().get(SET_COOKIE).is_none());
        let rows: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM dashboard_sessions WHERE session_type = 'twitch'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(rows, 0);
    }

    #[tokio::test]
    async fn nur_konfigurierte_id_wird_gebunden_trotz_leerem_login_partner() {
        let Some(pool) = make_pool("t_demo_leerlogin").await else {
            return;
        };
        let _guard = ENV_LOCK.lock().await;
        set_secrets();
        sqlx::query("DELETE FROM twitch_partners")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO twitch_partners (twitch_login, twitch_user_id, status, partnered_at)
             VALUES ('', '55550000', 'active', NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_partners (twitch_login, twitch_user_id, status, partnered_at)
             VALUES ('pruefkonto', $1, 'active', '2020-01-01')",
        )
        .bind(TEST_USER_ID)
        .execute(&pool)
        .await
        .unwrap();
        let state = DashboardAuthState::new(pool.clone(), test_fernet_key());

        let resp = app(pool.clone(), state, 100)
            .oneshot(post_req(login_body(TEST_USER, &TEST_PASSWORD)))
            .await
            .unwrap();
        clear_secrets();

        assert_eq!(
            resp.status(),
            StatusCode::SEE_OTHER,
            "Login des konfigurierten Kontos darf nicht an einer Leer-Login-Partnerzeile scheitern"
        );
        let payload_enc: Vec<u8> = sqlx::query_scalar(
            "SELECT payload_enc FROM dashboard_sessions WHERE session_type = 'twitch'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let token = String::from_utf8(payload_enc).unwrap();
        let plaintext = crate::auth::fernet::decrypt(&test_fernet_key(), &token, None).unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&plaintext).unwrap();
        let bound_id = payload
            .get("twitch_user_id")
            .and_then(|v| v.as_str())
            .unwrap();
        let bound_login = payload
            .get("twitch_login")
            .and_then(|v| v.as_str())
            .unwrap();
        assert_eq!(bound_id, TEST_USER_ID);
        assert_eq!(bound_login, "pruefkonto");
    }

    #[tokio::test]
    async fn build_auth_router_get_offen_post_limitiert() {
        let Some(pool) = make_pool("t_demo_wiring").await else {
            return;
        };
        let _guard = ENV_LOCK.lock().await;
        clear_secrets();
        let limiter = RateLimiter::new(pool.clone(), test_fernet_key());
        let router = crate::build_auth_router(limiter);

        for _ in 0..15 {
            let resp = router.clone().oneshot(get_req()).await.unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::NOT_FOUND,
                "GET auf /twitch/auth/google darf nicht ratenbegrenzt sein"
            );
        }

        let mut saw_404 = false;
        let mut saw_429 = false;
        for _ in 0..15 {
            let resp = router
                .clone()
                .oneshot(post_req(login_body("x", &WRONG_PASSWORD)))
                .await
                .unwrap();
            match resp.status() {
                StatusCode::NOT_FOUND => saw_404 = true,
                StatusCode::TOO_MANY_REQUESTS => saw_429 = true,
                other => panic!("unerwarteter Status {other}"),
            }
        }
        assert!(saw_404, "vor dem Limit erreicht der POST den Handler");
        assert!(saw_429, "der POST-Pfad muss ratenbegrenzt sein");
    }

    #[tokio::test]
    async fn nicht_aktiver_partner_wird_abgelehnt() {
        let Some(pool) = make_pool("t_demo_inaktiv").await else {
            return;
        };
        let _guard = ENV_LOCK.lock().await;
        set_secrets();
        sqlx::query("DELETE FROM twitch_partners")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO twitch_partners (twitch_login, twitch_user_id, status)
             VALUES ('pruefkonto', $1, 'departnered')",
        )
        .bind(TEST_USER_ID)
        .execute(&pool)
        .await
        .unwrap();
        let state = DashboardAuthState::new(pool.clone(), test_fernet_key());

        let resp = app(pool.clone(), state, 100)
            .oneshot(post_req(login_body(TEST_USER, &TEST_PASSWORD)))
            .await
            .unwrap();
        clear_secrets();

        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        assert!(resp.headers().get(SET_COOKIE).is_none());
        let rows: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM dashboard_sessions WHERE session_type = 'twitch'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(rows, 0);
    }

    #[tokio::test]
    async fn erschoepfte_verify_slots_ergeben_429() {
        let Some(pool) = make_pool("t_demo_slots").await else {
            return;
        };
        let _guard = ENV_LOCK.lock().await;
        set_secrets();
        let state = DashboardAuthState::new(pool.clone(), test_fernet_key());

        let mut permits = Vec::new();
        for _ in 0..super::MAX_CONCURRENT_VERIFY {
            permits.push(super::VERIFY_SLOTS.try_acquire().unwrap());
        }

        let resp = app(pool.clone(), state, 100)
            .oneshot(post_req(login_body(TEST_USER, &TEST_PASSWORD)))
            .await
            .unwrap();
        drop(permits);
        clear_secrets();

        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
        assert!(resp.headers().get(SET_COOKIE).is_none());
        let rows: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM dashboard_sessions WHERE session_type = 'twitch'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(rows, 0);
    }
}
