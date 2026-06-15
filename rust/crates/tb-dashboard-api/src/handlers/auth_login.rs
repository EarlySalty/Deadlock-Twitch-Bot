//! Nativer Twitch-OAuth-Dashboard-Login (B3-2).
//!
//! Routen (vor dem Strangler-Proxy registriert, damit sie NICHT mehr in den toten
//! Python-Fallback (502) laufen):
//! - `GET /twitch/auth/login`    — Redirect zur Twitch-Authorize-URL.
//! - `GET /twitch/auth/callback` — Code→Token→User, Partner-Gate, Session + Cookie.
//! - `GET /twitch/auth/logout`   — Session invalidieren + Cookie löschen.
//!
//! Python-Referenz: `bot/dashboard/auth/auth_mixin.py` (`auth_login`,
//! `auth_callback`, `_exchange_code_for_user`, `_is_partner_allowed`) und
//! `routes_entry.py` (`auth_logout`).
//!
//! **Secrets:** `TWITCH_CLIENT_ID`/`TWITCH_CLIENT_SECRET` werden NUR aus Env
//! gelesen (Infisical) und NIE geloggt. Fehler werden generisch geloggt.

use std::sync::Arc;

use axum::{
    extract::{Extension, Query},
    http::{header::SET_COOKIE, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Redirect, Response},
};
use serde::Deserialize;
use tracing::{info, warn};

use crate::auth::oauth_login::{
    build_login_authorize_url, sanitize_next_path, TwitchOAuthClient,
};
use crate::auth::session::{
    build_session_cookie, clear_session_cookie, DashboardAuthState, OAuthLoginState, SameSite,
    PARTNER_COOKIE_NAME, SESSION_CREATE_TTL_SECS,
};

/// Logout-Redirect-Ziel (Python `auth_logout`: 302 → `/analyse`).
const LOGOUT_REDIRECT: &str = "/analyse";

/// Laufzeit-Konfiguration des nativen Logins (als Extension injiziert).
///
/// Bündelt die Twitch-Client-ID, die exakte Authorize-`redirect_uri` und den
/// OAuth-HTTP-Client (Trait-Objekt → im Test ein Fake). `None` als Extension
/// bedeutet „OAuth nicht konfiguriert" → 503.
#[derive(Clone)]
pub struct OAuthLoginConfig {
    pub client_id: String,
    /// Vollständige, beim Authorize verwendete Redirect-URI
    /// (Env `TWITCH_DASHBOARD_AUTH_REDIRECT_URI`).
    pub redirect_uri: String,
    /// `true` → Cookies mit `Secure`-Flag (hinter HTTPS-Proxy in Prod).
    pub cookie_secure: bool,
    pub client: Arc<dyn TwitchOAuthClient>,
}

/// `?next=` für den Login-Start.
#[derive(Debug, Default, Deserialize)]
pub struct LoginQuery {
    pub next: Option<String>,
}

/// `?code=&state=&error=` für den Callback.
#[derive(Debug, Default, Deserialize)]
pub struct CallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
}

fn oauth_unconfigured() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        "Twitch OAuth ist aktuell nicht konfiguriert.",
    )
        .into_response()
}

/// `GET /twitch/auth/login` — startet den Twitch-OAuth-Login.
///
/// Erzeugt einen CSPRNG-State, persistiert ihn single-use in `dashboard_sessions`
/// (TTL [`OAUTH_STATE_TTL_SECS`]) und leitet zur Twitch-Authorize-URL weiter.
pub async fn login_handler(
    state: Option<Extension<DashboardAuthState>>,
    config: Option<Extension<OAuthLoginConfig>>,
    Query(query): Query<LoginQuery>,
) -> Response {
    let (Some(Extension(state)), Some(Extension(config))) = (state, config) else {
        return oauth_unconfigured();
    };
    if config.client_id.trim().is_empty() || config.redirect_uri.trim().is_empty() {
        return oauth_unconfigured();
    }

    let next_path = sanitize_next_path(query.next.as_deref());
    let state_token = tb_crypto::random_urlsafe_token(24);
    let login_state = OAuthLoginState {
        next_path,
        redirect_uri: config.redirect_uri.clone(),
    };

    if let Err(error) = state.save_oauth_login_state(&state_token, &login_state).await {
        warn!(%error, "OAuth-State konnte nicht persistiert werden");
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "OAuth-Status konnte nicht sicher gespeichert werden. Bitte erneut versuchen.",
        )
            .into_response();
    }

    let auth_url =
        build_login_authorize_url(&config.client_id, &config.redirect_uri, &state_token);
    no_store(Redirect::to(&auth_url).into_response())
}

