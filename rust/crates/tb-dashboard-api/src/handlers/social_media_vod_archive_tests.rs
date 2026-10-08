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
    sqlx::query("ALTER TABLE twitch_vod_archive_parts ADD COLUMN file_path TEXT, ADD COLUMN upload_session_uri TEXT, ADD COLUMN upload_offset BIGINT DEFAULT 0")
        .execute(&database.pool).await.unwrap();
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
            assert_eq!(recovery["can_retry"], state != "confirmed");
            assert_eq!(recovery["youtube_check"]["state"], state);
        }
    }
    eprintln!("YOUTUBE_API_DB_PROOF: all YouTube states preserve independent Drive success and relevant recovery explanations for YouTube and Drive uploads");
}

#[tokio::test]
async fn explicit_terminal_retry_is_scoped_and_preserves_successful_parts_and_history() {
    let database = database().await;
    let pool = &database.pool;
    let directory =
        std::env::temp_dir().join(format!("vod-recovery-rejected-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let file = directory.join("rejected.mp4");
    std::fs::write(&file, b"synthetic media").unwrap();
    sqlx::query("INSERT INTO social_media_platform_auth(twitch_user_id,platform,platform_user_id) VALUES ('42','youtube',NULL)").execute(pool).await.unwrap();
    sqlx::query("UPDATE twitch_vod_archive_vods SET status='uploaded',uploaded_at='2026-10-01T13:00:00Z' WHERE id=1").execute(pool).await.unwrap();
    sqlx::query("UPDATE twitch_vod_archive_parts SET status='done',youtube_video_id='rejected',file_path=$1 WHERE vod_id=1").bind(file.to_str().unwrap()).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO twitch_vod_archive_parts(vod_id,part_index,status,youtube_video_id,file_path,last_error) VALUES (1,1,'done','successful','/synthetic/absent-success','old history')").execute(pool).await.unwrap();
    let before:Value=sqlx::query_scalar("SELECT jsonb_build_array(to_jsonb(v),(SELECT jsonb_agg(to_jsonb(p) ORDER BY p.part_index) FROM twitch_vod_archive_parts p WHERE p.vod_id=v.id)) FROM twitch_vod_archive_vods v WHERE id=1").fetch_one(pool).await.unwrap();
    assert_eq!(
        apply_action(pool, 1, Some("42"), "check").await.unwrap(),
        Some(true)
    );
    sqlx::query("UPDATE twitch_vod_youtube_checks SET state='rejected',channel_id='own',requested_at=NULL,last_success_at=NOW(),observations=$1 WHERE vod_id=1").bind(json!([{"video_id":"rejected","part_index":0,"state":"rejected"},{"video_id":"successful","part_index":1,"state":"processed"}])).execute(pool).await.unwrap();
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
    assert_eq!(data["items"][0]["can_retry"], true);
    assert_eq!(data["items"][0]["can_drive"], false);
    assert!(data["items"][0]["parts"][0].get("file_path").is_none());
    let after:Value=sqlx::query_scalar("SELECT jsonb_build_array(to_jsonb(v),(SELECT jsonb_agg(to_jsonb(p) ORDER BY p.part_index) FROM twitch_vod_archive_parts p WHERE p.vod_id=v.id)) FROM twitch_vod_archive_vods v WHERE id=1").fetch_one(pool).await.unwrap();
    assert_eq!(before, after);
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
    let good_before: Value = sqlx::query_scalar(
        "SELECT to_jsonb(p) FROM twitch_vod_archive_parts p WHERE vod_id=1 AND part_index=1",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    let saved: Value =
        sqlx::query_scalar("SELECT to_jsonb(c) FROM twitch_vod_youtube_checks c WHERE vod_id=1")
            .fetch_one(pool)
            .await
            .unwrap();
    for mutation in [
        "auth_id=auth_id+100",
        "auth_revision='stale'",
        "channel_id=NULL",
        "upload_snapshot='[]'::jsonb",
    ] {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE twitch_vod_youtube_checks SET {mutation} WHERE vod_id=1"
        )))
        .execute(pool)
        .await
        .unwrap();
        for action in ["retry", "drive"] {
            assert_eq!(
                apply_action(pool, 1, Some("42"), action).await.unwrap(),
                Some(false)
            );
        }
        sqlx::query("UPDATE twitch_vod_youtube_checks SET auth_id=($1->>'auth_id')::integer,auth_revision=$1->>'auth_revision',channel_id=$1->>'channel_id',upload_snapshot=$1->'upload_snapshot' WHERE vod_id=1").bind(&saved).execute(pool).await.unwrap();
    }
    sqlx::query("UPDATE social_media_platform_auth SET platform_user_id='different' WHERE twitch_user_id='42'").execute(pool).await.unwrap();
    for action in ["retry", "drive"] {
        assert_eq!(
            apply_action(pool, 1, Some("42"), action).await.unwrap(),
            Some(false)
        );
    }
    sqlx::query(
        "UPDATE social_media_platform_auth SET platform_user_id=NULL WHERE twitch_user_id='42'",
    )
    .execute(pool)
    .await
    .unwrap();
    assert_eq!(
        apply_action(pool, 1, Some("42"), "retry").await.unwrap(),
        Some(true)
    );
    let state: (String, bool, String) = sqlx::query_as(
        "SELECT status,drive_requested,uploaded_at::text FROM twitch_vod_archive_vods WHERE id=1",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(state.0, "downloaded");
    assert!(!state.1);
    assert!(state.2.starts_with("2026-10-01"));
    let good_after: Value = sqlx::query_scalar(
        "SELECT to_jsonb(p) FROM twitch_vod_archive_parts p WHERE vod_id=1 AND part_index=1",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(good_before, good_after);
    let part:(String,String)=sqlx::query_as("SELECT status,youtube_video_id FROM twitch_vod_archive_parts WHERE vod_id=1 AND part_index=0").fetch_one(pool).await.unwrap();
    assert_eq!(part, ("pending".into(), "rejected".into()));
    assert!(file.exists());
    std::fs::remove_dir_all(directory).unwrap();
    eprintln!("YOUTUBE_API_DB_PROOF: read/check mutate no uploads; protected explicit rejected-part retry accepts local source, preserves successful part and original IDs/history, no provider operation");
}

#[tokio::test]
async fn unavailable_terminal_drive_recovery_never_resets_accepted_targets_or_independent_drive() {
    let database = database().await;
    let pool = &database.pool;
    let directory =
        std::env::temp_dir().join(format!("vod-recovery-drive-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let file = directory.join("source.mp4");
    std::fs::write(&file, b"synthetic media").unwrap();
    sqlx::query("INSERT INTO social_media_platform_auth(twitch_user_id,platform,platform_user_id) VALUES ('42','youtube',NULL)").execute(pool).await.unwrap();
    sqlx::query("UPDATE twitch_vod_archive_vods SET status='archived',uploaded_at='2026-10-01T13:00:00Z' WHERE id=1").execute(pool).await.unwrap();
    sqlx::query("UPDATE twitch_vod_archive_parts SET status='done',youtube_video_id='accepted',file_path=$1 WHERE vod_id=1").bind(file.to_str().unwrap()).execute(pool).await.unwrap();
    assert_eq!(
        apply_action(pool, 1, Some("42"), "check").await.unwrap(),
        Some(true)
    );
    sqlx::query("UPDATE twitch_vod_youtube_checks SET state='unavailable',channel_id='own',requested_at=NULL,last_success_at=NOW(),observations=$1 WHERE vod_id=1").bind(json!([{"video_id":"accepted","part_index":0,"state":"unavailable"}])).execute(pool).await.unwrap();
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
    assert_eq!(data["items"][0]["can_retry"], false);
    assert_eq!(data["items"][0]["can_drive"], true);
    let parts_before: Value = sqlx::query_scalar(
        "SELECT jsonb_agg(to_jsonb(p)) FROM twitch_vod_archive_parts p WHERE vod_id=1",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(
        apply_action(pool, 1, Some("42"), "retry").await.unwrap(),
        Some(false)
    );
    assert_eq!(
        apply_action(pool, 1, Some("42"), "drive").await.unwrap(),
        Some(true)
    );
    let parts_after: Value = sqlx::query_scalar(
        "SELECT jsonb_agg(to_jsonb(p)) FROM twitch_vod_archive_parts p WHERE vod_id=1",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(parts_before, parts_after);
    let state: (String, bool) =
        sqlx::query_as("SELECT status,drive_requested FROM twitch_vod_archive_vods WHERE id=1")
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(state, ("downloaded".into(), true));
    sqlx::query("UPDATE twitch_vod_archive_vods SET status='drive_uploaded',drive_url='https://drive.google.com/drive/folders/synthetic' WHERE id=1").execute(pool).await.unwrap();
    let saved: Value =
        sqlx::query_scalar("SELECT to_jsonb(v) FROM twitch_vod_archive_vods v WHERE id=1")
            .fetch_one(pool)
            .await
            .unwrap();
    for action in ["retry", "drive"] {
        assert_eq!(
            apply_action(pool, 1, Some("42"), action).await.unwrap(),
            Some(false)
        );
    }
    let preserved: Value =
        sqlx::query_scalar("SELECT to_jsonb(v) FROM twitch_vod_archive_vods v WHERE id=1")
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(saved, preserved);
    std::fs::remove_file(&file).unwrap();
    sqlx::query("UPDATE twitch_vod_archive_vods SET status='archived',drive_url=NULL WHERE id=1")
        .execute(pool)
        .await
        .unwrap();
    let absent = json(
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
    assert_eq!(absent["items"][0]["can_retry"], false);
    assert!(absent["items"][0]["reason"].is_string());
    for action in ["retry", "drive"] {
        assert_eq!(
            apply_action(pool, 1, Some("42"), action).await.unwrap(),
            Some(false)
        );
    }
    std::fs::remove_dir_all(directory).unwrap();
    eprintln!("YOUTUBE_API_DB_PROOF: unavailable does not imply deletion; existing Drive recovery can queue available local source without resetting accepted YouTube data; absent source and independent Drive success reject recovery honestly, no provider writes");
}

#[tokio::test]
async fn archive_actions_lock_auth_before_vod_and_parts_like_reconciliation_writers() {
    let database = database().await;
    let pool = &database.pool;
    sqlx::query("INSERT INTO social_media_platform_auth(twitch_user_id,platform,platform_user_id) VALUES ('42','youtube',NULL)").execute(pool).await.unwrap();
    sqlx::query("UPDATE twitch_vod_archive_vods SET status='uploaded' WHERE id=1")
        .execute(pool)
        .await
        .unwrap();
    for action in ["retry", "drive", "check"] {
        let mut writer = pool.begin().await.unwrap();
        let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *writer)
            .await
            .unwrap();
        sqlx::query(
            "SELECT id FROM social_media_platform_auth WHERE twitch_user_id='42' FOR UPDATE",
        )
        .fetch_all(&mut *writer)
        .await
        .unwrap();
        let action_pool = pool.clone();
        let operation =
            tokio::spawn(async move { apply_action(&action_pool, 1, Some("42"), action).await });
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let waiting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE pg_blocking_pids(pid) @> ARRAY[$1] AND query LIKE '%social_media_platform_auth%')").bind(pid).fetch_one(pool).await.unwrap();
                if waiting {
                    break;
                }
                tokio::task::yield_now().await;
            }
        }).await.expect("action must wait for authorization before touching archive rows");
        sqlx::query("SELECT id FROM twitch_vod_archive_vods WHERE id=1 FOR UPDATE NOWAIT")
            .fetch_one(&mut *writer)
            .await
            .unwrap();
        sqlx::query("SELECT id FROM twitch_vod_archive_parts WHERE vod_id=1 ORDER BY part_index FOR UPDATE NOWAIT").fetch_all(&mut *writer).await.unwrap();
        writer.commit().await.unwrap();
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(5), operation)
                .await
                .unwrap()
                .unwrap()
                .is_ok()
        );
    }
    eprintln!("YOUTUBE_API_DB_PROOF: retry, Drive and check wait on auth before VOD/parts; concurrent auth-first reconciliation transaction finishes without deadlock");
}

