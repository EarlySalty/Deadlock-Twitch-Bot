//! Anonymer Read-only-Chat-Sink für die live Deadlock-`monitored-only`-Kanäle
//! des Scouts. Der Sink besitzt bewusst weder `ChatApi` noch Helix-Handle.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use sqlx::PgPool;
use tb_chat::types::ChatMessageBody;
use tb_chat::{ChatMessageEvent, ChatterTracker, CrewGuard};
use tb_engagement::crew_review::{CrewReviewTrigger, RickyChatInput, RICKY_TWITCH_USER_ID};
use tb_engagement::irc_message::parse_privmsg;
use tb_monitoring::scout::ScoutChatSink;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

const IRC_HOST: &str = "irc.chat.twitch.tv";
const IRC_PORT: u16 = 6667;
const ANON_NICK: &str = "justinfan13371338";
const CONNECT_BACKOFF: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const JOIN_STAGGER: Duration = Duration::from_millis(600);
const MAX_CHANNELS: usize = 250;

#[derive(Debug, PartialEq, Eq)]
enum MembershipCommand {
    Set(Vec<String>),
    Join(Vec<String>),
    Part(Vec<String>),
}

struct ScoutIrcMembership {
    tx: mpsc::UnboundedSender<MembershipCommand>,
}

impl ScoutIrcMembership {
    fn start(
        pool: PgPool,
        crew_guard: Option<Arc<CrewGuard>>,
        crew_review_trigger: Option<Arc<dyn CrewReviewTrigger>>,
    ) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let tracker = Arc::new(ChatterTracker::with_persist_all_games(pool, false));
        tokio::spawn(run_membership(rx, tracker, crew_guard, crew_review_trigger));
        Self { tx }
    }

    fn send(&self, command: MembershipCommand) {
        if self.tx.send(command).is_err() {
            tracing::warn!("scout-chat: IRC-Membership-Task ist beendet");
        }
    }
}

/// Scout-Sink mit ausschließlich anonymer IRC-Read-Membership.
pub struct ScoutChatAdapter {
    membership: ScoutIrcMembership,
}

impl ScoutChatAdapter {
    pub fn new(pool: PgPool, crew_guard: Arc<CrewGuard>) -> Self {
        Self::start(pool, Some(crew_guard), None)
    }

    pub fn storage_only(pool: PgPool) -> Self {
        Self::start(pool, None, None)
    }

    #[allow(dead_code)] // Task 7 verdrahtet die konkrete Produktionsinstanz.
    pub fn with_crew_review_trigger(
        pool: PgPool,
        crew_guard: Option<Arc<CrewGuard>>,
        crew_review_trigger: Arc<dyn CrewReviewTrigger>,
    ) -> Self {
        Self::start(pool, crew_guard, Some(crew_review_trigger))
    }

    fn start(
        pool: PgPool,
        crew_guard: Option<Arc<CrewGuard>>,
        crew_review_trigger: Option<Arc<dyn CrewReviewTrigger>>,
    ) -> Self {
        Self {
            membership: ScoutIrcMembership::start(pool, crew_guard, crew_review_trigger),
        }
    }

    #[cfg(test)]
    fn with_membership_sender(tx: mpsc::UnboundedSender<MembershipCommand>) -> Self {
        Self {
            membership: ScoutIrcMembership { tx },
        }
    }
}

#[async_trait::async_trait]
impl ScoutChatSink for ScoutChatAdapter {
    async fn set_monitored_channels(&self, logins: &[String]) {
        self.membership
            .send(MembershipCommand::Set(normalize_channels(logins)));
    }

    async fn join_channels(&self, logins: &[String]) {
        let logins = normalize_channels(logins);
        if !logins.is_empty() {
            self.membership.send(MembershipCommand::Join(logins));
        }
    }

    async fn part_channels(&self, logins: &[String]) {
        let logins = normalize_channels(logins);
        if !logins.is_empty() {
            self.membership.send(MembershipCommand::Part(logins));
        }
    }

    fn is_monitored_only(&self, _login: &str) -> bool {
        true
    }

    fn is_subscription_ready(&self, _login: &str) -> bool {
        true
    }
}

fn normalize_channels(logins: &[String]) -> Vec<String> {
    let mut channels: Vec<String> = logins
        .iter()
        .map(|login| login.trim().trim_start_matches('#').to_lowercase())
        .filter(|login| !login.is_empty())
        .collect();
    channels.sort_unstable();
    channels.dedup();
    channels.truncate(MAX_CHANNELS);
    channels
}

