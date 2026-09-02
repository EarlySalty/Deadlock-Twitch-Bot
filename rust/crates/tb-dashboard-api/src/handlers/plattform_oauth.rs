#![allow(clippy::result_large_err)]

use async_trait::async_trait;
use serde::de::{self, Deserializer};
use serde::Deserialize;

const KICK_ID_BASE: &str = "https://id.kick.com";
const KICK_API_BASE: &str = "https://api.kick.com";
const GOOGLE_TOKEN_BASE: &str = "https://oauth2.googleapis.com";
const GOOGLE_API_BASE: &str = "https://www.googleapis.com";

pub const KICK_SCOPES: &[&str] = &[
    "user:read",
    "channel:read",
    "chat:write",
    "streamkey:read",
    "events:subscribe",
];

pub const YOUTUBE_SCOPE: &str = "https://www.googleapis.com/auth/youtube.force-ssl";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthToken {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: i64,
    pub scopes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OAuthFehler {
    InvalidGrant,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KickKonto {
    pub user_id: String,
    pub slug: String,
    pub rtmp_url: String,
    pub stream_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YouTubeKonto {
    pub channel_id: String,
    pub titel: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YouTubeZiel {
    pub rtmp_url: String,
    pub stream_key: String,
}

#[async_trait]
pub trait KickApi: Send + Sync {
    async fn exchange_code(
        &self,
        code: &str,
        redirect_uri: &str,
        verifier: &str,
    ) -> Result<OAuthToken, OAuthFehler>;
    async fn refresh(&self, refresh_token: &str) -> Result<OAuthToken, OAuthFehler>;
    async fn revoke(&self, access_token: &str) -> Result<(), OAuthFehler>;
    async fn konto(&self, access_token: &str) -> Result<KickKonto, OAuthFehler>;
}

#[async_trait]
pub trait YouTubeApi: Send + Sync {
    async fn exchange_code(&self, code: &str, redirect_uri: &str)
        -> Result<OAuthToken, OAuthFehler>;
    async fn refresh(&self, refresh_token: &str) -> Result<OAuthToken, OAuthFehler>;
    async fn revoke(&self, access_token: &str) -> Result<(), OAuthFehler>;
    async fn konto(&self, access_token: &str) -> Result<YouTubeKonto, OAuthFehler>;
    async fn ziel(&self, access_token: &str) -> Result<Option<YouTubeZiel>, OAuthFehler>;
}

fn scope_feld<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Roh {
        Text(String),
        Liste(Vec<String>),
        Nichts,
    }
    match Roh::deserialize(deserializer).map_err(de::Error::custom)? {
        Roh::Text(s) => Ok(s.split_whitespace().map(str::to_string).collect()),
        Roh::Liste(v) => Ok(v),
        Roh::Nichts => Ok(Vec::new()),
    }
}

fn default_expires_in() -> i64 {
    3600
}

#[derive(Deserialize)]
struct TokenAntwort {
    #[serde(default)]
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default = "default_expires_in")]
    expires_in: i64,
    #[serde(default, deserialize_with = "scope_feld")]
    scope: Vec<String>,
}

impl TokenAntwort {
    fn in_token(self) -> OAuthToken {
        let refresh_token = self
            .refresh_token
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        OAuthToken {
            access_token: self.access_token.trim().to_string(),
            refresh_token,
            expires_in: self.expires_in,
            scopes: self.scope,
        }
    }
}

fn ist_invalid_grant(status: u16, body: &str) -> bool {
    if status != 400 && status != 401 {
        return false;
    }
    body.to_lowercase().contains("invalid_grant")
}

async fn token_request(url: &str, params: &[(&str, &str)]) -> Result<OAuthToken, OAuthFehler> {
    let antwort = reqwest::Client::new()
        .post(url)
        .form(params)
        .send()
        .await
        .map_err(|e| OAuthFehler::Other(format!("request failed: {e}")))?;
    let status = antwort.status().as_u16();
    if status != 200 {
        let body = antwort.text().await.unwrap_or_default();
        if ist_invalid_grant(status, &body) {
            return Err(OAuthFehler::InvalidGrant);
        }
        let snippet: String = body.chars().take(300).collect();
        return Err(OAuthFehler::Other(format!("HTTP {status}: {snippet}")));
    }
    let parsed = antwort
        .json::<TokenAntwort>()
        .await
        .map_err(|e| OAuthFehler::Other(format!("invalid token response: {e}")))?;
    if parsed.access_token.trim().is_empty() {
        return Err(OAuthFehler::Other("token response ohne access_token".into()));
    }
    Ok(parsed.in_token())
}

pub struct KickOAuth {
    client_id: String,
    client_secret: String,
    id_base: String,
    api_base: String,
}

impl KickOAuth {
    pub fn aus_umgebung() -> Option<Self> {
        let client_id = non_empty_env("KICK_CLIENT_ID")?;
        let client_secret = non_empty_env("KICK_CLIENT_SECRET")?;
        Some(Self {
            client_id,
            client_secret,
            id_base: KICK_ID_BASE.to_string(),
            api_base: KICK_API_BASE.to_string(),
        })
    }

    #[cfg(test)]
    fn fuer_test(id_base: &str, api_base: &str) -> Self {
        Self {
            client_id: "kick-cid".into(),
            client_secret: "kick-sec".into(),
            id_base: id_base.trim_end_matches('/').to_string(),
            api_base: api_base.trim_end_matches('/').to_string(),
        }
    }
}

#[async_trait]
impl KickApi for KickOAuth {
    async fn exchange_code(
        &self,
        code: &str,
        redirect_uri: &str,
        verifier: &str,
    ) -> Result<OAuthToken, OAuthFehler> {
        let url = format!("{}/oauth/token", self.id_base);
        token_request(
            &url,
            &[
                ("grant_type", "authorization_code"),
                ("client_id", &self.client_id),
                ("client_secret", &self.client_secret),
                ("redirect_uri", redirect_uri),
                ("code_verifier", verifier),
                ("code", code),
            ],
        )
        .await
    }

    async fn refresh(&self, refresh_token: &str) -> Result<OAuthToken, OAuthFehler> {
        let url = format!("{}/oauth/token", self.id_base);
        token_request(
            &url,
            &[
                ("grant_type", "refresh_token"),
                ("client_id", &self.client_id),
                ("client_secret", &self.client_secret),
                ("refresh_token", refresh_token),
            ],
        )
        .await
    }

    async fn revoke(&self, access_token: &str) -> Result<(), OAuthFehler> {
        let url = format!("{}/oauth/revoke", self.id_base);
        let antwort = reqwest::Client::new()
            .post(&url)
            .query(&[("token", access_token), ("token_hint_type", "access_token")])
            .send()
            .await
            .map_err(|e| OAuthFehler::Other(format!("request failed: {e}")))?;
        let status = antwort.status().as_u16();
        if status != 200 {
            return Err(OAuthFehler::Other(format!("revoke HTTP {status}")));
        }
        Ok(())
    }

    async fn konto(&self, access_token: &str) -> Result<KickKonto, OAuthFehler> {
        let user_url = format!("{}/public/v1/users", self.api_base);
        let user: KickUsersAntwort = api_get(&user_url, access_token).await?;
        let user = user
            .data
            .into_iter()
            .next()
            .ok_or_else(|| OAuthFehler::Other("kick users ohne data".into()))?;

        let channel_url = format!("{}/public/v1/channels", self.api_base);
        let channel: KickChannelsAntwort = api_get(&channel_url, access_token).await?;
        let channel = channel
            .data
            .into_iter()
            .next()
            .ok_or_else(|| OAuthFehler::Other("kick channels ohne data".into()))?;

        let user_id = user.user_id;
        if user_id.trim().is_empty() || user_id == "0" {
            return Err(OAuthFehler::Other("kick users ohne user_id".into()));
        }
        Ok(KickKonto {
            user_id,
            slug: channel.slug,
            rtmp_url: channel.stream.url,
            stream_key: channel.stream.key,
        })
    }
}

async fn api_get<T: for<'de> Deserialize<'de>>(
    url: &str,
    access_token: &str,
) -> Result<T, OAuthFehler> {
    let antwort = reqwest::Client::new()
        .get(url)
        .bearer_auth(access_token)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| OAuthFehler::Other(format!("request failed: {e}")))?;
    let status = antwort.status().as_u16();
    if status != 200 {
        if status == 401 {
            return Err(OAuthFehler::InvalidGrant);
        }
        return Err(OAuthFehler::Other(format!("api HTTP {status}")));
    }
    antwort
        .json::<T>()
        .await
        .map_err(|e| OAuthFehler::Other(format!("api antwort nicht lesbar: {e}")))
}

