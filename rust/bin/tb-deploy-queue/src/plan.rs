use crate::git;
use anyhow::{anyhow, Context, Result};
use regex::Regex;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct BinarySpec {
    pub name: &'static str,
    pub package: &'static str,
    pub services: &'static [&'static str],
}

pub struct FrontendSpec {
    pub dir: &'static str,
    pub dist: &'static str,
    pub inputs: &'static [&'static str],
}

const BOT: &str = "twitch-bot";
const DASHBOARD: &str = "twitch-dashboard";
const AUDIT: &str = "coaching-watch";
const HUB: &str = "discord-hub";

pub const BINARIES: [BinarySpec; 11] = [
    BinarySpec {
        name: "tb-bot",
        package: "tb-bot",
        services: &[BOT],
    },
    BinarySpec {
        name: "tb-dashboard",
        package: "tb-dashboard",
        services: &[DASHBOARD],
    },
    BinarySpec {
        name: "clip_context_learn",
        package: "tb-dashboard",
        services: &[DASHBOARD],
    },
    BinarySpec {
        name: "tb-stream-audit",
        package: "tb-stream-audit-bin",
        services: &[AUDIT],
    },
    BinarySpec {
        name: "tb-config-check",
        package: "tb-config",
        services: &[],
    },
    BinarySpec {
        name: "tb-llm-usage-recover",
        package: "tb-llm",
        services: &[],
    },
    BinarySpec {
        name: "tb-discord-hub",
        package: "tb-discord-hub",
        services: &[HUB],
    },
    BinarySpec {
        name: "tb-stt-server",
        package: "tb-stt-server",
        services: &[BOT, DASHBOARD],
    },
    BinarySpec {
        name: "tb-category-collector",
        package: "tb-category-collector",
        services: &[],
    },
    BinarySpec {
        name: "tb-twitch-watchdog",
        package: "tb-category-collector",
        services: &[],
    },
    BinarySpec {
        name: "tb-category-storage",
        package: "tb-category-collector",
        services: &[],
    },
];

pub const FRONTENDS: [FrontendSpec; 3] = [
    FrontendSpec {
        dir: "bot/dashboard_v2",
        dist: "bot/analytics/dashboard_v2/dist",
        inputs: &["bot/dashboard_v2/", "bot/shared-theme/"],
    },
    FrontendSpec {
        dir: "bot/admin_dashboard",
        dist: "bot/admin_dashboard/dist",
        inputs: &["bot/admin_dashboard/", "bot/shared-theme/"],
    },
    FrontendSpec {
        dir: "website",
        dist: "website/dist",
        inputs: &["website/"],
    },
];

const SERVICE_PATHS: [(&str, &[&str]); 6] = [
    ("rust/scripts/run_tb_bot_service.sh", &[BOT]),
    ("rust/scripts/run_tb_dashboard_service.sh", &[DASHBOARD]),
    ("rust/scripts/run_stream_audit_service.sh", &[AUDIT]),
    ("ops/systemd/run-discord-hub", &[HUB]),
    (
        "ops/systemd/deadlock-twitch-discord-hub-rust.service",
        &[HUB],
    ),
    ("ops/social-ai/", &[BOT, DASHBOARD]),
];

const UNAPPLIED_PATHS: [&str; 10] = [
    "ops/systemd/discord-hub.conf",
    "ops/systemd/deadlock-twitch-bot-rust.service",
    "ops/systemd/bot.conf",
    "ops/systemd/deadlock-twitch-dashboard-rust.service",
    "ops/systemd/dashboard.conf",
    "ops/systemd/deadlock-twitch-stream-coaching-watch.service",
    "ops/systemd/audit.conf",
    "ops/systemd/20-audit-release.conf",
    "ops/stt-server/",
    "ops/systemd/deadlock-twitch-bot-watchdog.conf",
];

const IGNORED_PREFIXES: [&str; 10] = [
    ".tasks/",
    ".github/",
    "docs/",
    "agent-docs/",
    "tests/",
    "skills/",
    "features/",
    "ops/tests/",
    "ops/deploy-queue/",
    "ops/systemd/test_",
];

