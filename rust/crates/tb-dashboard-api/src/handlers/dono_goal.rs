use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Duration;

use axum::{
    extract::{Query, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::{PgPool, Row};
use tb_crypto::FieldCipher;

use crate::auth::level::DashboardAuthLevel;
use crate::auth::streamer_scope::resolve_settings_target;
use crate::handlers::overlay::{cached_or_compute, forget_cached, OverlayCache};

const SE_TIPS_BASE: &str = "https://api.streamelements.com/kappa/v2/tips";
const CACHE_TTL: Duration = Duration::from_secs(15);
const SE_TIMEOUT: Duration = Duration::from_secs(8);
const PAGE_LIMIT: usize = 100;
const MAX_PAGES: usize = 50;
const MAX_TITLE_CHARS: usize = 80;
const MAX_GOAL: f64 = 10_000_000.0;
const MAX_CHANNEL_ID_CHARS: usize = 64;
const MAX_JWT_CHARS: usize = 4096;
const DONO_GOAL_HTML: &str = include_str!("dono_goal.html");

static GOAL_CACHE: OnceLock<Mutex<OverlayCache>> = OnceLock::new();
static LAST_GOOD: OnceLock<Mutex<HashMap<String, TipSummary>>> = OnceLock::new();

#[derive(Deserialize, Default)]
pub struct DonoGoalQuery {
    #[serde(default)]
    pub streamer: Option<String>,
}

#[derive(Deserialize)]
pub struct DonoGoalUpdate {
    #[serde(default)]
    pub title: String,
    pub goal: f64,
    #[serde(default)]
    pub currency: String,
    #[serde(default)]
    pub se_channel_id: String,
    #[serde(default)]
    pub se_jwt: Option<String>,
    #[serde(default)]
    pub clear_jwt: bool,
}

#[derive(Debug, PartialEq)]
struct ValidUpdate {
    title: String,
    goal: f64,
    currency: String,
    channel_id: String,
    jwt: Option<String>,
    clear_jwt: bool,
}

struct GoalConfig {
    title: String,
    goal: f64,
    currency: String,
    count_from: DateTime<Utc>,
    channel_id: String,
    jwt_enc: Option<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq)]
struct LastTip {
    id: String,
    name: String,
    amount: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct TipSummary {
    total: f64,
    last: Option<LastTip>,
}

#[derive(Debug, PartialEq)]
enum SeError {
    Unauthorized,
    Unreachable,
    Decode,
}

impl SeError {
    fn code(&self) -> &'static str {
        match self {
            SeError::Unauthorized => "unauthorized",
            SeError::Unreachable | SeError::Decode => "unreachable",
        }
    }
}

#[derive(Deserialize)]
struct TipsPage {
    #[serde(default)]
    docs: Vec<SeTip>,
    #[serde(default)]
    total: Option<u64>,
}

#[derive(Deserialize)]
struct SeTip {
    #[serde(rename = "_id", default)]
    id: String,
    #[serde(default)]
    donation: Option<SeDonation>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    deleted: bool,
    #[serde(rename = "createdAt", default)]
    created_at: Option<String>,
}

#[derive(Deserialize)]
struct SeDonation {
    #[serde(default)]
    user: Option<SeUser>,
    #[serde(default)]
    amount: Option<Value>,
}

#[derive(Deserialize)]
struct SeUser {
    #[serde(default)]
    username: Option<String>,
}

fn goal_cache() -> &'static Mutex<OverlayCache> {
    GOAL_CACHE.get_or_init(|| Mutex::new(OverlayCache::default()))
}

fn last_good() -> MutexGuard<'static, HashMap<String, TipSummary>> {
    LAST_GOOD
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn invalidate(login: &str) {
    forget_cached(goal_cache(), login);
    last_good().remove(login);
}

fn jwt_aad(login: &str) -> String {
    format!("twitch_dono_goal:{login}:se_jwt")
}

fn round_cents(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn tip_amount(raw: &Value) -> Option<f64> {
    let amount = match raw {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse::<f64>().ok(),
        _ => None,
    }?;
    (amount.is_finite() && amount > 0.0).then_some(amount)
}

fn tip_counts(tip: &SeTip) -> bool {
    !tip.deleted
        && tip
            .status
            .as_deref()
            .is_none_or(|status| status == "success")
}

fn tip_created_at(tip: &SeTip) -> Option<DateTime<Utc>> {
    let raw = tip.created_at.as_deref()?;
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|value| value.with_timezone(&Utc))
}

