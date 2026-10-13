use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use serde::Serialize;
use sqlx::{PgPool, Row};
use tb_domain::vod_range::{VodRange, MAX_FILE_BYTES};
use tb_transport_twitch::{streams::HelixArchiveVideo, HelixClient};
use tokio::io::AsyncReadExt;

const ADMISSION_LOCK: i64 = 74326811;
const WORKER_LOCK: i64 = 74326812;
pub const PROCESS_TIMEOUT: Duration = Duration::from_secs(900);
const TWITCH_CLOUDFRONT_HOSTS: &[&str] = &[
    "ds0h3roq6wcgc.cloudfront.net",
    "d2nvs31859zcd8.cloudfront.net",
    "d2aba1wr3818hz.cloudfront.net",
    "d3c27h4odz752x.cloudfront.net",
    "dgeft87wbj63p.cloudfront.net",
    "d1m7jfoe9zdc1j.cloudfront.net",
    "d3vd9lfkzbru3h.cloudfront.net",
    "ddacn6pr5v0tl.cloudfront.net",
    "d3aqoihi2n8ty8.cloudfront.net",
    "d3fi1amfgojobc.cloudfront.net",
    "d2vi6trrdongqn.cloudfront.net",
    "d3stzm2eumvgb4.cloudfront.net",
];

#[derive(Debug, thiserror::Error)]
pub enum RangeError {
    #[error("database")]
    Database(#[from] sqlx::Error),
    #[error("media_io")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Rejected(&'static str),
}

impl RangeError {
    pub fn message(&self) -> &'static str {
        match self {
            Self::Rejected("disabled") => "Der VOD-Befehl ist ausgeschaltet. Der Kanalinhaber kann ihn im Dashboard unter Chat einschalten.",
            Self::Rejected("active") => "Für diesen Kanal wird bereits ein VOD-Ausschnitt vorbereitet.",
            Self::Rejected("rate") => "Pro Kanal sind höchstens drei VOD-Ausschnitte pro Stunde möglich.",
            Self::Rejected("capacity") => "Die VOD-Warteschlange ist voll. Versuch es später nochmal.",
            Self::Rejected("unavailable") => "Für diesen Stream ist gerade keine Twitch-Aufzeichnung verfügbar. Bei einem laufenden Stream muss Twitch die Aufzeichnung bereits bereitstellen.",
            Self::Rejected("bounds") => "Der gewählte Bereich liegt außerhalb der verfügbaren Aufzeichnung. Versuch es später nochmal, wenn Twitch den Abschnitt bereitgestellt hat.",
            _ => "Der VOD-Ausschnitt konnte nicht vorbereitet werden. Versuch es später nochmal.",
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::Rejected(code) => code,
            Self::Database(_) => "database",
            Self::Io(_) => "media_io",
        }
    }
}

pub fn select_source(
    videos: Vec<HelixArchiveVideo>,
    owner: &str,
    live_stream: Option<&str>,
    range: VodRange,
) -> Result<HelixArchiveVideo, RangeError> {
    let source = videos
        .into_iter()
        .find(|video| {
            video.user_id == owner
                && live_stream.is_none_or(|id| video.stream_id.as_deref() == Some(id))
        })
        .ok_or(RangeError::Rejected("unavailable"))?;
    validate_source(&source, owner, range)?;
    Ok(source)
}

fn validate_source(
    video: &HelixArchiveVideo,
    owner: &str,
    range: VodRange,
) -> Result<(), RangeError> {
    if video.user_id != owner || !valid_id(&video.id) {
        return Err(RangeError::Rejected("unavailable"));
    }
    if video
        .duration_seconds()
        .is_none_or(|duration| range.end > duration)
    {
        return Err(RangeError::Rejected("bounds"));
    }
    Ok(())
}

fn valid_id(id: &str) -> bool {
    id.len() <= 20
        && !id.starts_with('0')
        && !id.is_empty()
        && id.bytes().all(|byte| byte.is_ascii_digit())
}

