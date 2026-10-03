//! Clip-Einreichung aus Twitch für den wöchentlichen Clip-Contest im Discord
//! (Community-Streamer-Brücke, Paket E).
//!
//! Ein Dienst für beide Wege: den Chat-Befehl `!clipcontest [clip-url]` im
//! eigenen Kanal eines aktiven Partners und den Knopf im Social-Studio des
//! Dashboards. Ablauf:
//!
//! 1. Kanal muss aktiver Partner sein.
//! 2. Clip-URL prüfen (nur `clips.twitch.tv/<slug>` oder
//!    `twitch.tv/<kanal>/clip/<slug>`); ohne URL den jüngsten Clip der
//!    laufenden Session nehmen (`!clip`-Ereignisse und Clip-Pipeline).
//! 3. Bestand und Tageslimit je Kanal prüfen (`twitch_clip_contest_forwards`).
//! 4. Clip per Helix `GET /clips?id=` prüfen: existiert und gehört zum Kanal.
//! 5. Einreichung beanspruchen (Advisory-Lock je Kanal, Zeile `pending`),
//!    dann `POST /internal/master/v1/clips/submit` am Master-Broker mit
//!    `idempotency_key = "twitch-clip-<clip_id>"`.
//! 6. Ergebnis speichern und als [`SubmitOutcome`] zurückgeben.
//!
//! Der Broker-Zugang steckt hinter [`ClipContestBroker`]: der tb-bot nutzt
//! den vorhandenen `BrokerRelay`, das Dashboard seinen vorhandenen
//! Broker-Aufruf. Beide verwenden das bestehende interne Token.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{PgPool, Row};
use tb_transport_twitch::HelixClient;

use crate::types::ChatMessageEvent;

/// Höchstens so viele Einreichungen je Kanal und Berliner Tag.
pub const DAILY_LIMIT_PER_CHANNEL: i64 = 3;
/// So lange gilt eine laufende Einreichung als in Arbeit (Doppelsend-Schutz).
pub const PENDING_TTL_SECS: i64 = 120;
/// Obergrenze des Titels laut Broker-Vertrag.
pub const MAX_TITLE_CHARS: usize = 200;
/// Präfix des Idempotenz-Schlüssels laut Bauplan.
pub const IDEMPOTENCY_PREFIX: &str = "twitch-clip-";

pub const REPLY_ACCEPTED: &str = "Clip ist im Wochen-Contest, die Community stimmt im Discord ab.";
pub const REPLY_ALREADY_IN: &str = "Der Clip wurde bereits eingereicht und wird nicht erneut gesendet.";
pub const REPLY_BROKER_UNAVAILABLE: &str =
    "Der Discord ist gerade nicht erreichbar. Versuch es später nochmal.";
pub const REPLY_HELP: &str =
    "So geht's: !clipcontest <Clip-Link> reicht einen Clip aus diesem Kanal für den Wochen-Contest im Discord ein. Ohne Link nehme ich den neuesten Clip aus dem laufenden Stream.";
pub const REPLY_NOT_FOUND: &str = "Diesen Clip finde ich auf Twitch nicht.";
pub const REPLY_FOREIGN: &str = "Es zählen nur Clips aus diesem Kanal.";
pub const REPLY_NOT_PARTNER: &str = "Clips einreichen geht nur in Kanälen von Partnern.";
pub const REPLY_RATE_LIMITED: &str =
    "Heute sind schon 3 Clips aus diesem Kanal eingereicht. Morgen geht es weiter.";
pub const REPLY_TWITCH_UNAVAILABLE: &str =
    "Twitch antwortet gerade nicht. Versuch es gleich nochmal.";
pub const REPLY_STORE_UNAVAILABLE: &str = "Das klappt gerade nicht. Versuch es gleich nochmal.";
pub const REPLY_NOT_ALLOWED: &str = "Clips einreichen können nur der Broadcaster und Mods.";

// ─── URL ────────────────────────────────────────────────────────────────────

