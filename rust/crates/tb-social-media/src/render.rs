use sqlx::PgPool;

use crate::enrichment::get_enrichment;
use crate::layout::get_clip_stored_layout;
use crate::subtitles::{ass_from_segments, build_branded_ass, correct_segments, segment_subtitles, SubtitleSegment};
use crate::video_processor::{VideoProcessor, VideoProcessorError};
use crate::vocab::load_all_vocab;

/// Untertitel-Schalter des Streamers zum Clip. Fehlt die Einstellung, ist der
/// Standard an (Default TRUE in der Spalte, hier gespiegelt).
pub async fn subtitles_enabled_for_clip(pool: &PgPool, clip_db_id: i64) -> bool {
    sqlx::query_scalar!(
        "SELECT COALESCE(s.subtitles_enabled, TRUE) AS \"enabled!\" \
           FROM twitch_clips_social_media c \
           LEFT JOIN social_media_streamer_settings s \
             ON LOWER(s.streamer_login) = LOWER(c.streamer_login) \
          WHERE c.id = $1 LIMIT 1",
        clip_db_id
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .unwrap_or(true)
}

/// Baut die ASS-Untertiteldatei fuer einen Clip aus dem gespeicherten Transkript
/// (Vokabel-Korrektur inklusive). `None`, wenn kein Transkript vorliegt.
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
    clip_db_id: i64,
    input_path: &str,
    output_path: &str,
    max_duration: i64,
) -> Result<(), VideoProcessorError> {
    let layout = get_clip_stored_layout(pool, clip_db_id).await;
    let (login, clip_title, custom_title): (String, Option<String>, Option<String>) =
        sqlx::query_as(
            "SELECT streamer_login, clip_title, custom_title FROM twitch_clips_social_media WHERE id = $1",
        )
        .bind(clip_db_id)
        .fetch_one(pool)
        .await
        .map_err(|error| VideoProcessorError::Parse(error.to_string()))?;
    let cues = if subtitles_enabled_for_clip(pool, clip_db_id).await {
        if let Ok(id) = i32::try_from(clip_db_id) {
            if let Some(enrichment) = get_enrichment(pool, id).await {
                let segments: Vec<SubtitleSegment> = enrichment
                    .transcript_segments
                    .iter()
                    .filter_map(SubtitleSegment::from_json)
                    .collect();
                let vocab = load_all_vocab(pool).await;
                segment_subtitles(&correct_segments(&segments, &vocab))
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };
    let source_duration = vp.get_video_info(input_path).await?.duration;
    let duration = if source_duration > 0.0 { source_duration.min(max_duration as f64) } else { max_duration as f64 };
    let cam_height = layout
        .as_ref()
        .filter(|value| value.cam_enabled && value.mode == "stacked")
        .map(|value| value.cam_position.clamped_to_target().h)
        .unwrap_or(600);
    let ass = build_branded_ass(
        &cues,
        custom_title.as_deref().filter(|s| !s.trim().is_empty()).or(clip_title.as_deref()).unwrap_or(""),
        &login,
        cam_height,
        duration,
    );
    vp.render_branded(input_path, output_path, max_duration, layout.as_ref(), &ass).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = crate::test_support::test_dsn()?;
        let admin = PgPoolOptions::new().max_connections(1).connect(&dsn).await.unwrap();
        sqlx::query(crate::test_sql::drop_schema(schema, true)).execute(&admin).await.unwrap();
        sqlx::query(crate::test_sql::create_schema(schema, false)).execute(&admin).await.unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn).unwrap().options([("search_path", schema)]);
        let pool = PgPoolOptions::new().max_connections(3).connect_with(opts).await.unwrap();
        for ddl in [
            "CREATE TABLE twitch_clips_social_media (id SERIAL PRIMARY KEY, streamer_login TEXT)",
            "CREATE TABLE social_media_streamer_settings (streamer_login TEXT PRIMARY KEY, subtitles_enabled BOOLEAN NOT NULL DEFAULT TRUE)",
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
        let a: i32 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (streamer_login) VALUES ('nani') RETURNING id").fetch_one(&pool).await.unwrap();
        // Ohne Settings-Zeile: Default an.
        assert!(subtitles_enabled_for_clip(&pool, a as i64).await);
        // Ausgeschaltet.
        sqlx::query("INSERT INTO social_media_streamer_settings (streamer_login, subtitles_enabled) VALUES ('nani', FALSE)").execute(&pool).await.unwrap();
        assert!(!subtitles_enabled_for_clip(&pool, a as i64).await);
    }

    #[tokio::test]
    async fn ass_aus_gespeichertem_transkript() {
        let Some(pool) = make_pool("t_sm_render_ass").await else {
            return;
        };
        let clip: i32 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (streamer_login) VALUES ('nani') RETURNING id").fetch_one(&pool).await.unwrap();
        // Ohne Transkript: kein ASS.
        sqlx::query("INSERT INTO social_media_clip_enrichment (clip_db_id, status) VALUES ($1, 'done')").bind(clip).execute(&pool).await.unwrap();
        assert!(build_clip_ass(&pool, clip as i64).await.is_none());
        // Mit Segmenten + Vokabel: ASS mit korrigiertem Text.
        sqlx::query("INSERT INTO deadlock_vocab (term, canonical, category) VALUES ('haze','Haze','hero')").execute(&pool).await.unwrap();
        sqlx::query("UPDATE social_media_clip_enrichment SET transcript_segments = $1::text::jsonb WHERE clip_db_id = $2")
            .bind(r#"[{"start_seconds":0.0,"end_seconds":2.0,"text":"haze ist stark"}]"#)
            .bind(clip)
            .execute(&pool).await.unwrap();
        let ass = build_clip_ass(&pool, clip as i64).await.expect("ASS aus Transkript");
        assert!(ass.contains("Haze ist stark"), "{ass}");
    }
}
