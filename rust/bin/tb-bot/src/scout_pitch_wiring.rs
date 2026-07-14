//! Read-only IRC-Reader und Seiteneffekt-Schale der Scout-Pitch-Pipeline.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use sqlx::{PgPool, Row};
use tb_engagement::minimax_chat::{sanitize_chat_text, EngagementMinimaxClient};
use tb_engagement::scout_pitch::{
    decide, parse_judge_json, parse_pitch_json, ChatLine, Decision, DecisionInput, JudgeState,
    LedgerAction, LedgerEntry, ScoutPitchLedger, TriggerType, JUDGE_SYSTEM_PROMPT,
    PITCH_SYSTEM_PROMPT,
};
use tb_monitoring::{ScoutEventSink, StreamSnapshot};
use tb_transport_discord::{BrokerRelay, DiscordBackend, SendRichMessage};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use crate::task_supervisor::TaskSupervisor;

const STAFF_CHANNEL_ID: i64 = 1_374_364_800_817_303_632;
const ROSTER_INTERVAL: Duration = Duration::from_secs(30);
const JUDGE_COOLDOWN: Duration = Duration::from_secs(5 * 60);
const RECONNECT_DELAY: Duration = Duration::from_secs(30);
const SCOUT_COLOR: i64 = 0xC8_A8_6B;

#[derive(Debug, Clone, PartialEq, Eq)]
struct IrcPrivmsg {
    channel: String,
    chatter: String,
    text: String,
}

fn parse_privmsg(line: &str) -> Option<IrcPrivmsg> {
    let (prefix, rest) = line.split_once(" PRIVMSG #")?;
    let (channel, text) = rest.split_once(" :")?;
    let chatter = prefix
        .rsplit_once(' ')
        .map(|(_, value)| value)
        .unwrap_or(prefix)
        .trim_start_matches(':')
        .split_once('!')?
        .0
        .trim()
        .to_lowercase();
    let channel = channel.trim().trim_start_matches('#').to_lowercase();
    let text = text.trim().to_string();
    if channel.is_empty() || chatter.is_empty() || text.is_empty() {
        return None;
    }
    Some(IrcPrivmsg {
        channel,
        chatter,
        text,
    })
}

fn anonymous_handshake(nick: &str) -> Vec<String> {
    vec![
        format!("NICK {nick}\r\n"),
        "CAP REQ :twitch.tv/tags twitch.tv/commands\r\n".to_string(),
    ]
}

enum ReaderCommand {
    SetChannels(Vec<String>),
}

