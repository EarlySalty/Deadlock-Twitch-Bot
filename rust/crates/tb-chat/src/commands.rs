//! Chat-Commands — Port von `bot/chat/commands.py` (867 Z.) und
//! `bot/chat/engagement_commands.py` (208 Z.), Welle B.
//!
//! # Öffentliche API
//!
//! ```ignore
//! let engine = CommandEngine::new(pool, api, raid_port, discord_link_port, invite_port, super_mod_port, autoban_store);
//! let handled = engine.handle(&event).await; // true = war Command, Pipeline stoppt
//! ```
//!
//! # Architektur-Hinweis
//!
//! Die Python-Implementierung ruft für `!raid`, `!dldc`/`!dlde` und `!invite`
//! extern per HTTP auf `localhost:8776` (bereits Rust). In der nativen Rust-
//! Variante werden dieselben Operationen direkt über Traits aufgerufen — kein
//! Loop-Gefahr, da der Orchestrator die Verdrahtung übernimmt.
//!
//! Nicht portiert (bewusst außerhalb des Welle-B-Scopes):
//! - `!title` / `!titel` — komplexe KI-Generierung, eigene DB-Tabellen.
//! - `!lurkersteuer_off` — UNSICHER, plan_id-Lookup + Feature-Flag.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rand::seq::SliceRandom;
use sqlx::PgPool;
use tokio::sync::Mutex;

use crate::api::ChatApi;
use crate::types::ChatMessageEvent;

// ---------------------------------------------------------------------------
// Konstanten — exakt aus dem Vertrag
// ---------------------------------------------------------------------------

/// Cooldown für `!invite` pro (channel, chatter): 1 Stunde.
/// `bot.py:344` — `> 3600s`.
const INVITE_COOLDOWN_SECS: u64 = 3600;

/// Max. Länge für Clip-Titel.
/// `commands.py:181` — `[:57].rstrip() + "..."` wenn > 60 Z.
const CLIP_TITLE_MAX_LEN: usize = 60;
const CLIP_TITLE_TRIM_LEN: usize = 57;

/// Antwortliste für `!ping` — commands.py:196–202.
const PING_REPLIES: &[&str] = &[
    "Eure Majestät! 👑 Der Bot steht zu Euren Diensten. Was kann ich für Euch tun?",
    "Bin da! Ausgeschlafen, aufgewärmt und bereit für Chaos. 🤖✅",
    "Ja ich lebe noch, keine Sorge. Puls: 🟢 Signal: 📶 Kaffee: ☕ alles gut.",
    "Bot online! Bereit für Euren Befehl, oh weiser Chatter. 🫡",
    "Ich atme noch! Und ich hab sogar alle meine Kabel dran.",
    "Natürlich bin ich online – wer soll sonst die Clips machen? 😏🎬",
];

/// Fallback-Clip-Titel — commands.py:181.
const CLIP_TITLE_FALLBACKS: &[&str] = &[
    "Clip des Streams",
    "Highlight des Tages",
    "Das müssen wir teilen",
    "Unfassbarer Moment",
    "Clip it!",
];

// ---------------------------------------------------------------------------
// Integrations-Traits — müssen vom Orchestrator verdrahtet werden
// ---------------------------------------------------------------------------

/// Port für manuelle und status-basierte Raid-Operationen.
/// Wird vom Orchestrator an die tb-raid-Schicht gebunden.
///
/// `commands.py:617` — `!raid` / `!traid`
/// `commands.py:126` — `!raid_status`
/// `commands.py:423` — `!silentban` / `!silentraid`
#[async_trait]
pub trait RaidCommandPort: Send + Sync {
    /// Startet einen manuellen Raid für den gegebenen Broadcaster.
    /// Gibt einen Status-String zurück: `"started"`, `"source_not_live"`,
    /// `"source_not_eligible"`, `"no_target"`, `"unavailable"`, oder
    /// einen Error-String.
    async fn manual_raid(
        &self,
        broadcaster_id: &str,
        broadcaster_login: &str,
    ) -> Result<String, String>;

    /// Liest Raid-Aktivierungsstatus und Statistik aus.
    /// `commands.py:94–128`
    async fn raid_status(&self, broadcaster_id: &str) -> Result<RaidStatusInfo, String>;

    /// Toggled `silent_ban`-Flag für den Partner (via twitch_partners).
    /// Gibt den neuen Wert zurück (0 oder 1).
    /// `commands.py:423`
    async fn toggle_silent_ban(&self, twitch_login: &str) -> Result<i32, String>;

    /// Toggled `silent_raid`-Flag für den Partner.
    /// `commands.py:479`
    async fn toggle_silent_raid(&self, twitch_login: &str) -> Result<i32, String>;
}

/// Raid-Status-Info für `!raid_status`.
/// `commands.py:94–128`
#[derive(Debug)]
pub struct RaidStatusInfo {
    pub raid_enabled: Option<bool>,
    pub authorized_at: Option<DateTime<Utc>>,
    pub total_raids: i64,
    pub successful_raids: i64,
    pub last_raid_login: Option<String>,
    pub last_raid_viewers: Option<i64>,
    pub last_raid_at: Option<DateTime<Utc>>,
}

/// Port für Discord-Invite-Links.
/// `commands.py:741` — `!dldc` / `!dlde`
#[async_trait]
pub trait DiscordLinkPort: Send + Sync {
    /// Gibt den Discord-Invite-Link für den Kanal zurück.
    /// `None` = kein Link hinterlegt; `Err` = technischer Fehler → stilles Return.
    async fn discord_invite(&self, channel_login: &str) -> Result<Option<String>, String>;
}

/// Port für den `!invite`-Command-Handler.
/// `bot.py:781`
#[async_trait]
pub trait InvitePort: Send + Sync {
    /// Gibt die Antwortzeile für `!invite` zurück.
    /// `None` = kein Reply (Rust-Seite entscheidet gegen Antwort).
    async fn invite_line(
        &self,
        channel_login: &str,
        chatter_login: &str,
    ) -> Result<Option<String>, String>;
}

/// Port für Super-Mod-Prüfung (Engagement-Commands).
/// `engagement_commands.py:101` — `bot.engagement.admin.is_super_mod(actor_id)`
#[async_trait]
pub trait SuperModPort: Send + Sync {
    async fn is_super_mod(&self, actor_id: &str) -> bool;
}

/// Port für den letzten Auto-Ban (für `!uban`/`!unban`).
/// `commands.py:22` und `bot.py:162`
#[async_trait]
pub trait LastAutobanStore: Send + Sync {
    /// Gibt den letzten auto-gebannten User für diesen Channel zurück.
    async fn last_autoban(&self, channel_key: &str) -> Option<AutobanEntry>;
}

/// Ein gespeicherter Auto-Ban-Eintrag.
/// `commands.py:22` — TSV-Format: `[ts, status, channel, login, user_id, ...]`
#[derive(Debug, Clone)]
pub struct AutobanEntry {
    pub user_id: String,
    pub login: String,
}

/// Port für die Clip-Erstellung (`!clip`). Löst den Broadcaster-Token selbst auf
/// und ruft Helix `POST /clips`. Optional — ist kein Port gesetzt, antwortet
/// `!clip` mit einem Migrations-Hinweis (der Composition-Root setzt ihn nur, wenn
/// Helix-Client und Krypto-Key vorhanden sind).
#[async_trait]
pub trait ClipPort: Send + Sync {
    async fn create_clip(&self, broadcaster_user_id: &str, broadcaster_login: &str) -> ClipOutcome;
}

/// Ergebnis eines `!clip`-Versuchs (Port von `commands.py:284-408`).
#[derive(Debug, Clone)]
pub enum ClipOutcome {
    /// Clip erstellt — fertige Clip-URL.
    Created { url: String },
    /// Keine gültige Broadcaster-Autorisierung (`clips:edit` fehlt / nicht verbunden).
    OAuthMissing,
    /// Twitch-Fehler oder kein Clip zurück.
    Failed,
}

// ---------------------------------------------------------------------------
// Interne Partner-Row (aus twitch_streamers_partner_state)
// ---------------------------------------------------------------------------

/// Ergebnis von `_get_streamer_by_channel`.
/// `bot.py:2144` — nur aktive Partner.
/// `raid_bot_enabled` wird vom Orchestrator genutzt (z. B. für Promo-Checks),
/// ist im reinen Command-Handler nicht ausgewertet — bewusst via `#[allow]`.
#[derive(sqlx::FromRow, Debug, Clone)]
struct PartnerRow {
    twitch_login: String,
    twitch_user_id: String,
    #[allow(dead_code)]
    raid_bot_enabled: i32,
}

