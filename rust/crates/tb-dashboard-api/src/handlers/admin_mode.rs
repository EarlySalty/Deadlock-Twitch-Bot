//! Sessiongebundener Toggle für die Admin-Präsentation im Dashboard.
//!
//! Der Auth-Level bleibt unverändert `Admin`; dieser Endpoint steuert nur, ob
//! `auth-status` den Twitch-OAuth-Admin als normalen Partner oder mit
//! Admin-Vollzugriff präsentiert. Die Route läuft bewusst NICHT durch
//! `csrf_protect`: Das Streamer-Dashboard (`dashboard_v2`) bekommt von
//! `auth-status` kein CSRF-Token (`csrfToken: null`) und könnte den
//! Header-Schutz nie erfüllen. Geschützt ist der rein präsentationssteuernde
//! Toggle durch die Auth-Prüfung `Admin { actor: Some(_) }` plus das
//! `SameSite=Lax`-Session-Cookie.

use axum::{
    extract::Extension,
    http::{header::SET_COOKIE, HeaderValue},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;
use tb_http_core::ApiError;

use crate::{
    auth::{
        level::DashboardAuthLevel,
        session::{build_transient_session_cookie, clear_session_cookie, SameSite},
    },
    handlers::{auth_login::OAuthLoginConfig, auth_status::ADMIN_MODE_COOKIE},
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdminModeRequest {
    enabled: bool,
}

/// `POST /twitch/api/v2/admin-mode` — Admin-Präsentation für diese Browser-
/// Session aktivieren oder deaktivieren.
pub async fn set_admin_mode_handler(
    auth: DashboardAuthLevel,
    config: Option<Extension<OAuthLoginConfig>>,
    Json(body): Json<AdminModeRequest>,
) -> Response {
    if !matches!(auth, DashboardAuthLevel::Admin { actor: Some(_) }) {
        return ApiError::forbidden_generic().into_response();
    }

    let cookie_secure = config
        .as_ref()
        .map(|extension| extension.0.cookie_secure)
        .unwrap_or(true);
    let cookie = if body.enabled {
        build_transient_session_cookie(ADMIN_MODE_COOKIE, "2", cookie_secure, SameSite::Lax)
    } else {
        clear_session_cookie(ADMIN_MODE_COOKIE, cookie_secure, SameSite::Lax)
    };

    let mut response = Json(json!({ "adminMode": body.enabled })).into_response();
    match HeaderValue::from_str(&cookie) {
        Ok(value) => {
            response.headers_mut().append(SET_COOKIE, value);
            response
        }
        Err(error) => {
            tracing::error!("admin-mode: Set-Cookie-Header konnte nicht gebaut werden: {error}");
            ApiError::internal().into_response()
        }
    }
}

/// Router für den Admin-Modus-Toggle.
///
/// Wird in der App **ohne** `csrf_protect`-Layer gemerged (siehe Modul-Doc).
/// `DashboardAuthState`/`OAuthLoginConfig` kommen als globale Extensions aus der
/// `tb-dashboard`-main, daher ist hier kein eigener State nötig.
pub fn build_admin_mode_router() -> Router {
    Router::new().route("/twitch/api/v2/admin-mode", post(set_admin_mode_handler))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::level::AdminActor;
    use axum::{
        http::{header::SET_COOKIE, StatusCode},
        response::IntoResponse,
        Json,
    };

    fn twitch_admin() -> DashboardAuthLevel {
        DashboardAuthLevel::Admin {
            actor: Some(AdminActor {
                twitch_user_id: "42".to_string(),
                twitch_login: "earlysalty".to_string(),
            }),
        }
    }

    fn partner() -> DashboardAuthLevel {
        DashboardAuthLevel::Partner {
            twitch_login: "partner".to_string(),
            twitch_user_id: "99".to_string(),
            display_name: "Partner".to_string(),
        }
    }

    #[tokio::test]
    async fn enabled_setzt_session_cookie_ohne_ablaufzeit() {
        let response = set_admin_mode_handler(
            twitch_admin(),
            None,
            Json(AdminModeRequest { enabled: true }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let cookie = response
            .headers()
            .get(SET_COOKIE)
            .and_then(|value| value.to_str().ok())
            .unwrap();
        assert!(cookie.starts_with("tb_admin_mode=2;"));
        assert!(cookie.contains("Path=/"));
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("SameSite=Lax"));
        assert!(cookie.contains("Secure"));
        assert!(!cookie.contains("Max-Age"));
        assert!(!cookie.contains("Expires"));

        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["adminMode"], true);
    }

    #[tokio::test]
    async fn disabled_loescht_admin_mode_cookie() {
        let response = set_admin_mode_handler(
            twitch_admin(),
            None,
            Json(AdminModeRequest { enabled: false }),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let cookie = response
            .headers()
            .get(SET_COOKIE)
            .and_then(|value| value.to_str().ok())
            .unwrap();
        assert!(cookie.starts_with("tb_admin_mode=;"));
        assert!(cookie.contains("Max-Age=0"));
    }

    #[tokio::test]
    async fn partner_und_unauth_erhalten_403() {
        for auth in [
            partner(),
            DashboardAuthLevel::None,
            DashboardAuthLevel::admin(),
        ] {
            let response =
                set_admin_mode_handler(auth, None, Json(AdminModeRequest { enabled: true }))
                    .await
                    .into_response();
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
        }
    }
}
