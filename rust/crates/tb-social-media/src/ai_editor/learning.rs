use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{PgPool, Row};

use super::{
    skill_registry::{self, LearnedChange, NewProposal, SkillEvidence, SkillRegistryError},
    SkillId,
};

const PERSONAL_MIN_REVIEWS: u32 = 2;
const GLOBAL_MIN_USERS: u32 = 3;
const LEARNED_HEADING: &str = "## Learned guidance";
const LEARNING_ACTOR: &str = "ai-learning";
const MAX_USERS_PER_PASS: i64 = 200;
const MAX_EVENTS_PER_USER: i64 = 5000;
const PASS_LOCK: &str = "social_ai_learning_pass";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dimension {
    SubtitleSize,
    IntroLength,
    VideoLength,
    EditingPace,
    KeepAction,
    KeepReaction,
    FacecamOcclusion,
    GameUnderstanding,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Signal {
    Toward(Dimension, i8),
    Restore(Dimension),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Accepted,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedbackEvent {
    pub root_job_id: i64,
    pub job_id: i64,
    pub kind: String,
    pub feedback: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersonalPreference {
    pub dimension: Dimension,
    pub direction: i8,
    pub supporting_reviews: u32,
    pub opposing_reviews: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersonalProfile {
    pub preferences: Vec<PersonalPreference>,
    pub feedback_count: u32,
    pub accepted_versions: u32,
    pub rejected_versions: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalRule {
    pub dimension: Dimension,
    pub direction: i8,
    pub supporting_users: u32,
    pub opposing_users: u32,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct PassReport {
    pub profiles_updated: usize,
    pub skills_changed: Vec<(SkillId, i32)>,
    pub proposals_recorded: usize,
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

const SMALLER: &[&str] = &[
    "kleiner",
    "smaller",
    "zu groß",
    "zu gross",
    "too big",
    "too large",
];
const LARGER: &[&str] = &[
    "größer",
    "groesser",
    "grösser",
    "bigger",
    "larger",
    "zu klein",
    "too small",
];
const SHORTER: &[&str] = &["kürzer", "kuerzer", "shorter", "zu lang", "too long"];
const LONGER: &[&str] = &["länger", "laenger", "longer", "zu kurz", "too short"];
const RESTORE: &[&str] = &[
    "wie vorher",
    "wie zuvor",
    "ursprünglich",
    "urspruenglich",
    "original size",
    "originalgröße",
    "zurück auf",
    "back to",
    "restore",
];

fn size_signal(dimension: Dimension, text: &str) -> Option<Signal> {
    if contains_any(text, RESTORE) {
        Some(Signal::Restore(dimension))
    } else if contains_any(text, SMALLER) {
        Some(Signal::Toward(dimension, -1))
    } else if contains_any(text, LARGER) {
        Some(Signal::Toward(dimension, 1))
    } else {
        None
    }
}

fn length_signal(dimension: Dimension, text: &str) -> Option<Signal> {
    if contains_any(text, RESTORE) {
        Some(Signal::Restore(dimension))
    } else if contains_any(text, SHORTER) {
        Some(Signal::Toward(dimension, -1))
    } else if contains_any(text, LONGER) {
        Some(Signal::Toward(dimension, 1))
    } else {
        None
    }
}

const KEEP_PHRASES: &[&str] = &[
    "nicht rausschneiden",
    "nicht schneiden",
    "nicht kürzen",
    "don't cut",
    "do not cut",
];
const NEGATIONS: &[&str] = &[
    "nicht", "nichts", "kein", "keine", "keinen", "keiner", "keinem", "nie", "niemals", "ohne",
    "not", "no", "never", "without", "don't", "dont", "doesn't", "didn't", "isn't", "aren't",
];

fn negated(clause: &str) -> bool {
    let mut rest = clause.to_owned();
    for phrase in KEEP_PHRASES {
        rest = rest.replace(phrase, " ");
    }
    rest.split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .any(|word| NEGATIONS.contains(&word))
}

fn classify(feedback: &str) -> Vec<Signal> {
    let text = feedback.to_lowercase();
    let mut signals = Vec::new();
    for clause in text.split(['.', '!', '?', ';', ',', '\n']) {
        let clause = clause.trim();
        if clause.is_empty() || negated(clause) {
            continue;
        }
        if contains_any(clause, &["untertitel", "subtitle", "caption"]) {
            signals.extend(size_signal(Dimension::SubtitleSize, clause));
        } else if contains_any(clause, &["intro", "einstieg", "anfang"]) {
            signals.extend(length_signal(Dimension::IntroLength, clause));
        } else if contains_any(clause, &["video", "clip", "schnitt insgesamt", "overall"]) {
            signals.extend(length_signal(Dimension::VideoLength, clause));
        }
        if contains_any(
            clause,
            &[
                "weniger schnitte",
                "fewer cuts",
                "ruhiger",
                "calmer",
                "less editing",
            ],
        ) {
            signals.push(Signal::Toward(Dimension::EditingPace, -1));
        } else if contains_any(
            clause,
            &[
                "mehr schnitte",
                "more cuts",
                "dynamischer",
                "faster pacing",
                "more dynamic",
            ],
        ) {
            signals.push(Signal::Toward(Dimension::EditingPace, 1));
        }
        if contains_any(clause, &["teamfight", "kampf", "fight"])
            && contains_any(
                clause,
                &["mehr", "more", "behalt", "keep", "ganz", "komplett", "full"],
            )
        {
            signals.push(Signal::Toward(Dimension::KeepAction, 1));
        }
        if contains_any(clause, &["reaktion", "reaction"])
            && (contains_any(clause, &["behalt", "keep", "lass", "drin"])
                || contains_any(clause, KEEP_PHRASES))
        {
            signals.push(Signal::Toward(Dimension::KeepReaction, 1));
        }
        if contains_any(clause, &["facecam", "webcam", "kamera", "camera"])
            && contains_any(
                clause,
                &[
                    "verdeck",
                    "überdeck",
                    "ueberdeck",
                    "covering",
                    "covers",
                    "blocks",
                    "im weg",
                    "in the way",
                ],
            )
        {
            signals.push(Signal::Toward(Dimension::FacecamOcclusion, 1));
        }
        if contains_any(
            clause,
            &[
                "missverstanden",
                "falsch verstanden",
                "misunderstood",
                "falsch interpretiert",
                "misread",
            ],
        ) {
            signals.push(Signal::Toward(Dimension::GameUnderstanding, 1));
        }
    }
    signals
}

fn outcomes(events: &[FeedbackEvent]) -> BTreeMap<i64, Outcome> {
    let mut map = BTreeMap::new();
    for event in events {
        match event.kind.as_str() {
            "version_accepted" => {
                map.insert(event.job_id, Outcome::Accepted);
            }
            "version_rejected" | "proposal_rejected" => {
                map.insert(event.job_id, Outcome::Rejected);
            }
            _ => {}
        }
    }
    map
}

fn review_directions(events: &[FeedbackEvent]) -> BTreeMap<i64, BTreeMap<Dimension, i32>> {
    let decided = outcomes(events);
    let mut reviews: BTreeMap<i64, BTreeMap<Dimension, i32>> = BTreeMap::new();
    for event in events.iter().filter(|event| event.kind == "feedback") {
        if decided.get(&event.job_id) != Some(&Outcome::Accepted) {
            continue;
        }
        let Some(feedback) = &event.feedback else {
            continue;
        };
        let nets = reviews.entry(event.root_job_id).or_default();
        for signal in classify(feedback) {
            match signal {
                Signal::Toward(dimension, direction) => {
                    *nets.entry(dimension).or_default() += i32::from(direction);
                }
                Signal::Restore(dimension) => {
                    nets.insert(dimension, 0);
                }
            }
        }
    }
    reviews
}

pub fn personal_profile(events: &[FeedbackEvent]) -> PersonalProfile {
    let mut tally: BTreeMap<Dimension, (u32, u32)> = BTreeMap::new();
    for nets in review_directions(events).values() {
        for (dimension, net) in nets {
            let entry = tally.entry(*dimension).or_default();
            match net.signum() {
                1 => entry.0 += 1,
                -1 => entry.1 += 1,
                _ => {}
            }
        }
    }
    let preferences = tally
        .into_iter()
        .filter_map(|(dimension, (up, down))| {
            let (direction, supporting, opposing) = if up >= down {
                (1, up, down)
            } else {
                (-1, down, up)
            };
            (supporting >= PERSONAL_MIN_REVIEWS && supporting >= 2 * opposing).then_some(
                PersonalPreference {
                    dimension,
                    direction,
                    supporting_reviews: supporting,
                    opposing_reviews: opposing,
                },
            )
        })
        .collect();
    let count = |kind: &str| events.iter().filter(|event| event.kind == kind).count() as u32;
    PersonalProfile {
        preferences,
        feedback_count: count("feedback"),
        accepted_versions: count("version_accepted"),
        rejected_versions: count("version_rejected"),
    }
}

pub fn global_rules(profiles: &[PersonalProfile]) -> Vec<GlobalRule> {
    let mut tally: BTreeMap<Dimension, (u32, u32)> = BTreeMap::new();
    for profile in profiles {
        for preference in &profile.preferences {
            let entry = tally.entry(preference.dimension).or_default();
            if preference.direction > 0 {
                entry.0 += 1;
            } else {
                entry.1 += 1;
            }
        }
    }
    tally
        .into_iter()
        .filter_map(|(dimension, (up, down))| {
            let (direction, supporting, opposing) = if up >= down {
                (1, up, down)
            } else {
                (-1, down, up)
            };
            (supporting >= GLOBAL_MIN_USERS && supporting >= 2 * opposing).then_some(GlobalRule {
                dimension,
                direction,
                supporting_users: supporting,
                opposing_users: opposing,
            })
        })
        .collect()
}

fn target_skill(dimension: Dimension) -> SkillId {
    match dimension {
        Dimension::SubtitleSize => SkillId::Subtitles,
        Dimension::IntroLength
        | Dimension::VideoLength
        | Dimension::EditingPace
        | Dimension::KeepReaction => SkillId::EditingStrategy,
        Dimension::KeepAction => SkillId::HighlightAnalysis,
        Dimension::FacecamOcclusion => SkillId::VideoLayout,
        Dimension::GameUnderstanding => SkillId::DeadlockUnderstanding,
    }
}

fn guidance(dimension: Dimension, direction: i8) -> &'static str {
    match (dimension, direction > 0) {
        (Dimension::SubtitleSize, false) => "Prefer a compact subtitle size that keeps gameplay visible.",
        (Dimension::SubtitleSize, true) => "Prefer a larger, easily readable subtitle size.",
        (Dimension::IntroLength, false) => "Keep the intro short and reach the first action quickly.",
        (Dimension::IntroLength, true) => "Allow more setup before the first action.",
        (Dimension::VideoLength, false) => "Prefer tighter overall edits without filler.",
        (Dimension::VideoLength, true) => "Keep more context around the main action.",
        (Dimension::EditingPace, false) => "Prefer fewer, calmer cuts.",
        (Dimension::EditingPace, true) => "Prefer a more dynamic pace with more cuts.",
        (Dimension::KeepAction, _) => "Keep complete teamfights instead of cutting them short.",
        (Dimension::KeepReaction, _) => "Keep the streamer's original reaction after the key moment.",
        (Dimension::FacecamOcclusion, _) => "Check that the facecam does not cover important action and flag the video for review when unsure.",
        (Dimension::GameUnderstanding, _) => "Mark hero ability interpretations as uncertain unless they are clearly visible.",
    }
}

fn summary(dimension: Dimension, direction: i8) -> &'static str {
    match (dimension, direction > 0) {
        (Dimension::SubtitleSize, false) => "Kompaktere Untertitel bevorzugt",
        (Dimension::SubtitleSize, true) => "Größere Untertitel bevorzugt",
        (Dimension::IntroLength, false) => "Kürzerer Einstieg bevorzugt",
        (Dimension::IntroLength, true) => "Längerer Einstieg bevorzugt",
        (Dimension::VideoLength, false) => "Straffere Schnitte bevorzugt",
        (Dimension::VideoLength, true) => "Mehr Kontext um die Hauptszene bevorzugt",
        (Dimension::EditingPace, false) => "Ruhigere Schnittfolge bevorzugt",
        (Dimension::EditingPace, true) => "Dynamischere Schnittfolge bevorzugt",
        (Dimension::KeepAction, _) => "Teamfights vollständig behalten",
        (Dimension::KeepReaction, _) => "Originalreaktion behalten",
        (Dimension::FacecamOcclusion, _) => "Facecam-Überdeckung prüfen",
        (Dimension::GameUnderstanding, _) => "Fähigkeiten vorsichtiger deuten",
    }
}

fn proposal_for(dimension: Dimension) -> Option<(&'static str, &'static str)> {
    match dimension {
        Dimension::FacecamOcclusion => Some((
            "Facecam-Überdeckung automatisch erkennen",
            "Eine Bildprüfung im Render-Schritt könnte erkennen, ob die Facecam wichtige Spielszenen verdeckt, und das Layout vor dem Rendern anpassen.",
        )),
        Dimension::GameUnderstanding => Some((
            "Fähigkeitserkennung im Spielverständnis verbessern",
            "Wiederholte Hinweise auf falsch verstandene Fähigkeiten deuten auf fehlende Spieldaten im Analysekontext hin. Eine gezielte Anreicherung mit Fähigkeitsdaten aus dem Deadlock Brain sollte manuell geprüft werden.",
        )),
        _ => None,
    }
}

fn strip_learned(text: &str) -> &str {
    match text.find(&format!("\n{LEARNED_HEADING}")) {
        Some(index) => text[..index].trim_end(),
        None => text.trim_end(),
    }
}

pub fn compose_skill_text(active: &str, rules: &[&GlobalRule]) -> String {
    let base = strip_learned(active);
    let mut lines: Vec<&str> = rules
        .iter()
        .map(|rule| guidance(rule.dimension, rule.direction))
        .collect();
    lines.sort_unstable();
    lines.dedup();
    if lines.is_empty() {
        return format!("{base}\n");
    }
    let mut text = format!("{base}\n\n{LEARNED_HEADING}\n\n");
    for line in lines {
        text.push_str("- ");
        text.push_str(line);
        text.push('\n');
    }
    text
}

pub fn planning_hints(profile: &PersonalProfile) -> Vec<String> {
    profile
        .preferences
        .iter()
        .map(|preference| guidance(preference.dimension, preference.direction).to_owned())
        .collect()
}

pub async fn personal_hints(pool: &PgPool, owner: &str) -> Result<Vec<String>, sqlx::Error> {
    let Ok(owner) = owner.parse::<i64>() else {
        return Ok(Vec::new());
    };
    let profile: Option<Value> = sqlx::query_scalar(
        "SELECT profile FROM public.social_ai_personal_preferences WHERE twitch_user_id = $1",
    )
    .bind(owner)
    .fetch_optional(pool)
    .await?;
    Ok(profile
        .and_then(|value| serde_json::from_value::<PersonalProfile>(value).ok())
        .map(|profile| planning_hints(&profile))
        .unwrap_or_default())
}

async fn stale_users(pool: &PgPool) -> Result<Vec<i64>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT e.twitch_user_id FROM public.social_ai_revision_events e LEFT JOIN public.social_ai_personal_preferences p ON p.twitch_user_id = e.twitch_user_id GROUP BY e.twitch_user_id, p.last_event_id HAVING MAX(e.id) > COALESCE(p.last_event_id, 0) ORDER BY e.twitch_user_id LIMIT $1",
    )
    .bind(MAX_USERS_PER_PASS)
    .fetch_all(pool)
    .await
}

async fn user_events(pool: &PgPool, user: i64) -> Result<(Vec<FeedbackEvent>, i64), sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, root_job_id, job_id, event_kind, payload->>'feedback' AS feedback FROM public.social_ai_revision_events WHERE twitch_user_id = $1 ORDER BY id DESC LIMIT $2",
    )
    .bind(user)
    .bind(MAX_EVENTS_PER_USER)
    .fetch_all(pool)
    .await?;
    let mut last = 0;
    let mut events = Vec::with_capacity(rows.len());
    for row in rows.iter().rev() {
        let id: i64 = row.try_get("id")?;
        last = last.max(id);
        events.push(FeedbackEvent {
            root_job_id: row.try_get("root_job_id")?,
            job_id: row.try_get("job_id")?,
            kind: row.try_get("event_kind")?,
            feedback: row.try_get("feedback")?,
        });
    }
    Ok((events, last))
}

pub async fn refresh_personal(pool: &PgPool) -> Result<usize, sqlx::Error> {
    let users = stale_users(pool).await?;
    for user in &users {
        let (events, last) = user_events(pool, *user).await?;
        let profile = serde_json::to_value(personal_profile(&events))
            .map_err(|error| sqlx::Error::Encode(Box::new(error)))?;
        sqlx::query(
            "INSERT INTO public.social_ai_personal_preferences (twitch_user_id, profile, last_event_id) VALUES ($1, $2, $3) ON CONFLICT (twitch_user_id) DO UPDATE SET profile = EXCLUDED.profile, last_event_id = EXCLUDED.last_event_id, updated_at = now() WHERE social_ai_personal_preferences.last_event_id <= EXCLUDED.last_event_id",
        )
        .bind(user)
        .bind(profile)
        .bind(last)
        .execute(pool)
        .await?;
    }
    Ok(users.len())
}

async fn all_profiles(pool: &PgPool) -> Result<Vec<PersonalProfile>, sqlx::Error> {
    let values: Vec<Value> =
        sqlx::query_scalar("SELECT profile FROM public.social_ai_personal_preferences")
            .fetch_all(pool)
            .await?;
    Ok(values
        .into_iter()
        .filter_map(|value| serde_json::from_value(value).ok())
        .collect())
}

async fn was_rolled_back(pool: &PgPool, id: SkillId, sha256: &str) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM public.social_ai_skill_activations a JOIN public.social_ai_skill_versions v ON v.skill_id = a.skill_id AND v.version = a.from_version WHERE a.skill_id = $1 AND a.kind = 'rollback' AND v.sha256 = $2)",
    )
    .bind(id.as_str())
    .bind(sha256)
    .fetch_one(pool)
    .await
}

