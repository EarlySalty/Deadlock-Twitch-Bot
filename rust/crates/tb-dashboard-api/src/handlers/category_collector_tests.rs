use super::*;
use crate::test_postgres::TestPostgres;
use axum::body::to_bytes;
use chrono::{DateTime, Duration, Timelike, Utc};
use serde_json::Value;

// Nur die vom API gelesenen Spalten. Isolierter PostgreSQL, keine Live-DB.
const SCHEMA: &str = r#"
CREATE TABLE category_collector_config (
 id integer PRIMARY KEY, enabled boolean NOT NULL DEFAULT true,
 discovery_interval_seconds integer NOT NULL DEFAULT 60,
 retention_days integer NOT NULL DEFAULT 90
);
INSERT INTO category_collector_config(id) VALUES(1);
CREATE TABLE category_collector_status (
 id integer PRIMARY KEY, last_error_at timestamptz,
 chat_privmsgs_dropped bigint NOT NULL DEFAULT 0,
 chat_queue_dropped bigint NOT NULL DEFAULT 0,
 chat_commands_dropped bigint NOT NULL DEFAULT 0,
 updated_at timestamptz NOT NULL DEFAULT now()
);
INSERT INTO category_collector_status(id) VALUES(1);
CREATE TABLE category_polls (
 poll_id bigint PRIMARY KEY, started_at timestamptz NOT NULL,
 completed_at timestamptz, status text NOT NULL,
 stream_count integer NOT NULL DEFAULT 0, viewer_total bigint NOT NULL DEFAULT 0
);
CREATE TABLE category_channels (
 user_id text PRIMARY KEY, login text NOT NULL, display_name text
);
CREATE TABLE category_stream_snapshots (
 poll_id bigint NOT NULL, snapshot_at timestamptz NOT NULL,
 stream_id text NOT NULL, user_id text NOT NULL,
 user_login text NOT NULL, language text, viewer_count integer NOT NULL
);
CREATE TABLE category_chat_messages (
 room_user_id text NOT NULL, message_id text NOT NULL,
 source_message_id text, sent_at timestamptz NOT NULL,
 chatter_user_id text NOT NULL, chatter_login text NOT NULL,
 message_text text NOT NULL, detected_lang text
);
CREATE TABLE category_chat_rollup (
 hour_bucket timestamptz NOT NULL, room_user_id text NOT NULL,
 lang text NOT NULL, messages bigint NOT NULL, distinct_chatters bigint NOT NULL DEFAULT 1
);
"#;

#[tokio::test]
async fn handler_future_is_send() {
    fn assert_send<T: Send>(_: T) {}
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgresql://unused@localhost/unused")
        .unwrap();
    assert_send(get_handler(
        DashboardAuthLevel::None,
        State(pool),
        Query(CollectorQuery::default()),
        None,
    ));
}

async fn setup() -> TestPostgres {
    let database = TestPostgres::start().await;
    sqlx::raw_sql(SCHEMA).execute(&database.pool).await.unwrap();
    database
}

async fn response(
    pool: &PgPool,
    auth: DashboardAuthLevel,
    days: Option<&str>,
) -> (StatusCode, Value) {
    let reply = get_handler(
        auth,
        State(pool.clone()),
        Query(CollectorQuery {
            days: days.map(str::to_owned),
        }),
        None,
    )
    .await;
    let status = reply.status();
    let body = to_bytes(reply.into_body(), 2_000_000).await.unwrap();
    (status, serde_json::from_slice(&body).unwrap())
}

fn hour() -> DateTime<Utc> {
    Utc::now()
        .with_minute(0)
        .unwrap()
        .with_second(0)
        .unwrap()
        .with_nanosecond(0)
        .unwrap()
}

async fn poll(pool: &PgPool, id: i64, at: DateTime<Utc>, status: &str, viewers: i64, stream: bool) {
    sqlx::query("INSERT INTO category_polls VALUES($1,$2,$2,$3,$4,$5)")
        .bind(id)
        .bind(at)
        .bind(status)
        .bind(i32::from(stream))
        .bind(viewers)
        .execute(pool)
        .await
        .unwrap();
    if stream {
        sqlx::query("INSERT INTO category_stream_snapshots VALUES($1,$2,'stream-one','123','test_channel','de',$3)")
            .bind(id).bind(at).bind(viewers as i32).execute(pool).await.unwrap();
    }
}

