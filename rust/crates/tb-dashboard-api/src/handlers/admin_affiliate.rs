//! Admin-Affiliate-Übersichten (Read-Only).
//!
//! Port von `bot/analytics/api_admin.py:_api_admin_affiliate_*`. Datenschicht in
//! [`tb_analytics::admin_affiliate`]. Admin über `AuthLevel::is_privileged`.
//!
//! Status: stats portiert; list/detail/gutschriften folgen als Teil 2+.

use axum::{
    extract::{Path, State},
    response::IntoResponse,
    Json,
};
use chrono::{Datelike, DateTime, SecondsFormat, Utc};
use serde_json::json;
use sqlx::PgPool;
use tb_analytics::admin_affiliate::ToggleError;
use tb_http_core::{ApiError, AuthLevel};

/// Monatsanfang (1. des aktuellen Monats, 00:00 UTC) als ISO-String — Python
/// `datetime.now(UTC).replace(day=1, hour=0, ...).isoformat()`.
fn first_of_month_utc_iso() -> String {
    let now = Utc::now();
    let first_day = now.date_naive().with_day(1).unwrap_or_else(|| now.date_naive());
    let first = first_day.and_hms_opt(0, 0, 0).unwrap_or_else(|| now.naive_utc());
    DateTime::<Utc>::from_naive_utc_and_offset(first, Utc).to_rfc3339_opts(SecondsFormat::Secs, false)
}

/// `GET /twitch/api/admin/affiliates/stats` — Affiliate-Programm-Statistik (Admin).
pub async fn stats_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }
    let month_start = first_of_month_utc_iso();
    match tb_analytics::admin_affiliate::load_affiliate_stats(&pool, &month_start).await {
        Ok(v) => Ok(Json(v)),
        Err(e) => {
            tracing::error!("affiliate-stats SELECT-Fehler: {e}");
            Err(ApiError::internal())
        }
    }
}

/// `GET /twitch/api/admin/affiliates` — Affiliate-Liste mit Claims/Provisionen (Admin).
pub async fn list_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }
    match tb_analytics::admin_affiliate::load_affiliates_list(&pool).await {
        Ok(v) => Ok(Json(v)),
        Err(e) => {
            tracing::error!("affiliate-list SELECT-Fehler: {e}");
            Err(ApiError::internal())
        }
    }
}

/// `GET /twitch/api/admin/affiliates/gutschriften` — alle Gutschriften (Admin).
pub async fn gutschriften_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }
    match tb_analytics::admin_affiliate::load_affiliate_gutschriften(&pool).await {
        Ok(v) => Ok(Json(v)),
        Err(e) => {
            tracing::error!("affiliate-gutschriften SELECT-Fehler: {e}");
            Err(ApiError::internal())
        }
    }
}

/// `POST /twitch/api/admin/affiliates/:login/toggle` — is_active flippen (Admin).
pub async fn toggle_handler(
    auth: AuthLevel,
    State(pool): State<PgPool>,
    Path(login_raw): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }
    let Some(login) = tb_domain::login::normalize_twitch_login(&login_raw) else {
        return Err(ApiError::bad_request_with_body(json!({ "error": "invalid_login" })));
    };
    match tb_analytics::admin_affiliate::toggle_affiliate(&pool, &login).await {
        Ok(v) => Ok(Json(v)),
        Err(ToggleError::NotFound) => Err(ApiError::not_found()),
        Err(ToggleError::Db(e)) => {
            tracing::error!("affiliate-toggle Fehler: {e}");
            Err(ApiError::internal())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use serde_json::Value;
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

    #[test]
    fn month_start_format() {
        let s = first_of_month_utc_iso();
        assert!(s.ends_with("-01T00:00:00+00:00"), "Monatsanfang ISO: {s}");
    }

    #[tokio::test]
    async fn unauth_401() {
        let Some(pool) = make_pool("t_affh_unauth").await else { return };
        let (s, _) = body_json(stats_handler(AuthLevel::None, State(pool)).await).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn ohne_tabellen_liefert_nullwerte_200() {
        let Some(pool) = make_pool("t_affh_empty").await else { return };
        let (s, j) = body_json(stats_handler(AuthLevel::Admin, State(pool)).await).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(j["total_affiliates"], 0);
        assert_eq!(j["total_provision"], 0.0);
    }

    #[tokio::test]
    async fn list_unauth_und_leer() {
        let Some(pool) = make_pool("t_affh_list").await else { return };
        let (s, _) = body_json(list_handler(AuthLevel::None, State(pool.clone())).await).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
        // ohne Tabellen → {affiliates: []}.
        let (s, j) = body_json(list_handler(AuthLevel::Admin, State(pool)).await).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(j["affiliates"], serde_json::json!([]));
    }

    #[tokio::test]
    async fn gutschriften_unauth_und_leer() {
        let Some(pool) = make_pool("t_affh_gut").await else { return };
        let (s, _) = body_json(gutschriften_handler(AuthLevel::None, State(pool.clone())).await).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
        let (s, j) = body_json(gutschriften_handler(AuthLevel::Admin, State(pool)).await).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(j["count"], 0);
        assert_eq!(j["gutschriften"], serde_json::json!([]));
    }

    #[tokio::test]
    async fn toggle_auth_invalid_notfound_happy() {
        let Some(pool) = make_pool("t_affh_toggle").await else { return };
        // unauth → 401.
        let (s, _) = body_json(toggle_handler(AuthLevel::None, State(pool.clone()), Path("nani".into())).await).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
        // ungültiger Login → 400.
        let (s, j) = body_json(toggle_handler(AuthLevel::Admin, State(pool.clone()), Path("!!!".into())).await).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        assert_eq!(j["error"], "invalid_login");
        // unbekannt (kein Schema) → 404.
        let (s, _) = body_json(toggle_handler(AuthLevel::Admin, State(pool.clone()), Path("ghostuser".into())).await).await;
        assert_eq!(s, StatusCode::NOT_FOUND);
        // happy: Tabelle + Zeile → 200, active false.
        sqlx::query("CREATE TABLE affiliate_accounts (twitch_login TEXT PRIMARY KEY, is_active INTEGER NOT NULL DEFAULT 1, updated_at TEXT)").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO affiliate_accounts (twitch_login, is_active) VALUES ('nani', 1)").execute(&pool).await.unwrap();
        let (s, j) = body_json(toggle_handler(AuthLevel::Admin, State(pool), Path("nani".into())).await).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(j["login"], "nani");
        assert_eq!(j["active"], false);
    }
}
