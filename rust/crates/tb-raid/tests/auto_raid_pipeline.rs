//! End-to-End-Tests der Auto-Raid-Pipeline: Auswahl → Readiness → Executor →
//! Pending/Strikes/Blacklist. Echte Stores gegen den Test-Container,
//! Stub-RaidApi mit per-Ziel-Verhalten, Stub-Fallback-Streams.

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use chrono::{Duration, Utc};
use serde_json::json;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;
use tb_crypto::{aad, FieldCipher, KID};
use tb_observability::{
    AnalyticsObservabilityService, EventSink, MillisSource, ObservabilityEvent,
    RaidObservabilityService,
};
use tb_raid::{
    ArrivalReadiness, AutoRaidPipeline, AutoRaidPipelineOutcome, AutoRaidRequest,
    FairnessCandidate, FallbackStreamSource, FollowerEnricher, FollowersEnrichmentObservation,
    OnlineCandidate, PendingRaidStore, RaidApi, RaidAuthStore, RaidBlacklistStore, RaidExecutor,
    RaidHistoryStore, RaidTokenRefresher, RefreshError, ScoreStore, StreamData, StrikesStore,
    TokenBlacklistStore, TokenOwnerInfo, TokenProvider, TokenResponse, TwitchTokenClient,
    FOLLOWERS_UNKNOWN,
};

const TEST_KEY_HEX: &str = "0f0e0d0c0b0a09080706050403020100ffeeddccbbaa99887766554433221100";

macro_rules! pool_or_skip {
    ($schema:expr) => {{
        let Some(dsn) = std::env::var("TB_TEST_DATABASE_URL").ok() else {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        };
        pool_in_schema(&dsn, $schema).await
    }};
}

async fn pool_in_schema(dsn: &str, schema: &str) -> PgPool {
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(dsn)
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
    let opts = PgConnectOptions::from_str(dsn)
        .unwrap()
        .options([("search_path", schema)]);
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(opts)
        .await
        .unwrap();
    for ddl in [
        "CREATE TABLE twitch_raid_auth (
            twitch_user_id TEXT PRIMARY KEY, twitch_login TEXT, access_token TEXT, refresh_token TEXT,
            token_expires_at TIMESTAMPTZ, scopes TEXT, raid_enabled BOOLEAN DEFAULT TRUE,
            needs_reauth BOOLEAN DEFAULT FALSE, access_token_enc BYTEA, refresh_token_enc BYTEA,
            enc_version INTEGER, enc_kid TEXT, last_refreshed_at TIMESTAMPTZ )",
        "CREATE TABLE twitch_raid_history (
            id BIGSERIAL PRIMARY KEY, from_broadcaster_id TEXT, from_broadcaster_login TEXT,
            to_broadcaster_id TEXT, to_broadcaster_login TEXT, viewer_count INTEGER,
            stream_duration_sec INTEGER, reason TEXT, executed_at TIMESTAMPTZ, success BOOLEAN,
            error_message TEXT, target_stream_started_at TIMESTAMPTZ, candidates_count INTEGER )",
        "CREATE TABLE twitch_token_blacklist (
            twitch_user_id TEXT PRIMARY KEY, twitch_login TEXT, error_message TEXT,
            error_count INTEGER DEFAULT 1, first_error_at TEXT, last_error_at TEXT,
            notified INTEGER DEFAULT 0, grace_expires_at TEXT )",
        "CREATE TABLE twitch_raid_blacklist (
            target_login TEXT PRIMARY KEY, target_id TEXT, reason TEXT, added_at TEXT )",
        "CREATE TABLE twitch_chatter_global_ban (
            chatter_login TEXT PRIMARY KEY, chatter_id TEXT, reason TEXT,
            added_by TEXT, added_at TIMESTAMPTZ NOT NULL DEFAULT NOW() )",
        "CREATE TABLE twitch_streamers (
            twitch_login TEXT PRIMARY KEY, twitch_user_id TEXT )",
        "CREATE TABLE twitch_exclusions (
            twitch_user_id TEXT PRIMARY KEY, kind TEXT NOT NULL, reason TEXT,
            excluded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), reactivated_at TIMESTAMPTZ )",
        "CREATE TABLE twitch_partner_outreach (
            streamer_login TEXT, streamer_user_id TEXT, detected_at TEXT,
            contacted_at TEXT, status TEXT, cooldown_until TEXT, notes TEXT,
            raid_used_at TEXT, conversation_status TEXT )",
        "CREATE TABLE twitch_partners (
            twitch_user_id TEXT, twitch_login TEXT, status TEXT )",
        "CREATE TABLE twitch_raid_disabled_strikes (
            target_id TEXT, target_login TEXT NOT NULL, strike_count INTEGER NOT NULL DEFAULT 1,
            last_seen_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), last_reason TEXT,
            CONSTRAINT twitch_raid_disabled_strikes_pkey PRIMARY KEY (target_login) )",
        "CREATE TABLE twitch_partner_raid_scores (
            twitch_user_id TEXT PRIMARY KEY, twitch_login TEXT NOT NULL,
            avg_duration_sec INTEGER NOT NULL DEFAULT 0,
            time_pattern_score_base DOUBLE PRECISION NOT NULL DEFAULT 0.5,
            received_successful_raids_total INTEGER NOT NULL DEFAULT 0,
            is_new_partner_preferred INTEGER NOT NULL DEFAULT 0,
            new_partner_multiplier DOUBLE PRECISION NOT NULL DEFAULT 1.0,
            raid_boost_multiplier DOUBLE PRECISION NOT NULL DEFAULT 1.0,
            is_live INTEGER NOT NULL DEFAULT 0, current_started_at TEXT,
            current_uptime_sec INTEGER NOT NULL DEFAULT 0,
            duration_score DOUBLE PRECISION NOT NULL DEFAULT 0.5,
            time_pattern_score DOUBLE PRECISION NOT NULL DEFAULT 0.5,
            base_score DOUBLE PRECISION NOT NULL DEFAULT 0.5,
            final_score DOUBLE PRECISION NOT NULL DEFAULT 0.5,
            today_received_raids INTEGER NOT NULL DEFAULT 0,
            last_computed_at TEXT NOT NULL,
            readiness_score DOUBLE PRECISION NOT NULL DEFAULT 0.5,
            fairness_score DOUBLE PRECISION NOT NULL DEFAULT 0.5,
            internal_sent_raids_30d INTEGER NOT NULL DEFAULT 0,
            internal_received_raids_7d INTEGER NOT NULL DEFAULT 0,
            internal_received_raids_30d INTEGER NOT NULL DEFAULT 0 )",
    ] {
        sqlx::query(ddl).execute(&pool).await.unwrap();
    }
    pool
}

