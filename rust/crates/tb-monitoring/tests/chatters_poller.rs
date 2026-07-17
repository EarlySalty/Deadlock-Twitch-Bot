//! Hermetische Tests des Helix-Chatters-Pollers (#11). Kein Netz: der Helix-Call
//! läuft über einen Fake-[`ChattersFetcher`], Token-Ports + Provisioner ebenso.

mod support;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use sqlx::PgPool;
use tb_monitoring::chatters_poller::{
    load_live_roster, poll_streamer_once_for_test, BotChatterAuth, ChattersFetcher, CycleStats,
    KeyedCooldown, LiveStreamer, SelfHealCooldowns, StreamerTokenSource,
};
use tb_monitoring::subscriptions::ModeratorProvisioner;
use tb_monitoring::{record_chatters_for_streamer, ChattersCollector};
use tb_transport_twitch::HelixError;

// ---------------------------------------------------------------------------
// Fakes
// ---------------------------------------------------------------------------

#[derive(Clone)]
enum FetchOutcome {
    Ok(Vec<(String, Option<String>)>),
    NotModerator,
}

/// Fetcher mit pro-Call scriptbarer Antwortliste (FIFO), getrennt nach
/// `moderator_id == broadcaster_id` (Streamer-Pfad) vs. Bot-Pfad.
struct ScriptedFetcher {
    bot_calls: std::sync::Mutex<Vec<FetchOutcome>>,
    streamer_calls: std::sync::Mutex<Vec<FetchOutcome>>,
    bot_seen: AtomicUsize,
    streamer_seen: AtomicUsize,
}

