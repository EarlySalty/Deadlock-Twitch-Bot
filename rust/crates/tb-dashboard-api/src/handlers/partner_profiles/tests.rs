use super::*;
use axum::body::to_bytes;

fn partner() -> DashboardAuthLevel {
    DashboardAuthLevel::Partner {
        twitch_login: "alice".into(),
        twitch_user_id: "1".into(),
        display_name: "Alice".into(),
    }
}
fn update(revision: i64, published: bool) -> Update {
    Update {
        revision,
        published,
        profile: Content::default(),
    }
}
async fn body(response: Response) -> String {
    String::from_utf8(
        to_bytes(response.into_body(), 2_000_000)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap()
}
async fn database() -> crate::test_database::Database {
    let db = crate::test_database::Database::new().await;
    sqlx::raw_sql(r#"
        CREATE TABLE twitch_partners (twitch_user_id TEXT PRIMARY KEY,twitch_login TEXT,status TEXT DEFAULT 'active',departnered_at TEXT,admin_archived_at TEXT,manual_partner_opt_out INT DEFAULT 0,raid_bot_enabled INT DEFAULT 1,technical_pause_reason TEXT);
        CREATE TABLE twitch_live_state (twitch_user_id TEXT PRIMARY KEY,is_live INT,last_seen_at TEXT,last_game TEXT);
        CREATE TABLE twitch_stream_sessions (twitch_user_id TEXT,streamer_login TEXT,started_at TIMESTAMPTZ,ended_at TIMESTAMPTZ,game_name TEXT,stream_title TEXT);
        INSERT INTO twitch_partners(twitch_user_id,twitch_login) VALUES('1','alice'),('2','bob');
        INSERT INTO twitch_stream_sessions VALUES('1','alice',now()-INTERVAL '2 days',now()-INTERVAL '2 days'+INTERVAL '2 hours','Deadlock','A public title');
    "#).execute(&db.pool).await.unwrap();
    sqlx::raw_sql(include_str!(
        "../../../../../migrations/20260918210000_partner_profiles.sql"
    ))
    .execute(&db.pool)
    .await
    .unwrap();
    db
}
#[test]
fn input_and_url_validation() {
    assert_eq!(valid_login(" EarlySalty "), Some("earlysalty".into()));
    for raw in ["../a", "@alice", "a/b", "<script>", ""] {
        assert!(valid_login(raw).is_none());
    }
    for url in [
        "javascript:alert(1)",
        "http://youtube.com/a",
        "//youtube.com",
        "https://u:p@youtube.com",
        "https://127.0.0.1/",
        "https://[::1]",
        "https://localhost/a",
        "https://a.local/a",
        "https://example.com:8080",
    ] {
        assert!(!safe_url(url), "{url}");
    }
    assert!(safe_url("https://www.youtube.com/@alice"));
    let mut u = update(0, true);
    u.profile.socials.push(Social {
        label: "Website".into(),
        url: "javascript:alert(1)".into(),
    });
    assert!(validate(&mut u).is_err());
    u.profile.socials.clear();
    u.profile.headline = "a".repeat(121);
    assert!(validate(&mut u).is_err());
    u.profile.headline = "Grüße aus der Community".into();
    assert!(validate(&mut u).is_ok());
}
#[test]
fn calendar_limits_and_dst() {
    let mut u = update(0, true);
    let start = Utc::now();
    u.profile.events.push(Event {
        id: uuid::Uuid::new_v4(),
        title: "Stream".into(),
        description: "".into(),
        starts_at: start,
        ends_at: start + Duration::hours(2),
    });
    assert!(validate(&mut u).is_ok());
    u.profile.events.push(u.profile.events[0].clone());
    assert!(validate(&mut u).is_err());
    u.profile.events.pop();
    u.profile.events[0].ends_at = start;
    assert!(validate(&mut u).is_err());
    u.profile.events[0].ends_at = start + Duration::hours(49);
    assert!(validate(&mut u).is_err());
    assert!(month_range(Some("2026-13"), start).is_none());
    assert!(month_range(Some("9999-01"), start).is_none());
    let (_, a, b) = month_range(Some("2026-03"), start).unwrap();
    assert_eq!((b - a).num_hours(), 31 * 24 - 1);
    let (_, a, b) = month_range(Some("2026-10"), start).unwrap();
    assert_eq!((b - a).num_hours(), 31 * 24 + 1);
}
#[test]
fn owners_are_bound_to_session() {
    assert!(owner(&partner(), &OwnerParams::default()).is_ok());
    assert_eq!(
        owner(
            &partner(),
            &OwnerParams {
                streamer: Some("bob".into())
            }
        )
        .unwrap_err()
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        owner(&DashboardAuthLevel::None, &OwnerParams::default())
            .unwrap_err()
            .status(),
        StatusCode::UNAUTHORIZED
    );
}
#[tokio::test]
async fn profiles_are_opt_in_atomic_and_xss_safe() {
    let db = database().await;
    let response = page_handler(
        State(db.pool.clone()),
        Path("@alice".into()),
        Query(PageParams::default()),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        response.headers()[header::CACHE_CONTROL],
        "no-store, max-age=0"
    );
    let get = get_handler(
        partner(),
        State(db.pool.clone()),
        Query(OwnerParams::default()),
    )
    .await;
    let value: serde_json::Value = serde_json::from_str(&body(get).await).unwrap();
    assert_eq!(value["revision"], 0);
    assert_eq!(value["published"], false);
    let mut first = update(0, true);
    first.profile.headline = "<script>alert(1)</script>".into();
    first.profile.about = "Grüße & Spaß".into();
    first.profile.featured = vec!["bob".into()];
    let save = put_handler(
        partner(),
        State(db.pool.clone()),
        Query(OwnerParams::default()),
        Json(first),
    )
    .await;
    assert_eq!(save.status(), StatusCode::OK);
    let stale = put_handler(
        partner(),
        State(db.pool.clone()),
        Query(OwnerParams::default()),
        Json(update(0, false)),
    )
    .await;
    assert_eq!(stale.status(), StatusCode::CONFLICT);
    let response = page_handler(
        State(db.pool.clone()),
        Path("@alice".into()),
        Query(PageParams::default()),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response
        .headers()
        .contains_key(header::CONTENT_SECURITY_POLICY));
    let html = body(response).await;
    assert!(!html.contains("<script>"));
    assert!(html.contains("&lt;script&gt;"));
    assert!(html.contains("Grüße &amp; Spaß"));
    assert!(html.contains("1 erfasste Streams"));
    assert!(!html.contains("/streamer/@bob"));
    assert!(!html.contains("twitch_user_id"));
    assert!(!html.contains("avg_viewers"));
    let (a, b) = tokio::join!(
        put_handler(
            partner(),
            State(db.pool.clone()),
            Query(OwnerParams::default()),
            Json(update(1, false))
        ),
        put_handler(
            partner(),
            State(db.pool.clone()),
            Query(OwnerParams::default()),
            Json(update(1, false))
        )
    );
    let mut statuses = vec![a.status().as_u16(), b.status().as_u16()];
    statuses.sort();
    assert_eq!(statuses, vec![200, 409]);
    assert_eq!(
        page_handler(
            State(db.pool.clone()),
            Path("@alice".into()),
            Query(PageParams::default())
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    db.close().await;
}
#[tokio::test]
async fn every_disconnect_flag_hides_all_public_surfaces_without_erasing_content() {
    let db = database().await;
    assert_eq!(
        put_handler(
            partner(),
            State(db.pool.clone()),
            Query(OwnerParams::default()),
            Json(update(0, true))
        )
        .await
        .status(),
        StatusCode::OK
    );
    for change in [
        "status='archived'",
        "departnered_at='2026-01-01'",
        "admin_archived_at='2026-01-01'",
        "manual_partner_opt_out=1",
        "raid_bot_enabled=0",
        "technical_pause_reason='bot_banned'",
        "technical_pause_reason='token_error'",
    ] {
        sqlx::query(&format!(
            "UPDATE twitch_partners SET {change} WHERE twitch_user_id='1'"
        ))
        .execute(&db.pool)
        .await
        .unwrap();
        let response = page_handler(
            State(db.pool.clone()),
            Path("@alice".into()),
            Query(PageParams::default()),
        )
        .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{change}");
        assert_eq!(response.headers()["x-robots-tag"], "noindex, nofollow");
        assert!(directory(&db.pool).await.unwrap().is_empty());
        assert_eq!(
            put_handler(
                partner(),
                State(db.pool.clone()),
                Query(OwnerParams::default()),
                Json(update(1, true))
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM twitch_partner_profiles")
                .fetch_one(&db.pool)
                .await
                .unwrap(),
            1
        );
        sqlx::query("UPDATE twitch_partners SET status='active',departnered_at=NULL,admin_archived_at=NULL,manual_partner_opt_out=0,raid_bot_enabled=1,technical_pause_reason=NULL WHERE twitch_user_id='1'").execute(&db.pool).await.unwrap();
    }
    assert_eq!(directory(&db.pool).await.unwrap().len(), 1);
    // Rename follows stable Twitch ID and invalidates the old URL.
    sqlx::query("UPDATE twitch_partners SET twitch_login='alice_new' WHERE twitch_user_id='1'")
        .execute(&db.pool)
        .await
        .unwrap();
    assert_eq!(
        page_handler(
            State(db.pool.clone()),
            Path("@alice".into()),
            Query(PageParams::default())
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    let renamed = page_handler(
        State(db.pool.clone()),
        Path("@alice_new".into()),
        Query(PageParams::default()),
    )
    .await;
    assert_eq!(renamed.status(), StatusCode::OK);
    assert!(body(renamed).await.contains("1 erfasste Streams"));
    db.close().await;
}
#[tokio::test]
async fn mounted_routes_and_csrf_are_enforced() {
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;
    let db = database().await;
    let router = crate::build_public_router(db.pool.clone());
    let response = router
        .oneshot(
            Request::builder()
                .uri("/streamer/@alice")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let router = crate::build_authed_router(
        db.pool.clone(),
        "test-only".into(),
        crate::RateLimiter::new(db.pool.clone(), "test-only".into()),
    );
    let response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/twitch/api/v2/streamer/profile")
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    db.close().await;
}
