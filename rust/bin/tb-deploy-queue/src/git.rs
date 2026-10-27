use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;

fn git(repo: &Path) -> Command {
    let mut command = Command::new("git");
    command.arg("-C").arg(repo);
    command
}

pub fn output(repo: &Path, args: &[&str]) -> Result<String> {
    let result = git(repo)
        .args(args)
        .output()
        .with_context(|| format!("cannot run git {}", args.join(" ")))?;
    if !result.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&result.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&result.stdout).into_owned())
}

pub fn is_ancestor(repo: &Path, ancestor: &str, descendant: &str) -> Result<bool> {
    let status = git(repo)
        .args(["merge-base", "--is-ancestor", ancestor, descendant])
        .status()
        .context("cannot run git merge-base")?;
    match status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => bail!("git merge-base --is-ancestor {ancestor} {descendant} failed"),
    }
}

pub fn has_commit(repo: &Path, sha: &str) -> bool {
    git(repo)
        .args(["cat-file", "-e", &format!("{sha}^{{commit}}")])
        .status()
        .is_ok_and(|status| status.success())
}

pub fn changed_paths(repo: &Path, from: &str, to: &str) -> Result<Vec<String>> {
    let text = output(
        repo,
        &["diff", "--no-renames", "-z", "--name-only", from, to, "--"],
    )?;
    Ok(text
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_string)
        .collect())
}

pub fn workspace_members(repo: &Path, revision: &str) -> Result<Vec<String>> {
    let text = output(
        repo,
        &[
            "ls-tree",
            "-r",
            "--name-only",
            revision,
            "--",
            "rust/crates",
            "rust/bin",
        ],
    )?;
    Ok(text
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split('/').collect();
            (parts.len() == 4 && parts[3] == "Cargo.toml")
                .then(|| format!("{}/{}/{}/", parts[0], parts[1], parts[2]))
        })
        .collect())
}

pub fn commits(repo: &Path, from: &str, to: &str, limit: usize) -> Result<Vec<String>> {
    let range = format!("{from}..{to}");
    let max = format!("--max-count={limit}");
    let text = output(repo, &["log", "--oneline", "--no-decorate", &max, &range])?;
    Ok(text.lines().map(str::to_string).collect())
}
