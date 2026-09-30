use chrono::{DateTime, Duration, Utc};
use serde_json::json;
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    PgPool,
};
#[path = "../../../test-support/database.rs"]
mod test_database;
use std::str::FromStr;
use tb_config::challenges::Challenges;
use tb_effort::{
    calendar::{berlin_week_start, midnight},
    Engine, Event, EventKind,
};

fn idle_helix() -> tb_transport_twitch::HelixClient {
    let mut config = tb_transport_twitch::HelixConfig::new("fixture", "fixture");
    config.token_url = "http://127.0.0.1:1/token".into();
    config.helix_base = "http://127.0.0.1:1".into();
    tb_transport_twitch::HelixClient::new(config).unwrap()
}

async fn fixture() -> (PgPool, PgPool, String) {
    let dsn =
        test_database::database_url().expect("isolierte Testdatenbank muss konfiguriert sein");
    let options = PgConnectOptions::from_str(&dsn).unwrap();
    let admin = PgPoolOptions::new()
        .max_connections(2)
        .connect_with(options.clone())
        .await
        .unwrap();
    let name = format!(
        "tb_effort_{}_{}",
        std::process::id(),
        Utc::now().timestamp_subsec_nanos()
    );
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE DATABASE {name}")))
        .execute(&admin)
        .await
        .unwrap();
    let pool = PgPoolOptions::new()
        .max_connections(16)
        .connect_with(options.database(&name))
        .await
        .unwrap();
    sqlx::query("CREATE EXTENSION IF NOT EXISTS timescaledb")
        .execute(&pool)
        .await
        .unwrap();
    tb_db::run_migrations(&pool).await.unwrap();
    sqlx::query("UPDATE category_collector_config SET poll_seconds=300 WHERE singleton")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO category_collection_runs(snapshot_at,completed_at,streams,viewers,poll_seconds) SELECT at,at,0,0,300 FROM generate_series('2026-09-01T00:00:00Z'::timestamptz,'2026-11-02T00:00:00Z'::timestamptz,INTERVAL '5 minutes') AS at")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::raw_sql("CREATE SCHEMA activity; CREATE SCHEMA core; CREATE SCHEMA bot; CREATE SCHEMA steam;
        CREATE TABLE core.steam_links(discord_id bigint,steam_id text,verified boolean);
        CREATE TABLE activity.voice_session_log(id bigint PRIMARY KEY,user_id bigint,guild_id bigint,channel_id bigint,started_at timestamptz,ended_at timestamptz);
        CREATE TABLE steam.steam_tasks(id bigint PRIMARY KEY,type text,payload jsonb,status text,result jsonb,finished_at timestamptz);
        CREATE TABLE bot.twitch_invite_qualification_status(singleton boolean PRIMARY KEY DEFAULT TRUE CHECK(singleton),last_completed_at timestamptz NOT NULL,last_successful_at timestamptz,evaluation_interval_seconds integer NOT NULL CHECK(evaluation_interval_seconds BETWEEN 1 AND 86400),healthy boolean NOT NULL,CHECK(NOT healthy OR last_successful_at=last_completed_at));
        INSERT INTO bot.twitch_invite_qualification_status(singleton,last_completed_at,last_successful_at,evaluation_interval_seconds,healthy) VALUES(TRUE,'2026-12-31','2026-12-31',300,TRUE);
        INSERT INTO twitch_partners(twitch_user_id,twitch_login,status) VALUES('101','alice','active'),('102','bob','active'),('103','inactive','inactive');")
        .execute(&pool).await.unwrap();
    sqlx::raw_sql(include_str!("fixtures/qualified_twitch_invites.sql"))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO category_collector_status(singleton,heartbeat_at,details) VALUES(TRUE,'2026-12-31','{\"disk_paused\":false}') ON CONFLICT(singleton) DO UPDATE SET heartbeat_at=EXCLUDED.heartbeat_at,details=EXCLUDED.details").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO twitch_live_state(twitch_user_id,streamer_login,is_live,last_seen_at) VALUES('999','poller-fixture',0,'2026-12-31T00:00:00Z')").execute(&pool).await.unwrap();
    (admin, pool, name)
}

async fn insert_invite(pool: &PgPool, join: i64, guild: i64, at: DateTime<Utc>) {
    sqlx::query("INSERT INTO bot.twitch_invite_joins(join_id,guild_id,user_id,streamer_login,streamer_twitch_user_id,inviter_twitch_user_id,invite_code,joined_at,eligible) VALUES($1,$2,$3,'alice','101','501',$4,$5,TRUE)")
        .bind(join).bind(guild).bind(1000+join).bind(format!("invite:{join}"))
        .bind(at-Duration::days(15)).execute(pool).await.unwrap();
    sqlx::query(
        "UPDATE bot.twitch_invite_joins SET status='qualified',qualified_at=$2 WHERE join_id=$1",
    )
    .bind(join)
    .bind(at)
    .execute(pool)
    .await
    .unwrap();
}

async fn insert_referral(pool: &PgPool, referred: &str, at: DateTime<Utc>) {
    sqlx::query("INSERT INTO twitch_streamer_referral_credits(referred_twitch_user_id,source_id,streamer_twitch_user_id,streamer_login,referred_login,source_claimed_at,credited_at) VALUES($1,$2,'101','alice',$1,$3,$4)")
        .bind(referred).bind(format!("streamer_referral:{referred}"))
        .bind(at-Duration::days(1)).bind(at).execute(pool).await.unwrap();
}

fn event(id: &str, kind: EventKind, source: &str, at: DateTime<Utc>) -> Event {
    Event {
        partner_twitch_user_id: id.into(),
        kind,
        source_id: source.into(),
        occurred_at: at,
        viewer_twitch_user_id: None,
        metadata: json!({}),
    }
}

