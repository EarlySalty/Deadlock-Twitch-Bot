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

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use sqlx::PgPool;

use crate::token_blacklist::{BLACKLIST_DISABLE_THRESHOLD, GRACE_PERIOD_DAYS};
use crate::util::mask_log_identifier as mask;

/// Admin-Channel für Token-Fehler-Benachrichtigungen (Python
/// `TOKEN_ERROR_CHANNEL_ID`). Konstante 1:1 übernommen.
pub const TOKEN_ERROR_CHANNEL_ID: i64 = 1374364800817303632;

/// Standard-Re-Auth-Ziel: das Verwaltungs-Dashboard. Dort ist "Twitch-Verbindung"
/// der erste Punkt, über den der Streamer den Bot neu autorisiert — damit gilt das
/// Dashboard sofort wieder als vertraut. Per Env `STREAMER_REAUTH_URL`
/// überschreibbar (kein Domain-Raten im Code).
pub const DEFAULT_REAUTH_URL: &str = "https://deutsche-deadlock-community.de/twitch/verwaltung";

/// Obergrenze an Kanälen pro aktivem Ban-Sweep. Reine Sicherheitsleine gegen
/// einen Helix-Ansturm, wenn die Partner-Zahl unerwartet wächst.
const BAN_PROBE_MAX_PER_SWEEP: i64 = 400;

/// Pause zwischen zwei Ban-Proben. Hält den Sweep weit unter dem Helix-Budget,
/// ohne dass ein Lauf spürbar länger dauert.
const BAN_PROBE_DELAY: std::time::Duration = std::time::Duration::from_millis(120);

/// So frisch muss die letzte Chat-Zeile sein, damit sie als Beweis zählt, dass
/// der Bot im Kanal weiter mitliest. Großzügig gewählt: viele Partner streamen
/// nicht täglich, und ein stiller Kanal ist kein Bann.
const CHAT_FRISCH_FENSTER: Duration = Duration::days(7);

/// Was die aktive Prüfung über einen Kanal weiß.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProbeZustand {
    /// Einsetzung abgelehnt, aber der Chat läuft weiter: keine Mod-Rechte,
    /// kein Bann.
    ModRechteWeg,
    /// Einsetzung abgelehnt, und aus dem Kanal kam länger kein Chat an.
    ModRechteWegStill,
}

impl ProbeZustand {
    fn als_text(self) -> &'static str {
        match self {
            ProbeZustand::ModRechteWeg => "mod_rechte_weg",
            ProbeZustand::ModRechteWegStill => "mod_rechte_weg_still",
        }
    }
}

/// Letzte Chat-Zeile aus einem Kanal.
#[derive(Debug, Clone, Copy)]
struct ChatSpur {
    letzte: Option<DateTime<Utc>>,
}

impl ChatSpur {
    /// Kam zuletzt innerhalb des Frischefensters Chat an?
    fn frisch(&self) -> bool {
        self.letzte
            .is_some_and(|ts| Utc::now().signed_duration_since(ts) < CHAT_FRISCH_FENSTER)
    }

    /// Für die Meldung: seit wann kein Chat mehr da ist, in Klartext.
    fn beschreibung(&self) -> String {
        match self.letzte {
            None => "seit mindestens 30 Tagen keine Zeile".to_string(),
            Some(ts) => {
                let alter = Utc::now().signed_duration_since(ts);
                if alter.num_hours() < 1 {
                    format!("vor {} Minuten", alter.num_minutes().max(1))
                } else if alter.num_days() < 1 {
                    format!("vor {} Stunden", alter.num_hours())
                } else {
                    format!("vor {} Tagen", alter.num_days())
                }
            }
        }
    }
}

/// Anker der Bot-Sektion im Verwaltungs-Dashboard. Dort unten sitzt
/// "Bot vom Kanal trennen" — der einzige saubere Weg, den Bot loszuwerden.
pub const BOT_SECTION_URL: &str = "https://deutsche-deadlock-community.de/twitch/verwaltung#bot";

/// Twitch-Login des Bots, wie ihn der Streamer in seinem Chat tippt. Per Env
/// `BOT_TWITCH_LOGIN` überschreibbar.
pub fn bot_twitch_login() -> String {
    std::env::var("BOT_TWITCH_LOGIN")
        .ok()
        .map(|v| v.trim().to_lowercase())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "deutschedeadlockcommunity".to_string())
}

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
    let title = "🚨 Grace-Period abgelaufen, Streamer-Rolle entzogen".to_string();
    let description = format!(
        "Der Streamer **{twitch_login}** hat seinen Token innerhalb von \
         **{GRACE_PERIOD_DAYS} Tagen** nicht erneuert. Die Streamer-Rolle wurde \
         automatisch entzogen.\n\n\
         Streamer: [{twitch_login}](https://twitch.tv/{twitch_login})\n\
         Discord: {mention}\n\
         User ID: `{twitch_user_id}`\n\
         Bitte kontaktiere {mention} direkt. Ein Re-Auth über die Website stellt die \
         Rolle automatisch wieder her."
    );
    (title, description)
}

