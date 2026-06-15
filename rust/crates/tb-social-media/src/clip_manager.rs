//! Dashboard-seitige Clip-Funktionen (Port des Rests von
//! `bot/social_media/clip_manager.py`).
//!
//! `register_manual_upload` legt einen manuell hochgeladenen Clip an (Quelle
//! `manual_upload`) und belegt das Default-Layout; `get_clips_for_dashboard`
//! liefert die Clip-Liste samt Anzahl offener Uploads für die Dashboard-Ansicht.
//! Die übrigen clip_manager-Teile (register_clip/fetch_recent_clips/Queue/
//! Templates/Analytics) sind bereits in eigenen Modulen.

use serde_json::{json, Value};
use sqlx::{PgPool, Row};

use crate::layout::apply_default_layout;
use crate::retention::refresh_clip_publication_status;

#[derive(Debug, thiserror::Error)]
pub enum ManualUploadError {
    #[error("clip_id already exists")]
    AlreadyExists,
    #[error("unknown streamer")]
    UnknownStreamer,
    #[error("db: {0}")]
    Db(#[from] sqlx::Error),
}

/// Registriert einen manuell hochgeladenen Clip und belegt das Default-Layout.
/// Liefert (clip_db_id, retention_until als ISO-Text).
pub async fn register_manual_upload(
    pool: &PgPool,
    clip_id: &str,
    streamer_login: &str,
    title: Option<&str>,
    local_path: &str,
    duration_seconds: f64,
) -> Result<(i32, String), ManualUploadError> {
    let created_at = chrono::Utc::now().to_rfc3339();

    if sqlx::query_scalar::<_, i32>("SELECT id FROM twitch_clips_social_media WHERE clip_id = $1")
        .bind(clip_id)
        .fetch_optional(pool)
        .await?
        .is_some()
    {
        return Err(ManualUploadError::AlreadyExists);
    }

    let streamer: Option<(Option<String>,)> = sqlx::query_as(
        "SELECT twitch_user_id FROM twitch_streamers WHERE LOWER(twitch_login) = LOWER($1) LIMIT 1",
    )
    .bind(streamer_login)
    .fetch_optional(pool)
    .await?;
    let Some((twitch_user_id,)) = streamer else {
        return Err(ManualUploadError::UnknownStreamer);
    };

    let (clip_db_id, retention_until): (i32, Option<String>) = sqlx::query_as(
        "INSERT INTO twitch_clips_social_media \
            (clip_id, clip_url, clip_title, clip_thumbnail_url, streamer_login, twitch_user_id, \
             created_at, duration_seconds, view_count, game_name, status, source_kind, \
             upload_local_path, local_file_path) \
         VALUES ($1, $2, $3, NULL, $4, $5, $6, $7, 0, NULL, 'pending', 'manual_upload', $2, $2) \
         RETURNING id, retention_until::text",
    )
    .bind(clip_id)
    .bind(local_path)
    .bind(title)
    .bind(streamer_login)
    .bind(&twitch_user_id)
    .bind(&created_at)
    .bind(duration_seconds)
    .fetch_one(pool)
    .await?;

    let _ = apply_default_layout(pool, clip_db_id, streamer_login).await;
    Ok((clip_db_id, retention_until.unwrap_or_default()))
}

/// Clips für die Dashboard-Anzeige (volle Zeile + Anzahl offener Uploads),
/// optional nach Streamer/Status gefiltert.
pub async fn get_clips_for_dashboard(
    pool: &PgPool,
    streamer_login: Option<&str>,
    status: Option<&str>,
    limit: i64,
) -> Vec<Value> {
    let rows = sqlx::query(
        "SELECT to_jsonb(c)::text AS clip, \
                COALESCE((SELECT COUNT(*) FROM twitch_clips_upload_queue q \
                          WHERE q.clip_id = c.id AND q.status = 'pending'), 0) AS pending_uploads \
           FROM twitch_clips_social_media c \
          WHERE ($1::text IS NULL OR LOWER(c.streamer_login) = LOWER($1)) \
            AND ($2::text IS NULL OR c.status = $2) \
          ORDER BY c.created_at DESC LIMIT $3",
    )
    .bind(streamer_login)
    .bind(status)
    .bind(limit)
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    rows.iter()
        .map(|r| {
            let clip_text: String = r.try_get("clip").unwrap_or_else(|_| "{}".to_string());
            let pending: i64 = r.try_get("pending_uploads").unwrap_or(0);
            let mut obj: Value = serde_json::from_str(&clip_text).unwrap_or_else(|_| json!({}));
            if let Some(map) = obj.as_object_mut() {
                map.insert("pending_uploads".to_string(), json!(pending));
            }
            obj
        })
        .collect()
}

/// Markiert einen Clip auf den gegebenen Plattformen als hochgeladen (manuell);
/// unbekannte Plattformen werden übersprungen. Liefert `false` bei DB-Fehler.
/// Frischt anschließend den Publication-Status auf.
pub async fn mark_clip_uploaded(pool: &PgPool, clip_db_id: i32, platforms: &[String], manual: bool) -> bool {
    let now = chrono::Utc::now().to_rfc3339();
    let updates = async {
        let mut tx = pool.begin().await?;
        for platform in platforms {
            let sql = match platform.as_str() {
                "tiktok" => "UPDATE twitch_clips_social_media SET uploaded_tiktok = 1, tiktok_uploaded_at = $1 WHERE id = $2",
                "youtube" => "UPDATE twitch_clips_social_media SET uploaded_youtube = 1, youtube_uploaded_at = $1 WHERE id = $2",
                "instagram" => "UPDATE twitch_clips_social_media SET uploaded_instagram = 1, instagram_uploaded_at = $1 WHERE id = $2",
                _ => continue,
            };
            sqlx::query(sql).bind(&now).bind(clip_db_id).execute(&mut *tx).await?;
        }
        tx.commit().await
    }
    .await;
    if updates.is_err() {
        return false;
    }
    tracing::info!(clip_db_id, ?platforms, manual, "Clip als hochgeladen markiert");
    refresh_clip_publication_status(pool, clip_db_id).await;
    true
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
        for ddl in [
            "CREATE TABLE twitch_streamers (twitch_login TEXT PRIMARY KEY, twitch_user_id TEXT)",
            "CREATE TABLE twitch_clips_social_media (id SERIAL PRIMARY KEY, clip_id TEXT UNIQUE, clip_url TEXT, clip_title TEXT, clip_thumbnail_url TEXT, streamer_login TEXT, twitch_user_id TEXT, created_at TEXT, duration_seconds DOUBLE PRECISION, view_count INTEGER DEFAULT 0, game_name TEXT, status TEXT DEFAULT 'pending', source_kind TEXT, upload_local_path TEXT, local_file_path TEXT, retention_until TIMESTAMPTZ DEFAULT (NOW() + INTERVAL '14 days'), discarded_at TIMESTAMPTZ, layout_override_json JSONB, uploaded_tiktok INTEGER DEFAULT 0, uploaded_youtube INTEGER DEFAULT 0, uploaded_instagram INTEGER DEFAULT 0, tiktok_uploaded_at TEXT, youtube_uploaded_at TEXT, instagram_uploaded_at TEXT)",
            "CREATE TABLE social_media_streamer_layout (streamer_login TEXT PRIMARY KEY, layout_json JSONB NOT NULL, cam_enabled BOOLEAN NOT NULL DEFAULT TRUE, mode TEXT NOT NULL DEFAULT 'pip', updated_at TIMESTAMPTZ DEFAULT NOW(), updated_by TEXT)",
            "CREATE TABLE twitch_clips_upload_queue (id SERIAL PRIMARY KEY, clip_id INTEGER, platform TEXT, status TEXT DEFAULT 'pending')",
            "CREATE TABLE social_media_platform_auth (id SERIAL PRIMARY KEY, platform TEXT, streamer_login TEXT, enabled INTEGER DEFAULT 1)",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        Some(pool)
    }

    #[tokio::test]
    async fn manual_upload_und_dashboard_liste() {
        let Some(pool) = make_pool("t_sm_clip_manager").await else { return };
        sqlx::query("INSERT INTO twitch_streamers (twitch_login, twitch_user_id) VALUES ('nani', '123')").execute(&pool).await.unwrap();

        // Registrierung.
        let (id, retention) = register_manual_upload(&pool, "m1", "nani", Some("Mein Upload"), "/data/v.mp4", 30.0).await.unwrap();
        assert!(id > 0);
        assert!(!retention.is_empty());
        let (kind, path, status, layout): (String, String, String, Option<String>) = sqlx::query_as("SELECT source_kind, upload_local_path, status, layout_override_json::text FROM twitch_clips_social_media WHERE id = $1").bind(id).fetch_one(&pool).await.unwrap();
        assert_eq!(kind, "manual_upload");
        assert_eq!(path, "/data/v.mp4");
        assert_eq!(status, "pending");
        assert!(layout.is_some()); // apply_default_layout lief

        // Duplikat + unbekannter Streamer.
        assert!(matches!(register_manual_upload(&pool, "m1", "nani", None, "/x.mp4", 1.0).await, Err(ManualUploadError::AlreadyExists)));
        assert!(matches!(register_manual_upload(&pool, "m2", "ghost", None, "/x.mp4", 1.0).await, Err(ManualUploadError::UnknownStreamer)));

        // Dashboard-Liste.
        let clips = get_clips_for_dashboard(&pool, Some("nani"), None, 50).await;
        assert_eq!(clips.len(), 1);
        assert_eq!(clips[0]["clip_id"], "m1");
        assert_eq!(clips[0]["pending_uploads"], 0);
        // Status-Filter greift.
        assert_eq!(get_clips_for_dashboard(&pool, None, Some("processing"), 50).await.len(), 0);
        // Pending-Upload erhöht den Zähler.
        sqlx::query("INSERT INTO twitch_clips_upload_queue (clip_id, platform, status) VALUES ($1, 'tiktok', 'pending')").bind(id).execute(&pool).await.unwrap();
        assert_eq!(get_clips_for_dashboard(&pool, Some("nani"), None, 50).await[0]["pending_uploads"], 1);
    }

    #[tokio::test]
    async fn mark_uploaded_setzt_flags_und_refresh() {
        let Some(pool) = make_pool("t_sm_mark_uploaded").await else { return };
        // tiktok ist die einzige aktive Plattform für nani.
        sqlx::query("INSERT INTO social_media_platform_auth (platform, streamer_login) VALUES ('tiktok', 'nani')").execute(&pool).await.unwrap();
        let clip: i32 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, streamer_login) VALUES ('m1', 'nani') RETURNING id").fetch_one(&pool).await.unwrap();

        // tiktok markieren (+ unbekannte Plattform wird übersprungen).
        assert!(mark_clip_uploaded(&pool, clip, &["tiktok".into(), "snapchat".into()], true).await);
        let (up, at, status): (i32, Option<String>, String) = sqlx::query_as("SELECT uploaded_tiktok, tiktok_uploaded_at, status FROM twitch_clips_social_media WHERE id = $1").bind(clip).fetch_one(&pool).await.unwrap();
        assert_eq!(up, 1);
        assert!(at.is_some());
        // tiktok einzige aktive Plattform + jetzt hochgeladen → published_all.
        assert_eq!(status, "published_all");
    }
}
