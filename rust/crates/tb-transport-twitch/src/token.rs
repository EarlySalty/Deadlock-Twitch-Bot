//! App-Token-Verwaltung für Twitch client_credentials.
//!
//! Expiry-Logik ist pure (keine Uhrzeit-Abhängigkeit im Struct) —
//! testbar ohne Mock-Clock. HTTP-Fetch gegen wiremock testbar.

use reqwest::Client;
use serde::Deserialize;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;
use tokio::sync::Mutex;

const EXPIRY_MARGIN_SECS: i64 = 60;

/// Cooldown nach `invalid_client`-Ablehnung (Python `_block_auth`,
/// `cooldown_seconds=900.0` — 15 Minuten). Während dieser Zeit unterdrückt der
/// Manager weitere Token-Requests, statt Twitch erfolglos zu bombardieren.
const AUTH_BLOCK_COOLDOWN_SECS: u64 = 900;

/// Fehlertyp für Token-Operationen.
#[derive(Debug, Error)]
pub enum TokenError {
    #[error("HTTP-Fehler beim Token-Abruf: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Token-Response nicht parsebar: {0}")]
    Parse(#[from] serde_json::Error),
    /// Twitch hat mit einem Fehler-Statuscode geantwortet (z. B. 400/401 bei
    /// ungültigen Credentials). `message` enthält die Twitch-Fehlermeldung,
    /// falls vorhanden — niemals den Secret-Wert selbst.
    #[error("Token-Endpunkt antwortete mit HTTP {status}: {message}")]
    HttpStatus { status: u16, message: String },
    /// Die App-Auth ist nach einer `invalid_client`-Ablehnung für 15 Minuten
    /// gesperrt (Circuit-Breaker, Python `is_auth_blocked`/`_block_auth`).
    /// Weitere Token-Requests werden bis zum Ablauf des Cooldowns unterdrückt.
    #[error("Twitch-App-Auth ist gesperrt (invalid_client-Cooldown aktiv)")]
    AuthBlocked,
}

/// In-Memory-Repräsentation eines gültigen App-Tokens.
#[derive(Debug, Clone)]
pub struct AppToken {
    pub access_token: String,
    /// Unix-Zeitstempel (Sekunden), ab dem der Token spätestens erneuert werden muss.
    pub expiry_unix: i64,
}

impl AppToken {
    /// Erstellt einen neuen AppToken.
    ///
    /// `issued_at_unix` + `expires_in` → `expiry_unix`.
    pub fn new(access_token: String, expires_in: u64, issued_at_unix: i64) -> Self {
        let expiry_unix = issued_at_unix + expires_in as i64;
        Self {
            access_token,
            expiry_unix,
        }
    }

    /// Gibt `true` zurück, wenn der Token erneuert werden sollte.
    ///
    /// Erneuerung nötig, wenn `now_unix >= expiry_unix - EXPIRY_MARGIN_SECS`.
    pub fn needs_refresh(&self, now_unix: i64) -> bool {
        now_unix >= self.expiry_unix - EXPIRY_MARGIN_SECS
    }
}

/// Liefert den aktuellen Unix-Zeitstempel in Sekunden.
pub fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Python-Parität: fehlt `expires_in` in der Twitch-Antwort, gilt 3600 s.
fn default_expires_in() -> u64 {
    3600
}

/// Rohe Twitch-Token-Response (nur benötigte Felder).
#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    /// Lebensdauer in Sekunden. Lässt Twitch das Feld weg, fällt serde auf
    /// 3600 zurück (statt das Parsen mit "missing field" abzubrechen).
    #[serde(default = "default_expires_in")]
    expires_in: u64,
}

/// Twitch-Fehler-Body (z. B. `{"status":400,"message":"invalid client"}`).
/// Nur für den Fehlerfall — enthält keine Credentials.
#[derive(Debug, Deserialize, Default)]
struct TokenErrorBody {
    #[serde(default)]
    message: String,
    /// Manche OAuth-Fehler tragen den Hinweis im `error`-Feld statt in `message`.
    #[serde(default)]
    error: String,
}

