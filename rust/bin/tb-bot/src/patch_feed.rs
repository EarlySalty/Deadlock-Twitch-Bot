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

trait FeedCursor {
    fn read_or_init(
        &self,
        newest_id: i64,
    ) -> impl Future<Output = Result<(i64, Option<i64>), PatchFeedError>> + Send;
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
    let newest_id = entries.last().map_or(0, |entry| entry.id);
    let (cursor, pending) = store.read_or_init(newest_id).await?;
    if let Some(pending) = pending {
        if pending > cursor && !entries.iter().any(|entry| entry.id == pending) {
            return Err(PatchFeedError::InvalidFeed(format!(
                "Offener Patch {pending} fehlt im Index"
            )));
        }
    }
    let floor = pending.filter(|id| *id > cursor).unwrap_or(cursor);
    let mut count = 0;
    for entry in entries
        .into_iter()
        .filter(|entry| entry.id > cursor && entry.id >= floor)
    {
        let article = fetch_article(http, &entry).await?;
        if deliver(article).await? {
            count += 1;
        }
    }
    Ok(count)
}

async fn read_progress(
    pool: &PgPool,
    newest_id: i64,
) -> Result<(i64, Option<i64>), PatchFeedError> {
    sqlx::query(
        "INSERT INTO twitch_patch_feed_progress (singleton, last_processed_patch_id) \
         VALUES (TRUE, $1) ON CONFLICT (singleton) DO NOTHING",
    )
    .bind(newest_id)
    .execute(pool)
    .await?;
    let row = sqlx::query(
        "SELECT last_processed_patch_id, pending_patch_id \
         FROM twitch_patch_feed_progress WHERE singleton = TRUE",
    )
    .fetch_one(pool)
    .await?;
    Ok((
        row.try_get("last_processed_patch_id")?,
        row.try_get("pending_patch_id")?,
    ))
}

