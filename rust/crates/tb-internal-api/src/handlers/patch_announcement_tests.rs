use super::*;
#[allow(clippy::duplicate_mod)]
#[path = "../../../../test-support/postgres.rs"]
mod postgres;
use postgres::TestPostgres;
use std::{
    collections::HashSet,
    process::Command,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc, Arc, Mutex,
    },
    time::Duration,
};
use tb_chat::api::{BanOutcome, SourceOnlyPreSendCheck};
use tb_chat::channel_policy::{ChannelPolicyChatApi, PolicyContext};
use tb_chat::global_ban_sweep::PartnerRoster;
use tb_chat::moderation::TimeoutGuard;
use tb_chat::timeout_tracking::TimeoutTrackingChatApi;
use tb_transport_twitch::HelixConfig;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

struct FakeTransport {
    pool: PgPool,
    streams: Mutex<Vec<HelixStream>>,
    sent: Mutex<Vec<(String, String)>>,
    fail: AtomicBool,
    fail_after_claim: AtomicBool,
    saw_attempt: AtomicBool,
    leave_after_snapshot: AtomicBool,
    revoke_before_send: AtomicBool,
    stream_calls: AtomicUsize,
    outcome: &'static str,
    known_result: Mutex<Option<DeliveryResult>>,
    send_delay: Duration,
    send_started: Arc<tokio::sync::Notify>,
}

impl FakeTransport {
    fn new(pool: PgPool) -> Self {
        Self {
            pool,
            streams: Mutex::new(vec![stream("42")]),
            sent: Mutex::new(vec![]),
            fail: AtomicBool::new(false),
            fail_after_claim: AtomicBool::new(false),
            saw_attempt: AtomicBool::new(false),
            leave_after_snapshot: AtomicBool::new(false),
            revoke_before_send: AtomicBool::new(false),
            stream_calls: AtomicUsize::new(0),
            outcome: "sent",
            known_result: Mutex::new(None),
            send_delay: Duration::ZERO,
            send_started: Arc::new(tokio::sync::Notify::new()),
        }
    }
}

#[async_trait::async_trait]
impl Transport for FakeTransport {
    async fn streams(&self, ids: &[String]) -> Result<Vec<HelixStream>, PatchProcessError> {
        if self.fail.load(Ordering::SeqCst) {
            return Err(PatchProcessError::Unavailable);
        }
        let result = {
            let mut all = self.streams.lock().unwrap();
            let result = all
                .iter()
                .filter(|stream| ids.contains(&stream.user_id))
                .cloned()
                .collect();
            if self.leave_after_snapshot.swap(false, Ordering::SeqCst) {
                all.clear();
            }
            result
        };
        if self.stream_calls.fetch_add(1, Ordering::SeqCst) > 0
            && self.revoke_before_send.swap(false, Ordering::SeqCst)
        {
            sqlx::query(
                "UPDATE twitch_streamers_partner_state SET manual_partner_opt_out=1 \
                 WHERE twitch_user_id='42'",
            )
            .execute(&self.pool)
            .await
            .map_err(database_error)?;
        }
        Ok(result)
    }

    async fn can_send(
        &self,
        id: &str,
        stream_id: &str,
    ) -> Result<Option<&'static str>, PatchProcessError> {
        authorized_login(&self.pool, id, stream_id)
            .await
            .map(|login| login.is_none().then_some("authorization_unavailable"))
    }

    async fn send(
        &self,
        id: &str,
        _stream_id: &str,
        message: &str,
        _detected_at: DateTime<Utc>,
    ) -> Result<DeliveryResult, PatchProcessError> {
        let attempted: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM twitch_patch_announcement_deliveries \
             WHERE broadcaster_id=$1 AND status='attempted' AND attempted_at IS NOT NULL",
        )
        .bind(id)
        .fetch_one(&self.pool)
        .await
        .map_err(database_error)?;
        self.saw_attempt.store(attempted == 1, Ordering::SeqCst);
        if self.fail_after_claim.load(Ordering::SeqCst) {
            return Err(PatchProcessError::Unavailable);
        }
        self.sent.lock().unwrap().push((id.into(), message.into()));
        self.send_started.notify_one();
        tokio::time::sleep(self.send_delay).await;
        Ok(self
            .known_result
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(|| DeliveryResult::status(self.outcome)))
    }
}

fn stream(id: &str) -> HelixStream {
    HelixStream {
        id: format!("session-{id}"),
        user_id: id.into(),
        game_id: "deadlock".into(),
        game_name: "Deadlock".into(),
        started_at: (Utc::now() - chrono::Duration::hours(1)).to_rfc3339(),
        ..Default::default()
    }
}

fn patch_event(id: i64) -> PatchEvent {
    let now = Utc::now();
    let observed_at = DateTime::from_timestamp_micros(now.timestamp_micros()).unwrap();
    PatchEvent::from_article(
        id,
        format!("https://deutsche-deadlock-community.de/patchnotes/patch-{id}/"),
        "https://forums.playdeadlock.com/posts/123/".into(),
        observed_at,
    )
    .unwrap()
}

