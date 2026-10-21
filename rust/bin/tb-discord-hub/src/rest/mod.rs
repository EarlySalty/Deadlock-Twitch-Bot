mod actions;
mod http;
mod idempotency;
mod payload;
mod ratelimit;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use axum::body::Bytes;
use axum::http::Method;
use serde_json::{json, Map, Value};

use crate::{DirectDiscord, HubError, InboundRequest, OutboundResponse, INTERNAL_PREFIX};

pub use http::DISCORD_API_BASE;
use http::{DiscordHttp, RestError};
use idempotency::{Decision, IdempotencyConfig, IdempotencyStore, CONFLICT_MESSAGE};

tokio::task_local! {
    static WRITE_UNCERTAIN: Arc<AtomicBool>;
}

struct ExecutionGuard<'a> {
    store: &'a IdempotencyStore,
    key: &'a str,
    hash: &'a str,
    uncertain: Arc<AtomicBool>,
    cancellation_body: Value,
    settled: bool,
}

impl Drop for ExecutionGuard<'_> {
    fn drop(&mut self) {
        if !self.settled {
            self.store.settle(
                self.key,
                self.hash,
                502,
                &self.cancellation_body,
                self.uncertain.load(Ordering::Relaxed),
            );
        }
    }
}

const CHANNEL_CACHE_TTL: Duration = Duration::from_secs(60);
const CHANNEL_CACHE_MAX: usize = 2048;

type RoleLocks = HashMap<(u64, String), Weak<tokio::sync::Mutex<()>>>;

pub struct DiscordRest {
    http: DiscordHttp,
    store: IdempotencyStore,
    channel_cache: Mutex<HashMap<u64, (Option<u64>, Instant)>>,
    role_locks: Mutex<RoleLocks>,
    dm_optout_pool: Option<sqlx::PgPool>,
}

impl std::fmt::Debug for DiscordRest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DiscordRest")
            .field("http", &self.http)
            .finish_non_exhaustive()
    }
}

impl DiscordRest {
    pub fn new(bot_token: &str) -> Result<Self, HubError> {
        Self::with_base_url(bot_token, DISCORD_API_BASE)
    }

    pub fn with_base_url(bot_token: &str, base_url: &str) -> Result<Self, HubError> {
        Ok(Self {
            http: DiscordHttp::new(bot_token, base_url)?,
            store: IdempotencyStore::new(IdempotencyConfig::default()),
            channel_cache: Mutex::new(HashMap::new()),
            role_locks: Mutex::new(HashMap::new()),
            dm_optout_pool: None,
        })
    }

    pub fn with_dm_optout_pool(mut self, pool: Option<sqlx::PgPool>) -> Self {
        self.dm_optout_pool = pool;
        self
    }
}

#[async_trait::async_trait]
impl DirectDiscord for DiscordRest {
    async fn handle(
        &self,
        guild_id: u64,
        req: InboundRequest,
    ) -> Result<OutboundResponse, HubError> {
        if guild_id == 0 {
            return Err(HubError::BadRequest("guild_id fehlt".to_string()));
        }
        let Some(sub) = req.path.strip_prefix(INTERNAL_PREFIX) else {
            return Err(HubError::NotFound(req.path.clone()));
        };
        let sub = sub.trim_end_matches('/').to_string();
        let ctx = Ctx {
            guild_id,
            rid: new_request_id(),
        };
        let expected = match sub.as_str() {
            "discord/send-rich-message"
            | "discord/edit-rich-message"
            | "discord/delete-message"
            | "discord/send-dm"
            | "discord/resolve-user"
            | "discord/member/add-role"
            | "discord/member/remove-role"
            | "discord/role/create"
            | "discord/create-invite" => Method::POST,
            "discord/roles"
            | "discord/role-members"
            | "discord/members"
            | "discord/message-reactions" => Method::GET,
            other => {
                tracing::info!(
                    guild_id,
                    pfad = other,
                    "Endpunkt für fremde Guild nicht unterstützt"
                );
                return Err(HubError::Unsupported(other.to_string()));
            }
        };
        if req.method != expected {
            return Ok(OutboundResponse {
                status: 405,
                content_type: None,
                body: Bytes::new(),
            });
        }

        let response = match sub.as_str() {
            "discord/send-rich-message" => self.send_rich_message(&ctx, &req).await,
            "discord/edit-rich-message" => self.edit_rich_message(&ctx, &req).await,
            "discord/delete-message" => self.delete_message(&ctx, &req).await,
            "discord/send-dm" => self.send_dm(&ctx, &req).await,
            "discord/resolve-user" => self.resolve_user(&ctx, &req).await,
            "discord/member/add-role" => self.role_action(&ctx, &req, true).await,
            "discord/member/remove-role" => self.role_action(&ctx, &req, false).await,
            "discord/role/create" => self.create_role(&ctx, &req).await,
            "discord/create-invite" => self.create_invite(&ctx, &req).await,
            "discord/roles" => self.list_roles(&ctx, &req).await,
            "discord/role-members" => self.role_members(&ctx, &req).await,
            "discord/members" => self.members(&ctx).await,
            "discord/message-reactions" => self.message_reactions(&ctx, &req).await,
            _ => return Err(HubError::Unsupported(sub)),
        };
        Ok(response)
    }
}

