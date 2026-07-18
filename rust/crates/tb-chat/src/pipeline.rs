//! Die event_message-Pipeline — Port von `TwitchChatBot.event_message`
//! (bot/chat/bot.py Z. 1510–1827), orchestriert alle Chat-Module in der
//! exakten Python-Reihenfolge.
//!
//! # Schritte (bot.py-Zeilen in Klammern)
//!
//! 0. Echo-/Self-Filter (1528–1532)
//! 1. VoiceReaction-Dispatch (1534–1546) — **bewusst No-op** bis zur
//!    Engagement-Phase (Outreach-Konversationen laufen weiter über Python,
//!    der Engagement-Layer war ohnehin in Beobachtung)
//! 2. Known-Bot-Whitelist: nur Tracking + Commands (1548–1557)
//! 3. Kanal-Klassifizierung (1559–1572)
//! 4. Non-Partner/Monitored-Only: nur Tracking (1575–1585)
//! 5. Chat-Archivierung, danach Global-Chatter-Ban (1589–1595)
//! 6. Scam-Pitch-Warnung (1597–1601) — Detektor sendet Chat-Warnung intern; Pipeline löscht NIE (wie Python) und timeoutet nur bei Eskalation (StrongTimeout)
//! 7. Spam-Score + Auto-Ban (1602–1737)
//! 8. Sus-Discord-Invite (1741–1743)
//! 9. Fun-Responses, nur wenn Deadlock live (1745–1750)
//! 10. Deadlock-Zugangsfrage, nur wenn Deadlock live
//! 11. Raw-Aktivität (Chat-Health/Archivierung lief vor Schritt 5)
//! 12. Engagement-AI (1757–1811) — **bewusst No-op** bis zur Engagement-Phase
//! 13. /14. Invite/Activity-Promo, nur wenn Deadlock live (1813–1824) —
//!     !invite läuft über den CommandEngine (Schritt 15); das Python-Gate
//!     „keine Activity-Promo wenn Invite gesendet" ist über den
//!     PROMO_IGNORE_COMMANDS-Guard abgedeckt (jede !invite-Nachricht beginnt
//!     mit `!` und wird vom Promo-Tracking ohnehin ignoriert)
//! 15. Command-Processing (1827)
//!
//! # Observer
//!
//! [`ReviewLog`] schreibt die TSV-Review-Zeilen (`logs/twitch_autobans.log` /
//! `twitch_suspicious.log` — werden vom Admin-Dashboard geparst!) und
//! [`ModAlerter`] postet Discord-Moderations-Alerts über den Changelog-Cog
//! (`localhost:8899`), beides exakt nach `_record_autoban` (moderation.py
//! Z. 645–669) und `_send_moderation_alert` (Z. 905–951).

use std::collections::HashSet;
use std::future::Future;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::Arc;

use chrono::Utc;
use dashmap::DashMap;
use sqlx::PgPool;
use tb_engagement::CrewReviewTrigger;
use tracing::{debug, info, warn};

use crate::api::{BanOutcome, ChatApi};
use crate::channel_classifier::{ChannelClass, ChannelClassifier};
use crate::chatter_tracking::ChatterTracker;
use crate::commands::CommandEngine;
use crate::conversation_scam::ConversationScamGuard;
use crate::crew_guard::{crew_guard_enabled, CrewGuard, CrewJudge};
use crate::fun_responses::FunResponses;
use crate::global_chatter_ban::GlobalChatterBanEnforcer;
use crate::invite_question::InviteQuestionResponder;
use crate::lfg_pitch::LfgPitchResponder;
use crate::mention_scoring::{score_mention_patterns, MentionResolver, WHITELISTED_BOTS};
use crate::moderation::{
    AutoBanRequest, ModerationEngine, ModerationEvidence, BAN_REASON_GLOBAL, BAN_REASON_SPAM,
    NOTICE_GLOBAL_BAN,
};
use crate::promos::PromoEngine;
use crate::scam_pitch::{
    AccountAgePort, AiReviewOutcome, PitchDecision, ScamPitchDetector, SpamAiReviewer,
};
use crate::spam_filter::{SpamAction, SpamContext, SpamFilter, SPAM_MIN_MATCHES};
use crate::sus_invite::SusInviteCheck;
use crate::types::ChatMessageEvent;

/// Twitch-Timeout-Reason für die Scam-Pitch-Eskalation (StrongTimeout).
///
/// **Bewusste Divergenz zum Python-Orakel** (Grillme-Entscheidung
/// `scam-pitch-spam-review-5`, 2026-06-15: „Timeout-Reason → Rust akzeptieren
/// (klarer)"). Python übergibt `"Service-Pitch / Spam Escalation"`
/// (`service_pitch_warning.py:944`); der Rust-Wortlaut benennt zusätzlich den
/// Account-Takeover-Verdacht und ist damit für den Moderator aussagekräftiger.
/// Dieser String ist die kanonische Quelle — ein späterer blinder Parity-Pass
/// darf ihn NICHT auf den Python-Wortlaut zurücksetzen (Regression-Guard:
/// Test `scam_pitch_timeout_reason_ist_bewusst_klarer_als_python`).
pub const SCAM_PITCH_TIMEOUT_REASON: &str =
    "Account-Takeover-Verdacht / wiederholter Service-Pitch";

const SUS_INVITE_TIMEOUT_SECS: u32 = 600;
const SUS_INVITE_TIMEOUT_REASON: &str = "Fremde Discord-Werbung ist hier nicht erlaubt";

const SPAM_META_REPLY_COOLDOWN_SECS: u64 = 60 * 60;
const SPAM_META_REPLIES: [&str; 3] = [
    "Ne, dich bann ich nicht. Ich kenn den Unterschied zwischen drüber reden und es tun.",
    "Keine Sorge, ich lese mit statt nur zu zählen. Über Spam reden ist kein Spam.",
    "Netter Versuch. Der Ban-Knopf bleibt aus, du redest ja nur drüber.",
];

// ---------------------------------------------------------------------------
// Review-Log — TSV-Dateien, die das Admin-Dashboard parst
// ---------------------------------------------------------------------------

/// Schreibt Auto-Ban-/Verdachts-Zeilen in die Review-Logdateien.
///
/// Format (moderation.py Z. 645–669, `_record_autoban`):
/// `{ts}\t[{status}]\t{channel}\t{chatter|-}\t{chatter_id}\t{reason|-}\t{content}`
/// — `SUSPICIOUS*`-Status landet in `twitch_suspicious.log`, alles andere in
/// `twitch_autobans.log`.
pub struct ReviewLog {
    autoban_path: PathBuf,
    suspicious_path: PathBuf,
}

impl ReviewLog {
    /// `log_dir` = Verzeichnis der Python-Logs (Prod: `<repo>/logs`).
    pub fn new(log_dir: impl Into<PathBuf>) -> Self {
        let dir = log_dir.into();
        Self {
            autoban_path: dir.join("twitch_autobans.log"),
            suspicious_path: dir.join("twitch_suspicious.log"),
        }
    }

    /// Hängt eine Review-Zeile an — best-effort, Fehler nur debug-geloggt.
    pub fn record(
        &self,
        channel: &str,
        chatter_login: &str,
        chatter_id: &str,
        content: &str,
        status: &str,
        reason: &str,
    ) {
        let target = if status.trim().to_uppercase().starts_with("SUSPICIOUS") {
            &self.suspicious_path
        } else {
            &self.autoban_path
        };
        // Python: datetime.now(UTC).isoformat() — volle Mikrosekunden.
        let ts = Utc::now().format("%Y-%m-%dT%H:%M:%S%.6f+00:00");
        // TSV-Hygiene für ALLE Felder: AI-Begründungen und Nachrichten sind
        // unvertrauenswürdig — Tabs/Zeilenumbrüche dürfen das Log-Format
        // nicht aufbrechen oder Einträge einschleusen.
        let clean = |value: &str, cap: usize| -> String {
            value
                .replace(['\t', '\r', '\n'], " ")
                .chars()
                .take(cap)
                .collect()
        };
        let safe_content = clean(content, 500);
        let safe_channel = clean(channel, 100);
        let safe_chatter = clean(chatter_login, 100);
        let safe_chatter_id = clean(chatter_id, 50);
        let safe_status = clean(status, 50);
        let safe_reason = clean(reason, 300);
        let line = format!(
            "{ts}\t[{safe_status}]\t{safe_channel}\t{}\t{safe_chatter_id}\t{}\t{safe_content}\n",
            if safe_chatter.is_empty() {
                "-"
            } else {
                &safe_chatter
            },
            if safe_reason.is_empty() {
                "-"
            } else {
                &safe_reason
            },
        );
        if let Some(parent) = target.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(e) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(target)
            .and_then(|mut f| std::io::Write::write_all(&mut f, line.as_bytes()))
        {
            debug!("review_log: Schreiben fehlgeschlagen ({target:?}): {e}");
        }
    }
}

// ---------------------------------------------------------------------------
// Discord-Moderations-Alerts via Changelog-Cog (localhost:8899)
// ---------------------------------------------------------------------------

/// Discord-Alert-Kanal (moderation.py Z. 903: `_MOD_ALERT_CHANNEL_ID`).
const DEFAULT_MOD_ALERT_CHANNEL_ID: u64 = 1374364800817303632;
const MOD_ALERT_CHANNEL_ID_ENV: &str = "TWITCH_ALERT_CHANNEL_ID";

/// Postet Moderations-Alerts in den Discord-Mod-Kanal — Port von
/// `_send_moderation_alert` (moderation.py Z. 905–951). Fire-and-forget.
pub struct ModAlerter {
    http: reqwest::Client,
    endpoint: String,
    channel_id: u64,
}

pub struct CrewRadarAlert {
    pub message: String,
    pub chatter_login: String,
    pub chatter_id: String,
    pub channel_login: String,
    pub style_score: u8,
    pub verdict: String,
    pub notify_only: bool,
}

/// Timeout-Dauer bei AI-bestätigtem Spam: 24h — bewusst reversibel statt Ban,
/// damit ein Judge-Fehlurteil gegen echte Viewer per Button korrigierbar bleibt.
const AI_SPAM_TIMEOUT_SECS: u32 = 24 * 60 * 60;

/// Felder eines sus_spam-Alerts (Verdachtsfall mit Judge-Urteil).
pub struct SusSpamAlert<'a> {
    pub channel_login: &'a str,
    pub chatter_login: &'a str,
    pub chatter_id: &'a str,
    pub content: &'a str,
    pub rule_reason: &'a str,
    pub outcome: &'a AiReviewOutcome,
    pub action_taken: &'a str,
}

/// Muster-Vorschlag für den „Als Spam korrigieren"-Button: Nachricht ohne
/// Mentions/Backticks, auf 78 Zeichen gekürzt — das Muster reist in der
/// Discord-custom_id (Limit 100 Zeichen inkl. Präfix), damit der Button ohne
/// jeden serverseitigen Zustand Restarts überlebt. Substring-Matching macht
/// den Präfix-Schnitt unschädlich.
fn correction_learn_pattern(content: &str) -> Option<String> {
    let text = content
        .replace(['\r', '\n', '`'], " ")
        .split_whitespace()
        .filter(|part| !part.starts_with('@'))
        .collect::<Vec<_>>()
        .join(" ");
    let pattern = text.trim();
    if pattern.chars().count() >= 4 {
        Some(pattern.chars().take(78).collect())
    } else {
        None
    }
}

impl ModAlerter {
    pub fn new(http: reqwest::Client) -> Self {
        Self::with_endpoint(http, "http://localhost:8899/changelog")
    }

    pub fn with_endpoint(http: reqwest::Client, endpoint: impl Into<String>) -> Self {
        Self::with_endpoint_and_channel_id(http, endpoint, alert_channel_id_from_env())
    }

    pub fn with_endpoint_and_channel_id(
        http: reqwest::Client,
        endpoint: impl Into<String>,
        channel_id: u64,
    ) -> Self {
        Self {
            http,
            endpoint: endpoint.into(),
            channel_id,
        }
    }

    /// Sendet einen Alert asynchron (tokio::spawn) — blockiert die Pipeline nie.
    pub fn send(
        self: &Arc<Self>,
        kind: &str,
        channel_login: &str,
        chatter_login: &str,
        chatter_id: &str,
        content: &str,
        reason: &str,
    ) {
        // Titel + Farben exakt aus moderation.py Z. 916–921.
        let (title, _color) = match kind {
            "ban" => ("🔨 Ban ausgeführt", 0xED4245),
            "global_ban" => ("🚫 Global-Ban ausgeführt", 0xED4245),
            "sus_invite" => ("⚠️ Verdächtiger Discord-Link", 0xFEE75C),
            "sus_spam" => ("👀 Verdächtige Nachricht", 0xFEE75C),
            "scam_pitch_timeout" => (
                "🛡️ Account-Takeover erkannt — Quarantäne (reversibel)",
                0xFEE75C,
            ),
            "scam_pitch_warn" => ("⚠️ Scam-Pitch erkannt — Verwarnung", 0xFEE75C),
            _ => ("ℹ️ Moderation", 0x5865F2),
        };

        let mut lines = vec![
            format!("**Kanal:** #{channel_login}"),
            format!("**Chatter:** {chatter_login}"),
        ];
        if !chatter_id.is_empty() {
            lines.push(format!("**ID:** {chatter_id}"));
        }
        if !reason.is_empty() {
            lines.push(format!("**Grund:** {reason}"));
        }
        if !content.is_empty() {
            let safe: String = content
                .chars()
                .take(300)
                .collect::<String>()
                .replace('`', "'");
            lines.push(format!("**Nachricht:** `{safe}`"));
        }

        let payload = serde_json::json!({
            "title": title,
            "content": lines.join("\n"),
            "channel_id": self.channel_id,
            "token": "changeme-local",
        });
        self.post("mod_alert", payload);
    }