async fn save_recovery_check(pool: &PgPool, state: &str, complete: bool, observations: Value) {
    sqlx::query("INSERT INTO twitch_vod_youtube_checks(vod_id,auth_id,auth_revision,channel_id,state,complete,observations,upload_snapshot) SELECT 1,a.id,md5(COALESCE(a.refresh_token_enc::text,'') || COALESCE(a.platform_user_id,'') || COALESCE(a.authorized_at::text,'')),'own',$1,$2,$3,(SELECT jsonb_agg(jsonb_build_object('id',p.id,'index',p.part_index,'status',p.status,'video_id',p.youtube_video_id,'updated_at',p.updated_at) ORDER BY p.part_index) FROM twitch_vod_archive_parts p WHERE p.vod_id=1) FROM social_media_platform_auth a WHERE a.twitch_user_id='42' AND a.platform='youtube' ON CONFLICT(vod_id) DO UPDATE SET state=EXCLUDED.state,complete=EXCLUDED.complete,observations=EXCLUDED.observations,upload_snapshot=EXCLUDED.upload_snapshot")
        .bind(state).bind(complete).bind(observations).execute(pool).await.unwrap();
}

async fn recovery_item(pool: &PgPool) -> Value {
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
    data["items"][0].clone()
}

#[tokio::test]
async fn complete_current_proof_blocks_both_retry_branches_and_preserves_drive_only_parts() {
    let database = database().await;
    let pool = &database.pool;
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("source.mp4");
    std::fs::write(&file, b"synthetic media").unwrap();
    sqlx::query("INSERT INTO social_media_platform_auth(twitch_user_id,platform,platform_user_id) VALUES ('42','youtube','own')").execute(pool).await.unwrap();
    for status in ["uploaded", "archived", "upload_failed", "downloaded", "new"] {
        for part_status in ["rejected", "failed", "pending"] {
            sqlx::query("UPDATE twitch_vod_archive_vods SET status=$1,local_path=$2,uploaded_at='2026-10-01T13:00:00Z',drive_requested=FALSE WHERE id=1").bind(status).bind(file.to_str().unwrap()).execute(pool).await.unwrap();
            sqlx::query("UPDATE twitch_vod_archive_parts SET status=$1,youtube_video_id=NULL,file_path=$2,last_error='historical',upload_session_uri=NULL,upload_offset=7 WHERE vod_id=1").bind(part_status).bind(file.to_str().unwrap()).execute(pool).await.unwrap();
            save_recovery_check(
                pool,
                "confirmed",
                true,
                json!([{"video_id":"found","part_index":0,"state":"processed"}]),
            )
            .await;
            let before: Value = sqlx::query_scalar("SELECT jsonb_build_array(to_jsonb(v),(SELECT jsonb_agg(to_jsonb(p)) FROM twitch_vod_archive_parts p WHERE p.vod_id=v.id)) FROM twitch_vod_archive_vods v WHERE id=1").fetch_one(pool).await.unwrap();
            let item = recovery_item(pool).await;
            assert_eq!(item["youtube_verified_complete"], true);
            assert_eq!(item["can_retry"], false, "{status}/{part_status}");
            let response = action_handler(
                partner(),
                State(pool.clone()),
                Json(ArchiveAction {
                    id: 1,
                    action: "retry".into(),
                    twitch_user_id: None,
                }),
            )
            .await;
            assert_eq!(response.status(), StatusCode::CONFLICT);
            let after: Value = sqlx::query_scalar("SELECT jsonb_build_array(to_jsonb(v),(SELECT jsonb_agg(to_jsonb(p)) FROM twitch_vod_archive_parts p WHERE p.vod_id=v.id)) FROM twitch_vod_archive_vods v WHERE id=1").fetch_one(pool).await.unwrap();
            assert_eq!(before, after);
            if item["can_drive"] == true {
                let parts_before: Value = sqlx::query_scalar(
                    "SELECT jsonb_agg(to_jsonb(p)) FROM twitch_vod_archive_parts p WHERE vod_id=1",
                )
                .fetch_one(pool)
                .await
                .unwrap();
                assert_eq!(
                    apply_action(pool, 1, Some("42"), "drive").await.unwrap(),
                    Some(true)
                );
                let parts_after: Value = sqlx::query_scalar(
                    "SELECT jsonb_agg(to_jsonb(p)) FROM twitch_vod_archive_parts p WHERE vod_id=1",
                )
                .fetch_one(pool)
                .await
                .unwrap();
                assert_eq!(parts_before, parts_after);
            }
        }
    }
    eprintln!("YOUTUBE_API_DB_PROOF: current complete proof rejects GET/POST retries for terminal and ordinary recovery despite legacy rejected/failed/pending flags and absent IDs; Drive actions preserve all part rows");
}

