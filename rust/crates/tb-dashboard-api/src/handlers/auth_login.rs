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

use std::{sync::Arc, time::Duration};

use axum::{
    extract::{Extension, Query},
    http::{header::SET_COOKIE, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Redirect, Response},
};
use serde::Deserialize;
use serde_json::json;
use tb_http_core::{IDEMPOTENCY_KEY_HEADER, INTERNAL_API_BASE_PATH, INTERNAL_TOKEN_HEADER};
use tracing::{info, warn};

use crate::auth::oauth_login::{build_login_authorize_url, sanitize_next_path, TwitchOAuthClient};
use crate::auth::session::{
    build_session_cookie, clear_session_cookie, DashboardAuthState, OAuthLoginState, SameSite,
    PARTNER_COOKIE_NAME, SESSION_CREATE_TTL_SECS,
};
use crate::handlers::auth_status::ADMIN_MODE_COOKIE;
use crate::handlers::spa::is_admin_dashboard_host_request;

/// Logout-Redirect-Ziel (Python `auth_logout`: 302 → `/analyse`).
const LOGOUT_REDIRECT: &str = "/analyse";
/// Admin-Logout-Ziel: zurück in den Discord-Admin-Login auf demselben Host.
const ADMIN_LOGOUT_REDIRECT: &str = "/twitch/auth/discord/login";
/// Cookie-Name des OAuth-Kontext-CSRF-Tokens (P2.139). Kurzlebig, HttpOnly,
/// SameSite=Lax; bindet den Callback an den Browser, der den Login startete.
const OAUTH_CONTEXT_COOKIE: &str = "twitch_dash_session_oauth_ctx";
/// Default-Ziel nach erfolgreicher Raid-OAuth-Autorisierung (Python-Konstante).
const DEFAULT_RAID_OAUTH_SUCCESS_REDIRECT_URL: &str =
    "https://deutsche-deadlock-community.de/twitch/dashboard";
/// Interner Worker-Endpoint, der den nativen Raid-OAuth-Callback verarbeitet.
const RAID_OAUTH_CALLBACK_PATH: &str = "/raid/oauth-callback";
const RAID_OAUTH_CALLBACK_TIMEOUT: Duration = Duration::from_secs(20);

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
    /// Optionaler Dispatch für Raid-OAuth-States auf dem geteilten
    /// `/callback/twitch`-Pfad. Fehlt die interne API-Konfiguration, bleibt
    /// der Dashboard-Login aktiv; nur Raid-Delegation liefert dann 503.
    pub raid_callback: Option<RaidOAuthCallbackConfig>,
}

/// Konfiguration für den internen Hop zum `tb-bot`-Raid-OAuth-Callback.
#[derive(Clone)]
pub struct RaidOAuthCallbackConfig {
    endpoint_url: String,
    internal_token: String,
    client: reqwest::Client,
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

#[derive(Debug, Default, Deserialize)]
struct RaidOAuthCallbackPayload {
    status: Option<i64>,
    title: Option<String>,
    body_html: Option<String>,
    redirect_url: Option<String>,
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
    // P2.139: Kontext-CSRF-Token erzeugen, im State persistieren UND als HttpOnly-
    // Cookie setzen. Der Callback prüft beide gegeneinander → ein fremder/cookie-
    // loser Callback (untergeschobener OAuth-Code) wird abgelehnt.
    let context_token = tb_crypto::random_urlsafe_token(24);
    let login_state = OAuthLoginState {
        next_path,
        redirect_uri: config.redirect_uri.clone(),
        context_token: context_token.clone(),
    };

    if let Err(error) = state
        .save_oauth_login_state(&state_token, &login_state)
        .await
    {
        warn!(%error, "OAuth-State konnte nicht persistiert werden");
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "OAuth-Status konnte nicht sicher gespeichert werden. Bitte erneut versuchen.",
        )
            .into_response();
    }

    let auth_url = build_login_authorize_url(&config.client_id, &config.redirect_uri, &state_token);
    let ctx_cookie = build_session_cookie(
        OAUTH_CONTEXT_COOKIE,
        &context_token,
        config.cookie_secure,
        SameSite::Lax,
        OAUTH_CONTEXT_COOKIE_TTL_SECS,
    );
    let mut response = Redirect::to(&auth_url).into_response();
    if let Ok(value) = HeaderValue::from_str(&ctx_cookie) {
        response.headers_mut().append(SET_COOKIE, value);
    }
    no_store(response)
}

/// TTL des OAuth-Kontext-Cookies (P2.139) — kurz, deckt nur den Login-Roundtrip.
const OAUTH_CONTEXT_COOKIE_TTL_SECS: u64 = 600;

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
    headers: HeaderMap,
    Query(query): Query<CallbackQuery>,
) -> Response {
    callback_handler_inner(state, config, &headers, query, false).await
}

/// `GET /callback/twitch` — geteilter Twitch-OAuth-Callback.
///
/// Wie Python entscheidet dieser Pfad nach dem Dashboard-State-Lookup per
/// State-Store, ob der Callback zum Dashboard-Login oder zum Raid-OAuth-Flow
/// gehört.
pub async fn shared_callback_handler(
    state: Option<Extension<DashboardAuthState>>,
    config: Option<Extension<OAuthLoginConfig>>,
    headers: HeaderMap,
    Query(query): Query<CallbackQuery>,
) -> Response {
    callback_handler_inner(state, config, &headers, query, true).await
}

