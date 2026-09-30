//! Twitch-eigener Admin-Editor, gemeinsamer sicherer TOML-Speicherkern in dl-web.
//! Bestehende Sessions, sessiongebundene CSRF-Tokens und Dienstauth; keine
//! zweite Konfigurationsdatei und keine neuen Zugangsdaten.
use crate::auth::{
    level::{AuthenticatedAdminSessionId, AuthenticatedPartnerSessionId, DashboardAuthLevel},
    session::DashboardAuthState,
};
use axum::{
    body::Bytes,
    extract::{Extension, Path},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};

fn no_store(mut response: Response) -> Response {
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
}
fn error(status: StatusCode, message: &str) -> Response {
    no_store((status, Json(json!({"error": message, "message": message}))).into_response())
}

async fn session_proof(
    auth: &DashboardAuthLevel,
    state: Option<Extension<DashboardAuthState>>,
    admin: Option<Extension<AuthenticatedAdminSessionId>>,
    partner: Option<Extension<AuthenticatedPartnerSessionId>>,
) -> Result<(String, String), Response> {
    if let Some(error) = crate::auth::require_admin(auth) {
        return Err(no_store(error.into_response()));
    }
    let Some(Extension(state)) = state else {
        return Err(error(
            StatusCode::UNAUTHORIZED,
            "Bitte erneut im Admin-Dashboard anmelden.",
        ));
    };
    let (csrf, actor) = if let Some(Extension(admin)) = admin {
        let token = state.admin_csrf_token(&admin.0).await.ok().flatten();
        let actor = state
            .load_admin_session_user_id(&admin.0)
            .await
            .ok()
            .flatten()
            .unwrap_or_default();
        (token, actor)
    } else if let Some(Extension(partner)) = partner {
        let token = state.partner_csrf_token(&partner.0).await.ok().flatten();
        let actor = match auth {
            DashboardAuthLevel::Admin { actor: Some(actor) } => actor.twitch_user_id.clone(),
            _ => String::new(),
        };
        (token, actor)
    } else {
        (None, String::new())
    };
    let Some(csrf) = csrf.filter(|value| !value.is_empty()) else {
        return Err(error(
            StatusCode::UNAUTHORIZED,
            "Die Admin-Sitzung ist nicht mehr gültig. Bitte erneut anmelden.",
        ));
    };
    Ok((csrf, actor))
}

fn strict_origin(headers: &HeaderMap) -> bool {
    let Some(host) = headers.get(header::HOST).and_then(|value| value.to_str().ok()) else { return false; };
    let Some(origin) = headers.get(header::ORIGIN).and_then(|value| value.to_str().ok()) else { return false; };
    let (Ok(expected), Ok(origin)) = (url::Url::parse(&format!("https://{host}")), url::Url::parse(origin)) else { return false; };
    origin.scheme() == "https" && origin.origin() == expected.origin()
        && origin.username().is_empty() && origin.password().is_none()
        && origin.path() == "/" && origin.query().is_none() && origin.fragment().is_none()
}

fn write_allowed(headers: &HeaderMap, expected: &str) -> bool {
    let presented = headers
        .get("X-CSRF-Token")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    !expected.is_empty()
        && !presented.is_empty()
        && tb_crypto::constant_time_eq(expected.as_bytes(), presented.as_bytes())
        && strict_origin(headers)
        && headers
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| {
                value
                    .split(';')
                    .next()
                    .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("application/json"))
            })
}

