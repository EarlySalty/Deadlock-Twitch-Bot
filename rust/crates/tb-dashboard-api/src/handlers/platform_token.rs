//! Die interne Token-Route fuer rs-relay:
//! `GET /twitch/api/v2/internal/platform-token?streamer=&platform=`.
//!
//! Das Relay braucht einen gueltigen Access-Token des Streamers, um dessen
//! Chat zu lesen und in seinem Namen zu antworten. Es bekommt genau das und
//! sonst nichts: kein Refresh-Token, kein Cookie, kein Umweg ueber den
//! Browser. Nur Loopback plus `X-Internal-Token`.
//!
//! Gelesen wird `twitch_raid_auth` — derselbe Speicher, in den der
//! Streamer-OAuth ohnehin schreibt. Vorher lag daneben eine zweite Tabelle mit
//! einem zweiten Grant, einem zweiten Refresh-Job und einer zweiten
//! Verschluesselung. Zwei Token-Staende fuer dasselbe Konto heisst: einer ist
//! irgendwann der falsche. Es gibt jetzt einen.
//!
//! Erneuert wird ueber denselben Schreibpfad wie beim Raid-Bot
//! ([`tb_raid::token_refresher::RaidTokenRefresher`]), inklusive
//! Advisory-Lock. Ein zweiter Refresh-Job wuerde sich mit dem ersten um
//! denselben Refresh-Token streiten, und Twitch rotiert ihn bei jeder Nutzung.

// Axum-Responses direkt im Result, wie in uplink.rs.
#![allow(clippy::result_large_err)]

use std::{net::SocketAddr, sync::Arc};

use axum::{
    extract::{ConnectInfo, Extension, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;
use tb_crypto::FieldCipher;
use tb_http_core::{ExpectedToken, INTERNAL_TOKEN_HEADER};
use tb_raid::{
    token_refresher::{
        RaidTokenRefresher, RefreshError, TokenOwnerInfo, TokenResponse, TwitchTokenClient,
    },
    token_store::RaidAuthStore,
    TokenBlacklistStore,
};
use tb_transport_twitch::{user_token::UserTokenError, HelixClient, HelixConfig};

use crate::auth::security::require_internal;

/// Einzige Plattform mit fertigem Verbinden-Weg. Andere Namen kommen ueber
/// dieselbe Route und werden sauber abgewiesen.
pub const PLATFORM_TWITCH: &str = "twitch";

/// Ohne dieses Recht kann das Relay den Chat gar nicht lesen. Ein Grant ohne
/// es ist fuer diese Route wertlos, also gibt es ihn auch nicht heraus.
pub const CHAT_LESE_SCOPE: &str = "user:read:chat";

/// Ab dieser Restlaufzeit wird beim Abruf vorab erneuert. Das Relay haelt
/// EventSub-Verbindungen ueber Stunden; ein Token, das waehrenddessen
/// ablaeuft, reisst sie ab.
pub const REFRESH_VORLAUF: Duration = Duration::minutes(10);

// ───────────────────────────────────────────────────────────────────────────
// Konfiguration
// ───────────────────────────────────────────────────────────────────────────

/// Was die Route zur Laufzeit braucht (als Extension). Fehlt sie, antwortet
/// die Route mit 503 statt zu raten.
#[derive(Clone)]
pub struct PlatformTokenConfig {
    pub cipher: Arc<FieldCipher>,
    pub token_client: Arc<dyn TwitchTokenClient>,
    pub kick: Option<Arc<dyn super::plattform_oauth::KickApi>>,
    pub youtube: Option<Arc<dyn super::plattform_oauth::YouTubeApi>>,
}

fn non_empty_env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// Baut die Config aus der Prozessumgebung (Infisical-geladen). Braucht
/// `TWITCH_CLIENT_ID`, `TWITCH_CLIENT_SECRET` und den Feldschluessel
/// `DB_MASTER_KEY_V1`. Fehlt eines, bleibt die Route mit 503 zu. Secrets
/// werden nicht geloggt.
pub fn platform_token_config_from_env() -> Option<PlatformTokenConfig> {
    let client_id = non_empty_env("TWITCH_CLIENT_ID")?;
    let client_secret = non_empty_env("TWITCH_CLIENT_SECRET")?;
    let cipher = match FieldCipher::from_env() {
        Ok(c) => Arc::new(c),
        Err(e) => {
            tracing::warn!(error = %e, "platform_token: Feldschluessel fehlt, Route bleibt zu");
            return None;
        }
    };
    let helix = match HelixClient::new(HelixConfig::new(&client_id, &client_secret)) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(error = %e, "platform_token: Helix-Client nicht baubar");
            return None;
        }
    };
    let kick = super::plattform_oauth::KickOAuth::aus_umgebung()
        .map(|c| Arc::new(c) as Arc<dyn super::plattform_oauth::KickApi>);
    let youtube = super::plattform_oauth::GoogleOAuth::aus_umgebung()
        .map(|c| Arc::new(c) as Arc<dyn super::plattform_oauth::YouTubeApi>);
    Some(PlatformTokenConfig {
        cipher,
        token_client: Arc::new(HelixRefreshClient { helix }),
        kick,
        youtube,
    })
}

/// Der Refresh-Weg zu Twitch. `exchange_code` und `token_owner` gehoeren zum
/// Callback und laufen im Bot-Prozess; hier sind sie unerreichbar und sagen
/// das auch, statt still etwas Falsches zu tun.
pub struct HelixRefreshClient {
    helix: HelixClient,
}

fn map_token_error(error: UserTokenError) -> RefreshError {
    match error {
        UserTokenError::InvalidClient => RefreshError::InvalidClient,
        UserTokenError::InvalidGrant => RefreshError::InvalidGrant,
        UserTokenError::Other(message) => RefreshError::Other(message),
    }
}

#[async_trait::async_trait]
impl TwitchTokenClient for HelixRefreshClient {
    async fn refresh(&self, refresh_token: &str) -> Result<TokenResponse, RefreshError> {
        self.helix
            .refresh_user_token(refresh_token)
            .await
            .map(|r| TokenResponse {
                access_token: r.access_token,
                refresh_token: r.refresh_token,
                expires_in: r.expires_in,
                scopes: r.scope,
            })
            .map_err(map_token_error)
    }

