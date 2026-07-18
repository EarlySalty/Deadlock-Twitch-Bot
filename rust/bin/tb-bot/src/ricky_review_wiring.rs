use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use sqlx::types::Uuid;
use sqlx::PgPool;
use tb_config::BrokerConfig;
use tb_engagement::audio_capture::{CaptureError, MemoryAudioCapturer};
use tb_engagement::crew_review::{
    ClaimedModelInputs, CrewReviewTrigger, FireworksReviewClient, NewReviewEvent, ReviewDecision,
    ReviewError, ReviewEvent, ReviewEventKind, ReviewModelInput, ReviewSession, RickyChatInput,
    FIREWORKS_DEFAULT_MODEL,
};
use tb_engagement::crew_review_store::DiscordCard;
use tb_engagement::crew_review_store::{CrewReviewStore, StoreError};
use tb_engagement::transcribe::{OpenAiTranscriber, TranscribeError};
use tb_transport_discord::{
    BrokerRelay, DeleteMessage, DiscordBackend, DiscordError, SendRichMessage,
};

use crate::task_supervisor::TaskSupervisor;

const PROCESS_INTERVAL: Duration = Duration::from_secs(5);
const DISCORD_FORWARD_INTERVAL: Duration = Duration::from_secs(60);
const RETENTION_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const AUDIO_CAPTURE_DURATION: Duration = Duration::from_secs(20);
const DISCORD_BATCH_LIMIT: i64 = 20;
const RETENTION_LIMIT: i64 = 50;
const DISCORD_CARD_MAX_CHARS: usize = 3_500;
const RICKY_REVIEW_CHANNEL_ID: i64 = 1_374_364_800_817_303_632;
const RICKY_REVIEW_GOLD: i64 = 0xC8A86B;

pub fn start(
    supervisor: &TaskSupervisor,
    pool: PgPool,
    broker: &BrokerConfig,
) -> Arc<dyn CrewReviewTrigger> {
    let store = CrewReviewStore::new(pool);
    let trigger: Arc<dyn CrewReviewTrigger> = Arc::new(PgRickyReviewTrigger {
        store: store.clone(),
        supervisor: supervisor.clone(),
    });
    spawn_processor(supervisor, store.clone());

    match BrokerRelay::new(broker) {
        Ok(relay) => {
            let discord: Arc<dyn DiscordBackend> = Arc::new(relay);
            spawn_discord_forwarder(supervisor, store.clone(), Arc::clone(&discord));
            spawn_retention(supervisor, store, discord);
        }
        Err(error) => {
            tracing::warn!(
                "Ricky-Review Discord-Laeufe nicht gestartet: BrokerRelay nicht initialisierbar: {error}"
            );
        }
    }

    trigger
}

struct PgRickyReviewTrigger {
    store: CrewReviewStore,
    supervisor: TaskSupervisor,
}

impl CrewReviewTrigger for PgRickyReviewTrigger {
    fn observe(&self, input: RickyChatInput) {
        let store = self.store.clone();
        self.supervisor.spawn("ricky_review_trigger", async move {
            if let Err(error) = store.record_trigger(&input).await {
                tracing::warn!(%error, "Ricky-Review-Trigger konnte nicht geschrieben werden");
            }
        });
    }
}

fn spawn_processor(supervisor: &TaskSupervisor, store: CrewReviewStore) {
    supervisor.spawn("ricky_review_processor", async move {
        match store
            .close_all_open_sessions("process_restart", Utc::now())
            .await
        {
            Ok(closed) if closed > 0 => {
                tracing::info!(
                    closed,
                    "Ricky-Review: offene Sessions beim Start geschlossen"
                )
            }
            Ok(_) => {}
            Err(error) => tracing::warn!(
                %error,
                "Ricky-Review: Startup-Close offener Sessions fehlgeschlagen"
            ),
        }

        let decider = LiveReviewDecider::from_env();
        let audio = LiveAudioReviewer::from_env();
        let mut tick = tokio::time::interval(PROCESS_INTERVAL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            if let Err(error) = process_once(&store, &decider, &audio, Utc::now()).await {
                tracing::warn!(%error, "Ricky-Review-Prozessorlauf fehlgeschlagen");
            }
        }
    });
}

fn spawn_discord_forwarder(
    supervisor: &TaskSupervisor,
    store: CrewReviewStore,
    discord: Arc<dyn DiscordBackend>,
) {
    supervisor.spawn("ricky_review_discord_forwarder", async move {
        let mut tick = tokio::time::interval(DISCORD_FORWARD_INTERVAL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            match forward_discord_once(&store, discord.as_ref(), DISCORD_BATCH_LIMIT).await {
                Ok(sent) if sent > 0 => {
                    tracing::info!(sent, "Ricky-Review: Discord-Karten weitergeleitet")
                }
                Ok(_) => {}
                Err(error) => tracing::warn!(%error, "Ricky-Review Discord-Forward fehlgeschlagen"),
            }
        }
    });
}

fn spawn_retention(
    supervisor: &TaskSupervisor,
    store: CrewReviewStore,
    discord: Arc<dyn DiscordBackend>,
) {
    supervisor.spawn("ricky_review_retention", async move {
        if let Err(error) = cleanup_once(&store, discord.as_ref(), Utc::now()).await {
            tracing::warn!(%error, "Ricky-Review Retention-Cleanup fehlgeschlagen");
        }
        loop {
            tokio::time::sleep(RETENTION_INTERVAL).await;
            if let Err(error) = cleanup_once(&store, discord.as_ref(), Utc::now()).await {
                tracing::warn!(%error, "Ricky-Review Retention-Cleanup fehlgeschlagen");
            }
        }
    });
}

#[async_trait::async_trait]
trait ReviewDecider: Send + Sync {
    async fn decide_review(&self, input: &ReviewModelInput) -> Result<ReviewDecision, ReviewError>;
}

struct LiveReviewDecider {
    inner: Option<FireworksReviewClient>,
}

impl LiveReviewDecider {
    fn from_env() -> Self {
        Self {
            inner: FireworksReviewClient::from_env().ok(),
        }
    }
}

#[async_trait::async_trait]
impl ReviewDecider for LiveReviewDecider {
    async fn decide_review(&self, input: &ReviewModelInput) -> Result<ReviewDecision, ReviewError> {
        let Some(inner) = &self.inner else {
            return Err(ReviewError::Unavailable);
        };
        inner.decide(input).await
    }
}

struct AudioReview {
    text: String,
    duration_seconds: f64,
    provider: String,
    model: String,
}

#[derive(Debug)]
struct ProviderFailure {
    provider: &'static str,
    error_class: String,
}

impl ProviderFailure {
    fn new(provider: &'static str, error_class: impl std::fmt::Display) -> Self {
        Self {
            provider,
            error_class: bounded_error_class(&error_class.to_string()),
        }
    }
}

