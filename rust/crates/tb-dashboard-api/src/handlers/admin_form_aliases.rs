//! Legacy Admin-Form-Aliasse fuer Streamer-Aktionen.
//!
//! Python registriert diese POST-Routen in `bot/dashboard/routes_entry.py`:
//! - `/twitch/verify` (`login`, `mode`)
//! - `/twitch/archive` (`login`, `mode`, Default `toggle`)
//! - `/twitch/discord_flag` (`login`, `mode`)
//!
//! Die alte Admin-Tabelle (`bot/dashboard/live/live.py`) postet
//! `application/x-www-form-urlencoded` inklusive `csrf_token` im Body. Deshalb
//! laufen diese Aliasse wie die anderen Legacy-Form-Routen ohne Header-CSRF-Layer
//! und validieren den Body-CSRF selbst ueber [`legacy_form::gate`].

use axum::{
    extract::{Extension, RawForm, State},
    response::Response,
};
use sqlx::PgPool;
use tb_analytics::streamers_crud::{
    archive_streamer, departner_streamer, set_discord_flag, verify_streamer, ArchiveMode,
    VerifyStreamerResult,
};
use tb_domain::login::normalize_twitch_login;

use super::legacy_form::{form_get, gate, parse_form, redirect_with};
use crate::auth::level::DashboardAuthLevel;
use crate::auth::session::DashboardAuthState;

const ADMIN_PATH: &str = "/twitch/admin";

fn redirect_ok(message: &str) -> Response {
    redirect_with(ADMIN_PATH, "ok", message)
}

fn redirect_err(message: &str) -> Response {
    redirect_with(ADMIN_PATH, "err", message)
}

/// `POST /twitch/verify` -- Form-Alias auf die native Verify-/Departner-Logik.
pub async fn verify_handler(
    auth: DashboardAuthLevel,
    config: Option<Extension<DashboardAuthState>>,
    State(pool): State<PgPool>,
    headers: axum::http::HeaderMap,
    RawForm(body): RawForm,
) -> Response {
    let form = parse_form(&body);
    if let Some(resp) = gate(&auth, config.as_ref(), &headers, &form, redirect_err).await {
        return resp;
    }

    let raw_login = form_get(&form, "login").trim();
    let Some(login) = normalize_twitch_login(raw_login) else {
        return redirect_ok("Ungültiger Login");
    };
    let mode = form_get(&form, "mode").trim().to_lowercase();

    let message = if matches!(mode.as_str(), "clear" | "failed") {
        match departner_streamer(&pool, &login).await {
            Ok(Some(_)) if mode == "clear" => {
                format!("Verifizierung für {login} zurückgesetzt (keine DM versendet)")
            }
            Ok(Some(_)) => format!("{login}: Verifizierung fehlgeschlagen"),
            Ok(None) => format!("{login} ist nicht gespeichert"),
            Err(e) => {
                tracing::error!("legacy verify/departner Fehler fuer {login}: {e}");
                return redirect_err("Verifizierung fehlgeschlagen");
            }
        }
    } else {
        match verify_streamer(&pool, &login, &mode).await {
            Ok(VerifyStreamerResult::Verified) if mode == "temp" => {
                format!("{login} für 30 Tage verifiziert")
            }
            Ok(VerifyStreamerResult::Verified) => format!("{login} dauerhaft verifiziert"),
            Ok(VerifyStreamerResult::NotAPartner) => format!("{login} ist nicht gespeichert"),
            Ok(VerifyStreamerResult::UnknownMode) => "Unbekannter Modus".to_string(),
            Ok(VerifyStreamerResult::RequiresPartnerLifecycle) => {
                format!("{login} ist nicht gespeichert")
            }
            Err(e) => {
                tracing::error!("legacy verify Fehler fuer {login}: {e}");
                return redirect_err("Verifizierung fehlgeschlagen");
            }
        }
    };

    redirect_ok(&message)
}

