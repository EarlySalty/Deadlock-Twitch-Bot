use std::collections::HashSet;
use std::error::Error;
use std::future::Future;
use std::time::Duration;

use chrono::{DateTime, Utc};
use reqwest::{redirect::Policy, Client, StatusCode};
use serde::Deserialize;
use sqlx::{PgPool, Row};
use tb_internal_api::{PatchEvent, PatchProcessError, PatchReceiver};

const BASE_URL: &str = "https://deutsche-deadlock-community.de/patchnotes";
const INDEX_URL: &str = "https://deutsche-deadlock-community.de/patchnotes/index.json";
const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
const MAX_PENDING_AGE_SECONDS: i64 = 120;

#[derive(Debug, thiserror::Error)]
pub enum PatchFeedError {
    #[error("Patchfeed-HTTP-Fehler: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Patchfeed-Datenbankfehler: {0}")]
    Database(#[from] sqlx::Error),
    #[error("Ungültiger Patchfeed: {0}")]
    InvalidFeed(String),
    #[error("Patch-Ankündigung fehlgeschlagen: {0}")]
    Callback(#[source] Box<dyn Error + Send + Sync>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatchArticle {
    pub id: i64,
    pub url: String,
    pub source_url: String,
    pub observed_at: DateTime<Utc>,
}

pub struct PatchFeedClient {
    inner: Client,
}

impl PatchFeedClient {
    pub fn new() -> Result<Self, reqwest::Error> {
        Ok(Self {
            inner: Client::builder()
                .redirect(Policy::none())
                .timeout(Duration::from_secs(15))
                .build()?,
        })
    }
}

struct FeedResponse {
    status: StatusCode,
    url: String,
    body: Vec<u8>,
}

trait FeedHttp {
    fn get(&self, url: &str) -> impl Future<Output = Result<FeedResponse, PatchFeedError>> + Send;
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PendingObservation {
    id: i64,
    observed_at: DateTime<Utc>,
}

trait FeedCursor {
    fn observe_index(
        &self,
        ids: Vec<i64>,
    ) -> impl Future<Output = Result<Vec<PendingObservation>, PatchFeedError>> + Send;

    fn expire_pending(
        &self,
        id: i64,
        status: &'static str,
    ) -> impl Future<Output = Result<bool, PatchFeedError>> + Send;
}

impl FeedHttp for PatchFeedClient {
    async fn get(&self, url: &str) -> Result<FeedResponse, PatchFeedError> {
        let mut response = self.inner.get(url).send().await?;
        let status = response.status();
        let final_url = response.url().to_string();
        let mut body = Vec::new();
        if status == StatusCode::OK && final_url == url {
            while let Some(chunk) = response.chunk().await? {
                if chunk.len() > MAX_RESPONSE_BYTES.saturating_sub(body.len()) {
                    return Err(PatchFeedError::InvalidFeed("HTTP-Antwort zu groß".into()));
                }
                body.extend_from_slice(&chunk);
            }
        }
        Ok(FeedResponse {
            status,
            url: final_url,
            body,
        })
    }
}

#[derive(Deserialize)]
struct IndexEntry {
    id: i64,
    url: String,
}

#[derive(Deserialize)]
struct ArticleMetadata {
    id: i64,
    source_url: String,
    urls: ArticleUrls,
}

#[derive(Deserialize)]
struct ArticleUrls {
    de: String,
}

#[derive(Clone)]
struct ValidatedArticle {
    id: i64,
    url: String,
    source_url: String,
}

impl ValidatedArticle {
    fn into_patch_article(self, observed_at: DateTime<Utc>) -> PatchArticle {
        PatchArticle {
            id: self.id,
            url: self.url,
            source_url: self.source_url,
            observed_at,
        }
    }
}

fn article_url(id: i64) -> Result<String, PatchFeedError> {
    if id <= 0 {
        return Err(PatchFeedError::InvalidFeed("Ungültige Patch-ID".into()));
    }
    Ok(format!("{BASE_URL}/patch-{id}/"))
}

fn valid_source_url(value: &str) -> bool {
    let Ok(url) = url::Url::parse(value) else {
        return false;
    };
    url.scheme() == "https"
        && matches!(
            url.host_str(),
            Some("forums.playdeadlock.com" | "steamcommunity.com" | "store.steampowered.com")
        )
        && matches!(url.port(), None | Some(443))
        && url.username().is_empty()
        && url.password().is_none()
        && !value.chars().any(char::is_control)
}

fn checked_body(response: FeedResponse, url: &str) -> Result<Vec<u8>, PatchFeedError> {
    if response.status != StatusCode::OK || response.url != url {
        return Err(PatchFeedError::InvalidFeed(format!(
            "Unerwartete HTTP-Antwort für {url}: {} {}",
            response.status, response.url
        )));
    }
    if response.body.is_empty() || response.body.len() > MAX_RESPONSE_BYTES {
        return Err(PatchFeedError::InvalidFeed(format!(
            "Leere oder zu große HTTP-Antwort für {url}"
        )));
    }
    Ok(response.body)
}

async fn fetch_index<H: FeedHttp>(http: &H) -> Result<Vec<IndexEntry>, PatchFeedError> {
    let body = checked_body(http.get(INDEX_URL).await?, INDEX_URL)?;
    let mut entries: Vec<IndexEntry> = serde_json::from_slice(&body)
        .map_err(|error| PatchFeedError::InvalidFeed(error.to_string()))?;
    let mut seen = HashSet::new();
    for entry in &entries {
        if entry.url != article_url(entry.id)? || !seen.insert(entry.id) {
            return Err(PatchFeedError::InvalidFeed(format!(
                "Ungültiger oder doppelter Indexeintrag: {}",
                entry.id
            )));
        }
    }
    entries.sort_unstable_by_key(|entry| entry.id);
    Ok(entries)
}

async fn fetch_article<H: FeedHttp>(
    http: &H,
    entry: &IndexEntry,
) -> Result<ValidatedArticle, PatchFeedError> {
    let meta_url = format!("{BASE_URL}/patch-{}/meta.json", entry.id);
    let body = checked_body(http.get(&meta_url).await?, &meta_url)?;
    let meta: ArticleMetadata = serde_json::from_slice(&body)
        .map_err(|error| PatchFeedError::InvalidFeed(error.to_string()))?;
    if meta.id != entry.id || meta.urls.de != entry.url || !valid_source_url(&meta.source_url) {
        return Err(PatchFeedError::InvalidFeed(format!(
            "Metadaten für Patch {} passen nicht zum Index",
            entry.id
        )));
    }
    checked_body(http.get(&entry.url).await?, &entry.url)?;
    Ok(ValidatedArticle {
        id: entry.id,
        url: entry.url.clone(),
        source_url: meta.source_url,
    })
}

fn pending_is_expired(now: DateTime<Utc>, observed_at: DateTime<Utc>) -> bool {
    now.signed_duration_since(observed_at) >= chrono::Duration::seconds(MAX_PENDING_AGE_SECONDS)
}

async fn run_feed<H, S, D, Fut>(
    http: &H,
    store: &S,
    mut deliver: D,
) -> Result<usize, PatchFeedError>
where
    H: FeedHttp,
    S: FeedCursor,
    D: FnMut(ValidatedArticle) -> Fut,
    Fut: Future<Output = Result<bool, PatchFeedError>>,
{
    let entries = fetch_index(http).await?;
    let ids = entries.iter().map(|entry| entry.id).collect();
    let entries: std::collections::HashMap<_, _> =
        entries.into_iter().map(|entry| (entry.id, entry)).collect();
    let pending = store.observe_index(ids).await?;
    let mut count = 0;
    for observation in pending {
        let entry = entries.get(&observation.id);
        if pending_is_expired(Utc::now(), observation.observed_at) {
            let status = if entry.is_some() {
                "expired_timeout"
            } else {
                "expired_missing_from_index"
            };
            if store.expire_pending(observation.id, status).await? {
                tracing::warn!(
                    patch_id = observation.id,
                    status,
                    "Patchfeed-Eintrag terminal übersprungen"
                );
            }
            continue;
        }
        let Some(entry) = entry else {
            return Err(PatchFeedError::InvalidFeed(format!(
                "Offener Patch {} fehlt im Index",
                observation.id
            )));
        };
        let article = match fetch_article(http, entry).await {
            Ok(article) => article,
            Err(error) => {
                if pending_is_expired(Utc::now(), observation.observed_at) {
                    if store
                        .expire_pending(observation.id, "expired_unavailable")
                        .await?
                    {
                        tracing::warn!(
                            patch_id = observation.id,
                            status = "expired_unavailable",
                            "Patchfeed-Eintrag terminal übersprungen"
                        );
                    }
                    continue;
                }
                return Err(error);
            }
        };
        if deliver(article).await? {
            count += 1;
        }
    }
    Ok(count)
}

async fn observe_index(
    pool: &PgPool,
    ids: &[i64],
) -> Result<Vec<PendingObservation>, PatchFeedError> {
    let mut tx = pool.begin().await?;
    let inserted = if ids.is_empty() {
        false
    } else {
        sqlx::query_scalar::<_, bool>(
            "INSERT INTO twitch_patch_feed_state (singleton, bootstrapped_at) \
             VALUES (TRUE, NOW()) ON CONFLICT (singleton) DO NOTHING RETURNING singleton",
        )
        .fetch_optional(&mut *tx)
        .await?
        .unwrap_or(false)
    };
    let state = sqlx::query(
        "SELECT bootstrapped_at FROM twitch_patch_feed_state \
         WHERE singleton = TRUE FOR UPDATE",
    )
    .fetch_optional(&mut *tx)
    .await?;
    if let Some(state) = state {
        let bootstrapped_at: DateTime<Utc> = state.try_get("bootstrapped_at")?;
        if inserted {
            for id in ids {
                sqlx::query(
                    "INSERT INTO twitch_patch_feed_observations \
                     (patch_id, observed_at, status, finalized_at) \
                     VALUES ($1, $2, 'historical', $2) ON CONFLICT (patch_id) DO NOTHING",
                )
                .bind(id)
                .bind(bootstrapped_at)
                .execute(&mut *tx)
                .await?;
            }
        } else {
            for id in ids {
                sqlx::query(
                    "INSERT INTO twitch_patch_feed_observations \
                     (patch_id, observed_at, status, finalized_at) \
                     VALUES ($1, NOW(), 'pending', NULL) ON CONFLICT (patch_id) DO NOTHING",
                )
                .bind(id)
                .execute(&mut *tx)
                .await?;
            }
        }
    }
    let pending = sqlx::query(
        "SELECT patch_id, observed_at FROM twitch_patch_feed_observations \
         WHERE status = 'pending' ORDER BY observed_at, patch_id",
    )
    .fetch_all(&mut *tx)
    .await?
    .into_iter()
    .map(|row| {
        Ok(PendingObservation {
            id: row.try_get("patch_id")?,
            observed_at: row.try_get("observed_at")?,
        })
    })
    .collect::<Result<Vec<_>, sqlx::Error>>()?;
    tx.commit().await?;
    Ok(pending)
}

async fn expire_pending(
    pool: &PgPool,
    id: i64,
    status: &'static str,
) -> Result<bool, PatchFeedError> {
    let mut tx = pool.begin().await?;
    let row = sqlx::query(
        "SELECT status, observed_at FROM twitch_patch_feed_observations \
         WHERE patch_id = $1 FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(row) = row else {
        tx.commit().await?;
        return Ok(false);
    };
    let current_status: String = row.try_get("status")?;
    let observed_at: DateTime<Utc> = row.try_get("observed_at")?;
    if current_status != "pending" || !pending_is_expired(Utc::now(), observed_at) {
        tx.commit().await?;
        return Ok(false);
    }
    sqlx::query(
        "UPDATE twitch_patch_feed_observations \
         SET status = $2, finalized_at = NOW() WHERE patch_id = $1 AND status = 'pending'",
    )
    .bind(id)
    .bind(status)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(true)
}

impl FeedCursor for PgPool {
    async fn observe_index(
        &self,
        ids: Vec<i64>,
    ) -> Result<Vec<PendingObservation>, PatchFeedError> {
        observe_index(self, &ids).await
    }

    async fn expire_pending(&self, id: i64, status: &'static str) -> Result<bool, PatchFeedError> {
        expire_pending(self, id, status).await
    }
}

async fn deliver_article<F, Fut, E>(
    pool: &PgPool,
    article: ValidatedArticle,
    callback: &mut F,
) -> Result<bool, PatchFeedError>
where
    F: FnMut(PatchArticle) -> Fut,
    Fut: Future<Output = Result<(), E>>,
    E: Error + Send + Sync + 'static,
{
    let mut delivery = pool.begin().await?;
    let row = sqlx::query(
        "SELECT status, observed_at FROM twitch_patch_feed_observations \
         WHERE patch_id = $1 FOR UPDATE",
    )
    .bind(article.id)
    .fetch_optional(&mut *delivery)
    .await?;
    let Some(row) = row else {
        return Err(PatchFeedError::InvalidFeed(format!(
            "Patch {} hat keine Beobachtungszeile",
            article.id
        )));
    };
    let status: String = row.try_get("status")?;
    let observed_at: DateTime<Utc> = row.try_get("observed_at")?;
    if status != "pending" {
        delivery.commit().await?;
        return Ok(false);
    }
    if pending_is_expired(Utc::now(), observed_at) {
        sqlx::query(
            "UPDATE twitch_patch_feed_observations \
             SET status = 'expired_timeout', finalized_at = NOW() \
             WHERE patch_id = $1 AND status = 'pending'",
        )
        .bind(article.id)
        .execute(&mut *delivery)
        .await?;
        delivery.commit().await?;
        tracing::warn!(
            patch_id = article.id,
            status = "expired_timeout",
            "Patchfeed-Eintrag terminal übersprungen"
        );
        return Ok(false);
    }
    let article_id = article.id;
    callback(article.into_patch_article(observed_at))
        .await
        .map_err(|error| PatchFeedError::Callback(Box::new(error)))?;
    sqlx::query(
        "UPDATE twitch_patch_feed_observations \
         SET status = 'processed', finalized_at = NOW() WHERE patch_id = $1 AND status = 'pending'",
    )
    .bind(article_id)
    .execute(&mut *delivery)
    .await?;
    delivery.commit().await?;
    Ok(true)
}

async fn poll_patch_feed_with_http<H, F, Fut, E>(
    http: &H,
    pool: &PgPool,
    callback: F,
) -> Result<usize, PatchFeedError>
where
    H: FeedHttp,
    F: FnMut(PatchArticle) -> Fut,
    Fut: Future<Output = Result<(), E>>,
    E: Error + Send + Sync + 'static,
{
    let callback = tokio::sync::Mutex::new(callback);
    run_feed(http, pool, |article| async {
        let mut callback = callback.lock().await;
        deliver_article(pool, article, &mut *callback).await
    })
    .await
}

pub async fn forward_patch_article(
    receiver: &PatchReceiver,
    article: PatchArticle,
) -> Result<(), PatchProcessError> {
    let event = PatchEvent::from_article(
        article.id,
        article.url,
        article.source_url,
        article.observed_at,
    )?;
    receiver.process(&event).await.map(|_| ())
}

pub async fn poll_patch_feed<F, Fut, E>(
    client: &PatchFeedClient,
    pool: &PgPool,
    callback: F,
) -> Result<usize, PatchFeedError>
where
    F: FnMut(PatchArticle) -> Fut,
    Fut: Future<Output = Result<(), E>>,
    E: Error + Send + Sync + 'static,
{
    poll_patch_feed_with_http(client, pool, callback).await
}

#[cfg(test)]
#[allow(clippy::duplicate_mod)]
#[path = "../../../test-support/postgres.rs"]
mod postgres;

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::process::Command;
    use std::sync::{mpsc, Arc, Mutex};

    use super::*;
    use tb_chat::api::{BanOutcome, SourceOnlyPreSendCheck};
    use tb_chat::{ChatApi, SendOutcome};
    use tb_internal_api::PatchReceiver;
    use tb_transport_twitch::{HelixClient, HelixConfig};
    use wiremock::matchers::{body_partial_json, header, method, path, query_param};
    use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

    use super::postgres::TestPostgres;

    struct MockEndpointChat {
        helix: HelixClient,
    }

    #[async_trait::async_trait]
    impl ChatApi for MockEndpointChat {
        async fn send_message(&self, _: &str, _: &str) -> Result<SendOutcome, String> {
            Err("only guarded source-only sends are supported".into())
        }

        async fn send_source_only_message_guarded(
            &self,
            broadcaster_id: &str,
            message: &str,
            pre_send_check: SourceOnlyPreSendCheck,
        ) -> Result<SendOutcome, String> {
            self.helix
                .send_source_only_chat_message_guarded(
                    broadcaster_id,
                    "777",
                    message,
                    pre_send_check,
                )
                .await
                .map_err(|error| error.to_string())
        }

        async fn send_announcement(&self, _: &str, _: &str, _: &str) -> Result<bool, String> {
            Err("unsupported".into())
        }

        async fn ban_user(&self, _: &str, _: &str, _: &str) -> Result<BanOutcome, String> {
            Err("unsupported".into())
        }

        async fn timeout_user(
            &self,
            _: &str,
            _: &str,
            _: u32,
            _: &str,
        ) -> Result<BanOutcome, String> {
            Err("unsupported".into())
        }

        async fn unban_user(&self, _: &str, _: &str) -> Result<bool, String> {
            Err("unsupported".into())
        }

        async fn delete_message(&self, _: &str, _: &str) -> Result<bool, String> {
            Err("unsupported".into())
        }

        async fn user_created_at(&self, _: &str) -> Result<Option<DateTime<Utc>>, String> {
            Err("unsupported".into())
        }

        async fn resolve_user_id(&self, _: &str) -> Result<Option<String>, String> {
            Err("unsupported".into())
        }

        async fn bot_user_id(&self) -> String {
            "777".into()
        }
    }

    async fn isolated_bot_database() -> (TestPostgres, PgPool, PgPool) {
        let postgres = TestPostgres::start().await;
        sqlx::query("CREATE ROLE postgres SUPERUSER NOLOGIN")
            .execute(&postgres.pool)
            .await
            .unwrap();
        sqlx::query("CREATE DATABASE twitch_analytics")
            .execute(&postgres.pool)
            .await
            .unwrap();
        let socket_dir: String = sqlx::query_scalar("SHOW unix_socket_directories")
            .fetch_one(&postgres.pool)
            .await
            .unwrap();
        let admin = sqlx::postgres::PgPoolOptions::new()
            .max_connections(3)
            .connect_with(
                sqlx::postgres::PgConnectOptions::new()
                    .host(&socket_dir)
                    .username("uplink_test")
                    .database("twitch_analytics"),
            )
            .await
            .unwrap();
        sqlx::raw_sql(
            "CREATE TABLE twitch_streamers_partner_state (
                 twitch_user_id TEXT, twitch_login TEXT, is_partner_active INT,
                 manual_partner_opt_out INT DEFAULT 0);
             CREATE TABLE twitch_raid_auth (twitch_user_id TEXT, scopes TEXT, needs_reauth BOOLEAN);
             CREATE TABLE twitch_live_state (
                 twitch_user_id TEXT, last_stream_id TEXT, last_seen_at TEXT,
                 is_live INT, last_game TEXT);
             CREATE TABLE twitch_streamer_identities (
                 twitch_user_id TEXT PRIMARY KEY, twitch_login TEXT);
             INSERT INTO twitch_streamer_identities VALUES ('42','renamed'), ('43','later');
             INSERT INTO twitch_streamers_partner_state VALUES
                 ('42','renamed',1,0), ('43','later',0,0);
             INSERT INTO twitch_raid_auth VALUES
                 ('42','user:read:chat channel:bot',FALSE), ('43','channel:bot',FALSE);
             INSERT INTO twitch_live_state VALUES
                 ('42','s42',to_char(now(), 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'),1,'Deadlock'),
                 ('43','s43',to_char(now(), 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'),1,'Deadlock');",
        )
        .execute(&admin)
        .await
        .unwrap();
        sqlx::raw_sql(tb_chat::moderation::SUPPRESSION_DDL)
            .execute(&admin)
            .await
            .unwrap();
        sqlx::raw_sql(include_str!(
            "../../../migrations/20260928120000_patch_announcements.sql"
        ))
        .execute(&admin)
        .await
        .unwrap();
        let output = Command::new("/usr/lib/postgresql/16/bin/psql")
            .args([
                "-h",
                &socket_dir,
                "-U",
                "uplink_test",
                "-d",
                "twitch_analytics",
                "-v",
                "ON_ERROR_STOP=1",
                "-f",
                concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../../ops/systemd/twitch-runtime-roles.sql"
                ),
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "runtime role matrix failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let bot = sqlx::postgres::PgPoolOptions::new()
            .max_connections(5)
            .connect_with(
                sqlx::postgres::PgConnectOptions::new()
                    .host(&socket_dir)
                    .username("twitchbot")
                    .database("twitch_analytics"),
            )
            .await
            .unwrap();
        let identity: String = sqlx::query_scalar("SELECT current_user")
            .fetch_one(&bot)
            .await
            .unwrap();
        assert_eq!(identity, "twitchbot");
        (postgres, admin, bot)
    }

    async fn poll_with_receiver(
        http: &FakeHttp,
        pool: &PgPool,
        receiver: &Arc<PatchReceiver>,
    ) -> Result<usize, PatchFeedError> {
        poll_patch_feed_with_http(http, pool, |article| {
            let receiver = Arc::clone(receiver);
            async move { forward_patch_article(&receiver, article).await }
        })
        .await
    }

    fn distinct_source(http: &FakeHttp, id: i64) {
        let url = article_url(id).unwrap();
        let meta_url = format!("{BASE_URL}/patch-{id}/meta.json");
        http.add(
            &meta_url,
            serde_json::to_vec(&serde_json::json!({
                "id": id,
                "source_url": format!("https://forums.playdeadlock.com/posts/{id}/"),
                "urls": { "de": url },
            }))
            .unwrap(),
        );
    }

    struct PausedChatResponse {
        entered: Arc<tokio::sync::Notify>,
        release: Mutex<mpsc::Receiver<()>>,
    }

    impl Respond for PausedChatResponse {
        fn respond(&self, _: &Request) -> ResponseTemplate {
            self.entered.notify_one();
            self.release.lock().unwrap().recv().unwrap();
            ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"is_sent": true, "message_id": "sent-after-abort"}]
            }))
        }
    }

    #[derive(Default)]
    struct FakeHttp {
        responses: Mutex<HashMap<String, FeedResponse>>,
    }

    impl FakeHttp {
        fn add(&self, url: &str, body: impl Into<Vec<u8>>) {
            self.add_response(url, StatusCode::OK, url, body);
        }

        fn add_response(
            &self,
            url: &str,
            status: StatusCode,
            final_url: &str,
            body: impl Into<Vec<u8>>,
        ) {
            self.responses.lock().unwrap().insert(
                url.into(),
                FeedResponse {
                    status,
                    url: final_url.into(),
                    body: body.into(),
                },
            );
        }
    }

    impl FeedHttp for FakeHttp {
        async fn get(&self, url: &str) -> Result<FeedResponse, PatchFeedError> {
            let response = self
                .responses
                .lock()
                .unwrap()
                .get(url)
                .map(|response| FeedResponse {
                    status: response.status,
                    url: response.url.clone(),
                    body: response.body.clone(),
                });
            response.ok_or_else(|| PatchFeedError::InvalidFeed("HTTP vorübergehend weg".into()))
        }
    }

    #[derive(Clone)]
    struct FakeObservation {
        observed_at: DateTime<Utc>,
        status: &'static str,
    }

    #[derive(Default)]
    struct FakeState {
        bootstrapped: bool,
        observations: HashMap<i64, FakeObservation>,
    }

    #[derive(Default)]
    struct FakeCursor {
        state: Mutex<FakeState>,
    }

    impl FakeCursor {
        fn existing(cursor: i64, pending: Option<i64>) -> Self {
            let mut state = FakeState {
                bootstrapped: true,
                observations: HashMap::new(),
            };
            for id in 1..=cursor {
                state.observations.insert(
                    id,
                    FakeObservation {
                        observed_at: Utc::now(),
                        status: "historical",
                    },
                );
            }
            if let Some(id) = pending {
                state.observations.insert(
                    id,
                    FakeObservation {
                        observed_at: Utc::now(),
                        status: "pending",
                    },
                );
            }
            Self {
                state: Mutex::new(state),
            }
        }

        fn advance(&self, id: i64) -> bool {
            let mut state = self.state.lock().unwrap();
            let Some(observation) = state.observations.get_mut(&id) else {
                return false;
            };
            if observation.status != "pending" {
                return false;
            }
            observation.status = "processed";
            true
        }

        fn observed_at(&self, id: i64) -> Option<DateTime<Utc>> {
            self.state
                .lock()
                .unwrap()
                .observations
                .get(&id)
                .map(|observation| observation.observed_at)
        }

        fn set_observed_at(&self, id: i64, observed_at: DateTime<Utc>) {
            if let Some(observation) = self.state.lock().unwrap().observations.get_mut(&id) {
                observation.observed_at = observed_at;
            }
        }

        fn current_status(&self, id: i64) -> Option<&'static str> {
            self.state
                .lock()
                .unwrap()
                .observations
                .get(&id)
                .map(|observation| observation.status)
        }
    }

