use super::*;
use serde_json::json;

const CONFIG: &str = r#"schema_version=1
[twitch]
bot_user_id="1"
notify_channel_id="2"
eventsub_callback_url="https://example.invalid/callback"
[bot]
chat_enabled=false # Chat bleibt aus
auto_raid_offline_grace_seconds=30
[bot.brain_client]
mode="legacy" # Ausgangsmodus
timeout_ms=8000
[database]
pool_max=17
"#;

fn request(text: &str, changes: Value) -> ChangeRequest {
    serde_json::from_value(json!({
        "revision": crate::editor::revision(text),
        "changes": changes,
    }))
    .expect("request")
}

fn apply(changes: Value) -> Result<String, CatalogError> {
    apply_changes(CONFIG, &request(CONFIG, changes))
}

fn segment_is_secret(segment: &str) -> bool {
    const EXACT: &[&str] = &["token", "key", "secret", "password", "dsn", "webhook"];
    const SUFFIXES: &[&str] = &["_token", "_secret", "_password", "_dsn", "_key", "_webhook"];
    const PARTS: &[&str] = &[
        "secret",
        "password",
        "passwd",
        "credential",
        "dsn",
        "webhook",
        "api_key",
        "apikey",
        "private_key",
        "client_secret",
        "access_token",
        "refresh_token",
    ];
    EXACT.contains(&segment)
        || SUFFIXES.iter().any(|suffix| segment.ends_with(suffix))
        || PARTS.iter().any(|part| segment.contains(part))
}

#[test]
fn katalog_enthaelt_keine_geheimnisse_und_keine_datenbankverbindung() {
    for field in fields() {
        let lower = field.path.to_ascii_lowercase();
        assert!(
            !lower.split('.').any(segment_is_secret),
            "geheimnisartiger Pfad {}",
            field.path
        );
        assert!(!lower.starts_with("database.") || lower.starts_with("database.retry."));
        assert!(!lower.ends_with("_site_key") && !lower.contains("stripe"));
    }
    assert!(segment_is_secret("client_secret"));
    assert!(segment_is_secret("database_dsn"));
    assert!(segment_is_secret("webhook_url"));
    assert!(!segment_is_secret("max_input_tokens"));
}

#[test]
fn katalog_ist_eindeutig_und_vollstaendig_beschrieben() {
    let mut paths = BTreeSet::new();
    let group_ids: BTreeSet<_> = groups().iter().map(|group| group.id).collect();
    for field in fields() {
        assert!(paths.insert(field.path), "doppelt {}", field.path);
        assert!(group_ids.contains(field.group), "Gruppe {}", field.path);
        assert!(!field.label.is_empty() && !field.help.is_empty());
        assert!(field.restart_required);
        match field.kind {
            Kind::Integer | Kind::IntegerList => {
                assert!(field.min.is_some() && field.max.is_some(), "{}", field.path)
            }
            Kind::Choice => assert!(!field.choices.is_empty(), "{}", field.path),
            _ => {}
        }
    }
    for group in groups() {
        assert!(fields().iter().any(|field| field.group == group.id));
    }
}

fn sample(field: &Field) -> toml::Value {
    let integer = || {
        field
            .min
            .as_deref()
            .and_then(|min| min.parse::<i64>().ok())
            .map(toml::Value::Integer)
            .expect("min")
    };
    let items = field.min_items.unwrap_or(1).max(1);
    match field.kind {
        Kind::Boolean => toml::Value::Boolean(true),
        Kind::Integer => integer(),
        Kind::Number => toml::Value::Float(
            field
                .min
                .as_deref()
                .and_then(|min| min.parse().ok())
                .expect("min"),
        ),
        Kind::String => toml::Value::String("1".to_owned()),
        Kind::Choice => toml::Value::String(field.choices[0].to_owned()),
        Kind::StringList => toml::Value::Array(vec![toml::Value::String("de".to_owned()); items]),
        Kind::IntegerList => toml::Value::Array(vec![integer(); items]),
    }
}

