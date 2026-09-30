//! Ausschließlich die beim Dashboardstart installierte TOML, keine zweite Quelle.
use std::{
    ops::Deref,
    path::{Path, PathBuf},
    sync::Arc,
};
pub(crate) fn settings() -> Arc<tb_config::BotConfig> {
    #[cfg(test)]
    if let Some(settings) = crate::test_config::settings() {
        return settings;
    }
    tb_config::runtime::active()
        .expect("Dashboard benötigt die beim Start geprüfte TOML")
        .shared_settings()
}
pub(crate) struct Options(Arc<tb_config::BotConfig>);
impl Deref for Options {
    type Target = tb_config::dashboard_options::DashboardOptions;
    fn deref(&self) -> &Self::Target {
        &self.0.dashboard.options
    }
}
pub(crate) fn options() -> Options {
    Options(settings())
}
pub(crate) fn path(value: &Path) -> PathBuf {
    #[cfg(test)]
    if let Some(path) = crate::test_config::resolve(value) {
        return path;
    }
    tb_config::runtime::active()
        .expect("Dashboard benötigt die beim Start geprüfte TOML")
        .resolve(value)
        .expect("Der Konfigurationspfad wurde beim Start geprüft")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dashboard_readers_share_the_explicit_snapshot_and_restore_nested_test_scopes() {
        let _outer = crate::test_config::scope("[dashboard.options]\ncookie_insecure=true\nlegal_pages_path='legal/custom.json'\nadmin_owner_user_id=987\n[internal_api]\nclient_base_url='http://localhost:18776'\n");
        assert!(options().cookie_insecure);
        assert_eq!(options().admin_owner_user_id, Some(987));
        assert_eq!(
            path(&options().legal_pages_path),
            Path::new("/test/legal/custom.json")
        );
        assert_eq!(
            settings().internal_api.client_base_url(),
            "http://localhost:18776"
        );
        {
            let _inner = crate::test_config::scope("");
            assert!(!options().cookie_insecure);
            assert!(options().admin_owner_user_id.is_none());
        }
        assert!(options().cookie_insecure);
    }
}
