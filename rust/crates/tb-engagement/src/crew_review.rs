use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

pub const RICKY_TWITCH_USER_ID: &str = "147713656";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RickyChatInput {
    pub channel_login: String,
    pub subject_twitch_user_id: String,
    pub source_message_id: Option<String>,
    pub occurred_at: DateTime<Utc>,
    pub content: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewEventKind {
    SessionStarted,
    RickyMessage,
    StreamerTranscript,
    AiDecision,
    AiDraft,
    ProviderError,
    SessionEnded,
}

impl ReviewEventKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SessionStarted => "session_started",
            Self::RickyMessage => "ricky_message",
            Self::StreamerTranscript => "streamer_transcript",
            Self::AiDecision => "ai_decision",
            Self::AiDraft => "ai_draft",
            Self::ProviderError => "provider_error",
            Self::SessionEnded => "session_ended",
        }
    }

    pub(crate) fn from_db(value: &str) -> Option<Self> {
        match value {
            "session_started" => Some(Self::SessionStarted),
            "ricky_message" => Some(Self::RickyMessage),
            "streamer_transcript" => Some(Self::StreamerTranscript),
            "ai_decision" => Some(Self::AiDecision),
            "ai_draft" => Some(Self::AiDraft),
            "provider_error" => Some(Self::ProviderError),
            "session_ended" => Some(Self::SessionEnded),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct NewReviewEvent {
    pub session_id: Uuid,
    pub channel_login: String,
    pub subject_twitch_user_id: String,
    pub event_kind: ReviewEventKind,
    pub source_message_id: Option<String>,
    pub occurred_at: DateTime<Utc>,
    pub content: Option<String>,
    pub metadata: Value,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub confidence: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReviewEvent {
    pub id: i64,
    pub session_id: Uuid,
    pub channel_login: String,
    pub subject_twitch_user_id: String,
    pub event_kind: ReviewEventKind,
    pub source_message_id: Option<String>,
    pub occurred_at: DateTime<Utc>,
    pub content: Option<String>,
    pub metadata: Value,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub confidence: Option<f64>,
    pub discord_message_id: Option<String>,
    pub discord_deleted_at: Option<DateTime<Utc>>,
    pub last_delete_error: Option<String>,
    pub tombstoned_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReviewCycle {
    pub cycle_id: Uuid,
    pub session_id: Uuid,
    pub channel_login: String,
    pub events: Vec<ReviewEvent>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewSession {
    pub session_id: Uuid,
    pub channel_login: String,
    pub subject_twitch_user_id: String,
    pub started_at: DateTime<Utc>,
    pub last_activity_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExpiredDiscordGroup {
    pub discord_message_id: String,
    pub event_ids: Vec<i64>,
}
