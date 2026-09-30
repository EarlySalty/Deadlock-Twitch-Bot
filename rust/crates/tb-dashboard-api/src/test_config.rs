//! Explizite TOML-Fixtures für Tests; kein Ersatzpfad im Produktionsbuild.
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    sync::Arc,
};
use tb_config::BotConfigSnapshot;
thread_local! { static SCOPED: RefCell<Option<BotConfigSnapshot>> = const { RefCell::new(None) }; }
pub(crate) struct Scope(Option<BotConfigSnapshot>);
impl Drop for Scope {
    fn drop(&mut self) {
        SCOPED.with(|slot| {
            slot.replace(self.0.take());
        });
    }
}
pub(crate) fn scope(section: &str) -> Scope {
    let raw=format!("schema_version=1\n[twitch]\nbot_user_id='123'\nnotify_channel_id='456'\neventsub_callback_url='https://example.invalid/callback'\n{section}");
    let snapshot = BotConfigSnapshot::parse(&raw, Path::new("/test/bot.toml"))
        .expect("Gültige synthetische Testkonfiguration");
    Scope(SCOPED.with(|slot| slot.replace(Some(snapshot))))
}
pub(crate) fn settings() -> Option<Arc<tb_config::BotConfig>> {
    SCOPED.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(BotConfigSnapshot::shared_settings)
    })
}
pub(crate) fn resolve(path: &Path) -> Option<PathBuf> {
    SCOPED.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|snapshot| snapshot.resolve(path).expect("Testpfad"))
    })
}
