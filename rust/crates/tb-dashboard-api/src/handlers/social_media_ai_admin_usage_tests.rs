use super::*;
use axum::{
    body::{to_bytes, Body},
    http::Request,
};
use serde_json::{json, Value};
use tower::ServiceExt;

async fn fixture() -> (crate::test_database::Database, DashboardAuthState, Router) {
    let database = crate::test_database::Database::new().await;
    let pool = &database.pool;
    sqlx::raw_sql("CREATE TABLE public.twitch_streamers(twitch_user_id TEXT PRIMARY KEY); INSERT INTO public.twitch_streamers VALUES ('101'); CREATE TABLE public.twitch_clips_social_media(id BIGINT PRIMARY KEY); CREATE TABLE public.llm_usage(id BIGSERIAL PRIMARY KEY,ts TEXT NOT NULL,purpose TEXT,provider TEXT,model TEXT,attempt_state TEXT,tokens_in BIGINT,tokens_out BIGINT,latency_ms BIGINT); CREATE TABLE public.dashboard_sessions(session_id TEXT PRIMARY KEY,session_type TEXT NOT NULL,payload_enc BYTEA NOT NULL,created_at DOUBLE PRECISION NOT NULL,expires_at DOUBLE PRECISION NOT NULL); CREATE TABLE public.twitch_partners(id BIGINT PRIMARY KEY,twitch_login TEXT NOT NULL,twitch_user_id TEXT NOT NULL,status TEXT NOT NULL DEFAULT 'active',technical_pause_reason TEXT,admin_archived_at TEXT,departnered_at TEXT,partnered_at TEXT DEFAULT CURRENT_TIMESTAMP)")
        .execute(pool).await.expect("isolated schema");
    for migration in [
        include_str!("../../../../migrations/20261011200001_social_ai_provider_usage.sql"),
        include_str!("../../../../migrations/20261011200002_social_ai_jobs.sql"),
        include_str!("../../../../migrations/20261011204000_social_ai_admin_limits.sql"),
    ] {
        sqlx::raw_sql(migration)
            .execute(pool)
            .await
            .expect("real migration");
    }
    let state = DashboardAuthState::new(
        pool.clone(),
        "dGVzdGtleTEyMzQ1Njc4OTAxMjM0NTY3ODkwMTIzNDU=".to_owned(),
    );
    let router = crate::build_admin_config_router(pool.clone(), "synthetic-internal".into())
        .layer(Extension(state.clone()));
    (database, state, router)
}

async fn call(
    app: &Router,
    path: &str,
    body: Option<Value>,
    cookie: Option<&str>,
    csrf: Option<&str>,
) -> Response {
    let mut request = Request::builder()
        .uri(format!("/twitch/api/admin/social-ai{path}"))
        .header("host", "admin.test")
        .header("x-dashboard-context", "admin");
    if let Some(cookie) = cookie {
        request = request.header("cookie", cookie);
    }
    if let Some(csrf) = csrf {
        request = request.header("X-CSRF-Token", csrf);
    }
    let body = match body {
        Some(body) => {
            request = request
                .method("POST")
                .header("Content-Type", "application/json");
            Body::from(body.to_string())
        }
        None => Body::empty(),
    };
    app.clone()
        .oneshot(request.body(body).expect("request"))
        .await
        .expect("response")
}

async fn data(response: Response) -> Value {
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(CACHE_CONTROL).expect("cache header"),
        "no-store"
    );
    serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.expect("body"))
        .expect("public DTO")
}