#[test]
fn jeder_katalogpfad_ausserhalb_von_social_ai_ist_ein_bekanntes_feld() {
    let base: toml::Table = CONFIG.parse().expect("config");
    for field in fields()
        .iter()
        .filter(|f| !f.path.starts_with("social_ai."))
    {
        let mut document = base.clone();
        let parts: Vec<&str> = field.path.split('.').collect();
        let (leaf, parents) = parts.split_last().expect("path");
        let mut table = &mut document;
        for part in parents {
            table = table
                .entry(part.to_string())
                .or_insert_with(|| toml::Value::Table(toml::Table::new()))
                .as_table_mut()
                .expect("table");
        }
        table.insert(leaf.to_string(), sample(field));
        let text = toml::to_string(&document).expect("text");
        assert!(
            toml::from_str::<BotConfig>(&text).is_ok(),
            "unbekannt oder falscher Typ: {}",
            field.path
        );
    }
}

#[test]
fn lesen_liefert_ganzzahlen_als_text_und_fehlende_werte_als_null() {
    let catalog = read_catalog(CONFIG).expect("catalog");
    assert_eq!(catalog.revision, crate::editor::revision(CONFIG));
    assert_eq!(catalog.values["bot.brain_client.timeout_ms"], json!("8000"));
    assert_eq!(catalog.values["bot.chat_enabled"], json!(false));
    assert_eq!(catalog.values["bot.brain_client.mode"], json!("legacy"));
    assert_eq!(catalog.values["bot.mcp_port"], Value::Null);
    assert_eq!(catalog.values.len(), fields().len());
    let wire = serde_json::to_value(&catalog).expect("json");
    assert_eq!(wire["fields"][0]["kind"], json!("string"));
}

#[test]
fn aenderung_erhaelt_kommentare_und_ist_wieder_lesbar() {
    let candidate = apply(json!([
        {"path": "bot.brain_client.mode", "value": "shadow"},
        {"path": "bot.brain_client.endpoint", "value": "http://127.0.0.1:8789"},
        {"path": "bot.brain_client.public_scopes", "value": ["bot.public"]},
        {"path": "bot.chat_enabled", "value": true},
        {"path": "stt.threads", "value": "8"}
    ]))
    .expect("candidate");
    assert!(candidate.contains("mode=\"shadow\" # Ausgangsmodus"));
    assert!(candidate.contains("chat_enabled=true # Chat bleibt aus"));
    assert!(candidate.contains("timeout_ms=8000\n") && candidate.contains("pool_max=17\n"));
    let catalog = read_catalog(&candidate).expect("catalog");
    assert_eq!(catalog.values["bot.brain_client.mode"], json!("shadow"));
    assert_eq!(catalog.values["stt.threads"], json!("8"));
    assert_eq!(catalog.values["bot.brain_client.timeout_ms"], json!("8000"));
}

#[test]
fn groesste_ganzzahl_bleibt_exakt() {
    let maximum = i64::MAX.to_string();
    let candidate = apply(json!([
        {"path": "bot.auto_raid_offline_grace_seconds", "value": maximum}
    ]))
    .expect("candidate");
    let catalog = read_catalog(&candidate).expect("catalog");
    assert_eq!(
        catalog.values["bot.auto_raid_offline_grace_seconds"],
        json!(i64::MAX.to_string())
    );
    assert_eq!(
        apply(json!([{"path": "bot.auto_raid_offline_grace_seconds", "value": 30}])),
        Err(CatalogError::InvalidValue(
            "bot.auto_raid_offline_grace_seconds"
        ))
    );
    assert_eq!(
        apply(
            json!([{"path": "bot.auto_raid_offline_grace_seconds", "value": "9223372036854775808"}])
        ),
        Err(CatalogError::InvalidValue(
            "bot.auto_raid_offline_grace_seconds"
        ))
    );
    assert_eq!(
        apply(json!([{"path": "bot.auto_raid_offline_grace_seconds", "value": "+5"}])),
        Err(CatalogError::InvalidValue(
            "bot.auto_raid_offline_grace_seconds"
        ))
    );
}