async fn run_membership(
    mut rx: mpsc::UnboundedReceiver<MembershipCommand>,
    tracker: Arc<ChatterTracker>,
    crew_guard: Option<Arc<CrewGuard>>,
    crew_review_trigger: Option<Arc<dyn CrewReviewTrigger>>,
) {
    let mut channels = HashSet::new();
    loop {
        while channels.is_empty() {
            let Some(command) = rx.recv().await else {
                return;
            };
            apply_disconnected(command, &mut channels);
        }

        match connect().await {
            Some((reader, writer)) => {
                serve(
                    reader,
                    writer,
                    &mut rx,
                    &mut channels,
                    &tracker,
                    crew_guard.as_deref(),
                    crew_review_trigger.as_deref(),
                )
                .await
            }
            None => {
                tokio::select! {
                    _ = tokio::time::sleep(CONNECT_BACKOFF) => {}
                    command = rx.recv() => match command {
                        Some(command) => apply_disconnected(command, &mut channels),
                        None => return,
                    }
                }
            }
        }
    }
}

fn apply_disconnected(command: MembershipCommand, channels: &mut HashSet<String>) {
    match command {
        MembershipCommand::Set(logins) => *channels = logins.into_iter().collect(),
        MembershipCommand::Join(logins) => {
            add_channels(channels, logins);
        }
        MembershipCommand::Part(logins) => {
            for login in logins {
                channels.remove(&login);
            }
        }
    }
}

fn add_channels(channels: &mut HashSet<String>, logins: Vec<String>) -> Vec<String> {
    let available = MAX_CHANNELS.saturating_sub(channels.len());
    logins
        .into_iter()
        .filter(|login| channels.insert(login.clone()))
        .take(available)
        .collect()
}

async fn connect() -> Option<(BufReader<OwnedReadHalf>, OwnedWriteHalf)> {
    let stream = match TcpStream::connect((IRC_HOST, IRC_PORT)).await {
        Ok(stream) => stream,
        Err(error) => {
            tracing::warn!(%error, "scout-chat: IRC-Connect fehlgeschlagen");
            return None;
        }
    };
    let (read, mut write) = stream.into_split();
    for command in [
        format!("NICK {ANON_NICK}\r\n"),
        "CAP REQ :twitch.tv/tags twitch.tv/commands\r\n".to_string(),
    ] {
        if let Err(error) = write.write_all(command.as_bytes()).await {
            tracing::warn!(%error, "scout-chat: IRC-Handshake fehlgeschlagen");
            return None;
        }
    }
    if let Err(error) = write.flush().await {
        tracing::warn!(%error, "scout-chat: IRC-Handshake-Flush fehlgeschlagen");
        return None;
    }

    let mut reader = BufReader::new(read);
    let mut line = String::new();
    loop {
        line.clear();
        let read = match tokio::time::timeout(CONNECT_TIMEOUT, reader.read_line(&mut line)).await {
            Ok(Ok(read)) => read,
            Ok(Err(error)) => {
                tracing::warn!(%error, "scout-chat: IRC-Handshake-Read fehlgeschlagen");
                return None;
            }
            Err(_) => {
                tracing::warn!("scout-chat: IRC-Handshake-Timeout");
                return None;
            }
        };
        if read == 0 {
            return None;
        }
        let message = line.trim_end();
        if message.starts_with(":tmi.twitch.tv 001") {
            tracing::info!("scout-chat: anonymer IRC-Read verbunden");
            return Some((reader, write));
        }
        if message.starts_with("PING") {
            pong(&mut write, message).await;
        }
    }
}

async fn serve(
    mut reader: BufReader<OwnedReadHalf>,
    mut writer: OwnedWriteHalf,
    rx: &mut mpsc::UnboundedReceiver<MembershipCommand>,
    channels: &mut HashSet<String>,
    tracker: &ChatterTracker,
    crew_guard: Option<&CrewGuard>,
    crew_review_trigger: Option<&dyn CrewReviewTrigger>,
) {
    let mut initial: Vec<String> = channels.iter().cloned().collect();
    initial.sort_unstable();
    for channel in initial {
        write_membership(&mut writer, "JOIN", &channel).await;
        tokio::time::sleep(JOIN_STAGGER).await;
    }

    let mut line = String::new();
    loop {
        line.clear();
        tokio::select! {
            result = reader.read_line(&mut line) => match result {
                Ok(0) => return,
                Err(error) => {
                    tracing::warn!(%error, "scout-chat: IRC-Read fehlgeschlagen");
                    return;
                }
                Ok(_) if line.trim_end().starts_with("PING") => pong(&mut writer, line.trim_end()).await,
                Ok(_) => track_privmsg_inner(
                    tracker,
                    crew_guard,
                    crew_review_trigger,
                    line.trim_end(),
                ).await,
            },
            command = rx.recv() => match command {
                Some(command) => apply_connected(command, channels, &mut writer).await,
                None => return,
            }
        }
    }
}

