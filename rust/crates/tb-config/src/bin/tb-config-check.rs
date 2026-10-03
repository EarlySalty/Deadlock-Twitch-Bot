include!(concat!(env!("OUT_DIR"), "/build_revision.rs"));

use std::{
    ffi::OsStr,
    io::{Read, Write},
};
use tb_config::{
    BotConfigSnapshot,
    editor::{
        BRAIN_CONFIG_PATH, BrainInspection, BrainPatch, EditError, inspect_brain, save_brain,
    },
    file::ConfigArguments,
};

fn print_brain(inspection: &BrainInspection) -> Result<(), EditError> {
    let mut output = std::io::stdout().lock();
    serde_json::to_writer(&mut output, inspection).map_err(|_| EditError::Io)?;
    output.write_all(b"\n").map_err(|_| EditError::Io)
}

fn report_edit(result: Result<(), EditError>) -> std::process::ExitCode {
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            match error {
                EditError::Invalid(error) => eprintln!("{error}"),
                EditError::Conflict => eprintln!(
                    "Die gespeicherte Konfiguration wurde verändert. Brain-Stand erneut prüfen."
                ),
                EditError::Busy => {
                    eprintln!("Die Konfiguration wird gerade bearbeitet. Später erneut versuchen.")
                }
                EditError::UnsafeLocation => eprintln!(
                    "Konfiguration und Sperre müssen reguläre Dateien ohne Verknüpfungen sein und außerhalb eines Git-Checkouts liegen."
                ),
                EditError::Io => eprintln!(
                    "Brain-Konfiguration konnte nicht gelesen oder gespeichert werden. Dateirechte und Betriebsablage prüfen."
                ),
            }
            std::process::ExitCode::from(2)
        }
    }
}

fn read_patch(input: impl Read) -> Result<BrainPatch, EditError> {
    const MAX_PATCH_BYTES: u64 = 16 * 1024;
    let mut bytes = Vec::new();
    input
        .take(MAX_PATCH_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| EditError::Io)?;
    if bytes.len() as u64 <= MAX_PATCH_BYTES {
        serde_json::from_slice::<BrainPatch>(&bytes).ok()
    } else {
        None
    }
    .ok_or_else(|| tb_config::file::FileError::invalid("brain_patch").into())
}

fn apply_brain(expected_revision: &str) -> Result<(), EditError> {
    let patch = read_patch(std::io::stdin().lock())?;
    print_brain(&save_brain(expected_revision, &patch)?)
}

fn main() -> std::process::ExitCode {
    if print_build_revision() {
        return std::process::ExitCode::SUCCESS;
    }
    let arguments = match ConfigArguments::parse(std::env::args_os().skip(1)) {
        Ok(arguments) => arguments,
        Err(error) => {
            eprintln!("{error}");
            return std::process::ExitCode::from(2);
        }
    };
    match arguments.remaining.as_slice() {
        [action] if action == "--brain-inspect" => {
            return report_edit(
                inspect_brain(&arguments.path).and_then(|state| print_brain(&state)),
            );
        }
        [action, expected, hash]
            if action == "--brain-apply" && expected == "--expected-revision" =>
        {
            if arguments.path.as_os_str() != OsStr::new(BRAIN_CONFIG_PATH) {
                eprintln!(
                    "Brain-Änderungen sind auf /var/lib/deadlock-twitch/config/bot.toml begrenzt."
                );
                return std::process::ExitCode::from(2);
            }
            let Some(hash) = hash.to_str() else {
                eprintln!("Der erwartete alte SHA-256 muss als Hexadezimalwert angegeben werden.");
                return std::process::ExitCode::from(2);
            };
            return report_edit(apply_brain(hash));
        }
        [] => {}
        _ => {
            eprintln!(
                "Erlaubt sind --config, zusätzlich --brain-inspect oder --brain-apply --expected-revision mit dem alten SHA-256."
            );
            return std::process::ExitCode::from(2);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strukturierte_eingabe_bleibt_streng_typisiert() {
        let patch = read_patch(br#"{"bot_brain_client":{"mode":"typed","endpoint":"http://127.0.0.1:8788"},"bot_brain_chat_enabled":true}"#.as_slice()).unwrap();
        assert_eq!(patch.bot_brain_chat_enabled, Some(true));
        assert!(patch.dashboard_brain_client.is_none());
        assert_eq!(
            patch.bot_brain_client.unwrap().mode,
            Some(tb_config::dashboard_options::BrainClientMode::Typed)
        );
        for input in [
            r#"{"bot_brain_chat_enabled":"true"}"#,
            r#"{"bot_brain_client":{"timeout_ms":1000}}"#,
            r#"{"bot_brain_chat_enabled":null}"#,
            r#"{"synthetic-private-value":"unterminated}"#,
            r#"{"bot_brain_chat_enabled":true} trailing"#,
        ] {
            let error = match read_patch(input.as_bytes()) {
                Err(EditError::Invalid(error)) => error,
                _ => panic!("Ungültige Eingabe wurde nicht abgewiesen"),
            };
            assert!(!error.to_string().contains("synthetic-private-value"));
        }
    }

    #[test]
    fn eingabe_ist_auf_sechzehn_kib_begrenzt() {
        let mut input = br#"{"bot_brain_chat_enabled":true}"#.to_vec();
        input.resize(16 * 1024, b' ');
        assert!(read_patch(input.as_slice()).is_ok());
        input.push(b' ');
        assert!(matches!(
            read_patch(input.as_slice()),
            Err(EditError::Invalid(_))
        ));
    }

    #[test]
    fn lesefehler_werden_nicht_verschluckt() {
        struct FailedRead;
        impl Read for FailedRead {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("synthetic-private-value"))
            }
        }
        assert!(matches!(read_patch(FailedRead), Err(EditError::Io)));
    }
}
