//! Entscheidungskern und Persistenz der Streamer-Scout-Pitch-Pipeline.

use sqlx::PgPool;

pub const JUDGE_SYSTEM_PROMPT: &str = "Du beobachtest einen deutschen Twitch-Chat eines Deadlock-Streamers. Prüfe NUR, ob der STREAMER selbst gerade eines dieser Probleme äußert: (1) Ärger über Spam- oder Scam-Bots im Chat, (2) Frust dass ihn niemand raidet oder er keine Zuschauer bekommt, (3) er sucht Mitspieler oder eine Community. Antworte als JSON {\"trigger\": \"spam_bots\"|\"no_raids\"|\"lfg\"|\"none\", \"confidence\": 0.0-1.0, \"quote\": \"die auslösende Zeile\"}. Chatter-Aussagen zählen nicht, nur der Streamer (sein Login ist bekannt). Im Zweifel none.";

pub const PITCH_SYSTEM_PROMPT: &str = "Du schreibst 1 bis 3 kurze Twitch-Chat-Nachrichten, die EarlySalty (Entwickler der Deutschen Deadlock Community) selbst in diesem fremden Chat senden würde. Sein Stil, zwingend: kurz (unter 120 Zeichen pro Nachricht), lockeres Chat-Deutsch, keine Emojis, keine Ausrufezeichen, keine Gedankenstriche, kleine Tippfehler sind okay. Inhaltlich-Regeln, zwingend: NIE einen Link schreiben. Erst Nutzen als Mechanismus erklären, dann höchstens ein Angebot wie 'wenn du willst schick ich dir den link'. Nie Feature-Listen, nie Marketing-Sprache. Website heißt deutsche-deadlock-community.de/twitch (nur erwähnen, nicht verlinken), Discord nur optional. Echte Beispiele seines Stils: 'Aber wenn du generell mehr DL zockst auf Discord gibts ne Deutsche Deadlock Community. Die bieten auch so ne Streamer Partnerschaft hat einige sehr geile vorteile' / 'wenn du willst ich kann dir nen link schicken' / 'mit den Leuten in Discord zu Zocken weil die kommen dann hin und wieder in den Stream und so haste mehr Zuschauer' / 'Also wenn du offline gehst 10 15 sekunden warten dann wirst du in OBS sehen das der Bot einen Channel Raiden will da einfach bestätigen drücken' / 'Falls der bot keinen Raidet gibts keinen der DL streamt auf Deutsch' / 'Hast du eigentlich ne idee was würdest du dir wünschen von einem Twitch Bot an funktionen' / 'und mein Bot bannt das so gut wie Instant'. Antworte als JSON {\"messages\": [\"...\", ...]}.";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TriggerType {
    ProblemMoment,
    SpamBots,
    NoRaids,
    Lfg,
    OfflineMoment,
    NewStreamer,
}

