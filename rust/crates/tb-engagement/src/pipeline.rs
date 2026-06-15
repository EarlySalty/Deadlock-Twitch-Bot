//! Engagement-Pipeline — reine Helfer (Slice 1).
//!
//! Port der I/O-freien Logik aus `bot/engagement/pipeline.py`: der billige
//! Pre-Filter ([`should_skip_trigger`]) und die Kostenrechnung
//! ([`calc_cost_usd`]). Der async Orchestrator (`EngagementPipeline.handle`)
//! folgt in späteren Slices, sobald die Provider portiert sind.

use std::collections::HashSet;

use chrono::Utc;
use sqlx::PgPool;

use crate::channel_background::ChannelBackground;
use crate::conversation::ConversationBuffer;
use crate::deadlock_patches::DeadlockPatches;
use crate::deadlock_stats::DeadlockStats;
use crate::deadlock_wiki::DeadlockWiki;
use crate::gate;
use crate::global_sentiment::{self, GlobalSentiment};
use crate::lurker_signal::{lurker_hint_to_prompt_fragment, LurkerSignal};
use crate::match_context::MatchContext;
use crate::minimax_chat::{
    build_baseline_system_prompt, ChatMessage, EngagementMinimaxClient, GenerateError,
};
use crate::persona::Persona;
use crate::rhythm::RhythmGuard;
use crate::soul_store::SoulStore;
use crate::stream_state::StreamState;
use crate::stream_transcripts::{segments_to_prompt_fragment, StreamTranscripts};
use crate::style_examples::StyleExamples;
use crate::threads::{threads_to_prompt_fragment, Threads};
use crate::types::{Decision, HandleResult, IncomingMessage};

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

/// Geschätzte MiniMax-Kosten in USD (Python `_calc_cost_usd`). Fehlen Token-
/// Zahlen → None. Die Raten kommen aus Env (`MINIMAX_PRICE_INPUT/OUTPUT_PER_1K`);
/// ist eine gesetzte Rate unparsebar, fallen wie in Python BEIDE auf die
/// Defaults zurück.
pub fn calc_cost_usd(prompt_tokens: Option<i64>, completion_tokens: Option<i64>) -> Option<f64> {
    let pt = prompt_tokens?;
    let ct = completion_tokens?;
    let (input_rate, output_rate) = match (
        parse_rate("MINIMAX_PRICE_INPUT_PER_1K", 0.0008),
        parse_rate("MINIMAX_PRICE_OUTPUT_PER_1K", 0.0024),
    ) {
        (Some(i), Some(o)) => (i, o),
        _ => (0.0008, 0.0024),
    };
    Some((pt as f64 / 1000.0) * input_rate + (ct as f64 / 1000.0) * output_rate)
}

/// `float(os.getenv(var, default))`-Semantik: ungesetzt → Default; gesetzt +
/// parsebar → Wert; gesetzt + unparsebar → None (löst den Beide-Defaults-Pfad
/// aus, wie Pythons `except ValueError`).
fn parse_rate(var: &str, default: f64) -> Option<f64> {
    match std::env::var(var) {
        Ok(v) => v.trim().parse::<f64>().ok(),
        Err(_) => Some(default),
    }
}

/// Erstes Wort, kleingeschrieben, ohne `.,!?` am Ende (für den Starter-Repeat-Guard).
fn first_token_norm(s: &str) -> String {
    s.split_whitespace()
        .next()
        .map(|w| w.to_lowercase().trim_end_matches(['.', ',', '!', '?']).to_string())
        .unwrap_or_default()
}

/// Hängt ein Fragment mit Leerzeile an, falls nicht leer.
fn append_fragment(prompt: &mut String, fragment: &str) {
    if !fragment.is_empty() {
        prompt.push_str("\n\n");
        prompt.push_str(fragment);
    }
}

/// Der Engagement-Orchestrator: hält alle Provider und verarbeitet pro
/// eingehender Chat-Message die Gate-Kaskade + Prompt-Anreicherung +
/// Modell-Call (Port von `EngagementPipeline`).
pub struct EngagementPipeline {
    pool: PgPool,
    conversation: ConversationBuffer,
    rhythm: RhythmGuard,
    minimax: EngagementMinimaxClient,
    wiki: DeadlockWiki,
    stats: DeadlockStats,
    patches: DeadlockPatches,
    persona: Persona,
    style: StyleExamples,
    soul: SoulStore,
    channel_bg: ChannelBackground,
    match_ctx: MatchContext,
    transcripts: StreamTranscripts,
    sentiment: GlobalSentiment,
    lurker: LurkerSignal,
    threads: Threads,
    stream_state: StreamState,
}