#[tokio::test]
async fn rejects_anonymous_and_partner_before_database_access() {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .acquire_timeout(std::time::Duration::from_millis(10))
        .connect_lazy("postgresql://unused@127.0.0.1:1/unused")
        .unwrap();
    assert_eq!(
        response(&pool, DashboardAuthLevel::None, None).await.0,
        StatusCode::UNAUTHORIZED
    );
    let partner = DashboardAuthLevel::Partner {
        twitch_login: "partner".into(),
        twitch_user_id: "123".into(),
        display_name: "Partner".into(),
    };
    assert_eq!(
        response(&pool, partner, None).await.0,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn days_are_exactly_7_30_or_90() {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .acquire_timeout(std::time::Duration::from_millis(10))
        .connect_lazy("postgresql://unused@127.0.0.1:1/unused")
        .unwrap();
    for days in ["0", "-7", "8", "365", "30junk", "7.0", " 7", ""] {
        assert_eq!(
            response(&pool, DashboardAuthLevel::admin(), Some(days))
                .await
                .0,
            StatusCode::BAD_REQUEST,
            "days={days:?}"
        );
    }
}

#[tokio::test]
async fn missing_schema_is_503_not_zero_activity() {
    let database = TestPostgres::start().await;
    let (status, body) = response(&database.pool, DashboardAuthLevel::admin(), None).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(body.get("error").is_some());
    assert!(body.get("stream_languages").is_none());
}

#[tokio::test]
async fn seeded_empty_collector_is_an_honest_empty_state() {
    let database = setup().await;
    let (status, body) = response(&database.pool, DashboardAuthLevel::admin(), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["meta"]["collector_status"], "not_started");
    assert_eq!(body["meta"]["measurement_seconds"], 0.0);
    assert!(body["meta"]["first_seen_at"].is_null());
    assert!(body["meta"]["last_completed_poll_at"].is_null());
    for list in [
        "stream_languages",
        "message_languages",
        "top_channels",
        "trend",
        "chat_heatmap",
    ] {
        assert_eq!(body[list], json!([]), "{list}");
    }
}

#[tokio::test]
async fn viewer_average_is_duration_weighted_and_gaps_are_not_broadcast_hours() {
    let database = setup().await;
    let base = hour() - Duration::hours(2);
    poll(&database.pool, 1, base, "complete", 10, true).await;
    poll(
        &database.pool,
        2,
        base + Duration::seconds(60),
        "complete",
        30,
        true,
    )
    .await;
    poll(
        &database.pool,
        3,
        base + Duration::seconds(180),
        "complete",
        50,
        true,
    )
    .await;
    poll(
        &database.pool,
        4,
        base + Duration::seconds(181),
        "failed",
        999,
        true,
    )
    .await;
    poll(
        &database.pool,
        5,
        base + Duration::seconds(500),
        "complete",
        900,
        true,
    )
    .await;
    let (status, body) = response(&database.pool, DashboardAuthLevel::admin(), Some("7")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["meta"]["measurement_seconds"], 180.0);
    assert_eq!(body["meta"]["complete_polls"], 4);
    assert_eq!(body["meta"]["incomplete_polls"], 1);
    let lang = &body["stream_languages"][0];
    assert_eq!(lang["language"], "de");
    assert_eq!(lang["unique_streams"], 1);
    assert_eq!(lang["unique_channels"], 1);
    assert!((lang["broadcast_hours"].as_f64().unwrap() - 0.05).abs() < 1e-9);
    assert!((lang["avg_viewers"].as_f64().unwrap() - 23.333333333333).abs() < 1e-8);
    assert!((lang["viewer_hours"].as_f64().unwrap() - 1.166666666667).abs() < 1e-8);
    assert_eq!(body["top_channels"][0]["avg_viewers"], lang["avg_viewers"]);
}

#[tokio::test]
async fn single_snapshot_has_unknown_time_weighted_average() {
    let database = setup().await;
    poll(
        &database.pool,
        1,
        hour() - Duration::hours(1),
        "complete",
        42,
        true,
    )
    .await;
    let (status, body) = response(&database.pool, DashboardAuthLevel::admin(), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["stream_languages"][0]["broadcast_hours"], 0.0);
    assert!(body["stream_languages"][0]["avg_viewers"].is_null());
    assert!(body["top_channels"][0]["avg_viewers"].is_null());
    assert_eq!(body["trend"][0]["avg_viewers"], 42.0);
}

#[tokio::test]
async fn confirmed_empty_poll_is_zero_but_failed_poll_is_not_a_trend_sample() {
    let database = setup().await;
    poll(
        &database.pool,
        1,
        hour() - Duration::hours(2),
        "complete",
        0,
        false,
    )
    .await;
    poll(
        &database.pool,
        2,
        hour() - Duration::hours(1),
        "failed",
        0,
        false,
    )
    .await;
    let (status, body) = response(&database.pool, DashboardAuthLevel::admin(), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["trend"].as_array().unwrap().len(), 1);
    assert_eq!(body["trend"][0]["avg_streams"], 0.0);
    assert_eq!(body["trend"][0]["poll_samples"], 1);
}

#[tokio::test]
async fn message_language_is_separate_and_raw_text_and_chatter_ids_never_leave_api() {
    let database = setup().await;
    let base = hour();
    poll(
        &database.pool,
        1,
        base - Duration::minutes(2),
        "complete",
        10,
        true,
    )
    .await;
    sqlx::query("INSERT INTO category_chat_rollup VALUES($1,'123','en',5,4),($1,'123','de',2,2)")
        .bind(base - Duration::hours(1))
        .execute(&database.pool)
        .await
        .unwrap();
    // Ein bereits berechneter aktueller Rollup darf aktuelle Rohzeilen nicht doppelt zählen.
    sqlx::query("INSERT INTO category_chat_rollup VALUES($1,'123','fr',999,1)")
        .bind(base)
        .execute(&database.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO category_chat_messages VALUES('123','original',NULL,$1,'private-id','private-login','PRIVATE_RAW_TEXT','en'),('123','forwarded','source-elsewhere',$1,'private-id','private-login','PRIVATE_RAW_TEXT','fr')")
        .bind(Utc::now() - Duration::milliseconds(1)).execute(&database.pool).await.unwrap();
    let (status, body) = response(&database.pool, DashboardAuthLevel::admin(), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["stream_languages"][0]["language"], "de");
    let messages = body["message_languages"].as_array().unwrap();
    assert!(messages
        .iter()
        .any(|row| row["language"] == "en" && row["messages"] == 6));
    assert!(!messages.iter().any(|row| row["language"] == "fr"));
    let text = body.to_string();
    for private in [
        "PRIVATE_RAW_TEXT",
        "private-id",
        "private-login",
        "distinct_chatters",
        "message_text",
    ] {
        assert!(!text.contains(private), "Leck: {private}");
    }
}

#[tokio::test]
async fn router_is_wired_admin_only_and_cache_never_bypasses_auth() {
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;
    let database = setup().await;
    let router = crate::build_authed_router(
        database.pool.clone(),
        "test-only-internal-token".into(),
        crate::RateLimiter::new(database.pool.clone(), String::new()),
    );
    for token in [
        None,
        Some("wrong-token"),
        Some("test-only-internal-token"),
        None,
    ] {
        let mut request = Request::builder().uri("/twitch/api/v2/admin/category-collector?days=7");
        if let Some(token) = token {
            request = request.header("X-Internal-Token", token);
        }
        let reply = router
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        if token == Some("test-only-internal-token") {
            assert_eq!(reply.status(), StatusCode::OK);
            assert_eq!(reply.headers()["cache-control"], "private, no-store");
        } else {
            assert_eq!(reply.status(), StatusCode::UNAUTHORIZED);
        }
    }
}

#[tokio::test]
async fn short_cache_is_shared_but_errors_are_not_cached() {
    let database = TestPostgres::start().await;
    let cache = CategoryCollectorCache::default();
    let call = || {
        get_handler(
            DashboardAuthLevel::admin(),
            State(database.pool.clone()),
            Query(CollectorQuery::default()),
            Some(Extension(cache.clone())),
        )
    };
    assert_eq!(call().await.status(), StatusCode::SERVICE_UNAVAILABLE);
    sqlx::raw_sql(SCHEMA).execute(&database.pool).await.unwrap();
    assert_eq!(call().await.status(), StatusCode::OK);
    sqlx::query("UPDATE category_collector_config SET enabled=false")
        .execute(&database.pool)
        .await
        .unwrap();
    let reply = call().await;
    let body: Value =
        serde_json::from_slice(&to_bytes(reply.into_body(), 2_000_000).await.unwrap()).unwrap();
    assert_eq!(body["meta"]["collector_status"], "not_started");
    cache.0.lock().await.clear();
    let reply = call().await;
    let body: Value =
        serde_json::from_slice(&to_bytes(reply.into_body(), 2_000_000).await.unwrap()).unwrap();
    assert_eq!(body["meta"]["collector_status"], "disabled");
}

#[tokio::test]
async fn permanent_retention_is_reported_as_null_without_fabricated_default() {
    let database = setup().await;
    sqlx::raw_sql("ALTER TABLE category_collector_config ALTER COLUMN retention_days DROP NOT NULL; UPDATE category_collector_config SET retention_days=NULL")
        .execute(&database.pool).await.unwrap();
    let (status, body) = response(&database.pool, DashboardAuthLevel::admin(), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["meta"]["retention_days"].is_null());
}

#[tokio::test]
async fn all_allowed_periods_and_disabled_status_are_reported() {
    let database = setup().await;
    sqlx::query("UPDATE category_collector_config SET enabled=false, retention_days=30")
        .execute(&database.pool)
        .await
        .unwrap();
    for days in ["7", "30", "90"] {
        let (status, body) =
            response(&database.pool, DashboardAuthLevel::admin(), Some(days)).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["meta"]["days"], days.parse::<i64>().unwrap());
        assert_eq!(body["meta"]["retention_days"], 30);
        assert_eq!(body["meta"]["collector_status"], "disabled");
        assert!(body["meta"]["storage_bytes"].as_i64().unwrap() > 0);
    }
}
