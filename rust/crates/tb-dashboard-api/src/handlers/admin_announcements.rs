//! Admin-Announcements-Editor (Stream-Promo-Event-Text).
//!
//! Port von `bot/analytics/api_admin.py:_api_admin_announcements` (+ `_save`).
//! „Announcements" ist nur eine vereinfachte Sicht auf den globalen Promo-Modus
//! ([`tb_analytics::promo_mode`]): GET liest die `custom_message` als `body`,
//! POST überschreibt **nur** die `custom_message` (Modus/Zeitfenster/is_enabled
//! bleiben erhalten) und validiert + speichert via `save_global_promo_mode`.
//!
//! Admin über `DashboardAuthLevel`; CSRF erzwingt der `csrf_protect`-Layer
//! des admin_config_routers (B3-7). updated_by nutzt die Discord-Admin-Session,
//! sonst Pythons Fallback `admin`.

use axum::{extract::{Extension, State}, http::HeaderMap, response::IntoResponse, Json};
use serde_json::{json, Value};
use sqlx::PgPool;
use crate::auth::level::DashboardAuthLevel;
use tb_http_core::ApiError;

use crate::auth::session::DashboardAuthState;
use crate::handlers::admin_actor;
use tb_analytics::promo_mode::{
    load_global_promo_mode, save_global_promo_mode, PromoModeConfig, SavePromoModeError,
};

/// `{ body, lastUpdatedAt, lastUpdatedBy }` (Python `_admin_announcement_payload`).
fn announcement_payload(config: &PromoModeConfig) -> Value {
    json!({
        "body": config.custom_message,
        "lastUpdatedAt": config.updated_at,
        "lastUpdatedBy": if config.updated_by.trim().is_empty() {
            Value::Null
        } else {
            json!(config.updated_by.trim())
        },
    })
}

fn db_error(e: sqlx::Error) -> ApiError {
    tracing::error!("admin_announcements DB-Fehler: {e}");
    ApiError::internal()
}

/// `GET /twitch/api/admin/announcements` — aktuellen Event-Text lesen (Admin).
pub async fn get_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
) -> Result<impl IntoResponse, ApiError> {
    if let Some(err) = crate::auth::require_admin(&auth) {
        return Err(err);
    }
    let config = load_global_promo_mode(&pool).await.map_err(db_error)?;
    Ok(Json(announcement_payload(&config)))
}