impl EngagementPipeline {
    /// Baut die Pipeline. Die drei HTTP-Provider (wiki/stats/patches) werden
    /// injiziert (Tests/Defaults); die DB-Provider entstehen aus dem Pool.
    pub fn new(
        pool: PgPool,
        minimax: EngagementMinimaxClient,
        wiki: DeadlockWiki,
        stats: DeadlockStats,
        patches: DeadlockPatches,
    ) -> Self {
        Self {
            conversation: ConversationBuffer::new(pool.clone()),
            rhythm: RhythmGuard::new(None, None, None),
            minimax,
            wiki,
            stats,
            patches,
            persona: Persona::new(pool.clone()),
            style: StyleExamples::new(pool.clone()),
            soul: SoulStore::new(pool.clone()),
            channel_bg: ChannelBackground::new(pool.clone()),
            match_ctx: MatchContext::new(pool.clone()),
            transcripts: StreamTranscripts::new(pool.clone()),
            sentiment: GlobalSentiment::new(pool.clone()),
            lurker: LurkerSignal::new(pool.clone()),
            threads: Threads::new(pool.clone()),
            stream_state: StreamState::new(pool.clone()),
            pool,
        }
    }

    /// Wie [`Self::new`], aber mit den produktiven HTTP-Endpunkten.
    pub fn with_defaults(pool: PgPool, minimax: EngagementMinimaxClient) -> Self {
        Self::new(
            pool,
            minimax,
            DeadlockWiki::new(),
            DeadlockStats::new(),
            DeadlockPatches::new(),
        )
    }

    /// Verarbeitet eine Nachricht und loggt die Entscheidung (außer DISABLED).
    pub async fn handle(&self, msg: &IncomingMessage) -> HandleResult {
        let result = self.handle_inner(msg).await;
        if result.decision != Decision::Disabled {
            let cost = calc_cost_usd(result.prompt_tokens, result.completion_tokens);
            gate::log_decision(&self.pool, &msg.channel_login, msg.message_id.as_deref(), &result, cost)
                .await;
        }
        result
    }

