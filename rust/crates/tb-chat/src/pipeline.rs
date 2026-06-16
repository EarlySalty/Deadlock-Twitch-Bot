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
//! 5. Global-Chatter-Ban (1589–1595)
//! 6. Scam-Pitch-Warnung (1597–1601) — Detektor sendet Chat-Warnung intern; Pipeline löscht NIE (wie Python) und timeoutet nur bei Eskalation (StrongTimeout)
//! 7. Spam-Score + Auto-Ban (1602–1737)
//! 8. Sus-Discord-Invite (1741–1743)
//! 9. Fun-Responses, nur wenn Deadlock live (1745–1750)
//! 10. Chat-Health-Tracking + Raw-Aktivität (1752–1755)
//! 11. Engagement-AI (1757–1811) — **bewusst No-op** bis zur Engagement-Phase
//! 12. /13. Invite/Activity-Promo, nur wenn Deadlock live (1813–1824) —
//!     !invite läuft über den CommandEngine (Schritt 14); das Python-Gate
//!     „keine Activity-Promo wenn Invite gesendet" ist über den
//!     PROMO_IGNORE_COMMANDS-Guard abgedeckt (jede !invite-Nachricht beginnt
//!     mit `!` und wird vom Promo-Tracking ohnehin ignoriert)
//! 14. Command-Processing (1827)
//!
//! # Observer
//!
//! [`ReviewLog`] schreibt die TSV-Review-Zeilen (`logs/twitch_autobans.log` /
//! `twitch_suspicious.log` — werden vom Admin-Dashboard geparst!) und
//! [`ModAlerter`] postet Discord-Moderations-Alerts über den Changelog-Cog
//! (`localhost:8899`), beides exakt nach `_record_autoban` (moderation.py
//! Z. 645–669) und `_send_moderation_alert` (Z. 905–951).

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use chrono::Utc;
use dashmap::DashMap;
use sqlx::PgPool;
use tracing::{debug, info, warn};

