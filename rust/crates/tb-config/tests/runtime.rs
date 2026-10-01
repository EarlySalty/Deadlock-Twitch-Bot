use std::path::Path;
use tb_config::{runtime, BotConfigSnapshot};

#[test]
fn fachverbraucher_bekommen_nur_die_gestartete_gepruefte_momentaufnahme() {
    assert!(runtime::settings().is_err());
    let snapshot = BotConfigSnapshot::parse(
        "schema_version=1\n[twitch]\nbot_user_id='11'\nnotify_channel_id='22'\neventsub_callback_url='https://example.invalid/callback'\ntarget_game='Testspiel'\n[broker]\nbase_url='http://127.0.0.1:18770'\n[engagement]\ntranscript_capture_seconds=67\nlearn_enabled=true\nstreamlink_binary='/usr/bin/streamlink'\n[dashboard.options]\nturnier_internal_base_url='http://127.0.0.1:18900'\ncookie_insecure=true\n[vod_archive]\nmax_uploads_per_run=3\ninterval_hours=12\nplaylist_id='public-playlist'\n",
        Path::new("/tmp/twitch-config-runtime-test/bot.toml"),
    ).unwrap();
    runtime::install(snapshot.clone()).unwrap();
    let settings = runtime::settings().unwrap();
    assert_eq!(settings.twitch.bot_user_id, "11");
    assert_eq!(settings.twitch.notify_channel_id, "22");
    assert_eq!(settings.twitch.target_game, "Testspiel");
    assert_eq!(settings.broker.base_url, "http://127.0.0.1:18770");
    assert_eq!(
        runtime::engagement_value("ENGAGEMENT_TRANSCRIPT_CAPTURE_SECONDS").unwrap(),
        "67"
    );
    assert_eq!(
        runtime::engagement_value("ENGAGEMENT_LEARN_ENABLED").unwrap(),
        "1"
    );
    assert_eq!(
        runtime::engagement_value("VOICE_REACTION_STREAMLINK_BIN").unwrap(),
        "/usr/bin/streamlink"
    );
    assert_eq!(
        runtime::dashboard_value("TURNIER_INTERNAL_API_BASE_URL").unwrap(),
        "http://127.0.0.1:18900"
    );
    assert_eq!(
        runtime::dashboard_value("TB_DASHBOARD_COOKIE_INSECURE").unwrap(),
        "1"
    );
    assert!(runtime::dashboard_value("TWITCH_CLIENT_SECRET").is_err());
    assert!(runtime::dashboard_value("UNKNOWN_CONSUMER").is_err());
    assert_eq!(settings.vod_archive.max_uploads_per_run, 3);
    assert_eq!(settings.vod_archive.interval_hours, 12);
    assert_eq!(
        settings.vod_archive.playlist_id.as_deref(),
        Some("public-playlist")
    );
    assert!(runtime::install(snapshot).is_err());
    assert!(std::ptr::eq(settings, runtime::settings().unwrap()));
}
