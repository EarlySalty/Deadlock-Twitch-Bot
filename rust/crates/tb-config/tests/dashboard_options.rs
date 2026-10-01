use std::path::Path;
use tb_config::BotConfigSnapshot;
const CONFIG: &str = "schema_version=1\n[twitch]\nbot_user_id='123'\nnotify_channel_id='456'\neventsub_callback_url='https://example.invalid/callback'\n";
fn snapshot(section: &str) -> Result<BotConfigSnapshot, tb_config::file::FileError> {
    BotConfigSnapshot::parse(
        &format!("{CONFIG}\n{section}"),
        Path::new("/config/bot.toml"),
    )
}
#[test]
fn dashboard_defaults_preserve_security_and_disabled_optional_features() {
    let snapshot = snapshot("").unwrap();
    let options = &snapshot.settings().dashboard.options;
    assert!(options.runtime_enforce);
    assert!(options.runtime_role.is_empty());
    assert!(!options.cookie_insecure);
    assert!(!options.noauth_readiness);
    assert!(!options.pentest_disable_rate_limits);
    assert!(options.oauth_redirect_uri.is_none());
    assert!(options.demo_login_twitch_user_id.is_none());
    assert!(options.stripe_price_ids.is_empty());
    assert_eq!(
        snapshot.resolve(&options.legal_pages_path).unwrap(),
        Path::new("/config/data/admin_dashboard/legal_pages.json")
    );
}
#[test]
fn typed_stripe_ids_roundtrip_without_changing_prices() {
    let parsed = snapshot("[dashboard.options.stripe_price_ids.plus]\nmonthly='price_synthetic_month'\nyearly='price_synthetic_year'\n[dashboard.options.stripe_product_ids]\nplus='prod_synthetic'\n").unwrap();
    let prices = &parsed.settings().dashboard.options.stripe_price_ids["plus"];
    assert_eq!(
        prices.cycles().collect::<Vec<_>>(),
        vec![(1, "price_synthetic_month"), (12, "price_synthetic_year")]
    );
    let encoded = toml::to_string(parsed.settings()).unwrap();
    let decoded = BotConfigSnapshot::parse(&encoded, Path::new("/config/bot.toml")).unwrap();
    assert_eq!(parsed.fingerprint(), decoded.fingerprint());
    assert!(snapshot("[dashboard.options.stripe_price_ids.plus]\nmonthly=' '\n").is_err());
    assert!(snapshot("[dashboard.options.stripe_price_ids.plus]\nweekly='price_x'\n").is_err());
}
#[test]
fn dashboard_urls_reject_embedded_credentials_and_remote_cleartext() {
    for field in [
        "oauth_redirect_uri",
        "discord_oauth_broker_url",
        "steam_rank_url",
        "helix_base_url",
    ] {
        for bad in [
            "https://user:synthetic@example.invalid/path",
            "http://example.invalid/path",
            "https://example.invalid/#fragment",
        ] {
            assert!(
                snapshot(&format!("[dashboard.options]\n{field}='{bad}'\n")).is_err(),
                "{field}"
            );
        }
    }
    assert!(
        snapshot("[dashboard.options]\ndiscord_oauth_broker_url='http://localhost:18770'\n")
            .is_ok()
    );
}
#[test]
fn dashboard_ids_and_cookie_domain_are_validated_before_startup() {
    for invalid in [
        "admin_owner_user_id=0",
        "admin_guild_ids=[0]",
        "demo_login_twitch_user_id='not-an-id'",
        "shared_admin_cookie_domain='example.invalid; Secure'",
        "legal_pages_path=''",
        "unknown_switch=true",
    ] {
        assert!(
            snapshot(&format!("[dashboard.options]\n{invalid}\n")).is_err(),
            "{invalid}"
        );
    }
}
#[test]
fn affiliate_mail_is_optional_and_cannot_contain_credentials() {
    let parsed=snapshot("[dashboard.options.affiliate_mail]\nhost='smtp.example.invalid'\nfrom_email='billing@example.invalid'\n[dashboard.options.affiliate_seller]\nwebsite='https://example.invalid'\n").unwrap();
    assert_eq!(parsed.settings().dashboard.options.affiliate_mail.port, 587);
    assert!(parsed.settings().dashboard.options.affiliate_mail.starttls);
    for invalid in [
        "password='synthetic'",
        "host='https://smtp.example.invalid'",
        "port=0",
    ] {
        assert!(snapshot(&format!("[dashboard.options.affiliate_mail]\n{invalid}\n")).is_err());
    }
}