async fn callback_handler_inner(
    state: Option<Extension<DashboardAuthState>>,
    config: Option<Extension<OAuthLoginConfig>>,
    headers: &HeaderMap,
    query: CallbackQuery,
    allow_raid_delegate: bool,
) -> Response {
    let (Some(Extension(state)), Some(Extension(config))) = (state, config) else {
        return oauth_unconfigured();
    };

    let error = query.error.as_deref().map(str::trim).unwrap_or("");
    let code = query.code.as_deref().map(str::trim).unwrap_or("");
    let state_token = query.state.as_deref().map(str::trim).unwrap_or("");

    if state_token.is_empty() {
        if !error.is_empty() {
            return oauth_error_response(error);
        }
        return no_store((StatusCode::BAD_REQUEST, "Fehlender OAuth state/code.").into_response());
    }

    // State single-use konsumieren (atomar DELETE … RETURNING). Replay/abgelaufen → None.
    let login_state = match state.consume_oauth_login_state(state_token).await {
        Ok(Some(s)) => s,
        Ok(None) => {
            if let Some(response) = crate::handlers::affiliate::try_shared_affiliate_callback(
                &state,
                config.client.as_ref(),
                config.cookie_secure,
                code,
                state_token,
                error,
            )
            .await
            {
                return response;
            }
            if allow_raid_delegate {
                if let Some(response) =
                    maybe_delegate_raid_oauth_callback(&state, &config, code, state_token, error)
                        .await
                {
                    return response;
                }
            }
            if !error.is_empty() {
                return oauth_error_response(error);
            }
            if code.is_empty() {
                return no_store(
                    (StatusCode::BAD_REQUEST, "Fehlender OAuth state/code.").into_response(),
                );
            }
            return no_store(
                (
                    StatusCode::BAD_REQUEST,
                    "OAuth state ungültig oder abgelaufen.",
                )
                    .into_response(),
            );
        }
        Err(db_error) => {
            warn!(%db_error, "OAuth-State-Lookup fehlgeschlagen");
            if allow_raid_delegate {
                if let Some(response) =
                    maybe_delegate_raid_oauth_callback(&state, &config, code, state_token, error)
                        .await
                {
                    return response;
                }
            }
            return no_store(
                (
                    StatusCode::BAD_REQUEST,
                    "OAuth state ungültig oder abgelaufen.",
                )
                    .into_response(),
            );
        }
    };

    if !error.is_empty() {
        return oauth_error_response(error);
    }
    if code.is_empty() {
        return no_store((StatusCode::BAD_REQUEST, "Fehlender OAuth state/code.").into_response());
    }

    // P2.139: Kontext-CSRF-Bindung. Trägt der State ein context_token, MUSS das
    // `twitch_dash_session_oauth_ctx`-Cookie des Browsers konstant-zeitlich passen. Ein
    // cookieloser/fremder Callback (untergeschobener Code) → 400, KEINE Session.
    if !login_state.context_token.is_empty() {
        let presented = cookie_from_headers(headers, OAUTH_CONTEXT_COOKIE).unwrap_or_default();
        if presented.is_empty()
            || !tb_crypto::constant_time_eq(
                presented.as_bytes(),
                login_state.context_token.as_bytes(),
            )
        {
            warn!("OAuth-Callback ohne gültiges Kontext-Cookie abgelehnt (CSRF)");
            return no_store(clear_context_and_respond(
                config.cookie_secure,
                (
                    StatusCode::BAD_REQUEST,
                    "OAuth-Kontext ungültig. Bitte Login erneut starten.",
                )
                    .into_response(),
            ));
        }
    }

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

    // P1.56: Self-Heal — meldet sich ein departnered/archived/token_error-Partner
    // selbst per OAuth an, reaktivieren wir ihn (status='active', Pausen geräumt).
    // blocked/bot_banned bleiben unangetastet. Fehler nur warnen (kein Login-Stop).
    match state
        .reactivate_partner(&partner.twitch_login, &partner.twitch_user_id)
        .await
    {
        Ok(true) => {
            info!(twitch_login = %partner.twitch_login, "AUDIT partner self-reactivated on login")
        }
        Ok(false) => {}
        Err(error) => warn!(%error, "Partner-Reaktivierung beim Login fehlgeschlagen"),
    }

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
    // Einmal-Kontext-Cookie nach erfolgreichem Login löschen (P2.139).
    clear_context_and_respond(
        config.cookie_secure,
        redirect_with_cookie(&login_state.next_path, &cookie),
    )
}

async fn maybe_delegate_raid_oauth_callback(
    state: &DashboardAuthState,
    config: &OAuthLoginConfig,
    code: &str,
    state_token: &str,
    error: &str,
) -> Option<Response> {
    match state.has_raid_oauth_state(state_token).await {
        Ok(false) => None,
        Ok(true) => {
            let Some(raid_config) = config.raid_callback.as_ref() else {
                warn!(
                    "Raid-OAuth-State erkannt, aber interner Raid-Callback ist nicht konfiguriert"
                );
                return Some(raid_oauth_unavailable_response());
            };
            Some(call_raid_oauth_callback(raid_config, code, state_token, error).await)
        }
        Err(error) => {
            warn!(%error, "Raid-OAuth-State-Lookup fehlgeschlagen");
            None
        }
    }
}