fn summarize_tips(tips: &[SeTip], since: DateTime<Utc>) -> TipSummary {
    let mut total = 0.0;
    let mut latest: Option<(DateTime<Utc>, LastTip)> = None;
    for tip in tips.iter().filter(|tip| tip_counts(tip)) {
        let Some(created) = tip_created_at(tip).filter(|created| *created >= since) else {
            continue;
        };
        let Some(donation) = tip.donation.as_ref() else {
            continue;
        };
        let Some(amount) = donation.amount.as_ref().and_then(tip_amount) else {
            continue;
        };
        total += amount;
        if latest.as_ref().is_none_or(|(at, _)| created > *at) {
            let name = donation
                .user
                .as_ref()
                .and_then(|user| user.username.as_deref())
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .unwrap_or("Anonym")
                .to_string();
            latest = Some((
                created,
                LastTip {
                    id: tip.id.clone(),
                    name,
                    amount: round_cents(amount),
                },
            ));
        }
    }
    TipSummary {
        total: round_cents(total),
        last: latest.map(|(_, tip)| tip),
    }
}

fn page_is_last(page: &TipsPage, fetched: usize) -> bool {
    page.docs.len() < PAGE_LIMIT || page.total.is_some_and(|total| fetched as u64 >= total)
}

async fn fetch_page(
    client: &Client,
    channel_id: &str,
    jwt: &str,
    since: DateTime<Utc>,
    limit: usize,
    offset: usize,
) -> Result<TipsPage, SeError> {
    let url = format!("{SE_TIPS_BASE}/{channel_id}");
    let query = [
        ("limit", limit.to_string()),
        ("offset", offset.to_string()),
        ("sort", "-createdAt".to_string()),
        ("after", since.timestamp_millis().to_string()),
    ];
    let response = client
        .get(&url)
        .bearer_auth(jwt)
        .query(&query)
        .send()
        .await
        .map_err(|error| {
            tracing::warn!(error = %error.without_url(), channel_id, "dono goal SE request failed");
            SeError::Unreachable
        })?;
    let status = response.status();
    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        tracing::warn!(
            status = status.as_u16(),
            channel_id,
            "dono goal SE auth rejected"
        );
        return Err(SeError::Unauthorized);
    }
    if !status.is_success() {
        tracing::warn!(status = status.as_u16(), channel_id, "dono goal SE non-2xx");
        return Err(SeError::Unreachable);
    }
    response.json::<TipsPage>().await.map_err(|error| {
        tracing::warn!(error = %error.without_url(), channel_id, "dono goal SE decode failed");
        SeError::Decode
    })
}

async fn fetch_summary(
    client: &Client,
    channel_id: &str,
    jwt: &str,
    since: DateTime<Utc>,
) -> Result<TipSummary, SeError> {
    let mut tips = Vec::new();
    for page_index in 0..MAX_PAGES {
        let page = fetch_page(
            client,
            channel_id,
            jwt,
            since,
            PAGE_LIMIT,
            page_index * PAGE_LIMIT,
        )
        .await?;
        let done = page_is_last(&page, tips.len() + page.docs.len());
        tips.extend(page.docs);
        if done {
            break;
        }
    }
    Ok(summarize_tips(&tips, since))
}

fn se_client() -> Result<Client, SeError> {
    Client::builder()
        .timeout(SE_TIMEOUT)
        .build()
        .map_err(|error| {
            tracing::error!(%error, "dono goal reqwest client could not be built");
            SeError::Unreachable
        })
}

async fn load_config(pool: &PgPool, login: &str) -> Result<Option<GoalConfig>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT title, target_amount, currency, count_from, se_channel_id, se_jwt_enc \
           FROM twitch_dono_goal WHERE twitch_login = $1",
    )
    .bind(login)
    .fetch_optional(pool)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    Ok(Some(GoalConfig {
        title: row.try_get("title")?,
        goal: row.try_get("target_amount")?,
        currency: row.try_get("currency")?,
        count_from: row.try_get("count_from")?,
        channel_id: row.try_get("se_channel_id")?,
        jwt_enc: row.try_get("se_jwt_enc")?,
    }))
}

