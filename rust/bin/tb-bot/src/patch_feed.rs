use std::collections::HashSet;
use std::error::Error;
use std::future::Future;
use std::time::Duration;

use chrono::{DateTime, Utc};
use reqwest::{redirect::Policy, Client, StatusCode};
use serde::Deserialize;
use sqlx::{PgPool, Row};

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
    now.signed_duration_since(observed_at)
        >= chrono::Duration::seconds(MAX_PENDING_AGE_SECONDS)
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
                .bind(bootstrapped_at.clone())
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
    callback(article.into_patch_article(observed_at))
        .await
        .map_err(|error| PatchFeedError::Callback(Box::new(error)))?;
    sqlx::query(
        "UPDATE twitch_patch_feed_observations \
         SET status = 'processed', finalized_at = NOW() WHERE patch_id = $1 AND status = 'pending'",
    )
    .bind(article.id)
    .execute(&mut *delivery)
    .await?;
    delivery.commit().await?;
    Ok(true)
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
    let callback = tokio::sync::Mutex::new(callback);
    run_feed(client, pool, |article| async {
        let mut callback = callback.lock().await;
        deliver_article(pool, article, &mut *callback).await
    })
    .await
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use super::*;

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
                .map(|observation| observation.observed_at.clone())
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
                    observed_at: observation.observed_at.clone(),
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
        assert_eq!(cursor.current_status(286), Some("expired_missing_from_index"));
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
}
