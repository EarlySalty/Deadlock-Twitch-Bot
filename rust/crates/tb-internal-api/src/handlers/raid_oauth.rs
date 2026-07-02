//! Handler für die 6 Raid-OAuth-Endpoints (nativer Port der bislang an
//! Python 8779 proxied Routen unter `/raid/auth-url`, `/raid/auth-state`,
//! `/raid/block-state`, `/raid/go-url`, `/raid/requirements`,
//! `/raid/oauth-callback`).
//!
//! # Vertrags-Parität zu `bot/internal_api/routes/raid.py`
//!
//! ```text
//! GET  /internal/twitch/v1/raid/auth-url
//!   ?login=<str>  [&discord_user_id=<digits>]  [&scope_profile=<str>]
//!   → 200 {ok:true, auth_url:<str>, login:<str>}
//!   → 400 {error:"bad_request",    message:"invalid or missing login"}
//!   → 403 {error:"forbidden",      message:"forbidden"}
//!   → 404 {error:"not_found",      message:"resource not found"}
//!   → 503 {error:"upstream_unavailable", message:"upstream unavailable"}
//!
//! GET  /internal/twitch/v1/raid/auth-state
//!   ?discord_user_id=<digits>  (required)
//!   → 200 {ok:true, discord_user_id, twitch_login, twitch_user_id,
//!           authorized, partner_opt_out, token_blacklisted,
//!           raid_blacklisted, blocked}
//!   → 400 / 500
//!
//! GET  /internal/twitch/v1/raid/block-state
//!   ?discord_user_id=<digits>  [&twitch_login=<str>]
//!   → 200 {ok:true, <same Felder wie auth-state>}
//!   → 400 / 500
//!
//! GET  /internal/twitch/v1/raid/go-url
//!   ?state=<str>  (required)
//!   → 200 {ok:true, auth_url:<str>}
//!   → 400 {error:"bad_request",  message:"missing state parameter"}
//!   → 404 {error:"not_found",    message:"state not found or expired"}
//!   → 503 {error:"upstream_unavailable", message:"upstream unavailable"}
//!
//! POST /internal/twitch/v1/raid/requirements
//!   body {login|streamer|twitch_login: <str>, [guild_id, channel_id, role_id]}
//!   → 200 {ok:true, login:<str>, message:<str>}
//!   → 400 / 403 / 404 / 503
//!   nativ verdrahtet: tb-bot sendet die Discord-DM ueber den Broker und
//!   dedupliziert persistent pro Partner/Zweck.
//!
//! POST /internal/twitch/v1/raid/oauth-callback
//!   body {code:<str>, state:<str>, error:<str>,
//!         [guild_id, channel_id, role_id]}
//!   → HTTP IMMER 200; Ergebnis-Status nur im Body:
//!     {status:<200..599>, title:<str>, body_html:<str>[, redirect_url]}
//!   Idempotenz über den geteilten Layer (Scope-Key, Fingerprint→409,
//!   Replay mit X-Idempotency-Replayed) — OAuth-Codes sind Single-Use.
//!   NICHT verdrahtet, bis die complete_setup-/sync_partner_state-Followups
//!   nativ sind (Python startet sie als Background-Tasks nach save_auth).
//! ```
//!
//! # Port-Trait
//!
//! `tb-internal-api` hängt bewusst NICHT an `tb-raid` — Crypto/DB-Schicht
//! gehört zum Kompositions-Root. Alle sechs Operationen werden über
//! [`RaidOAuthPort`] abstrahiert; die Impl mit dem echten `tb-raid`-Stack
//! wird in `rust/bin/tb-bot` verdrahtet (s. `wiring_needed.composition_root`
//! in der Orchestrator-Ausgabe).
//!
//! Fehler aus dem Trait:
//! - `RaidOAuthError::NotInitialized`  → 503
//! - `RaidOAuthError::NotFound`        → 404
//! - `RaidOAuthError::Forbidden`       → 403
//! - `RaidOAuthError::BadRequest(msg)` → 400
//! - `RaidOAuthError::Upstream`        → 503
//! - `RaidOAuthError::Internal`        → 500

