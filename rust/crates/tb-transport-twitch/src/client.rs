//! HelixClient — dünner reqwest-Wrapper für Twitch Helix.
//!
//! Hält einen geteilten Arc<reqwest::Client> und erneuert den App-Token
//! automatisch bei Bedarf (in-memory, kein persistenter Cache).

use crate::token::{unix_now, AppTokenManager, TokenError};
use reqwest::{Client, RequestBuilder};
use serde::de::DeserializeOwned;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::Mutex;

/// Cooldown nach `invalid_client` auf dem **User-Token**-OAuth-Pfad
/// (Python `auth.py` `_block_client_auth`, `cooldown_seconds=900.0` — 15 min).
/// Während dieser Zeit unterdrückt der Client weitere User-Token-Exchanges/
/// -Refreshes, statt Twitch mit aussichtslosen Requests zu bombardieren.
const USER_AUTH_BLOCK_COOLDOWN_SECS: i64 = 900;

/// Request-Timeout für alle Helix-Aufrufe.
///
/// Python-Parität (`twitch_api.py:59`: `aiohttp.ClientTimeout(total=20)`):
/// langsame-aber-am-Ende-OK-Antworten von Twitch (große `/users`-/`/streams`-
/// Batches) liefen unter Python bis 20 s durch; bei 10 s würden sie hart
/// timeouten. Zusammen mit dem 5xx/transient-Retry ([`RETRY_BACKOFF_BASE_MS`])
/// bildet das die Helix-Resilienz von Python nach.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

/// Anzahl Versuche für Helix-GET/POST inkl. Erstversuch (Python `range(3)`).
const MAX_HELIX_ATTEMPTS: usize = 3;

/// Backoff-Basis in Millisekunden: Wartezeit vor Versuch `n` (0-basiert) ist
/// `RETRY_BACKOFF_BASE_MS * (n + 1)` (Python `0.5 * (attempt + 1)` Sekunden →
/// 500/1000 ms vor dem 2./3. Versuch).
const RETRY_BACKOFF_BASE_MS: u64 = 500;

/// HTTP-Status, die als transiente Twitch-Upstream-Aussetzer gelten und einen
/// Retry auslösen (Python `if r.status in {500, 502, 503, 504}`).
fn is_transient_status(status: u16) -> bool {
    matches!(status, 500 | 502 | 503 | 504)
}

/// Ist ein `reqwest::Error` ein transienter Netz-/Timeout-/Connect-Fehler, den
/// Python via `except (TimeoutError, aiohttp.ClientError, OSError)` erneut
/// versucht hätte? (Status-Fehler werden hier NICHT als transient gewertet —
/// die deckt [`is_transient_status`] auf Response-Ebene ab.)
fn is_transient_error(err: &reqwest::Error) -> bool {
    err.is_timeout() || err.is_connect() || err.is_request()
}