async fn database() -> TestPostgres {
    let postgres = TestPostgres::start().await;
    let pool = postgres.pool.clone();
    sqlx::raw_sql(include_str!(
        "../../../../migrations/20260928120000_patch_announcements.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    sqlx::raw_sql(
        "CREATE TABLE twitch_streamers_partner_state (
             twitch_user_id TEXT, twitch_login TEXT, is_partner_active INT,
             manual_partner_opt_out INT DEFAULT 0);
         CREATE TABLE twitch_raid_auth (twitch_user_id TEXT, scopes TEXT, needs_reauth BOOLEAN);
         CREATE TABLE twitch_live_state (
             twitch_user_id TEXT, last_stream_id TEXT, last_seen_at TEXT, is_live INT, last_game TEXT);
         INSERT INTO twitch_streamers_partner_state VALUES
             ('42','renamed',1,0), ('43','later',1,0), ('44','optout',1,1), ('99','notpartner',0,0);
         INSERT INTO twitch_raid_auth VALUES
             ('42','user:read:chat channel:bot',FALSE), ('43','channel:bot',FALSE),
             ('44','channel:bot',FALSE), ('99','channel:bot',FALSE);",
    )
    .execute(&pool)
    .await
    .unwrap();
    for (id, game) in [
        ("42", "Deadlock"),
        ("43", "Other"),
        ("44", "Deadlock"),
        ("99", "Deadlock"),
    ] {
        sqlx::query("INSERT INTO twitch_live_state VALUES ($1,$2,$3,1,$4)")
            .bind(id)
            .bind(format!("session-{id}"))
            .bind(Utc::now().to_rfc3339())
            .bind(game)
            .execute(&pool)
            .await
            .unwrap();
    }
    sqlx::raw_sql(
        "INSERT INTO twitch_patch_feed_observations (patch_id, observed_at, status) VALUES \
         (286, now(), 'pending'), (287, now(), 'pending'), \
         (288, now(), 'pending'), (289, now(), 'pending')",
    )
    .execute(&pool)
    .await
    .unwrap();
    postgres
}

#[tokio::test]
async fn suppression_database_failure_blocks_patch_send() {
    let db = database().await;
    let helix = HelixClient::new(HelixConfig::new("cid", "sec")).unwrap();
    let receiver = PatchReceiver::new(
        db.pool.clone(),
        helix.clone(),
        Arc::new(MockEndpointChat { helix }),
        Arc::new(tb_chat::CombinedSuppression::new(
            Arc::new(tb_chat::OutboundSuppressionStore::new(db.pool.clone())),
            Arc::new(tb_chat::TimeoutGuard::new()),
        )),
    );
    let transport = LiveTransport {
        receiver: &receiver,
    };
    assert!(matches!(
        transport.can_send("42", "session-42").await,
        Err(PatchProcessError::Unavailable)
    ));
}

#[tokio::test]
async fn concurrent_retries_and_source_duplicates_send_exactly_once() {
    let db = database().await;
    let pool = db.pool.clone();
    let transport = FakeTransport::new(pool.clone());
    let event = patch_event(286);
    let (first, second) = tokio::join!(
        process(&pool, &transport, &event),
        process(&pool, &transport, &event)
    );
    assert!(matches!(first.unwrap(), PatchProcessOutcome::Processed(_)));
    assert!(matches!(second.unwrap(), PatchProcessOutcome::Processed(_)));
    assert!(transport.saw_attempt.load(Ordering::SeqCst));
    let duplicate = patch_event(287);
    assert!(matches!(
        process(&pool, &transport, &duplicate).await.unwrap(),
        PatchProcessOutcome::DuplicateSource
    ));
    assert_eq!(transport.sent.lock().unwrap().len(), 1);
    let status: String =
        sqlx::query_scalar("SELECT status FROM twitch_patch_announcement_deliveries")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status, "sent");
}

#[tokio::test]
async fn expired_event_records_reason_for_each_pending_target() {
    let db = database().await;
    let transport = FakeTransport::new(db.pool.clone());
    let mut event = patch_event(286);
    event.detected_at -= chrono::Duration::seconds(121);
    sqlx::query("UPDATE twitch_patch_feed_observations SET observed_at=$2 WHERE patch_id=286")
        .bind(286_i64)
        .bind(event.detected_at)
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO twitch_patch_announcements \
         (event_id, article_url, source_url, detected_at, message) VALUES ($1,$2,$3,$4,$5)",
    )
    .bind(&event.event_id)
    .bind(&event.article_url)
    .bind(&event.source_url)
    .bind(event.detected_at)
    .bind(&event.message)
    .execute(&db.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO twitch_patch_announcement_deliveries \
         (event_id, broadcaster_id, stream_id) VALUES ($1,'42','session-42'),($1,'43','session-43')",
    )
    .bind(&event.event_id)
    .execute(&db.pool)
    .await
    .unwrap();

    assert!(matches!(
        process_inner(
            &db.pool,
            &transport,
            &event,
            Arc::new(Mutex::new(HashSet::new()))
        )
        .await
        .unwrap(),
        PatchProcessOutcome::SkippedExpired
    ));
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT broadcaster_id, status, uncertainty_reason \
         FROM twitch_patch_announcement_deliveries WHERE event_id=$1 ORDER BY broadcaster_id",
    )
    .bind(&event.event_id)
    .fetch_all(&db.pool)
    .await
    .unwrap();
    assert_eq!(
        rows,
        vec![
            (
                "42".into(),
                "skipped".into(),
                "event_expired_before_claim".into(),
            ),
            (
                "43".into(),
                "skipped".into(),
                "event_expired_before_claim".into(),
            ),
        ]
    );
    assert!(transport.sent.lock().unwrap().is_empty());
}

