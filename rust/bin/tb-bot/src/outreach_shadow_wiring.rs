use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::PgPool;
use tb_config::BrokerConfig;
use tb_engagement::audio_capture::{CaptureError, MemoryAudioCapturer};
use tb_engagement::outreach_shadow::{
    CycleResult, EvidenceSource, NewOutreachEvent, OutreachError, OutreachReviewClient,
};
use tb_engagement::outreach_shadow_store::{
    ClaimedOutreachEvent, OutreachEvent, OutreachShadowStore, StoreError,
};
use tb_engagement::transcribe::{OpenAiTranscriber, TranscribeError};
use tb_raid::{
    build_recruitment_message, plan_recruitment_delivery, RecruitmentDeliveryConfig,
    RecruitmentDeliveryRequest,
};
use tb_transport_discord::{
    BrokerRelay, DeleteMessage, DiscordBackend, DiscordError, SendRichMessage,
};

use crate::task_supervisor::TaskSupervisor;

const PROCESS_INTERVAL: Duration = Duration::from_secs(5);
const DISCORD_INTERVAL: Duration = Duration::from_secs(30);
const RETENTION_INTERVAL: Duration = Duration::from_secs(60 * 60);
const AUDIO_SEGMENT: Duration = Duration::from_secs(20);
const DISCORD_BATCH_LIMIT: i64 = 20;
const RETENTION_BATCH_LIMIT: i64 = 20;
const DEFAULT_REVIEW_CHANNEL_ID: i64 = 1_374_364_800_817_303_632;
const OUTREACH_GOLD: i64 = 0xC8A86B;

/// Beschriftungen der Discord-Review-Karte.
///
/// Fehlt ein Feld, bleibt der Post fail-closed aus. `retention_delete_reason`
/// ist der Audit-Grund beim späteren Löschen.
const OUTREACH_DISCORD_COPY_JSON: &str = r#"{
  "title": "Selbstvermarktung im Schattenbetrieb",
  "channel": "Kanal",
  "session": "Sitzung",
  "duration": "Laufzeit",
  "stage": "Stufe",
  "outcome": "Ergebnis",
  "evidence": "Beleg",
  "occasion": "Anlass",
  "evidence_source": "Quelle des Belegs",
  "evidence_at": "Zeitpunkt des Belegs",
  "opener": "Das würde der Bot sagen",
  "why": "Begründung",
  "confidence": "Sicherheit",
  "static_comparison": "Der bisherige Bot hätte gesendet",
  "silent_reason": "Warum nichts vorgeschlagen wurde",
  "error_class": "Fehlerart",
  "source_transcript": "Gesagt im Stream",
  "source_chat": "Aus dem Chat",
  "retention_delete_reason": "Aufbewahrungsfrist abgelaufen"
}"#;

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OutreachDeliveryTarget {
    Database,
    Discord,
}

#[cfg(test)]
impl OutreachDeliveryTarget {
    const ALL: [Self; 2] = [Self::Database, Self::Discord];
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct OutreachConfig {
    enabled: bool,
}

impl OutreachConfig {
    fn from_env() -> Self {
        let raw = std::env::var("OUTREACH_SHADOW_ENABLED").ok();
        Self::from_value(raw.as_deref())
    }

