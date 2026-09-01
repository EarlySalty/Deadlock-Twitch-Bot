//! Plattformfreie Clip-Aufbereitung für Vorschau und spätere Uploads.
//!
//! Pro Clip entsteht genau eine sichere, höchstens 60 Sekunden lange
//! 1080x1920-MP4. Quelle und Layout gehen in Fingerprints ein; damit kann der
//! Upload-Pfad exakt dasselbe geprüfte Artefakt wiederverwenden.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use sha2::{Digest, Sha256};
use sqlx::PgPool;

use crate::layout::{get_clip_effective_layout_checked, EffectiveLayoutError, StreamerLayout};
use crate::video_processor::{VideoProcessor, VideoProcessorError};

pub const PREPARATION_MAX_DURATION_SECS: i64 = 60;
pub const DEFAULT_CLIPS_DIR: &str = "data/clips";
const WORKER_INTERVAL_SECS: u64 = 30;
const RENDERER_VERSION: &str = "layout-compose-v1-1080x1920-60s";
const STALE_LEASE_MINUTES: i64 = 30;
const MAX_MATERIALIZED_SOURCE_BYTES: u64 = 512 * 1024 * 1024;

struct SecureDirectory {
    handle: tokio::fs::File,
    #[cfg(not(target_os = "linux"))]
    logical_path: PathBuf,
}

impl SecureDirectory {
    fn child_path(&self, name: &std::ffi::OsStr) -> PathBuf {
        #[cfg(target_os = "linux")]
        {
            use std::os::fd::AsRawFd;
            PathBuf::from(format!("/proc/self/fd/{}", self.handle.as_raw_fd())).join(name)
        }
        #[cfg(not(target_os = "linux"))]
        {
            self.logical_path.join(name)
        }
    }
}

struct OpenedSource {
    _handle: tokio::fs::File,
    tool_path: PathBuf,
    logical_path: PathBuf,
}

struct WorkArtifact {
    directory: SecureDirectory,
    name: std::ffi::OsString,
    handle: tokio::fs::File,
    device: u64,
    inode: u64,
}

fn discard_work_artifact(artifact: &WorkArtifact) {
    let Ok(current) = open_file_from_directory(&artifact.directory, &artifact.name) else {
        return;
    };
    use std::os::unix::fs::MetadataExt;
    if current
        .metadata()
        .is_ok_and(|metadata| metadata.dev() == artifact.device && metadata.ino() == artifact.inode)
    {
        let _ = unlink_from_directory(&artifact.directory, &artifact.name);
    }
}

async fn open_or_create_secure_directory(
    path: &Path,
    final_mode: u32,
) -> Result<SecureDirectory, std::io::Error> {
    #[cfg(target_os = "linux")]
    {
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        const O_DIRECTORY: i32 = 0o200000;
        const O_NOFOLLOW: i32 = 0o400000;
        const O_NONBLOCK: i32 = 0o4000;
        let mut options = tokio::fs::OpenOptions::new();
        options
            .read(true)
            .custom_flags(O_DIRECTORY | O_NOFOLLOW | O_NONBLOCK);
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
            let child = PathBuf::from(format!(
                "/proc/self/fd/{}/{}",
                current.as_raw_fd(),
                name.to_string_lossy()
            ));
            current = match options.open(&child).await {
                Ok(directory) => directory,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    tokio::fs::create_dir(&child).await?;
                    options.open(&child).await?
                }
                Err(error) => return Err(error),
            };
        }
        let metadata = current.metadata().await?;
        unsafe extern "C" {
            fn geteuid() -> u32;
        }
        if metadata.uid() != unsafe { geteuid() } {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "Unsicherer Eigentümer des Medienverzeichnisses",
            ));
        }
        current
            .set_permissions(std::fs::Permissions::from_mode(final_mode))
            .await?;
        Ok(SecureDirectory { handle: current })
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (path, final_mode);
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "Sichere Medienverzeichnisse werden auf diesem System nicht unterstützt",
        ))
    }
}

#[cfg(target_os = "linux")]
fn publish_artifact_noreplace(
    artifact: &WorkArtifact,
    destination: &SecureDirectory,
    destination_name: &std::ffi::OsStr,
) -> std::io::Result<()> {
    use std::ffi::CString;
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStrExt;
    const AT_FDCWD: i32 = -100;
    const AT_SYMLINK_FOLLOW: i32 = 0x400;
    unsafe extern "C" {
        fn linkat(
            olddirfd: i32,
            oldpath: *const std::os::raw::c_char,
            newdirfd: i32,
            newpath: *const std::os::raw::c_char,
            flags: i32,
        ) -> i32;
    }
    if !artifact_name_matches(artifact)? {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Artefaktname verweist nicht mehr auf den geprüften Inode",
        ));
    }
    let source_path = CString::new(format!(
        "/proc/{}/fd/{}",
        std::process::id(),
        artifact.handle.as_raw_fd()
    ))
    .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let destination_c_name = CString::new(destination_name.as_bytes())
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let result = unsafe {
        linkat(
            AT_FDCWD,
            source_path.as_ptr(),
            destination.handle.as_raw_fd(),
            destination_c_name.as_ptr(),
            AT_SYMLINK_FOLLOW,
        )
    };
    if result != 0 {
        return Err(std::io::Error::last_os_error());
    }
    if !artifact_name_matches(artifact)? {
        let _ = unlink_from_directory(destination, destination_name);
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Artefaktname wurde während der Veröffentlichung ausgetauscht",
        ));
    }
    if let Err(error) = unlink_from_directory(&artifact.directory, &artifact.name) {
        let _ = unlink_from_directory(destination, destination_name);
        return Err(error);
    }
    let mut metadata: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstat(artifact.handle.as_raw_fd(), &mut metadata) } != 0 {
        let _ = unlink_from_directory(destination, destination_name);
        return Err(std::io::Error::last_os_error());
    }
    if metadata.st_dev != artifact.device
        || metadata.st_ino != artifact.inode
        || metadata.st_nlink != 1
    {
        let _ = unlink_from_directory(destination, destination_name);
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Artefakt-Inode wurde während der Veröffentlichung verändert",
        ));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn artifact_name_matches(artifact: &WorkArtifact) -> std::io::Result<bool> {
    use std::os::unix::fs::MetadataExt;
    match open_file_from_directory(&artifact.directory, &artifact.name) {
        Ok(file) => {
            let metadata = file.metadata()?;
            Ok(metadata.file_type().is_file()
                && metadata.dev() == artifact.device
                && metadata.ino() == artifact.inode)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

#[cfg(not(target_os = "linux"))]
fn publish_artifact_noreplace(
    _: &WorkArtifact,
    _: &SecureDirectory,
    _: &std::ffi::OsStr,
) -> std::io::Result<()> {
    Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
}

#[cfg(target_os = "linux")]
fn open_file_from_directory(
    directory: &SecureDirectory,
    name: &std::ffi::OsStr,
) -> std::io::Result<std::fs::File> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    const O_RDONLY: i32 = 0;
    const O_CLOEXEC: i32 = 0o2000000;
    const O_NOFOLLOW: i32 = 0o400000;
    const O_NONBLOCK: i32 = 0o4000;
    let name = CString::new(name.as_bytes())
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let fd = unsafe {
        libc::openat(
            directory.handle.as_raw_fd(),
            name.as_ptr(),
            O_RDONLY | O_CLOEXEC | O_NOFOLLOW | O_NONBLOCK,
            0,
        )
    };
    if fd < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(unsafe { std::fs::File::from_raw_fd(fd) })
    }
}

#[cfg(target_os = "linux")]
fn unlink_from_directory(
    directory: &SecureDirectory,
    name: &std::ffi::OsStr,
) -> std::io::Result<()> {
    use std::ffi::CString;
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStrExt;
    unsafe extern "C" {
        fn unlinkat(dirfd: i32, pathname: *const std::os::raw::c_char, flags: i32) -> i32;
    }
    let name = CString::new(name.as_bytes())
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let result = unsafe { unlinkat(directory.handle.as_raw_fd(), name.as_ptr(), 0) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

async fn open_controlled_source(
    clips_dir: &Path,
    logical_path: &Path,
) -> Result<Option<OpenedSource>, std::io::Error> {
    #[cfg(target_os = "linux")]
    {
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::MetadataExt;
        const O_DIRECTORY: i32 = 0o200000;
        const O_NOFOLLOW: i32 = 0o400000;
        const O_NONBLOCK: i32 = 0o4000;
        let relative = logical_path
            .strip_prefix(clips_dir)
            .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
        let root = open_or_create_secure_directory(clips_dir, 0o2750).await?;
        let mut directory = root.handle;
        let mut components = relative.components().peekable();
        let mut directory_options = tokio::fs::OpenOptions::new();
        directory_options
            .read(true)
            .custom_flags(O_DIRECTORY | O_NOFOLLOW | O_NONBLOCK);
        while let Some(component) = components.next() {
            let std::path::Component::Normal(name) = component else {
                return Err(std::io::Error::from(std::io::ErrorKind::InvalidInput));
            };
            let child = PathBuf::from(format!(
                "/proc/self/fd/{}/{}",
                directory.as_raw_fd(),
                name.to_string_lossy()
            ));
            if components.peek().is_some() {
                directory = match directory_options.open(child).await {
                    Ok(directory) => directory,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                    Err(error) => return Err(error),
                };
                continue;
            }
            let mut file_options = tokio::fs::OpenOptions::new();
            file_options
                .read(true)
                .custom_flags(O_NOFOLLOW | O_NONBLOCK);
            let file = match file_options.open(child).await {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(error),
            };
            let metadata = file.metadata().await?;
            if !metadata.file_type().is_file() || metadata.len() == 0 || metadata.nlink() != 1 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Unsichere Quelldatei",
                ));
            }
            let tool_path = PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd()));
            return Ok(Some(OpenedSource {
                _handle: file,
                tool_path,
                logical_path: logical_path.to_path_buf(),
            }));
        }
        Ok(None)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (clips_dir, logical_path);
        Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
    }
}