/// Admin-Embed-Inhalt bei Kanal-seitigem Bot-Ban.
///
/// Bis dahin lief der Bot-Ban komplett lautlos: der Streamer bekam eine DM, im
/// Admin-Log stand nichts. Damit war nicht zu sehen, dass ein Partner den Bot
/// rausgeworfen hat, obwohl er die Streamer-Rolle behielt. `quelle` benennt, wer
/// den Ban gemeldet hat (Chat-Signal, EventSub oder aktive Prüfung).
pub fn admin_bot_banned_text(
    twitch_login: &str,
    twitch_user_id: &str,
    discord_user_id: Option<&str>,
    quelle: &str,
    error_message: &str,
) -> (String, String) {
    let mention = match discord_user_id {
        Some(id) if !id.is_empty() => format!("<@{id}>"),
        _ => "unbekannt".to_string(),
    };
    let title = "🚫 Bot im Partner-Kanal gebannt".to_string();
    let err = truncate_chars(error_message.trim(), 200);
    let detail = if err.is_empty() {
        String::new()
    } else {
        format!("Signal: ```{err}```\n")
    };
    let description = format!(
        "Der Bot ist in **{twitch_login}** gebannt oder entmoddet. Auto-Raid, \
         Chat-Schutz und Analytics sind für diesen Kanal pausiert.\n\n\
         Streamer: [{twitch_login}](https://twitch.tv/{twitch_login})\n\
         Discord: {mention}\n\
         User ID: `{twitch_user_id}`\n\
         Erkannt über: `{quelle}`\n\
         {detail}\n\
         Der Streamer hat eine DM mit Recovery- und Trenn-Anleitung bekommen. \
         Die Streamer-Rolle bleibt vorerst bestehen."
    );
    (title, description)
}

/// Admin-Meldung: Die Moderator-Einsetzung scheitert, der Bot liest im Kanal
/// aber weiter mit. Das ist kein Bann, sondern eine fehlende Autorisierung.
///
/// Der Chat ist hier der Beweis: ein gebannter Account bekommt vom Kanal keine
/// Nachrichten mehr. Kommen sie an, trägt schlicht der Streamer-Token die
/// Einsetzung nicht mehr.
pub fn admin_mod_rechte_weg_text(
    twitch_login: &str,
    twitch_user_id: &str,
    letzte_chatzeile: &str,
) -> (String, String) {
    let title = format!("🔧 Kein Moderator mehr in {twitch_login}");
    let description = format!(
        "In **{twitch_login}** kann der Bot sich nicht mehr als Moderator \
         einsetzen. Ein Bann ist es nicht: aus dem Kanal kommen weiter \
         Chat-Nachrichten an, zuletzt {letzte_chatzeile}.\n\n\
         Streamer: [{twitch_login}](https://twitch.tv/{twitch_login})\n\
         User ID: `{twitch_user_id}`\n\n\
         **Zustand:** Der Bot liest mit, hat aber keine Mod-Rechte. Ursache ist \
         die Streamer-Autorisierung (abgelaufen oder ohne den nötigen Scope). \
         Behoben wird das durch eine neue Autorisierung unter \
         {DEFAULT_REAUTH_URL}.\n\n\
         Pause, Blacklist und Streamer-DM bleiben aus. Diese Meldung kommt genau \
         einmal und erst wieder, wenn sich der Zustand ändert."
    );
    (title, description)
}

/// Admin-Meldung: Moderator-Einsetzung abgelehnt, und aus dem Kanal kam schon
/// länger kein Chat an.
///
/// Hier fehlt der Beweis in beide Richtungen: ein stiller Kanal sieht genauso
/// aus wie ein Bann. Der Text nennt deshalb nur, was gemessen wurde.
pub fn admin_mod_rechte_weg_still_text(
    twitch_login: &str,
    twitch_user_id: &str,
    letzte_chatzeile: &str,
) -> (String, String) {
    let title = format!("🔇 Kein Moderator mehr in {twitch_login}, Kanal still");
    let description = format!(
        "In **{twitch_login}** wird die Moderator-Einsetzung abgelehnt, und aus \
         dem Kanal kam zuletzt {letzte_chatzeile} eine Nachricht an.\n\n\
         Streamer: [{twitch_login}](https://twitch.tv/{twitch_login})\n\
         User ID: `{twitch_user_id}`\n\n\
         **Zustand:** keine Mod-Rechte. Ob der Kanal nur nicht streamt oder der \
         Bot dort rausgeflogen ist, lässt sich von hier aus nicht unterscheiden. \
         Sicher entschieden wird das erst beim nächsten Sendeversuch im Chat.\n\n\
         Pause, Blacklist und Streamer-DM bleiben aus. Diese Meldung kommt genau \
         einmal und erst wieder, wenn sich der Zustand ändert."
    );
    (title, description)
}

/// Admin-Meldung: Der Bot ist im Kanal wieder Moderator.
pub fn admin_mod_rechte_zurueck_text(twitch_login: &str) -> (String, String) {
    let title = format!("✅ Wieder Moderator in {twitch_login}");
    let description = format!(
        "Die Moderator-Einsetzung in **{twitch_login}** greift wieder. Der zuvor \
         gemeldete Zustand ist damit erledigt, es ist nichts zu tun."
    );
    (title, description)
}

