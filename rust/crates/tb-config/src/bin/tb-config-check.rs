//! Prüft dieselbe typisierte Datei wie die Dienste, ohne Zugangsdaten zu lesen
//! oder Clients, Verzeichnisse und Hintergrundaufgaben anzulegen.

use tb_config::{file::ConfigArguments, BotConfigSnapshot};

fn main() -> std::process::ExitCode {
    let arguments = match ConfigArguments::parse(std::env::args_os().skip(1)) {
        Ok(arguments) => arguments,
        Err(error) => {
            eprintln!("{error}");
            return std::process::ExitCode::from(2);
        }
    };
    if !arguments.remaining.is_empty() {
        eprintln!("Die Config-Prüfung akzeptiert nur --config mit einem absoluten Dateipfad.");
        return std::process::ExitCode::from(2);
    }
    match BotConfigSnapshot::load(&arguments.path) {
        Ok(snapshot) => {
            println!(
                "TWITCH_CONFIG_VALID schema_version={} fingerprint={} restart_required_for_changes=true",
                snapshot.settings().schema_version,
                snapshot.fingerprint(),
            );
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::from(2)
        }
    }
}
