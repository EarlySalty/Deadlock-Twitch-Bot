use std::collections::HashSet;

use serde::Deserialize;
use tb_chat::crew_guard::{CrewJudge, OpenAiCrewJudge};

#[derive(Debug, Deserialize)]
struct EvalCase {
    id: String,
    source: String,
    context: Vec<String>,
    message: String,
    expected: Expected,
}

#[derive(Debug, Deserialize)]
struct Expected {
    reply: bool,
    escalate: bool,
    patterns: Vec<String>,
}

fn cases() -> Vec<EvalCase> {
    serde_json::from_str(include_str!("fixtures/ricky_eval_cases.json"))
        .expect("Ricky-Eval-Fixtures müssen valides JSON sein")
}

#[test]
fn ricky_eval_fixtures_are_redacted_and_complete() {
    let cases = cases();
    assert!(
        (50..=100).contains(&cases.len()),
        "erwartet 50–100 Fälle, gefunden: {}",
        cases.len()
    );

    let mut ids = HashSet::new();
    for case in cases {
        assert!(ids.insert(case.id.clone()), "doppelte ID: {}", case.id);
        assert_eq!(case.source, "production_redacted", "Quelle: {}", case.id);
        assert!(
            !case.expected.reply,
            "Crew-Guard antwortet nie: {}",
            case.id
        );
        assert!(
            !case.expected.escalate || !case.expected.patterns.is_empty(),
            "Eskalation ohne Kernmuster: {}",
            case.id
        );
        assert!(
            case.expected
                .patterns
                .iter()
                .all(|pattern| matches!(pattern.as_str(), "a" | "b" | "c")),
            "ungültiges Kernmuster: {}",
            case.id
        );

        for text in case.context.iter().chain([&case.message]) {
            assert!(!text.contains("http"), "URL nicht redigiert: {}", case.id);
            assert!(
                !text.contains("discord.gg"),
                "Invite nicht redigiert: {}",
                case.id
            );
            assert!(!text.contains('@'), "Handle nicht redigiert: {}", case.id);
            assert!(
                !text
                    .split(|character: char| !character.is_ascii_digit())
                    .any(|digits| digits.len() >= 6),
                "ID nicht redigiert: {}",
                case.id
            );
        }
    }
}

#[tokio::test]
#[ignore = "Live-Baseline: braucht OPENAI_API_KEY und CREW_GUARD_MODEL"]
async fn ricky_eval_baseline() {
    if std::env::var("OPENAI_API_KEY")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .is_none()
        || std::env::var("CREW_GUARD_MODEL")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .is_none()
    {
        eprintln!("SKIP ricky_eval_baseline: OPENAI_API_KEY/CREW_GUARD_MODEL nicht gesetzt");
        return;
    }

    let cases = cases();
    let judge = OpenAiCrewJudge::from_env();
    let mut full_correct = 0usize;
    let mut escalation_correct = 0usize;
    let mut pattern_correct = 0usize;
    let mut false_positives = 0usize;
    let mut false_negatives = 0usize;
    let mut nonzero_confidence = 0usize;

    for case in &cases {
        let verdict = judge.judge(&case.message, &case.context).await;
        let actual_escalation = verdict.is_crew && verdict.confidence >= 0.7;
        let escalation_ok = actual_escalation == case.expected.escalate;
        let actual_patterns = verdict
            .patterns
            .iter()
            .map(|pattern| pattern.trim().to_ascii_lowercase())
            .collect::<HashSet<_>>();
        let patterns_ok = case
            .expected
            .patterns
            .iter()
            .all(|pattern| actual_patterns.contains(pattern));

        nonzero_confidence += usize::from(verdict.confidence > 0.0);
        escalation_correct += usize::from(escalation_ok);
        pattern_correct += usize::from(patterns_ok);
        full_correct += usize::from(escalation_ok && patterns_ok);
        false_positives += usize::from(actual_escalation && !case.expected.escalate);
        false_negatives += usize::from(!actual_escalation && case.expected.escalate);

        if !escalation_ok || !patterns_ok {
            eprintln!(
                "MISS {} expected_escalate={} actual_escalate={} confidence={:.2} expected_patterns={:?} actual_patterns={:?} reasoning={}",
                case.id,
                case.expected.escalate,
                actual_escalation,
                verdict.confidence,
                case.expected.patterns,
                verdict.patterns,
                verdict.reasoning
            );
        }
    }

    assert!(
        nonzero_confidence > 0,
        "Judge lieferte für alle Fälle fail-safe Confidence 0; Baseline nicht messbar"
    );
    eprintln!(
        "RICKY_BASELINE model={} cases={} full={}/{} escalation={}/{} patterns={}/{} reply={}/{} fp={} fn={}",
        std::env::var("CREW_GUARD_MODEL").unwrap_or_else(|_| "<unset>".to_string()),
        cases.len(),
        full_correct,
        cases.len(),
        escalation_correct,
        cases.len(),
        pattern_correct,
        cases.len(),
        cases.len(),
        cases.len(),
        false_positives,
        false_negatives
    );
}