fn evidence_text(rules: &[&GlobalRule]) -> (String, i64) {
    let supporting: u32 = rules
        .iter()
        .map(|rule| rule.supporting_users)
        .max()
        .unwrap_or(0);
    let parts: Vec<String> = rules
        .iter()
        .map(|rule| {
            format!(
                "{}: {} Kanäle dafür, {} dagegen",
                summary(rule.dimension, rule.direction),
                rule.supporting_users,
                rule.opposing_users
            )
        })
        .collect();
    (
        format!(
            "Anonymisierte Auswertung wiederkehrender Rückmeldungen. {}",
            parts.join("; ")
        ),
        i64::from(supporting),
    )
}

async fn apply_skill(
    pool: &PgPool,
    id: SkillId,
    rules: &[&GlobalRule],
    max_bytes: usize,
) -> Result<Option<i32>, SkillRegistryError> {
    let active = skill_registry::active_skill(pool, id, max_bytes).await?;
    let candidate = compose_skill_text(&active.text, rules);
    let sha256 = skill_registry::sha256_hex(&candidate);
    if sha256 == active.sha256
        || skill_registry::validate_candidate(id, &candidate, max_bytes).is_err()
        || was_rolled_back(pool, id, &sha256).await?
    {
        return Ok(None);
    }
    let summary_text = if rules.is_empty() {
        "Gelernte Hinweise entfernt".to_owned()
    } else {
        rules
            .iter()
            .map(|rule| summary(rule.dimension, rule.direction))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let (evidence_summary, sample_count) = evidence_text(rules);
    let evidence = SkillEvidence {
        summary: evidence_summary,
        sample_count: Some(sample_count),
    };
    let version = skill_registry::activate_learned(
        pool,
        id,
        LearnedChange {
            text: &candidate,
            summary: &summary_text,
            reason: "Mehrere Kanäle haben unabhängig voneinander dieselbe Korrektur bestätigt.",
            evidence: Some(&evidence),
            actor: LEARNING_ACTOR,
            max_bytes,
            expected_version: Some(active.version),
        },
    )
    .await;
    match version {
        Ok(version) => Ok(Some(version)),
        Err(SkillRegistryError::Conflict) => Ok(None),
        Err(error) => Err(error),
    }
}

async fn record_proposals(
    pool: &PgPool,
    rules: &[GlobalRule],
) -> Result<usize, SkillRegistryError> {
    let mut recorded = 0;
    for rule in rules {
        let Some((title, body)) = proposal_for(rule.dimension) else {
            continue;
        };
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM public.social_ai_change_proposals WHERE title = $1)",
        )
        .bind(title)
        .fetch_one(pool)
        .await?;
        if exists {
            continue;
        }
        let evidence = format!(
            "{} Kanäle mit übereinstimmender Rückmeldung, {} widersprechend.",
            rule.supporting_users, rule.opposing_users
        );
        skill_registry::record_proposal(
            pool,
            NewProposal {
                title,
                summary: body,
                reason: "Diese Verbesserung braucht eine Code- oder Architekturänderung und wird deshalb nur vorgeschlagen.",
                evidence: Some(&evidence),
            },
        )
        .await?;
        recorded += 1;
    }
    Ok(recorded)
}

