//! Hermetische Tests der Raid-Retention-Berechnung (#11, P1.24).

mod support;

use chrono::{DateTime, Duration, Utc};
use sqlx::PgPool;
use tb_monitoring::compute_raid_retention;

async fn seed_session(
    pool: &PgPool,
    login: &str,
    started: DateTime<Utc>,
    ended: Option<DateTime<Utc>>,
) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO twitch_stream_sessions (streamer_login, started_at, ended_at) \
         VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(login)
    .bind(started)
    .bind(ended)
    .fetch_one(pool)
    .await
    .unwrap()
}

async fn seed_raid(
    pool: &PgPool,
    id: i64,
    from: &str,
    to: &str,
    viewers: i32,
    executed: DateTime<Utc>,
) {
    sqlx::query(
        "INSERT INTO twitch_raid_history \
         (id, from_broadcaster_login, to_broadcaster_login, viewer_count, executed_at) \
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(id)
    .bind(from)
    .bind(to)
    .bind(viewers)
    .bind(executed)
    .execute(pool)
    .await
    .unwrap();
}

#[allow(clippy::too_many_arguments)]
async fn seed_chatter(
    pool: &PgPool,
    session: i64,
    streamer: &str,
    login: &str,
    last_seen: DateTime<Utc>,
    first_message_at: DateTime<Utc>,
    messages: i32,
) {
    sqlx::query(
        "INSERT INTO twitch_session_chatters \
         (session_id, streamer_login, chatter_login, first_message_at, messages, \
          is_first_time_streamer, seen_via_chatters_api, last_seen_at) \
         VALUES ($1, $2, $3, $4, $5, FALSE, TRUE, $6)",
    )
    .bind(session)
    .bind(streamer)
    .bind(login)
    .bind(first_message_at)
    .bind(messages)
    .bind(last_seen)
    .execute(pool)
    .await
    .unwrap();
}

async fn seed_rollup(pool: &PgPool, streamer: &str, login: &str, first_seen: DateTime<Utc>) {
    sqlx::query(
        "INSERT INTO twitch_chatter_rollup \
         (streamer_login, chatter_login, first_seen_at, last_seen_at, total_messages, total_sessions) \
         VALUES ($1, $2, $3, $3, 0, 1)",
    )
    .bind(streamer)
    .bind(login)
    .bind(first_seen)
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn windows_and_splits_computed() {
    let Some(pool) = support::pool_with_chatters_schema("t_raid_windows").await else {
        return;
    };
    let executed = Utc::now() - Duration::hours(2);
    let session = seed_session(&pool, "target", executed - Duration::minutes(5), None).await;
    seed_raid(&pool, 1, "raider", "target", 10, executed).await;

    // alice: kommt +3min und bleibt bis +30min (in allen Fenstern).
    seed_chatter(
        &pool,
        session,
        "target",
        "alice",
        executed + Duration::minutes(30),
        executed + Duration::minutes(3),
        2,
    )
    .await;
    seed_rollup(&pool, "raider", "alice", executed - Duration::days(2)).await;
    // bob: kommt +4min und bleibt bis +15min (5/15, nicht 30).
    seed_chatter(
        &pool,
        session,
        "target",
        "bob",
        executed + Duration::minutes(15),
        executed + Duration::minutes(4),
        0,
    )
    .await;
    // carol: kommt +5min und geht danach (nur 5), war schon Stammgast des targets.
    seed_chatter(
        &pool,
        session,
        "target",
        "carol",
        executed + Duration::minutes(5),
        executed + Duration::minutes(5),
        4,
    )
    .await;
    seed_rollup(&pool, "target", "carol", executed - Duration::days(3)).await;
    // nightbot: muss gefiltert werden.
    seed_chatter(
        &pool,
        session,
        "target",
        "nightbot",
        executed + Duration::minutes(2),
        executed + Duration::minutes(2),
        9,
    )
    .await;

    let stats = compute_raid_retention(&pool).await.unwrap();
    assert_eq!(stats.raids_computed, 1);

    let (at5, at15, at30, known, new_to, new_ch, tsid): (i32, i32, i32, i32, i32, i32, i32) =
        sqlx::query_as(
            "SELECT chatters_at_plus5m, chatters_at_plus15m, chatters_at_plus30m, \
                    known_from_raider, new_to_target, new_chatters, target_session_id \
             FROM twitch_raid_retention WHERE raid_id = 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(at5, 3, "+5: alice + bob + carol (nightbot gefiltert)");
    assert_eq!(at15, 2, "+15: alice + bob");
    assert_eq!(at30, 1, "+30: nur alice");
    assert_eq!(known, 1, "alice kommt aus raider-rollup");
    assert_eq!(
        new_to, 2,
        "alice + bob neu für target (carol war Stammgast)"
    );
    assert_eq!(new_ch, 1, "nur alice schreibt neu (bob ist Lurker)");
    assert_eq!(
        tsid as i64, session,
        "target_session_id aufgelöst (int4-Cast)"
    );
}

#[tokio::test]
async fn late_arrival_only_counts_in_later_retention_windows() {
    let Some(pool) = support::pool_with_chatters_schema("t_raid_late_arrival").await else {
        return;
    };
    let executed = Utc::now() - Duration::hours(2);
    let session = seed_session(&pool, "target", executed - Duration::minutes(5), None).await;
    seed_raid(&pool, 25, "raider", "target", 10, executed).await;
    seed_chatter(
        &pool,
        session,
        "target",
        "latecomer",
        executed + Duration::minutes(30),
        executed + Duration::minutes(9),
        1,
    )
    .await;

    compute_raid_retention(&pool).await.unwrap();
    let counts: (i32, i32, i32) = sqlx::query_as(
        "SELECT chatters_at_plus5m, chatters_at_plus15m, chatters_at_plus30m \
         FROM twitch_raid_retention WHERE raid_id = 25",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(counts, (0, 1, 1));
}

#[tokio::test]
async fn chatter_von_vor_dem_raid_zaehlt_nicht_zur_retention() {
    let Some(pool) = support::pool_with_chatters_schema("t_raid_preexisting").await else {
        return;
    };
    let executed = Utc::now() - Duration::hours(2);
    let session = seed_session(&pool, "target", executed - Duration::minutes(5), None).await;
    seed_raid(&pool, 24, "raider", "target", 10, executed).await;
    seed_chatter(
        &pool,
        session,
        "target",
        "organic",
        executed + Duration::minutes(30),
        executed - Duration::minutes(1),
        1,
    )
    .await;

    compute_raid_retention(&pool).await.unwrap();
    let counts: (i32, i32, i32) = sqlx::query_as(
        "SELECT chatters_at_plus5m, chatters_at_plus15m, chatters_at_plus30m \
         FROM twitch_raid_retention WHERE raid_id = 24",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(counts, (0, 0, 0));
}

#[tokio::test]
async fn late_seen_counted_in_known_and_new_to_target() {
    // Python zählt known_from_raider / new_to_target NUR mit Untergrenze
    // (last_seen_at >= executed_at, KEINE +30min-Obergrenze). Ein Zuschauer der
    // erst +45min zuletzt gesehen wird muss daher mitzählen.
    let Some(pool) = support::pool_with_chatters_schema("t_raid_late_seen").await else {
        return;
    };
    let executed = Utc::now() - Duration::hours(2);
    let session = seed_session(&pool, "target", executed - Duration::minutes(5), None).await;
    seed_raid(&pool, 21, "raider", "target", 10, executed).await;

    // dave: kommt im Raid-Fenster an und ist bei +45min weiterhin da.
    seed_chatter(
        &pool,
        session,
        "target",
        "dave",
        executed + Duration::minutes(45),
        executed + Duration::minutes(2),
        0,
    )
    .await;
    seed_rollup(&pool, "raider", "dave", executed - Duration::days(2)).await;

    let stats = compute_raid_retention(&pool).await.unwrap();
    assert_eq!(stats.raids_computed, 1);

    let (at30, known, new_to): (i32, i32, i32) = sqlx::query_as(
        "SELECT chatters_at_plus30m, known_from_raider, new_to_target \
         FROM twitch_raid_retention WHERE raid_id = 21",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(
        at30, 1,
        "+30-Retention zählt dave auch bei last_seen +45min"
    );
    assert_eq!(
        known, 1,
        "known_from_raider hat KEINE Obergrenze → dave zählt"
    );
    assert_eq!(new_to, 1, "new_to_target hat KEINE Obergrenze → dave zählt");
}

#[tokio::test]
async fn known_from_raider_requires_rollup_before_raid() {
    let Some(pool) = support::pool_with_chatters_schema("t_raid_known_before").await else {
        return;
    };
    let executed = Utc::now() - Duration::hours(2);
    let session = seed_session(&pool, "target", executed - Duration::minutes(5), None).await;
    seed_raid(&pool, 23, "raider", "target", 10, executed).await;

    seed_chatter(
        &pool,
        session,
        "target",
        "futurefan",
        executed + Duration::minutes(3),
        executed + Duration::minutes(3),
        1,
    )
    .await;
    seed_rollup(&pool, "raider", "futurefan", executed).await;

    let stats = compute_raid_retention(&pool).await.unwrap();
    assert_eq!(stats.raids_computed, 1);

    let known: i32 = sqlx::query_scalar(
        "SELECT known_from_raider FROM twitch_raid_retention WHERE raid_id = 23",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(
        known, 0,
        "first_seen_at >= executed_at zählt nicht als known_from_raider"
    );
}

#[tokio::test]
async fn new_chatter_independent_of_last_seen() {
    // Python new_chatters hat GAR KEINE last_seen_at-Bedingung — nur
    // first_message_at >= executed_at AND messages > 0 (+ not-in-rollup-of-to).
    let Some(pool) = support::pool_with_chatters_schema("t_raid_new_chatter").await else {
        return;
    };
    let executed = Utc::now() - Duration::hours(2);
    let session = seed_session(&pool, "target", executed - Duration::minutes(5), None).await;
    seed_raid(&pool, 22, "raider", "target", 10, executed).await;

    // erin: schreibt erstmals +10min (messages>0) und ist bei +50min noch da.
    seed_chatter(
        &pool,
        session,
        "target",
        "erin",
        executed + Duration::minutes(50),
        executed + Duration::minutes(10),
        3,
    )
    .await;

    let stats = compute_raid_retention(&pool).await.unwrap();
    assert_eq!(stats.raids_computed, 1);

    let (at30, new_ch): (i32, i32) = sqlx::query_as(
        "SELECT chatters_at_plus30m, new_chatters \
         FROM twitch_raid_retention WHERE raid_id = 22",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(at30, 1, "+30-Retention zählt erin bei last_seen +50min");
    assert_eq!(
        new_ch, 1,
        "new_chatters ignoriert last_seen_at → erin (first_message +10, messages>0) zählt"
    );
}

#[tokio::test]
async fn skip_if_already_computed() {
    let Some(pool) = support::pool_with_chatters_schema("t_raid_skip").await else {
        return;
    };
    let executed = Utc::now() - Duration::hours(1);
    let session = seed_session(&pool, "target", executed - Duration::minutes(1), None).await;
    seed_raid(&pool, 5, "raider", "target", 3, executed).await;
    seed_chatter(
        &pool,
        session,
        "target",
        "alice",
        executed + Duration::minutes(2),
        executed + Duration::minutes(2),
        1,
    )
    .await;

    // Bereits berechnete Zeile (manuell), abweichende Werte → muss UNBERÜHRT bleiben.
    sqlx::query(
        "INSERT INTO twitch_raid_retention \
         (raid_id, from_broadcaster_login, to_broadcaster_login, viewer_count_sent, \
          executed_at, target_session_id, chatters_at_plus5m) \
         VALUES (5, 'raider', 'target', 3, $1, $2, 999)",
    )
    .bind(executed)
    .bind(session as i32)
    .execute(&pool)
    .await
    .unwrap();

    let stats = compute_raid_retention(&pool).await.unwrap();
    assert_eq!(stats.raids_skipped_existing, 1);
    assert_eq!(stats.raids_computed, 0);

    let at5: i32 =
        sqlx::query_scalar("SELECT chatters_at_plus5m FROM twitch_raid_retention WHERE raid_id=5")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(at5, 999, "DO NOTHING — bestehende Zeile bleibt fix");
}

#[tokio::test]
async fn skip_if_no_target_session() {
    let Some(pool) = support::pool_with_chatters_schema("t_raid_nosession").await else {
        return;
    };
    let executed = Utc::now() - Duration::hours(1);
    // Session des targets endete VOR dem Raid → keine passende Session.
    seed_session(
        &pool,
        "target",
        executed - Duration::hours(3),
        Some(executed - Duration::hours(2)),
    )
    .await;
    seed_raid(&pool, 9, "raider", "target", 5, executed).await;

    let stats = compute_raid_retention(&pool).await.unwrap();
    assert_eq!(stats.raids_skipped_no_session, 1);
    assert_eq!(stats.raids_computed, 0);
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_raid_retention")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

#[tokio::test]
async fn old_raids_outside_7d_ignored() {
    let Some(pool) = support::pool_with_chatters_schema("t_raid_old").await else {
        return;
    };
    let executed = Utc::now() - Duration::days(10);
    seed_session(&pool, "target", executed - Duration::minutes(1), None).await;
    seed_raid(&pool, 11, "raider", "target", 5, executed).await;

    let stats = compute_raid_retention(&pool).await.unwrap();
    assert_eq!(stats.raids_scanned, 0, "10 Tage alt → außerhalb 7d-Fenster");
    assert_eq!(stats.raids_computed, 0);
}