pub(crate) struct Ctx {
    pub guild_id: u64,
    pub rid: String,
}

pub(crate) enum ChannelCheck {
    Ok,
    NotFound,
    Foreign,
    Failed(RestError),
}

fn new_request_id() -> String {
    use sha2::Digest;
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let digest = sha2::Sha256::digest(format!("{nanos}:{n}:{}", std::process::id()).as_bytes());
    hex::encode(&digest[..8])
}

pub(crate) fn respond(status: u16, body: Value) -> OutboundResponse {
    OutboundResponse::json(status, &body)
}

pub(crate) fn error_body(rid: &str, idem: Option<&str>, code: &str, message: &str) -> Value {
    json!({
        "ok": false,
        "request_id": rid,
        "idempotency_key": idem,
        "cached": false,
        "result": null,
        "error": {"code": code, "message": message},
    })
}

pub(crate) fn success_body(rid: &str, idem: Option<&str>, result: Value) -> Value {
    json!({
        "ok": true,
        "request_id": rid,
        "idempotency_key": idem,
        "cached": false,
        "result": result,
        "error": null,
    })
}

pub(crate) fn bad_request(rid: &str, message: &str) -> OutboundResponse {
    respond(400, error_body(rid, None, "bad_request", message))
}

pub(crate) fn forbidden(
    rid: &str,
    idem: Option<&str>,
    scope: &str,
    value: u64,
) -> OutboundResponse {
    respond(
        403,
        error_body(
            rid,
            idem,
            "forbidden",
            &format!("{scope}_id {value} is not permitted"),
        ),
    )
}

pub(crate) fn json_object(body: &[u8]) -> Option<Map<String, Value>> {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| v.as_object().cloned())
}

pub(crate) fn query_params(query: Option<&str>) -> HashMap<String, String> {
    let Some(query) = query.filter(|q| !q.is_empty()) else {
        return HashMap::new();
    };
    reqwest::Url::parse(&format!("http://hub.invalid/?{query}"))
        .map(|url| url.query_pairs().into_owned().collect())
        .unwrap_or_default()
}

pub(crate) fn snowflake(value: &Value, key: &str) -> Option<u64> {
    match value.get(key)? {
        Value::String(s) => s.parse::<u64>().ok(),
        Value::Number(n) => n.as_u64(),
        _ => None,
    }
}

impl DiscordRest {
    pub(crate) fn role_lock(&self, guild_id: u64, name: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self.role_locks.lock().unwrap_or_else(|p| p.into_inner());
        locks.retain(|_, lock| lock.strong_count() > 0);
        let key = (guild_id, name.to_string());
        if let Some(lock) = locks.get(&key).and_then(Weak::upgrade) {
            return lock;
        }
        let lock = Arc::new(tokio::sync::Mutex::new(()));
        locks.insert(key, Arc::downgrade(&lock));
        lock
    }

