#[tokio::test]
async fn platforms_status_read_errors_return_500_not_disconnected() {
    struct MasterKeyGuard(Option<std::ffi::OsString>);
    impl Drop for MasterKeyGuard {
        fn drop(&mut self) {
            match &self.0 {
                Some(key) => std::env::set_var("DB_MASTER_KEY_V1", key),
                None => std::env::remove_var("DB_MASTER_KEY_V1"),
            }
        }
    }

    let pool = make_pool("t_dash_sm_status_errors").await.unwrap();
    let _key_guard = MasterKeyGuard(std::env::var_os("DB_MASTER_KEY_V1"));
    std::env::set_var("DB_MASTER_KEY_V1", "ab".repeat(32));
    let cipher = FieldCipher::from_env().unwrap();
    sqlx::query(
        "ALTER TABLE social_media_platform_auth \
         ADD COLUMN access_token_enc BYTEA, ADD COLUMN refresh_token_enc BYTEA, \
         ADD COLUMN client_id TEXT, ADD COLUMN client_secret_enc BYTEA, \
         ADD COLUMN token_expires_at TEXT, ADD COLUMN scopes TEXT, \
         ADD COLUMN platform_user_id TEXT, ADD COLUMN platform_username TEXT, \
         ADD COLUMN enc_version INTEGER DEFAULT 1, ADD COLUMN authorized_at TEXT, \
         ADD COLUMN refresh_expires_at TIMESTAMPTZ, \
         ADD COLUMN needs_reauth BOOLEAN NOT NULL DEFAULT FALSE",
    )
    .execute(&pool)
    .await
    .unwrap();

    async fn status_response(pool: &PgPool) -> Response {
        platforms_status_handler(
            DashboardAuthLevel::admin(),
            State(pool.clone()),
            Query(StreamerQuery {
                twitch_user_id: None,
                streamer: Some(GLOBAL_SCOPE_MARKER.into()),
            }),
        )
        .await
    }

    let response = status_response(&pool).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    let platforms = body["platforms"].as_array().unwrap();
    assert_eq!(platforms.len(), PLATFORMS.len());
    assert!(platforms.iter().all(|status| status["connected"] == false));

    for platform in PLATFORMS {
        let encrypted = cipher
            .encrypt_field(
                "local-access",
                &tb_crypto::aad::social_media("access_token", platform, None, 1),
            )
            .unwrap();
        sqlx::query(
            "INSERT INTO social_media_platform_auth (platform, access_token_enc) VALUES ($1, $2)",
        )
        .bind(platform)
        .bind(encrypted)
        .execute(&pool)
        .await
        .unwrap();
        let response = status_response(&pool).await;
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

        // Nur der zweite Lesezugriff scheitert, Credentials bleiben lesbar.
        sqlx::query("ALTER TABLE social_media_platform_auth DROP COLUMN refresh_expires_at")
            .execute(&pool)
            .await
            .unwrap();
        assert!(build_credential_manager(pool.clone())
            .unwrap()
            .get_credentials(platform, None)
            .await
            .is_some());
        let response = status_response(&pool).await;
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

    // Auch der erste SQL-Zugriff darf keinen getrennten Zugang vortäuschen.
    sqlx::query("DROP TABLE social_media_platform_auth")
        .execute(&pool)
        .await
        .unwrap();
    let response = status_response(&pool).await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        body_json(response).await,
        json!({ "error": "platform_status_failed" })
    );
}
