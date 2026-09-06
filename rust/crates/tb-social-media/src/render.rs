use sqlx::PgPool;

use crate::enrichment::get_enrichment;
use crate::layout::{get_clip_stored_layout, StreamerLayout, TARGET_HEIGHT, TARGET_WIDTH};
use crate::subtitles::ass_from_segments;
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

async fn render_base(
    vp: &VideoProcessor,
    layout: &Option<StreamerLayout>,
    input_path: &str,
    output_path: &str,
    max_duration: i64,
) -> Result<(), VideoProcessorError> {
    match layout {
        Some(l) => vp.compose_and_trim(input_path, output_path, max_duration, l).await,
        None => {
            vp.convert_and_trim(input_path, output_path, max_duration, TARGET_WIDTH, TARGET_HEIGHT)
                .await
        }
    }
}

/// Rendert einen Clip ins Hochformat: gespeichertes Layout (sonst Center-Crop),
/// danach eingebrannte Untertitel, falls der Streamer sie anhat und ein
/// Transkript vorliegt. Zentraler Baustein fuer Upload, Vorschau und Batch.
pub async fn render_clip_vertical(
    vp: &VideoProcessor,
    pool: &PgPool,
    clip_db_id: i64,
    input_path: &str,
    output_path: &str,
    max_duration: i64,
) -> Result<(), VideoProcessorError> {
    let layout = get_clip_stored_layout(pool, clip_db_id).await;

    let ass = if subtitles_enabled_for_clip(pool, clip_db_id).await {
        build_clip_ass(pool, clip_db_id).await
    } else {
        None
    };

    match ass {
        Some(ass_content) => {
            let base = format!("{output_path}.pre.mp4");
            render_base(vp, &layout, input_path, &base, max_duration).await?;
            let ass_path = format!("{output_path}.ass");
            tokio::fs::write(&ass_path, ass_content).await?;
            let burned = vp.burn_subtitles(&base, output_path, &ass_path).await;
            let _ = tokio::fs::remove_file(&base).await;
            let _ = tokio::fs::remove_file(&ass_path).await;
            burned?;
        }
        None => render_base(vp, &layout, input_path, output_path, max_duration).await?,
    }
    Ok(())
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
