//! Execute real fail-closed gate/scanner controls on disposable fixtures only.
//! This generator never executes fixture source, installs fixture dependencies,
//! contacts an application endpoint, or handles a real credential.
use std::{env, fs, path::Path, process::Command};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const RESULTS: &[&str] = &[
    "SCOPE_RESULT",
    "MANIFEST_RESULT",
    "SECRETS_RESULT",
    "SAST_RESULT",
    "TRIVY_RESULT",
    "ACTIONS_RESULT",
    "DEEP_RESULT",
    "RUST_SECURITY_RESULT",
    "RUST_SQLX_RESULT",
    "FRONTEND_RESULT",
];

fn gate(binary: &Path) -> Result<()> {
    let invoke = |change: Option<(&str, &str)>, docs: bool| -> Result<bool> {
        let mut command = Command::new(binary);
        command.arg("gate").env_clear();
        for key in RESULTS {
            let irrelevant = docs
                && matches!(
                    *key,
                    "RUST_SECURITY_RESULT" | "RUST_SQLX_RESULT" | "FRONTEND_RESULT"
                );
            command.env(key, if irrelevant { "skipped" } else { "success" });
        }
        command.env("RUST_REQUIRED", if docs { "false" } else { "true" });
        command.env("FRONTEND_REQUIRED", if docs { "false" } else { "true" });
        if let Some((key, value)) = change {
            if value.is_empty() {
                command.env_remove(key);
            } else {
                command.env(key, value);
            }
        }
        let result = command.output()?;
        if !result.status.success()
            && !String::from_utf8_lossy(&result.stderr).contains("Required PR Gate:")
        {
            return Err("Gate failed for an unexpected reason, not its fail-closed policy".into());
        }
        Ok(result.status.success())
    };
    if !invoke(None, false)? || !invoke(None, true)? {
        return Err("A complete successful run or legitimate docs scope was rejected".into());
    }
    let mut negatives = 0;
    for key in RESULTS {
        for state in [
            "failure",
            "cancelled",
            "skipped",
            "",
            "neutral",
            "timed_out",
            "unexpected",
        ] {
            if invoke(Some((key, state)), false)? {
                return Err(format!("Gate incorrectly accepted {key}={state:?}").into());
            }
            negatives += 1;
        }
    }
    for key in ["RUST_REQUIRED", "FRONTEND_REQUIRED"] {
        for value in ["", "TRUE", "null", "0"] {
            if invoke(Some((key, value)), false)? {
                return Err(format!("Gate accepted invalid scope {key}={value:?}").into());
            }
            negatives += 1;
        }
    }
    println!("Real gate executable: 2 positive controls and {negatives} negative controls passed.");
    Ok(())
}

fn write(root: &Path, relative: &str, data: &str) -> Result<()> {
    let target = root.join(relative);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(target, data)?;
    Ok(())
}

