pub async fn schema(pool: &sqlx::PgPool) {
    sqlx::raw_sql(
        "CREATE TABLE IF NOT EXISTS affiliate_accounts (
             twitch_login TEXT PRIMARY KEY, twitch_user_id TEXT, is_active INTEGER NOT NULL DEFAULT 1
         );
         CREATE TABLE IF NOT EXISTS affiliate_streamer_claims (
             affiliate_twitch_login TEXT NOT NULL, claimed_streamer_login TEXT PRIMARY KEY, claimed_at TEXT NOT NULL
         );",
    ).execute(pool).await.expect("referral source fixture");
    sqlx::raw_sql(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/20260927010000_streamer_referral_credits.sql"
    )))
    .execute(pool)
    .await
    .expect("referral credit migration");
}