use axum::{
    extract::{Extension, Query},
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tb_domain::normalize_twitch_login;
use tb_http_core::{ApiError, AuthLevel};

// ── Fehlertyp des Port-Traits ─────────────────────────────────────────────────

/// Fehler, die der [`RaidOAuthPort`] zurückgeben kann.
/// Handler übersetzen diese 1:1 in HTTP-Statuscodes + ApiError-Payload.
#[derive(Debug)]
pub enum RaidOAuthError {
    /// Raid-Stack nicht initialisiert (Python `_empty_*`-Fallbacks → 503).
    NotInitialized,
    /// Ressource nicht gefunden (Python `LookupError` → 404).
    NotFound,
    /// Zugriff verboten (Python `PermissionError` → 403).
    Forbidden,
    /// Ungültige Eingabe (Python `ValueError` → 400), Nachricht wird geloggt.
    BadRequest(String),
    /// Upstream nicht verfügbar (Python `RuntimeError` → 503).
    Upstream,
    /// Unerwarteter Fehler → 500.
    Internal,
}

impl From<RaidOAuthError> for ApiError {
    fn from(e: RaidOAuthError) -> Self {
        match e {
            RaidOAuthError::NotInitialized => ApiError::unavailable(),
            RaidOAuthError::NotFound => ApiError::not_found(),
            // Python raid.py:53-57: error="forbidden", message="forbidden" —
            // NICHT die Loopback-Middleware-Message.
            RaidOAuthError::Forbidden => ApiError::forbidden_generic(),
            RaidOAuthError::BadRequest(_) => ApiError::bad_request("invalid request parameters"),
            RaidOAuthError::Upstream => ApiError::unavailable(),
            RaidOAuthError::Internal => ApiError::internal(),
        }
    }
}

// ── Rückgabe-Typen des Port-Traits ────────────────────────────────────────────

/// Normalisiertes State-Payload für auth-state und block-state.
/// Entspricht dem Python `_normalize_raid_state_payload`-Ergebnis.
#[derive(Debug, Clone)]
pub struct RaidStatePayload {
    pub discord_user_id: Option<String>,
    pub twitch_login: Option<String>,
    pub twitch_user_id: Option<String>,
    pub authorized: bool,
    pub partner_opt_out: bool,
    pub token_blacklisted: bool,
    pub raid_blacklisted: bool,
    pub blocked: bool,
}

/// Ergebnis des OAuth-Callbacks (Python: dict mit status/title/body_html).
#[derive(Debug, Clone)]
pub struct OAuthCallbackResult {
    /// Ergebnis-Status im Body (wird auf 200–599 geclampt wie in Python;
    /// der HTTP-Status der Antwort ist IMMER 200).
    pub status: u16,
    pub title: String,
    pub body_html: String,
    /// Nur im Erfolgsfall gesetzt (Python: `redirect_url` nur im 200-Dict).
    pub redirect_url: Option<String>,
}

// ── Port-Trait ────────────────────────────────────────────────────────────────

/// Abstraktion über den tb-raid-OAuth-Stack für `tb-internal-api`.
/// Implementierung in `rust/bin/tb-bot` (Composition-Root).
#[async_trait::async_trait]
pub trait RaidOAuthPort: Send + Sync {
    /// Erzeugt die Authorize-URL für `login`.
    /// `discord_user_id` + `scope_profile` sind optionale Hints.
    ///
    /// Python: `_invoke_raid_auth_url` → `_raid_auth_url`.
    async fn auth_url(
        &self,
        login: &str,
        discord_user_id: Option<&str>,
        scope_profile: Option<&str>,
    ) -> Result<String, RaidOAuthError>;

    /// Gibt den OAuth-Auth-State für einen Discord-User zurück.
    ///
    /// Python: `_raid_auth_state(discord_user_id)`.
    async fn auth_state(
        &self,
        discord_user_id: &str,
    ) -> Result<RaidStatePayload, RaidOAuthError>;

    /// Gibt den Block-State für einen Discord-User und/oder Twitch-Login zurück.
    ///
    /// Python: `_raid_block_state(discord_user_id=..., twitch_login=...)`.
    async fn block_state(
        &self,
        discord_user_id: Option<&str>,
        twitch_login: Option<&str>,
    ) -> Result<RaidStatePayload, RaidOAuthError>;

    /// Löst einen State-Token auf und gibt die Authorize-URL zurück.
    /// Gibt `None` zurück wenn der State nicht mehr gültig/unbekannt ist
    /// (→ 404).
    ///
    /// Python: `_raid_go_url(state)`.
    async fn go_url(&self, state: &str) -> Result<Option<String>, RaidOAuthError>;

    /// Sendet Raid-Anforderungen für `login` und gibt eine Status-Nachricht zurück.
    ///
    /// Python: `_raid_requirements(login)`.
    async fn requirements(&self, login: &str) -> Result<String, RaidOAuthError>;

    /// Verarbeitet den OAuth-Callback.
    ///
    /// Python: `_raid_oauth_callback(code=..., state=..., error=...)`.
    async fn oauth_callback(
        &self,
        code: &str,
        state: &str,
        error: &str,
    ) -> Result<OAuthCallbackResult, RaidOAuthError>;

    /// Prüft `guild_id`/`channel_id`/`role_id` gegen die konfigurierten Allowlists.
    /// Kein Fehler wenn `None` übergeben wird (Felder fehlen im Body) UND kein Guard aktiv.
    /// Wenn ein Guard aktiv ist und der Wert fehlt oder nicht in der Allowlist → `Forbidden`.
    ///
    /// Python: `_enforce_discord_action_scope(body)`.
    async fn enforce_discord_action_scope(
        &self,
        guild_id: Option<&serde_json::Value>,
        channel_id: Option<&serde_json::Value>,
        role_id: Option<&serde_json::Value>,
    ) -> Result<(), RaidOAuthError>;
}

// ── Extension-Wrapper (None → 503, wie ManualRaidExt) ────────────────────────

/// Extension-Wrapper für den Router.
/// `None` = Raid-OAuth-Stack nicht verdrahtet → alle Handler antworten 503.
#[derive(Clone)]
pub struct RaidOAuthExt(pub Option<Arc<dyn RaidOAuthPort>>);

// ── Query- und Request-Structs ────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct AuthUrlQuery {
    #[serde(default)]
    pub login: Option<String>,
    #[serde(default)]
    pub discord_user_id: Option<String>,
    #[serde(default)]
    pub scope_profile: Option<String>,
}

#[derive(Deserialize)]
pub struct AuthStateQuery {
    #[serde(default)]
    pub discord_user_id: Option<String>,
}

#[derive(Deserialize)]
pub struct BlockStateQuery {
    #[serde(default)]
    pub discord_user_id: Option<String>,
    #[serde(default)]
    pub twitch_login: Option<String>,
}

#[derive(Deserialize)]
pub struct GoUrlQuery {
    #[serde(default)]
    pub state: Option<String>,
}

#[derive(Deserialize)]
pub struct RequirementsRequest {
    #[serde(default)]
    pub login: Option<String>,
    #[serde(default)]
    pub streamer: Option<String>,
    #[serde(default)]
    pub twitch_login: Option<String>,
    // Discord-Scope-Allowlist-Felder — werden zur Idempotenz-Fingerprint-
    // Kompatibilität mitgelesen, aber (bis zum nativen Idempotenz-Layer) nicht
    // ausgewertet (open_risk: _enforce_discord_action_scope fehlt noch nativ).
    #[serde(default)]
    pub guild_id: Option<serde_json::Value>,
    #[serde(default)]
    pub channel_id: Option<serde_json::Value>,
    #[serde(default)]
    pub role_id: Option<serde_json::Value>,
}

#[derive(Deserialize)]
pub struct OAuthCallbackRequest {
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    // Discord-Scope-Felder wie oben.
    #[serde(default)]
    pub guild_id: Option<serde_json::Value>,
    #[serde(default)]
    pub channel_id: Option<serde_json::Value>,
    #[serde(default)]
    pub role_id: Option<serde_json::Value>,
}

// ── Response-Structs ──────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct AuthUrlResponse {
    pub ok: bool,
    pub auth_url: String,
    pub login: String,
}

#[derive(Serialize)]
pub struct StateResponse {
    pub ok: bool,
    pub discord_user_id: Option<String>,
    pub twitch_login: Option<String>,
    pub twitch_user_id: Option<String>,
    pub authorized: bool,
    pub partner_opt_out: bool,
    pub token_blacklisted: bool,
    pub raid_blacklisted: bool,
    pub blocked: bool,
}