fn valid_slug(slug: &str) -> bool {
    (3..=100).contains(&slug.len())
        && slug
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Twitch-ID in der Form, die der Broker annimmt (Ziffern, keine führende
/// Null, höchstens 20 Stellen). Sonst würde der Broker mit 400 ablehnen.
pub fn valid_twitch_id(id: &str) -> bool {
    (1..=20).contains(&id.len()) && !id.starts_with('0') && id.bytes().all(|b| b.is_ascii_digit())
}

fn valid_login(login: &str) -> bool {
    (1..=25).contains(&login.len())
        && login
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// Liefert die Clip-ID (Slug) aus einer Twitch-Clip-URL. Erlaubt sind nur
/// `clips.twitch.tv/<slug>` und `(www.|m.)twitch.tv/<kanal>/clip/<slug>`,
/// mit oder ohne `https://`. Query und Fragment werden ignoriert.
pub fn parse_clip_url(raw: &str) -> Option<String> {
    let raw = raw.trim().trim_start_matches('<').trim_end_matches('>');
    if raw.is_empty() || raw.len() > 400 || raw.chars().any(char::is_whitespace) {
        return None;
    }
    let with_scheme = if raw.contains("://") {
        raw.to_string()
    } else {
        format!("https://{raw}")
    };
    let url = reqwest::Url::parse(&with_scheme).ok()?;
    if !matches!(url.scheme(), "https" | "http")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return None;
    }
    let host = url.host_str()?.to_ascii_lowercase();
    let segments: Vec<&str> = url.path_segments()?.filter(|s| !s.is_empty()).collect();
    let slug = match (host.as_str(), segments.as_slice()) {
        ("clips.twitch.tv", [slug]) => *slug,
        ("twitch.tv" | "www.twitch.tv" | "m.twitch.tv", [channel, "clip", slug])
            if valid_login(channel) =>
        {
            *slug
        }
        _ => return None,
    };
    valid_slug(slug).then(|| slug.to_string())
}

/// Kanonische Clip-URL, wie sie der Broker speichert.
pub fn canonical_clip_url(clip_id: &str) -> String {
    format!("https://clips.twitch.tv/{clip_id}")
}

/// Idempotenz-Schlüssel laut Bauplan: `twitch-clip-<clip_id>`.
pub fn idempotency_key(clip_id: &str) -> String {
    format!("{IDEMPOTENCY_PREFIX}{clip_id}")
}

// ─── Rechte, Limit, Bestand (reine Regeln) ─────────────────────────────────

/// Wer darf im Chat einreichen?
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatRecht {
    Erlaubt,
    NichtErlaubt,
    /// Nachricht stammt per Shared Chat aus einem anderen Kanal: Abzeichen
    /// gelten dort, nicht hier. Keine Antwort.
    FremderKanal,
}

/// Nur Broadcaster und Moderatoren des eigenen Kanals.
pub fn chat_recht(event: &ChatMessageEvent) -> ChatRecht {
    if event
        .source_broadcaster_user_id
        .as_deref()
        .is_some_and(|source| !source.is_empty() && source != event.broadcaster_user_id)
    {
        return ChatRecht::FremderKanal;
    }
    if event.is_broadcaster() || event.is_mod_or_broadcaster() {
        ChatRecht::Erlaubt
    } else {
        ChatRecht::NichtErlaubt
    }
}

/// Tageslimit erreicht?
pub fn limit_erreicht(einreichungen_heute: i64) -> bool {
    einreichungen_heute >= DAILY_LIMIT_PER_CHANNEL
}

/// Was der vorhandene Eintrag eines Clips für eine neue Einreichung bedeutet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bestand {
    /// Kein Eintrag oder ein neuer Versuch ist erlaubt (failed, rejected,
    /// veraltetes pending).
    Frei,
    /// Schon angenommen oder als Duplikat gemeldet.
    SchonDrin,
    /// Läuft gerade (Doppelsend).
    Laeuft,
}

pub fn bestand(status: &str, updated_at: DateTime<Utc>, now: DateTime<Utc>) -> Bestand {
    match status {
        "accepted" | "duplicate" => Bestand::SchonDrin,
        "pending" if now - updated_at < Duration::seconds(PENDING_TTL_SECS) => Bestand::Laeuft,
        _ => Bestand::Frei,
    }
}

/// Titel für den Broker: getrimmt, ohne Steuerzeichen, höchstens 200 Zeichen.
pub fn broker_title(raw: &str) -> Option<String> {
    let cleaned: String = raw
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(
        trimmed
            .chars()
            .take(MAX_TITLE_CHARS)
            .collect::<String>()
            .trim_end()
            .to_string(),
    )
}

// ─── Ports ──────────────────────────────────────────────────────────────────

/// Clip-Daten aus Helix `GET /clips?id=`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipInfo {
    pub id: String,
    pub broadcaster_id: String,
    pub title: String,
}