const IGNORED_FILES: [&str; 4] = [
    ".gitignore",
    "ops/twitch-release-build",
    "ops/systemd/deploy-twitch-release",
    "ops/systemd/install-twitch-release.sh",
];

const WORKSPACE_EXEMPT_PREFIXES: [&str; 2] = ["rust/scripts/", "rust/docs/"];

pub struct Plan {
    pub parent: String,
    pub target: String,
    pub build_packages: BTreeSet<String>,
    pub built_binaries: Vec<String>,
    pub carried_binaries: Vec<(String, String)>,
    pub built_frontends: Vec<String>,
    pub carried_frontends: Vec<(String, String)>,
    pub restarts: BTreeSet<&'static str>,
    pub deploy_paths: Vec<String>,
    pub unapplied_paths: Vec<String>,
    pub reasons: Vec<String>,
    lines: Vec<String>,
}

impl Plan {
    pub fn needs_deploy(&self) -> bool {
        !self.built_binaries.is_empty()
            || !self.built_frontends.is_empty()
            || !self.restarts.is_empty()
            || !self.deploy_paths.is_empty()
    }

    pub fn file_text(&self) -> String {
        let mut text = self.lines.join("\n");
        text.push('\n');
        text
    }

    pub fn summary(&self) -> String {
        let mut out = Vec::new();
        out.push(format!("parent {} target {}", self.parent, self.target));
        out.push(format!(
            "build packages: {}",
            join_or_none(self.build_packages.iter())
        ));
        out.push(format!(
            "built binaries: {}",
            join_or_none(self.built_binaries.iter())
        ));
        out.push(format!(
            "carried binaries: {}",
            join_or_none(
                self.carried_binaries
                    .iter()
                    .map(|(name, origin)| format!("{name}@{}", short(origin)))
            )
        ));
        out.push(format!(
            "built frontends: {}",
            join_or_none(self.built_frontends.iter())
        ));
        out.push(format!(
            "carried frontends: {}",
            join_or_none(
                self.carried_frontends
                    .iter()
                    .map(|(name, origin)| format!("{name}@{}", short(origin)))
            )
        ));
        out.push(format!("restarts: {}", join_or_none(self.restarts.iter())));
        out.push(format!(
            "deploy-only paths: {}",
            join_or_none(self.deploy_paths.iter())
        ));
        out.push(format!(
            "not installed by the deploy: {}",
            join_or_none(self.unapplied_paths.iter())
        ));
        out.extend(
            self.reasons
                .iter()
                .map(|reason| format!("reason: {reason}")),
        );
        out.join("\n")
    }
}

pub fn short(sha: &str) -> &str {
    sha.get(..10).unwrap_or(sha)
}

fn join_or_none<I, T>(items: I) -> String
where
    I: Iterator<Item = T>,
    T: AsRef<str>,
{
    let joined: Vec<String> = items.map(|item| item.as_ref().to_string()).collect();
    if joined.is_empty() {
        "none".into()
    } else {
        joined.join(", ")
    }
}

pub struct Inputs<'a> {
    pub repo: &'a Path,
    pub cargo: &'a Path,
    pub child_path: &'a str,
    pub release_root: &'a Path,
    pub parent: &'a str,
    pub target: &'a str,
}

struct Planner<'a> {
    inputs: &'a Inputs<'a>,
    members: Vec<String>,
    diffs: BTreeMap<String, Vec<String>>,
    reasons: Vec<String>,
}

