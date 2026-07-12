//! Admin-Affiliate-Übersichten (Read-Only).
//!
//! Port von `bot/analytics/api_admin.py:_api_admin_affiliate_*`. Datenschicht in
//! [`tb_analytics::admin_affiliate`]. Admin über `DashboardAuthLevel`.
//!
//! Status: stats portiert; list/detail/gutschriften folgen als Teil 2+.

use crate::auth::level::DashboardAuthLevel;
use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Datelike, SecondsFormat, Utc};
use serde_json::json;
use sqlx::PgPool;
use tb_analytics::admin_affiliate::{DetailError, ForLoginError, RateError, ToggleError};
use tb_analytics::affiliate_gutschrift::{
    self, AffiliateGutschriftEmailSender, AffiliateGutschriftSeller, GenerateGutschriftResult,
    GutschriftError, SmtpAffiliateEmailSender,
};
use tb_crypto::FieldCipher;
use tb_http_core::ApiError;

/// Monatsanfang (1. des aktuellen Monats, 00:00 UTC) als ISO-String — Python
/// `datetime.now(UTC).replace(day=1, hour=0, ...).isoformat()`.
fn first_of_month_utc_iso() -> String {
    let now = Utc::now();
    let first_day = now
        .date_naive()
        .with_day(1)
        .unwrap_or_else(|| now.date_naive());
    let first = first_day
        .and_hms_opt(0, 0, 0)
        .unwrap_or_else(|| now.naive_utc());
    DateTime::<Utc>::from_naive_utc_and_offset(first, Utc)
        .to_rfc3339_opts(SecondsFormat::Secs, false)
}

fn env_secret(keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        std::env::var(key)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

fn public_url_from_env() -> Option<String> {
    env_secret(&[
        "TWITCH_PUBLIC_DASHBOARD_BASE_URL",
        "TWITCH_PUBLIC_URL",
        "PUBLIC_URL",
        "TWITCH_ADMIN_PUBLIC_URL",
        "MASTER_DASHBOARD_PUBLIC_URL",
    ])
}

fn payload_from_body(body: &Bytes) -> serde_json::Value {
    if body.is_empty() {
        return json!({});
    }
    match serde_json::from_slice::<serde_json::Value>(body) {
        Ok(value) if value.is_object() => value,
        _ => json!({}),
    }
}

fn strict_payload_from_body(body: &Bytes) -> Result<serde_json::Value, ApiError> {
    if body.is_empty() {
        return Ok(json!({}));
    }
    match serde_json::from_slice::<serde_json::Value>(body) {
        Ok(value) if value.is_object() => Ok(value),
        Ok(serde_json::Value::Null) => Ok(json!({})),
        Ok(_) => Err(ApiError::bad_request_with_body(
            json!({ "error": "invalid_payload" }),
        )),
        Err(_) => Ok(json!({})),
    }
}

fn payload_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => String::new(),
        serde_json::Value::String(value) => value.clone(),
        serde_json::Value::Bool(value) => value.to_string(),
        serde_json::Value::Number(value) => value.to_string(),
        other => other.to_string(),
    }
}

