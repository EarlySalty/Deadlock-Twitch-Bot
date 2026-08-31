//! Admin-Bulk-Config der Partner-Flags (Raid- + Chat-Toggles).
//!
//! Port von `bot/analytics/api_admin.py:_api_admin_config_raids` +
//! `_api_admin_config_chat`. Zwei POST-Endpoints setzen netzweit die Flags
//! `raid_bot_enabled`/`live_ping_enabled` (raids) bzw. `silent_ban`/`silent_raid`
//! (chat) auf allen aktiven Partnern und geben Aggregat-Snapshots zurück.
//!
//! CSRF wird — wie im übrigen Rust-Dashboard etabliert — nicht geprüft; Admin
//! über `DashboardAuthLevel`. updated_by = "admin" (Rust-Auth ohne
//! Discord-User-ID, = Pythons Fallback).

use axum::{
    extract::{Query, State},
    response::IntoResponse,
    Json,
};
use chrono::SecondsFormat;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgPool;
use crate::auth::level::DashboardAuthLevel;
use tb_http_core::ApiError;

use tb_analytics::admin_config::{
    bulk_update_partner_flags, load_streamer_config_snapshots, parse_admin_scope, PartnerFlagUpdate,
};
use tb_analytics::promo_mode::{evaluate_global_promo_mode, load_global_promo_mode};

/// Boolean-Coercion für Admin-Payloads (Python `_admin_normalize_bool`):
/// echte bools, `0`/`1` (int/float), Strings `1/true/yes/on` bzw. `0/false/no/off`.
/// Alles andere → `None` (= validation_failed).
fn normalize_admin_bool(value: Option<&Value>) -> Option<bool> {
    match value {
        Some(Value::Bool(b)) => Some(*b),
        Some(Value::Number(n)) => match n.as_i64() {
            Some(0) => Some(false),
            Some(1) => Some(true),
            _ => match n.as_f64() {
                Some(0.0) => Some(false),
                Some(1.0) => Some(true),
                _ => None,
            },
        },
        Some(Value::String(s)) => match s.trim().to_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Some(true),
            "0" | "false" | "no" | "off" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

fn parse_object_body(body: &[u8]) -> Result<Value, ApiError> {
    match serde_json::from_slice::<Value>(body) {
        Ok(v @ Value::Object(_)) => Ok(v),
        Ok(_) => Err(ApiError::bad_request_with_body(json!({ "error": "invalid_payload" }))),
        Err(_) => Err(ApiError::bad_request_with_body(json!({ "error": "invalid_json" }))),
    }
}

/// Liest + validiert den Scope aus dem Payload (Python `_admin_parse_scope`).
fn scope_or_error(payload: &Value) -> Result<String, ApiError> {
    let raw = payload.get("scope").and_then(Value::as_str);
    parse_admin_scope(raw).ok_or_else(|| {
        ApiError::bad_request_with_body(json!({
            "error": "invalid_scope",
            "message": "scope muss 'active' oder 'all' sein.",
        }))
    })
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Micros, false)
}

#[derive(Debug, Deserialize)]
pub struct OverviewQuery {
    pub scope: Option<String>,
}

/// `GET /twitch/api/admin/config/overview` — Aggregat-Read der Admin-Config:
/// ausgewerteter Promo-Modus + Raid-/Chat-Flag-Snapshots (Python
/// `_api_admin_config_overview`). `announcements` spiegelt die Promo-Config,
/// `csrfToken`/`csrf_token` sind `null` (CSRF im Rust-Dashboard nicht portiert).
///
/// Der Python-Snapshot-Loader baut zusätzlich changelog/raid_history, die der
/// Overview-Endpoint aber NICHT in seine Antwort übernimmt — daher hier weggelassen.
pub async fn config_overview_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(params): Query<OverviewQuery>,
) -> Result<impl IntoResponse, ApiError> {
    if let Some(err) = crate::auth::require_admin(&auth) {
        return Err(err);
    }
    let scope = parse_admin_scope(params.scope.as_deref()).ok_or_else(|| {
        ApiError::bad_request_with_body(json!({
            "error": "invalid_scope",
            "message": "scope muss 'active' oder 'all' sein.",
        }))
    })?;

    let promo_config = load_global_promo_mode(&pool).await.map_err(db_error)?;
    let evaluation = evaluate_global_promo_mode(&promo_config.to_json(), None);
    let snaps = load_streamer_config_snapshots(&pool, &scope).await.map_err(db_error)?;

    Ok(Json(json!({
        "promo": evaluation.to_json(),
        "raids": snaps.raid_snapshot(),
        "chat": snaps.chat_snapshot(),
        // announcements = Promo-Config-Sub-Objekt (Python promo.get("config", {})).
        "announcements": evaluation.config.to_json(),
        "csrfToken": Value::Null,
        "csrf_token": Value::Null,
    })))
}

