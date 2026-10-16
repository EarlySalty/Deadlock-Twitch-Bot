use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;

use crate::{
    autopilot_settings, posting_plan::PLATFORMS, produced_video, video_processor::format_hashtags,
};

use super::AiEditorError;

const YOUTUBE_TITLE_MAX: usize = 100;
const YOUTUBE_DESCRIPTION_MAX_BYTES: usize = 5000;
const CAPTION_MAX: usize = 2200;
const HASHTAG_MAX: usize = 30;
const HASHTAG_CHARS_MAX: usize = 100;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlatformCopy {
    pub title: String,
    pub description: String,
    pub hashtags: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct VersionMetadata {
    pub video_id: i64,
    pub editable: bool,
    pub targets: Vec<String>,
    pub platforms: BTreeMap<String, PlatformCopy>,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct CopyIssue {
    pub platform: String,
    pub field: &'static str,
    pub message: String,
}

#[derive(Debug)]
pub enum MetadataError {
    Editor(AiEditorError),
    Invalid(Vec<CopyIssue>),
}

impl From<AiEditorError> for MetadataError {
    fn from(error: AiEditorError) -> Self {
        Self::Editor(error)
    }
}

const UNCOMMITTED: &str = "v.publication_executed_at IS NULL AND NOT EXISTS (
    SELECT 1 FROM twitch_clips_upload_queue q WHERE q.produced_video_id = v.id
    AND (q.effect_started_at IS NOT NULL OR q.external_id IS NOT NULL
    OR q.status NOT IN ('waiting_exact_file', 'autopilot_held', 'autopilot_discarded')))";

pub fn normalize_hashtag(tag: &str) -> String {
    tag.trim().trim_start_matches('#').to_string()
}

pub fn normalize_copy(copy: PlatformCopy) -> PlatformCopy {
    PlatformCopy {
        title: copy.title.trim().to_string(),
        description: copy.description.trim().to_string(),
        hashtags: copy
            .hashtags
            .iter()
            .map(|tag| normalize_hashtag(tag))
            .filter(|tag| !tag.is_empty())
            .collect(),
    }
}

pub fn targets(metadata: &Value) -> Vec<String> {
    let stored = metadata.get("platforms").and_then(Value::as_object);
    let targets: Vec<String> = PLATFORMS
        .iter()
        .filter(|platform| stored.is_some_and(|copies| copies.contains_key(**platform)))
        .map(|platform| (*platform).to_string())
        .collect();
    if targets.is_empty() {
        PLATFORMS
            .iter()
            .map(|platform| (*platform).to_string())
            .collect()
    } else {
        targets
    }
}

fn stored_copy(value: Option<&Value>) -> PlatformCopy {
    let text = |key: &str| {
        value
            .and_then(|copy| copy.get(key))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let hashtags = value
        .and_then(|copy| copy.get("hashtags"))
        .and_then(Value::as_array)
        .map(|tags| {
            tags.iter()
                .filter_map(Value::as_str)
                .map(normalize_hashtag)
                .filter(|tag| !tag.is_empty())
                .collect()
        })
        .unwrap_or_default();
    PlatformCopy {
        title: text("title"),
        description: text("description"),
        hashtags,
    }
}

fn view(video: &produced_video::ProducedVideo, uncommitted: bool) -> VersionMetadata {
    let stored = video.metadata.get("platforms");
    let targets = targets(&video.metadata);
    let copy_of = |platform: &str| stored_copy(stored.and_then(|value| value.get(platform)));
    let mirror = targets
        .first()
        .map(|platform| copy_of(platform))
        .unwrap_or_default();
    let platforms = PLATFORMS
        .iter()
        .map(|platform| {
            let copy = if targets.iter().any(|target| target == platform) {
                copy_of(platform)
            } else {
                mirror.clone()
            };
            ((*platform).to_string(), copy)
        })
        .collect();
    VersionMetadata {
        video_id: video.id,
        editable: uncommitted
            && video.status == "review"
            && video.file_path.is_some()
            && video
                .expires_at
                .is_some_and(|expiry| expiry > chrono::Utc::now()),
        targets,
        platforms,
    }
}

async fn owned_video(
    pool: &PgPool,
    owner: &str,
    id: i64,
) -> Result<produced_video::ProducedVideo, AiEditorError> {
    produced_video::get(pool, owner, id)
        .await
        .map_err(|_| AiEditorError::Storage)?
        .filter(|video| video.status != "expired")
        .ok_or(AiEditorError::NotFound)
}

async fn uncommitted(pool: &PgPool, id: i64) -> Result<bool, AiEditorError> {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT {UNCOMMITTED} FROM social_produced_videos v WHERE v.id = $1"
    )))
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|_| AiEditorError::Storage)?
    .ok_or(AiEditorError::NotFound)
}

pub async fn load(pool: &PgPool, owner: &str, id: i64) -> Result<VersionMetadata, AiEditorError> {
    let video = owned_video(pool, owner, id).await?;
    let uncommitted = uncommitted(pool, video.id).await?;
    Ok(view(&video, uncommitted))
}

