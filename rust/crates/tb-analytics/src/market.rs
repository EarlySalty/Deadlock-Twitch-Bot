//! Queries für `GET /twitch/api/v2/market-share` (Admin).
//!
//! Datenbasis ist `twitch_stats_category`: der Kategorie-Poll schreibt pro
//! Tick (~17 s) eine Zeile je Live-Stream der Deadlock-Kategorie inkl.
//! `is_partner`-Flag (Stand zum Schreibzeitpunkt). Marktanteil = Summe
//! Partner-Viewer / Summe aller Viewer, tick-normalisiert pro Zeit-Bucket.
//!
//! Hinweis Datenhistorie: bis zum Cutover am 10.06.2026 enthielt die Tabelle
//! nur die sprachgefilterte (de) Teilmenge der Kategorie, seitdem die volle
//! Kategorie. Die `german`-Sicht filtert über die Stream-Tags (Näherung —
//! die Tabelle hat keine language-Spalte).

use chrono::{DateTime, Utc};
use sqlx::PgPool;

/// Ein Zeit-Bucket der Marktanteils-Serie (tick-normalisierte Durchschnitte).
#[derive(Debug, sqlx::FromRow)]
pub struct MarketShareBucketRow {
    pub bucket: DateTime<Utc>,
    pub partner_viewers: Option<f64>,
    pub total_viewers: Option<f64>,
    pub partner_streams: Option<f64>,
    pub total_streams: Option<f64>,
}

/// Marktanteils-Zeitreihe aus `twitch_stats_category`.
///
/// Tick-Normalisierung: `SUM(...) / COUNT(DISTINCT ts_utc)` ergibt den
/// Durchschnitt der Per-Tick-Summen im Bucket, unabhängig davon wie viele
/// Poll-Ticks in den Bucket fallen. Bucketing über epoch-floor statt
/// `time_bucket`, damit die Query auch ohne TimescaleDB läuft (Tests).
///
/// `german_only`-Markt-Definition (wie die Bot-Discovery): Stream-Sprache
/// `de` ODER Partner. Für Alt-Zeilen ohne `language`-Wert (Spalte existiert
/// erst seit 11.06.2026) gilt der Fallback: vor dem Kategorie-Poll-Cutover
/// (10.06.2026) zählen sie ungefiltert (Erhebung war bereits language=de),
/// danach über die Stream-Tags (Deutsch/German) als Näherung.
pub async fn market_share_series(
    pool: &PgPool,
    since: DateTime<Utc>,
    bucket_seconds: i64,
    german_only: bool,
) -> Result<Vec<MarketShareBucketRow>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT
            to_timestamp(floor(extract(epoch FROM ts_utc) / $1) * $1) AS bucket,
            (SUM(viewer_count) FILTER (WHERE is_partner))::FLOAT8
                / NULLIF(COUNT(DISTINCT ts_utc), 0)                   AS partner_viewers,
            SUM(viewer_count)::FLOAT8
                / NULLIF(COUNT(DISTINCT ts_utc), 0)                   AS total_viewers,
            (COUNT(*) FILTER (WHERE is_partner))::FLOAT8
                / NULLIF(COUNT(DISTINCT ts_utc), 0)                   AS partner_streams,
            COUNT(*)::FLOAT8
                / NULLIF(COUNT(DISTINCT ts_utc), 0)                   AS total_streams
        FROM twitch_stats_category
        WHERE ts_utc >= $2
          AND ($3::BOOL IS FALSE
               OR is_partner
               OR language = 'de'
               OR (language IS NULL
                   AND (ts_utc < '2026-06-10T00:00:00+00'
                        OR tags ILIKE '%deutsch%' OR tags ILIKE '%german%')))
        GROUP BY bucket
        ORDER BY bucket
        "#,
    )
    .bind(bucket_seconds)
    .bind(since)
    .bind(german_only)
    .fetch_all(pool)
    .await
}

