//! Frische Migrationen gegen eine leere Wegwerf-DB.
//! Ohne `TEST_DATABASE_URL` wird der Test laut uebersprungen.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
    time::Duration,
};

use sqlx::postgres::PgPoolOptions;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");
const FRESH_SCHEMA_QUERY: &str = "SELECT table_name, column_name, data_type, is_nullable, coalesce(column_default,'') FROM information_schema.columns WHERE table_schema='public' AND table_name <> '_sqlx_migrations'";
const SCHEMA_SNAPSHOT: &str = include_str!("fresh_schema_snapshot.txt");

fn test_dsn() -> Option<String> {
    std::env::var("TEST_DATABASE_URL").ok()
}

async fn fresh_schema_lines(pool: &sqlx::PgPool) -> BTreeSet<String> {
    sqlx::query_as::<_, (String, String, String, String, String)>(FRESH_SCHEMA_QUERY)
        .fetch_all(pool)
        .await
        .expect("read fresh schema from information_schema.columns")
        .into_iter()
        .map(|(table, column, data_type, is_nullable, column_default)| {
            format!("{table}|{column}|{data_type}|{is_nullable}|{column_default}")
        })
        .collect()
}

async fn sequence_type(pool: &sqlx::PgPool, sequence: &str) -> Option<String> {
    sqlx::query_scalar(
        "SELECT format_type(seqtypid, NULL)
           FROM pg_sequence
          WHERE seqrelid = to_regclass($1)",
    )
    .bind(sequence)
    .fetch_optional(pool)
    .await
    .unwrap_or_else(|err| panic!("sequence type for {sequence}: {err}"))
}