pub async fn save(
    pool: &PgPool,
    owner: &str,
    id: i64,
    platforms: BTreeMap<String, PlatformCopy>,
) -> Result<VersionMetadata, MetadataError> {
    let video = owned_video(pool, owner, id).await?;
    let targets = targets(&video.metadata);
    let platforms = normalized(platforms);
    let issues = validate_targets(&platforms, &targets);
    if !issues.is_empty() {
        return Err(MetadataError::Invalid(issues));
    }
    let selected: BTreeMap<String, PlatformCopy> = platforms
        .into_iter()
        .filter(|(platform, _)| targets.contains(platform))
        .collect();
    let payload = serde_json::to_string(&selected).map_err(|_| AiEditorError::Storage)?;
    let mut tx = pool.begin().await.map_err(|_| AiEditorError::Storage)?;
    autopilot_settings::lock_owner(&mut tx, owner)
        .await
        .map_err(|_| AiEditorError::Storage)?;
    let changed = sqlx::query(sqlx::AssertSqlSafe(format!(
        "UPDATE social_produced_videos v SET metadata = jsonb_set(v.metadata, '{{platforms}}', $3::text::jsonb)
         WHERE v.id = $1 AND v.twitch_user_id = $2 AND v.status = 'review' AND v.file_path IS NOT NULL
         AND v.expires_at > NOW() AND v.artifact_deleted_at IS NULL AND {UNCOMMITTED}"
    )))
    .bind(id)
    .bind(owner)
    .bind(payload)
    .execute(&mut *tx)
    .await
    .map_err(|_| AiEditorError::Storage)?
    .rows_affected();
    if changed != 1 {
        owned_video(pool, owner, id).await?;
        return Err(AiEditorError::Conflict.into());
    }
    sqlx::query("UPDATE twitch_clips_upload_queue q SET title=v.metadata->'platforms'->q.platform->>'title',description=v.metadata->'platforms'->q.platform->>'description',hashtags=(v.metadata->'platforms'->q.platform->'hashtags')::text FROM social_produced_videos v WHERE v.id=$1 AND v.twitch_user_id=$2 AND q.produced_video_id=v.id AND q.status IN ('waiting_exact_file','autopilot_held','autopilot_discarded') AND q.effect_started_at IS NULL AND q.external_id IS NULL")
        .bind(id).bind(owner).execute(&mut *tx).await.map_err(|_| AiEditorError::Storage)?;
    tx.commit().await.map_err(|_| AiEditorError::Storage)?;
    Ok(load(pool, owner, id).await?)
}

fn normalized(platforms: BTreeMap<String, PlatformCopy>) -> BTreeMap<String, PlatformCopy> {
    platforms
        .into_iter()
        .map(|(platform, copy)| (platform, normalize_copy(copy)))
        .collect()
}

pub fn validate(platforms: &BTreeMap<String, PlatformCopy>) -> Vec<CopyIssue> {
    let all: Vec<String> = PLATFORMS
        .iter()
        .map(|platform| (*platform).to_string())
        .collect();
    validate_targets(platforms, &all)
}

fn validate_targets(
    platforms: &BTreeMap<String, PlatformCopy>,
    targets: &[String],
) -> Vec<CopyIssue> {
    let mut issues = Vec::new();
    for platform in platforms.keys() {
        if !PLATFORMS.contains(&platform.as_str()) {
            issues.push(issue(
                platform,
                "platform",
                "Diese Plattform wird nicht unterstützt.",
            ));
        }
    }
    for platform in targets {
        match platforms.get(platform) {
            Some(copy) => issues.extend(platform_issues(platform, copy)),
            None => issues.push(issue(
                platform,
                "platform",
                "Für diese Plattform fehlen Titel und Beschreibung.",
            )),
        }
    }
    issues
}

pub fn summary(issues: &[CopyIssue]) -> String {
    issues
        .iter()
        .map(|issue| format!("{}: {}", platform_label(&issue.platform), issue.message))
        .collect::<Vec<_>>()
        .join(" ")
}

fn platform_label(platform: &str) -> &str {
    match platform {
        "youtube" => "YouTube",
        "tiktok" => "TikTok",
        "instagram" => "Instagram",
        other => other,
    }
}

fn issue(platform: &str, field: &'static str, message: &str) -> CopyIssue {
    CopyIssue {
        platform: platform.to_string(),
        field,
        message: message.to_string(),
    }
}

