use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;
use tb_config::BrokerConfig;
use tb_engagement::audio_capture::AudioCapturer;
use tb_engagement::background::capture_transcript_segment;
use tb_engagement::smalltalk_loop_store::{
    ClaimedReport, SmalltalkLoopStore, SmalltalkReport, SmalltalkTranscript, StoreError,
};
use tb_engagement::stream_transcripts::StreamTranscripts;
use tb_engagement::transcribe::OpenAiTranscriber;
use tb_transport_discord::{
    BrokerRelay, DeleteMessage, DiscordBackend, DiscordError, SendRichMessage,
};

use crate::task_supervisor::TaskSupervisor;

const LOOP_INTERVAL: Duration = Duration::from_secs(5);
/// Takt der Ton-Aufnahme. Ein Block dauert selbst schon
/// `ENGAGEMENT_TRANSCRIPT_CAPTURE_SECONDS`, der Takt ist also nur die Pause
/// zwischen zwei Bloecken und die Wartezeit, bis eine neue Sitzung aufgegriffen
/// wird.
const TRANSCRIPT_INTERVAL: Duration = Duration::from_secs(5);
const DISCORD_INTERVAL: Duration = Duration::from_secs(30);
const RETENTION_INTERVAL: Duration = Duration::from_secs(60 * 60);
/// Wie weit ein Ton-Abschnitt vor einer Nachricht liegen darf, um in der
/// Auswertung als ihr Kontext zu gelten.
const TRANSCRIPT_CONTEXT_WINDOW: chrono::Duration = chrono::Duration::minutes(3);
const DISCORD_BATCH_LIMIT: i64 = 20;
const RETENTION_BATCH_LIMIT: i64 = 20;
const DEFAULT_REVIEW_GUILD_ID: i64 = 1_289_721_245_281_292_288;
const DEFAULT_REVIEW_CHANNEL_ID: i64 = 1_374_364_800_817_303_632;
const SMALLTALK_GOLD: i64 = 0xC8A86B;
/// Discord begrenzt die Komponenten eines Components-V2-Containers. Der Rest
/// des Repos rechnet mit 39 Text-Displays pro Karte (`ricky_review_wiring.rs`),
/// hier gilt dasselbe: eine Stunde Testmodus kann deutlich mehr Nachrichten
/// erzeugen, und ein zu grosser Post scheitert, laeuft nach drei Versuchen aus
/// und der Report verschwindet genau dann, wenn es am meisten zu sehen gaebe.
const MAX_TEXT_DISPLAYS_PER_CARD: usize = 39;

/// Beschriftungen der Smalltalk-Auswertung. Fehlt ein Feld, bleibt die
/// Discord-Auslieferung fail-closed aus.
const SMALLTALK_DISCORD_COPY_JSON: &str = r#"{
  "title": "Smalltalk-Testauswertung",
  "channel": "Kanal",
  "session": "Sitzung",
  "duration": "Laufzeit",
  "viewers": "Zuschauer",
  "end_reason": "Endgrund",
  "end_session_timeout": "Sitzungsdauer erreicht",
  "end_stream_ended": "Stream beendet",
  "end_process_start": "Offene Sitzung beim Prozessstart zurückgesetzt",
  "end_process_shutdown": "Prozess beendet",
  "end_kill_switch": "Smalltalk-Loop ausgeschaltet",
  "end_provider_error": "Provider-Fehler",
  "generated": "Erzeugte Nachrichten",
  "would_send": "Würde senden",
  "rejected": "Verworfen",
  "rejection_reasons": "Verworfen nach Grund",
  "provider_errors": "Provider-Fehler",
  "last_provider_error": "Letzter Provider-Fehler",
  "transcripts": "Stream-Ton (lokal transkribiert)",
  "transcripts_missing": "nicht erfasst",
  "transcript_context": "Stream davor",
  "trigger": "Auslöser",
  "generated_text": "Erzeugter Text",
  "result": "Ergebnis",
  "result_would_send": "Würde senden",
  "result_rejected": "Verworfen",
  "truncated_note": "Weitere Nachrichten dieser Sitzung stehen nur in der Datenbank",
  "reason_dash": "Gedankenstrich",
  "reason_quote": "Anführungszeichen",
  "reason_list": "Aufzählung",
  "reason_repeated_punctuation": "Satzzeichenfolge",
  "reason_too_long": "Mehr als 120 Zeichen",
  "reason_offer_or_link": "Angebot oder Link",
  "reason_empty": "Nach Bereinigung leer",
  "not_available": "nicht verfügbar",
  "retention_delete_reason": "Aufbewahrungsfrist abgelaufen"
}"#;

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SmalltalkDeliveryTarget {
    Database,
    Discord,
}

