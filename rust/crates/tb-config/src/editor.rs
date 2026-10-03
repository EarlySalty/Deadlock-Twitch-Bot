use crate::{
    dashboard_options::BrainClientMode,
    file::{ErrorKind, FileError, Schema, MAX_CONFIG_BYTES},
    global::Database,
    BotConfigSnapshot,
};
use serde::{Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, Metadata, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

pub const BRAIN_CONFIG_PATH: &str = "/var/lib/deadlock-twitch/config/bot.toml";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OperatingOptions {
    pub pool_max: u32,
    pub acquire_timeout_ms: u64,
    pub connect_timeout_seconds: u64,
}

impl From<&Database> for OperatingOptions {
    fn from(value: &Database) -> Self {
        Self {
            pool_max: value.pool_max,
            acquire_timeout_ms: value.acquire_timeout_ms,
            connect_timeout_seconds: value.connect_timeout_seconds,
        }
    }
}

fn supplied<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(deserializer).map(Some)
}

#[derive(Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrainClientPatch {
    #[serde(default, deserialize_with = "supplied")]
    pub mode: Option<BrainClientMode>,
    #[serde(default, deserialize_with = "supplied")]
    pub endpoint: Option<String>,
}

#[derive(Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrainPatch {
    #[serde(default, deserialize_with = "supplied")]
    pub bot_brain_client: Option<BrainClientPatch>,
    #[serde(default, deserialize_with = "supplied")]
    pub bot_brain_chat_enabled: Option<bool>,
    #[serde(default, deserialize_with = "supplied")]
    pub dashboard_brain_client: Option<BrainClientPatch>,
}

#[derive(Serialize)]
pub struct BrainClientInspection {
    mode: BrainClientMode,
    endpoint: Option<String>,
}

#[derive(Serialize)]
pub struct BrainInspection {
    revision: String,
    bot_brain_client: BrainClientInspection,
    bot_brain_chat_enabled: bool,
    dashboard_brain_client: BrainClientInspection,
}

#[derive(Debug)]
pub enum EditError {
    Invalid(FileError),
    Conflict,
    Busy,
    UnsafeLocation,
    Io,
}

pub struct SavedConfig {
    pub snapshot: BotConfigSnapshot,
    pub revision: String,
}

