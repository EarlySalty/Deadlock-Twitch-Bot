use std::path::Path;

use serde_json::{json, Value};
use tb_llm::social_editor::{fixture::SOCIAL_AI_SECTION, EditorConfig, SocialAiSection};
use tb_social_media::ai_editor::{runtime_config::MediaRuntime, AiEditorError, EditorRuntime};
use tempfile::TempDir;

fn document() -> Value {
    let mut document: Value = toml::from_str(SOCIAL_AI_SECTION).expect("social_ai fixture");
    document["runtime"]["source_root"] = json!("clips");
    document["runtime"]["work_root"] = json!("clips/.ai-editor-work");
    document["runtime"]["output_root"] = json!("clips");
    document
}

fn fixture_value(value: &Value) -> toml::Value {
    match value {
        Value::Object(fields) => toml::Value::Table(
            fields
                .iter()
                .map(|(key, value)| (key.clone(), fixture_value(value)))
                .collect(),
        ),
        Value::Array(values) => toml::Value::Array(values.iter().map(fixture_value).collect()),
        Value::String(value) => toml::Value::String(value.clone()),
        Value::Bool(value) => toml::Value::Boolean(*value),
        Value::Number(value) => value
            .as_i64()
            .map(toml::Value::Integer)
            .unwrap_or_else(|| toml::Value::Float(value.as_f64().expect("bounded fixture number"))),
        Value::Null => panic!("TOML fixtures omit null options"),
    }
}

fn section(document: &Value) -> toml::Table {
    match fixture_value(document) {
        toml::Value::Table(table) => table,
        _ => panic!("social_ai fixture must be a table"),
    }
}

struct Package {
    root: TempDir,
    section: toml::Table,
}

impl Package {
    fn path(&self) -> &Path {
        self.root.path()
    }

    fn media(&self) -> Result<MediaRuntime, AiEditorError> {
        MediaRuntime::load(self.path(), &self.section)
    }

    fn editor(&self) -> Result<EditorRuntime, AiEditorError> {
        EditorRuntime::load(self.path(), &self.section)
    }
}

fn package(document: &Value) -> Package {
    Package {
        root: tempfile::tempdir().expect("package root"),
        section: section(document),
    }
}

fn assert_configuration_error(package: &Package) {
    assert!(matches!(package.media(), Err(AiEditorError::Configuration)));
}

#[test]
fn valid_editor_media_matches_resolved_limits_and_snapshot() {
    let root = package(&document());
    assert!(!root.path().join("ops/social-ai/skills").exists());
    let media: tb_social_media::ai_editor::MediaRuntime =
        root.media().expect("independent media runtime");
    let editor = root.editor().expect("full editor runtime");
    let canonical_root = root.path().canonicalize().expect("canonical package root");
    assert_eq!(media.package_root, canonical_root);
    assert_eq!(media.source_root, canonical_root.join("clips"));
    assert_eq!(media.output_root, media.source_root);
    assert_eq!(media.work_root, media.output_root.join(".ai-editor-work"));
    assert_eq!(editor.media().package_root, editor.package_root);
    assert_eq!(editor.media().source_root, editor.limits.source_root);
    assert_eq!(editor.media().work_root, editor.limits.work_root);
    assert_eq!(editor.media().output_root, editor.limits.output_root);
    assert_eq!(editor.media().source_root, media.source_root);
    assert_eq!(editor.media().work_root, media.work_root);
    assert_eq!(editor.media().output_root, media.output_root);
    let resources = serde_json::to_value(&media.resources).expect("media resource values");
    assert_eq!(
        serde_json::to_value(&editor.media().resources).expect("editor media resource values"),
        resources
    );
    assert_eq!(
        serde_json::to_value(&editor.limits.resources).expect("editor resource values"),
        resources
    );
    assert_eq!(
        editor.configuration.runtime,
        serde_json::to_value(&editor.limits).expect("normalized runtime snapshot")
    );
    assert_eq!(
        editor.limits.stt.binary,
        canonical_root.join("rust/target/release/tb-stt-server")
    );
}

#[test]
fn media_only_document_needs_no_ai_sections() {
    let runtime = document()["runtime"].clone();
    let document = json!({
        "runtime": {
            "source_root": runtime["source_root"],
            "work_root": runtime["work_root"],
            "output_root": runtime["output_root"],
            "resources": runtime["resources"],
        },
    });
    let root = package(&document);
    root.media()
        .expect("media without provider, model, STT or skills");
    assert!(root.editor().is_err());
}

#[test]
fn invalid_ai_sections_do_not_disable_media() {
    let mut documents = Vec::new();
    let mut invalid = document();
    invalid["provider"] = json!(false);
    documents.push(invalid);
    let mut invalid = document();
    invalid["provider"]["model"] = json!("");
    documents.push(invalid);
    let mut invalid = document();
    invalid["provider"]["kind"] = json!("unavailable");
    documents.push(invalid);
    let mut invalid = document();
    invalid["runtime"]["stt"] = json!(false);
    documents.push(invalid);
    let mut invalid = document();
    invalid["runtime"]["stt"]["settings"]["port"] = json!(8791);
    documents.push(invalid);
    let mut invalid = document();
    invalid["runtime"]["plan"] = json!(false);
    documents.push(invalid);
    let mut invalid = document();
    invalid["runtime"]["brain"] = json!(false);
    documents.push(invalid);
    for invalid in documents {
        let root = package(&invalid);
        root.media().expect("media ignores AI-only values");
        assert!(root.editor().is_err());
    }
}

