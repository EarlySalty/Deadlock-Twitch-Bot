use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use tb_config::{
    editor::{save, EditError, OperatingOptions},
    BotConfigSnapshot,
};

const CONFIG: &str = "schema_version=1\n[twitch]\nbot_user_id=\"1\"\nnotify_channel_id=\"2\"\neventsub_callback_url=\"https://example.invalid/callback\"\n";

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "tb-config-edit-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        std::fs::write(directory.join("bot.toml"), CONFIG).unwrap();
        Self(directory)
    }
    fn path(&self) -> PathBuf {
        self.0.join("bot.toml")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn speichern_aendert_nur_erlaubte_werte_und_nicht_aktive_momentaufnahme() {
    let fixture = Fixture::new();
    let active = BotConfigSnapshot::load(&fixture.path()).unwrap();
    let options = OperatingOptions {
        pool_max: 19,
        acquire_timeout_ms: 2100,
        connect_timeout_seconds: 7,
    };
    let saved = save(&fixture.path(), active.fingerprint(), &options).unwrap();
    assert_eq!(saved.settings().database.pool_max, 19);
    assert_eq!(active.settings().database.pool_max, 10);
    assert_eq!(
        saved.settings().twitch.bot_user_id,
        active.settings().twitch.bot_user_id
    );
    assert_eq!(
        saved.fingerprint(),
        BotConfigSnapshot::load(&fixture.path())
            .unwrap()
            .fingerprint()
    );
    assert!(matches!(
        save(&fixture.path(), active.fingerprint(), &options),
        Err(EditError::Conflict)
    ));
}

#[test]
fn ungueltiger_wert_laesst_datei_unveraendert() {
    let fixture = Fixture::new();
    let original = BotConfigSnapshot::load(&fixture.path()).unwrap();
    let options = OperatingOptions {
        pool_max: 0,
        acquire_timeout_ms: 2100,
        connect_timeout_seconds: 7,
    };
    assert!(matches!(
        save(&fixture.path(), original.fingerprint(), &options),
        Err(EditError::Invalid(_))
    ));
    assert_eq!(std::fs::read_to_string(fixture.path()).unwrap(), CONFIG);
}

#[test]
fn git_checkout_ist_keine_betriebsablage() {
    let fixture = Fixture::new();
    std::fs::write(fixture.0.join(".git"), "gitdir: fixture").unwrap();
    let original = BotConfigSnapshot::load(&fixture.path()).unwrap();
    let options = OperatingOptions::from(&original.settings().database);
    assert!(matches!(
        save(&fixture.path(), original.fingerprint(), &options),
        Err(EditError::UnsafeLocation)
    ));
}

#[cfg(unix)]
#[test]
fn dateirechte_und_gruppe_bleiben_erhalten_symlinks_abgewiesen() {
    use std::os::unix::fs::{symlink, MetadataExt, PermissionsExt};
    let fixture = Fixture::new();
    std::fs::set_permissions(fixture.path(), std::fs::Permissions::from_mode(0o640)).unwrap();
    let before = std::fs::metadata(fixture.path()).unwrap();
    let original = BotConfigSnapshot::load(&fixture.path()).unwrap();
    let options = OperatingOptions::from(&original.settings().database);
    save(&fixture.path(), original.fingerprint(), &options).unwrap();
    let after = std::fs::metadata(fixture.path()).unwrap();
    assert_eq!(after.permissions().mode(), before.permissions().mode());
    assert_eq!(after.gid(), before.gid());
    assert_eq!(after.uid(), before.uid());
    let link = fixture.0.join("link.toml");
    symlink(fixture.path(), &link).unwrap();
    assert!(matches!(
        save(&link, original.fingerprint(), &options),
        Err(EditError::UnsafeLocation)
    ));
}
