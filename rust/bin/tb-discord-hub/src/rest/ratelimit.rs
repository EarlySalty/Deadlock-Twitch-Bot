use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use reqwest::header::HeaderMap;
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};
use tokio::time::Instant;

#[derive(Debug, Default)]
struct BucketState {
    remaining: Option<u64>,
    reset_at: Option<Instant>,
    blocked_until: Option<Instant>,
}

type Bucket = Arc<AsyncMutex<BucketState>>;

#[derive(Debug, Default)]
struct Inner {
    route_bucket: HashMap<String, String>,
    routes: HashMap<RouteKey, Bucket>,
    buckets: HashMap<(String, String), Bucket>,
    global_until: Option<Instant>,
}

#[derive(Debug, Default)]
pub(crate) struct RateLimiter {
    inner: Mutex<Inner>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct RouteKey {
    pub template: String,
    pub major: String,
}

impl RouteKey {
    pub(crate) fn new(method: &str, path: &str) -> Self {
        let path = path.split('?').next().unwrap_or_default();
        let mut parts = Vec::new();
        let mut major = String::new();
        let mut previous = "";
        for segment in path.split('/') {
            let is_id = !segment.is_empty() && segment.bytes().all(|b| b.is_ascii_digit());
            if is_id {
                if major.is_empty() && matches!(previous, "channels" | "guilds" | "webhooks") {
                    major = segment.to_string();
                    parts.push("{major}".to_string());
                } else {
                    parts.push("{id}".to_string());
                }
            } else if previous == "reactions" {
                parts.push("{emoji}".to_string());
            } else {
                parts.push(segment.to_string());
            }
            previous = segment;
        }
        Self {
            template: format!("{method} {}", parts.join("/")),
            major,
        }
    }
}

pub(crate) struct BucketGuard<'a> {
    limiter: &'a RateLimiter,
    route: RouteKey,
    state: OwnedMutexGuard<BucketState>,
}

impl BucketGuard<'_> {
    pub(crate) async fn update(mut self, headers: &HeaderMap) -> Self {
        let header = |name: &str| {
            headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::trim)
                .filter(|v| !v.is_empty())
        };
        let mut discovered = false;
        if let Some(hash) = header("x-ratelimit-bucket") {
            let bucket = {
                let mut inner = self.limiter.lock();
                inner
                    .route_bucket
                    .insert(self.route.template.clone(), hash.to_string());
                inner
                    .buckets
                    .entry((hash.to_string(), self.route.major.clone()))
                    .or_insert_with(|| OwnedMutexGuard::mutex(&self.state).clone())
                    .clone()
            };
            if !Arc::ptr_eq(&bucket, OwnedMutexGuard::mutex(&self.state)) {
                drop(self.state);
                self.state = bucket.lock_owned().await;
                discovered = true;
            }
        }
        let remaining = header("x-ratelimit-remaining").and_then(|v| v.parse::<u64>().ok());
        let reset_at = header("x-ratelimit-reset-after")
            .and_then(|v| v.parse::<f64>().ok())
            .and_then(|v| Duration::try_from_secs_f64(v).ok())
            .and_then(|wait| Instant::now().checked_add(wait.max(Duration::from_millis(1))));
        if let (Some(remaining), Some(reset_at)) = (remaining, reset_at) {
            self.state.remaining = Some(if discovered {
                self.state
                    .remaining
                    .map_or(remaining, |old| old.min(remaining))
            } else {
                remaining
            });
            self.state.reset_at = Some(if discovered {
                self.state
                    .reset_at
                    .map_or(reset_at, |old| old.max(reset_at))
            } else {
                reset_at
            });
        }
        self
    }

    pub(crate) fn block(&mut self, retry_after: Duration, global: bool) {
        let Some(until) = Instant::now().checked_add(retry_after.max(Duration::from_millis(1)))
        else {
            return;
        };
        if global {
            let mut inner = self.limiter.lock();
            inner.global_until = Some(inner.global_until.map_or(until, |old| old.max(until)));
        } else {
            self.state.blocked_until =
                Some(self.state.blocked_until.map_or(until, |old| old.max(until)));
        }
    }
}