pub fn compute(inputs: &Inputs) -> Result<Plan> {
    let (package_inputs, unbounded) = package_inputs(inputs)?;
    let members = git::workspace_members(inputs.repo, inputs.target)?;
    let mut context = Planner {
        inputs,
        members,
        diffs: BTreeMap::new(),
        reasons: Vec::new(),
    };
    for package in &unbounded {
        context.reasons.push(format!(
            "{package}: unbounded file reference, always rebuilt"
        ));
    }
    let parent_on_main = git::is_ancestor(inputs.repo, inputs.parent, inputs.target)?;
    if !parent_on_main {
        context.reasons.push(format!(
            "live release {} is not an ancestor of the target, building everything",
            short(inputs.parent)
        ));
    }
    let expected = expected_binaries(inputs.repo);
    let mut build_packages = BTreeSet::new();
    let mut origins = BTreeMap::new();
    for spec in BINARIES.iter().filter(|spec| expected.contains(spec.name)) {
        let package_input = package_inputs
            .get(spec.package)
            .ok_or_else(|| anyhow!("cargo metadata has no package {}", spec.package))?;
        let carryable = parent_on_main && !unbounded.contains(spec.package);
        match binary_origin(&mut context, spec, package_input, carryable)? {
            Some(origin) => {
                origins.insert(spec.name, origin);
            }
            None => {
                build_packages.insert(spec.package.to_string());
            }
        }
    }
    let mut lines = vec![format!("parent {}", inputs.parent)];
    let mut built_binaries = Vec::new();
    let mut carried_binaries = Vec::new();
    let mut restarts = BTreeSet::new();
    let mut covered: Vec<String> = Vec::new();
    for package in &build_packages {
        lines.push(format!("build-package {package}"));
    }
    for spec in BINARIES.iter().filter(|spec| expected.contains(spec.name)) {
        let Some(package_input) = package_inputs.get(spec.package) else {
            continue;
        };
        if build_packages.contains(spec.package) {
            built_binaries.push(spec.name.to_string());
            restarts.extend(spec.services.iter().copied());
            covered.extend(package_input.iter().cloned());
        } else if let Some(origin) = origins.get(spec.name) {
            lines.push(format!(
                "carry-binary {} {}",
                spec.name,
                package_input.join(" ")
            ));
            carried_binaries.push((spec.name.to_string(), origin.clone()));
        }
    }
    let mut built_frontends = Vec::new();
    let mut carried_frontends = Vec::new();
    for spec in &FRONTENDS {
        let frontend_inputs: Vec<String> =
            spec.inputs.iter().map(|input| input.to_string()).collect();
        match frontend_origin(&mut context, spec, &frontend_inputs, parent_on_main)? {
            Some(origin) => {
                lines.push(format!(
                    "carry-frontend {} {}",
                    spec.dir,
                    frontend_inputs.join(" ")
                ));
                carried_frontends.push((spec.dir.to_string(), origin));
            }
            None => {
                built_frontends.push(spec.dir.to_string());
                restarts.insert(DASHBOARD);
                covered.extend(frontend_inputs);
            }
        }
    }
    let all_inputs: Vec<String> = package_inputs.values().flatten().cloned().collect();
    let mut deploy_paths = Vec::new();
    let mut unapplied_paths = Vec::new();
    let changed = context.diff(inputs.parent)?;
    for path in &changed {
        if unapplied(path) {
            unapplied_paths.push(path.clone());
            continue;
        }
        if under_any(path, &covered) {
            continue;
        }
        if let Some(services) = service_path(path) {
            restarts.extend(services.iter().copied());
            deploy_paths.push(path.clone());
            continue;
        }
        if ignored(path) || under_any(path, &all_inputs) || in_member(path, &context.members) {
            continue;
        }
        deploy_paths.push(path.clone());
    }
    Ok(Plan {
        parent: inputs.parent.to_string(),
        target: inputs.target.to_string(),
        build_packages,
        built_binaries,
        carried_binaries,
        built_frontends,
        carried_frontends,
        restarts,
        deploy_paths,
        unapplied_paths,
        reasons: context.reasons,
        lines,
    })
}

impl Planner<'_> {
    fn diff(&mut self, origin: &str) -> Result<Vec<String>> {
        if let Some(changed) = self.diffs.get(origin) {
            return Ok(changed.clone());
        }
        let changed = git::changed_paths(self.inputs.repo, origin, self.inputs.target)?;
        self.diffs.insert(origin.to_string(), changed.clone());
        Ok(changed)
    }

    fn origin_usable(&mut self, artifact: &str, origin: &str) -> Result<bool> {
        if !is_sha(origin) {
            self.reasons
                .push(format!("{artifact}: parent copy has no valid origin"));
            return Ok(false);
        }
        if !git::is_ancestor(self.inputs.repo, origin, self.inputs.target)? {
            self.reasons.push(format!(
                "{artifact}: origin {} is not an ancestor",
                short(origin)
            ));
            return Ok(false);
        }
        Ok(true)
    }
}

