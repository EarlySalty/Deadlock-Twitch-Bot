//! Moderations-Engine — AutoBan, TimeoutGuard, OutboundSuppression.
//!
//! Enthält:
//! - [`HelixChatClient`] — impl [`ChatApi`] via HelixClient + BotTokenManager
//!   (2-Attempt-Muster: 401 → force_refresh → einmal retry).
//! - [`ModerationEngine`] — AutoBan-Ablauf (Delete + Ban, silent_ban-Guard,
//!   _last_autoban In-Memory + DB-Absicherung).
//! - [`TimeoutGuard`] — 2 Timeouts/Tag oder 5/Woche → 7-Tage-Stummschaltung
//!   + Werbefrei-Pitch-Flag.
//! - [`OutboundSuppressionStore`] — twitch_outbound_chat_suppressions,
//!   promo/recruitment=7d, partner_raid=3d.
//!
//! Port: `bot/chat/moderation.py:1293–1903`, `bot/chat/timeout_guard.py`.

use crate::api::{AnnouncementOutcome, BanOutcome, ChatApi};
use crate::commands::{AutobanEntry, LastAutobanStore};
use crate::promos::OutboundSuppressionCheck as PromoSuppressionCheck;
use crate::suppression_guard::{
    DbManualPartnerOptOutCheck, ManualPartnerOptOutCheck, SuppressionGuardChatApi,
};
use crate::token::BotTokenManager;
use crate::types::SendOutcome;
use async_trait::async_trait;
use chrono::DateTime;
use chrono::{Duration, Utc};
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tracing::{debug, warn};

// ---------------------------------------------------------------------------
// Konstanten — wörtlich aus dem Vertrag
// ---------------------------------------------------------------------------

/// Standard-Ban-Reason für Spam (moderation.py Z. 250).
pub const BAN_REASON_SPAM: &str = "Automatischer Spam-Ban (Bot-Phrase)";

/// Ban-Reason für netzwerkweite Bans (moderation.py Z. 253).
pub const BAN_REASON_GLOBAL: &str = "Netzwerkweiter Ban: Verstoß gegen Community-Richtlinien";

/// Chat-Notice nach Spam-Ban (moderation.py Z. 256).
/// Platzhalter `{login}` muss ersetzt werden.
pub const NOTICE_SPAM_BAN: &str =
    "🛡️ Auto-Mod: {login} wurde wegen Spam-Verdacht gebannt. (!unban zum Rückgängigmachen)";

/// Chat-Notice nach globalem Ban (moderation.py Z. 257).
/// Platzhalter `{login}` muss ersetzt werden.
pub const NOTICE_GLOBAL_BAN: &str =
    "🛡️ {login} steht netzwerkweit auf der Bannliste (Verstoß gegen die Community-Richtlinien) und wurde hier gebannt.";

/// Suppression-Dauer für `promo`-Quelle: 7 Tage (moderation.py Z. 364).
pub const SUPPRESSION_PROMO_SECS: i64 = 7 * 24 * 3600; // 604800

/// Suppression-Dauer für `recruitment`-Quelle: 7 Tage (moderation.py Z. 365).
pub const SUPPRESSION_RECRUITMENT_SECS: i64 = 7 * 24 * 3600; // 604800

/// Suppression-Dauer für `partner_raid`-Quelle: 3 Tage (moderation.py Z. 366).
pub const SUPPRESSION_PARTNER_RAID_SECS: i64 = 3 * 24 * 3600; // 259200

/// Reason-Code der Suppression auslöst (moderation.py Z. 373).
pub const SUPPRESSION_TRIGGER_CODE: &str = "channel_settings";

/// Timeout-Guard: Tages-Schwelle (timeout_guard.py `_MUTE_DAILY_THRESHOLD`).
pub const TIMEOUT_MUTE_DAILY_THRESHOLD: usize = 2;

/// Timeout-Guard: Wochen-Schwelle (timeout_guard.py `_MUTE_WEEKLY_THRESHOLD`).
pub const TIMEOUT_MUTE_WEEKLY_THRESHOLD: usize = 5;

/// Timeout-Guard: Stummschaltungs-Dauer 7 Tage in Sekunden.
pub const TIMEOUT_MUTE_DURATION_SECS: u64 = 7 * 24 * 3600; // 604800

/// Timeout-Guard: Mindest-Abstand zwischen Werbefrei-Pitches (24h).
pub const TIMEOUT_PITCH_COOLDOWN_SECS: u64 = 24 * 3600;

/// Drop-Codes, die einen Timeout-Guard-Record auslösen.
pub const BOT_TIMEOUT_DROP_CODES: &[&str] = &["sender_banned", "sender_timedout"];

/// Werbefrei-Pitch-URL (timeout_guard.py).
pub const WERBEFREI_PITCH_URL: &str = "https://deutsche-deadlock-community.de/twitch/pricing";

/// Werbefrei-Pitch-Nachricht (timeout_guard.py).
pub const WERBEFREI_PITCH_MSG: &str =
    "Kurzer Hinweis: Beim letzten Stream wurde der Bot in diesem Chat getimed outed 🙈 \
     Falls die automatischen Promo-Nachrichten stören – es gibt ein Werbefrei-Abo, \
     das alle Bot-Features ohne automatische Nachrichten bietet: \
     https://deutsche-deadlock-community.de/twitch/pricing";