#[async_trait::async_trait]
trait AudioReviewer: Send + Sync {
    async fn review_audio(
        &self,
        channel_login: &str,
    ) -> Result<Option<AudioReview>, ProviderFailure>;
}

struct LiveAudioReviewer {
    capturer: MemoryAudioCapturer,
    transcriber: Option<OpenAiTranscriber>,
}

impl LiveAudioReviewer {
    fn from_env() -> Self {
        Self {
            capturer: MemoryAudioCapturer::new(
                nonempty_env("RICKY_SHADOW_REVIEW_YTDLP_BIN")
                    .unwrap_or_else(|| "yt-dlp".to_owned()),
                nonempty_env("FFMPEG_BIN").unwrap_or_else(|| "ffmpeg".to_owned()),
            ),
            transcriber: OpenAiTranscriber::from_env(),
        }
    }
}

#[async_trait::async_trait]
impl AudioReviewer for LiveAudioReviewer {
    async fn review_audio(
        &self,
        channel_login: &str,
    ) -> Result<Option<AudioReview>, ProviderFailure> {
        let Some(transcriber) = &self.transcriber else {
            return Ok(None);
        };
        let wav = self
            .capturer
            .capture_wav(channel_login, AUDIO_CAPTURE_DURATION)
            .await
            .map_err(capture_failure)?;
        let transcript = transcriber
            .transcribe_bytes(wav)
            .await
            .map_err(transcribe_failure)?;
        let text = transcript.text.trim().to_owned();
        if text.is_empty() {
            return Ok(None);
        }
        Ok(Some(AudioReview {
            text,
            duration_seconds: transcript.duration_seconds,
            provider: transcript.engine,
            model: transcript.model,
        }))
    }
}

async fn process_once<D, A>(
    store: &CrewReviewStore,
    decider: &D,
    audio: &A,
    now: DateTime<Utc>,
) -> Result<(), StoreError>
where
    D: ReviewDecider,
    A: AudioReviewer,
{
    store
        .close_inactive_sessions("inactivity_timeout", now)
        .await?;
    let sessions = store.active_sessions(now).await?;
    for session in sessions {
        process_pending_inputs(store, decider, session.session_id, now).await?;
        match audio.review_audio(&session.channel_login).await {
            Ok(Some(review)) => {
                append_audio_review(store, &session, review, Utc::now()).await?;
                process_pending_inputs(store, decider, session.session_id, Utc::now()).await?;
            }
            Ok(None) => {}
            Err(error) => {
                append_provider_error(store, &session, error, Utc::now()).await?;
            }
        }
    }
    Ok(())
}

async fn process_pending_inputs<D>(
    store: &CrewReviewStore,
    decider: &D,
    session_id: Uuid,
    now: DateTime<Utc>,
) -> Result<(), StoreError>
where
    D: ReviewDecider,
{
    let Some(claim) = store.pending_model_inputs(session_id).await? else {
        return Ok(());
    };
    let events = store.session_events(session_id).await?;
    let claim_id = claim.claim_id;
    let claim_until = claim.claim_until;
    for group in model_event_groups_by_cycle(&claim.events)? {
        let claim = ClaimedModelInputs {
            claim_id,
            claim_until,
            events: group.events,
        };
        let input = model_input(&events, &claim.events);
        match decider.decide_review(&input).await {
            Ok(decision) => complete_decision(store, claim, decision, now).await?,
            Err(error) => complete_provider_error(store, claim, error, now).await?,
        }
    }
    Ok(())
}

struct ModelEventGroup {
    cycle_id: Uuid,
    events: Vec<ReviewEvent>,
}

fn model_event_groups_by_cycle(events: &[ReviewEvent]) -> Result<Vec<ModelEventGroup>, StoreError> {
    let mut groups: Vec<ModelEventGroup> = Vec::new();
    for event in events {
        let cycle_id = event_cycle_id(event)?;
        if let Some(group) = groups.iter_mut().find(|group| group.cycle_id == cycle_id) {
            group.events.push(event.clone());
        } else {
            groups.push(ModelEventGroup {
                cycle_id,
                events: vec![event.clone()],
            });
        }
    }
    Ok(groups)
}

fn model_input(events: &[ReviewEvent], pending: &[ReviewEvent]) -> ReviewModelInput {
    let pending_ids: std::collections::HashSet<i64> =
        pending.iter().map(|event| event.id).collect();
    ReviewModelInput {
        ricky_messages: pending
            .iter()
            .filter(|event| event.event_kind == ReviewEventKind::RickyMessage)
            .filter_map(|event| event.content.clone())
            .collect(),
        streamer_transcripts: pending
            .iter()
            .filter(|event| event.event_kind == ReviewEventKind::StreamerTranscript)
            .filter_map(|event| event.content.clone())
            .collect(),
        previous_decisions: events
            .iter()
            .filter(|event| !pending_ids.contains(&event.id))
            .filter(|event| {
                matches!(
                    event.event_kind,
                    ReviewEventKind::AiDecision
                        | ReviewEventKind::AiDraft
                        | ReviewEventKind::ProviderError
                )
            })
            .map(|event| {
                event
                    .content
                    .clone()
                    .unwrap_or_else(|| event.metadata.to_string())
            })
            .collect(),
        session_state: events.first().map_or_else(
            || json!({}),
            |event| {
                json!({
                    "session_id": event.session_id.to_string(),
                    "channel_login": event.channel_login,
                })
            },
        ),
    }
}

async fn complete_decision(
    store: &CrewReviewStore,
    claim: ClaimedModelInputs,
    decision: ReviewDecision,
    occurred_at: DateTime<Utc>,
) -> Result<(), StoreError> {
    let Some(first) = claim.events.first() else {
        return Err(StoreError::InvalidClaim);
    };
    let cycle_id = event_cycle_id(first)?;
    let draft = decision.draft.as_ref().map(|draft| NewReviewEvent {
        session_id: first.session_id,
        channel_login: first.channel_login.clone(),
        subject_twitch_user_id: first.subject_twitch_user_id.clone(),
        event_kind: ReviewEventKind::AiDraft,
        source_message_id: None,
        occurred_at,
        content: Some(draft.clone()),
        metadata: json!({"cycle_id": cycle_id.to_string()}),
        provider: Some("fireworks".to_owned()),
        model: Some(FIREWORKS_DEFAULT_MODEL.to_owned()),
        confidence: Some(decision.confidence),
    });
    let terminal = NewReviewEvent {
        session_id: first.session_id,
        channel_login: first.channel_login.clone(),
        subject_twitch_user_id: first.subject_twitch_user_id.clone(),
        event_kind: ReviewEventKind::AiDecision,
        source_message_id: None,
        occurred_at,
        content: None,
        metadata: json!({
            "cycle_id": cycle_id.to_string(),
            "action": decision.action,
            "topic_active": decision.topic_active,
            "reason": decision.reason,
            "used_fact_ids": decision.used_fact_ids,
        }),
        provider: Some("fireworks".to_owned()),
        model: Some(FIREWORKS_DEFAULT_MODEL.to_owned()),
        confidence: Some(decision.confidence),
    };
    store
        .complete_claimed_model_cycle(claim.claim_id, draft, terminal)
        .await
}