// ── Seeding-Helfer ──

async fn seed_source_token(pool: &PgPool, user_id: &str) {
    let cipher = FieldCipher::from_hex_key(TEST_KEY_HEX, KID).unwrap();
    let acc = cipher
        .encrypt_field("acc-tok", &aad::raid_auth("access_token", user_id, 1))
        .unwrap();
    let refr = cipher
        .encrypt_field("ref-tok", &aad::raid_auth("refresh_token", user_id, 1))
        .unwrap();
    sqlx::query(
        "INSERT INTO twitch_raid_auth
            (twitch_user_id, twitch_login, raid_enabled, enc_version, enc_kid,
             access_token_enc, refresh_token_enc, token_expires_at)
         VALUES ($1, 'quelle', TRUE, 1, 'v1', $2, $3, $4)",
    )
    .bind(user_id)
    .bind(acc)
    .bind(refr)
    .bind(Utc::now() + Duration::minutes(60))
    .execute(pool)
    .await
    .unwrap();
}

async fn seed_score(pool: &PgPool, user_id: &str, final_score: f64, is_live: i32) {
    sqlx::query(
        "INSERT INTO twitch_partner_raid_scores
            (twitch_user_id, twitch_login, is_live, final_score, last_computed_at)
         VALUES ($1, $1, $2, $3, $4)",
    )
    .bind(user_id)
    .bind(is_live)
    .bind(final_score)
    .bind(Utc::now().to_rfc3339())
    .execute(pool)
    .await
    .unwrap();
}

fn partner(user_id: &str, login: &str) -> OnlineCandidate {
    OnlineCandidate {
        twitch_user_id: user_id.to_string(),
        twitch_login: login.to_string(),
        raid_enabled: true,
        stream: StreamData {
            viewer_count: 10,
            followers_total: 0,
            started_at: Some("2026-06-10T17:00:00+00:00".to_string()),
            game_name: Some("Deadlock".to_string()),
        },
    }
}

// ── Stubs ──

struct StubTokenClient;
#[async_trait::async_trait]
impl TwitchTokenClient for StubTokenClient {
    async fn refresh(&self, _t: &str) -> Result<TokenResponse, RefreshError> {
        Err(RefreshError::Other("nicht erwartet".into()))
    }
    async fn exchange_code(&self, _c: &str) -> Result<TokenResponse, RefreshError> {
        unreachable!()
    }
    async fn token_owner(&self, _a: &str) -> Result<TokenOwnerInfo, RefreshError> {
        unreachable!("token_owner im Test ungenutzt")
    }
}