async fn validate_work_artifact(
    directory: SecureDirectory,
    name: std::ffi::OsString,
) -> Result<WorkArtifact, PreparationError> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        const O_NOFOLLOW: i32 = 0o400000;
        const O_NONBLOCK: i32 = 0o4000;
        let path = directory.child_path(&name);
        let mut options = tokio::fs::OpenOptions::new();
        options.read(true).custom_flags(O_NOFOLLOW | O_NONBLOCK);
        let file = match options.open(&path).await {
            Ok(file) => file,
            Err(error) => {
                let _ = unlink_from_directory(&directory, &name);
                return Err(error.into());
            }
        };
        let metadata = match file.metadata().await {
            Ok(metadata) => metadata,
            Err(error) => {
                let _ = unlink_from_directory(&directory, &name);
                return Err(error.into());
            }
        };
        if !metadata.file_type().is_file() || metadata.len() == 0 || metadata.nlink() != 1 {
            let _ = unlink_from_directory(&directory, &name);
            return Err(PreparationError::Renderer(
                "Medienwerkzeug hat kein sicheres Artefakt erzeugt".to_string(),
            ));
        }
        if let Err(error) = file
            .set_permissions(std::fs::Permissions::from_mode(0o640))
            .await
        {
            let _ = unlink_from_directory(&directory, &name);
            return Err(error.into());
        }
        let metadata = match file.metadata().await {
            Ok(metadata) => metadata,
            Err(error) => {
                let _ = unlink_from_directory(&directory, &name);
                return Err(error.into());
            }
        };
        Ok(WorkArtifact {
            directory,
            name,
            handle: file,
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (directory, name);
        Err(PreparationError::Io(std::io::Error::from(
            std::io::ErrorKind::Unsupported,
        )))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkArtifactState {
    Materializing,
    Rendering,
}

fn parse_work_artifact_name(name: &str) -> Option<(i64, String, WorkArtifactState)> {
    if let Some(prefix) = name.strip_suffix("-source.tmp.mp4") {
        let (clip_id, lease) = prefix.split_once('-')?;
        let clip_id = clip_id.parse().ok()?;
        let lease = uuid::Uuid::parse_str(lease).ok()?.to_string();
        return (clip_id > 0).then_some((clip_id, lease, WorkArtifactState::Materializing));
    }
    let prefix = name.strip_suffix("-render.tmp.mp4")?;
    let (clip_id, rest) = prefix.split_once('-')?;
    let clip_id: i64 = clip_id.parse().ok()?;
    if clip_id <= 0 || rest.len() != 36 + 1 + 64 {
        return None;
    }
    let lease = uuid::Uuid::parse_str(&rest[..36]).ok()?.to_string();
    let fingerprint = rest.strip_prefix(&format!("{}-", &rest[..36]))?;
    if fingerprint.len() != 64
        || !fingerprint
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return None;
    }
    Some((clip_id, lease, WorkArtifactState::Rendering))
}

async fn existing_work_artifact_is_safe(
    directory: &SecureDirectory,
    name: &std::ffi::OsStr,
) -> Result<bool, std::io::Error> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        const O_NOFOLLOW: i32 = 0o400000;
        const O_NONBLOCK: i32 = 0o4000;
        let mut options = tokio::fs::OpenOptions::new();
        options.read(true).custom_flags(O_NOFOLLOW | O_NONBLOCK);
        let file = match options.open(directory.child_path(name)).await {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error),
        };
        let metadata = file.metadata().await?;
        unsafe extern "C" {
            fn geteuid() -> u32;
        }
        Ok(metadata.file_type().is_file()
            && metadata.uid() == unsafe { geteuid() }
            && metadata.nlink() == 1
            && metadata.permissions().mode() & 0o777 == 0o640)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (directory, name);
        Ok(false)
    }
}

async fn open_controlled_render(
    clips_dir: &Path,
    destination: &Path,
) -> Result<Option<OpenedSource>, std::io::Error> {
    let _rendered = open_or_create_secure_directory(&clips_dir.join("rendered"), 0o2750).await?;
    let Some(source) = open_controlled_source(clips_dir, destination).await? else {
        return Ok(None);
    };
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        unsafe extern "C" {
            fn geteuid() -> u32;
        }
        let metadata = source._handle.metadata().await?;
        if metadata.uid() != unsafe { geteuid() } || metadata.permissions().mode() & 0o777 != 0o640
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "Unsicheres Render-Artefakt",
            ));
        }
    }
    Ok(Some(source))
}

