use std::collections::{BTreeMap, BTreeSet};

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
    FacecamPosition,
    FacecamSize,
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
    Discarded,
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
    #[serde(default)]
    pub rejected_reviews: u32,
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
    pub rules_held_back: usize,
    pub skills_reverted: Vec<(SkillId, i32)>,
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

fn directed_signal(
    dimension: Dimension,
    text: &str,
    lower: &[&str],
    higher: &[&str],
) -> Option<Signal> {
    if contains_any(text, RESTORE) {
        return Some(Signal::Restore(dimension));
    }
    match (contains_any(text, lower), contains_any(text, higher)) {
        (true, false) => Some(Signal::Toward(dimension, -1)),
        (false, true) => Some(Signal::Toward(dimension, 1)),
        _ => None,
    }
}

const FACECAM: &[&str] = &["facecam", "webcam", "kamera", "camera"];
const HIGHER_PLACE: &[&str] = &["oben", "höher", "hoeher", "higher", "top"];
const LOWER_PLACE: &[&str] = &["unten", "tiefer", "lower", "bottom"];

fn facecam_signals(clause: &str) -> Vec<Signal> {
    if !contains_any(clause, FACECAM) {
        return Vec::new();
    }
    [
        directed_signal(
            Dimension::FacecamPosition,
            clause,
            LOWER_PLACE,
            HIGHER_PLACE,
        ),
        size_signal(Dimension::FacecamSize, clause),
    ]
    .into_iter()
    .flatten()
    .collect()
}

fn size_signal(dimension: Dimension, text: &str) -> Option<Signal> {
    directed_signal(dimension, text, SMALLER, LARGER)
}

fn length_signal(dimension: Dimension, text: &str) -> Option<Signal> {
    directed_signal(dimension, text, SHORTER, LONGER)
}

const KEEP_PHRASES: &[&str] = &[
    "nicht rausschneiden",
    "nicht schneiden",
    "nicht kürzen",
    "don't cut",
    "do not cut",
];
const NEGATION_STEMS: &[&str] = &[
    "nicht", "kein", "nie", "nirgend", "weder", "ohne", "statt", "anstatt", "vermeid", "aufhör",
    "aufhoer", "verzicht", "avoid", "stop",
];
const NEGATION_WORDS: &[&str] = &[
    "no", "not", "nor", "never", "neither", "none", "nothing", "without", "instead", "dont",
    "cannot", "hör", "hoer",
];

fn negation_word(word: &str) -> bool {
    NEGATION_WORDS.contains(&word)
        || word.ends_with("n't")
        || NEGATION_STEMS.iter().any(|stem| word.starts_with(stem))
}

fn words(clause: &str) -> Vec<String> {
    clause
        .replace('’', "'")
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect()
}

fn negated(clause: &str) -> bool {
    words(clause).iter().any(|word| negation_word(word))
}

const REMOVAL_STEMS: &[&str] = &[
    "weg", "raus", "heraus", "entfern", "streich", "schneid", "kürz", "kuerz", "lösch", "loesch",
    "remov", "drop", "cut", "trim", "delet", "skip",
];
const KEEP_STEMS: &[&str] = &[
    "behalt",
    "behält",
    "behaelt",
    "keep",
    "drin",
    "komplett",
    "ganz",
    "full",
    "vollständig",
    "vollstaendig",
];

fn has_stem(clause: &str, stems: &[&str]) -> bool {
    words(clause)
        .iter()
        .any(|word| stems.iter().any(|stem| word.starts_with(stem)))
}

fn keep_request(clause: &str, extra: &[&str]) -> bool {
    let wants_keep = has_stem(clause, KEEP_STEMS)
        || words(clause)
            .iter()
            .any(|word| extra.contains(&word.as_str()));
    wants_keep && !has_stem(clause, REMOVAL_STEMS)
}

fn without_keep_phrases(clause: &str) -> Option<String> {
    let mut rest = clause.replace('’', "'");
    let mut found = false;
    for phrase in KEEP_PHRASES {
        if rest.contains(phrase) {
            found = true;
            rest = rest.replace(phrase, " ");
        }
    }
    found.then_some(rest)
}

fn mentions_reaction(clause: &str) -> bool {
    contains_any(clause, &["reaktion", "reaction"])
}

fn kept_reaction(sentence: &str) -> bool {
    if !mentions_reaction(sentence) {
        return false;
    }
    without_keep_phrases(sentence)
        .is_some_and(|rest| !negated(&rest) && !has_stem(&rest, REMOVAL_STEMS))
}

fn affirmative_parts(text: &str, signals: &mut Vec<Signal>) -> Vec<String> {
    let mut parts = Vec::new();
    for sentence in text.split(['.', '!', '?', ';', '\n']) {
        let sentence = sentence.trim();
        if sentence.is_empty() {
            continue;
        }
        if negated(sentence) {
            if kept_reaction(sentence) {
                signals.push(Signal::Toward(Dimension::KeepReaction, 1));
            }
            continue;
        }
        parts.extend(
            sentence
                .split(',')
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .map(str::to_owned),
        );
    }
    parts
}

