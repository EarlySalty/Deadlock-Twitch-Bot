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

use crate::api::{BanOutcome, ChatApi};
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
            let token = self
                .token_mgr
                .get_valid_token(force)
                .await?;
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

    /// Sendet Ankündigung — 2-Attempt.
    /// moderator_id = Bot-User-ID, intern via token_mgr bezogen.
    async fn send_announcement(
        &self,
        broadcaster_id: &str,
        message: &str,
        color: &str,
    ) -> Result<bool, String> {
        let moderator_id = self.token_mgr.bot_user_id().await;
        for attempt in 0..2usize {
            let force = attempt > 0;
            let token = match self.token_mgr.get_valid_token(force).await {
                Ok(t) => t,
                Err(e) => return Err(e),
            };
            match self
                .helix
                .send_announcement(broadcaster_id, &moderator_id, message, color, &token)
                .await
            {
                Ok(true) => return Ok(true),
                Ok(false) if attempt == 0 => {
                    // Könnte 401 sein — retry mit force_refresh
                    continue;
                }
                Ok(false) => return Ok(false),
                Err(e) => {
                    warn!("Announcement-Fehler: {e}");
                    return Err(e.to_string());
                }
            }
        }
        Ok(false)
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
    async fn unban_user(
        &self,
        broadcaster_id: &str,
        target_user_id: &str,
    ) -> Result<bool, String> {
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
    async fn delete_message(
        &self,
        broadcaster_id: &str,
        message_id: &str,
    ) -> Result<bool, String> {
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
    async fn user_created_at(
        &self,
        user_id: &str,
    ) -> Result<Option<DateTime<Utc>>, String> {
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
                .ban_user(broadcaster_id, moderator_id, user_id, reason, duration_secs, &token)
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

/// Moderations-Engine — koordiniert Delete + Ban + DB-Persistierung.
///
/// Port: `moderation.py:_auto_ban_and_cleanup` (Z. 1561–1829).
pub struct ModerationEngine {
    api: Arc<dyn ChatApi>,
    pool: PgPool,
    /// In-Memory-Store: channel_login (lowercase) → letzter AutoBan.
    last_autoban: Arc<Mutex<HashMap<String, AutoBanRecord>>>,
}

impl ModerationEngine {
    /// Erstellt eine neue ModerationEngine.
    pub fn new(api: Arc<dyn ChatApi>, pool: PgPool) -> Self {
        Self {
            api,
            pool,
            last_autoban: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Führt AutoBan aus: 1. Nachricht löschen, 2. Ban (wenn `req.ban=true`).
    ///
    /// Rückgabe: `true` wenn mindestens eine Aktion erfolgreich war.
    ///
    /// Port: `moderation.py:_auto_ban_and_cleanup` (Z. 1561–1829).
    pub async fn auto_ban_and_cleanup(&self, req: AutoBanRequest<'_>) -> bool {
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

        // Schritt 1: Nachricht löschen (moderation.py Z. 1631–1666)
        let _ = self.api.delete_message(broadcaster_id, message_id).await;

        if !ban {
            // Delete-Only-Pfad (moderation.py Z. 259–261)
            self.persist_autoban_record(channel_login, chatter_id, chatter_login, content)
                .await;
            return true;
        }

        // Schritt 2: Ban (moderation.py Z. 1679–1816)
        let outcome = match self.api.ban_user(broadcaster_id, chatter_id, reason_text).await {
            Ok(o) => o,
            Err(e) => {
                warn!("AutoBan Fehler: {e}");
                return false;
            }
        };

        match &outcome {
            BanOutcome::Banned | BanOutcome::AlreadyBanned => {
                // In-Memory + DB speichern
                self.persist_autoban_record(channel_login, chatter_id, chatter_login, content)
                    .await;

                // Chat-Notice (nur wenn nicht silent)
                // Port: moderation.py Z. 238–239
                if !silent {
                    let notice = notice_text
                        .map(|t| t.replace("{login}", chatter_login))
                        .unwrap_or_else(|| NOTICE_SPAM_BAN.replace("{login}", chatter_login));
                    let _ = self.api.send_message(broadcaster_id, &notice).await;
                }
                true
            }
            BanOutcome::Forbidden => {
                warn!(
                    "AutoBan: Bot ist wahrscheinlich kein Moderator in #{channel_login}"
                );
                false
            }
            BanOutcome::Failed { status, body } => {
                warn!("AutoBan fehlgeschlagen: HTTP {status} — {body}");
                false
            }
            BanOutcome::Unbanned => false,
        }
    }

    /// Gibt den letzten AutoBan-Eintrag für einen Kanal zurück.
    ///
    /// Port: `self._last_autoban.get(channel_key)` (commands.py Z. 224).
    pub fn last_autoban(&self, channel_login: &str) -> Option<AutoBanRecord> {
        let key = channel_login.to_lowercase();
        self.last_autoban.lock().unwrap().get(&key).cloned()
    }

    /// Speichert den AutoBan In-Memory und in der DB.
    ///
    /// DB-Tabelle: `tb_chat_autoban_log` (eigene kleine Tabelle, nicht in Prod-Schema
    /// vorhanden — wird durch `apply_ddl` in Tests angelegt).
    /// Port: `self._last_autoban[channel_key] = {...}` (moderation.py Z. 235).
    async fn persist_autoban_record(
        &self,
        channel_login: &str,
        chatter_id: &str,
        chatter_login: &str,
        content: &str,
    ) {
        let key = channel_login.to_lowercase();
        let record = AutoBanRecord {
            user_id: chatter_id.to_string(),
            login: chatter_login.to_string(),
            content: content.chars().take(500).collect(),
            ts: Utc::now(),
        };

        // In-Memory
        {
            let mut guard = self.last_autoban.lock().unwrap();
            guard.insert(key.clone(), record.clone());
        }

        // DB — eigene Tabelle (muss in Migrations-DDL angelegt werden)
        let ts_str = record.ts.to_rfc3339();
        let content_trunc: String = record.content.clone();
        if let Err(e) = sqlx::query(
            r#"INSERT INTO tb_chat_autoban_log
               (channel_login, chatter_id, chatter_login, content, banned_at)
               VALUES ($1, $2, $3, $4, $5)
               ON CONFLICT DO NOTHING"#,
        )
        .bind(&key)
        .bind(chatter_id)
        .bind(chatter_login)
        .bind(&content_trunc)
        .bind(record.ts)
        .execute(&self.pool)
        .await
        {
            // DB-Fehler sind nicht fatal — In-Memory ist primäre Quelle
            debug!("persist_autoban_record DB-Fehler: {e} (ts={ts_str})");
        }
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

/// DDL für `twitch_outbound_chat_suppressions` (auto-create, moderation.py Z. 387–401).
///
/// Wird vom Orchestrator beim Start einmalig ausgeführt.
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
    async fn check_suppression(
        &self,
        target_login: &str,
        source: &str,
    ) -> Option<SuppressionEntry>;
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
        sqlx::query(
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
        )
        .bind(target_login)
        .bind(source)
        .bind(target_id)
        .bind(reason_code)
        .bind(reason_detail)
        .bind(suppressed_until)
        .bind(now)
        .bind(now)
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
        let row = sqlx::query_as::<_, (String, Option<String>, String, String, Option<String>, DateTime<Utc>)>(
            r#"SELECT target_login, target_id, source, reason_code, reason_detail, suppressed_until
               FROM twitch_outbound_chat_suppressions
               WHERE target_login = $1 AND source = $2 AND suppressed_until > $3
               LIMIT 1"#,
        )
        .bind(target_login)
        .bind(source)
        .bind(now)
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();

        row.map(|(tl, tid, src, rc, rd, su)| SuppressionEntry {
            target_login: tl,
            target_id: tid,
            source: src,
            reason_code: rc,
            reason_detail: rd,
            suppressed_until: su,
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SendOutcome;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    // -----------------------------------------------------------------------
    // Mock-ChatApi
    // -----------------------------------------------------------------------

    struct MockApi {
        ban_calls: AtomicUsize,
        delete_calls: AtomicUsize,
        send_calls: AtomicUsize,
        ban_result: Mutex<BanOutcome>,
    }

    impl MockApi {
        fn with_ban_result(result: BanOutcome) -> Arc<Self> {
            Arc::new(Self {
                ban_calls: AtomicUsize::new(0),
                delete_calls: AtomicUsize::new(0),
                send_calls: AtomicUsize::new(0),
                ban_result: Mutex::new(result),
            })
        }
    }

    #[async_trait]
    impl ChatApi for MockApi {
        async fn send_message(&self, _b: &str, _m: &str) -> Result<SendOutcome, String> {
            self.send_calls.fetch_add(1, Ordering::SeqCst);
            Ok(SendOutcome::Sent)
        }
        async fn send_announcement(
            &self,
            _b: &str,
            _m: &str,
            _c: &str,
        ) -> Result<bool, String> {
            Ok(true)
        }
        async fn ban_user(
            &self,
            _b: &str,
            _u: &str,
            _r: &str,
        ) -> Result<BanOutcome, String> {
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
            Ok(BanOutcome::Banned)
        }
        async fn unban_user(&self, _b: &str, _u: &str) -> Result<bool, String> {
            Ok(true)
        }
        async fn delete_message(&self, _b: &str, _m: &str) -> Result<bool, String> {
            self.delete_calls.fetch_add(1, Ordering::SeqCst);
            Ok(true)
        }
        async fn user_created_at(
            &self,
            _u: &str,
        ) -> Result<Option<chrono::DateTime<Utc>>, String> {
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
        assert_eq!(api.delete_calls.load(Ordering::SeqCst), 1, "Delete aufgerufen");
        assert_eq!(api.ban_calls.load(Ordering::SeqCst), 1, "Ban aufgerufen");
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
        assert_eq!(api.ban_calls.load(Ordering::SeqCst), 0, "kein Ban bei ban=false");
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
        assert!(!guard.consume_stream_start_pitch("pitchkanal2"), "zweites Consume: false");
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