/// Ein Stream des letzten Kategorie-Ticks.
#[derive(Debug, sqlx::FromRow)]
pub struct MarketStreamRow {
    pub ts_utc: DateTime<Utc>,
    pub streamer: String,
    pub viewer_count: Option<i32>,
    pub is_partner: bool,
    pub is_german: Option<bool>,
    pub language: Option<String>,
}

/// Alle Streams des letzten Kategorie-Ticks, absteigend nach Viewern.
/// `is_german` = Stream-Sprache `de`; Tag-Fallback nur für Zeilen ohne
/// `language`-Wert (Ticks vor Einführung der Spalte).
pub async fn market_current_tick(pool: &PgPool) -> Result<Vec<MarketStreamRow>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT
            ts_utc,
            streamer,
            viewer_count,
            COALESCE(is_partner, FALSE)                            AS is_partner,
            (language = 'de'
             OR (language IS NULL
                 AND (tags ILIKE '%deutsch%' OR tags ILIKE '%german%'))) AS is_german,
            language
        FROM twitch_stats_category
        WHERE ts_utc = (SELECT MAX(ts_utc) FROM twitch_stats_category)
        ORDER BY viewer_count DESC NULLS LAST, streamer
        "#,
    )
    .fetch_all(pool)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use sqlx::postgres::PgPoolOptions;

    fn test_dsn() -> Option<String> {
        std::env::var("TB_TEST_DATABASE_URL").ok()
    }

    macro_rules! db_dsn_or_skip {
        () => {
            match test_dsn() {
                Some(d) => d,
                None => {
                    if std::env::var("TB_TEST_REQUIRE_DB").as_deref() == Ok("1") {
                        panic!(
                            "TB_TEST_REQUIRE_DB=1 ist gesetzt, aber TB_TEST_DATABASE_URL fehlt"
                        );
                    }
                    eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                    return;
                }
            }
        };
    }

    async fn make_pool(dsn: &str, schema: &str) -> PgPool {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(dsn)
            .await
            .expect("connect test-db");
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&pool)
            .await
            .expect("Schema droppen");
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&pool)
            .await
            .expect("Schema anlegen");
        sqlx::query(&format!("SET search_path TO {schema}"))
            .execute(&pool)
            .await
            .expect("search_path setzen fehlgeschlagen");
        sqlx::query(
            r#"
            CREATE TABLE twitch_stats_category (
                ts_utc       TIMESTAMPTZ NOT NULL,
                streamer     TEXT NOT NULL,
                viewer_count INTEGER,
                is_partner   BOOLEAN DEFAULT FALSE,
                game_name    TEXT,
                stream_title TEXT,
                tags         TEXT,
                language     TEXT
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("DDL fehlgeschlagen");
        pool
    }

    async fn seed(
        pool: &PgPool,
        ts: DateTime<Utc>,
        rows: &[(&str, i32, bool, &str, Option<&str>)],
    ) {
        for (streamer, viewers, partner, tags, language) in rows {
            sqlx::query(
                "INSERT INTO twitch_stats_category
                     (ts_utc, streamer, viewer_count, is_partner, tags, language)
                 VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(ts)
            .bind(streamer)
            .bind(viewers)
            .bind(partner)
            .bind(tags)
            .bind(language)
            .execute(pool)
            .await
            .expect("seed insert");
        }
    }

    #[tokio::test]
    async fn series_normalisiert_ueber_ticks() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_market_series").await;
        let base = Utc.with_ymd_and_hms(2026, 6, 11, 12, 0, 0).unwrap();

        // Zwei Ticks im selben Stunden-Bucket: Summen müssen gemittelt werden,
        // nicht aufaddiert.
        for offset in [0, 30] {
            let ts = base + chrono::Duration::seconds(offset);
            seed(
                &pool,
                ts,
                &[
                    ("partner_a", 10, true, r#"["Deutsch","deadlock"]"#, Some("de")),
                    ("big_intl", 90, false, r#"["English"]"#, Some("en")),
                ],
            )
            .await;
        }

        let rows = market_share_series(&pool, base - chrono::Duration::hours(1), 3600, false)
            .await
            .expect("series query");
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert!((row.partner_viewers.unwrap() - 10.0).abs() < 1e-9);
        assert!((row.total_viewers.unwrap() - 100.0).abs() < 1e-9);
        assert!((row.partner_streams.unwrap() - 1.0).abs() < 1e-9);
        assert!((row.total_streams.unwrap() - 2.0).abs() < 1e-9);
    }

    #[tokio::test]
    async fn series_german_zaehlt_altdaten_und_partner_ohne_tag() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_market_altdaten").await;

        // Vor dem Cutover (10.06.2026), language NULL: Erhebung war bereits
        // DE-gefiltert → zählt im german-Scope auch ohne Deutsch-Tag.
        let alt = Utc.with_ymd_and_hms(2026, 5, 1, 12, 0, 0).unwrap();
        seed(&pool, alt, &[("alt_de", 30, false, r#"["deadlock"]"#, None)]).await;

        // Nach dem Cutover: Partner zählt auch mit Fremdsprache zum DE-Markt,
        // fremdsprachiger Nicht-Partner nicht.
        let neu = Utc.with_ymd_and_hms(2026, 6, 11, 12, 0, 0).unwrap();
        seed(
            &pool,
            neu,
            &[
                ("partner_ohne_tag", 10, true, r#"["English"]"#, Some("en")),
                ("intl", 500, false, r#"["English"]"#, Some("en")),
            ],
        )
        .await;

        let rows = market_share_series(&pool, alt - chrono::Duration::hours(1), 3600, true)
            .await
            .expect("series query");
        assert_eq!(rows.len(), 2);
        assert!((rows[0].total_viewers.unwrap() - 30.0).abs() < 1e-9);
        assert!((rows[1].total_viewers.unwrap() - 10.0).abs() < 1e-9);
        assert!((rows[1].partner_viewers.unwrap() - 10.0).abs() < 1e-9);
    }

    #[tokio::test]
    async fn series_german_filter_und_current_tick() {
        let dsn = db_dsn_or_skip!();
        let pool = make_pool(&dsn, "test_market_german").await;
        let ts = Utc.with_ymd_and_hms(2026, 6, 11, 12, 0, 0).unwrap();
        seed(
            &pool,
            ts,
            &[
                ("partner_a", 10, true, r#"["Deutsch","deadlock"]"#, Some("de")),
                ("de_fremd", 40, false, r#"["deadlock"]"#, Some("de")),
                // metro-Fall: Deutsch-Tag gesetzt, streamt aber englisch →
                // language gewinnt, zählt NICHT zum DE-Markt.
                ("tag_only", 100, false, r#"["Deutsch","English"]"#, Some("en")),
                ("big_intl", 950, false, r#"["English"]"#, Some("en")),
            ],
        )
        .await;

        let rows = market_share_series(&pool, ts - chrono::Duration::hours(1), 3600, true)
            .await
            .expect("series query");
        assert_eq!(rows.len(), 1);
        assert!((rows[0].total_viewers.unwrap() - 50.0).abs() < 1e-9);
        assert!((rows[0].partner_viewers.unwrap() - 10.0).abs() < 1e-9);

        let current = market_current_tick(&pool).await.expect("current query");
        assert_eq!(current.len(), 4);
        assert_eq!(current[0].streamer, "big_intl");
        assert!(!current[0].is_partner);
        assert_eq!(current[0].is_german, Some(false));
        let partner = current.iter().find(|r| r.streamer == "partner_a").unwrap();
        assert!(partner.is_partner);
        assert_eq!(partner.is_german, Some(true));
        let tag_only = current.iter().find(|r| r.streamer == "tag_only").unwrap();
        assert_eq!(tag_only.is_german, Some(false));
        assert_eq!(tag_only.language.as_deref(), Some("en"));
    }
}