#[async_trait]
pub trait ClipLookup: Send + Sync {
    /// `Ok(None)`: Clip existiert nicht (mehr). `Err`: Twitch nicht erreichbar.
    async fn clip(&self, clip_id: &str) -> Result<Option<ClipInfo>, String>;
}

/// Helix-Abfrage mit dem vorhandenen App-Token-Client.
pub struct HelixClipLookup {
    helix: HelixClient,
}

impl HelixClipLookup {
    pub fn new(helix: HelixClient) -> Self {
        Self { helix }
    }
}

/// Liest den passenden Clip aus der Helix-Antwort.
pub fn clip_from_helix(body: &Value, clip_id: &str) -> Option<ClipInfo> {
    let item = body
        .get("data")?
        .as_array()?
        .iter()
        .find(|item| item.get("id").and_then(Value::as_str) == Some(clip_id))?;
    let text = |name: &str| {
        item.get(name)
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default()
            .to_string()
    };
    let broadcaster_id = text("broadcaster_id");
    if broadcaster_id.is_empty() {
        return None;
    }
    Some(ClipInfo {
        id: clip_id.to_string(),
        broadcaster_id,
        title: text("title"),
    })
}

#[async_trait]
impl ClipLookup for HelixClipLookup {
    async fn clip(&self, clip_id: &str) -> Result<Option<ClipInfo>, String> {
        let builder = self
            .helix
            .get("/clips")
            .await
            .map_err(|e| e.to_string())?
            .query(&[("id", clip_id)]);
        let response = self
            .helix
            .send_with_retry(builder)
            .await
            .map_err(|e| e.to_string())?;
        let status = response.status();
        if status.as_u16() == 404 {
            return Ok(None);
        }
        if !status.is_success() {
            return Err(format!("Helix-Status {}", status.as_u16()));
        }
        let body: Value = response.json().await.map_err(|e| e.to_string())?;
        Ok(clip_from_helix(&body, clip_id))
    }
}

/// Body von `POST /internal/master/v1/clips/submit` (exakt nach Bauplan).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BrokerClipRequest {
    pub source: String,
    pub clip_url: String,
    pub streamer_twitch_user_id: String,
    pub streamer_login: String,
    pub submitted_by_twitch_user_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub submitted_at: Option<String>,
    pub title: Option<String>,
    pub idempotency_key: String,
}

enum ClaimOutcome {
    Finished(SubmitOutcome),
    Forward {
        submitted_by: Option<String>,
        submitted_at: Option<DateTime<Utc>>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BrokerClipStatus {
    Accepted,
    Duplicate,
    Rejected,
}

/// `result` der Broker-Antwort.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct BrokerClipResponse {
    pub status: BrokerClipStatus,
    #[serde(default)]
    pub submission_id: Option<i64>,
    #[serde(default)]
    pub reason: Option<String>,
}

/// Liest den Broker-Envelope `{"ok":true,"result":{...}}`.
pub fn parse_broker_envelope(value: &Value) -> Option<BrokerClipResponse> {
    if value.get("ok").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    serde_json::from_value(value.get("result")?.clone()).ok()
}

#[async_trait]
pub trait ClipContestBroker: Send + Sync {
    /// `Err` heißt: Broker nicht erreichbar oder Antwort unbrauchbar.
    async fn submit(&self, request: &BrokerClipRequest) -> Result<BrokerClipResponse, String>;
}

// ─── Dienst ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmitVia {
    Chat,
    Dashboard,
}