enum ProtocolCommand<'a> {
    Join(&'a str),
    Part(&'a str),
    Pong(&'a str),
}

impl ProtocolCommand<'_> {
    fn line(&self) -> String {
        match self {
            Self::Join(channel) => format!("JOIN #{channel}\r\n"),
            Self::Part(channel) => format!("PART #{channel}\r\n"),
            Self::Pong(payload) => format!("PONG {}\r\n", payload.trim()),
        }
    }
}

async fn write_protocol(writer: &mut OwnedWriteHalf, command: ProtocolCommand<'_>) -> bool {
    let line = command.line();
    match writer.write_all(line.as_bytes()).await {
        Ok(()) => writer.flush().await.is_ok(),
        Err(error) => {
            tracing::warn!(%error, "Scout-IRC: Protokoll-Write fehlgeschlagen");
            false
        }
    }
}

async fn irc_reader_loop(
    mut commands: mpsc::UnboundedReceiver<ReaderCommand>,
    events: mpsc::UnboundedSender<ScoutSignal>,
) {
    let mut channels = HashSet::new();
    loop {
        match TcpStream::connect(("irc.chat.twitch.tv", 6667)).await {
            Ok(stream) => {
                let (reader, mut writer) = stream.into_split();
                let nick = "justinfan12345";
                let mut handshake_ok = true;
                for line in anonymous_handshake(nick) {
                    if writer.write_all(line.as_bytes()).await.is_err() {
                        handshake_ok = false;
                        break;
                    }
                }
                if handshake_ok && writer.flush().await.is_ok() {
                    serve_irc(reader, &mut writer, &mut commands, &events, &mut channels).await;
                }
            }
            Err(error) => tracing::warn!(%error, "Scout-IRC: Verbindung fehlgeschlagen"),
        }
        tokio::time::sleep(RECONNECT_DELAY).await;
    }
}

async fn serve_irc(
    reader: tokio::net::tcp::OwnedReadHalf,
    writer: &mut OwnedWriteHalf,
    commands: &mut mpsc::UnboundedReceiver<ReaderCommand>,
    events: &mpsc::UnboundedSender<ScoutSignal>,
    channels: &mut HashSet<String>,
) {
    for channel in channels.iter() {
        if !write_protocol(writer, ProtocolCommand::Join(channel)).await {
            return;
        }
    }
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    loop {
        line.clear();
        tokio::select! {
            result = reader.read_line(&mut line) => match result {
                Ok(0) | Err(_) => return,
                Ok(_) => {
                    let message = line.trim_end();
                    if let Some(payload) = message.strip_prefix("PING ") {
                        if !write_protocol(writer, ProtocolCommand::Pong(payload)).await {
                            return;
                        }
                    } else if let Some(message) = parse_privmsg(message) {
                        let _ = events.send(ScoutSignal::Chat(message));
                    }
                }
            },
            command = commands.recv() => match command {
                Some(ReaderCommand::SetChannels(next)) => {
                    let next: HashSet<String> = next.into_iter().collect();
                    for channel in channels.difference(&next) {
                        if !write_protocol(writer, ProtocolCommand::Part(channel)).await {
                            return;
                        }
                    }
                    for channel in next.difference(channels) {
                        if !write_protocol(writer, ProtocolCommand::Join(channel)).await {
                            return;
                        }
                    }
                    *channels = next;
                }
                None => return,
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LiveScoutChannel {
    login: String,
    is_live: bool,
    viewer_count: Option<i32>,
    stream_key: String,
}

#[cfg(test)]
impl LiveScoutChannel {
    fn test(login: &str, is_live: bool, stream_key: &str) -> Self {
        Self {
            login: login.to_string(),
            is_live,
            viewer_count: Some(1),
            stream_key: stream_key.to_string(),
        }
    }
}

fn offline_transitions(
    previous: &HashMap<String, LiveScoutChannel>,
    current: &HashMap<String, LiveScoutChannel>,
) -> Vec<LiveScoutChannel> {
    let mut transitions: Vec<_> = previous
        .iter()
        .filter(|(_, before)| before.is_live)
        .filter_map(|(login, before)| {
            current
                .get(login)
                .filter(|after| !after.is_live)
                .map(|_| before.clone())
        })
        .collect();
    transitions.sort_by(|a, b| a.login.cmp(&b.login));
    transitions
}

async fn load_scout_channels(
    pool: &PgPool,
) -> Result<HashMap<String, LiveScoutChannel>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT LOWER(s.twitch_login) AS login, COALESCE(ls.is_live, 0) AS is_live, \
                ls.last_viewer_count, ls.last_stream_id, ls.last_started_at, ls.active_session_id \
         FROM twitch_streamers s \
         LEFT JOIN twitch_live_state ls ON ls.twitch_user_id = s.twitch_user_id \
                                      OR LOWER(ls.streamer_login) = LOWER(s.twitch_login) \
         WHERE NOT EXISTS (SELECT 1 FROM twitch_partners p \
                           WHERE p.twitch_user_id = s.twitch_user_id \
                              OR LOWER(p.twitch_login) = LOWER(s.twitch_login))",
    )
    .fetch_all(pool)
    .await?;
    let mut channels = HashMap::new();
    for row in rows {
        let login: String = row.try_get("login")?;
        let stream_key = row
            .try_get::<Option<String>, _>("last_stream_id")?
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                row.try_get::<Option<String>, _>("last_started_at")
                    .ok()
                    .flatten()
            })
            .or_else(|| {
                row.try_get::<Option<i64>, _>("active_session_id")
                    .ok()
                    .flatten()
                    .map(|value| value.to_string())
            })
            .unwrap_or_else(|| format!("unknown:{login}"));
        channels.insert(
            login.clone(),
            LiveScoutChannel {
                login,
                is_live: row.try_get::<i32, _>("is_live")? != 0,
                viewer_count: row.try_get("last_viewer_count")?,
                stream_key,
            },
        );
    }
    Ok(channels)
}

async fn roster_loop(
    pool: PgPool,
    reader: mpsc::UnboundedSender<ReaderCommand>,
    events: mpsc::UnboundedSender<ScoutSignal>,
) {
    let mut previous = HashMap::new();
    let mut tick = tokio::time::interval(ROSTER_INTERVAL);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tick.tick().await;
        match load_scout_channels(&pool).await {
            Ok(current) => {
                for channel in offline_transitions(&previous, &current) {
                    let _ = events.send(ScoutSignal::Offline(channel));
                }
                let live_logins = current
                    .values()
                    .filter(|channel| channel.is_live)
                    .map(|channel| channel.login.clone())
                    .collect();
                let _ = reader.send(ReaderCommand::SetChannels(live_logins));
                let _ = events.send(ScoutSignal::Roster(current.clone()));
                previous = current;
            }
            Err(error) => tracing::warn!(%error, "Scout-Pitch: Roster nicht lesbar"),
        }
    }
}

