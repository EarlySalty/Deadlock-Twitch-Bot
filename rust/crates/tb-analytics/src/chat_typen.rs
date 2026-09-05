use std::collections::HashSet;
use std::sync::OnceLock;
use std::time::Duration;

use regex::Regex;
use serde::Deserialize;

use crate::chat_analytics_lexicon::{
    FEEDBACK, GAME, GREETING, HYPE, QUESTION, REACTION, SOCIAL, TECHNICAL,
};

pub const BOT_LOGINS: &[&str] = &[
    "botrix",
    "deutschedeadlockcommunity",
    "fossabot",
    "moobot",
    "nightbot",
    "pretzelrocks",
    "soundalerts",
    "streamlabs",
    "streamelements",
    "wizebot",
];

pub const GAME_EXTRA: &[&str] = &[
    "initiate",
    "seeker",
    "alchemist",
    "arcanist",
    "ritualist",
    "emissary",
    "archon",
    "oracle",
    "phantom",
    "ascendant",
    "eternus",
    "souls",
    "patch",
    "meta",
    "buff",
    "nerf",
    "matchmaking",
    "ranked",
    "carry",
    "gecarryt",
    "sniper",
    "velocity",
    "ability",
    "gank",
    "farm",
    "jungle",
    "midboss",
    "base",
    "spawn",
    "calico",
    "holliday",
    "vyper",
    "sinclair",
    "mina",
    "drifter",
    "victor",
    "paige",
    "doorman",
    "fathom",
    "magician",
    "trapper",
    "raven",
    "wrecker",
    "bookworm",
    "frank",
    "paradox",
];

const KNOWN_EMOTES: &[&str] = &[
    "lul", "lulw", "kekw", "omegalul", "kappa", "monkas", "sadge", "copium", "deadge", "pepega",
    "pepehands", "widepeepohappy", "catjam", "peepohappy",
];

const SHORT_WORDS: &[&str] = &[
    "jo", "yes", "rip", "woa", "ja", "ne", "nein", "ok", "lol", "xd",
];

const QUESTION_WORDS: &[&str] = &[
    "was", "wo", "wer", "wie", "wann", "why", "how", "warum", "weshalb",
];

const STATEMENT_MIN_WOERTER: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nachrichtentyp {
    Command,
    Hype,
    Greeting,
    Question,
    Feedback,
    Technical,
    Social,
    Reaction,
    GameRelated,
    Statement,
    Other,
    System,
}

impl Nachrichtentyp {
    pub fn api_key(self) -> &'static str {
        match self {
            Self::Command => "Command",
            Self::Hype => "Hype",
            Self::Greeting => "Greeting",
            Self::Question => "Question",
            Self::Feedback => "Feedback",
            Self::Technical => "Technical",
            Self::Social => "Social",
            Self::Reaction => "Reaction",
            Self::GameRelated => "Game-Related",
            Self::Statement => "Statement",
            Self::Other => "Other",
            Self::System => "System",
        }
    }

    pub fn from_api_key(key: &str) -> Option<Self> {
        Some(match key {
            "Command" => Self::Command,
            "Hype" => Self::Hype,
            "Greeting" => Self::Greeting,
            "Question" => Self::Question,
            "Feedback" => Self::Feedback,
            "Technical" => Self::Technical,
            "Social" => Self::Social,
            "Reaction" => Self::Reaction,
            "Game-Related" => Self::GameRelated,
            "Statement" => Self::Statement,
            "Other" => Self::Other,
            "System" => Self::System,
            _ => return None,
        })
    }
}

fn redeemed_bits_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"redeemed .* for \d+ bits").expect("gueltiges Muster"))
}

fn clean_token(tok: &str) -> String {
    tok.chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>()
        .to_lowercase()
}

fn ist_emoji_char(c: char) -> bool {
    let u = c as u32;
    (0x1F000..=0x1FAFF).contains(&u)
        || (0x1F300..=0x1F5FF).contains(&u)
        || (0x1F600..=0x1F64F).contains(&u)
        || (0x1F680..=0x1F6FF).contains(&u)
        || (0x1F900..=0x1F9FF).contains(&u)
        || (0x2600..=0x27BF).contains(&u)
        || (0x2B00..=0x2BFF).contains(&u)
        || (0xFE00..=0xFE0F).contains(&u)
        || u == 0x200D
        || u == 0x2764
}