    fn from_value(value: Option<&str>) -> Self {
        Self {
            enabled: value.is_some_and(|value| {
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            }),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DiscordCopy {
    title: String,
    channel: String,
    session: String,
    duration: String,
    stage: String,
    outcome: String,
    evidence: String,
    occasion: String,
    evidence_source: String,
    evidence_at: String,
    opener: String,
    why: String,
    confidence: String,
    static_comparison: String,
    silent_reason: String,
    error_class: String,
    source_transcript: String,
    source_chat: String,
    retention_delete_reason: String,
}

impl DiscordCopy {
    fn configured() -> Option<Self> {
        let copy = serde_json::from_str::<Self>(OUTREACH_DISCORD_COPY_JSON).ok()?;
        [
            &copy.title,
            &copy.channel,
            &copy.session,
            &copy.duration,
            &copy.stage,
            &copy.outcome,
            &copy.evidence,
            &copy.occasion,
            &copy.evidence_source,
            &copy.evidence_at,
            &copy.opener,
            &copy.why,
            &copy.confidence,
            &copy.static_comparison,
            &copy.silent_reason,
            &copy.error_class,
            &copy.source_transcript,
            &copy.source_chat,
            &copy.retention_delete_reason,
        ]
        .iter()
        .all(|value| !value.trim().is_empty())
        .then_some(copy)
    }

    #[cfg(test)]
    fn test_copy() -> Self {
        Self {
            title: "title".to_owned(),
            channel: "channel".to_owned(),
            session: "session".to_owned(),
            duration: "duration".to_owned(),
            stage: "stage".to_owned(),
            outcome: "outcome".to_owned(),
            evidence: "evidence".to_owned(),
            occasion: "occasion".to_owned(),
            evidence_source: "evidence_source".to_owned(),
            evidence_at: "evidence_at".to_owned(),
            opener: "opener".to_owned(),
            why: "why".to_owned(),
            confidence: "confidence".to_owned(),
            static_comparison: "static_comparison".to_owned(),
            silent_reason: "silent_reason".to_owned(),
            error_class: "error_class".to_owned(),
            source_transcript: "source_transcript".to_owned(),
            source_chat: "source_chat".to_owned(),
            retention_delete_reason: "retention_delete_reason".to_owned(),
        }
    }
}

#[derive(Clone)]
pub struct OutreachShadowRuntime {
    store: OutreachShadowStore,
}

impl OutreachShadowRuntime {
    pub async fn close_open_session(&self, reason: &str) {
        if let Err(error) = self.store.close_all_open_sessions(reason, Utc::now()).await {
            tracing::warn!(event = "outreach_shadow.session_close_failed", %error, reason);
        }
    }
}

pub fn start(
    supervisor: &TaskSupervisor,
    pool: PgPool,
    broker: &BrokerConfig,
) -> OutreachShadowRuntime {
    let store = OutreachShadowStore::new(pool);
    let config = OutreachConfig::from_env();
    if !config.enabled {
        tracing::info!(event = "outreach_shadow.disabled");
        return inactive_runtime(supervisor, store, "kill_switch");
    }
    let Some(copy) = DiscordCopy::configured() else {
        tracing::warn!(event = "outreach_shadow.discord_copy_missing");
        return inactive_runtime(supervisor, store, "discord_copy_missing");
    };
    let Some(transcriber) = OpenAiTranscriber::from_env() else {
        tracing::warn!(event = "outreach_shadow.transcriber_unavailable");
        return inactive_runtime(supervisor, store, "openai_unavailable");
    };
    let reviewer = match OutreachReviewClient::from_env() {
        Ok(reviewer) => reviewer,
        Err(error) => {
            tracing::warn!(
                event = "outreach_shadow.reviewer_unavailable",
                error_class = error.code()
            );
            return inactive_runtime(supervisor, store, "fireworks_unavailable");
        }
    };
    let discord: Arc<dyn DiscordBackend> = match BrokerRelay::new(broker) {
        Ok(relay) => Arc::new(relay),
        Err(error) => {
            tracing::warn!(event = "outreach_shadow.discord_unavailable", %error);
            return inactive_runtime(supervisor, store, "discord_unavailable");
        }
    };
    let capturer = MemoryAudioCapturer::new(
        nonempty_env("YTDLP_BIN").unwrap_or_else(|| "yt-dlp".to_owned()),
        nonempty_env("FFMPEG_BIN").unwrap_or_else(|| "ffmpeg".to_owned()),
    );
    spawn_processor(supervisor, store.clone(), capturer, transcriber, reviewer);
    spawn_discord_forwarder(
        supervisor,
        store.clone(),
        discord.clone(),
        DEFAULT_REVIEW_CHANNEL_ID,
        copy.clone(),
    );
    spawn_retention(supervisor, store.clone(), discord, copy);
    tracing::info!(
        event = "outreach_shadow.started",
        channel_id = DEFAULT_REVIEW_CHANNEL_ID,
        segment_seconds = AUDIO_SEGMENT.as_secs(),
    );
    OutreachShadowRuntime { store }
}

fn inactive_runtime(
    supervisor: &TaskSupervisor,
    store: OutreachShadowStore,
    reason: &'static str,
) -> OutreachShadowRuntime {
    let close_store = store.clone();
    supervisor.spawn_finite("outreach_shadow_inactive_close", async move {
        if let Err(error) = close_store
            .close_all_open_sessions(reason, Utc::now())
            .await
        {
            tracing::warn!(event = "outreach_shadow.inactive_close_failed", %error, reason);
        }
    });
    OutreachShadowRuntime { store }
}

fn spawn_processor(
    supervisor: &TaskSupervisor,
    store: OutreachShadowStore,
    capturer: MemoryAudioCapturer,
    transcriber: OpenAiTranscriber,
    reviewer: OutreachReviewClient,
) {
    supervisor.spawn("outreach_shadow_processor", async move {
        if let Err(error) = store
            .close_all_open_sessions("process_start", Utc::now())
            .await
        {
            tracing::warn!(event = "outreach_shadow.startup_close_failed", %error);
        }
        let mut tick = tokio::time::interval(PROCESS_INTERVAL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            if let Err(error) =
                process_once(&store, &capturer, &transcriber, &reviewer, Utc::now()).await
            {
                tracing::warn!(event = "outreach_shadow.process_failed", %error);
            }
        }
    });
}

async fn process_once(
    store: &OutreachShadowStore,
    capturer: &MemoryAudioCapturer,
    transcriber: &OpenAiTranscriber,
    reviewer: &OutreachReviewClient,
    now: DateTime<Utc>,
) -> Result<(), StoreError> {
    store.close_ineligible_session(now).await?;
    if store.active_session().await?.is_none() {
        store.start_next_session(now).await?;
    }
    let Some(claim) = store.claim_active_session(now).await? else {
        return Ok(());
    };
    let session = claim.session;
    let mut close_reason = None;
    let (transcript, result) = match capturer
        .capture_wav(&session.channel_login, AUDIO_SEGMENT)
        .await
    {
        Ok(wav) => match transcriber.transcribe_bytes(wav).await {
            Ok(transcript) if transcript.text.trim().is_empty() => {
                store
                    .release_processor_claim(session.id, claim.claim_id, true)
                    .await?;
                return Ok(());
            }
            Ok(transcript) => {
                let text = transcript.text.trim().to_owned();
                let context = store.load_context(&session, &text, Utc::now()).await?;
                let result = match reviewer.decide(&context.input, &context.evidence).await {
                    Ok(decision) => CycleResult::Decision(decision),
                    Err(OutreachError::Decode | OutreachError::Validation) => {
                        CycleResult::ParserError
                    }
                    Err(OutreachError::Timeout) => CycleResult::Timeout,
                    Err(error) => CycleResult::ProviderError(error.code().to_owned()),
                };
                let mut event = NewOutreachEvent::from_cycle_result(
                    &session,
                    claim.cycle_id,
                    Utc::now(),
                    Some(text.clone()),
                    result,
                );
                event.static_recruitment_text =
                    static_recruitment_text(&session, context.raid_count);
                persist_and_log(store, event, claim.claim_id).await?;
                return Ok(());
            }
            Err(TranscribeError::Timeout) => (None, CycleResult::Timeout),
            Err(error) => (
                None,
                CycleResult::WhisperError(transcribe_error_class(&error).to_owned()),
            ),
        },
        Err(error) => {
            if matches!(
                &error,
                CaptureError::SourceStart
                    | CaptureError::SourceTimeout
                    | CaptureError::SourceUnavailable
            ) {
                close_reason = Some("stream_unavailable");
            }
            (
                None,
                CycleResult::WhisperError(capture_error_class(&error).to_owned()),
            )
        }
    };
    let event = NewOutreachEvent::from_cycle_result(
        &session,
        claim.cycle_id,
        Utc::now(),
        transcript,
        result,
    );
    persist_and_log(store, event, claim.claim_id).await?;
    if let Some(reason) = close_reason {
        store.close_active_session(reason, Utc::now()).await?;
    }
    Ok(())
}

async fn persist_and_log(
    store: &OutreachShadowStore,
    event: NewOutreachEvent,
    claim_id: Uuid,
) -> Result<(), StoreError> {
    let session_id = event.session_id;
    let channel = event.channel_login.clone();
    let outcome = event.outcome.as_str();
    let stage = event.stage.as_str();
    let hooks = event
        .decision
        .as_ref()
        .map_or(0, |decision| decision.hooks.len());
    let error_class = event.error_class.as_deref().unwrap_or("none").to_owned();
    let inserted = store.record_cycle(&event).await?;
    store
        .release_processor_claim(session_id, claim_id, true)
        .await?;
    if inserted {
        tracing::info!(
            event = "outreach_shadow.cycle_persisted",
            %session_id,
            %channel,
            stage,
            hooks,
            error_class,
            outcome,
        );
    }
    Ok(())
}

fn static_recruitment_text(
    session: &tb_engagement::outreach_shadow::OutreachSession,
    raid_count: i64,
) -> Option<String> {
    let request = RecruitmentDeliveryRequest {
        from_broadcaster_login: String::new(),
        to_broadcaster_login: session.channel_login.clone(),
        target_id: Some(session.streamer_user_id.clone()),
        recent_raid_count: 0,
        total_recruitment_raid_count: Some(raid_count.max(1)),
        followers_total: None,
        chat_bot_available: true,
        target_blacklisted: false,
        target_is_partner: false,
        outbound_chat_suppressed: false,
    };
    plan_recruitment_delivery(&request, &RecruitmentDeliveryConfig::default())
        .message_variant
        .map(|variant| build_recruitment_message(variant, &session.channel_login))
}

fn spawn_discord_forwarder(
    supervisor: &TaskSupervisor,
    store: OutreachShadowStore,
    discord: Arc<dyn DiscordBackend>,
    channel_id: i64,
    copy: DiscordCopy,
) {
    supervisor.spawn("outreach_shadow_discord_forwarder", async move {
        let mut tick = tokio::time::interval(DISCORD_INTERVAL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            if let Err(error) =
                forward_discord_once(&store, discord.as_ref(), channel_id, &copy).await
            {
                tracing::warn!(event = "outreach_shadow.discord_forward_failed", %error);
            }
        }
    });
}

fn spawn_retention(
    supervisor: &TaskSupervisor,
    store: OutreachShadowStore,
    discord: Arc<dyn DiscordBackend>,
    copy: DiscordCopy,
) {
    supervisor.spawn("outreach_shadow_retention", async move {
        let mut tick = tokio::time::interval(RETENTION_INTERVAL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            if let Err(error) = cleanup_once(&store, discord.as_ref(), &copy, Utc::now()).await {
                tracing::warn!(event = "outreach_shadow.retention_failed", %error);
            }
        }
    });
}

async fn cleanup_once(
    store: &OutreachShadowStore,
    discord: &dyn DiscordBackend,
    copy: &DiscordCopy,
    now: DateTime<Utc>,
) -> Result<(), String> {
    store
        .delete_expired_unposted(now)
        .await
        .map_err(|error| error.to_string())?;
    for event in store
        .expired_discord_events(RETENTION_BATCH_LIMIT, now)
        .await
        .map_err(|error| error.to_string())?
    {
        match discord
            .delete_message(DeleteMessage {
                channel_id: DEFAULT_REVIEW_CHANNEL_ID,
                message_id: event.message_id.clone(),
                reason: copy.retention_delete_reason.clone(),
            })
            .await
        {
            // Ein bereits verschwundener Post ist das Ziel dieses Laufs, kein
            // Fehler. Ohne diesen Zweig würde der unbegrenzte Retry ewig auf
            // einer von Hand gelöschten Nachricht weiterlaufen.
            Ok(()) | Err(DiscordError::BrokerError { status: 404, .. }) => store
                .delete_expired_event(event.id, &event.message_id, now)
                .await
                .map_err(|error| error.to_string())?,
            Err(error) => store
                .mark_discord_delete_failed(event.id, discord_error_class(&error), now)
                .await
                .map_err(|store_error| store_error.to_string())?,
        }
    }
    Ok(())
}

async fn forward_discord_once(
    store: &OutreachShadowStore,
    discord: &dyn DiscordBackend,
    channel_id: i64,
    copy: &DiscordCopy,
) -> Result<(), String> {
    let claims = store
        .claim_discord_events(DISCORD_BATCH_LIMIT, Utc::now())
        .await
        .map_err(|error| error.to_string())?;
    for ClaimedOutreachEvent { claim_id, event } in claims {
        let payload = match build_discord_card(&event, channel_id, copy) {
            Ok(payload) => payload,
            Err(error_class) => {
                store
                    .mark_discord_failed(event.id, claim_id, &error_class, Utc::now())
                    .await
                    .map_err(|error| error.to_string())?;
                continue;
            }
        };
        match discord.send_rich_message(payload).await {
            Ok(sent) => store
                .mark_discord_sent(event.id, claim_id, &sent.result.message_id)
                .await
                .map_err(|error| error.to_string())?,
            Err(error) => {
                let error_class = discord_error_class(&error);
                store
                    .mark_discord_failed(event.id, claim_id, error_class, Utc::now())
                    .await
                    .map_err(|store_error| store_error.to_string())?;
            }
        }
    }
    Ok(())
}

fn build_discord_card(
    event: &OutreachEvent,
    channel_id: i64,
    copy: &DiscordCopy,
) -> Result<SendRichMessage, String> {
    let runtime = (event.occurred_at - event.session.started_at)
        .num_seconds()
        .max(0);
    let mut displays = vec![format!(
        "**{}**\n{}: #{}\n{}: `{}`\n{}: {}s\n{}: `{}`\n{}: `{}`",
        copy.title,
        copy.channel,
        clean(&event.session.channel_login),
        copy.session,
        event.session.id,
        copy.duration,
        runtime,
        copy.stage,
        event.session.stage.as_str(),
        copy.outcome,
        event.outcome.as_str()
    )];
    if let Some(decision) = &event.decision {
        if let Some(reason) = &decision.silent_reason {
            displays.push(format!("**{}**\n{}", copy.silent_reason, clean(reason)));
        }
        for hook in &decision.hooks {
            let source = match hook.evidence_source {
                EvidenceSource::Transcript => &copy.source_transcript,
                EvidenceSource::Chat => &copy.source_chat,
            };
            displays.push(format!(
                "**{} · {}**\n{}: {}\n{}: {}\n{}: {}\n{}: {}\n{}: {}\n{}: {}\n{}: {:.3}",
                copy.evidence,
                hook.kind.as_str(),
                copy.occasion,
                hook.occasion.map_or("-", |occasion| occasion.as_str()),
                copy.evidence_source,
                source,
                copy.evidence_at,
                hook.evidence_at.to_rfc3339(),
                copy.evidence,
                clean(&hook.evidence),
                copy.opener,
                clean(&hook.opener),
                copy.why,
                clean(&hook.why),
                copy.confidence,
                hook.confidence
            ));
        }
    }
    if let Some(error) = &event.error_class {
        displays.push(format!("**{}**\n`{}`", copy.error_class, clean(error)));
    }
    if let Some(static_text) = &event.static_recruitment_text {
        displays.push(format!(
            "**{}**\n{}",
            copy.static_comparison,
            clean(static_text)
        ));
    }
    if displays
        .iter()
        .any(|display| display.chars().count() > 3_800)
    {
        return Err("discord_text_too_long".to_owned());
    }
    Ok(SendRichMessage {
        channel_id,
        content: None,
        embed: json!({}),
        components: Some(json!([{
            "type": 17,
            "accent_color": OUTREACH_GOLD,
            "components": displays.into_iter().map(|content| json!({
                "type": 10,
                "content": content
            })).collect::<Vec<_>>()
        }])),
        allowed_role_ids: vec![],
        view_spec: None,
    })
}

fn clean(value: &str) -> String {
    let neutralized = value
        .replace("@everyone", "everyone")
        .replace("@here", "here")
        .replace("<@", "< @")
        .replace("<#", "< #")
        .replace("<&", "< &")
        .replace("https://", "https ://")
        .replace("http://", "http ://")
        .replace("www.", "www .")
        .replace("discord.gg", "discord .gg");
    let mut cleaned = String::with_capacity(neutralized.len());
    for character in neutralized.chars() {
        if character.is_control()
            || matches!(
                character,
                '\u{061c}'
                    | '\u{200e}'
                    | '\u{200f}'
                    | '\u{202a}'..='\u{202e}'
                    | '\u{2066}'..='\u{2069}'
            )
        {
            cleaned.push(' ');
        } else {
            if matches!(
                character,
                '\\' | '*' | '_' | '~' | '`' | '|' | '[' | ']' | '(' | ')' | '>' | '#'
            ) {
                cleaned.push('\\');
            }
            cleaned.push(character);
        }
    }
    cleaned
}

fn capture_error_class(error: &CaptureError) -> &'static str {
    match error {
        CaptureError::InvalidInput => "invalid_input",
        CaptureError::SourceStart => "source_start",
        CaptureError::SourceTimeout => "source_timeout",
        CaptureError::SourceUnavailable => "source_unavailable",
        CaptureError::FfmpegStart => "ffmpeg_start",
        CaptureError::FfmpegTimeout => "ffmpeg_timeout",
        CaptureError::FfmpegFailed => "ffmpeg_failed",
        CaptureError::AudioEmpty => "audio_empty",
        CaptureError::AudioTooLarge => "audio_too_large",
        CaptureError::Legacy(_) => "legacy",
    }
}

fn transcribe_error_class(error: &TranscribeError) -> &'static str {
    match error {
        TranscribeError::Unavailable => "unavailable",
        TranscribeError::Timeout => "timeout",
        TranscribeError::Transport => "transport",
        TranscribeError::HttpStatus(_) => "http_status",
        TranscribeError::Decode => "decode",
    }
}

fn discord_error_class(error: &DiscordError) -> &'static str {
    match error {
        DiscordError::Http(error) if error.is_timeout() => "timeout",
        DiscordError::Http(_) => "transport",
        DiscordError::BrokerError { .. } => "broker_status",
        DiscordError::Deserialize(_) => "decode",
    }
}

fn nonempty_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use serde_json::Value;
    use sqlx::types::Uuid;
    use tb_engagement::outreach_shadow::{
        EvidenceSource, HookKind, OutreachDecision, OutreachHook, OutreachOutcome, OutreachSession,
        OutreachStage,
    };
    use tb_engagement::outreach_shadow_store::OutreachEvent;