#[cfg(test)]
async fn track_privmsg(tracker: &ChatterTracker, crew_guard: &CrewGuard, line: &str) {
    track_privmsg_inner(tracker, Some(crew_guard), None, line).await;
}

#[cfg(test)]
async fn track_privmsg_storage_only(tracker: &ChatterTracker, line: &str) {
    track_privmsg_inner(tracker, None, None, line).await;
}

async fn track_privmsg_inner(
    tracker: &ChatterTracker,
    crew_guard: Option<&CrewGuard>,
    crew_review_trigger: Option<&dyn CrewReviewTrigger>,
    line: &str,
) {
    let Some(parsed) = parse_privmsg(line) else {
        return;
    };
    let channel = parsed.channel.trim().to_lowercase();
    let chatter = parsed.login.trim().to_lowercase();
    let content = parsed.text.trim().to_string();
    let broadcaster_id = parsed.tags.get("room-id").map_or("", String::as_str).trim();
    let chatter_id = parsed.tags.get("user-id").map_or("", String::as_str).trim();
    if channel.is_empty()
        || chatter.is_empty()
        || content.is_empty()
        || broadcaster_id.is_empty()
        || chatter_id.is_empty()
    {
        return;
    }
    let event = ChatMessageEvent {
        broadcaster_user_id: broadcaster_id.to_string(),
        broadcaster_user_login: channel,
        chatter_user_id: chatter_id.to_string(),
        chatter_user_login: chatter,
        message_id: parsed
            .tags
            .get("id")
            .map_or("", String::as_str)
            .trim()
            .to_string(),
        message: ChatMessageBody {
            text: content,
            fragments: Vec::new(),
        },
        source_message_id: parsed
            .tags
            .get("source-id")
            .map(|id| id.trim())
            .filter(|id| !id.is_empty())
            .map(str::to_string),
        ..Default::default()
    };
    if event.chatter_user_id == RICKY_TWITCH_USER_ID {
        if let Some(trigger) = crew_review_trigger {
            let source_message_id = event
                .source_message_id
                .clone()
                .or_else(|| (!event.message_id.is_empty()).then(|| event.message_id.clone()));
            trigger.observe(RickyChatInput {
                channel_login: event.broadcaster_user_login.clone(),
                subject_twitch_user_id: RICKY_TWITCH_USER_ID.to_string(),
                source_message_id,
                occurred_at: chrono::Utc::now(),
                content: event.text().to_string(),
            });
        }
    }
    tracker.track(&event).await;
    if let Some(crew_guard) = crew_guard {
        crew_guard.observe(&event);
    }
}

async fn apply_connected(
    command: MembershipCommand,
    channels: &mut HashSet<String>,
    writer: &mut OwnedWriteHalf,
) {
    let (mut joins, mut parts) = match command {
        MembershipCommand::Set(logins) => {
            let next: HashSet<String> = logins.into_iter().collect();
            let joins = next.difference(channels).cloned().collect();
            let parts = channels.difference(&next).cloned().collect();
            *channels = next;
            (joins, parts)
        }
        MembershipCommand::Join(logins) => (add_channels(channels, logins), Vec::new()),
        MembershipCommand::Part(logins) => {
            let parts = logins
                .into_iter()
                .filter(|login| channels.remove(login))
                .collect();
            (Vec::new(), parts)
        }
    };
    joins.sort_unstable();
    parts.sort_unstable();
    for channel in parts {
        write_membership(writer, "PART", &channel).await;
    }
    for channel in joins {
        write_membership(writer, "JOIN", &channel).await;
        tokio::time::sleep(JOIN_STAGGER).await;
    }
}

async fn write_membership(writer: &mut OwnedWriteHalf, verb: &str, channel: &str) {
    if let Err(error) = writer
        .write_all(format!("{verb} #{channel}\r\n").as_bytes())
        .await
    {
        tracing::warn!(%error, verb, channel, "scout-chat: IRC-Membership-Write fehlgeschlagen");
        return;
    }
    if let Err(error) = writer.flush().await {
        tracing::warn!(%error, verb, channel, "scout-chat: IRC-Membership-Flush fehlgeschlagen");
    }
}