/// RaidApi-Stub mit per-Ziel-Verhalten: `errors_by_target[to_id]` → Fehler.
struct TargetedRaidApi {
    errors_by_target: HashMap<String, String>,
    calls: Mutex<Vec<String>>,
}
#[async_trait::async_trait]
impl RaidApi for TargetedRaidApi {
    async fn start_raid(&self, _from: &str, to: &str, _token: &str) -> Result<(), String> {
        self.calls.lock().unwrap().push(to.to_string());
        match self.errors_by_target.get(to) {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }
}

struct StubReadiness {
    calls: Mutex<Vec<String>>,
}
#[async_trait::async_trait]
impl ArrivalReadiness for StubReadiness {
    async fn ensure_ready(&self, to_broadcaster_id: &str, _login: &str) -> bool {
        self.calls
            .lock()
            .unwrap()
            .push(to_broadcaster_id.to_string());
        true
    }
}

struct StubFallback {
    streams: Vec<FairnessCandidate>,
}
#[async_trait::async_trait]
impl FallbackStreamSource for StubFallback {
    async fn category_streams(
        &self,
        _category_id: &str,
        _language: &str,
        _limit: usize,
    ) -> Result<Vec<FairnessCandidate>, String> {
        Ok(self.streams.clone())
    }
}

/// Follower-Enricher-Stub: setzt `followers_total` aus einer user_id→Zahl-Map.
/// Fehlende IDs bleiben auf ihrem Sentinel (best-effort wie Helix).
struct StubFollowerEnricher {
    followers_by_id: HashMap<String, i32>,
}
#[async_trait::async_trait]
impl FollowerEnricher for StubFollowerEnricher {
    async fn enrich(&self, pool: &mut [FairnessCandidate]) {
        for candidate in pool.iter_mut() {
            if let Some(total) = self.followers_by_id.get(candidate.user_id.trim()) {
                candidate.followers_total = *total;
            }
        }
    }
}

struct ObservedFollowerEnricher {
    followers_by_id: HashMap<String, i32>,
    observation: FollowersEnrichmentObservation,
}
#[async_trait::async_trait]
impl FollowerEnricher for ObservedFollowerEnricher {
    async fn enrich(&self, pool: &mut [FairnessCandidate]) {
        for candidate in pool.iter_mut() {
            if let Some(total) = self.followers_by_id.get(candidate.user_id.trim()) {
                candidate.followers_total = *total;
            }
        }
    }

