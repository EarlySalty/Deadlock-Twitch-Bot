use sqlx::PgPool;

use crate::{LlmError, Request};

const STREAMER_PURPOSES: &[&str] = &[
    "promo_pitch",
    "lfg_pitch",
    "invite_question",
    "spam-review",
    "spam_judge",
    "conversation_scam_judge",
    "conversation_scam_followup",
    "engagement",
    "channel_profile",
    "outreach_shadow",
    "title_ai",
    "title",
    "title-insight",
    "ai_analysis",
    "ai-analysis",
    "dashboard_ai_analysis",
    "ai_chat",
    "ai-chat",
    "dashboard_ai_chat",
    "dashboard_chat_deep_analysis",
    "dashboard-assistent",
    "dashboard_assistent",
    "conversation_scam_guidance",
    "community_sentiment",
    "bot_memory",
    "reaction_profile",
    "chat_message_type",
    "post_stream",
    "post_stream_opus",
    "post-stream-report",
    "post-stream-report-claude",
    "stream_audit",
    "social_media",
    "social-media-fireworks",
];

pub(crate) async fn check(use_case: &str, request: &Request) -> Result<(), LlmError> {
    let purpose = match &request.ledger {
        Some(crate::Ledger::Purpose(purpose)) => purpose.as_str(),
        _ => use_case,
    };
    if !request.twitch_channels.is_empty() {
        let Some(pool) = crate::ledger::pool().await else {
            return Err(LlmError::Unavailable(
                "KI-Schalter konnte nicht geprüft werden.".into(),
            ));
        };
        let allowed: bool = sqlx::query_scalar(
            "SELECT COALESCE(bool_and(public.twitch_streamer_ai_allowed(NULL, channel)), false)
             FROM (SELECT DISTINCT unnest($1::text[]) AS channel) sources",
        )
        .bind(&request.twitch_channels)
        .fetch_one(pool)
        .await
        .map_err(|_| LlmError::Unavailable("KI-Schalter konnte nicht geprüft werden.".into()))?;
        if !allowed {
            return Err(LlmError::Disabled(
                "KI-Funktionen sind für einen Quellkanal ausgeschaltet.".into(),
            ));
        }
        if request.twitch_broadcaster.is_none() && request.twitch_channel.is_none() {
            return Ok(());
        }
    }
    if request.twitch_broadcaster.is_none()
        && request.twitch_channel.is_none()
        && !STREAMER_PURPOSES.contains(&use_case)
        && !STREAMER_PURPOSES.contains(&purpose)
    {
        return Ok(());
    }
    if streamer_ai_allowed(
        request.twitch_broadcaster.as_deref(),
        request.twitch_channel.as_deref(),
    )
    .await?
    {
        Ok(())
    } else {
        Err(LlmError::Disabled(
            "KI-Funktionen sind für diesen Streamer ausgeschaltet.".into(),
        ))
    }
}

pub async fn streamer_ai_allowed(id: Option<&str>, login: Option<&str>) -> Result<bool, LlmError> {
    let id = clean(id);
    let login = clean(login);
    if id.is_none() && login.is_none() {
        return Ok(false);
    }
    let Some(pool) = crate::ledger::pool().await else {
        return Err(LlmError::Unavailable(
            "KI-Schalter konnte nicht geprüft werden.".into(),
        ));
    };
    allowed_with_pool(pool, id, login).await
}

pub async fn allowed_with_pool(
    pool: &PgPool,
    id: Option<&str>,
    login: Option<&str>,
) -> Result<bool, LlmError> {
    sqlx::query_scalar("SELECT public.twitch_streamer_ai_allowed($1, $2)")
        .bind(clean(id))
        .bind(clean(login))
        .fetch_one(pool)
        .await
        .map_err(|error| {
            tracing::debug!(%error, "AI policy query failed");
            LlmError::Unavailable("KI-Schalter konnte nicht geprüft werden.".into())
        })
}

fn clean(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn missing_scope_rejects_streamer_calls_before_provider_dispatch() {
        for purpose in STREAMER_PURPOSES {
            let error = crate::complete(purpose, Request::prompt("test").no_ledger()).await;
            assert!(matches!(error, Err(LlmError::Disabled(_))), "{purpose}");
        }
    }
}
