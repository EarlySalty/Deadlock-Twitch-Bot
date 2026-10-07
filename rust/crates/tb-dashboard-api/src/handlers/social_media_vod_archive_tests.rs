use super::*;

async fn database() -> crate::test_postgres::TestPostgres {
    let database = crate::test_postgres::TestPostgres::start().await;
    for sql in [
        "CREATE TABLE twitch_vod_archive_vods (id BIGINT PRIMARY KEY, twitch_id TEXT, streamer_login TEXT, twitch_user_id TEXT, title TEXT, duration_sec BIGINT, recorded_at DATE, discovered_at TIMESTAMPTZ DEFAULT NOW(), status TEXT, local_path TEXT, last_error TEXT, uploaded_at TIMESTAMPTZ, updated_at TIMESTAMPTZ DEFAULT NOW())",
        "CREATE TABLE twitch_vod_archive_parts (vod_id BIGINT, part_index INT, status TEXT, youtube_video_id TEXT, last_error TEXT, updated_at TIMESTAMPTZ DEFAULT NOW())",
        "CREATE TABLE social_media_partner_access (twitch_user_id TEXT PRIMARY KEY, granted BOOLEAN)",
        "CREATE TABLE social_media_platform_auth (id BIGSERIAL PRIMARY KEY, twitch_user_id TEXT, platform TEXT, access_token_enc BYTEA, refresh_token_enc BYTEA, scopes TEXT, token_expires_at TEXT, enabled INTEGER DEFAULT 1, needs_reauth BOOLEAN DEFAULT FALSE, refresh_expires_at TIMESTAMPTZ, authorized_at TIMESTAMPTZ DEFAULT NOW())",
        "INSERT INTO social_media_partner_access VALUES ('42', TRUE), ('99', TRUE)",
        "INSERT INTO twitch_vod_archive_vods (id, twitch_id, streamer_login, twitch_user_id, title, duration_sec, status) VALUES (1,'v1','renamed','42','Eigener Stream',3600,'upload_failed'), (2,'v2','other','99','Fremder Stream',1800,'uploaded')",
        "INSERT INTO twitch_vod_archive_parts VALUES (1,0,'failed',NULL,NULL,NOW()), (2,0,'done','video99',NULL,NOW())",
    ] {
        sqlx::query(sql).execute(&database.pool).await.unwrap();
    }
    sqlx::raw_sql(include_str!(
        "../../../../migrations/20261007220000_vod_archive_management.sql"
    ))
    .execute(&database.pool)
    .await
    .unwrap();
    sqlx::query("ALTER TABLE twitch_vod_archive_parts ADD COLUMN id BIGSERIAL")
        .execute(&database.pool)
        .await
        .unwrap();
    sqlx::query("ALTER TABLE social_media_platform_auth ADD COLUMN platform_user_id TEXT")
        .execute(&database.pool)
        .await
        .unwrap();
    sqlx::raw_sql(include_str!(
        "../../../../migrations/20261008003000_vod_youtube_checks.sql"
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

#[tokio::test]
async fn youtube_check_is_scoped_debounced_read_only_and_visible() {
    let database = database().await;
    let pool = &database.pool;
    sqlx::query("INSERT INTO social_media_platform_auth(twitch_user_id,platform,platform_user_id,access_token_enc,scopes) VALUES ('99','youtube','own',decode('01','hex'),'https://www.googleapis.com/auth/youtube.readonly')").execute(pool).await.unwrap();
    assert_eq!(
        apply_action(pool, 2, Some("42"), "check").await.unwrap(),
        None
    );
    assert_eq!(
        apply_action(pool, 2, Some("99"), "check").await.unwrap(),
        Some(true)
    );
    assert_eq!(
        apply_action(pool, 2, Some("99"), "check").await.unwrap(),
        Some(false)
    );
    let before: Value=sqlx::query_scalar("SELECT jsonb_build_array(v.status,v.uploaded_at,(SELECT jsonb_agg(p) FROM twitch_vod_archive_parts p WHERE p.vod_id=v.id)) FROM twitch_vod_archive_vods v WHERE v.id=2").fetch_one(pool).await.unwrap();
    let observations = json!([{"video_id":"video99","part_index":0,"part_total":1,"state":"processed","privacy":"private","observed_at":"2026-10-08T01:00:00Z"}]);
    sqlx::query("UPDATE twitch_vod_youtube_checks SET state='confirmed',complete=TRUE,observations=$1,requested_at=NULL,last_attempt_at=NOW(),last_success_at=NOW(),channel_id='own' WHERE vod_id=2").bind(observations).execute(pool).await.unwrap();
    assert_eq!(
        apply_action(pool, 2, Some("99"), "check").await.unwrap(),
        Some(false)
    );
    let data = json(
        list_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Query(ArchiveQuery {
                twitch_user_id: Some("99".into()),
                page: None,
            }),
        )
        .await,
    )
    .await;
    let item = &data["items"][0];
    assert_eq!(item["display_status"], "youtube_confirmed");
    assert_eq!(item["youtube_verified_complete"], true);
    assert_eq!(
        item["youtube_check"]["observations"][0]["privacy"],
        "private"
    );
    assert!(item["youtube_check"]["last_success_at"].is_string());
    if let Ok(path) = std::env::var("VOD_ARCHIVE_PROOF_PATH") {
        let path = std::path::Path::new(&path).with_file_name("youtube-current.json");
        std::fs::write(path, serde_json::to_vec_pretty(&data).unwrap()).unwrap();
    }
    sqlx::query("UPDATE twitch_vod_youtube_checks SET state='partial',complete=FALSE,observations=jsonb_set(observations,'{0,part_total}','2'::jsonb) WHERE vod_id=2")
        .execute(pool).await.unwrap();
    let partial = json(
        list_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Query(ArchiveQuery {
                twitch_user_id: Some("99".into()),
                page: None,
            }),
        )
        .await,
    )
    .await;
    assert_eq!(partial["items"][0]["display_status"], "partial");
    assert_eq!(partial["items"][0]["youtube_verified_complete"], false);
    assert_eq!(
        partial["items"][0]["youtube_check"]["observations"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    if let Ok(path) = std::env::var("VOD_ARCHIVE_PROOF_PATH") {
        let path = std::path::Path::new(&path).with_file_name("youtube-partial.json");
        std::fs::write(path, serde_json::to_vec_pretty(&partial).unwrap()).unwrap();
    }
    sqlx::query("UPDATE twitch_vod_youtube_checks SET state='error',last_error='connection',last_attempt_at=NOW() WHERE vod_id=2")
        .execute(pool).await.unwrap();
    let connection = json(
        list_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Query(ArchiveQuery {
                twitch_user_id: Some("99".into()),
                page: None,
            }),
        )
        .await,
    )
    .await;
    assert_eq!(connection["items"][0]["display_status"], "youtube_error");
    assert_eq!(
        connection["items"][0]["youtube_check"]["error"],
        "connection"
    );
    assert_eq!(
        connection["items"][0]["youtube_check"]["observations"],
        partial["items"][0]["youtube_check"]["observations"]
    );
    assert_eq!(
        connection["items"][0]["youtube_check"]["last_success_at"],
        data["items"][0]["youtube_check"]["last_success_at"]
    );
    if let Ok(path) = std::env::var("VOD_ARCHIVE_PROOF_PATH") {
        let path = std::path::Path::new(&path).with_file_name("youtube-connection.json");
        std::fs::write(path, serde_json::to_vec_pretty(&connection).unwrap()).unwrap();
    }
    let after:Value=sqlx::query_scalar("SELECT jsonb_build_array(v.status,v.uploaded_at,(SELECT jsonb_agg(p) FROM twitch_vod_archive_parts p WHERE p.vod_id=v.id)) FROM twitch_vod_archive_vods v WHERE v.id=2").fetch_one(pool).await.unwrap();
    assert_eq!(before, after);
    sqlx::query("UPDATE social_media_platform_auth SET platform_user_id='changed' WHERE twitch_user_id='99'").execute(pool).await.unwrap();
    let changed = json(
        list_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Query(ArchiveQuery {
                twitch_user_id: Some("99".into()),
                page: None,
            }),
        )
        .await,
    )
    .await;
    assert!(changed["items"][0]["youtube_check"].is_null());
    eprintln!("YOUTUBE_API_DB_PROOF: queue write debounced and session-scoped, current private evidence returned, historical upload untouched, switched channel hides old evidence");
}

#[tokio::test]
async fn youtube_evidence_keeps_drive_success_and_upload_recovery_visible() {
    let database = database().await;
    let pool = &database.pool;
    sqlx::query("INSERT INTO social_media_platform_auth(twitch_user_id,platform,platform_user_id,access_token_enc,scopes) VALUES ('42','youtube','own',decode('01','hex'),'https://www.googleapis.com/auth/youtube.readonly')")
        .execute(pool).await.unwrap();
    sqlx::query(
        "UPDATE twitch_vod_archive_parts SET youtube_video_id='synthetic-existing' WHERE vod_id=1",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO twitch_vod_archive_vods (id,twitch_id,streamer_login,twitch_user_id,title,duration_sec,status,uploaded_at,drive_url) VALUES (3,'v3','synthetic','42','Drive-Kopie',120,'drive_uploaded','2026-10-01T13:00:00Z','https://drive.google.com/drive/folders/synthetic')")
        .execute(pool).await.unwrap();
    sqlx::query("INSERT INTO twitch_vod_archive_parts(vod_id,part_index,status,youtube_video_id) VALUES (3,0,'pending','synthetic-drive')")
        .execute(pool).await.unwrap();
    for id in [1, 3] {
        assert_eq!(
            apply_action(pool, id, Some("42"), "check").await.unwrap(),
            Some(true)
        );
    }
    for state in ["error", "partial", "processing", "rejected", "confirmed"] {
        sqlx::query("UPDATE twitch_vod_youtube_checks SET state=$1,complete=$1='confirmed',requested_at=NULL,last_error=CASE WHEN $1='error' THEN 'connection' END")
            .bind(state).execute(pool).await.unwrap();
        for drive_requested in [false, true] {
            sqlx::query("UPDATE twitch_vod_archive_vods SET drive_requested=$1 WHERE id=1")
                .bind(drive_requested)
                .execute(pool)
                .await
                .unwrap();
            let data = json(
                list_handler(
                    partner(),
                    State(pool.clone()),
                    Query(ArchiveQuery {
                        twitch_user_id: None,
                        page: None,
                    }),
                )
                .await,
            )
            .await;
            let items = data["items"].as_array().unwrap();
            let drive = items.iter().find(|item| item["id"] == 3).unwrap();
            assert_eq!(drive["display_status"], "drive_uploaded");
            assert_eq!(drive["drive_complete"], true);
            assert_eq!(drive["youtube_check"]["state"], state);
            let recovery = items.iter().find(|item| item["id"] == 1).unwrap();
            assert_eq!(
                recovery["reason"],
                json!(error_label("upload_failed", None, drive_requested))
            );
            assert_eq!(recovery["can_retry"], true);
            assert_eq!(recovery["youtube_check"]["state"], state);
        }
    }
    eprintln!("YOUTUBE_API_DB_PROOF: all YouTube states preserve independent Drive success and relevant recovery explanations for YouTube and Drive uploads");
}

#[test]
fn progress_requires_confirmation_and_keeps_targets_separate() {
    for parts in [
        json!([]),
        json!([{"status":"pending", "youtube_video_id":null}]),
        json!([{"status":"pending", "youtube_video_id":"link-only"}]),
        json!([{"status":"done", "youtube_video_id":""}]),
        json!([{"status":"done", "youtube_video_id":"video"}, {"status":"pending", "youtube_video_id":null}]),
    ] {
        let progress = archive_progress("archived", &parts, None, true);
        assert_eq!(progress.state, "unknown");
        assert!(!progress.youtube_complete && !progress.drive_complete && !progress.can_retry);
    }
    let parts = json!([{"status":"done", "youtube_video_id":"video"}]);
    for has_time in [false, true] {
        let progress = archive_progress("archived", &parts, None, has_time);
        assert_eq!(progress.state, "youtube_uploaded");
        assert!(progress.youtube_complete && !progress.drive_complete);
    }
    let progress = archive_progress("upload_failed", &parts, None, false);
    assert_eq!(progress.state, "partial");
    assert!(!progress.youtube_complete && progress.can_retry);
    let progress = archive_progress(
        "drive_uploaded",
        &json!([]),
        Some("https://drive.google.com/drive/folders/synthetic"),
        true,
    );
    assert!(progress.drive_complete && !progress.youtube_complete);
    assert_eq!(progress.confirmed_parts, 0);
    let progress = archive_progress("drive_uploaded", &parts, None, true);
    assert_eq!(progress.state, "unknown");
    let progress = archive_progress("future_state", &json!([]), None, false);
    assert_eq!(progress.state, "unknown");
    let progress = archive_progress(
        "downloaded",
        &json!([{"status":"uploading", "youtube_video_id":null}]),
        None,
        false,
    );
    assert_eq!(progress.state, "waiting");
}

#[tokio::test]
async fn list_exposes_confirmed_historical_partial_failed_and_unknown_states() {
    let database = database().await;
    let pool = &database.pool;
    sqlx::query("INSERT INTO twitch_vod_archive_vods (id,twitch_id,streamer_login,twitch_user_id,title,duration_sec,status,uploaded_at,last_attempt_at,drive_url) VALUES \
        (3,'v3','testkanal','42','Bestätigter älterer Upload',3600,'archived','2026-09-01T12:00:00Z',NULL,NULL), \
        (4,'v4','testkanal','42','Teilweise hochgeladen',50000,'upload_failed',NULL,'2026-10-01T13:00:00Z',NULL), \
        (5,'v5','testkanal','42','Unklarer älterer Abschluss',3600,'archived',NULL,NULL,NULL), \
        (6,'v6','testkanal','42','Bestätigte Drive-Kopie',3600,'drive_uploaded','2026-10-01T13:00:00Z',NULL,'https://drive.google.com/drive/folders/synthetic'), \
        (7,'v7','testkanal','42','Upload läuft',3600,'uploading',NULL,'2026-10-01T13:00:00Z',NULL), \
        (8,'v8','testkanal','42','Wartet auf Upload',3600,'downloaded',NULL,NULL,NULL), \
        (9,'v9','testkanal','42','Älterer Upload ohne Abschlusszeit',3600,'archived',NULL,NULL,NULL)")
        .execute(pool).await.unwrap();
    sqlx::query(
        "INSERT INTO twitch_vod_archive_parts VALUES \
        (3,0,'done','synthetic3',NULL,NOW()), \
        (4,0,'done','synthetic4',NULL,NOW()),(4,1,'failed',NULL,'synthetic failure',NOW()), \
        (5,0,'pending','link-only',NULL,NOW()), \
        (6,0,'pending',NULL,NULL,NOW()), \
        (7,0,'uploading',NULL,NULL,NOW()), \
        (8,0,'pending',NULL,NULL,NOW()), \
        (9,0,'done','synthetic9',NULL,NOW())",
    )
    .execute(pool)
    .await
    .unwrap();
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
    let items = data["items"].as_array().unwrap();
    for (id, state, confirmed, youtube, drive) in [
        (1, "failed", 0, false, false),
        (3, "youtube_uploaded", 1, true, false),
        (4, "partial", 1, false, false),
        (5, "unknown", 0, false, false),
        (6, "drive_uploaded", 0, false, true),
        (7, "uploading", 0, false, false),
        (8, "waiting", 0, false, false),
        (9, "youtube_uploaded", 1, true, false),
    ] {
        let item = items.iter().find(|item| item["id"] == id).unwrap();
        assert_eq!(item["display_status"], state);
        assert_eq!(item["confirmed_parts"], confirmed);
        assert_eq!(item["youtube_complete"], youtube);
        assert_eq!(item["drive_complete"], drive);
    }
    let historical = items.iter().find(|item| item["id"] == 3).unwrap();
    assert!(historical["last_attempt_at"].is_null());
    assert!(historical["uploaded_at"].is_string());
    let no_time = items.iter().find(|item| item["id"] == 9).unwrap();
    assert!(no_time["uploaded_at"].is_null());
    if let Ok(path) = std::env::var("VOD_ARCHIVE_PROOF_PATH") {
        std::fs::write(path, serde_json::to_vec_pretty(&data).unwrap()).unwrap();
    }
}