/// Admin-Meldung, wenn der Bot eine unbelegte eigene Bann-Markierung zurücknimmt.
pub fn admin_ban_probe_rueckname_text(twitch_login: &str) -> (String, String) {
    let title = "♻️ Unbelegte Bann-Markierung zurückgenommen".to_string();
    let description = format!(
        "**{twitch_login}** war als gebannt markiert, obwohl das nie belegt war: \
         die Markierung stammt aus der aktiven Prüfung, die einen kaputten \
         Streamer-Token nicht von einem Bann unterscheiden konnte.\n\n\
         Streamer: [{twitch_login}](https://twitch.tv/{twitch_login})\n\n\
         Pause und Blacklist-Eintrag sind aufgehoben, der Kanal läuft wieder \
         normal. Ist der Bot dort doch gebannt, fällt das beim nächsten \
         Chat-Versuch auf und wird dann sauber erkannt."
    );
    (title, description)
}

/// User-DM-Text bei Token-Fehler (Erst-DM). Text-only mit Re-Auth-Link.
///
/// Der Re-Auth läuft bewusst über das Verwaltungs-Dashboard und nicht über einen
/// nackten OAuth-Link: nach der Neu-Autorisierung im ersten Punkt
/// "Twitch-Verbindung" gilt das Dashboard sofort wieder als vertraut.
pub fn user_dm_token_error_text(twitch_login: &str, reauth_url: &str) -> String {
    format!(
        "⚠️ **Twitch-Verbindung fehlgeschlagen**\n\n\
         Die Verbindung für **{twitch_login}** ist abgelaufen (z. B. nach Passwort- \
         oder 2FA-Änderung). Auto-Raid, Chat-Schutz und Analytics pausieren, bis sie \
         wieder steht.\n\n\
         **So verbindest du neu:**\n\
         1️⃣ Dashboard öffnen: {reauth_url}\n\
         2️⃣ Erster Punkt **Twitch-Verbindung**\n\
         3️⃣ **Bot neu autorisieren** klicken\n\n\
         Danach läuft alles von allein weiter.\n\n\
         ⏳ Du hast {GRACE_PERIOD_DAYS} Tage, danach entfällt die Streamer-Rolle.\n\n\
         Willst du kein Partner mehr sein, ignorier die Nachricht einfach."
    )
}

/// User-DM-Text als Grace-Reminder (Python `is_reminder=True`).
pub fn user_dm_reminder_text(twitch_login: &str, reauth_url: &str) -> String {
    format!(
        "⚠️ **Twitch-Verbindung fehlt weiterhin**\n\n\
         Für **{twitch_login}** ist die Verbindung seit {GRACE_PERIOD_DAYS} Tagen \
         offen. Die Bot-Funktionen bleiben so lange aus.\n\n\
         **So verbindest du neu:**\n\
         1️⃣ Dashboard öffnen: {reauth_url}\n\
         2️⃣ Erster Punkt **Twitch-Verbindung**\n\
         3️⃣ **Bot neu autorisieren** klicken\n\n\
         Willst du kein Partner mehr sein, ignorier die Nachricht einfach."
    )
}

