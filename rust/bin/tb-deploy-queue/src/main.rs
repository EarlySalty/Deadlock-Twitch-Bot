mod config;
mod daemon;
mod db;
mod git;
mod plan;
mod run;
mod t3;

use anyhow::{anyhow, bail, Context, Result};
use chrono::Local;
use std::path::{Path, PathBuf};

const USAGE: &str = "usage:
  deploy-queue submit --sha <main sha> --thread <t3 thread id> --task <absolute .tasks dir> [--now]
  deploy-queue status [ticket]
  deploy-queue daemon
  deploy-queue plan --repo <checkout> [--parent <sha>] [--target <rev>]";

#[tokio::main]
async fn main() {
    if let Err(error) = dispatch(std::env::args().skip(1).collect()).await {
        eprintln!("deploy-queue: {error:#}");
        std::process::exit(1);
    }
}

async fn dispatch(args: Vec<String>) -> Result<()> {
    let Some((command, rest)) = args.split_first() else {
        bail!("{USAGE}");
    };
    match command.as_str() {
        "submit" => submit(rest).await,
        "status" => status(rest).await,
        "daemon" => daemon::serve(load_config()?).await,
        "plan" => dry_plan(rest),
        "help" | "--help" | "-h" => {
            println!("{USAGE}");
            Ok(())
        }
        other => bail!("unknown command {other}\n{USAGE}"),
    }
}

fn load_config() -> Result<config::Config> {
    config::load(&config::default_path()?)
}

struct Flags {
    values: Vec<(String, String)>,
    switches: Vec<String>,
    positional: Vec<String>,
}

impl Flags {
    fn parse(args: &[String], valued: &[&str], switches: &[&str]) -> Result<Self> {
        let mut flags = Self {
            values: Vec::new(),
            switches: Vec::new(),
            positional: Vec::new(),
        };
        let mut iter = args.iter();
        while let Some(arg) = iter.next() {
            if valued.contains(&arg.as_str()) {
                let value = iter.next().ok_or_else(|| anyhow!("{arg} needs a value"))?;
                flags.values.push((arg.clone(), value.clone()));
            } else if switches.contains(&arg.as_str()) {
                flags.switches.push(arg.clone());
            } else if arg.starts_with("--") {
                bail!("unknown option {arg}\n{USAGE}");
            } else {
                flags.positional.push(arg.clone());
            }
        }
        Ok(flags)
    }

    fn value(&self, name: &str) -> Option<&str> {
        self.values
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    fn required(&self, name: &str) -> Result<&str> {
        self.value(name)
            .ok_or_else(|| anyhow!("{name} is required\n{USAGE}"))
    }

    fn switch(&self, name: &str) -> bool {
        self.switches.iter().any(|switch| switch == name)
    }
}

async fn submit(args: &[String]) -> Result<()> {
    let flags = Flags::parse(args, &["--sha", "--thread", "--task"], &["--now"])?;
    let sha = flags.required("--sha")?.to_ascii_lowercase();
    if !plan::is_sha(&sha) {
        bail!("--sha must be a full 40 character commit SHA");
    }
    let thread = flags.required("--thread")?;
    if thread.is_empty()
        || !thread
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        bail!("--thread must be a T3 thread id");
    }
    let task = Path::new(flags.required("--task")?);
    if !task.is_absolute() {
        bail!("--task must be an absolute path");
    }
    if !task.is_dir() {
        bail!("--task {} is not a directory", task.display());
    }
    let config = load_config()?;
    let pool = db::connect(&config.database_url).await?;
    let task_text = task
        .to_str()
        .ok_or_else(|| anyhow!("--task is not valid UTF-8"))?;
    let id = db::insert_ticket(&pool, &sha, thread, task_text, flags.switch("--now")).await?;
    println!("{id}");
    Ok(())
}

async fn status(args: &[String]) -> Result<()> {
    let config = load_config()?;
    let pool = db::connect(&config.database_url).await?;
    match args {
        [] => list(&pool).await,
        [id] => show(&pool, id.parse().context("ticket must be a number")?).await,
        _ => bail!("{USAGE}"),
    }
}