#[tokio::test]
async fn ledger_caps_sources_streaks_achievements_and_fail_closed() {
    let (admin, pool, name) = fixture().await;
    let now = DateTime::parse_from_rfc3339("2026-10-26T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let cfg = Challenges::default();
    let engine = Engine::new(
        pool.clone(),
        cfg.clone(),
        Some(pool.clone()),
        Some(idle_helix()),
    )
    .unwrap();
    let mut invite = event(
        "101",
        EventKind::QualifiedInvite,
        "discord-join:one",
        now - Duration::hours(1),
    );
    invite.viewer_twitch_user_id = Some("501".into());
    assert!(engine.append(&invite, now).await.unwrap());
    assert!(!engine.append(&invite, now).await.unwrap());
    let mut conflicting = invite.clone();
    conflicting.viewer_twitch_user_id = Some("502".into());
    assert!(engine.append(&conflicting, now).await.is_err());
    conflicting = invite.clone();
    conflicting.partner_twitch_user_id = "102".into();
    assert!(engine.append(&conflicting, now).await.is_err());
    assert!(engine
        .append(
            &event("103", EventKind::QualifiedInvite, "inactive", now),
            now
        )
        .await
        .is_err());
    assert!(engine
        .append(
            &event(
                "101",
                EventKind::QualifiedInvite,
                "future",
                now + Duration::seconds(1)
            ),
            now
        )
        .await
        .is_err());
    assert!(engine
        .append(
            &event("101", EventKind::QuestDone, "forged-quest", now),
            now
        )
        .await
        .is_err());

    let mut pending = Vec::new();
    for n in 0..12 {
        let engine = engine.clone();
        pending.push(tokio::spawn(async move {
            engine
                .append(
                    &event("101", EventKind::PartyPlay, &format!("match:{n}"), now),
                    now,
                )
                .await
                .unwrap()
        }));
    }
    for task in pending {
        assert!(task.await.unwrap());
    }
    let (count,points): (i64,i64)=sqlx::query_as("SELECT COUNT(*),SUM(points)::bigint FROM partner_effort_events WHERE event_type='party_play'").fetch_one(&pool).await.unwrap();
    assert_eq!((count, points), (12, 20));
    for n in 0..4 {
        engine
            .append(
                &event(
                    "101",
                    EventKind::ClipSubmitted,
                    &format!("submitted:{n}"),
                    now,
                ),
                now,
            )
            .await
            .unwrap();
    }
    let points: i64 = sqlx::query_scalar(
        "SELECT SUM(points)::bigint FROM partner_effort_events WHERE event_type='clip_submitted'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(points, 4);
    let mut winner = event("101", EventKind::ClipTop3, "winner:1", now);
    winner.metadata = json!({"rank":0});
    assert!(engine.append(&winner, now).await.is_err());
    for rank in 1..=3 {
        winner.source_id = format!("winner:{rank}");
        winner.metadata = json!({"rank":rank});
        engine.append(&winner, now).await.unwrap();
    }
    let points: i64 = sqlx::query_scalar(
        "SELECT SUM(points)::bigint FROM partner_effort_events WHERE event_type='clip_top3'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(points, 45);
    for sql in [
        "UPDATE partner_effort_events SET points=999",
        "DELETE FROM partner_effort_events",
        "TRUNCATE partner_effort_events",
    ] {
        assert!(sqlx::query(sql).execute(&pool).await.is_err());
    }

    for n in [10i64, 11] {
        insert_invite(
            &pool,
            n,
            cfg.community_guild_id,
            now - Duration::minutes(30),
        )
        .await;
    }
    insert_referral(&pool, "102", now - Duration::minutes(20)).await;
    insert_contest_submission(&pool, "101").await;
    sqlx::query(
        "INSERT INTO twitch_clip_contest_effort_outbox(id,event_type,partner_twitch_user_id,streamer_login,source_id,occurred_at,metadata) VALUES(1,'clip_top3','101','alice','outbox:top3', $1, $2)",
    )
    .bind(now - Duration::minutes(10))
    .bind(json!({"submission_id":1,"rank":2}))
    .execute(&pool)
    .await
    .unwrap();
    let week = berlin_week_start(now);
    sqlx::query("INSERT INTO partner_effort_stream_weeks VALUES('101','stream-1',$1,1800,$2)")
        .bind(week)
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();
    engine.tick(now).await.unwrap();
    engine.ensure_ready(now).await.unwrap();
    let sources: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM partner_effort_source_receipts")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(sources, 4);
    let total_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM partner_effort_events")
        .fetch_one(&pool)
        .await
        .unwrap();
    engine.tick(now + Duration::seconds(1)).await.unwrap();
    let total_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM partner_effort_events")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(total_before, total_after);
    let me = engine.me("101", now + Duration::seconds(2)).await.unwrap();
    assert_eq!(me.quests.len(), 3);
    assert_eq!(me.streak.current, 1);
    assert!(me.streak.week_qualified);
    assert_eq!(me.season.rank, 1);
    assert_eq!(me.season.active_partners, 2);
    assert_eq!(me.with_us.people_brought_in_who_stayed, 3);
    assert!(
        me.achievements
            .iter()
            .find(|a| a.key == "team_player")
            .unwrap()
            .tiers[0]
            .unlocked
    );
    assert!(
        me.achievements
            .iter()
            .find(|a| a.key == "clip_hunter")
            .unwrap()
            .tiers[0]
            .unlocked
    );
    assert!(me.level.total_points >= 144);
    assert!(
        me.next_goal.fastest_route.contains("Partner")
            || me.next_goal.fastest_route.contains("Einladung")
    );
    assert!(engine.me("103", now).await.is_err());
    assert!(engine.viewers("101", now).await.is_err());
    let old_week = midnight(week - Duration::weeks(1)) + Duration::hours(1);
    let before = engine.me("101", now).await.unwrap().level.total_points;
    engine
        .append(
            &event("101", EventKind::PartyPlay, "prior-week-match", old_week),
            now,
        )
        .await
        .unwrap();
    assert_eq!(
        engine.me("101", now).await.unwrap().level.total_points,
        before + 5
    );
    let prior_month = DateTime::parse_from_rfc3339("2026-09-20T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    engine
        .append(
            &event(
                "101",
                EventKind::StreamerReferral,
                "old-referral",
                prior_month,
            ),
            now,
        )
        .await
        .unwrap();
    let later = engine.me("101", now).await.unwrap();
    assert_eq!(later.level.total_points, before + 55);
    assert_eq!(later.season.points, me.season.points + 55);

    sqlx::query("UPDATE partner_effort_source_state SET healthy=FALSE WHERE source='invites'")
        .execute(&pool)
        .await
        .unwrap();
    assert!(engine.ensure_ready(now).await.is_err());
    let missing_helix = Engine::new(pool.clone(), cfg.clone(), Some(pool.clone()), None).unwrap();
    assert!(missing_helix
        .tick(now + Duration::seconds(3))
        .await
        .is_err());
    let healthy: bool = sqlx::query_scalar(
        "SELECT healthy FROM partner_effort_source_state WHERE source='shared_chat'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(!healthy);
    assert!(missing_helix
        .ensure_ready(now + Duration::seconds(3))
        .await
        .is_err());
    pool.close().await;
    assert!(engine
        .append(
            &event("101", EventKind::QualifiedInvite, "after-close", now),
            now
        )
        .await
        .is_err());
    assert!(engine.me("101", now).await.is_err());
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "DROP DATABASE {name} WITH (FORCE)"
    )))
    .execute(&admin)
    .await
    .unwrap();
    admin.close().await;
}

#[tokio::test]
async fn stream_only_quest_does_not_supply_independent_streak_proof() {
    let (admin, pool, name) = fixture().await;
    let now = DateTime::parse_from_rfc3339("2026-10-23T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let week = berlin_week_start(now);
    let cfg = Challenges::default();
    let engine = Engine::new(
        pool.clone(),
        cfg.clone(),
        Some(pool.clone()),
        Some(idle_helix()),
    )
    .unwrap();
    sqlx::query("UPDATE twitch_partners SET status='inactive' WHERE twitch_user_id='102'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO bot.twitch_invite_joins(join_id,guild_id,user_id,streamer_login,streamer_twitch_user_id,inviter_twitch_user_id,invite_code,joined_at,eligible) VALUES(700,$1,1700,'alice','101','501','invite:700','2026-10-10T12:00:00Z',TRUE)")
        .bind(cfg.community_guild_id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO partner_effort_stream_weeks(partner_twitch_user_id,stream_id,week_start,deadlock_seconds,observed_through) VALUES('101','stream-only',$1,7200,$2)")
        .bind(week).bind(now).execute(&pool).await.unwrap();
    engine.tick(now).await.unwrap();
    let stream_quest: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM partner_effort_events WHERE partner_twitch_user_id='101' AND event_type='quest_done' AND metadata->>'quest'='stream_above_average'")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(stream_quest, 1);
    let me = engine.me("101", now).await.unwrap();
    assert_eq!(me.quests.len(), 2);
    assert!(!me.streak.week_qualified);
    assert_eq!(me.streak.current, 0);
    engine
        .append(
            &event("101", EventKind::QualifiedInvite, "separate-invite", now),
            now,
        )
        .await
        .unwrap();
    engine.tick(now + Duration::seconds(1)).await.unwrap();
    assert!(
        engine
            .me("101", now + Duration::seconds(1))
            .await
            .unwrap()
            .streak
            .week_qualified
    );
    pool.close().await;
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "DROP DATABASE {name} WITH (FORCE)"
    )))
    .execute(&admin)
    .await
    .unwrap();
    admin.close().await;
}

#[tokio::test]
async fn late_confirmation_credits_the_next_berlin_month() {
    let (admin, pool, name) = fixture().await;
    let before = DateTime::parse_from_rfc3339("2026-10-31T22:59:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let after = before + Duration::minutes(6);
    let engine = Engine::new(
        pool.clone(),
        Challenges::default(),
        Some(pool.clone()),
        Some(idle_helix()),
    )
    .unwrap();
    engine.tick(before).await.unwrap();
    assert_eq!(engine.me("101", before).await.unwrap().season.points, 0);
    engine
        .append(
            &event("101", EventKind::PartyPlay, "late-match", before),
            after,
        )
        .await
        .unwrap();
    engine.tick(after).await.unwrap();
    assert_eq!(engine.me("101", before).await.unwrap().season.points, 0);
    assert_eq!(engine.me("101", after).await.unwrap().season.points, 5);
    let (occurred, credited): (DateTime<Utc>, DateTime<Utc>) = sqlx::query_as(
        "SELECT occurred_at,credited_at FROM partner_effort_events WHERE source_id='late-match'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((occurred, credited), (before, after));
    pool.close().await;
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "DROP DATABASE {name} WITH (FORCE)"
    )))
    .execute(&admin)
    .await
    .unwrap();
    admin.close().await;
}