fn revision(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn load_saved(source: &Path) -> Result<SavedConfig, FileError> {
    let (snapshot, text) = BotConfigSnapshot::load_document(source)?;
    Ok(SavedConfig {
        snapshot,
        revision: revision(&text),
    })
}

impl From<FileError> for EditError {
    fn from(value: FileError) -> Self {
        Self::Invalid(value)
    }
}

fn regular_path(path: &Path) -> Result<PathBuf, EditError> {
    if !path.is_absolute() {
        return Err(EditError::UnsafeLocation);
    }
    let mut resolved = PathBuf::new();
    for component in path.components() {
        match component {
            Component::RootDir | Component::Normal(_) => resolved.push(component),
            _ => return Err(EditError::UnsafeLocation),
        }
        let metadata = fs::symlink_metadata(&resolved).map_err(|_| EditError::Io)?;
        if metadata.file_type().is_symlink() {
            return Err(EditError::UnsafeLocation);
        }
    }
    let metadata = fs::symlink_metadata(&resolved).map_err(|_| EditError::Io)?;
    if !metadata.is_file() {
        return Err(EditError::UnsafeLocation);
    }
    Ok(resolved)
}

fn persistent_path(path: &Path) -> Result<PathBuf, EditError> {
    let canonical = regular_path(path)?;
    if canonical
        .ancestors()
        .any(|parent| parent.join(".git").exists())
    {
        return Err(EditError::UnsafeLocation);
    }
    Ok(canonical)
}

fn read_saved(source: &Path) -> Result<(SavedConfig, String, Metadata), EditError> {
    regular_path(source)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(source).map_err(|_| EditError::Io)?;
    let metadata = file.metadata().map_err(|_| EditError::Io)?;
    if !metadata.is_file() {
        return Err(EditError::UnsafeLocation);
    }
    if metadata.len() > MAX_CONFIG_BYTES {
        return Err(FileError::new(ErrorKind::FileTooLarge).into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_CONFIG_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| EditError::Io)?;
    if bytes.len() as u64 > MAX_CONFIG_BYTES {
        return Err(FileError::new(ErrorKind::FileTooLarge).into());
    }
    let text = String::from_utf8(bytes).map_err(|_| FileError::new(ErrorKind::InvalidEncoding))?;
    let snapshot = BotConfigSnapshot::parse(&text, source)?;
    Ok((
        SavedConfig {
            snapshot,
            revision: revision(&text),
        },
        text,
        metadata,
    ))
}

fn inspect_saved(saved: &SavedConfig) -> Result<BrainInspection, EditError> {
    let settings = saved.snapshot.settings();
    let bot = &settings.bot.brain_client;
    let dashboard = &settings.dashboard.options.brain_client;
    for (endpoint, field) in [
        (&bot.endpoint, "bot.brain_client.endpoint"),
        (
            &dashboard.endpoint,
            "dashboard.options.brain_client.endpoint",
        ),
    ] {
        if let Some(endpoint) = endpoint {
            crate::global::public_url(endpoint, field, true)?;
        }
    }
    Ok(BrainInspection {
        revision: saved.revision.clone(),
        bot_brain_client: BrainClientInspection {
            mode: bot.mode,
            endpoint: bot.endpoint.clone(),
        },
        bot_brain_chat_enabled: settings.bot.brain_chat.enabled,
        dashboard_brain_client: BrainClientInspection {
            mode: dashboard.mode,
            endpoint: dashboard.endpoint.clone(),
        },
    })
}

pub fn inspect_brain(source: &Path) -> Result<BrainInspection, EditError> {
    inspect_saved(&read_saved(source)?.0)
}

pub fn save_brain(
    expected_revision: &str,
    patch: &BrainPatch,
) -> Result<BrainInspection, EditError> {
    save_brain_at(Path::new(BRAIN_CONFIG_PATH), expected_revision, patch)
}

fn save_brain_at(
    source: &Path,
    expected_revision: &str,
    patch: &BrainPatch,
) -> Result<BrainInspection, EditError> {
    if expected_revision.len() != 64
        || !expected_revision
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(FileError::invalid("expected_revision").into());
    }
    if patch.bot_brain_chat_enabled.is_none()
        && patch.bot_brain_client.is_none()
        && patch.dashboard_brain_client.is_none()
    {
        return Err(FileError::invalid("brain_patch").into());
    }
    for (client, field) in [
        (&patch.bot_brain_client, "bot.brain_client.endpoint"),
        (
            &patch.dashboard_brain_client,
            "dashboard.options.brain_client.endpoint",
        ),
    ] {
        if let Some(client) = client {
            if client.mode.is_none() && client.endpoint.is_none() {
                return Err(FileError::invalid("brain_patch").into());
            }
            if let Some(endpoint) = &client.endpoint {
                crate::global::public_url(endpoint, field, true)?;
            }
        }
    }
    let saved = save_with(source, expected_revision, |current, document| {
        inspect_saved(current)?;
        for (client, path) in [
            (&patch.bot_brain_client, &["bot", "brain_client"][..]),
            (
                &patch.dashboard_brain_client,
                &["dashboard", "options", "brain_client"][..],
            ),
        ] {
            if let Some(client) = client {
                if let Some(mode) = client.mode {
                    let mode = match mode {
                        BrainClientMode::Legacy => "legacy",
                        BrainClientMode::Shadow => "shadow",
                        BrainClientMode::Typed => "typed",
                    };
                    replace_value(document, path, "mode", toml_edit::value(mode))?;
                }
                if let Some(endpoint) = &client.endpoint {
                    replace_value(document, path, "endpoint", toml_edit::value(endpoint))?;
                }
            }
        }
        if let Some(enabled) = patch.bot_brain_chat_enabled {
            replace_value(
                document,
                &["bot", "brain_chat"],
                "enabled",
                toml_edit::value(enabled),
            )?;
        }
        Ok(())
    })?;
    inspect_saved(&saved)
}

fn replace_value(
    document: &mut toml_edit::DocumentMut,
    path: &[&str],
    key: &str,
    mut value: toml_edit::Item,
) -> Result<(), EditError> {
    let mut item = document.as_item_mut();
    for segment in path {
        let inline = item.is_inline_table();
        let table = item.as_table_like_mut().ok_or(EditError::Io)?;
        if !table.contains_key(segment) {
            let child = if inline {
                toml_edit::Item::Value(toml_edit::Value::InlineTable(toml_edit::InlineTable::new()))
            } else {
                let mut child = toml_edit::Table::new();
                child.set_implicit(true);
                toml_edit::Item::Table(child)
            };
            table.insert(segment, child);
        }
        item = table.get_mut(segment).ok_or(EditError::Io)?;
    }
    let table = item.as_table_like_mut().ok_or(EditError::Io)?;
    if let (Some(previous), Some(next)) = (
        table.get(key).and_then(toml_edit::Item::as_value),
        value.as_value_mut(),
    ) {
        *next.decor_mut() = previous.decor().clone();
    }
    table.insert(key, value);
    Ok(())
}

pub fn save(
    source: &Path,
    expected_revision: &str,
    options: &OperatingOptions,
) -> Result<SavedConfig, EditError> {
    save_with(source, expected_revision, |current, document| {
        let mut candidate = current.snapshot.settings().clone();
        candidate.database.pool_max = options.pool_max;
        candidate.database.acquire_timeout_ms = options.acquire_timeout_ms;
        candidate.database.connect_timeout_seconds = options.connect_timeout_seconds;
        candidate.validate()?;
        for (key, value) in [
            ("pool_max", i64::from(options.pool_max)),
            ("acquire_timeout_ms", options.acquire_timeout_ms as i64),
            (
                "connect_timeout_seconds",
                options.connect_timeout_seconds as i64,
            ),
        ] {
            replace_value(document, &["database"], key, toml_edit::value(value))?;
        }
        Ok(())
    })
}

fn open_lock(directory: &Path, metadata: &Metadata) -> Result<File, EditError> {
    let path = directory.join(".bot.toml.lock");
    let mut options = OpenOptions::new();
    options.write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let lock = match options.open(&path) {
        Ok(lock) => lock,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mut create = options.clone();
            create.create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                create.mode(0o600);
            }
            match create.open(&path) {
                Ok(lock) => {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::{fchown, MetadataExt, PermissionsExt};
                        fchown(&lock, Some(metadata.uid()), Some(metadata.gid()))
                            .map_err(|_| EditError::Io)?;
                        let group = if metadata.mode() & 0o040 != 0 {
                            0o060
                        } else {
                            0
                        };
                        lock.set_permissions(fs::Permissions::from_mode(0o600 | group))
                            .map_err(|_| EditError::Io)?;
                    }
                    lock
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    options.open(&path).map_err(|_| EditError::Io)?
                }
                Err(_) => return Err(EditError::Io),
            }
        }
        Err(_) => return Err(EditError::Io),
    };
    let lock_metadata = lock.metadata().map_err(|_| EditError::Io)?;
    if !lock_metadata.is_file() {
        return Err(EditError::UnsafeLocation);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if lock_metadata.nlink() != 1 {
            return Err(EditError::UnsafeLocation);
        }
        let accessible = if lock_metadata.uid() == metadata.uid() {
            lock_metadata.mode() & 0o600 == 0o600
        } else {
            lock_metadata.gid() == metadata.gid() && lock_metadata.mode() & 0o060 == 0o060
        };
        if !accessible {
            return Err(EditError::Io);
        }
    }
    Ok(lock)
}