#[tokio::test]
async fn m4_registered_routes_enforce_real_admin_session_and_csrf() {
    let (database, state, app) = fixture().await;
    for path in ["/usage", "/limits"] {
        assert_eq!(
            call(&app, path, None, None, None).await.status(),
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        call(&app, "/limits/global", Some(json!({})), None, None)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    sqlx::query("INSERT INTO public.twitch_partners(id,twitch_login,twitch_user_id) VALUES(101,'fixture','101')").execute(&database.pool).await.expect("actual partner");
    let partner = state
        .create_partner_session("fixture", "101", "Fixture")
        .await
        .expect("partner session");
    let partner_cookie = format!(
        "{}={}",
        crate::auth::session::PARTNER_COOKIE_NAME,
        partner.session_id
    );
    assert_eq!(
        call(&app, "/usage", None, Some(&partner_cookie), None)
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    let admin = state
        .create_admin_session("42", "Fixture Admin")
        .await
        .expect("actual admin session");
    let cookie = format!(
        "{}={}",
        crate::auth::session::ADMIN_COOKIE_NAME,
        admin.session_id
    );
    let report = data(call(&app, "/limits", None, Some(&cookie), None).await).await;
    assert_eq!(report["global"]["revision"], 1);
    assert_eq!(report["global"]["initial_total_cap_micro_usd"], 2_000_000);
    let mut body = json!({"expected_revision":1,"paused":true,"reason":"fixture"});
    let cross_origin = Request::builder()
        .method("POST")
        .uri("/twitch/api/admin/social-ai/limits/global")
        .header("host", "admin.test")
        .header("origin", "https://attacker.example")
        .header("x-dashboard-context", "admin")
        .header("cookie", &cookie)
        .header("Content-Type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("request");
    assert_eq!(
        app.clone()
            .oneshot(cross_origin)
            .await
            .expect("response")
            .status(),
        StatusCode::FORBIDDEN
    );
    body["csrf_token"] = json!(admin.csrf_token);
    let saved = data(
        call(
            &app,
            "/limits/global",
            Some(body.clone()),
            Some(&cookie),
            Some(&admin.csrf_token),
        )
        .await,
    )
    .await;
    assert_eq!(saved["global"]["revision"], 2);
    assert_eq!(saved["global"]["paused"], true);
    assert_eq!(saved["global"]["updated_by"], "discord:42");
    assert_eq!(
        call(
            &app,
            "/limits/global",
            Some(body),
            Some(&cookie),
            Some(&admin.csrf_token)
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    let unchanged = data(call(&app, "/limits", None, Some(&cookie), None).await).await;
    assert_eq!(unchanged["global"], saved["global"]);
    let forbidden = json!({"expected_revision":2,"paused":false,"reason":"fixture","initial_total_cap_micro_usd":4000000,"csrf_token":admin.csrf_token});
    assert_eq!(
        call(
            &app,
            "/limits/global",
            Some(forbidden),
            Some(&cookie),
            Some(&admin.csrf_token)
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    let user = json!({"twitch_user_id":"101","expected_revision":0,"paused":true,"monthly_cap_micro_usd":0,"reason":"fixture","csrf_token":admin.csrf_token});
    let saved = data(
        call(
            &app,
            "/limits/user",
            Some(user.clone()),
            Some(&cookie),
            Some(&admin.csrf_token),
        )
        .await,
    )
    .await;
    assert_eq!(saved["users"][0]["revision"], 1);
    assert_eq!(
        call(
            &app,
            "/limits/user",
            Some(user),
            Some(&cookie),
            Some(&admin.csrf_token)
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    let plan = json!({"plan_id":catalog()[0].0,"expected_revision":0,"monthly_cap_micro_usd":0,"monthly_ai_jobs":0,"monthly_revisions":0,"reason":"fixture","csrf_token":admin.csrf_token});
    let saved = data(
        call(
            &app,
            "/limits/plan",
            Some(plan.clone()),
            Some(&cookie),
            Some(&admin.csrf_token),
        )
        .await,
    )
    .await;
    assert_eq!(saved["plan_policies_active"], false);
    assert_eq!(
        call(
            &app,
            "/limits/plan",
            Some(plan),
            Some(&cookie),
            Some(&admin.csrf_token)
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    let usage = data(
        call(
            &app,
            "/usage?group_by=job&limit=25&offset=0&provider=%27%20OR%20TRUE%20--",
            None,
            Some(&cookie),
            None,
        )
        .await,
    )
    .await;
    assert_eq!(usage["totals"]["attempts"], 0);
    assert_eq!(usage["group_by"], "job");
    assert_eq!(
        call(&app, "/usage?limit=101", None, Some(&cookie), None)
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    database.close().await;
}

#[test]
fn m4_strict_write_contract_rejects_cap_actor_and_unknown_fields() {
    assert!(parse::<GlobalControlChange>(
        br#"{"expected_revision":1,"paused":false,"reason":"fixture","csrf_token":"synthetic"}"#
    )
    .is_ok());
    for body in [br#"{"expected_revision":1,"paused":false,"reason":"fixture","updated_by":"forged"}"#.as_slice(), br#"{"expected_revision":1,"paused":false,"reason":"fixture","initial_total_cap_micro_usd":2000001}"#.as_slice()] { assert!(parse::<GlobalControlChange>(body).is_err()); }
    assert_eq!(
        limit_error(LimitError::Conflict).status,
        StatusCode::CONFLICT
    );
}