#[tokio::test]
async fn shared_chat_collection_is_bounded_and_keeps_completed_partner_evidence() {
    use tb_transport_twitch::{HelixClient, HelixConfig};
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };
    let (admin, pool, name) = fixture().await;
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"access_token":"fixture","expires_in":3600,"token_type":"bearer"}),
        ))
        .mount(&server)
        .await;
    let ids: Vec<_> = (101..=112).filter(|id| *id != 103).collect();
    let participants: Vec<_> = ids
        .iter()
        .map(|id| json!({"broadcaster_id":id.to_string()}))
        .collect();
    Mock::given(method("GET"))
        .and(path("/shared_chat/session"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(std::time::Duration::from_secs(1))
                .set_body_json(json!({"data":[{"session_id":"shared-session","host_broadcaster_id":"101","participants":participants,"created_at":"2026-10-26T10:00:00Z","updated_at":"2026-10-26T10:00:00Z"}]})),
        )
        .mount(&server)
        .await;
    let mut hc = HelixConfig::new("fixture", "fixture");
    hc.helix_base = server.uri();
    hc.token_url = format!("{}/token", server.uri());
    let engine = Engine::new(
        pool.clone(),
        Challenges::default(),
        Some(pool.clone()),
        Some(HelixClient::new(hc).unwrap()),
    )
    .unwrap();
    let start = DateTime::parse_from_rfc3339("2026-10-26T10:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    sqlx::query("INSERT INTO twitch_partners(twitch_user_id,twitch_login,status) SELECT id::text,'partner'||id,'active' FROM generate_series(104,112) AS id")
        .execute(&pool)
        .await
        .unwrap();
    for id in &ids {
        let login = format!("partner{id}");
        let session_id = i64::from(*id) + 10_000;
        let stream_id = format!("stream-{id}");
        sqlx::query("INSERT INTO twitch_stream_sessions(id,twitch_user_id,streamer_login,stream_id,started_at,game_name) VALUES($1,$2,$3,$4,$5,'Deadlock')")
            .bind(session_id)
            .bind(id.to_string())
            .bind(&login)
            .bind(&stream_id)
            .bind(start)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO twitch_live_state(twitch_user_id,streamer_login,active_session_id,is_live,last_seen_at,last_game) VALUES($1,$2,$3,1,$4,'Deadlock')")
            .bind(id.to_string())
            .bind(&login)
            .bind(session_id)
            .bind(start.to_rfc3339())
            .execute(&pool)
            .await
            .unwrap();
    }
    sqlx::query("INSERT INTO partner_effort_shared_chat_observations(partner_twitch_user_id,stream_id,other_partner_twitch_user_id,shared_chat_session_id,first_seen_at,last_seen_at,confirmed_seconds) VALUES('101','stream-101','102','shared-session',$1,$2,120)")
        .bind(start - Duration::seconds(120))
        .bind(start - Duration::seconds(60))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO partner_effort_source_state(source,checked_at,healthy,error_code) VALUES('shared_chat',$1,FALSE,'source_unavailable')")
        .bind(start - Duration::seconds(1))
        .execute(&pool)
        .await
        .unwrap();

    engine.tick(start).await.unwrap();

    let seconds: i64 = sqlx::query_scalar("SELECT confirmed_seconds FROM partner_effort_shared_chat_observations WHERE partner_twitch_user_id='101' AND other_partner_twitch_user_id='102'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(seconds, 180);
    let observations: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM partner_effort_shared_chat_observations")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(observations, 110);
    pool.close().await;
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "DROP DATABASE {name} WITH (FORCE)"
    )))
    .execute(&admin)
    .await
    .unwrap();
    admin.close().await;
}

