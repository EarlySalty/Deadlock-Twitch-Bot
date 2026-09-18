//! App credentials from one protected JSON file; collection policy from PG.
//! No environment-variable configuration and no authenticated chat identity.
use serde::Deserialize;
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    ConnectOptions,
};
include!(concat!(env!("OUT_DIR"), "/build_revision.rs"));

use std::{os::unix::fs::PermissionsExt, path::Path, str::FromStr, time::Duration};
use tb_category_collector::collector::SammlerDienst;
use tb_transport_twitch::{HelixClient, HelixConfig};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Credentials {
    dsn: String,
    client_id: String,
    client_secret: String,
}
fn parse_document(bytes: &[u8]) -> Result<Credentials, &'static str> {
    let data: Credentials =
        serde_json::from_slice(bytes).map_err(|_| "Invalid collector credential document")?;
    if data.dsn.trim().is_empty()
        || data.client_id.trim().is_empty()
        || data.client_secret.trim().is_empty()
    {
        return Err("Collector credentials are incomplete");
    }
    Ok(data)
}
fn read_credentials(path: &Path) -> Result<Credentials, &'static str> {
    let meta =
        std::fs::symlink_metadata(path).map_err(|_| "Collector credential file cannot be read")?;
    if !meta.is_file() || meta.permissions().mode() & 0o077 != 0 || meta.len() > 16_384 {
        return Err(
            "Collector credentials require a regular private file (0600 or 0400, at most 16 KiB)",
        );
    }
    let bytes = std::fs::read(path).map_err(|_| "Collector credential file cannot be read")?;
    parse_document(&bytes)
}
async fn start() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let mut path = None;
    let mut duration = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" if path.is_none()=>{path=Some(args.next().ok_or("--config requires a file")?);},
            "--duration-seconds" if duration.is_none()=>{
                let seconds:u64=args.next().ok_or("Duration missing")?.parse().map_err(|_|"Invalid smoke duration")?;
                if !(10..=3600).contains(&seconds) {return Err("Smoke duration must be 10..3600 seconds".into());}
                duration=Some(Duration::from_secs(seconds));
            },
            _=>return Err("Usage: tb-category-collector --config /protected/credentials.json [--duration-seconds 90]".into()),
        }
    }
    let path = path.ok_or("An explicit protected --config file is required")?;
    let cfg = read_credentials(Path::new(&path))?;
    let options = PgConnectOptions::from_str(&cfg.dsn)
        .map_err(|_| "Invalid collector database configuration")?
        .application_name("tb-category-collector")
        .options([
            ("statement_timeout", "15000"),
            ("lock_timeout", "10000"),
            ("timezone", "UTC"),
        ])
        .disable_statement_logging();
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .acquire_timeout(Duration::from_secs(10))
        .connect_with(options)
        .await
        .map_err(|_| "Collector database connection failed")?;
    let helix = HelixClient::new(HelixConfig::new(cfg.client_id, cfg.client_secret))
        .map_err(|_| "Collector HTTP initialization failed")?;
    let service=SammlerDienst::new(pool.clone(),helix).await.map_err(|_|"Collector startup failed: check migration, database grants, configuration and singleton lock")?;
    tracing::info!("category collector started: app-only Helix, anonymous read-only chat, mandatory PostgreSQL retention");
    let result = service.run(duration).await;
    pool.close().await;
    result
}
#[tokio::main(worker_threads = 2)]
async fn main() -> std::process::ExitCode {
    if print_build_revision() {
        return std::process::ExitCode::SUCCESS;
    }
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .init();
    match start().await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(message) => {
            tracing::error!(error = message, "category collector stopped");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bot_token_is_not_an_accepted_configuration_field() {
        assert!(parse_document(br#"{"dsn":"postgres://fixture","client_id":"x","client_secret":"y","bot_token":"forbidden"}"#).is_err());
    }
    #[test]
    fn complete_credentials_and_no_guessed_defaults() {
        assert!(parse_document(
            br#"{"dsn":"postgres://fixture","client_id":"x","client_secret":"y"}"#
        )
        .is_ok());
        assert!(parse_document(br#"{"dsn":"postgres://fixture","client_id":"x"}"#).is_err());
        assert!(parse_document(br#"{"dsn":"","client_id":"x","client_secret":"y"}"#).is_err());
    }
    #[test]
    fn group_or_world_readable_credentials_are_rejected() {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(
            file.path(),
            br#"{"dsn":"postgres://fixture","client_id":"x","client_secret":"y"}"#,
        )
        .unwrap();
        std::fs::set_permissions(file.path(), std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(read_credentials(file.path()).is_ok());
        std::fs::set_permissions(file.path(), std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(read_credentials(file.path()).is_err());
    }
}