pub async fn submit(
    pool: &PgPool,
    helix: &HelixClient,
    owner: &str,
    range: VodRange,
) -> Result<String, RangeError> {
    if !valid_id(owner)
        || range.end <= range.start
        || !(60..=600).contains(&(range.end - range.start))
        || range.end > 172800
    {
        return Err(RangeError::Rejected("bounds"));
    }
    let enabled: bool = sqlx::query_scalar("SELECT COALESCE((SELECT enabled FROM twitch_private_vod_settings WHERE owner_id=$1), false)").bind(owner).fetch_one(pool).await?;
    if !enabled {
        return Err(RangeError::Rejected("disabled"));
    }
    let streams = helix
        .get_streams_by_user_ids(&[owner.to_string()], None)
        .await
        .map_err(|_| RangeError::Rejected("unavailable"))?;
    let live = streams.iter().find(|stream| stream.user_id == owner);
    let videos = helix
        .archive_videos(owner)
        .await
        .map_err(|_| RangeError::Rejected("unavailable"))?;
    let source = select_source(videos, owner, live.map(|stream| stream.id.as_str()), range)?;
    enqueue(pool, owner, &source.id, range).await
}

pub async fn enqueue(
    pool: &PgPool,
    owner: &str,
    video: &str,
    range: VodRange,
) -> Result<String, RangeError> {
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(ADMISSION_LOCK)
        .execute(&mut *tx)
        .await?;
    let enabled: bool = sqlx::query_scalar("SELECT COALESCE((SELECT enabled FROM twitch_private_vod_settings WHERE owner_id=$1), false)").bind(owner).fetch_one(&mut *tx).await?;
    if !enabled {
        return Err(RangeError::Rejected("disabled"));
    }
    let active: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM twitch_private_vod_jobs WHERE owner_id=$1 AND status IN ('queued','running'))").bind(owner).fetch_one(&mut *tx).await?;
    if active {
        return Err(RangeError::Rejected("active"));
    }
    let recent: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_private_vod_jobs WHERE owner_id=$1 AND created_at > now()-INTERVAL '1 hour'").bind(owner).fetch_one(&mut *tx).await?;
    if recent >= 3 {
        return Err(RangeError::Rejected("rate"));
    }
    let retained: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_private_vod_jobs WHERE expires_at > now() OR status IN ('queued','running','ready')").fetch_one(&mut *tx).await?;
    if retained >= 20 {
        return Err(RangeError::Rejected("capacity"));
    }
    let id = uuid::Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO twitch_private_vod_jobs (id,owner_id,video_id,start_seconds,end_seconds) VALUES ($1,$2,$3,$4,$5)")
        .bind(&id).bind(owner).bind(video).bind(range.start as i32).bind(range.end as i32).execute(&mut *tx).await?;
    tx.commit().await?;
    tracing::info!(job_id=%id, "Private VOD range queued");
    Ok(id)
}

#[derive(Clone)]
pub struct PrivateRangePaths {
    pub directory: PathBuf,
    pub yt_dlp: PathBuf,
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
}

impl PrivateRangePaths {
    pub fn file(&self, id: &str) -> Option<PathBuf> {
        let uuid = uuid::Uuid::parse_str(id).ok()?;
        if uuid.to_string() != id {
            return None;
        }
        Some(self.directory.join(format!("{id}.mp4")))
    }

    pub async fn prepare(&self) -> Result<(), RangeError> {
        tokio::fs::create_dir_all(&self.directory).await?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            tokio::fs::set_permissions(&self.directory, std::fs::Permissions::from_mode(0o700))
                .await?;
        }
        Ok(())
    }
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct RangeJob {
    pub id: String,
    pub owner_id: String,
    pub video_id: String,
    pub start_seconds: i32,
    pub end_seconds: i32,
    pub status: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub file_bytes: Option<i64>,
    pub error_code: Option<String>,
}

pub async fn cleanup(pool: &PgPool, paths: &PrivateRangePaths) -> Result<(), RangeError> {
    sqlx::query("UPDATE twitch_private_vod_jobs SET status='expired' WHERE expires_at<=now() AND status<>'expired'").execute(pool).await?;
    let expired: Vec<String> =
        sqlx::query_scalar("SELECT id FROM twitch_private_vod_jobs WHERE status='expired'")
            .fetch_all(pool)
            .await?;
    let expired_count = expired.len();
    for id in expired {
        if let Some(path) = paths.file(&id) {
            for file in [path.clone(), path.with_extension("part.mp4")] {
                remove_if_present(&file).await?;
            }
            sqlx::query("DELETE FROM twitch_private_vod_jobs WHERE id=$1 AND status='expired'")
                .bind(&id)
                .execute(pool)
                .await?;
        }
    }
    let mut entries = tokio::fs::read_dir(&paths.directory).await?;
    while let Some(entry) = entries.next_entry().await? {
        let name = entry.file_name().to_string_lossy().into_owned();
        let id = name
            .strip_suffix(".part.mp4")
            .or_else(|| name.strip_suffix(".mp4"));
        let Some(id) = id.filter(|id| paths.file(id).is_some()) else {
            continue;
        };
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM twitch_private_vod_jobs WHERE id=$1 AND status IN ('ready','running') AND expires_at>now())").bind(id).fetch_one(pool).await?;
        if !exists {
            remove_if_present(&entry.path()).await?;
        }
    }
    tracing::info!(expired_jobs = expired_count, "Private VOD cleanup complete");
    Ok(())
}