#[tokio::test]
async fn shared_chat_duration_and_completed_steam_match_are_required() {
    use tb_transport_twitch::{HelixClient, HelixConfig};
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };
    let (admin, pool, name) = fixture().await;
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"access_token":"fixture","expires_in":3600,"token_type":"bearer"}),
        ))
        .mount(&server)
        .await;
    Mock::given(method("GET")).and(path("/shared_chat/session")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[{"session_id":"shared-session","host_broadcaster_id":"101","participants":[{"broadcaster_id":"101"},{"broadcaster_id":"102"}],"created_at":"2026-10-26T10:00:00Z","updated_at":"2026-10-26T10:00:00Z"}]}))).mount(&server).await;
    let mut hc = HelixConfig::new("fixture", "fixture");
    hc.helix_base = server.uri();
    hc.token_url = format!("{}/token", server.uri());
    let cfg = Challenges::default();
    let engine = Engine::new(
        pool.clone(),
        cfg.clone(),
        Some(pool.clone()),
        Some(HelixClient::new(hc).unwrap()),
    )
    .unwrap();
    let start = DateTime::parse_from_rfc3339("2026-10-26T10:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    sqlx::raw_sql("CREATE SCHEMA voice;
        CREATE TABLE voice.deadlock_party_members(party_id text,steam_id text,seen_at timestamptz);
        CREATE TABLE activity.live_player_state(steam_id text,deadlock_minutes integer,deadlock_updated_at timestamptz,in_deadlock_now boolean,in_match_now_strict boolean);
        INSERT INTO twitch_streamer_identities(twitch_user_id,twitch_login,discord_user_id,is_on_discord) VALUES('101','alice','1001',1),('102','bob','1002',1) ON CONFLICT(twitch_user_id) DO UPDATE SET discord_user_id=EXCLUDED.discord_user_id,is_on_discord=EXCLUDED.is_on_discord;
        INSERT INTO twitch_streamer_identities(twitch_user_id,twitch_login,discord_user_id,is_on_discord) VALUES('103','inactive','1003',0) ON CONFLICT(twitch_user_id) DO UPDATE SET discord_user_id=EXCLUDED.discord_user_id,is_on_discord=EXCLUDED.is_on_discord;
        INSERT INTO core.steam_links VALUES(1001,'76561197960265801',TRUE),(1002,'76561197960265802',TRUE),(1003,'76561197960265803',TRUE);")
        .execute(&pool).await.unwrap();
    for (id, login, stream, session) in [
        ("101", "alice", "stream-a", 9001i64),
        ("102", "bob", "stream-b", 9002),
    ] {
        sqlx::query("INSERT INTO twitch_stream_sessions(id,twitch_user_id,streamer_login,stream_id,started_at,game_name) VALUES($1,$2,$3,$4,$5,'Deadlock')").bind(session).bind(id).bind(login).bind(stream).bind(start).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_live_state(twitch_user_id,streamer_login,active_session_id,is_live,last_seen_at,last_game) VALUES($1,$2,$3,1,$4,'Deadlock')").bind(id).bind(login).bind(session).bind(start.to_rfc3339()).execute(&pool).await.unwrap();
    }
    for n in 0..=30 {
        let now = start + Duration::minutes(n);
        sqlx::query("UPDATE twitch_live_state SET last_seen_at=$1")
            .bind(now.to_rfc3339())
            .execute(&pool)
            .await
            .unwrap();
        engine.tick(now).await.unwrap();
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM partner_effort_events WHERE event_type='co_stream'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(count, if n < 30 { 0 } else { 2 });
    }
    engine.tick(start + Duration::minutes(31)).await.unwrap();
    let points: i64 = sqlx::query_scalar(
        "SELECT SUM(points)::bigint FROM partner_effort_events WHERE event_type='co_stream'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(points, 16);
    let observed = start + Duration::minutes(35);
    for steam in [
        "76561197960265801",
        "76561197960265802",
        "76561197960265803",
    ] {
        sqlx::query("INSERT INTO voice.deadlock_party_members VALUES('real-party',$1,$2)")
            .bind(steam)
            .bind(observed)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO activity.live_player_state VALUES($1,35,$2,TRUE,TRUE)")
            .bind(steam)
            .bind(observed)
            .execute(&pool)
            .await
            .unwrap();
    }
    sqlx::query("UPDATE twitch_live_state SET last_seen_at=$1")
        .bind(observed.to_rfc3339())
        .execute(&pool)
        .await
        .unwrap();
    engine.tick(observed).await.unwrap();
    let pending: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM partner_effort_party_observations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(pending, 2);
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM partner_effort_events WHERE event_type='party_play'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 0);
    let finished = observed + Duration::minutes(5);
    for (id, steam, result) in [(1i64, "76561197960265801", 1), (2, "76561197960265802", 0)] {
        sqlx::query("INSERT INTO steam.steam_tasks VALUES($1,'GC_GET_MATCH_HISTORY',$2,'DONE',$3,$4)")
            .bind(id).bind(json!({"steam_id":steam})).bind(json!({"ok":true,"data":{"steam_id64":steam,"matches":[{"match_id":500,"start_time":start.timestamp(),"match_result":result}]}})).bind(finished-Duration::seconds(10)).execute(&pool).await.unwrap();
    }
    for task in 0_i64..9 {
        for (peer, steam) in ["76561197960265801", "76561197960265802"]
            .into_iter()
            .enumerate()
        {
            let task_id = 3 + task * 2 + peer as i64;
            let completed_at = finished - Duration::seconds(9 - task);
            sqlx::query(
                "INSERT INTO steam.steam_tasks VALUES($1,'GC_GET_MATCH_HISTORY',$2,'DONE',$3,$4)",
            )
            .bind(task_id)
            .bind(json!({"steam_id":steam}))
            .bind(json!({"ok":true,"data":{"steam_id64":steam,"matches":[]}}))
            .bind(completed_at)
            .execute(&pool)
            .await
            .unwrap();
        }
    }
    // The real Twitch poller continues while Steam finishes the match.
    sqlx::query("UPDATE twitch_live_state SET last_seen_at=$1")
        .bind(finished.to_rfc3339())
        .execute(&pool)
        .await
        .unwrap();
    engine.tick(finished).await.unwrap();
    engine.tick(finished + Duration::seconds(1)).await.unwrap();
    let (count,points): (i64,i64)=sqlx::query_as("SELECT COUNT(*),SUM(points)::bigint FROM partner_effort_events WHERE event_type='party_play'").fetch_one(&pool).await.unwrap();
    assert_eq!((count, points), (2, 10));
    sqlx::query("INSERT INTO activity.voice_session_log VALUES(1,1001,$1,99,$2,$3),(2,1002,$1,99,$2,$3),(3,1002,$1,99,$2,$3)").bind(cfg.community_guild_id).bind(start).bind(start+Duration::hours(1)).execute(&pool).await.unwrap();
    let me = engine.me("101", finished).await.unwrap();
    assert_eq!(me.with_us.community_hours, 1.0);
    Mock::given(method("GET"))
        .and(path("/users"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"data":[{"id":"501","login":"recruiter","display_name":"Recruiter Name"}]}),
        ))
        .mount(&server)
        .await;
    let mut invite = event(
        "101",
        EventKind::QualifiedInvite,
        "recruiter-proof",
        finished,
    );
    invite.viewer_twitch_user_id = Some("501".into());
    engine.append(&invite, finished).await.unwrap();
    let viewers = engine.viewers("101", finished).await.unwrap();
    assert_eq!(viewers.recruiters.len(), 1);
    assert_eq!(
        viewers.recruiters[0].display_name.as_deref(),
        Some("Recruiter Name")
    );
    assert_eq!(viewers.recruiters[0].qualified_invites, 1);
    sqlx::query("INSERT INTO partner_effort_party_observations(partner_twitch_user_id,stream_id,party_id,steam_id,other_steam_id,discord_id,other_discord_id,observed_at,match_started_at,stream_started_at) VALUES('101','expired-stream','expired-party','76561197960265801','76561197960265802',1001,1002,$1,$1,$1)")
        .bind(finished-Duration::days(8)).execute(&pool).await.unwrap();
    engine.tick(finished + Duration::seconds(2)).await.unwrap();
    let expired: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM partner_effort_party_observations WHERE party_id='expired-party'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(expired, 0);
    let permanent: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM partner_effort_events WHERE event_type='party_play'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(permanent, 2);

    pool.close().await;
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "DROP DATABASE {name} WITH (FORCE)"
    )))
    .execute(&admin)
    .await
    .unwrap();
    admin.close().await;
}

