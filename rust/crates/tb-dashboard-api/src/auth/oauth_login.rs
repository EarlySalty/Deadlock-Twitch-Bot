//! Twitch-OAuth-Login-Bausteine (B3-1/B3-2) — reine Logik ohne axum/DB.
//!
//! Python-Referenz: `bot/dashboard/auth/auth_mixin.py`
//! - `auth_login` (Z. 1202-1268) — Authorize-URL-Bau + State-Erzeugung
//! - `_exchange_code_for_user` (Z. 856-907) — Code→Token→User
//! - `_canonical_post_login_destination`/`_normalize_next_path` (Z. 325-481)
//!
//! **Bewusste Abweichung von Python (modernisiert):** Der Authorize-Request
//! fordert wie das Python-Pendant KEINE Scopes und KEIN `force_verify` an — der
//! Dashboard-Login ist ein reiner Identitäts-Login (wer bist du?), kein
//! Streamer-Token-Grant (das macht der separate Raid-/Re-Auth-Flow). Wir bauen
//! die exakt vier Query-Parameter `client_id`, `redirect_uri`, `response_type`,
//! `state`.

use async_trait::async_trait;
use url::Url;

use tb_transport_twitch::user_token::{TokenOwner, UserTokenError};
use tb_transport_twitch::{HelixClient, HelixConfig};

/// Basis-URL des Twitch-Authorization-Code-Flows (Python: auth_mixin.py:29).
pub const TWITCH_AUTHORIZE_URL: &str = "https://id.twitch.tv/oauth2/authorize";

/// Default-Post-Login-Ziel (Python `_canonical_post_login_destination`-Fallback,
/// auth_mixin.py:1406 — `/twitch/dashboard`).
pub const DEFAULT_POST_LOGIN_PATH: &str = "/twitch/dashboard";

/// Erlaubte Post-Login-Redirect-Pfad-Präfixe (Python `_canonical_post_login_destination`,
/// auth_mixin.py:447-481). Reine Pfad-Whitelist — verhindert Open-Redirect: ein
/// `next`-Wert, der nicht mit einem dieser internen Präfixe beginnt, fällt auf
/// [`DEFAULT_POST_LOGIN_PATH`] zurück.
const ALLOWED_NEXT_PREFIXES: &[&str] = &[
    "/twitch/dashboard",
    "/twitch/abbo",
    "/twitch/abo",
    "/twitch/stats",
    "/twitch/verwaltung",
    "/twitch/pricing",
    "/twitch/raid/auth",
    "/analyse",
];

/// Normalisiert einen `next`-Query-Parameter auf ein sicheres internes Ziel.
///
/// Open-Redirect-Schutz: Nur Pfade, die mit einem [`ALLOWED_NEXT_PREFIXES`]-Eintrag
/// beginnen, werden übernommen (inkl. evtl. Query-String). Alles andere — fehlend,
/// extern (`http://…`, `//host`), unbekannt — fällt auf [`DEFAULT_POST_LOGIN_PATH`].
pub fn sanitize_next_path(raw: Option<&str>) -> String {
    let candidate = raw.map(str::trim).unwrap_or("");
    // Protocol-relative (`//evil`) und absolute (`http://…`) URLs sind nie intern.
    if candidate.is_empty() || candidate.starts_with("//") || !candidate.starts_with('/') {
        return DEFAULT_POST_LOGIN_PATH.to_string();
    }
    // Pfadteil (vor `?`) gegen die Whitelist prüfen.
    let path_only = candidate.split(['?', '#']).next().unwrap_or("");
    let allowed = ALLOWED_NEXT_PREFIXES.iter().any(|prefix| {
        path_only == *prefix || path_only.starts_with(&format!("{prefix}/"))
    });
    if allowed {
        candidate.to_string()
    } else {
        DEFAULT_POST_LOGIN_PATH.to_string()
    }
}

/// Baut die Twitch-Authorize-URL des Dashboard-Logins (Python: auth_mixin.py:1263).
///
/// Genau vier Parameter, keine Scopes, kein `force_verify` (siehe Modul-Doku).
pub fn build_login_authorize_url(client_id: &str, redirect_uri: &str, state: &str) -> String {
    let mut url =
        Url::parse(TWITCH_AUTHORIZE_URL).expect("TWITCH_AUTHORIZE_URL ist eine valide statische URL");
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("client_id", client_id);
        q.append_pair("redirect_uri", redirect_uri);
        q.append_pair("response_type", "code");
        q.append_pair("state", state);
    }
    url.to_string()
}

/// Identität des eingeloggten Twitch-Users nach dem Code-Tausch
/// (Python `_exchange_code_for_user`-Rückgabe, auth_mixin.py:903-907).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TwitchIdentity {
    pub twitch_login: String,
    pub twitch_user_id: String,
    pub display_name: String,
}

