use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use reqwest::header::HeaderMap;

const MAX_BUCKETS: usize = 4096;

#[derive(Debug, Clone, Copy)]
struct BucketState {
    remaining: u64,
    limit: u64,
    window: Duration,
    reset_at: Instant,
}

#[derive(Debug, Default)]
struct Inner {
    route_bucket: HashMap<String, String>,
    buckets: HashMap<String, BucketState>,
    global_until: Option<Instant>,
    pending: HashMap<RouteKey, u64>,
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

pub(crate) struct Reservation<'a> {
    limiter: &'a RateLimiter,
    route: RouteKey,
}

impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        let mut inner = self.limiter.lock();
        if let Some(pending) = inner.pending.get_mut(&self.route) {
            *pending = pending.saturating_sub(1);
            if *pending == 0 {
                inner.pending.remove(&self.route);
            }
        }
    }
}

impl Inner {
    fn outstanding(&self, hash: &str, major: &str) -> u64 {
        self.pending
            .iter()
            .filter(|(route, _)| {
                route.major == major
                    && self
                        .route_bucket
                        .get(&route.template)
                        .is_some_and(|h| h == hash)
            })
            .map(|(_, count)| *count)
            .sum()
    }
}

impl RateLimiter {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub(crate) fn acquire(&self, route: &RouteKey) -> Result<Reservation<'_>, Duration> {
        let now = Instant::now();
        let mut inner = self.lock();
        let mut wait = Duration::ZERO;
        if let Some(until) = inner.global_until {
            if until > now {
                wait = until - now;
            } else {
                inner.global_until = None;
            }
        }
        if let Some(hash) = inner.route_bucket.get(&route.template).cloned() {
            let key = format!("{hash}:{}", route.major);
            let outstanding = inner.outstanding(&hash, &route.major);
            if let Some(state) = inner.buckets.get_mut(&key) {
                if state.reset_at <= now {
                    state.remaining = state.limit.saturating_sub(outstanding);
                    state.reset_at = now.checked_add(state.window).unwrap_or(now);
                }
                if state.remaining == 0 {
                    wait = wait.max(state.reset_at.saturating_duration_since(now));
                } else if wait.is_zero() {
                    state.remaining -= 1;
                }
            }
        }
        if !wait.is_zero() {
            return Err(wait);
        }
        *inner.pending.entry(route.clone()).or_default() += 1;
        Ok(Reservation {
            limiter: self,
            route: route.clone(),
        })
    }

    pub(crate) fn update(&self, route: &RouteKey, headers: &HeaderMap) {
        let header = |name: &str| {
            headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::trim)
                .filter(|v| !v.is_empty())
        };
        let Some(hash) = header("x-ratelimit-bucket") else {
            return;
        };
        let remaining = header("x-ratelimit-remaining").and_then(|v| v.parse::<u64>().ok());
        let limit = header("x-ratelimit-limit")
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|limit| *limit > 0);
        let window = header("x-ratelimit-reset-after")
            .and_then(|v| v.parse::<f64>().ok())
            .and_then(|v| Duration::try_from_secs_f64(v).ok())
            .map(|window| window.max(Duration::from_millis(1)));
        let now = Instant::now();
        let mut inner = self.lock();
        if inner.route_bucket.len() >= MAX_BUCKETS
            && !inner.route_bucket.contains_key(&route.template)
        {
            inner.route_bucket.clear();
        }
        inner
            .route_bucket
            .insert(route.template.clone(), hash.to_string());
        let (Some(remaining), Some(window)) = (remaining, window) else {
            return;
        };
        let Some(reset_at) = now.checked_add(window) else {
            return;
        };
        let own_reservation = u64::from(inner.pending.get(route).is_some_and(|count| *count > 0));
        let outstanding = inner
            .outstanding(hash, &route.major)
            .saturating_sub(own_reservation);
        let available = remaining.saturating_sub(outstanding);
        let key = format!("{hash}:{}", route.major);
        if let Some(state) = inner.buckets.get_mut(&key) {
            state.remaining = if state.reset_at > now {
                state.remaining.min(available)
            } else {
                available
            };
            state.limit = limit.unwrap_or(state.limit);
            state.window = state.window.max(window);
            state.reset_at = state.reset_at.max(reset_at);
        } else {
            if inner.buckets.len() >= MAX_BUCKETS {
                inner.buckets.retain(|_, state| state.reset_at > now);
            }
            inner.buckets.insert(
                key,
                BucketState {
                    remaining: available,
                    limit: limit.unwrap_or_else(|| remaining.saturating_add(1)),
                    window,
                    reset_at,
                },
            );
        }
    }

    pub(crate) fn block(&self, route: &RouteKey, retry_after: Duration, global: bool) {
        let window = retry_after.max(Duration::from_millis(1));
        let Some(until) = Instant::now().checked_add(window) else {
            return;
        };
        let mut inner = self.lock();
        if global {
            inner.global_until = Some(inner.global_until.map_or(until, |u| u.max(until)));
            return;
        }
        if let Some(hash) = inner.route_bucket.get(&route.template).cloned() {
            let key = format!("{hash}:{}", route.major);
            if let Some(state) = inner.buckets.get_mut(&key) {
                state.remaining = 0;
                state.reset_at = state.reset_at.max(until);
            } else {
                inner.buckets.insert(
                    key,
                    BucketState {
                        remaining: 0,
                        limit: 1,
                        window,
                        reset_at: until,
                    },
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::HeaderValue;

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

    #[test]
    fn erschoepfter_bucket_erzwingt_wartezeit() {
        let limiter = RateLimiter::default();
        let route = RouteKey::new("POST", "/channels/1/messages");
        let mut headers = HeaderMap::new();
        headers.insert("x-ratelimit-bucket", HeaderValue::from_static("abc"));
        headers.insert("x-ratelimit-remaining", HeaderValue::from_static("0"));
        headers.insert("x-ratelimit-reset-after", HeaderValue::from_static("5"));
        limiter.update(&route, &headers);
        assert!(limiter.acquire(&route).err().expect("wait") > Duration::from_secs(4));
        let other = RouteKey::new("POST", "/channels/2/messages");
        assert!(limiter.acquire(&other).is_ok());
    }

    #[test]
    fn oversized_reset_header_is_ignored_without_panicking() {
        let limiter = RateLimiter::default();
        let route = RouteKey::new("GET", "/guilds/42/members");
        let mut headers = HeaderMap::new();
        headers.insert("x-ratelimit-bucket", HeaderValue::from_static("synthetic"));
        headers.insert("x-ratelimit-remaining", HeaderValue::from_static("0"));
        headers.insert("x-ratelimit-reset-after", HeaderValue::from_static("1e100"));
        limiter.update(&route, &headers);
        assert!(limiter.acquire(&route).is_ok());
    }

    #[test]
    fn globales_limit_gilt_fuer_alle_routen() {
        let limiter = RateLimiter::default();
        let route = RouteKey::new("GET", "/users/1");
        limiter.block(&route, Duration::from_secs(3), true);
        let other = RouteKey::new("POST", "/channels/2/messages");
        assert!(limiter.acquire(&other).err().expect("wait") > Duration::from_secs(2));
    }
    #[test]
    fn active_blocks_survive_success_and_shorter_blocks() {
        let limiter = RateLimiter::default();
        let route = RouteKey::new("POST", "/channels/1/messages");
        let mut headers = HeaderMap::new();
        headers.insert("x-ratelimit-bucket", HeaderValue::from_static("abc"));
        headers.insert("x-ratelimit-remaining", HeaderValue::from_static("5"));
        headers.insert("x-ratelimit-reset-after", HeaderValue::from_static("1"));
        limiter.update(&route, &headers);
        limiter.block(&route, Duration::from_secs(5), false);
        limiter.update(&route, &headers);
        limiter.block(&route, Duration::from_secs(1), false);
        assert!(limiter.acquire(&route).err().expect("wait") > Duration::from_secs(4));
    }
    fn quota_headers(remaining: &'static str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("x-ratelimit-bucket", HeaderValue::from_static("quota"));
        headers.insert("x-ratelimit-limit", HeaderValue::from_static("3"));
        headers.insert("x-ratelimit-remaining", HeaderValue::from_static(remaining));
        headers.insert("x-ratelimit-reset-after", HeaderValue::from_static("60"));
        headers
    }

    #[test]
    fn response_counts_preserve_outstanding_reservations() {
        let limiter = RateLimiter::default();
        let route = RouteKey::new("POST", "/channels/1/messages");
        limiter.update(&route, &quota_headers("3"));
        let first = limiter.acquire(&route).expect("first");
        let second = limiter.acquire(&route).expect("second");
        limiter.update(&route, &quota_headers("2"));
        drop(first);
        let third = limiter.acquire(&route).expect("third");
        assert!(limiter.acquire(&route).is_err());
        limiter.update(&route, &quota_headers("1"));
        drop(second);
        assert!(limiter.acquire(&route).is_err());
        drop(third);
    }

    #[test]
    fn reset_admission_reserves_capacity_and_preserves_pending_requests() {
        let limiter = RateLimiter::default();
        let route = RouteKey::new("POST", "/channels/1/messages");
        limiter.update(&route, &quota_headers("1"));
        let pending = limiter.acquire(&route).expect("pending");
        limiter
            .lock()
            .buckets
            .get_mut("quota:1")
            .expect("bucket")
            .reset_at = Instant::now() - Duration::from_secs(1);
        let first = limiter.acquire(&route).expect("first after reset");
        let second = limiter.acquire(&route).expect("second after reset");
        assert!(limiter.acquire(&route).is_err());
        drop((pending, first, second));
        assert!(limiter.lock().pending.is_empty());
    }

    #[test]
    fn newly_discovered_shared_bucket_accounts_for_pending_requests() {
        let limiter = RateLimiter::default();
        let first_route = RouteKey::new("GET", "/channels/1/messages/5");
        let second_route = RouteKey::new("DELETE", "/channels/1/messages/5");
        let first = limiter.acquire(&first_route).expect("first");
        let second = limiter.acquire(&second_route).expect("second");
        limiter.update(&second_route, &quota_headers("2"));
        limiter.update(&first_route, &quota_headers("2"));
        drop(first);
        let third = limiter.acquire(&first_route).expect("third");
        assert!(limiter.acquire(&second_route).is_err());
        drop((second, third));
    }
}
