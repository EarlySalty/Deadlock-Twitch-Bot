use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use tokio::io::AsyncReadExt;

use crate::clip_prep_worker::{download_atomic, register_local_file, ClipDownloader};
use crate::layout::StreamerLayout;

use super::{AiEditorError, EditorRuntime, ResourceLimits};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SourceIdentity {
    pub twitch_clip_id: String,
    pub path: PathBuf,
    pub sha256: String,
    pub bytes: u64,
    pub login: String,
    pub title: String,
}

pub(super) struct SourceRecord {
    pub twitch_clip_id: String,
    pub path: PathBuf,
    pub login: String,
    pub title: String,
    pub layout: Option<Value>,
    pub template: Value,
}

pub(super) fn numeric_owner(owner: &str) -> Result<(), AiEditorError> {
    if owner.starts_with('0')
        || owner.is_empty()
        || !owner.bytes().all(|b| b.is_ascii_digit())
        || owner.parse::<i64>().ok().is_none_or(|id| id <= 0)
    {
        return Err(AiEditorError::Source);
    }
    Ok(())
}

pub fn authorized_path(root: &Path, path: &Path, max_bytes: u64) -> Result<PathBuf, AiEditorError> {
    let root = root.canonicalize().map_err(|_| AiEditorError::Source)?;
    let path = path.canonicalize().map_err(|_| AiEditorError::Source)?;
    let metadata = std::fs::metadata(&path).map_err(|_| AiEditorError::Source)?;
    if !path.starts_with(root)
        || !metadata.is_file()
        || metadata.len() == 0
        || metadata.len() > max_bytes
    {
        return Err(AiEditorError::Source);
    }
    Ok(path)
}

fn classify_download(error: &str) -> AiEditorError {
    let lower = error.to_ascii_lowercase();
    if ["http error 404", "http error 410", "no longer available"]
        .iter()
        .any(|marker| lower.contains(marker))
    {
        AiEditorError::SourceMissing
    } else {
        AiEditorError::SourceDownload
    }
}

fn stored_path(runtime: &EditorRuntime, stored: &str) -> PathBuf {
    let path = PathBuf::from(stored);
    if path.is_absolute() {
        path
    } else {
        runtime.package_root.join(path)
    }
}

fn owned_source_path(runtime: &EditorRuntime, path: &Path) -> Result<PathBuf, AiEditorError> {
    let canonical = authorized_path(
        &runtime.limits.source_root,
        path,
        runtime.limits.resources.max_source_bytes,
    )
    .map_err(|_| AiEditorError::SourceUnreadable)?;
    let canonical_root = runtime
        .limits
        .source_root
        .canonicalize()
        .map_err(|_| AiEditorError::SourceUnreadable)?;
    let relative = canonical
        .strip_prefix(canonical_root)
        .map_err(|_| AiEditorError::SourceUnreadable)?;
    Ok(runtime.limits.source_root.join(relative))
}

async fn ensure_local_file(
    pool: &PgPool,
    runtime: &EditorRuntime,
    clip_id: i64,
    clip_url: &str,
    stored: Option<&str>,
    downloader: Option<&dyn ClipDownloader>,
) -> Result<PathBuf, AiEditorError> {
    let stored = stored.filter(|path| !path.is_empty());
    if let Some(stored) = stored {
        if let Ok(path) = owned_source_path(runtime, &stored_path(runtime, stored)) {
            return Ok(path);
        }
    }
    let unreadable = stored.is_some();
    if clip_url.trim().is_empty() {
        return Err(if unreadable {
            AiEditorError::SourceUnreadable
        } else {
            AiEditorError::SourceMissing
        });
    }
    let Some(downloader) = downloader else {
        return Err(if unreadable {
            AiEditorError::SourceUnreadable
        } else {
            AiEditorError::SourceDownload
        });
    };
    tokio::fs::create_dir_all(&runtime.limits.source_root)
        .await
        .map_err(|_| AiEditorError::SourceDownload)?;
    let dest = runtime.limits.source_root.join(format!("{clip_id}.mp4"));
    let target = dest
        .to_str()
        .ok_or(AiEditorError::SourceDownload)?
        .to_owned();
    if let Err(error) = download_atomic(downloader, clip_url, &target).await {
        return Err(classify_download(&error));
    }
    register_local_file(pool, clip_id, &target)
        .await
        .map_err(|_| AiEditorError::Storage)?;
    owned_source_path(runtime, Path::new(&target))
}