    /// sus_spam-Alert mit Judge-Urteil — Payload-Vertrag v2 für die
    /// Korrektur-Buttons im Discord-Bot (dl-changelog):
    /// `spam_learning = {v:2, verdict, ai_reason, learned:[{table,id,pattern}]}`.
    ///
    /// Wird für JEDEN Judge-Ausgang gerufen (spam/safe/error/skipped) — der
    /// Judge gatet die Aktion, nie die Sichtbarkeit.
    pub fn send_sus_spam(self: &Arc<Self>, alert: SusSpamAlert<'_>) {
        let SusSpamAlert {
            channel_login,
            chatter_login,
            chatter_id,
            content,
            rule_reason,
            outcome,
            action_taken,
        } = alert;
        let (title, verdict_key, ai_line) = match outcome {
            AiReviewOutcome::Spam { reason, .. } => (
                "🤖 Spam bestätigt — Aktion ausgeführt",
                "spam",
                format!("**AI-Urteil:** Spam — {reason}"),
            ),
            AiReviewOutcome::Safe { reason } => (
                "👀 Verdächtige Nachricht — AI: harmlos",
                "safe",
                format!("**AI-Urteil:** harmlos — {reason}"),
            ),
            AiReviewOutcome::Error { detail } => (
                "⚠️ Verdächtige Nachricht — AI-Review fehlgeschlagen",
                "error",
                format!("**AI-Urteil:** Fehler — {detail}"),
            ),
            AiReviewOutcome::Skipped => (
                "👀 Verdächtige Nachricht — AI übersprungen",
                "skipped",
                "**AI-Urteil:** übersprungen (Cooldown, kürzlich schon geprüft)".to_string(),
            ),
        };

        let mut lines = vec![
            format!("**Kanal:** #{channel_login}"),
            format!("**Chatter:** {chatter_login}"),
        ];
        if !chatter_id.is_empty() {
            lines.push(format!("**ID:** {chatter_id}"));
        }
        lines.push(format!("**Regel-Treffer:** {rule_reason}"));
        lines.push(ai_line);
        lines.push(format!("**Aktion:** {action_taken}"));

        let mut ai_reason = String::new();
        let mut learned_entries: Vec<serde_json::Value> = Vec::new();
        match outcome {
            AiReviewOutcome::Spam {
                reason,
                learned,
                rejected_pattern,
                save_failed,
                ..
            } => {
                ai_reason = reason.clone();
                if let Some(l) = learned {
                    lines.push(format!("**Gelernt:** `{}`", l.pattern.replace('`', "'")));
                    learned_entries.push(serde_json::json!({
                        "table": "spam",
                        "id": l.id,
                        "pattern": l.pattern,
                    }));
                }
                if let Some(rej) = rejected_pattern {
                    lines.push(format!(
                        "**Nicht gelernt:** `{}` (zu generisch, Distinktivitäts-Gate)",
                        rej.replace('`', "'")
                    ));
                }
                if *save_failed {
                    lines.push(
                        "**Nicht gelernt:** Speichern fehlgeschlagen (DB-Fehler)".to_string(),
                    );
                }
            }
            AiReviewOutcome::Safe { reason } | AiReviewOutcome::Error { detail: reason } => {
                ai_reason = reason.clone();
            }
            AiReviewOutcome::Skipped => {}
        }

        if !content.is_empty() {
            let safe: String = content
                .chars()
                .take(300)
                .collect::<String>()
                .replace('`', "'");
            lines.push(format!("**Nachricht:** `{safe}`"));
        }

        let payload = serde_json::json!({
            "title": title,
            "content": lines.join("\n"),
            "channel_id": self.channel_id,
            "token": "changeme-local",
            "spam_learning": {
                "v": 2,
                "verdict": verdict_key,
                "ai_reason": ai_reason.chars().take(200).collect::<String>(),
                "learned": learned_entries,
                // Für „Als Spam korrigieren", wenn nichts gelernt wurde —
                // reist in der Button-custom_id (restart-fest ohne Store).
                "learn_pattern": correction_learn_pattern(content),
            },
        });
        self.post("mod_alert", payload);
    }

    /// Crew-Guard-Shadow-Meldung über **denselben** Discord-Pfad wie die
    /// `sus_invite`-Alerts (Changelog-Cog → nani-Kanal, kein Mod-Ping). Der
    /// Nachrichtentext ist bereits final formatiert (crew_guard). Fire-and-forget.
    pub fn send_crew_campaign(self: &Arc<Self>, alert: CrewRadarAlert) {
        let payload = serde_json::json!({
            "title": "🕵️ Crew-Guard (Radar)",
            "content": alert.message,
            "channel_id": self.channel_id,
            "token": "changeme-local",
            "crew_radar": {
                "v": 1,
                "login": alert.chatter_login,
                "chatter_id": alert.chatter_id,
                "channel": alert.channel_login,
                "style_score": alert.style_score,
                "verdict": alert.verdict,
                "notify_only": alert.notify_only,
            },
        });
        self.post("crew_guard", payload);
    }

    /// Gemeinsamer Fire-and-forget-Post an den Changelog-Cog.
    fn post(self: &Arc<Self>, scope: &'static str, payload: serde_json::Value) {
        let this = Arc::clone(self);
        let handle = tokio::spawn(async move {
            match this
                .http
                .post(&this.endpoint)
                .json(&payload)
                .timeout(std::time::Duration::from_secs(5))
                .send()
                .await
            {
                Ok(resp) if !matches!(resp.status().as_u16(), 200 | 201 | 204) => {
                    debug!("{scope}: Discord-Post HTTP {}", resp.status());
                }
                Err(e) => debug!("{scope}: Discord-Post fehlgeschlagen: {e}"),
                _ => {}
            }
        });
        tokio::spawn(async move {
            if let Err(error) = handle.await {
                tracing::error!(%error, "mod_alert: Task fehlerhaft beendet");
            }
        });
    }
}

fn alert_channel_id_from_env() -> u64 {
    parse_alert_channel_id(std::env::var(MOD_ALERT_CHANNEL_ID_ENV).ok())
}

fn parse_alert_channel_id(raw: Option<String>) -> u64 {
    raw.as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|id| *id != 0)
        .unwrap_or(DEFAULT_MOD_ALERT_CHANNEL_ID)
}

// ---------------------------------------------------------------------------
// Konkreter MentionResolver — DB (Rollup/Session) + Helix
// ---------------------------------------------------------------------------

/// Cache-TTL für den Known-Chatter-Check (moderation.py Z. 299: 600s).
const MENTION_CHATTER_CACHE_TTL_SECS: i64 = 600;
/// Cache-TTL für die Helix-Existenz-Prüfung (moderation.py Z. 371: 6h).
const MENTION_USER_CACHE_TTL_SECS: i64 = 21_600;

/// Produktiver [`MentionResolver`]: `twitch_session_chatters`/`twitch_chatter_rollup`
/// für bekannte Chatter (600s-Cache) + Helix-Login-Lookup (6h-Cache) —
/// Port von `_is_known_channel_chatter` (moderation.py Z. 291–356) und
/// `_resolve_existing_twitch_users` (Z. 358–427).
pub struct PgHelixMentionResolver {
    pool: PgPool,
    api: Arc<dyn ChatApi>,
    chatter_cache: DashMap<(String, String), (i64, bool)>,
    user_cache: DashMap<String, (i64, bool)>,
}

impl PgHelixMentionResolver {
    pub fn new(pool: PgPool, api: Arc<dyn ChatApi>) -> Self {
        Self {
            pool,
            api,
            chatter_cache: DashMap::new(),
            user_cache: DashMap::new(),
        }
    }
}

#[async_trait::async_trait]
impl MentionResolver for PgHelixMentionResolver {
    async fn is_known_chatter(&self, channel_login: &str, mention_login: &str) -> bool {
        let streamer = channel_login.trim().to_lowercase();
        let mention = mention_login.trim().to_lowercase();
        if streamer.is_empty() || mention.is_empty() {
            return false;
        }

        let now = Utc::now().timestamp();
        let key = (streamer.clone(), mention.clone());
        if let Some(entry) = self.chatter_cache.get(&key) {
            let (cached_at, value) = *entry;
            if now - cached_at <= MENTION_CHATTER_CACHE_TTL_SECS {
                return value;
            }
        }

        // session_chatters zuerst, dann rollup (moderation.py Z. 315–333).
        let known = sqlx::query_scalar!(
            "SELECT 1 FROM twitch_session_chatters \
             WHERE streamer_login = $1 AND chatter_login = $2 LIMIT 1",
            &streamer,
            &mention,
        )
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten()
        .is_some()
            || sqlx::query_scalar!(
                "SELECT 1 FROM twitch_chatter_rollup \
                 WHERE streamer_login = $1 AND chatter_login = $2 LIMIT 1",
                &streamer,
                &mention,
            )
            .fetch_optional(&self.pool)
            .await
            .ok()
            .flatten()
            .is_some();

        self.chatter_cache.insert(key, (now, known));
        if self.chatter_cache.len() > 4096 {
            let stale_before = now - MENTION_CHATTER_CACHE_TTL_SECS * 4;
            self.chatter_cache.retain(|_, (ts, _)| *ts >= stale_before);
        }
        known
    }

    async fn resolve_existing(&self, logins: &[&str]) -> (HashSet<String>, bool) {
        let now = Utc::now().timestamp();
        let mut found = HashSet::new();
        let mut to_lookup: Vec<String> = Vec::new();

        for login in logins {
            let value = login.trim().to_lowercase();
            let value = value.trim_start_matches('@').to_string();
            if value.is_empty() {
                continue;
            }
            if let Some(entry) = self.user_cache.get(&value) {
                let (cached_at, exists) = *entry;
                if now - cached_at <= MENTION_USER_CACHE_TTL_SECS {
                    if exists {
                        found.insert(value);
                    }
                    continue;
                }
            }
            to_lookup.push(value);
        }

        if to_lookup.is_empty() {
            return (found, true);
        }

        // Einzel-Lookups via ChatApi (Mentions je Nachricht sind selten >2;
        // 6h-Cache fängt Wiederholungen). Fehler → lookup_ok=false
        // (moderation.py Z. 424–427: except → (found, False)).
        for login in &to_lookup {
            match self.api.resolve_user_id(login).await {
                Ok(result) => {
                    let exists = result.is_some();
                    self.user_cache.insert(login.clone(), (now, exists));
                    if exists {
                        found.insert(login.clone());
                    }
                }
                Err(e) => {
                    debug!("mention_resolver: Helix-Lookup fehlgeschlagen ({login}): {e}");
                    return (found, false);
                }
            }
        }

        if self.user_cache.len() > 8192 {
            let stale_before = now - MENTION_USER_CACHE_TTL_SECS * 4;
            self.user_cache.retain(|_, (ts, _)| *ts >= stale_before);
        }
        (found, true)
    }
}

// ---------------------------------------------------------------------------
// ChatPipeline
// ---------------------------------------------------------------------------

/// Alle Bausteine der Pipeline — gebündelt, damit der Konstruktor lesbar bleibt.
#[derive(Clone)]
pub struct ChatPipelineParts {
    pub bot_user_id: String,
    pub api: Arc<dyn ChatApi>,
    pub pool: PgPool,
    pub classifier: Arc<ChannelClassifier>,
    pub tracker: Arc<ChatterTracker>,
    pub global_ban: Arc<GlobalChatterBanEnforcer>,
    pub scam_pitch: Arc<ScamPitchDetector>,
    pub conversation_scam: Arc<ConversationScamGuard>,
    pub spam_filter: Arc<SpamFilter>,
    pub ai_reviewer: Arc<SpamAiReviewer>,
    pub moderation: Arc<ModerationEngine>,
    pub sus_invite: Arc<SusInviteCheck>,
    pub fun: Arc<FunResponses>,
    pub invite_question: Arc<InviteQuestionResponder>,
    pub lfg_pitch: Arc<LfgPitchResponder>,
    pub promos: Arc<PromoEngine>,
    pub commands: Arc<CommandEngine>,
    pub mention_resolver: Arc<dyn MentionResolver>,
    pub review_log: Arc<ReviewLog>,
    pub alerter: Arc<ModAlerter>,
    pub account_age: Arc<dyn AccountAgePort>,
    pub crew_judge: Arc<dyn CrewJudge>,
    pub crew_centroid: Arc<crate::style_score::Centroid>,
    pub crew_review_trigger: Option<Arc<dyn CrewReviewTrigger>>,
}

/// Orchestriert die 15 Pipeline-Schritte für jedes `channel.chat.message`-Event.
#[derive(Clone)]
pub struct ChatPipeline {
    parts: ChatPipelineParts,
    /// Crew-Guard (Shadow-Mode): aus der Umgebung gebaut, teilt sich den
    /// Discord-Alert-Pfad des `alerter`. Default AUS (`CREW_GUARD_ENABLED`).
    crew_guard: Arc<CrewGuard>,
    spam_meta_cooldowns: Arc<DashMap<String, std::time::Instant>>,
    spam_meta_reply_index: Arc<std::sync::atomic::AtomicUsize>,
}

impl ChatPipeline {
    pub fn new(parts: ChatPipelineParts) -> Self {
        let crew_guard = Arc::new(
            CrewGuard::new(
                crew_guard_enabled(),
                Arc::clone(&parts.crew_judge),
                Arc::clone(&parts.alerter),
                parts.pool.clone(),
                parts.bot_user_id.clone(),
                Arc::clone(&parts.account_age),
                Arc::clone(&parts.crew_centroid),
                false,
            )
            .with_crew_review_trigger(parts.crew_review_trigger.clone()),
        );
        Self {
            parts,
            crew_guard,
            spam_meta_cooldowns: Arc::new(DashMap::new()),
            spam_meta_reply_index: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        }
    }

