use super::DisposablePostgres;

pub(super) async fn start(test_name: &str, twitch_user_id: &str) -> Option<DisposablePostgres> {
    if !crate::hub_test_fixture::in_isolated_process(test_name) {
        return None;
    }
    let database = DisposablePostgres::start().await;
    let pool = &database.pool;
    for schema in [
        include_str!("../../../../test-support/multiserver_ai_schema.sql"),
        include_str!("../../../../test-support/20261012140000_ai_features_rule.sql"),
        include_str!("../../../../test-support/20261013150200_multiserver_ai_contract.sql"),
    ] {
        sqlx::raw_sql(schema)
            .execute(pool)
            .await
            .expect("accepted Editor authorization schema");
    }
    sqlx::query("INSERT INTO public.discord_guilds (guild_id, ai_features) VALUES (900000000000000041, true)")
        .execute(pool)
        .await
        .expect("synthetic Editor guild");
    sqlx::query("INSERT INTO public.streamer_guilds (twitch_user_id, guild_id, active) VALUES ($1, 900000000000000041, true)")
        .bind(twitch_user_id)
        .execute(pool)
        .await
        .expect("scoped Editor membership");
    sqlx::query("INSERT INTO public.twitch_streamer_identities (twitch_user_id, twitch_login) VALUES ($1, 'editor_fixture')")
        .bind(twitch_user_id)
        .execute(pool)
        .await
        .expect("stable Editor identity");
    sqlx::query("INSERT INTO public.twitch_partners_all_state (twitch_user_id, is_partner_active) VALUES ($1, 1)")
        .bind(twitch_user_id)
        .execute(pool)
        .await
        .expect("active synthetic Editor partner");
    assert!(std::sync::Arc::ptr_eq(
        &crate::ledger::pool()
            .await
            .expect("Editor ledger pool")
            .connect_options(),
        &pool.connect_options(),
    ));
    assert_eq!(
        crate::ai_guard::streamer_ai_allowed(Some(twitch_user_id), Some("editor_fixture")).await,
        Ok(true),
    );
    assert_eq!(
        crate::ai_guard::twitch_user_ai_allowed(pool, twitch_user_id).await,
        Ok(true),
    );
    Some(database)
}