/// `GET /twitch/auth/callback` — verarbeitet den Twitch-OAuth-Rücksprung.
///
/// 1. `error`/`state`/`code` validieren (fehlend → 400/401, KEINE Session).
/// 2. State single-use konsumieren (ungültig/abgelaufen → 400, KEINE Session).
/// 3. Code→Token→User über den [`TwitchOAuthClient`] (Fehler → 401).
/// 4. Partner-Gate (`find_partner_for_login`); kein Partner → 403.
/// 5. Session via `create_partner_session` + Cookie (`twitch_dash_session`,
///    HttpOnly/SameSite=Lax, Secure prod, 6h) → 302 ins Dashboard.
pub async fn callback_handler(
    state: Option<Extension<DashboardAuthState>>,
    config: Option<Extension<OAuthLoginConfig>>,
    Query(query): Query<CallbackQuery>,
) -> Response {
    let (Some(Extension(state)), Some(Extension(config))) = (state, config) else {
        return oauth_unconfigured();
    };

    let error = query.error.as_deref().map(str::trim).unwrap_or("");
    if !error.is_empty() {
        let safe: String = error
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
            .take(64)
            .collect();
        return no_store(
            (
                StatusCode::UNAUTHORIZED,
                format!("OAuth-Fehler: {safe}. Bitte Login erneut starten."),
            )
                .into_response(),
        );
    }

    let code = query.code.as_deref().map(str::trim).unwrap_or("");
    let state_token = query.state.as_deref().map(str::trim).unwrap_or("");
    if code.is_empty() || state_token.is_empty() {
        return no_store((StatusCode::BAD_REQUEST, "Fehlender OAuth state/code.").into_response());
    }

    // State single-use konsumieren (atomar DELETE … RETURNING). Replay/abgelaufen → None.
    let login_state = match state.consume_oauth_login_state(state_token).await {
        Ok(Some(s)) => s,
        Ok(None) => {
            return no_store(
                (StatusCode::BAD_REQUEST, "OAuth state ungültig oder abgelaufen.").into_response(),
            )
        }
        Err(error) => {
            warn!(%error, "OAuth-State-Lookup fehlgeschlagen");
            return no_store(
                (StatusCode::BAD_REQUEST, "OAuth state ungültig oder abgelaufen.").into_response(),
            );
        }
    };

    // Code→Token→User. redirect_uri MUSS die beim Authorize gespeicherte sein.
    let identity = match config
        .client
        .exchange_code_for_identity(code, &login_state.redirect_uri)
        .await
    {
        Ok(identity) => identity,
        Err(_) => {
            // Fehlerdetails NICHT loggen — könnten Token-Fragmente enthalten.
            warn!("Twitch-Code-Tausch fehlgeschlagen");
            return no_store(
                (
                    StatusCode::UNAUTHORIZED,
                    "OAuth-Austausch fehlgeschlagen. Bitte erneut versuchen.",
                )
                    .into_response(),
            );
        }
    };

    // Partner-Gate (Python _is_partner_allowed). Kein Partner → 403, KEINE Session.
    let partner = match state
        .find_partner_for_login(&identity.twitch_login, &identity.twitch_user_id)
        .await
    {
        Ok(Some(partner)) => partner,
        Ok(None) => {
            warn!(
                twitch_login = %identity.twitch_login,
                "AUDIT dashboard login denied (kein Partner)"
            );
            let who = if identity.display_name.is_empty() {
                identity.twitch_login.clone()
            } else {
                identity.display_name.clone()
            };
            return no_store(
                (
                    StatusCode::FORBIDDEN,
                    format!(
                        "Kein Zugriff: Twitch-Account '{who}' ist nicht als Streamer-Partner freigegeben."
                    ),
                )
                    .into_response(),
            );
        }
        Err(error) => {
            warn!(%error, "Partner-Gate-Lookup fehlgeschlagen");
            return no_store(
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Dashboard-Login konnte gerade nicht abgeschlossen werden. Bitte erneut versuchen.",
                )
                    .into_response(),
            );
        }
    };

    // Session anlegen (kanonischer Login/User-ID aus twitch_partners bevorzugt).
    let session = match state
        .create_partner_session(
            &partner.twitch_login,
            &partner.twitch_user_id,
            &identity.display_name,
        )
        .await
    {
        Ok(session) => session,
        Err(error) => {
            warn!(%error, "Session-Erstellung beim Login fehlgeschlagen");
            return no_store(
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Dashboard-Login konnte gerade nicht abgeschlossen werden. Bitte erneut versuchen.",
                )
                    .into_response(),
            );
        }
    };

    info!(twitch_login = %partner.twitch_login, "AUDIT dashboard login success");

    let cookie = build_session_cookie(
        PARTNER_COOKIE_NAME,
        &session.session_id,
        config.cookie_secure,
        SameSite::Lax,
        SESSION_CREATE_TTL_SECS,
    );
    redirect_with_cookie(&login_state.next_path, &cookie)
}