#[test]
fn provider_relative_resource_checks_remain_editor_only() {
    for (field, value) in [("max_frames", 9), ("max_frame_bytes", 262_144)] {
        let mut document = document();
        document["runtime"]["resources"][field] = json!(value);
        let root = package(&document);
        root.media().expect("intrinsic media resource limits");
        assert!(root.editor().is_err());
        document
            .as_object_mut()
            .expect("document object")
            .remove("pricing");
        let configuration: EditorConfig =
            serde_json::from_value(document).expect("typed editor configuration");
        assert!(EditorRuntime::from_configuration(root.path(), configuration).is_err());
    }
}

#[test]
fn editor_only_runtime_bounds_do_not_disable_media() {
    for (field, value) in [
        ("stage_attempts", 0),
        ("job_deadline_ms", 60_000),
        ("max_skill_bytes", 0),
    ] {
        let mut document = document();
        document["runtime"][field] = json!(value);
        let root = package(&document);
        root.media().expect("media ignores editor runtime bounds");
        assert!(root.editor().is_err());
    }
}

#[test]
fn invalid_media_roots_are_rejected_by_both_loaders() {
    let overlong = "x".repeat(4097);
    for (field, path) in [
        ("source_root", ""),
        ("source_root", "different"),
        ("output_root", "elsewhere"),
        ("work_root", "clips"),
        ("work_root", "clips-other/work"),
        ("work_root", "clips/../outside"),
        ("work_root", "./clips/work"),
        ("work_root", "clips/work\n"),
        ("work_root", overlong.as_str()),
    ] {
        let mut document = document();
        document["runtime"][field] = json!(path);
        let root = package(&document);
        assert_configuration_error(&root);
        assert!(root.editor().is_err(), "{field}");
    }
}

#[test]
fn intrinsic_resource_bounds_are_rejected_by_both_loaders() {
    for (field, value) in [
        ("max_source_bytes", 0),
        ("max_source_bytes", 512 * 1024 * 1024 + 1),
        ("max_output_bytes", 0),
        ("max_output_bytes", 512 * 1024 * 1024 + 1),
        ("max_probe_bytes", 1023),
        ("max_probe_bytes", 65_537),
        ("max_media_duration_ms", 0),
        ("max_media_duration_ms", 120_001),
        ("max_media_width", 1919),
        ("max_media_width", 7681),
        ("max_media_height", 1919),
        ("max_media_height", 4321),
        ("max_frames", 0),
        ("frame_width", 1),
        ("frame_width", 3),
        ("frame_width", 1922),
        ("max_frame_bytes", 0),
        ("max_ass_bytes", 1023),
        ("max_ass_bytes", 262_145),
        ("process_deadline_ms", 999),
        ("process_deadline_ms", 600_001),
        ("total_frames_deadline_ms", 999),
        ("total_frames_deadline_ms", 600_001),
        ("max_duration_drift_ms", 0),
        ("max_duration_drift_ms", 1001),
        ("encoder_threads", 0),
        ("encoder_threads", 4),
    ] {
        let mut document = document();
        document["runtime"]["resources"][field] = json!(value);
        let root = package(&document);
        assert_configuration_error(&root);
        assert!(root.editor().is_err(), "{field}={value}");
    }
}

#[test]
fn missing_or_invalid_media_fields_fail_closed() {
    let mut documents = Vec::new();
    let mut invalid = document();
    invalid
        .as_object_mut()
        .expect("document object")
        .remove("runtime");
    documents.push(invalid);
    for field in ["source_root", "work_root", "output_root", "resources"] {
        let mut invalid = document();
        invalid["runtime"]
            .as_object_mut()
            .expect("runtime object")
            .remove(field);
        documents.push(invalid);
    }
    let mut invalid = document();
    invalid["runtime"]["resources"]["unknown"] = json!(true);
    documents.push(invalid);
    let mut invalid = document();
    invalid["runtime"]["resources"]["encoder_threads"] = json!("two");
    documents.push(invalid);
    for invalid in documents {
        let root = package(&invalid);
        assert_configuration_error(&root);
    }
}

#[test]
fn missing_or_empty_section_fails_closed() {
    let root = package(&json!({}));
    assert_configuration_error(&root);
    assert!(root.editor().is_err());
}