/// Holt einen neuen App-Token via client_credentials-Grant.
pub async fn fetch_app_token(
    client: &Client,
    token_url: &str,
    client_id: &str,
    client_secret: &str,
) -> Result<AppToken, TokenError> {
    let issued_at = unix_now();
    let resp = client
        .post(token_url)
        .form(&[
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("grant_type", "client_credentials"),
        ])
        .send()
        .await?;

    // Status prüfen bevor JSON geparst wird — bei 401/400 fehlt `access_token`
    // im Body und serde würde mit "missing field" abbrechen statt klarem Fehler.
    let http_status = resp.status();
    let body = resp.text().await?;
    if !http_status.is_success() {
        let message = serde_json::from_str::<TokenErrorBody>(&body)
            .unwrap_or_default()
            .message;
        return Err(TokenError::HttpStatus {
            status: http_status.as_u16(),
            message,
        });
    }
    let parsed: TokenResponse = serde_json::from_str(&body)?;
    Ok(AppToken::new(
        parsed.access_token,
        parsed.expires_in,
        issued_at,
    ))
}

/// Python `is_invalid_client_response` (twitch_auth.py): HTTP 400 + „invalid
/// client" im Roh-Body oder in den JSON-Feldern `message`/`error`. Nur dieser
/// Fall löst den 15-Min-Cooldown aus — andere 4xx/5xx sind transiente Fehler.
fn is_invalid_client(status: u16, body: &str) -> bool {
    if status != 400 {
        return false;
    }
    if body.to_lowercase().contains("invalid client") {
        return true;
    }
    let parsed: TokenErrorBody = serde_json::from_str(body).unwrap_or_default();
    parsed.message.to_lowercase().contains("invalid client")
        || parsed.error.to_lowercase().contains("invalid client")
}

/// Verwaltet den Twitch-App-Token (client_credentials) zentral:
///
/// - **In-Flight-Dedupe** (B18-5): Der einzelne `Mutex` wird über den gesamten
///   Fetch gehalten — parallele Aufrufer warten am Lock und sehen danach den
///   frischen Token, statt jeder einen eigenen Refresh auszulösen (Port von
///   Pythons `asyncio.Lock` in `_ensure_token`).
/// - **Circuit-Breaker** (B18-3): Antwortet Twitch mit `invalid_client`, wird
///   für 15 Minuten ein Cooldown gesetzt; weitere Requests scheitern sofort mit
///   [`TokenError::AuthBlocked`], ohne Twitch zu kontaktieren. Ein erfolgreicher
///   Token-Abruf hebt den Cooldown wieder auf.
///
/// Der Cooldown-Deadline liegt bewusst als lock-freies [`AtomicI64`] (Unix-Sek.,
/// `0` = nicht gesperrt) **neben** dem async-Mutex: So liest das synchrone
/// `StreamSource::is_auth_blocked`-Trait-Gate des Pollers den Zustand ohne den
/// async-Lock und ohne `try_lock`-Glücksspiel (Python `is_auth_blocked` ist
/// ebenfalls ein reiner Zeitvergleich ohne Lock).
#[derive(Clone)]
pub struct AppTokenManager {
    http: Arc<Client>,
    token_url: String,
    client_id: String,
    client_secret: String,
    /// Serialisiert Token-Fetches (In-Flight-Dedupe). Cache des aktuellen Tokens.
    token: Arc<Mutex<Option<AppToken>>>,
    /// Cooldown-Deadline als Unix-Sekunde; `0` = kein Cooldown aktiv.
    blocked_until_unix: Arc<AtomicI64>,
}

impl AppTokenManager {
    /// Erstellt einen neuen Manager. Secrets werden nur in-memory gehalten.
    pub fn new(
        http: Arc<Client>,
        token_url: impl Into<String>,
        client_id: impl Into<String>,
        client_secret: impl Into<String>,
    ) -> Self {
        Self {
            http,
            token_url: token_url.into(),
            client_id: client_id.into(),
            client_secret: client_secret.into(),
            token: Arc::new(Mutex::new(None)),
            blocked_until_unix: Arc::new(AtomicI64::new(0)),
        }
    }

    /// Ist die App-Auth aktuell gesperrt? Synchron + lock-frei, damit das
    /// `StreamSource::is_auth_blocked`-Trait-Gate (Default `false`,
    /// monitoring.py:1207) es direkt aufrufen kann, ohne den async-Lock zu
    /// nehmen.
    pub fn is_auth_blocked(&self) -> bool {
        let until = self.blocked_until_unix.load(Ordering::Acquire);
        until != 0 && unix_now() < until
    }