impl FeedCursor for PgPool {
    async fn read_or_init(&self, newest_id: i64) -> Result<(i64, Option<i64>), PatchFeedError> {
        read_progress(self, newest_id).await
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
    let mut claim = pool.begin().await?;
    let row = sqlx::query(
        "SELECT last_processed_patch_id, pending_patch_id, pending_observed_at \
         FROM twitch_patch_feed_progress WHERE singleton = TRUE FOR UPDATE",
    )
    .fetch_one(&mut *claim)
    .await?;
    let cursor: i64 = row.try_get("last_processed_patch_id")?;
    let pending: Option<i64> = row.try_get("pending_patch_id")?;
    if article.id <= cursor {
        claim.commit().await?;
        return Ok(false);
    }
    if let Some(pending) = pending {
        if pending != article.id {
            return Err(PatchFeedError::InvalidFeed(format!(
                "Patch {pending} wartet noch vor Patch {}",
                article.id
            )));
        }
        let observed_at: Option<DateTime<Utc>> = row.try_get("pending_observed_at")?;
        if observed_at.is_none() {
            return Err(PatchFeedError::InvalidFeed("Beobachtungszeit fehlt".into()));
        }
    } else {
        sqlx::query(
            "UPDATE twitch_patch_feed_progress \
             SET pending_patch_id = $1, pending_observed_at = NOW() \
             WHERE singleton = TRUE",
        )
        .bind(article.id)
        .execute(&mut *claim)
        .await?;
    }
    claim.commit().await?;

    let mut delivery = pool.begin().await?;
    let row = sqlx::query(
        "SELECT last_processed_patch_id, pending_patch_id, pending_observed_at \
         FROM twitch_patch_feed_progress WHERE singleton = TRUE FOR UPDATE",
    )
    .fetch_one(&mut *delivery)
    .await?;
    let cursor: i64 = row.try_get("last_processed_patch_id")?;
    if article.id <= cursor {
        delivery.commit().await?;
        return Ok(false);
    }
    let pending: Option<i64> = row.try_get("pending_patch_id")?;
    if pending != Some(article.id) {
        return Err(PatchFeedError::InvalidFeed(format!(
            "Patch {} hat keinen gültigen Claim",
            article.id
        )));
    }
    let observed_at: DateTime<Utc> = row
        .try_get::<Option<DateTime<Utc>>, _>("pending_observed_at")?
        .ok_or_else(|| PatchFeedError::InvalidFeed("Beobachtungszeit fehlt".into()))?;
    let id = article.id;
    callback(article.into_patch_article(observed_at))
        .await
        .map_err(|error| PatchFeedError::Callback(Box::new(error)))?;
    sqlx::query(
        "UPDATE twitch_patch_feed_progress \
         SET last_processed_patch_id = $1, pending_patch_id = NULL, pending_observed_at = NULL \
         WHERE singleton = TRUE",
    )
    .bind(id)
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

    #[derive(Default)]
    struct FakeCursor {
        progress: Mutex<Option<(i64, Option<i64>)>>,
    }

    impl FakeCursor {
        fn existing(cursor: i64, pending: Option<i64>) -> Self {
            Self {
                progress: Mutex::new(Some((cursor, pending))),
            }
        }

        fn advance(&self, id: i64) {
            *self.progress.lock().unwrap() = Some((id, None));
        }

        fn set_pending(&self, id: i64) {
            self.progress.lock().unwrap().as_mut().unwrap().1 = Some(id);
        }

        fn current(&self) -> Option<(i64, Option<i64>)> {
            *self.progress.lock().unwrap()
        }
    }

    impl FeedCursor for FakeCursor {
        async fn read_or_init(&self, newest_id: i64) -> Result<(i64, Option<i64>), PatchFeedError> {
            let mut progress = self.progress.lock().unwrap();
            Ok(*progress.get_or_insert((newest_id, None)))
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
                cursor.advance(article.id);
                sent.lock().unwrap().push(article.id);
                Ok(true)
            }
        })
        .await
    }

    #[tokio::test]
    async fn bootstrap_uses_latest_published_id_then_restart_sends_only_newer() {
        let cursor = FakeCursor::default();
        let sent = Arc::new(Mutex::new(Vec::new()));
        let first = feed(&[283, 284, 285, 286]);
        assert_eq!(run(&first, &cursor, &sent).await.unwrap(), 0);
        assert_eq!(cursor.current(), Some((286, None)));
        let next = feed(&[287, 285, 286]);
        assert_eq!(run(&next, &cursor, &sent).await.unwrap(), 1);
        assert_eq!(run(&next, &cursor, &sent).await.unwrap(), 0);
        assert_eq!(&*sent.lock().unwrap(), &[287]);
        assert_eq!(cursor.current(), Some((287, None)));
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
        cursor.set_pending(286);
        let mut retried = Vec::new();
        run_feed(&http, &cursor, |article| {
            retried.push(article.id);
            cursor.advance(article.id);
            async { Ok(true) }
        })
        .await
        .unwrap();
        assert_eq!(retried, vec![286, 287]);
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
        assert_eq!(cursor.current(), None);
        let malformed = FakeHttp::default();
        malformed.add(INDEX_URL, "{bad json");
        assert!(run_feed(&malformed, &cursor, |_| async { Ok(true) })
            .await
            .is_err());
        assert_eq!(cursor.current(), None);
        let published = feed(&[286]);
        assert_eq!(
            run_feed(&published, &cursor, |_| async { Ok(true) })
                .await
                .unwrap(),
            0
        );
        assert_eq!(cursor.current(), Some((286, None)));
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
    async fn pending_patch_precedes_late_older_index_item() {
        let http = feed(&[286, 287, 288]);
        let cursor = FakeCursor::existing(285, Some(287));
        let mut sent = Vec::new();
        run_feed(&http, &cursor, |article| {
            sent.push(article.id);
            async { Ok(true) }
        })
        .await
        .unwrap();
        assert_eq!(sent, vec![287, 288]);
    }

    #[tokio::test]
    async fn missing_pending_patch_is_not_skipped() {
        let http = feed(&[287]);
        let cursor = FakeCursor::existing(285, Some(286));
        assert!(run_feed(&http, &cursor, |_| async { Ok(true) })
            .await
            .is_err());
    }
}