#[test]
fn packaged_loaders_read_the_active_main_bot_configuration() {
    let root = tempfile::tempdir().expect("package root");
    let clips = root.path().join("clips");
    let mut value = document();
    value["runtime"]["source_root"] = json!(clips);
    value["runtime"]["output_root"] = json!(clips);
    value["runtime"]["work_root"] = json!(clips.join("work"));
    let mut main: toml::Table = toml::from_str(
        "schema_version=1\n[twitch]\nbot_user_id=\"1\"\nnotify_channel_id=\"2\"\neventsub_callback_url=\"https://example.invalid/callback\"\n",
    )
    .expect("main configuration");
    main.insert("social_ai".to_owned(), fixture_value(&value));
    let text = toml::to_string(&main).expect("main TOML");
    let path = root.path().join("bot.toml");
    std::fs::write(&path, &text).expect("main file");
    let snapshot = tb_config::BotConfigSnapshot::load(&path).expect("main snapshot");
    tb_config::runtime::install(snapshot).expect("active main snapshot");
    assert_eq!(
        MediaRuntime::load_packaged()
            .expect("active media")
            .source_root,
        clips
    );
    assert_eq!(
        EditorRuntime::load_packaged()
            .expect("active editor")
            .limits
            .source_root,
        clips
    );
    for invalid in [
        "[social_ai]\nprovider = [\nruntime = []",
        "[social_ai]\nschema_version=1\nschema_version=1",
        "\u{fffd}",
    ] {
        std::fs::write(&path, invalid).expect("malformed main file");
        assert!(tb_config::BotConfigSnapshot::load(&path).is_err());
    }
    std::fs::write(&path, b"\xff").expect("invalid encoding");
    assert!(tb_config::BotConfigSnapshot::load(&path).is_err());
    let oversized = format!(
        "{text}{}",
        " ".repeat(tb_config::file::MAX_CONFIG_BYTES as usize)
    );
    std::fs::write(&path, oversized).expect("oversized main file");
    assert!(tb_config::BotConfigSnapshot::load(&path).is_err());
}

#[test]
fn social_ai_section_rejects_unknown_runtime_fields_at_each_typed_boundary() {
    let raw = SOCIAL_AI_SECTION;
    for table in [
        "runtime",
        "runtime.resources",
        "runtime.plan",
        "runtime.original_preview",
        "runtime.copy",
        "runtime.stt",
        "runtime.stt.settings",
        "runtime.brain",
    ] {
        let header = format!("[{table}]");
        let unknown = if raw.contains(&header) {
            raw.replace(&header, &format!("{header}\nunknown_setting = true"))
        } else {
            format!("{raw}\n{header}\nunknown_setting = true\n")
        };
        let parsed: SocialAiSection = toml::from_str(&unknown).expect("valid outer TOML");
        let error = serde_json::from_value::<
            tb_social_media::ai_editor::runtime_config::RuntimeConfig,
        >(parsed.runtime)
        .err()
        .expect("unknown runtime field");
        assert!(error.to_string().contains("unknown field"), "{table}");
        assert!(error.to_string().contains("unknown_setting"), "{table}");
        let root = Package {
            root: tempfile::tempdir().expect("package root"),
            section: toml::from_str(&unknown).expect("unknown setting fixture"),
        };
        assert!(root.editor().is_err(), "{table}");
    }
}

#[test]
fn social_ai_section_keeps_optional_services_disabled_and_the_shared_cap() {
    let root = Package {
        root: tempfile::tempdir().expect("package root"),
        section: toml::from_str(SOCIAL_AI_SECTION).expect("social_ai fixture"),
    };
    let runtime = root.editor().expect("fixture runtime");
    assert_eq!(
        runtime.configuration.provider.initial_total_cap_micro_usd,
        2_000_000
    );
    assert!(runtime.limits.brain.is_none());
    assert!(runtime.limits.stt.settings.language.is_none());
    assert!(runtime.limits.stt.settings.remote_endpoint.is_none());
}

#[test]
fn absolute_roots_and_canonical_package_resolution_are_preserved() {
    let mut document: Value = toml::from_str(SOCIAL_AI_SECTION).expect("social_ai fixture");
    let root = package(&document);
    let media = root.media().expect("absolute media roots");
    assert_eq!(
        media.source_root,
        Path::new(
            document["runtime"]["source_root"]
                .as_str()
                .expect("source root")
        )
    );
    assert_eq!(
        media.work_root,
        Path::new(
            document["runtime"]["work_root"]
                .as_str()
                .expect("work root")
        )
    );
    document["runtime"]["source_root"] = json!("clips");
    document["runtime"]["work_root"] = json!("clips/work");
    document["runtime"]["output_root"] = json!("clips");
    let relative = section(&document);
    let link_root = tempfile::tempdir().expect("symlink parent");
    let link = link_root.path().join("package");
    std::os::unix::fs::symlink(root.path(), &link).expect("package symlink");
    let media = MediaRuntime::load(&link, &relative).expect("symlink package root");
    let editor = EditorRuntime::load(&link, &relative).expect("symlink editor root");
    assert_eq!(
        media.package_root,
        root.path().canonicalize().expect("root")
    );
    assert_eq!(media.source_root, media.package_root.join("clips"));
    assert_eq!(editor.media().source_root, media.source_root);
}
