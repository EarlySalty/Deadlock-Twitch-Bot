async fn referral_source(
    pool: &PgPool,
    target: &str,
    target_id: &str,
    claim_time: chrono::DateTime<chrono::Utc>,
) {
    sqlx::query("INSERT INTO affiliate_accounts (twitch_login, twitch_user_id, is_active) VALUES ('referrer', '500', 1) ON CONFLICT DO NOTHING")
        .execute(pool).await.expect("verified referrer");
    sqlx::query("INSERT INTO affiliate_streamer_claims (affiliate_twitch_login, claimed_streamer_login, claimed_at, claimed_streamer_user_id) VALUES ('referrer', $1, $2, $3)")
        .bind(target).bind(claim_time.to_rfc3339()).bind(target_id).execute(pool).await.expect("existing partner claim");
}

#[tokio::test]
async fn streamer_referral_is_credited_only_after_first_activation_once() {
    let pool = pool_or_skip!("test_referral_first_activation");
    let now = chrono::Utc::now();
    promote(&pool, &default_args("referrer", "500")).await;
    referral_source(&pool, "referred", "501", now - chrono::Duration::hours(1)).await;
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_streamer_referral_credits")
        .fetch_one(&pool)
        .await
        .expect("no signup credit");
    assert_eq!(count, 0);
    promote(&pool, &default_args("referred", "501")).await;
    promote(&pool, &default_args("referred", "501")).await;
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT source_id, streamer_twitch_user_id, referred_twitch_user_id FROM twitch_streamer_referral_credits",
    ).fetch_all(&pool).await.expect("credit");
    assert_eq!(
        rows,
        vec![("streamer_referral:501".into(), "500".into(), "501".into())]
    );
    assert!(sqlx::query("DELETE FROM twitch_streamer_referral_credits")
        .execute(&pool)
        .await
        .is_err());
    assert!(
        sqlx::query("UPDATE twitch_streamer_referral_credits SET streamer_login = 'other'")
            .execute(&pool)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn streamer_referral_excludes_post_activation_stale_and_non_partner_claimants() {
    let pool = pool_or_skip!("test_referral_invalid_sources");
    let now = chrono::Utc::now();
    promote(&pool, &default_args("referrer", "500")).await;
    referral_source(&pool, "stale", "502", now - chrono::Duration::days(5)).await;
    promote(&pool, &default_args("stale", "502")).await;
    promote(&pool, &default_args("already", "503")).await;
    referral_source(&pool, "already", "503", now).await;
    promote(&pool, &default_args("already", "503")).await;
    sqlx::query(
        "UPDATE twitch_partners SET manual_partner_opt_out = 1 WHERE twitch_user_id = '500'",
    )
    .execute(&pool)
    .await
    .expect("inactive referrer");
    referral_source(&pool, "inactive", "504", now).await;
    promote(&pool, &default_args("inactive", "504")).await;
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_streamer_referral_credits")
        .fetch_one(&pool)
        .await
        .expect("invalid claims");
    assert_eq!(count, 0);
}

#[tokio::test]
async fn streamer_referral_rolls_back_with_the_partner_activation() {
    let pool = pool_or_skip!("test_referral_atomic_activation");
    let now = chrono::Utc::now();
    promote(&pool, &default_args("referrer", "500")).await;
    referral_source(&pool, "referred", "505", now - chrono::Duration::hours(1)).await;
    let mut tx = pool.begin().await.expect("activation transaction");
    promote_streamer_to_partner(&mut tx, &default_args("referred", "505"), now)
        .await
        .expect("activation and credit");
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_streamer_referral_credits")
        .fetch_one(&mut *tx)
        .await
        .expect("transaction credit");
    assert_eq!(count, 1);
    tx.rollback().await.expect("rollback");
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_streamer_referral_credits")
        .fetch_one(&pool)
        .await
        .expect("rolled back credit");
    assert_eq!(count, 0);
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM twitch_partners WHERE twitch_user_id = '505')",
    )
    .fetch_one(&pool)
    .await
    .expect("rolled back partner");
    assert!(!exists);
}
