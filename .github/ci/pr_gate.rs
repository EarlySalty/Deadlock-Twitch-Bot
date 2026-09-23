//! Dependency-free scope detection and fail-closed aggregation for GitHub Actions.
//! Build/test with rustc; no application secrets, network or LLM are used.
use std::{
    env,
    io::{self, Read},
    process::ExitCode,
};

#[derive(Debug, Default, PartialEq, Eq)]
struct Scope {
    rust: bool,
    frontend: bool,
}

fn classify(paths: &[u8]) -> Result<Scope, String> {
    if !paths.is_empty() && paths.last() != Some(&0) {
        return Err("expected NUL-terminated git diff output".into());
    }
    let text =
        std::str::from_utf8(paths).map_err(|_| "non-UTF-8 path: scope cannot be established")?;
    let mut scope = Scope::default();
    for path in text.split('\0').filter(|p| !p.is_empty()) {
        if path.starts_with("rust/") {
            scope.rust = true;
        } else if ["website/", "bot/admin_dashboard/", "bot/dashboard_v2/"]
            .iter()
            .any(|prefix| path.starts_with(prefix))
        {
            scope.frontend = true;
        } else if !path.starts_with(".github/") && (path.ends_with(".md") || path == "LICENSE") {
            // Documentation may skip compilation, never security scanning.
        } else {
            // Unknown source/config/build changes are conservatively relevant.
            scope.rust = true;
            scope.frontend = true;
        }
    }
    Ok(scope)
}

fn acceptable(required: bool, result: &str) -> bool {
    result == "success" || (!required && result == "skipped")
}

fn boolean(value: &str) -> Result<bool, String> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err("missing or invalid scope output".into()),
    }
}

fn aggregate(mut get: impl FnMut(&str) -> String) -> Result<(), String> {
    for key in [
        "SCOPE_RESULT",
        "MANIFEST_RESULT",
        "SECRETS_RESULT",
        "SAST_RESULT",
        "TRIVY_RESULT",
        "ACTIONS_RESULT",
        "DEEP_RESULT",
    ] {
        if !acceptable(true, &get(key)) {
            return Err(format!("{key} did not succeed"));
        }
    }
    let rust = boolean(&get("RUST_REQUIRED"))?;
    let frontend = boolean(&get("FRONTEND_REQUIRED"))?;
    for (key, required) in [
        ("RUST_SECURITY_RESULT", rust),
        ("RUST_SQLX_RESULT", rust),
        ("FRONTEND_RESULT", frontend),
    ] {
        if !acceptable(required, &get(key)) {
            return Err(format!("{key} failed or was not intentionally skipped"));
        }
    }
    Ok(())
}

