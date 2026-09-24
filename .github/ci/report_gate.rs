//! Fail-closed CodeQL SARIF policy. jq parses JSON; this helper never executes
//! analyzed source and never uploads reports or reads repository credentials.
use std::{
    env,
    path::Path,
    process::{Command, ExitCode},
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

// SARIF 2.1.0 sections 3.27.7, 3.52 and 3.54: rule indices belong to
// their referenced component, not to a flattened list of query-pack rules.
const POLICY: &str = r#"
def checked_index($entries; $index):
  if ($entries | type) != "array" or ($index | type) != "number"
  then error("invalid rule or component index")
  elif $index < 0 or ($index | floor) != $index or $index >= ($entries | length)
  then error("rule or component index out of range")
  else $entries[$index] end;
def unique_match($matches):
  if ($matches | length) != 1 then error("unknown or ambiguous reference")
  else $matches[0] end;
def prefer($primary; $fallback):
  if $primary == null then $fallback else $primary end;
def rule_for($run; $finding):
  prefer($finding.rule; {}) as $ref |
  prefer($ref.toolComponent; {}) as $component_ref |
  if ($ref | type) != "object" or ($component_ref | type) != "object"
  then error("invalid rule reference") else . end |
  if ($finding.ruleId != null and $ref.id != null and $finding.ruleId != $ref.id)
    or ($finding.ruleIndex != null and $ref.index != null and $finding.ruleIndex != $ref.index)
  then error("conflicting rule references") else . end |
  (if $component_ref.index != null
   then checked_index($run.tool.extensions; $component_ref.index)
   elif $component_ref.guid != null
   then unique_match([$run.tool.driver, $run.tool.extensions[]?] |
     map(select(.guid == $component_ref.guid)))
   else $run.tool.driver end) as $component |
  if ($component_ref.name != null and $component_ref.name != $component.name)
    or ($component_ref.guid != null and $component_ref.guid != $component.guid)
  then error("conflicting component identity") else . end |
  prefer($ref.index; $finding.ruleIndex) as $index |
  prefer($ref.id; $finding.ruleId) as $id |
  (if $index != null then checked_index($component.rules; $index)
   elif $ref.guid != null
   then unique_match([$component.rules[]? | select(.guid == $ref.guid)])
   elif ($id | type) == "string" and ($id | length) > 0
   then unique_match([$component.rules[]? | select(.id == $id)])
   else error("missing rule reference") end) as $rule |
  if ($rule | type) != "object" or ($rule.id | type) != "string"
    or ($id != null and $id != $rule.id)
    or ($ref.guid != null and $ref.guid != $rule.guid)
  then error("finding references an unknown rule") else $rule end;
if .version != "2.1.0" or (.runs | type) != "array" or (.runs | length) == 0
then error("missing SARIF analysis") else
  [.runs[] |
    if (.tool.driver | type) != "object" or (.results | type) != "array"
      or (.tool.extensions != null and (.tool.extensions | type) != "array")
    then error("missing rules or results") else . end |
    [.tool.driver, .tool.extensions[]?] as $components |
    if any($components[]; type != "object" or (.rules != null and (.rules | type) != "array"))
      or ([$components[].rules[]?] | length) == 0
    then error("missing or malformed rule metadata") else . end |
    if any(.invocations[]?; .executionSuccessful == false or
        any(.toolExecutionNotifications[]?; .level == "error"))
    then error("unsuccessful scanner invocation") else . end |
    . as $run | .results[] | . as $finding |
    rule_for($run; $finding) as $rule |
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

    // Mirrors CodeQL 4.38.1: the driver has no rules; query packs are
    // tool.extensions and each result carries rule.toolComponent.index.
    fn extension_report(score: &str, results: &str) -> String {
        format!(
            r#"{{"version":"2.1.0","runs":[{{"tool":{{"driver":{{"name":"CodeQL"}},"extensions":[{{"name":"diff-range","rules":[]}},{{"name":"javascript-queries","guid":"11111111-1111-1111-1111-111111111111","rules":[{{"id":"test/security","properties":{{"security-severity":"{score}","tags":["security"]}}}}]}}]}},"results":{results},"invocations":[{{"executionSuccessful":true}}]}}]}}"#
        )
    }

    #[test]
    fn codeql_extension_rules_preserve_the_severity_threshold() {
        let finding = r#"[{"ruleId":"test/security","rule":{"id":"test/security","index":0,"toolComponent":{"index":1}}}]"#;
        for (score, count) in [("6.9", 0), ("7.0", 1), ("9.8", 1)] {
            assert_eq!(
                Fixture::new(&extension_report(score, finding))
                    .result()
                    .unwrap(),
                count
            );
        }
        assert_eq!(
            Fixture::new(&extension_report("9.8", "[]"))
                .result()
                .unwrap(),
            0
        );
    }

    #[test]
    fn component_guid_and_rule_id_resolve_without_array_indices() {
        let finding = r#"[{"ruleId":"test/security","rule":{"id":"test/security","toolComponent":{"guid":"11111111-1111-1111-1111-111111111111"}}}]"#;
        assert_eq!(
            Fixture::new(&extension_report("9.8", finding))
                .result()
                .unwrap(),
            1
        );
    }

    #[test]
    fn malformed_or_conflicting_extension_references_fail_closed() {
        for finding in [
            r#"[{"rule":{"index":0,"toolComponent":{"index":-1}}}]"#,
            r#"[{"rule":{"index":0,"toolComponent":{"index":9}}}]"#,
            r#"[{"rule":{"index":0,"toolComponent":{"index":1.5}}}]"#,
            r#"[{"rule":{"index":0,"toolComponent":{"index":"1"}}}]"#,
            r#"[{"rule":{"index":-1,"toolComponent":{"index":1}}}]"#,
            r#"[{"rule":{"index":7,"toolComponent":{"index":1}}}]"#,
            r#"[{"ruleId":"unknown","rule":{"index":0,"toolComponent":{"index":1}}}]"#,
            r#"[{"ruleId":"test/security","rule":{"id":"other","index":0,"toolComponent":{"index":1}}}]"#,
            r#"[{"ruleIndex":1,"rule":{"index":0,"toolComponent":{"index":1}}}]"#,
            r#"[{"rule":{"index":0,"toolComponent":{"index":1,"name":"wrong-pack"}}}]"#,
            r#"[{"rule":{"index":0,"toolComponent":{"guid":"unknown"}}}]"#,
            r#"[{"rule":{"index":0,"toolComponent":{"index":1,"guid":"wrong"}}}]"#,
            r#"[{"rule":{"index":0,"toolComponent":true}}]"#,
        ] {
            assert!(
                Fixture::new(&extension_report("9.8", finding))
                    .result()
                    .is_err(),
                "{finding}"
            );
        }
    }

    #[test]
    fn negative_driver_indices_cannot_select_a_different_rule() {
        assert!(Fixture::new(&report("6.9", r#"[{"ruleIndex":-1}]"#, "[]"))
            .result()
            .is_err());
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