#[derive(Deserialize)]
struct KickUsersAntwort {
    #[serde(default)]
    data: Vec<KickUser>,
}

#[derive(Deserialize)]
struct KickUser {
    #[serde(default, deserialize_with = "id_feld")]
    user_id: String,
}

fn id_feld<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Roh {
        Zahl(i64),
        Text(String),
        Nichts,
    }
    match Roh::deserialize(deserializer).map_err(de::Error::custom)? {
        Roh::Zahl(n) => Ok(n.to_string()),
        Roh::Text(s) => Ok(s),
        Roh::Nichts => Ok(String::new()),
    }
}

#[derive(Deserialize)]
struct KickChannelsAntwort {
    #[serde(default)]
    data: Vec<KickChannel>,
}

#[derive(Deserialize)]
struct KickChannel {
    #[serde(default)]
    slug: String,
    #[serde(default)]
    stream: KickStream,
}

#[derive(Deserialize, Default)]
struct KickStream {
    #[serde(default)]
    url: String,
    #[serde(default)]
    key: String,
}

pub struct GoogleOAuth {
    client_id: String,
    client_secret: String,
    token_base: String,
    api_base: String,
}

impl GoogleOAuth {
    pub fn aus_umgebung() -> Option<Self> {
        let client_id = google_client_id()?;
        let client_secret = google_client_secret()?;
        Some(Self {
            client_id,
            client_secret,
            token_base: GOOGLE_TOKEN_BASE.to_string(),
            api_base: GOOGLE_API_BASE.to_string(),
        })
    }