#[tokio::test]
async fn partial_processed_proof_excludes_exact_parts_from_both_resets_and_repeated_recovery() {
    let database = database().await;
    let pool = &database.pool;
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("source.mp4");
    std::fs::write(&file, b"synthetic media").unwrap();
    sqlx::query("INSERT INTO social_media_platform_auth(twitch_user_id,platform,platform_user_id) VALUES ('42','youtube','own')").execute(pool).await.unwrap();
    for status in ["uploaded", "upload_failed"] {
        sqlx::query("UPDATE twitch_vod_archive_vods SET status=$1,local_path=$2,drive_requested=FALSE WHERE id=1").bind(status).bind(file.to_str().unwrap()).execute(pool).await.unwrap();
        sqlx::query("DELETE FROM twitch_vod_archive_parts WHERE vod_id=1")
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO twitch_vod_archive_parts(vod_id,part_index,status,youtube_video_id,file_path,last_error,upload_offset) VALUES (1,0,'rejected','found0',$1,'history0',7),(1,1,'failed',NULL,$1,'history1',7),(1,2,'pending',NULL,$1,'history2',7),(1,3,'rejected','bad3',$1,'failure3',7),(1,4,'failed',NULL,$1,'failure4',7)").bind(file.to_str().unwrap()).execute(pool).await.unwrap();
        save_recovery_check(
            pool,
            "rejected",
            false,
            json!([
                {"video_id":"found0","part_index":0,"state":"processed"},
                {"video_id":"found1","part_index":1,"state":"processed"},
                {"video_id":"found2","part_index":2,"state":"processed"},
                {"video_id":"bad3","part_index":3,"state":"rejected"},
                {"video_id":"unrelated","part_index":9,"state":"processed"}
            ]),
        )
        .await;
        let saved: Value = sqlx::query_scalar(
            "SELECT to_jsonb(c) FROM twitch_vod_youtube_checks c WHERE vod_id=1",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        let protected: Value = sqlx::query_scalar("SELECT jsonb_agg(to_jsonb(p) ORDER BY part_index) FROM twitch_vod_archive_parts p WHERE vod_id=1 AND part_index<3").fetch_one(pool).await.unwrap();
        let item = recovery_item(pool).await;
        assert_eq!(item["can_retry"], true);
        assert_eq!(item["youtube_verified_complete"], false);
        for _ in 0..2 {
            let response = action_handler(
                partner(),
                State(pool.clone()),
                Json(ArchiveAction {
                    id: 1,
                    action: "retry".into(),
                    twitch_user_id: None,
                }),
            )
            .await;
            assert_eq!(response.status(), StatusCode::OK);
            let preserved: Value = sqlx::query_scalar("SELECT jsonb_agg(to_jsonb(p) ORDER BY part_index) FROM twitch_vod_archive_parts p WHERE vod_id=1 AND part_index<3").fetch_one(pool).await.unwrap();
            assert_eq!(protected, preserved);
            let reset: Vec<(i32,String)> = sqlx::query_as("SELECT part_index,status FROM twitch_vod_archive_parts WHERE vod_id=1 AND part_index>=3 ORDER BY part_index").fetch_all(pool).await.unwrap();
            assert_eq!(reset, vec![(3, "pending".into()), (4, "pending".into())]);
            let proof: Value = sqlx::query_scalar(
                "SELECT to_jsonb(c) FROM twitch_vod_youtube_checks c WHERE vod_id=1",
            )
            .fetch_one(pool)
            .await
            .unwrap();
            assert_eq!(saved, proof);
            let item = recovery_item(pool).await;
            assert!(item["youtube_check"].is_null());
            assert_eq!(item["can_retry"], true);
        }
        let before: Value = sqlx::query_scalar("SELECT jsonb_agg(to_jsonb(p) ORDER BY part_index) FROM twitch_vod_archive_parts p WHERE vod_id=1").fetch_one(pool).await.unwrap();
        assert_eq!(
            apply_action(pool, 1, Some("42"), "drive").await.unwrap(),
            Some(true)
        );
        let after: Value = sqlx::query_scalar("SELECT jsonb_agg(to_jsonb(p) ORDER BY part_index) FROM twitch_vod_archive_parts p WHERE vod_id=1").fetch_one(pool).await.unwrap();
        assert_eq!(before, after);
    }
    eprintln!("YOUTUBE_API_DB_PROOF: both reset branches and repeated retries preserve exactly the unchanged processed known-ID and ID-less rejected/failed/pending parts; genuinely unconfirmed targets reset; original evidence and Drive-only part rows remain untouched");
}

#[tokio::test]
async fn recovery_get_and_post_reject_stale_binding_as_confirmation() {
    let database = database().await;
    let pool = &database.pool;
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("source.mp4");
    std::fs::write(&file, b"synthetic media").unwrap();
    sqlx::query("INSERT INTO social_media_platform_auth(twitch_user_id,platform,platform_user_id) VALUES ('42','youtube','own')").execute(pool).await.unwrap();
    for mutation in [
        "auth_id=auth_id+100",
        "auth_revision='stale'",
        "channel_id=NULL",
        "channel_id='different'",
        "upload_snapshot='[]'::jsonb",
    ] {
        sqlx::query(
            "UPDATE twitch_vod_archive_vods SET status='uploaded',local_path=$1 WHERE id=1",
        )
        .bind(file.to_str().unwrap())
        .execute(pool)
        .await
        .unwrap();
        sqlx::query("UPDATE twitch_vod_archive_parts SET status='rejected',youtube_video_id=NULL,file_path=$1 WHERE vod_id=1").bind(file.to_str().unwrap()).execute(pool).await.unwrap();
        sqlx::query("DELETE FROM twitch_vod_youtube_checks WHERE vod_id=1")
            .execute(pool)
            .await
            .unwrap();
        save_recovery_check(
            pool,
            "confirmed",
            true,
            json!([{"video_id":"found","part_index":0,"state":"processed"}]),
        )
        .await;
        let item = recovery_item(pool).await;
        assert_eq!(item["can_retry"], false);
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "UPDATE twitch_vod_youtube_checks SET {mutation} WHERE vod_id=1"
        )))
        .execute(pool)
        .await
        .unwrap();
        let item = recovery_item(pool).await;
        assert!(item["youtube_check"].is_null(), "{mutation}");
        assert_eq!(item["youtube_verified_complete"], false);
        assert_eq!(item["can_retry"], true, "{mutation}");
        let response = action_handler(
            partner(),
            State(pool.clone()),
            Json(ArchiveAction {
                id: 1,
                action: "retry".into(),
                twitch_user_id: None,
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let part_status: String =
            sqlx::query_scalar("SELECT status FROM twitch_vod_archive_parts WHERE vod_id=1")
                .fetch_one(pool)
                .await
                .unwrap();
        assert_eq!(part_status, "pending");
    }
    eprintln!("YOUTUBE_API_DB_PROOF: GET and POST apply identical auth-ID, revision, known/legacy channel and snapshot bindings; stale proof cannot suppress genuinely unconfirmed recovery");
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