fn decrypt_jwt(login: &str, blob: &[u8]) -> Option<String> {
    let cipher = match FieldCipher::from_env() {
        Ok(cipher) => cipher,
        Err(error) => {
            tracing::error!(%error, "dono goal cipher unavailable");
            return None;
        }
    };
    match cipher.decrypt_field(blob, &jwt_aad(login)) {
        Ok(jwt) => Some(jwt),
        Err(error) => {
            tracing::error!(%error, login, "dono goal jwt decrypt failed");
            None
        }
    }
}

fn credentials(login: &str, config: &GoalConfig) -> Option<(String, String)> {
    if config.channel_id.is_empty() {
        return None;
    }
    let jwt = decrypt_jwt(login, config.jwt_enc.as_deref()?)?;
    Some((config.channel_id.clone(), jwt))
}

fn public_body(config: &GoalConfig, summary: &TipSummary) -> Value {
    let last = summary
        .last
        .as_ref()
        .map(|tip| json!({ "id": tip.id, "name": tip.name, "amount": tip.amount }));
    json!({
        "ok": true,
        "title": config.title,
        "goal": config.goal,
        "current": summary.total,
        "currency": config.currency,
        "last": last,
    })
}

async fn current_summary(login: &str, config: &GoalConfig) -> TipSummary {
    let Some((channel_id, jwt)) = credentials(login, config) else {
        return TipSummary::default();
    };
    let fetched = match se_client() {
        Ok(client) => fetch_summary(&client, &channel_id, &jwt, config.count_from).await,
        Err(error) => Err(error),
    };
    match fetched {
        Ok(summary) => {
            last_good().insert(login.to_string(), summary.clone());
            summary
        }
        Err(_) => last_good().get(login).cloned().unwrap_or_default(),
    }
}

async fn build_public_body(pool: &PgPool, login: &str) -> Value {
    match load_config(pool, login).await {
        Ok(Some(config)) => {
            let summary = current_summary(login, &config).await;
            public_body(&config, &summary)
        }
        Ok(None) => json!({ "ok": false }),
        Err(error) => {
            tracing::error!(%error, "dono goal config load failed");
            json!({ "ok": false })
        }
    }
}

fn normalize_login(streamer: Option<&str>) -> Option<String> {
    streamer
        .map(str::trim)
        .filter(|login| !login.is_empty() && login.len() <= 64)
        .map(str::to_lowercase)
}

pub async fn public_api_handler(
    State(pool): State<PgPool>,
    Query(query): Query<DonoGoalQuery>,
) -> Response {
    let Some(login) = normalize_login(query.streamer.as_deref()) else {
        return Json(json!({ "ok": false })).into_response();
    };
    let body = cached_or_compute(goal_cache(), login.clone(), CACHE_TTL, || {
        build_public_body(&pool, &login)
    })
    .await;
    ([(header::CACHE_CONTROL, "no-store")], Json(body)).into_response()
}

pub async fn html_handler() -> Response {
    ([(header::CACHE_CONTROL, "no-store")], Html(DONO_GOAL_HTML)).into_response()
}

fn error_response(status: StatusCode, code: &str) -> Response {
    (status, Json(json!({ "error": code }))).into_response()
}

fn db_error(error: sqlx::Error, action: &str) -> Response {
    tracing::error!(%error, action, "dono goal DB error");
    error_response(StatusCode::INTERNAL_SERVER_ERROR, "db")
}

fn settings_body(login: &str, config: Option<&GoalConfig>) -> Value {
    match config {
        Some(config) => json!({
            "login": login,
            "configured": true,
            "title": config.title,
            "goal": config.goal,
            "currency": config.currency,
            "count_from": config.count_from.to_rfc3339(),
            "se_channel_id": config.channel_id,
            "connected": config.jwt_enc.is_some() && !config.channel_id.is_empty(),
        }),
        None => json!({
            "login": login,
            "configured": false,
            "title": "",
            "goal": 100.0,
            "currency": "EUR",
            "count_from": Value::Null,
            "se_channel_id": "",
            "connected": false,
        }),
    }
}

pub async fn get_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<DonoGoalQuery>,
) -> Response {
    let (login, _) = match resolve_settings_target(&auth, &query.streamer) {
        Ok(target) => target,
        Err(response) => return response,
    };
    match load_config(&pool, &login).await {
        Ok(config) => Json(settings_body(&login, config.as_ref())).into_response(),
        Err(error) => db_error(error, "get"),
    }
}