    async fn enrich_with_observability(
        &self,
        pool: &mut [FairnessCandidate],
    ) -> FollowersEnrichmentObservation {
        self.enrich(pool).await;
        self.observation.clone()
    }
}

#[derive(Default)]
struct RecordingObservabilitySink {
    events: Mutex<Vec<ObservabilityEvent>>,
}
impl RecordingObservabilitySink {
    fn events(&self) -> Vec<ObservabilityEvent> {
        self.events.lock().unwrap().clone()
    }
}
impl EventSink for RecordingObservabilitySink {
    fn emit(&self, event: &ObservabilityEvent) {
        self.events.lock().unwrap().push(event.clone());
    }
}

fn fixed_millis(value: u64) -> MillisSource {
    Arc::new(move || value)
}

// ── Aufbau ──

struct Harness {
    pipeline: AutoRaidPipeline,
    pending: Arc<Mutex<PendingRaidStore>>,
    api: Arc<TargetedRaidApi>,
    readiness: Arc<StubReadiness>,
}

fn build(
    pool: &PgPool,
    errors_by_target: HashMap<String, String>,
    fallback_streams: Vec<FairnessCandidate>,
) -> Harness {
    build_with_followers(pool, errors_by_target, fallback_streams, HashMap::new())
}

fn build_with_followers(
    pool: &PgPool,
    errors_by_target: HashMap<String, String>,
    fallback_streams: Vec<FairnessCandidate>,
    followers_by_id: HashMap<String, i32>,
) -> Harness {
    build_with_enricher(
        pool,
        errors_by_target,
        fallback_streams,
        Some(Arc::new(StubFollowerEnricher { followers_by_id })),
    )
}

fn build_with_enricher(
    pool: &PgPool,
    errors_by_target: HashMap<String, String>,
    fallback_streams: Vec<FairnessCandidate>,
    follower_enricher: Option<Arc<dyn FollowerEnricher>>,
) -> Harness {
    let cipher = Arc::new(FieldCipher::from_hex_key(TEST_KEY_HEX, KID).unwrap());
    let token_blacklist = Arc::new(TokenBlacklistStore::new(pool.clone()));
    let refresher = RaidTokenRefresher::new(
        pool.clone(),
        cipher.clone(),
        Arc::new(StubTokenClient),
        token_blacklist.clone(),
    );
    let provider = Arc::new(TokenProvider::new(
        RaidAuthStore::new(pool.clone(), cipher),
        refresher,
        token_blacklist,
    ));
    let api = Arc::new(TargetedRaidApi {
        errors_by_target,
        calls: Mutex::new(Vec::new()),
    });
    let executor = RaidExecutor::new(api.clone(), provider, RaidHistoryStore::new(pool.clone()));
    let pending = Arc::new(Mutex::new(PendingRaidStore::new()));
    let readiness = Arc::new(StubReadiness {
        calls: Mutex::new(Vec::new()),
    });
    let pipeline = AutoRaidPipeline::new(
        RaidBlacklistStore::new(pool.clone()),
        ScoreStore::new(pool.clone()),
        RaidHistoryStore::new(pool.clone()),
        StrikesStore::new(pool.clone()),
        executor,
        pending.clone(),
        readiness.clone(),
        Some(Arc::new(StubFallback {
            streams: fallback_streams,
        })),
        follower_enricher,
        Some(tb_raid::OutreachBoostStore::new(pool.clone())),
    );
    Harness {
        pipeline,
        pending,
        api,
        readiness,
    }
}

fn request(partners: Vec<OnlineCandidate>) -> AutoRaidRequest {
    AutoRaidRequest {
        broadcaster_id: "100".to_string(),
        broadcaster_login: "quelle".to_string(),
        viewer_count: 42,
        stream_duration_sec: 3600,
        partners,
        category_id: Some("cat-deadlock".to_string()),
        offline_trigger_ts: Some(1000.0),
        reason: "auto_raid_on_offline".to_string(),
    }
}

fn fairness(user_id: &str, login: &str) -> FairnessCandidate {
    FairnessCandidate {
        user_id: user_id.to_string(),
        user_login: login.to_string(),
        viewer_count: 5,
        followers_total: 0,
        started_at: "2026-06-10T16:00:00+00:00".to_string(),
    }
}

// ── Tests ──

#[tokio::test]
async fn partner_pfad_startet_raid_und_registriert_pending() {
    let pool = pool_or_skip!("t6w_pipe_ok");
    seed_source_token(&pool, "100").await;
    seed_score(&pool, "200", 0.9, 1).await;
    let h = build(&pool, HashMap::new(), vec![]);

    let outcome = h.pipeline.run(&request(vec![partner("200", "ziel")])).await;
    assert_eq!(
        outcome,
        AutoRaidPipelineOutcome::Started {
            target_login: "ziel".to_string(),
            is_partner_raid: true,
        }
    );
    // Readiness vor dem Start fürs Ziel sichergestellt.
    assert_eq!(h.readiness.calls.lock().unwrap().clone(), vec!["200"]);
    // History-Zeile mit Erfolg + Kandidatenzahl.
    let (success, candidates): (bool, i32) = sqlx::query_as(
        "SELECT success, candidates_count FROM twitch_raid_history WHERE to_broadcaster_id='200'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(success);
    assert_eq!(candidates, 1);
    // Pending-Raid registriert (Arrival-Korrelation).
    let store = h.pending.lock().unwrap();
    let pending = store.get("200", Some("quelle")).unwrap();
    assert!(pending.is_partner_raid);
    assert_eq!(pending.registered_viewer_count, 42);
    assert_eq!(pending.offline_trigger_ts, Some(1000.0));
    assert_eq!(pending.channel_raid_ready, Some(true));
}

#[tokio::test]
async fn observability_partner_flow_emittiert_attempt_started_und_counter() {
    let pool = pool_or_skip!("t6w_pipe_obs_started");
    seed_source_token(&pool, "100").await;
    seed_score(&pool, "200", 0.9, 1).await;
    let h = build(&pool, HashMap::new(), vec![]);
    let obs_sink = Arc::new(RecordingObservabilitySink::default());
    let raid_observability = Arc::new(RaidObservabilityService::with_millis_source(
        Some(obs_sink.clone()),
        fixed_millis(1234),
    ));
    let pipeline = h
        .pipeline
        .with_observability(Some(raid_observability.clone()), None);

    let outcome = pipeline.run(&request(vec![partner("200", "ziel")])).await;
    assert!(matches!(outcome, AutoRaidPipelineOutcome::Started { .. }));

    let events = obs_sink.events();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].step, "attempt_selected");
    assert_eq!(events[0].decision, "candidate_selected");
    assert_eq!(events[1].step, "raid_started");
    assert_eq!(events[1].decision, "success");
    assert!(events.iter().all(|event| event.flow_id == "raid-1234-1"));
    assert_eq!(
        raid_observability.counters().get("raid_flow_started_total"),
        Some(&1)
    );
    assert_eq!(events[0].details.get("candidates_count"), Some(&json!(1)));
    assert_eq!(
        events[0].details.get("reason"),
        Some(&json!("auto_raid_on_offline"))
    );
    assert!(
        events[0]
            .details
            .get("selection_ms")
            .and_then(|v| v.as_i64())
            .unwrap()
            > 0
    );
    assert!(
        events[1]
            .details
            .get("api_call_ms")
            .and_then(|v| v.as_i64())
            .unwrap()
            > 0
    );
    assert!(
        events[1]
            .details
            .get("total_ms")
            .and_then(|v| v.as_i64())
            .unwrap()
            > 0
    );
}