impl StateResponse {
    fn from_payload(ok: bool, p: RaidStatePayload) -> Self {
        Self {
            ok,
            discord_user_id: p.discord_user_id,
            twitch_login: p.twitch_login,
            twitch_user_id: p.twitch_user_id,
            authorized: p.authorized,
            partner_opt_out: p.partner_opt_out,
            token_blacklisted: p.token_blacklisted,
            raid_blacklisted: p.raid_blacklisted,
            blocked: p.blocked,
        }
    }
}

#[derive(Serialize)]
pub struct GoUrlResponse {
    pub ok: bool,
    pub auth_url: String,
}

#[derive(Serialize)]
pub struct RequirementsResponse {
    pub ok: bool,
    pub login: String,
    pub message: String,
}

// OAuthCallbackResponse wird direkt als serde_json::Value gerendert
// (Python liefert das result-dict transparent, inkl. beliebiger Extra-Felder).

// ── Hilfsfunktionen ───────────────────────────────────────────────────────────

/// Normalisiert einen Discord-User-ID-String: nur Ziffern, nicht leer.
/// Python: `normalize_discord_user_id(value, required=...)`.
fn normalize_discord_user_id(raw: &str, required: bool) -> Result<Option<String>, ApiError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        if required {
            return Err(ApiError::bad_request("invalid discord_user_id"));
        }
        return Ok(None);
    }
    if !trimmed.chars().all(|c| c.is_ascii_digit()) {
        return Err(ApiError::bad_request("invalid discord_user_id"));
    }
    Ok(Some(trimmed.to_string()))
}

/// Normalisiert ein `login`-Query-/Body-Feld für den `auth-url`-Endpoint.
/// Erlaubt `public:website_onboarding` und `discord:<digits>` zusätzlich
/// zum regulären Twitch-Login.
///
/// Python: `normalize_raid_auth_target`.
fn normalize_raid_auth_target(raw: &str) -> Option<String> {
    let decoded = percent_decode(raw.trim());
    if decoded.is_empty() {
        return None;
    }
    let lowered = decoded.to_ascii_lowercase();
    // Sonder-Targets
    if lowered == "public:website_onboarding" {
        return Some(lowered);
    }
    if let Some(suffix) = lowered.strip_prefix("discord:") {
        let id = suffix.trim();
        if !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()) {
            return Some(format!("discord:{id}"));
        }
        return None;
    }
    // Regulärer Twitch-Login
    normalize_twitch_login(&decoded)
}