/// Abstrahiert den Twitch-OAuth-HTTP-Pfad (Code→Token→User), damit der Handler
/// im Test mit einem Fake bedient werden kann (kein echter Twitch-Call, keine
/// Secrets). Die echte Implementierung ist [`HelixOAuthClient`].
#[async_trait]
pub trait TwitchOAuthClient: Send + Sync {
    /// Tauscht `code` (mit exakt der Authorize-`redirect_uri`) gegen die
    /// Identität des einloggenden Twitch-Users. `Err` bei Exchange-/Lookup-Fehler.
    async fn exchange_code_for_identity(
        &self,
        code: &str,
        redirect_uri: &str,
    ) -> Result<TwitchIdentity, UserTokenError>;
}

/// Echte Implementierung über den [`HelixClient`]: `exchange_user_code`
/// (grant_type=authorization_code) + `fetch_token_owner` (`GET /helix/users`
/// mit dem frischen User-Bearer). Beide Schritte spiegeln Python 1:1.
pub struct HelixOAuthClient {
    helix: HelixClient,
}

impl HelixOAuthClient {
    /// Baut den Client aus Twitch-App-Credentials. Standard-Twitch-URLs.
    pub fn new(client_id: &str, client_secret: &str) -> Result<Self, reqwest::Error> {
        let helix = HelixClient::new(HelixConfig::new(client_id, client_secret))?;
        Ok(Self { helix })
    }

    /// Baut den Client mit überschriebenen Twitch-URLs (für Tests via wiremock).
    pub fn from_config(config: HelixConfig) -> Result<Self, reqwest::Error> {
        Ok(Self { helix: HelixClient::new(config)? })
    }
}

#[async_trait]
impl TwitchOAuthClient for HelixOAuthClient {
    async fn exchange_code_for_identity(
        &self,
        code: &str,
        redirect_uri: &str,
    ) -> Result<TwitchIdentity, UserTokenError> {
        let token = self.helix.exchange_user_code(code, redirect_uri).await?;
        if token.access_token.trim().is_empty() {
            return Err(UserTokenError::Other("missing access_token".to_string()));
        }
        let owner: TokenOwner = self.helix.fetch_token_owner(&token.access_token).await?;
        Ok(TwitchIdentity {
            twitch_login: owner.login,
            twitch_user_id: owner.id,
            display_name: owner.display_name,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authorize_url_hat_genau_vier_parameter_ohne_scope() {
        let url = build_login_authorize_url("cid", "https://x.test/twitch/auth/callback", "stTok");
        assert!(url.starts_with(TWITCH_AUTHORIZE_URL));
        assert!(url.contains("client_id=cid"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("state=stTok"));
        // KEINE Scopes, KEIN force_verify (reiner Identitäts-Login).
        assert!(!url.contains("scope="));
        assert!(!url.contains("force_verify"));
    }

    #[test]
    fn authorize_url_encodiert_redirect_und_state() {
        let url = build_login_authorize_url("c", "https://x.test/cb?a=b", "a b&c");
        // redirect_uri und state müssen percent-encodiert sein.
        assert!(url.contains("redirect_uri=https%3A%2F%2Fx.test%2Fcb%3Fa%3Db"));
        assert!(!url.contains("state=a b&c"));
    }

    #[test]
    fn next_path_default_bei_fehlend_oder_extern() {
        assert_eq!(sanitize_next_path(None), DEFAULT_POST_LOGIN_PATH);
        assert_eq!(sanitize_next_path(Some("")), DEFAULT_POST_LOGIN_PATH);
        assert_eq!(sanitize_next_path(Some("   ")), DEFAULT_POST_LOGIN_PATH);
        // Open-Redirect-Versuche:
        assert_eq!(sanitize_next_path(Some("https://evil.test")), DEFAULT_POST_LOGIN_PATH);
        assert_eq!(sanitize_next_path(Some("//evil.test")), DEFAULT_POST_LOGIN_PATH);
        assert_eq!(sanitize_next_path(Some("/etc/passwd")), DEFAULT_POST_LOGIN_PATH);
    }

    #[test]
    fn next_path_erlaubte_ziele_bleiben() {
        assert_eq!(sanitize_next_path(Some("/analyse")), "/analyse");
        assert_eq!(sanitize_next_path(Some("/twitch/stats")), "/twitch/stats");
        assert_eq!(
            sanitize_next_path(Some("/twitch/abbo/rechnungen?x=1")),
            "/twitch/abbo/rechnungen?x=1"
        );
        // Präfix-Grenze: /twitch/statszzz ist NICHT /twitch/stats.
        assert_eq!(sanitize_next_path(Some("/twitch/statszzz")), DEFAULT_POST_LOGIN_PATH);
    }
}
