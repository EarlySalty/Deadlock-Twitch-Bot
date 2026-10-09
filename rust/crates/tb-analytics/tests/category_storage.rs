#[path = "../../../test-support/category_postgres.rs"]
mod test_postgres;

use chrono::{Duration, Utc};
use sqlx::PgPool;
use std::{
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};
use tb_analytics::category::{
    self,
    storage::{self, ArchiveConfig, Error, ObjectStore},
};
use tb_crypto::FieldCipher;
use tb_transport_twitch::streams::HelixStream;

struct TestDrive {
    directory: tempfile::TempDir,
    fail: AtomicBool,
    promote: Option<PgPool>,
}
impl TestDrive {
    fn new() -> Self {
        Self {
            directory: tempfile::tempdir().unwrap(),
            fail: AtomicBool::new(false),
            promote: None,
        }
    }
    fn path(&self, remote: &str) -> std::path::PathBuf {
        self.directory
            .path()
            .join(remote.rsplit('/').next().unwrap())
    }
}
impl ObjectStore for TestDrive {
    async fn upload(&self, local: &Path, remote: &str) -> Result<(), Error> {
        if self.fail.load(Ordering::Relaxed) {
            return Err("Synthetischer Uploadfehler".into());
        }
        tokio::fs::copy(local, self.path(remote)).await?;
        if let Some(pool) = &self.promote {
            sqlx::query(
                "INSERT INTO twitch_partners VALUES('300','active') ON CONFLICT DO NOTHING",
            )
            .execute(pool)
            .await?;
        }
        Ok(())
    }
    async fn download(&self, remote: &str, local: &Path) -> Result<(), Error> {
        tokio::fs::copy(self.path(remote), local).await?;
        Ok(())
    }
}
fn cipher() -> FieldCipher {
    FieldCipher::from_hex_key(
        "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
        "category-v1",
    )
    .unwrap()
}
async fn base() -> test_postgres::TestPostgres {
    let db = test_postgres::TestPostgres::start().await;
    for sql in [
        include_str!("../../../migrations/20260918123000_category_collector.sql"),
        include_str!("../../../migrations/20260918170000_category_permanent_archive.sql"),
    ] {
        sqlx::raw_sql(sql).execute(&db.pool).await.unwrap();
    }
    db
}
async fn fixture() -> (test_postgres::TestPostgres, chrono::DateTime<Utc>) {
    let db = base().await;
    test_postgres::storage_schema(&db.pool).await;
    let day = "2026-10-07".parse::<chrono::NaiveDate>().unwrap();
    let at = day.and_hms_opt(12, 0, 0).unwrap().and_utc();
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE TABLE category_chat_messages_p{} PARTITION OF category_chat_messages FOR VALUES FROM ('{day} 00:00:00+00') TO ('{} 00:00:00+00')",day.format("%Y%m%d"),day.succ_opt().unwrap()))).execute(&db.pool).await.unwrap();
    sqlx::query("INSERT INTO twitch_partners VALUES('100','active')")
        .execute(&db.pool)
        .await
        .unwrap();
    let streams: Vec<HelixStream> = ["100", "200", "300"]
        .map(|id| HelixStream {
            id: format!("stream-{id}"),
            user_id: id.into(),
            user_login: if id == "100" {
                "renamed-partner"
            } else {
                "same-login"
            }
            .into(),
            user_name: "Synthetic".into(),
            title: "Synthetischer Archivbeweis".into(),
            language: "de".into(),
            viewer_count: 42,
            started_at: (at - Duration::hours(1)).to_rfc3339(),
            tags: Some(vec!["B".into(), "A".into()]),
            ..Default::default()
        })
        .to_vec();
    category::store_snapshot(&db.pool, at, &streams, 60)
        .await
        .unwrap();
    category::store_snapshot(&db.pool, at + Duration::seconds(60), &streams, 60)
        .await
        .unwrap();
    sqlx::query("UPDATE category_snapshot_samples SET sample_seconds=CASE WHEN stream_id='stream-100' THEN 0.000000019 ELSE 59.99999999999999 END WHERE snapshot_at=$1")
        .bind(at + Duration::seconds(60)).execute(&db.pool).await.unwrap();
    let mut messages = Vec::new();
    for (room, id, user, extra) in [
        ("100", "partner", "500", ""),
        ("200", "original", "501", ""),
        ("200", "userclear", "502", ""),
        (
            "300",
            "copy",
            "501",
            ";source-room-id=200;source-id=original",
        ),
        ("300", "independent", "503", ""),
    ] {
        let line=format!("@room-id={room};user-id={user};id={id};tmi-sent-ts={};unused=discard{extra} :synthetic!s@s PRIVMSG #sample :Dies ist eine synthetische Nachricht für den echten Datenbankbeweis.",at.timestamp_millis());
        messages.push(category::raw_message(&line, at, room, "de").unwrap());
    }
    category::store_messages(&db.pool, &messages).await.unwrap();
    category::flush_rollups(&db.pool, 100).await.unwrap();
    storage::finalize_chat_metrics(&db.pool).await.unwrap();
    storage::finalize_normalization(&db.pool).await.unwrap();
    sqlx::query("UPDATE category_storage_state SET removal_authorized=true WHERE singleton")
        .execute(&db.pool)
        .await
        .unwrap();
    (db, at)
}
async fn count(pool: &PgPool, table: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
        .fetch_one(pool)
        .await
        .unwrap()
}
fn stable_report(mut report: serde_json::Value) -> serde_json::Value {
    report.as_object_mut().unwrap().remove("generated_at");
    report
}