fn binary_origin(
    context: &mut Planner,
    spec: &BinarySpec,
    inputs: &[String],
    parent_on_main: bool,
) -> Result<Option<String>> {
    if !parent_on_main {
        return Ok(None);
    }
    let parent_copy = context
        .inputs
        .release_root
        .join(context.inputs.parent)
        .join("rust/target/release")
        .join(spec.name);
    let Some(origin) = stamp(&parent_copy) else {
        context.reasons.push(format!(
            "{}: no stamped copy in the live release",
            spec.name
        ));
        return Ok(None);
    };
    if !context.origin_usable(spec.name, &origin)? {
        return Ok(None);
    }
    let changed = context.diff(&origin)?;
    if let Some(path) = changed
        .iter()
        .find(|path| workspace_wide(path, &context.members))
    {
        context
            .reasons
            .push(format!("{}: workspace-wide change {path}", spec.name));
        return Ok(None);
    }
    if let Some(path) = changed.iter().find(|path| under_any(path, inputs)) {
        context
            .reasons
            .push(format!("{}: input {path} changed", spec.name));
        return Ok(None);
    }
    Ok(Some(origin))
}

fn frontend_origin(
    context: &mut Planner,
    spec: &FrontendSpec,
    inputs: &[String],
    parent_on_main: bool,
) -> Result<Option<String>> {
    if !parent_on_main {
        return Ok(None);
    }
    let parent_release = context.inputs.release_root.join(context.inputs.parent);
    if !parent_release.join(spec.dist).join("index.html").is_file() {
        context
            .reasons
            .push(format!("{}: no dist in the live release", spec.dir));
        return Ok(None);
    }
    let origin = provenance_origin(&parent_release, spec.dist)
        .unwrap_or_else(|| context.inputs.parent.to_string());
    if !context.origin_usable(spec.dir, &origin)? {
        return Ok(None);
    }
    let changed = context.diff(&origin)?;
    if let Some(path) = changed.iter().find(|path| under_any(path, inputs)) {
        context
            .reasons
            .push(format!("{}: input {path} changed", spec.dir));
        return Ok(None);
    }
    Ok(Some(origin))
}

fn provenance_origin(release: &Path, artifact: &str) -> Option<String> {
    let text = std::fs::read_to_string(release.join("PROVENANCE")).ok()?;
    text.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        (fields.next() == Some(artifact)).then(|| fields.next().map(str::to_string))?
    })
}

fn stamp(binary: &Path) -> Option<String> {
    let output = Command::new("readelf")
        .arg("--string-dump=.twitch_build")
        .arg(binary)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| line.contains('['))
        .filter_map(|line| line.split_whitespace().last().map(str::to_string))
        .next()
}

pub fn is_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn expected_binaries(repo: &Path) -> BTreeSet<&'static str> {
    let mut expected: BTreeSet<&'static str> = [
        "tb-bot",
        "tb-dashboard",
        "tb-stream-audit",
        "tb-config-check",
        "tb-llm-usage-recover",
        "tb-discord-hub",
    ]
    .into_iter()
    .collect();
    let exists = |relative: &str| repo.join(relative).is_file();
    if exists("rust/crates/tb-social-media/src/ai_editor/mod.rs") {
        expected.insert("tb-stt-server");
    }
    if exists("rust/bin/tb-category-collector/Cargo.toml") {
        if !exists("rust/bin/tb-category-collector/src/lib.rs") {
            expected.insert("tb-category-collector");
        }
        for binary in ["tb-twitch-watchdog", "tb-category-storage"] {
            if exists(&format!(
                "rust/bin/tb-category-collector/src/bin/{binary}.rs"
            )) {
                expected.insert(binary);
            }
        }
    }
    if exists("rust/bin/tb-dashboard/src/bin/clip_context_learn.rs") {
        expected.insert("clip_context_learn");
    }
    expected
}