fn ist_emoji(tok: &str) -> bool {
    !tok.is_empty() && tok.chars().all(ist_emoji_char)
}

fn ist_emote(tok: &str) -> bool {
    if tok.is_empty() {
        return false;
    }
    if ist_emoji(tok) {
        return true;
    }
    let clean: String = tok.chars().filter(|c| c.is_alphanumeric()).collect();
    if clean.is_empty() {
        return false;
    }
    if KNOWN_EMOTES.contains(&clean.to_lowercase().as_str()) {
        return true;
    }
    let mut seen_lower = false;
    for c in clean.chars() {
        if c.is_lowercase() {
            seen_lower = true;
        } else if c.is_uppercase() && seen_lower {
            return true;
        }
    }
    false
}

fn matches_list(list: &[&str], content_lower: &str, word_set: &HashSet<String>) -> bool {
    list.iter().any(|w| {
        if w.contains(' ') {
            content_lower.contains(w)
        } else {
            word_set.contains(*w)
        }
    })
}

pub fn klassifiziere_regel(content: &str, chatter_login: &str) -> Nachrichtentyp {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Nachrichtentyp::Other;
    }

    let content_lower = content.to_lowercase();
    let login = chatter_login.trim().to_lowercase();
    if !login.is_empty() && BOT_LOGINS.contains(&login.as_str()) {
        return Nachrichtentyp::System;
    }
    if redeemed_bits_re().is_match(&content_lower)
        || content_lower.contains("truhe")
        || content_lower.contains("abofalle")
        || (content_lower.contains("!loot") && content_lower.contains("!dodge"))
    {
        return Nachrichtentyp::System;
    }

    if trimmed.starts_with('!') {
        return Nachrichtentyp::Command;
    }

    let toks: Vec<&str> = content.split_whitespace().collect();
    let mut reaktiv = 0usize;
    let mut woerter = 0usize;
    let mut mentions = 0usize;
    let mut satz_woerter = 0usize;
    let mut word_set: HashSet<String> = HashSet::new();
    let mut first_word: Option<String> = None;
    for tok in &toks {
        if tok.starts_with('@') {
            mentions += 1;
            continue;
        }
        let clean = clean_token(tok);
        if clean.is_empty() {
            if ist_emoji(tok) {
                reaktiv += 1;
            }
            continue;
        }
        if first_word.is_none() {
            first_word = Some(clean.clone());
        }
        if clean.chars().all(|c| c.is_alphabetic()) && clean.chars().count() >= 2 {
            satz_woerter += 1;
        }
        if ist_emote(tok) || SHORT_WORDS.contains(&clean.as_str()) {
            reaktiv += 1;
        } else {
            woerter += 1;
        }
        word_set.insert(clean);
    }

    if mentions == 0 && reaktiv >= 1 && reaktiv >= woerter {
        return Nachrichtentyp::Reaction;
    }

    if mentions >= 1 && (reaktiv + woerter) <= 2 {
        return Nachrichtentyp::Social;
    }

    if matches_list(HYPE, &content_lower, &word_set) {
        return Nachrichtentyp::Hype;
    }
    if matches_list(GREETING, &content_lower, &word_set) {
        return Nachrichtentyp::Greeting;
    }
    let question_phrase = QUESTION
        .iter()
        .filter(|w| w.contains(' '))
        .any(|w| content_lower.contains(w));
    let question_start = first_word
        .as_deref()
        .map(|fw| QUESTION_WORDS.contains(&fw))
        .unwrap_or(false);
    if content.contains('?') || question_start || question_phrase {
        return Nachrichtentyp::Question;
    }
    if matches_list(FEEDBACK, &content_lower, &word_set) {
        return Nachrichtentyp::Feedback;
    }
    if matches_list(TECHNICAL, &content_lower, &word_set) {
        return Nachrichtentyp::Technical;
    }
    if matches_list(SOCIAL, &content_lower, &word_set) {
        return Nachrichtentyp::Social;
    }
    if matches_list(REACTION, &content_lower, &word_set) {
        return Nachrichtentyp::Reaction;
    }
    if matches_list(GAME, &content_lower, &word_set)
        || matches_list(GAME_EXTRA, &content_lower, &word_set)
    {
        return Nachrichtentyp::GameRelated;
    }

    if satz_woerter >= STATEMENT_MIN_WOERTER {
        return Nachrichtentyp::Statement;
    }
    Nachrichtentyp::Other
}