#[tokio::test]
async fn all_twelve_columns_backfill_restart_and_cutover_are_lossless() {
    let db = base().await;
    sqlx::raw_sql("INSERT INTO category_collection_runs SELECT at,at,1,17,60 FROM generate_series('2026-09-27 12:00:00.123456+00'::timestamptz,'2026-09-27 12:40:04.123456+00',interval '1 second') at;
        INSERT INTO category_stream_snapshots SELECT snapshot_at,'legacy','200',CASE WHEN extract(second FROM snapshot_at)::int%2=0 THEN 'old-login' ELSE 'new-login' END,17,'Änderung','de','2026-09-27 11:00:00.654321+00',CASE WHEN extract(second FROM snapshot_at)::int%3=0 THEN '[0:2]={B,NULL,A}'::text[] ELSE ARRAY['A','B'] END,'url',false,CASE WHEN extract(second FROM snapshot_at)::int%5=0 THEN '-0'::float8 ELSE 59.99999999999999 END FROM category_collection_runs;").execute(&db.pool).await.unwrap();
    test_postgres::storage_schema(&db.pool).await;
    assert!(storage::finalize_normalization(&db.pool).await.is_err());
    assert_eq!(count(&db.pool, "category_stream_snapshots").await, 2405);
    sqlx::raw_sql("INSERT INTO category_collection_runs VALUES('2026-09-28 12:00:00.999999+00',now(),1,5,60); INSERT INTO category_stream_snapshots VALUES('2026-09-28 12:00:00.999999+00','late-old','300','case-Preserved',5,'Null-Tag','en','2026-09-28 11:00:00.000001+00',ARRAY[NULL,'x',NULL]::text[],'',true,0.12345678912345678)").execute(&db.pool).await.unwrap();
    let day = "2026-09-27".parse().unwrap();
    let proof = storage::backfill(&db.pool, day).await.unwrap();
    assert_eq!(proof["proof"]["rows"], 2405);
    assert_eq!(proof["proof"]["mismatches"], 0);
    assert_eq!(
        storage::backfill(&db.pool, day).await.unwrap()["backfilled"],
        0
    );
    let next = storage::backfill(&db.pool, "2026-09-28".parse().unwrap())
        .await
        .unwrap();
    assert_eq!(next["proof"]["rows"], 1);
    let at = "2026-09-28T12:01:00.999999Z".parse().unwrap();
    category::store_snapshot(
        &db.pool,
        at,
        &[HelixStream {
            id: "new-writer".into(),
            user_id: "400".into(),
            user_login: "new".into(),
            language: "de".into(),
            started_at: "2026-09-28T11:00:00Z".into(),
            ..Default::default()
        }],
        60,
    )
    .await
    .unwrap();
    assert_eq!(count(&db.pool, "category_stream_snapshots").await, 2406);
    let final_proofs = storage::finalize_normalization(&db.pool).await.unwrap();
    assert_eq!(final_proofs.as_array().unwrap().len(), 2);
    assert_eq!(
        storage::finalize_normalization(&db.pool).await.unwrap(),
        final_proofs
    );
    assert_eq!(
        storage::backfill(&db.pool, day).await.unwrap()["proof"]["rows"],
        2405
    );
    let exists: bool =
        sqlx::query_scalar("SELECT to_regclass('category_stream_snapshots') IS NOT NULL")
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert!(!exists);
    assert_eq!(count(&db.pool, "category_snapshots_normalized").await, 2407);
    let unique = count(&db.pool, "category_snapshot_versions").await;
    assert!(
        unique < 20,
        "unchanged fields must not repeat per measurement: {unique}"
    );
    println!("NORMALISIERUNG: 2406 Originalzeilen, 2 UTC-Tage, 12 Spalten, 0 Abweichungen; 1 zusätzlicher neuer Write; {unique} Versionen");
}

#[tokio::test]
async fn verified_archive_keeps_partners_and_the_exact_report_after_rollup_rebuild() {
    let (db, at) = fixture().await;
    let before = stable_report(
        category::report_at(&db.pool, 7, at + Duration::days(2))
            .await
            .unwrap(),
    );
    let mut drive = TestDrive::new();
    drive.promote = Some(db.pool.clone());
    let config = ArchiveConfig {
        remove_enabled: true,
        ..ArchiveConfig::default()
    };
    let key = cipher();
    let chat = storage::archive_day(&db.pool, at.date_naive(), "chat", &config, &key, &drive)
        .await
        .unwrap();
    let snapshots = storage::archive_day(
        &db.pool,
        at.date_naive(),
        "snapshots",
        &config,
        &key,
        &drive,
    )
    .await
    .unwrap();
    assert_eq!(chat.rows, 4);
    assert_eq!(snapshots.rows, 2);
    assert_eq!(
        storage::remove_local(&db.pool, chat.id, &config, &key, &drive)
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        storage::remove_local(&db.pool, snapshots.id, &config, &key, &drive)
            .await
            .unwrap(),
        2
    );
    assert_eq!(count(&db.pool, "category_chat_messages").await, 3);
    assert_eq!(count(&db.pool, "category_snapshots_normalized").await, 4);
    sqlx::query("INSERT INTO category_chat_dirty SELECT hour_at,room_user_id,language FROM category_chat_rollup ON CONFLICT DO NOTHING").execute(&db.pool).await.unwrap();
    category::flush_rollups(&db.pool, 100).await.unwrap();
    let after = stable_report(
        category::report_at(&db.pool, 7, at + Duration::days(2))
            .await
            .unwrap(),
    );
    assert_eq!(before, after);
    let dry = storage::dry_run(&db.pool, at.date_naive(), at.date_naive())
        .await
        .unwrap();
    assert_eq!(dry.as_array().unwrap().len(), 4);
    let remains: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM category_chat_messages WHERE room_user_id IN ('100','300')",
    )
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(remains, 3);
    assert_eq!(
        storage::remove_local(&db.pool, chat.id, &config, &key, &drive)
            .await
            .unwrap(),
        0
    );
    assert_eq!(before["languages"].as_array().unwrap().len(), 1);
    assert!(storage::pending_removals(&db.pool)
        .await
        .unwrap()
        .is_empty());
    sqlx::query("DELETE FROM twitch_partners WHERE twitch_user_id='300'")
        .execute(&db.pool)
        .await
        .unwrap();
    assert_eq!(
        storage::pending_removals(&db.pool).await.unwrap(),
        vec![chat.id]
    );
    assert_eq!(
        storage::remove_local(&db.pool, chat.id, &config, &key, &drive)
            .await
            .unwrap(),
        2
    );
    assert_eq!(count(&db.pool, "category_chat_messages").await, 1);
    assert!(storage::pending_removals(&db.pool)
        .await
        .unwrap()
        .is_empty());
    println!("ARCHIV: Partner-ID und Partnerwechsel vor Entfernung geschützt; Report und Stundenaggregate vor/nach Archiv identisch; spätere Partnerabmeldung automatisch nachgeholt");
}

