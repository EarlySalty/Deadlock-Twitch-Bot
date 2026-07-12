//! Crew-Guard — Shadow-Mode.
//!
//! Erkennt eine EINE koordinierte Abwerbe-/Diffamierungs-Kampagne im
//! Twitch-Chat und meldet sie im **Shadow-Mode ausschliesslich nach Discord**
//! (an nani). Es gibt hier bewusst KEINE Aktion gegen den Chatter: kein Ban,
//! kein oeffentlicher Chat-Post, kein Whisper — nur die Info, damit der Mensch
//! neue Kampagnen-Versuche sieht und die Erkennung adaptieren kann.
//!
//! Zweistufig:
//!   1. [`screen`] — reine, synchrone Vorfilterung. Liefert **nur** ein Signal,
//!      NIE ein Urteil. Harte Signale (bekanntes Konto per Twitch-User-ID,
//!      bekannter Rival-Invite-Code) sind deterministisch; die Trigger-Muster
//!      sind bewusst MEHRDEUTIG und eskalieren lediglich zur GPT-Pruefung.
//!   2. [`CrewJudge`] — konservativer LLM-Klassifikator, der nur dann `is_crew`
//!      setzt, wenn das Kampagnen-Muster klar erkennbar ist. Fail-safe „unsure".

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use async_trait::async_trait;
use regex::Regex;
use serde::Deserialize;
use tracing::{debug, error, warn};

use crate::pipeline::ModAlerter;
use crate::types::ChatMessageEvent;

// ---------------------------------------------------------------------------
// Crew-Registry (harte Fakten) — bekannte Konten + bekannte Rival-Invite-Codes
// ---------------------------------------------------------------------------

/// Ein bekanntes Kampagnen-Konto. `has_behavioral_evidence` markiert, ob wir
/// zu diesem Konto bereits konkretes Kampagnen-Verhalten belegt haben.
struct CrewAccount {
    twitch_user_id: &'static str,
    login: &'static str,
    has_behavioral_evidence: bool,
}

/// Bekannte Kampagnen-Konten (hart). Match erfolgt ausschliesslich ueber die
/// Twitch-User-ID — ein umbenannter Account bleibt so erkannt.
const CREW_REGISTRY: &[CrewAccount] = &[
    CrewAccount {
        twitch_user_id: "89018048",
        login: "blackhusky45",
        has_behavioral_evidence: true,
    },
    CrewAccount {
        twitch_user_id: "147713656",
        login: "helmbombenricky",
        has_behavioral_evidence: true,
    },
    CrewAccount {
        twitch_user_id: "823493023",
        login: "skifahrertv",
        has_behavioral_evidence: true,
    },
    CrewAccount {
        twitch_user_id: "595804185",
        login: "h4teme666",
        has_behavioral_evidence: false,
    },
    CrewAccount {
        twitch_user_id: "1445014969",
        login: "mr_horizont",
        has_behavioral_evidence: false,
    },
    // Zweitkonto derselben Person wie mr_horizont (Ansage nani). Eigener
    // Verhaltensbeleg: ismile_e, 2026-07-06 — trug die Kampagne weiter,
    // nachdem helmbombenricky dort gebannt wurde.
    CrewAccount {
        twitch_user_id: "771345179",
        login: "wall_horizon",
        has_behavioral_evidence: true,
    },
];

/// Bekannte Rival-Invite-Codes (hart). Ein `discord.gg/<code>` mit einem dieser
/// Codes ist ein deterministisches Kampagnen-Signal.
const RIVAL_INVITE_CODES: &[&str] = &[
    "ZWSNyNfdG",
    "W7kCyBBcf",
    "XtXbc4ER",
    "cXndRbd2",
    "SBRrArXjHf",
];

// ---------------------------------------------------------------------------
// Signal aus der reinen Vorfilterung
// ---------------------------------------------------------------------------

/// Ergebnis von [`screen`]. Priorität: `HardId` > `HardInvite` > `Trigger` >
/// `None`. **Kein Urteil** — `Trigger` heisst nur „bitte GPT pruefen".
#[derive(Debug, Clone, PartialEq)]
pub enum CrewSignal {
    /// Bekanntes Konto per Twitch-User-ID getroffen.
    HardId {
        login: &'static str,
        has_evidence: bool,
    },
    /// Bekannter Rival-Invite-Code im Text.
    HardInvite { code: String },
    /// Ein oder mehrere mehrdeutige Trigger-Muster (Labels) getroffen.
    Trigger { hits: Vec<&'static str> },
    /// Nichts Relevantes.
    None,
}

/// Kompiliert (lazy) die mehrdeutigen Trigger-Matcher: `(Label, Regex)`.
///
/// WICHTIG: Ein Trigger-Treffer allein bedeutet NICHTS. Die Woerter kommen auch
/// in voellig harmlosem Chat vor. Ein Treffer eskaliert nur zur GPT-Pruefung;
/// [`screen`] faellt niemals ein Ban-Urteil.
fn trigger_matchers() -> &'static [(&'static str, Regex)] {
    static MATCHERS: OnceLock<Vec<(&'static str, Regex)>> = OnceLock::new();
    MATCHERS
        .get_or_init(|| {
            [
                ("helmbomben", r"helmbomben"),
                ("ricky", r"\bricky\b"),
                ("freund-gebannt", r"(freund|kollege).{0,40}(gebannt|banned)"),
                ("gebannt-freund", r"(gebannt|banned).{0,40}(freund|kollege)"),
                ("nani", r"\bna[nm]i\b"),
                ("bot-von-nani", r"bot von na[nm]i"),
                ("bannliste", r"bann?liste"),
            ]
            .into_iter()
            .filter_map(
                |(label, pattern)| match Regex::new(&format!("(?i){pattern}")) {
                    Ok(re) => Some((label, re)),
                    Err(err) => {
                        warn!("crew_guard: ungueltiges Trigger-Regex {label}: {err}");
                        None
                    }
                },
            )
            .collect()
        })
        .as_slice()
}

/// Lazy kompilierter Invite-Matcher: `discord.gg/<einer der bekannten Codes>`,
/// case-insensitive. Capture-Gruppe 1 = der gefundene Code (Original-Casing).
fn invite_matcher() -> Option<&'static Regex> {
    static MATCHER: OnceLock<Option<Regex>> = OnceLock::new();
    MATCHER
        .get_or_init(|| {
            let codes = RIVAL_INVITE_CODES.join("|");
            Regex::new(&format!(r"(?i)discord\.gg/({codes})")).ok()
        })
        .as_ref()
}

