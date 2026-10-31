//! Engagement-Pipeline — reine Helfer (Slice 1).
//!
//! Port der I/O-freien Logik aus `bot/engagement/pipeline.py`: der billige
//! Pre-Filter ([`should_skip_trigger`]) und die Kostenrechnung
//! ([`calc_cost_usd`]). Der async Orchestrator (`EngagementPipeline.handle`)
//! folgt in späteren Slices, sobald die Provider portiert sind.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;
use sqlx::PgPool;
use tb_llm::brain_route::{
    answered_text, ChatLine, ChatPersona, ConversationPort, ConversationTask,
    InstalledConversation, StreamContext, MAX_CHAT_LINES, MAX_CHAT_LINE_CHARS,
};
use tb_llm::LlmError;

use crate::conversation::{ConversationBuffer, ConversationTurn};
use crate::gate;
use crate::llm_chat::{sanitize_test_mode_text, PersonaMode};
use crate::rhythm::RhythmGuard;
use crate::smalltalk_loop_store::{GeneratedOutcome, SmalltalkLoopStore};
use crate::stream_state::StreamState;
use crate::types::{Decision, EngagementSettings, HandleResult, IncomingMessage, OutputMode};

/// Billiger Pre-Filter ohne Modell-Call: Nachrichten, auf die ein Zuschauer nie
/// antworten würde, fliegen sofort raus (Python `_should_skip_trigger`).
///
/// (1) führendes `@name`/`!command` → an eine Person/einen Bot gerichtet;
/// (2) ein einzelnes Token ohne `?` → Emote/Reaktionswort (Ein-Wort-Fragen
/// gehen bewusst weiter); (3) dasselbe Token mehrfach → Emote-/Cheer-Spam.
pub fn should_skip_trigger(content: &str) -> bool {
    let text = content.trim();
    if text.is_empty() {
        return true;
    }
    match text.chars().next() {
        Some('@') | Some('!') => return true,
        _ => {}
    }
    let tokens: Vec<&str> = text.split_whitespace().collect();
    if tokens.len() == 1 && !text.ends_with('?') {
        return true;
    }
    if tokens.len() >= 2 && tokens.iter().collect::<HashSet<_>>().len() == 1 {
        return true;
    }
    false
}

const PRICE_INPUT_PER_1K: f64 = 0.0008;
const PRICE_OUTPUT_PER_1K: f64 = 0.0024;

/// Geschätzte KI-Kosten in USD (Python `_calc_cost_usd`). Fehlen Token-Zahlen → None.
pub fn calc_cost_usd(prompt_tokens: Option<i64>, completion_tokens: Option<i64>) -> Option<f64> {
    let pt = prompt_tokens?;
    let ct = completion_tokens?;
    Some((pt as f64 / 1000.0) * PRICE_INPUT_PER_1K + (ct as f64 / 1000.0) * PRICE_OUTPUT_PER_1K)
}

/// Erstes Wort, kleingeschrieben, ohne `.,!?` am Ende (für den Starter-Repeat-Guard).
fn first_token_norm(s: &str) -> String {
    s.split_whitespace()
        .next()
        .map(|w| {
            w.to_lowercase()
                .trim_end_matches(['.', ',', '!', '?'])
                .to_string()
        })
        .unwrap_or_default()
}

const ASSISTANT_AUTHOR: &str = "bot";

fn chat_persona() -> ChatPersona {
    match PersonaMode::from_env() {
        PersonaMode::Veteran => ChatPersona::Veteran,
        PersonaMode::Rookie => ChatPersona::Rookie,
    }
}

fn chat_line(turn: &ConversationTurn) -> Option<ChatLine> {
    let text: String = turn
        .content
        .trim()
        .chars()
        .take(MAX_CHAT_LINE_CHARS)
        .collect();
    if text.is_empty() {
        return None;
    }
    let author = if turn.role == "user" {
        turn.twitch_login
            .as_deref()
            .map(str::trim)
            .filter(|a| !a.is_empty())?
    } else {
        ASSISTANT_AUTHOR
    };
    Some(ChatLine {
        author: author.to_string(),
        text,
    })
}

fn recent_chat_lines(turns: &[ConversationTurn], message_id: Option<&str>) -> Vec<ChatLine> {
    let earlier = match turns.last() {
        Some(last) if message_id.is_some() && last.message_id.as_deref() == message_id => {
            &turns[..turns.len() - 1]
        }
        _ => turns,
    };
    let lines: Vec<ChatLine> = earlier.iter().filter_map(chat_line).collect();
    let skip = lines.len().saturating_sub(MAX_CHAT_LINES);
    lines.into_iter().skip(skip).collect()
}

fn mentions_tabu_topic(text: &str, tabu_topics: &[String]) -> bool {
    let text = text.to_lowercase();
    tabu_topics
        .iter()
        .map(|topic| topic.trim().to_lowercase())
        .any(|topic| !topic.is_empty() && text.contains(&topic))
}

fn smalltalk_task(
    msg: &IncomingMessage,
    turns: &[ConversationTurn],
    talkative: bool,
) -> ConversationTask {
    ConversationTask::TwitchChat {
        stream: StreamContext {
            streamer_login: msg.channel_login.clone(),
            game: None,
            title: None,
            deadlock_live: true,
        },
        author: msg.twitch_login.clone(),
        message: msg.content.clone(),
        recent_messages: recent_chat_lines(turns, msg.message_id.as_deref()),
        persona: chat_persona(),
        talkative,
    }
}