#[cfg(test)]
impl SmalltalkDeliveryTarget {
    const ALL: [Self; 2] = [Self::Database, Self::Discord];
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SmalltalkConfig {
    enabled: bool,
}

impl SmalltalkConfig {
    fn from_env() -> Self {
        let raw = std::env::var("SMALLTALK_LOOP_ENABLED").ok();
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
    viewers: String,
    end_reason: String,
    end_session_timeout: String,
    end_stream_ended: String,
    end_process_start: String,
    end_process_shutdown: String,
    end_kill_switch: String,
    end_provider_error: String,
    generated: String,
    would_send: String,
    rejected: String,
    rejection_reasons: String,
    provider_errors: String,
    last_provider_error: String,
    transcripts: String,
    transcripts_missing: String,
    transcript_context: String,
    trigger: String,
    generated_text: String,
    result: String,
    result_would_send: String,
    result_rejected: String,
    truncated_note: String,
    reason_dash: String,
    reason_quote: String,
    reason_list: String,
    reason_repeated_punctuation: String,
    reason_too_long: String,
    reason_offer_or_link: String,
    reason_empty: String,
    not_available: String,
    retention_delete_reason: String,
}

impl DiscordCopy {
    fn configured() -> Option<Self> {
        let copy = serde_json::from_str::<Self>(SMALLTALK_DISCORD_COPY_JSON).ok()?;
        [
            &copy.title,
            &copy.channel,
            &copy.session,
            &copy.duration,
            &copy.viewers,
            &copy.end_reason,
            &copy.end_session_timeout,
            &copy.end_stream_ended,
            &copy.end_process_start,
            &copy.end_process_shutdown,
            &copy.end_kill_switch,
            &copy.end_provider_error,
            &copy.generated,
            &copy.would_send,
            &copy.rejected,
            &copy.rejection_reasons,
            &copy.provider_errors,
            &copy.last_provider_error,
            &copy.transcripts,
            &copy.transcripts_missing,
            &copy.transcript_context,
            &copy.trigger,
            &copy.generated_text,
            &copy.result,
            &copy.result_would_send,
            &copy.result_rejected,
            &copy.truncated_note,
            &copy.reason_dash,
            &copy.reason_quote,
            &copy.reason_list,
            &copy.reason_repeated_punctuation,
            &copy.reason_too_long,
            &copy.reason_offer_or_link,
            &copy.reason_empty,
            &copy.not_available,
            &copy.retention_delete_reason,
        ]
        .iter()
        .all(|value| !value.trim().is_empty())
        .then_some(copy)
    }

    #[cfg(test)]
    fn test_copy() -> Self {
        serde_json::from_str(SMALLTALK_DISCORD_COPY_JSON).expect("Test-Copy ist vollständig")
    }

    fn reason_label<'a>(&'a self, reason: &'a str) -> &'a str {
        match reason {
            "dash" => &self.reason_dash,
            "quote" => &self.reason_quote,
            "list" => &self.reason_list,
            "repeated_punctuation" => &self.reason_repeated_punctuation,
            "too_long" => &self.reason_too_long,
            "offer_or_link" => &self.reason_offer_or_link,
            "empty" => &self.reason_empty,
            _ => reason,
        }
    }