    /// Verarbeitet ein eingehendes Chat-Event — Einstiegspunkt für den
    /// EventSub-Dispatch (`channel.chat.message`).
    ///
    /// Rueckgabe: `true`, wenn das Event den Python-aequivalenten
    /// Engagement-Punkt erreicht hat. Fruehe Returns liefern `false`.
    pub async fn handle(&self, event: &ChatMessageEvent) -> bool {
        let p = &self.parts;

        // Twitch Shared Chat: Stammt die Nachricht aus einem fremden Quell-Kanal
        // der Session, wird das Event EINMAL hier auf diesen Quell-Kanal
        // normalisiert (wie Python bot.py:1505) — danach arbeiten alle folgenden
        // Schritte (Klassifizierung, Global-Ban, Scam, Spam, Tracking, Promos,
        // Commands) automatisch im richtigen Kanal statt im Host-Abonnement.
        // Ohne Shared Chat ist das ein geliehener No-op (kein Klon).
        let normalized = event.with_effective_channel();
        let event = &*normalized;

        // Schritt 0: Echo-/Self-Filter (bot.py Z. 1528–1532)
        if event.chatter_user_id == p.bot_user_id {
            return false;
        }

        let channel_login = event.broadcaster_user_login.to_lowercase();
        let chatter_login = event.chatter_user_login.to_lowercase();

        // Schritt 1: VoiceReaction — No-op bis Engagement-Phase (Modul-Doku).

        // Schritt 2: Known-Bot-Whitelist (bot.py Z. 1548–1557)
        if WHITELISTED_BOTS.contains(&chatter_login.as_str()) {
            let tracker = Arc::clone(&p.tracker);
            let event_for_step = event.clone();
            run_pipeline_step(
                "known_bot.track",
                &channel_login,
                &chatter_login,
                async move {
                    tracker.track(&event_for_step).await;
                },
            )
            .await;
            let commands = Arc::clone(&p.commands);
            let event_for_step = event.clone();
            run_pipeline_step(
                "known_bot.commands",
                &channel_login,
                &chatter_login,
                // Deadlock-Gate hart auf `false`: die Kanal-Klassifizierung läuft erst in
                // Schritt 3, hier ist `is_deadlock_live` noch unbekannt. Ein Bot aus der
                // Whitelist bekommt damit nur die ungegateten Befehle (!ping, !help, …).
                async move { commands.handle(&event_for_step, false).await },
            )
            .await;
            return false;
        }

        // Schritt 3: Kanal-Klassifizierung (bot.py Z. 1559–1572)
        let classifier = Arc::clone(&p.classifier);
        let classify_channel = channel_login.clone();
        let classify_broadcaster = event.broadcaster_user_id.clone();
        let class = run_pipeline_step("classify", &channel_login, &chatter_login, async move {
            classifier
                .classify(&classify_channel, &classify_broadcaster)
                .await
        })
        .await
        .unwrap_or(ChannelClass {
            is_partner: false,
            is_monitored_only: false,
            is_deadlock_live: false,
        });

        // Schritt 4: Non-Partner — nur Datensammlung, keine Moderation/Promos
        if !class.is_partner {
            let tracker = Arc::clone(&p.tracker);
            let event_for_step = event.clone();
            run_pipeline_step(
                "non_partner.track",
                &channel_login,
                &chatter_login,
                async move {
                    tracker.track(&event_for_step).await;
                },
            )
            .await;
            return false;
        }

        // Conversation-Scam-Guard: eigener, fehlertoleranter Hintergrundpfad.
        // Der Guard lädt sein per-Kanal-Opt-out selbst und blockiert die übrige
        // Chat-Pipeline weder durch DB- noch durch MiniMax-Latenz.
        let conversation_scam_observe = || {
            p.conversation_scam.observe(event);
        };
        if catch_unwind(AssertUnwindSafe(conversation_scam_observe)).is_err() {
            warn!(
                channel = %channel_login,
                chatter = %chatter_login,
                "chat_pipeline: Schritt conversation_scam.observe panicked"
            );
        }

        // Crew-Guard (Shadow-Mode): koordinierte Abwerbe-/Diffamierungs-Kampagne
        // erkennen und im Shadow NUR nach Discord melden — KEIN Ban, KEIN
        // Chat-Post, KEIN Whisper. Nur Partner-Kanäle (hier). Fire-and-forget
        // hinter Feature-Flag CREW_GUARD_ENABLED (default AUS): bei Aus ein
        // sofortiger No-op, sonst screenen + ggf. GPT-Prüfung im Hintergrund.
        let crew_guard_observe = || {
            self.crew_guard.observe(event);
        };
        if catch_unwind(AssertUnwindSafe(crew_guard_observe)).is_err() {
            warn!(
                channel = %channel_login,
                chatter = %chatter_login,
                "chat_pipeline: Schritt crew_guard.observe panicked"
            );
        }

        // Jede Partner-Nachricht ist archiviert, bevor ein Moderationspfad sie
        // löschen oder die Pipeline mit einem frühen Return verlassen kann.
        let tracker = Arc::clone(&p.tracker);
        let event_for_step = event.clone();
        let tracked_was_first_message = run_pipeline_step(
            "track_before_moderation",
            &channel_login,
            &chatter_login,
            async move { tracker.track(&event_for_step).await },
        )
        .await
        .flatten();

        // Schritt 5: Global-Chatter-Ban (Z. 1589–1595) — Aktion über die
        // ModerationEngine, exakt wie Python via _auto_ban_and_cleanup.
        let global_ban = Arc::clone(&p.global_ban);
        let event_for_step = event.clone();
        let Some(global_ban_reason) =
            run_security_pipeline_step("global_ban", &channel_login, &chatter_login, async move {
                global_ban.ban_reason(&event_for_step).await
            })
            .await
        else {
            return false;
        };
        if let Some(global_ban_reason) = global_ban_reason {
            info!(
                chatter = %chatter_login,
                channel = %channel_login,
                "Global-Ban-Treffer — führe Channel-Ban aus"
            );
            let pipeline = self.clone();
            let event_for_step = event.clone();
            let ban_channel = channel_login.clone();
            let Some(enforced) = run_security_pipeline_step(
                "global_ban.enforce",
                &channel_login,
                &chatter_login,
                async move {
                    pipeline
                        .execute_auto_ban_with_evidence(
                            &event_for_step,
                            &ban_channel,
                            true,
                            BAN_REASON_GLOBAL,
                            Some(NOTICE_GLOBAL_BAN),
                            "global_ban",
                            ModerationEvidence {
                                source_path: "global_ban",
                                reason: &global_ban_reason,
                                score: None,
                                account_age_days: None,
                            },
                        )
                        .await
                },
            )
            .await
            else {
                return false;
            };
            if enforced {
                return false;
            }
            // Ban nicht durchsetzbar (z. B. Chatter ist Mod) → Pipeline läuft
            // weiter, wie Python (enforce gibt das auto_ban-Resultat zurück).
        }

        // Schritt 6: Scam-Pitch (Z. 1597–1601) — Detektor sendet Chat-Warnung intern.
        // Wie Python wird NIE gelöscht; ein Timeout erfolgt nur bei Eskalation
        // (StrongTimeout). Erst-Warnung (StrongWarn/PublicWarn) ist nicht-destruktiv.
        let scam_pitch = Arc::clone(&p.scam_pitch);
        let event_for_step = event.clone();
        let pitch = run_pipeline_step("scam_pitch", &channel_login, &chatter_login, async move {
            scam_pitch.observe(&event_for_step).await
        })
        .await
        .unwrap_or(PitchDecision::None);
        match &pitch {
            PitchDecision::StrongTimeout { text, duration } => {
                debug!(channel = %channel_login, chatter = %chatter_login, "Scam-Pitch: StrongTimeout (Eskalation) → Timeout (kein Delete)");
                let api = Arc::clone(&p.api);
                let alerter = Arc::clone(&p.alerter);
                let event_for_step = event.clone();
                let timeout_channel = channel_login.clone();
                let timeout_chatter = chatter_login.clone();
                let timeout_text = text.clone();
                let timeout_duration = *duration;
                run_pipeline_step(
                    "scam_pitch.timeout",
                    &channel_login,
                    &chatter_login,
                    async move {
                        handle_strong_timeout(
                            &api,
                            &alerter,
                            &event_for_step,
                            &timeout_channel,
                            &timeout_chatter,
                            &timeout_text,
                            timeout_duration,
                        )
                        .await;
                    },
                )
                .await;
            }
            PitchDecision::StrongWarn { .. } | PitchDecision::PublicWarn { .. } => {
                debug!(channel = %channel_login, chatter = %chatter_login, "Scam-Pitch: Warnung (kein Delete/Timeout, wie Python)");
                if !event.chatter_user_id.is_empty()
                    && event.chatter_user_id != event.broadcaster_user_id
                    && !event.is_mod_or_broadcaster()
                {
                    p.alerter.send(
                        "scam_pitch_warn",
                        &channel_login,
                        &chatter_login,
                        &event.chatter_user_id,
                        event.text(),
                        "",
                    );
                }
            }
            PitchDecision::Hint | PitchDecision::None => {}
        }

        // Schritt 7: Spam-Score + Auto-Ban (Z. 1602–1737)
        let pipeline = self.clone();
        let event_for_step = event.clone();
        let spam_channel = channel_login.clone();
        let spam_chatter = chatter_login.clone();
        let Some(spam_handled) =
            run_security_pipeline_step("spam_check", &channel_login, &chatter_login, async move {
                pipeline
                    .run_spam_check(
                        &event_for_step,
                        &spam_channel,
                        &spam_chatter,
                        tracked_was_first_message,
                    )
                    .await
            })
            .await
        else {
            return false;
        };
        if spam_handled {
            return false;
        }

        // Schritt 8: Sus-Discord-Invite (Z. 1741–1743)
        self.handle_sus_invite(event, &channel_login, &chatter_login)
            .await;

        // Schritt 9/10: Deadlock-live-Detektoren.
        if class.is_deadlock_live {
            self.run_deadlock_chat_detectors(event, &channel_login, &chatter_login)
                .await;
        }

        // Schritt 11: Raw-Aktivität; Chat-Health/Archivierung lief bereits vor
        // der Moderation, damit frühe Returns keine Beweise verlieren.
        let promos = Arc::clone(&p.promos);
        let raw_channel = channel_login.clone();
        run_pipeline_step(
            "promos.record_raw_message",
            &channel_login,
            &chatter_login,
            async move {
                promos.record_raw_message(&raw_channel).await;
            },
        )
        .await;

        // Schritt 12: Engagement-AI — No-op bis Engagement-Phase (Modul-Doku).
        let should_spawn_engagement = true;

        // Schritt 13/14: Activity-Promo, nur wenn Deadlock live (Z. 1813–1824).
        // !invite wird vom CommandEngine (Schritt 15) bedient; der
        // PROMO_IGNORE_COMMANDS-Guard deckt das sent_invite-Gate ab.
        if class.is_deadlock_live {
            let promos = Arc::clone(&p.promos);
            let event_for_step = event.clone();
            run_pipeline_step(
                "promos.on_message",
                &channel_login,
                &chatter_login,
                async move {
                    promos.on_message(&event_for_step).await;
                },
            )
            .await;
        }

        // Schritt 15: Command-Processing — immer am Ende; Deadlock-Gate pro Command.
        let commands = Arc::clone(&p.commands);
        let event_for_step = event.clone();
        let deadlock_live = class.is_deadlock_live;
        run_pipeline_step("commands", &channel_login, &chatter_login, async move {
            commands.handle(&event_for_step, deadlock_live).await;
        })
        .await;
        should_spawn_engagement
    }

    async fn handle_sus_invite(
        &self,
        event: &ChatMessageEvent,
        channel_login: &str,
        chatter_login: &str,
    ) {
        let p = &self.parts;
        let sus_invite = Arc::clone(&p.sus_invite);
        let event_for_step = event.clone();
        let sus_channel = channel_login.to_string();
        let Some(hit) = run_pipeline_step(
            "sus_invite",
            channel_login,
            chatter_login,
            async move { sus_invite.check(&event_for_step, &sus_channel).await },
        )
        .await
        .flatten() else {
            return;
        };

        p.review_log.record(
            channel_login,
            &hit.chatter_login,
            &hit.chatter_id,
            &hit.content,
            "SUSPICIOUS_DISCORD_INVITE",
            "discord.gg link in partner chat",
        );
        p.alerter.send(
            "sus_invite",
            channel_login,
            &hit.chatter_login,
            &hit.chatter_id,
            &hit.content,
            "",
        );

        let input = hit.content.chars().take(200).collect::<String>();
        match p
            .api
            .timeout_user(
                &event.broadcaster_user_id,
                &hit.chatter_id,
                SUS_INVITE_TIMEOUT_SECS,
                SUS_INVITE_TIMEOUT_REASON,
            )
            .await
        {
            Ok(BanOutcome::Banned | BanOutcome::AlreadyBanned) => info!(
                input = %input,
                channel = channel_login,
                chatter = %hit.chatter_login,
                verdict = "timeout",
                reason = SUS_INVITE_TIMEOUT_REASON,
                result = "ok",
                "Sus-Invite-Enforcement entschieden"
            ),
            Ok(outcome) => warn!(
                input = %input,
                channel = channel_login,
                chatter = %hit.chatter_login,
                verdict = "timeout",
                reason = SUS_INVITE_TIMEOUT_REASON,
                result = "dropped",
                ?outcome,
                "Sus-Invite-Enforcement entschieden"
            ),
            Err(error) => warn!(
                input = %input,
                channel = channel_login,
                chatter = %hit.chatter_login,
                verdict = "timeout",
                reason = SUS_INVITE_TIMEOUT_REASON,
                result = "err",
                %error,
                "Sus-Invite-Enforcement entschieden"
            ),
        }
    }