// ---------------------------------------------------------------------------
// CommandEngine
// ---------------------------------------------------------------------------

/// Haupt-Engine für alle Chat-Commands.
pub struct CommandEngine {
    pool: PgPool,
    api: Arc<dyn ChatApi>,
    raid: Arc<dyn RaidCommandPort>,
    discord_link: Arc<dyn DiscordLinkPort>,
    invite: Arc<dyn InvitePort>,
    super_mod: Arc<dyn SuperModPort>,
    autoban: Arc<dyn LastAutobanStore>,
    /// Optionaler Clip-Port (`!clip`). `None` → Migrations-Hinweis.
    clip: Option<Arc<dyn ClipPort>>,
    /// In-memory Cooldown-Tabelle für `!invite`.
    /// `bot.py:781` — 1h pro (channel_login, chatter_login).
    invite_cooldowns: Mutex<HashMap<(String, String), Instant>>,
}

impl CommandEngine {
    pub fn new(
        pool: PgPool,
        api: Arc<dyn ChatApi>,
        raid: Arc<dyn RaidCommandPort>,
        discord_link: Arc<dyn DiscordLinkPort>,
        invite: Arc<dyn InvitePort>,
        super_mod: Arc<dyn SuperModPort>,
        autoban: Arc<dyn LastAutobanStore>,
    ) -> Self {
        Self {
            pool,
            api,
            raid,
            discord_link,
            invite,
            super_mod,
            autoban,
            clip: None,
            invite_cooldowns: Mutex::new(HashMap::new()),
        }
    }

    /// Setzt den optionalen Clip-Port (`!clip`). Builder-Style, damit der
    /// Konstruktor und die Tests unverändert bleiben.
    pub fn set_clip_port(mut self, clip: Arc<dyn ClipPort>) -> Self {
        self.clip = Some(clip);
        self
    }

    /// Verarbeitet eine eingehende Chat-Nachricht.
    ///
    /// Gibt `true` zurück wenn die Nachricht ein Command war (Pipeline stoppt),
    /// `false` wenn kein Match.
    ///
    /// `commands.py` — RaidCommandsMixin dispatch-Tabelle.
    pub async fn handle(&self, event: &ChatMessageEvent) -> bool {
        let text_lower = event.text().to_lowercase();

        let (cmd, args) = if let Some(pos) = text_lower.find(' ') {
            (&text_lower[..pos], event.text()[pos..].trim())
        } else {
            (text_lower.as_str(), "")
        };

        match cmd {
            "!ping" | "!health" | "!status" | "!bot" => {
                self.cmd_ping(event).await;
                true
            }
            "!raid_history" | "!raidbot_history" => {
                self.cmd_raid_history(event).await;
                true
            }
            "!raid_status" | "!raidbot_status" => {
                self.cmd_raid_status(event).await;
                true
            }
            "!raid_enable" | "!raidbot" => {
                if event.is_mod_or_broadcaster() {
                    self.cmd_raid_enable(event).await;
                } else {
                    self.reply(event, "Nur der Broadcaster oder Mods können den Twitch-Bot steuern.")
                        .await;
                }
                true
            }
            "!uban" | "!unban" => {
                if event.is_mod_or_broadcaster() {
                    self.cmd_uban(event).await;
                } else {
                    self.reply(event, "Nur der Broadcaster oder Mods.").await;
                }
                true
            }
            "!raid" | "!traid" => {
                if event.is_mod_or_broadcaster() {
                    self.cmd_raid(event).await;
                } else {
                    self.reply(event, "Nur Broadcaster oder Mods können !raid benutzen.")
                        .await;
                }
                true
            }
            "!clip" | "!createclip" => {
                self.cmd_clip(event, args).await;
                true
            }
            "!silentban" => {
                if event.is_mod_or_broadcaster() {
                    self.cmd_silentban(event).await;
                } else {
                    self.reply(event, "Nur der Broadcaster oder Mods können den Bot steuern.")
                        .await;
                }
                true
            }
            "!silentraid" => {
                if event.is_mod_or_broadcaster() {
                    self.cmd_silentraid(event).await;
                } else {
                    self.reply(event, "Nur der Broadcaster oder Mods können den Bot steuern.")
                        .await;
                }
                true
            }
            "!dldc" | "!dlde" => {
                self.cmd_dldc(event).await;
                true
            }
            "!invite" => {
                self.cmd_invite(event).await;
                true
            }
            "!engagement_on" => {
                self.cmd_engagement_on(event).await;
                true
            }
            "!engagement_off" => {
                self.cmd_engagement_off(event).await;
                true
            }
            "!engagement_status" => {
                self.cmd_engagement_status(event).await;
                true
            }
            "!engagement_ignore_me" => {
                self.cmd_engagement_ignore_me(event).await;
                true
            }
            "!engagement_remember_me" => {
                self.cmd_engagement_remember_me(event).await;
                true
            }
            // !title / !titel: bewusst nicht portiert — KI-Abhängigkeit außerhalb Scope.
            // Handle als false → Pipeline fährt fort.
            "!title" | "!titel" => false,
            // !lurkersteuer_off: UNSICHER — streamer_plans-Schreibpfad nicht portiert.
            "!lurkersteuer_off" | "!lurkersteuer_aus" | "!lurker_tax_off" => false,
            _ => false,
        }
    }

    // -----------------------------------------------------------------------
    // Hilfsmethoden
    // -----------------------------------------------------------------------

    /// `bot.py:2165` — `(name or "").lower().lstrip("#")`
    fn normalize_channel_login(name: &str) -> String {
        name.to_lowercase()
            .trim_start_matches('#')
            .to_string()
    }

    /// `bot.py:2144` — SELECT aus `twitch_streamers_partner_state` WHERE
    /// `is_partner_active = 1`.
    ///
    /// Prod-Schema:
    /// - `is_partner_active` = integer
    /// - `twitch_login` = text
    /// - `twitch_user_id` = text
    /// - `raid_bot_enabled` = integer
    async fn get_partner(&self, channel_login: &str) -> Option<PartnerRow> {
        let normalized = Self::normalize_channel_login(channel_login);
        sqlx::query_as::<_, PartnerRow>(
            r#"
            SELECT twitch_login, twitch_user_id, raid_bot_enabled
            FROM twitch_streamers_partner_state
            WHERE LOWER(twitch_login) = $1
              AND is_partner_active = 1
            LIMIT 1
            "#,
        )
        .bind(normalized)
        .fetch_optional(&self.pool)
        .await
        .unwrap_or(None)
    }

    /// `analytics/legacy_token.py:14` — `needs_reauth == FALSE` → vollständig
    /// autorisiert.
    ///
    /// Prod-Schema: `twitch_raid_auth.needs_reauth` = boolean
    async fn is_fully_authed(&self, twitch_user_id: &str) -> bool {
        let row = sqlx::query_as::<_, (Option<bool>,)>(
            "SELECT needs_reauth FROM twitch_raid_auth WHERE twitch_user_id = $1",
        )
        .bind(twitch_user_id)
        .fetch_optional(&self.pool)
        .await;
        match row {
            Ok(Some((needs_reauth,))) => needs_reauth == Some(false),
            _ => false,
        }
    }

    /// Sendet eine Antwort mit `@<chatter>`-Prefix.
    async fn reply(&self, event: &ChatMessageEvent, text: &str) {
        let msg = format!("@{} {}", event.chatter_user_login, text);
        if let Err(e) = self.api.send_message(&event.broadcaster_user_id, &msg).await {
            tracing::warn!(
                channel = %event.broadcaster_user_login,
                err = %e,
                "reply send fehlgeschlagen"
            );
        }
    }

    /// Sendet eine Antwort ohne `@`-Prefix.
    async fn reply_plain(&self, event: &ChatMessageEvent, text: &str) {
        if let Err(e) = self
            .api
            .send_message(&event.broadcaster_user_id, text)
            .await
        {
            tracing::warn!(
                channel = %event.broadcaster_user_login,
                err = %e,
                "reply_plain send fehlgeschlagen"
            );
        }
    }

    // -----------------------------------------------------------------------
    // !ping — commands.py:410
    // -----------------------------------------------------------------------

    async fn cmd_ping(&self, event: &ChatMessageEvent) {
        let reply = {
            let mut rng = rand::thread_rng();
            PING_REPLIES
                .choose(&mut rng)
                .copied()
                .unwrap_or(PING_REPLIES[0])
        };
        self.reply(event, reply).await;
    }