enum ScoutSignal {
    Chat(IrcPrivmsg),
    Roster(HashMap<String, LiveScoutChannel>),
    Offline(LiveScoutChannel),
    NewStreamer(LiveScoutChannel),
}

struct ScoutPitchEventSink {
    events: mpsc::UnboundedSender<ScoutSignal>,
}

#[async_trait]
impl ScoutEventSink for ScoutPitchEventSink {
    async fn on_new_streamer(&self, stream: &StreamSnapshot) {
        let login = stream.user_login.trim().to_lowercase();
        if login.is_empty() {
            return;
        }
        let stream_key = stream
            .id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .or(stream.started_at.as_deref())
            .unwrap_or("unknown")
            .to_string();
        let _ = self.events.send(ScoutSignal::NewStreamer(LiveScoutChannel {
            login,
            is_live: true,
            viewer_count: Some(stream.viewer_count),
            stream_key,
        }));
    }
}

struct ScoutPitchRuntime {
    ledger: ScoutPitchLedger,
    llm: Arc<EngagementMinimaxClient>,
    discord: Arc<dyn DiscordBackend>,
    channel_id: i64,
    channels: HashMap<String, LiveScoutChannel>,
    windows: HashMap<String, VecDeque<ChatLine>>,
    last_judge: HashMap<String, Instant>,
}

impl ScoutPitchRuntime {
    async fn run(mut self, mut events: mpsc::UnboundedReceiver<ScoutSignal>) {
        while let Some(signal) = events.recv().await {
            match signal {
                ScoutSignal::Roster(channels) => self.channels = channels,
                ScoutSignal::Chat(message) => self.handle_chat(message).await,
                ScoutSignal::Offline(channel) => {
                    let lines = self.window(&channel.login);
                    self.handle_trigger(
                        TriggerType::OfflineMoment,
                        &channel,
                        "offline ohne Raid-Ziel",
                        JudgeState::NotNeeded,
                        "offline_moment",
                        None,
                        &lines,
                    )
                    .await;
                    self.last_judge.remove(&channel.login);
                }
                ScoutSignal::NewStreamer(channel) => {
                    self.handle_trigger(
                        TriggerType::NewStreamer,
                        &channel,
                        "zum ersten Mal gesehen",
                        JudgeState::NotNeeded,
                        "new_streamer",
                        None,
                        &[],
                    )
                    .await;
                }
            }
        }
    }

    fn window(&self, login: &str) -> Vec<ChatLine> {
        self.windows
            .get(login)
            .map(|window| window.iter().cloned().collect())
            .unwrap_or_default()
    }

