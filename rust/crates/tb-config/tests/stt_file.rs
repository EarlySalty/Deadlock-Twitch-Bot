use std::{
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};
use tb_config::{file::ErrorKind, BotConfigSnapshot};

const BASE: &str = "schema_version=1\n[twitch]\nbot_user_id=\"1\"\nnotify_channel_id=\"2\"\neventsub_callback_url=\"https://example.invalid/callback\"\n";

fn config(section: &str) -> Result<BotConfigSnapshot, tb_config::file::FileError> {
    BotConfigSnapshot::parse(
        &format!("{BASE}\n[stt]\n{section}"),
        Path::new("/srv/twitch/config/bot.toml"),
    )
}

#[test]
fn client_und_server_haben_denselben_endpunkt_ohne_zweite_einstellung() {
    let snapshot = config("host='::1'\nport=19091\nmodel='local/model'\nthreads=3").unwrap();
    let stt = &snapshot.settings().stt;
    assert_eq!(stt.local_origin(), "http://[::1]:19091");
    assert_eq!(
        stt.transcription_endpoint(),
        "http://[::1]:19091/v1/audio/transcriptions"
    );
    assert_eq!(stt.request_model(), "local/model");
    assert!(stt.remote_endpoint.is_none());
}

#[test]
fn stt_default_transkribiert_lokal_und_erkennt_die_sprache() {
    let snapshot = config("").unwrap();
    let stt = &snapshot.settings().stt;
    assert_eq!(
        stt.transcription_endpoint(),
        "http://127.0.0.1:8791/v1/audio/transcriptions"
    );
    assert_eq!(stt.threads, 8);
    assert_eq!(stt.language, None);
    assert_eq!(stt.no_speech_max, 0.6);
    assert_eq!(stt.avg_logprob_min, -1.0);
}

#[test]
fn stt_falsche_typen_unbekannte_felder_und_grenzen_sind_fatal() {
    for section in [
        "port=0",
        "port=65536",
        "threads=0",
        "threads=65",
        "threads='8'",
        "host='0.0.0.0'",
        "host='::'",
        "no_speech_max=nan",
        "no_speech_max=1.01",
        "avg_logprob_min=-inf",
        "avg_logprob_min=0.1",
        "avg_logprob_min=-20.1",
        "timeout_seconds=0",
        "timeout_seconds=3601",
        "extraction_timeout_seconds=0",
        "max_upload_bytes=0",
        "max_upload_bytes=26214401",
        "language=''",
        "language='de?REDACTION_SENTINEL'",
        "model='https://secret.invalid/?token=REDACTION_SENTINEL'",
        "modell='unbekannt'",
        "port=8776",
        "port=8769",
    ] {
        let error = config(section).unwrap_err();
        assert!(!format!("{error} {error:?}").contains("REDACTION_SENTINEL"));
    }
}

#[test]
fn nur_bestehende_remote_freigabe_keine_neuen_audio_anbieter() {
    let remote =
        config("remote_endpoint='https://api.openai.com/v1/audio/transcriptions'").unwrap();
    assert_eq!(remote.settings().stt.request_model(), "whisper-1");
    for endpoint in [
        "https://other.invalid/v1/audio/transcriptions",
        "http://api.openai.com/v1/audio/transcriptions",
        "https://api.openai.com:8443/v1/audio/transcriptions",
        "https://api.openai.com/v1/audio/transcriptions?token=REDACTION_SENTINEL",
        "https://REDACTION_SENTINEL@api.openai.com/v1/audio/transcriptions",
        "https://api.openai.com.other.invalid/v1/audio/transcriptions",
    ] {
        assert!(config(&format!("remote_endpoint='{endpoint}'")).is_err());
    }
    assert!(config("remote_endpoint='https://api.openai.com/v1/audio/transcriptions'\nremote_model='another-model'").is_err());
}