/// Minimales URL-Percent-Decoding für `%XX`-Sequenzen.
/// Python-Seite nutzt `urllib.parse.unquote` — wir brauchen nur das Wesentliche.
fn percent_decode(s: &str) -> String {
    // Schneller Pfad: kein `%` → kein Decoding nötig.
    if !s.contains('%') {
        return s.to_string();
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (
                (bytes[i + 1] as char).to_digit(16),
                (bytes[i + 2] as char).to_digit(16),
            ) {
                out.push(((h << 4) | l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ── Handler ───────────────────────────────────────────────────────────────────

/// `GET /internal/twitch/v1/raid/auth-url`
///
/// Erzeugt die Twitch-Authorize-URL für den angegebenen Login. Optionale
/// Parameter `discord_user_id` und `scope_profile` werden durchgereicht.
///
/// Besonderheit: wenn `login` mit `discord:` beginnt, wird die enthaltene
/// Discord-User-ID extrahiert und (falls `discord_user_id`-Parameter separat
/// angegeben wurde und nicht übereinstimmt) mit 400 abgelehnt.
pub async fn auth_url_handler(
    auth: AuthLevel,
    Extension(port): Extension<RaidOAuthExt>,
    Query(params): Query<AuthUrlQuery>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }
    let Some(port) = port.0 else {
        return Err(ApiError::unavailable());
    };

    // Login normalisieren (Python: _normalize_raid_auth_target)
    let raw_login = params.login.as_deref().unwrap_or("").trim().to_string();
    let login = normalize_raid_auth_target(&raw_login)
        .ok_or(ApiError::bad_request("invalid or missing login"))?;

    // discord_user_id aus Query-Param (optional)
    let raw_discord = params.discord_user_id.as_deref().unwrap_or("");
    let mut discord_user_id = normalize_discord_user_id(raw_discord, false)
        .map_err(|_| ApiError::bad_request("invalid request parameters"))?;

    // scope_profile trimmen (Python: str(request.query.get("scope_profile") or "").strip() or None)
    let scope_profile = params
        .scope_profile
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    // Python: wenn login mit "discord:" beginnt → discord_user_id aus Login ableiten
    if login.starts_with("discord:") {
        let target_discord_id = login.strip_prefix("discord:").unwrap_or("").trim();
        if let Some(ref existing) = discord_user_id {
            if existing.as_str() != target_discord_id {
                return Err(ApiError::bad_request("invalid request parameters"));
            }
        }
        discord_user_id = Some(target_discord_id.to_string());
    }

    let auth_url_result = port
        .auth_url(
            &login,
            discord_user_id.as_deref(),
            scope_profile.as_deref(),
        )
        .await;

    let auth_url = match auth_url_result {
        Ok(url) if url.is_empty() => {
            // Python: `if not auth_url: return _json_error("upstream_unavailable", 503, ...)`
            return Err(ApiError::unavailable());
        }
        Ok(url) => url,
        Err(RaidOAuthError::NotInitialized) => {
            return Err(ApiError::unavailable());
        }
        Err(e) => {
            tracing::error!("raid auth url Fehler: {e:?}");
            return Err(ApiError::from(e));
        }
    };

    Ok(Json(AuthUrlResponse {
        ok: true,
        auth_url,
        login,
    }))
}

/// `GET /internal/twitch/v1/raid/auth-state`
///
/// Gibt den OAuth-Auth-State für einen Discord-User zurück.
/// `discord_user_id` ist Pflicht.
pub async fn auth_state_handler(
    auth: AuthLevel,
    Extension(port): Extension<RaidOAuthExt>,
    Query(params): Query<AuthStateQuery>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }
    let Some(port) = port.0 else {
        return Err(ApiError::unavailable());
    };

    // Python (raid.py:90-94) wrappt JEDEN ValueError dieser Route als
    // "invalid query parameters" — keine feldspezifische Message.
    let raw = params.discord_user_id.as_deref().unwrap_or("");
    let discord_user_id = normalize_discord_user_id(raw, true)
        .map_err(|_| ApiError::bad_request("invalid query parameters"))?
        .ok_or(ApiError::bad_request("invalid query parameters"))?;

    let payload = port.auth_state(&discord_user_id).await.map_err(|e| {
        tracing::error!("raid auth state Fehler: {e:?}");
        ApiError::from(e)
    })?;

    Ok(Json(StateResponse::from_payload(true, payload)))
}

/// `GET /internal/twitch/v1/raid/block-state`
///
/// Gibt den Block-State für einen Discord-User und/oder Twitch-Login zurück.
/// Mindestens eines von `discord_user_id` oder `twitch_login` ist Pflicht.
pub async fn block_state_handler(
    auth: AuthLevel,
    Extension(port): Extension<RaidOAuthExt>,
    Query(params): Query<BlockStateQuery>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }
    let Some(port) = port.0 else {
        return Err(ApiError::unavailable());
    };

    // Python (raid.py:127-131) wrappt JEDEN ValueError dieser Route als
    // "invalid query parameters" — keine feldspezifische Message.
    let raw_discord = params.discord_user_id.as_deref().unwrap_or("");
    let discord_user_id = normalize_discord_user_id(raw_discord, false)
        .map_err(|_| ApiError::bad_request("invalid query parameters"))?;

    // twitch_login normalisieren (Python: _normalize_login via normalize_twitch_login)
    let raw_login = params.twitch_login.as_deref().unwrap_or("").trim().to_string();
    let twitch_login = if raw_login.is_empty() {
        None
    } else {
        let normalized = normalize_twitch_login(&raw_login)
            .ok_or(ApiError::bad_request("invalid query parameters"))?;
        Some(normalized)
    };

    // Python: if discord_user_id is None and not twitch_login: raise ValueError
    if discord_user_id.is_none() && twitch_login.is_none() {
        return Err(ApiError::bad_request("invalid query parameters"));
    }

    let payload = port
        .block_state(discord_user_id.as_deref(), twitch_login.as_deref())
        .await
        .map_err(|e| {
            tracing::error!("raid block state Fehler: {e:?}");
            ApiError::from(e)
        })?;

    Ok(Json(StateResponse::from_payload(true, payload)))
}

/// `GET /internal/twitch/v1/raid/go-url`
///
/// Löst einen State-Token auf und gibt die Authorize-URL zurück.
/// `state` ist Pflicht.
pub async fn go_url_handler(
    auth: AuthLevel,
    Extension(port): Extension<RaidOAuthExt>,
    Query(params): Query<GoUrlQuery>,
) -> Result<impl IntoResponse, ApiError> {
    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }
    let Some(port) = port.0 else {
        return Err(ApiError::unavailable());
    };

    let state = params
        .state
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_string();
    if state.is_empty() {
        return Err(ApiError::bad_request("missing state parameter"));
    }

    let maybe_url = port.go_url(&state).await.map_err(|e| {
        tracing::error!("raid go url Fehler: {e:?}");
        ApiError::from(e)
    })?;

    // Python (raid.py:145): 404 mit "state not found or expired".
    let auth_url = maybe_url
        .filter(|u| !u.trim().is_empty())
        .ok_or(ApiError::not_found_with("state not found or expired"))?;

    Ok(Json(GoUrlResponse {
        ok: true,
        auth_url,
    }))
}

/// `POST /internal/twitch/v1/raid/requirements`
///
/// Sendet Raid-Anforderungen für den angegebenen Login.
///
/// # Idempotenz
/// Python implementiert hier `_prepare_idempotency` + `_release_idempotency_owner`.
/// Der native Handler nutzt denselben Idempotency-Layer wie `oauth-callback`;
/// der Port dedupliziert zusaetzlich persistent pro Partner/Zweck.
pub async fn requirements_handler(
    auth: AuthLevel,
    headers: axum::http::HeaderMap,
    axum::extract::OriginalUri(uri): axum::extract::OriginalUri,
    Extension(port): Extension<RaidOAuthExt>,
    Extension(idem): Extension<crate::idempotency::IdempotencyState>,
    Json(raw_payload): Json<serde_json::Value>,
) -> Result<axum::response::Response, ApiError> {
    use crate::idempotency::{Prepared, IDEMPOTENCY_KEY_HEADER};

    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }
    let Some(port) = port.0 else {
        tracing::error!("raid requirements ohne verdrahteten Port aufgerufen");
        return Err(ApiError::unavailable());
    };
    let body: RequirementsRequest = serde_json::from_value(raw_payload.clone())
        .map_err(|_| ApiError::bad_request("invalid request body"))?;

    let raw_key = headers
        .get(IDEMPOTENCY_KEY_HEADER)
        .and_then(|v| v.to_str().ok());
    let path = uri.path().to_string();
    let path_qs = uri
        .path_and_query()
        .map(|pq| pq.as_str().to_string())
        .unwrap_or_else(|| path.clone());
    let owner = match idem
        .prepare(raw_key, "POST", &path, &path_qs, &raw_payload)
        .await
    {
        Prepared::Skip => None,
        Prepared::Immediate(resp) => return Ok(resp),
        Prepared::Owner(slot) => Some(slot),
    };

    let result = requirements_inner(port.as_ref(), body).await;
    match result {
        Ok(payload) => {
            if let Some(slot) = owner {
                slot.complete(200, &payload, true);
            }
            Ok((axum::http::StatusCode::OK, Json(payload)).into_response())
        }
        Err(error) => {
            if let Some(slot) = owner {
                slot.complete(error.status.as_u16(), &error.payload_json(), false);
            }
            Err(error)
        }
    }
}

async fn requirements_inner(
    port: &dyn RaidOAuthPort,
    body: RequirementsRequest,
) -> Result<serde_json::Value, ApiError> {
    // Discord-Scope-Guard (Python: _enforce_discord_action_scope →
    // 403 {"error":"forbidden","message":"action outside configured scope"}).
    port.enforce_discord_action_scope(
        body.guild_id.as_ref(),
        body.channel_id.as_ref(),
        body.role_id.as_ref(),
    )
    .await
    .map_err(|_| ApiError::forbidden_scope())?;

    // Python: body.get("login") or body.get("streamer") or body.get("twitch_login")
    let raw = {
        let a = body.login.as_deref().unwrap_or("").trim().to_string();
        if !a.is_empty() {
            a
        } else {
            let b = body.streamer.as_deref().unwrap_or("").trim().to_string();
            if !b.is_empty() {
                b
            } else {
                body.twitch_login
                    .as_deref()
                    .unwrap_or("")
                    .trim()
                    .to_string()
            }
        }
    };

    let login = normalize_twitch_login(&raw)
        .ok_or(ApiError::bad_request("invalid or missing login"))?;

    let message = port.requirements(&login).await.map_err(|e| {
        tracing::error!("raid requirements Fehler: {e:?}");
        ApiError::from(e)
    })?;

    Ok(serde_json::json!(RequirementsResponse {
        ok: true,
        login,
        message,
    }))
}