#[test]
fn null_entfernt_den_schluessel_und_leere_tabellen() {
    let candidate = apply(json!([
        {"path": "bot.brain_client.mode", "value": null},
        {"path": "bot.brain_client.timeout_ms", "value": null}
    ]))
    .expect("candidate");
    assert!(!candidate.contains("timeout_ms"));
    assert!(!candidate.contains("brain_client"));
    let catalog = read_catalog(&candidate).expect("catalog");
    assert_eq!(catalog.values["bot.brain_client.mode"], Value::Null);
    assert_eq!(
        apply(json!([{"path": "twitch.bot_user_id", "value": null}])),
        Err(CatalogError::InvalidValue("twitch.bot_user_id"))
    );
}

#[test]
fn null_und_leere_liste_bleiben_verschieden() {
    let path = "discord.raid_oauth.allowed_guild_ids";
    let empty = apply(json!([{"path": path, "value": []}])).expect("empty");
    assert_eq!(
        read_catalog(&empty).expect("catalog").values[path],
        json!([])
    );
    let filled = apply(json!([{"path": path, "value": ["9223372036854775807"]}])).expect("filled");
    assert_eq!(
        read_catalog(&filled).expect("catalog").values[path],
        json!(["9223372036854775807"])
    );
    let removed = apply_changes(
        &filled,
        &request(&filled, json!([{"path": path, "value": null}])),
    )
    .expect("removed");
    assert_eq!(
        read_catalog(&removed).expect("catalog").values[path],
        Value::Null
    );
}

#[test]
fn grenzen_auswahl_und_typen_werden_abgelehnt() {
    for (path, value) in [
        ("stt.threads", json!("0")),
        ("stt.threads", json!("65")),
        ("bot.brain_client.mode", json!("aus")),
        ("bot.chat_enabled", json!("true")),
        ("stt.no_speech_max", json!(1.5)),
        ("stt.no_speech_max", json!(9007199254740993_u64)),
        ("twitch.language_filters", json!([])),
        ("challenges.points.clip_top3", json!(["1", "2"])),
        ("knowledge.directory", json!("a\nb")),
    ] {
        assert_eq!(
            apply(json!([{"path": path, "value": value}])),
            Err(CatalogError::InvalidValue(field(path).expect("field").path)),
            "{path}"
        );
    }
}

#[test]
fn unbekannte_pfade_und_fehlerhafte_anfragen_werden_abgelehnt() {
    for path in [
        "database.dsn",
        "twitch.client_secret",
        "bot",
        "database.pool_max",
        "social_ai",
    ] {
        assert_eq!(
            apply(json!([{"path": path, "value": "1"}])),
            Err(CatalogError::UnknownPath),
            "{path}"
        );
    }
    assert_eq!(apply(json!([])), Err(CatalogError::InvalidRequest));
    assert_eq!(
        apply(json!([
            {"path": "stt.threads", "value": "2"},
            {"path": "stt.threads", "value": "3"}
        ])),
        Err(CatalogError::InvalidRequest)
    );
    assert!(serde_json::from_value::<ChangeRequest>(
        json!({"revision": "x", "changes": [{"path": "stt.threads"}]})
    )
    .is_err());
}

#[test]
fn veraltete_revision_und_gesamtpruefung_greifen() {
    let stale = ChangeRequest {
        revision: "0".repeat(64),
        changes: Vec::new(),
    };
    assert_eq!(
        apply_changes(CONFIG, &stale),
        Err(CatalogError::StaleRevision)
    );
    assert!(matches!(
        apply(json!([{"path": "twitch.bot_user_id", "value": "kein-wert"}])),
        Err(CatalogError::Rejected(_))
    ));
}
