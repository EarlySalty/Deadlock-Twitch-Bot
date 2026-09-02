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
    std::env::var("KICK_REDIRECT_URI")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| KICK_DEFAULT_REDIRECT.to_string())
}

pub fn youtube_redirect_uri() -> String {
    std::env::var("YOUTUBE_UPLINK_REDIRECT_URI")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| YOUTUBE_DEFAULT_REDIRECT.to_string())
}

pub fn kick_konfiguriert() -> bool {
    KickOAuth::aus_umgebung().is_some()
}

pub fn youtube_konfiguriert() -> bool {
    GoogleOAuth::aus_umgebung().is_some()
}

fn pkce_verifier() -> String {
    tb_crypto::random_urlsafe_token(48)
}

fn pkce_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest)
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

async fn persist_connect_state(
    pool: &PgPool,
    platform: &str,
    streamer_id: i64,
    redirect_uri: &str,
    verifier: &str,
    state_token: &str,
    jetzt: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    let expires_at = jetzt + Duration::seconds(STATE_TTL_SECONDS);
    let lookup = tb_crypto::token_lookup_key(state_token);
    sqlx::query(
        "INSERT INTO oauth_state_tokens \
         (state_token, platform, streamer_login, redirect_uri, pkce_verifier, expires_at) \
         VALUES ($1, $2, $3, $4, $5, $6) \
         ON CONFLICT (state_token) DO UPDATE SET \
             platform = EXCLUDED.platform, \
             streamer_login = EXCLUDED.streamer_login, \
             redirect_uri = EXCLUDED.redirect_uri, \
             pkce_verifier = EXCLUDED.pkce_verifier, \
             expires_at = EXCLUDED.expires_at",
    )
    .bind(&lookup)
    .bind(platform)
    .bind(streamer_id.to_string())
    .bind(redirect_uri)
    .bind(verifier)
    .bind(expires_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub struct ConnectState {
    pub streamer_id: i64,
    pub redirect_uri: String,
    pub verifier: String,
}

async fn consume_connect_state(
    pool: &PgPool,
    platform: &str,
    state_token: &str,
    jetzt: DateTime<Utc>,
) -> Result<Option<ConnectState>, sqlx::Error> {
    let lookup = tb_crypto::token_lookup_key(state_token);
    let zeile: Option<(String, Option<String>, Option<String>)> = sqlx::query_as(
        "DELETE FROM oauth_state_tokens \
         WHERE state_token = $1 AND platform = $2 AND expires_at > $3 \
         RETURNING COALESCE(streamer_login, ''), redirect_uri, pkce_verifier",
    )
    .bind(&lookup)
    .bind(platform)
    .bind(jetzt)
    .fetch_optional(pool)
    .await?;
    Ok(zeile.and_then(|(login, redirect, verifier)| {
        let streamer_id = login.trim().parse::<i64>().ok()?;
        Some(ConnectState {
            streamer_id,
            redirect_uri: redirect.unwrap_or_default(),
            verifier: verifier.unwrap_or_default(),
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
    (StatusCode::SERVICE_UNAVAILABLE, Json(json!({ "error": text }))).into_response()
}

fn zurueck_zum_dashboard(query: &str) -> Response {
    Redirect::to(&format!("/twitch/uplink?{query}")).into_response()
}

pub async fn connect_kick_start_handler(
    State(pool): State<PgPool>,
    auth: DashboardAuthLevel,
) -> Response {
    let Some(_client) = KickOAuth::aus_umgebung() else {
        return nicht_eingerichtet("Kick ist auf dieser Instanz noch nicht eingerichtet");
    };
    let client_id = match super::plattform_oauth::non_empty_env("KICK_CLIENT_ID") {
        Some(id) => id,
        None => return nicht_eingerichtet("Kick ist auf dieser Instanz noch nicht eingerichtet"),
    };
    let id = match partner_id(&pool, &auth).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    let redirect_uri = kick_redirect_uri();
    let state_token = tb_crypto::random_urlsafe_token(32);
    let verifier = pkce_verifier();
    let challenge = pkce_challenge(&verifier);
    if let Err(error) =
        persist_connect_state(&pool, "kick", id, &redirect_uri, &verifier, &state_token, Utc::now())
            .await
    {
        tracing::warn!(%error, "kick-connect: State nicht speicherbar");
        return nicht_eingerichtet("Kick-Verbindung konnte nicht gestartet werden");
    }
    Redirect::to(&kick_authorize_url(&client_id, &redirect_uri, &state_token, &challenge))
        .into_response()
}

pub async fn connect_youtube_start_handler(
    State(pool): State<PgPool>,
    auth: DashboardAuthLevel,
) -> Response {
    let Some(_client) = GoogleOAuth::aus_umgebung() else {
        return nicht_eingerichtet("YouTube ist auf dieser Instanz noch nicht eingerichtet");
    };
    let Some(client_id) = google_client_id() else {
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
        "youtube",
        id,
        &redirect_uri,
        "",
        &state_token,
        Utc::now(),
    )
    .await
    {
        tracing::warn!(%error, "youtube-connect: State nicht speicherbar");
        return nicht_eingerichtet("YouTube-Verbindung konnte nicht gestartet werden");
    }
    Redirect::to(&youtube_authorize_url(&client_id, &redirect_uri, &state_token)).into_response()
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
) -> bool {
    if stream_key.trim().is_empty() || !ingest_url_ok(rtmp_url) {
        return false;
    }
    match relay.ziel_setzen(streamer_id, platform, rtmp_url, stream_key).await {
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
    store
        .upsert(&verbindung)
        .await
        .map_err(|e| format!("speichern: {e}"))?;

    let ziel_gesetzt = ziel_setzen_falls_moeglich(
        relay,
        state.streamer_id,
        "kick",
        &konto.rtmp_url,
        &konto.stream_key,
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
    store
        .upsert(&verbindung)
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
    let access_token = match platform_token_antwort(pool, config, streamer_id, platform, jetzt).await
    {
        Ok(antwort) => antwort.access_token,
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
    match relay
        .ziel_setzen(streamer_id, platform, &rtmp_url, &stream_key)
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
    let Some(client) = KickOAuth::aus_umgebung() else {
        return nicht_eingerichtet("Kick ist auf dieser Instanz noch nicht eingerichtet");
    };
    let state = match consume_connect_state(&pool, "kick", state_token, Utc::now()).await {
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
    let Some(client) = GoogleOAuth::aus_umgebung() else {
        return nicht_eingerichtet("YouTube ist auf dieser Instanz noch nicht eingerichtet");
    };
    let state = match consume_connect_state(&pool, "youtube", state_token, Utc::now()).await {
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
    KeineVerbindung,
    RelayFehler,
    SpeicherFehler,
}

pub async fn plattform_trennen(
    pool: &PgPool,
    config: &PlatformTokenConfig,
    relay: &dyn RelayZiele,
    kick: Option<&dyn KickApi>,
    youtube: Option<&dyn YouTubeApi>,
    streamer_id: i64,
    platform: &str,
) -> PlattformTrennenErgebnis {
    let store = PlatformConnectionStore::new(pool.clone(), config.cipher.clone());
    let verbindung = match store.load(streamer_id, platform).await {
        Ok(Some(v)) => v,
        Ok(None) => return PlattformTrennenErgebnis::KeineVerbindung,
        Err(error) => {
            tracing::warn!(streamer_id, platform, %error, "trennen: Verbindung nicht lesbar");
            return PlattformTrennenErgebnis::SpeicherFehler;
        }
    };
    if let Err(error) = relay.ziel_loeschen(streamer_id, platform).await {
        tracing::warn!(streamer_id, platform, %error, "trennen: Uplink-Ziel nicht entfernt");
        return PlattformTrennenErgebnis::RelayFehler;
    }
    match store.delete(streamer_id, platform).await {
        Ok(_) => {}
        Err(error) => {
            tracing::error!(streamer_id, platform, %error, "trennen: Zeile nicht loeschbar");
            return PlattformTrennenErgebnis::SpeicherFehler;
        }
    }
    let widerruf = match platform {
        "kick" => match kick {
            Some(c) => {
                if let Err(error) = c.event_subscriptions_loeschen(&verbindung.access_token).await {
                    tracing::warn!(streamer_id, platform, error = %fehlertext(error), "trennen: Kick-Abos nicht loeschbar");
                }
                c.revoke(&verbindung.access_token).await.err()
            }
            None => None,
        },
        "youtube" => match youtube {
            Some(c) => c.revoke(&verbindung.access_token).await.err(),
            None => None,
        },
        _ => None,
    };
    if let Some(error) = widerruf {
        tracing::warn!(streamer_id, platform, error = %fehlertext(error), "trennen: Widerruf fehlgeschlagen");
    }
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
        let erwartet = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(Sha256::digest(b"verifier"));
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

    async fn maybe_pool() -> Option<PgPool> {
        if std::env::var("TB_TEST_REQUIRE_DB").as_deref() != Ok("1") {
            return None;
        }
        let url = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let schema = crate::auth::session::test_schema_name("plattform_connect");
        let admin = PgPool::connect(&url).await.ok()?;
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .ok()?;
        admin.close().await;
        let opts: sqlx::postgres::PgConnectOptions = url.parse().ok()?;
        let opts = opts.options([("search_path", schema.as_str())]);
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(2)
            .connect_with(opts)
            .await
            .ok()?;
        sqlx::query(
            "CREATE TABLE oauth_state_tokens (state_token TEXT PRIMARY KEY, platform TEXT, \
             streamer_login TEXT, redirect_uri TEXT, pkce_verifier TEXT, expires_at TIMESTAMPTZ, \
             consumed_at TIMESTAMPTZ)",
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
        Some(pool)
    }

    macro_rules! pool_oder_ende {
        () => {
            match maybe_pool().await {
                Some(p) => p,
                None => {
                    assert!(
                        std::env::var("TB_TEST_REQUIRE_DB").as_deref() != Ok("1"),
                        "TB_TEST_REQUIRE_DB=1, aber keine Test-DB erreichbar"
                    );
                    return;
                }
            }
        };
    }

    #[tokio::test]
    async fn connect_state_persist_und_consume() {
        let pool = pool_oder_ende!();
        let jetzt = zeit("2026-09-02T10:00:00Z");
        persist_connect_state(&pool, "kick", 9001, "https://x.test/callback/kick", "verf", "stt", jetzt)
            .await
            .unwrap();

        assert!(
            consume_connect_state(&pool, "youtube", "stt", jetzt).await.unwrap().is_none(),
            "falsche Plattform konsumiert nicht"
        );
        let state = consume_connect_state(&pool, "kick", "stt", jetzt)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(state.streamer_id, 9001);
        assert_eq!(state.redirect_uri, "https://x.test/callback/kick");
        assert_eq!(state.verifier, "verf");
        assert!(
            consume_connect_state(&pool, "kick", "stt", jetzt).await.unwrap().is_none(),
            "zweimal konsumieren geht nicht"
        );
    }

    #[tokio::test]
    async fn abgelaufener_state_wird_nicht_konsumiert() {
        let pool = pool_oder_ende!();
        let jetzt = zeit("2026-09-02T10:00:00Z");
        persist_connect_state(&pool, "kick", 9002, "https://x.test/callback/kick", "v", "alt", jetzt)
            .await
            .unwrap();
        let spaeter = jetzt + Duration::seconds(STATE_TTL_SECONDS + 1);
        assert!(consume_connect_state(&pool, "kick", "alt", spaeter)
            .await
            .unwrap()
            .is_none());
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
        ) -> Result<(), String> {
            self.gesetzt.lock().unwrap().push((
                streamer_id,
                platform.to_string(),
                rtmp_url.to_string(),
                stream_key.to_string(),
            ));
            Ok(())
        }
        async fn ziel_loeschen(&self, _streamer_id: i64, _platform: &str) -> Result<bool, String> {
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
        let pool = pool_oder_ende!();
        let config = config();
        let relay = FakeRelay {
            gesetzt: std::sync::Mutex::new(Vec::new()),
        };
        let jetzt = zeit("2026-09-02T10:00:00Z");
        let state = ConnectState {
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
        assert!(ingest_url_ok("rtmps://fa723.global-contribute.live-video.net"));
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
        ) -> Result<(), String> {
            Err("relay HTTP 502".into())
        }
        async fn ziel_loeschen(&self, _streamer_id: i64, _platform: &str) -> Result<bool, String> {
            Ok(false)
        }
    }

    #[tokio::test]
    async fn kick_callback_kern_ohne_key_meldet_ziel_offen() {
        let pool = pool_oder_ende!();
        let config = config();
        let relay = FakeRelay {
            gesetzt: std::sync::Mutex::new(Vec::new()),
        };
        let jetzt = zeit("2026-09-02T10:00:00Z");
        let state = ConnectState {
            streamer_id: 9110,
            redirect_uri: "https://x.test/callback/kick".into(),
            verifier: "verf".into(),
        };
        let erg = kick_callback_kern(&pool, &config, &FakeKickKontoLeer, &relay, state, "code", jetzt)
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
        let pool = pool_oder_ende!();
        let config = config();
        let jetzt = zeit("2026-09-02T10:00:00Z");
        let state = ConnectState {
            streamer_id: 9111,
            redirect_uri: "https://x.test/callback/kick".into(),
            verifier: "verf".into(),
        };
        let erg = kick_callback_kern(&pool, &config, &FakeKick, &FakeRelayFehler, state, "code", jetzt)
            .await
            .unwrap();
        assert!(erg.ziel_offen);
        assert!(!erg.neu_verbinden);
    }

    #[tokio::test]
    async fn kick_callback_kern_ohne_refresh_token_meldet_neu_verbinden() {
        let pool = pool_oder_ende!();
        let config = config();
        let relay = FakeRelay {
            gesetzt: std::sync::Mutex::new(Vec::new()),
        };
        let jetzt = zeit("2026-09-02T10:00:00Z");
        let state = ConnectState {
            streamer_id: 9112,
            redirect_uri: "https://x.test/callback/kick".into(),
            verifier: "verf".into(),
        };
        let erg =
            kick_callback_kern(&pool, &config, &FakeKickOhneRefresh, &relay, state, "code", jetzt)
                .await
                .unwrap();
        assert!(erg.neu_verbinden);
        let store = PlatformConnectionStore::new(pool, config.cipher.clone());
        let gespeichert = store.load(9112, "kick").await.unwrap().unwrap();
        assert!(gespeichert.needs_reauth);
    }

    #[tokio::test]
    async fn streamkey_fuer_kick_setzt_ziel() {
        let pool = pool_oder_ende!();
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
}