async fn complete_provider_error(
    store: &CrewReviewStore,
    claim: ClaimedModelInputs,
    error: ReviewError,
    occurred_at: DateTime<Utc>,
) -> Result<(), StoreError> {
    let Some(first) = claim.events.first() else {
        return Err(StoreError::InvalidClaim);
    };
    let cycle_id = event_cycle_id(first)?;
    let terminal = NewReviewEvent {
        session_id: first.session_id,
        channel_login: first.channel_login.clone(),
        subject_twitch_user_id: first.subject_twitch_user_id.clone(),
        event_kind: ReviewEventKind::ProviderError,
        source_message_id: None,
        occurred_at,
        content: None,
        metadata: json!({"cycle_id": cycle_id.to_string(), "error_class": error.code()}),
        provider: Some("fireworks".to_owned()),
        model: Some(FIREWORKS_DEFAULT_MODEL.to_owned()),
        confidence: None,
    };
    store
        .complete_claimed_model_cycle(claim.claim_id, None, terminal)
        .await
}

async fn append_audio_review(
    store: &CrewReviewStore,
    session: &ReviewSession,
    review: AudioReview,
    occurred_at: DateTime<Utc>,
) -> Result<(), StoreError> {
    let text = review.text.trim();
    if text.is_empty() {
        return Ok(());
    }
    let event = NewReviewEvent {
        session_id: session.session_id,
        channel_login: session.channel_login.clone(),
        subject_twitch_user_id: session.subject_twitch_user_id.clone(),
        event_kind: ReviewEventKind::StreamerTranscript,
        source_message_id: None,
        occurred_at,
        content: Some(text.to_owned()),
        metadata: json!({
            "cycle_id": Uuid::new_v4().to_string(),
            "subject_mentioned": mentions_ricky(text),
            "duration_seconds": review.duration_seconds,
        }),
        provider: Some(review.provider),
        model: Some(review.model),
        confidence: None,
    };
    store.append_event(event).await.map(|_| ())
}

async fn append_provider_error(
    store: &CrewReviewStore,
    session: &ReviewSession,
    error: ProviderFailure,
    occurred_at: DateTime<Utc>,
) -> Result<(), StoreError> {
    let event = NewReviewEvent {
        session_id: session.session_id,
        channel_login: session.channel_login.clone(),
        subject_twitch_user_id: session.subject_twitch_user_id.clone(),
        event_kind: ReviewEventKind::ProviderError,
        source_message_id: None,
        occurred_at,
        content: None,
        metadata: json!({
            "cycle_id": Uuid::new_v4().to_string(),
            "error_class": error.error_class,
        }),
        provider: Some(error.provider.to_owned()),
        model: None,
        confidence: None,
    };
    store.append_event(event).await.map(|_| ())
}

async fn forward_discord_once(
    store: &CrewReviewStore,
    discord: &dyn DiscordBackend,
    limit: i64,
) -> Result<u64, String> {
    let cycles = store
        .pending_discord_cycles(limit)
        .await
        .map_err(|error| error.to_string())?;
    let mut sent = 0;
    for cycle in cycles {
        let cards = discord_payloads_for_cycles(std::slice::from_ref(&cycle));
        let mut sent_cards = Vec::new();
        for card in cards {
            let result = discord
                .send_rich_message(card.payload)
                .await
                .map_err(|error| error.to_string())?;
            let event_ids = card
                .cycles
                .into_iter()
                .flat_map(|cycle| cycle.event_ids)
                .collect();
            sent_cards.push(DiscordCard {
                event_ids,
                message_id: result.result.message_id,
            });
        }
        if !sent_cards.is_empty() {
            store
                .mark_discord_cards_sent(&sent_cards, cycle.claim_id)
                .await
                .map_err(|error| error.to_string())?;
            sent += sent_cards.len() as u64;
        }
    }
    Ok(sent)
}

struct PackedCycle {
    event_ids: Vec<i64>,
}

struct PackedDiscordCard {
    cycles: Vec<PackedCycle>,
    payload: SendRichMessage,
}

fn discord_payloads_for_cycles(
    cycles: &[tb_engagement::crew_review::ReviewCycle],
) -> Vec<PackedDiscordCard> {
    let mut cards = Vec::new();
    for cycle in cycles {
        cards.extend(discord_payloads_for_cycle(cycle));
    }
    cards
}

fn discord_payloads_for_cycle(
    cycle: &tb_engagement::crew_review::ReviewCycle,
) -> Vec<PackedDiscordCard> {
    let header = cycle_header(cycle);
    let mut cards = Vec::new();
    let mut current_text = header.clone();
    let mut current_event_ids = Vec::new();
    let mut has_event_text = false;
    let header_chars = header.chars().count();

    for event in &cycle.events {
        let segments = event_text_segments(event, header_chars);
        for (index, segment) in segments.into_iter().enumerate() {
            let separator_chars = usize::from(has_event_text || current_text == header);
            let would_len =
                current_text.chars().count() + separator_chars + segment.chars().count();
            if has_event_text && would_len > DISCORD_CARD_MAX_CHARS {
                cards.push(card_from_parts(
                    std::mem::replace(&mut current_text, header.clone()),
                    std::mem::take(&mut current_event_ids),
                ));
            }
            current_text.push('\n');
            current_text.push_str(&segment);
            if index == 0 {
                current_event_ids.push(event.id);
            }
            has_event_text = true;
        }
    }

    if has_event_text {
        cards.push(card_from_parts(current_text, current_event_ids));
    }
    cards
}

fn card_from_parts(content: String, event_ids: Vec<i64>) -> PackedDiscordCard {
    let components = json!([{
        "type": 17,
        "accent_color": RICKY_REVIEW_GOLD,
        "components": [
            {"type": 10, "content": content}
        ]
    }]);
    PackedDiscordCard {
        cycles: vec![PackedCycle { event_ids }],
        payload: SendRichMessage {
            channel_id: RICKY_REVIEW_CHANNEL_ID,
            content: None,
            embed: json!({}),
            components: Some(components),
            allowed_role_ids: vec![],
            view_spec: None,
        },
    }
}

fn cycle_header(cycle: &tb_engagement::crew_review::ReviewCycle) -> String {
    format!(
        "PLATZHALTER: Ricky-Review-Karte #{}",
        sanitize_discord_text(&cycle.channel_login)
    )
}

