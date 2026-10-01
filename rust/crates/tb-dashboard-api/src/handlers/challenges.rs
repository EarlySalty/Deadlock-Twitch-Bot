use crate::auth::level::{AuthenticatedAdminSessionId, DashboardAuthLevel};
use axum::{
    extract::{Extension, Query},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::Utc;
use serde::Deserialize;
use serde_json::json;
use tb_effort::{Engine, Error};

#[derive(Clone)]
pub struct ChallengeEngine(pub Option<Engine>);

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Params {
    pub streamer: Option<String>,
}

fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        [(header::CACHE_CONTROL, "private, no-store")],
        Json(json!({"error":code,"message":message})),
    )
        .into_response()
}

#[allow(clippy::result_large_err)]
fn scope(
    auth: &DashboardAuthLevel,
    admin_session: bool,
    requested: Option<&str>,
) -> Result<Option<String>, Response> {
    match auth {
        DashboardAuthLevel::None => Err(error(
            StatusCode::UNAUTHORIZED,
            "login_required",
            "Bitte anmelden.",
        )),
        DashboardAuthLevel::Partner {
            twitch_user_id,
            twitch_login,
            ..
        } => {
            if requested.is_some_and(|s| !s.eq_ignore_ascii_case(twitch_login)) {
                return Err(error(
                    StatusCode::FORBIDDEN,
                    "self_only",
                    "Du kannst nur deine eigenen Challenges sehen.",
                ));
            }
            Ok(Some(twitch_user_id.clone()))
        }
        DashboardAuthLevel::Admin { actor } => {
            if actor.is_none() && !admin_session {
                return Err(error(
                    StatusCode::UNAUTHORIZED,
                    "session_required",
                    "Bitte mit einer persönlichen Sitzung anmelden.",
                ));
            }
            if requested.is_some() {
                Ok(None)
            } else {
                actor
                    .as_ref()
                    .map(|a| Some(a.twitch_user_id.clone()))
                    .ok_or_else(|| {
                        error(
                            StatusCode::BAD_REQUEST,
                            "streamer_required",
                            "Bitte einen Partner auswählen.",
                        )
                    })
            }
        }
    }
}

pub async fn me_handler(
    auth: DashboardAuthLevel,
    admin: Option<Extension<AuthenticatedAdminSessionId>>,
    Extension(ChallengeEngine(engine)): Extension<ChallengeEngine>,
    Query(params): Query<Params>,
) -> Response {
    handle(auth, admin.is_some(), engine, params, false).await
}

pub async fn viewers_handler(
    auth: DashboardAuthLevel,
    admin: Option<Extension<AuthenticatedAdminSessionId>>,
    Extension(ChallengeEngine(engine)): Extension<ChallengeEngine>,
    Query(params): Query<Params>,
) -> Response {
    handle(auth, admin.is_some(), engine, params, true).await
}

async fn handle(
    auth: DashboardAuthLevel,
    admin_session: bool,
    engine: Option<Engine>,
    params: Params,
    viewers: bool,
) -> Response {
    let own = match scope(&auth, admin_session, params.streamer.as_deref()) {
        Ok(id) => id,
        Err(response) => return response,
    };
    let Some(engine) = engine else {
        return error(
            StatusCode::SERVICE_UNAVAILABLE,
            "unavailable",
            "Challenges sind gerade nicht verfügbar.",
        );
    };
    let result = tokio::time::timeout(std::time::Duration::from_secs(8), async {
        let id = match own {
            Some(id) => id,
            None => {
                engine
                    .partner_by_login(
                        params
                            .streamer
                            .as_deref()
                            .ok_or(Error::Invalid("streamer"))?,
                    )
                    .await?
                    .twitch_user_id
            }
        };
        let now = Utc::now();
        engine.ensure_display_ready(now).await?;
        if viewers {
            serde_json::to_value(engine.viewers(&id, now).await?)
                .map_err(|_| Error::Invalid("response"))
        } else {
            serde_json::to_value(engine.me(&id, now).await?).map_err(|_| Error::Invalid("response"))
        }
    })
    .await;
    match result {
        Ok(Ok(value)) => {
            ([(header::CACHE_CONTROL, "private, no-store")], Json(value)).into_response()
        }
        Ok(Err(Error::NotFound)) => error(
            StatusCode::NOT_FOUND,
            "partner_not_found",
            "Dieser Kanal ist derzeit kein aktiver Partner.",
        ),
        Ok(Err(err)) => {
            tracing::warn!(error=%err,"Challenge-Abfrage fehlgeschlagen");
            error(
                StatusCode::SERVICE_UNAVAILABLE,
                "unavailable",
                "Challenge-Daten sind gerade nicht vollständig verfügbar.",
            )
        }
        Err(_) => error(
            StatusCode::SERVICE_UNAVAILABLE,
            "timeout",
            "Challenge-Daten sind gerade nicht verfügbar.",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn session_only_scope_cannot_be_selected_by_partner_or_internal_token() {
        let partner = DashboardAuthLevel::Partner {
            twitch_user_id: "11".into(),
            twitch_login: "alice".into(),
            display_name: "Alice".into(),
        };
        assert_eq!(scope(&partner, false, None).unwrap(), Some("11".into()));
        assert_eq!(
            scope(&partner, false, Some("bob")).unwrap_err().status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            scope(&DashboardAuthLevel::None, false, None)
                .unwrap_err()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            scope(&DashboardAuthLevel::admin(), false, Some("alice"))
                .unwrap_err()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            scope(&DashboardAuthLevel::admin(), true, Some("alice")).unwrap(),
            None
        );
    }
    #[tokio::test]
    async fn anonymous_and_internal_only_fail_before_data_access() {
        for auth in [DashboardAuthLevel::None, DashboardAuthLevel::admin()] {
            let response = handle(auth, false, None, Params::default(), false).await;
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
            assert_eq!(
                response.headers()[header::CACHE_CONTROL],
                "private, no-store"
            );
        }
    }
}