    #[cfg(test)]
    fn fuer_test(token_base: &str, api_base: &str) -> Self {
        Self {
            client_id: "g-cid".into(),
            client_secret: "g-sec".into(),
            token_base: token_base.trim_end_matches('/').to_string(),
            api_base: api_base.trim_end_matches('/').to_string(),
        }
    }
}

#[async_trait]
impl YouTubeApi for GoogleOAuth {
    async fn exchange_code(
        &self,
        code: &str,
        redirect_uri: &str,
    ) -> Result<OAuthToken, OAuthFehler> {
        let url = format!("{}/token", self.token_base);
        token_request(
            &url,
            &[
                ("grant_type", "authorization_code"),
                ("client_id", &self.client_id),
                ("client_secret", &self.client_secret),
                ("redirect_uri", redirect_uri),
                ("code", code),
            ],
        )
        .await
    }

    async fn refresh(&self, refresh_token: &str) -> Result<OAuthToken, OAuthFehler> {
        let url = format!("{}/token", self.token_base);
        token_request(
            &url,
            &[
                ("grant_type", "refresh_token"),
                ("client_id", &self.client_id),
                ("client_secret", &self.client_secret),
                ("refresh_token", refresh_token),
            ],
        )
        .await
    }

    async fn revoke(&self, access_token: &str) -> Result<(), OAuthFehler> {
        let url = format!("{}/revoke", self.token_base);
        let antwort = reqwest::Client::new()
            .post(&url)
            .form(&[("token", access_token)])
            .send()
            .await
            .map_err(|e| OAuthFehler::Other(format!("request failed: {e}")))?;
        let status = antwort.status().as_u16();
        if status != 200 {
            return Err(OAuthFehler::Other(format!("revoke HTTP {status}")));
        }
        Ok(())
    }

    async fn konto(&self, access_token: &str) -> Result<YouTubeKonto, OAuthFehler> {
        let url = format!(
            "{}/youtube/v3/channels?mine=true&part=snippet",
            self.api_base
        );
        let antwort: YtChannelsAntwort = api_get(&url, access_token).await?;
        let kanal = antwort
            .items
            .into_iter()
            .next()
            .ok_or_else(|| OAuthFehler::Other("youtube channels ohne items".into()))?;
        if kanal.id.trim().is_empty() {
            return Err(OAuthFehler::Other("youtube channel ohne id".into()));
        }
        Ok(YouTubeKonto {
            channel_id: kanal.id,
            titel: kanal.snippet.title,
        })
    }

