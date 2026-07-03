//! Affiliate-Onboarding: Twitch-OAuth, Affiliate-Session, Stripe-Connect und
//! Profil-PII. Port von `bot/dashboard/affiliate/affiliate_mixin.py` Welle A.

use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::{Extension, Path, Query, State},
    http::{
        header::{CONTENT_DISPOSITION, CONTENT_TYPE, SET_COOKIE},
        HeaderMap, HeaderValue, StatusCode,
    },
    response::{IntoResponse, Redirect, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::{PgPool, Row};
use tb_analytics::{
    affiliate_claim_window::{sql_reservation_fresh_predicate, POST_ACTIVATION_GRACE},
    affiliate_commission::connect_account_and_replay,
    affiliate_gutschrift::{self, GutschriftError},
    affiliate_pii::{
        build_readiness, is_valid_ust_status, load_affiliate_pii, migrate_from_plaintext,
        save_affiliate_pii, PiiInput, PiiPayload,
    },
    stripe::StripeClient,
};
use tb_crypto::FieldCipher;

use crate::auth::{
    oauth_login::{TwitchIdentity, TwitchOAuthClient, TWITCH_AUTHORIZE_URL},
    session::{
        build_session_cookie, AffiliateConnectState, AffiliateOAuthState, AffiliateSession,
        DashboardAuthState, SameSite, AFFILIATE_COOKIE_NAME, AFFILIATE_SESSION_TTL_SECS,
    },
};

const DEFAULT_PUBLIC_ORIGIN: &str = "https://deutsche-deadlock-community.de";
const SHARED_TWITCH_CALLBACK_PATH: &str = "/callback/twitch";
const AFFILIATE_STRIPE_CALLBACK_PATH: &str = "/twitch/affiliate/connect/stripe/callback";
const STRIPE_CONNECT_AUTHORIZE_URL: &str = "https://connect.stripe.com/oauth/authorize";

/// Laufzeit-Konfiguration des Affiliate-Twitch-OAuth-Flows.
#[derive(Clone)]
pub struct AffiliateOAuthConfig {
    pub client_id: String,
    pub cookie_secure: bool,
    pub client: Arc<dyn TwitchOAuthClient>,
}

/// Laufzeit-Konfiguration für Stripe-Connect. `client` ist nur für den Callback
/// nötig; der Start-Redirect braucht lediglich die öffentliche Connect-Client-ID.
#[derive(Clone, Default)]
pub struct AffiliateStripeConfig {
    pub connect_client_id: Option<String>,
    pub client: Option<StripeClient>,
}

#[derive(Debug, Default, Deserialize)]
pub struct CallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
struct AffiliateAccount {
    twitch_login: String,
    display_name: String,
    stripe_account_id: String,
    stripe_connect_status: String,
    is_active: i32,
    created_at: String,
    updated_at: String,
}

pub fn affiliate_oauth_config_from_env() -> Option<AffiliateOAuthConfig> {
    let client_id = non_empty_env(&["TWITCH_CLIENT_ID"])?;
    let client_secret = non_empty_env(&["TWITCH_CLIENT_SECRET"])?;
    let cookie_secure = std::env::var("TB_DASHBOARD_COOKIE_INSECURE").as_deref() != Ok("1");
    let client =
        crate::auth::oauth_login::HelixOAuthClient::new(&client_id, &client_secret).ok()?;
    Some(AffiliateOAuthConfig {
        client_id,
        cookie_secure,
        client: Arc::new(client),
    })
}

pub fn affiliate_stripe_config_from_env() -> Option<AffiliateStripeConfig> {
    let connect_client_id = non_empty_env(&["STRIPE_CONNECT_CLIENT_ID"]);
    let client = non_empty_env(&["STRIPE_SECRET_KEY", "TWITCH_BILLING_STRIPE_SECRET_KEY"])
        .and_then(|secret| StripeClient::new(secret).ok());
    if connect_client_id.is_none() && client.is_none() {
        return None;
    }
    Some(AffiliateStripeConfig {
        connect_client_id,
        client,
    })
}

/// Gemeinsamer Affiliate-Session-Lookup für Affiliate-Handler und Portal-API.
pub async fn affiliate_session_from_headers(
    state: Option<&DashboardAuthState>,
    headers: &HeaderMap,
) -> Option<AffiliateSession> {
    let state = state?;
    let session_id = cookie_from_headers(headers, AFFILIATE_COOKIE_NAME)?;
    if session_id.trim().is_empty() {
        return None;
    }
    state
        .load_affiliate_session(&session_id)
        .await
        .ok()
        .flatten()
}

/// `GET /twitch/auth/affiliate/login`.
pub async fn auth_login_handler(
    state: Option<Extension<DashboardAuthState>>,
    config: Option<Extension<AffiliateOAuthConfig>>,
    headers: HeaderMap,
) -> Response {
    let Some(Extension(state)) = state else {
        return text(
            StatusCode::SERVICE_UNAVAILABLE,
            "OAuth ist nicht konfiguriert.",
        );
    };
    let Some(config) = affiliate_oauth_config(config) else {
        return text(
            StatusCode::SERVICE_UNAVAILABLE,
            "OAuth ist nicht konfiguriert.",
        );
    };
    if affiliate_session_from_headers(Some(&state), &headers)
        .await
        .is_some()
    {
        return Redirect::to("/twitch/affiliate/portal").into_response();
    }

    let redirect_uri = affiliate_auth_redirect_uri();
    let state_token = tb_crypto::random_urlsafe_token(24);
    let oauth_state = AffiliateOAuthState {
        redirect_uri: redirect_uri.clone(),
    };
    if let Err(error) = state
        .save_affiliate_oauth_state(&state_token, &oauth_state)
        .await
    {
        tracing::warn!(%error, "Affiliate-OAuth-State konnte nicht persistiert werden");
        return text(
            StatusCode::SERVICE_UNAVAILABLE,
            "OAuth-Status konnte nicht sicher gespeichert werden. Bitte erneut versuchen.",
        );
    }

    let url = build_affiliate_authorize_url(&config.client_id, &redirect_uri, &state_token);
    Redirect::to(&url).into_response()
}

/// `GET /twitch/auth/affiliate/callback`.
pub async fn auth_callback_handler(
    state: Option<Extension<DashboardAuthState>>,
    config: Option<Extension<AffiliateOAuthConfig>>,
    headers: HeaderMap,
    Query(query): Query<CallbackQuery>,
) -> Response {
    let Some(Extension(state)) = state else {
        return text(
            StatusCode::SERVICE_UNAVAILABLE,
            "OAuth ist nicht konfiguriert.",
        );
    };
    let Some(config) = affiliate_oauth_config(config) else {
        return text(
            StatusCode::SERVICE_UNAVAILABLE,
            "OAuth ist nicht konfiguriert.",
        );
    };

    let error = query.error.as_deref().map(str::trim).unwrap_or("");
    if !error.is_empty() {
        return text(StatusCode::UNAUTHORIZED, &format!("OAuth-Fehler: {error}"));
    }

    let state_token = query.state.as_deref().map(str::trim).unwrap_or("");
    let code = query.code.as_deref().map(str::trim).unwrap_or("");
    if state_token.is_empty() || code.is_empty() {
        return text(StatusCode::BAD_REQUEST, "Fehlender OAuth state/code.");
    }

    let oauth_state = match state.consume_affiliate_oauth_state(state_token).await {
        Ok(Some(state)) => state,
        Ok(None) => {
            return text(
                StatusCode::BAD_REQUEST,
                "OAuth state ungueltig oder abgelaufen.",
            )
        }
        Err(error) => {
            tracing::warn!(%error, "Affiliate-OAuth-State-Lookup fehlgeschlagen");
            return text(
                StatusCode::BAD_REQUEST,
                "OAuth state ungueltig oder abgelaufen.",
            );
        }
    };

    complete_affiliate_login(
        &state,
        config.client.as_ref(),
        oauth_state,
        cookie_secure(&headers, Some(&config)),
        code,
    )
    .await
}

pub(crate) async fn complete_affiliate_login(
    state: &DashboardAuthState,
    client: &dyn TwitchOAuthClient,
    oauth_state: AffiliateOAuthState,
    cookie_secure: bool,
    code: &str,
) -> Response {
    let identity = match client
        .exchange_code_for_identity(code, &oauth_state.redirect_uri)
        .await
    {
        Ok(identity) => identity,
        Err(error) => {
            tracing::warn!(?error, "Affiliate-OAuth-Austausch fehlgeschlagen");
            return text(StatusCode::UNAUTHORIZED, "OAuth-Austausch fehlgeschlagen.");
        }
    };

    let session = match state
        .create_affiliate_session(
            &identity.twitch_login,
            &identity.twitch_user_id,
            &identity.display_name,
            &identity.email,
        )
        .await
    {
        Ok(session) => session,
        Err(error) => {
            tracing::warn!(%error, "Affiliate-Session-Erstellung fehlgeschlagen");
            return text(
                StatusCode::SERVICE_UNAVAILABLE,
                "OAuth-Status konnte nicht sicher gespeichert werden. Bitte erneut versuchen.",
            );
        }
    };

    if let Err(error) = upsert_account_and_pii(state, &identity).await {
        tracing::warn!(?error, "Affiliate-Konto/PII-Upsert fehlgeschlagen");
        return text(
            StatusCode::SERVICE_UNAVAILABLE,
            "OAuth-Status konnte nicht sicher gespeichert werden. Bitte erneut versuchen.",
        );
    }

    let cookie = build_session_cookie(
        AFFILIATE_COOKIE_NAME,
        &session.session_id,
        cookie_secure,
        SameSite::Lax,
        AFFILIATE_SESSION_TTL_SECS,
    );
    redirect_with_cookie("/twitch/affiliate/portal", &cookie)
}

pub(crate) async fn try_shared_affiliate_callback(
    state: &DashboardAuthState,
    client: &dyn TwitchOAuthClient,
    cookie_secure: bool,
    code: &str,
    state_token: &str,
    error: &str,
) -> Option<Response> {
    let oauth_state = match state.consume_affiliate_oauth_state(state_token).await {
        Ok(Some(state)) => state,
        Ok(None) => return None,
        Err(error) => {
            tracing::warn!(%error, "Affiliate-OAuth-State-Lookup fehlgeschlagen");
            return None;
        }
    };
    if !error.is_empty() {
        return Some(text(
            StatusCode::UNAUTHORIZED,
            &format!("OAuth-Fehler: {error}"),
        ));
    }
    if code.is_empty() {
        return Some(text(StatusCode::BAD_REQUEST, "Fehlender OAuth state/code."));
    }
    Some(complete_affiliate_login(state, client, oauth_state, cookie_secure, code).await)
}

/// `GET /twitch/affiliate/connect/stripe`.
pub async fn connect_stripe_handler(
    state: Option<Extension<DashboardAuthState>>,
    config: Option<Extension<AffiliateStripeConfig>>,
    headers: HeaderMap,
) -> Response {
    let Some(Extension(state)) = state else {
        return Redirect::to("/twitch/auth/affiliate/login").into_response();
    };
    let Some(session) = affiliate_session_from_headers(Some(&state), &headers).await else {
        return Redirect::to("/twitch/auth/affiliate/login").into_response();
    };
    let Some(stripe_config) = affiliate_stripe_config(config) else {
        return text(
            StatusCode::SERVICE_UNAVAILABLE,
            "Stripe Connect ist nicht konfiguriert.",
        );
    };
    let Some(client_id) = stripe_config
        .connect_client_id
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    else {
        return text(
            StatusCode::SERVICE_UNAVAILABLE,
            "Stripe Connect ist nicht konfiguriert.",
        );
    };

    let state_token = tb_crypto::random_urlsafe_token(24);
    let redirect_uri = affiliate_stripe_redirect_uri();
    let connect_state = AffiliateConnectState {
        redirect_uri: redirect_uri.clone(),
        twitch_login: session.twitch_login.clone(),
    };
    if let Err(error) = state
        .save_affiliate_connect_state(&state_token, &connect_state)
        .await
    {
        tracing::warn!(%error, "Affiliate-Connect-State konnte nicht persistiert werden");
        return text(
            StatusCode::SERVICE_UNAVAILABLE,
            "State konnte nicht sicher gespeichert werden. Bitte erneut versuchen.",
        );
    }

    let url = build_stripe_connect_authorize_url(client_id, &redirect_uri, &state_token);
    Redirect::to(&url).into_response()
}

/// `GET /twitch/affiliate/connect/stripe/callback`.
pub async fn connect_stripe_callback_handler(
    state: Option<Extension<DashboardAuthState>>,
    config: Option<Extension<AffiliateStripeConfig>>,
    headers: HeaderMap,
    Query(query): Query<CallbackQuery>,
) -> Response {
    let Some(Extension(state)) = state else {
        return Redirect::to("/twitch/auth/affiliate/login").into_response();
    };
    let Some(session) = affiliate_session_from_headers(Some(&state), &headers).await else {
        return Redirect::to("/twitch/auth/affiliate/login").into_response();
    };
    let state_token = query.state.as_deref().map(str::trim).unwrap_or("");
    let code = query.code.as_deref().map(str::trim).unwrap_or("");
    if state_token.is_empty() || code.is_empty() {
        return text(StatusCode::BAD_REQUEST, "Fehlender state/code.");
    }

    let connect_state = match state.consume_affiliate_connect_state(state_token).await {
        Ok(Some(state)) => state,
        Ok(None) => return text(StatusCode::BAD_REQUEST, "State ungueltig oder abgelaufen."),
        Err(error) => {
            tracing::warn!(%error, "Affiliate-Connect-State-Lookup fehlgeschlagen");
            return text(StatusCode::BAD_REQUEST, "State ungueltig oder abgelaufen.");
        }
    };
    let session_login = session.twitch_login.trim().to_lowercase();
    if connect_state.twitch_login.trim().to_lowercase() != session_login {
        return text(
            StatusCode::FORBIDDEN,
            "Affiliate-Session passt nicht zum Stripe Connect state.",
        );
    }

    let Some(stripe_config) = affiliate_stripe_config(config) else {
        return text(
            StatusCode::SERVICE_UNAVAILABLE,
            "Stripe ist nicht konfiguriert.",
        );
    };
    let Some(client) = stripe_config.client.as_ref() else {
        return text(
            StatusCode::SERVICE_UNAVAILABLE,
            "Stripe ist nicht konfiguriert.",
        );
    };

    let value = match client
        .exchange_connect_oauth_code(code, &connect_state.redirect_uri)
        .await
    {
        Ok(value) => value,
        Err(error) => {
            tracing::warn!(?error, "Stripe Connect token exchange failed");
            return text(StatusCode::BAD_GATEWAY, "Stripe Connect fehlgeschlagen.");
        }
    };
    let stripe_user_id = value
        .get("stripe_user_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if stripe_user_id.is_empty() {
        return text(StatusCode::BAD_GATEWAY, "Keine Stripe Account ID erhalten.");
    }

    if let Err(error) =
        connect_account_and_replay(state.pool(), Some(client), &session_login, stripe_user_id).await
    {
        tracing::error!(%error, "Affiliate Stripe Connect DB-Update fehlgeschlagen");
        return text(StatusCode::BAD_GATEWAY, "Stripe Connect fehlgeschlagen.");
    }
    Redirect::to("/twitch/affiliate/portal").into_response()
}

/// `POST /twitch/affiliate/claim`.
pub async fn claim_handler(
    state: Option<Extension<DashboardAuthState>>,
    headers: HeaderMap,
    State(pool): State<PgPool>,
    body: Bytes,
) -> Response {
    let Some(session) =
        affiliate_session_from_headers(state.as_ref().map(|s| &s.0), &headers).await
    else {
        return json_error(StatusCode::UNAUTHORIZED, "unauthorized");
    };
    let body: Value = match serde_json::from_slice(&body) {
        Ok(body) => body,
        Err(_) => return json_error(StatusCode::BAD_REQUEST, "invalid_json"),
    };
    let Some(obj) = body.as_object() else {
        return json_error(StatusCode::BAD_REQUEST, "invalid_payload");
    };
    let streamer_login = obj
        .get("streamer_login")
        .map(value_to_string)
        .unwrap_or_default()
        .trim()
        .to_lowercase();
    if !is_valid_twitch_login(&streamer_login) {
        return json_error(StatusCode::BAD_REQUEST, "invalid_login");
    }
    let twitch_login = session.twitch_login.trim().to_lowercase();
    // POLICY: Claims sind Reservierungen. Nicht-Partner dürfen vorab reserviert
    // werden, aktive Partner nur innerhalb der Nachfrist; frische oder bereits
    // konvertierte Claims blockieren, abgelaufene Nicht-Partner-Reservierungen
    // sind überschreibbar.
    match claim_streamer(&pool, &twitch_login, &streamer_login).await {
        Ok(ClaimStatus::Ok) => {
            Json(json!({ "ok": true, "claimed": streamer_login })).into_response()
        }
        Ok(ClaimStatus::StreamerAlreadyRegistered) => {
            json_error(StatusCode::CONFLICT, "streamer_already_registered")
        }
        Ok(ClaimStatus::AlreadyClaimed) => json_error(StatusCode::CONFLICT, "already_claimed"),
        Err(error) => {
            tracing::error!(%error, "Affiliate-Claim fehlgeschlagen");
            json_error(StatusCode::INTERNAL_SERVER_ERROR, "db")
        }
    }
}

/// `GET /twitch/api/affiliate/me`.
pub async fn api_me_handler(
    state: Option<Extension<DashboardAuthState>>,
    cipher: Option<Extension<Arc<FieldCipher>>>,
    headers: HeaderMap,
    State(pool): State<PgPool>,
) -> Response {
    let Some(session) =
        affiliate_session_from_headers(state.as_ref().map(|s| &s.0), &headers).await
    else {
        return json_error(StatusCode::UNAUTHORIZED, "unauthorized");
    };
    let cipher = match resolve_cipher(cipher) {
        Ok(cipher) => cipher,
        Err(error) => {
            tracing::error!(?error, "Affiliate-PII-Chiffre nicht verfügbar");
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "db");
        }
    };
    if let Err(error) = migrate_legacy_plaintext_pii(&pool, cipher.as_ref()).await {
        tracing::error!(?error, "Affiliate-PII-Legacy-Migration fehlgeschlagen");
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, "db");
    }
    match load_profile(&pool, &cipher, &session.twitch_login).await {
        Ok(Some((account, pii))) => {
            let readiness = build_readiness(&pii);
            Json(profile_payload(&account, &pii, readiness)).into_response()
        }
        Ok(None) => json_error(StatusCode::NOT_FOUND, "not_found"),
        Err(error) => {
            tracing::error!(?error, "Affiliate-Profil-Lookup fehlgeschlagen");
            json_error(StatusCode::INTERNAL_SERVER_ERROR, "db")
        }
    }
}

