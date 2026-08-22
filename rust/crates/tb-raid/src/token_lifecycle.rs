//! Token-Ausfall-Reaktionen (Block 4) — Port der Discord-/Reaktions-Hälfte von
//! `api/token_error_handler.py` (`notify_token_error`, `_send_user_dm_token_error`,
//! `check_grace_periods`, `restore_bot_banned_channel`, `cleanup_old_entries`).
//!
//! Architektur: Der Twitch-Bot hat KEINEN Discord-Zugang. Alle Discord-Wirkungen
//! (Admin-Embed / User-DM / Rollen-Entzug) laufen über den F4-Master-Broker. Diese
//! Außenkopplung ist ein Port ([`TokenLifecycleNotifier`]) — die Reaktions-Logik
//! (Dedup-Flags, Grace-Sweep, Restore) bleibt ohne Netz testbar.
//!
//! Bewusste Cutover-Abweichung von Python (grillme-Entscheidung Block 4,
//! `token-lifecycle-2`): Die User-DM ist eine **Text-DM mit Re-Auth-Link** statt
//! eines Embeds mit persistentem Button. Der Twitch-Bot kann keine persistenten
//! Discord-Button-Views hosten (kein Discord-Gateway); der Re-Auth läuft über den
//! Website-Aktivierungs-Flow. Der Broker-`send-dm`-Endpunkt nimmt ohnehin nur
//! `user_id` + Text-`content` (kein Embed) entgegen.
//!
//! Schema (`twitch_token_blacklist`, Alt-Stil verifiziert): Timestamps TEXT (ISO),
//! Flags INTEGER. Spalten `notified`/`user_dm_sent`/`reminder_sent`/`role_removed`
//! deduplizieren jede Reaktion: Admin-Embed + User-DM genau **1×/Streamer**,
//! Reminder + Rollen-Entzug genau **1×** je abgelaufener Grace-Period.

use std::sync::Arc;

use chrono::{DateTime, SecondsFormat, Utc};
use sqlx::PgPool;

use crate::token_blacklist::{BLACKLIST_DISABLE_THRESHOLD, GRACE_PERIOD_DAYS};
use crate::util::mask_log_identifier as mask;

/// Admin-Channel für Token-Fehler-Benachrichtigungen (Python
/// `TOKEN_ERROR_CHANNEL_ID`). Konstante 1:1 übernommen.
pub const TOKEN_ERROR_CHANNEL_ID: i64 = 1374364800817303632;

/// Standard-Re-Auth-Ziel: die Website-Streamer-Aktivierungsseite. Per Env
/// `STREAMER_REAUTH_URL` überschreibbar (kein Domain-Raten im Code).
pub const DEFAULT_REAUTH_URL: &str = "https://deadlock-twitch.de/streamer/";

// ---------------------------------------------------------------------------
// Notifier-Port (F4-Broker-Außenkopplung)
// ---------------------------------------------------------------------------

/// Discord-Reaktions-Port — echte Impl im tb-bot-Bin über den F4-`BrokerRelay`.
///
/// Alle Methoden sind **best-effort**: Fehler werden von der Implementierung
/// geloggt, nie propagiert (Python-Parität — eine fehlgeschlagene DM darf den
/// Lockout-/Grace-Pfad nicht abbrechen). Der Rückgabewert `bool` signalisiert
/// nur „zugestellt ja/nein" zur Flag-Steuerung.
#[async_trait::async_trait]
pub trait TokenLifecycleNotifier: Send + Sync {
    /// Admin-Channel-Embed in [`TOKEN_ERROR_CHANNEL_ID`]
    /// (Python `notify_token_error` / `_notify_admin_grace_expired`).
    async fn send_admin_embed(&self, channel_id: i64, title: &str, description: &str) -> bool;

    /// Text-DM an den Streamer (Python `_send_user_dm_token_error`, hier ohne
    /// Embed/Button). `discord_user_id` ist die numerische Discord-ID als String.
    async fn send_user_dm(&self, discord_user_id: &str, content: &str) -> bool;

    /// Streamer-Rolle entziehen (Python `schedule_streamer_role_sync(False)`).
    /// Best-effort; `false` wenn nicht zugestellt.
    async fn revoke_streamer_role(&self, discord_user_id: &str, reason: &str) -> bool;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BotBanStatus {
    NotBanned,
    Banned,
    Unknown,
}

#[async_trait::async_trait]
pub trait BotBanStatusProbe: Send + Sync {
    async fn bot_ban_status(&self, twitch_user_id: &str, twitch_login: &str) -> BotBanStatus;
}

// ---------------------------------------------------------------------------
// Reine Entscheidungs-/Text-Bausteine (ohne DB/Netz — voll unit-testbar)
// ---------------------------------------------------------------------------

/// Admin-Embed-Inhalt bei Token-Fehler (Python `notify_token_error`-Embed,
/// auf den Text-Broker-Pfad reduziert).
pub fn admin_token_error_text(twitch_login: &str, error_message: &str) -> (String, String) {
    let title = "⚠️ Twitch Token Error".to_string();
    let err = truncate_chars(error_message, 200);
    let description = format!(
        "Der Refresh-Token für **{twitch_login}** ist ungültig.\n\n\
         Streamer: [{twitch_login}](https://twitch.tv/{twitch_login})\n\
         Fehler: ```{err}```\n\
         Der Streamer muss den Bot **neu für seinen Kanal aktivieren**. \
         Auto-Raid bleibt deaktiviert bis zur Re-Autorisierung."
    );
    (title, description)
}

/// Admin-Embed-Inhalt bei abgelaufener Grace-Period (Python
/// `_notify_admin_grace_expired`).
pub fn admin_grace_expired_text(
    twitch_login: &str,
    twitch_user_id: &str,
    discord_user_id: Option<&str>,
) -> (String, String) {
    let mention = match discord_user_id {
        Some(id) if !id.is_empty() => format!("<@{id}>"),
        _ => format!("`{twitch_login}`"),
    };
    let title = "🚨 Grace-Period abgelaufen – Streamer-Rolle entzogen".to_string();
    let description = format!(
        "Der Streamer **{twitch_login}** hat seinen Token innerhalb von \
         **{GRACE_PERIOD_DAYS} Tagen** nicht erneuert. Die Streamer-Rolle wurde \
         automatisch entzogen.\n\n\
         Streamer: [{twitch_login}](https://twitch.tv/{twitch_login})\n\
         Discord: {mention}\n\
         User ID: `{twitch_user_id}`\n\
         Bitte kontaktiere {mention} direkt — Re-Auth über die Website stellt die \
         Rolle automatisch wieder her."
    );
    (title, description)
}

/// User-DM-Text bei Token-Fehler (Erst-DM). Text-only mit Re-Auth-Link.
pub fn user_dm_token_error_text(twitch_login: &str, reauth_url: &str) -> String {
    format!(
        "⚠️ **Twitch Bot – Verbindung fehlgeschlagen**\n\n\
         Die Verbindung für **{twitch_login}** ist fehlgeschlagen und muss erneuert \
         werden (z. B. nach Passwort-/2FA-Änderung oder Deautorisierung).\n\n\
         Bis zur erneuten Verbindung bleiben Auto-Raid, Chat-Schutz und Analytics \
         für deinen Kanal deaktiviert. Du hast {GRACE_PERIOD_DAYS} Tage Zeit, bevor \
         die Streamer-Rolle entzogen wird.\n\n\
         🔗 Jetzt neu verbinden: {reauth_url}\n\n\
         Wenn du die Verbindung bewusst entfernt hast und kein Partner mehr sein \
         möchtest, kannst du diese Nachricht ignorieren."
    )
}

/// User-DM-Text als Grace-Reminder (Python `is_reminder=True`).
pub fn user_dm_reminder_text(twitch_login: &str, reauth_url: &str) -> String {
    format!(
        "⚠️ **Twitch Bot – Aktivierung weiterhin ausstehend**\n\n\
         Die Verbindung für **{twitch_login}** wurde seit {GRACE_PERIOD_DAYS} Tagen \
         noch nicht erneuert. Die Bot-Funktionen bleiben deaktiviert, bis dein Kanal \
         wieder verbunden ist.\n\n\
         🔗 Jetzt neu verbinden: {reauth_url}\n\n\
         Wenn du die Verbindung bewusst entfernt hast, kannst du diese Nachricht \
         ignorieren."
    )
}

/// User-DM-Text bei Kanal-seitigem Bot-Ban (Python `_send_user_dm_bot_banned`).
/// Der technische `error_message` fließt bewusst NICHT in die DM (verwirrt den
/// Streamer) — er bleibt im Blacklist-`reason` und in den Logs erhalten.
pub fn user_dm_bot_banned_text(twitch_login: &str, _error_message: &str) -> String {
    format!(
        "⚠️ **Twitch Bot – in deinem Channel blockiert**\n\n\
         Der Bot wurde in **{twitch_login}** gebannt oder als Moderator entfernt. \
         Solange das so ist, pausieren Auto-Raid, Chat-Schutz und Analytics für \
         deinen Kanal.\n\n\
         **So holst du ihn zurück** – schick diese beiden Befehle in deinem eigenen \
         Twitch-Chat:\n\
         1️⃣ Ban aufheben: `/unban deutschedeadlockcommunity`\n\
         2️⃣ Wieder zum Mod machen: `/mod deutschedeadlockcommunity`\n\n\
         Sobald das erledigt ist, läuft alles von allein wieder an – du musst sonst \
         nichts weiter tun. War der Ban Absicht, kannst du diese Nachricht einfach \
         ignorieren."
    )
}

/// Python-Parität für `_get_discord_user_id`: nur rein numerische IDs zählen.
pub fn sanitize_discord_user_id(raw: Option<&str>) -> Option<String> {
    let trimmed = raw.unwrap_or("").trim();
    if !trimmed.is_empty() && trimmed.chars().all(|c| c.is_ascii_digit()) {
        Some(trimmed.to_string())
    } else {
        None
    }
}

/// Schneidet einen String auf höchstens `max` Zeichen (char-sicher).
fn truncate_chars(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

fn bot_banned_blacklist_reason(error_message: &str) -> String {
    let compact = error_message.replace('\n', " ");
    let trimmed = compact.trim();
    if trimmed.is_empty() {
        return "chat_bot_banned_in_channel".to_string();
    }
    format!(
        "chat_bot_banned_in_channel: {}",
        truncate_chars(trimmed, 180)
    )
}

// ---------------------------------------------------------------------------
// Reactor (DB + Notifier)
// ---------------------------------------------------------------------------

/// Ergebnis einer [`TokenLifecycleReactor::notify_token_error`]-Reaktion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NotifyOutcome {
    /// Admin-Channel-Embed gesendet.
    pub admin_sent: bool,
    /// User-DM gesendet.
    pub user_dm_sent: bool,
    /// Bereits zuvor benachrichtigt (notified-Flag gesetzt) → übersprungen.
    pub already_notified: bool,
}

impl NotifyOutcome {
    fn any_sent(&self) -> bool {
        self.admin_sent || self.user_dm_sent
    }
}

/// Ergebnis einer Kanal-seitigen Bot-Ban-Reaktion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BotBannedOutcome {
    /// Auth-/Partner-State wurde auf technischen Opt-out gesetzt.
    pub opt_out_marked: bool,
    /// Recovery-DM wurde ueber den Notifier-Port zugestellt.
    pub user_dm_sent: bool,
    /// Vorher existierte bereits ein `bot_banned`-Blacklist-Grund.
    pub already_flagged: bool,
}

