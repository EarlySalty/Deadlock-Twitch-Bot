use reqwest::Method;
use serde_json::{json, Map, Value};

use super::http::RestError;
use super::idempotency::payload_hash;
use super::payload::{self, ViewSpec};
use super::{
    bad_request, error_body, forbidden, json_object, query_params, respond, snowflake,
    success_body, ChannelCheck, Ctx, DiscordRest,
};
use crate::{InboundRequest, OutboundResponse};

const FLAG_COMPONENTS_V2: u64 = 1 << 15;
const MAX_MEMBER_PAGES: usize = 200;

struct Rich {
    channel_id: u64,
    content: Option<String>,
    embed: Value,
    components: Option<Value>,
    allowed_user_ids: Vec<u64>,
    allowed_role_ids: Vec<u64>,
    view_spec: Option<ViewSpec>,
}

fn begin_action(
    ctx: &Ctx,
    req: &InboundRequest,
) -> Result<(Map<String, Value>, String), OutboundResponse> {
    let Some(payload) = json_object(&req.body) else {
        return Err(bad_request(&ctx.rid, "invalid JSON payload"));
    };
    let idem = payload::idempotency_key(req.idempotency_key.as_deref(), &payload)
        .map_err(|msg| bad_request(&ctx.rid, &msg))?;
    Ok((payload, idem))
}

fn parse_rich(payload: &Map<String, Value>) -> Result<Rich, String> {
    let channel_id = payload::positive_int(payload, "channel_id")?;
    let content = payload::optional_content(payload, "content")?;
    let embed = payload::embed_dict(payload)?;
    let components = payload::components(payload)?
        .filter(|value| value.as_array().is_some_and(|rows| !rows.is_empty()));
    let allowed_user_ids = payload::id_list(payload, "allowed_user_ids")?;
    let allowed_role_ids = payload::id_list(payload, "allowed_role_ids")?;
    let view_spec = payload::view_spec(payload)?;
    if components.is_some() && (content.is_some() || !embed.is_empty()) {
        return Err("components V2 cannot be combined with content or embed".to_string());
    }
    if content.is_none() && embed.is_empty() && components.is_none() {
        return Err("content or embed is required".to_string());
    }
    Ok(Rich {
        channel_id,
        content,
        embed: Value::Object(embed),
        components,
        allowed_user_ids,
        allowed_role_ids,
        view_spec,
    })
}

fn rich_op(rich: &Rich) -> Map<String, Value> {
    let mut op = Map::new();
    op.insert("channel_id".into(), json!(rich.channel_id));
    op.insert("content".into(), json!(rich.content));
    op.insert("embed".into(), rich.embed.clone());
    if let Some(components) = &rich.components {
        op.insert("components".into(), components.clone());
    }
    op.insert("allowed_user_ids".into(), json!(rich.allowed_user_ids));
    op.insert("allowed_role_ids".into(), json!(rich.allowed_role_ids));
    op.insert("view_spec".into(), json!(format!("{:?}", rich.view_spec)));
    op
}

fn view_components(spec: &ViewSpec) -> Option<Value> {
    let link = |label: &str, url: &str| {
        json!([{
            "type": 1,
            "components": [{"type": 2, "style": 5, "label": label, "url": url}],
        }])
    };
    match spec {
        ViewSpec::LinkButton { label, url } => Some(link(label, url)),
        ViewSpec::TwitchLiveTracking {
            referral_url,
            button_label,
            ..
        } => {
            let url = referral_url.trim();
            let valid = (url.starts_with("https://") || url.starts_with("http://"))
                && url
                    .split_once("://")
                    .is_some_and(|(_, rest)| !rest.is_empty() && !rest.starts_with('/'));
            if !valid {
                return None;
            }
            let label = button_label.trim();
            let label: String = if label.is_empty() {
                "Auf Twitch ansehen".to_string()
            } else {
                label.chars().take(80).collect()
            };
            Some(link(&label, url))
        }
        ViewSpec::ScamRevoke { .. } => None,
    }
}

fn allowed_mentions(guild_id: u64, user_ids: &[u64], role_ids: &[u64]) -> Value {
    json!({
        "parse": [],
        "users": user_ids.iter().map(u64::to_string).collect::<Vec<_>>(),
        "roles": role_ids
            .iter()
            .filter(|id| **id != guild_id)
            .map(|v| v.to_string())
            .collect::<Vec<_>>(),
        "replied_user": false,
    })
}

