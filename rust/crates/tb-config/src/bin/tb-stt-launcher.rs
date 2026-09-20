//! Übergibt die geprüfte gemeinsame Konfiguration an den bestehenden
//! Python-Ausnahmedienst. Kein ENV-Export, kein zweiter TOML-Parser.

use std::{
    path::Path,
    process::{Command, ExitCode},
};
use tb_config::{file::ConfigArguments, BotConfigSnapshot};

fn executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

fn command(snapshot: &BotConfigSnapshot) -> Result<Command, &'static str> {
    let config = &snapshot.settings().stt;
    let launch = config
        .launch
        .as_ref()
        .ok_or("Der STT-Start benötigt stt.launch in der gemeinsamen Konfiguration.")?;
    let python = snapshot
        .resolve(&launch.python_binary)
        .map_err(|_| "Der STT-Python-Pfad ist ungültig.")?;
    let script = snapshot
        .resolve(&launch.server_script)
        .map_err(|_| "Der STT-Skriptpfad ist ungültig.")?;
    let cache = snapshot
        .resolve(&launch.cache_directory)
        .map_err(|_| "Der STT-Cachepfad ist ungültig.")?;
    if !executable(&python) {
        return Err("Das konfigurierte STT-Python-Binary ist nicht ausführbar.");
    }
    if !script.is_file() {
        return Err("Das konfigurierte STT-Skript ist keine vorhandene reguläre Datei.");
    }
    let model_path = snapshot
        .resolve(Path::new(&config.model))
        .map_err(|_| "Der STT-Modellpfad ist ungültig.")?;
    let explicitly_local = Path::new(&config.model).is_absolute()
        || config.model.starts_with("./")
        || config.model.starts_with("../");
    if explicitly_local && !model_path.is_dir() {
        return Err("Das konfigurierte lokale STT-Modellverzeichnis fehlt.");
    }
    let model = if explicitly_local || model_path.is_dir() {
        model_path.into_os_string()
    } else {
        config.model.clone().into()
    };
    let mut command = Command::new(python);
    command
        .current_dir(
            snapshot
                .source()
                .parent()
                .ok_or("Der Konfigurationspfad ist ungültig.")?,
        )
        .arg(script)
        .arg("--host")
        .arg(config.host.to_string())
        .arg("--port")
        .arg(config.port.to_string())
        .arg("--model")
        .arg(model)
        .arg("--threads")
        .arg(config.threads.to_string())
        .arg("--language")
        .arg(config.language.as_deref().unwrap_or(""))
        .arg("--no-speech-max")
        .arg(config.no_speech_max.to_string())
        .arg("--avg-logprob-min")
        .arg(config.avg_logprob_min.to_string())
        .arg("--max-upload-bytes")
        .arg(config.max_upload_bytes.to_string())
        .arg("--cache-directory")
        .arg(cache)
        .arg("--config-fingerprint")
        .arg(snapshot.fingerprint());
    Ok(command)
}

fn main() -> ExitCode {
    let arguments = match ConfigArguments::parse(std::env::args_os().skip(1)) {
        Ok(arguments) if arguments.remaining.is_empty() => arguments,
        Ok(_) => {
            eprintln!("Der STT-Start akzeptiert nur --config mit einem absoluten Dateipfad.");
            return ExitCode::from(2);
        }
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };
    let snapshot = match BotConfigSnapshot::load(&arguments.path) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };
    let mut command = match command(&snapshot) {
        Ok(command) => command,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };
    eprintln!(
        "TWITCH_STT_CONFIG_V1 fingerprint={} listener=loopback",
        snapshot.fingerprint()
    );
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // exec erhält die überwachte PID. Kein zusätzlicher Dauersupervisor.
        let _error = command.exec();
        eprintln!("Der geprüfte STT-Prozess konnte nicht gestartet werden.");
        ExitCode::FAILURE
    }
    #[cfg(not(unix))]
    {
        match command.status() {
            Ok(status) if status.success() => ExitCode::SUCCESS,
            _ => {
                eprintln!("Der geprüfte STT-Prozess ist fehlgeschlagen.");
                ExitCode::FAILURE
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fehlende_startpfade_starten_keinen_prozess() {
        let snapshot = BotConfigSnapshot::parse(
            "schema_version=1\n[twitch]\nbot_user_id=\"1\"\nnotify_channel_id=\"2\"\neventsub_callback_url=\"https://example.invalid/callback\"\n",
            Path::new("/srv/twitch/config/bot.toml"),
        ).unwrap();
        assert!(command(&snapshot).is_err());
    }
}
