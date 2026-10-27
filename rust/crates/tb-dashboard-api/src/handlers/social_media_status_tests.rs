use tb_social_media::credentials::PLATFORMS as CONNECTION_PLATFORMS;
static STATUS_MASTER_KEY_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct StatusMasterKeyGuard {
    previous_key: Option<std::ffi::OsString>,
    _lock: tokio::sync::MutexGuard<'static, ()>,
}

impl StatusMasterKeyGuard {
    async fn new() -> Self {
        let lock = STATUS_MASTER_KEY_LOCK.lock().await;
        let guard = Self {
            previous_key: std::env::var_os("DB_MASTER_KEY_V1"),
            _lock: lock,
        };
        std::env::set_var("DB_MASTER_KEY_V1", "ab".repeat(32));
        guard
    }
}

impl Drop for StatusMasterKeyGuard {
    fn drop(&mut self) {
        match &self.previous_key {
            Some(key) => std::env::set_var("DB_MASTER_KEY_V1", key),
            None => std::env::remove_var("DB_MASTER_KEY_V1"),
        }
    }
}

#[tokio::test]
async fn status_master_key_guard_serializes_and_restores() {
    use std::future::{poll_fn, Future};
    use std::task::Poll;

    let first = StatusMasterKeyGuard::new().await;
    let previous_key = first.previous_key.clone();
    let mut second = std::pin::pin!(StatusMasterKeyGuard::new());
    poll_fn(|cx| {
        assert!(second.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    assert!(std::env::var_os("DB_MASTER_KEY_V1") == Some("ab".repeat(32).into()));
    drop(first);

    let second = second.await;
    assert!(second.previous_key == previous_key);
    assert!(std::env::var_os("DB_MASTER_KEY_V1") == Some("ab".repeat(32).into()));
    drop(second);

    let _lock = STATUS_MASTER_KEY_LOCK.lock().await;
    assert!(std::env::var_os("DB_MASTER_KEY_V1") == previous_key);
}

async fn make_platform_status_pool(schema: &str) -> PgPool {
    let pool = make_pool(schema).await.unwrap();
    sqlx::query(
        "ALTER TABLE social_media_platform_auth \
         ADD COLUMN token_expires_at TEXT, ADD COLUMN scopes TEXT, \
         ADD COLUMN platform_user_id TEXT, ADD COLUMN platform_username TEXT, \
         ADD COLUMN authorized_at TEXT, \
         ADD COLUMN refresh_expires_at TIMESTAMPTZ, \
         ADD COLUMN needs_reauth BOOLEAN NOT NULL DEFAULT FALSE",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool
}

async fn stored_platform_status_response(pool: &PgPool, twitch_user_id: Option<&str>) -> Response {
    platforms_status_handler(
        DashboardAuthLevel::admin(),
        State(pool.clone()),
        Query(StreamerQuery {
            twitch_user_id: twitch_user_id.map(str::to_string),
            streamer: twitch_user_id.is_none().then(|| GLOBAL_SCOPE_MARKER.into()),
        }),
    )
    .await
}

fn platform_status_fixture(platform: &str, scopes: Option<&str>) -> PlatformStatus {
    PlatformStatus {
        platform: platform.into(),
        connected: true,
        username: Some("synthetic-account".into()),
        user_id: Some("synthetic-id".into()),
        expires_at: Some("2026-10-11T12:00:00Z".into()),
        expired: false,
        refresh_expires_at: Some("2026-10-18T12:00:00Z".into()),
        needs_reauth: false,
        reauth_soon: true,
        automatically_renewed: true,
        scopes: scopes.map(str::to_string),
        uses_global_fallback: false,
    }
}

#[test]
fn platform_status_authorization_unknown_missing_and_sufficient() {
    for (scopes, expected, missing) in [
        (None, "unknown", Value::Null),
        (Some(""), "unknown", Value::Null),
        (Some(" \t, \n"), "unknown", Value::Null),
        (Some("[\"video.upload\""), "unknown", Value::Null),
        (Some("{\"scope\":\"video.upload\"}"), "unknown", Value::Null),
        (Some("video.upload\u{0000}"), "unknown", Value::Null),
        (Some("video.upload,\"unexpected\""), "unknown", Value::Null),
        (Some("video.publish"), "missing", json!(["video.upload"])),
        (
            Some("video.upload.extra"),
            "missing",
            json!(["video.upload"]),
        ),
        (Some("VIDEO.UPLOAD"), "missing", json!(["video.upload"])),
        (Some("video.upload"), "sufficient", json!([])),
        (
            Some("user.info.basic,video.publish,video.upload"),
            "sufficient",
            json!([]),
        ),
        (
            Some(" user.info.basic\tvideo.upload\nvideo.publish "),
            "sufficient",
            json!([]),
        ),
    ] {
        let value = platform_status_json(&platform_status_fixture("tiktok", scopes), true);
        assert_eq!(value["authorization"]["upload_scope_status"], expected);
        assert_eq!(value["authorization"]["missing_upload_scopes"], missing);
        assert_eq!(
            value["authorization"]["required_upload_scopes"],
            json!(["video.upload"])
        );
        assert_eq!(value["authorization"]["source"], "stored");
    }
}

#[test]
fn platform_status_youtube_accepts_upload_and_broad_equivalent_scopes() {
    for scope in ["youtube.upload", "youtube", "youtube.force-ssl"] {
        for scopes in [
            scope.to_string(),
            format!("https://www.googleapis.com/auth/{scope}"),
        ] {
            let value =
                platform_status_json(&platform_status_fixture("youtube", Some(&scopes)), true);
            assert_eq!(value["authorization"]["upload_scope_status"], "sufficient");
            assert_eq!(value["authorization"]["missing_upload_scopes"], json!([]));
            assert_eq!(
                value["authorization"]["required_upload_scopes"],
                json!(["youtube.upload"])
            );
        }
    }
    for scopes in [
        "youtube.readonly",
        "https://www.googleapis.com/auth/youtube.readonly",
        "https://example.invalid/auth/youtube.upload",
        "youtube.upload.extra",
    ] {
        let value = platform_status_json(&platform_status_fixture("youtube", Some(scopes)), true);
        assert_eq!(value["authorization"]["upload_scope_status"], "missing");
        assert_eq!(
            value["authorization"]["missing_upload_scopes"],
            json!(["youtube.upload"])
        );
    }
}

#[test]
fn platform_status_instagram_requires_both_business_scopes() {
    for (scopes, missing) in [
        (
            "instagram_basic,instagram_content_publish",
            json!([
                "instagram_business_basic",
                "instagram_business_content_publish"
            ]),
        ),
        (
            "instagram_business_basic",
            json!(["instagram_business_content_publish"]),
        ),
        (
            "instagram_business_content_publish",
            json!(["instagram_business_basic"]),
        ),
        (
            "instagram_business_basic,instagram_business_content_publish",
            json!([]),
        ),
    ] {
        let value = platform_status_json(&platform_status_fixture("instagram", Some(scopes)), true);
        assert_eq!(value["authorization"]["missing_upload_scopes"], missing);
        assert_eq!(
            value["authorization"]["upload_scope_status"],
            if missing.as_array().unwrap().is_empty() {
                "sufficient"
            } else {
                "missing"
            }
        );
    }
}

#[test]
fn platform_status_tiktok_fallback_requires_channel_account_without_changing_scope_result() {
    for platform in PLATFORMS {
        let mut status = platform_status_fixture(platform, Some("video.upload"));
        status.uses_global_fallback = true;
        for has_scope in [false, true] {
            let value = platform_status_json(&status, has_scope);
            assert_eq!(
                value["authorization"]["channel_account_required"],
                has_scope && platform == "tiktok"
            );
            assert_eq!(value["username"].is_null(), has_scope);
            assert_eq!(value["user_id"].is_null(), has_scope);
            assert_eq!(value["connected"], true);
            assert_eq!(value["uses_global_fallback"], true);
        }
        status.uses_global_fallback = false;
        let value = platform_status_json(&status, true);
        assert_eq!(value["authorization"]["channel_account_required"], false);
        assert_eq!(value["username"], "synthetic-account");
    }
}

#[test]
fn platform_status_keeps_authorization_separate_from_connection_expiry_and_capabilities() {
    let mut status = platform_status_fixture(
        "instagram",
        Some("instagram_business_basic,instagram_business_content_publish"),
    );
    status.connected = false;
    status.expired = true;
    status.needs_reauth = true;
    let previous_approval = std::env::var_os("INSTAGRAM_APP_APPROVED");
    std::env::remove_var("INSTAGRAM_APP_APPROVED");
    let value = platform_status_json(&status, true);
    if let Some(approval) = previous_approval {
        std::env::set_var("INSTAGRAM_APP_APPROVED", approval);
    }
    assert_eq!(value["capabilities"]["upload"], false);
    assert_eq!(value["connected"], false);
    assert_eq!(value["expired"], true);
    assert_eq!(value["needs_reauth"], true);
    assert_eq!(value["reauth_soon"], true);
    assert_eq!(value["automatically_renewed"], true);
    assert_eq!(value["expires_at"], json!(status.expires_at));
    assert_eq!(
        value["refresh_expires_at"],
        json!(status.refresh_expires_at)
    );
    assert_eq!(value["capabilities"]["upload_mode"], "reel");
    assert!(value["capabilities"]["reason"].is_string());
    assert_eq!(value["authorization"]["upload_scope_status"], "sufficient");
}

#[test]
fn platform_status_authorization_exposes_only_allowlisted_metadata() {
    let status = platform_status_fixture(
        "tiktok",
        Some("video.upload,arbitrary.stored.permission,synthetic-token-marker"),
    );
    let value = platform_status_json(&status, true);
    assert_eq!(
        value["authorization"],
        json!({
            "source": "stored",
            "upload_scope_status": "sufficient",
            "required_upload_scopes": ["video.upload"],
            "missing_upload_scopes": [],
            "channel_account_required": false,
        })
    );
    let serialized = value.to_string();
    assert!(!serialized.contains("arbitrary.stored.permission"));
    assert!(!serialized.contains("synthetic-token-marker"));
    for field in [
        "scopes",
        "granted_scopes",
        "access_token",
        "refresh_token",
        "client_secret",
        "approval",
        "publish_status",
    ] {
        assert!(value.get(field).is_none());
    }
}

#[tokio::test]
async fn platforms_status_stored_scopes_keep_disabled_connections_and_channel_fallback_distinct() {
    let pool = make_platform_status_pool("t_dash_sm_stored_authorization").await;
    let _key_guard = StatusMasterKeyGuard::new().await;
    let cipher = FieldCipher::from_env().unwrap();
    sqlx::query("INSERT INTO twitch_streamers (twitch_login, twitch_user_id) VALUES ('synthetic-channel', '42')")
        .execute(&pool).await.unwrap();
    for (platform, scopes, twitch_user_id) in [
        ("tiktok", "video.upload,arbitrary.permission", None),
        (
            "youtube",
            "https://www.googleapis.com/auth/youtube.upload",
            Some("42"),
        ),
        (
            "instagram",
            "instagram_business_basic,instagram_business_content_publish",
            Some("42"),
        ),
    ] {
        let encrypted = cipher
            .encrypt_field(
                "synthetic-status-token",
                &tb_crypto::aad::social_media("access_token", platform, None, 1),
            )
            .unwrap();
        sqlx::query("INSERT INTO social_media_platform_auth (platform, twitch_user_id, access_token_enc, enc_version, scopes, platform_user_id, platform_username, token_expires_at) VALUES ($1, $2, $3, 1, $4, 'synthetic-id', 'synthetic-account', '2099-01-01T00:00:00Z')")
            .bind(platform).bind(twitch_user_id).bind(encrypted).bind(scopes)
            .execute(&pool).await.unwrap();
    }
    let response = stored_platform_status_response(&pool, Some("42")).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    let platforms = body["platforms"].as_array().unwrap();
    assert_eq!(platforms.len(), CONNECTION_PLATFORMS.len());
    let platforms: Vec<_> = platforms
        .iter()
        .filter(|status| status["platform"] != "youtube_vod")
        .collect();
    assert_eq!(platforms.len(), 3);
    for status in platforms {
        assert_eq!(status["authorization"]["upload_scope_status"], "sufficient");
        assert_eq!(status["connected"], true);
        assert_eq!(status["expired"], false);
        let is_tiktok = status["platform"] == "tiktok";
        assert_eq!(
            status["authorization"]["channel_account_required"],
            is_tiktok
        );
        assert_eq!(status["username"].is_null(), is_tiktok);
        assert_eq!(status["uses_global_fallback"], is_tiktok);
    }
    assert!(!body.to_string().contains("synthetic-status-token"));
    assert!(!body.to_string().contains("arbitrary.permission"));
    let global = body_json(stored_platform_status_response(&pool, None).await).await;
    let tiktok = global["platforms"]
        .as_array()
        .unwrap()
        .iter()
        .find(|status| status["platform"] == "tiktok")
        .unwrap();
    assert_eq!(tiktok["authorization"]["channel_account_required"], false);
    assert_eq!(tiktok["username"], "synthetic-account");
    sqlx::query("UPDATE social_media_platform_auth SET enabled = 0 WHERE platform = 'youtube'")
        .execute(&pool)
        .await
        .unwrap();
    let body = body_json(stored_platform_status_response(&pool, Some("42")).await).await;
    let youtube = body["platforms"]
        .as_array()
        .unwrap()
        .iter()
        .find(|status| status["platform"] == "youtube")
        .unwrap();
    assert_eq!(youtube["connected"], false);
    assert_eq!(youtube["capabilities"]["upload"], true);
    assert_eq!(youtube["authorization"]["upload_scope_status"], "unknown");
    assert!(youtube["authorization"]["missing_upload_scopes"].is_null());
}

#[tokio::test]
async fn platforms_status_read_errors_return_500_not_disconnected() {
    let pool = make_platform_status_pool("t_dash_sm_status_errors").await;
    let _key_guard = StatusMasterKeyGuard::new().await;
    let cipher = FieldCipher::from_env().unwrap();

    let response = stored_platform_status_response(&pool, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    let platforms = body["platforms"].as_array().unwrap();
    assert_eq!(platforms.len(), CONNECTION_PLATFORMS.len());
    assert!(platforms.iter().all(|status| status["connected"] == false));
    assert!(platforms
        .iter()
        .all(|status| status["authorization"]["upload_scope_status"] == "unknown"));

    for platform in CONNECTION_PLATFORMS {
        let encrypted = cipher
            .encrypt_field(
                "local-access",
                &tb_crypto::aad::social_media("access_token", platform, None, 1),
            )
            .unwrap();
        sqlx::query(
            "INSERT INTO social_media_platform_auth (platform, access_token_enc, enc_version) VALUES ($1, $2, 1)",
        )
        .bind(platform)
        .bind(encrypted)
        .execute(&pool)
        .await
        .unwrap();
        let response = stored_platform_status_response(&pool, None).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = body_json(response).await;
        assert_eq!(
            body["platforms"]
                .as_array()
                .unwrap()
                .iter()
                .find(|status| status["platform"] == platform)
                .unwrap()["connected"],
            true
        );

        sqlx::query("ALTER TABLE social_media_platform_auth DROP COLUMN refresh_expires_at")
            .execute(&pool)
            .await
            .unwrap();
        assert!(build_credential_manager(pool.clone())
            .unwrap()
            .get_credentials(platform, None)
            .await
            .is_some());
        let response = stored_platform_status_response(&pool, None).await;
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = body_json(response).await;
        assert_eq!(body, json!({ "error": "platform_status_failed" }));
        sqlx::query(
            "ALTER TABLE social_media_platform_auth ADD COLUMN refresh_expires_at TIMESTAMPTZ",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("TRUNCATE social_media_platform_auth")
            .execute(&pool)
            .await
            .unwrap();
    }

    sqlx::query("DROP TABLE social_media_platform_auth")
        .execute(&pool)
        .await
        .unwrap();
    let response = stored_platform_status_response(&pool, None).await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        body_json(response).await,
        json!({ "error": "platform_status_failed" })
    );
}

#[tokio::test]
async fn platform_status_queue_read_failure_is_not_empty_publication_state() {
    let pool = make_pool("t_dash_sm_queue_read_failure").await.unwrap();
    let clip: i64 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, streamer_login) VALUES ('queue-read-failure', 'synthetic-channel') RETURNING id")
        .fetch_one(&pool).await.unwrap();
    assert!(load_upload_queue_info(&pool, &[clip])
        .await
        .unwrap()
        .is_empty());
    let row = load_clip_row(&pool, clip).await.unwrap().unwrap();
    let value = serialize_clip_record(&pool, &row).await.unwrap();
    assert!(value["upload_states"]["tiktok"].is_null());
    sqlx::query("DROP TABLE twitch_clips_upload_queue")
        .execute(&pool)
        .await
        .unwrap();
    assert!(load_upload_queue_info(&pool, &[clip]).await.is_err());
    assert_eq!(
        serialize_clip_record(&pool, &row)
            .await
            .unwrap_err()
            .status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
    let response = admin_clip_detail_handler(
        DashboardAuthLevel::admin(),
        State(pool.clone()),
        Path(clip.to_string()),
    )
    .await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        body_json(response).await,
        json!({"error": "clip_load_failed"})
    );
    let response = admin_clips_handler(
        DashboardAuthLevel::admin(),
        State(pool),
        Query(AdminClipsQuery {
            twitch_user_id: None,
            page: None,
            page_size: None,
            status: None,
            streamer: None,
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        body_json(response).await,
        json!({"error": "clip_load_failed"})
    );
}

#[tokio::test]
async fn platform_status_approval_read_failure_is_not_missing_approval() {
    let pool = make_pool("t_dash_sm_approval_read_failure").await.unwrap();
    let clip: i64 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, streamer_login) VALUES ('approval-read-failure', 'synthetic-channel') RETURNING id")
        .fetch_one(&pool).await.unwrap();
    assert!(optional_approval_json(&pool, clip).await.unwrap().is_null());
    sqlx::query("INSERT INTO social_media_clip_approval (clip_db_id, state, approved_platforms) VALUES ($1, 'approved', '[\" YouTube \",\"youtube\",\"unsupported\",\"tiktok\"]'::jsonb)")
        .bind(clip as i32).execute(&pool).await.unwrap();
    let record = optional_approval_json(&pool, clip).await.unwrap();
    assert_eq!(record["state"], "approved");
    assert_eq!(record["approved_platforms"], json!(["youtube", "tiktok"]));
    sqlx::query("DROP TABLE social_media_clip_approval")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        optional_approval_json(&pool, clip)
            .await
            .unwrap_err()
            .status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
    let response = approval_get_handler(
        DashboardAuthLevel::admin(),
        State(pool.clone()),
        Path(clip.to_string()),
    )
    .await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        body_json(response).await,
        json!({"error": "clip_load_failed"})
    );
    let response = admin_clip_detail_handler(
        DashboardAuthLevel::admin(),
        State(pool.clone()),
        Path(clip.to_string()),
    )
    .await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let response = admin_clips_handler(
        DashboardAuthLevel::admin(),
        State(pool),
        Query(AdminClipsQuery {
            twitch_user_id: None,
            page: None,
            page_size: None,
            status: None,
            streamer: None,
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn platform_status_approval_decode_failure_is_not_missing_approval() {
    let pool = make_pool("t_dash_sm_approval_decode_failure")
        .await
        .unwrap();
    let clip: i64 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, streamer_login) VALUES ('approval-decode-failure', 'synthetic-channel') RETURNING id")
        .fetch_one(&pool).await.unwrap();
    sqlx::query("INSERT INTO social_media_clip_approval (clip_db_id, approved_platforms) VALUES ($1, '{}'::jsonb)")
        .bind(clip as i32).execute(&pool).await.unwrap();
    assert_eq!(
        optional_approval_json(&pool, clip)
            .await
            .unwrap_err()
            .status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
    sqlx::query("ALTER TABLE social_media_clip_approval ALTER COLUMN approver_user_id TYPE INTEGER USING NULL")
        .execute(&pool).await.unwrap();
    sqlx::query("UPDATE social_media_clip_approval SET approved_platforms = '[]'::jsonb, approver_user_id = 1")
        .execute(&pool).await.unwrap();
    assert_eq!(
        optional_approval_json(&pool, clip)
            .await
            .unwrap_err()
            .status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
}

#[tokio::test]
async fn platform_status_queue_decode_failure_is_not_empty_publication_state() {
    let pool = make_pool("t_dash_sm_queue_decode_failure").await.unwrap();
    let clip: i64 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, streamer_login) VALUES ('queue-decode-failure', 'synthetic-channel') RETURNING id")
        .fetch_one(&pool).await.unwrap();
    sqlx::query("INSERT INTO twitch_clips_upload_queue (clip_id, platform, status) VALUES ($1, NULL, 'completed')")
        .bind(clip).execute(&pool).await.unwrap();
    assert!(load_upload_queue_info(&pool, &[clip]).await.is_err());
    sqlx::query("UPDATE twitch_clips_upload_queue SET platform = 'youtube'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("ALTER TABLE twitch_clips_upload_queue ALTER COLUMN youtube_visibility TYPE INTEGER USING NULL").execute(&pool).await.unwrap();
    sqlx::query("UPDATE twitch_clips_upload_queue SET youtube_visibility = 1")
        .execute(&pool)
        .await
        .unwrap();
    assert!(load_upload_queue_info(&pool, &[clip]).await.is_err());
}