    async fn ziel(&self, access_token: &str) -> Result<Option<YouTubeZiel>, OAuthFehler> {
        let url = format!(
            "{}/youtube/v3/liveStreams?mine=true&part=cdn,snippet",
            self.api_base
        );
        let antwort: YtLiveStreamsAntwort = api_get(&url, access_token).await?;
        if antwort.items.is_empty() {
            return Ok(None);
        }
        let gewaehlt = antwort
            .items
            .iter()
            .find(|s| s.snippet.is_default_stream)
            .or_else(|| antwort.items.first())
            .cloned()
            .expect("nicht leer geprueft");
        let rtmp_url = if !gewaehlt.cdn.ingestion_info.rtmps_ingestion_address.is_empty() {
            gewaehlt.cdn.ingestion_info.rtmps_ingestion_address
        } else {
            gewaehlt.cdn.ingestion_info.ingestion_address
        };
        let stream_key = gewaehlt.cdn.ingestion_info.stream_name;
        if rtmp_url.trim().is_empty() || stream_key.trim().is_empty() {
            return Ok(None);
        }
        Ok(Some(YouTubeZiel {
            rtmp_url,
            stream_key,
        }))
    }
}

#[derive(Deserialize)]
struct YtChannelsAntwort {
    #[serde(default)]
    items: Vec<YtChannel>,
}

#[derive(Deserialize)]
struct YtChannel {
    #[serde(default)]
    id: String,
    #[serde(default)]
    snippet: YtChannelSnippet,
}

#[derive(Deserialize, Default)]
struct YtChannelSnippet {
    #[serde(default)]
    title: String,
}

#[derive(Deserialize)]
struct YtLiveStreamsAntwort {
    #[serde(default)]
    items: Vec<YtLiveStream>,
}

#[derive(Deserialize, Clone)]
struct YtLiveStream {
    #[serde(default)]
    snippet: YtLiveStreamSnippet,
    #[serde(default)]
    cdn: YtCdn,
}

#[derive(Deserialize, Default, Clone)]
struct YtLiveStreamSnippet {
    #[serde(default, rename = "isDefaultStream")]
    is_default_stream: bool,
}

#[derive(Deserialize, Default, Clone)]
struct YtCdn {
    #[serde(default, rename = "ingestionInfo")]
    ingestion_info: YtIngestionInfo,
}

#[derive(Deserialize, Default, Clone)]
struct YtIngestionInfo {
    #[serde(default, rename = "streamName")]
    stream_name: String,
    #[serde(default, rename = "ingestionAddress")]
    ingestion_address: String,
    #[serde(default, rename = "rtmpsIngestionAddress")]
    rtmps_ingestion_address: String,
}

pub fn non_empty_env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

pub fn google_client_id() -> Option<String> {
    non_empty_env("GOOGLE_OAUTH_ID")
        .or_else(|| non_empty_env("GOOGLE_CLIENT_ID"))
        .or_else(|| non_empty_env("YOUTUBE_CLIENT_ID"))
}