fn same_metadata(left: &Metadata, right: &Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        left.dev() == right.dev()
            && left.ino() == right.ino()
            && left.uid() == right.uid()
            && left.gid() == right.gid()
            && left.mode() == right.mode()
            && left.nlink() == right.nlink()
    }
    #[cfg(not(unix))]
    {
        left.permissions().readonly() == right.permissions().readonly() && left.len() == right.len()
    }
}

fn save_with(
    source: &Path,
    expected_revision: &str,
    update: impl FnOnce(&SavedConfig, &mut toml_edit::DocumentMut) -> Result<(), EditError>,
) -> Result<SavedConfig, EditError> {
    let source = persistent_path(source)?;
    let directory = source.parent().ok_or(EditError::UnsafeLocation)?;
    let initial = fs::symlink_metadata(&source).map_err(|_| EditError::Io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if initial.nlink() != 1 {
            return Err(EditError::UnsafeLocation);
        }
    }
    let lock = open_lock(directory, &initial)?;
    lock.try_lock().map_err(|error| match error {
        std::fs::TryLockError::WouldBlock => EditError::Busy,
        std::fs::TryLockError::Error(_) => EditError::Io,
    })?;
    let (current, original, metadata) = read_saved(&source)?;
    if current.revision != expected_revision || !same_metadata(&initial, &metadata) {
        return Err(EditError::Conflict);
    }
    let mut document = original
        .parse::<toml_edit::DocumentMut>()
        .map_err(|_| EditError::Io)?;
    update(&current, &mut document)?;
    let text = document.to_string();
    let checked = BotConfigSnapshot::parse(&text, &source)?;
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let temporary = directory.join(format!(
        ".bot.toml.{}.{}.tmp",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let mut create = OpenOptions::new();
    create.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        create.mode(0o600);
    }
    let mut file = create.open(&temporary).map_err(|_| EditError::Io)?;
    let result = (|| {
        file.write_all(text.as_bytes()).map_err(|_| EditError::Io)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::{fchown, MetadataExt};
            fchown(&file, Some(metadata.uid()), Some(metadata.gid())).map_err(|_| EditError::Io)?;
        }
        file.set_permissions(metadata.permissions())
            .map_err(|_| EditError::Io)?;
        file.sync_all().map_err(|_| EditError::Io)?;
        let (latest, _, latest_metadata) = read_saved(&source)?;
        if latest.revision != expected_revision || !same_metadata(&metadata, &latest_metadata) {
            return Err(EditError::Conflict);
        }
        let named_lock =
            fs::symlink_metadata(directory.join(".bot.toml.lock")).map_err(|_| EditError::Io)?;
        if !same_metadata(&lock.metadata().map_err(|_| EditError::Io)?, &named_lock) {
            return Err(EditError::UnsafeLocation);
        }
        fs::rename(&temporary, &source).map_err(|_| EditError::Io)?;
        File::open(directory)
            .and_then(|f| f.sync_all())
            .map_err(|_| EditError::Io)?;
        Ok(SavedConfig {
            snapshot: checked,
            revision: revision(&text),
        })
    })();
    if temporary.exists() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod brain_tests {
    use super::*;

    const CONFIG: &str = r#"schema_version=1
[twitch]
bot_user_id="1"
notify_channel_id="2"
eventsub_callback_url="https://example.invalid/callback"
[bot]
chat_enabled=false
[bot.brain_client]
mode="legacy" # Ausgangsmodus
endpoint="http://127.0.0.1:8789"
public_scopes=["bot.public"]
timeout_ms=8000
[bot.brain_chat]
enabled=false
user_cooldown_seconds=90
channel_hourly_limit=30
global_daily_limit=600
[dashboard.options]
noauth_readiness=true
[dashboard.options.brain_client]
mode="legacy"
endpoint="http://127.0.0.1:8789"
public_scopes=["bot.public"]
timeout_ms=10000
[database]
pool_max=17
"#;

    struct Fixture(PathBuf);

    impl Fixture {
        fn new(text: &str) -> Self {
            static SEQ: AtomicU64 = AtomicU64::new(0);
            let directory = std::env::temp_dir().join(format!(
                "tb-brain-edit-{}-{}",
                std::process::id(),
                SEQ.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&directory).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
            }
            fs::write(directory.join("bot.toml"), text).unwrap();
            Self(directory)
        }

        fn path(&self) -> PathBuf {
            self.0.join("bot.toml")
        }

        fn apply(&self, input: &str) -> Result<BrainInspection, EditError> {
            let hash = load_saved(&self.path()).unwrap().revision;
            save_brain_at(&self.path(), &hash, &serde_json::from_str(input).unwrap())
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn unselected(snapshot: &BotConfigSnapshot) -> serde_json::Value {
        let mut value = serde_json::to_value(snapshot.settings()).unwrap();
        for path in ["/bot/brain_client", "/dashboard/options/brain_client"] {
            let client = value.pointer_mut(path).unwrap().as_object_mut().unwrap();
            client.remove("mode");
            client.remove("endpoint");
        }
        value["bot"]["brain_chat"]
            .as_object_mut()
            .unwrap()
            .remove("enabled");
        value
    }

    #[test]
    fn brain_aendert_fuenf_felder_und_erhaelt_den_rest() {
        let fixture = Fixture::new(CONFIG);
        let original = load_saved(&fixture.path()).unwrap();
        let inspection = fixture.apply(r#"{"bot_brain_client":{"mode":"typed","endpoint":"http://127.0.0.1:8788"},"bot_brain_chat_enabled":true,"dashboard_brain_client":{"mode":"shadow","endpoint":"http://127.0.0.1:8788"}}"#).unwrap();
        let saved = load_saved(&fixture.path()).unwrap();
        assert_ne!(inspection.revision, original.revision);
        assert_eq!(inspection.revision, saved.revision);
        assert_eq!(
            saved.snapshot.settings().bot.brain_client.mode,
            BrainClientMode::Typed
        );
        assert_eq!(
            saved
                .snapshot
                .settings()
                .dashboard
                .options
                .brain_client
                .mode,
            BrainClientMode::Shadow
        );
        assert_eq!(
            saved
                .snapshot
                .settings()
                .bot
                .brain_client
                .endpoint
                .as_deref(),
            Some("http://127.0.0.1:8788")
        );
        assert!(saved.snapshot.settings().bot.brain_chat.enabled);
        assert_eq!(unselected(&original.snapshot), unselected(&saved.snapshot));
        assert!(fs::read_to_string(fixture.path())
            .unwrap()
            .contains("# Ausgangsmodus"));
        assert_eq!(
            original.snapshot.settings().bot.brain_client.mode,
            BrainClientMode::Legacy
        );
    }

    #[test]
    fn partielle_aenderung_erhaelt_nicht_ausgewaehlte_brain_felder() {
        let fixture = Fixture::new(CONFIG);
        let original = load_saved(&fixture.path()).unwrap();
        fixture.apply(r#"{"bot_brain_chat_enabled":true}"#).unwrap();
        let saved = load_saved(&fixture.path()).unwrap();
        let mut before = serde_json::to_value(original.snapshot.settings()).unwrap();
        before["bot"]["brain_chat"]["enabled"] = true.into();
        assert_eq!(
            before,
            serde_json::to_value(saved.snapshot.settings()).unwrap()
        );
        fixture
            .apply(r#"{"bot_brain_client":{"mode":"typed"}}"#)
            .unwrap();
        assert_eq!(
            load_saved(&fixture.path())
                .unwrap()
                .snapshot
                .settings()
                .bot
                .brain_client
                .endpoint
                .as_deref(),
            Some("http://127.0.0.1:8789")
        );
        fixture
            .apply(r#"{"bot_brain_client":{"mode":"legacy"}}"#)
            .unwrap();
        assert_eq!(
            load_saved(&fixture.path())
                .unwrap()
                .snapshot
                .settings()
                .bot
                .brain_client
                .mode,
            BrainClientMode::Legacy
        );
    }

    #[test]
    fn patch_verweigert_unbekannte_felder_typfehler_und_null() {
        for input in [
            r#"{"database":{}}"#,
            r#"{"bot_brain_client":{"public_scopes":["other"]}}"#,
            r#"{"dashboard_brain_client":{"timeout_ms":1}}"#,
            r#"{"bot_brain_client":{"mode":"remote"}}"#,
            r#"{"bot_brain_chat_enabled":"true"}"#,
            r#"{"bot_brain_chat_enabled":null}"#,
            r#"{"bot_brain_client":null}"#,
            r#"{"dashboard_brain_client":{"endpoint":null}}"#,
            r#"{"bot_brain_client":{"mode":null}}"#,
            r#"{"bot_brain_chat_enabled":true,"bot_brain_chat_enabled":false}"#,
        ] {
            assert!(serde_json::from_str::<BrainPatch>(input).is_err());
        }
        let fixture = Fixture::new(CONFIG);
        for input in [r#"{}"#, r#"{"bot_brain_client":{}}"#] {
            assert!(matches!(fixture.apply(input), Err(EditError::Invalid(_))));
            assert_eq!(fs::read_to_string(fixture.path()).unwrap(), CONFIG);
        }
    }

    #[test]
    fn endpoints_bleiben_lokal_und_ohne_eingebettete_zugangsdaten() {
        let fixture = Fixture::new(CONFIG);
        for endpoint in [
            "https://example.invalid",
            "http://localhost:8788",
            "http://user:synthetic-password@127.0.0.1:8788",
            "http://127.0.0.1:8788?key=synthetic-password",
            "http://127.0.0.1:8788#synthetic-password",
            "http://127.0.0.1:0",
            "unix:///tmp/brain.sock",
        ] {
            for field in ["bot_brain_client", "dashboard_brain_client"] {
                let input = serde_json::json!({field: {"endpoint": endpoint}}).to_string();
                assert!(matches!(fixture.apply(&input), Err(EditError::Invalid(_))));
                assert_eq!(fs::read_to_string(fixture.path()).unwrap(), CONFIG);
            }
        }
        fixture
            .apply(r#"{"bot_brain_client":{"endpoint":"http://[::1]:8788"}}"#)
            .unwrap();
    }

    #[test]
    fn typed_ohne_bestehende_scopes_wird_nicht_gespeichert() {
        let text = CONFIG.replace("public_scopes=[\"bot.public\"]", "public_scopes=[]");
        let fixture = Fixture::new(&text);
        assert!(matches!(
            fixture.apply(r#"{"bot_brain_client":{"mode":"typed"}}"#),
            Err(EditError::Invalid(_))
        ));
        assert_eq!(fs::read_to_string(fixture.path()).unwrap(), text);
    }

    #[test]
    fn stale_hash_und_ungueltiger_hash_verhindern_speichern() {
        let fixture = Fixture::new(CONFIG);
        let original = load_saved(&fixture.path()).unwrap();
        let patch: BrainPatch = serde_json::from_str(r#"{"bot_brain_chat_enabled":true}"#).unwrap();
        let changed = format!("{CONFIG}\n# Neuer Hinweis\n");
        fs::write(fixture.path(), &changed).unwrap();
        assert!(matches!(
            save_brain_at(&fixture.path(), &original.revision, &patch),
            Err(EditError::Conflict)
        ));
        for hash in ["", "invalid", &"g".repeat(64)] {
            assert!(matches!(
                save_brain_at(&fixture.path(), hash, &patch),
                Err(EditError::Invalid(_))
            ));
        }
        assert_eq!(fs::read_to_string(fixture.path()).unwrap(), changed);
    }

    #[test]
    fn inspektion_gibt_nur_revision_und_freigegebene_felder_aus() {
        let fixture = Fixture::new(CONFIG);
        let value = serde_json::to_value(inspect_brain(&fixture.path()).unwrap()).unwrap();
        assert_eq!(value.as_object().unwrap().len(), 4);
        assert_eq!(
            value["revision"],
            load_saved(&fixture.path()).unwrap().revision
        );
        for field in ["bot_brain_client", "dashboard_brain_client"] {
            assert_eq!(value[field].as_object().unwrap().len(), 2);
            assert_eq!(value[field]["mode"], "legacy");
            assert_eq!(value[field]["endpoint"], "http://127.0.0.1:8789");
        }
        assert_eq!(value["bot_brain_chat_enabled"], false);
        let unsafe_fixture = Fixture::new(&CONFIG.replace(
            "http://127.0.0.1:8789",
            "http://user:synthetic-password@127.0.0.1:8789",
        ));
        assert!(matches!(
            inspect_brain(&unsafe_fixture.path()),
            Err(EditError::Invalid(_))
        ));
    }

    #[test]
    fn fehlende_tabellen_und_inline_tabellen_erhalten_fremde_werte() {
        let minimal = "schema_version=1\n[twitch]\nbot_user_id=\"1\"\nnotify_channel_id=\"2\"\neventsub_callback_url=\"https://example.invalid/callback\"\n";
        let fixture = Fixture::new(minimal);
        fixture.apply(r#"{"bot_brain_client":{"endpoint":"http://127.0.0.1:8788"},"bot_brain_chat_enabled":false}"#).unwrap();
        assert!(
            !load_saved(&fixture.path())
                .unwrap()
                .snapshot
                .settings()
                .bot
                .brain_chat
                .enabled
        );
        let inline = "schema_version=1\nbot={brain_client={mode=\"legacy\", public_scopes=[\"bot.public\"], timeout_ms=7000}, chat_enabled=false}\ndashboard={options={brain_client={mode=\"legacy\",public_scopes=[\"bot.public\"]},noauth_readiness=true}}\n[twitch]\nbot_user_id=\"1\"\nnotify_channel_id=\"2\"\neventsub_callback_url=\"https://example.invalid/callback\"\n";
        let fixture = Fixture::new(inline);
        let before = load_saved(&fixture.path()).unwrap();
        fixture.apply(r#"{"bot_brain_client":{"mode":"typed","endpoint":"http://127.0.0.1:8788"},"dashboard_brain_client":{"mode":"shadow","endpoint":"http://127.0.0.1:8788"},"bot_brain_chat_enabled":false}"#).unwrap();
        assert_eq!(
            unselected(&before.snapshot),
            unselected(&load_saved(&fixture.path()).unwrap().snapshot)
        );
    }

    #[test]
    fn bestehende_sperre_serialisiert_beide_editoren() {
        let fixture = Fixture::new(CONFIG);
        let lock = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(fixture.0.join(".bot.toml.lock"))
            .unwrap();
        lock.try_lock().unwrap();
        assert!(matches!(
            fixture.apply(r#"{"bot_brain_chat_enabled":true}"#),
            Err(EditError::Busy)
        ));
        let current = load_saved(&fixture.path()).unwrap();
        let options = OperatingOptions::from(&current.snapshot.settings().database);
        assert!(matches!(
            save(&fixture.path(), &current.revision, &options),
            Err(EditError::Busy)
        ));
        assert_eq!(fs::read_to_string(fixture.path()).unwrap(), CONFIG);
    }

    #[test]
    fn externer_schreiber_waehrend_serialisierung_wird_erkannt() {
        let fixture = Fixture::new(CONFIG);
        let hash = load_saved(&fixture.path()).unwrap().revision;
        let changed = format!("{CONFIG}\n# Externe Änderung\n");
        let result = save_with(&fixture.path(), &hash, |_, document| {
            replace_value(
                document,
                &["bot", "brain_chat"],
                "enabled",
                toml_edit::value(true),
            )?;
            fs::write(fixture.path(), &changed).unwrap();
            Ok(())
        });
        assert!(matches!(result, Err(EditError::Conflict)));
        assert_eq!(fs::read_to_string(fixture.path()).unwrap(), changed);
        assert!(!fs::read_dir(&fixture.0).unwrap().any(|entry| entry
            .unwrap()
            .path()
            .extension()
            .is_some_and(|ext| ext == "tmp")));
    }

    #[cfg(unix)]
    #[test]
    fn eigentuemer_gruppe_rechte_und_bestehende_sperre_bleiben_erhalten() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let fixture = Fixture::new(CONFIG);
        fs::set_permissions(fixture.path(), fs::Permissions::from_mode(0o640)).unwrap();
        let before = fs::metadata(fixture.path()).unwrap();
        fixture.apply(r#"{"bot_brain_chat_enabled":true}"#).unwrap();
        let after = fs::metadata(fixture.path()).unwrap();
        assert_eq!(
            (before.uid(), before.gid(), before.mode()),
            (after.uid(), after.gid(), after.mode())
        );
        let lock_before = fs::metadata(fixture.0.join(".bot.toml.lock")).unwrap();
        assert_eq!(
            (
                lock_before.uid(),
                lock_before.gid(),
                lock_before.mode() & 0o777
            ),
            (before.uid(), before.gid(), 0o660)
        );
        fixture
            .apply(r#"{"bot_brain_chat_enabled":false}"#)
            .unwrap();
        assert!(same_metadata(
            &lock_before,
            &fs::metadata(fixture.0.join(".bot.toml.lock")).unwrap()
        ));
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_und_harte_links_werden_abgewiesen() {
        use std::os::unix::fs::symlink;
        let fixture = Fixture::new(CONFIG);
        let hash = load_saved(&fixture.path()).unwrap().revision;
        let patch: BrainPatch = serde_json::from_str(r#"{"bot_brain_chat_enabled":true}"#).unwrap();
        let link = fixture.0.join("link.toml");
        symlink(fixture.path(), &link).unwrap();
        assert!(matches!(
            save_brain_at(&link, &hash, &patch),
            Err(EditError::UnsafeLocation)
        ));
        assert!(matches!(
            inspect_brain(&link),
            Err(EditError::UnsafeLocation)
        ));
        let parent = fixture.0.join("parent");
        symlink(&fixture.0, &parent).unwrap();
        assert!(matches!(
            save_brain_at(&parent.join("bot.toml"), &hash, &patch),
            Err(EditError::UnsafeLocation)
        ));
        symlink(fixture.path(), fixture.0.join(".bot.toml.lock")).unwrap();
        assert!(save_brain_at(&fixture.path(), &hash, &patch).is_err());
        fs::remove_file(fixture.0.join(".bot.toml.lock")).unwrap();
        fs::hard_link(fixture.path(), fixture.0.join("copy.toml")).unwrap();
        assert!(matches!(
            save_brain_at(&fixture.path(), &hash, &patch),
            Err(EditError::UnsafeLocation)
        ));
        assert_eq!(fs::read_to_string(fixture.path()).unwrap(), CONFIG);
    }

    #[cfg(unix)]
    #[test]
    fn sonderdateien_und_unzugaengliche_sperre_werden_abgewiesen() {
        use std::os::unix::fs::PermissionsExt;
        let fixture = Fixture::new(CONFIG);
        let hash = load_saved(&fixture.path()).unwrap().revision;
        let patch: BrainPatch = serde_json::from_str(r#"{"bot_brain_chat_enabled":true}"#).unwrap();
        let lock = fixture.0.join(".bot.toml.lock");
        fs::write(&lock, "").unwrap();
        fs::set_permissions(&lock, fs::Permissions::from_mode(0o000)).unwrap();
        assert!(save_brain_at(&fixture.path(), &hash, &patch).is_err());
        assert_eq!(fs::metadata(&lock).unwrap().permissions().mode() & 0o777, 0);
        fs::remove_file(&lock).unwrap();
        let fifo = fixture.0.join("fifo");
        let encoded = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(encoded.as_ptr(), 0o600) }, 0);
        assert!(matches!(
            inspect_brain(&fifo),
            Err(EditError::UnsafeLocation)
        ));
        assert!(matches!(
            save_brain_at(&fifo, &hash, &patch),
            Err(EditError::UnsafeLocation)
        ));
        fs::rename(&fifo, &lock).unwrap();
        assert!(save_brain_at(&fixture.path(), &hash, &patch).is_err());
        assert_eq!(fs::read_to_string(fixture.path()).unwrap(), CONFIG);
    }
}
