use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use tb_config::{
    file::{ConfigArguments, ErrorKind, MAX_CONFIG_BYTES},
    BotConfigSnapshot,
};

const VALID: &str = r#"
schema_version = 1
[twitch]
bot_user_id = "1422558159"
notify_channel_id = "1304169815505637458"
eventsub_callback_url = "https://deutsche-deadlock-community.de/twitch/eventsub/callback"
[database]
pool_max = 17
acquire_timeout_ms = 2500
connect_timeout_seconds = 9
[internal_api]
host = "127.0.0.1"
port = 18776
[dashboard]
host = "127.0.0.1"
port = 18769
[broker]
base_url = "http://127.0.0.1:18770"
"#;

fn parse(text: &str) -> Result<BotConfigSnapshot, tb_config::file::FileError> {
    BotConfigSnapshot::parse(text, Path::new("/srv/twitch/config/bot.toml"))
}

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "tb-config-file-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn file(&self) -> PathBuf {
        self.0.join("bot.toml")
    }
    fn write(&self, data: impl AsRef<[u8]>) {
        std::fs::write(self.file(), data).unwrap();
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn gueltige_datei_und_nachvollziehbare_defaults() {
    let snapshot = parse(VALID).unwrap();
    assert_eq!(snapshot.settings().database.pool_max, 17);
    assert_eq!(snapshot.settings().twitch.target_game, "Deadlock");
    assert_eq!(
        snapshot.settings().twitch.language_filters,
        ["de", "de-de", "de-at", "de-ch"]
    );
    assert_eq!(snapshot.settings().logging.level.as_str(), "info");
    assert_eq!(snapshot.fingerprint().len(), 64);
}

#[test]
fn dieselbe_validierte_momentaufnahme_wird_geteilt() {
    let first = parse(VALID).unwrap();
    let second = first.clone();
    assert!(Arc::ptr_eq(
        &first.shared_settings(),
        &second.shared_settings()
    ));
}

#[test]
fn version_und_pflichtwerte_fehlen_nicht_still() {
    for text in [
        VALID.replace("schema_version = 1\n", ""),
        VALID.replace("bot_user_id = \"1422558159\"\n", ""),
    ] {
        assert_eq!(parse(&text).unwrap_err().kind, ErrorKind::InvalidDocument);
    }
    assert!(parse(&VALID.replace("schema_version = 1", "schema_version = 2")).is_err());
}

#[test]
fn unbekannte_felder_werden_auf_jeder_ebene_abgelehnt() {
    for text in [
        format!("unbekannt = 4\n{VALID}"),
        VALID.replace("pool_max = 17", "pool_max = 17\nunbekannt = 4"),
        VALID.replace("bot_user_id =", "bot_usre_id ="),
    ] {
        assert_eq!(parse(&text).unwrap_err().kind, ErrorKind::InvalidDocument);
    }
}

#[test]
fn typen_sind_keine_env_strings() {
    for text in [
        VALID.replace("port = 18776", "port = \"18776\""),
        VALID.replace("pool_max = 17", "pool_max = false"),
        VALID.replace("acquire_timeout_ms = 2500", "acquire_timeout_ms = 2.5"),
        VALID.replace("bot_user_id = \"1422558159\"", "bot_user_id = 1422558159"),
    ] {
        assert!(parse(&text).is_err());
    }
}

#[test]
fn port_grenzen_und_listener_kollision() {
    for value in ["0", "65536", "-1"] {
        assert!(parse(&VALID.replace("port = 18776", &format!("port = {value}"))).is_err());
    }
    assert!(parse(&VALID.replace("port = 18776", "port = 65535")).is_ok());
    assert!(parse(&VALID.replace("port = 18769", "port = 18776")).is_err());
    assert!(parse(&VALID.replace("host = \"127.0.0.1\"", "host = \"0.0.0.0\"")).is_err());
}

#[test]
fn ids_ohne_null_vorzeichen_unicode_oder_bigint_ueberlauf() {
    for value in [
        "",
        "0",
        "-1",
        "+1",
        "12.3",
        "１２３",
        "9223372036854775808",
        "18446744073709551616",
    ] {
        assert!(
            parse(&VALID.replace("1422558159", value)).is_err(),
            "ID-Grenze wurde nicht geprüft"
        );
    }
    assert!(parse(&VALID.replace("1422558159", "9223372036854775807")).is_ok());
}

#[test]
fn zeitlimit_und_pool_grenzen_vor_clientaufbau() {
    for (field, old, invalid) in [
        ("pool_max", "17", "0"),
        ("pool_max", "17", "1001"),
        ("acquire_timeout_ms", "2500", "99"),
        ("acquire_timeout_ms", "2500", "300001"),
        ("connect_timeout_seconds", "9", "0"),
        ("connect_timeout_seconds", "9", "301"),
    ] {
        assert!(parse(
            &VALID.replace(&format!("{field} = {old}"), &format!("{field} = {invalid}"))
        )
        .is_err());
    }
}

#[test]
fn leere_sprachliste_aktiviert_nicht_alle_sprachen() {
    let text = VALID.replace("[database]", "language_filters = []\n[database]");
    assert!(parse(&text).is_err());
}

#[test]
fn zugangsdaten_in_urls_und_externe_broker_werden_abgelehnt() {
    for url in [
        "http://127.0.0.1:18770/?access_token=REDACTION_SENTINEL",
        "http://REDACTION_SENTINEL@127.0.0.1:18770",
        "http://127.0.0.1:18770/#REDACTION_SENTINEL",
        "https://untrusted.invalid",
        "http://127.0.0.1.untrusted.invalid:18770",
    ] {
        let error = parse(&VALID.replace("http://127.0.0.1:18770", url)).unwrap_err();
        assert!(!format!("{error} {error:?}").contains("REDACTION_SENTINEL"));
        assert_eq!(error.field, Some("broker.base_url"));
    }
}

#[test]
fn parserfehler_enthalten_weder_zeilenausschnitt_noch_wert_noch_quelle() {
    use std::error::Error;
    for text in [
        VALID.replace("pool_max = 17", "pool_max = \"REDACTION_SENTINEL\""),
        format!("REDACTION_SENTINEL = \"unbekannt\"\n{VALID}"),
        format!("{VALID}\n[REDACTION_SENTINEL"),
    ] {
        let error = parse(&text).unwrap_err();
        let output = format!("{error} {error:?}");
        assert!(!output.contains("REDACTION_SENTINEL"));
        assert!(!output.contains("pool_max ="));
        assert!(error.source().is_none());
    }
}

#[test]
fn debug_der_momentaufnahme_gibt_keine_freien_texte_aus() {
    let snapshot = parse(&VALID.replace(
        "[database]",
        "target_game = \"REDACTION_SENTINEL\"\n[database]",
    ))
    .unwrap();
    assert!(!format!("{snapshot:?} {:?}", snapshot.settings()).contains("REDACTION_SENTINEL"));
}

#[test]
fn relative_pfade_beziehen_sich_auf_config_und_nicht_auf_cwd() {
    let snapshot = parse(VALID).unwrap();
    assert_eq!(
        snapshot.resolve(Path::new("../data/cache")).unwrap(),
        Path::new("/srv/twitch/data/cache")
    );
    assert_eq!(
        snapshot.resolve(Path::new("logs/./audit")).unwrap(),
        Path::new("/srv/twitch/config/logs/audit")
    );
    assert_eq!(
        snapshot.resolve(Path::new("/var/lib/twitch")).unwrap(),
        Path::new("/var/lib/twitch")
    );
    assert!(snapshot.resolve(Path::new("")).is_err());
    assert!(snapshot.resolve(Path::new("../../../../escape")).is_err());
    assert!(BotConfigSnapshot::parse(VALID, Path::new("config/bot.toml")).is_err());
}

#[test]
fn fehlende_datei_und_verzeichnis_sind_keine_leere_default_config() {
    let directory = Directory::new();
    assert_eq!(
        BotConfigSnapshot::load(&directory.file()).unwrap_err().kind,
        ErrorKind::FileMissing
    );
    assert_eq!(
        BotConfigSnapshot::load(&directory.0).unwrap_err().kind,
        ErrorKind::RegularFileRequired
    );
    assert!(!directory.file().exists());
}

#[test]
fn lesen_schreibt_keine_defaults_und_behaelt_kommentare() {
    let directory = Directory::new();
    let input = format!("# Eigene Betriebskommentare bleiben erhalten.\n{VALID}");
    directory.write(&input);
    let snapshot = BotConfigSnapshot::load(&directory.file()).unwrap();
    assert_eq!(std::fs::read_to_string(directory.file()).unwrap(), input);
    assert!(snapshot.recheck().is_ok());
}

#[test]
fn dateigroesse_und_utf8_werden_begrenzt() {
    let directory = Directory::new();
    directory.write(vec![b'#'; MAX_CONFIG_BYTES as usize + 1]);
    assert_eq!(
        BotConfigSnapshot::load(&directory.file()).unwrap_err().kind,
        ErrorKind::FileTooLarge
    );
    directory.write([255, 254]);
    assert_eq!(
        BotConfigSnapshot::load(&directory.file()).unwrap_err().kind,
        ErrorKind::InvalidEncoding
    );
}

#[test]
fn fehlerhafter_recheck_behaelt_vorherige_werte_und_gueltige_aenderung_verlangt_neustart() {
    let directory = Directory::new();
    directory.write(VALID);
    let active = BotConfigSnapshot::load(&directory.file()).unwrap();
    let shared = active.shared_settings();
    directory.write(VALID.replace("port = 18776", "port = 0"));
    assert!(active.recheck().is_err());
    assert_eq!(active.settings().internal_api.port, 18776);
    assert!(Arc::ptr_eq(&shared, &active.shared_settings()));
    directory.write(VALID.replace("port = 18776", "port = 18777"));
    assert_eq!(
        active.recheck().unwrap_err().kind,
        ErrorKind::RestartRequired
    );
    assert_eq!(active.settings().internal_api.port, 18776);
    directory.write(VALID);
    assert!(active.recheck().is_ok());
}

#[test]
fn bekannte_betriebswerte_werden_an_bestehende_verbrauchertypen_weitergereicht() {
    let snapshot = parse(VALID).unwrap();
    let queries = std::cell::RefCell::new(Vec::new());
    let runtime = snapshot
        .runtime_settings(&|key| {
            queries.borrow_mut().push(key.to_string());
            Some("REDACTION_SENTINEL".into())
        })
        .unwrap();
    assert!(!format!("{runtime:?}").contains("REDACTION_SENTINEL"));
    assert_eq!(runtime.db.pool_max, 17);
    assert_eq!(runtime.db.acquire_timeout, Duration::from_millis(2500));
    assert_eq!(runtime.db.connect_timeout, Duration::from_secs(9));
    assert_eq!(runtime.internal_api.port, 18776);
    assert_eq!(runtime.broker.base_url, "http://127.0.0.1:18770");
    assert!(queries.borrow().iter().all(|key| matches!(
        key.as_str(),
        "TWITCH_ANALYTICS_DSN"
            | "TWITCH_INTERNAL_API_TOKEN"
            | "MASTER_BROKER_TOKEN"
            | "MAIN_BOT_INTERNAL_TOKEN"
    )));
}

#[test]
fn nicht_geheime_werte_aus_bisherigem_getter_koennen_die_datei_nicht_uebersteuern() {
    let snapshot = parse(VALID).unwrap();
    let settings = snapshot
        .runtime_settings(&|key| {
            Some(
                match key {
                    "TWITCH_INTERNAL_API_PORT" => "9999",
                    "TWITCH_ANALYTICS_POOL_MAXSIZE" => "999",
                    "MASTER_BROKER_BASE_URL" => "https://untrusted.invalid",
                    _ => "REDACTION_SENTINEL",
                }
                .to_string(),
            )
        })
        .unwrap();
    assert_eq!(settings.internal_api.port, 18776);
    assert_eq!(settings.db.pool_max, 17);
    assert_eq!(settings.broker.base_url, "http://127.0.0.1:18770");
}

#[test]
fn echte_prozessumgebung_uebersteuert_die_toml_nicht() {
    let directory = Directory::new();
    directory.write(VALID);
    let run = |port: &str| {
        std::process::Command::new(env!("CARGO_BIN_EXE_tb-config-check"))
            .env_clear()
            .env("TWITCH_INTERNAL_API_PORT", port)
            .env("TWITCH_ANALYTICS_POOL_MAXSIZE", "999")
            .env("DASHBOARD_PORT", "ungueltig")
            .env("RUST_LOG", "REDACTION_SENTINEL")
            .args([
                std::ffi::OsStr::new("--config"),
                directory.file().as_os_str(),
            ])
            .output()
            .unwrap()
    };
    let first = run("9998");
    let second = run("9999");
    assert!(first.status.success());
    assert!(second.status.success());
    assert_eq!(first.stdout, second.stdout);
    assert!(first.stderr.is_empty());
    assert!(second.stderr.is_empty());
}

#[test]
fn explizite_datei_funktioniert_aus_verschiedenen_arbeitsverzeichnissen() {
    let first = Directory::new();
    let second = Directory::new();
    first.write(VALID);
    let run = |cwd: &Path| {
        std::process::Command::new(env!("CARGO_BIN_EXE_tb-config-check"))
            .env_clear()
            .current_dir(cwd)
            .args([std::ffi::OsStr::new("--config"), first.file().as_os_str()])
            .output()
            .unwrap()
    };
    let a = run(&first.0);
    let b = run(&second.0);
    assert!(a.status.success() && b.status.success());
    assert_eq!(a.stdout, b.stdout);
}

#[test]
fn env_config_pfade_werden_nicht_als_startargument_verwendet() {
    let directory = Directory::new();
    directory.write(VALID);
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_tb-config-check"))
        .env_clear()
        .env("TWITCH_RUNTIME_CONFIG_FILE", directory.file())
        .env("BOT_CONFIG_PATH", directory.file())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
}

#[test]
fn echte_cli_redigiert_parserfehler() {
    let directory = Directory::new();
    directory.write(VALID.replace("pool_max = 17", "pool_max = \"REDACTION_SENTINEL\""));
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_tb-config-check"))
        .env_clear()
        .args([
            std::ffi::OsStr::new("--config"),
            directory.file().as_os_str(),
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&output.stderr).contains("REDACTION_SENTINEL"));
}

#[test]
fn config_argument_explizit_eindeutig_absolut_und_fachargumente_bleiben_erhalten() {
    let args = ConfigArguments::parse(
        [
            "--once",
            "--config",
            "/srv/twitch/config/bot.toml",
            "--channel",
            "test",
        ]
        .map(OsString::from),
    )
    .unwrap();
    assert_eq!(args.path, Path::new("/srv/twitch/config/bot.toml"));
    assert_eq!(
        args.remaining,
        ["--once", "--channel", "test"].map(OsString::from)
    );
    assert!(
        ConfigArguments::parse(["--config=/srv/twitch/config/bot.toml"].map(OsString::from))
            .is_ok()
    );
    assert_eq!(
        ConfigArguments::parse(Vec::new()).unwrap_err().kind,
        ErrorKind::ConfigArgumentMissing
    );
    assert!(ConfigArguments::parse(["--config", "config/bot.toml"].map(OsString::from)).is_err());
    assert_eq!(
        ConfigArguments::parse(["--config", "/a.toml", "--config=/b.toml"].map(OsString::from))
            .unwrap_err()
            .kind,
        ErrorKind::ConfigArgumentDuplicate
    );
}
