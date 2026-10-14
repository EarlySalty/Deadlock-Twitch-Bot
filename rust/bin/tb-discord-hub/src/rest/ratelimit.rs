use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use reqwest::header::HeaderMap;

const MAX_BUCKETS: usize = 4096;

#[derive(Debug, Clone, Copy)]
struct BucketState {
    remaining: u64,
    reset_at: Instant,
}

#[derive(Debug, Default)]
struct Inner {
    route_bucket: HashMap<String, String>,
    buckets: HashMap<String, BucketState>,
    global_until: Option<Instant>,
}

#[derive(Debug, Default)]
pub(crate) struct RateLimiter {
    inner: Mutex<Inner>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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

impl RateLimiter {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub(crate) fn acquire(&self, route: &RouteKey) -> Duration {
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
        let Some(hash) = inner.route_bucket.get(&route.template).cloned() else {
            return wait;
        };
        let key = format!("{hash}:{}", route.major);
        if let Some(state) = inner.buckets.get_mut(&key) {
            if state.reset_at <= now {
                return wait;
            }
            if state.remaining == 0 {
                wait = wait.max(state.reset_at - now);
            } else if wait.is_zero() {
                state.remaining -= 1;
            }
        }
        wait
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
        let reset_after = header("x-ratelimit-reset-after")
            .and_then(|v| v.parse::<f64>().ok())
            .and_then(|v| Duration::try_from_secs_f64(v).ok())
            .and_then(|duration| Instant::now().checked_add(duration));
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
        if let (Some(remaining), Some(reset_after)) = (remaining, reset_after) {
            if inner.buckets.len() >= MAX_BUCKETS {
                inner.buckets.retain(|_, s| s.reset_at > now);
                if inner.buckets.len() >= MAX_BUCKETS {
                    inner.buckets.clear();
                }
            }
            let key = format!("{hash}:{}", route.major);
            if let Some(state) = inner.buckets.get(&key) {
                if state.remaining == 0 && state.reset_at > now {
                    return;
                }
            }
            inner.buckets.insert(
                key,
                BucketState {
                    remaining,
                    reset_at: reset_after,
                },
            );
        }
    }

    pub(crate) fn block(&self, route: &RouteKey, retry_after: Duration, global: bool) {
        let Some(until) = Instant::now().checked_add(retry_after) else {
            return;
        };
        let mut inner = self.lock();
        if global {
            inner.global_until = Some(inner.global_until.map_or(until, |u| u.max(until)));
            return;
        }
        if let Some(hash) = inner.route_bucket.get(&route.template).cloned() {
            let key = format!("{hash}:{}", route.major);
            let until = inner
                .buckets
                .get(&key)
                .filter(|state| state.remaining == 0)
                .map_or(until, |state| state.reset_at.max(until));
            inner.buckets.insert(
                key,
                BucketState {
                    remaining: 0,
                    reset_at: until,
                },
            );
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
        assert!(limiter.acquire(&route) > Duration::from_secs(4));
        let other = RouteKey::new("POST", "/channels/2/messages");
        assert_eq!(limiter.acquire(&other), Duration::ZERO);
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
        assert_eq!(limiter.acquire(&route), Duration::ZERO);
    }

    #[test]
    fn globales_limit_gilt_fuer_alle_routen() {
        let limiter = RateLimiter::default();
        let route = RouteKey::new("GET", "/users/1");
        limiter.block(&route, Duration::from_secs(3), true);
        let other = RouteKey::new("POST", "/channels/2/messages");
        assert!(limiter.acquire(&other) > Duration::from_secs(2));
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
        assert!(limiter.acquire(&route) > Duration::from_secs(4));
    }
}
