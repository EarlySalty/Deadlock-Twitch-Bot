use super::*;
#[allow(clippy::duplicate_mod)]
#[path = "../../../../test-support/postgres.rs"]
mod postgres;
use postgres::TestPostgres;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Mutex,
};

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

    async fn can_send(&self, id: &str, stream_id: &str) -> Result<bool, PatchProcessError> {
        authorized_login(&self.pool, id, stream_id)
            .await
            .map(|login| login.is_some())
    }

    async fn send(
        &self,
        id: &str,
        _stream_id: &str,
        message: &str,
    ) -> Result<&'static str, PatchProcessError> {
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
        Ok(self.outcome)
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
    postgres
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
async fn stream_ending_before_send_and_stale_snapshot_are_skipped() {
    let db = database().await;
    let pool = db.pool.clone();
    let transport = FakeTransport::new(pool.clone());
    transport.leave_after_snapshot.store(true, Ordering::SeqCst);
    let event = patch_event(286);
    process(&pool, &transport, &event).await.unwrap();
    assert!(transport.sent.lock().unwrap().is_empty());
    let status: String =
        sqlx::query_scalar("SELECT status FROM twitch_patch_announcement_deliveries")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status, "skipped");
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
    let status: String =
        sqlx::query_scalar("SELECT status FROM twitch_patch_announcement_deliveries")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status, "skipped");
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
async fn rights_reauth_optout_and_expired_events_never_snapshot() {
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
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM twitch_patch_announcement_deliveries")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 0);
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
    assert!(matches!(
        process(&pool, &transport, &expired).await.unwrap(),
        PatchProcessOutcome::SkippedExpired
    ));
    assert!(transport.sent.lock().unwrap().is_empty());
}

#[tokio::test]
async fn feed_schema_tracks_each_patch_without_numeric_cursor_and_grants_bot() {
    let db = TestPostgres::start().await;
    sqlx::raw_sql("CREATE ROLE twitchbot NOLOGIN; CREATE ROLE twitchdash NOLOGIN")
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::raw_sql(include_str!(
        "../../../../migrations/20260928120000_patch_announcements.sql"
    ))
    .execute(&db.pool)
    .await
    .unwrap();
    let granted: Vec<bool> = sqlx::query_scalar(
        "SELECT has_table_privilege('twitchbot', 'twitch_patch_feed_state', 'INSERT') \
         UNION ALL SELECT has_column_privilege('twitchbot', 'twitch_patch_feed_observations', 'status', 'UPDATE') \
         UNION ALL SELECT NOT has_column_privilege('twitchbot', 'twitch_patch_feed_observations', 'observed_at', 'UPDATE') \
         UNION ALL SELECT has_table_privilege('twitchdash', 'twitch_patch_feed_observations', 'SELECT')",
    )
    .fetch_all(&db.pool)
    .await
    .unwrap();
    assert_eq!(granted, vec![true; 4]);
    sqlx::query("INSERT INTO twitch_patch_feed_state (singleton) VALUES (TRUE)")
        .execute(&db.pool)
        .await
        .unwrap();
    assert!(
        sqlx::query("INSERT INTO twitch_patch_feed_state (singleton) VALUES (FALSE)")
            .execute(&db.pool)
            .await
            .is_err()
    );
    for (id, status) in [
        (290_i64, "pending"),
        (285, "historical"),
        (289, "expired_timeout"),
    ] {
        sqlx::query(
            "INSERT INTO twitch_patch_feed_observations (patch_id, observed_at, status) \
             VALUES ($1, now(), $2)",
        )
        .bind(id)
        .bind(status)
        .execute(&db.pool)
        .await
        .unwrap();
    }
    assert!(sqlx::query(
        "INSERT INTO twitch_patch_feed_observations (patch_id, observed_at, status) \
         VALUES (291, now(), 'invalid')",
    )
    .execute(&db.pool)
    .await
    .is_err());
    let order: Vec<i64> = sqlx::query_scalar(
        "SELECT patch_id FROM twitch_patch_feed_observations ORDER BY observed_at, patch_id",
    )
    .fetch_all(&db.pool)
    .await
    .unwrap();
    assert_eq!(order.len(), 3);
}

#[tokio::test]
async fn endpoint_requires_auth_and_wiring_before_any_delivery() {
    use axum::{
        body::Body,
        extract::ConnectInfo,
        http::{Request, StatusCode},
        middleware,
        routing::post,
        Router,
    };
    use tb_http_core::{internal_auth, loopback_only, ExpectedToken};
    use tower::ServiceExt;
    let app = Router::new()
        .route("/patch", post(handler))
        .layer(middleware::from_fn_with_state(
            "test-token".to_string(),
            internal_auth,
        ))
        .layer(middleware::from_fn(loopback_only))
        .layer(Extension(ExpectedToken("test-token".into())));
    for (token, peer, expected) in [
        (None, "127.0.0.1:1234", StatusCode::UNAUTHORIZED),
        (Some("wrong"), "127.0.0.1:1234", StatusCode::UNAUTHORIZED),
        (Some("test-token"), "192.0.2.5:1234", StatusCode::FORBIDDEN),
        (
            Some("test-token"),
            "127.0.0.1:1234",
            StatusCode::SERVICE_UNAVAILABLE,
        ),
    ] {
        let mut builder = Request::builder()
            .uri("/patch")
            .method("POST")
            .header("Content-Type", "application/json");
        if let Some(token) = token {
            builder = builder.header("X-Internal-Token", token);
        }
        let mut request = builder
            .body(Body::from(serde_json::to_vec(&patch_event(286)).unwrap()))
            .unwrap();
        request
            .extensions_mut()
            .insert(ConnectInfo(peer.parse::<std::net::SocketAddr>().unwrap()));
        assert_eq!(
            app.clone().oneshot(request).await.unwrap().status(),
            expected
        );
    }
}