#[tokio::test]
async fn archive_redactions_and_restore_never_resurrect_deleted_content() {
    let (db, at) = fixture().await;
    let config = ArchiveConfig {
        remove_enabled: true,
        ..ArchiveConfig::default()
    };
    let key = cipher();
    let drive = TestDrive::new();
    let original = stable_report(
        category::report_at(&db.pool, 7, at + Duration::days(2))
            .await
            .unwrap(),
    );
    let chat = storage::archive_day(&db.pool, at.date_naive(), "chat", &config, &key, &drive)
        .await
        .unwrap();
    let snapshots = storage::archive_day(
        &db.pool,
        at.date_naive(),
        "snapshots",
        &config,
        &key,
        &drive,
    )
    .await
    .unwrap();
    storage::remove_local(&db.pool, chat.id, &config, &key, &drive)
        .await
        .unwrap();
    storage::remove_local(&db.pool, snapshots.id, &config, &key, &drive)
        .await
        .unwrap();
    assert_eq!(
        original,
        stable_report(
            category::report_at(&db.pool, 7, at + Duration::days(2))
                .await
                .unwrap()
        )
    );
    category::delete_chat(
        &db.pool,
        "@room-id=200;target-msg-id=original :tmi.twitch.tv CLEARMSG #sample :synthetic",
        "200",
        at + Duration::minutes(1),
    )
    .await
    .unwrap();
    category::delete_chat(
        &db.pool,
        "@room-id=200;target-user-id=502 :tmi.twitch.tv CLEARCHAT #sample :synthetic",
        "200",
        at + Duration::minutes(1),
    )
    .await
    .unwrap();
    category::flush_rollups(&db.pool, 100).await.unwrap();
    let removed = stable_report(
        category::report_at(&db.pool, 7, at + Duration::days(2))
            .await
            .unwrap(),
    );
    storage::restore(&db.pool, chat.id, &config, &key, &drive)
        .await
        .unwrap();
    storage::restore(&db.pool, snapshots.id, &config, &key, &drive)
        .await
        .unwrap();
    category::flush_rollups(&db.pool, 100).await.unwrap();
    assert_eq!(
        removed,
        stable_report(
            category::report_at(&db.pool, 7, at + Duration::days(2))
                .await
                .unwrap()
        )
    );
    let prohibited:i64=sqlx::query_scalar("SELECT count(*) FROM category_chat_messages WHERE message_id IN ('original','copy','userclear')").fetch_one(&db.pool).await.unwrap();
    assert_eq!(prohibited, 0);
    assert_eq!(count(&db.pool, "category_chat_messages").await, 2);
    storage::restore(&db.pool, chat.id, &config, &key, &drive)
        .await
        .unwrap();
    assert_eq!(count(&db.pool, "category_chat_messages").await, 2);
    println!("RÜCKHOLUNG: vollständige Krypto-/Zeilenprüfung; CLEARMSG, zielbezogener CLEARCHAT und Shared-Chat bleiben entfernt; wiederholte Rückholung ohne Duplikate");
}

