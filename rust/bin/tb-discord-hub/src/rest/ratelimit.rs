use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use reqwest::header::HeaderMap;
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};
use tokio::time::Instant;

const MAX_BUCKETS: usize = 4096;

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

impl Inner {
    fn prune(&mut self) {
        if self.routes.len() + self.buckets.len() < MAX_BUCKETS
            && self.route_bucket.len() < MAX_BUCKETS
        {
            return;
        }
        let now = Instant::now();
        let retain = |bucket: &Bucket| {
            Arc::strong_count(bucket) > 1
                || bucket.try_lock().map_or(true, |state| {
                    state.reset_at.is_some_and(|until| until > now)
                        || state.blocked_until.is_some_and(|until| until > now)
                })
        };
        self.routes.retain(|_, bucket| retain(bucket));
        self.buckets.retain(|_, bucket| retain(bucket));
        let hashes: HashSet<_> = self.buckets.keys().map(|(hash, _)| hash.as_str()).collect();
        let templates: HashSet<_> = self
            .routes
            .keys()
            .map(|route| route.template.as_str())
            .collect();
        self.route_bucket.retain(|template, hash| {
            hashes.contains(hash.as_str()) || templates.contains(template.as_str())
        });
    }
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
    binding_route: bool,
}

impl Drop for BucketGuard<'_> {
    fn drop(&mut self) {
        if self.binding_route {
            self.limiter.lock().routes.remove(&self.route);
        }
    }
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
            let candidate = Bucket::default();
            let candidate_state = candidate.clone().lock_owned().await;
            let (bucket, unknown) = {
                let mut inner = self.limiter.lock();
                inner.prune();
                let unknown = inner.routes.contains_key(&self.route);
                inner
                    .route_bucket
                    .insert(self.route.template.clone(), hash.to_string());
                let bucket = inner
                    .buckets
                    .entry((hash.to_string(), self.route.major.clone()))
                    .or_insert_with(|| candidate.clone())
                    .clone();
                (bucket, unknown)
            };
            if Arc::ptr_eq(&bucket, &candidate) {
                self.limiter.lock().routes.remove(&self.route);
                self.state = candidate_state;
            } else if !Arc::ptr_eq(&bucket, OwnedMutexGuard::mutex(&self.state)) {
                if unknown {
                    self.binding_route = true;
                    let state = bucket.lock_owned().await;
                    self.limiter.lock().routes.remove(&self.route);
                    self.state = state;
                    self.binding_route = false;
                } else {
                    self.state = candidate_state;
                    self.state = bucket.lock_owned().await;
                }
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

    fn bucket(&self, route: &RouteKey) -> Result<Bucket, ()> {
        let mut inner = self.lock();
        inner.prune();
        let count = inner.routes.len() + inner.buckets.len();
        if count > MAX_BUCKETS || inner.route_bucket.len() > MAX_BUCKETS {
            return Err(());
        }
        if let Some(bucket) = inner.routes.get(route) {
            return Ok(bucket.clone());
        }
        if let Some(hash) = inner.route_bucket.get(&route.template).cloned() {
            let key = (hash, route.major.clone());
            if count >= MAX_BUCKETS && !inner.buckets.contains_key(&key) {
                return Err(());
            }
            Ok(inner.buckets.entry(key).or_default().clone())
        } else {
            if inner.route_bucket.len() >= MAX_BUCKETS || count >= MAX_BUCKETS {
                return Err(());
            }
            Ok(inner.routes.entry(route.clone()).or_default().clone())
        }
    }

    pub(crate) async fn acquire(
        &self,
        route: &RouteKey,
        max_wait: Duration,
    ) -> Result<BucketGuard<'_>, ()> {
        tokio::time::timeout(max_wait, async {
            loop {
                let bucket = self.bucket(route)?;
                let mut state = bucket.clone().lock_owned().await;
                if !Arc::ptr_eq(&bucket, &self.bucket(route)?) {
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
                        return Ok(BucketGuard {
                            limiter: self,
                            route: route.clone(),
                            state,
                            binding_route: false,
                        });
                    }
                }
            }
        })
        .await
        .map_err(|_| ())?
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
    async fn shared_bucket_binding_preserves_route_serialization_and_cancellation() {
        for cancel in [false, true] {
            let limiter = RateLimiter::default();
            let known = RouteKey::new("GET", "/channels/1/messages");
            let unknown = RouteKey::new("DELETE", "/channels/1/messages/5");
            let held = limiter
                .acquire(&known, Duration::from_secs(10))
                .await
                .expect("known");
            let held = held.update(&headers("shared", "3", "60")).await;
            let first = limiter
                .acquire(&unknown, Duration::from_secs(10))
                .await
                .expect("unknown");
            let mut next = Box::pin(limiter.acquire(&unknown, Duration::from_secs(10)));
            pending(next.as_mut()).await;
            let response = headers("shared", "1", "60");
            let mut binding = Box::pin(first.update(&response));
            pending(binding.as_mut()).await;
            pending(next.as_mut()).await;
            if cancel {
                drop(binding);
                pending(next.as_mut()).await;
                drop(held);
            } else {
                drop(held);
                let mut first = binding.await;
                first.block(Duration::from_secs(5), false);
                drop(first);
                tokio::time::advance(Duration::from_secs(4)).await;
                pending(next.as_mut()).await;
                tokio::time::advance(Duration::from_secs(1)).await;
            }
            assert!(next.await.is_ok());
        }
    }

    #[tokio::test(start_paused = true)]
    async fn discovery_keeps_waiters_serialized_and_hash_changes_do_not_alias_state() {
        let limiter = RateLimiter::default();
        let route = RouteKey::new("GET", "/channels/1/messages");
        let first = limiter
            .acquire(&route, Duration::from_secs(10))
            .await
            .expect("first");
        let other = RouteKey::new("GET", "/channels/2/messages");
        limiter
            .acquire(&other, Duration::from_secs(10))
            .await
            .expect("other")
            .update(&headers("old", "3", "60"))
            .await;
        let next = limiter.acquire(&route, Duration::from_secs(10));
        tokio::pin!(next);
        pending(next.as_mut()).await;
        let first = first.update(&headers("old", "3", "60")).await;
        pending(next.as_mut()).await;
        drop(first);
        let second = next.await.expect("second");
        let _second = second.update(&headers("new", "3", "60")).await;
        let inner = limiter.lock();
        assert!(!Arc::ptr_eq(
            inner.buckets.get(&("old".into(), "1".into())).expect("old"),
            inner.buckets.get(&("new".into(), "1".into())).expect("new"),
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn idle_routes_are_reclaimed_without_evicting_an_active_guard() {
        let limiter = RateLimiter::default();
        let route = RouteKey::new("GET", "/channels/1/messages");
        let first = limiter
            .acquire(&route, Duration::from_secs(10))
            .await
            .expect("first");
        let first = first.update(&headers("active", "3", "60")).await;
        for major in 2..MAX_BUCKETS * 2 {
            let other = RouteKey::new("GET", &format!("/guilds/{major}/members"));
            drop(
                limiter
                    .acquire(&other, Duration::from_secs(10))
                    .await
                    .expect("idle"),
            );
        }
        {
            let inner = limiter.lock();
            assert!(inner.routes.len() + inner.buckets.len() <= MAX_BUCKETS);
        }
        let next = limiter.acquire(&route, Duration::from_secs(10));
        tokio::pin!(next);
        pending(next.as_mut()).await;
        drop(first);
        assert!(next.await.is_ok());
    }

    #[tokio::test(start_paused = true)]
    async fn full_registry_preserves_deadlines_and_recovers_after_expiry() {
        let limiter = RateLimiter::default();
        for major in 1..=MAX_BUCKETS {
            let route = RouteKey::new("GET", &format!("/channels/{major}/messages"));
            limiter
                .acquire(&route, Duration::from_secs(10))
                .await
                .expect("guard")
                .update(&headers("quota", "0", "60"))
                .await;
        }
        let other = RouteKey::new("GET", "/channels/99999/messages");
        assert!(limiter
            .acquire(&other, Duration::from_secs(10))
            .await
            .is_err());
        assert_eq!(limiter.lock().buckets.len(), MAX_BUCKETS);
        let blocked = RouteKey::new("GET", "/channels/1/messages");
        let mut next = Box::pin(limiter.acquire(&blocked, Duration::from_secs(10)));
        pending(next.as_mut()).await;
        drop(next);
        tokio::time::advance(Duration::from_secs(61)).await;
        assert!(limiter
            .acquire(&other, Duration::from_secs(10))
            .await
            .is_ok());
        assert!(limiter.lock().buckets.len() < MAX_BUCKETS);
    }

    #[tokio::test(start_paused = true)]
    async fn shared_bucket_route_mappings_are_bounded() {
        let limiter = RateLimiter::default();
        for index in 0..MAX_BUCKETS {
            let route = RouteKey {
                template: format!("GET /synthetic-{index}"),
                major: "1".into(),
            };
            limiter
                .acquire(&route, Duration::from_secs(10))
                .await
                .expect("guard")
                .update(&headers("shared", "999999", "60"))
                .await;
        }
        let route = RouteKey::new("GET", "/new-route");
        assert!(limiter
            .acquire(&route, Duration::from_secs(10))
            .await
            .is_err());
        assert_eq!(limiter.lock().route_bucket.len(), MAX_BUCKETS);
        tokio::time::advance(Duration::from_secs(61)).await;
        assert!(limiter
            .acquire(&route, Duration::from_secs(10))
            .await
            .is_ok());
        assert!(limiter.lock().route_bucket.is_empty());
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
