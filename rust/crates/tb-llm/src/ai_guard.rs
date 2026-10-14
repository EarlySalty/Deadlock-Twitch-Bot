use sqlx::postgres::PgPool;

use crate::hub::{LlmError, Request};

pub const STREAMER_PURPOSES: &[&str] = &[
    "promo_pitch",
    "lfg_pitch",
    "invite_question",
    "spam-review",
    "conversation_scam_judge",
    "conversation_scam_followup",
    "engagement",
    "channel_profile",
    "outreach_shadow",
    "title",
    "title-insight",
    "ai-analysis",
    "dashboard_ai_analysis",
    "ai-chat",
    "dashboard_ai_chat",
    "dashboard_chat_deep_analysis",
    "dashboard-assistent",
    "post-stream-report",
    "post-stream-report-claude",
    "stream_audit",
];

pub fn is_streamer_purpose(purpose: &str) -> bool {
    STREAMER_PURPOSES.contains(&purpose)
}

pub(crate) async fn check(
    use_case: &str,
    purpose: &str,
    request: &Request,
) -> Result<(), LlmError> {
    let id = clean(request.twitch_broadcaster.as_deref());
    let login = clean(request.twitch_channel.as_deref()).map(str::to_ascii_lowercase);
    if id.is_none() && login.is_none() {
        tb_observability::warning_budget::warn(
            "ai_guard_ohne_streamer",
            "Herkunft der Eingabedaten ist nicht eindeutig; KI-Aufruf gesperrt",
        );
        tracing::warn!(
            use_case,
            purpose,
            "KI_GUARD: Aufruf ohne Streamerbezug abgewiesen"
        );
        return Err(LlmError::Disabled(
            "Herkunft der Eingabedaten ist nicht eindeutig; KI-Aufruf gesperrt".into(),
        ));
    }
    let Some(pool) = crate::ledger::pool().await else {
        tb_observability::warning_budget::warn(
            "ai_guard_db",
            "KI-Schalter nicht prüfbar; KI-Aufruf gesperrt",
        );
        tracing::debug!(use_case, purpose, "KI_GUARD: Datenbankpool fehlt");
        return Err(LlmError::Unavailable(
            "KI-Schalter nicht prüfbar; KI-Aufruf gesperrt".into(),
        ));
    };

    let (twitch_user_id, erlaubt) = streamer_verdict(pool, id, login.as_deref()).await?;
    tracing::debug!(
        use_case,
        purpose,
        twitch_user_id = twitch_user_id.as_deref(),
        erlaubt,
        "KI_GUARD: KI-Zugriff geprüft"
    );
    if !erlaubt || twitch_user_id.is_none() {
        return Err(LlmError::Disabled(
            "KI-Funktionen sind für diesen Streamer nicht freigegeben".into(),
        ));
    }
    Ok(())
}

pub async fn streamer_ai_allowed(
    twitch_user_id: Option<&str>,
    channel_login: Option<&str>,
) -> Result<bool, LlmError> {
    let id = clean(twitch_user_id);
    let login = clean(channel_login).map(str::to_ascii_lowercase);
    if id.is_none() && login.is_none() {
        return Ok(false);
    }
    let Some(pool) = crate::ledger::pool().await else {
        return Ok(false);
    };
    Ok(streamer_verdict(pool, id, login.as_deref()).await?.1)
}

pub(crate) async fn twitch_user_ai_allowed(
    pool: &PgPool,
    twitch_user_id: &str,
) -> Result<bool, LlmError> {
    match clean(Some(twitch_user_id)) {
        Some(id) => Ok(streamer_verdict(pool, Some(id), None).await?.1),
        None => Ok(false),
    }
}

fn clean(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

async fn streamer_verdict(
    pool: &PgPool,
    id: Option<&str>,
    login: Option<&str>,
) -> Result<(Option<String>, bool), LlmError> {
    load(pool, id, login).await.map_err(|error| {
        tb_observability::warning_budget::warn(
            "ai_guard_db",
            "KI-Schalter nicht lesbar; KI-Aufruf gesperrt",
        );
        tracing::debug!(%error, "KI_GUARD: Abfrage fehlgeschlagen");
        LlmError::Unavailable("KI-Schalter nicht lesbar; KI-Aufruf gesperrt".into())
    })
}

async fn load(
    pool: &PgPool,
    id: Option<&str>,
    login: Option<&str>,
) -> Result<(Option<String>, bool), sqlx::Error> {
    sqlx::query_as::<_, (Option<String>, bool)>(
        "SELECT COALESCE( \
                    $1::text, \
                    (SELECT i.twitch_user_id FROM public.twitch_streamer_identities i \
                      WHERE LOWER(i.twitch_login) = $2 \
                      ORDER BY i.twitch_user_id LIMIT 1)), \
                public.twitch_streamer_ai_allowed($1, $2)",
    )
    .bind(id)
    .bind(login)
    .fetch_one(pool)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streamer_zwecke() {
        assert!(is_streamer_purpose("promo_pitch"));
        assert!(is_streamer_purpose("lfg_pitch"));
        assert!(is_streamer_purpose("spam-review"));
        assert!(!is_streamer_purpose("social-media-fireworks"));
        assert!(!is_streamer_purpose("conversation_scam_guidance"));
        assert!(!is_streamer_purpose("community_sentiment"));
    }

    #[tokio::test]
    async fn ohne_ledger_pool_sperrt_der_guard() {
        if !crate::hub_test_fixture::in_isolated_process(
            "ai_guard::tests::ohne_ledger_pool_sperrt_der_guard",
        ) {
            return;
        }
        for request in [
            Request::prompt("x").twitch_channel("Kanal"),
            Request::prompt("x").twitch_broadcaster("42"),
        ] {
            assert!(matches!(
                check("engagement", "engagement", &request).await,
                Err(LlmError::Unavailable(_))
            ));
        }
        assert!(!matches!(
            streamer_ai_allowed(Some("42"), None).await,
            Ok(true)
        ));
    }

    #[tokio::test]
    async fn ohne_streamerbezug_sperrt_jeder_zweck() {
        for purpose in STREAMER_PURPOSES.iter().copied().chain([
            "community_sentiment",
            "conversation_scam_guidance",
            "social-media-fireworks",
            "unbekannter_zweck",
        ]) {
            for request in [
                Request::prompt("x"),
                Request::prompt("x")
                    .twitch_broadcaster("  ")
                    .twitch_channel(" \t "),
            ] {
                assert!(matches!(
                    check("engagement", purpose, &request).await,
                    Err(LlmError::Disabled(_))
                ));
            }
        }
        assert_eq!(streamer_ai_allowed(None, None).await, Ok(false));
        assert_eq!(streamer_ai_allowed(Some(" "), Some("\t")).await, Ok(false));
    }
}
