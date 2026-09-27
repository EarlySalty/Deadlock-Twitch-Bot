use chrono::{DateTime, Duration, Utc};
use serde_json::json;
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    PgPool,
};
use std::str::FromStr;
use tb_config::challenges::Challenges;
use tb_effort::{
    calendar::{berlin_week_start, midnight},
    Engine, Event, EventKind,
};

async fn fixture() -> (PgPool, PgPool, String) {
    let dsn = std::env::var("TB_TEST_DATABASE_URL")
        .expect("TB_TEST_DATABASE_URL must point to the disposable test container");
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
    sqlx::query(&format!("CREATE DATABASE {name}"))
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
    sqlx::raw_sql("CREATE SCHEMA activity; CREATE SCHEMA core; CREATE SCHEMA bot; CREATE SCHEMA steam;
        CREATE TABLE core.steam_links(discord_id bigint,steam_id text,verified boolean);
        CREATE TABLE bot.twitch_streamer_invites(streamer_login text,guild_id bigint,twitch_user_id text);
        CREATE TABLE activity.twitch_invite_qualifications(join_event_id bigint PRIMARY KEY,user_id bigint,guild_id bigint,streamer_login text,inviter_twitch_user_id text,status text,qualified_at timestamptz);
        CREATE TABLE activity.streamer_referral_credits(join_event_id bigint PRIMARY KEY,inviter_twitch_user_id text,invited_twitch_user_id text,guild_id bigint,credited_at timestamptz);
        CREATE TABLE activity.voice_session_log(id bigint PRIMARY KEY,user_id bigint,guild_id bigint,channel_id bigint,started_at timestamptz,ended_at timestamptz);
        CREATE TABLE steam.steam_tasks(id bigint PRIMARY KEY,type text,payload jsonb,status text,result jsonb,finished_at timestamptz);
        CREATE TABLE twitch_clip_contest_submissions(id bigint PRIMARY KEY,contest_month date,submitter_provider text,submitter_user_id text,broadcaster_twitch_id text);
        CREATE TABLE twitch_clip_contest_effort_outbox(id bigint PRIMARY KEY,event_type text,source_id text,occurred_at timestamptz,metadata jsonb);
        INSERT INTO twitch_partners(twitch_user_id,twitch_login,status) VALUES('101','alice','active'),('102','bob','active'),('103','inactive','inactive');")
        .execute(&pool).await.unwrap();
    (admin, pool, name)
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
    let engine = Engine::new(pool.clone(), cfg.clone(), Some(pool.clone()), None).unwrap();
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

    sqlx::query("INSERT INTO bot.twitch_streamer_invites VALUES('alice',$1,'101')")
        .bind(cfg.community_guild_id)
        .execute(&pool)
        .await
        .unwrap();
    for n in [10i64, 11] {
        sqlx::query("INSERT INTO activity.twitch_invite_qualifications VALUES($1,$2,$3,'alice','501','qualified',$4)")
            .bind(n).bind(1000+n).bind(cfg.community_guild_id).bind(now-Duration::minutes(30)).execute(&pool).await.unwrap();
    }
    sqlx::query("INSERT INTO activity.streamer_referral_credits VALUES(99,'101','102',$1,$2)")
        .bind(cfg.community_guild_id)
        .bind(now - Duration::minutes(20))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO twitch_clip_contest_submissions VALUES(1,'2026-10-01','twitch','101','101')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO twitch_clip_contest_effort_outbox VALUES(1,'clip_top3','outbox:top3', $1, $2)",
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
    assert_eq!(later.season.points, me.season.points + 5);

    sqlx::query("UPDATE partner_effort_source_state SET healthy=FALSE WHERE source='invites'")
        .execute(&pool)
        .await
        .unwrap();
    assert!(engine.ensure_ready(now).await.is_err());
    pool.close().await;
    assert!(engine
        .append(
            &event("101", EventKind::QualifiedInvite, "after-close", now),
            now
        )
        .await
        .is_err());
    assert!(engine.me("101", now).await.is_err());
    sqlx::query(&format!("DROP DATABASE {name}"))
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
        INSERT INTO core.steam_links VALUES(1001,'76561197960265801',TRUE),(1002,'76561197960265802',TRUE);")
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
    for steam in ["76561197960265801", "76561197960265802"] {
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
    for (id, steam, result) in [(1i64, "76561197960265801", 1), (2, "76561197960265802", 2)] {
        sqlx::query("INSERT INTO steam.steam_tasks VALUES($1,'GC_GET_MATCH_HISTORY',$2,'DONE',$3,$4)")
            .bind(id).bind(json!({"steam_id":steam})).bind(json!({"ok":true,"data":{"steam_id64":steam,"matches":[{"match_id":500,"start_time":start.timestamp(),"match_result":result}]}})).bind(finished).execute(&pool).await.unwrap();
    }
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

    pool.close().await;
    sqlx::query(&format!("DROP DATABASE {name}"))
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
        None,
    )
    .unwrap();
    let start = DateTime::parse_from_rfc3339("2026-10-26T10:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    for n in 1..=40 {
        let at = start + Duration::minutes(n);
        sqlx::query("INSERT INTO category_collection_runs(snapshot_at,completed_at,streams,viewers,poll_seconds) VALUES($1,$1,2,0,60)").bind(at).execute(&pool).await.unwrap();
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
    sqlx::query("DELETE FROM category_collection_runs")
        .execute(&pool)
        .await
        .unwrap();
    engine.tick(now + Duration::seconds(2)).await.unwrap();
    assert_eq!(engine.me("102", now).await.unwrap().streak.current, 1);
    assert_eq!(
        engine.me("102", now).await.unwrap().level.total_points,
        bob.level.total_points
    );
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE {name}"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}