/// `POST /twitch/api/admin/config/raids` — Raid-/Live-Ping-Flags netzweit setzen.
pub async fn config_raids_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    body: axum::body::Bytes,
) -> Result<impl IntoResponse, ApiError> {
    if let Some(err) = crate::auth::require_admin(&auth) {
        return Err(err);
    }
    let payload = parse_object_body(&body)?;

    let raid_bot_enabled = normalize_admin_bool(payload.get("raid_bot_enabled"));
    let live_ping_enabled = normalize_admin_bool(payload.get("live_ping_enabled"));
    let (Some(raid_bot_enabled), Some(live_ping_enabled)) = (raid_bot_enabled, live_ping_enabled) else {
        return Err(ApiError::bad_request_with_body(json!({
            "error": "validation_failed",
            "validation": [
                { "path": "raid_bot_enabled", "message": "raid_bot_enabled muss boolean sein." },
                { "path": "live_ping_enabled", "message": "live_ping_enabled muss boolean sein." },
            ],
        })));
    };
    let scope = scope_or_error(&payload)?;

    let count = bulk_update_partner_flags(
        &pool,
        &PartnerFlagUpdate {
            raid_bot_enabled: Some(raid_bot_enabled),
            live_ping_enabled: Some(live_ping_enabled),
            ..Default::default()
        },
    )
    .await
    .map_err(db_error)?;
    let snaps = load_streamer_config_snapshots(&pool, &scope).await.map_err(db_error)?;

    let mut raids = snaps.raid_snapshot();
    raids["raidBotEnabled"] = json!(raid_bot_enabled);
    raids["livePingEnabled"] = json!(live_ping_enabled);

    Ok(Json(json!({
        "ok": true,
        "scope": scope,
        "updatedAt": now_iso(),
        "updatedBy": "admin",
        "targetCount": count,
        "updatedCount": count,
        "raids": raids,
        "chat": snaps.chat_snapshot(),
    })))
}

/// `POST /twitch/api/admin/config/chat` — Silent-Ban/Silent-Raid netzweit setzen.
pub async fn config_chat_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    body: axum::body::Bytes,
) -> Result<impl IntoResponse, ApiError> {
    if let Some(err) = crate::auth::require_admin(&auth) {
        return Err(err);
    }
    let payload = parse_object_body(&body)?;

    let silent_ban = normalize_admin_bool(payload.get("silent_ban"));
    let silent_raid = normalize_admin_bool(payload.get("silent_raid"));
    let (Some(silent_ban), Some(silent_raid)) = (silent_ban, silent_raid) else {
        return Err(ApiError::bad_request_with_body(json!({
            "error": "validation_failed",
            "validation": [
                { "path": "silent_ban", "message": "silent_ban muss boolean sein." },
                { "path": "silent_raid", "message": "silent_raid muss boolean sein." },
            ],
        })));
    };
    let scope = scope_or_error(&payload)?;

    let count = bulk_update_partner_flags(
        &pool,
        &PartnerFlagUpdate {
            silent_ban: Some(silent_ban),
            silent_raid: Some(silent_raid),
            ..Default::default()
        },
    )
    .await
    .map_err(db_error)?;
    let snaps = load_streamer_config_snapshots(&pool, &scope).await.map_err(db_error)?;

    let mut chat = snaps.chat_snapshot();
    chat["silentBan"] = json!(silent_ban);
    chat["silentRaid"] = json!(silent_raid);

    Ok(Json(json!({
        "ok": true,
        "scope": scope,
        "updatedAt": now_iso(),
        "updatedBy": "admin",
        "targetCount": count,
        "updatedCount": count,
        "raids": snaps.raid_snapshot(),
        "chat": chat,
    })))
}