/// Fehlertyp für Helix-Operationen.
#[derive(Debug, Error)]
pub enum HelixError {
    #[error("Token-Fehler: {0}")]
    Token(#[from] TokenError),
    #[error("HTTP-Fehler: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Helix-Status {status}")]
    Status { status: u16 },
    /// 403 auf `/chat/chatters` — der angefragte Account ist kein Moderator
    /// (bzw. der Token hat `moderator:read:chatters` nicht). Eigener Zweig,
    /// damit der Poller einen Mod-Self-Heal anstoßen kann (Block-6).
    #[error("kein Moderator im Ziel-Channel (403)")]
    NotModerator,
}

/// Konfiguration für den HelixClient.
#[derive(Debug, Clone)]
pub struct HelixConfig {
    /// Twitch Client-ID.
    pub client_id: String,
    /// Twitch Client-Secret.
    pub client_secret: String,
    /// OAuth-Token-URL (überschreibbar für Tests).
    pub token_url: String,
    /// Helix-Basis-URL (überschreibbar für Tests).
    pub helix_base: String,
}

impl HelixConfig {
    /// Erstellt eine HelixConfig mit den Standard-Twitch-URLs.
    pub fn new(client_id: impl Into<String>, client_secret: impl Into<String>) -> Self {
        Self {
            client_id: client_id.into(),
            client_secret: client_secret.into(),
            token_url: "https://id.twitch.tv/oauth2/token".to_string(),
            helix_base: "https://api.twitch.tv/helix".to_string(),
        }
    }
}

/// HTTP-Client für die Twitch Helix API.
///
/// Verwaltet den App-Token intern (auto-refresh bei Ablauf).
#[derive(Clone)]
pub struct HelixClient {
    http: Arc<Client>,
    config: HelixConfig,
    /// Zentraler App-Token-Manager: Cache + In-Flight-Dedupe (B18-5) +
    /// `invalid_client`-Circuit-Breaker (B18-3).
    token: AppTokenManager,
    /// Kategorie-Name (lowercase) → game_id (Python: `_category_cache`).
    pub(crate) category_cache: Arc<Mutex<std::collections::HashMap<String, String>>>,
    /// Cooldown-Deadline (Unix-Sek., `0` = nicht gesperrt) für den **User-Token**-
    /// OAuth-Pfad nach einer `invalid_client`-Ablehnung (P2.33, Python
    /// `_client_auth_blocked_until`). Bewusst getrennt vom App-Token-Breaker in
    /// [`AppTokenManager`]: der User-Token-Pfad (Exchange/Refresh/Sweep) nutzt
    /// NICHT den App-Token, also braucht er seinen eigenen Schalter. Lock-frei,
    /// damit `is_client_auth_blocked` synchron im Sweep-Gate abgefragt werden kann.
    pub(crate) user_auth_blocked_until: Arc<AtomicI64>,
}

impl HelixClient {
    /// Erstellt einen neuen HelixClient.
    pub fn new(config: HelixConfig) -> Result<Self, reqwest::Error> {
        let http = Arc::new(Client::builder().timeout(REQUEST_TIMEOUT).build()?);
        let token = AppTokenManager::new(
            http.clone(),
            config.token_url.clone(),
            config.client_id.clone(),
            config.client_secret.clone(),
        );
        Ok(Self {
            http,
            config,
            token,
            category_cache: Arc::new(Mutex::new(std::collections::HashMap::new())),
            user_auth_blocked_until: Arc::new(AtomicI64::new(0)),
        })
    }

    /// Ist der **User-Token**-OAuth-Pfad nach einer `invalid_client`-Ablehnung
    /// gesperrt (15-Min-Cooldown, P2.33)? Synchron + lock-frei, damit der
    /// Hintergrund-Sweep (`refresh_all_due`) vor dem Iterieren aller Streamer
    /// kurzschließen kann (Python `if self.is_client_auth_blocked(): return 0`).
    pub fn is_client_auth_blocked(&self) -> bool {
        let until = self.user_auth_blocked_until.load(Ordering::Acquire);
        until != 0 && unix_now() < until
    }

    /// Setzt den 15-Min-`invalid_client`-Cooldown für den User-Token-Pfad
    /// (Python `_block_client_auth`).
    pub(crate) fn block_client_auth(&self) {
        self.user_auth_blocked_until.store(
            unix_now() + USER_AUTH_BLOCK_COOLDOWN_SECS,
            Ordering::Release,
        );
    }

    /// Hebt den User-Token-`invalid_client`-Cooldown auf (Python: nach
    /// erfolgreichem Exchange/Refresh `_client_auth_blocked_until = 0.0`).
    pub(crate) fn clear_client_auth_block(&self) {
        self.user_auth_blocked_until.store(0, Ordering::Release);
    }

    /// Gibt den aktuellen Access-Token zurück, holt bei Bedarf einen neuen.
    pub async fn access_token(&self) -> Result<String, HelixError> {
        Ok(self.token.access_token().await?)
    }

    /// Verwirft den gecachten App-Token, damit der nächste Helix-Request ihn
    /// frisch per client_credentials abruft.
    pub async fn invalidate_app_token(&self) {
        self.token.invalidate().await;
    }