    async fn exchange_code(&self, _code: &str) -> Result<TokenResponse, RefreshError> {
        Err(RefreshError::Other(
            "Code-Tausch laeuft im Bot-Prozess, nicht im Dashboard".into(),
        ))
    }

    async fn token_owner(&self, access_token: &str) -> Result<TokenOwnerInfo, RefreshError> {
        let owner = self
            .helix
            .fetch_token_owner(access_token)
            .await
            .map_err(map_token_error)?;
        Ok(TokenOwnerInfo {
            twitch_user_id: owner.id,
            twitch_login: owner.login,
        })
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Antwort
// ───────────────────────────────────────────────────────────────────────────

#[derive(Debug, Default, Deserialize)]
pub struct PlatformTokenQuery {
    pub streamer: Option<i64>,
    pub platform: Option<String>,
}

/// Was das Relay bekommt. Bewusst ein eigener Typ ohne `refresh_token`: die
/// Serialisierung kann ihn gar nicht mitschicken (Contract REQ-7).
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct PlatformTokenAntwort {
    pub access_token: String,
    pub expires_at: DateTime<Utc>,
    pub platform_user_id: String,
    pub platform_login: String,
    pub scopes: Vec<String>,
}

/// Warum kein Token herausgeht.
#[derive(Debug, PartialEq, Eq)]
pub enum TokenFehler {
    /// Keine Zeile, kein lesbarer Token, oder der Grant traegt das Chat-Recht
    /// nicht. Fuer das Relay ist das derselbe Fall: es gibt nichts zu holen.
    KeineVerbindung,
    /// Der Streamer muss neu durch den Twitch-Dialog.
    NeuVerbinden,
    /// Der Refresh ist gescheitert; ein spaeterer Versuch kann klappen.
    NichtLieferbar,
}

/// Ob ein gespeicherter Token erneuert werden soll.
///
/// Eigene Funktion, damit die Frist ohne Datenbank und ohne Uhr pruefbar ist.
/// Eine fehlende Ablaufzeit gilt als faellig: unbekannt ist keine Zusage.
pub fn refresh_faellig(expires_at: Option<DateTime<Utc>>, jetzt: DateTime<Utc>) -> bool {
    match expires_at {
        None => true,
        Some(exp) => jetzt + REFRESH_VORLAUF >= exp,
    }
}

/// Ob ein Token wirklich tot ist.
///
/// Die Trennung von [`refresh_faellig`] ist kein Feinschliff, sondern der
/// Unterschied zwischen einem gelieferten Token und einer 502. Der Vorlauf
/// hier betraegt zehn Minuten, der Refresher haelt einen Token unter seinem
/// eigenen Lock aber schon ab fuenf Minuten Restlaufzeit fuer frisch genug und
/// gibt ihn unveraendert zurueck (`token_refresher.rs`, Puffer 300 s). Wer
/// nach dem Refresh erneut auf `refresh_faellig` prueft, lehnt in genau diesem
/// Fenster einen voellig brauchbaren Token ab, und zwar bei jedem Versuch, bis
/// die Restlaufzeit unter fuenf Minuten faellt. Nach dem Refresh zaehlt
/// deshalb nur noch, ob der Token abgelaufen ist.
fn abgelaufen(expires_at: Option<DateTime<Utc>>, jetzt: DateTime<Utc>) -> bool {
    match expires_at {
        None => true,
        Some(exp) => jetzt >= exp,
    }
}

/// Ob dieser Grant fuer das Relay taugt.
pub fn taugt_fuer_chat(scopes: &[String]) -> bool {
    scopes.iter().any(|s| s.trim() == CHAT_LESE_SCOPE)
}

// ───────────────────────────────────────────────────────────────────────────
// Kern
// ───────────────────────────────────────────────────────────────────────────

/// Ein gueltiger Twitch-Token des Streamers, bei Bedarf frisch geholt.
///
/// Der eine Weg zu einem brauchbaren Token, den jeder Aufrufer nimmt: die
/// interne Route fuer rs-relay genauso wie der Stream-Key-Nachlauf und der
/// Widerruf beim Trennen. Wer stattdessen die Zeile direkt liest, arbeitet
/// frueher oder spaeter mit einem abgelaufenen Token: `refresh_all_due` im Bot
/// fasst nur Zeilen mit `raid_enabled IS TRUE` an, ein Streamer ohne Raids
/// haette also nach wenigen Stunden nichts Gueltiges mehr, und die Aufrufer
/// haetten keinen Weg, es zu merken.
///
/// `scopes` kommt mit zurueck, weil jeder Aufrufer eine andere Frage daran
/// hat und ein zweiter Lesezugriff dieselbe Zeile noch einmal anfassen wuerde.
pub async fn gueltiger_twitch_token(
    pool: &PgPool,
    config: &PlatformTokenConfig,
    streamer_id: i64,
    jetzt: DateTime<Utc>,
) -> Result<(tb_raid::token_store::RaidTokens, Vec<String>), TokenFehler> {
    let uid = streamer_id.to_string();
    let store = RaidAuthStore::new(pool.clone(), config.cipher.clone());

    // `load_decrypted_unrestricted` und nicht `load_decrypted`: das
    // `raid_enabled`-Gate gehoert zum Raid-Bot. Wer Raids abgeschaltet hat,
    // darf trotzdem seinen eigenen Chat im Dock sehen.
    let tokens = match store.load_decrypted_unrestricted(&uid).await {
        Ok(Some(t)) => t,
        Ok(None) => return Err(TokenFehler::KeineVerbindung),
        Err(e) => {
            tracing::warn!(streamer_id, error = %e, "platform_token: Zeile nicht lesbar");
            return Err(TokenFehler::NichtLieferbar);
        }
    };
    if tokens.needs_reauth {
        return Err(TokenFehler::NeuVerbinden);
    }
    let scopes = match store.get_scopes(&uid).await {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(streamer_id, error = %e, "platform_token: Scopes nicht lesbar");
            return Err(TokenFehler::NichtLieferbar);
        }
    };
    if !refresh_faellig(tokens.token_expires_at, jetzt) {
        return Ok((tokens, scopes));
    }

    // Derselbe Schreibpfad wie beim Raid-Bot: Advisory-Lock, Re-Read unterm
    // Lock, verschluesseltes Zurueckschreiben. Der Refresher meldet keinen
    // frischen Token zurueck, deshalb wird danach neu gelesen.
    let refresher = RaidTokenRefresher::new(
        pool.clone(),
        config.cipher.clone(),
        config.token_client.clone(),
        Arc::new(TokenBlacklistStore::new(pool.clone())),
    );
    let ausgang = refresher
        .refresh_and_store(
            &uid,
            &tokens.twitch_login,
            tokens.refresh_token.as_deref().unwrap_or(""),
            jetzt,
        )
        .await;
    match ausgang {
        Ok(tb_raid::token_refresher::RefreshOutcome::Blacklisted) => {
            return Err(TokenFehler::NeuVerbinden)
        }
        Ok(_) => {}
        Err(e) => {
            tracing::warn!(streamer_id, error = %e, "platform_token: Refresh fehlgeschlagen");
            return Err(TokenFehler::NichtLieferbar);
        }
    }

    let frisch = match store.load_decrypted_unrestricted(&uid).await {
        Ok(Some(t)) => t,
        Ok(None) => return Err(TokenFehler::KeineVerbindung),
        Err(e) => {
            tracing::warn!(streamer_id, error = %e, "platform_token: Zeile nach Refresh nicht lesbar");
            return Err(TokenFehler::NichtLieferbar);
        }
    };
    if frisch.needs_reauth {
        return Err(TokenFehler::NeuVerbinden);
    }
    // Scopes nach dem Refresh neu lesen: haette er den Satz veraendert, bekaeme
    // das Relay sonst den alten und legte Subscriptions an, fuer die kein
    // Recht mehr vorliegt.
    let scopes = store.get_scopes(&uid).await.unwrap_or(scopes);
    // Ein Token, das auch nach dem Refresh abgelaufen ist, ist keiner. Lieber
    // eine ehrliche Absage als ein Token, mit dem der Aufrufer ins Leere laeuft.
    if abgelaufen(frisch.token_expires_at, jetzt) {
        return Err(TokenFehler::NichtLieferbar);
    }
    Ok((frisch, scopes))
}

pub async fn plattform_refresh_all_due(
    pool: &PgPool,
    config: &PlatformTokenConfig,
    jetzt: DateTime<Utc>,
) -> Result<usize, super::platform_store::StoreFehler> {
    let store =
        super::platform_store::PlatformConnectionStore::new(pool.clone(), config.cipher.clone());
    let faellige = store.faellige(REFRESH_VORLAUF, jetzt).await?;
    let mut erneuert = 0usize;
    for (streamer_id, platform) in faellige {
        if platform != "kick" && platform != "youtube" {
            continue;
        }
        if plattform_refresh(&store, config, streamer_id, &platform, jetzt)
            .await
            .is_ok()
        {
            erneuert += 1;
        }
    }
    Ok(erneuert)
}

/// Der Kern der Route: Auth ist schon geprueft.
///
/// Verzweigt nach Plattform. Twitch liegt in `twitch_raid_auth`, weil der
/// Streamer dort ohnehin autorisiert. Kick, YouTube und TikTok haben keinen
/// Raid-Bot, an dessen Grant sich etwas anhaengen liesse; fuer sie bleibt
/// `platform_connections` der Speicher. Solange dort niemand verbunden ist,
/// antwortet dieser Zweig mit 404, und genau das ist die richtige Antwort.
pub async fn platform_token_antwort(
    pool: &PgPool,
    config: &PlatformTokenConfig,
    streamer_id: i64,
    platform: &str,
    jetzt: DateTime<Utc>,
) -> Result<PlatformTokenAntwort, TokenFehler> {
    if platform != PLATFORM_TWITCH {
        return fremde_plattform_antwort(pool, config, streamer_id, platform, jetzt).await;
    }
    let (tokens, scopes) = gueltiger_twitch_token(pool, config, streamer_id, jetzt).await?;
    // Ohne Chat-Lese-Recht kann das Relay den Chat gar nicht abonnieren. Der
    // Grant ist fuer diese Route wertlos, also gibt es ihn auch nicht heraus.
    // Bewusst NACH dem Token-Weg: ein Streamer mit altem Raid-Grant soll
    // dieselbe 404 bekommen wie einer ohne Zeile, und der Refresh haelt seine
    // Raid-Tokens dabei nebenbei frisch.
    if !taugt_fuer_chat(&scopes) {
        return Err(TokenFehler::KeineVerbindung);
    }
    Ok(PlatformTokenAntwort {
        access_token: tokens.access_token,
        expires_at: tokens.token_expires_at.unwrap_or(jetzt),
        platform_user_id: streamer_id.to_string(),
        platform_login: tokens.twitch_login,
        scopes,
    })
}

fn plattform_client_konfiguriert(config: &PlatformTokenConfig, platform: &str) -> bool {
    match platform {
        "kick" => config.kick.is_some(),
        "youtube" => config.youtube.is_some(),
        _ => false,
    }
}

async fn fremde_plattform_antwort(
    pool: &PgPool,
    config: &PlatformTokenConfig,
    streamer_id: i64,
    platform: &str,
    jetzt: DateTime<Utc>,
) -> Result<PlatformTokenAntwort, TokenFehler> {
    let store =
        super::platform_store::PlatformConnectionStore::new(pool.clone(), config.cipher.clone());
    let verbindung = match store.load(streamer_id, platform).await {
        Ok(Some(v)) => v,
        Ok(None) => return Err(TokenFehler::KeineVerbindung),
        Err(e) => {
            tracing::warn!(streamer_id, platform, error = %e, "platform_token: Verbindung nicht lesbar");
            return Err(TokenFehler::NichtLieferbar);
        }
    };
    if verbindung.needs_reauth {
        return Err(TokenFehler::NeuVerbinden);
    }

    if refresh_faellig(Some(verbindung.expires_at), jetzt) {
        if !plattform_client_konfiguriert(config, platform) {
            if abgelaufen(Some(verbindung.expires_at), jetzt) {
                return Err(TokenFehler::NeuVerbinden);
            }
            return Ok(PlatformTokenAntwort {
                access_token: verbindung.access_token,
                expires_at: verbindung.expires_at,
                platform_user_id: verbindung.platform_user_id,
                platform_login: verbindung.platform_login,
                scopes: verbindung.scopes,
            });
        }
        plattform_refresh(&store, config, streamer_id, platform, jetzt).await?;
        let frisch = match store.load(streamer_id, platform).await {
            Ok(Some(v)) => v,
            Ok(None) => return Err(TokenFehler::KeineVerbindung),
            Err(e) => {
                tracing::warn!(streamer_id, platform, error = %e, "platform_token: Verbindung nach Refresh nicht lesbar");
                return Err(TokenFehler::NichtLieferbar);
            }
        };
        if frisch.needs_reauth {
            return Err(TokenFehler::NeuVerbinden);
        }
        if abgelaufen(Some(frisch.expires_at), jetzt) {
            return Err(TokenFehler::NichtLieferbar);
        }
        return Ok(PlatformTokenAntwort {
            access_token: frisch.access_token,
            expires_at: frisch.expires_at,
            platform_user_id: frisch.platform_user_id,
            platform_login: frisch.platform_login,
            scopes: frisch.scopes,
        });
    }

    Ok(PlatformTokenAntwort {
        access_token: verbindung.access_token,
        expires_at: verbindung.expires_at,
        platform_user_id: verbindung.platform_user_id,
        platform_login: verbindung.platform_login,
        scopes: verbindung.scopes,
    })
}

pub async fn plattform_refresh(
    store: &super::platform_store::PlatformConnectionStore,
    config: &PlatformTokenConfig,
    streamer_id: i64,
    platform: &str,
    jetzt: DateTime<Utc>,
) -> Result<(), TokenFehler> {
    use super::platform_store::{NeuerToken, RefreshAbbruch, RefreshAusgang};
    use super::plattform_oauth::{OAuthFehler, OAuthToken};

    fn in_neuer_token(token: OAuthToken, jetzt: DateTime<Utc>) -> NeuerToken {
        NeuerToken {
            access_token: token.access_token,
            refresh_token: token.refresh_token,
            expires_at: jetzt + Duration::seconds(token.expires_in.max(0)),
            scopes: token.scopes,
        }
    }
    fn in_abbruch(fehler: OAuthFehler) -> RefreshAbbruch {
        match fehler {
            OAuthFehler::InvalidGrant => RefreshAbbruch::NeuAnmeldung,
            OAuthFehler::Other(_) => RefreshAbbruch::Fehler,
        }
    }

    let ausgang = match platform {
        "kick" => {
            let Some(client) = config.kick.clone() else {
                return Ok(());
            };
            store
                .refresh_and_store(streamer_id, platform, REFRESH_VORLAUF, jetzt, |rt| async move {
                    client
                        .refresh(&rt)
                        .await
                        .map(|t| in_neuer_token(t, jetzt))
                        .map_err(in_abbruch)
                })
                .await
        }
        "youtube" => {
            let Some(client) = config.youtube.clone() else {
                return Ok(());
            };
            store
                .refresh_and_store(streamer_id, platform, REFRESH_VORLAUF, jetzt, |rt| async move {
                    client
                        .refresh(&rt)
                        .await
                        .map(|t| in_neuer_token(t, jetzt))
                        .map_err(in_abbruch)
                })
                .await
        }
        _ => return Ok(()),
    };
    match ausgang {
        Ok(RefreshAusgang::Erneuert) | Ok(RefreshAusgang::NichtNoetig) => Ok(()),
        Ok(RefreshAusgang::NeuAnmeldungNoetig) => Err(TokenFehler::NeuVerbinden),
        Ok(RefreshAusgang::Fehlgeschlagen) => Err(TokenFehler::NichtLieferbar),
        Err(e) => {
            tracing::warn!(streamer_id, platform, error = %e, "platform_token: Plattform-Refresh fehlgeschlagen");
            Err(TokenFehler::NichtLieferbar)
        }
    }
}

fn fehler_antwort(fehler: TokenFehler) -> Response {
    match fehler {
        TokenFehler::KeineVerbindung => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "keine_verbindung" })),
        )
            .into_response(),
        TokenFehler::NeuVerbinden => (
            StatusCode::CONFLICT,
            Json(json!({ "error": "needs_reauth" })),
        )
            .into_response(),
        TokenFehler::NichtLieferbar => (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": "token_nicht_lieferbar" })),
        )
            .into_response(),
    }
}

