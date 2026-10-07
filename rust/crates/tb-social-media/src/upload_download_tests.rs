use super::*;
use wiremock::{matchers::path, Mock, MockServer, ResponseTemplate};

#[path = "../../../test-support/postgres.rs"]
mod postgres;

#[tokio::test]
async fn upload_download_uses_real_ytdlp_and_registers_cached_file() {
    let database = postgres::TestPostgres::start().await;
    sqlx::query("CREATE TABLE twitch_clips_social_media (id BIGINT PRIMARY KEY, local_file_path TEXT, downloaded_at TIMESTAMPTZ, download_failed_at TIMESTAMPTZ)")
        .execute(&database.pool).await.unwrap();
    sqlx::query(
        "INSERT INTO twitch_clips_social_media (id, download_failed_at) VALUES (42, NOW())",
    )
    .execute(&database.pool)
    .await
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let fixture = dir.path().join("fixture.mp4");
    let output = tokio::process::Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=black:s=64x64:d=0.2",
            "-c:v",
            "libx264",
            "-y",
        ])
        .arg(&fixture)
        .output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let bytes = tokio::fs::read(&fixture).await.unwrap();
    let server = MockServer::start().await;
    Mock::given(path("/clip.mp4"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(bytes.clone())
                .insert_header("Content-Type", "video/mp4"),
        )
        .mount(&server)
        .await;
    let task = UploadTask {
        pool: database.pool.clone(),
        video_processor: VideoProcessor::default(),
        yt_dlp_path: "yt-dlp".into(),
        clips_dir: dir.path().join("downloads").to_string_lossy().into_owned(),
    };
    let local = task
        .download_clip(&format!("{}/clip.mp4", server.uri()), 42)
        .await
        .unwrap();
    assert_eq!(tokio::fs::read(&local).await.unwrap(), bytes);
    let files = std::fs::read_dir(&task.clips_dir).unwrap().count();
    assert_eq!(files, 1);
    let (registered, downloaded, failed): (Option<String>, Option<chrono::DateTime<Utc>>, Option<chrono::DateTime<Utc>>) =
        sqlx::query_as("SELECT local_file_path, downloaded_at, download_failed_at FROM twitch_clips_social_media WHERE id = 42")
            .fetch_one(&database.pool).await.unwrap();
    assert_eq!(registered.as_deref(), Some(local.as_str()));
    assert!(downloaded.is_some());
    assert!(failed.is_none());
    sqlx::query("UPDATE twitch_clips_social_media SET local_file_path = NULL, downloaded_at = NULL, download_failed_at = NOW() WHERE id = 42")
        .execute(&database.pool).await.unwrap();
    drop(server);
    assert_eq!(task.download_clip("", 42).await.unwrap(), local);
    let cached: (Option<String>, Option<chrono::DateTime<Utc>>) = sqlx::query_as(
        "SELECT local_file_path, download_failed_at FROM twitch_clips_social_media WHERE id = 42",
    )
    .fetch_one(&database.pool)
    .await
    .unwrap();
    assert_eq!(cached.0.as_deref(), Some(local.as_str()));
    assert!(cached.1.is_none());
}

#[tokio::test]
async fn upload_download_does_not_hide_registration_failure() {
    let database = postgres::TestPostgres::start().await;
    let dir = tempfile::tempdir().unwrap();
    tokio::fs::write(dir.path().join("42.mp4"), b"cached")
        .await
        .unwrap();
    let task = UploadTask {
        pool: database.pool.clone(),
        video_processor: VideoProcessor::default(),
        yt_dlp_path: "unused".into(),
        clips_dir: dir.path().to_string_lossy().into_owned(),
    };
    assert!(matches!(
        task.download_clip("", 42).await,
        Err(WorkerError::Db(_))
    ));
}