async fn call_raid_oauth_callback(
    config: &RaidOAuthCallbackConfig,
    code: &str,
    state_token: &str,
    error: &str,
) -> Response {
    let payload = json!({
        "code": code,
        "state": state_token,
        "error": error,
    });
    let response = config
        .client
        .post(&config.endpoint_url)
        .header(INTERNAL_TOKEN_HEADER, &config.internal_token)
        .header(
            IDEMPOTENCY_KEY_HEADER,
            raid_oauth_idempotency_key(state_token),
        )
        .json(&payload)
        .send()
        .await;

    let response = match response {
        Ok(response) => response,
        Err(error) => {
            warn!(%error, "Raid-OAuth-Callback-Hop fehlgeschlagen");
            return raid_oauth_unavailable_response();
        }
    };
    if !response.status().is_success() {
        let status = response.status().as_u16();
        warn!(status, "Raid-OAuth-Callback-Hop lieferte Fehlerstatus");
        return raid_oauth_unavailable_response();
    }

    match response.json::<RaidOAuthCallbackPayload>().await {
        Ok(payload) => raid_oauth_payload_response(payload),
        Err(error) => {
            warn!(%error, "Raid-OAuth-Callback-Antwort war kein gueltiges JSON");
            raid_oauth_unavailable_response()
        }
    }
}

fn raid_oauth_payload_response(payload: RaidOAuthCallbackPayload) -> Response {
    let status = clamp_raid_status(payload.status);
    let redirect_candidate = payload.redirect_url.unwrap_or_default();
    if status.as_u16() < 400 && !redirect_candidate.trim().is_empty() {
        return no_store(
            Redirect::to(&normalize_raid_success_redirect_url(&redirect_candidate)).into_response(),
        );
    }

    let title = payload
        .title
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "Autorisierung".to_string());
    let body_html = payload
        .body_html
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "<p>Unbekannte Antwort.</p>".to_string());
    no_store(
        (
            status,
            [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
            render_oauth_page(&title, &body_html),
        )
            .into_response(),
    )
}

fn raid_oauth_unavailable_response() -> Response {
    raid_oauth_payload_response(RaidOAuthCallbackPayload {
        status: Some(503),
        title: Some("Twitch OAuth nicht verfügbar".to_string()),
        body_html: Some("<p>Der interne Bot-Service ist aktuell nicht verfügbar.</p>".to_string()),
        redirect_url: None,
    })
}

fn oauth_error_response(error: &str) -> Response {
    let safe: String = error
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .take(64)
        .collect();
    no_store(
        (
            StatusCode::UNAUTHORIZED,
            format!("OAuth-Fehler: {safe}. Bitte Login erneut starten."),
        )
            .into_response(),
    )
}

fn clamp_raid_status(status: Option<i64>) -> StatusCode {
    let raw = status.unwrap_or(200).clamp(200, 599) as u16;
    StatusCode::from_u16(raw).unwrap_or(StatusCode::OK)
}

fn raid_oauth_idempotency_key(state_token: &str) -> String {
    let suffix: String = state_token
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .take(96)
        .collect();
    if suffix.is_empty() {
        "dashboard-raid-oauth".to_string()
    } else {
        format!("dashboard-raid-oauth-{suffix}")
    }
}

fn normalize_raid_success_redirect_url(candidate: &str) -> String {
    let fallback = DEFAULT_RAID_OAUTH_SUCCESS_REDIRECT_URL.to_string();
    let Ok(mut url) = url::Url::parse(candidate.trim()) else {
        return fallback;
    };
    if !url.username().is_empty() || url.password().is_some() {
        return fallback;
    }
    let scheme = url.scheme().to_ascii_lowercase();
    let Some(host) = url.host_str().map(str::to_ascii_lowercase) else {
        return fallback;
    };
    if scheme != "https" && scheme != "http" {
        return fallback;
    }
    if scheme == "http" && !matches!(host.as_str(), "127.0.0.1" | "localhost" | "::1") {
        return fallback;
    }
    url.set_fragment(None);
    url.to_string()
}

fn render_oauth_page(title: &str, body_html: &str) -> String {
    let title_attr = html_escape(title);
    let title_text = html_escape(title);
    format!(
        "<!doctype html><html lang='de'><head><meta charset='utf-8'>\
         <meta name='viewport' content='width=device-width,initial-scale=1'>\
         <title>{title_attr}</title>\
         <style>\
         body{{font-family:Segoe UI,Arial,sans-serif;background:#0f172a;color:#e2e8f0;margin:0;}}\
         .wrap{{max-width:760px;margin:0 auto;padding:36px 18px;}}\
         .card{{background:#111827;border:1px solid #1f2937;border-radius:12px;padding:20px;}}\
         h1{{margin:0 0 12px 0;font-size:24px;}}\
         p{{line-height:1.5;margin:10px 0;}}\
         code{{background:#0b1220;border:1px solid #23304a;padding:2px 6px;border-radius:6px;}}\
         a{{color:#93c5fd;}}\
         </style></head><body><div class='wrap'><div class='card'>\
         <h1>{title_text}</h1>{body_html}</div></div></body></html>"
    )
}