#[tokio::test]
async fn verified_discord_peer_without_streamer_profile_confirms_party_match() {
    use tb_transport_twitch::{HelixClient, HelixConfig};
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };
    let (admin, pool, name) = fixture().await;
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"access_token":"fixture","expires_in":3600,"token_type":"bearer"}),
        ))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/shared_chat/session"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data":[]})))
        .mount(&server)
        .await;
    let mut hc = HelixConfig::new("fixture", "fixture");
    hc.helix_base = server.uri();
    hc.token_url = format!("{}/token", server.uri());
    let engine = Engine::new(
        pool.clone(),
        Challenges::default(),
        Some(pool.clone()),
        Some(HelixClient::new(hc).unwrap()),
    )
    .unwrap();
    let start = DateTime::parse_from_rfc3339("2026-10-26T10:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let observed = start + Duration::minutes(35);
    sqlx::raw_sql("CREATE SCHEMA voice;
        CREATE TABLE voice.deadlock_party_members(party_id text,steam_id text,seen_at timestamptz);
        CREATE TABLE activity.live_player_state(steam_id text,deadlock_minutes integer,deadlock_updated_at timestamptz,in_deadlock_now boolean,in_match_now_strict boolean);
        INSERT INTO twitch_streamer_identities(twitch_user_id,twitch_login,discord_user_id,is_on_discord) VALUES('101','alice','1001',1) ON CONFLICT(twitch_user_id) DO UPDATE SET discord_user_id=EXCLUDED.discord_user_id,is_on_discord=EXCLUDED.is_on_discord;
        INSERT INTO core.steam_links VALUES(1001,'76561197960265828',TRUE),(1004,'76561197960265832',TRUE);")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO twitch_stream_sessions(id,twitch_user_id,streamer_login,stream_id,started_at,game_name) VALUES(9001,'101','alice','stream-a',$1,'Deadlock')")
        .bind(start).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO twitch_live_state(twitch_user_id,streamer_login,active_session_id,is_live,last_seen_at,last_game) VALUES('101','alice',9001,1,$1,'Deadlock')")
        .bind(observed.to_rfc3339()).execute(&pool).await.unwrap();
    for steam in ["76561197960265828", "76561197960265832"] {
        sqlx::query("INSERT INTO voice.deadlock_party_members VALUES('peer-party',$1,$2)")
            .bind(steam)
            .bind(observed)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO activity.live_player_state VALUES($1,35,$2,TRUE,TRUE)")
            .bind(steam)
            .bind(observed)
            .execute(&pool)
            .await
            .unwrap();
    }
    engine.tick(observed).await.unwrap();
    let peer: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM partner_effort_party_observations WHERE partner_twitch_user_id='101' AND other_discord_id=1004")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(peer, 1);
    let finished = observed + Duration::minutes(5);
    sqlx::query("INSERT INTO steam.steam_tasks VALUES(1,'GC_GET_MATCH_HISTORY',$1,'DONE',$2,$3)")
        .bind(json!({"steam_id":"76561197960265828"}))
        .bind(json!({"ok":true,"data":{"steam_id64":"76561197960265828","account_id":100,"matches":[{"match_id":501,"start_time":start.timestamp(),"match_result":1}]}}))
        .bind(finished).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO steam.steam_tasks VALUES(2,'GC_GET_MATCH_HISTORY',$1,'DONE',$2,$3)")
        .bind(json!({"account_id":104,"ranked_only":true}))
        .bind(json!({"ok":true,"data":{"steam_id64":null,"account_id":104,"matches":[{"match_id":501,"start_time":start.timestamp(),"match_result":0}]}}))
        .bind(finished).execute(&pool).await.unwrap();
    engine.tick(finished).await.unwrap();
    let scored: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM partner_effort_events WHERE event_type='party_play' AND partner_twitch_user_id='101' AND points>0")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(scored, 1);
    pool.close().await;
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "DROP DATABASE {name} WITH (FORCE)"
    )))
    .execute(&admin)
    .await
    .unwrap();
    admin.close().await;
}