fn under(path: &str, input: &str) -> bool {
    let input = input.trim_end_matches('/');
    path == input
        || path
            .strip_prefix(input)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn under_any(path: &str, inputs: &[String]) -> bool {
    inputs.iter().any(|input| under(path, input))
}

fn in_member(path: &str, members: &[String]) -> bool {
    members
        .iter()
        .any(|member| path.starts_with(member.as_str()))
}

pub fn workspace_wide(path: &str, members: &[String]) -> bool {
    path.starts_with("rust/")
        && !in_member(path, members)
        && !WORKSPACE_EXEMPT_PREFIXES
            .iter()
            .any(|prefix| path.starts_with(prefix))
        && !path.ends_with(".md")
}

fn unapplied(path: &str) -> bool {
    UNAPPLIED_PATHS.iter().any(|entry| {
        if entry.ends_with('/') {
            path.starts_with(entry)
        } else {
            path == *entry
        }
    })
}

fn service_path(path: &str) -> Option<&'static [&'static str]> {
    SERVICE_PATHS
        .iter()
        .find(|(service_path, _)| {
            if service_path.ends_with('/') {
                path.starts_with(service_path)
            } else {
                path == *service_path
            }
        })
        .map(|(_, services)| *services)
}

fn ignored(path: &str) -> bool {
    IGNORED_PREFIXES
        .iter()
        .any(|prefix| path.starts_with(prefix))
        || IGNORED_FILES.contains(&path)
        || path.ends_with(".md")
}

struct Package {
    dir: String,
    dependencies: Vec<String>,
}

type PackageInputs = (BTreeMap<String, Vec<String>>, BTreeSet<String>);