fn log_autoban_notice_send_result(
    channel_login: &str,
    broadcaster_id: &str,
    result: Result<SendOutcome, String>,
) {
    match result {
        Ok(SendOutcome::Sent) => {}
        Ok(SendOutcome::Dropped { code, message }) => {
            warn!(
                channel = %channel_login,
                broadcaster_id = %broadcaster_id,
                code = %code,
                message = %message,
                "AutoBan-Notice von Twitch verworfen"
            );
        }
        Ok(SendOutcome::HttpError { status, body }) => {
            warn!(
                channel = %channel_login,
                broadcaster_id = %broadcaster_id,
                status = status,
                body = %body,
                "AutoBan-Notice HTTP-Fehler"
            );
        }
        Err(err) => {
            warn!(
                channel = %channel_login,
                broadcaster_id = %broadcaster_id,
                err = %err,
                "AutoBan-Notice send fehlgeschlagen"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// HelixChatClient — impl ChatApi
// ---------------------------------------------------------------------------

/// Prod-Implementierung von [`ChatApi`] via [`HelixClient`] + [`BotTokenManager`].
///
/// 2-Attempt-Muster: bei HTTP-401 im ersten Versuch → `force_refresh` → retry.
///
/// Port: `moderation.py:_token_manager + 2-Attempt-Loop` (Z. 1631, 1679).
pub struct HelixChatClient {
    helix: Arc<tb_transport_twitch::HelixClient>,
    token_mgr: Arc<BotTokenManager>,
}

impl HelixChatClient {
    /// Erstellt einen neuen HelixChatClient.
    pub fn new(
        helix: Arc<tb_transport_twitch::HelixClient>,
        token_mgr: Arc<BotTokenManager>,
    ) -> Self {
        Self { helix, token_mgr }
    }
}

#[async_trait]
impl ChatApi for HelixChatClient {
    /// Sendet eine Chat-Nachricht — 2-Attempt: 401 → force_refresh → retry.
    /// sender_id = Bot-User-ID, intern via token_mgr bezogen.
    async fn send_message(
        &self,
        broadcaster_id: &str,
        message: &str,
    ) -> Result<SendOutcome, String> {
        let sender_id = self.token_mgr.bot_user_id().await;
        for attempt in 0..2usize {
            let force = attempt > 0;
            let token = self.token_mgr.get_valid_token(force).await?;
            match self
                .helix
                .send_chat_message(broadcaster_id, &sender_id, message, &token)
                .await
            {
                Ok(SendOutcome::HttpError { status: 401, body }) if attempt == 0 => {
                    debug!("send_message 401, force_refresh + retry (body: {body})");
                    continue;
                }
                Ok(outcome) => return Ok(outcome),
                Err(e) => return Err(e.to_string()),
            }
        }
        Ok(SendOutcome::HttpError {
            status: 401,
            body: "nach force_refresh noch 401".to_string(),
        })
    }

    /// Sendet Whisper — 2-Attempt: 401 → force_refresh → retry.
    async fn send_whisper(&self, to_user_id: &str, message: &str) -> Result<bool, String> {
        let from_user_id = self.token_mgr.bot_user_id().await;
        for attempt in 0..2usize {
            let force = attempt > 0;
            let token = self.token_mgr.get_valid_token(force).await?;
            match self
                .helix
                .send_whisper(&from_user_id, to_user_id, message, &token)
                .await
            {
                Ok(outcome) if outcome.accepted => return Ok(true),
                Ok(outcome) if outcome.status == Some(401) && attempt == 0 => continue,
                Ok(outcome) => {
                    warn!(
                        to_user_id,
                        status = ?outcome.status,
                        detail = ?outcome.detail,
                        "Whisper nicht akzeptiert"
                    );
                    return Ok(false);
                }
                Err(e) => return Err(e.to_string()),
            }
        }
        Ok(false)
    }

    async fn send_announcement(
        &self,
        broadcaster_id: &str,
        message: &str,
        color: &str,
    ) -> Result<bool, String> {
        self.send_announcement_detailed(broadcaster_id, message, color)
            .await
            .map(|outcome| outcome.accepted)
    }

    /// Sendet Ankündigung — 2-Attempt.
    /// moderator_id = Bot-User-ID, intern via token_mgr bezogen.
    async fn send_announcement_detailed(
        &self,
        broadcaster_id: &str,
        message: &str,
        color: &str,
    ) -> Result<AnnouncementOutcome, String> {
        let moderator_id = self.token_mgr.bot_user_id().await;
        for attempt in 0..2usize {
            let force = attempt > 0;
            let token = match self.token_mgr.get_valid_token(force).await {
                Ok(t) => t,
                Err(e) => return Err(e),
            };
            match self
                .helix
                .send_announcement_detailed(broadcaster_id, &moderator_id, message, color, &token)
                .await
            {
                Ok(outcome) if outcome.accepted => return Ok(outcome),
                Ok(outcome) if outcome.status == Some(401) && attempt == 0 => {
                    continue;
                }
                Ok(outcome) => return Ok(outcome),
                Err(e) => {
                    warn!("Announcement-Fehler: {e}");
                    return Err(e.to_string());
                }
            }
        }
        Ok(AnnouncementOutcome {
            accepted: false,
            status: Some(401),
            detail: Some("nach force_refresh noch 401".to_string()),
        })
    }

    /// Permanenter Ban — 2-Attempt bei 401.
    /// moderator_id = Bot-User-ID, intern via token_mgr bezogen.
    async fn ban_user(
        &self,
        broadcaster_id: &str,
        target_user_id: &str,
        reason: &str,
    ) -> Result<BanOutcome, String> {
        let moderator_id = self.token_mgr.bot_user_id().await;
        self.ban_internal(broadcaster_id, &moderator_id, target_user_id, reason, None)
            .await
    }

    /// Timeout (zeitlich begrenzter Ban) — 2-Attempt bei 401.
    /// moderator_id = Bot-User-ID, intern via token_mgr bezogen.
    async fn timeout_user(
        &self,
        broadcaster_id: &str,
        target_user_id: &str,
        duration_secs: u32,
        reason: &str,
    ) -> Result<BanOutcome, String> {
        let moderator_id = self.token_mgr.bot_user_id().await;
        self.ban_internal(
            broadcaster_id,
            &moderator_id,
            target_user_id,
            reason,
            Some(duration_secs),
        )
        .await
    }

    /// Hebt Ban auf — 2-Attempt bei 401.
    async fn unban_user(&self, broadcaster_id: &str, target_user_id: &str) -> Result<bool, String> {
        let moderator_id = self.token_mgr.bot_user_id().await;
        for attempt in 0..2usize {
            let force = attempt > 0;
            let token = match self.token_mgr.get_valid_token(force).await {
                Ok(t) => t,
                Err(e) => return Err(e),
            };
            match self
                .helix
                .unban_user(broadcaster_id, &moderator_id, target_user_id, &token)
                .await
            {
                Ok(BanOutcome::Failed { status: 401, body }) if attempt == 0 => {
                    debug!("unban_user 401, retry (body: {body})");
                    continue;
                }
                Ok(BanOutcome::Unbanned) => return Ok(true),
                Ok(_) => return Ok(false),
                Err(e) => return Err(e.to_string()),
            }
        }
        Err("nach force_refresh noch 401".to_string())
    }

    /// Löscht Nachricht — 2-Attempt bei 401.
    async fn delete_message(&self, broadcaster_id: &str, message_id: &str) -> Result<bool, String> {
        let moderator_id = self.token_mgr.bot_user_id().await;
        for attempt in 0..2usize {
            let force = attempt > 0;
            let token = match self.token_mgr.get_valid_token(force).await {
                Ok(t) => t,
                Err(e) => return Err(e),
            };
            match self
                .helix
                .delete_chat_message(broadcaster_id, &moderator_id, message_id, &token)
                .await
            {
                Ok(true) => return Ok(true),
                Ok(false) if attempt == 0 => continue,
                Ok(false) => return Ok(false),
                Err(e) => {
                    warn!("delete_message Fehler: {e}");
                    return Err(e.to_string());
                }
            }
        }
        Ok(false)
    }

    /// Account-Erstellungsdatum via Helix GET /users?id=.
    async fn user_created_at(&self, user_id: &str) -> Result<Option<DateTime<Utc>>, String> {
        let token = self.token_mgr.get_valid_token(false).await?;
        let users = self
            .helix
            .get_users_created_at(&[user_id], &token)
            .await
            .map_err(|e| e.to_string())?;
        match users.into_iter().next() {
            None => Ok(None),
            Some(u) => {
                let dt = u
                    .created_at
                    .parse::<DateTime<Utc>>()
                    .map_err(|e| format!("created_at parse error: {e}"))?;
                Ok(Some(dt))
            }
        }
    }

    /// Löst Login-Name → user_id via Helix GET /users?login= auf.
    async fn resolve_user_id(&self, login: &str) -> Result<Option<String>, String> {
        let token = self.token_mgr.get_valid_token(false).await?;
        self.helix
            .get_user_by_login(login, &token)
            .await
            .map(|opt| opt.map(|u| u.id))
            .map_err(|e| e.to_string())
    }

    /// Bot-User-ID (sender_id / moderator_id für alle Helix-Aufrufe).
    async fn bot_user_id(&self) -> String {
        self.token_mgr.bot_user_id().await
    }
}

impl HelixChatClient {
    /// Interner Ban-Helfer für ban_user (None) und timeout_user (Some(duration)).
    async fn ban_internal(
        &self,
        broadcaster_id: &str,
        moderator_id: &str,
        user_id: &str,
        reason: &str,
        duration_secs: Option<u32>,
    ) -> Result<BanOutcome, String> {
        for attempt in 0..2usize {
            let force = attempt > 0;
            let token = match self.token_mgr.get_valid_token(force).await {
                Ok(t) => t,
                Err(e) => return Err(e),
            };
            match self
                .helix
                .ban_user(
                    broadcaster_id,
                    moderator_id,
                    user_id,
                    reason,
                    duration_secs,
                    &token,
                )
                .await
            {
                Ok(BanOutcome::Failed { status: 401, body }) if attempt == 0 => {
                    debug!("ban_user 401, retry (body: {body})");
                    continue;
                }
                Ok(outcome) => return Ok(outcome),
                Err(e) => return Err(e.to_string()),
            }
        }
        Ok(BanOutcome::Failed {
            status: 401,
            body: "nach force_refresh noch 401".to_string(),
        })
    }
}

// ---------------------------------------------------------------------------
// ModerationEngine
// ---------------------------------------------------------------------------

/// Gespeicherter letzter Auto-Ban pro Kanal (In-Memory).
///
/// Port: `self._last_autoban[channel_key]` (moderation.py Z. 235).
#[derive(Debug, Clone)]
pub struct AutoBanRecord {
    pub user_id: String,
    pub login: String,
    pub content: String,
    pub ts: DateTime<Utc>,
}

/// Trait: Prüft ob ein User in einer Kanal-Sitzung als Moderator oder
/// Broadcaster agiert (Schutz gegen Selbst-Ban). Injiziert vom Orchestrator.
#[async_trait]
pub trait ChannelGuardPort: Send + Sync {
    /// Gibt `true` zurück wenn `user_id` Moderator oder Broadcaster in `channel_login` ist.
    async fn is_mod_or_broadcaster(&self, channel_login: &str, user_id: &str) -> bool;

    /// Prüft ob `silent_ban` für diesen Partner-Kanal aktiv ist.
    /// Port: `load_active_partner(conn, twitch_user_id=...)["silent_ban"]` (moderation.py Z. 238).
    async fn is_silent_ban(&self, broadcaster_id: &str, pool: &PgPool) -> bool;
}

/// Parameter-Struct für [`ModerationEngine::auto_ban_and_cleanup`].
///
/// Vermeidet die >7-Argumente-Grenze (Clippy `too_many_arguments`).
/// Port: `moderation.py:_auto_ban_and_cleanup` (Z. 1561–1829).
pub struct AutoBanRequest<'a> {
    pub channel_login: &'a str,
    pub broadcaster_id: &'a str,
    pub bot_id: &'a str,
    pub chatter_login: &'a str,
    pub chatter_id: &'a str,
    pub message_id: &'a str,
    pub content: &'a str,
    /// `true` = permanenter Ban, `false` = nur Nachricht löschen.
    pub ban: bool,
    pub reason_text: &'a str,
    /// Überschreibt den Standard-Notice-Text (Platzhalter `{login}`).
    pub notice_text: Option<&'a str>,
    /// `true` = kein Chat-Notice (aus `silent_ban`-Prüfung).
    pub silent: bool,
}

/// Nachvollziehbarer Ausloeser einer automatischen Moderationsaktion.
pub struct ModerationEvidence<'a> {
    pub source_path: &'a str,
    pub reason: &'a str,
    pub score: Option<f32>,
    pub account_age_days: Option<i64>,
}

/// Moderations-Engine — koordiniert Delete + Ban + DB-Persistierung.
///
/// Port: `moderation.py:_auto_ban_and_cleanup` (Z. 1561–1829).
pub struct ModerationEngine {
    api: Arc<dyn ChatApi>,
    pool: PgPool,
    notice_suppression: Option<Arc<dyn PromoSuppressionCheck>>,
    notice_manual_opt_out: Arc<dyn ManualPartnerOptOutCheck>,
    /// In-Memory-Store: channel_login (lowercase) → letzter AutoBan.
    last_autoban: Arc<Mutex<HashMap<String, AutoBanRecord>>>,
}

impl ModerationEngine {
    /// Erstellt eine neue ModerationEngine.
    pub fn new(api: Arc<dyn ChatApi>, pool: PgPool) -> Self {
        let notice_manual_opt_out = Arc::new(DbManualPartnerOptOutCheck::new(pool.clone()));
        Self {
            api,
            pool,
            notice_suppression: None,
            notice_manual_opt_out,
            last_autoban: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Verdrahtet den zentralen Outbound-Suppression-Guard fuer Auto-Ban-Notices.
    pub fn with_notice_suppression(mut self, suppression: Arc<dyn PromoSuppressionCheck>) -> Self {
        self.notice_suppression = Some(suppression);
        self
    }

    /// Ersetzt den Opt-out-Checker fuer Tests.
    #[cfg(test)]
    pub fn with_notice_manual_opt_out_check(
        mut self,
        check: Arc<dyn ManualPartnerOptOutCheck>,
    ) -> Self {
        self.notice_manual_opt_out = check;
        self
    }

    async fn send_autoban_notice(&self, channel_login: &str, broadcaster_id: &str, notice: &str) {
        let result = if let Some(suppression) = self.notice_suppression.as_ref() {
            let api = SuppressionGuardChatApi::new(
                Arc::clone(&self.api),
                Arc::clone(suppression),
                Arc::clone(&self.notice_manual_opt_out),
                "promo",
                channel_login,
            );
            api.send_message(broadcaster_id, notice).await
        } else {
            self.api.send_message(broadcaster_id, notice).await
        };
        log_autoban_notice_send_result(channel_login, broadcaster_id, result);
    }

    /// Führt AutoBan aus: 1. Nachricht löschen, 2. Ban (wenn `req.ban=true`).
    ///
    /// Rückgabe: `true` wenn mindestens eine Aktion erfolgreich war.
    ///
    /// Port: `moderation.py:_auto_ban_and_cleanup` (Z. 1561–1829).
    pub async fn auto_ban_and_cleanup(&self, req: AutoBanRequest<'_>) -> bool {
        let source_path = if req.reason_text == BAN_REASON_GLOBAL {
            "global_ban"
        } else if req.reason_text == BAN_REASON_SPAM {
            "spam"
        } else {
            "scam"
        };
        let reason = req.reason_text;
        self.auto_ban_and_cleanup_with_evidence(
            req,
            ModerationEvidence {
                source_path,
                reason,
                score: None,
                account_age_days: None,
            },
        )
        .await
    }

    /// Wie [`Self::auto_ban_and_cleanup`], aber mit beweiskraeftigem Regelkontext.
    pub async fn auto_ban_and_cleanup_with_evidence(
        &self,
        req: AutoBanRequest<'_>,
        evidence: ModerationEvidence<'_>,
    ) -> bool {
        let AutoBanRequest {
            channel_login,
            broadcaster_id,
            bot_id: _,
            chatter_login,
            chatter_id,
            message_id,
            content,
            ban,
            reason_text,
            notice_text,
            silent,
        } = req;

        // Schritt 0: Safe-List. Hier laufen alle Auto-Ban-Pfade der Chat-
        // Pipeline zusammen (Spam, Scam, Crew, Global-Ban). Der Guard steht vor
        // dem Message-Delete, sonst verlöre ein Safe-Konto trotzdem Nachrichten.
        if crate::safe_list::is_safe(Some(chatter_id), chatter_login) {
            warn!(
                channel = %channel_login,
                chatter = %chatter_login,
                reason = %reason_text,
                "AutoBan gegen Safe-List-Konto unterdrückt"
            );
            self.persist_autoban_record(
                channel_login,
                chatter_id,
                chatter_login,
                content,
                "suppressed_safe_list",
                &evidence,
                false,
            )
            .await;
            self.update_message_moderation(message_id, "suppressed_safe_list", evidence.reason)
                .await;
            return false;
        }

        // Schritt 1: Nachricht löschen (moderation.py Z. 1631–1666)
        let deleted = match self.api.delete_message(broadcaster_id, message_id).await {
            Ok(deleted) => deleted,
            Err(error) => {
                warn!(
                    %error,
                    channel = %channel_login,
                    chatter = %chatter_login,
                    message_id = %message_id,
                    "AutoBan-Cleanup: Message-Delete fehlgeschlagen"
                );
                false
            }
        };

        if !ban {
            if !deleted {
                return false;
            }
            // Delete-Only-Pfad (moderation.py Z. 259–261)
            self.persist_autoban_record(
                channel_login,
                chatter_id,
                chatter_login,
                content,
                "delete_only",
                &evidence,
                true,
            )
            .await;
            self.update_message_moderation(message_id, "delete_only", evidence.reason)
                .await;
            return true;
        }

        // Schritt 2: Ban (moderation.py Z. 1679–1816)
        let outcome = match self
            .api
            .ban_user(broadcaster_id, chatter_id, reason_text)
            .await
        {
            Ok(o) => o,
            Err(e) => {
                warn!("AutoBan Fehler: {e}");
                return false;
            }
        };

        match &outcome {
            BanOutcome::Banned | BanOutcome::AlreadyBanned => {
                // In-Memory immer (beide Pfade).
                self.persist_autoban_record(
                    channel_login,
                    chatter_id,
                    chatter_login,
                    content,
                    "ban",
                    &evidence,
                    true,
                )
                .await;
                self.update_message_moderation(message_id, "ban", evidence.reason)
                    .await;

                // Nur ein FRISCHER Ban speist die öffentliche recent-bans-Statistik
                // und sendet die Chat-Notice. Re-Detektion eines bereits gebannten
                // Spammers (AlreadyBanned/400) unterdrückt beides — wie Python
                // (moderation.py:1812-1843), sonst Doppel-Events + wiederholte Notices.
                if matches!(outcome, BanOutcome::Banned) {
                    self.record_ban_event_db(broadcaster_id, chatter_login, chatter_id, content)
                        .await;
                    if !silent {
                        let notice = notice_text
                            .map(|t| t.replace("{login}", chatter_login))
                            .unwrap_or_else(|| NOTICE_SPAM_BAN.replace("{login}", chatter_login));
                        self.send_autoban_notice(channel_login, broadcaster_id, &notice)
                            .await;
                    }
                }
                true
            }
            BanOutcome::Forbidden => {
                warn!("AutoBan: Bot ist wahrscheinlich kein Moderator in #{channel_login}");
                false
            }
            BanOutcome::Failed { status, body } => {
                warn!("AutoBan fehlgeschlagen: HTTP {status} — {body}");
                false
            }
            BanOutcome::Unbanned => false,
        }
    }

    /// Löscht die auslösende Nachricht best-effort und setzt danach einen Timeout.
    pub async fn timeout_and_cleanup(
        &self,
        broadcaster_id: &str,
        chatter_id: &str,
        message_id: &str,
        duration_secs: u32,
        reason_text: &str,
    ) -> bool {
        self.timeout_and_cleanup_with_evidence(
            broadcaster_id,
            broadcaster_id,
            "",
            chatter_id,
            message_id,
            "",
            duration_secs,
            reason_text,
            ModerationEvidence {
                source_path: "scam",
                reason: reason_text,
                score: None,
                account_age_days: None,
            },
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn timeout_and_cleanup_with_evidence(
        &self,
        channel_login: &str,
        broadcaster_id: &str,
        chatter_login: &str,
        chatter_id: &str,
        message_id: &str,
        content: &str,
        duration_secs: u32,
        reason_text: &str,
        evidence: ModerationEvidence<'_>,
    ) -> bool {
        // Safe-List: Timeout ist ebenfalls Moderation. Guard vor dem Delete.
        if crate::safe_list::is_safe(Some(chatter_id), chatter_login) {
            warn!(
                chatter_id = %chatter_id,
                reason = %reason_text,
                "Timeout gegen Safe-List-Konto unterdrückt"
            );
            self.persist_autoban_record(
                channel_login,
                chatter_id,
                chatter_login,
                content,
                "suppressed_safe_list",
                &evidence,
                false,
            )
            .await;
            self.update_message_moderation(message_id, "suppressed_safe_list", evidence.reason)
                .await;
            return false;
        }

        if let Err(error) = self.api.delete_message(broadcaster_id, message_id).await {
            warn!("Timeout-Cleanup Delete-Fehler: {error}");
        }

        let enforced = match self
            .api
            .timeout_user(broadcaster_id, chatter_id, duration_secs, reason_text)
            .await
        {
            Ok(BanOutcome::Banned | BanOutcome::AlreadyBanned) => true,
            Ok(_) => false,
            Err(error) => {
                warn!("Timeout Fehler: {error}");
                false
            }
        };

        if enforced {
            self.persist_autoban_record(
                channel_login,
                chatter_id,
                chatter_login,
                content,
                "timeout",
                &evidence,
                false,
            )
            .await;
            self.update_message_moderation(message_id, "timeout", evidence.reason)
                .await;
        }
        enforced
    }

    /// Gibt den letzten AutoBan-Eintrag für einen Kanal zurück.
    ///
    /// Port: `self._last_autoban.get(channel_key)` plus Restart-Fallback aus
    /// `tb_chat_autoban_log` (commands.py Z. 224).
    pub async fn last_autoban(&self, channel_login: &str) -> Option<AutoBanRecord> {
        let key = channel_login.to_lowercase();
        if let Some(record) = self.last_autoban.lock().unwrap().get(&key).cloned() {
            return Some(record);
        }

        self.last_autoban_db_fallback(&key).await
    }

    async fn last_autoban_db_fallback(&self, key: &str) -> Option<AutoBanRecord> {
        sqlx::query!(
            r#"SELECT chatter_id, chatter_login, content, banned_at
               FROM tb_chat_autoban_log
               WHERE channel_login = $1
               ORDER BY banned_at DESC
               LIMIT 1"#,
            key,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| debug!("last_autoban DB-Fallback fehlgeschlagen: {e}"))
        .ok()
        .flatten()
        .and_then(|row| {
            self.cache_autoban_record(
                key,
                Some(row.chatter_id),
                Some(row.chatter_login),
                row.content,
                row.banned_at,
            )
        })
    }

    fn cache_autoban_record(
        &self,
        key: &str,
        user_id: Option<String>,
        login: Option<String>,
        content: Option<String>,
        ts: DateTime<Utc>,
    ) -> Option<AutoBanRecord> {
        let user_id = user_id.unwrap_or_default();
        if user_id.trim().is_empty() {
            return None;
        }

        let record = AutoBanRecord {
            user_id,
            login: login.unwrap_or_default(),
            content: content.unwrap_or_default(),
            ts,
        };
        self.last_autoban
            .lock()
            .unwrap()
            .insert(key.to_string(), record.clone());
        Some(record)
    }

    /// Speichert den AutoBan In-Memory und in der DB.
    ///
    /// DB-Tabelle: `tb_chat_autoban_log` (Migration 20260630141000).
    /// Port: `self._last_autoban[channel_key] = {...}` (moderation.py Z. 235).
    #[allow(clippy::too_many_arguments)]
    async fn persist_autoban_record(
        &self,
        channel_login: &str,
        chatter_id: &str,
        chatter_login: &str,
        content: &str,
        action: &str,
        evidence: &ModerationEvidence<'_>,
        cache: bool,
    ) {
        let key = channel_login.to_lowercase();
        let record = AutoBanRecord {
            user_id: chatter_id.to_string(),
            login: chatter_login.to_string(),
            content: content.chars().take(500).collect(),
            ts: Utc::now(),
        };

        // In-Memory: Safe-List-Unterdrueckungen sind kein ruecknehmbarer Ban.
        if cache {
            let mut guard = self.last_autoban.lock().unwrap();
            guard.insert(key.clone(), record.clone());
        }

        // DB — Schema aus Migration 20260630141000.
        let ts_str = record.ts.to_rfc3339();
        let content_trunc: String = record.content.clone();
        if let Err(e) = sqlx::query(
            r#"INSERT INTO tb_chat_autoban_log
               (channel_login, chatter_id, chatter_login, content, banned_at,
                action, source_path, reason, score, account_age_days)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
               ON CONFLICT DO NOTHING"#,
        )
        .bind(&key)
        .bind(chatter_id)
        .bind(chatter_login)
        .bind(&content_trunc)
        .bind(record.ts)
        .bind(action)
        .bind(evidence.source_path)
        .bind(evidence.reason)
        .bind(evidence.score)
        .bind(evidence.account_age_days)
        .execute(&self.pool)
        .await
        {
            // DB-Fehler sind nicht fatal — In-Memory ist primäre Quelle
            debug!("persist_autoban_record DB-Fehler: {e} (ts={ts_str})");
        }
    }

    async fn update_message_moderation(&self, message_id: &str, action: &str, reason: &str) {
        if message_id.trim().is_empty() {
            warn!(
                action,
                reason, "Moderationsnachtrag ohne message_id ausgelassen"
            );
            return;
        }

        match sqlx::query(
            "UPDATE twitch_chat_messages \
             SET moderation_action = $1, moderation_reason = $2 \
             WHERE message_id = $3",
        )
        .bind(action)
        .bind(reason)
        .bind(message_id)
        .execute(&self.pool)
        .await
        {
            Ok(result) if result.rows_affected() == 0 => warn!(
                message_id,
                action, "Moderationsnachtrag fand keine archivierte Nachricht"
            ),
            Ok(_) => {}
            Err(error) => warn!(%error, message_id, action, "Moderationsnachtrag fehlgeschlagen"),
        }
    }

    /// Best-effort-Protokoll eines Auto-Bans in `twitch_ban_events`, damit die
    /// öffentliche `recent-bans`-Statistik die echte Spam-/Viewer-Bot-Moderation
    /// widerspiegelt (sonst zeigt der Live-Ban-Feed praktisch nichts).
    ///
    /// Port von `moderation.py:_record_autoban_db_event`. Der Spam-Inhalt dient als
    /// Feed-Grund (`reason`, gekürzt auf 300 Zeichen). DB-Fehler dürfen den Ban-Flow
    /// niemals unterbrechen. `event_type = 'ban'` wird vom recent-bans-Filter erwartet.
    async fn record_ban_event_db(
        &self,
        broadcaster_id: &str,
        chatter_login: &str,
        chatter_id: &str,
        reason: &str,
    ) {
        let reason_trunc: String = reason.trim().chars().take(300).collect();
        let reason_opt = (!reason_trunc.is_empty()).then_some(reason_trunc);
        let login_norm = chatter_login.trim().to_lowercase();
        let login_opt = (!login_norm.is_empty()).then_some(login_norm);

        if let Err(e) = sqlx::query!(
            r#"INSERT INTO twitch_ban_events
                   (twitch_user_id, event_type, target_login, target_id, reason, received_at)
               VALUES ($1, 'ban', $2, $3, $4, $5)"#,
            broadcaster_id,
            login_opt,
            chatter_id,
            reason_opt,
            Utc::now(),
        )
        .execute(&self.pool)
        .await
        {
            debug!("record_ban_event_db DB-Fehler: {e}");
        }
    }
}

#[async_trait]
impl LastAutobanStore for ModerationEngine {
    async fn last_autoban(&self, channel_key: &str) -> Option<AutobanEntry> {
        self.last_autoban(channel_key)
            .await
            .map(|record| AutobanEntry {
                user_id: record.user_id,
                login: record.login,
            })
    }
}

// ---------------------------------------------------------------------------
// TimeoutGuard
// ---------------------------------------------------------------------------

/// Wacht über ausgehende Chat-Timeouts des Bots.
///
/// Wenn der Bot in einem Kanal getimed outed wird (drop_code `sender_banned`
/// oder `sender_timedout`), zählt das als Timeout-Ereignis. Bei Überschreitung
/// der Schwellen → 7-Tage-Stummschaltung + Werbefrei-Pitch-Flag.
///
/// Port: `bot/chat/timeout_guard.py`.
pub struct TimeoutGuard {
    /// login → Vec<Instant> der Timeout-Ereignisse (Monotonic).
    timeouts: Mutex<HashMap<String, Vec<Instant>>>,
    /// login → Instant-Deadline der Stummschaltung (Monotonic).
    muted_until: Mutex<HashMap<String, Instant>>,
    /// Logins mit austehendem Pitch.
    pending_pitch: Mutex<std::collections::HashSet<String>>,
    /// login → letzter Pitch-Zeitpunkt (Monotonic).
    last_pitch: Mutex<HashMap<String, Instant>>,
}

impl TimeoutGuard {
    /// Erstellt einen neuen TimeoutGuard.
    pub fn new() -> Self {
        Self {
            timeouts: Mutex::new(HashMap::new()),
            muted_until: Mutex::new(HashMap::new()),
            pending_pitch: Mutex::new(std::collections::HashSet::new()),
            last_pitch: Mutex::new(HashMap::new()),
        }
    }

    /// Registriert ein Timeout-Ereignis für `login`.
    ///
    /// Prüft danach Tages- und Wochen-Schwellen. Bei Überschreitung:
    /// 7-Tage-Stummschaltung + Werbefrei-Pitch einplanen.
    ///
    /// Port: `timeout_guard.py:record_timeout` (Z. 38–54).
    pub fn record_timeout(&self, login: &str) {
        let now = Instant::now();
        let week_secs = TIMEOUT_MUTE_DURATION_SECS;
        let day_secs = 24 * 3600_u64;

        let (day_count, week_count) = {
            let mut guard = self.timeouts.lock().unwrap();
            let entries = guard.entry(login.to_lowercase()).or_default();
            // Einträge älter als _WEEK_SEC prunen (Monotonic: seit `now`)
            let week_dur = std::time::Duration::from_secs(week_secs);
            entries.retain(|t| now.duration_since(*t) < week_dur);
            entries.push(now);

            let day_dur = std::time::Duration::from_secs(day_secs);
            let day_count = entries
                .iter()
                .filter(|t| now.duration_since(**t) < day_dur)
                .count();
            let week_count = entries.len();
            (day_count, week_count)
        };

        let should_mute = day_count >= TIMEOUT_MUTE_DAILY_THRESHOLD
            || week_count >= TIMEOUT_MUTE_WEEKLY_THRESHOLD;

        if should_mute && !self.is_muted(login) {
            let mut guard = self.muted_until.lock().unwrap();
            guard.insert(
                login.to_lowercase(),
                now + std::time::Duration::from_secs(week_secs),
            );
        }

        // Werbefrei-Pitch einplanen wenn Cooldown abgelaufen
        let pitch_cooldown = std::time::Duration::from_secs(TIMEOUT_PITCH_COOLDOWN_SECS);
        let needs_pitch = {
            let guard = self.last_pitch.lock().unwrap();
            guard
                .get(&login.to_lowercase())
                .map(|t| now.duration_since(*t) >= pitch_cooldown)
                .unwrap_or(true)
        };
        if needs_pitch {
            self.pending_pitch
                .lock()
                .unwrap()
                .insert(login.to_lowercase());
        }
    }

    /// Gibt `true` zurück wenn `login` aktuell stummgeschaltet ist.
    ///
    /// Port: `timeout_guard.py:is_muted` (Z. 56–60).
    pub fn is_muted(&self, login: &str) -> bool {
        let now = Instant::now();
        let guard = self.muted_until.lock().unwrap();
        guard
            .get(&login.to_lowercase())
            .map(|t| *t > now)
            .unwrap_or(false)
    }

    /// Konsumiert den Pitch-Eintrag für `login` (beim Stream-Start aufrufen).
    ///
    /// Gibt `true` zurück wenn ein Pitch fällig war.
    /// Port: `timeout_guard.py:consume_stream_start_pitch` (Z. 62–68).
    pub fn consume_stream_start_pitch(&self, login: &str) -> bool {
        let key = login.to_lowercase();
        let removed = self.pending_pitch.lock().unwrap().remove(&key);
        if removed {
            let now = Instant::now();
            self.last_pitch.lock().unwrap().insert(key, now);
        }
        removed
    }
}

impl Default for TimeoutGuard {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// OutboundSuppressionStore + OutboundSuppressionCheck-Trait
// ---------------------------------------------------------------------------

/// Test fixture DDL for `twitch_outbound_chat_suppressions`.
///
/// Canonical schema lives in migration 20260630141000.
pub const SUPPRESSION_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS twitch_outbound_chat_suppressions (
    target_login TEXT NOT NULL,
    source TEXT NOT NULL,
    target_id TEXT,
    reason_code TEXT NOT NULL,
    reason_detail TEXT,
    suppressed_until TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (target_login, source)
);
CREATE INDEX IF NOT EXISTS idx_twitch_outbound_chat_suppressions_until
    ON twitch_outbound_chat_suppressions (suppressed_until);
"#;

/// Eintrag aus `twitch_outbound_chat_suppressions`.
#[derive(Debug, Clone)]
pub struct SuppressionEntry {
    pub target_login: String,
    pub target_id: Option<String>,
    pub source: String,
    pub reason_code: String,
    pub reason_detail: Option<String>,
    pub suppressed_until: DateTime<Utc>,
}

/// Trait: Prüft ob eine ausgehende Nachricht für einen Kanal/Source unterdrückt ist.
///
/// Der Orchestrator verdrahtet [`OutboundSuppressionStore`] als Impl.
/// Andere Module (Promo-Engine, Recruitment) können gegen diesen Trait testen.
#[async_trait]
pub trait OutboundSuppressionCheck: Send + Sync {
    /// Gibt `Some(entry)` zurück wenn `(target_login, source)` aktuell unterdrückt ist.
    async fn check_suppression(&self, target_login: &str, source: &str)
        -> Option<SuppressionEntry>;
}

/// Liest und schreibt `twitch_outbound_chat_suppressions`.
///
/// Port: `moderation.py:_get_outbound_chat_suppression` + `_blacklist_streamer_for_source`
/// (Z. 1030–1137).
pub struct OutboundSuppressionStore {
    pool: PgPool,
    /// Dedup-Cache: (target_login, source, reason_code) → Instant (Monotonic).
    /// TTL: 3600s (moderation.py Z. 404).
    log_cooldown: Mutex<HashMap<(String, String, String), Instant>>,
}

impl OutboundSuppressionStore {
    /// Erstellt einen neuen Store.
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            log_cooldown: Mutex::new(HashMap::new()),
        }
    }

    /// Liefert die Suppression-Dauer für `source`+`reason_code`.
    ///
    /// Gibt `None` zurück wenn `reason_code != "channel_settings"` oder
    /// `source` nicht in der erlaubten Menge.
    ///
    /// Port: `moderation.py:_get_outbound_chat_suppression_ttl` (Z. 372–374).
    pub fn suppression_ttl(source: &str, reason_code: &str) -> Option<Duration> {
        if reason_code.trim().to_lowercase() != SUPPRESSION_TRIGGER_CODE {
            return None;
        }
        match source {
            "promo" | "recruitment" => Some(Duration::seconds(SUPPRESSION_PROMO_SECS)),
            "partner_raid" => Some(Duration::seconds(SUPPRESSION_PARTNER_RAID_SECS)),
            _ => None,
        }
    }

    /// Schreibt einen Suppression-Eintrag (UPSERT).
    ///
    /// Port: `moderation.py:_blacklist_streamer_for_source` (Z. 1133–1137) +
    /// `twitch_outbound_chat_suppressions` UPSERT-Logik.
    pub async fn upsert_suppression(
        &self,
        target_login: &str,
        target_id: Option<&str>,
        source: &str,
        reason_code: &str,
        reason_detail: Option<&str>,
        ttl: Duration,
    ) -> Result<(), sqlx::Error> {
        let now = Utc::now();
        let suppressed_until = now + ttl;
        sqlx::query!(
            r#"INSERT INTO twitch_outbound_chat_suppressions
               (target_login, source, target_id, reason_code, reason_detail,
                suppressed_until, created_at, updated_at)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
               ON CONFLICT (target_login, source) DO UPDATE SET
                   target_id = COALESCE(EXCLUDED.target_id, twitch_outbound_chat_suppressions.target_id),
                   reason_code = EXCLUDED.reason_code,
                   reason_detail = EXCLUDED.reason_detail,
                   suppressed_until = EXCLUDED.suppressed_until,
                   updated_at = EXCLUDED.updated_at"#,
            target_login,
            source,
            target_id,
            reason_code,
            reason_detail,
            suppressed_until,
            now,
            now,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Prüft ob ein Suppression-Log-Cooldown aktiv ist und setzt ihn.
    ///
    /// Gibt `true` zurück wenn das Event innerhalb der letzten 3600s bereits
    /// geloggt wurde (Cooldown aktiv = nicht nochmal loggen).
    ///
    /// Port: `moderation.py:_suppression_log_cooldown` (Z. 403–404).
    pub fn check_and_set_log_cooldown(
        &self,
        target_login: &str,
        source: &str,
        reason_code: &str,
    ) -> bool {
        let key = (
            target_login.to_lowercase(),
            source.to_string(),
            reason_code.to_string(),
        );
        let now = Instant::now();
        let cooldown = std::time::Duration::from_secs(3600);
        let mut guard = self.log_cooldown.lock().unwrap();
        if let Some(last) = guard.get(&key) {
            if now.duration_since(*last) < cooldown {
                return true; // Cooldown aktiv
            }
        }
        guard.insert(key, now);
        false
    }
}

#[async_trait]
impl OutboundSuppressionCheck for OutboundSuppressionStore {
    /// Liest aktive Suppression aus DB.
    ///
    /// Port: `moderation.py:_get_outbound_chat_suppression` (Z. 1030–1079).
    /// Gibt `None` wenn `source` nicht in erlaubter Menge (kein DB-Call).
    async fn check_suppression(
        &self,
        target_login: &str,
        source: &str,
    ) -> Option<SuppressionEntry> {
        // Schnell-Prüfung: nur erlaubte source-Tags (moderation.py Z. 375–376)
        if !matches!(source, "promo" | "recruitment" | "partner_raid") {
            return None;
        }
        let now = Utc::now();
        // Prod: suppressed_until ist TIMESTAMPTZ → DateTime<Utc> binden
        let row = sqlx::query!(
            r#"SELECT target_login, target_id, source, reason_code, reason_detail, suppressed_until
               FROM twitch_outbound_chat_suppressions
               WHERE target_login = $1 AND source = $2 AND suppressed_until > $3
               LIMIT 1"#,
            target_login,
            source,
            now,
        )
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();

        row.map(|row| SuppressionEntry {
            target_login: row.target_login,
            target_id: row.target_id,
            source: row.source,
            reason_code: row.reason_code,
            reason_detail: row.reason_detail,
            suppressed_until: row.suppressed_until,
        })
    }
}

#[async_trait]
impl crate::promos::OutboundSuppressionCheck for OutboundSuppressionStore {
    /// Promo-Mute-Brücke: der Promo-Pfad prüft die `promo`-Suppression
    /// (Python: `_get_outbound_chat_suppression(login, "promo")` vor jedem
    /// Promo-Send). Aktiver Eintrag = Kanal ist stumm.
    async fn is_muted(&self, channel_login: &str) -> bool {
        self.check_suppression(channel_login, "promo")
            .await
            .is_some()
    }
}

#[async_trait]
impl crate::promos::OutboundSuppressionWriter for OutboundSuppressionStore {
    /// Schreibseite: bei einem `channel_settings`-Drop wird der Kanal für die
    /// quell-spezifische Dauer (7d promo/recruitment, 3d partner_raid)
    /// stummgeschaltet. Nicht-passende `source`/`reason_code` → No-op (kein
    /// DB-Write), exakt wie [`OutboundSuppressionStore::suppression_ttl`].
    ///
    /// Port: `moderation.py:_maybe_blacklist_for_drop_reason` (Z. 1310–1329) +
    /// `_set_outbound_chat_suppression`. Schreibfehler sind best-effort (geloggt,
    /// kein Abbruch des Sendepfads).
    async fn suppress_for_drop(
        &self,
        channel_login: &str,
        channel_id: Option<&str>,
        source: &str,
        reason_code: &str,
        reason_detail: Option<&str>,
    ) {
        let Some(ttl) = Self::suppression_ttl(source, reason_code) else {
            return;
        };
        if let Err(e) = self
            .upsert_suppression(
                channel_login,
                channel_id,
                source,
                reason_code,
                reason_detail,
                ttl,
            )
            .await
        {
            tracing::warn!(
                channel = %channel_login,
                source = %source,
                "Outbound-Suppression-Write fehlgeschlagen: {e}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SendOutcome;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    // -----------------------------------------------------------------------
    // Mock-ChatApi
    // -----------------------------------------------------------------------

    struct MockApi {
        ban_calls: AtomicUsize,
        delete_calls: AtomicUsize,
        send_calls: AtomicUsize,
        timeout_calls: AtomicUsize,
        ban_result: Mutex<BanOutcome>,
        send_result: Mutex<Result<SendOutcome, String>>,
    }

    impl MockApi {
        fn with_ban_result(result: BanOutcome) -> Arc<Self> {
            Arc::new(Self {
                ban_calls: AtomicUsize::new(0),
                delete_calls: AtomicUsize::new(0),
                send_calls: AtomicUsize::new(0),
                timeout_calls: AtomicUsize::new(0),
                ban_result: Mutex::new(result),
                send_result: Mutex::new(Ok(SendOutcome::Sent)),
            })
        }

        fn with_send_result(result: Result<SendOutcome, String>) -> Arc<Self> {
            let api = Self::with_ban_result(BanOutcome::Banned);
            *api.send_result.lock().unwrap() = result;
            api
        }
    }

    struct FixedSuppression(bool);

    #[async_trait]
    impl crate::promos::OutboundSuppressionCheck for FixedSuppression {
        async fn is_muted(&self, _channel_login: &str) -> bool {
            self.0
        }
    }

    struct FixedManualOptOut(bool);

    #[async_trait]
    impl ManualPartnerOptOutCheck for FixedManualOptOut {
        async fn is_manual_partner_opt_out(&self, _target_login: &str) -> bool {
            self.0
        }
    }

    #[async_trait]
    impl ChatApi for MockApi {
        async fn send_message(&self, _b: &str, _m: &str) -> Result<SendOutcome, String> {
            self.send_calls.fetch_add(1, Ordering::SeqCst);
            self.send_result.lock().unwrap().clone()
        }
        async fn send_announcement(&self, _b: &str, _m: &str, _c: &str) -> Result<bool, String> {
            Ok(true)
        }
        async fn ban_user(&self, _b: &str, _u: &str, _r: &str) -> Result<BanOutcome, String> {
            self.ban_calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.ban_result.lock().unwrap().clone())
        }
        async fn timeout_user(
            &self,
            _b: &str,
            _u: &str,
            _d: u32,
            _r: &str,
        ) -> Result<BanOutcome, String> {
            self.timeout_calls.fetch_add(1, Ordering::SeqCst);
            Ok(BanOutcome::Banned)
        }
        async fn unban_user(&self, _b: &str, _u: &str) -> Result<bool, String> {
            Ok(true)
        }
        async fn delete_message(&self, _b: &str, _m: &str) -> Result<bool, String> {
            self.delete_calls.fetch_add(1, Ordering::SeqCst);
            Ok(true)
        }
        async fn user_created_at(&self, _u: &str) -> Result<Option<chrono::DateTime<Utc>>, String> {
            Ok(None)
        }
        async fn resolve_user_id(&self, _l: &str) -> Result<Option<String>, String> {
            Ok(None)
        }
        async fn bot_user_id(&self) -> String {
            "mock-bot-id".to_string()
        }
    }

    // -----------------------------------------------------------------------
    // ModerationEngine-Tests
    // -----------------------------------------------------------------------

    async fn pg_pool_in_schema_or_skip(schema: &str) -> Option<PgPool> {
        let dsn = match std::env::var("TB_TEST_DATABASE_URL") {
            Ok(dsn) => dsn,
            Err(_) => {
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return None;
            }
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
        Some(
            PgPoolOptions::new()
                .max_connections(4)
                .connect_with(opts)
                .await
                .unwrap(),
        )
    }

    #[tokio::test]
    async fn autoban_ruft_delete_und_ban_auf() {
        let api = MockApi::with_ban_result(BanOutcome::Banned);
        // connect_lazy baut keinen echten Pool auf — Fehler in persist_autoban_record
        // werden nur debug-geloggt. In-Memory-Logik und API-Aufrufe testen wir hier.
        let pool = sqlx::PgPool::connect_lazy("postgres://x:x@127.0.0.1:1/x").unwrap();
        let engine = ModerationEngine::new(api.clone(), pool);
        let result = engine
            .auto_ban_and_cleanup(AutoBanRequest {
                channel_login: "kanal",
                broadcaster_id: "broadcast-id",
                bot_id: "bot-id",
                chatter_login: "spammer",
                chatter_id: "user-123",
                message_id: "msg-id",
                content: "Spam-Text",
                ban: true,
                reason_text: BAN_REASON_SPAM,
                notice_text: None,
                silent: false,
            })
            .await;
        assert!(result, "AutoBan soll true zurückgeben");
        assert_eq!(
            api.delete_calls.load(Ordering::SeqCst),
            1,
            "Delete aufgerufen"
        );
        assert_eq!(api.ban_calls.load(Ordering::SeqCst), 1, "Ban aufgerufen");
    }

    /// Safe-List: weder Ban noch Message-Delete, egal welcher Pfad ruft.
    #[tokio::test]
    async fn autoban_verschont_safe_konten() {
        for safe in crate::safe_list::SAFE_ACCOUNTS {
            let api = MockApi::with_ban_result(BanOutcome::Banned);
            let pool = sqlx::PgPool::connect_lazy("postgres://x:x@127.0.0.1:1/x").unwrap();
            let engine = ModerationEngine::new(api.clone(), pool);
            let result = engine
                .auto_ban_and_cleanup(AutoBanRequest {
                    channel_login: "kanal",
                    broadcaster_id: "broadcast-id",
                    bot_id: "bot-id",
                    chatter_login: safe.login,
                    chatter_id: safe.twitch_user_id,
                    message_id: "msg-id",
                    content: "jaja frag mal ricky",
                    ban: true,
                    reason_text: BAN_REASON_SPAM,
                    notice_text: None,
                    silent: false,
                })
                .await;

            assert!(!result, "Safe-Konto {} darf kein true liefern", safe.login);
            assert_eq!(
                api.ban_calls.load(Ordering::SeqCst),
                0,
                "Safe-Konto {} wurde gebannt",
                safe.login
            );
            assert_eq!(
                api.delete_calls.load(Ordering::SeqCst),
                0,
                "Nachricht von Safe-Konto {} wurde geloescht",
                safe.login
            );
        }
    }

    /// Timeout ist auch Moderation: Safe-Konten bekommen weder Timeout noch
    /// Message-Delete. (Merge-Kritiker 2026-07-10: Pfad war ungeschützt.)
    #[tokio::test]
    async fn timeout_verschont_safe_konten() {
        for safe in crate::safe_list::SAFE_ACCOUNTS {
            let api = MockApi::with_ban_result(BanOutcome::Banned);
            let pool = sqlx::PgPool::connect_lazy("postgres://x:x@127.0.0.1:1/x").unwrap();
            let engine = ModerationEngine::new(api.clone(), pool);
            let result = engine
                .timeout_and_cleanup(
                    "broadcast-id",
                    safe.twitch_user_id,
                    "msg-id",
                    600,
                    "Scam-Verdacht",
                )
                .await;

            assert!(!result, "Safe-Konto {} darf kein true liefern", safe.login);
            assert_eq!(
                api.timeout_calls.load(Ordering::SeqCst),
                0,
                "Safe-Konto {} wurde getimeoutet",
                safe.login
            );
            assert_eq!(
                api.delete_calls.load(Ordering::SeqCst),
                0,
                "Nachricht von Safe-Konto {} wurde geloescht",
                safe.login
            );
        }
    }

    #[tokio::test]
    async fn timeout_greift_bei_fremdem_konto() {
        let api = MockApi::with_ban_result(BanOutcome::Banned);
        let pool = sqlx::PgPool::connect_lazy("postgres://x:x@127.0.0.1:1/x").unwrap();
        let engine = ModerationEngine::new(api.clone(), pool);
        let result = engine
            .timeout_and_cleanup("broadcast-id", "999999999", "msg-id", 600, "Scam")
            .await;

        assert!(result);
        assert_eq!(api.timeout_calls.load(Ordering::SeqCst), 1);
    }

    /// Der Login allein schützt nicht: fremde ID mit übernommenem Namen bannt.
    #[tokio::test]
    async fn autoban_bannt_bei_uebernommenem_safe_login() {
        let api = MockApi::with_ban_result(BanOutcome::Banned);
        let pool = sqlx::PgPool::connect_lazy("postgres://x:x@127.0.0.1:1/x").unwrap();
        let engine = ModerationEngine::new(api.clone(), pool);
        let result = engine
            .auto_ban_and_cleanup(AutoBanRequest {
                channel_login: "kanal",
                broadcaster_id: "broadcast-id",
                bot_id: "bot-id",
                chatter_login: "kubi_kubi_kubi",
                chatter_id: "999999999",
                message_id: "msg-id",
                content: "Spam-Text",
                ban: true,
                reason_text: BAN_REASON_SPAM,
                notice_text: None,
                silent: false,
            })
            .await;

        assert!(result, "fremde ID darf nicht vom Login profitieren");
        assert_eq!(api.ban_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn last_autoban_laed_restart_fallback_aus_db() {
        let schema = format!("autoban_restart_{}", std::process::id());
        let Some(pool) = pg_pool_in_schema_or_skip(&schema).await else {
            return;
        };
        sqlx::query(
            r#"CREATE TABLE tb_chat_autoban_log (
                id BIGSERIAL PRIMARY KEY,
                channel_login TEXT NOT NULL,
                chatter_id TEXT NOT NULL,
                chatter_login TEXT NOT NULL,
                content TEXT,
                banned_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )"#,
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            r#"INSERT INTO tb_chat_autoban_log
               (channel_login, chatter_id, chatter_login, content, banned_at)
               VALUES
               ('restartkanal', 'u-old', 'old_spammer', 'alter spam', NOW() - INTERVAL '5 minutes'),
               ('restartkanal', 'u-new', 'new_spammer', 'neuer spam', NOW())"#,
        )
        .execute(&pool)
        .await
        .unwrap();

        let engine =
            ModerationEngine::new(MockApi::with_ban_result(BanOutcome::Banned), pool.clone());
        let record = engine.last_autoban("RestartKanal").await.unwrap();
        assert_eq!(record.user_id, "u-new");
        assert_eq!(record.login, "new_spammer");
        assert_eq!(record.content, "neuer spam");

        sqlx::query("DROP TABLE tb_chat_autoban_log")
            .execute(&pool)
            .await
            .unwrap();
        let cached = engine.last_autoban("restartkanal").await.unwrap();
        assert_eq!(cached.user_id, "u-new");
    }

    #[tokio::test]
    async fn delete_only_ruft_keinen_ban_auf() {
        let api = MockApi::with_ban_result(BanOutcome::Banned);
        let pool = sqlx::PgPool::connect_lazy("postgres://x:x@127.0.0.1:1/x").unwrap();
        let engine = ModerationEngine::new(api.clone(), pool);
        engine
            .auto_ban_and_cleanup(AutoBanRequest {
                channel_login: "kanal",
                broadcaster_id: "bid",
                bot_id: "bot",
                chatter_login: "user",
                chatter_id: "u1",
                message_id: "m1",
                content: "content",
                ban: false,
                reason_text: BAN_REASON_SPAM,
                notice_text: None,
                silent: false,
            })
            .await;
        assert_eq!(
            api.ban_calls.load(Ordering::SeqCst),
            0,
            "kein Ban bei ban=false"
        );
        assert_eq!(api.delete_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn forbidden_ban_gibt_false_zurueck() {
        let api = MockApi::with_ban_result(BanOutcome::Forbidden);
        let pool = sqlx::PgPool::connect_lazy("postgres://x:x@127.0.0.1:1/x").unwrap();
        let engine = ModerationEngine::new(api.clone(), pool);
        let result = engine
            .auto_ban_and_cleanup(AutoBanRequest {
                channel_login: "kanal",
                broadcaster_id: "bid",
                bot_id: "bot",
                chatter_login: "user",
                chatter_id: "u1",
                message_id: "m1",
                content: "content",
                ban: true,
                reason_text: BAN_REASON_SPAM,
                notice_text: None,
                silent: true,
            })
            .await;
        assert!(!result, "Forbidden → false");
    }

    #[tokio::test]
    async fn silent_ban_sendet_kein_notice() {
        let api = MockApi::with_ban_result(BanOutcome::Banned);
        let pool = sqlx::PgPool::connect_lazy("postgres://x:x@127.0.0.1:1/x").unwrap();
        let engine = ModerationEngine::new(api.clone(), pool);
        engine
            .auto_ban_and_cleanup(AutoBanRequest {
                channel_login: "kanal",
                broadcaster_id: "bid",
                bot_id: "bot",
                chatter_login: "user",
                chatter_id: "u1",
                message_id: "m1",
                content: "content",
                ban: true,
                reason_text: BAN_REASON_SPAM,
                notice_text: None,
                silent: true,
            })
            .await;
        assert_eq!(
            api.send_calls.load(Ordering::SeqCst),
            0,
            "silent_ban: kein Chat-Notice"
        );
    }

    #[tokio::test]
    async fn not_silent_ban_sendet_notice() {
        let api = MockApi::with_ban_result(BanOutcome::Banned);
        let pool = sqlx::PgPool::connect_lazy("postgres://x:x@127.0.0.1:1/x").unwrap();
        let engine = ModerationEngine::new(api.clone(), pool);
        engine
            .auto_ban_and_cleanup(AutoBanRequest {
                channel_login: "kanal",
                broadcaster_id: "bid",
                bot_id: "bot",
                chatter_login: "user",
                chatter_id: "u1",
                message_id: "m1",
                content: "content",
                ban: true,
                reason_text: BAN_REASON_SPAM,
                notice_text: None,
                silent: false,
            })
            .await;
        assert_eq!(
            api.send_calls.load(Ordering::SeqCst),
            1,
            "kein silent → Chat-Notice gesendet"
        );
    }

    #[tokio::test]
    async fn autoban_notice_send_fehler_unterbricht_autoban_nicht() {
        for result in [
            Ok(SendOutcome::Dropped {
                code: "channel_settings".to_string(),
                message: "muted".to_string(),
            }),
            Ok(SendOutcome::HttpError {
                status: 500,
                body: "server error".to_string(),
            }),
            Err("network down".to_string()),
        ] {
            let api = MockApi::with_send_result(result);
            let pool = sqlx::PgPool::connect_lazy("postgres://x:x@127.0.0.1:1/x").unwrap();
            let engine = ModerationEngine::new(api.clone(), pool);
            let ok = engine
                .auto_ban_and_cleanup(AutoBanRequest {
                    channel_login: "kanal",
                    broadcaster_id: "bid",
                    bot_id: "bot",
                    chatter_login: "user",
                    chatter_id: "u1",
                    message_id: "m1",
                    content: "content",
                    ban: true,
                    reason_text: BAN_REASON_SPAM,
                    notice_text: None,
                    silent: false,
                })
                .await;

            assert!(ok, "Notice-Sendfehler darf AutoBan nicht unterbrechen");
            assert_eq!(api.ban_calls.load(Ordering::SeqCst), 1);
            assert_eq!(api.send_calls.load(Ordering::SeqCst), 1);
        }
    }

    #[tokio::test]
    async fn autoban_notice_suppression_guard_skippt_notice() {
        let api = MockApi::with_ban_result(BanOutcome::Banned);
        let pool = sqlx::PgPool::connect_lazy("postgres://x:x@127.0.0.1:1/x").unwrap();
        let engine = ModerationEngine::new(api.clone(), pool)
            .with_notice_suppression(Arc::new(FixedSuppression(true)))
            .with_notice_manual_opt_out_check(Arc::new(FixedManualOptOut(false)));
        engine
            .auto_ban_and_cleanup(AutoBanRequest {
                channel_login: "kanal",
                broadcaster_id: "bid",
                bot_id: "bot",
                chatter_login: "user",
                chatter_id: "u1",
                message_id: "m1",
                content: "content",
                ban: true,
                reason_text: BAN_REASON_SPAM,
                notice_text: None,
                silent: false,
            })
            .await;
        assert_eq!(api.ban_calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            api.send_calls.load(Ordering::SeqCst),
            0,
            "Suppression-Guard verhindert nur die Notice"
        );
    }

    #[tokio::test]
    async fn autoban_notice_suppression_guard_allowed_sendet_notice() {
        let api = MockApi::with_ban_result(BanOutcome::Banned);
        let pool = sqlx::PgPool::connect_lazy("postgres://x:x@127.0.0.1:1/x").unwrap();
        let engine = ModerationEngine::new(api.clone(), pool)
            .with_notice_suppression(Arc::new(FixedSuppression(false)))
            .with_notice_manual_opt_out_check(Arc::new(FixedManualOptOut(false)));
        engine
            .auto_ban_and_cleanup(AutoBanRequest {
                channel_login: "kanal",
                broadcaster_id: "bid",
                bot_id: "bot",
                chatter_login: "user",
                chatter_id: "u1",
                message_id: "m1",
                content: "content",
                ban: true,
                reason_text: BAN_REASON_SPAM,
                notice_text: None,
                silent: false,
            })
            .await;
        assert_eq!(
            api.send_calls.load(Ordering::SeqCst),
            1,
            "Allowed-Guard delegiert die Notice"
        );
    }

    // -----------------------------------------------------------------------
    // TimeoutGuard-Tests
    // -----------------------------------------------------------------------

    #[test]
    fn timeout_guard_zwei_timeouts_pro_tag_triggert_mute() {
        let guard = TimeoutGuard::new();
        guard.record_timeout("kanal");
        assert!(!guard.is_muted("kanal"), "nach 1 Timeout: noch nicht muted");
        guard.record_timeout("kanal");
        assert!(guard.is_muted("kanal"), "nach 2 Timeouts/Tag: muted");
    }

    #[test]
    fn timeout_guard_funf_timeouts_pro_woche_triggert_mute() {
        let guard = TimeoutGuard::new();
        for _ in 0..5 {
            guard.record_timeout("kanal2");
        }
        assert!(guard.is_muted("kanal2"), "nach 5/Woche: muted");
    }

    #[test]
    fn timeout_guard_pitch_wird_geplant() {
        let guard = TimeoutGuard::new();
        guard.record_timeout("pitchkanal");
        let consumed = guard.consume_stream_start_pitch("pitchkanal");
        assert!(consumed, "Pitch soll nach record_timeout geplant sein");
    }

    #[test]
    fn timeout_guard_pitch_nur_einmal_konsumiert() {
        let guard = TimeoutGuard::new();
        guard.record_timeout("pitchkanal2");
        assert!(guard.consume_stream_start_pitch("pitchkanal2"));
        assert!(
            !guard.consume_stream_start_pitch("pitchkanal2"),
            "zweites Consume: false"
        );
    }

    // -----------------------------------------------------------------------
    // OutboundSuppressionStore-Tests (reine Logik, kein DB)
    // -----------------------------------------------------------------------

    #[test]
    fn suppression_ttl_gibt_korrekte_werte() {
        assert_eq!(
            OutboundSuppressionStore::suppression_ttl("promo", "channel_settings"),
            Some(Duration::seconds(SUPPRESSION_PROMO_SECS)),
        );
        assert_eq!(
            OutboundSuppressionStore::suppression_ttl("recruitment", "channel_settings"),
            Some(Duration::seconds(SUPPRESSION_RECRUITMENT_SECS)),
        );
        assert_eq!(
            OutboundSuppressionStore::suppression_ttl("partner_raid", "channel_settings"),
            Some(Duration::seconds(SUPPRESSION_PARTNER_RAID_SECS)),
        );
    }

    #[test]
    fn suppression_ttl_nur_bei_channel_settings() {
        // Kein anderer reason_code darf eine TTL liefern
        assert_eq!(
            OutboundSuppressionStore::suppression_ttl("promo", "some_other_code"),
            None,
        );
        assert_eq!(
            OutboundSuppressionStore::suppression_ttl("promo", "CHANNEL_SETTINGS"), // case-insensitive
            Some(Duration::seconds(SUPPRESSION_PROMO_SECS)),
            "case-insensitive Prüfung aus Vertrag Z. 373"
        );
    }

    #[test]
    fn suppression_ttl_unbekannte_source_gibt_none() {
        assert_eq!(
            OutboundSuppressionStore::suppression_ttl("unbekannt", "channel_settings"),
            None,
        );
    }

    #[tokio::test]
    async fn log_cooldown_verhindert_doppel_log() {
        let pool = sqlx::PgPool::connect_lazy("postgres://x:x@127.0.0.1:1/x").unwrap();
        let store = OutboundSuppressionStore::new(pool);
        assert!(!store.check_and_set_log_cooldown("k", "promo", "channel_settings"));
        assert!(
            store.check_and_set_log_cooldown("k", "promo", "channel_settings"),
            "zweiter Aufruf: Cooldown aktiv"
        );
        // Anderer Key: kein Cooldown
        assert!(!store.check_and_set_log_cooldown("k", "recruitment", "channel_settings"));
    }

    // -----------------------------------------------------------------------
    // Konstanten-Checks
    // -----------------------------------------------------------------------

    #[test]
    fn ban_reason_texte_wortgetreu() {
        assert_eq!(BAN_REASON_SPAM, "Automatischer Spam-Ban (Bot-Phrase)");
        assert_eq!(
            BAN_REASON_GLOBAL,
            "Netzwerkweiter Ban: Verstoß gegen Community-Richtlinien"
        );
    }

    #[test]
    fn notice_texte_enthalten_platzhalter() {
        assert!(NOTICE_SPAM_BAN.contains("{login}"));
        assert!(NOTICE_GLOBAL_BAN.contains("{login}"));
        assert!(NOTICE_SPAM_BAN.starts_with("🛡️ Auto-Mod:"));
        assert!(NOTICE_GLOBAL_BAN.starts_with("🛡️ "));
    }

    #[test]
    fn werbefrei_pitch_url_korrekt() {
        assert_eq!(
            WERBEFREI_PITCH_URL,
            "https://deutsche-deadlock-community.de/twitch/pricing"
        );
    }
}