    impl FeedCursor for FakeCursor {
        async fn observe_index(
            &self,
            ids: Vec<i64>,
        ) -> Result<Vec<PendingObservation>, PatchFeedError> {
            let mut state = self.state.lock().unwrap();
            if !state.bootstrapped && !ids.is_empty() {
                state.bootstrapped = true;
                let now = Utc::now();
                for id in ids {
                    state.observations.insert(
                        id,
                        FakeObservation {
                            observed_at: now,
                            status: "historical",
                        },
                    );
                }
            } else {
                let now = Utc::now();
                for id in ids {
                    state.observations.entry(id).or_insert(FakeObservation {
                        observed_at: now,
                        status: "pending",
                    });
                }
            }
            let mut pending: Vec<_> = state
                .observations
                .iter()
                .filter(|(_, observation)| observation.status == "pending")
                .map(|(id, observation)| PendingObservation {
                    id: *id,
                    observed_at: observation.observed_at,
                })
                .collect();
            pending.sort_by_key(|observation| (observation.observed_at, observation.id));
            Ok(pending)
        }

        async fn expire_pending(
            &self,
            id: i64,
            status: &'static str,
        ) -> Result<bool, PatchFeedError> {
            let mut state = self.state.lock().unwrap();
            let Some(observation) = state.observations.get_mut(&id) else {
                return Ok(false);
            };
            if observation.status != "pending"
                || !pending_is_expired(Utc::now(), observation.observed_at)
            {
                return Ok(false);
            }
            observation.status = status;
            Ok(true)
        }
    }

