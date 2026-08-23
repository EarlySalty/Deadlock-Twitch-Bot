//! Analytics-Zusammenfassung fürs Dashboard (Port von
//! `clip_manager.get_analytics_summary`).
//!
//! Aggregiert Clip-Upload-Zahlen, ausstehende Queue-Jobs je Plattform und die
//! Social-Analytics der letzten 30 Tage — optional streamer-gefiltert. Die
//! with/without-Streamer-Branches sind über `($1 IS NULL OR …)` zu je einer
//! Query vereint.

use serde_json::{json, Value};
use sqlx::PgPool;

/// Analytics-Summary (Python-Struktur: `clips` / `queue` / `analytics`).
pub async fn get_analytics_summary(pool: &PgPool, streamer_login: Option<&str>) -> Value {
    // Clip-Upload-Zahlen.
    let clip_stats = sqlx::query!(
        "SELECT COUNT(*) AS \"total!\", \
                SUM(CASE WHEN uploaded_tiktok THEN 1 ELSE 0 END) AS tiktok_uploads, \
                SUM(CASE WHEN uploaded_youtube THEN 1 ELSE 0 END) AS youtube_uploads, \
                SUM(CASE WHEN uploaded_instagram THEN 1 ELSE 0 END) AS instagram_uploads \
         FROM twitch_clips_social_media c \
         WHERE ($1::text IS NULL OR c.streamer_login = $1)",
        streamer_login
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let clips = match clip_stats {
        Some(row) => json!({
            "total": row.total,
            "tiktok_uploads": row.tiktok_uploads.unwrap_or(0),
            "youtube_uploads": row.youtube_uploads.unwrap_or(0),
            "instagram_uploads": row.instagram_uploads.unwrap_or(0),
        }),
        None => json!({}),
    };

    // Ausstehende Queue-Jobs je Plattform.
    let queue_rows = sqlx::query!(
        "SELECT q.platform AS \"platform?\", COUNT(*) AS \"pending!\" FROM twitch_clips_upload_queue q \
         JOIN twitch_clips_social_media c ON c.id = q.clip_id \
         WHERE q.status = 'pending' AND ($1::text IS NULL OR c.streamer_login = $1) \
         GROUP BY q.platform",
        streamer_login
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    let mut queue = serde_json::Map::new();
    for row in queue_rows {
        queue.insert(row.platform.unwrap_or_default(), json!(row.pending));
    }

    // Social-Analytics der letzten 30 Tage je Plattform.
    let analytics_rows = sqlx::query!(
        "SELECT a.platform AS \"platform?\", COUNT(DISTINCT a.clip_id) AS \"clips_n!\", \
                SUM(a.views) AS views, SUM(a.likes) AS likes, SUM(a.comments) AS comments, SUM(a.shares) AS shares \
         FROM twitch_clips_social_analytics a \
         JOIN twitch_clips_social_media c ON c.id = a.clip_id \
         WHERE a.synced_at > NOW() - INTERVAL '30 days' \
           AND ($1::text IS NULL OR c.streamer_login = $1) \
         GROUP BY a.platform",
        streamer_login
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    let mut analytics = serde_json::Map::new();
    for row in analytics_rows {
        analytics.insert(
            row.platform.unwrap_or_default(),
            json!({
                "clips": row.clips_n,
                "views": row.views.unwrap_or(0),
                "likes": row.likes.unwrap_or(0),
                "comments": row.comments.unwrap_or(0),
                "shares": row.shares.unwrap_or(0),
            }),
        );
    }

    json!({ "clips": clips, "queue": Value::Object(queue), "analytics": Value::Object(analytics) })
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
            .max_connections(2)
            .connect_with(opts)
            .await
            .unwrap();
        for ddl in [
            "CREATE TABLE twitch_clips_social_media (id SERIAL PRIMARY KEY, streamer_login TEXT, uploaded_tiktok BOOLEAN DEFAULT FALSE, uploaded_youtube BOOLEAN DEFAULT FALSE, uploaded_instagram BOOLEAN DEFAULT FALSE)",
            "CREATE TABLE twitch_clips_upload_queue (id SERIAL PRIMARY KEY, clip_id INTEGER, platform TEXT, status TEXT)",
            "CREATE TABLE twitch_clips_social_analytics (id SERIAL PRIMARY KEY, clip_id INTEGER, platform TEXT, views INTEGER, likes INTEGER, comments INTEGER, shares INTEGER, synced_at TIMESTAMPTZ)",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        Some(pool)
    }

    #[tokio::test]
    async fn analytics_summary_aggregiert() {
        let Some(pool) = make_pool("t_sm_analytics").await else {
            return;
        };
        // 2 Clips für nani: 1× tiktok hochgeladen.
        let c1: i32 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (streamer_login, uploaded_tiktok) VALUES ('nani', TRUE) RETURNING id").fetch_one(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_clips_social_media (streamer_login) VALUES ('nani')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO twitch_clips_social_media (streamer_login, uploaded_youtube) VALUES ('other', TRUE)").execute(&pool).await.unwrap();
        // Pending Queue-Job tiktok.
        sqlx::query("INSERT INTO twitch_clips_upload_queue (clip_id, platform, status) VALUES ($1, 'youtube', 'pending')").bind(c1).execute(&pool).await.unwrap();
        // Analytics: tiktok views.
        sqlx::query("INSERT INTO twitch_clips_social_analytics (clip_id, platform, views, likes, comments, shares, synced_at) VALUES ($1, 'tiktok', 100, 10, 5, 2, NOW())").bind(c1).execute(&pool).await.unwrap();

        let summary = get_analytics_summary(&pool, Some("nani")).await;
        assert_eq!(summary["clips"]["total"], 2);
        assert_eq!(summary["clips"]["tiktok_uploads"], 1);
        assert_eq!(summary["clips"]["youtube_uploads"], 0); // 'other' nicht gezaehlt
        assert_eq!(summary["queue"]["youtube"], 1);
        assert_eq!(summary["analytics"]["tiktok"]["views"], 100);
        assert_eq!(summary["analytics"]["tiktok"]["likes"], 10);

        // Ohne Filter: alle 3 Clips.
        let all = get_analytics_summary(&pool, None).await;
        assert_eq!(all["clips"]["total"], 3);
        assert_eq!(all["clips"]["youtube_uploads"], 1); // 'other'
    }
}