/// `PUT /twitch/api/affiliate/profile`.
pub async fn api_profile_update_handler(
    state: Option<Extension<DashboardAuthState>>,
    cipher: Option<Extension<Arc<FieldCipher>>>,
    headers: HeaderMap,
    State(pool): State<PgPool>,
    body: Bytes,
) -> Response {
    let Some(session) =
        affiliate_session_from_headers(state.as_ref().map(|s| &s.0), &headers).await
    else {
        return json_error(StatusCode::UNAUTHORIZED, "unauthorized");
    };
    let body: Value = match serde_json::from_slice(&body) {
        Ok(body) => body,
        Err(_) => return json_error(StatusCode::BAD_REQUEST, "invalid_json"),
    };
    let Some(obj) = body.as_object() else {
        return json_error(StatusCode::BAD_REQUEST, "invalid_payload");
    };
    let ust_status = obj
        .get("ust_status")
        .map(value_to_string)
        .unwrap_or_default()
        .trim()
        .to_lowercase();
    if !ust_status.is_empty() && !is_valid_ust_status(&ust_status) {
        return json_error(StatusCode::BAD_REQUEST, "invalid_ust_status");
    }
    let cipher = match resolve_cipher(cipher) {
        Ok(cipher) => cipher,
        Err(error) => {
            tracing::error!(?error, "Affiliate-PII-Chiffre nicht verfügbar");
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "db");
        }
    };
    if let Err(error) = migrate_legacy_plaintext_pii(&pool, cipher.as_ref()).await {
        tracing::error!(?error, "Affiliate-PII-Legacy-Migration fehlgeschlagen");
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, "db");
    }

    let login = session.twitch_login.trim().to_lowercase();
    let account_exists = match load_account(&pool, &login).await {
        Ok(Some(_)) => true,
        Ok(None) => false,
        Err(error) => {
            tracing::error!(%error, "Affiliate-Konto-Lookup fehlgeschlagen");
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "db");
        }
    };
    if !account_exists {
        return json_error(StatusCode::NOT_FOUND, "not_found");
    }

    let input = PiiInput {
        full_name: Some(
            obj.get("full_name")
                .map(value_to_string)
                .unwrap_or_default(),
        ),
        email: Some(obj.get("email").map(value_to_string).unwrap_or_default()),
        address_line1: Some(
            obj.get("address_line1")
                .map(value_to_string)
                .unwrap_or_default(),
        ),
        address_city: Some(
            obj.get("address_city")
                .map(value_to_string)
                .unwrap_or_default(),
        ),
        address_zip: Some(
            obj.get("address_zip")
                .map(value_to_string)
                .unwrap_or_default(),
        ),
        address_country: Some(
            obj.get("address_country")
                .map(value_to_string)
                .unwrap_or_default(),
        ),
        tax_id: Some(obj.get("tax_id").map(value_to_string).unwrap_or_default()),
        vat_id: Some(obj.get("vat_id").map(value_to_string).unwrap_or_default()),
        ust_status: Some(if ust_status.is_empty() {
            "unknown".to_string()
        } else {
            ust_status
        }),
    };

    if let Err(error) = save_affiliate_pii(&pool, &cipher, &login, &input).await {
        tracing::error!(?error, "Affiliate-PII-Speichern fehlgeschlagen");
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, "db");
    }
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, false);
    if let Err(error) =
        sqlx::query("UPDATE affiliate_accounts SET updated_at = $1 WHERE twitch_login = $2")
            .bind(&now)
            .bind(&login)
            .execute(&pool)
            .await
    {
        tracing::error!(%error, "Affiliate-Konto-updated_at fehlgeschlagen");
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, "db");
    }

    match load_profile(&pool, &cipher, &login).await {
        Ok(Some((account, pii))) => {
            let readiness = build_readiness(&pii);
            Json(json!({
                "ok": true,
                "profile": profile_payload(&account, &pii, readiness),
            }))
            .into_response()
        }
        Ok(None) => json_error(StatusCode::NOT_FOUND, "not_found"),
        Err(error) => {
            tracing::error!(?error, "Affiliate-Profil-Reload fehlgeschlagen");
            json_error(StatusCode::INTERNAL_SERVER_ERROR, "db")
        }
    }
}

