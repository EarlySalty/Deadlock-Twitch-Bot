//! Eine private Infisical-Momentaufnahme vor dem Start von Clients und Aufgaben.
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Mutex, OnceLock},
};
use zeroize::Zeroizing;

static VALUES: OnceLock<BTreeMap<String, Zeroizing<String>>> = OnceLock::new();
static START: Mutex<()> = Mutex::new(());
static SOURCE: OnceLock<Source> = OnceLock::new();

#[derive(serde::Deserialize)]
struct Source {
    project_id: String,
    environment: String,
    secret_path: String,
    socket_path: std::path::PathBuf,
}

pub fn load(config_source: &Path) -> Result<(), &'static str> {
    let _guard = START
        .lock()
        .map_err(|_| "Private Startgrenze ist nicht verfügbar.")?;
    if VALUES.get().is_some() {
        return Err("Private Momentaufnahme wurde bereits geladen.");
    }
    let source_path = config_source.with_file_name("infisical.json");
    let metadata = std::fs::read(&source_path)
        .map_err(|_| "Private Infisical-Metadaten sind nicht lesbar.")?;
    if metadata.len() > 65536 {
        return Err("Private Infisical-Metadaten sind zu groß.");
    }
    let source = serde_json::from_slice(&metadata)
        .map_err(|_| "Private Infisical-Metadaten sind ungültig.")?;
    let values = dl_token_secrets::private_values(&source_path)
        .map_err(|_| "Private Infisical-Momentaufnahme konnte nicht geladen werden.")?;
    SOURCE
        .set(source)
        .map_err(|_| "Private Momentaufnahme wurde bereits geladen.")?;
    VALUES
        .set(values.into_iter().collect())
        .map_err(|_| "Private Momentaufnahme wurde bereits geladen.")
}

pub fn matches_source(project: &str, environment: &str, path: &str, socket: &Path) -> bool {
    SOURCE.get().is_some_and(|source| {
        source.project_id == project
            && source.environment == environment
            && source.secret_path == path
            && source.socket_path == socket
    })
}

pub fn values() -> Result<impl Iterator<Item = (&'static str, &'static str)>, &'static str> {
    Ok(VALUES
        .get()
        .ok_or("Private Momentaufnahme fehlt.")?
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str())))
}

pub fn value(name: &str) -> Option<&'static str> {
    VALUES
        .get()?
        .get(name)
        .map(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
}

/// Fehlende Werte bleiben fehlend; es gibt keine zweite Quelle oder ENV-Rückfall.
pub fn secret(name: &str) -> Result<String, &'static str> {
    value(name)
        .map(str::to_owned)
        .ok_or("Privater Dienstzugang fehlt.")
}

/// Dienstbezogene DSNs dürfen nicht von einer gemeinsamen älteren DSN verdrängt werden.
pub fn service_secret(name: &str, role: &str) -> Option<String> {
    let name = if name == "TWITCH_ANALYTICS_DSN" {
        match role {
            "bot" => "TWITCH_BOT_ANALYTICS_DSN",
            "dashboard" => "TWITCH_DASHBOARD_ANALYTICS_DSN",
            _ => return None,
        }
    } else {
        name
    };
    value(name).map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::Write,
        process::{Command, Stdio},
    };

    #[test]
    fn private_snapshot_reads_fifo_once_and_role_dsns_fail_closed() {
        let directory = tempfile::tempdir().expect("private FIFO fixture");
        std::fs::write(directory.path().join("infisical.json"),
            br#"{"secret_values_fd":3,"project_id":"fixture","environment":"fixture","secret_path":"/","socket_path":"/nonexistent","database_secret":"TWITCH_ANALYTICS_DSN"}"#)
            .expect("private FIFO fixture");
        let mut child = Command::new("/bin/sh")
            .args(["-c", "exec 3<&0; exec \"$@\"", "private-fifo-test"])
            .arg(std::env::current_exe().expect("private FIFO fixture"))
            .args([
                "--ignored",
                "--exact",
                "private::tests::child_fifo_contract",
            ])
            .current_dir(directory.path())
            .stdin(Stdio::piped())
            .spawn()
            .expect("private FIFO fixture");
        child.stdin.take().expect("private FIFO fixture").write_all(
            br#"{"TWITCH_BOT_ANALYTICS_DSN":"synthetic-bot","TWITCH_ANALYTICS_DSN":"wrong-shared","EMPTY":"  "}"#)
            .expect("private FIFO fixture");
        assert!(child.wait().expect("private FIFO fixture").success());
    }

    #[test]
    #[ignore = "isolierter Kindprozess mit privater FIFO"]
    fn child_fifo_contract() {
        let path = std::env::current_dir()
            .expect("private FIFO fixture")
            .join("bot.toml");
        load(&path).expect("private FIFO fixture");
        assert_eq!(
            service_secret("TWITCH_ANALYTICS_DSN", "bot").as_deref(),
            Some("synthetic-bot")
        );
        assert!(service_secret("TWITCH_ANALYTICS_DSN", "dashboard").is_none());
        assert!(secret("EMPTY").is_err());
        assert!(secret("MISSING").is_err());
        assert!(load(&path).is_err());
    }
}
