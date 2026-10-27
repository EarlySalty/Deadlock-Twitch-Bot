use crate::config::Config;
use crate::db::RunRecord;
use crate::git;
use crate::plan::{self, Plan};
use anyhow::{anyhow, bail, Context, Result};
use chrono::Utc;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub struct Batch {
    pub run_id: i64,
    pub log_path: String,
    pub tickets: Vec<(i64, String)>,
}

pub struct Report {
    pub record: RunRecord,
    pub rejected: Vec<(i64, String)>,
    pub commits: Vec<String>,
    pub note: String,
}

struct Log {
    file: File,
}

impl Log {
    fn open(path: &str) -> Result<Self> {
        if let Some(parent) = Path::new(path).parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("cannot create {}", parent.display()))?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .with_context(|| format!("cannot open log {path}"))?;
        Ok(Self { file })
    }

    fn line(&mut self, text: &str) {
        let _ = writeln!(
            self.file,
            "{} {text}",
            Utc::now().format("%Y-%m-%dT%H:%M:%SZ")
        );
    }

    fn step(&mut self, name: &str) {
        self.line(&format!("== step {name}"));
    }

    fn exec(&mut self, command: &mut Command) -> Result<(bool, String)> {
        self.line(&format!("$ {command:?}"));
        let stderr = self.file.try_clone().context("cannot share the log file")?;
        let mut child = command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(stderr))
            .spawn()
            .context("cannot start the command")?;
        let mut collected = String::new();
        if let Some(stdout) = child.stdout.take() {
            for line in BufReader::new(stdout).lines() {
                let line = line.context("cannot read command output")?;
                let _ = writeln!(self.file, "{line}");
                collected.push_str(&line);
                collected.push('\n');
            }
        }
        let status = child.wait().context("cannot wait for the command")?;
        self.line(&format!("exit {status}"));
        Ok((status.success(), collected))
    }
}

struct Failure {
    step: &'static str,
    error: anyhow::Error,
}

trait StepResult<T> {
    fn at(self, step: &'static str) -> std::result::Result<T, Failure>;
}

impl<T> StepResult<T> for Result<T> {
    fn at(self, step: &'static str) -> std::result::Result<T, Failure> {
        self.map_err(|error| Failure { step, error })
    }
}

pub fn execute(config: &Config, batch: &Batch) -> Report {
    let mut report = Report {
        record: RunRecord::default(),
        rejected: Vec::new(),
        commits: Vec::new(),
        note: String::new(),
    };
    let mut log = match Log::open(&batch.log_path) {
        Ok(log) => log,
        Err(error) => {
            report.record.outcome = "failed".into();
            report.record.failed_step = Some("log".into());
            report.note = format!("{error:#}");
            return report;
        }
    };
    log.line(&format!(
        "run {} tickets {}",
        batch.run_id,
        batch
            .tickets
            .iter()
            .map(|(id, sha)| format!("#{id}={}", plan::short(sha)))
            .collect::<Vec<_>>()
            .join(" ")
    ));
    if let Err(failure) = steps(config, batch, &mut log, &mut report) {
        log.line(&format!("FAILED at {}: {:#}", failure.step, failure.error));
        report.record.outcome = "failed".into();
        report.record.failed_step = Some(failure.step.into());
        report.note = format!("{:#}", failure.error);
    }
    log.line(&format!("outcome {}", report.record.outcome));
    report
}

fn steps(
    config: &Config,
    batch: &Batch,
    log: &mut Log,
    report: &mut Report,
) -> std::result::Result<(), Failure> {
    let repo = config.repo_dir.as_path();
    log.step("fetch");
    sync_clone(config, log).at("fetch")?;
    let target = git::output(repo, &["rev-parse", "origin/main"])
        .at("fetch")?
        .trim()
        .to_string();
    report.record.target_sha = Some(target.clone());
    log.step("resolve");
    let accepted = accepted_tickets(repo, &target, batch, report).at("resolve")?;
    if accepted == 0 {
        return Err(Failure {
            step: "resolve",
            error: anyhow!("no ticket SHA is on origin/main {}", plan::short(&target)),
        });
    }
    let parent = live_release(&config.current_link).at("resolve")?;
    report.record.parent_sha = Some(parent.clone());
    log.line(&format!("parent {parent} target {target}"));
    report.commits = if git::has_commit(repo, &parent) {
        git::commits(repo, &parent, &target, 20).at("resolve")?
    } else {
        Vec::new()
    };
    if parent == target {
        report.record.outcome = "no_deploy_needed".into();
        report.note = format!("{} is already the live release", plan::short(&target));
        return Ok(());
    }
    log.step("checkout");
    checkout(repo, &target, log).at("checkout")?;
    log.step("plan");
    let plan = plan::compute(&plan::Inputs {
        repo,
        cargo: &config.cargo,
        child_path: &config.child_path,
        release_root: &config.release_root,
        parent: &parent,
        target: &target,
    })
    .at("plan")?;
    log.line(&plan.summary());
    report.record.built = artifacts(&plan.built_binaries, &plan.built_frontends);
    report.record.carried = carried(&plan);
    if !plan.needs_deploy() {
        report.record.outcome = "no_deploy_needed".into();
        report.note = format!(
            "nothing runtime-relevant changed between the live release {} and {}",
            plan::short(&parent),
            plan::short(&target)
        );
        return Ok(());
    }
    let plan_path = PathBuf::from(format!("{}.plan", batch.log_path.trim_end_matches(".log")));
    std::fs::write(&plan_path, plan.file_text())
        .with_context(|| format!("cannot write {}", plan_path.display()))
        .at("plan")?;
    log.step("build");
    let started = Instant::now();
    build(config, &target, &plan_path, log, report).at("build")?;
    report.record.build_seconds = i32::try_from(started.elapsed().as_secs()).ok();
    log.step("deploy");
    deploy(config, &target, &plan, log).at("deploy")?;
    report.record.live_at = Some(Utc::now());
    report.record.restarted = restart_label(&plan);
    log.step("pruefen");
    pruefen(config, log, report).at("pruefen")?;
    report.record.outcome = "deployed".into();
    Ok(())
}

fn sync_clone(config: &Config, log: &mut Log) -> Result<()> {
    let repo = &config.repo_dir;
    if !repo.join(".git").is_dir() {
        if let Some(parent) = repo.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("cannot create {}", parent.display()))?;
        }
        let (ok, _) = log.exec(
            Command::new("git")
                .arg("clone")
                .arg("--no-checkout")
                .arg(&config.remote)
                .arg(repo),
        )?;
        if !ok {
            bail!("git clone failed");
        }
    }
    let remote = git::output(repo, &["remote", "get-url", "origin"])?;
    if remote.trim() != config.remote {
        bail!(
            "the queue clone points to {} instead of {}",
            remote.trim(),
            config.remote
        );
    }
    let (ok, _) = log.exec(
        Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(["fetch", "--prune", "origin"]),
    )?;
    if !ok {
        bail!("git fetch failed");
    }
    Ok(())
}

