use std::path::Path;
use tb_config::{runtime, BotConfigSnapshot};

#[test]
fn fachverbraucher_bekommen_nur_die_gestartete_gepruefte_momentaufnahme() {
    assert!(runtime::settings().is_err());
    let snapshot = BotConfigSnapshot::parse(
        "schema_version=1\n[twitch]\nbot_user_id='11'\nnotify_channel_id='22'\neventsub_callback_url='https://example.invalid/callback'\ntarget_game='Testspiel'\n[broker]\nbase_url='http://127.0.0.1:18770'\n",
        Path::new("/tmp/twitch-config-runtime-test/bot.toml"),
    ).unwrap();
    runtime::install(snapshot.clone()).unwrap();
    let settings = runtime::settings().unwrap();
    assert_eq!(settings.twitch.bot_user_id, "11");
    assert_eq!(settings.twitch.notify_channel_id, "22");
    assert_eq!(settings.twitch.target_game, "Testspiel");
    assert_eq!(settings.broker.base_url, "http://127.0.0.1:18770");
    assert!(runtime::install(snapshot).is_err());
    assert!(std::ptr::eq(settings, runtime::settings().unwrap()));
}
