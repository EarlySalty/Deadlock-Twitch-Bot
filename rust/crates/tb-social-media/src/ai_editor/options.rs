use sqlx::PgPool;

use crate::autopilot_settings::{self, PublishLanguage};

use super::{AiEditorError, EditorOptions};

pub async fn load(pool: &PgPool, owner: &str) -> Result<EditorOptions, AiEditorError> {
    let settings = autopilot_settings::load(pool, owner)
        .await
        .map_err(|_| AiEditorError::Storage)?;
    Ok(EditorOptions {
        multi_output_enabled: settings.multi_output_enabled,
        experimental_ai_layout: settings.experimental_ai_layout,
        publish_language: settings.publish_language,
    })
}

pub async fn save(
    pool: &PgPool,
    owner: &str,
    options: EditorOptions,
) -> Result<EditorOptions, AiEditorError> {
    let mut tx = pool.begin().await.map_err(|_| AiEditorError::Storage)?;
    autopilot_settings::lock_owner(&mut tx, owner)
        .await
        .map_err(|_| AiEditorError::Storage)?;
    sqlx::query("INSERT INTO social_media_autopilot_settings (twitch_user_id, multi_output_enabled, experimental_ai_layout, publish_language) VALUES ($1, $2, $3, $4) ON CONFLICT (twitch_user_id) DO UPDATE SET multi_output_enabled = EXCLUDED.multi_output_enabled, experimental_ai_layout = EXCLUDED.experimental_ai_layout, publish_language = EXCLUDED.publish_language, updated_at = NOW()")
        .bind(owner)
        .bind(options.multi_output_enabled)
        .bind(options.experimental_ai_layout)
        .bind(language_code(options.publish_language))
        .execute(&mut *tx)
        .await
        .map_err(|_| AiEditorError::Storage)?;
    tx.commit().await.map_err(|_| AiEditorError::Storage)?;
    load(pool, owner).await
}

fn language_code(language: PublishLanguage) -> &'static str {
    match language {
        PublishLanguage::De => "de",
        PublishLanguage::En => "en",
        PublishLanguage::Auto => "auto",
    }
}
