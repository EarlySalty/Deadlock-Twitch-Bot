//! Chat-Plattformen mit dem Uplink verbinden (Multi-Chat, Ausbaustufe Twitch).
//!
//! Drei Dinge leben hier:
//!
//! 1. Der OAuth-Flow "Chat verbinden": `GET /twitch/api/v2/uplink/connect/{platform}`
//!    schickt den eingeloggten Partner mit Chat-Scopes zu Twitch, der Callback
//!    tauscht den Code, prueft, dass das gewaehrte Konto wirklich das Konto
//!    des Streamers ist, und legt beide Tokens verschluesselt in
//!    `platform_connections` ab.
//! 2. Der Refresh: lazy beim Abruf (wenn weniger als zehn Minuten Restlaufzeit)
//!    und periodisch alle 5 Minuten fuer alles, was innerhalb des Vorlaufs
//!    ablaeuft. Der Takt liegt unter dem Vorlauf, damit kein Token zwischen
//!    zwei Laeufen unbemerkt ablaeuft. Ein `invalid_grant` markiert die
//!    Verbindung als `needs_reauth`.
//! 3. Die interne Route `GET /twitch/api/v2/internal/platform-token`, ueber die
//!    rs-relay einen gueltigen Access-Token holt. Nur Loopback plus
//!    `X-Internal-Token`. Die Antwort traegt nie den Refresh-Token: das Relay
//!    soll ihn gar nicht kennen (Contract REQ-7).
//!
//! Der Raid-Token-Speicher (`twitch_raid_auth`) bleibt unberuehrt; das ist ein
//! anderer Scope-Satz fuer einen anderen Zweck.

// Axum-Responses direkt im Result, wie in uplink.rs.
#![allow(clippy::result_large_err)]

use std::{collections::HashMap, net::SocketAddr, sync::Arc, time::Duration};