#[tokio::test]
async fn stream_proof_ignores_reach_and_survives_snapshot_retention() {
    let (admin, pool, name) = fixture().await;
    let engine = Engine::new(
        pool.clone(),
        Challenges::default(),
        Some(pool.clone()),
        Some(idle_helix()),
    )
    .unwrap();
    let start = DateTime::parse_from_rfc3339("2026-10-26T10:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    for n in 1..=40 {
        let at = start + Duration::minutes(n);
        sqlx::query("INSERT INTO category_collection_runs(snapshot_at,completed_at,streams,viewers,poll_seconds) VALUES($1,$1,2,0,60) ON CONFLICT (snapshot_at) DO NOTHING").bind(at).execute(&pool).await.unwrap();
        for (id, login, stream, stream_start) in [
            (
                "101",
                "alice",
                if n <= 20 { "short-a" } else { "short-b" },
                if n <= 20 {
                    start
                } else {
                    start + Duration::minutes(20)
                },
            ),
            ("102", "bob", "long", start),
        ] {
            sqlx::query("INSERT INTO category_stream_snapshots(snapshot_at,stream_id,user_id,user_login,viewer_count,title,language,started_at,is_mature,sample_seconds) VALUES($1,$2,$3,$4,0,'Deadlock','de',$5,FALSE,60)")
                .bind(at).bind(stream).bind(id).bind(login).bind(stream_start).execute(&pool).await.unwrap();
        }
    }
    let now = start + Duration::minutes(41);
    for id in ["101", "102"] {
        engine
            .append(
                &event(
                    id,
                    EventKind::QualifiedInvite,
                    &format!("snapshot-invite:{id}"),
                    start,
                ),
                now,
            )
            .await
            .unwrap();
    }
    engine.tick(now).await.unwrap();
    let alice = engine.me("101", now).await.unwrap();
    let bob = engine.me("102", now).await.unwrap();
    assert_eq!(alice.streak.current, 0);
    assert_eq!(bob.streak.current, 1);
    sqlx::query("UPDATE category_stream_snapshots SET viewer_count=1000000")
        .execute(&pool)
        .await
        .unwrap();
    engine.tick(now + Duration::seconds(1)).await.unwrap();
    assert_eq!(
        engine.me("101", now).await.unwrap().level.total_points,
        alice.level.total_points
    );
    assert_eq!(
        engine.me("102", now).await.unwrap().level.total_points,
        bob.level.total_points
    );
    let before: i64 = sqlx::query_scalar("SELECT deadlock_seconds FROM partner_effort_stream_weeks WHERE partner_twitch_user_id='102' AND stream_id='long'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(before, 2400);
    sqlx::query("DELETE FROM category_stream_snapshots")
        .execute(&pool)
        .await
        .unwrap();
    engine.tick(now + Duration::seconds(2)).await.unwrap();
    assert_eq!(engine.me("102", now).await.unwrap().streak.current, 1);
    assert_eq!(
        engine.me("102", now).await.unwrap().level.total_points,
        bob.level.total_points
    );
    let next = now + Duration::minutes(3);
    sqlx::query("INSERT INTO category_collection_runs(snapshot_at,completed_at,streams,viewers,poll_seconds) VALUES($1,$1,1,0,60) ON CONFLICT (snapshot_at) DO NOTHING")
        .bind(next).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO category_stream_snapshots(snapshot_at,stream_id,user_id,user_login,viewer_count,title,language,started_at,is_mature,sample_seconds) VALUES($1,'long','102','bob',0,'Deadlock','de',$2,FALSE,60)")
        .bind(next).bind(start).execute(&pool).await.unwrap();
    engine.tick(next).await.unwrap();
    let after: i64 = sqlx::query_scalar("SELECT deadlock_seconds FROM partner_effort_stream_weeks WHERE partner_twitch_user_id='102' AND stream_id='long'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(after, before + 60);
    engine.tick(next + Duration::seconds(1)).await.unwrap();
    let repeated: i64 = sqlx::query_scalar("SELECT deadlock_seconds FROM partner_effort_stream_weeks WHERE partner_twitch_user_id='102' AND stream_id='long'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(repeated, after);
    pool.close().await;
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "DROP DATABASE {name} WITH (FORCE)"
    )))
    .execute(&admin)
    .await
    .unwrap();
    admin.close().await;
}

#[tokio::test]
async fn unattributed_invite_does_not_block_qualified_source() {
    let (admin, pool, name) = fixture().await;
    let cfg = Challenges::default();
    let now = DateTime::parse_from_rfc3339("2026-10-26T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    for (join, streamer, inviter) in [
        (301i64, None, Some("501")),
        (302i64, Some("101"), Some("not-a-twitch-id")),
    ] {
        sqlx::query("INSERT INTO bot.twitch_invite_joins(join_id,guild_id,user_id,streamer_login,streamer_twitch_user_id,inviter_twitch_user_id,invite_code,joined_at,eligible) VALUES($1,$2,$3,'alice',$4,$5,$6,$7,TRUE)")
            .bind(join).bind(cfg.community_guild_id).bind(1000+join).bind(streamer)
            .bind(inviter).bind(format!("invite:{join}"))
            .bind(now-Duration::days(15)).execute(&pool).await.unwrap();
        sqlx::query("UPDATE bot.twitch_invite_joins SET status='qualified',qualified_at=$2 WHERE join_id=$1")
            .bind(join).bind(now-Duration::minutes(1)).execute(&pool).await.unwrap();
    }
    let engine = Engine::new(pool.clone(), cfg, Some(pool.clone()), Some(idle_helix())).unwrap();
    engine.tick(now).await.unwrap();
    engine.ensure_ready(now).await.unwrap();
    let receipts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM partner_effort_source_receipts WHERE source='invites' AND source_id IN ('301','302')")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(receipts, 2);
    let events: Vec<(String, Option<String>)> = sqlx::query_as("SELECT source_id,viewer_twitch_user_id FROM partner_effort_events WHERE event_type='qualified_invite'")
        .fetch_all(&pool).await.unwrap();
    assert_eq!(events, vec![("discord-join:302".into(), None)]);
    engine.tick(now + Duration::seconds(1)).await.unwrap();
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM partner_effort_events WHERE event_type='qualified_invite'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 1);
    pool.close().await;
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "DROP DATABASE {name} WITH (FORCE)"
    )))
    .execute(&admin)
    .await
    .unwrap();
    admin.close().await;
}