#[tokio::test]
async fn observability_retryable_flow_bleibt_auf_einer_flow_id() {
    let pool = pool_or_skip!("t6w_pipe_obs_retry");
    seed_source_token(&pool, "100").await;
    seed_score(&pool, "200", 0.9, 1).await;
    let errors: HashMap<String, String> = [(
        "200".to_string(),
        "HTTP 400: target does not allow raids".to_string(),
    )]
    .into();
    let h = build(&pool, errors, vec![fairness("300", "de_streamer")]);
    let obs_sink = Arc::new(RecordingObservabilitySink::default());
    let raid_observability = Arc::new(RaidObservabilityService::with_millis_source(
        Some(obs_sink.clone()),
        fixed_millis(2233),
    ));
    let pipeline = h
        .pipeline
        .with_observability(Some(raid_observability.clone()), None);

    let outcome = pipeline
        .run(&request(vec![partner("200", "partner_zu")]))
        .await;
    assert!(matches!(outcome, AutoRaidPipelineOutcome::Started { .. }));

    let events = obs_sink.events();
    let steps: Vec<&str> = events.iter().map(|event| event.step.as_str()).collect();
    assert_eq!(
        steps,
        vec![
            "attempt_selected",
            "raid_failed_retryable",
            "attempt_selected",
            "raid_started"
        ]
    );
    assert!(events.iter().all(|event| event.flow_id == "raid-2233-1"));
    assert_eq!(events[1].decision, "skip_blacklist");
    assert_eq!(events[1].details.get("attempt"), Some(&json!(1)));
    assert_eq!(events[3].details.get("attempt"), Some(&json!(2)));
    assert_eq!(
        raid_observability.counters().get("raid_flow_started_total"),
        Some(&2)
    );
}

#[tokio::test]
async fn observability_non_retryable_emittiert_raid_failed() {
    let pool = pool_or_skip!("t6w_pipe_obs_failed");
    seed_source_token(&pool, "100").await;
    seed_score(&pool, "200", 0.9, 1).await;
    let errors: HashMap<String, String> = [(
        "200".to_string(),
        "Raid API failed: HTTP 500: kaputt".to_string(),
    )]
    .into();
    let h = build(&pool, errors, vec![fairness("300", "unbenutzt")]);
    let obs_sink = Arc::new(RecordingObservabilitySink::default());
    let raid_observability = Arc::new(RaidObservabilityService::with_millis_source(
        Some(obs_sink.clone()),
        fixed_millis(3344),
    ));
    let pipeline = h
        .pipeline
        .with_observability(Some(raid_observability), None);

    let outcome = pipeline.run(&request(vec![partner("200", "ziel")])).await;
    assert!(matches!(outcome, AutoRaidPipelineOutcome::Failed { .. }));

    let events = obs_sink.events();
    let last = events.last().unwrap();
    assert_eq!(last.flow_id, "raid-3344-1");
    assert_eq!(last.step, "raid_failed");
    assert_eq!(last.decision, "non_retryable");
    assert_eq!(
        last.details.get("error"),
        Some(&json!("Raid API failed: HTTP 500: kaputt"))
    );
}

/// P2.30: Geht eine `channel.chat.notification` ein BEVOR das Pending
/// registriert ist, muss beim Registrieren das Orphan-Signal gezogen und
/// nachgespielt werden (pending-korrelierte Bestätigung sofort, nicht erst
/// nach Grace als unabhängiges Arrival).
#[tokio::test]
async fn register_pending_spielt_orphan_chat_notification_nach() {
    use tb_raid::{OrphanChatNotification, OrphanReplay};

    struct StubOrphanReplay {
        orphan: Option<OrphanChatNotification>,
        popped: Mutex<Vec<(String, String)>>,
        replayed: Mutex<Vec<OrphanChatNotification>>,
    }
    #[async_trait::async_trait]
    impl OrphanReplay for StubOrphanReplay {
        async fn pop_orphan(
            &self,
            to_broadcaster_id: &str,
            from_broadcaster_login: &str,
        ) -> Option<OrphanChatNotification> {
            self.popped.lock().unwrap().push((
                to_broadcaster_id.to_string(),
                from_broadcaster_login.to_string(),
            ));
            self.orphan.clone()
        }
        async fn replay(&self, orphan: OrphanChatNotification) {
            self.replayed.lock().unwrap().push(orphan);
        }
    }

    let pool = pool_or_skip!("t6w_pipe_orphan");
    seed_source_token(&pool, "100").await;
    seed_score(&pool, "200", 0.9, 1).await;

    let orphan = OrphanChatNotification {
        to_broadcaster_id: "200".to_string(),
        to_broadcaster_login: "ziel".to_string(),
        from_broadcaster_login: "quelle".to_string(),
        viewer_count: 42,
        from_broadcaster_id: Some("100".to_string()),
        message_id: Some("m-orphan-1".to_string()),
        event_timestamp: Some("2026-06-21T18:00:00+00:00".to_string()),
    };
    let replay = Arc::new(StubOrphanReplay {
        orphan: Some(orphan.clone()),
        popped: Mutex::new(Vec::new()),
        replayed: Mutex::new(Vec::new()),
    });

    let h = build(&pool, HashMap::new(), vec![]);
    let pipeline = h.pipeline.with_orphan_replay(replay.clone());

    let outcome = pipeline.run(&request(vec![partner("200", "ziel")])).await;
    assert!(matches!(outcome, AutoRaidPipelineOutcome::Started { .. }));

    // Orphan wurde für (target_id, source_login) gezogen …
    assert_eq!(
        replay.popped.lock().unwrap().clone(),
        vec![("200".to_string(), "quelle".to_string())]
    );
    // … und genau einmal nachgespielt.
    assert_eq!(replay.replayed.lock().unwrap().clone(), vec![orphan]);
    // Pending ist registriert (Store-Schritt lief vor dem Replay).
    assert!(h
        .pending
        .lock()
        .unwrap()
        .get("200", Some("quelle"))
        .is_some());
}