fn run() -> Result<(), String> {
    match env::args().nth(1).as_deref() {
        Some("scope") => {
            let mut input = Vec::new();
            io::stdin()
                .read_to_end(&mut input)
                .map_err(|e| e.to_string())?;
            let scope = classify(&input)?;
            println!("rust={}\nfrontend={}", scope.rust, scope.frontend);
            Ok(())
        }
        Some("gate") => aggregate(|key| env::var(key).unwrap_or_default()),
        _ => Err("usage: pr-gate scope|gate".into()),
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("Required PR Gate: {message}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn all_green(key: &str) -> String {
        match key {
            "RUST_REQUIRED" | "FRONTEND_REQUIRED" => "true",
            _ => "success",
        }
        .into()
    }
    #[test]
    fn accepts_success_only_for_required_jobs() {
        assert!(acceptable(true, "success"));
    }
    #[test]
    fn rejects_every_non_success_for_required_jobs() {
        for result in [
            "",
            "failure",
            "cancelled",
            "skipped",
            "neutral",
            "timed_out",
            "unknown",
        ] {
            assert!(!acceptable(true, result), "{result}");
        }
    }
    #[test]
    fn accepts_only_explicitly_irrelevant_skips() {
        assert!(acceptable(false, "skipped"));
        assert!(acceptable(false, "success"));
    }
    #[test]
    fn irrelevant_failure_is_still_red() {
        for result in ["", "failure", "cancelled", "neutral"] {
            assert!(!acceptable(false, result));
        }
    }
    #[test]
    fn all_green_passes() {
        assert!(aggregate(all_green).is_ok());
    }
    #[test]
    fn every_required_dependency_is_checked() {
        for key in [
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
        ] {
            for result in ["failure", "cancelled", "skipped", ""] {
                assert!(
                    aggregate(|k| if k == key {
                        result.into()
                    } else {
                        all_green(k)
                    })
                    .is_err(),
                    "{key}={result}"
                );
            }
        }
    }
    #[test]
    fn missing_or_invalid_scope_fails_closed() {
        for key in ["RUST_REQUIRED", "FRONTEND_REQUIRED"] {
            for value in ["", "TRUE", "null", "0"] {
                assert!(aggregate(|k| if k == key { value.into() } else { all_green(k) }).is_err());
            }
        }
    }
    #[test]
    fn docs_pr_still_needs_security() {
        let get = |key: &str| match key {
            "RUST_REQUIRED" | "FRONTEND_REQUIRED" => "false".into(),
            "RUST_SECURITY_RESULT" | "RUST_SQLX_RESULT" | "FRONTEND_RESULT" => "skipped".into(),
            _ => all_green(key),
        };
        assert!(aggregate(get).is_ok());
        assert!(aggregate(|key| if key == "SECRETS_RESULT" {
            "skipped".into()
        } else {
            get(key)
        })
        .is_err());
    }
    #[test]
    fn docs_scope() {
        assert_eq!(
            classify(b"README.md\0docs/usage.md\0").unwrap(),
            Scope::default()
        );
    }
    #[test]
    fn empty_diff_scope() {
        assert_eq!(classify(b"").unwrap(), Scope::default());
    }
    #[test]
    fn rust_scope() {
        assert_eq!(
            classify(b"rust/Cargo.lock\0").unwrap(),
            Scope {
                rust: true,
                frontend: false
            }
        );
    }
    #[test]
    fn every_frontend_scope() {
        for path in [
            "website/src/a.tsx\0",
            "bot/admin_dashboard/package.json\0",
            "bot/dashboard_v2/tests/x.ts\0",
        ] {
            assert_eq!(
                classify(path.as_bytes()).unwrap(),
                Scope {
                    rust: false,
                    frontend: true
                }
            );
        }
    }
    #[test]
    fn policy_changes_test_everything() {
        assert_eq!(
            classify(b".github/workflows/required-pr-gate.yml\0").unwrap(),
            Scope {
                rust: true,
                frontend: true
            }
        );
    }
    #[test]
    fn unknown_sources_are_not_silently_skipped() {
        for path in [
            "ops/tool.py\0",
            "new-service/app.go\0",
            "Cargo.toml\0",
            "scripts/build.sh\0",
            "docs/example.rs\0",
        ] {
            assert_eq!(
                classify(path.as_bytes()).unwrap(),
                Scope {
                    rust: true,
                    frontend: true
                }
            );
        }
    }
    #[test]
    fn mixed_scopes() {
        assert_eq!(
            classify(b"website/a.ts\0rust/a.rs\0").unwrap(),
            Scope {
                rust: true,
                frontend: true
            }
        );
    }
    #[test]
    fn newlines_in_filenames_cannot_inject_outputs() {
        assert_eq!(
            classify(b"website/odd\nfile.ts\0").unwrap(),
            Scope {
                rust: false,
                frontend: true
            }
        );
    }
    #[test]
    fn invalid_utf8_is_not_a_green_skip() {
        assert!(classify(&[255, 0]).is_err());
    }
    #[test]
    fn truncated_path_input_is_not_a_green_skip() {
        assert!(classify(b"rust/src/lib.rs").is_err());
    }
}
