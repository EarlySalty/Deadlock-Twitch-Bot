use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use sqlx::types::Uuid;
use sqlx::PgPool;
use tb_chat::types::ChatMessageEvent;
use tb_config::BrokerConfig;
use tb_engagement::audio_capture::{CaptureError, MemoryAudioCapturer};
use tb_engagement::crew_review::{
    ClaimedModelInputs, CrewReviewTrigger, FireworksReviewClient, NewReviewEvent, ReviewDecision,
    ReviewError, ReviewEvent, ReviewEventKind, ReviewModelInput, ReviewSession, RickyChatInput,
    FIREWORKS_DEFAULT_MODEL, RICKY_TWITCH_USER_ID,
};
use tb_engagement::crew_review_store::{CrewReviewStore, DiscordCard, StoreError};
use tb_engagement::transcribe::{OpenAiTranscriber, TranscribeError};
use tb_monitoring::{ChatNotificationKind, EventSubHooks};
use tb_transport_discord::{
    BrokerRelay, DeleteMessage, DiscordBackend, DiscordError, SendRichMessage,
};

use crate::task_supervisor::TaskSupervisor;

const PROCESS_INTERVAL: Duration = Duration::from_secs(5);
const DISCORD_FORWARD_INTERVAL: Duration = Duration::from_secs(60);
const RETENTION_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const DISCORD_BATCH_LIMIT: i64 = 20;
const MODEL_SESSION_BATCH_LIMIT: i64 = 100;
const RETENTION_LIMIT: i64 = 50;
const TEXT_DISPLAY_MAX_CHARS: usize = 3_500;
const MAX_TEXT_DISPLAYS_PER_CARD: usize = 39;
const MAX_EVENT_TEXT_DISPLAYS_PER_CARD: usize = MAX_TEXT_DISPLAYS_PER_CARD - 1;
const DEFAULT_REVIEW_CHANNEL_ID: i64 = 1_374_364_800_817_303_632;
const RICKY_REVIEW_GOLD: i64 = 0xC8A86B;
const RETENTION_DELETE_REASON: &str = "Ricky-Review: Aufbewahrungsfrist abgelaufen";
const AUDIO_SEGMENT_SECONDS: u64 = 20;

#[derive(Clone, Debug, Eq, PartialEq)]
struct ReviewConfig {
    enabled: bool,
    channel_id: i64,
    segment_seconds: u64,
}

impl ReviewConfig {
    fn from_env() -> Self {
        let mut enabled = env_bool("RICKY_SHADOW_REVIEW_ENABLED").unwrap_or(false);
        let mut channel_id = DEFAULT_REVIEW_CHANNEL_ID;
        let segment_seconds = AUDIO_SEGMENT_SECONDS;

        if let Some(raw) = nonempty_env("RICKY_SHADOW_REVIEW_CHANNEL_ID") {
            match raw.parse::<i64>() {
                Ok(value) if value > 0 => channel_id = value,
                _ => {
                    enabled = false;
                    tracing::error!(
                        setting = "RICKY_SHADOW_REVIEW_CHANNEL_ID",
                        value = %raw,
                        "Ricky-Review fail-closed: ungueltige Discord-Channel-ID"
                    );
                }
            }
        }

        if let Some(raw) = nonempty_env("RICKY_SHADOW_REVIEW_SEGMENT_SECONDS") {
            match raw.parse::<u64>() {
                Ok(AUDIO_SEGMENT_SECONDS) => {}
                _ => {
                    enabled = false;
                    tracing::error!(
                        setting = "RICKY_SHADOW_REVIEW_SEGMENT_SECONDS",
                        value = %raw,
                        "Ricky-Review fail-closed: ungueltige Segmentdauer"
                    );
                }
            }
        }

        Self {
            enabled,
            channel_id,
            segment_seconds,
        }
    }
}

#[derive(Clone)]
pub struct RickyReviewRuntime {
    trigger: Arc<dyn CrewReviewTrigger>,
    store: CrewReviewStore,
}

impl RickyReviewRuntime {
    pub fn trigger(&self) -> Arc<dyn CrewReviewTrigger> {
        Arc::clone(&self.trigger)
    }

    pub fn store(&self) -> CrewReviewStore {
        self.store.clone()
    }

    pub async fn close_all_open_sessions(&self, reason: &str) {
        if let Err(error) = self.store.close_all_open_sessions(reason, Utc::now()).await {
            tracing::warn!(%error, reason, "Ricky-Review: Session-Close fehlgeschlagen");
        }
    }
}

pub fn wrap_eventsub_hooks(
    inner: Arc<dyn EventSubHooks>,
    trigger: Arc<dyn CrewReviewTrigger>,
    store: CrewReviewStore,
) -> Arc<dyn EventSubHooks> {
    Arc::new(RickyReviewEventSubHooks {
        inner,
        trigger,
        store,
    })
}

struct RickyReviewEventSubHooks {
    inner: Arc<dyn EventSubHooks>,
    trigger: Arc<dyn CrewReviewTrigger>,
    store: CrewReviewStore,
}

#[async_trait::async_trait]
impl EventSubHooks for RickyReviewEventSubHooks {
    async fn on_channel_raid(&self, event: &Value, message_id: Option<&str>) {
        self.inner.on_channel_raid(event, message_id).await;
    }

    async fn on_channel_moderate(&self, broadcaster_id: &str, login: &str, event: &Value) {
        self.inner
            .on_channel_moderate(broadcaster_id, login, event)
            .await;
    }

    async fn on_stream_went_live(&self, twitch_user_id: &str, login: &str) {
        self.inner.on_stream_went_live(twitch_user_id, login).await;
    }

    async fn on_stream_went_live_with_stream_id(
        &self,
        twitch_user_id: &str,
        login: &str,
        stream_id: Option<&str>,
    ) {
        self.inner
            .on_stream_went_live_with_stream_id(twitch_user_id, login, stream_id)
            .await;
    }

