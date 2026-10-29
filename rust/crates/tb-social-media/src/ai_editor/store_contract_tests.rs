use std::path::PathBuf;

use chrono::{DateTime, TimeZone, Utc};

use super::super::pipeline::PlanFailure;
use super::super::revision::{rejected_proposal, RevisionProposal};
use super::super::AiEditorError;
use super::{AiJobView, AiVideoVersion, JobFailure, TimelineEntry};

const STATUSES: [&str; 11] = [
    "queued",
    "preparing",
    "transcribing",
    "analyzing",
    "planning",
    "rendering",
    "validating",
    "review",
    "failed",
    "awaiting_confirmation",
    "rejected",
];

const OWNER: i64 = 123_456_789;
const ROOT_JOB: i64 = 1101;
const ROOT_VERSION: i64 = 2201;
const MAX_TEXT_BYTES: usize = 512;

fn at(minute: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 27, 18, minute, 0)
        .single()
        .expect("valid fixture time")
}

fn entry(start_ms: i64, end_ms: i64, description: &str) -> TimelineEntry {
    TimelineEntry {
        start_ms,
        end_ms,
        description: description.to_owned(),
    }
}

fn media_url(job: i64, version: i64, kind: &str) -> String {
    format!(
        "/twitch/social-media/api/ai/jobs/{job}/versions/{version}/{kind}?twitch_user_id={OWNER}"
    )
}

fn version(job: i64, id: i64, version_no: i32, parent: Option<i64>) -> AiVideoVersion {
    AiVideoVersion {
        id,
        version_no,
        created_at: at(10 + version_no as u32),
        status: "review".to_owned(),
        duration_ms: Some(18_400),
        file_url: Some(media_url(job, id, "file")),
        parent_version_id: parent,
        job_id: job,
        variant_index: 0,
        original_url: Some(media_url(job, id, "original")),
        decisions: vec![entry(1_200, 19_600, "Teamkampf in der Mitte bis zum Sieg")],
        approved_platforms: vec!["youtube".to_owned()],
        rating: Some("accepted".to_owned()),
        layout_mode: Some("layout".to_owned()),
        streamer_layout_configured: true,
    }
}

fn base(id: i64, status: &str, stage: &str) -> AiJobView {
    AiJobView {
        id,
        clip_db_id: 501,
        status: status.to_owned(),
        stage: stage.to_owned(),
        created_at: at(0),
        updated_at: at(2),
        completed_at: None,
        provider: None,
        model: None,
        original_duration_ms: Some(24_000),
        output_duration_ms: None,
        processing_ms: None,
        provider_latency_ms: None,
        rendering_ms: None,
        input_tokens: None,
        output_tokens: None,
        estimated_provider_cost_usd: None,
        measured_provider_cost_usd: None,
        cost_basis: "unknown".to_owned(),
        highlights: Vec::new(),
        decisions: Vec::new(),
        uncertainty: Vec::new(),
        review_reasons: Vec::new(),
        original_url: None,
        versions: Vec::new(),
        failure_code: None,
        failure_message: None,
        can_retry: false,
        root_job_id: id,
        parent_version_id: None,
        selected_version_id: None,
        revision_proposal: None,
        publishing_in_autopilot: false,
    }
}

fn analysed(mut job: AiJobView) -> AiJobView {
    job.provider = Some("anthropic".to_owned());
    job.model = Some("model-under-test".to_owned());
    job.processing_ms = Some(41_000);
    job.provider_latency_ms = Some(9_800);
    job.input_tokens = Some(18_250);
    job.output_tokens = Some(2_140);
    job.estimated_provider_cost_usd = Some(0.0912);
    job.cost_basis = "usage_priced_estimate".to_owned();
    job.highlights = vec![entry(4_000, 16_000, "Doppelter Abschuss am Mittelturm")];
    job
}

fn finished(mut job: AiJobView) -> AiJobView {
    job = analysed(job);
    job.completed_at = Some(at(9));
    job.output_duration_ms = Some(18_400);
    job.rendering_ms = Some(12_300);
    job.decisions = vec![entry(1_200, 19_600, "Teamkampf in der Mitte bis zum Sieg")];
    job.uncertainty = vec!["Der Ton ist am Anfang leise.".to_owned()];
    job.review_reasons = vec!["Bitte prüfe den Einstieg.".to_owned()];
    job.versions = vec![version(job.id, ROOT_VERSION, 1, None)];
    job.selected_version_id = Some(ROOT_VERSION);
    job.original_url = Some(media_url(job.id, ROOT_VERSION, "original"));
    job
}

