use sqlx::PgPool;

use crate::enrichment::get_enrichment;
use crate::subtitles::ass_from_segments;
use crate::video_processor::{VideoProcessor, VideoProcessorError};
use crate::vocab::load_all_vocab;

pub async fn subtitles_enabled_for_clip(pool: &PgPool, clip_db_id: i64) -> bool {
    sqlx::query_scalar!(
        "SELECT COALESCE(s.subtitles_enabled, TRUE) AS \"enabled!\" \
           FROM twitch_clips_social_media c \
           LEFT JOIN social_media_streamer_settings s \
             ON s.twitch_user_id = c.twitch_user_id \
          WHERE c.id = $1 LIMIT 1",
        clip_db_id
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .unwrap_or(true)
}

pub async fn build_clip_ass(pool: &PgPool, clip_db_id: i64) -> Option<String> {
    let clip_db_id_i32 = i32::try_from(clip_db_id).ok()?;
    let enrichment = get_enrichment(pool, clip_db_id_i32).await?;
    if enrichment.transcript_segments.is_empty() {
        return None;
    }
    let vocab = load_all_vocab(pool).await;
    ass_from_segments(&enrichment.transcript_segments, &vocab)
}

pub async fn render_clip_vertical(
    vp: &VideoProcessor,
    pool: &PgPool,
    input_path: &str,
    output_path: &str,
    max_duration: i64,
) -> Result<(), VideoProcessorError> {
    let Some(mut lease) = crate::production_lease::acquire_production_lease(pool)
        .await
        .map_err(VideoProcessorError::ProductionLease)?
    else {
        return Err(VideoProcessorError::ProductionBusy);
    };
    let result =
        render_clip_vertical_with_lease(&mut lease, vp, input_path, output_path, max_duration)
            .await;
    lease
        .release()
        .await
        .map_err(VideoProcessorError::ProductionLease)?;
    result
}

pub async fn render_clip_vertical_with_lease(
    lease: &mut crate::production_lease::ProductionLease,
    vp: &VideoProcessor,
    input_path: &str,
    output_path: &str,
    max_duration: i64,
) -> Result<(), VideoProcessorError> {
    lease
        .ensure_held()
        .await
        .map_err(VideoProcessorError::ProductionLease)?;
    lease.mark_claimed_work();
    vp.render_native(input_path, output_path, max_duration)
        .await
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
        sqlx::query(crate::test_sql::drop_schema(schema, true))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(crate::test_sql::create_schema(schema, false))
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
        for ddl in [
            "CREATE TABLE twitch_clips_social_media (twitch_user_id TEXT DEFAULT '42', id SERIAL PRIMARY KEY, streamer_login TEXT)",
            "CREATE TABLE social_media_streamer_settings (twitch_user_id TEXT DEFAULT '42', streamer_login TEXT PRIMARY KEY, subtitles_enabled BOOLEAN NOT NULL DEFAULT TRUE)",
            "CREATE TABLE social_media_clip_enrichment (clip_db_id INTEGER PRIMARY KEY, transcript_raw TEXT, transcript_corrected TEXT, transcript_segments JSONB, transcript_lang TEXT, detected_terms JSONB DEFAULT '[]'::jsonb, title_youtube TEXT, title_tiktok TEXT, title_instagram TEXT, description_youtube TEXT, description_tiktok TEXT, description_instagram TEXT, hashtags_youtube JSONB DEFAULT '[]'::jsonb, hashtags_tiktok JSONB DEFAULT '[]'::jsonb, hashtags_instagram JSONB DEFAULT '[]'::jsonb, llm_provider TEXT, llm_model TEXT, cost_usd_estimate NUMERIC(10,6), status TEXT DEFAULT 'pending', error_message TEXT, started_at TIMESTAMPTZ, completed_at TIMESTAMPTZ, edited_by TEXT, updated_at TIMESTAMPTZ DEFAULT NOW())",
            "CREATE TABLE deadlock_vocab (term TEXT PRIMARY KEY, canonical TEXT NOT NULL, category TEXT NOT NULL, source TEXT NOT NULL DEFAULT 'manual', aliases JSONB NOT NULL DEFAULT '[]'::jsonb, weight INTEGER NOT NULL DEFAULT 1, updated_at TIMESTAMPTZ DEFAULT NOW())",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        Some(pool)
    }

    #[tokio::test]
    async fn untertitel_default_an_und_abschaltbar() {
        let Some(pool) = make_pool("t_sm_render_flag").await else {
            return;
        };
        let a: i32 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media (streamer_login) VALUES ('nani') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(subtitles_enabled_for_clip(&pool, a as i64).await);
        sqlx::query("INSERT INTO social_media_streamer_settings (streamer_login, subtitles_enabled) VALUES ('nani', FALSE)").execute(&pool).await.unwrap();
        assert!(!subtitles_enabled_for_clip(&pool, a as i64).await);
    }

    #[tokio::test]
    async fn ass_aus_gespeichertem_transkript() {
        let Some(pool) = make_pool("t_sm_render_ass").await else {
            return;
        };
        let clip: i32 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media (streamer_login) VALUES ('nani') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO social_media_clip_enrichment (clip_db_id, status) VALUES ($1, 'done')",
        )
        .bind(clip)
        .execute(&pool)
        .await
        .unwrap();
        assert!(build_clip_ass(&pool, clip as i64).await.is_none());
        sqlx::query(
            "INSERT INTO deadlock_vocab (term, canonical, category) VALUES ('haze','Haze','hero')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("UPDATE social_media_clip_enrichment SET transcript_segments = $1::text::jsonb WHERE clip_db_id = $2")
            .bind(r#"[{"start_seconds":0.0,"end_seconds":2.0,"text":"haze ist stark"}]"#)
            .bind(clip)
            .execute(&pool).await.unwrap();
        let ass = build_clip_ass(&pool, clip as i64)
            .await
            .expect("ASS aus Transkript");
        assert!(ass.contains("Haze ist stark"), "{ass}");
    }
}