/// `POST /twitch/archive` -- Form-Alias auf die native Archive-/Block-Logik.
pub async fn archive_handler(
    auth: DashboardAuthLevel,
    config: Option<Extension<DashboardAuthState>>,
    State(pool): State<PgPool>,
    headers: axum::http::HeaderMap,
    RawForm(body): RawForm,
) -> Response {
    let form = parse_form(&body);
    if let Some(resp) = gate(&auth, config.as_ref(), &headers, &form, redirect_err).await {
        return resp;
    }

    let raw_login = form_get(&form, "login").trim();
    let Some(login) = normalize_twitch_login(raw_login) else {
        return redirect_err("Ungültiger Login");
    };
    let raw_mode = form_get(&form, "mode").trim();
    let mode = if raw_mode.is_empty() {
        ArchiveMode::Toggle
    } else {
        ArchiveMode::parse(raw_mode)
    };

    let before = match archive_state(&pool, &login).await {
        Ok(state) => state,
        Err(e) => {
            tracing::error!("legacy archive Status-Lookup fuer {login}: {e}");
            return redirect_err("Archivierung fehlgeschlagen");
        }
    };
    if matches!(mode, ArchiveMode::Archive | ArchiveMode::Unarchive) && before.is_none() {
        return redirect_err(&format!("{login} ist nicht gespeichert"));
    }
    if mode == ArchiveMode::Archive {
        if let Some(Some(archived_at)) = before.as_ref() {
            if archived_at.trim().is_empty() {
                return redirect_ok(&format!("{login} ist bereits archiviert"));
            }
            return redirect_ok(&format!("{login} ist bereits archiviert (seit {archived_at})"));
        }
    }
    if mode == ArchiveMode::Unarchive && before.as_ref().is_some_and(|v| v.is_none()) {
        return redirect_ok(&format!("{login} ist nicht archiviert"));
    }

    let changed = archive_streamer(&pool, &login, mode).await;
    match changed {
        Ok(true) => redirect_ok(&archive_success_message(&login, mode, before.as_ref())),
        Ok(false) => redirect_err(&format!("{login} ist nicht gespeichert")),
        Err(e) => {
            tracing::error!("legacy archive Fehler fuer {login}: {e}");
            redirect_err("Archivierung fehlgeschlagen")
        }
    }
}

/// `POST /twitch/discord_flag` -- Form-Alias auf die native Discord-Flag-Logik.
pub async fn discord_flag_handler(
    auth: DashboardAuthLevel,
    config: Option<Extension<DashboardAuthState>>,
    State(pool): State<PgPool>,
    headers: axum::http::HeaderMap,
    RawForm(body): RawForm,
) -> Response {
    let form = parse_form(&body);
    if let Some(resp) = gate(&auth, config.as_ref(), &headers, &form, redirect_err).await {
        return resp;
    }

    let raw_login = form_get(&form, "login").trim();
    let Some(login) = normalize_twitch_login(raw_login) else {
        return redirect_err("Ungültiger Login");
    };

    let desired = match form_get(&form, "mode").trim().to_lowercase().as_str() {
        "mark" | "on" | "enable" | "1" => true,
        "unmark" | "off" | "disable" | "0" => false,
        _ => return redirect_err("Ungültiger Modus für Discord-Markierung"),
    };

    match set_discord_flag(&pool, &login, desired).await {
        Ok(true) if desired => redirect_ok(&format!("{login} als Discord-Mitglied markiert")),
        Ok(true) => redirect_ok(&format!("Discord-Markierung für {login} entfernt")),
        Ok(false) => redirect_err(&format!("{login} ist nicht gespeichert")),
        Err(e) => {
            tracing::error!("legacy discord_flag Fehler fuer {login}: {e}");
            redirect_err("Discord-Markierung konnte nicht aktualisiert werden")
        }
    }
}

async fn archive_state(pool: &PgPool, login: &str) -> Result<Option<Option<String>>, sqlx::Error> {
    sqlx::query_scalar::<_, Option<String>>(
        "SELECT admin_archived_at FROM twitch_partners \
         WHERE LOWER(twitch_login) = LOWER($1) LIMIT 1",
    )
    .bind(login)
    .fetch_optional(pool)
    .await
}

fn archive_success_message(
    login: &str,
    mode: ArchiveMode,
    before: Option<&Option<String>>,
) -> String {
    match mode {
        ArchiveMode::Archive => format!("{login} archiviert"),
        ArchiveMode::Unarchive => format!("{login} ent-archiviert"),
        ArchiveMode::Block => format!("{login} dauerhaft blockiert"),
        ArchiveMode::Unblock => format!("{login} entsperrt"),
        ArchiveMode::ToggleBlock => format!("Block-Status für {login} aktualisiert"),
        ArchiveMode::Toggle if before.is_some_and(|v| v.is_some()) => {
            format!("{login} reaktiviert")
        }
        ArchiveMode::Toggle => format!("{login} archiviert"),
    }
}