fn classify(feedback: &str) -> Vec<Signal> {
    let text = feedback.to_lowercase();
    let mut signals = Vec::new();
    for clause in affirmative_parts(&text, &mut signals) {
        let clause = clause.as_str();
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
            && keep_request(clause, &["mehr", "more"])
        {
            signals.push(Signal::Toward(Dimension::KeepAction, 1));
        }
        if mentions_reaction(clause) && keep_request(clause, &[]) {
            signals.push(Signal::Toward(Dimension::KeepReaction, 1));
        }
        signals.extend(facecam_signals(clause));
        if contains_any(clause, FACECAM)
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
            "version_rejected" => {
                map.insert(event.job_id, Outcome::Rejected);
            }
            "proposal_rejected" => {
                map.insert(event.job_id, Outcome::Discarded);
            }
            _ => {}
        }
    }
    map
}

#[derive(Debug, Default)]
struct RootReview {
    accepted: BTreeMap<Dimension, i32>,
    rejected: BTreeMap<Dimension, i32>,
}

impl RootReview {
    fn nets(&mut self, outcome: Outcome) -> Option<&mut BTreeMap<Dimension, i32>> {
        match outcome {
            Outcome::Accepted => Some(&mut self.accepted),
            Outcome::Rejected => Some(&mut self.rejected),
            Outcome::Discarded => None,
        }
    }

    fn rejected_against(&self) -> impl Iterator<Item = (Dimension, i8)> + '_ {
        self.rejected.iter().filter_map(|(dimension, net)| {
            let accepted = self.accepted.get(dimension).is_some_and(|net| *net != 0);
            (!accepted && *net != 0).then_some((*dimension, net.signum() as i8))
        })
    }
}

