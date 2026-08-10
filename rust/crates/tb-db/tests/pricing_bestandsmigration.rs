//! Beweis fuer `migrations/20260810100000_pricing_bestandsmigration.sql`.
//!
//! Der Test legt den in der Spec dokumentierten Bestand (30 Zeilen, Stand
//! 2026-08-09) in einem Wegwerf-Schema an, laesst die Migration darueber
//! laufen und zaehlt vorher gegen nachher. Danach faehrt das Rueckwaerts-Skript
//! den Ausgangszustand wieder her.
//!
//! Warum nicht ueber den Migrator: ein frischer Migrationslauf bricht seit
//! `20260806120000_social_media_partner_access.sql` an einem Fremdschluessel ab
//! (vorbestehend, siehe Bericht). Der Test fuehrt deshalb genau die eine
//! Migrationsdatei aus. `public.` wird dabei entfernt, damit alles im
//! Wegwerf-Schema landet und nicht in der echten `public`-Ablage.
//!
//! Ohne `TB_TEST_DATABASE_URL` ueberspringt der Test.

use std::collections::BTreeMap;

use sqlx::{postgres::PgPoolOptions, PgPool, Row};

const MIGRATION: &str = include_str!("../../../migrations/20260810100000_pricing_bestandsmigration.sql");
const RUECKWAERTS: &str =
    include_str!("../../../scripts/20260810_pricing_bestandsmigration_rueckwaerts.sql");

/// Timosius, der einzige zahlende Bestandskunde.
const TIMOSIUS_ID: &str = "123175963";
const TIMOSIUS_ABLAUF: &str = "2027-08-08T00:00:00+00:00";

fn ohne_public(sql: &str) -> String {
    sql.replace("public.", "")
}

async fn pool_or_skip(schema: &str) -> Option<PgPool> {
    let dsn = std::env::var("TB_TEST_DATABASE_URL")
        .or_else(|_| std::env::var("TEST_DATABASE_URL"))
        .ok()?;
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&dsn)
        .await
        .expect("connect test db");
    for stmt in [
        format!("DROP SCHEMA IF EXISTS {schema} CASCADE"),
        format!("CREATE SCHEMA {schema}"),
        format!("SET search_path TO {schema}"),
    ] {
        sqlx::query(&stmt).execute(&pool).await.expect("schema");
    }
    sqlx::query(
        r#"CREATE TABLE streamer_plans (
               twitch_user_id TEXT PRIMARY KEY,
               twitch_login TEXT,
               manual_plan_id TEXT,
               manual_plan_expires_at TEXT,
               plan_name TEXT NOT NULL DEFAULT 'free'
           )"#,
    )
    .execute(&pool)
    .await
    .expect("create streamer_plans");
    Some(pool)
}

