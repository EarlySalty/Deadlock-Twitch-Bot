//! Fail-closed CodeQL SARIF policy. jq parses JSON; this helper never executes
//! analyzed source and never uploads reports or reads repository credentials.
use std::{
    env,
    path::Path,
    process::{Command, ExitCode},
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const POLICY: &str = r#"
if .version != "2.1.0" or (.runs | type) != "array" or (.runs | length) == 0
then error("missing SARIF analysis") else
  [.runs[] |
    if (.tool.driver.rules | type) != "array" or (.results | type) != "array"
    then error("missing rules or results") else . end |
    if any(.invocations[]?; .executionSuccessful == false or
        any(.toolExecutionNotifications[]?; .level == "error"))
    then error("unsuccessful scanner invocation") else . end |
    . as $run | .results[] | . as $finding |
    (if .ruleIndex != null then $run.tool.driver.rules[.ruleIndex]
     else [$run.tool.driver.rules[] | select(.id == $finding.ruleId)][0] end) as $rule |
    if $rule == null or ($finding.ruleId != null and $finding.ruleId != $rule.id)
    then error("finding references an unknown rule") else . end |
    ($rule.properties["security-severity"] // null) as $score |
    if $score == null and (($rule.properties.tags // []) | index("security")) != null
    then error("unscored security finding") else . end |
    if (($score // "0" | tostring | test("^[0-9]+(\\.[0-9]+)?$")) | not)
    then error("invalid security score") else . end |
    (($score // "0") | tonumber) as $severity |
    if $severity < 0 or $severity > 10 then error("invalid security score") else . end |
    (.level // $rule.defaultConfiguration.level // "warning") as $level |
    if (["none", "note", "warning", "error"] | index($level)) == null
    then error("invalid finding level") else . end |
    select($severity >= 7 or $level == "error")
  ] | length
end
"#;

fn count_blocking(report: &Path) -> Result<u64> {
    let result = Command::new("jq")
        .args(["--exit-status", POLICY])
        .arg(report)
        .output()?;
    if !result.status.success() {
        return Err(
            "Missing, malformed or incomplete CodeQL evidence; merge remains blocked".into(),
        );
    }
    Ok(std::str::from_utf8(&result.stdout)?.trim().parse()?)
}

fn main() -> ExitCode {
    let args: Vec<_> = env::args_os().skip(1).collect();
    let result = if args.len() == 1 {
        count_blocking(Path::new(&args[0]))
    } else {
        Err("Usage: report-gate CODEQL_SARIF".into())
    };
    match result {
        Ok(0) => {
            println!("CodeQL policy: no HIGH/CRITICAL or ERROR findings; evidence is structurally complete.");
            ExitCode::SUCCESS
        }
        Ok(count) => {
            eprintln!(
                "CodeQL policy: {count} blocking findings; see the preserved SARIF artifact."
            );
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("CodeQL policy: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
    };
    static NEXT: AtomicUsize = AtomicUsize::new(0);

    struct Fixture(PathBuf);
    impl Fixture {
        fn new(content: &str) -> Self {
            let dir = env::temp_dir().join(format!(
                "tb-sarif-control-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&dir).unwrap();
            fs::write(dir.join("report.json"), content).unwrap();
            Self(dir)
        }
        fn result(&self) -> Result<u64> {
            count_blocking(&self.0.join("report.json"))
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    fn report(score: &str, results: &str, invocation: &str) -> String {
        format!(
            r#"{{"version":"2.1.0","runs":[{{"tool":{{"driver":{{"rules":[{{"id":"test/security","properties":{{"security-severity":"{score}","tags":["security"]}}}}]}}}},"results":{results},"invocations":{invocation}}}]}}"#
        )
    }

    #[test]
    fn clean_analysis_is_accepted() {
        assert_eq!(
            Fixture::new(&report("8.1", "[]", "[]")).result().unwrap(),
            0
        );
    }

    #[test]
    fn high_and_critical_findings_block_by_rule_id_or_index() {
        for score in ["7.0", "9.8"] {
            for finding in [
                r#"[{"ruleId":"test/security"}]"#,
                r#"[{"ruleId":"test/security","ruleIndex":0}]"#,
            ] {
                assert_eq!(
                    Fixture::new(&report(score, finding, "[]"))
                        .result()
                        .unwrap(),
                    1
                );
            }
        }
    }

    #[test]
    fn below_threshold_is_explicitly_nonblocking() {
        assert_eq!(
            Fixture::new(&report("6.9", r#"[{"ruleId":"test/security"}]"#, "[]"))
                .result()
                .unwrap(),
            0
        );
    }

    #[test]
    fn error_level_is_blocking_even_below_cvss_threshold() {
        assert_eq!(
            Fixture::new(&report(
                "6.9",
                r#"[{"ruleId":"test/security","level":"error"}]"#,
                "[]"
            ))
            .result()
            .unwrap(),
            1
        );
    }

    #[test]
    fn missing_or_malformed_analysis_is_not_a_clean_scan() {
        for content in [
            "{}",
            "not json",
            r#"{"version":"2.1.0","runs":[]}"#,
            &report("8.1", "null", "[]"),
        ] {
            assert!(Fixture::new(content).result().is_err());
        }
        assert!(count_blocking(Path::new("/a-nonexistent-tb-ci-report/report.json")).is_err());
    }

    #[test]
    fn unsuccessful_invocations_are_rejected() {
        for invocation in [
            r#"[{"executionSuccessful":false}]"#,
            r#"[{"executionSuccessful":true,"toolExecutionNotifications":[{"level":"error"}]}]"#,
        ] {
            assert!(Fixture::new(&report("8.1", "[]", invocation))
                .result()
                .is_err());
        }
    }

    #[test]
    fn unknown_rules_and_invalid_scores_cannot_drop_findings() {
        assert!(
            Fixture::new(&report("8.1", r#"[{"ruleId":"unknown"}]"#, "[]"))
                .result()
                .is_err()
        );
        for score in ["unknown", "11", "-1", "NaN", "Infinity"] {
            assert!(Fixture::new(&report(score, r#"[{"ruleIndex":0}]"#, "[]"))
                .result()
                .is_err());
        }
        let unscored = report("8.1", r#"[{"ruleIndex":0}]"#, "[]")
            .replace(r#""security-severity":"8.1","#, "");
        assert!(Fixture::new(&unscored).result().is_err());
    }
}