#[tokio::test]
async fn technical_pause_defers_source_receipts_until_partner_is_eligible() {
    let (admin, pool, name) = fixture().await;
    let now = DateTime::parse_from_rfc3339("2026-10-26T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let cfg = Challenges::default();
    sqlx::query("UPDATE twitch_partners SET technical_pause_reason='maintenance' WHERE twitch_user_id='101'")
        .execute(&pool)
        .await
        .unwrap();
    insert_invite(
        &pool,
        601,
        cfg.community_guild_id,
        now - Duration::seconds(1),
    )
    .await;
    insert_referral(&pool, "901", now - Duration::seconds(1)).await;
    insert_contest_submission(&pool, "501").await;
    sqlx::query("INSERT INTO twitch_clip_contest_effort_outbox(id,event_type,partner_twitch_user_id,streamer_login,source_id,occurred_at,metadata) VALUES(1,'clip_submitted','101','alice','clip-901',$1,$2)")
        .bind(now - Duration::seconds(1))
        .bind(json!({"submission_id":"1"}))
        .execute(&pool)
        .await
        .unwrap();
    let engine = Engine::new(pool.clone(), cfg, Some(pool.clone()), Some(idle_helix())).unwrap();

    engine.tick(now).await.unwrap();
    let receipts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM partner_effort_source_receipts WHERE (source,source_id) IN (('invites','601'),('referrals','901'),('clips','1'))")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(receipts, 0);
    let events: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM partner_effort_events WHERE partner_twitch_user_id='101'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(events, 0);

    sqlx::query("UPDATE twitch_partners SET technical_pause_reason='' WHERE twitch_user_id='101'")
        .execute(&pool)
        .await
        .unwrap();
    engine.tick(now + Duration::seconds(1)).await.unwrap();
    let receipts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM partner_effort_source_receipts WHERE (source,source_id) IN (('invites','601'),('referrals','901'),('clips','1'))")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(receipts, 3);
    let events: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM partner_effort_events WHERE partner_twitch_user_id='101' AND event_type IN ('qualified_invite','streamer_referral','clip_submitted')")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(events, 3);
    pool.close().await;
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "DROP DATABASE {name} WITH (FORCE)"
    )))
    .execute(&admin)
    .await
    .unwrap();
    admin.close().await;
}

#[tokio::test]
async fn missing_category_coverage_blocks_settlement_and_readiness() {
    let (admin, pool, name) = fixture().await;
    let engine = Engine::new(
        pool.clone(),
        Challenges::default(),
        Some(pool.clone()),
        Some(idle_helix()),
    )
    .unwrap();
    let now = DateTime::parse_from_rfc3339("2026-10-26T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    sqlx::query("DELETE FROM category_collection_runs")
        .execute(&pool)
        .await
        .unwrap();

    assert!(engine.tick(now).await.is_err());
    assert!(engine.ensure_ready(now).await.is_err());
    let assigned: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM partner_effort_weekly_quests")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(assigned, 0);
    let state: (bool, Option<String>) = sqlx::query_as("SELECT healthy,error_code FROM partner_effort_source_state WHERE source='category_collection'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, (false, Some("source_unavailable".into())));

    sqlx::query("INSERT INTO category_collection_runs(snapshot_at,completed_at,streams,viewers,poll_seconds) VALUES($1,$1,0,0,60)")
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();
    assert!(engine.tick(now + Duration::seconds(1)).await.is_err());
    assert!(engine
        .ensure_ready(now + Duration::seconds(1))
        .await
        .is_err());
    let assigned: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM partner_effort_weekly_quests")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(assigned, 0);

    let since = DateTime::parse_from_rfc3339("2026-09-27T22:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    sqlx::query("UPDATE partner_effort_program SET started_at=$1 WHERE singleton")
        .bind(since - Duration::days(10))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM category_collection_runs")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO category_collection_runs(snapshot_at,completed_at,streams,viewers,poll_seconds) SELECT at,at,0,0,300 FROM generate_series($1::timestamptz,$2::timestamptz,INTERVAL '1 day') AS at")
        .bind(since - Duration::days(10))
        .bind(since - Duration::days(1))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO category_collection_runs(snapshot_at,completed_at,streams,viewers,poll_seconds) SELECT at,at,0,0,300 FROM generate_series($1::timestamptz,$2::timestamptz,INTERVAL '5 minutes') AS at")
        .bind(since - Duration::minutes(5))
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();
    engine.tick(now + Duration::seconds(2)).await.unwrap();
    assert!(engine
        .ensure_ready(now + Duration::seconds(2))
        .await
        .is_ok());

    pool.close().await;
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "DROP DATABASE {name} WITH (FORCE)"
    )))
    .execute(&admin)
    .await
    .unwrap();
    admin.close().await;
}

#[tokio::test]
async fn invite_quest_requires_a_join_that_can_qualify_during_its_week() {
    let (admin, pool, name) = fixture().await;
    let cfg = Challenges::default();
    sqlx::query("UPDATE twitch_partners SET status='inactive' WHERE twitch_user_id='102'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO bot.twitch_invite_joins(join_id,guild_id,user_id,streamer_login,streamer_twitch_user_id,inviter_twitch_user_id,invite_code,joined_at,eligible) VALUES(701,$1,1701,'alice','101','501','invite:701','2026-10-10T12:00:00Z',TRUE)")
        .bind(cfg.community_guild_id)
        .execute(&pool)
        .await
        .unwrap();
    let engine = Engine::new(pool.clone(), cfg, Some(pool.clone()), Some(idle_helix())).unwrap();
    let first_week = DateTime::parse_from_rfc3339("2026-10-12T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    engine.tick(first_week).await.unwrap();
    let first: Vec<String> = sqlx::query_scalar("SELECT quest_key FROM partner_effort_weekly_quests WHERE partner_twitch_user_id='101' AND week_start=$1 ORDER BY position")
        .bind(berlin_week_start(first_week))
        .fetch_all(&pool)
        .await
        .unwrap();
    assert!(!first.contains(&"active_discord_invite".into()));

    let next_week = DateTime::parse_from_rfc3339("2026-10-19T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    engine.tick(next_week).await.unwrap();
    let next: Vec<String> = sqlx::query_scalar("SELECT quest_key FROM partner_effort_weekly_quests WHERE partner_twitch_user_id='101' AND week_start=$1 ORDER BY position")
        .bind(berlin_week_start(next_week))
        .fetch_all(&pool)
        .await
        .unwrap();
    assert!(next.contains(&"active_discord_invite".into()));
    pool.close().await;
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "DROP DATABASE {name} WITH (FORCE)"
    )))
    .execute(&admin)
    .await
    .unwrap();
    admin.close().await;
}