impl SubmitVia {
    fn as_str(self) -> &'static str {
        match self {
            SubmitVia::Chat => "chat",
            SubmitVia::Dashboard => "dashboard",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SubmitRequest {
    pub broadcaster_id: String,
    pub broadcaster_login: String,
    /// Twitch-ID der Person, die einreicht (Broadcaster oder Mod).
    pub submitted_by: Option<String>,
    /// `None`: jüngster Clip der laufenden Session.
    pub clip_url: Option<String>,
    pub via: SubmitVia,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubmitOutcome {
    Accepted {
        clip_url: String,
        submission_id: Option<i64>,
    },
    AlreadyIn {
        clip_url: String,
    },
    Rejected {
        reason: Option<String>,
    },
    BrokerUnavailable,
    InvalidUrl,
    NoRecentClip,
    ClipNotFound,
    ForeignClip,
    NotPartner,
    RateLimited,
    InFlight,
    TwitchUnavailable,
    StoreUnavailable,
}

impl SubmitOutcome {
    /// Maschinenlesbarer Kurzname (Dashboard-Antwort, Logs).
    pub fn code(&self) -> &'static str {
        match self {
            SubmitOutcome::Accepted { .. } => "accepted",
            SubmitOutcome::AlreadyIn { .. } => "already_in",
            SubmitOutcome::Rejected { .. } => "rejected",
            SubmitOutcome::BrokerUnavailable => "broker_unavailable",
            SubmitOutcome::InvalidUrl => "invalid_url",
            SubmitOutcome::NoRecentClip => "no_recent_clip",
            SubmitOutcome::ClipNotFound => "clip_not_found",
            SubmitOutcome::ForeignClip => "foreign_clip",
            SubmitOutcome::NotPartner => "not_partner",
            SubmitOutcome::RateLimited => "rate_limited",
            SubmitOutcome::InFlight => "in_flight",
            SubmitOutcome::TwitchUnavailable => "twitch_unavailable",
            SubmitOutcome::StoreUnavailable => "unavailable",
        }
    }

    /// Antworttext für Chat und Dashboard. `None`: keine Antwort
    /// (Doppelsend, die erste Einreichung antwortet schon).
    pub fn reply(&self) -> Option<String> {
        Some(match self {
            SubmitOutcome::Accepted { .. } => REPLY_ACCEPTED.to_string(),
            SubmitOutcome::AlreadyIn { .. } => REPLY_ALREADY_IN.to_string(),
            SubmitOutcome::Rejected { reason } => format!(
                "Der Clip wurde nicht angenommen: {}",
                reject_reason_text(reason.as_deref())
            ),
            SubmitOutcome::BrokerUnavailable => REPLY_BROKER_UNAVAILABLE.to_string(),
            SubmitOutcome::InvalidUrl | SubmitOutcome::NoRecentClip => REPLY_HELP.to_string(),
            SubmitOutcome::ClipNotFound => REPLY_NOT_FOUND.to_string(),
            SubmitOutcome::ForeignClip => REPLY_FOREIGN.to_string(),
            SubmitOutcome::NotPartner => REPLY_NOT_PARTNER.to_string(),
            SubmitOutcome::RateLimited => REPLY_RATE_LIMITED.to_string(),
            SubmitOutcome::InFlight => return None,
            SubmitOutcome::TwitchUnavailable => REPLY_TWITCH_UNAVAILABLE.to_string(),
            SubmitOutcome::StoreUnavailable => REPLY_STORE_UNAVAILABLE.to_string(),
        })
    }
}

/// Kurzer, verständlicher Grund zu einem Broker-`reason`.
pub fn reject_reason_text(reason: Option<&str>) -> &'static str {
    match reason {
        Some("not_partner") => {
            "dieser Kanal ist im Discord noch nicht als Partner eingetragen. Bitte melde dich beim Team."
        }
        Some("invalid_clip_url") => "der Link ist kein gültiger Twitch-Clip.",
        Some("idempotency_conflict") => "er passt nicht zu einer früheren Einreichung.",
        Some("duplicate_clip_this_week") => "er ist diese Woche schon dabei.",
        _ => "der Contest nimmt ihn gerade nicht an.",
    }
}

pub struct ClipContestSubmitter {
    pool: PgPool,
    lookup: Arc<dyn ClipLookup>,
    broker: Arc<dyn ClipContestBroker>,
}

struct BestandZeile {
    status: String,
    updated_at: DateTime<Utc>,
}

impl ClipContestSubmitter {
    pub fn new(
        pool: PgPool,
        lookup: Arc<dyn ClipLookup>,
        broker: Arc<dyn ClipContestBroker>,
    ) -> Self {
        Self {
            pool,
            lookup,
            broker,
        }
    }

