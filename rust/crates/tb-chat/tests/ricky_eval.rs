use std::collections::HashSet;

use serde::Deserialize;
use tb_chat::crew_guard::{CrewJudge, OpenAiCrewJudge};
use tb_engagement::crew_review::{parse_review_decision, ReviewError, REVIEW_SYSTEM_PROMPT};

const COMMUNITY_BAN_FACT: &str =
    "Ricky wurde am 29. Mai 2026 aus dem Discord der Deutschen Deadlock Community entfernt.";
const RACIST_GREETING_FACT: &str =
    "Als Grund dafür wurde unter anderem genannt, dass er Leute dort mit dem N-Wort begrüßt habe.";
const TWITCH_PITCH_FACT: &str = "Zwischen dem 29. Mai und 17. Juli 2026 wurden 145 Nachrichten von Rickys Twitch-Account in neun Kanälen gespeichert; in acht davon bot er einen Deadlock-Community-Discord an oder fragte nach Interesse.";

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

fn review_reply(fact_ids: &[&str], draft: &str) -> String {
    serde_json::json!({
        "action": "reply",
        "topic_active": true,
        "confidence": 0.9,
        "used_fact_ids": fact_ids,
        "reason": "fact_based_reply",
        "draft": draft,
    })
    .to_string()
}

#[test]
fn ricky_review_akzeptiert_den_exakten_twitch_snapshot() {
    let parsed = parse_review_decision(&review_reply(&["twitch_pitch_history"], TWITCH_PITCH_FACT))
        .expect("exakter freigegebener Snapshot");

    assert_eq!(parsed.draft.as_deref(), Some(TWITCH_PITCH_FACT));
}

#[test]
fn ricky_review_verwirft_paraphrase_aggressiven_einstieg_und_falsche_quelle() {
    let paraphrase =
        "Von Ende Mai bis Mitte Juli hat Ricky in vielen Twitch-Chats für seinen Discord geworben.";
    let aggressive = format!("Glaub Ricky kein Wort. {COMMUNITY_BAN_FACT}");
    let wrong_source = format!("Laut Twitch-Datenbank: {RACIST_GREETING_FACT}");

    for (fact_ids, draft) in [
        (&["twitch_pitch_history"][..], paraphrase),
        (&["community_ban_2026_05_29"][..], aggressive.as_str()),
        (&["racist_greeting_report"][..], wrong_source.as_str()),
    ] {
        assert_eq!(
            parse_review_decision(&review_reply(fact_ids, draft)),
            Err(ReviewError::Validation)
        );
    }
}

#[test]
fn ricky_review_prompt_verlangt_wortgleiche_fakten_und_trennt_quellen() {
    assert!(REVIEW_SYSTEM_PROMPT.contains("wortgleich und vollständig"));
    assert!(
        REVIEW_SYSTEM_PROMPT.contains("Die Fakten 2 und 3 stammen nicht aus der Twitch-Datenbank")
    );
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