/// `GET /twitch/api/affiliate/claims`.
pub async fn api_claims_handler(
    state: Option<Extension<DashboardAuthState>>,
    headers: HeaderMap,
    State(pool): State<PgPool>,
) -> Response {
    let Some(session) =
        affiliate_session_from_headers(state.as_ref().map(|s| &s.0), &headers).await
    else {
        return json_error(StatusCode::UNAUTHORIZED, "unauthorized");
    };
    match load_claims(&pool, session.twitch_login.trim()).await {
        Ok(claims) => Json(json!({ "claims": claims })).into_response(),
        Err(error) => {
            tracing::error!(%error, "Affiliate-Claims-Lookup fehlgeschlagen");
            json_error(StatusCode::INTERNAL_SERVER_ERROR, "db")
        }
    }
}

/// `GET /twitch/api/affiliate/gutschriften`.
pub async fn api_gutschriften_handler(
    state: Option<Extension<DashboardAuthState>>,
    cipher: Option<Extension<Arc<FieldCipher>>>,
    headers: HeaderMap,
    State(pool): State<PgPool>,
) -> Response {
    let Some(session) =
        affiliate_session_from_headers(state.as_ref().map(|s| &s.0), &headers).await
    else {
        return json_error(StatusCode::UNAUTHORIZED, "unauthorized");
    };
    let login = session.twitch_login.trim().to_lowercase();
    let cipher = match resolve_cipher(cipher) {
        Ok(cipher) => cipher,
        Err(error) => {
            tracing::error!(%error, "Affiliate-Gutschriften: FieldCipher fehlt");
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "db");
        }
    };
    let (account, pii) = match load_profile(&pool, &cipher, &login).await {
        Ok(Some(profile)) => profile,
        Ok(None) => return json_error(StatusCode::NOT_FOUND, "not_found"),
        Err(error) => {
            tracing::error!(?error, "Affiliate-Gutschriften-Profil fehlgeschlagen");
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "db");
        }
    };
    let documents = match affiliate_gutschrift::list_for_affiliate(&pool, &login).await {
        Ok(documents) => documents,
        Err(GutschriftError::InvalidLogin) => {
            return json_error(StatusCode::NOT_FOUND, "not_found");
        }
        Err(error) => {
            tracing::error!(%error, "Affiliate-Gutschriften-Liste fehlgeschlagen");
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "db");
        }
    };
    let readiness = build_readiness(&pii);
    Json(json!({
        "gutschriften": documents,
        "readiness": readiness.clone(),
        "profile": profile_payload(&account, &pii, readiness),
    }))
    .into_response()
}

/// `GET /twitch/api/affiliate/gutschriften/:gutschrift_id/pdf`.
pub async fn api_gutschrift_pdf_handler(
    state: Option<Extension<DashboardAuthState>>,
    headers: HeaderMap,
    State(pool): State<PgPool>,
    Path(gutschrift_id): Path<String>,
) -> Response {
    let Some(session) =
        affiliate_session_from_headers(state.as_ref().map(|s| &s.0), &headers).await
    else {
        return json_error(StatusCode::UNAUTHORIZED, "unauthorized");
    };
    let id = match gutschrift_id.trim().parse::<i64>() {
        Ok(id) if id > 0 => id,
        _ => return json_error(StatusCode::BAD_REQUEST, "invalid_gutschrift_id"),
    };
    let login = session.twitch_login.trim().to_lowercase();
    match affiliate_gutschrift::get_pdf(&pool, &login, id).await {
        Ok(Some((metadata, bytes))) => {
            let raw_name = metadata.gutschrift_number.trim();
            let filename = if raw_name.is_empty() {
                format!("gutschrift-{id}")
            } else {
                raw_name.replace('"', "")
            };
            let mut response = (StatusCode::OK, bytes).into_response();
            response
                .headers_mut()
                .insert(CONTENT_TYPE, HeaderValue::from_static("application/pdf"));
            let disposition = format!("inline; filename=\"{filename}.pdf\"");
            if let Ok(value) = HeaderValue::from_str(&disposition) {
                response.headers_mut().insert(CONTENT_DISPOSITION, value);
            }
            response
        }
        Ok(None) => json_error(StatusCode::NOT_FOUND, "not_found"),
        Err(GutschriftError::InvalidLogin) => json_error(StatusCode::NOT_FOUND, "not_found"),
        Err(error) => {
            tracing::error!(%error, "Affiliate-Gutschrift-PDF fehlgeschlagen");
            json_error(StatusCode::INTERNAL_SERVER_ERROR, "db")
        }
    }
}

async fn upsert_account_and_pii(
    state: &DashboardAuthState,
    identity: &TwitchIdentity,
) -> Result<(), AffiliatePersistError> {
    let pool = state.pool();
    let login = identity.twitch_login.trim().to_lowercase();
    if login.is_empty() {
        return Ok(());
    }
    let exists = load_account(pool, &login).await?.is_some();
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, false);
    if !exists {
        sqlx::query(
            r#"
            INSERT INTO affiliate_accounts
                (twitch_login, twitch_user_id, display_name, email, full_name,
                 address_line1, address_city, address_zip, address_country,
                 created_at, updated_at)
            VALUES ($1, $2, $3, '', '', '', '', '', 'DE', $4, $5)
            ON CONFLICT (twitch_login) DO NOTHING
            "#,
        )
        .bind(&login)
        .bind(identity.twitch_user_id.trim())
        .bind(display_or_login(identity))
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await?;
    }
    if !identity.email.trim().is_empty() {
        let cipher = FieldCipher::from_env().map_err(AffiliatePersistError::Crypto)?;
        let input = PiiInput {
            email: Some(identity.email.trim().to_string()),
            ..PiiInput::default()
        };
        save_affiliate_pii(pool, &cipher, &login, &input).await?;
    }
    Ok(())
}

async fn load_profile(
    pool: &PgPool,
    cipher: &FieldCipher,
    login: &str,
) -> Result<Option<(AffiliateAccount, PiiPayload)>, AffiliatePersistError> {
    let Some(account) = load_account(pool, login).await? else {
        return Ok(None);
    };
    let pii = load_affiliate_pii(pool, cipher, login).await?;
    Ok(Some((account, pii)))
}

