use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;

use crate::autopilot_publish::tiktok_account_on;
use crate::autopilot_settings::SettingsError;
use crate::uploaders::tiktok::{build_caption, DirectPost, CAPTION_MAX};
use crate::uploaders::truncate_utf16;

pub const PRIVACY_LEVELS: [&str; 4] = [
    "PUBLIC_TO_EVERYONE",
    "MUTUAL_FOLLOW_FRIENDS",
    "FOLLOWER_OF_CREATOR",
    "SELF_ONLY",
];
const MAX_FIXED_HASHTAGS: usize = 10;
const MAX_HASHTAG_CHARS: usize = 50;
const MAX_FOOTER_CHARS: usize = 500;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TikTokPublishing {
    pub privacy_level: String,
    pub allow_comment: bool,
    pub allow_duet: bool,
    pub allow_stitch: bool,
    pub fixed_hashtags: Vec<String>,
    pub caption_footer: String,
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
        if !self.privacy_level.is_empty() && !PRIVACY_LEVELS.contains(&self.privacy_level.as_str())
        {
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
        let tail = build_caption(
            "",
            self.caption_footer.trim(),
            &merge_tags(hashtags, &self.fixed_hashtags),
        );
        let room = CAPTION_MAX.saturating_sub(tail.encode_utf16().count() + 2);
        let head = truncate_utf16(&build_caption(title, description, &[]), room);
        build_caption(head.trim(), &tail, &[])
    }

    pub fn direct_post(
        &self,
        review: bool,
        music_consent: bool,
        title: &str,
        description: &str,
        hashtags: &[String],
    ) -> Option<DirectPost> {
        if review || !music_consent || self.privacy_level.is_empty() {
            return None;
        }
        Some(DirectPost {
            caption: self.caption(title, description, hashtags),
            privacy_level: self.privacy_level.clone(),
            allow_comment: self.allow_comment,
            allow_duet: self.allow_duet,
            allow_stitch: self.allow_stitch,
        })
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

#[derive(Debug, Clone, Serialize)]
pub struct TikTokPublishingView {
    #[serde(flatten)]
    pub settings: TikTokPublishing,
    pub music_consent: bool,
    pub account_connected: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TikTokPublishingInput {
    #[serde(flatten)]
    pub settings: TikTokPublishing,
    #[serde(default)]
    pub music_consent: bool,
}

fn consent_key(owner: &str, account: &str) -> String {
    format!("tiktok_music_consent:{owner}:{account}")
}

pub async fn view(pool: &PgPool, owner: &str) -> Result<TikTokPublishingView, SettingsError> {
    let settings = load(pool, owner).await?;
    let mut connection = pool.acquire().await?;
    let account = tiktok_account_on(&mut connection, owner).await?;
    Ok(TikTokPublishingView {
        settings,
        music_consent: account.music_consent,
        account_connected: account.platform_user_id.is_some(),
    })
}

pub async fn save_view(
    pool: &PgPool,
    owner: &str,
    input: &TikTokPublishingInput,
) -> Result<TikTokPublishingView, SettingsError> {
    save(pool, owner, &input.settings).await?;
    let mut connection = pool.acquire().await?;
    let account = tiktok_account_on(&mut connection, owner).await?;
    if let Some(account) = account.platform_user_id.as_deref() {
        let key = consent_key(owner, account);
        if input.music_consent {
            let value = json!({"platform_user_id": account, "consented_at": chrono::Utc::now()});
            crate::settings::set_setting(pool, &key, &value, Some(owner)).await?;
        } else {
            sqlx::query("DELETE FROM social_media_settings WHERE key = $1")
                .bind(&key)
                .execute(pool)
                .await?;
        }
    }
    view(pool, owner).await
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

    #[test]
    fn long_description_is_cut_before_footer_and_fixed_tags() {
        let settings = TikTokPublishing {
            fixed_hashtags: vec!["ddc".into()],
            caption_footer: "Discord: discord.gg/z5TfVHuQq2".into(),
            ..TikTokPublishing::default()
        };
        let caption = settings.caption("Titel", &"ä".repeat(3000), &["fyp".into()]);
        assert!(caption.encode_utf16().count() <= CAPTION_MAX);
        assert!(caption.ends_with("Discord: discord.gg/z5TfVHuQq2\n\n#fyp #ddc"));
        assert!(caption.starts_with("Titel\n\nää"));
    }

    #[test]
    fn direct_post_needs_chosen_visibility_consent_and_auto_mode() {
        let chosen = TikTokPublishing {
            privacy_level: "SELF_ONLY".into(),
            ..TikTokPublishing::default()
        };
        assert!(TikTokPublishing::default()
            .direct_post(false, true, "T", "", &[])
            .is_none());
        assert!(chosen.direct_post(false, false, "T", "", &[]).is_none());
        assert!(chosen.direct_post(true, true, "T", "", &[]).is_none());
        let post = chosen.direct_post(false, true, "T", "", &[]);
        assert_eq!(
            post.map(|post| post.privacy_level),
            Some("SELF_ONLY".into())
        );
    }

    #[test]
    fn unset_visibility_is_valid_and_unknown_is_rejected() {
        assert!(TikTokPublishing::default().validate().is_ok());
        let unknown = TikTokPublishing {
            privacy_level: "EVERYONE".into(),
            ..TikTokPublishing::default()
        };
        assert!(unknown.validate().is_err());
    }
}