async fn remove_if_present(path: &Path) -> Result<(), std::io::Error> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

pub async fn run_worker(
    pool: PgPool,
    helix: Option<HelixClient>,
    paths: PrivateRangePaths,
) -> Result<(), RangeError> {
    paths.prepare().await?;
    let mut lock = pool.acquire().await?.detach();
    let acquired: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock($1)")
        .bind(WORKER_LOCK)
        .fetch_one(&mut lock)
        .await?;
    if !acquired {
        return Err(RangeError::Rejected("worker_locked"));
    }
    sqlx::query("UPDATE twitch_private_vod_jobs SET status='failed', error_code='interrupted' WHERE status='running'").execute(&pool).await?;
    cleanup(&pool, &paths).await?;
    tracing::info!("Private VOD range worker ready");
    let mut tick = tokio::time::interval(Duration::from_secs(60));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tick.tick().await;
        sqlx::query("SELECT 1").execute(&mut lock).await?;
        cleanup(&pool, &paths).await?;
        let row = sqlx::query("UPDATE twitch_private_vod_jobs SET status='running', started_at=now() WHERE id=(SELECT id FROM twitch_private_vod_jobs WHERE status='queued' AND expires_at>now() ORDER BY created_at LIMIT 1) RETURNING id,owner_id,video_id,start_seconds,end_seconds,expires_at")
            .fetch_optional(&pool).await?;
        let Some(row) = row else {
            continue;
        };
        let id: String = row.try_get("id")?;
        let owner: String = row.try_get("owner_id")?;
        let video: String = row.try_get("video_id")?;
        let range = VodRange {
            start: row.try_get::<i32, _>("start_seconds")? as u32,
            end: row.try_get::<i32, _>("end_seconds")? as u32,
        };
        let expires_at = row.try_get("expires_at")?;
        let budget = processing_budget(expires_at, chrono::Utc::now());
        let timeout_code = if budget < PROCESS_TIMEOUT {
            "expired"
        } else {
            "timeout"
        };
        let deadline = tokio::time::Instant::now() + budget;
        let result = {
            let future = tokio::time::timeout_at(
                deadline,
                process(&paths, helix.as_ref(), &owner, &video, range, &id),
            );
            tokio::pin!(future);
            loop {
                tokio::select! {
                    result = &mut future => break result.unwrap_or(Err(RangeError::Rejected(timeout_code))),
                    _ = tick.tick() => {
                        let maintenance = async {
                            sqlx::query("SELECT 1").execute(&mut lock).await?;
                            cleanup(&pool, &paths).await
                        };
                        let maintenance_deadline = deadline.min(tokio::time::Instant::now() + Duration::from_secs(10));
                        tokio::time::timeout_at(maintenance_deadline, maintenance).await
                            .map_err(|_| RangeError::Rejected("maintenance_timeout"))??;
                    }
                }
            }
        };
        match result {
            Ok(bytes) => {
                let updated = sqlx::query("UPDATE twitch_private_vod_jobs SET status='ready',file_bytes=$2 WHERE id=$1 AND status='running' AND expires_at>now()")
                    .bind(&id).bind(bytes as i64).execute(&pool).await?;
                if updated.rows_affected() == 0 {
                    if let Some(path) = paths.file(&id) {
                        remove_if_present(&path).await?;
                    }
                } else {
                    tracing::info!(job_id=%id, bytes, "Private VOD range ready");
                }
            }
            Err(error) => {
                if let Some(path) = paths.file(&id) {
                    for file in [path.clone(), path.with_extension("part.mp4")] {
                        remove_if_present(&file).await?;
                    }
                }
                sqlx::query("UPDATE twitch_private_vod_jobs SET status='failed',error_code=$2 WHERE id=$1 AND status='running'").bind(&id).bind(error.code()).execute(&pool).await?;
                tracing::warn!(job_id=%id, code=error.code(), "Private VOD range failed");
            }
        }
    }
}