#[derive(Debug, Deserialize)]
struct ModellAntwort {
    #[serde(default)]
    labels: Vec<ModellLabel>,
}

#[derive(Debug, Deserialize)]
struct ModellLabel {
    i: usize,
    t: String,
}

const MODELL_USE_CASE: &str = "chat_message_type";
const MODELL_TIMEOUT_SECS: u64 = 60;

fn modell_prompt(pakete: &[(i64, String)]) -> String {
    let mut zeilen = String::new();
    for (idx, (_id, content)) in pakete.iter().enumerate() {
        let sauber = content.replace(['\n', '\r'], " ");
        zeilen.push_str(&format!("{idx}: {sauber}\n"));
    }
    format!(
        "Ordne jede Chat-Nachricht genau einem Typ zu. Erlaubte Typen und ihre Bedeutung:\n\
         Command: ein Chatbefehl.\n\
         Hype: Begeisterung, Jubel, Feiern eines Moments.\n\
         Greeting: Begruessung oder Verabschiedung.\n\
         Question: eine echte Frage an den Streamer oder Chat.\n\
         Feedback: Bewertung oder Meinung zu Stream, Spiel oder Person.\n\
         Technical: Hinweis auf Ton, Bild, Lag oder Technik.\n\
         Social: Interaktion mit anderen, Anrede, Dank, Community.\n\
         Reaction: kurze Reaktion, Lachen, Emotes.\n\
         Game-Related: Bezug zum Spiel Deadlock, Helden, Raenge, Items, Matchmaking.\n\
         Statement: eine allgemeine Aussage oder ein Kommentar ohne obigen Bezug.\n\
         Other: nicht einzuordnen oder Spam.\n\
         Antworte ausschliesslich als JSON: {{\"labels\":[{{\"i\":0,\"t\":\"Question\"}}]}} mit einem Eintrag je Nachricht.\n\n\
         Nachrichten:\n{zeilen}"
    )
}

pub async fn klassifiziere_modell(
    pakete: &[(i64, String)],
) -> Result<(Vec<(i64, Nachrichtentyp)>, String), tb_llm::LlmError> {
    klassifiziere_modell_intern(pakete, None).await
}

async fn klassifiziere_modell_intern(
    pakete: &[(i64, String)],
    endpoint: Option<tb_llm::LlmEndpoint>,
) -> Result<(Vec<(i64, Nachrichtentyp)>, String), tb_llm::LlmError> {
    if pakete.is_empty() {
        return Ok((Vec::new(), String::new()));
    }
    let mut request = tb_llm::Request::prompt(modell_prompt(pakete))
        .temperature(0.0)
        .max_tokens((pakete.len() as i64) * 24 + 256)
        .timeout(Duration::from_secs(MODELL_TIMEOUT_SECS))
        .json_object();
    if let Some(endpoint) = endpoint {
        request = request.no_ledger().endpoint(endpoint);
    }
    let response = tb_llm::complete(MODELL_USE_CASE, request).await?;
    let antwort: ModellAntwort = serde_json::from_str(response.text.trim()).map_err(|error| {
        tb_llm::LlmError::Unparsable(format!("JSON nicht lesbar: {error}"))
    })?;
    let mut ergebnis: Vec<(i64, Nachrichtentyp)> = pakete
        .iter()
        .map(|(id, _)| (*id, Nachrichtentyp::Other))
        .collect();
    for label in antwort.labels {
        if let Some((id, _)) = pakete.get(label.i) {
            let typ = Nachrichtentyp::from_api_key(&label.t).unwrap_or(Nachrichtentyp::Other);
            ergebnis[label.i] = (*id, typ);
        }
    }
    Ok((ergebnis, response.model))
}