fn rich_body(guild_id: u64, rich: &Rich, editing: bool) -> Value {
    let mut body = Map::new();
    let view = rich.view_spec.as_ref().and_then(view_components);
    if let Some(components) = &rich.components {
        let mut rows = components.as_array().cloned().unwrap_or_default();
        if let Some(Value::Array(extra)) = view {
            rows.extend(extra);
        }
        if editing {
            body.insert("content".into(), Value::Null);
            body.insert("embeds".into(), json!([]));
        }
        body.insert("flags".into(), json!(FLAG_COMPONENTS_V2));
        body.insert("components".into(), Value::Array(rows));
        body.insert(
            "allowed_mentions".into(),
            allowed_mentions(guild_id, &rich.allowed_user_ids, &rich.allowed_role_ids),
        );
        return Value::Object(body);
    }
    body.insert("content".into(), json!(rich.content));
    if rich.embed.as_object().is_some_and(|e| !e.is_empty()) {
        body.insert("embeds".into(), json!([rich.embed]));
    } else if editing {
        body.insert("embeds".into(), json!([]));
    }
    body.insert(
        "allowed_mentions".into(),
        allowed_mentions(guild_id, &rich.allowed_user_ids, &rich.allowed_role_ids),
    );
    if let Some(view) = view {
        body.insert("components".into(), view);
    } else if editing {
        body.insert("components".into(), json!([]));
    }
    Value::Object(body)
}

fn reason_or(payload: &Map<String, Value>, default: &str) -> String {
    payload
        .get("reason")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(default)
        .to_string()
}

fn is_not_found(err: &RestError) -> bool {
    err.is_status(404)
}

fn member_display_name(member: &Value) -> String {
    let user = member.get("user").cloned().unwrap_or(Value::Null);
    let name = [
        member.get("nick"),
        user.get("global_name"),
        user.get("username"),
    ]
    .into_iter()
    .flatten()
    .filter_map(Value::as_str)
    .map(str::trim)
    .find(|s| !s.is_empty())
    .unwrap_or_default()
    .to_string();
    name
}

impl DiscordRest {
    async fn require_channel(
        &self,
        ctx: &Ctx,
        idem: &str,
        channel_id: u64,
        not_found: impl FnOnce() -> OutboundResponse,
        failed: &str,
    ) -> Result<(), OutboundResponse> {
        match self.check_channel(ctx.guild_id, channel_id).await {
            ChannelCheck::Ok => Ok(()),
            ChannelCheck::Foreign => Err(forbidden(&ctx.rid, Some(idem), "channel", channel_id)),
            ChannelCheck::NotFound => Err(not_found()),
            ChannelCheck::Failed(err) => {
                tracing::error!(%err, channel_id, "Kanalprüfung fehlgeschlagen");
                Err(respond(
                    502,
                    error_body(&ctx.rid, Some(idem), "discord_error", failed),
                ))
            }
        }
    }

    fn require_guild(ctx: &Ctx, idem: Option<&str>, guild_id: u64) -> Result<(), OutboundResponse> {
        if guild_id == ctx.guild_id {
            Ok(())
        } else {
            tracing::warn!(
                header_guild = ctx.guild_id,
                payload_guild = guild_id,
                "guild_id im Payload passt nicht zur Header-Guild – abgelehnt"
            );
            Err(forbidden(&ctx.rid, idem, "guild", guild_id))
        }
    }

    pub(super) async fn send_rich_message(
        &self,
        ctx: &Ctx,
        req: &InboundRequest,
    ) -> OutboundResponse {
        let (payload, idem) = match begin_action(ctx, req) {
            Ok(v) => v,
            Err(resp) => return resp,
        };
        let rich = match parse_rich(&payload) {
            Ok(v) => v,
            Err(msg) => return bad_request(&ctx.rid, &msg),
        };
        let hash = payload_hash(&rich_op(&rich));
        let rid = ctx.rid.clone();
        self.run_idempotent(ctx, "discord.send_rich_message", &idem, &hash, async {
            let not_found = || {
                (
                    404,
                    error_body(
                        &rid,
                        Some(&idem),
                        "not_found",
                        &format!("channel {} not found", rich.channel_id),
                    ),
                )
            };
            if let Err(resp) = self
                .require_channel(
                    ctx,
                    &idem,
                    rich.channel_id,
                    || respond(404, not_found().1),
                    "failed to send message",
                )
                .await
            {
                return into_pair(resp);
            }
            let body = rich_body(ctx.guild_id, &rich, false);
            match self
                .http
                .request(
                    Method::POST,
                    &format!("/channels/{}/messages", rich.channel_id),
                    Some(&body),
                    None,
                )
                .await
            {
                Ok(message) => match snowflake(&message, "id") {
                    Some(message_id) => (
                        200,
                        success_body(
                            &rid,
                            Some(&idem),
                            json!({
                                "channel_id": rich.channel_id,
                                "message_id": message_id.to_string(),
                            }),
                        ),
                    ),
                    None => (
                        502,
                        error_body(&rid, Some(&idem), "discord_error", "failed to send message"),
                    ),
                },
                Err(err) if is_not_found(&err) => not_found(),
                Err(err) => {
                    tracing::error!(%err, channel_id = rich.channel_id, "send_rich_message fehlgeschlagen");
                    (
                        502,
                        error_body(&rid, Some(&idem), "discord_error", "failed to send message"),
                    )
                }
            }
        })
        .await
    }

