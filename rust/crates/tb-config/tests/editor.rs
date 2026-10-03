use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use tb_config::{
    editor::{load_saved, save, EditError, OperatingOptions},
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
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
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
    let original = load_saved(&fixture.path()).unwrap();
    let active = &original.snapshot;
    let options = OperatingOptions {
        pool_max: 19,
        acquire_timeout_ms: 2100,
        connect_timeout_seconds: 7,
    };
    let saved = save(&fixture.path(), &original.revision, &options).unwrap();
    assert_eq!(saved.snapshot.settings().database.pool_max, 19);
    assert_eq!(active.settings().database.pool_max, 10);
    assert_eq!(
        saved.snapshot.settings().twitch.bot_user_id,
        active.settings().twitch.bot_user_id
    );
    assert_eq!(
        saved.snapshot.fingerprint(),
        BotConfigSnapshot::load(&fixture.path())
            .unwrap()
            .fingerprint()
    );
    assert!(matches!(
        save(&fixture.path(), &original.revision, &options),
        Err(EditError::Conflict)
    ));
}

#[test]
fn ungueltiger_wert_laesst_datei_unveraendert() {
    let fixture = Fixture::new();
    let original = load_saved(&fixture.path()).unwrap();
    let options = OperatingOptions {
        pool_max: 0,
        acquire_timeout_ms: 2100,
        connect_timeout_seconds: 7,
    };
    assert!(matches!(
        save(&fixture.path(), &original.revision, &options),
        Err(EditError::Invalid(_))
    ));
    assert_eq!(std::fs::read_to_string(fixture.path()).unwrap(), CONFIG);
}

#[test]
fn kommentare_bleiben_erhalten_und_kommentaraenderungen_verhindern_ueberschreiben() {
    let fixture = Fixture::new();
    let document = format!("# Betreiberhinweis\n{CONFIG}\n[database]\npool_max = 10 # Reserve\n");
    std::fs::write(fixture.path(), &document).unwrap();
    let original = load_saved(&fixture.path()).unwrap();
    let options = OperatingOptions {
        pool_max: 11,
        acquire_timeout_ms: 5000,
        connect_timeout_seconds: 5,
    };
    let changed_comment = document.replace("Betreiberhinweis", "Neuer Betreiberhinweis");
    std::fs::write(fixture.path(), &changed_comment).unwrap();
    let new = load_saved(&fixture.path()).unwrap();
    assert_eq!(new.snapshot.fingerprint(), original.snapshot.fingerprint());
    assert_ne!(new.revision, original.revision);
    assert!(matches!(
        save(&fixture.path(), &original.revision, &options),
        Err(EditError::Conflict)
    ));
    assert_eq!(
        std::fs::read_to_string(fixture.path()).unwrap(),
        changed_comment
    );
    save(&fixture.path(), &new.revision, &options).unwrap();
    let saved = std::fs::read_to_string(fixture.path()).unwrap();
    assert!(saved.contains("# Neuer Betreiberhinweis"));
    assert!(saved.contains("pool_max = 11 # Reserve"));
}

#[test]
fn git_checkout_ist_keine_betriebsablage() {
    let fixture = Fixture::new();
    std::fs::write(fixture.0.join(".git"), "gitdir: fixture").unwrap();
    let original = load_saved(&fixture.path()).unwrap();
    let options = OperatingOptions::from(&original.snapshot.settings().database);
    assert!(matches!(
        save(&fixture.path(), &original.revision, &options),
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
    let original = load_saved(&fixture.path()).unwrap();
    let options = OperatingOptions::from(&original.snapshot.settings().database);
    save(&fixture.path(), &original.revision, &options).unwrap();
    let after = std::fs::metadata(fixture.path()).unwrap();
    assert_eq!(after.permissions().mode(), before.permissions().mode());
    assert_eq!(after.gid(), before.gid());
    assert_eq!(after.uid(), before.uid());
    let link = fixture.0.join("link.toml");
    symlink(fixture.path(), &link).unwrap();
    assert!(matches!(
        save(&link, &original.revision, &options),
        Err(EditError::UnsafeLocation)
    ));
}

#[test]
fn pool_editor_erhält_getrennte_retry_optionen() {
    let fixture = Fixture::new();
    std::fs::write(fixture.path(), format!("{CONFIG}\n[database.retry]\nattempts=7\nbase_delay_seconds=0.2\nmax_delay_seconds=0.9\n")).unwrap();
    let original = load_saved(&fixture.path()).unwrap();
    let mut options = OperatingOptions::from(&original.snapshot.settings().database);
    options.pool_max = 21;
    let saved = save(&fixture.path(), &original.revision, &options).unwrap();
    assert_eq!(saved.snapshot.settings().database.retry.attempts, 7);
    assert_eq!(
        saved.snapshot.settings().database.retry.base_delay_seconds,
        0.2
    );
    assert_eq!(
        saved.snapshot.settings().database.retry.max_delay_seconds,
        0.9
    );
}

#[test]
fn config_cli_bestehende_pruefung_und_begrenzte_brain_inspektion() {
    let fixture = Fixture::new();
    let check = std::process::Command::new(env!("CARGO_BIN_EXE_tb-config-check"))
        .arg("--config")
        .arg(fixture.path())
        .output()
        .unwrap();
    assert!(check.status.success());
    let expected = load_saved(&fixture.path()).unwrap();
    let check_text = String::from_utf8(check.stdout).unwrap();
    assert!(check_text.contains(expected.snapshot.fingerprint()));
    let inspect = std::process::Command::new(env!("CARGO_BIN_EXE_tb-config-check"))
        .arg("--config")
        .arg(fixture.path())
        .arg("--brain-inspect")
        .output()
        .unwrap();
    assert!(inspect.status.success());
    assert!(inspect.stderr.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&inspect.stdout).unwrap();
    assert_eq!(value.as_object().unwrap().len(), 4);
    assert_eq!(value["revision"], expected.revision);
    for field in ["bot_brain_client", "dashboard_brain_client"] {
        assert_eq!(value[field].as_object().unwrap().len(), 2);
        assert_eq!(value[field]["mode"], "legacy");
        assert!(value[field]["endpoint"].is_null());
    }
    assert!(value["bot_brain_chat_enabled"].is_boolean());
    assert_eq!(std::fs::read_to_string(fixture.path()).unwrap(), CONFIG);
    assert!(!fixture.0.join(".bot.toml.lock").exists());
}

#[test]
fn config_cli_schreibt_nicht_an_frei_gewaehlte_ziele() {
    let fixture = Fixture::new();
    let expected = load_saved(&fixture.path()).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_tb-config-check"))
        .arg("--config")
        .arg(fixture.path())
        .arg("--brain-apply")
        .arg("--expected-revision")
        .arg(&expected.revision)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(std::fs::read_to_string(fixture.path()).unwrap(), CONFIG);
    assert!(!fixture.0.join(".bot.toml.lock").exists());
}

#[test]
fn config_cli_verweigert_unbekannte_und_doppelte_argumente() {
    let fixture = Fixture::new();
    for extra in [
        vec!["--brain-inspect", "--brain-inspect"],
        vec!["--brain-apply"],
        vec!["--brain-apply", "--expected-revision"],
        vec!["synthetic-private-value"],
    ] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_tb-config-check"))
            .arg("--config")
            .arg(fixture.path())
            .args(extra)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(!String::from_utf8(output.stderr)
            .unwrap()
            .contains("synthetic-private-value"));
    }
    assert_eq!(std::fs::read_to_string(fixture.path()).unwrap(), CONFIG);
}