/// Der Bestand aus der Spec: 30 Zeilen.
async fn bestand_anlegen(pool: &PgPool) {
    // 11 ohne Plan.
    sqlx::query(
        "INSERT INTO streamer_plans (twitch_user_id, twitch_login, plan_name)
         SELECT 'ohne' || i, 'ohne' || i, 'free' FROM generate_series(1, 11) AS i",
    )
    .execute(pool)
    .await
    .unwrap();
    // 8 abgelaufene Trials.
    sqlx::query(
        "INSERT INTO streamer_plans (twitch_user_id, twitch_login, manual_plan_id, manual_plan_expires_at, plan_name)
         SELECT 'trialalt' || i, 'trialalt' || i, 'analytics_trial',
                (NOW() - INTERVAL '40 days')::text, 'free'
           FROM generate_series(1, 8) AS i",
    )
    .execute(pool)
    .await
    .unwrap();
    // 6 laufende Trials.
    sqlx::query(
        "INSERT INTO streamer_plans (twitch_user_id, twitch_login, manual_plan_id, manual_plan_expires_at, plan_name)
         SELECT 'trialneu' || i, 'trialneu' || i, 'analytics_trial',
                (NOW() + INTERVAL '9 days')::text, 'free'
           FROM generate_series(1, 6) AS i",
    )
    .execute(pool)
    .await
    .unwrap();
    // 2 unbefristete Geschenke.
    sqlx::query(
        "INSERT INTO streamer_plans (twitch_user_id, twitch_login, manual_plan_id, plan_name)
         SELECT 'geschenk' || i, 'geschenk' || i, 'analysis_dashboard', 'analysis'
           FROM generate_series(1, 2) AS i",
    )
    .execute(pool)
    .await
    .unwrap();
    // 1 abgelaufenes analysis_dashboard.
    sqlx::query(
        "INSERT INTO streamer_plans (twitch_user_id, twitch_login, manual_plan_id, manual_plan_expires_at, plan_name)
         VALUES ('analysealt', 'analysealt', 'analysis_dashboard', (NOW() - INTERVAL '3 days')::text, 'analysis')",
    )
    .execute(pool)
    .await
    .unwrap();
    // Timosius, chat_quiet bis 2027-08-08.
    sqlx::query(
        "INSERT INTO streamer_plans (twitch_user_id, twitch_login, manual_plan_id, manual_plan_expires_at, plan_name)
         VALUES ($1, 'timosius', 'chat_quiet', $2, 'pro')",
    )
    .bind(TIMOSIUS_ID)
    .bind(TIMOSIUS_ABLAUF)
    .execute(pool)
    .await
    .unwrap();
    // 1 raid_free.
    sqlx::query(
        "INSERT INTO streamer_plans (twitch_user_id, twitch_login, manual_plan_id, plan_name)
         VALUES ('raidfrei', 'raidfrei', 'raid_free', 'free')",
    )
    .execute(pool)
    .await
    .unwrap();
}

/// Zaehlung je `manual_plan_id` (`NULL` wird zu `(keiner)`).
async fn zaehlung(pool: &PgPool) -> BTreeMap<String, i64> {
    sqlx::query(
        "SELECT COALESCE(manual_plan_id, '(keiner)') AS plan, COUNT(*)::bigint AS anzahl
           FROM streamer_plans GROUP BY 1 ORDER BY 1",
    )
    .fetch_all(pool)
    .await
    .unwrap()
    .into_iter()
    .map(|row| (row.get::<String, _>("plan"), row.get::<i64, _>("anzahl")))
    .collect()
}

/// Vollstaendiger Zeilenabzug fuer den „nichts sonst veraendert"-Vergleich.
async fn abzug(pool: &PgPool) -> BTreeMap<String, (Option<String>, Option<String>, String)> {
    sqlx::query(
        "SELECT twitch_user_id, manual_plan_id, manual_plan_expires_at, plan_name
           FROM streamer_plans",
    )
    .fetch_all(pool)
    .await
    .unwrap()
    .into_iter()
    .map(|row| {
        (
            row.get::<String, _>("twitch_user_id"),
            (
                row.get::<Option<String>, _>("manual_plan_id"),
                row.get::<Option<String>, _>("manual_plan_expires_at"),
                row.get::<String, _>("plan_name"),
            ),
        )
    })
    .collect()
}

async fn skript_ausfuehren(pool: &PgPool, sql: &str) {
    sqlx::raw_sql(&ohne_public(sql))
        .execute(pool)
        .await
        .expect("skript laeuft durch");
}

