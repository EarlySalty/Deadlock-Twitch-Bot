//! Retention-Worker (Port von `bot/social_media/retention_worker.py`).
//!
//! Löscht abgelaufene Social-Media-Clips — aber erst, wenn sie entweder
//! verworfen (`discarded_at`) ODER auf allen aktiven Plattformen veröffentlicht
//! sind. Pro Treffer wird die lokale Datei entfernt und die Clip-Zeile gelöscht.
//! An/Aus 1:1: in Python dauerhaft an (kein Gate), Intervall 30min.

use std::path::PathBuf;
use std::time::Duration;

use chrono::Utc;
use sqlx::{PgPool, Row};

use crate::preparation::DEFAULT_CLIPS_DIR;
use crate::retention::iter_expired_clips_for_retention;

const INTERVAL_SECS: u64 = 30 * 60;
const INITIAL_DELAY_SECS: u64 = 30;

/// Worker, der abgelaufene Clips aufräumt.
pub struct RetentionWorker {
    pool: PgPool,
    interval: Duration,
    clips_dir: PathBuf,
}

impl RetentionWorker {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            interval: Duration::from_secs(INTERVAL_SECS),
            clips_dir: PathBuf::from(DEFAULT_CLIPS_DIR),
        }
    }

    pub fn with_clips_dir(mut self, clips_dir: impl Into<PathBuf>) -> Self {
        self.clips_dir = clips_dir.into();
        self
    }

    /// Ein Durchlauf (Python `_cleanup_expired_clips`).
    pub async fn run_once(&self) {
        self.reconcile_quarantine().await;
        let now = Utc::now().to_rfc3339();
        let candidates = match iter_expired_clips_for_retention(&self.pool, &now).await {
            Ok(candidates) => candidates,
            Err(error) => {
                tracing::error!(
                    code = "retention_candidates_load_failed",
                    database_code = ?error
                        .as_database_error()
                        .and_then(|database| database.code()),
                    "Social-Media-Retention: Kandidaten konnten nicht sicher geladen werden"
                );
                return;
            }
        };
        for clip in candidates {
            if let Err(error) = self.cleanup_clip(clip.id).await {
                tracing::warn!(
                    clip_db_id = clip.id,
                    code = "retention_cleanup_failed",
                    error = %error,
                    "Social-Media-Retention: Clip konnte nicht sicher bereinigt werden"
                );
            }
        }
    }

    async fn reconcile_quarantine(&self) {
        #[cfg(target_os = "linux")]
        {
            use std::os::fd::AsRawFd;
            use std::os::unix::fs::MetadataExt;
            const O_DIRECTORY: i32 = 0o200000;
            const O_NOFOLLOW: i32 = 0o400000;
            const O_NONBLOCK: i32 = 0o4000;
            let mut directory_options = tokio::fs::OpenOptions::new();
            directory_options
                .read(true)
                .custom_flags(O_DIRECTORY | O_NOFOLLOW | O_NONBLOCK);
            let Ok(root) = open_directory_chain(&self.clips_dir, &directory_options).await else {
                return;
            };
            let quarantine_path = std::path::PathBuf::from(format!(
                "/proc/self/fd/{}/.retention-quarantine",
                root.as_raw_fd()
            ));
            let quarantine = match directory_options.open(&quarantine_path).await {
                Ok(directory) => directory,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
                Err(_) => {
                    tracing::warn!(
                        code = "retention_quarantine_rejected",
                        "Social-Media-Retention: Quarantäne-Verzeichnis wurde abgelehnt"
                    );
                    return;
                }
            };
            let read_path =
                std::path::PathBuf::from(format!("/proc/self/fd/{}", quarantine.as_raw_fd()));
            let Ok(mut entries) = tokio::fs::read_dir(read_path).await else {
                return;
            };
            while let Ok(Some(entry)) = entries.next_entry().await {
                let name = entry.file_name();
                let Some((clip_db_id, kind)) = parse_quarantine_name(&name) else {
                    tracing::warn!(
                        code = "retention_quarantine_name_rejected",
                        "Social-Media-Retention: unbekannter Quarantäne-Eintrag bleibt erhalten"
                    );
                    continue;
                };
                let anchored = std::path::PathBuf::from(format!(
                    "/proc/self/fd/{}/{}",
                    quarantine.as_raw_fd(),
                    name.to_string_lossy()
                ));
                let mut file_options = tokio::fs::OpenOptions::new();
                file_options
                    .read(true)
                    .custom_flags(O_NOFOLLOW | O_NONBLOCK);
                let Ok(file) = file_options.open(&anchored).await else {
                    continue;
                };
                let Ok(metadata) = file.metadata().await else {
                    continue;
                };
                if !metadata.file_type().is_file()
                    || metadata.nlink() != 1
                    || metadata.mode() & 0o777 != 0o640
                {
                    tracing::warn!(
                        clip_db_id,
                        code = "retention_quarantine_file_rejected",
                        "Social-Media-Retention: unsicherer Quarantäne-Eintrag bleibt erhalten"
                    );
                    continue;
                }

                let mut transaction = match self.pool.begin().await {
                    Ok(transaction) => transaction,
                    Err(_) => return,
                };
                let preparation = match sqlx::query(
                    "SELECT state, render_path, render_fingerprint \
                     FROM social_media_clip_preparation WHERE clip_db_id = $1 FOR UPDATE",
                )
                .bind(clip_db_id)
                .fetch_optional(transaction.as_mut())
                .await
                {
                    Ok(row) => row,
                    Err(_) => {
                        let _ = transaction.rollback().await;
                        continue;
                    }
                };
                let clip = match sqlx::query(
                    "SELECT clip_id, streamer_login, source_kind, upload_local_path, local_file_path \
                     FROM twitch_clips_social_media WHERE id = $1 FOR UPDATE",
                )
                .bind(clip_db_id)
                .fetch_optional(transaction.as_mut())
                .await
                {
                    Ok(row) => row,
                    Err(_) => {
                        let _ = transaction.rollback().await;
                        continue;
                    }
                };
                let Some(clip) = clip else {
                    if transaction.commit().await.is_ok() {
                        let _ = unlink_in_directory(&quarantine, &name);
                    }
                    continue;
                };
                if preparation.as_ref().is_some_and(|row| {
                    row.try_get::<String, _>("state")
                        .is_ok_and(|state| matches!(state.as_str(), "materializing" | "rendering"))
                }) {
                    let _ = transaction.rollback().await;
                    continue;
                }
                let expected = match kind {
                    QuarantineKind::Source => {
                        let clip_id = clip.try_get::<String, _>("clip_id").ok();
                        let streamer = clip.try_get::<String, _>("streamer_login").ok();
                        let source_kind = clip.try_get::<String, _>("source_kind").ok();
                        let stored = clip
                            .try_get::<Option<String>, _>("upload_local_path")
                            .ok()
                            .flatten()
                            .filter(|value| !value.trim().is_empty())
                            .or_else(|| {
                                clip.try_get::<Option<String>, _>("local_file_path")
                                    .ok()
                                    .flatten()
                                    .filter(|value| !value.trim().is_empty())
                            });
                        match (clip_id, streamer, source_kind, stored) {
                            (Some(clip_id), Some(streamer), Some(kind), Some(stored))
                                if kind == "manual_upload"
                                    && safe_component(&clip_id)
                                    && safe_component(&streamer) =>
                            {
                                let expected = self
                                    .clips_dir
                                    .join("uploads")
                                    .join(streamer)
                                    .join(format!("{clip_id}.mp4"));
                                (expected == std::path::Path::new(&stored)).then_some(expected)
                            }
                            (_, _, Some(kind), Some(stored)) if kind == "twitch" => {
                                let expected = self.clips_dir.join(format!("{clip_db_id}.mp4"));
                                (expected == std::path::Path::new(&stored)).then_some(expected)
                            }
                            _ => None,
                        }
                    }
                    QuarantineKind::Render(quarantined_fingerprint) => {
                        preparation.as_ref().and_then(|row| {
                            let stored = row
                                .try_get::<Option<String>, _>("render_path")
                                .ok()
                                .flatten()?;
                            let fingerprint = row
                                .try_get::<Option<String>, _>("render_fingerprint")
                                .ok()
                                .flatten()?;
                            if fingerprint.len() != 64
                                || !fingerprint.bytes().all(|byte| byte.is_ascii_hexdigit())
                                || fingerprint != quarantined_fingerprint
                            {
                                return None;
                            }
                            let expected = self
                                .clips_dir
                                .join("rendered")
                                .join(format!("{clip_db_id}-{fingerprint}.mp4"));
                            (expected == std::path::Path::new(&stored)).then_some(expected)
                        })
                    }
                };
                let Some(expected) = expected else {
                    let _ = transaction.rollback().await;
                    continue;
                };
                let Ok(relative) = expected.strip_prefix(&self.clips_dir) else {
                    let _ = transaction.rollback().await;
                    continue;
                };
                let Some((target_directory, target_name)) =
                    open_relative_parent(&self.clips_dir, relative, &directory_options).await
                else {
                    let _ = transaction.rollback().await;
                    continue;
                };
                if rename_noreplace(&quarantine, &name, &target_directory, &target_name).is_err() {
                    let _ = transaction.rollback().await;
                    continue;
                }
                if transaction.commit().await.is_err() {
                    // Commit-Ausgang unklar: die Datei liegt wieder am
                    // erwarteten Ort; sie wird niemals zusätzlich gelöscht.
                    tracing::error!(
                        clip_db_id,
                        code = "retention_restore_commit_uncertain",
                        "Social-Media-Retention: Wiederherstellungs-Commit ist unklar"
                    );
                }
            }
        }
    }

    async fn cleanup_clip(&self, clip_db_id: i64) -> Result<bool, sqlx::Error> {
        let mut transaction = self.pool.begin().await?;
        // Globale Reihenfolge: Preparation → Clip → Queue. Dadurch kann weder
        // ein laufender Render noch ein Provider-Start zwischen Eligibility
        // und Dateilöschung rutschen.
        let preparation = sqlx::query(
            "SELECT state, render_path, render_fingerprint FROM social_media_clip_preparation \
             WHERE clip_db_id = $1 FOR UPDATE",
        )
        .bind(clip_db_id)
        .fetch_optional(transaction.as_mut())
        .await?;
        let clip = sqlx::query(
            "SELECT clip_id, streamer_login, source_kind, upload_local_path, local_file_path, \
                    discarded_at IS NOT NULL AS discarded, \
                    COALESCE(uploaded_tiktok, FALSE) AS uploaded_tiktok, \
                    COALESCE(uploaded_youtube, FALSE) AS uploaded_youtube, \
                    COALESCE(uploaded_instagram, FALSE) AS uploaded_instagram \
             FROM twitch_clips_social_media \
             WHERE id = $1 AND retention_until IS NOT NULL \
               AND retention_until <= CURRENT_TIMESTAMP FOR UPDATE",
        )
        .bind(clip_db_id)
        .fetch_optional(transaction.as_mut())
        .await?;
        let Some(clip) = clip else {
            transaction.rollback().await?;
            return Ok(false);
        };
        if preparation.as_ref().is_some_and(|row| {
            row.try_get::<String, _>("state")
                .is_ok_and(|state| matches!(state.as_str(), "materializing" | "rendering"))
        }) {
            transaction.rollback().await?;
            return Ok(false);
        }
        let queue_rows = sqlx::query(
            "SELECT status, provider_started_at FROM twitch_clips_upload_queue \
             WHERE clip_id = $1 FOR UPDATE",
        )
        .bind(clip_db_id)
        .fetch_all(transaction.as_mut())
        .await?;
        let mut provider_outcome_open = false;
        for row in &queue_rows {
            let status: String = row.try_get("status")?;
            let started = row
                .try_get::<Option<chrono::DateTime<Utc>>, _>("provider_started_at")?
                .is_some();
            provider_outcome_open |=
                matches!(status.as_str(), "processing" | "reconciliation_required")
                    || (started && status != "completed" && status != "failed");
        }
        if provider_outcome_open {
            transaction.rollback().await?;
            return Ok(false);
        }

        let streamer_login: String = clip.try_get("streamer_login")?;
        let discarded: bool = clip.try_get("discarded")?;
        if !discarded {
            let uploaded_tiktok: bool = clip.try_get("uploaded_tiktok")?;
            let uploaded_youtube: bool = clip.try_get("uploaded_youtube")?;
            let uploaded_instagram: bool = clip.try_get("uploaded_instagram")?;
            let active: Vec<String> = sqlx::query_scalar(
                "SELECT DISTINCT platform FROM social_media_platform_auth \
                 WHERE enabled = 1 AND (streamer_login IS NULL OR LOWER(streamer_login) = LOWER($1))",
            )
            .bind(&streamer_login)
            .fetch_all(transaction.as_mut())
            .await?;
            let published = !active.is_empty()
                && active.iter().all(|platform| match platform.as_str() {
                    "tiktok" => uploaded_tiktok,
                    "youtube" => uploaded_youtube,
                    "instagram" => uploaded_instagram,
                    _ => false,
                });
            if !published {
                transaction.rollback().await?;
                return Ok(false);
            }
        }

        let clip_id: String = clip.try_get("clip_id")?;
        let source_kind: String = clip.try_get("source_kind")?;
        let upload_local_path: Option<String> = clip.try_get("upload_local_path")?;
        let local_file_path: Option<String> = clip.try_get("local_file_path")?;
        let stored_source = upload_local_path
            .as_deref()
            .filter(|path| !path.trim().is_empty())
            .or_else(|| {
                local_file_path
                    .as_deref()
                    .filter(|path| !path.trim().is_empty())
            });
        let expected_source = match source_kind.as_str() {
            "manual_upload" if safe_component(&streamer_login) && safe_component(&clip_id) => Some(
                self.clips_dir
                    .join("uploads")
                    .join(&streamer_login)
                    .join(format!("{clip_id}.mp4")),
            ),
            "twitch" => Some(self.clips_dir.join(format!("{clip_db_id}.mp4"))),
            _ => None,
        };
        let mut controlled_files: Vec<(String, PathBuf)> = Vec::new();
        if let Some(stored) = stored_source {
            let Some(expected) = expected_source
                .as_ref()
                .filter(|path| path.as_path() == std::path::Path::new(stored))
            else {
                tracing::warn!(
                    clip_db_id,
                    code = "retention_source_path_rejected",
                    "Social-Media-Retention: gespeicherter Quellpfad wurde abgelehnt"
                );
                transaction.rollback().await?;
                return Ok(false);
            };
            let relative = expected.strip_prefix(&self.clips_dir).unwrap_or(expected);
            controlled_files.push((format!("{clip_db_id}-source"), relative.to_path_buf()));
        }

        if let Some(preparation) = preparation {
            let render_path: Option<String> = preparation.try_get("render_path")?;
            let fingerprint: Option<String> = preparation.try_get("render_fingerprint")?;
            if let Some(stored) = render_path
                .as_deref()
                .filter(|path| !path.trim().is_empty())
            {
                let Some(fingerprint) = fingerprint.as_deref().filter(|value| {
                    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
                }) else {
                    transaction.rollback().await?;
                    return Ok(false);
                };
                let expected = self
                    .clips_dir
                    .join("rendered")
                    .join(format!("{clip_db_id}-{fingerprint}.mp4"));
                if expected != std::path::Path::new(stored) {
                    tracing::warn!(
                        clip_db_id,
                        code = "retention_render_path_rejected",
                        "Social-Media-Retention: Renderpfad wurde abgelehnt"
                    );
                    transaction.rollback().await?;
                    return Ok(false);
                }
                controlled_files.push((
                    format!("{clip_db_id}-render-{fingerprint}"),
                    expected
                        .strip_prefix(&self.clips_dir)
                        .unwrap_or(&expected)
                        .to_path_buf(),
                ));
            }
        }

        let mut staged = Vec::with_capacity(controlled_files.len());
        for (label, relative) in &controlled_files {
            match stage_controlled_file(&self.clips_dir, relative, label).await {
                Ok(Some(file)) => staged.push(file),
                Ok(None) => {}
                Err(()) => {
                    for file in staged {
                        file.restore();
                    }
                    transaction.rollback().await?;
                    return Ok(false);
                }
            }
        }

        let deleted = sqlx::query("DELETE FROM twitch_clips_social_media WHERE id = $1")
            .bind(clip_db_id)
            .execute(transaction.as_mut())
            .await;
        if let Err(error) = deleted {
            let _ = transaction.rollback().await;
            for file in staged {
                file.restore();
            }
            return Err(error);
        }
        if let Err(error) = transaction.commit().await {
            // Commit-Ausgang unklar: nichts löschen und nichts blind
            // zurückverschieben. Die Dateien bleiben im privaten,
            // clip-gekennzeichneten Quarantäne-Verzeichnis rekonstruierbar.
            tracing::error!(
                clip_db_id,
                code = "retention_commit_uncertain",
                "Social-Media-Retention: Commit-Ausgang ist unklar"
            );
            return Err(error);
        }
        for file in staged {
            file.purge();
        }
        Ok(true)
    }

    /// Hintergrund-Loop (30s Initial-Delay + 30min-Intervall). Noch nicht in
    /// tb-bot gespawnt (Wiring = Cutover-Slice).
    pub async fn run(&self) {
        tokio::time::sleep(Duration::from_secs(INITIAL_DELAY_SECS)).await;
        loop {
            self.run_once().await;
            tokio::time::sleep(self.interval).await;
        }
    }
}

