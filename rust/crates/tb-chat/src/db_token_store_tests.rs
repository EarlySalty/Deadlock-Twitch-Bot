use super::*;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use std::str::FromStr;

#[tokio::test]
async fn encrypted_bot_store_preserves_identity_revocation_and_rotation() {
    let dsn = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../token-db-tests.conf"
    ))
    .expect("Explizite Wegwerf-DB in token-db-tests.conf angeben");
    let dsn = dsn.trim();
    let options = PgConnectOptions::from_str(dsn).unwrap();
    assert!(options
        .get_database()
        .is_some_and(|name| name.starts_with("token_db_")));
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(options.clone())
        .await
        .unwrap();
    // Die Migration benennt public ausdrücklich. Deshalb ein vollständig
    // eigenes Wegwerf-DB-Objekt statt eines bloßen search_path-Schemas.
    let database = format!("token_db_chat_{}", std::process::id());
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE DATABASE {database}")))
        .execute(&admin)
        .await
        .unwrap();
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect_with(options.database(&database))
        .await
        .unwrap();
    // Die echte neue Migration statt einer nachgebauten Credential-Tabelle
    // prüfen. Nur ihre bestehenden Nachbartabellen sind minimale Fixtures.
    sqlx::raw_sql("CREATE TABLE oauth_state_tokens(expires_at TIMESTAMPTZ); CREATE TABLE twitch_vod_archive_parts(upload_session_uri TEXT);")
        .execute(&pool).await.unwrap();
    sqlx::raw_sql(include_str!(
        "../../../migrations/20260930220200_token_storage.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    let cipher = Arc::new(FieldCipher::from_hex_key(&"11".repeat(32), "v1").unwrap());
    let make =
        |client: &str| DatabaseTokenStore::new(pool.clone(), cipher.clone(), client.to_string());
    let first = make("synthetic-client");
    assert!(first.seed_and_load(SeedTokens::default()).await.is_err());
    let seeds = SeedTokens {
        access_token: Some("synthetic-access".into()),
        refresh_token: Some("synthetic-refresh".into()),
    };
    assert_eq!(first.seed_and_load(seeds.clone()).await.unwrap(), seeds);
    first.bind_identity("123").await.unwrap();
    assert!(first.bind_identity("456").await.is_err());
    assert!(make("other-client")
        .seed_and_load(seeds.clone())
        .await
        .is_err());
    let stale = make("synthetic-client");
    stale.seed_and_load(SeedTokens::default()).await.unwrap();
    assert!(first.prepare_refresh().await.is_ok());
    first
        .persist("synthetic-access-2", Some("synthetic-refresh-2"))
        .await
        .unwrap();
    assert!(stale.persist("synthetic-stale", None).await.is_err());
    assert!(stale.prepare_refresh().await.is_err());
    let current = first.seed_and_load(seeds).await.unwrap();
    assert_eq!(
        current.refresh_token.as_deref(),
        Some("synthetic-refresh-2")
    );
    let access: Vec<u8> = sqlx::query_scalar("SELECT access_token_enc FROM twitch_bot_tokens")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(!access
        .windows(b"synthetic-access-2".len())
        .any(|v| v == b"synthetic-access-2"));
    assert!(cipher
        .decrypt_field(&access, &first.aad("refresh_token"))
        .is_err());
    let wrong = DatabaseTokenStore::new(
        pool.clone(),
        Arc::new(FieldCipher::from_hex_key(&"22".repeat(32), "v1").unwrap()),
        "synthetic-client".into(),
    );
    assert!(wrong.seed_and_load(SeedTokens::default()).await.is_err());
    sqlx::query("UPDATE twitch_bot_tokens SET revoked_at=now()")
        .execute(&pool)
        .await
        .unwrap();
    assert!(first.persist("synthetic-new", None).await.is_err());
    assert!(first.prepare_refresh().await.is_err());
    assert!(first
        .seed_and_load(SeedTokens {
            access_token: None,
            refresh_token: Some("synthetic-seed".into())
        })
        .await
        .is_err());
    pool.close().await;
    assert!(first.seed_and_load(SeedTokens::default()).await.is_err());
    sqlx::query(sqlx::AssertSqlSafe(format!("DROP DATABASE {database}")))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}
