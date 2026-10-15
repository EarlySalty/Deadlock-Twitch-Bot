#[path = "../../../test-support/guild_membership.rs"]
mod guilds;
#[path = "../../../test-support/postgres.rs"]
mod postgres;

use std::collections::HashMap;
use tb_raid::{build_online_candidates, PartnerRosterEntry, PartnerRosterStore, StreamData};

#[tokio::test]
async fn roster_preserves_german_home_partners_and_excludes_source() {
    let db = postgres::TestPostgres::start().await;
    guilds::schema(&db.pool).await;
    for (id, login) in [
        ("100", "Quelle"),
        ("200", "Enabled"),
        ("300", "OnlyAuth"),
        ("400", "NoAuth"),
        ("500", "Inactive"),
    ] {
        guilds::member(&db.pool, id, login).await;
    }
    sqlx::query("UPDATE streamer_guilds SET active=false WHERE twitch_user_id='500'")
        .execute(&db.pool)
        .await
        .unwrap();
    let roster = PartnerRosterStore::new(db.pool.clone())
        .load_roster("100")
        .await
        .unwrap();
    let ids: Vec<_> = roster
        .iter()
        .map(|partner| partner.twitch_user_id.as_str())
        .collect();
    assert_eq!(ids, ["200", "300", "400"]);
    assert!(roster.iter().all(|partner| partner.raid_enabled));
}

#[tokio::test]
async fn pool_filters_language_and_honors_both_cross_server_switches() {
    let db = postgres::TestPostgres::start().await;
    guilds::schema(&db.pool).await;
    for id in ["100", "200", "300", "400", "500"] {
        guilds::member(&db.pool, id, &format!("streamer{id}")).await;
    }
    sqlx::raw_sql("INSERT INTO discord_guilds(guild_id,language,cross_server_raids) VALUES(5,'de-ch',true),(6,'en-us',true),(7,'de',false);
        UPDATE streamer_guilds SET guild_id=5 WHERE twitch_user_id='300';
        UPDATE streamer_guilds SET guild_id=6 WHERE twitch_user_id='400';
        UPDATE streamer_guilds SET guild_id=7 WHERE twitch_user_id='500';
        UPDATE twitch_streamer_identities SET broadcaster_language='DE_at' WHERE twitch_user_id='100';
        UPDATE twitch_streamer_identities SET broadcaster_language='de-ch' WHERE twitch_user_id='300';
        UPDATE twitch_streamer_identities SET broadcaster_language='en-gb' WHERE twitch_user_id='400';")
        .execute(&db.pool).await.unwrap();
    let store = PartnerRosterStore::new(db.pool.clone());
    assert_eq!(ids(&store, "100").await, ["200", "300"]);
    let home = tb_db::guild_membership::community_guild_id().unwrap();
    sqlx::query("UPDATE discord_guilds SET cross_server_raids=false WHERE guild_id=$1")
        .bind(home)
        .execute(&db.pool)
        .await
        .unwrap();
    assert_eq!(ids(&store, "100").await, ["200"]);
    sqlx::query("INSERT INTO streamer_guilds(twitch_user_id,guild_id) VALUES('100',7)")
        .execute(&db.pool)
        .await
        .unwrap();
    assert_eq!(ids(&store, "100").await, ["200", "500"]);
    sqlx::query("UPDATE twitch_streamer_identities SET broadcaster_language='en-us' WHERE twitch_user_id='100'").execute(&db.pool).await.unwrap();
    assert!(ids(&store, "100").await.is_empty());
    sqlx::query("UPDATE discord_guilds SET cross_server_raids=true WHERE guild_id=$1")
        .bind(home)
        .execute(&db.pool)
        .await
        .unwrap();
    assert_eq!(ids(&store, "100").await, ["400"]);
    sqlx::query("UPDATE streamer_guilds SET active=false WHERE twitch_user_id='100'")
        .execute(&db.pool)
        .await
        .unwrap();
    assert!(ids(&store, "100").await.is_empty());
    assert_eq!(store.source_language("100").await.unwrap(), None);
}

async fn ids(store: &PartnerRosterStore, source: &str) -> Vec<String> {
    store
        .load_roster(source)
        .await
        .unwrap()
        .into_iter()
        .map(|partner| partner.twitch_user_id)
        .collect()
}

#[test]
fn online_kandidaten_nur_fuer_live_partner() {
    let roster = vec![
        PartnerRosterEntry {
            twitch_login: "live".into(),
            twitch_user_id: "1".into(),
            raid_enabled: true,
        },
        PartnerRosterEntry {
            twitch_login: "offline".into(),
            twitch_user_id: "2".into(),
            raid_enabled: true,
        },
    ];
    let mut streams = HashMap::new();
    streams.insert(
        "live".to_string(),
        StreamData {
            viewer_count: 50,
            ..Default::default()
        },
    );
    let candidates = build_online_candidates(&roster, &streams);
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].twitch_user_id, "1");
    assert_eq!(candidates[0].stream.viewer_count, 50);
}
