pub async fn schema(pool: &sqlx::PgPool) {
    sqlx::raw_sql(
        "CREATE TABLE IF NOT EXISTS affiliate_accounts (
             twitch_login TEXT PRIMARY KEY, twitch_user_id TEXT, is_active INTEGER NOT NULL DEFAULT 1
         );
         CREATE TABLE IF NOT EXISTS affiliate_streamer_claims (
             affiliate_twitch_login TEXT NOT NULL, claimed_streamer_login TEXT PRIMARY KEY, claimed_at TEXT NOT NULL,
             claimed_streamer_user_id TEXT
         );",
    ).execute(pool).await.expect("referral source fixture");
    sqlx::raw_sql(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/20260927010000_streamer_referral_credits.sql"
    )))
    .execute(pool)
    .await
    .expect("referral credit migration");
    let roles = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../migrations/20260930010000_partner_challenge_runtime_roles.sql"
    ));
    let body = roles
        .split("$claims$")
        .nth(1)
        .expect("production referral function body");
    let schema: String = sqlx::query_scalar("SELECT current_schema()")
        .fetch_one(pool)
        .await
        .expect("isolated schema");
    assert!(schema
        .bytes()
        .all(|c| c.is_ascii_alphanumeric() || c == b'_'));
    // Logic tests run in isolated schemas. The full migration/role tests separately
    // verify the production SECURITY DEFINER and fixed public search_path contract.
    let body = body.replace("public.", &format!("{schema}."));
    let function = format!(
        "CREATE FUNCTION {schema}.twitch_referral_claim_candidates(target_twitch_user_id text)
        RETURNS TABLE(streamer_user_id text, streamer_login text, claimed_at text)
        LANGUAGE sql SECURITY INVOKER AS $claims${body}$claims$"
    );
    sqlx::raw_sql(&function)
        .execute(pool)
        .await
        .expect("production referral query in fixture schema");
}