    use super::*;

    #[test]
    fn runtime_hat_keinen_twitch_send_port() {
        assert_eq!(
            OutreachDeliveryTarget::ALL,
            [
                OutreachDeliveryTarget::Database,
                OutreachDeliveryTarget::Discord
            ]
        );
    }

    #[test]
    fn feature_ist_standardmaessig_aus() {
        assert!(!OutreachConfig::from_value(None).enabled);
    }

    #[test]
    fn discord_karte_nutzt_gold_components_v2_und_keine_mentions() {
        let now = Utc.with_ymd_and_hms(2026, 7, 27, 20, 0, 0).unwrap();
        let event = OutreachEvent {
            id: 1,
            session: OutreachSession {
                id: Uuid::nil(),
                channel_login: "kandidat".to_owned(),
                streamer_user_id: "42".to_owned(),
                started_at: now,
                stage: OutreachStage::Smalltalk,
            },
            cycle_id: Uuid::nil(),
            occurred_at: now,
            outcome: OutreachOutcome::Hook,
            transcript: Some("ich stream jeden tag".to_owned()),
            decision: Some(OutreachDecision {
                hooks: vec![OutreachHook {
                    kind: HookKind::Smalltalk,
                    occasion: None,
                    evidence: "ich stream jeden tag".to_owned(),
                    evidence_source: EvidenceSource::Transcript,
                    evidence_at: now,
                    opener: "wie laufen die runden".to_owned(),
                    why: "test".to_owned(),
                    confidence: 0.8,
                }],
                stage: OutreachStage::Smalltalk,
                silent_reason: None,
            }),
            static_recruitment_text: Some("statischer text".to_owned()),
            error_class: None,
            provider: Some("fireworks".to_owned()),
            model: Some("modell".to_owned()),
        };
        let payload =
            build_discord_card(&event, 123, &DiscordCopy::test_copy()).expect("Discord-Karte");
        let components = payload.components.expect("Components");

        assert_eq!(payload.channel_id, 123);
        assert!(payload.content.is_none());
        assert!(payload.allowed_role_ids.is_empty());
        assert_eq!(components[0]["type"], Value::from(17));
        assert_eq!(components[0]["accent_color"], Value::from(0xC8A86B));
    }

    #[test]
    fn discord_text_neutralisiert_mentions_markdown_links_und_bidi() {
        let cleaned = clean("@everyone **Titel** [Link](https://example.com)\u{202e}<@42>");

        assert!(!cleaned.contains("@everyone"));
        assert!(!cleaned.contains("**Titel**"));
        assert!(!cleaned.contains("https://"));
        assert!(!cleaned.contains('\u{202e}'));
        assert!(!cleaned.contains("<@"));
    }
}