/// Loopback plus `X-Internal-Token`, konstante Laufzeit, fail-closed.
fn intern_erlaubt(
    connect: Option<&ConnectInfo<SocketAddr>>,
    headers: &HeaderMap,
    expected: Option<&ExpectedToken>,
) -> bool {
    let loopback = connect.map(|c| c.0.ip().is_loopback()).unwrap_or(false);
    let presented = headers
        .get(INTERNAL_TOKEN_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .trim();
    let expected = expected.map(|e| e.0.trim()).unwrap_or("");
    require_internal(loopback, presented, expected)
}

/// `GET /twitch/api/v2/internal/platform-token?streamer=&platform=`.
pub async fn internal_platform_token_handler(
    State(pool): State<PgPool>,
    connect: Option<ConnectInfo<SocketAddr>>,
    expected: Option<Extension<ExpectedToken>>,
    config: Option<Extension<PlatformTokenConfig>>,
    headers: HeaderMap,
    Query(query): Query<PlatformTokenQuery>,
) -> Response {
    if !intern_erlaubt(connect.as_ref(), &headers, expected.as_ref().map(|e| &e.0)) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "unauthorized" })),
        )
            .into_response();
    }
    let Some(Extension(config)) = config else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "nicht_konfiguriert" })),
        )
            .into_response();
    };
    let (Some(streamer_id), Some(platform)) = (query.streamer, query.platform.as_deref()) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "streamer und platform fehlen" })),
        )
            .into_response();
    };
    let platform = platform.trim().to_lowercase();
    match platform_token_antwort(&pool, &config, streamer_id, &platform, Utc::now()).await {
        Ok(antwort) => Json(antwort).into_response(),
        Err(fehler) => fehler_antwort(fehler),
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Tests
// ───────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use std::sync::Mutex;
    use tb_crypto::aad;

    /// Test-Feldschluessel (32 Byte Hex). Kein Produktionswert.
    const TEST_KEY_HEX: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

    fn cipher() -> Arc<FieldCipher> {
        Arc::new(FieldCipher::from_hex_key(TEST_KEY_HEX, "v1").unwrap())
    }

    fn zeit(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    /// Refresh-Attrappe: merkt sich die Aufrufe und liefert ein festes Paar.
    struct FakeTokenClient {
        ergebnis: Mutex<Result<TokenResponse, RefreshError>>,
        aufrufe: Mutex<Vec<String>>,
    }

    impl FakeTokenClient {
        fn neu() -> Self {
            Self {
                ergebnis: Mutex::new(Ok(TokenResponse {
                    access_token: "acc-frisch".into(),
                    refresh_token: "ref-frisch".into(),
                    expires_in: 14000,
                    scopes: Vec::new(),
                })),
                aufrufe: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait::async_trait]
    impl TwitchTokenClient for FakeTokenClient {
        async fn refresh(&self, refresh_token: &str) -> Result<TokenResponse, RefreshError> {
            self.aufrufe.lock().unwrap().push(refresh_token.to_string());
            match &*self.ergebnis.lock().unwrap() {
                Ok(r) => Ok(r.clone()),
                Err(e) => Err(e.clone()),
            }
        }
        async fn exchange_code(&self, _code: &str) -> Result<TokenResponse, RefreshError> {
            unreachable!("kein Code-Tausch in diesem Modul")
        }
        async fn token_owner(&self, _t: &str) -> Result<TokenOwnerInfo, RefreshError> {
            unreachable!("kein Owner-Lookup in diesem Modul")
        }
    }

    // ── ohne DB ────────────────────────────────────────────────────────────

    #[test]
    fn ohne_ablaufzeit_gilt_der_token_als_faellig() {
        let jetzt = zeit("2026-08-28T10:00:00Z");
        assert!(refresh_faellig(None, jetzt));
        // Genau auf der Frist: faellig, nicht knapp daneben.
        assert!(refresh_faellig(Some(jetzt + Duration::minutes(10)), jetzt));
        assert!(refresh_faellig(Some(jetzt + Duration::minutes(9)), jetzt));
        assert!(!refresh_faellig(Some(jetzt + Duration::minutes(11)), jetzt));
    }

    #[test]
    fn ohne_chat_lese_recht_taugt_der_grant_nicht() {
        let voll: Vec<String> = tb_raid::scope_profiles::UPLINK_SCOPES
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert!(taugt_fuer_chat(&voll));
        let raid: Vec<String> = tb_raid::scope_profiles::FULL_STREAMER_SCOPES
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert!(!taugt_fuer_chat(&raid));
        assert!(!taugt_fuer_chat(&[]));
    }

    #[tokio::test]
    async fn ohne_token_401() {
        // Weder Loopback noch Header: fail-closed.
        let headers = HeaderMap::new();
        assert!(!intern_erlaubt(None, &headers, None));
        let erwartet = ExpectedToken("geheim".into());
        assert!(!intern_erlaubt(None, &headers, Some(&erwartet)));
        // Loopback allein reicht nicht.
        let connect = ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 40000)));
        assert!(!intern_erlaubt(Some(&connect), &headers, Some(&erwartet)));
        // Falscher Token reicht auch nicht.
        let mut falsch = HeaderMap::new();
        falsch.insert(INTERNAL_TOKEN_HEADER, "daneben".parse().unwrap());
        assert!(!intern_erlaubt(Some(&connect), &falsch, Some(&erwartet)));
        // Richtig plus Loopback: durch.
        let mut richtig = HeaderMap::new();
        richtig.insert(INTERNAL_TOKEN_HEADER, "geheim".parse().unwrap());
        assert!(intern_erlaubt(Some(&connect), &richtig, Some(&erwartet)));
    }

    #[tokio::test]
    async fn fremder_peer_401() {
        // Richtiger Token, aber nicht von der Maschine: abgewiesen. Sonst
        // reichte ein geleakter Header aus dem Netz.
        let erwartet = ExpectedToken("geheim".into());
        let mut headers = HeaderMap::new();
        headers.insert(INTERNAL_TOKEN_HEADER, "geheim".parse().unwrap());
        let fremd = ConnectInfo(SocketAddr::from(([10, 0, 0, 7], 40000)));
        assert!(!intern_erlaubt(Some(&fremd), &headers, Some(&erwartet)));
    }

    // ── mit DB ─────────────────────────────────────────────────────────────

    /// Eigenes Testschema mit den zwei Tabellen, die dieser Weg anfasst.
    /// Spaltentypen wie in `fresh_schema_snapshot.txt`, damit ein Test nicht
    /// gruen wird, den die Produktionstabelle ablehnen wuerde.
    async fn maybe_pool() -> Option<PgPool> {
        if std::env::var("TB_TEST_REQUIRE_DB").as_deref() != Ok("1") {
            return None;
        }
        let url = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let schema = crate::auth::session::test_schema_name("platform_token");
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
            "CREATE TABLE twitch_raid_auth (
                twitch_user_id TEXT NOT NULL PRIMARY KEY,
                twitch_login TEXT NOT NULL,
                access_token TEXT DEFAULT 'ENC',
                refresh_token TEXT DEFAULT 'ENC',
                token_expires_at TIMESTAMPTZ NOT NULL,
                scopes TEXT NOT NULL,
                authorized_at TIMESTAMPTZ DEFAULT NOW(),
                last_refreshed_at TIMESTAMPTZ,
                raid_enabled BOOLEAN DEFAULT TRUE,
                created_at TIMESTAMPTZ DEFAULT NOW(),
                needs_reauth BOOLEAN DEFAULT FALSE,
                reauth_notified_at TIMESTAMPTZ,
                access_token_enc BYTEA,
                refresh_token_enc BYTEA,
                enc_version INTEGER DEFAULT 1,
                enc_kid TEXT DEFAULT 'v1',
                enc_migrated_at TIMESTAMPTZ
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        // Die plattformneutrale Tabelle: Spalten wie in der Migration.
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
        sqlx::query(
            "CREATE TABLE twitch_token_blacklist (
                twitch_user_id TEXT NOT NULL,
                twitch_login TEXT NOT NULL,
                error_message TEXT,
                error_count INTEGER DEFAULT 1,
                first_error_at TEXT NOT NULL,
                last_error_at TEXT NOT NULL,
                notified INTEGER DEFAULT 0,
                grace_expires_at TEXT,
                user_dm_sent INTEGER DEFAULT 0,
                reminder_sent INTEGER DEFAULT 0,
                role_removed INTEGER DEFAULT 0
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

    /// Legt eine Zeile in `twitch_raid_auth` an, verschluesselt wie der
    /// AuthWriter es tut.
    async fn zeile_anlegen(
        pool: &PgPool,
        cipher: &FieldCipher,
        uid: &str,
        login: &str,
        scopes: &[&str],
        expires_at: Option<DateTime<Utc>>,
        needs_reauth: bool,
    ) {
        let access = cipher
            .encrypt_field("acc-alt", &aad::raid_auth("access_token", uid, 1))
            .unwrap();
        let refresh = cipher
            .encrypt_field("ref-alt", &aad::raid_auth("refresh_token", uid, 1))
            .unwrap();
        sqlx::query(
            "INSERT INTO twitch_raid_auth \
             (twitch_user_id, twitch_login, access_token, refresh_token, \
              access_token_enc, refresh_token_enc, enc_version, enc_kid, \
              token_expires_at, scopes, authorized_at, raid_enabled, needs_reauth) \
             VALUES ($1, $2, 'ENC', 'ENC', $3, $4, 1, 'v1', $5, $6, NOW(), FALSE, $7) \
             ON CONFLICT (twitch_user_id) DO UPDATE SET \
                 access_token_enc = EXCLUDED.access_token_enc, \
                 refresh_token_enc = EXCLUDED.refresh_token_enc, \
                 token_expires_at = EXCLUDED.token_expires_at, \
                 scopes = EXCLUDED.scopes, \
                 needs_reauth = EXCLUDED.needs_reauth",
        )
        .bind(uid)
        .bind(login)
        .bind(&access)
        .bind(&refresh)
        .bind(expires_at)
        .bind(scopes.join(" "))
        .bind(needs_reauth)
        .execute(pool)
        .await
        .unwrap();
    }

    fn config_mit(client: Arc<dyn TwitchTokenClient>) -> PlatformTokenConfig {
        PlatformTokenConfig {
            cipher: cipher(),
            token_client: client,
            kick: None,
            youtube: None,
        }
    }

    const UPLINK: &[&str] = &[
        "channel:manage:raids",
        "channel:manage:moderators",
        "channel:bot",
        "clips:edit",
        "channel:read:ads",
        "bits:read",
        "channel:read:redemptions",
        "channel:read:subscriptions",
        "channel:read:hype_train",
        "channel:manage:broadcast",
        "user:read:chat",
        "user:write:chat",
        "channel:read:stream_key",
        "moderator:read:followers",
        "channel:manage:redemptions",
    ];

    #[tokio::test]
    async fn liefert_access_token_ohne_refresh_token() {
        let pool = pool_oder_ende!();
        let config = config_mit(Arc::new(FakeTokenClient::neu()));
        let jetzt = zeit("2026-08-28T10:00:00Z");
        zeile_anlegen(
            &pool,
            &config.cipher,
            "5101",
            "streamerin",
            UPLINK,
            Some(jetzt + Duration::hours(3)),
            false,
        )
        .await;

        let antwort = platform_token_antwort(&pool, &config, 5101, "twitch", jetzt)
            .await
            .expect("Token muss kommen");
        assert_eq!(antwort.access_token, "acc-alt");
        assert_eq!(antwort.platform_user_id, "5101");
        assert_eq!(antwort.platform_login, "streamerin");
        assert!(antwort.scopes.contains(&"user:read:chat".to_string()));

        // REQ-7: kein Refresh-Token, weder als Feld noch als Wert im JSON.
        let json = serde_json::to_value(&antwort).unwrap();
        assert!(json.get("refresh_token").is_none());
        assert!(!json.to_string().contains("ref-alt"));
    }

    #[tokio::test]
    async fn keine_zeile_ist_404() {
        let pool = pool_oder_ende!();
        let config = config_mit(Arc::new(FakeTokenClient::neu()));
        let jetzt = zeit("2026-08-28T10:00:00Z");
        assert_eq!(
            platform_token_antwort(&pool, &config, 5102, "twitch", jetzt).await,
            Err(TokenFehler::KeineVerbindung)
        );
        let antwort = fehler_antwort(TokenFehler::KeineVerbindung);
        assert_eq!(antwort.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn ohne_chat_scope_404() {
        let pool = pool_oder_ende!();
        let config = config_mit(Arc::new(FakeTokenClient::neu()));
        let jetzt = zeit("2026-08-28T10:00:00Z");
        // Ein reiner Raid-Grant: Tokens sind gueltig, taugen aber nicht.
        let raid: Vec<&str> = tb_raid::scope_profiles::FULL_STREAMER_SCOPES.to_vec();
        zeile_anlegen(
            &pool,
            &config.cipher,
            "5103",
            "raiderin",
            &raid,
            Some(jetzt + Duration::hours(3)),
            false,
        )
        .await;
        assert_eq!(
            platform_token_antwort(&pool, &config, 5103, "twitch", jetzt).await,
            Err(TokenFehler::KeineVerbindung)
        );
    }

    #[tokio::test]
    async fn needs_reauth_409() {
        let pool = pool_oder_ende!();
        let config = config_mit(Arc::new(FakeTokenClient::neu()));
        let jetzt = zeit("2026-08-28T10:00:00Z");
        zeile_anlegen(
            &pool,
            &config.cipher,
            "5104",
            "abgelaufen",
            UPLINK,
            Some(jetzt + Duration::hours(3)),
            true,
        )
        .await;
        assert_eq!(
            platform_token_antwort(&pool, &config, 5104, "twitch", jetzt).await,
            Err(TokenFehler::NeuVerbinden)
        );
        assert_eq!(
            fehler_antwort(TokenFehler::NeuVerbinden).status(),
            StatusCode::CONFLICT
        );
    }

    #[tokio::test]
    async fn abgelaufen_wird_ueber_refresher_erneuert() {
        let pool = pool_oder_ende!();
        let client = Arc::new(FakeTokenClient::neu());
        let config = config_mit(client.clone());
        let jetzt = zeit("2026-08-28T10:00:00Z");
        // Restlaufzeit unter dem Vorlauf: der Abruf muss erneuern.
        zeile_anlegen(
            &pool,
            &config.cipher,
            "5105",
            "knapp",
            UPLINK,
            Some(jetzt + Duration::minutes(2)),
            false,
        )
        .await;

        let antwort = platform_token_antwort(&pool, &config, 5105, "twitch", jetzt)
            .await
            .expect("Token muss kommen");
        // Der frische Token aus dem Refresh, nicht der alte aus der Zeile.
        assert_eq!(antwort.access_token, "acc-frisch");
        // Und der Refresher hat wirklich den gespeicherten Refresh-Token benutzt.
        assert_eq!(client.aufrufe.lock().unwrap().as_slice(), &["ref-alt"]);
    }

    /// Ein Twitch-Grant gilt nur fuer Twitch. Wuerde der Zweig fehlen, bekaeme
    /// rs-relay fuer `platform=kick` ein Twitch-Token und redete damit im
    /// falschen Chat.
    #[tokio::test]
    async fn fremde_plattform_bekommt_kein_twitch_token() {
        let pool = pool_oder_ende!();
        let config = config_mit(Arc::new(FakeTokenClient::neu()));
        let jetzt = zeit("2026-08-28T10:00:00Z");
        zeile_anlegen(
            &pool,
            &config.cipher,
            "5106",
            "streamerin",
            UPLINK,
            Some(jetzt + Duration::hours(3)),
            false,
        )
        .await;
        for fremd in ["kick", "youtube", "tiktok"] {
            assert_eq!(
                platform_token_antwort(&pool, &config, 5106, fremd, jetzt).await,
                Err(TokenFehler::KeineVerbindung),
                "{fremd}"
            );
        }
    }

    /// Der Fall, den die frueheren zehn Minuten Vorlauf gefressen haben: ein
    /// Token mit sieben Minuten Restlaufzeit galt als faellig, der Refresher
    /// hielt ihn unter seinem Lock fuer frisch genug und gab ihn unveraendert
    /// zurueck, und die Nachpruefung lehnte denselben Token wieder ab. Ergebnis
    /// war eine 502 bei jedem Versuch, bis die Restlaufzeit unter fuenf
    /// Minuten fiel.
    #[tokio::test]
    async fn token_im_fenster_zwischen_den_vorlaeufen_kommt_trotzdem() {
        let pool = pool_oder_ende!();
        let config = config_mit(Arc::new(FakeTokenClient::neu()));
        let jetzt = zeit("2026-08-28T10:00:00Z");
        zeile_anlegen(
            &pool,
            &config.cipher,
            "5108",
            "dazwischen",
            UPLINK,
            Some(jetzt + Duration::minutes(7)),
            false,
        )
        .await;
        let antwort = platform_token_antwort(&pool, &config, 5108, "twitch", jetzt)
            .await
            .expect("Token muss kommen, nicht 502");
        assert!(!antwort.access_token.is_empty());
    }

    #[test]
    fn abgelaufen_ist_strenger_als_faellig() {
        let jetzt = zeit("2026-08-28T10:00:00Z");
        // Sieben Minuten Rest: erneuern ja, tot nein.
        assert!(refresh_faellig(Some(jetzt + Duration::minutes(7)), jetzt));
        assert!(!abgelaufen(Some(jetzt + Duration::minutes(7)), jetzt));
        // Genau abgelaufen zaehlt als abgelaufen.
        assert!(abgelaufen(Some(jetzt), jetzt));
        assert!(abgelaufen(Some(jetzt - Duration::seconds(1)), jetzt));
        assert!(abgelaufen(None, jetzt));
    }

    /// Der Zweig fuer Kick, YouTube und TikTok. Die Tabelle ist heute leer,
    /// also ist 404 die richtige Antwort und nicht ein Fehler.
    #[tokio::test]
    async fn ohne_eintrag_ist_die_fremde_plattform_404() {
        let pool = pool_oder_ende!();
        let config = config_mit(Arc::new(FakeTokenClient::neu()));
        let jetzt = zeit("2026-08-28T10:00:00Z");
        assert_eq!(
            platform_token_antwort(&pool, &config, 5109, "kick", jetzt).await,
            Err(TokenFehler::KeineVerbindung)
        );
    }

    struct FakeKick {
        ergebnis: std::sync::Mutex<Result<super::super::plattform_oauth::OAuthToken, super::super::plattform_oauth::OAuthFehler>>,
    }

    #[async_trait::async_trait]
    impl super::super::plattform_oauth::KickApi for FakeKick {
        async fn exchange_code(
            &self,
            _code: &str,
            _redirect_uri: &str,
            _verifier: &str,
        ) -> Result<super::super::plattform_oauth::OAuthToken, super::super::plattform_oauth::OAuthFehler>
        {
            unreachable!("kein exchange in diesem test")
        }
        async fn refresh(
            &self,
            _refresh_token: &str,
        ) -> Result<super::super::plattform_oauth::OAuthToken, super::super::plattform_oauth::OAuthFehler>
        {
            match &*self.ergebnis.lock().unwrap() {
                Ok(t) => Ok(t.clone()),
                Err(e) => Err(e.clone()),
            }
        }
        async fn revoke(
            &self,
            _access_token: &str,
        ) -> Result<(), super::super::plattform_oauth::OAuthFehler> {
            unreachable!("kein revoke in diesem test")
        }
        async fn konto(
            &self,
            _access_token: &str,
        ) -> Result<super::super::plattform_oauth::KickKonto, super::super::plattform_oauth::OAuthFehler>
        {
            unreachable!("kein konto in diesem test")
        }
        async fn event_subscriptions_loeschen(
            &self,
            _access_token: &str,
        ) -> Result<(), super::super::plattform_oauth::OAuthFehler> {
            Ok(())
        }
    }

    async fn kick_zeile_anlegen(
        pool: &PgPool,
        cipher: &FieldCipher,
        streamer_id: i64,
        expires_at: DateTime<Utc>,
    ) {
        let aad = super::super::platform_store::PlatformConnectionStore::aad(streamer_id, "kick");
        let access = cipher.encrypt_field("kick-alt", &aad).unwrap();
        let refresh = cipher.encrypt_field("kick-ref-alt", &aad).unwrap();
        sqlx::query(
            "INSERT INTO platform_connections \
             (streamer_id, platform, platform_user_id, platform_login, \
              access_token_enc, refresh_token_enc, scopes, expires_at) \
             VALUES ($1, 'kick', 'k-1', 'streamerin', $2, $3, $4, $5)",
        )
        .bind(streamer_id)
        .bind(&access)
        .bind(&refresh)
        .bind(vec!["chat:write".to_string()])
        .bind(expires_at)
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn kick_token_wird_bei_ablauf_erneuert() {
        let pool = pool_oder_ende!();
        let mut config = config_mit(Arc::new(FakeTokenClient::neu()));
        config.kick = Some(Arc::new(FakeKick {
            ergebnis: std::sync::Mutex::new(Ok(super::super::plattform_oauth::OAuthToken {
                access_token: "kick-frisch".into(),
                refresh_token: Some("kick-ref-frisch".into()),
                expires_in: 7200,
                scopes: vec!["chat:write".into()],
            })),
        }));
        let jetzt = zeit("2026-08-28T10:00:00Z");
        kick_zeile_anlegen(&pool, &config.cipher, 5201, jetzt + Duration::minutes(2)).await;

        let antwort = platform_token_antwort(&pool, &config, 5201, "kick", jetzt)
            .await
            .expect("frischer Token muss kommen");
        assert_eq!(antwort.access_token, "kick-frisch");
        let json = serde_json::to_value(&antwort).unwrap();
        assert!(!json.to_string().contains("kick-ref"));
    }

    #[tokio::test]
    async fn kick_invalid_grant_setzt_needs_reauth() {
        let pool = pool_oder_ende!();
        let mut config = config_mit(Arc::new(FakeTokenClient::neu()));
        config.kick = Some(Arc::new(FakeKick {
            ergebnis: std::sync::Mutex::new(Err(
                super::super::plattform_oauth::OAuthFehler::InvalidGrant,
            )),
        }));
        let jetzt = zeit("2026-08-28T10:00:00Z");
        kick_zeile_anlegen(&pool, &config.cipher, 5202, jetzt + Duration::minutes(2)).await;

        assert_eq!(
            platform_token_antwort(&pool, &config, 5202, "kick", jetzt).await,
            Err(TokenFehler::NeuVerbinden)
        );
        let reauth: bool = sqlx::query_scalar(
            "SELECT needs_reauth FROM platform_connections WHERE streamer_id = 5202",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(reauth);
    }

    #[tokio::test]
    async fn kick_ohne_client_und_abgelaufen_meldet_neu_verbinden() {
        let pool = pool_oder_ende!();
        let config = config_mit(Arc::new(FakeTokenClient::neu()));
        let jetzt = zeit("2026-08-28T10:00:00Z");
        kick_zeile_anlegen(&pool, &config.cipher, 5203, jetzt - Duration::minutes(1)).await;
        assert_eq!(
            platform_token_antwort(&pool, &config, 5203, "kick", jetzt).await,
            Err(TokenFehler::NeuVerbinden)
        );
    }

    #[tokio::test]
    async fn kick_ohne_client_aber_gueltig_liefert_den_token() {
        let pool = pool_oder_ende!();
        let config = config_mit(Arc::new(FakeTokenClient::neu()));
        let jetzt = zeit("2026-08-28T10:00:00Z");
        kick_zeile_anlegen(&pool, &config.cipher, 5204, jetzt + Duration::minutes(2)).await;
        let antwort = platform_token_antwort(&pool, &config, 5204, "kick", jetzt)
            .await
            .expect("gueltiger Token muss kommen");
        assert_eq!(antwort.access_token, "kick-alt");
    }

    /// Und wenn dort etwas liegt, kommt es auch heraus, ohne Refresh-Token.
    #[tokio::test]
    async fn eintrag_im_plattform_speicher_wird_geliefert() {
        let pool = pool_oder_ende!();
        let config = config_mit(Arc::new(FakeTokenClient::neu()));
        let jetzt = zeit("2026-08-28T10:00:00Z");
        let aad = super::super::platform_store::PlatformConnectionStore::aad(5110, "kick");
        let access = config.cipher.encrypt_field("kick-acc", &aad).unwrap();
        let refresh = config.cipher.encrypt_field("kick-ref", &aad).unwrap();
        sqlx::query(
            "INSERT INTO platform_connections \
             (streamer_id, platform, platform_user_id, platform_login, \
              access_token_enc, refresh_token_enc, scopes, expires_at) \
             VALUES (5110, 'kick', 'k-1', 'streamerin', $1, $2, $3, $4)",
        )
        .bind(&access)
        .bind(&refresh)
        .bind(vec!["chat:read".to_string()])
        .bind(jetzt + Duration::hours(2))
        .execute(&pool)
        .await
        .unwrap();

        let antwort = platform_token_antwort(&pool, &config, 5110, "kick", jetzt)
            .await
            .expect("Token muss kommen");
        assert_eq!(antwort.access_token, "kick-acc");
        assert_eq!(antwort.platform_login, "streamerin");
        let json = serde_json::to_value(&antwort).unwrap();
        assert!(json.get("refresh_token").is_none());
        assert!(!json.to_string().contains("kick-ref"));
    }

    #[tokio::test]
    async fn die_antwort_ist_json_ohne_refresh_token() {
        let antwort = Json(PlatformTokenAntwort {
            access_token: "acc".into(),
            expires_at: zeit("2026-08-28T12:00:00Z"),
            platform_user_id: "5107".into(),
            platform_login: "streamerin".into(),
            scopes: vec!["user:read:chat".into()],
        })
        .into_response();
        let body = to_bytes(antwort.into_body(), 64 * 1024).await.unwrap();
        let text = String::from_utf8(body.to_vec()).unwrap();
        assert!(text.contains("access_token"));
        assert!(!text.contains("refresh"));
    }
}