fn event_text_segments(event: &ReviewEvent, header_chars: usize) -> Vec<String> {
    let content = discord_event_content(event);
    if content.is_empty() {
        return vec![format!("PLATZHALTER: {}", event.event_kind.as_str())];
    }
    let first_prefix = format!("PLATZHALTER: {} ", event.event_kind.as_str());
    let next_prefix = format!(
        "PLATZHALTER: {} PLATZHALTER: Fortsetzung ",
        event.event_kind.as_str()
    );
    let mut chunks = Vec::new();
    let mut remaining = content.as_str();
    let mut first = true;
    while !remaining.is_empty() {
        let prefix = if first { &first_prefix } else { &next_prefix };
        let max_content = DISCORD_CARD_MAX_CHARS
            .saturating_sub(header_chars)
            .saturating_sub(1)
            .saturating_sub(prefix.chars().count())
            .max(1);
        let chunk: String = remaining.chars().take(max_content).collect();
        let chunk_len = chunk.len();
        chunks.push(format!("{prefix}{chunk}"));
        remaining = &remaining[chunk_len..];
        first = false;
    }
    chunks
}

fn discord_event_content(event: &ReviewEvent) -> String {
    if let Some(content) = event
        .content
        .as_deref()
        .filter(|content| !content.is_empty())
    {
        return sanitize_discord_text(content);
    }
    if event.event_kind == ReviewEventKind::AiDecision {
        let parts: Vec<String> = ["action", "topic_active", "reason", "used_fact_ids"]
            .into_iter()
            .filter_map(|key| {
                event
                    .metadata
                    .get(key)
                    .map(|value| format!("{key}={}", compact_metadata_value(value)))
            })
            .collect();
        if !parts.is_empty() {
            return sanitize_discord_text(&parts.join(" "));
        }
    }
    let fallback = event
        .metadata
        .get("error_class")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| compact_metadata_value(&event.metadata));
    sanitize_discord_text(&fallback)
}

async fn cleanup_once(
    store: &CrewReviewStore,
    discord: &dyn DiscordBackend,
    now: DateTime<Utc>,
) -> Result<(), String> {
    store
        .delete_expired_unposted(now)
        .await
        .map_err(|error| error.to_string())?;
    for group in store
        .expired_discord_groups(now, RETENTION_LIMIT)
        .await
        .map_err(|error| error.to_string())?
    {
        let delete = discord
            .delete_message(DeleteMessage {
                channel_id: RICKY_REVIEW_CHANNEL_ID,
                message_id: group.discord_message_id.clone(),
                reason: "PLATZHALTER: Ricky-Review-Retention".to_owned(),
            })
            .await;
        match delete {
            Ok(()) => {
                store
                    .delete_expired_group(&group.discord_message_id)
                    .await
                    .map_err(|error| error.to_string())?;
            }
            Err(error) => {
                store
                    .tombstone_group(&group.discord_message_id, &discord_error_class(&error))
                    .await
                    .map_err(|error| error.to_string())?;
            }
        }
    }
    Ok(())
}

fn event_cycle_id(event: &ReviewEvent) -> Result<Uuid, StoreError> {
    event
        .metadata
        .get("cycle_id")
        .and_then(Value::as_str)
        .ok_or(StoreError::InvalidMetadata)
        .and_then(|value| Uuid::parse_str(value).map_err(|_| StoreError::InvalidMetadata))
}

fn capture_failure(error: CaptureError) -> ProviderFailure {
    ProviderFailure::new("audio_capture", error)
}

fn transcribe_failure(error: TranscribeError) -> ProviderFailure {
    ProviderFailure::new("openai_transcribe", error)
}

fn discord_error_class(error: &DiscordError) -> String {
    match error {
        DiscordError::BrokerError { status, .. } => format!("discord_status_{status}"),
        DiscordError::Http(_) => "discord_http".to_owned(),
        DiscordError::Deserialize(_) => "discord_decode".to_owned(),
    }
}

fn bounded_error_class(value: &str) -> String {
    let bounded: String = value.trim().chars().take(64).collect();
    if bounded.is_empty() {
        "unknown".to_owned()
    } else {
        bounded
    }
}

fn mentions_ricky(text: &str) -> bool {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .any(|word| word == "ricky" || word == "helmbombenricky")
}

fn sanitize_discord_text(value: &str) -> String {
    value
        .replace("<@", "<\u{200B}@")
        .replace('@', "@\u{200B}")
        .chars()
        .filter(|character| *character != '\r')
        .collect()
}

