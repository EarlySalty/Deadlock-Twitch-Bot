use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sqlx::PgPool;
use tracing::{info, warn};

use crate::api::ChatApi;
use crate::types::{ChatMessageEvent, SendOutcome};

pub const FOLLOW_HINT_PLACEHOLDER: &str = "{user}";
pub const FOLLOW_HINT_DEFAULT_MESSAGE: &str =
    "@{user} Schön, dass du zum ersten Mal im Chat bist! Wenn dir der Stream gefällt, lass doch gern einen Follow da 💜";
pub const FOLLOW_HINT_MAX_CHARS: usize = 300;

const CHANNEL_COOLDOWN: Duration = Duration::from_secs(90);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FollowHintMessageError {
    TooLong,
}

pub fn normalize_follow_hint_message(raw: &str) -> Result<Option<String>, FollowHintMessageError> {
    let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return Ok(None);
    }
    if collapsed.chars().count() > FOLLOW_HINT_MAX_CHARS {
        return Err(FollowHintMessageError::TooLong);
    }
    Ok(Some(collapsed))
}

pub fn render_follow_hint(template: Option<&str>, chatter_login: &str) -> String {
    let template = template
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .unwrap_or(FOLLOW_HINT_DEFAULT_MESSAGE);
    if template.contains(FOLLOW_HINT_PLACEHOLDER) {
        template.replace(FOLLOW_HINT_PLACEHOLDER, chatter_login)
    } else {
        format!("@{chatter_login} {template}")
    }
}

fn is_candidate(event: &ChatMessageEvent) -> bool {
    let foreign_shared_chat = event
        .source_broadcaster_user_id
        .as_deref()
        .is_some_and(|source| !source.is_empty() && source != event.broadcaster_user_id);
    !event.chatter_user_id.is_empty()
        && !event.chatter_user_login.is_empty()
        && !event.is_mod_or_broadcaster()
        && !foreign_shared_chat
        && !event.text().trim_start().starts_with('!')
}

struct FollowHintSettings {
    enabled: bool,
    message: Option<String>,
}

pub struct FollowHint {
    api: Arc<dyn ChatApi>,
    pool: PgPool,
    cooldowns: Mutex<HashMap<String, Instant>>,
}

impl FollowHint {
    pub fn new(api: Arc<dyn ChatApi>, pool: PgPool) -> Self {
        Self {
            api,
            pool,
            cooldowns: Mutex::new(HashMap::new()),
        }
    }

    async fn settings(&self, event: &ChatMessageEvent, channel_login: &str) -> FollowHintSettings {
        let row: Result<Option<(i32, Option<String>)>, sqlx::Error> = sqlx::query_as(
            "SELECT COALESCE(follow_hint_enabled, 0), follow_hint_message
               FROM streamer_plans
              WHERE twitch_user_id = $2
                 OR LOWER(COALESCE(twitch_login, '')) = LOWER($1)
              ORDER BY (twitch_user_id = $2) DESC
              LIMIT 1",
        )
        .bind(&event.broadcaster_user_login)
        .bind(&event.broadcaster_user_id)
        .fetch_optional(&self.pool)
        .await;
        match row {
            Ok(Some((enabled, message))) => FollowHintSettings {
                enabled: enabled != 0,
                message,
            },
            Ok(None) => FollowHintSettings {
                enabled: false,
                message: None,
            },
            Err(error) => {
                warn!(channel = channel_login, %error, "follow hint settings unreadable, staying silent");
                FollowHintSettings {
                    enabled: false,
                    message: None,
                }
            }
        }
    }

    pub async fn maybe_send(&self, event: &ChatMessageEvent, channel_login: &str) -> bool {
        if !is_candidate(event) {
            return false;
        }
        if event.chatter_user_id == self.api.bot_user_id().await {
            return false;
        }
        let settings = self.settings(event, channel_login).await;
        if !settings.enabled {
            return false;
        }
        let Some(previous) = self.reserve(channel_login) else {
            info!(channel = channel_login, chatter = %event.chatter_user_login, "follow hint skipped: channel cooldown");
            return false;
        };
        match self
            .api
            .user_follows(&event.broadcaster_user_id, &event.chatter_user_id)
            .await
        {
            Ok(Some(false)) => {}
            Ok(Some(true)) => {
                self.release(channel_login, previous);
                info!(channel = channel_login, chatter = %event.chatter_user_login, "follow hint skipped: already following");
                return false;
            }
            Ok(None) => {
                self.release(channel_login, previous);
                info!(channel = channel_login, chatter = %event.chatter_user_login, "follow hint skipped: follow state unknown");
                return false;
            }
            Err(error) => {
                self.release(channel_login, previous);
                warn!(channel = channel_login, chatter = %event.chatter_user_login, %error, "follow hint skipped: follow lookup failed");
                return false;
            }
        }
        let message = render_follow_hint(settings.message.as_deref(), &event.chatter_user_login);
        match self
            .api
            .send_message(&event.broadcaster_user_id, &message)
            .await
        {
            Ok(SendOutcome::Sent) => {
                info!(channel = channel_login, chatter = %event.chatter_user_login, "follow hint sent");
                true
            }
            Ok(outcome) => {
                self.release(channel_login, previous);
                warn!(
                    channel = channel_login,
                    ?outcome,
                    "follow hint dropped by Twitch"
                );
                false
            }
            Err(error) => {
                self.release(channel_login, previous);
                warn!(channel = channel_login, %error, "follow hint not sent");
                false
            }
        }
    }

    fn reserve(&self, channel_login: &str) -> Option<Option<Instant>> {
        let mut cooldowns = self.cooldowns.lock().ok()?;
        let previous = cooldowns.get(channel_login).copied();
        if previous.is_some_and(|last| last.elapsed() < CHANNEL_COOLDOWN) {
            return None;
        }
        cooldowns.insert(channel_login.to_string(), Instant::now());
        Some(previous)
    }

    fn release(&self, channel_login: &str, previous: Option<Instant>) {
        if let Ok(mut cooldowns) = self.cooldowns.lock() {
            match previous {
                Some(last) => cooldowns.insert(channel_login.to_string(), last),
                None => cooldowns.remove(channel_login),
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_replaces_placeholder_or_prefixes_mention() {
        assert_eq!(
            render_follow_hint(Some("Hey {user}, lass nen Follow da!"), "neuling"),
            "Hey neuling, lass nen Follow da!"
        );
        assert_eq!(
            render_follow_hint(Some("Lass nen Follow da!"), "neuling"),
            "@neuling Lass nen Follow da!"
        );
        assert!(render_follow_hint(None, "neuling").starts_with("@neuling "));
        assert!(render_follow_hint(Some("   "), "neuling").starts_with("@neuling "));
    }

    #[test]
    fn normalize_collapses_whitespace_and_limits_length() {
        assert_eq!(normalize_follow_hint_message("  \n "), Ok(None));
        assert_eq!(
            normalize_follow_hint_message("a\n  b"),
            Ok(Some("a b".to_string()))
        );
        assert_eq!(
            normalize_follow_hint_message(&"x".repeat(FOLLOW_HINT_MAX_CHARS + 1)),
            Err(FollowHintMessageError::TooLong)
        );
    }
}