    /// Ist die App-Auth nach einer `invalid_client`-Ablehnung gesperrt
    /// (15-Min-Cooldown)? Synchron + lock-frei, damit der `tb-bot`-Adapter es
    /// direkt im synchronen `StreamSource::is_auth_blocked`-Trait-Gate des
    /// Pollers (monitoring.py:1207) verdrahten kann, sodass der Tick
    /// Helix-Requests überspringt.
    pub fn is_auth_blocked(&self) -> bool {
        self.token.is_auth_blocked()
    }

    /// Erstellt einen vorbereiteten GET-Request an einen Helix-Endpunkt.
    ///
    /// `path` — z. B. `"/streams"` (ohne Basis-URL).
    pub async fn get(&self, path: &str) -> Result<RequestBuilder, HelixError> {
        let token = self.access_token().await?;
        let url = format!("{}{}", self.config.helix_base, path);
        Ok(self
            .http
            .get(&url)
            .header("Client-Id", &self.config.client_id)
            .header("Authorization", format!("Bearer {token}")))
    }

    /// Erstellt einen vorbereiteten POST-Request (App-Token; per
    /// `.header("Authorization", …)` überschreibbar).
    pub async fn post(&self, path: &str) -> Result<RequestBuilder, HelixError> {
        let token = self.access_token().await?;
        let url = format!("{}{}", self.config.helix_base, path);
        Ok(self
            .http
            .post(&url)
            .header("Client-Id", &self.config.client_id)
            .header("Authorization", format!("Bearer {token}")))
    }

    /// Erstellt einen vorbereiteten DELETE-Request.
    pub async fn delete(&self, path: &str) -> Result<RequestBuilder, HelixError> {
        let token = self.access_token().await?;
        let url = format!("{}{}", self.config.helix_base, path);
        Ok(self
            .http
            .delete(&url)
            .header("Client-Id", &self.config.client_id)
            .header("Authorization", format!("Bearer {token}")))
    }

    /// Sendet einen vorbereiteten Helix-Request mit Retry-with-Backoff bei
    /// transienten Fehlern — Python-Parität (`twitch_api.py` `_get`/`_post`:
    /// `for attempt in range(3)`).
    ///
    /// Retry-Auslöser (bis zu [`MAX_HELIX_ATTEMPTS`] Versuche, Backoff
    /// `RETRY_BACKOFF_BASE_MS * (attempt + 1)` vor dem nächsten Versuch):
    /// - HTTP 500/502/503/504 ([`is_transient_status`]),
    /// - transiente reqwest-Fehler (Timeout/Connect/Request-Build,
    ///   [`is_transient_error`]).
    ///
    /// Alle anderen Antworten (2xx, 4xx wie 401/403/429) sowie nicht-transiente
    /// Fehler werden sofort zurückgegeben — kein blindes Retry auf Auth-/Rate-
    /// Limit-Fehler. Der `builder` muss klonbar sein (kein Stream-Body); für
    /// Helix-GET/POST mit Query/JSON ist das stets der Fall.
    pub async fn send_with_retry(
        &self,
        builder: RequestBuilder,
    ) -> Result<reqwest::Response, HelixError> {
        let mut last_status: Option<u16> = None;
        for attempt in 0..MAX_HELIX_ATTEMPTS {
            // Vor jedem Versuch frisch klonen — `send()` konsumiert den Builder.
            let Some(attempt_builder) = builder.try_clone() else {
                // Nicht klonbar (Stream-Body): einmal senden, kein Retry möglich.
                return Ok(builder.send().await?);
            };

            match attempt_builder.send().await {
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    if is_transient_status(status) && attempt + 1 < MAX_HELIX_ATTEMPTS {
                        last_status = Some(status);
                        self.backoff(attempt).await;
                        continue;
                    }
                    return Ok(resp);
                }
                Err(err) => {
                    if is_transient_error(&err) && attempt + 1 < MAX_HELIX_ATTEMPTS {
                        self.backoff(attempt).await;
                        continue;
                    }
                    return Err(err.into());
                }
            }
        }
        // Schleife erschöpft (nur transiente 5xx): letzten Status als Fehler.
        Err(HelixError::Status {
            status: last_status.unwrap_or(0),
        })
    }

