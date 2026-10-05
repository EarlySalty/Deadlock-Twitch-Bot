//! Liefert ausschließlich strukturierte Verbrauchszähler aus dem Systemjournal nach.
use serde::Deserialize;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use std::{
    io::{self, BufRead},
    path::PathBuf,
    time::Duration,
};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    socket: PathBuf,
    database: String,
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 2 {
        return Err("Normale Konfigurationsdatei fehlt".into());
    }
    let config: Config = toml::from_str(&std::fs::read_to_string(&args[1])?)?;
    if !config.socket.is_absolute() || config.database != "twitch_analytics" {
        return Err("Ungültiger Ledger-Peervertrag".into());
    }
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(10))
        .connect_with(
            PgConnectOptions::new()
                .host(config.socket.to_str().ok_or("Socketpfad ungültig")?)
                .database(&config.database)
                .username("twitchbot"),
        )
        .await?;
    let mut failed = 0;
    for line in io::stdin().lock().lines() {
        let entry: serde_json::Value = serde_json::from_str(&line?)?;
        let Some(raw_message) = entry["MESSAGE"].as_str() else {
            continue;
        };
        let message = strip_ansi(raw_message);
        let Some(start) = message.find("recovery=") else {
            continue;
        };
        let raw = &message[start + 9..];
        let Some(Ok(record)) = serde_json::Deserializer::from_str(raw)
            .into_iter::<tb_llm::ledger::Recovery>()
            .next()
        else {
            failed += 1;
            continue;
        };
        if record.project != "Deadlock-Twitch-Bot"
            || !matches!(
                record.service.as_str(),
                "deadlock-twitch-bot-rust"
                    | "deadlock-twitch-stream-coaching-watch"
                    | "deadlock-twitch-dashboard-rust"
                    | "deadlock-llm-model-resolver"
            )
        {
            continue;
        }
        let unit = if record.service == "deadlock-llm-model-resolver" {
            entry["_SYSTEMD_USER_UNIT"].as_str()
        } else {
            entry["_SYSTEMD_UNIT"].as_str()
        }
        .unwrap_or_default();
        if unit != format!("{}.service", record.service)
            || (record.service == "deadlock-llm-model-resolver"
                && entry["_UID"].as_str() != Some("1000"))
        {
            failed += 1;
            continue;
        }
        if tb_llm::ledger::recover_with_pool(
            &pool,
            &record.completion,
            &record.project,
            &record.service,
        )
        .await
        .is_err()
        {
            failed += 1;
        }
    }
    if failed > 0 {
        return Err(
            format!("{failed} Verbrauchsabschlüsse konnten nicht nachgeliefert werden").into(),
        );
    }
    Ok(())
}

fn strip_ansi(message: &str) -> String {
    static ANSI: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    ANSI.get_or_init(|| {
        regex::Regex::new(r"\x1b\[[0-?]*[ -/]*[@-~]").expect("Fester ANSI-Ausdruck")
    })
    .replace_all(message, "")
    .into_owned()
}
#[cfg(test)]
mod tests {
    #[test]
    fn tracing_ansi_bleibt_lesbar() {
        let text="\x1b[31mERROR\x1b[0m LLM_USAGE_RECOVERY \x1b[3mrecovery\x1b[0m\x1b[2m=\x1b[0m{\"project\":\"Deadlock-Twitch-Bot\"}";
        assert!(super::strip_ansi(text).contains("recovery={\"project\":"));
    }
}
