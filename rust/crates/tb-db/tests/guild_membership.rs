#[path = "../../../test-support/guild_membership.rs"]
mod guilds;
#[path = "../../../test-support/postgres.rs"]
mod postgres;

use tb_db::guild_membership::{home_guild_id, scoped_invite_for_id, scoped_invite_for_login};

#[tokio::test]
async fn home_server_is_active_community_then_oldest_with_stable_ties() {
    let db = postgres::TestPostgres::start().await;
    guilds::schema(&db.pool).await;
    guilds::member(&db.pool, "1", "streamer").await;
    let community = tb_db::guild_membership::community_guild_id().unwrap();
    sqlx::raw_sql("INSERT INTO discord_guilds(guild_id) VALUES(5),(6),(7);
        INSERT INTO streamer_guilds VALUES('1',6,true,'2020-01-01'),('1',5,true,'2020-01-01'),('1',7,true,'2019-01-01');")
        .execute(&db.pool).await.unwrap();
    assert_eq!(
        home_guild_id(&db.pool, "1", community).await.unwrap(),
        Some(community)
    );
    sqlx::query("UPDATE discord_guilds SET status='removed' WHERE guild_id=$1")
        .bind(community)
        .execute(&db.pool)
        .await
        .unwrap();
    assert_eq!(
        home_guild_id(&db.pool, "1", community).await.unwrap(),
        Some(7)
    );
    sqlx::query("UPDATE streamer_guilds SET active=false WHERE guild_id=7")
        .execute(&db.pool)
        .await
        .unwrap();
    assert_eq!(
        home_guild_id(&db.pool, "1", community).await.unwrap(),
        Some(5)
    );
    sqlx::query("UPDATE streamer_guilds SET active=false")
        .execute(&db.pool)
        .await
        .unwrap();
    assert_eq!(home_guild_id(&db.pool, "1", community).await.unwrap(), None);
}

#[tokio::test]
async fn invites_never_cross_home_server_or_fallback_globally() {
    let db = postgres::TestPostgres::start().await;
    guilds::schema(&db.pool).await;
    guilds::member(&db.pool, "1", "streamer").await;
    sqlx::raw_sql("CREATE TABLE twitch_streamer_invites(streamer_login text, guild_id bigint, invite_url text, twitch_user_id text);
        INSERT INTO discord_guilds(guild_id,language,promo_invite_url) VALUES(5,'en','https://discord.gg/foreign');
        INSERT INTO streamer_guilds(twitch_user_id,guild_id) VALUES('1',5);
        INSERT INTO twitch_streamer_invites VALUES('streamer',5,'https://discord.gg/wrong',NULL);")
        .execute(&db.pool).await.unwrap();
    assert_eq!(
        scoped_invite_for_login(&db.pool, "streamer").await.unwrap(),
        Some(("https://discord.gg/community".into(), false))
    );
    let community = tb_db::guild_membership::community_guild_id().unwrap();
    sqlx::query(
        "INSERT INTO twitch_streamer_invites VALUES('streamer',$1,'https://discord.gg/personal','1')",
    )
    .bind(community)
    .execute(&db.pool)
    .await
    .unwrap();
    assert_eq!(
        scoped_invite_for_id(&db.pool, "1").await.unwrap(),
        Some(("https://discord.gg/personal".into(), true))
    );
    sqlx::query("UPDATE streamer_guilds SET active=false WHERE guild_id=$1")
        .bind(community)
        .execute(&db.pool)
        .await
        .unwrap();
    assert_eq!(
        scoped_invite_for_login(&db.pool, "streamer").await.unwrap(),
        Some(("https://discord.gg/wrong".into(), true))
    );
    sqlx::query("DELETE FROM twitch_streamer_invites")
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE discord_guilds SET promo_invite_url=NULL WHERE guild_id=5")
        .execute(&db.pool)
        .await
        .unwrap();
    assert_eq!(
        scoped_invite_for_login(&db.pool, "streamer").await.unwrap(),
        None
    );
    assert_eq!(
        scoped_invite_for_login(&db.pool, "unknown").await.unwrap(),
        None
    );
}

#[tokio::test]
async fn numeric_login_and_id_lookups_stay_separate() {
    let db = postgres::TestPostgres::start().await;
    guilds::schema(&db.pool).await;
    guilds::member(&db.pool, "12345", "alpha").await;
    guilds::member(&db.pool, "67890", "12345").await;
    sqlx::raw_sql("CREATE TABLE twitch_streamer_invites(streamer_login text, guild_id bigint, invite_url text, twitch_user_id text);")
        .execute(&db.pool).await.unwrap();
    let community = tb_db::guild_membership::community_guild_id().unwrap();
    sqlx::query("INSERT INTO twitch_streamer_invites VALUES('alpha',$1,'https://discord.gg/alpha','12345'),('12345',$1,'https://discord.gg/numeric','67890')")
        .bind(community)
        .execute(&db.pool)
        .await
        .unwrap();
    assert_eq!(
        scoped_invite_for_login(&db.pool, "12345").await.unwrap(),
        Some(("https://discord.gg/numeric".into(), true))
    );
    assert_eq!(
        scoped_invite_for_id(&db.pool, "12345").await.unwrap(),
        Some(("https://discord.gg/alpha".into(), true))
    );
    assert_eq!(scoped_invite_for_id(&db.pool, "alpha").await.unwrap(), None);
}