/// P2.30: Ohne passenden Orphan wird nichts nachgespielt (Replay-Liste leer).
#[tokio::test]
async fn register_pending_ohne_orphan_kein_replay() {
    use tb_raid::{OrphanChatNotification, OrphanReplay};

    struct EmptyOrphanReplay {
        replayed: Mutex<usize>,
    }
    #[async_trait::async_trait]
    impl OrphanReplay for EmptyOrphanReplay {
        async fn pop_orphan(&self, _t: &str, _f: &str) -> Option<OrphanChatNotification> {
            None
        }
        async fn replay(&self, _orphan: OrphanChatNotification) {
            *self.replayed.lock().unwrap() += 1;
        }
    }

    let pool = pool_or_skip!("t6w_pipe_no_orphan");
    seed_source_token(&pool, "100").await;
    seed_score(&pool, "200", 0.9, 1).await;

    let replay = Arc::new(EmptyOrphanReplay {
        replayed: Mutex::new(0),
    });
    let pipeline = build(&pool, HashMap::new(), vec![])
        .pipeline
        .with_orphan_replay(replay.clone());

    let outcome = pipeline.run(&request(vec![partner("200", "ziel")])).await;
    assert!(matches!(outcome, AutoRaidPipelineOutcome::Started { .. }));
    assert_eq!(
        *replay.replayed.lock().unwrap(),
        0,
        "kein Replay ohne Orphan"
    );
}

#[tokio::test]
async fn partner_lehnt_ab_wird_uebersprungen_fallback_uebernimmt() {
    let pool = pool_or_skip!("t6w_pipe_fallback");
    seed_source_token(&pool, "100").await;
    seed_score(&pool, "200", 0.9, 1).await;
    let errors: HashMap<String, String> = [(
        "200".to_string(),
        "HTTP 400: target does not allow raids".to_string(),
    )]
    .into();
    let h = build(&pool, errors, vec![fairness("300", "de_streamer")]);

    let outcome = h
        .pipeline
        .run(&request(vec![partner("200", "partner_zu")]))
        .await;
    assert_eq!(
        outcome,
        AutoRaidPipelineOutcome::Started {
            target_login: "de_streamer".to_string(),
            is_partner_raid: false,
        }
    );
    // Partner-Ziel: KEIN Strike, KEINE Blacklist (nur überspringen).
    let strikes: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_raid_disabled_strikes")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(strikes, 0);
    let blacklisted: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_raid_blacklist")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(blacklisted, 0);
    // Beide Versuche in der History (Fehlschlag + Erfolg).
    let attempts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_raid_history")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(attempts, 2);
    assert_eq!(h.api.calls.lock().unwrap().clone(), vec!["200", "300"]);
}