fn processing_budget(
    expires_at: chrono::DateTime<chrono::Utc>,
    now: chrono::DateTime<chrono::Utc>,
) -> Duration {
    (expires_at - now)
        .to_std()
        .unwrap_or_default()
        .min(PROCESS_TIMEOUT)
}

async fn process(
    paths: &PrivateRangePaths,
    helix: Option<&HelixClient>,
    owner: &str,
    video_id: &str,
    range: VodRange,
    id: &str,
) -> Result<u64, RangeError> {
    let helix = helix.ok_or(RangeError::Rejected("unavailable"))?;
    let video = helix
        .archive_video(video_id)
        .await
        .map_err(|_| RangeError::Rejected("unavailable"))?
        .ok_or(RangeError::Rejected("unavailable"))?;
    validate_source(&video, owner, range)?;
    let url = tool_output(
        &paths.yt_dlp,
        &[
            "--ignore-config".into(),
            "--no-cache-dir".into(),
            "--no-playlist".into(),
            "--no-warnings".into(),
            "--socket-timeout".into(),
            "20".into(),
            "--retries".into(),
            "1".into(),
            "-f".into(),
            "best[height<=720]/worst".into(),
            "--get-url".into(),
            crate::twitch::vod_url(video_id),
        ],
        Duration::from_secs(60),
    )
    .await?;
    let url = url.trim();
    if !trusted_media_url(url) {
        return Err(RangeError::Rejected("media_source"));
    }
    extract(paths, url, range, id).await
}

pub fn trusted_media_url(raw: &str) -> bool {
    if raw
        .bytes()
        .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
    {
        return false;
    }
    url::Url::parse(raw).is_ok_and(|url| {
        url.scheme() == "https"
            && url.username().is_empty()
            && url.password().is_none()
            && url.port().is_none_or(|port| port == 443)
            && url.host_str().is_some_and(|host| {
                host.ends_with(".ttvnw.net")
                    || host.ends_with(".twitch.tv")
                    || TWITCH_CLOUDFRONT_HOSTS.contains(&host)
            })
    })
}

async fn tool_output(
    program: &Path,
    args: &[String],
    timeout: Duration,
) -> Result<String, RangeError> {
    let mut child = tokio::process::Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or(RangeError::Rejected("media_io"))?;
    tokio::time::timeout(timeout, async {
        let mut data = Vec::new();
        stdout.take(8193).read_to_end(&mut data).await?;
        if data.len() > 8192 {
            return Err(RangeError::Rejected("media_output"));
        }
        if !child.wait().await?.success() {
            return Err(RangeError::Rejected("media_tool"));
        }
        String::from_utf8(data).map_err(|_| RangeError::Rejected("media_output"))
    })
    .await
    .map_err(|_| RangeError::Rejected("timeout"))?
}