fn platform_issues(platform: &str, copy: &PlatformCopy) -> Vec<CopyIssue> {
    let mut issues = hashtag_issues(platform, &copy.hashtags);
    let tags = format_hashtags(&copy.hashtags);
    match platform {
        "youtube" => {
            if copy.title.is_empty() {
                issues.push(issue(platform, "title", "Bitte gib einen Titel ein."));
            } else if copy.title.chars().count() > YOUTUBE_TITLE_MAX {
                issues.push(issue(
                    platform,
                    "title",
                    "Der YouTube-Titel darf höchstens 100 Zeichen lang sein.",
                ));
            } else if copy.title.contains(['<', '>']) {
                issues.push(issue(
                    platform,
                    "title",
                    "YouTube erlaubt im Titel keine spitzen Klammern.",
                ));
            }
            let description = format!("{}\n\n{tags}", copy.description)
                .chars()
                .filter(|ch| *ch != '<' && *ch != '>')
                .collect::<String>();
            if description.trim_end().len() + "\n\n#Shorts".len() > YOUTUBE_DESCRIPTION_MAX_BYTES {
                issues.push(issue(
                    platform,
                    "description",
                    "Beschreibung und Hashtags sind für YouTube zusammen zu lang. Bitte kürze sie.",
                ));
            }
        }
        "tiktok" => {
            if copy.title.is_empty() {
                issues.push(issue(platform, "title", "Bitte gib einen Titel ein."));
            }
            let caption = [
                copy.title.as_str(),
                copy.description.as_str(),
                tags.as_str(),
            ]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n");
            if caption.encode_utf16().count() > CAPTION_MAX {
                issues.push(issue(
                    platform,
                    "description",
                    "Titel, Beschreibung und Hashtags dürfen bei TikTok zusammen höchstens 2200 Zeichen lang sein.",
                ));
            }
        }
        _ => {
            if copy.description.is_empty() && copy.hashtags.is_empty() {
                issues.push(issue(
                    platform,
                    "description",
                    "Bitte gib eine Beschreibung oder Hashtags ein.",
                ));
            }
            let caption = [copy.description.as_str(), tags.as_str()]
                .into_iter()
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join("\n\n");
            if caption.chars().count() > CAPTION_MAX {
                issues.push(issue(
                    platform,
                    "description",
                    "Beschreibung und Hashtags dürfen bei Instagram zusammen höchstens 2200 Zeichen lang sein.",
                ));
            }
        }
    }
    issues
}

fn hashtag_issues(platform: &str, hashtags: &[String]) -> Vec<CopyIssue> {
    let mut issues = Vec::new();
    if hashtags.len() > HASHTAG_MAX {
        issues.push(issue(
            platform,
            "hashtags",
            "Bitte verwende höchstens 30 Hashtags.",
        ));
    }
    if hashtags.iter().any(|tag| {
        tag.chars().count() > HASHTAG_CHARS_MAX
            || tag
                .chars()
                .any(|c| c.is_whitespace() || c == '#' || c == ',')
    }) {
        issues.push(issue(
            platform,
            "hashtags",
            "Hashtags dürfen keine Leerzeichen, Kommas oder weiteren Rauten enthalten und höchstens 100 Zeichen lang sein.",
        ));
    }
    issues
}

#[cfg(test)]
mod tests {
    use super::*;

    fn copy(title: &str, description: &str, hashtags: &[&str]) -> PlatformCopy {
        PlatformCopy {
            title: title.into(),
            description: description.into(),
            hashtags: hashtags.iter().map(|tag| (*tag).to_string()).collect(),
        }
    }

    fn all(copy: PlatformCopy) -> BTreeMap<String, PlatformCopy> {
        PLATFORMS
            .iter()
            .map(|platform| ((*platform).to_string(), copy.clone()))
            .collect()
    }

    #[test]
    fn platform_limits_follow_the_uploaders() {
        let valid = normalized(all(copy(" Titel ", "Text", &["#Deadlock", "Clips"])));
        assert!(validate(&valid).is_empty());
        assert_eq!(valid["tiktok"].hashtags, vec!["Deadlock", "Clips"]);
        let mut youtube_boundary = valid.clone();
        youtube_boundary
            .get_mut("youtube")
            .expect("youtube")
            .description = "a".repeat(4990);
        assert_eq!(validate(&youtube_boundary).len(), 1);
        youtube_boundary
            .get_mut("youtube")
            .expect("youtube")
            .hashtags
            .clear();
        assert!(validate(&youtube_boundary).is_empty());
        let long_title = all(copy(&"a".repeat(101), "Text", &[]));
        let issues = validate(&long_title);
        assert_eq!(issues.len(), 1);
        assert_eq!(
            (issues[0].platform.as_str(), issues[0].field),
            ("youtube", "title")
        );
        let long_caption = all(copy("Titel", &"ä".repeat(2500), &["Deadlock"]));
        let issues = validate(&long_caption);
        let platforms: Vec<&str> = issues.iter().map(|issue| issue.platform.as_str()).collect();
        assert_eq!(platforms, vec!["youtube", "tiktok", "instagram"]);
        let mut missing = all(copy("Titel", "Text", &[]));
        missing.remove("instagram");
        missing.insert("myspace".into(), copy("Titel", "Text", &[]));
        assert_eq!(validate(&missing).len(), 2);
        let spaced = all(copy("Titel", "Text", &["zwei worte"]));
        assert_eq!(validate(&spaced).len(), 3);
    }
}