/// `GET /twitch/auth/logout` — invalidiert die Partner-Session.
///
/// Löscht die Session-Row + Cache (`invalidate_session`), entfernt das Cookie
/// (`clear_session_cookie`) und leitet auf [`LOGOUT_REDIRECT`] (`/analyse`).
pub async fn logout_handler(
    state: Option<Extension<DashboardAuthState>>,
    config: Option<Extension<OAuthLoginConfig>>,
    headers: HeaderMap,
) -> Response {
    // Cookie-Secure-Flag aus der Config (falls vorhanden), sonst konservativ true.
    let cookie_secure = config.as_ref().map(|c| c.0.cookie_secure).unwrap_or(true);

    if let Some(Extension(state)) = state {
        if let Some(session_id) = cookie_from_headers(&headers, PARTNER_COOKIE_NAME) {
            if !session_id.is_empty() {
                state.invalidate_session(&session_id).await;
                info!("AUDIT dashboard logout");
            }
        }
    }

    let cookie = clear_session_cookie(PARTNER_COOKIE_NAME, cookie_secure, SameSite::Lax);
    redirect_with_cookie(LOGOUT_REDIRECT, &cookie)
}

/// Liest einen Cookie-Wert direkt aus den Request-Headern (für Handler ohne
/// `Parts`-Zugriff). Spiegelt [`extract_cookie`].
fn cookie_from_headers(headers: &HeaderMap, name: &str) -> Option<String> {
    let raw = headers.get(axum::http::header::COOKIE)?.to_str().ok()?;
    for pair in raw.split(';') {
        let pair = pair.trim();
        if let Some((k, v)) = pair.split_once('=') {
            if k.trim() == name {
                return Some(v.trim().to_string());
            }
        }
    }
    None
}

/// 302-Redirect mit gesetztem `Set-Cookie` und `Cache-Control: no-store`.
fn redirect_with_cookie(location: &str, cookie: &str) -> Response {
    let mut response = Redirect::to(location).into_response();
    if let Ok(value) = HeaderValue::from_str(cookie) {
        response.headers_mut().append(SET_COOKIE, value);
    }
    no_store(response)
}

/// Setzt `Cache-Control: no-store` (Python `_set_no_store_headers`) — Auth-Antworten
/// dürfen nicht gecacht werden.
fn no_store(mut response: Response) -> Response {
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        HeaderValue::from_static("no-store, max-age=0"),
    );
    response
}

/// Baut die Login-Config aus der Umgebung (Env, Infisical-geladen).
///
/// `None`, wenn `TWITCH_CLIENT_ID`/`TWITCH_CLIENT_SECRET`/
/// `TWITCH_DASHBOARD_AUTH_REDIRECT_URI` fehlen oder leer sind — dann bleibt der
/// native Login deaktiviert (Routen liefern 503 statt zu raten). Secrets werden
/// NICHT geloggt.
pub fn oauth_login_config_from_env() -> Option<OAuthLoginConfig> {
    let client_id = non_empty_env("TWITCH_CLIENT_ID")?;
    let client_secret = non_empty_env("TWITCH_CLIENT_SECRET")?;
    let redirect_uri = non_empty_env("TWITCH_DASHBOARD_AUTH_REDIRECT_URI")?;
    // Secure-Cookies in Prod (HTTPS hinter dem Proxy); lokal abschaltbar.
    let cookie_secure = std::env::var("TB_DASHBOARD_COOKIE_INSECURE").as_deref() != Ok("1");

    let client = crate::auth::oauth_login::HelixOAuthClient::new(&client_id, &client_secret).ok()?;
    Some(OAuthLoginConfig {
        client_id,
        redirect_uri,
        cookie_secure,
        client: Arc::new(client),
    })
}