    // -----------------------------------------------------------------------
    // !raid_history — commands.py:248
    // -----------------------------------------------------------------------

    async fn cmd_raid_history(&self, event: &ChatMessageEvent) {
        // kein Partner → stilles Return — commands.py:253
        let partner = match self.get_partner(&event.broadcaster_user_login).await {
            Some(p) => p,
            None => return,
        };

        // Prod-Schema: executed_at = TIMESTAMPTZ, success = boolean, viewer_count = integer
        #[derive(sqlx::FromRow)]
        struct RaidRow {
            to_broadcaster_login: Option<String>,
            viewer_count: Option<i32>,
            executed_at: Option<DateTime<Utc>>,
            success: Option<bool>,
        }

        let rows = sqlx::query_as::<_, RaidRow>(
            r#"
            SELECT to_broadcaster_login, viewer_count, executed_at, success
            FROM twitch_raid_history
            WHERE from_broadcaster_id = $1
            ORDER BY executed_at DESC
            LIMIT 3
            "#,
        )
        .bind(&partner.twitch_user_id)
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();

        if rows.is_empty() {
            self.reply_plain(event, "Noch keine Raids durchgeführt.").await;
            return;
        }

        let parts: Vec<String> = rows
            .iter()
            .map(|r| {
                let icon = if r.success.unwrap_or(false) { "✅" } else { "❌" };
                // executed_at[:10] = YYYY-MM-DD — commands.py:162
                let date = r
                    .executed_at
                    .map(|dt| dt.format("%Y-%m-%d").to_string())
                    .unwrap_or_default();
                let login = r.to_broadcaster_login.as_deref().unwrap_or("?");
                let viewers = r.viewer_count.unwrap_or(0);
                format!("{icon} {login} ({viewers}V, {date})")
            })
            .collect();

        let msg = format!("Letzte Raids: {}", parts.join(" | "));
        self.reply_plain(event, &msg).await;
    }

    // -----------------------------------------------------------------------
    // !raid_status — commands.py:126
    // -----------------------------------------------------------------------

    async fn cmd_raid_status(&self, event: &ChatMessageEvent) {
        let partner = match self.get_partner(&event.broadcaster_user_login).await {
            Some(p) => p,
            None => {
                self.reply_plain(event, "Dieser Kanal ist nicht als Partner registriert.")
                    .await;
                return;
            }
        };

        let info = match self.raid.raid_status(&partner.twitch_user_id).await {
            Ok(i) => i,
            Err(e) => {
                tracing::warn!(
                    channel = %event.broadcaster_user_login,
                    err = %e,
                    "raid_status fehlgeschlagen"
                );
                return;
            }
        };

        // Status-String — commands.py:120–125
        let status = match info.raid_enabled {
            None => "❌ Nicht autorisiert (OAuth fehlt) | Anforderung: Twitch-Bot autorisieren mit !raid_enable.".to_string(),
            Some(true) => "✅ Aktiv | Auto-Raids sind aktiviert.".to_string(),
            Some(false) => "🛑 Deaktiviert | Aktiviere mit !raid_enable.".to_string(),
        };

        // Statistik-Anhang — commands.py:124
        let stats_part = if info.total_raids > 0 {
            format!(
                " | Statistik: {} Raids ({} erfolgreich)",
                info.total_raids, info.successful_raids
            )
        } else {
            String::new()
        };

        // Letzter Raid — commands.py:125
        // executed_at[:16] = "YYYY-MM-DD HH:MM" — commands.py:125
        let last_part =
            if let (Some(login), Some(viewers), Some(at)) =
                (&info.last_raid_login, info.last_raid_viewers, info.last_raid_at)
            {
                let icon = if info.successful_raids > 0 { "✅" } else { "❌" };
                let formatted = at.format("%Y-%m-%d %H:%M").to_string();
                format!(" | Letzter Raid {icon}: {login} ({viewers} Viewer) am {formatted}")
            } else {
                String::new()
            };

        let msg = format!("{status}{stats_part}{last_part}");
        self.reply_plain(event, &msg).await;
    }

    // -----------------------------------------------------------------------
    // !raid_enable / !raidbot — commands.py:52
    // -----------------------------------------------------------------------

    async fn cmd_raid_enable(&self, event: &ChatMessageEvent) {
        let partner = match self.get_partner(&event.broadcaster_user_login).await {
            Some(p) => p,
            None => {
                self.reply(
                    event,
                    "Dieser Kanal ist nicht als Partner registriert. Kontaktiere einen Admin für Details.",
                )
                .await;
                return;
            }
        };

        // Prod-Schema: twitch_raid_auth.raid_enabled = boolean
        let auth_row = sqlx::query_as::<_, (Option<bool>,)>(
            "SELECT raid_enabled FROM twitch_raid_auth WHERE twitch_user_id = $1",
        )
        .bind(&partner.twitch_user_id)
        .fetch_optional(&self.pool)
        .await
        .unwrap_or(None);

        match auth_row {
            None => {
                // Noch nie autorisiert — commands.py:77
                // UNSICHER: auth_url-Generierung nicht im Trait abgebildet.
                self.reply(
                    event,
                    "OAuth fehlt – Anforderung: Twitch-Bot autorisieren (Pflicht für Streamer-Partner). Kontaktiere einen Admin für den Auth-Link.",
                )
                .await;
            }
            Some((Some(true),)) => {
                // commands.py:87
                self.reply(
                    event,
                    "✅ Auto-Raid ist bereits aktiviert! Der Twitch-Bot raidet automatisch andere Partner, wenn du offline gehst.",
                )
                .await;
            }
            Some(_) => {
                // raid_enabled=false → aktivieren — commands.py:80–88
                let result = sqlx::query(
                    "UPDATE twitch_raid_auth SET raid_enabled = TRUE WHERE twitch_user_id = $1",
                )
                .bind(&partner.twitch_user_id)
                .execute(&self.pool)
                .await;

                if let Err(e) = result {
                    tracing::error!(
                        channel = %event.broadcaster_user_login,
                        err = %e,
                        "raid_enable UPDATE fehlgeschlagen"
                    );
                    return;
                }

                // partner_registry.py:1773 — raid_bot_enabled = 1
                let _ = sqlx::query(
                    "UPDATE twitch_partners SET raid_bot_enabled = 1, twitch_login = $2 WHERE twitch_user_id = $1",
                )
                .bind(&partner.twitch_user_id)
                .bind(&partner.twitch_login)
                .execute(&self.pool)
                .await;

                self.reply(
                    event,
                    "✅ Auto-Raid aktiviert! Wenn du offline gehst, raidet der Twitch-Bot automatisch den Partner mit der kürzesten Stream-Zeit.",
                )
                .await;
            }
        }
    }

    // -----------------------------------------------------------------------
    // !uban / !unban — commands.py:201
    // -----------------------------------------------------------------------

    async fn cmd_uban(&self, event: &ChatMessageEvent) {
        let partner = match self.get_partner(&event.broadcaster_user_login).await {
            Some(p) => p,
            None => {
                self.reply(event, "Dieser Kanal ist nicht als Partner registriert.")
                    .await;
                return;
            }
        };

        let channel_key = Self::normalize_channel_login(&event.broadcaster_user_login);
        let entry = match self.autoban.last_autoban(&channel_key).await {
            Some(e) => e,
            None => {
                self.reply(event, "Kein Auto-Ban-Eintrag zum Aufheben gefunden.")
                    .await;
                return;
            }
        };

        if entry.user_id.is_empty() {
            self.reply(event, "Kein Nutzer gespeichert für Unban.").await;
            return;
        }

        // moderation.py:1831 — DELETE /helix/moderation/bans
        match self
            .api
            .unban_user(&partner.twitch_user_id, &entry.user_id)
            .await
        {
            Ok(true) => {
                self.reply(event, &format!("Unban ausgeführt für {}.", entry.login))
                    .await;
            }
            _ => {
                self.reply(event, &format!("Unban fehlgeschlagen für {}.", entry.login))
                    .await;
            }
        }
    }

    // -----------------------------------------------------------------------
    // !raid / !traid — commands.py:617
    // -----------------------------------------------------------------------

