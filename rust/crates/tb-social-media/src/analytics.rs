//! Clip-Analytics-Persistenz (Port von `bot/social_media/analytics/__init__.py`,
//! Clip-Statistik-Teil).
//!
//! Speichert je Clip × Plattform × Zeitfenster (`24h`/`7d`/`30d`) einen
//! Statistik-Snapshot in `twitch_clips_social_analytics`. `upsert_clip_analytics`
//! aktualisiert eine vorhandene Zeile oder legt sie an (UPDATE-then-INSERT wie
//! Python). Die Report-Funktionen (social_media_reports) folgen mit dem
//! report_dispatcher-Slice.

use sqlx::PgPool;
use sqlx::Row;

/// Auswertungs-Zeitfenster (Python `BUCKETS`).
pub const BUCKETS: [&str; 3] = ["24h", "7d", "30d"];
/// Plattform-Reihenfolge (Python `PLATFORMS`).
pub const PLATFORMS: [&str; 3] = ["youtube", "tiktok", "instagram"];

/// Gelesener Statistik-Snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct ClipAnalyticsSnapshot {
    pub clip_db_id: i32,
    pub platform: String,
    pub bucket: String,
    pub views: i64,
    pub likes: i64,
    pub comments: i64,
    pub shares: i64,
    pub watch_time_seconds: Option<i64>,
    pub ctr_percent: Option<f64>,
    pub engagement_rate: Option<f64>,
    pub provider: Option<String>,
    pub synced_at: Option<String>,
    pub next_pull_at: Option<String>,
}

/// Eingabe für `upsert_clip_analytics` (Default = Retry-Fall: nur Metadaten).
#[derive(Debug, Clone, Default)]
pub struct ClipAnalyticsUpsert {
    pub clip_db_id: i32,
    pub platform: String,
    pub bucket: String,
    pub views: i64,
    pub likes: i64,
    pub comments: i64,
    pub shares: i64,
    pub watch_time_seconds: Option<i64>,
    pub ctr_percent: Option<f64>,
    pub engagement_rate: Option<f64>,
    pub provider: Option<String>,
    pub synced_at: Option<String>,
    pub next_pull_at: Option<String>,
}

fn round2(value: Option<f64>) -> Option<f64> {
    value.map(|v| (v * 100.0).round() / 100.0)
}