fn first_payload_text(payload: &serde_json::Value, keys: &[&str]) -> String {
    keys.iter()
        .filter_map(|key| payload.get(*key).map(payload_text))
        .find(|value| !value.trim().is_empty())
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn payload_bool(payload: &serde_json::Value, key: &str) -> bool {
    let raw = payload
        .get(key)
        .map(payload_text)
        .unwrap_or_default()
        .trim()
        .to_lowercase()
        .to_string();
    matches!(raw.as_str(), "1" | "true" | "yes" | "on")
}

fn payload_field_present(payload: &serde_json::Value, key: &str) -> bool {
    payload
        .get(key)
        .map(payload_text)
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

fn payload_i32(payload: &serde_json::Value, key: &str) -> Option<i32> {
    let value = payload.get(key)?;
    match value {
        serde_json::Value::Number(number) => {
            number.as_i64().and_then(|value| i32::try_from(value).ok())
        }
        serde_json::Value::Bool(value) => Some(if *value { 1 } else { 0 }),
        _ => payload_text(value).trim().parse::<i32>().ok(),
    }
}

fn gutschrift_error_to_api(error: GutschriftError) -> ApiError {
    match error {
        GutschriftError::InvalidLogin => {
            ApiError::bad_request_with_body(json!({ "error": "invalid_login" }))
        }
        GutschriftError::InvalidPeriod | GutschriftError::InvalidYearMonth => {
            ApiError::bad_request_with_body(json!({ "error": "invalid_period" }))
        }
        other => {
            tracing::error!("affiliate-gutschriften job failed: {other}");
            ApiError::internal()
        }
    }
}

async fn run_gutschrift_job(
    pool: &PgPool,
    cipher: &FieldCipher,
    affiliate_login: Option<&str>,
    year: Option<i32>,
    month: Option<i32>,
    force: bool,
) -> Result<Vec<GenerateGutschriftResult>, GutschriftError> {
    let email_sender = SmtpAffiliateEmailSender::from_secret_loader(env_secret);
    let sender_ref = email_sender
        .as_ref()
        .map(|sender| sender as &dyn AffiliateGutschriftEmailSender);
    let public_url = public_url_from_env();
    let seller = AffiliateGutschriftSeller::from_secret_loader(env_secret, public_url.as_deref());

    if let (Some(year), Some(month)) = (year, month) {
        return affiliate_gutschrift::generate_monthly_gutschriften(
            pool,
            cipher,
            year,
            month,
            sender_ref,
            Some(&seller),
            affiliate_login,
            force,
        )
        .await;
    }

    if let Some(login) = affiliate_login {
        let mut results = Vec::new();
        for (due_login, due_year, due_month) in
            affiliate_gutschrift::due_periods(pool, None).await?
        {
            if due_login != login {
                continue;
            }
            results.push(
                affiliate_gutschrift::generate_for_period(
                    pool,
                    cipher,
                    &due_login,
                    due_year,
                    due_month,
                    sender_ref,
                    Some(&seller),
                    force,
                )
                .await?,
            );
        }
        return Ok(results);
    }

    affiliate_gutschrift::run_pending(pool, cipher, sender_ref, Some(&seller), None, 100).await
}

async fn generate_gutschriften_payload(
    pool: &PgPool,
    payload: serde_json::Value,
) -> Result<serde_json::Value, ApiError> {
    let raw_login = first_payload_text(&payload, &["affiliate_login", "twitch_login", "login"]);
    let affiliate_login = if raw_login.is_empty() {
        None
    } else {
        Some(
            tb_domain::login::normalize_twitch_login(&raw_login).ok_or_else(|| {
                ApiError::bad_request_with_body(json!({ "error": "invalid_login" }))
            })?,
        )
    };

    let year_present = payload_field_present(&payload, "year");
    let month_present = payload_field_present(&payload, "month");
    let (year, month) = if year_present || month_present {
        if !year_present || !month_present {
            return Err(ApiError::bad_request_with_body(
                json!({ "error": "invalid_period" }),
            ));
        }
        let Some(year) = payload_i32(&payload, "year") else {
            return Err(ApiError::bad_request_with_body(
                json!({ "error": "invalid_period" }),
            ));
        };
        let Some(month) = payload_i32(&payload, "month") else {
            return Err(ApiError::bad_request_with_body(
                json!({ "error": "invalid_period" }),
            ));
        };
        if year < 2000 || !(1..=12).contains(&month) {
            return Err(ApiError::bad_request_with_body(
                json!({ "error": "invalid_period" }),
            ));
        }
        (Some(year), Some(month))
    } else {
        (None, None)
    };

    let force = payload_bool(&payload, "force");
    let cipher = FieldCipher::from_env().map_err(|error| {
        tracing::error!("affiliate-gutschriften: FieldCipher unavailable: {error}");
        ApiError::internal()
    })?;
    let results = run_gutschrift_job(
        pool,
        &cipher,
        affiliate_login.as_deref(),
        year,
        month,
        force,
    )
    .await
    .map_err(gutschrift_error_to_api)?;

    Ok(json!({ "ok": true, "results": results }))
}

pub async fn run_pending_gutschriften_for_background(
    pool: &PgPool,
) -> Result<Vec<GenerateGutschriftResult>, String> {
    let cipher =
        FieldCipher::from_env().map_err(|error| format!("FieldCipher unavailable: {error}"))?;
    run_gutschrift_job(pool, &cipher, None, None, None, false)
        .await
        .map_err(|error| error.to_string())
}

/// `GET /twitch/api/admin/affiliates/stats` — Affiliate-Programm-Statistik (Admin).
pub async fn stats_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
) -> Result<impl IntoResponse, ApiError> {
    if let Some(err) = crate::auth::require_admin(&auth) {
        return Err(err);
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
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
) -> Result<impl IntoResponse, ApiError> {
    if let Some(err) = crate::auth::require_admin(&auth) {
        return Err(err);
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
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
) -> Result<impl IntoResponse, ApiError> {
    if let Some(err) = crate::auth::require_admin(&auth) {
        return Err(err);
    }
    match tb_analytics::admin_affiliate::load_affiliate_gutschriften(&pool).await {
        Ok(v) => Ok(Json(v)),
        Err(e) => {
            tracing::error!("affiliate-gutschriften SELECT-Fehler: {e}");
            Err(ApiError::internal())
        }
    }
}

/// `GET /twitch/api/admin/affiliates/gutschriften/:gutschrift_id/pdf` —
/// gespeichertes Gutschrift-PDF herunterladen (Admin). Streamt das `pdf_blob`-
/// BYTEA als `application/pdf` (kein Generieren).
pub async fn gutschrift_pdf_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Path(id_raw): Path<String>,
) -> Response {
    if let Some(err) = crate::auth::require_admin(&auth) {
        return err.into_response();
    }
    let invalid = || {
        ApiError::bad_request_with_body(json!({ "error": "invalid_gutschrift_id" })).into_response()
    };
    let id: i64 = match id_raw.trim().parse() {
        Ok(n) if n > 0 => n,
        _ => return invalid(),
    };

    match tb_analytics::admin_affiliate::load_gutschrift_pdf(&pool, id).await {
        Ok(Some((name, bytes))) => {
            let filename = name.replace('"', "");
            let mut resp = (StatusCode::OK, bytes).into_response();
            let headers = resp.headers_mut();
            headers.insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/pdf"),
            );
            let disposition = format!("attachment; filename=\"{filename}.pdf\"");
            if let Ok(value) = HeaderValue::from_str(&disposition) {
                headers.insert(header::CONTENT_DISPOSITION, value);
            }
            resp
        }
        Ok(None) => ApiError::not_found().into_response(),
        Err(e) => {
            tracing::error!("affiliate-gutschrift-pdf Fehler: {e}");
            ApiError::internal().into_response()
        }
    }
}

/// `GET /twitch/api/admin/affiliates/:login` — Affiliate-Detail (Admin).
/// Inkl. PII-Readiness (entschlüsselt verschlüsselte Stammdaten via Field-Cipher).
pub async fn detail_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Path(login_raw): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    if let Some(err) = crate::auth::require_admin(&auth) {
        return Err(err);
    }
    let Some(login) = tb_domain::login::normalize_twitch_login(&login_raw) else {
        return Err(ApiError::bad_request_with_body(
            json!({ "error": "invalid_login" }),
        ));
    };
    let cipher = match FieldCipher::from_env() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("affiliate-detail: kein Field-Cipher ({e})");
            return Err(ApiError::internal());
        }
    };
    match tb_analytics::admin_affiliate::load_affiliate_detail(&pool, &cipher, &login).await {
        Ok(v) => Ok(Json(v)),
        Err(DetailError::NotFound) => Err(ApiError::not_found()),
        Err(DetailError::Db(e)) => {
            tracing::error!("affiliate-detail DB-Fehler: {e}");
            Err(ApiError::internal())
        }
        Err(DetailError::Decrypt(s)) => {
            tracing::error!("affiliate-detail PII-Decrypt-Fehler: {s}");
            Err(ApiError::internal())
        }
    }
}