/// Alle getroffenen Trigger-Labels (mehrdeutig — nur Auslöser, kein Urteil).
fn trigger_hits(content: &str) -> Vec<&'static str> {
    trigger_matchers()
        .iter()
        .filter(|(_, re)| re.is_match(content))
        .map(|(label, _)| *label)
        .collect()
}

/// Reine Vorfilterung. Liefert das höchstpriorisierte Signal, ohne je ein
/// Urteil zu faellen. `chatter_id` = Twitch-User-ID des Chatters (falls bekannt).
pub fn screen(content: &str, chatter_id: Option<&str>) -> CrewSignal {
    // Priorität 1: bekanntes Konto per Twitch-User-ID (deterministisch).
    if let Some(id) = chatter_id {
        let id = id.trim();
        if let Some(account) = CREW_REGISTRY.iter().find(|acc| acc.twitch_user_id == id) {
            return CrewSignal::HardId {
                login: account.login,
                has_evidence: account.has_behavioral_evidence,
            };
        }
    }

    // Priorität 2: bekannter Rival-Invite-Code.
    if let Some(re) = invite_matcher() {
        if let Some(code) = re.captures(content).and_then(|caps| caps.get(1)) {
            return CrewSignal::HardInvite {
                code: code.as_str().to_string(),
            };
        }
    }

    // Priorität 3: mehrdeutige Trigger — NUR Auslöser für die GPT-Pruefung.
    let hits = trigger_hits(content);
    if !hits.is_empty() {
        return CrewSignal::Trigger { hits };
    }

    CrewSignal::None
}

// ---------------------------------------------------------------------------
// LLM-Judge (konservativer Klassifikator)
// ---------------------------------------------------------------------------

/// Urteil des LLM-Judge. `unsure` = fail-safe (kein Crew, Confidence 0).
#[derive(Debug, Clone, PartialEq)]
pub struct CrewVerdict {
    pub is_crew: bool,
    pub confidence: f32,
    pub patterns: Vec<String>,
    pub reasoning: String,
    pub failure_warning: bool,
}

impl CrewVerdict {
    /// Fail-safe: nichts erkannt, keine Aktion.
    pub fn unsure() -> Self {
        Self {
            is_crew: false,
            confidence: 0.0,
            patterns: Vec::new(),
            reasoning: String::new(),
            failure_warning: false,
        }
    }
}

#[async_trait]
pub trait CrewJudge: Send + Sync {
    async fn judge(&self, content: &str, recent_context: &[String]) -> CrewVerdict;
}

const JUDGE_FAILURE_WARNING_THRESHOLD: usize = 5;

#[derive(Default)]
struct JudgeFailureTracker {
    consecutive: AtomicUsize,
}

impl JudgeFailureTracker {
    fn record_failure(&self) -> bool {
        let previous = self
            .consecutive
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |count| {
                Some(count.saturating_add(1))
            })
            .unwrap_or_else(|count| count);
        previous == JUDGE_FAILURE_WARNING_THRESHOLD - 1
    }

    fn record_success(&self) {
        self.consecutive.store(0, Ordering::Relaxed);
    }
}

/// Wörtlicher deutscher System-Prompt (konservativer Klassifikator).
const CREW_JUDGE_SYSTEM_PROMPT: &str = r#"Du bist ein konservativer Klassifikator gegen EINE koordinierte Twitch-Chat-Kampagne. Muster: (a) fragt einen Streamer warum ein 'Freund/Kollege' (oft Helmbombenricky/Ricky) gebannt sei; (b) redet die Moderation bzw. 'den Bot von nani' schlecht (bannt unbewusst viele/Bannliste/nani ist woke/Rassist/Scheisse); (c) wirbt in einen anderen Discord ab (komm bei uns rein/unser Discord) oder postet einen Invite. Die Woerter nani, Ricky, Freund gebannt, Bannliste sind MEHRDEUTIG und kommen auch in harmlosem Chat vor. Stufe NIEMALS allein aufgrund dieser Woerter als Kampagne ein. Setze is_crew=true NUR wenn (b) UND ((c) ODER (a)) klar erkennbar sind. Im Zweifel is_crew=false. Antworte NUR als JSON: {"is_crew":bool,"confidence":0..1,"patterns":["a","b","c"],"reasoning":"kurz"}."#;

/// Timeout des Judge-HTTP-Calls.
const CREW_JUDGE_TIMEOUT_SECS: u64 = 12;
/// Default-Endpoint (OpenAI-kompatibel); via `OPENAI_BASE_URL` überschreibbar.
const DEFAULT_OPENAI_BASE_URL: &str = "https://api.openai.com/v1";

/// OpenAI-kompatibler Judge. Modellname NIE hardcoden — kommt aus
/// `CREW_GUARD_MODEL`. Fehlt das Modell (oder der Key), antwortet der Judge
/// fail-safe mit `unsure` (kein Crew), statt zu raten.
pub struct OpenAiCrewJudge {
    client: reqwest::Client,
    api_key: Option<String>,
    model: Option<String>,
    base_url: String,
    failures: JudgeFailureTracker,
}

impl OpenAiCrewJudge {
    pub fn from_env() -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(CREW_JUDGE_TIMEOUT_SECS))
            .build()
            .unwrap_or_default();
        Self {
            client,
            api_key: non_empty_env("OPENAI_API_KEY"),
            model: non_empty_env("CREW_GUARD_MODEL"),
            base_url: non_empty_env("OPENAI_BASE_URL")
                .unwrap_or_else(|| DEFAULT_OPENAI_BASE_URL.to_string()),
            failures: JudgeFailureTracker::default(),
        }
    }

    fn failure(
        &self,
        content: &str,
        error_kind: &'static str,
        detail: impl std::fmt::Display,
    ) -> CrewVerdict {
        let failure_warning = self.failures.record_failure();
        let consecutive_failures = self.failures.consecutive.load(Ordering::Relaxed);
        let input = truncate_content(content, CONTENT_PREVIEW_MAX);
        error!(
            error_kind,
            consecutive_failures,
            input = %input,
            error = %detail,
            "crew_guard: Judge-Ausfall"
        );
        CrewVerdict {
            failure_warning,
            ..CrewVerdict::unsure()
        }
    }
}