    /// Schläft `RETRY_BACKOFF_BASE_MS * (attempt + 1)` ms vor dem nächsten
    /// Versuch (Python `await asyncio.sleep(0.5 * (attempt + 1))`).
    async fn backoff(&self, attempt: usize) {
        let delay = Duration::from_millis(RETRY_BACKOFF_BASE_MS * (attempt as u64 + 1));
        tokio::time::sleep(delay).await;
    }

    /// Roh-Zugriff für Schwester-Module (z. B. den User-Token-Refresh).
    pub(crate) fn http_client(&self) -> &Client {
        &self.http
    }

    pub(crate) fn helix_config(&self) -> &HelixConfig {
        &self.config
    }

    /// POST mit einem **User-Token** (Client-Id + dieser Bearer, KEIN App-Token).
    /// Für Endpoints, die die Identität des Broadcasters brauchen (z. B. `/raids`).
    pub fn post_with_user_token(&self, path: &str, user_token: &str) -> RequestBuilder {
        let url = format!("{}{}", self.config.helix_base, path);
        self.http
            .post(&url)
            .header("Client-Id", &self.config.client_id)
            .header("Authorization", format!("Bearer {user_token}"))
    }

    /// GET mit einem **User-Token** (Client-Id + dieser Bearer, KEIN App-Token).
    pub fn get_with_user_token(&self, path: &str, user_token: &str) -> RequestBuilder {
        let url = format!("{}{}", self.config.helix_base, path);
        self.http
            .get(&url)
            .header("Client-Id", &self.config.client_id)
            .header("Authorization", format!("Bearer {user_token}"))
    }

    /// DELETE mit einem **User-Token** (analog [`Self::post_with_user_token`]).
    pub fn delete_with_user_token(&self, path: &str, user_token: &str) -> RequestBuilder {
        let url = format!("{}{}", self.config.helix_base, path);
        self.http
            .delete(&url)
            .header("Client-Id", &self.config.client_id)
            .header("Authorization", format!("Bearer {user_token}"))
    }

    /// Erstellt einen Clip aus dem aktuellen Stream-Buffer (Helix `POST /clips`).
    ///
    /// Braucht ein **User-Token mit `clips:edit`-Scope** (i. d. R. der Broadcaster).
    /// Titel/Dauer akzeptiert Helix hier nicht — der Clip kommt aus dem Live-Buffer
    /// (~letzte 30 s); der Titel wird nur in der Chat-Antwort verwendet.
    /// `Ok(None)` = Twitch lieferte kein Clip-Objekt zurück.
    pub async fn create_clip(
        &self,
        broadcaster_id: &str,
        user_token: &str,
    ) -> Result<Option<ClipInfo>, HelixError> {
        let path = format!("/clips?broadcaster_id={broadcaster_id}&has_delay=false");
        let resp = self
            .send_with_retry(self.post_with_user_token(&path, user_token))
            .await?;
        let body: HelixClipsResponse = check_status_and_json(resp).await?;
        Ok(body.data.into_iter().next())
    }

    /// Holt Twitch-User-Infos für eine Liste von Logins.
    ///
    /// Gibt eine Map `login (lowercase) → TwitchUser` zurück.
    /// Leere Logins werden ignoriert. Unbekannte Logins erscheinen nicht in der Map.
    pub async fn get_users(
        &self,
        logins: &[&str],
    ) -> Result<std::collections::HashMap<String, TwitchUser>, HelixError> {
        let mut out = std::collections::HashMap::new();
        // Helix /users akzeptiert max. 100 login-Parameter pro Request — größere
        // Listen müssen gechunkt werden (Python batcht ebenso), sonst lehnt Twitch ab.
        for chunk in logins.chunks(100) {
            let params: Vec<(&str, &str)> = chunk
                .iter()
                .map(|l| l.trim())
                .filter(|l| !l.is_empty())
                .map(|l| ("login", l))
                .collect();
            if params.is_empty() {
                continue;
            }
            let builder = self.get("/users").await?;
            let resp = self.send_with_retry(builder.query(&params)).await?;
            let body: HelixUsersResponse = check_status_and_json(resp).await?;
            for u in body.data {
                out.insert(u.login.to_lowercase(), u);
            }
        }
        Ok(out)
    }