use async_trait::async_trait;
use axum::{
    extract::{ConnectInfo, Extension, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Redirect, Response},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;
use tb_crypto::FieldCipher;
use tb_http_core::{ExpectedToken, INTERNAL_TOKEN_HEADER};
use tb_transport_twitch::{
    user_token::{TokenOwner, UserTokenError},
    HelixClient, HelixConfig,
};

use crate::auth::{
    level::DashboardAuthLevel,
    oauth_login::{build_scoped_authorize_url, CHAT_SCOPES},
    security::require_internal,
    session::{DashboardAuthState, PlatformConnectState},
};

/// Einzige Plattform mit fertigem Verbinden-Flow. Andere Namen kommen ueber
/// dieselbe Route, werden aber sauber abgewiesen.
pub const PLATFORM_TWITCH: &str = "twitch";

/// Wohin der Browser nach dem Verbinden zurueckkommt.
const UPLINK_SEITE: &str = "/twitch/uplink";

/// Ab dieser Restlaufzeit wird beim Abruf vorab erneuert.
pub const REFRESH_VORLAUF: chrono::Duration = chrono::Duration::minutes(10);

/// Takt des periodischen Refresh-Jobs. Muss unter [`REFRESH_VORLAUF`]
/// liegen: ein Token, das kurz nach einem Lauf in den Vorlauf rutscht, wird
/// sonst erst angefasst, wenn es schon tot ist.
pub const REFRESH_TAKT: Duration = Duration::from_secs(5 * 60);

/// Wie viele Verbindungen ein Lauf des periodischen Jobs hoechstens anfasst.
const REFRESH_LAUF_LIMIT: i64 = 50;

// ───────────────────────────────────────────────────────────────────────────
// OAuth-Client (Trait fuer Fakes im Test)
// ───────────────────────────────────────────────────────────────────────────

/// Token-Paar, wie es der Token-Endpoint liefert.
#[derive(Debug, Clone)]
pub struct PlatformTokens {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: i64,
    pub scopes: Vec<String>,
}

/// Der HTTP-Pfad zur Plattform (Code-Tausch, Inhaber, Refresh), damit der
/// Handler im Test mit einem Fake bedient werden kann.
#[async_trait]
pub trait PlatformOAuthClient: Send + Sync {
    async fn exchange_code(
        &self,
        code: &str,
        redirect_uri: &str,
    ) -> Result<PlatformTokens, UserTokenError>;
    async fn fetch_owner(&self, access_token: &str) -> Result<TokenOwner, UserTokenError>;
    async fn refresh(&self, refresh_token: &str) -> Result<PlatformTokens, UserTokenError>;
}

/// Echte Twitch-Anbindung ueber den vorhandenen Helix-Client.
pub struct HelixPlatformClient {
    helix: HelixClient,
}

impl HelixPlatformClient {
    pub fn new(client_id: &str, client_secret: &str) -> Result<Self, reqwest::Error> {
        Ok(Self {
            helix: HelixClient::new(HelixConfig::new(client_id, client_secret))?,
        })
    }

    /// Mit ueberschriebenen URLs (wiremock).
    pub fn from_config(config: HelixConfig) -> Result<Self, reqwest::Error> {
        Ok(Self {
            helix: HelixClient::new(config)?,
        })
    }
}

fn tokens_aus(antwort: tb_transport_twitch::user_token::UserTokenResponse) -> PlatformTokens {
    PlatformTokens {
        access_token: antwort.access_token,
        refresh_token: antwort.refresh_token,
        expires_in: antwort.expires_in,
        scopes: antwort.scope,
    }
}

#[async_trait]
impl PlatformOAuthClient for HelixPlatformClient {
    async fn exchange_code(
        &self,
        code: &str,
        redirect_uri: &str,
    ) -> Result<PlatformTokens, UserTokenError> {
        self.helix
            .exchange_user_code(code, redirect_uri)
            .await
            .map(tokens_aus)
    }

    async fn fetch_owner(&self, access_token: &str) -> Result<TokenOwner, UserTokenError> {
        self.helix.fetch_token_owner(access_token).await
    }

    async fn refresh(&self, refresh_token: &str) -> Result<PlatformTokens, UserTokenError> {
        self.helix
            .refresh_user_token(refresh_token)
            .await
            .map(tokens_aus)
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Konfiguration
// ───────────────────────────────────────────────────────────────────────────

/// Laufzeit-Konfiguration des Verbinden-Flows (als Extension). Fehlt sie,
/// antworten die Routen mit 503 statt zu raten.
#[derive(Clone)]
pub struct PlatformConnectConfig {
    pub client_id: String,
    /// Exakte Redirect-URI des Verbinden-Callbacks, muss in der Twitch-Konsole
    /// registriert sein.
    pub redirect_uri: String,
    pub client: Arc<dyn PlatformOAuthClient>,
    pub cipher: Arc<FieldCipher>,
}

fn non_empty_env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// Baut die Config aus der Prozessumgebung (Infisical-geladen). Braucht
/// `TWITCH_CLIENT_ID`, `TWITCH_CLIENT_SECRET`,
/// `TWITCH_PLATFORM_CONNECT_REDIRECT_URI` und den Feldschluessel
/// `DB_MASTER_KEY_V1`. Fehlt eines, bleibt der Flow aus. Secrets werden nicht
/// geloggt.
pub fn platform_connect_config_from_env() -> Option<PlatformConnectConfig> {
    let client_id = non_empty_env("TWITCH_CLIENT_ID")?;
    let client_secret = non_empty_env("TWITCH_CLIENT_SECRET")?;
    let redirect_uri = non_empty_env("TWITCH_PLATFORM_CONNECT_REDIRECT_URI")?;
    let cipher = match FieldCipher::from_env() {
        Ok(c) => Arc::new(c),
        Err(e) => {
            tracing::warn!(error = %e, "platform_connect: Feldschluessel fehlt, Verbinden-Flow bleibt aus");
            return None;
        }
    };
    let client = match HelixPlatformClient::new(&client_id, &client_secret) {
        Ok(c) => Arc::new(c),
        Err(e) => {
            tracing::warn!(error = %e, "platform_connect: Helix-Client nicht baubar");
            return None;
        }
    };
    Some(PlatformConnectConfig {
        client_id,
        redirect_uri,
        client,
        cipher,
    })
}

// ───────────────────────────────────────────────────────────────────────────
// Speicher
// ───────────────────────────────────────────────────────────────────────────

/// Eine gespeicherte Verbindung, entschluesselt. Der Refresh-Token bleibt
/// absichtlich in diesem Typ und wandert nie in eine HTTP-Antwort.
#[derive(Debug, Clone)]
pub struct PlatformConnection {
    pub streamer_id: i64,
    pub platform: String,
    pub platform_user_id: String,
    pub platform_login: String,
    pub access_token: String,
    pub refresh_token: String,
    pub scopes: Vec<String>,
    pub expires_at: DateTime<Utc>,
    pub needs_reauth: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum StoreFehler {
    #[error("db: {0}")]
    Db(#[from] sqlx::Error),
    #[error("crypto: {0}")]
    Crypto(String),
}

/// Zugriff auf `platform_connections` samt Feldverschluesselung.
#[derive(Clone)]
pub struct PlatformConnectionStore {
    pool: PgPool,
    cipher: Arc<FieldCipher>,
}

type RefreshSchluessel = (i64, String);

/// Ein Schloss je (Streamer, Plattform) fuer den Refresh, prozessweit. Die
/// Store-Instanzen werden je Request und fuer den periodischen Job getrennt
/// gebaut (`internal_platform_token_handler`, `uplink.rs`, `lib.rs`), darum
/// darf die Karte nicht am Store haengen: sonst haette jeder Aufrufer seine
/// eigene leere Karte und das Schloss schuetzt nichts. Die Begruendung des
/// Schutzes steht an [`erneuern`].
static REFRESH_SCHLOESSER: std::sync::LazyLock<
    std::sync::Mutex<HashMap<RefreshSchluessel, Arc<tokio::sync::Mutex<()>>>>,
> = std::sync::LazyLock::new(Default::default);

/// Haelt das Schloss eines Streamers und raeumt seinen Eintrag beim Loslassen
/// wieder aus der Karte, wenn niemand sonst darauf wartet.
struct RefreshWache {
    schluessel: RefreshSchluessel,
    schloss: Arc<tokio::sync::Mutex<()>>,
    _wache: Option<tokio::sync::OwnedMutexGuard<()>>,
}

impl RefreshWache {
    async fn nehmen(streamer_id: i64, platform: &str) -> Self {
        let schluessel = (streamer_id, platform.to_string());
        let schloss = REFRESH_SCHLOESSER
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(schluessel.clone())
            .or_default()
            .clone();
        let wache = schloss.clone().lock_owned().await;
        Self {
            schluessel,
            schloss,
            _wache: Some(wache),
        }
    }
}

impl Drop for RefreshWache {
    fn drop(&mut self) {
        self._wache = None;
        let mut karte = REFRESH_SCHLOESSER
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // Unter der Karten-Sperre: neue Wartende holen sich ihren Klon nur
        // hier, also ist die Zaehlung eindeutig. Zwei Klone sind die Karte
        // selbst und wir.
        if Arc::strong_count(&self.schloss) <= 2 {
            karte.remove(&self.schluessel);
        }
    }
}

/// Zeile, wie sie aus der Tabelle kommt (noch verschluesselt).
#[derive(sqlx::FromRow)]
struct Zeile {
    streamer_id: i64,
    platform: String,
    platform_user_id: String,
    platform_login: String,
    access_token_enc: Vec<u8>,
    refresh_token_enc: Vec<u8>,
    scopes: Vec<String>,
    expires_at: DateTime<Utc>,
    needs_reauth: bool,
}

const SELECT_ZEILE: &str = "SELECT streamer_id, platform, platform_user_id, platform_login, \
     access_token_enc, refresh_token_enc, scopes, expires_at, needs_reauth \
     FROM platform_connections";

impl PlatformConnectionStore {
    pub fn new(pool: PgPool, cipher: Arc<FieldCipher>) -> Self {
        Self { pool, cipher }
    }

    /// Zusatzdaten der Verschluesselung: bindet den Blob an Streamer und
    /// Plattform, damit ein umkopierter Blob in einer anderen Zeile nicht
    /// aufgeht.
    pub fn aad(streamer_id: i64, platform: &str) -> String {
        format!("platform_connections:{streamer_id}:{platform}")
    }

    fn entschluesseln(&self, zeile: Zeile) -> Result<PlatformConnection, StoreFehler> {
        let aad = Self::aad(zeile.streamer_id, &zeile.platform);
        let access_token = self
            .cipher
            .decrypt_field(&zeile.access_token_enc, &aad)
            .map_err(|e| StoreFehler::Crypto(e.to_string()))?;
        let refresh_token = self
            .cipher
            .decrypt_field(&zeile.refresh_token_enc, &aad)
            .map_err(|e| StoreFehler::Crypto(e.to_string()))?;
        Ok(PlatformConnection {
            streamer_id: zeile.streamer_id,
            platform: zeile.platform,
            platform_user_id: zeile.platform_user_id,
            platform_login: zeile.platform_login,
            access_token,
            refresh_token,
            scopes: zeile.scopes,
            expires_at: zeile.expires_at,
            needs_reauth: zeile.needs_reauth,
        })
    }

    fn verschluesseln(
        &self,
        streamer_id: i64,
        platform: &str,
        tokens: &PlatformTokens,
    ) -> Result<(Vec<u8>, Vec<u8>), StoreFehler> {
        let aad = Self::aad(streamer_id, platform);
        let access = self
            .cipher
            .encrypt_field(&tokens.access_token, &aad)
            .map_err(|e| StoreFehler::Crypto(e.to_string()))?;
        let refresh = self
            .cipher
            .encrypt_field(&tokens.refresh_token, &aad)
            .map_err(|e| StoreFehler::Crypto(e.to_string()))?;
        Ok((access, refresh))
    }

    /// Legt die Verbindung an oder ersetzt sie (neuer Grant). Setzt
    /// `needs_reauth` zurueck.
    pub async fn upsert(
        &self,
        streamer_id: i64,
        platform: &str,
        platform_user_id: &str,
        platform_login: &str,
        tokens: &PlatformTokens,
        jetzt: DateTime<Utc>,
    ) -> Result<(), StoreFehler> {
        let (access, refresh) = self.verschluesseln(streamer_id, platform, tokens)?;
        let expires_at = jetzt + chrono::Duration::seconds(tokens.expires_in.max(0));
        sqlx::query(
            "INSERT INTO platform_connections \
               (streamer_id, platform, platform_user_id, platform_login, \
                access_token_enc, refresh_token_enc, enc_kid, scopes, expires_at, \
                needs_reauth, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, FALSE, $10, $10) \
             ON CONFLICT (streamer_id, platform) DO UPDATE SET \
               platform_user_id = EXCLUDED.platform_user_id, \
               platform_login = EXCLUDED.platform_login, \
               access_token_enc = EXCLUDED.access_token_enc, \
               refresh_token_enc = EXCLUDED.refresh_token_enc, \
               enc_kid = EXCLUDED.enc_kid, \
               scopes = EXCLUDED.scopes, \
               expires_at = EXCLUDED.expires_at, \
               needs_reauth = FALSE, \
               updated_at = EXCLUDED.updated_at",
        )
        .bind(streamer_id)
        .bind(platform)
        .bind(platform_user_id)
        .bind(platform_login)
        .bind(access)
        .bind(refresh)
        .bind(self.cipher.kid())
        .bind(&tokens.scopes)
        .bind(expires_at)
        .bind(jetzt)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Nur die Tokens nach einem Refresh ersetzen.
    pub async fn update_tokens(
        &self,
        streamer_id: i64,
        platform: &str,
        tokens: &PlatformTokens,
        jetzt: DateTime<Utc>,
    ) -> Result<DateTime<Utc>, StoreFehler> {
        self.update_tokens_mit(&self.pool, streamer_id, platform, tokens, jetzt)
            .await
    }

    /// Wie [`Self::update_tokens`], aber auf einem uebergebenen Executor,
    /// damit [`Self::tokens_ablegen`] in seiner kurzen Schreibtransaktion
    /// bleibt.
    async fn update_tokens_mit<'e, E>(
        &self,
        exec: E,
        streamer_id: i64,
        platform: &str,
        tokens: &PlatformTokens,
        jetzt: DateTime<Utc>,
    ) -> Result<DateTime<Utc>, StoreFehler>
    where
        E: sqlx::PgExecutor<'e>,
    {
        let (access, refresh) = self.verschluesseln(streamer_id, platform, tokens)?;
        let expires_at = jetzt + chrono::Duration::seconds(tokens.expires_in.max(0));
        sqlx::query(
            "UPDATE platform_connections SET \
               access_token_enc = $3, refresh_token_enc = $4, enc_kid = $5, \
               scopes = CASE WHEN cardinality($6::text[]) > 0 THEN $6 ELSE scopes END, \
               expires_at = $7, needs_reauth = FALSE, updated_at = $8 \
             WHERE streamer_id = $1 AND platform = $2",
        )
        .bind(streamer_id)
        .bind(platform)
        .bind(access)
        .bind(refresh)
        .bind(self.cipher.kid())
        .bind(&tokens.scopes)
        .bind(expires_at)
        .bind(jetzt)
        .execute(exec)
        .await?;
        Ok(expires_at)
    }

    pub async fn load(
        &self,
        streamer_id: i64,
        platform: &str,
    ) -> Result<Option<PlatformConnection>, StoreFehler> {
        self.load_mit(&self.pool, streamer_id, platform, false)
            .await
    }

    /// Wie [`Self::load`], auf einem Executor; mit `sperren` haelt die
    /// umgebende Transaktion die Zeile per `FOR UPDATE`, bis sie endet.
    async fn load_mit<'e, E>(
        &self,
        exec: E,
        streamer_id: i64,
        platform: &str,
        sperren: bool,
    ) -> Result<Option<PlatformConnection>, StoreFehler>
    where
        E: sqlx::PgExecutor<'e>,
    {
        let sperre = if sperren { " FOR UPDATE" } else { "" };
        let zeile: Option<Zeile> = sqlx::query_as(&format!(
            "{SELECT_ZEILE} WHERE streamer_id = $1 AND platform = $2{sperre}"
        ))
        .bind(streamer_id)
        .bind(platform)
        .fetch_optional(exec)
        .await?;
        zeile.map(|z| self.entschluesseln(z)).transpose()
    }

    pub async fn mark_reauth(&self, streamer_id: i64, platform: &str) -> Result<(), StoreFehler> {
        self.mark_reauth_mit(&self.pool, streamer_id, platform)
            .await
    }

    async fn mark_reauth_mit<'e, E>(
        &self,
        exec: E,
        streamer_id: i64,
        platform: &str,
    ) -> Result<(), StoreFehler>
    where
        E: sqlx::PgExecutor<'e>,
    {
        sqlx::query(
            "UPDATE platform_connections SET needs_reauth = TRUE, updated_at = NOW() \
             WHERE streamer_id = $1 AND platform = $2",
        )
        .bind(streamer_id)
        .bind(platform)
        .execute(exec)
        .await?;
        Ok(())
    }

    /// Verbindungen eines Streamers als (Plattform, Status). Status ist
    /// `verbunden` oder `neu_verbinden`; was nicht in der Tabelle steht, ist
    /// `getrennt` und wird vom Aufrufer ergaenzt.
    pub async fn status_liste(
        &self,
        streamer_id: i64,
    ) -> Result<Vec<(String, &'static str)>, StoreFehler> {
        let zeilen: Vec<(String, bool)> = sqlx::query_as(
            "SELECT platform, needs_reauth FROM platform_connections \
             WHERE streamer_id = $1 ORDER BY platform",
        )
        .bind(streamer_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(zeilen
            .into_iter()
            .map(|(p, reauth)| (p, if reauth { "neu_verbinden" } else { "verbunden" }))
            .collect())
    }

    /// Schluessel aller Verbindungen, die vor `frist` ablaufen und nicht
    /// schon auf `needs_reauth` stehen. Ohne Zeilensperre und ohne
    /// Entschluesseln: [`erneuern`] liest die Zeile unter dem Schloss ohnehin
    /// neu, und dort liegt auch der Schutz gegen doppeltes Einloesen.
    async fn faellige_laden(
        &self,
        frist: DateTime<Utc>,
    ) -> Result<Vec<RefreshSchluessel>, StoreFehler> {
        Ok(sqlx::query_as(
            "SELECT streamer_id, platform FROM platform_connections \
             WHERE needs_reauth = FALSE AND expires_at < $1 \
             ORDER BY expires_at LIMIT $2",
        )
        .bind(frist)
        .bind(REFRESH_LAUF_LIMIT)
        .fetch_all(&self.pool)
        .await?)
    }

    /// Legt frisch geholte Tokens ab, aber nur, wenn die Zeile noch den
    /// Refresh-Token traegt, der dafuer eingeloest wurde (`eingeloest`).
    /// Kurze Transaktion: sperren, vergleichen, schreiben, commit. Traegt die
    /// Zeile schon einen anderen Refresh-Token, hat ein zweiter Prozess
    /// zwischenzeitlich erneuert; dann gelten dessen Werte.
    ///
    /// Schlaegt das Schreiben fehl, sind die neuen Tokens verloren und der
    /// alte ist bei Twitch verbraucht. Die Verbindung wird dann, so gut es
    /// geht, auf `needs_reauth` gesetzt, damit der Streamer es im Dashboard
    /// sieht statt still ohne Token dazustehen.
    async fn tokens_ablegen(
        &self,
        mut verbindung: PlatformConnection,
        eingeloest: &str,
        tokens: &PlatformTokens,
        jetzt: DateTime<Utc>,
    ) -> Result<PlatformConnection, TokenFehler> {
        let (sid, plattform) = (verbindung.streamer_id, verbindung.platform.clone());
        let schreiben = async {
            let mut tx = self.pool.begin().await.map_err(StoreFehler::from)?;
            let zeile = self
                .load_mit(&mut *tx, sid, &plattform, true)
                .await?
                .ok_or(TokenFehler::KeineVerbindung)?;
            if zeile.refresh_token != eingeloest {
                tx.commit().await.map_err(StoreFehler::from)?;
                return Ok(Err(zeile));
            }
            let expires_at = self
                .update_tokens_mit(&mut *tx, sid, &plattform, tokens, jetzt)
                .await?;
            tx.commit().await.map_err(StoreFehler::from)?;
            Ok::<_, TokenFehler>(Ok(expires_at))
        };
        match schreiben.await {
            Ok(Ok(expires_at)) => {
                verbindung.access_token = tokens.access_token.clone();
                verbindung.refresh_token = tokens.refresh_token.clone();
                if !tokens.scopes.is_empty() {
                    verbindung.scopes = tokens.scopes.clone();
                }
                verbindung.expires_at = expires_at;
                verbindung.needs_reauth = false;
                Ok(verbindung)
            }
            Ok(Err(fremd)) => {
                tracing::info!(streamer_id = sid, platform = %plattform, "platform_connect: Refresh von anderer Seite uebernommen");
                if fremd.needs_reauth {
                    Err(TokenFehler::NeuVerbinden)
                } else {
                    Ok(fremd)
                }
            }
            Err(TokenFehler::KeineVerbindung) => Err(TokenFehler::KeineVerbindung),
            Err(fehler) => {
                tracing::error!(streamer_id = sid, platform = %plattform, error = %fehler, "platform_connect: frische Tokens nicht speicherbar, Verbindung wird markiert");
                if let Err(markieren) = self.mark_reauth(sid, &plattform).await {
                    tracing::error!(streamer_id = sid, platform = %plattform, error = %markieren, "platform_connect: needs_reauth nicht setzbar");
                }
                Err(fehler)
            }
        }
    }

    /// Nach einem `invalid_grant`: `needs_reauth` nur setzen, wenn die Zeile
    /// noch den eingeloesten Refresh-Token traegt. Hat ein zweiter Prozess
    /// ihn inzwischen erneuert, war unser Aufruf bloss der Verlierer des
    /// Rennens, und dessen frische Werte gelten.
    async fn reauth_wenn_unveraendert(
        &self,
        streamer_id: i64,
        platform: &str,
        eingeloest: &str,
    ) -> Result<PlatformConnection, TokenFehler> {
        let mut tx = self.pool.begin().await.map_err(StoreFehler::from)?;
        let zeile = self
            .load_mit(&mut *tx, streamer_id, platform, true)
            .await?
            .ok_or(TokenFehler::KeineVerbindung)?;
        let unveraendert = zeile.refresh_token == eingeloest;
        if unveraendert {
            self.mark_reauth_mit(&mut *tx, streamer_id, platform)
                .await?;
        }
        tx.commit().await.map_err(StoreFehler::from)?;
        if unveraendert || zeile.needs_reauth {
            Err(TokenFehler::NeuVerbinden)
        } else {
            Ok(zeile)
        }
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Refresh
// ───────────────────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TokenFehler {
    #[error("keine Verbindung")]
    KeineVerbindung,
    #[error("Verbindung muss neu bestaetigt werden")]
    NeuVerbinden,
    #[error("Speicher: {0}")]
    Speicher(String),
    #[error("Plattform: {0}")]
    Plattform(String),
}

impl From<StoreFehler> for TokenFehler {
    fn from(e: StoreFehler) -> Self {
        TokenFehler::Speicher(e.to_string())
    }
}

fn ist_frisch(verbindung: &PlatformConnection, jetzt: DateTime<Utc>) -> bool {
    verbindung.expires_at - jetzt >= REFRESH_VORLAUF
}

/// Liefert die Verbindung mit einem gueltigen Access-Token. Erneuert vorab,
/// wenn weniger als [`REFRESH_VORLAUF`] Restlaufzeit bleibt. `invalid_grant`
/// (Twitch 400, Refresh-Token widerrufen oder verbraucht) setzt `needs_reauth`.
/// Ein `invalid_client` ist unser Fehler, nicht der des Streamers, und wird
/// nur gemeldet.
///
/// Der Normalfall (gueltiger Token) kommt ohne Sperre aus. Wie der Refresh
/// gegen doppeltes Einloesen geschuetzt ist, steht an [`erneuern`].
pub async fn refresh_if_needed(
    store: &PlatformConnectionStore,
    client: &dyn PlatformOAuthClient,
    streamer_id: i64,
    platform: &str,
    jetzt: DateTime<Utc>,
) -> Result<PlatformConnection, TokenFehler> {
    let verbindung = store
        .load(streamer_id, platform)
        .await?
        .ok_or(TokenFehler::KeineVerbindung)?;
    if verbindung.needs_reauth {
        return Err(TokenFehler::NeuVerbinden);
    }
    if ist_frisch(&verbindung, jetzt) {
        return Ok(verbindung);
    }
    erneuern(store, client, streamer_id, platform, jetzt).await
}

/// Der eigentliche Refresh.
///
/// Twitch macht den Refresh-Token beim ersten Einloesen ungueltig. Zwei
/// gleichzeitige Abrufe (zwei Relay-Adapter beim Session-Start, oder Abruf
/// plus periodischer Lauf) duerfen ihn deshalb nicht beide einloesen, und der
/// Verlierer darf die Verbindung nicht faelschlich auf `needs_reauth` setzen.
///
/// Der Netzaufruf steht bewusst in keiner Datenbank-Transaktion: eine
/// Zeilensperre ueber den HTTP-Aufruf hinweg haette beim Scheitern des
/// Commits die frischen Tokens verworfen, waehrend der alte bei Twitch schon
/// verbraucht ist. Stattdessen zwei Lagen:
///
/// 1. Ein prozessweites Schloss je (Streamer, Plattform)
///    ([`REFRESH_SCHLOESSER`], unabhaengig von der Store-Instanz). Wer es
///    nach einem anderen bekommt, liest neu und sieht dessen frische Tokens,
///    ohne selbst zu Twitch zu gehen. Das deckt alle Aufrufer in diesem
///    Prozess ab (Request-Handler und periodischer Job), also den Normalfall.
/// 2. Ein Vergleich beim Schreiben ([`PlatformConnectionStore::tokens_ablegen`]):
///    geschrieben wird nur, wenn die Zeile noch den eingeloesten Refresh-Token
///    traegt. Ein zweiter Prozess kann so hoechstens einen Twitch-Aufruf
///    verschwenden; sein `invalid_grant` setzt kein `needs_reauth`, wenn die
///    Zeile inzwischen einen anderen Refresh-Token traegt
///    ([`PlatformConnectionStore::reauth_wenn_unveraendert`]).
///
/// Gewaehlt statt einer Markierungsspalte ("Refresh laeuft"), weil es ohne
/// Migration und ohne haengende Markierungen nach einem Absturz auskommt und
/// die Zeile nur fuer das Schreiben selbst gesperrt bleibt.
async fn erneuern(
    store: &PlatformConnectionStore,
    client: &dyn PlatformOAuthClient,
    streamer_id: i64,
    platform: &str,
    jetzt: DateTime<Utc>,
) -> Result<PlatformConnection, TokenFehler> {
    let _wache = RefreshWache::nehmen(streamer_id, platform).await;

    // Unter dem Schloss noch einmal lesen: ein anderer Aufruf kann die Zeile
    // inzwischen erneuert oder auf needs_reauth gesetzt haben.
    let aktuell = store
        .load(streamer_id, platform)
        .await?
        .ok_or(TokenFehler::KeineVerbindung)?;
    if aktuell.needs_reauth {
        return Err(TokenFehler::NeuVerbinden);
    }
    if ist_frisch(&aktuell, jetzt) {
        return Ok(aktuell);
    }

    let eingeloest = aktuell.refresh_token.clone();
    match client.refresh(&eingeloest).await {
        Ok(tokens) => {
            if tokens.access_token.trim().is_empty() || tokens.refresh_token.trim().is_empty() {
                return Err(TokenFehler::Plattform("Refresh ohne Tokens".into()));
            }
            store
                .tokens_ablegen(aktuell, &eingeloest, &tokens, jetzt)
                .await
        }
        Err(UserTokenError::InvalidGrant) => {
            store
                .reauth_wenn_unveraendert(streamer_id, platform, &eingeloest)
                .await
        }
        Err(UserTokenError::InvalidClient) => Err(TokenFehler::Plattform(
            "Client-Zugangsdaten abgelehnt".into(),
        )),
        Err(UserTokenError::Other(text)) => Err(TokenFehler::Plattform(text)),
    }
}

/// Ein Lauf des periodischen Jobs: alles erneuern, was innerhalb des
/// Vorlaufs ablaeuft. Jede Verbindung bekommt ihren eigenen Refresh mit
/// eigener kurzer Schreibtransaktion; ein Fehler bei einer Verbindung laesst
/// die schon erneuerten stehen. Gibt die Zahl der erneuerten Verbindungen
/// zurueck.
pub async fn refresh_faellige(
    store: &PlatformConnectionStore,
    client: &dyn PlatformOAuthClient,
    jetzt: DateTime<Utc>,
) -> Result<usize, TokenFehler> {
    let faellige = store.faellige_laden(jetzt + REFRESH_VORLAUF).await?;
    let mut erneuert = 0usize;
    for (sid, plattform) in faellige {
        match erneuern(store, client, sid, &plattform, jetzt).await {
            Ok(_) => erneuert += 1,
            Err(TokenFehler::NeuVerbinden) => {
                tracing::info!(streamer_id = sid, platform = %plattform, "platform_connect: Verbindung braucht neue Bestaetigung");
            }
            Err(e) => {
                tracing::warn!(streamer_id = sid, platform = %plattform, error = %e, "platform_connect: Refresh fehlgeschlagen");
            }
        }
    }
    Ok(erneuert)
}

/// Startet den periodischen Refresh (alle [`REFRESH_TAKT`]). Braucht eine
/// laufende Tokio-Runtime; ohne (etwa in synchronen Router-Tests) passiert
/// nichts.
pub fn spawn_refresh_job(store: PlatformConnectionStore, client: Arc<dyn PlatformOAuthClient>) {
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        return;
    };
    handle.spawn(async move {
        let mut takt = tokio::time::interval(REFRESH_TAKT);
        takt.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            takt.tick().await;
            match refresh_faellige(&store, client.as_ref(), Utc::now()).await {
                Ok(0) => {}
                Ok(n) => tracing::info!(erneuert = n, "platform_connect: Tokens erneuert"),
                Err(e) => {
                    tracing::warn!(error = %e, "platform_connect: Refresh-Lauf fehlgeschlagen")
                }
            }
        }
    });
}

// ───────────────────────────────────────────────────────────────────────────
// Verbinden-Flow
// ───────────────────────────────────────────────────────────────────────────

fn text(status: StatusCode, s: &'static str) -> Response {
    (status, s).into_response()
}

fn nicht_konfiguriert() -> Response {
    text(
        StatusCode::SERVICE_UNAVAILABLE,
        "Chat verbinden ist auf diesem Server noch nicht eingerichtet.",
    )
}

fn plattform_pruefen(platform: &str) -> Result<String, Response> {
    let p = platform.trim().to_lowercase();
    if p == PLATFORM_TWITCH {
        Ok(p)
    } else {
        Err(text(
            StatusCode::BAD_REQUEST,
            "Diese Plattform kann noch nicht verbunden werden.",
        ))
    }
}

/// `GET /twitch/api/v2/uplink/connect/{platform}`: Start des Grants.
pub async fn connect_start_handler(
    State(pool): State<PgPool>,
    auth: DashboardAuthLevel,
    state: Option<Extension<DashboardAuthState>>,
    config: Option<Extension<PlatformConnectConfig>>,
    Path(platform): Path<String>,
) -> Response {
    let platform = match plattform_pruefen(&platform) {
        Ok(p) => p,
        Err(r) => return r,
    };
    let (Some(Extension(state)), Some(Extension(config))) = (state, config) else {
        return nicht_konfiguriert();
    };
    let (login, streamer_id) = match super::uplink::partner_identitaet(&pool, &auth).await {
        Ok(v) => v,
        Err(r) => return r,
    };

    let state_token = tb_crypto::random_urlsafe_token(24);
    let connect_state = PlatformConnectState {
        redirect_uri: config.redirect_uri.clone(),
        streamer_id,
        twitch_login: login,
        platform,
    };
    if let Err(error) = state
        .save_platform_connect_state(&state_token, &connect_state)
        .await
    {
        tracing::warn!(%error, "platform_connect: State nicht speicherbar");
        return text(
            StatusCode::SERVICE_UNAVAILABLE,
            "Der Verbindungsversuch konnte nicht gespeichert werden. Bitte noch einmal versuchen.",
        );
    }
    let url = build_scoped_authorize_url(
        &config.client_id,
        &config.redirect_uri,
        &state_token,
        &CHAT_SCOPES,
    );
    Redirect::to(&url).into_response()
}

/// `?code=&state=&error=` des Rueckwegs.
#[derive(Debug, Default, Deserialize)]
pub struct ConnectCallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
}

/// Ergebnis des Callbacks ohne HTTP-Huelle, damit der Kern ohne Router
/// testbar bleibt.
#[derive(Debug, PartialEq, Eq)]
pub enum CallbackErgebnis {
    Gespeichert,
    Abgebrochen,
    StateUngueltig,
    FremdeSession,
    FremdesKonto,
    TauschFehlgeschlagen,
    /// Der Grant kam ohne die Chat-Rechte zurueck (Streamer hat sie im
    /// Twitch-Dialog abgewaehlt). Ohne `user:write:chat` koennte das Dock
    /// nie senden, also wird der Grant gar nicht erst gespeichert.
    ScopesFehlen,
    SpeicherFehler,
}

/// Prueft, ob der Grant alle [`CHAT_SCOPES`] traegt. Eine leere Scope-Liste
/// ist kein Erfolg: dann ist unbekannt, was Twitch wirklich gewaehrt hat.
fn chat_scopes_fehlen(scopes: &[String]) -> bool {
    CHAT_SCOPES
        .iter()
        .any(|noetig| !scopes.iter().any(|s| s == noetig))
}

/// Kern des Callbacks: State verbrauchen, Code tauschen, Inhaber pruefen,
/// verschluesselt ablegen.
pub async fn callback_verarbeiten(
    state: &DashboardAuthState,
    config: &PlatformConnectConfig,
    store: &PlatformConnectionStore,
    session_streamer_id: i64,
    query: &ConnectCallbackQuery,
    jetzt: DateTime<Utc>,
) -> CallbackErgebnis {
    let state_token = query.state.as_deref().map(str::trim).unwrap_or("");
    let code = query.code.as_deref().map(str::trim).unwrap_or("");
    let fehler = query.error.as_deref().map(str::trim).unwrap_or("");

    if state_token.is_empty() {
        return CallbackErgebnis::StateUngueltig;
    }
    let connect_state = match state.consume_platform_connect_state(state_token).await {
        Ok(Some(s)) => s,
        Ok(None) => return CallbackErgebnis::StateUngueltig,
        Err(error) => {
            tracing::warn!(%error, "platform_connect: State-Lookup fehlgeschlagen");
            return CallbackErgebnis::StateUngueltig;
        }
    };
    if connect_state.streamer_id != session_streamer_id {
        return CallbackErgebnis::FremdeSession;
    }
    if !fehler.is_empty() || code.is_empty() {
        return CallbackErgebnis::Abgebrochen;
    }

    let tokens = match config
        .client
        .exchange_code(code, &connect_state.redirect_uri)
        .await
    {
        Ok(t) if t.access_token.trim().is_empty() || t.refresh_token.trim().is_empty() => {
            return CallbackErgebnis::TauschFehlgeschlagen
        }
        // Ohne Scope-Liste ist die Antwort unvollstaendig; wir raten nicht,
        // was gewaehrt wurde.
        Ok(t) if t.scopes.is_empty() => {
            tracing::warn!("platform_connect: Token-Antwort ohne Scopes");
            return CallbackErgebnis::TauschFehlgeschlagen;
        }
        Ok(t) if chat_scopes_fehlen(&t.scopes) => return CallbackErgebnis::ScopesFehlen,
        Ok(t) => t,
        Err(error) => {
            tracing::warn!(?error, "platform_connect: Code-Tausch fehlgeschlagen");
            return CallbackErgebnis::TauschFehlgeschlagen;
        }
    };
    let owner = match config.client.fetch_owner(&tokens.access_token).await {
        Ok(o) => o,
        Err(error) => {
            tracing::warn!(?error, "platform_connect: Inhaber nicht abrufbar");
            return CallbackErgebnis::TauschFehlgeschlagen;
        }
    };
    // Der Grant muss vom Streamer-Konto selbst kommen. Wer im Browser als
    // jemand anderes bei Twitch eingeloggt ist, bekommt sonst dessen Token in
    // die eigene Verbindung, und das Dock wuerde in einem fremden Chat senden.
    if owner.id.trim().parse::<i64>().ok() != Some(session_streamer_id) {
        return CallbackErgebnis::FremdesKonto;
    }

    match store
        .upsert(
            session_streamer_id,
            &connect_state.platform,
            owner.id.trim(),
            &owner.login.trim().to_lowercase(),
            &tokens,
            jetzt,
        )
        .await
    {
        Ok(()) => CallbackErgebnis::Gespeichert,
        Err(error) => {
            tracing::error!(%error, "platform_connect: Verbindung nicht speicherbar");
            CallbackErgebnis::SpeicherFehler
        }
    }
}

fn ergebnis_antwort(ergebnis: CallbackErgebnis) -> Response {
    match ergebnis {
        CallbackErgebnis::Gespeichert => {
            Redirect::to(&format!("{UPLINK_SEITE}?chat=verbunden")).into_response()
        }
        CallbackErgebnis::Abgebrochen => {
            Redirect::to(&format!("{UPLINK_SEITE}?chat=abgebrochen")).into_response()
        }
        CallbackErgebnis::StateUngueltig => text(
            StatusCode::BAD_REQUEST,
            "Der Verbindungsversuch ist abgelaufen. Bitte im Dashboard noch einmal auf Verbinden klicken.",
        ),
        CallbackErgebnis::FremdeSession => text(
            StatusCode::FORBIDDEN,
            "Dieser Verbindungsversuch gehört zu einem anderen Login.",
        ),
        CallbackErgebnis::FremdesKonto => text(
            StatusCode::FORBIDDEN,
            "Das bei Twitch bestätigte Konto ist nicht dein Streamer-Konto. Bitte bei Twitch mit dem Streamer-Konto anmelden und erneut verbinden.",
        ),
        CallbackErgebnis::TauschFehlgeschlagen => text(
            StatusCode::BAD_GATEWAY,
            "Twitch hat die Verbindung nicht bestätigt. Bitte später noch einmal versuchen.",
        ),
        CallbackErgebnis::ScopesFehlen => text(
            StatusCode::BAD_REQUEST,
            "Die Verbindung wurde ohne die Chat-Rechte bestätigt. Bitte erneut verbinden und im Twitch-Dialog beide Chat-Rechte (lesen und schreiben) zulassen.",
        ),
        CallbackErgebnis::SpeicherFehler => text(
            StatusCode::SERVICE_UNAVAILABLE,
            "Die Verbindung konnte nicht gespeichert werden. Bitte später noch einmal versuchen.",
        ),
    }
}

/// `GET /twitch/api/v2/uplink/connect/{platform}/callback`.
pub async fn connect_callback_handler(
    State(pool): State<PgPool>,
    auth: DashboardAuthLevel,
    state: Option<Extension<DashboardAuthState>>,
    config: Option<Extension<PlatformConnectConfig>>,
    Path(platform): Path<String>,
    Query(query): Query<ConnectCallbackQuery>,
) -> Response {
    if let Err(r) = plattform_pruefen(&platform) {
        return r;
    }
    let (Some(Extension(state)), Some(Extension(config))) = (state, config) else {
        return nicht_konfiguriert();
    };
    let (_, streamer_id) = match super::uplink::partner_identitaet(&pool, &auth).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    let store = PlatformConnectionStore::new(pool, config.cipher.clone());
    let ergebnis =
        callback_verarbeiten(&state, &config, &store, streamer_id, &query, Utc::now()).await;
    ergebnis_antwort(ergebnis)
}

// ───────────────────────────────────────────────────────────────────────────
// Interne Token-Route (rs-relay)
// ───────────────────────────────────────────────────────────────────────────

#[derive(Debug, Default, Deserialize)]
pub struct PlatformTokenQuery {
    pub streamer: Option<i64>,
    pub platform: Option<String>,
}

/// Was das Relay bekommt. Bewusst ein eigener Typ ohne `refresh_token`: die
/// Serialisierung kann ihn gar nicht mitschicken.
#[derive(Debug, Serialize)]
pub struct PlatformTokenAntwort {
    pub access_token: String,
    pub expires_at: DateTime<Utc>,
    pub platform_user_id: String,
    pub platform_login: String,
    pub scopes: Vec<String>,
}

impl From<PlatformConnection> for PlatformTokenAntwort {
    fn from(v: PlatformConnection) -> Self {
        Self {
            access_token: v.access_token,
            expires_at: v.expires_at,
            platform_user_id: v.platform_user_id,
            platform_login: v.platform_login,
            scopes: v.scopes,
        }
    }
}

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

/// Kern der internen Route: Auth ist schon geprueft.
pub async fn platform_token_antwort(
    store: &PlatformConnectionStore,
    client: &dyn PlatformOAuthClient,
    streamer_id: i64,
    platform: &str,
    jetzt: DateTime<Utc>,
) -> Result<PlatformTokenAntwort, Response> {
    match refresh_if_needed(store, client, streamer_id, platform, jetzt).await {
        Ok(v) => Ok(v.into()),
        Err(TokenFehler::KeineVerbindung) => Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "keine_verbindung" })),
        )
            .into_response()),
        Err(TokenFehler::NeuVerbinden) => Err((
            StatusCode::CONFLICT,
            Json(json!({ "error": "needs_reauth" })),
        )
            .into_response()),
        Err(e) => {
            tracing::warn!(streamer_id, platform, error = %e, "platform_token: nicht lieferbar");
            Err((
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": "token_nicht_lieferbar" })),
            )
                .into_response())
        }
    }
}