fn compact_metadata_value(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Array(values) => values
            .iter()
            .map(compact_metadata_value)
            .collect::<Vec<_>>()
            .join(","),
        _ => value.to_string(),
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
    use std::str::FromStr;
    use std::sync::{Arc, Mutex};

    use chrono::{TimeZone, Utc};
    use serde_json::json;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use sqlx::types::Uuid;
    use sqlx::PgPool;
    use tb_engagement::crew_review::{
        NewReviewEvent, ReviewAction, ReviewDecision, ReviewError, ReviewEventKind, RickyChatInput,
        RICKY_TWITCH_USER_ID,
    };
    use tb_engagement::crew_review_store::CrewReviewStore;
    use tb_transport_discord::backend::SendResultInner;
    use tb_transport_discord::{
        DeleteMessage, DiscordBackend, DiscordError, EditRichMessage, SendAlertEmbed, SendResult,
        SendRichMessage, SendUserDm,
    };

    const MIGRATION: &str =
        include_str!("../../../migrations/20260717120000_twitch_crew_review_events.sql");

    #[derive(Clone)]
    struct FakeDecider {
        result: Arc<Mutex<Result<ReviewDecision, ReviewError>>>,
        inputs: Arc<Mutex<Vec<tb_engagement::crew_review::ReviewModelInput>>>,
    }

    impl FakeDecider {
        fn new(result: Result<ReviewDecision, ReviewError>) -> Self {
            Self {
                result: Arc::new(Mutex::new(result)),
                inputs: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    #[async_trait::async_trait]
    impl super::ReviewDecider for FakeDecider {
        async fn decide_review(
            &self,
            input: &tb_engagement::crew_review::ReviewModelInput,
        ) -> Result<ReviewDecision, ReviewError> {
            self.inputs.lock().unwrap().push(input.clone());
            self.result.lock().unwrap().clone()
        }
    }

    struct NoopAudio;

    #[async_trait::async_trait]
    impl super::AudioReviewer for NoopAudio {
        async fn review_audio(
            &self,
            _channel_login: &str,
        ) -> Result<Option<super::AudioReview>, super::ProviderFailure> {
            Ok(None)
        }
    }

    #[derive(Default)]
    struct FakeDiscord {
        sends: Mutex<Vec<SendRichMessage>>,
        deletes: Mutex<Vec<String>>,
        delete_result: Mutex<Option<Result<(), DiscordError>>>,
        pool_seen_before_delete: Mutex<Option<PgPool>>,
        expire_discord_claims_after_send: Mutex<Option<PgPool>>,
    }

    #[async_trait::async_trait]
    impl DiscordBackend for FakeDiscord {
        async fn send_rich_message(
            &self,
            payload: SendRichMessage,
        ) -> Result<SendResult, DiscordError> {
            self.sends.lock().unwrap().push(payload);
            let message_no = self.sends.lock().unwrap().len();
            let expire_pool = self
                .expire_discord_claims_after_send
                .lock()
                .unwrap()
                .clone();
            if let Some(pool) = expire_pool {
                sqlx::query(
                    "UPDATE twitch_crew_review_events
                        SET discord_claim_until = NOW() - INTERVAL '1 second'
                      WHERE discord_claim_id IS NOT NULL",
                )
                .execute(&pool)
                .await
                .unwrap();
            }
            Ok(SendResult {
                ok: true,
                result: SendResultInner {
                    message_id: format!("discord-{message_no}"),
                },
            })
        }

        async fn edit_rich_message(&self, _payload: EditRichMessage) -> Result<(), DiscordError> {
            Ok(())
        }

        async fn delete_message(&self, payload: DeleteMessage) -> Result<(), DiscordError> {
            let pool_seen_before_delete = self.pool_seen_before_delete.lock().unwrap().clone();
            if let Some(pool) = pool_seen_before_delete {
                let rows: i64 =
                    sqlx::query_scalar("SELECT COUNT(*) FROM twitch_crew_review_events")
                        .fetch_one(&pool)
                        .await
                        .unwrap();
                assert!(rows > 0, "DB-Zeilen wurden vor Discord geloescht");
            }
            self.deletes.lock().unwrap().push(payload.message_id);
            self.delete_result.lock().unwrap().take().unwrap_or(Ok(()))
        }

        async fn send_user_dm(&self, _payload: SendUserDm) -> Result<SendResult, DiscordError> {
            unreachable!()
        }

        async fn send_alert_embed(
            &self,
            _payload: SendAlertEmbed,
        ) -> Result<SendResult, DiscordError> {
            unreachable!()
        }

        async fn remove_member_role(
            &self,
            _guild_id: u64,
            _user_id: u64,
            _role_id: u64,
            _reason: &str,
        ) -> Result<(), DiscordError> {
            unreachable!()
        }
    }

    async fn test_pool(schema: &str) -> Option<PgPool> {
        let dsn = match std::env::var("TB_TEST_DATABASE_URL") {
            Ok(dsn) => dsn,
            Err(_) if std::env::var("TB_TEST_REQUIRE_DB").as_deref() == Ok("1") => {
                panic!("TB_TEST_DATABASE_URL fehlt trotz TB_TEST_REQUIRE_DB=1")
            }
            Err(_) => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return None;
            }
        };
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .expect("connect test postgres");
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .expect("drop test schema");
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .expect("create test schema");
        admin.close().await;

        let options = PgConnectOptions::from_str(&dsn)
            .expect("parse test postgres URL")
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await
            .expect("connect isolated test schema");
        sqlx::raw_sql(MIGRATION)
            .execute(&pool)
            .await
            .expect("apply crew review migration");
        Some(pool)
    }

    fn ricky_input(channel: &str, msg: &str) -> RickyChatInput {
        ricky_input_with_content(channel, msg, "Ricky fragt nach Discord")
    }

    fn ricky_input_with_content(channel: &str, msg: &str, content: &str) -> RickyChatInput {
        RickyChatInput {
            channel_login: channel.to_owned(),
            subject_twitch_user_id: RICKY_TWITCH_USER_ID.to_owned(),
            source_message_id: Some(msg.to_owned()),
            occurred_at: Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 0).unwrap(),
            content: content.to_owned(),
        }
    }

    fn reply_decision() -> ReviewDecision {
        ReviewDecision {
            action: ReviewAction::Reply,
            topic_active: true,
            confidence: 0.9,
            used_fact_ids: vec!["community_ban_2026_05_29".to_owned()],
            reason: "fact_based_reply".to_owned(),
            draft: Some("Ricky wurde aus der Deutschen Deadlock Community entfernt.".to_owned()),
        }
    }

    async fn session_id_for(pool: &PgPool, message_id: &str) -> Uuid {
        sqlx::query_scalar(
            "SELECT review_session_id
               FROM twitch_crew_review_events
              WHERE source_message_id = $1",
        )
        .bind(message_id)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    fn event(
        session_id: Uuid,
        channel: &str,
        kind: ReviewEventKind,
        occurred_at: chrono::DateTime<Utc>,
    ) -> NewReviewEvent {
        event_for_cycle(session_id, channel, kind, occurred_at, Uuid::new_v4())
    }

    fn event_for_cycle(
        session_id: Uuid,
        channel: &str,
        kind: ReviewEventKind,
        occurred_at: chrono::DateTime<Utc>,
        cycle_id: Uuid,
    ) -> NewReviewEvent {
        NewReviewEvent {
            session_id,
            channel_login: channel.to_owned(),
            subject_twitch_user_id: RICKY_TWITCH_USER_ID.to_owned(),
            event_kind: kind,
            source_message_id: None,
            occurred_at,
            content: Some("PLATZHALTER: Testinhalt @everyone <@42>".to_owned()),
            metadata: json!({"cycle_id": cycle_id.to_string()}),
            provider: Some("test".to_owned()),
            model: Some("test".to_owned()),
            confidence: Some(0.7),
        }
    }

    fn review_event(
        id: i64,
        session_id: Uuid,
        channel: &str,
        kind: ReviewEventKind,
        content: Option<String>,
        metadata: serde_json::Value,
    ) -> tb_engagement::crew_review::ReviewEvent {
        tb_engagement::crew_review::ReviewEvent {
            id,
            session_id,
            channel_login: channel.to_owned(),
            subject_twitch_user_id: RICKY_TWITCH_USER_ID.to_owned(),
            event_kind: kind,
            source_message_id: None,
            occurred_at: Utc::now(),
            content,
            metadata,
            provider: None,
            model: None,
            confidence: None,
            discord_message_id: None,
            discord_deleted_at: None,
            last_delete_error: None,
            tombstoned_at: None,
            created_at: Utc::now(),
            expires_at: Utc::now(),
        }
    }

    fn review_cycle(
        claim_id: Uuid,
        channel: &str,
        events: Vec<tb_engagement::crew_review::ReviewEvent>,
    ) -> tb_engagement::crew_review::ReviewCycle {
        tb_engagement::crew_review::ReviewCycle {
            cycle_id: Uuid::new_v4(),
            session_id: Uuid::new_v4(),
            channel_login: channel.to_owned(),
            claim_id,
            claim_until: Utc::now(),
            events,
        }
    }

    fn payload_text(payload: &SendRichMessage) -> String {
        payload.components.as_ref().unwrap()[0]["components"][0]["content"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    #[tokio::test]
    async fn trigger_bis_draft_schreibt_db_vor_discord() {
        let Some(pool) = test_pool("ricky_review_full_cycle").await else {
            return;
        };
        let store = CrewReviewStore::new(pool.clone());
        store
            .record_trigger(&ricky_input("nani", "msg-1"))
            .await
            .unwrap();

        let decider = FakeDecider::new(Ok(reply_decision()));
        super::process_once(
            &store,
            &decider,
            &NoopAudio,
            Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 1).unwrap(),
        )
        .await
        .unwrap();

        let rows: Vec<String> = sqlx::query_scalar(
            "SELECT event_kind
               FROM twitch_crew_review_events
              WHERE event_kind IN ('ricky_message', 'ai_draft', 'ai_decision')
              ORDER BY id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(rows, vec!["ricky_message", "ai_draft", "ai_decision"]);
        let posted_before_forward: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM twitch_crew_review_events WHERE discord_message_id IS NOT NULL",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(posted_before_forward, 0);

        let discord = FakeDiscord::default();
        super::forward_discord_once(&store, &discord, 10)
            .await
            .unwrap();
        assert_eq!(discord.sends.lock().unwrap().len(), 1);
        let posted_after_forward: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM twitch_crew_review_events WHERE discord_message_id = 'discord-1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(posted_after_forward, 4);
    }

    #[tokio::test]
    async fn providerfehler_schreibt_error_und_keinen_draft() {
        let Some(pool) = test_pool("ricky_review_provider_error").await else {
            return;
        };
        let store = CrewReviewStore::new(pool.clone());
        store
            .record_trigger(&ricky_input("nani", "msg-err"))
            .await
            .unwrap();

        let decider = FakeDecider::new(Err(ReviewError::Timeout));
        super::process_once(
            &store,
            &decider,
            &NoopAudio,
            Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 1).unwrap(),
        )
        .await
        .unwrap();

        let rows: Vec<String> =
            sqlx::query_scalar("SELECT event_kind FROM twitch_crew_review_events ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert!(rows.contains(&"provider_error".to_owned()));
        assert!(!rows.contains(&"ai_draft".to_owned()));
    }

    #[tokio::test]
    async fn mehrere_offene_modellcycles_werden_getrennt_ohne_reclaim_delay_verarbeitet() {
        let Some(pool) = test_pool("ricky_review_split_model_cycles").await else {
            return;
        };
        let store = CrewReviewStore::new(pool.clone());
        let mut first = ricky_input_with_content("model-split", "msg-model-a", "Ricky A");
        first.occurred_at = Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 0).unwrap();
        let first_cycle = store.record_trigger(&first).await.unwrap().unwrap();
        let mut second = ricky_input_with_content("model-split", "msg-model-b", "Ricky B");
        second.occurred_at = Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 1).unwrap();
        let second_cycle = store.record_trigger(&second).await.unwrap().unwrap();
        let session_id = session_id_for(&pool, "msg-model-a").await;
        let decider = FakeDecider::new(Ok(reply_decision()));

        super::process_pending_inputs(
            &store,
            &decider,
            session_id,
            Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 2).unwrap(),
        )
        .await
        .unwrap();

        let inputs = decider.inputs.lock().unwrap().clone();
        assert_eq!(inputs.len(), 2);
        assert_eq!(inputs[0].ricky_messages, vec!["Ricky A"]);
        assert_eq!(inputs[1].ricky_messages, vec!["Ricky B"]);
        let terminal_counts: Vec<(String, i64)> = sqlx::query_as(
            "SELECT metadata->>'cycle_id', COUNT(*)
               FROM twitch_crew_review_events
              WHERE event_kind = 'ai_decision'
              GROUP BY metadata->>'cycle_id'
              ORDER BY metadata->>'cycle_id'",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        let mut expected = vec![(first_cycle.to_string(), 1), (second_cycle.to_string(), 1)];
        expected.sort();
        assert_eq!(terminal_counts, expected);
    }

    #[test]
    fn claimed_model_inputs_werden_deterministisch_pro_cycle_getrennt() {
        let session_id = Uuid::new_v4();
        let first_cycle = Uuid::new_v4();
        let second_cycle = Uuid::new_v4();
        let events = vec![
            review_event(
                1,
                session_id,
                "model-groups",
                ReviewEventKind::RickyMessage,
                Some("Ricky A".to_owned()),
                json!({"cycle_id": first_cycle.to_string()}),
            ),
            review_event(
                2,
                session_id,
                "model-groups",
                ReviewEventKind::StreamerTranscript,
                Some("Transcript B".to_owned()),
                json!({"cycle_id": second_cycle.to_string()}),
            ),
            review_event(
                3,
                session_id,
                "model-groups",
                ReviewEventKind::StreamerTranscript,
                Some("Transcript A".to_owned()),
                json!({"cycle_id": first_cycle.to_string()}),
            ),
        ];

        let groups = super::model_event_groups_by_cycle(&events).unwrap();

        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].cycle_id, first_cycle);
        assert_eq!(
            groups[0]
                .events
                .iter()
                .map(|event| event.id)
                .collect::<Vec<_>>(),
            vec![1, 3]
        );
        assert_eq!(groups[1].cycle_id, second_cycle);
        assert_eq!(
            groups[1]
                .events
                .iter()
                .map(|event| event.id)
                .collect::<Vec<_>>(),
            vec![2]
        );
    }

    #[test]
    fn components_v2_karte_hat_gold_und_keine_mentions() {
        let cycle = tb_engagement::crew_review::ReviewCycle {
            cycle_id: Uuid::new_v4(),
            session_id: Uuid::new_v4(),
            channel_login: "nani".to_owned(),
            claim_id: Uuid::new_v4(),
            claim_until: Utc::now(),
            events: vec![tb_engagement::crew_review::ReviewEvent {
                id: 7,
                session_id: Uuid::new_v4(),
                channel_login: "nani".to_owned(),
                subject_twitch_user_id: RICKY_TWITCH_USER_ID.to_owned(),
                event_kind: ReviewEventKind::AiDraft,
                source_message_id: None,
                occurred_at: Utc::now(),
                content: Some("Hallo @everyone <@42>".to_owned()),
                metadata: json!({"cycle_id": Uuid::new_v4().to_string()}),
                provider: None,
                model: None,
                confidence: None,
                discord_message_id: None,
                discord_deleted_at: None,
                last_delete_error: None,
                tombstoned_at: None,
                created_at: Utc::now(),
                expires_at: Utc::now(),
            }],
        };

        let payloads = super::discord_payloads_for_cycles(&[cycle]);
        assert_eq!(payloads.len(), 1);
        let payload = &payloads[0].payload;
        assert_eq!(payload.channel_id, super::RICKY_REVIEW_CHANNEL_ID);
        assert_eq!(payload.content, None);
        assert_eq!(payload.embed, json!({}));
        assert!(payload.allowed_role_ids.is_empty());
        let components = payload.components.as_ref().unwrap();
        assert_eq!(components[0]["type"], 17);
        assert_eq!(components[0]["accent_color"], 0xC8A86B);
        assert_eq!(components[0]["components"][0]["type"], 10);
        let rendered = serde_json::to_string(components).unwrap();
        assert!(!rendered.contains("@everyone"));
        assert!(!rendered.contains("<@"));
    }

    #[test]
    fn discord_payloads_trennen_einen_cycle_an_eventgrenzen_ohne_inhaltverlust() {
        let claim_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();
        let cycle_id = Uuid::new_v4();
        let long = "7".repeat(super::DISCORD_CARD_MAX_CHARS - 160);
        let tail = "8".repeat(240);
        let cycle = review_cycle(
            claim_id,
            "split-events",
            vec![
                review_event(
                    11,
                    session_id,
                    "split-events",
                    ReviewEventKind::AiDraft,
                    Some(long.clone()),
                    json!({"cycle_id": cycle_id.to_string()}),
                ),
                review_event(
                    12,
                    session_id,
                    "split-events",
                    ReviewEventKind::ProviderError,
                    Some(tail.clone()),
                    json!({"cycle_id": cycle_id.to_string()}),
                ),
            ],
        );

        let payloads = super::discord_payloads_for_cycles(&[cycle]);

        assert_eq!(
            payloads
                .iter()
                .map(|payload| {
                    payload
                        .cycles
                        .iter()
                        .flat_map(|cycle| cycle.event_ids.clone())
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>(),
            vec![vec![11], vec![12]]
        );
        let rendered = payloads
            .iter()
            .map(|payload| payload_text(&payload.payload))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(rendered.matches('7').count(), long.len());
        assert_eq!(rendered.matches('8').count(), tail.len());
    }

    #[test]
    fn discord_payloads_teilen_ein_uebergrosses_event_verlustfrei_und_ordnen_id_einmal_zu() {
        let claim_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();
        let cycle_id = Uuid::new_v4();
        let content = "7".repeat(super::DISCORD_CARD_MAX_CHARS * 2 + 123);
        let cycle = review_cycle(
            claim_id,
            "split-one-event",
            vec![review_event(
                21,
                session_id,
                "split-one-event",
                ReviewEventKind::AiDecision,
                Some(content.clone()),
                json!({"cycle_id": cycle_id.to_string()}),
            )],
        );

        let payloads = super::discord_payloads_for_cycles(&[cycle]);

        assert!(payloads.len() > 1);
        assert_eq!(
            payloads
                .iter()
                .flat_map(|payload| {
                    payload
                        .cycles
                        .iter()
                        .flat_map(|cycle| cycle.event_ids.clone())
                })
                .collect::<Vec<_>>(),
            vec![21]
        );
        let rendered = payloads
            .iter()
            .map(|payload| payload_text(&payload.payload))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(rendered.matches('7').count(), content.len());
    }

    #[test]
    fn contentlose_ai_decision_zeigt_discord_relevante_metadaten_ohne_mentions() {
        let claim_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();
        let cycle_id = Uuid::new_v4();
        let cycle = review_cycle(
            claim_id,
            "decision-meta",
            vec![review_event(
                31,
                session_id,
                "decision-meta",
                ReviewEventKind::AiDecision,
                None,
                json!({
                    "cycle_id": cycle_id.to_string(),
                    "action": "reply",
                    "topic_active": true,
                    "reason": "Grund @everyone <@42>",
                    "used_fact_ids": ["fact_a", "fact_b"],
                }),
            )],
        );

        let payloads = super::discord_payloads_for_cycles(&[cycle]);
        let rendered = payload_text(&payloads[0].payload);

        assert!(rendered.contains("reply"));
        assert!(rendered.contains("true"));
        assert!(rendered.contains("Grund"));
        assert!(rendered.contains("fact_a"));
        assert!(rendered.contains("fact_b"));
        assert!(!rendered.contains("@everyone"));
        assert!(!rendered.contains("<@"));
    }

    #[tokio::test]
    async fn forward_discord_markiert_mehrkarten_cycle_atomar() {
        let Some(pool) = test_pool("ricky_review_forward_multicard").await else {
            return;
        };
        let store = CrewReviewStore::new(pool.clone());
        store
            .record_trigger(&ricky_input("forward-multi", "msg-forward-multi"))
            .await
            .unwrap();
        let session_id = session_id_for(&pool, "msg-forward-multi").await;
        let cycle_id = Uuid::new_v4();
        let first_id = store
            .append_event(event_for_cycle(
                session_id,
                "forward-multi",
                ReviewEventKind::AiDraft,
                Utc::now(),
                cycle_id,
            ))
            .await
            .unwrap();
        let second_id = store
            .append_event(event_for_cycle(
                session_id,
                "forward-multi",
                ReviewEventKind::AiDecision,
                Utc::now() + chrono::Duration::seconds(1),
                cycle_id,
            ))
            .await
            .unwrap();
        sqlx::query(
            "UPDATE twitch_crew_review_events
                SET content = CASE
                    WHEN id = $1 THEN $3
                    WHEN id = $2 THEN $4
                    ELSE content
                END
              WHERE id = ANY($5::bigint[])",
        )
        .bind(first_id)
        .bind(second_id)
        .bind("A".repeat(super::DISCORD_CARD_MAX_CHARS - 160))
        .bind("B".repeat(240))
        .bind(vec![first_id, second_id])
        .execute(&pool)
        .await
        .unwrap();
        let discord = FakeDiscord::default();

        super::forward_discord_once(&store, &discord, 10)
            .await
            .unwrap();

        assert_eq!(discord.sends.lock().unwrap().len(), 2);
        let posted: Vec<(i64, Option<String>)> = sqlx::query_as(
            "SELECT id, discord_message_id
               FROM twitch_crew_review_events
              WHERE id = ANY($1::bigint[])
              ORDER BY id",
        )
        .bind(vec![first_id, second_id])
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            posted,
            vec![
                (first_id, Some("discord-1".to_owned())),
                (second_id, Some("discord-2".to_owned())),
            ]
        );
    }

    #[tokio::test]
    async fn forward_discord_teilt_uebergrosses_einzelevent_und_markiert_event_einmal() {
        let Some(pool) = test_pool("ricky_review_forward_single_oversize").await else {
            return;
        };
        let store = CrewReviewStore::new(pool.clone());
        store
            .record_trigger(&ricky_input("forward-single", "msg-forward-single"))
            .await
            .unwrap();
        let session_id = session_id_for(&pool, "msg-forward-single").await;
        let cycle_id = Uuid::new_v4();
        let event_id = store
            .append_event(event_for_cycle(
                session_id,
                "forward-single",
                ReviewEventKind::AiDecision,
                Utc::now(),
                cycle_id,
            ))
            .await
            .unwrap();
        let content = "7".repeat(super::DISCORD_CARD_MAX_CHARS * 2 + 123);
        sqlx::query("UPDATE twitch_crew_review_events SET content = $1 WHERE id = $2")
            .bind(&content)
            .bind(event_id)
            .execute(&pool)
            .await
            .unwrap();
        let discord = FakeDiscord::default();

        let sent = super::forward_discord_once(&store, &discord, 10)
            .await
            .unwrap();

        assert!(sent > 1);
        let (send_count, rendered) = {
            let sends = discord.sends.lock().unwrap();
            (
                sends.len(),
                sends
                    .iter()
                    .map(payload_text)
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
        };
        assert_eq!(send_count as u64, sent);
        assert_eq!(rendered.matches('7').count(), content.len());
        let posted: Option<String> = sqlx::query_scalar(
            "SELECT discord_message_id FROM twitch_crew_review_events WHERE id = $1",
        )
        .bind(event_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(posted, Some("discord-1".to_owned()));
        assert!(store.pending_discord_cycles(10).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn forward_discord_markierungsfehler_laesst_cycle_retrybar_unmarkiert() {
        let Some(pool) = test_pool("ricky_review_forward_mark_fail").await else {
            return;
        };
        let store = CrewReviewStore::new(pool.clone());
        store
            .record_trigger(&ricky_input("forward-fail", "msg-forward-fail"))
            .await
            .unwrap();
        let session_id = session_id_for(&pool, "msg-forward-fail").await;
        let cycle_id = Uuid::new_v4();
        let first_id = store
            .append_event(event_for_cycle(
                session_id,
                "forward-fail",
                ReviewEventKind::AiDraft,
                Utc::now(),
                cycle_id,
            ))
            .await
            .unwrap();
        let second_id = store
            .append_event(event_for_cycle(
                session_id,
                "forward-fail",
                ReviewEventKind::AiDecision,
                Utc::now() + chrono::Duration::seconds(1),
                cycle_id,
            ))
            .await
            .unwrap();
        sqlx::query(
            "UPDATE twitch_crew_review_events
                SET content = CASE
                    WHEN id = $1 THEN $3
                    WHEN id = $2 THEN $4
                    ELSE content
                END
              WHERE id = ANY($5::bigint[])",
        )
        .bind(first_id)
        .bind(second_id)
        .bind("A".repeat(super::DISCORD_CARD_MAX_CHARS - 160))
        .bind("B".repeat(240))
        .bind(vec![first_id, second_id])
        .execute(&pool)
        .await
        .unwrap();
        let discord = FakeDiscord::default();
        *discord.expire_discord_claims_after_send.lock().unwrap() = Some(pool.clone());

        let result = super::forward_discord_once(&store, &discord, 10).await;

        assert!(result.is_err());
        assert_eq!(discord.sends.lock().unwrap().len(), 2);
        let posted: Vec<Option<String>> = sqlx::query_scalar(
            "SELECT discord_message_id
               FROM twitch_crew_review_events
              WHERE id = ANY($1::bigint[])
              ORDER BY id",
        )
        .bind(vec![first_id, second_id])
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(posted, vec![None, None]);
        let retry = store.pending_discord_cycles(10).await.unwrap();
        assert_eq!(retry.len(), 1);
        assert_eq!(
            retry[0]
                .events
                .iter()
                .map(|event| event.id)
                .collect::<Vec<_>>(),
            vec![first_id, second_id]
        );
    }

    #[tokio::test]
    async fn cleanup_loescht_discord_vor_db() {
        let Some(pool) = test_pool("ricky_review_cleanup_order").await else {
            return;
        };
        let store = CrewReviewStore::new(pool.clone());
        store
            .record_trigger(&ricky_input("cleanup", "msg-clean"))
            .await
            .unwrap();
        let session_id = session_id_for(&pool, "msg-clean").await;
        let event_id = store
            .append_event(event(
                session_id,
                "cleanup",
                ReviewEventKind::ProviderError,
                Utc::now() - chrono::Duration::days(200),
            ))
            .await
            .unwrap();
        sqlx::query(
            "UPDATE twitch_crew_review_events
                SET discord_message_id = 'discord-clean',
                    expires_at = NOW() - INTERVAL '1 second'
              WHERE id = $1",
        )
        .bind(event_id)
        .execute(&pool)
        .await
        .unwrap();

        let discord = FakeDiscord::default();
        *discord.pool_seen_before_delete.lock().unwrap() = Some(pool.clone());
        super::cleanup_once(&store, &discord, Utc::now())
            .await
            .unwrap();
        assert_eq!(
            discord.deletes.lock().unwrap().as_slice(),
            ["discord-clean"]
        );
        let remaining: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM twitch_crew_review_events WHERE id = $1")
                .bind(event_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(remaining, 0);
    }

    #[tokio::test]
    async fn cleanup_tombstoned_bei_discord_ausfall() {
        let Some(pool) = test_pool("ricky_review_cleanup_tombstone").await else {
            return;
        };
        let store = CrewReviewStore::new(pool.clone());
        store
            .record_trigger(&ricky_input("tombstone", "msg-tombstone"))
            .await
            .unwrap();
        let session_id = session_id_for(&pool, "msg-tombstone").await;
        let event_id = store
            .append_event(event(
                session_id,
                "tombstone",
                ReviewEventKind::ProviderError,
                Utc::now() - chrono::Duration::days(200),
            ))
            .await
            .unwrap();
        sqlx::query(
            "UPDATE twitch_crew_review_events
                SET discord_message_id = 'discord-fail',
                    expires_at = NOW() - INTERVAL '1 second'
              WHERE id = $1",
        )
        .bind(event_id)
        .execute(&pool)
        .await
        .unwrap();

        let discord = FakeDiscord::default();
        *discord.delete_result.lock().unwrap() = Some(Err(DiscordError::BrokerError {
            status: 500,
            body: "boom".to_owned(),
        }));
        super::cleanup_once(&store, &discord, Utc::now())
            .await
            .unwrap();

        let row: (Option<String>, Option<chrono::DateTime<Utc>>) = sqlx::query_as(
            "SELECT content, tombstoned_at FROM twitch_crew_review_events WHERE id = $1",
        )
        .bind(event_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.0, None);
        assert!(row.1.is_some());
    }
}