pub async fn run_pass(
    pool: &PgPool,
    max_skill_bytes: usize,
) -> Result<PassReport, SkillRegistryError> {
    let mut guard = pool.begin().await?;
    let locked: bool = sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(hashtext($1))")
        .bind(PASS_LOCK)
        .fetch_one(&mut *guard)
        .await?;
    if !locked {
        return Ok(PassReport::default());
    }
    let report = locked_pass(pool, max_skill_bytes).await;
    guard.commit().await?;
    report
}

async fn locked_pass(
    pool: &PgPool,
    max_skill_bytes: usize,
) -> Result<PassReport, SkillRegistryError> {
    let profiles_updated = refresh_personal(pool).await?;
    let rules = global_rules(&all_profiles(pool).await?);
    let learned = learned_skills(pool).await?;
    let mut skills_changed = Vec::new();
    for id in SkillId::ALL {
        let mine: Vec<&GlobalRule> = rules
            .iter()
            .filter(|rule| target_skill(rule.dimension) == id)
            .collect();
        if mine.is_empty() && !learned.contains(&id) {
            continue;
        }
        if let Some(version) = apply_skill(pool, id, &mine, max_skill_bytes).await? {
            skills_changed.push((id, version));
        }
    }
    let proposals_recorded = record_proposals(pool, &rules).await?;
    Ok(PassReport {
        profiles_updated,
        skills_changed,
        proposals_recorded,
    })
}

async fn learned_skills(pool: &PgPool) -> Result<Vec<SkillId>, SkillRegistryError> {
    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT a.skill_id FROM public.social_ai_skill_active a JOIN public.social_ai_skill_versions v ON v.skill_id = a.skill_id AND v.version = a.version WHERE v.origin = 'learned'",
    )
    .fetch_all(pool)
    .await?;
    Ok(ids.iter().filter_map(|id| SkillId::parse(id)).collect())
}

#[cfg(test)]
#[path = "learning_tests.rs"]
mod tests;