impl ScriptedFetcher {
    fn new(bot: Vec<FetchOutcome>, streamer: Vec<FetchOutcome>) -> Self {
        Self {
            bot_calls: std::sync::Mutex::new(bot),
            streamer_calls: std::sync::Mutex::new(streamer),
            bot_seen: AtomicUsize::new(0),
            streamer_seen: AtomicUsize::new(0),
        }
    }
    fn bot_call_count(&self) -> usize {
        self.bot_seen.load(Ordering::SeqCst)
    }
    fn streamer_call_count(&self) -> usize {
        self.streamer_seen.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl ChattersFetcher for ScriptedFetcher {
    async fn fetch_chatters(
        &self,
        broadcaster_id: &str,
        moderator_id: &str,
        _token: &str,
    ) -> Result<Vec<(String, Option<String>)>, HelixError> {
        let streamer_path = broadcaster_id == moderator_id;
        let (queue, seen) = if streamer_path {
            (&self.streamer_calls, &self.streamer_seen)
        } else {
            (&self.bot_calls, &self.bot_seen)
        };
        seen.fetch_add(1, Ordering::SeqCst);
        let mut q = queue.lock().unwrap();
        let outcome = if q.is_empty() {
            FetchOutcome::Ok(vec![])
        } else {
            q.remove(0)
        };
        match outcome {
            FetchOutcome::Ok(v) => Ok(v),
            FetchOutcome::NotModerator => Err(HelixError::NotModerator),
        }
    }
}

struct FakeAuth {
    token: Option<String>,
    user_id: Option<String>,
    login: Option<String>,
    scope: bool,
}
impl FakeAuth {
    fn bot(login: &str) -> Self {
        Self {
            token: Some("bot-token".into()),
            user_id: Some("bot-uid".into()),
            login: Some(login.into()),
            scope: true,
        }
    }
    fn none() -> Self {
        Self {
            token: None,
            user_id: None,
            login: None,
            scope: false,
        }
    }
}
#[async_trait]
impl BotChatterAuth for FakeAuth {
    async fn bot_token(&self) -> Option<String> {
        self.token.clone()
    }
    async fn bot_user_id(&self) -> Option<String> {
        self.user_id.clone()
    }
    async fn bot_login(&self) -> Option<String> {
        self.login.clone()
    }
    async fn has_chatters_scope(&self) -> bool {
        self.scope
    }
}

struct FakeStreamerTokens {
    enabled: std::collections::HashSet<String>,
}
impl FakeStreamerTokens {
    fn with(ids: &[&str]) -> Self {
        Self {
            enabled: ids.iter().map(|s| s.to_string()).collect(),
        }
    }
    fn none() -> Self {
        Self {
            enabled: Default::default(),
        }
    }
}
#[async_trait]
impl StreamerTokenSource for FakeStreamerTokens {
    async fn streamer_token(&self, twitch_user_id: &str) -> Option<String> {
        self.enabled
            .contains(twitch_user_id)
            .then(|| "streamer-token".to_string())
    }
}

struct FakeProvisioner {
    result: bool,
    calls: AtomicUsize,
}
impl FakeProvisioner {
    fn new(result: bool) -> Self {
        Self {
            result,
            calls: AtomicUsize::new(0),
        }
    }
    fn call_count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}
#[async_trait]
impl ModeratorProvisioner for FakeProvisioner {
    async fn ensure_bot_is_mod(&self, _broadcaster_id: &str, _login: &str) -> bool {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.result
    }
}

// ---------------------------------------------------------------------------
// Helfer
// ---------------------------------------------------------------------------

fn streamer(user_id: &str, login: &str, session: i64, partner: bool) -> LiveStreamer {
    LiveStreamer {
        twitch_user_id: user_id.into(),
        streamer_login: login.into(),
        active_session_id: session,
        is_partner_active: partner,
    }
}

async fn seed_live(pool: &PgPool, user_id: &str, login: &str, session: i64, partner: i32) {
    sqlx::query(
        "INSERT INTO twitch_live_state \
             (twitch_user_id, streamer_login, is_live, last_seen_at, active_session_id) \
         VALUES ($1, $2, 1, $3, $4)",
    )
    .bind(user_id)
    .bind(login)
    .bind(Utc::now().to_rfc3339())
    .bind(session)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO twitch_streamers_partner_state (twitch_login, twitch_user_id, is_partner_active) \
         VALUES ($1, $2, $3)",
    )
    .bind(login)
    .bind(user_id)
    .bind(partner)
    .execute(pool)
    .await
    .unwrap();
}

fn logins(v: &[&str]) -> Vec<(String, Option<String>)> {
    v.iter().map(|s| (s.to_string(), None)).collect()
}

/// Chatter-Liste mit gesetzten Helix-`user_id`s — `(login, user_id)`.
fn chatters_with_ids(v: &[(&str, &str)]) -> Vec<(String, Option<String>)> {
    v.iter()
        .map(|(login, id)| (login.to_string(), Some(id.to_string())))
        .collect()
}

// ---------------------------------------------------------------------------
// Tests: Batch-Write
// ---------------------------------------------------------------------------

#[tokio::test]
async fn lurker_insert_messages_zero_and_seen_via_api() {
    let Some(pool) = support::pool_with_chatters_schema("t_chat_lurker").await else {
        return;
    };
    let s = streamer("1", "nani", 42, true);
    let tick = Utc::now();

    let (written, lurkers) =
        record_chatters_for_streamer(&pool, &s, &logins(&["Alice", "Bob"]), Some("mybot"), tick)
            .await
            .unwrap();
    assert_eq!((written, lurkers), (2, 2));

    let (messages, seen, first_time): (i32, bool, bool) = sqlx::query_as(
        "SELECT messages, seen_via_chatters_api, is_first_time_streamer \
         FROM twitch_session_chatters WHERE chatter_login = 'alice'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(messages, 0);
    assert!(seen);
    assert!(first_time);
}

#[tokio::test]
async fn bot_and_self_logins_filtered() {
    let Some(pool) = support::pool_with_chatters_schema("t_chat_filter").await else {
        return;
    };
    let s = streamer("1", "nani", 42, true);
    let tick = Utc::now();
    // nightbot = known bot; mybot = self; Alice = echter Viewer.
    let (written, _) = record_chatters_for_streamer(
        &pool,
        &s,
        &logins(&["Alice", "Nightbot", "MyBot"]),
        Some("mybot"),
        tick,
    )
    .await
    .unwrap();
    assert_eq!(written, 1);
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_session_chatters")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 1);
}

#[tokio::test]
async fn conflict_updates_only_last_seen_at() {
    let Some(pool) = support::pool_with_chatters_schema("t_chat_conflict").await else {
        return;
    };
    let s = streamer("1", "nani", 42, true);
    // Vorhandene Message-Pfad-Zeile: messages=5, seen_via_chatters_api=FALSE.
    let first_msg = Utc::now() - Duration::minutes(10);
    sqlx::query(
        "INSERT INTO twitch_session_chatters \
         (session_id, streamer_login, chatter_login, first_message_at, messages, \
          is_first_time_streamer, seen_via_chatters_api, last_seen_at) \
         VALUES (42, 'nani', 'alice', $1, 5, FALSE, FALSE, $1)",
    )
    .bind(first_msg)
    .execute(&pool)
    .await
    .unwrap();

    let tick = Utc::now();
    record_chatters_for_streamer(&pool, &s, &logins(&["Alice"]), Some("mybot"), tick)
        .await
        .unwrap();

    let (messages, seen, first_at, last): (i32, bool, DateTime<Utc>, DateTime<Utc>) =
        sqlx::query_as(
            "SELECT messages, seen_via_chatters_api, first_message_at, last_seen_at \
             FROM twitch_session_chatters WHERE chatter_login = 'alice'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(messages, 5, "messages NICHT überschrieben");
    assert!(!seen, "seen_via_chatters_api NICHT überschrieben");
    assert_eq!(
        first_at.timestamp(),
        first_msg.timestamp(),
        "first_message_at fix"
    );
    assert_eq!(
        last.timestamp(),
        tick.timestamp(),
        "nur last_seen_at aktualisiert"
    );
}

#[tokio::test]
async fn is_first_time_streamer_via_preread() {
    let Some(pool) = support::pool_with_chatters_schema("t_chat_firsttime").await else {
        return;
    };
    let s = streamer("1", "nani", 42, true);
    let earlier = Utc::now() - Duration::days(1);
    // Bob ist bereits im Rollup → NICHT first-time. Alice ist neu.
    sqlx::query(
        "INSERT INTO twitch_chatter_rollup \
         (streamer_login, chatter_login, first_seen_at, last_seen_at, total_messages, total_sessions) \
         VALUES ('nani', 'bob', $1, $1, 3, 2)",
    )
    .bind(earlier)
    .execute(&pool)
    .await
    .unwrap();

    record_chatters_for_streamer(
        &pool,
        &s,
        &logins(&["Alice", "Bob"]),
        Some("mybot"),
        Utc::now(),
    )
    .await
    .unwrap();

    let alice_ft: bool = sqlx::query_scalar(
        "SELECT is_first_time_streamer FROM twitch_session_chatters WHERE chatter_login='alice'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let bob_ft: bool = sqlx::query_scalar(
        "SELECT is_first_time_streamer FROM twitch_session_chatters WHERE chatter_login='bob'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(alice_ft, "neuer Chatter = first-time");
    assert!(!bob_ft, "bekannter Chatter = nicht first-time");
}

#[tokio::test]
async fn rollup_no_increment_on_conflict() {
    let Some(pool) = support::pool_with_chatters_schema("t_chat_rollup").await else {
        return;
    };
    let s = streamer("1", "nani", 42, true);
    let earlier = Utc::now() - Duration::days(1);
    sqlx::query(
        "INSERT INTO twitch_chatter_rollup \
         (streamer_login, chatter_login, first_seen_at, last_seen_at, total_messages, total_sessions) \
         VALUES ('nani', 'alice', $1, $1, 7, 3)",
    )
    .bind(earlier)
    .execute(&pool)
    .await
    .unwrap();

    let tick = Utc::now();
    record_chatters_for_streamer(&pool, &s, &logins(&["Alice"]), Some("mybot"), tick)
        .await
        .unwrap();

    let (msgs, sessions, first_at, last): (i32, i32, DateTime<Utc>, DateTime<Utc>) =
        sqlx::query_as(
            "SELECT total_messages, total_sessions, first_seen_at, last_seen_at \
             FROM twitch_chatter_rollup WHERE chatter_login='alice'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(msgs, 7, "total_messages NICHT inkrementiert");
    assert_eq!(sessions, 3, "total_sessions NICHT inkrementiert");
    assert_eq!(
        first_at.timestamp(),
        earlier.timestamp(),
        "first_seen_at fix"
    );
    assert_eq!(
        last.timestamp(),
        tick.timestamp(),
        "last_seen_at aktualisiert"
    );

    // Neuer Chatter → Insert mit total_sessions=1.
    record_chatters_for_streamer(&pool, &s, &logins(&["Bob"]), Some("mybot"), tick)
        .await
        .unwrap();
    let bob_sessions: i32 = sqlx::query_scalar(
        "SELECT total_sessions FROM twitch_chatter_rollup WHERE chatter_login='bob'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(bob_sessions, 1);
}

#[tokio::test]
async fn presence_tick_idempotent_same_tick_at() {
    let Some(pool) = support::pool_with_chatters_schema("t_chat_presence").await else {
        return;
    };
    let s = streamer("1", "nani", 42, true);
    let tick = Utc::now();
    record_chatters_for_streamer(&pool, &s, &logins(&["Alice", "Bob"]), Some("mybot"), tick)
        .await
        .unwrap();
    let n1: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM twitch_viewer_presence_ticks WHERE session_id=42")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(n1, 2);

    // Gleicher tick_at → keine neuen Presence-Ticks.
    record_chatters_for_streamer(&pool, &s, &logins(&["Alice", "Bob"]), Some("mybot"), tick)
        .await
        .unwrap();
    let n2: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM twitch_viewer_presence_ticks WHERE session_id=42")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(n2, 2, "gleicher tick_at = idempotent");
}

#[tokio::test]
async fn chatter_id_persisted_into_session_and_rollup() {
    // Helix-user_id muss in session_chatters UND rollup landen (Python-Parität).
    let Some(pool) = support::pool_with_chatters_schema("t_chat_id_persist").await else {
        return;
    };
    let s = streamer("1", "nani", 42, true);
    let tick = Utc::now();

    record_chatters_for_streamer(
        &pool,
        &s,
        &chatters_with_ids(&[("Alice", "111")]),
        Some("mybot"),
        tick,
    )
    .await
    .unwrap();

    let session_id: Option<String> = sqlx::query_scalar(
        "SELECT chatter_id FROM twitch_session_chatters WHERE chatter_login = 'alice'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        session_id.as_deref(),
        Some("111"),
        "chatter_id in session_chatters"
    );

    let rollup_id: Option<String> = sqlx::query_scalar(
        "SELECT chatter_id FROM twitch_chatter_rollup WHERE chatter_login = 'alice'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(rollup_id.as_deref(), Some("111"), "chatter_id in rollup");
}

#[tokio::test]
async fn rollup_chatter_id_coalesce_on_conflict() {
    // Bestehende rollup-chatter_id gewinnt (COALESCE), NULL→neu wird nachgetragen.
    let Some(pool) = support::pool_with_chatters_schema("t_chat_id_coalesce").await else {
        return;
    };
    let s = streamer("1", "nani", 42, true);
    let earlier = Utc::now() - Duration::days(1);

    // bob hat bereits chatter_id='OLD'; alice existiert mit chatter_id NULL.
    sqlx::query(
        "INSERT INTO twitch_chatter_rollup \
         (streamer_login, chatter_login, chatter_id, first_seen_at, last_seen_at, total_messages, total_sessions) \
         VALUES ('nani', 'bob', 'OLD', $1, $1, 0, 1), \
                ('nani', 'alice', NULL, $1, $1, 0, 1)",
    )
    .bind(earlier)
    .execute(&pool)
    .await
    .unwrap();

    record_chatters_for_streamer(
        &pool,
        &s,
        &chatters_with_ids(&[("Bob", "NEW"), ("Alice", "222")]),
        Some("mybot"),
        Utc::now(),
    )
    .await
    .unwrap();

    let bob_id: Option<String> = sqlx::query_scalar(
        "SELECT chatter_id FROM twitch_chatter_rollup WHERE chatter_login='bob'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        bob_id.as_deref(),
        Some("OLD"),
        "bestehende ID gewinnt (COALESCE)"
    );

    let alice_id: Option<String> = sqlx::query_scalar(
        "SELECT chatter_id FROM twitch_chatter_rollup WHERE chatter_login='alice'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        alice_id.as_deref(),
        Some("222"),
        "NULL→neue ID nachgetragen"
    );
}

// ---------------------------------------------------------------------------
// Tests: Poll-Logik (Token-Reihenfolge, Self-Heal)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn self_heal_403_then_single_retry_success() {
    // Bot-Pfad: 403 → Self-Heal (partner, kein Cooldown) → genau 1 Retry → OK.
    let fetcher = ScriptedFetcher::new(
        vec![
            FetchOutcome::NotModerator,
            FetchOutcome::Ok(logins(&["alice"])),
        ],
        vec![],
    );
    let prov = FakeProvisioner::new(true);
    let cooldowns = SelfHealCooldowns::new();
    let not_mod_backoff = KeyedCooldown::not_mod_default();
    let mut stats = CycleStats::default();
    let s = streamer("1", "nani", 42, true);

    let result = poll_streamer_once_for_test(
        &s,
        Some("bot-token"),
        Some("bot-uid"),
        None,
        &fetcher,
        &prov,
        &cooldowns,
        &not_mod_backoff,
        &mut stats,
    )
    .await;

    assert!(result.0, "succeeded");
    assert_eq!(result.1, logins(&["alice"]));
    assert_eq!(fetcher.bot_call_count(), 2, "genau 1 Retry");
    assert_eq!(prov.call_count(), 1);
    assert_eq!(stats.self_heal_success, 1);
    assert_eq!(stats.bot_path_success, 1);
}

#[tokio::test]
async fn self_heal_failure_sets_cooldown_no_retry() {
    let fetcher = ScriptedFetcher::new(vec![FetchOutcome::NotModerator], vec![]);
    let prov = FakeProvisioner::new(false);
    let cooldowns = SelfHealCooldowns::new();
    // Frischer Backoff je Poll, damit dieser Test NUR den Self-Heal-Cooldown
    // prüft (der not-mod-Backoff würde sonst beim zweiten Poll den Bot-Pfad
    // schon vor dem Provisioner überspringen).
    let mut stats = CycleStats::default();
    let s = streamer("1", "nani", 42, true);

    let result = poll_streamer_once_for_test(
        &s,
        Some("bot-token"),
        Some("bot-uid"),
        None,
        &fetcher,
        &prov,
        &cooldowns,
        &KeyedCooldown::not_mod_default(),
        &mut stats,
    )
    .await;

    assert!(!result.0, "nicht succeeded");
    assert_eq!(fetcher.bot_call_count(), 1, "kein Retry nach Heal-Fehler");
    assert_eq!(stats.self_heal_failure, 1);
    assert_eq!(stats.bot_path_failure, 1);

    // Zweiter Poll: Self-Heal-Cooldown aktiv → Provisioner NICHT erneut gerufen.
    // Frischer not-mod-Backoff, damit der Bot-Pfad NICHT vorher übersprungen wird.
    let fetcher2 = ScriptedFetcher::new(vec![FetchOutcome::NotModerator], vec![]);
    let mut stats2 = CycleStats::default();
    let r2 = poll_streamer_once_for_test(
        &s,
        Some("bot-token"),
        Some("bot-uid"),
        None,
        &fetcher2,
        &prov,
        &cooldowns,
        &KeyedCooldown::not_mod_default(),
        &mut stats2,
    )
    .await;
    assert!(!r2.0);
    assert_eq!(prov.call_count(), 1, "Cooldown verhindert zweiten Heal");
}

#[tokio::test]
async fn self_heal_skipped_for_non_partner() {
    let fetcher = ScriptedFetcher::new(vec![FetchOutcome::NotModerator], vec![]);
    let prov = FakeProvisioner::new(true);
    let cooldowns = SelfHealCooldowns::new();
    let not_mod_backoff = KeyedCooldown::not_mod_default();
    let mut stats = CycleStats::default();
    let s = streamer("1", "nani", 42, false); // NICHT partner

    poll_streamer_once_for_test(
        &s,
        Some("bot-token"),
        Some("bot-uid"),
        None,
        &fetcher,
        &prov,
        &cooldowns,
        &not_mod_backoff,
        &mut stats,
    )
    .await;
    assert_eq!(prov.call_count(), 0, "non-partner → kein Self-Heal");
    assert_eq!(stats.self_heal_success, 0);
    assert_eq!(stats.self_heal_failure, 0);
}

#[tokio::test]
async fn streamer_fallback_only_when_token_present() {
    // Bot-Pfad nicht erfolgreich (403, kein Heal weil non-partner) + Streamer-Token vorhanden.
    let fetcher = ScriptedFetcher::new(
        vec![FetchOutcome::NotModerator],
        vec![FetchOutcome::Ok(logins(&["alice", "bob"]))],
    );
    let prov = FakeProvisioner::new(false);
    let cooldowns = SelfHealCooldowns::new();
    let mut stats = CycleStats::default();
    let s = streamer("1", "nani", 42, false);

    let result = poll_streamer_once_for_test(
        &s,
        Some("bot-token"),
        Some("bot-uid"),
        Some("streamer-token"),
        &fetcher,
        &prov,
        &cooldowns,
        &KeyedCooldown::not_mod_default(),
        &mut stats,
    )
    .await;

    assert!(result.0, "Fallback erfolgreich");
    assert_eq!(result.1, logins(&["alice", "bob"]));
    assert_eq!(fetcher.streamer_call_count(), 1);
    assert_eq!(stats.fallback_to_streamer_token, 1);

    // Ohne Streamer-Token: kein Fallback.
    let fetcher2 = ScriptedFetcher::new(vec![FetchOutcome::NotModerator], vec![]);
    let mut stats2 = CycleStats::default();
    let r2 = poll_streamer_once_for_test(
        &s,
        Some("bot-token"),
        Some("bot-uid"),
        None,
        &fetcher2,
        &prov,
        &cooldowns,
        &KeyedCooldown::not_mod_default(),
        &mut stats2,
    )
    .await;
    assert!(!r2.0);
    assert_eq!(
        fetcher2.streamer_call_count(),
        0,
        "kein Token = kein Fallback"
    );
}

// ---------------------------------------------------------------------------
// Tests: Bot-nicht-Mod-Backoff (Effizienz — futile 403-Calls überspringen)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn not_mod_backoff_skips_bot_path_on_next_poll() {
    // 1. Poll: non-partner 403 (Self-Heal nicht anwendbar) → Backoff gesetzt.
    // 2. Poll: Bot-Pfad wird übersprungen (kein get_chatters-Call mehr),
    //    bot_path_skipped_backoff=1, Fetcher NICHT erneut für den Bot-Pfad gerufen.
    let fetcher = ScriptedFetcher::new(vec![FetchOutcome::NotModerator], vec![]);
    let prov = FakeProvisioner::new(false);
    let cooldowns = SelfHealCooldowns::new();
    let not_mod_backoff = KeyedCooldown::not_mod_default();
    let mut stats = CycleStats::default();
    let s = streamer("1", "nani", 42, false); // non-partner → kein Self-Heal

    poll_streamer_once_for_test(
        &s,
        Some("bot-token"),
        Some("bot-uid"),
        None,
        &fetcher,
        &prov,
        &cooldowns,
        &not_mod_backoff,
        &mut stats,
    )
    .await;
    assert_eq!(
        fetcher.bot_call_count(),
        1,
        "erster Poll feuert den Bot-Call"
    );
    assert_eq!(stats.bot_path_failure, 1);
    assert_eq!(stats.bot_path_skipped_backoff, 0);

    // Zweiter Poll mit demselben (jetzt aktiven) Backoff.
    let mut stats2 = CycleStats::default();
    let r2 = poll_streamer_once_for_test(
        &s,
        Some("bot-token"),
        Some("bot-uid"),
        None,
        &fetcher,
        &prov,
        &cooldowns,
        &not_mod_backoff,
        &mut stats2,
    )
    .await;
    assert!(!r2.0, "ohne Streamer-Token kein Erfolg");
    assert_eq!(
        fetcher.bot_call_count(),
        1,
        "Bot-Call NICHT erneut gefeuert (Backoff aktiv)"
    );
    assert_eq!(stats2.bot_path_skipped_backoff, 1);
    assert_eq!(stats2.bot_path_attempt, 0, "kein Bot-Attempt im Backoff");
}

#[tokio::test]
async fn bot_path_success_clears_existing_backoff() {
    // Ein abgelaufenes Backoff-Fenster lässt den Bot-Pfad wieder laufen; bei
    // Erfolg (Bot ist wieder Mod) muss der gespeicherte Eintrag GELÖSCHT werden
    // (nicht nur abgelaufen). Duration::ZERO modelliert das abgelaufene Fenster:
    // der Eintrag wird gesetzt, blockt aber nicht → der nächste Poll läuft durch.
    let cooldowns = SelfHealCooldowns::new();
    let backoff = KeyedCooldown::new(std::time::Duration::ZERO);
    let prov = FakeProvisioner::new(false);
    let s = streamer("1", "nani", 42, false);

    // Schritt 1: 403 setzt einen (sofort abgelaufenen) Backoff-Eintrag für 'nani'.
    let fetcher_fail = ScriptedFetcher::new(vec![FetchOutcome::NotModerator], vec![]);
    let mut stats = CycleStats::default();
    poll_streamer_once_for_test(
        &s,
        Some("bot-token"),
        Some("bot-uid"),
        None,
        &fetcher_fail,
        &prov,
        &cooldowns,
        &backoff,
        &mut stats,
    )
    .await;
    assert!(
        backoff.contains_for_test("nani").await,
        "Backoff-Eintrag gesetzt"
    );

    // Schritt 2: Fenster abgelaufen → Bot-Pfad läuft, Bot ist wieder Mod (200).
    let fetcher_ok = ScriptedFetcher::new(vec![FetchOutcome::Ok(logins(&["alice"]))], vec![]);
    let mut stats2 = CycleStats::default();
    let r = poll_streamer_once_for_test(
        &s,
        Some("bot-token"),
        Some("bot-uid"),
        None,
        &fetcher_ok,
        &prov,
        &cooldowns,
        &backoff,
        &mut stats2,
    )
    .await;
    assert!(r.0, "Bot-Pfad erfolgreich");
    assert_eq!(stats2.bot_path_success, 1);
    assert_eq!(stats2.bot_path_skipped_backoff, 0, "abgelaufen → kein Skip");
    assert!(
        !backoff.contains_for_test("nani").await,
        "Erfolg löscht den Backoff-Eintrag"
    );
}

#[tokio::test]
async fn streamer_token_channel_polled_via_fallback_despite_backoff() {
    // Aktiver Bot-Backoff blockt NUR den Bot-Pfad. Ein raid_enabled-Kanal
    // bzw. jeder Kanal mit geliefertem Streamer-Token wird trotzdem per
    // Streamer-Fallback gepollt.
    let cooldowns = SelfHealCooldowns::new();
    let not_mod_backoff = KeyedCooldown::not_mod_default();
    let s = streamer("1", "nani", 42, false);

    // Schritt 1: 403 setzt den Backoff (ohne Streamer-Token).
    let fetcher_fail = ScriptedFetcher::new(vec![FetchOutcome::NotModerator], vec![]);
    let prov = FakeProvisioner::new(false);
    let mut stats = CycleStats::default();
    poll_streamer_once_for_test(
        &s,
        Some("bot-token"),
        Some("bot-uid"),
        None,
        &fetcher_fail,
        &prov,
        &cooldowns,
        &not_mod_backoff,
        &mut stats,
    )
    .await;

    // Schritt 2: Backoff aktiv → Bot-Pfad übersprungen, ABER Streamer-Token da.
    let fetcher = ScriptedFetcher::new(
        vec![FetchOutcome::NotModerator],
        vec![FetchOutcome::Ok(logins(&["carol"]))],
    );
    let mut stats2 = CycleStats::default();
    let r = poll_streamer_once_for_test(
        &s,
        Some("bot-token"),
        Some("bot-uid"),
        Some("streamer-token"),
        &fetcher,
        &prov,
        &cooldowns,
        &not_mod_backoff,
        &mut stats2,
    )
    .await;
    assert!(r.0, "Streamer-Fallback erfolgreich trotz Bot-Backoff");
    assert_eq!(r.1, logins(&["carol"]));
    assert_eq!(stats2.bot_path_skipped_backoff, 1, "Bot-Pfad übersprungen");
    assert_eq!(fetcher.bot_call_count(), 0, "kein Bot-Call im Backoff");
    assert_eq!(
        fetcher.streamer_call_count(),
        1,
        "Streamer-Fallback gefeuert"
    );
    assert_eq!(stats2.fallback_to_streamer_token, 1);
}

// ---------------------------------------------------------------------------
// Test: voller Collect-Zyklus über ChattersCollector
// ---------------------------------------------------------------------------

#[tokio::test]
async fn roster_ignoriert_stale_live_ghosts() {
    let Some(pool) = support::pool_with_chatters_schema("t_chat_roster_stale").await else {
        return;
    };
    let stale_seen = (Utc::now() - Duration::minutes(20)).to_rfc3339();
    let fresh_seen = (Utc::now() - Duration::minutes(5)).to_rfc3339();

    sqlx::query(
        "INSERT INTO twitch_live_state \
             (twitch_user_id, streamer_login, is_live, last_seen_at, active_session_id) \
         VALUES ('old', 'oldlogin', 1, $1, 11), \
                ('fresh', 'freshlogin', 1, $2, 22)",
    )
    .bind(stale_seen)
    .bind(fresh_seen)
    .execute(&pool)
    .await
    .unwrap();

    let roster = load_live_roster(&pool).await.unwrap();
    let logins: Vec<String> = roster.into_iter().map(|s| s.streamer_login).collect();
    assert_eq!(logins, vec!["freshlogin".to_string()]);
}

#[tokio::test]
async fn full_cycle_inserts_lurkers() {
    let Some(pool) = support::pool_with_chatters_schema("t_chat_cycle").await else {
        return;
    };
    seed_live(&pool, "1", "nani", 42, 1).await;

    let collector = ChattersCollector::new(
        pool.clone(),
        Arc::new(FakeAuth::bot("mybot")),
        Arc::new(FakeStreamerTokens::none()),
        Arc::new(ScriptedFetcher::new(
            vec![FetchOutcome::Ok(logins(&["alice", "bob", "nightbot"]))],
            vec![],
        )),
        Arc::new(FakeProvisioner::new(false)),
    );

    let stats = collector.run_cycle().await;
    assert_eq!(stats.live_streamers, 1);
    assert_eq!(stats.chatters_written, 2, "nightbot gefiltert");
    assert_eq!(stats.lurkers_new, 2);

    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_session_chatters")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 2);
}

#[tokio::test]
async fn cycle_without_bot_token_uses_streamer_fallback() {
    let Some(pool) = support::pool_with_chatters_schema("t_chat_cycle_fb").await else {
        return;
    };
    seed_live(&pool, "99", "drag", 7, 0).await;

    let collector = ChattersCollector::new(
        pool.clone(),
        Arc::new(FakeAuth::none()),
        Arc::new(FakeStreamerTokens::with(&["99"])),
        Arc::new(ScriptedFetcher::new(
            vec![],
            vec![FetchOutcome::Ok(logins(&["carol"]))],
        )),
        Arc::new(FakeProvisioner::new(false)),
    );

    let stats = collector.run_cycle().await;
    assert_eq!(stats.chatters_written, 1);
    let exists: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM twitch_session_chatters WHERE chatter_login='carol'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(exists, 1);
}