pub async fn lade_unlabelte(
    pool: &sqlx::PgPool,
    limit: i64,
) -> Result<Vec<(i64, String, String)>, sqlx::Error> {
    let rows: Vec<(i64, String, String)> = sqlx::query_as(
        r#"
        SELECT cm.id,
               COALESCE(cm.content, '') AS content,
               COALESCE(cm.chatter_login, '') AS chatter_login
        FROM twitch_chat_messages cm
        LEFT JOIN twitch_chat_message_labels l ON l.message_id = cm.id
        WHERE l.message_id IS NULL
          AND cm.message_ts < now() - interval '5 minutes'
        ORDER BY cm.message_ts DESC
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn speichere_labels(
    pool: &sqlx::PgPool,
    rows: &[(i64, Nachrichtentyp, &str, Option<String>)],
) -> Result<u64, sqlx::Error> {
    if rows.is_empty() {
        return Ok(0);
    }
    let ids: Vec<i64> = rows.iter().map(|(id, _, _, _)| *id).collect();
    let labels: Vec<String> = rows
        .iter()
        .map(|(_, typ, _, _)| typ.api_key().to_string())
        .collect();
    let quellen: Vec<String> = rows.iter().map(|(_, _, q, _)| (*q).to_string()).collect();
    let modelle: Vec<Option<String>> = rows.iter().map(|(_, _, _, m)| m.clone()).collect();
    let result = sqlx::query(
        r#"
        INSERT INTO twitch_chat_message_labels (message_id, label, quelle, modell)
        SELECT * FROM UNNEST($1::bigint[], $2::text[], $3::text[], $4::text[])
        ON CONFLICT (message_id) DO NOTHING
        "#,
    )
    .bind(&ids)
    .bind(&labels)
    .bind(&quellen)
    .bind(&modelle)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn regel(content: &str) -> &'static str {
        klassifiziere_regel(content, "").api_key()
    }

    #[test]
    fn api_schluessel_sind_rundreise_stabil() {
        for typ in [
            Nachrichtentyp::Command,
            Nachrichtentyp::Hype,
            Nachrichtentyp::Greeting,
            Nachrichtentyp::Question,
            Nachrichtentyp::Feedback,
            Nachrichtentyp::Technical,
            Nachrichtentyp::Social,
            Nachrichtentyp::Reaction,
            Nachrichtentyp::GameRelated,
            Nachrichtentyp::Statement,
            Nachrichtentyp::Other,
            Nachrichtentyp::System,
        ] {
            assert_eq!(Nachrichtentyp::from_api_key(typ.api_key()), Some(typ));
        }
    }

    #[test]
    fn alte_beispiele_bleiben_stabil() {
        assert_eq!(regel(""), "Other");
        assert_eq!(regel("!uptime"), "Command");
        assert_eq!(regel("POG das war insane"), "Hype");
        assert_eq!(regel("moin"), "Greeting");
        assert_eq!(regel("warum lagt das?"), "Question");
        assert_eq!(regel("wie geht es dir"), "Question");
        assert_eq!(regel("nice play"), "Feedback");
        assert_eq!(regel("lag und fps drops"), "Technical");
        assert_eq!(regel("danke fuers following"), "Social");
        assert_eq!(regel("lol haha"), "Reaction");
        assert_eq!(regel("haze build ist gut"), "Game-Related");
        assert_eq!(regel("zzz"), "Other");
        assert_eq!(regel("!pog"), "Command");
    }

    #[test]
    fn evidence_reaction() {
        assert_eq!(regel("LUL LUL LUL LUL LUL"), "Reaction");
        assert_eq!(regel("KappaPride"), "Reaction");
        assert_eq!(
            regel("NotLikeThis NotLikeThis NotLikeThis  ich sag ja lexus"),
            "Reaction"
        );
        assert_eq!(regel("jo"), "Reaction");
        assert_eq!(regel("yes"), "Reaction");
        assert_eq!(regel("rip"), "Reaction");
        assert_eq!(regel("woa"), "Reaction");
    }

    #[test]
    fn evidence_social() {
        assert_eq!(regel("@Zenkay123 IAmClap missmo107FLEISCHWURST"), "Social");
    }

    #[test]
    fn evidence_system() {
        assert_eq!(regel("Plorki_GER redeemed Confetti (duo) for 0 Bits"), "System");
        assert_eq!(
            regel("🔧 Eisen-Truhe! !loot (riskant) · !prüfen (vorsichtig) · !dodge (sicher) ... 45s"),
            "System"
        );
        assert_eq!(regel("Brain_BP Abofalle schlägt zu! Vielen Dank HappyPag"), "System");
        assert_eq!(klassifiziere_regel("hallo zusammen", "nightbot").api_key(), "System");
    }

    #[test]
    fn evidence_game() {
        assert_eq!(regel("phantom 4 gerade"), "Game-Related");
        assert_eq!(regel("du bist phantom 5 und nicht top 1000 ingame"), "Game-Related");
        assert_eq!(regel("Emmi 1 angefangen solo hoch gecarryt auf Emmi 6"), "Game-Related");
        assert_eq!(regel("Rigged matchmaking"), "Game-Related");
        assert_eq!(
            regel("ich versteh auch nicht wie niedrig die velocity von der sniper ist"),
            "Game-Related"
        );
        assert_eq!(regel("Graves Doorman hätten nie ins Game kommen sollen"), "Game-Related");
    }

    #[test]
    fn evidence_statement_und_other() {
        assert_eq!(
            regel("ist schon echt gut muss ich sagen und sieht qualitativ auch echt ok aus ngl"),
            "Statement"
        );
        assert_eq!(regel("das ist der reiz.. arc raider war mir zu viel mimimi"), "Statement");
        assert_eq!(regel("Astronomie meint er übrigens, nicht Astrologie"), "Statement");
        assert_eq!(regel("Ai viewers streamboo . Com"), "Other");
    }

    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn mock_endpoint(server: &MockServer) -> tb_llm::LlmEndpoint {
        tb_llm::LlmEndpoint {
            provider: "fireworks",
            base_url: server.uri(),
            model: "accounts/fireworks/models/deepseek-v4-flash-0731".to_string(),
            api_key: Some("k".to_string()),
        }
    }

    #[tokio::test]
    async fn modell_bekommt_nur_other_und_wird_genau_einmal_gerufen() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "model": "accounts/fireworks/models/deepseek-v4-flash-0731",
                "choices": [{"message": {"content": "{\"labels\":[{\"i\":0,\"t\":\"Statement\"},{\"i\":1,\"t\":\"Unbekannt\"}]}"}}],
                "usage": {"prompt_tokens": 10, "completion_tokens": 4}
            })))
            .mount(&server)
            .await;

        let alle: Vec<(i64, String)> = vec![
            (10, "moin".to_string()),
            (11, "zzz".to_string()),
            (12, "!uptime".to_string()),
            (13, "Ai viewers streamboo . Com".to_string()),
        ];
        let others: Vec<(i64, String)> = alle
            .iter()
            .filter(|(_, c)| klassifiziere_regel(c, "") == Nachrichtentyp::Other)
            .cloned()
            .collect();
        assert_eq!(others.iter().map(|(id, _)| *id).collect::<Vec<_>>(), vec![11, 13]);

        let (ergebnis, modell) = klassifiziere_modell_intern(&others, Some(mock_endpoint(&server)))
            .await
            .expect("Modellantwort");

        assert_eq!(modell, "accounts/fireworks/models/deepseek-v4-flash-0731");
        assert_eq!(ergebnis[0], (11, Nachrichtentyp::Statement));
        assert_eq!(ergebnis[1], (13, Nachrichtentyp::Other));

        let requests = server.received_requests().await.expect("Requests");
        assert_eq!(requests.len(), 1);
        let body = String::from_utf8(requests[0].body.clone()).expect("utf8");
        assert!(body.contains("zzz"));
        assert!(body.contains("streamboo"));
        assert!(!body.contains("moin"));
        assert!(!body.contains("uptime"));
    }

    #[tokio::test]
    async fn leeres_paket_ruft_das_modell_nicht() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        let (ergebnis, _) = klassifiziere_modell_intern(&[], Some(mock_endpoint(&server)))
            .await
            .expect("leer");
        assert!(ergebnis.is_empty());
        assert!(server.received_requests().await.expect("req").is_empty());
    }
}
