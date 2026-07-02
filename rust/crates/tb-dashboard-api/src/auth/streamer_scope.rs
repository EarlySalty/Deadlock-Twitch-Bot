//! Zentraler Streamer-Scope-Resolver (IDOR-Guard).
//!
//! Eingeloggte Partner dürfen NUR auf ihren eigenen Twitch-Login zugreifen;
//! ein `?streamer=<fremd>` führt zu 403. Admin/Localhost dürfen `requested`
//! frei wählen (oder `None` für „alle"). `None`-Auth → 401.
//!
//! Dieser Helfer war ursprünglich lokal in `handlers/social_media.rs` definiert
//! (`_resolve_streamer_scope`, Python-Port) und wird hier zentralisiert, damit
//! alle Daten-Endpoints des Dashboards dieselbe Ownership-Prüfung teilen.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

use crate::auth::level::DashboardAuthLevel;

fn forbidden(message: &str) -> Response {
    (StatusCode::FORBIDDEN, message.to_string()).into_response()
}

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({ "error": "Authentication required." })),
    )
        .into_response()
}

/// Effektiver Streamer-Scope mit Session-Ownership (Python
/// `_resolve_streamer_scope`). Partner sind auf den eigenen Login beschränkt
/// (Cross-Account-Zugriff → 403); Admin/Localhost dürfen `requested` frei
/// wählen (oder `None` für „alle"). `None`-Auth → 401.
///
/// Wird von allen Daten-Endpoints des Dashboards wiederverwendet.
#[allow(clippy::result_large_err)]
pub(crate) fn resolve_streamer_scope(
    auth: &DashboardAuthLevel,
    requested: Option<&str>,
    required: bool,
) -> Result<Option<String>, Response> {
    let requested = requested
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty());
    match auth {
        DashboardAuthLevel::Partner { twitch_login, .. } => {
            let session = twitch_login.to_lowercase();
            if let Some(req) = &requested {
                if *req != session {
                    return Err(forbidden(
                        "Du kannst nur auf deinen eigenen Twitch-Account zugreifen.",
                    ));
                }
            }
            Ok(Some(session))
        }
        DashboardAuthLevel::Admin { .. } => {
            if required && requested.is_none() {
                return Err(
                    (StatusCode::BAD_REQUEST, "streamer parameter required").into_response()
                );
            }
            Ok(requested)
        }
        DashboardAuthLevel::None => Err(unauthorized()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::level::AdminActor;

    fn partner(login: &str) -> DashboardAuthLevel {
        DashboardAuthLevel::Partner {
            twitch_login: login.to_string(),
            twitch_user_id: "42".to_string(),
            display_name: login.to_string(),
        }
    }

    #[test]
    fn partner_eigener_login_gibt_eigenen_scope() {
        let auth = partner("EarlySalty");
        let scope = resolve_streamer_scope(&auth, Some("earlysalty"), false).unwrap();
        assert_eq!(scope, Some("earlysalty".to_string()));
    }

    #[test]
    fn partner_ohne_requested_gibt_eigenen_login() {
        let auth = partner("EarlySalty");
        let scope = resolve_streamer_scope(&auth, None, false).unwrap();
        assert_eq!(scope, Some("earlysalty".to_string()));
    }

    #[test]
    fn partner_fremder_login_ist_forbidden() {
        let auth = partner("earlysalty");
        let err = resolve_streamer_scope(&auth, Some("ismile_e"), false).unwrap_err();
        assert_eq!(err.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn admin_beliebiger_login_durchgelassen() {
        let auth = DashboardAuthLevel::Admin { actor: None };
        let scope = resolve_streamer_scope(&auth, Some("ismile_e"), true).unwrap();
        assert_eq!(scope, Some("ismile_e".to_string()));
    }

    #[test]
    fn admin_mit_actor_beliebiger_login_durchgelassen() {
        let auth = DashboardAuthLevel::Admin {
            actor: Some(AdminActor {
                twitch_user_id: "1".to_string(),
                twitch_login: "earlysalty".to_string(),
            }),
        };
        let scope = resolve_streamer_scope(&auth, Some("ismile_e"), false).unwrap();
        assert_eq!(scope, Some("ismile_e".to_string()));
    }

    #[test]
    fn admin_ohne_requested_und_required_ist_bad_request() {
        let auth = DashboardAuthLevel::Admin { actor: None };
        let err = resolve_streamer_scope(&auth, None, true).unwrap_err();
        assert_eq!(err.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn admin_ohne_requested_und_nicht_required_gibt_none() {
        let auth = DashboardAuthLevel::Admin { actor: None };
        let scope = resolve_streamer_scope(&auth, None, false).unwrap();
        assert_eq!(scope, None);
    }

    #[test]
    fn none_auth_ist_unauthorized() {
        let auth = DashboardAuthLevel::None;
        let err = resolve_streamer_scope(&auth, Some("earlysalty"), false).unwrap_err();
        assert_eq!(err.status(), StatusCode::UNAUTHORIZED);
    }
}
