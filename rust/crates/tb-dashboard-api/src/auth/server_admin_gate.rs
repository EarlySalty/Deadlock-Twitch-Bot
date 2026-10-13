//! Zentrale Rechte-Schranke des Server-Admin-Dashboards (Multi-Server Paket A,
//! PLAN.md 4.5 / E9 / E11).
//!
//! Diese Datei ist die **einzige** Stelle, die entscheidet,
//! - wer das Server-Admin-Dashboard überhaupt benutzen darf
//!   ([`ServerAdminPrincipal`]: Guild-Admin per eigener Discord-Session),
//! - auf welche Guild eine Anfrage wirken darf ([`ServerAdminContext::authorize_guild`]),
//! - welche Bereiche im Server-Admin-Dashboard erlaubt sind ([`ServerAdminArea`]).
//!
//! Handler bekommen eine [`AuthorizedGuild`] nur aus dieser Schranke. Der Typ
//! hat keinen öffentlichen Konstruktor; jede Datenbankfunktion des Server-Admin-
//! Dashboards nimmt ihn statt einer rohen Guild-ID entgegen. Eine Guild-ID aus
//! dem Pfad wird dadurch nie ungeprüft verwendet.
//!
//! Strikte Trennung vom globalen Admin-Bereich (Betreiber, „Admin-Modus“ im
//! Partner-Dashboard): Die Server-Admin-Session hat einen eigenen Typ und ein
//! eigenes Cookie, die von `DashboardAuthLevel`, `csrf_protect` und allen
//! Admin-Routen nicht akzeptiert werden. Umgekehrt wertet diese Schranke weder
//! die globale Admin-Session noch den Admin-Modus aus: Zugang gibt es hier nur
//! über den eigenen Discord-Login mit „Server verwalten“.

use std::collections::BTreeSet;

use axum::{
    extract::FromRequestParts,
    http::{request::Parts, Method},
};
use serde_json::json;
use sqlx::PgPool;
use tb_http_core::ApiError;

use super::csrf::{check_same_origin, OriginCheck, CSRF_HEADER};
use super::level::cookie_values;
use super::session::{DashboardAuthState, SERVER_ADMIN_COOKIE_NAME};

/// Bereiche, die im Server-Admin-Dashboard auftauchen können.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ServerAdminArea {
    // Erlaubt (PLAN.md 4.5).
    AutoPartnerDefault,
    StreamerActivation,
    Channels,
    Roles,
    Language,
    Invite,
    CrossServerRaids,
    // Nicht erlaubt (PLAN.md 4.5): bleiben der globalen Admin-Ebene vorbehalten.
    Tags,
    GlobalBans,
    Blacklist,
    Billing,
    AiFeatures,
}

impl ServerAdminArea {
    pub const ALL: [ServerAdminArea; 12] = [
        Self::AutoPartnerDefault,
        Self::StreamerActivation,
        Self::Channels,
        Self::Roles,
        Self::Language,
        Self::Invite,
        Self::CrossServerRaids,
        Self::Tags,
        Self::GlobalBans,
        Self::Blacklist,
        Self::Billing,
        Self::AiFeatures,
    ];

    /// Allowlist: Nur diese Bereiche sind im Server-Admin-Dashboard freigegeben.
    /// Alles andere bleibt der globalen Betreiber-Ebene vorbehalten.
    pub const fn permitted(self) -> bool {
        matches!(
            self,
            Self::AutoPartnerDefault
                | Self::StreamerActivation
                | Self::Channels
                | Self::Roles
                | Self::Language
                | Self::Invite
                | Self::CrossServerRaids
        )
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AutoPartnerDefault => "auto_partner_default",
            Self::StreamerActivation => "streamer_activation",
            Self::Channels => "channels",
            Self::Roles => "roles",
            Self::Language => "language",
            Self::Invite => "invite",
            Self::CrossServerRaids => "cross_server_raids",
            Self::Tags => "tags",
            Self::GlobalBans => "global_bans",
            Self::Blacklist => "blacklist",
            Self::Billing => "billing",
            Self::AiFeatures => "ai_features",
        }
    }

    pub fn from_slug(slug: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|area| area.as_str() == slug)
    }
}