    fn end_reason_label<'a>(&'a self, reason: &'a str) -> &'a str {
        match reason {
            "session_timeout" => &self.end_session_timeout,
            "stream_ended" => &self.end_stream_ended,
            "process_start" => &self.end_process_start,
            "process_shutdown" => &self.end_process_shutdown,
            "kill_switch" => &self.end_kill_switch,
            "provider_error" => &self.end_provider_error,
            _ => reason,
        }
    }
}

#[derive(Clone)]
pub struct SmalltalkLoopRuntime {
    store: SmalltalkLoopStore,
}

impl SmalltalkLoopRuntime {
    pub async fn close_open_session(&self, reason: &str) {
        if let Err(error) = self.store.close_all_open_sessions(reason, Utc::now()).await {
            tracing::warn!(event = "smalltalk_loop.session_close_failed", %error, reason);
        }
    }
}

pub fn start(
    supervisor: &TaskSupervisor,
    pool: PgPool,
    broker: &BrokerConfig,
) -> SmalltalkLoopRuntime {
    let store = SmalltalkLoopStore::new(pool.clone());
    let config = SmalltalkConfig::from_env();
    let copy = DiscordCopy::configured();
    let discord: Option<Arc<dyn DiscordBackend>> = match BrokerRelay::new(broker) {
        Ok(relay) => Some(Arc::new(relay)),
        Err(error) => {
            tracing::warn!(event = "smalltalk_loop.discord_unavailable", %error);
            None
        }
    };

    if let (Some(copy), Some(discord)) = (copy.clone(), discord.clone()) {
        spawn_retention(
            supervisor,
            store.clone(),
            Arc::clone(&discord),
            copy.clone(),
        );
        // Beendete Sitzungen werden auch bei ausgeschaltetem Loop ausgeliefert.
        // Sonst bliebe gerade der wichtige Startup-Reset nach einem Absturz
        // unsichtbar.
        spawn_discord_forwarder(
            supervisor,
            store.clone(),
            discord,
            DEFAULT_REVIEW_CHANNEL_ID,
            copy,
        );
    }
    if !config.enabled {
        tracing::info!(event = "smalltalk_loop.disabled");
        return inactive_runtime(supervisor, store, "kill_switch");
    }
    let Some(_) = copy else {
        tracing::warn!(event = "smalltalk_loop.discord_copy_missing");
        return inactive_runtime(supervisor, store, "discord_copy_missing");
    };
    let Some(_) = discord else {
        return inactive_runtime(supervisor, store, "discord_unavailable");
    };

    spawn_loop(supervisor, store.clone());
    spawn_transcript_capture(supervisor, store.clone(), pool);
    tracing::info!(
        event = "smalltalk_loop.started",
        guild_id = DEFAULT_REVIEW_GUILD_ID,
        channel_id = DEFAULT_REVIEW_CHANNEL_ID,
    );
    SmalltalkLoopRuntime { store }
}

fn inactive_runtime(
    supervisor: &TaskSupervisor,
    store: SmalltalkLoopStore,
    reason: &'static str,
) -> SmalltalkLoopRuntime {
    let close_store = store.clone();
    supervisor.spawn_finite("smalltalk_loop_inactive_close", async move {
        if let Err(error) = close_store
            .close_all_open_sessions(reason, Utc::now())
            .await
        {
            tracing::warn!(event = "smalltalk_loop.inactive_close_failed", %error, reason);
        }
    });
    SmalltalkLoopRuntime { store }
}

fn spawn_loop(supervisor: &TaskSupervisor, store: SmalltalkLoopStore) {
    supervisor.spawn("smalltalk_loop", async move {
        if let Err(error) = store
            .close_all_open_sessions("process_start", Utc::now())
            .await
        {
            tracing::warn!(event = "smalltalk_loop.startup_close_failed", %error);
        }
        let mut tick = tokio::time::interval(LOOP_INTERVAL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            if let Err(error) = process_once(&store, Utc::now()).await {
                tracing::warn!(event = "smalltalk_loop.process_failed", %error);
            }
        }
    });
}

/// Nimmt den Stream der laufenden Sitzung in Bloecken auf und transkribiert ihn
/// lokal.
///
/// Zwei Ablagen, ein Mitschnitt: der Ringpuffer
/// (`twitch_engagement_stream_transcripts`) fuettert den Prompt, damit der Bot
/// ueberhaupt weiss, wovon der Streamer gerade redet; die Sitzungsablage
/// (`twitch_smalltalk_transcripts`) bleibt bis zur Auswertung liegen, weil der
/// Ringpuffer nach einer Stunde getrimmt wird und eine Sitzung genau so lange
/// dauert.
///
/// Aufgenommen wird nur, solange eine Sitzung offen ist, und nur gegen einen
/// Endpunkt auf dieser Maschine. Zeigt `ENGAGEMENT_STT_BASE_URL` nach draussen,
/// wird gar nicht aufgenommen: fremder Stream-Ton geht an keinen Fremdanbieter,
/// und ein Test ohne Ton ist besser als einer, der Daten abgibt.
fn spawn_transcript_capture(supervisor: &TaskSupervisor, store: SmalltalkLoopStore, pool: PgPool) {
    supervisor.spawn("smalltalk_loop_transcripts", async move {
        let capturer = AudioCapturer::from_env();
        let transcripts = StreamTranscripts::new(pool);
        let mut transcriber: Option<OpenAiTranscriber> = None;
        let mut remote_warned = false;
        let mut tick = tokio::time::interval(TRANSCRIPT_INTERVAL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            let session = match store.active_session().await {
                Ok(Some(session)) => session,
                Ok(None) => continue,
                Err(error) => {
                    tracing::warn!(event = "smalltalk_loop.transcript_session_failed", %error);
                    continue;
                }
            };
            if transcriber.is_none() {
                transcriber = OpenAiTranscriber::from_env();
            }
            let Some(transcriber) = &transcriber else {
                tracing::warn!(event = "smalltalk_loop.transcriber_unavailable");
                continue;
            };
            if !transcriber.is_local() {
                if !remote_warned {
                    remote_warned = true;
                    tracing::warn!(
                        event = "smalltalk_loop.transcript_skipped",
                        reason = "stt_endpoint_not_local",
                    );
                }
                continue;
            }
            let channel = session.channel_login.clone();
            let Some(segment) = capture_transcript_segment(&channel, &capturer, transcriber).await
            else {
                continue;
            };
            if let Err(error) = transcripts.append_segment(&segment).await {
                tracing::warn!(
                    event = "smalltalk_loop.transcript_context_failed",
                    %error,
                    channel = %channel,
                );
            }
            match store.record_transcript(&channel, &segment).await {
                Ok(true) => tracing::debug!(
                    event = "smalltalk_loop.transcript_recorded",
                    session_id = %session.id,
                    channel = %channel,
                ),
                // Sitzung endete waehrend der Aufnahme: der Block gehoert zu
                // keinem Test mehr und wird nicht aufbewahrt.
                Ok(false) => {}
                Err(error) => tracing::warn!(
                    event = "smalltalk_loop.transcript_store_failed",
                    %error,
                    channel = %channel,
                ),
            }
        }
    });
}

async fn process_once(
    store: &SmalltalkLoopStore,
    now: chrono::DateTime<Utc>,
) -> Result<(), StoreError> {
    store.close_ineligible_session(now).await?;
    if store.active_session().await?.is_none() {
        store.start_next_session(now).await?;
    }
    Ok(())
}

fn spawn_discord_forwarder(
    supervisor: &TaskSupervisor,
    store: SmalltalkLoopStore,
    discord: Arc<dyn DiscordBackend>,
    channel_id: i64,
    copy: DiscordCopy,
) {
    supervisor.spawn("smalltalk_loop_discord_forwarder", async move {
        let mut tick = tokio::time::interval(DISCORD_INTERVAL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            if let Err(error) =
                forward_discord_once(&store, discord.as_ref(), channel_id, &copy).await
            {
                tracing::warn!(event = "smalltalk_loop.discord_forward_failed", %error);
            }
        }
    });
}

fn spawn_retention(
    supervisor: &TaskSupervisor,
    store: SmalltalkLoopStore,
    discord: Arc<dyn DiscordBackend>,
    copy: DiscordCopy,
) {
    supervisor.spawn("smalltalk_loop_retention", async move {
        let mut tick = tokio::time::interval(RETENTION_INTERVAL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            if let Err(error) = cleanup_once(&store, discord.as_ref(), &copy, Utc::now()).await {
                tracing::warn!(event = "smalltalk_loop.retention_failed", %error);
            }
        }
    });
}

async fn forward_discord_once(
    store: &SmalltalkLoopStore,
    discord: &dyn DiscordBackend,
    channel_id: i64,
    copy: &DiscordCopy,
) -> Result<(), String> {
    let claims = store
        .claim_reports(DISCORD_BATCH_LIMIT, Utc::now())
        .await
        .map_err(|error| error.to_string())?;
    for ClaimedReport { claim_id, report } in claims {
        let session_id = report.session.id;
        let channel = report.session.channel_login.clone();
        let payload = match build_discord_card(&report, channel_id, copy) {
            Ok(payload) => payload,
            Err(error_class) => {
                store
                    .mark_report_failed(session_id, claim_id, &error_class, Utc::now())
                    .await
                    .map_err(|error| error.to_string())?;
                tracing::warn!(
                    event = "smalltalk_loop.report_failed",
                    %session_id,
                    %channel,
                    reason = %error_class,
                );
                continue;
            }
        };
        match discord.send_rich_message(payload).await {
            Ok(sent) => {
                store
                    .mark_report_sent(session_id, claim_id, &sent.result.message_id)
                    .await
                    .map_err(|error| error.to_string())?;
                tracing::info!(
                    event = "smalltalk_loop.report_sent",
                    %session_id,
                    %channel,
                    result = "sent",
                );
            }
            Err(error) => {
                let error_class = discord_error_class(&error);
                store
                    .mark_report_failed(session_id, claim_id, error_class, Utc::now())
                    .await
                    .map_err(|store_error| store_error.to_string())?;
                tracing::warn!(
                    event = "smalltalk_loop.report_failed",
                    %session_id,
                    %channel,
                    reason = error_class,
                );
            }
        }
    }
    Ok(())
}

async fn cleanup_once(
    store: &SmalltalkLoopStore,
    discord: &dyn DiscordBackend,
    copy: &DiscordCopy,
    now: chrono::DateTime<Utc>,
) -> Result<(), String> {
    store
        .delete_expired_unposted(now)
        .await
        .map_err(|error| error.to_string())?;
    for report in store
        .expired_discord_reports(RETENTION_BATCH_LIMIT, now)
        .await
        .map_err(|error| error.to_string())?
    {
        match discord
            .delete_message(DeleteMessage {
                channel_id: DEFAULT_REVIEW_CHANNEL_ID,
                message_id: report.message_id.clone(),
                reason: copy.retention_delete_reason.clone(),
            })
            .await
        {
            Ok(()) | Err(DiscordError::BrokerError { status: 404, .. }) => store
                .delete_expired_report(report.session_id, &report.message_id, now)
                .await
                .map_err(|error| error.to_string())?,
            Err(error) => store
                .mark_report_delete_failed(report.session_id, discord_error_class(&error), now)
                .await
                .map_err(|store_error| store_error.to_string())?,
        }
    }
    Ok(())
}

fn build_discord_card(
    report: &SmalltalkReport,
    channel_id: i64,
    copy: &DiscordCopy,
) -> Result<SendRichMessage, String> {
    let duration = (report.session.ended_at - report.session.started_at)
        .num_seconds()
        .max(0);
    let would_send = report
        .messages
        .iter()
        .filter(|message| message.outcome == "would_send")
        .count();
    let rejected = report.messages.len().saturating_sub(would_send);
    let mut reasons = BTreeMap::<&str, usize>::new();
    for reason in report
        .messages
        .iter()
        .filter_map(|message| message.reject_reason.as_deref())
    {
        *reasons.entry(reason).or_default() += 1;
    }
    let reason_summary = if reasons.is_empty() {
        "-".to_string()
    } else {
        reasons
            .into_iter()
            .map(|(reason, count)| format!("{}: {count}", copy.reason_label(reason)))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let viewers = report
        .session
        .viewer_count
        .map_or_else(|| copy.not_available.clone(), |count| count.to_string());
    let last_provider_error = report
        .session
        .last_provider_error
        .as_deref()
        .map(clean)
        .unwrap_or_else(|| "-".to_string());
    let transcript_summary = if report.transcripts.is_empty() {
        copy.transcripts_missing.clone()
    } else {
        let sekunden: i64 = report
            .transcripts
            .iter()
            .map(|segment| (segment.ended_at - segment.started_at).num_seconds().max(0))
            .sum();
        format!(
            "{} Abschnitte, {} min",
            report.transcripts.len(),
            sekunden / 60
        )
    };
    let mut displays = vec![format!(
        "**{title}**\n{channel_label}: #{channel}\n{session_label}: `{session_id}`\
\n{duration_label}: {duration}s\n{viewers_label}: {viewers}\n{end_label}: `{end_reason}`\
\n{generated_label}: {generated}\n{would_send_label}: {would_send}\
\n{rejected_label}: {rejected}\n{provider_errors_label}: {provider_errors}\
\n{last_provider_error_label}: {last_provider_error}\
\n{transcripts_label}: {transcript_summary}\n{reasons_label}:\n{reason_summary}",
        title = copy.title,
        channel_label = copy.channel,
        channel = clean(&report.session.channel_login),
        session_label = copy.session,
        session_id = report.session.id,
        duration_label = copy.duration,
        viewers_label = copy.viewers,
        end_label = copy.end_reason,
        end_reason = clean(copy.end_reason_label(&report.session.end_reason)),
        generated_label = copy.generated,
        generated = report.messages.len(),
        would_send_label = copy.would_send,
        rejected_label = copy.rejected,
        provider_errors_label = copy.provider_errors,
        provider_errors = report.session.provider_error_count,
        last_provider_error_label = copy.last_provider_error,
        transcripts_label = copy.transcripts,
        reasons_label = copy.rejection_reasons,
    )];
    for message in &report.messages {
        let result = match (&*message.outcome, message.reject_reason.as_deref()) {
            ("would_send", _) => copy.result_would_send.clone(),
            ("rejected", Some(reason)) => {
                format!("{}: {}", copy.result_rejected, copy.reason_label(reason))
            }
            ("rejected", None) => copy.result_rejected.clone(),
            _ => clean(&message.outcome),
        };
        let mut display = format!(
            "**{}**\n{}\n**{}**\n{}\n**{}**\n{}",
            copy.trigger,
            clean(&message.trigger_text),
            copy.generated_text,
            clean(&message.generated_text),
            copy.result,
            result
        );
        // Der Chat-Auslöser allein sagt nicht, ob die Antwort zum Moment
        // gepasst hat. Erst der Ton daneben macht das beurteilbar.
        if let Some(context) = transcript_context(&report.transcripts, message.generated_at) {
            display.push_str(&format!(
                "\n**{}**\n{}",
                copy.transcript_context,
                clean(context)
            ));
        }
        displays.push(display);
    }
    // Lieber ein Report, der ankommt und sagt was fehlt, als ein
    // vollstaendiger, den Discord ablehnt und der nach drei Versuchen
    // verschwindet. Die Zusammenfassung steht in displays[0] und bleibt
    // dadurch immer erhalten; gekuerzt werden nur die Einzelnachrichten, und
    // die Kuerzung steht sichtbar in der Karte.
    if displays.len() > MAX_TEXT_DISPLAYS_PER_CARD {
        let gezeigt = MAX_TEXT_DISPLAYS_PER_CARD - 1;
        let verborgen = displays.len() - gezeigt;
        displays.truncate(gezeigt);
        displays.push(format!(
            "**{}**\n{} von {} Nachrichten sind hier nicht abgebildet.\n{}: `{}`",
            copy.truncated_note,
            verborgen,
            report.messages.len(),
            copy.session,
            report.session.id
        ));
    }
    if displays
        .iter()
        .any(|display| display.chars().count() > 3_800)
    {
        return Err("discord_text_too_long".to_string());
    }
    Ok(SendRichMessage {
        channel_id,
        content: None,
        embed: json!({}),
        components: Some(json!([{
            "type": 17,
            "accent_color": SMALLTALK_GOLD,
            "components": displays.into_iter().map(|content| json!({
                "type": 10,
                "content": content
            })).collect::<Vec<_>>()
        }])),
        allowed_role_ids: vec![],
        view_spec: None,
    })
}

/// Der Stream-Ton, der zu einer erzeugten Nachricht gehoert: der juengste
/// Abschnitt, der vor ihr begonnen hat und hoechstens
/// [`TRANSCRIPT_CONTEXT_WINDOW`] vorher endete.
///
/// Aelteres wird nicht gezeigt: ein Ausschnitt von vor zehn Minuten erklaert
/// die Nachricht nicht, sondern legt einen Zusammenhang nahe, den es nicht gab.
fn transcript_context(
    transcripts: &[SmalltalkTranscript],
    generated_at: chrono::DateTime<Utc>,
) -> Option<&str> {
    transcripts
        .iter()
        .rfind(|segment| {
            segment.started_at <= generated_at
                && generated_at - segment.ended_at <= TRANSCRIPT_CONTEXT_WINDOW
        })
        .map(|segment| segment.text.as_str())
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

fn discord_error_class(error: &DiscordError) -> &'static str {
    match error {
        DiscordError::Http(error) if error.is_timeout() => "timeout",
        DiscordError::Http(_) => "transport",
        DiscordError::BrokerError { .. } => "broker_status",
        DiscordError::Deserialize(_) => "decode",
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration as ChronoDuration, TimeZone, Utc};
    use serde_json::Value;
    use sqlx::types::Uuid;
    use tb_engagement::smalltalk_loop_store::{ReportSession, SmalltalkMessage, SmalltalkReport};

    use super::*;

    #[test]
    fn feature_ist_standardmaessig_aus() {
        assert!(!SmalltalkConfig::from_value(None).enabled);
        assert!(SmalltalkConfig::from_value(Some("on")).enabled);
        assert!(!SmalltalkConfig::from_value(Some("off")).enabled);
    }

    #[test]
    fn testmodus_hat_keinen_twitch_send_port() {
        assert_eq!(
            SmalltalkDeliveryTarget::ALL,
            [
                SmalltalkDeliveryTarget::Database,
                SmalltalkDeliveryTarget::Discord,
            ]
        );
    }

    /// Eine Stunde Testmodus kann mehr Nachrichten erzeugen, als in einen
    /// Components-V2-Container passen. Ein zu grosser Post scheitert bei
    /// Discord, laeuft nach drei Versuchen aus und der Report verschwindet
    /// genau dann, wenn es am meisten zu sehen gaebe. Also wird gekuerzt,
    /// aber sichtbar: die Zusammenfassung bleibt, und die Karte sagt, wie
    /// viele Nachrichten nur in der Datenbank stehen.
    #[test]
    fn viele_nachrichten_kuerzen_sichtbar_statt_die_karte_zu_sprengen() {
        let now = Utc.with_ymd_and_hms(2026, 7, 27, 20, 0, 0).unwrap();
        let report = SmalltalkReport {
            session: ReportSession {
                id: Uuid::nil(),
                channel_login: "kandidat".to_string(),
                started_at: now,
                ended_at: now + ChronoDuration::minutes(60),
                end_reason: "time_limit".to_string(),
                viewer_count: Some(30),
                provider_error_count: 0,
                last_provider_error: None,
            },
            messages: (0..120)
                .map(|i| SmalltalkMessage {
                    generated_at: now,
                    generated_text: format!("antwort {i}"),
                    trigger_text: format!("ausloeser {i}"),
                    outcome: "would_send".to_string(),
                    reject_reason: None,
                })
                .collect(),
            transcripts: vec![],
        };

        let copy_text = DiscordCopy::test_copy();
        let payload = build_discord_card(&report, 123, &copy_text).expect("Discord-Karte");
        let components = payload.components.expect("Components");
        let displays = components[0]["components"]
            .as_array()
            .expect("Displays")
            .len();

        assert!(
            displays <= MAX_TEXT_DISPLAYS_PER_CARD,
            "die Karte muss unter dem Discord-Limit bleiben, war {displays}"
        );
        let zusammenfassung = components[0]["components"][0]["content"]
            .as_str()
            .expect("Zusammenfassung");
        assert!(
            zusammenfassung.contains("120"),
            "die Gesamtzahl bleibt sichtbar, auch wenn Einzelnachrichten fehlen"
        );
        let letzter = components[0]["components"][displays - 1]["content"]
            .as_str()
            .expect("Hinweis");
        assert!(
            letzter.contains(&copy_text.truncated_note) && letzter.contains("120"),
            "die Karte muss ausweisen, wie viele Nachrichten fehlen: {letzter}"
        );
    }

    #[test]
    fn discord_karte_zeigt_auch_leere_sitzung_ohne_mentions() {
        let now = Utc.with_ymd_and_hms(2026, 7, 27, 20, 0, 0).unwrap();
        let report = SmalltalkReport {
            session: ReportSession {
                id: Uuid::nil(),
                channel_login: "kandidat".to_string(),
                started_at: now,
                ended_at: now + ChronoDuration::minutes(5),
                end_reason: "stream_ended".to_string(),
                viewer_count: Some(12),
                provider_error_count: 0,
                last_provider_error: None,
            },
            messages: vec![],
            transcripts: vec![],
        };

        let payload =
            build_discord_card(&report, 123, &DiscordCopy::test_copy()).expect("Discord-Karte");
        let components = payload.components.expect("Components");
        let text = components[0]["components"][0]["content"]
            .as_str()
            .expect("Kartentext");

        assert_eq!(payload.channel_id, 123);
        assert!(payload.content.is_none());
        assert!(payload.allowed_role_ids.is_empty());
        assert_eq!(components[0]["type"], Value::from(17));
        assert_eq!(components[0]["accent_color"], Value::from(0xC8A86B));
        assert!(text.contains("0"));
        assert!(text.contains("Stream beendet"));
    }

    #[test]
    fn discord_karte_zaehlt_gruende_und_zeigt_jeden_text_mit_ausloeser() {
        let now = Utc.with_ymd_and_hms(2026, 7, 27, 20, 0, 0).unwrap();
        let report = SmalltalkReport {
            session: ReportSession {
                id: Uuid::nil(),
                channel_login: "kandidat".to_string(),
                started_at: now,
                ended_at: now + ChronoDuration::minutes(60),
                end_reason: "session_timeout".to_string(),
                viewer_count: None,
                provider_error_count: 1,
                last_provider_error: Some("http_status".to_string()),
            },
            messages: vec![
                SmalltalkMessage {
                    generated_at: now,
                    generated_text: "haze bleibt stark".to_string(),
                    trigger_text: "was haltet ihr von haze".to_string(),
                    outcome: "would_send".to_string(),
                    reject_reason: None,
                },
                SmalltalkMessage {
                    generated_at: now,
                    generated_text: "komm auf discord".to_string(),
                    trigger_text: "wo spielt ihr".to_string(),
                    outcome: "rejected".to_string(),
                    reject_reason: Some("offer_or_link".to_string()),
                },
            ],
            transcripts: vec![],
        };

        let payload =
            build_discord_card(&report, 123, &DiscordCopy::test_copy()).expect("Discord-Karte");
        let text = payload.components.expect("Components").to_string();
        assert!(text.contains("haze bleibt stark"));
        assert!(text.contains("was haltet ihr von haze"));
        assert!(text.contains("komm auf discord"));
        assert!(text.contains("wo spielt ihr"));
        assert!(text.contains("Angebot oder Link"));
        assert!(text.contains("http"));
    }

    /// Ohne den Stream-Ton ist eine Nachricht nicht zu beurteilen: sie kann zum
    /// Chat passen und zum Moment trotzdem daneben liegen. Die Karte zeigt
    /// deshalb den Abschnitt, der zur Nachricht gehoert, und nur den.
    #[test]
    fn discord_karte_zeigt_den_stream_ton_zur_nachricht() {
        let now = Utc.with_ymd_and_hms(2026, 8, 13, 20, 0, 0).unwrap();
        let report = SmalltalkReport {
            session: ReportSession {
                id: Uuid::nil(),
                channel_login: "kandidat".to_string(),
                started_at: now,
                ended_at: now + ChronoDuration::minutes(60),
                end_reason: "session_timeout".to_string(),
                viewer_count: Some(9),
                provider_error_count: 0,
                last_provider_error: None,
            },
            messages: vec![SmalltalkMessage {
                generated_at: now + ChronoDuration::minutes(30),
                generated_text: "der ult kam echt spaet".to_string(),
                trigger_text: "gg".to_string(),
                outcome: "would_send".to_string(),
                reject_reason: None,
            }],
            transcripts: vec![
                SmalltalkTranscript {
                    started_at: now,
                    ended_at: now + ChronoDuration::seconds(45),
                    text: "viel zu alt fuer den kontext".to_string(),
                    engine: "openai_api".to_string(),
                    model: Some("whisper-1".to_string()),
                },
                SmalltalkTranscript {
                    started_at: now + ChronoDuration::minutes(29),
                    ended_at: now + ChronoDuration::minutes(29) + ChronoDuration::seconds(45),
                    text: "ich haette den ult frueher zuenden muessen".to_string(),
                    engine: "openai_api".to_string(),
                    model: Some("whisper-1".to_string()),
                },
                SmalltalkTranscript {
                    started_at: now + ChronoDuration::minutes(40),
                    ended_at: now + ChronoDuration::minutes(40) + ChronoDuration::seconds(45),
                    text: "das kam erst nach der nachricht".to_string(),
                    engine: "openai_api".to_string(),
                    model: Some("whisper-1".to_string()),
                },
            ],
        };

        let copy_text = DiscordCopy::test_copy();
        let payload = build_discord_card(&report, 123, &copy_text).expect("Discord-Karte");
        let components = payload.components.expect("Components");
        let zusammenfassung = components[0]["components"][0]["content"]
            .as_str()
            .expect("Zusammenfassung");
        let nachricht = components[0]["components"][1]["content"]
            .as_str()
            .expect("Nachricht");

        assert!(
            zusammenfassung.contains(&copy_text.transcripts)
                && zusammenfassung.contains("3 Abschnitte"),
            "die Sitzung muss ausweisen, wie viel Ton erfasst wurde: {zusammenfassung}"
        );
        assert!(
            nachricht.contains("ich haette den ult frueher zuenden muessen"),
            "der Abschnitt vor der Nachricht gehoert dazu: {nachricht}"
        );
        assert!(
            !nachricht.contains("viel zu alt")
                && !nachricht.contains("das kam erst nach der nachricht"),
            "weder alter noch spaeterer Ton darf einen Zusammenhang vortaeuschen: {nachricht}"
        );
    }

    /// Eine Sitzung ohne Ton ist ein Ergebnis, kein Anlass, das Feld
    /// wegzulassen: sonst saehe "STT war aus" wie "der Streamer schwieg" aus.
    #[test]
    fn discord_karte_weist_fehlenden_stream_ton_aus() {
        let now = Utc.with_ymd_and_hms(2026, 8, 13, 20, 0, 0).unwrap();
        let report = SmalltalkReport {
            session: ReportSession {
                id: Uuid::nil(),
                channel_login: "kandidat".to_string(),
                started_at: now,
                ended_at: now + ChronoDuration::minutes(20),
                end_reason: "stream_ended".to_string(),
                viewer_count: None,
                provider_error_count: 0,
                last_provider_error: None,
            },
            messages: vec![],
            transcripts: vec![],
        };

        let copy_text = DiscordCopy::test_copy();
        let payload = build_discord_card(&report, 123, &copy_text).expect("Discord-Karte");
        let text = payload.components.expect("Components")[0]["components"][0]["content"]
            .as_str()
            .expect("Kartentext")
            .to_string();

        assert!(text.contains(&copy_text.transcripts));
        assert!(text.contains(&copy_text.transcripts_missing));
    }
}
