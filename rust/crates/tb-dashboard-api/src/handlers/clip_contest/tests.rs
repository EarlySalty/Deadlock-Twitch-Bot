use super::*;
use base64::Engine;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use std::str::FromStr;

mod test_database {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../test-support/database.rs"
    ));
}

fn instant(raw: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(raw)
        .unwrap()
        .with_timezone(&Utc)
}

#[test]
fn boundaries_include_berlin_dst_leap_year_and_year_change() {
    for (before, after) in [
        ("2026-09-21T21:59:59Z", "2026-09-21T22:00:00Z"),
        ("2026-12-21T22:59:59Z", "2026-12-21T23:00:00Z"),
    ] {
        assert_eq!(contest_clock(instant(before)).phase, Phase::Submission);
        assert_eq!(contest_clock(instant(after)).phase, Phase::Voting);
    }
    assert_eq!(
        contest_clock(instant("2026-12-31T23:00:00Z"))
            .month
            .to_string(),
        "2027-01-01"
    );
    assert_eq!(
        contest_clock(instant("2028-02-29T22:59:59Z")).phase,
        Phase::Voting
    );
    let before = contest_clock(instant("2026-09-30T21:59:59Z"));
    let after = contest_clock(instant("2026-09-30T22:00:00Z"));
    assert!(!phase_matches(&before, &after, Phase::Voting));
}

#[test]
fn clip_urls_reject_credentials_ports_fake_hosts_and_extra_segments() {
    for url in [
        "https://clips.twitch.tv@evil.test/Clip123",
        "https://evil.test/?clip=123",
        "https://clips.twitch.tv:444/Clip123",
        "https://u:p@clips.twitch.tv/Clip123",
        "https://clips.twitch.tv/Clip123/extra",
        "https://www.twitch.tv/a/clip/Clip123/extra",
        "javascript:alert(1)",
        "https://clips.twitch.tv.evil.test/Clip123",
    ] {
        assert!(clip_id_from_url(url).is_none(), "accepted {url}");
    }
    assert_eq!(
        clip_id_from_url("https://clips.twitch.tv/Clip123?tt_content=url").as_deref(),
        Some("Clip123")
    );
}

#[test]
fn write_origin_is_required_and_cross_site_is_rejected() {
    let mut headers = HeaderMap::new();
    assert!(!valid_write_origin(&headers));
    headers.insert(
        "host",
        HeaderValue::from_static("deutsche-deadlock-community.de"),
    );
    headers.insert("x-forwarded-proto", HeaderValue::from_static("https"));
    headers.insert(
        "origin",
        HeaderValue::from_static("https://deutsche-deadlock-community.de"),
    );
    assert!(valid_write_origin(&headers));
    headers.insert("sec-fetch-site", HeaderValue::from_static("cross-site"));
    assert!(!valid_write_origin(&headers));
}

