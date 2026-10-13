use tb_monitoring::TrackedStore;

mod support;

macro_rules! pool_or_skip {
    ($schema:expr) => {
        match support::pool_in_schema($schema).await {
            Some(pool) => pool,
            None => return,
        }
    };
}

#[tokio::test]
async fn persist_broadcaster_language_schreibt_helix_sprache() {
    let pool = pool_or_skip!("lang_persist");
    sqlx::query(
        "INSERT INTO twitch_streamer_identities (twitch_user_id, twitch_login)
         VALUES ('42', 'nani')",
    )
    .execute(&pool)
    .await
    .unwrap();
    TrackedStore::new(pool.clone())
        .persist_broadcaster_language("42", "DE")
        .await
        .unwrap();
    let language: String = sqlx::query_scalar(
        "SELECT broadcaster_language FROM twitch_streamer_identities WHERE twitch_user_id = '42'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(language, "de");
}
