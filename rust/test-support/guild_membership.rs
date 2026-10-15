pub fn install_config() {
    static CONFIG: std::sync::Once = std::sync::Once::new();
    CONFIG.call_once(|| {
        if tb_config::runtime::active().is_none() {
            let snapshot = tb_config::BotConfigSnapshot::parse(
                "schema_version=1\n[twitch]\nbot_user_id='11'\nnotify_channel_id='22'\neventsub_callback_url='https://example.invalid/callback'\n",
                std::path::Path::new("/tmp/tb-guild-test/bot.toml"),
            ).expect("valid isolated configuration");
            let _ = tb_config::runtime::install(snapshot);
        }
    });
}

pub async fn schema(pool: &sqlx::PgPool) {
    install_config();
    sqlx::raw_sql(
        "CREATE TABLE discord_guilds (guild_id bigint PRIMARY KEY, name text, language text NOT NULL DEFAULT 'de',
            cross_server_raids boolean NOT NULL DEFAULT true, promo_invite_url text,
            status text NOT NULL DEFAULT 'active', removed_at timestamptz, announce_channel_id bigint,
            updated_at timestamptz, updated_by text);
         CREATE TABLE streamer_guilds (twitch_user_id text, guild_id bigint, active boolean DEFAULT true,
            joined_at timestamptz NOT NULL DEFAULT now());
         CREATE TABLE twitch_streamer_identities (twitch_user_id text PRIMARY KEY, twitch_login text, broadcaster_language text);
         CREATE VIEW partner_guild_active AS SELECT DISTINCT twitch_user_id FROM streamer_guilds WHERE active;",
    ).execute(pool).await.expect("guild fixture schema");
    let community = tb_db::guild_membership::community_guild_id().expect("configured community");
    sqlx::query("INSERT INTO discord_guilds(guild_id, name, promo_invite_url) VALUES($1, 'Community', 'https://discord.gg/community')")
        .bind(community).execute(pool).await.expect("community fixture");
}

pub async fn member(pool: &sqlx::PgPool, id: &str, login: &str) {
    let community = tb_db::guild_membership::community_guild_id().expect("configured community");
    sqlx::query("INSERT INTO twitch_streamer_identities(twitch_user_id,twitch_login,broadcaster_language) VALUES($1,$2,'de')")
        .bind(id).bind(login).execute(pool).await.expect("identity fixture");
    sqlx::query("INSERT INTO streamer_guilds(twitch_user_id,guild_id) VALUES($1,$2)")
        .bind(id)
        .bind(community)
        .execute(pool)
        .await
        .expect("membership fixture");
}
