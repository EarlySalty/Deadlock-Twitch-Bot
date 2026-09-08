use axum::{
    extract::{Extension, Query, State},
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
    Json,
};
use base64::Engine;
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::PgPool;

use super::platform_store::{PlatformConnection, PlatformConnectionStore};
use super::platform_token::{platform_token_antwort, PlatformTokenConfig, TokenFehler};
use super::plattform_oauth::{
    google_client_id, GoogleOAuth, KickApi, KickOAuth, OAuthFehler, YouTubeApi, KICK_SCOPES,
    YOUTUBE_SCOPE,
};
use super::uplink::{partner_id, HttpRelayZiele, RelayZiele, StreamKeyStand};
use crate::auth::level::DashboardAuthLevel;

const STATE_TTL_SECONDS: i64 = 600;
const KICK_AUTHORIZE: &str = "https://id.kick.com/oauth/authorize";
const GOOGLE_AUTHORIZE: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const KICK_DEFAULT_REDIRECT: &str = "https://deutsche-deadlock-community.de/callback/kick";
const YOUTUBE_DEFAULT_REDIRECT: &str = "https://deutsche-deadlock-community.de/callback/youtube";

pub fn kick_redirect_uri() -> String {
    crate::uplink_config::runtime()
        .ok()
        .map(|runtime| runtime.kick_redirect_uri.clone())
        .unwrap_or_else(|| KICK_DEFAULT_REDIRECT.to_string())
}

pub fn youtube_redirect_uri() -> String {
    crate::uplink_config::runtime()
        .ok()
        .map(|runtime| runtime.youtube_redirect_uri.clone())
        .unwrap_or_else(|| YOUTUBE_DEFAULT_REDIRECT.to_string())
}

pub fn kick_konfiguriert() -> bool {
    KickOAuth::aus_konfiguration().is_some()
}

pub fn youtube_konfiguriert() -> bool {
    GoogleOAuth::aus_konfiguration().is_some()
}

fn pkce_verifier() -> String {
    tb_crypto::random_urlsafe_token(48)
}

fn pkce_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest)
}

const PKCE_ENC_PREFIX: &str = "enc:v1:";

fn pkce_aad(platform: &str, state_lookup_key: &str) -> String {
    format!("oauth_state_tokens|pkce_verifier|{platform}|{state_lookup_key}|1")
}

fn verifier_verschluesseln(
    cipher: &tb_crypto::FieldCipher,
    platform: &str,
    state_lookup_key: &str,
    verifier: &str,
) -> Result<String, String> {
    let blob = cipher
        .encrypt_field(verifier, &pkce_aad(platform, state_lookup_key))
        .map_err(|_| "pkce-verschluesselung".to_string())?;
    Ok(format!(
        "{PKCE_ENC_PREFIX}{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(blob)
    ))
}

fn verifier_entschluesseln(
    cipher: &tb_crypto::FieldCipher,
    platform: &str,
    state_lookup_key: &str,
    encoded: &str,
) -> Result<String, String> {
    let payload = encoded
        .strip_prefix(PKCE_ENC_PREFIX)
        .ok_or_else(|| "pkce-format".to_string())?;
    let blob = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| "pkce-base64".to_string())?;
    cipher
        .decrypt_field(&blob, &pkce_aad(platform, state_lookup_key))
        .map_err(|_| "pkce-entschluesselung".to_string())
}

pub fn kick_authorize_url(
    client_id: &str,
    redirect_uri: &str,
    state: &str,
    challenge: &str,
) -> String {
    url::Url::parse_with_params(
        KICK_AUTHORIZE,
        &[
            ("client_id", client_id),
            ("response_type", "code"),
            ("redirect_uri", redirect_uri),
            ("scope", &KICK_SCOPES.join(" ")),
            ("state", state),
            ("code_challenge", challenge),
            ("code_challenge_method", "S256"),
        ],
    )
    .expect("statische Kick-Authorize-URL ist parsebar")
    .to_string()
}

pub fn youtube_authorize_url(client_id: &str, redirect_uri: &str, state: &str) -> String {
    url::Url::parse_with_params(
        GOOGLE_AUTHORIZE,
        &[
            ("client_id", client_id),
            ("response_type", "code"),
            ("redirect_uri", redirect_uri),
            ("scope", YOUTUBE_SCOPE),
            ("state", state),
            ("access_type", "offline"),
            ("prompt", "consent"),
            ("include_granted_scopes", "true"),
        ],
    )
    .expect("statische Google-Authorize-URL ist parsebar")
    .to_string()
}

struct ConnectStateEingabe<'a> {
    platform: &'a str,
    streamer_id: i64,
    redirect_uri: &'a str,
    verifier: Option<&'a str>,
    state_token: &'a str,
}

async fn persist_connect_state(
    pool: &PgPool,
    cipher: &tb_crypto::FieldCipher,
    eingabe: ConnectStateEingabe<'_>,
    jetzt: DateTime<Utc>,
) -> Result<(), String> {
    let ConnectStateEingabe {
        platform,
        streamer_id,
        redirect_uri,
        verifier,
        state_token,
    } = eingabe;
    let expires_at = jetzt + Duration::seconds(STATE_TTL_SECONDS);
    let lookup = tb_crypto::token_lookup_key(state_token);
    let verifier_enc = verifier
        .map(|v| verifier_verschluesseln(cipher, platform, &lookup, v))
        .transpose()?;
    sqlx::query(
        "INSERT INTO oauth_state_tokens \
         (state_token, platform, streamer_login, redirect_uri, pkce_verifier, expires_at, created_at) \
         VALUES ($1, $2, $3, $4, $5, $6, clock_timestamp()) \
         ON CONFLICT (state_token) DO UPDATE SET \
             platform = EXCLUDED.platform, \
             streamer_login = EXCLUDED.streamer_login, \
             redirect_uri = EXCLUDED.redirect_uri, \
             pkce_verifier = EXCLUDED.pkce_verifier, \
             expires_at = EXCLUDED.expires_at, created_at = EXCLUDED.created_at",
    )
    .bind(&lookup)
    .bind(platform)
    .bind(streamer_id.to_string())
    .bind(redirect_uri)
    .bind(verifier_enc.as_deref())
    .bind(expires_at)
    .execute(pool)
    .await
    .map_err(|e| format!("state speichern: {e}"))?;
    Ok(())
}

pub struct ConnectState {
    pub created_at: DateTime<Utc>,
    pub streamer_id: i64,
    pub redirect_uri: String,
    pub verifier: String,
}

type ConnectStateRow = (String, Option<String>, Option<String>, DateTime<Utc>);