fn accepted_tickets(
    repo: &Path,
    target: &str,
    batch: &Batch,
    report: &mut Report,
) -> Result<usize> {
    let mut accepted = 0;
    for (id, sha) in &batch.tickets {
        if !git::has_commit(repo, sha) {
            report
                .rejected
                .push((*id, format!("commit {sha} does not exist on the remote")));
        } else if !git::is_ancestor(repo, sha, target)? {
            report.rejected.push((
                *id,
                format!("commit {sha} is not on origin/main {}", plan::short(target)),
            ));
        } else {
            accepted += 1;
        }
    }
    Ok(accepted)
}

pub fn live_release(link: &Path) -> Result<String> {
    let target =
        std::fs::read_link(link).with_context(|| format!("cannot read {}", link.display()))?;
    let name = target
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow!("{} has no release name", link.display()))?;
    if !plan::is_sha(name) {
        bail!(
            "{} points to {}, not a release",
            link.display(),
            target.display()
        );
    }
    Ok(name.to_string())
}

fn checkout(repo: &Path, target: &str, log: &mut Log) -> Result<()> {
    let (ok, _) = log.exec(
        Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(["checkout", "--force", "--detach", target]),
    )?;
    if !ok {
        bail!("git checkout failed");
    }
    let (ok, _) = log.exec(
        Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(["clean", "-ffdx"]),
    )?;
    if !ok {
        bail!("git clean failed");
    }
    Ok(())
}

fn artifacts(binaries: &[String], frontends: &[String]) -> String {
    binaries
        .iter()
        .chain(frontends)
        .cloned()
        .collect::<Vec<_>>()
        .join(" ")
}

fn carried(plan: &Plan) -> String {
    plan.carried_binaries
        .iter()
        .chain(&plan.carried_frontends)
        .map(|(name, origin)| format!("{name}@{}", plan::short(origin)))
        .collect::<Vec<_>>()
        .join(" ")
}

fn staging_dir(target: &str) -> Result<PathBuf> {
    let home = std::env::var_os("HOME").ok_or_else(|| anyhow!("HOME is not set"))?;
    Ok(Path::new(&home).join(format!("repos/twitch-release-{target}")))
}