async fn fixture() -> Option<(PgPool, PgPool, String)> {
    if !test_database::required() {
        eprintln!("clip contest PostgreSQL test not run: TB_TEST_REQUIRE_DB=1 required");
        return None;
    }
    let dsn = test_database::database_url().expect("disposable test DSN required");
    let admin = PgPool::connect(&dsn)
        .await
        .expect("test PostgreSQL must be reachable");
    let suffix: String = tb_crypto::random_urlsafe_token(12)
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect();
    let schema = format!("clip_contest_{}", suffix.to_ascii_lowercase());
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .execute(&admin)
        .await
        .unwrap();
    let options = PgConnectOptions::from_str(&dsn)
        .unwrap()
        .options([("search_path", schema.as_str())]);
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .connect_with(options)
        .await
        .unwrap();
    sqlx::raw_sql("CREATE TABLE twitch_streamers (id BIGSERIAL PRIMARY KEY,twitch_login TEXT UNIQUE,twitch_user_id TEXT);
        CREATE TABLE twitch_clips_social_media (id BIGSERIAL PRIMARY KEY,clip_id TEXT UNIQUE,
        clip_url TEXT,clip_title TEXT,clip_thumbnail_url TEXT,streamer_login TEXT,twitch_user_id TEXT,
        created_at TIMESTAMPTZ,duration_seconds DOUBLE PRECISION,view_count INTEGER,game_name TEXT,
        game_id TEXT,category_key TEXT,status TEXT,vod_id TEXT,vod_offset_s INTEGER,layout_override_json JSONB);
        CREATE TABLE twitch_streamer_identities (twitch_user_id TEXT,discord_user_id TEXT);
        CREATE TABLE dashboard_sessions (session_id TEXT PRIMARY KEY,session_type TEXT NOT NULL,
        payload_enc BYTEA NOT NULL,created_at DOUBLE PRECISION NOT NULL,expires_at DOUBLE PRECISION NOT NULL);")
        .execute(&pool).await.unwrap();
    let migration = include_str!("../../../../../migrations/20260927023000_clip_contest.sql")
        .replace("public.", &format!("{schema}."));
    sqlx::raw_sql(sqlx::AssertSqlSafe(&migration))
        .execute(&pool)
        .await
        .unwrap();
    Some((pool, admin, schema))
}

fn identity(id: &str) -> SubmitIdentity {
    SubmitIdentity {
        provider: "discord",
        user_id: id.into(),
        person_key: format!("discord:{id}"),
        aliases: vec![format!("discord:{id}")],
        display_name: format!("Test {id}"),
    }
}

fn clip(index: i64) -> HelixClip {
    HelixClip {
        clip_id: format!("SyntheticClip{index}"),
        url: format!("https://clips.twitch.tv/SyntheticClip{index}"),
        title: format!("Prüfclip {index}"),
        thumbnail_url: None,
        broadcaster_id: "900".into(),
        broadcaster_name: "Prüfkanal".into(),
        creator_id: Some("901".into()),
        game_id: DEADLOCK_GAME_ID_FALLBACK.into(),
        created_at: Utc::now() - Duration::days(1),
        duration_seconds: 30.0,
        view_count: 0,
        vod_id: None,
        vod_offset_s: None,
    }
}

#[test]
fn exact_sixty_day_age_is_allowed_but_future_and_older_are_not() {
    let now = instant("2026-09-20T12:00:00Z");
    let mut c = clip(1);
    c.created_at = now - Duration::days(60);
    assert!(validate_clip_age(&c, now).is_ok());
    c.created_at -= Duration::milliseconds(1);
    assert!(validate_clip_age(&c, now).is_err());
    c.created_at = now + Duration::milliseconds(1);
    assert!(validate_clip_age(&c, now).is_err());
}

#[tokio::test]
async fn postgres_quotas_identity_audit_finalization_and_session_isolation() {
    let Some((pool, admin, schema)) = fixture().await else {
        return;
    };
    let month = NaiveDate::from_ymd_opt(2020, 1, 1).unwrap();
    sqlx::query("INSERT INTO twitch_clip_contest_months (contest_month) VALUES ($1)")
        .bind(month)
        .execute(&pool)
        .await
        .unwrap();
    let mut ids = Vec::new();
    for n in 1..=8i64 {
        let shared: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media (clip_id) VALUES ($1) RETURNING id",
        )
        .bind(format!("SyntheticClip{n}"))
        .fetch_one(&pool)
        .await
        .unwrap();
        let mut tx = pool.begin().await.unwrap();
        lock_month(&mut tx, month).await.unwrap();
        let owner = if n <= 3 {
            identity("100")
        } else {
            identity(&format!("10{n}"))
        };
        ids.push(
            persist_submission(
                &mut tx,
                month,
                &owner,
                &clip(n),
                "testchannel",
                shared,
                Utc::now() + Duration::days(1),
            )
            .await
            .unwrap(),
        );
        tx.commit().await.unwrap();
    }
    let mut tx = pool.begin().await.unwrap();
    lock_month(&mut tx, month).await.unwrap();
    assert_eq!(
        persist_submission(
            &mut tx,
            month,
            &identity("100"),
            &clip(9),
            "testchannel",
            1,
            Utc::now() + Duration::days(1)
        )
        .await
        .unwrap_err()
        .status(),
        StatusCode::TOO_MANY_REQUESTS
    );
    tx.rollback().await.unwrap();
    // A later verified link still counts the historical submissions against the quota.
    let linked = SubmitIdentity {
        provider: "twitch",
        user_id: "500".into(),
        person_key: "discord:100".into(),
        aliases: vec!["discord:100".into(), "twitch:500".into()],
        display_name: "Linked".into(),
    };
    assert_eq!(
        submissions_used(&pool, month, &linked.aliases)
            .await
            .unwrap(),
        3
    );
    let mut tx = pool.begin().await.unwrap();
    lock_month(&mut tx, month).await.unwrap();
    assert_eq!(
        persist_vote(
            &mut tx,
            month,
            "100",
            &linked.aliases,
            ids[0],
            Utc::now() + Duration::days(1)
        )
        .await
        .unwrap_err()
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        persist_vote(
            &mut tx,
            month,
            "700",
            &["twitch:900".into()],
            ids[0],
            Utc::now() + Duration::days(1)
        )
        .await
        .unwrap_err()
        .status(),
        StatusCode::FORBIDDEN
    );
    tx.rollback().await.unwrap();
    // Eight parallel attempts, but only five committed votes for the same account.
    let mut expired_tx = pool.begin().await.unwrap();
    lock_month(&mut expired_tx, month).await.unwrap();
    let expired = Utc::now() - Duration::seconds(1);
    assert_eq!(
        persist_vote(
            &mut expired_tx,
            month,
            "850",
            &["discord:850".into()],
            ids[0],
            expired
        )
        .await
        .unwrap_err()
        .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        persist_submission(
            &mut expired_tx,
            month,
            &identity("999"),
            &clip(99),
            "testchannel",
            1,
            expired
        )
        .await
        .unwrap_err()
        .status(),
        StatusCode::CONFLICT
    );
    expired_tx.commit().await.unwrap();
    let late_votes: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM twitch_clip_contest_votes WHERE voter_discord_id='850'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(late_votes, 0);
    let mut tasks = tokio::task::JoinSet::new();
    for id in ids.iter().copied() {
        let pool = pool.clone();
        tasks.spawn(async move {
            let mut tx = pool.begin().await.unwrap();
            lock_month(&mut tx, month).await.unwrap();
            match persist_vote(
                &mut tx,
                month,
                "800",
                &["discord:800".into()],
                id,
                Utc::now() + Duration::days(1),
            )
            .await
            {
                Ok(_) => {
                    tx.commit().await.unwrap();
                    true
                }
                Err(response) => {
                    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
                    false
                }
            }
        });
    }
    let mut successes = 0;
    while let Some(result) = tasks.join_next().await {
        successes += usize::from(result.unwrap());
    }
    assert_eq!(successes, 5);
    let voted: i64 =
        sqlx::query_scalar("SELECT submission_id FROM twitch_clip_contest_votes LIMIT 1")
            .fetch_one(&pool)
            .await
            .unwrap();
    let mut tx = pool.begin().await.unwrap();
    lock_month(&mut tx, month).await.unwrap();
    assert_eq!(
        persist_vote(
            &mut tx,
            month,
            "800",
            &["discord:800".into()],
            voted,
            Utc::now() + Duration::days(1)
        )
        .await
        .unwrap_err()
        .status(),
        StatusCode::CONFLICT
    );
    tx.rollback().await.unwrap();
    let before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_clip_contest_votes")
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE twitch_clip_contest_submissions SET hidden_at=now(),hidden_by='test-admin' WHERE id=$1").bind(voted).execute(&pool).await.unwrap();
    assert!(
        sqlx::query("DELETE FROM twitch_clip_contest_votes WHERE submission_id=$1")
            .bind(voted)
            .execute(&pool)
            .await
            .is_err()
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM twitch_clip_contest_votes")
            .fetch_one(&pool)
            .await
            .unwrap(),
        before
    );
    let (a, b) = tokio::join!(finalize_month(&pool, month), finalize_month(&pool, month));
    a.unwrap();
    b.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM twitch_clip_contest_hall_of_fame")
            .fetch_one(&pool)
            .await
            .unwrap(),
        3
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM twitch_clip_contest_effort_outbox")
            .fetch_one(&pool)
            .await
            .unwrap(),
        11
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM twitch_clip_contest_hall_of_fame WHERE submission_id=$1"
        )
        .bind(voted)
        .fetch_one(&pool)
        .await
        .unwrap(),
        0
    );
    sqlx::query("UPDATE twitch_clip_contest_submissions SET hidden_at=NULL WHERE id=$1")
        .bind(voted)
        .execute(&pool)
        .await
        .unwrap();
    finalize_month(&pool, month).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM twitch_clip_contest_effort_outbox")
            .fetch_one(&pool)
            .await
            .unwrap(),
        11
    );
    let empty = NaiveDate::from_ymd_opt(2020, 2, 1).unwrap();
    sqlx::query("INSERT INTO twitch_clip_contest_months (contest_month) VALUES ($1)")
        .bind(empty)
        .execute(&pool)
        .await
        .unwrap();
    finalize_month(&pool, empty).await.unwrap();
    assert!(sqlx::query_scalar::<_, bool>(
        "SELECT finalized_at IS NOT NULL FROM twitch_clip_contest_months WHERE contest_month=$1"
    )
    .bind(empty)
    .fetch_one(&pool)
    .await
    .unwrap());
    // Same outbox columns as the effort-engine consumer, with authoritative channel ID.
    let rows=sqlx::query("SELECT event_type,streamer_login,source_id,occurred_at,metadata::text AS metadata FROM twitch_clip_contest_effort_outbox ORDER BY occurred_at,id")
        .fetch_all(&pool).await.unwrap();
    assert_eq!(rows.len(), 11);
    assert!(
        sqlx::query("UPDATE twitch_clip_contest_effort_outbox SET streamer_login='changed'")
            .execute(&pool)
            .await
            .is_err()
    );
    // Exact clip-consumer query from PR #997 (07f75b4), exercised against our migration.
    sqlx::query("CREATE TABLE partner_effort_source_receipts (source TEXT, source_id TEXT)")
        .execute(&pool)
        .await
        .unwrap();
    let events = sqlx::query("SELECT o.id,o.event_type,o.source_id,o.occurred_at,o.metadata,s.broadcaster_twitch_id FROM twitch_clip_contest_effort_outbox o JOIN twitch_clip_contest_submissions s ON s.id=(o.metadata->>'submission_id')::bigint WHERE o.occurred_at <= $1 AND NOT EXISTS(SELECT 1 FROM partner_effort_source_receipts r WHERE r.source='clips' AND r.source_id=o.id::text) ORDER BY o.occurred_at,o.id LIMIT $2")
        .bind(Utc::now() + Duration::seconds(1)).bind(100i64).fetch_all(&pool).await.unwrap();
    assert_eq!(events.len(), 11);
    for event in events {
        assert_eq!(event.get::<String, _>("broadcaster_twitch_id"), "900");
        let metadata: Value = event.get("metadata");
        assert_eq!(metadata["partner_twitch_user_id"], "900");
        assert!(metadata["submission_id"].as_i64().unwrap() > 0);
        if event.get::<String, _>("event_type") == "clip_top3" {
            assert!((1..=3).contains(&metadata["rank"].as_i64().unwrap()));
        }
    }
    let key = base64::engine::general_purpose::URL_SAFE.encode([7u8; 32]);
    let state = DashboardAuthState::new(pool.clone(), key);
    let session = state
        .create_clip_contest_session("twitch", "500", "Zuschauer")
        .await
        .unwrap();
    let loaded = state
        .load_clip_contest_session(&session.session_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.0, "twitch");
    assert_eq!(loaded.1, "500");
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM dashboard_sessions WHERE session_id=$1")
            .bind(session_lookup_key(&session.session_id))
            .fetch_one(&pool)
            .await
            .unwrap(),
        1,
        "shared session store hashes exactly once"
    );
    let revoke = state
        .create_clip_contest_session("discord", "501", "Logout check")
        .await
        .unwrap();
    sqlx::query(
        "DELETE FROM dashboard_sessions WHERE session_type='clip_contest' AND session_id=$1",
    )
    .bind(session_lookup_key(&revoke.session_id))
    .execute(&pool)
    .await
    .unwrap();
    assert!(state
        .load_clip_contest_session(&revoke.session_id)
        .await
        .unwrap()
        .is_none());
    // A viewer identity does not become a dashboard/partner identity, even with the hash.
    assert!(state
        .load_partner_session(&session.session_id)
        .await
        .unwrap()
        .is_none());
    assert!(state
        .load_partner_session(&session_lookup_key(&session.session_id))
        .await
        .unwrap()
        .is_none());
    sqlx::query("UPDATE dashboard_sessions SET expires_at=0")
        .execute(&pool)
        .await
        .unwrap();
    assert!(state
        .load_clip_contest_session(&session.session_id)
        .await
        .unwrap()
        .is_none());
    pool.close().await;
    sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}

