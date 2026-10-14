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
        .route("/twitch/connect/primary", post(set_primary))
        .route("/twitch/connect/remove", post(remove_account))
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
    headers.insert("content-security-policy", HeaderValue::from_static("default-src 'none'; style-src 'unsafe-inline'; font-src 'self'; img-src 'self'; form-action 'self' https://steamcommunity.com; base-uri 'none'; frame-ancestors 'none'"));
    response
}
fn stepper(stage: u8) -> String {
    const LABELS: [&str; 3] = ["Twitch bestätigen", "Steam verbinden", "Fertig"];
    let steps = LABELS
        .iter()
        .enumerate()
        .map(|(index, label)| {
            let number = (index + 1) as u8;
            let class = if number < stage {
                "flow-step done"
            } else if number == stage {
                "flow-step active"
            } else {
                "flow-step"
            };
            let marker = if number < stage { "✓".to_string() } else { number.to_string() };
            let current = if number == stage { " aria-current=\"step\"" } else { "" };
            format!(
                "<div class=\"{class}\"{current}><span class=step-dot>{marker}</span><span class=step-label>{label}</span></div>"
            )
        })
        .collect::<String>();
    format!("<div class=flow-steps aria-label=\"Verknüpfungsfortschritt\">{steps}</div>")
}

