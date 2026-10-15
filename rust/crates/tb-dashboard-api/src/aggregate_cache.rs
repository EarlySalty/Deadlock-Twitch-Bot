use std::{
    collections::HashMap,
    future::Future,
    sync::Arc,
    time::{Duration, Instant},
};

use axum::{
    body::{to_bytes, Body, Bytes},
    http::{
        header::{CACHE_CONTROL, SET_COOKIE, VARY},
        HeaderMap, HeaderValue, StatusCode,
    },
    response::{IntoResponse, Response},
};
use tokio::sync::{Mutex, Semaphore};

const TTL: Duration = Duration::from_secs(60);
const MAX_KEYS: usize = 256;

type Slot = Arc<Mutex<Option<CachedResponse>>>;

#[derive(Clone)]
pub struct AggregateCache {
    slots: Arc<Mutex<HashMap<String, Slot>>>,
    computations: Arc<Semaphore>,
}

impl Default for AggregateCache {
    fn default() -> Self {
        Self {
            slots: Arc::default(),
            computations: Arc::new(Semaphore::new(2)),
        }
    }
}

struct CachedResponse {
    created: Instant,
    status: StatusCode,
    headers: HeaderMap,
    body: Bytes,
}

impl CachedResponse {
    fn response(&self) -> Response {
        let mut response = Response::new(Body::from(self.body.clone()));
        *response.status_mut() = self.status;
        *response.headers_mut() = self.headers.clone();
        response
    }
}

impl AggregateCache {
    pub async fn respond<F>(&self, key: String, compute: F) -> Response
    where
        F: Future<Output = Response>,
    {
        let slot = {
            let mut slots = self.slots.lock().await;
            if !slots.contains_key(&key) && slots.len() >= MAX_KEYS {
                slots.retain(|_, slot| {
                    Arc::strong_count(slot) > 1
                        || slot.try_lock().map_or(true, |entry| {
                            entry
                                .as_ref()
                                .is_some_and(|value| value.created.elapsed() < TTL)
                        })
                });
                if slots.len() >= MAX_KEYS {
                    drop(slots);
                    return self.compute(compute).await;
                }
            }
            slots.entry(key).or_default().clone()
        };
        let mut entry = slot.lock().await;
        if let Some(cached) = entry
            .as_ref()
            .filter(|cached| cached.created.elapsed() < TTL)
        {
            return cached.response();
        }
        let response = self.compute(compute).await;
        if !response.status().is_success()
            || response.headers().contains_key(SET_COOKIE)
            || response.headers().contains_key(CACHE_CONTROL)
        {
            return response;
        }
        let (mut parts, body) = response.into_parts();
        let body = match to_bytes(body, usize::MAX).await {
            Ok(body) => body,
            Err(error) => {
                tracing::error!(%error, "aggregate response buffering failed");
                return crate::auth::analytics_request_failed_json().into_response();
            }
        };
        if !parts.headers.contains_key(CACHE_CONTROL) {
            parts.headers.insert(
                CACHE_CONTROL,
                HeaderValue::from_static("private, max-age=30"),
            );
        }
        parts
            .headers
            .append(VARY, HeaderValue::from_static("Cookie, X-Internal-Token"));
        let cached = CachedResponse {
            created: Instant::now(),
            status: parts.status,
            headers: parts.headers,
            body,
        };
        let response = cached.response();
        if cached.body.len() <= 1024 * 1024 {
            *entry = Some(cached);
        }
        response
    }
    async fn compute<F>(&self, compute: F) -> Response
    where
        F: Future<Output = Response>,
    {
        let _permit = match self.computations.acquire().await {
            Ok(permit) => permit,
            Err(_) => return crate::auth::analytics_request_failed_json().into_response(),
        };
        compute.await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn same_key_shares_computation_and_preserves_bytes() {
        let cache = AggregateCache::default();
        let calls = AtomicUsize::new(0);
        let compute = || async {
            calls.fetch_add(1, Ordering::SeqCst);
            tokio::task::yield_now().await;
            "{\"value\":1}".into_response()
        };
        let (first, second) = tokio::join!(
            cache.respond("one".into(), compute()),
            cache.respond("one".into(), compute())
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(first.headers()[CACHE_CONTROL], "private, max-age=30");
        assert_eq!(first.headers()[VARY], "Cookie, X-Internal-Token");
        assert_eq!(
            to_bytes(first.into_body(), 100).await.expect("body"),
            to_bytes(second.into_body(), 100).await.expect("body")
        );
        cache.respond("two".into(), compute()).await;
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn failures_are_not_cached() {
        let cache = AggregateCache::default();
        for _ in 0..2 {
            let response = cache
                .respond("one".into(), async {
                    StatusCode::INTERNAL_SERVER_ERROR.into_response()
                })
                .await;
            assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        }
        let response = cache
            .respond("one".into(), async { "ok".into_response() })
            .await;
        assert_eq!(response.status(), StatusCode::OK);
    }
    #[tokio::test]
    async fn expired_entries_are_replaced_and_policy_is_respected() {
        let cache = AggregateCache::default();
        cache
            .respond("key".into(), async { "old".into_response() })
            .await;
        let slot = cache.slots.lock().await.get("key").expect("slot").clone();
        slot.lock().await.as_mut().expect("entry").created = Instant::now() - TTL;
        let response = cache
            .respond("key".into(), async { "new".into_response() })
            .await;
        assert_eq!(
            to_bytes(response.into_body(), 100).await.expect("body"),
            "new"
        );
        cache
            .respond("uncached".into(), async {
                ([(CACHE_CONTROL, "no-store")], "private").into_response()
            })
            .await;
        let slot = cache
            .slots
            .lock()
            .await
            .get("uncached")
            .expect("slot")
            .clone();
        assert!(slot.lock().await.is_none());
    }
}