impl TriggerType {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ProblemMoment => "problem_moment",
            Self::SpamBots => "spam_bots",
            Self::NoRaids => "no_raids",
            Self::Lfg => "lfg",
            Self::OfflineMoment => "offline_moment",
            Self::NewStreamer => "new_streamer",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::ProblemMoment => "Problem-Moment",
            Self::SpamBots => "Ärger über Spam-Bots",
            Self::NoRaids => "bekommt keine Raids",
            Self::Lfg => "sucht Mitspieler",
            Self::OfflineMoment => "offline ohne Raid-Ziel",
            Self::NewStreamer => "neuer deutscher DL-Streamer",
        }
    }

    pub const fn requires_pitch(self) -> bool {
        !matches!(self, Self::NewStreamer)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatLine {
    pub chatter: String,
    pub text: String,
}

impl ChatLine {
    pub fn new(chatter: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            chatter: chatter.into(),
            text: text.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct JudgeVerdict {
    pub trigger_type: Option<TriggerType>,
    pub confidence: f32,
    pub quote: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum JudgeState {
    NotNeeded,
    None,
    Triggered { confidence: f32 },
    Error,
    Timeout,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DecisionInput {
    pub trigger_type: TriggerType,
    pub blacklisted: bool,
    pub cooldown_active: bool,
    pub posted_for_stream: bool,
    pub judge: JudgeState,
    pub sanitized_message_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Post,
    Record(LedgerAction),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedgerAction {
    Posted,
    SuppressedCooldown,
    SuppressedPerStreamLimit,
    SuppressedBlacklist,
    SuppressedLowConfidence,
    SuppressedSanitizer,
    JudgeNone,
    JudgeError,
    JudgeTimeout,
    DiscordError,
}

impl LedgerAction {
    pub const ALL: [Self; 10] = [
        Self::Posted,
        Self::SuppressedCooldown,
        Self::SuppressedPerStreamLimit,
        Self::SuppressedBlacklist,
        Self::SuppressedLowConfidence,
        Self::SuppressedSanitizer,
        Self::JudgeNone,
        Self::JudgeError,
        Self::JudgeTimeout,
        Self::DiscordError,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Posted => "posted",
            Self::SuppressedCooldown => "suppressed_cooldown",
            Self::SuppressedPerStreamLimit => "suppressed_per_stream_limit",
            Self::SuppressedBlacklist => "suppressed_blacklist",
            Self::SuppressedLowConfidence => "suppressed_low_confidence",
            Self::SuppressedSanitizer => "suppressed_sanitizer",
            Self::JudgeNone => "judge_none",
            Self::JudgeError => "judge_error",
            Self::JudgeTimeout => "judge_timeout",
            Self::DiscordError => "discord_error",
        }
    }
}

pub fn decide(input: &DecisionInput) -> Decision {
    if input.blacklisted {
        return Decision::Record(LedgerAction::SuppressedBlacklist);
    }
    if input.posted_for_stream {
        return Decision::Record(LedgerAction::SuppressedPerStreamLimit);
    }
    if input.cooldown_active {
        return Decision::Record(LedgerAction::SuppressedCooldown);
    }
    match input.judge {
        JudgeState::Error => return Decision::Record(LedgerAction::JudgeError),
        JudgeState::Timeout => return Decision::Record(LedgerAction::JudgeTimeout),
        JudgeState::None => return Decision::Record(LedgerAction::JudgeNone),
        JudgeState::Triggered { confidence } if confidence < 0.7 => {
            return Decision::Record(LedgerAction::SuppressedLowConfidence);
        }
        JudgeState::Triggered { .. } | JudgeState::NotNeeded => {}
    }
    if input.trigger_type.requires_pitch() && input.sanitized_message_count == 0 {
        return Decision::Record(LedgerAction::SuppressedSanitizer);
    }
    Decision::Post
}

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("ungueltiges JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("ungueltiger Scout-Payload: {0}")]
    Invalid(&'static str),
}

pub fn parse_judge_json(raw: &str) -> Result<JudgeVerdict, ParseError> {
    let value: serde_json::Value = serde_json::from_str(json_body(raw))?;
    let trigger = value
        .get("trigger")
        .and_then(serde_json::Value::as_str)
        .ok_or(ParseError::Invalid("trigger fehlt"))?;
    let trigger_type = match trigger {
        "spam_bots" => Some(TriggerType::SpamBots),
        "no_raids" => Some(TriggerType::NoRaids),
        "lfg" => Some(TriggerType::Lfg),
        "none" => None,
        _ => return Err(ParseError::Invalid("trigger unbekannt")),
    };
    let confidence = value
        .get("confidence")
        .and_then(serde_json::Value::as_f64)
        .ok_or(ParseError::Invalid("confidence fehlt"))?;
    if !(0.0..=1.0).contains(&confidence) {
        return Err(ParseError::Invalid("confidence ausserhalb 0..=1"));
    }
    let quote = value
        .get("quote")
        .and_then(serde_json::Value::as_str)
        .ok_or(ParseError::Invalid("quote fehlt"))?
        .trim()
        .to_string();
    Ok(JudgeVerdict {
        trigger_type,
        confidence: confidence as f32,
        quote,
    })
}

pub fn parse_pitch_json(raw: &str) -> Result<Vec<String>, ParseError> {
    let value: serde_json::Value = serde_json::from_str(json_body(raw))?;
    let messages = value
        .get("messages")
        .and_then(serde_json::Value::as_array)
        .ok_or(ParseError::Invalid("messages fehlt"))?;
    messages
        .iter()
        .map(|message| {
            message
                .as_str()
                .map(str::to_string)
                .ok_or(ParseError::Invalid("message ist kein String"))
        })
        .collect()
}

fn json_body(raw: &str) -> &str {
    let trimmed = raw.trim();
    let without_open = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .unwrap_or(trimmed)
        .trim();
    without_open
        .strip_suffix("```")
        .unwrap_or(without_open)
        .trim()
}

#[derive(Debug, Clone)]
pub struct LedgerEntry {
    pub streamer_login: String,
    pub trigger_type: TriggerType,
    pub judge_input_excerpt: Option<String>,
    pub judge_verdict: String,
    pub confidence: Option<f32>,
    pub action: LedgerAction,
    pub detail: Option<String>,
    pub discord_message_id: Option<String>,
}

#[derive(Clone)]
pub struct ScoutPitchLedger {
    pool: PgPool,
}

impl ScoutPitchLedger {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn is_blacklisted(&self, login: &str) -> Result<bool, sqlx::Error> {
        sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM twitch_scout_pitch_blacklist WHERE LOWER(streamer_login) = LOWER($1))",
        )
        .bind(login)
        .fetch_one(&self.pool)
        .await
    }

    pub async fn has_posted_for_stream(
        &self,
        login: &str,
        trigger_type: TriggerType,
        stream_key: &str,
    ) -> Result<bool, sqlx::Error> {
        sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM twitch_scout_pitch_ledger \
             WHERE LOWER(streamer_login) = LOWER($1) AND trigger_type = $2 \
               AND action = 'posted' AND detail = $3)",
        )
        .bind(login)
        .bind(trigger_type.as_str())
        .bind(stream_key)
        .fetch_one(&self.pool)
        .await
    }

    pub async fn record(&self, entry: &LedgerEntry) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO twitch_scout_pitch_ledger \
             (streamer_login, trigger_type, judge_input_excerpt, judge_verdict, confidence, action, detail, discord_message_id) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(&entry.streamer_login)
        .bind(entry.trigger_type.as_str())
        .bind(&entry.judge_input_excerpt)
        .bind(&entry.judge_verdict)
        .bind(entry.confidence)
        .bind(entry.action.as_str())
        .bind(&entry.detail)
        .bind(&entry.discord_message_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
