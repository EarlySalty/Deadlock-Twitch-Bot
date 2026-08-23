//! Clip-Analytics-Persistenz (Port von `bot/social_media/analytics/__init__.py`,
//! Clip-Statistik-Teil).
//!
//! Speichert je Clip × Plattform × Zeitfenster (`24h`/`7d`/`30d`) einen
//! Statistik-Snapshot in `twitch_clips_social_analytics`. `upsert_clip_analytics`
//! aktualisiert eine vorhandene Zeile oder legt sie an (UPDATE-then-INSERT wie
//! Python). Die Report-Funktionen (social_media_reports) folgen mit dem
//! report_dispatcher-Slice.

use sqlx::PgPool;

/// Auswertungs-Zeitfenster (Python `BUCKETS`).
pub const BUCKETS: [&str; 3] = ["24h", "7d", "30d"];
/// Plattform-Reihenfolge (Python `PLATFORMS`).
pub const PLATFORMS: [&str; 3] = ["youtube", "tiktok", "instagram"];

/// Gelesener Statistik-Snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct ClipAnalyticsSnapshot {
    pub clip_db_id: i64,
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
    pub clip_db_id: i64,
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
    provider
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn int4_metric(field: &str, value: i64) -> Result<i32, sqlx::Error> {
    i32::try_from(value).map_err(|error| {
        sqlx::Error::InvalidArgument(format!("{field}={value} does not fit into int4: {error}"))
    })
}

fn optional_int4_metric(field: &str, value: Option<i64>) -> Result<Option<i32>, sqlx::Error> {
    value.map(i32::try_from).transpose().map_err(|error| {
        sqlx::Error::InvalidArgument(format!("{field} does not fit into int4: {error}"))
    })
}

/// Schreibt nur Provider und naechsten Abholtermin, ohne die Messwerte
/// anzufassen. Der Retry-Pfad hat frueher den vollen Upsert benutzt und dabei
/// die Standardwerte 0 mitgeschickt: ein einziger API-Fehler ueberschrieb damit
/// einen Snapshot mit 50.000 Views durch eine glaubwuerdig aussehende Null.
/// Existiert noch keine Zeile, wird eine mit Nullen angelegt; dort gibt es
/// nichts zu zerstoeren.
pub async fn schedule_clip_analytics_retry(
    pool: &PgPool,
    clip_db_id: i64,
    platform: &str,
    bucket: &str,
    provider: Option<&str>,
    next_pull_at: &str,
) -> Result<(), sqlx::Error> {
    let provider = clean_provider(provider);
    let updated = sqlx::query!(
        "UPDATE twitch_clips_social_analytics \
            SET provider = COALESCE($1, provider), next_pull_at = $2::text::timestamptz \
          WHERE clip_id = $3 AND platform = $4 AND bucket = $5",
        provider.as_deref(),
        next_pull_at,
        clip_db_id,
        platform,
        bucket
    )
    .execute(pool)
    .await?;

    if updated.rows_affected() == 0 {
        sqlx::query!(
            "INSERT INTO twitch_clips_social_analytics \
                (clip_id, platform, bucket, views, likes, comments, shares, provider, synced_at, next_pull_at) \
             VALUES ($1, $2, $3, 0, 0, 0, 0, $4, CURRENT_TIMESTAMP, $5::text::timestamptz)",
            clip_db_id,
            platform,
            bucket,
            provider.as_deref(),
            next_pull_at
        )
        .execute(pool)
        .await?;
    }
    Ok(())
}