fn safe_component(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum QuarantineKind {
    Source,
    Render(String),
}

fn parse_quarantine_name(name: &std::ffi::OsStr) -> Option<(i64, QuarantineKind)> {
    let name = name.to_str()?.strip_suffix(".mp4")?;
    let (clip_db_id, remainder) = name.split_once('-')?;
    let clip_db_id = clip_db_id.parse::<i64>().ok().filter(|id| *id > 0)?;
    let (kind, uuid) = if let Some(uuid) = remainder.strip_prefix("source-") {
        (QuarantineKind::Source, uuid)
    } else {
        let render = remainder.strip_prefix("render-")?;
        let (fingerprint, uuid) = render.split_once('-')?;
        if fingerprint.len() != 64
            || !fingerprint
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return None;
        }
        (QuarantineKind::Render(fingerprint.to_string()), uuid)
    };
    uuid::Uuid::parse_str(uuid).ok()?;
    Some((clip_db_id, kind))
}

#[cfg(target_os = "linux")]
async fn open_relative_parent(
    root: &std::path::Path,
    relative: &std::path::Path,
    options: &tokio::fs::OpenOptions,
) -> Option<(tokio::fs::File, std::ffi::OsString)> {
    use std::os::fd::AsRawFd;
    let mut directory = open_directory_chain(root, options).await.ok()?;
    let mut components = relative.components().peekable();
    while let Some(component) = components.next() {
        let std::path::Component::Normal(name) = component else {
            return None;
        };
        if components.peek().is_none() {
            return Some((directory, name.to_os_string()));
        }
        let child = std::path::PathBuf::from(format!(
            "/proc/self/fd/{}/{}",
            directory.as_raw_fd(),
            name.to_string_lossy()
        ));
        directory = options.open(child).await.ok()?;
    }
    None
}

