use std::path::Path;
use std::sync::Arc;

use sqlx::PgPool;

use crate::clip_prep_worker::{download_atomic, ClipDownloader};
use crate::render::render_clip_vertical;
use crate::video_processor::VideoProcessor;

const BATCH_MAX_SECS: i64 = 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchClip {
    pub clip_db_id: i64,
    pub clip_id: String,
    pub clip_url: String,
    pub local_file_path: Option<String>,
}

/// Alle Clips eines Streamers, aufgeloest ueber die Twitch-User-ID (nie ueber den
/// Login). Neueste zuerst.
pub async fn select_clips_for_user(pool: &PgPool, twitch_user_id: &str) -> Vec<BatchClip> {
    let uid = twitch_user_id.trim();
    if uid.is_empty() {
        return Vec::new();
    }
    let rows = sqlx::query!(
        "SELECT id AS \"id!\", clip_id AS \"clip_id!\", clip_url AS \"clip_url!\", local_file_path \
           FROM twitch_clips_social_media \
          WHERE twitch_user_id = $1 AND discarded_at IS NULL \
          ORDER BY created_at DESC NULLS LAST, id DESC",
        uid
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    rows.into_iter()
        .map(|r| BatchClip {
            clip_db_id: r.id,
            clip_id: r.clip_id,
            clip_url: r.clip_url,
            local_file_path: r.local_file_path,
        })
        .collect()
}

pub struct BatchResult {
    pub rendered: Vec<String>,
    pub failed: Vec<(String, String)>,
}

/// Rendert alle Clips eines Streamers nach REQ-01 bis REQ-03 (Layout, Blur-Rand,
/// Untertitel) in `out_dir`, ohne Upload. Laedt fehlende Clips per `downloader`
/// nach `clips_dir`.
pub async fn render_all_for_user(
    pool: &PgPool,
    vp: &VideoProcessor,
    downloader: Arc<dyn ClipDownloader>,
    twitch_user_id: &str,
    out_dir: &str,
    clips_dir: &str,
) -> BatchResult {
    let clips = select_clips_for_user(pool, twitch_user_id).await;
    let _ = tokio::fs::create_dir_all(out_dir).await;
    let _ = tokio::fs::create_dir_all(clips_dir).await;
    let mut result = BatchResult { rendered: Vec::new(), failed: Vec::new() };

    for clip in clips {
        let input = match ensure_local(&clip, downloader.as_ref(), clips_dir).await {
            Ok(path) => path,
            Err(e) => {
                result.failed.push((clip.clip_id.clone(), format!("download: {e}")));
                continue;
            }
        };
        let output = format!("{out_dir}/{}.mp4", clip.clip_id);
        match render_clip_vertical(vp, pool, clip.clip_db_id, &input, &output, BATCH_MAX_SECS).await {
            Ok(()) => result.rendered.push(output),
            Err(e) => result.failed.push((clip.clip_id.clone(), format!("render: {e}"))),
        }
    }
    result
}

async fn ensure_local(
    clip: &BatchClip,
    downloader: &dyn ClipDownloader,
    clips_dir: &str,
) -> Result<String, String> {
    if let Some(path) = clip.local_file_path.as_deref().filter(|s| !s.is_empty()) {
        if Path::new(path).exists() {
            return Ok(path.to_string());
        }
    }
    let dest = format!("{clips_dir}/{}.mp4", clip.clip_db_id);
    download_atomic(downloader, &clip.clip_url, &dest).await?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = crate::test_support::test_dsn()?;
        let admin = PgPoolOptions::new().max_connections(1).connect(&dsn).await.unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).execute(&admin).await.unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&admin).await.unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn).unwrap().options([("search_path", schema)]);
        let pool = PgPoolOptions::new().max_connections(3).connect_with(opts).await.unwrap();
        sqlx::query("CREATE TABLE twitch_clips_social_media (id BIGSERIAL PRIMARY KEY, clip_id TEXT NOT NULL, clip_url TEXT NOT NULL, streamer_login TEXT, twitch_user_id TEXT, local_file_path TEXT, discarded_at TIMESTAMPTZ, created_at TIMESTAMPTZ DEFAULT NOW())")
            .execute(&pool).await.unwrap();
        Some(pool)
    }

    #[tokio::test]
    async fn auswahl_nur_ueber_twitch_user_id() {
        let Some(pool) = make_pool("t_sm_batch_select").await else {
            return;
        };
        // Zwei Clips fuer earlysalty (1186925760), einer fuer einen Fremden.
        sqlx::query("INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login, twitch_user_id) VALUES ('a','https://c/a','earlysalty','1186925760'), ('b','https://c/b','earlysalty','1186925760'), ('z','https://c/z','other','999')").execute(&pool).await.unwrap();
        // Ein verworfener Clip des Nutzers zaehlt nicht.
        sqlx::query("INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login, twitch_user_id, discarded_at) VALUES ('d','https://c/d','earlysalty','1186925760', NOW())").execute(&pool).await.unwrap();

        let clips = select_clips_for_user(&pool, "1186925760").await;
        let ids: Vec<&str> = clips.iter().map(|c| c.clip_id.as_str()).collect();
        assert_eq!(clips.len(), 2, "genau die zwei aktiven Clips des Nutzers: {ids:?}");
        assert!(ids.contains(&"a") && ids.contains(&"b"));
        assert!(!ids.contains(&"z"), "Fremd-Clip nicht enthalten");
        assert!(!ids.contains(&"d"), "verworfener Clip nicht enthalten");

        // Leere User-ID -> nichts.
        assert!(select_clips_for_user(&pool, "  ").await.is_empty());
    }
}