/// `GET /twitch/api/admin/affiliates/:login/gutschriften` — Gutschriften eines
/// Affiliates inkl. Konto + PII-Readiness + Summary (Admin).
pub async fn gutschriften_for_login_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Path(login_raw): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    if let Some(err) = crate::auth::require_admin(&auth) {
        return Err(err);
    }
    let Some(login) = tb_domain::login::normalize_twitch_login(&login_raw) else {
        return Err(ApiError::bad_request_with_body(
            json!({ "error": "invalid_login" }),
        ));
    };
    let cipher = match FieldCipher::from_env() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("affiliate-gutschriften-for-login: kein Field-Cipher ({e})");
            return Err(ApiError::internal());
        }
    };
    match tb_analytics::admin_affiliate::load_gutschriften_for_login(&pool, &cipher, &login).await {
        Ok(v) => Ok(Json(v)),
        Err(ForLoginError::NotFound) => Err(ApiError::not_found()),
        Err(ForLoginError::Db(e)) => {
            tracing::error!("affiliate-gutschriften-for-login DB-Fehler: {e}");
            Err(ApiError::internal())
        }
        Err(ForLoginError::Decrypt(s)) => {
            tracing::error!("affiliate-gutschriften-for-login PII-Decrypt-Fehler: {s}");
            Err(ApiError::internal())
        }
    }
}