/// `POST /internal/twitch/v1/raid/oauth-callback`
///
/// Verarbeitet den OAuth-Callback-Payload und gibt status/title/body_html
/// (+ redirect_url im Erfolgsfall) zurück. Der HTTP-Status der Antwort ist
/// IMMER 200 — Python ruft `_json_response(result)` ohne `status=`-Argument
/// auf (`raid.py:291`); der geklemmte `result.status` (200–599) erscheint
/// nur als Feld im JSON-Body. Die OAuth-Flow-UI wertet das Body-Feld aus.
///
/// # Idempotenz (geteilter Layer, Python-Vertrag)
/// OAuth-Codes sind Single-Use — ein Netz-Retry mit demselben
/// `Idempotency-Key` bekommt die gecachte Antwort statt eines geplatzten
/// Zweit-Tauschs. Wie in Python wird das Callback-ERGEBNIS (auch mit
/// `status: 4xx/5xx` im Body) als HTTP-200 gecacht (`raid.py:292`:
/// `owner_cacheable = True` nach dem Callback-Aufruf); nur
/// Transport-Fehler (401/403/503 vor dem Callback) sind nicht cachebar.
pub async fn oauth_callback_handler(
    auth: AuthLevel,
    headers: axum::http::HeaderMap,
    axum::extract::OriginalUri(uri): axum::extract::OriginalUri,
    Extension(port): Extension<RaidOAuthExt>,
    Extension(idem): Extension<crate::idempotency::IdempotencyState>,
    Json(raw_payload): Json<serde_json::Value>,
) -> Result<axum::response::Response, ApiError> {
    use crate::idempotency::{Prepared, IDEMPOTENCY_KEY_HEADER};

    if !auth.is_privileged() {
        return Err(ApiError::unauthorized());
    }
    let Some(port) = port.0 else {
        return Err(ApiError::unavailable());
    };
    let body: OAuthCallbackRequest = serde_json::from_value(raw_payload.clone())
        .map_err(|_| ApiError::bad_request("invalid request body"))?;

    let raw_key = headers
        .get(IDEMPOTENCY_KEY_HEADER)
        .and_then(|v| v.to_str().ok());
    let path = uri.path().to_string();
    let path_qs = uri
        .path_and_query()
        .map(|pq| pq.as_str().to_string())
        .unwrap_or_else(|| path.clone());

    match idem.prepare(raw_key, "POST", &path, &path_qs, &raw_payload).await {
        Prepared::Immediate(resp) => Ok(resp),
        Prepared::Skip => {
            let json_body = run_oauth_callback(&*port, &body).await?;
            Ok((axum::http::StatusCode::OK, Json(json_body)).into_response())
        }
        Prepared::Owner(slot) => match run_oauth_callback(&*port, &body).await {
            Ok(json_body) => {
                // Python cacht das Ergebnis als HTTP 200 — auch wenn der
                // Body-Status ein Fehler ist (Spec: Sonderfall oauth_callback).
                slot.complete(200, &json_body, true);
                Ok((axum::http::StatusCode::OK, Json(json_body)).into_response())
            }
            Err(e) => {
                slot.complete(e.status.as_u16(), &e.payload_json(), false);
                Err(e)
            }
        },
    }
}