#[tokio::test]
async fn unreachable_quest_pool_is_partner_local_before_berlin_week_rollover() {
    let (admin, pool, name) = fixture().await;
    let cfg = Challenges::default();
    let now = DateTime::parse_from_rfc3339("2026-09-27T23:59:00+02:00")
        .unwrap()
        .with_timezone(&Utc);
    sqlx::query(
        "INSERT INTO twitch_partners(twitch_user_id,twitch_login,status) \
         VALUES('104','latecomer','active')",
    )
    .execute(&pool)
    .await
    .unwrap();
    insert_invite(&pool, 777701, cfg.community_guild_id, now).await;
    let engine = Engine::new(pool.clone(), cfg, Some(pool.clone()), Some(idle_helix())).unwrap();

    engine.ensure_ready(now).await.unwrap();

    let latecomer = engine.me("104", now).await.unwrap();
    assert!(latecomer.quests.is_empty());
    assert_eq!(
        latecomer.quest_assignment_status,
        tb_effort::types::QuestAssignmentStatus::NoReachableQuests
    );
    assert!(latecomer.season.active_partners > 0);

    let healthy_partner = engine.me("101", now).await.unwrap();
    assert!(!healthy_partner.quests.is_empty());
    assert_eq!(
        healthy_partner.quest_assignment_status,
        tb_effort::types::QuestAssignmentStatus::Assigned
    );

    pool.close().await;
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "DROP DATABASE {name} WITH (FORCE)"
    )))
    .execute(&admin)
    .await
    .unwrap();
    admin.close().await;
}

#[tokio::test]
async fn source_cursors_resume_and_reconcile_late_rows_without_starving_new_events() {
    let (admin, pool, name) = fixture().await;
    let cfg = Challenges {
        source_batch_size: 2,
        ..Challenges::default()
    };
    let now = DateTime::parse_from_rfc3339("2026-10-26T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    for id in [10i64, 20, 30, 40, 50, 60] {
        insert_invite(&pool, id, cfg.community_guild_id, now - Duration::days(2)).await;
        insert_referral(&pool, &format!("90{id}"), now - Duration::days(2)).await;
    }
    let engine = Engine::new(
        pool.clone(),
        cfg.clone(),
        Some(pool.clone()),
        Some(idle_helix()),
    )
    .unwrap();
    assert!(engine.tick(now).await.is_err());
    let cursors: Vec<String>=sqlx::query_scalar("SELECT source_id FROM partner_effort_source_cursors WHERE lane='reconcile' ORDER BY source").fetch_all(&pool).await.unwrap();
    assert_eq!(cursors, vec!["20", "9020"]);
    assert!(engine.ensure_ready(now).await.is_err());
    insert_invite(&pool, 99, cfg.community_guild_id, now).await;
    let restarted = Engine::new(
        pool.clone(),
        cfg.clone(),
        Some(pool.clone()),
        Some(idle_helix()),
    )
    .unwrap();
    assert!(restarted.tick(now + Duration::seconds(1)).await.is_err());
    let cursors: Vec<String>=sqlx::query_scalar("SELECT source_id FROM partner_effort_source_cursors WHERE lane='reconcile' ORDER BY source").fetch_all(&pool).await.unwrap();
    assert_eq!(cursors, vec!["40", "9040"]);
    let fresh: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM partner_effort_events WHERE source_id='discord-join:99')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(fresh);
    let _ = restarted.tick(now + Duration::seconds(2)).await;
    restarted.tick(now + Duration::seconds(3)).await.unwrap();
    restarted
        .ensure_ready(now + Duration::seconds(3))
        .await
        .unwrap();
    insert_invite(&pool, 5, cfg.community_guild_id, now - Duration::days(3)).await;
    insert_referral(&pool, "905", now - Duration::days(3)).await;
    insert_referral(&pool, "99999999999999999999", now + Duration::seconds(4)).await;
    restarted.tick(now + Duration::seconds(4)).await.unwrap();
    let late: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM partner_effort_events WHERE source_id='discord-join:5')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(late);
    let invites: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM partner_effort_events WHERE event_type='qualified_invite'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let referrals: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM partner_effort_events WHERE event_type='streamer_referral'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(invites, 8);
    assert_eq!(referrals, 8);
    let large_id: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM partner_effort_events WHERE source_id='streamer_referral:99999999999999999999')")
        .fetch_one(&pool).await.unwrap();
    assert!(large_id);
    pool.close().await;
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "DROP DATABASE {name} WITH (FORCE)"
    )))
    .execute(&admin)
    .await
    .unwrap();
    admin.close().await;
}

async fn insert_contest_submission(pool: &PgPool, submitter: &str) {
    sqlx::query("INSERT INTO twitch_clip_contest_months(contest_month) VALUES('2026-10-01') ON CONFLICT DO NOTHING").execute(pool).await.unwrap();
    sqlx::query("INSERT INTO twitch_clip_contest_submissions(id,contest_month,twitch_clip_id,clip_url,clip_title,broadcaster_twitch_id,broadcaster_login,game_id,clip_created_at,submitter_provider,submitter_user_id,submitter_person_key,submitter_aliases,submitter_display_name,submission_slot) VALUES(1,'2026-10-01','fixture','https://clips.twitch.tv/fixture','Fixture','101','alice','509658','2026-10-20','twitch',$1,$1,ARRAY[$1],'Fixture',1)").bind(submitter).execute(pool).await.unwrap();
}

#[tokio::test]
async fn single_connection_tick_and_concurrent_readiness_keep_working() {
    let (admin, pool, name) = fixture().await;
    let one = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(pool.connect_options().as_ref().clone())
        .await
        .unwrap();
    let now = DateTime::parse_from_rfc3339("2026-10-26T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let engine = Engine::new(
        one.clone(),
        Challenges::default(),
        Some(pool.clone()),
        Some(idle_helix()),
    )
    .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(10), engine.tick(now))
        .await
        .unwrap()
        .unwrap();
    // Eine vor dem parallelen Tick begonnene Anfrage akzeptiert dessen neueren
    // erfolgreichen Quellenstand; die obere Zeitgrenze darf keinen 503 erzeugen.
    engine
        .ensure_ready(now - Duration::seconds(1))
        .await
        .unwrap();
    sqlx::query("UPDATE category_collector_status SET details='{\"disk_paused\":true}'")
        .execute(&pool)
        .await
        .unwrap();
    assert!(engine.tick(now + Duration::seconds(1)).await.is_err());
    assert!(engine
        .ensure_ready(now + Duration::seconds(1))
        .await
        .is_err());
    one.close().await;
    pool.close().await;
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "DROP DATABASE {name} WITH (FORCE)"
    )))
    .execute(&admin)
    .await
    .unwrap();
    admin.close().await;
}