fn fixtures(root: &Path) -> Result<()> {
    // Refuse pre-existing paths, including symlinks. Callers use RUNNER_TEMP,
    // never a checked-out source directory or a deployment directory.
    fs::create_dir(root)?;
    let fresh = ["nJ3kP1cV8tA4mR6zB", "9qF2sD5wE7uH0xY4"].concat();
    let line = format!("api_key = \"{fresh}\";\n");
    for path in [
        "rust/crates/tb-dashboard-api/src/handlers/ad_manager.rs",
        "rust/crates/tb-dashboard-api/src/obs/ws.rs",
        "website/src/components/partner-clean/Security.tsx",
    ] {
        write(&root.join("secrets-negative"), path, &line)?;
    }
    let uuid = "018f0f65-7c2e-7df1-9f64-4a8f5b711234";
    let uuid_line = format!("    idempotency_key: \"{uuid}\".into(),\n");
    write(
        &root.join("secrets-allowed"),
        "rust/crates/tb-dashboard-api/src/handlers/ad_manager.rs",
        &uuid_line,
    )?;
    write(
        &root.join("secrets-negative"),
        "outside-allowlist.rs",
        &uuid_line,
    )?;
    let nonce = "dGhlIHNhbXBsZSBub25jZQ==";
    write(
        &root.join("secrets-allowed"),
        "rust/crates/tb-dashboard-api/src/obs/ws.rs",
        &format!("websocket_key = \"{nonce}\";\n"),
    )?;
    // Existing public, non-decryptable animation samples, not secret keys.
    let samples = [
        "01027631cc1a318fc855cd096f414d4eb9140a1efaabbe7033619b7376eb2ade6399c5600e0bde49e5a9a314708da992a6e6c430146335861b9766631c795c9379b40f03da76d94ee642ec",
        "010276315877678bb2ed322b62caed1893dda920643dafb46c11418b47095a2276fa41c0b95787b2fba008224c4d32173bfd6631bbca88554ed73a66aea84ff9de382af161cde3f65398480e",
    ];
    let lines = samples
        .iter()
        .map(|value| format!("api_key = \"{value}\";\n"))
        .collect::<String>();
    write(
        &root.join("secrets-allowed"),
        "website/src/components/partner-clean/Security.tsx",
        &lines,
    )?;
    let provider_sample = ["gh", "p_", "aB3dE6gH9jK2mN5pQ8sT1vW4yZ7cF0iL3oR6"].concat();
    write(
        root,
        "provider-negative/provider.txt",
        &format!("github_token = \"{provider_sample}\"\n"),
    )?;
    write(
        root,
        "sast-negative/insecure.rs",
        r#"fn main() {
    let _ = reqwest::Client::builder().danger_accept_invalid_certs(true).build();
}
"#,
    )?;
    // Only metadata is written: no download, install, import or execution of
    // this intentionally vulnerable package can occur in these controls.
    write(
        root,
        "dependency-negative/package-lock.json",
        r#"{
  "name": "isolated-scanner-control", "version": "0.0.0", "lockfileVersion": 3,
  "packages": {
    "": {"name": "isolated-scanner-control", "version": "0.0.0", "dependencies": {"lodash": "4.17.20"}},
    "node_modules/lodash": {"version": "4.17.20", "resolved": "https://registry.npmjs.org/lodash/-/lodash-4.17.20.tgz"}
  }
}
"#,
    )?;
    write(root, "actions-negative/syntax.yml", "name: invalid\non: push\ninvalid_workflow_key: true\njobs:\n  fixture:\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo never-executed\n")?;
    write(
        root,
        "actions-negative/security.yml",
        r#"name: never-executed-security-control
on: pull_request_target
permissions: write-all
jobs:
  fixture:
    runs-on: ubuntu-latest
    steps:
      - run: echo "${{ github.event.pull_request.title }}"
"#,
    )?;
    write(
        root,
        "sast-positive/secure.rs",
        "fn main() { let _ = reqwest::Client::builder().build(); }\n",
    )?;
    write(root, "actions-positive/valid.yml", "name: safe-control\non: push\npermissions:\n  contents: read\njobs:\n  fixture:\n    runs-on: ubuntu-latest\n    timeout-minutes: 1\n    steps:\n      - run: echo never-executed\n")?;
    write(
        root,
        "dependency-positive/package-lock.json",
        r#"{"name":"isolated-clean-control","version":"0.0.0","lockfileVersion":3,"packages":{"":{"name":"isolated-clean-control","version":"0.0.0","dependencies":{"lodash":"4.18.1"}},"node_modules/lodash":{"version":"4.18.1","resolved":"https://registry.npmjs.org/lodash/-/lodash-4.18.1.tgz"}}}"#,
    )?;
    println!(
        "Isolated, non-executed scanner fixtures written to {}",
        root.display()
    );
    Ok(())
}

fn expect(args: &[String], capture_stdout: bool) -> Result<()> {
    let split = args
        .iter()
        .position(|arg| arg == "--")
        .ok_or("Missing command delimiter")?;
    if split < 3 || split + 1 >= args.len() {
        return Err("expect: exit, report, evidence and command required".into());
    }
    let expected: i32 = args[0].parse()?;
    if expected == 0 {
        return Err("Negative control must require a nonzero scanner finding code".into());
    }
    let report = Path::new(&args[1]);
    if report.exists() {
        return Err("Refusing a stale negative-control report".into());
    }
    let mut command = Command::new(&args[split + 1]);
    command.args(&args[split + 2..]);
    let status = if capture_stdout {
        let output = command.output()?;
        fs::write(report, &output.stdout)?;
        output.status
    } else {
        command.status()?
    };
    if status.code() != Some(expected) {
        return Err(format!(
            "Scanner returned {:?}, expected finding code {expected}",
            status.code()
        )
        .into());
    }
    let evidence = fs::read_to_string(report)?;
    for needle in &args[2..split] {
        if !evidence.contains(needle) {
            return Err(format!("Missing expected finding evidence: {needle}").into());
        }
    }
    println!(
        "Scanner rejected isolated fixture with finding code {expected}; report evidence verified."
    );
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<_> = env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("gate") if args.len() == 2 => gate(Path::new(&args[1])),
        Some("fixtures") if args.len() == 2 => fixtures(Path::new(&args[1])),
        Some("expect") => expect(&args[1..], false),
        Some("expect-stdout") => expect(&args[1..], true),
        _ => Err("Usage: negative-controls gate BINARY | fixtures NEW_DIRECTORY | expect[-stdout] EXIT REPORT EVIDENCE... -- COMMAND...".into()),
    }
}