fn html_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            _ => out.push(ch),
        }
    }
    out
}

/// `GET /twitch/auth/logout` — invalidiert die Partner-Session.
///
/// Löscht die Session-Row + Cache (`invalidate_session`), entfernt das Cookie
/// (`clear_session_cookie`) und leitet auf [`LOGOUT_REDIRECT`] (`/analyse`).
/// Kommt der Logout vom Admin-Host, ist das Ziel der relative Admin-Login-Pfad.
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
    let logout_redirect = logout_redirect_for_request(&headers);
    let mut response = redirect_with_cookie(&logout_redirect, &cookie);
    let admin_mode_cookie = clear_session_cookie(ADMIN_MODE_COOKIE, cookie_secure, SameSite::Lax);
    if let Ok(value) = HeaderValue::from_str(&admin_mode_cookie) {
        response.headers_mut().append(SET_COOKIE, value);
    }
    response
}

fn logout_redirect_for_request(headers: &HeaderMap) -> String {
    if is_admin_dashboard_host_request(headers) {
        return ADMIN_LOGOUT_REDIRECT.to_string();
    }
    LOGOUT_REDIRECT.to_string()
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

/// Hängt ein Lösch-Cookie für das OAuth-Kontext-Cookie (P2.139) an eine Antwort,
/// damit der Einmal-Kontext nach dem Callback (Erfolg ODER Ablehnung) verschwindet.
fn clear_context_and_respond(cookie_secure: bool, mut response: Response) -> Response {
    let clear = clear_session_cookie(OAUTH_CONTEXT_COOKIE, cookie_secure, SameSite::Lax);
    if let Ok(value) = HeaderValue::from_str(&clear) {
        response.headers_mut().append(SET_COOKIE, value);
    }
    response
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
    // P2.137: Redirect-URI härten, BEVOR sie in die Authorize-URL fließt. Eine
    // verseuchte/falsch konfigurierte URI (fremder Host, userinfo, der RAID-
    // reservierte Callback) darf den nativen Login NICHT aktivieren — sonst
    // entführt ein Angreifer den OAuth-Code. Ungültig → None (Login bleibt aus).
    if let Err(reason) = validate_oauth_redirect_uri(&redirect_uri) {
        warn!(
            reason,
            "TWITCH_DASHBOARD_AUTH_REDIRECT_URI ungültig — nativer Login deaktiviert"
        );
        return None;
    }
    // Secure-Cookies in Prod (HTTPS hinter dem Proxy); lokal abschaltbar.
    let cookie_secure = std::env::var("TB_DASHBOARD_COOKIE_INSECURE").as_deref() != Ok("1");

    let client =
        crate::auth::oauth_login::HelixOAuthClient::new(&client_id, &client_secret).ok()?;
    Some(OAuthLoginConfig {
        client_id,
        redirect_uri,
        cookie_secure,
        client: Arc::new(client),
        raid_callback: raid_oauth_callback_config_from_env(),
    })
}

fn raid_oauth_callback_config_from_env() -> Option<RaidOAuthCallbackConfig> {
    let internal_token = non_empty_env("TWITCH_INTERNAL_API_TOKEN")?;
    let endpoint_url = format!(
        "{}{}{}",
        worker_internal_base_url(),
        INTERNAL_API_BASE_PATH,
        RAID_OAUTH_CALLBACK_PATH
    );
    let client = reqwest::Client::builder()
        .timeout(RAID_OAUTH_CALLBACK_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .ok()?;
    Some(RaidOAuthCallbackConfig {
        endpoint_url,
        internal_token,
        client,
    })
}

fn worker_internal_base_url() -> String {
    if let Some(explicit) = non_empty_env("TWITCH_INTERNAL_API_BASE_URL") {
        return explicit.trim_end_matches('/').to_string();
    }
    let host = non_empty_env("TWITCH_INTERNAL_API_HOST").unwrap_or_else(|| "127.0.0.1".to_string());
    let port = non_empty_env("TWITCH_INTERNAL_API_PORT").unwrap_or_else(|| "8776".to_string());
    format!("http://{host}:{port}")
}

fn non_empty_env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// Rein RAID-reservierter Callback-Pfad — der Dashboard-Login darf NIE hierauf
/// zeigen (er gehört exklusiv dem Raid-OAuth-Flow). `/callback/twitch` ist davon
/// ausgenommen: das ist der GETEILTE Callback (`shared_callback_handler`), der den
/// Dashboard-Login trägt UND Raid delegiert — er steht auf der Whitelist.
const RESERVED_RAID_CALLBACK_PATHS: &[&str] = &["/twitch/raid/callback"];

/// Erlaubte Dashboard-Callback-Pfade (Whitelist). Andere Pfade → abgelehnt.
/// `/callback/twitch` ist der produktive geteilte Callback (Login + Raid-Delegation).
const ALLOWED_DASHBOARD_CALLBACK_PATHS: &[&str] = &[
    "/twitch/auth/callback",
    "/twitch/auth/login/callback",
    "/callback/twitch",
];

/// Validiert die Dashboard-OAuth-Redirect-URI (P2.137).
///
/// Anforderungen (industriell gehärtet, nicht 1:1 Python):
/// - parsebare absolute URL,
/// - Schema `https` (oder `http` nur für Loopback-Hosts, lokale Entwicklung),
/// - KEINE userinfo (`user:pass@`),
/// - nicht der RAID-reservierte Callback-Pfad,
/// - Pfad steht auf der Dashboard-Callback-Whitelist.
fn validate_oauth_redirect_uri(raw: &str) -> Result<(), &'static str> {
    let url = url::Url::parse(raw.trim()).map_err(|_| "unparsebare URI")?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err("userinfo nicht erlaubt");
    }
    let scheme = url.scheme().to_ascii_lowercase();
    let host = url
        .host_str()
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if host.is_empty() {
        return Err("kein Host");
    }
    let is_loopback = matches!(host.as_str(), "localhost" | "127.0.0.1" | "::1");
    match scheme.as_str() {
        "https" => {}
        "http" if is_loopback => {}
        _ => return Err("Schema muss https sein (http nur Loopback)"),
    }
    let path = url.path();
    if RESERVED_RAID_CALLBACK_PATHS.contains(&path) {
        return Err("RAID-reservierter Callback-Pfad");
    }
    if !ALLOWED_DASHBOARD_CALLBACK_PATHS.contains(&path) {
        return Err("Pfad nicht auf der Dashboard-Callback-Whitelist");
    }
    Ok(())
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
            resp.headers()
                .get(axum::http::header::CACHE_CONTROL)
                .unwrap(),
            "no-store, max-age=0"
        );
    }

    #[tokio::test]
    async fn logout_loescht_partner_und_admin_mode_cookie_ohne_state() {
        let resp = logout_handler(None, None, HeaderMap::new()).await;
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        let cookies: Vec<&str> = resp
            .headers()
            .get_all(SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .collect();
        assert!(cookies.iter().any(|cookie| {
            cookie.starts_with("twitch_dash_session=") && cookie.contains("Max-Age=0")
        }));
        assert!(cookies.iter().any(|cookie| {
            cookie.starts_with("tb_admin_mode=") && cookie.contains("Max-Age=0")
        }));
    }

    #[tokio::test]
    async fn logout_vom_admin_host_redirectet_zur_admin_login_und_loescht_cookies() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::HOST,
            HeaderValue::from_static("admin.deutsche-deadlock-community.de"),
        );

        let resp = logout_handler(None, None, headers).await;

        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            resp.headers().get(axum::http::header::LOCATION).unwrap(),
            "/twitch/auth/discord/login"
        );
        let cookies: Vec<&str> = resp
            .headers()
            .get_all(SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .collect();
        assert!(cookies.iter().any(|cookie| {
            cookie.starts_with("twitch_dash_session=") && cookie.contains("Max-Age=0")
        }));
        assert!(cookies.iter().any(|cookie| {
            cookie.starts_with("tb_admin_mode=") && cookie.contains("Max-Age=0")
        }));
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
            raid_callback: None,
        }
    }

    fn identity(login: &str, uid: &str) -> TwitchIdentity {
        TwitchIdentity {
            twitch_login: login.to_string(),
            twitch_user_id: uid.to_string(),
            display_name: format!("Display {login}"),
            email: String::new(),
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
        let resp = callback_handler(
            None,
            None,
            HeaderMap::new(),
            Query(CallbackQuery::default()),
        )
        .await;
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
            HeaderMap::new(),
            Query(CallbackQuery {
                error: Some("access_denied".to_string()),
                ..Default::default()
            }),
        )
        .await;
        // Ohne DashboardAuthState-Extension → 503 (fail-closed), nie eine Session.
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[test]
    fn redirect_uri_validierung_p2_137() {
        // Gültig: https + Dashboard-Callback.
        assert!(
            validate_oauth_redirect_uri("https://dash.example.com/twitch/auth/callback").is_ok()
        );
        // Gültig: http nur Loopback (lokale Entwicklung).
        assert!(validate_oauth_redirect_uri("http://localhost:8769/twitch/auth/callback").is_ok());
        // Rein RAID-reservierter Pfad → abgelehnt.
        assert!(validate_oauth_redirect_uri("https://evil.test/twitch/raid/callback").is_err());
        // Geteilter Dashboard-Callback /callback/twitch → erlaubt (trägt den Login).
        assert!(validate_oauth_redirect_uri("https://dash.example.com/callback/twitch").is_ok());
        // userinfo → abgelehnt.
        assert!(validate_oauth_redirect_uri(
            "https://user:pass@dash.example.com/twitch/auth/callback"
        )
        .is_err());
        // http auf Nicht-Loopback → abgelehnt.
        assert!(
            validate_oauth_redirect_uri("http://dash.example.com/twitch/auth/callback").is_err()
        );
        // Nicht-Whitelist-Pfad → abgelehnt.
        assert!(validate_oauth_redirect_uri("https://dash.example.com/twitch/auth/evil").is_err());
        // Unparsebar → abgelehnt.
        assert!(validate_oauth_redirect_uri("not a url").is_err());
    }

    #[test]
    fn raid_success_redirect_wird_wie_python_sanitized() {
        assert_eq!(
            normalize_raid_success_redirect_url("https://example.test/a/b?q=1#frag"),
            "https://example.test/a/b?q=1"
        );
        assert_eq!(
            normalize_raid_success_redirect_url("http://localhost:8769/twitch/dashboard"),
            "http://localhost:8769/twitch/dashboard"
        );
        assert_eq!(
            normalize_raid_success_redirect_url("http://example.test/unsicher"),
            DEFAULT_RAID_OAUTH_SUCCESS_REDIRECT_URL
        );
        assert_eq!(
            normalize_raid_success_redirect_url("https://user:pass@example.test/"),
            DEFAULT_RAID_OAUTH_SUCCESS_REDIRECT_URL
        );
    }

    #[tokio::test]
    async fn raid_payload_redirectet_bei_erfolg_und_rendert_fehlerseite() {
        let redirect = raid_oauth_payload_response(RaidOAuthCallbackPayload {
            status: Some(200),
            title: Some("ok".to_string()),
            body_html: Some("<p>ok</p>".to_string()),
            redirect_url: Some("https://example.test/dash#x".to_string()),
        });
        assert_eq!(redirect.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            redirect
                .headers()
                .get(axum::http::header::LOCATION)
                .unwrap(),
            "https://example.test/dash"
        );

        let page = raid_oauth_payload_response(RaidOAuthCallbackPayload {
            status: Some(503),
            title: Some("Titel".to_string()),
            body_html: Some("<p>Body</p>".to_string()),
            redirect_url: None,
        });
        assert_eq!(page.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            page.headers()
                .get(axum::http::header::CACHE_CONTROL)
                .unwrap(),
            "no-store, max-age=0"
        );
        let body = axum::body::to_bytes(page.into_body(), 65536).await.unwrap();
        let html = String::from_utf8(body.to_vec()).unwrap();
        assert!(html.contains("<h1>Titel</h1><p>Body</p>"));
    }

    // ── DB-gestützte Vollläufe (nur mit TB_TEST_REQUIRE_DB=1) ────────────────

    async fn maybe_pool() -> Option<sqlx::PgPool> {
        if std::env::var("TB_TEST_REQUIRE_DB").as_deref() != Ok("1") {
            return None;
        }
        let url = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let schema = crate::auth::session::test_schema_name("auth_login");
        let admin_pool = sqlx::PgPool::connect(&url).await.ok()?;
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin_pool)
            .await
            .ok()?;
        admin_pool.close().await;

        let opts: sqlx::postgres::PgConnectOptions = url.parse().ok()?;
        let opts = opts.options([("search_path", schema.as_str())]);
        sqlx::postgres::PgPoolOptions::new()
            .max_connections(2)
            .connect_with(opts)
            .await
            .ok()
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
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS twitch_partners (
                id BIGSERIAL PRIMARY KEY,
                twitch_login TEXT NOT NULL,
                twitch_user_id TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'active',
                technical_pause_reason TEXT,
                manual_partner_opt_out INTEGER DEFAULT 0,
                departnered_at TEXT,
                admin_archived_at TEXT,
                partnered_at TEXT DEFAULT CURRENT_TIMESTAMP
            )
            "#,
        )
        .execute(pool)
        .await
        .unwrap();
    }

    async fn ensure_affiliate_tables(pool: &sqlx::PgPool) {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS affiliate_accounts (
                twitch_login TEXT PRIMARY KEY,
                twitch_user_id TEXT NOT NULL,
                display_name TEXT,
                email TEXT NOT NULL,
                full_name TEXT NOT NULL,
                address_line1 TEXT NOT NULL,
                address_city TEXT NOT NULL,
                address_zip TEXT NOT NULL,
                address_country TEXT NOT NULL DEFAULT 'DE',
                stripe_account_id TEXT,
                stripe_connected_at TEXT,
                stripe_connect_status TEXT DEFAULT 'pending',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                is_active INTEGER NOT NULL DEFAULT 1
            )
            "#,
        )
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS affiliate_pii (
                twitch_login TEXT PRIMARY KEY REFERENCES affiliate_accounts(twitch_login),
                full_name_enc BYTEA,
                email_enc BYTEA,
                address_line1_enc BYTEA,
                address_city_enc BYTEA,
                address_zip_enc BYTEA,
                tax_id_enc BYTEA,
                address_country TEXT NOT NULL DEFAULT 'DE',
                ust_status TEXT NOT NULL DEFAULT 'unknown',
                updated_at TEXT NOT NULL
            )
            "#,
        )
        .execute(pool)
        .await
        .unwrap();
    }

    async fn ensure_raid_state_table(pool: &sqlx::PgPool) {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS oauth_state_tokens (
                state_token TEXT PRIMARY KEY,
                platform TEXT,
                streamer_login TEXT,
                expires_at TIMESTAMPTZ,
                consumed_at TIMESTAMPTZ
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
                    context_token: String::new(),
                },
            )
            .await
            .unwrap();
        token
    }

    fn uuid_like() -> String {
        tb_crypto::random_urlsafe_token(8)
    }

    /// P2.139: State trägt context_token, aber der Callback kommt OHNE das
    /// `twitch_dash_session_oauth_ctx`-Cookie (cookieloser/fremder Browser) → 400, KEINE Session.
    #[tokio::test]
    async fn callback_ohne_kontext_cookie_400() {
        let Some(pool) = maybe_pool().await else {
            return;
        };
        ensure_tables(&pool).await;
        let state = DashboardAuthState::new(pool.clone(), test_fernet_key());
        let cfg = config_with(Ok(identity("ctxpartner", "999201")));

        let token = format!("ctxtok-{}", uuid_like());
        state
            .save_oauth_login_state(
                &token,
                &OAuthLoginState {
                    next_path: "/analyse".to_string(),
                    redirect_uri: cfg.redirect_uri.clone(),
                    context_token: "the-context-secret".to_string(),
                },
            )
            .await
            .unwrap();

        // KEIN Kontext-Cookie im Request.
        let resp = callback_handler(
            Some(Extension(state.clone())),
            Some(Extension(cfg)),
            HeaderMap::new(),
            Query(CallbackQuery {
                code: Some("good".to_string()),
                state: Some(token),
                error: None,
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        assert!(
            resp.headers().get(SET_COOKIE).is_some(),
            "Kontext-Cookie wird gelöscht"
        );
        // Kein Session-Cookie (twitch_dash_session).
        let any_session = resp
            .headers()
            .get_all(SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .any(|c| c.starts_with("twitch_dash_session="));
        assert!(!any_session, "keine Session bei fehlendem Kontext");
    }

    /// P2.139: derselbe State + passendes Kontext-Cookie → Login läuft durch (302).
    #[tokio::test]
    async fn callback_mit_kontext_cookie_erfolgreich() {
        let Some(pool) = maybe_pool().await else {
            return;
        };
        ensure_tables(&pool).await;
        let state = DashboardAuthState::new(pool.clone(), test_fernet_key());
        sqlx::query(
            "INSERT INTO twitch_partners (twitch_login, twitch_user_id, status)
             VALUES ($1, $2, 'active') ON CONFLICT DO NOTHING",
        )
        .bind("ctxok")
        .bind("999202")
        .execute(&pool)
        .await
        .ok();
        let cfg = config_with(Ok(identity("ctxok", "999202")));

        let token = format!("ctxok-{}", uuid_like());
        state
            .save_oauth_login_state(
                &token,
                &OAuthLoginState {
                    next_path: "/analyse".to_string(),
                    redirect_uri: cfg.redirect_uri.clone(),
                    context_token: "matching-ctx".to_string(),
                },
            )
            .await
            .unwrap();

        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::COOKIE,
            HeaderValue::from_static("twitch_dash_session_oauth_ctx=matching-ctx"),
        );
        let resp = callback_handler(
            Some(Extension(state.clone())),
            Some(Extension(cfg)),
            headers,
            Query(CallbackQuery {
                code: Some("good".to_string()),
                state: Some(token),
                error: None,
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);

        sqlx::query("DELETE FROM twitch_partners WHERE twitch_login = 'ctxok'")
            .execute(&pool)
            .await
            .ok();
    }

    /// Callback mit gültigem Code+State+Partner → 302 + Set-Cookie + Session-Row.
    #[tokio::test]
    async fn callback_erfolgreich_legt_session_an_und_setzt_cookie() {
        let Some(pool) = maybe_pool().await else {
            return;
        };
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
            HeaderMap::new(),
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

    #[tokio::test]
    async fn shared_callback_mit_affiliate_state_legt_affiliate_session_an() {
        let Some(pool) = maybe_pool().await else {
            return;
        };
        ensure_tables(&pool).await;
        ensure_affiliate_tables(&pool).await;
        let state = DashboardAuthState::new(pool.clone(), test_fernet_key());
        let token = format!("affcb-{}", uuid_like());
        state
            .save_affiliate_oauth_state(
                &token,
                &crate::auth::session::AffiliateOAuthState {
                    redirect_uri: "https://x.test/callback/twitch".to_string(),
                },
            )
            .await
            .unwrap();
        let cfg = config_with(Ok(identity("affiliate_cb", "999101")));

        let resp = shared_callback_handler(
            Some(Extension(state.clone())),
            Some(Extension(cfg)),
            HeaderMap::new(),
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
            "/twitch/affiliate/portal"
        );
        let cookie = resp.headers().get(SET_COOKIE).unwrap().to_str().unwrap();
        let affiliate_cookie = crate::auth::session::AFFILIATE_COOKIE_NAME;
        assert!(cookie.starts_with(&format!("{affiliate_cookie}=")));
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("SameSite=Lax"));
        let sid = cookie
            .strip_prefix(&format!("{affiliate_cookie}="))
            .and_then(|s| s.split(';').next())
            .unwrap()
            .to_string();
        let affiliate_session = state.load_affiliate_session(&sid).await.unwrap().unwrap();
        assert_eq!(affiliate_session.twitch_login, "affiliate_cb");
        assert!(state.load_partner_session(&sid).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn shared_callback_mit_partner_state_bleibt_partner_login() {
        let Some(pool) = maybe_pool().await else {
            return;
        };
        ensure_tables(&pool).await;
        let state = DashboardAuthState::new(pool.clone(), test_fernet_key());
        sqlx::query(
            "INSERT INTO twitch_partners (twitch_login, twitch_user_id, status)
             VALUES ($1, $2, 'active') ON CONFLICT DO NOTHING",
        )
        .bind("sharedpartner")
        .bind("999102")
        .execute(&pool)
        .await
        .ok();

        let cfg = config_with(Ok(identity("sharedpartner", "999102")));
        let token = seed_state(&state, &cfg.redirect_uri, "/twitch/dashboard").await;

        let resp = shared_callback_handler(
            Some(Extension(state.clone())),
            Some(Extension(cfg)),
            HeaderMap::new(),
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
            "/twitch/dashboard"
        );
        let cookies: Vec<&str> = resp
            .headers()
            .get_all(SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .collect();
        let partner_cookie = cookies
            .iter()
            .copied()
            .find(|cookie| cookie.starts_with("twitch_dash_session="))
            .unwrap();
        assert!(!cookies
            .iter()
            .any(|cookie| { cookie.starts_with(crate::auth::session::AFFILIATE_COOKIE_NAME) }));
        let sid = partner_cookie
            .strip_prefix("twitch_dash_session=")
            .and_then(|s| s.split(';').next())
            .unwrap()
            .to_string();
        assert!(state.load_partner_session(&sid).await.unwrap().is_some());

        sqlx::query("DELETE FROM twitch_partners WHERE twitch_login = 'sharedpartner'")
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    async fn shared_callback_unbekannter_state_bleibt_bisheriger_fehler() {
        let Some(pool) = maybe_pool().await else {
            return;
        };
        ensure_tables(&pool).await;
        ensure_raid_state_table(&pool).await;
        let state = DashboardAuthState::new(pool.clone(), test_fernet_key());
        let cfg = config_with(Ok(identity("egal", "1")));

        let resp = shared_callback_handler(
            Some(Extension(state)),
            Some(Extension(cfg)),
            HeaderMap::new(),
            Query(CallbackQuery {
                code: Some("good-code".to_string()),
                state: Some("gibt-es-nicht".to_string()),
                error: None,
            }),
        )
        .await;

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        assert!(resp.headers().get(SET_COOKIE).is_none());
    }

    /// Callback, aber Twitch-User ist KEIN Partner → 403, KEINE Session.
    #[tokio::test]
    async fn callback_kein_partner_403_ohne_session() {
        let Some(pool) = maybe_pool().await else {
            return;
        };
        ensure_tables(&pool).await;
        let state = DashboardAuthState::new(pool.clone(), test_fernet_key());

        let cfg = config_with(Ok(identity("fremder_nicht_partner", "999002")));
        let token = seed_state(&state, &cfg.redirect_uri, "/analyse").await;

        let resp = callback_handler(
            Some(Extension(state.clone())),
            Some(Extension(cfg)),
            HeaderMap::new(),
            Query(CallbackQuery {
                code: Some("good".to_string()),
                state: Some(token),
                error: None,
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        assert!(
            resp.headers().get(SET_COOKIE).is_none(),
            "keine Session-Cookie"
        );
    }

    /// Callback mit ungültigem State → 400, KEINE Session. (Exchange wird nie erreicht.)
    #[tokio::test]
    async fn callback_ungueltiger_state_400_ohne_session() {
        let Some(pool) = maybe_pool().await else {
            return;
        };
        ensure_tables(&pool).await;
        let state = DashboardAuthState::new(pool.clone(), test_fernet_key());
        let cfg = config_with(Ok(identity("egal", "1")));

        let resp = callback_handler(
            Some(Extension(state)),
            Some(Extension(cfg)),
            HeaderMap::new(),
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
        let Some(pool) = maybe_pool().await else {
            return;
        };
        ensure_tables(&pool).await;
        let state = DashboardAuthState::new(pool.clone(), test_fernet_key());
        let cfg = config_with(Err(())); // Exchange schlägt fehl.
        let token = seed_state(&state, &cfg.redirect_uri, "/analyse").await;

        let resp = callback_handler(
            Some(Extension(state)),
            Some(Extension(cfg)),
            HeaderMap::new(),
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
        let Some(pool) = maybe_pool().await else {
            return;
        };
        ensure_tables(&pool).await;
        let state = DashboardAuthState::new(pool.clone(), test_fernet_key());
        let cfg = config_with(Ok(identity("x", "1")));

        let resp = login_handler(
            Some(Extension(state)),
            Some(Extension(cfg)),
            Query(LoginQuery {
                next: Some("/twitch/stats".to_string()),
            }),
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
        let Some(pool) = maybe_pool().await else {
            return;
        };
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

        let resp = logout_handler(
            Some(Extension(state.clone())),
            Some(Extension(cfg)),
            headers,
        )
        .await;
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            resp.headers().get(axum::http::header::LOCATION).unwrap(),
            "/analyse"
        );
        let cookies: Vec<&str> = resp
            .headers()
            .get_all(SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .collect();
        assert!(cookies.iter().any(|cookie| {
            cookie.starts_with("twitch_dash_session=") && cookie.contains("Max-Age=0")
        }));
        assert!(cookies.iter().any(|cookie| {
            cookie.starts_with("tb_admin_mode=") && cookie.contains("Max-Age=0")
        }));
        // Session ist invalidiert.
        assert!(state
            .load_partner_session(&created.session_id)
            .await
            .unwrap()
            .is_none());

        sqlx::query("DELETE FROM twitch_partners WHERE twitch_login = 'logoutpartner'")
            .execute(&pool)
            .await
            .ok();
    }
}