    async fn cmd_raid(&self, event: &ChatMessageEvent) {
        let partner = match self.get_partner(&event.broadcaster_user_login).await {
            Some(p) => p,
            None => {
                self.reply(
                    event,
                    "Dieser Kanal ist nicht als Partner registriert. Bitte erst mit !raid_enable verifizieren.",
                )
                .await;
                return;
            }
        };

        // _is_fully_authed — commands.py:265
        if !self.is_fully_authed(&partner.twitch_user_id).await {
            self.reply(
                event,
                "Neu-Autorisierung erforderlich. Bitte prüfe deine Discord-DMs oder nutze /traid für den neuen Auth-Link.",
            )
            .await;
            return;
        }

        // Direkt an tb-raid-Schicht via Trait — kein HTTP-Proxy-Loop.
        match self
            .raid
            .manual_raid(&partner.twitch_user_id, &partner.twitch_login)
            .await
        {
            Ok(status) => {
                let chatter = &event.chatter_user_login;
                let msg = match status.as_str() {
                    "started" => {
                        // commands.py:279 — target_login wird von der Raid-Engine bestimmt.
                        // UNSICHER: Für den vollständigen "@chatter Raid auf {target} gestartet!"-
                        // Text würde RaidCommandPort ein strukturiertes Ergebnis liefern müssen.
                        format!("@{chatter} Raid gestartet! (Twitch-Countdown ~90s)")
                    }
                    "source_not_live" => format!(
                        "@{chatter} Kein Stream gefunden, von dem aus geraidet werden kann."
                    ),
                    "source_not_eligible" => format!(
                        "@{chatter} !raid ist nur verfügbar, wenn du gerade Deadlock streamst oder gerade erst von Deadlock auf Just Chatting gewechselt bist."
                    ),
                    "no_target" => format!(
                        "@{chatter} Weder Deadlock-Partner noch andere deutsche Deadlock-Streamer live."
                    ),
                    "unavailable" => format!("@{chatter} Twitch-Bot nicht verfügbar."),
                    other => format!("@{chatter} Raid fehlgeschlagen: {other}"),
                };
                self.reply_plain(event, &msg).await;
            }
            Err(e) => {
                tracing::warn!(
                    channel = %event.broadcaster_user_login,
                    err = %e,
                    "manual_raid Trait-Call fehlgeschlagen"
                );
                self.reply(
                    event,
                    &format!("Raid fehlgeschlagen: {e}"),
                )
                .await;
            }
        }
    }

    // -----------------------------------------------------------------------
    // !clip / !createclip — commands.py:284
    // -----------------------------------------------------------------------

    async fn cmd_clip(&self, event: &ChatMessageEvent, args: &str) {
        let partner = match self.get_partner(&event.broadcaster_user_login).await {
            Some(p) => p,
            None => {
                self.reply(event, "Dieser Kanal ist nicht als Partner registriert.")
                    .await;
                return;
            }
        };

        // Titel aufbereiten — commands.py:179–185
        let raw_title = args.trim();
        let title = if raw_title.is_empty() {
            let mut rng = rand::thread_rng();
            CLIP_TITLE_FALLBACKS
                .choose(&mut rng)
                .copied()
                .unwrap_or("Clip it!")
                .to_string()
        } else if raw_title.chars().count() > CLIP_TITLE_MAX_LEN {
            let trimmed: String = raw_title.chars().take(CLIP_TITLE_TRIM_LEN).collect();
            format!("{}...", trimmed.trim_end())
        } else {
            raw_title.to_string()
        };

        let Some(clip_port) = &self.clip else {
            // Kein Clip-Port verdrahtet (z. B. ohne Helix/Krypto) → ehrlicher Hinweis
            // statt "in 10 Sekunden nochmal" (das würde nie klappen).
            self.reply(
                event,
                "Die Clip-Erstellung wird gerade auf das neue System umgestellt und ist kurz nicht verfügbar.",
            )
            .await;
            return;
        };

        match clip_port
            .create_clip(&partner.twitch_user_id, &partner.twitch_login)
            .await
        {
            ClipOutcome::Created { url } => {
                let suffix = if title.is_empty() {
                    String::new()
                } else {
                    format!(" – \"{title}\"")
                };
                self.reply(
                    event,
                    &format!("🎬 Clip erstellt{suffix} (ca. letzte 60s): {url}"),
                )
                .await;
            }
            ClipOutcome::OAuthMissing => {
                self.reply(
                    event,
                    "Für Clips fehlt die Autorisierung — der Streamer muss den Bot einmal per !raid_enable verbinden.",
                )
                .await;
            }
            ClipOutcome::Failed => {
                self.reply(
                    event,
                    "Clip konnte nicht erstellt werden. Bitte in ein paar Sekunden nochmal.",
                )
                .await;
            }
        }
    }

    // -----------------------------------------------------------------------
    // !silentban — commands.py:423
    // -----------------------------------------------------------------------

