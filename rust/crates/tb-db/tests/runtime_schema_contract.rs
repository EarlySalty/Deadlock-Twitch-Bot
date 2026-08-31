use std::path::{Path, PathBuf};

const RUNTIME_SOURCES: &[&str] = &[
    "crates/tb-llm/src/ledger.rs",
    "crates/tb-engagement/src/irc_reader.rs",
    "crates/tb-engagement/src/sender_auth.rs",
    "crates/tb-analytics/src/post_stream.rs",
    "crates/tb-analytics/src/promo_mode.rs",
    "bin/tb-bot/src/raid_oauth_impl.rs",
];

fn rust_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("tb-db muss unter rust/crates liegen")
        .to_path_buf()
}

fn produktiver_quelltext(relativ: &str) -> String {
    let quelltext = std::fs::read_to_string(rust_root().join(relativ))
        .unwrap_or_else(|error| panic!("{relativ} konnte nicht gelesen werden: {error}"));
    quelltext
        .split("#[cfg(test)]")
        .next()
        .expect("split liefert mindestens ein Element")
        .to_string()
}

#[test]
fn runtime_quellen_enthalten_keine_schema_aenderungen() {
    for relativ in RUNTIME_SOURCES {
        let produktiv = produktiver_quelltext(relativ).to_ascii_uppercase();
        for ddl in ["CREATE TABLE", "CREATE INDEX", "ALTER TABLE"] {
            assert!(
                !produktiv.contains(ddl),
                "{relativ} enthält produktives Runtime-DDL: {ddl}"
            );
        }
    }
}

#[test]
fn requirements_dedupe_ist_als_idempotente_migration_definiert() {
    let migration = std::fs::read_to_string(
        rust_root().join("migrations/20260831100000_raid_requirements_dm_dedupe.sql"),
    )
    .expect("Requirements-Dedupe-Migration fehlt");
    let sql = migration.to_ascii_uppercase();

    assert!(sql.contains("CREATE TABLE IF NOT EXISTS PUBLIC.TWITCH_RAID_REQUIREMENTS_DM_DEDUPE"));
    for spalte in [
        "TWITCH_USER_ID",
        "PURPOSE",
        "TWITCH_LOGIN",
        "DISCORD_USER_ID",
        "STATUS",
        "MESSAGE_ID",
        "ERROR_MESSAGE",
        "CLAIMED_AT",
        "SENT_AT",
        "UPDATED_AT",
    ] {
        assert!(
            sql.contains(spalte),
            "Spalte {spalte} fehlt in der Migration"
        );
    }
    assert!(sql.contains("PRIMARY KEY (TWITCH_USER_ID, PURPOSE)"));
}

#[tokio::test]
async fn requirements_dedupe_migration_laeuft_zweimal_ohne_fehler() {
    let Ok(dsn) = std::env::var("TB_TEST_DATABASE_URL") else {
        return;
    };
    let schema = format!("t_runtime_schema_contract_{}", std::process::id());
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&dsn)
        .await
        .expect("Testdatenbank verbinden");
    sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
        .execute(&pool)
        .await
        .expect("altes Testschema entfernen");
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&pool)
        .await
        .expect("Testschema anlegen");

    let migration = std::fs::read_to_string(
        rust_root().join("migrations/20260831100000_raid_requirements_dm_dedupe.sql"),
    )
    .expect("Requirements-Dedupe-Migration fehlt")
    .replace(
        "public.twitch_raid_requirements_dm_dedupe",
        &format!("{schema}.twitch_raid_requirements_dm_dedupe"),
    );
    sqlx::raw_sql(&migration)
        .execute(&pool)
        .await
        .expect("erste Anwendung der Migration");
    sqlx::raw_sql(&migration)
        .execute(&pool)
        .await
        .expect("zweite Anwendung der Migration");

    let spalten: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM information_schema.columns \
         WHERE table_schema = $1 AND table_name = 'twitch_raid_requirements_dm_dedupe'",
    )
    .bind(&schema)
    .fetch_one(&pool)
    .await
    .expect("Spalten zählen");
    assert_eq!(spalten, 10);

    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&pool)
        .await
        .expect("Testschema entfernen");
}