async fn migrate_legacy_plaintext_pii(
    pool: &PgPool,
    cipher: &FieldCipher,
) -> Result<(), AffiliatePersistError> {
    let migrated = migrate_from_plaintext(pool, cipher).await?;
    if migrated > 0 {
        tracing::info!(migrated, "Affiliate-PII-Legacy-Klartext migriert");
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClaimStatus {
    Ok,
    StreamerAlreadyRegistered,
    AlreadyClaimed,
}

async fn claim_streamer(
    pool: &PgPool,
    twitch_login: &str,
    streamer_login: &str,
) -> Result<ClaimStatus, sqlx::Error> {
    claim_streamer_at(pool, twitch_login, streamer_login, chrono::Utc::now()).await
}

async fn claim_streamer_at(
    pool: &PgPool,
    twitch_login: &str,
    streamer_login: &str,
    now_dt: DateTime<Utc>,
) -> Result<ClaimStatus, sqlx::Error> {
    let twitch_login = twitch_login.trim().to_lowercase();
    let streamer_login = streamer_login.trim().to_lowercase();
    let now = now_dt.to_rfc3339_opts(chrono::SecondsFormat::Micros, false);

    let mut tx = pool.begin().await?;
    sqlx::query(
        r#"
        SELECT 1
        FROM (SELECT pg_advisory_xact_lock(hashtext(LOWER($1))::bigint)) AS claim_lock
        "#,
    )
    .bind(&streamer_login)
    .fetch_one(&mut *tx)
    .await?;

    let partner_state: Option<(i32, Option<String>)> = sqlx::query_as(
        r#"
        SELECT COALESCE(is_partner_active, 0) AS is_partner_active, created_at
        FROM twitch_streamers_partner_state
        WHERE LOWER(twitch_login) = LOWER($1)
        ORDER BY COALESCE(is_partner_active, 0) DESC
        LIMIT 1
        "#,
    )
    .bind(&streamer_login)
    .fetch_optional(&mut *tx)
    .await?;
    let partner_active = partner_state
        .as_ref()
        .is_some_and(|(is_active, _)| *is_active != 0);
    let partnered_at = partner_state
        .as_ref()
        .and_then(|(_, created_at)| created_at.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);

    let fresh_predicate = sql_reservation_fresh_predicate("$1", "claimed_at");
    let existing_sql = format!(
        r#"
        SELECT affiliate_twitch_login,
               claimed_at,
               {fresh_predicate} AS reservation_fresh
        FROM affiliate_streamer_claims
        WHERE LOWER(claimed_streamer_login) = LOWER($2)
        LIMIT 1
        FOR UPDATE
        "#
    );
    let existing_claim: Option<(String, String, bool)> = sqlx::query_as(&existing_sql)
        .bind(&now)
        .bind(&streamer_login)
        .fetch_optional(&mut *tx)
        .await?;
    if let Some((_existing_affiliate, _claimed_at, reservation_fresh)) = existing_claim {
        if partner_active || reservation_fresh {
            tx.commit().await?;
            return Ok(ClaimStatus::AlreadyClaimed);
        }
        sqlx::query(
            r#"
            DELETE FROM affiliate_streamer_claims
            WHERE LOWER(claimed_streamer_login) = LOWER($1)
            "#,
        )
        .bind(&streamer_login)
        .execute(&mut *tx)
        .await?;
    }

    if partner_active {
        let Some(partnered_at) = partnered_at else {
            tracing::warn!(streamer_login = %streamer_login, "Aktiver Partner ohne partnered_at im Affiliate-Claim-Gate");
            tx.commit().await?;
            return Ok(ClaimStatus::StreamerAlreadyRegistered);
        };
        let partnered_at = match parse_rfc3339_utc(&partnered_at) {
            Ok(value) => value,
            Err(error) => {
                tracing::warn!(%error, streamer_login = %streamer_login, "Aktiver Partner mit ungueltigem partnered_at im Affiliate-Claim-Gate");
                tx.commit().await?;
                return Ok(ClaimStatus::StreamerAlreadyRegistered);
            }
        };
        let in_grace =
            now_dt <= partnered_at + chrono::Duration::seconds(POST_ACTIVATION_GRACE.seconds());
        if !in_grace {
            tx.commit().await?;
            return Ok(ClaimStatus::StreamerAlreadyRegistered);
        }
    }

    let insert = sqlx::query(
        r#"
        INSERT INTO affiliate_streamer_claims
            (affiliate_twitch_login, claimed_streamer_login, claimed_at)
        VALUES ($1, $2, $3)
        "#,
    )
    .bind(&twitch_login)
    .bind(&streamer_login)
    .bind(&now)
    .execute(&mut *tx)
    .await;
    match insert {
        Ok(_) => {
            tx.commit().await?;
            Ok(ClaimStatus::Ok)
        }
        Err(error) if is_duplicate_sqlx_error(&error) => {
            if let Err(rollback_error) = tx.rollback().await {
                tracing::warn!(%rollback_error, "Affiliate-Claim-Transaktion nach Unique-Race konnte nicht sauber zurueckrollen");
            }
            Ok(ClaimStatus::AlreadyClaimed)
        }
        Err(error) => Err(error),
    }
}

async fn load_claims(pool: &PgPool, twitch_login: &str) -> Result<Vec<Value>, sqlx::Error> {
    type ClaimRow = (Option<String>, Option<String>, i64, i64);
    let rows = sqlx::query_as::<_, ClaimRow>(
        r#"
        SELECT c.claimed_streamer_login,
               c.claimed_at,
               COUNT(co.id)::bigint AS commission_count,
               COALESCE(SUM(co.commission_cents), 0)::bigint AS total_commission_cents
        FROM affiliate_streamer_claims c
        LEFT JOIN affiliate_commissions co
          ON co.affiliate_twitch_login = c.affiliate_twitch_login
         AND co.streamer_login = c.claimed_streamer_login
        WHERE c.affiliate_twitch_login = $1
        GROUP BY c.claimed_streamer_login, c.claimed_at
        "#,
    )
    .bind(twitch_login)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(
            |(streamer_login, claimed_at, commission_count, total_commission_cents)| {
                json!({
                    "streamer_login": streamer_login.unwrap_or_default(),
                    "claimed_at": claimed_at,
                    "commission_count": commission_count,
                    "total_commission_cents": total_commission_cents,
                })
            },
        )
        .collect())
}

async fn load_account(pool: &PgPool, login: &str) -> Result<Option<AffiliateAccount>, sqlx::Error> {
    let row = sqlx::query(
        r#"
        SELECT twitch_login, display_name, stripe_account_id, stripe_connect_status,
               is_active, created_at, updated_at
        FROM affiliate_accounts
        WHERE twitch_login = $1
        "#,
    )
    .bind(login)
    .fetch_optional(pool)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    Ok(Some(AffiliateAccount {
        twitch_login: row.try_get::<String, _>("twitch_login")?,
        display_name: row
            .try_get::<Option<String>, _>("display_name")?
            .unwrap_or_default(),
        stripe_account_id: row
            .try_get::<Option<String>, _>("stripe_account_id")?
            .unwrap_or_default(),
        stripe_connect_status: row
            .try_get::<Option<String>, _>("stripe_connect_status")?
            .unwrap_or_default(),
        is_active: row.try_get::<i32, _>("is_active")?,
        created_at: row.try_get::<String, _>("created_at")?,
        updated_at: row.try_get::<String, _>("updated_at")?,
    }))
}

fn profile_payload(account: &AffiliateAccount, pii: &PiiPayload, readiness: Value) -> Value {
    let stripe_id = account.stripe_account_id.trim();
    let masked = if stripe_id.len() > 12 {
        format!(
            "{}...{}",
            &stripe_id[..8],
            &stripe_id[stripe_id.len() - 4..]
        )
    } else {
        stripe_id.to_string()
    };
    json!({
        "twitch_login": account.twitch_login.as_str(),
        "display_name": account.display_name.as_str(),
        "email": pii.email.as_str(),
        "full_name": pii.full_name.as_str(),
        "address_line1": pii.address_line1.as_str(),
        "address_city": pii.address_city.as_str(),
        "address_zip": pii.address_zip.as_str(),
        "address_country": pii.address_country.as_str(),
        "tax_id": pii.tax_id.as_str(),
        "vat_id": pii.vat_id.as_str(),
        "ust_status": if pii.ust_status.trim().is_empty() { "unknown" } else { pii.ust_status.as_str() },
        "stripe_connect_status": account.stripe_connect_status.as_str(),
        "stripe_account_id": masked,
        "is_active": account.is_active != 0,
        "created_at": account.created_at.as_str(),
        "updated_at": account.updated_at.as_str(),
        "profile_updated_at": pii.updated_at.as_deref(),
        "gutschrift_readiness": readiness,
    })
}

fn affiliate_oauth_config(
    config: Option<Extension<AffiliateOAuthConfig>>,
) -> Option<AffiliateOAuthConfig> {
    config.map(|c| c.0).or_else(affiliate_oauth_config_from_env)
}

fn affiliate_stripe_config(
    config: Option<Extension<AffiliateStripeConfig>>,
) -> Option<AffiliateStripeConfig> {
    config
        .map(|c| c.0)
        .or_else(affiliate_stripe_config_from_env)
}

fn resolve_cipher(
    cipher: Option<Extension<Arc<FieldCipher>>>,
) -> Result<Arc<FieldCipher>, tb_error::CryptoError> {
    if let Some(Extension(cipher)) = cipher {
        return Ok(cipher);
    }
    FieldCipher::from_env().map(Arc::new)
}

fn build_affiliate_authorize_url(client_id: &str, redirect_uri: &str, state: &str) -> String {
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("response_type", "code")
        .append_pair("scope", "user:read:email")
        .append_pair("state", state)
        .finish();
    format!("{TWITCH_AUTHORIZE_URL}?{query}")
}

fn build_stripe_connect_authorize_url(client_id: &str, redirect_uri: &str, state: &str) -> String {
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("response_type", "code")
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("scope", "read_write")
        .append_pair("state", state)
        .finish();
    format!("{STRIPE_CONNECT_AUTHORIZE_URL}?{query}")
}

fn affiliate_auth_redirect_uri() -> String {
    if let Some(uri) = non_empty_env(&["TWITCH_AFFILIATE_AUTH_REDIRECT_URI"]) {
        return uri;
    }
    format!("{}{}", public_origin(), SHARED_TWITCH_CALLBACK_PATH)
}

fn affiliate_stripe_redirect_uri() -> String {
    format!("{}{}", public_origin(), AFFILIATE_STRIPE_CALLBACK_PATH)
}

fn public_origin() -> String {
    non_empty_env(&[
        "TWITCH_PUBLIC_DASHBOARD_BASE_URL",
        "TWITCH_PUBLIC_URL",
        "PUBLIC_URL",
    ])
    .and_then(|value| origin_from_urlish(&value))
    .unwrap_or_else(|| DEFAULT_PUBLIC_ORIGIN.to_string())
}

fn origin_from_urlish(raw: &str) -> Option<String> {
    let raw = raw.trim().trim_end_matches('/');
    if raw.is_empty() {
        return None;
    }
    let with_scheme = if raw.contains("://") {
        raw.to_string()
    } else {
        format!("https://{raw}")
    };
    let url = url::Url::parse(&with_scheme).ok()?;
    let scheme = match url.scheme() {
        "http" if url.host_str().is_some_and(is_loopback_host) => "http",
        "https" => "https",
        _ => "https",
    };
    let host = url.host_str()?;
    let host = match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    };
    Some(format!("{scheme}://{}", sanitize_host(&host)?))
}

fn header_first(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToOwned::to_owned)
}

fn sanitize_host(raw: &str) -> Option<String> {
    let host = raw.trim().trim_matches('"').trim();
    if host.is_empty() || host.contains('/') || host.contains('@') {
        return None;
    }
    if host
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '[' | ']'))
    {
        Some(host.to_string())
    } else {
        None
    }
}

fn is_loopback_host(host: &str) -> bool {
    let host = host
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(':')
        .next()
        .unwrap_or("")
        .to_lowercase();
    matches!(host.as_str(), "localhost" | "127.0.0.1" | "::1")
}

