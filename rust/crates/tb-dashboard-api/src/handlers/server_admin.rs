//! API des Server-Admin-Dashboards (Multi-Server Paket A, PLAN.md 4.5 / E9 / E11).
//!
//! Jeder Handler holt sich zuerst den [`ServerAdminContext`] (Login-Prüfung) und
//! danach über [`ServerAdminContext::authorize_guild`] eine [`AuthorizedGuild`].
//! Alle Abfragen unten nehmen nur diesen Typ entgegen und filtern damit auf die
//! Guild des Admins. Gesperrte Bereiche laufen über
//! [`ServerAdminContext::require_area`].
//!
//! Wirkung auf Raids, Ankündigungen und Partnerstatus (neuer Partnerbegriff,
//! Sprach-Raids) kommt aus den Paketen G und R; hier werden nur die
//! Einstellungen und Guild-Zuordnungen gepflegt.

use axum::{
    extract::Path,
    response::{IntoResponse, Response},
    routing::{any, get, patch, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sqlx::Row;
use tb_http_core::ApiError;

use crate::auth::server_admin_gate::{
    parse_guild_id, AuthorizedGuild, ServerAdminArea, ServerAdminContext,
};

fn db_error(error: sqlx::Error) -> ApiError {
    tracing::error!(%error, "server_admin DB-Fehler");
    ApiError::internal()
}

fn no_store(mut response: Response) -> Response {
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    response
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuildSettings {
    pub guild_id: String,
    pub name: String,
    pub language: String,
    pub announce_channel_id: Option<String>,
    pub alert_channel_id: Option<String>,
    pub streamer_role_id: Option<String>,
    pub ping_role_id: Option<String>,
    pub promo_invite_url: Option<String>,
    pub auto_partner_default: bool,
    pub cross_server_raids: bool,
    pub active_streamers: i64,
    pub inactive_streamers: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuildStreamer {
    pub twitch_user_id: String,
    pub twitch_login: Option<String>,
    pub active: bool,
    pub source: String,
    pub joined_at: String,
    pub deactivated_at: Option<String>,
}

fn id_string(value: Option<i64>) -> Option<String> {
    value.map(|v| v.to_string())
}

// ---------------------------------------------------------------------------
// Datenzugriff — ausschließlich über `AuthorizedGuild`.
// ---------------------------------------------------------------------------

pub async fn load_settings(
    ctx: &ServerAdminContext,
    guild: &AuthorizedGuild,
) -> Result<GuildSettings, ApiError> {
    let row = sqlx::query(
        "SELECT g.guild_id, g.name, g.language, g.announce_channel_id, g.alert_channel_id,
                g.streamer_role_id, g.ping_role_id, g.promo_invite_url,
                g.auto_partner_default, g.cross_server_raids,
                (SELECT count(*) FROM streamer_guilds s WHERE s.guild_id = g.guild_id AND s.active) AS active_count,
                (SELECT count(*) FROM streamer_guilds s WHERE s.guild_id = g.guild_id AND NOT s.active) AS inactive_count
           FROM discord_guilds g
          WHERE g.guild_id = $1",
    )
    .bind(guild.id())
    .fetch_optional(ctx.pool())
    .await
    .map_err(db_error)?
    .ok_or_else(ApiError::not_found)?;
    Ok(GuildSettings {
        guild_id: row.get::<i64, _>("guild_id").to_string(),
        name: row.get("name"),
        language: row.get("language"),
        announce_channel_id: id_string(row.get("announce_channel_id")),
        alert_channel_id: id_string(row.get("alert_channel_id")),
        streamer_role_id: id_string(row.get("streamer_role_id")),
        ping_role_id: id_string(row.get("ping_role_id")),
        promo_invite_url: row.get("promo_invite_url"),
        auto_partner_default: row.get("auto_partner_default"),
        cross_server_raids: row.get("cross_server_raids"),
        active_streamers: row.get("active_count"),
        inactive_streamers: row.get("inactive_count"),
    })
}

pub async fn load_streamers(
    ctx: &ServerAdminContext,
    guild: &AuthorizedGuild,
) -> Result<Vec<GuildStreamer>, ApiError> {
    let rows = sqlx::query(
        "SELECT sg.twitch_user_id, sg.active, sg.source,
                to_char(sg.joined_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') AS joined_at,
                to_char(sg.deactivated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') AS deactivated_at,
                (SELECT ts.twitch_login FROM twitch_streamers ts
                  WHERE ts.twitch_user_id = sg.twitch_user_id
                  ORDER BY ts.twitch_login LIMIT 1) AS twitch_login
           FROM streamer_guilds sg
          WHERE sg.guild_id = $1
          ORDER BY sg.active DESC, twitch_login NULLS LAST, sg.twitch_user_id",
    )
    .bind(guild.id())
    .fetch_all(ctx.pool())
    .await
    .map_err(db_error)?;
    Ok(rows
        .into_iter()
        .map(|row| GuildStreamer {
            twitch_user_id: row.get("twitch_user_id"),
            twitch_login: row.get("twitch_login"),
            active: row.get("active"),
            source: row.get("source"),
            joined_at: row
                .get::<Option<String>, _>("joined_at")
                .unwrap_or_default(),
            deactivated_at: row.get("deactivated_at"),
        })
        .collect())
}

async fn audit(
    ctx: &ServerAdminContext,
    guild: &AuthorizedGuild,
    action: &str,
    detail: Value,
) -> Result<(), ApiError> {
    sqlx::query(
        "INSERT INTO server_admin_audit_log (guild_id, actor, action, detail) VALUES ($1, $2, $3, $4)",
    )
    .bind(guild.id())
    .bind(ctx.principal.actor())
    .bind(action)
    .bind(detail)
    .execute(ctx.pool())
    .await
    .map_err(db_error)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Einstellungen: Feld → Bereich, Validierung.
// ---------------------------------------------------------------------------

/// Welcher Bereich ein Feld der Einstellungs-API ist. `None` = unbekannt.
pub fn area_for_field(field: &str) -> Option<ServerAdminArea> {
    Some(match field {
        "autoPartnerDefault" => ServerAdminArea::AutoPartnerDefault,
        "announceChannelId" | "alertChannelId" => ServerAdminArea::Channels,
        "streamerRoleId" | "pingRoleId" => ServerAdminArea::Roles,
        "language" => ServerAdminArea::Language,
        "promoInviteUrl" => ServerAdminArea::Invite,
        "crossServerRaids" => ServerAdminArea::CrossServerRaids,
        // Gesperrte Felder ausdrücklich benennen, damit sie 403 statt 400 liefern.
        "aiFeatures" | "ai_features" => ServerAdminArea::AiFeatures,
        "tags" => ServerAdminArea::Tags,
        "globalBans" | "global_bans" => ServerAdminArea::GlobalBans,
        "blacklist" => ServerAdminArea::Blacklist,
        "billing" | "plan" => ServerAdminArea::Billing,
        _ => return None,
    })
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SettingsPatch {
    pub auto_partner_default: Option<bool>,
    pub cross_server_raids: Option<bool>,
    pub language: Option<String>,
    pub announce_channel_id: Option<Option<i64>>,
    pub alert_channel_id: Option<Option<i64>>,
    pub streamer_role_id: Option<Option<i64>>,
    pub ping_role_id: Option<Option<i64>>,
    pub promo_invite_url: Option<Option<String>>,
}

fn invalid_field(field: &str, reason: &str) -> ApiError {
    ApiError::bad_request_with_body(json!({
        "error": "invalid_field",
        "field": field,
        "reason": reason,
    }))
}

fn snowflake_value(field: &str, value: &Value) -> Result<Option<i64>, ApiError> {
    match value {
        Value::Null => Ok(None),
        Value::String(raw) if raw.trim().is_empty() => Ok(None),
        Value::String(raw) => parse_guild_id(raw)
            .map(Some)
            .ok_or_else(|| invalid_field(field, "snowflake")),
        _ => Err(invalid_field(field, "snowflake")),
    }
}

fn bool_value(field: &str, value: &Value) -> Result<bool, ApiError> {
    value
        .as_bool()
        .ok_or_else(|| invalid_field(field, "boolean"))
}

/// Erlaubt nur echte Discord-Einladungen und normalisiert auf `https://discord.gg/<code>`.
pub fn normalize_invite(raw: &str) -> Option<String> {
    let raw = raw.trim();
    let rest = raw
        .strip_prefix("https://")
        .or_else(|| raw.strip_prefix("http://"))
        .unwrap_or(raw);
    let code = rest
        .strip_prefix("discord.gg/")
        .or_else(|| rest.strip_prefix("www.discord.gg/"))
        .or_else(|| rest.strip_prefix("discord.com/invite/"))
        .or_else(|| rest.strip_prefix("www.discord.com/invite/"))
        .or_else(|| rest.strip_prefix("discordapp.com/invite/"))?;
    let code = code.trim_end_matches('/');
    if (2..=32).contains(&code.len())
        && code.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        Some(format!("https://discord.gg/{code}"))
    } else {
        None
    }
}

/// Prüft zuerst ALLE Felder gegen die Rechte-Schranke, dann die Werte. Ein
/// einziges gesperrtes Feld verwirft die ganze Anfrage, nichts wird teilweise
/// geschrieben.
pub fn parse_settings_patch(
    ctx: &ServerAdminContext,
    body: &Map<String, Value>,
) -> Result<SettingsPatch, ApiError> {
    if body.is_empty() {
        return Err(ApiError::bad_request("empty patch"));
    }
    for field in body.keys() {
        let Some(area) = area_for_field(field) else {
            return Err(ApiError::bad_request_with_body(json!({
                "error": "unknown_field",
                "field": field,
            })));
        };
        ctx.require_area(area)?;
    }
    let mut patch = SettingsPatch::default();
    for (field, value) in body {
        match field.as_str() {
            "autoPartnerDefault" => patch.auto_partner_default = Some(bool_value(field, value)?),
            "crossServerRaids" => patch.cross_server_raids = Some(bool_value(field, value)?),
            "language" => {
                let language = value
                    .as_str()
                    .map(|v| v.trim().to_ascii_lowercase())
                    .filter(|v| matches!(v.as_str(), "de" | "en"))
                    .ok_or_else(|| invalid_field(field, "language"))?;
                patch.language = Some(language);
            }
            "announceChannelId" => patch.announce_channel_id = Some(snowflake_value(field, value)?),
            "alertChannelId" => patch.alert_channel_id = Some(snowflake_value(field, value)?),
            "streamerRoleId" => patch.streamer_role_id = Some(snowflake_value(field, value)?),
            "pingRoleId" => patch.ping_role_id = Some(snowflake_value(field, value)?),
            "promoInviteUrl" => {
                patch.promo_invite_url = Some(match value {
                    Value::Null => None,
                    Value::String(raw) if raw.trim().is_empty() => None,
                    Value::String(raw) => Some(
                        normalize_invite(raw)
                            .ok_or_else(|| invalid_field(field, "discord_invite"))?,
                    ),
                    _ => return Err(invalid_field(field, "discord_invite")),
                })
            }
            _ => return Err(ApiError::bad_request("unknown field")),
        }
    }
    Ok(patch)
}

async fn apply_settings_patch(
    ctx: &ServerAdminContext,
    guild: &AuthorizedGuild,
    patch: &SettingsPatch,
) -> Result<(), ApiError> {
    // `$n::bool IS NULL` heißt „nicht ändern“; für nullbare IDs gibt es je ein
    // eigenes Flag, damit „leeren“ von „nicht ändern“ unterscheidbar bleibt.
    sqlx::query(
        "UPDATE discord_guilds SET
            auto_partner_default = COALESCE($2, auto_partner_default),
            cross_server_raids   = COALESCE($3, cross_server_raids),
            language             = COALESCE($4, language),
            announce_channel_id  = CASE WHEN $5 THEN $6 ELSE announce_channel_id END,
            alert_channel_id     = CASE WHEN $7 THEN $8 ELSE alert_channel_id END,
            streamer_role_id     = CASE WHEN $9 THEN $10 ELSE streamer_role_id END,
            ping_role_id         = CASE WHEN $11 THEN $12 ELSE ping_role_id END,
            promo_invite_url     = CASE WHEN $13 THEN $14 ELSE promo_invite_url END,
            updated_at = NOW(),
            updated_by = $15
          WHERE guild_id = $1",
    )
    .bind(guild.id())
    .bind(patch.auto_partner_default)
    .bind(patch.cross_server_raids)
    .bind(patch.language.as_deref())
    .bind(patch.announce_channel_id.is_some())
    .bind(patch.announce_channel_id.flatten())
    .bind(patch.alert_channel_id.is_some())
    .bind(patch.alert_channel_id.flatten())
    .bind(patch.streamer_role_id.is_some())
    .bind(patch.streamer_role_id.flatten())
    .bind(patch.ping_role_id.is_some())
    .bind(patch.ping_role_id.flatten())
    .bind(patch.promo_invite_url.is_some())
    .bind(patch.promo_invite_url.clone().flatten())
    .bind(ctx.principal.actor())
    .execute(ctx.pool())
    .await
    .map_err(db_error)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// `GET /twitch/api/server-admin/me`
pub async fn me_handler(ctx: ServerAdminContext) -> Result<Response, ApiError> {
    let guilds = ctx.accessible_guilds().await?;
    let mut items = Vec::with_capacity(guilds.len());
    for guild in &guilds {
        items.push(load_settings(&ctx, guild).await?);
    }
    let permitted: Vec<&str> = ServerAdminArea::ALL
        .into_iter()
        .filter(|a| a.permitted())
        .map(ServerAdminArea::as_str)
        .collect();
    Ok(no_store(
        Json(json!({
            "principal": ctx.principal.kind(),
            "displayName": ctx.principal.display_name,
            "csrfToken": ctx.principal.csrf_token,
            "guilds": items,
            "permittedAreas": permitted,
        }))
        .into_response(),
    ))
}

/// `GET /twitch/api/server-admin/guilds/{guild_id}`
pub async fn guild_handler(
    ctx: ServerAdminContext,
    Path(guild_id): Path<String>,
) -> Result<Response, ApiError> {
    let guild = ctx.authorize_guild(&guild_id).await?;
    Ok(no_store(
        Json(load_settings(&ctx, &guild).await?).into_response(),
    ))
}

/// `PATCH /twitch/api/server-admin/guilds/{guild_id}/settings`
pub async fn settings_patch_handler(
    ctx: ServerAdminContext,
    Path(guild_id): Path<String>,
    Json(body): Json<Map<String, Value>>,
) -> Result<Response, ApiError> {
    let guild = ctx.authorize_guild(&guild_id).await?;
    let patch = parse_settings_patch(&ctx, &body)?;
    apply_settings_patch(&ctx, &guild, &patch).await?;
    let fields: Vec<&String> = body.keys().collect();
    audit(&ctx, &guild, "settings_update", json!({ "fields": fields })).await?;
    Ok(no_store(
        Json(load_settings(&ctx, &guild).await?).into_response(),
    ))
}

/// `GET /twitch/api/server-admin/guilds/{guild_id}/streamers`
pub async fn streamers_handler(
    ctx: ServerAdminContext,
    Path(guild_id): Path<String>,
) -> Result<Response, ApiError> {
    let guild = ctx.authorize_guild(&guild_id).await?;
    ctx.require_area(ServerAdminArea::StreamerActivation)?;
    Ok(no_store(
        Json(json!({ "streamers": load_streamers(&ctx, &guild).await? })).into_response(),
    ))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StreamerActivationRequest {
    pub active: bool,
}

/// `POST /twitch/api/server-admin/guilds/{guild_id}/streamers/{twitch_user_id}`
///
/// Wirkt nur auf die Zeile dieser Guild. Andere Guilds desselben Streamers und
/// globale Zustände (Bans, Blacklist, Tags, Abo) bleiben unberührt.
pub async fn streamer_activation_handler(
    ctx: ServerAdminContext,
    Path((guild_id, twitch_user_id)): Path<(String, String)>,
    Json(body): Json<StreamerActivationRequest>,
) -> Result<Response, ApiError> {
    let guild = ctx.authorize_guild(&guild_id).await?;
    ctx.require_area(ServerAdminArea::StreamerActivation)?;
    let twitch_user_id = twitch_user_id.trim();
    if twitch_user_id.is_empty()
        || twitch_user_id.len() > 20
        || !twitch_user_id.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(ApiError::bad_request("invalid twitch_user_id"));
    }
    let updated = sqlx::query(
        "UPDATE streamer_guilds SET
            active = $3,
            deactivated_by = CASE WHEN $3 THEN NULL ELSE $4 END,
            deactivated_at = CASE WHEN $3 THEN NULL ELSE NOW() END
          WHERE guild_id = $1 AND twitch_user_id = $2",
    )
    .bind(guild.id())
    .bind(twitch_user_id)
    .bind(body.active)
    .bind(ctx.principal.actor())
    .execute(ctx.pool())
    .await
    .map_err(db_error)?
    .rows_affected();
    if updated == 0 {
        return Err(ApiError::not_found_with("streamer not in this guild"));
    }
    audit(
        &ctx,
        &guild,
        if body.active {
            "streamer_activate"
        } else {
            "streamer_deactivate"
        },
        json!({ "twitch_user_id": twitch_user_id }),
    )
    .await?;
    Ok(no_store(
        Json(json!({ "ok": true, "twitchUserId": twitch_user_id, "active": body.active }))
            .into_response(),
    ))
}

/// Gesperrte Bereiche: ausdrücklich 403 statt stillem 404. Die Anfrage muss
/// trotzdem eingeloggt sein (sonst 401).
async fn forbidden_area(ctx: ServerAdminContext, area: ServerAdminArea) -> Response {
    match ctx.require_area(area) {
        Ok(()) => ApiError::not_found().into_response(),
        Err(error) => error.into_response(),
    }
}

/// Pfadsegmente der gesperrten Bereiche unter `/guilds/{guild_id}/…`.
pub const FORBIDDEN_AREA_ROUTES: [(&str, ServerAdminArea); 5] = [
    ("tags", ServerAdminArea::Tags),
    ("global-bans", ServerAdminArea::GlobalBans),
    ("blacklist", ServerAdminArea::Blacklist),
    ("billing", ServerAdminArea::Billing),
    ("ai-features", ServerAdminArea::AiFeatures),
];

/// Routen der Server-Admin-API (ohne Login-Routen).
pub fn api_routes() -> Router {
    let mut router = Router::new()
        .route("/twitch/api/server-admin/me", get(me_handler))
        .route(
            "/twitch/api/server-admin/guilds/{guild_id}",
            get(guild_handler),
        )
        .route(
            "/twitch/api/server-admin/guilds/{guild_id}/settings",
            patch(settings_patch_handler),
        )
        .route(
            "/twitch/api/server-admin/guilds/{guild_id}/streamers",
            get(streamers_handler),
        )
        .route(
            "/twitch/api/server-admin/guilds/{guild_id}/streamers/{twitch_user_id}",
            post(streamer_activation_handler),
        );
    for (segment, area) in FORBIDDEN_AREA_ROUTES {
        router = router.route(
            &format!("/twitch/api/server-admin/guilds/{{guild_id}}/{segment}"),
            any(move |ctx: ServerAdminContext| forbidden_area(ctx, area)),
        );
    }
    router
}

#[cfg(test)]
#[path = "server_admin_tests.rs"]
mod tests;