#[async_trait]
impl CrewJudge for OpenAiCrewJudge {
    async fn judge(&self, content: &str, recent_context: &[String]) -> CrewVerdict {
        let Some(model) = self.model.as_deref() else {
            warn!("crew_guard: CREW_GUARD_MODEL nicht gesetzt — Crew-Judge fail-safe unsure");
            return CrewVerdict::unsure();
        };
        let Some(api_key) = self.api_key.as_deref() else {
            debug!("crew_guard: OPENAI_API_KEY nicht gesetzt — Crew-Judge fail-safe unsure");
            return CrewVerdict::unsure();
        };

        let body = serde_json::json!({
            "model": model,
            "messages": [
                {"role": "system", "content": CREW_JUDGE_SYSTEM_PROMPT},
                {"role": "user", "content": build_user_content(content, recent_context)},
            ],
            "temperature": 0.0,
            "response_format": {"type": "json_object"},
        });
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));

        let resp = match self
            .client
            .post(&url)
            .bearer_auth(api_key)
            .json(&body)
            .send()
            .await
        {
            Ok(resp) => resp,
            Err(err) => {
                return self.failure(content, "http_transport", err);
            }
        };
        if !resp.status().is_success() {
            return self.failure(content, "http_status", resp.status());
        }
        let parsed = match resp.json::<ChatCompletion>().await {
            Ok(parsed) => parsed,
            Err(err) => {
                return self.failure(content, "response_json", err);
            }
        };
        let raw = parsed
            .choices
            .into_iter()
            .next()
            .and_then(|choice| choice.message.content)
            .unwrap_or_default();
        let Some(verdict) = parse_crew_verdict(&raw) else {
            return self.failure(content, "verdict_json", "ungültiges Judge-Urteil");
        };
        self.failures.record_success();
        verdict
    }
}

