//! Category-Activity-Series (Legacy-Stats-Vergleich category vs tracked).
//!
//! Port von `bot/analytics/api_performance.py:_load_category_activity_series_payload_sync`.
//! Zwei GROUP-BY-Queries (Stunde + Wochentag) über `twitch_stats_tracked` +
//! `twitch_stats_category`, je Durchschnitt/Peak/Samples, ausgerollt auf 24
//! Stunden- bzw. 7 Wochentags-Punkte.

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use serde_json::{json, Map, Value};
use sqlx::PgPool;

/// Eine Aggregat-Zeile: (source_key, bucket, avg, peak, samples).
type AggRow = (String, i32, Option<f64>, Option<i64>, i64);

type Bucket = HashMap<i32, (Option<f64>, Option<i64>, i64)>;

/// Auf 1 Nachkommastelle runden — spiegelt Pythons `_float_or_none` (`round(x, 1)`),
/// das die avg-Werte vor der JSON-Ausgabe rundet. Gleiches Idiom wie in
/// `exp_analytics`/`monetization`.
fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

fn agg_sql(extract: &str) -> String {
    format!(
        "WITH source_rows AS ( \
            SELECT 'tracked' AS source_key, viewer_count, ts_utc FROM twitch_stats_tracked \
            UNION ALL \
            SELECT 'category' AS source_key, viewer_count, ts_utc FROM twitch_stats_category \
         ) \
         SELECT source_key, EXTRACT({extract} FROM ts_utc)::integer AS bucket, \
                AVG(viewer_count)::float8 AS avg_v, MAX(viewer_count)::bigint AS max_v, \
                COUNT(*)::bigint AS samples \
         FROM source_rows WHERE ts_utc >= $1 GROUP BY 1, 2 ORDER BY 1, 2"
    )
}

fn split_buckets(rows: Vec<AggRow>) -> (Bucket, Bucket) {
    let (mut category, mut tracked) = (Bucket::new(), Bucket::new());
    for (source, bucket, avg, peak, samples) in rows {
        match source.trim().to_lowercase().as_str() {
            "category" => {
                category.insert(bucket, (avg, peak, samples));
            }
            "tracked" => {
                tracked.insert(bucket, (avg, peak, samples));
            }
            _ => {}
        }
    }
    (category, tracked)
}

fn weekday_label(weekday: i32) -> String {
    match weekday {
        0 => "Sonntag",
        1 => "Montag",
        2 => "Dienstag",
        3 => "Mittwoch",
        4 => "Donnerstag",
        5 => "Freitag",
        6 => "Samstag",
        _ => return weekday.to_string(),
    }
    .to_string()
}

fn build_point(bucket_key: &str, bucket: i32, label: String, category: &Bucket, tracked: &Bucket) -> Value {
    let cat = category.get(&bucket);
    let trk = tracked.get(&bucket);
    let mut obj = Map::new();
    obj.insert(bucket_key.to_string(), json!(bucket));
    obj.insert("label".to_string(), json!(label));
    obj.insert("categoryAvg".to_string(), json!(cat.and_then(|c| c.0).map(round1)));
    obj.insert("trackedAvg".to_string(), json!(trk.and_then(|c| c.0).map(round1)));
    obj.insert("categoryPeak".to_string(), json!(cat.and_then(|c| c.1)));
    obj.insert("trackedPeak".to_string(), json!(trk.and_then(|c| c.1)));
    obj.insert("categorySamples".to_string(), json!(cat.map(|c| c.2).unwrap_or(0)));
    obj.insert("trackedSamples".to_string(), json!(trk.map(|c| c.2).unwrap_or(0)));
    Value::Object(obj)
}