/// Wer gerade das Server-Admin-Dashboard benutzt: ein Discord-Nutzer mit
/// „Server verwalten“ auf mindestens einer Guild. Es gibt bewusst keinen
/// zweiten Principal-Typ (kein globaler Admin, kein Admin-Modus).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerAdminPrincipal {
    pub discord_user_id: String,
    pub display_name: String,
    /// Login-Snapshot der Guilds mit „Server verwalten“ (und installiertem Bot).
    pub managed_guild_ids: BTreeSet<i64>,
    pub csrf_token: String,
}

impl ServerAdminPrincipal {
    pub fn kind(&self) -> &'static str {
        "guild_admin"
    }

    /// Kennung für das Audit-Log.
    pub fn actor(&self) -> String {
        format!("discord:{}", self.discord_user_id)
    }
}

/// Eine Guild, für die die Schranke den Zugriff bestätigt hat. Nur in diesem
/// Modul konstruierbar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthorizedGuild {
    guild_id: i64,
}

impl AuthorizedGuild {
    pub fn id(&self) -> i64 {
        self.guild_id
    }
}

/// Request-Kontext des Server-Admin-Dashboards (axum-Extractor).
#[derive(Clone)]
pub struct ServerAdminContext {
    pub principal: ServerAdminPrincipal,
    pool: PgPool,
}

pub fn login_required_error() -> ApiError {
    ApiError::unauthorized_with_body(json!({
        "error": "server_admin_login_required",
        "loginUrl": "/twitch/server-admin/auth/login",
    }))
}

pub fn guild_forbidden_error() -> ApiError {
    ApiError::forbidden_with_body(json!({ "error": "guild_forbidden" }))
}

pub fn area_forbidden_error(area: ServerAdminArea) -> ApiError {
    ApiError::forbidden_with_body(json!({
        "error": "area_forbidden",
        "area": area.as_str(),
    }))
}

fn invalid_csrf_error() -> ApiError {
    ApiError::forbidden_with_body(json!({ "error": "invalid_csrf" }))
}

fn is_write_method(method: &Method) -> bool {
    matches!(
        *method,
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    )
}

fn db_error(error: sqlx::Error) -> ApiError {
    tracing::error!(%error, "Server-Admin-Schranke: DB-Fehler");
    ApiError::internal()
}

impl<S> FromRequestParts<S> for ServerAdminContext
where
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let Some(auth_state) = parts.extensions.get::<DashboardAuthState>().cloned() else {
            return Err(login_required_error());
        };
        let write = is_write_method(&parts.method);

        // 1. Eigene Server-Admin-Session (Discord, „Server verwalten“).
        let session_ids: Vec<String> = cookie_values(&parts.headers, SERVER_ADMIN_COOKIE_NAME)
            .into_iter()
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect();
        for session_id in session_ids {
            let Some(session) = auth_state
                .load_server_admin_session(&session_id)
                .await
                .map_err(db_error)?
            else {
                continue;
            };
            if write {
                // Schreibzugriffe: gleicher Ursprung UND sessiongebundenes Token.
                let presented = parts
                    .headers
                    .get(CSRF_HEADER)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("")
                    .trim();
                if matches!(check_same_origin(&parts.headers), OriginCheck::CrossOrigin)
                    || presented.is_empty()
                    || !tb_crypto::constant_time_eq(
                        presented.as_bytes(),
                        session.csrf_token.as_bytes(),
                    )
                {
                    return Err(invalid_csrf_error());
                }
            }
            return Ok(Self {
                principal: ServerAdminPrincipal {
                    discord_user_id: session.discord_user_id,
                    display_name: session.display_name,
                    managed_guild_ids: session.managed_guild_ids.into_iter().collect(),
                    csrf_token: session.csrf_token,
                },
                pool: auth_state.pool().clone(),
            });
        }

        // Bewusst KEIN Rückfall auf die globale Admin-Session (Admin-Modus):
        // Das Server-Admin-Dashboard ist ein eigenes Panel nur für Guild-Admins.
        Err(login_required_error())
    }
}

impl ServerAdminContext {
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Lehnt jeden Bereich außerhalb der Allowlist ab.
    pub fn require_area(&self, area: ServerAdminArea) -> Result<(), ApiError> {
        if area.permitted() {
            Ok(())
        } else {
            tracing::warn!(
                principal = self.principal.kind(),
                area = area.as_str(),
                "AUDIT server-admin: gesperrter Bereich abgelehnt"
            );
            Err(area_forbidden_error(area))
        }
    }