    pub async fn submit(&self, request: SubmitRequest) -> SubmitOutcome {
        match self.submit_inner(&request).await {
            Ok(outcome) => outcome,
            Err(error) => {
                tracing::warn!(%error, channel = %request.broadcaster_login, "Clip-Contest-Einreichung: Datenbankfehler");
                SubmitOutcome::StoreUnavailable
            }
        }
    }

    async fn submit_inner(&self, request: &SubmitRequest) -> Result<SubmitOutcome, sqlx::Error> {
        let broadcaster_id = request.broadcaster_id.trim();
        if broadcaster_id.is_empty() {
            return Ok(SubmitOutcome::NotPartner);
        }
        let Some(partner_login) = self.active_partner_login(broadcaster_id).await? else {
            return Ok(SubmitOutcome::NotPartner);
        };
        let streamer_login = if partner_login.trim().is_empty() {
            request.broadcaster_login.trim().to_ascii_lowercase()
        } else {
            partner_login.trim().to_ascii_lowercase()
        };

        let clip_id = match request.clip_url.as_deref().map(str::trim) {
            Some(raw) if !raw.is_empty() => match parse_clip_url(raw) {
                Some(id) => id,
                None => return Ok(SubmitOutcome::InvalidUrl),
            },
            _ => match self
                .latest_session_clip(broadcaster_id, &streamer_login)
                .await?
            {
                Some(id) => id,
                None => return Ok(SubmitOutcome::NoRecentClip),
            },
        };
        let clip_url = canonical_clip_url(&clip_id);

        let now = Utc::now();
        if let Some(outcome) = self
            .precheck(&clip_id, &clip_url, broadcaster_id, now)
            .await?
        {
            return Ok(outcome);
        }

        let clip = match self.lookup.clip(&clip_id).await {
            Ok(Some(clip)) => clip,
            Ok(None) => return Ok(SubmitOutcome::ClipNotFound),
            Err(error) => {
                tracing::warn!(%error, clip_id, "Clip-Contest: Helix-Abfrage fehlgeschlagen");
                return Ok(SubmitOutcome::TwitchUnavailable);
            }
        };
        if clip.broadcaster_id != broadcaster_id {
            return Ok(SubmitOutcome::ForeignClip);
        }

        let (submitted_by, submitted_at) = match self
            .claim(
                request,
                &clip_id,
                &clip_url,
                broadcaster_id,
                &streamer_login,
            )
            .await?
        {
            ClaimOutcome::Finished(outcome) => return Ok(outcome),
            ClaimOutcome::Forward {
                submitted_by,
                submitted_at,
            } => (submitted_by, submitted_at),
        };

        let broker_request = BrokerClipRequest {
            source: "twitch".into(),
            clip_url: clip_url.clone(),
            streamer_twitch_user_id: broadcaster_id.to_string(),
            streamer_login,
            submitted_by_twitch_user_id: submitted_by
                .as_deref()
                .map(str::trim)
                .filter(|id| valid_twitch_id(id))
                .map(str::to_string),
            submitted_at: submitted_at.map(|at| at.to_rfc3339_opts(SecondsFormat::Micros, true)),
            title: broker_title(&clip.title),
            idempotency_key: idempotency_key(&clip_id),
        };
        let (status, submission_id, reason, outcome) =
            match self.broker.submit(&broker_request).await {
                Ok(response) => match response.status {
                    BrokerClipStatus::Accepted => (
                        "accepted",
                        response.submission_id,
                        None,
                        SubmitOutcome::Accepted {
                            clip_url: clip_url.clone(),
                            submission_id: response.submission_id,
                        },
                    ),
                    BrokerClipStatus::Duplicate => (
                        "duplicate",
                        response.submission_id,
                        response.reason.clone(),
                        SubmitOutcome::AlreadyIn {
                            clip_url: clip_url.clone(),
                        },
                    ),
                    BrokerClipStatus::Rejected => (
                        "rejected",
                        response.submission_id,
                        response.reason.clone(),
                        SubmitOutcome::Rejected {
                            reason: response.reason.clone(),
                        },
                    ),
                },
                Err(error) => {
                    tracing::warn!(%error, clip_id, "Clip-Contest: Broker nicht erreichbar");
                    ("failed", None, None, SubmitOutcome::BrokerUnavailable)
                }
            };
        let persisted = sqlx::query(
            "UPDATE twitch_clip_contest_forwards
                SET status = $2, broker_submission_id = $3, reason = $4,
                    updated_at = clock_timestamp()
              WHERE clip_id = $1",
        )
        .bind(&clip_id)
        .bind(status)
        .bind(submission_id)
        .bind(reason)
        .execute(&self.pool)
        .await?;
        if persisted.rows_affected() != 1 {
            return Err(sqlx::Error::RowNotFound);
        }
        Ok(outcome)
    }