fn cookie_secure(headers: &HeaderMap, config: Option<&AffiliateOAuthConfig>) -> bool {
    if let Some(config) = config {
        return config.cookie_secure;
    }
    if std::env::var("TB_DASHBOARD_COOKIE_INSECURE").as_deref() == Ok("1") {
        return false;
    }
    header_first(headers, "x-forwarded-proto")
        .map(|p| p.eq_ignore_ascii_case("https"))
        .unwrap_or(true)
}

fn redirect_with_cookie(location: &str, cookie: &str) -> Response {
    let mut response = Redirect::to(location).into_response();
    if let Ok(value) = HeaderValue::from_str(cookie) {
        response.headers_mut().append(SET_COOKIE, value);
    }
    response
}

fn cookie_from_headers(headers: &HeaderMap, name: &str) -> Option<String> {
    let raw = headers.get(axum::http::header::COOKIE)?.to_str().ok()?;
    for pair in raw.split(';') {
        let pair = pair.trim();
        if let Some((k, v)) = pair.split_once('=') {
            if k.trim() == name {
                return Some(v.trim().to_string());
            }
        }
    }
    None
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(value) => value.trim().to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        other => other.to_string(),
    }
}

fn display_or_login(identity: &TwitchIdentity) -> &str {
    let display = identity.display_name.trim();
    if display.is_empty() {
        identity.twitch_login.trim()
    } else {
        display
    }
}

fn text(status: StatusCode, body: &str) -> Response {
    (status, body.to_string()).into_response()
}

fn json_error(status: StatusCode, code: &str) -> Response {
    (status, Json(json!({ "error": code }))).into_response()
}