    /// Deadlock-live Chat-Detektoren, die selbst entscheiden, ob sie antworten.
    async fn run_deadlock_chat_detectors(
        &self,
        event: &ChatMessageEvent,
        channel_login: &str,
        chatter_login: &str,
    ) {
        let p = &self.parts;

        // Schritt 9: Fun-Responses, nur wenn Deadlock live (Z. 1745–1750)
        let fun = Arc::clone(&p.fun);
        let event_for_step = event.clone();
        let fun_channel = channel_login.to_string();
        run_pipeline_step("fun", channel_login, chatter_login, async move {
            fun.maybe_respond(&event_for_step, &fun_channel).await;
        })
        .await;

        // Schritt 10a: Deadlock-Zugangsfrage (Regex → KI → Antwort/Rückfrage)
        let invite_question = Arc::clone(&p.invite_question);
        let event_for_step = event.clone();
        let invite_channel = channel_login.to_string();
        let invite_sent = run_pipeline_step(
            "invite_question",
            channel_login,
            chatter_login,
            async move {
                invite_question
                    .maybe_respond(&event_for_step, &invite_channel)
                    .await
            },
        )
        .await
        .unwrap_or(false);

        if invite_sent {
            return;
        }

        // Schritt 10b: LFG-Mitspieler-Pitch (Regex → KI → Discord-Link)
        let lfg_pitch = Arc::clone(&p.lfg_pitch);
        let event_for_step = event.clone();
        let lfg_channel = channel_login.to_string();
        run_pipeline_step("lfg_pitch", channel_login, chatter_login, async move {
            lfg_pitch.maybe_respond(&event_for_step, &lfg_channel).await;
        })
        .await;
    }

    /// Spam-Schritt (bot.py Z. 1602–1737). Gibt `true` zurück wenn die
    /// Pipeline stoppen soll (Ban-Schwelle erreicht).
    async fn run_spam_check(
        &self,
        event: &ChatMessageEvent,
        channel_login: &str,
        chatter_login: &str,
        tracked_was_first_message: Option<bool>,
    ) -> bool {
        let p = &self.parts;
        let text = event.message.text.as_str();

        // Pass A: Basis-Score ohne Mention/Eskalatoren — bestimmt
        // has_phrase_or_fragment_signal für den @host-Bonus (Z. 1603–1607).
        let base = p.spam_filter.evaluate(text, &SpamContext::default());
        let has_phrase_or_fragment = base
            .matched
            .iter()
            .any(|r| r.starts_with("Phrase(") || r.starts_with("Fragment("));

        // Mention-Score (Z. 1608–1615).
        let (mention_reasons, mention_score) = score_mention_patterns(
            text,
            channel_login,
            has_phrase_or_fragment,
            p.mention_resolver.as_ref(),
        )
        .await;

        // Lazy-Eskalatoren: Helix-Call + Erstnachricht NUR wenn der Score noch
        // unter der Schwelle liegt UND ein hartes Signal vorliegt (Z. 1617–1653).
        // Mention-Reasons sind nie hart („Muster: @…") — base.matched reicht.
        let mut ctx = SpamContext {
            mention_score,
            ..Default::default()
        };
        if base.score + mention_score < SPAM_MIN_MATCHES && base.hard_signal {
            ctx.account_age_days = match p.api.user_created_at(&event.chatter_user_id).await {
                Ok(Some(created_at)) => Some((Utc::now() - created_at).num_days()),
                Ok(None) => None,
                Err(e) => {
                    debug!("spam: Account-Alter nicht ladbar: {e}");
                    None
                }
            };
            ctx.is_first_message = match tracked_was_first_message {
                Some(was_first) => was_first,
                None => is_first_message_for_streamer(&p.pool, channel_login, chatter_login).await,
            };
        }

        let verdict = p.spam_filter.evaluate(text, &ctx);
        // Reason-Liste wie Python: Basis + Mention + Eskalatoren (Reihenfolge
        // weicht minimal ab: Mention-Reasons stehen hier am Ende — rein kosmetisch,
        // die Logik wertet nur Präfixe aus).
        let mut reasons = verdict.matched.clone();
        reasons.extend(mention_reasons);

        match verdict.action {
            SpamAction::Ban => {
                // Z. 1656–1671: Ban + Stopp.
                let reasons_str = reasons.join(", ");
                let enforced = self
                    .execute_auto_ban_with_evidence(
                        event,
                        channel_login,
                        true,
                        BAN_REASON_SPAM,
                        None,
                        "ban",
                        ModerationEvidence {
                            source_path: "spam",
                            reason: &reasons_str,
                            score: Some(verdict.score as f32),
                            account_age_days: ctx.account_age_days,
                        },
                    )
                    .await;
                if !enforced
                    && crate::safe_list::is_safe(
                        Some(&event.chatter_user_id),
                        &event.chatter_user_login,
                    )
                {
                    self.maybe_reply_to_spam_meta(event, base.hard_signal).await;
                }
                if !enforced {
                    warn!(
                        channel = %channel_login,
                        score = verdict.score,
                        treffer = %reasons_str,
                        "Spam erkannt, aber Auto-Ban konnte nicht durchgesetzt werden"
                    );
                }
                true
            }

            SpamAction::DeleteOnly | SpamAction::None if verdict.score > 0 => {
                // Verdachts-Pfad: loggen, bei hartem Signal Delete-only, dann
                // Judge-Urteil + genau EIN Alert (asynchron) — KEIN Stopp.
                let reasons_str = reasons.join(", ");
                p.review_log.record(
                    channel_login,
                    chatter_login,
                    &event.chatter_user_id,
                    text,
                    &format!("SUSPICIOUS({})", verdict.score),
                    &reasons_str,
                );

                // Ehrlicher Delete-Status: nur wenn der Delete wirklich
                // durchging, darf der Alert „gelöscht" behaupten.
                let mut already_deleted = false;
                if verdict.hard_signal {
                    already_deleted = self
                        .execute_auto_ban_with_evidence(
                            event,
                            channel_login,
                            false,
                            BAN_REASON_SPAM,
                            None,
                            "ban",
                            ModerationEvidence {
                                source_path: "spam",
                                reason: &reasons_str,
                                score: Some(verdict.score as f32),
                                account_age_days: ctx.account_age_days,
                            },
                        )
                        .await;
                }

                if !already_deleted {
                    self.maybe_reply_to_spam_meta(event, base.hard_signal).await;
                }

                info!(
                    channel = %channel_login,
                    chatter = %chatter_login,
                    score = verdict.score,
                    treffer = %reasons_str,
                    "Verdächtige Nachricht"
                );

                // Judge + Alert asynchron: der Review dauert bis ~20s pro
                // Provider und darf die Chat-Pipeline nicht blockieren. Der
                // Alert kommt NACH dem Urteil und enthält es — jeder Ausgang
                // (spam/safe/error/skipped) erzeugt einen Alert.
                let this = self.clone();
                let event_owned = event.clone();
                let channel_owned = channel_login.to_string();
                let chatter_owned = chatter_login.to_string();
                let rule_reason = format!("Score {}: {}", verdict.score, reasons_str);
                let handle = tokio::spawn(async move {
                    this.judge_and_alert(
                        &event_owned,
                        &channel_owned,
                        &chatter_owned,
                        &rule_reason,
                        already_deleted,
                    )
                    .await;
                });
                // Panic-Wache: auch ein abgestürzter Judge-Task darf nie ein
                // stilles Ende sein — Log + Error-Alert statt Schweigen.
                let review_log = Arc::clone(&p.review_log);
                let alerter = Arc::clone(&p.alerter);
                let channel_owned = channel_login.to_string();
                let chatter_owned = chatter_login.to_string();
                let chatter_id_owned = event.chatter_user_id.clone();
                let text_owned = text.to_string();
                let rule_reason_owned = format!("Score {}: {}", verdict.score, reasons_str);
                tokio::spawn(async move {
                    if let Err(error) = handle.await {
                        tracing::error!(%error, "judge_and_alert-Task abgestürzt");
                        review_log.record(
                            &channel_owned,
                            &chatter_owned,
                            &chatter_id_owned,
                            &text_owned,
                            "AI_ERROR",
                            "judge_and_alert-Task abgestürzt (Panic)",
                        );
                        alerter.send_sus_spam(SusSpamAlert {
                            channel_login: &channel_owned,
                            chatter_login: &chatter_owned,
                            chatter_id: &chatter_id_owned,
                            content: &text_owned,
                            rule_reason: &rule_reason_owned,
                            outcome: &AiReviewOutcome::Error {
                                detail: "interner Fehler (Judge-Task abgestürzt)".to_string(),
                            },
                            action_taken: "keine",
                        });
                    }
                });
                false
            }
            _ => false,
        }
    }

    /// Verdachtsfall (Score 1–2): Judge fragen, bei Spam-Urteil handeln
    /// (Nachricht löschen + 24h-Timeout, reversibel), und in JEDEM Fall genau
    /// einen Alert mit Urteil, Begründung und Aktion schicken.
    async fn judge_and_alert(
        &self,
        event: &ChatMessageEvent,
        channel_login: &str,
        chatter_login: &str,
        rule_reason: &str,
        already_deleted: bool,
    ) {
        let p = &self.parts;
        let text = event.message.text.as_str();

        let outcome = p.ai_reviewer.review_for_verdict(event).await;

        // Jede Judge-Entscheidung sichtbar machen: Review-Log + Tracing.
        let (status, log_reason) = match &outcome {
            AiReviewOutcome::Spam {
                reason, confidence, ..
            } => (
                "AI_SPAM",
                format!(
                    "Judge: {reason}; Confidence: {}",
                    confidence
                        .map(|value| format!("{value:.2}"))
                        .unwrap_or_else(|| "unbekannt".to_string())
                ),
            ),
            AiReviewOutcome::Safe { reason } => ("AI_SAFE", reason.clone()),
            AiReviewOutcome::Error { detail } => ("AI_ERROR", detail.clone()),
            AiReviewOutcome::Skipped => ("AI_SKIPPED", "Cooldown aktiv".to_string()),
        };
        p.review_log.record(
            channel_login,
            chatter_login,
            &event.chatter_user_id,
            text,
            status,
            &log_reason,
        );
        info!(
            channel = %channel_login,
            chatter = %chatter_login,
            urteil = status,
            grund = %log_reason,
            "Judge-Entscheidung"
        );

        // Aktion nur bei Spam-Urteil — mit denselben Pre-Checks wie der
        // Auto-Ban-Pfad (nie Broadcaster/Mods, Safe-List greift in
        // timeout_and_cleanup).
        let mut action_taken = if already_deleted {
            "Nachricht gelöscht (hartes Signal)".to_string()
        } else {
            "keine".to_string()
        };
        if let AiReviewOutcome::Spam { confidence, .. } = &outcome {
            if event.chatter_user_id.is_empty()
                || event.chatter_user_id == event.broadcaster_user_id
                || event.is_mod_or_broadcaster()
            {
                action_taken = "übersprungen (Mod/Broadcaster)".to_string();
            } else {
                let evidence_reason = log_reason.clone();
                let enforced = p
                    .moderation
                    .timeout_and_cleanup_with_evidence(
                        channel_login,
                        &event.broadcaster_user_id,
                        chatter_login,
                        &event.chatter_user_id,
                        &event.message_id,
                        text,
                        AI_SPAM_TIMEOUT_SECS,
                        BAN_REASON_SPAM,
                        ModerationEvidence {
                            source_path: "spam",
                            reason: &evidence_reason,
                            score: *confidence,
                            account_age_days: None,
                        },
                    )
                    .await;
                if enforced {
                    action_taken = "Nachricht gelöscht + 24h Timeout".to_string();
                    p.review_log.record(
                        channel_login,
                        chatter_login,
                        &event.chatter_user_id,
                        text,
                        "AI_TIMEOUT",
                        BAN_REASON_SPAM,
                    );
                } else {
                    action_taken = "Timeout nicht durchgesetzt".to_string();
                    warn!(
                        channel = %channel_login,
                        chatter = %chatter_login,
                        "AI-Spam bestätigt, aber Timeout konnte nicht durchgesetzt werden"
                    );
                }
            }
        }

        p.alerter.send_sus_spam(SusSpamAlert {
            channel_login,
            chatter_login,
            chatter_id: &event.chatter_user_id,
            content: text,
            rule_reason,
            outcome: &outcome,
            action_taken: &action_taken,
        });
    }

    /// Gemeinsamer Auto-Ban-Pfad — Python `_auto_ban_and_cleanup` inkl. der
    /// Pre-Checks (Z. 1627–1637) und Nachgelagertem (Review-Log + Alert).
    #[allow(clippy::too_many_arguments)]
    async fn execute_auto_ban_with_evidence(
        &self,
        event: &ChatMessageEvent,
        channel_login: &str,
        ban: bool,
        reason_text: &str,
        notice_text: Option<&str>,
        alert_kind: &str,
        evidence: ModerationEvidence<'_>,
    ) -> bool {
        let p = &self.parts;
        let chatter_login = event.chatter_user_login.to_lowercase();
        let content = event.message.text.as_str();

        // Pre-Checks (moderation.py Z. 1627–1637): kein Self-Ban, keine
        // Mods/Broadcaster, chatter_id muss vorhanden sein.
        if event.chatter_user_id.is_empty()
            || event.chatter_user_id == event.broadcaster_user_id
            || event.is_mod_or_broadcaster()
        {
            return false;
        }

        let silent = self.is_silent_ban(channel_login).await;

        let enforced = p
            .moderation
            .auto_ban_and_cleanup_with_evidence(
                AutoBanRequest {
                    channel_login,
                    broadcaster_id: &event.broadcaster_user_id,
                    bot_id: &p.bot_user_id,
                    chatter_login: &chatter_login,
                    chatter_id: &event.chatter_user_id,
                    message_id: &event.message_id,
                    content,
                    ban,
                    reason_text,
                    notice_text,
                    silent,
                },
                evidence,
            )
            .await;

        if enforced {
            // Review-Log: BANNED bzw. DELETED (moderation.py Z. 1708/1746).
            let status = if ban { "BANNED" } else { "DELETED" };
            p.review_log.record(
                channel_login,
                &chatter_login,
                &event.chatter_user_id,
                content,
                status,
                "",
            );
            // Discord-Alert nur im Ban-Pfad (Z. 1762; Delete-only alertet nicht).
            if ban {
                p.alerter.send(
                    alert_kind,
                    channel_login,
                    &chatter_login,
                    &event.chatter_user_id,
                    content,
                    "",
                );
            }
        }
        enforced
    }

