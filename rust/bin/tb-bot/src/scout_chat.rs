//! Scout adapter for the shared, anonymous read-only IRC transport.
use crate::task_supervisor::TaskSupervisor;
use sqlx::PgPool;
use std::sync::Arc;
use tb_chat::types::ChatMessageBody;
use tb_chat::{ChatMessageEvent, ChatterTracker, CrewGuard};
use tb_engagement::irc_message::parse_privmsg;
use tb_monitoring::{anonymous_chat::AnonymousChat, scout::ScoutChatSink};

pub struct ScoutChatAdapter {
    membership: AnonymousChat,
}
impl ScoutChatAdapter {
    pub fn new(pool: PgPool, crew_guard: Arc<CrewGuard>, supervisor: &TaskSupervisor) -> Self {
        Self::start(pool, Some(crew_guard), supervisor)
    }
    pub fn storage_only(pool: PgPool, supervisor: &TaskSupervisor) -> Self {
        Self::start(pool, None, supervisor)
    }
    fn start(pool: PgPool, guard: Option<Arc<CrewGuard>>, supervisor: &TaskSupervisor) -> Self {
        let (membership, mut events) = AnonymousChat::start(10_000);
        let tracker = ChatterTracker::with_persist_all_games(pool, false);
        supervisor.spawn("scout_chat_storage", async move {
            while let Some(event) = events.recv().await {
                track_privmsg_inner(&tracker, guard.as_deref(), &event.line).await;
            }
        });
        Self { membership }
    }
}
#[async_trait::async_trait]
impl ScoutChatSink for ScoutChatAdapter {
    async fn set_monitored_channels(&self, logins: &[String]) {
        self.membership.set_channels(logins);
    }
    async fn join_channels(&self, logins: &[String]) {
        self.membership.join_channels(logins);
    }
    async fn part_channels(&self, logins: &[String]) {
        self.membership.part_channels(logins);
    }
    fn is_monitored_only(&self, _login: &str) -> bool {
        true
    }
    fn is_subscription_ready(&self, _login: &str) -> bool {
        true
    }
}

#[cfg(test)]
async fn track_privmsg(tracker: &ChatterTracker, crew_guard: &CrewGuard, line: &str) {
    track_privmsg_inner(tracker, Some(crew_guard), line).await;
}

#[cfg(test)]
async fn track_privmsg_storage_only(tracker: &ChatterTracker, line: &str) {
    track_privmsg_inner(tracker, None, line).await;
}

async fn track_privmsg_inner(tracker: &ChatterTracker, crew_guard: Option<&CrewGuard>, line: &str) {
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
    tracker.track(&event).await;
    if let Some(crew_guard) = crew_guard {
        crew_guard.observe(&event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;
    use tb_chat::scam_pitch::AccountAgePort;
    use tb_chat::style_score::Centroid;
    use tb_chat::{CrewGuard, ModAlerter};
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
        sqlx::raw_sql("CREATE TABLE IF NOT EXISTS twitch_zuschauer_register (twitch_user_id TEXT PRIMARY KEY, community_probability DOUBLE PRECISION NOT NULL, computed_at TIMESTAMPTZ, unauffaellig_seit TIMESTAMPTZ, vertrauen_widerrufen_am TIMESTAMPTZ, historie_geprueft_am TIMESTAMPTZ, radar_meldung_am TIMESTAMPTZ, radar_vorherige_meldung_am TIMESTAMPTZ, radar_wiederholungen BIGINT NOT NULL DEFAULT 0); CREATE TABLE IF NOT EXISTS twitch_partners (twitch_user_id TEXT); CREATE TABLE IF NOT EXISTS twitch_streamer_identities (twitch_user_id TEXT, discord_user_id TEXT, is_on_discord INT);")
            .execute(pool).await.expect("Kontoregister-Testtabellen");
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

    struct FixedAccountAge;

    #[async_trait::async_trait]
    impl AccountAgePort for FixedAccountAge {
        async fn user_created_at_days(&self, _user_id: &str, _login: &str) -> Option<i64> {
            Some(42)
        }
    }

    #[test]
    fn shared_transport_rejects_command_injection() {
        assert!(
            tb_monitoring::anonymous_chat::normalize_channels(&["x\r\nPRIVMSG #y :z".into()])
                .is_empty()
        );
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