#[cfg(target_os = "linux")]
struct StagedFile {
    source_directory: tokio::fs::File,
    source_name: std::ffi::OsString,
    quarantine_directory: tokio::fs::File,
    quarantine_name: std::ffi::OsString,
}

#[cfg(target_os = "linux")]
impl StagedFile {
    fn restore(self) {
        if let Err(error) = rename_noreplace(
            &self.quarantine_directory,
            &self.quarantine_name,
            &self.source_directory,
            &self.source_name,
        ) {
            tracing::error!(code = "retention_restore_failed", error = %error, "Social-Media-Retention: Quarantäne-Datei konnte nicht zurückgestellt werden");
        }
    }

    fn purge(self) {
        if let Err(error) = unlink_in_directory(&self.quarantine_directory, &self.quarantine_name) {
            tracing::warn!(code = "retention_quarantine_cleanup_failed", error = %error, "Social-Media-Retention: Quarantäne-Datei blieb erhalten");
        }
    }
}

#[cfg(target_os = "linux")]
fn rename_noreplace(
    from_directory: &tokio::fs::File,
    from_name: &std::ffi::OsStr,
    to_directory: &tokio::fs::File,
    to_name: &std::ffi::OsStr,
) -> std::io::Result<()> {
    use std::ffi::CString;
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStrExt;
    const RENAME_NOREPLACE: u32 = 1;
    unsafe extern "C" {
        fn renameat2(
            olddirfd: i32,
            oldpath: *const std::os::raw::c_char,
            newdirfd: i32,
            newpath: *const std::os::raw::c_char,
            flags: u32,
        ) -> i32;
    }
    let from_name = CString::new(from_name.as_bytes())
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let to_name = CString::new(to_name.as_bytes())
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let result = unsafe {
        renameat2(
            from_directory.as_raw_fd(),
            from_name.as_ptr(),
            to_directory.as_raw_fd(),
            to_name.as_ptr(),
            RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(target_os = "linux")]
fn unlink_in_directory(directory: &tokio::fs::File, name: &std::ffi::OsStr) -> std::io::Result<()> {
    use std::ffi::CString;
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStrExt;
    unsafe extern "C" {
        fn unlinkat(dirfd: i32, pathname: *const std::os::raw::c_char, flags: i32) -> i32;
    }
    let name = CString::new(name.as_bytes())
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let result = unsafe { unlinkat(directory.as_raw_fd(), name.as_ptr(), 0) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(target_os = "linux")]
async fn stage_controlled_file(
    root: &std::path::Path,
    relative: &std::path::Path,
    label: &str,
) -> Result<Option<StagedFile>, ()> {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    const O_DIRECTORY: i32 = 0o200000;
    const O_NOFOLLOW: i32 = 0o400000;
    const O_NONBLOCK: i32 = 0o4000;

    let mut directory_options = tokio::fs::OpenOptions::new();
    directory_options
        .read(true)
        .custom_flags(O_DIRECTORY | O_NOFOLLOW | O_NONBLOCK);
    let root_directory = open_directory_chain(root, &directory_options)
        .await
        .map_err(|_| ())?;
    let quarantine_path = std::path::PathBuf::from(format!(
        "/proc/self/fd/{}/.retention-quarantine",
        root_directory.as_raw_fd()
    ));
    let quarantine_directory = match directory_options.open(&quarantine_path).await {
        Ok(directory) => directory,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            tokio::fs::create_dir(&quarantine_path)
                .await
                .map_err(|_| ())?;
            directory_options
                .open(&quarantine_path)
                .await
                .map_err(|_| ())?
        }
        Err(_) => return Err(()),
    };
    let quarantine_metadata = quarantine_directory.metadata().await.map_err(|_| ())?;
    unsafe extern "C" {
        fn geteuid() -> u32;
    }
    if quarantine_metadata.uid() != unsafe { geteuid() } {
        return Err(());
    }
    quarantine_directory
        .set_permissions(std::fs::Permissions::from_mode(0o700))
        .await
        .map_err(|_| ())?;

    let mut source_directory = root_directory;
    let mut components = relative.components().peekable();
    while let Some(component) = components.next() {
        let std::path::Component::Normal(name) = component else {
            return Err(());
        };
        if components.peek().is_some() {
            let child = std::path::PathBuf::from(format!(
                "/proc/self/fd/{}/{}",
                source_directory.as_raw_fd(),
                name.to_string_lossy()
            ));
            source_directory = match directory_options.open(child).await {
                Ok(directory) => directory,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(_) => return Err(()),
            };
            continue;
        }

        let source_name = name.to_os_string();
        let source_path = std::path::PathBuf::from(format!(
            "/proc/self/fd/{}/{}",
            source_directory.as_raw_fd(),
            name.to_string_lossy()
        ));
        let mut file_options = tokio::fs::OpenOptions::new();
        file_options
            .read(true)
            .custom_flags(O_NOFOLLOW | O_NONBLOCK);
        let source_file = match file_options.open(&source_path).await {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(()),
        };
        let source_metadata = source_file.metadata().await.map_err(|_| ())?;
        if !source_metadata.file_type().is_file()
            || source_metadata.nlink() != 1
            || source_metadata.mode() & 0o777 != 0o640
        {
            return Err(());
        }

        let quarantine_name =
            std::ffi::OsString::from(format!("{label}-{}.mp4", uuid::Uuid::new_v4()));
        rename_noreplace(
            &source_directory,
            &source_name,
            &quarantine_directory,
            &quarantine_name,
        )
        .map_err(|_| ())?;
        let quarantined_path = std::path::PathBuf::from(format!(
            "/proc/self/fd/{}/{}",
            quarantine_directory.as_raw_fd(),
            quarantine_name.to_string_lossy()
        ));
        let quarantined_file = match file_options.open(quarantined_path).await {
            Ok(file) => file,
            Err(_) => {
                let _ = rename_noreplace(
                    &quarantine_directory,
                    &quarantine_name,
                    &source_directory,
                    &source_name,
                );
                return Err(());
            }
        };
        let quarantined_metadata = quarantined_file.metadata().await.map_err(|_| ())?;
        if source_metadata.dev() != quarantined_metadata.dev()
            || source_metadata.ino() != quarantined_metadata.ino()
        {
            let _ = rename_noreplace(
                &quarantine_directory,
                &quarantine_name,
                &source_directory,
                &source_name,
            );
            return Err(());
        }
        return Ok(Some(StagedFile {
            source_directory,
            source_name,
            quarantine_directory,
            quarantine_name,
        }));
    }
    Err(())
}

#[cfg(not(target_os = "linux"))]
struct StagedFile;

#[cfg(not(target_os = "linux"))]
impl StagedFile {
    fn restore(self) {}
    fn purge(self) {}
}

#[cfg(not(target_os = "linux"))]
async fn stage_controlled_file(
    _: &std::path::Path,
    _: &std::path::Path,
    _: &str,
) -> Result<Option<StagedFile>, ()> {
    Err(())
}

#[cfg(target_os = "linux")]
async fn open_directory_chain(
    path: &std::path::Path,
    options: &tokio::fs::OpenOptions,
) -> std::io::Result<tokio::fs::File> {
    use std::os::fd::AsRawFd;
    let mut current = if path.is_absolute() {
        options.open("/").await?
    } else {
        options.open(".").await?
    };
    for component in path.components() {
        let name = match component {
            std::path::Component::RootDir | std::path::Component::CurDir => continue,
            std::path::Component::Normal(name) => name,
            _ => return Err(std::io::Error::from(std::io::ErrorKind::InvalidInput)),
        };
        let child = std::path::PathBuf::from(format!(
            "/proc/self/fd/{}/{}",
            current.as_raw_fd(),
            name.to_string_lossy()
        ));
        current = options.open(child).await?;
    }
    Ok(current)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    #[cfg(unix)]
    fn set_media_mode(path: &std::path::Path) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o640)).unwrap();
    }

    #[cfg(not(unix))]
    fn set_media_mode(_: &std::path::Path) {}

    fn retention_test_dir(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("tb-retention-{label}-{}", uuid::Uuid::new_v4()))
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn dateigrenze_lehnt_symlinks_fifo_hardlinks_und_falschen_modus_ab() {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::fs::symlink;

        let root = retention_test_dir("adversarial-files");
        let outside = retention_test_dir("outside");
        tokio::fs::create_dir_all(&root).await.unwrap();
        tokio::fs::create_dir_all(&outside).await.unwrap();

        let outside_file = outside.join("ziel.mp4");
        tokio::fs::write(&outside_file, b"niemals anfassen")
            .await
            .unwrap();
        set_media_mode(&outside_file);
        symlink(&outside_file, root.join("final-symlink.mp4")).unwrap();
        assert!(stage_controlled_file(
            &root,
            std::path::Path::new("final-symlink.mp4"),
            "1-source"
        )
        .await
        .is_err());
        assert_eq!(
            tokio::fs::read(&outside_file).await.unwrap(),
            b"niemals anfassen"
        );

        symlink(&outside, root.join("parent-symlink")).unwrap();
        assert!(stage_controlled_file(
            &root,
            std::path::Path::new("parent-symlink/ziel.mp4"),
            "2-source"
        )
        .await
        .is_err());
        assert!(outside_file.exists());

        let hardlink = root.join("hardlink.mp4");
        let second_link = root.join("hardlink-zwei.mp4");
        tokio::fs::write(&hardlink, b"doppelt").await.unwrap();
        set_media_mode(&hardlink);
        std::fs::hard_link(&hardlink, &second_link).unwrap();
        assert!(
            stage_controlled_file(&root, std::path::Path::new("hardlink.mp4"), "3-source")
                .await
                .is_err()
        );
        assert!(hardlink.exists() && second_link.exists());

        let wrong_mode = root.join("wrong-mode.mp4");
        tokio::fs::write(&wrong_mode, b"mode").await.unwrap();
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&wrong_mode, std::fs::Permissions::from_mode(0o660))
            .await
            .unwrap();
        assert!(
            stage_controlled_file(&root, std::path::Path::new("wrong-mode.mp4"), "4-source")
                .await
                .is_err()
        );
        assert!(wrong_mode.exists());

        let fifo = root.join("fifo.mp4");
        let fifo_c = CString::new(fifo.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo_c.as_ptr(), 0o640) }, 0);
        let fifo_result = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            stage_controlled_file(&root, std::path::Path::new("fifo.mp4"), "5-source"),
        )
        .await
        .expect("FIFO darf den Retention-Worker nicht blockieren");
        assert!(fifo_result.is_err());
        assert!(fifo.exists());

        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(outside);
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn quarantine_stellt_bei_rollback_wieder_her_und_loescht_nur_nach_commit() {
        let root = retention_test_dir("quarantine-outcomes");
        tokio::fs::create_dir_all(&root).await.unwrap();

        let restored_path = root.join("restore.mp4");
        tokio::fs::write(&restored_path, b"rollback").await.unwrap();
        set_media_mode(&restored_path);
        let staged = stage_controlled_file(&root, std::path::Path::new("restore.mp4"), "11-source")
            .await
            .unwrap()
            .unwrap();
        assert!(!restored_path.exists());
        staged.restore();
        assert_eq!(tokio::fs::read(&restored_path).await.unwrap(), b"rollback");

        let purged_path = root.join("purge.mp4");
        tokio::fs::write(&purged_path, b"commit").await.unwrap();
        set_media_mode(&purged_path);
        let staged = stage_controlled_file(&root, std::path::Path::new("purge.mp4"), "12-source")
            .await
            .unwrap()
            .unwrap();
        staged.purge();
        assert!(!purged_path.exists());
        let mut quarantine = tokio::fs::read_dir(root.join(".retention-quarantine"))
            .await
            .unwrap();
        assert!(quarantine.next_entry().await.unwrap().is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    async fn make_pool(schema: &str) -> Option<PgPool> {
        let dsn = crate::test_support::test_dsn()?;
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
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
            "CREATE TABLE twitch_clips_social_media (id BIGSERIAL PRIMARY KEY, clip_id TEXT NOT NULL, clip_url TEXT NOT NULL, streamer_login TEXT NOT NULL, source_kind TEXT NOT NULL DEFAULT 'twitch', upload_local_path TEXT, local_file_path TEXT, status TEXT DEFAULT 'pending', created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), retention_until TIMESTAMPTZ, discarded_at TIMESTAMPTZ, uploaded_tiktok BOOLEAN DEFAULT FALSE, uploaded_youtube BOOLEAN DEFAULT FALSE, uploaded_instagram BOOLEAN DEFAULT FALSE)",
            "CREATE TABLE social_media_platform_auth (id SERIAL PRIMARY KEY, platform TEXT, streamer_login TEXT, enabled INTEGER DEFAULT 1)",
            "CREATE TABLE social_media_clip_preparation (clip_db_id BIGINT PRIMARY KEY, state TEXT NOT NULL DEFAULT 'pending', render_path TEXT, render_fingerprint TEXT)",
            "CREATE TABLE twitch_clips_upload_queue (id BIGSERIAL PRIMARY KEY, clip_id BIGINT NOT NULL, status TEXT NOT NULL DEFAULT 'pending', provider_started_at TIMESTAMPTZ)",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        Some(pool)
    }

    #[tokio::test]
    async fn cleanup_loescht_nur_fertige_clips() {
        let Some(pool) = make_pool("t_sm_retention_worker").await else {
            return;
        };
        // Aktive Plattform tiktok für 'nani'.
        sqlx::query("INSERT INTO social_media_platform_auth (platform, streamer_login) VALUES ('tiktok', 'nani')").execute(&pool).await.unwrap();

        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let clips_dir = std::env::temp_dir().join(format!("tb_retention_clips_{nonce}"));
        let render_dir = clips_dir.join("rendered");
        tokio::fs::create_dir_all(&render_dir).await.unwrap();
        // Clip A: abgelaufen + verworfen + exakt verankerte Twitch-Quelle.
        let a: i64 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login, discarded_at, retention_until) VALUES ('a', 'https://clips.test/a', 'nani', NOW(), NOW() - INTERVAL '1 day') RETURNING id")
            .fetch_one(&pool).await.unwrap();
        let file_a = clips_dir.join(format!("{a}.mp4"));
        tokio::fs::write(&file_a, b"x").await.unwrap();
        set_media_mode(&file_a);
        sqlx::query("UPDATE twitch_clips_social_media SET upload_local_path = $2 WHERE id = $1")
            .bind(a)
            .bind(file_a.to_string_lossy().as_ref())
            .execute(&pool)
            .await
            .unwrap();
        let fingerprint = "a".repeat(64);
        let render_path = render_dir.join(format!("{a}-{fingerprint}.mp4"));
        tokio::fs::write(&render_path, b"render").await.unwrap();
        set_media_mode(&render_path);
        sqlx::query(
            "INSERT INTO social_media_clip_preparation (clip_db_id, render_path, render_fingerprint) VALUES ($1, $2, $3)",
        )
        .bind(a)
        .bind(render_path.to_string_lossy().as_ref())
        .bind(&fingerprint)
        .execute(&pool)
        .await
        .unwrap();

        // Clip B: abgelaufen, NICHT verworfen, tiktok aktiv aber nicht hochgeladen → behalten.
        let b: i64 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login, retention_until) VALUES ('b', 'https://clips.test/b', 'nani', NOW() - INTERVAL '1 day') RETURNING id").fetch_one(&pool).await.unwrap();

        // Clip C: abgelaufen, NICHT verworfen, tiktok hochgeladen → voll veröffentlicht → gelöscht.
        let _c: i64 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login, retention_until, uploaded_tiktok) VALUES ('c', 'https://clips.test/c', 'nani', NOW() - INTERVAL '1 day', TRUE) RETURNING id").fetch_one(&pool).await.unwrap();

        // Clip D: in der Zukunft → gar kein Kandidat.
        let d: i64 = sqlx::query_scalar("INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login, retention_until) VALUES ('d', 'https://clips.test/d', 'nani', NOW() + INTERVAL '5 days') RETURNING id").fetch_one(&pool).await.unwrap();

        RetentionWorker::new(pool.clone())
            .with_clips_dir(&clips_dir)
            .run_once()
            .await;

        let remaining: Vec<i64> =
            sqlx::query_scalar("SELECT id FROM twitch_clips_social_media ORDER BY id")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(remaining, vec![b, d]); // A + C gelöscht, B + D bleiben
        assert!(!file_a.exists()); // Datei von A entfernt
        assert!(!render_path.exists()); // abgeleitetes Ready-MP4 ebenfalls entfernt
        let _ = std::fs::remove_dir_all(clips_dir);
    }

    #[tokio::test]
    async fn cleanup_bewahrt_fremdpfad_und_offenen_provider_versuch() {
        let Some(pool) = make_pool("t_sm_retention_worker_guard").await else {
            return;
        };
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let clips_dir = std::env::temp_dir().join(format!("tb_retention_guard_{nonce}"));
        tokio::fs::create_dir_all(&clips_dir).await.unwrap();
        let outside = std::env::temp_dir().join(format!("tb_retention_outside_{nonce}.mp4"));
        tokio::fs::write(&outside, "niemals löschen".as_bytes())
            .await
            .unwrap();

        let foreign: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media \
             (clip_id, clip_url, streamer_login, upload_local_path, discarded_at, retention_until) \
             VALUES ('foreign', 'https://clips.test/foreign', 'nani', $1, NOW(), NOW() - INTERVAL '1 day') RETURNING id",
        )
        .bind(outside.to_string_lossy().as_ref())
        .fetch_one(&pool)
        .await
        .unwrap();
        let running: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media \
             (clip_id, clip_url, streamer_login, discarded_at, retention_until) \
             VALUES ('running', 'https://clips.test/running', 'nani', NOW(), NOW() - INTERVAL '1 day') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let running_source = clips_dir.join(format!("{running}.mp4"));
        tokio::fs::write(&running_source, "läuft".as_bytes())
            .await
            .unwrap();
        sqlx::query("UPDATE twitch_clips_social_media SET local_file_path = $2 WHERE id = $1")
            .bind(running)
            .bind(running_source.to_string_lossy().as_ref())
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO twitch_clips_upload_queue \
             (clip_id, status, provider_started_at) VALUES ($1, 'reconciliation_required', NOW())",
        )
        .bind(running)
        .execute(&pool)
        .await
        .unwrap();

        RetentionWorker::new(pool.clone())
            .with_clips_dir(&clips_dir)
            .run_once()
            .await;
        assert!(outside.exists(), "DB-Fremdpfade dürfen nie gelöscht werden");
        assert!(
            running_source.exists(),
            "Abgleichanker und Quelle bleiben erhalten"
        );
        let kept: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM twitch_clips_social_media WHERE id = ANY($1)")
                .bind(vec![foreign, running])
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(kept, 2);
        let _ = std::fs::remove_file(outside);
        let _ = std::fs::remove_dir_all(clips_dir);
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn db_rollback_stellt_quelle_wieder_her() {
        let Some(pool) = make_pool("t_sm_retention_worker_db_rollback").await else {
            return;
        };
        sqlx::query(
            "CREATE TABLE retention_delete_blocker (clip_id BIGINT PRIMARY KEY \
             REFERENCES twitch_clips_social_media(id) ON DELETE RESTRICT)",
        )
        .execute(&pool)
        .await
        .unwrap();
        let clips_dir = retention_test_dir("db-rollback");
        tokio::fs::create_dir_all(&clips_dir).await.unwrap();
        let clip_db_id: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media \
             (clip_id, clip_url, streamer_login, discarded_at, retention_until) \
             VALUES ('rollback', 'https://clips.test/rollback', 'nani', NOW(), \
                     NOW() - INTERVAL '1 day') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let source = clips_dir.join(format!("{clip_db_id}.mp4"));
        tokio::fs::write(&source, b"erhalten").await.unwrap();
        set_media_mode(&source);
        sqlx::query("UPDATE twitch_clips_social_media SET local_file_path = $2 WHERE id = $1")
            .bind(clip_db_id)
            .bind(source.to_string_lossy().as_ref())
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO retention_delete_blocker (clip_id) VALUES ($1)")
            .bind(clip_db_id)
            .execute(&pool)
            .await
            .unwrap();

        assert!(RetentionWorker::new(pool.clone())
            .with_clips_dir(&clips_dir)
            .cleanup_clip(clip_db_id)
            .await
            .is_err());
        assert_eq!(tokio::fs::read(&source).await.unwrap(), b"erhalten");
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM twitch_clips_social_media WHERE id = $1")
                .bind(clip_db_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count, 1);
        let mut quarantine = tokio::fs::read_dir(clips_dir.join(".retention-quarantine"))
            .await
            .unwrap();
        assert!(quarantine.next_entry().await.unwrap().is_none());
        let _ = std::fs::remove_dir_all(clips_dir);
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn alter_quarantaene_render_fingerprint_wird_nie_als_neuer_restauriert() {
        use std::os::unix::fs::PermissionsExt;

        let Some(pool) = make_pool("t_sm_retention_worker_fingerprint_restore").await else {
            return;
        };
        let clips_dir = retention_test_dir("fingerprint-restore");
        let rendered = clips_dir.join("rendered");
        let quarantine = clips_dir.join(".retention-quarantine");
        tokio::fs::create_dir_all(&rendered).await.unwrap();
        tokio::fs::create_dir_all(&quarantine).await.unwrap();
        tokio::fs::set_permissions(&quarantine, std::fs::Permissions::from_mode(0o700))
            .await
            .unwrap();
        let clip_db_id: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media (clip_id, clip_url, streamer_login) \
             VALUES ('fingerprint', 'https://clips.test/fingerprint', 'nani') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let old_fingerprint = "a".repeat(64);
        let new_fingerprint = "b".repeat(64);
        let new_render = rendered.join(format!("{clip_db_id}-{new_fingerprint}.mp4"));
        sqlx::query(
            "INSERT INTO social_media_clip_preparation \
             (clip_db_id, state, render_path, render_fingerprint) \
             VALUES ($1, 'preview_ready', $2, $3)",
        )
        .bind(clip_db_id)
        .bind(new_render.to_string_lossy().as_ref())
        .bind(&new_fingerprint)
        .execute(&pool)
        .await
        .unwrap();
        let quarantine_name = format!(
            "{clip_db_id}-render-{old_fingerprint}-{}.mp4",
            uuid::Uuid::new_v4()
        );
        let old_quarantined = quarantine.join(&quarantine_name);
        tokio::fs::write(&old_quarantined, b"altes-render")
            .await
            .unwrap();
        set_media_mode(&old_quarantined);
        let worker = RetentionWorker::new(pool.clone()).with_clips_dir(&clips_dir);
        worker.reconcile_quarantine().await;
        assert!(old_quarantined.exists());
        assert!(!new_render.exists());

        let old_render = rendered.join(format!("{clip_db_id}-{old_fingerprint}.mp4"));
        sqlx::query(
            "UPDATE social_media_clip_preparation \
             SET render_path = $2, render_fingerprint = $3 WHERE clip_db_id = $1",
        )
        .bind(clip_db_id)
        .bind(old_render.to_string_lossy().as_ref())
        .bind(&old_fingerprint)
        .execute(&pool)
        .await
        .unwrap();
        worker.reconcile_quarantine().await;
        assert!(!old_quarantined.exists());
        assert_eq!(tokio::fs::read(&old_render).await.unwrap(), b"altes-render");
        let _ = std::fs::remove_dir_all(clips_dir);
    }
}