    fn feed(ids: &[i64]) -> FakeHttp {
        let http = FakeHttp::default();
        let index: Vec<_> = ids
            .iter()
            .map(|id| serde_json::json!({ "id": id, "url": article_url(*id).unwrap() }))
            .collect();
        http.add(INDEX_URL, serde_json::to_vec(&index).unwrap());
        for id in ids {
            let url = article_url(*id).unwrap();
            let meta_url = format!("{BASE_URL}/patch-{id}/meta.json");
            http.add(
                &meta_url,
                serde_json::to_vec(&serde_json::json!({
                    "id": id, "source_url": "https://forums.playdeadlock.com/t/patch",
                    "urls": { "de": url },
                }))
                .unwrap(),
            );
            http.add(&url, "<html>Patch</html>");
        }
        http
    }

    async fn run(
        http: &FakeHttp,
        cursor: &FakeCursor,
        sent: &Arc<Mutex<Vec<i64>>>,
    ) -> Result<usize, PatchFeedError> {
        run_feed(http, cursor, |article| {
            let sent = Arc::clone(sent);
            async move {
                if cursor.advance(article.id) {
                    sent.lock().unwrap().push(article.id);
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
        })
        .await
    }

    #[tokio::test]
    async fn empty_first_index_does_not_bootstrap_then_existing_ids_are_historical() {
        let cursor = FakeCursor::default();
        let sent = Arc::new(Mutex::new(Vec::new()));
        let empty = feed(&[]);
        assert_eq!(run(&empty, &cursor, &sent).await.unwrap(), 0);
        assert!(!cursor.state.lock().unwrap().bootstrapped);

        let existing = feed(&[283, 284, 285, 286]);
        assert_eq!(run(&existing, &cursor, &sent).await.unwrap(), 0);
        assert!(cursor.state.lock().unwrap().bootstrapped);
        assert!(sent.lock().unwrap().is_empty());

        let next = feed(&[285, 286, 287]);
        assert_eq!(run(&next, &cursor, &sent).await.unwrap(), 1);
        assert_eq!(run(&next, &cursor, &sent).await.unwrap(), 0);
        assert_eq!(&*sent.lock().unwrap(), &[287]);
    }

    #[tokio::test]
    async fn multiple_new_ids_are_processed_in_order() {
        let cursor = FakeCursor::existing(285, None);
        let sent = Arc::new(Mutex::new(Vec::new()));
        let http = feed(&[288, 285, 287, 286]);
        assert_eq!(run(&http, &cursor, &sent).await.unwrap(), 3);
        assert_eq!(run(&http, &cursor, &sent).await.unwrap(), 0);
        assert_eq!(&*sent.lock().unwrap(), &[286, 287, 288]);
    }

    #[tokio::test]
    async fn failed_callback_stops_before_advancing_past_item() {
        let http = feed(&[285, 286, 287]);
        let cursor = FakeCursor::existing(285, None);
        let mut sent = Vec::new();
        let result = run_feed(&http, &cursor, |article| {
            sent.push(article.id);
            async {
                Err(PatchFeedError::InvalidFeed(
                    "Callback fehlgeschlagen".into(),
                ))
            }
        })
        .await;
        assert!(result.is_err());
        assert_eq!(sent, vec![286]);
        assert_eq!(cursor.current_status(286), Some("pending"));
        let observed_at = cursor.observed_at(286).unwrap();
        let mut retried = Vec::new();
        run_feed(&http, &cursor, |article| {
            let sent = cursor.advance(article.id);
            if sent {
                retried.push(article.id);
            }
            async move { Ok(sent) }
        })
        .await
        .unwrap();
        assert_eq!(retried, vec![286, 287]);
        assert_eq!(cursor.observed_at(286), Some(observed_at));
    }

    #[tokio::test]
    async fn concurrent_pollers_only_deliver_each_observation_once() {
        let cursor = FakeCursor::existing(285, None);
        let http = feed(&[286]);
        let sent = Arc::new(Mutex::new(Vec::new()));
        let first = run_feed(&http, &cursor, |article| {
            let id = article.id;
            let cursor = &cursor;
            let sent = Arc::clone(&sent);
            async move {
                if cursor.advance(id) {
                    sent.lock().unwrap().push(id);
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
        });
        let second = run_feed(&http, &cursor, |article| {
            let id = article.id;
            let cursor = &cursor;
            let sent = Arc::clone(&sent);
            async move {
                if cursor.advance(id) {
                    sent.lock().unwrap().push(id);
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
        });
        let (first, second) = tokio::join!(first, second);
        assert_eq!(first.unwrap() + second.unwrap(), 1);
        assert_eq!(&*sent.lock().unwrap(), &[286]);
    }

    #[tokio::test]
    async fn old_republished_patch_is_not_sent() {
        let http = feed(&[289, 286, 288, 287]);
        let cursor = FakeCursor::existing(288, None);
        let mut sent = Vec::new();
        run_feed(&http, &cursor, |article| {
            sent.push(article.id);
            async { Ok(true) }
        })
        .await
        .unwrap();
        assert_eq!(sent, vec![289]);
    }

    #[tokio::test]
    async fn rejects_foreign_links_and_redirects() {
        let cursor = FakeCursor::existing(285, None);
        let http = feed(&[286]);
        http.add(
            INDEX_URL,
            serde_json::to_vec(&serde_json::json!([{
                "id": 286, "url": "https://attacker.example/patchnotes/patch-286/"
            }]))
            .unwrap(),
        );
        assert!(run_feed(&http, &cursor, |_| async { Ok(true) })
            .await
            .is_err());
        let http = feed(&[286]);
        let meta_url = format!("{BASE_URL}/patch-286/meta.json");
        http.add_response(
            &meta_url,
            StatusCode::FOUND,
            "https://attacker.example/",
            "",
        );
        assert!(run_feed(&http, &cursor, |_| async { Ok(true) })
            .await
            .is_err());
        let http = feed(&[286]);
        http.add_response(INDEX_URL, StatusCode::OK, "https://attacker.example/", "[]");
        assert!(run_feed(&http, &cursor, |_| async { Ok(true) })
            .await
            .is_err());
    }

    #[tokio::test]
    async fn missing_article_or_transient_http_error_keeps_cursor() {
        let cursor = FakeCursor::existing(285, None);
        let http = feed(&[286, 287]);
        let url = article_url(286).unwrap();
        http.add_response(&url, StatusCode::NOT_FOUND, &url, "");
        let mut sent = Vec::new();
        assert!(run_feed(&http, &cursor, |article| {
            sent.push(article.id);
            async { Ok(true) }
        })
        .await
        .is_err());
        assert!(sent.is_empty());
        let http = feed(&[286, 287]);
        http.responses.lock().unwrap().remove(INDEX_URL);
        assert!(run_feed(&http, &cursor, |_| async { Ok(true) })
            .await
            .is_err());
    }

    #[tokio::test]
    async fn failed_first_fetch_does_not_create_baseline() {
        let cursor = FakeCursor::default();
        let unavailable = FakeHttp::default();
        assert!(run_feed(&unavailable, &cursor, |_| async { Ok(true) })
            .await
            .is_err());
        assert!(!cursor.state.lock().unwrap().bootstrapped);
        let malformed = FakeHttp::default();
        malformed.add(INDEX_URL, "{bad json");
        assert!(run_feed(&malformed, &cursor, |_| async { Ok(true) })
            .await
            .is_err());
        assert!(!cursor.state.lock().unwrap().bootstrapped);
        let published = feed(&[286]);
        assert_eq!(
            run_feed(&published, &cursor, |_| async { Ok(true) })
                .await
                .unwrap(),
            0
        );
        assert_eq!(cursor.current_status(286), Some("historical"));
    }

    #[tokio::test]
    async fn validated_source_url_reaches_callback_event() {
        let cursor = FakeCursor::existing(285, None);
        let http = feed(&[286]);
        let observed_at = DateTime::from_timestamp(1_000_000, 0).unwrap();
        let mut received = Vec::new();
        run_feed(&http, &cursor, |article| {
            received.push(article.into_patch_article(observed_at));
            async { Ok(true) }
        })
        .await
        .unwrap();
        assert_eq!(received.len(), 1);
        assert_eq!(
            received[0].source_url,
            "https://forums.playdeadlock.com/t/patch"
        );
        assert_eq!(received[0].url, article_url(286).unwrap());
        assert_eq!(received[0].observed_at, observed_at);

        let invalid = feed(&[286]);
        let meta_url = format!("{BASE_URL}/patch-286/meta.json");
        invalid.add(
            &meta_url,
            serde_json::to_vec(&serde_json::json!({
                "id": 286, "source_url": "https://attacker.example/t/patch",
                "urls": { "de": article_url(286).unwrap() },
            }))
            .unwrap(),
        );
        assert!(run_feed(&invalid, &cursor, |_| async { Ok(true) })
            .await
            .is_err());
    }

    #[tokio::test]
    async fn late_first_seen_lower_id_is_processed_after_newer_published_id() {
        let cursor = FakeCursor::existing(285, None);
        let sent = Arc::new(Mutex::new(Vec::new()));
        let first = feed(&[287, 285]);
        assert_eq!(run(&first, &cursor, &sent).await.unwrap(), 1);
        let later = feed(&[286, 287, 285]);
        assert_eq!(run(&later, &cursor, &sent).await.unwrap(), 1);
        assert_eq!(&*sent.lock().unwrap(), &[287, 286]);
    }

    #[tokio::test]
    async fn expired_missing_pending_is_terminal_then_later_id_proceeds() {
        let cursor = FakeCursor::existing(285, Some(286));
        cursor.set_observed_at(286, Utc::now() - chrono::Duration::seconds(121));
        let http = feed(&[287]);
        let mut sent = Vec::new();
        assert_eq!(
            run_feed(&http, &cursor, |article| {
                sent.push(article.id);
                async { Ok(true) }
            })
            .await
            .unwrap(),
            1
        );
        assert_eq!(
            cursor.current_status(286),
            Some("expired_missing_from_index")
        );
        assert_eq!(sent, vec![287]);
    }

    #[tokio::test]
    async fn expired_unavailable_pending_is_terminal_without_fetch_then_later_id_proceeds() {
        let cursor = FakeCursor::existing(285, Some(286));
        cursor.set_observed_at(286, Utc::now() - chrono::Duration::seconds(121));
        let http = feed(&[286, 287]);
        let url = article_url(286).unwrap();
        http.add_response(&url, StatusCode::NOT_FOUND, &url, "");
        let mut sent = Vec::new();
        assert_eq!(
            run_feed(&http, &cursor, |article| {
                sent.push(article.id);
                async { Ok(true) }
            })
            .await
            .unwrap(),
            1
        );
        assert_eq!(cursor.current_status(286), Some("expired_timeout"));
        assert_eq!(sent, vec![287]);
    }

    #[tokio::test]
    async fn feed_to_source_only_receiver_uses_isolated_postgres_runtime_role() {
        let (_postgres, admin, bot) = isolated_bot_database().await;
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "app-tok", "expires_in": 3600
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/helix/streams"))
            .and(query_param("user_id", "42"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"id": "s42", "user_id": "42", "game_id": "deadlock",
                    "game_name": "Deadlock", "started_at": "2020-01-01T00:00:00Z"}]
            })))
            .expect(2)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/helix/chat/messages"))
            .and(header("Authorization", "Bearer app-tok"))
            .and(body_partial_json(serde_json::json!({
                "broadcaster_id": "42", "sender_id": "777", "for_source_only": true
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"is_sent": true, "message_id": "sent-42"}]
            })))
            .expect(1)
            .mount(&server)
            .await;
        let mut config = HelixConfig::new("cid", "sec");
        config.helix_base = format!("{}/helix", server.uri());
        config.token_url = format!("{}/oauth2/token", server.uri());
        let helix = HelixClient::new(config).unwrap();
        let timeout_guard = Arc::new(tb_chat::moderation::TimeoutGuard::new());
        let tracked: Arc<dyn ChatApi> =
            Arc::new(tb_chat::timeout_tracking::TimeoutTrackingChatApi::new(
                Arc::new(MockEndpointChat {
                    helix: helix.clone(),
                }),
                Arc::clone(&timeout_guard),
                bot.clone(),
            ));
        let chat: Arc<dyn ChatApi> = Arc::new(tb_chat::channel_policy::ChannelPolicyChatApi::new(
            tracked,
            tb_chat::channel_policy::PolicyContext::Standard(
                crate::chat_wiring::patch_test_policy_roster(bot.clone()),
            ),
        ));
        let suppression = Arc::new(tb_chat::timeout_tracking::CombinedSuppression::new(
            Arc::new(tb_chat::moderation::OutboundSuppressionStore::new(
                bot.clone(),
            )),
            timeout_guard,
        ));
        let receiver = Arc::new(PatchReceiver::new(bot.clone(), helix, chat, suppression));

        assert_eq!(
            poll_with_receiver(&feed(&[]), &bot, &receiver)
                .await
                .unwrap(),
            0
        );
        let baseline: i64 = sqlx::query_scalar("SELECT count(*) FROM twitch_patch_feed_state")
            .fetch_one(&bot)
            .await
            .unwrap();
        assert_eq!(baseline, 0);
        assert_eq!(
            poll_with_receiver(&feed(&[284, 285]), &bot, &receiver)
                .await
                .unwrap(),
            0
        );
        let historical: Vec<(i64, String)> = sqlx::query_as(
            "SELECT patch_id, status FROM twitch_patch_feed_observations ORDER BY patch_id",
        )
        .fetch_all(&bot)
        .await
        .unwrap();
        assert_eq!(
            historical,
            vec![(284, "historical".into()), (285, "historical".into())]
        );
        let no_historical_recipients: i64 =
            sqlx::query_scalar("SELECT count(*) FROM twitch_patch_announcement_recipients")
                .fetch_one(&bot)
                .await
                .unwrap();
        assert_eq!(no_historical_recipients, 0);

        let published = feed(&[284, 285, 286]);
        distinct_source(&published, 286);
        assert_eq!(
            poll_with_receiver(&published, &bot, &receiver)
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            poll_with_receiver(&published, &bot, &receiver)
                .await
                .unwrap(),
            0
        );
        let recipients: Vec<(String, String)> = sqlx::query_as(
            "SELECT broadcaster_id, stream_id FROM twitch_patch_announcement_recipients \
             WHERE patch_id=286",
        )
        .fetch_all(&bot)
        .await
        .unwrap();
        assert_eq!(recipients, vec![("42".into(), "s42".into())]);
        let delivered: (String, bool) = sqlx::query_as(
            "SELECT status, attempted_at IS NOT NULL FROM twitch_patch_announcement_deliveries \
             WHERE broadcaster_id='42'",
        )
        .fetch_one(&bot)
        .await
        .unwrap();
        assert_eq!(delivered, ("sent".into(), true));

        let pending = observe_index(&bot, &[284, 285, 286, 287]).await.unwrap();
        assert_eq!(
            pending.iter().map(|entry| entry.id).collect::<Vec<_>>(),
            vec![287]
        );
        sqlx::raw_sql(
            "UPDATE twitch_streamers_partner_state SET is_partner_active=1 \
             WHERE twitch_user_id='43';
             UPDATE twitch_live_state SET last_game='Other' WHERE twitch_user_id='42';",
        )
        .execute(&admin)
        .await
        .unwrap();
        let fixed_recipients: Vec<(String, String)> = sqlx::query_as(
            "SELECT broadcaster_id, stream_id FROM twitch_patch_announcement_recipients \
             WHERE patch_id=287",
        )
        .fetch_all(&bot)
        .await
        .unwrap();
        assert_eq!(fixed_recipients, vec![("42".into(), "s42".into())]);
        Mock::given(method("GET"))
            .and(path("/helix/streams"))
            .and(query_param("user_id", "42"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"id": "s42", "user_id": "42", "game_id": "other",
                    "game_name": "Other", "started_at": "2020-01-01T00:00:00Z"}]
            })))
            .with_priority(1)
            .expect(1)
            .mount(&server)
            .await;
        let switched = feed(&[284, 285, 286, 287]);
        distinct_source(&switched, 286);
        distinct_source(&switched, 287);
        assert_eq!(
            poll_with_receiver(&switched, &bot, &receiver)
                .await
                .unwrap(),
            1
        );
        let no_stale_delivery: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM twitch_patch_announcement_deliveries \
             WHERE event_id=(SELECT event_id FROM twitch_patch_announcements \
             WHERE article_url='https://deutsche-deadlock-community.de/patchnotes/patch-287/')",
        )
        .fetch_one(&bot)
        .await
        .unwrap();
        assert_eq!(no_stale_delivery, 0);

        Mock::given(method("GET"))
            .and(path("/helix/streams"))
            .and(query_param("user_id", "43"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"id": "s43", "user_id": "43", "game_id": "deadlock",
                    "game_name": "Deadlock", "started_at": "2020-01-01T00:00:00Z"}]
            })))
            .expect(2)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/helix/chat/messages"))
            .and(body_partial_json(serde_json::json!({
                "broadcaster_id": "43", "sender_id": "777", "for_source_only": true
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"is_sent": true, "message_id": "sent-43"}]
            })))
            .expect(1)
            .mount(&server)
            .await;
        let late_lower_id = feed(&[283, 284, 285, 286, 287]);
        for id in [283, 286, 287] {
            distinct_source(&late_lower_id, id);
        }
        assert_eq!(
            poll_with_receiver(&late_lower_id, &bot, &receiver)
                .await
                .unwrap(),
            1
        );
        let lower_id_recipients: Vec<(String, String)> = sqlx::query_as(
            "SELECT broadcaster_id, stream_id FROM twitch_patch_announcement_recipients \
             WHERE patch_id=283",
        )
        .fetch_all(&bot)
        .await
        .unwrap();
        assert_eq!(lower_id_recipients, vec![("43".into(), "s43".into())]);
        assert_eq!(
            poll_with_receiver(&late_lower_id, &bot, &receiver)
                .await
                .unwrap(),
            0
        );

        sqlx::query("UPDATE twitch_streamers_partner_state SET is_partner_active=0 WHERE twitch_user_id='43'")
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO twitch_patch_feed_observations (patch_id, observed_at, status) \
             VALUES (288, now() - interval '121 seconds', 'pending')",
        )
        .execute(&admin)
        .await
        .unwrap();
        let after_expiry = feed(&[283, 284, 285, 286, 287, 288, 289]);
        for id in [283, 286, 287, 289] {
            distinct_source(&after_expiry, id);
        }
        assert_eq!(
            poll_with_receiver(&after_expiry, &bot, &receiver)
                .await
                .unwrap(),
            1
        );
        let final_statuses: Vec<(i64, String)> = sqlx::query_as(
            "SELECT patch_id, status FROM twitch_patch_feed_observations \
             WHERE patch_id IN (283, 286, 287, 288, 289) ORDER BY patch_id",
        )
        .fetch_all(&bot)
        .await
        .unwrap();
        assert_eq!(
            final_statuses,
            vec![
                (283, "processed".into()),
                (286, "processed".into()),
                (287, "processed".into()),
                (288, "expired_timeout".into()),
                (289, "processed".into()),
            ]
        );
        let unauthorized_update = sqlx::query(
            "UPDATE twitch_patch_feed_observations SET observed_at=now() WHERE patch_id=288",
        )
        .execute(&bot)
        .await
        .unwrap_err();
        assert_eq!(
            unauthorized_update
                .as_database_error()
                .and_then(|error| error.code())
                .as_deref(),
            Some("42501")
        );
        let rights: Vec<bool> = sqlx::query_scalar(
            "SELECT has_table_privilege('twitchdash', 'twitch_patch_announcement_recipients', 'SELECT') \
             UNION ALL SELECT NOT has_table_privilege('twitchdash', 'twitch_patch_announcement_recipients', 'INSERT') \
             UNION ALL SELECT NOT has_table_privilege('twitchlegacy', 'twitch_patch_announcements', 'SELECT')",
        )
        .fetch_all(&admin)
        .await
        .unwrap();
        assert_eq!(rights, vec![true; 3]);

        sqlx::query("UPDATE twitch_streamers_partner_state SET is_partner_active=1 WHERE twitch_user_id='43'")
            .execute(&admin)
            .await
            .unwrap();
        Mock::given(method("GET"))
            .and(path("/helix/streams"))
            .and(query_param("user_id", "43"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"id": "s43", "user_id": "43", "game_id": "deadlock",
                    "game_name": "Deadlock", "started_at": "2020-01-01T00:00:00Z"}]
            })))
            .with_priority(1)
            .expect(2)
            .mount(&server)
            .await;
        let entered = Arc::new(tokio::sync::Notify::new());
        let (release_tx, release_rx) = mpsc::channel();
        Mock::given(method("POST"))
            .and(path("/helix/chat/messages"))
            .and(body_partial_json(serde_json::json!({
                "broadcaster_id": "43", "sender_id": "777", "for_source_only": true
            })))
            .respond_with(PausedChatResponse {
                entered: Arc::clone(&entered),
                release: Mutex::new(release_rx),
            })
            .with_priority(1)
            .expect(1)
            .mount(&server)
            .await;
        let interrupted = feed(&[283, 284, 285, 286, 287, 288, 289, 290]);
        for id in [283, 286, 287, 289, 290] {
            distinct_source(&interrupted, id);
        }
        let retry_feed = feed(&[283, 284, 285, 286, 287, 288, 289, 290]);
        for id in [283, 286, 287, 289, 290] {
            distinct_source(&retry_feed, id);
        }
        let running_bot = bot.clone();
        let running_receiver = Arc::clone(&receiver);
        let mut interrupted_poll = tokio::spawn(async move {
            poll_with_receiver(&interrupted, &running_bot, &running_receiver).await
        });
        tokio::select! {
            _ = entered.notified() => {}
            result = &mut interrupted_poll => panic!("poll ended before chat send: {result:?}"),
        }
        let (attempt_status, attempt_saved): (String, bool) = sqlx::query_as(
            "SELECT d.status, d.attempted_at IS NOT NULL \
             FROM twitch_patch_announcement_deliveries d \
             JOIN twitch_patch_announcements a USING (event_id) \
             WHERE a.article_url='https://deutsche-deadlock-community.de/patchnotes/patch-290/'",
        )
        .fetch_one(&bot)
        .await
        .unwrap();
        assert_eq!(
            (attempt_status.as_str(), attempt_saved),
            ("attempted", true)
        );
        interrupted_poll.abort();
        assert!(interrupted_poll.await.unwrap_err().is_cancelled());
        release_tx.send(()).unwrap();
        assert_eq!(
            poll_with_receiver(&retry_feed, &bot, &receiver)
                .await
                .unwrap(),
            1
        );
        let (status, observation): (String, String) = sqlx::query_as(
            "SELECT d.status, o.status FROM twitch_patch_announcement_deliveries d \
             JOIN twitch_patch_announcements a USING (event_id) \
             JOIN twitch_patch_feed_observations o ON o.patch_id=290 \
             WHERE a.article_url='https://deutsche-deadlock-community.de/patchnotes/patch-290/'",
        )
        .fetch_one(&bot)
        .await
        .unwrap();
        assert_eq!(
            (status.as_str(), observation.as_str()),
            ("attempted", "processed")
        );
        let unavailable_article = feed(&[283, 284, 285, 286, 287, 288, 289, 290, 291]);
        let unavailable_url = article_url(291).unwrap();
        unavailable_article.add_response(
            &unavailable_url,
            StatusCode::NOT_FOUND,
            &unavailable_url,
            "",
        );
        assert!(poll_with_receiver(&unavailable_article, &bot, &receiver)
            .await
            .is_err());
        let pending_without_announcement: (String, i64) = sqlx::query_as(
            "SELECT o.status, (SELECT count(*) FROM twitch_patch_announcements \
             WHERE article_url='https://deutsche-deadlock-community.de/patchnotes/patch-291/') \
             FROM twitch_patch_feed_observations o WHERE o.patch_id=291",
        )
        .fetch_one(&bot)
        .await
        .unwrap();
        assert_eq!(pending_without_announcement, ("pending".into(), 0));
        server.verify().await;
    }
}