/// `POST /twitch/api/admin/affiliates/:login/toggle` — is_active flippen (Admin).
pub async fn toggle_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Path(login_raw): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    if let Some(err) = crate::auth::require_admin(&auth) {
        return Err(err);
    }
    let Some(login) = tb_domain::login::normalize_twitch_login(&login_raw) else {
        return Err(ApiError::bad_request_with_body(
            json!({ "error": "invalid_login" }),
        ));
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

/// `POST /twitch/api/admin/affiliates/:login/commission-rate` — Provisionssatz setzen.
pub async fn set_commission_rate_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Path(login_raw): Path<String>,
    body: Bytes,
) -> Result<impl IntoResponse, ApiError> {
    if let Some(err) = crate::auth::require_admin(&auth) {
        return Err(err);
    }
    let Some(login) = tb_domain::login::normalize_twitch_login(&login_raw) else {
        return Err(ApiError::bad_request_with_body(
            json!({ "error": "invalid_login" }),
        ));
    };
    let rate_pct = serde_json::from_slice::<serde_json::Value>(&body)
        .ok()
        .and_then(|payload| payload.get("commission_rate_pct")?.as_i64())
        .filter(|rate| (0..=100).contains(rate))
        .and_then(|rate| i16::try_from(rate).ok())
        .ok_or_else(|| ApiError::bad_request_with_body(json!({ "error": "invalid_rate" })))?;

    let old_rate_pct = sqlx::query_scalar::<_, i16>(
        "SELECT commission_rate_pct FROM affiliate_accounts WHERE twitch_login = $1",
    )
    .bind(&login)
    .fetch_optional(&pool)
    .await
    .ok()
    .flatten();

    match tb_analytics::admin_affiliate::set_commission_rate(&pool, &login, rate_pct).await {
        Ok(value) => {
            tracing::info!(
                affiliate_login = %login,
                old_rate_pct = ?old_rate_pct,
                new_rate_pct = rate_pct,
                "Affiliate-Provisionssatz geändert"
            );
            Ok(Json(value))
        }
        Err(RateError::NotFound) => Err(ApiError::not_found()),
        Err(RateError::Db(error)) => {
            tracing::error!(affiliate_login = %login, "affiliate-commission-rate Fehler: {error}");
            Err(ApiError::internal())
        }
    }
}

/// `POST /twitch/api/admin/affiliates/generate-gutschriften` — Gutschriften als
/// Admin anstoßen. CSRF wird vom Admin-Config-Router geprüft; der Body wird wie
/// Python tolerant als `{}` behandelt, wenn er fehlt oder kein JSON-Objekt ist.
pub async fn generate_gutschriften_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    body: Bytes,
) -> Result<impl IntoResponse, ApiError> {
    if let Some(err) = crate::auth::require_admin(&auth) {
        return Err(err);
    }

    Ok(Json(
        generate_gutschriften_payload(&pool, payload_from_body(&body)).await?,
    ))
}

