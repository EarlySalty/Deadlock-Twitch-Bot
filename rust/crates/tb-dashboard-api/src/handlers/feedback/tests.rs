use super::*;
use axum::{body::to_bytes, extract::State, Json};

fn partner(id: &str) -> DashboardAuthLevel {
    DashboardAuthLevel::Partner {
        twitch_user_id: id.into(),
        twitch_login: "gleicher_name".into(),
        display_name: "Partner".into(),
    }
}
fn submission(key: &str) -> CreateBody {
    CreateBody {
        request_id: key.into(),
        kind: "feature".into(),
        title: "Mein Wunsch".into(),
        body: "Eine verständliche Beschreibung.".into(),
        area: "Verwaltung".into(),
    }
}
#[test]
fn identity_requires_real_id_and_never_uses_login() {
    assert!(owner_id(&partner("")).is_err());
    assert!(owner_id(&DashboardAuthLevel::None).is_err());
    assert!(owner_id(&DashboardAuthLevel::admin()).is_err());
    assert_eq!(owner_id(&partner("123")).unwrap(), "123");
}
#[test]
fn validation_rejects_incompatible_status_and_unsafe_function_paths() {
    assert!(!valid_status("feature", "answered"));
    assert!(valid_status("feedback", "answered"));
    assert!(!valid_status("feature", "unknown"));
    assert!(valid_result_path("/twitch/verwaltung#overlay"));
    for path in [
        "//evil.example",
        "javascript:alert(1)",
        "/twitch/feedback/42",
        "/twitch/verwaltung?token=x",
        "/twitch/verwaltung%2f..",
        "/twitch/verwaltung/../feedback",
        "/twitch/auth/login",
    ] {
        assert!(!valid_result_path(path), "{path}");
    }
}
#[test]
fn create_validates_lengths_and_key() {
    let mut body = submission("22222222-2222-4222-8222-222222222222");
    assert!(body.normalize().is_ok());
    body.title = "ä".repeat(121);
    assert!(body.normalize().is_err());
    body.title = "Titel".into();
    body.request_id = "kein-schluessel".into();
    assert!(body.normalize().is_err());
}

