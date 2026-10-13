use sqlx::PgPool;
use tb_domain::{UiLang, UiLangHints};

pub use tb_domain::{
    UiLang as ChannelUiLang, UiLangHints as ChannelUiLangHints, COMMUNITY_GUILD_ID,
};

pub async fn channel_ui_lang(pool: &PgPool, twitch_user_id: &str) -> UiLang {
    let stream_language = lookup_stream_language(pool, twitch_user_id).await;
    UiLang::resolve(UiLangHints {
        guild_id: None,
        guild_language: None,
        stream_language: stream_language.as_deref(),
    })
}

async fn lookup_stream_language(pool: &PgPool, twitch_user_id: &str) -> Option<String> {
    let twitch_user_id = twitch_user_id.trim();
    if twitch_user_id.is_empty() {
        return None;
    }
    let from_updates: Result<Option<String>, sqlx::Error> = sqlx::query_scalar(
        "SELECT language FROM twitch_channel_updates
          WHERE twitch_user_id = $1
            AND NULLIF(BTRIM(language), '') IS NOT NULL
          ORDER BY id DESC
          LIMIT 1",
    )
    .bind(twitch_user_id)
    .fetch_optional(pool)
    .await;
    if let Ok(Some(code)) = from_updates {
        return Some(code);
    }
    sqlx::query_scalar(
        "SELECT NULLIF(BTRIM(broadcaster_language), '')
           FROM category_channels
          WHERE user_id = $1
          LIMIT 1",
    )
    .bind(twitch_user_id)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_id_is_german() {
        assert_eq!(UiLang::from_code(None), UiLang::De);
    }
}