pub(super) async fn load_source(
    pool: &PgPool,
    runtime: &EditorRuntime,
    owner: &str,
    clip_id: i64,
    downloader: Option<&dyn ClipDownloader>,
) -> Result<SourceRecord, AiEditorError> {
    numeric_owner(owner)?;
    let row = sqlx::query("SELECT c.clip_id, c.clip_url, c.local_file_path, c.streamer_login, COALESCE(c.clip_title, '') AS title,
        c.layout_override_json::text AS override_json, l.layout_json::text AS layout_json, l.cam_enabled, l.mode,
        c.custom_description, c.hashtags
        FROM twitch_clips_social_media c JOIN social_media_category k ON k.category_key = c.category_key
        LEFT JOIN social_media_streamer_layout l ON l.twitch_user_id = c.twitch_user_id
        WHERE c.id = $1 AND c.twitch_user_id = $2 AND c.source_kind = 'twitch' AND c.category_key = 'deadlock'
        AND c.game_id = k.twitch_game_id AND k.twitch_game_id IS NOT NULL AND c.discarded_at IS NULL")
        .bind(clip_id).bind(owner).fetch_optional(pool).await.map_err(|_| AiEditorError::Storage)?
        .ok_or(AiEditorError::Source)?;
    let stored: Option<String> = row
        .try_get("local_file_path")
        .map_err(|_| AiEditorError::Storage)?;
    let clip_url: Option<String> = row
        .try_get("clip_url")
        .map_err(|_| AiEditorError::Storage)?;
    let clip_url = clip_url.unwrap_or_default();
    let path = ensure_local_file(
        pool,
        runtime,
        clip_id,
        &clip_url,
        stored.as_deref(),
        downloader,
    )
    .await?;
    let override_json: Option<String> = row
        .try_get("override_json")
        .map_err(|_| AiEditorError::Storage)?;
    let layout_json: Option<String> = row
        .try_get("layout_json")
        .map_err(|_| AiEditorError::Storage)?;
    let layout = if let Some(raw) = override_json {
        Some(parse_layout(&raw, None, None)?)
    } else if let Some(raw) = layout_json {
        Some(parse_layout(
            &raw,
            row.try_get("cam_enabled")
                .map_err(|_| AiEditorError::Storage)?,
            row.try_get::<Option<String>, _>("mode")
                .map_err(|_| AiEditorError::Storage)?
                .as_deref(),
        )?)
    } else {
        None
    };
    let template = sqlx::query("SELECT COALESCE(description_template, '') AS description, hashtags FROM clip_templates_streamer WHERE twitch_user_id = $1 AND is_default ORDER BY id LIMIT 1")
        .bind(owner).fetch_optional(pool).await.map_err(|_| AiEditorError::Storage)?;
    let template = match template {
        Some(template) => serde_json::json!({
            "description_template": template.try_get::<String, _>("description").map_err(|_| AiEditorError::Storage)?,
            "hashtags": template.try_get::<String, _>("hashtags").map_err(|_| AiEditorError::Storage)?,
        }),
        None => serde_json::json!({
            "description_template": row.try_get::<Option<String>, _>("custom_description").map_err(|_| AiEditorError::Storage)?,
            "hashtags": row.try_get::<Option<String>, _>("hashtags").map_err(|_| AiEditorError::Storage)?,
        }),
    };
    let source = SourceRecord {
        twitch_clip_id: row.try_get("clip_id").map_err(|_| AiEditorError::Storage)?,
        path,
        login: row
            .try_get("streamer_login")
            .map_err(|_| AiEditorError::Storage)?,
        title: row.try_get("title").map_err(|_| AiEditorError::Storage)?,
        layout,
        template,
    };
    if source.twitch_clip_id.is_empty()
        || source.twitch_clip_id.len() > 256
        || source.login.is_empty()
        || source.login.len() > 64
        || source.title.len() > runtime.limits.plan.max_text_bytes
    {
        return Err(AiEditorError::Source);
    }
    Ok(source)
}

fn parse_layout(raw: &str, cam: Option<bool>, mode: Option<&str>) -> Result<Value, AiEditorError> {
    if raw.len() > 8192 {
        return Err(AiEditorError::Source);
    }
    let payload: Value = serde_json::from_str(raw).map_err(|_| AiEditorError::Source)?;
    StreamerLayout::from_stored_value(&payload, cam, mode)
        .map(|layout| layout.to_override_json())
        .map_err(|_| AiEditorError::Source)
}

pub(super) async fn fingerprint(
    path: &Path,
    max_bytes: u64,
) -> Result<(String, u64), AiEditorError> {
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|_| AiEditorError::Source)?;
    let mut hash = Sha256::new();
    let mut bytes = 0_u64;
    let mut buffer = [0_u8; 65_536];
    loop {
        let count = file
            .read(&mut buffer)
            .await
            .map_err(|_| AiEditorError::Source)?;
        if count == 0 {
            break;
        }
        bytes = bytes
            .checked_add(count as u64)
            .ok_or(AiEditorError::Source)?;
        if bytes > max_bytes {
            return Err(AiEditorError::Source);
        }
        hash.update(&buffer[..count]);
    }
    if bytes == 0 {
        return Err(AiEditorError::Source);
    }
    let sha256 = hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Ok((sha256, bytes))
}

