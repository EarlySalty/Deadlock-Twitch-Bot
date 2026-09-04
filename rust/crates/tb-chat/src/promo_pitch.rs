use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub const USE_CASE: &str = "promo_pitch";

const PITCH_TIMEOUT: Duration = Duration::from_secs(20);
const PITCH_MAX_CHARS: usize = 400;

pub const PITCH_SYSTEM_PROMPT: &str = r#"Du bist im Twitch-Chat eines deutschen Deadlock-Streamers, der Partner der Deutschen Deadlock Community ist. Ein Zuschauer hat gerade etwas geschrieben. Prüfe, ob die Nachricht einen echten Anlass trifft, bei dem die Community zu der Person passt.

Diese Anlässe zählen:
no_mates: der Person fehlen Leute zum Zocken, Freunde sind nicht dabei oder nicht überzeugt.
game_unpopular: die Person findet das Spiel zu klein, unbekannt oder am Sterben.
too_tryhard: die Person findet das Spiel zu tryhard oder zu sweaty.
solo_queue: die Person ärgert sich über Solo Queue.
new_player: die Person ist neu in Deadlock oder unsicher.
wants_help: die Person sucht Hilfe, Tipps oder Coaching.

Passt keiner dieser Anlässe, setzt du occasion auf null und lässt reply leer.

Passt ein Anlass, schreibst du eine Antwort in zwei Teilen und genau dieser Reihenfolge:
1. Geh zuerst echt auf das ein, was die Person gesagt hat. Kurz, ehrlich, auf Augenhöhe.
2. Danach höchstens ein Satz zur Community in dritter Person, passend zum Anlass. Kein Aufruf, keine Einladung.

So schreibst du:
Deutsch, kurz, locker. Kleinschreibung ist normal. Emojis benutzt du nicht, höchstens :) Keine Ausrufezeichen-Werbung, keine Superlative, keine Mitgliederzahlen. Du sagst nie, dass wir die größte oder beste Community sind. Du benutzt keine Gedankenstriche. Du schickst keinen Link und forderst niemanden auf, irgendwo beizutreten. Kein komm auf, kein join, kein tritt bei.

Antworte ausschließlich mit diesem JSON:
{"occasion": null oder einer der sechs Anlässe, "reply": "deine Antwort oder leer", "confidence": 0.0}"#;

pub const CHANNEL_PROMO_SYSTEM_PROMPT: &str = r#"Du schreibst eine kurze Einladung in den Twitch-Chat eines deutschen Deadlock-Streamers, der Partner der Deutschen Deadlock Community ist. Der Einladungslink wird automatisch ans Ende gehängt, du schreibst ihn nicht selbst.

Schreib einen einzigen kurzen Satz, der zur Community einlädt und zum aktuellen Moment im Stream passt (Spiel, Titel, Chat). Locker, deutsch, Kleinschreibung ist normal. Keine Ausrufezeichen-Werbung, keine Superlative, keine Mitgliederzahlen, keine Gedankenstriche. Kein komm auf, kein join, kein tritt bei. Nenne keinen Link.

Antworte nur mit dem Satz, ohne Anführungszeichen."#;

pub const TARGETED_PITCH_SYSTEM_PROMPT: &str = r#"Du schreibst eine kurze, persönliche Nachricht an einen Zuschauer im Twitch-Chat eines deutschen Deadlock-Streamers, der Partner der Deutschen Deadlock Community ist. Geh auf das ein, was die Person zuletzt geschrieben hat, und erwähne die Community passend in dritter Person. Kein Link, keine Einladung zum Beitreten.

Locker, deutsch, kurz, Kleinschreibung ist normal. Keine Ausrufezeichen-Werbung, keine Superlative, keine Mitgliederzahlen, keine Gedankenstriche. Kein komm auf, kein join, kein tritt bei.

Antworte nur mit der Nachricht, ohne Anführungszeichen."#;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PitchOccasion {
    NoMates,
    GameUnpopular,
    TooTryhard,
    SoloQueue,
    NewPlayer,
    WantsHelp,
}

