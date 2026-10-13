use super::*;
use axum::body::{to_bytes, Body};
use axum::http::Request;
use serde_json::{json, Value};
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;

async fn schedule_request(
    router: &axum::Router,
    session: &crate::SessionCreation,
    method: &str,
    suffix: &str,
    body: Value,
) -> (StatusCode, Value) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(format!("/twitch/social-media/api/autopilot{suffix}"))
                .header("host", "dashboard.test")
                .header("origin", "https://dashboard.test")
                .header("accept", "application/json")
                .header("content-type", "application/json")
                .header(
                    "cookie",
                    format!("{}={}", crate::PARTNER_COOKIE_NAME, session.session_id),
                )
                .header("x-csrf-token", &session.csrf_token)
                .extension(axum::extract::ConnectInfo(std::net::SocketAddr::from((
                    [192, 0, 2, 1],
                    12345,
                ))))
                .body(Body::from(body.to_string()))
                .expect("valid synthetic authenticated request"),
        )
        .await
        .expect("actual composed route");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("bounded schedule response");
    let body = if status.is_success() {
        serde_json::from_slice(&bytes).expect("successful schedule JSON response")
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, body)
}

#[tokio::test]
async fn autopilot_schedule_authenticated_composition_persists_owned_schedule_and_queue() {
    use base64::Engine;
    use sqlx::postgres::PgConnectOptions;
    use std::str::FromStr;
    use tb_social_media::production_jobs::{self, JobSource, NewProductionJob, ProductionMode};

    let dsn = std::env::var("TB_TEST_DATABASE_URL").expect("owned isolated database required");
    let database = format!("t_sm_b_api_{}", std::process::id());
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&dsn)
        .await
        .expect("isolated admin");
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE DATABASE {database}")))
        .execute(&admin)
        .await
        .expect("owned test database");
    let options = PgConnectOptions::from_str(&dsn)
        .expect("test DSN")
        .database(&database);
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(options)
        .await
        .expect("owned pool");
    sqlx::migrate!("../../migrations")
        .run(&pool)
        .await
        .expect("actual fresh application migrations");
    sqlx::raw_sql("INSERT INTO twitch_streamers (twitch_login,twitch_user_id) VALUES ('b_test_renamed','42'),('b_test_other','99');
        INSERT INTO twitch_partners (id,twitch_login,twitch_user_id,status) VALUES (42,'b_test_renamed','42','active');
        INSERT INTO social_media_partner_access (twitch_user_id,streamer_login,granted) VALUES ('42','b_test_renamed',TRUE)")
        .execute(&pool).await.expect("synthetic partner and existing access policy");
    let key = base64::engine::general_purpose::URL_SAFE.encode(rand::random::<[u8; 32]>());
    let auth = crate::DashboardAuthState::new(pool.clone(), key);
    let session = auth
        .create_partner_session("b_test_old_login", "42", "Test channel")
        .await
        .expect("encrypted synthetic session with lookup-hash storage");
    let router = crate::build_authed_router(
        pool.clone(),
        String::new(),
        crate::RateLimiter::new(pool.clone(), String::new()),
    )
    .layer(axum::Extension(auth));
    let (status, initial) =
        schedule_request(&router, &session, "GET", "/schedule", json!(null)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(initial["persisted"], false);
    let (status, _) = schedule_request(
        &router,
        &session,
        "GET",
        "/schedule?twitch_user_id=99",
        json!(null),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, saved) = schedule_request(
        &router,
        &session,
        "PUT",
        "/schedule",
        json!({
            "enabled":false,"paused":true,"ai_editing_enabled":false,"lead_time_hours":0,
            "timezone":"Europe/Berlin","schedule_slots":[{"weekday":1,"local_time":"18:00"}]
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(saved["persisted"], true);
    assert_eq!(saved["settings"]["enabled"], false);
    assert_eq!(saved["settings"]["ai_editing_enabled"], false);
    assert_eq!(saved["settings"]["lead_time_hours"], 0);
    assert_eq!(saved["production_hold"], "disabled");
    let mut jobs = Vec::new();
    for index in 0..2 {
        let clip: i64 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (twitch_user_id,streamer_login,clip_id,clip_url,clip_title) VALUES ('42','b_test_renamed',$1,'https://clips.test/synthetic','Test clip') RETURNING id::bigint")
            .bind(format!("b_test_{index}")).fetch_one(&pool).await.expect("synthetic owned clip");
        jobs.push(
            production_jobs::insert(
                &pool,
                &NewProductionJob {
                    twitch_user_id: "42".into(),
                    clip_id: clip,
                    source: JobSource::Manual,
                    requested_mode: Some(ProductionMode::Static),
                    scheduled_at: None,
                    max_attempts: 3,
                },
            )
            .await
            .expect("existing queue insertion"),
        );
    }
    let (status, _) = schedule_request(
        &router,
        &session,
        "PUT",
        "/queue/order",
        json!({"job_ids":[jobs[0].id]}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, reordered) = schedule_request(
        &router,
        &session,
        "PUT",
        "/queue/order",
        json!({"job_ids":[jobs[1].id,jobs[0].id]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reordered["jobs"][0]["id"], jobs[1].id);
    let (status, next) = schedule_request(
        &router,
        &session,
        "POST",
        &format!("/queue/{}/next", jobs[0].id),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(next["jobs"][0]["id"], jobs[0].id);
    let (status, _) = schedule_request(
        &router,
        &session,
        "POST",
        &format!("/queue/{}/cancel", jobs[1].id),
        json!({"twitch_user_id":"99"}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, cancelled) = schedule_request(
        &router,
        &session,
        "POST",
        &format!("/queue/{}/cancel", jobs[1].id),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cancelled["jobs"].as_array().expect("queue DTO").len(), 1);
    assert_eq!(
        production_jobs::get(&pool, "42", jobs[1].id)
            .await
            .expect("persisted job")
            .expect("owned job")
            .status,
        "cancelled"
    );
    let uploads: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_clips_upload_queue")
        .fetch_one(&pool)
        .await
        .expect("no publication");
    assert_eq!(uploads, 0);
    drop(router);
    pool.close().await;
    sqlx::query(sqlx::AssertSqlSafe(format!("DROP DATABASE {database}")))
        .execute(&admin)
        .await
        .expect("drop owned database");
    admin.close().await;
}

#[tokio::test]
async fn autopilot_schedule_composed_routes_require_authentication() {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgresql://postgres@127.0.0.1:1/tb_test_unconnected")
        .expect("no database connection is used");
    let router = crate::build_authed_router(
        pool.clone(),
        "synthetic-internal-token".into(),
        crate::RateLimiter::new(pool, String::new()),
    );
    for prefix in [
        "/twitch/social-media/api/autopilot",
        "/social-media/api/autopilot",
    ] {
        for (method, suffix, body) in [
            ("GET", "/schedule", ""),
            ("PUT", "/schedule", "{}"),
            ("PUT", "/queue/order", "{\"job_ids\":[]}"),
            ("POST", "/queue/1/next", "{}"),
            ("POST", "/queue/1/cancel", "{}"),
        ] {
            let response = router
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(format!("{prefix}{suffix}"))
                        .header("host", "localhost")
                        .header("content-type", "application/json")
                        .extension(axum::extract::ConnectInfo(std::net::SocketAddr::from((
                            [127, 0, 0, 1],
                            12345,
                        ))))
                        .body(Body::from(body))
                        .expect("valid synthetic request"),
                )
                .await
                .expect("composed route");
            assert_eq!(
                response.status(),
                StatusCode::UNAUTHORIZED,
                "{method} {prefix}{suffix}"
            );
        }
    }
}

#[test]
fn schedule_body_rejects_foreign_settings_and_preserves_false_values() {
    assert!(serde_json::from_value::<ScheduleWrite>(json!({"publish_mode":"full_auto"})).is_err());
    assert!(
        serde_json::from_value::<ScheduleWrite>(json!({"clip_date_filter":{"kind":"all"}}))
            .is_err()
    );
    let body: ScheduleWrite = serde_json::from_value(
        json!({"twitch_user_id":"42","enabled":false,"ai_editing_enabled":false}),
    )
    .expect("narrow schedule request");
    let patch = body.patch();
    assert_eq!(patch.enabled, Some(false));
    assert_eq!(patch.ai_editing_enabled, Some(false));
    assert!(patch.timezone.is_none());
}

#[test]
fn queue_actions_do_not_accept_mode_or_another_job_payload() {
    assert!(serde_json::from_value::<QueueActionWrite>(json!({"mode":"static"})).is_err());
    assert!(
        serde_json::from_value::<QueueOrderWrite>(json!({"job_ids":[1,2],"enabled":true})).is_err()
    );
    assert!(serde_json::from_value::<QueueOrderWrite>(json!({"job_ids":["1"]})).is_err());
}

#[tokio::test]
async fn handler_errors_keep_stable_status_and_safe_response_shapes() {
    for (error, status, code) in [
        (
            ScheduleError::NotFound,
            StatusCode::NOT_FOUND,
            "job_not_found",
        ),
        (ScheduleError::Started, StatusCode::CONFLICT, "job_started"),
        (
            ScheduleError::QueueChanged,
            StatusCode::CONFLICT,
            "queue_changed",
        ),
        (
            ScheduleError::Invalid("invalid".into()),
            StatusCode::BAD_REQUEST,
            "invalid_schedule",
        ),
    ] {
        let response = failure(error);
        assert_eq!(response.status(), status);
        let bytes = to_bytes(response.into_body(), 4096)
            .await
            .expect("response body");
        let body: Value = serde_json::from_slice(&bytes).expect("JSON response");
        assert_eq!(body["error"], code);
        assert!(body["message"].is_string());
    }
}