impl RateLimiter {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn bucket(&self, route: &RouteKey) -> Bucket {
        let mut inner = self.lock();
        if let Some(hash) = inner.route_bucket.get(&route.template).cloned() {
            inner
                .buckets
                .entry((hash, route.major.clone()))
                .or_default()
                .clone()
        } else {
            inner.routes.entry(route.clone()).or_default().clone()
        }
    }

    pub(crate) async fn acquire(
        &self,
        route: &RouteKey,
        max_wait: Duration,
    ) -> Result<BucketGuard<'_>, ()> {
        tokio::time::timeout(max_wait, async {
            loop {
                let bucket = self.bucket(route);
                let mut state = bucket.clone().lock_owned().await;
                if !Arc::ptr_eq(&bucket, &self.bucket(route)) {
                    continue;
                }
                loop {
                    let now = Instant::now();
                    if state.reset_at.is_some_and(|until| until <= now) {
                        state.remaining = None;
                        state.reset_at = None;
                    }
                    let until = self
                        .lock()
                        .global_until
                        .into_iter()
                        .chain(state.blocked_until)
                        .chain(state.reset_at.filter(|_| state.remaining == Some(0)))
                        .filter(|until| *until > now)
                        .max();
                    if let Some(until) = until {
                        tokio::time::sleep_until(until).await;
                    } else {
                        if let Some(remaining) = &mut state.remaining {
                            *remaining = remaining.saturating_sub(1);
                        }
                        return BucketGuard {
                            limiter: self,
                            route: route.clone(),
                            state,
                        };
                    }
                }
            }
        })
        .await
        .map_err(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::HeaderValue;
    use std::future::Future;
    use std::task::Poll;

    fn headers(hash: &'static str, remaining: &'static str, reset: &'static str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("x-ratelimit-bucket", HeaderValue::from_static(hash));
        headers.insert("x-ratelimit-remaining", HeaderValue::from_static(remaining));
        headers.insert("x-ratelimit-reset-after", HeaderValue::from_static(reset));
        headers
    }

    async fn pending<F: Future>(mut future: std::pin::Pin<&mut F>) {
        std::future::poll_fn(|cx| {
            assert!(future.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
    }

    #[test]
    fn route_key_trennt_major_und_neben_ids() {
        let a = RouteKey::new("POST", "/channels/11/messages");
        let b = RouteKey::new("POST", "/channels/22/messages");
        assert_eq!(a.template, b.template);
        assert_eq!(a.major, "11");
        let c = RouteKey::new("PUT", "/guilds/5/members/7/roles/9");
        assert_eq!(c.template, "PUT /guilds/{major}/members/{id}/roles/{id}");
        assert_eq!(c.major, "5");
    }

    #[tokio::test(start_paused = true)]
    async fn same_bucket_is_serialized_across_routes() {
        let limiter = RateLimiter::default();
        let a = RouteKey::new("GET", "/channels/1/messages/5");
        let b = RouteKey::new("DELETE", "/channels/1/messages/5");
        for route in [&a, &b] {
            limiter
                .acquire(route, Duration::from_secs(10))
                .await
                .expect("guard")
                .update(&headers("shared", "3", "60"))
                .await;
        }
        let first = limiter
            .acquire(&a, Duration::from_secs(10))
            .await
            .expect("first");
        let next = limiter.acquire(&b, Duration::from_secs(10));
        tokio::pin!(next);
        pending(next.as_mut()).await;
        drop(first);
        assert!(next.await.is_ok());
    }

    #[tokio::test(start_paused = true)]
    async fn unknown_route_serialization_and_cancellation_release() {
        let limiter = RateLimiter::default();
        let route = RouteKey::new("GET", "/users/1");
        let mut in_flight = Box::pin(async {
            let _guard = limiter
                .acquire(&route, Duration::from_secs(10))
                .await
                .expect("first");
            std::future::pending::<()>().await;
        });
        pending(in_flight.as_mut()).await;
        let mut cancelled = Box::pin(limiter.acquire(&route, Duration::from_secs(10)));
        pending(cancelled.as_mut()).await;
        drop(cancelled);
        drop(in_flight);
        assert!(limiter
            .acquire(&route, Duration::from_secs(10))
            .await
            .is_ok());
    }

    #[tokio::test(start_paused = true)]
    async fn different_buckets_and_major_parameters_run_in_parallel() {
        let limiter = RateLimiter::default();
        let a = RouteKey::new("GET", "/channels/1/messages");
        let b = RouteKey::new("GET", "/channels/2/messages");
        let c = RouteKey::new("GET", "/guilds/1/members");
        let first = limiter
            .acquire(&a, Duration::from_secs(10))
            .await
            .expect("first");
        let _first = first.update(&headers("messages", "3", "60")).await;
        let second = limiter
            .acquire(&b, Duration::from_secs(10))
            .await
            .expect("second");
        let _second = second.update(&headers("messages", "3", "60")).await;
        let third = limiter
            .acquire(&c, Duration::from_secs(10))
            .await
            .expect("third");
        let _third = third.update(&headers("members", "3", "60")).await;
    }

    #[tokio::test(start_paused = true)]
    async fn retry_after_delays_next_request_even_without_bucket_header() {
        let limiter = RateLimiter::default();
        let route = RouteKey::new("POST", "/channels/1/messages");
        let mut first = limiter
            .acquire(&route, Duration::from_secs(10))
            .await
            .expect("first");
        first.block(Duration::from_secs(5), false);
        first.block(Duration::from_secs(1), false);
        drop(first);
        let next = limiter.acquire(&route, Duration::from_secs(10));
        tokio::pin!(next);
        pending(next.as_mut()).await;
        tokio::time::advance(Duration::from_secs(4)).await;
        pending(next.as_mut()).await;
        tokio::time::advance(Duration::from_secs(1)).await;
        assert!(next.await.is_ok());
    }

    #[tokio::test(start_paused = true)]
    async fn global_429_blocks_other_buckets_and_rechecks_extended_deadline() {
        let limiter = RateLimiter::default();
        let a = RouteKey::new("GET", "/users/1");
        let b = RouteKey::new("GET", "/guilds/2/members");
        let mut first = limiter
            .acquire(&a, Duration::from_secs(20))
            .await
            .expect("first");
        first.block(Duration::from_secs(5), true);
        let next = limiter.acquire(&b, Duration::from_secs(20));
        tokio::pin!(next);
        pending(next.as_mut()).await;
        tokio::time::advance(Duration::from_secs(4)).await;
        first.block(Duration::from_secs(5), true);
        tokio::time::advance(Duration::from_secs(1)).await;
        pending(next.as_mut()).await;
        tokio::time::advance(Duration::from_secs(4)).await;
        assert!(next.await.is_ok());
    }

    #[tokio::test(start_paused = true)]
    async fn exhausted_bucket_waits_for_reset_and_invalid_header_is_ignored() {
        let limiter = RateLimiter::default();
        let route = RouteKey::new("GET", "/guilds/42/members");
        let guard = limiter
            .acquire(&route, Duration::from_secs(10))
            .await
            .expect("guard");
        let guard = guard.update(&headers("quota", "0", "5")).await;
        drop(guard);
        let start = Instant::now();
        let next = limiter
            .acquire(&route, Duration::from_secs(10))
            .await
            .expect("reset");
        assert_eq!(start.elapsed(), Duration::from_secs(5));
        let next = next.update(&headers("quota", "0", "1e100")).await;
        drop(next);
        assert!(limiter
            .acquire(&route, Duration::from_secs(10))
            .await
            .is_ok());
    }
}
