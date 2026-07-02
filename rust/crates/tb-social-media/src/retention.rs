//! Retention/Publication-Logik (vollständiger Port von
//! `bot/social_media/retention.py`).
//!
//! Bestimmt, ob ein Clip auf allen *aktiven* Plattformen des Streamers
//! veröffentlicht ist, und pflegt daraus den `status` der Clip-Zeile
//! (`published_all` ↔ `pending`). Aktive Plattformen = solche mit aktivem
//! Auth-Record. Dazu der Discard-/Cleanup-Teil: Verwerfen einzelner Clips sowie
//! das Auslesen + Löschen abgelaufener Clips (für den Retention-Worker).

use std::collections::HashSet;

use sqlx::PgPool;

/// Plattform → Upload-Flag-Spalte.
const PLATFORM_UPLOAD_COLUMNS: [(&str, &str); 3] = [
    ("tiktok", "uploaded_tiktok"),
    ("youtube", "uploaded_youtube"),
    ("instagram", "uploaded_instagram"),
];

/// Plattformen, für die der Streamer (oder global) einen aktiven Auth-Record hat.
pub async fn get_active_platforms_for_streamer(
    pool: &PgPool,
    streamer_login: Option<&str>,
) -> HashSet<String> {
    let login = streamer_login.unwrap_or("").trim().to_lowercase();
    sqlx::query_scalar!(
        "SELECT DISTINCT platform FROM social_media_platform_auth \
         WHERE enabled = 1 AND (LOWER(COALESCE(streamer_login, '')) = LOWER($1) OR streamer_login IS NULL)",
        &login
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default()
    .into_iter()
    .collect()
}

/// `true`, wenn der Clip auf allen aktiven Plattformen hochgeladen ist (keine
/// aktiven Plattformen → `true`, mirror Python).
pub async fn is_clip_published_on_all_active_platforms(
    pool: &PgPool,
    clip_db_id: impl Into<i64>,
) -> bool {
    let clip_db_id = clip_db_id.into();
    let row = sqlx::query!(
        "SELECT streamer_login, COALESCE(uploaded_tiktok, false) AS \"uploaded_tiktok!\", \
                COALESCE(uploaded_youtube, false) AS \"uploaded_youtube!\", \
                COALESCE(uploaded_instagram, false) AS \"uploaded_instagram!\" \
         FROM twitch_clips_social_media WHERE id = $1 LIMIT 1",
        clip_db_id
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    let Some(row) = row else {
        return false;
    };
    let active = get_active_platforms_for_streamer(pool, Some(row.streamer_login.as_str())).await;
    if active.is_empty() {
        return true;
    }
    let uploaded = |col: &str| -> bool {
        match col {
            "uploaded_tiktok" => row.uploaded_tiktok,
            "uploaded_youtube" => row.uploaded_youtube,
            _ => row.uploaded_instagram,
        }
    };
    for (platform, col) in PLATFORM_UPLOAD_COLUMNS {
        if active.contains(platform) && !uploaded(col) {
            return false;
        }
    }
    true
}

/// Aktualisiert den Clip-`status` aus dem Publication-Stand (Python
/// `refresh_clip_publication_status`). Liefert, ob auf allen aktiven Plattformen
/// veröffentlicht.
pub async fn refresh_clip_publication_status(pool: &PgPool, clip_db_id: impl Into<i64>) -> bool {
    let clip_db_id = clip_db_id.into();
    let published_all = is_clip_published_on_all_active_platforms(pool, clip_db_id).await;
    if published_all {
        let _ = sqlx::query!(
            "UPDATE twitch_clips_social_media SET status = 'published_all' \
             WHERE id = $1 AND discarded_at IS NULL",
            clip_db_id
        )
        .execute(pool)
        .await;
    } else {
        let _ = sqlx::query!(
            "UPDATE twitch_clips_social_media \
             SET status = CASE WHEN discarded_at IS NOT NULL THEN status ELSE 'pending' END \
             WHERE id = $1 AND status = 'published_all'",
            clip_db_id
        )
        .execute(pool)
        .await;
    }
    published_all
}

/// Ein abgelaufener Clip (für den Retention-Cleanup). Timestamps als ISO-Text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpiredClip {
    pub id: i64,
    pub clip_id: Option<String>,
    pub streamer_login: Option<String>,
    pub source_kind: Option<String>,
    pub upload_local_path: Option<String>,
    pub local_file_path: Option<String>,
    pub retention_until: Option<String>,
    pub discarded_at: Option<String>,
    pub status: Option<String>,
}

/// Markiert einen Clip als verworfen (`discarded`). Liefert `true`, wenn eine
/// Zeile getroffen wurde.
pub async fn mark_clip_discarded(pool: &PgPool, clip_db_id: impl Into<i64>) -> bool {
    let clip_db_id = clip_db_id.into();
    sqlx::query_scalar!(
        "UPDATE twitch_clips_social_media \
            SET discarded_at = CURRENT_TIMESTAMP, status = 'discarded' \
          WHERE id = $1 RETURNING id AS \"id!\"",
        clip_db_id
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .is_some()
}

/// Clips, deren Retention-Frist (`retention_until`) bis `now` (rfc3339)
/// erreicht ist — älteste zuerst.
pub async fn iter_expired_clips_for_retention(pool: &PgPool, now: &str) -> Vec<ExpiredClip> {
    let rows = sqlx::query!(
            "SELECT id AS \"id!\", clip_id, streamer_login, source_kind, upload_local_path, local_file_path, \
                    retention_until::text, discarded_at::text, status \
               FROM twitch_clips_social_media \
              WHERE retention_until IS NOT NULL AND retention_until <= $1::text::timestamptz \
              ORDER BY retention_until ASC, id ASC",
            now
        )
        .fetch_all(pool)
        .await
        .unwrap_or_default();
    rows.into_iter()
        .map(|row| ExpiredClip {
            id: row.id,
            clip_id: Some(row.clip_id),
            streamer_login: Some(row.streamer_login),
            source_kind: Some(row.source_kind),
            upload_local_path: row.upload_local_path,
            local_file_path: row.local_file_path,
            retention_until: row.retention_until,
            discarded_at: row.discarded_at,
            status: row.status,
        })
        .collect()
}

/// Löscht die Clip-Zeilen mit den gegebenen IDs (leere Liste = No-op).
pub async fn delete_clips_by_ids(pool: &PgPool, clip_ids: &[i64]) {
    if clip_ids.is_empty() {
        return;
    }
    let _ = sqlx::query!(
        "DELETE FROM twitch_clips_social_media WHERE id = ANY($1::bigint[])",
        clip_ids
    )
    .execute(pool)
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
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
        sqlx::query("CREATE TABLE social_media_platform_auth (id SERIAL PRIMARY KEY, platform TEXT, streamer_login TEXT, enabled INTEGER DEFAULT 1)")
            .execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE twitch_clips_social_media (id BIGSERIAL PRIMARY KEY, clip_id TEXT NOT NULL, clip_url TEXT NOT NULL, streamer_login TEXT NOT NULL, source_kind TEXT NOT NULL DEFAULT 'twitch', upload_local_path TEXT, local_file_path TEXT, status TEXT DEFAULT 'pending', created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), retention_until TIMESTAMPTZ, discarded_at TIMESTAMPTZ, uploaded_tiktok BOOLEAN DEFAULT FALSE, uploaded_youtube BOOLEAN DEFAULT FALSE, uploaded_instagram BOOLEAN DEFAULT FALSE)")
            .execute(&pool).await.unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn publication_status_kette() {
        let Some(pool) = make_pool("t_sm_retention").await else {
            return;
        };
        // Aktive Plattformen für 'nani': tiktok (streamer) + youtube (global).
        sqlx::query("INSERT INTO social_media_platform_auth (platform, streamer_login) VALUES ('tiktok','nani'), ('youtube', NULL)").execute(&pool).await.unwrap();
        let active = get_active_platforms_for_streamer(&pool, Some("nani")).await;
        assert!(
            active.contains("tiktok")
                && active.contains("youtube")
                && !active.contains("instagram")
        );

        // Clip: nur tiktok hochgeladen → nicht alle aktiven.
        let id: i64 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login, uploaded_tiktok) VALUES ('pub-1', 'https://clips.test/pub-1', 'nani', TRUE) RETURNING id").fetch_one(&pool).await.unwrap();
        assert!(!is_clip_published_on_all_active_platforms(&pool, id).await);
        assert!(!refresh_clip_publication_status(&pool, id).await);

        // Jetzt auch youtube → alle aktiven veröffentlicht → published_all.
        sqlx::query("UPDATE twitch_clips_social_media SET uploaded_youtube = TRUE WHERE id = $1")
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();
        assert!(refresh_clip_publication_status(&pool, id).await);
        let status: String =
            sqlx::query_scalar("SELECT status FROM twitch_clips_social_media WHERE id = $1")
                .bind(id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(status, "published_all");
    }

    #[tokio::test]
    async fn keine_aktiven_plattformen_ist_published() {
        let Some(pool) = make_pool("t_sm_retention_none").await else {
            return;
        };
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login) VALUES ('none-1', 'https://clips.test/none-1', 'x') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        // Keine Auth-Records → keine aktiven Plattformen → published.
        assert!(is_clip_published_on_all_active_platforms(&pool, id).await);
    }

    #[tokio::test]
    async fn discard_und_cleanup() {
        let Some(pool) = make_pool("t_sm_retention_cleanup").await else {
            return;
        };
        // mark_clip_discarded setzt status + discarded_at, liefert true.
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login) VALUES ('discard-1', 'https://clips.test/discard-1', 'nani') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(mark_clip_discarded(&pool, id).await);
        let (status, discarded): (String, Option<String>) = sqlx::query_as(
            "SELECT status, discarded_at::text FROM twitch_clips_social_media WHERE id = $1",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(status, "discarded");
        assert!(discarded.is_some());
        // Nicht existierender Clip → false.
        assert!(!mark_clip_discarded(&pool, 999_999).await);

        // Zwei abgelaufene + ein zukünftiger Clip.
        let past1: i64 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login, source_kind, upload_local_path, retention_until) VALUES ('a', 'https://clips.test/a', 'nani', 'twitch', '/a.mp4', NOW() - INTERVAL '2 days') RETURNING id").fetch_one(&pool).await.unwrap();
        let past2: i64 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login, retention_until) VALUES ('b', 'https://clips.test/b', 'nani', NOW() - INTERVAL '1 day') RETURNING id").fetch_one(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login, retention_until) VALUES ('future', 'https://clips.test/future', 'nani', NOW() + INTERVAL '5 days')").execute(&pool).await.unwrap();

        let now = chrono::Utc::now().to_rfc3339();
        let expired = iter_expired_clips_for_retention(&pool, &now).await;
        let ids: Vec<i64> = expired.iter().map(|c| c.id).collect();
        // Älteste zuerst (past1 vor past2), zukünftiger NICHT dabei.
        assert_eq!(ids, vec![past1, past2]);
        assert_eq!(expired[0].clip_id.as_deref(), Some("a"));
        assert_eq!(expired[0].upload_local_path.as_deref(), Some("/a.mp4"));

        // delete_clips_by_ids entfernt sie; leere Liste = No-op.
        delete_clips_by_ids(&pool, &[]).await;
        delete_clips_by_ids(&pool, &ids).await;
        let remaining: Vec<i64> = sqlx::query_scalar(
            "SELECT id FROM twitch_clips_social_media WHERE clip_id IN ('a','b')",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert!(remaining.is_empty());
    }
}