    async fn on_score_refresh(
        &self,
        twitch_user_id: &str,
        login: Option<&str>,
        trigger: &'static str,
    ) {
        self.inner
            .on_score_refresh(twitch_user_id, login, trigger)
            .await;
    }

    async fn on_stream_offline_engagement(&self, twitch_user_id: &str, login: Option<&str>) {
        self.inner
            .on_stream_offline_engagement(twitch_user_id, login)
            .await;
    }

    async fn on_stream_offline_global_ban(&self, twitch_user_id: &str, login: Option<&str>) {
        self.inner
            .on_stream_offline_global_ban(twitch_user_id, login)
            .await;
    }

    async fn on_stream_offline(&self, twitch_user_id: &str, login: Option<&str>) {
        self.inner.on_stream_offline(twitch_user_id, login).await;
        let Some(login) = login.map(str::trim).filter(|value| !value.is_empty()) else {
            return;
        };
        if let Err(error) = self
            .store
            .close_channel_session(login, "stream_offline", Utc::now())
            .await
        {
            tracing::warn!(%error, login, "Ricky-Review: EventSub-Offline-Close fehlgeschlagen");
        }
    }

    async fn on_chat_message(&self, event: &Value, message_id: Option<&str>) {
        match serde_json::from_value::<ChatMessageEvent>(event.clone()) {
            Ok(chat_event) if chat_event.chatter_user_id == RICKY_TWITCH_USER_ID => {
                let normalized = chat_event.with_effective_channel();
                let source_message_id = (!normalized.message_id.trim().is_empty())
                    .then(|| normalized.message_id.trim().to_owned())
                    .or_else(|| {
                        message_id
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                            .map(str::to_owned)
                    });
                self.trigger.observe(RickyChatInput {
                    channel_login: normalized.broadcaster_user_login.clone(),
                    subject_twitch_user_id: RICKY_TWITCH_USER_ID.to_owned(),
                    source_message_id,
                    occurred_at: Utc::now(),
                    content: normalized.text().to_owned(),
                });
            }
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(%error, "Ricky-Review: EventSub-Chat nicht deserialisierbar")
            }
        }
        self.inner.on_chat_message(event, message_id).await;
    }

    async fn on_chat_subscription_notification(
        &self,
        kind: ChatNotificationKind,
        event: &Value,
        message_id: Option<&str>,
    ) {
        self.inner
            .on_chat_subscription_notification(kind, event, message_id)
            .await;
    }

    async fn on_chat_raid_notification(&self, event: &Value, message_id: Option<&str>) {
        self.inner
            .on_chat_raid_notification(event, message_id)
            .await;
    }

    async fn on_chat_unraid_notification(&self, event: &Value, message_id: Option<&str>) {
        self.inner
            .on_chat_unraid_notification(event, message_id)
            .await;
    }
}

pub fn start(
    supervisor: &TaskSupervisor,
    pool: PgPool,
    broker: &BrokerConfig,
) -> RickyReviewRuntime {
    let config = ReviewConfig::from_env();
    let store = CrewReviewStore::new(pool);
    if !config.enabled {
        tracing::info!("Ricky-Review ist per Kill-Switch deaktiviert");
        return inactive_runtime(
            supervisor,
            store,
            "kill_switch",
            "ricky_review_kill_switch_close",
        );
    }

    let discord: Arc<dyn DiscordBackend> = match BrokerRelay::new(broker) {
        Ok(relay) => Arc::new(relay),
        Err(error) => {
            tracing::warn!(
                %error,
                "Ricky-Review bleibt deaktiviert: BrokerRelay nicht initialisierbar"
            );
            return inactive_runtime(
                supervisor,
                store,
                "discord_unavailable",
                "ricky_review_discord_unavailable_close",
            );
        }
    };

    start_enabled(supervisor, store, config, Some(discord))
}

fn start_enabled(
    supervisor: &TaskSupervisor,
    store: CrewReviewStore,
    config: ReviewConfig,
    discord: Option<Arc<dyn DiscordBackend>>,
) -> RickyReviewRuntime {
    let Some(discord) = discord else {
        return inactive_runtime(
            supervisor,
            store,
            "discord_unavailable",
            "ricky_review_discord_unavailable_close",
        );
    };

    let trigger: Arc<dyn CrewReviewTrigger> = Arc::new(PgRickyReviewTrigger {
        store: store.clone(),
        supervisor: supervisor.clone(),
    });
    spawn_processor(supervisor, store.clone(), config.segment_seconds);
    spawn_discord_forwarder(
        supervisor,
        store.clone(),
        Arc::clone(&discord),
        config.channel_id,
    );
    spawn_retention(supervisor, store.clone(), discord);
    tracing::info!(
        channel_id = config.channel_id,
        segment_seconds = config.segment_seconds,
        "Ricky-Review Shadow-Modus gestartet; Twitch-Ausgabe deaktiviert"
    );

    RickyReviewRuntime { trigger, store }
}

fn inactive_runtime(
    supervisor: &TaskSupervisor,
    store: CrewReviewStore,
    reason: &'static str,
    task_name: &'static str,
) -> RickyReviewRuntime {
    let close_store = store.clone();
    supervisor.spawn_finite(task_name, async move {
        if let Err(error) = close_store
            .close_all_open_sessions(reason, Utc::now())
            .await
        {
            tracing::warn!(%error, reason, "Ricky-Review Session-Close fehlgeschlagen");
        }
    });
    RickyReviewRuntime {
        trigger: Arc::new(NoopCrewReviewTrigger),
        store,
    }
}

struct NoopCrewReviewTrigger;

impl CrewReviewTrigger for NoopCrewReviewTrigger {
    fn observe(&self, _input: RickyChatInput) {}
}

struct PgRickyReviewTrigger {
    store: CrewReviewStore,
    supervisor: TaskSupervisor,
}