/// Lädt die Category-Activity-Series (Python `_load_category_activity_series_payload_sync`).
pub async fn load_category_activity_series(pool: &PgPool, days: i64) -> Result<Value, sqlx::Error> {
    let since: DateTime<Utc> = Utc::now() - Duration::days(days);

    let hourly_rows: Vec<AggRow> = sqlx::query_as(&agg_sql("HOUR")).bind(since).fetch_all(pool).await?;
    let weekday_rows: Vec<AggRow> = sqlx::query_as(&agg_sql("DOW")).bind(since).fetch_all(pool).await?;

    let (hourly_cat, hourly_trk) = split_buckets(hourly_rows);
    let (weekday_cat, weekday_trk) = split_buckets(weekday_rows);

    let hourly: Vec<Value> = (0..24)
        .map(|hour| build_point("hour", hour, format!("{hour:02}:00"), &hourly_cat, &hourly_trk))
        .collect();
    // Python-Reihenfolge: Mo–So = [1,2,3,4,5,6,0].
    let weekly: Vec<Value> = [1, 2, 3, 4, 5, 6, 0]
        .into_iter()
        .map(|weekday| build_point("weekday", weekday, weekday_label(weekday), &weekday_cat, &weekday_trk))
        .collect();

    Ok(json!({
        "hourly": hourly,
        "weekly": weekly,
        "windowDays": days,
        "source": "legacy_stats_chart",
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
        let admin = PgPoolOptions::new().max_connections(1).connect(&dsn).await.unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).execute(&admin).await.unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&admin).await.unwrap();
        admin.close().await;
        // timezone=UTC, damit EXTRACT(HOUR FROM ts_utc::timestamptz) deterministisch ist.
        let opts = PgConnectOptions::from_str(&dsn).unwrap().options([("search_path", schema), ("timezone", "UTC")]);
        let pool = PgPoolOptions::new().max_connections(2).connect_with(opts).await.unwrap();
        for ddl in [
            "CREATE TABLE twitch_stats_tracked (ts_utc TIMESTAMPTZ, streamer TEXT, viewer_count INTEGER)",
            "CREATE TABLE twitch_stats_category (ts_utc TIMESTAMPTZ, streamer TEXT, viewer_count INTEGER)",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        Some(pool)
    }

    #[tokio::test]
    async fn series_struktur_und_buckets() {
        let Some(pool) = make_pool("t_catact").await else { return };
        // Heute 10:00 UTC (innerhalb 30d, deterministische Stunde dank Session-UTC).
        sqlx::query("INSERT INTO twitch_stats_tracked (ts_utc, streamer, viewer_count) VALUES ((NOW()::date + TIME '10:00'), 'a', 100), ((NOW()::date + TIME '10:00'), 'b', 200)").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_stats_category (ts_utc, streamer, viewer_count) VALUES ((NOW()::date + TIME '10:00'), 'x', 50)").execute(&pool).await.unwrap();

        let v = load_category_activity_series(&pool, 30).await.unwrap();
        assert_eq!(v["windowDays"], 30);
        assert_eq!(v["source"], "legacy_stats_chart");
        assert_eq!(v["hourly"].as_array().unwrap().len(), 24);
        assert_eq!(v["weekly"].as_array().unwrap().len(), 7);
        // Wochentags-Reihenfolge: erster Eintrag ist Montag (weekday=1).
        assert_eq!(v["weekly"][0]["weekday"], 1);
        assert_eq!(v["weekly"][0]["label"], "Montag");
        // Stunde 10: tracked avg 150 (peak 200, 2 samples), category avg 50.
        let h10 = v["hourly"].as_array().unwrap().iter().find(|p| p["hour"] == 10).unwrap();
        assert_eq!(h10["trackedAvg"], 150.0);
        assert_eq!(h10["trackedPeak"], 200);
        assert_eq!(h10["trackedSamples"], 2);
        assert_eq!(h10["categoryAvg"], 50.0);
        assert_eq!(h10["categorySamples"], 1);
        // Stunde ohne Daten → null avg, 0 samples.
        let h3 = v["hourly"].as_array().unwrap().iter().find(|p| p["hour"] == 3).unwrap();
        assert!(h3["trackedAvg"].is_null());
        assert_eq!(h3["trackedSamples"], 0);
    }

    #[tokio::test]
    async fn avg_wird_auf_1_nachkommastelle_gerundet() {
        let Some(pool) = make_pool("t_catact_round").await else { return };
        // Nicht-glatter Schnitt: (10+10+11)/3 = 10.333… → muss als 10.3 ausgegeben werden
        // (Python _float_or_none → round(x, 1)); ohne Rundung käme 10.333333… raus.
        sqlx::query("INSERT INTO twitch_stats_tracked (ts_utc, streamer, viewer_count) VALUES ((NOW()::date + TIME '10:00'),'a',10),((NOW()::date + TIME '10:00'),'b',10),((NOW()::date + TIME '10:00'),'c',11)")
            .execute(&pool).await.unwrap();
        let v = load_category_activity_series(&pool, 30).await.unwrap();
        let h10 = v["hourly"].as_array().unwrap().iter().find(|p| p["hour"] == 10).unwrap();
        assert_eq!(h10["trackedAvg"], 10.3);
    }

    #[tokio::test]
    async fn leere_tabellen_volle_struktur() {
        let Some(pool) = make_pool("t_catact_empty").await else { return };
        let v = load_category_activity_series(&pool, 30).await.unwrap();
        assert_eq!(v["hourly"].as_array().unwrap().len(), 24);
        assert_eq!(v["weekly"].as_array().unwrap().len(), 7);
        assert!(v["hourly"][0]["categoryAvg"].is_null());
    }
}
