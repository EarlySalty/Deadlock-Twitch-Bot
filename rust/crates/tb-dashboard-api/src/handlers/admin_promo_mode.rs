//! Admin-Endpoint zum Setzen des globalen Promo-Modus.
//!
//! Port von `bot/analytics/api_admin.py:_api_admin_config_promo`
//! (`POST /twitch/api/admin/config/promo`). Validiert die rohe Config und
//! persistiert sie als Singleton (`twitch_global_promo_modes`). Die Auswertung
//! + der Chat-Konsum liegen in [`tb_analytics::promo_mode`] (geteilte Logik mit
//!   `tb-chat::promos`).
//!
//! CSRF wird — wie im übrigen Rust-Dashboard (s. `internal_home.rs`, `auth_status`
//! liefert `csrfToken: null`) — bewusst NICHT geprüft; der Admin-Zugriff ist
//! über die Dashboard-Session-Auth (`DashboardAuthLevel`) abgesichert.

use axum::{extract::{Extension, State}, http::HeaderMap, response::IntoResponse, Json};
use serde_json::{json, Value};
use sqlx::PgPool;
use crate::auth::level::DashboardAuthLevel;
use tb_http_core::ApiError;

use crate::auth::session::DashboardAuthState;
use crate::handlers::admin_actor;
use tb_analytics::promo_mode::{save_global_promo_mode, validate_global_promo_mode_config, SavePromoModeError};

/// `POST /twitch/api/admin/config/promo` — globalen Promo-Modus setzen (Admin).
pub async fn set_promo_handler(
    auth: DashboardAuthLevel,
    config: Option<Extension<DashboardAuthState>>,
    headers: HeaderMap,
    State(pool): State<PgPool>,
    body: axum::body::Bytes,
) -> Result<impl IntoResponse, ApiError> {
    if let Some(err) = crate::auth::require_admin(&auth) {
        return Err(err);
    }

    let payload: Value = match serde_json::from_slice(&body) {
        Ok(v @ Value::Object(_)) => v,
        Ok(_) => return Err(ApiError::bad_request_with_body(json!({ "error": "invalid_payload" }))),
        Err(_) => return Err(ApiError::bad_request_with_body(json!({ "error": "invalid_json" }))),
    };

    // Validierung der rohen Config (Python: issues → 400 validation_failed).
    let (_normalized, issues) = validate_global_promo_mode_config(&payload);
    if !issues.is_empty() {
        let validation: Vec<Value> = issues.iter().map(|i| i.to_json()).collect();
        return Err(ApiError::bad_request_with_body(json!({
            "error": "validation_failed",
            "validation": validation,
        })));
    }

    let actor = admin_actor::admin_actor_label(config.as_ref(), &headers).await;
    match save_global_promo_mode(&pool, &payload, &actor).await {
        Ok(config) => Ok(Json(config.to_json())),
        Err(SavePromoModeError::Validation(msg)) => {
            Err(ApiError::bad_request_with_body(json!({ "error": msg })))
        }
        Err(SavePromoModeError::Db(e)) => {
            tracing::error!("save_global_promo_mode Fehler: {e}");
            Err(ApiError::internal())
        }
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
        Some(PgPoolOptions::new().max_connections(2).connect_with(opts).await.unwrap())
    }

    async fn body_json(r: Result<impl IntoResponse, ApiError>) -> (StatusCode, Value) {
        let resp = r.into_response();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), 65536).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    #[tokio::test]
    async fn unauth_ist_auth_required_401() {
        let Some(pool) = make_pool("t_promo_unauth").await else { return };
        let resp = set_promo_handler(DashboardAuthLevel::None, None, HeaderMap::new(), State(pool), Bytes::from("{}")).await;
        let (status, body) = body_json(resp).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"], "auth_required");
        assert_eq!(body["required"], "admin");
    }

    #[tokio::test]
    async fn custom_event_ohne_message_400() {
        let Some(pool) = make_pool("t_promo_validation").await else { return };
        // custom_event aktiv ohne custom_message → validation_failed.
        let body = Bytes::from(r#"{"mode":"custom_event","is_enabled":true}"#);
        let resp = set_promo_handler(DashboardAuthLevel::admin(), None, HeaderMap::new(), State(pool), body).await;
        let (status, _) = body_json(resp).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn standard_speichern_200() {
        let Some(pool) = make_pool("t_promo_save").await else { return };
        let body = Bytes::from(r#"{"mode":"standard"}"#);
        let resp = set_promo_handler(DashboardAuthLevel::admin(), None, HeaderMap::new(), State(pool.clone()), body).await;
        let (status, _) = body_json(resp).await;
        assert_eq!(status, StatusCode::OK);
        // Persistiert: load liefert standard.
        let cfg = tb_analytics::promo_mode::load_global_promo_mode(&pool).await.unwrap();
        assert_eq!(cfg.mode, "standard");
    }

    #[tokio::test]
    async fn custom_event_speichern_und_aktiv() {
        let Some(pool) = make_pool("t_promo_custom").await else { return };
        let body = Bytes::from(r#"{"mode":"custom_event","is_enabled":true,"custom_message":"Event bei {invite}!"}"#);
        let resp = set_promo_handler(DashboardAuthLevel::admin(), None, HeaderMap::new(), State(pool.clone()), body).await;
        let (status, _) = body_json(resp).await;
        assert_eq!(status, StatusCode::OK);
        let cfg = tb_analytics::promo_mode::load_global_promo_mode(&pool).await.unwrap();
        let eval = tb_analytics::promo_mode::evaluate_global_promo_mode(&cfg.to_json(), None);
        assert_eq!(eval.status, "active");
        assert_eq!(eval.active_message.as_deref(), Some("Event bei {invite}!"));
    }
}