/// Aktualisiert den Snapshot oder legt ihn an (UPDATE → bei 0 Zeilen INSERT).
pub async fn upsert_clip_analytics(
    pool: &PgPool,
    upsert: &ClipAnalyticsUpsert,
) -> Result<(), sqlx::Error> {
    let synced = upsert
        .synced_at
        .clone()
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
    let provider = clean_provider(upsert.provider.as_deref());
    let ctr = round2(upsert.ctr_percent);
    let engagement = round2(upsert.engagement_rate);
    let views = int4_metric("views", upsert.views)?;
    let likes = int4_metric("likes", upsert.likes)?;
    let comments = int4_metric("comments", upsert.comments)?;
    let shares = int4_metric("shares", upsert.shares)?;
    let watch_time_seconds = optional_int4_metric("watch_time_seconds", upsert.watch_time_seconds)?;

    let updated = sqlx::query!(
        "UPDATE twitch_clips_social_analytics \
            SET views = $1, likes = $2, comments = $3, shares = $4, \
                watch_time_seconds = $5, ctr_percent = $6::double precision, engagement_rate = $7::double precision, \
                provider = $8, synced_at = $9::text::timestamptz, next_pull_at = $10::text::timestamptz \
          WHERE clip_id = $11 AND platform = $12 AND bucket = $13",
        views,
        likes,
        comments,
        shares,
        watch_time_seconds,
        ctr,
        engagement,
        provider.as_deref(),
        &synced,
        upsert.next_pull_at.as_deref(),
        upsert.clip_db_id,
        &upsert.platform,
        &upsert.bucket
    )
    .execute(pool)
    .await?;

    if updated.rows_affected() == 0 {
        sqlx::query!(
            "INSERT INTO twitch_clips_social_analytics \
                (clip_id, platform, bucket, views, likes, comments, shares, \
                 watch_time_seconds, ctr_percent, engagement_rate, provider, synced_at, next_pull_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9::double precision, $10::double precision, $11, $12::text::timestamptz, $13::text::timestamptz)",
            upsert.clip_db_id,
            &upsert.platform,
            &upsert.bucket,
            views,
            likes,
            comments,
            shares,
            watch_time_seconds,
            ctr,
            engagement,
            provider.as_deref(),
            &synced,
            upsert.next_pull_at.as_deref()
        )
        .execute(pool)
        .await?;
    }
    Ok(())
}

/// Alle Snapshots eines Clips (Platform/Bucket sortiert).
pub async fn list_clip_analytics(
    pool: &PgPool,
    clip_db_id: impl Into<i64>,
) -> Vec<ClipAnalyticsSnapshot> {
    let clip_db_id = clip_db_id.into();
    let rows = sqlx::query!(
        "SELECT clip_id AS \"clip_id!\", platform AS \"platform!\", COALESCE(bucket, '') AS \"bucket!\", \
                COALESCE(views, 0) AS \"views!\", COALESCE(likes, 0) AS \"likes!\", \
                COALESCE(comments, 0) AS \"comments!\", COALESCE(shares, 0) AS \"shares!\", \
                watch_time_seconds, ctr_percent::double precision AS \"ctr?\", \
                engagement_rate::double precision AS \"eng?\", provider, synced_at::text AS synced, \
                next_pull_at::text AS next_pull \
           FROM twitch_clips_social_analytics WHERE clip_id = $1 \
          ORDER BY platform ASC, bucket ASC",
        clip_db_id
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    rows.iter()
        .map(|r| ClipAnalyticsSnapshot {
            clip_db_id: r.clip_id,
            platform: r.platform.clone(),
            bucket: r.bucket.clone(),
            views: r.views as i64,
            likes: r.likes as i64,
            comments: r.comments as i64,
            shares: r.shares as i64,
            watch_time_seconds: r.watch_time_seconds.map(|v| v as i64),
            ctr_percent: r.ctr,
            engagement_rate: r.eng,
            provider: r.provider.clone(),
            synced_at: r.synced.clone(),
            next_pull_at: r.next_pull.clone(),
        })
        .collect()
}

/// Ein gespeicherter Social-Media-Report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SocialMediaReportRecord {
    pub id: i32,
    pub kind: String,
    pub streamer_login: Option<String>,
    pub period_start: String,
    pub period_end: String,
    pub content_md: String,
    pub model: Option<String>,
    pub created_at: Option<String>,
}