    pub(super) async fn edit_rich_message(
        &self,
        ctx: &Ctx,
        req: &InboundRequest,
    ) -> OutboundResponse {
        let (payload, idem) = match begin_action(ctx, req) {
            Ok(v) => v,
            Err(resp) => return resp,
        };
        let message_id = match payload::positive_int(&payload, "message_id") {
            Ok(v) => v,
            Err(msg) => return bad_request(&ctx.rid, &msg),
        };
        let rich = match parse_rich(&payload) {
            Ok(v) => v,
            Err(msg) => return bad_request(&ctx.rid, &msg),
        };
        let mut op = rich_op(&rich);
        op.insert("message_id".into(), json!(message_id));
        let hash = payload_hash(&op);
        let rid = ctx.rid.clone();
        self.run_idempotent(ctx, "discord.edit_rich_message", &idem, &hash, async {
            let channel_missing = || {
                respond(
                    404,
                    error_body(
                        &rid,
                        Some(&idem),
                        "not_found",
                        &format!("channel {} not found", rich.channel_id),
                    ),
                )
            };
            if let Err(resp) = self
                .require_channel(
                    ctx,
                    &idem,
                    rich.channel_id,
                    channel_missing,
                    "failed to edit message",
                )
                .await
            {
                return into_pair(resp);
            }
            let body = rich_body(ctx.guild_id, &rich, true);
            match self
                .http
                .request(
                    Method::PATCH,
                    &format!("/channels/{}/messages/{message_id}", rich.channel_id),
                    Some(&body),
                    None,
                )
                .await
            {
                Ok(_) => (
                    200,
                    success_body(
                        &rid,
                        Some(&idem),
                        json!({"channel_id": rich.channel_id, "message_id": message_id}),
                    ),
                ),
                Err(err) if is_not_found(&err) => (
                    404,
                    error_body(
                        &rid,
                        Some(&idem),
                        "not_found",
                        &format!("message {message_id} not found"),
                    ),
                ),
                Err(err) => {
                    tracing::error!(%err, message_id, "edit_rich_message fehlgeschlagen");
                    (
                        502,
                        error_body(&rid, Some(&idem), "discord_error", "failed to edit message"),
                    )
                }
            }
        })
        .await
    }