    /// Holt Twitch-User-Infos für eine Liste numerischer User-IDs.
    ///
    /// Gibt eine Map `id → TwitchUser` zurück (Login und Anzeigename inklusive).
    /// Leere IDs werden ignoriert. Unbekannte IDs erscheinen nicht in der Map.
    /// Gegenstück zu [`get_users`](Self::get_users), nur mit `id`- statt
    /// `login`-Parametern; Helix erlaubt beide an `/users`.
    pub async fn get_users_by_id(
        &self,
        ids: &[&str],
    ) -> Result<std::collections::HashMap<String, TwitchUser>, HelixError> {
        let mut out = std::collections::HashMap::new();
        // Helix /users akzeptiert max. 100 id-Parameter pro Request, wie bei login.
        for chunk in ids.chunks(100) {
            let params: Vec<(&str, &str)> = chunk
                .iter()
                .map(|l| l.trim())
                .filter(|l| !l.is_empty())
                .map(|l| ("id", l))
                .collect();
            if params.is_empty() {
                continue;
            }
            let builder = self.get("/users").await?;
            let resp = self.send_with_retry(builder.query(&params)).await?;
            let body: HelixUsersResponse = check_status_and_json(resp).await?;
            for u in body.data {
                out.insert(u.id.clone(), u);
            }
        }
        Ok(out)
    }
}

/// Prüft den HTTP-Status einer Helix-Response und deserialisiert den Body.
///
/// Bei non-2xx wird `HelixError::Status` zurückgegeben, bevor JSON geparst wird.
/// So entsteht bei 429/5xx ein klarer Status-Fehler statt eines serde-Parse-Fehlers.
pub(crate) async fn check_status_and_json<T: DeserializeOwned>(
    resp: reqwest::Response,
) -> Result<T, HelixError> {
    let status = resp.status();
    if !status.is_success() {
        return Err(HelixError::Status {
            status: status.as_u16(),
        });
    }
    Ok(resp.json::<T>().await?)
}

/// Twitch-User-Daten aus der Helix-API.
#[derive(Debug, serde::Deserialize)]
pub struct TwitchUser {
    pub id: String,
    pub login: String,
    pub display_name: String,
    #[serde(default)]
    pub profile_image_url: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct HelixUsersResponse {
    pub data: Vec<TwitchUser>,
}

/// Clip-Daten aus `POST /clips` (Helix liefert id + edit_url).
#[derive(Debug, serde::Deserialize)]
pub struct ClipInfo {
    pub id: String,
    #[serde(default)]
    pub edit_url: String,
}

#[derive(Debug, serde::Deserialize)]
struct HelixClipsResponse {
    data: Vec<ClipInfo>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_string_contains, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_config(token_url: &str, helix_base: &str) -> HelixConfig {
        HelixConfig {
            client_id: "test-client-id".to_string(),
            client_secret: "test-client-secret".to_string(),
            token_url: token_url.to_string(),
            helix_base: helix_base.to_string(),
        }
    }

