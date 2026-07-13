//! Geteilte Helfer für die Legacy-Admin-Form-POST-Handler (`submitLegacyAction`,
//! `admin_dashboard/.../client.ts`).
//!
//! Der Legacy-Admin-Client sendet `application/x-www-form-urlencoded` inkl.
//! `csrf_token` IM BODY (nicht im `X-CSRF-Token`-Header). Diese Routen dürfen
//! daher NICHT durch die Header-basierte `csrf_protect`-Middleware laufen; sie
//! validieren den Body-CSRF selbst gegen die Session (Localhost-Bypass), genau
//! wie Pythons `_read_post_with_csrf`. Auth + CSRF sind für alle diese Handler
//! identisch (`_require_token` → Admin/Localhost), darum hier zentral.

use axum::{
    extract::Extension,
    response::{IntoResponse, Redirect, Response},
};

use crate::auth::level::{cookie_values, DashboardAuthLevel};
use crate::auth::session::{DashboardAuthState, ADMIN_COOKIE_NAME, PARTNER_COOKIE_NAME};

/// Parst `application/x-www-form-urlencoded` in Key/Value-Paare.
pub(crate) fn parse_form(body: &[u8]) -> Vec<(String, String)> {
    url::form_urlencoded::parse(body)
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}

/// Liest einen Form-Wert (leerer String, wenn nicht vorhanden).
pub(crate) fn form_get<'a>(form: &'a [(String, String)], key: &str) -> &'a str {
    form.iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
        .unwrap_or("")
}

/// Admin-Auth (Localhost/Admin) + CSRF aus dem Form-Body. Gibt `Some(redirect)`
/// zurück, wenn der Request abzulehnen ist, sonst `None`.
///
/// Parität zu Pythons `_require_token` (Admin-Prefixes) + `_read_post_with_csrf`:
/// Localhost ist der interne Loopback und braucht kein CSRF; eine Admin-Cookie-
/// Session muss das sessiongebundene CSRF-Token im Body mitliefern.
pub(crate) async fn gate(
    auth: &DashboardAuthLevel,
    config: Option<&Extension<DashboardAuthState>>,
    headers: &axum::http::HeaderMap,
    form: &[(String, String)],
    redirect_err: impl Fn(&str) -> Response,
) -> Option<Response> {
    if !auth.is_privileged() {
        return Some(redirect_err("Nicht autorisiert."));
    }
    let presented = form_get(form, "csrf_token").trim().to_string();
    let Some(Extension(state)) = config else {
        return Some(redirect_err("CSRF-Prüfung nicht verfügbar."));
    };
    match validate_form_csrf(state, headers, &presented).await {
        Some(true) => None,
        Some(false) => Some(redirect_err("Ungültiges CSRF-Token.")),
        None => Some(redirect_err("Sitzung fehlt.")),
    }
}

fn csrf_cookie_candidates(
    headers: &axum::http::HeaderMap,
) -> Vec<(String, &'static str)> {
    let mut candidates = Vec::new();
    for session_id in cookie_values(headers, ADMIN_COOKIE_NAME) {
        if !session_id.is_empty() {
            candidates.push((session_id.to_string(), "discord_admin"));
        }
    }
    for session_id in cookie_values(headers, PARTNER_COOKIE_NAME) {
        if !session_id.is_empty() {
            candidates.push((session_id.to_string(), "twitch"));
        }
    }
    candidates
}

pub(crate) async fn validate_form_csrf(
    state: &DashboardAuthState,
    headers: &axum::http::HeaderMap,
    presented: &str,
) -> Option<bool> {
    let candidates = csrf_cookie_candidates(headers);
    if candidates.is_empty() {
        return None;
    }
    for (session_id, session_type) in candidates {
        if state
            .validate_csrf(&session_id, session_type, presented)
            .await
            .unwrap_or(false)
        {
            return Some(true);
        }
    }
    Some(false)
}

/// Baut einen 302-Redirect mit URL-kodierter Statusmeldung im Query.
///
/// `target_path` ist der Basis-Pfad (Python `default_path`), `query_key` ist
/// `"ok"` oder `"err"`. Der Legacy-Client folgt dem Redirect und liest den
/// Status aus dem finalen Query.
pub(crate) fn redirect_with(target_path: &str, query_key: &str, message: &str) -> Response {
    let encoded: String = url::form_urlencoded::byte_serialize(message.as_bytes()).collect();
    Redirect::to(&format!("{target_path}?{query_key}={encoded}")).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csrf_prueft_alle_gleichnamigen_admin_cookies() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            axum::http::header::COOKIE,
            "master_dash_session=veraltet; master_dash_session=zentral-gueltig"
                .parse()
                .unwrap(),
        );

        assert_eq!(
            csrf_cookie_candidates(&headers),
            vec![
                ("veraltet".to_string(), "discord_admin"),
                ("zentral-gueltig".to_string(), "discord_admin"),
            ]
        );
    }
}