    /// Liefert einen gültigen Access-Token, holt bei Bedarf einen neuen.
    ///
    /// Der Lock wird über den gesamten Fetch gehalten (In-Flight-Dedupe). Bei
    /// aktivem Cooldown bricht der Aufruf sofort mit [`TokenError::AuthBlocked`]
    /// ab, ohne Twitch zu kontaktieren.
    pub async fn access_token(&self) -> Result<String, TokenError> {
        let mut guard = self.token.lock().await;

        // Cooldown zuerst prüfen — Python `_raise_if_auth_blocked` vor jedem Fetch.
        if self.is_auth_blocked() {
            return Err(TokenError::AuthBlocked);
        }

        // Cached Token noch gültig? (Re-Check nach Lock-Erwerb deckt Waiter ab,
        // die hinter einem gerade abgeschlossenen Refresh anstanden.)
        let now_unix = unix_now();
        if let Some(token) = guard.as_ref() {
            if !token.needs_refresh(now_unix) {
                return Ok(token.access_token.clone());
            }
        }

        match fetch_app_token(
            &self.http,
            &self.token_url,
            &self.client_id,
            &self.client_secret,
        )
        .await
        {
            Ok(fresh) => {
                let access = fresh.access_token.clone();
                *guard = Some(fresh);
                // Erfolgreicher Abruf hebt den Cooldown auf (Python: `= 0.0`).
                self.blocked_until_unix.store(0, Ordering::Release);
                Ok(access)
            }
            Err(TokenError::HttpStatus { status, message })
                if is_invalid_client(status, &message) =>
            {
                self.blocked_until_unix.store(
                    unix_now() + AUTH_BLOCK_COOLDOWN_SECS as i64,
                    Ordering::Release,
                );
                Err(TokenError::AuthBlocked)
            }
            Err(other) => Err(other),
        }
    }

