use std::path::Path;
use tb_config::BotConfigSnapshot;
const CONFIG: &str = "schema_version=1\n[twitch]\nbot_user_id='123'\nnotify_channel_id='456'\neventsub_callback_url='https://example.invalid/callback'\n";
#[test]
fn shared_options_preserve_defaults_and_resolve_knowledge_from_the_file() {
    let snapshot = BotConfigSnapshot::parse(CONFIG, Path::new("/test/config/bot.toml")).unwrap();
    assert_eq!(
        snapshot
            .resolve(&snapshot.settings().knowledge.directory)
            .unwrap(),
        Path::new("/test/config/rust/knowledge")
    );
    assert!(!snapshot.settings().media.youtube_audit_passed);
    assert_eq!(
        snapshot.settings().media.social_media_public_origin,
        "https://admin.deutsche-deadlock-community.de"
    );
}
#[test]
fn client_override_does_not_change_listener_or_probe_permission() {
    let raw=format!("{CONFIG}\n[internal_api]\nclient_base_url='https://internal.example.invalid/'\nport=18776\n");
    let snapshot = BotConfigSnapshot::parse(&raw, Path::new("/test/bot.toml")).unwrap();
    assert_eq!(snapshot.settings().internal_api.port, 18776);
    assert_eq!(
        snapshot.settings().internal_api.client_base_url(),
        "https://internal.example.invalid"
    );
    assert!(!snapshot.settings().internal_api.probe_allow_non_loopback);
}
#[test]
fn shared_public_urls_do_not_accept_embedded_credentials() {
    let raw = format!(
        "{CONFIG}\n[media]\nsocial_media_public_origin='https://user:synthetic@example.invalid'\n"
    );
    assert!(BotConfigSnapshot::parse(&raw, Path::new("/test/bot.toml")).is_err());
}