/// `GET /twitch/api/v2/internal/platform-token?streamer=&platform=`.
/// Nur Loopback plus `X-Internal-Token`; kein Cookie, kein CSRF.
pub async fn internal_platform_token_handler(
    State(pool): State<PgPool>,
    connect: Option<ConnectInfo<SocketAddr>>,
    expected: Option<Extension<ExpectedToken>>,
    config: Option<Extension<PlatformConnectConfig>>,
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
    let store = PlatformConnectionStore::new(pool, config.cipher.clone());
    match platform_token_antwort(
        &store,
        config.client.as_ref(),
        streamer_id,
        &platform,
        Utc::now(),
    )
    .await
    {
        Ok(antwort) => Json(antwort).into_response(),
        Err(r) => r,
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Tests
// ───────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Test-Feldschluessel (32 Byte Hex). Kein Produktionswert.
    const TEST_KEY_HEX: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
    const FERNET_KEY: &str = "dGVzdGtleTEyMzQ1Njc4OTAxMjM0NTY3ODkwMTIzNDU=";
    const MIGRATION: &str =
        include_str!("../../../../migrations/20260827090000_platform_connections.sql");

    fn cipher() -> Arc<FieldCipher> {
        Arc::new(FieldCipher::from_hex_key(TEST_KEY_HEX, "v1").expect("Testschluessel"))
    }

    fn zeit(roh: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(roh)
            .unwrap()
            .with_timezone(&Utc)
    }

    /// Fake-Plattform: liefert feste Tokens, zaehlt Refreshs, kann Fehler
    /// simulieren.
    struct FakePlattform {
        owner_id: String,
        /// Scopes, die der Code-Tausch zurueckgibt (Standard: CHAT_SCOPES).
        tausch_scopes: Mutex<Vec<String>>,
        refresh_ergebnis: Mutex<Result<PlatformTokens, UserTokenError>>,
        refreshs: Mutex<Vec<String>>,
        /// Kuenstliche Dauer des Refresh-Aufrufs, damit sich zwei parallele
        /// Abrufe ueberlappen koennen.
        refresh_dauer: Duration,
    }

    impl FakePlattform {
        fn neu(owner_id: &str) -> Self {
            Self {
                owner_id: owner_id.to_string(),
                tausch_scopes: Mutex::new(CHAT_SCOPES.iter().map(|s| s.to_string()).collect()),
                refresh_ergebnis: Mutex::new(Ok(tokens("acc-neu", "ref-neu", 14000))),
                refreshs: Mutex::new(Vec::new()),
                refresh_dauer: Duration::ZERO,
            }
        }
        fn mit_tausch_scopes(self, scopes: &[&str]) -> Self {
            *self.tausch_scopes.lock().unwrap() = scopes.iter().map(|s| s.to_string()).collect();
            self
        }
        fn mit_dauer(mut self, dauer: Duration) -> Self {
            self.refresh_dauer = dauer;
            self
        }
        fn mit_refresh(self, ergebnis: Result<PlatformTokens, UserTokenError>) -> Self {
            *self.refresh_ergebnis.lock().unwrap() = ergebnis;
            self
        }
        fn refresh_anzahl(&self) -> usize {
            self.refreshs.lock().unwrap().len()
        }
    }

    fn tokens(access: &str, refresh: &str, expires_in: i64) -> PlatformTokens {
        PlatformTokens {
            access_token: access.into(),
            refresh_token: refresh.into(),
            expires_in,
            scopes: CHAT_SCOPES.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[async_trait]
    impl PlatformOAuthClient for FakePlattform {
        async fn exchange_code(
            &self,
            code: &str,
            _redirect_uri: &str,
        ) -> Result<PlatformTokens, UserTokenError> {
            if code == "kaputt" {
                return Err(UserTokenError::Other("fake".into()));
            }
            Ok(PlatformTokens {
                scopes: self.tausch_scopes.lock().unwrap().clone(),
                ..tokens("acc-1", "ref-1", 14000)
            })
        }
        async fn fetch_owner(&self, _access_token: &str) -> Result<TokenOwner, UserTokenError> {
            Ok(TokenOwner {
                id: self.owner_id.clone(),
                login: "streamerin".into(),
                display_name: "Streamerin".into(),
                email: String::new(),
            })
        }
        async fn refresh(&self, refresh_token: &str) -> Result<PlatformTokens, UserTokenError> {
            self.refreshs
                .lock()
                .unwrap()
                .push(refresh_token.to_string());
            if !self.refresh_dauer.is_zero() {
                tokio::time::sleep(self.refresh_dauer).await;
            }
            self.refresh_ergebnis.lock().unwrap().clone()
        }
    }

    fn config_mit(client: Arc<dyn PlatformOAuthClient>) -> PlatformConnectConfig {
        PlatformConnectConfig {
            client_id: "cid".into(),
            redirect_uri: "https://x.test/twitch/api/v2/uplink/connect/twitch/callback".into(),
            client,
            cipher: cipher(),
        }
    }

    // ── ohne DB ────────────────────────────────────────────────────────────

    #[test]
    fn nur_twitch_ist_verbindbar() {
        assert_eq!(plattform_pruefen(" Twitch ").unwrap(), "twitch");
        assert!(plattform_pruefen("kick").is_err());
        assert!(plattform_pruefen("").is_err());
    }

    #[test]
    fn antwort_typ_kennt_keinen_refresh_token() {
        // REQ-7 auf Typebene: das Relay bekommt einen Typ, der den
        // Refresh-Token strukturell nicht tragen kann.
        let antwort: PlatformTokenAntwort = PlatformConnection {
            streamer_id: 1,
            platform: "twitch".into(),
            platform_user_id: "1".into(),
            platform_login: "s".into(),
            access_token: "acc".into(),
            refresh_token: "GEHEIM".into(),
            scopes: vec![],
            expires_at: zeit("2026-08-27T10:00:00Z"),
            needs_reauth: false,
        }
        .into();
        let json = serde_json::to_string(&antwort).unwrap();
        assert!(json.contains("\"access_token\":\"acc\""));
        assert!(!json.contains("refresh"), "{json}");
        assert!(!json.contains("GEHEIM"), "{json}");
    }

    fn header_mit(token: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(INTERNAL_TOKEN_HEADER, token.parse().unwrap());
        h
    }

    fn loopback() -> ConnectInfo<SocketAddr> {
        ConnectInfo("127.0.0.1:5555".parse().unwrap())
    }

    fn fremd() -> ConnectInfo<SocketAddr> {
        ConnectInfo("10.0.0.7:5555".parse().unwrap())
    }

    #[test]
    fn ohne_token_401() {
        let erwartet = ExpectedToken("geheim".into());
        assert!(!intern_erlaubt(
            Some(&loopback()),
            &HeaderMap::new(),
            Some(&erwartet)
        ));
        assert!(!intern_erlaubt(
            Some(&loopback()),
            &header_mit("falsch"),
            Some(&erwartet)
        ));
        // Ohne konfigurierten Token bleibt die Route zu.
        assert!(!intern_erlaubt(
            Some(&loopback()),
            &header_mit(""),
            Some(&ExpectedToken(String::new()))
        ));
        assert!(!intern_erlaubt(
            Some(&loopback()),
            &header_mit("geheim"),
            None
        ));
        // Der gute Fall, damit der Test nicht nur Verneinungen kennt.
        assert!(intern_erlaubt(
            Some(&loopback()),
            &header_mit("geheim"),
            Some(&erwartet)
        ));
    }

    #[test]
    fn fremder_peer_401() {
        let erwartet = ExpectedToken("geheim".into());
        assert!(!intern_erlaubt(
            Some(&fremd()),
            &header_mit("geheim"),
            Some(&erwartet)
        ));
        // Ohne Peer-Information (kein ConnectInfo) ebenfalls zu: fail-closed.
        assert!(!intern_erlaubt(
            None,
            &header_mit("geheim"),
            Some(&erwartet)
        ));
    }

    #[tokio::test]
    async fn interne_route_ohne_token_antwortet_401_vor_allem_anderen() {
        // Kein Pool noetig: die Auth-Pruefung kommt vor jedem DB-Zugriff. Ein
        // Pool, der nie verbindet, reicht.
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://nie:nie@127.0.0.1:1/nie")
            .unwrap();
        let resp = internal_platform_token_handler(
            State(pool),
            Some(fremd()),
            Some(Extension(ExpectedToken("geheim".into()))),
            None,
            header_mit("geheim"),
            Query(PlatformTokenQuery {
                streamer: Some(1),
                platform: Some("twitch".into()),
            }),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    // ── mit DB ─────────────────────────────────────────────────────────────

    async fn maybe_pool() -> Option<PgPool> {
        if std::env::var("TB_TEST_REQUIRE_DB").as_deref() != Ok("1") {
            return None;
        }
        let url = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let schema = crate::auth::session::test_schema_name("platform_connect");
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
            "CREATE TABLE IF NOT EXISTS dashboard_sessions (
                session_id   TEXT NOT NULL PRIMARY KEY,
                session_type TEXT NOT NULL,
                payload_enc  BYTEA NOT NULL,
                created_at   DOUBLE PRECISION NOT NULL,
                expires_at   DOUBLE PRECISION NOT NULL
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        // Dasselbe DDL wie in Produktion, im Testschema.
        sqlx::raw_sql(MIGRATION).execute(&pool).await.unwrap();
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

    async fn seed_state(state: &DashboardAuthState, streamer_id: i64) -> String {
        let token = format!("pc-{}", tb_crypto::random_urlsafe_token(8));
        state
            .save_platform_connect_state(
                &token,
                &PlatformConnectState {
                    redirect_uri: "https://x.test/cb".into(),
                    streamer_id,
                    twitch_login: "streamerin".into(),
                    platform: "twitch".into(),
                },
            )
            .await
            .unwrap();
        token
    }

    fn query(code: &str, state: &str) -> ConnectCallbackQuery {
        ConnectCallbackQuery {
            code: Some(code.into()),
            state: Some(state.into()),
            error: None,
        }
    }

    #[tokio::test]
    async fn callback_speichert_verschluesselt() {
        let pool = pool_oder_ende!();
        let state = DashboardAuthState::new(pool.clone(), FERNET_KEY.into());
        let config = config_mit(Arc::new(FakePlattform::neu("4242")));
        let store = PlatformConnectionStore::new(pool.clone(), config.cipher.clone());
        let jetzt = zeit("2026-08-27T10:00:00Z");
        let token = seed_state(&state, 4242).await;

        let ergebnis =
            callback_verarbeiten(&state, &config, &store, 4242, &query("code", &token), jetzt)
                .await;
        assert_eq!(ergebnis, CallbackErgebnis::Gespeichert);

        // Roh in der Tabelle: kein Klartext-Token.
        let (acc, refr, login, reauth): (Vec<u8>, Vec<u8>, String, bool) = sqlx::query_as(
            "SELECT access_token_enc, refresh_token_enc, platform_login, needs_reauth \
             FROM platform_connections WHERE streamer_id = 4242 AND platform = 'twitch'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(!acc.windows(5).any(|w| w == b"acc-1"));
        assert!(!refr.windows(5).any(|w| w == b"ref-1"));
        assert_eq!(login, "streamerin");
        assert!(!reauth);

        // Entschluesselt ueber den Store kommt es wieder heraus.
        let geladen = store.load(4242, "twitch").await.unwrap().unwrap();
        assert_eq!(geladen.access_token, "acc-1");
        assert_eq!(geladen.refresh_token, "ref-1");
        assert_eq!(geladen.expires_at, jetzt + chrono::Duration::seconds(14000));
        assert_eq!(
            geladen.scopes,
            CHAT_SCOPES
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        );

        // Der State ist verbraucht: Replay scheitert.
        let replay =
            callback_verarbeiten(&state, &config, &store, 4242, &query("code", &token), jetzt)
                .await;
        assert_eq!(replay, CallbackErgebnis::StateUngueltig);
    }

    #[tokio::test]
    async fn callback_lehnt_fremde_identitaet_ab() {
        let pool = pool_oder_ende!();
        let state = DashboardAuthState::new(pool.clone(), FERNET_KEY.into());
        let store = PlatformConnectionStore::new(pool.clone(), cipher());
        let jetzt = zeit("2026-08-27T10:00:00Z");

        // 1. Der Grant kommt von einem anderen Twitch-Konto als dem Streamer.
        let config = config_mit(Arc::new(FakePlattform::neu("9999")));
        let token = seed_state(&state, 4242).await;
        let ergebnis =
            callback_verarbeiten(&state, &config, &store, 4242, &query("code", &token), jetzt)
                .await;
        assert_eq!(ergebnis, CallbackErgebnis::FremdesKonto);
        assert!(store.load(4242, "twitch").await.unwrap().is_none());

        // 2. Der State gehoert zu einer anderen Session.
        let config = config_mit(Arc::new(FakePlattform::neu("4242")));
        let token = seed_state(&state, 1111).await;
        let ergebnis =
            callback_verarbeiten(&state, &config, &store, 4242, &query("code", &token), jetzt)
                .await;
        assert_eq!(ergebnis, CallbackErgebnis::FremdeSession);
        assert!(store.load(4242, "twitch").await.unwrap().is_none());
        assert!(store.load(1111, "twitch").await.unwrap().is_none());
    }

    /// Ohne Scope-Liste oder ohne `user:write:chat` wird nichts gespeichert:
    /// ein Dock, das nie senden kann, waere sonst "verbunden".
    #[tokio::test]
    async fn callback_lehnt_grant_ohne_chat_scopes_ab() {
        let pool = pool_oder_ende!();
        let state = DashboardAuthState::new(pool.clone(), FERNET_KEY.into());
        let jetzt = zeit("2026-08-27T10:00:00Z");
        let faelle: [(&[&str], CallbackErgebnis); 2] = [
            (&[], CallbackErgebnis::TauschFehlgeschlagen),
            (&["user:read:chat"], CallbackErgebnis::ScopesFehlen),
        ];
        for (scopes, erwartet) in faelle {
            let config = config_mit(Arc::new(FakePlattform::neu("4343").mit_tausch_scopes(scopes)));
            let store = PlatformConnectionStore::new(pool.clone(), config.cipher.clone());
            sqlx::query("DELETE FROM platform_connections WHERE streamer_id = 4343")
                .execute(&pool)
                .await
                .unwrap();
            let token = seed_state(&state, 4343).await;
            let ergebnis =
                callback_verarbeiten(&state, &config, &store, 4343, &query("code", &token), jetzt)
                    .await;
            assert_eq!(ergebnis, erwartet, "{scopes:?}");
            assert!(store.load(4343, "twitch").await.unwrap().is_none());
        }
        assert!(!chat_scopes_fehlen(
            &["user:write:chat".to_string(), "user:read:chat".to_string()]
        ));
    }

    async fn verbindung_anlegen(
        store: &PlatformConnectionStore,
        streamer_id: i64,
        expires_in: i64,
        jetzt: DateTime<Utc>,
    ) {
        store
            .upsert(
                streamer_id,
                "twitch",
                &streamer_id.to_string(),
                "streamerin",
                &tokens("acc-alt", "ref-alt", expires_in),
                jetzt,
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn kein_refresh_wenn_frisch() {
        let pool = pool_oder_ende!();
        let store = PlatformConnectionStore::new(pool, cipher());
        let fake = FakePlattform::neu("1");
        let jetzt = zeit("2026-08-27T10:00:00Z");
        verbindung_anlegen(&store, 1, 3600, jetzt).await;

        let v = refresh_if_needed(&store, &fake, 1, "twitch", jetzt)
            .await
            .unwrap();
        assert_eq!(v.access_token, "acc-alt");
        assert_eq!(fake.refresh_anzahl(), 0);
    }

    #[tokio::test]
    async fn refresh_bei_ablauf_ersetzt_beide_tokens() {
        let pool = pool_oder_ende!();
        let store = PlatformConnectionStore::new(pool, cipher());
        let fake = FakePlattform::neu("2");
        let jetzt = zeit("2026-08-27T10:00:00Z");
        // Noch 5 Minuten: unter dem Vorlauf von 10.
        verbindung_anlegen(&store, 2, 300, jetzt).await;

        let v = refresh_if_needed(&store, &fake, 2, "twitch", jetzt)
            .await
            .unwrap();
        assert_eq!(v.access_token, "acc-neu");
        assert_eq!(v.expires_at, jetzt + chrono::Duration::seconds(14000));
        assert_eq!(fake.refresh_anzahl(), 1);

        let geladen = store.load(2, "twitch").await.unwrap().unwrap();
        assert_eq!(geladen.access_token, "acc-neu");
        assert_eq!(geladen.refresh_token, "ref-neu");
        assert!(!geladen.needs_reauth);
    }

    #[tokio::test]
    async fn refresh_fehler_setzt_needs_reauth() {
        let pool = pool_oder_ende!();
        let store = PlatformConnectionStore::new(pool, cipher());
        let fake = FakePlattform::neu("3").mit_refresh(Err(UserTokenError::InvalidGrant));
        let jetzt = zeit("2026-08-27T10:00:00Z");
        verbindung_anlegen(&store, 3, 60, jetzt).await;

        let fehler = refresh_if_needed(&store, &fake, 3, "twitch", jetzt)
            .await
            .unwrap_err();
        assert_eq!(fehler, TokenFehler::NeuVerbinden);
        let geladen = store.load(3, "twitch").await.unwrap().unwrap();
        assert!(geladen.needs_reauth);
        // Der alte Token bleibt stehen, nichts wird geloescht.
        assert_eq!(geladen.refresh_token, "ref-alt");
        assert_eq!(
            store.status_liste(3).await.unwrap(),
            vec![("twitch".to_string(), "neu_verbinden")]
        );
    }

    /// Zwei Abrufe der internen Route zur selben Zeit (etwa zwei Relay-Adapter
    /// beim Session-Start) duerfen den Refresh-Token nur einmal einloesen:
    /// Twitch macht ihn beim ersten Refresh ungueltig, der zweite Einloeser
    /// bekaeme invalid_grant und wuerde die Verbindung faelschlich auf
    /// needs_reauth setzen.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn paralleler_abruf_loest_den_refresh_nur_einmal_ein() {
        let pool = pool_oder_ende!();
        let store = Arc::new(PlatformConnectionStore::new(pool, cipher()));
        let fake = Arc::new(FakePlattform::neu("40").mit_dauer(Duration::from_millis(300)));
        let jetzt = zeit("2026-08-27T10:00:00Z");
        verbindung_anlegen(&store, 40, 60, jetzt).await;

        // Jeder Lauf mit eigener Store-Instanz, wie in der Produktion (der
        // Handler baut den Store je Request): das Schloss muss prozessweit
        // greifen, nicht nur innerhalb einer Instanz.
        let lauf = |store: Arc<PlatformConnectionStore>, fake: Arc<FakePlattform>| {
            let eigener = PlatformConnectionStore::new(store.pool.clone(), store.cipher.clone());
            tokio::spawn(async move {
                refresh_if_needed(&eigener, fake.as_ref(), 40, "twitch", jetzt).await
            })
        };
        let a = lauf(store.clone(), fake.clone());
        let b = lauf(store.clone(), fake.clone());
        let (ra, rb) = (a.await.unwrap(), b.await.unwrap());
        assert!(ra.is_ok(), "{ra:?}");
        assert!(rb.is_ok(), "{rb:?}");
        assert_eq!(ra.unwrap().access_token, "acc-neu");
        assert_eq!(rb.unwrap().access_token, "acc-neu");
        assert_eq!(
            fake.refresh_anzahl(),
            1,
            "Refresh-Token darf nur einmal eingeloest werden"
        );
        let geladen = store.load(40, "twitch").await.unwrap().unwrap();
        assert_eq!(geladen.refresh_token, "ref-neu");
        assert!(!geladen.needs_reauth);
        // Die Schloss-Karte raeumt hinter sich auf.
        assert!(!REFRESH_SCHLOESSER
            .lock()
            .unwrap()
            .contains_key(&(40, "twitch".to_string())));
    }

    #[tokio::test]
    async fn periodischer_lauf_erneuert_nur_faellige() {
        let pool = pool_oder_ende!();
        let store = PlatformConnectionStore::new(pool, cipher());
        let fake = FakePlattform::neu("x");
        let jetzt = zeit("2026-08-27T10:00:00Z");
        verbindung_anlegen(&store, 10, 120, jetzt).await; // faellig
        verbindung_anlegen(&store, 11, 7200, jetzt).await; // frisch

        let n = refresh_faellige(&store, &fake, jetzt).await.unwrap();
        assert_eq!(n, 1);
        assert_eq!(fake.refresh_anzahl(), 1);
        assert_eq!(
            store
                .load(10, "twitch")
                .await
                .unwrap()
                .unwrap()
                .access_token,
            "acc-neu"
        );
        assert_eq!(
            store
                .load(11, "twitch")
                .await
                .unwrap()
                .unwrap()
                .access_token,
            "acc-alt"
        );
    }

    #[tokio::test]
    async fn liefert_access_token_ohne_refresh_token() {
        let pool = pool_oder_ende!();
        let store = PlatformConnectionStore::new(pool, cipher());
        let fake = FakePlattform::neu("20");
        let jetzt = zeit("2026-08-27T10:00:00Z");
        verbindung_anlegen(&store, 20, 3600, jetzt).await;

        let antwort = platform_token_antwort(&store, &fake, 20, "twitch", jetzt)
            .await
            .unwrap_or_else(|_| panic!("Antwort erwartet"));
        let json = serde_json::to_value(&antwort).unwrap();
        assert_eq!(json["access_token"], "acc-alt");
        assert_eq!(json["platform_user_id"], "20");
        assert_eq!(json["platform_login"], "streamerin");
        assert_eq!(
            json["expires_at"],
            serde_json::to_value(jetzt + chrono::Duration::seconds(3600)).unwrap()
        );
        assert_eq!(json["scopes"].as_array().unwrap().len(), 2);
        // REQ-7: kein Refresh-Token, weder als Feld noch als Wert.
        let text = json.to_string();
        assert!(json.get("refresh_token").is_none());
        assert!(!text.contains("ref-alt"), "{text}");
        assert!(!text.contains("refresh"), "{text}");
    }

    #[tokio::test]
    async fn needs_reauth_409_und_unbekannt_404() {
        let pool = pool_oder_ende!();
        let store = PlatformConnectionStore::new(pool, cipher());
        let fake = FakePlattform::neu("30");
        let jetzt = zeit("2026-08-27T10:00:00Z");
        verbindung_anlegen(&store, 30, 3600, jetzt).await;
        store.mark_reauth(30, "twitch").await.unwrap();

        let r = platform_token_antwort(&store, &fake, 30, "twitch", jetzt).await;
        assert_eq!(r.err().map(|r| r.status()), Some(StatusCode::CONFLICT));

        let r = platform_token_antwort(&store, &fake, 31, "twitch", jetzt).await;
        assert_eq!(r.err().map(|r| r.status()), Some(StatusCode::NOT_FOUND));
    }
}
