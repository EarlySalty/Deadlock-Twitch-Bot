use std::{path::Path, process::Stdio};

use async_trait::async_trait;
use thiserror::Error;

use crate::{config::FFMPEG_PATH, twitch_vod::TwitchVodApi};

pub const TARGET_LOGIN: &str = "dach_lock";
pub const LINK_EXPIRY: &str = "7d";

pub fn should_export(login: Option<&str>) -> bool {
    login == Some(TARGET_LOGIN)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

#[async_trait]
pub trait CommandRunner: Send + Sync {
    async fn run(&self, program: &Path, args: &[String]) -> std::io::Result<CommandOutput>;
}

pub struct TokioCommandRunner;

#[async_trait]
impl CommandRunner for TokioCommandRunner {
    async fn run(&self, program: &Path, args: &[String]) -> std::io::Result<CommandOutput> {
        let output = tokio::process::Command::new(program)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await?;
        Ok(CommandOutput {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

#[derive(Debug, Error)]
pub enum VodExportError {
    #[error("kein Archiv-VOD gefunden")]
    NoVod,
    #[error("ungueltige Twitch-VOD-ID: {0}")]
    InvalidVodId(String),
    #[error("lokales Export-Verzeichnis konnte nicht erstellt werden: {0}")]
    CreateDirectory(#[source] std::io::Error),
    #[error("{program} konnte nicht gestartet werden: {source}")]
    StartCommand {
        program: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{program} fehlgeschlagen: {stderr}")]
    CommandFailed { program: String, stderr: String },
    #[error("yt-dlp meldete Erfolg, aber die Ausgabedatei fehlt")]
    MissingDownload,
    #[error("lokale VOD-Datei konnte nicht geloescht werden: {0}")]
    Cleanup(#[source] std::io::Error),
    #[error("rclone lieferte keinen Freigabelink")]
    MissingLink,
}

pub async fn export_latest_vod(
    api: &dyn TwitchVodApi,
    runner: &dyn CommandRunner,
    yt_dlp_path: &Path,
    rclone_path: &Path,
    bucket: &str,
    temp_dir: &Path,
    channel_id: &str,
) -> Result<String, VodExportError> {
    let vods = api.get_archive_videos(channel_id, 1).await;
    let vod_id = vods
        .first()
        .and_then(|vod| vod.get("id"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .ok_or(VodExportError::NoVod)?;
    if !vod_id.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(VodExportError::InvalidVodId(vod_id.to_string()));
    }

    std::fs::create_dir_all(temp_dir).map_err(VodExportError::CreateDirectory)?;
    let local_path = temp_dir.join(format!("{vod_id}.mp4"));
    cleanup_download_files(temp_dir, vod_id);

    if let Err(error) = run_checked(runner, yt_dlp_path, &yt_dlp_args(vod_id, &local_path)).await {
        cleanup_download_files(temp_dir, vod_id);
        return Err(error);
    }
    if !local_path.is_file() {
        cleanup_download_files(temp_dir, vod_id);
        return Err(VodExportError::MissingDownload);
    }

    let upload_result = run_checked(
        runner,
        rclone_path,
        &rclone_copy_args(&local_path, bucket, vod_id),
    )
    .await;
    let cleanup_result = std::fs::remove_file(&local_path);
    if let Err(error) = &cleanup_result {
        tracing::warn!(
            %error,
            path = %local_path.display(),
            "VOD-Export: lokale Datei konnte nicht geloescht werden"
        );
    }
    upload_result?;
    cleanup_result.map_err(VodExportError::Cleanup)?;

    let link_output = run_checked(runner, rclone_path, &rclone_link_args(bucket, vod_id)).await?;
    link_output
        .stdout
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
        .ok_or(VodExportError::MissingLink)
}

fn yt_dlp_args(vod_id: &str, output_path: &Path) -> Vec<String> {
    vec![
        "--ffmpeg-location".to_string(),
        FFMPEG_PATH.to_string(),
        "--merge-output-format".to_string(),
        "mp4".to_string(),
        "-f".to_string(),
        "bestvideo+bestaudio/best".to_string(),
        "-o".to_string(),
        output_path.to_string_lossy().into_owned(),
        format!("https://www.twitch.tv/videos/{vod_id}"),
    ]
}

fn storj_object_path(bucket: &str, vod_id: &str) -> String {
    format!("storj:{bucket}/vod-export/{TARGET_LOGIN}/{vod_id}.mp4")
}

fn rclone_copy_args(local_path: &Path, bucket: &str, vod_id: &str) -> Vec<String> {
    vec![
        "copy".to_string(),
        local_path.to_string_lossy().into_owned(),
        storj_object_path(bucket, vod_id),
    ]
}

fn rclone_link_args(bucket: &str, vod_id: &str) -> Vec<String> {
    vec![
        "link".to_string(),
        "--expire".to_string(),
        LINK_EXPIRY.to_string(),
        storj_object_path(bucket, vod_id),
    ]
}

async fn run_checked(
    runner: &dyn CommandRunner,
    program: &Path,
    args: &[String],
) -> Result<CommandOutput, VodExportError> {
    let program_name = program.display().to_string();
    let output =
        runner
            .run(program, args)
            .await
            .map_err(|source| VodExportError::StartCommand {
                program: program_name.clone(),
                source,
            })?;
    if output.success {
        Ok(output)
    } else {
        Err(VodExportError::CommandFailed {
            program: program_name,
            stderr: output.stderr.trim().to_string(),
        })
    }
}

fn cleanup_download_files(temp_dir: &Path, vod_id: &str) {
    let prefix = format!("{vod_id}.");
    let Ok(entries) = std::fs::read_dir(temp_dir) else {
        return;
    };
    for entry in entries.flatten() {
        if entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(&prefix))
        {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        path::Path,
        sync::{Arc, Mutex},
    };

    use async_trait::async_trait;
    use serde_json::json;

    use super::*;
    use crate::twitch_vod::TwitchVodApi;

    #[test]
    fn trigger_nur_fuer_dach_lock() {
        assert!(should_export(Some("dach_lock")));
        assert!(!should_export(Some("DACH_LOCK")));
        assert!(!should_export(Some("anderer_kanal")));
        assert!(!should_export(None));
    }

    #[test]
    fn storj_pfad_und_rclone_befehle_sind_korrekt() {
        let local = Path::new("/tmp/987.mp4");

        assert_eq!(
            storj_object_path("server-backup", "987"),
            "storj:server-backup/vod-export/dach_lock/987.mp4"
        );
        assert_eq!(
            rclone_copy_args(local, "server-backup", "987"),
            vec![
                "copy",
                "/tmp/987.mp4",
                "storj:server-backup/vod-export/dach_lock/987.mp4",
            ]
        );
        assert_eq!(
            rclone_link_args("server-backup", "987"),
            vec![
                "link",
                "--expire",
                "7d",
                "storj:server-backup/vod-export/dach_lock/987.mp4",
            ]
        );
    }

    struct MockApi;

    #[async_trait]
    impl TwitchVodApi for MockApi {
        async fn get_user_info(&self, _login: &str) -> Option<serde_json::Value> {
            None
        }

        async fn get_archive_videos(
            &self,
            _channel_id: &str,
            first: u32,
        ) -> Vec<serde_json::Value> {
            assert_eq!(first, 1);
            vec![json!({ "id": "987" })]
        }
    }

    #[derive(Default)]
    struct MockRunner {
        calls: Mutex<Vec<(String, Vec<String>)>>,
        fail_download: bool,
    }

    #[async_trait]
    impl CommandRunner for MockRunner {
        async fn run(&self, program: &Path, args: &[String]) -> std::io::Result<CommandOutput> {
            self.calls
                .lock()
                .expect("call lock")
                .push((program.display().to_string(), args.to_vec()));
            if program == Path::new("/opt/yt-dlp") {
                let output_index = args.iter().position(|arg| arg == "-o").expect("-o");
                let output_path = &args[output_index + 1];
                if self.fail_download {
                    std::fs::write(format!("{output_path}.part"), b"partial")?;
                } else {
                    std::fs::write(output_path, b"vod")?;
                }
            }
            Ok(CommandOutput {
                success: !(self.fail_download && program == Path::new("/opt/yt-dlp")),
                stdout: if args.first().map(String::as_str) == Some("link") {
                    "notice\nhttps://share.example/987\n".to_string()
                } else {
                    String::new()
                },
                stderr: String::new(),
            })
        }
    }

    #[tokio::test]
    async fn export_nutzt_gemockte_subprozesse_und_loescht_lokale_datei() {
        let runner = Arc::new(MockRunner::default());
        let temp_dir = std::env::temp_dir().join(format!(
            "tb-vod-export-test-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("unnamed")
        ));
        std::fs::create_dir_all(&temp_dir).expect("temp dir");

        let link = export_latest_vod(
            &MockApi,
            runner.as_ref(),
            Path::new("/opt/yt-dlp"),
            Path::new("rclone"),
            "server-backup",
            &temp_dir,
            "channel-1",
        )
        .await
        .expect("export succeeds");

        assert_eq!(link, "https://share.example/987");
        assert!(!temp_dir.join("987.mp4").exists());
        let calls = runner.calls.lock().expect("call lock");
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[0].0, "/opt/yt-dlp");
        assert!(!calls[0].1.iter().any(|arg| arg == "--download-sections"));
        assert_eq!(
            calls[1].1,
            rclone_copy_args(&temp_dir.join("987.mp4"), "server-backup", "987")
        );
        assert_eq!(calls[2].1, rclone_link_args("server-backup", "987"));

        std::fs::remove_dir_all(temp_dir).expect("temp cleanup");
    }

    #[tokio::test]
    async fn fehlgeschlagener_download_loescht_part_datei() {
        let runner = MockRunner {
            fail_download: true,
            ..MockRunner::default()
        };
        let temp_dir =
            std::env::temp_dir().join(format!("tb-vod-export-failure-test-{}", std::process::id()));
        std::fs::create_dir_all(&temp_dir).expect("temp dir");

        let result = export_latest_vod(
            &MockApi,
            &runner,
            Path::new("/opt/yt-dlp"),
            Path::new("rclone"),
            "server-backup",
            &temp_dir,
            "channel-1",
        )
        .await;

        assert!(matches!(result, Err(VodExportError::CommandFailed { .. })));
        assert!(!temp_dir.join("987.mp4.part").exists());
        std::fs::remove_dir_all(temp_dir).expect("temp cleanup");
    }
}