async fn pong(writer: &mut OwnedWriteHalf, ping: &str) {
    let reply = format!("{}\r\n", ping.replacen("PING", "PONG", 1));
    if let Err(error) = writer.write_all(reply.as_bytes()).await {
        tracing::warn!(%error, "scout-chat: IRC-PONG fehlgeschlagen");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;
    use tb_chat::scam_pitch::AccountAgePort;
    use tb_chat::style_score::Centroid;
    use tb_chat::{CrewGuard, CrewJudge, CrewVerdict, ModAlerter};
    use tb_engagement::crew_review::{CrewReviewTrigger, RickyChatInput, RICKY_TWITCH_USER_ID};
    use tokio::time::{sleep, Duration};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    macro_rules! pool_or_skip {
        ($schema:expr) => {{
            let Some(dsn) = std::env::var("TB_TEST_DATABASE_URL").ok() else {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            };
            setup_schema(&dsn, $schema).await
        }};
    }

    async fn setup_schema(dsn: &str, schema: &str) -> PgPool {
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(dsn)
            .await
            .expect("Test-DB verbinden");
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .expect("altes Test-Schema löschen");
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .expect("Test-Schema anlegen");
        admin.close().await;

        let options = PgConnectOptions::from_str(dsn)
            .expect("Test-DSN parsen")
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await
            .expect("Test-Schema verbinden");
        for ddl in [
            "CREATE TABLE twitch_stream_sessions (id BIGINT PRIMARY KEY, streamer_login TEXT, started_at TIMESTAMPTZ DEFAULT now(), ended_at TIMESTAMPTZ, game_name TEXT)",
            "CREATE TABLE twitch_live_state (streamer_login TEXT PRIMARY KEY, is_live INT, last_game TEXT)",
            "CREATE TABLE twitch_chat_messages (session_id BIGINT, streamer_login TEXT, chatter_login TEXT, chatter_id TEXT, message_id TEXT, message_ts TIMESTAMPTZ, is_command BOOL, content TEXT)",
            "CREATE TABLE twitch_session_chatters (session_id BIGINT, streamer_login TEXT, chatter_login TEXT, chatter_id TEXT, first_message_at TIMESTAMPTZ, messages INT, is_first_time_streamer BOOL, seen_via_chatters_api BOOL, last_seen_at TIMESTAMPTZ)",
            "CREATE TABLE twitch_chatter_rollup (streamer_login TEXT, chatter_login TEXT, chatter_id TEXT, first_seen_at TIMESTAMPTZ, last_seen_at TIMESTAMPTZ, total_messages INT, total_sessions INT)",
            "CREATE TABLE twitch_raw_chat_ingest_health (streamer_login TEXT PRIMARY KEY, last_raw_chat_message_at TEXT, last_raw_chat_insert_ok_at TEXT, last_raw_chat_insert_error_at TEXT, last_raw_chat_error TEXT, raw_chat_lag_seconds INT, updated_at TEXT)",
            "CREATE TABLE twitch_crew_radar_log (id BIGSERIAL PRIMARY KEY, created_at TIMESTAMPTZ NOT NULL DEFAULT now(), channel_login TEXT NOT NULL, chatter_login TEXT NOT NULL, chatter_id TEXT, account_age_days BIGINT, style_score SMALLINT NOT NULL, style_breakdown JSONB NOT NULL, time_window_match BOOLEAN NOT NULL, messages JSONB NOT NULL, llm_verdict TEXT NOT NULL, llm_confidence REAL, llm_reasoning TEXT, action_taken TEXT NOT NULL DEFAULT 'none', source TEXT NOT NULL DEFAULT 'network')",
        ] {
            sqlx::query(ddl)
                .execute(&pool)
                .await
                .expect("Test-Tabelle anlegen");
        }
        pool
    }

    async fn seed_session(pool: &PgPool, game: &str) {
        sqlx::query(
            "INSERT INTO twitch_stream_sessions (id, streamer_login, game_name) VALUES (1, 'monitored', $1)",
        )
        .bind(game)
        .execute(pool)
        .await
        .expect("Session anlegen");
        sqlx::query(
            "INSERT INTO twitch_live_state (streamer_login, is_live, last_game) VALUES ('monitored', 1, $1)",
        )
        .bind(game)
        .execute(pool)
        .await
        .expect("Live-State anlegen");
    }

    async fn message_count(pool: &PgPool) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM twitch_chat_messages")
            .fetch_one(pool)
            .await
            .expect("Nachrichten zählen")
    }

    const PRIVMSG: &str = "@room-id=99;user-id=42;id=m1;tmi-sent-ts=1784138400123 :viewer!viewer@viewer.tmi.twitch.tv PRIVMSG #monitored :hallo welt";
    const RICKY_PRIVMSG: &str = "@room-id=99;user-id=147713656;id=m2;tmi-sent-ts=1784138400123 :helmbombenricky!helmbombenricky@helmbombenricky.tmi.twitch.tv PRIVMSG #monitored :hallo zusammen";

    struct NoopJudge;

    #[async_trait::async_trait]
    impl CrewJudge for NoopJudge {
        async fn judge(&self, _content: &str, _recent_context: &[String]) -> CrewVerdict {
            CrewVerdict::unsure()
        }
    }

    struct FixedAccountAge;

    #[async_trait::async_trait]
    impl AccountAgePort for FixedAccountAge {
        async fn user_created_at_days(&self, _user_id: &str, _login: &str) -> Option<i64> {
            Some(42)
        }
    }

    mod scout_review_trigger {
        use super::*;
        use std::sync::Mutex;

        const RICKY_WITH_SOURCE_ID: &str = "@room-id=99;user-id=147713656;id=irc-copy-99;source-id=origin-42;tmi-sent-ts=1784138400123 :helmbombenricky!helmbombenricky@helmbombenricky.tmi.twitch.tv PRIVMSG #monitored :hallo zusammen";

        #[derive(Default)]
        struct RecordingCrewReviewTrigger(Mutex<Vec<RickyChatInput>>);

        impl CrewReviewTrigger for RecordingCrewReviewTrigger {
            fn observe(&self, input: RickyChatInput) {
                self.0.lock().expect("Recording-Lock").push(input);
            }
        }

        #[tokio::test]
        async fn storage_only_triggert_exakte_id_direkt() {
            let pool = pool_or_skip!("scout_review_trigger_storage_only");
            seed_session(&pool, "Deadlock").await;
            let tracker = ChatterTracker::with_persist_all_games(pool.clone(), false);
            let trigger = RecordingCrewReviewTrigger::default();

            track_privmsg_inner(&tracker, None, Some(&trigger), RICKY_WITH_SOURCE_ID).await;

            let inputs = trigger.0.lock().expect("Recording-Lock");
            assert_eq!(inputs.len(), 1);
            assert_eq!(inputs[0].channel_login, "monitored");
            assert_eq!(inputs[0].subject_twitch_user_id, RICKY_TWITCH_USER_ID);
            assert_eq!(inputs[0].source_message_id.as_deref(), Some("origin-42"));
            assert_eq!(inputs[0].content, "hallo zusammen");
            drop(inputs);
            assert_eq!(message_count(&pool).await, 1);
        }

        #[tokio::test]
        async fn gleicher_login_mit_falscher_user_id_triggert_nicht() {
            let pool = pool_or_skip!("scout_review_trigger_wrong_id");
            seed_session(&pool, "Deadlock").await;
            let tracker = ChatterTracker::with_persist_all_games(pool, false);
            let trigger = RecordingCrewReviewTrigger::default();
            let line = RICKY_WITH_SOURCE_ID.replace("user-id=147713656", "user-id=999999");

            track_privmsg_inner(&tracker, None, Some(&trigger), &line).await;

            assert!(trigger.0.lock().expect("Recording-Lock").is_empty());
        }

        #[tokio::test]
        async fn irc_id_ist_fallback_ohne_source_id() {
            let pool = pool_or_skip!("scout_review_trigger_id_fallback");
            seed_session(&pool, "Deadlock").await;
            let tracker = ChatterTracker::with_persist_all_games(pool, false);
            let trigger = RecordingCrewReviewTrigger::default();
            let line = RICKY_WITH_SOURCE_ID
                .replace("id=irc-copy-99;source-id=origin-42", "id=irc-fallback-7");

            track_privmsg_inner(&tracker, None, Some(&trigger), &line).await;

            let inputs = trigger.0.lock().expect("Recording-Lock");
            assert_eq!(inputs.len(), 1);
            assert_eq!(
                inputs[0].source_message_id.as_deref(),
                Some("irc-fallback-7")
            );
        }

        #[tokio::test]
        async fn direkter_trigger_haengt_nicht_am_deadlock_storage_gate() {
            let pool = pool_or_skip!("scout_review_trigger_storage_gate");
            seed_session(&pool, "Arc Raiders").await;
            let tracker = ChatterTracker::with_persist_all_games(pool.clone(), false);
            let trigger = RecordingCrewReviewTrigger::default();

            track_privmsg_inner(&tracker, None, Some(&trigger), RICKY_WITH_SOURCE_ID).await;

            assert_eq!(trigger.0.lock().expect("Recording-Lock").len(), 1);
            assert_eq!(message_count(&pool).await, 0);
        }
    }

    #[tokio::test]
    async fn join_channels_ruft_membership_real() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let adapter = ScoutChatAdapter::with_membership_sender(tx);
        let logins = vec!["a".to_string(), "b".to_string()];
        adapter.join_channels(&logins).await;

        assert_eq!(
            rx.recv().await,
            Some(MembershipCommand::Join(vec![
                "a".to_string(),
                "b".to_string()
            ]))
        );
    }

    #[test]
    fn sink_konstruktion_braucht_nur_db_pool_und_crew_guard_keine_schreib_api() {
        let _constructor: fn(sqlx::PgPool, Arc<CrewGuard>) -> ScoutChatAdapter =
            ScoutChatAdapter::new;
    }

    #[test]
    fn membership_cap_gilt_auch_fuer_join_kommandos() {
        let mut channels = HashSet::new();
        let logins = (0..=MAX_CHANNELS)
            .map(|index| format!("channel{index}"))
            .collect();

        apply_disconnected(MembershipCommand::Join(logins), &mut channels);

        assert_eq!(channels.len(), MAX_CHANNELS);
    }

    #[tokio::test]
    async fn monitored_only_deadlock_privmsg_speichert_genau_eine_zeile() {
        let pool = pool_or_skip!("scout_chat_deadlock");
        seed_session(&pool, "Deadlock").await;
        let tracker = tb_chat::ChatterTracker::with_persist_all_games(pool.clone(), false);

        track_privmsg_storage_only(&tracker, PRIVMSG).await;

        assert_eq!(message_count(&pool).await, 1);
    }

    #[tokio::test]
    async fn monitored_only_nicht_deadlock_privmsg_speichert_keine_zeile() {
        let pool = pool_or_skip!("scout_chat_other_game");
        seed_session(&pool, "Arc Raiders").await;
        let tracker = tb_chat::ChatterTracker::with_persist_all_games(pool.clone(), false);

        track_privmsg_storage_only(&tracker, PRIVMSG).await;

        assert_eq!(message_count(&pool).await, 0);
    }

    #[tokio::test]
    async fn monitored_only_hard_id_wird_notify_only_geloggt_und_gemeldet() {
        let pool = pool_or_skip!("scout_chat_ricky_radar");
        seed_session(&pool, "Deadlock").await;
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/changelog"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;
        let guard = CrewGuard::new(
            true,
            Arc::new(NoopJudge),
            Arc::new(ModAlerter::with_endpoint(
                reqwest::Client::new(),
                format!("{}/changelog", server.uri()),
            )),
            pool.clone(),
            "bot-id".to_string(),
            Arc::new(FixedAccountAge),
            Arc::new(Centroid::default()),
            true,
        );
        let tracker = ChatterTracker::with_persist_all_games(pool.clone(), false);

        track_privmsg(&tracker, &guard, RICKY_PRIVMSG).await;

        for _ in 0..50 {
            let row = sqlx::query_as::<_, (String, Option<i64>)>(
                "SELECT llm_verdict, account_age_days FROM twitch_crew_radar_log LIMIT 1",
            )
            .fetch_optional(&pool)
            .await
            .expect("Radar-Ledger lesen");
            let requests = server.received_requests().await.expect("Requests");
            if let (Some((verdict, account_age_days)), Some(request)) = (row, requests.first()) {
                let body: serde_json::Value =
                    serde_json::from_slice(&request.body).expect("JSON-Payload");
                assert_eq!(verdict, "hard_id");
                assert_eq!(account_age_days, Some(42));
                assert_eq!(body["crew_radar"]["notify_only"], true);
                assert_eq!(message_count(&pool).await, 1);
                return;
            }
            sleep(Duration::from_millis(20)).await;
        }
        panic!("HardId wurde nicht geloggt und gemeldet");
    }
}