fn valid_currency(currency: &str) -> bool {
    currency.len() == 3 && currency.chars().all(|c| c.is_ascii_uppercase())
}

fn valid_channel_id(channel_id: &str) -> bool {
    channel_id.len() <= MAX_CHANNEL_ID_CHARS
        && channel_id.chars().all(|c| c.is_ascii_alphanumeric())
}

fn clean_jwt(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let token = trimmed
        .strip_prefix("Bearer ")
        .or_else(|| trimmed.strip_prefix("bearer "))
        .unwrap_or(trimmed)
        .trim();
    (!token.is_empty()).then(|| token.to_string())
}

fn validate_update(update: &DonoGoalUpdate) -> Result<ValidUpdate, &'static str> {
    let title = update.title.trim().to_string();
    if title.chars().count() > MAX_TITLE_CHARS {
        return Err("title_too_long");
    }
    if !update.goal.is_finite() || update.goal <= 0.0 || update.goal > MAX_GOAL {
        return Err("invalid_goal");
    }
    let currency = update.currency.trim().to_ascii_uppercase();
    if !valid_currency(&currency) {
        return Err("invalid_currency");
    }
    let channel_id = update.se_channel_id.trim().to_string();
    if !valid_channel_id(&channel_id) {
        return Err("invalid_channel_id");
    }
    let jwt = update.se_jwt.as_deref().and_then(clean_jwt);
    if jwt.as_ref().is_some_and(|jwt| {
        jwt.len() > MAX_JWT_CHARS || jwt.chars().any(|c| c.is_whitespace() || c.is_control())
    }) {
        return Err("invalid_jwt");
    }
    Ok(ValidUpdate {
        title,
        goal: round_cents(update.goal),
        currency,
        channel_id,
        jwt,
        clear_jwt: update.clear_jwt,
    })
}

fn encrypt_jwt(login: &str, jwt: &str) -> Result<Vec<u8>, (StatusCode, &'static str)> {
    let cipher = FieldCipher::from_env().map_err(|error| {
        tracing::error!(%error, "dono goal cipher unavailable");
        (StatusCode::SERVICE_UNAVAILABLE, "crypto_unavailable")
    })?;
    cipher.encrypt_field(jwt, &jwt_aad(login)).map_err(|error| {
        tracing::error!(%error, "dono goal jwt encrypt failed");
        (StatusCode::INTERNAL_SERVER_ERROR, "crypto")
    })
}

const UPSERT_SQL: &str = "INSERT INTO twitch_dono_goal \
       (twitch_login, twitch_user_id, title, target_amount, currency, se_channel_id, se_jwt_enc) \
     VALUES ($1, $2, $3, $4, $5, $6, $7) \
     ON CONFLICT (twitch_login) DO UPDATE SET \
       twitch_user_id = COALESCE(NULLIF(EXCLUDED.twitch_user_id, ''), twitch_dono_goal.twitch_user_id), \
       title = EXCLUDED.title, \
       target_amount = EXCLUDED.target_amount, \
       currency = EXCLUDED.currency, \
       se_channel_id = EXCLUDED.se_channel_id, \
       se_jwt_enc = CASE WHEN $8 THEN NULL \
                         ELSE COALESCE(EXCLUDED.se_jwt_enc, twitch_dono_goal.se_jwt_enc) END, \
       updated_at = now()";

pub async fn post_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<DonoGoalQuery>,
    Json(body): Json<DonoGoalUpdate>,
) -> Response {
    let (login, user_id) = match resolve_settings_target(&auth, &query.streamer) {
        Ok(target) => target,
        Err(response) => return response,
    };
    let update = match validate_update(&body) {
        Ok(update) => update,
        Err(code) => return error_response(StatusCode::BAD_REQUEST, code),
    };
    let jwt_enc = match update.jwt.as_deref() {
        Some(jwt) if !update.clear_jwt => match encrypt_jwt(&login, jwt) {
            Ok(blob) => Some(blob),
            Err((status, code)) => return error_response(status, code),
        },
        _ => None,
    };
    let result = sqlx::query(UPSERT_SQL)
        .bind(&login)
        .bind(&user_id)
        .bind(&update.title)
        .bind(update.goal)
        .bind(&update.currency)
        .bind(&update.channel_id)
        .bind(jwt_enc)
        .bind(update.clear_jwt)
        .execute(&pool)
        .await;
    if let Err(error) = result {
        return db_error(error, "save");
    }
    invalidate(&login);
    match load_config(&pool, &login).await {
        Ok(config) => Json(settings_body(&login, config.as_ref())).into_response(),
        Err(error) => db_error(error, "reload"),
    }
}