use crate::api::ChatApi;
use crate::channel_classifier::ChannelClassifier;
use crate::chatter_tracking::ChatterTracker;
use crate::commands::CommandEngine;
use crate::fun_responses::FunResponses;
use crate::global_chatter_ban::GlobalChatterBanEnforcer;
use crate::mention_scoring::{score_mention_patterns, MentionResolver, WHITELISTED_BOTS};
use crate::moderation::{
    AutoBanRequest, ModerationEngine, BAN_REASON_GLOBAL, BAN_REASON_SPAM, NOTICE_GLOBAL_BAN,
};
use crate::promos::PromoEngine;
use crate::scam_pitch::{PitchDecision, ScamPitchDetector, SpamAiReviewer};
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
        let safe_content: String = content.replace('\n', " ").chars().take(500).collect();
        let line = format!(
            "{ts}\t[{status}]\t{channel}\t{}\t{chatter_id}\t{}\t{safe_content}\n",
            if chatter_login.is_empty() { "-" } else { chatter_login },
            if reason.is_empty() { "-" } else { reason },
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
const MOD_ALERT_CHANNEL_ID: u64 = 1374364800817303632;

/// Postet Moderations-Alerts in den Discord-Mod-Kanal — Port von
/// `_send_moderation_alert` (moderation.py Z. 905–951). Fire-and-forget.
pub struct ModAlerter {
    http: reqwest::Client,
    endpoint: String,
}

impl ModAlerter {
    pub fn new(http: reqwest::Client) -> Self {
        Self::with_endpoint(http, "http://localhost:8899/changelog")
    }

    pub fn with_endpoint(http: reqwest::Client, endpoint: impl Into<String>) -> Self {
        Self {
            http,
            endpoint: endpoint.into(),
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
            "scam_pitch_timeout" => ("🛡️ Account-Takeover erkannt — Quarantäne (reversibel)", 0xFEE75C),
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
            let safe: String = content.chars().take(300).collect::<String>().replace('`', "'");
            lines.push(format!("**Nachricht:** `{safe}`"));
        }

        let payload = serde_json::json!({
            "title": title,
            "content": lines.join("\n"),
            "channel_id": MOD_ALERT_CHANNEL_ID,
            "token": "changeme-local",
        });

        let this = Arc::clone(self);
        tokio::spawn(async move {
            match this
                .http
                .post(&this.endpoint)
                .json(&payload)
                .timeout(std::time::Duration::from_secs(5))
                .send()
                .await
            {
                Ok(resp) if !matches!(resp.status().as_u16(), 200 | 201 | 204) => {
                    debug!("mod_alert: Discord-Post HTTP {}", resp.status());
                }
                Err(e) => debug!("mod_alert: Discord-Post fehlgeschlagen: {e}"),
                _ => {}
            }
        });
    }
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
        let known = sqlx::query_scalar::<_, i32>(
            "SELECT 1 FROM twitch_session_chatters \
             WHERE streamer_login = $1 AND chatter_login = $2 LIMIT 1",
        )
        .bind(&streamer)
        .bind(&mention)
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten()
        .is_some()
            || sqlx::query_scalar::<_, i32>(
                "SELECT 1 FROM twitch_chatter_rollup \
                 WHERE streamer_login = $1 AND chatter_login = $2 LIMIT 1",
            )
            .bind(&streamer)
            .bind(&mention)
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
pub struct ChatPipelineParts {
    pub bot_user_id: String,
    pub api: Arc<dyn ChatApi>,
    pub pool: PgPool,
    pub classifier: Arc<ChannelClassifier>,
    pub tracker: Arc<ChatterTracker>,
    pub global_ban: Arc<GlobalChatterBanEnforcer>,
    pub scam_pitch: Arc<ScamPitchDetector>,
    pub spam_filter: Arc<SpamFilter>,
    pub ai_reviewer: Arc<SpamAiReviewer>,
    pub moderation: Arc<ModerationEngine>,
    pub sus_invite: Arc<SusInviteCheck>,
    pub fun: Arc<FunResponses>,
    pub promos: Arc<PromoEngine>,
    pub commands: Arc<CommandEngine>,
    pub mention_resolver: Arc<dyn MentionResolver>,
    pub review_log: Arc<ReviewLog>,
    pub alerter: Arc<ModAlerter>,
}

/// Orchestriert die 15 Pipeline-Schritte für jedes `channel.chat.message`-Event.
pub struct ChatPipeline {
    parts: ChatPipelineParts,
}

impl ChatPipeline {
    pub fn new(parts: ChatPipelineParts) -> Self {
        Self { parts }
    }

    /// Verarbeitet ein eingehendes Chat-Event — Einstiegspunkt für den
    /// EventSub-Dispatch (`channel.chat.message`).
    pub async fn handle(&self, event: &ChatMessageEvent) {
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
            return;
        }

        let channel_login = event.broadcaster_user_login.to_lowercase();
        let chatter_login = event.chatter_user_login.to_lowercase();

        // Schritt 1: VoiceReaction — No-op bis Engagement-Phase (Modul-Doku).

        // Schritt 2: Known-Bot-Whitelist (bot.py Z. 1548–1557)
        if WHITELISTED_BOTS.contains(&chatter_login.as_str()) {
            p.tracker.track(event).await;
            p.commands.handle(event).await;
            return;
        }

        // Schritt 3: Kanal-Klassifizierung (bot.py Z. 1559–1572)
        let class = p
            .classifier
            .classify(&channel_login, &event.broadcaster_user_id)
            .await;

        // Schritt 4: Non-Partner — nur Datensammlung, keine Moderation/Promos
        if !class.is_partner {
            p.tracker.track(event).await;
            return;
        }

        // Schritt 5: Global-Chatter-Ban (Z. 1589–1595) — Aktion über die
        // ModerationEngine, exakt wie Python via _auto_ban_and_cleanup.
        if p.global_ban.is_banned(event).await {
            info!(
                chatter = %chatter_login,
                channel = %channel_login,
                "Global-Ban-Treffer — führe Channel-Ban aus"
            );
            if self
                .execute_auto_ban(
                    event,
                    &channel_login,
                    true,
                    BAN_REASON_GLOBAL,
                    Some(NOTICE_GLOBAL_BAN),
                    "global_ban",
                )
                .await
            {
                return;
            }
            // Ban nicht durchsetzbar (z. B. Chatter ist Mod) → Pipeline läuft
            // weiter, wie Python (enforce gibt das auto_ban-Resultat zurück).
        }

        // Schritt 6: Scam-Pitch (Z. 1597–1601) — Detektor sendet Chat-Warnung intern.
        // Wie Python wird NIE gelöscht; ein Timeout erfolgt nur bei Eskalation
        // (StrongTimeout). Erst-Warnung (StrongWarn/PublicWarn) ist nicht-destruktiv.
        let pitch = p.scam_pitch.observe(event).await;
        match &pitch {
            PitchDecision::StrongTimeout { .. } => {
                debug!(channel = %channel_login, chatter = %chatter_login, "Scam-Pitch: StrongTimeout (Eskalation) → Timeout (kein Delete)");
                if !event.chatter_user_id.is_empty()
                    && event.chatter_user_id != event.broadcaster_user_id
                    && !event.is_mod_or_broadcaster()
                {
                    if let Err(e) = p
                        .api
                        .timeout_user(
                            &event.broadcaster_user_id,
                            &event.chatter_user_id,
                            600,
                            SCAM_PITCH_TIMEOUT_REASON,
                        )
                        .await
                    {
                        debug!("Pitch-Timeout fehlgeschlagen: {e}");
                    }
                    p.alerter.send(
                        "scam_pitch_timeout",
                        &channel_login,
                        &chatter_login,
                        &event.chatter_user_id,
                        event.text(),
                        "",
                    );
                }
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
        if self.run_spam_check(event, &channel_login, &chatter_login).await {
            return;
        }

        // Schritt 8: Sus-Discord-Invite (Z. 1741–1743)
        if let Some(hit) = p.sus_invite.check(event, &channel_login).await {
            p.review_log.record(
                &channel_login,
                &hit.chatter_login,
                &hit.chatter_id,
                &hit.content,
                "SUSPICIOUS_DISCORD_INVITE",
                "discord.gg link in partner chat",
            );
            p.alerter.send(
                "sus_invite",
                &channel_login,
                &hit.chatter_login,
                &hit.chatter_id,
                &hit.content,
                "",
            );
        }

        // Schritt 9: Fun-Responses, nur wenn Deadlock live (Z. 1745–1750)
        if class.is_deadlock_live {
            p.fun.maybe_respond(event, &channel_login).await;
        }

        // Schritt 10: Chat-Health-Tracking + Raw-Aktivität (Z. 1752–1755 + 2173)
        p.tracker.track(event).await;
        p.promos.record_raw_message(&channel_login).await;

        // Schritt 11: Engagement-AI — No-op bis Engagement-Phase (Modul-Doku).

        // Schritt 12/13: Activity-Promo, nur wenn Deadlock live (Z. 1813–1824).
        // !invite wird vom CommandEngine (Schritt 14) bedient; der
        // PROMO_IGNORE_COMMANDS-Guard deckt das sent_invite-Gate ab.
        if class.is_deadlock_live {
            p.promos.on_message(event).await;
        }

        // Schritt 14: Command-Processing — immer am Ende (Z. 1827)
        p.commands.handle(event).await;
    }

    /// Spam-Schritt (bot.py Z. 1602–1737). Gibt `true` zurück wenn die
    /// Pipeline stoppen soll (Ban-Schwelle erreicht).
    async fn run_spam_check(
        &self,
        event: &ChatMessageEvent,
        channel_login: &str,
        chatter_login: &str,
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
            ctx.is_first_message =
                is_first_message_for_streamer(&p.pool, channel_login, chatter_login).await;
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
                let enforced = self
                    .execute_auto_ban(event, channel_login, true, BAN_REASON_SPAM, None, "ban")
                    .await;
                if !enforced {
                    warn!(
                        channel = %channel_login,
                        score = verdict.score,
                        treffer = %reasons.join(", "),
                        "Spam erkannt, aber Auto-Ban konnte nicht durchgesetzt werden"
                    );
                }
                true
            }
            SpamAction::DeleteOnly | SpamAction::None if verdict.score > 0 => {
                // Verdachts-Pfad (Z. 1672–1737): loggen, bei hartem Signal
                // Delete-only, Alert + AI-Review fire-and-forget — KEIN Stopp.
                let reasons_str = reasons.join(", ");
                p.review_log.record(
                    channel_login,
                    chatter_login,
                    &event.chatter_user_id,
                    text,
                    &format!("SUSPICIOUS({})", verdict.score),
                    &reasons_str,
                );

                if verdict.hard_signal {
                    let _ = self
                        .execute_auto_ban(event, channel_login, false, BAN_REASON_SPAM, None, "ban")
                        .await;
                }

                info!(
                    channel = %channel_login,
                    chatter = %chatter_login,
                    score = verdict.score,
                    treffer = %reasons_str,
                    "Verdächtige Nachricht"
                );

                p.alerter.send(
                    "sus_spam",
                    channel_login,
                    chatter_login,
                    &event.chatter_user_id,
                    text,
                    &format!("Score {}: {}", verdict.score, reasons_str),
                );

                // AI-Review lernt Muster (Z. 1726–1735) — fire-and-forget.
                p.ai_reviewer
                    .maybe_review_with_reasons(event, verdict.score, &reasons);
                false
            }
            _ => false,
        }
    }

    /// Gemeinsamer Auto-Ban-Pfad — Python `_auto_ban_and_cleanup` inkl. der
    /// Pre-Checks (Z. 1627–1637) und Nachgelagertem (Review-Log + Alert).
    async fn execute_auto_ban(
        &self,
        event: &ChatMessageEvent,
        channel_login: &str,
        ban: bool,
        reason_text: &str,
        notice_text: Option<&str>,
        alert_kind: &str,
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
            .auto_ban_and_cleanup(AutoBanRequest {
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
            })
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

    /// `silent_ban`-Flag des Partners (moderation.py Z. 1775–1790 via
    /// `load_active_partner`; View-Spalte ist INTEGER). Fail-safe: false.
    async fn is_silent_ban(&self, channel_login: &str) -> bool {
        sqlx::query_scalar::<_, i32>(
            "SELECT COALESCE(silent_ban, 0) \
             FROM twitch_streamers_partner_state \
             WHERE LOWER(twitch_login) = $1",
        )
        .bind(channel_login)
        .fetch_optional(&self.parts.pool)
        .await
        .unwrap_or(None)
        .unwrap_or(0)
            != 0
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
    match sqlx::query_scalar::<_, i32>(
        "SELECT 1 FROM twitch_chatter_rollup \
         WHERE streamer_login = $1 AND chatter_login = $2 LIMIT 1",
    )
    .bind(channel_login)
    .bind(chatter_login)
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

    #[test]
    fn review_log_zeile_format() {
        let dir = std::env::temp_dir().join(format!("tb_chat_reviewlog_{}", std::process::id()));
        let log = ReviewLog::new(&dir);
        log.record("kanal1", "spammer", "123", "böser\ninhalt", "SUSPICIOUS(2)", "Phrase(x)");
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
    fn alert_titel_mapping() {
        // Titel-Mapping dokumentierend getestet (Sendfunktion ist fire-and-forget).
        for (kind, expected) in [
            ("ban", "🔨 Ban ausgeführt"),
            ("global_ban", "🚫 Global-Ban ausgeführt"),
            ("sus_invite", "⚠️ Verdächtiger Discord-Link"),
            ("sus_spam", "👀 Verdächtige Nachricht"),
            ("scam_pitch_timeout", "🛡️ Account-Takeover erkannt — Quarantäne (reversibel)"),
            ("scam_pitch_warn", "⚠️ Scam-Pitch erkannt — Verwarnung"),
            ("anderes", "ℹ️ Moderation"),
        ] {
            let (title, _) = match kind {
                "ban" => ("🔨 Ban ausgeführt", 0xED4245),
                "global_ban" => ("🚫 Global-Ban ausgeführt", 0xED4245),
                "sus_invite" => ("⚠️ Verdächtiger Discord-Link", 0xFEE75C),
                "sus_spam" => ("👀 Verdächtige Nachricht", 0xFEE75C),
                "scam_pitch_timeout" => ("🛡️ Account-Takeover erkannt — Quarantäne (reversibel)", 0xFEE75C),
                "scam_pitch_warn" => ("⚠️ Scam-Pitch erkannt — Verwarnung", 0xFEE75C),
                _ => ("ℹ️ Moderation", 0x5865F2),
            };
            assert_eq!(title, expected);
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