#[tokio::test]
async fn multi_batch_archive_reclaims_physical_space_and_keeps_complete_partner_rows() {
    let (db, at) = fixture().await;
    sqlx::query("INSERT INTO category_chat_messages SELECT $1::timestamptz+i*interval '1 millisecond',$1::timestamptz+i*interval '1 millisecond','synthetic-'||i,CASE WHEN i<10 THEN '100' ELSE '200' END,'501','synthetic',payload,length(payload),'und',0,'de',0,false,'{}'::jsonb FROM generate_series(1,2000) i CROSS JOIN LATERAL (SELECT string_agg(md5(i::text||'/'||j::text),'') payload FROM generate_series(1,120) j) text_data")
        .bind(at).execute(&db.pool).await.unwrap();
    category::flush_rollups(&db.pool, 100).await.unwrap();
    let bytes_before: i64 =
        sqlx::query_scalar("SELECT pg_total_relation_size('category_chat_messages_p20261007')")
            .fetch_one(&db.pool)
            .await
            .unwrap();
    let partners:Vec<String>=sqlx::query_scalar("SELECT category_chat_wire(m) FROM category_chat_messages m WHERE room_user_id='100' ORDER BY sent_at,message_id").fetch_all(&db.pool).await.unwrap();
    let config = ArchiveConfig {
        remove_enabled: true,
        ..ArchiveConfig::default()
    };
    let drive = TestDrive::new();
    let key = cipher();
    let archived = storage::archive_day(&db.pool, at.date_naive(), "chat", &config, &key, &drive)
        .await
        .unwrap();
    assert_eq!(archived.rows, 1995);
    let wire: String = sqlx::query_scalar(
        "SELECT category_chat_wire(m) FROM category_chat_messages m WHERE message_id='original'",
    )
    .fetch_one(&db.pool)
    .await
    .unwrap();
    let mut late: category::RawMessage = serde_json::from_str(&wire).unwrap();
    late.message_id = "late-uncovered".into();
    late.sent_at = at + Duration::seconds(1);
    late.received_at = late.sent_at;
    category::store_messages(&db.pool, &[late]).await.unwrap();
    category::flush_rollups(&db.pool, 100).await.unwrap();
    let before = stable_report(
        category::report_at(&db.pool, 7, at + Duration::days(2))
            .await
            .unwrap(),
    );
    sqlx::query("CREATE TABLE category_chat_messages_p20261007_replacement(dummy integer)")
        .execute(&db.pool)
        .await
        .unwrap();
    assert!(
        storage::remove_local(&db.pool, archived.id, &config, &key, &drive)
            .await
            .is_err()
    );
    assert_eq!(count(&db.pool, "category_chat_messages").await, 11);
    assert_eq!(
        storage::pending_removals(&db.pool).await.unwrap(),
        vec![archived.id]
    );
    sqlx::query("DROP TABLE category_chat_messages_p20261007_replacement")
        .execute(&db.pool)
        .await
        .unwrap();
    assert_eq!(
        storage::remove_local(&db.pool, archived.id, &config, &key, &drive)
            .await
            .unwrap(),
        0
    );
    let bytes_after: i64 =
        sqlx::query_scalar("SELECT pg_total_relation_size('category_chat_messages_p20261007')")
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert!(
        bytes_after < bytes_before / 2,
        "Physische Belegung {bytes_before} -> {bytes_after}"
    );
    let kept:Vec<String>=sqlx::query_scalar("SELECT category_chat_wire(m) FROM category_chat_messages m WHERE room_user_id='100' ORDER BY sent_at,message_id").fetch_all(&db.pool).await.unwrap();
    assert_eq!(partners, kept);
    assert_eq!(count(&db.pool, "category_chat_messages").await, 11);
    let uncovered: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM category_chat_messages WHERE message_id='late-uncovered'",
    )
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(uncovered, 1);
    sqlx::query("INSERT INTO category_chat_dirty SELECT hour_at,room_user_id,language FROM category_chat_rollup ON CONFLICT DO NOTHING").execute(&db.pool).await.unwrap();
    category::flush_rollups(&db.pool, 100).await.unwrap();
    assert_eq!(
        before,
        stable_report(
            category::report_at(&db.pool, 7, at + Duration::days(2))
                .await
                .unwrap()
        )
    );
    storage::restore(&db.pool, archived.id, &config, &key, &drive)
        .await
        .unwrap();
    assert_eq!(count(&db.pool, "category_chat_messages").await, 2006);
    assert_eq!(count(&db.pool, "category_chat_metrics").await, 2006);
    println!("MEHRBATCH: 1995 verifizierte Nichtpartnerzeilen entfernt, 10 Partnerzeilen und 1 unbestätigte späte Zeile vollständig erhalten; Verdichtung nach Fehler wiederholt; physische Belegung {bytes_before} -> {bytes_after}; 2006 Roh- und Kennzahlenzeilen nach Rückholung");
}

