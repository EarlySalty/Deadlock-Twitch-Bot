use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::autopilot_settings::SettingsError;
use crate::uploaders::tiktok::build_caption;

pub const PRIVACY_LEVELS: [&str; 4] = [
    "PUBLIC_TO_EVERYONE",
    "MUTUAL_FOLLOW_FRIENDS",
    "FOLLOWER_OF_CREATOR",
    "SELF_ONLY",
];
const MAX_FIXED_HASHTAGS: usize = 10;
const MAX_HASHTAG_CHARS: usize = 50;
const MAX_FOOTER_CHARS: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TikTokPublishing {
    pub privacy_level: String,
    pub allow_comment: bool,
    pub allow_duet: bool,
    pub allow_stitch: bool,
    pub fixed_hashtags: Vec<String>,
    pub caption_footer: String,
}

impl Default for TikTokPublishing {
    fn default() -> Self {
        Self {
            privacy_level: "PUBLIC_TO_EVERYONE".into(),
            allow_comment: true,
            allow_duet: true,
            allow_stitch: true,
            fixed_hashtags: Vec::new(),
            caption_footer: String::new(),
        }
    }
}

fn tag_body(tag: &str) -> &str {
    tag.trim().trim_start_matches('#')
}

fn tag_valid(tag: &str) -> bool {
    let body = tag_body(tag);
    !body.is_empty()
        && body.chars().count() <= MAX_HASHTAG_CHARS
        && !body
            .chars()
            .any(|c| c.is_whitespace() || c == '#' || c == ',')
}

fn merge_tags(generated: &[String], fixed: &[String]) -> Vec<String> {
    let mut tags: Vec<String> = Vec::new();
    for tag in generated.iter().chain(fixed) {
        let body = tag_body(tag);
        if body.is_empty() || tags.iter().any(|known| known.eq_ignore_ascii_case(body)) {
            continue;
        }
        tags.push(body.to_string());
    }
    tags
}

impl TikTokPublishing {
    pub fn validate(&self) -> Result<(), SettingsError> {
        if !PRIVACY_LEVELS.contains(&self.privacy_level.as_str()) {
            return Err(SettingsError::Invalid(
                "Unbekannte TikTok-Sichtbarkeit".into(),
            ));
        }
        if self.fixed_hashtags.len() > MAX_FIXED_HASHTAGS {
            return Err(SettingsError::Invalid(
                "Höchstens 10 feste Hashtags sind möglich".into(),
            ));
        }
        if !self.fixed_hashtags.iter().all(|tag| tag_valid(tag)) {
            return Err(SettingsError::Invalid(
                "Hashtags dürfen keine Leerzeichen, Kommas oder weiteren # enthalten".into(),
            ));
        }
        if self.caption_footer.chars().count() > MAX_FOOTER_CHARS {
            return Err(SettingsError::Invalid(
                "Der feste Zusatztext darf höchstens 500 Zeichen lang sein".into(),
            ));
        }
        Ok(())
    }

    pub fn caption(&self, title: &str, description: &str, hashtags: &[String]) -> String {
        let body = [description.trim(), self.caption_footer.trim()]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n");
        build_caption(title, &body, &merge_tags(hashtags, &self.fixed_hashtags))
    }

    fn normalized(&self) -> Self {
        Self {
            fixed_hashtags: merge_tags(&[], &self.fixed_hashtags),
            caption_footer: self.caption_footer.trim().to_string(),
            ..self.clone()
        }
    }
}

pub async fn load(pool: &PgPool, owner: &str) -> Result<TikTokPublishing, SettingsError> {
    let raw: Option<String> = sqlx::query_scalar(
        "SELECT tiktok_publishing::text FROM social_media_autopilot_settings WHERE twitch_user_id = $1",
    )
    .bind(owner)
    .fetch_optional(pool)
    .await?;
    match raw {
        Some(raw) => Ok(serde_json::from_str(&raw)?),
        None => Ok(TikTokPublishing::default()),
    }
}

pub async fn save(
    pool: &PgPool,
    owner: &str,
    settings: &TikTokPublishing,
) -> Result<TikTokPublishing, SettingsError> {
    settings.validate()?;
    let settings = settings.normalized();
    sqlx::query(
        "INSERT INTO social_media_autopilot_settings (twitch_user_id, tiktok_publishing) VALUES ($1, $2::text::jsonb)
         ON CONFLICT (twitch_user_id) DO UPDATE SET tiktok_publishing = EXCLUDED.tiktok_publishing, updated_at = NOW()",
    )
    .bind(owner)
    .bind(serde_json::to_string(&settings)?)
    .execute(pool)
    .await?;
    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caption_appends_footer_and_fixed_tags_once() {
        let settings = TikTokPublishing {
            fixed_hashtags: vec!["#deadlock".into(), "ddc".into()],
            caption_footer: "Discord: discord.gg/z5TfVHuQq2".into(),
            ..TikTokPublishing::default()
        };
        let caption = settings.caption(
            "Doppelkill",
            "Haze räumt ab.",
            &["Deadlock".into(), "#fyp".into()],
        );
        assert_eq!(
            caption,
            "Doppelkill\n\nHaze räumt ab.\n\nDiscord: discord.gg/z5TfVHuQq2\n\n#Deadlock #fyp #ddc"
        );
    }
}