async fn consume_connect_state(
    pool: &PgPool,
    cipher: &tb_crypto::FieldCipher,
    platform: &str,
    state_token: &str,
    jetzt: DateTime<Utc>,
) -> Result<Option<ConnectState>, sqlx::Error> {
    let lookup = tb_crypto::token_lookup_key(state_token);
    let zeile: Option<ConnectStateRow> = sqlx::query_as(
        "DELETE FROM oauth_state_tokens \
         WHERE state_token = $1 AND platform = $2 AND expires_at > $3 \
         RETURNING COALESCE(streamer_login, ''), redirect_uri, pkce_verifier, created_at",
    )
    .bind(&lookup)
    .bind(platform)
    .bind(jetzt)
    .fetch_optional(pool)
    .await?;
    Ok(zeile.and_then(|(login, redirect, verifier, created_at)| {
        let streamer_id = login.trim().parse::<i64>().ok().filter(|id| *id > 0)?;
        let verifier = match verifier {
            Some(enc) => match verifier_entschluesseln(cipher, platform, &lookup, &enc) {
                Ok(klar) => klar,
                Err(fehler) => {
                    tracing::warn!(platform, %fehler, "connect-state: PKCE nicht entschluesselbar");
                    return None;
                }
            },
            None => String::new(),
        };
        Some(ConnectState {
            created_at,
            streamer_id,
            redirect_uri: redirect.unwrap_or_default(),
            verifier,
        })
    }))
}

#[derive(Deserialize)]
pub struct CallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
}

fn nicht_eingerichtet(text: &str) -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({ "error": text })),
    )
        .into_response()
}

fn zurueck_zum_dashboard(query: &str) -> Response {
    Redirect::to(&format!("/twitch/uplink?{query}")).into_response()
}

pub async fn connect_kick_start_handler(
    State(pool): State<PgPool>,
    config: Option<Extension<PlatformTokenConfig>>,
    auth: DashboardAuthLevel,
) -> Response {
    let Some(_client) = KickOAuth::aus_konfiguration() else {
        return nicht_eingerichtet("Kick ist auf dieser Instanz noch nicht eingerichtet");
    };
    let client_id = match super::plattform_oauth::non_empty_config("KICK_CLIENT_ID") {
        Some(id) => id,
        None => return nicht_eingerichtet("Kick ist auf dieser Instanz noch nicht eingerichtet"),
    };
    let Some(Extension(config)) = config else {
        return nicht_eingerichtet("Kick ist auf dieser Instanz noch nicht eingerichtet");
    };
    let id = match partner_id(&pool, &auth).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    let redirect_uri = kick_redirect_uri();
    let state_token = tb_crypto::random_urlsafe_token(32);
    let verifier = pkce_verifier();
    let challenge = pkce_challenge(&verifier);
    if let Err(error) = persist_connect_state(
        &pool,
        &config.cipher,
        ConnectStateEingabe {
            platform: "kick",
            streamer_id: id,
            redirect_uri: &redirect_uri,
            verifier: Some(&verifier),
            state_token: &state_token,
        },
        Utc::now(),
    )
    .await
    {
        tracing::warn!(%error, "kick-connect: State nicht speicherbar");
        return nicht_eingerichtet("Kick-Verbindung konnte nicht gestartet werden");
    }
    Redirect::to(&kick_authorize_url(
        &client_id,
        &redirect_uri,
        &state_token,
        &challenge,
    ))
    .into_response()
}

pub async fn connect_youtube_start_handler(
    State(pool): State<PgPool>,
    config: Option<Extension<PlatformTokenConfig>>,
    auth: DashboardAuthLevel,
) -> Response {
    let Some(_client) = GoogleOAuth::aus_konfiguration() else {
        return nicht_eingerichtet("YouTube ist auf dieser Instanz noch nicht eingerichtet");
    };
    let Some(client_id) = google_client_id() else {
        return nicht_eingerichtet("YouTube ist auf dieser Instanz noch nicht eingerichtet");
    };
    let Some(Extension(config)) = config else {
        return nicht_eingerichtet("YouTube ist auf dieser Instanz noch nicht eingerichtet");
    };
    let id = match partner_id(&pool, &auth).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    let redirect_uri = youtube_redirect_uri();
    let state_token = tb_crypto::random_urlsafe_token(32);
    if let Err(error) = persist_connect_state(
        &pool,
        &config.cipher,
        ConnectStateEingabe {
            platform: "youtube",
            streamer_id: id,
            redirect_uri: &redirect_uri,
            verifier: None,
            state_token: &state_token,
        },
        Utc::now(),
    )
    .await
    {
        tracing::warn!(%error, "youtube-connect: State nicht speicherbar");
        return nicht_eingerichtet("YouTube-Verbindung konnte nicht gestartet werden");
    }
    Redirect::to(&youtube_authorize_url(
        &client_id,
        &redirect_uri,
        &state_token,
    ))
    .into_response()
}

