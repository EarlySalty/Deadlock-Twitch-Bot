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

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use async_trait::async_trait;
use regex::Regex;
use serde::Deserialize;
use tracing::{debug, warn};

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
}

impl CrewVerdict {
    /// Fail-safe: nichts erkannt, keine Aktion.
    pub fn unsure() -> Self {
        Self {
            is_crew: false,
            confidence: 0.0,
            patterns: Vec::new(),
            reasoning: String::new(),
        }
    }
}

#[async_trait]
pub trait CrewJudge: Send + Sync {
    async fn judge(&self, content: &str, recent_context: &[String]) -> CrewVerdict;
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
                debug!("crew_guard: Judge-HTTP fehlgeschlagen: {err}");
                return CrewVerdict::unsure();
            }
        };
        if !resp.status().is_success() {
            debug!("crew_guard: Judge-HTTP {}", resp.status());
            return CrewVerdict::unsure();
        }
        let parsed = match resp.json::<ChatCompletion>().await {
            Ok(parsed) => parsed,
            Err(err) => {
                debug!("crew_guard: Judge-Antwort nicht lesbar: {err}");
                return CrewVerdict::unsure();
            }
        };
        let raw = parsed
            .choices
            .into_iter()
            .next()
            .and_then(|choice| choice.message.content)
            .unwrap_or_default();
        parse_crew_verdict(&raw)
    }
}

fn build_user_content(content: &str, recent_context: &[String]) -> String {
    if recent_context.is_empty() {
        format!("Zu pruefende Nachricht:\n{content}")
    } else {
        let ctx = recent_context.join("\n");
        format!("Kontext (vorherige Nachrichten):\n{ctx}\n\nZu pruefende Nachricht:\n{content}")
    }
}

/// Robustes Bergen des JSON-Urteils (Stil wie `conversation_scam::parse_verdict`).
fn parse_crew_verdict(raw: &str) -> CrewVerdict {
    let parsed = serde_json::from_str::<RawCrewVerdict>(raw.trim()).or_else(|_| {
        extract_json_object(raw)
            .ok_or_else(|| serde_json::Error::io(std::io::Error::other("kein JSON-Objekt")))
            .and_then(serde_json::from_str::<RawCrewVerdict>)
    });
    let Ok(parsed) = parsed else {
        return CrewVerdict::unsure();
    };
    if !parsed.confidence.is_finite() {
        return CrewVerdict::unsure();
    }
    CrewVerdict {
        is_crew: parsed.is_crew,
        confidence: parsed.confidence.clamp(0.0, 1.0),
        patterns: parsed.patterns,
        reasoning: parsed.reasoning,
    }
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
}

impl CrewGuard {
    pub fn new(enabled: bool, judge: Arc<dyn CrewJudge>, alerter: Arc<ModAlerter>) -> Self {
        Self {
            enabled,
            threshold: JUDGE_CONFIDENCE_THRESHOLD,
            judge,
            alerter,
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
                &alerter,
            )
            .await;
        });
    }
}

/// Kern der Shadow-Auswertung: screenen, ggf. Judge fragen, ggf. melden.
/// Meldet ausschliesslich nach Discord — nie ein Ban/Chat-Post/Whisper.
async fn evaluate(
    content: &str,
    chatter_id: &str,
    channel: &str,
    login: &str,
    threshold: f32,
    judge: &dyn CrewJudge,
    alerter: &Arc<ModAlerter>,
) {
    match screen(content, Some(chatter_id)) {
        CrewSignal::HardId {
            login: registry_login,
            has_evidence,
        } => {
            let patterns = hard_id_patterns(content, has_evidence);
            alerter.send_crew_campaign(format_known_account_alert(
                registry_login,
                channel,
                &patterns,
            ));
        }
        CrewSignal::HardInvite { code } => {
            let patterns = format!("Rival-Invite {code}");
            alerter.send_crew_campaign(format_known_account_alert(login, channel, &patterns));
        }
        CrewSignal::Trigger { hits } => {
            let verdict = judge.judge(content, &[]).await;
            if verdict.is_crew && verdict.confidence >= threshold {
                let patterns = if verdict.patterns.is_empty() {
                    hits.join(", ")
                } else {
                    verdict.patterns.join(", ")
                };
                alerter.send_crew_campaign(format_new_account_alert(
                    login,
                    channel,
                    &patterns,
                    verdict.confidence,
                    content,
                ));
            }
        }
        CrewSignal::None => {}
    }
}

// ---------------------------------------------------------------------------
// Backtest — Vertrauen vor Live (statische Fixtures, kein Netz/DB)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
        let verdict = parse_crew_verdict(raw);
        assert!(verdict.is_crew);
        assert_eq!(verdict.confidence, 0.9);
        assert_eq!(verdict.patterns, vec!["b".to_string(), "c".to_string()]);

        // Müll → fail-safe unsure.
        assert_eq!(parse_crew_verdict("kein json"), CrewVerdict::unsure());
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
}