type ReportRow = (
    i32,
    String,
    Option<String>,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
);

fn opt_str(value: Option<String>) -> Option<String> {
    value
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn row_to_report(r: ReportRow) -> SocialMediaReportRecord {
    SocialMediaReportRecord {
        id: r.0,
        kind: r.1,
        streamer_login: opt_str(r.2),
        period_start: r.3,
        period_end: r.4,
        content_md: r.5,
        model: opt_str(r.6),
        created_at: opt_str(r.7),
    }
}

const REPORT_COLUMNS: &str = "id, kind, streamer_login, period_start::text, period_end::text, \
    content_md, model, created_at::text";

/// Listet Reports, optional nach Art/Streamer gefiltert (neueste zuerst).
pub async fn list_reports(
    pool: &PgPool,
    kind: Option<&str>,
    streamer_login: Option<&str>,
    limit: i64,
) -> Vec<SocialMediaReportRecord> {
    let rows: Vec<ReportRow> = sqlx::query_as(&format!(
        "SELECT {REPORT_COLUMNS} FROM social_media_reports \
          WHERE ($1::text IS NULL OR kind = $1) \
            AND ($2::text IS NULL OR LOWER(COALESCE(streamer_login, '')) = LOWER($2)) \
          ORDER BY period_end DESC, created_at DESC, id DESC LIMIT $3"
    ))
    .bind(kind)
    .bind(streamer_login)
    .bind(limit.clamp(1, 100))
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    rows.into_iter().map(row_to_report).collect()
}

/// Sucht einen vorhandenen Report für denselben Zeitraum (Idempotenz-Check).
pub async fn get_existing_report(
    pool: &PgPool,
    kind: &str,
    period_start: &str,
    period_end: &str,
    streamer_login: Option<&str>,
) -> Option<SocialMediaReportRecord> {
    let row: Option<ReportRow> = sqlx::query_as(&format!(
        "SELECT {REPORT_COLUMNS} FROM social_media_reports \
          WHERE kind = $1 AND period_start = $2::timestamptz AND period_end = $3::timestamptz \
            AND (streamer_login = $4 OR (streamer_login IS NULL AND $4::text IS NULL)) \
          ORDER BY created_at DESC, id DESC LIMIT 1"
    ))
    .bind(kind)
    .bind(period_start)
    .bind(period_end)
    .bind(streamer_login)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    row.map(row_to_report)
}

/// Legt einen Report an und liefert den gespeicherten Datensatz.
pub async fn insert_report(
    pool: &PgPool,
    kind: &str,
    streamer_login: Option<&str>,
    period_start: &str,
    period_end: &str,
    content_md: &str,
    model: Option<&str>,
) -> Result<SocialMediaReportRecord, sqlx::Error> {
    let row: ReportRow = sqlx::query_as(&format!(
        "INSERT INTO social_media_reports (kind, streamer_login, period_start, period_end, content_md, model) \
         VALUES ($1, $2, $3::timestamptz, $4::timestamptz, $5, $6) RETURNING {REPORT_COLUMNS}"
    ))
    .bind(kind)
    .bind(streamer_login)
    .bind(period_start)
    .bind(period_end)
    .bind(content_md)
    .bind(model)
    .fetch_one(pool)
    .await?;
    Ok(row_to_report(row))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = crate::test_support::test_dsn()?;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn)
            .unwrap()
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(3)
            .connect_with(opts)
            .await
            .unwrap();
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
        let Some(pool) = make_pool("t_sm_analytics_persist").await else {
            return;
        };
        // Insert-Pfad.
        upsert_clip_analytics(
            &pool,
            &ClipAnalyticsUpsert {
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
            },
        )
        .await
        .unwrap();
        let snaps = list_clip_analytics(&pool, 5).await;
        assert_eq!(snaps.len(), 1);
        assert_eq!(snaps[0].views, 100);
        assert_eq!(snaps[0].engagement_rate, Some(20.0));
        assert_eq!(snaps[0].provider.as_deref(), Some("tiktok_open_api_v2")); // getrimmt
        assert!(snaps[0].synced_at.is_some());

        // Update-Pfad (gleicher Key) — keine zweite Zeile.
        upsert_clip_analytics(
            &pool,
            &ClipAnalyticsUpsert {
                clip_db_id: 5,
                platform: "tiktok".into(),
                bucket: "24h".into(),
                views: 250,
                likes: 30,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let snaps = list_clip_analytics(&pool, 5).await;
        assert_eq!(snaps.len(), 1);
        assert_eq!(snaps[0].views, 250);
        assert_eq!(snaps[0].likes, 30);
        assert_eq!(snaps[0].provider, None); // im Update auf NULL gesetzt

        // Anderer Bucket → eigene Zeile.
        upsert_clip_analytics(
            &pool,
            &ClipAnalyticsUpsert {
                clip_db_id: 5,
                platform: "tiktok".into(),
                bucket: "7d".into(),
                provider: Some("error:tiktok:api".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let snaps = list_clip_analytics(&pool, 5).await;
        assert_eq!(snaps.len(), 2);
        // sortiert nach bucket → 24h vor 7d
        assert_eq!(snaps[0].bucket, "24h");
        assert_eq!(snaps[1].bucket, "7d");
        assert_eq!(snaps[1].views, 0); // Retry-Fall ohne Metriken
    }

    async fn make_reports_pool(schema: &str) -> Option<PgPool> {
        let dsn = crate::test_support::test_dsn()?;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn)
            .unwrap()
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(3)
            .connect_with(opts)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE social_media_reports (id SERIAL PRIMARY KEY, kind TEXT NOT NULL, streamer_login TEXT, \
             period_start TIMESTAMPTZ NOT NULL, period_end TIMESTAMPTZ NOT NULL, content_md TEXT NOT NULL, \
             model TEXT, created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP)",
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn report_insert_get_list() {
        let Some(pool) = make_reports_pool("t_sm_reports").await else {
            return;
        };
        let (ps, pe) = ("2026-06-01T00:00:00+00:00", "2026-06-08T00:00:00+00:00");
        // Insert streamer-spezifisch.
        let rec = insert_report(
            &pool,
            "weekly",
            Some("nani"),
            ps,
            pe,
            "# Report",
            Some("minimax"),
        )
        .await
        .unwrap();
        assert_eq!(rec.kind, "weekly");
        assert_eq!(rec.streamer_login.as_deref(), Some("nani"));
        assert_eq!(rec.content_md, "# Report");
        assert!(rec.created_at.is_some());

        // get_existing trifft denselben Zeitraum (timestamptz-Gleichheit, formatunabhängig).
        let found = get_existing_report(&pool, "weekly", ps, pe, Some("nani")).await;
        assert_eq!(found.map(|r| r.id), Some(rec.id));
        // Anderer Streamer → kein Treffer.
        assert!(get_existing_report(&pool, "weekly", ps, pe, Some("other"))
            .await
            .is_none());
        // Globaler Report (streamer NULL) separat.
        insert_report(&pool, "weekly", None, ps, pe, "# Global", None)
            .await
            .unwrap();
        let global = get_existing_report(&pool, "weekly", ps, pe, None)
            .await
            .unwrap();
        assert_eq!(global.streamer_login, None);
        assert_eq!(global.model, None);

        // list: ohne Filter beide, kind-Filter greift, streamer-Filter greift.
        assert_eq!(list_reports(&pool, None, None, 20).await.len(), 2);
        assert_eq!(
            list_reports(&pool, Some("weekly"), Some("nani"), 20)
                .await
                .len(),
            1
        );
        assert_eq!(
            list_reports(&pool, Some("monthly"), None, 20).await.len(),
            0
        );
    }
}