#[derive(Debug, thiserror::Error)]
pub enum PreparationError {
    #[error("Clip {0} wurde nicht gefunden")]
    ClipNotFound(i64),
    #[error("Clip {0} wurde verworfen")]
    Discarded(i64),
    #[error("Clip {0} wird bereits aufbereitet")]
    Busy(i64),
    #[error("Keine nutzbare Quelldatei für Clip {0}")]
    SourceMissing(i64),
    #[error("Ungültige Twitch-Clip-URL")]
    InvalidSourceUrl,
    #[error("Download fehlgeschlagen: {0}")]
    Download(String),
    #[error("Automatischer Twitch-Download benötigt einen isolierten Netzwerkpfad")]
    DownloadIsolationRequired,
    #[error(transparent)]
    Render(#[from] VideoProcessorError),
    #[error(transparent)]
    Layout(#[from] EffectiveLayoutError),
    #[error("Renderer fehlgeschlagen: {0}")]
    Renderer(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

impl PreparationError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::ClipNotFound(_) => "clip_not_found",
            Self::Discarded(_) => "clip_discarded",
            Self::Busy(_) => "preparation_busy",
            Self::SourceMissing(_) => "source_missing",
            Self::InvalidSourceUrl => "invalid_source_url",
            Self::Download(_) => "download_failed",
            Self::DownloadIsolationRequired => "download_isolation_required",
            Self::Render(_) | Self::Renderer(_) => "render_failed",
            Self::Layout(EffectiveLayoutError::Db(_)) => "database_failed",
            Self::Layout(_) => "layout_invalid",
            Self::Io(_) => "io_failed",
            Self::Db(_) => "database_failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipPreparationRecord {
    pub clip_db_id: i64,
    pub state: String,
    pub source_fingerprint: Option<String>,
    pub render_fingerprint: Option<String>,
    pub render_path: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub requested_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub updated_at: String,
}

impl ClipPreparationRecord {
    pub fn source_ready(&self) -> bool {
        self.source_fingerprint.is_some()
    }

    pub fn preview_ready(&self) -> bool {
        self.state == "preview_ready"
            && self
                .render_fingerprint
                .as_deref()
                .is_some_and(|value| !value.is_empty())
            && self
                .render_path
                .as_deref()
                .is_some_and(stored_file_is_regular_nonempty)
    }

    pub fn ready_path(&self) -> Option<&str> {
        if self.preview_ready() {
            self.render_path.as_deref()
        } else {
            None
        }
    }
}

#[async_trait]
pub trait ClipSourceDownloader: Send + Sync {
    async fn download(&self, clip_url: &str, destination: &Path) -> Result<(), PreparationError>;
}

#[derive(Debug, Clone, Copy)]
struct UnconfiguredDownloader;

#[async_trait]
impl ClipSourceDownloader for UnconfiguredDownloader {
    async fn download(&self, _: &str, _: &Path) -> Result<(), PreparationError> {
        Err(PreparationError::Download(
            "Clip-Downloader ist nicht konfiguriert".to_string(),
        ))
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct IsolatedClipDownloader;

impl IsolatedClipDownloader {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ClipSourceDownloader for IsolatedClipDownloader {
    async fn download(&self, clip_url: &str, destination: &Path) -> Result<(), PreparationError> {
        validate_twitch_clip_url(clip_url)?;
        #[cfg(not(target_os = "linux"))]
        return Err(PreparationError::Download(
            "Medien-Sandbox wird auf diesem System nicht unterstützt".to_string(),
        ));
        #[cfg(target_os = "linux")]
        let output_len = {
            crate::downloader_ipc::download_to_path(clip_url, destination)
                .await
                .map_err(|error| match error {
                    crate::downloader_ipc::DownloaderIpcError::Unavailable => {
                        PreparationError::DownloadIsolationRequired
                    }
                    _ => PreparationError::Download(
                        "Isolierter Downloader hat den Auftrag nicht abgeschlossen".to_string(),
                    ),
                })?
        };
        if output_len > MAX_MATERIALIZED_SOURCE_BYTES {
            return Err(PreparationError::Download(
                "Clip-Quelle überschreitet das Größenlimit".to_string(),
            ));
        }
        Ok(())
    }
}

#[async_trait]
pub trait ClipRenderer: Send + Sync {
    async fn compose_and_trim(
        &self,
        source: &Path,
        destination: &Path,
        layout: &StreamerLayout,
        max_duration_secs: i64,
    ) -> Result<(), PreparationError>;
}

#[derive(Debug, Clone, Copy)]
struct UnconfiguredRenderer;

#[async_trait]
impl ClipRenderer for UnconfiguredRenderer {
    async fn compose_and_trim(
        &self,
        _: &Path,
        _: &Path,
        _: &StreamerLayout,
        _: i64,
    ) -> Result<(), PreparationError> {
        Err(PreparationError::Renderer(
            "Clip-Renderer ist nicht konfiguriert".to_string(),
        ))
    }
}

#[derive(Debug, Clone)]
pub struct VideoClipRenderer {
    processor: VideoProcessor,
}

impl VideoClipRenderer {
    pub fn new(processor: VideoProcessor) -> Self {
        Self { processor }
    }
}

#[async_trait]
impl ClipRenderer for VideoClipRenderer {
    async fn compose_and_trim(
        &self,
        source: &Path,
        destination: &Path,
        layout: &StreamerLayout,
        max_duration_secs: i64,
    ) -> Result<(), PreparationError> {
        self.processor
            .compose_and_trim(
                &source.to_string_lossy(),
                &destination.to_string_lossy(),
                layout,
                max_duration_secs,
            )
            .await?;
        Ok(())
    }
}

struct SourceMaterializationRequest<'a> {
    clip_db_id: i64,
    clip_url: &'a str,
    source_kind: &'a str,
    clip_id: &'a str,
    streamer_login: &'a str,
    local_file_path: Option<&'a str>,
    upload_local_path: Option<&'a str>,
    lease_token: &'a str,
}

#[derive(Clone)]
pub struct ClipPreparationService {
    pool: PgPool,
    downloader: Arc<dyn ClipSourceDownloader>,
    renderer: Arc<dyn ClipRenderer>,
    clips_dir: PathBuf,
}

impl ClipPreparationService {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            // Kein impliziter PATH-Fallback. Produktive Verarbeitung muss das
            // gebündelte, explizit aufgelöste Binary injizieren; Dashboard-
            // Aufrufer nutzen ausschließlich ensure/request/get.
            downloader: Arc::new(UnconfiguredDownloader),
            renderer: Arc::new(UnconfiguredRenderer),
            clips_dir: PathBuf::from(DEFAULT_CLIPS_DIR),
        }
    }

    pub fn with_downloader(mut self, downloader: Arc<dyn ClipSourceDownloader>) -> Self {
        self.downloader = downloader;
        self
    }

    pub fn with_renderer(mut self, renderer: Arc<dyn ClipRenderer>) -> Self {
        self.renderer = renderer;
        self
    }

    pub fn with_clips_dir(mut self, clips_dir: impl Into<PathBuf>) -> Self {
        self.clips_dir = clips_dir.into();
        self
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Legt den Auftrag idempotent an. Neue Clips bekommen dieselbe Zeile
    /// zusätzlich durch den Migrationstrigger.
    pub async fn ensure_pending(&self, clip_db_id: i64) -> Result<(), PreparationError> {
        let inserted = sqlx::query(
            "INSERT INTO social_media_clip_preparation (clip_db_id) \
             SELECT id FROM twitch_clips_social_media WHERE id = $1 \
             ON CONFLICT (clip_db_id) DO NOTHING",
        )
        .bind(clip_db_id)
        .execute(&self.pool)
        .await?;
        if inserted.rows_affected() == 0 && !clip_exists(&self.pool, clip_db_id).await? {
            return Err(PreparationError::ClipNotFound(clip_db_id));
        }
        Ok(())
    }

    /// Fordert eine neue Prüfung an. Fingerprints bleiben erhalten, damit ein
    /// unverändertes fertiges Artefakt ohne FFmpeg wieder bereitsteht.
    pub async fn request(
        &self,
        clip_db_id: i64,
    ) -> Result<ClipPreparationRecord, PreparationError> {
        self.ensure_pending(clip_db_id).await?;
        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            "SELECT clip_db_id FROM social_media_clip_preparation \
             WHERE clip_db_id = $1 FOR UPDATE",
        )
        .bind(clip_db_id)
        .fetch_one(transaction.as_mut())
        .await?;
        sqlx::query("SELECT id FROM twitch_clips_social_media WHERE id = $1 FOR UPDATE")
            .bind(clip_db_id)
            .fetch_one(transaction.as_mut())
            .await?;
        let provider_active: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM twitch_clips_upload_queue \
             WHERE clip_id = $1 AND provider_started_at IS NOT NULL \
               AND status IN ('processing', 'reconciliation_required') FOR UPDATE)",
        )
        .bind(clip_db_id)
        .fetch_one(transaction.as_mut())
        .await?;
        if provider_active {
            transaction.rollback().await?;
            return Err(PreparationError::Busy(clip_db_id));
        }
        let changed = sqlx::query(
            "UPDATE social_media_clip_preparation SET state = 'pending', \
             lease_token = NULL, \
             requested_at = CURRENT_TIMESTAMP, error_code = NULL, error_message = NULL, \
             completed_at = NULL, updated_at = CURRENT_TIMESTAMP WHERE clip_db_id = $1 \
             AND (state NOT IN ('materializing', 'rendering') \
                  OR updated_at < CURRENT_TIMESTAMP - INTERVAL '30 minutes')",
        )
        .bind(clip_db_id)
        .execute(transaction.as_mut())
        .await?
        .rows_affected();
        if changed == 0 {
            transaction.rollback().await?;
            return Err(PreparationError::Busy(clip_db_id));
        }
        transaction.commit().await?;
        self.get(clip_db_id)
            .await?
            .ok_or(PreparationError::ClipNotFound(clip_db_id))
    }

    pub async fn get(
        &self,
        clip_db_id: i64,
    ) -> Result<Option<ClipPreparationRecord>, PreparationError> {
        let row: Option<PreparationRow> = sqlx::query_as(
            "SELECT clip_db_id, state, source_fingerprint, render_fingerprint, render_path, \
                    error_code, error_message, requested_at::text, started_at::text, \
                    completed_at::text, updated_at::text \
               FROM social_media_clip_preparation WHERE clip_db_id = $1",
        )
        .bind(clip_db_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(Into::into))
    }

    /// Materialisiert und rendert einen Clip. Dieselben Quell-/Layoutdaten
    /// verwenden ein vorhandenes Artefakt wieder.
    pub async fn prepare(
        &self,
        clip_db_id: i64,
    ) -> Result<ClipPreparationRecord, PreparationError> {
        self.ensure_pending(clip_db_id).await?;
        let lease_token = uuid::Uuid::new_v4().to_string();
        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            "SELECT clip_db_id FROM social_media_clip_preparation \
             WHERE clip_db_id = $1 FOR UPDATE",
        )
        .bind(clip_db_id)
        .fetch_one(transaction.as_mut())
        .await?;
        sqlx::query("SELECT id FROM twitch_clips_social_media WHERE id = $1 FOR UPDATE")
            .bind(clip_db_id)
            .fetch_one(transaction.as_mut())
            .await?;
        let provider_active: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM twitch_clips_upload_queue \
             WHERE clip_id = $1 AND provider_started_at IS NOT NULL \
               AND status IN ('processing', 'reconciliation_required') FOR UPDATE)",
        )
        .bind(clip_db_id)
        .fetch_one(transaction.as_mut())
        .await?;
        if provider_active {
            transaction.rollback().await?;
            return Err(PreparationError::Busy(clip_db_id));
        }
        let claimed = sqlx::query(
            "UPDATE social_media_clip_preparation \
                SET state = 'materializing', started_at = CURRENT_TIMESTAMP, \
                    lease_token = $2, \
                    completed_at = NULL, error_code = NULL, error_message = NULL, \
                    updated_at = CURRENT_TIMESTAMP \
              WHERE clip_db_id = $1 AND (state NOT IN ('materializing', 'rendering') \
                    OR updated_at < CURRENT_TIMESTAMP - INTERVAL '30 minutes')",
        )
        .bind(clip_db_id)
        .bind(&lease_token)
        .execute(transaction.as_mut())
        .await?
        .rows_affected();
        if claimed == 0 {
            transaction.rollback().await?;
            return Err(PreparationError::Busy(clip_db_id));
        }
        transaction.commit().await?;

        match self.prepare_claimed(clip_db_id, &lease_token).await {
            Ok(record) => Ok(record),
            Err(error) => {
                self.record_failure(clip_db_id, &lease_token, &error).await;
                Err(error)
            }
        }
    }

    async fn prepare_claimed(
        &self,
        clip_db_id: i64,
        lease_token: &str,
    ) -> Result<ClipPreparationRecord, PreparationError> {
        type ClipSourceRow = (
            String,
            String,
            String,
            String,
            Option<String>,
            Option<String>,
            bool,
        );
        let clip: Option<ClipSourceRow> = sqlx::query_as(
            "SELECT clip_url, source_kind, clip_id, streamer_login, \
                    NULLIF(local_file_path, ''), NULLIF(upload_local_path, ''), \
                    discarded_at IS NOT NULL \
               FROM twitch_clips_social_media WHERE id = $1",
        )
        .bind(clip_db_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some((
            clip_url,
            source_kind,
            clip_id,
            streamer_login,
            local_file_path,
            upload_local_path,
            discarded,
        )) = clip
        else {
            return Err(PreparationError::ClipNotFound(clip_db_id));
        };
        if discarded {
            return Err(PreparationError::Discarded(clip_db_id));
        }

        let source = self
            .materialize_source(SourceMaterializationRequest {
                clip_db_id,
                clip_url: &clip_url,
                source_kind: &source_kind,
                clip_id: &clip_id,
                streamer_login: &streamer_login,
                local_file_path: local_file_path.as_deref(),
                upload_local_path: upload_local_path.as_deref(),
                lease_token,
            })
            .await?;
        let source_fingerprint = hash_file(&source.tool_path).await?;
        let source_ready = sqlx::query(
            "UPDATE social_media_clip_preparation \
                SET state = 'source_ready', source_fingerprint = $2, updated_at = CURRENT_TIMESTAMP \
              WHERE clip_db_id = $1 AND lease_token = $3",
        )
        .bind(clip_db_id)
        .bind(&source_fingerprint)
        .bind(lease_token)
        .execute(&self.pool)
        .await?
        .rows_affected();
        if source_ready != 1 {
            return Err(PreparationError::Busy(clip_db_id));
        }

        let layout = get_clip_effective_layout_checked(&self.pool, clip_db_id).await?;
        let fingerprint = render_fingerprint(&source_fingerprint, &layout);
        let destination = render_path(&self.clips_dir, clip_db_id, &fingerprint);
        let previous = self.get(clip_db_id).await?;
        let rendering = sqlx::query(
            "UPDATE social_media_clip_preparation SET state = 'rendering', \
             render_fingerprint = $2, updated_at = CURRENT_TIMESTAMP \
             WHERE clip_db_id = $1 AND lease_token = $3",
        )
        .bind(clip_db_id)
        .bind(&fingerprint)
        .bind(lease_token)
        .execute(&self.pool)
        .await?
        .rows_affected();
        if rendering != 1 {
            return Err(PreparationError::Busy(clip_db_id));
        }
        // Ein Commit-Timeout kann das von uns bereits no-clobber publizierte
        // Artefakt bei zurückgerollter DB-TX zurücklassen. Der vorherige Lauf
        // hat den aktuellen Fingerprint schon lease-gebunden gespeichert; nur
        // in genau diesem Fall darf das erwartete, bot-eigene Ziel adoptiert
        // werden. Ein beliebig vorplatziertes Ziel eines neuen Auftrags bleibt
        // damit ein Fehler und wird nie als Vorschau übernommen.
        let reusable = if previous.as_ref().is_some_and(|record| {
            record.render_fingerprint.as_deref() == Some(fingerprint.as_str())
        }) {
            open_controlled_render(&self.clips_dir, &destination)
                .await?
                .is_some()
        } else {
            false
        };
        if reusable {
            self.finalize_ready(
                clip_db_id,
                &source_fingerprint,
                &fingerprint,
                &destination,
                None,
                lease_token,
            )
            .await?;
            return self
                .get(clip_db_id)
                .await?
                .ok_or(PreparationError::ClipNotFound(clip_db_id));
        }

        let work_directory =
            open_or_create_secure_directory(&self.clips_dir.join(".preparation-work"), 0o700)
                .await?;
        let temporary_name = std::ffi::OsString::from(format!(
            "{clip_db_id}-{lease_token}-{fingerprint}-render.tmp.mp4"
        ));
        let temporary = work_directory.child_path(&temporary_name);
        let render_result = self
            .renderer
            .compose_and_trim(
                &source.tool_path,
                &temporary,
                &layout,
                PREPARATION_MAX_DURATION_SECS,
            )
            .await;
        if let Err(error) = render_result {
            let _ = unlink_from_directory(&work_directory, &temporary_name);
            return Err(error);
        }
        let artifact = validate_work_artifact(work_directory, temporary_name).await?;
        self.finalize_ready(
            clip_db_id,
            &source_fingerprint,
            &fingerprint,
            &destination,
            Some(artifact),
            lease_token,
        )
        .await?;
        if let Some(old_path) = previous.and_then(|record| record.render_path) {
            let old_path = PathBuf::from(old_path);
            if old_path != destination {
                remove_derived_render(&self.clips_dir, clip_db_id, &old_path).await;
            }
        }
        self.get(clip_db_id)
            .await?
            .ok_or(PreparationError::ClipNotFound(clip_db_id))
    }

    async fn materialize_source(
        &self,
        request: SourceMaterializationRequest<'_>,
    ) -> Result<OpenedSource, PreparationError> {
        let SourceMaterializationRequest {
            clip_db_id,
            clip_url,
            source_kind,
            clip_id,
            streamer_login,
            local_file_path,
            upload_local_path,
            lease_token,
        } = request;
        let expected_source = match source_kind {
            "manual_upload"
                if safe_media_component(clip_id) && safe_media_component(streamer_login) =>
            {
                self.clips_dir
                    .join("uploads")
                    .join(streamer_login)
                    .join(format!("{clip_id}.mp4"))
            }
            "twitch" => self.clips_dir.join(format!("{clip_db_id}.mp4")),
            _ => return Err(PreparationError::SourceMissing(clip_db_id)),
        };
        for candidate in [upload_local_path, local_file_path].into_iter().flatten() {
            if Path::new(candidate) == expected_source {
                if let Some(source) =
                    open_controlled_source(&self.clips_dir, &expected_source).await?
                {
                    return Ok(source);
                }
            }
        }
        if let Some(source) = open_controlled_source(&self.clips_dir, &expected_source).await? {
            if source_kind == "twitch" {
                self.persist_source_path(clip_db_id, &source.logical_path, lease_token)
                    .await?;
            }
            return Ok(source);
        }
        if source_kind != "twitch" || clip_url.trim().is_empty() {
            return Err(PreparationError::SourceMissing(clip_db_id));
        }

        let work_directory =
            open_or_create_secure_directory(&self.clips_dir.join(".preparation-work"), 0o700)
                .await?;
        let temporary_name =
            std::ffi::OsString::from(format!("{clip_db_id}-{lease_token}-source.tmp.mp4"));
        let temporary = work_directory.child_path(&temporary_name);
        let result = self.downloader.download(clip_url, &temporary).await;
        if let Err(error) = result {
            let _ = unlink_from_directory(&work_directory, &temporary_name);
            return Err(error);
        }
        let artifact = validate_work_artifact(work_directory, temporary_name).await?;
        self.finalize_download(clip_db_id, lease_token, artifact, &expected_source)
            .await?;
        open_controlled_source(&self.clips_dir, &expected_source)
            .await?
            .ok_or(PreparationError::SourceMissing(clip_db_id))
    }

    async fn finalize_download(
        &self,
        clip_db_id: i64,
        lease_token: &str,
        temporary: WorkArtifact,
        source: &Path,
    ) -> Result<(), PreparationError> {
        let mut transaction = self.pool.begin().await?;
        let owns_lease: Option<i64> = sqlx::query_scalar(
            "SELECT clip_db_id FROM social_media_clip_preparation \
             WHERE clip_db_id = $1 AND lease_token = $2 AND state = 'materializing' FOR UPDATE",
        )
        .bind(clip_db_id)
        .bind(lease_token)
        .fetch_optional(transaction.as_mut())
        .await?;
        if owns_lease.is_none() {
            transaction.rollback().await?;
            let _ = unlink_from_directory(&temporary.directory, &temporary.name);
            return Err(PreparationError::Busy(clip_db_id));
        }
        let destination_directory =
            open_or_create_secure_directory(&self.clips_dir, 0o2750).await?;
        let destination_name = source.file_name().ok_or_else(|| {
            PreparationError::Io(std::io::Error::from(std::io::ErrorKind::InvalidInput))
        })?;
        if let Err(error) =
            publish_artifact_noreplace(&temporary, &destination_directory, destination_name)
        {
            transaction.rollback().await?;
            let _ = unlink_from_directory(&temporary.directory, &temporary.name);
            return Err(PreparationError::Io(error));
        }
        let persisted = self
            .persist_source_path_in_transaction(
                transaction.as_mut(),
                clip_db_id,
                source,
                lease_token,
            )
            .await;
        if let Err(error) = persisted {
            transaction.rollback().await?;
            let _ = unlink_from_directory(&destination_directory, destination_name);
            return Err(error);
        }
        if let Err(error) = transaction.commit().await {
            // Commit kann nach dem Senden bereits erfolgt sein. Die sichere
            // Quelldatei bleibt deshalb am deterministischen Ziel und wird im
            // nächsten Lauf anhand der DB-Lease erneut abgeglichen.
            return Err(PreparationError::Db(error));
        }
        Ok(())
    }

    async fn persist_source_path(
        &self,
        clip_db_id: i64,
        source: &Path,
        lease_token: &str,
    ) -> Result<(), PreparationError> {
        let mut transaction = self.pool.begin().await?;
        self.persist_source_path_in_transaction(
            transaction.as_mut(),
            clip_db_id,
            source,
            lease_token,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    async fn persist_source_path_in_transaction(
        &self,
        connection: &mut sqlx::PgConnection,
        clip_db_id: i64,
        source: &Path,
        lease_token: &str,
    ) -> Result<(), PreparationError> {
        let updated = sqlx::query(
            "UPDATE twitch_clips_social_media SET local_file_path = $2, \
             downloaded_at = COALESCE(downloaded_at, CURRENT_TIMESTAMP) WHERE id = $1 \
             AND EXISTS (SELECT 1 FROM social_media_clip_preparation \
                         WHERE clip_db_id = $1 AND lease_token = $3)",
        )
        .bind(clip_db_id)
        .bind(source.to_string_lossy().as_ref())
        .bind(lease_token)
        .execute(&mut *connection)
        .await?
        .rows_affected();
        if updated != 1 {
            return Err(PreparationError::Busy(clip_db_id));
        }
        Ok(())
    }

    async fn finalize_ready(
        &self,
        clip_db_id: i64,
        source_fingerprint: &str,
        fingerprint: &str,
        destination: &Path,
        temporary: Option<WorkArtifact>,
        lease_token: &str,
    ) -> Result<(), PreparationError> {
        let mut transaction = self.pool.begin().await?;
        let preparation: Option<(String, Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT state, render_fingerprint, lease_token \
             FROM social_media_clip_preparation WHERE clip_db_id = $1 FOR UPDATE",
        )
        .bind(clip_db_id)
        .fetch_optional(transaction.as_mut())
        .await?;
        let Some((state, claimed_fingerprint, claimed_lease)) = preparation else {
            transaction.rollback().await?;
            if let Some(temporary) = temporary.as_ref() {
                discard_work_artifact(temporary);
            }
            return Err(PreparationError::ClipNotFound(clip_db_id));
        };
        let discarded: Option<bool> = sqlx::query_scalar(
            "SELECT discarded_at IS NOT NULL FROM twitch_clips_social_media \
             WHERE id = $1 FOR UPDATE",
        )
        .bind(clip_db_id)
        .fetch_optional(transaction.as_mut())
        .await?;
        let Some(discarded) = discarded else {
            transaction.rollback().await?;
            if let Some(temporary) = temporary.as_ref() {
                discard_work_artifact(temporary);
            }
            return Err(PreparationError::ClipNotFound(clip_db_id));
        };
        if discarded {
            transaction.rollback().await?;
            if let Some(temporary) = temporary.as_ref() {
                discard_work_artifact(temporary);
            }
            return Err(PreparationError::Discarded(clip_db_id));
        }
        if state != "rendering"
            || claimed_fingerprint.as_deref() != Some(fingerprint)
            || claimed_lease.as_deref() != Some(lease_token)
        {
            transaction.rollback().await?;
            if let Some(temporary) = temporary.as_ref() {
                discard_work_artifact(temporary);
            }
            return Err(PreparationError::Busy(clip_db_id));
        }

        let destination_directory =
            open_or_create_secure_directory(&self.clips_dir.join("rendered"), 0o2750).await?;
        let destination_name = destination.file_name().ok_or_else(|| {
            PreparationError::Io(std::io::Error::from(std::io::ErrorKind::InvalidInput))
        })?;
        let renamed = if let Some(temporary) = temporary.as_ref() {
            if let Err(error) =
                publish_artifact_noreplace(temporary, &destination_directory, destination_name)
            {
                transaction.rollback().await?;
                discard_work_artifact(temporary);
                return Err(PreparationError::Io(error));
            }
            true
        } else {
            false
        };
        let updated = sqlx::query(
            "UPDATE social_media_clip_preparation SET state = 'preview_ready', \
             source_fingerprint = $2, render_fingerprint = $3, render_path = $4, \
             lease_token = NULL, \
             error_code = NULL, error_message = NULL, completed_at = CURRENT_TIMESTAMP, \
             updated_at = CURRENT_TIMESTAMP WHERE clip_db_id = $1 \
               AND state = 'rendering' AND render_fingerprint = $3 AND lease_token = $5",
        )
        .bind(clip_db_id)
        .bind(source_fingerprint)
        .bind(fingerprint)
        .bind(destination.to_string_lossy().as_ref())
        .bind(lease_token)
        .execute(transaction.as_mut())
        .await;
        let result = match updated {
            Ok(result) if result.rows_affected() == 1 => transaction.commit().await,
            Ok(_) => {
                transaction.rollback().await?;
                if renamed {
                    let _ = unlink_from_directory(&destination_directory, destination_name);
                }
                return Err(PreparationError::Busy(clip_db_id));
            }
            Err(error) => {
                transaction.rollback().await?;
                if renamed {
                    let _ = unlink_from_directory(&destination_directory, destination_name);
                }
                return Err(PreparationError::Db(error));
            }
        };
        if let Err(error) = result {
            // Commit-Ausgang kann unklar sein; das deterministische, sichere
            // Artefakt bleibt deshalb erhalten und wird nicht blind gelöscht.
            return Err(PreparationError::Db(error));
        }
        Ok(())
    }

    async fn record_failure(&self, clip_db_id: i64, lease_token: &str, error: &PreparationError) {
        if let Err(db_error) = sqlx::query(
            "UPDATE social_media_clip_preparation SET state = 'failed', error_code = $2, \
             error_message = NULL, lease_token = NULL, completed_at = CURRENT_TIMESTAMP, \
             updated_at = CURRENT_TIMESTAMP WHERE clip_db_id = $1 \
             AND lease_token = $3 AND state IN ('materializing', 'source_ready', 'rendering')",
        )
        .bind(clip_db_id)
        .bind(error.code())
        .bind(lease_token)
        .execute(&self.pool)
        .await
        {
            tracing::warn!(%db_error, clip_db_id, "Clip-Aufbereitungsfehler konnte nicht gespeichert werden");
        }
    }

    /// Arbeitet offene Aufträge ab. Das Wiring kann unabhängig vom Upload-Worker
    /// aktiviert werden; der Kern selbst enthält keinerlei Plattformzugriff.
    pub async fn process_pending(&self, limit: i64) -> usize {
        match sqlx::query(
            "UPDATE social_media_clip_preparation SET state = 'pending', \
             lease_token = NULL, \
             error_code = 'preparation_stale', error_message = NULL, \
             requested_at = CURRENT_TIMESTAMP, updated_at = CURRENT_TIMESTAMP \
             WHERE state IN ('materializing', 'rendering') \
               AND updated_at < CURRENT_TIMESTAMP - INTERVAL '30 minutes'",
        )
        .execute(&self.pool)
        .await
        {
            Ok(result) if result.rows_affected() > 0 => {
                tracing::warn!(
                    count = result.rows_affected(),
                    stale_after_minutes = STALE_LEASE_MINUTES,
                    "Verwaiste Clip-Aufbereitungen wurden erneut eingereiht"
                );
            }
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(%error, "Verwaiste Clip-Aufbereitungen konnten nicht geprüft werden");
                return 0;
            }
        }
        if let Err(error) = self.cleanup_preparation_work().await {
            tracing::warn!(
                code = error.code(),
                "Verwaiste Medien-Arbeitsdateien konnten nicht sicher bereinigt werden"
            );
            return 0;
        }
        let ids: Vec<i64> = match sqlx::query_scalar(
            "SELECT clip_db_id FROM social_media_clip_preparation \
             WHERE state = 'pending' ORDER BY requested_at, clip_db_id LIMIT $1",
        )
        .bind(limit.max(0))
        .fetch_all(&self.pool)
        .await
        {
            Ok(ids) => ids,
            Err(error) => {
                tracing::warn!(%error, "Offene Clip-Aufbereitungen konnten nicht geladen werden");
                return 0;
            }
        };
        let mut completed = 0;
        for clip_db_id in ids {
            match self.prepare(clip_db_id).await {
                Ok(_) => completed += 1,
                Err(error) => {
                    tracing::warn!(
                        clip_db_id,
                        code = error.code(),
                        "Clip-Aufbereitung fehlgeschlagen"
                    );
                }
            }
        }
        completed
    }

    async fn cleanup_preparation_work(&self) -> Result<usize, PreparationError> {
        let directory =
            open_or_create_secure_directory(&self.clips_dir.join(".preparation-work"), 0o700)
                .await?;
        let mut entries =
            tokio::fs::read_dir(directory.child_path(std::ffi::OsStr::new(""))).await?;
        let mut removed = 0;
        let mut rejected = 0;
        while let Some(entry) = entries.next_entry().await? {
            let name = entry.file_name();
            let Some(name_text) = name.to_str() else {
                rejected += 1;
                continue;
            };
            let Some((clip_db_id, lease_token, expected_state)) =
                parse_work_artifact_name(name_text)
            else {
                rejected += 1;
                continue;
            };
            if !existing_work_artifact_is_safe(&directory, &name).await? {
                rejected += 1;
                continue;
            }
            let state = match expected_state {
                WorkArtifactState::Materializing => "materializing",
                WorkArtifactState::Rendering => "rendering",
            };
            let active: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM social_media_clip_preparation \
                 WHERE clip_db_id = $1 AND lease_token = $2 AND state = $3)",
            )
            .bind(clip_db_id)
            .bind(&lease_token)
            .bind(state)
            .fetch_one(&self.pool)
            .await?;
            if !active {
                match unlink_from_directory(&directory, &name) {
                    Ok(()) => removed += 1,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(PreparationError::Io(error)),
                }
            }
        }
        if rejected > 0 {
            tracing::warn!(
                count = rejected,
                "Unbekannte oder unsichere Preparation-Arbeitsdateien wurden nicht angefasst"
            );
        }
        Ok(removed)
    }
}