#[tokio::test]
async fn fallback_ziel_sammelt_strike_und_blacklist_ab_schwelle() {
    let pool = pool_or_skip!("t6w_pipe_strike");
    seed_source_token(&pool, "100").await;
    // Vorbelastung: Ziel 300 hat bereits 1 Strike.
    sqlx::query(
        "INSERT INTO twitch_raid_disabled_strikes (target_id, target_login, strike_count)
         VALUES ('300', 'de_zu', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();
    let errors: HashMap<String, String> = [(
        "300".to_string(),
        "HTTP 400: raids are disabled".to_string(),
    )]
    .into();
    let h = build(&pool, errors, vec![fairness("300", "de_zu")]);

    let outcome = h.pipeline.run(&request(vec![])).await;
    // Einziges Ziel abgelehnt → danach kein Ziel mehr.
    assert_eq!(outcome, AutoRaidPipelineOutcome::NoTarget);
    // Strike 2 erreicht → Blacklist-Eintrag.
    let strike_count: i32 = sqlx::query_scalar(
        "SELECT strike_count FROM twitch_raid_disabled_strikes WHERE target_login='de_zu'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(strike_count, 2);
    let reason: String =
        sqlx::query_scalar("SELECT reason FROM twitch_raid_blacklist WHERE target_login='de_zu'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(reason.contains("raids are disabled"));
}

#[tokio::test]
async fn nicht_wiederholbarer_fehler_bricht_ab() {
    let pool = pool_or_skip!("t6w_pipe_fatal");
    seed_source_token(&pool, "100").await;
    seed_score(&pool, "200", 0.9, 1).await;
    let errors: HashMap<String, String> = [(
        "200".to_string(),
        "Raid API failed: HTTP 500: kaputt".to_string(),
    )]
    .into();
    let h = build(&pool, errors, vec![fairness("300", "unbenutzt")]);

    let outcome = h.pipeline.run(&request(vec![partner("200", "ziel")])).await;
    assert_eq!(
        outcome,
        AutoRaidPipelineOutcome::Failed {
            error: "Raid API failed: HTTP 500: kaputt".to_string()
        }
    );
    // Kein weiterer Versuch nach nicht-wiederholbarem Fehler.
    assert_eq!(h.api.calls.lock().unwrap().len(), 1);
    assert!(h.pending.lock().unwrap().is_empty());
}

#[tokio::test]
async fn blacklist_und_quelle_werden_nie_geraidet() {
    let pool = pool_or_skip!("t6w_pipe_blocked");
    seed_source_token(&pool, "100").await;
    seed_score(&pool, "200", 0.9, 1).await;
    sqlx::query(
        "INSERT INTO twitch_raid_blacklist (target_login, target_id) VALUES ('boese', '200')",
    )
    .execute(&pool)
    .await
    .unwrap();
    // Quelle selbst als "Partner" (Roster filtert das normal schon) + Blacklist-Ziel.
    let h = build(&pool, HashMap::new(), vec![]);
    let outcome = h
        .pipeline
        .run(&request(vec![
            partner("100", "quelle"),
            partner("200", "boese"),
        ]))
        .await;
    assert_eq!(outcome, AutoRaidPipelineOutcome::NoTarget);
    assert!(h.api.calls.lock().unwrap().is_empty(), "kein API-Aufruf");
}

#[tokio::test]
async fn blacklist_globale_bans_und_id_only_exclusions_filtern_fallback() {
    let pool = pool_or_skip!("t6w_pipe_global_ban_filter");
    seed_source_token(&pool, "100").await;

    sqlx::query(
        "INSERT INTO twitch_raid_blacklist (target_login, target_id, reason)
         VALUES ('raid_blacklisted', '300', 'raid blacklist')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO twitch_chatter_global_ban (chatter_login, chatter_id, reason)
         VALUES ('global_banned', '400', 'global ban')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO twitch_streamers (twitch_login, twitch_user_id)
         VALUES ('exclusion_banned', '500')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO twitch_exclusions (twitch_user_id, kind, reason)
         VALUES ('500', 'banned', 'hard ban')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO twitch_exclusions (twitch_user_id, kind, reason)
         VALUES ('550', 'banned', 'id-only hard ban')",
    )
    .execute(&pool)
    .await
    .unwrap();

    let mut raid_blacklisted = fairness("300", "raid_blacklisted");
    raid_blacklisted.viewer_count = 1;
    let mut global_banned = fairness("400", "global_banned");
    global_banned.viewer_count = 2;
    let mut exclusion_banned = fairness("500", "exclusion_banned");
    exclusion_banned.viewer_count = 3;
    let mut id_only_exclusion_banned = fairness("550", "id_only_exclusion_banned");
    id_only_exclusion_banned.viewer_count = 1;
    let mut allowed = fairness("600", "allowed_target");
    allowed.viewer_count = 20;

    let h = build(
        &pool,
        HashMap::new(),
        vec![
            raid_blacklisted,
            global_banned,
            exclusion_banned,
            id_only_exclusion_banned,
            allowed,
        ],
    );

    let outcome = h.pipeline.run(&request(vec![])).await;
    assert_eq!(
        outcome,
        AutoRaidPipelineOutcome::Started {
            target_login: "allowed_target".to_string(),
            is_partner_raid: false,
        },
        "Raid-Blacklist, globale Ban-Liste und Exclusion-Bans inklusive ID-only-Bans muessen vor der Auswahl greifen"
    );
    assert_eq!(h.api.calls.lock().unwrap().clone(), vec!["600"]);
}

#[tokio::test]
async fn fallback_follower_anreicherung_entscheidet_tie_break() {
    // Zwei DE-Fallback-Streams, gleiche Raids (0) + gleiche Viewer: die
    // 3. Tie-Break-Ebene (Follower) muss entscheiden. Beide starten auf dem
    // FOLLOWERS_UNKNOWN-Sentinel; der Enricher füllt echte Zahlen — der
    // follower-ärmere Kandidat (Python-konform) gewinnt, NICHT der zuerst
    // einsortierte oder früher gestartete.
    let pool = pool_or_skip!("t6w_pipe_follower_tiebreak");
    seed_source_token(&pool, "100").await;

    // Gleiche Viewer + gleiche Startzeit → nur Follower trennt sie.
    let mut gross = fairness("300", "viele_follower");
    gross.viewer_count = 5;
    gross.followers_total = FOLLOWERS_UNKNOWN;
    let mut klein = fairness("400", "wenig_follower");
    klein.viewer_count = 5;
    klein.followers_total = FOLLOWERS_UNKNOWN;

    // 300 hat 9000 Follower, 400 nur 100 → 400 gewinnt nach Anreicherung.
    let followers: HashMap<String, i32> =
        [("300".to_string(), 9000), ("400".to_string(), 100)].into();
    let h = build_with_followers(&pool, HashMap::new(), vec![gross, klein], followers);

    let outcome = h.pipeline.run(&request(vec![])).await;
    assert_eq!(
        outcome,
        AutoRaidPipelineOutcome::Started {
            target_login: "wenig_follower".to_string(),
            is_partner_raid: false,
        },
        "follower-ärmerer Kandidat gewinnt den Tie-Break (Python-Parität)"
    );
    assert_eq!(h.api.calls.lock().unwrap().clone(), vec!["400"]);
}

#[tokio::test]
async fn followers_observability_mappt_ok_http_error_und_request_error() {
    let pool = pool_or_skip!("t6w_pipe_obs_followers");
    seed_source_token(&pool, "100").await;
    let obs_sink = Arc::new(RecordingObservabilitySink::default());
    let analytics = Arc::new(AnalyticsObservabilityService::with_millis_source(
        Some(obs_sink.clone()),
        fixed_millis(9000),
    ));

    let cases = vec![
        (
            "300",
            "followers_ok",
            FollowersEnrichmentObservation::ok(1, 1),
            "ok",
            None,
        ),
        (
            "400",
            "followers_http",
            FollowersEnrichmentObservation::http_error(403, "missing_scope"),
            "http_error",
            Some(403),
        ),
        (
            "500",
            "followers_request",
            FollowersEnrichmentObservation::request_error("helix_timeout"),
            "request_error",
            None,
        ),
    ];

    for (user_id, login, observation, expected_result, _expected_status) in &cases {
        let mut candidate = fairness(user_id, login);
        candidate.followers_total = FOLLOWERS_UNKNOWN;
        let enricher = ObservedFollowerEnricher {
            followers_by_id: [(user_id.to_string(), 100)].into(),
            observation: observation.clone(),
        };
        let h = build_with_enricher(
            &pool,
            HashMap::new(),
            vec![candidate],
            Some(Arc::new(enricher)),
        );
        let pipeline = h.pipeline.with_observability(None, Some(analytics.clone()));

        let outcome = pipeline.run(&request(vec![])).await;
        assert!(
            matches!(outcome, AutoRaidPipelineOutcome::Started { .. }),
            "Followers-Fall {expected_result} sollte den Raid nicht blockieren"
        );
    }

    let events = obs_sink.events();
    let analytics_events: Vec<&ObservabilityEvent> = events
        .iter()
        .filter(|event| event.flow_type == "analytics")
        .collect();
    assert_eq!(analytics_events.len(), 3);
    for (idx, event) in analytics_events.iter().enumerate() {
        assert_eq!(event.step, "terminal_decision");
        assert_eq!(event.decision, "terminal_decision");
        assert_eq!(event.details.get("flow"), Some(&json!("followers")));
        assert_eq!(event.details.get("login"), Some(&json!("quelle")));
        assert_eq!(
            event.details.get("request_result"),
            Some(&json!(cases[idx].3))
        );
        assert_eq!(event.details.get("request_attempted"), Some(&json!(true)));
        match cases[idx].4 {
            Some(status) => {
                assert_eq!(event.details.get("http_status"), Some(&json!(status)));
            }
            None => {
                assert_eq!(event.details.get("http_status"), Some(&json!(null)));
            }
        }
    }
    assert_eq!(
        analytics_events[1].details.get("error_code"),
        Some(&json!("missing_scope"))
    );
    assert_eq!(
        analytics_events[2].details.get("error_code"),
        Some(&json!("helix_timeout"))
    );
}

#[tokio::test]
async fn outreach_boost_gewinnt_vor_partner_und_wird_verbraucht() {
    let pool = pool_or_skip!("t6w_pipe_boost");
    seed_source_token(&pool, "100").await;
    seed_score(&pool, "200", 0.9, 1).await;
    // Frischer Outreach-Empfänger "300" — in den Kategorie-Streams vorhanden.
    sqlx::query(
        "INSERT INTO twitch_partner_outreach (streamer_login, status, contacted_at)
         VALUES ('boost_ziel', 'sent', (NOW() - INTERVAL '2 hours')::text)",
    )
    .execute(&pool)
    .await
    .unwrap();
    let h = build(&pool, HashMap::new(), vec![fairness("300", "boost_ziel")]);

    let outcome = h
        .pipeline
        .run(&request(vec![partner("200", "partner")]))
        .await;
    assert_eq!(
        outcome,
        AutoRaidPipelineOutcome::Started {
            target_login: "boost_ziel".to_string(),
            is_partner_raid: false,
        },
        "Boost-Ziel schlägt den online Partner"
    );
    assert_eq!(h.api.calls.lock().unwrap().clone(), vec!["300"]);
    // Boost per CAS verbraucht.
    let used: Option<String> = sqlx::query_scalar(
        "SELECT raid_used_at FROM twitch_partner_outreach WHERE streamer_login='boost_ziel'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(used.is_some(), "raid_used_at gesetzt");
}