/// `POST /twitch/api/admin/announcements` — Event-Text setzen (Admin).
///
/// Überschreibt nur `custom_message`; der restliche Promo-Modus bleibt. Fehlt der
/// `body`-Schlüssel → 400 `validation_failed`. Validierungsfehler aus
/// `save_global_promo_mode` (z. B. custom_event-Modus mit leerem Text) → 400.
pub async fn save_handler(
    auth: DashboardAuthLevel,
    config: Option<Extension<DashboardAuthState>>,
    headers: HeaderMap,
    State(pool): State<PgPool>,
    body: axum::body::Bytes,
) -> Result<impl IntoResponse, ApiError> {
    if let Some(err) = crate::auth::require_admin(&auth) {
        return Err(err);
    }

    // Python: `"body" not in payload` → 400. Erfordert ein Objekt mit body-Key.
    let payload: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let Some(body_val) = payload.as_object().and_then(|o| o.get("body")) else {
        return Err(ApiError::bad_request_with_body(json!({
            "error": "validation_failed",
            "message": "body ist erforderlich.",
        })));
    };
    // Python `str(payload.get("body") or "")`: null/leer → "".
    let body_str = body_val.as_str().unwrap_or("").to_string();

    // Aktuelle Config laden, nur custom_message überschreiben, dann speichern.
    let current = load_global_promo_mode(&pool).await.map_err(db_error)?;
    let mut raw = current.to_json();
    raw["custom_message"] = json!(body_str);

    let actor = admin_actor::admin_actor_label(config.as_ref(), &headers).await;
    match save_global_promo_mode(&pool, &raw, &actor).await {
        Ok(saved) => Ok(Json(announcement_payload(&saved))),
        Err(SavePromoModeError::Validation(msg)) => {
            Err(ApiError::bad_request_with_body(json!({ "error": msg })))
        }
        Err(SavePromoModeError::Db(e)) => Err(db_error(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Bytes;
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new().max_connections(1).connect(&dsn).await.unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).execute(&admin).await.unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&admin).await.unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn).unwrap().options([("search_path", schema)]);
        let pool = PgPoolOptions::new().max_connections(2).connect_with(opts).await.unwrap();
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
        Some(pool)
    }

    async fn body_json(r: Result<impl IntoResponse, ApiError>) -> (StatusCode, Value) {
        let resp = r.into_response();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), 65536).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    fn partner_auth() -> DashboardAuthLevel {
        DashboardAuthLevel::Partner {
            twitch_login: "partner".to_string(),
            twitch_user_id: "42".to_string(),
            display_name: "Partner".to_string(),
        }
    }

    #[tokio::test]
    async fn get_fresh_ist_leer() {
        let Some(pool) = make_pool("t_ann_get").await else { return };
        let (s, j) = body_json(get_handler(DashboardAuthLevel::admin(), State(pool)).await).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(j["body"], "");
        assert!(j["lastUpdatedAt"].is_null());
        assert!(j["lastUpdatedBy"].is_null());
    }

    #[tokio::test]
    async fn unauth_auth_required_401() {
        let Some(pool) = make_pool("t_ann_unauth").await else { return };
        let (s, j) = body_json(get_handler(DashboardAuthLevel::None, State(pool.clone())).await).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
        assert_eq!(j["error"], "auth_required");
        assert_eq!(j["required"], "admin");
        let (s, j) = body_json(save_handler(DashboardAuthLevel::None, None, HeaderMap::new(), State(pool), Bytes::from(r#"{"body":"x"}"#)).await).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
        assert_eq!(j["error"], "auth_required");
        assert_eq!(j["required"], "admin");
    }

    #[tokio::test]
    async fn partner_admin_required_403() {
        let Some(pool) = make_pool("t_ann_partner").await else { return };
        let (s, j) = body_json(get_handler(partner_auth(), State(pool.clone())).await).await;
        assert_eq!(s, StatusCode::FORBIDDEN);
        assert_eq!(j["error"], "admin_required");
        assert_eq!(j["required"], "admin");
        let (s, j) = body_json(save_handler(partner_auth(), None, HeaderMap::new(), State(pool), Bytes::from(r#"{"body":"x"}"#)).await).await;
        assert_eq!(s, StatusCode::FORBIDDEN);
        assert_eq!(j["error"], "admin_required");
        assert_eq!(j["required"], "admin");
    }

    #[tokio::test]
    async fn save_ohne_body_key_400() {
        let Some(pool) = make_pool("t_ann_nobody").await else { return };
        let (s, j) = body_json(save_handler(DashboardAuthLevel::admin(), None, HeaderMap::new(), State(pool), Bytes::from(r#"{"foo":1}"#)).await).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        assert_eq!(j["error"], "validation_failed");
    }

    #[tokio::test]
    async fn save_und_roundtrip() {
        let Some(pool) = make_pool("t_ann_save").await else { return };
        // Default-Modus standard → custom_message darf beliebig sein.
        let (s, j) = body_json(save_handler(DashboardAuthLevel::admin(), None, HeaderMap::new(), State(pool.clone()), Bytes::from(r#"{"body":"Event-Wochenende!"}"#)).await).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(j["body"], "Event-Wochenende!");
        assert_eq!(j["lastUpdatedBy"], "admin");
        assert!(j["lastUpdatedAt"].is_string());
        // GET liest denselben Text.
        let (_s, j2) = body_json(get_handler(DashboardAuthLevel::admin(), State(pool)).await).await;
        assert_eq!(j2["body"], "Event-Wochenende!");
    }
}