pub(crate) fn stored_file_is_regular_nonempty(path: &str) -> bool {
    std::fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_file() && metadata.len() > 0)
        .unwrap_or(false)
}

#[cfg(test)]
async fn set_media_permissions(path: &Path) -> Result<(), std::io::Error> {
    use std::os::unix::fs::PermissionsExt;
    tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o640)).await
}

pub struct ClipPreparationWorker {
    service: ClipPreparationService,
    interval: Duration,
    batch_size: i64,
}

impl ClipPreparationWorker {
    pub fn new(service: ClipPreparationService) -> Self {
        Self {
            service,
            interval: Duration::from_secs(WORKER_INTERVAL_SECS),
            batch_size: 2,
        }
    }

    pub async fn run_once(&self) -> usize {
        self.service.process_pending(self.batch_size).await
    }

    pub async fn run(&self) {
        loop {
            self.run_once().await;
            tokio::time::sleep(self.interval).await;
        }
    }
}

type PreparationRow = (
    i64,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
    Option<String>,
    Option<String>,
    String,
);

impl From<PreparationRow> for ClipPreparationRecord {
    fn from(row: PreparationRow) -> Self {
        Self {
            clip_db_id: row.0,
            state: row.1,
            source_fingerprint: row.2,
            render_fingerprint: row.3,
            render_path: row.4,
            error_code: row.5,
            error_message: row.6,
            requested_at: row.7,
            started_at: row.8,
            completed_at: row.9,
            updated_at: row.10,
        }
    }
}