fn clear_stale_staging(target: &str, log: &mut Log) -> Result<()> {
    let staging = staging_dir(target)?;
    if !staging.exists() {
        return Ok(());
    }
    let head = git::output(&staging, &["rev-parse", "HEAD"])?;
    let dirty = git::output(
        &staging,
        &["status", "--porcelain", "--untracked-files=all"],
    )?;
    if head.trim() != target || !dirty.trim().is_empty() {
        bail!(
            "{} exists and is not a clean build of {target}",
            staging.display()
        );
    }
    log.line(&format!(
        "removing stale staging clone {}",
        staging.display()
    ));
    std::fs::remove_dir_all(&staging)
        .with_context(|| format!("cannot remove {}", staging.display()))
}

fn build(
    config: &Config,
    target: &str,
    plan_path: &Path,
    log: &mut Log,
    report: &mut Report,
) -> Result<()> {
    clear_stale_staging(target, log)?;
    let script = config.repo_dir.join("ops/twitch-release-build");
    let (ok, output) = log.exec(
        Command::new(&script)
            .arg(target)
            .arg("--plan")
            .arg(plan_path)
            .current_dir(&config.repo_dir)
            .env("PATH", &config.child_path)
            .env("CARGO_SLOT_DEPLOY", "1"),
    )?;
    if !ok {
        bail!("ops/twitch-release-build failed");
    }
    let mut built = Vec::new();
    let mut carried = Vec::new();
    for line in output.lines() {
        if let Some(rest) = line.strip_prefix("artifact built ") {
            built.push(rest.split('=').nth(1).unwrap_or(rest).to_string());
        } else if let Some(rest) = line.strip_prefix("artifact carried ") {
            let mut fields = rest.split_whitespace();
            let name = fields
                .next()
                .and_then(|field| field.split('=').nth(1))
                .unwrap_or(rest);
            let origin = fields
                .next()
                .and_then(|field| field.split('=').nth(1))
                .unwrap_or_default();
            carried.push(format!("{name}@{}", plan::short(origin)));
        }
    }
    report.record.built = built.join(" ");
    report.record.carried = carried.join(" ");
    Ok(())
}

fn restart_args(plan: &Plan) -> Vec<String> {
    if plan.restarts.is_empty() {
        return vec!["--restart".into(), "none".into()];
    }
    plan.restarts
        .iter()
        .flat_map(|service| ["--restart".to_string(), (*service).to_string()])
        .collect()
}

fn restart_label(plan: &Plan) -> String {
    plan.restarts.iter().copied().collect::<Vec<_>>().join(" ")
}

fn deploy(config: &Config, target: &str, plan: &Plan, log: &mut Log) -> Result<()> {
    let (ok, _) = log.exec(
        Command::new(&config.deploy_wrapper)
            .arg(target)
            .args(restart_args(plan))
            .env("PATH", &config.child_path),
    )?;
    if !ok {
        bail!("deploy-twitch-release failed");
    }
    Ok(())
}

fn pruefen(config: &Config, log: &mut Log, report: &mut Report) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(config.pruefen_wait_seconds);
    loop {
        let mut command = Command::new(&config.deploy_wrapper);
        command.arg("--pruefen").env("PATH", &config.child_path);
        log.line(&format!("$ {command:?}"));
        let output = command
            .output()
            .context("cannot run deploy-twitch-release --pruefen")?;
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let _ = log.file.write_all(stdout.as_bytes());
        let _ = log.file.write_all(&output.stderr);
        let code = output.status.code().unwrap_or(-1);
        log.line(&format!("exit {code}"));
        report.record.pruefen_exit = Some(code);
        report.record.exe_report = exe_report(&stdout);
        if code == 0 {
            return Ok(());
        }
        if Instant::now() >= deadline {
            bail!("deploy-twitch-release --pruefen exited {code}");
        }
        std::thread::sleep(Duration::from_secs(10));
    }
}

fn exe_report(stdout: &str) -> String {
    stdout
        .lines()
        .filter_map(|line| {
            let field = |key: &str| {
                line.split_whitespace()
                    .find_map(|part| part.strip_prefix(key))
                    .map(str::to_string)
            };
            let unit = field("Unit=")?;
            let release = field("Release-SHA=")?;
            let pid = field("MainPID=").unwrap_or_default();
            Some(format!("{unit} pid {pid} exe@{}", plan::short(&release)))
        })
        .collect::<Vec<_>>()
        .join("; ")
}