/// Token-Lifecycle-Reaktor: bindet `twitch_token_blacklist` an den Discord-Port.
pub struct TokenLifecycleReactor<N: TokenLifecycleNotifier> {
    pool: PgPool,
    notifier: N,
    reauth_url: String,
    bot_ban_status_probe: Option<Arc<dyn BotBanStatusProbe>>,
}

impl<N: TokenLifecycleNotifier> TokenLifecycleReactor<N> {
    pub fn new(pool: PgPool, notifier: N) -> Self {
        let reauth_url = std::env::var("STREAMER_REAUTH_URL")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| DEFAULT_REAUTH_URL.to_string());
        Self {
            pool,
            notifier,
            reauth_url,
            bot_ban_status_probe: None,
        }
    }

    #[must_use]
    pub fn with_bot_ban_status_probe(mut self, probe: Arc<dyn BotBanStatusProbe>) -> Self {
        self.bot_ban_status_probe = Some(probe);
        self
    }

    fn iso(dt: DateTime<Utc>) -> String {
        dt.to_rfc3339_opts(SecondsFormat::Secs, false)
    }

    /// Token-Fehler-Reaktion: Admin-Embed + User-DM, **genau 1×/Streamer**
    /// (notified-Flag). Port von Python `notify_token_error`.
    ///
    /// Reihenfolge wie Python: notified prüfen → Admin-Embed → User-DM →
    /// bei mindestens einer Zustellung `notified=1` setzen. `user_dm_sent=1`
    /// wird zusätzlich nur bei erfolgreicher DM gesetzt.
    pub async fn notify_token_error(
        &self,
        twitch_user_id: &str,
        twitch_login: &str,
        error_message: &str,
    ) -> NotifyOutcome {
        // Dedup-Gate: notified bereits gesetzt → nichts tun (Python).
        match self.is_notified(twitch_user_id).await {
            Ok(true) => {
                return NotifyOutcome {
                    already_notified: true,
                    ..Default::default()
                };
            }
            Ok(false) => {}
            Err(error) => {
                tracing::error!(%error, user = %mask(twitch_user_id), "notify_token_error: notified-Check fehlgeschlagen");
                return NotifyOutcome::default();
            }
        }

        let (title, description) = admin_token_error_text(twitch_login, error_message);
        let admin_sent = self
            .notifier
            .send_admin_embed(TOKEN_ERROR_CHANNEL_ID, &title, &description)
            .await;

        let discord_user_id = self.discord_user_id_for(twitch_user_id, twitch_login).await;
        let user_dm_sent = if let Some(ref did) = discord_user_id {
            let text = user_dm_token_error_text(twitch_login, &self.reauth_url);
            let sent = self.notifier.send_user_dm(did, &text).await;
            if sent {
                self.set_user_dm_sent(twitch_user_id).await;
            }
            sent
        } else {
            false
        };

        let outcome = NotifyOutcome {
            admin_sent,
            user_dm_sent,
            already_notified: false,
        };

        if outcome.any_sent() {
            self.set_notified(twitch_user_id).await;
        }

        tracing::info!(
            user = %mask(twitch_user_id),
            admin = outcome.admin_sent,
            user_dm = outcome.user_dm_sent,
            "Token-Fehler-Reaktion verarbeitet"
        );
        outcome
    }

    /// Sweep über alle blacklisteten, noch nicht benachrichtigten Streamer und
    /// löst je einmalig [`Self::notify_token_error`] aus. Native Entsprechung des
    /// reaktiven Python-Aufrufs aus dem Refresh-Fehlerpfad: Da im Rust-Cutover der
    /// Refresh-Schreibpfad (`tb-raid`) bewusst KEINE Discord-Kopplung hat, holt
    /// dieser Sweep die Reaktion nach. Das `notified`-Flag garantiert „genau
    /// 1×/Streamer" — egal ob reaktiv oder per Sweep ausgelöst.
    /// Liefert die Anzahl tatsächlich neu benachrichtigter Streamer.
    ///
    /// Parität: Python feuert `notify_token_error` schon beim **ersten**
    /// `invalid_grant` (direkt nach `add_to_blacklist`), nicht erst ab dem
    /// dritten Fehler. Der Eintrag existiert ab dem ersten Fehler
    /// (`add_to_blacklist_inner` INSERTet ihn), darum genügt hier „Eintrag
    /// existiert UND notified=0".
    pub async fn notify_pending_errors(&self) -> u64 {
        let pending = sqlx::query!(
            r#"
            SELECT twitch_user_id AS "twitch_user_id!",
                   twitch_login AS "twitch_login!",
                   error_message AS "error_message?"
            FROM twitch_token_blacklist
            WHERE COALESCE(notified, 0) = 0
            "#,
        )
        .fetch_all(&self.pool)
        .await;
        let rows = match pending {
            Ok(rows) => rows,
            Err(error) => {
                tracing::error!(%error, "notify_pending_errors: DB-Query fehlgeschlagen");
                return 0;
            }
        };
        let mut notified = 0u64;
        for row in rows {
            let outcome = self
                .notify_token_error(
                    &row.twitch_user_id,
                    &row.twitch_login,
                    row.error_message
                        .as_deref()
                        .unwrap_or("invalid refresh grant"),
                )
                .await;
            if outcome.any_sent() {
                notified += 1;
            }
        }
        notified
    }

    /// Stündlicher Grace-Sweep (Python `check_grace_periods`): für jede Zeile mit
    /// abgelaufener Grace-Period (`error_count >= 3`, `grace_expires_at <= now`,
    /// `role_removed = 0`)
    /// sendet er einmalig Reminder-DM + Admin-Notify
    /// (reminder_sent), entzieht die Streamer-Rolle und setzt
    /// `manual_partner_opt_out=1`, `technical_pause_reason='token_error_expired'`
    /// und `role_removed=1`. Liefert die Anzahl bearbeiteter Streamer.
    pub async fn check_grace_periods(&self) -> u64 {
        let now_iso = Self::iso(Utc::now());
        let expired = match self.load_expired_grace(&now_iso).await {
            Ok(rows) => rows,
            Err(error) => {
                tracing::error!(%error, "check_grace_periods: DB-Query fehlgeschlagen");
                return 0;
            }
        };

        let mut processed = 0u64;
        for row in expired {
            let discord_user_id = self
                .discord_user_id_for(&row.twitch_user_id, &row.twitch_login)
                .await;

            // 1. Einmalig: Reminder-DM + Admin-Notify.
            if row.reminder_sent.unwrap_or(0) == 0 {
                if let Some(ref did) = discord_user_id {
                    let text = user_dm_reminder_text(&row.twitch_login, &self.reauth_url);
                    self.notifier.send_user_dm(did, &text).await;
                }
                let (title, description) = admin_grace_expired_text(
                    &row.twitch_login,
                    &row.twitch_user_id,
                    discord_user_id.as_deref(),
                );
                self.notifier
                    .send_admin_embed(TOKEN_ERROR_CHANNEL_ID, &title, &description)
                    .await;
                self.set_reminder_sent(&row.twitch_user_id).await;
            }

            // 2. Streamer-Rolle entziehen (best-effort via Broker).
            if let Some(ref did) = discord_user_id {
                let reason = format!(
                    "Twitch-Token seit {GRACE_PERIOD_DAYS} Tagen ungültig – Grace-Period abgelaufen"
                );
                self.notifier.revoke_streamer_role(did, &reason).await;
            }

            // 3. DB-State: abgelaufener Token-Error + manueller Opt-out + role_removed.
            if let Err(error) = self
                .mark_grace_expired(&row.twitch_user_id, &row.twitch_login)
                .await
            {
                tracing::warn!(%error, user = %mask(&row.twitch_user_id), "Grace-Expiry-State nicht setzbar");
            } else {
                processed += 1;
                tracing::info!(
                    user = %mask(&row.twitch_user_id),
                    "Grace-Period abgelaufen – Rolle entzogen, token_error_expired gesetzt"
                );
            }
        }
        processed
    }

    /// Blacklist-Cleanup (Python `cleanup_old_entries`): löscht Einträge, deren
    /// letzter Fehler älter als `days` Tage ist. Liefert die Anzahl gelöschter Zeilen.
    pub async fn cleanup_old_entries(&self, days: i64) -> u64 {
        let cutoff = Utc::now() - chrono::Duration::days(days);
        let cutoff_iso = Self::iso(cutoff);
        match sqlx::query!(
            "DELETE FROM twitch_token_blacklist WHERE last_error_at < $1",
            &cutoff_iso
        )
        .execute(&self.pool)
        .await
        {
            Ok(result) => {
                let deleted = result.rows_affected();
                if deleted > 0 {
                    tracing::info!(deleted, days, "Alte Token-Blacklist-Einträge entfernt");
                }
                deleted
            }
            Err(error) => {
                tracing::error!(%error, "Token-Blacklist-Cleanup fehlgeschlagen");
                0
            }
        }
    }

    /// Restore nach aufgehobenem Kanal-Ban. Ein gesunder Streamer-Token ist nur
    /// Vorbedingung fuer die echte Ban-Pruefung, nie selbst der Restore-Beweis.
    pub async fn restore_bot_banned_channel(
        &self,
        twitch_user_id: &str,
        twitch_login: &str,
    ) -> bool {
        let needs_reauth = match sqlx::query_scalar::<_, Option<bool>>(
            "SELECT needs_reauth FROM twitch_raid_auth WHERE twitch_user_id = $1 LIMIT 1",
        )
        .bind(twitch_user_id)
        .fetch_optional(&self.pool)
        .await
        {
            Ok(Some(needs_reauth)) => needs_reauth.unwrap_or(true),
            Ok(None) => {
                tracing::info!(
                    login = twitch_login,
                    urteil = "unsicher",
                    grund = "keine Auth-Zeile",
                    "Bot-Ban-Restore-Entscheidung"
                );
                return false;
            }
            Err(error) => {
                tracing::warn!(
                    %error,
                    login = twitch_login,
                    urteil = "fehler",
                    grund = "Auth-Status nicht lesbar",
                    "Bot-Ban-Restore-Entscheidung"
                );
                return false;
            }
        };
        if needs_reauth {
            tracing::info!(
                login = twitch_login,
                urteil = "nein",
                grund = "Streamer-Token braucht Reauth",
                "Bot-Ban-Restore-Entscheidung"
            );
            return false;
        }

        // Getrennte/archivierte Kanäle nie restaurieren — und vor allem nicht
        // proben: der Ban-Probe läuft über `ensure_bot_is_mod` und würde den Bot
        // im getrennten Kanal als Nebenwirkung wieder als Moderator setzen.
        // Fail-closed: ist der Zustand nicht lesbar, wird nicht restauriert.
        match self
            .partner_disconnect_state(twitch_user_id, twitch_login)
            .await
        {
            Ok(Some(state)) => {
                tracing::info!(
                    login = twitch_login,
                    urteil = "nein",
                    grund = state,
                    "Bot-Ban-Restore-Entscheidung"
                );
                return false;
            }
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(
                    %error,
                    login = twitch_login,
                    urteil = "fehler",
                    grund = "Partner-Status nicht lesbar",
                    "Bot-Ban-Restore-Entscheidung"
                );
                return false;
            }
        }

        let Some(probe) = &self.bot_ban_status_probe else {
            tracing::info!(
                login = twitch_login,
                urteil = "unsicher",
                grund = "kein Ban-Status-Provisioner verdrahtet",
                "Bot-Ban-Restore-Entscheidung"
            );
            return false;
        };
        match probe.bot_ban_status(twitch_user_id, twitch_login).await {
            BotBanStatus::Banned => {
                tracing::info!(
                    login = twitch_login,
                    urteil = "nein",
                    grund = "Bot ist weiterhin im Kanal gebannt",
                    "Bot-Ban-Restore-Entscheidung"
                );
                return false;
            }
            BotBanStatus::Unknown => {
                tracing::info!(
                    login = twitch_login,
                    urteil = "unsicher",
                    grund = "Ban-Status konnte nicht sicher bestimmt werden",
                    "Bot-Ban-Restore-Entscheidung"
                );
                return false;
            }
            BotBanStatus::NotBanned => {}
        }

        match self
            .restore_bot_banned_inner(twitch_user_id, twitch_login)
            .await
        {
            Ok(restored) => {
                tracing::info!(
                    login = twitch_login,
                    urteil = if restored { "ja" } else { "nein" },
                    grund = if restored {
                        "Bot ist nicht mehr gebannt"
                    } else {
                        "Zustand nicht mehr fuer Bot-Ban-Restore geeignet"
                    },
                    "Bot-Ban-Restore-Entscheidung"
                );
                restored
            }
            Err(error) => {
                tracing::warn!(
                    %error,
                    login = twitch_login,
                    urteil = "fehler",
                    grund = "DB-Restore fehlgeschlagen",
                    "Bot-Ban-Restore-Entscheidung"
                );
                false
            }
        }
    }

    /// Kanal-seitiger Bot-Ban (Python `handle_bot_banned_channel`):
    /// Raid fuer diesen Partner technisch deaktivieren, Bot-Ban-Blacklist setzen
    /// und dem Streamer genau einmal eine Recovery-DM senden.
    pub async fn handle_bot_banned_channel(
        &self,
        twitch_user_id: &str,
        twitch_login: &str,
        error_message: &str,
    ) -> BotBannedOutcome {
        let already_flagged = match self
            .mark_bot_banned_inner(twitch_user_id, twitch_login, error_message)
            .await
        {
            Ok(already_flagged) => already_flagged,
            Err(error) => {
                tracing::warn!(%error, user = %mask(twitch_user_id), "Bot-Ban-Opt-out fehlgeschlagen");
                return BotBannedOutcome::default();
            }
        };
        if already_flagged {
            return BotBannedOutcome {
                already_flagged: true,
                ..Default::default()
            };
        }

        let discord_user_id = self.discord_user_id_for(twitch_user_id, twitch_login).await;
        let user_dm_sent = if let Some(ref did) = discord_user_id {
            let text = user_dm_bot_banned_text(twitch_login, error_message);
            self.notifier.send_user_dm(did, &text).await
        } else {
            false
        };

        tracing::info!(
            user = %mask(twitch_login),
            user_dm = user_dm_sent,
            "Bot-Ban-Opt-out verarbeitet"
        );
        BotBannedOutcome {
            opt_out_marked: true,
            user_dm_sent,
            already_flagged: false,
        }
    }

    /// Stündlicher Restore-Sweep für technische Bot-Ban-Pausen. Selektiert nur
    /// echte Bot-Ban-Zustände (`bot_banned`, Bot-Ban-Blacklist-Marker oder
    /// Legacy-Manual-Opt-out) und delegiert die Sicherheitslogik an
    /// [`Self::restore_bot_banned_channel`].
    pub async fn restore_ready_bot_banned_channels(&self) -> u64 {
        let rows = match sqlx::query_as::<_, (String, String)>(
            r#"
            SELECT DISTINCT
                   ra.twitch_user_id,
                   COALESCE(
                       NULLIF(LOWER(ra.twitch_login), ''),
                       NULLIF(LOWER(p.twitch_login), ''),
                       NULLIF(LOWER(rb.target_login), ''),
                       ''
                   ) AS twitch_login
              FROM twitch_raid_auth ra
              LEFT JOIN twitch_partners p
                ON p.twitch_user_id = ra.twitch_user_id
                OR LOWER(p.twitch_login) = LOWER(ra.twitch_login)
              LEFT JOIN twitch_raid_blacklist rb
                ON (
                       rb.target_id = ra.twitch_user_id
                       OR LOWER(rb.target_login) = COALESCE(
                              NULLIF(LOWER(ra.twitch_login), ''),
                              NULLIF(LOWER(p.twitch_login), ''),
                              ''
                          )
                   )
               AND LOWER(COALESCE(rb.reason, '')) LIKE '%bot_banned%'
             WHERE (
                    LOWER(TRIM(COALESCE(p.technical_pause_reason, ''))) = 'bot_banned'
                    OR rb.target_login IS NOT NULL
                    OR (
                        COALESCE(p.manual_partner_opt_out, 0) = 1
                        AND COALESCE(TRIM(p.technical_pause_reason), '') = ''
                        AND COALESCE(ra.raid_enabled, FALSE) = FALSE
                    )
               )
               -- Bewusst getrennte oder archivierte Kanäle bleiben draußen.
               -- `disconnect-bot` hinterlässt exakt die Signatur des
               -- Legacy-Opt-out-Zweigs (opt_out=1, keine Pause, raid_enabled=0);
               -- ohne diesen Filter hat der Sweep die Trennung Stunden später
               -- teilweise zurückgedreht und den Bot dabei wieder gemoddet.
               AND NOT EXISTS (
                   SELECT 1
                     FROM twitch_partners dp
                    WHERE (dp.twitch_user_id = ra.twitch_user_id
                        OR LOWER(dp.twitch_login) = LOWER(ra.twitch_login))
                      AND (
                           LOWER(TRIM(COALESCE(dp.status, ''))) IN ('departnered', 'archived')
                        OR dp.departnered_at IS NOT NULL
                        OR dp.admin_archived_at IS NOT NULL
                      )
               )
            "#,
        )
        .fetch_all(&self.pool)
        .await
        {
            Ok(rows) => rows,
            Err(error) => {
                tracing::warn!(%error, "Bot-Ban-Restore-Sweep: DB-Query fehlgeschlagen");
                return 0;
            }
        };

        let mut restored = 0u64;
        for (twitch_user_id, twitch_login) in rows {
            if self
                .restore_bot_banned_channel(&twitch_user_id, &twitch_login)
                .await
            {
                restored += 1;
            }
        }
        restored
    }

    /// Reaktiviert Partner, die nur wegen `token_error*` pausiert sind, wenn die
    /// Auth-Zeile DB-verifizierbar gesund ist und kein Bot-Ban-/Blocked-Marker
    /// vorliegt. Das ist bewusst getrennt vom Bot-Ban-Restore.
    pub async fn reactivate_token_error_partners_with_valid_auth(&self) -> u64 {
        let count = sqlx::query_scalar::<_, i64>(
            r#"
            WITH eligible AS (
                SELECT DISTINCT
                       p.twitch_user_id,
                       COALESCE(NULLIF(LOWER(p.twitch_login), ''),
                                NULLIF(LOWER(a.twitch_login), ''),
                                '') AS twitch_login
                  FROM twitch_partners p
                  JOIN twitch_raid_auth a
                    ON a.twitch_user_id = p.twitch_user_id
                 WHERE LOWER(TRIM(COALESCE(p.status, ''))) = 'active'
                   AND LOWER(TRIM(COALESCE(p.technical_pause_reason, ''))) LIKE 'token_error%'
                   AND COALESCE(a.needs_reauth, TRUE) = FALSE
                   AND a.access_token_enc IS NOT NULL
                   AND OCTET_LENGTH(a.access_token_enc) > 0
                   AND a.token_expires_at IS NOT NULL
                   AND a.token_expires_at > NOW()
                   AND NOT EXISTS (
                       SELECT 1
                         FROM twitch_partners hp
                        WHERE (hp.twitch_user_id = p.twitch_user_id
                            OR LOWER(hp.twitch_login) = LOWER(p.twitch_login))
                          AND LOWER(TRIM(COALESCE(hp.technical_pause_reason, '')))
                              IN ('blocked', 'bot_banned')
                   )
                   AND NOT EXISTS (
                       SELECT 1
                         FROM twitch_raid_blacklist rb
                        WHERE (rb.target_id = p.twitch_user_id
                            OR LOWER(rb.target_login) = LOWER(p.twitch_login))
                          AND LOWER(COALESCE(rb.reason, '')) LIKE '%bot_banned%'
                   )
            ),
            updated_partners AS (
                UPDATE twitch_partners p
                   SET technical_pause_reason = NULL,
                       manual_partner_opt_out = 0,
                       raid_bot_enabled = 1
                  FROM eligible e
                 WHERE p.twitch_user_id = e.twitch_user_id
                   AND LOWER(TRIM(COALESCE(p.technical_pause_reason, ''))) LIKE 'token_error%'
                RETURNING p.twitch_user_id
            ),
            updated_auth AS (
                UPDATE twitch_raid_auth a
                   SET raid_enabled = TRUE,
                       needs_reauth = FALSE,
                       reauth_notified_at = NULL
                  FROM eligible e
                 WHERE a.twitch_user_id = e.twitch_user_id
                RETURNING a.twitch_user_id
            ),
            deleted_blacklist AS (
                DELETE FROM twitch_token_blacklist b
                 USING eligible e
                 WHERE b.twitch_user_id = e.twitch_user_id
                RETURNING b.twitch_user_id
            )
            SELECT COUNT(*)::BIGINT FROM updated_partners
            "#,
        )
        .fetch_one(&self.pool)
        .await;
        match count {
            Ok(count) => count.max(0) as u64,
            Err(error) => {
                tracing::warn!(%error, "Token-Error-Reactivation-Sweep fehlgeschlagen");
                0
            }
        }
    }

    /// Reconciliation: aktiviert raid_bot_enabled für aktive Partner mit nachweislich
    /// gesundem Raid-Token, deren Partner-Toggle (durch alten Token-Error-Pfad)
    /// auf 0 hängt, OHNE technische Pause. Schließt die Lücke, die der
    /// Bot-Ban/Token-Error-Restore nicht abdeckt. Idempotent. Liefert Anzahl geheilter Zeilen.
    pub async fn reconcile_healthy_raid_toggles(&self) -> u64 {
        match sqlx::query!(
            r#"
            UPDATE twitch_partners p
               SET raid_bot_enabled = 1
              FROM twitch_raid_auth a
             WHERE a.twitch_user_id = p.twitch_user_id
               AND LOWER(TRIM(COALESCE(p.status, ''))) = 'active'
               AND COALESCE(p.raid_bot_enabled, 0) = 0
               AND COALESCE(p.manual_partner_opt_out, 0) = 0
               AND COALESCE(TRIM(p.technical_pause_reason), '') = ''
               AND a.raid_enabled IS TRUE
               AND COALESCE(a.needs_reauth, TRUE) = FALSE
            "#,
        )
        .execute(&self.pool)
        .await
        {
            Ok(result) => result.rows_affected(),
            Err(error) => {
                tracing::warn!(%error, "Raid-Toggle-Reconciliation-Sweep fehlgeschlagen");
                0
            }
        }
    }

    // -- DB-Helfer --------------------------------------------------------

    async fn is_notified(&self, twitch_user_id: &str) -> Result<bool, sqlx::Error> {
        let row: Option<Option<i32>> = sqlx::query_scalar!(
            r#"SELECT notified AS "notified?" FROM twitch_token_blacklist WHERE twitch_user_id = $1"#,
            twitch_user_id
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(matches!(row, Some(Some(n)) if n == 1))
    }

    async fn set_notified(&self, twitch_user_id: &str) {
        if let Err(error) = sqlx::query!(
            "UPDATE twitch_token_blacklist SET notified = 1 WHERE twitch_user_id = $1",
            twitch_user_id
        )
        .execute(&self.pool)
        .await
        {
            tracing::warn!(%error, user = %mask(twitch_user_id), "notified-Flag nicht setzbar");
        }
    }

    async fn set_user_dm_sent(&self, twitch_user_id: &str) {
        if let Err(error) = sqlx::query!(
            "UPDATE twitch_token_blacklist SET user_dm_sent = 1 WHERE twitch_user_id = $1",
            twitch_user_id
        )
        .execute(&self.pool)
        .await
        {
            tracing::debug!(%error, user = %mask(twitch_user_id), "user_dm_sent-Flag nicht setzbar");
        }
    }

    async fn set_reminder_sent(&self, twitch_user_id: &str) {
        if let Err(error) = sqlx::query!(
            "UPDATE twitch_token_blacklist SET reminder_sent = 1 WHERE twitch_user_id = $1",
            twitch_user_id
        )
        .execute(&self.pool)
        .await
        {
            tracing::warn!(%error, user = %mask(twitch_user_id), "reminder_sent-Flag nicht setzbar");
        }
    }

    async fn load_expired_grace(&self, now_iso: &str) -> Result<Vec<ExpiredGraceRow>, sqlx::Error> {
        sqlx::query_as::<_, ExpiredGraceRow>(
            r#"
            SELECT twitch_user_id,
                   twitch_login,
                   reminder_sent
            FROM twitch_token_blacklist
            WHERE error_count >= $1
              AND grace_expires_at IS NOT NULL
              AND grace_expires_at <= $2
              AND role_removed = 0
            "#,
        )
        .bind(BLACKLIST_DISABLE_THRESHOLD as i32)
        .bind(now_iso)
        .fetch_all(&self.pool)
        .await
    }

    /// Grace-Block: Partner technisch pausieren, Raid-Auth invalidieren und
    /// `role_removed=1` setzen. In einer Transaktion (idempotent).
    async fn mark_grace_expired(
        &self,
        twitch_user_id: &str,
        twitch_login: &str,
    ) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            r#"
            UPDATE twitch_partners
               SET manual_partner_opt_out = 1,
                   technical_pause_reason = 'token_error_expired',
                   raid_bot_enabled = 0
             WHERE twitch_user_id = $1
               AND (
                    COALESCE(TRIM(technical_pause_reason), '') = ''
                    OR LOWER(TRIM(COALESCE(technical_pause_reason, ''))) LIKE 'token_error%'
               )
               AND LOWER(TRIM(COALESCE(technical_pause_reason, '')))
                   NOT IN ('blocked', 'bot_banned')
            "#,
        )
        .bind(twitch_user_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query!(
            r#"
            UPDATE twitch_raid_auth
               SET raid_enabled = FALSE,
                   needs_reauth = TRUE,
                   twitch_login = COALESCE(NULLIF($1, ''), twitch_login)
             WHERE twitch_user_id = $2
                OR LOWER(twitch_login) = LOWER($1)
            "#,
            twitch_login,
            twitch_user_id
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query!(
            "UPDATE twitch_token_blacklist SET role_removed = 1 WHERE twitch_user_id = $1",
            twitch_user_id
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Markiert einen Kanal als Bot-Ban-Opt-out. Rueckgabe `true` bedeutet:
    /// vor dem Update war bereits ein `bot_banned`-Grund vorhanden, also keine
    /// erneute DM-Reaktion.
    async fn mark_bot_banned_inner(
        &self,
        twitch_user_id: &str,
        twitch_login: &str,
        error_message: &str,
    ) -> Result<bool, sqlx::Error> {
        let login_hint = twitch_login.trim().to_lowercase();
        if login_hint.is_empty() {
            return Ok(true);
        }
        let target_id = twitch_user_id.trim();
        let target_id = (!target_id.is_empty()).then_some(target_id);
        let reason = bot_banned_blacklist_reason(error_message);
        let added_at = Self::iso(Utc::now());

        let mut tx = self.pool.begin().await?;
        let existing_reason: Option<Option<String>> = sqlx::query_scalar!(
            r#"SELECT reason AS "reason?" FROM twitch_raid_blacklist WHERE LOWER(target_login) = LOWER($1) LIMIT 1"#,
            &login_hint
        )
        .fetch_optional(&mut *tx)
        .await?;
        let already_flagged = existing_reason
            .flatten()
            .map(|reason| reason.to_lowercase().contains("bot_banned"))
            .unwrap_or(false);

        if let Some(tid) = target_id {
            sqlx::query!(
                "DELETE FROM twitch_raid_blacklist
                  WHERE target_id = $1 AND LOWER(target_login) <> $2",
                tid,
                &login_hint
            )
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query!(
            r#"
            INSERT INTO twitch_raid_blacklist (target_id, target_login, reason, added_at)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (target_login) DO UPDATE SET
                target_id = COALESCE(EXCLUDED.target_id, twitch_raid_blacklist.target_id),
                reason = EXCLUDED.reason,
                added_at = EXCLUDED.added_at
            "#,
            target_id,
            &login_hint,
            &reason,
            &added_at
        )
        .execute(&mut *tx)
        .await?;

        if already_flagged {
            tx.commit().await?;
            return Ok(true);
        }

        sqlx::query!(
            r#"
            UPDATE twitch_raid_auth
               SET raid_enabled = FALSE,
                   twitch_login = COALESCE(NULLIF($1, ''), twitch_login)
             WHERE twitch_user_id = $2
            "#,
            &login_hint,
            twitch_user_id
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query!(
            r#"
            UPDATE twitch_partners
               SET technical_pause_reason = 'bot_banned',
                   raid_bot_enabled = 0,
                   twitch_login = COALESCE(NULLIF($1, ''), twitch_login)
             WHERE twitch_user_id = $2
                OR LOWER(twitch_login) = LOWER($1)
            "#,
            &login_hint,
            twitch_user_id
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(false)
    }

    /// Kern von `restore_bot_banned_channel`: nur restaurieren, wenn die Auth-Zeile
    /// existiert UND `needs_reauth = FALSE` (Kanal wieder gesund). Hebt nur echte
    /// Bot-Ban-Zustände auf und re-aktiviert Raid, sofern kein manueller Opt-out
    /// vorliegt.
    /// Ist der Kanal bewusst getrennt (`disconnect-bot`) oder archiviert?
    /// Liefert `Some(grund)` für den Log, `None` wenn nichts dagegen spricht.
    /// Getrennt heißt: kein Partner mehr, weder als Raid-Quelle noch als Ziel.
    /// Ein Bot-Ban-Restore hätte hier nichts zu heilen, es gibt keinen Ban.
    async fn partner_disconnect_state(
        &self,
        twitch_user_id: &str,
        twitch_login: &str,
    ) -> Result<Option<&'static str>, sqlx::Error> {
        let login_hint = twitch_login.trim().to_lowercase();
        let row = sqlx::query_as::<_, (Option<String>, Option<String>, Option<String>)>(
            r#"
            SELECT status, departnered_at::TEXT, admin_archived_at::TEXT
              FROM twitch_partners
             WHERE twitch_user_id = $1
                OR ($2 <> '' AND LOWER(twitch_login) = $2)
             LIMIT 1
            "#,
        )
        .bind(twitch_user_id)
        .bind(&login_hint)
        .fetch_optional(&self.pool)
        .await?;
        let Some((status, departnered_at, archived_at)) = row else {
            return Ok(None);
        };
        let status = status.unwrap_or_default().trim().to_lowercase();
        if status == "archived" || archived_at.is_some() {
            return Ok(Some("Kanal ist archiviert"));
        }
        if status == "departnered" || departnered_at.is_some() {
            return Ok(Some("Kanal ist bewusst getrennt"));
        }
        Ok(None)
    }

    async fn restore_bot_banned_inner(
        &self,
        twitch_user_id: &str,
        twitch_login: &str,
    ) -> Result<bool, sqlx::Error> {
        let login_hint = twitch_login.trim().to_lowercase();
        let mut tx = self.pool.begin().await?;

        let auth = sqlx::query!(
            r#"SELECT raid_enabled AS "raid_enabled?",
                      needs_reauth AS "needs_reauth?"
                 FROM twitch_raid_auth
                WHERE twitch_user_id = $1
                LIMIT 1"#,
            twitch_user_id
        )
        .fetch_optional(&mut *tx)
        .await?;
        let Some(auth) = auth else {
            tx.commit().await?;
            return Ok(false);
        };
        // Kanal noch nicht gesund → nicht restaurieren.
        if auth.needs_reauth.unwrap_or(true) {
            tx.commit().await?;
            return Ok(false);
        }

        let blacklist_marker = sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS (
                SELECT 1
                 FROM twitch_raid_blacklist
                 WHERE (target_id = $1 OR LOWER(target_login) = LOWER($2))
                   AND LOWER(COALESCE(reason, '')) LIKE '%bot_banned%'
            )
            "#,
        )
        .bind(twitch_user_id)
        .bind(&login_hint)
        .fetch_one(&mut *tx)
        .await?;

        // Liegt überhaupt ein technischer Bot-Ban vor? (Partner-Pause/Blacklist
        // oder Legacy-Opt-out-Zustand.)
        // `manual_partner_opt_out` ist in `twitch_partners` ein INTEGER-Flag
        // (DEFAULT 0, Python liest es als `bool(...)`) — daher als i32 dekodieren
        // und gegen 0 prüfen. Ein bool-Decode würde am int4-Spaltentyp scheitern.
        let partner = sqlx::query!(
            r#"
            SELECT manual_partner_opt_out AS "manual_partner_opt_out?",
                   technical_pause_reason AS "technical_pause_reason?"
            FROM twitch_partners
            WHERE twitch_user_id = $1
               OR LOWER(twitch_login) = LOWER($2)
            LIMIT 1
            "#,
            twitch_user_id,
            &login_hint
        )
        .fetch_optional(&mut *tx)
        .await?;
        let (manual_opt_out, pause_reason) = match partner {
            Some(row) => (
                row.manual_partner_opt_out.unwrap_or(0) != 0,
                row.technical_pause_reason
                    .unwrap_or_default()
                    .trim()
                    .to_lowercase(),
            ),
            None => (false, String::new()),
        };
        // Zweiter Halt innerhalb der Transaktion: zwischen Kandidaten-Query und
        // hier kann die Trennung passiert sein. Ohne diesen Check würde ein
        // bewusst getrennter Kanal über den Legacy-Opt-out-Zweig unten wieder
        // auf `manual_partner_opt_out = 0` und `raid_enabled = true` gesetzt.
        let disconnected = sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS (
                SELECT 1
                  FROM twitch_partners
                 WHERE (twitch_user_id = $1 OR ($2 <> '' AND LOWER(twitch_login) = $2))
                   AND (
                        LOWER(TRIM(COALESCE(status, ''))) IN ('departnered', 'archived')
                     OR departnered_at IS NOT NULL
                     OR admin_archived_at IS NOT NULL
                   )
            )
            "#,
        )
        .bind(twitch_user_id)
        .bind(&login_hint)
        .fetch_one(&mut *tx)
        .await?;
        if disconnected {
            tx.commit().await?;
            return Ok(false);
        }

        let legacy_manual_opt_out_state =
            pause_reason.is_empty() && manual_opt_out && !auth.raid_enabled.unwrap_or(false);
        let restores_bot_banned =
            blacklist_marker || pause_reason == "bot_banned" || legacy_manual_opt_out_state;
        if !restores_bot_banned {
            tx.commit().await?;
            return Ok(false);
        }

        // Restore: Pause-Reason löschen; Raid nur re-aktivieren, wenn kein manueller
        // Opt-out vorliegt (Python-Parität).
        let reenable = !manual_opt_out || legacy_manual_opt_out_state;
        sqlx::query(
            r#"
            DELETE FROM twitch_raid_blacklist
            WHERE (target_id = $1 OR LOWER(target_login) = LOWER($2))
              AND LOWER(COALESCE(reason, '')) LIKE '%bot_banned%'
            "#,
        )
        .bind(twitch_user_id)
        .bind(&login_hint)
        .execute(&mut *tx)
        .await?;
        sqlx::query!(
            r#"
            UPDATE twitch_raid_auth
               SET raid_enabled = $1,
                   twitch_login = COALESCE(NULLIF($2, ''), twitch_login)
             WHERE twitch_user_id = $3
            "#,
            reenable,
            &login_hint,
            twitch_user_id
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            r#"
            UPDATE twitch_partners
               SET technical_pause_reason = NULL,
                   manual_partner_opt_out = CASE WHEN $1 THEN 0 ELSE manual_partner_opt_out END,
                   raid_bot_enabled = CASE WHEN $2 THEN 1 ELSE raid_bot_enabled END
             WHERE twitch_user_id = $3
                OR LOWER(twitch_login) = LOWER($4)
            "#,
        )
        .bind(legacy_manual_opt_out_state)
        .bind(reenable)
        .bind(twitch_user_id)
        .bind(&login_hint)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(true)
    }

    /// Discord-User-ID eines Streamers (Python `_get_discord_user_id`): aus
    /// `twitch_streamer_identities`, nur rein numerische IDs.
    async fn discord_user_id_for(
        &self,
        twitch_user_id: &str,
        twitch_login: &str,
    ) -> Option<String> {
        let login = tb_domain::normalize_twitch_login(twitch_login).unwrap_or_default();
        let row: Result<Option<String>, _> = sqlx::query_scalar!(
            r#"
            SELECT discord_user_id AS "discord_user_id?"
            FROM twitch_streamer_identities
            WHERE ($1 <> '' AND twitch_user_id = $1)
               OR ($2 <> '' AND LOWER(twitch_login) = $2)
            ORDER BY updated_at DESC
            LIMIT 1
            "#,
            twitch_user_id.trim(),
            &login
        )
        .fetch_optional(&self.pool)
        .await
        .map(Option::flatten);
        match row {
            Ok(raw) => sanitize_discord_user_id(raw.as_deref()),
            Err(error) => {
                tracing::warn!(%error, user = %mask(twitch_login), "discord_user_id-Lookup fehlgeschlagen");
                None
            }
        }
    }
}

#[derive(sqlx::FromRow)]
struct ExpiredGraceRow {
    twitch_user_id: String,
    twitch_login: String,
    reminder_sent: Option<i32>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    type PartnerStateRow = (String, Option<i32>, Option<String>, Option<i32>);

    /// Zählender Fake-Notifier: zählt Admin-Embeds / User-DMs / Rollen-Entzüge.
    #[derive(Default)]
    struct CountingNotifier {
        admin_embeds: AtomicUsize,
        user_dms: AtomicUsize,
        role_revokes: AtomicUsize,
        last_dm: std::sync::Mutex<Option<String>>,
    }

    #[async_trait::async_trait]
    impl TokenLifecycleNotifier for Arc<CountingNotifier> {
        async fn send_admin_embed(&self, _channel: i64, _title: &str, _desc: &str) -> bool {
            self.admin_embeds.fetch_add(1, Ordering::SeqCst);
            true
        }
        async fn send_user_dm(&self, _did: &str, content: &str) -> bool {
            self.user_dms.fetch_add(1, Ordering::SeqCst);
            *self.last_dm.lock().unwrap() = Some(content.to_string());
            true
        }
        async fn revoke_streamer_role(&self, _did: &str, _reason: &str) -> bool {
            self.role_revokes.fetch_add(1, Ordering::SeqCst);
            true
        }
    }

    struct FixedBotBanStatus(BotBanStatus);

    #[async_trait::async_trait]
    impl BotBanStatusProbe for FixedBotBanStatus {
        async fn bot_ban_status(&self, _twitch_user_id: &str, _twitch_login: &str) -> BotBanStatus {
            self.0
        }
    }

    /// Zählt die Probe-Aufrufe. In Prod hängt am Probe ein `ensure_bot_is_mod`,
    /// jeder Aufruf ist also ein potenzieller Remod im fremden Kanal.
    #[derive(Default)]
    struct CountingBotBanStatus(AtomicUsize);

    #[async_trait::async_trait]
    impl BotBanStatusProbe for CountingBotBanStatus {
        async fn bot_ban_status(&self, _twitch_user_id: &str, _twitch_login: &str) -> BotBanStatus {
            self.0.fetch_add(1, Ordering::SeqCst);
            BotBanStatus::NotBanned
        }
    }

    // --- Reine Logik (kein DB nötig) ------------------------------------

    #[tokio::test]
    async fn notifier_zaehlt_admin_und_dm() {
        // Verifiziert die Port-Mechanik direkt: 1 Admin-Embed + 1 User-DM.
        let n = Arc::new(CountingNotifier::default());
        let (t, d) = admin_token_error_text("foo", "invalid_grant");
        n.send_admin_embed(TOKEN_ERROR_CHANNEL_ID, &t, &d).await;
        let text = user_dm_token_error_text("foo", DEFAULT_REAUTH_URL);
        n.send_user_dm("123", &text).await;
        assert_eq!(n.admin_embeds.load(Ordering::SeqCst), 1);
        assert_eq!(n.user_dms.load(Ordering::SeqCst), 1);
        assert_eq!(n.role_revokes.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn admin_channel_konstante_ist_python_paritaet() {
        assert_eq!(TOKEN_ERROR_CHANNEL_ID, 1374364800817303632);
    }

    #[test]
    fn sanitize_discord_id_nur_numerisch() {
        assert_eq!(
            sanitize_discord_user_id(Some(" 123 ")).as_deref(),
            Some("123")
        );
        assert_eq!(sanitize_discord_user_id(Some("abc")), None);
        assert_eq!(sanitize_discord_user_id(Some("")), None);
        assert_eq!(sanitize_discord_user_id(None), None);
        assert_eq!(sanitize_discord_user_id(Some("12a3")), None);
    }

    #[test]
    fn user_dm_enthaelt_reauth_link_und_kein_button() {
        let text = user_dm_token_error_text("foo", "https://example.test/streamer/");
        assert!(text.contains("https://example.test/streamer/"));
        assert!(text.contains("Verbindung fehlgeschlagen"));
        // Text-only: kein Button-Marker.
        assert!(!text.to_lowercase().contains("klicke auf den button"));
    }

    #[test]
    fn bot_banned_dm_nennt_kanal_und_recovery_schritte() {
        let text = user_dm_bot_banned_text("foo", "sender_banned");
        // Personalisiert auf den betroffenen Kanal.
        assert!(text.contains("foo"));
        // Beide konkreten Recovery-Befehle mit dem Bot-Account.
        assert!(text.contains("/unban deutschedeadlockcommunity"));
        assert!(text.contains("/mod deutschedeadlockcommunity"));
        // Der technische error_message gehört NICHT in die User-DM.
        assert!(!text.contains("sender_banned"));
        // Platzhalter ist ersetzt.
        assert_ne!(text, "Platzhalter");
    }

    #[test]
    fn reminder_dm_referenziert_grace_dauer() {
        let text = user_dm_reminder_text("foo", DEFAULT_REAUTH_URL);
        assert!(text.contains(&GRACE_PERIOD_DAYS.to_string()));
        assert!(text.contains("Aktivierung weiterhin ausstehend"));
    }

    #[test]
    fn admin_grace_text_mention_mit_und_ohne_discord_id() {
        let (_t, with) = admin_grace_expired_text("foo", "42", Some("999"));
        assert!(with.contains("<@999>"));
        let (_t, without) = admin_grace_expired_text("foo", "42", None);
        assert!(without.contains("`foo`"));
        assert!(!without.contains("<@"));
    }

    #[test]
    fn error_message_wird_auf_200_zeichen_gekuerzt() {
        let long = "x".repeat(500);
        let (_t, desc) = admin_token_error_text("foo", &long);
        // 200 'x' im Codeblock, nicht 500.
        assert!(desc.contains(&"x".repeat(200)));
        assert!(!desc.contains(&"x".repeat(201)));
    }

    #[test]
    fn notify_outcome_any_sent() {
        assert!(NotifyOutcome {
            admin_sent: true,
            ..Default::default()
        }
        .any_sent());
        assert!(NotifyOutcome {
            user_dm_sent: true,
            ..Default::default()
        }
        .any_sent());
        assert!(!NotifyOutcome::default().any_sent());
    }

    // --- DB-Integration (env-gated via TB_TEST_DATABASE_URL) -------------
    //
    // Diese Tests brauchen eine erreichbare Postgres-Test-DB. Ohne
    // `TB_TEST_DATABASE_URL` werden sie übersprungen (keine harte Abhängigkeit
    // im CI ohne DB). Muster: isoliertes Schema pro Test (wie score_store).

    fn test_db_url() -> Option<String> {
        std::env::var("TB_TEST_DATABASE_URL")
            .ok()
            .filter(|v| !v.trim().is_empty())
    }

    async fn setup_db(schema: &str) -> PgPool {
        let url = test_db_url().expect("TB_TEST_DATABASE_URL muss gesetzt sein");
        let admin = PgPool::connect(&url).await.expect("Test-DB-Verbindung");
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .unwrap();
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;

        let schema_owned = schema.to_string();
        let pool = sqlx::postgres::PgPoolOptions::new()
            .after_connect(move |conn, _| {
                let schema = schema_owned.clone();
                Box::pin(async move {
                    sqlx::query(&format!("SET search_path TO {schema}"))
                        .execute(conn)
                        .await?;
                    Ok(())
                })
            })
            .connect(&url)
            .await
            .expect("Schema-Pool");

        for ddl in [
            "CREATE TABLE twitch_token_blacklist (
                twitch_user_id text PRIMARY KEY, twitch_login text NOT NULL,
                error_message text, error_count integer DEFAULT 1,
                first_error_at text NOT NULL, last_error_at text NOT NULL,
                notified integer DEFAULT 0, grace_expires_at text,
                user_dm_sent integer DEFAULT 0, reminder_sent integer DEFAULT 0,
                role_removed integer DEFAULT 0)",
            "CREATE TABLE twitch_streamer_identities (
                twitch_user_id text, twitch_login text, discord_user_id text,
                discord_display_name text, updated_at timestamptz DEFAULT now())",
            "CREATE TABLE twitch_partners (
                id bigserial PRIMARY KEY, twitch_user_id text, twitch_login text,
                status text DEFAULT 'active',
                manual_partner_opt_out integer DEFAULT 0,
                technical_pause_reason text, raid_bot_enabled integer DEFAULT 1,
                departnered_at timestamptz, admin_archived_at timestamptz)",
            "CREATE TABLE twitch_raid_auth (
                twitch_user_id text PRIMARY KEY, twitch_login text,
                raid_enabled boolean DEFAULT true, needs_reauth boolean DEFAULT false,
                access_token_enc bytea, token_expires_at timestamptz,
                reauth_notified_at timestamptz)",
            "CREATE TABLE twitch_raid_blacklist (
                target_id text, target_login text PRIMARY KEY, reason text, added_at text)",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        pool
    }

    async fn seed_blacklist(pool: &PgPool, uid: &str, login: &str, grace_iso: &str, count: i32) {
        sqlx::query(
            "INSERT INTO twitch_token_blacklist
                (twitch_user_id, twitch_login, error_message, error_count,
                 first_error_at, last_error_at, grace_expires_at)
             VALUES ($1, $2, 'invalid_grant', $3, $4, $4, $5)",
        )
        .bind(uid)
        .bind(login)
        .bind(count)
        .bind(Utc::now().to_rfc3339())
        .bind(grace_iso)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn seed_raid_auth(
        pool: &PgPool,
        uid: &str,
        login: &str,
        raid_enabled: bool,
        needs_reauth: bool,
        token_expires_at: DateTime<Utc>,
    ) {
        sqlx::query(
            "INSERT INTO twitch_raid_auth
                (twitch_user_id, twitch_login, raid_enabled, needs_reauth,
                 access_token_enc, token_expires_at)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(uid)
        .bind(login)
        .bind(raid_enabled)
        .bind(needs_reauth)
        .bind(vec![1_u8, 2, 3])
        .bind(token_expires_at)
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn notify_token_error_loest_genau_eine_reaktion_aus() {
        let Some(_) = test_db_url() else {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        };
        let pool = setup_db("tl_notify_once").await;
        let grace = (Utc::now() + chrono::Duration::days(7)).to_rfc3339();
        seed_blacklist(&pool, "100", "foo", &grace, 1).await;
        sqlx::query("INSERT INTO twitch_streamer_identities (twitch_user_id, twitch_login, discord_user_id) VALUES ('100', 'foo', '555')")
            .execute(&pool).await.unwrap();

        let notifier = Arc::new(CountingNotifier::default());
        let reactor = TokenLifecycleReactor::new(pool.clone(), notifier.clone());

        // 1. Aufruf → genau 1 Admin-Embed + 1 User-DM.
        let out = reactor
            .notify_token_error("100", "foo", "invalid_grant")
            .await;
        assert!(out.admin_sent && out.user_dm_sent && !out.already_notified);
        assert_eq!(notifier.admin_embeds.load(Ordering::SeqCst), 1);
        assert_eq!(notifier.user_dms.load(Ordering::SeqCst), 1);

        // Flags gesetzt.
        let (notified, dm_sent): (Option<i32>, Option<i32>) = sqlx::query_as(
            "SELECT notified, user_dm_sent FROM twitch_token_blacklist WHERE twitch_user_id = '100'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(notified, Some(1));
        assert_eq!(dm_sent, Some(1));

        // 2. Aufruf → übersprungen (notified-Flag), KEINE weitere Reaktion.
        let out2 = reactor
            .notify_token_error("100", "foo", "invalid_grant")
            .await;
        assert!(out2.already_notified);
        assert_eq!(notifier.admin_embeds.load(Ordering::SeqCst), 1);
        assert_eq!(notifier.user_dms.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn notify_pending_errors_feuert_ab_erstem_fehler_und_dedupt() {
        let Some(_) = test_db_url() else {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        };
        let pool = setup_db("tl_sweep").await;
        // error_count = 1 (erster Fehler) — Python notifiziert hier bereits.
        let grace = (Utc::now() + chrono::Duration::days(7)).to_rfc3339();
        seed_blacklist(&pool, "400", "qux", &grace, 1).await;
        sqlx::query("INSERT INTO twitch_streamer_identities (twitch_user_id, twitch_login, discord_user_id) VALUES ('400', 'qux', '888')")
            .execute(&pool).await.unwrap();

        let notifier = Arc::new(CountingNotifier::default());
        let reactor = TokenLifecycleReactor::new(pool.clone(), notifier.clone());

        let n1 = reactor.notify_pending_errors().await;
        assert_eq!(n1, 1, "erster Fehler (count=1) wird benachrichtigt");
        assert_eq!(notifier.admin_embeds.load(Ordering::SeqCst), 1);
        assert_eq!(notifier.user_dms.load(Ordering::SeqCst), 1);

        // 2. Sweep: notified=1 → keine Doppelung.
        let n2 = reactor.notify_pending_errors().await;
        assert_eq!(n2, 0);
        assert_eq!(notifier.admin_embeds.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn check_grace_periods_entzieht_rolle_und_setzt_flags() {
        let Some(_) = test_db_url() else {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        };
        let pool = setup_db("tl_grace_expire").await;
        // Abgelaufene Grace (vor 1 Tag), error_count = 3, role_removed = 0.
        // Python laesst Grace erst nach dem Blacklist-Threshold ablaufen.
        let expired = (Utc::now() - chrono::Duration::days(1)).to_rfc3339();
        seed_blacklist(&pool, "200", "bar", &expired, 3).await;
        sqlx::query("INSERT INTO twitch_streamer_identities (twitch_user_id, twitch_login, discord_user_id) VALUES ('200', 'bar', '777')")
            .execute(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO twitch_partners (twitch_user_id, twitch_login) VALUES ('200', 'bar')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_raid_auth (twitch_user_id, twitch_login) VALUES ('200', 'bar')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let notifier = Arc::new(CountingNotifier::default());
        let reactor = TokenLifecycleReactor::new(pool.clone(), notifier.clone());

        let processed = reactor.check_grace_periods().await;
        assert_eq!(processed, 1);
        // Reminder-DM + Admin-Notify + Rollen-Entzug je 1×.
        assert_eq!(notifier.user_dms.load(Ordering::SeqCst), 1);
        assert_eq!(notifier.admin_embeds.load(Ordering::SeqCst), 1);
        assert_eq!(notifier.role_revokes.load(Ordering::SeqCst), 1);

        // role_removed + reminder_sent gesetzt; Grace-Expiry setzt den Partner
        // wie Python auf manuellen Opt-out + token_error_expired.
        let (role_removed, reminder): (Option<i32>, Option<i32>) = sqlx::query_as(
            "SELECT role_removed, reminder_sent FROM twitch_token_blacklist WHERE twitch_user_id = '200'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(role_removed, Some(1));
        assert_eq!(reminder, Some(1));
        let (opt_out, pause, raid_enabled): (Option<i32>, Option<String>, Option<i32>) =
            sqlx::query_as(
                "SELECT manual_partner_opt_out, technical_pause_reason, raid_bot_enabled
             FROM twitch_partners WHERE twitch_user_id = '200'",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(opt_out, Some(1));
        assert_eq!(pause.as_deref(), Some("token_error_expired"));
        assert_eq!(raid_enabled, Some(0));

        // 2. Lauf: role_removed = 1 → Zeile nicht mehr selektiert (keine Doppelung).
        let processed2 = reactor.check_grace_periods().await;
        assert_eq!(processed2, 0);
        assert_eq!(notifier.role_revokes.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn check_grace_periods_ignoriert_unter_threshold() {
        let Some(_) = test_db_url() else {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        };
        let pool = setup_db("tl_grace_threshold").await;
        let expired = (Utc::now() - chrono::Duration::days(1)).to_rfc3339();
        seed_blacklist(&pool, "201", "lowcount", &expired, 1).await;
        sqlx::query(
            "INSERT INTO twitch_partners (twitch_user_id, twitch_login) VALUES ('201', 'lowcount')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_raid_auth (twitch_user_id, twitch_login) VALUES ('201', 'lowcount')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let notifier = Arc::new(CountingNotifier::default());
        let reactor = TokenLifecycleReactor::new(pool.clone(), notifier);

        assert_eq!(reactor.check_grace_periods().await, 0);
        let (opt_out, pause, role_removed): (Option<i32>, Option<String>, Option<i32>) =
            sqlx::query_as(
                "SELECT p.manual_partner_opt_out, p.technical_pause_reason, b.role_removed
                   FROM twitch_partners p
                   JOIN twitch_token_blacklist b ON b.twitch_user_id = p.twitch_user_id
                  WHERE p.twitch_user_id = '201'",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(opt_out, Some(0));
        assert_eq!(pause, None);
        assert_eq!(role_removed, Some(0));
    }

    #[tokio::test]
    async fn restore_bot_banned_nur_bei_gesundem_kanal() {
        let Some(_) = test_db_url() else {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        };
        let pool = setup_db("tl_restore").await;
        sqlx::query("INSERT INTO twitch_partners (twitch_user_id, twitch_login, technical_pause_reason, raid_bot_enabled) VALUES ('300', 'baz', 'bot_banned', 0)")
            .execute(&pool).await.unwrap();
        // needs_reauth = TRUE → noch nicht gesund.
        sqlx::query("INSERT INTO twitch_raid_auth (twitch_user_id, twitch_login, raid_enabled, needs_reauth) VALUES ('300', 'baz', false, true)")
            .execute(&pool).await.unwrap();

        let notifier = Arc::new(CountingNotifier::default());
        let reactor = TokenLifecycleReactor::new(pool.clone(), notifier)
            .with_bot_ban_status_probe(Arc::new(FixedBotBanStatus(BotBanStatus::NotBanned)));

        // Kanal noch nicht gesund → kein Restore.
        assert!(!reactor.restore_bot_banned_channel("300", "baz").await);

        // Health-Restore simulieren.
        sqlx::query(
            "UPDATE twitch_raid_auth SET needs_reauth = false WHERE twitch_user_id = '300'",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert!(reactor.restore_bot_banned_channel("300", "baz").await);

        let reason: Option<String> = sqlx::query_scalar(
            "SELECT technical_pause_reason FROM twitch_partners WHERE twitch_user_id = '300'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(reason, None);
        let raid: Option<bool> = sqlx::query_scalar(
            "SELECT raid_enabled FROM twitch_raid_auth WHERE twitch_user_id = '300'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(raid, Some(true));
    }

    /// Regression: `disconnect-bot` hinterlässt genau die Signatur, die der
    /// Legacy-Opt-out-Zweig als Bot-Ban gelesen hat (opt_out=1, keine
    /// technische Pause, raid_enabled=false). Der Sweep hat den getrennten
    /// Kanal deshalb Stunden später halb reaktiviert: `manual_partner_opt_out`
    /// zurück auf 0, `raid_enabled` auf true, und über den Ban-Probe den Bot
    /// wieder als Moderator gesetzt — während `status` auf `departnered` stand.
    #[tokio::test]
    async fn restore_bot_banned_fasst_getrennte_kanaele_nicht_an() {
        let Some(_) = test_db_url() else {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        };
        let pool = setup_db("tl_restore_departnered").await;
        sqlx::query(
            "INSERT INTO twitch_partners
                (twitch_user_id, twitch_login, status, manual_partner_opt_out,
                 raid_bot_enabled, departnered_at)
             VALUES ('310', 'getrennt', 'departnered', 1, 0, now()),
                    ('311', 'archiviert', 'archived', 1, 0, NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "UPDATE twitch_partners SET admin_archived_at = now() WHERE twitch_user_id = '311'",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_raid_auth (twitch_user_id, twitch_login, raid_enabled, needs_reauth)
             VALUES ('310', 'getrennt', false, false), ('311', 'archiviert', false, false)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let probe = Arc::new(CountingBotBanStatus::default());
        let reactor =
            TokenLifecycleReactor::new(pool.clone(), Arc::new(CountingNotifier::default()))
                .with_bot_ban_status_probe(probe.clone());

        // Kandidaten-Query lässt beide draußen.
        assert_eq!(
            reactor.restore_ready_bot_banned_channels().await,
            0,
            "getrennte und archivierte Kanäle sind keine Bot-Ban-Kandidaten"
        );
        // Direkter Aufruf ebenfalls, und ohne den Bot zu proben (= zu remodden).
        assert!(!reactor.restore_bot_banned_channel("310", "getrennt").await);
        assert!(
            !reactor
                .restore_bot_banned_channel("311", "archiviert")
                .await
        );
        assert_eq!(
            probe.0.load(Ordering::SeqCst),
            0,
            "kein Ban-Probe im getrennten Kanal — der würde den Bot wieder modden"
        );

        // DB-Zustand unverändert: Opt-out bleibt, Raid bleibt aus.
        let rows: Vec<(String, Option<i32>, Option<bool>)> = sqlx::query_as(
            "SELECT p.twitch_user_id, p.manual_partner_opt_out, a.raid_enabled
               FROM twitch_partners p
               JOIN twitch_raid_auth a ON a.twitch_user_id = p.twitch_user_id
              ORDER BY p.twitch_user_id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            rows,
            vec![
                ("310".to_string(), Some(1), Some(false)),
                ("311".to_string(), Some(1), Some(false)),
            ]
        );
    }

    #[tokio::test]
    async fn restore_bot_banned_bleibt_ohne_echten_ban_status_fail_closed() {
        let Some(_) = test_db_url() else {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        };
        let pool = setup_db("tl_restore_fail_closed").await;
        sqlx::query("INSERT INTO twitch_partners (twitch_user_id, twitch_login, technical_pause_reason, raid_bot_enabled) VALUES ('305', 'stillbanned', 'bot_banned', 0)")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_raid_auth (twitch_user_id, twitch_login, raid_enabled, needs_reauth) VALUES ('305', 'stillbanned', false, false)")
            .execute(&pool).await.unwrap();

        let reactor =
            TokenLifecycleReactor::new(pool.clone(), Arc::new(CountingNotifier::default()));

        assert!(
            !reactor
                .restore_bot_banned_channel("305", "stillbanned")
                .await,
            "ein gesunder OAuth-Token beweist nicht, dass der Chat-Ban aufgehoben ist"
        );
        let reason: Option<String> = sqlx::query_scalar(
            "SELECT technical_pause_reason FROM twitch_partners WHERE twitch_user_id = '305'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(reason.as_deref(), Some("bot_banned"));
    }

    #[tokio::test]
    async fn restore_bot_banned_bleibt_bei_ban_oder_unklarem_status_pausiert() {
        let Some(_) = test_db_url() else {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        };
        let pool = setup_db("tl_restore_status").await;
        sqlx::query("INSERT INTO twitch_partners (twitch_user_id, twitch_login, technical_pause_reason, raid_bot_enabled) VALUES ('306', 'banned', 'bot_banned', 0), ('307', 'unknown', 'bot_banned', 0)")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_raid_auth (twitch_user_id, twitch_login, raid_enabled, needs_reauth) VALUES ('306', 'banned', false, false), ('307', 'unknown', false, false)")
            .execute(&pool).await.unwrap();

        let banned =
            TokenLifecycleReactor::new(pool.clone(), Arc::new(CountingNotifier::default()))
                .with_bot_ban_status_probe(Arc::new(FixedBotBanStatus(BotBanStatus::Banned)));
        let unknown =
            TokenLifecycleReactor::new(pool.clone(), Arc::new(CountingNotifier::default()))
                .with_bot_ban_status_probe(Arc::new(FixedBotBanStatus(BotBanStatus::Unknown)));

        assert!(!banned.restore_bot_banned_channel("306", "banned").await);
        assert!(!unknown.restore_bot_banned_channel("307", "unknown").await);
        let active_pauses: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM twitch_partners WHERE technical_pause_reason = 'bot_banned'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(active_pauses, 2);
    }

    #[tokio::test]
    async fn handle_bot_banned_channel_markiert_optout_und_dedupt_dm() {
        let Some(_) = test_db_url() else {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        };
        let pool = setup_db("tl_bot_banned").await;
        sqlx::query("INSERT INTO twitch_partners (twitch_user_id, twitch_login, technical_pause_reason, raid_bot_enabled) VALUES ('500', 'banme', NULL, 1)")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_raid_auth (twitch_user_id, twitch_login, raid_enabled, needs_reauth) VALUES ('500', 'banme', true, false)")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_streamer_identities (twitch_user_id, twitch_login, discord_user_id) VALUES ('500', 'banme', '999')")
            .execute(&pool).await.unwrap();

        let notifier = Arc::new(CountingNotifier::default());
        let reactor = TokenLifecycleReactor::new(pool.clone(), notifier.clone());

        let outcome = reactor
            .handle_bot_banned_channel("500", "banme", "sender_banned")
            .await;
        assert!(outcome.opt_out_marked);
        assert!(outcome.user_dm_sent);
        assert!(!outcome.already_flagged);
        assert_eq!(notifier.user_dms.load(Ordering::SeqCst), 1);

        let (raid_enabled, needs_reauth): (Option<bool>, Option<bool>) = sqlx::query_as(
            "SELECT raid_enabled, needs_reauth FROM twitch_raid_auth WHERE twitch_user_id = '500'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(raid_enabled, Some(false));
        assert_eq!(needs_reauth, Some(false));
        let (pause, partner_enabled): (Option<String>, Option<i32>) = sqlx::query_as(
            "SELECT technical_pause_reason, raid_bot_enabled FROM twitch_partners WHERE twitch_user_id = '500'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(pause.as_deref(), Some("bot_banned"));
        assert_eq!(partner_enabled, Some(0));
        let reason: Option<String> = sqlx::query_scalar(
            "SELECT reason FROM twitch_raid_blacklist WHERE target_login = 'banme'",
        )
        .fetch_optional(&pool)
        .await
        .unwrap();
        assert!(
            reason.as_deref().unwrap_or_default().contains("bot_banned"),
            "Blacklist-Reason muss Dedup-Marker tragen"
        );

        let duplicate = reactor
            .handle_bot_banned_channel("500", "banme", "sender_banned again")
            .await;
        assert!(duplicate.already_flagged);
        assert!(!duplicate.user_dm_sent);
        assert_eq!(notifier.user_dms.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn restore_sweep_hebt_technische_pausen_auf() {
        let Some(_) = test_db_url() else {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        };
        let pool = setup_db("tl_restore_sweep").await;
        sqlx::query("INSERT INTO twitch_partners (twitch_user_id, twitch_login, manual_partner_opt_out, technical_pause_reason, raid_bot_enabled)
            VALUES ('600', 'ready', 0, 'bot_banned', 0),
                   ('601', 'blocked', 0, 'blocked', 0),
                   ('602', 'tokenready', 0, 'token_error_retry', 0),
                   ('603', 'legacyban', 1, NULL, 0),
                   ('604', 'renamedban', 0, NULL, 0)")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_raid_auth (twitch_user_id, twitch_login, raid_enabled, needs_reauth)
            VALUES ('600', 'ready', false, false),
                   ('601', 'blocked', false, false),
                   ('602', 'tokenready', false, false),
                   ('603', 'legacyban', false, false),
                   ('604', 'renamedban', false, false)")
            .execute(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO twitch_raid_blacklist (target_id, target_login, reason, added_at)
             VALUES ('604', 'stale-renamedban', 'chat_bot_banned_in_channel', $1)",
        )
        .bind(Utc::now().to_rfc3339())
        .execute(&pool)
        .await
        .unwrap();

        let notifier = Arc::new(CountingNotifier::default());
        let reactor = TokenLifecycleReactor::new(pool.clone(), notifier)
            .with_bot_ban_status_probe(Arc::new(FixedBotBanStatus(BotBanStatus::NotBanned)));
        assert_eq!(reactor.restore_ready_bot_banned_channels().await, 3);

        let ready_reason: Option<String> = sqlx::query_scalar(
            "SELECT technical_pause_reason FROM twitch_partners WHERE twitch_user_id = '600'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let blocked_reason: Option<String> = sqlx::query_scalar(
            "SELECT technical_pause_reason FROM twitch_partners WHERE twitch_user_id = '601'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let (token_reason, token_raid): (Option<String>, Option<i32>) = sqlx::query_as(
            "SELECT technical_pause_reason, raid_bot_enabled
             FROM twitch_partners WHERE twitch_user_id = '602'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let (legacy_opt_out, legacy_reason, legacy_raid): (
            Option<i32>,
            Option<String>,
            Option<i32>,
        ) = sqlx::query_as(
            "SELECT manual_partner_opt_out, technical_pause_reason, raid_bot_enabled
             FROM twitch_partners WHERE twitch_user_id = '603'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let (renamed_reason, renamed_raid): (Option<String>, Option<i32>) = sqlx::query_as(
            "SELECT technical_pause_reason, raid_bot_enabled
             FROM twitch_partners WHERE twitch_user_id = '604'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let renamed_marker_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM twitch_raid_blacklist WHERE target_id = '604'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(ready_reason, None);
        assert_eq!(blocked_reason.as_deref(), Some("blocked"));
        assert_eq!(token_reason.as_deref(), Some("token_error_retry"));
        assert_eq!(token_raid, Some(0));
        assert_eq!(legacy_opt_out, Some(0));
        assert_eq!(legacy_reason, None);
        assert_eq!(legacy_raid, Some(1));
        assert_eq!(renamed_reason, None);
        assert_eq!(renamed_raid, Some(1));
        assert_eq!(renamed_marker_count, 0);
    }

    #[tokio::test]
    async fn token_error_reactivation_heilt_nur_mit_validem_auth_und_ohne_bot_ban() {
        let Some(_) = test_db_url() else {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        };
        let pool = setup_db("tl_token_error_reactivate").await;
        sqlx::query(
            "INSERT INTO twitch_partners
                (twitch_user_id, twitch_login, manual_partner_opt_out,
                 technical_pause_reason, raid_bot_enabled)
             VALUES
                ('800', 'retry', 0, 'token_error_retry', 0),
                ('801', 'expired', 1, 'token_error_expired', 0),
                ('802', 'banmarker', 0, 'token_error_retry', 0),
                ('803', 'hardban', 0, 'bot_banned', 0),
                ('804', 'expiredtoken', 0, 'token_error_retry', 0),
                ('805', 'reauth', 0, 'token_error_retry', 0),
                ('806', 'sharedlogin', 0, 'token_error_retry', 0)",
        )
        .execute(&pool)
        .await
        .unwrap();
        let valid_until = Utc::now() + chrono::Duration::hours(1);
        let expired_at = Utc::now() - chrono::Duration::hours(1);
        seed_raid_auth(&pool, "800", "retry", false, false, valid_until).await;
        seed_raid_auth(&pool, "801", "expired", false, false, valid_until).await;
        seed_raid_auth(&pool, "802", "banmarker", false, false, valid_until).await;
        seed_raid_auth(&pool, "803", "hardban", false, false, valid_until).await;
        seed_raid_auth(&pool, "804", "expiredtoken", false, false, expired_at).await;
        seed_raid_auth(&pool, "805", "reauth", false, true, valid_until).await;
        seed_raid_auth(&pool, "900", "sharedlogin", false, false, valid_until).await;
        sqlx::query(
            "INSERT INTO twitch_raid_blacklist (target_id, target_login, reason, added_at)
             VALUES ('802', 'stale-banmarker', 'chat_bot_banned_in_channel: sender_banned', $1)",
        )
        .bind(Utc::now().to_rfc3339())
        .execute(&pool)
        .await
        .unwrap();
        let future_grace = (Utc::now() + chrono::Duration::days(1)).to_rfc3339();
        seed_blacklist(&pool, "800", "retry", &future_grace, 3).await;
        seed_blacklist(&pool, "801", "expired", &future_grace, 3).await;

        let notifier = Arc::new(CountingNotifier::default());
        let reactor = TokenLifecycleReactor::new(pool.clone(), notifier);

        assert_eq!(
            reactor
                .reactivate_token_error_partners_with_valid_auth()
                .await,
            2
        );

        let healed: Vec<PartnerStateRow> = sqlx::query_as(
            "SELECT twitch_user_id, manual_partner_opt_out, technical_pause_reason, raid_bot_enabled
             FROM twitch_partners
             WHERE twitch_user_id IN ('800', '801', '802', '803', '804', '805', '806')
             ORDER BY twitch_user_id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            healed,
            vec![
                ("800".to_string(), Some(0), None, Some(1)),
                ("801".to_string(), Some(0), None, Some(1)),
                (
                    "802".to_string(),
                    Some(0),
                    Some("token_error_retry".to_string()),
                    Some(0),
                ),
                (
                    "803".to_string(),
                    Some(0),
                    Some("bot_banned".to_string()),
                    Some(0),
                ),
                (
                    "804".to_string(),
                    Some(0),
                    Some("token_error_retry".to_string()),
                    Some(0),
                ),
                (
                    "805".to_string(),
                    Some(0),
                    Some("token_error_retry".to_string()),
                    Some(0),
                ),
                (
                    "806".to_string(),
                    Some(0),
                    Some("token_error_retry".to_string()),
                    Some(0),
                ),
            ]
        );
        let auth_enabled: Vec<(String, Option<bool>)> = sqlx::query_as(
            "SELECT twitch_user_id, raid_enabled
             FROM twitch_raid_auth
             WHERE twitch_user_id IN ('800', '801', '802', '803', '804', '805', '900')
             ORDER BY twitch_user_id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            auth_enabled,
            vec![
                ("800".to_string(), Some(true)),
                ("801".to_string(), Some(true)),
                ("802".to_string(), Some(false)),
                ("803".to_string(), Some(false)),
                ("804".to_string(), Some(false)),
                ("805".to_string(), Some(false)),
                ("900".to_string(), Some(false)),
            ]
        );
        let remaining_blacklist: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM twitch_token_blacklist WHERE twitch_user_id IN ('800', '801')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(remaining_blacklist, 0);
    }

    #[tokio::test]
    async fn grace_expiry_ueberschreibt_harte_pausen_nicht() {
        let Some(_) = test_db_url() else {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        };
        let pool = setup_db("tl_grace_hard_pause").await;
        let expired = (Utc::now() - chrono::Duration::days(1)).to_rfc3339();
        seed_blacklist(&pool, "210", "hardblocked", &expired, 3).await;
        seed_blacklist(&pool, "211", "hardbanned", &expired, 3).await;
        sqlx::query(
            "INSERT INTO twitch_partners
                (twitch_user_id, twitch_login, manual_partner_opt_out,
                 technical_pause_reason, raid_bot_enabled)
             VALUES
                ('210', 'hardblocked', 0, 'blocked', 0),
                ('211', 'hardbanned', 0, 'bot_banned', 0)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_raid_auth (twitch_user_id, twitch_login)
             VALUES ('210', 'hardblocked'), ('211', 'hardbanned')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let notifier = Arc::new(CountingNotifier::default());
        let reactor = TokenLifecycleReactor::new(pool.clone(), notifier);

        assert_eq!(reactor.check_grace_periods().await, 2);

        let partners: Vec<PartnerStateRow> = sqlx::query_as(
            "SELECT twitch_user_id, manual_partner_opt_out, technical_pause_reason, raid_bot_enabled
             FROM twitch_partners
             ORDER BY twitch_user_id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            partners,
            vec![
                (
                    "210".to_string(),
                    Some(0),
                    Some("blocked".to_string()),
                    Some(0),
                ),
                (
                    "211".to_string(),
                    Some(0),
                    Some("bot_banned".to_string()),
                    Some(0),
                ),
            ]
        );
        let removed: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM twitch_token_blacklist WHERE role_removed = 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(removed, 2);
    }

    #[tokio::test]
    async fn reconcile_healthy_raid_toggles_heilt_nur_aktive_partner_ohne_pause() {
        let Some(_) = test_db_url() else {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        };
        let pool = setup_db("tl_reconcile_healthy_raid_toggles").await;
        sqlx::query(
            "INSERT INTO twitch_partners
                (twitch_user_id, twitch_login, status, raid_bot_enabled,
                 manual_partner_opt_out, technical_pause_reason)
             VALUES
                ('700', 'healme', 'active', 0, 0, NULL),
                ('701', 'tokenpause', 'active', 0, 0, 'token_error'),
                ('702', 'blocked', 'active', 0, 0, 'blocked'),
                ('703', 'manualout', 'active', 0, 1, NULL),
                ('704', 'authoptout', 'active', 0, 0, NULL),
                ('705', 'reauth', 'active', 0, 0, NULL),
                ('706', 'archived', 'archived', 0, 0, NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_raid_auth
                (twitch_user_id, twitch_login, raid_enabled, needs_reauth)
             VALUES
                ('700', 'healme', true, false),
                ('701', 'tokenpause', true, false),
                ('702', 'blocked', true, false),
                ('703', 'manualout', true, false),
                ('704', 'authoptout', false, false),
                ('705', 'reauth', true, true),
                ('706', 'archived', true, false)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let notifier = Arc::new(CountingNotifier::default());
        let reactor = TokenLifecycleReactor::new(pool.clone(), notifier);

        assert_eq!(reactor.reconcile_healthy_raid_toggles().await, 1);

        let toggles: Vec<(String, Option<i32>)> = sqlx::query_as(
            "SELECT twitch_user_id, raid_bot_enabled
             FROM twitch_partners
             ORDER BY twitch_user_id",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            toggles,
            vec![
                ("700".to_string(), Some(1)),
                ("701".to_string(), Some(0)),
                ("702".to_string(), Some(0)),
                ("703".to_string(), Some(0)),
                ("704".to_string(), Some(0)),
                ("705".to_string(), Some(0)),
                ("706".to_string(), Some(0)),
            ]
        );

        assert_eq!(reactor.reconcile_healthy_raid_toggles().await, 0);
    }

    #[tokio::test]
    async fn cleanup_loescht_nur_alte_eintraege() {
        let Some(_) = test_db_url() else {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        };
        let pool = setup_db("tl_cleanup").await;
        let old = (Utc::now() - chrono::Duration::days(40)).to_rfc3339();
        let recent = Utc::now().to_rfc3339();
        sqlx::query("INSERT INTO twitch_token_blacklist (twitch_user_id, twitch_login, first_error_at, last_error_at) VALUES ('old', 'o', $1, $1)")
            .bind(&old).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_token_blacklist (twitch_user_id, twitch_login, first_error_at, last_error_at) VALUES ('new', 'n', $1, $1)")
            .bind(&recent).execute(&pool).await.unwrap();

        let notifier = Arc::new(CountingNotifier::default());
        let reactor = TokenLifecycleReactor::new(pool.clone(), notifier);
        let deleted = reactor.cleanup_old_entries(30).await;
        assert_eq!(deleted, 1);
        let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM twitch_token_blacklist")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(remaining, 1);
    }
}