pub async fn reset_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<DonoGoalQuery>,
) -> Response {
    let (login, _) = match resolve_settings_target(&auth, &query.streamer) {
        Ok(target) => target,
        Err(response) => return response,
    };
    let result = sqlx::query(
        "UPDATE twitch_dono_goal SET count_from = now(), updated_at = now() WHERE twitch_login = $1",
    )
    .bind(&login)
    .execute(&pool)
    .await;
    match result {
        Ok(done) if done.rows_affected() > 0 => {
            invalidate(&login);
            match load_config(&pool, &login).await {
                Ok(config) => Json(settings_body(&login, config.as_ref())).into_response(),
                Err(error) => db_error(error, "reload"),
            }
        }
        Ok(_) => error_response(StatusCode::NOT_FOUND, "not_configured"),
        Err(error) => db_error(error, "reset"),
    }
}

pub async fn test_handler(
    auth: DashboardAuthLevel,
    State(pool): State<PgPool>,
    Query(query): Query<DonoGoalQuery>,
) -> Response {
    let (login, _) = match resolve_settings_target(&auth, &query.streamer) {
        Ok(target) => target,
        Err(response) => return response,
    };
    let config = match load_config(&pool, &login).await {
        Ok(config) => config,
        Err(error) => return db_error(error, "test"),
    };
    let Some((channel_id, jwt)) = config
        .as_ref()
        .and_then(|config| credentials(&login, config))
    else {
        return Json(json!({ "ok": false, "error": "not_configured" })).into_response();
    };
    let checked = match se_client() {
        Ok(client) => fetch_page(&client, &channel_id, &jwt, DateTime::UNIX_EPOCH, 1, 0)
            .await
            .map(|_| ()),
        Err(error) => Err(error),
    };
    match checked {
        Ok(()) => Json(json!({ "ok": true })).into_response(),
        Err(error) => Json(json!({ "ok": false, "error": error.code() })).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    const SE_FIXTURE: &str = r#"{
      "docs": [
        {
          "_id": "65f0c0de0000000000000003",
          "channel": "5b2e2007760aeb3b1c7a3f4d",
          "donation": {
            "user": { "username": "HazeMain", "geo": null, "email": "haze@example.com" },
            "message": "Viel Erfolg beim Turnier!",
            "amount": 12.5,
            "currency": "EUR"
          },
          "provider": "paypal",
          "status": "success",
          "deleted": false,
          "createdAt": "2026-10-16T19:30:00.000Z",
          "approved": "allowed",
          "updatedAt": "2026-10-16T19:30:01.000Z"
        },
        {
          "_id": "65f0c0de0000000000000002",
          "channel": "5b2e2007760aeb3b1c7a3f4d",
          "donation": {
            "user": { "username": "Lurker42" },
            "message": "geheime Nachricht",
            "amount": "5",
            "currency": "EUR"
          },
          "provider": "paypal",
          "status": "success",
          "deleted": false,
          "createdAt": "2026-10-16T18:00:00.000Z",
          "approved": "allowed",
          "updatedAt": "2026-10-16T18:00:00.000Z"
        },
        {
          "_id": "65f0c0de0000000000000004",
          "channel": "5b2e2007760aeb3b1c7a3f4d",
          "donation": { "user": { "username": "Refund" }, "amount": 50, "currency": "EUR" },
          "provider": "paypal",
          "status": "refunded",
          "deleted": false,
          "createdAt": "2026-10-16T19:45:00.000Z"
        },
        {
          "_id": "65f0c0de0000000000000005",
          "channel": "5b2e2007760aeb3b1c7a3f4d",
          "donation": { "user": { "username": "Geloescht" }, "amount": 20, "currency": "EUR" },
          "provider": "paypal",
          "status": "success",
          "deleted": true,
          "createdAt": "2026-10-16T19:50:00.000Z"
        },
        {
          "_id": "65f0c0de0000000000000001",
          "channel": "5b2e2007760aeb3b1c7a3f4d",
          "donation": { "user": { "username": "Alt" }, "amount": 100, "currency": "EUR" },
          "provider": "paypal",
          "status": "success",
          "deleted": false,
          "createdAt": "2026-10-15T10:00:00.000Z"
        }
      ],
      "total": 5,
      "limit": 100,
      "offset": 0
    }"#;

    fn fixture() -> TipsPage {
        serde_json::from_str(SE_FIXTURE).expect("fixture parses")
    }

    fn since() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 16, 0, 0, 0)
            .single()
            .expect("valid date")
    }

    #[test]
    fn summarize_counts_only_successful_tips_after_count_from() {
        let summary = summarize_tips(&fixture().docs, since());
        assert_eq!(summary.total, 17.5);
        assert_eq!(
            summary.last,
            Some(LastTip {
                id: "65f0c0de0000000000000003".into(),
                name: "HazeMain".into(),
                amount: 12.5,
            })
        );
    }

    #[test]
    fn summarize_empty_or_all_old_yields_zero() {
        let later = Utc
            .with_ymd_and_hms(2026, 10, 17, 0, 0, 0)
            .single()
            .expect("valid date");
        assert_eq!(
            summarize_tips(&fixture().docs, later),
            TipSummary::default()
        );
        assert_eq!(summarize_tips(&[], since()), TipSummary::default());
    }

    #[test]
    fn public_body_never_contains_message_or_secrets() {
        let config = GoalConfig {
            title: "Neues Setup".into(),
            goal: 300.0,
            currency: "EUR".into(),
            count_from: since(),
            channel_id: "abc".into(),
            jwt_enc: Some(vec![1, 2, 3]),
        };
        let body = public_body(&config, &summarize_tips(&fixture().docs, since()));
        let text = body.to_string();
        assert_eq!(body["current"], 17.5);
        assert_eq!(body["last"]["name"], "HazeMain");
        assert!(!text.contains("Viel Erfolg"));
        assert!(!text.contains("email"));
        assert!(!text.contains("jwt"));
        assert!(!text.contains("abc"));
    }

    #[test]
    fn page_is_last_respects_total_and_short_pages() {
        let page = fixture();
        assert!(page_is_last(&page, 5));
        let full = TipsPage {
            docs: (0..PAGE_LIMIT)
                .map(|_| serde_json::from_str::<SeTip>("{}").expect("empty tip"))
                .collect(),
            total: Some(250),
        };
        assert!(!page_is_last(&full, 100));
        assert!(page_is_last(&full, 300));
    }

    fn update(goal: f64, currency: &str, channel: &str, jwt: Option<&str>) -> DonoGoalUpdate {
        DonoGoalUpdate {
            title: "  Neues Setup  ".into(),
            goal,
            currency: currency.into(),
            se_channel_id: channel.into(),
            se_jwt: jwt.map(str::to_string),
            clear_jwt: false,
        }
    }

    #[test]
    fn validate_update_normalizes_and_rejects_bad_input() {
        let ok = validate_update(&update(
            300.0,
            "eur",
            "5b2e2007",
            Some("Bearer abc.def.ghi"),
        ))
        .expect("valid");
        assert_eq!(ok.title, "Neues Setup");
        assert_eq!(ok.currency, "EUR");
        assert_eq!(ok.jwt.as_deref(), Some("abc.def.ghi"));
        assert_eq!(
            validate_update(&update(0.0, "EUR", "x", None)),
            Err("invalid_goal")
        );
        assert_eq!(
            validate_update(&update(10.0, "EURO", "x", None)),
            Err("invalid_currency")
        );
        assert_eq!(
            validate_update(&update(10.0, "EUR", "../x", None)),
            Err("invalid_channel_id")
        );
        assert_eq!(
            validate_update(&update(10.0, "EUR", "x", Some("a b"))),
            Err("invalid_jwt")
        );
        assert_eq!(
            validate_update(&update(10.0, "EUR", "x", Some("   "))).map(|u| u.jwt),
            Ok(None)
        );
    }

    #[test]
    fn jwt_roundtrip_uses_login_bound_aad() {
        let cipher = FieldCipher::from_hex_key(
            "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
            "v1",
        )
        .expect("cipher");
        let blob = cipher
            .encrypt_field("abc.def.ghi", &jwt_aad("nani"))
            .expect("encrypt");
        assert_eq!(
            cipher
                .decrypt_field(&blob, &jwt_aad("nani"))
                .expect("decrypt"),
            "abc.def.ghi"
        );
        assert!(cipher.decrypt_field(&blob, &jwt_aad("other")).is_err());
    }
}