fn package_inputs(inputs: &Inputs) -> Result<PackageInputs> {
    let packages = cargo_packages(inputs)?;
    let by_dir: BTreeMap<&str, &str> = packages
        .iter()
        .map(|(name, package)| (package.dir.as_str(), name.as_str()))
        .collect();
    let literal =
        Regex::new(r#""([^"\\\n]*\.\./[^"\\\n]*)""#).context("invalid reference pattern")?;
    let mut references: BTreeMap<String, (BTreeSet<String>, bool)> = BTreeMap::new();
    let mut result = BTreeMap::new();
    let mut unbounded = BTreeSet::new();
    for spec in &BINARIES {
        if result.contains_key(spec.package) || !packages.contains_key(spec.package) {
            continue;
        }
        let mut closure = BTreeSet::new();
        let mut pending = vec![spec.package.to_string()];
        while let Some(name) = pending.pop() {
            let Some(package) = packages.get(&name) else {
                continue;
            };
            if !closure.insert(package.dir.clone()) {
                continue;
            }
            for dependency in &package.dependencies {
                if let Some(dependency_name) = by_dir.get(dependency.as_str()) {
                    pending.push((*dependency_name).to_string());
                }
            }
        }
        let mut all = closure.clone();
        for dir in &closure {
            if !references.contains_key(dir) {
                let found = crate_references(inputs.repo, dir, &literal)?;
                references.insert(dir.clone(), found);
            }
            if let Some((found, open)) = references.get(dir) {
                all.extend(found.iter().cloned());
                if *open {
                    unbounded.insert(spec.package.to_string());
                }
            }
        }
        result.insert(spec.package.to_string(), all.into_iter().collect());
    }
    Ok((result, unbounded))
}

fn cargo_packages(inputs: &Inputs) -> Result<BTreeMap<String, Package>> {
    let output = Command::new(inputs.cargo)
        .args([
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--offline",
        ])
        .current_dir(inputs.repo.join("rust"))
        .env("PATH", inputs.child_path)
        .output()
        .context("cannot run cargo metadata")?;
    if !output.status.success() {
        anyhow::bail!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let metadata: Value =
        serde_json::from_slice(&output.stdout).context("cargo metadata is not JSON")?;
    let repo = std::fs::canonicalize(inputs.repo).context("cannot resolve the repository path")?;
    let mut packages = BTreeMap::new();
    for package in metadata
        .get("packages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let name = package
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let manifest = package
            .get("manifest_path")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let Some(dir) = relative_dir(&repo, Path::new(manifest).parent()) else {
            continue;
        };
        let dependencies = package
            .get("dependencies")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|dependency| dependency.get("kind").and_then(Value::as_str) != Some("dev"))
            .filter_map(|dependency| dependency.get("path").and_then(Value::as_str))
            .filter_map(|path| relative_dir(&repo, Some(Path::new(path))))
            .collect();
        packages.insert(name.to_string(), Package { dir, dependencies });
    }
    Ok(packages)
}

fn relative_dir(repo: &Path, dir: Option<&Path>) -> Option<String> {
    let relative = dir?.strip_prefix(repo).ok()?;
    let text = relative.to_str()?;
    Some(format!("{text}/"))
}

fn crate_references(
    repo: &Path,
    crate_dir: &str,
    literal: &Regex,
) -> Result<(BTreeSet<String>, bool)> {
    let mut found = BTreeSet::new();
    let mut open = false;
    let mut pending = vec![repo.join(crate_dir)];
    while let Some(dir) = pending.pop() {
        for entry in
            std::fs::read_dir(&dir).with_context(|| format!("cannot read {}", dir.display()))?
        {
            let path = entry?.path();
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            if path.is_dir() {
                if name != "target" && name != "node_modules" && !name.starts_with('.') {
                    pending.push(path);
                }
                continue;
            }
            if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let file_dir = relative_path(repo, path.parent());
            for reference in literal
                .captures_iter(&text)
                .filter_map(|capture| capture.get(1))
            {
                for base in [file_dir.as_deref(), Some(crate_dir)].into_iter().flatten() {
                    match resolve_reference(base, reference.as_str()) {
                        Some(Reference::Path(resolved)) if !under(&resolved, crate_dir) => {
                            found.insert(resolved);
                        }
                        Some(Reference::Unbounded) => open = true,
                        _ => {}
                    }
                }
            }
        }
    }
    Ok((found, open))
}

fn relative_path(repo: &Path, path: Option<&Path>) -> Option<String> {
    path?.strip_prefix(repo).ok()?.to_str().map(str::to_string)
}

#[derive(Debug, PartialEq, Eq)]
pub enum Reference {
    Path(String),
    Unbounded,
}

pub fn resolve_reference(base: &str, reference: &str) -> Option<Reference> {
    let joined: PathBuf = if reference.starts_with('/') {
        PathBuf::from(format!("{base}{reference}"))
    } else {
        Path::new(base).join(reference)
    };
    let mut parts: Vec<String> = Vec::new();
    for part in joined.to_str()?.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other.to_string()),
        }
    }
    let normalized = parts.join("/");
    let valid = |character: char| character.is_ascii_alphanumeric() || "_./-".contains(character);
    let bounded = match normalized.find(|character: char| !valid(character)) {
        None => normalized,
        Some(index) => match normalized[..index].rfind('/') {
            Some(slash) => normalized[..=slash].to_string(),
            None => String::new(),
        },
    };
    if bounded.is_empty() || bounded.starts_with('-') {
        return Some(Reference::Unbounded);
    }
    Some(Reference::Path(bounded))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_references_outside_the_crate() {
        assert_eq!(
            resolve_reference(
                "rust/crates/tb-social-media/src/ai_editor",
                "../../../../../ops/social-ai/skills/subtitles.md"
            ),
            Some(Reference::Path("ops/social-ai/skills/subtitles.md".into()))
        );
        assert_eq!(
            resolve_reference("rust/crates/tb-monitoring/", "/../../test-database-url.txt"),
            Some(Reference::Path("rust/test-database-url.txt".into()))
        );
        assert_eq!(
            resolve_reference("rust/crates/tb-db/src", "../../../migrations/{name}.sql"),
            Some(Reference::Path("rust/migrations/".into()))
        );
        assert_eq!(
            resolve_reference("rust/crates/x", "../../../{root}"),
            Some(Reference::Unbounded)
        );
        assert_eq!(resolve_reference("rust", "../../../etc"), None);
    }

    #[test]
    fn workspace_rule_matches_the_installer() {
        let members = vec![
            "rust/crates/tb-db/".to_string(),
            "rust/bin/tb-bot/".to_string(),
        ];
        assert!(workspace_wide("rust/Cargo.lock", &members));
        assert!(workspace_wide("rust/migrations/1.sql", &members));
        assert!(!workspace_wide("rust/crates/tb-db/src/lib.rs", &members));
        assert!(!workspace_wide(
            "rust/scripts/run_tb_bot_service.sh",
            &members
        ));
        assert!(!workspace_wide("rust/knowledge/readme.md", &members));
        assert!(!workspace_wide("bot/dashboard_v2/src/App.tsx", &members));
    }
}