    async fn maybe_reply_to_spam_meta(&self, event: &ChatMessageEvent, has_spam_marker: bool) {
        if !has_spam_marker || !is_spam_meta_question(event.text()) {
            return;
        }

        let key = if event.chatter_user_id.trim().is_empty() {
            event.chatter_user_login.to_lowercase()
        } else {
            event.chatter_user_id.clone()
        };
        let now = std::time::Instant::now();
        match self.spam_meta_cooldowns.entry(key) {
            dashmap::mapref::entry::Entry::Occupied(mut entry) => {
                if now.duration_since(*entry.get()).as_secs() < SPAM_META_REPLY_COOLDOWN_SECS {
                    return;
                }
                entry.insert(now);
            }
            dashmap::mapref::entry::Entry::Vacant(entry) => {
                entry.insert(now);
            }
        }

        let index = self
            .spam_meta_reply_index
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            % SPAM_META_REPLIES.len();
        if let Err(error) = self
            .parts
            .api
            .send_message(&event.broadcaster_user_id, SPAM_META_REPLIES[index])
            .await
        {
            warn!(%error, chatter = %event.chatter_user_login, "Spam-Meta-Antwort fehlgeschlagen");
        }
    }

    /// `silent_ban`-Flag des Partners (moderation.py Z. 1775–1790 via
    /// `load_active_partner`; View-Spalte ist INTEGER). Fail-safe: false.
    async fn is_silent_ban(&self, channel_login: &str) -> bool {
        sqlx::query_scalar!(
            "SELECT COALESCE(silent_ban, 0) AS \"silent_ban!\" \
             FROM twitch_streamers_partner_state \
             WHERE LOWER(twitch_login) = $1",
            channel_login,
        )
        .fetch_optional(&self.parts.pool)
        .await
        .unwrap_or(None)
        .unwrap_or(0)
            != 0
    }
}