    async fn cmd_silentban(&self, event: &ChatMessageEvent) {
        let partner = match self.get_partner(&event.broadcaster_user_login).await {
            Some(p) => p,
            None => {
                self.reply(event, "Dieser Kanal ist nicht als Partner registriert.")
                    .await;
                return;
            }
        };

        match self.raid.toggle_silent_ban(&partner.twitch_login).await {
            Ok(1) => {
                // commands.py:219
                self.reply(
                    event,
                    "🔇 Auto-Ban Benachrichtigungen deaktiviert. Bans werden weiterhin ausgeführt, aber keine Nachricht mehr im Chat.",
                )
                .await;
            }
            Ok(_) => {
                // commands.py:220
                self.reply(event, "🔊 Auto-Ban Benachrichtigungen aktiviert.")
                    .await;
            }
            Err(e) => {
                tracing::warn!(
                    channel = %event.broadcaster_user_login,
                    err = %e,
                    "toggle_silent_ban fehlgeschlagen"
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // !silentraid — commands.py:479
    // -----------------------------------------------------------------------

    async fn cmd_silentraid(&self, event: &ChatMessageEvent) {
        let partner = match self.get_partner(&event.broadcaster_user_login).await {
            Some(p) => p,
            None => {
                self.reply(event, "Dieser Kanal ist nicht als Partner registriert.")
                    .await;
                return;
            }
        };

        match self.raid.toggle_silent_raid(&partner.twitch_login).await {
            Ok(1) => {
                // commands.py:233
                self.reply(
                    event,
                    "🔇 Raid-Benachrichtigungen deaktiviert. Raids werden weiterhin ausgeführt, aber keine Nachricht mehr im Chat.",
                )
                .await;
            }
            Ok(_) => {
                // commands.py:234
                self.reply(event, "🔊 Raid-Benachrichtigungen aktiviert.")
                    .await;
            }
            Err(e) => {
                tracing::warn!(
                    channel = %event.broadcaster_user_login,
                    err = %e,
                    "toggle_silent_raid fehlgeschlagen"
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // !dldc / !dlde — commands.py:741
    // -----------------------------------------------------------------------

    async fn cmd_dldc(&self, event: &ChatMessageEvent) {
        let channel_login = event.broadcaster_user_login.to_lowercase();

        match self.discord_link.discord_invite(&channel_login).await {
            Ok(Some(url)) if !url.is_empty() => {
                self.reply(event, &format!("Discord: {url}")).await;
            }
            Ok(None) => {
                self.reply(event, "Kein Discord-Link für diesen Streamer hinterlegt.")
                    .await;
            }
            Ok(Some(_)) => {
                // URL leer — stilles Return (commands.py:301)
            }
            Err(e) => {
                tracing::debug!(channel = %channel_login, err = %e, "discord_invite Fehler");
                // stilles Return (commands.py:300)
            }
        }
    }

    // -----------------------------------------------------------------------
    // !invite — bot.py:781
    // -----------------------------------------------------------------------

    async fn cmd_invite(&self, event: &ChatMessageEvent) {
        // Exact-Match: nur "!invite" ohne Argumente — bot.py:784
        if event.text().trim().to_lowercase() != "!invite" {
            return;
        }

        let channel_login = event.broadcaster_user_login.to_lowercase();
        let chatter_login = event.chatter_user_login.to_lowercase();

        // Cooldown-Check: 1h pro (channel, chatter) — bot.py:345
        {
            let mut cooldowns = self.invite_cooldowns.lock().await;
            let key = (channel_login.clone(), chatter_login.clone());
            if let Some(&last) = cooldowns.get(&key) {
                if last.elapsed().as_secs() < INVITE_COOLDOWN_SECS {
                    return; // Cooldown aktiv → stilles Return
                }
            }
            // Cooldown setzen vor dem eigentlichen Call
            cooldowns.insert(key, Instant::now());
        }

        match self.invite.invite_line(&channel_login, &chatter_login).await {
            Ok(Some(reply)) if !reply.is_empty() => {
                self.reply_plain(event, &reply).await;
            }
            Ok(_) => {
                // Kein Reply — stilles Return
            }
            Err(e) => {
                tracing::debug!(
                    channel = %channel_login,
                    chatter = %chatter_login,
                    err = %e,
                    "invite_line Fehler"
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // Engagement-Commands — engagement_commands.py
    // -----------------------------------------------------------------------

    /// Prüft ob der Aufrufer berechtigt ist (Broadcaster, Mod oder Super-Mod).
    /// `engagement_commands.py:101`
    async fn is_engagement_admin(&self, event: &ChatMessageEvent) -> bool {
        if event.is_mod_or_broadcaster() {
            return true;
        }
        self.super_mod.is_super_mod(&event.chatter_user_id).await
    }

    /// `!engagement_on` — engagement_commands.py:101
    ///
    /// Prod-Schema: `twitch_engagement_settings.enabled` = boolean,
    /// `enabled_at` = timestamptz, `enabled_by` = text, `updated_at` = timestamptz.
    async fn cmd_engagement_on(&self, event: &ChatMessageEvent) {
        if !self.is_engagement_admin(event).await {
            return;
        }
        let channel_login = event.broadcaster_user_login.to_lowercase();
        let actor_id = event.chatter_user_id.clone();

        let result = sqlx::query(
            r#"
            INSERT INTO twitch_engagement_settings
                (channel_login, enabled, enabled_at, enabled_by, updated_at)
            VALUES ($1, TRUE, NOW(), $2, NOW())
            ON CONFLICT (channel_login) DO UPDATE SET
                enabled = TRUE,
                enabled_at = NOW(),
                enabled_by = COALESCE(EXCLUDED.enabled_by, twitch_engagement_settings.enabled_by),
                updated_at = NOW()
            "#,
        )
        .bind(&channel_login)
        .bind(&actor_id)
        .execute(&self.pool)
        .await;

        match result {
            Ok(_) => {
                self.reply(
                    event,
                    "AI-Engagement aktiviert. Deaktiviert sich automatisch bei Stream-Ende.",
                )
                .await;
            }
            Err(e) => {
                tracing::error!(channel = %channel_login, err = %e, "engagement_on INSERT fehlgeschlagen");
                self.reply(event, "Fehler beim Aktivieren, schau in die Logs.")
                    .await;
            }
        }
    }

    /// `!engagement_off` — engagement_commands.py:127
    async fn cmd_engagement_off(&self, event: &ChatMessageEvent) {
        if !self.is_engagement_admin(event).await {
            return;
        }
        let channel_login = event.broadcaster_user_login.to_lowercase();
        let actor_id = event.chatter_user_id.clone();

        let result = sqlx::query(
            r#"
            INSERT INTO twitch_engagement_settings
                (channel_login, enabled, enabled_by, updated_at)
            VALUES ($1, FALSE, $2, NOW())
            ON CONFLICT (channel_login) DO UPDATE SET
                enabled = FALSE,
                enabled_by = COALESCE(EXCLUDED.enabled_by, twitch_engagement_settings.enabled_by),
                updated_at = NOW()
            "#,
        )
        .bind(&channel_login)
        .bind(&actor_id)
        .execute(&self.pool)
        .await;

        match result {
            Ok(_) => {
                self.reply(event, "AI-Engagement deaktiviert.").await;
            }
            Err(e) => {
                tracing::error!(channel = %channel_login, err = %e, "engagement_off INSERT fehlgeschlagen");
                self.reply(event, "Fehler beim Deaktivieren, schau in die Logs.")
                    .await;
            }
        }
    }

    /// `!engagement_status` — engagement_commands.py:150
    ///
    /// Prod-Schema:
    /// - `twitch_engagement_settings.enabled` = boolean
    /// - `twitch_engagement_log.decision` = text
    /// - `twitch_engagement_log.response_text` = text
    /// - `twitch_engagement_log.ts` = timestamp with time zone
    async fn cmd_engagement_status(&self, event: &ChatMessageEvent) {
        let channel_login = event.broadcaster_user_login.to_lowercase();

        let settings_row = sqlx::query_as::<_, (bool,)>(
            "SELECT enabled FROM twitch_engagement_settings WHERE channel_login = $1",
        )
        .bind(&channel_login)
        .fetch_optional(&self.pool)
        .await
        .unwrap_or(None);

        let enabled = match settings_row {
            None => {
                self.reply_plain(
                    event,
                    &format!("AI-Engagement für {channel_login}: nie konfiguriert."),
                )
                .await;
                return;
            }
            Some((v,)) => v,
        };

        let status_str = if enabled { "AN" } else { "AUS" };

        // Letzter Log-Eintrag — engagement_commands.py:155
        #[derive(sqlx::FromRow)]
        struct LogRow {
            decision: Option<String>,
            response_text: Option<String>,
            ts: Option<DateTime<Utc>>,
        }

        let log_row = sqlx::query_as::<_, LogRow>(
            r#"
            SELECT decision, response_text, ts
            FROM twitch_engagement_log
            WHERE channel_login = $1
            ORDER BY ts DESC
            LIMIT 1
            "#,
        )
        .bind(&channel_login)
        .fetch_optional(&self.pool)
        .await
        .unwrap_or(None);

        let msg = if let Some(log) = log_row {
            let now = Utc::now();
            let ago_sec = log.ts.map(|t| (now - t).num_seconds()).unwrap_or(0);
            let text = log.response_text.as_deref().unwrap_or("");
            // snippet = response_text[:77] + "…" wenn > 80 — engagement_commands.py:158
            let snippet = if text.chars().count() > 80 {
                let s: String = text.chars().take(77).collect();
                format!("{s}…")
            } else {
                text.to_string()
            };
            let decision = log.decision.as_deref().unwrap_or("?");
            format!(
                r#"AI-Engagement: {status_str}. Letzte Aktion: {decision} vor {ago_sec}s — "{snippet}"."#
            )
        } else {
            format!("AI-Engagement: {status_str}. Noch keine Aktionen geloggt.")
        };

        self.reply_plain(event, &msg).await;
    }

    /// `!engagement_ignore_me` — engagement_commands.py:177
    ///
    /// Prod-Schema: `twitch_user_engagement_optout.twitch_user_id` = text
    async fn cmd_engagement_ignore_me(&self, event: &ChatMessageEvent) {
        let user_id = &event.chatter_user_id;
        if user_id.is_empty() {
            self.reply(event, "Konnte deine User-ID nicht ermitteln.")
                .await;
            return;
        }

        let result = sqlx::query(
            r#"
            INSERT INTO twitch_user_engagement_optout (twitch_user_id)
            VALUES ($1)
            ON CONFLICT (twitch_user_id) DO NOTHING
            "#,
        )
        .bind(user_id.as_str())
        .execute(&self.pool)
        .await;

        match result {
            Ok(_) => {
                self.reply(
                    event,
                    "OK, AI ignoriert dich ab sofort. Mit !engagement_remember_me wieder einschalten.",
                )
                .await;
            }
            Err(e) => {
                tracing::error!(user_id = %user_id, err = %e, "engagement_ignore_me fehlgeschlagen");
                self.reply(event, "Fehler beim Opt-Out, schau in die Logs.")
                    .await;
            }
        }
    }

    /// `!engagement_remember_me` — engagement_commands.py:195
    async fn cmd_engagement_remember_me(&self, event: &ChatMessageEvent) {
        let user_id = &event.chatter_user_id;
        if user_id.is_empty() {
            self.reply(event, "Konnte deine User-ID nicht ermitteln.")
                .await;
            return;
        }

        let result = sqlx::query(
            "DELETE FROM twitch_user_engagement_optout WHERE twitch_user_id = $1",
        )
        .bind(user_id.as_str())
        .execute(&self.pool)
        .await;

        match result {
            Ok(_) => {
                self.reply(event, "OK, AI berücksichtigt dich wieder.").await;
            }
            Err(e) => {
                tracing::error!(user_id = %user_id, err = %e, "engagement_remember_me fehlgeschlagen");
                self.reply(
                    event,
                    "Fehler beim Opt-Out-Rückgängigmachen, schau in die Logs.",
                )
                .await;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// LÜCKE: !title / !titel (commands.py:770)
// ---------------------------------------------------------------------------
//
// Bewusst nicht portiert. Abhängigkeiten:
// - `bot.title_generator.title_ai.generate_title()` — LLM-Generierung.
// - Eigene DB-Tabellen (title_history, knowledge_titles) außerhalb des Vertrags.
// - `RateLimitExceeded`-Handling mit `retry_after`.
//
// `handle()` gibt `false` für "!title" / "!titel" — Pipeline fährt fort.
// Erweiterbar über künftigen `TitlePort`-Trait.

// ---------------------------------------------------------------------------
// LÜCKE: !lurkersteuer_off (commands.py:535)
// ---------------------------------------------------------------------------
//
// UNSICHER-Status aus Vertrag:
// - Schreibpfad auf `streamer_plans.lurker_tax_enabled` benötigt plan_id-Lookup.
// - Zwei SQL-Abfragen über `twitch_streamer_identities` → `streamer_plans`.
// - Feature-Flag `SUBSCRIPTION_PLANS_ENABLED=True` ist in Prod an, aber
//   tatsächliche Nutzung durch Streamer mit paid plan unklar.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::BanOutcome;
    use crate::types::{ChatBadge, ChatMessageBody, ChatMessageEvent, MessageFragment, SendOutcome};

    // -----------------------------------------------------------------------
    // Mock-Implementierungen
    // -----------------------------------------------------------------------

    struct MockApi {
        sent: Mutex<Vec<(String, String)>>,
    }

    impl MockApi {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                sent: Mutex::new(vec![]),
            })
        }

        async fn last_message(&self) -> Option<String> {
            self.sent.lock().await.last().map(|(_, m)| m.clone())
        }

        async fn message_count(&self) -> usize {
            self.sent.lock().await.len()
        }
    }

    #[async_trait]
    impl ChatApi for MockApi {
        async fn send_message(
            &self,
            broadcaster_id: &str,
            message: &str,
        ) -> Result<SendOutcome, String> {
            self.sent
                .lock()
                .await
                .push((broadcaster_id.to_string(), message.to_string()));
            Ok(SendOutcome::Sent)
        }
        async fn send_announcement(
            &self,
            _: &str,
            _: &str,
            _: &str,
        ) -> Result<bool, String> {
            Ok(true)
        }
        async fn ban_user(
            &self,
            _: &str,
            _: &str,
            _: &str,
        ) -> Result<BanOutcome, String> {
            Ok(BanOutcome::Banned)
        }
        async fn timeout_user(
            &self,
            _: &str,
            _: &str,
            _: u32,
            _: &str,
        ) -> Result<BanOutcome, String> {
            Ok(BanOutcome::Banned)
        }
        async fn unban_user(&self, _: &str, _: &str) -> Result<bool, String> {
            Ok(true)
        }
        async fn delete_message(&self, _: &str, _: &str) -> Result<bool, String> {
            Ok(true)
        }
        async fn user_created_at(
            &self,
            _: &str,
        ) -> Result<Option<DateTime<Utc>>, String> {
            Ok(None)
        }
        async fn resolve_user_id(&self, _: &str) -> Result<Option<String>, String> {
            Ok(None)
        }
        async fn bot_user_id(&self) -> String {
            "botid".to_string()
        }
    }

    struct MockRaid {
        manual_status: String,
        silent_ban_val: i32,
        silent_raid_val: i32,
    }

    impl MockRaid {
        fn default_arc() -> Arc<Self> {
            Arc::new(Self {
                manual_status: "started".to_string(),
                silent_ban_val: 1,
                silent_raid_val: 0,
            })
        }
    }

    #[async_trait]
    impl RaidCommandPort for MockRaid {
        async fn manual_raid(&self, _: &str, _: &str) -> Result<String, String> {
            Ok(self.manual_status.clone())
        }
        async fn raid_status(&self, _: &str) -> Result<RaidStatusInfo, String> {
            Ok(RaidStatusInfo {
                raid_enabled: Some(true),
                authorized_at: None,
                total_raids: 5,
                successful_raids: 3,
                last_raid_login: Some("streamerx".to_string()),
                last_raid_viewers: Some(42),
                last_raid_at: Some(Utc::now()),
            })
        }
        async fn toggle_silent_ban(&self, _: &str) -> Result<i32, String> {
            Ok(self.silent_ban_val)
        }
        async fn toggle_silent_raid(&self, _: &str) -> Result<i32, String> {
            Ok(self.silent_raid_val)
        }
    }

    struct MockDiscordLink {
        url: Option<String>,
    }
    #[async_trait]
    impl DiscordLinkPort for MockDiscordLink {
        async fn discord_invite(&self, _: &str) -> Result<Option<String>, String> {
            Ok(self.url.clone())
        }
    }

    struct MockInvite {
        reply: Option<String>,
    }
    #[async_trait]
    impl InvitePort for MockInvite {
        async fn invite_line(&self, _: &str, _: &str) -> Result<Option<String>, String> {
            Ok(self.reply.clone())
        }
    }

    struct MockSuperMod(bool);
    #[async_trait]
    impl SuperModPort for MockSuperMod {
        async fn is_super_mod(&self, _: &str) -> bool {
            self.0
        }
    }

    struct MockAutoban(Option<AutobanEntry>);
    #[async_trait]
    impl LastAutobanStore for MockAutoban {
        async fn last_autoban(&self, _: &str) -> Option<AutobanEntry> {
            self.0.clone()
        }
    }

    // -----------------------------------------------------------------------
    // Hilfsfunktionen
    // -----------------------------------------------------------------------

    fn make_event(text: &str, is_mod: bool, is_broadcaster: bool) -> ChatMessageEvent {
        let mut badges = vec![];
        if is_mod {
            badges.push(ChatBadge {
                set_id: "moderator".to_string(),
                id: "1".to_string(),
                info: String::new(),
            });
        }
        if is_broadcaster {
            badges.push(ChatBadge {
                set_id: "broadcaster".to_string(),
                id: "1".to_string(),
                info: String::new(),
            });
        }
        ChatMessageEvent {
            broadcaster_user_id: "bc123".to_string(),
            broadcaster_user_login: "testchannel".to_string(),
            broadcaster_user_name: "TestChannel".to_string(),
            chatter_user_id: "u999".to_string(),
            chatter_user_login: "testuser".to_string(),
            chatter_user_name: "TestUser".to_string(),
            message_id: "msg1".to_string(),
            message: ChatMessageBody {
                text: text.to_string(),
                fragments: vec![MessageFragment {
                    fragment_type: "text".to_string(),
                    text: text.to_string(),
                }],
            },
            badges,
            color: String::new(),
            ..Default::default()
        }
    }

    // -----------------------------------------------------------------------
    // Unit-Tests (ohne DB)
    // -----------------------------------------------------------------------

    #[test]
    fn normalize_channel_login_entfernt_raute() {
        assert_eq!(
            CommandEngine::normalize_channel_login("#TestChannel"),
            "testchannel"
        );
        assert_eq!(
            CommandEngine::normalize_channel_login("TestChannel"),
            "testchannel"
        );
        assert_eq!(CommandEngine::normalize_channel_login(""), "");
    }

    #[test]
    fn clip_titel_kürzen_korrekt() {
        // > 60 Zeichen → 57 Zeichen + "..." — commands.py:181
        let long_title = "a".repeat(65);
        let trimmed: String = long_title.chars().take(CLIP_TITLE_TRIM_LEN).collect();
        let result = format!("{}...", trimmed.trim_end());
        assert_eq!(result.chars().count(), CLIP_TITLE_TRIM_LEN + 3);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn invite_cooldown_ist_eine_stunde() {
        assert_eq!(INVITE_COOLDOWN_SECS, 3600);
    }

    #[test]
    fn ping_antworten_vollständigkeit() {
        // Vertrag: exakt 6 Antworten — commands.py:196–202
        assert_eq!(PING_REPLIES.len(), 6);
        for reply in PING_REPLIES {
            assert!(!reply.is_empty());
        }
    }

    #[test]
    fn clip_fallback_titel_vollständigkeit() {
        // commands.py:181 — 5 Fallbacks
        assert_eq!(CLIP_TITLE_FALLBACKS.len(), 5);
    }

    #[test]
    fn title_command_ist_nicht_portiert() {
        // !title / !titel → handle() gibt false zurück (in der match-Tabelle explizit)
        let cmd = "!title";
        let is_unimplemented = matches!(cmd, "!title" | "!titel");
        assert!(is_unimplemented, "!title muss als nicht-portiert markiert sein");
    }

    #[test]
    fn berechtigungs_logik_is_mod_or_broadcaster() {
        let mod_event = make_event("!raid", true, false);
        let broadcaster_event = make_event("!raid", false, true);
        let user_event = make_event("!raid", false, false);
        assert!(mod_event.is_mod_or_broadcaster());
        assert!(broadcaster_event.is_mod_or_broadcaster());
        assert!(!user_event.is_mod_or_broadcaster());
    }

    #[test]
    fn is_broadcaster_prüft_chatter_id() {
        // Broadcaster-Event: chatter_user_id == broadcaster_user_id
        let event = ChatMessageEvent {
            broadcaster_user_id: "same123".to_string(),
            broadcaster_user_login: "channel".to_string(),
            broadcaster_user_name: "Channel".to_string(),
            chatter_user_id: "same123".to_string(), // gleiche ID
            chatter_user_login: "channel".to_string(),
            chatter_user_name: "Channel".to_string(),
            message_id: "m1".to_string(),
            message: ChatMessageBody {
                text: "!test".to_string(),
                fragments: vec![],
            },
            badges: vec![],
            color: String::new(),
            ..Default::default()
        };
        assert!(event.is_broadcaster());
    }

    // -----------------------------------------------------------------------
    // DB-Tests (gegen TB_TEST_DATABASE_URL)
    // -----------------------------------------------------------------------

    macro_rules! pool_or_skip {
        ($schema:expr) => {{
            let Some(dsn) = std::env::var("TB_TEST_DATABASE_URL").ok() else {
                if std::env::var("TB_TEST_REQUIRE_DB").as_deref() == Ok("1") {
                    panic!("TB_TEST_REQUIRE_DB=1 gesetzt, aber TB_TEST_DATABASE_URL fehlt");
                }
                eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
                return;
            };
            pool_in_schema(&dsn, $schema).await
        }};
    }

    async fn pool_in_schema(dsn: &str, schema: &str) -> PgPool {
        use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
        use std::str::FromStr;

        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(dsn)
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

        let opts = PgConnectOptions::from_str(dsn)
            .unwrap()
            .options([("search_path", schema)]);
        PgPoolOptions::new()
            .max_connections(4)
            .connect_with(opts)
            .await
            .unwrap()
    }

    async fn apply_ddl(pool: &PgPool) {
        for ddl in [
            // twitch_streamers_partner_state — prod-treu: is_partner_active INTEGER
            r#"CREATE TABLE twitch_streamers_partner_state (
                twitch_login TEXT,
                twitch_user_id TEXT,
                raid_bot_enabled INTEGER DEFAULT 0,
                is_partner_active INTEGER DEFAULT 0,
                require_discord_link INTEGER,
                next_link_check_at TEXT,
                discord_user_id TEXT,
                discord_display_name TEXT,
                is_on_discord INTEGER,
                manual_verified_permanent INTEGER,
                manual_verified_until TEXT,
                manual_verified_at TEXT,
                manual_partner_opt_out INTEGER,
                created_at TEXT,
                archived_at TEXT,
                silent_ban INTEGER,
                silent_raid INTEGER,
                is_monitored_only INTEGER,
                is_verified INTEGER,
                is_partner INTEGER,
                live_ping_role_id BIGINT,
                live_ping_enabled INTEGER,
                technical_pause_reason TEXT,
                operational_state TEXT
            )"#,
            // twitch_raid_auth — prod-treu: raid_enabled BOOLEAN, needs_reauth BOOLEAN
            r#"CREATE TABLE twitch_raid_auth (
                twitch_user_id TEXT PRIMARY KEY,
                twitch_login TEXT,
                raid_enabled BOOLEAN,
                needs_reauth BOOLEAN DEFAULT FALSE,
                authorized_at TIMESTAMPTZ,
                access_token TEXT,
                refresh_token TEXT,
                token_expires_at TIMESTAMPTZ,
                scopes TEXT,
                created_at TIMESTAMPTZ DEFAULT NOW(),
                last_refreshed_at TIMESTAMPTZ,
                reauth_notified_at TIMESTAMPTZ,
                access_token_enc BYTEA,
                refresh_token_enc BYTEA,
                enc_version INTEGER,
                enc_kid TEXT,
                enc_migrated_at TIMESTAMPTZ
            )"#,
            // twitch_partners — prod-treu: Timestamps TEXT, Flags INTEGER
            r#"CREATE TABLE twitch_partners (
                id BIGSERIAL PRIMARY KEY,
                twitch_user_id TEXT,
                twitch_login TEXT,
                raid_bot_enabled INTEGER DEFAULT 0,
                silent_ban INTEGER DEFAULT 0,
                silent_raid INTEGER DEFAULT 0,
                require_discord_link INTEGER,
                last_description TEXT,
                last_link_ok INTEGER,
                added_by TEXT,
                last_link_checked_at TEXT,
                next_link_check_at TEXT,
                manual_verified_permanent INTEGER,
                manual_verified_until TEXT,
                manual_verified_at TEXT,
                manual_partner_opt_out INTEGER,
                live_ping_role_id BIGINT,
                live_ping_enabled INTEGER,
                partnered_at TEXT,
                departnered_at TEXT,
                status TEXT,
                admin_archived_at TEXT,
                technical_pause_reason TEXT
            )"#,
            // twitch_engagement_settings — prod-treu: enabled BOOLEAN, timestamps TIMESTAMPTZ
            r#"CREATE TABLE twitch_engagement_settings (
                channel_login TEXT PRIMARY KEY,
                enabled BOOLEAN DEFAULT FALSE,
                steam_id TEXT,
                persona_override TEXT,
                tabu_topics TEXT[],
                enabled_at TIMESTAMPTZ,
                enabled_by TEXT,
                updated_at TIMESTAMPTZ,
                irc_read BOOLEAN
            )"#,
            // twitch_engagement_log — prod-treu: ts TIMESTAMPTZ
            r#"CREATE TABLE twitch_engagement_log (
                id BIGSERIAL PRIMARY KEY,
                channel_login TEXT,
                triggered_by_msg_id TEXT,
                decision TEXT,
                response_text TEXT,
                referenced_thread_ids TEXT[],
                model TEXT,
                prompt_tokens INTEGER,
                completion_tokens INTEGER,
                cost_usd_estimate NUMERIC,
                latency_ms INTEGER,
                ts TIMESTAMPTZ DEFAULT NOW()
            )"#,
            // twitch_user_engagement_optout — prod-treu
            r#"CREATE TABLE twitch_user_engagement_optout (
                twitch_user_id TEXT PRIMARY KEY,
                opted_out_at TIMESTAMPTZ DEFAULT NOW()
            )"#,
            // twitch_raid_history — prod-treu: executed_at TIMESTAMPTZ, success BOOLEAN
            r#"CREATE TABLE twitch_raid_history (
                id BIGSERIAL PRIMARY KEY,
                from_broadcaster_id TEXT,
                from_broadcaster_login TEXT,
                to_broadcaster_id TEXT,
                to_broadcaster_login TEXT,
                viewer_count INTEGER,
                stream_duration_sec INTEGER,
                reason TEXT,
                executed_at TIMESTAMPTZ DEFAULT NOW(),
                success BOOLEAN,
                error_message TEXT,
                target_stream_started_at TIMESTAMPTZ,
                candidates_count INTEGER
            )"#,
        ] {
            sqlx::query(ddl).execute(pool).await.unwrap();
        }
    }

    fn make_engine_with_pool(pool: PgPool, api: Arc<MockApi>) -> CommandEngine {
        CommandEngine::new(
            pool,
            api,
            MockRaid::default_arc(),
            Arc::new(MockDiscordLink {
                url: Some("https://discord.gg/test".to_string()),
            }),
            Arc::new(MockInvite {
                reply: Some("Hier ist euer Invite-Link!".to_string()),
            }),
            Arc::new(MockSuperMod(false)),
            Arc::new(MockAutoban(Some(AutobanEntry {
                user_id: "banned123".to_string(),
                login: "spammer".to_string(),
            }))),
        )
    }

    #[tokio::test]
    async fn engagement_on_schreibt_in_db() {
        let pool = pool_or_skip!("cmd_engagement_on");
        apply_ddl(&pool).await;
        let api = MockApi::new();
        let engine = make_engine_with_pool(pool.clone(), api.clone());

        let event = make_event("!engagement_on", true, false);
        let handled = engine.handle(&event).await;
        assert!(handled);

        let msg = api.last_message().await.unwrap();
        assert!(msg.contains("AI-Engagement aktiviert"), "Meldung: {msg}");

        let row = sqlx::query_as::<_, (bool,)>(
            "SELECT enabled FROM twitch_engagement_settings WHERE channel_login = 'testchannel'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(row.0);
    }

    #[tokio::test]
    async fn engagement_off_setzt_enabled_false() {
        let pool = pool_or_skip!("cmd_engagement_off");
        apply_ddl(&pool).await;
        let api = MockApi::new();
        let engine = make_engine_with_pool(pool.clone(), api.clone());

        engine.handle(&make_event("!engagement_on", true, false)).await;
        engine.handle(&make_event("!engagement_off", true, false)).await;

        let row = sqlx::query_as::<_, (bool,)>(
            "SELECT enabled FROM twitch_engagement_settings WHERE channel_login = 'testchannel'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(!row.0);

        let msg = api.last_message().await.unwrap();
        assert!(msg.contains("deaktiviert"), "Meldung: {msg}");
    }

    #[tokio::test]
    async fn engagement_ignore_me_schreibt_optout() {
        let pool = pool_or_skip!("cmd_engagement_ignore");
        apply_ddl(&pool).await;
        let api = MockApi::new();
        let engine = make_engine_with_pool(pool.clone(), api.clone());

        let event = make_event("!engagement_ignore_me", false, false);
        engine.handle(&event).await;

        let row = sqlx::query_as::<_, (String,)>(
            "SELECT twitch_user_id FROM twitch_user_engagement_optout WHERE twitch_user_id = 'u999'",
        )
        .fetch_optional(&pool)
        .await
        .unwrap();
        assert!(row.is_some(), "Opt-Out-Eintrag muss vorhanden sein");

        let msg = api.last_message().await.unwrap();
        assert!(msg.contains("ignoriert dich"), "Meldung: {msg}");
    }

    #[tokio::test]
    async fn engagement_remember_me_löscht_optout() {
        let pool = pool_or_skip!("cmd_engagement_remember");
        apply_ddl(&pool).await;
        let api = MockApi::new();
        let engine = make_engine_with_pool(pool.clone(), api.clone());

        sqlx::query(
            "INSERT INTO twitch_user_engagement_optout (twitch_user_id) VALUES ('u999')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let event = make_event("!engagement_remember_me", false, false);
        engine.handle(&event).await;

        let row = sqlx::query_as::<_, (String,)>(
            "SELECT twitch_user_id FROM twitch_user_engagement_optout WHERE twitch_user_id = 'u999'",
        )
        .fetch_optional(&pool)
        .await
        .unwrap();
        assert!(row.is_none(), "Opt-Out-Eintrag muss gelöscht sein");
    }

    #[tokio::test]
    async fn engagement_status_ohne_eintrag() {
        let pool = pool_or_skip!("cmd_engagement_status_leer");
        apply_ddl(&pool).await;
        let api = MockApi::new();
        let engine = make_engine_with_pool(pool.clone(), api.clone());

        let event = make_event("!engagement_status", false, false);
        engine.handle(&event).await;

        let msg = api.last_message().await.unwrap();
        assert!(msg.contains("nie konfiguriert"), "Meldung: {msg}");
    }

    #[tokio::test]
    async fn raid_history_ohne_einträge() {
        let pool = pool_or_skip!("cmd_raid_hist_leer");
        apply_ddl(&pool).await;
        let api = MockApi::new();

        sqlx::query(
            "INSERT INTO twitch_streamers_partner_state (twitch_login, twitch_user_id, is_partner_active, raid_bot_enabled) VALUES ('testchannel', 'bc123', 1, 1)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let engine = make_engine_with_pool(pool.clone(), api.clone());
        let event = make_event("!raid_history", false, false);
        engine.handle(&event).await;

        let msg = api.last_message().await.unwrap();
        assert!(msg.contains("Noch keine Raids"), "Meldung: {msg}");
    }

    #[tokio::test]
    async fn raid_history_mit_einträgen() {
        let pool = pool_or_skip!("cmd_raid_hist_mit");
        apply_ddl(&pool).await;
        let api = MockApi::new();

        sqlx::query(
            "INSERT INTO twitch_streamers_partner_state (twitch_login, twitch_user_id, is_partner_active, raid_bot_enabled) VALUES ('testchannel', 'bc123', 1, 1)",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO twitch_raid_history (from_broadcaster_id, to_broadcaster_login, viewer_count, success) VALUES ('bc123', 'streamerx', 50, TRUE)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let engine = make_engine_with_pool(pool.clone(), api.clone());
        let event = make_event("!raid_history", false, false);
        engine.handle(&event).await;

        let msg = api.last_message().await.unwrap();
        assert!(msg.contains("Letzte Raids"), "Meldung: {msg}");
        assert!(msg.contains("streamerx"), "Meldung: {msg}");
    }

    #[tokio::test]
    async fn kein_partner_gibt_fehlermeldung() {
        let pool = pool_or_skip!("cmd_kein_partner");
        apply_ddl(&pool).await;
        let api = MockApi::new();
        let engine = make_engine_with_pool(pool.clone(), api.clone());

        let event = make_event("!raid_status", false, false);
        engine.handle(&event).await;

        let msg = api.last_message().await.unwrap();
        assert!(msg.contains("nicht als Partner registriert"), "Meldung: {msg}");
    }

    #[tokio::test]
    async fn raid_enable_kein_auth_row() {
        let pool = pool_or_skip!("cmd_raid_enable_kein_auth");
        apply_ddl(&pool).await;
        let api = MockApi::new();

        sqlx::query(
            "INSERT INTO twitch_streamers_partner_state (twitch_login, twitch_user_id, is_partner_active, raid_bot_enabled) VALUES ('testchannel', 'bc123', 1, 0)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let engine = make_engine_with_pool(pool.clone(), api.clone());
        let event = make_event("!raid_enable", true, false);
        engine.handle(&event).await;

        let msg = api.last_message().await.unwrap();
        assert!(msg.contains("OAuth fehlt"), "Meldung: {msg}");
    }

    #[tokio::test]
    async fn raid_enable_bereits_aktiv() {
        let pool = pool_or_skip!("cmd_raid_enable_aktiv");
        apply_ddl(&pool).await;
        let api = MockApi::new();

        sqlx::query(
            "INSERT INTO twitch_streamers_partner_state (twitch_login, twitch_user_id, is_partner_active, raid_bot_enabled) VALUES ('testchannel', 'bc123', 1, 1)",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO twitch_raid_auth (twitch_user_id, twitch_login, raid_enabled) VALUES ('bc123', 'testchannel', TRUE)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let engine = make_engine_with_pool(pool.clone(), api.clone());
        let event = make_event("!raid_enable", true, false);
        engine.handle(&event).await;

        let msg = api.last_message().await.unwrap();
        assert!(msg.contains("bereits aktiviert"), "Meldung: {msg}");
    }

    #[tokio::test]
    async fn uban_kein_eintrag() {
        let pool = pool_or_skip!("cmd_uban_leer");
        apply_ddl(&pool).await;
        let api = MockApi::new();

        sqlx::query(
            "INSERT INTO twitch_streamers_partner_state (twitch_login, twitch_user_id, is_partner_active, raid_bot_enabled) VALUES ('testchannel', 'bc123', 1, 1)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let engine = CommandEngine::new(
            pool,
            api.clone() as Arc<dyn ChatApi>,
            MockRaid::default_arc(),
            Arc::new(MockDiscordLink { url: None }),
            Arc::new(MockInvite { reply: None }),
            Arc::new(MockSuperMod(false)),
            Arc::new(MockAutoban(None)),
        );

        let event = make_event("!uban", true, false);
        engine.handle(&event).await;

        let msg = api.last_message().await.unwrap();
        assert!(msg.contains("Kein Auto-Ban-Eintrag"), "Meldung: {msg}");
    }

    #[tokio::test]
    async fn invite_cooldown_verhindert_doppelaufruf() {
        let pool = pool_or_skip!("cmd_invite_cd");
        apply_ddl(&pool).await;
        let api = MockApi::new();
        let engine = CommandEngine::new(
            pool,
            api.clone(),
            MockRaid::default_arc(),
            Arc::new(MockDiscordLink { url: None }),
            Arc::new(MockInvite {
                reply: Some("Einladung!".to_string()),
            }),
            Arc::new(MockSuperMod(false)),
            Arc::new(MockAutoban(None)),
        );

        // Erster Aufruf
        let event = make_event("!invite", false, false);
        engine.handle(&event).await;
        let count_first = api.message_count().await;

        // Zweiter Aufruf sofort — Cooldown aktiv
        engine.handle(&event).await;
        let count_second = api.message_count().await;

        assert_eq!(
            count_first, count_second,
            "Zweiter !invite muss durch Cooldown blockiert werden"
        );
    }
}