    #[tokio::test]
    async fn token_fetch_sendet_form_encoded_body() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .and(body_string_contains("grant_type=client_credentials"))
            .and(body_string_contains("client_id=test-client-id"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "tok-abc",
                "expires_in": 3600
            })))
            .mount(&server)
            .await;

        let config = test_config(
            &format!("{}/oauth2/token", server.uri()),
            &format!("{}/helix", server.uri()),
        );
        let client = HelixClient::new(config).unwrap();
        let token = client.access_token().await.unwrap();
        assert_eq!(token, "tok-abc");
    }

    #[tokio::test]
    async fn helix_request_setzt_korrekte_header() {
        let server = MockServer::start().await;

        // Token-Endpunkt
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "tok-helix",
                "expires_in": 3600
            })))
            .mount(&server)
            .await;

        // Helix-Endpunkt
        Mock::given(method("GET"))
            .and(path("/helix/streams"))
            .and(header("Client-Id", "test-client-id"))
            .and(header("Authorization", "Bearer tok-helix"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let config = test_config(
            &format!("{}/oauth2/token", server.uri()),
            &format!("{}/helix", server.uri()),
        );
        let client = HelixClient::new(config).unwrap();
        let builder = client.get("/streams").await.unwrap();
        let resp = builder.send().await.unwrap();
        assert_eq!(resp.status(), 200);
    }

    #[test]
    fn is_transient_status_nur_5xx_blips() {
        assert!(is_transient_status(500));
        assert!(is_transient_status(502));
        assert!(is_transient_status(503));
        assert!(is_transient_status(504));
        // Nicht-transiente: kein Retry (Auth/Rate-Limit/Erfolg).
        assert!(!is_transient_status(200));
        assert!(!is_transient_status(401));
        assert!(!is_transient_status(403));
        assert!(!is_transient_status(429));
        assert!(!is_transient_status(501));
    }

    /// P2.26: Liefert Twitch erst 503, dann 200, recovered der Client
    /// transparent — der zweite Request landet beim Mock (Python `range(3)`).
    #[tokio::test]
    async fn get_users_retry_nach_503_dann_200() {
        use wiremock::matchers::query_param;
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "tok", "expires_in": 3600
            })))
            .mount(&server)
            .await;

        // Erst 503 (genau 1x), dann 200 mit Userdaten. wiremock wertet Mocks in
        // Definitionsreihenfolge mit Limit aus: der 503-Mock greift nur einmal,
        // der Folge-Request fällt auf den 200-Mock.
        Mock::given(method("GET"))
            .and(path("/helix/users"))
            .and(query_param("login", "nani"))
            .respond_with(ResponseTemplate::new(503))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/helix/users"))
            .and(query_param("login", "nani"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"id": "1", "login": "nani", "display_name": "Nani"}]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let config = test_config(
            &format!("{}/oauth2/token", server.uri()),
            &format!("{}/helix", server.uri()),
        );
        let client = HelixClient::new(config).unwrap();
        let out = client.get_users(&["nani"]).await.unwrap();
        assert!(out.contains_key("nani"));
        // Beide Mocks (503 + 200) müssen genau einmal getroffen worden sein.
        server.verify().await;
    }

    /// P2.26: Persistente 5xx über alle Versuche → am Ende `HelixError::Status`,
    /// nachdem [`MAX_HELIX_ATTEMPTS`] Versuche erschöpft sind.
    #[tokio::test]
    async fn get_users_persistentes_503_endet_als_status_fehler() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "tok", "expires_in": 3600
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/helix/users"))
            .respond_with(ResponseTemplate::new(503))
            .expect(MAX_HELIX_ATTEMPTS as u64)
            .mount(&server)
            .await;

        let config = test_config(
            &format!("{}/oauth2/token", server.uri()),
            &format!("{}/helix", server.uri()),
        );
        let client = HelixClient::new(config).unwrap();
        let err = client.get_users(&["nani"]).await.unwrap_err();
        assert!(matches!(err, HelixError::Status { status: 503 }));
        server.verify().await;
    }

    /// P2.26: Ein 403 ist NICHT transient — sofortiger Status-Fehler, kein
    /// Retry (genau ein Request beim Mock).
    #[tokio::test]
    async fn get_users_403_kein_retry() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "tok", "expires_in": 3600
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/helix/users"))
            .respond_with(ResponseTemplate::new(403))
            .expect(1)
            .mount(&server)
            .await;

        let config = test_config(
            &format!("{}/oauth2/token", server.uri()),
            &format!("{}/helix", server.uri()),
        );
        let client = HelixClient::new(config).unwrap();
        let err = client.get_users(&["nani"]).await.unwrap_err();
        assert!(matches!(err, HelixError::Status { status: 403 }));
        server.verify().await;
    }

    #[tokio::test]
    async fn token_wird_nicht_neu_geholt_wenn_noch_gueltig() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "tok-cached",
                "expires_in": 3600
            })))
            .expect(1)
            .mount(&server)
            .await;

        let config = test_config(
            &format!("{}/oauth2/token", server.uri()),
            &format!("{}/helix", server.uri()),
        );
        let client = HelixClient::new(config).unwrap();
        let t1 = client.access_token().await.unwrap();
        let t2 = client.access_token().await.unwrap();
        assert_eq!(t1, t2);
        server.verify().await;
    }
}