fn non_empty_env(keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        std::env::var(key)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

fn is_valid_twitch_login(value: &str) -> bool {
    let len = value.len();
    (3..=25).contains(&len)
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

fn is_duplicate_sqlx_error(error: &sqlx::Error) -> bool {
    let msg = error.to_string().to_lowercase();
    msg.contains("unique") || msg.contains("duplicate")
}

fn parse_rfc3339_utc(value: &str) -> Result<DateTime<Utc>, chrono::ParseError> {
    DateTime::parse_from_rfc3339(value).map(|parsed| parsed.with_timezone(&Utc))
}

#[derive(Debug, thiserror::Error)]
enum AffiliatePersistError {
    #[error("db")]
    Db(#[from] sqlx::Error),
    #[error("pii")]
    Pii,
    #[error("crypto")]
    Crypto(#[from] tb_error::CryptoError),
}

impl From<tb_analytics::affiliate_pii::PiiError> for AffiliatePersistError {
    fn from(_: tb_analytics::affiliate_pii::PiiError) -> Self {
        Self::Pii
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use axum::body::{to_bytes, Body};
    use axum::http::header::{COOKIE, LOCATION, SET_COOKIE};
    use axum::http::Request;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;
    use std::sync::Mutex;
    use tb_transport_twitch::user_token::UserTokenError;
    use tower::ServiceExt;
    use wiremock::matchers::{body_string_contains, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const TEST_FERNET_KEY: &str = "dGVzdGtleTEyMzQ1Njc4OTAxMjM0NTY3ODkwMTIzNDU=";
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, previous }
        }

        fn remove(key: &'static str) -> Self {
            let previous = std::env::var(key).ok();
            std::env::remove_var(key);
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    #[derive(Clone)]
    struct FakeOAuth {
        identity: TwitchIdentity,
    }

    #[async_trait]
    impl TwitchOAuthClient for FakeOAuth {
        async fn exchange_code_for_identity(
            &self,
            _code: &str,
            _redirect_uri: &str,
        ) -> Result<TwitchIdentity, UserTokenError> {
            Ok(self.identity.clone())
        }
    }

    fn test_cipher() -> FieldCipher {
        FieldCipher::from_hex_key(&"ab".repeat(32), "v1").unwrap()
    }

    fn oauth_config(identity: TwitchIdentity) -> AffiliateOAuthConfig {
        AffiliateOAuthConfig {
            client_id: "cid".to_string(),
            cookie_secure: false,
            client: Arc::new(FakeOAuth { identity }),
        }
    }

    async fn pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .ok()?;
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .ok()?;
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .ok()?;
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn)
            .ok()?
            .options([("search_path", schema)]);
        PgPoolOptions::new()
            .max_connections(2)
            .connect_with(opts)
            .await
            .ok()
    }

    async fn create_tables(pool: &PgPool) {
        sqlx::query(
            r#"
            CREATE TABLE dashboard_sessions (
                session_id TEXT PRIMARY KEY,
                session_type TEXT NOT NULL,
                payload_enc BYTEA NOT NULL,
                created_at DOUBLE PRECISION NOT NULL,
                expires_at DOUBLE PRECISION NOT NULL
            )
            "#,
        )
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            r#"
            CREATE TABLE affiliate_accounts (
                twitch_login TEXT PRIMARY KEY,
                twitch_user_id TEXT NOT NULL,
                display_name TEXT,
                email TEXT NOT NULL,
                full_name TEXT NOT NULL,
                address_line1 TEXT NOT NULL,
                address_city TEXT NOT NULL,
                address_zip TEXT NOT NULL,
                address_country TEXT NOT NULL DEFAULT 'DE',
                stripe_account_id TEXT,
                stripe_connected_at TEXT,
                stripe_connect_status TEXT DEFAULT 'pending',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                is_active INTEGER NOT NULL DEFAULT 1
            )
            "#,
        )
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            r#"
            CREATE TABLE affiliate_pii (
                twitch_login TEXT PRIMARY KEY REFERENCES affiliate_accounts(twitch_login),
                full_name_enc BYTEA,
                email_enc BYTEA,
                address_line1_enc BYTEA,
                address_city_enc BYTEA,
                address_zip_enc BYTEA,
                tax_id_enc BYTEA,
                address_country TEXT NOT NULL DEFAULT 'DE',
                ust_status TEXT NOT NULL DEFAULT 'unknown',
                updated_at TEXT NOT NULL
            )
            "#,
        )
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            r#"
            CREATE TABLE twitch_streamers_partner_state (
                twitch_login TEXT,
                is_partner_active INTEGER,
                created_at TEXT
            )
            "#,
        )
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            r#"
            CREATE TABLE affiliate_streamer_claims (
                id INTEGER GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
                affiliate_twitch_login TEXT NOT NULL,
                claimed_streamer_login TEXT NOT NULL UNIQUE,
                claimed_at TEXT NOT NULL
            )
            "#,
        )
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            r#"
            CREATE TABLE affiliate_commissions (
                id INTEGER GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
                affiliate_twitch_login TEXT NOT NULL,
                streamer_login TEXT NOT NULL,
                stripe_event_id TEXT UNIQUE NOT NULL,
                stripe_invoice_id TEXT,
                stripe_customer_id TEXT,
                stripe_transfer_id TEXT,
                brutto_cents INTEGER NOT NULL,
                commission_cents INTEGER NOT NULL,
                currency TEXT NOT NULL DEFAULT 'eur',
                status TEXT NOT NULL DEFAULT 'pending',
                period_start TEXT,
                period_end TEXT,
                created_at TEXT NOT NULL,
                transferred_at TEXT,
                error_message TEXT
            )
            "#,
        )
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            r#"
            CREATE TABLE affiliate_gutschriften (
                id INTEGER GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY,
                gutschrift_number TEXT UNIQUE NOT NULL,
                affiliate_twitch_login TEXT NOT NULL,
                period_year INTEGER NOT NULL,
                period_month INTEGER NOT NULL,
                net_amount_cents INTEGER NOT NULL,
                vat_rate_percent NUMERIC(5,2) NOT NULL DEFAULT 0,
                vat_amount_cents INTEGER NOT NULL DEFAULT 0,
                gross_amount_cents INTEGER NOT NULL,
                affiliate_name TEXT NOT NULL DEFAULT '',
                affiliate_address TEXT NOT NULL DEFAULT '',
                affiliate_tax_id TEXT,
                affiliate_ust_status TEXT NOT NULL DEFAULT 'unknown',
                issuer_name TEXT NOT NULL DEFAULT '',
                issuer_address TEXT NOT NULL DEFAULT '',
                issuer_tax_id TEXT NOT NULL DEFAULT '',
                pdf_blob BYTEA,
                pdf_generated_at TEXT,
                email_sent_at TEXT,
                email_error TEXT,
                commission_ids TEXT,
                created_at TEXT NOT NULL,
                UNIQUE (affiliate_twitch_login, period_year, period_month)
            )
            "#,
        )
        .execute(pool)
        .await
        .unwrap();
    }

    fn state(pool: PgPool) -> DashboardAuthState {
        DashboardAuthState::new(pool, TEST_FERNET_KEY.to_string())
    }

    fn ts_offset(offset: chrono::Duration) -> String {
        (chrono::Utc::now() + offset).to_rfc3339_opts(chrono::SecondsFormat::Micros, false)
    }

    async fn insert_partner_state(
        pool: &PgPool,
        login: &str,
        is_partner_active: i32,
        created_at: Option<&str>,
    ) {
        sqlx::query(
            r#"
            INSERT INTO twitch_streamers_partner_state (twitch_login, is_partner_active, created_at)
            VALUES ($1, $2, $3)
            "#,
        )
        .bind(login)
        .bind(is_partner_active)
        .bind(created_at)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn insert_claim(pool: &PgPool, affiliate: &str, streamer: &str, claimed_at: &str) {
        sqlx::query(
            r#"
            INSERT INTO affiliate_streamer_claims
                (affiliate_twitch_login, claimed_streamer_login, claimed_at)
            VALUES ($1, $2, $3)
            "#,
        )
        .bind(affiliate)
        .bind(streamer)
        .bind(claimed_at)
        .execute(pool)
        .await
        .unwrap();
    }

    #[test]
    fn authorize_url_enthaelt_email_scope() {
        let url = build_affiliate_authorize_url(
            "cid",
            "https://example.test/twitch/auth/affiliate/callback",
            "state-123",
        );
        assert!(url.starts_with(TWITCH_AUTHORIZE_URL));
        assert!(url.contains("client_id=cid"));
        assert!(url.contains("scope=user%3Aread%3Aemail"));
        assert!(url.contains("state=state-123"));
    }

    #[test]
    fn redirect_uri_nutzt_secret_und_public_url_env() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let _auth = EnvGuard::set(
            "TWITCH_AFFILIATE_AUTH_REDIRECT_URI",
            "https://auth.example.test/custom/callback",
        );
        let _public = EnvGuard::set(
            "TWITCH_PUBLIC_DASHBOARD_BASE_URL",
            "https://public.example.test",
        );
        let _legacy_public = EnvGuard::remove("TWITCH_PUBLIC_URL");
        let _generic_public = EnvGuard::remove("PUBLIC_URL");

        assert_eq!(
            affiliate_auth_redirect_uri(),
            "https://auth.example.test/custom/callback"
        );
        assert_eq!(
            affiliate_stripe_redirect_uri(),
            "https://public.example.test/twitch/affiliate/connect/stripe/callback"
        );
    }

    #[test]
    fn redirect_uri_fallback_ignoriert_request_host() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let _auth = EnvGuard::remove("TWITCH_AFFILIATE_AUTH_REDIRECT_URI");
        let _public_dashboard = EnvGuard::remove("TWITCH_PUBLIC_DASHBOARD_BASE_URL");
        let _legacy_public = EnvGuard::remove("TWITCH_PUBLIC_URL");
        let _generic_public = EnvGuard::remove("PUBLIC_URL");

        assert_eq!(
            affiliate_auth_redirect_uri(),
            "https://deutsche-deadlock-community.de/callback/twitch"
        );
        assert!(affiliate_auth_redirect_uri().ends_with("/callback/twitch"));
        assert_eq!(
            affiliate_stripe_redirect_uri(),
            "https://deutsche-deadlock-community.de/twitch/affiliate/connect/stripe/callback"
        );
    }

    #[tokio::test]
    async fn callback_legt_affiliate_session_account_und_email_pii_an() {
        let Some(pool) = pool("t_affiliate_callback").await else {
            return;
        };
        create_tables(&pool).await;
        std::env::set_var("DB_MASTER_KEY_V1", "ab".repeat(32));
        let state = state(pool.clone());
        state
            .save_affiliate_oauth_state(
                "state-123",
                &AffiliateOAuthState {
                    redirect_uri: "https://example.test/twitch/auth/affiliate/callback".into(),
                },
            )
            .await
            .unwrap();
        let identity = TwitchIdentity {
            twitch_login: "Partner_One".into(),
            twitch_user_id: "1001".into(),
            display_name: "Partner One".into(),
            email: "partner@example.test".into(),
        };

        let response = auth_callback_handler(
            Some(Extension(state.clone())),
            Some(Extension(oauth_config(identity))),
            HeaderMap::new(),
            Query(CallbackQuery {
                code: Some("oauth-code".into()),
                state: Some("state-123".into()),
                error: None,
            }),
        )
        .await;
        assert!(response.status().is_redirection());
        assert_eq!(
            response.headers().get(LOCATION).unwrap(),
            "/twitch/affiliate/portal"
        );
        let set_cookie = response
            .headers()
            .get(SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(set_cookie.starts_with("twitch_affiliate_session="));

        let row = sqlx::query(
            "SELECT email, full_name, address_line1, address_city, address_zip, address_country FROM affiliate_accounts WHERE twitch_login = 'partner_one'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        for column in [
            "email",
            "full_name",
            "address_line1",
            "address_city",
            "address_zip",
        ] {
            assert_eq!(row.try_get::<String, _>(column).unwrap(), "");
        }
        assert_eq!(row.try_get::<String, _>("address_country").unwrap(), "DE");
        let pii = load_affiliate_pii(&pool, &test_cipher(), "partner_one")
            .await
            .unwrap();
        assert_eq!(pii.email, "partner@example.test");
    }

    #[tokio::test]
    async fn connect_redirect_speichert_state_mit_redirect_uri() {
        let Some(pool) = pool("t_affiliate_connect_start").await else {
            return;
        };
        create_tables(&pool).await;
        let state = state(pool.clone());
        let session = state
            .create_affiliate_session("partner_one", "1001", "Partner One", "")
            .await
            .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            COOKIE,
            format!("{}={}", AFFILIATE_COOKIE_NAME, session.session_id)
                .parse()
                .unwrap(),
        );
        headers.insert("host", "attacker.test".parse().unwrap());

        let response = connect_stripe_handler(
            Some(Extension(state.clone())),
            Some(Extension(AffiliateStripeConfig {
                connect_client_id: Some("ca_code_123".into()),
                client: None,
            })),
            headers,
        )
        .await;
        assert!(response.status().is_redirection());
        let location = response.headers().get(LOCATION).unwrap().to_str().unwrap();
        assert!(location.starts_with("https://connect.stripe.com/oauth/authorize?"));
        let parsed = url::Url::parse(location).unwrap();
        let params: std::collections::HashMap<_, _> = parsed.query_pairs().collect();
        assert_eq!(
            params.get("client_id").map(|v| v.as_ref()),
            Some("ca_code_123")
        );
        let redirect_uri = params.get("redirect_uri").map(|v| v.as_ref()).unwrap();
        let expected_redirect_uri = affiliate_stripe_redirect_uri();
        assert_eq!(redirect_uri, expected_redirect_uri.as_str());
        assert!(!redirect_uri.contains("attacker.test"));
        let state_token = params.get("state").unwrap().to_string();
        let saved = state
            .consume_affiliate_connect_state(&state_token)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(saved.twitch_login, "partner_one");
        assert_eq!(saved.redirect_uri, expected_redirect_uri);
    }

    #[tokio::test]
    async fn connect_callback_nutzt_stripe_user_id() {
        let Some(pool) = pool("t_affiliate_connect_callback").await else {
            return;
        };
        create_tables(&pool).await;
        sqlx::query(
            r#"
            INSERT INTO affiliate_accounts
                (twitch_login, twitch_user_id, display_name, email, full_name, address_line1,
                 address_city, address_zip, address_country, created_at, updated_at)
            VALUES ('partner_one', '1001', 'Partner One', '', '', '', '', '', 'DE',
                    '2026-01-01T00:00:00+00:00', '2026-01-01T00:00:00+00:00')
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        let state = state(pool.clone());
        let session = state
            .create_affiliate_session("partner_one", "1001", "Partner One", "")
            .await
            .unwrap();
        let redirect_uri =
            "https://example.test/twitch/affiliate/connect/stripe/callback".to_string();
        state
            .save_affiliate_connect_state(
                "connect-state",
                &AffiliateConnectState {
                    redirect_uri: redirect_uri.clone(),
                    twitch_login: "partner_one".into(),
                },
            )
            .await
            .unwrap();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth/token"))
            .and(body_string_contains("grant_type=authorization_code"))
            .and(body_string_contains("code=oauth-code"))
            .and(body_string_contains(
                "redirect_uri=https%3A%2F%2Fexample.test%2Ftwitch%2Faffiliate%2Fconnect%2Fstripe%2Fcallback",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "stripe_user_id": "acct_123"
            })))
            .mount(&server)
            .await;
        let stripe_client = StripeClient::new("sk_test_secret")
            .unwrap()
            .with_connect_base(server.uri());
        let mut headers = HeaderMap::new();
        headers.insert(
            COOKIE,
            format!("{}={}", AFFILIATE_COOKIE_NAME, session.session_id)
                .parse()
                .unwrap(),
        );

        let response = connect_stripe_callback_handler(
            Some(Extension(state)),
            Some(Extension(AffiliateStripeConfig {
                connect_client_id: None,
                client: Some(stripe_client),
            })),
            headers,
            Query(CallbackQuery {
                code: Some("oauth-code".into()),
                state: Some("connect-state".into()),
                error: None,
            }),
        )
        .await;
        assert!(response.status().is_redirection());
        let row = sqlx::query(
            "SELECT stripe_account_id, stripe_connect_status FROM affiliate_accounts WHERE twitch_login = 'partner_one'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            row.try_get::<Option<String>, _>("stripe_account_id")
                .unwrap()
                .unwrap(),
            "acct_123"
        );
        assert_eq!(
            row.try_get::<Option<String>, _>("stripe_connect_status")
                .unwrap()
                .unwrap(),
            "connected"
        );
    }

    #[tokio::test]
    async fn connect_callback_replayt_pending_commission() {
        let Some(pool) = pool("t_affiliate_connect_replay").await else {
            return;
        };
        create_tables(&pool).await;
        sqlx::query(
            r#"
            INSERT INTO affiliate_accounts
                (twitch_login, twitch_user_id, display_name, email, full_name, address_line1,
                 address_city, address_zip, address_country, created_at, updated_at)
            VALUES ('partner_one', '1001', 'Partner One', '', '', '', '', '', 'DE',
                    '2026-01-01T00:00:00+00:00', '2026-01-01T00:00:00+00:00')
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO affiliate_commissions
                (affiliate_twitch_login, streamer_login, stripe_event_id, stripe_invoice_id,
                 stripe_customer_id, brutto_cents, commission_cents, currency, status, created_at)
            VALUES ('partner_one', 'kunde', 'evt_cb', 'in_cb', 'cus_cb', 1000, 300,
                    'eur', 'pending', '2026-01-02T00:00:00+00:00')
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        let state = state(pool.clone());
        let session = state
            .create_affiliate_session("partner_one", "1001", "Partner One", "")
            .await
            .unwrap();
        let redirect_uri =
            "https://example.test/twitch/affiliate/connect/stripe/callback".to_string();
        state
            .save_affiliate_connect_state(
                "connect-state",
                &AffiliateConnectState {
                    redirect_uri: redirect_uri.clone(),
                    twitch_login: "partner_one".into(),
                },
            )
            .await
            .unwrap();
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "stripe_user_id": "acct_123"
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/transfers"))
            .and(header("Idempotency-Key", "affiliate-transfer:1"))
            .and(body_string_contains("amount=300"))
            .and(body_string_contains("destination=acct_123"))
            .and(body_string_contains("transfer_group=evt_cb"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "tr_cb"
            })))
            .expect(1)
            .mount(&server)
            .await;
        let stripe_client = StripeClient::new("sk_test_secret")
            .unwrap()
            .with_connect_base(server.uri())
            .with_api_base(server.uri());
        let mut headers = HeaderMap::new();
        headers.insert(
            COOKIE,
            format!("{}={}", AFFILIATE_COOKIE_NAME, session.session_id)
                .parse()
                .unwrap(),
        );

        let response = connect_stripe_callback_handler(
            Some(Extension(state)),
            Some(Extension(AffiliateStripeConfig {
                connect_client_id: None,
                client: Some(stripe_client),
            })),
            headers,
            Query(CallbackQuery {
                code: Some("oauth-code".into()),
                state: Some("connect-state".into()),
                error: None,
            }),
        )
        .await;
        assert!(response.status().is_redirection());
        let row = sqlx::query(
            "SELECT status, stripe_transfer_id, error_message FROM affiliate_commissions WHERE stripe_event_id = 'evt_cb'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.try_get::<String, _>("status").unwrap(), "transferred");
        assert_eq!(
            row.try_get::<Option<String>, _>("stripe_transfer_id")
                .unwrap()
                .as_deref(),
            Some("tr_cb")
        );
        assert!(row
            .try_get::<Option<String>, _>("error_message")
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn claim_api_legt_claim_an_und_liefert_claims() {
        let Some(pool) = pool("t_affiliate_claim_ok").await else {
            return;
        };
        create_tables(&pool).await;
        sqlx::query(
            r#"
            INSERT INTO affiliate_accounts
                (twitch_login, twitch_user_id, display_name, email, full_name, address_line1,
                 address_city, address_zip, address_country, created_at, updated_at)
            VALUES ('partner_one', '1001', 'Partner One', '', '', '', '', '', 'DE',
                    '2026-01-01T00:00:00+00:00', '2026-01-01T00:00:00+00:00')
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        let state = state(pool.clone());
        let session = state
            .create_affiliate_session("partner_one", "1001", "Partner One", "")
            .await
            .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            COOKIE,
            format!("{}={}", AFFILIATE_COOKIE_NAME, session.session_id)
                .parse()
                .unwrap(),
        );

        let response = claim_handler(
            Some(Extension(state.clone())),
            headers.clone(),
            State(pool.clone()),
            Bytes::from_static(br#"{"streamer_login":"Customer_One"}"#),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let value: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["ok"], true);
        assert_eq!(value["claimed"], "customer_one");

        sqlx::query(
            r#"
            INSERT INTO affiliate_commissions
                (affiliate_twitch_login, streamer_login, stripe_event_id, brutto_cents,
                 commission_cents, currency, status, created_at)
            VALUES ('partner_one', 'customer_one', 'evt_claim', 1000, 300, 'eur',
                    'pending', '2026-01-02T00:00:00+00:00')
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        let claims = api_claims_handler(Some(Extension(state)), headers, State(pool.clone())).await;
        assert_eq!(claims.status(), StatusCode::OK);
        let body = to_bytes(claims.into_body(), usize::MAX).await.unwrap();
        let value: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["claims"][0]["streamer_login"], "customer_one");
        assert_eq!(value["claims"][0]["commission_count"], 1);
        assert_eq!(value["claims"][0]["total_commission_cents"], 300);
    }

    #[tokio::test]
    async fn gutschriften_api_liste_pdf_und_ownership() {
        let Some(pool) = pool("t_affiliate_gutschriften_api").await else {
            return;
        };
        create_tables(&pool).await;
        for (login, user_id, display) in [
            ("affiliate_one", "1001", "Affiliate One"),
            ("affiliate_two", "1002", "Affiliate Two"),
        ] {
            sqlx::query(
                r#"
                INSERT INTO affiliate_accounts
                    (twitch_login, twitch_user_id, display_name, email, full_name, address_line1,
                     address_city, address_zip, address_country, created_at, updated_at)
                VALUES ($1, $2, $3, '', '', '', '', '', 'DE',
                        '2026-01-01T00:00:00+00:00', '2026-01-01T00:00:00+00:00')
                "#,
            )
            .bind(login)
            .bind(user_id)
            .bind(display)
            .execute(&pool)
            .await
            .unwrap();
        }
        sqlx::query(
            r#"
            INSERT INTO affiliate_gutschriften (
                id, gutschrift_number, affiliate_twitch_login, period_year, period_month,
                net_amount_cents, vat_amount_cents, gross_amount_cents, affiliate_ust_status,
                pdf_blob, pdf_generated_at, commission_ids, created_at
            ) VALUES
                (5, 'GS-202606-0001', 'affiliate_one', 2026, 6, 1000, 0, 1000,
                 'kleinunternehmer', E'\\x255044462d', '2026-07-01T00:00:00+00:00', '[1]', '2026-07-01T00:00:00+00:00'),
                (6, 'GS-202606-0002', 'affiliate_two', 2026, 6, 2000, 0, 2000,
                 'kleinunternehmer', E'\\x255044462d', '2026-07-01T00:00:00+00:00', '[2]', '2026-07-01T00:00:00+00:00')
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        let state = state(pool.clone());
        let session = state
            .create_affiliate_session("affiliate_one", "1001", "Affiliate One", "")
            .await
            .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            COOKIE,
            format!("{}={}", AFFILIATE_COOKIE_NAME, session.session_id)
                .parse()
                .unwrap(),
        );

        let list = api_gutschriften_handler(
            Some(Extension(state.clone())),
            Some(Extension(Arc::new(test_cipher()))),
            headers.clone(),
            State(pool.clone()),
        )
        .await;
        assert_eq!(list.status(), StatusCode::OK);
        let body = to_bytes(list.into_body(), usize::MAX).await.unwrap();
        let value: Value = serde_json::from_slice(&body).unwrap();
        let docs = value["gutschriften"].as_array().unwrap();
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0]["id"], 5);
        assert_eq!(
            docs[0]["download_path"],
            "/twitch/api/affiliate/gutschriften/5/pdf"
        );

        let own = api_gutschrift_pdf_handler(
            Some(Extension(state.clone())),
            headers.clone(),
            State(pool.clone()),
            Path("5".into()),
        )
        .await;
        assert_eq!(own.status(), StatusCode::OK);
        assert_eq!(own.headers().get(CONTENT_TYPE).unwrap(), "application/pdf");
        let own_body = to_bytes(own.into_body(), usize::MAX).await.unwrap();
        assert_eq!(&own_body[..5], b"%PDF-");

        let app = crate::build_affiliate_router(
            pool.clone(),
            crate::auth::security::RateLimiter::new(pool.clone(), TEST_FERNET_KEY.to_string()),
        )
        .layer(Extension(Arc::new(test_cipher())))
        .layer(Extension(state.clone()));
        let routed = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/twitch/api/affiliate/gutschriften")
                    .header(
                        COOKIE,
                        format!("{}={}", AFFILIATE_COOKIE_NAME, session.session_id),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(routed.status(), StatusCode::OK);

        let foreign = api_gutschrift_pdf_handler(
            Some(Extension(state)),
            headers,
            State(pool),
            Path("6".into()),
        )
        .await;
        assert_eq!(foreign.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn claim_api_blockt_aktive_partner() {
        let Some(pool) = pool("t_affiliate_claim_partner").await else {
            return;
        };
        create_tables(&pool).await;
        sqlx::query(
            r#"
            INSERT INTO affiliate_accounts
                (twitch_login, twitch_user_id, display_name, email, full_name, address_line1,
                 address_city, address_zip, address_country, created_at, updated_at)
            VALUES ('partner_one', '1001', 'Partner One', '', '', '', '', '', 'DE',
                    '2026-01-01T00:00:00+00:00', '2026-01-01T00:00:00+00:00')
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_streamers_partner_state (twitch_login, is_partner_active) VALUES ('customer_one', 1)",
        )
        .execute(&pool)
        .await
        .unwrap();
        let state = state(pool.clone());
        let session = state
            .create_affiliate_session("partner_one", "1001", "Partner One", "")
            .await
            .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            COOKIE,
            format!("{}={}", AFFILIATE_COOKIE_NAME, session.session_id)
                .parse()
                .unwrap(),
        );

        let response = claim_handler(
            Some(Extension(state)),
            headers,
            State(pool.clone()),
            Bytes::from_static(br#"{"streamer_login":"customer_one"}"#),
        )
        .await;
        assert_eq!(response.status(), StatusCode::CONFLICT);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let value: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["error"], "streamer_already_registered");
    }

    #[tokio::test]
    async fn profile_put_schreibt_nur_verschluesselte_pii() {
        let Some(pool) = pool("t_affiliate_profile_put").await else {
            return;
        };
        create_tables(&pool).await;
        sqlx::query(
            r#"
            INSERT INTO affiliate_accounts
                (twitch_login, twitch_user_id, display_name, email, full_name, address_line1,
                 address_city, address_zip, address_country, created_at, updated_at)
            VALUES ('affiliate_one', '1001', 'Affiliate One', '', '', '', '', '', '',
                    '2026-01-01T00:00:00+00:00', '2026-01-01T00:00:00+00:00')
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        let state = state(pool.clone());
        let session = state
            .create_affiliate_session("affiliate_one", "1001", "Affiliate One", "")
            .await
            .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            COOKIE,
            format!("{}={}", AFFILIATE_COOKIE_NAME, session.session_id)
                .parse()
                .unwrap(),
        );
        let body = Bytes::from(
            json!({
                "full_name": "Updated Affiliate",
                "email": "updated@example.com",
                "address_line1": "Neue Str. 8",
                "address_city": "Munich",
                "address_zip": "80331",
                "address_country": "de",
                "tax_id": "DE999",
                "ust_status": "regelbesteuert"
            })
            .to_string(),
        );

        let response = api_profile_update_handler(
            Some(Extension(state)),
            Some(Extension(Arc::new(test_cipher()))),
            headers,
            State(pool.clone()),
            body,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let value: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["ok"], true);
        assert_eq!(value["profile"]["email"], "updated@example.com");
        assert_eq!(value["profile"]["ust_status"], "regelbesteuert");

        let account = sqlx::query(
            "SELECT email, full_name, address_line1, address_city, address_zip, address_country FROM affiliate_accounts WHERE twitch_login = 'affiliate_one'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        for column in [
            "email",
            "full_name",
            "address_line1",
            "address_city",
            "address_zip",
            "address_country",
        ] {
            assert_eq!(account.try_get::<String, _>(column).unwrap(), "");
        }
        let raw = sqlx::query(
            "SELECT email_enc, full_name_enc, tax_id_enc, address_country, ust_status FROM affiliate_pii WHERE twitch_login = 'affiliate_one'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(raw
            .try_get::<Option<Vec<u8>>, _>("email_enc")
            .unwrap()
            .is_some());
        assert!(raw
            .try_get::<Option<Vec<u8>>, _>("full_name_enc")
            .unwrap()
            .is_some());
        assert!(raw
            .try_get::<Option<Vec<u8>>, _>("tax_id_enc")
            .unwrap()
            .is_some());
        assert_eq!(raw.try_get::<String, _>("address_country").unwrap(), "DE");
        assert_eq!(
            raw.try_get::<String, _>("ust_status").unwrap(),
            "regelbesteuert"
        );
    }

    #[tokio::test]
    async fn api_me_migriert_legacy_plaintext_pii() {
        let Some(pool) = pool("t_affiliate_api_me_migrate").await else {
            return;
        };
        create_tables(&pool).await;
        sqlx::query(
            r#"
            INSERT INTO affiliate_accounts
                (twitch_login, twitch_user_id, display_name, email, full_name, address_line1,
                 address_city, address_zip, address_country, created_at, updated_at)
            VALUES ('affiliate_one', '1001', 'Affiliate One', 'legacy@example.com',
                    'Legacy Partner', 'Altbau 5', 'Hamburg', '20095', 'DE',
                    '2026-01-01T00:00:00+00:00', '2026-01-01T00:00:00+00:00')
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        let state = state(pool.clone());
        let session = state
            .create_affiliate_session("affiliate_one", "1001", "Affiliate One", "")
            .await
            .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            COOKIE,
            format!("{}={}", AFFILIATE_COOKIE_NAME, session.session_id)
                .parse()
                .unwrap(),
        );

        let response = api_me_handler(
            Some(Extension(state)),
            Some(Extension(Arc::new(test_cipher()))),
            headers,
            State(pool.clone()),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let value: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["email"], "legacy@example.com");
        assert_eq!(value["full_name"], "Legacy Partner");
        assert_eq!(value["address_line1"], "Altbau 5");
        assert_eq!(value["address_city"], "Hamburg");
        assert_eq!(value["address_zip"], "20095");

        let account = sqlx::query(
            "SELECT email, full_name, address_line1, address_city, address_zip, address_country FROM affiliate_accounts WHERE twitch_login = 'affiliate_one'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        for column in [
            "email",
            "full_name",
            "address_line1",
            "address_city",
            "address_zip",
            "address_country",
        ] {
            assert_eq!(account.try_get::<String, _>(column).unwrap(), "");
        }
    }

    #[tokio::test]
    async fn profile_put_rejects_invalid_ust_status() {
        let Some(pool) = pool("t_affiliate_profile_invalid").await else {
            return;
        };
        create_tables(&pool).await;
        let state = state(pool.clone());
        let session = state
            .create_affiliate_session("affiliate_one", "1001", "Affiliate One", "")
            .await
            .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            COOKIE,
            format!("{}={}", AFFILIATE_COOKIE_NAME, session.session_id)
                .parse()
                .unwrap(),
        );
        let response = api_profile_update_handler(
            Some(Extension(state)),
            Some(Extension(Arc::new(test_cipher()))),
            headers,
            State(pool),
            Bytes::from(r#"{"ust_status":"foo"}"#),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let value: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["error"], "invalid_ust_status");
    }

    #[tokio::test]
    async fn claim_nicht_partner_pre_claim_erfolgreich() {
        let Some(pool) = pool("t_aff_claim_pre_claim").await else {
            return;
        };
        create_tables(&pool).await;

        let status = claim_streamer(&pool, "aff_one", "fresh_streamer")
            .await
            .unwrap();
        assert_eq!(status, ClaimStatus::Ok);

        let affiliate: String = sqlx::query_scalar(
            "SELECT affiliate_twitch_login FROM affiliate_streamer_claims WHERE claimed_streamer_login = 'fresh_streamer'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(affiliate, "aff_one");
    }

    #[tokio::test]
    async fn claim_etablierter_aktiver_partner_wird_abgelehnt() {
        let Some(pool) = pool("t_aff_claim_established_partner").await else {
            return;
        };
        create_tables(&pool).await;
        let partnered_at = ts_offset(chrono::Duration::hours(-25));
        insert_partner_state(&pool, "established", 1, Some(&partnered_at)).await;

        let status = claim_streamer(&pool, "aff_one", "established")
            .await
            .unwrap();
        assert_eq!(status, ClaimStatus::StreamerAlreadyRegistered);
    }

    #[tokio::test]
    async fn claim_frischer_aktiver_partner_in_nachfrist_erfolgreich() {
        let Some(pool) = pool("t_aff_claim_grace_partner").await else {
            return;
        };
        create_tables(&pool).await;
        let partnered_at = ts_offset(chrono::Duration::hours(-23));
        insert_partner_state(&pool, "new_partner", 1, Some(&partnered_at)).await;

        let status = claim_streamer(&pool, "aff_one", "new_partner")
            .await
            .unwrap();
        assert_eq!(status, ClaimStatus::Ok);
    }

    #[tokio::test]
    async fn claim_aktiver_partner_exakt_an_grace_grenze_erlaubt() {
        let Some(pool) = pool("t_aff_claim_grace_exact").await else {
            return;
        };
        create_tables(&pool).await;
        let now = chrono::DateTime::parse_from_rfc3339("2026-07-03T12:00:00+00:00")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let partnered_at = (now - chrono::Duration::seconds(POST_ACTIVATION_GRACE.seconds()))
            .to_rfc3339_opts(chrono::SecondsFormat::Micros, false);
        insert_partner_state(&pool, "new_partner_exact", 1, Some(&partnered_at)).await;

        let status = claim_streamer_at(&pool, "aff_one", "new_partner_exact", now)
            .await
            .unwrap();
        assert_eq!(status, ClaimStatus::Ok);
    }

    #[tokio::test]
    async fn claim_aktiver_partner_eine_sekunde_nach_grace_wird_abgelehnt() {
        let Some(pool) = pool("t_aff_claim_grace_plus_one").await else {
            return;
        };
        create_tables(&pool).await;
        let now = chrono::DateTime::parse_from_rfc3339("2026-07-03T12:00:00+00:00")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let partnered_at = (now - chrono::Duration::seconds(POST_ACTIVATION_GRACE.seconds() + 1))
            .to_rfc3339_opts(chrono::SecondsFormat::Micros, false);
        insert_partner_state(&pool, "new_partner_late", 1, Some(&partnered_at)).await;

        let status = claim_streamer_at(&pool, "aff_one", "new_partner_late", now)
            .await
            .unwrap();
        assert_eq!(status, ClaimStatus::StreamerAlreadyRegistered);
    }

    #[tokio::test]
    async fn claim_aktiver_partner_malformed_partnered_at_wird_abgelehnt() {
        let Some(pool) = pool("t_aff_claim_grace_malformed").await else {
            return;
        };
        create_tables(&pool).await;
        insert_partner_state(&pool, "bad_partnered_at", 1, Some("not-a-timestamp")).await;

        let status = claim_streamer(&pool, "aff_one", "bad_partnered_at")
            .await
            .unwrap();
        assert_eq!(status, ClaimStatus::StreamerAlreadyRegistered);
    }

    #[tokio::test]
    async fn claim_bestehende_frische_reservierung_blockiert() {
        let Some(pool) = pool("t_aff_claim_fresh_existing").await else {
            return;
        };
        create_tables(&pool).await;
        let claimed_at = ts_offset(chrono::Duration::days(-2));
        insert_claim(&pool, "aff_old", "reserved", &claimed_at).await;

        let status = claim_streamer(&pool, "aff_new", "reserved").await.unwrap();
        assert_eq!(status, ClaimStatus::AlreadyClaimed);

        let affiliate: String = sqlx::query_scalar(
            "SELECT affiliate_twitch_login FROM affiliate_streamer_claims WHERE claimed_streamer_login = 'reserved'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(affiliate, "aff_old");
    }

    #[tokio::test]
    async fn claim_abgelaufene_reservierung_wird_ueberschrieben() {
        let Some(pool) = pool("t_aff_claim_expired_reclaim").await else {
            return;
        };
        create_tables(&pool).await;
        let old_claimed_at = ts_offset(chrono::Duration::days(-5));
        insert_claim(&pool, "aff_old", "stale_slot", &old_claimed_at).await;

        let status = claim_streamer(&pool, "aff_new", "stale_slot")
            .await
            .unwrap();
        assert_eq!(status, ClaimStatus::Ok);

        let (affiliate, claimed_at): (String, String) = sqlx::query_as(
            "SELECT affiliate_twitch_login, claimed_at FROM affiliate_streamer_claims WHERE claimed_streamer_login = 'stale_slot'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(affiliate, "aff_new");
        assert_ne!(claimed_at, old_claimed_at);
    }

    #[tokio::test]
    async fn claim_konvertierter_claim_bleibt_blockiert() {
        let Some(pool) = pool("t_aff_claim_converted_blocks").await else {
            return;
        };
        create_tables(&pool).await;
        let old_claimed_at = ts_offset(chrono::Duration::days(-10));
        let partnered_at = ts_offset(chrono::Duration::days(-2));
        insert_claim(&pool, "aff_old", "converted", &old_claimed_at).await;
        insert_partner_state(&pool, "converted", 1, Some(&partnered_at)).await;

        let status = claim_streamer(&pool, "aff_new", "converted").await.unwrap();
        assert_eq!(status, ClaimStatus::AlreadyClaimed);

        let affiliate: String = sqlx::query_scalar(
            "SELECT affiliate_twitch_login FROM affiliate_streamer_claims WHERE claimed_streamer_login = 'converted'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(affiliate, "aff_old");
    }

    #[tokio::test]
    async fn claim_race_zwei_parallele_claims_einer_gewinnt() {
        let Some(pool) = pool("t_aff_claim_race").await else {
            return;
        };
        create_tables(&pool).await;

        let first_pool = pool.clone();
        let first =
            tokio::spawn(async move { claim_streamer(&first_pool, "aff_a", "race_slot").await });
        let second_pool = pool.clone();
        let second =
            tokio::spawn(async move { claim_streamer(&second_pool, "aff_b", "race_slot").await });

        let outcomes = vec![
            first.await.unwrap().unwrap(),
            second.await.unwrap().unwrap(),
        ];
        assert_eq!(
            outcomes
                .iter()
                .filter(|status| **status == ClaimStatus::Ok)
                .count(),
            1
        );
        assert_eq!(
            outcomes
                .iter()
                .filter(|status| **status == ClaimStatus::AlreadyClaimed)
                .count(),
            1
        );
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM affiliate_streamer_claims WHERE claimed_streamer_login = 'race_slot'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count, 1);
    }
}