/// Der Engagement-Orchestrator: hält die Gates und lässt die Antwort pro
/// eingehender Chat-Message vom Brain formulieren.
pub struct EngagementPipeline {
    pool: PgPool,
    conversation: ConversationBuffer,
    rhythm: RhythmGuard,
    brain: Arc<dyn ConversationPort>,
    stream_state: StreamState,
}

impl EngagementPipeline {
    pub fn new(pool: PgPool) -> Self {
        Self {
            conversation: ConversationBuffer::new(pool.clone()),
            rhythm: RhythmGuard::new(None, None, None),
            brain: Arc::new(InstalledConversation),
            stream_state: StreamState::new(pool.clone()),
            pool,
        }
    }

    pub fn with_conversation(mut self, brain: Arc<dyn ConversationPort>) -> Self {
        self.brain = brain;
        self
    }

    async fn record_smalltalk_provider_error(&self, channel: &str, reason: &'static str) {
        let store = SmalltalkLoopStore::new(self.pool.clone());
        if let Err(error) = store.record_provider_error(channel, reason).await {
            tracing::warn!(
                event = "smalltalk_loop.provider_error_persist_failed",
                channel = %channel,
                reason,
                %error,
            );
        }
    }

    async fn ask_brain(
        &self,
        msg: &IncomingMessage,
        task: ConversationTask,
        smalltalk_mode: bool,
    ) -> Result<Option<String>, ()> {
        let call = self.brain.converse(&msg.channel_user_id, task);
        let reply = if smalltalk_mode {
            match tokio::time::timeout(Duration::from_secs(7), call).await {
                Ok(reply) => reply,
                Err(_) => {
                    self.record_smalltalk_provider_error(&msg.channel_login, "deadline_7s")
                        .await;
                    tracing::warn!(
                        event = "smalltalk_loop.model_deadline",
                        channel = %msg.channel_login,
                        deadline_ms = 7000,
                    );
                    return Err(());
                }
            }
        } else {
            call.await
        };
        match reply {
            Ok(reply) => Ok(answered_text(reply)),
            Err(error) => {
                let reason = match error {
                    LlmError::Unavailable(_) => "unavailable",
                    _ => "generate_error",
                };
                if smalltalk_mode {
                    self.record_smalltalk_provider_error(&msg.channel_login, reason)
                        .await;
                }
                tracing::warn!(
                    code = error.code(),
                    "Engagement: Brain-Antwort fehlgeschlagen"
                );
                Err(())
            }
        }
    }

    /// Verarbeitet eine Nachricht und loggt die Entscheidung (außer DISABLED).
    pub async fn handle(&self, msg: &IncomingMessage) -> HandleResult {
        let result = self.handle_inner(msg).await;
        if result.decision != Decision::Disabled {
            let cost = calc_cost_usd(result.prompt_tokens, result.completion_tokens);
            gate::log_decision(
                &self.pool,
                &msg.channel_login,
                msg.message_id.as_deref(),
                &result,
                cost,
            )
            .await;
        }
        result
    }

    async fn handle_inner(&self, msg: &IncomingMessage) -> HandleResult {
        // --- Gate-Kaskade ---
        let settings =
            match gate::load_settings(&self.pool, &msg.channel_login, Some(&msg.channel_user_id))
                .await
            {
                Some(s) if s.enabled => s,
                _ => return HandleResult::new(Decision::Disabled),
            };
        let smalltalk_mode = matches!(
            settings.output_mode,
            OutputMode::Test | OutputMode::SmalltalkLive
        );
        let result = self.handle_gated(msg, settings).await;
        // Der Smalltalk-Testmodus misst, ob der Bot in einem fremden Kanal
        // mitreden *könnte*. Bricht ein Gate vor der Generierung ab, entstand
        // bisher keine einzige Zeile — Stille im Journal war dann nicht von
        // einem kaputten Pfad zu unterscheiden. Darum bekommt hier JEDE
        // Testmodus-Entscheidung eine Zeile, nicht nur die erfolgreiche
        // (`smalltalk_loop.message_recorded`) oder die fehlgeschlagene.
        // Nachrichtentexte gehören laut Spec in keine Logzeile dieses Features.
        if smalltalk_mode {
            tracing::info!(
                event = "smalltalk_loop.decision",
                channel = %msg.channel_login,
                decision = result.decision.as_str(),
                generated = result.shadow_text.is_some() || result.response_text.is_some(),
            );
        }
        result
    }