use crate::test_database;
async fn fixture() -> test_database::Database {
    let db = test_database::Database::new().await;
    sqlx::raw_sql("CREATE TABLE twitch_roadmap_items (id BIGINT PRIMARY KEY, title TEXT NOT NULL, status TEXT NOT NULL, updated_at TIMESTAMPTZ NOT NULL DEFAULT now());")
        .execute(&db.pool).await.unwrap();
    sqlx::raw_sql(include_str!(
        "../../../../../migrations/20260909211000_dashboard_feedback.sql"
    ))
    .execute(&db.pool)
    .await
    .unwrap();
    db
}
async fn json(response: Response) -> (StatusCode, Value) {
    let status = response.status();
    let body = to_bytes(response.into_body(), 100000).await.unwrap();
    (status, serde_json::from_slice(&body).unwrap())
}
#[tokio::test]
async fn durable_idempotency_ownership_and_rate_limit() {
    let db = fixture().await;
    let key = "22222222-2222-4222-8222-222222222222";
    let (first, second) = tokio::join!(
        create(
            partner("123"),
            State(db.pool.clone()),
            Json(submission(key))
        ),
        create(
            partner("123"),
            State(db.pool.clone()),
            Json(submission(key))
        )
    );
    let (a, b) = (json(first).await, json(second).await);
    assert!(matches!(a.0, StatusCode::OK | StatusCode::CREATED));
    assert!(matches!(b.0, StatusCode::OK | StatusCode::CREATED));
    assert_eq!(a.1["id"], b.1["id"]);
    let id = a.1["id"].as_i64().unwrap();
    assert_eq!(
        detail(partner("456"), State(db.pool.clone()), Path(id))
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        mark_read(
            partner("456"),
            State(db.pool.clone()),
            Path(id),
            Json(ReadBody {
                observed_at: chrono::Utc::now(),
                inbox: false
            })
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        list(
            partner("456"),
            State(db.pool.clone()),
            Query(ListQuery {
                inbox: true,
                before: None
            })
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    let mut changed = submission(key);
    changed.body = "Anderer Inhalt".into();
    assert_eq!(
        create(partner("123"), State(db.pool.clone()), Json(changed))
            .await
            .status(),
        StatusCode::CONFLICT
    );
    for _ in 0..4 {
        let key = uuid::Uuid::new_v4().to_string();
        assert_eq!(
            create(
                partner("123"),
                State(db.pool.clone()),
                Json(submission(&key))
            )
            .await
            .status(),
            StatusCode::CREATED
        );
    }
    assert_eq!(
        create(
            partner("123"),
            State(db.pool.clone()),
            Json(submission(&uuid::Uuid::new_v4().to_string()))
        )
        .await
        .status(),
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(
        create(
            partner("123"),
            State(db.pool.clone()),
            Json(submission(key))
        )
        .await
        .status(),
        StatusCode::OK
    );
    db.close().await;
}
#[tokio::test]
async fn updates_are_atomic_private_and_read_markers_do_not_swallow_later_replies() {
    let db = fixture().await;
    let (_, entry) = json(
        create(
            partner("123"),
            State(db.pool.clone()),
            Json(submission(&uuid::Uuid::new_v4().to_string())),
        )
        .await,
    )
    .await;
    let id = entry["id"].as_i64().unwrap();
    let mut body = UpdateBody {
        request_id: uuid::Uuid::new_v4().to_string(),
        expected_revision: 0,
        status: "declined".into(),
        reply: "".into(),
        result_path: None,
        decision_reason: None,
        roadmap_id: None,
        duplicate_of: None,
    };
    assert_eq!(
        update(
            partner("456"),
            State(db.pool.clone()),
            Path(id),
            Json(body.clone())
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        update(
            DashboardAuthLevel::admin(),
            State(db.pool.clone()),
            Path(id),
            Json(body.clone())
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    body.status = "reviewing".into();
    body.reply = "Wir prüfen deinen Wunsch.".into();
    assert_eq!(
        update(
            DashboardAuthLevel::admin(),
            State(db.pool.clone()),
            Path(id),
            Json(body.clone())
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        update(
            DashboardAuthLevel::admin(),
            State(db.pool.clone()),
            Path(id),
            Json(body.clone())
        )
        .await
        .status(),
        StatusCode::OK
    );
    let (_, viewed) = json(detail(partner("123"), State(db.pool.clone()), Path(id)).await).await;
    assert_eq!(viewed["events"].as_array().unwrap().len(), 1);
    assert_eq!(viewed["unread"], true);
    let seen: chrono::DateTime<chrono::Utc> =
        serde_json::from_value(viewed["observed_at"].clone()).unwrap();
    body.request_id = uuid::Uuid::new_v4().to_string();
    body.expected_revision = 1;
    body.reply = "Eine zweite Antwort.".into();
    assert_eq!(
        update(
            DashboardAuthLevel::admin(),
            State(db.pool.clone()),
            Path(id),
            Json(body.clone())
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        mark_read(
            partner("123"),
            State(db.pool.clone()),
            Path(id),
            Json(ReadBody {
                observed_at: seen,
                inbox: false
            })
        )
        .await
        .status(),
        StatusCode::OK
    );
    let (_, unread) = json(detail(partner("123"), State(db.pool.clone()), Path(id)).await).await;
    assert_eq!(unread["unread"], true);
    body.request_id = uuid::Uuid::new_v4().to_string();
    body.expected_revision = 0;
    assert_eq!(
        update(
            DashboardAuthLevel::admin(),
            State(db.pool.clone()),
            Path(id),
            Json(body)
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    db.close().await;
}
#[tokio::test]
async fn duplicates_and_roadmap_expose_only_public_progress() {
    let db = fixture().await;
    let (_, first) = json(
        create(
            partner("123"),
            State(db.pool.clone()),
            Json(submission(&uuid::Uuid::new_v4().to_string())),
        )
        .await,
    )
    .await;
    let (_, second) = json(
        create(
            partner("456"),
            State(db.pool.clone()),
            Json(submission(&uuid::Uuid::new_v4().to_string())),
        )
        .await,
    )
    .await;
    let (a, b) = (
        first["id"].as_i64().unwrap(),
        second["id"].as_i64().unwrap(),
    );
    sqlx::query("INSERT INTO twitch_roadmap_items(id,title,status) VALUES (1,'Öffentlicher Plan','planned')").execute(&db.pool).await.unwrap();
    let mut body = UpdateBody {
        request_id: uuid::Uuid::new_v4().to_string(),
        expected_revision: 0,
        status: "planned".into(),
        reply: "Private Antwort für A".into(),
        result_path: Some("/twitch/verwaltung#overlay".into()),
        decision_reason: None,
        roadmap_id: Some(1),
        duplicate_of: None,
    };
    assert_eq!(
        update(
            DashboardAuthLevel::admin(),
            State(db.pool.clone()),
            Path(a),
            Json(body.clone())
        )
        .await
        .status(),
        StatusCode::OK
    );
    body.request_id = uuid::Uuid::new_v4().to_string();
    body.reply = "Dein Wunsch ist gebündelt.".into();
    body.roadmap_id = None;
    body.result_path = None;
    body.duplicate_of = Some(a);
    assert_eq!(
        update(
            DashboardAuthLevel::admin(),
            State(db.pool.clone()),
            Path(b),
            Json(body.clone())
        )
        .await
        .status(),
        StatusCode::OK
    );
    sqlx::query(
        "UPDATE twitch_roadmap_items SET status='done',updated_at=clock_timestamp() WHERE id=1",
    )
    .execute(&db.pool)
    .await
    .unwrap();
    let (_, visible) = json(detail(partner("456"), State(db.pool.clone()), Path(b)).await).await;
    assert_eq!(visible["status"], "done");
    assert_eq!(visible["result_path"], "/twitch/verwaltung#overlay");
    assert_eq!(visible["bundled"], true);
    assert!(visible.get("duplicate_of").is_none());
    assert!(visible.get("owner_id").is_none());
    assert!(!visible.to_string().contains("Private Antwort für A"));
    body.request_id = uuid::Uuid::new_v4().to_string();
    body.expected_revision = 1;
    body.duplicate_of = Some(b);
    assert_eq!(
        update(
            DashboardAuthLevel::admin(),
            State(db.pool.clone()),
            Path(a),
            Json(body)
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    db.close().await;
}

#[tokio::test]
async fn bundled_decline_has_explicit_shared_reason_but_no_private_reply() {
    let db = fixture().await;
    let (_, first) = json(
        create(
            partner("123"),
            State(db.pool.clone()),
            Json(submission(&uuid::Uuid::new_v4().to_string())),
        )
        .await,
    )
    .await;
    let (_, second) = json(
        create(
            partner("456"),
            State(db.pool.clone()),
            Json(submission(&uuid::Uuid::new_v4().to_string())),
        )
        .await,
    )
    .await;
    let (a, b) = (
        first["id"].as_i64().unwrap(),
        second["id"].as_i64().unwrap(),
    );
    let mut change = UpdateBody {
        request_id: uuid::Uuid::new_v4().to_string(),
        expected_revision: 0,
        status: "reviewing".into(),
        reply: "".into(),
        result_path: None,
        decision_reason: None,
        roadmap_id: None,
        duplicate_of: Some(a),
    };
    assert_eq!(
        update(
            DashboardAuthLevel::admin(),
            State(db.pool.clone()),
            Path(b),
            Json(change.clone())
        )
        .await
        .status(),
        StatusCode::OK
    );
    change.request_id = uuid::Uuid::new_v4().to_string();
    change.duplicate_of = None;
    change.status = "declined".into();
    change.reply = "Private Einzelantwort für A".into();
    change.decision_reason = Some("Die Plattform bietet dafür keine Schnittstelle.".into());
    assert_eq!(
        update(
            DashboardAuthLevel::admin(),
            State(db.pool.clone()),
            Path(a),
            Json(change)
        )
        .await
        .status(),
        StatusCode::OK
    );
    let (_, viewed) = json(detail(partner("456"), State(db.pool.clone()), Path(b)).await).await;
    assert_eq!(viewed["status"], "declined");
    assert_eq!(
        viewed["decision_reason"],
        "Die Plattform bietet dafür keine Schnittstelle."
    );
    assert!(!viewed.to_string().contains("Private Einzelantwort"));
    let (_, pending) = json(
        counts(
            DashboardAuthLevel::admin(),
            State(db.pool.clone()),
            Query(ListQuery {
                inbox: true,
                before: None,
            }),
        )
        .await,
    )
    .await;
    assert_eq!(pending["unread"], 0);
    db.close().await;
}