    async fn active_partner_login(
        &self,
        broadcaster_id: &str,
    ) -> Result<Option<String>, sqlx::Error> {
        sqlx::query_scalar::<_, Option<String>>(
            "SELECT twitch_login FROM twitch_streamers_partner_state
              WHERE twitch_user_id = $1 AND is_partner_active = 1
              LIMIT 1",
        )
        .bind(broadcaster_id)
        .fetch_optional(&self.pool)
        .await
        .map(|row| row.map(Option::unwrap_or_default))
    }

    /// Jüngster Clip der laufenden Session: `!clip`-Ereignisse und die
    /// Clip-Pipeline (`twitch_clips_social_media`). Ohne offene Session `None`.
    pub async fn latest_session_clip(
        &self,
        broadcaster_id: &str,
        login: &str,
    ) -> Result<Option<String>, sqlx::Error> {
        let started_at: Option<DateTime<Utc>> = sqlx::query_scalar(
            "SELECT started_at FROM twitch_stream_sessions
              WHERE (twitch_user_id = $1
                     OR (COALESCE(twitch_user_id, '') = '' AND LOWER(streamer_login) = $2))
                AND ended_at IS NULL
              ORDER BY started_at DESC
              LIMIT 1",
        )
        .bind(broadcaster_id)
        .bind(login)
        .fetch_optional(&self.pool)
        .await?;
        let Some(started_at) = started_at else {
            return Ok(None);
        };
        let command: Option<(String, DateTime<Utc>)> = sqlx::query_as(
            "SELECT clip_id, requested_at FROM twitch_clip_command_events
              WHERE twitch_user_id = $1 AND requested_at >= $2
              ORDER BY requested_at DESC
              LIMIT 1",
        )
        .bind(broadcaster_id)
        .bind(started_at)
        .fetch_optional(&self.pool)
        .await?;
        let pipeline: Option<(String, DateTime<Utc>)> = sqlx::query_as(
            "SELECT clip_id, created_at FROM twitch_clips_social_media
              WHERE twitch_user_id = $1
                AND source_kind = 'twitch'
                AND discarded_at IS NULL
                AND created_at >= $2
              ORDER BY created_at DESC
              LIMIT 1",
        )
        .bind(broadcaster_id)
        .bind(started_at)
        .fetch_optional(&self.pool)
        .await?;
        let newest = match (command, pipeline) {
            (Some(a), Some(b)) => Some(if b.1 > a.1 { b } else { a }),
            (a, b) => a.or(b),
        };
        Ok(newest.map(|(id, _)| id).filter(|id| valid_slug(id)))
    }

    async fn bestand_zeile(&self, clip_id: &str) -> Result<Option<BestandZeile>, sqlx::Error> {
        let row = sqlx::query(
            "SELECT status, updated_at FROM twitch_clip_contest_forwards WHERE clip_id = $1",
        )
        .bind(clip_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| {
            Ok(BestandZeile {
                status: row.try_get("status")?,
                updated_at: row.try_get("updated_at")?,
            })
        })
        .transpose()
    }