async fn clip_exists(pool: &PgPool, clip_db_id: i64) -> Result<bool, sqlx::Error> {
    Ok(
        sqlx::query_scalar::<_, i32>("SELECT 1 FROM twitch_clips_social_media WHERE id = $1")
            .bind(clip_db_id)
            .fetch_optional(pool)
            .await?
            .is_some(),
    )
}

async fn hash_file(path: &Path) -> Result<String, std::io::Error> {
    use tokio::io::AsyncReadExt;

    let mut file = tokio::fs::File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).await?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub fn render_fingerprint(source_fingerprint: &str, layout: &StreamerLayout) -> String {
    let mut hasher = Sha256::new();
    hasher.update(RENDERER_VERSION.as_bytes());
    hasher.update([0]);
    hasher.update(source_fingerprint.as_bytes());
    hasher.update([0]);
    hasher.update(layout.to_override_json().to_string().as_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn render_path(clips_dir: &Path, clip_db_id: i64, fingerprint: &str) -> PathBuf {
    clips_dir
        .join("rendered")
        .join(format!("{clip_db_id}-{fingerprint}.mp4"))
}

pub(crate) fn validate_twitch_clip_url(raw: &str) -> Result<(), PreparationError> {
    let authority = raw
        .strip_prefix("https://")
        .and_then(|rest| rest.split(['/', '?', '#']).next())
        .ok_or(PreparationError::InvalidSourceUrl)?;
    // `url` normalisiert einen ausdrücklich angegebenen Standardport weg. Für
    // die enge Downloader-Allowlist prüfen wir deshalb zusätzlich die rohe
    // Authority und erlauben weder Ports noch Userinfo.
    if authority.contains(':') || authority.contains('@') {
        return Err(PreparationError::InvalidSourceUrl);
    }
    let parsed = url::Url::parse(raw).map_err(|_| PreparationError::InvalidSourceUrl)?;
    let allowed_host = matches!(
        parsed.host_str().map(str::to_ascii_lowercase).as_deref(),
        Some("clips.twitch.tv") | Some("twitch.tv") | Some("www.twitch.tv")
    );
    if parsed.scheme() != "https"
        || !allowed_host
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.port().is_some()
    {
        return Err(PreparationError::InvalidSourceUrl);
    }
    let segments: Vec<&str> = parsed
        .path_segments()
        .map(|segments| segments.filter(|segment| !segment.is_empty()).collect())
        .unwrap_or_default();
    let is_clip_path = match parsed.host_str().map(str::to_ascii_lowercase).as_deref() {
        Some("clips.twitch.tv") => segments.len() == 1,
        Some("twitch.tv") | Some("www.twitch.tv") => {
            (segments.len() == 2 && segments[0].eq_ignore_ascii_case("clip"))
                || (segments.len() == 3 && segments[1].eq_ignore_ascii_case("clip"))
        }
        _ => false,
    };
    if !is_clip_path {
        return Err(PreparationError::InvalidSourceUrl);
    }
    Ok(())
}

fn safe_media_component(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

/// Entfernt ausschließlich Dateien direkt unter `<clips>/rendered`; ein
/// beschädigter DB-Pfad darf nie beliebige Dateien löschen.
pub async fn remove_derived_render(clips_dir: &Path, clip_db_id: i64, candidate: &Path) -> bool {
    let rendered_dir = clips_dir.join("rendered");
    let Some(name) = candidate.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let expected_prefix = format!("{clip_db_id}-");
    let fingerprint = name
        .strip_prefix(&expected_prefix)
        .and_then(|name| name.strip_suffix(".mp4"));
    if candidate.parent() != Some(rendered_dir.as_path())
        || fingerprint.is_none_or(|fingerprint| {
            fingerprint.len() != 64
                || !fingerprint
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        })
    {
        return false;
    }
    let directory = match open_or_create_secure_directory(&rendered_dir, 0o2750).await {
        Ok(directory) => directory,
        Err(_) => return false,
    };
    match open_controlled_render(clips_dir, candidate).await {
        Ok(Some(_)) => {}
        Ok(None) => return true,
        Err(_) => return false,
    }
    match unlink_from_directory(&directory, std::ffi::OsStr::new(name)) {
        Ok(()) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use crate::layout::default_streamer_layout;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;

    #[test]
    fn render_fingerprint_ist_stabil_und_layoutsensitiv() {
        let layout = default_streamer_layout();
        let first = super::render_fingerprint("source-sha", &layout);
        let second = super::render_fingerprint("source-sha", &layout);
        assert_eq!(first, second);

        let mut changed = layout;
        changed.cam_enabled = false;
        assert_ne!(first, super::render_fingerprint("source-sha", &changed));
    }

    #[test]
    fn renderpfad_ist_pro_clip_und_fingerprint_eindeutig() {
        let root = std::path::Path::new("data/clips");
        assert_eq!(
            super::render_path(root, 42, "abcdef"),
            root.join("rendered/42-abcdef.mp4")
        );
    }

    #[test]
    fn downloader_erlaubt_nur_https_twitch_clip_urls_ohne_port_oder_userinfo() {
        for allowed in [
            "https://clips.twitch.tv/FancyClip",
            "https://www.twitch.tv/nani/clip/FancyClip",
            "https://twitch.tv/nani/clip/FancyClip",
        ] {
            assert!(
                super::validate_twitch_clip_url(allowed).is_ok(),
                "{allowed}"
            );
        }
        for denied in [
            "http://clips.twitch.tv/FancyClip",
            "https://clips.twitch.tv:443/FancyClip",
            "https://user@clips.twitch.tv/FancyClip",
            "https://clips.twitch.tv.evil.test/FancyClip",
            "file:///etc/passwd",
            "https://127.0.0.1/clip",
        ] {
            assert!(super::validate_twitch_clip_url(denied).is_err(), "{denied}");
        }
    }

    async fn make_pool(schema: &str) -> Option<sqlx::PgPool> {
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
        let options = PgConnectOptions::from_str(&dsn)
            .unwrap()
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(options)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE twitch_clips_social_media (id BIGSERIAL PRIMARY KEY, clip_url TEXT NOT NULL, \
             source_kind TEXT NOT NULL DEFAULT 'twitch', local_file_path TEXT, upload_local_path TEXT, \
             downloaded_at TIMESTAMPTZ, discarded_at TIMESTAMPTZ, status TEXT DEFAULT 'pending', \
             streamer_login TEXT NOT NULL DEFAULT 'nani', clip_id TEXT NOT NULL DEFAULT 'test-clip', \
             layout_override_json JSONB)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE social_media_clip_preparation (clip_db_id BIGINT PRIMARY KEY REFERENCES \
             twitch_clips_social_media(id), state TEXT NOT NULL DEFAULT 'pending', lease_token TEXT, source_fingerprint TEXT, \
             render_fingerprint TEXT, render_path TEXT, error_code TEXT, error_message TEXT, \
             requested_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), started_at TIMESTAMPTZ, completed_at TIMESTAMPTZ, \
             updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW())",
        )
        .execute(&pool)
        .await
        .unwrap();
        for ddl in [
            "CREATE TABLE social_media_streamer_layout (streamer_login TEXT PRIMARY KEY, layout_json JSONB NOT NULL DEFAULT '{}'::jsonb, cam_enabled BOOLEAN NOT NULL DEFAULT TRUE, mode TEXT NOT NULL DEFAULT 'pip')",
            "CREATE TABLE social_media_clip_approval (clip_db_id INTEGER PRIMARY KEY, state TEXT NOT NULL DEFAULT 'awaiting_approval', approved_platforms JSONB NOT NULL DEFAULT '[]'::jsonb, approver_user_id TEXT, decided_at TIMESTAMPTZ, approved_render_fingerprint TEXT)",
            "CREATE TABLE twitch_clips_upload_queue (id BIGSERIAL PRIMARY KEY, clip_id BIGINT NOT NULL, status TEXT NOT NULL DEFAULT 'pending', last_error TEXT, last_attempt_at TIMESTAMPTZ, provider_started_at TIMESTAMPTZ, provider_lease_token TEXT, provider_external_id TEXT, provider_accepted_at TIMESTAMPTZ)",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        Some(pool)
    }

    #[test]
    fn workdateinamen_binden_clip_lease_und_renderfingerprint() {
        let lease = uuid::Uuid::new_v4();
        let source = format!("42-{lease}-source.tmp.mp4");
        assert_eq!(
            super::parse_work_artifact_name(&source),
            Some((
                42,
                lease.to_string(),
                super::WorkArtifactState::Materializing
            ))
        );
        let render = format!("42-{lease}-{}-render.tmp.mp4", "a".repeat(64));
        assert_eq!(
            super::parse_work_artifact_name(&render),
            Some((42, lease.to_string(), super::WorkArtifactState::Rendering))
        );
        for invalid in [
            "42-not-a-uuid-source.tmp.mp4",
            "42-00000000-0000-0000-0000-000000000000-short-render.tmp.mp4",
            "../42-source.tmp.mp4",
            "42-00000000-0000-0000-0000-000000000000-source.tmp.mp4.link",
        ] {
            assert!(
                super::parse_work_artifact_name(invalid).is_none(),
                "{invalid}"
            );
        }
    }

    #[tokio::test]
    async fn workartefakt_wird_bei_validierungsfehler_entfernt_und_fd_gebunden_publiziert() {
        let root = std::env::temp_dir().join(format!(
            "tb-preparation-held-artifact-{}",
            uuid::Uuid::new_v4()
        ));
        let work = root.join(".preparation-work");
        let rendered = root.join("rendered");
        tokio::fs::create_dir_all(&work).await.unwrap();
        tokio::fs::create_dir_all(&rendered).await.unwrap();

        let empty_name = std::ffi::OsString::from("empty.tmp.mp4");
        tokio::fs::write(work.join(&empty_name), b"").await.unwrap();
        let empty_directory = super::open_or_create_secure_directory(&work, 0o700)
            .await
            .unwrap();
        assert!(
            super::validate_work_artifact(empty_directory, empty_name.clone())
                .await
                .is_err()
        );
        assert!(!work.join(empty_name).exists());

        let name = std::ffi::OsString::from("valid.tmp.mp4");
        let source = work.join(&name);
        tokio::fs::write(&source, b"gehaltener-inode")
            .await
            .unwrap();
        let artifact = super::validate_work_artifact(
            super::open_or_create_secure_directory(&work, 0o700)
                .await
                .unwrap(),
            name.clone(),
        )
        .await
        .unwrap();
        let destination_directory = super::open_or_create_secure_directory(&rendered, 0o2750)
            .await
            .unwrap();
        super::publish_artifact_noreplace(
            &artifact,
            &destination_directory,
            std::ffi::OsStr::new("fertig.mp4"),
        )
        .unwrap();
        assert_eq!(
            tokio::fs::read(rendered.join("fertig.mp4")).await.unwrap(),
            b"gehaltener-inode"
        );
        assert!(!source.exists());

        let swapped_name = std::ffi::OsString::from("swapped.tmp.mp4");
        let swapped_source = work.join(&swapped_name);
        tokio::fs::write(&swapped_source, b"original")
            .await
            .unwrap();
        let swapped = super::validate_work_artifact(
            super::open_or_create_secure_directory(&work, 0o700)
                .await
                .unwrap(),
            swapped_name,
        )
        .await
        .unwrap();
        tokio::fs::remove_file(&swapped_source).await.unwrap();
        tokio::fs::write(&swapped_source, b"ausgetauschter-name")
            .await
            .unwrap();
        assert!(super::publish_artifact_noreplace(
            &swapped,
            &destination_directory,
            std::ffi::OsStr::new("darf-nicht-publiziert-werden.mp4"),
        )
        .is_err());
        assert_eq!(
            tokio::fs::read(&swapped_source).await.unwrap(),
            b"ausgetauschter-name"
        );
        assert!(!rendered.join("darf-nicht-publiziert-werden.mp4").exists());
        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    /// Ohne kontrollierten Egress darf auch ein korrekt gestagtes Binary nicht
    /// ins Host-Netz. Die bereits validierte Twitch-URL bleibt dabei Daten,
    /// und es entsteht kein scheinbar fertiges Quellartefakt.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn twitch_download_bleibt_ohne_helper_gesperrt() {
        use super::{ClipSourceDownloader, PreparationError};

        let root = std::env::temp_dir().join(format!("tb-ytdlp-closed-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let destination = root.join("clip.mp4");
        let downloader = super::IsolatedClipDownloader::new();
        let error = downloader
            .download(
                "https://clips.twitch.tv/FamousRoundHorseAMPTropPunch-jjgWeEWUq2Yw7caL",
                &destination,
            )
            .await
            .unwrap_err();
        assert!(matches!(error, PreparationError::DownloadIsolationRequired));
        assert!(!destination.exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn reaper_entfernt_nur_verwaiste_gueltige_workdateien() {
        let Some(pool) = make_pool("t_sm_preparation_work_reaper").await else {
            return;
        };
        let clip_db_id: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media (clip_url) VALUES \
             ('https://clips.twitch.tv/WorkReaper') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let active_lease = uuid::Uuid::new_v4();
        sqlx::query(
            "INSERT INTO social_media_clip_preparation (clip_db_id, state, lease_token) \
             VALUES ($1, 'materializing', $2)",
        )
        .bind(clip_db_id)
        .bind(active_lease.to_string())
        .execute(&pool)
        .await
        .unwrap();
        let clips_dir =
            std::env::temp_dir().join(format!("tb-sm-work-reaper-{}", uuid::Uuid::new_v4()));
        let work_dir = clips_dir.join(".preparation-work");
        tokio::fs::create_dir_all(&work_dir).await.unwrap();
        let active = work_dir.join(format!("{clip_db_id}-{active_lease}-source.tmp.mp4"));
        let orphan = work_dir.join(format!(
            "{clip_db_id}-{}-source.tmp.mp4",
            uuid::Uuid::new_v4()
        ));
        let unknown = work_dir.join("fremde-datei.mp4");
        for path in [&active, &orphan, &unknown] {
            tokio::fs::write(path, b"work").await.unwrap();
            super::set_media_permissions(path).await.unwrap();
        }
        let service = super::ClipPreparationService::new(pool).with_clips_dir(clips_dir.clone());
        assert_eq!(service.cleanup_preparation_work().await.unwrap(), 1);
        assert!(active.exists());
        assert!(!orphan.exists());
        assert!(unknown.exists());
        std::fs::remove_dir_all(clips_dir).unwrap();
    }

    struct RejectingRenderer(std::sync::Arc<std::sync::atomic::AtomicUsize>);

    #[async_trait::async_trait]
    impl super::ClipRenderer for RejectingRenderer {
        async fn compose_and_trim(
            &self,
            _: &std::path::Path,
            _: &std::path::Path,
            _: &crate::layout::StreamerLayout,
            _: i64,
        ) -> Result<(), super::PreparationError> {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err(super::PreparationError::Renderer(
                "Renderer darf bei Adoption nicht laufen".to_string(),
            ))
        }
    }

    #[tokio::test]
    async fn commit_unklar_artefakt_wird_nur_mit_vorherigem_fingerprint_adoptiert() {
        let Some(pool) = make_pool("t_sm_preparation_ambiguous_adopt").await else {
            return;
        };
        let clip_db_id: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media (clip_url, clip_id) VALUES \
             ('https://clips.twitch.tv/AmbiguousCommit', 'ambiguous') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let clips_dir =
            std::env::temp_dir().join(format!("tb-sm-ambiguous-adopt-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(clips_dir.join("rendered"))
            .await
            .unwrap();
        let source = clips_dir.join(format!("{clip_db_id}.mp4"));
        tokio::fs::write(&source, b"sichere quelle").await.unwrap();
        let source_fingerprint = super::hash_file(&source).await.unwrap();
        let fingerprint = super::render_fingerprint(
            &source_fingerprint,
            &crate::layout::default_streamer_layout(),
        );
        let destination = super::render_path(&clips_dir, clip_db_id, &fingerprint);
        tokio::fs::write(&destination, b"fertiges render")
            .await
            .unwrap();
        super::set_media_permissions(&destination).await.unwrap();
        sqlx::query(
            "INSERT INTO social_media_clip_preparation \
             (clip_db_id, state, render_fingerprint, error_code) \
             VALUES ($1, 'failed', $2, 'database_failed')",
        )
        .bind(clip_db_id)
        .bind(&fingerprint)
        .execute(&pool)
        .await
        .unwrap();
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let record = super::ClipPreparationService::new(pool)
            .with_clips_dir(clips_dir.clone())
            .with_renderer(std::sync::Arc::new(RejectingRenderer(calls.clone())))
            .prepare(clip_db_id)
            .await
            .unwrap();
        assert!(record.preview_ready());
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        std::fs::remove_dir_all(clips_dir).unwrap();
    }

    #[tokio::test]
    async fn vorplatziertes_ziel_ohne_passenden_vorlauf_wird_nicht_adoptiert() {
        let Some(pool) = make_pool("t_sm_preparation_preplaced_target").await else {
            return;
        };
        let clip_db_id: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media (clip_url) VALUES \
             ('https://clips.twitch.tv/PreplacedTarget') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let clips_dir =
            std::env::temp_dir().join(format!("tb-sm-preplaced-target-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(clips_dir.join("rendered"))
            .await
            .unwrap();
        let source = clips_dir.join(format!("{clip_db_id}.mp4"));
        tokio::fs::write(&source, b"quelle").await.unwrap();
        let source_fingerprint = super::hash_file(&source).await.unwrap();
        let fingerprint = super::render_fingerprint(
            &source_fingerprint,
            &crate::layout::default_streamer_layout(),
        );
        let destination = super::render_path(&clips_dir, clip_db_id, &fingerprint);
        tokio::fs::write(&destination, b"vorgepflanzt")
            .await
            .unwrap();
        super::set_media_permissions(&destination).await.unwrap();
        sqlx::query(
            "INSERT INTO social_media_clip_preparation \
             (clip_db_id, state, render_fingerprint) VALUES ($1, 'failed', $2)",
        )
        .bind(clip_db_id)
        .bind("b".repeat(64))
        .execute(&pool)
        .await
        .unwrap();
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let error = super::ClipPreparationService::new(pool.clone())
            .with_clips_dir(clips_dir.clone())
            .with_renderer(std::sync::Arc::new(RejectingRenderer(calls.clone())))
            .prepare(clip_db_id)
            .await
            .unwrap_err();
        assert!(matches!(error, super::PreparationError::Renderer(_)));
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(std::fs::read(&destination).unwrap(), b"vorgepflanzt");
        let error_code: Option<String> = sqlx::query_scalar(
            "SELECT error_code FROM social_media_clip_preparation WHERE clip_db_id = $1",
        )
        .bind(clip_db_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(error_code.as_deref(), Some("render_failed"));
        std::fs::remove_dir_all(clips_dir).unwrap();
    }

    #[tokio::test]
    async fn ungueltiges_gespeichertes_layout_rendert_nicht_still_den_default() {
        let Some(pool) = make_pool("t_sm_preparation_invalid_layout").await else {
            return;
        };
        let clip_db_id: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media (clip_url, layout_override_json) VALUES \
             ('https://clips.twitch.tv/InvalidLayout', '{\"version\":999}'::jsonb) RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let clips_dir =
            std::env::temp_dir().join(format!("tb-sm-invalid-layout-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&clips_dir).await.unwrap();
        let source = clips_dir.join(format!("{clip_db_id}.mp4"));
        tokio::fs::write(&source, b"quelle").await.unwrap();
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let service = super::ClipPreparationService::new(pool.clone())
            .with_clips_dir(clips_dir.clone())
            .with_renderer(std::sync::Arc::new(RejectingRenderer(calls.clone())));
        let error = service.prepare(clip_db_id).await.unwrap_err();
        assert!(matches!(
            error,
            super::PreparationError::Layout(crate::layout::EffectiveLayoutError::InvalidClipLayout)
        ));
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        let state: (String, Option<String>) = sqlx::query_as(
            "SELECT state, error_code FROM social_media_clip_preparation WHERE clip_db_id = $1",
        )
        .bind(clip_db_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(state.0, "failed");
        assert_eq!(state.1.as_deref(), Some("layout_invalid"));
        std::fs::remove_dir_all(clips_dir).unwrap();
    }

    #[tokio::test]
    async fn request_ueberschreibt_keine_laufende_aufbereitung() {
        let Some(pool) = make_pool("t_sm_preparation_busy").await else {
            return;
        };
        let clip_db_id: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media (clip_url) VALUES \
             ('https://clips.twitch.tv/FancyClip') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO social_media_clip_preparation (clip_db_id, state) VALUES ($1, 'rendering')",
        )
        .bind(clip_db_id)
        .execute(&pool)
        .await
        .unwrap();

        let error = super::ClipPreparationService::new(pool.clone())
            .request(clip_db_id)
            .await
            .unwrap_err();
        assert!(matches!(error, super::PreparationError::Busy(id) if id == clip_db_id));
        let state: String = sqlx::query_scalar(
            "SELECT state FROM social_media_clip_preparation WHERE clip_db_id = $1",
        )
        .bind(clip_db_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(state, "rendering");
    }

    #[tokio::test]
    async fn request_reaktiviert_nur_verwaiste_lease() {
        let Some(pool) = make_pool("t_sm_preparation_stale").await else {
            return;
        };
        let clip_db_id: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media (clip_url) VALUES \
             ('https://clips.twitch.tv/StaleClip') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO social_media_clip_preparation (clip_db_id, state, updated_at) \
             VALUES ($1, 'materializing', NOW() - INTERVAL '31 minutes')",
        )
        .bind(clip_db_id)
        .execute(&pool)
        .await
        .unwrap();

        let record = super::ClipPreparationService::new(pool)
            .request(clip_db_id)
            .await
            .unwrap();
        assert_eq!(record.state, "pending");
    }

    #[tokio::test]
    async fn discard_waehrend_rendering_kann_nicht_ready_werden_und_raeumt_tempdatei() {
        let Some(pool) = make_pool("t_sm_preparation_discard_race").await else {
            return;
        };
        let clip_db_id: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media (clip_url) VALUES \
             ('https://clips.twitch.tv/DiscardRace') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO social_media_clip_preparation \
             (clip_db_id, state, lease_token, source_fingerprint, render_fingerprint) \
             VALUES ($1, 'rendering', 'lease-a', 'source', 'render')",
        )
        .bind(clip_db_id)
        .execute(&pool)
        .await
        .unwrap();
        let mut clips_dir = std::env::temp_dir();
        clips_dir.push(format!(
            "tb_sm_preparation_discard_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let rendered_dir = clips_dir.join("rendered");
        tokio::fs::create_dir_all(&rendered_dir).await.unwrap();
        let work_dir = clips_dir.join(".preparation-work");
        tokio::fs::create_dir_all(&work_dir).await.unwrap();
        let temporary_name =
            std::ffi::OsString::from(format!("{clip_db_id}-lease-a-render-render.tmp.mp4"));
        let temporary = work_dir.join(&temporary_name);
        let destination = rendered_dir.join(format!("{clip_db_id}-render.mp4"));
        tokio::fs::write(&temporary, b"render").await.unwrap();

        crate::retention::discard_clip(&pool, clip_db_id)
            .await
            .unwrap()
            .unwrap();
        let service =
            super::ClipPreparationService::new(pool.clone()).with_clips_dir(clips_dir.clone());
        let temporary = super::validate_work_artifact(
            super::open_or_create_secure_directory(&work_dir, 0o700)
                .await
                .unwrap(),
            temporary_name,
        )
        .await
        .unwrap();
        let error = service
            .finalize_ready(
                clip_db_id,
                "source",
                "render",
                &destination,
                Some(temporary),
                "lease-a",
            )
            .await
            .unwrap_err();
        assert!(matches!(error, super::PreparationError::Discarded(id) if id == clip_db_id));
        service.record_failure(clip_db_id, "lease-a", &error).await;
        assert!(!work_dir
            .join(format!("{clip_db_id}-lease-a-render-render.tmp.mp4"))
            .exists());
        assert!(!destination.exists());
        let preparation: (String, Option<String>) = sqlx::query_as(
            "SELECT state, error_code FROM social_media_clip_preparation WHERE clip_db_id = $1",
        )
        .bind(clip_db_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(preparation.0, "failed");
        assert_eq!(preparation.1.as_deref(), Some("clip_discarded"));
        let _ = std::fs::remove_dir_all(clips_dir);
    }

    #[tokio::test]
    async fn alte_lease_darf_neuen_lauf_weder_finalisieren_noch_auf_failed_setzen() {
        let Some(pool) = make_pool("t_sm_preparation_lease_generation").await else {
            return;
        };
        let clip_db_id: i64 = sqlx::query_scalar(
            "INSERT INTO twitch_clips_social_media (clip_url) VALUES \
             ('https://clips.twitch.tv/LeaseGeneration') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO social_media_clip_preparation \
             (clip_db_id, state, lease_token, source_fingerprint, render_fingerprint) \
             VALUES ($1, 'rendering', 'lease-neu', 'source', 'render')",
        )
        .bind(clip_db_id)
        .execute(&pool)
        .await
        .unwrap();
        let mut clips_dir = std::env::temp_dir();
        clips_dir.push(format!(
            "tb_sm_preparation_lease_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let destination = clips_dir
            .join("rendered")
            .join(format!("{clip_db_id}-render.mp4"));
        tokio::fs::create_dir_all(destination.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(&destination, b"neuer render")
            .await
            .unwrap();
        let service =
            super::ClipPreparationService::new(pool.clone()).with_clips_dir(clips_dir.clone());

        let error = service
            .finalize_ready(
                clip_db_id,
                "source",
                "render",
                &destination,
                None,
                "lease-alt",
            )
            .await
            .unwrap_err();
        assert!(matches!(error, super::PreparationError::Busy(id) if id == clip_db_id));
        service
            .record_failure(
                clip_db_id,
                "lease-alt",
                &super::PreparationError::Renderer("alter Lauf".to_string()),
            )
            .await;

        let state: (String, Option<String>, Option<String>) = sqlx::query_as(
            "SELECT state, lease_token, error_code FROM social_media_clip_preparation \
             WHERE clip_db_id = $1",
        )
        .bind(clip_db_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(state.0, "rendering");
        assert_eq!(state.1.as_deref(), Some("lease-neu"));
        assert!(state.2.is_none());
        assert!(destination.exists());
        let _ = std::fs::remove_dir_all(clips_dir);
    }
}