#[test]
fn startpfade_sind_keine_umgebungs_fallbacks() {
    let error = config("[stt.launch]\npython_binary='/usr/bin/python3'\nserver_script='server.py'")
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::InvalidDocument);
    let snapshot = config("[stt.launch]\npython_binary='/usr/bin/python3'\nserver_script='../ops/stt-server/stt_server.py'\ncache_directory='../data/models'").unwrap();
    let launch = snapshot.settings().stt.launch.as_ref().unwrap();
    assert_eq!(
        snapshot.resolve(&launch.server_script).unwrap(),
        Path::new("/srv/twitch/ops/stt-server/stt_server.py")
    );
    assert_eq!(
        snapshot.resolve(&launch.cache_directory).unwrap(),
        Path::new("/srv/twitch/data/models")
    );
}

#[test]
#[cfg(unix)]
fn starter_uebergibt_echte_gepruefte_argumente_statt_environment() {
    // Der isolierte Kindprozess liest nur argv und lädt kein Whisper-Modell.
    // Er bestätigt die tatsächlich übergebenen Argumente, nicht nur einen Mock
    // des Command-Builders. Die Standardbibliothek reicht dafür aus.
    let python = Path::new("/usr/bin/python3");
    assert!(
        python.is_file(),
        "Dieser Unix-Prozesstest benötigt System-Python3."
    );
    static NUMBER: AtomicU64 = AtomicU64::new(0);
    let root = std::env::temp_dir().join(format!(
        "tb-stt-launch-{}-{}",
        std::process::id(),
        NUMBER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&root).unwrap();
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(root.clone());
    std::fs::write(
        root.join("child.py"),
        "import json, sys\nprint(json.dumps(sys.argv[1:]))\n",
    )
    .unwrap();
    let document = format!("{BASE}\n[stt]\nport=19091\nthreads=3\nmodel='local/model'\nno_speech_max=0.4\navg_logprob_min=-0.8\nmax_upload_bytes=1048576\n[stt.launch]\npython_binary='/usr/bin/python3'\nserver_script='child.py'\ncache_directory='models'\n");
    std::fs::write(root.join("bot.toml"), &document).unwrap();
    let checked = BotConfigSnapshot::load(&root.join("bot.toml")).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_tb-stt-launcher"))
        .env_clear()
        .env("STT_THREADS", "63")
        .env("STT_PORT", "18888")
        .current_dir("/")
        .arg("--config")
        .arg(root.join("bot.toml"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "STT-Teststarter muss erfolgreich exec ausführen"
    );
    let args: Vec<String> = serde_json::from_slice(&output.stdout).unwrap();
    let pairs: std::collections::BTreeMap<_, _> = args
        .chunks_exact(2)
        .map(|pair| (pair[0].as_str(), pair[1].as_str()))
        .collect();
    assert_eq!(pairs["--threads"], "3");
    assert_eq!(pairs["--port"], "19091");
    assert_eq!(pairs["--model"], "local/model");
    assert_eq!(pairs["--language"], "");
    assert_eq!(pairs["--no-speech-max"], "0.4");
    assert_eq!(pairs["--avg-logprob-min"], "-0.8");
    assert_eq!(pairs["--max-upload-bytes"], "1048576");
    assert_eq!(
        pairs["--cache-directory"],
        root.join("models").to_str().unwrap()
    );
    assert_eq!(pairs["--config-fingerprint"], checked.fingerprint());
    assert!(String::from_utf8_lossy(&output.stderr).contains("TWITCH_STT_CONFIG_V1"));
    assert_eq!(
        std::fs::read_to_string(root.join("bot.toml")).unwrap(),
        document
    );
    std::fs::create_dir_all(root.join("models/local")).unwrap();
    std::fs::write(
        root.join("bot.toml"),
        document.replace("local/model", "./models/local"),
    )
    .unwrap();
    for cwd in [Path::new("/"), root.as_path()] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_tb-stt-launcher"))
            .env_clear()
            .current_dir(cwd)
            .arg("--config")
            .arg(root.join("bot.toml"))
            .output()
            .unwrap();
        assert!(output.status.success());
        let args: Vec<String> = serde_json::from_slice(&output.stdout).unwrap();
        let model = args.windows(2).find(|pair| pair[0] == "--model").unwrap();
        assert_eq!(Path::new(&model[1]), root.join("models/local"));
    }
}