#[tokio::test]
async fn receiver_requires_the_saved_pending_observation_timestamp() {
    let db = database().await;
    let transport = FakeTransport::new(db.pool.clone());
    let event = patch_event(286);
    assert!(matches!(
        process_inner(
            &db.pool,
            &transport,
            &event,
            Arc::new(Mutex::new(HashSet::new()))
        )
        .await,
        Err(PatchProcessError::Invalid(_))
    ));
    sqlx::query(
        "UPDATE twitch_patch_feed_observations \
         SET observed_at=$2, status='expired_unavailable' WHERE patch_id=286",
    )
    .bind(286_i64)
    .bind(event.detected_at)
    .execute(&db.pool)
    .await
    .unwrap();
    assert!(matches!(
        process_inner(
            &db.pool,
            &transport,
            &event,
            Arc::new(Mutex::new(HashSet::new()))
        )
        .await,
        Err(PatchProcessError::Invalid(_))
    ));
    assert!(transport.sent.lock().unwrap().is_empty());
    let announcements: i64 = sqlx::query_scalar("SELECT count(*) FROM twitch_patch_announcements")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(announcements, 0);
}

#[tokio::test]
async fn unexpected_success_http_status_is_uncertain_and_never_retried() {
    let db = database().await;
    let pool = db.pool.clone();
    let transport = FakeTransport::new(pool.clone());
    for (patch_id, status, body, expected_reason) in [
        (286, 204, "", "unexpected_success_status"),
        (287, 422, "", "http_error"),
        (288, 503, "", "http_error"),
        (
            289,
            200,
            "response_body_unreadable",
            "response_body_unreadable",
        ),
    ] {
        *transport.known_result.lock().unwrap() =
            Some(delivery_result_from_send_outcome(SendOutcome::HttpError {
                status,
                body: body.to_string(),
            }));
        let mut event = patch_event(patch_id);
        event.source_url = format!("https://forums.playdeadlock.com/posts/{patch_id}/");
        process(&pool, &transport, &event).await.unwrap();
        process(&pool, &transport, &event).await.unwrap();
        let (delivery_status, http_status, reason): (String, Option<i16>, Option<String>) =
            sqlx::query_as(
                "SELECT status, http_status, uncertainty_reason \
                 FROM twitch_patch_announcement_deliveries WHERE event_id=$1",
            )
            .bind(&event.event_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(delivery_status, "uncertain");
        assert_eq!(http_status, Some(status as i16));
        assert_eq!(reason.as_deref(), Some(expected_reason));
    }
    assert_eq!(transport.sent.lock().unwrap().len(), 4);
}

#[tokio::test]
async fn uncertain_drop_and_crash_after_claim_never_retry() {
    let db = database().await;
    let pool = db.pool.clone();
    let mut transport = FakeTransport::new(pool.clone());
    transport.outcome = "uncertain";
    let event = patch_event(286);
    process(&pool, &transport, &event).await.unwrap();
    process(&pool, &transport, &event).await.unwrap();
    assert_eq!(transport.sent.lock().unwrap().len(), 1);
    let mut changed = event.clone();
    changed.message.push('!');
    assert!(process(&pool, &transport, &changed).await.is_err());
    let mut dropped = patch_event(287);
    dropped.source_url = "https://forums.playdeadlock.com/posts/456/".into();
    transport.outcome = "dropped";
    process(&pool, &transport, &dropped).await.unwrap();
    process(&pool, &transport, &dropped).await.unwrap();
    assert_eq!(transport.sent.lock().unwrap().len(), 2);
    let mut crash = patch_event(288);
    crash.source_url = "https://forums.playdeadlock.com/posts/789/".into();
    transport.fail_after_claim.store(true, Ordering::SeqCst);
    assert!(process(&pool, &transport, &crash).await.is_err());
    transport.fail_after_claim.store(false, Ordering::SeqCst);
    process(&pool, &transport, &crash).await.unwrap();
    assert_eq!(transport.sent.lock().unwrap().len(), 2);
    let statuses: Vec<String> = sqlx::query_scalar(
        "SELECT status FROM twitch_patch_announcement_deliveries ORDER BY event_id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(statuses.len(), 3);
    assert!(statuses.contains(&"uncertain".into()));
    assert!(statuses.contains(&"dropped".into()));
    assert!(statuses.contains(&"attempted".into()));
}

#[tokio::test]
async fn known_drop_with_final_update_failure_is_terminal_without_retry() {
    let db = database().await;
    let pool = db.pool.clone();
    let transport = FakeTransport::new(pool.clone());
    *transport.known_result.lock().unwrap() = Some(DeliveryResult {
        status: "dropped",
        drop_code: Some("sender_timedout".into()),
        http_status: None,
        uncertainty_reason: None,
    });
    sqlx::raw_sql(
        "CREATE FUNCTION reject_known_drop_update() RETURNS trigger LANGUAGE plpgsql AS $$ \
         BEGIN IF OLD.status='attempted' AND NEW.status='dropped' THEN \
         RAISE EXCEPTION 'forced final update failure' USING ERRCODE='23514'; \
         END IF; RETURN NEW; END $$; \
         CREATE TRIGGER reject_known_drop_update BEFORE UPDATE ON twitch_patch_announcement_deliveries \
         FOR EACH ROW EXECUTE FUNCTION reject_known_drop_update();",
    )
    .execute(&pool)
    .await
    .unwrap();
    let event = patch_event(286);
    assert!(matches!(
        process(&pool, &transport, &event).await,
        Err(PatchProcessError::Database { sqlstate: Some(ref code) }) if code == "23514"
    ));
    let (status, drop_code): (String, Option<String>) = sqlx::query_as(
        "SELECT status, drop_code FROM twitch_patch_announcement_deliveries WHERE event_id=$1",
    )
    .bind(&event.event_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(status, "attempted");
    assert_eq!(drop_code, None);
    process(&pool, &transport, &event).await.unwrap();
    assert_eq!(transport.sent.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn timeout_after_external_send_marks_only_its_attempt_uncertain() {
    let db = database().await;
    let pool = db.pool.clone();
    let mut transport = FakeTransport::new(pool.clone());
    transport.send_delay = Duration::from_secs(10);
    let event = patch_event(286);
    approve_observation(&pool, &event).await;
    let processing = process_with_timeout(&pool, &transport, &event, Duration::from_secs(2));
    tokio::pin!(processing);
    tokio::select! {
        _ = transport.send_started.notified() => {}
        result = &mut processing => panic!("processing ended before the send began: {result:?}"),
    }
    sqlx::query(
        "INSERT INTO twitch_patch_announcement_deliveries \
         (event_id, broadcaster_id, stream_id, status, attempted_at) \
         VALUES ($1, '43', 'session-43', 'attempted', now())",
    )
    .bind(&event.event_id)
    .execute(&pool)
    .await
    .unwrap();
    assert!(matches!(
        processing.await,
        Err(PatchProcessError::Unavailable)
    ));
    let (status, reason): (String, Option<String>) = sqlx::query_as(
        "SELECT status, uncertainty_reason FROM twitch_patch_announcement_deliveries \
         WHERE event_id=$1 AND broadcaster_id='42'",
    )
    .bind(&event.event_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(status, "uncertain");
    assert_eq!(reason.as_deref(), Some("receiver_timeout_after_attempt"));
    let other_status: String = sqlx::query_scalar(
        "SELECT status FROM twitch_patch_announcement_deliveries \
         WHERE event_id=$1 AND broadcaster_id='43'",
    )
    .bind(&event.event_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(other_status, "attempted");
    let other_update = sqlx::query(
        "UPDATE twitch_patch_announcement_deliveries SET status='sent' \
         WHERE event_id=$1 AND broadcaster_id='43' AND status='attempted'",
    )
    .bind(&event.event_id)
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(other_update.rows_affected(), 1);
    process_with_timeout(&pool, &transport, &event, Duration::from_secs(2))
        .await
        .unwrap();
    assert_eq!(transport.sent.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn helix_error_rolls_back_snapshot_then_retry_can_succeed() {
    let db = database().await;
    let pool = db.pool.clone();
    let transport = FakeTransport::new(pool.clone());
    transport.fail.store(true, Ordering::SeqCst);
    let event = patch_event(286);
    assert!(process(&pool, &transport, &event).await.is_err());
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM twitch_patch_announcements")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    assert!(transport.sent.lock().unwrap().is_empty());
    transport.fail.store(false, Ordering::SeqCst);
    process(&pool, &transport, &event).await.unwrap();
    assert_eq!(transport.sent.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn observation_snapshot_excludes_late_partner_and_rejects_game_switch() {
    let db = database().await;
    let pool = db.pool.clone();
    sqlx::query("UPDATE twitch_live_state SET last_game='Deadlock' WHERE twitch_user_id='43'")
        .execute(&pool)
        .await
        .unwrap();
    let transport = FakeTransport::new(pool.clone());
    *transport.streams.lock().unwrap() = vec![
        HelixStream {
            game_name: "Other".into(),
            ..stream("42")
        },
        stream("43"),
    ];
    let event = patch_event(286);
    process(&pool, &transport, &event).await.unwrap();
    assert!(transport.sent.lock().unwrap().is_empty());
    let (status, reason): (String, Option<String>) = sqlx::query_as(
        "SELECT status, uncertainty_reason FROM twitch_patch_announcement_deliveries \
         ORDER BY broadcaster_id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(status, "skipped");
    assert_eq!(reason.as_deref(), Some("game_changed"));
    let recipients: Vec<String> = sqlx::query_scalar(
        "SELECT broadcaster_id FROM twitch_patch_announcement_recipients \
         WHERE patch_id=286 ORDER BY broadcaster_id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(recipients, vec!["42"]);
}

#[tokio::test]
async fn stream_ending_before_send_and_stale_snapshot_are_skipped() {
    let db = database().await;
    let pool = db.pool.clone();
    let transport = FakeTransport::new(pool.clone());
    transport.leave_after_snapshot.store(true, Ordering::SeqCst);
    let event = patch_event(286);
    process(&pool, &transport, &event).await.unwrap();
    assert!(transport.sent.lock().unwrap().is_empty());
    let (status, reason): (String, Option<String>) = sqlx::query_as(
        "SELECT status, uncertainty_reason FROM twitch_patch_announcement_deliveries",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(status, "skipped");
    assert_eq!(reason.as_deref(), Some("stream_not_live"));
    let mut next = patch_event(287);
    next.source_url = "https://forums.playdeadlock.com/posts/456/".into();
    transport.streams.lock().unwrap().push(stream("42"));
    sqlx::query("UPDATE twitch_live_state SET last_seen_at='2000-01-01T00:00:00Z'")
        .execute(&pool)
        .await
        .unwrap();
    process(&pool, &transport, &next).await.unwrap();
    assert!(transport.sent.lock().unwrap().is_empty());
}

#[tokio::test]
async fn optout_after_snapshot_is_skipped_and_never_replayed() {
    let db = database().await;
    let pool = db.pool.clone();
    let transport = FakeTransport::new(pool.clone());
    transport.revoke_before_send.store(true, Ordering::SeqCst);
    let event = patch_event(286);
    process(&pool, &transport, &event).await.unwrap();
    assert!(transport.sent.lock().unwrap().is_empty());
    let (status, reason): (String, Option<String>) = sqlx::query_as(
        "SELECT status, uncertainty_reason FROM twitch_patch_announcement_deliveries",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(status, "skipped");
    assert_eq!(reason.as_deref(), Some("authorization_unavailable"));
    sqlx::query(
        "UPDATE twitch_streamers_partner_state SET manual_partner_opt_out=0 \
         WHERE twitch_user_id='42'",
    )
    .execute(&pool)
    .await
    .unwrap();
    process(&pool, &transport, &event).await.unwrap();
    assert!(transport.sent.lock().unwrap().is_empty());
}

#[tokio::test]
async fn rights_reauth_optout_and_expired_events_record_terminal_skips() {
    let db = database().await;
    let pool = db.pool.clone();
    let transport = FakeTransport::new(pool.clone());
    assert!(authorized_login(&pool, "42", "session-42")
        .await
        .unwrap()
        .is_some());
    let denied = patch_event(286);
    for (scopes, reauth) in [
        ("channel:bot:spoof", false),
        ("user:read:chat", false),
        ("channel:bot", true),
    ] {
        sqlx::query(
            "UPDATE twitch_raid_auth SET scopes=$1, needs_reauth=$2 WHERE twitch_user_id='42'",
        )
        .bind(scopes)
        .bind(reauth)
        .execute(&pool)
        .await
        .unwrap();
        assert!(authorized_login(&pool, "42", "session-42")
            .await
            .unwrap()
            .is_none());
        process(&pool, &transport, &denied).await.unwrap();
    }
    let (count, status, reason): (i64, String, Option<String>) = sqlx::query_as(
        "SELECT count(*), min(status), min(uncertainty_reason) \
         FROM twitch_patch_announcement_deliveries",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 1);
    assert_eq!(status, "skipped");
    assert_eq!(reason.as_deref(), Some("authorization_unavailable"));
    sqlx::query(
        "UPDATE twitch_raid_auth SET scopes='channel:bot', needs_reauth=FALSE \
         WHERE twitch_user_id='42'",
    )
    .execute(&pool)
    .await
    .unwrap();
    process(&pool, &transport, &denied).await.unwrap();
    assert!(transport.sent.lock().unwrap().is_empty());
    assert!(authorized_login(&pool, "44", "session-44")
        .await
        .unwrap()
        .is_none());
    let mut expired = patch_event(287);
    expired.detected_at -= chrono::Duration::minutes(3);
    expired.source_url = "https://forums.playdeadlock.com/posts/287/".into();
    assert!(matches!(
        process(&pool, &transport, &expired).await.unwrap(),
        PatchProcessOutcome::SkippedExpired
    ));
    assert!(transport.sent.lock().unwrap().is_empty());
}

struct MockEndpointChat {
    helix: HelixClient,
}

#[async_trait::async_trait]
impl ChatApi for MockEndpointChat {
    async fn send_message(&self, _: &str, _: &str) -> Result<SendOutcome, String> {
        Err("only guarded source-only sends are supported".into())
    }

    async fn send_source_only_message_guarded(
        &self,
        broadcaster_id: &str,
        message: &str,
        pre_send_check: SourceOnlyPreSendCheck,
    ) -> Result<SendOutcome, String> {
        self.helix
            .send_source_only_chat_message_guarded(broadcaster_id, "777", message, pre_send_check)
            .await
            .map_err(|error| error.to_string())
    }

    async fn send_announcement(&self, _: &str, _: &str, _: &str) -> Result<bool, String> {
        Err("unsupported".into())
    }

    async fn ban_user(&self, _: &str, _: &str, _: &str) -> Result<BanOutcome, String> {
        Err("unsupported".into())
    }

    async fn timeout_user(&self, _: &str, _: &str, _: u32, _: &str) -> Result<BanOutcome, String> {
        Err("unsupported".into())
    }

    async fn unban_user(&self, _: &str, _: &str) -> Result<bool, String> {
        Err("unsupported".into())
    }

    async fn delete_message(&self, _: &str, _: &str) -> Result<bool, String> {
        Err("unsupported".into())
    }

    async fn user_created_at(&self, _: &str) -> Result<Option<DateTime<Utc>>, String> {
        Err("unsupported".into())
    }

    async fn resolve_user_id(&self, _: &str) -> Result<Option<String>, String> {
        Err("unsupported".into())
    }

    async fn bot_user_id(&self) -> String {
        "777".into()
    }
}

struct PatchTestRoster {
    allowed: bool,
}

#[async_trait::async_trait]
impl PartnerRoster for PatchTestRoster {
    async fn all_active_partners(&self) -> Vec<(String, String)> {
        Vec::new()
    }

    async fn valid_auth_ids(&self) -> HashSet<String> {
        HashSet::new()
    }

    async fn live_broadcaster_ids(&self) -> HashSet<String> {
        HashSet::new()
    }

    async fn is_operational_partner_channel(&self, _: &str) -> bool {
        self.allowed
    }

    async fn global_ban_enforcement_enabled(&self, _: &str) -> bool {
        true
    }

    async fn streamer_global_ban_enabled(&self, _: &str) -> bool {
        true
    }
}

async fn patch_read_server() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth2/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "read-token", "expires_in": 3600
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/helix/streams"))
        .and(query_param("user_id", "42"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [{"id": "session-42", "user_id": "42", "game_id": "deadlock",
                "game_name": "Deadlock", "started_at": "2020-01-01T00:00:00Z"}]
        })))
        .expect(2)
        .mount(&server)
        .await;
    server
}

async fn prepare_timeout_identity(pool: &PgPool) {
    sqlx::raw_sql(
        "CREATE TABLE twitch_streamer_identities (twitch_user_id TEXT PRIMARY KEY, twitch_login TEXT); \
         INSERT INTO twitch_streamer_identities VALUES ('42', 'renamed');",
    )
    .execute(pool)
    .await
    .unwrap();
}

fn patch_chat_chain(
    pool: PgPool,
    chat_helix: HelixClient,
    allowed: bool,
    guard: Arc<TimeoutGuard>,
) -> Arc<dyn ChatApi> {
    let tracking = Arc::new(TimeoutTrackingChatApi::new(
        Arc::new(MockEndpointChat { helix: chat_helix }),
        guard,
        pool,
    ));
    Arc::new(ChannelPolicyChatApi::new(
        tracking,
        PolicyContext::Standard(Arc::new(PatchTestRoster { allowed })),
    ))
}

#[tokio::test]
async fn channel_policy_rejection_is_stored_as_skipped_without_chat_post() {
    let db = database().await;
    prepare_timeout_identity(&db.pool).await;
    let read_server = patch_read_server().await;
    let chat_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth2/token"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&chat_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/helix/chat/messages"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&chat_server)
        .await;
    let mut read_config = HelixConfig::new("cid", "sec");
    read_config.helix_base = format!("{}/helix", read_server.uri());
    read_config.token_url = format!("{}/oauth2/token", read_server.uri());
    let mut chat_config = HelixConfig::new("cid", "sec");
    chat_config.helix_base = format!("{}/helix", chat_server.uri());
    chat_config.token_url = format!("{}/oauth2/token", chat_server.uri());
    let event = patch_event(286);
    approve_observation(&db.pool, &event).await;
    let receiver = PatchReceiver::new(
        db.pool.clone(),
        HelixClient::new(read_config).unwrap(),
        patch_chat_chain(
            db.pool.clone(),
            HelixClient::new(chat_config).unwrap(),
            false,
            Arc::new(TimeoutGuard::new()),
        ),
        Arc::new(tb_chat::promos::NoopSuppressionCheck),
    );

    assert!(matches!(
        receiver.process(&event).await.unwrap(),
        PatchProcessOutcome::Processed(_)
    ));
    let (status, reason, drop_code): (String, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT status, uncertainty_reason, drop_code FROM twitch_patch_announcement_deliveries \
         WHERE broadcaster_id='42'",
    )
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(status, "skipped");
    assert_eq!(reason.as_deref(), Some("channel_policy_denied"));
    assert_eq!(drop_code, None);
    read_server.verify().await;
    chat_server.verify().await;
}

#[tokio::test]
async fn final_timeout_mute_is_stored_as_skipped_without_chat_post() {
    let db = database().await;
    prepare_timeout_identity(&db.pool).await;
    let read_server = patch_read_server().await;
    let chat_server = MockServer::start().await;
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    Mock::given(method("POST"))
        .and(path("/oauth2/token"))
        .respond_with(PausedTokenRefresh {
            entered: entered_tx,
            release: Mutex::new(release_rx),
        })
        .expect(1)
        .mount(&chat_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/helix/chat/messages"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&chat_server)
        .await;
    let mut read_config = HelixConfig::new("cid", "sec");
    read_config.helix_base = format!("{}/helix", read_server.uri());
    read_config.token_url = format!("{}/oauth2/token", read_server.uri());
    let mut chat_config = HelixConfig::new("cid", "sec");
    chat_config.helix_base = format!("{}/helix", chat_server.uri());
    chat_config.token_url = format!("{}/oauth2/token", chat_server.uri());
    let event = patch_event(286);
    approve_observation(&db.pool, &event).await;
    let guard = Arc::new(TimeoutGuard::new());
    let receiver = PatchReceiver::new(
        db.pool.clone(),
        HelixClient::new(read_config).unwrap(),
        patch_chat_chain(
            db.pool.clone(),
            HelixClient::new(chat_config).unwrap(),
            true,
            Arc::clone(&guard),
        ),
        Arc::new(tb_chat::promos::NoopSuppressionCheck),
    );
    let processing = tokio::spawn(async move { receiver.process(&event).await });
    tokio::task::spawn_blocking(move || entered_rx.recv().unwrap())
        .await
        .unwrap();
    guard.record_timeout("renamed");
    guard.record_timeout("renamed");
    assert!(guard.is_muted("renamed"));
    release_tx.send(()).unwrap();

    assert!(matches!(
        processing.await.unwrap().unwrap(),
        PatchProcessOutcome::Processed(_)
    ));
    let (status, reason, drop_code): (String, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT status, uncertainty_reason, drop_code FROM twitch_patch_announcement_deliveries \
         WHERE broadcaster_id='42'",
    )
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(status, "skipped");
    assert_eq!(reason.as_deref(), Some("source_only_chat_muted"));
    assert_eq!(drop_code, None);
    read_server.verify().await;
    chat_server.verify().await;
}

struct PausedTokenRefresh {
    entered: mpsc::Sender<()>,
    release: Mutex<mpsc::Receiver<()>>,
}

impl Respond for PausedTokenRefresh {
    fn respond(&self, _: &Request) -> ResponseTemplate {
        self.entered.send(()).unwrap();
        self.release.lock().unwrap().recv().unwrap();
        ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "app-token", "expires_in": 3600
        }))
    }
}

#[tokio::test]
async fn token_refresh_crossing_deadline_skips_without_chat_post_or_retry() {
    let db = database().await;
    let read_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth2/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "read-token", "expires_in": 3600
        })))
        .expect(1)
        .mount(&read_server)
        .await;
    Mock::given(method("GET"))
        .and(path("/helix/streams"))
        .and(query_param("user_id", "42"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [{"id": "session-42", "user_id": "42", "game_id": "deadlock",
                "game_name": "Deadlock", "started_at": "2020-01-01T00:00:00Z"}]
        })))
        .expect(2)
        .mount(&read_server)
        .await;
    let chat_server = MockServer::start().await;
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    Mock::given(method("POST"))
        .and(path("/oauth2/token"))
        .respond_with(PausedTokenRefresh {
            entered: entered_tx,
            release: Mutex::new(release_rx),
        })
        .expect(1)
        .mount(&chat_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/helix/chat/messages"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&chat_server)
        .await;
    let mut read_config = HelixConfig::new("cid", "sec");
    read_config.helix_base = format!("{}/helix", read_server.uri());
    read_config.token_url = format!("{}/oauth2/token", read_server.uri());
    let mut chat_config = HelixConfig::new("cid", "sec");
    chat_config.helix_base = format!("{}/helix", chat_server.uri());
    chat_config.token_url = format!("{}/oauth2/token", chat_server.uri());
    let event = patch_event(286);
    let detected_at = event.detected_at;
    approve_observation(&db.pool, &event).await;
    let expired = Arc::new(AtomicBool::new(false));
    let check_expired = Arc::clone(&expired);
    let receiver = PatchReceiver::new(
        db.pool.clone(),
        HelixClient::new(read_config).unwrap(),
        Arc::new(MockEndpointChat {
            helix: HelixClient::new(chat_config).unwrap(),
        }),
        Arc::new(tb_chat::promos::NoopSuppressionCheck),
    )
    .with_deadline_clock(Arc::new(move || {
        detected_at
            + chrono::Duration::seconds(if check_expired.load(Ordering::SeqCst) {
                EVENT_TTL_SECONDS + 1
            } else {
                1
            })
    }));
    let processing = tokio::spawn(async move { receiver.process(&event).await });
    tokio::task::spawn_blocking(move || entered_rx.recv().unwrap())
        .await
        .unwrap();
    let (before_release, attempted): (String, bool) = sqlx::query_as(
        "SELECT status, attempted_at IS NOT NULL FROM twitch_patch_announcement_deliveries \
         WHERE broadcaster_id='42'",
    )
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!((before_release.as_str(), attempted), ("attempted", true));
    expired.store(true, Ordering::SeqCst);
    release_tx.send(()).unwrap();
    assert!(matches!(
        processing.await.unwrap().unwrap(),
        PatchProcessOutcome::Processed(_)
    ));
    let (status, reason, drop_code): (String, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT status, uncertainty_reason, drop_code FROM twitch_patch_announcement_deliveries \
         WHERE broadcaster_id='42'",
    )
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(status, "skipped");
    assert_eq!(reason.as_deref(), Some("event_expired_before_post"));
    assert_eq!(drop_code, None);
    read_server.verify().await;
    chat_server.verify().await;
}

async fn run_runtime_role_matrix(pool: &PgPool) {
    let socket_dir: String = sqlx::query_scalar("SHOW unix_socket_directories")
        .fetch_one(pool)
        .await
        .unwrap();
    let script = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../ops/systemd/twitch-runtime-roles.sql"
    );
    let output = Command::new("/usr/lib/postgresql/16/bin/psql")
        .args([
            "-h",
            &socket_dir,
            "-U",
            "uplink_test",
            "-d",
            "twitch_analytics",
            "-v",
            "ON_ERROR_STOP=1",
            "-f",
            script,
        ])
        .output()
        .expect("psql must be available for runtime-role integration test");
    assert!(
        output.status.success(),
        "runtime role matrix failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

async fn insert_pending_as_bot(pool: &PgPool, patch_id: i64, observed_at: DateTime<Utc>) -> u64 {
    let mut conn = pool.acquire().await.unwrap();
    sqlx::query("SET ROLE twitchbot")
        .execute(&mut *conn)
        .await
        .unwrap();
    let inserted = sqlx::query(
        "INSERT INTO twitch_patch_feed_observations (patch_id, observed_at, status) \
         VALUES ($1, $2, 'pending') ON CONFLICT (patch_id) DO NOTHING",
    )
    .bind(patch_id)
    .bind(observed_at)
    .execute(&mut *conn)
    .await
    .unwrap()
    .rows_affected();
    sqlx::query("RESET ROLE").execute(&mut *conn).await.unwrap();
    inserted
}

#[tokio::test]
async fn feed_schema_tracks_each_patch_without_numeric_cursor_under_runtime_roles() {
    let db = TestPostgres::start().await;
    sqlx::query("CREATE DATABASE twitch_analytics")
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::raw_sql("CREATE ROLE postgres SUPERUSER NOLOGIN")
        .execute(&db.pool)
        .await
        .unwrap();
    let socket_dir: String = sqlx::query_scalar("SHOW unix_socket_directories")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(3)
        .connect_with(
            sqlx::postgres::PgConnectOptions::new()
                .host(&socket_dir)
                .username("uplink_test")
                .database("twitch_analytics"),
        )
        .await
        .unwrap();
    sqlx::raw_sql(
        "CREATE TABLE twitch_streamers_partner_state (
             twitch_user_id TEXT, twitch_login TEXT, is_partner_active INT,
             manual_partner_opt_out INT DEFAULT 0);
         CREATE TABLE twitch_raid_auth (twitch_user_id TEXT, scopes TEXT, needs_reauth BOOLEAN);
         CREATE TABLE twitch_live_state (
             twitch_user_id TEXT, last_stream_id TEXT, last_seen_at TEXT, is_live INT, last_game TEXT);
         INSERT INTO twitch_streamers_partner_state VALUES ('42','early',1,0), ('43','late',1,0);
         INSERT INTO twitch_raid_auth VALUES
             ('42','channel:bot',FALSE), ('43','channel:bot',FALSE);
         INSERT INTO twitch_live_state VALUES
             ('42','s42',now()::text,1,'Deadlock'), ('43','s43',now()::text,1,'Other');",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::raw_sql(include_str!(
        "../../../../migrations/20260928120000_patch_announcements.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    run_runtime_role_matrix(&pool).await;

    let privileges: Vec<bool> = sqlx::query_scalar(
        "SELECT has_column_privilege('twitchbot', 'twitch_patch_feed_state', 'singleton', 'UPDATE') \
         UNION ALL SELECT has_column_privilege('twitchbot', 'twitch_patch_feed_state', 'last_successful_index_at', 'UPDATE') \
         UNION ALL SELECT NOT has_column_privilege('twitchbot', 'twitch_patch_feed_observations', 'observed_at', 'UPDATE') \
         UNION ALL SELECT has_column_privilege('twitchbot', 'twitch_patch_feed_observations', 'status', 'UPDATE') \
         UNION ALL SELECT has_table_privilege('twitchbot', 'twitch_patch_announcement_recipients', 'INSERT') \
         UNION ALL SELECT NOT has_table_privilege('twitchbot', 'twitch_patch_announcement_recipients', 'UPDATE') \
         UNION ALL SELECT has_table_privilege('twitchdash', 'twitch_patch_announcement_recipients', 'SELECT') \
         UNION ALL SELECT NOT has_table_privilege('twitchdash', 'twitch_patch_announcement_recipients', 'INSERT') \
         UNION ALL SELECT NOT has_table_privilege('twitchlegacy', 'twitch_patch_announcement_recipients', 'SELECT')",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(privileges, vec![true; 9]);

    let mut conn = pool.acquire().await.unwrap();
    sqlx::query("SET ROLE twitchbot")
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO twitch_patch_feed_state (singleton, last_successful_index_at) \
         VALUES (TRUE, now())",
    )
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query("SELECT singleton FROM twitch_patch_feed_state WHERE singleton FOR UPDATE")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert!(
        sqlx::query("INSERT INTO twitch_patch_feed_state (singleton) VALUES (FALSE)")
            .execute(&mut *conn)
            .await
            .is_err()
    );
    sqlx::query(
        "INSERT INTO twitch_patch_feed_observations (patch_id, observed_at, status) \
         VALUES (285, now(), 'historical'), (289, now(), 'expired_timeout'), \
         (290, now(), 'pending')",
    )
    .execute(&mut *conn)
    .await
    .unwrap();
    assert!(sqlx::query(
        "INSERT INTO twitch_patch_feed_observations (patch_id, observed_at, status) \
         VALUES (291, now(), 'invalid')",
    )
    .execute(&mut *conn)
    .await
    .is_err());
    let order: Vec<i64> = sqlx::query_scalar(
        "SELECT patch_id FROM twitch_patch_feed_observations ORDER BY observed_at, patch_id",
    )
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    assert_eq!(order, vec![285, 289, 290]);
    let snapshot: Vec<(String, String)> = sqlx::query_as(
        "SELECT broadcaster_id, stream_id FROM twitch_patch_announcement_recipients \
         WHERE patch_id=290",
    )
    .fetch_all(&mut *conn)
    .await
    .unwrap();
    assert_eq!(snapshot, vec![("42".into(), "s42".into())]);
    let historical: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM twitch_patch_announcement_recipients WHERE patch_id=285",
    )
    .fetch_one(&mut *conn)
    .await
    .unwrap();
    assert_eq!(historical, 0);
    assert!(sqlx::query(
        "UPDATE twitch_patch_feed_observations SET observed_at=now() WHERE patch_id=290",
    )
    .execute(&mut *conn)
    .await
    .is_err());
    sqlx::query("RESET ROLE").execute(&mut *conn).await.unwrap();
    drop(conn);
    let first_observed_at = DateTime::from_timestamp_micros(Utc::now().timestamp_micros()).unwrap();
    let second_observed_at = first_observed_at + chrono::Duration::seconds(1);
    let (first_poll, second_poll) = tokio::join!(
        insert_pending_as_bot(&pool, 291, first_observed_at),
        insert_pending_as_bot(&pool, 291, second_observed_at),
    );
    assert_eq!(first_poll + second_poll, 1);
    let saved_observed_at: DateTime<Utc> = sqlx::query_scalar(
        "SELECT observed_at FROM twitch_patch_feed_observations WHERE patch_id=291",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(saved_observed_at == first_observed_at || saved_observed_at == second_observed_at);
    let later_poll_at = saved_observed_at + chrono::Duration::seconds(30);
    assert_eq!(insert_pending_as_bot(&pool, 291, later_poll_at).await, 0);
    let after_repeat_at: DateTime<Utc> = sqlx::query_scalar(
        "SELECT observed_at FROM twitch_patch_feed_observations WHERE patch_id=291",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(after_repeat_at, saved_observed_at);
    let recipients_291: Vec<String> = sqlx::query_scalar(
        "SELECT broadcaster_id FROM twitch_patch_announcement_recipients \
         WHERE patch_id=291",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(recipients_291, vec!["42"]);
    sqlx::raw_sql(
        "UPDATE twitch_streamers_partner_state SET is_partner_active=1 WHERE twitch_user_id='43'; \
         UPDATE twitch_live_state SET last_game='Deadlock' WHERE twitch_user_id='43'; \
         UPDATE twitch_live_state SET last_game='Other' WHERE twitch_user_id='42';",
    )
    .execute(&pool)
    .await
    .unwrap();
    let recipients: Vec<String> = sqlx::query_scalar(
        "SELECT broadcaster_id FROM twitch_patch_announcement_recipients \
         WHERE patch_id=290 ORDER BY broadcaster_id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(recipients, vec!["42"]);
}