fn clean_provider(provider: Option<&str>) -> Option<String> {
    provider.map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

/// Aktualisiert den Snapshot oder legt ihn an (UPDATE → bei 0 Zeilen INSERT).
pub async fn upsert_clip_analytics(pool: &PgPool, upsert: &ClipAnalyticsUpsert) -> Result<(), sqlx::Error> {
    let synced = upsert.synced_at.clone().unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
    let provider = clean_provider(upsert.provider.as_deref());
    let ctr = round2(upsert.ctr_percent);
    let engagement = round2(upsert.engagement_rate);

    let updated = sqlx::query(
        "UPDATE twitch_clips_social_analytics \
            SET views = $1, likes = $2, comments = $3, shares = $4, \
                watch_time_seconds = $5, ctr_percent = $6, engagement_rate = $7, \
                provider = $8, synced_at = $9::timestamptz, next_pull_at = $10::timestamptz \
          WHERE clip_id = $11 AND platform = $12 AND bucket = $13",
    )
    .bind(upsert.views)
    .bind(upsert.likes)
    .bind(upsert.comments)
    .bind(upsert.shares)
    .bind(upsert.watch_time_seconds)
    .bind(ctr)
    .bind(engagement)
    .bind(provider.as_deref())
    .bind(&synced)
    .bind(upsert.next_pull_at.as_deref())
    .bind(upsert.clip_db_id)
    .bind(&upsert.platform)
    .bind(&upsert.bucket)
    .execute(pool)
    .await?;

    if updated.rows_affected() == 0 {
        sqlx::query(
            "INSERT INTO twitch_clips_social_analytics \
                (clip_id, platform, bucket, views, likes, comments, shares, \
                 watch_time_seconds, ctr_percent, engagement_rate, provider, synced_at, next_pull_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12::timestamptz, $13::timestamptz)",
        )
        .bind(upsert.clip_db_id)
        .bind(&upsert.platform)
        .bind(&upsert.bucket)
        .bind(upsert.views)
        .bind(upsert.likes)
        .bind(upsert.comments)
        .bind(upsert.shares)
        .bind(upsert.watch_time_seconds)
        .bind(ctr)
        .bind(engagement)
        .bind(provider.as_deref())
        .bind(&synced)
        .bind(upsert.next_pull_at.as_deref())
        .execute(pool)
        .await?;
    }
    Ok(())
}

/// Alle Snapshots eines Clips (Platform/Bucket sortiert).
pub async fn list_clip_analytics(pool: &PgPool, clip_db_id: i32) -> Vec<ClipAnalyticsSnapshot> {
    let rows = sqlx::query(
        "SELECT clip_id, platform, bucket, views, likes, comments, shares, watch_time_seconds, \
                ctr_percent::double precision AS ctr, engagement_rate::double precision AS eng, \
                provider, synced_at::text AS synced, next_pull_at::text AS next_pull \
           FROM twitch_clips_social_analytics WHERE clip_id = $1 \
          ORDER BY platform ASC, bucket ASC",
    )
    .bind(clip_db_id)
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    rows.iter()
        .map(|r| ClipAnalyticsSnapshot {
            clip_db_id: r.try_get("clip_id").unwrap_or(0),
            platform: r.try_get("platform").unwrap_or_default(),
            bucket: r.try_get("bucket").unwrap_or_default(),
            views: r.try_get::<Option<i32>, _>("views").unwrap_or(None).unwrap_or(0) as i64,
            likes: r.try_get::<Option<i32>, _>("likes").unwrap_or(None).unwrap_or(0) as i64,
            comments: r.try_get::<Option<i32>, _>("comments").unwrap_or(None).unwrap_or(0) as i64,
            shares: r.try_get::<Option<i32>, _>("shares").unwrap_or(None).unwrap_or(0) as i64,
            watch_time_seconds: r.try_get::<Option<i32>, _>("watch_time_seconds").unwrap_or(None).map(|v| v as i64),
            ctr_percent: r.try_get::<Option<f64>, _>("ctr").unwrap_or(None),
            engagement_rate: r.try_get::<Option<f64>, _>("eng").unwrap_or(None),
            provider: r.try_get::<Option<String>, _>("provider").unwrap_or(None),
            synced_at: r.try_get::<Option<String>, _>("synced").unwrap_or(None),
            next_pull_at: r.try_get::<Option<String>, _>("next_pull").unwrap_or(None),
        })
        .collect()
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
        let opts = PgConnectOptions::from_str(&dsn).unwrap().options([("search_path", schema)]);
        let pool = PgPoolOptions::new().max_connections(3).connect_with(opts).await.unwrap();
        sqlx::query(
            "CREATE TABLE twitch_clips_social_analytics (id SERIAL PRIMARY KEY, clip_id INTEGER, platform TEXT, bucket TEXT, \
             views INTEGER, likes INTEGER, comments INTEGER, shares INTEGER, watch_time_seconds INTEGER, \
             ctr_percent NUMERIC(5,2), engagement_rate NUMERIC(5,2), provider TEXT, synced_at TIMESTAMPTZ, next_pull_at TIMESTAMPTZ)",
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn upsert_insert_dann_update() {
        let Some(pool) = make_pool("t_sm_analytics_persist").await else { return };
        // Insert-Pfad.
        upsert_clip_analytics(&pool, &ClipAnalyticsUpsert {
            clip_db_id: 5,
            platform: "tiktok".into(),
            bucket: "24h".into(),
            views: 100,
            likes: 10,
            comments: 5,
            shares: 5,
            engagement_rate: Some(20.0),
            provider: Some("  tiktok_open_api_v2 ".into()),
            next_pull_at: Some("2026-07-01T00:00:00+00:00".into()),
            ..Default::default()
        })
        .await
        .unwrap();
        let snaps = list_clip_analytics(&pool, 5).await;
        assert_eq!(snaps.len(), 1);
        assert_eq!(snaps[0].views, 100);
        assert_eq!(snaps[0].engagement_rate, Some(20.0));
        assert_eq!(snaps[0].provider.as_deref(), Some("tiktok_open_api_v2")); // getrimmt
        assert!(snaps[0].synced_at.is_some());

        // Update-Pfad (gleicher Key) — keine zweite Zeile.
        upsert_clip_analytics(&pool, &ClipAnalyticsUpsert {
            clip_db_id: 5,
            platform: "tiktok".into(),
            bucket: "24h".into(),
            views: 250,
            likes: 30,
            ..Default::default()
        })
        .await
        .unwrap();
        let snaps = list_clip_analytics(&pool, 5).await;
        assert_eq!(snaps.len(), 1);
        assert_eq!(snaps[0].views, 250);
        assert_eq!(snaps[0].likes, 30);
        assert_eq!(snaps[0].provider, None); // im Update auf NULL gesetzt

        // Anderer Bucket → eigene Zeile.
        upsert_clip_analytics(&pool, &ClipAnalyticsUpsert {
            clip_db_id: 5,
            platform: "tiktok".into(),
            bucket: "7d".into(),
            provider: Some("error:tiktok:api".into()),
            ..Default::default()
        })
        .await
        .unwrap();
        let snaps = list_clip_analytics(&pool, 5).await;
        assert_eq!(snaps.len(), 2);
        // sortiert nach bucket → 24h vor 7d
        assert_eq!(snaps[0].bucket, "24h");
        assert_eq!(snaps[1].bucket, "7d");
        assert_eq!(snaps[1].views, 0); // Retry-Fall ohne Metriken
    }
}
