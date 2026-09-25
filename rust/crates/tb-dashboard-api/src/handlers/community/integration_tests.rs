//! Real disposable PostgreSQL, never production data or network integrations.
use super::*;
use axum::body::to_bytes;

#[tokio::test]
async fn real_database_filters_partners_and_returns_private_read_only_recommendations() {
    let db = crate::test_database::Database::new().await;
    sqlx::raw_sql(r#"
        CREATE TABLE twitch_partners (
          twitch_user_id TEXT PRIMARY KEY, twitch_login TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'active',
          departnered_at TEXT, admin_archived_at TEXT, manual_partner_opt_out INTEGER DEFAULT 0,
          technical_pause_reason TEXT
        );
        CREATE TABLE twitch_streamer_identities (twitch_user_id TEXT PRIMARY KEY, discord_user_id TEXT);
        CREATE VIEW twitch_partners_all_state AS
        SELECT p.twitch_user_id, p.twitch_login, p.departnered_at, i.discord_user_id,
          CASE WHEN p.status='active' AND COALESCE(p.manual_partner_opt_out,0)=0
            AND COALESCE(p.technical_pause_reason,'')='' AND p.admin_archived_at IS NULL
            THEN 1 ELSE 0 END AS is_partner_active
        FROM twitch_partners p
        LEFT JOIN twitch_streamer_identities i ON i.twitch_user_id=p.twitch_user_id;
        CREATE TABLE twitch_live_state (
          twitch_user_id TEXT PRIMARY KEY, is_live INTEGER, last_seen_at TEXT, last_game TEXT, last_title TEXT
        );
        CREATE TABLE twitch_stream_sessions (
          streamer_login TEXT NOT NULL, started_at TIMESTAMPTZ NOT NULL, ended_at TIMESTAMPTZ,
          game_name TEXT, stream_title TEXT
        );
        INSERT INTO twitch_partners (twitch_user_id,twitch_login) VALUES ('1','Alice'),('2','Bob'),('3','archived'),('4','optout'),('5','former'),('6','paused'),('7','stale_former');
        UPDATE twitch_partners SET admin_archived_at='2026-01-01' WHERE twitch_user_id='3';
        UPDATE twitch_partners SET manual_partner_opt_out=1 WHERE twitch_user_id='4';
        UPDATE twitch_partners SET status='departnered', departnered_at='2026-01-01' WHERE twitch_user_id='5';
        UPDATE twitch_partners SET technical_pause_reason='token_error' WHERE twitch_user_id='6';
        UPDATE twitch_partners SET departnered_at='2026-01-01' WHERE twitch_user_id='7';
        INSERT INTO twitch_streamer_identities VALUES ('1',NULL),('2',NULL);
        INSERT INTO twitch_stream_sessions
          SELECT login, NOW()-make_interval(days=>day), NOW()-make_interval(days=>day)+INTERVAL '2 hours',
                 'Deadlock','Street Brawl mit euch'
          FROM unnest(ARRAY['Alice','Bob','archived','optout','former','paused','stale_former']) login CROSS JOIN unnest(ARRAY[2,9,16]) day;
        -- Missing end and obviously broken duration must not fabricate a schedule.
        INSERT INTO twitch_stream_sessions VALUES ('Bob',NOW()-INTERVAL '1 day',NULL,'Deadlock',NULL),
          ('Bob',NOW()-INTERVAL '4 days',NOW()-INTERVAL '1 day','Deadlock',NULL);
    "#).execute(&db.pool).await.unwrap();
    let response = get_handler(
        DashboardAuthLevel::Partner { twitch_user_id:"1".into(), twitch_login:"alice".into(), display_name:"Alice".into() },
        State(db.pool.clone()), Query(Params { streamer:Some("alice".into()),days:Some("56".into()) }),
    ).await;
    assert_eq!(response.status(),StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL],"private, no-store");
    let bytes=to_bytes(response.into_body(),1_000_000).await.unwrap();
    let value:serde_json::Value=serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["candidate_count"],1);
    assert_eq!(value["recommendations"].as_array().unwrap().len(),1);
    let bob=&value["recommendations"][0];
    assert_eq!(bob["login"],"bob");
    assert_eq!(bob["schedule"]["sessions"],3);
    assert_eq!(bob["observed_overlap_minutes"],360);
    assert!(bob["score"].as_u64().unwrap()>0);
    assert_eq!(bob["profile"]["mode_source"],"historical_title");
    assert!(bob["profile"]["rank_tier"].is_null());
    assert_eq!(value["discord"]["status"],"link_required");
    assert_eq!(value["own_schedule"]["slots"].as_array().unwrap().len(),336);
    let text=String::from_utf8(bytes.to_vec()).unwrap();
    assert!(!text.contains("discord_id"));
    assert!(!text.contains("twitch_user_id"));
    assert!(!text.contains("steam_id"));
    assert_eq!(sqlx::query_scalar::<_,i64>("SELECT COUNT(*) FROM twitch_stream_sessions").fetch_one(&db.pool).await.unwrap(),23);
    db.close().await;
}