impl PitchOccasion {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoMates => "no_mates",
            Self::GameUnpopular => "game_unpopular",
            Self::TooTryhard => "too_tryhard",
            Self::SoloQueue => "solo_queue",
            Self::NewPlayer => "new_player",
            Self::WantsHelp => "wants_help",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PitchResponse {
    #[serde(default)]
    pub occasion: Option<PitchOccasion>,
    #[serde(default)]
    pub reply: String,
    #[serde(default)]
    pub confidence: f32,
}

pub fn parse_pitch_response(raw: &str) -> Option<PitchResponse> {
    let trimmed = raw.trim();
    if let Ok(parsed) = serde_json::from_str::<PitchResponse>(trimmed) {
        return Some(parsed);
    }
    let object = extract_json_object(raw)?;
    serde_json::from_str::<PitchResponse>(object).ok()
}

fn extract_json_object(raw: &str) -> Option<&str> {
    let bytes = raw.as_bytes();
    for start in raw.match_indices('{').map(|(index, _)| index) {
        let mut depth = 0usize;
        let mut in_string = false;
        let mut escaped = false;
        for (offset, byte) in bytes[start..].iter().enumerate() {
            if in_string {
                if escaped {
                    escaped = false;
                } else if *byte == b'\\' {
                    escaped = true;
                } else if *byte == b'"' {
                    in_string = false;
                }
                continue;
            }
            match *byte {
                b'"' => in_string = true,
                b'{' => depth += 1,
                b'}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return raw.get(start..=start + offset);
                    }
                }
                _ => {}
            }
        }
    }
    None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PitchRejectReason {
    Link,
    MemberCount,
    Superlative,
    Dash,
    Emoji,
    TooLong,
    JoinPhrase,
}

impl PitchRejectReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Link => "link",
            Self::MemberCount => "member_count",
            Self::Superlative => "superlative",
            Self::Dash => "dash",
            Self::Emoji => "emoji",
            Self::TooLong => "too_long",
            Self::JoinPhrase => "join_phrase",
        }
    }
}

pub fn pitch_filter_reject(text: &str) -> Option<PitchRejectReason> {
    filter_reject_inner(text, false)
}

fn filter_reject_inner(text: &str, allow_link: bool) -> Option<PitchRejectReason> {
    let lower = text.to_lowercase();
    if !allow_link && contains_link(&lower) {
        return Some(PitchRejectReason::Link);
    }
    if contains_member_count(&lower) {
        return Some(PitchRejectReason::MemberCount);
    }
    if contains_superlative(&lower) {
        return Some(PitchRejectReason::Superlative);
    }
    if contains_hard_dash(text) {
        return Some(PitchRejectReason::Dash);
    }
    if contains_forbidden_emoji(text) {
        return Some(PitchRejectReason::Emoji);
    }
    if text.chars().count() > PITCH_MAX_CHARS {
        return Some(PitchRejectReason::TooLong);
    }
    if contains_join_phrase(&lower) {
        return Some(PitchRejectReason::JoinPhrase);
    }
    None
}

