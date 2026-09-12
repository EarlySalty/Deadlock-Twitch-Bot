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

/// Ziel für kanaleigene Einstellungen. Der Admin-Modus ändert die Rechte,
/// aber nicht die Twitch-Identität der eigenen Verwaltung. Nur Admins dürfen
/// ein anderes Ziel wählen; ohne Ziel gilt die ID aus der Twitch-Session.
/// Ein Admin ohne Twitch-Identität muss weiterhin ein Ziel angeben.
#[allow(clippy::result_large_err)]
pub(crate) fn resolve_settings_target(
    auth: &DashboardAuthLevel,
    requested: &Option<String>,
) -> Result<(String, String), Response> {
    let requested = requested
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let identity = match auth {
        DashboardAuthLevel::Partner {
            twitch_login,
            twitch_user_id,
            ..
        } => (twitch_login, twitch_user_id),
        DashboardAuthLevel::Admin { actor } => {
            if let Some(login) = requested {
                // Bei expliziter Wahl des eigenen Kanals bleibt die Session-ID erhalten.
                if !actor
                    .as_ref()
                    .is_some_and(|actor| actor.twitch_login.eq_ignore_ascii_case(login))
                {
                    return Ok((login.to_lowercase(), String::new()));
                }
            }
            let Some(actor) = actor else {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": "streamer required"})),
                )
                    .into_response());
            };
            (&actor.twitch_login, &actor.twitch_user_id)
        }
        DashboardAuthLevel::None => return Err(unauthorized()),
    };
    if identity.1.trim().is_empty() {
        return Err(unauthorized());
    }
    Ok((
        identity.0.trim().to_lowercase(),
        identity.1.trim().to_string(),
    ))
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
    fn settings_admin_modus_behaelt_eigene_twitch_identitaet() {
        let partner = partner("EarlySalty");
        let admin = DashboardAuthLevel::Admin {
            actor: Some(AdminActor {
                twitch_user_id: "42".into(),
                twitch_login: "earlysalty".into(),
            }),
        };
        for requested in [None, Some("  ".into()), Some("EarlySalty".into())] {
            assert_eq!(
                resolve_settings_target(&admin, &requested).unwrap(),
                resolve_settings_target(&partner, &requested).unwrap(),
            );
        }
        assert_eq!(
            resolve_settings_target(&admin, &None).unwrap(),
            ("earlysalty".into(), "42".into())
        );
    }

    #[test]
    fn settings_nur_admin_darf_fremdes_ziel_waehlen() {
        let requested = Some(" AndererKanal ".into());
        assert_eq!(
            resolve_settings_target(&partner("earlysalty"), &requested).unwrap(),
            ("earlysalty".into(), "42".into())
        );
        for actor in [
            None,
            Some(AdminActor {
                twitch_user_id: "42".into(),
                twitch_login: "earlysalty".into(),
            }),
        ] {
            assert_eq!(
                resolve_settings_target(&DashboardAuthLevel::Admin { actor }, &requested).unwrap(),
                ("andererkanal".into(), String::new())
            );
        }
        assert_eq!(
            resolve_settings_target(&DashboardAuthLevel::None, &requested)
                .unwrap_err()
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }

    #[test]
    fn settings_ohne_twitch_identitaet_kein_eigener_kanal() {
        assert_eq!(
            resolve_settings_target(&DashboardAuthLevel::admin(), &None)
                .unwrap_err()
                .status(),
            StatusCode::BAD_REQUEST
        );
        let broken_admin = DashboardAuthLevel::Admin {
            actor: Some(AdminActor {
                twitch_user_id: " ".into(),
                twitch_login: "earlysalty".into(),
            }),
        };
        for requested in [None, Some("earlysalty".into())] {
            assert_eq!(
                resolve_settings_target(&broken_admin, &requested)
                    .unwrap_err()
                    .status(),
                StatusCode::UNAUTHORIZED
            );
        }
    }

    #[test]
    fn partner_eigener_login_gibt_eigenen_scope() {
        let auth = partner("EarlySalty");
        let scope = resolve_streamer_scope(&auth, Some("earlysalty"), false).unwrap();
        assert_eq!(scope, Some("earlysalty".to_string()));
    }

    #[tokio::test]
    async fn settings_admin_lesen_schreiben_bleibt_auf_session_id() {
        use crate::handlers::{
            clip_command_settings, greeting_settings, lurk_command_settings, lurker_tax_settings,
            silent_settings, stat_command_settings, title_command_settings,
        };
        use axum::extract::{Query, State};
        let database = crate::test_postgres::TestPostgres::start().await;
        let pool = &database.pool;
        sqlx::raw_sql("CREATE TABLE streamer_plans (
            twitch_user_id TEXT PRIMARY KEY, twitch_login TEXT,
            greeting_reply_enabled INTEGER DEFAULT 1,
            lurk_command_enabled INTEGER DEFAULT 1,
            clip_command_enabled INTEGER DEFAULT 1,
            title_command_enabled INTEGER DEFAULT 1,
            lurker_tax_enabled INTEGER DEFAULT 1,
            stat_command_settings JSONB DEFAULT '{}');
            INSERT INTO streamer_plans (twitch_user_id, twitch_login) VALUES ('99','earlysalty'), ('42','alter_name');
            CREATE TABLE twitch_partners (id SERIAL PRIMARY KEY, twitch_user_id TEXT, twitch_login TEXT, status TEXT, silent_ban INTEGER DEFAULT 0, silent_raid INTEGER DEFAULT 0);
            INSERT INTO twitch_partners (twitch_user_id,twitch_login,status) VALUES ('42','alter_name','active'), ('99','earlysalty','active');
            CREATE TABLE twitch_raid_auth (twitch_login TEXT, scopes TEXT);
            CREATE TABLE twitch_bot_capabilities (id INTEGER PRIMARY KEY, has_chatters_scope BOOLEAN);")
            .execute(pool).await.unwrap();
        let auth = DashboardAuthLevel::Admin {
            actor: Some(AdminActor {
                twitch_login: "earlysalty".into(),
                twitch_user_id: "42".into(),
            }),
        };
        macro_rules! toggle {
            ($module:ident, $query:ident, $update:ident, $field:ident) => {{
                let response = $module::post_handler(
                    auth.clone(),
                    State(pool.clone()),
                    Query($module::$query::default()),
                    Json($module::$update { $field: false }),
                )
                .await;
                assert_eq!(response.status(), StatusCode::OK, stringify!($module));
                let response = $module::get_handler(
                    auth.clone(),
                    State(pool.clone()),
                    Query($module::$query::default()),
                )
                .await;
                assert_eq!(response.status(), StatusCode::OK, stringify!($module));
                let bytes = axum::body::to_bytes(response.into_body(), 65536)
                    .await
                    .unwrap();
                let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(body[stringify!($field)], false, stringify!($module));
                let foreign: i32 = sqlx::query_scalar(concat!(
                    "SELECT ",
                    stringify!($field),
                    " FROM streamer_plans WHERE twitch_user_id='99'"
                ))
                .fetch_one(pool)
                .await
                .unwrap();
                assert_eq!(foreign, 1, stringify!($module));
            }};
        }
        toggle!(
            greeting_settings,
            GreetingQuery,
            GreetingUpdate,
            greeting_reply_enabled
        );
        toggle!(
            lurk_command_settings,
            LurkCommandQuery,
            LurkCommandUpdate,
            lurk_command_enabled
        );
        toggle!(
            clip_command_settings,
            ClipCommandQuery,
            ClipCommandUpdate,
            clip_command_enabled
        );
        toggle!(
            title_command_settings,
            TitleCommandQuery,
            TitleCommandUpdate,
            title_command_enabled
        );
        toggle!(
            lurker_tax_settings,
            LurkerTaxQuery,
            LurkerTaxUpdate,
            lurker_tax_enabled
        );
        let response = silent_settings::post_handler(
            auth.clone(),
            State(pool.clone()),
            Query(silent_settings::SilentQuery::default()),
            Json(silent_settings::SilentUpdate {
                silent_ban: true,
                silent_raid: true,
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let response = silent_settings::get_handler(
            auth.clone(),
            State(pool.clone()),
            Query(silent_settings::SilentQuery::default()),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 65536)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["silent_ban"], true);
        assert_eq!(body["silent_raid"], true);
        let foreign: i32 = sqlx::query_scalar(
            "SELECT silent_ban + silent_raid FROM twitch_partners WHERE twitch_user_id='99'",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(foreign, 0);
        let response = stat_command_settings::post_handler(
            auth.clone(),
            State(pool.clone()),
            Query(stat_command_settings::StatCommandQuery::default()),
            Ok(Json(stat_command_settings::StatCommandUpdate {
                command: tb_chat::stat_commands::StatCommand::Rank,
                enabled: false,
            })),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let response = stat_command_settings::get_handler(
            auth,
            State(pool.clone()),
            Query(stat_command_settings::StatCommandQuery::default()),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let settings: serde_json::Value = sqlx::query_scalar(
            "SELECT stat_command_settings FROM streamer_plans WHERE twitch_user_id='42'",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(settings["rank"], false);
        let foreign: serde_json::Value = sqlx::query_scalar(
            "SELECT stat_command_settings FROM streamer_plans WHERE twitch_user_id='99'",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(foreign, json!({}));
        pool.close().await;
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