pub async fn extract(
    paths: &PrivateRangePaths,
    source: &str,
    range: VodRange,
    id: &str,
) -> Result<u64, RangeError> {
    let output = paths.file(id).ok_or(RangeError::Rejected("media_path"))?;
    let partial = output.with_extension("part.mp4");
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    options.open(&partial).await?;
    let mut command = tokio::process::Command::new(&paths.ffmpeg);
    command.args([
        "-nostdin",
        "-hide_banner",
        "-loglevel",
        "error",
        "-y",
        "-threads",
        "2",
    ]);
    if source.starts_with("https://") {
        command.args([
            "-protocol_whitelist",
            "https,tls,tcp,crypto",
            "-rw_timeout",
            "20000000",
        ]);
    }
    command.args([
        "-ss",
        &range.start.to_string(),
        "-i",
        source,
        "-t",
        &(range.end - range.start).to_string(),
        "-map",
        "0:v:0",
        "-map",
        "0:a:0?",
        "-vf",
        "scale=w=1280:h=720:force_original_aspect_ratio=decrease:force_divisible_by=2",
        "-filter_threads",
        "1",
        "-c:v",
        "libx264",
        "-preset",
        "ultrafast",
        "-threads",
        "2",
        "-b:v",
        "600k",
        "-maxrate",
        "700k",
        "-bufsize",
        "1400k",
        "-c:a",
        "aac",
        "-b:a",
        "96k",
        "-fs",
        &MAX_FILE_BYTES.to_string(),
        "-movflags",
        "+faststart",
    ]);
    let status = command
        .arg(&partial)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()?
        .wait()
        .await?;
    if !status.success() {
        return Err(RangeError::Rejected("media_tool"));
    }
    let metadata = tokio::fs::metadata(&partial).await?;
    if metadata.len() == 0 || metadata.len() >= MAX_FILE_BYTES {
        return Err(RangeError::Rejected("size"));
    }
    let measured = tool_output(
        &paths.ffprobe,
        &[
            "-v".into(),
            "error".into(),
            "-show_entries".into(),
            "format=duration".into(),
            "-of".into(),
            "default=nw=1:nk=1".into(),
            partial.to_string_lossy().into_owned(),
        ],
        Duration::from_secs(15),
    )
    .await?;
    let measured: f64 = measured
        .trim()
        .parse()
        .map_err(|_| RangeError::Rejected("duration"))?;
    if !measured.is_finite() || (measured - f64::from(range.end - range.start)).abs() > 1.0 {
        return Err(RangeError::Rejected("duration"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&partial, std::fs::Permissions::from_mode(0o600)).await?;
    }
    tokio::fs::rename(&partial, &output).await?;
    Ok(metadata.len())
}

#[cfg(test)]
#[tokio::test]
async fn extractor_output_accepts_only_verified_twitch_cloudfront_hosts() {
    let fixture = include_str!("../tests/fixtures/twitch-cloudfront-get-url.txt");
    let output = tool_output(
        Path::new("/usr/bin/printf"),
        &["%s".into(), fixture.into()],
        Duration::from_secs(5),
    )
    .await
    .unwrap();
    assert_eq!(output, fixture);
    assert!(trusted_media_url(output.trim()));
    for host in TWITCH_CLOUDFRONT_HOSTS {
        assert!(trusted_media_url(&format!(
            "https://{host}/fixture/720p30/index-dvr.m3u8"
        )));
    }
    assert!(trusted_media_url(
        "https://d3stzm2eumvgb4.cloudfront.net:443/fixture/index-dvr.m3u8"
    ));
    for source in [
        "https://arbitrary.cloudfront.net/fixture/index-dvr.m3u8",
        "https://cloudfront.net/fixture/index-dvr.m3u8",
        "https://evil.d3stzm2eumvgb4.cloudfront.net/fixture/index-dvr.m3u8",
        "https://d3stzm2eumvgb4.cloudfront.net.attacker.test/fixture/index-dvr.m3u8",
        "https://d3stzm2eumvgb4.cloudfront.net@attacker.test/fixture/index-dvr.m3u8",
        "https://user@d3stzm2eumvgb4.cloudfront.net/fixture/index-dvr.m3u8",
        "https://user:password@d3stzm2eumvgb4.cloudfront.net/fixture/index-dvr.m3u8",
        "http://d3stzm2eumvgb4.cloudfront.net/fixture/index-dvr.m3u8",
        "https://d3stzm2eumvgb4.cloudfront.net:8443/fixture/index-dvr.m3u8",
        "https://d3stzm2eumvgb4.cloudfront.net/one.m3u8\nhttps://attacker.test/two.m3u8",
    ] {
        assert!(!trusted_media_url(source));
    }
}

#[cfg(test)]
#[tokio::test]
async fn extractor_output_stays_bounded() {
    assert!(matches!(
        tool_output(
            Path::new("/usr/bin/printf"),
            &["%s".into(), "x".repeat(8193)],
            Duration::from_secs(5)
        )
        .await,
        Err(RangeError::Rejected("media_output"))
    ));
}

#[cfg(test)]
#[test]
fn processing_deadline_cannot_outlive_retention() {
    let now = chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap();
    assert_eq!(processing_budget(now, now), Duration::ZERO);
    assert_eq!(
        processing_budget(now - chrono::Duration::seconds(1), now),
        Duration::ZERO
    );
    assert_eq!(
        processing_budget(now + chrono::Duration::seconds(30), now),
        Duration::from_secs(30)
    );
    assert_eq!(
        processing_budget(now + chrono::Duration::hours(24), now),
        PROCESS_TIMEOUT
    );
}