    /// Prüft eine Guild-ID aus dem Request gegen die Rechte des Nutzers.
    ///
    /// Nur Guilds aus dem geprüften Login-Snapshot („Server verwalten“), die
    /// aktuell installiert sind. Für fremde und unbekannte Guilds gibt es dieselbe
    /// Antwort, damit sich keine Guild-IDs abtasten lassen.
    pub async fn authorize_guild(&self, raw_guild_id: &str) -> Result<AuthorizedGuild, ApiError> {
        let Some(guild_id) = parse_guild_id(raw_guild_id) else {
            return Err(guild_forbidden_error());
        };
        if !self.principal.managed_guild_ids.contains(&guild_id) {
            tracing::warn!(
                principal = self.principal.kind(),
                actor = %self.principal.actor(),
                guild_id,
                "AUDIT server-admin: Zugriff auf fremde Guild abgelehnt"
            );
            return Err(guild_forbidden_error());
        }
        let installed: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM discord_guilds WHERE guild_id = $1 AND status = 'active')",
        )
        .bind(guild_id)
        .fetch_one(&self.pool)
        .await
        .map_err(db_error)?;
        if !installed {
            return Err(guild_forbidden_error());
        }
        Ok(AuthorizedGuild { guild_id })
    }

    /// Alle Guilds, die dieser Nutzer verwalten darf (nur installierte).
    pub async fn accessible_guilds(&self) -> Result<Vec<AuthorizedGuild>, ApiError> {
        let wanted: Vec<i64> = self.principal.managed_guild_ids.iter().copied().collect();
        let ids: Vec<i64> = sqlx::query_scalar(
            "SELECT guild_id FROM discord_guilds
             WHERE status = 'active' AND guild_id = ANY($1)
             ORDER BY lower(name), guild_id",
        )
        .bind(wanted)
        .fetch_all(&self.pool)
        .await
        .map_err(db_error)?;
        Ok(ids
            .into_iter()
            .map(|guild_id| AuthorizedGuild { guild_id })
            .collect())
    }
}

/// Discord-Snowflake als positive `i64`.
pub fn parse_guild_id(raw: &str) -> Option<i64> {
    let raw = raw.trim();
    if raw.is_empty() || raw.len() > 20 || !raw.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    raw.parse::<i64>().ok().filter(|id| *id > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowlist_deckt_plan_ab() {
        let permitted: Vec<_> = ServerAdminArea::ALL
            .into_iter()
            .filter(|a| a.permitted())
            .collect();
        assert_eq!(
            permitted,
            vec![
                ServerAdminArea::AutoPartnerDefault,
                ServerAdminArea::StreamerActivation,
                ServerAdminArea::Channels,
                ServerAdminArea::Roles,
                ServerAdminArea::Language,
                ServerAdminArea::Invite,
                ServerAdminArea::CrossServerRaids,
            ]
        );
        for forbidden in [
            ServerAdminArea::Tags,
            ServerAdminArea::GlobalBans,
            ServerAdminArea::Blacklist,
            ServerAdminArea::Billing,
            ServerAdminArea::AiFeatures,
        ] {
            assert!(!forbidden.permitted(), "{forbidden:?} muss gesperrt sein");
        }
    }

    #[test]
    fn slugs_sind_eindeutig_und_umkehrbar() {
        for area in ServerAdminArea::ALL {
            assert_eq!(ServerAdminArea::from_slug(area.as_str()), Some(area));
        }
        assert_eq!(ServerAdminArea::from_slug("admin_mode"), None);
    }

    #[test]
    fn guild_id_parsing_ist_streng() {
        assert_eq!(
            parse_guild_id("1289721245281292288"),
            Some(1_289_721_245_281_292_288)
        );
        assert_eq!(parse_guild_id("0"), None);
        assert_eq!(parse_guild_id("-5"), None);
        assert_eq!(parse_guild_id("12a"), None);
        assert_eq!(parse_guild_id(""), None);
        assert_eq!(parse_guild_id("99999999999999999999"), None);
    }
}