    pub(crate) async fn check_channel(&self, guild_id: u64, channel_id: u64) -> ChannelCheck {
        let cached = {
            let cache = self.channel_cache.lock().unwrap_or_else(|p| p.into_inner());
            cache
                .get(&channel_id)
                .filter(|(_, at)| at.elapsed() <= CHANNEL_CACHE_TTL)
                .map(|(g, _)| *g)
        };
        let channel_guild = match cached {
            Some(g) => g,
            None => {
                match self
                    .http
                    .request(Method::GET, &format!("/channels/{channel_id}"), None, None)
                    .await
                {
                    Ok(channel) => {
                        if snowflake(&channel, "id") != Some(channel_id)
                            || channel.get("guild_id").is_some_and(|value| {
                                !value.is_null()
                                    && !snowflake(&channel, "guild_id").is_some_and(|id| id > 0)
                            })
                        {
                            return ChannelCheck::Failed(RestError::InvalidResponse);
                        }
                        let g = snowflake(&channel, "guild_id");
                        let mut cache =
                            self.channel_cache.lock().unwrap_or_else(|p| p.into_inner());
                        if cache.len() >= CHANNEL_CACHE_MAX {
                            cache.retain(|_, (_, at)| at.elapsed() <= CHANNEL_CACHE_TTL);
                            if cache.len() >= CHANNEL_CACHE_MAX {
                                cache.clear();
                            }
                        }
                        cache.insert(channel_id, (g, Instant::now()));
                        g
                    }
                    Err(err) if err.is_status(404) || err.code() == Some(10003) => {
                        return ChannelCheck::NotFound
                    }
                    Err(err) if err.is_status(403) => return ChannelCheck::Foreign,
                    Err(err) => return ChannelCheck::Failed(err),
                }
            }
        };
        if channel_guild == Some(guild_id) {
            ChannelCheck::Ok
        } else {
            tracing::warn!(
                guild_id,
                channel_id,
                kanal_guild = ?channel_guild,
                "Kanal gehört nicht zur anfragenden Guild – abgelehnt"
            );
            ChannelCheck::Foreign
        }
    }

    pub(crate) async fn run_idempotent<Fut>(
        &self,
        ctx: &Ctx,
        action: &str,
        idem: &str,
        hash: &str,
        operation: Fut,
    ) -> OutboundResponse
    where
        Fut: std::future::Future<Output = (u16, Value)>,
    {
        let cache_key = format!("{}:{action}:{idem}", ctx.guild_id);
        let decision = match self.store.begin(&cache_key, hash).await {
            Ok(d) => d,
            Err(()) => {
                return respond(
                    409,
                    error_body(
                        &ctx.rid,
                        Some(idem),
                        "idempotency_conflict",
                        CONFLICT_MESSAGE,
                    ),
                )
            }
        };
        let mark_cached = |mut body: Value| {
            if let Some(obj) = body.as_object_mut() {
                obj.insert("cached".to_string(), json!(true));
            }
            body
        };
        match decision {
            Decision::Cached(status, body) => respond(status, mark_cached(body)),
            Decision::Pending(mut rx) => {
                let waited = tokio::time::timeout(self.store.waiter_timeout(), async {
                    loop {
                        if let Some(result) = rx.borrow().clone() {
                            return Some(result);
                        }
                        if rx.changed().await.is_err() {
                            return None;
                        }
                    }
                })
                .await;
                match waited {
                    Ok(Some((status, body))) => respond(status, mark_cached(body)),
                    Ok(None) => respond(
                        500,
                        error_body(
                            &ctx.rid,
                            Some(idem),
                            "internal_error",
                            "failed to await idempotent result",
                        ),
                    ),
                    Err(_) => respond(
                        504,
                        error_body(
                            &ctx.rid,
                            Some(idem),
                            "idempotency_wait_timeout",
                            "idempotent operation is still pending; retry later",
                        ),
                    ),
                }
            }
            Decision::Execute => {
                let uncertain = Arc::new(AtomicBool::new(false));
                let mut guard = ExecutionGuard {
                    store: &self.store,
                    key: &cache_key,
                    hash,
                    uncertain: uncertain.clone(),
                    cancellation_body: error_body(
                        &ctx.rid,
                        Some(idem),
                        "operation_cancelled",
                        "operation cancelled; write outcome may be unknown",
                    ),
                    settled: false,
                };
                let (status, body) = WRITE_UNCERTAIN.scope(uncertain.clone(), operation).await;
                self.store.settle(
                    &cache_key,
                    hash,
                    status,
                    &body,
                    status < 400 || uncertain.load(Ordering::Relaxed),
                );
                guard.settled = true;
                respond(status, body)
            }
        }
    }
}