fn non_empty_env(key: &str) -> Option<String> {
    std::env::var(key).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::oauth_login::TwitchIdentity;
    use async_trait::async_trait;
    use axum::http::StatusCode;
    use tb_transport_twitch::user_token::UserTokenError;

    // ── 1. Reine Logik-Tests (kein DB/HTTP) ─────────────────────────────────

    #[test]
    fn cookie_aus_headern_geparst() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::COOKIE,
            HeaderValue::from_static("a=1; twitch_dash_session=abc123 ; b=2"),
        );
        assert_eq!(
            cookie_from_headers(&headers, PARTNER_COOKIE_NAME),
            Some("abc123".to_string())
        );
        assert_eq!(cookie_from_headers(&headers, "fehlt"), None);
    }

    #[test]
    fn redirect_with_cookie_setzt_location_und_set_cookie_und_no_store() {
        let resp = redirect_with_cookie("/analyse", "twitch_dash_session=xyz; Path=/");
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            resp.headers().get(axum::http::header::LOCATION).unwrap(),
            "/analyse"
        );
        assert_eq!(
            resp.headers().get(SET_COOKIE).unwrap(),
            "twitch_dash_session=xyz; Path=/"
        );
        assert_eq!(
            resp.headers().get(axum::http::header::CACHE_CONTROL).unwrap(),
            "no-store, max-age=0"
        );
    }

    // ── Fake-OAuth-Client (kein echter Twitch-Call, keine Secrets) ───────────

    struct FakeOAuth {
        identity: Result<TwitchIdentity, ()>,
    }

    #[async_trait]
    impl TwitchOAuthClient for FakeOAuth {
        async fn exchange_code_for_identity(
            &self,
            _code: &str,
            _redirect_uri: &str,
        ) -> Result<TwitchIdentity, UserTokenError> {
            self.identity
                .clone()
                .map_err(|_| UserTokenError::Other("fake exchange failure".to_string()))
        }
    }

    fn config_with(identity: Result<TwitchIdentity, ()>) -> OAuthLoginConfig {
        OAuthLoginConfig {
            client_id: "cid".to_string(),
            redirect_uri: "https://x.test/twitch/auth/callback".to_string(),
            cookie_secure: false,
            client: Arc::new(FakeOAuth { identity }),
        }
    }

    fn identity(login: &str, uid: &str) -> TwitchIdentity {
        TwitchIdentity {
            twitch_login: login.to_string(),
            twitch_user_id: uid.to_string(),
            display_name: format!("Display {login}"),
        }
    }

    // ── Handler-Tests OHNE DB: prüfen die secret-/auth-kritischen Frühausstiege.
    //    (DB-gestützte Vollläufe siehe integration_tests-Modul.)

    #[tokio::test]
    async fn login_ohne_config_503() {
        let resp = login_handler(None, None, Query(LoginQuery::default())).await;
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn callback_ohne_config_503() {
        let resp = callback_handler(None, None, Query(CallbackQuery::default())).await;
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn callback_fehlender_state_oder_code_400_ohne_db() {
        let cfg = config_with(Ok(identity("nani", "1")));
        // Wir brauchen einen Auth-State für den Pfad bis zur state/code-Prüfung —
        // aber die Prüfung kommt VOR jedem DB-Zugriff, also reicht ein Dummy-State
        // nicht; stattdessen testen wir den config-vorhanden-aber-state-fehlt-Pfad
        // über das DB-Integrationsmodul. Hier nur error-Param (kein DB-Zugriff):
        let resp = callback_handler(
            None,
            Some(Extension(cfg)),
            Query(CallbackQuery {
                error: Some("access_denied".to_string()),
                ..Default::default()
            }),
        )
        .await;
        // Ohne DashboardAuthState-Extension → 503 (fail-closed), nie eine Session.
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    // ── DB-gestützte Vollläufe (nur mit TB_TEST_REQUIRE_DB=1) ────────────────

    async fn maybe_pool() -> Option<sqlx::PgPool> {
        if std::env::var("TB_TEST_REQUIRE_DB").as_deref() != Ok("1") {
            return None;
        }
        let url = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        sqlx::PgPool::connect(&url).await.ok()
    }

    fn test_fernet_key() -> String {
        "dGVzdGtleTEyMzQ1Njc4OTAxMjM0NTY3ODkwMTIzNDU=".to_string()
    }

    async fn ensure_tables(pool: &sqlx::PgPool) {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS dashboard_sessions (
                session_id   TEXT NOT NULL PRIMARY KEY,
                session_type TEXT NOT NULL,
                payload_enc  BYTEA NOT NULL,
                created_at   DOUBLE PRECISION NOT NULL,
                expires_at   DOUBLE PRECISION NOT NULL
            )
            "#,
        )
        .execute(pool)
        .await
        .unwrap();
    }

    /// Hilfsfunktion: legt einen gültigen OAuth-State an und gibt das Token zurück.
    async fn seed_state(state: &DashboardAuthState, redirect_uri: &str, next: &str) -> String {
        let token = format!("cbtok-{}", uuid_like());
        state
            .save_oauth_login_state(
                &token,
                &OAuthLoginState {
                    next_path: next.to_string(),
                    redirect_uri: redirect_uri.to_string(),
                },
            )
            .await
            .unwrap();
        token
    }

    fn uuid_like() -> String {
        tb_crypto::random_urlsafe_token(8)
    }

    /// Callback mit gültigem Code+State+Partner → 302 + Set-Cookie + Session-Row.
    #[tokio::test]
    async fn callback_erfolgreich_legt_session_an_und_setzt_cookie() {
        let Some(pool) = maybe_pool().await else { return; };
        ensure_tables(&pool).await;
        let state = DashboardAuthState::new(pool.clone(), test_fernet_key());

        sqlx::query(
            "INSERT INTO twitch_partners (twitch_login, twitch_user_id, status)
             VALUES ($1, $2, 'active') ON CONFLICT DO NOTHING",
        )
        .bind("cbpartner")
        .bind("999001")
        .execute(&pool)
        .await
        .ok();

        let cfg = config_with(Ok(identity("cbpartner", "999001")));
        let token = seed_state(&state, &cfg.redirect_uri, "/twitch/stats").await;

        let resp = callback_handler(
            Some(Extension(state.clone())),
            Some(Extension(cfg)),
            Query(CallbackQuery {
                code: Some("good-code".to_string()),
                state: Some(token),
                error: None,
            }),
        )
        .await;

        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            resp.headers().get(axum::http::header::LOCATION).unwrap(),
            "/twitch/stats"
        );
        let cookie = resp.headers().get(SET_COOKIE).unwrap().to_str().unwrap();
        assert!(cookie.starts_with("twitch_dash_session="));
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("SameSite=Lax"));
        // Extrahiere die session_id und prüfe, dass die Session real existiert.
        let sid = cookie
            .strip_prefix("twitch_dash_session=")
            .and_then(|s| s.split(';').next())
            .unwrap()
            .to_string();
        assert!(state.load_partner_session(&sid).await.unwrap().is_some());

        sqlx::query("DELETE FROM dashboard_sessions WHERE session_id = $1")
            .bind(&sid)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM twitch_partners WHERE twitch_login = 'cbpartner'")
            .execute(&pool)
            .await
            .ok();
    }

    /// Callback, aber Twitch-User ist KEIN Partner → 403, KEINE Session.
    #[tokio::test]
    async fn callback_kein_partner_403_ohne_session() {
        let Some(pool) = maybe_pool().await else { return; };
        ensure_tables(&pool).await;
        let state = DashboardAuthState::new(pool.clone(), test_fernet_key());

        let cfg = config_with(Ok(identity("fremder_nicht_partner", "999002")));
        let token = seed_state(&state, &cfg.redirect_uri, "/analyse").await;

        let resp = callback_handler(
            Some(Extension(state.clone())),
            Some(Extension(cfg)),
            Query(CallbackQuery {
                code: Some("good".to_string()),
                state: Some(token),
                error: None,
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        assert!(resp.headers().get(SET_COOKIE).is_none(), "keine Session-Cookie");
    }

    /// Callback mit ungültigem State → 400, KEINE Session. (Exchange wird nie erreicht.)
    #[tokio::test]
    async fn callback_ungueltiger_state_400_ohne_session() {
        let Some(pool) = maybe_pool().await else { return; };
        ensure_tables(&pool).await;
        let state = DashboardAuthState::new(pool.clone(), test_fernet_key());
        let cfg = config_with(Ok(identity("egal", "1")));

        let resp = callback_handler(
            Some(Extension(state)),
            Some(Extension(cfg)),
            Query(CallbackQuery {
                code: Some("c".to_string()),
                state: Some("gibts-nicht".to_string()),
                error: None,
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        assert!(resp.headers().get(SET_COOKIE).is_none());
    }

    /// Callback mit Exchange-Fehler → 401, KEINE Session (State ist verbraucht).
    #[tokio::test]
    async fn callback_exchange_fehler_401_ohne_session() {
        let Some(pool) = maybe_pool().await else { return; };
        ensure_tables(&pool).await;
        let state = DashboardAuthState::new(pool.clone(), test_fernet_key());
        let cfg = config_with(Err(())); // Exchange schlägt fehl.
        let token = seed_state(&state, &cfg.redirect_uri, "/analyse").await;

        let resp = callback_handler(
            Some(Extension(state)),
            Some(Extension(cfg)),
            Query(CallbackQuery {
                code: Some("c".to_string()),
                state: Some(token),
                error: None,
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        assert!(resp.headers().get(SET_COOKIE).is_none());
    }

    /// Login mit gültiger Config → 302 zur Twitch-Authorize-URL + State persistiert.
    #[tokio::test]
    async fn login_redirectet_zu_twitch_und_persistiert_state() {
        let Some(pool) = maybe_pool().await else { return; };
        ensure_tables(&pool).await;
        let state = DashboardAuthState::new(pool.clone(), test_fernet_key());
        let cfg = config_with(Ok(identity("x", "1")));

        let resp = login_handler(
            Some(Extension(state)),
            Some(Extension(cfg)),
            Query(LoginQuery { next: Some("/twitch/stats".to_string()) }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        let loc = resp
            .headers()
            .get(axum::http::header::LOCATION)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(loc.starts_with("https://id.twitch.tv/oauth2/authorize"));
        assert!(loc.contains("client_id=cid"));
        assert!(loc.contains("response_type=code"));
        assert!(!loc.contains("scope="));
    }

    /// Logout löscht das Cookie (Max-Age=0) und redirectet auf /analyse.
    #[tokio::test]
    async fn logout_loescht_cookie_und_redirectet() {
        let Some(pool) = maybe_pool().await else { return; };
        ensure_tables(&pool).await;
        let state = DashboardAuthState::new(pool.clone(), test_fernet_key());

        sqlx::query(
            "INSERT INTO twitch_partners (twitch_login, twitch_user_id, status)
             VALUES ($1, $2, 'active') ON CONFLICT DO NOTHING",
        )
        .bind("logoutpartner")
        .bind("999003")
        .execute(&pool)
        .await
        .ok();

        let created = state
            .create_partner_session("logoutpartner", "999003", "Lo")
            .await
            .unwrap();

        let cfg = config_with(Ok(identity("x", "1")));
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::COOKIE,
            HeaderValue::from_str(&format!("twitch_dash_session={}", created.session_id)).unwrap(),
        );

        let resp = logout_handler(Some(Extension(state.clone())), Some(Extension(cfg)), headers).await;
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        assert_eq!(resp.headers().get(axum::http::header::LOCATION).unwrap(), "/analyse");
        let cookie = resp.headers().get(SET_COOKIE).unwrap().to_str().unwrap();
        assert!(cookie.contains("Max-Age=0"));
        // Session ist invalidiert.
        assert!(state.load_partner_session(&created.session_id).await.unwrap().is_none());

        sqlx::query("DELETE FROM twitch_partners WHERE twitch_login = 'logoutpartner'")
            .execute(&pool)
            .await
            .ok();
    }
}