    /// Verwirft den gecachten App-Token. Der nächste [`Self::access_token`]-
    /// Aufruf holt dadurch gezielt einen frischen Token.
    pub async fn invalidate(&self) {
        *self.token.lock().await = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn needs_refresh_false_wenn_weit_von_ablauf() {
        let token = AppToken::new("tok".to_string(), 3600, 1_000_000);
        // expiry = 1_003_600; jetzt = 1_000_000; Delta = 3600 >> 60 → kein Refresh
        assert!(!token.needs_refresh(1_000_000));
    }

    #[test]
    fn needs_refresh_true_bei_genau_60s_vorlauf() {
        let token = AppToken::new("tok".to_string(), 3600, 1_000_000);
        // expiry = 1_003_600; jetzt = 1_003_540; Delta = 60 → Refresh
        assert!(token.needs_refresh(1_003_540));
    }

    #[test]
    fn needs_refresh_true_wenn_abgelaufen() {
        let token = AppToken::new("tok".to_string(), 3600, 1_000_000);
        // expiry = 1_003_600; jetzt = 1_003_700 > expiry → Refresh
        assert!(token.needs_refresh(1_003_700));
    }

    #[test]
    fn needs_refresh_true_wenn_59s_verbleiben() {
        let token = AppToken::new("tok".to_string(), 3600, 1_000_000);
        // expiry = 1_003_600; jetzt = 1_003_541; Delta = 59 < 60 → Refresh nötig (Marge = 60)
        assert!(token.needs_refresh(1_003_541));
    }

    #[test]
    fn needs_refresh_false_wenn_61s_verbleiben() {
        let token = AppToken::new("tok".to_string(), 3600, 1_000_000);
        // expiry = 1_003_600; jetzt = 1_003_539; Delta = 61 > 60 → noch gültig
        assert!(!token.needs_refresh(1_003_539));
    }

    #[test]
    fn token_response_expires_in_default_3600() {
        // Fehlt `expires_in`, fällt serde auf 3600 zurück (Python-Parität)
        // statt das Parsen mit "missing field" abzubrechen.
        let parsed: TokenResponse = serde_json::from_str(r#"{"access_token":"abc"}"#).unwrap();
        assert_eq!(parsed.access_token, "abc");
        assert_eq!(parsed.expires_in, 3600);
    }

    #[test]
    fn token_response_expires_in_wird_uebernommen() {
        // Ist das Feld vorhanden, gilt der gelieferte Wert.
        let parsed: TokenResponse =
            serde_json::from_str(r#"{"access_token":"abc","expires_in":7200}"#).unwrap();
        assert_eq!(parsed.expires_in, 7200);
    }

    #[test]
    fn is_invalid_client_erkennt_roh_body_und_json_felder() {
        // Nur HTTP 400 + „invalid client" (Roh-Body) zählt.
        assert!(is_invalid_client(400, "invalid client"));
        assert!(is_invalid_client(400, r#"{"message":"Invalid client"}"#));
        assert!(is_invalid_client(
            400,
            r#"{"error":"invalid client secret"}"#
        ));
        // Anderer Status oder andere Meldung → kein invalid_client.
        assert!(!is_invalid_client(401, "invalid client"));
        assert!(!is_invalid_client(400, r#"{"message":"server error"}"#));
        assert!(!is_invalid_client(500, "boom"));
    }

    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn manager_for(server: &MockServer) -> AppTokenManager {
        AppTokenManager::new(
            Arc::new(Client::new()),
            format!("{}/oauth2/token", server.uri()),
            "cid",
            "sec",
        )
    }

    #[tokio::test]
    async fn manager_cached_token_loest_nur_einen_fetch_aus() {
        // In-Flight-Dedupe + Cache: zwei Aufrufe → genau ein Token-Request.
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "tok-cached", "expires_in": 3600
            })))
            .expect(1)
            .mount(&server)
            .await;

        let mgr = manager_for(&server);
        let a = mgr.access_token().await.unwrap();
        let b = mgr.access_token().await.unwrap();
        assert_eq!(a, "tok-cached");
        assert_eq!(b, "tok-cached");
        assert!(!mgr.is_auth_blocked());
        server.verify().await;
    }

    #[tokio::test]
    async fn manager_parallele_aufrufe_teilen_einen_inflight_refresh() {
        // B18-5: 5 gleichzeitige Erst-Aufrufe → der Lock serialisiert den Fetch,
        // nur EIN Token-Request landet bei Twitch (Python asyncio.Lock-Parität).
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(std::time::Duration::from_millis(80))
                    .set_body_json(serde_json::json!({
                        "access_token": "tok-shared", "expires_in": 3600
                    })),
            )
            .expect(1)
            .mount(&server)
            .await;

        let mgr = manager_for(&server);
        let mut handles = Vec::new();
        for _ in 0..5 {
            let m = mgr.clone();
            handles.push(tokio::spawn(async move { m.access_token().await }));
        }
        for h in handles {
            assert_eq!(h.await.unwrap().unwrap(), "tok-shared");
        }
        server.verify().await;
    }

    #[tokio::test]
    async fn manager_invalid_client_setzt_15min_cooldown() {
        // B18-3: invalid_client → AuthBlocked + Circuit-Breaker aktiv; ein
        // Folge-Aufruf scheitert sofort, ohne Twitch erneut zu kontaktieren.
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "status": 400, "message": "invalid client"
            })))
            .expect(1) // nur der erste Versuch erreicht Twitch
            .mount(&server)
            .await;

        let mgr = manager_for(&server);
        assert!(matches!(
            mgr.access_token().await,
            Err(TokenError::AuthBlocked)
        ));
        assert!(mgr.is_auth_blocked());
        // Zweiter Aufruf bricht im Cooldown ab, ohne Request (expect(1) bewacht das).
        assert!(matches!(
            mgr.access_token().await,
            Err(TokenError::AuthBlocked)
        ));
        server.verify().await;
    }

    #[tokio::test]
    async fn manager_transienter_fehler_setzt_keinen_cooldown() {
        // Andere Fehler (z. B. 500) lösen KEINEN Circuit-Breaker aus — der
        // nächste Aufruf darf erneut versuchen.
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
            .mount(&server)
            .await;

        let mgr = manager_for(&server);
        assert!(matches!(
            mgr.access_token().await,
            Err(TokenError::HttpStatus { status: 500, .. })
        ));
        assert!(!mgr.is_auth_blocked());
    }

    #[tokio::test]
    async fn manager_erfolg_nach_block_hebt_cooldown_auf() {
        // Erfolgreicher Token-Abruf setzt blocked_until zurück (Python:
        // `_auth_blocked_until = 0.0`). Ein bereits abgelaufener Cooldown
        // (Deadline in der Vergangenheit) blockiert nicht mehr → der Fetch läuft
        // und nullt die Deadline; sonst bräuchte der Test 15 Min Wartezeit.
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "tok-recovered", "expires_in": 3600
            })))
            .mount(&server)
            .await;

        let mgr = manager_for(&server);
        // Abgelaufene Cooldown-Deadline setzen (Unix-Sekunde in der Vergangenheit).
        mgr.blocked_until_unix
            .store(unix_now() - 1, Ordering::Release);
        assert!(!mgr.is_auth_blocked());

        let token = mgr.access_token().await.unwrap();
        assert_eq!(token, "tok-recovered");
        assert_eq!(mgr.blocked_until_unix.load(Ordering::Acquire), 0);
    }
}