pub fn google_client_secret() -> Option<String> {
    non_empty_env("GOOGLE_CLIENT_SECRET").or_else(|| non_empty_env("YOUTUBE_CLIENT_SECRET"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_string_contains, header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn kick_exchange_liefert_tokens_und_scopes() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth/token"))
            .and(body_string_contains("grant_type=authorization_code"))
            .and(body_string_contains("code_verifier=verf"))
            .and(body_string_contains("code=abc"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "kacc", "refresh_token": "kref",
                "expires_in": 7200, "scope": "user:read chat:write"
            })))
            .mount(&server)
            .await;
        let client = KickOAuth::fuer_test(&server.uri(), &server.uri());
        let token = client
            .exchange_code("abc", "https://x.test/callback/kick", "verf")
            .await
            .unwrap();
        assert_eq!(token.access_token, "kacc");
        assert_eq!(token.refresh_token.as_deref(), Some("kref"));
        assert_eq!(token.expires_in, 7200);
        assert_eq!(token.scopes, vec!["user:read", "chat:write"]);
    }

    #[tokio::test]
    async fn kick_refresh_invalid_grant_wird_erkannt() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth/token"))
            .and(body_string_contains("grant_type=refresh_token"))
            .respond_with(
                ResponseTemplate::new(400)
                    .set_body_json(serde_json::json!({ "error": "invalid_grant" })),
            )
            .mount(&server)
            .await;
        let client = KickOAuth::fuer_test(&server.uri(), &server.uri());
        assert_eq!(
            client.refresh("tot").await.unwrap_err(),
            OAuthFehler::InvalidGrant
        );
    }

    #[tokio::test]
    async fn kick_konto_holt_user_und_channel() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/public/v1/users"))
            .and(header("Authorization", "Bearer kacc"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{ "user_id": 4242, "name": "streamerin" }]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/public/v1/channels"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{ "slug": "streamerin", "stream": { "url": "rtmps://kick/app", "key": "sk_geheim" } }]
            })))
            .mount(&server)
            .await;
        let client = KickOAuth::fuer_test(&server.uri(), &server.uri());
        let konto = client.konto("kacc").await.unwrap();
        assert_eq!(konto.user_id, "4242");
        assert_eq!(konto.slug, "streamerin");
        assert_eq!(konto.rtmp_url, "rtmps://kick/app");
        assert_eq!(konto.stream_key, "sk_geheim");
    }

    #[tokio::test]
    async fn google_refresh_ohne_neuen_refresh_token() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .and(body_string_contains("grant_type=refresh_token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "gacc2", "expires_in": 3599,
                "scope": "https://www.googleapis.com/auth/youtube.force-ssl"
            })))
            .mount(&server)
            .await;
        let client = GoogleOAuth::fuer_test(&server.uri(), &server.uri());
        let token = client.refresh("gref").await.unwrap();
        assert_eq!(token.access_token, "gacc2");
        assert_eq!(token.refresh_token, None);
        assert_eq!(token.scopes, vec![YOUTUBE_SCOPE.to_string()]);
    }

    #[tokio::test]
    async fn google_konto_liest_kanal() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/youtube/v3/channels"))
            .and(query_param("mine", "true"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "items": [{ "id": "UC123", "snippet": { "title": "Mein Kanal" } }]
            })))
            .mount(&server)
            .await;
        let client = GoogleOAuth::fuer_test(&server.uri(), &server.uri());
        let konto = client.konto("gacc").await.unwrap();
        assert_eq!(konto.channel_id, "UC123");
        assert_eq!(konto.titel, "Mein Kanal");
    }

    #[tokio::test]
    async fn google_ziel_waehlt_default_stream() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/youtube/v3/liveStreams"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "items": [
                    { "snippet": { "isDefaultStream": false },
                      "cdn": { "ingestionInfo": { "streamName": "erst", "rtmpsIngestionAddress": "rtmps://a" } } },
                    { "snippet": { "isDefaultStream": true },
                      "cdn": { "ingestionInfo": { "streamName": "haupt", "rtmpsIngestionAddress": "rtmps://b", "ingestionAddress": "rtmp://b" } } }
                ]
            })))
            .mount(&server)
            .await;
        let client = GoogleOAuth::fuer_test(&server.uri(), &server.uri());
        let ziel = client.ziel("gacc").await.unwrap().unwrap();
        assert_eq!(ziel.rtmp_url, "rtmps://b");
        assert_eq!(ziel.stream_key, "haupt");
    }

    #[tokio::test]
    async fn google_ziel_ohne_stream_ist_none() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/youtube/v3/liveStreams"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({ "items": [] })),
            )
            .mount(&server)
            .await;
        let client = GoogleOAuth::fuer_test(&server.uri(), &server.uri());
        assert_eq!(client.ziel("gacc").await.unwrap(), None);
    }

    #[tokio::test]
    async fn api_401_wird_zu_invalid_grant() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/youtube/v3/channels"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        let client = GoogleOAuth::fuer_test(&server.uri(), &server.uri());
        assert_eq!(client.konto("tot").await.unwrap_err(), OAuthFehler::InvalidGrant);
    }
}
