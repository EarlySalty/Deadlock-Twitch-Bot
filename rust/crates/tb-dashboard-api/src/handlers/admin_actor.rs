use axum::extract::Extension;
use axum::http::{header, HeaderMap};

use crate::auth::session::{DashboardAuthState, ADMIN_COOKIE_NAME};

fn read_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    let cookie_header = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())?;
    cookie_header.split(';').find_map(|pair| {
        let pair = pair.trim();
        pair.split_once('=')
            .filter(|(k, _)| k.trim() == name)
            .map(|(_, v)| v.trim().to_string())
    })
}

/// Python `_admin_actor_label`: gültige Discord-Admin-Session → `discord:<id>`,
/// sonst sauberer Fallback am Systemrand.
pub async fn admin_actor_label(
    config: Option<&Extension<DashboardAuthState>>,
    headers: &HeaderMap,
) -> String {
    let Some(Extension(state)) = config else {
        return "admin".to_string();
    };
    let Some(cookie) = read_cookie(headers, ADMIN_COOKIE_NAME) else {
        return "admin".to_string();
    };
    match state.load_admin_session_user_id(&cookie).await {
        Ok(Some(user_id)) if user_id.chars().all(|c| c.is_ascii_digit()) => {
            format!("discord:{user_id}")
        }
        Ok(_) => "admin".to_string(),
        Err(error) => {
            tracing::warn!(%error, "admin actor konnte nicht aus Discord-Session gelesen werden");
            "admin".to_string()
        }
    }
}