fn snapshot_lines() -> BTreeSet<String> {
    SCHEMA_SNAPSHOT
        .lines()
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn write_schema_snapshot(actual: &BTreeSet<String>) {
    let snapshot_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fresh_schema_snapshot.txt");
    let mut content = actual.iter().cloned().collect::<Vec<_>>().join("\n");
    content.push('\n');
    fs::write(&snapshot_path, content).unwrap_or_else(|err| {
        panic!(
            "write schema snapshot to {}: {err}",
            snapshot_path.display()
        )
    });
}

fn schema_line_key(line: &str) -> String {
    let mut parts = line.splitn(3, '|');
    let table = parts.next().unwrap_or_default();
    let column = parts.next().unwrap_or_default();
    format!("{table}|{column}")
}

fn schema_lines_by_key(lines: &BTreeSet<String>) -> BTreeMap<String, &str> {
    lines
        .iter()
        .map(|line| (schema_line_key(line), line.as_str()))
        .collect()
}

#[derive(Debug)]
struct ChangedSchemaLine {
    key: String,
    expected: String,
    actual: String,
}

#[derive(Debug)]
struct SchemaDiff {
    only_in_actual: Vec<String>,
    only_in_expected: Vec<String>,
    changed: Vec<ChangedSchemaLine>,
}

impl SchemaDiff {
    fn is_empty(&self) -> bool {
        self.only_in_actual.is_empty()
            && self.only_in_expected.is_empty()
            && self.changed.is_empty()
    }

    fn count(&self) -> usize {
        self.only_in_actual.len() + self.only_in_expected.len() + self.changed.len()
    }

    fn render(&self) -> String {
        let mut output = String::from("schema drift against fresh_schema_snapshot.txt\n");

        if !self.only_in_actual.is_empty() {
            output.push_str("\nnur_im_Ist (NEU):\n");
            for line in &self.only_in_actual {
                output.push_str("  + ");
                output.push_str(line);
                output.push('\n');
            }
        }

        if !self.only_in_expected.is_empty() {
            output.push_str("\nnur_im_Soll (FEHLT):\n");
            for line in &self.only_in_expected {
                output.push_str("  - ");
                output.push_str(line);
                output.push('\n');
            }
        }

        if !self.changed.is_empty() {
            output.push_str("\ngeaenderte_Zeilen:\n");
            for changed in &self.changed {
                output.push_str("  * ");
                output.push_str(&changed.key);
                output.push('\n');
                output.push_str("    soll: ");
                output.push_str(&changed.expected);
                output.push('\n');
                output.push_str("    ist : ");
                output.push_str(&changed.actual);
                output.push('\n');
            }
        }

        output
    }
}

fn diff_schema(actual: &BTreeSet<String>, expected: &BTreeSet<String>) -> SchemaDiff {
    let actual_by_key = schema_lines_by_key(actual);
    let expected_by_key = schema_lines_by_key(expected);

    let only_in_actual = actual_by_key
        .iter()
        .filter(|(key, _)| !expected_by_key.contains_key(*key))
        .map(|(_, line)| (*line).to_owned())
        .collect();

    let only_in_expected = expected_by_key
        .iter()
        .filter(|(key, _)| !actual_by_key.contains_key(*key))
        .map(|(_, line)| (*line).to_owned())
        .collect();

    let changed = expected_by_key
        .iter()
        .filter_map(|(key, expected_line)| {
            let actual_line = actual_by_key.get(key)?;
            (*actual_line != *expected_line).then(|| ChangedSchemaLine {
                key: key.clone(),
                expected: (*expected_line).to_owned(),
                actual: (*actual_line).to_owned(),
            })
        })
        .collect();

    SchemaDiff {
        only_in_actual,
        only_in_expected,
        changed,
    }
}

#[tokio::test]
async fn fresh_migrations_match_committed_schema_snapshot() {
    let dsn = match test_dsn() {
        Some(dsn) => dsn,
        None => {
            eprintln!("SKIP: TEST_DATABASE_URL nicht gesetzt");
            return;
        }
    };

    let pool = PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(10))
        .connect(&dsn)
        .await
        .expect("connect fresh test db");

    sqlx::query("CREATE EXTENSION IF NOT EXISTS timescaledb")
        .execute(&pool)
        .await
        .expect("create timescaledb extension");

    MIGRATOR.run(&pool).await.expect("run all migrations");

    let actual = fresh_schema_lines(&pool).await;

    if matches!(std::env::var("UPDATE_SCHEMA_SNAPSHOT").as_deref(), Ok("1")) {
        write_schema_snapshot(&actual);
        return;
    }

    let expected = snapshot_lines();
    let diff = diff_schema(&actual, &expected);
    if !diff.is_empty() {
        eprintln!("{}", diff.render());
        panic!(
            "schema drift detected: {} differences ({} new, {} missing, {} changed)",
            diff.count(),
            diff.only_in_actual.len(),
            diff.only_in_expected.len(),
            diff.changed.len()
        );
    }

    for sequence in [
        "public.clip_fetch_history_id_seq",
        "public.clip_templates_global_id_seq",
        "public.clip_templates_streamer_id_seq",
        "public.twitch_ad_break_events_id_seq",
        "public.twitch_ads_schedule_snapshot_id_seq",
        "public.twitch_ban_events_id_seq",
        "public.twitch_bits_events_id_seq",
        "public.twitch_channel_points_events_id_seq",
        "public.twitch_channel_updates_id_seq",
        "public.twitch_chat_messages_id_seq",
        "public.twitch_clips_social_analytics_id_seq",
        "public.twitch_clips_social_media_id_seq",
        "public.twitch_clips_upload_queue_id_seq",
        "public.twitch_eventsub_capacity_snapshot_id_seq",
        "public.twitch_follow_events_id_seq",
        "public.twitch_hype_train_events_id_seq",
        "public.twitch_link_clicks_id_seq",
        "public.twitch_shoutout_events_id_seq",
        "public.twitch_stream_sessions_id_seq",
        "public.twitch_subscription_events_id_seq",
        "public.twitch_subscriptions_snapshot_id_seq",
    ] {
        if let Some(actual) = sequence_type(&pool, sequence).await {
            assert_eq!(actual, "bigint", "{sequence}");
        }
    }
}