fn oauth_expires_at(jetzt: DateTime<Utc>, expires_in: i64) -> DateTime<Utc> {
    jetzt + Duration::seconds(expires_in.max(0))
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct KonnektErgebnis {
    pub neu_verbinden: bool,
    pub ziel_offen: bool,
}

pub fn ingest_url_ok(rtmp_url: &str) -> bool {
    let getrimmt = rtmp_url.trim();
    if !(getrimmt.starts_with("rtmp://") || getrimmt.starts_with("rtmps://")) {
        return false;
    }
    url::Url::parse(getrimmt)
        .ok()
        .and_then(|u| u.host_str().map(|h| !h.is_empty()))
        .unwrap_or(false)
}

async fn ziel_setzen_falls_moeglich(
    relay: &dyn RelayZiele,
    streamer_id: i64,
    platform: &str,
    rtmp_url: &str,
    stream_key: &str,
    generation: i64,
) -> bool {
    if stream_key.trim().is_empty() || !ingest_url_ok(rtmp_url) {
        return false;
    }
    match relay
        .ziel_setzen(streamer_id, platform, rtmp_url, stream_key, generation)
        .await
    {
        Ok(()) => true,
        Err(error) => {
            tracing::warn!(streamer_id, platform, %error, "connect: Uplink-Ziel nicht gesetzt");
            false
        }
    }
}

pub async fn kick_callback_kern(
    pool: &PgPool,
    config: &PlatformTokenConfig,
    client: &dyn KickApi,
    relay: &dyn RelayZiele,
    state: ConnectState,
    code: &str,
    jetzt: DateTime<Utc>,
) -> Result<KonnektErgebnis, String> {
    let token = client
        .exchange_code(code, &state.redirect_uri, &state.verifier)
        .await
        .map_err(|e| format!("code-tausch: {}", fehlertext(e)))?;
    let konto = client
        .konto(&token.access_token)
        .await
        .map_err(|e| format!("konto: {}", fehlertext(e)))?;

    let neu_verbinden = token.refresh_token.is_none();
    let verbindung = PlatformConnection {
        streamer_id: state.streamer_id,
        platform: "kick".to_string(),
        platform_user_id: konto.user_id.clone(),
        platform_login: konto.slug,
        access_token: token.access_token,
        refresh_token: token.refresh_token.unwrap_or_default(),
        scopes: token.scopes,
        expires_at: oauth_expires_at(jetzt, token.expires_in),
        needs_reauth: neu_verbinden,
    };
    let store = PlatformConnectionStore::new(pool.clone(), config.cipher.clone());
    let generation = store
        .upsert_callback(&verbindung, state.created_at)
        .await
        .map_err(|e| format!("speichern: {e}"))?;

    let ziel_gesetzt = ziel_setzen_falls_moeglich(
        relay,
        state.streamer_id,
        "kick",
        &konto.rtmp_url,
        &konto.stream_key,
        generation,
    )
    .await;
    Ok(KonnektErgebnis {
        neu_verbinden,
        ziel_offen: !ziel_gesetzt,
    })
}

pub async fn youtube_callback_kern(
    pool: &PgPool,
    config: &PlatformTokenConfig,
    client: &dyn YouTubeApi,
    relay: &dyn RelayZiele,
    state: ConnectState,
    code: &str,
    jetzt: DateTime<Utc>,
) -> Result<KonnektErgebnis, String> {
    let token = client
        .exchange_code(code, &state.redirect_uri)
        .await
        .map_err(|e| format!("code-tausch: {}", fehlertext(e)))?;
    let konto = client
        .konto(&token.access_token)
        .await
        .map_err(|e| format!("konto: {}", fehlertext(e)))?;

    let neu_verbinden = token.refresh_token.is_none();
    let verbindung = PlatformConnection {
        streamer_id: state.streamer_id,
        platform: "youtube".to_string(),
        platform_user_id: konto.channel_id.clone(),
        platform_login: konto.titel,
        access_token: token.access_token.clone(),
        refresh_token: token.refresh_token.unwrap_or_default(),
        scopes: token.scopes,
        expires_at: oauth_expires_at(jetzt, token.expires_in),
        needs_reauth: neu_verbinden,
    };
    let store = PlatformConnectionStore::new(pool.clone(), config.cipher.clone());
    let generation = store
        .upsert_callback(&verbindung, state.created_at)
        .await
        .map_err(|e| format!("speichern: {e}"))?;

    let ziel_gesetzt = match client.ziel(&verbindung.access_token).await {
        Ok(Some(ziel)) => {
            ziel_setzen_falls_moeglich(
                relay,
                state.streamer_id,
                "youtube",
                &ziel.rtmp_url,
                &ziel.stream_key,
                generation,
            )
            .await
        }
        Ok(None) => false,
        Err(error) => {
            tracing::warn!(streamer_id = state.streamer_id, error = %fehlertext(error), "youtube-connect: Stream-Ziel nicht abrufbar");
            false
        }
    };
    Ok(KonnektErgebnis {
        neu_verbinden,
        ziel_offen: !ziel_gesetzt,
    })
}

fn connect_query(platform: &str, erg: &KonnektErgebnis) -> String {
    let mut query = format!("verbunden={platform}");
    if erg.neu_verbinden {
        query.push_str("&neu_verbinden=1");
    }
    if erg.ziel_offen {
        query.push_str("&ziel_offen=1");
    }
    query
}

pub async fn plattform_stream_key_hinterlegen(
    pool: &PgPool,
    config: &PlatformTokenConfig,
    relay: &dyn RelayZiele,
    streamer_id: i64,
    platform: &str,
    jetzt: DateTime<Utc>,
) -> StreamKeyStand {
    let (access_token, generation) =
        match platform_token_antwort(pool, config, streamer_id, platform, jetzt).await {
            Ok(antwort) => (antwort.access_token, antwort.connection_generation),
            Err(TokenFehler::KeineVerbindung) | Err(TokenFehler::NeuVerbinden) => {
                return StreamKeyStand::KeineVerbindung
            }
            Err(TokenFehler::NichtLieferbar) => return StreamKeyStand::Fehlgeschlagen,
        };
    let ziel = match platform {
        "kick" => {
            let Some(client) = config.kick.as_ref() else {
                return StreamKeyStand::KeineVerbindung;
            };
            match client.konto(&access_token).await {
                Ok(konto) => Some((konto.rtmp_url, konto.stream_key)),
                Err(error) => {
                    tracing::warn!(streamer_id, platform, error = %fehlertext(error), "streamkey: Konto nicht abrufbar");
                    return StreamKeyStand::Fehlgeschlagen;
                }
            }
        }
        "youtube" => {
            let Some(client) = config.youtube.as_ref() else {
                return StreamKeyStand::KeineVerbindung;
            };
            match client.ziel(&access_token).await {
                Ok(Some(ziel)) => Some((ziel.rtmp_url, ziel.stream_key)),
                Ok(None) => None,
                Err(error) => {
                    tracing::warn!(streamer_id, platform, error = %fehlertext(error), "streamkey: Stream-Ziel nicht abrufbar");
                    return StreamKeyStand::Fehlgeschlagen;
                }
            }
        }
        _ => return StreamKeyStand::KeineVerbindung,
    };
    let Some((rtmp_url, stream_key)) = ziel else {
        return StreamKeyStand::Fehlgeschlagen;
    };
    if stream_key.trim().is_empty() || !ingest_url_ok(&rtmp_url) {
        return StreamKeyStand::Fehlgeschlagen;
    }
    if !matches!(
        tb_raid::target_generation::check_current(
            pool,
            &streamer_id.to_string(),
            platform,
            generation
        )
        .await,
        Ok(true)
    ) {
        return StreamKeyStand::KeineVerbindung;
    }
    match relay
        .ziel_setzen(streamer_id, platform, &rtmp_url, &stream_key, generation)
        .await
    {
        Ok(()) => StreamKeyStand::Hinterlegt,
        Err(error) => {
            tracing::warn!(streamer_id, platform, %error, "streamkey: Uplink-Ziel nicht gespeichert");
            StreamKeyStand::Fehlgeschlagen
        }
    }
}

fn fehlertext(fehler: OAuthFehler) -> String {
    match fehler {
        OAuthFehler::InvalidGrant => "invalid_grant".into(),
        OAuthFehler::Other(text) => text,
    }
}

pub async fn callback_kick_handler(
    State(pool): State<PgPool>,
    config: Option<Extension<PlatformTokenConfig>>,
    Query(query): Query<CallbackQuery>,
) -> Response {
    if let Some(fehler) = query.error.as_deref().filter(|s| !s.trim().is_empty()) {
        tracing::warn!(fehler, "kick-callback: Anbieter meldet Fehler");
        return zurueck_zum_dashboard("verbinden_fehler=kick");
    }
    let (Some(code), Some(state_token)) = (query.code.as_deref(), query.state.as_deref()) else {
        return zurueck_zum_dashboard("verbinden_fehler=kick");
    };
    let Some(Extension(config)) = config else {
        return nicht_eingerichtet("Kick ist auf dieser Instanz noch nicht eingerichtet");
    };
    let Some(client) = KickOAuth::aus_konfiguration() else {
        return nicht_eingerichtet("Kick ist auf dieser Instanz noch nicht eingerichtet");
    };
    let state =
        match consume_connect_state(&pool, &config.cipher, "kick", state_token, Utc::now()).await {
            Ok(Some(s)) => s,
            Ok(None) => return zurueck_zum_dashboard("verbinden_fehler=kick"),
            Err(error) => {
                tracing::warn!(%error, "kick-callback: State nicht lesbar");
                return zurueck_zum_dashboard("verbinden_fehler=kick");
            }
        };
    match kick_callback_kern(
        &pool,
        &config,
        &client,
        &HttpRelayZiele::default(),
        state,
        code,
        Utc::now(),
    )
    .await
    {
        Ok(erg) => zurueck_zum_dashboard(&connect_query("kick", &erg)),
        Err(error) => {
            tracing::warn!(%error, "kick-callback: Verbinden fehlgeschlagen");
            zurueck_zum_dashboard("verbinden_fehler=kick")
        }
    }
}

pub async fn callback_youtube_handler(
    State(pool): State<PgPool>,
    config: Option<Extension<PlatformTokenConfig>>,
    Query(query): Query<CallbackQuery>,
) -> Response {
    if let Some(fehler) = query.error.as_deref().filter(|s| !s.trim().is_empty()) {
        tracing::warn!(fehler, "youtube-callback: Anbieter meldet Fehler");
        return zurueck_zum_dashboard("verbinden_fehler=youtube");
    }
    let (Some(code), Some(state_token)) = (query.code.as_deref(), query.state.as_deref()) else {
        return zurueck_zum_dashboard("verbinden_fehler=youtube");
    };
    let Some(Extension(config)) = config else {
        return nicht_eingerichtet("YouTube ist auf dieser Instanz noch nicht eingerichtet");
    };
    let Some(client) = GoogleOAuth::aus_konfiguration() else {
        return nicht_eingerichtet("YouTube ist auf dieser Instanz noch nicht eingerichtet");
    };
    let state = match consume_connect_state(
        &pool,
        &config.cipher,
        "youtube",
        state_token,
        Utc::now(),
    )
    .await
    {
        Ok(Some(s)) => s,
        Ok(None) => return zurueck_zum_dashboard("verbinden_fehler=youtube"),
        Err(error) => {
            tracing::warn!(%error, "youtube-callback: State nicht lesbar");
            return zurueck_zum_dashboard("verbinden_fehler=youtube");
        }
    };
    match youtube_callback_kern(
        &pool,
        &config,
        &client,
        &HttpRelayZiele::default(),
        state,
        code,
        Utc::now(),
    )
    .await
    {
        Ok(erg) => zurueck_zum_dashboard(&connect_query("youtube", &erg)),
        Err(error) => {
            tracing::warn!(%error, "youtube-callback: Verbinden fehlgeschlagen");
            zurueck_zum_dashboard("verbinden_fehler=youtube")
        }
    }
}

pub enum PlattformTrennenErgebnis {
    Getrennt,
    RelayFehler,
    SpeicherFehler,
}

pub async fn plattform_trennen(
    pool: &PgPool,
    config: &PlatformTokenConfig,
    relay: &dyn RelayZiele,
    _kick: Option<&dyn KickApi>,
    _youtube: Option<&dyn YouTubeApi>,
    streamer_id: i64,
    platform: &str,
) -> PlattformTrennenErgebnis {
    let generation = match tb_raid::target_generation::begin_disconnect(
        pool,
        &streamer_id.to_string(),
        platform,
    )
    .await
    {
        Ok(g) => g,
        Err(_) => return PlattformTrennenErgebnis::SpeicherFehler,
    };
    if !matches!(
        relay.ziel_loeschen(streamer_id, platform, generation).await,
        Ok(true)
    ) {
        return PlattformTrennenErgebnis::RelayFehler;
    }
    let store = PlatformConnectionStore::new(pool.clone(), config.cipher.clone());
    if !matches!(
        store.delete_fenced(streamer_id, platform, generation).await,
        Ok(true)
    ) {
        return PlattformTrennenErgebnis::SpeicherFehler;
    }
    // Kein verzögerter Widerruf eines inzwischen erneut verwendeten Grants.
    // Remote-Abonnements gehören zum generationstreuen Chat-Lifecycle.
    PlattformTrennenErgebnis::Getrennt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kick_authorize_url_traegt_pkce_und_scopes() {
        let url = kick_authorize_url(
            "cid",
            "https://deutsche-deadlock-community.de/callback/kick",
            "state123",
            "chal",
        );
        assert!(url.starts_with(KICK_AUTHORIZE));
        assert!(url.contains("code_challenge=chal"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("scope=user"));
        assert!(url.contains("streamkey"));
        assert!(url.contains("state=state123"));
    }

    #[test]
    fn youtube_authorize_url_traegt_offline_consent_und_scope() {
        let url = youtube_authorize_url(
            "cid",
            "https://deutsche-deadlock-community.de/callback/youtube",
            "state123",
        );
        assert!(url.starts_with(GOOGLE_AUTHORIZE));
        assert!(url.contains("access_type=offline"));
        assert!(url.contains("prompt=consent"));
        assert!(url.contains("include_granted_scopes=true"));
        assert!(url.contains("youtube.force-ssl"));
    }

    #[test]
    fn pkce_challenge_ist_base64url_sha256() {
        let challenge = pkce_challenge("verifier");
        let erwartet =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(b"verifier"));
        assert_eq!(challenge, erwartet);
        assert!(!challenge.contains('='));
    }

    #[test]
    fn redirect_defaults_liegen_auf_der_domain() {
        assert_eq!(
            KICK_DEFAULT_REDIRECT,
            "https://deutsche-deadlock-community.de/callback/kick"
        );
        assert_eq!(
            YOUTUBE_DEFAULT_REDIRECT,
            "https://deutsche-deadlock-community.de/callback/youtube"
        );
    }

    const TEST_KEY_HEX: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

    fn cipher() -> std::sync::Arc<tb_crypto::FieldCipher> {
        std::sync::Arc::new(tb_crypto::FieldCipher::from_hex_key(TEST_KEY_HEX, "v1").unwrap())
    }

    fn zeit(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    async fn test_pool() -> (PgPool, crate::test_postgres::TestPostgres) {
        let database = crate::test_postgres::TestPostgres::start().await;
        let pool = database.pool.clone();
        sqlx::raw_sql(include_str!(
            "../../../../migrations/20260908220000_uplink_target_generations.sql"
        ))
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE oauth_state_tokens (\
             state_token TEXT PRIMARY KEY \
                 CONSTRAINT oauth_state_tokens_state_token_sha256 \
                 CHECK (state_token ~ '^[0-9a-f]{64}$'), \
             platform TEXT, \
             streamer_login TEXT, redirect_uri TEXT, pkce_verifier TEXT, expires_at TIMESTAMPTZ, \
             consumed_at TIMESTAMPTZ, created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(), \
             CONSTRAINT oauth_state_tokens_pkce_encrypted CHECK (\
                 platform NOT IN ('tiktok', 'youtube') \
                 OR pkce_verifier IS NULL \
                 OR pkce_verifier ~ '^enc:v1:[A-Za-z0-9_-]+$'))",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE platform_connections (
                streamer_id BIGINT NOT NULL,
                platform TEXT NOT NULL,
                platform_user_id TEXT NOT NULL,
                platform_login TEXT NOT NULL,
                access_token_enc BYTEA NOT NULL,
                refresh_token_enc BYTEA NOT NULL,
                enc_kid TEXT NOT NULL DEFAULT 'v1',
                scopes TEXT[] NOT NULL DEFAULT '{}',
                expires_at TIMESTAMPTZ NOT NULL,
                needs_reauth BOOLEAN NOT NULL DEFAULT FALSE,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                PRIMARY KEY (streamer_id, platform)
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        (pool, database)
    }

    #[tokio::test]
    async fn connect_state_persist_und_consume() {
        let (pool, _database) = test_pool().await;
        let cipher = cipher();
        let jetzt = zeit("2026-09-02T10:00:00Z");
        persist_connect_state(
            &pool,
            &cipher,
            ConnectStateEingabe {
                platform: "kick",
                streamer_id: 9001,
                redirect_uri: "https://x.test/callback/kick",
                verifier: Some("verf"),
                state_token: "stt",
            },
            jetzt,
        )
        .await
        .unwrap();

        assert!(
            consume_connect_state(&pool, &cipher, "youtube", "stt", jetzt)
                .await
                .unwrap()
                .is_none(),
            "falsche Plattform konsumiert nicht"
        );
        let state = consume_connect_state(&pool, &cipher, "kick", "stt", jetzt)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(state.streamer_id, 9001);
        assert_eq!(state.redirect_uri, "https://x.test/callback/kick");
        assert_eq!(state.verifier, "verf");
        assert!(
            consume_connect_state(&pool, &cipher, "kick", "stt", jetzt)
                .await
                .unwrap()
                .is_none(),
            "zweimal konsumieren geht nicht"
        );
    }

    #[tokio::test]
    async fn kick_verifier_liegt_verschluesselt_in_der_zeile() {
        let (pool, _database) = test_pool().await;
        let cipher = cipher();
        let jetzt = zeit("2026-09-02T10:00:00Z");
        persist_connect_state(
            &pool,
            &cipher,
            ConnectStateEingabe {
                platform: "kick",
                streamer_id: 9003,
                redirect_uri: "https://x.test/callback/kick",
                verifier: Some("klartext-verifier"),
                state_token: "kstt",
            },
            jetzt,
        )
        .await
        .unwrap();
        let roh: Option<String> = sqlx::query_scalar(
            "SELECT pkce_verifier FROM oauth_state_tokens WHERE state_token = $1",
        )
        .bind(tb_crypto::token_lookup_key("kstt"))
        .fetch_one(&pool)
        .await
        .unwrap();
        let roh = roh.unwrap();
        assert!(
            roh.starts_with("enc:v1:"),
            "Verifier nicht verschluesselt: {roh}"
        );
        assert!(!roh.contains("klartext-verifier"));
    }

    #[tokio::test]
    async fn youtube_state_ohne_verifier_erfuellt_pkce_constraint() {
        let (pool, _database) = test_pool().await;
        let cipher = cipher();
        let jetzt = zeit("2026-09-02T10:00:00Z");
        persist_connect_state(
            &pool,
            &cipher,
            ConnectStateEingabe {
                platform: "youtube",
                streamer_id: 9004,
                redirect_uri: "https://x.test/callback/youtube",
                verifier: None,
                state_token: "ystt",
            },
            jetzt,
        )
        .await
        .unwrap();
        let state = consume_connect_state(&pool, &cipher, "youtube", "ystt", jetzt)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(state.streamer_id, 9004);
        assert_eq!(state.verifier, "");

        let klartext = sqlx::query(
            "INSERT INTO oauth_state_tokens \
             (state_token, platform, streamer_login, redirect_uri, pkce_verifier, expires_at, created_at) \
             VALUES ($1, 'youtube', '9004', 'https://x.test/callback/youtube', 'klartext', $2, clock_timestamp())",
        )
        .bind(tb_crypto::token_lookup_key("ystt2"))
        .bind(jetzt + Duration::seconds(STATE_TTL_SECONDS))
        .execute(&pool)
        .await;
        let error = klartext.expect_err("Klartext-Verifier muss abgelehnt werden");
        let database_error = error.as_database_error().expect("PostgreSQL-Fehler");
        assert_eq!(database_error.code().as_deref(), Some("23514"));
        assert_eq!(
            database_error.constraint(),
            Some("oauth_state_tokens_pkce_encrypted")
        );
    }

    #[tokio::test]
    async fn abgelaufener_state_wird_nicht_konsumiert() {
        let (pool, _database) = test_pool().await;
        let cipher = cipher();
        let jetzt = zeit("2026-09-02T10:00:00Z");
        persist_connect_state(
            &pool,
            &cipher,
            ConnectStateEingabe {
                platform: "kick",
                streamer_id: 9002,
                redirect_uri: "https://x.test/callback/kick",
                verifier: Some("v"),
                state_token: "alt",
            },
            jetzt,
        )
        .await
        .unwrap();
        let spaeter = jetzt + Duration::seconds(STATE_TTL_SECONDS + 1);
        assert!(
            consume_connect_state(&pool, &cipher, "kick", "alt", spaeter)
                .await
                .unwrap()
                .is_none()
        );
    }

    struct FakeKick;

    #[async_trait::async_trait]
    impl KickApi for FakeKick {
        async fn exchange_code(
            &self,
            _code: &str,
            _redirect_uri: &str,
            _verifier: &str,
        ) -> Result<super::super::plattform_oauth::OAuthToken, OAuthFehler> {
            Ok(super::super::plattform_oauth::OAuthToken {
                access_token: "kick-acc".into(),
                refresh_token: Some("kick-ref".into()),
                expires_in: 7200,
                scopes: vec!["chat:write".into()],
            })
        }
        async fn refresh(
            &self,
            _refresh_token: &str,
        ) -> Result<super::super::plattform_oauth::OAuthToken, OAuthFehler> {
            unreachable!()
        }
        async fn revoke(&self, _access_token: &str) -> Result<(), OAuthFehler> {
            Ok(())
        }
        async fn konto(
            &self,
            _access_token: &str,
        ) -> Result<super::super::plattform_oauth::KickKonto, OAuthFehler> {
            Ok(super::super::plattform_oauth::KickKonto {
                user_id: "4242".into(),
                slug: "streamerin".into(),
                rtmp_url: "rtmps://kick/app".into(),
                stream_key: "sk_geheim".into(),
            })
        }
        async fn event_subscriptions_loeschen(
            &self,
            _access_token: &str,
        ) -> Result<(), OAuthFehler> {
            Ok(())
        }
    }

    struct FakeRelay {
        gesetzt: std::sync::Mutex<Vec<(i64, String, String, String)>>,
    }

    #[async_trait::async_trait]
    impl RelayZiele for FakeRelay {
        async fn ziel_setzen(
            &self,
            streamer_id: i64,
            platform: &str,
            rtmp_url: &str,
            stream_key: &str,
            _generation: i64,
        ) -> Result<(), String> {
            self.gesetzt.lock().unwrap().push((
                streamer_id,
                platform.to_string(),
                rtmp_url.to_string(),
                stream_key.to_string(),
            ));
            Ok(())
        }
        async fn ziel_loeschen(
            &self,
            _streamer_id: i64,
            _platform: &str,
            _generation: i64,
        ) -> Result<bool, String> {
            Ok(true)
        }
    }

    fn config() -> PlatformTokenConfig {
        PlatformTokenConfig {
            cipher: cipher(),
            token_client: std::sync::Arc::new(StummerRefresh),
            kick: None,
            youtube: None,
        }
    }

    struct StummerRefresh;

    #[async_trait::async_trait]
    impl tb_raid::token_refresher::TwitchTokenClient for StummerRefresh {
        async fn refresh(
            &self,
            _refresh_token: &str,
        ) -> Result<tb_raid::token_refresher::TokenResponse, tb_raid::token_refresher::RefreshError>
        {
            unreachable!()
        }
        async fn exchange_code(
            &self,
            _code: &str,
        ) -> Result<tb_raid::token_refresher::TokenResponse, tb_raid::token_refresher::RefreshError>
        {
            unreachable!()
        }
        async fn token_owner(
            &self,
            _access_token: &str,
        ) -> Result<tb_raid::token_refresher::TokenOwnerInfo, tb_raid::token_refresher::RefreshError>
        {
            unreachable!()
        }
    }

    #[tokio::test]
    async fn kick_callback_kern_speichert_und_setzt_ziel() {
        let (pool, _database) = test_pool().await;
        let config = config();
        let relay = FakeRelay {
            gesetzt: std::sync::Mutex::new(Vec::new()),
        };
        let jetzt = zeit("2026-09-02T10:00:00Z");
        let state = ConnectState {
            created_at: Utc::now(),
            streamer_id: 9100,
            redirect_uri: "https://x.test/callback/kick".into(),
            verifier: "verf".into(),
        };
        kick_callback_kern(&pool, &config, &FakeKick, &relay, state, "code", jetzt)
            .await
            .unwrap();

        let store = PlatformConnectionStore::new(pool, config.cipher.clone());
        let gespeichert = store.load(9100, "kick").await.unwrap().unwrap();
        assert_eq!(gespeichert.access_token, "kick-acc");
        assert_eq!(gespeichert.platform_user_id, "4242");
        assert_eq!(gespeichert.platform_login, "streamerin");
        let gesetzt = relay.gesetzt.lock().unwrap();
        assert_eq!(gesetzt.len(), 1);
        assert_eq!(gesetzt[0].1, "kick");
        assert_eq!(gesetzt[0].3, "sk_geheim");
    }

    #[test]
    fn ingest_url_wird_geprueft() {
        assert!(ingest_url_ok("rtmp://live.twitch.tv/app"));
        assert!(ingest_url_ok(
            "rtmps://fa723.global-contribute.live-video.net"
        ));
        assert!(!ingest_url_ok(""));
        assert!(!ingest_url_ok("https://example.test/app"));
        assert!(!ingest_url_ok("rtmp://"));
        assert!(!ingest_url_ok("live.twitch.tv/app"));
    }

    struct FakeKickKontoLeer;

    #[async_trait::async_trait]
    impl KickApi for FakeKickKontoLeer {
        async fn exchange_code(
            &self,
            _code: &str,
            _redirect_uri: &str,
            _verifier: &str,
        ) -> Result<super::super::plattform_oauth::OAuthToken, OAuthFehler> {
            Ok(super::super::plattform_oauth::OAuthToken {
                access_token: "kick-acc".into(),
                refresh_token: Some("kick-ref".into()),
                expires_in: 7200,
                scopes: vec!["chat:write".into()],
            })
        }
        async fn refresh(
            &self,
            _refresh_token: &str,
        ) -> Result<super::super::plattform_oauth::OAuthToken, OAuthFehler> {
            unreachable!()
        }
        async fn revoke(&self, _access_token: &str) -> Result<(), OAuthFehler> {
            Ok(())
        }
        async fn konto(
            &self,
            _access_token: &str,
        ) -> Result<super::super::plattform_oauth::KickKonto, OAuthFehler> {
            Ok(super::super::plattform_oauth::KickKonto {
                user_id: "4242".into(),
                slug: "streamerin".into(),
                rtmp_url: String::new(),
                stream_key: String::new(),
            })
        }
        async fn event_subscriptions_loeschen(
            &self,
            _access_token: &str,
        ) -> Result<(), OAuthFehler> {
            Ok(())
        }
    }

    struct FakeKickOhneRefresh;

    #[async_trait::async_trait]
    impl KickApi for FakeKickOhneRefresh {
        async fn exchange_code(
            &self,
            _code: &str,
            _redirect_uri: &str,
            _verifier: &str,
        ) -> Result<super::super::plattform_oauth::OAuthToken, OAuthFehler> {
            Ok(super::super::plattform_oauth::OAuthToken {
                access_token: "kick-acc".into(),
                refresh_token: None,
                expires_in: 7200,
                scopes: vec!["chat:write".into()],
            })
        }
        async fn refresh(
            &self,
            _refresh_token: &str,
        ) -> Result<super::super::plattform_oauth::OAuthToken, OAuthFehler> {
            unreachable!()
        }
        async fn revoke(&self, _access_token: &str) -> Result<(), OAuthFehler> {
            Ok(())
        }
        async fn konto(
            &self,
            _access_token: &str,
        ) -> Result<super::super::plattform_oauth::KickKonto, OAuthFehler> {
            Ok(super::super::plattform_oauth::KickKonto {
                user_id: "4242".into(),
                slug: "streamerin".into(),
                rtmp_url: "rtmps://kick/app".into(),
                stream_key: "sk_geheim".into(),
            })
        }
        async fn event_subscriptions_loeschen(
            &self,
            _access_token: &str,
        ) -> Result<(), OAuthFehler> {
            Ok(())
        }
    }

    struct FakeRelayFehler;

    #[async_trait::async_trait]
    impl RelayZiele for FakeRelayFehler {
        async fn ziel_setzen(
            &self,
            _streamer_id: i64,
            _platform: &str,
            _rtmp_url: &str,
            _stream_key: &str,
            _generation: i64,
        ) -> Result<(), String> {
            Err("relay HTTP 502".into())
        }
        async fn ziel_loeschen(
            &self,
            _streamer_id: i64,
            _platform: &str,
            _generation: i64,
        ) -> Result<bool, String> {
            Ok(false)
        }
    }

    #[tokio::test]
    async fn kick_callback_kern_ohne_key_meldet_ziel_offen() {
        let (pool, _database) = test_pool().await;
        let config = config();
        let relay = FakeRelay {
            gesetzt: std::sync::Mutex::new(Vec::new()),
        };
        let jetzt = zeit("2026-09-02T10:00:00Z");
        let state = ConnectState {
            created_at: Utc::now(),
            streamer_id: 9110,
            redirect_uri: "https://x.test/callback/kick".into(),
            verifier: "verf".into(),
        };
        let erg = kick_callback_kern(
            &pool,
            &config,
            &FakeKickKontoLeer,
            &relay,
            state,
            "code",
            jetzt,
        )
        .await
        .unwrap();
        assert!(erg.ziel_offen);
        assert!(!erg.neu_verbinden);
        assert!(relay.gesetzt.lock().unwrap().is_empty());
        let store = PlatformConnectionStore::new(pool, config.cipher.clone());
        assert!(store.load(9110, "kick").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn kick_callback_kern_bei_relay_fehler_meldet_ziel_offen() {
        let (pool, _database) = test_pool().await;
        let config = config();
        let jetzt = zeit("2026-09-02T10:00:00Z");
        let state = ConnectState {
            created_at: Utc::now(),
            streamer_id: 9111,
            redirect_uri: "https://x.test/callback/kick".into(),
            verifier: "verf".into(),
        };
        let erg = kick_callback_kern(
            &pool,
            &config,
            &FakeKick,
            &FakeRelayFehler,
            state,
            "code",
            jetzt,
        )
        .await
        .unwrap();
        assert!(erg.ziel_offen);
        assert!(!erg.neu_verbinden);
    }

    #[tokio::test]
    async fn kick_callback_kern_ohne_refresh_token_meldet_neu_verbinden() {
        let (pool, _database) = test_pool().await;
        let config = config();
        let relay = FakeRelay {
            gesetzt: std::sync::Mutex::new(Vec::new()),
        };
        let jetzt = zeit("2026-09-02T10:00:00Z");
        let state = ConnectState {
            created_at: Utc::now(),
            streamer_id: 9112,
            redirect_uri: "https://x.test/callback/kick".into(),
            verifier: "verf".into(),
        };
        let erg = kick_callback_kern(
            &pool,
            &config,
            &FakeKickOhneRefresh,
            &relay,
            state,
            "code",
            jetzt,
        )
        .await
        .unwrap();
        assert!(erg.neu_verbinden);
        let store = PlatformConnectionStore::new(pool, config.cipher.clone());
        let gespeichert = store.load(9112, "kick").await.unwrap().unwrap();
        assert!(gespeichert.needs_reauth);
    }

    #[tokio::test]
    async fn streamkey_fuer_kick_setzt_ziel() {
        let (pool, _database) = test_pool().await;
        let mut config = config();
        config.kick = Some(std::sync::Arc::new(FakeKick) as std::sync::Arc<dyn KickApi>);
        let jetzt = zeit("2026-09-02T10:00:00Z");
        let store = PlatformConnectionStore::new(pool.clone(), config.cipher.clone());
        store
            .upsert(&PlatformConnection {
                streamer_id: 9120,
                platform: "kick".to_string(),
                platform_user_id: "4242".into(),
                platform_login: "streamerin".into(),
                access_token: "kick-acc".into(),
                refresh_token: "kick-ref".into(),
                scopes: vec!["chat:write".into()],
                expires_at: jetzt + Duration::hours(2),
                needs_reauth: false,
            })
            .await
            .unwrap();
        let relay = FakeRelay {
            gesetzt: std::sync::Mutex::new(Vec::new()),
        };
        let stand =
            plattform_stream_key_hinterlegen(&pool, &config, &relay, 9120, "kick", jetzt).await;
        assert_eq!(stand, StreamKeyStand::Hinterlegt);
        let gesetzt = relay.gesetzt.lock().unwrap();
        assert_eq!(gesetzt.len(), 1);
        assert_eq!(gesetzt[0].1, "kick");
        assert_eq!(gesetzt[0].3, "sk_geheim");
    }

    struct LoeschRelay {
        geloescht: std::sync::Mutex<Vec<(i64, String)>>,
    }

    #[async_trait::async_trait]
    impl RelayZiele for LoeschRelay {
        async fn ziel_setzen(
            &self,
            _streamer_id: i64,
            _platform: &str,
            _rtmp_url: &str,
            _stream_key: &str,
            _generation: i64,
        ) -> Result<(), String> {
            Ok(())
        }
        async fn ziel_loeschen(
            &self,
            streamer_id: i64,
            platform: &str,
            _generation: i64,
        ) -> Result<bool, String> {
            self.geloescht
                .lock()
                .unwrap()
                .push((streamer_id, platform.to_string()));
            Ok(true)
        }
    }

    #[tokio::test]
    async fn trennen_loescht_relay_ziel_auch_ohne_zeile() {
        let (pool, _database) = test_pool().await;
        let config = config();
        let relay = LoeschRelay {
            geloescht: std::sync::Mutex::new(Vec::new()),
        };
        let erg = plattform_trennen(&pool, &config, &relay, None, None, 9200, "kick").await;
        assert!(matches!(erg, PlattformTrennenErgebnis::Getrennt));
        let geloescht = relay.geloescht.lock().unwrap();
        assert_eq!(geloescht.len(), 1);
        assert_eq!(geloescht[0], (9200, "kick".to_string()));
    }
    struct PausedYouTube {
        entered: tokio::sync::Notify,
        resume: tokio::sync::Notify,
    }
    #[async_trait::async_trait]
    impl YouTubeApi for PausedYouTube {
        async fn exchange_code(
            &self,
            _: &str,
            _: &str,
        ) -> Result<super::super::plattform_oauth::OAuthToken, OAuthFehler> {
            Ok(super::super::plattform_oauth::OAuthToken {
                access_token: "synthetic-google".into(),
                refresh_token: Some("synthetic-refresh".into()),
                expires_in: 7200,
                scopes: vec!["https://www.googleapis.com/auth/youtube.force-ssl".into()],
            })
        }
        async fn refresh(
            &self,
            _: &str,
        ) -> Result<super::super::plattform_oauth::OAuthToken, OAuthFehler> {
            unreachable!()
        }
        async fn revoke(&self, _: &str) -> Result<(), OAuthFehler> {
            panic!("Uplink disconnect must not revoke a later shared grant")
        }
        async fn konto(
            &self,
            _: &str,
        ) -> Result<super::super::plattform_oauth::YouTubeKonto, OAuthFehler> {
            Ok(super::super::plattform_oauth::YouTubeKonto {
                channel_id: "UC-synthetic".into(),
                titel: "Synthetic".into(),
            })
        }
        async fn ziel(
            &self,
            _: &str,
        ) -> Result<Option<super::super::plattform_oauth::YouTubeZiel>, OAuthFehler> {
            self.entered.notify_one();
            self.resume.notified().await;
            Ok(Some(super::super::plattform_oauth::YouTubeZiel {
                rtmp_url: "rtmps://youtube.test/live".into(),
                stream_key: "synthetic-key".into(),
            }))
        }
    }
    #[derive(Default)]
    struct FencedRelay {
        state: std::sync::Mutex<(i64, bool, usize)>,
    }
    #[async_trait::async_trait]
    impl RelayZiele for FencedRelay {
        async fn ziel_setzen(
            &self,
            _: i64,
            _: &str,
            _: &str,
            _: &str,
            generation: i64,
        ) -> Result<(), String> {
            let mut s = self.state.lock().unwrap();
            if generation < s.0 || generation == s.0 && s.1 {
                return Err("relay HTTP 409".into());
            }
            *s = (generation, false, s.2 + 1);
            Ok(())
        }
        async fn ziel_loeschen(&self, _: i64, _: &str, generation: i64) -> Result<bool, String> {
            let mut s = self.state.lock().unwrap();
            if generation < s.0 {
                return Err("relay HTTP 409".into());
            }
            *s = (generation, true, s.2);
            Ok(true)
        }
    }
    #[tokio::test]
    async fn delayed_youtube_callback_cannot_restore_target_after_disconnect() {
        let (pool, _database) = test_pool().await;
        let config = config();
        let relay = FencedRelay::default();
        let client = PausedYouTube {
            entered: tokio::sync::Notify::new(),
            resume: tokio::sync::Notify::new(),
        };
        let state = ConnectState {
            streamer_id: 9201,
            redirect_uri: "https://local.test/callback".into(),
            verifier: String::new(),
            created_at: Utc::now(),
        };
        let callback = youtube_callback_kern(
            &pool,
            &config,
            &client,
            &relay,
            state,
            "synthetic-code",
            Utc::now(),
        );
        let disconnect = async {
            client.entered.notified().await;
            assert!(matches!(
                plattform_trennen(&pool, &config, &relay, None, Some(&client), 9201, "youtube")
                    .await,
                PlattformTrennenErgebnis::Getrennt
            ));
            client.resume.notify_one();
        };
        let (result, ()) = tokio::time::timeout(std::time::Duration::from_secs(10), async {
            tokio::join!(callback, disconnect)
        })
        .await
        .unwrap();
        assert!(result.unwrap().ziel_offen);
        assert_eq!(relay.state.lock().unwrap().2, 0);
        assert!(relay.state.lock().unwrap().1);
        let store = PlatformConnectionStore::new(pool.clone(), config.cipher.clone());
        assert!(store
            .load_for_uplink(9201, "youtube")
            .await
            .unwrap()
            .is_none());
        let state = tb_raid::target_generation::snapshot(&pool, "9201", "youtube")
            .await
            .unwrap();
        assert!(!state.enabled && !state.disconnect_pending);
    }
    #[tokio::test]
    async fn old_kick_callback_state_cannot_reactivate_after_disconnect() {
        let (pool, _database) = test_pool().await;
        let config = config();
        let relay = FencedRelay::default();
        let state = ConnectState {
            streamer_id: 9202,
            redirect_uri: "https://local.test/callback".into(),
            verifier: "synthetic-verifier".into(),
            created_at: Utc::now(),
        };
        assert!(matches!(
            plattform_trennen(&pool, &config, &relay, None, None, 9202, "kick").await,
            PlattformTrennenErgebnis::Getrennt
        ));
        assert!(kick_callback_kern(
            &pool,
            &config,
            &FakeKick,
            &relay,
            state,
            "synthetic-code",
            Utc::now()
        )
        .await
        .is_err());
        assert_eq!(relay.state.lock().unwrap().2, 0);
        assert!(
            PlatformConnectionStore::new(pool.clone(), config.cipher.clone())
                .load_for_uplink(9202, "kick")
                .await
                .unwrap()
                .is_none()
        );
    }
}
