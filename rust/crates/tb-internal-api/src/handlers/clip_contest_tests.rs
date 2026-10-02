use super::*;
use axum::{body::Body, extract::ConnectInfo, http::Request};
use serde_json::Value;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use std::{
    net::SocketAddr,
    str::FromStr,
    sync::atomic::{AtomicUsize, Ordering},
};
use tb_chat::clip_contest_submit::{
    BrokerClipRequest, BrokerClipResponse, BrokerClipStatus, ClipContestBroker, ClipInfo,
    ClipLookup,
};
use tower::ServiceExt;

async fn migrated_pool(db_name: &str) -> Option<PgPool> {
    let Ok(dsn) = std::env::var("TB_TEST_DATABASE_URL") else {
        if std::env::var("TB_TEST_REQUIRE_DB").as_deref() == Ok("1") {
            panic!("TB_TEST_REQUIRE_DB=1 gesetzt, aber TB_TEST_DATABASE_URL fehlt");
        }
        eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
        return None;
    };
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&dsn)
        .await
        .expect("admin connect");
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "DROP DATABASE IF EXISTS {db_name} WITH (FORCE)"
    )))
    .execute(&admin)
    .await
    .unwrap();
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE DATABASE {db_name}")))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
    let opts = PgConnectOptions::from_str(&dsn).unwrap().database(db_name);
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(opts)
        .await
        .expect("connect");
    sqlx::query("CREATE EXTENSION IF NOT EXISTS timescaledb")
        .execute(&pool)
        .await
        .ok();
    let _guard = crate::handlers::TEST_MIGRATE_LOCK.lock().await;
    tb_db::migrate::MIGRATOR
        .run(&pool)
        .await
        .expect("Migrationen");
    Some(pool)
}

struct Lookup;
#[async_trait::async_trait]
impl ClipLookup for Lookup {
    async fn clip(&self, id: &str) -> Result<Option<ClipInfo>, String> {
        Ok(Some(ClipInfo {
            id: id.into(),
            broadcaster_id: if id == "WrongHelix" { "999" } else { "456" }.into(),
            title: "Eigener Clip".into(),
        }))
    }
}
struct Broker(AtomicUsize);
#[async_trait::async_trait]
impl ClipContestBroker for Broker {
    async fn submit(&self, body: &BrokerClipRequest) -> Result<BrokerClipResponse, String> {
        assert_eq!(body.streamer_twitch_user_id, "456");
        assert_eq!(body.submitted_by_twitch_user_id.as_deref(), Some("456"));
        assert_eq!(body.streamer_login, "authentischer_partner");
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(BrokerClipResponse {
            status: BrokerClipStatus::Duplicate,
            submission_id: Some(123),
            reason: Some("idempotency_metadata_drift".into()),
        })
    }
}
fn request(id: i64, actor: &str, token: Option<&str>) -> Request<Body> {
    let mut request = Request::builder()
        .method("POST")
        .uri("/internal/twitch/v1/clips/contest/submit")
        .header("content-type", "application/json")
        .extension(ConnectInfo("127.0.0.1:5000".parse::<SocketAddr>().unwrap()));
    if let Some(token) = token {
        request = request.header("X-Internal-Token", token);
    }
    request
        .body(Body::from(
            json!({"clip_db_id":id,"actor_twitch_user_id":actor}).to_string(),
        ))
        .unwrap()
}
async fn call(router: &axum::Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
#[tokio::test]
async fn producer_prueft_auth_identitaet_helix_und_dauerhaften_drift_replay() {
    let Some(pool) = migrated_pool("tb_clip_producer").await else {
        return;
    };
    sqlx::raw_sql("INSERT INTO twitch_partners(twitch_user_id,twitch_login,status) VALUES ('456','authentischer_partner','active'),('457','inaktiv','archived');
        INSERT INTO twitch_clips_social_media(id,clip_id,clip_url,streamer_login,twitch_user_id,created_at) VALUES
        (1,'OwnClip','https://clips.twitch.tv/OwnClip','falscher_client_login','456',NOW()),
        (2,'WrongHelix','https://clips.twitch.tv/WrongHelix','falscher_client_login','456',NOW()),
        (3,'InactiveClip','https://clips.twitch.tv/InactiveClip','inaktiv','457',NOW());")
        .execute(&pool).await.unwrap();
    let broker = Arc::new(Broker(AtomicUsize::new(0)));
    let submitter = Arc::new(ClipContestSubmitter::new(
        pool.clone(),
        Arc::new(Lookup),
        broker.clone(),
    ));
    let router = crate::build_internal_router(
        pool.clone(),
        "test-token".into(),
        Arc::new(None),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .layer(Extension(ClipContestExt(Some(submitter))));
    assert_eq!(
        call(&router, request(1, "456", None)).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(&router, request(1, "999", Some("test-token"))).await.1["status"],
        "foreign_clip"
    );
    assert_eq!(
        call(&router, request(2, "456", Some("test-token"))).await.1["status"],
        "foreign_clip"
    );
    assert_eq!(
        call(&router, request(3, "457", Some("test-token"))).await.1["status"],
        "not_partner"
    );
    let original = call(&router, request(1, "456", Some("test-token"))).await;
    assert_eq!(original.0, StatusCode::OK);
    assert_eq!(original.1["status"], "already_in");
    assert_eq!(
        original.1["message"],
        "Der Clip wurde bereits eingereicht und wird nicht erneut gesendet."
    );
    assert_eq!(
        call(&router, request(1, "456", Some("test-token"))).await,
        original
    );
    assert_eq!(broker.0.load(Ordering::SeqCst), 1);
    let stored:(String,i64,String)=sqlx::query_as("SELECT status,broker_submission_id,reason FROM twitch_clip_contest_forwards WHERE clip_id='OwnClip'")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(
        stored,
        ("duplicate".into(), 123, "idempotency_metadata_drift".into())
    );
}