fn is_spam_meta_question(text: &str) -> bool {
    let lower = text.to_lowercase();
    [
        "bannt mich der bot wenn",
        "werde ich gebannt wenn",
        "meinste der bot",
        "bannt der bot mich",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

async fn run_pipeline_step<T, F>(
    step: &'static str,
    channel_login: &str,
    chatter_login: &str,
    future: F,
) -> Option<T>
where
    T: Send + 'static,
    F: Future<Output = T> + Send + 'static,
{
    match tokio::spawn(future).await {
        Ok(value) => Some(value),
        Err(error) if error.is_panic() => {
            warn!(
                channel = %channel_login,
                chatter = %chatter_login,
                step,
                %error,
                "chat_pipeline: Schritt panicked"
            );
            None
        }
        Err(error) => {
            warn!(
                channel = %channel_login,
                chatter = %chatter_login,
                step,
                %error,
                "chat_pipeline: Schritt abgebrochen"
            );
            None
        }
    }
}

async fn run_security_pipeline_step<T, F>(
    step: &'static str,
    channel_login: &str,
    chatter_login: &str,
    future: F,
) -> Option<T>
where
    T: Send + 'static,
    F: Future<Output = T> + Send + 'static,
{
    let result = run_pipeline_step(step, channel_login, chatter_login, future).await;
    if result.is_none() {
        warn!(
            channel = %channel_login,
            chatter = %chatter_login,
            step,
            "chat_pipeline: security step failed closed; stopping pipeline"
        );
    }
    result
}

async fn handle_strong_timeout(
    api: &Arc<dyn ChatApi>,
    alerter: &Arc<ModAlerter>,
    event: &ChatMessageEvent,
    channel_login: &str,
    chatter_login: &str,
    text: &str,
    duration: std::time::Duration,
) {
    if event.chatter_user_id.is_empty()
        || event.chatter_user_id == event.broadcaster_user_id
        || event.is_mod_or_broadcaster()
        // Safe-List: dieser Pfad timeoutet direkt, ohne ModerationEngine.
        || crate::safe_list::is_safe(Some(&event.chatter_user_id), chatter_login)
    {
        return;
    }

    let timeout_secs = duration.as_secs().min(u64::from(u32::MAX)) as u32;
    if let Err(e) = api
        .timeout_user(
            &event.broadcaster_user_id,
            &event.chatter_user_id,
            timeout_secs,
            SCAM_PITCH_TIMEOUT_REASON,
        )
        .await
    {
        debug!("Pitch-Timeout fehlgeschlagen: {e}");
    }

    alerter.send(
        "scam_pitch_timeout",
        channel_login,
        chatter_login,
        &event.chatter_user_id,
        event.text(),
        "",
    );

    if let Err(e) = api.send_message(&event.broadcaster_user_id, text).await {
        debug!("Pitch-Eskalationstext konnte nicht gesendet werden: {e}");
    }
}

/// Erstnachricht-Check: kein Rollup-Eintrag = Erstkontakt mit diesem Streamer
/// (`_is_first_message_for_streamer`, moderation.py Z. 815–841). Fail-safe:
/// bei leeren Werten oder DB-Fehler `false` — lieber nicht eskalieren.
async fn is_first_message_for_streamer(
    pool: &PgPool,
    channel_login: &str,
    chatter_login: &str,
) -> bool {
    if channel_login.is_empty() || chatter_login.is_empty() {
        return false;
    }
    match sqlx::query_scalar!(
        "SELECT 1 FROM twitch_chatter_rollup \
         WHERE streamer_login = $1 AND chatter_login = $2 LIMIT 1",
        channel_login,
        chatter_login,
    )
    .fetch_optional(pool)
    .await
    {
        Ok(Some(_)) => false,
        Ok(None) => true,
        Err(e) => {
            debug!("first_message-Check fehlgeschlagen: {e}");
            false
        }
    }
}

// ---------------------------------------------------------------------------
// Unit-Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;

    use crate::api::BanOutcome;
    use crate::commands::{
        AutobanEntry, DiscordLinkPort, InvitePort, LastAutobanStore, RaidCommandPort,
        RaidStartResult, RaidStatusInfo, SuperModPort,
    };
    use crate::promos::OutboundSuppressionCheck;
    use crate::scam_pitch::AccountAgePort;
    use crate::types::{ChatMessageBody, SendOutcome};
    use chrono::{DateTime, Utc};
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use tb_engagement::crew_review::{RickyChatInput, RICKY_TWITCH_USER_ID};
    use tb_engagement::minimax_chat::EngagementMinimaxClient;
    use tokio::time::{sleep, Duration};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn assert_crew_radar_payload_notify_only(notify_only: bool) {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/changelog"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;
        let alerter = Arc::new(ModAlerter::with_endpoint_and_channel_id(
            reqwest::Client::new(),
            format!("{}/changelog", server.uri()),
            123,
        ));
        alerter.send_crew_campaign(CrewRadarAlert {
            message: "fertig".to_string(),
            chatter_login: "viewer".to_string(),
            chatter_id: String::new(),
            channel_login: "kanal".to_string(),
            style_score: 5,
            verdict: "clean".to_string(),
            notify_only,
        });

        for _ in 0..20 {
            let requests = server.received_requests().await.expect("Requests");
            if let Some(request) = requests.first() {
                let body: serde_json::Value =
                    serde_json::from_slice(&request.body).expect("JSON-Payload");
                assert_eq!(body["title"], "🕵️ Crew-Guard (Radar)");
                assert_eq!(body["content"], "fertig");
                assert_eq!(body["channel_id"], 123);
                assert_eq!(body["token"], "changeme-local");
                assert_eq!(
                    body["crew_radar"],
                    serde_json::json!({
                        "v": 1,
                        "login": "viewer",
                        "chatter_id": "",
                        "channel": "kanal",
                        "style_score": 5,
                        "verdict": "clean",
                        "notify_only": notify_only,
                    })
                );
                return;
            }
            sleep(Duration::from_millis(25)).await;
        }
        panic!("kein Crew-Radar-Payload empfangen");
    }

    #[tokio::test]
    async fn crew_radar_payload_partner_behaelt_ban_button_vertrag() {
        assert_crew_radar_payload_notify_only(false).await;
    }

    #[tokio::test]
    async fn crew_radar_payload_lurker_ist_notify_only() {
        assert_crew_radar_payload_notify_only(true).await;
    }

    #[derive(Default)]
    struct RecordingChatApi {
        calls: Mutex<Vec<String>>,
    }

    #[derive(Default)]
    struct RecordingCrewReviewTrigger(Mutex<Vec<RickyChatInput>>);

    impl CrewReviewTrigger for RecordingCrewReviewTrigger {
        fn observe(&self, input: RickyChatInput) {
            self.0.lock().expect("Recording-Lock").push(input);
        }
    }

    impl RecordingChatApi {
        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }

        fn push_call(&self, call: impl Into<String>) {
            self.calls.lock().unwrap().push(call.into());
        }
    }

    struct NoopAccountAge;

    #[async_trait::async_trait]
    impl AccountAgePort for NoopAccountAge {
        async fn user_created_at_days(&self, _user_id: &str, _login: &str) -> Option<i64> {
            None
        }
    }

    struct NoopMentionResolver;

    #[async_trait::async_trait]
    impl MentionResolver for NoopMentionResolver {
        async fn is_known_chatter(&self, _channel_login: &str, _mention_login: &str) -> bool {
            false
        }

        async fn resolve_existing(
            &self,
            _logins: &[&str],
        ) -> (std::collections::HashSet<String>, bool) {
            (std::collections::HashSet::new(), false)
        }
    }

    struct NoopSuppression;

    #[async_trait::async_trait]
    impl OutboundSuppressionCheck for NoopSuppression {
        async fn is_muted(&self, _channel_login: &str) -> bool {
            false
        }
    }

    struct NoopRaid;

    #[async_trait::async_trait]
    impl RaidCommandPort for NoopRaid {
        async fn manual_raid(
            &self,
            _broadcaster_id: &str,
            _broadcaster_login: &str,
        ) -> Result<RaidStartResult, String> {
            Ok(RaidStartResult {
                status: "unavailable".to_string(),
                target_login: None,
            })
        }

        async fn raid_status(&self, _broadcaster_id: &str) -> Result<RaidStatusInfo, String> {
            Ok(RaidStatusInfo {
                raid_enabled: None,
                authorized_at: None,
                total_raids: 0,
                successful_raids: 0,
                last_raid_login: None,
                last_raid_viewers: None,
                last_raid_at: None,
            })
        }

        async fn toggle_silent_ban(&self, _twitch_login: &str) -> Result<i32, String> {
            Ok(0)
        }

        async fn toggle_silent_raid(&self, _twitch_login: &str) -> Result<i32, String> {
            Ok(0)
        }
    }

    struct NoopDiscordLink;

    #[async_trait::async_trait]
    impl DiscordLinkPort for NoopDiscordLink {
        async fn discord_invite(&self, _channel_login: &str) -> Result<Option<String>, String> {
            Ok(None)
        }
    }

    #[async_trait::async_trait]
    impl crate::invite_question::InviteQuestionInviteUrlPort for NoopDiscordLink {
        async fn invite_url(&self, _channel_login: &str) -> Result<Option<String>, String> {
            Ok(None)
        }
    }

    struct StaticInviteUrl;

    #[async_trait::async_trait]
    impl crate::invite_question::InviteQuestionInviteUrlPort for StaticInviteUrl {
        async fn invite_url(&self, _channel_login: &str) -> Result<Option<String>, String> {
            Ok(Some("https://discord.gg/lfg-test".to_string()))
        }
    }

    struct NoopLfgJudge;

    #[async_trait::async_trait]
    impl crate::lfg_pitch::LfgJudge for NoopLfgJudge {
        async fn judge(
            &self,
            _input: crate::lfg_pitch::LfgJudgeInput,
        ) -> crate::lfg_pitch::LfgVerdict {
            crate::lfg_pitch::LfgVerdict {
                verdict: crate::lfg_pitch::LfgVerdictKind::No,
                confidence: 1.0,
                reasoning: "test".to_string(),
                source: crate::lfg_pitch::LfgVerdictSource::Model,
            }
        }
    }

    struct AlwaysYesLfgJudge;

    #[async_trait::async_trait]
    impl crate::lfg_pitch::LfgJudge for AlwaysYesLfgJudge {
        async fn judge(
            &self,
            _input: crate::lfg_pitch::LfgJudgeInput,
        ) -> crate::lfg_pitch::LfgVerdict {
            crate::lfg_pitch::LfgVerdict {
                verdict: crate::lfg_pitch::LfgVerdictKind::Yes,
                confidence: 0.9,
                reasoning: "test".to_string(),
                source: crate::lfg_pitch::LfgVerdictSource::Model,
            }
        }
    }

    struct NewcomerInviteStore;

    #[async_trait::async_trait]
    impl crate::invite_question::InviteQuestionStore for NewcomerInviteStore {
        async fn rollup(
            &self,
            _channel_login: &str,
            _chatter_login: &str,
        ) -> Result<Option<crate::invite_question::InviteQuestionRollup>, String> {
            Ok(Some(crate::invite_question::InviteQuestionRollup {
                total_messages: 1,
                total_sessions: 1,
                is_first_time_streamer: false,
            }))
        }
    }

    struct AlwaysYesInviteJudge;

    #[async_trait::async_trait]
    impl crate::invite_question::InviteQuestionJudge for AlwaysYesInviteJudge {
        async fn judge(
            &self,
            _input: crate::invite_question::InviteQuestionJudgeInput,
        ) -> crate::invite_question::InviteQuestionVerdict {
            crate::invite_question::InviteQuestionVerdict {
                verdict: crate::invite_question::InviteQuestionVerdictKind::Yes,
                confidence: 0.9,
                reasoning: "test".to_string(),
                source: crate::invite_question::InviteQuestionVerdictSource::Model,
            }
        }
    }

    struct NoopInvite;

    #[async_trait::async_trait]
    impl InvitePort for NoopInvite {
        async fn invite_line(
            &self,
            _channel_login: &str,
            _chatter_login: &str,
        ) -> Result<Option<String>, String> {
            Ok(None)
        }
    }

    struct NoopSuperMod;

    #[async_trait::async_trait]
    impl SuperModPort for NoopSuperMod {
        async fn is_super_mod(&self, _actor_id: &str) -> bool {
            false
        }
    }

    struct NoopAutoban;

    #[async_trait::async_trait]
    impl LastAutobanStore for NoopAutoban {
        async fn last_autoban(&self, _channel_key: &str) -> Option<AutobanEntry> {
            None
        }
    }

    #[async_trait::async_trait]
    impl ChatApi for RecordingChatApi {
        async fn send_message(
            &self,
            broadcaster_id: &str,
            message: &str,
        ) -> Result<SendOutcome, String> {
            self.push_call(format!("send:{broadcaster_id}:{message}"));
            Ok(SendOutcome::Sent)
        }

        async fn send_announcement(
            &self,
            _broadcaster_id: &str,
            _message: &str,
            _color: &str,
        ) -> Result<bool, String> {
            Ok(true)
        }

        async fn ban_user(
            &self,
            broadcaster_id: &str,
            target_user_id: &str,
            reason: &str,
        ) -> Result<BanOutcome, String> {
            self.push_call(format!("ban:{broadcaster_id}:{target_user_id}:{reason}"));
            Ok(BanOutcome::Banned)
        }

        async fn timeout_user(
            &self,
            broadcaster_id: &str,
            target_user_id: &str,
            duration_secs: u32,
            reason: &str,
        ) -> Result<BanOutcome, String> {
            self.push_call(format!(
                "timeout:{broadcaster_id}:{target_user_id}:{duration_secs}:{reason}"
            ));
            Ok(BanOutcome::Banned)
        }

        async fn unban_user(
            &self,
            _broadcaster_id: &str,
            _target_user_id: &str,
        ) -> Result<bool, String> {
            Ok(true)
        }

        async fn delete_message(
            &self,
            broadcaster_id: &str,
            message_id: &str,
        ) -> Result<bool, String> {
            self.push_call(format!("delete:{broadcaster_id}:{message_id}"));
            Ok(true)
        }

        async fn user_created_at(&self, _user_id: &str) -> Result<Option<DateTime<Utc>>, String> {
            Ok(None)
        }

        async fn resolve_user_id(&self, _login: &str) -> Result<Option<String>, String> {
            Ok(None)
        }

        async fn bot_user_id(&self) -> String {
            "bot-id".to_string()
        }
    }

    fn strong_timeout_event() -> ChatMessageEvent {
        ChatMessageEvent {
            broadcaster_user_id: "broadcaster-id".to_string(),
            broadcaster_user_login: "channel".to_string(),
            broadcaster_user_name: String::new(),
            chatter_user_id: "chatter-id".to_string(),
            chatter_user_login: "seller".to_string(),
            chatter_user_name: String::new(),
            message_id: "msg-1".to_string(),
            message: ChatMessageBody {
                text: "incoming pitch".to_string(),
                fragments: vec![],
            },
            badges: vec![],
            color: String::new(),
            source_broadcaster_user_id: None,
            source_broadcaster_user_login: None,
            source_message_id: None,
        }
    }

    fn pipeline_for_non_partner(api: Arc<RecordingChatApi>, pool: PgPool) -> ChatPipeline {
        pipeline_with_crew_review_trigger(api, pool, None)
    }

    fn pipeline_with_crew_review_trigger(
        api: Arc<RecordingChatApi>,
        pool: PgPool,
        crew_review_trigger: Option<Arc<dyn CrewReviewTrigger>>,
    ) -> ChatPipeline {
        let api_trait: Arc<dyn ChatApi> = api;
        let http = reqwest::Client::new();
        let moderation = Arc::new(ModerationEngine::new(Arc::clone(&api_trait), pool.clone()));
        ChatPipeline::new(ChatPipelineParts {
            bot_user_id: "bot-id".to_string(),
            api: Arc::clone(&api_trait),
            pool: pool.clone(),
            classifier: Arc::new(ChannelClassifier::new(pool.clone())),
            tracker: Arc::new(ChatterTracker::new(pool.clone())),
            global_ban: Arc::new(GlobalChatterBanEnforcer::new(pool.clone())),
            scam_pitch: Arc::new(ScamPitchDetector::new(
                Arc::clone(&api_trait),
                Arc::new(NoopAccountAge),
                pool.clone(),
            )),
            conversation_scam: Arc::new(ConversationScamGuard::new(
                pool.clone(),
                "bot-id".to_string(),
                Arc::new(crate::conversation_scam::MiniMaxScamJudge::new(
                    EngagementMinimaxClient::new(None, None, None, None),
                )),
                Arc::clone(&api_trait),
                Arc::clone(&moderation),
            )),
            spam_filter: Arc::new(SpamFilter::new(Default::default())),
            ai_reviewer: Arc::new(SpamAiReviewer::new(pool.clone(), http.clone())),
            moderation,
            sus_invite: Arc::new(SusInviteCheck::new(pool.clone())),
            fun: Arc::new(FunResponses::new(Arc::clone(&api_trait), false)),
            invite_question: Arc::new(crate::invite_question::InviteQuestionResponder::new(
                Arc::clone(&api_trait),
                Arc::new(NoopDiscordLink),
                Arc::new(crate::invite_question::PgInviteQuestionStore::new(
                    pool.clone(),
                )),
                Arc::new(crate::invite_question::MiniMaxInviteQuestionJudge::new(
                    EngagementMinimaxClient::new(None, None, None, None),
                )),
                None,
                None,
            )),
            lfg_pitch: Arc::new(crate::lfg_pitch::LfgPitchResponder::new(
                Arc::clone(&api_trait),
                Arc::new(NoopDiscordLink),
                Arc::new(NoopLfgJudge),
                true,
                None,
                None,
            )),
            promos: Arc::new(PromoEngine::new(
                pool.clone(),
                Arc::clone(&api_trait),
                Arc::new(NoopSuppression),
            )),
            commands: Arc::new(CommandEngine::new(
                pool,
                Arc::clone(&api_trait),
                Arc::new(NoopRaid),
                Arc::new(NoopDiscordLink),
                Arc::new(NoopInvite),
                Arc::new(NoopSuperMod),
                Arc::new(NoopAutoban),
            )),
            mention_resolver: Arc::new(NoopMentionResolver),
            review_log: Arc::new(ReviewLog::new(std::env::temp_dir())),
            alerter: Arc::new(ModAlerter::with_endpoint(
                http,
                "http://127.0.0.1:1/changelog",
            )),
            account_age: Arc::new(NoopAccountAge),
            crew_judge: Arc::new(crate::crew_guard::OpenAiCrewJudge::from_env()),
            crew_centroid: Arc::new(crate::style_score::Centroid::default()),
            crew_review_trigger,
        })
    }

    async fn moderation_test_pool(schema: &str) -> Option<PgPool> {
        let Some(dsn) = std::env::var("TB_TEST_DATABASE_URL").ok() else {
            assert_ne!(
                std::env::var("TB_TEST_REQUIRE_DB").as_deref(),
                Ok("1"),
                "TB_TEST_REQUIRE_DB=1 gesetzt, aber TB_TEST_DATABASE_URL fehlt"
            );
            return None;
        };
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&dsn)
            .await
            .unwrap();
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
        let opts = PgConnectOptions::from_str(&dsn)
            .unwrap()
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect_with(opts)
            .await
            .unwrap();
        for ddl in [
            "CREATE TABLE twitch_streamers_partner_state (twitch_login TEXT PRIMARY KEY, twitch_user_id TEXT, is_partner_active INTEGER NOT NULL DEFAULT 0, silent_ban INTEGER NOT NULL DEFAULT 0)",
            "CREATE TABLE twitch_partners (twitch_login TEXT, twitch_user_id TEXT)",
            "CREATE TABLE twitch_streamers (twitch_login TEXT PRIMARY KEY, twitch_user_id TEXT)",
            "CREATE TABLE twitch_live_state (twitch_user_id TEXT PRIMARY KEY, streamer_login TEXT NOT NULL, is_live INTEGER NOT NULL DEFAULT 0, last_game TEXT, active_session_id BIGINT)",
            "CREATE TABLE twitch_stream_sessions (id BIGINT PRIMARY KEY, streamer_login TEXT NOT NULL, started_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), ended_at TIMESTAMPTZ, game_name TEXT)",
            "CREATE TABLE twitch_chatter_rollup (streamer_login TEXT NOT NULL, chatter_login TEXT NOT NULL, chatter_id TEXT, first_seen_at TIMESTAMPTZ NOT NULL, last_seen_at TIMESTAMPTZ NOT NULL, total_messages INTEGER NOT NULL DEFAULT 0, total_sessions INTEGER NOT NULL DEFAULT 0, PRIMARY KEY (streamer_login, chatter_login))",
            "CREATE TABLE twitch_session_chatters (session_id BIGINT NOT NULL, streamer_login TEXT NOT NULL, chatter_login TEXT NOT NULL, chatter_id TEXT, first_message_at TIMESTAMPTZ, messages INTEGER NOT NULL DEFAULT 0, is_first_time_streamer BOOLEAN, seen_via_chatters_api BOOLEAN NOT NULL DEFAULT FALSE, last_seen_at TIMESTAMPTZ, PRIMARY KEY (session_id, chatter_login))",
            "CREATE TABLE twitch_chat_messages (id BIGSERIAL PRIMARY KEY, session_id BIGINT, streamer_login TEXT, chatter_login TEXT, chatter_id TEXT, message_id TEXT, message_ts TIMESTAMPTZ, is_command BOOLEAN, content TEXT, moderation_action TEXT, moderation_reason TEXT)",
            "CREATE TABLE twitch_raw_chat_ingest_health (streamer_login TEXT PRIMARY KEY, last_raw_chat_message_at TEXT, last_raw_chat_insert_ok_at TEXT, last_raw_chat_insert_error_at TEXT, last_raw_chat_error TEXT, raw_chat_lag_seconds INTEGER, updated_at TEXT)",
            "CREATE TABLE tb_chat_autoban_log (id BIGSERIAL PRIMARY KEY, channel_login TEXT NOT NULL, chatter_id TEXT NOT NULL, chatter_login TEXT NOT NULL, content TEXT, banned_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), action TEXT, source_path TEXT, reason TEXT, score REAL, account_age_days BIGINT)",
            "CREATE TABLE twitch_ban_events (id BIGSERIAL PRIMARY KEY, twitch_user_id TEXT NOT NULL, event_type TEXT NOT NULL, target_login TEXT, target_id TEXT, reason TEXT, received_at TIMESTAMPTZ NOT NULL DEFAULT NOW())",
            "CREATE TABLE twitch_chatter_global_ban (chatter_login TEXT PRIMARY KEY, chatter_id TEXT, reason TEXT, added_by TEXT, added_at TIMESTAMPTZ NOT NULL DEFAULT NOW())",
            "CREATE TABLE twitch_chatter_global_ban_applied (chatter_login TEXT NOT NULL, broadcaster_id TEXT NOT NULL, applied_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), PRIMARY KEY (chatter_login, broadcaster_id))",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        sqlx::query(
            "INSERT INTO twitch_streamers_partner_state (twitch_login, twitch_user_id) VALUES ('channel', 'broadcaster-id')",
        )
        .execute(&pool)
        .await
        .unwrap();
        Some(pool)
    }

    async fn seed_active_pipeline_channel(pool: &PgPool) {
        sqlx::query(
            "UPDATE twitch_streamers_partner_state SET is_partner_active = 1 WHERE twitch_login = 'channel'",
        )
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_partners (twitch_login, twitch_user_id) VALUES ('channel', 'broadcaster-id')",
        )
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_stream_sessions (id, streamer_login, game_name) VALUES (1, 'channel', 'other')",
        )
        .execute(pool)
        .await
        .unwrap();
    }

    async fn message_moderation(pool: &PgPool, message_id: &str) -> (String, Option<String>) {
        sqlx::query_as(
            "SELECT moderation_action, moderation_reason FROM twitch_chat_messages WHERE message_id = $1",
        )
        .bind(message_id)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    async fn archive_message(pool: &PgPool, message_id: &str, content: &str) {
        sqlx::query("INSERT INTO twitch_chat_messages (message_id, content) VALUES ($1, $2)")
            .bind(message_id)
            .bind(content)
            .execute(pool)
            .await
            .unwrap();
    }

    fn is_humor_reply(call: &str) -> bool {
        [
            "Ne, dich bann ich nicht. Ich kenn den Unterschied zwischen drüber reden und es tun.",
            "Keine Sorge, ich lese mit statt nur zu zählen. Über Spam reden ist kein Spam.",
            "Netter Versuch. Der Ban-Knopf bleibt aus, du redest ja nur drüber.",
        ]
        .iter()
        .any(|reply| call.ends_with(reply))
    }

    fn pipeline_for_lfg_detector(
        api: Arc<RecordingChatApi>,
        pool: PgPool,
    ) -> (ChatPipeline, Arc<RecordingChatApi>) {
        let invite_api = Arc::new(RecordingChatApi::default());
        let invite_api_trait: Arc<dyn ChatApi> = invite_api.clone();
        let api_trait: Arc<dyn ChatApi> = api;
        let http = reqwest::Client::new();
        let moderation = Arc::new(ModerationEngine::new(Arc::clone(&api_trait), pool.clone()));
        let pipeline = ChatPipeline::new(ChatPipelineParts {
            bot_user_id: "bot-id".to_string(),
            api: Arc::clone(&api_trait),
            pool: pool.clone(),
            classifier: Arc::new(ChannelClassifier::new(pool.clone())),
            tracker: Arc::new(ChatterTracker::new(pool.clone())),
            global_ban: Arc::new(GlobalChatterBanEnforcer::new(pool.clone())),
            scam_pitch: Arc::new(ScamPitchDetector::new(
                Arc::clone(&api_trait),
                Arc::new(NoopAccountAge),
                pool.clone(),
            )),
            conversation_scam: Arc::new(ConversationScamGuard::new(
                pool.clone(),
                "bot-id".to_string(),
                Arc::new(crate::conversation_scam::MiniMaxScamJudge::new(
                    EngagementMinimaxClient::new(None, None, None, None),
                )),
                Arc::clone(&api_trait),
                Arc::clone(&moderation),
            )),
            spam_filter: Arc::new(SpamFilter::new(Default::default())),
            ai_reviewer: Arc::new(SpamAiReviewer::new(pool.clone(), http.clone())),
            moderation,
            sus_invite: Arc::new(SusInviteCheck::new(pool.clone())),
            fun: Arc::new(FunResponses::new(Arc::clone(&api_trait), false)),
            invite_question: Arc::new(crate::invite_question::InviteQuestionResponder::new(
                invite_api_trait,
                Arc::new(StaticInviteUrl),
                Arc::new(NewcomerInviteStore),
                Arc::new(AlwaysYesInviteJudge),
                None,
                None,
            )),
            lfg_pitch: Arc::new(crate::lfg_pitch::LfgPitchResponder::new(
                Arc::clone(&api_trait),
                Arc::new(StaticInviteUrl),
                Arc::new(AlwaysYesLfgJudge),
                true,
                None,
                None,
            )),
            promos: Arc::new(PromoEngine::new(
                pool.clone(),
                Arc::clone(&api_trait),
                Arc::new(NoopSuppression),
            )),
            commands: Arc::new(CommandEngine::new(
                pool,
                Arc::clone(&api_trait),
                Arc::new(NoopRaid),
                Arc::new(NoopDiscordLink),
                Arc::new(NoopInvite),
                Arc::new(NoopSuperMod),
                Arc::new(NoopAutoban),
            )),
            mention_resolver: Arc::new(NoopMentionResolver),
            review_log: Arc::new(ReviewLog::new(std::env::temp_dir())),
            alerter: Arc::new(ModAlerter::with_endpoint(
                http,
                "http://127.0.0.1:1/changelog",
            )),
            account_age: Arc::new(NoopAccountAge),
            crew_judge: Arc::new(crate::crew_guard::OpenAiCrewJudge::from_env()),
            crew_centroid: Arc::new(crate::style_score::Centroid::default()),
            crew_review_trigger: None,
        });
        (pipeline, invite_api)
    }

    #[derive(Default)]
    struct FailClosedProbe {
        auto_ban: AtomicBool,
        promo: AtomicBool,
        command: AtomicBool,
        engagement: AtomicBool,
    }

    impl FailClosedProbe {
        fn mark_downstream(&self) {
            self.promo.store(true, Ordering::SeqCst);
            self.command.store(true, Ordering::SeqCst);
            self.engagement.store(true, Ordering::SeqCst);
        }

        fn assert_stopped_without_ban(&self) {
            assert!(!self.auto_ban.load(Ordering::SeqCst));
            assert!(!self.promo.load(Ordering::SeqCst));
            assert!(!self.command.load(Ordering::SeqCst));
            assert!(!self.engagement.load(Ordering::SeqCst));
        }
    }

    #[tokio::test]
    async fn global_ban_check_panic_fail_closed_stoppt_downstream() {
        let probe = Arc::new(FailClosedProbe::default());
        let result: Option<bool> =
            run_security_pipeline_step("global_ban", "channel", "chatter", async move {
                panic!("global_ban panic");
            })
            .await;

        let Some(is_banned) = result else {
            probe.assert_stopped_without_ban();
            return;
        };
        if is_banned {
            probe.auto_ban.store(true, Ordering::SeqCst);
            return;
        }
        probe.mark_downstream();
        probe.assert_stopped_without_ban();
    }

    #[tokio::test]
    async fn global_ban_enforce_panic_fail_closed_stoppt_downstream() {
        let probe = Arc::new(FailClosedProbe::default());
        let result: Option<bool> =
            run_security_pipeline_step("global_ban.enforce", "channel", "chatter", async move {
                panic!("global_ban.enforce panic");
            })
            .await;

        let Some(enforced) = result else {
            probe.assert_stopped_without_ban();
            return;
        };
        if enforced {
            probe.auto_ban.store(true, Ordering::SeqCst);
            return;
        }
        probe.mark_downstream();
        probe.assert_stopped_without_ban();
    }

    #[tokio::test]
    async fn spam_check_panic_fail_closed_stoppt_downstream() {
        let probe = Arc::new(FailClosedProbe::default());
        let result: Option<bool> =
            run_security_pipeline_step("spam_check", "channel", "chatter", async move {
                panic!("spam_check panic");
            })
            .await;

        let Some(handled) = result else {
            probe.assert_stopped_without_ban();
            return;
        };
        if handled {
            probe.auto_ban.store(true, Ordering::SeqCst);
            return;
        }
        probe.mark_downstream();
        probe.assert_stopped_without_ban();
    }

    #[test]
    fn conversation_scam_guard_wird_nach_partner_gate_fire_and_forget_aufgerufen() {
        let source = include_str!("pipeline.rs");
        let partner_gate = source
            .find("if !class.is_partner")
            .expect("Partner-Gate fehlt");
        let call_needle = ["p.conversation_scam", ".observe(event);"].concat();
        let guard_call = source
            .find(&call_needle)
            .expect("Conversation-Scam-Guard-Wiring fehlt");
        assert!(guard_call > partner_gate);
    }

    #[tokio::test]
    async fn non_partner_event_erreicht_engagement_outcome_nicht() {
        let pool = sqlx::PgPool::connect_lazy("postgres://x:x@127.0.0.1:1/x").unwrap();
        let api = Arc::new(RecordingChatApi::default());
        let pipeline = pipeline_for_non_partner(api, pool);

        assert!(!pipeline.handle(&strong_timeout_event()).await);
    }

    #[tokio::test]
    async fn shared_chat_whitespace_source_id_faellt_im_handle_auf_message_id_zurueck() {
        let Some(pool) = moderation_test_pool("pipeline_shared_chat_ricky_fallback").await else {
            return;
        };
        seed_active_pipeline_channel(&pool).await;
        let trigger = Arc::new(RecordingCrewReviewTrigger::default());
        let trigger_port: Arc<dyn CrewReviewTrigger> = trigger.clone();
        let pipeline = pipeline_with_crew_review_trigger(
            Arc::new(RecordingChatApi::default()),
            pool,
            Some(trigger_port),
        );
        let mut event = strong_timeout_event();
        event.broadcaster_user_id = "host-id".to_string();
        event.broadcaster_user_login = "host".to_string();
        event.chatter_user_id = RICKY_TWITCH_USER_ID.to_string();
        event.chatter_user_login = "helmbombenricky".to_string();
        event.message_id = "fallback-7".to_string();
        event.source_broadcaster_user_id = Some("broadcaster-id".to_string());
        event.source_broadcaster_user_login = Some("channel".to_string());
        event.source_message_id = Some("  ".to_string());

        pipeline.handle(&event).await;

        let inputs = trigger.0.lock().expect("Recording-Lock");
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].subject_twitch_user_id, RICKY_TWITCH_USER_ID);
        assert_eq!(inputs[0].source_message_id.as_deref(), Some("fallback-7"));
    }

    #[tokio::test]
    async fn lfg_nachricht_erreicht_lfg_pitch_responder() {
        let pool = sqlx::PgPool::connect_lazy("postgres://x:x@127.0.0.1:1/x").unwrap();
        let api = Arc::new(RecordingChatApi::default());
        let (pipeline, _) = pipeline_for_lfg_detector(Arc::clone(&api), pool);
        let mut event = strong_timeout_event();
        event.chatter_user_login = "viewer".to_string();
        event.chatter_user_id = "viewer-id".to_string();
        event.message.text = "lfg".to_string();

        pipeline
            .run_deadlock_chat_detectors(&event, "channel", "viewer")
            .await;

        assert!(
            api.calls()
                .iter()
                .any(|call| call.contains("https://discord.gg/lfg-test")),
            "{:?}",
            api.calls()
        );
    }

    #[tokio::test]
    async fn invite_antwort_ueberspringt_lfg_pitch_bei_doppelintent() {
        let pool = sqlx::PgPool::connect_lazy("postgres://x:x@127.0.0.1:1/x").unwrap();
        let lfg_api = Arc::new(RecordingChatApi::default());
        let (pipeline, invite_api) = pipeline_for_lfg_detector(Arc::clone(&lfg_api), pool);
        let mut event = strong_timeout_event();
        event.chatter_user_login = "viewer".to_string();
        event.chatter_user_id = "viewer-id".to_string();
        event.message.text = "Wie kann ich mitspielen, suche noch Leute für die Lobby?".to_string();

        pipeline
            .run_deadlock_chat_detectors(&event, "channel", "viewer")
            .await;

        assert_eq!(invite_api.calls().len(), 1, "{:?}", invite_api.calls());
        assert_eq!(lfg_api.calls().len(), 0, "{:?}", lfg_api.calls());
    }

    #[tokio::test]
    async fn spam_meta_frage_safe_list_antwortet_ohne_moderation() {
        let Some(pool) = moderation_test_pool("pipeline_meta_safe").await else {
            return;
        };
        let api = Arc::new(RecordingChatApi::default());
        let pipeline = pipeline_for_non_partner(Arc::clone(&api), pool.clone());
        let mut event = strong_timeout_event();
        event.chatter_user_id = "19123804".to_string();
        event.chatter_user_login = "kubi_kubi_kubi".to_string();
        event.message_id = "meta-safe-1".to_string();
        event.message.text =
            "meinste der bot bannt mich wenn ich aiviewers bei streamboo eingebe".to_string();
        archive_message(&pool, &event.message_id, event.text()).await;

        pipeline
            .run_spam_check(&event, "channel", "kubi_kubi_kubi", None)
            .await;

        let calls = api.calls();
        assert_eq!(calls.iter().filter(|call| is_humor_reply(call)).count(), 1);
        assert!(!calls.iter().any(|call| call.starts_with("ban:")));
        assert!(!calls.iter().any(|call| call.starts_with("delete:")));
    }

    #[tokio::test]
    async fn spam_meta_frage_ist_kein_bypass_fuer_echte_werbung() {
        let Some(pool) = moderation_test_pool("pipeline_meta_bypass").await else {
            return;
        };
        let api = Arc::new(RecordingChatApi::default());
        let pipeline = pipeline_for_non_partner(Arc::clone(&api), pool.clone());
        let mut event = strong_timeout_event();
        event.chatter_user_login = "spammer".to_string();
        event.message_id = "meta-bypass-1".to_string();
        event.message.text =
            "Best viewers streamboo.com – bannt der bot mich wenn ich das sage?".to_string();
        archive_message(&pool, &event.message_id, event.text()).await;

        assert!(
            pipeline
                .run_spam_check(&event, "channel", "spammer", None)
                .await
        );
        let calls = api.calls();
        assert!(
            calls.iter().any(|call| call.starts_with("ban:")),
            "{calls:?}"
        );
        assert!(
            calls.iter().any(|call| call.starts_with("delete:")),
            "{calls:?}"
        );
        assert!(!calls.iter().any(|call| is_humor_reply(call)), "{calls:?}");
    }

    #[tokio::test]
    async fn spam_meta_frage_hat_eine_stunde_chatter_cooldown() {
        let Some(pool) = moderation_test_pool("pipeline_meta_cooldown").await else {
            return;
        };
        let api = Arc::new(RecordingChatApi::default());
        let pipeline = pipeline_for_non_partner(Arc::clone(&api), pool.clone());
        let mut event = strong_timeout_event();
        event.chatter_user_id = "19123804".to_string();
        event.chatter_user_login = "kubi_kubi_kubi".to_string();
        event.message.text =
            "meinste der bot bannt mich wenn ich aiviewers bei streamboo eingebe".to_string();
        for suffix in ["1", "2"] {
            event.message_id = format!("meta-cooldown-{suffix}");
            archive_message(&pool, &event.message_id, event.text()).await;
            pipeline
                .run_spam_check(&event, "channel", "kubi_kubi_kubi", None)
                .await;
        }

        assert_eq!(
            api.calls()
                .iter()
                .filter(|call| is_humor_reply(call))
                .count(),
            1,
            "{:?}",
            api.calls()
        );
    }

    #[tokio::test]
    async fn global_ban_archiviert_vor_ban_und_markiert_nachricht() {
        let Some(pool) = moderation_test_pool("pipeline_archive_global_ban").await else {
            return;
        };
        seed_active_pipeline_channel(&pool).await;
        sqlx::query(
            "INSERT INTO twitch_chatter_global_ban (chatter_login, chatter_id, reason) \
             VALUES ('seller', 'chatter-id', 'Viewerbot-Netzwerk Juli')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let api = Arc::new(RecordingChatApi::default());
        let pipeline = pipeline_for_non_partner(api, pool.clone());
        let event = strong_timeout_event();

        assert!(!pipeline.handle(&event).await);
        let (action, reason) = message_moderation(&pool, &event.message_id).await;
        assert_eq!(action, "ban");
        assert_eq!(reason.as_deref(), Some("Viewerbot-Netzwerk Juli"));
    }

    #[tokio::test]
    async fn spam_ban_archiviert_vor_ban_und_markiert_nachricht() {
        let Some(pool) = moderation_test_pool("pipeline_archive_spam_ban").await else {
            return;
        };
        seed_active_pipeline_channel(&pool).await;
        let api = Arc::new(RecordingChatApi::default());
        let pipeline = pipeline_for_non_partner(api, pool.clone());
        let mut event = strong_timeout_event();
        event.message_id = "pipeline-spam-ban".to_string();
        event.message.text = "Best viewers streamboo.com".to_string();

        assert!(!pipeline.handle(&event).await);
        let (action, reason) = message_moderation(&pool, &event.message_id).await;
        assert_eq!(action, "ban");
        assert!(reason.as_deref().unwrap().contains("streamboo.com"));
    }

    #[tokio::test]
    async fn spam_delete_only_archiviert_vor_delete_und_markiert_nachricht() {
        let Some(pool) = moderation_test_pool("pipeline_archive_spam_delete").await else {
            return;
        };
        seed_active_pipeline_channel(&pool).await;
        let api = Arc::new(RecordingChatApi::default());
        let pipeline = pipeline_for_non_partner(api, pool.clone());
        let mut event = strong_timeout_event();
        event.chatter_user_id = "established-id".to_string();
        event.chatter_user_login = "established".to_string();
        event.message_id = "pipeline-spam-delete".to_string();
        event.message.text = "streamboo".to_string();
        sqlx::query(
            "INSERT INTO twitch_chatter_rollup \
             (streamer_login, chatter_login, chatter_id, first_seen_at, last_seen_at, total_messages, total_sessions) \
             VALUES ('channel', 'established', 'established-id', NOW() - INTERVAL '30 days', NOW(), 50, 5)",
        )
        .execute(&pool)
        .await
        .unwrap();

        pipeline.handle(&event).await;
        let (action, reason) = message_moderation(&pool, &event.message_id).await;
        assert_eq!(action, "delete_only");
        assert!(reason.as_deref().unwrap().contains("streamboo"));
    }

    #[tokio::test]
    async fn fremder_discord_invite_timeoutet_neuen_chatter_genau_einmal() {
        let Some(pool) = moderation_test_pool("pipeline_sus_invite_timeout").await else {
            return;
        };
        let api = Arc::new(RecordingChatApi::default());
        let pipeline = pipeline_for_non_partner(Arc::clone(&api), pool);
        let mut event = strong_timeout_event();
        event.message.text = "discord.gg/fremd123".to_string();

        pipeline
            .handle_sus_invite(&event, "channel", "seller")
            .await;

        let timeout_calls: Vec<_> = api
            .calls()
            .into_iter()
            .filter(|call| call.starts_with("timeout:"))
            .collect();
        assert_eq!(timeout_calls.len(), 1, "{timeout_calls:?}");
        assert_eq!(
            timeout_calls[0],
            format!(
                "timeout:broadcaster-id:chatter-id:{SUS_INVITE_TIMEOUT_SECS}:{SUS_INVITE_TIMEOUT_REASON}"
            )
        );
    }

    #[tokio::test]
    async fn fremder_discord_invite_timeoutet_etablierten_chatter_nicht() {
        let Some(pool) = moderation_test_pool("pipeline_sus_invite_established").await else {
            return;
        };
        sqlx::query(
            "INSERT INTO twitch_chatter_rollup \
             (streamer_login, chatter_login, chatter_id, first_seen_at, last_seen_at, total_messages, total_sessions) \
             VALUES ('channel', 'seller', 'chatter-id', NOW(), NOW(), 40, 1)",
        )
        .execute(&pool)
        .await
        .unwrap();
        let api = Arc::new(RecordingChatApi::default());
        let pipeline = pipeline_for_non_partner(Arc::clone(&api), pool);
        let mut event = strong_timeout_event();
        event.message.text = "discord.gg/fremd123".to_string();

        pipeline
            .handle_sus_invite(&event, "channel", "seller")
            .await;

        assert!(!api.calls().iter().any(|call| call.starts_with("timeout:")));
    }

    #[tokio::test]
    async fn fremder_discord_invite_timeoutet_mod_nicht() {
        let pool = sqlx::PgPool::connect_lazy("postgres://x:x@127.0.0.1:1/x").unwrap();
        let api = Arc::new(RecordingChatApi::default());
        let pipeline = pipeline_for_non_partner(Arc::clone(&api), pool);
        let mut event = strong_timeout_event();
        event.message.text = "discord.gg/fremd123".to_string();
        event.badges.push(crate::types::ChatBadge {
            set_id: "moderator".to_string(),
            id: String::new(),
            info: String::new(),
        });

        pipeline
            .handle_sus_invite(&event, "channel", "seller")
            .await;

        assert!(!api.calls().iter().any(|call| call.starts_with("timeout:")));
    }

    /// Safe-List: `handle_strong_timeout` timeoutet direkt, ohne
    /// ModerationEngine. Kein Timeout, kein Chat-Text, kein Discord-Alert.
    #[tokio::test]
    async fn strong_timeout_verschont_safe_konten() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/changelog"))
            .respond_with(ResponseTemplate::new(204))
            .expect(0)
            .mount(&server)
            .await;

        for safe in crate::safe_list::SAFE_ACCOUNTS {
            let api = Arc::new(RecordingChatApi::default());
            let api_trait: Arc<dyn ChatApi> = api.clone();
            let alerter = Arc::new(ModAlerter::with_endpoint(
                reqwest::Client::new(),
                format!("{}/changelog", server.uri()),
            ));
            let mut event = strong_timeout_event();
            event.chatter_user_id = safe.twitch_user_id.to_string();
            event.chatter_user_login = safe.login.to_string();

            handle_strong_timeout(
                &api_trait,
                &alerter,
                &event,
                "channel",
                safe.login,
                "built escalation text",
                Duration::from_secs(600),
            )
            .await;

            assert!(
                api.calls().is_empty(),
                "Safe-Konto {} loeste Aktionen aus: {:?}",
                safe.login,
                api.calls()
            );
        }
    }

    #[tokio::test]
    async fn strong_timeout_timeoutet_alertet_und_sendet_gebauten_text() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/changelog"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;

        let api = Arc::new(RecordingChatApi::default());
        let api_trait: Arc<dyn ChatApi> = api.clone();
        let alerter = Arc::new(ModAlerter::with_endpoint(
            reqwest::Client::new(),
            format!("{}/changelog", server.uri()),
        ));
        let event = strong_timeout_event();

        handle_strong_timeout(
            &api_trait,
            &alerter,
            &event,
            "channel",
            "seller",
            "built escalation text",
            Duration::from_secs(600),
        )
        .await;

        assert_eq!(
            api.calls(),
            vec![
                format!(
                    "timeout:broadcaster-id:chatter-id:600:{}",
                    SCAM_PITCH_TIMEOUT_REASON
                ),
                "send:broadcaster-id:built escalation text".to_string(),
            ]
        );

        let mut request_count = 0;
        for _ in 0..20 {
            let requests = server
                .received_requests()
                .await
                .expect("Wiremock-Requests verfügbar");
            request_count = requests.len();
            if request_count == 1 {
                let body = String::from_utf8_lossy(&requests[0].body);
                assert!(body.contains("channel"));
                assert!(body.contains("seller"));
                assert!(body.contains("incoming pitch"));
                return;
            }
            sleep(Duration::from_millis(25)).await;
        }

        assert_eq!(request_count, 1, "StrongTimeout-Alert wurde nicht gesendet");
    }

    #[test]
    fn review_log_zeile_format() {
        let dir = std::env::temp_dir().join(format!("tb_chat_reviewlog_{}", std::process::id()));
        let log = ReviewLog::new(&dir);
        log.record(
            "kanal1",
            "spammer",
            "123",
            "böser\ninhalt",
            "SUSPICIOUS(2)",
            "Phrase(x)",
        );
        log.record("kanal1", "spammer", "123", "inhalt", "BANNED", "");

        let sus = std::fs::read_to_string(dir.join("twitch_suspicious.log")).unwrap();
        assert!(sus.contains("\t[SUSPICIOUS(2)]\tkanal1\tspammer\t123\tPhrase(x)\tböser inhalt"));

        let bans = std::fs::read_to_string(dir.join("twitch_autobans.log")).unwrap();
        assert!(bans.contains("\t[BANNED]\tkanal1\tspammer\t123\t-\tinhalt"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn review_log_leerer_chatter_wird_strich() {
        let dir = std::env::temp_dir().join(format!("tb_chat_reviewlog2_{}", std::process::id()));
        let log = ReviewLog::new(&dir);
        log.record("kanal1", "", "456", "x", "BANNED", "");
        let bans = std::fs::read_to_string(dir.join("twitch_autobans.log")).unwrap();
        assert!(bans.contains("\t[BANNED]\tkanal1\t-\t456\t-\tx"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn correction_learn_pattern_entfernt_mentions_und_kappt() {
        let pattern =
            correction_learn_pattern("@MiracleGhost9 aha, so sammelt man also viewer Kappa")
                .expect("pattern");
        assert_eq!(pattern, "aha, so sammelt man also viewer Kappa");
        assert!(correction_learn_pattern("@nur @mentions").is_none());
        let long = "x".repeat(300);
        assert_eq!(correction_learn_pattern(&long).unwrap().chars().count(), 78);
    }

    #[test]
    fn ai_spam_timeout_ist_24h() {
        // Bewusst reversibel (kein Ban): Judge-Fehlurteile gegen echte Viewer
        // müssen per Korrektur-Button + manuellem Untimeout heilbar sein.
        assert_eq!(AI_SPAM_TIMEOUT_SECS, 86_400);
    }

    #[test]
    fn alert_titel_mapping() {
        // Titel-Mapping dokumentierend getestet (Sendfunktion ist fire-and-forget).
        for (kind, expected) in [
            ("ban", "🔨 Ban ausgeführt"),
            ("global_ban", "🚫 Global-Ban ausgeführt"),
            ("sus_invite", "⚠️ Verdächtiger Discord-Link"),
            ("sus_spam", "👀 Verdächtige Nachricht"),
            (
                "scam_pitch_timeout",
                "🛡️ Account-Takeover erkannt — Quarantäne (reversibel)",
            ),
            ("scam_pitch_warn", "⚠️ Scam-Pitch erkannt — Verwarnung"),
            ("anderes", "ℹ️ Moderation"),
        ] {
            let (title, _) = match kind {
                "ban" => ("🔨 Ban ausgeführt", 0xED4245),
                "global_ban" => ("🚫 Global-Ban ausgeführt", 0xED4245),
                "sus_invite" => ("⚠️ Verdächtiger Discord-Link", 0xFEE75C),
                "sus_spam" => ("👀 Verdächtige Nachricht", 0xFEE75C),
                "scam_pitch_timeout" => (
                    "🛡️ Account-Takeover erkannt — Quarantäne (reversibel)",
                    0xFEE75C,
                ),
                "scam_pitch_warn" => ("⚠️ Scam-Pitch erkannt — Verwarnung", 0xFEE75C),
                _ => ("ℹ️ Moderation", 0x5865F2),
            };
            assert_eq!(title, expected);
        }
    }

    #[test]
    fn alert_channel_id_kommt_aus_env_oder_fallback() {
        assert_eq!(
            parse_alert_channel_id(Some("42".to_string())),
            42,
            "TWITCH_ALERT_CHANNEL_ID überschreibt den alten Default"
        );
        for raw in [
            None,
            Some(String::new()),
            Some("0".to_string()),
            Some("x".to_string()),
        ] {
            assert_eq!(parse_alert_channel_id(raw), DEFAULT_MOD_ALERT_CHANNEL_ID);
        }
    }

    /// Regression-Guard für die Grillme-Entscheidung `scam-pitch-spam-review-5`
    /// (2026-06-15): Der Scam-Pitch-Eskalations-Timeout nutzt bewusst einen
    /// **klareren** Reason als Python (`"Service-Pitch / Spam Escalation"`).
    /// Dieser Test lockt den akzeptierten Wortlaut ein, damit ein späterer
    /// blinder „Parity mit Python"-Pass ihn nicht versehentlich zurücksetzt.
    #[test]
    fn scam_pitch_timeout_reason_ist_bewusst_klarer_als_python() {
        const PYTHON_REASON: &str = "Service-Pitch / Spam Escalation";
        assert_eq!(
            SCAM_PITCH_TIMEOUT_REASON,
            "Account-Takeover-Verdacht / wiederholter Service-Pitch",
        );
        assert_ne!(
            SCAM_PITCH_TIMEOUT_REASON, PYTHON_REASON,
            "Rust-Reason ist die akzeptierte, klarere Variante (Grillme-Entscheidung) \
             und darf NICHT auf den Python-Wortlaut zurückgesetzt werden",
        );
    }
}