fn apply_signals(nets: &mut BTreeMap<Dimension, i32>, feedback: &str) {
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

fn root_reviews(events: &[FeedbackEvent]) -> BTreeMap<i64, RootReview> {
    let decided = outcomes(events);
    let mut reviews: BTreeMap<i64, RootReview> = BTreeMap::new();
    for event in events.iter().filter(|event| event.kind == "feedback") {
        let (Some(outcome), Some(feedback)) = (decided.get(&event.job_id), &event.feedback) else {
            continue;
        };
        if let Some(nets) = reviews.entry(event.root_job_id).or_default().nets(*outcome) {
            apply_signals(nets, feedback);
        }
    }
    reviews
}

#[derive(Debug, Default, Clone, Copy)]
struct Tally {
    up: u32,
    down: u32,
    rejected_up: u32,
    rejected_down: u32,
}

impl Tally {
    fn preference(self, dimension: Dimension) -> Option<PersonalPreference> {
        let (direction, supporting, opposing, rejected) = if self.up >= self.down {
            (1, self.up, self.down, self.rejected_up)
        } else {
            (-1, self.down, self.up, self.rejected_down)
        };
        let opposing = opposing + rejected;
        (supporting >= PERSONAL_MIN_REVIEWS && supporting >= 2 * opposing).then_some(
            PersonalPreference {
                dimension,
                direction,
                supporting_reviews: supporting,
                opposing_reviews: opposing,
                rejected_reviews: rejected,
            },
        )
    }
}

fn tally(reviews: &BTreeMap<i64, RootReview>) -> BTreeMap<Dimension, Tally> {
    let mut tally: BTreeMap<Dimension, Tally> = BTreeMap::new();
    for review in reviews.values() {
        for (dimension, net) in &review.accepted {
            let entry = tally.entry(*dimension).or_default();
            match net.signum() {
                1 => entry.up += 1,
                -1 => entry.down += 1,
                _ => {}
            }
        }
        for (dimension, direction) in review.rejected_against() {
            let entry = tally.entry(dimension).or_default();
            if direction > 0 {
                entry.rejected_up += 1;
            } else {
                entry.rejected_down += 1;
            }
        }
    }
    tally
}

pub(super) fn rejected_directions(events: &[FeedbackEvent]) -> BTreeSet<(Dimension, i8)> {
    root_reviews(events)
        .values()
        .flat_map(RootReview::rejected_against)
        .collect()
}

pub fn personal_profile(events: &[FeedbackEvent]) -> PersonalProfile {
    let preferences = tally(&root_reviews(events))
        .into_iter()
        .filter_map(|(dimension, tally)| tally.preference(dimension))
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

fn target_skill(dimension: Dimension) -> Option<SkillId> {
    match dimension {
        Dimension::SubtitleSize => Some(SkillId::Subtitles),
        Dimension::IntroLength
        | Dimension::VideoLength
        | Dimension::EditingPace
        | Dimension::KeepReaction => Some(SkillId::EditingStrategy),
        Dimension::KeepAction => Some(SkillId::HighlightAnalysis),
        Dimension::FacecamOcclusion => Some(SkillId::VideoLayout),
        Dimension::GameUnderstanding => Some(SkillId::DeadlockUnderstanding),
        Dimension::FacecamPosition | Dimension::FacecamSize => None,
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
        (Dimension::FacecamPosition, false) => "The streamer prefers the facecam placed lower. The layout is fixed, so flag the video for review when the facecam placement hurts the result.",
        (Dimension::FacecamPosition, true) => "The streamer prefers the facecam placed higher. The layout is fixed, so flag the video for review when the facecam placement hurts the result.",
        (Dimension::FacecamSize, false) => "The streamer prefers a smaller facecam. The layout is fixed, so flag the video for review when the facecam takes too much space.",
        (Dimension::FacecamSize, true) => "The streamer prefers a larger facecam. The layout is fixed, so flag the video for review when the facecam is hard to see.",
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
        (Dimension::FacecamPosition, false) => "Facecam weiter unten bevorzugt",
        (Dimension::FacecamPosition, true) => "Facecam weiter oben bevorzugt",
        (Dimension::FacecamSize, false) => "Kleinere Facecam bevorzugt",
        (Dimension::FacecamSize, true) => "Größere Facecam bevorzugt",
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
        Dimension::FacecamPosition | Dimension::FacecamSize => Some((
            "Facecam-Position und -Größe je Kanal einstellbar machen",
            "Mehrere Kanäle wünschen wiederholt eine andere Position oder Größe der Facecam. Das stabile Layout lässt das bisher nicht zu; eine Einstellung im Layout-Schritt bräuchte eine Codeänderung.",
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
        "SELECT e.twitch_user_id FROM public.social_ai_revision_events e LEFT JOIN public.social_ai_personal_preferences p ON p.twitch_user_id = e.twitch_user_id GROUP BY e.twitch_user_id, p.last_event_id, p.updated_at HAVING MAX(e.id) > COALESCE(p.last_event_id, 0) ORDER BY p.updated_at ASC NULLS FIRST, MIN(e.id) FILTER (WHERE e.id > COALESCE(p.last_event_id, 0)), e.twitch_user_id LIMIT $1",
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
        "SELECT EXISTS(SELECT 1 FROM public.social_ai_skill_activations a JOIN public.social_ai_skill_versions v ON v.skill_id = a.skill_id AND v.version = a.from_version WHERE a.skill_id = $1 AND a.kind IN ('rollback', 'auto_rollback') AND v.sha256 = $2)",
    )
    .bind(id.as_str())
    .bind(sha256)
    .fetch_one(pool)
    .await
}

fn learned_lines(text: &str) -> BTreeSet<&str> {
    match text.find(&format!("\n{LEARNED_HEADING}")) {
        Some(index) => text[index..]
            .lines()
            .filter_map(|line| line.strip_prefix("- "))
            .collect(),
        None => BTreeSet::new(),
    }
}

async fn vetoed_guidance(pool: &PgPool, id: SkillId) -> Result<BTreeSet<String>, sqlx::Error> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT f.content, t.content FROM public.social_ai_skill_activations a JOIN public.social_ai_skill_versions f ON f.skill_id = a.skill_id AND f.version = a.from_version JOIN public.social_ai_skill_versions t ON t.skill_id = a.skill_id AND t.version = a.to_version WHERE a.skill_id = $1 AND a.kind IN ('rollback', 'auto_rollback')",
    )
    .bind(id.as_str())
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .flat_map(|(from, to)| {
            let kept = learned_lines(to);
            learned_lines(from)
                .into_iter()
                .filter(move |line| !kept.contains(line))
                .map(str::to_owned)
        })
        .collect())
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
    let vetoed = vetoed_guidance(pool, id).await?;
    let rules: Vec<&GlobalRule> = rules
        .iter()
        .copied()
        .filter(|rule| !vetoed.contains(guidance(rule.dimension, rule.direction)))
        .collect();
    let rules = rules.as_slice();
    let candidate = compose_skill_text(&active.text, rules);
    let sha256 = skill_registry::sha256_hex(&candidate);
    if sha256 == active.sha256
        || !guard::valid_structure(id, &active.text, &candidate, max_bytes)
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
    let skills_reverted = guard::revert_regressions(pool).await?;
    let candidates = global_rules(&all_profiles(pool).await?);
    let rejections = guard::recent_rejections(pool).await?;
    let (rules, held_back): (Vec<GlobalRule>, Vec<GlobalRule>) = candidates
        .into_iter()
        .partition(|rule| !guard::regresses(rule, &rejections));
    let learned = learned_skills(pool).await?;
    let mut skills_changed = Vec::new();
    for id in SkillId::ALL {
        let mine: Vec<&GlobalRule> = rules
            .iter()
            .filter(|rule| target_skill(rule.dimension) == Some(id))
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
        rules_held_back: held_back.len(),
        skills_reverted,
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

#[path = "learning_guard.rs"]
mod guard;

#[cfg(test)]
#[path = "learning_tests.rs"]
mod tests;