/// `POST /twitch/api/affiliate/gutschriften/trigger` und Python-Alias
/// `/twitch/api/affiliate/admin/generate-gutschriften` — admin-gated, ohne
/// Header-CSRF-Layer wie Python `_affiliate_api_gutschrift_trigger`.
pub async fn generate_gutschriften_trigger_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    body: Bytes,
) -> Result<impl IntoResponse, ApiError> {
    if let Some(err) = crate::auth::require_admin(&auth) {
        return Err(err);
    }
    Ok(Json(
        generate_gutschriften_payload(&pool, strict_payload_from_body(&body)?).await?,
    ))
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
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn)
            .unwrap()
            .options([("search_path", schema)]);
        Some(
            PgPoolOptions::new()
                .max_connections(2)
                .connect_with(opts)
                .await
                .unwrap(),
        )
    }

    async fn body_json(r: Result<impl IntoResponse, ApiError>) -> (StatusCode, Value) {
        let resp = r.into_response();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), 65536).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    fn partner_auth() -> DashboardAuthLevel {
        DashboardAuthLevel::Partner {
            twitch_login: "partner".into(),
            twitch_user_id: "100".into(),
            display_name: "Partner".into(),
        }
    }

    #[test]
    fn month_start_format() {
        let s = first_of_month_utc_iso();
        assert!(s.ends_with("-01T00:00:00+00:00"), "Monatsanfang ISO: {s}");
    }

    #[tokio::test]
    async fn unauth_auth_required_401() {
        let Some(pool) = make_pool("t_affh_unauth").await else {
            return;
        };
        let (s, j) =
            body_json(stats_handler(DashboardAuthLevel::None, State(pool.clone())).await).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
        assert_eq!(j["error"], "auth_required");
        assert_eq!(j["required"], "admin");

        let (s, j) = body_json(stats_handler(partner_auth(), State(pool)).await).await;
        assert_eq!(s, StatusCode::FORBIDDEN);
        assert_eq!(j["error"], "admin_required");
        assert_eq!(j["required"], "admin");
    }

    #[tokio::test]
    async fn ohne_tabellen_liefert_nullwerte_200() {
        let Some(pool) = make_pool("t_affh_empty").await else {
            return;
        };
        let (s, j) = body_json(stats_handler(DashboardAuthLevel::admin(), State(pool)).await).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(j["total_affiliates"], 0);
        assert_eq!(j["total_provision"], 0.0);
    }

    #[tokio::test]
    async fn list_unauth_und_leer() {
        let Some(pool) = make_pool("t_affh_list").await else {
            return;
        };
        let (s, _) =
            body_json(list_handler(DashboardAuthLevel::None, State(pool.clone())).await).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
        // ohne Tabellen → {affiliates: []}.
        let (s, j) = body_json(list_handler(DashboardAuthLevel::admin(), State(pool)).await).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(j["affiliates"], serde_json::json!([]));
    }

    #[tokio::test]
    async fn gutschriften_unauth_und_leer() {
        let Some(pool) = make_pool("t_affh_gut").await else {
            return;
        };
        let (s, _) =
            body_json(gutschriften_handler(DashboardAuthLevel::None, State(pool.clone())).await)
                .await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
        let (s, j) =
            body_json(gutschriften_handler(DashboardAuthLevel::admin(), State(pool)).await).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(j["count"], 0);
        assert_eq!(j["gutschriften"], serde_json::json!([]));
    }

    #[tokio::test]
    async fn pdf_auth_invalid_notfound() {
        let Some(pool) = make_pool("t_affh_pdf").await else {
            return;
        };
        // unauth → auth_required.
        assert_eq!(
            gutschrift_pdf_handler(
                DashboardAuthLevel::None,
                State(pool.clone()),
                Path("5".into())
            )
            .await
            .status(),
            StatusCode::UNAUTHORIZED
        );
        // ungültige ID → 400.
        assert_eq!(
            gutschrift_pdf_handler(
                DashboardAuthLevel::admin(),
                State(pool.clone()),
                Path("0".into())
            )
            .await
            .status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            gutschrift_pdf_handler(
                DashboardAuthLevel::admin(),
                State(pool.clone()),
                Path("abc".into())
            )
            .await
            .status(),
            StatusCode::BAD_REQUEST
        );
        // kein Schema/keine Gutschrift → 404.
        assert_eq!(
            gutschrift_pdf_handler(DashboardAuthLevel::admin(), State(pool), Path("5".into()))
                .await
                .status(),
            StatusCode::NOT_FOUND
        );
    }

    #[tokio::test]
    async fn for_login_unauth_und_invalid() {
        let Some(pool) = make_pool("t_affh_forlogin").await else {
            return;
        };
        let (s, _) = body_json(
            gutschriften_for_login_handler(
                DashboardAuthLevel::None,
                State(pool.clone()),
                Path("nani".into()),
            )
            .await,
        )
        .await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
        let (s, j) = body_json(
            gutschriften_for_login_handler(
                DashboardAuthLevel::admin(),
                State(pool),
                Path("!!!".into()),
            )
            .await,
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        assert_eq!(j["error"], "invalid_login");
    }

    #[tokio::test]
    async fn detail_unauth_und_invalid_login() {
        let Some(pool) = make_pool("t_affh_detail").await else {
            return;
        };
        let (s, _) = body_json(
            detail_handler(
                DashboardAuthLevel::None,
                State(pool.clone()),
                Path("nani".into()),
            )
            .await,
        )
        .await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
        let (s, j) = body_json(
            detail_handler(DashboardAuthLevel::admin(), State(pool), Path("!!!".into())).await,
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        assert_eq!(j["error"], "invalid_login");
    }

    #[tokio::test]
    async fn toggle_auth_invalid_notfound_happy() {
        let Some(pool) = make_pool("t_affh_toggle").await else {
            return;
        };
        // unauth → auth_required.
        let (s, _) = body_json(
            toggle_handler(
                DashboardAuthLevel::None,
                State(pool.clone()),
                Path("nani".into()),
            )
            .await,
        )
        .await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
        let (s, j) = body_json(
            toggle_handler(partner_auth(), State(pool.clone()), Path("nani".into())).await,
        )
        .await;
        assert_eq!(s, StatusCode::FORBIDDEN);
        assert_eq!(j["error"], "admin_required");
        // ungültiger Login → 400.
        let (s, j) = body_json(
            toggle_handler(
                DashboardAuthLevel::admin(),
                State(pool.clone()),
                Path("!!!".into()),
            )
            .await,
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        assert_eq!(j["error"], "invalid_login");
        // unbekannt (kein Schema) → 404.
        let (s, _) = body_json(
            toggle_handler(
                DashboardAuthLevel::admin(),
                State(pool.clone()),
                Path("ghostuser".into()),
            )
            .await,
        )
        .await;
        assert_eq!(s, StatusCode::NOT_FOUND);
        // happy: Tabelle + Zeile → 200, active false.
        sqlx::query("CREATE TABLE affiliate_accounts (twitch_login TEXT PRIMARY KEY, is_active INTEGER NOT NULL DEFAULT 1, updated_at TEXT)").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO affiliate_accounts (twitch_login, is_active) VALUES ('nani', 1)")
            .execute(&pool)
            .await
            .unwrap();
        let (s, j) = body_json(
            toggle_handler(
                DashboardAuthLevel::admin(),
                State(pool),
                Path("nani".into()),
            )
            .await,
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(j["login"], "nani");
        assert_eq!(j["active"], false);
    }

    #[tokio::test]
    async fn set_commission_rate_auth_validation_notfound_and_happy() {
        let Some(pool) = make_pool("t_affh_rate").await else {
            return;
        };
        let valid_body = Bytes::from_static(br#"{"commission_rate_pct":40}"#);

        let (s, _) = body_json(
            set_commission_rate_handler(
                DashboardAuthLevel::None,
                State(pool.clone()),
                Path("nani".into()),
                valid_body.clone(),
            )
            .await,
        )
        .await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
        let (s, _) = body_json(
            set_commission_rate_handler(
                partner_auth(),
                State(pool.clone()),
                Path("nani".into()),
                valid_body.clone(),
            )
            .await,
        )
        .await;
        assert_eq!(s, StatusCode::FORBIDDEN);

        for body in [
            br#"{}"#.as_slice(),
            br#"{"commission_rate_pct":"40"}"#.as_slice(),
            br#"{"commission_rate_pct":101}"#.as_slice(),
        ] {
            let (s, j) = body_json(
                set_commission_rate_handler(
                    DashboardAuthLevel::admin(),
                    State(pool.clone()),
                    Path("nani".into()),
                    Bytes::copy_from_slice(body),
                )
                .await,
            )
            .await;
            assert_eq!(s, StatusCode::BAD_REQUEST);
            assert_eq!(j["error"], "invalid_rate");
        }

        let (s, _) = body_json(
            set_commission_rate_handler(
                DashboardAuthLevel::admin(),
                State(pool.clone()),
                Path("ghost".into()),
                valid_body.clone(),
            )
            .await,
        )
        .await;
        assert_eq!(s, StatusCode::NOT_FOUND);

        sqlx::query("CREATE TABLE affiliate_accounts (twitch_login TEXT PRIMARY KEY, commission_rate_pct SMALLINT NOT NULL DEFAULT 30, updated_at TEXT)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO affiliate_accounts (twitch_login) VALUES ('nani')")
            .execute(&pool)
            .await
            .unwrap();
        let (s, j) = body_json(
            set_commission_rate_handler(
                DashboardAuthLevel::admin(),
                State(pool.clone()),
                Path("nani".into()),
                valid_body,
            )
            .await,
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(j["commission_rate_pct"], 40);
        let stored: i16 = sqlx::query_scalar(
            "SELECT commission_rate_pct FROM affiliate_accounts WHERE twitch_login = 'nani'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(stored, 40);
    }

    #[tokio::test]
    async fn generate_auth_und_payload_fehler() {
        let Some(pool) = make_pool("t_affh_generate_auth").await else {
            return;
        };
        let (s, j) = body_json(
            generate_gutschriften_handler(
                DashboardAuthLevel::None,
                State(pool.clone()),
                Bytes::new(),
            )
            .await,
        )
        .await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
        assert_eq!(j["error"], "auth_required");

        let (s, j) = body_json(
            generate_gutschriften_handler(partner_auth(), State(pool.clone()), Bytes::new()).await,
        )
        .await;
        assert_eq!(s, StatusCode::FORBIDDEN);
        assert_eq!(j["error"], "admin_required");

        let (s, j) = body_json(
            generate_gutschriften_handler(
                DashboardAuthLevel::admin(),
                State(pool.clone()),
                Bytes::from_static(br#"{"affiliate_login":"!!!"}"#),
            )
            .await,
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        assert_eq!(j["error"], "invalid_login");

        let (s, j) = body_json(
            generate_gutschriften_handler(
                DashboardAuthLevel::admin(),
                State(pool),
                Bytes::from_static(br#"{"year":2026}"#),
            )
            .await,
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        assert_eq!(j["error"], "invalid_period");
    }

    #[tokio::test]
    async fn generate_trigger_nutzt_welle_c_monatsfunktion() {
        let Some(pool) = make_pool("t_affh_generate_trigger").await else {
            return;
        };
        sqlx::query("CREATE TABLE affiliate_commissions (id INTEGER GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY, affiliate_twitch_login TEXT, status TEXT, created_at TEXT)")
            .execute(&pool)
            .await
            .unwrap();
        let cipher = FieldCipher::from_hex_key(&"ab".repeat(32), "v1").unwrap();

        let results = run_gutschrift_job(&pool, &cipher, None, Some(2026), Some(6), false)
            .await
            .unwrap();
        assert!(results.is_empty());
    }
}