fn db_error(e: sqlx::Error) -> ApiError {
    tracing::error!("admin_config Bulk-Update/Snapshot Fehler: {e}");
    ApiError::internal()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Bytes;
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    #[test]
    fn normalize_bool_varianten() {
        assert_eq!(normalize_admin_bool(Some(&json!(true))), Some(true));
        assert_eq!(normalize_admin_bool(Some(&json!(1))), Some(true));
        assert_eq!(normalize_admin_bool(Some(&json!(0))), Some(false));
        assert_eq!(normalize_admin_bool(Some(&json!("on"))), Some(true));
        assert_eq!(normalize_admin_bool(Some(&json!("off"))), Some(false));
        assert_eq!(normalize_admin_bool(Some(&json!(2))), None);
        assert_eq!(normalize_admin_bool(Some(&json!("vielleicht"))), None);
        assert_eq!(normalize_admin_bool(None), None);
    }

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new().max_connections(1).connect(&dsn).await.unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).execute(&admin).await.unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&admin).await.unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn).unwrap().options([("search_path", schema)]);
        let pool = PgPoolOptions::new().max_connections(2).connect_with(opts).await.unwrap();
        sqlx::query(
            "CREATE TABLE twitch_partners (twitch_user_id TEXT PRIMARY KEY, twitch_login TEXT, status TEXT, \
             raid_bot_enabled INTEGER DEFAULT 0, live_ping_enabled INTEGER DEFAULT 1, \
             silent_ban INTEGER DEFAULT 0, silent_raid INTEGER DEFAULT 0)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE twitch_global_promo_modes (\
                config_key TEXT PRIMARY KEY, mode TEXT NOT NULL DEFAULT 'standard', \
                custom_message TEXT, starts_at TEXT, ends_at TEXT, \
                is_enabled INTEGER NOT NULL DEFAULT 0, \
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP, updated_by TEXT)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO twitch_partners (twitch_user_id, twitch_login, status) VALUES ('a', 'a', 'active')")
            .execute(&pool).await.unwrap();
        Some(pool)
    }

    async fn body_json(r: Result<impl IntoResponse, ApiError>) -> (StatusCode, Value) {
        let resp = r.into_response();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), 65536).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    #[tokio::test]
    async fn raids_unauth_auth_required_401() {
        let Some(pool) = make_pool("t_acfg_raids_unauth").await else { return };
        let (s, _) = body_json(config_raids_handler(DashboardAuthLevel::None, State(pool), Bytes::from("{}")).await).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn raids_validation_und_scope() {
        let Some(pool) = make_pool("t_acfg_raids_val").await else { return };
        // fehlende bools → validation_failed.
        let (s, _) = body_json(config_raids_handler(DashboardAuthLevel::admin(), State(pool.clone()), Bytes::from(r#"{"scope":"active"}"#)).await).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        // ungültiger scope → invalid_scope.
        let body = r#"{"raid_bot_enabled":true,"live_ping_enabled":true,"scope":"bogus"}"#;
        let (s, j) = body_json(config_raids_handler(DashboardAuthLevel::admin(), State(pool), Bytes::from(body)).await).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        assert_eq!(j["error"], "invalid_scope");
    }

    #[tokio::test]
    async fn raids_happy_setzt_und_snapshot() {
        let Some(pool) = make_pool("t_acfg_raids_ok").await else { return };
        let body = r#"{"raid_bot_enabled":true,"live_ping_enabled":false,"scope":"active"}"#;
        let (s, j) = body_json(config_raids_handler(DashboardAuthLevel::admin(), State(pool.clone()), Bytes::from(body)).await).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(j["ok"], true);
        assert_eq!(j["updatedCount"], 1);
        assert_eq!(j["raids"]["raidBotEnabled"], true);
        assert_eq!(j["raids"]["livePingEnabled"], false);
        assert_eq!(j["raids"]["raidBotEnabledCount"], 1);
        // DB: aktiver Partner hat raid_bot_enabled=1.
        let v: i32 = sqlx::query_scalar("SELECT raid_bot_enabled FROM twitch_partners WHERE twitch_user_id='a'")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(v, 1);
    }

    #[tokio::test]
    async fn overview_aggregiert_promo_raids_chat() {
        let Some(pool) = make_pool("t_acfg_overview").await else { return };
        // scope=None → active. Das Testschema bildet die Migration bereits ab.
        let r = config_overview_handler(DashboardAuthLevel::admin(), State(pool.clone()), Query(OverviewQuery { scope: None })).await;
        let (s, j) = body_json(r).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(j["promo"]["status"], "standard"); // Default ohne gesetzten Modus
        assert_eq!(j["raids"]["totalManagedStreamers"], 1);
        assert_eq!(j["chat"]["totalManagedStreamers"], 1);
        assert!(j["announcements"].is_object());
        assert!(j["csrfToken"].is_null());
        assert!(j["csrf_token"].is_null());

        // unauth → auth_required, bad scope → 400.
        let (s, _) = body_json(config_overview_handler(DashboardAuthLevel::None, State(pool.clone()), Query(OverviewQuery { scope: None })).await).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
        let (s, j) = body_json(config_overview_handler(DashboardAuthLevel::admin(), State(pool), Query(OverviewQuery { scope: Some("bogus".into()) })).await).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        assert_eq!(j["error"], "invalid_scope");
    }

    #[tokio::test]
    async fn chat_happy_setzt_silent() {
        let Some(pool) = make_pool("t_acfg_chat_ok").await else { return };
        let body = r#"{"silent_ban":true,"silent_raid":true,"scope":"active"}"#;
        let (s, j) = body_json(config_chat_handler(DashboardAuthLevel::admin(), State(pool.clone()), Bytes::from(body)).await).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(j["chat"]["silentBan"], true);
        assert_eq!(j["chat"]["allSilentRaid"], true);
        let v: i32 = sqlx::query_scalar("SELECT silent_ban FROM twitch_partners WHERE twitch_user_id='a'")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(v, 1);
    }
}
