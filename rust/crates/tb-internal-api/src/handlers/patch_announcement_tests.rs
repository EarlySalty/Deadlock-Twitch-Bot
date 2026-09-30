use super::*;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};

struct FakeTransport {
    streams: Mutex<Vec<HelixStream>>,
    sent: Mutex<Vec<(String, String)>>,
    fail: AtomicBool,
    leave_after_snapshot: AtomicBool,
    outcome: &'static str,
}

impl FakeTransport {
    fn new() -> Self {
        Self {
            streams: Mutex::new(vec![stream("42")]),
            sent: Mutex::new(vec![]),
            fail: AtomicBool::new(false),
            leave_after_snapshot: AtomicBool::new(false),
            outcome: "sent",
        }
    }
}

#[async_trait::async_trait]
impl Transport for FakeTransport {
    async fn streams(&self, ids: &[String]) -> Result<Vec<HelixStream>, ()> {
        if self.fail.load(Ordering::SeqCst) {
            return Err(());
        }
        let mut all = self.streams.lock().unwrap();
        let result = all
            .iter()
            .filter(|s| ids.contains(&s.user_id))
            .cloned()
            .collect();
        if self.leave_after_snapshot.swap(false, Ordering::SeqCst) {
            all.clear();
        }
        Ok(result)
    }
    async fn send(&self, id: &str, message: &str) -> &'static str {
        self.sent.lock().unwrap().push((id.into(), message.into()));
        self.outcome
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

fn event() -> PatchEvent {
    // The wire producer (Python) and Postgres both use microsecond precision.
    let now = Utc::now();
    let detected_at = DateTime::from_timestamp_micros(now.timestamp_micros()).unwrap();
    let discord_url = "https://discord.com/channels/123/456".to_string();
    PatchEvent {
        event_id: "a".repeat(64),
        source_url: "https://forums.playdeadlock.com/posts/123/".into(),
        detected_at,
        discord_url: discord_url.clone(),
        message: format!("🔥 Neuer Deadlock-Patch ist da! Auf Deutsch im Discord: {discord_url}"),
    }
}

async fn database() -> (PgPool, PgPool, String) {
    let dsn = std::env::var("TB_TEST_DATABASE_URL").expect("dedicated disposable DB required");
    assert!(
        dsn.starts_with("postgres://postgres:patch-test-only@127.0.0.1:"),
        "refuse non-test DB"
    );
    let admin = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&dsn)
        .await
        .unwrap();
    let schema = format!("patch_test_{}", uuid::Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin)
        .await
        .unwrap();
    let options: sqlx::postgres::PgConnectOptions = dsn.parse().unwrap();
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(6)
        .connect_with(options.options([("search_path", schema.as_str())]))
        .await
        .unwrap();
    sqlx::raw_sql(include_str!(
        "../../../../migrations/20260920143000_patch_announcements.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    sqlx::raw_sql("CREATE TABLE twitch_streamers_partner_state (twitch_user_id TEXT, twitch_login TEXT, is_partner_active INT, manual_partner_opt_out INT DEFAULT 0);
        CREATE TABLE twitch_live_state (twitch_user_id TEXT, last_stream_id TEXT, last_seen_at TEXT, is_live INT, last_game TEXT);
        INSERT INTO twitch_streamers_partner_state VALUES ('42','renamed',1,0), ('43','later',1,0), ('99','notpartner',0,0);")
        .execute(&pool).await.unwrap();
    for (id, game) in [("42", "Deadlock"), ("43", "Other"), ("99", "Deadlock")] {
        sqlx::query("INSERT INTO twitch_live_state VALUES ($1,$2,$3,1,$4)")
            .bind(id)
            .bind(format!("session-{id}"))
            .bind(Utc::now().to_rfc3339())
            .bind(game)
            .execute(&pool)
            .await
            .unwrap();
    }
    (pool, admin, schema)
}

async fn cleanup(pool: PgPool, admin: PgPool, schema: String) {
    pool.close().await;
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}

#[tokio::test]
#[ignore = "requires dedicated disposable PostgreSQL"]
async fn concurrent_retries_and_source_duplicates_send_exactly_once() {
    let (pool, admin, schema) = database().await;
    let transport = FakeTransport::new();
    let e = event();
    let (a, b) = tokio::join!(
        process(&pool, &transport, &e),
        process(&pool, &transport, &e)
    );
    a.unwrap();
    b.unwrap();
    let mut duplicate = e.clone();
    duplicate.event_id = "b".repeat(64);
    process(&pool, &transport, &duplicate).await.unwrap();
    assert_eq!(transport.sent.lock().unwrap().len(), 1);
    let status: String =
        sqlx::query_scalar("SELECT status FROM twitch_patch_announcement_deliveries")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status, "sent");
    cleanup(pool, admin, schema).await;
}

#[tokio::test]
#[ignore = "requires dedicated disposable PostgreSQL"]
async fn uncertain_send_restart_and_later_join_never_replay() {
    let (pool, admin, schema) = database().await;
    let mut transport = FakeTransport::new();
    transport.outcome = "uncertain";
    let e = event();
    process(&pool, &transport, &e).await.unwrap();
    sqlx::query("UPDATE twitch_live_state SET last_game='Deadlock' WHERE twitch_user_id='43'")
        .execute(&pool)
        .await
        .unwrap();
    transport.streams.lock().unwrap().push(stream("43"));
    // New invocation with no in-memory delivery state, like a process restart.
    process(&pool, &transport, &e).await.unwrap();
    assert_eq!(transport.sent.lock().unwrap().len(), 1);
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM twitch_patch_announcement_deliveries")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 1);
    let mut changed = e.clone();
    changed.message = format!("Changed {}", changed.discord_url);
    assert!(process(&pool, &transport, &changed).await.is_err());
    cleanup(pool, admin, schema).await;
}

#[tokio::test]
#[ignore = "requires dedicated disposable PostgreSQL"]
async fn helix_error_rolls_back_snapshot_then_retry_can_succeed() {
    let (pool, admin, schema) = database().await;
    let transport = FakeTransport::new();
    transport.fail.store(true, Ordering::SeqCst);
    let e = event();
    assert!(process(&pool, &transport, &e).await.is_err());
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM twitch_patch_announcements")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    assert!(transport.sent.lock().unwrap().is_empty());
    transport.fail.store(false, Ordering::SeqCst);
    process(&pool, &transport, &e).await.unwrap();
    assert_eq!(transport.sent.lock().unwrap().len(), 1);
    cleanup(pool, admin, schema).await;
}

#[tokio::test]
#[ignore = "requires dedicated disposable PostgreSQL"]
async fn stream_ending_before_send_and_stale_snapshot_are_skipped() {
    let (pool, admin, schema) = database().await;
    let transport = FakeTransport::new();
    transport.leave_after_snapshot.store(true, Ordering::SeqCst);
    let e = event();
    process(&pool, &transport, &e).await.unwrap();
    assert!(transport.sent.lock().unwrap().is_empty());
    let status: String =
        sqlx::query_scalar("SELECT status FROM twitch_patch_announcement_deliveries")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(status, "skipped");
    let mut next = e;
    next.event_id = "c".repeat(64);
    next.source_url = "https://forums.playdeadlock.com/posts/456/".into();
    transport.streams.lock().unwrap().push(stream("42"));
    sqlx::query("UPDATE twitch_live_state SET last_seen_at='2000-01-01T00:00:00Z'")
        .execute(&pool)
        .await
        .unwrap();
    process(&pool, &transport, &next).await.unwrap();
    assert!(transport.sent.lock().unwrap().is_empty());
    cleanup(pool, admin, schema).await;
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
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://unused:unused@127.0.0.1:1/unused")
        .unwrap();
    let app = Router::new()
        .route("/patch", post(handler))
        .layer(middleware::from_fn_with_state("test-token".to_string(), internal_auth))
        .layer(middleware::from_fn(loopback_only))
        .layer(Extension(ExpectedToken("test-token".into())))
        .layer(Extension(Arc::new(None::<HelixClient>)))
        .layer(Extension(pool));
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
            .body(Body::from(serde_json::to_vec(&event()).unwrap()))
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
