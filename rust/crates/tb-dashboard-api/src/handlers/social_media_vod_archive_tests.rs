use super::*;

async fn database() -> crate::test_postgres::TestPostgres {
    let database = crate::test_postgres::TestPostgres::start().await;
    for sql in [
        "CREATE TABLE twitch_vod_archive_vods (id BIGINT PRIMARY KEY, twitch_id TEXT, streamer_login TEXT, twitch_user_id TEXT, title TEXT, duration_sec BIGINT, recorded_at DATE, discovered_at TIMESTAMPTZ DEFAULT NOW(), status TEXT, local_path TEXT, last_error TEXT, updated_at TIMESTAMPTZ DEFAULT NOW())",
        "CREATE TABLE twitch_vod_archive_parts (vod_id BIGINT, part_index INT, status TEXT, youtube_video_id TEXT, last_error TEXT, updated_at TIMESTAMPTZ DEFAULT NOW())",
        "CREATE TABLE social_media_partner_access (twitch_user_id TEXT PRIMARY KEY, granted BOOLEAN)",
        "CREATE TABLE social_media_platform_auth (id BIGSERIAL PRIMARY KEY, twitch_user_id TEXT, platform TEXT, access_token_enc BYTEA, refresh_token_enc BYTEA, scopes TEXT, token_expires_at TEXT, enabled INTEGER DEFAULT 1, needs_reauth BOOLEAN DEFAULT FALSE, refresh_expires_at TIMESTAMPTZ, authorized_at TIMESTAMPTZ DEFAULT NOW())",
        "INSERT INTO social_media_partner_access VALUES ('42', TRUE), ('99', TRUE)",
        "INSERT INTO twitch_vod_archive_vods (id, twitch_id, streamer_login, twitch_user_id, title, duration_sec, status) VALUES (1,'v1','renamed','42','Eigener Stream',3600,'upload_failed'), (2,'v2','other','99','Fremder Stream',1800,'uploaded')",
        "INSERT INTO twitch_vod_archive_parts VALUES (1,0,'failed',NULL,NULL,NOW()), (2,0,'done','video99',NULL,NOW())",
    ] { sqlx::query(sql).execute(&database.pool).await.unwrap(); }
    sqlx::raw_sql(include_str!(
        "../../../../migrations/20261007220000_vod_archive_management.sql"
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    database
}

fn partner() -> DashboardAuthLevel {
    DashboardAuthLevel::Partner {
        twitch_user_id: "42".into(),
        twitch_login: "old_name".into(),
        display_name: String::new(),
    }
}

async fn json(response: Response) -> Value {
    let bytes = axum::body::to_bytes(response.into_body(), 1_000_000)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn session_id_filters_list_and_forbids_foreign_actions() {
    let database = database().await;
    let pool = database.pool.clone();
    let response = list_handler(
        partner(),
        State(pool.clone()),
        Query(ArchiveQuery {
            twitch_user_id: None,
            page: None,
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let data = json(response).await;
    assert_eq!(data["items"].as_array().unwrap().len(), 1);
    assert_eq!(data["items"][0]["twitch_user_id"], "42");
    assert_eq!(data["items"][0]["needs_connection"], true);
    let denied = list_handler(
        partner(),
        State(pool.clone()),
        Query(ArchiveQuery {
            twitch_user_id: Some("99".into()),
            page: None,
        }),
    )
    .await;
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    let denied = action_handler(
        partner(),
        State(pool.clone()),
        Json(ArchiveAction {
            id: 2,
            action: "hide".into(),
            twitch_user_id: None,
        }),
    )
    .await;
    assert_eq!(denied.status(), StatusCode::NOT_FOUND);
    let admin = list_handler(
        DashboardAuthLevel::admin(),
        State(pool),
        Query(ArchiveQuery {
            twitch_user_id: None,
            page: None,
        }),
    )
    .await;
    assert_eq!(json(admin).await["items"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn retry_drive_hide_and_active_lock_keep_completed_parts() {
    let database = database().await;
    let pool = &database.pool;
    assert_eq!(
        apply_action(pool, 1, Some("99"), "retry").await.unwrap(),
        None
    );
    let mut guard = pool.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(186976768,1)")
        .execute(&mut *guard)
        .await
        .unwrap();
    assert_eq!(
        apply_action(pool, 1, Some("42"), "retry").await.unwrap(),
        Some(false)
    );
    guard.rollback().await.unwrap();
    assert_eq!(
        apply_action(pool, 1, Some("42"), "retry").await.unwrap(),
        Some(true)
    );
    let state: String = sqlx::query_scalar("SELECT status FROM twitch_vod_archive_vods WHERE id=1")
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(state, "new");
    assert_eq!(
        apply_action(pool, 1, Some("42"), "drive").await.unwrap(),
        Some(true)
    );
    assert!(sqlx::query_scalar::<_, bool>(
        "SELECT drive_requested FROM twitch_vod_archive_vods WHERE id=1"
    )
    .fetch_one(pool)
    .await
    .unwrap());
    assert_eq!(
        apply_action(pool, 2, None, "retry").await.unwrap(),
        Some(false)
    );
    assert_eq!(
        apply_action(pool, 1, Some("42"), "hide").await.unwrap(),
        Some(true)
    );
    let response = list_handler(
        partner(),
        State(pool.clone()),
        Query(ArchiveQuery {
            twitch_user_id: None,
            page: None,
        }),
    )
    .await;
    assert!(json(response).await["items"].as_array().unwrap().is_empty());
    let video: String =
        sqlx::query_scalar("SELECT youtube_video_id FROM twitch_vod_archive_parts WHERE vod_id=2")
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(video, "video99");
}

#[tokio::test]
async fn empty_pages_keep_visible_totals_within_the_authorized_scope() {
    let database = database().await;
    let pool = &database.pool;
    sqlx::query("INSERT INTO twitch_vod_archive_vods (id, twitch_id, streamer_login, twitch_user_id, title, duration_sec, status) SELECT id, 'v' || id, 'renamed', '42', 'Stream ' || id, 3600, 'new' FROM generate_series(3,52) id")
        .execute(pool)
        .await
        .unwrap();
    let response = list_handler(
        partner(),
        State(pool.clone()),
        Query(ArchiveQuery {
            twitch_user_id: None,
            page: Some(2),
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let data = json(response).await;
    assert_eq!(data["total"], 51);
    assert_eq!(data["page"], 2);
    assert_eq!(data["items"].as_array().unwrap().len(), 1);
    assert_eq!(data["items"][0]["id"], 1);
    let response = action_handler(
        partner(),
        State(pool.clone()),
        Json(ArchiveAction {
            id: 1,
            action: "hide".into(),
            twitch_user_id: None,
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    for (auth, scope, page, total) in [
        (partner(), None, 2, 50),
        (partner(), None, 100_000, 50),
        (DashboardAuthLevel::admin(), Some("42"), 2, 50),
        (DashboardAuthLevel::admin(), Some("99"), 2, 1),
        (DashboardAuthLevel::admin(), None, 3, 51),
        (DashboardAuthLevel::admin(), Some("101"), 1, 0),
    ] {
        let response = list_handler(
            auth,
            State(pool.clone()),
            Query(ArchiveQuery {
                twitch_user_id: scope.map(str::to_string),
                page: Some(page),
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let data = json(response).await;
        assert_eq!(data["total"], total);
        assert_eq!(data["page"], page);
        assert!(data["items"].as_array().unwrap().is_empty());
    }
    let response = list_handler(
        partner(),
        State(pool.clone()),
        Query(ArchiveQuery {
            twitch_user_id: None,
            page: Some(1),
        }),
    )
    .await;
    let data = json(response).await;
    assert_eq!(data["total"], 50);
    let items = data["items"].as_array().unwrap();
    assert_eq!(items.len(), 50);
    assert!(items.iter().all(|item| item["twitch_user_id"] == "42"));
    assert_eq!(items[0]["id"], 52);
    assert_eq!(items[49]["id"], 3);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM twitch_vod_archive_parts WHERE vod_id=1"
        )
        .fetch_one(pool)
        .await
        .unwrap(),
        1
    );
}

#[test]
fn incomplete_archived_records_are_not_reported_as_clean_uploads() {
    assert_eq!(
        status_label(
            "archived",
            &json!([{"status":"pending", "youtube_video_id":null}])
        ),
        status_label("upload_failed", &json!([]))
    );
    assert_ne!(
        status_label(
            "archived",
            &json!([{"status":"done", "youtube_video_id":"video"}])
        ),
        status_label("upload_failed", &json!([]))
    );
}