#[tokio::test]
async fn unresolved_partner_ids_and_alert_retry_keep_local_data_safe() {
    let (db, at) = fixture().await;
    let config = ArchiveConfig {
        remove_enabled: true,
        ..ArchiveConfig::default()
    };
    let drive = TestDrive::new();
    let key = cipher();
    sqlx::query("INSERT INTO twitch_partners VALUES('','active')")
        .execute(&db.pool)
        .await
        .unwrap();
    assert!(
        storage::archive_day(&db.pool, at.date_naive(), "chat", &config, &key, &drive)
            .await
            .is_err()
    );
    assert_eq!(count(&db.pool, "category_archive_manifest").await, 0);
    sqlx::query("DELETE FROM twitch_partners WHERE twitch_user_id=''")
        .execute(&db.pool)
        .await
        .unwrap();
    let archived = storage::archive_day(&db.pool, at.date_naive(), "chat", &config, &key, &drive)
        .await
        .unwrap();
    sqlx::query("INSERT INTO twitch_partners VALUES('','active')")
        .execute(&db.pool)
        .await
        .unwrap();
    assert!(
        storage::remove_local(&db.pool, archived.id, &config, &key, &drive)
            .await
            .is_err()
    );
    assert_eq!(count(&db.pool, "category_chat_messages").await, 5);
    storage::record_failure(&db.pool, at).await.unwrap();
    storage::record_failure(&db.pool, at + Duration::hours(1))
        .await
        .unwrap();
    assert_eq!(count(&db.pool, "category_archive_notifications").await, 1);
    storage::record_failure(&db.pool, at + Duration::days(1))
        .await
        .unwrap();
    assert_eq!(count(&db.pool, "category_archive_notifications").await, 2);
    let failures: i64 = sqlx::query_scalar("SELECT failures FROM category_storage_state")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(failures, 3);
}