    async fn handle_gated(
        &self,
        msg: &IncomingMessage,
        settings: EngagementSettings,
    ) -> HandleResult {
        // Output-Modus-Gate (Block-19-Grillme): `off` = no-op, gar kein KI-Output
        // erzeugen — vor dem teuren Modell-Call abbrechen. Default ist `off`,
        // damit die Engagement-KI ohne expliziten Dashboard-Toggle stumm bleibt.
        // `shadow`/`live` laufen weiter; die Verzweigung "senden vs. staging"
        // passiert nach erfolgreicher Generierung am Ende von handle_inner.
        if settings.output_mode == OutputMode::Off {
            return HandleResult::new(Decision::Disabled);
        }
        let smalltalk_mode = matches!(
            settings.output_mode,
            OutputMode::Test | OutputMode::SmalltalkLive
        );
        if !smalltalk_mode && !gate::is_operational_partner(&self.pool, &msg.channel_login).await {
            return HandleResult::new(Decision::Disabled);
        }
        if !self
            .stream_state
            .is_streaming_deadlock(&msg.channel_login)
            .await
        {
            return HandleResult::new(Decision::Disabled);
        }
        if gate::is_opted_out(&self.pool, &msg.twitch_user_id).await {
            return HandleResult::new(Decision::Optout);
        }

        self.rhythm.note_user_post(&msg.channel_login);
        if self
            .conversation
            .append_user_turn(
                &msg.channel_login,
                &msg.twitch_user_id,
                &msg.twitch_login,
                &msg.content,
                msg.message_id.as_deref(),
            )
            .await
            .is_err()
        {
            return HandleResult::new(Decision::ProviderError);
        }

        // Führendes @name / !command → an eine Person gerichtet, hart überspringen.
        if should_skip_trigger(&msg.content) {
            return HandleResult::new(Decision::Silent);
        }

        let now = Utc::now();
        if !self.rhythm.anti_flood_ok(&msg.channel_login, now) {
            return HandleResult::new(Decision::FloodGuard);
        }
        if !self.rhythm.anti_burst_ok(&msg.channel_login, now) {
            return HandleResult::new(Decision::AntiBurst);
        }

        let history_turns = match self
            .conversation
            .load_recent_buffer(&msg.channel_login, 100)
            .await
        {
            Ok(h) => h,
            Err(_) => return HandleResult::new(Decision::ProviderError),
        };
        let started = Instant::now();
        let task = smalltalk_task(msg, &history_turns, smalltalk_mode);
        let Ok(answer) = self.ask_brain(msg, task, smalltalk_mode).await else {
            return HandleResult::new(Decision::ProviderError);
        };
        let latency_ms = i64::try_from(started.elapsed().as_millis()).ok();
        let silent = HandleResult {
            latency_ms,
            ..HandleResult::new(Decision::Silent)
        };
        let Some(answer) = answer else {
            return silent;
        };
        if mentions_tabu_topic(&answer, &settings.tabu_topics) {
            return silent;
        }

        let test_evaluation = smalltalk_mode.then(|| {
            sanitize_test_mode_text(&answer)
                .map(|text| (text, GeneratedOutcome::WouldSend))
                .unwrap_or_else(|reason| (answer.clone(), GeneratedOutcome::Rejected(reason)))
        });
        let text = test_evaluation
            .as_ref()
            .map_or_else(|| answer.clone(), |(text, _)| text.clone());

        // Starter-Repeat-Guard: gleicher erster Begriff wie letzte Bot-Antwort → still.
        if let Some(last_bot) = history_turns.iter().rev().find(|t| t.role == "assistant") {
            let prev = first_token_norm(&last_bot.content);
            let this = first_token_norm(&text);
            if !prev.is_empty() && prev == this {
                return silent;
            }
        }

        // --- Output-Modus-Verzweigung (Block-19-Grillme) ---
        // `shadow`: die Antwort wurde erzeugt, geht aber NICHT in den Chat. Damit
        // der Live-Kontext nicht verfälscht wird, lösen wir KEINE Sende-Seiten-
        // effekte aus (kein note_bot_post, kein assistant-Turn im Buffer, keine
        // Thread-Referenzierung) — der Bot hat ja nichts gesagt. Der Text wird
        // über das Decision-Log (response_text-Spalte) gestaged; das Discord-
        // Review-Ticket holt ihn dort ab. response_text (= Sendesignal für
        // tb-bot) bleibt bewusst None, shadow_text trägt den Text.
        if smalltalk_mode {
            let outcome = test_evaluation
                .as_ref()
                .map(|(_, outcome)| *outcome)
                .unwrap_or(GeneratedOutcome::WouldSend);
            let store = SmalltalkLoopStore::new(self.pool.clone());
            match store
                .record_generated(
                    &msg.channel_login,
                    msg.message_id.as_deref(),
                    &answer,
                    &msg.content,
                    outcome,
                    Utc::now(),
                )
                .await
            {
                Ok(true) => {}
                Ok(false) => tracing::warn!(
                    event = "smalltalk_loop.message_not_persisted",
                    channel = %msg.channel_login,
                    reason = "missing_session_or_duplicate",
                    ?outcome,
                ),
                Err(error) => tracing::warn!(
                    event = "smalltalk_loop.message_persist_failed",
                    channel = %msg.channel_login,
                    reason = "database",
                    ?outcome,
                    %error,
                ),
            }
            if settings.output_mode == OutputMode::Test
                || matches!(outcome, GeneratedOutcome::Rejected(_))
            {
                return HandleResult {
                    decision: Decision::Tested,
                    shadow_text: Some(text),
                    latency_ms,
                    ..HandleResult::new(Decision::Tested)
                };
            }
            // SmalltalkLive faellt absichtlich in den normalen Live-Zweig durch.
            // Dort werden Rhythmus und Conversation-Memory aktualisiert.
        }

        if settings.output_mode == OutputMode::Shadow {
            return HandleResult {
                decision: Decision::Shadowed,
                shadow_text: Some(text),
                latency_ms,
                ..HandleResult::new(Decision::Shadowed)
            };
        }

        // `live`: normal senden — Sende-Seiteneffekte ausführen.
        self.rhythm.note_bot_post(&msg.channel_login, Utc::now());
        if let Err(error) = self
            .conversation
            .append_assistant_turn(&msg.channel_login, &text)
            .await
        {
            tracing::warn!(
                %error,
                channel = %msg.channel_login,
                "Engagement: Assistant-Turn konnte nicht gespeichert werden"
            );
        }

        HandleResult {
            decision: Decision::Spoke,
            response_text: Some(text),
            latency_ms,
            ..HandleResult::new(Decision::Spoke)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skip_trigger_faelle() {
        assert!(should_skip_trigger("")); // leer
        assert!(should_skip_trigger("   ")); // nur Whitespace
        assert!(should_skip_trigger("@nani hi")); // an Person
        assert!(should_skip_trigger("!title")); // an Bot
        assert!(should_skip_trigger("gg")); // Ein-Wort ohne ?
        assert!(should_skip_trigger("Cheer1 Cheer1 Cheer1")); // wiederholtes Token
                                                              // Durchgelassen:
        assert!(!should_skip_trigger("haze?")); // Ein-Wort-Frage
        assert!(!should_skip_trigger("was haltet ihr von haze")); // Mehrwort
        assert!(!should_skip_trigger("gg wp")); // zwei verschiedene Tokens
    }

    #[test]
    fn cost_none_ohne_tokens() {
        assert_eq!(calc_cost_usd(None, Some(10)), None);
        assert_eq!(calc_cost_usd(Some(10), None), None);
    }

    #[test]
    fn cost_default_raten() {
        let cost = calc_cost_usd(Some(1000), Some(1000)).unwrap();
        assert!((cost - 0.0032).abs() < 1e-9);
    }

    use async_trait::async_trait;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;
    use tb_llm::brain_route::{BotReply, SilentReason};

    struct FixedBrain(Option<String>);

    #[async_trait]
    impl ConversationPort for FixedBrain {
        async fn converse(
            &self,
            _broadcaster_id: &str,
            _task: ConversationTask,
        ) -> Result<BotReply, LlmError> {
            Ok(match &self.0 {
                Some(text) => BotReply::Answered {
                    text: text.clone(),
                    signals: Vec::new(),
                    citations: Vec::new(),
                    artifacts: Vec::new(),
                },
                None => BotReply::Silent {
                    reason: SilentReason::NoOccasion,
                },
            })
        }
    }

    struct DownBrain;

    #[async_trait]
    impl ConversationPort for DownBrain {
        async fn converse(
            &self,
            _broadcaster_id: &str,
            _task: ConversationTask,
        ) -> Result<BotReply, LlmError> {
            Err(LlmError::Unavailable("down".to_string()))
        }
    }

    fn turn(role: &str, login: Option<&str>, content: &str, id: Option<&str>) -> ConversationTurn {
        ConversationTurn {
            role: role.to_string(),
            twitch_user_id: None,
            twitch_login: login.map(str::to_string),
            content: content.to_string(),
            message_id: id.map(str::to_string),
            ts: Utc::now(),
        }
    }

    #[test]
    fn verlauf_laesst_die_aktuelle_nachricht_und_leere_zeilen_weg() {
        let turns = vec![
            turn("user", Some("a"), "hallo", Some("m0")),
            turn("assistant", None, "moin", None),
            turn("user", None, "ohne autor", Some("m1")),
            turn("user", Some("b"), "  ", Some("m2")),
            turn("user", Some("chatter"), "jetzt", Some("m3")),
        ];
        let lines = recent_chat_lines(&turns, Some("m3"));
        let got: Vec<(&str, &str)> = lines
            .iter()
            .map(|l| (l.author.as_str(), l.text.as_str()))
            .collect();
        assert_eq!(got, vec![("a", "hallo"), (ASSISTANT_AUTHOR, "moin")]);
    }

    #[test]
    fn verlauf_haelt_die_vertragsgrenzen_ein() {
        let long = "x".repeat(MAX_CHAT_LINE_CHARS + 5);
        let turns: Vec<ConversationTurn> = (0..MAX_CHAT_LINES + 5)
            .map(|_| turn("user", Some("a"), &long, None))
            .collect();
        let lines = recent_chat_lines(&turns, None);
        assert_eq!(lines.len(), MAX_CHAT_LINES);
        assert!(lines
            .iter()
            .all(|l| l.text.chars().count() == MAX_CHAT_LINE_CHARS));
    }

    #[test]
    fn tabu_thema_wird_ohne_gross_kleinschreibung_erkannt() {
        let tabu = vec!["Politik".to_string(), " ".to_string()];
        assert!(mentions_tabu_topic("die politik nervt", &tabu));
        assert!(!mentions_tabu_topic("haze ist stark", &tabu));
    }

    async fn make_pool(schema: &str) -> Option<PgPool> {
        const SMALLTALK_MIGRATION: &str =
            include_str!("../../../migrations/20260727150000_twitch_smalltalk_loop.sql");
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .unwrap();
        sqlx::query(crate::test_sql::drop_schema(schema, true))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(crate::test_sql::create_schema(schema, false))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn)
            .unwrap()
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(3)
            .connect_with(opts)
            .await
            .unwrap();
        for ddl in [
            "CREATE TABLE twitch_engagement_settings (channel_login TEXT PRIMARY KEY, channel_user_id TEXT, enabled BOOLEAN NOT NULL DEFAULT FALSE, steam_id TEXT, persona_override TEXT, tabu_topics TEXT[], irc_read BOOLEAN NOT NULL DEFAULT FALSE, output_mode TEXT NOT NULL DEFAULT 'off')",
            "CREATE TABLE twitch_streamers_partner_state (twitch_login TEXT, is_partner_active INTEGER)",
            "CREATE TABLE twitch_live_state (twitch_user_id TEXT PRIMARY KEY, streamer_login TEXT NOT NULL, is_live INTEGER DEFAULT 0, last_game TEXT)",
            "CREATE TABLE twitch_user_engagement_optout (twitch_user_id TEXT PRIMARY KEY, opted_out_at TIMESTAMPTZ DEFAULT NOW())",
            "CREATE TABLE twitch_engagement_conversation (id BIGSERIAL PRIMARY KEY, channel_login TEXT, role TEXT, twitch_user_id TEXT, twitch_login TEXT, content TEXT, message_id TEXT, ts TIMESTAMPTZ NOT NULL DEFAULT NOW())",
            "CREATE TABLE twitch_engagement_log (id BIGSERIAL PRIMARY KEY, channel_login TEXT NOT NULL, triggered_by_msg_id TEXT, decision TEXT NOT NULL, response_text TEXT, referenced_thread_ids BIGINT[], model TEXT NOT NULL, prompt_tokens INT, completion_tokens INT, cost_usd_estimate DOUBLE PRECISION, latency_ms INT, created_at TIMESTAMPTZ NOT NULL DEFAULT NOW())",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        sqlx::raw_sql(
            "CREATE TABLE twitch_partner_outreach (
                streamer_login TEXT PRIMARY KEY,
                cooldown_until TEXT
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::raw_sql(SMALLTALK_MIGRATION)
            .execute(&pool)
            .await
            .unwrap();
        // Auch die nachfolgende Schema-Erweiterung gehört zur Testbasis.
        // Die Tests dürfen nicht gegen den historischen Modus-Constraint laufen.
        sqlx::raw_sql(include_str!(
            "../../../migrations/20260916013000_smalltalk_live_mode.sql"
        ))
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    fn pipeline_with(pool: PgPool, answer: &str) -> EngagementPipeline {
        EngagementPipeline::new(pool)
            .with_conversation(Arc::new(FixedBrain(Some(answer.to_string()))))
    }

    fn msg() -> IncomingMessage {
        IncomingMessage {
            channel_login: "nani".to_string(),
            channel_user_id: "c1".to_string(),
            twitch_user_id: "u1".to_string(),
            twitch_login: "chatter".to_string(),
            content: "lohnt sich trophy collector auf haze".to_string(),
            message_id: Some("m1".to_string()),
        }
    }

    /// Führt `fut` mit einem eigenen tracing-Subscriber aus und gibt die
    /// gesammelten Logzeilen zurück. `with_subscriber` statt `with_default`,
    /// damit der Dispatcher auch über `await`-Punkte gilt.
    async fn capture_logs<F: std::future::Future>(fut: F) -> (F::Output, String) {
        use std::sync::{Arc, Mutex};
        use tracing::instrument::WithSubscriber;

        #[derive(Clone)]
        struct LogBuf(Arc<Mutex<Vec<u8>>>);
        impl std::io::Write for LogBuf {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let buf = LogBuf(Arc::new(Mutex::new(Vec::new())));
        let writer = buf.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(move || writer.clone())
            .with_max_level(tracing::Level::INFO)
            .with_ansi(false)
            .finish();
        let out = fut.with_subscriber(subscriber).await;
        let logs = String::from_utf8_lossy(&buf.0.lock().unwrap()).into_owned();
        (out, logs)
    }

    /// Der Testmodus misst, ob der Bot mitreden *könnte*. Bricht ein Gate vor
    /// der Generierung ab, gab es bisher keine einzige Zeile — Stille sah dann
    /// aus wie "nichts los", war aber womöglich ein kaputter Pfad. Jede
    /// Testmodus-Entscheidung loggt jetzt, ohne je den Nachrichtentext zu
    /// zeigen.
    #[tokio::test]
    async fn testmodus_loggt_auch_ohne_generierung() {
        let Some(pool) = make_pool("t_eng_pipe_test_log").await else {
            return;
        };
        sqlx::query("INSERT INTO twitch_engagement_settings (channel_login, enabled, output_mode) VALUES ('nani', TRUE, 'test')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_live_state (twitch_user_id, streamer_login, is_live, last_game) VALUES ('1','nani',1,'Deadlock')").execute(&pool).await.unwrap();

        let pipe = pipeline_with(pool, "unbenutzt");
        let mut m = msg();
        // Führendes @ → skip-Trigger → SILENT, lange vor jedem Modell-Call.
        m.content = "@someone zwiebelkuchen".to_string();
        let (r, logs) = capture_logs(pipe.handle(&m)).await;

        assert_eq!(r.decision, Decision::Silent);
        assert!(
            logs.contains("smalltalk_loop.decision"),
            "keine Entscheidungszeile: {logs}"
        );
        assert!(
            logs.contains("decision=\"silent\""),
            "Entscheidung fehlt: {logs}"
        );
        assert!(
            logs.contains("generated=false"),
            "Generierungsflag fehlt: {logs}"
        );
        assert!(
            !logs.contains("zwiebelkuchen"),
            "Nachrichtentext darf nie ins Log: {logs}"
        );
    }

    /// Andere Output-Modi bleiben unberührt — die Zeile ist rein fürs Testmodus-
    /// Messprotokoll und darf den Live-/Shadow-Betrieb nicht zuspammen.
    #[tokio::test]
    async fn andere_modi_loggen_keine_testmodus_zeile() {
        let Some(pool) = make_pool("t_eng_pipe_live_nolog").await else {
            return;
        };
        sqlx::query("INSERT INTO twitch_engagement_settings (channel_login, enabled, output_mode) VALUES ('nani', TRUE, 'live')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_streamers_partner_state (twitch_login, is_partner_active) VALUES ('nani', 1)").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_live_state (twitch_user_id, streamer_login, is_live, last_game) VALUES ('1','nani',1,'Deadlock')").execute(&pool).await.unwrap();

        let pipe = pipeline_with(pool, "unbenutzt");
        let mut m = msg();
        m.content = "@someone zwiebelkuchen".to_string();
        let (r, logs) = capture_logs(pipe.handle(&m)).await;

        assert_eq!(r.decision, Decision::Silent);
        assert!(
            !logs.contains("smalltalk_loop.decision"),
            "live loggt keine Testzeile: {logs}"
        );
    }

    #[tokio::test]
    async fn disabled_wenn_settings_aus() {
        let Some(pool) = make_pool("t_eng_pipe_disabled").await else {
            return;
        };
        // Kein Settings-Eintrag → DISABLED (keine weiteren Gates/KI nötig).
        let pipe = pipeline_with(pool, "unbenutzt");
        let r = pipe.handle(&msg()).await;
        assert_eq!(r.decision, Decision::Disabled);
    }

    #[tokio::test]
    async fn spoke_voller_pfad() {
        let Some(pool) = make_pool("t_eng_pipe_spoke").await else {
            return;
        };
        // output_mode='live' → senden (Default wäre 'off' = no-op).
        sqlx::query("INSERT INTO twitch_engagement_settings (channel_login, enabled, output_mode) VALUES ('nani', TRUE, 'live')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_streamers_partner_state (twitch_login, is_partner_active) VALUES ('nani', 1)").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_live_state (twitch_user_id, streamer_login, is_live, last_game) VALUES ('1','nani',1,'Deadlock')").execute(&pool).await.unwrap();

        let pipe = pipeline_with(pool.clone(), "klar haze, stark !clip");
        let r = pipe.handle(&msg()).await;
        assert_eq!(r.decision, Decision::Spoke);
        assert_eq!(r.response_text.as_deref(), Some("klar haze, stark !clip"));
        assert!(r.shadow_text.is_none(), "live setzt shadow_text nicht");
        // User- + Assistant-Turn persistiert.
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_engagement_conversation")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(n, 2);
        // Decision geloggt.
        let dec: String = sqlx::query_scalar("SELECT decision FROM twitch_engagement_log LIMIT 1")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(dec, "spoke");
    }

    /// output_mode='shadow': die Antwort wird ERZEUGT, aber NICHT gesendet
    /// (response_text leer → tb-bot sendet nicht). Der Text steht in shadow_text
    /// und wird ins Decision-Log (response_text-Spalte) gestaged. KEINE
    /// Sende-Seiteneffekte: der assistant-Turn landet NICHT im Buffer (nur der
    /// user-Turn), damit der Live-Kontext nicht verfälscht wird.
    #[tokio::test]
    async fn shadow_erzeugt_aber_sendet_nicht() {
        let Some(pool) = make_pool("t_eng_pipe_shadow").await else {
            return;
        };
        sqlx::query("INSERT INTO twitch_engagement_settings (channel_login, enabled, output_mode) VALUES ('nani', TRUE, 'shadow')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_streamers_partner_state (twitch_login, is_partner_active) VALUES ('nani', 1)").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_live_state (twitch_user_id, streamer_login, is_live, last_game) VALUES ('1','nani',1,'Deadlock')").execute(&pool).await.unwrap();

        let pipe = pipeline_with(pool.clone(), "shadow-antwort nur fuer review");
        let r = pipe.handle(&msg()).await;
        assert_eq!(r.decision, Decision::Shadowed);
        // Kein Sendesignal für tb-bot.
        assert!(
            r.response_text.is_none(),
            "shadow darf response_text NICHT setzen (kein Senden)"
        );
        assert_eq!(
            r.shadow_text.as_deref(),
            Some("shadow-antwort nur fuer review")
        );
        // Nur der user-Turn im Buffer — KEIN assistant-Turn (Bot hat nichts gesagt).
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_engagement_conversation")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(n, 1, "shadow appended keinen assistant-Turn");
        // Decision='shadowed' geloggt, Text als Staging in response_text-Spalte.
        let row: (String, Option<String>) =
            sqlx::query_as("SELECT decision, response_text FROM twitch_engagement_log LIMIT 1")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(row.0, "shadowed");
        assert_eq!(row.1.as_deref(), Some("shadow-antwort nur fuer review"));
    }

    /// `test` überspringt nur das Partner-Gate. Die Antwort bleibt strukturell
    /// vom Twitch-Sendepfad getrennt: `response_text` ist immer leer.
    #[tokio::test]
    async fn testmodus_fremdkanal_erzeugt_aber_sendet_nicht() {
        let Some(pool) = make_pool("t_eng_pipe_test").await else {
            return;
        };
        sqlx::query("INSERT INTO twitch_engagement_settings (channel_login, enabled, output_mode) VALUES ('nani', TRUE, 'test')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_live_state (twitch_user_id, streamer_login, is_live, last_game) VALUES ('1','nani',1,'Deadlock')").execute(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO twitch_smalltalk_sessions
                (id, channel_login, streamer_user_id, started_at, viewer_count,
                 settings_existed, previous_enabled, previous_irc_read,
                 previous_output_mode)
             VALUES ($1, 'nani', '1', NOW(), 10, TRUE, FALSE, FALSE, 'off')",
        )
        .bind(uuid::Uuid::new_v4())
        .execute(&pool)
        .await
        .unwrap();

        let r = pipeline_with(pool.clone(), "fremdkanal antwort")
            .handle(&msg())
            .await;
        // Bewusst NICHT `Shadowed`: der bestehende Shadow-Review forwardet
        // jede Zeile mit `decision='shadowed'` nach Discord. Waere der
        // Testmodus dort eingereiht, laege jede Smalltalk-Antwort zusaetzlich
        // im Partner-Review und wuerde dort wie ein Partner-Vorschlag wirken.
        assert_eq!(r.decision, Decision::Tested);
        assert!(
            r.response_text.is_none(),
            "Testmodus darf nie das Twitch-Sendesignal setzen"
        );
        assert_eq!(r.shadow_text.as_deref(), Some("fremdkanal antwort"));
        let assistant_turns: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM twitch_engagement_conversation WHERE role = 'assistant'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            assistant_turns, 0,
            "Testmodus führt keine Sende-Seiteneffekte aus"
        );
        let saved: (String, String, String) = sqlx::query_as(
            "SELECT generated_text, trigger_text, outcome FROM twitch_smalltalk_messages",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            saved,
            (
                "fremdkanal antwort".to_string(),
                "lohnt sich trophy collector auf haze".to_string(),
                "would_send".to_string(),
            )
        );
    }

    #[tokio::test]
    async fn smalltalk_live_fremdkanal_setzt_send_signal_aber_nutzt_testfilter() {
        let Some(pool) = make_pool("t_eng_pipe_smalltalk_live").await else {
            return;
        };
        sqlx::query("INSERT INTO twitch_engagement_settings (channel_login, enabled, output_mode) VALUES ('nani', TRUE, 'smalltalk_live')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_live_state (twitch_user_id, streamer_login, is_live, last_game) VALUES ('1','nani',1,'Deadlock')").execute(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO twitch_smalltalk_sessions
                (id, channel_login, streamer_user_id, started_at, viewer_count,
                 settings_existed, previous_enabled, previous_irc_read,
                 previous_output_mode)
             VALUES ($1, 'nani', '1', NOW(), 10, TRUE, FALSE, FALSE, 'off')",
        )
        .bind(uuid::Uuid::new_v4())
        .execute(&pool)
        .await
        .unwrap();

        let r = pipeline_with(pool.clone(), "jo das war knapp")
            .handle(&msg())
            .await;
        assert_eq!(r.decision, Decision::Spoke);
        assert_eq!(r.response_text.as_deref(), Some("jo das war knapp"));
        assert!(r.shadow_text.is_none());
        let assistant_turns: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM twitch_engagement_conversation WHERE role = 'assistant'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            assistant_turns, 1,
            "echter Smalltalk muss im Verlauf landen"
        );
    }

    #[tokio::test]
    async fn smalltalk_live_blockiert_pitch_text_vor_send_signal() {
        let Some(pool) = make_pool("t_eng_pipe_smalltalk_live_pitch_block").await else {
            return;
        };
        sqlx::query("INSERT INTO twitch_engagement_settings (channel_login, enabled, output_mode) VALUES ('nani', TRUE, 'smalltalk_live')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_live_state (twitch_user_id, streamer_login, is_live, last_game) VALUES ('1','nani',1,'Deadlock')").execute(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO twitch_smalltalk_sessions
                (id, channel_login, streamer_user_id, started_at, viewer_count,
                 settings_existed, previous_enabled, previous_irc_read,
                 previous_output_mode)
             VALUES ($1, 'nani', '1', NOW(), 10, TRUE, FALSE, FALSE, 'off')",
        )
        .bind(uuid::Uuid::new_v4())
        .execute(&pool)
        .await
        .unwrap();

        let r = pipeline_with(pool.clone(), "komm auf Discord")
            .handle(&msg())
            .await;
        assert_eq!(r.decision, Decision::Tested);
        assert!(
            r.response_text.is_none(),
            "Pitch-Wort darf nie zum Sender gelangen"
        );
        let saved: (String, Option<String>) =
            sqlx::query_as("SELECT outcome, reject_reason FROM twitch_smalltalk_messages LIMIT 1")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(saved.0, "rejected");
        assert_eq!(saved.1.as_deref(), Some("offer_or_link"));
    }

    #[tokio::test]
    async fn testmodus_speichert_verworfenen_text_mit_grund() {
        let Some(pool) = make_pool("t_eng_pipe_test_rejected").await else {
            return;
        };
        sqlx::query("INSERT INTO twitch_engagement_settings (channel_login, enabled, output_mode) VALUES ('nani', TRUE, 'test')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_live_state (twitch_user_id, streamer_login, is_live, last_game) VALUES ('1','nani',1,'Deadlock')").execute(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO twitch_smalltalk_sessions
                (id, channel_login, streamer_user_id, started_at, viewer_count,
                 settings_existed, previous_enabled, previous_irc_read,
                 previous_output_mode)
             VALUES ($1, 'nani', '1', NOW(), 10, TRUE, FALSE, FALSE, 'off')",
        )
        .bind(uuid::Uuid::new_v4())
        .execute(&pool)
        .await
        .unwrap();
        let r = pipeline_with(pool.clone(), "komm auf Discord")
            .handle(&msg())
            .await;
        assert!(r.response_text.is_none());
        assert_eq!(r.shadow_text.as_deref(), Some("komm auf Discord"));
        let saved: (String, String, Option<String>) = sqlx::query_as(
            "SELECT generated_text, outcome, reject_reason FROM twitch_smalltalk_messages",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            saved,
            (
                "komm auf Discord".to_string(),
                "rejected".to_string(),
                Some("offer_or_link".to_string()),
            )
        );
    }

    /// output_mode='off' bei enabled=TRUE: no-op. Kein KI-Call (Mock würde
    /// sonst zünden), kein Output, Decision=Disabled → kein Log-Eintrag.
    #[tokio::test]
    async fn off_ist_noop_kein_call() {
        let Some(pool) = make_pool("t_eng_pipe_off").await else {
            return;
        };
        sqlx::query("INSERT INTO twitch_engagement_settings (channel_login, enabled, output_mode) VALUES ('nani', TRUE, 'off')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_streamers_partner_state (twitch_login, is_partner_active) VALUES ('nani', 1)").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_live_state (twitch_user_id, streamer_login, is_live, last_game) VALUES ('1','nani',1,'Deadlock')").execute(&pool).await.unwrap();

        let pipe = EngagementPipeline::new(pool.clone()).with_conversation(Arc::new(DownBrain));
        let r = pipe.handle(&msg()).await;
        assert_eq!(r.decision, Decision::Disabled);
        assert!(r.response_text.is_none() && r.shadow_text.is_none());
        // Kein user-Turn (Abbruch vor dem Buffer-Append) und kein Log.
        let conv: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_engagement_conversation")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(conv, 0);
        let logs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_engagement_log")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(logs, 0, "Disabled wird nicht geloggt");
    }

    #[tokio::test]
    async fn silent_bei_skip_trigger() {
        let Some(pool) = make_pool("t_eng_pipe_silent").await else {
            return;
        };
        sqlx::query("INSERT INTO twitch_engagement_settings (channel_login, enabled, output_mode) VALUES ('nani', TRUE, 'live')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_streamers_partner_state (twitch_login, is_partner_active) VALUES ('nani', 1)").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_live_state (twitch_user_id, streamer_login, is_live, last_game) VALUES ('1','nani',1,'Deadlock')").execute(&pool).await.unwrap();

        let pipe = pipeline_with(pool.clone(), "unbenutzt");
        let mut m = msg();
        m.content = "@someone hi".to_string(); // führendes @ → skip → SILENT (kein KI)
        let r = pipe.handle(&m).await;
        assert_eq!(r.decision, Decision::Silent);
        // User-Turn trotzdem im Buffer (für Kontext), aber keine Bot-Antwort.
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_engagement_conversation")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(n, 1);
    }

    async fn live_channel(schema: &str) -> Option<PgPool> {
        let pool = make_pool(schema).await?;
        sqlx::query("INSERT INTO twitch_engagement_settings (channel_login, enabled, output_mode, tabu_topics) VALUES ('nani', TRUE, 'live', ARRAY['politik'])").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_streamers_partner_state (twitch_login, is_partner_active) VALUES ('nani', 1)").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_live_state (twitch_user_id, streamer_login, is_live, last_game) VALUES ('1','nani',1,'Deadlock')").execute(&pool).await.unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn brain_silent_sendet_nichts() {
        let Some(pool) = live_channel("t_eng_pipe_brain_silent").await else {
            return;
        };
        let pipe = EngagementPipeline::new(pool).with_conversation(Arc::new(FixedBrain(None)));
        let r = pipe.handle(&msg()).await;
        assert_eq!(r.decision, Decision::Silent);
        assert!(r.response_text.is_none());
    }

    #[tokio::test]
    async fn brain_ausfall_sendet_keinen_ersatztext() {
        let Some(pool) = live_channel("t_eng_pipe_brain_down").await else {
            return;
        };
        let pipe = EngagementPipeline::new(pool).with_conversation(Arc::new(DownBrain));
        let r = pipe.handle(&msg()).await;
        assert_eq!(r.decision, Decision::ProviderError);
        assert!(r.response_text.is_none());
    }

    #[tokio::test]
    async fn tabu_thema_in_der_antwort_bleibt_still() {
        let Some(pool) = live_channel("t_eng_pipe_brain_tabu").await else {
            return;
        };
        let r = pipeline_with(pool, "reden wir über Politik")
            .handle(&msg())
            .await;
        assert_eq!(r.decision, Decision::Silent);
        assert!(r.response_text.is_none());
    }
}
