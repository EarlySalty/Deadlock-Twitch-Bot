//! Public, viewer-only Twitch + Steam linking. No partner access or Discord required.
use super::auth_login::{LoginQuery, OAuthLoginConfig};
use crate::auth::{
    oauth_login::TwitchIdentity,
    session::{
        build_session_cookie, session_lookup_key, DashboardAuthState, PlayerSession,
        PlayerSteamFlow, SameSite, PLAYER_COOKIE_NAME, PLAYER_SESSION_TTL,
    },
    steam_openid::{self, OpenIdError, SteamOpenIdClient},
};
use axum::{
    extract::{Extension, RawQuery},
    http::{
        header::{LOCATION, SET_COOKIE},
        HeaderMap, HeaderValue, StatusCode,
    },
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
    Form, Router,
};
use serde::Deserialize;
use tb_chat::player_links::{self, CONNECT_PATH};

pub fn router() -> Router {
    Router::new()
        .route(CONNECT_PATH, get(page))
        .route("/twitch/connect/", get(page))
        .route("/twitch/connect/twitch", get(twitch_login))
        .route("/twitch/connect/steam", post(steam_start))
        .route("/twitch/connect/steam/callback", get(steam_callback))
        .route("/twitch/connect/unlink", post(unlink))
}

fn secure_response(mut response: Response) -> Response {
    let headers = response.headers_mut();
    headers.insert("cache-control", HeaderValue::from_static("no-store"));
    headers.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert("content-security-policy", HeaderValue::from_static("default-src 'none'; style-src 'unsafe-inline'; form-action 'self' https://steamcommunity.com; base-uri 'none'; frame-ancestors 'none'"));
    response
}
fn error(status: StatusCode, message: &str) -> Response {
    secure_response((status, Html(render_page(&format!("<h2>Verknüpfung nicht abgeschlossen</h2><p>{}</p><a class=button href=/twitch/connect>Zurück zur Kontoverknüpfung</a>", escape(message))))).into_response())
}
fn unavailable() -> Response {
    error(
        StatusCode::SERVICE_UNAVAILABLE,
        "Die Kontoverknüpfung ist gerade nicht erreichbar. Bitte erneut versuchen.",
    )
}
fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    let mut values = headers
        .get_all("cookie")
        .iter()
        .filter_map(|h| h.to_str().ok())
        .flat_map(|header| header.split(';'))
        .filter_map(|part| part.trim().split_once('='))
        .filter(|(key, _)| *key == name)
        .map(|(_, value)| value.to_string());
    let value = values.next()?;
    if values.next().is_some() || value.is_empty() || value.len() > 128 {
        return None;
    }
    Some(value)
}
fn origin(config: &OAuthLoginConfig) -> Option<String> {
    let url = url::Url::parse(&config.redirect_uri).ok()?;
    (url.scheme() == "https"
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none())
    .then(|| url.origin().ascii_serialization())
}
fn connect_origin() -> Option<String> {
    let url = url::Url::parse(player_links::CONNECT_URL).ok()?;
    (url.scheme() == "https" && url.host_str().is_some())
        .then(|| url.origin().ascii_serialization())
}
fn allowed_form_origins(config: &OAuthLoginConfig) -> Vec<String> {
    let mut origins = Vec::new();
    if let Some(o) = connect_origin() {
        origins.push(o);
    }
    if let Some(o) = origin(config) {
        if !origins.contains(&o) {
            origins.push(o);
        }
    }
    origins
}
async fn session(
    state: &DashboardAuthState,
    headers: &HeaderMap,
) -> Result<Option<(String, PlayerSession)>, sqlx::Error> {
    let Some(token) = cookie(headers, PLAYER_COOKIE_NAME) else {
        return Ok(None);
    };
    Ok(state.load_player_session(&token).await?.map(|s| (token, s)))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectForm {
    csrf_token: String,
}
fn valid_form(
    headers: &HeaderMap,
    config: &OAuthLoginConfig,
    session: &PlayerSession,
    form: &ConnectForm,
) -> bool {
    let allowed = allowed_form_origins(config);
    if allowed.is_empty() {
        return false;
    }
    if let Some(value) = headers.get("origin") {
        let matches = value
            .to_str()
            .ok()
            .is_some_and(|v| allowed.iter().any(|o| o == v));
        if !matches {
            return false;
        }
    }
    if headers.get("sec-fetch-site").and_then(|v| v.to_str().ok()) == Some("cross-site") {
        return false;
    }
    !form.csrf_token.is_empty()
        && tb_crypto::constant_time_eq(form.csrf_token.as_bytes(), session.csrf_token.as_bytes())
}

pub async fn page(state: Option<Extension<DashboardAuthState>>, headers: HeaderMap) -> Response {
    let Some(Extension(state)) = state else {
        return unavailable();
    };
    let session = match session(&state, &headers).await {
        Ok(s) => s,
        Err(_) => return unavailable(),
    };
    let body = if let Some((_, session)) = session {
        let link = match player_links::load(state.pool(), &session.twitch_user_id).await {
            Ok(link) => link,
            Err(_) => return unavailable(),
        };
        let who = escape(&session.twitch_login);
        let csrf = escape(&session.csrf_token);
        let status = match link.as_ref() {
            Some(link) if !link.lookup_enabled => "<div class=notice>Deine Steam-Zuordnung ist deaktiviert. Automatische Zuordnung über Discord oder Namenssuche bleibt aus, bis du hier erneut verbindest.</div>".to_string(),
            Some(link) if link.steam_id64.is_some() => format!("<div class=success><strong>Verbunden</strong><p>Steam-ID: {}<br>Im Chat: <code>!rank @{who}</code></p></div>", link.steam_id64.unwrap_or_default()),
            _ => "<p>Dein Twitch-Konto ist bestätigt. Verbinde jetzt deinen Steam-Account.</p>".to_string(),
        };
        let button = if link.as_ref().and_then(|l| l.steam_id64).is_some() {
            "Steam-Konto wechseln"
        } else {
            "Mit Steam verbinden"
        };
        format!("<div class=eyebrow>TWITCH BESTÄTIGT</div><h2>@{who}</h2>{status}
            <p>Mit dem nächsten Schritt verknüpfst du dieses Twitch-Konto mit dem Steam-Konto, das du bei Steam bestätigst. Der Bot darf damit deinen verfügbaren Deadlock-Rang auf <code>!rank @{who}</code> öffentlich im Chat anzeigen.</p>
            <form method=post action=/twitch/connect/steam><input type=hidden name=csrf_token value=\"{csrf}\"><button type=submit>{button}</button></form>
            <p class=muted>Wir speichern die Twitch-ID und die bestätigte Steam-ID. Steam-Passwort und Inventarberechtigungen erhalten wir nicht. Der Steam-Login garantiert keine Rangdaten; diese müssen in der Deadlock API verfügbar sein.</p>
            <div class=actions><a href=/twitch/connect/twitch>Anderes Twitch-Konto</a>
            <form method=post action=/twitch/connect/unlink><input type=hidden name=csrf_token value=\"{csrf}\"><button class=secondary type=submit>Verknüpfung entfernen</button></form></div>")
    } else {
        "<div class=eyebrow>FÜR ALLE ZUSCHAUER</div><h2>Dein Rang. Dein Account.</h2><p>Verbinde Twitch und Steam, damit <code>!rank @deinname</code> deinen richtigen Deadlock-Account findet – auch bei unterschiedlichen Namen.</p><ol><li>Twitch-Konto bestätigen</li><li>Bei Steam anmelden und Verbindung bestätigen</li><li>Im Chat <code>!rank @deinname</code> nutzen</li></ol><a class=button href=/twitch/connect/twitch>Mit Twitch anmelden</a><p class=muted>Kein Discord-Konto und keine Streamer-Partnerschaft nötig. Freiwillig; mit <code>!unconnect</code> im Chat oder hier auf der Seite wieder trennbar.</p>".to_string()
    };
    secure_response(Html(render_page(&body)).into_response())
}

pub async fn twitch_login(
    state: Option<Extension<DashboardAuthState>>,
    config: Option<Extension<OAuthLoginConfig>>,
) -> Response {
    let mut response = super::auth_login::login_handler(
        state,
        config,
        axum::extract::Query(LoginQuery {
            next: Some(CONNECT_PATH.into()),
        }),
    )
    .await;
    if let Some(location) = response
        .headers()
        .get(LOCATION)
        .and_then(|h| h.to_str().ok())
    {
        if let Ok(mut url) = url::Url::parse(location) {
            if url.host_str() == Some("id.twitch.tv") {
                url.query_pairs_mut().append_pair("force_verify", "true");
                if let Ok(value) = HeaderValue::from_str(url.as_str()) {
                    response.headers_mut().insert(LOCATION, value);
                }
            }
        }
    }
    secure_response(response)
}

/// Called only after the existing OAuth state + browser context + code checks.
/// This must NOT create a partner session or confer dashboard privileges.
pub async fn complete_twitch_login(
    state: &DashboardAuthState,
    config: &OAuthLoginConfig,
    identity: TwitchIdentity,
) -> Response {
    if identity.twitch_user_id.is_empty() || identity.twitch_login.is_empty() {
        return unavailable();
    }
    let created = match state
        .create_player_session(&identity.twitch_user_id, &identity.twitch_login)
        .await
    {
        Ok(created) => created,
        Err(_) => return unavailable(),
    };
    let mut response = Redirect::to(CONNECT_PATH).into_response();
    let header = build_session_cookie(
        PLAYER_COOKIE_NAME,
        &created.session_id,
        config.cookie_secure,
        SameSite::Lax,
        PLAYER_SESSION_TTL,
    );
    match HeaderValue::from_str(&header) {
        Ok(value) => {
            response.headers_mut().append(SET_COOKIE, value);
        }
        Err(_) => return unavailable(),
    }
    secure_response(response)
}

pub async fn steam_start(
    state: Option<Extension<DashboardAuthState>>,
    config: Option<Extension<OAuthLoginConfig>>,
    headers: HeaderMap,
    Form(form): Form<ConnectForm>,
) -> Response {
    let (Some(Extension(state)), Some(Extension(config))) = (state, config) else {
        return unavailable();
    };
    let (token, session) = match session(&state, &headers).await {
        Ok(Some(s)) => s,
        Ok(None) => {
            return error(
                StatusCode::UNAUTHORIZED,
                "Bitte zuerst mit Twitch anmelden.",
            )
        }
        Err(_) => return unavailable(),
    };
    if !valid_form(&headers, &config, &session, &form) {
        return error(
            StatusCode::FORBIDDEN,
            "Die Bestätigung ist ungültig. Bitte die Seite neu öffnen.",
        );
    }
    let Some(origin) = connect_origin().or_else(|| origin(&config)) else {
        return unavailable();
    };
    let state_token = tb_crypto::random_urlsafe_token(32);
    let return_to = format!("{origin}/twitch/connect/steam/callback?state={state_token}");
    let revision = match player_links::prepare(state.pool(), &session.twitch_user_id).await {
        Ok(revision) => revision,
        Err(_) => return unavailable(),
    };
    let flow = PlayerSteamFlow {
        session_key: session_lookup_key(&token),
        twitch_user_id: session.twitch_user_id,
        revision,
        return_to: return_to.clone(),
        expires_at: (chrono::Utc::now().timestamp() + 600) as u64,
    };
    if state
        .save_player_steam_flow(&state_token, &flow)
        .await
        .is_err()
    {
        return unavailable();
    }
    match steam_openid::authorize_url(&return_to) {
        Ok(url) => secure_response(Redirect::to(&url).into_response()),
        Err(_) => unavailable(),
    }
}

pub async fn steam_callback(
    state: Option<Extension<DashboardAuthState>>,
    client: Option<Extension<SteamOpenIdClient>>,
    headers: HeaderMap,
    RawQuery(raw): RawQuery,
) -> Response {
    let Some(Extension(state)) = state else {
        return unavailable();
    };
    let (token, session) = match session(&state, &headers).await {
        Ok(Some(s)) => s,
        Ok(None) => {
            return error(
                StatusCode::UNAUTHORIZED,
                "Deine Twitch-Anmeldung ist abgelaufen. Bitte neu anmelden.",
            )
        }
        Err(_) => return unavailable(),
    };
    let params = match steam_openid::parse_query(raw.as_deref().unwrap_or("")) {
        Ok(params) => params,
        Err(_) => return error(StatusCode::BAD_REQUEST, "Die Steam-Antwort ist ungültig."),
    };
    let Some(state_token) = params
        .get("state")
        .filter(|s| !s.is_empty() && s.len() <= 128)
    else {
        return error(StatusCode::BAD_REQUEST, "Der Verknüpfungsauftrag fehlt.");
    };
    let flow = match state.consume_player_steam_flow(state_token).await {
        Ok(Some(flow)) => flow,
        Ok(None) => {
            return error(
                StatusCode::BAD_REQUEST,
                "Dieser Auftrag ist abgelaufen oder wurde bereits verwendet.",
            )
        }
        Err(_) => return unavailable(),
    };
    if flow.twitch_user_id != session.twitch_user_id
        || !tb_crypto::constant_time_eq(
            flow.session_key.as_bytes(),
            session_lookup_key(&token).as_bytes(),
        )
    {
        return error(
            StatusCode::FORBIDDEN,
            "Die Verknüpfung gehört zu einer anderen Anmeldung. Bitte erneut starten.",
        );
    }
    if params.get("openid.mode").map(String::as_str) == Some("cancel") {
        return error(
            StatusCode::BAD_REQUEST,
            "Die Steam-Anmeldung wurde abgebrochen. Es wurde nichts verknüpft.",
        );
    }
    let client = client.map(|Extension(c)| c).unwrap_or_default();
    let (steam_id64, nonce) = match client.verify(&params, &flow.return_to).await {
        Ok(verified) => verified,
        Err(OpenIdError::Unavailable) => return unavailable(),
        Err(OpenIdError::Invalid) => {
            return error(
                StatusCode::BAD_REQUEST,
                "Steam konnte die Anmeldung nicht bestätigen. Bitte erneut starten.",
            )
        }
    };
    match player_links::complete(state.pool(), &session.twitch_user_id, flow.revision, steam_id64, &nonce).await {
        Ok(true) => secure_response(Redirect::to(CONNECT_PATH).into_response()),
        Ok(false) => error(StatusCode::CONFLICT, "Die Zuordnung wurde zwischenzeitlich geändert oder diese Steam-Antwort bereits verwendet. Bitte erneut starten."),
        Err(_) => unavailable(),
    }
}

pub async fn unlink(
    state: Option<Extension<DashboardAuthState>>,
    config: Option<Extension<OAuthLoginConfig>>,
    headers: HeaderMap,
    Form(form): Form<ConnectForm>,
) -> Response {
    let (Some(Extension(state)), Some(Extension(config))) = (state, config) else {
        return unavailable();
    };
    let (_, session) = match session(&state, &headers).await {
        Ok(Some(s)) => s,
        Ok(None) => {
            return error(
                StatusCode::UNAUTHORIZED,
                "Bitte zuerst mit Twitch anmelden.",
            )
        }
        Err(_) => return unavailable(),
    };
    if !valid_form(&headers, &config, &session, &form) {
        return error(
            StatusCode::FORBIDDEN,
            "Die Bestätigung ist ungültig. Bitte die Seite neu öffnen.",
        );
    }
    match player_links::disconnect(state.pool(), &session.twitch_user_id).await {
        Ok(()) => secure_response(Redirect::to(CONNECT_PATH).into_response()),
        Err(_) => unavailable(),
    }
}
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
fn render_page(content: &str) -> String {
    include_str!("player_connect/page.html").replace("{{CONTENT}}", content)
}

#[cfg(test)]
mod tests;