/// User-DM-Text bei Kanal-seitigem Bot-Ban (Python `_send_user_dm_bot_banned`).
/// Der technische `error_message` fließt bewusst NICHT in die DM (verwirrt den
/// Streamer) — er bleibt im Blacklist-`reason` und in den Logs erhalten.
///
/// Zweiter Block ist der saubere Ausstieg: ein Ban allein trennt den Bot nicht,
/// er lässt ihn nur blockiert zurück. Wer ihn wirklich loswerden will, muss erst
/// entbannen (sonst kann der Bot seine Mod-Rechte nicht abgeben) und dann im
/// Dashboard trennen.
pub fn user_dm_bot_banned_text(twitch_login: &str, _error_message: &str) -> String {
    let bot = bot_twitch_login();
    format!(
        "⚠️ **Der Bot ist in deinem Kanal blockiert**\n\n\
         Der Bot wurde in **{twitch_login}** gebannt oder als Moderator entfernt. \
         Auto-Raid, Chat-Schutz und Analytics pausieren so lange.\n\n\
         **War das ein Versehen?** Zwei Befehle in deinem Chat:\n\
         1️⃣ `/unban {bot}`\n\
         2️⃣ `/mod {bot}`\n\n\
         Danach läuft alles von allein wieder an.\n\n\
         **Willst du den Bot loswerden?** Bitte in dieser Reihenfolge:\n\
         1️⃣ `/unban {bot}` in deinem Chat (ohne das behält der Bot seine Rechte)\n\
         2️⃣ {BOT_SECTION_URL} öffnen\n\
         3️⃣ Ganz unten **Bot vom Kanal trennen** klicken\n\n\
         Damit ist er sauber raus und wird nicht mehr für dich tätig."
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
    /// Admin-Log-Embed wurde zugestellt.
    pub admin_sent: bool,
    /// Vorher existierte bereits ein `bot_banned`-Blacklist-Grund.
    pub already_flagged: bool,
}

/// Leitet aus dem internen Ban-Grund ab, welcher Pfad den Ban gemeldet hat.
/// Landet als `Erkannt über` im Admin-Embed, damit im Log unterscheidbar ist,
/// ob der Ban beim Senden aufgefallen ist oder erst die aktive Prüfung ihn fand.
fn quelle_aus_reason(reason: &str) -> &'static str {
    let reason = reason.to_lowercase();
    if reason.contains("ban_probe") {
        "aktive Prüfung"
    } else if reason.contains("eventsub") {
        "EventSub"
    } else if reason.contains("chat") {
        "Chat-Sendeversuch"
    } else {
        "unbekannt"
    }
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
                    "Twitch-Token seit {GRACE_PERIOD_DAYS} Tagen ungültig, Grace-Period abgelaufen"
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
                    "Grace-Period abgelaufen: Rolle entzogen, token_error_expired gesetzt"
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

        // Admin-Log: ein Partner-Ban ist ein Vorgang, der gesehen werden muss.
        // Ohne diese Meldung fiel ein Rauswurf nur auf, wenn zufällig jemand die
        // Blacklist gelesen hat.
        let (title, description) = admin_bot_banned_text(
            twitch_login,
            twitch_user_id,
            discord_user_id.as_deref(),
            quelle_aus_reason(error_message),
            error_message,
        );
        let admin_sent = self
            .notifier
            .send_admin_embed(TOKEN_ERROR_CHANNEL_ID, &title, &description)
            .await;

        tracing::info!(
            user = %mask(twitch_login),
            user_dm = user_dm_sent,
            admin = admin_sent,
            "Bot-Ban-Opt-out verarbeitet"
        );
        BotBannedOutcome {
            opt_out_marked: true,
            user_dm_sent,
            admin_sent,
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

    /// Nimmt Bot-Ban-Markierungen zurück, die aus der aktiven Prüfung stammen.
    ///
    /// Diese Prüfung durfte einmal selbst pausieren und hat dabei einen kaputten
    /// Streamer-Token für einen Kanal-Bann gehalten. Alles, was sie damals gesetzt
    /// hat, ist unbelegt: der Grund-String trägt `ban_probe`, kein anderer Pfad
    /// benutzt ihn. Statt solche Zustände von Hand aus der Datenbank zu putzen,
    /// räumt der Bot sie hier selbst weg.
    ///
    /// Bewusst ohne erneute Twitch-Abfrage. Ist der Kanal wirklich gebannt, fällt
    /// das beim nächsten Chat-Versuch auf und läuft über den reaktiven Pfad, der
    /// einen echten `sender_banned`-Drop gesehen hat. Ein unbelegter Verdacht darf
    /// keinen Partner dauerhaft lahmlegen.
    ///
    /// Liefert die Anzahl geheilter Kanäle.
    pub async fn clear_unverified_ban_probe_marks(&self) -> u64 {
        let logins = match sqlx::query_as::<_, (String,)>(
            r#"
            DELETE FROM twitch_raid_blacklist
             WHERE LOWER(COALESCE(reason, '')) LIKE '%ban_probe%'
            RETURNING target_login
            "#,
        )
        .fetch_all(&self.pool)
        .await
        {
            Ok(rows) => rows,
            Err(error) => {
                tracing::warn!(%error, "Ban-Probe-Cleanup: Blacklist-Query fehlgeschlagen");
                return 0;
            }
        };
        if logins.is_empty() {
            return 0;
        }

        let mut healed = 0u64;
        for (login,) in logins {
            // Die Pause nur dort aufheben, wo sie genau diesen Grund trägt. Ein
            // Kanal, der zusätzlich echt gesperrt ist, bleibt gesperrt.
            let result = sqlx::query(
                r#"
                UPDATE twitch_partners
                   SET technical_pause_reason = NULL,
                       raid_bot_enabled = 1
                 WHERE LOWER(twitch_login) = LOWER($1)
                   AND LOWER(TRIM(COALESCE(technical_pause_reason, ''))) = 'bot_banned'
                "#,
            )
            .bind(&login)
            .execute(&self.pool)
            .await;
            match result {
                Ok(result) if result.rows_affected() > 0 => {
                    healed += 1;
                    tracing::warn!(
                        login = %login,
                        "Unbelegte Bot-Ban-Markierung aus der aktiven Prüfung zurückgenommen"
                    );
                    let (title, description) = admin_ban_probe_rueckname_text(&login);
                    self.notifier
                        .send_admin_embed(TOKEN_ERROR_CHANNEL_ID, &title, &description)
                        .await;
                }
                Ok(_) => {
                    tracing::info!(
                        login = %login,
                        "Ban-Probe-Blacklist entfernt, Partner-Pause hatte einen anderen Grund"
                    );
                }
                Err(error) => {
                    tracing::warn!(%error, login = %login, "Ban-Probe-Cleanup: Pause nicht aufhebbar");
                }
            }
        }
        healed
    }

    /// Aktive Ban-Prüfung über alle gesunden Partner-Kanäle.
    ///
    /// Die bisherige Erkennung war rein reaktiv: sie hing daran, dass der Bot in
    /// dem Kanal etwas sendet oder eine EventSub-Subscription scheitert. In einem
    /// Kanal, in dem der Bot gerade still ist, blieb ein Ban deshalb beliebig
    /// lange unbemerkt, der Partner galt weiter als aktiv und behielt seine
    /// Streamer-Rolle. Dieser Sweep fragt den Zustand stattdessen selbst ab.
    ///
    /// Kandidaten sind nur Kanäle mit gesundem Streamer-Token und ohne bereits
    /// gesetzten Bot-Ban-/Blocked-Marker. Kanäle in der Deadlock-Pause bleiben
    /// außen vor: dort ist der Bot bewusst entmoddet, die Probe würde ihn sofort
    /// wieder einsetzen.
    ///
    /// **Die Prüfung meldet nur, sie reagiert nicht.** Ein fehlgeschlagener
    /// Moderator-Einsetzungs-Versuch ist ein Indiz, kein Beweis: er kann auch an
    /// einem kaputten Token oder einem fehlenden Scope liegen. Genau diese
    /// Verwechslung hat einem gesunden Partner eine Bann-DM eingebracht und ihn
    /// pausiert. Deshalb landet der Befund ausschließlich im Admin-Log; die
    /// vollen Konsequenzen (Pause, Blacklist, Streamer-DM) zieht weiterhin nur
    /// der reaktive Pfad, der einen echten Chat-Drop mit `sender_banned` gesehen
    /// hat.
    ///
    /// Liefert die Anzahl gemeldeter Verdachtsfälle.
    pub async fn detect_bot_bans(&self) -> u64 {
        let Some(probe) = &self.bot_ban_status_probe else {
            tracing::debug!("Bot-Ban-Sweep übersprungen: kein Ban-Status-Provisioner verdrahtet");
            return 0;
        };

        let rows = match sqlx::query_as::<_, (String, String)>(
            r#"
            SELECT p.twitch_user_id,
                   COALESCE(NULLIF(LOWER(p.twitch_login), ''),
                            NULLIF(LOWER(a.twitch_login), ''),
                            '') AS twitch_login
              FROM twitch_partners p
              JOIN twitch_raid_auth a
                ON a.twitch_user_id = p.twitch_user_id
             WHERE LOWER(TRIM(COALESCE(p.status, ''))) = 'active'
               AND COALESCE(a.needs_reauth, TRUE) = FALSE
               AND a.access_token_enc IS NOT NULL
               AND OCTET_LENGTH(a.access_token_enc) > 0
               AND LOWER(TRIM(COALESCE(p.technical_pause_reason, '')))
                   NOT IN ('blocked', 'bot_banned')
               -- Kanäle in der Deadlock-Pause bleiben außen vor: dort ist der
               -- Bot absichtlich entmoddet, und die Probe würde ihn im selben
               -- Call wieder einsetzen (siehe `crate::deadlock_pause`).
               AND p.deadlock_pause_unmodded_at IS NULL
               AND NOT EXISTS (
                   SELECT 1
                     FROM twitch_raid_blacklist rb
                    WHERE (rb.target_id = p.twitch_user_id
                        OR LOWER(rb.target_login) = LOWER(p.twitch_login))
                      AND LOWER(COALESCE(rb.reason, '')) LIKE '%bot_banned%'
               )
             ORDER BY p.twitch_login
             LIMIT $1
            "#,
        )
        .bind(BAN_PROBE_MAX_PER_SWEEP)
        .fetch_all(&self.pool)
        .await
        {
            Ok(rows) => rows,
            Err(error) => {
                tracing::warn!(%error, "Bot-Ban-Sweep: DB-Query fehlgeschlagen");
                return 0;
            }
        };

        let mut detected = 0u64;
        for (twitch_user_id, twitch_login) in rows {
            if twitch_login.is_empty() {
                continue;
            }
            match probe.bot_ban_status(&twitch_user_id, &twitch_login).await {
                BotBanStatus::Banned => {
                    // Die abgelehnte Einsetzung allein sagt nicht, was los ist.
                    // Der Chat entscheidet: kommen aus dem Kanal weiter
                    // Nachrichten an, ist der Bot nicht gebannt, sondern nur
                    // seine Autorisierung durch.
                    let chat = self.letzte_chatzeile(&twitch_login).await;
                    let zustand = if chat.frisch() {
                        ProbeZustand::ModRechteWeg
                    } else {
                        ProbeZustand::ModRechteWegStill
                    };
                    if self
                        .zustand_ist_neu(&twitch_user_id, &twitch_login, zustand)
                        .await
                    {
                        detected += 1;
                        let (title, description) = match zustand {
                            ProbeZustand::ModRechteWeg => admin_mod_rechte_weg_text(
                                &twitch_login,
                                &twitch_user_id,
                                &chat.beschreibung(),
                            ),
                            ProbeZustand::ModRechteWegStill => admin_mod_rechte_weg_still_text(
                                &twitch_login,
                                &twitch_user_id,
                                &chat.beschreibung(),
                            ),
                        };
                        tracing::warn!(
                            login = %twitch_login,
                            zustand = zustand.als_text(),
                            "Aktive Prüfung: Zustand gemeldet, keine Reaktion"
                        );
                        self.notifier
                            .send_admin_embed(TOKEN_ERROR_CHANNEL_ID, &title, &description)
                            .await;
                    } else {
                        tracing::debug!(
                            login = %twitch_login,
                            zustand = zustand.als_text(),
                            "Aktive Prüfung: Zustand unverändert, keine neue Meldung"
                        );
                    }
                }
                // NotBanned heißt hier zugleich: der Bot ist (wieder) Moderator,
                // die Probe setzt ihn im selben Call ein.
                BotBanStatus::NotBanned => {
                    // Entwarnung nur für Kanäle, die vorher gemeldet waren. Ein
                    // gesunder Kanal, der gesund bleibt, sagt gar nichts.
                    if self.zustand_aufloesen(&twitch_user_id).await {
                        let (title, description) = admin_mod_rechte_zurueck_text(&twitch_login);
                        self.notifier
                            .send_admin_embed(TOKEN_ERROR_CHANNEL_ID, &title, &description)
                            .await;
                    }
                }
                // Unknown ist ein Netz-/Token-Problem und sagt über den Kanal
                // nichts aus: kein Zustandswechsel, keine Meldung.
                BotBanStatus::Unknown => {}
            }
            tokio::time::sleep(BAN_PROBE_DELAY).await;
        }
        detected
    }

    /// Wann zuletzt eine Chat-Nachricht aus diesem Kanal angekommen ist.
    ///
    /// Der Bot schreibt jede empfangene Zeile mit. Bekommt er weiter welche, ist
    /// er im Kanal nicht gebannt: Twitch stellt einem gebannten Account keine
    /// Chat-Nachrichten mehr zu.
    async fn letzte_chatzeile(&self, twitch_login: &str) -> ChatSpur {
        let letzte = sqlx::query_scalar::<_, Option<DateTime<Utc>>>(
            // MAX(...) liefert genau eine Zeile, bei leerem Kanal mit NULL.
            r#"
            SELECT MAX(message_ts)
              FROM twitch_chat_messages
             WHERE LOWER(streamer_login) = LOWER($1)
               AND message_ts > NOW() - INTERVAL '30 days'
            "#,
        )
        .bind(twitch_login)
        .fetch_one(&self.pool)
        .await;
        match letzte {
            Ok(ts) => ChatSpur { letzte: ts },
            Err(error) => {
                tracing::warn!(%error, login = %twitch_login, "Chat-Spur nicht lesbar");
                ChatSpur { letzte: None }
            }
        }
    }

    /// Bucht den Zustand und sagt, ob er neu ist. Nur ein Wechsel wird gemeldet,
    /// jede weitere Prüfung zählt still mit. Schlägt die Buchung fehl, gilt der
    /// Zustand als bekannt: lieber eine Meldung zu wenig als der stündliche
    /// Wiederholer, den diese Tabelle abstellt.
    async fn zustand_ist_neu(
        &self,
        twitch_user_id: &str,
        twitch_login: &str,
        zustand: ProbeZustand,
    ) -> bool {
        let row = sqlx::query_as::<_, (bool,)>(
            r#"
            INSERT INTO twitch_ban_probe_zustand
                        (twitch_user_id, twitch_login, zustand, seit, letzte_probe, proben)
                 VALUES ($1, $2, $3, NOW(), NOW(), 1)
            ON CONFLICT (twitch_user_id) DO UPDATE
                    SET twitch_login = EXCLUDED.twitch_login,
                        zustand = EXCLUDED.zustand,
                        seit = CASE
                                   WHEN twitch_ban_probe_zustand.zustand = EXCLUDED.zustand
                                   THEN twitch_ban_probe_zustand.seit
                                   ELSE NOW()
                               END,
                        letzte_probe = NOW(),
                        proben = CASE
                                     WHEN twitch_ban_probe_zustand.zustand = EXCLUDED.zustand
                                     THEN twitch_ban_probe_zustand.proben + 1
                                     ELSE 1
                                 END
              RETURNING (xmax = 0 OR proben = 1) AS gewechselt
            "#,
        )
        .bind(twitch_user_id)
        .bind(twitch_login)
        .bind(zustand.als_text())
        .fetch_one(&self.pool)
        .await;
        match row {
            Ok((gewechselt,)) => gewechselt,
            Err(error) => {
                tracing::warn!(%error, login = %twitch_login, "Prüf-Zustand nicht speicherbar");
                false
            }
        }
    }

    /// Räumt den gemerkten Zustand ab, sobald der Kanal wieder in Ordnung ist.
    /// Liefert `true`, wenn vorher tatsächlich etwas gemeldet war (nur dann gibt
    /// es eine Entwarnung).
    async fn zustand_aufloesen(&self, twitch_user_id: &str) -> bool {
        let geloescht = sqlx::query_scalar::<_, i64>(
            r#"
            WITH weg AS (
                DELETE FROM twitch_ban_probe_zustand
                      WHERE twitch_user_id = $1
                  RETURNING 1
            )
            SELECT COUNT(*) FROM weg
            "#,
        )
        .bind(twitch_user_id)
        .fetch_one(&self.pool)
        .await;
        match geloescht {
            Ok(count) => count > 0,
            Err(error) => {
                tracing::warn!(%error, "Prüf-Zustand nicht löschbar");
                false
            }
        }
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
        discord_user_id_for(&self.pool, twitch_user_id, twitch_login).await
    }
}

/// Discord-ID eines Streamers aus `twitch_streamer_identities`, gesucht über
/// User-ID oder Login. Freie Funktion, damit alle Lifecycle-Pfade (Token-Fehler,
/// Bot-Ban, Deadlock-Pause) dieselbe Auflösung benutzen statt je eigener Queries.
pub async fn discord_user_id_for(
    pool: &PgPool,
    twitch_user_id: &str,
    twitch_login: &str,
) -> Option<String> {
    let login = tb_domain::normalize_twitch_login(twitch_login).unwrap_or_default();
    // Einrückung des Query-Strings bewusst unverändert gelassen: der
    // sqlx-Offline-Cache hasht den Literal-Text, ein Umformatieren würde den
    // vorbereiteten Eintrag verwaisen lassen.
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
    .fetch_optional(pool)
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

    /// Merkt sich, welche Kanäle die Ban-Probe überhaupt angefasst hat.
    #[derive(Default)]
    struct RecordingBanProbe {
        seen: std::sync::Mutex<Vec<String>>,
    }

    #[async_trait::async_trait]
    impl BotBanStatusProbe for Arc<RecordingBanProbe> {
        async fn bot_ban_status(&self, _twitch_user_id: &str, twitch_login: &str) -> BotBanStatus {
            self.seen.lock().unwrap().push(twitch_login.to_string());
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

    /// Der Re-Auth-Weg ist bewusst der Dashboard-Weg: erst dadurch gilt das
    /// Dashboard danach wieder als vertraut. Ein nackter OAuth-Link tut das nicht.
    #[test]
    fn token_error_dm_fuehrt_durchs_dashboard() {
        let text = user_dm_token_error_text("foo", DEFAULT_REAUTH_URL);
        assert!(text.contains("https://deutsche-deadlock-community.de/twitch/verwaltung"));
        assert!(text.contains("Twitch-Verbindung"));
        assert!(text.contains("Bot neu autorisieren"));
    }

    #[test]
    fn default_reauth_url_zeigt_aufs_verwaltungs_dashboard() {
        assert_eq!(
            DEFAULT_REAUTH_URL,
            "https://deutsche-deadlock-community.de/twitch/verwaltung"
        );
        assert!(BOT_SECTION_URL.starts_with(DEFAULT_REAUTH_URL));
        assert!(BOT_SECTION_URL.ends_with("#bot"));
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

    /// Ein Ban ist kein Trennen: ohne Unban behält der Bot seine Mod-Rechte, weil
    /// Twitch den Unmod-Call für einen gebannten User ablehnt. Die DM muss die
    /// Reihenfolge deshalb ausdrücklich nennen.
    #[test]
    fn bot_banned_dm_erklaert_sauberes_trennen_in_reihenfolge() {
        let text = user_dm_bot_banned_text("foo", "");
        let unban_pos = text
            .find("/unban deutschedeadlockcommunity` in deinem Chat")
            .expect("Unban-Schritt der Trenn-Anleitung fehlt");
        let dashboard_pos = text.find(BOT_SECTION_URL).expect("Dashboard-Link fehlt");
        let trennen_pos = text
            .find("Bot vom Kanal trennen")
            .expect("Trenn-Button fehlt");
        assert!(
            unban_pos < dashboard_pos && dashboard_pos < trennen_pos,
            "Reihenfolge muss Unban → Dashboard → Trennen sein"
        );
    }

    #[test]
    fn reminder_dm_referenziert_grace_dauer() {
        let text = user_dm_reminder_text("foo", DEFAULT_REAUTH_URL);
        assert!(text.contains(&GRACE_PERIOD_DAYS.to_string()));
        assert!(text.contains("Verbindung fehlt weiterhin"));
        assert!(text.contains("Bot neu autorisieren"));
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
                technical_pause_reason text, raid_bot_enabled integer DEFAULT 1)",
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

    /// Der Bot raeumt seinen eigenen Fehler auf: Markierungen aus der aktiven
    /// Pruefung sind unbelegt und werden zurueckgenommen, ohne dass jemand die
    /// Datenbank von Hand anfassen muss. Ein echter Bann bleibt bestehen.
    #[tokio::test]
    async fn ban_probe_marken_werden_selbst_zurueckgenommen() {
        if test_db_url().is_none() {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        }
        let pool = setup_db("tl_ban_probe_cleanup").await;

        // Kanal A: Opfer der Fehlklassifikation.
        sqlx::query(
            "INSERT INTO twitch_partners (twitch_user_id, twitch_login, status, technical_pause_reason, raid_bot_enabled)
             VALUES ('1', 'falschpositiv', 'active', 'bot_banned', 0)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_raid_blacklist (target_id, target_login, reason, added_at)
             VALUES ('1', 'falschpositiv', 'chat_bot_banned_in_channel: ban_probe: Twitch lehnt ab', '2026-08-15T03:28:24+00:00')",
        )
        .execute(&pool)
        .await
        .unwrap();

        // Kanal B: echter Bann aus dem reaktiven Pfad, muss bestehen bleiben.
        sqlx::query(
            "INSERT INTO twitch_partners (twitch_user_id, twitch_login, status, technical_pause_reason, raid_bot_enabled)
             VALUES ('2', 'echtgebannt', 'active', 'bot_banned', 0)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_raid_blacklist (target_id, target_login, reason, added_at)
             VALUES ('2', 'echtgebannt', 'chat_bot_banned_in_channel: sender_banned', '2026-08-15T03:00:00+00:00')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let notifier = Arc::new(CountingNotifier::default());
        let reactor = TokenLifecycleReactor::new(pool.clone(), notifier.clone());

        assert_eq!(reactor.clear_unverified_ban_probe_marks().await, 1);

        // Kanal A ist frei, Kanal B unangetastet.
        let a: (Option<String>, Option<i32>) = sqlx::query_as(
            "SELECT technical_pause_reason, raid_bot_enabled FROM twitch_partners WHERE twitch_user_id = '1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(a.0, None, "Pause muss weg sein");
        assert_eq!(a.1, Some(1), "Raid muss wieder an sein");

        let b: (Option<String>,) = sqlx::query_as(
            "SELECT technical_pause_reason FROM twitch_partners WHERE twitch_user_id = '2'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            b.0.as_deref(),
            Some("bot_banned"),
            "ein echter Bann bleibt bestehen"
        );

        let rest: Vec<(String,)> =
            sqlx::query_as("SELECT target_login FROM twitch_raid_blacklist ORDER BY target_login")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(rest.len(), 1);
        assert_eq!(rest[0].0, "echtgebannt");

        // Genau eine Admin-Meldung, keine Streamer-DM.
        assert_eq!(notifier.admin_embeds.load(Ordering::SeqCst), 1);
        assert_eq!(notifier.user_dms.load(Ordering::SeqCst), 0);

        // Zweiter Lauf ist ein No-op.
        assert_eq!(reactor.clear_unverified_ban_probe_marks().await, 0);
    }

    /// Regression miracleghost9: Die aktive Pruefung darf einen Kanal nicht
    /// pausieren und dem Streamer keine DM schicken. Ein abgelehnter
    /// Moderator-Einsetzungs-Versuch kann auch an einem kaputten Token liegen;
    /// genau diese Verwechslung hat einen gesunden Partner getroffen. Gemeldet
    /// wird nur ins Admin-Log.
    #[tokio::test]
    async fn ban_sweep_meldet_nur_und_pausiert_niemanden() {
        if test_db_url().is_none() {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        }
        let pool = setup_db("tl_ban_sweep_meldet").await;
        sqlx::query(
            "ALTER TABLE twitch_partners ADD COLUMN IF NOT EXISTS deadlock_pause_unmodded_at text",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_partners (twitch_user_id, twitch_login, status) VALUES ('1', 'verdaechtig', 'active')",
        )
        .execute(&pool)
        .await
        .unwrap();
        seed_raid_auth(
            &pool,
            "1",
            "verdaechtig",
            true,
            false,
            Utc::now() + chrono::Duration::days(1),
        )
        .await;
        sqlx::query(
            "INSERT INTO twitch_streamer_identities (twitch_user_id, twitch_login, discord_user_id)
             VALUES ('1', 'verdaechtig', '4711')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let notifier = Arc::new(CountingNotifier::default());
        let reactor = TokenLifecycleReactor::new(pool.clone(), notifier.clone())
            .with_bot_ban_status_probe(Arc::new(FixedBotBanStatus(BotBanStatus::Banned)));

        assert_eq!(reactor.detect_bot_bans().await, 1, "Verdacht wird gemeldet");

        // Genau eine Admin-Meldung, keine einzige Streamer-DM.
        assert_eq!(notifier.admin_embeds.load(Ordering::SeqCst), 1);
        assert_eq!(
            notifier.user_dms.load(Ordering::SeqCst),
            0,
            "die aktive Pruefung darf den Streamer nicht anschreiben"
        );

        // Und der Kanal bleibt unangetastet: keine Pause, keine Blacklist.
        let pause: Option<String> = sqlx::query_scalar(
            "SELECT technical_pause_reason FROM twitch_partners WHERE twitch_user_id = '1'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(pause, None, "kein technical_pause_reason gesetzt");
        let blacklisted: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM twitch_raid_blacklist WHERE target_id = '1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(blacklisted, 0, "kein Blacklist-Eintrag");
    }

    /// Der aktive Ban-Sweep probt über `add_channel_moderator` und setzt den Bot
    /// dabei als Moderator ein. In einem Kanal, der wegen Deadlock-Pause
    /// absichtlich entmoddet ist, würde er die Pause damit sofort aufheben.
    #[tokio::test]
    async fn ban_sweep_fasst_kanaele_in_der_deadlock_pause_nicht_an() {
        if test_db_url().is_none() {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        }
        let pool = setup_db("tl_ban_sweep_pause").await;
        sqlx::query(
            "ALTER TABLE twitch_partners ADD COLUMN IF NOT EXISTS deadlock_pause_unmodded_at text",
        )
        .execute(&pool)
        .await
        .unwrap();
        for (uid, login) in [("1", "normal"), ("2", "pausiert")] {
            sqlx::query(
                "INSERT INTO twitch_partners (twitch_user_id, twitch_login, status) VALUES ($1, $2, 'active')",
            )
            .bind(uid)
            .bind(login)
            .execute(&pool)
            .await
            .unwrap();
            seed_raid_auth(
                &pool,
                uid,
                login,
                true,
                false,
                Utc::now() + chrono::Duration::days(1),
            )
            .await;
        }
        sqlx::query(
            "UPDATE twitch_partners SET deadlock_pause_unmodded_at = '2026-01-01T00:00:00+00:00'
              WHERE twitch_login = 'pausiert'",
        )
        .execute(&pool)
        .await
        .unwrap();

        let probe = Arc::new(RecordingBanProbe::default());
        let reactor =
            TokenLifecycleReactor::new(pool.clone(), Arc::new(CountingNotifier::default()))
                .with_bot_ban_status_probe(Arc::new(probe.clone()));
        reactor.detect_bot_bans().await;

        let seen = probe.seen.lock().unwrap().clone();
        assert_eq!(
            seen,
            vec!["normal".to_string()],
            "der pausierte Kanal darf nicht geprobt werden"
        );
    }
}