#[tokio::test]
async fn bestand_wandert_nach_free_und_premium() {
    let Some(pool) = pool_or_skip("tb_pricing_bestand").await else {
        return;
    };
    bestand_anlegen(&pool).await;

    let vorher = zaehlung(&pool).await;
    let abzug_vorher = abzug(&pool).await;
    assert_eq!(
        vorher,
        BTreeMap::from([
            ("(keiner)".into(), 11),
            ("analysis_dashboard".into(), 3),
            ("analytics_trial".into(), 14),
            ("chat_quiet".into(), 1),
            ("raid_free".into(), 1),
        ]),
        "Ausgangsbestand laut Spec"
    );

    println!("VORHER  {vorher:?}");

    skript_ausfuehren(&pool, MIGRATION).await;

    let nachher = zaehlung(&pool).await;
    println!("NACHHER {nachher:?}");
    assert_eq!(
        nachher,
        BTreeMap::from([
            ("(keiner)".into(), 11),
            ("analytics_trial".into(), 6),
            ("free".into(), 10),
            ("premium".into(), 3),
        ]),
        "nachher: 3 Premium, 10 Free, 6 laufende Trials, 11 ohne Plan"
    );
    assert_eq!(
        nachher.values().sum::<i64>(),
        30,
        "keine Zeile verloren oder dazugekommen"
    );

    // Timosius: premium, Ablaufdatum unveraendert.
    let (plan, ablauf, name): (String, String, String) = sqlx::query_as(
        "SELECT manual_plan_id, manual_plan_expires_at, plan_name
           FROM streamer_plans WHERE twitch_user_id = $1",
    )
    .bind(TIMOSIUS_ID)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(plan, "premium");
    assert_eq!(ablauf, TIMOSIUS_ABLAUF, "Ablaufdatum bleibt 2027-08-08");
    assert_eq!(name, "premium", "plan_name wird mitgezogen");

    // Die zwei Geschenke: unbefristetes premium.
    let geschenke: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM streamer_plans
          WHERE twitch_login LIKE 'geschenk%'
            AND manual_plan_id = 'premium'
            AND manual_plan_expires_at IS NULL",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(geschenke, 2, "Geschenke bleiben unbefristet");

    // Die sechs laufenden Trials: Plan und Datum unangetastet.
    for (id, vorwerte) in abzug_vorher
        .iter()
        .filter(|(id, _)| id.starts_with("trialneu"))
    {
        let nachwerte = abzug(&pool).await;
        assert_eq!(
            nachwerte.get(id),
            Some(vorwerte),
            "laufender Trial {id} darf sich nicht aendern"
        );
    }

    // Keine Zeile ausserhalb der Zuordnungstabelle veraendert: die 11 ohne
    // Plan und die 6 laufenden Trials muessen Zeichen fuer Zeichen gleich sein.
    let abzug_nachher = abzug(&pool).await;
    let unberuehrt: Vec<&String> = abzug_vorher
        .keys()
        .filter(|id| id.starts_with("ohne") || id.starts_with("trialneu"))
        .collect();
    assert_eq!(unberuehrt.len(), 17);
    for id in unberuehrt {
        assert_eq!(
            abzug_nachher.get(id),
            abzug_vorher.get(id),
            "Zeile {id} steht nicht in der Zuordnungstabelle"
        );
    }

    // Sicherung: 30 Zeilen mit den Ausgangswerten.
    let gesichert: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM streamer_plans_pricing_backup_20260810")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(gesichert, 30);
    let gesicherter_timosius: String = sqlx::query_scalar(
        "SELECT manual_plan_id FROM streamer_plans_pricing_backup_20260810 WHERE twitch_user_id = $1",
    )
    .bind(TIMOSIUS_ID)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(gesicherter_timosius, "chat_quiet");

    // Zweiter Lauf aendert nichts mehr.
    skript_ausfuehren(&pool, MIGRATION).await;
    assert_eq!(zaehlung(&pool).await, nachher, "Migration ist idempotent");

    // Rueckwaerts-Skript stellt Plan und plan_name wieder her.
    skript_ausfuehren(&pool, RUECKWAERTS).await;
    let zurueck = abzug(&pool).await;
    assert_eq!(
        zurueck, abzug_vorher,
        "Rueckwaerts-Skript stellt den Ausgangszustand her"
    );
}
