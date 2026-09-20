use std::path::Path;
use tb_config::BotConfigSnapshot;

const BASE: &str = "schema_version=1\n[twitch]\nbot_user_id=\"1\"\nnotify_channel_id=\"2\"\neventsub_callback_url=\"https://example.invalid/callback\"\n";

fn config(section: &str) -> Result<BotConfigSnapshot, tb_config::file::FileError> {
    BotConfigSnapshot::parse(
        &format!("{BASE}\n[stt]\n{section}"),
        Path::new("/srv/twitch/config/bot.toml"),
    )
}

#[test]
fn client_und_server_haben_denselben_endpunkt_ohne_zweite_einstellung() {
    let snapshot =
        config("host='::1'\nport=19091\nmodel='ggml-large-v3-turbo-q5_0'\nthreads=3")
            .unwrap();
    let stt = &snapshot.settings().stt;
    assert_eq!(stt.local_origin(), "http://[::1]:19091");
    assert_eq!(
        stt.transcription_endpoint(),
        "http://[::1]:19091/v1/audio/transcriptions"
    );
    assert_eq!(stt.request_model(), "ggml-large-v3-turbo-q5_0");
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
    assert_eq!(stt.model, "ggml-large-v3-turbo-q5_0");
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
    assert!(config(
        "remote_endpoint='https://api.openai.com/v1/audio/transcriptions'\nremote_model='another-model'"
    )
    .is_err());
}