impl SourceIdentity {
    pub async fn capture(
        source: &SourceRecord,
        limits: &ResourceLimits,
    ) -> Result<Self, AiEditorError> {
        let (sha256, bytes) = fingerprint(&source.path, limits.max_source_bytes).await?;
        Ok(Self {
            twitch_clip_id: source.twitch_clip_id.clone(),
            path: source.path.clone(),
            sha256,
            bytes,
            login: source.login.clone(),
            title: source.title.clone(),
        })
    }

    pub async fn recheck(
        &self,
        pool: &PgPool,
        runtime: &EditorRuntime,
        owner: &str,
        clip_id: i64,
    ) -> Result<(), AiEditorError> {
        numeric_owner(owner)?;
        let row: Option<(String, Option<String>)> = sqlx::query_as("SELECT c.clip_id, c.local_file_path FROM twitch_clips_social_media c JOIN social_media_category k ON k.category_key = c.category_key WHERE c.id = $1 AND c.twitch_user_id = $2 AND c.source_kind = 'twitch' AND c.category_key = 'deadlock' AND c.game_id = k.twitch_game_id AND k.twitch_game_id IS NOT NULL AND c.discarded_at IS NULL")
            .bind(clip_id).bind(owner).fetch_optional(pool).await.map_err(|_| AiEditorError::Storage)?;
        let (twitch_clip_id, path) = row.ok_or(AiEditorError::Source)?;
        let path = PathBuf::from(path.ok_or(AiEditorError::Source)?);
        let path = if path.is_absolute() {
            path
        } else {
            runtime.package_root.join(path)
        };
        let canonical = authorized_path(
            &runtime.limits.source_root,
            &path,
            runtime.limits.resources.max_source_bytes,
        )?;
        let root = runtime
            .limits
            .source_root
            .canonicalize()
            .map_err(|_| AiEditorError::Source)?;
        let relative = canonical
            .strip_prefix(root)
            .map_err(|_| AiEditorError::Source)?;
        let path = runtime.limits.source_root.join(relative);
        if twitch_clip_id != self.twitch_clip_id || path != self.path {
            return Err(AiEditorError::Source);
        }
        let (hash, bytes) = fingerprint(&path, runtime.limits.resources.max_source_bytes).await?;
        if hash != self.sha256 || bytes != self.bytes {
            return Err(AiEditorError::Source);
        }
        Ok(())
    }
}

pub(super) struct Workspace(pub PathBuf);

impl Workspace {
    pub fn create(runtime: &EditorRuntime) -> Result<Self, AiEditorError> {
        let clips = runtime
            .limits
            .output_root
            .canonicalize()
            .map_err(|_| AiEditorError::Configuration)?;
        let parent = runtime
            .limits
            .work_root
            .parent()
            .ok_or(AiEditorError::Configuration)?
            .canonicalize()
            .map_err(|_| AiEditorError::Configuration)?;
        if parent != clips {
            return Err(AiEditorError::Configuration);
        }
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        match builder.create(&runtime.limits.work_root) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err(AiEditorError::Storage),
        }
        let metadata = std::fs::symlink_metadata(&runtime.limits.work_root)
            .map_err(|_| AiEditorError::Configuration)?;
        let root = runtime
            .limits
            .work_root
            .canonicalize()
            .map_err(|_| AiEditorError::Configuration)?;
        if !root.starts_with(clips) || !metadata.is_dir() {
            return Err(AiEditorError::Configuration);
        }
        let path = root.join(format!("job-{}", tb_crypto::random_hex_token(16)));
        builder.create(&path).map_err(|_| AiEditorError::Storage)?;
        Ok(Self(path))
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!("AI editor workspace cleanup failed");
            }
        }
    }
}

#[cfg(test)]
mod classify_download_tests {
    use super::{classify_download, AiEditorError};

    #[test]
    fn only_gone_clips_are_permanent_and_other_download_errors_retry() {
        for missing in [
            "ERROR: [twitch:clips] abc: HTTP Error 404: Not Found",
            "ERROR: [twitch:clips] abc: HTTP Error 410: Gone",
            "ERROR: [twitch:clips] abc: This clip is no longer available",
        ] {
            let error = classify_download(missing);
            assert!(matches!(error, AiEditorError::SourceMissing));
            assert!(!error.is_transient(), "{missing}");
        }
        for retried in [
            "ERROR: [twitch:clips] abc: HTTP Error 503: Service Unavailable",
            "ERROR: Requested format is not available",
            "ERROR: ffmpeg not found",
        ] {
            let error = classify_download(retried);
            assert!(matches!(error, AiEditorError::SourceDownload));
            assert!(error.is_transient(), "{retried}");
        }
    }
}
