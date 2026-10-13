use super::*;
use axum::{
    body::{to_bytes, Body},
    http::Request,
};
use serde_json::{json, Value};
use tb_social_media::ai_editor::skill_registry::{LearnedChange, NewProposal, SkillEvidence};
use tower::ServiceExt;

async fn fixture() -> (crate::test_database::Database, DashboardAuthState, Router) {
    let database = crate::test_database::Database::new().await;
    let pool = &database.pool;
    sqlx::raw_sql("CREATE TABLE public.dashboard_sessions(session_id TEXT PRIMARY KEY,session_type TEXT NOT NULL,payload_enc BYTEA NOT NULL,created_at DOUBLE PRECISION NOT NULL,expires_at DOUBLE PRECISION NOT NULL)")
        .execute(pool).await.expect("isolated schema");
    sqlx::raw_sql(include_str!(
        "../../../../migrations/20261011204100_social_ai_skill_versions.sql"
    ))
    .execute(pool)
    .await
    .expect("real migration");
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
    serde_json::from_slice(&to_bytes(response.into_body(), 262144).await.expect("body"))
        .expect("public DTO")
}

fn skill<'a>(report: &'a Value, id: &str) -> &'a Value {
    report["skills"]
        .as_array()
        .expect("skills")
        .iter()
        .find(|skill| skill["skill_id"] == id)
        .expect("skill entry")
}

#[tokio::test]
async fn skill_history_shows_learned_diff_and_rollback_restores_bundled_version() {
    let (database, state, app) = fixture().await;
    assert_eq!(
        call(&app, "/skills", None, None, None).await.status(),
        StatusCode::UNAUTHORIZED
    );
    let admin = state
        .create_admin_session("42", "Fixture Admin")
        .await
        .expect("admin session");
    let cookie = format!(
        "{}={}",
        crate::auth::session::ADMIN_COOKIE_NAME,
        admin.session_id
    );
    let initial = data(call(&app, "/skills", None, Some(&cookie), None).await).await;
    assert_eq!(initial["skills"].as_array().map(Vec::len), Some(8));
    assert_eq!(skill(&initial, "copywriting")["current_version"], "1");

    let base = tb_social_media::ai_editor::skill_snapshot(SkillId::Copywriting).text;
    let learned = format!(
        "{}\nPrefer the streamer's own wording for callouts.\n",
        base.replace("# Copywriting v1", "# Copywriting v2")
    );
    let evidence = SkillEvidence {
        summary: "Wiederholte Korrekturen".into(),
        sample_count: Some(6),
    };
    let version = skill_registry::activate_learned(
        &database.pool,
        SkillId::Copywriting,
        LearnedChange {
            text: &learned,
            summary: "Eigene Formulierungen",
            reason: "Wiederholte Korrekturen",
            evidence: Some(&evidence),
            actor: "system:learning",
            max_bytes: 4096,
            expected_version: None,
        },
    )
    .await
    .expect("learned version");
    assert_eq!(version, 2);
    let active = skill_registry::active_skill(&database.pool, SkillId::Copywriting, 4096)
        .await
        .expect("active");
    assert_eq!(
        (active.version, active.text.as_str()),
        (2, learned.as_str())
    );
    skill_registry::record_proposal(
        &database.pool,
        NewProposal {
            title: "Feld vorschlagen",
            summary: "Zusatzfeld",
            reason: "Häufige Nachfrage",
            evidence: None,
        },
    )
    .await
    .expect("proposal");

    let learned_report = data(call(&app, "/skills", None, Some(&cookie), None).await).await;
    let entry = skill(&learned_report, "copywriting");
    assert_eq!(entry["current_version"], "2");
    assert_eq!(entry["versions"][0]["previous_version"], "1");
    assert_eq!(entry["versions"][0]["evidence"]["sample_count"], 6);
    let diff = entry["versions"][0]["diff"].as_array().expect("diff");
    assert!(diff
        .iter()
        .any(|line| line["kind"] == "added" && line["text"] == "# Copywriting v2"));
    assert!(diff
        .iter()
        .any(|line| line["kind"] == "removed" && line["text"] == "# Copywriting v1"));
    assert_eq!(learned_report["proposals"][0]["title"], "Feld vorschlagen");

    let cross_origin = Request::builder()
        .method("POST")
        .uri("/twitch/api/admin/social-ai/skills/copywriting/rollback")
        .header("host", "admin.test")
        .header("origin", "https://attacker.example")
        .header("x-dashboard-context", "admin")
        .header("cookie", &cookie)
        .header("Content-Type", "application/json")
        .body(Body::from(
            json!({"expected_version":"2","target_version":"1","reason":"fixture"}).to_string(),
        ))
        .expect("request");
    assert_eq!(
        app.clone()
            .oneshot(cross_origin)
            .await
            .expect("response")
            .status(),
        StatusCode::FORBIDDEN
    );

    let stale = json!({"expected_version":"1","target_version":"2","reason":"fixture","csrf_token":admin.csrf_token});
    assert_eq!(
        call(
            &app,
            "/skills/copywriting/rollback",
            Some(stale),
            Some(&cookie),
            Some(&admin.csrf_token)
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    let forged = json!({"expected_version":"2","target_version":"1","reason":"fixture","actor":"forged","csrf_token":admin.csrf_token});
    assert_eq!(
        call(
            &app,
            "/skills/copywriting/rollback",
            Some(forged),
            Some(&cookie),
            Some(&admin.csrf_token)
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    let missing = json!({"expected_version":"2","target_version":"9","reason":"fixture","csrf_token":admin.csrf_token});
    assert_eq!(
        call(
            &app,
            "/skills/copywriting/rollback",
            Some(missing),
            Some(&cookie),
            Some(&admin.csrf_token)
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    let unknown = json!({"expected_version":"2","target_version":"1","reason":"fixture","csrf_token":admin.csrf_token});
    assert_eq!(
        call(
            &app,
            "/skills/unknown/rollback",
            Some(unknown),
            Some(&cookie),
            Some(&admin.csrf_token)
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );

    let body = json!({"expected_version":"2","target_version":"1","reason":"Rückkehr zur Ausgangsversion","csrf_token":admin.csrf_token});
    let rolled = data(
        call(
            &app,
            "/skills/copywriting/rollback",
            Some(body),
            Some(&cookie),
            Some(&admin.csrf_token),
        )
        .await,
    )
    .await;
    assert_eq!(skill(&rolled, "copywriting")["current_version"], "1");
    let active = skill_registry::active_skill(&database.pool, SkillId::Copywriting, 4096)
        .await
        .expect("active");
    assert_eq!((active.version, active.text.as_str()), (1, base));
    let audit: (String, i32, i32, String) = sqlx::query_as("SELECT kind, from_version, to_version, actor FROM public.social_ai_skill_activations ORDER BY id DESC LIMIT 1")
        .fetch_one(&database.pool)
        .await
        .expect("audit row");
    assert_eq!(
        audit,
        ("rollback".to_owned(), 2, 1, "discord:42".to_owned())
    );
    database.close().await;
}