fn revision_child(id: i64, status: &str, stage: &str) -> AiJobView {
    let mut job = analysed(base(id, status, stage));
    job.root_job_id = ROOT_JOB;
    job.parent_version_id = Some(ROOT_VERSION);
    job.selected_version_id = Some(ROOT_VERSION);
    job
}

fn failed_revision() -> AiJobView {
    let mut job = revision_child(1127, "failed", "planning");
    let failure = PlanFailure::from(AiEditorError::Provider);
    job.completed_at = Some(at(4));
    job.failure_code = Some(JobFailure::ProviderUnavailable.code().to_owned());
    job.failure_message = Some(JobFailure::ProviderUnavailable.message().to_owned());
    job.can_retry = true;
    job.revision_proposal = Some(rejected_proposal(&failure.answer(), MAX_TEXT_BYTES));
    job
}

fn awaiting_confirmation() -> AiJobView {
    let mut job = revision_child(1104, "awaiting_confirmation", "planning");
    job.revision_proposal = Some(RevisionProposal {
        summary: "Neue Analyse für einen späteren Einstieg".to_owned(),
        impact: "Die KI schaut sich den Clip noch einmal an und setzt den Start später an."
            .to_owned(),
        reanalyze: true,
        requires_confirmation: true,
        estimated_provider_cost_usd: Some(0.0415),
    });
    job
}

fn rejected() -> AiJobView {
    let mut job = revision_child(1105, "rejected", "planning");
    job.completed_at = Some(at(5));
    job.revision_proposal = Some(RevisionProposal {
        summary: "Neue Analyse für einen späteren Einstieg".to_owned(),
        impact: "Du hast den Vorschlag abgelehnt, die bisherige Fassung bleibt bestehen."
            .to_owned(),
        reanalyze: true,
        requires_confirmation: true,
        estimated_provider_cost_usd: Some(0.0415),
    });
    job
}

fn view_for(status: &str) -> AiJobView {
    match status {
        "queued" => base(1001, status, "preparing"),
        "preparing" | "transcribing" => base(1002, status, status),
        "analyzing" | "planning" | "rendering" | "validating" => {
            analysed(base(1003, status, status))
        }
        "review" => finished(base(ROOT_JOB, status, "validating")),
        "failed" => failed_revision(),
        "awaiting_confirmation" => awaiting_confirmation(),
        _ => rejected(),
    }
}

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../bot/dashboard_v2/src/components/socialmedia/ai/fixtures")
}

fn rendered(status: &str) -> String {
    let json = serde_json::to_string_pretty(&view_for(status)).expect("view serialises");
    format!("{json}\n")
}

fn write_mode() -> bool {
    std::env::var("AIJOB_FIXTURES_WRITE").is_ok_and(|value| value == "1")
}

fn committed_names() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(fixture_dir())
        .expect("fixture directory exists")
        .map(|entry| {
            entry
                .expect("fixture entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|name| name.ends_with(".json"))
        .collect();
    names.sort();
    names
}

#[test]
fn failed_revision_fixture_carries_a_non_empty_impact() {
    let proposal = failed_revision().revision_proposal.expect("proposal");
    assert!(!proposal.impact.trim().is_empty());
    assert!(!proposal.summary.trim().is_empty());
}

#[test]
fn aijob_fixtures_match_the_serialised_views() {
    let dir = fixture_dir();
    if write_mode() {
        std::fs::create_dir_all(&dir).expect("fixture directory");
        for status in STATUSES {
            std::fs::write(dir.join(format!("{status}.json")), rendered(status))
                .expect("fixture written");
        }
    }
    let mut expected: Vec<String> = STATUSES
        .iter()
        .map(|status| format!("{status}.json"))
        .collect();
    expected.sort();
    assert_eq!(
        committed_names(),
        expected,
        "fixture set differs from the known statuses"
    );
    for status in STATUSES {
        let path = dir.join(format!("{status}.json"));
        let committed = std::fs::read_to_string(&path).expect("fixture readable");
        assert_eq!(
            committed,
            rendered(status),
            "{} drifted from AiJobView, regenerate with AIJOB_FIXTURES_WRITE=1",
            path.display()
        );
    }
}
