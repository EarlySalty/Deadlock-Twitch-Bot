use super::*;
use axum::{body::Body, extract::ConnectInfo, http::Request};
use serde_json::Value;

use std::{
    net::SocketAddr,
    sync::atomic::{AtomicUsize, Ordering},
};
use tb_chat::clip_contest_submit::{
    BrokerClipRequest, BrokerClipResponse, BrokerClipStatus, ClipContestBroker, ClipInfo,
    ClipLookup,
};
use tower::ServiceExt;

async fn migrated_pool(_db_name: &str) -> crate::test_postgres::TestPostgres {
    let db = crate::test_postgres::TestPostgres::start_with_timescaledb().await;
    let pool = db.pool.clone();
    sqlx::query("CREATE EXTENSION IF NOT EXISTS timescaledb")
        .execute(&pool)
        .await
        .expect("TimescaleDB");
    let _guard = crate::handlers::TEST_MIGRATE_LOCK.lock().await;
    tb_db::migrate::MIGRATOR
        .run(&pool)
        .await
        .expect("Migrationen");
    db
}

#[tokio::test]
async fn clip_contest_rollenmatrix_erhaelt_community_und_watchdog_vertrag() {
    let db = crate::test_postgres::TestPostgres::start_with_timescaledb().await;
    for ddl in [
        "CREATE ROLE postgres NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS",
        "CREATE DATABASE twitch_analytics",
        "CREATE DATABASE twitch_all_live_test",
        "CREATE ROLE twitchcollector NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS",
    ] {
        sqlx::query(ddl).execute(&db.pool).await.unwrap();
    }
    let options = (*db.pool.connect_options())
        .clone()
        .database("twitch_all_live_test");
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect_with(options.clone())
        .await
        .unwrap();
    sqlx::query("CREATE EXTENSION timescaledb")
        .execute(&pool)
        .await
        .unwrap();
    tb_db::migrate::MIGRATOR.run(&pool).await.unwrap();
    let watchdog = tb_db::migrate::MIGRATOR
        .iter()
        .find(|migration| migration.version == 20261001220000)
        .expect("Autoritative Watchdogmigration gehört zum vollständigen Quellstand");
    let checksum: Vec<u8> = sqlx::query_scalar(
        "SELECT checksum FROM _sqlx_migrations WHERE version=20261001220000 AND success",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(checksum.as_slice(), watchdog.checksum.as_ref());
    let sql = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../ops/systemd/test_community_runtime_roles.sql");
    let output = std::process::Command::new("/usr/lib/postgresql/16/bin/psql")
        .args([
            "--no-psqlrc",
            "--host",
            options.get_host(),
            "--username",
            "uplink_test",
            "--dbname",
            "twitch_all_live_test",
            "--file",
        ])
        .arg(sql)
        .output()
        .expect("Rollenfixture benötigt PostgreSQL 16 mit /usr/lib/postgresql/16/bin/psql; uplink_test wird ausschließlich im isolierten Testcluster angelegt");
    assert!(
        output.status.success(),
        "Rollenprüfung fehlgeschlagen: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    pool.close().await;
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
        let submitted_at = body
            .submitted_at
            .as_deref()
            .expect("Serverseitige DB-Herkunft");
        assert!(chrono::DateTime::parse_from_rfc3339(submitted_at).is_ok());
        assert!(submitted_at.ends_with('Z'));
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
    let _db = migrated_pool("tb_clip_producer").await;
    let pool = _db.pool.clone();
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