    pub(super) async fn delete_message(&self, ctx: &Ctx, req: &InboundRequest) -> OutboundResponse {
        let (payload, idem) = match begin_action(ctx, req) {
            Ok(v) => v,
            Err(resp) => return resp,
        };
        let parsed = (|| -> Result<(u64, u64, String), String> {
            let channel_id = payload::positive_int(&payload, "channel_id")?;
            let message_id = payload::positive_int(&payload, "message_id")?;
            let reason = payload
                .get("reason")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|r| !r.is_empty())
                .ok_or_else(|| "reason is required".to_string())?
                .to_string();
            Ok((channel_id, message_id, reason))
        })();
        let (channel_id, message_id, reason) = match parsed {
            Ok(v) => v,
            Err(msg) => return bad_request(&ctx.rid, &msg),
        };
        let mut op = Map::new();
        op.insert("channel_id".into(), json!(channel_id));
        op.insert("message_id".into(), json!(message_id));
        op.insert("reason".into(), json!(reason));
        let hash = payload_hash(&op);
        let rid = ctx.rid.clone();
        self.run_idempotent(ctx, "discord.delete_message", &idem, &hash, async {
            let absent = |already_absent: bool| {
                (
                    200,
                    success_body(
                        &rid,
                        Some(&idem),
                        json!({
                            "channel_id": channel_id,
                            "message_id": message_id,
                            "already_absent": already_absent,
                        }),
                    ),
                )
            };
            match self.check_channel(ctx.guild_id, channel_id).await {
                ChannelCheck::Ok => {}
                ChannelCheck::NotFound => return absent(true),
                ChannelCheck::Foreign => {
                    return into_pair(forbidden(&rid, Some(&idem), "channel", channel_id))
                }
                ChannelCheck::Failed(err) => {
                    tracing::error!(%err, channel_id, "Kanalprüfung fehlgeschlagen");
                    return (
                        502,
                        error_body(
                            &rid,
                            Some(&idem),
                            "discord_error",
                            "failed to delete message",
                        ),
                    );
                }
            }
            match self
                .http
                .request(
                    Method::DELETE,
                    &format!("/channels/{channel_id}/messages/{message_id}"),
                    None,
                    Some(&reason),
                )
                .await
            {
                Ok(_) => absent(false),
                Err(err) if is_not_found(&err) => absent(true),
                Err(err) => {
                    tracing::error!(%err, channel_id, message_id, "delete_message fehlgeschlagen");
                    (
                        502,
                        error_body(
                            &rid,
                            Some(&idem),
                            "discord_error",
                            "failed to delete message",
                        ),
                    )
                }
            }
        })
        .await
    }

    pub(super) async fn send_dm(&self, ctx: &Ctx, req: &InboundRequest) -> OutboundResponse {
        let (payload, idem) = match begin_action(ctx, req) {
            Ok(v) => v,
            Err(resp) => return resp,
        };
        let parsed = (|| -> Result<(u64, String), String> {
            Ok((
                payload::positive_int(&payload, "user_id")?,
                payload::required_content(&payload)?,
            ))
        })();
        let (user_id, content) = match parsed {
            Ok(v) => v,
            Err(msg) => return bad_request(&ctx.rid, &msg),
        };
        let mut op = Map::new();
        op.insert("user_id".into(), json!(user_id));
        op.insert("content".into(), json!(content));
        let hash = payload_hash(&op);
        let rid = ctx.rid.clone();
        let guild_id = ctx.guild_id;
        self.run_idempotent(ctx, "discord.send_dm", &idem, &hash, async {
            let fail =
                |message: &str| (502, error_body(&rid, Some(&idem), "discord_error", message));
            match self
                .http
                .request(
                    Method::GET,
                    &format!("/guilds/{guild_id}/members/{user_id}"),
                    None,
                    None,
                )
                .await
            {
                Ok(member)
                    if member.get("user").and_then(|u| snowflake(u, "id")) == Some(user_id) => {}
                Ok(_) => return fail("invalid member response"),
                Err(err) if is_not_found(&err) => {
                    return (
                        404,
                        error_body(
                            &rid,
                            Some(&idem),
                            "not_found",
                            &format!("user {user_id} not found"),
                        ),
                    )
                }
                Err(err) => {
                    tracing::error!(%err, user_id, "Mitgliedsprüfung für DM fehlgeschlagen");
                    return fail("failed to send DM");
                }
            }
            let dm = match self
                .http
                .request(
                    Method::POST,
                    "/users/@me/channels",
                    Some(&json!({"recipient_id": user_id.to_string()})),
                    None,
                )
                .await
            {
                Ok(dm) => dm,
                Err(err) => {
                    tracing::warn!(%err, user_id, "DM-Kanal nicht geöffnet");
                    return fail("failed to open DM channel");
                }
            };
            let Some(dm_channel_id) = snowflake(&dm, "id") else {
                return fail("failed to open DM channel");
            };
            let body = json!({
                "content": content,
                "allowed_mentions": {"parse": []},
            });
            match self
                .http
                .request(
                    Method::POST,
                    &format!("/channels/{dm_channel_id}/messages"),
                    Some(&body),
                    None,
                )
                .await
            {
                Ok(message) => match snowflake(&message, "id") {
                    Some(message_id) => (
                        200,
                        success_body(
                            &rid,
                            Some(&idem),
                            json!({
                                "user_id": user_id,
                                "channel_id": dm_channel_id,
                                "message_id": message_id,
                            }),
                        ),
                    ),
                    None => fail("failed to send DM"),
                },
                Err(err) => {
                    tracing::error!(%err, user_id, "send_dm fehlgeschlagen");
                    fail("failed to send DM")
                }
            }
        })
        .await
    }

    pub(super) async fn resolve_user(&self, ctx: &Ctx, req: &InboundRequest) -> OutboundResponse {
        let Some(payload) = json_object(&req.body) else {
            return bad_request(&ctx.rid, "invalid JSON payload");
        };
        let user_id = match payload::positive_int(&payload, "user_id") {
            Ok(v) => v,
            Err(msg) => return bad_request(&ctx.rid, &msg),
        };
        match self
            .http
            .request(Method::GET, &format!("/users/{user_id}"), None, None)
            .await
        {
            Ok(user) => {
                if snowflake(&user, "id") != Some(user_id)
                    || user.get("username").and_then(Value::as_str).is_none()
                {
                    return lookup_failed(ctx);
                }
                let name = user
                    .get("username")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let global_name = user
                    .get("global_name")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                let display_name = global_name.clone().unwrap_or_else(|| name.clone());
                respond(
                    200,
                    json!({
                        "ok": true,
                        "result": {
                            "found": true,
                            "user_id": user_id.to_string(),
                            "name": name,
                            "global_name": global_name,
                            "display_name": display_name,
                        }
                    }),
                )
            }
            Err(err) if is_not_found(&err) => {
                respond(200, json!({"ok": true, "result": {"found": false}}))
            }
            Err(err) => {
                tracing::warn!(%err, user_id, "resolve_user fehlgeschlagen");
                lookup_failed(ctx)
            }
        }
    }

    pub(super) async fn role_action(
        &self,
        ctx: &Ctx,
        req: &InboundRequest,
        add: bool,
    ) -> OutboundResponse {
        let (payload, idem) = match begin_action(ctx, req) {
            Ok(v) => v,
            Err(resp) => return resp,
        };
        let default_reason = if add {
            "master-broker:add-role"
        } else {
            "master-broker:remove-role"
        };
        let parsed = (|| -> Result<(u64, u64, u64), String> {
            Ok((
                payload::positive_int(&payload, "guild_id")?,
                payload::positive_int(&payload, "user_id")?,
                payload::positive_int(&payload, "role_id")?,
            ))
        })();
        let (guild_id, user_id, role_id) = match parsed {
            Ok(v) => v,
            Err(msg) => return bad_request(&ctx.rid, &msg),
        };
        let reason = reason_or(&payload, default_reason);
        if let Err(resp) = Self::require_guild(ctx, Some(&idem), guild_id) {
            return resp;
        }
        let mut op = Map::new();
        op.insert("guild_id".into(), json!(guild_id));
        op.insert("user_id".into(), json!(user_id));
        op.insert("role_id".into(), json!(role_id));
        op.insert("reason".into(), json!(reason));
        let hash = payload_hash(&op);
        let (action, fail_message, method) = if add {
            ("discord.add_role", "failed to add role", Method::PUT)
        } else {
            (
                "discord.remove_role",
                "failed to remove role",
                Method::DELETE,
            )
        };
        let rid = ctx.rid.clone();
        self.run_idempotent(ctx, action, &idem, &hash, async {
            let not_found = |what: String| {
                (
                    404,
                    error_body(&rid, Some(&idem), "not_found", &format!("{what} not found")),
                )
            };
            match self
                .http
                .request(
                    method,
                    &format!("/guilds/{guild_id}/members/{user_id}/roles/{role_id}"),
                    None,
                    Some(&reason),
                )
                .await
            {
                Ok(_) => (
                    200,
                    success_body(
                        &rid,
                        Some(&idem),
                        json!({"guild_id": guild_id, "user_id": user_id, "role_id": role_id}),
                    ),
                ),
                Err(err) if err.code() == Some(10004) => not_found(format!("guild {guild_id}")),
                Err(err) if err.code() == Some(10011) => not_found(format!("role {role_id}")),
                Err(err) if matches!(err.code(), Some(10007 | 10013)) => {
                    not_found(format!("member {user_id}"))
                }
                Err(err) => {
                    tracing::error!(%err, guild_id, user_id, role_id, "role_action fehlgeschlagen");
                    (
                        502,
                        error_body(&rid, Some(&idem), "discord_error", fail_message),
                    )
                }
            }
        })
        .await
    }

    pub(super) async fn create_role(&self, ctx: &Ctx, req: &InboundRequest) -> OutboundResponse {
        let (payload, idem) = match begin_action(ctx, req) {
            Ok(v) => v,
            Err(resp) => return resp,
        };
        let parsed = (|| -> Result<(u64, String, bool), String> {
            let guild_id = payload::positive_int(&payload, "guild_id")?;
            let name = payload
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                .to_string();
            if name.is_empty() {
                return Err("name is required".to_string());
            }
            if name.chars().count() > 100 {
                return Err("name exceeds Discord limit (100)".to_string());
            }
            let mentionable = payload
                .get("mentionable")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            Ok((guild_id, name, mentionable))
        })();
        let (guild_id, name, mentionable) = match parsed {
            Ok(v) => v,
            Err(msg) => return bad_request(&ctx.rid, &msg),
        };
        let reason = reason_or(&payload, "master-broker:create-role");
        if let Err(resp) = Self::require_guild(ctx, Some(&idem), guild_id) {
            return resp;
        }
        let mut op = Map::new();
        op.insert("guild_id".into(), json!(guild_id));
        op.insert("name".into(), json!(name));
        op.insert("mentionable".into(), json!(mentionable));
        op.insert("reason".into(), json!(reason));
        let hash = payload_hash(&op);
        let rid = ctx.rid.clone();
        self.run_idempotent(ctx, "discord.create_role", &idem, &hash, async {
            let lock = self.role_lock(guild_id, &name);
            let _guard = lock.lock().await;
            let ok = |role_id: u64| {
                (
                    200,
                    success_body(
                        &rid,
                        Some(&idem),
                        json!({"guild_id": guild_id.to_string(), "role_id": role_id, "name": name}),
                    ),
                )
            };
            let map_err = |err: RestError| {
                if err.code() == Some(10004) || is_not_found(&err) {
                    (
                        404,
                        error_body(
                            &rid,
                            Some(&idem),
                            "not_found",
                            &format!("guild {guild_id} not found"),
                        ),
                    )
                } else {
                    tracing::error!(%err, guild_id, "create_role fehlgeschlagen");
                    (
                        502,
                        error_body(&rid, Some(&idem), "discord_error", &err.to_string()),
                    )
                }
            };
            match self
                .http
                .request(
                    Method::GET,
                    &format!("/guilds/{guild_id}/roles"),
                    None,
                    None,
                )
                .await
            {
                Ok(Value::Array(roles)) => {
                    if !valid_roles(&roles) {
                        return map_err(RestError::InvalidResponse);
                    }
                    if let Some(id) = roles
                        .iter()
                        .find(|r| r.get("name").and_then(Value::as_str) == Some(name.as_str()))
                        .and_then(|r| snowflake(r, "id"))
                    {
                        return ok(id);
                    }
                }
                Ok(_) => return map_err(RestError::InvalidResponse),
                Err(err) => return map_err(err),
            }
            match self
                .http
                .request(
                    Method::POST,
                    &format!("/guilds/{guild_id}/roles"),
                    Some(&json!({"name": name, "mentionable": mentionable})),
                    Some(&reason),
                )
                .await
            {
                Ok(role) => match snowflake(&role, "id") {
                    Some(id) => ok(id),
                    None => (
                        502,
                        error_body(&rid, Some(&idem), "discord_error", "invalid role response"),
                    ),
                },
                Err(err) => map_err(err),
            }
        })
        .await
    }

    pub(super) async fn create_invite(&self, ctx: &Ctx, req: &InboundRequest) -> OutboundResponse {
        let (payload, idem) = match begin_action(ctx, req) {
            Ok(v) => v,
            Err(resp) => return resp,
        };
        let channel_id = match payload::positive_int(&payload, "channel_id") {
            Ok(v) => v,
            Err(msg) => return bad_request(&ctx.rid, &msg),
        };
        let reason: String = reason_or(&payload, "master-broker:create-invite")
            .chars()
            .take(512)
            .collect();
        let mut op = Map::new();
        op.insert("channel_id".into(), json!(channel_id));
        op.insert("reason".into(), json!(reason));
        let hash = payload_hash(&op);
        let rid = ctx.rid.clone();
        self.run_idempotent(ctx, "discord.create_invite", &idem, &hash, async {
            let not_found = || {
                respond(
                    404,
                    error_body(
                        &rid,
                        Some(&idem),
                        "not_found",
                        &format!("channel {channel_id} not found or does not support invites"),
                    ),
                )
            };
            if let Err(resp) = self
                .require_channel(ctx, &idem, channel_id, not_found, "failed to create invite")
                .await
            {
                return into_pair(resp);
            }
            match self
                .http
                .request(
                    Method::POST,
                    &format!("/channels/{channel_id}/invites"),
                    Some(&json!({"max_age": 0, "max_uses": 0, "temporary": false, "unique": true})),
                    Some(&reason),
                )
                .await
            {
                Ok(invite) => {
                    let code = invite
                        .get("code")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    if code.is_empty() {
                        return (
                            502,
                            error_body(
                                &rid,
                                Some(&idem),
                                "discord_error",
                                "failed to create invite",
                            ),
                        );
                    }
                    let guild_id = invite
                        .get("guild")
                        .and_then(|g| snowflake(g, "id"))
                        .or_else(|| snowflake(&invite, "guild_id"))
                        .unwrap_or(ctx.guild_id);
                    (
                        200,
                        success_body(
                            &rid,
                            Some(&idem),
                            json!({
                                "invite_url": format!("https://discord.gg/{code}"),
                                "code": code,
                                "channel_id": channel_id,
                                "guild_id": guild_id,
                            }),
                        ),
                    )
                }
                Err(err) if is_not_found(&err) || err.code() == Some(50024) => {
                    into_pair(not_found())
                }
                Err(err) => {
                    tracing::error!(%err, channel_id, "create_invite fehlgeschlagen");
                    (
                        502,
                        error_body(
                            &rid,
                            Some(&idem),
                            "discord_error",
                            "failed to create invite",
                        ),
                    )
                }
            }
        })
        .await
    }

    fn query_guild(
        ctx: &Ctx,
        params: &std::collections::HashMap<String, String>,
    ) -> Result<(), OutboundResponse> {
        match params
            .get("guild_id")
            .and_then(|s| s.trim().parse::<u64>().ok())
            .filter(|v| *v > 0)
        {
            Some(g) => Self::require_guild(ctx, None, g),
            None => Ok(()),
        }
    }

    pub(super) async fn list_roles(&self, ctx: &Ctx, req: &InboundRequest) -> OutboundResponse {
        let params = query_params(req.query.as_deref());
        if let Err(resp) = Self::query_guild(ctx, &params) {
            return resp;
        }
        let guild_id = ctx.guild_id;
        match self
            .http
            .request(
                Method::GET,
                &format!("/guilds/{guild_id}/roles"),
                None,
                None,
            )
            .await
        {
            Ok(Value::Array(roles)) if valid_roles(&roles) => respond(
                200,
                json!({
                    "ok": true,
                    "guild_id": guild_id.to_string(),
                    "chunked": false,
                    "roles": roles.iter().filter_map(|r| {
                        Some(json!({
                            "id": snowflake(r, "id")?.to_string(),
                            "name": r.get("name").and_then(Value::as_str).unwrap_or_default(),
                            "position": r.get("position").and_then(Value::as_i64).unwrap_or(0),
                            "member_count": 0,
                        }))
                    }).collect::<Vec<_>>(),
                }),
            ),
            Err(err) if is_not_found(&err) => respond(
                404,
                error_body(&ctx.rid, None, "not_found", "guild not found"),
            ),
            Ok(_) | Err(_) => lookup_failed(ctx),
        }
    }

    async fn all_members(&self, guild_id: u64) -> Result<Vec<Value>, RestError> {
        let mut out = Vec::new();
        let mut after = 0u64;
        for _ in 0..MAX_MEMBER_PAGES {
            let page = self
                .http
                .request(
                    Method::GET,
                    &format!("/guilds/{guild_id}/members?limit=1000&after={after}"),
                    None,
                    None,
                )
                .await?;
            let Value::Array(page) = page else {
                return Err(RestError::Transport("Ungültige Mitgliederliste".into()));
            };
            if page.iter().any(|member| !valid_member(member, after)) {
                return Err(RestError::InvalidResponse);
            }
            let cursor = page
                .iter()
                .filter_map(|m| m.get("user").and_then(|u| snowflake(u, "id")))
                .max();
            let full = page.len() >= 1000;
            out.extend(page);
            if !full {
                return Ok(out);
            }
            after = cursor
                .filter(|cursor| *cursor > after)
                .ok_or(RestError::InvalidResponse)?;
        }
        tracing::warn!(guild_id, "Mitgliederliste bei Seitenlimit abgeschnitten");
        Err(RestError::Transport(
            "Mitgliederliste nicht vollständig".into(),
        ))
    }

    pub(super) async fn members(&self, ctx: &Ctx) -> OutboundResponse {
        match self.all_members(ctx.guild_id).await {
            Ok(list) => {
                let guild = ctx.guild_id.to_string();
                let members: Vec<Value> = list
                    .iter()
                    .filter_map(|m| {
                        let user = m.get("user")?;
                        if user.get("bot").and_then(Value::as_bool).unwrap_or(false) {
                            return None;
                        }
                        Some(json!({
                            "guild_id": guild,
                            "id": snowflake(user, "id")?.to_string(),
                            "name": user.get("username").and_then(Value::as_str).unwrap_or_default(),
                            "global_name": user.get("global_name").and_then(Value::as_str),
                            "nick": m.get("nick").and_then(Value::as_str),
                        }))
                    })
                    .collect();
                respond(200, json!({"ok": true, "members": members}))
            }
            Err(err) if is_not_found(&err) => respond(
                404,
                error_body(&ctx.rid, None, "not_found", "guild not found"),
            ),
            Err(err) => {
                tracing::error!(%err, guild_id = ctx.guild_id, "members fehlgeschlagen");
                lookup_failed(ctx)
            }
        }
    }

    pub(super) async fn role_members(&self, ctx: &Ctx, req: &InboundRequest) -> OutboundResponse {
        let params = query_params(req.query.as_deref());
        if let Err(resp) = Self::query_guild(ctx, &params) {
            return resp;
        }
        let guild_id = ctx.guild_id;
        let role_id = params
            .get("role_id")
            .and_then(|s| s.trim().parse::<u64>().ok())
            .unwrap_or(0);
        let live = params.get("live").is_some_and(|v| v == "true");
        if live && role_id == 0 {
            return bad_request(&ctx.rid, "live role lookup requires guild_id and role_id");
        }
        let role_missing = || {
            respond(
                404,
                error_body(
                    &ctx.rid,
                    None,
                    "not_found",
                    &format!("role {role_id} not found"),
                ),
            )
        };
        let unavailable = || {
            respond(
                503,
                error_body(
                    &ctx.rid,
                    None,
                    "unavailable",
                    "live role members unavailable",
                ),
            )
        };
        let roles = match self
            .http
            .request(
                Method::GET,
                &format!("/guilds/{guild_id}/roles"),
                None,
                None,
            )
            .await
        {
            Ok(Value::Array(roles)) if valid_roles(&roles) => roles,
            Ok(_) | Err(_) if live => return unavailable(),
            Ok(_) => return lookup_failed(ctx),
            Err(err) if is_not_found(&err) => {
                return respond(
                    404,
                    error_body(&ctx.rid, None, "not_found", "guild not found"),
                )
            }
            Err(_) => return lookup_failed(ctx),
        };
        let Some(role) = roles.iter().find(|r| snowflake(r, "id") == Some(role_id)) else {
            return if live { unavailable() } else { role_missing() };
        };
        let members = match self.all_members(guild_id).await {
            Ok(m) => m,
            Err(err) => {
                tracing::warn!(%err, guild_id, role_id, "Rollenmitglieder nicht abrufbar");
                return if live {
                    unavailable()
                } else if is_not_found(&err) {
                    respond(
                        404,
                        error_body(&ctx.rid, None, "not_found", "guild not found"),
                    )
                } else {
                    lookup_failed(ctx)
                };
            }
        };
        let role_str = role_id.to_string();
        let members: Vec<Value> = members
            .iter()
            .filter(|m| {
                role_id == guild_id
                    || m.get("roles")
                        .and_then(Value::as_array)
                        .is_some_and(|r| r.iter().any(|v| v.as_str() == Some(role_str.as_str())))
            })
            .filter_map(|m| {
                let id = m.get("user").and_then(|u| snowflake(u, "id"))?;
                Some(json!({"id": id.to_string(), "display_name": member_display_name(m)}))
            })
            .collect();
        respond(
            200,
            json!({
                "ok": true,
                "complete": live,
                "guild_id": guild_id.to_string(),
                "role_id": role_id.to_string(),
                "name": role.get("name").and_then(Value::as_str).unwrap_or_default(),
                "members": members,
            }),
        )
    }

    pub(super) async fn message_reactions(
        &self,
        ctx: &Ctx,
        req: &InboundRequest,
    ) -> OutboundResponse {
        let params: Map<String, Value> = query_params(req.query.as_deref())
            .into_iter()
            .map(|(k, v)| (k, Value::String(v)))
            .collect();
        let channel_id = match payload::positive_int(&params, "channel_id") {
            Ok(v) => v,
            Err(msg) => return bad_request(&ctx.rid, &msg),
        };
        let message_id = match payload::positive_int(&params, "message_id") {
            Ok(v) => v,
            Err(msg) => return bad_request(&ctx.rid, &msg),
        };
        let not_found = || respond(200, json!({"found": false, "reactions": []}));
        let failed = || respond(502, json!({"error": "Discord request failed"}));
        match self.check_channel(ctx.guild_id, channel_id).await {
            ChannelCheck::Ok => {}
            ChannelCheck::NotFound => return not_found(),
            ChannelCheck::Foreign => return forbidden(&ctx.rid, None, "channel", channel_id),
            ChannelCheck::Failed(err) => {
                tracing::error!(%err, channel_id, "Kanalprüfung fehlgeschlagen");
                return failed();
            }
        }
        match self
            .http
            .request(
                Method::GET,
                &format!("/channels/{channel_id}/messages/{message_id}"),
                None,
                None,
            )
            .await
        {
            Ok(message) => {
                if snowflake(&message, "id") != Some(message_id)
                    || snowflake(&message, "channel_id") != Some(channel_id)
                {
                    return failed();
                }
                let reactions: Vec<Value> = message
                    .get("reactions")
                    .and_then(Value::as_array)
                    .map(|list| {
                        list.iter()
                            .map(|r| {
                                let emoji = r.get("emoji").cloned().unwrap_or(Value::Null);
                                let name = emoji
                                    .get("name")
                                    .and_then(Value::as_str)
                                    .unwrap_or_default();
                                let label = match snowflake(&emoji, "id") {
                                    Some(id) => format!("{name}:{id}"),
                                    None => name.to_string(),
                                };
                                json!({
                                    "emoji": label,
                                    "count": r.get("count").and_then(Value::as_u64).unwrap_or(0),
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                respond(200, json!({"found": true, "reactions": reactions}))
            }
            Err(err) if is_not_found(&err) => not_found(),
            Err(err) => {
                tracing::error!(%err, channel_id, message_id, "message_reactions fehlgeschlagen");
                failed()
            }
        }
    }
}

fn into_pair(resp: OutboundResponse) -> (u16, Value) {
    let body = serde_json::from_slice(&resp.body).unwrap_or(Value::Null);
    (resp.status, body)
}

fn lookup_failed(ctx: &Ctx) -> OutboundResponse {
    respond(
        502,
        error_body(
            &ctx.rid,
            None,
            "discord_error",
            "Discord-Abfrage fehlgeschlagen",
        ),
    )
}

fn valid_roles(roles: &[Value]) -> bool {
    roles.iter().all(|role| {
        snowflake(role, "id").is_some_and(|id| id > 0)
            && role.get("name").and_then(Value::as_str).is_some()
    })
}

fn valid_member(member: &Value, after: u64) -> bool {
    member.get("user").is_some_and(|user| {
        snowflake(user, "id").is_some_and(|id| id > after)
            && user.get("username").and_then(Value::as_str).is_some()
    }) && member
        .get("roles")
        .and_then(Value::as_array)
        .is_some_and(|roles| {
            roles.iter().all(|role| {
                role.as_str()
                    .and_then(|id| id.parse::<u64>().ok())
                    .is_some_and(|id| id > 0)
            })
        })
}