async fn upstream(
    path: &str,
    body: Option<Bytes>,
    actor: &str,
    limit: usize,
) -> Result<(StatusCode, Bytes), Response> {
    use crate::auth::discord_admin_login::{internal_token_from_env, BROKER_BASE_URL};
    let Some(token) = internal_token_from_env() else {
        return Err(error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Die interne Konfigurationsverbindung ist nicht verfügbar.",
        ));
    };
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(25))
        .build()
        .map_err(|_| {
            error(
                StatusCode::SERVICE_UNAVAILABLE,
                "Die interne Konfigurationsverbindung ist nicht verfügbar.",
            )
        })?;
    let url = format!("{BROKER_BASE_URL}{path}");
    let request = match body {
        Some(body) => client
            .post(&url)
            .header(header::CONTENT_TYPE, "application/json")
            .body(body),
        None => client.get(&url),
    };
    let mut request = request.header("X-Internal-Token", token);
    if !actor.is_empty() && actor.len() <= 24 && actor.bytes().all(|value| value.is_ascii_digit()) {
        request = request.header("X-Admin-Actor", actor);
    }
    let mut response = request.send().await.map_err(|_| error(StatusCode::SERVICE_UNAVAILABLE,
        "Der Konfigurationsdienst ist vorübergehend nicht erreichbar. Dein Entwurf bleibt erhalten."))?;
    let status = response.status();
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| {
        error(
            StatusCode::BAD_GATEWAY,
            "Die Antwort des Konfigurationsdienstes ist unvollständig.",
        )
    })? {
        if bytes.len() + chunk.len() > limit {
            return Err(error(
                StatusCode::BAD_GATEWAY,
                "Die Antwort des Konfigurationsdienstes ist zu groß.",
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok((status, Bytes::from(bytes)))
}

async fn json_proxy(body: Option<Bytes>, actor: &str, csrf: Option<&str>) -> Response {
    let (status, bytes) = match upstream(
        "/internal/twitch/v1/bot-config",
        body,
        actor,
        2 * 1024 * 1024,
    )
    .await
    {
        Ok(result) => result,
        Err(response) => return response,
    };
    if matches!(
        status,
        StatusCode::NOT_FOUND | StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
    ) {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Die Twitch-TOML ist noch nicht mit dem Konfigurationsdienst verbunden.",
        );
    }
    let Ok(mut value) = serde_json::from_slice::<Value>(&bytes) else {
        return error(
            StatusCode::BAD_GATEWAY,
            "Die Antwort des Konfigurationsdienstes konnte nicht geprüft werden.",
        );
    };
    if let (Some(token), Some(object)) = (csrf, value.as_object_mut()) {
        if status.is_success() {
            object.insert("csrf_token".into(), Value::String(token.into()));
        }
    }
    no_store((status, Json(value)).into_response())
}

pub async fn get(
    auth: DashboardAuthLevel,
    state: Option<Extension<DashboardAuthState>>,
    admin: Option<Extension<AuthenticatedAdminSessionId>>,
    partner: Option<Extension<AuthenticatedPartnerSessionId>>,
) -> Response {
    let (csrf, actor) = match session_proof(&auth, state, admin, partner).await {
        Ok(proof) => proof,
        Err(response) => return response,
    };
    json_proxy(None, &actor, Some(&csrf)).await
}

pub async fn mutate(
    auth: DashboardAuthLevel,
    state: Option<Extension<DashboardAuthState>>,
    admin: Option<Extension<AuthenticatedAdminSessionId>>,
    partner: Option<Extension<AuthenticatedPartnerSessionId>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let (csrf, actor) = match session_proof(&auth, state, admin, partner).await {
        Ok(proof) => proof,
        Err(response) => return response,
    };
    if !write_allowed(&headers, &csrf) {
        return error(
            StatusCode::FORBIDDEN,
            "Die Sicherheitsprüfung ist fehlgeschlagen. Bitte den gespeicherten Stand neu laden.",
        );
    }
    if body.len() > 256 * 1024 * 6 + 1024 {
        return error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "Die Konfigurationsdatei ist zu groß.",
        );
    }
    json_proxy(Some(body), &actor, None).await
}

pub async fn ui(auth: DashboardAuthLevel, Path(name): Path<String>) -> Response {
    if let Some(error) = crate::auth::require_admin(&auth) {
        return no_store(error.into_response());
    }
    let mime = match name.as_str() {
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        _ => return no_store(StatusCode::NOT_FOUND.into_response()),
    };
    let path = format!("/internal/twitch/v1/bot-config-editor/{name}");
    let (status, body) = match upstream(&path, None, "", 128 * 1024).await {
        Ok(result) => result,
        Err(response) => return response,
    };
    if status != StatusCode::OK || std::str::from_utf8(&body).is_err() {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Die Konfigurationsoberfläche ist noch nicht verfügbar.",
        );
    }
    no_store(([(header::CONTENT_TYPE, mime)], body).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn no_admin_or_no_session_never_reaches_files_or_upstream() {
        let partner = DashboardAuthLevel::Partner {
            twitch_login: "synthetic".into(),
            twitch_user_id: "42".into(),
            display_name: String::new(),
        };
        for (auth, expected) in [
            (DashboardAuthLevel::None, StatusCode::UNAUTHORIZED),
            (partner, StatusCode::FORBIDDEN),
            (DashboardAuthLevel::admin(), StatusCode::UNAUTHORIZED),
        ] {
            assert_eq!(get(auth, None, None, None).await.status(), expected);
        }
    }
    #[test]
    fn writes_require_bound_token_origin_and_json() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::HOST,
            HeaderValue::from_static("admin.deutsche-deadlock-community.de"),
        );
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://admin.deutsche-deadlock-community.de"),
        );
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        headers.insert(
            "x-csrf-token",
            HeaderValue::from_static("synthetic-csrf-value"),
        );
        assert!(write_allowed(&headers, "synthetic-csrf-value"));
        assert!(!write_allowed(&headers, "different"));
        assert!(!write_allowed(&headers, ""));
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://example.invalid"),
        );
        assert!(!write_allowed(&headers, "synthetic-csrf-value"));
        headers.remove(header::ORIGIN);
        assert!(!write_allowed(&headers, "synthetic-csrf-value"));
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://admin.deutsche-deadlock-community.de"),
        );
        headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/plain"));
        assert!(!write_allowed(&headers, "synthetic-csrf-value"));
    }
}