fn error(status: StatusCode, message: &str) -> Response {
    let body = format!(
        "<div class=panel><div class=error-panel><div class=error-icon aria-hidden=true>!</div><div class=eyebrow>Verknüpfung unterbrochen</div><h2>Das hat noch nicht geklappt.</h2><p>{}</p><a class=button href=/twitch/connect>Zurück zur Kontoverknüpfung</a></div></div>",
        escape(message)
    );
    secure_response((status, Html(render_page(&body))).into_response())
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountForm {
    csrf_token: String,
    steam_id64: i64,
}

fn normalized_origin(value: &str) -> Option<String> {
    let url = url::Url::parse(value).ok()?;
    (matches!(url.scheme(), "https" | "http")
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none())
    .then(|| url.origin().ascii_serialization())
}

fn valid_form(
    headers: &HeaderMap,
    config: &OAuthLoginConfig,
    session: &PlayerSession,
    form: &ConnectForm,
) -> bool {
    if form.csrf_token.is_empty()
        || !tb_crypto::constant_time_eq(form.csrf_token.as_bytes(), session.csrf_token.as_bytes())
    {
        return false;
    }

    let fetch_site = headers
        .get("sec-fetch-site")
        .and_then(|value| value.to_str().ok());
    if fetch_site == Some("cross-site") {
        return false;
    }

    // Moderne Browser liefern Fetch-Metadata. Wenn der Browser selbst den POST
    // als same-origin/same-site einordnet, ist die starke Session-CSRF-Bindung
    // maßgeblich. Das hält den Flow auch hinter Canonical-Host-/Proxy-Redirects
    // stabil, bei denen ein streng verglichener Origin-String abweichen kann.
    if matches!(fetch_site, Some("same-origin" | "same-site")) {
        return true;
    }

    let allowed = allowed_form_origins(config);
    if allowed.is_empty() {
        return false;
    }
    if let Some(value) = headers.get("origin") {
        let Some(presented) = value.to_str().ok().and_then(normalized_origin) else {
            return false;
        };
        return allowed.iter().any(|allowed| allowed == &presented);
    }

    // Ältere Clients ohne Origin/Fetch-Metadata bleiben über den zufälligen,
    // sessiongebundenen CSRF-Token geschützt.
    true
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
        let steam_accounts =
            match player_links::accounts(state.pool(), &session.twitch_user_id).await {
                Ok(accounts) => accounts,
                Err(_) => return unavailable(),
            };
        let who = escape(&session.twitch_login);
        let csrf = escape(&session.csrf_token);
        let steam_id = link.as_ref().and_then(|link| link.steam_id64);
        let lookup_enabled = link.as_ref().is_some_and(|link| link.lookup_enabled);
        let ready = lookup_enabled && steam_id.is_some();
        let stage = if ready { 3 } else { 2 };
        let account_list = steam_accounts.iter().map(|account| {
            let primary = if account.is_primary { "<strong>Standardkonto</strong>" } else { "" };
            let primary_action = if account.is_primary { String::new() } else {
                format!("<form method=post action=/twitch/connect/primary><input type=hidden name=csrf_token value=\"{csrf}\"><input type=hidden name=steam_id64 value=\"{}\"><button class=secondary type=submit>Als Standard</button></form>", account.steam_id64)
            };
            format!("<div class=status-box><p><code>{}</code> {primary}</p><div class=actions>{primary_action}<form method=post action=/twitch/connect/remove><input type=hidden name=csrf_token value=\"{csrf}\"><input type=hidden name=steam_id64 value=\"{}\"><button class=secondary type=submit>Entfernen</button></form></div></div>", account.steam_id64, account.steam_id64)
        }).collect::<String>();
        let status = match link.as_ref() {
            Some(link) if !link.lookup_enabled => "<div class=\"status-box notice\"><strong>Verknüpfung pausiert</strong><p>Deine bisherige Steam-Zuordnung ist deaktiviert. Verbinde Steam erneut, um die automatische Rang-Zuordnung wieder einzuschalten.</p></div>".to_string(),
            Some(link) if link.steam_id64.is_some() => format!(
                "<div class=\"status-box success\"><strong>{} Steam-Konto/Konten verbunden</strong><p>Das Standardkonto wird für normale Rangabfragen verwendet.</p><div class=success-code><span>Im Chat</span><code>!rank me</code></div></div>{account_list}",
                steam_accounts.len()
            ),
            _ => "<p class=supporting>Dein Twitch-Konto ist bestätigt. Jetzt fehlt nur noch dein Steam-Konto.</p>".to_string(),
        };
        let button = if !lookup_enabled {
            "Steam wieder verbinden"
        } else if steam_id.is_some() {
            "Weiteres Steam-Konto verbinden"
        } else {
            "Mit Steam verbinden"
        };
        let headline = if ready {
            "Verbindung steht."
        } else {
            "Jetzt Steam verbinden."
        };
        let side = if ready {
            "<aside class=side-card><h3>So nutzt du es</h3><ul class=side-list><li><span class=check>✓</span><span>Im Chat einfach <code>!rank me</code> schreiben.</span></li><li><span class=check>✓</span><span>Du kannst mehrere Steam-Konten verbinden und eines als Standard wählen.</span></li><li><span class=check>✓</span><span>Einzelne Konten oder die komplette Verknüpfung lassen sich jederzeit entfernen.</span></li></ul></aside>".to_string()
        } else {
            "<aside class=side-card><h3>Was passiert jetzt?</h3><ul class=side-list><li><span class=check>✓</span><span>Der Login öffnet direkt Steam.</span></li><li><span class=check>✓</span><span>Wir bekommen nur deine bestätigte öffentliche Steam-ID.</span></li><li><span class=check>✓</span><span>Passwort und Inventarberechtigungen sehen wir nicht.</span></li></ul></aside>".to_string()
        };
        format!(
            "{}<div class=panel><div class=panel-grid><div class=panel-main><div class=account-chip><span class=account-avatar aria-hidden=true>T</span><span>@{who}</span></div><div class=eyebrow>{}</div><h2>{headline}</h2>{status}<p>Damit darf der Bot deinen verfügbaren Deadlock-Rang deinem Twitch-Konto zuordnen. Mit <code>!rank me</code> fragst du im Chat dein Standardkonto ab.</p><form method=post action=/twitch/connect/steam><input type=hidden name=csrf_token value=\"{csrf}\"><button type=submit>{button}</button></form><p class=muted>Jeder weitere Steam-Login ergänzt ein Konto und macht es zunächst zum Standard. Der Steam-Login garantiert keine Rangdaten; sie müssen in der Deadlock API verfügbar sein.</p><div class=actions><a href=/twitch/connect/twitch>Anderes Twitch-Konto verwenden</a><form method=post action=/twitch/connect/unlink><input type=hidden name=csrf_token value=\"{csrf}\"><button class=secondary type=submit>Alle Steam-Verknüpfungen entfernen</button></form></div></div>{side}</div></div>",
            stepper(stage),
            if ready { "VERBUNDEN" } else { "SCHRITT 2 VON 3" }
        )
    } else {
        format!(
            "{}<div class=panel><div class=panel-grid><div class=panel-main><div class=eyebrow>SCHRITT 1 VON 3</div><h2>Twitch-Konto bestätigen.</h2><p>Wir gleichen zuerst deinen Twitch-Account ab. So weiß der Bot später genau, zu welchem Chat-Namen dein bestätigter Deadlock-Account gehört – auch wenn Twitch- und Steam-Name verschieden sind.</p><a class=button href=/twitch/connect/twitch>Mit Twitch anmelden</a><p class=muted>Freiwillig. Kein Discord-Konto und keine Streamer-Partnerschaft nötig.</p></div><aside class=side-card><h3>Danach geht es so weiter</h3><ul class=side-list><li><span class=check>2</span><span>Bei Steam anmelden und den gewünschten Account bestätigen.</span></li><li><span class=check>3</span><span>Im Chat <code>!rank @deinname</code> nutzen.</span></li><li><span class=check>✓</span><span>Mit <code>!unconnect</code> oder hier jederzeit wieder trennbar.</span></li></ul></aside></div></div>",
            stepper(1)
        )
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

pub async fn set_primary(
    state: Option<Extension<DashboardAuthState>>,
    config: Option<Extension<OAuthLoginConfig>>,
    headers: HeaderMap,
    Form(form): Form<AccountForm>,
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
    let csrf_form = ConnectForm {
        csrf_token: form.csrf_token.clone(),
    };
    if !valid_form(&headers, &config, &session, &csrf_form) {
        return error(
            StatusCode::FORBIDDEN,
            "Die Bestätigung ist ungültig. Bitte die Seite neu öffnen.",
        );
    }
    match player_links::set_primary(state.pool(), &session.twitch_user_id, form.steam_id64).await {
        Ok(true) => secure_response(Redirect::to(CONNECT_PATH).into_response()),
        Ok(false) => error(
            StatusCode::NOT_FOUND,
            "Dieses Steam-Konto gehört nicht zu deiner Verknüpfung.",
        ),
        Err(_) => unavailable(),
    }
}

pub async fn remove_account(
    state: Option<Extension<DashboardAuthState>>,
    config: Option<Extension<OAuthLoginConfig>>,
    headers: HeaderMap,
    Form(form): Form<AccountForm>,
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
    let csrf_form = ConnectForm {
        csrf_token: form.csrf_token.clone(),
    };
    if !valid_form(&headers, &config, &session, &csrf_form) {
        return error(
            StatusCode::FORBIDDEN,
            "Die Bestätigung ist ungültig. Bitte die Seite neu öffnen.",
        );
    }
    match player_links::remove_account(state.pool(), &session.twitch_user_id, form.steam_id64).await
    {
        Ok(true) => secure_response(Redirect::to(CONNECT_PATH).into_response()),
        Ok(false) => error(
            StatusCode::NOT_FOUND,
            "Dieses Steam-Konto gehört nicht zu deiner Verknüpfung.",
        ),
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