    async fn handle_chat(&mut self, message: IrcPrivmsg) {
        let window = self.windows.entry(message.channel.clone()).or_default();
        window.push_back(ChatLine::new(message.chatter, message.text));
        while window.len() > 20 {
            window.pop_front();
        }
        let lines: Vec<ChatLine> = window.iter().cloned().collect();
        let Some(channel) = self.channels.get(&message.channel).cloned() else {
            return;
        };
        if self
            .last_judge
            .get(&message.channel)
            .is_some_and(|last| last.elapsed() < JUDGE_COOLDOWN)
        {
            self.record(
                &channel,
                TriggerType::ProblemMoment,
                &lines_excerpt(&lines),
                "cooldown",
                None,
                LedgerAction::SuppressedCooldown,
                None,
                None,
            )
            .await;
            return;
        }
        self.last_judge
            .insert(message.channel.clone(), Instant::now());
        let excerpt = lines_excerpt(&lines);
        let user = format!(
            "Streamer-Login: {}\nLetzte Chatzeilen:\n{excerpt}",
            channel.login
        );
        let result = tokio::time::timeout(
            Duration::from_secs(35),
            self.llm.raw_completion_tracked(
                JUDGE_SYSTEM_PROMPT,
                &user,
                200,
                0.0,
                "scout-pitch-judge",
            ),
        )
        .await;
        let raw = match result {
            Err(_) => {
                self.record(
                    &channel,
                    TriggerType::ProblemMoment,
                    &excerpt,
                    "timeout",
                    None,
                    LedgerAction::JudgeTimeout,
                    None,
                    None,
                )
                .await;
                return;
            }
            Ok(Err(error)) => {
                self.record(
                    &channel,
                    TriggerType::ProblemMoment,
                    &excerpt,
                    "error",
                    None,
                    LedgerAction::JudgeError,
                    Some(error.to_string()),
                    None,
                )
                .await;
                return;
            }
            Ok(Ok(raw)) => raw,
        };
        let verdict = match parse_judge_json(&raw) {
            Ok(verdict) => verdict,
            Err(error) => {
                self.record(
                    &channel,
                    TriggerType::ProblemMoment,
                    &excerpt,
                    "parse_error",
                    None,
                    LedgerAction::JudgeError,
                    Some(error.to_string()),
                    None,
                )
                .await;
                return;
            }
        };
        let Some(trigger_type) = verdict.trigger_type else {
            self.handle_trigger(
                TriggerType::ProblemMoment,
                &channel,
                &verdict.quote,
                JudgeState::None,
                "none",
                Some(verdict.confidence),
                &lines,
            )
            .await;
            return;
        };
        self.handle_trigger(
            trigger_type,
            &channel,
            &verdict.quote,
            JudgeState::Triggered {
                confidence: verdict.confidence,
            },
            trigger_type.as_str(),
            Some(verdict.confidence),
            &lines,
        )
        .await;
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_trigger(
        &self,
        trigger_type: TriggerType,
        channel: &LiveScoutChannel,
        quote: &str,
        judge: JudgeState,
        verdict: &str,
        confidence: Option<f32>,
        lines: &[ChatLine],
    ) {
        let excerpt = lines_excerpt(lines);
        let (blacklisted, posted) = match self.gates(channel, trigger_type).await {
            Ok(gates) => gates,
            Err(error) => {
                self.record(
                    channel,
                    trigger_type,
                    &excerpt,
                    verdict,
                    confidence,
                    LedgerAction::JudgeError,
                    Some(error.to_string()),
                    None,
                )
                .await;
                return;
            }
        };
        let first = decide(&DecisionInput {
            trigger_type,
            blacklisted,
            cooldown_active: false,
            posted_for_stream: posted,
            judge,
            sanitized_message_count: usize::from(trigger_type.requires_pitch()),
        });
        if let Decision::Record(action) = first {
            self.record(
                channel,
                trigger_type,
                &excerpt,
                verdict,
                confidence,
                action,
                None,
                None,
            )
            .await;
            return;
        }

        let messages = if trigger_type.requires_pitch() {
            match self.generate_pitch(trigger_type, channel, lines).await {
                Ok(messages) => messages,
                Err((action, detail)) => {
                    self.record(
                        channel,
                        trigger_type,
                        &excerpt,
                        verdict,
                        confidence,
                        action,
                        Some(detail),
                        None,
                    )
                    .await;
                    return;
                }
            }
        } else {
            Vec::new()
        };
        let (blacklisted, posted) = match self.gates(channel, trigger_type).await {
            Ok(gates) => gates,
            Err(error) => {
                self.record(
                    channel,
                    trigger_type,
                    &excerpt,
                    verdict,
                    confidence,
                    LedgerAction::JudgeError,
                    Some(error.to_string()),
                    None,
                )
                .await;
                return;
            }
        };
        match decide(&DecisionInput {
            trigger_type,
            blacklisted,
            cooldown_active: false,
            posted_for_stream: posted,
            judge,
            sanitized_message_count: messages.len(),
        }) {
            Decision::Record(action) => {
                self.record(
                    channel,
                    trigger_type,
                    &excerpt,
                    verdict,
                    confidence,
                    action,
                    None,
                    None,
                )
                .await;
            }
            Decision::Post => {
                let payload = build_discord_payload(
                    self.channel_id,
                    trigger_type,
                    &channel.login,
                    channel.viewer_count,
                    quote,
                    lines,
                    &messages,
                );
                match self.discord.send_rich_message(payload).await {
                    Ok(result) => {
                        self.record(
                            channel,
                            trigger_type,
                            &excerpt,
                            verdict,
                            confidence,
                            LedgerAction::Posted,
                            Some(channel.stream_key.clone()),
                            Some(result.result.message_id),
                        )
                        .await;
                    }
                    Err(error) => {
                        self.record(
                            channel,
                            trigger_type,
                            &excerpt,
                            verdict,
                            confidence,
                            LedgerAction::DiscordError,
                            Some(error.to_string()),
                            None,
                        )
                        .await;
                    }
                }
            }
        }
    }

    async fn gates(
        &self,
        channel: &LiveScoutChannel,
        trigger_type: TriggerType,
    ) -> Result<(bool, bool), sqlx::Error> {
        let blacklisted = self.ledger.is_blacklisted(&channel.login).await?;
        let posted = self
            .ledger
            .has_posted_for_stream(&channel.login, trigger_type, &channel.stream_key)
            .await?;
        Ok((blacklisted, posted))
    }

    async fn generate_pitch(
        &self,
        trigger_type: TriggerType,
        channel: &LiveScoutChannel,
        lines: &[ChatLine],
    ) -> Result<Vec<String>, (LedgerAction, String)> {
        let viewer = channel
            .viewer_count
            .map(|value| format!("Viewer-Zahl: {value}\n"))
            .unwrap_or_default();
        let user = format!(
            "Trigger-Typ: {}\nStreamer-Login: {}\n{}Letzte Chat-Zeilen:\n{}",
            trigger_type.as_str(),
            channel.login,
            viewer,
            lines_excerpt(lines)
        );
        let result = tokio::time::timeout(
            Duration::from_secs(35),
            self.llm.raw_completion_tracked(
                PITCH_SYSTEM_PROMPT,
                &user,
                400,
                0.7,
                "scout-pitch-copy",
            ),
        )
        .await;
        let raw = match result {
            Err(_) => return Err((LedgerAction::JudgeTimeout, "pitch_timeout".to_string())),
            Ok(Err(error)) => return Err((LedgerAction::JudgeError, error.to_string())),
            Ok(Ok(raw)) => raw,
        };
        let generated = parse_pitch_json(&raw)
            .map_err(|error| (LedgerAction::JudgeError, error.to_string()))?;
        Ok(generated
            .into_iter()
            .take(3)
            .filter_map(|message| sanitize_chat_text(&message, 120))
            .collect())
    }

    #[allow(clippy::too_many_arguments)]
    async fn record(
        &self,
        channel: &LiveScoutChannel,
        trigger_type: TriggerType,
        excerpt: &str,
        verdict: &str,
        confidence: Option<f32>,
        action: LedgerAction,
        detail: Option<String>,
        discord_message_id: Option<String>,
    ) {
        let entry = LedgerEntry {
            streamer_login: channel.login.clone(),
            trigger_type,
            judge_input_excerpt: Some(truncate_chars(excerpt, 2_000)),
            judge_verdict: verdict.to_string(),
            confidence,
            action,
            detail,
            discord_message_id,
        };
        if let Err(error) = self.ledger.record(&entry).await {
            tracing::error!(%error, streamer = %channel.login, action = action.as_str(), "Scout-Pitch: Ledger-Schreiben fehlgeschlagen");
        }
    }
}

fn lines_excerpt(lines: &[ChatLine]) -> String {
    lines
        .iter()
        .map(|line| format!("{}: {}", line.chatter, line.text))
        .collect::<Vec<_>>()
        .join("\n")
}

fn truncate_chars(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

fn build_discord_payload(
    channel_id: i64,
    trigger_type: TriggerType,
    login: &str,
    viewer_count: Option<i32>,
    quote: &str,
    lines: &[ChatLine],
    messages: &[String],
) -> SendRichMessage {
    let viewer = viewer_count
        .map(|value| value.to_string())
        .unwrap_or_else(|| "PLATZHALTER: unbekannt".to_string());
    let mut body = vec![
        format!("**Streamer:** {login} ({viewer} Zuschauer)"),
        format!("**Trigger:** {}: {quote}", trigger_type.label()),
        "**Letzte Chatzeilen:**".to_string(),
    ];
    let recent: Vec<_> = lines.iter().rev().take(5).collect();
    body.extend(
        recent
            .into_iter()
            .rev()
            .map(|line| format!("{}: {}", line.chatter, line.text)),
    );
    if !messages.is_empty() {
        body.push("**Vorschlag (kopieren und als earlysalty senden):**".to_string());
        body.extend(
            messages
                .iter()
                .map(|message| format!("`{}`", message.replace('`', "'"))),
        );
    }
    SendRichMessage {
        channel_id,
        content: None,
        embed: serde_json::json!({}),
        components: Some(serde_json::json!([{
            "type": 17,
            "accent_color": SCOUT_COLOR,
            "components": [
                {"type": 10, "content": format!("## Scout: {} — {login}", trigger_type.label())},
                {"type": 14, "divider": true, "spacing": 1},
                {"type": 10, "content": body.join("\n")},
                {"type": 14, "divider": true, "spacing": 1},
                {"type": 10, "content": "-# Reaktion: 👍 gut / 👎 daneben"}
            ]
        }])),
        allowed_role_ids: vec![],
        view_spec: None,
    }
}

fn configured_channel_id() -> i64 {
    [
        "TB_SCOUT_PITCH_DISCORD_CHANNEL_ID",
        "SCAM_GUARD_DISCORD_CHANNEL_ID",
    ]
    .into_iter()
    .find_map(|name| {
        std::env::var(name)
            .ok()
            .and_then(|value| value.trim().parse::<i64>().ok())
            .filter(|value| *value > 0)
    })
    .unwrap_or(STAFF_CHANNEL_ID)
}

pub fn spawn_scout_pitch_pipeline(
    supervisor: &TaskSupervisor,
    pool: PgPool,
    broker: &tb_config::BrokerConfig,
) -> Option<Arc<dyn ScoutEventSink>> {
    if std::env::var("TB_SCOUT_PITCH_ENABLED").as_deref() != Ok("1") {
        tracing::info!("Scout-Pitch-Pipeline deaktiviert (TB_SCOUT_PITCH_ENABLED!=1)");
        return None;
    }
    let relay = match BrokerRelay::new(broker) {
        Ok(relay) => relay,
        Err(error) => {
            tracing::warn!(%error, "Scout-Pitch-Pipeline ohne Broker nicht startbar");
            return None;
        }
    };
    let (event_tx, event_rx) = mpsc::unbounded_channel();
    let (reader_tx, reader_rx) = mpsc::unbounded_channel();
    let runtime = ScoutPitchRuntime {
        ledger: ScoutPitchLedger::new(pool.clone()),
        llm: Arc::new(EngagementMinimaxClient::new(
            None,
            None,
            None,
            Some(Duration::from_secs(40)),
        )),
        discord: Arc::new(relay),
        channel_id: configured_channel_id(),
        channels: HashMap::new(),
        windows: HashMap::new(),
        last_judge: HashMap::new(),
    };
    supervisor.spawn("scout_pitch_runtime", runtime.run(event_rx));
    supervisor.spawn(
        "scout_pitch_irc_reader",
        irc_reader_loop(reader_rx, event_tx.clone()),
    );
    supervisor.spawn(
        "scout_pitch_roster",
        roster_loop(pool, reader_tx, event_tx.clone()),
    );
    tracing::info!(
        channel_id = configured_channel_id(),
        "Scout-Pitch-Pipeline aktiv (anonymer IRC-Read, Discord-Copilot)"
    );
    Some(Arc::new(ScoutPitchEventSink { events: event_tx }))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use tb_engagement::scout_pitch::{ChatLine, TriggerType};

    use super::*;

    #[test]
    fn privmsg_parser_reads_channel_chatter_and_text() {
        let parsed = parse_privmsg(
            "@badge-info=;badges= :streamer!streamer@streamer.tmi.twitch.tv PRIVMSG #streamer :moin zusammen",
        )
        .expect("PRIVMSG parsebar");
        assert_eq!(parsed.channel, "streamer");
        assert_eq!(parsed.chatter, "streamer");
        assert_eq!(parsed.text, "moin zusammen");
        assert!(
            parse_privmsg(":tmi.twitch.tv 366 justinfan #streamer :End of /NAMES list").is_none()
        );
    }

    #[test]
    fn anonymous_reader_has_no_credentials_or_chat_send_command() {
        let commands = anonymous_handshake("justinfan12345");
        assert_eq!(
            commands,
            vec![
                "NICK justinfan12345\r\n",
                "CAP REQ :twitch.tv/tags twitch.tv/commands\r\n"
            ]
        );
        assert!(commands.iter().all(|line| !line.starts_with("PASS ")));
        assert!(commands.iter().all(|line| !line.contains("PRIVMSG")));
    }

    #[test]
    fn offline_transition_only_fires_for_live_to_offline() {
        let previous = HashMap::from([
            (
                "live".to_string(),
                LiveScoutChannel::test("live", true, "s1"),
            ),
            (
                "still_off".to_string(),
                LiveScoutChannel::test("still_off", false, "s2"),
            ),
        ]);
        let current = HashMap::from([
            (
                "live".to_string(),
                LiveScoutChannel::test("live", false, "s1"),
            ),
            (
                "still_off".to_string(),
                LiveScoutChannel::test("still_off", false, "s2"),
            ),
            ("new".to_string(), LiveScoutChannel::test("new", true, "s3")),
        ]);
        assert_eq!(
            offline_transitions(&previous, &current)
                .into_iter()
                .map(|channel| channel.login)
                .collect::<Vec<_>>(),
            vec!["live"]
        );
    }

    #[test]
    fn discord_payload_snapshot_is_components_v2_without_buttons() {
        let lines = vec![
            ChatLine::new("tester", "schon wieder ein bot"),
            ChatLine::new("viewer", "ja nervt"),
        ];
        let payload = build_discord_payload(
            77,
            TriggerType::SpamBots,
            "tester",
            Some(42),
            "schon wieder ein bot",
            &lines,
            &["mein bot bannt das so gut wie instant".to_string()],
        );
        let value = serde_json::to_value(payload).expect("Payload serialisierbar");
        assert_eq!(
            value,
            serde_json::json!({
                "channel_id": 77,
                "content": null,
                "embed": {},
                "components": [{
                    "type": 17,
                    "accent_color": 13150315,
                    "components": [
                        {"type": 10, "content": "## Scout: Ärger über Spam-Bots — tester"},
                        {"type": 14, "divider": true, "spacing": 1},
                        {"type": 10, "content": "**Streamer:** tester (42 Zuschauer)\n**Trigger:** Ärger über Spam-Bots: schon wieder ein bot\n**Letzte Chatzeilen:**\ntester: schon wieder ein bot\nviewer: ja nervt\n**Vorschlag (kopieren und als earlysalty senden):**\n`mein bot bannt das so gut wie instant`"},
                        {"type": 14, "divider": true, "spacing": 1},
                        {"type": 10, "content": "-# Reaktion: 👍 gut / 👎 daneben"}
                    ]
                }],
                "allowed_role_ids": [],
                "view_spec": null
            })
        );
        let components = &value["components"];
        assert!(
            !components.to_string().contains("\"type\":2"),
            "keine Buttons"
        );
    }
}