/// Baut den User-Prompt: der bisherige Chatverlauf DIESES Users (chronologisch,
/// je Zeile als `> …` vorangestellt) plus die aktuelle Nachricht. Die Kampagne
/// ist ein Mehr-Nachrichten-Bogen — einzelne Zeilen sind bewusst zu wenig, der
/// Kontext ist deshalb ausschlaggebend für die Recall.
fn build_user_content(content: &str, recent_context: &[String]) -> String {
    if recent_context.is_empty() {
        format!("Zu pruefende (aktuelle) Nachricht dieses Users:\n{content}")
    } else {
        let history = recent_context
            .iter()
            .map(|line| format!("> {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "Bisheriger Chatverlauf dieses Users (chronologisch):\n{history}\n\nZu pruefende (aktuelle) Nachricht dieses Users:\n{content}"
        )
    }
}

/// Robustes Bergen des JSON-Urteils (Stil wie `conversation_scam::parse_verdict`).
fn parse_crew_verdict(raw: &str) -> Option<CrewVerdict> {
    let parsed = serde_json::from_str::<RawCrewVerdict>(raw.trim()).or_else(|_| {
        extract_json_object(raw)
            .ok_or_else(|| serde_json::Error::io(std::io::Error::other("kein JSON-Objekt")))
            .and_then(serde_json::from_str::<RawCrewVerdict>)
    });
    let Ok(parsed) = parsed else { return None };
    if !parsed.confidence.is_finite() {
        return None;
    }
    Some(CrewVerdict {
        is_crew: parsed.is_crew,
        confidence: parsed.confidence.clamp(0.0, 1.0),
        patterns: parsed.patterns,
        reasoning: parsed.reasoning,
        failure_warning: false,
    })
}

/// Erstes balanciertes JSON-Objekt aus einem String bergen (String-aware).
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

#[derive(Debug, Deserialize)]
struct RawCrewVerdict {
    #[serde(default)]
    is_crew: bool,
    #[serde(default)]
    confidence: f32,
    #[serde(default)]
    patterns: Vec<String>,
    #[serde(default)]
    reasoning: String,
}

#[derive(Debug, Deserialize)]
struct ChatCompletion {
    #[serde(default)]
    choices: Vec<CompletionChoice>,
}

#[derive(Debug, Deserialize)]
struct CompletionChoice {
    message: CompletionMessage,
}

#[derive(Debug, Deserialize)]
struct CompletionMessage {
    #[serde(default)]
    content: Option<String>,
}

fn non_empty_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

// ---------------------------------------------------------------------------
// Discord-Meldung (Shadow) — Wortlaut fest verdrahtet
// ---------------------------------------------------------------------------

/// Vorschau-Länge des Original-Nachrichtentexts in der Discord-Meldung.
const CONTENT_PREVIEW_MAX: usize = 160;
const JUDGE_FAILURE_WARNING: &str = "PLATZHALTER: Crew-Guard-Judge-Ausfallwarnung";

/// Kürzt `content` char-sicher auf `max` Zeichen.
fn truncate_content(content: &str, max: usize) -> String {
    content.chars().take(max).collect()
}

/// Meldung für ein bekanntes Konto (HardId) bzw. einen harten Invite-Treffer.
fn format_known_account_alert(login: &str, channel: &str, patterns: &str) -> String {
    format!(
        "👀 Crew-Guard (Shadow): {login} ist grad in #{channel} unterwegs und fährt die Ricky-Nummer ({patterns}). Ich hab nichts getan — nur zur Info."
    )
}

/// Meldung für einen neuen, nicht gelisteten Versuch (per Judge erkannt).
fn format_new_account_alert(
    login: &str,
    channel: &str,
    patterns: &str,
    confidence: f32,
    content: &str,
) -> String {
    let preview = truncate_content(content, CONTENT_PREVIEW_MAX);
    format!(
        "🆕 Crew-Guard (Shadow): Neuer Account {login} in #{channel} zeigt das Kampagnen-Muster ({patterns}), steht aber NICHT auf der Liste (GPT-Confidence {confidence:.2}). Guck mal ob wir den aufnehmen. Nachricht: \"{preview}\""
    )
}

/// Leise Meldung für einen Trigger-Treffer, den der Judge NICHT als Kampagne
/// wertet. Kein Alarm, sondern ein Logbuch: nani sieht jeden Treffer selbst und
/// merkt so auch, wenn der Judge danebenliegt oder ausfällt.
fn format_trigger_log(
    login: &str,
    channel: &str,
    patterns: &str,
    confidence: f32,
    content: &str,
) -> String {
    let preview = truncate_content(content, CONTENT_PREVIEW_MAX);
    format!(
        "🔎 Crew-Guard (Log): {login} in #{channel} hat Trigger ausgelöst ({patterns}). Der Judge sagt: kein Kampagnen-Muster (Confidence {confidence:.2}). Ich hab nichts getan, guck selbst drauf. Nachricht: \"{preview}\""
    )
}

/// Patterns-Text für eine HardId-Meldung: getroffene Trigger, sonst „bekanntes
/// Konto" (mit Nachweis-Vermerk, wenn Verhaltens-Evidenz vorliegt).
fn hard_id_patterns(content: &str, has_evidence: bool) -> String {
    let hits = trigger_hits(content);
    if !hits.is_empty() {
        hits.join(", ")
    } else if has_evidence {
        "bekanntes Konto (Verhaltens-Nachweis)".to_string()
    } else {
        "bekanntes Konto".to_string()
    }
}

// ---------------------------------------------------------------------------
// Kontextfenster je (channel, chatter) — In-Memory, speicherbegrenzt
// ---------------------------------------------------------------------------

/// Anzahl der letzten Nachrichten je User, die als Judge-Kontext dienen.
const CONTEXT_WINDOW: usize = 6;
/// Deckel gegen unbegrenztes Wachstum: max. so viele (channel, chatter)-Keys.
const CONTEXT_MAX_KEYS: usize = 4096;

#[derive(Default)]
struct ContextStore {
    /// (channel, chatter-identity) → letzte Nachrichten (chronologisch).
    windows: HashMap<(String, String), VecDeque<String>>,
    /// Einfüge-Reihenfolge der Keys (vorne = ältester) für FIFO-Verdrängung.
    order: VecDeque<(String, String)>,
}

/// Speicherbegrenzter In-Memory-Puffer der letzten Nachrichten je User.
/// Threadsicher via `Mutex`; der Lock wird nur kurz und **await-frei** gehalten.
struct ChatterContextBuffer {
    inner: Mutex<ContextStore>,
}

impl ChatterContextBuffer {
    fn new() -> Self {
        Self {
            inner: Mutex::new(ContextStore::default()),
        }
    }

    /// Liefert die BISHERIGEN Nachrichten dieses Users (ohne die aktuelle) und
    /// schiebt die aktuelle Nachricht danach in den Puffer. Ein einzelner,
    /// kurzer Lock ohne `await` — clippy-sauber und race-frei zur Reihenfolge.
    fn snapshot_then_push(&self, channel: &str, identity: &str, content: &str) -> Vec<String> {
        let key = (channel.to_string(), identity.to_string());
        let mut guard = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        let store = &mut *guard;

        let existing = store.windows.get(&key);
        let prev: Vec<String> = existing
            .map(|window| window.iter().cloned().collect())
            .unwrap_or_default();
        let existed = existing.is_some();

        let window = store.windows.entry(key.clone()).or_default();
        window.push_back(content.to_string());
        while window.len() > CONTEXT_WINDOW {
            window.pop_front();
        }

        if !existed {
            store.order.push_back(key);
            while store.order.len() > CONTEXT_MAX_KEYS {
                if let Some(oldest) = store.order.pop_front() {
                    store.windows.remove(&oldest);
                } else {
                    break;
                }
            }
        }
        prev
    }
}

/// Kontext-Key-Identität: bevorzugt die stabile Twitch-User-ID, sonst der Login.
fn context_identity<'a>(chatter_id: &'a str, login: &'a str) -> &'a str {
    if chatter_id.trim().is_empty() {
        login
    } else {
        chatter_id
    }
}

// ---------------------------------------------------------------------------
// CrewGuard — Verdrahtung (Shadow-Mode, fire-and-forget)
// ---------------------------------------------------------------------------

/// Confidence-Schwelle, ab der ein Judge-Treffer im Trigger-Pfad gemeldet wird.
const JUDGE_CONFIDENCE_THRESHOLD: f32 = 0.7;

/// Feature-Flag `CREW_GUARD_ENABLED` (default AUS).
fn crew_guard_enabled() -> bool {
    std::env::var("CREW_GUARD_ENABLED")
        .map(|value| {
            matches!(
                value.trim().to_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

/// Shadow-Mode-Wächter: screent jede Partner-Nachricht und meldet Kampagnen-
/// Verdacht NUR nach Discord (kein Ban, kein Chat-Post, kein Whisper).
pub struct CrewGuard {
    enabled: bool,
    threshold: f32,
    judge: Arc<dyn CrewJudge>,
    alerter: Arc<ModAlerter>,
    context: ChatterContextBuffer,
}

impl CrewGuard {
    pub fn new(enabled: bool, judge: Arc<dyn CrewJudge>, alerter: Arc<ModAlerter>) -> Self {
        Self {
            enabled,
            threshold: JUDGE_CONFIDENCE_THRESHOLD,
            judge,
            alerter,
            context: ChatterContextBuffer::new(),
        }
    }

    /// Baut den Wächter aus der Umgebung: Feature-Flag + OpenAI-Judge.
    pub fn from_env(alerter: Arc<ModAlerter>) -> Self {
        Self::new(
            crew_guard_enabled(),
            Arc::new(OpenAiCrewJudge::from_env()),
            alerter,
        )
    }

    /// Fire-and-forget: blockiert die Chat-Pipeline nie. Bei ausgeschaltetem
    /// Feature-Flag ein sofortiger No-op (kein Spawn, keine Kosten).
    pub fn observe(&self, event: &ChatMessageEvent) {
        if !self.enabled {
            return;
        }
        let content = event.text().to_string();
        if content.is_empty() {
            return;
        }
        let channel = event.broadcaster_user_login.to_lowercase();
        let login = event.chatter_user_login.clone();
        let chatter_id = event.chatter_user_id.clone();

        // Kontextfenster: vorherige Nachrichten dieses Users holen (OHNE die
        // aktuelle) und die aktuelle danach in den Puffer schieben. Kurzer,
        // await-freier Lock — läuft synchron vor dem Spawn, damit die
        // chronologische Reihenfolge bei Nachrichten-Bursts erhalten bleibt.
        let identity = context_identity(&chatter_id, &login);
        let recent_context = self
            .context
            .snapshot_then_push(&channel, identity, &content);

        let judge = Arc::clone(&self.judge);
        let alerter = Arc::clone(&self.alerter);
        let threshold = self.threshold;

        tokio::spawn(async move {
            evaluate(
                &content,
                &chatter_id,
                &channel,
                &login,
                threshold,
                judge.as_ref(),
                &recent_context,
                &alerter,
            )
            .await;
        });
    }
}

/// Entscheidet OHNE Seiteneffekt, ob (und mit welchem Text) gemeldet würde.
/// Trennt die Erkennung von der Discord-Zustellung, damit der Backtest die
/// Detektion messen kann, ohne echte Alerts zu senden. `None` = keine Meldung.
///
/// Der `recent_context` (vorherige Nachrichten desselben Users) fliesst NUR in
/// den Trigger→Judge-Pfad ein; `HardId`/`HardInvite` bleiben deterministisch.
async fn decide(
    content: &str,
    chatter_id: &str,
    channel: &str,
    login: &str,
    threshold: f32,
    judge: &dyn CrewJudge,
    recent_context: &[String],
) -> Option<String> {
    // Safe-List zuerst: diese Konten reden über die Kampagne, gehören ihr aber
    // nicht an. Sie treffen die Trigger-Wörter zwangsläufig — nie melden.
    if crate::safe_list::is_safe(Some(chatter_id), login) {
        return None;
    }

    match screen(content, Some(chatter_id)) {
        CrewSignal::HardId {
            login: registry_login,
            has_evidence,
        } => {
            let patterns = hard_id_patterns(content, has_evidence);
            Some(format_known_account_alert(
                registry_login,
                channel,
                &patterns,
            ))
        }
        CrewSignal::HardInvite { code } => {
            let patterns = format!("Rival-Invite {code}");
            Some(format_known_account_alert(login, channel, &patterns))
        }
        CrewSignal::Trigger { hits } => {
            let verdict = judge.judge(content, recent_context).await;
            if verdict.failure_warning {
                Some(JUDGE_FAILURE_WARNING.to_string())
            } else if verdict.is_crew && verdict.confidence >= threshold {
                let patterns = if verdict.patterns.is_empty() {
                    hits.join(", ")
                } else {
                    verdict.patterns.join(", ")
                };
                Some(format_new_account_alert(
                    login,
                    channel,
                    &patterns,
                    verdict.confidence,
                    content,
                ))
            } else {
                // Der Judge verneint — trotzdem melden, nur leiser. Am
                // 2026-07-06 verschluckte genau dieser Zweig `wall_horizon`:
                // Trigger sass, Judge sagte nein, nani erfuhr nie davon. Bei
                // rund 0,8 Trigger-Treffern pro Tag ist das keine Flut.
                Some(format_trigger_log(
                    login,
                    channel,
                    &hits.join(", "),
                    verdict.confidence,
                    content,
                ))
            }
        }
        CrewSignal::None => None,
    }
}

/// Kern der Shadow-Auswertung: entscheiden, dann ggf. NUR nach Discord melden —
/// nie ein Ban/Chat-Post/Whisper.
#[allow(clippy::too_many_arguments)]
async fn evaluate(
    content: &str,
    chatter_id: &str,
    channel: &str,
    login: &str,
    threshold: f32,
    judge: &dyn CrewJudge,
    recent_context: &[String],
    alerter: &Arc<ModAlerter>,
) {
    if let Some(message) = decide(
        content,
        chatter_id,
        channel,
        login,
        threshold,
        judge,
        recent_context,
    )
    .await
    {
        alerter.send_crew_campaign(message);
    }
}

// ---------------------------------------------------------------------------
// Backtest — Vertrauen vor Live (statische Fixtures, kein Netz/DB)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    // Nur Texte — KEINE echten Usernamen als Chatter-Identität.
    const POSITIVES: [&str; 5] = [
        "hey mal ne frage warum ist mein freund helmbombenricky gebannt bei dir ?",
        "hast du den bot von nani drinne? du bannst unbewusst viele leute wegen der bannliste",
        "wenn du willst zeig ich dir was fuer ne scheisse nani macht, betitelt ihn als rassist",
        "komm bei uns rein, unser discord: https://discord.gg/SBRrArXjHf",
        "https://discord.gg/ZWSNyNfdG",
    ];
    const NEGATIVES: [&str; 5] = [
        "warum ist mein freund eigentlich gebannt? hab ich was verpasst",
        "nani spielt echt gut heute lol",
        "gibts ne bannliste fuer den chat oder wie",
        "ricky komm ins game",
        "welcher discord invite war das nochmal fuers turnier",
    ];

    /// Judge-Attrappe mit festem Urteil — kein Netz.
    struct StubJudge(CrewVerdict);

    #[async_trait]
    impl CrewJudge for StubJudge {
        async fn judge(&self, _content: &str, _recent_context: &[String]) -> CrewVerdict {
            self.0.clone()
        }
    }

    struct WarningJudge;

    #[async_trait]
    impl CrewJudge for WarningJudge {
        async fn judge(&self, _content: &str, _recent_context: &[String]) -> CrewVerdict {
            CrewVerdict {
                failure_warning: true,
                ..CrewVerdict::unsure()
            }
        }
    }

    fn judge_nein() -> StubJudge {
        StubJudge(CrewVerdict::unsure())
    }

    fn judge_ja() -> StubJudge {
        StubJudge(CrewVerdict {
            is_crew: true,
            confidence: 0.9,
            patterns: vec!["b".into(), "c".into()],
            reasoning: "klar".into(),
            failure_warning: false,
        })
    }

    #[test]
    fn judge_ausfallserie_warnt_genau_einmal_und_reset() {
        let failures = JudgeFailureTracker::default();

        for _ in 0..4 {
            assert!(!failures.record_failure());
        }
        assert!(failures.record_failure(), "fünfter Ausfall muss warnen");
        for _ in 0..5 {
            assert!(!failures.record_failure(), "nur eine Warnung je Serie");
        }

        failures.record_success();
        for _ in 0..4 {
            assert!(!failures.record_failure());
        }
        assert!(failures.record_failure(), "neue Serie muss erneut warnen");
    }

    #[tokio::test]
    async fn judge_ausfallwarnung_nutzt_shadow_meldeweg() {
        let message = decide(
            "hast du den bot von nani drin?",
            "555000111",
            "ismile_e",
            "neuer_account",
            JUDGE_CONFIDENCE_THRESHOLD,
            &WarningJudge,
            &[],
        )
        .await;

        assert_eq!(
            message.as_deref(),
            Some("PLATZHALTER: Crew-Guard-Judge-Ausfallwarnung")
        );
    }

    #[tokio::test]
    async fn judge_http_fehler_zaehlen_bis_zur_einmalwarnung() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        let judge = OpenAiCrewJudge {
            client: reqwest::Client::new(),
            api_key: Some("test-key".to_string()),
            model: Some("test-model".to_string()),
            base_url: server.uri(),
            failures: JudgeFailureTracker::default(),
        };

        for _ in 0..4 {
            assert!(!judge.judge("nani bannliste", &[]).await.failure_warning);
        }
        assert!(judge.judge("nani bannliste", &[]).await.failure_warning);
        assert!(!judge.judge("nani bannliste", &[]).await.failure_warning);
    }

    #[tokio::test]
    async fn unlesbares_judge_urteil_zaehlt_als_ausfall() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{"message": {"content": "kein json"}}]
            })))
            .mount(&server)
            .await;
        let judge = OpenAiCrewJudge {
            client: reqwest::Client::new(),
            api_key: Some("test-key".to_string()),
            model: Some("test-model".to_string()),
            base_url: server.uri(),
            failures: JudgeFailureTracker::default(),
        };

        for _ in 0..4 {
            assert!(!judge.judge("nani bannliste", &[]).await.failure_warning);
        }
        assert!(judge.judge("nani bannliste", &[]).await.failure_warning);
    }

    /// Der Vorfall vom 2026-07-06: `wall_horizon` fuhr das Skript, der Judge
    /// verneinte, es kam KEINE Meldung. Ab jetzt: Trigger meldet immer.
    #[tokio::test]
    async fn trigger_meldet_auch_wenn_judge_verneint() {
        let msg = decide(
            "helmbombenricky wollte nochmal nachdeinem dc fragen aber er ist gebant",
            "771345179",
            "ismile_e",
            "wall_horizon",
            JUDGE_CONFIDENCE_THRESHOLD,
            &judge_nein(),
            &[],
        )
        .await
        .expect("Trigger muss auch bei Judge-Nein gemeldet werden");

        assert!(msg.contains("wall_horizon"), "Login fehlt: {msg}");
        assert!(msg.contains("ismile_e"), "Kanal fehlt: {msg}");
    }

    #[tokio::test]
    async fn trigger_mit_judge_ja_bleibt_neu_account_alarm() {
        let msg = decide(
            "hast du den bot von nani drin? der bannt unbewusst viele wegen der bannliste",
            "555000111",
            "ismile_e",
            "neuer_account",
            JUDGE_CONFIDENCE_THRESHOLD,
            &judge_ja(),
            &[],
        )
        .await
        .expect("Judge-Ja muss melden");

        assert!(msg.contains("Neuer Account"), "kein Neu-Alarm: {msg}");
    }

    /// Safe-Konten treffen die Trigger-Wörter, dürfen aber NIE gemeldet werden.
    #[tokio::test]
    async fn safe_konto_erzeugt_keine_meldung() {
        for safe in crate::safe_list::SAFE_ACCOUNTS {
            let msg = decide(
                "jaja frag mal ricky, der is einfach ueberall gebannt",
                safe.twitch_user_id,
                "ismile_e",
                safe.login,
                JUDGE_CONFIDENCE_THRESHOLD,
                &judge_ja(),
                &[],
            )
            .await;
            assert!(
                msg.is_none(),
                "Safe-Konto {} wurde gemeldet: {msg:?}",
                safe.login
            );
        }
    }

    #[test]
    fn wall_horizon_ist_bekanntes_konto() {
        match screen("alles gut bei dir?", Some("771345179")) {
            CrewSignal::HardId {
                login,
                has_evidence,
            } => {
                assert_eq!(login, "wall_horizon");
                assert!(has_evidence, "Verhaltensbeleg vom 2026-07-06 liegt vor");
            }
            other => panic!("erwartet HardId, war {other:?}"),
        }
    }

    #[test]
    fn textbasierte_positiva_ohne_invite_sind_trigger() {
        for text in POSITIVES.into_iter().take(3) {
            let signal = screen(text, None);
            assert!(
                matches!(signal, CrewSignal::Trigger { .. }),
                "erwartet Trigger für {text:?}, war {signal:?}"
            );
        }
    }

    #[test]
    fn invite_positiva_sind_hard_invite() {
        match screen(POSITIVES[3], None) {
            CrewSignal::HardInvite { code } => assert_eq!(code, "SBRrArXjHf"),
            other => panic!("erwartet HardInvite, war {other:?}"),
        }
        match screen(POSITIVES[4], None) {
            CrewSignal::HardInvite { code } => assert_eq!(code, "ZWSNyNfdG"),
            other => panic!("erwartet HardInvite, war {other:?}"),
        }
    }

    #[test]
    fn kein_negativ_ist_hart() {
        for text in NEGATIVES {
            let signal = screen(text, None);
            assert!(
                !matches!(
                    signal,
                    CrewSignal::HardId { .. } | CrewSignal::HardInvite { .. }
                ),
                "Negativ {text:?} darf nicht HART sein, war {signal:?}"
            );
        }
    }

    #[test]
    fn hard_id_per_chatter_id_erkannt() {
        match screen("hallo zusammen, alles gut?", Some("147713656")) {
            CrewSignal::HardId {
                login,
                has_evidence,
            } => {
                assert_eq!(login, "helmbombenricky");
                assert!(has_evidence);
            }
            other => panic!("erwartet HardId, war {other:?}"),
        }
    }

    #[test]
    fn hard_id_schlaegt_invite() {
        // Registriertes Konto + Rival-Invite → HardId gewinnt (Priorität).
        let signal = screen("https://discord.gg/ZWSNyNfdG", Some("595804185"));
        assert!(
            matches!(
                signal,
                CrewSignal::HardId {
                    has_evidence: false,
                    ..
                }
            ),
            "erwartet HardId (Priorität), war {signal:?}"
        );
    }

    #[test]
    fn unbekannte_id_faellt_auf_textsignal_zurueck() {
        // Fremde ID + Invite → HardInvite (kein HardId).
        match screen("https://discord.gg/W7kCyBBcf", Some("999999999")) {
            CrewSignal::HardInvite { code } => assert_eq!(code, "W7kCyBBcf"),
            other => panic!("erwartet HardInvite, war {other:?}"),
        }
    }

    #[test]
    fn harmloser_text_ohne_id_ist_none() {
        assert_eq!(screen("gg wp schönes match", None), CrewSignal::None);
    }

    #[test]
    fn meldungen_folgen_dem_vorgegebenen_wortlaut() {
        let known = format_known_account_alert("skifahrertv", "nani", "ricky");
        assert!(
            known.starts_with(
                "👀 Crew-Guard (Shadow): skifahrertv ist grad in #nani unterwegs und fährt die Ricky-Nummer (ricky)."
            ),
            "war: {known}"
        );
        assert!(known.ends_with("Ich hab nichts getan — nur zur Info."));

        let neu = format_new_account_alert("versuch", "nani", "b, c", 0.83, "komm zu uns");
        assert!(
            neu.starts_with("🆕 Crew-Guard (Shadow): Neuer Account versuch in #nani"),
            "war: {neu}"
        );
        assert!(neu.contains("GPT-Confidence 0.83"));
        assert!(neu.contains("Nachricht: \"komm zu uns\""));
    }

    #[test]
    fn nachricht_wird_auf_160_zeichen_gekuerzt() {
        let long = "x".repeat(400);
        let msg = format_new_account_alert("a", "b", "c", 0.9, &long);
        assert!(msg.contains(&"x".repeat(CONTENT_PREVIEW_MAX)));
        assert!(!msg.contains(&"x".repeat(CONTENT_PREVIEW_MAX + 1)));
    }

    #[test]
    fn verdict_parsing_ist_robust() {
        let raw = "hier kommt: {\"is_crew\":true,\"confidence\":0.9,\"patterns\":[\"b\",\"c\"],\"reasoning\":\"klar\"} ok";
        let verdict = parse_crew_verdict(raw).expect("gültiges Urteil");
        assert!(verdict.is_crew);
        assert_eq!(verdict.confidence, 0.9);
        assert_eq!(verdict.patterns, vec!["b".to_string(), "c".to_string()]);

        // Müll → fail-safe unsure.
        assert_eq!(parse_crew_verdict("kein json"), None);
    }

    #[tokio::test]
    async fn judge_backtest_precision_recall_wenn_konfiguriert() {
        if non_empty_env("OPENAI_API_KEY").is_none() || non_empty_env("CREW_GUARD_MODEL").is_none()
        {
            eprintln!(
                "SKIP judge_backtest_precision_recall_wenn_konfiguriert: OPENAI_API_KEY/CREW_GUARD_MODEL nicht gesetzt"
            );
            return;
        }
        let judge = OpenAiCrewJudge::from_env();

        let mut true_pos = 0usize;
        let mut false_neg = 0usize;
        for text in POSITIVES {
            if judge.judge(text, &[]).await.is_crew {
                true_pos += 1;
            } else {
                false_neg += 1;
            }
        }
        let mut false_pos = 0usize;
        let mut true_neg = 0usize;
        for text in NEGATIVES {
            if judge.judge(text, &[]).await.is_crew {
                false_pos += 1;
            } else {
                true_neg += 1;
            }
        }

        let precision = if true_pos + false_pos == 0 {
            0.0
        } else {
            true_pos as f32 / (true_pos + false_pos) as f32
        };
        let recall = if true_pos + false_neg == 0 {
            0.0
        } else {
            true_pos as f32 / (true_pos + false_neg) as f32
        };
        eprintln!(
            "crew_guard Judge-Backtest (5+5): TP={true_pos} FP={false_pos} FN={false_neg} TN={true_neg} | precision={precision:.2} recall={recall:.2}"
        );
    }

    #[test]
    fn context_identity_bevorzugt_id_sonst_login() {
        assert_eq!(context_identity("12345", "loginx"), "12345");
        assert_eq!(context_identity("", "loginx"), "loginx");
        assert_eq!(context_identity("   ", "loginx"), "loginx");
    }

    #[test]
    fn kontextpuffer_liefert_vorherige_ohne_aktuelle_und_verdraengt() {
        let buf = ChatterContextBuffer::new();
        // Erste Nachricht: kein Vorlauf.
        assert!(buf.snapshot_then_push("nani", "u1", "m1").is_empty());
        // Zweite sieht m1, aber NICHT sich selbst.
        assert_eq!(
            buf.snapshot_then_push("nani", "u1", "m2"),
            vec!["m1".to_string()]
        );
        assert_eq!(
            buf.snapshot_then_push("nani", "u1", "m3"),
            vec!["m1".to_string(), "m2".to_string()]
        );
        // Anderer User im selben Kanal ist getrennt.
        assert!(buf.snapshot_then_push("nani", "u2", "x1").is_empty());
        // Fenster begrenzt: nur die letzten CONTEXT_WINDOW Nachrichten.
        for i in 0..20 {
            buf.snapshot_then_push("nani", "u3", &format!("n{i}"));
        }
        let prev = buf.snapshot_then_push("nani", "u3", "final");
        assert_eq!(prev.len(), CONTEXT_WINDOW);
        assert_eq!(prev.last().map(String::as_str), Some("n19"));
    }

    #[test]
    fn user_prompt_bettet_kontext_als_zitatzeilen_ein() {
        let ctx = vec!["erste zeile".to_string(), "zweite zeile".to_string()];
        let prompt = build_user_content("aktuelle nachricht", &ctx);
        assert!(prompt.contains("> erste zeile"), "war: {prompt}");
        assert!(prompt.contains("> zweite zeile"), "war: {prompt}");
        assert!(prompt.contains("aktuelle nachricht"));
        // Ohne Kontext kein Verlaufsblock.
        let bare = build_user_content("nur eine", &[]);
        assert!(!bare.contains('>'), "war: {bare}");
        assert!(bare.contains("nur eine"));
    }

    fn precision_recall(true_pos: usize, false_pos: usize, false_neg: usize) -> (f32, f32) {
        let precision = if true_pos + false_pos == 0 {
            0.0
        } else {
            true_pos as f32 / (true_pos + false_pos) as f32
        };
        let recall = if true_pos + false_neg == 0 {
            0.0
        } else {
            true_pos as f32 / (true_pos + false_neg) as f32
        };
        (precision, recall)
    }

    /// Echter DB-Backtest gegen `twitch_chat_messages` (NUR lesend, keine
    /// Writes). Gated auf TB_TEST_DATABASE_URL + OPENAI_API_KEY + CREW_GUARD_MODEL
    /// und `#[ignore]` — läuft nur auf explizite Anforderung mit DSN+Key. Er
    /// misst zwei Dinge: (A) das End-to-End-System (screen + Judge mit Kontext)
    /// und (B) die EHRLICHE Judge-Recall auf realem Kampagnentext, indem der
    /// deterministische HardId-Kurzschluss bewusst umgangen wird.
    #[tokio::test]
    #[ignore = "DB-Backtest: braucht TB_TEST_DATABASE_URL + OPENAI_API_KEY + CREW_GUARD_MODEL; nur lesend"]
    async fn crew_guard_db_backtest_realdaten() {
        let (Some(dsn), Some(_), Some(_)) = (
            non_empty_env("TB_TEST_DATABASE_URL"),
            non_empty_env("OPENAI_API_KEY"),
            non_empty_env("CREW_GUARD_MODEL"),
        ) else {
            eprintln!(
                "SKIP crew_guard_db_backtest_realdaten: TB_TEST_DATABASE_URL/OPENAI_API_KEY/CREW_GUARD_MODEL nicht gesetzt"
            );
            return;
        };

        let pool = match sqlx::postgres::PgPoolOptions::new()
            .max_connections(2)
            .connect(&dsn)
            .await
        {
            Ok(pool) => pool,
            Err(err) => {
                eprintln!(
                    "SKIP crew_guard_db_backtest_realdaten: DB-Connect fehlgeschlagen: {err}"
                );
                return;
            }
        };

        // Die 5 Crew-Logins (lowercase) als Bind-Parameter.
        let crew_logins: Vec<String> = CREW_REGISTRY
            .iter()
            .map(|acc| acc.login.to_string())
            .collect();

        // Positiva: echte Nachrichten der Crew-Logins, je User chronologisch.
        let positives = match sqlx::query_as::<_, (String, String, String, String)>(
            "SELECT streamer_login, COALESCE(chatter_login, ''), COALESCE(chatter_id, ''), content \
             FROM twitch_chat_messages \
             WHERE lower(chatter_login) = ANY($1) \
               AND content IS NOT NULL AND length(btrim(content)) > 0 \
             ORDER BY chatter_login, message_ts",
        )
        .bind(crew_logins.clone())
        .fetch_all(&pool)
        .await
        {
            Ok(rows) => rows,
            Err(err) => {
                eprintln!("crew_guard_db_backtest_realdaten: Positiv-Query fehlgeschlagen: {err}");
                return;
            }
        };

        // Negativa: Zufallsstichprobe sonstiger Nachrichten (KEINE Crew), LIMIT 500.
        let negatives = match sqlx::query_as::<_, (String, String, String, String)>(
            "SELECT streamer_login, COALESCE(chatter_login, ''), COALESCE(chatter_id, ''), content \
             FROM twitch_chat_messages \
             WHERE (chatter_login IS NULL OR lower(chatter_login) <> ALL($1)) \
               AND content IS NOT NULL AND length(btrim(content)) > 0 \
             ORDER BY random() LIMIT 500",
        )
        .bind(crew_logins.clone())
        .fetch_all(&pool)
        .await
        {
            Ok(rows) => rows,
            Err(err) => {
                eprintln!("crew_guard_db_backtest_realdaten: Negativ-Query fehlgeschlagen: {err}");
                return;
            }
        };

        eprintln!(
            "crew_guard_db_backtest_realdaten: {} Crew-Nachrichten, {} Negativ-Stichprobe",
            positives.len(),
            negatives.len()
        );

        let judge = OpenAiCrewJudge::from_env();
        let threshold = JUDGE_CONFIDENCE_THRESHOLD;

        // ---- Metrik A: End-to-End (screen() + Judge mit Kontext) ----
        let ctx_pos = ChatterContextBuffer::new();
        let mut a_tp = 0usize;
        let mut a_false_neg = 0usize;
        for (streamer, login, chatter_id, content) in &positives {
            let channel = streamer.to_lowercase();
            let identity = context_identity(chatter_id, login);
            let prev = ctx_pos.snapshot_then_push(&channel, identity, content);
            if decide(
                content, chatter_id, &channel, login, threshold, &judge, &prev,
            )
            .await
            .is_some()
            {
                a_tp += 1;
            } else {
                a_false_neg += 1;
            }
        }
        let ctx_neg = ChatterContextBuffer::new();
        let mut a_fp = 0usize;
        let mut a_tn = 0usize;
        for (streamer, login, chatter_id, content) in &negatives {
            let channel = streamer.to_lowercase();
            let identity = context_identity(chatter_id, login);
            let prev = ctx_neg.snapshot_then_push(&channel, identity, content);
            if decide(
                content, chatter_id, &channel, login, threshold, &judge, &prev,
            )
            .await
            .is_some()
            {
                a_fp += 1;
            } else {
                a_tn += 1;
            }
        }
        let (a_precision, a_recall) = precision_recall(a_tp, a_fp, a_false_neg);
        eprintln!(
            "Metrik A (End-to-End screen+judge+Kontext): TP={a_tp} FP={a_fp} FN={a_false_neg} TN={a_tn} | precision={a_precision:.2} recall={a_recall:.2}"
        );

        // ---- Metrik B: ehrliche Judge-Recall auf realem Kampagnentext ----
        // HardId-Kurzschluss bewusst umgangen: Judge DIREKT auf die Crew-Texte,
        // mit den vorherigen Nachrichten desselben Users als Kontext.
        let ctx_b_pos = ChatterContextBuffer::new();
        let mut b_tp = 0usize;
        let mut b_false_neg = 0usize;
        for (streamer, login, chatter_id, content) in &positives {
            let channel = streamer.to_lowercase();
            let identity = context_identity(chatter_id, login);
            let prev = ctx_b_pos.snapshot_then_push(&channel, identity, content);
            if judge.judge(content, &prev).await.is_crew {
                b_tp += 1;
            } else {
                b_false_neg += 1;
            }
        }
        let ctx_b_neg = ChatterContextBuffer::new();
        let mut b_fp = 0usize;
        let mut b_tn = 0usize;
        for (streamer, login, chatter_id, content) in &negatives {
            let channel = streamer.to_lowercase();
            let identity = context_identity(chatter_id, login);
            let prev = ctx_b_neg.snapshot_then_push(&channel, identity, content);
            if judge.judge(content, &prev).await.is_crew {
                b_fp += 1;
            } else {
                b_tn += 1;
            }
        }
        let (b_precision, b_recall) = precision_recall(b_tp, b_fp, b_false_neg);
        eprintln!(
            "Metrik B (Judge direkt, HardId umgangen): TP={b_tp} FP={b_fp} FN={b_false_neg} TN={b_tn} | precision={b_precision:.2} recall={b_recall:.2}"
        );

        pool.close().await;
    }
}