async fn list(pool: &sqlx::PgPool) -> Result<()> {
    let tickets = db::recent_tickets(pool).await?;
    if tickets.is_empty() {
        println!("no open or recent tickets");
        return Ok(());
    }
    println!(
        "{:>5}  {:<10}  {:<8}  {:<16}  {:<10}  {:<8}  {:<19}  thread",
        "id", "sha", "state", "outcome", "compaction", "notified", "submitted"
    );
    for ticket in tickets {
        println!(
            "{:>5}  {:<10}  {:<8}  {:<16}  {:<10}  {:<8}  {:<19}  {}",
            ticket.id,
            plan::short(&ticket.sha),
            ticket.state,
            ticket.outcome.as_deref().unwrap_or("-"),
            ticket.compaction,
            if ticket.notified_at.is_some() {
                "yes"
            } else {
                "no"
            },
            ticket
                .submitted_at
                .with_timezone(&Local)
                .format("%Y-%m-%d %H:%M:%S"),
            ticket.thread_id,
        );
    }
    Ok(())
}

async fn show(pool: &sqlx::PgPool, id: i64) -> Result<()> {
    let ticket = db::ticket(pool, id)
        .await?
        .ok_or_else(|| anyhow!("ticket {id} not found"))?;
    println!("ticket {}", ticket.id);
    println!("  sha         {}", ticket.sha);
    println!("  thread      {}", ticket.thread_id);
    println!("  task        {}", ticket.task_dir);
    println!("  now         {}", ticket.deploy_now);
    println!(
        "  submitted   {}",
        ticket.submitted_at.with_timezone(&Local)
    );
    println!("  state       {}", ticket.state);
    println!("  outcome     {}", ticket.outcome.as_deref().unwrap_or("-"));
    println!(
        "  compaction  {} {}",
        ticket.compaction,
        ticket.compaction_detail.as_deref().unwrap_or("")
    );
    match ticket.notified_at {
        Some(at) => println!("  notified    {}", at.with_timezone(&Local)),
        None => println!(
            "  notified    no (attempts {}{})",
            ticket.notify_attempts,
            ticket
                .notify_error
                .as_deref()
                .map(|error| format!(", last error: {error}"))
                .unwrap_or_default()
        ),
    }
    if let Some(run_id) = ticket.run_id {
        if let Some(run) = db::run(pool, run_id).await? {
            println!("run {}", run.id);
            println!("  started     {}", run.started_at.with_timezone(&Local));
            if let Some(finished) = run.finished_at {
                println!("  finished    {}", finished.with_timezone(&Local));
            }
            println!("  parent      {}", run.parent_sha.as_deref().unwrap_or("-"));
            println!("  target      {}", run.target_sha.as_deref().unwrap_or("-"));
            println!(
                "  outcome     {}",
                run.outcome.as_deref().unwrap_or("running")
            );
            if let Some(step) = run.failed_step {
                println!("  failed at   {step}");
            }
            println!("  built       {}", run.built);
            println!("  carried     {}", run.carried);
            if let Some(seconds) = run.build_seconds {
                println!("  build time  {seconds}s");
            }
            println!("  restarted   {}", run.restarted);
            if let Some(code) = run.pruefen_exit {
                println!("  pruefen     exit {code}");
            }
            println!("  exe         {}", run.exe_report);
            if let Some(live) = run.live_at {
                println!("  live        {}", live.with_timezone(&Local));
            }
            println!("  log         {}", run.log_path);
        }
    }
    if let Some(message) = ticket.final_message {
        println!("final message:\n{message}");
    }
    Ok(())
}

fn dry_plan(args: &[String]) -> Result<()> {
    let flags = Flags::parse(args, &["--repo", "--parent", "--target"], &[])?;
    let config = load_config()?;
    let repo = PathBuf::from(flags.required("--repo")?);
    let parent = match flags.value("--parent") {
        Some(parent) => parent.to_string(),
        None => run::live_release(&config.current_link)?,
    };
    let target = git::output(
        &repo,
        &["rev-parse", flags.value("--target").unwrap_or("HEAD")],
    )?
    .trim()
    .to_string();
    let plan = plan::compute(&plan::Inputs {
        repo: &repo,
        cargo: &config.cargo,
        child_path: &config.child_path,
        release_root: &config.release_root,
        parent: &parent,
        target: &target,
    })?;
    println!("{}", plan.summary());
    println!("needs deploy: {}", plan.needs_deploy());
    println!("--- plan file ---\n{}", plan.file_text());
    Ok(())
}
