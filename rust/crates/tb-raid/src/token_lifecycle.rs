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

/// Token-Lifecycle-Reaktor: bindet `twitch_token_blacklist` an den Discord-Port.
pub struct TokenLifecycleReactor<N: TokenLifecycleNotifier> {
    pool: PgPool,
    notifier: N,
    reauth_url: String,
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
        }
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
    /// `invalid_grant` (direkt nach `add_to_blacklist`), nicht erst ab
    /// `error_count >= 3`. Der Blacklist-Eintrag existiert ab dem ersten Fehler
    /// (`add_to_blacklist_inner` INSERTet ihn), darum genügt hier „Eintrag
    /// existiert UND notified=0".
    pub async fn notify_pending_errors(&self) -> u64 {
        let pending: Result<Vec<(String, String, Option<String>)>, _> = sqlx::query_as(
            r#"
            SELECT twitch_user_id, twitch_login, error_message
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
        for (uid, login, err) in rows {
            let outcome = self
                .notify_token_error(&uid, &login, err.as_deref().unwrap_or("invalid refresh grant"))
                .await;
            if outcome.any_sent() {
                notified += 1;
            }
        }
        notified
    }

    /// Stündlicher Grace-Sweep (Python `check_grace_periods`): für jede Zeile mit
    /// abgelaufener Grace-Period (`error_count >= 3`, `grace_expires_at <= now`,
    /// `role_removed = 0`) sendet er einmalig Reminder-DM + Admin-Notify
    /// (reminder_sent), entzieht die Streamer-Rolle und setzt
    /// `manual_opt_out`/`role_removed`. Liefert die Anzahl bearbeiteter Streamer.
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

            // 3. DB-State: manual_opt_out + role_removed (Python-Block, idempotent).
            if let Err(error) = self
                .mark_grace_expired(&row.twitch_user_id, &row.twitch_login)
                .await
            {
                tracing::warn!(%error, user = %mask(&row.twitch_user_id), "Grace-Expiry-State nicht setzbar");
            } else {
                processed += 1;
                tracing::info!(
                    user = %mask(&row.twitch_user_id),
                    "Grace-Period abgelaufen – Rolle entzogen, manual_opt_out gesetzt"
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
        match sqlx::query("DELETE FROM twitch_token_blacklist WHERE last_error_at < $1")
            .bind(&cutoff_iso)
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

    /// Restore nach Health-Restore (Python `restore_bot_banned_channel`, Kern):
    /// hebt den technischen Bot-Ban-Opt-out wieder auf, sobald der Kanal wieder
    /// gesund ist (`needs_reauth = FALSE`). Liefert `true`, wenn etwas restauriert
    /// wurde. Bewusst auf den technischen Bot-Ban-Pfad fokussiert; manueller
    /// Opt-out bleibt unangetastet.
    pub async fn restore_bot_banned_channel(
        &self,
        twitch_user_id: &str,
        twitch_login: &str,
    ) -> bool {
        match self
            .restore_bot_banned_inner(twitch_user_id, twitch_login)
            .await
        {
            Ok(restored) => {
                if restored {
                    tracing::info!(
                        user = %mask(twitch_login),
                        "Technischer Bot-Ban-Opt-out wiederhergestellt"
                    );
                }
                restored
            }
            Err(error) => {
                tracing::warn!(%error, user = %mask(twitch_user_id), "Bot-Ban-Restore fehlgeschlagen");
                false
            }
        }
    }

    // -- DB-Helfer --------------------------------------------------------

    async fn is_notified(&self, twitch_user_id: &str) -> Result<bool, sqlx::Error> {
        let row: Option<Option<i32>> = sqlx::query_scalar(
            "SELECT notified FROM twitch_token_blacklist WHERE twitch_user_id = $1",
        )
        .bind(twitch_user_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(matches!(row, Some(Some(n)) if n == 1))
    }

    async fn set_notified(&self, twitch_user_id: &str) {
        if let Err(error) =
            sqlx::query("UPDATE twitch_token_blacklist SET notified = 1 WHERE twitch_user_id = $1")
                .bind(twitch_user_id)
                .execute(&self.pool)
                .await
        {
            tracing::warn!(%error, user = %mask(twitch_user_id), "notified-Flag nicht setzbar");
        }
    }

    async fn set_user_dm_sent(&self, twitch_user_id: &str) {
        if let Err(error) = sqlx::query(
            "UPDATE twitch_token_blacklist SET user_dm_sent = 1 WHERE twitch_user_id = $1",
        )
        .bind(twitch_user_id)
        .execute(&self.pool)
        .await
        {
            tracing::debug!(%error, user = %mask(twitch_user_id), "user_dm_sent-Flag nicht setzbar");
        }
    }

    async fn set_reminder_sent(&self, twitch_user_id: &str) {
        if let Err(error) = sqlx::query(
            "UPDATE twitch_token_blacklist SET reminder_sent = 1 WHERE twitch_user_id = $1",
        )
        .bind(twitch_user_id)
        .execute(&self.pool)
        .await
        {
            tracing::warn!(%error, user = %mask(twitch_user_id), "reminder_sent-Flag nicht setzbar");
        }
    }

    async fn load_expired_grace(&self, now_iso: &str) -> Result<Vec<ExpiredGraceRow>, sqlx::Error> {
        sqlx::query_as::<_, ExpiredGraceRow>(
            r#"
            SELECT twitch_user_id, twitch_login, reminder_sent
            FROM twitch_token_blacklist
            WHERE error_count >= $1
              AND grace_expires_at IS NOT NULL
              AND grace_expires_at <= $2
              AND role_removed = 0
            "#,
        )
        .bind(BLACKLIST_DISABLE_THRESHOLD)
        .bind(now_iso)
        .fetch_all(&self.pool)
        .await
    }

    /// Python-Grace-Block: Partner als manuellen Opt-out markieren, Raid-Auth
    /// invalidieren und `role_removed=1` setzen. In einer Transaktion (idempotent).
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
                OR LOWER(twitch_login) = LOWER($2)
            "#,
        )
        .bind(twitch_user_id)
        .bind(twitch_login)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            r#"
            UPDATE twitch_raid_auth
               SET raid_enabled = FALSE,
                   needs_reauth = TRUE,
                   twitch_login = COALESCE(NULLIF($1, ''), twitch_login)
             WHERE twitch_user_id = $2
                OR LOWER(twitch_login) = LOWER($1)
            "#,
        )
        .bind(twitch_login)
        .bind(twitch_user_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query("UPDATE twitch_token_blacklist SET role_removed = 1 WHERE twitch_user_id = $1")
            .bind(twitch_user_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Kern von `restore_bot_banned_channel`: nur restaurieren, wenn die Auth-Zeile
    /// existiert UND `needs_reauth = FALSE` (Kanal wieder gesund). Hebt
    /// `technical_pause_reason='bot_banned'` auf und re-aktiviert Raid, sofern kein
    /// manueller Opt-out vorliegt.
    async fn restore_bot_banned_inner(
        &self,
        twitch_user_id: &str,
        twitch_login: &str,
    ) -> Result<bool, sqlx::Error> {
        let login_hint = twitch_login.trim().to_lowercase();
        let mut tx = self.pool.begin().await?;

        let auth: Option<(Option<bool>, Option<bool>)> = sqlx::query_as(
            "SELECT raid_enabled, needs_reauth FROM twitch_raid_auth WHERE twitch_user_id = $1 LIMIT 1",
        )
        .bind(twitch_user_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some((_raid_enabled, needs_reauth)) = auth else {
            tx.commit().await?;
            return Ok(false);
        };
        // Kanal noch nicht gesund → nicht restaurieren.
        if needs_reauth.unwrap_or(true) {
            tx.commit().await?;
            return Ok(false);
        }

        // Liegt überhaupt ein technischer Bot-Ban vor? (Partner-Pause-Reason.)
        let partner: Option<(Option<bool>, Option<String>)> = sqlx::query_as(
            r#"
            SELECT manual_partner_opt_out, technical_pause_reason
            FROM twitch_partners
            WHERE twitch_user_id = $1
               OR LOWER(twitch_login) = LOWER($2)
            LIMIT 1
            "#,
        )
        .bind(twitch_user_id)
        .bind(&login_hint)
        .fetch_optional(&mut *tx)
        .await?;
        let (manual_opt_out, pause_reason) = match partner {
            Some((m, r)) => (m.unwrap_or(false), r.unwrap_or_default().trim().to_lowercase()),
            None => (false, String::new()),
        };
        if pause_reason != "bot_banned" {
            tx.commit().await?;
            return Ok(false);
        }

        // Restore: Pause-Reason löschen; Raid nur re-aktivieren, wenn kein manueller
        // Opt-out vorliegt (Python-Parität).
        let reenable = !manual_opt_out;
        sqlx::query(
            r#"
            DELETE FROM twitch_raid_blacklist
            WHERE LOWER(target_login) = LOWER($1)
              AND LOWER(COALESCE(reason, '')) LIKE '%bot_banned%'
            "#,
        )
        .bind(&login_hint)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            r#"
            UPDATE twitch_raid_auth
               SET raid_enabled = $1,
                   twitch_login = COALESCE(NULLIF($2, ''), twitch_login)
             WHERE twitch_user_id = $3
            "#,
        )
        .bind(reenable)
        .bind(&login_hint)
        .bind(twitch_user_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            r#"
            UPDATE twitch_partners
               SET technical_pause_reason = NULL,
                   raid_bot_enabled = CASE WHEN $1 THEN 1 ELSE raid_bot_enabled END
             WHERE twitch_user_id = $2
                OR LOWER(twitch_login) = LOWER($3)
            "#,
        )
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
    async fn discord_user_id_for(&self, twitch_user_id: &str, twitch_login: &str) -> Option<String> {
        let login = tb_domain::normalize_twitch_login(twitch_login).unwrap_or_default();
        let row: Result<Option<String>, _> = sqlx::query_scalar(
            r#"
            SELECT discord_user_id
            FROM twitch_streamer_identities
            WHERE ($1 <> '' AND twitch_user_id = $1)
               OR ($2 <> '' AND LOWER(twitch_login) = $2)
            ORDER BY updated_at DESC
            LIMIT 1
            "#,
        )
        .bind(twitch_user_id.trim())
        .bind(&login)
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
        assert_eq!(sanitize_discord_user_id(Some(" 123 ")).as_deref(), Some("123"));
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
        assert!(NotifyOutcome { admin_sent: true, ..Default::default() }.any_sent());
        assert!(NotifyOutcome { user_dm_sent: true, ..Default::default() }.any_sent());
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
                manual_partner_opt_out integer DEFAULT 0,
                technical_pause_reason text, raid_bot_enabled integer DEFAULT 1)",
            "CREATE TABLE twitch_raid_auth (
                twitch_user_id text PRIMARY KEY, twitch_login text,
                raid_enabled boolean DEFAULT true, needs_reauth boolean DEFAULT false)",
            "CREATE TABLE twitch_raid_blacklist (target_login text, reason text)",
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
        let out = reactor.notify_token_error("100", "foo", "invalid_grant").await;
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
        let out2 = reactor.notify_token_error("100", "foo", "invalid_grant").await;
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
        // Abgelaufene Grace (vor 1 Tag), error_count >= 3, role_removed = 0.
        let expired = (Utc::now() - chrono::Duration::days(1)).to_rfc3339();
        seed_blacklist(&pool, "200", "bar", &expired, 3).await;
        sqlx::query("INSERT INTO twitch_streamer_identities (twitch_user_id, twitch_login, discord_user_id) VALUES ('200', 'bar', '777')")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_partners (twitch_user_id, twitch_login) VALUES ('200', 'bar')")
            .execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO twitch_raid_auth (twitch_user_id, twitch_login) VALUES ('200', 'bar')")
            .execute(&pool).await.unwrap();

        let notifier = Arc::new(CountingNotifier::default());
        let reactor = TokenLifecycleReactor::new(pool.clone(), notifier.clone());

        let processed = reactor.check_grace_periods().await;
        assert_eq!(processed, 1);
        // Reminder-DM + Admin-Notify + Rollen-Entzug je 1×.
        assert_eq!(notifier.user_dms.load(Ordering::SeqCst), 1);
        assert_eq!(notifier.admin_embeds.load(Ordering::SeqCst), 1);
        assert_eq!(notifier.role_revokes.load(Ordering::SeqCst), 1);

        // role_removed + reminder_sent gesetzt; manual_opt_out im Partner.
        let (role_removed, reminder): (Option<i32>, Option<i32>) = sqlx::query_as(
            "SELECT role_removed, reminder_sent FROM twitch_token_blacklist WHERE twitch_user_id = '200'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(role_removed, Some(1));
        assert_eq!(reminder, Some(1));
        let opt_out: Option<i32> = sqlx::query_scalar(
            "SELECT manual_partner_opt_out FROM twitch_partners WHERE twitch_user_id = '200'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(opt_out, Some(1));

        // 2. Lauf: role_removed = 1 → Zeile nicht mehr selektiert (keine Doppelung).
        let processed2 = reactor.check_grace_periods().await;
        assert_eq!(processed2, 0);
        assert_eq!(notifier.role_revokes.load(Ordering::SeqCst), 1);
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
        let reactor = TokenLifecycleReactor::new(pool.clone(), notifier);

        // Kanal noch nicht gesund → kein Restore.
        assert!(!reactor.restore_bot_banned_channel("300", "baz").await);

        // Health-Restore simulieren.
        sqlx::query("UPDATE twitch_raid_auth SET needs_reauth = false WHERE twitch_user_id = '300'")
            .execute(&pool).await.unwrap();
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