fn contains_link(lower: &str) -> bool {
    [
        "http://",
        "https://",
        "www.",
        "discord.gg",
        ".de/",
        ".com/",
        "twitch.tv",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn contains_member_count(lower: &str) -> bool {
    let words = lower.split_whitespace().collect::<Vec<_>>();
    words.windows(2).any(|pair| {
        let first = pair[0].trim_matches(|ch: char| !ch.is_alphanumeric());
        let second = pair[1].trim_matches(|ch: char| !ch.is_alphanumeric());
        let is_label = |word| {
            matches!(
                word,
                "mitglieder" | "mitgliedern" | "leute" | "member" | "personen"
            )
        };
        let is_count = |word: &str| {
            word.chars().any(|ch| ch.is_ascii_digit())
                || matches!(
                    word,
                    "ein"
                        | "eine"
                        | "einen"
                        | "zwei"
                        | "drei"
                        | "vier"
                        | "fünf"
                        | "sechs"
                        | "sieben"
                        | "acht"
                        | "neun"
                        | "zehn"
                )
                || word.ends_with("hundert")
                || word.ends_with("tausend")
                || word.ends_with("million")
                || word.ends_with("millionen")
        };
        (is_count(first) && is_label(second)) || (is_label(first) && is_count(second))
    })
}

fn contains_superlative(lower: &str) -> bool {
    [
        "größte",
        "grösste",
        "aktivste",
        "beste",
        "stärkste",
        "bekannteste",
        "erfolgreichste",
        "nummer 1",
        "nr. 1",
        "#1",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn contains_hard_dash(text: &str) -> bool {
    text.contains('\u{2014}')
        || text.contains('\u{2013}')
        || text.contains('\u{2015}')
        || text.contains(" -- ")
        || text.contains(" - ")
}

fn contains_forbidden_emoji(text: &str) -> bool {
    let without_smiley = text.replace(":)", "");
    let ascii_lower = without_smiley.to_ascii_lowercase();
    if [
        ":-)", ":d", ":-d", ":p", ":-p", ":(", ":-(", ";)", ";-)", "<3", "^^", "xd", ":o",
    ]
    .iter()
    .any(|needle| ascii_lower.contains(needle))
    {
        return true;
    }
    without_smiley.chars().any(|ch| {
        !ch.is_ascii()
            && !ch.is_alphanumeric()
            && !ch.is_whitespace()
            && !matches!(ch, '„' | '“' | '‚' | '‘' | '…')
    })
}

fn contains_join_phrase(lower: &str) -> bool {
    ["komm auf", "join", "tritt bei"]
        .iter()
        .any(|needle| lower.contains(needle))
}

#[derive(Clone, Debug, Serialize)]
pub struct PitchJudgeInput {
    pub trigger_text: String,
    pub game: Option<String>,
    pub title: Option<String>,
    pub recent_chat: Vec<String>,
    pub target_login: String,
}

#[async_trait]
pub trait PitchJudge: Send + Sync {
    async fn decide(&self, input: PitchJudgeInput) -> Option<PitchResponse>;
}

pub struct FireworksPitchJudge;

#[async_trait]
impl PitchJudge for FireworksPitchJudge {
    async fn decide(&self, input: PitchJudgeInput) -> Option<PitchResponse> {
        let user = serde_json::to_string(&input).ok()?;
        let request = tb_llm::Request::simple(PITCH_SYSTEM_PROMPT, user)
            .temperature(0.0)
            .json_object()
            .timeout(PITCH_TIMEOUT);
        match tb_llm::complete(USE_CASE, request).await {
            Ok(response) => parse_pitch_response(&response.text),
            Err(_) => None,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ChannelPromoContext {
    pub game: Option<String>,
    pub title: Option<String>,
    pub recent_chat: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TargetedPitchContext {
    pub target_login: String,
    pub target_messages: Vec<String>,
    pub game: Option<String>,
    pub title: Option<String>,
    pub recent_chat: Vec<String>,
}

fn clean_model_line(text: &str) -> String {
    text.trim().trim_matches('"').trim().to_string()
}

pub fn finalize_channel_promo(model_text: &str, invite: &str) -> Option<String> {
    let body = clean_model_line(model_text);
    if body.is_empty() {
        return None;
    }
    if filter_reject_inner(&body, true).is_some() {
        return None;
    }
    Some(format!("{body} {invite}"))
}

pub fn finalize_targeted_pitch(model_text: &str) -> Option<String> {
    let body = clean_model_line(model_text);
    if body.is_empty() {
        return None;
    }
    if pitch_filter_reject(&body).is_some() {
        return None;
    }
    Some(body)
}

pub async fn build_channel_promo_text(ctx: &ChannelPromoContext, invite: &str) -> Option<String> {
    let user = serde_json::to_string(ctx).ok()?;
    let request = tb_llm::Request::simple(CHANNEL_PROMO_SYSTEM_PROMPT, user)
        .temperature(0.7)
        .timeout(PITCH_TIMEOUT);
    let response = tb_llm::complete(USE_CASE, request).await.ok()?;
    finalize_channel_promo(&response.text, invite)
}

pub async fn build_targeted_pitch_text(ctx: &TargetedPitchContext) -> Option<String> {
    let user = serde_json::to_string(ctx).ok()?;
    let request = tb_llm::Request::simple(TARGETED_PITCH_SYSTEM_PROMPT, user)
        .temperature(0.7)
        .timeout(PITCH_TIMEOUT);
    let response = tb_llm::complete(USE_CASE, request).await.ok()?;
    finalize_targeted_pitch(&response.text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_liest_anlass_und_reply() {
        let parsed = parse_pitch_response(
            r#"{"occasion":"game_unpopular","reply":"stimmt schon","confidence":0.8}"#,
        )
        .unwrap();
        assert_eq!(parsed.occasion, Some(PitchOccasion::GameUnpopular));
        assert_eq!(parsed.reply, "stimmt schon");
        assert!((parsed.confidence - 0.8).abs() < 0.001);
    }

    #[test]
    fn parser_akzeptiert_occasion_null() {
        let parsed =
            parse_pitch_response(r#"{"occasion":null,"reply":"","confidence":0.0}"#).unwrap();
        assert!(parsed.occasion.is_none());
        assert!(parsed.reply.is_empty());
    }

    #[test]
    fn parser_zieht_objekt_aus_rohtext() {
        let parsed = parse_pitch_response(
            "hier kommt json {\"occasion\":\"solo_queue\",\"reply\":\"kenn ich\",\"confidence\":0.5} ende",
        )
        .unwrap();
        assert_eq!(parsed.occasion, Some(PitchOccasion::SoloQueue));
    }

    #[test]
    fn parser_lehnt_muell_ab() {
        assert!(parse_pitch_response("kein json hier").is_none());
    }

    #[test]
    fn filter_faengt_link() {
        assert_eq!(
            pitch_filter_reject("schau mal auf https://discord.gg/test"),
            Some(PitchRejectReason::Link)
        );
    }

    #[test]
    fn filter_faengt_mitgliederzahl() {
        assert_eq!(
            pitch_filter_reject("wir sind 500 mitglieder"),
            Some(PitchRejectReason::MemberCount)
        );
    }

    #[test]
    fn filter_faengt_superlativ() {
        assert_eq!(
            pitch_filter_reject("die größte community weit und breit"),
            Some(PitchRejectReason::Superlative)
        );
    }

    #[test]
    fn filter_faengt_gedankenstrich() {
        assert_eq!(
            pitch_filter_reject("das spiel ist super \u{2014} wirklich"),
            Some(PitchRejectReason::Dash)
        );
        assert_eq!(
            pitch_filter_reject("das spiel ist super \u{2013} wirklich"),
            Some(PitchRejectReason::Dash)
        );
        assert_eq!(
            pitch_filter_reject("das spiel ist super \u{2015} wirklich"),
            Some(PitchRejectReason::Dash)
        );
        assert_eq!(
            pitch_filter_reject("das spiel ist super - wirklich"),
            Some(PitchRejectReason::Dash)
        );
        assert_eq!(
            pitch_filter_reject("das spiel ist super -- wirklich"),
            Some(PitchRejectReason::Dash)
        );
    }

    #[test]
    fn filter_faengt_emoji() {
        assert_eq!(
            pitch_filter_reject("na wie läuft es 🎮"),
            Some(PitchRejectReason::Emoji)
        );
    }

    #[test]
    fn filter_laesst_smiley_durch() {
        assert!(pitch_filter_reject("kein ding, viel spaß noch :)").is_none());
    }

    #[test]
    fn filter_faengt_zu_langen_text() {
        let long = "a".repeat(PITCH_MAX_CHARS + 1);
        assert_eq!(pitch_filter_reject(&long), Some(PitchRejectReason::TooLong));
    }

    #[test]
    fn filter_grenze_genau_erlaubt() {
        let exact = "a".repeat(PITCH_MAX_CHARS);
        assert!(pitch_filter_reject(&exact).is_none());
    }

    #[test]
    fn filter_faengt_join_wendungen() {
        assert_eq!(
            pitch_filter_reject("komm auf unseren server"),
            Some(PitchRejectReason::JoinPhrase)
        );
        assert_eq!(
            pitch_filter_reject("du kannst gerne join"),
            Some(PitchRejectReason::JoinPhrase)
        );
        assert_eq!(
            pitch_filter_reject("tritt bei wenn du magst"),
            Some(PitchRejectReason::JoinPhrase)
        );
    }

    #[test]
    fn filter_reihenfolge_link_vor_join() {
        assert_eq!(
            pitch_filter_reject("join uns auf https://discord.gg/x"),
            Some(PitchRejectReason::Link)
        );
    }

    #[test]
    fn channel_promo_haengt_invite_ans_ende() {
        let text = finalize_channel_promo("bei uns findest du leute zum zocken", "INVITE").unwrap();
        assert!(text.ends_with("INVITE"));
        assert!(text.starts_with("bei uns"));
    }

    #[test]
    fn channel_promo_erlaubt_kein_join_wort() {
        assert!(finalize_channel_promo("komm auf unseren discord", "INVITE").is_none());
    }

    #[test]
    fn targeted_pitch_ohne_link_bleibt() {
        let text = finalize_targeted_pitch("hey, bei uns findest du mitspieler").unwrap();
        assert_eq!(text, "hey, bei uns findest du mitspieler");
    }

    #[test]
    fn targeted_pitch_mit_link_faellt_weg() {
        assert!(finalize_targeted_pitch("schau auf https://discord.gg/x").is_none());
    }
}
