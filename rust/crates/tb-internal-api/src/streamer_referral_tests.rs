#[tokio::test]
async fn verify_first_activation_reuses_the_existing_referral_source_once() {
    let dsn = db_dsn_or_skip!();
    let pool = make_pool(&dsn, "test_lc_referral_credit").await;
    insert_active_partner(&pool, 1, "referrer", "500").await;
    sqlx::query("INSERT INTO affiliate_accounts (twitch_login, twitch_user_id, is_active) VALUES ('referrer', '500', 1)")
        .execute(&pool).await.expect("verified claimant");
    sqlx::query("INSERT INTO affiliate_streamer_claims (affiliate_twitch_login, claimed_streamer_login, claimed_at) VALUES ('referrer', 'referred', $1)")
        .bind((chrono::Utc::now() - chrono::Duration::hours(1)).to_rfc3339())
        .execute(&pool).await.expect("existing claim");
    let payload = VerificationPayload::for_mode("permanent").expect("verification");
    for _ in 0..2 {
        promote_streamer_to_partner(&pool, "referred", "501", None, None, 1, &payload)
            .await
            .expect("verify promotion");
    }
    let rows: Vec<(String, String)> = sqlx::query_as("SELECT streamer_twitch_user_id, referred_twitch_user_id FROM twitch_streamer_referral_credits")
        .fetch_all(&pool).await.expect("credit");
    assert_eq!(rows, vec![("500".into(), "501".into())]);
}

#[tokio::test]
async fn verify_credit_failure_rolls_back_partner_activation() {
    let dsn = db_dsn_or_skip!();
    let pool = make_pool(&dsn, "test_lc_referral_rollback").await;
    insert_active_partner(&pool, 1, "referrer", "500").await;
    sqlx::query("INSERT INTO affiliate_accounts (twitch_login, twitch_user_id, is_active) VALUES ('referrer', '500', 1)")
        .execute(&pool).await.expect("verified claimant");
    sqlx::query("INSERT INTO affiliate_streamer_claims (affiliate_twitch_login, claimed_streamer_login, claimed_at) VALUES ('referrer', 'referred', $1)")
        .bind((chrono::Utc::now() - chrono::Duration::hours(1)).to_rfc3339())
        .execute(&pool).await.expect("existing claim");
    sqlx::query("ALTER TABLE twitch_streamer_referral_credits ADD CONSTRAINT reject_fixture CHECK (referred_twitch_user_id <> '501')")
        .execute(&pool).await.expect("failure injection");
    let payload = VerificationPayload::for_mode("permanent").expect("verification");
    assert!(
        promote_streamer_to_partner(&pool, "referred", "501", None, None, 1, &payload)
            .await
            .is_err()
    );
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM twitch_partners WHERE twitch_user_id = '501')",
    )
    .fetch_one(&pool)
    .await
    .expect("atomic rollback");
    assert!(!exists);
}