/// Kern des Callback-Handlers: Scope-Guard → Port-Aufruf → Body-Aufbau.
async fn run_oauth_callback(
    port: &dyn RaidOAuthPort,
    body: &OAuthCallbackRequest,
) -> Result<serde_json::Value, ApiError> {
    // Discord-Scope-Guard (Python: _enforce_discord_action_scope →
    // 403 {"error":"forbidden","message":"action outside configured scope"}).
    port.enforce_discord_action_scope(
        body.guild_id.as_ref(),
        body.channel_id.as_ref(),
        body.role_id.as_ref(),
    )
    .await
    .map_err(|_| ApiError::forbidden_scope())?;

    let code = body.code.as_deref().unwrap_or("");
    let state = body.state.as_deref().unwrap_or("");
    let error = body.error.as_deref().unwrap_or("");

    let result = port.oauth_callback(code, state, error).await.map_err(|e| {
        tracing::error!("raid oauth callback Fehler: {e:?}");
        ApiError::from(e)
    })?;

    // Python: status_code = max(200, min(status_code, 599)) — nur Body-Feld,
    // NICHT der HTTP-Status (raid.py:284-291, _json_response default 200).
    let status_code = result.status.clamp(200, 599);

    let mut json_body = serde_json::json!({
        "status": status_code,
        "title": result.title,
        "body_html": result.body_html,
    });
    // Python: redirect_url nur im Erfolgs-Dict vorhanden.
    if let Some(redirect_url) = result.redirect_url {
        json_body["redirect_url"] = serde_json::Value::String(redirect_url);
    }
    Ok(json_body)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        extract::ConnectInfo,
        http::{Request, StatusCode},
        middleware,
        routing::{get, post},
        Router,
    };
    use std::net::SocketAddr;
    use tower::ServiceExt;

    use tb_http_core::{internal_auth, loopback_only, ExpectedToken, INTERNAL_API_BASE_PATH};

    // ── Stub-Implementierung ─────────────────────────────────────────────────

    struct StubPort {
        /// Wenn `true`, simuliert "nicht initialisiert" (→ 503).
        not_initialized: bool,
    }

    impl StubPort {
        fn ready() -> Self {
            Self {
                not_initialized: false,
            }
        }
        fn uninit() -> Self {
            Self {
                not_initialized: true,
            }
        }
    }

    #[async_trait::async_trait]
    impl RaidOAuthPort for StubPort {
        async fn auth_url(
            &self,
            login: &str,
            discord_user_id: Option<&str>,
            _scope_profile: Option<&str>,
        ) -> Result<String, RaidOAuthError> {
            if self.not_initialized {
                return Err(RaidOAuthError::NotInitialized);
            }
            if login == "forbidden_login" {
                return Err(RaidOAuthError::Forbidden);
            }
            if login == "notfound_login" {
                return Err(RaidOAuthError::NotFound);
            }
            let _ = discord_user_id;
            Ok(format!(
                "https://id.twitch.tv/oauth2/authorize?state=test&login={login}"
            ))
        }

        async fn auth_state(
            &self,
            discord_user_id: &str,
        ) -> Result<RaidStatePayload, RaidOAuthError> {
            if self.not_initialized {
                return Err(RaidOAuthError::NotInitialized);
            }
            Ok(RaidStatePayload {
                discord_user_id: Some(discord_user_id.to_string()),
                twitch_login: Some("dragscope".to_string()),
                twitch_user_id: Some("123".to_string()),
                authorized: true,
                partner_opt_out: false,
                token_blacklisted: false,
                raid_blacklisted: false,
                blocked: false,
            })
        }

        async fn block_state(
            &self,
            discord_user_id: Option<&str>,
            twitch_login: Option<&str>,
        ) -> Result<RaidStatePayload, RaidOAuthError> {
            if self.not_initialized {
                return Err(RaidOAuthError::NotInitialized);
            }
            Ok(RaidStatePayload {
                discord_user_id: discord_user_id.map(str::to_string),
                twitch_login: twitch_login.map(str::to_string),
                twitch_user_id: None,
                authorized: false,
                partner_opt_out: false,
                token_blacklisted: false,
                raid_blacklisted: false,
                blocked: false,
            })
        }

        async fn go_url(&self, state: &str) -> Result<Option<String>, RaidOAuthError> {
            if self.not_initialized {
                return Err(RaidOAuthError::NotInitialized);
            }
            if state == "expired" {
                return Ok(None);
            }
            Ok(Some(format!(
                "https://id.twitch.tv/oauth2/authorize?state={state}"
            )))
        }

        async fn requirements(&self, login: &str) -> Result<String, RaidOAuthError> {
            if self.not_initialized {
                return Err(RaidOAuthError::NotInitialized);
            }
            Ok(format!("sent to {login}"))
        }

        async fn oauth_callback(
            &self,
            _code: &str,
            _state: &str,
            _error: &str,
        ) -> Result<OAuthCallbackResult, RaidOAuthError> {
            if self.not_initialized {
                return Err(RaidOAuthError::NotInitialized);
            }
            Ok(OAuthCallbackResult {
                status: 200,
                title: "Autorisierung erfolgreich".to_string(),
                body_html: "<p>OK</p>".to_string(),
                redirect_url: Some("https://example.test/dashboard".to_string()),
            })
        }

        async fn enforce_discord_action_scope(
            &self,
            _guild_id: Option<&serde_json::Value>,
            _channel_id: Option<&serde_json::Value>,
            _role_id: Option<&serde_json::Value>,
        ) -> Result<(), RaidOAuthError> {
            // Stub: kein Guard aktiv.
            Ok(())
        }
    }

    /// Stub mit aktivem Discord-Scope-Guard (immer Forbidden).
    struct ScopeGuardedPort;

    #[async_trait::async_trait]
    impl RaidOAuthPort for ScopeGuardedPort {
        async fn auth_url(&self, _: &str, _: Option<&str>, _: Option<&str>) -> Result<String, RaidOAuthError> {
            Ok("https://example.com/auth".to_string())
        }
        async fn auth_state(&self, _: &str) -> Result<RaidStatePayload, RaidOAuthError> {
            Err(RaidOAuthError::NotInitialized)
        }
        async fn block_state(&self, _: Option<&str>, _: Option<&str>) -> Result<RaidStatePayload, RaidOAuthError> {
            Err(RaidOAuthError::NotInitialized)
        }
        async fn go_url(&self, _: &str) -> Result<Option<String>, RaidOAuthError> {
            Err(RaidOAuthError::NotInitialized)
        }
        async fn requirements(&self, _: &str) -> Result<String, RaidOAuthError> {
            Ok("ok".to_string())
        }
        async fn oauth_callback(&self, _: &str, _: &str, _: &str) -> Result<OAuthCallbackResult, RaidOAuthError> {
            Ok(OAuthCallbackResult { status: 200, title: "ok".to_string(), body_html: "ok".to_string(), redirect_url: None })
        }
        async fn enforce_discord_action_scope(
            &self,
            _guild_id: Option<&serde_json::Value>,
            _channel_id: Option<&serde_json::Value>,
            _role_id: Option<&serde_json::Value>,
        ) -> Result<(), RaidOAuthError> {
            // Simuliert einen aktiven Guard der den Wert ablehnt.
            Err(RaidOAuthError::Forbidden)
        }
    }

    // ── Router-Helfer ────────────────────────────────────────────────────────

    fn make_router(port: Option<Arc<dyn RaidOAuthPort>>) -> Router {
        let base = INTERNAL_API_BASE_PATH;
        Router::new()
            .route(&format!("{base}/raid/auth-url"), get(auth_url_handler))
            .route(&format!("{base}/raid/auth-state"), get(auth_state_handler))
            .route(&format!("{base}/raid/block-state"), get(block_state_handler))
            .route(&format!("{base}/raid/go-url"), get(go_url_handler))
            .route(
                &format!("{base}/raid/requirements"),
                post(requirements_handler),
            )
            .route(
                &format!("{base}/raid/oauth-callback"),
                post(oauth_callback_handler),
            )
            .layer(Extension(RaidOAuthExt(port)))
            .layer(Extension(crate::idempotency::IdempotencyState::new()))
            .layer(Extension(ExpectedToken("tok".to_string())))
            .layer(middleware::from_fn_with_state(
                "tok".to_string(),
                internal_auth,
            ))
            .layer(middleware::from_fn(loopback_only))
    }

    fn req(method: &str, uri: &str, body: &str, token: Option<&str>) -> Request<Body> {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .extension(ConnectInfo("127.0.0.1:55555".parse::<SocketAddr>().unwrap()));
        if let Some(t) = token {
            builder = builder.header("x-internal-token", t);
        }
        builder.body(Body::from(body.to_string())).unwrap()
    }

    async fn json_body(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), 65536).await.unwrap();
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
    }

    const BASE: &str = INTERNAL_API_BASE_PATH;

    // ── auth-url ─────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn auth_url_ohne_token_401() {
        let app = make_router(Some(Arc::new(StubPort::ready())));
        let resp = app
            .oneshot(req("GET", &format!("{BASE}/raid/auth-url?login=dragscope"), "", None))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn auth_url_ohne_login_400() {
        let app = make_router(Some(Arc::new(StubPort::ready())));
        let resp = app
            .oneshot(req("GET", &format!("{BASE}/raid/auth-url"), "", Some("tok")))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let j = json_body(resp).await;
        assert_eq!(j["error"], "bad_request");
        assert_eq!(j["message"], "invalid or missing login");
    }

    #[tokio::test]
    async fn auth_url_kein_port_503() {
        let app = make_router(None);
        let resp = app
            .oneshot(req(
                "GET",
                &format!("{BASE}/raid/auth-url?login=dragscope"),
                "",
                Some("tok"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn auth_url_normaler_login_200() {
        let app = make_router(Some(Arc::new(StubPort::ready())));
        let resp = app
            .oneshot(req(
                "GET",
                &format!("{BASE}/raid/auth-url?login=DragScope"),
                "",
                Some("tok"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let j = json_body(resp).await;
        assert_eq!(j["ok"], true);
        assert_eq!(j["login"], "dragscope");
        assert!(j["auth_url"].as_str().map(|u| u.contains("login=dragscope")).unwrap_or(false));
    }

    #[tokio::test]
    async fn auth_url_discord_prefix_extrahiert_discord_id() {
        let app = make_router(Some(Arc::new(StubPort::ready())));
        let resp = app
            .oneshot(req(
                "GET",
                &format!("{BASE}/raid/auth-url?login=discord:123456789"),
                "",
                Some("tok"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let j = json_body(resp).await;
        assert_eq!(j["login"], "discord:123456789");
    }

    #[tokio::test]
    async fn auth_url_discord_id_konflikt_400() {
        // discord_user_id stimmt nicht mit Login-Target überein
        let app = make_router(Some(Arc::new(StubPort::ready())));
        let resp = app
            .oneshot(req(
                "GET",
                &format!("{BASE}/raid/auth-url?login=discord:111&discord_user_id=999"),
                "",
                Some("tok"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn auth_url_forbidden_403() {
        let app = make_router(Some(Arc::new(StubPort::ready())));
        let resp = app
            .oneshot(req(
                "GET",
                &format!("{BASE}/raid/auth-url?login=forbidden_login"),
                "",
                Some("tok"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn auth_url_not_found_404() {
        let app = make_router(Some(Arc::new(StubPort::ready())));
        let resp = app
            .oneshot(req(
                "GET",
                &format!("{BASE}/raid/auth-url?login=notfound_login"),
                "",
                Some("tok"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    // ── auth-state ───────────────────────────────────────────────────────────

    #[tokio::test]
    async fn auth_state_discord_id_pflicht() {
        let app = make_router(Some(Arc::new(StubPort::ready())));
        let resp = app
            .oneshot(req("GET", &format!("{BASE}/raid/auth-state"), "", Some("tok")))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let j = json_body(resp).await;
        assert_eq!(j["error"], "bad_request");
    }

    #[tokio::test]
    async fn auth_state_nicht_numerisch_400() {
        let app = make_router(Some(Arc::new(StubPort::ready())));
        let resp = app
            .oneshot(req(
                "GET",
                &format!("{BASE}/raid/auth-state?discord_user_id=abc123"),
                "",
                Some("tok"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn auth_state_200_alle_felder() {
        let app = make_router(Some(Arc::new(StubPort::ready())));
        let resp = app
            .oneshot(req(
                "GET",
                &format!("{BASE}/raid/auth-state?discord_user_id=123456789"),
                "",
                Some("tok"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let j = json_body(resp).await;
        assert_eq!(j["ok"], true);
        assert_eq!(j["discord_user_id"], "123456789");
        assert_eq!(j["twitch_login"], "dragscope");
        assert_eq!(j["authorized"], true);
        assert_eq!(j["blocked"], false);
        // Alle Felder müssen vorhanden sein
        for key in &[
            "twitch_user_id",
            "partner_opt_out",
            "token_blacklisted",
            "raid_blacklisted",
        ] {
            assert!(j.get(key).is_some(), "Feld fehlt: {key}");
        }
    }

    // ── block-state ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn block_state_keiner_der_beiden_400() {
        let app = make_router(Some(Arc::new(StubPort::ready())));
        let resp = app
            .oneshot(req("GET", &format!("{BASE}/raid/block-state"), "", Some("tok")))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn block_state_mit_discord_id_200() {
        let app = make_router(Some(Arc::new(StubPort::ready())));
        let resp = app
            .oneshot(req(
                "GET",
                &format!("{BASE}/raid/block-state?discord_user_id=987654321"),
                "",
                Some("tok"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let j = json_body(resp).await;
        assert_eq!(j["ok"], true);
        assert_eq!(j["discord_user_id"], "987654321");
    }

    #[tokio::test]
    async fn block_state_mit_twitch_login_200() {
        let app = make_router(Some(Arc::new(StubPort::ready())));
        let resp = app
            .oneshot(req(
                "GET",
                &format!("{BASE}/raid/block-state?twitch_login=DragScope"),
                "",
                Some("tok"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let j = json_body(resp).await;
        // Login wird normalisiert
        assert_eq!(j["twitch_login"], "dragscope");
    }

    #[tokio::test]
    async fn block_state_ungültiger_login_400() {
        let app = make_router(Some(Arc::new(StubPort::ready())));
        // Zwei Zeichen = zu kurz für normalize_twitch_login
        let resp = app
            .oneshot(req(
                "GET",
                &format!("{BASE}/raid/block-state?twitch_login=ab"),
                "",
                Some("tok"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    // ── go-url ───────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn go_url_fehlender_state_400() {
        let app = make_router(Some(Arc::new(StubPort::ready())));
        let resp = app
            .oneshot(req("GET", &format!("{BASE}/raid/go-url"), "", Some("tok")))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let j = json_body(resp).await;
        assert_eq!(j["message"], "missing state parameter");
    }

    #[tokio::test]
    async fn go_url_abgelaufener_state_404() {
        let app = make_router(Some(Arc::new(StubPort::ready())));
        let resp = app
            .oneshot(req(
                "GET",
                &format!("{BASE}/raid/go-url?state=expired"),
                "",
                Some("tok"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn go_url_gültiger_state_200() {
        let app = make_router(Some(Arc::new(StubPort::ready())));
        let resp = app
            .oneshot(req(
                "GET",
                &format!("{BASE}/raid/go-url?state=valid_state_token"),
                "",
                Some("tok"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let j = json_body(resp).await;
        assert_eq!(j["ok"], true);
        assert!(j["auth_url"].as_str().map(|u| !u.is_empty()).unwrap_or(false));
    }

    // ── requirements ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn requirements_kein_login_400() {
        let app = make_router(Some(Arc::new(StubPort::ready())));
        let resp = app
            .oneshot(req(
                "POST",
                &format!("{BASE}/raid/requirements"),
                r#"{}"#,
                Some("tok"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let j = json_body(resp).await;
        assert_eq!(j["error"], "bad_request");
        assert_eq!(j["message"], "invalid or missing login");
    }

    #[tokio::test]
    async fn requirements_login_alias_streamer() {
        let app = make_router(Some(Arc::new(StubPort::ready())));
        // Python: body.get("streamer") als Fallback
        let resp = app
            .oneshot(req(
                "POST",
                &format!("{BASE}/raid/requirements"),
                r#"{"streamer":"DragScope"}"#,
                Some("tok"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let j = json_body(resp).await;
        assert_eq!(j["ok"], true);
        assert_eq!(j["login"], "dragscope");
    }

    #[tokio::test]
    async fn requirements_login_alias_twitch_login() {
        let app = make_router(Some(Arc::new(StubPort::ready())));
        let resp = app
            .oneshot(req(
                "POST",
                &format!("{BASE}/raid/requirements"),
                r#"{"twitch_login":"DragScope"}"#,
                Some("tok"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let j = json_body(resp).await;
        assert_eq!(j["login"], "dragscope");
    }

    #[tokio::test]
    async fn requirements_200_mit_message() {
        let app = make_router(Some(Arc::new(StubPort::ready())));
        let resp = app
            .oneshot(req(
                "POST",
                &format!("{BASE}/raid/requirements"),
                r#"{"login":"dragscope"}"#,
                Some("tok"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let j = json_body(resp).await;
        assert_eq!(j["ok"], true);
        assert!(j["message"].as_str().map(|m| !m.is_empty()).unwrap_or(false));
    }

    // ── oauth-callback ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn oauth_callback_200_mit_result() {
        let app = make_router(Some(Arc::new(StubPort::ready())));
        let resp = app
            .oneshot(req(
                "POST",
                &format!("{BASE}/raid/oauth-callback"),
                r#"{"code":"abc","state":"xyz","error":""}"#,
                Some("tok"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let j = json_body(resp).await;
        assert_eq!(j["status"], 200);
        assert!(j["title"].as_str().is_some());
        assert!(j["body_html"].as_str().is_some());
    }

    #[tokio::test]
    async fn oauth_callback_uninit_503() {
        let app = make_router(Some(Arc::new(StubPort::uninit())));
        let resp = app
            .oneshot(req(
                "POST",
                &format!("{BASE}/raid/oauth-callback"),
                r#"{"code":"","state":"","error":""}"#,
                Some("tok"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn oauth_callback_fehlende_felder_defaults_auf_leerstring() {
        // Python: str(body.get("code") or "") — d.h. fehlende Felder werden als "" behandelt
        let app = make_router(Some(Arc::new(StubPort::ready())));
        let resp = app
            .oneshot(req(
                "POST",
                &format!("{BASE}/raid/oauth-callback"),
                r#"{}"#,
                Some("tok"),
            ))
            .await
            .unwrap();
        // Kein 400 — leere Strings sind valide Eingaben für den Callback
        assert_eq!(resp.status(), StatusCode::OK);
    }

    // ── Helfer-Tests ─────────────────────────────────────────────────────────

    #[test]
    fn normalize_raid_target_regular_login() {
        assert_eq!(normalize_raid_auth_target("DragScope"), Some("dragscope".to_string()));
        // @ wird von normalize_twitch_login gestrippt
        assert_eq!(normalize_raid_auth_target("@DragScope"), Some("dragscope".to_string()));
    }

    #[test]
    fn normalize_raid_target_discord_prefix() {
        assert_eq!(
            normalize_raid_auth_target("discord:123456789"),
            Some("discord:123456789".to_string())
        );
        assert_eq!(normalize_raid_auth_target("discord:abc"), None);
        assert_eq!(normalize_raid_auth_target("discord:"), None);
    }

    #[test]
    fn normalize_raid_target_website_onboarding() {
        assert_eq!(
            normalize_raid_auth_target("public:website_onboarding"),
            Some("public:website_onboarding".to_string())
        );
        assert_eq!(
            normalize_raid_auth_target("PUBLIC:WEBSITE_ONBOARDING"),
            Some("public:website_onboarding".to_string())
        );
    }

    #[test]
    fn normalize_raid_target_leer_ergibt_none() {
        assert_eq!(normalize_raid_auth_target(""), None);
        assert_eq!(normalize_raid_auth_target("   "), None);
    }

    #[test]
    fn percent_decode_einfach() {
        assert_eq!(percent_decode("hello%20world"), "hello world");
        assert_eq!(percent_decode("no_encoding"), "no_encoding");
        assert_eq!(percent_decode("%2F"), "/");
    }

    #[test]
    fn normalize_discord_id_nur_ziffern() {
        assert_eq!(
            normalize_discord_user_id("123456", false).unwrap(),
            Some("123456".to_string())
        );
        assert!(normalize_discord_user_id("abc", false).is_err());
        assert_eq!(normalize_discord_user_id("", false).unwrap(), None);
        assert!(normalize_discord_user_id("", true).is_err());
    }

    // ── Discord-Scope-Guard ───────────────────────────────────────────────────

    #[tokio::test]
    async fn requirements_scope_guard_ablehnung_403() {
        // Port lehnt scope-Prüfung ab → 403 erwartet.
        let app = make_router(Some(Arc::new(ScopeGuardedPort)));
        let resp = app
            .oneshot(req(
                "POST",
                &format!("{BASE}/raid/requirements"),
                r#"{"login":"dragscope","guild_id":999}"#,
                Some("tok"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn oauth_callback_scope_guard_ablehnung_403() {
        let app = make_router(Some(Arc::new(ScopeGuardedPort)));
        let resp = app
            .oneshot(req(
                "POST",
                &format!("{BASE}/raid/oauth-callback"),
                r#"{"code":"abc","state":"xyz","error":"","guild_id":999}"#,
                Some("tok"),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }
}
