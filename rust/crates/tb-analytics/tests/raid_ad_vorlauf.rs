use chrono::{Duration, Utc};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use std::str::FromStr;
use tb_analytics::ad_manager::raid_vorlauf::RaidAdVorlauf;

#[path = "../../../test-support/database.rs"]
mod test_database;

async fn wait_for_receiver_lock(pool: &sqlx::PgPool, receiver: &str) {
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        loop {
            let waiting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_locks WHERE locktype='advisory' AND objsubid=1 AND NOT granted AND database=(SELECT oid FROM pg_database WHERE datname=current_database()) AND classid::bigint=((hashtextextended('raid-ad-vorlauf:' || $1::text,0) >> 32) & 4294967295) AND objid::bigint=(hashtextextended('raid-ad-vorlauf:' || $1::text,0) & 4294967295))")
                .bind(receiver).fetch_one(pool).await.unwrap();
            if waiting { return; }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    }).await.expect("Der konkurrierende Writer muss auf den Empfängerlock warten");
}

#[tokio::test]
async fn spaete_writes_ankunft_neustart_und_parallele_versuche_bleiben_monoton() {
    let Some(dsn) = test_database::database_url() else {
        assert!(!test_database::required(), "Testdatenbank ist erforderlich");
        return;
    };
    let schema = format!("t_raid_ad_vorlauf_{}", std::process::id());
    let admin = PgPoolOptions::new().max_connections(1).connect(&dsn).await.unwrap();
    sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))).execute(&admin).await.unwrap();
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}"))).execute(&admin).await.unwrap();
    let options = PgConnectOptions::from_str(&dsn).unwrap().options([("search_path", schema.as_str())]);
    let pool = PgPoolOptions::new().max_connections(4).connect_with(options).await.unwrap();
    sqlx::query("CREATE TABLE twitch_raid_arrival_tracking(from_broadcaster_id TEXT,to_broadcaster_id TEXT NOT NULL,detected_at TIMESTAMPTZ NOT NULL)").execute(&pool).await.unwrap();
    let migration = include_str!("../../../migrations/20261003010100_raid_ad_vorlauf.sql").replace("public.", &format!("{schema}."));
    sqlx::raw_sql(sqlx::AssertSqlSafe(migration)).execute(&pool).await.unwrap();
    let registry = RaidAdVorlauf::new(pool.clone());
    let now = chrono::DateTime::from_timestamp_micros(Utc::now().timestamp_micros()).unwrap();
    let pending = registry.announce("failed", "1", "2", now);
    let failed = registry.complete("failed", false, now).unwrap();
    registry.persist(&failed).await.unwrap();
    registry.persist(&pending).await.unwrap();
    assert_eq!(registry.active_until("2", now).await.unwrap(), None);

    let pending = registry.announce("arrives", "3", "2", now);
    let started = registry.complete("arrives", true, now + Duration::seconds(1)).unwrap();
    registry.persist(&pending).await.unwrap();
    sqlx::query("INSERT INTO twitch_raid_arrival_tracking VALUES('3','2',$1)").bind(now + Duration::seconds(2)).execute(&pool).await.unwrap();
    registry.persist(&started).await.unwrap();
    let status: String = sqlx::query_scalar("SELECT status FROM twitch_raid_ad_vorlauf WHERE attempt_id='arrives'").fetch_one(&pool).await.unwrap();
    assert_eq!(status, "arrived");

    // Arrival schreibt zuerst und hält ihren Triggerlock bis zum Commit.
    // Der echte Persistpfad muss danach einen neuen Snapshot verwenden.
    let target = format!("arrival_first_{}", std::process::id());
    let pending = registry.announce("arrival_first", "6", &target, now);
    let mut arrival_tx = pool.begin().await.unwrap();
    sqlx::query("INSERT INTO twitch_raid_arrival_tracking VALUES('6',$1,$2)")
        .bind(&target).bind(now + Duration::seconds(2)).execute(&mut *arrival_tx).await.unwrap();
    let blocked_registry = RaidAdVorlauf::new(pool.clone());
    let blocked_writer = tokio::spawn(async move { blocked_registry.persist(&pending).await });
    wait_for_receiver_lock(&pool, &target).await;
    arrival_tx.commit().await.unwrap();
    blocked_writer.await.unwrap().unwrap();
    let status: String = sqlx::query_scalar("SELECT status FROM twitch_raid_ad_vorlauf WHERE attempt_id='arrival_first'")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(status, "arrived");

    // Registrierung schreibt zuerst; der Arrivaltrigger muss auf den noch
    // offenen Abschlusswrite warten und diesen danach terminalisieren.
    let target = format!("registration_first_{}", std::process::id());
    let pending = registry.announce("registration_first", "7", &target, now);
    registry.persist(&pending).await.unwrap();
    let mut registration_tx = pool.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('raid-ad-vorlauf:' || $1::text,0))")
        .bind(&target).execute(&mut *registration_tx).await.unwrap();
    sqlx::query("UPDATE twitch_raid_ad_vorlauf SET status='started' WHERE attempt_id='registration_first'")
        .execute(&mut *registration_tx).await.unwrap();
    let arrival_pool = pool.clone();
    let arrival_target = target.clone();
    let blocked_arrival = tokio::spawn(async move {
        sqlx::query("INSERT INTO twitch_raid_arrival_tracking VALUES('7',$1,$2)")
            .bind(arrival_target).bind(now + Duration::seconds(2)).execute(&arrival_pool).await
    });
    wait_for_receiver_lock(&pool, &target).await;
    registration_tx.commit().await.unwrap();
    blocked_arrival.await.unwrap().unwrap();
    let status: String = sqlx::query_scalar("SELECT status FROM twitch_raid_ad_vorlauf WHERE attempt_id='registration_first'")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(status, "arrived");
    assert_eq!(registry.active_until("2", now).await.unwrap(), None);

    let pending = registry.announce("parallel", "4", "2", now);
    let arrival_time = now + Duration::seconds(2);
    let arrival_query = sqlx::query("INSERT INTO twitch_raid_arrival_tracking VALUES('4','2',$1)").bind(arrival_time);
    let (persist, arrival) = tokio::join!(
        registry.persist(&pending),
        arrival_query.execute(&pool),
    );
    persist.unwrap();
    arrival.unwrap();
    let status: String = sqlx::query_scalar("SELECT status FROM twitch_raid_ad_vorlauf WHERE attempt_id='parallel'").fetch_one(&pool).await.unwrap();
    assert_eq!(status, "arrived");

    let surviving = registry.announce("other", "5", "2", now);
    registry.persist(&surviving).await.unwrap();
    let restarted = RaidAdVorlauf::new(pool.clone());
    assert_eq!(restarted.active_until("2", now).await.unwrap(), Some(surviving.until));
    assert_eq!(restarted.active_until("2", surviving.until).await.unwrap(), None);
    let later = restarted.announce("later", "5", "2", now + Duration::seconds(5));
    restarted.persist(&later).await.unwrap();
    restarted.cancel_source_before("5", now).await.unwrap();
    assert_eq!(restarted.active_until("2", now).await.unwrap(), Some(later.until));
    let status: String = sqlx::query_scalar("SELECT status FROM twitch_raid_ad_vorlauf WHERE attempt_id='other'").fetch_one(&pool).await.unwrap();
    assert_eq!(status, "failed");
    pool.close().await;
    sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE"))).execute(&admin).await.unwrap();
}