impl CrewReviewTrigger for PgRickyReviewTrigger {
    fn observe(&self, input: RickyChatInput) {
        let store = self.store.clone();
        self.supervisor
            .spawn_finite("ricky_review_trigger", async move {
                if let Err(error) = store.record_trigger(&input).await {
                    tracing::warn!(%error, "Ricky-Review-Trigger konnte nicht geschrieben werden");
                }
            });
    }
}

fn spawn_processor(supervisor: &TaskSupervisor, store: CrewReviewStore, segment_seconds: u64) {
    supervisor.spawn("ricky_review_processor", async move {
        if let Err(error) = store
            .close_all_open_sessions("process_start", Utc::now())
            .await
        {
            tracing::warn!(%error, "Ricky-Review: Startup-Close fehlgeschlagen");
        }
        let decider = LiveReviewDecider::from_env();
        let audio = LiveAudioReviewer::from_env(Duration::from_secs(segment_seconds));
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
    channel_id: i64,
) {
    supervisor.spawn("ricky_review_discord_forwarder", async move {
        let mut tick = tokio::time::interval(DISCORD_FORWARD_INTERVAL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            match forward_discord_once(&store, discord.as_ref(), DISCORD_BATCH_LIMIT, channel_id)
                .await
            {
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
    close_session_reason: Option<&'static str>,
}

impl ProviderFailure {
    fn new(provider: &'static str, error_class: impl std::fmt::Display) -> Self {
        Self {
            provider,
            error_class: bounded_error_class(&error_class.to_string()),
            close_session_reason: None,
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
    duration: Duration,
}

impl LiveAudioReviewer {
    fn from_env(duration: Duration) -> Self {
        Self {
            capturer: MemoryAudioCapturer::new(
                nonempty_env("RICKY_SHADOW_REVIEW_YTDLP_BIN")
                    .or_else(|| nonempty_env("YTDLP_BIN"))
                    .unwrap_or_else(|| crate::yt_dlp_path().to_string_lossy().into_owned()),
                nonempty_env("FFMPEG_BIN").unwrap_or_else(|| "ffmpeg".to_owned()),
            ),
            transcriber: OpenAiTranscriber::from_env(),
            duration,
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
            return Err(ProviderFailure::new("openai_transcribe", "unavailable"));
        };
        let wav = self
            .capturer
            .capture_wav(channel_login, self.duration)
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
    for session_id in store
        .pending_model_session_ids(MODEL_SESSION_BATCH_LIMIT)
        .await?
    {
        process_pending_inputs(store, decider, session_id, now).await?;
    }
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
                let close_session_reason = error.close_session_reason;
                let occurred_at = Utc::now();
                append_provider_error(store, &session, error, occurred_at).await?;
                if let Some(reason) = close_session_reason {
                    store
                        .close_channel_session(&session.channel_login, reason, occurred_at)
                        .await?;
                }
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
    let claim_id = claim.claim_id;
    let claim_until = claim.claim_until;
    for group in model_event_groups_by_cycle(&claim.events)? {
        let events = store.session_events(session_id).await?;
        let input = model_input(&events, &group.events);
        let cycle_claim = ClaimedModelInputs {
            claim_id,
            claim_until,
            events: group.events,
        };
        match decider.decide_review(&input).await {
            Ok(decision) => complete_decision(store, cycle_claim, decision, now).await?,
            Err(error) => complete_provider_error(store, cycle_claim, error, now).await?,
        }
    }
    Ok(())
}

struct ModelEventGroup {
    cycle_id: Uuid,
    events: Vec<ReviewEvent>,
}

fn model_event_groups_by_cycle(events: &[ReviewEvent]) -> Result<Vec<ModelEventGroup>, StoreError> {
    let mut groups = Vec::<ModelEventGroup>::new();
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
    let pending_ids = pending.iter().map(|event| event.id).collect::<HashSet<_>>();
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
    let text = review.text;
    let subject_mentioned = mentions_ricky(&text);
    let event = NewReviewEvent {
        session_id: session.session_id,
        channel_login: session.channel_login.clone(),
        subject_twitch_user_id: session.subject_twitch_user_id.clone(),
        event_kind: ReviewEventKind::StreamerTranscript,
        source_message_id: None,
        occurred_at,
        content: Some(text),
        metadata: json!({
            "cycle_id": Uuid::new_v4().to_string(),
            "subject_mentioned": subject_mentioned,
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
    channel_id: i64,
) -> Result<u64, String> {
    let cycles = store
        .pending_discord_cycles(limit)
        .await
        .map_err(|error| error.to_string())?;
    let mut sent = 0;
    for cycle in cycles {
        let cards = discord_payloads_for_cycle(&cycle, channel_id)?;
        let mut sent_cards = Vec::new();
        for card in cards {
            let result = discord
                .send_rich_message(card.payload)
                .await
                .map_err(safe_discord_error)?;
            sent_cards.push(DiscordCard {
                event_ids: card.event_ids,
                message_id: result.result.message_id,
            });
        }
        if !sent_cards.is_empty() {
            store
                .mark_discord_cards_sent(&sent_cards, cycle.claim_id, channel_id)
                .await
                .map_err(|error| error.to_string())?;
            sent += sent_cards.len() as u64;
        }
    }
    Ok(sent)
}

struct PackedDiscordCard {
    event_ids: Vec<i64>,
    payload: SendRichMessage,
}

#[cfg(test)]
fn discord_payloads_for_cycles(
    cycles: &[tb_engagement::crew_review::ReviewCycle],
    channel_id: i64,
) -> Result<Vec<PackedDiscordCard>, String> {
    let mut cards = Vec::new();
    for cycle in cycles {
        cards.extend(discord_payloads_for_cycle(cycle, channel_id)?);
    }
    Ok(cards)
}

fn discord_payloads_for_cycle(
    cycle: &tb_engagement::crew_review::ReviewCycle,
    channel_id: i64,
) -> Result<Vec<PackedDiscordCard>, String> {
    let mut pending = Vec::<(Vec<String>, i64)>::new();
    for event in &cycle.events {
        let displays = event_text_displays(event, cycle.cycle_id)?;
        if displays.len() > MAX_EVENT_TEXT_DISPLAYS_PER_CARD {
            return Err("single_event_exceeds_discord_component_capacity".to_owned());
        }
        pending.push((displays, event.id));
    }

    let mut raw_cards = Vec::<(Vec<String>, Vec<i64>)>::new();
    let mut current_components = Vec::new();
    let mut current_ids = Vec::new();
    for (displays, event_id) in pending {
        if !current_components.is_empty()
            && current_components.len() + displays.len() > MAX_EVENT_TEXT_DISPLAYS_PER_CARD
        {
            raw_cards.push((
                std::mem::take(&mut current_components),
                std::mem::take(&mut current_ids),
            ));
        }
        current_components.extend(displays);
        current_ids.push(event_id);
    }
    if !current_components.is_empty() {
        raw_cards.push((current_components, current_ids));
    }

    let card_count = raw_cards.len();
    raw_cards
        .into_iter()
        .enumerate()
        .map(|(index, (components, event_ids))| {
            if event_ids.is_empty() {
                return Err("empty_discord_card".to_owned());
            }
            Ok(card_from_components(
                channel_id,
                &cycle.channel_login,
                cycle.cycle_id,
                index,
                card_count,
                components,
                event_ids,
            ))
        })
        .collect()
}

fn event_text_displays(event: &ReviewEvent, cycle_id: Uuid) -> Result<Vec<String>, String> {
    let content = discord_event_content(event);
    let label = event_label(event.event_kind);
    let first_prefix = format!("**{label} · Event {}**\n", event.id);
    let next_prefix = format!(
        "**{label} (Fortsetzung) · Event {} · Zyklus {cycle_id}**\n",
        event.id
    );
    split_display_text(&first_prefix, &next_prefix, &content)
}

fn split_display_text(
    first_prefix: &str,
    next_prefix: &str,
    content: &str,
) -> Result<Vec<String>, String> {
    let mut remaining = content;
    let mut first = true;
    let mut chunks = Vec::new();
    loop {
        let prefix = if first { first_prefix } else { next_prefix };
        let max_content = TEXT_DISPLAY_MAX_CHARS
            .checked_sub(prefix.chars().count())
            .ok_or_else(|| "discord_prefix_too_large".to_owned())?;
        let take = remaining
            .char_indices()
            .nth(max_content)
            .map_or(remaining.len(), |(idx, _)| idx);
        let chunk = &remaining[..take];
        chunks.push(format!("{prefix}{chunk}"));
        remaining = &remaining[take..];
        if remaining.is_empty() {
            return Ok(chunks);
        }
        first = false;
    }
}

fn card_from_components(
    channel_id: i64,
    channel_login: &str,
    cycle_id: Uuid,
    card_index: usize,
    card_count: usize,
    mut displays: Vec<String>,
    event_ids: Vec<i64>,
) -> PackedDiscordCard {
    let channel_login = sanitize_discord_text(channel_login);
    displays.insert(
        0,
        format!(
            "**Ricky-Review · #{channel_login} · Zyklus {cycle_id} · Teil {}/{}**",
            card_index + 1,
            card_count
        ),
    );
    let components = json!([{
        "type": 17,
        "accent_color": RICKY_REVIEW_GOLD,
        "components": displays.into_iter().map(|content| json!({
            "type": 10,
            "content": content,
        })).collect::<Vec<_>>()
    }]);
    PackedDiscordCard {
        event_ids,
        payload: SendRichMessage {
            channel_id,
            content: None,
            embed: json!({}),
            components: Some(components),
            allowed_role_ids: vec![],
            view_spec: None,
        },
    }
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
        let mut parts = ["action", "topic_active", "reason", "used_fact_ids"]
            .into_iter()
            .filter_map(|key| {
                event
                    .metadata
                    .get(key)
                    .map(|value| format!("{key}={}", compact_metadata_value(value)))
            })
            .collect::<Vec<_>>();
        if let Some(confidence) = event.confidence {
            parts.push(format!("confidence={confidence:.3}"));
        }
        if !parts.is_empty() {
            return sanitize_discord_text(&parts.join(" "));
        }
    }
    if event.event_kind == ReviewEventKind::SessionStarted {
        return "Sitzung eröffnet.".to_owned();
    }
    if event.event_kind == ReviewEventKind::SessionEnded {
        let reason = event
            .metadata
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or("unbekannt");
        return sanitize_discord_text(&format!("Grund: {reason}"));
    }
    if event.event_kind == ReviewEventKind::ProviderError {
        let error_class = event
            .metadata
            .get("error_class")
            .and_then(Value::as_str)
            .unwrap_or("unbekannt");
        return sanitize_discord_text(&format!("Fehlerklasse: {error_class}"));
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
                channel_id: group.discord_channel_id,
                message_id: group.discord_message_id.clone(),
                reason: RETENTION_DELETE_REASON.to_owned(),
            })
            .await;
        match delete {
            Ok(()) => {
                store
                    .delete_expired_group(group.discord_channel_id, &group.discord_message_id)
                    .await
                    .map_err(|error| error.to_string())?;
            }
            Err(error) => {
                store
                    .tombstone_group(
                        group.discord_channel_id,
                        &group.discord_message_id,
                        &discord_error_class(&error),
                    )
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
    let close_session_reason =
        matches!(&error, CaptureError::SourceUnavailable).then_some("stream_unavailable");
    let mut failure = ProviderFailure::new("audio_capture", error);
    failure.close_session_reason = close_session_reason;
    failure
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

fn safe_discord_error(error: DiscordError) -> String {
    discord_error_class(&error)
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

fn event_label(kind: ReviewEventKind) -> &'static str {
    match kind {
        ReviewEventKind::SessionStarted => "Start",
        ReviewEventKind::RickyMessage => "Ricky",
        ReviewEventKind::StreamerTranscript => "Streamer",
        ReviewEventKind::AiDecision => "Entscheidung",
        ReviewEventKind::AiDraft => "Entwurf",
        ReviewEventKind::ProviderError => "Providerfehler",
        ReviewEventKind::SessionEnded => "Ende",
    }
}

fn env_bool(name: &str) -> Option<bool> {
    let value = nonempty_env(name)?;
    match value.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => {
            tracing::error!(
                setting = name,
                value = %value,
                "Ricky-Review fail-closed: ungueltiger Bool-Wert"
            );
            Some(false)
        }
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
    use super::*;
    use chrono::{TimeZone, Utc};
    use serde_json::json;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use sqlx::types::Uuid;
    use sqlx::PgPool;
    use std::str::FromStr;
    use std::sync::{Mutex, MutexGuard};
    use tb_engagement::crew_review::{
        ReviewAction, ReviewDecision, ReviewError, ReviewEvent, ReviewEventKind, RickyChatInput,
        RICKY_TWITCH_USER_ID,
    };
    use tb_engagement::crew_review_store::CrewReviewStore;
    use tb_monitoring::NoopEventSubHooks;

    const TABLE_MIGRATION: &str =
        include_str!("../../../migrations/20260717121000_twitch_crew_review_events.sql");
    const CHANNEL_MIGRATION: &str =
        include_str!("../../../migrations/20260717121500_twitch_crew_review_discord_channel.sql");
    const SERVICE_WRAPPER: &str = include_str!("../../../scripts/run_tb_bot_service.sh");
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn config_default_ist_disabled_mit_sicheren_defaults() {
        let _guard = EnvGuard::clear();

        let config = ReviewConfig::from_env();

        assert!(!config.enabled);
        assert_eq!(config.channel_id, DEFAULT_REVIEW_CHANNEL_ID);
        assert_eq!(config.segment_seconds, 20);
    }

    #[test]
    fn config_akzeptiert_nur_den_festen_20s_runtime_vertrag() {
        let _guard = EnvGuard::set(&[
            ("RICKY_SHADOW_REVIEW_ENABLED", "1"),
            ("RICKY_SHADOW_REVIEW_CHANNEL_ID", "42"),
            ("RICKY_SHADOW_REVIEW_SEGMENT_SECONDS", "20"),
        ]);

        let config = ReviewConfig::from_env();

        assert!(config.enabled);
        assert_eq!(config.channel_id, 42);
        assert_eq!(config.segment_seconds, 20);
    }

    #[test]
    fn config_weichende_segmentdauer_schaltet_fail_closed_ab() {
        let _guard = EnvGuard::set(&[
            ("RICKY_SHADOW_REVIEW_ENABLED", "1"),
            ("RICKY_SHADOW_REVIEW_SEGMENT_SECONDS", "7"),
        ]);

        let config = ReviewConfig::from_env();

        assert!(!config.enabled);
        assert_eq!(config.segment_seconds, 20);
    }

    #[test]
    fn config_invalid_fail_closed_ohne_panic() {
        let _guard = EnvGuard::set(&[
            ("RICKY_SHADOW_REVIEW_ENABLED", "1"),
            ("RICKY_SHADOW_REVIEW_CHANNEL_ID", "-1"),
            ("RICKY_SHADOW_REVIEW_SEGMENT_SECONDS", "120"),
        ]);

        let config = ReviewConfig::from_env();

        assert!(!config.enabled);
        assert_eq!(config.channel_id, DEFAULT_REVIEW_CHANNEL_ID);
        assert_eq!(config.segment_seconds, 20);
    }

    #[test]
    fn service_wrapper_aktiviert_shadow_review_mit_finalen_defaults() {
        for expected in [
            r#"export TB_SCOUT_ENABLED="${TB_SCOUT_ENABLED:-1}""#,
            r#"export TB_CHAT_ENABLED="${TB_CHAT_ENABLED:-0}""#,
            r#"export RICKY_SHADOW_REVIEW_ENABLED="${RICKY_SHADOW_REVIEW_ENABLED:-1}""#,
            r#"export RICKY_SHADOW_REVIEW_CHANNEL_ID="${RICKY_SHADOW_REVIEW_CHANNEL_ID:-1374364800817303632}""#,
            r#"export RICKY_SHADOW_REVIEW_SEGMENT_SECONDS="${RICKY_SHADOW_REVIEW_SEGMENT_SECONDS:-20}""#,
            r#"export FFMPEG_BIN="${FFMPEG_BIN:-/usr/bin/ffmpeg}""#,
            r#"export FIREWORKS_BASE_URL="${FIREWORKS_BASE_URL:-https://api.fireworks.ai/inference/v1}""#,
            r#"export TB_LLM_MODEL_RICKY_CREW_REVIEW="${TB_LLM_MODEL_RICKY_CREW_REVIEW:-accounts/fireworks/models/deepseek-v4-flash}""#,
        ] {
            assert!(SERVICE_WRAPPER.contains(expected), "fehlt: {expected}");
        }
        // Der Deploy-Baum hat kein venv: ein venv-Default überschreibt die
        // Pfadsuche des Bots mit einem Pfad, den es dort nicht gibt.
        assert!(
            !SERVICE_WRAPPER.contains(".venv/bin/yt-dlp"),
            "venv-Default für yt-dlp ist zurück"
        );
    }

    #[tokio::test]
    async fn eventsub_decorator_prueft_ohne_chat_runtime_nur_die_exakte_id() {
        let recorded = Arc::new(RecordingTrigger::default());
        let trigger: Arc<dyn CrewReviewTrigger> = recorded.clone();
        let lazy_pool = PgPoolOptions::new()
            .connect_lazy("postgres://test:test@127.0.0.1:1/test")
            .unwrap();
        let hooks = wrap_eventsub_hooks(
            Arc::new(NoopEventSubHooks),
            trigger,
            CrewReviewStore::new(lazy_pool),
        );
        let mut event = json!({
            "broadcaster_user_id": "channel-id",
            "broadcaster_user_login": "kanal",
            "chatter_user_id": RICKY_TWITCH_USER_ID,
            "chatter_user_login": "beliebiger-login",
            "message_id": "eventsub-ricky-1",
            "message": {"text": "Test von Ricky"}
        });

        hooks.on_chat_message(&event, Some("envelope-1")).await;
        event["chatter_user_id"] = json!("147713657");
        event["message_id"] = json!("eventsub-other-1");
        hooks.on_chat_message(&event, Some("envelope-2")).await;

        let inputs = recorded.inputs.lock().unwrap();
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].channel_login, "kanal");
        assert_eq!(inputs[0].subject_twitch_user_id, RICKY_TWITCH_USER_ID);
        assert_eq!(
            inputs[0].source_message_id.as_deref(),
            Some("eventsub-ricky-1")
        );
        assert_eq!(inputs[0].content, "Test von Ricky");
    }

    #[tokio::test]
    async fn eventsub_offline_beendet_session_auch_ohne_andere_hooks() {
        let Some(pool) = test_pool("ricky_review_eventsub_offline").await else {
            return;
        };
        let store = CrewReviewStore::new(pool.clone());
        store
            .record_trigger(&ricky_input("eventsub-offline", "offline-event", "Ricky"))
            .await
            .unwrap()
            .unwrap();
        let hooks = wrap_eventsub_hooks(
            Arc::new(NoopEventSubHooks),
            Arc::new(RecordingTrigger::default()),
            store,
        );

        hooks
            .on_stream_offline("channel-id", Some("eventsub-offline"))
            .await;

        let reasons: Vec<String> = sqlx::query_scalar(
            "SELECT metadata->>'reason'
               FROM twitch_crew_review_events
              WHERE event_kind = 'session_ended'",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(reasons, vec!["stream_offline"]);
    }

    #[tokio::test]
    async fn fehlender_discord_broker_startet_keine_aufzeichnung() {
        let Some(pool) = test_pool("ricky_review_broker_fail_closed").await else {
            return;
        };
        let supervisor = TaskSupervisor::start();
        let store = CrewReviewStore::new(pool.clone());
        let runtime = start_enabled(
            &supervisor,
            store,
            ReviewConfig {
                enabled: true,
                channel_id: DEFAULT_REVIEW_CHANNEL_ID,
                segment_seconds: 20,
            },
            None,
        );

        runtime
            .trigger()
            .observe(ricky_input("broker-fail", "broker-fail-1", "Ricky Test"));
        tokio::time::sleep(Duration::from_millis(25)).await;

        let event_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_crew_review_events")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(event_count, 0);
        supervisor.shutdown().await;
    }

    #[tokio::test]
    async fn fehlender_openai_transcriber_wird_als_providerfehler_erfasst() {
        let reviewer = LiveAudioReviewer {
            capturer: MemoryAudioCapturer::new("unused", "unused"),
            transcriber: None,
            duration: Duration::from_secs(20),
        };

        let error = match reviewer.review_audio("unused").await {
            Err(error) => error,
            Ok(_) => panic!("fehlender OpenAI-Client darf nicht still verschwinden"),
        };

        assert_eq!(error.provider, "openai_transcribe");
        assert_eq!(error.error_class, "unavailable");
    }

    #[test]
    fn discord_entscheidung_zeigt_die_gespeicherte_confidence() {
        let cycle_id = Uuid::new_v4();
        let mut event = review_event(10, cycle_id, ReviewEventKind::AiDecision, None);
        event.metadata = json!({
            "cycle_id": cycle_id.to_string(),
            "action": "reply",
            "topic_active": true,
            "reason": "fact_based_reply",
            "used_fact_ids": ["community_ban_2026_05_29"],
        });
        event.confidence = Some(0.875);

        let rendered = discord_event_content(&event);

        assert!(rendered.contains("confidence=0.875"));
    }

    #[test]
    fn einzelnes_ueberlanges_event_bleibt_eine_discord_nachricht() {
        let cycle_id = Uuid::new_v4();
        let content = "q".repeat(TEXT_DISPLAY_MAX_CHARS * 2 + 123);
        let cycle = review_cycle(
            cycle_id,
            vec![review_event(
                11,
                cycle_id,
                ReviewEventKind::AiDecision,
                Some(content.clone()),
            )],
        );

        let cards = discord_payloads_for_cycle(&cycle, DEFAULT_REVIEW_CHANNEL_ID).unwrap();

        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].event_ids, vec![11]);
        let components = cards[0].payload.components.as_ref().unwrap();
        let displays = components[0]["components"].as_array().unwrap();
        assert!(displays.len() > 1);
        assert!(displays.iter().all(|display| display["type"] == 10
            && display["content"].as_str().unwrap().chars().count() <= TEXT_DISPLAY_MAX_CHARS));
        assert_eq!(payload_text(&cards[0]).matches('q').count(), content.len());
    }

    #[test]
    fn einzelnes_event_ueber_components_kapazitaet_fail_closed() {
        let cycle_id = Uuid::new_v4();
        let content = "x".repeat(TEXT_DISPLAY_MAX_CHARS * MAX_TEXT_DISPLAYS_PER_CARD + 1);
        let cycle = review_cycle(
            cycle_id,
            vec![review_event(
                12,
                cycle_id,
                ReviewEventKind::AiDraft,
                Some(content),
            )],
        );

        assert!(discord_payloads_for_cycle(&cycle, DEFAULT_REVIEW_CHANNEL_ID).is_err());
    }

    #[test]
    fn event_boundary_multi_card_ohne_inhaltsverlust() {
        let cycle_id = Uuid::new_v4();
        let first = "~".repeat(128_000);
        let second = "|".repeat(240);
        let cycle = review_cycle(
            cycle_id,
            vec![
                review_event(21, cycle_id, ReviewEventKind::AiDraft, Some(first.clone())),
                review_event(
                    22,
                    cycle_id,
                    ReviewEventKind::ProviderError,
                    Some(second.clone()),
                ),
            ],
        );

        let cards = discord_payloads_for_cycle(&cycle, DEFAULT_REVIEW_CHANNEL_ID).unwrap();

        assert_eq!(
            cards
                .iter()
                .map(|card| card.event_ids.clone())
                .collect::<Vec<_>>(),
            vec![vec![21], vec![22]]
        );
        let rendered = cards.iter().map(payload_text).collect::<Vec<_>>().join("");
        assert_eq!(rendered.matches('~').count(), first.len());
        assert_eq!(rendered.matches('|').count(), second.len());
    }

    #[test]
    fn identische_cycle_payloads_bleiben_durch_header_eindeutig() {
        let first_cycle_id = Uuid::new_v4();
        let second_cycle_id = Uuid::new_v4();
        let first = review_cycle(
            first_cycle_id,
            vec![review_event(
                31,
                first_cycle_id,
                ReviewEventKind::AiDraft,
                Some("gleich".into()),
            )],
        );
        let second = review_cycle(
            second_cycle_id,
            vec![review_event(
                32,
                second_cycle_id,
                ReviewEventKind::AiDraft,
                Some("gleich".into()),
            )],
        );

        let payloads =
            discord_payloads_for_cycles(&[first, second], DEFAULT_REVIEW_CHANNEL_ID).unwrap();

        assert_eq!(payloads.len(), 2);
        assert_ne!(
            serde_json::to_string(&payloads[0].payload).unwrap(),
            serde_json::to_string(&payloads[1].payload).unwrap()
        );
        assert!(payload_text(&payloads[0]).contains(&first_cycle_id.to_string()));
        assert!(payload_text(&payloads[1]).contains(&second_cycle_id.to_string()));
    }

    #[test]
    fn discord_karte_hat_finale_deutsche_labels_und_neutralisiert_mentions() {
        let cycle_id = Uuid::new_v4();
        let cycle = review_cycle(
            cycle_id,
            vec![review_event(
                41,
                cycle_id,
                ReviewEventKind::AiDraft,
                Some("@everyone bitte <@123> prüfen".to_string()),
            )],
        );

        let cards = discord_payloads_for_cycle(&cycle, DEFAULT_REVIEW_CHANNEL_ID).unwrap();
        let rendered = payload_text(&cards[0]);

        assert!(rendered.contains(&format!(
            "Ricky-Review · #nani · Zyklus {cycle_id} · Teil 1/1"
        )));
        assert!(rendered.contains("Entwurf · Event 41"));
        assert!(!rendered.contains("PLATZHALTER"));
        assert!(!rendered.contains("@everyone"));
        assert!(!rendered.contains("<@"));
        assert_eq!(
            RETENTION_DELETE_REASON,
            "Ricky-Review: Aufbewahrungsfrist abgelaufen"
        );
    }

    #[test]
    fn discord_send_fehler_verwirft_den_broker_body() {
        let error = DiscordError::BrokerError {
            status: 502,
            body: "vertraulicher Review-Inhalt".to_string(),
        };

        let safe = safe_discord_error(error);

        assert_eq!(safe, "discord_status_502");
        assert!(!safe.contains("vertraulich"));
    }

    #[tokio::test]
    async fn mehrere_offene_modellcycles_frischen_history_pro_cycle() {
        let Some(pool) = test_pool("ricky_review_model_history").await else {
            return;
        };
        let store = CrewReviewStore::new(pool.clone());
        let first_cycle = store
            .record_trigger(&ricky_input("history", "m1", "Ricky eins"))
            .await
            .unwrap()
            .unwrap();
        let second_cycle = store
            .record_trigger(&ricky_input("history", "m2", "Ricky zwei"))
            .await
            .unwrap()
            .unwrap();
        let session_id = session_id_for(&pool, "m1").await;
        let decider = FakeDecider::default();

        process_pending_inputs(&store, &decider, session_id, Utc::now())
            .await
            .unwrap();

        let inputs = decider.inputs.lock().unwrap().clone();
        assert_eq!(inputs.len(), 2);
        assert_eq!(inputs[0].ricky_messages, vec!["Ricky eins"]);
        assert_eq!(inputs[1].ricky_messages, vec!["Ricky zwei"]);
        assert!(inputs[1]
            .previous_decisions
            .iter()
            .any(|decision| decision.contains("fact_based_reply")));
        let terminals: Vec<(String, i64)> = sqlx::query_as(
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
        assert_eq!(terminals, expected);
    }

    #[tokio::test]
    async fn geschlossene_session_wird_beim_naechsten_prozessorlauf_nachgezogen() {
        let Some(pool) = test_pool("ricky_review_closed_recovery").await else {
            return;
        };
        let store = CrewReviewStore::new(pool.clone());
        let now = Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 0).unwrap();
        store
            .record_trigger(&ricky_input("closed-recovery", "closed-1", "Ricky offen"))
            .await
            .unwrap()
            .unwrap();
        store
            .close_channel_session(
                "closed-recovery",
                "stream_offline",
                now + chrono::Duration::seconds(1),
            )
            .await
            .unwrap();

        process_once(
            &store,
            &FakeDecider::default(),
            &NoAudio,
            now + chrono::Duration::seconds(2),
        )
        .await
        .unwrap();

        let terminal_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM twitch_crew_review_events WHERE event_kind = 'ai_decision'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(terminal_count, 1);
    }

    #[tokio::test]
    async fn nicht_verfuegbarer_stream_beendet_die_review_session() {
        let Some(pool) = test_pool("ricky_review_stream_unavailable").await else {
            return;
        };
        let store = CrewReviewStore::new(pool.clone());
        let now = Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 0).unwrap();
        store
            .record_trigger(&ricky_input("offline", "offline-1", "Ricky offen"))
            .await
            .unwrap()
            .unwrap();

        process_once(
            &store,
            &FakeDecider::default(),
            &UnavailableAudio,
            now + chrono::Duration::seconds(1),
        )
        .await
        .unwrap();

        let reasons: Vec<String> = sqlx::query_scalar(
            "SELECT metadata->>'reason'
               FROM twitch_crew_review_events
              WHERE event_kind = 'session_ended'",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(reasons, vec!["stream_unavailable"]);
    }

    #[derive(Default)]
    struct FakeDecider {
        inputs: Mutex<Vec<tb_engagement::crew_review::ReviewModelInput>>,
    }

    struct NoAudio;

    struct UnavailableAudio;

    #[derive(Default)]
    struct RecordingTrigger {
        inputs: Mutex<Vec<RickyChatInput>>,
    }

    impl CrewReviewTrigger for RecordingTrigger {
        fn observe(&self, input: RickyChatInput) {
            self.inputs.lock().unwrap().push(input);
        }
    }

    #[async_trait::async_trait]
    impl AudioReviewer for NoAudio {
        async fn review_audio(
            &self,
            _channel_login: &str,
        ) -> Result<Option<AudioReview>, ProviderFailure> {
            Ok(None)
        }
    }

    #[async_trait::async_trait]
    impl AudioReviewer for UnavailableAudio {
        async fn review_audio(
            &self,
            _channel_login: &str,
        ) -> Result<Option<AudioReview>, ProviderFailure> {
            Err(capture_failure(CaptureError::SourceUnavailable))
        }
    }

    #[async_trait::async_trait]
    impl ReviewDecider for FakeDecider {
        async fn decide_review(
            &self,
            input: &tb_engagement::crew_review::ReviewModelInput,
        ) -> Result<ReviewDecision, ReviewError> {
            self.inputs.lock().unwrap().push(input.clone());
            Ok(ReviewDecision {
                action: ReviewAction::Reply,
                topic_active: true,
                confidence: 0.9,
                used_fact_ids: vec!["community_ban_2026_05_29".to_string()],
                reason: "fact_based_reply".to_string(),
                draft: Some("Testentscheidung".to_string()),
            })
        }
    }

    async fn test_pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
        let options = PgConnectOptions::from_str(&dsn)
            .unwrap()
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await
            .unwrap();
        sqlx::raw_sql(TABLE_MIGRATION).execute(&pool).await.unwrap();
        sqlx::raw_sql(CHANNEL_MIGRATION)
            .execute(&pool)
            .await
            .unwrap();
        Some(pool)
    }

    fn ricky_input(channel: &str, id: &str, content: &str) -> RickyChatInput {
        RickyChatInput {
            channel_login: channel.to_string(),
            subject_twitch_user_id: RICKY_TWITCH_USER_ID.to_string(),
            source_message_id: Some(id.to_string()),
            occurred_at: Utc.with_ymd_and_hms(2026, 7, 17, 12, 0, 0).unwrap(),
            content: content.to_string(),
        }
    }

    async fn session_id_for(pool: &PgPool, source_message_id: &str) -> Uuid {
        sqlx::query_scalar(
            "SELECT review_session_id
               FROM twitch_crew_review_events
              WHERE source_message_id = $1",
        )
        .bind(source_message_id)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    fn review_cycle(
        cycle_id: Uuid,
        events: Vec<ReviewEvent>,
    ) -> tb_engagement::crew_review::ReviewCycle {
        tb_engagement::crew_review::ReviewCycle {
            cycle_id,
            session_id: Uuid::new_v4(),
            channel_login: "nani".to_string(),
            claim_id: Uuid::new_v4(),
            claim_until: Utc::now(),
            events,
        }
    }

    fn review_event(
        id: i64,
        cycle_id: Uuid,
        event_kind: ReviewEventKind,
        content: Option<String>,
    ) -> ReviewEvent {
        ReviewEvent {
            id,
            session_id: Uuid::new_v4(),
            channel_login: "nani".to_string(),
            subject_twitch_user_id: RICKY_TWITCH_USER_ID.to_string(),
            event_kind,
            source_message_id: None,
            occurred_at: Utc::now(),
            content,
            metadata: json!({"cycle_id": cycle_id.to_string()}),
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

    fn payload_text(card: &PackedDiscordCard) -> String {
        card.payload.components.as_ref().unwrap()[0]["components"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|component| component["content"].as_str())
            .collect()
    }

    struct EnvGuard {
        snapshot: Vec<(&'static str, Option<String>)>,
        _lock: MutexGuard<'static, ()>,
    }

    impl EnvGuard {
        fn clear() -> Self {
            Self::set(&[])
        }

        fn set(values: &[(&'static str, &'static str)]) -> Self {
            let lock = ENV_LOCK.lock().unwrap();
            let names = [
                "RICKY_SHADOW_REVIEW_ENABLED",
                "RICKY_SHADOW_REVIEW_CHANNEL_ID",
                "RICKY_SHADOW_REVIEW_SEGMENT_SECONDS",
            ];
            let snapshot = names
                .iter()
                .map(|name| (*name, std::env::var(name).ok()))
                .collect::<Vec<_>>();
            for name in names {
                std::env::remove_var(name);
            }
            for (name, value) in values {
                std::env::set_var(name, value);
            }
            Self {
                snapshot,
                _lock: lock,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (name, value) in &self.snapshot {
                match value {
                    Some(value) => std::env::set_var(name, value),
                    None => std::env::remove_var(name),
                }
            }
        }
    }
}