#[tokio::test]
async fn restart_finalizes_fully_missed_empty_months() {
    let Some((pool, admin, schema)) = fixture().await else {
        return;
    };
    let current = contest_clock(Utc::now()).month;
    let first = current.checked_sub_months(chrono::Months::new(3)).unwrap();
    sqlx::query("INSERT INTO twitch_clip_contest_months (contest_month) VALUES ($1)")
        .bind(first)
        .execute(&pool)
        .await
        .unwrap();
    finalize_pending(&pool).await.unwrap();
    finalize_pending(&pool).await.unwrap();
    let closed: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM twitch_clip_contest_months WHERE finalized_at IS NOT NULL",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(closed, 3);
    pool.close().await;
    sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
        .execute(&admin)
        .await
        .unwrap();
}

#[test]
fn discord_age_membership_and_live_rejoin_boundaries() {
    let now = instant("2026-09-25T12:00:00Z");
    let snowflake = |created: DateTime<Utc>| {
        (((created.timestamp_millis() as u64 - 1_420_070_400_000) << 22) + 1).to_string()
    };
    let id = snowflake(now - Duration::days(30));
    let joined = Some(now - Duration::days(7));
    let synced = Some(now - Duration::hours(1));
    let eligible = membership_eligibility_at(&id, now, joined, true, synced, None).unwrap();
    assert!(membership_eligibility_at(
        &id,
        now,
        joined,
        true,
        None,
        Some(("join".into(), now - Duration::days(20)))
    )
    .is_err());
    assert!(eligible.account_age_ok && eligible.member_age_ok);
    let young = snowflake(now - Duration::days(30) + Duration::milliseconds(1));
    assert!(
        !membership_eligibility_at(&young, now, joined, true, synced, None)
            .unwrap()
            .account_age_ok
    );
    assert!(
        !membership_eligibility_at(
            &id,
            now,
            Some(now - Duration::days(7) + Duration::milliseconds(1)),
            true,
            synced,
            None
        )
        .unwrap()
        .member_age_ok
    );
    assert!(
        !membership_eligibility_at(
            &id,
            now,
            joined,
            true,
            synced,
            Some(("leave".into(), now - Duration::minutes(5)))
        )
        .unwrap()
        .present
    );
    assert!(
        !membership_eligibility_at(
            &id,
            now,
            joined,
            true,
            synced,
            Some(("join".into(), now - Duration::minutes(1)))
        )
        .unwrap()
        .member_age_ok
    );
    assert!(membership_eligibility_at(
        &id,
        now,
        joined,
        true,
        Some(now - Duration::hours(27)),
        None
    )
    .is_err());
}
