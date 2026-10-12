use std::path::Path;

use serde_json::{json, Value};
use tb_llm::social_editor::EditorConfig;
use tb_social_media::ai_editor::{runtime_config::MediaRuntime, AiEditorError, EditorRuntime};
use tempfile::TempDir;

fn document() -> Value {
    let mut document: Value =
        serde_yaml::from_str(include_str!("../../../../ops/social-ai/editor.yaml"))
            .expect("packaged YAML");
    document["runtime"]["source_root"] = json!("clips");
    document["runtime"]["work_root"] = json!("clips/.ai-editor-work");
    document["runtime"]["output_root"] = json!("clips");
    document
}

fn write_document(root: &Path, document: &Value) {
    let directory = root.join("ops/social-ai");
    std::fs::create_dir_all(&directory).expect("configuration directory");
    std::fs::write(
        directory.join("editor.yaml"),
        serde_json::to_string(document).expect("JSON-form YAML"),
    )
    .expect("configuration file");
}

fn package(document: &Value) -> TempDir {
    let root = tempfile::tempdir().expect("package root");
    write_document(root.path(), document);
    root
}

fn assert_configuration_error(root: &Path) {
    assert!(matches!(
        MediaRuntime::load(root),
        Err(AiEditorError::Configuration)
    ));
}

#[test]
fn valid_editor_media_matches_resolved_limits_and_snapshot() {
    let root = package(&document());
    assert!(!root.path().join("ops/social-ai/skills").exists());
    let media: tb_social_media::ai_editor::MediaRuntime =
        MediaRuntime::load(root.path()).expect("independent media runtime");
    let editor = EditorRuntime::load(root.path()).expect("full editor runtime");
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
    MediaRuntime::load(root.path()).expect("media without provider, model, STT or skills");
    assert!(EditorRuntime::load(root.path()).is_err());
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
    invalid["runtime"]["plan"] = Value::Null;
    documents.push(invalid);
    let mut invalid = document();
    invalid["runtime"]["brain"] = json!(false);
    documents.push(invalid);
    for invalid in documents {
        let root = package(&invalid);
        MediaRuntime::load(root.path()).expect("media ignores AI-only values");
        assert!(EditorRuntime::load(root.path()).is_err());
    }
}

#[test]
fn provider_relative_resource_checks_remain_editor_only() {
    for (field, value) in [("max_frames", 9), ("max_frame_bytes", 262_144)] {
        let mut document = document();
        document["runtime"]["resources"][field] = json!(value);
        let root = package(&document);
        MediaRuntime::load(root.path()).expect("intrinsic media resource limits");
        assert!(EditorRuntime::load(root.path()).is_err());
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
        MediaRuntime::load(root.path()).expect("media ignores editor runtime bounds");
        assert!(EditorRuntime::load(root.path()).is_err());
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
        assert_configuration_error(root.path());
        assert!(EditorRuntime::load(root.path()).is_err(), "{field}");
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
        assert_configuration_error(root.path());
        assert!(EditorRuntime::load(root.path()).is_err(), "{field}={value}");
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
        assert_configuration_error(root.path());
    }
}

#[test]
fn malformed_missing_and_oversized_yaml_fail_closed() {
    let root = tempfile::tempdir().expect("package root");
    assert_configuration_error(root.path());
    write_document(root.path(), &document());
    let path = root.path().join("ops/social-ai/editor.yaml");
    std::fs::write(&path, "provider: [\nruntime: []").expect("malformed fixture");
    assert_configuration_error(root.path());
    let duplicate = format!(
        "provider: unavailable\n{}",
        include_str!("../../../../ops/social-ai/editor.yaml")
    );
    std::fs::write(&path, duplicate).expect("duplicate YAML mapping key");
    assert_configuration_error(root.path());
    let mut oversized = serde_json::to_string(&document()).expect("valid YAML");
    oversized.push_str(&" ".repeat(64 * 1024));
    std::fs::write(&path, oversized).expect("oversized fixture");
    assert_configuration_error(root.path());
}

#[test]
fn absolute_roots_and_canonical_package_resolution_are_preserved() {
    let mut document: Value =
        serde_yaml::from_str(include_str!("../../../../ops/social-ai/editor.yaml"))
            .expect("packaged YAML");
    let root = package(&document);
    std::fs::write(
        root.path().join("ops/social-ai/editor.yaml"),
        include_str!("../../../../ops/social-ai/editor.yaml"),
    )
    .expect("unmodified packaged YAML");
    let media = MediaRuntime::load(root.path()).expect("absolute media roots");
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
    write_document(root.path(), &document);
    let link_root = tempfile::tempdir().expect("symlink parent");
    let link = link_root.path().join("package");
    std::os::unix::fs::symlink(root.path(), &link).expect("package symlink");
    let media = MediaRuntime::load(&link).expect("symlink package root");
    let editor = EditorRuntime::load(&link).expect("symlink editor root");
    assert_eq!(
        media.package_root,
        root.path().canonicalize().expect("root")
    );
    assert_eq!(media.source_root, media.package_root.join("clips"));
    assert_eq!(editor.media().source_root, media.source_root);
}