    async fn count_today<'e, E>(
        executor: E,
        broadcaster_id: &str,
        now: DateTime<Utc>,
    ) -> Result<i64, sqlx::Error>
    where
        E: sqlx::PgExecutor<'e>,
    {
        let (day_start, _) = tb_analytics::community_points::berlin_day_bounds(
            tb_analytics::community_points::berlin_day(now),
        );
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM twitch_clip_contest_forwards
              WHERE broadcaster_twitch_id = $1
                AND created_at >= $2
                AND (status = 'accepted'
                     OR (status = 'pending' AND updated_at > $3))",
        )
        .bind(broadcaster_id)
        .bind(day_start)
        .bind(now - Duration::seconds(PENDING_TTL_SECS))
        .fetch_one(executor)
        .await
    }

    /// Günstige Vorprüfung ohne Lock: Bestand und Tageslimit.
    async fn precheck(
        &self,
        clip_id: &str,
        clip_url: &str,
        broadcaster_id: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<SubmitOutcome>, sqlx::Error> {
        if let Some(zeile) = self.bestand_zeile(clip_id).await? {
            match bestand(&zeile.status, zeile.updated_at, now) {
                Bestand::SchonDrin => {
                    return Ok(Some(SubmitOutcome::AlreadyIn {
                        clip_url: clip_url.to_string(),
                    }))
                }
                Bestand::Laeuft => return Ok(Some(SubmitOutcome::InFlight)),
                Bestand::Frei => {}
            }
        }
        if limit_erreicht(Self::count_today(&self.pool, broadcaster_id, now).await?) {
            return Ok(Some(SubmitOutcome::RateLimited));
        }
        Ok(None)
    }

    /// Beansprucht die Einreichung unter Advisory-Lock je Kanal und liefert
    /// den unveränderlichen Ursprung für die Brokerweitergabe.
    async fn claim(
        &self,
        request: &SubmitRequest,
        clip_id: &str,
        clip_url: &str,
        broadcaster_id: &str,
        streamer_login: &str,
    ) -> Result<ClaimOutcome, sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
            .bind(format!("clip-contest-forward:{broadcaster_id}"))
            .execute(&mut *tx)
            .await?;
        let now: DateTime<Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
            .fetch_one(&mut *tx)
            .await?;
        let zeile: Option<(String, DateTime<Utc>)> = sqlx::query_as(
            "SELECT status, updated_at FROM twitch_clip_contest_forwards
              WHERE clip_id = $1 FOR UPDATE",
        )
        .bind(clip_id)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some((status, updated_at)) = zeile {
            match bestand(&status, updated_at, now) {
                Bestand::SchonDrin => {
                    return Ok(ClaimOutcome::Finished(SubmitOutcome::AlreadyIn {
                        clip_url: clip_url.to_string(),
                    }))
                }
                Bestand::Laeuft => return Ok(ClaimOutcome::Finished(SubmitOutcome::InFlight)),
                Bestand::Frei => {}
            }
        }
        if limit_erreicht(Self::count_today(&mut *tx, broadcaster_id, now).await?) {
            return Ok(ClaimOutcome::Finished(SubmitOutcome::RateLimited));
        }
        let (submitted_by, submitted_at): (Option<String>, Option<DateTime<Utc>>) = sqlx::query_as(
            "INSERT INTO twitch_clip_contest_forwards
                 (clip_id, clip_url, broadcaster_twitch_id, broadcaster_login,
                  submitted_by_twitch_id, via, status, submitted_at)
             VALUES ($1, $2, $3, $4, $5, $6, 'pending', clock_timestamp())
             ON CONFLICT (clip_id) DO UPDATE SET
                 clip_url = EXCLUDED.clip_url,
                 broadcaster_login = EXCLUDED.broadcaster_login,
                 via = EXCLUDED.via,
                 status = 'pending',
                 broker_submission_id = NULL,
                 reason = NULL,
                 created_at = clock_timestamp(),
                 updated_at = clock_timestamp()
             RETURNING submitted_by_twitch_id, submitted_at",
        )
        .bind(clip_id)
        .bind(clip_url)
        .bind(broadcaster_id)
        .bind(streamer_login)
        .bind(
            request
                .submitted_by
                .as_deref()
                .filter(|id| !id.trim().is_empty()),
        )
        .bind(request.via.as_str())
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(ClaimOutcome::Forward {
            submitted_by,
            submitted_at,
        })
    }
}

#[cfg(test)]
#[path = "clip_contest_submit_tests.rs"]
mod tests;