    async fn handle_inner(&self, msg: &IncomingMessage) -> HandleResult {
        // --- Gate-Kaskade ---
        let settings = match gate::load_settings(&self.pool, &msg.channel_login).await {
            Some(s) if s.enabled => s,
            _ => return HandleResult::new(Decision::Disabled),
        };
        if !gate::is_operational_partner(&self.pool, &msg.channel_login).await {
            return HandleResult::new(Decision::Disabled);
        }
        if !self.stream_state.is_streaming_deadlock(&msg.channel_login).await {
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

        let history_turns = match self.conversation.load_recent_buffer(&msg.channel_login, 100).await
        {
            Ok(h) => h,
            Err(_) => return HandleResult::new(Decision::ProviderError),
        };
        let history: Vec<ChatMessage> = history_turns
            .iter()
            .map(|t| ChatMessage {
                role: t.role.clone(),
                content: t.content.clone(),
                name: if t.role == "user" { t.twitch_login.clone() } else { None },
            })
            .collect();

        // --- System-Prompt aus Baseline + ~12 optionalen Fragmenten ---
        let mut prompt = build_baseline_system_prompt(&msg.channel_login);
        append_fragment(&mut prompt, &self.soul.get_soul_extension_fragment().await);
        append_fragment(
            &mut prompt,
            &self.channel_bg.get_channel_profile_fragment(&msg.channel_login).await,
        );
        // Persona wird IMMER angehängt (Fragment ist nie leer).
        let persona = self.persona.sample_tone(&msg.channel_login, 50).await;
        append_fragment(&mut prompt, &persona.to_prompt_fragment());
        append_fragment(&mut prompt, &self.style.build_style_fragment(&msg.channel_login).await);

        let threads = self
            .threads
            .load_open_threads_for_user(&msg.twitch_user_id, &msg.channel_login, 5)
            .await;
        if !threads.is_empty() {
            append_fragment(&mut prompt, &threads_to_prompt_fragment(&msg.twitch_login, &threads));
        }

        let lurkers = self
            .lurker
            .known_regulars_currently_lurking(&msg.channel_login, 10, 30, 10, 5)
            .await;
        if !lurkers.is_empty() {
            append_fragment(&mut prompt, &lurker_hint_to_prompt_fragment(&lurkers));
        }

        if let Some(ms) = self.match_ctx.get_match_state(&msg.channel_login).await {
            if ms.is_live {
                append_fragment(&mut prompt, &ms.to_prompt_fragment());
            }
        }

        append_fragment(&mut prompt, &self.wiki.build_grounding_fragment(&msg.content).await);

        let segments = self.transcripts.load_recent_segments(&msg.channel_login, None, None).await;
        append_fragment(&mut prompt, &segments_to_prompt_fragment(&segments, None));

        append_fragment(
            &mut prompt,
            &self.sentiment.get_sentiment_fragment(global_sentiment::FRESH_MAX_AGE_HOURS).await,
        );

        // Patch: entity-getriggert, sonst ambient-Digest.
        let mut patch_fragment = self.patches.build_patch_fragment(&self.wiki, &msg.content).await;
        if patch_fragment.is_empty() {
            patch_fragment = self.patches.get_patch_digest_fragment(&msg.content).await;
        }
        append_fragment(&mut prompt, &patch_fragment);

        append_fragment(
            &mut prompt,
            &self.stats.build_stats_fragment(&self.wiki, &msg.content).await,
        );

        if let Some(po) = settings.persona_override.as_deref().filter(|p| !p.is_empty()) {
            prompt = format!("{prompt}\n\nZusätzliche Persona-Hinweise: {po}");
        }
        if !settings.tabu_topics.is_empty() {
            let joined = settings
                .tabu_topics
                .iter()
                .filter(|t| !t.is_empty())
                .cloned()
                .collect::<Vec<_>>()
                .join(", ");
            if !joined.is_empty() {
                prompt = format!("{prompt}\n\nTabu-Themen (niemals ansprechen): {joined}");
            }
        }

        // --- Modell-Call ---
        let response = match self.minimax.generate(&prompt, &history, 500, 480).await {
            Ok(r) => r,
            Err(GenerateError::Unavailable(_)) => {
                tracing::warn!("Engagement: MiniMax-Provider nicht verfügbar");
                return HandleResult::new(Decision::ProviderError);
            }
            Err(e) => {
                tracing::error!(error = %e, "Engagement: MiniMax-Call fehlgeschlagen");
                return HandleResult::new(Decision::ProviderError);
            }
        };

        let Some(text) = response.text.clone() else {
            return HandleResult {
                decision: Decision::Silent,
                model: Some(response.model),
                prompt_tokens: response.prompt_tokens,
                completion_tokens: response.completion_tokens,
                latency_ms: Some(response.latency_ms),
                ..HandleResult::new(Decision::Silent)
            };
        };

        // Starter-Repeat-Guard: gleicher erster Begriff wie letzte Bot-Antwort → still.
        if let Some(last_bot) = history_turns.iter().rev().find(|t| t.role == "assistant") {
            let prev = first_token_norm(&last_bot.content);
            let this = first_token_norm(&text);
            if !prev.is_empty() && prev == this {
                return HandleResult {
                    decision: Decision::Silent,
                    model: Some(response.model),
                    prompt_tokens: response.prompt_tokens,
                    completion_tokens: response.completion_tokens,
                    latency_ms: Some(response.latency_ms),
                    ..HandleResult::new(Decision::Silent)
                };
            }
        }

        self.rhythm.note_bot_post(&msg.channel_login, Utc::now());
        let _ = self.conversation.append_assistant_turn(&msg.channel_login, &text).await;

        let referenced_thread_ids: Option<Vec<i64>> = if threads.is_empty() {
            None
        } else {
            Some(threads.iter().map(|t| t.id).collect())
        };
        if let Some(ids) = &referenced_thread_ids {
            let _ = self.threads.mark_referenced(ids).await;
        }

        HandleResult {
            decision: Decision::Spoke,
            response_text: Some(text),
            model: Some(response.model),
            prompt_tokens: response.prompt_tokens,
            completion_tokens: response.completion_tokens,
            latency_ms: Some(response.latency_ms),
            referenced_thread_ids,
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
        // Ohne gesetzte Env: 1000 Input + 1000 Output → 0.0008 + 0.0024 = 0.0032.
        // (Env-frei im Testprozess vorausgesetzt; Defaults greifen.)
        if std::env::var("MINIMAX_PRICE_INPUT_PER_1K").is_err()
            && std::env::var("MINIMAX_PRICE_OUTPUT_PER_1K").is_err()
        {
            let cost = calc_cost_usd(Some(1000), Some(1000)).unwrap();
            assert!((cost - 0.0032).abs() < 1e-9);
        }
    }

    use crate::deadlock_patches::DeadlockPatches;
    use crate::deadlock_stats::DeadlockStats;
    use crate::deadlock_wiki::DeadlockWiki;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new().max_connections(1).connect(&dsn).await.unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).execute(&admin).await.unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&admin).await.unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn).unwrap().options([("search_path", schema)]);
        let pool = PgPoolOptions::new().max_connections(3).connect_with(opts).await.unwrap();
        for ddl in [
            "CREATE TABLE twitch_engagement_settings (channel_login TEXT PRIMARY KEY, enabled BOOLEAN NOT NULL DEFAULT FALSE, steam_id TEXT, persona_override TEXT, tabu_topics TEXT[])",
            "CREATE TABLE twitch_streamers_partner_state (twitch_login TEXT, is_partner_active INTEGER)",
            "CREATE TABLE twitch_live_state (twitch_user_id TEXT PRIMARY KEY, streamer_login TEXT NOT NULL, is_live INTEGER DEFAULT 0, last_game TEXT)",
            "CREATE TABLE twitch_user_engagement_optout (twitch_user_id TEXT PRIMARY KEY, opted_out_at TIMESTAMPTZ DEFAULT NOW())",
            "CREATE TABLE twitch_engagement_conversation (id BIGSERIAL PRIMARY KEY, channel_login TEXT, role TEXT, twitch_user_id TEXT, twitch_login TEXT, content TEXT, message_id TEXT, ts TIMESTAMPTZ NOT NULL DEFAULT NOW())",
            "CREATE TABLE twitch_engagement_log (id BIGSERIAL PRIMARY KEY, channel_login TEXT NOT NULL, triggered_by_msg_id TEXT, decision TEXT NOT NULL, response_text TEXT, referenced_thread_ids BIGINT[], model TEXT NOT NULL, prompt_tokens INT, completion_tokens INT, cost_usd_estimate DOUBLE PRECISION, latency_ms INT, created_at TIMESTAMPTZ NOT NULL DEFAULT NOW())",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        Some(pool)
    }

    /// Pipeline mit bogus-HTTP-Providern (fail fast → leere Fragmente).
    fn pipeline_with(pool: PgPool, minimax_uri: &str) -> EngagementPipeline {
        let minimax = EngagementMinimaxClient::new(
            Some("k".to_string()),
            Some(minimax_uri.to_string()),
            Some("MiniMax-M3".to_string()),
            None,
        );
        EngagementPipeline::new(
            pool,
            minimax,
            DeadlockWiki::with_bases("http://127.0.0.1:1", "http://127.0.0.1:1/api"),
            DeadlockStats::with_base("http://127.0.0.1:1"),
            DeadlockPatches::with_url("http://127.0.0.1:1/news"),
        )
    }

    fn msg() -> IncomingMessage {
        IncomingMessage {
            channel_login: "nani".to_string(),
            twitch_user_id: "u1".to_string(),
            twitch_login: "chatter".to_string(),
            content: "lohnt sich trophy collector auf haze".to_string(),
            message_id: Some("m1".to_string()),
        }
    }

    #[tokio::test]
    async fn disabled_wenn_settings_aus() {
        let Some(pool) = make_pool("t_eng_pipe_disabled").await else { return };
        // Kein Settings-Eintrag → DISABLED (keine weiteren Gates/MiniMax nötig).
        let pipe = pipeline_with(pool, "http://127.0.0.1:1");
        let r = pipe.handle(&msg()).await;
        assert_eq!(r.decision, Decision::Disabled);
    }

    #[tokio::test]
    async fn spoke_voller_pfad() {
        let Some(pool) = make_pool("t_eng_pipe_spoke").await else { return };
        sqlx::query("INSERT INTO twitch_engagement_settings (channel_login, enabled) VALUES ('nani', TRUE)").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_streamers_partner_state (twitch_login, is_partner_active) VALUES ('nani', 1)").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_live_state (twitch_user_id, streamer_login, is_live, last_game) VALUES ('1','nani',1,'Deadlock')").execute(&pool).await.unwrap();

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{"message": {"content": "klar haze ist stark grad"}}],
                "usage": {"prompt_tokens": 100, "completion_tokens": 8}
            })))
            .mount(&server)
            .await;

        let pipe = pipeline_with(pool.clone(), &server.uri());
        let r = pipe.handle(&msg()).await;
        assert_eq!(r.decision, Decision::Spoke);
        assert_eq!(r.response_text.as_deref(), Some("klar haze ist stark grad"));
        // User- + Assistant-Turn persistiert.
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_engagement_conversation")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(n, 2);
        // Decision geloggt.
        let dec: String = sqlx::query_scalar("SELECT decision FROM twitch_engagement_log LIMIT 1")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(dec, "spoke");
    }

    #[tokio::test]
    async fn silent_bei_skip_trigger() {
        let Some(pool) = make_pool("t_eng_pipe_silent").await else { return };
        sqlx::query("INSERT INTO twitch_engagement_settings (channel_login, enabled) VALUES ('nani', TRUE)").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_streamers_partner_state (twitch_login, is_partner_active) VALUES ('nani', 1)").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_live_state (twitch_user_id, streamer_login, is_live, last_game) VALUES ('1','nani',1,'Deadlock')").execute(&pool).await.unwrap();

        let pipe = pipeline_with(pool.clone(), "http://127.0.0.1:1");
        let mut m = msg();
        m.content = "@someone hi".to_string(); // führendes @ → skip → SILENT (kein MiniMax)
        let r = pipe.handle(&m).await;
        assert_eq!(r.decision, Decision::Silent);
        // User-Turn trotzdem im Buffer (für Kontext), aber keine Bot-Antwort.
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_engagement_conversation")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(n, 1);
    }
}