#[tokio::test]
async fn failed_upload_tampering_and_partial_removal_are_restart_safe_and_role_limited() {
    let (db, at) = fixture().await;
    let drive = TestDrive::new();
    drive.fail.store(true, Ordering::Relaxed);
    let config = ArchiveConfig {
        remove_enabled: true,
        ..ArchiveConfig::default()
    };
    let key = cipher();
    assert!(
        storage::archive_day(&db.pool, at.date_naive(), "chat", &config, &key, &drive)
            .await
            .is_err()
    );
    assert_eq!(count(&db.pool, "category_chat_messages").await, 5);
    let id = sqlx::query_scalar("SELECT id FROM category_archive_manifest WHERE kind='chat'")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert!(storage::remove_local(&db.pool, id, &config, &key, &drive)
        .await
        .is_err());
    drive.fail.store(false, Ordering::Relaxed);
    let chat = storage::archive_day(&db.pool, at.date_naive(), "chat", &config, &key, &drive)
        .await
        .unwrap();
    assert_eq!(chat.id, id);
    let mut bot = db.pool.acquire().await.unwrap();
    sqlx::query("SET ROLE twitchbot")
        .execute(&mut *bot)
        .await
        .unwrap();
    for forbidden in [
        "DELETE FROM category_chat_messages WHERE false",
        "SELECT category_archive_remove($1,1)",
        "SELECT category_archive_compact($1)",
        "UPDATE category_storage_state SET removal_authorized=true",
    ] {
        let error = if forbidden.contains("$1") {
            sqlx::query(forbidden)
                .bind(id)
                .execute(&mut *bot)
                .await
                .unwrap_err()
        } else {
            sqlx::query(forbidden).execute(&mut *bot).await.unwrap_err()
        };
        assert_eq!(
            error.as_database_error().and_then(|e| e.code()).as_deref(),
            Some("42501")
        );
    }
    sqlx::query("RESET ROLE").execute(&mut *bot).await.unwrap();
    drop(bot);
    let corrupt = drive.path(&chat.object_path);
    let bytes = std::fs::read(&corrupt).unwrap();
    let mut bad = bytes.clone();
    *bad.last_mut().unwrap() ^= 1;
    std::fs::write(&corrupt, bad).unwrap();
    assert!(storage::remove_local(&db.pool, id, &config, &key, &drive)
        .await
        .is_err());
    assert_eq!(count(&db.pool, "category_chat_messages").await, 5);
    std::fs::write(&corrupt, bytes).unwrap();
    let first: i64 = sqlx::query_scalar("SELECT category_archive_remove($1,1)")
        .bind(id)
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(first, 1);
    assert_eq!(
        storage::remove_local(&db.pool, id, &config, &key, &drive)
            .await
            .unwrap(),
        3
    );
    assert_eq!(count(&db.pool, "category_chat_messages").await, 1);
    println!("FEHLERPFAD: Uploadfehler und beschädigtes Objekt lassen Rohbestand liegen; Manifest-ID bei Neustart erhalten; 1+3 bestätigte Entfernungen; Bot hat weder Roh-DELETE noch Archiv-Entfernungsrecht");
}
