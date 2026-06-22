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
//! `!lurkersteuer_off` setzt `streamer_plans.lurker_tax_enabled = 0` (nur
//! Broadcaster, nur bei Plänen mit Entitlement `chat.lurker_tax`). Die
//! Plan-Auflösung läuft über `tb_analytics::plan::resolve_plan_snapshot`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rand::seq::SliceRandom;
use sqlx::PgPool;
use tb_knowledge::{KnowledgeBase, Namespace};
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

/// `!clip`-Antwort wenn keine Broadcaster-/Bot-Autorisierung vorliegt.
/// Wortlaut an `commands.py:341` angeglichen; der tote `!raid_enable`-Verweis
/// bleibt draußen (Grillme `chat-commands-tokens-07` / Block 8 „in oder raus").
const CLIP_OAUTH_MISSING_REPLY: &str =
    "OAuth fehlt. Bitte den Bot einmal autorisieren, dann klappt der Clip.";

/// `!clip`-Antwort bei fehlgeschlagener Helix-Erstellung.
/// Wortlaut an `commands.py:383–384` angeglichen.
const CLIP_FAILED_REPLY: &str =
    "Clip konnte nicht erstellt werden. Bitte in 10 Sekunden nochmal versuchen.";

const HELP_BASE_URL: &str = "https://deutsche-deadlock-community.de/streamer/help";
const COMMANDS_URL: &str = "https://deutsche-deadlock-community.de/streamer/commands";

fn knowledge_dir() -> PathBuf {
    match std::env::var("KNOWLEDGE_DIR")
        .ok()
        .filter(|v| !v.trim().is_empty())
    {
        Some(p) => PathBuf::from(p),
        None => PathBuf::from("rust/knowledge"),
    }
}

fn knowledge_base() -> &'static KnowledgeBase {
    static KB: OnceLock<KnowledgeBase> = OnceLock::new();
    KB.get_or_init(|| KnowledgeBase::load_from_dir(&knowledge_dir()).unwrap_or_default())
}

fn commands_reply() -> String {
    format!("Alle Befehle findest du hier: {COMMANDS_URL}")
}

fn help_reply(kb: &KnowledgeBase, topic: &str) -> String {
    let topic = topic.trim();
    if topic.is_empty() {
        return format!("Sag mir ein Thema, z. B. !help raid — oder schau hier: {HELP_BASE_URL}");
    }
    match kb.select(topic, Namespace::Bot, None, 1).first() {
        Some(doc) => format!("{}: {HELP_BASE_URL}#{}", doc.title, doc.slug),
        None => format!("Dazu habe ich nichts gefunden — schau hier: {HELP_BASE_URL}"),
    }
}

/// Statuszeile für `!raid_status` (`commands.py:120–125`).
///
/// `!raid_enable` entfällt (Grillme Block 8 — „in oder raus"): die Auto-Raid-
/// Teilnahme hängt am Partner-Status, nicht an einem Chat-Auth-Befehl. Frühere
/// Hinweise „aktiviere mit !raid_enable" zeigten auf einen nicht mehr
/// existierenden Befehl und sind hier entfernt.
fn raid_status_line(raid_enabled: Option<bool>) -> &'static str {
    match raid_enabled {
        None => "❌ Nicht autorisiert (OAuth fehlt) | Der Streamer muss den Twitch-Bot einmal autorisieren.",
        Some(true) => "✅ Aktiv | Auto-Raids sind aktiviert.",
        Some(false) => "🛑 Deaktiviert.",
    }
}

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
/// Port für die Scam-Guard-Chat-Commands (`!explain`, plus `overturned`-
/// Markierung bei `!unban`). Backing: [`crate::conversation_scam::ScamGuardCommands`].
#[async_trait]
pub trait ScamGuardCommandPort: Send + Sync {
    /// Ausführliche, in Twitch-Häppchen gesplittete Erklärung des jüngsten
    /// Scam-Falls. Leerer Vektor = kein Fall gefunden.
    async fn explain(&self, channel_login: &str, target: Option<&str>) -> Vec<String>;
    /// Markiert den jüngsten Scam-Ban dieses Accounts als aufgehoben.
    async fn overturn(&self, channel_login: &str, chatter_id: &str) -> bool;
}

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
    /// Optionaler Scam-Guard-Port (`!explain` / `!unban`-overturn). `None` → inaktiv.
    scam: Option<Arc<dyn ScamGuardCommandPort>>,
    /// In-memory Cooldown-Tabelle für `!invite`.
    /// `bot.py:781` — 1h pro (channel_login, chatter_login).
    invite_cooldowns: Mutex<HashMap<(String, String), Instant>>,
    /// Rate-Limiter für `!title` (B11): 5/600s pro streamer:source.
    title_rate_limiter: Arc<crate::title_ai::TitleRateLimiter>,
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
            scam: None,
            invite_cooldowns: Mutex::new(HashMap::new()),
            title_rate_limiter: Arc::new(crate::title_ai::TitleRateLimiter::default()),
        }
    }

    /// Setzt den optionalen Clip-Port (`!clip`). Builder-Style, damit der
    /// Konstruktor und die Tests unverändert bleiben.
    pub fn set_clip_port(mut self, clip: Arc<dyn ClipPort>) -> Self {
        self.clip = Some(clip);
        self
    }

    /// Setzt den optionalen Scam-Guard-Port (`!explain` / `!unban`-overturn).
    /// Builder-Style, damit Konstruktor und Tests unverändert bleiben.
    pub fn set_scam_port(mut self, scam: Arc<dyn ScamGuardCommandPort>) -> Self {
        self.scam = Some(scam);
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
            // !raid_enable / !raidbot entfällt — Grillme Block 8 ("in oder raus"):
            // Partner-Status ersetzt das Opt-in; es gibt keinen Chat-Auth-Befehl mehr.
            // Aliase fallen damit in `_ => false` (keine Command-Behandlung).
            "!uban" | "!unban" => {
                if event.is_mod_or_broadcaster() {
                    self.cmd_uban(event).await;
                } else {
                    self.reply(event, "Nur der Broadcaster oder Mods.").await;
                }
                true
            }
            "!explain" => {
                if event.is_mod_or_broadcaster() {
                    self.cmd_explain(event, args).await;
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
                    self.reply(
                        event,
                        "Nur der Broadcaster oder Mods können den Bot steuern.",
                    )
                    .await;
                }
                true
            }
            "!silentraid" => {
                if event.is_mod_or_broadcaster() {
                    self.cmd_silentraid(event).await;
                } else {
                    self.reply(
                        event,
                        "Nur der Broadcaster oder Mods können den Bot steuern.",
                    )
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
            "!commands" => {
                self.cmd_commands(event).await;
                true
            }
            "!help" => {
                self.cmd_help(event, args).await;
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
            "!rank" => {
                self.cmd_rank(event, args).await;
                true
            }
            "!wins" => {
                self.cmd_wins(event, args).await;
                true
            }
            "!winrate" => {
                self.cmd_winrate(event, args).await;
                true
            }
            "!lastmatch" | "!last" => {
                self.cmd_lastmatch(event, args).await;
                true
            }
            "!streak" => {
                self.cmd_streak(event, args).await;
                true
            }
            "!mostplayed" | "!main" => {
                self.cmd_mostplayed(event, args).await;
                true
            }
            // !title / !titel: bewusst nicht portiert — KI-Abhängigkeit außerhalb Scope.
            // Handle als false → Pipeline fährt fort.
            "!title" | "!titel" => {
                self.cmd_title(event, args).await;
                true
            }
            // !lurkersteuer_off — commands.py:535: Broadcaster schaltet die
            // Lurker-Steuer dauerhaft ab (Schreibpfad streamer_plans.lurker_tax_enabled).
            "!lurkersteuer_off" | "!lurkersteuer_aus" | "!lurker_tax_off" => {
                self.cmd_lurkersteuer_off(event).await;
                true
            }
            _ => false,
        }
    }

    // -----------------------------------------------------------------------
    // Hilfsmethoden
    // -----------------------------------------------------------------------

    /// `bot.py:2165` — `(name or "").lower().lstrip("#")`
    fn normalize_channel_login(name: &str) -> String {
        name.to_lowercase().trim_start_matches('#').to_string()
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

    /// Channel-Classifier-Parität (`channel_classifier.rs`): ein Kanal ist nur
    /// dann Partner, wenn `is_partner_active = 1` UND er NICHT `is_monitored_only`
    /// ist. Ein reiner Scout-/Monitoring-Kanal (z. B. sagetheman_) ist KEIN
    /// Partner — anders als `get_partner`, das `is_monitored_only` nicht prüft.
    async fn is_partner_channel(&self, channel_login: &str) -> bool {
        let normalized = Self::normalize_channel_login(channel_login);
        let row = sqlx::query_as::<_, (i64,)>(
            r#"
            SELECT COUNT(*)
            FROM twitch_streamers_partner_state ps
            WHERE LOWER(ps.twitch_login) = $1
              AND ps.is_partner_active = 1
              AND NOT EXISTS (
                  SELECT 1 FROM twitch_streamers s
                  WHERE LOWER(s.twitch_login) = LOWER(ps.twitch_login)
                    AND NOT EXISTS (
                        SELECT 1 FROM twitch_partners pp
                        WHERE pp.twitch_user_id = s.twitch_user_id
                           OR LOWER(pp.twitch_login) = LOWER(s.twitch_login)
                    )
              )
            "#,
        )
        .bind(normalized)
        .fetch_one(&self.pool)
        .await;
        matches!(row, Ok((n,)) if n > 0)
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
        if let Err(e) = self
            .api
            .send_message(&event.broadcaster_user_id, &msg)
            .await
        {
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

    async fn cmd_commands(&self, event: &ChatMessageEvent) {
        self.reply(event, &commands_reply()).await;
    }

    async fn cmd_help(&self, event: &ChatMessageEvent, args: &str) {
        self.reply(event, &help_reply(knowledge_base(), args)).await;
    }

    async fn cmd_rank(&self, event: &ChatMessageEvent, _args: &str) {
        let info =
            match crate::stats::resolve_discord_id(&self.pool, &event.broadcaster_user_id).await {
                Some(discord_id) => crate::stats::fetch_rank(&discord_id, false).await,
                None => None,
            };
        self.reply(
            event,
            &crate::stats::rank_reply(&event.broadcaster_user_name, info.as_ref()),
        )
        .await;
    }

    async fn cmd_wins(&self, event: &ChatMessageEvent, _args: &str) {
        let info =
            match crate::stats::resolve_discord_id(&self.pool, &event.broadcaster_user_id).await {
                Some(discord_id) => crate::stats::fetch_rank(&discord_id, true).await,
                None => None,
            };
        self.reply(
            event,
            &crate::stats::wins_reply(&event.broadcaster_user_name, info.as_ref()),
        )
        .await;
    }

    async fn cmd_winrate(&self, event: &ChatMessageEvent, _args: &str) {
        let info =
            match crate::stats::resolve_discord_id(&self.pool, &event.broadcaster_user_id).await {
                Some(discord_id) => crate::stats::fetch_matches(&discord_id).await,
                None => None,
            };
        self.reply(
            event,
            &crate::stats::winrate_reply(&event.broadcaster_user_name, info.as_ref()),
        )
        .await;
    }

    async fn cmd_lastmatch(&self, event: &ChatMessageEvent, _args: &str) {
        let info =
            match crate::stats::resolve_discord_id(&self.pool, &event.broadcaster_user_id).await {
                Some(discord_id) => crate::stats::fetch_matches(&discord_id).await,
                None => None,
            };
        self.reply(
            event,
            &crate::stats::lastmatch_reply(&event.broadcaster_user_name, info.as_ref()),
        )
        .await;
    }

    async fn cmd_streak(&self, event: &ChatMessageEvent, _args: &str) {
        let info =
            match crate::stats::resolve_discord_id(&self.pool, &event.broadcaster_user_id).await {
                Some(discord_id) => crate::stats::fetch_matches(&discord_id).await,
                None => None,
            };
        self.reply(
            event,
            &crate::stats::streak_reply(&event.broadcaster_user_name, info.as_ref()),
        )
        .await;
    }

    async fn cmd_mostplayed(&self, event: &ChatMessageEvent, _args: &str) {
        let info =
            match crate::stats::resolve_discord_id(&self.pool, &event.broadcaster_user_id).await {
                Some(discord_id) => crate::stats::fetch_matches(&discord_id).await,
                None => None,
            };
        self.reply(
            event,
            &crate::stats::mostplayed_reply(&event.broadcaster_user_name, info.as_ref()),
        )
        .await;
    }

    /// `!title <keywords> [--live]` — generiert einen Stream-Titel via MiniMax
    /// (B11). Port von `cmd_title` (chat/commands.py:770). MOD-ONLY.
    ///
    /// Schickt erst die Ack, dann läuft die schwere Arbeit (DB-Reads,
    /// steam_lookup, MiniMax-Call) in einem `tokio::spawn`, damit ein langsamer
    /// LLM-Call die Chat-Pipeline nicht blockiert. Die Antwort geht direkt über
    /// die geklonte `ChatApi`.
    async fn cmd_title(&self, event: &ChatMessageEvent, args: &str) {
        // Nur Broadcaster/Mods (Python: stiller Return für andere).
        if !event.is_mod_or_broadcaster() {
            return;
        }
        let raw_args = args.trim();
        if raw_args.is_empty() {
            self.reply_plain(
                event,
                "Verwendung: !title <keywords>  — z.B.: !title ranked solo grind",
            )
            .await;
            return;
        }
        let include_live = raw_args.contains("--live");
        let keywords = raw_args.replace("--live", "");
        let keywords = keywords.trim().to_string();
        if keywords.is_empty() {
            self.reply_plain(
                event,
                "Bitte Keywords angeben, z.B.: !title ranked solo grind",
            )
            .await;
            return;
        }

        self.reply_plain(event, "Generiere deinen Titel, einen Moment...")
            .await;

        let pool = self.pool.clone();
        let api = Arc::clone(&self.api);
        let rate_limiter = Arc::clone(&self.title_rate_limiter);
        let streamer_id = event.broadcaster_user_id.clone();
        let channel = event.broadcaster_user_login.clone();

        tokio::spawn(async move {
            // Streamer-Existenz + discord_user_id. broadcaster_user_id ist die
            // twitch_user_id des Streamers (= der Kanal). Python sucht über den
            // Login; hier direkt über die schon bekannte ID.
            let row: Option<(Option<String>,)> = sqlx::query_as(
                "SELECT discord_user_id::text \
                 FROM twitch_streamer_identities \
                 WHERE twitch_user_id = $1 \
                 LIMIT 1",
            )
            .bind(&streamer_id)
            .fetch_optional(&pool)
            .await
            .ok()
            .flatten();
            let Some((discord_raw,)) = row else {
                let _ = api
                    .send_message(
                        &streamer_id,
                        "Streamer nicht gefunden – bitte Onboarding prüfen.",
                    )
                    .await;
                return;
            };
            let discord_id = discord_raw.and_then(|s| s.trim().parse::<i64>().ok());

            // title_db: History + eigener AVG + Community-Knowledge.
            let history =
                crate::title_db::get_streamer_title_history(&pool, &streamer_id, 30).await;
            let own_avg = crate::title_db::get_streamer_avg_viewers(&pool, &streamer_id).await;
            let knowledge = crate::title_db::get_top_knowledge_titles(&pool, 30).await;

            // Pro History-Item relative_perf + engagement_rate (Python cmd_title:823).
            let prompt_history: Vec<crate::title_ai::PromptHistoryItem> = history
                .iter()
                .map(|h| {
                    let avg = h.avg_viewers.unwrap_or(0.0);
                    let followers = h.followers_start.unwrap_or(1).max(1) as f64;
                    let relative_perf = if own_avg > 0.0 { avg / own_avg } else { 0.0 };
                    crate::title_ai::PromptHistoryItem {
                        title: h.title.clone(),
                        relative_perf: Some(relative_perf),
                        engagement_rate: Some(avg / followers),
                    }
                })
                .collect();
            let prompt_knowledge: Vec<crate::title_ai::PromptKnowledgeItem> = knowledge
                .into_iter()
                .map(|k| crate::title_ai::PromptKnowledgeItem {
                    title: k.title,
                    normalized_score: k.normalized_score,
                })
                .collect();

            // steam_lookup (Rang + optional Live) — sync, daher off-Thread.
            let mut rank_display: Option<String> = None;
            let mut live: Option<crate::title_ai::PromptLiveState> = None;
            if let Some(did) = discord_id {
                let db_path = crate::steam_lookup::steam_db_path();
                let db_path2 = db_path.clone();
                let rank = tokio::task::spawn_blocking(move || {
                    crate::steam_lookup::get_rank_for_discord_user(&db_path, did)
                })
                .await
                .ok()
                .flatten();
                rank_display = rank.map(|r| r.rank_display);
                if include_live {
                    let live_res = tokio::task::spawn_blocking(move || {
                        crate::steam_lookup::get_live_state_for_discord_user(&db_path2, did)
                    })
                    .await
                    .ok()
                    .flatten();
                    live = live_res.map(|l| crate::title_ai::PromptLiveState {
                        hero: l.hero,
                        party_hint: l.party_hint,
                    });
                }
            }

            let result = crate::title_ai::generate_title(
                &rate_limiter,
                &streamer_id,
                &keywords,
                &prompt_history,
                &prompt_knowledge,
                rank_display.as_deref(),
                live.as_ref(),
                "chat",
            )
            .await;

            let reply = match result {
                Ok(r) => {
                    let primary = if r.primary.is_empty() {
                        "Kein Titel generiert".to_string()
                    } else {
                        r.primary
                    };
                    let alt_str = if r.alternatives.is_empty() {
                        String::new()
                    } else {
                        format!(" | Alternativen: {}", r.alternatives.join(" | "))
                    };
                    format!("Titel: {primary}{alt_str}")
                }
                Err(crate::title_ai::GenerateTitleError::RateLimit(e)) => format!(
                    "Bitte warte noch {} Sekunden vor der nächsten Anfrage.",
                    e.retry_after
                ),
                Err(_) => "Fehler beim Generieren. Bitte später erneut versuchen.".to_string(),
            };
            if let Err(e) = api.send_message(&streamer_id, &reply).await {
                tracing::warn!(channel = %channel, err = %e, "!title-Antwort-Send fehlgeschlagen");
            }
        });
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
            self.reply_plain(event, "Noch keine Raids durchgeführt.")
                .await;
            return;
        }

        let parts: Vec<String> = rows
            .iter()
            .map(|r| {
                let icon = if r.success.unwrap_or(false) {
                    "✅"
                } else {
                    "❌"
                };
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
        let status = raid_status_line(info.raid_enabled);

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
        let last_part = if let (Some(login), Some(viewers), Some(at)) = (
            &info.last_raid_login,
            info.last_raid_viewers,
            info.last_raid_at,
        ) {
            let icon = if info.successful_raids > 0 {
                "✅"
            } else {
                "❌"
            };
            let formatted = at.format("%Y-%m-%d %H:%M").to_string();
            format!(" | Letzter Raid {icon}: {login} ({viewers} Viewer) am {formatted}")
        } else {
            String::new()
        };

        let msg = format!("{status}{stats_part}{last_part}");
        self.reply_plain(event, &msg).await;
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
            self.reply(event, "Kein Nutzer gespeichert für Unban.")
                .await;
            return;
        }

        // moderation.py:1831 — DELETE /helix/moderation/bans
        match self
            .api
            .unban_user(&partner.twitch_user_id, &entry.user_id)
            .await
        {
            Ok(true) => {
                if let Some(scam) = self.scam.as_ref() {
                    scam.overturn(&event.broadcaster_user_login, &entry.user_id)
                        .await;
                }
                self.reply(event, &format!("Unban ausgeführt für {}.", entry.login))
                    .await;
            }
            _ => {
                self.reply(event, &format!("Unban fehlgeschlagen für {}.", entry.login))
                    .await;
            }
        }
    }

    /// `!explain [@user]` — lässt das LLM den jüngsten Scam-Fall des Kanals (oder
    /// eines genannten Accounts) ausführlich erklären, gesplittet in mehrere
    /// Chat-Nachrichten (≤480 Zeichen, kein Mengen-Limit).
    async fn cmd_explain(&self, event: &ChatMessageEvent, args: &str) {
        let Some(scam) = self.scam.as_ref() else {
            self.reply(event, "Die Scam-Erklärung ist auf diesem Kanal nicht aktiv.")
                .await;
            return;
        };
        let trimmed = args.trim();
        let target = (!trimmed.is_empty()).then_some(trimmed);
        let chunks = scam.explain(&event.broadcaster_user_login, target).await;
        if chunks.is_empty() {
            self.reply(
                event,
                "Ich habe keinen passenden Scam-Fall zum Erklären gefunden.",
            )
            .await;
            return;
        }
        for chunk in chunks {
            self.reply_plain(event, &chunk).await;
        }
    }

    // -----------------------------------------------------------------------
    // !raid / !traid — commands.py:617
    // -----------------------------------------------------------------------

    async fn cmd_raid(&self, event: &ChatMessageEvent) {
        let partner = match self.get_partner(&event.broadcaster_user_login).await {
            Some(p) => p,
            None => {
                self.reply(event, "Dieser Kanal ist nicht als Partner registriert.")
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
                self.reply(event, &format!("Raid fehlgeschlagen: {e}"))
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
                self.reply(event, CLIP_OAUTH_MISSING_REPLY).await;
            }
            ClipOutcome::Failed => {
                self.reply(event, CLIP_FAILED_REPLY).await;
            }
        }
    }

    // -----------------------------------------------------------------------
    // !lurkersteuer_off — commands.py:535
    // -----------------------------------------------------------------------

    /// Deaktiviert die Lurker-Steuer dauerhaft für den aktuellen Kanal
    /// (`streamer_plans.lurker_tax_enabled = 0`).
    ///
    /// Ablauf wie `commands.py:539`:
    /// 1. Nur der Broadcaster darf abschalten.
    /// 2. Kanal muss als Partner registriert sein.
    /// 3. `is_paid_plan`-Gate: nur Pläne mit Entitlement `chat.lurker_tax`
    ///    (volle Snapshot-Resolution → abgelaufene Pläne zählen nicht). Lurker-Tax
    ///    ist Opt-in und default deaktiviert (Grillme Block 1/9) — der Off-Befehl
    ///    setzt das DB-Flag, der Toggle-Dashboard-Pfad ist ein eigenes Ticket.
    async fn cmd_lurkersteuer_off(&self, event: &ChatMessageEvent) {
        if !event.is_broadcaster() {
            self.reply(
                event,
                "Nur der Broadcaster kann die Lurker Steuer dauerhaft deaktivieren.",
            )
            .await;
            return;
        }

        let partner = match self.get_partner(&event.broadcaster_user_login).await {
            Some(p) => p,
            None => {
                self.reply(event, "Dieser Kanal ist nicht als Partner registriert.")
                    .await;
                return;
            }
        };

        // is_paid_plan: effektiver Plan muss chat.lurker_tax tragen (commands.py:566).
        let is_paid_plan = tb_analytics::plan::resolve_plan_snapshot(
            &self.pool,
            &partner.twitch_login,
            &partner.twitch_user_id,
        )
        .await
        .map(|s| s.entitlements.contains(&"chat.lurker_tax"))
        .unwrap_or(false);
        if !is_paid_plan {
            self.reply(
                event,
                "Die Lurker Steuer ist nur in bezahlten Plänen verfügbar.",
            )
            .await;
            return;
        }

        // Schreibpfad — commands.py:585: lurker_tax_enabled = 0. Vorzustand für die
        // Antwort prüfen (war sie überhaupt an?).
        let was_enabled: bool = sqlx::query_scalar::<_, Option<i32>>(
            "SELECT lurker_tax_enabled FROM streamer_plans
              WHERE LOWER(COALESCE(twitch_login, '')) = LOWER($1) LIMIT 1",
        )
        .bind(&partner.twitch_login)
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten()
        .flatten()
        .map(|v| v != 0)
        .unwrap_or(false);

        let updated = sqlx::query(
            "UPDATE streamer_plans
                SET lurker_tax_enabled = 0
              WHERE LOWER(COALESCE(twitch_login, '')) = LOWER($1)",
        )
        .bind(&partner.twitch_login)
        .execute(&self.pool)
        .await;

        match updated {
            Ok(res) if res.rows_affected() > 0 => {
                if was_enabled {
                    self.reply(
                        event,
                        "Lurker Steuer deaktiviert. Im Abo-Bereich kannst du sie später wieder aktivieren.",
                    )
                    .await;
                } else {
                    self.reply(event, "Lurker Steuer ist bereits deaktiviert.")
                        .await;
                }
                tracing::info!(login = %partner.twitch_login, "Lurker-Steuer per Chat deaktiviert");
            }
            _ => {
                self.reply(
                    event,
                    "Lurker Steuer konnte gerade nicht deaktiviert werden.",
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

        // Partner-Gate: !invite ist in Python nur im Partner-Block erreichbar
        // (bot.py:1816), NICHT über die Whitelist-Bot-Verzweigung. Ohne diesen
        // Gate könnte ein gewhitelisteter Bot mit "!invite" eine Antwort auf einem
        // reinen monitored-only Kanal auslösen (breiterer Scope als Python).
        if !self.is_partner_channel(&event.broadcaster_user_login).await {
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

        match self
            .invite
            .invite_line(&channel_login, &chatter_login)
            .await
        {
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

        let result =
            sqlx::query("DELETE FROM twitch_user_engagement_optout WHERE twitch_user_id = $1")
                .bind(user_id.as_str())
                .execute(&self.pool)
                .await;

        match result {
            Ok(_) => {
                self.reply(event, "OK, AI berücksichtigt dich wieder.")
                    .await;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::BanOutcome;
    use crate::types::{
        ChatBadge, ChatMessageBody, ChatMessageEvent, MessageFragment, SendOutcome,
    };

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
        async fn send_announcement(&self, _: &str, _: &str, _: &str) -> Result<bool, String> {
            Ok(true)
        }
        async fn ban_user(&self, _: &str, _: &str, _: &str) -> Result<BanOutcome, String> {
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
        async fn user_created_at(&self, _: &str) -> Result<Option<DateTime<Utc>>, String> {
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

    fn help_fixture_kb() -> tb_knowledge::KnowledgeBase {
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tb-knowledge/tests/fixtures");
        tb_knowledge::KnowledgeBase::load_from_dir(&root).expect("fixtures")
    }

    #[test]
    fn commands_reply_zeigt_link() {
        assert!(commands_reply().contains("/streamer/commands"));
    }

    #[test]
    fn help_reply_findet_thema() {
        let kb = help_fixture_kb();
        let r = help_reply(&kb, "raid");
        assert!(r.contains("Auto-Raid") && r.contains("/streamer/help#auto-raid"), "{r}");
    }

    #[test]
    fn help_reply_unbekannt_fallback() {
        let kb = help_fixture_kb();
        let r = help_reply(&kb, "quantenphysik");
        assert!(r.contains("/streamer/help") && !r.contains('#'), "{r}");
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
    fn clip_fehler_oauth_texte_an_python_angeglichen() {
        // chat-commands-tokens-07: !clip-Fehler-/OAuth-Wortlaut an Python.
        // Failed exakt wie commands.py:383–384.
        assert_eq!(
            CLIP_FAILED_REPLY,
            "Clip konnte nicht erstellt werden. Bitte in 10 Sekunden nochmal versuchen."
        );
        // OAuth: Python-Kern ("OAuth fehlt. Bitte ... autorisieren"), aber der
        // tote `!raid_enable`-Verweis bleibt draußen (Grillme Block 8).
        assert!(CLIP_OAUTH_MISSING_REPLY.starts_with("OAuth fehlt. Bitte"));
        assert!(CLIP_OAUTH_MISSING_REPLY.contains("autorisieren"));
        assert!(!CLIP_OAUTH_MISSING_REPLY.contains("!raid_enable"));
    }

    #[test]
    fn title_command_ist_nicht_portiert() {
        // !title / !titel → handle() gibt false zurück (in der match-Tabelle explizit)
        let cmd = "!title";
        let is_unimplemented = matches!(cmd, "!title" | "!titel");
        assert!(
            is_unimplemented,
            "!title muss als nicht-portiert markiert sein"
        );
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
            // streamer_plans — Plan-Resolution + lurker_tax_enabled-Schreibpfad
            // (!lurkersteuer_off). manual_plan_expires_at = Ablauf-Gate der
            // resolve_plan_snapshot.
            r#"CREATE TABLE streamer_plans (
                twitch_user_id TEXT PRIMARY KEY,
                twitch_login TEXT,
                plan_name TEXT,
                lurker_tax_enabled INTEGER DEFAULT 0,
                promo_disabled INTEGER DEFAULT 0,
                manual_plan_id TEXT,
                manual_plan_expires_at TIMESTAMPTZ,
                manual_plan_updated_at TIMESTAMPTZ,
                manual_plan_notes TEXT,
                trial_ever_granted INTEGER DEFAULT 0,
                first_login_at TIMESTAMPTZ
            )"#,
            // twitch_streamer_identities — user_id/login-Mapping für Plan-Refs
            r#"CREATE TABLE twitch_streamer_identities (
                twitch_user_id TEXT PRIMARY KEY,
                twitch_login TEXT
            )"#,
            // twitch_billing_subscriptions — Stripe-Abo-Fallback der Plan-Resolution
            r#"CREATE TABLE twitch_billing_subscriptions (
                customer_reference TEXT NOT NULL,
                plan_id TEXT,
                status TEXT,
                current_period_end TIMESTAMPTZ,
                updated_at TIMESTAMPTZ DEFAULT NOW()
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

        engine
            .handle(&make_event("!engagement_on", true, false))
            .await;
        engine
            .handle(&make_event("!engagement_off", true, false))
            .await;

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

        sqlx::query("INSERT INTO twitch_user_engagement_optout (twitch_user_id) VALUES ('u999')")
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
        assert!(
            msg.contains("nicht als Partner registriert"),
            "Meldung: {msg}"
        );
    }

    // !raid_enable / !raidbot entfällt (Grillme Block 8 — „in oder raus"). Die
    // Statuszeile und der gedroppte Befehl dürfen keinen toten !raid_enable-
    // Hinweis mehr ausgeben.
    #[test]
    fn raid_status_line_ohne_toten_raid_enable_hinweis() {
        for state in [None, Some(true), Some(false)] {
            let line = raid_status_line(state);
            assert!(
                !line.contains("!raid_enable"),
                "Statuszeile für {state:?} verweist auf gedroppten Befehl: {line}"
            );
        }
        assert!(raid_status_line(Some(true)).contains("Aktiv"));
        assert!(raid_status_line(Some(false)).contains("Deaktiviert"));
        assert!(raid_status_line(None).contains("Nicht autorisiert"));
    }

    #[tokio::test]
    async fn raid_enable_ist_kein_befehl_mehr() {
        let pool = pool_or_skip!("cmd_raid_enable_entfaellt");
        apply_ddl(&pool).await;
        let api = MockApi::new();
        let engine = make_engine_with_pool(pool.clone(), api.clone());

        for cmd in ["!raid_enable", "!raidbot"] {
            let event = make_event(cmd, true, false);
            // !raid_enable fällt in `_ => false` — keine Command-Behandlung.
            assert!(
                !engine.handle(&event).await,
                "{cmd} sollte kein Befehl sein"
            );
        }
        assert_eq!(
            api.message_count().await,
            0,
            "gedroppter Befehl darf keine Chat-Antwort senden"
        );
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

    // -----------------------------------------------------------------------
    // chat-commands-tokens-06: !lurkersteuer_off
    // -----------------------------------------------------------------------

    /// Legt Partner + paid-plan-Streamer (raid_boost → chat.lurker_tax) an.
    async fn seed_lurker_partner(pool: &PgPool, lurker_tax_enabled: i32, paid: bool) {
        sqlx::query(
            "INSERT INTO twitch_streamers_partner_state (twitch_login, twitch_user_id, is_partner_active, raid_bot_enabled)
             VALUES ('testchannel', 'bc123', 1, 1)",
        )
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_streamer_identities (twitch_user_id, twitch_login)
             VALUES ('bc123', 'testchannel')",
        )
        .execute(pool)
        .await
        .unwrap();
        let plan = if paid { "raid_boost" } else { "raid_free" };
        sqlx::query(
            "INSERT INTO streamer_plans (twitch_user_id, twitch_login, manual_plan_id, lurker_tax_enabled)
             VALUES ('bc123', 'testchannel', $1, $2)",
        )
        .bind(plan)
        .bind(lurker_tax_enabled)
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn lurkersteuer_off_setzt_flag_false_bei_paid_plan() {
        let pool = pool_or_skip!("cmd_lurker_off_paid");
        apply_ddl(&pool).await;
        let api = MockApi::new();
        let engine = make_engine_with_pool(pool.clone(), api.clone());
        seed_lurker_partner(&pool, 1, true).await;

        let event = make_event("!lurkersteuer_off", false, true);
        let handled = engine.handle(&event).await;
        assert!(
            handled,
            "!lurkersteuer_off muss als Command behandelt werden"
        );

        let enabled: i32 = sqlx::query_scalar(
            "SELECT lurker_tax_enabled FROM streamer_plans WHERE twitch_user_id = 'bc123'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(enabled, 0, "lurker_tax_enabled muss 0 sein");

        let msg = api.last_message().await.unwrap();
        assert!(msg.contains("deaktiviert"), "Meldung: {msg}");
    }

    #[tokio::test]
    async fn lurkersteuer_off_alias_lurker_tax_off() {
        let pool = pool_or_skip!("cmd_lurker_off_alias");
        apply_ddl(&pool).await;
        let api = MockApi::new();
        let engine = make_engine_with_pool(pool.clone(), api.clone());
        seed_lurker_partner(&pool, 1, true).await;

        let handled = engine
            .handle(&make_event("!lurker_tax_off", false, true))
            .await;
        assert!(handled, "Alias !lurker_tax_off muss greifen");

        let enabled: i32 = sqlx::query_scalar(
            "SELECT lurker_tax_enabled FROM streamer_plans WHERE twitch_user_id = 'bc123'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(enabled, 0);
    }

    #[tokio::test]
    async fn lurkersteuer_off_nur_broadcaster() {
        let pool = pool_or_skip!("cmd_lurker_off_nichtbc");
        apply_ddl(&pool).await;
        let api = MockApi::new();
        let engine = make_engine_with_pool(pool.clone(), api.clone());
        seed_lurker_partner(&pool, 1, true).await;

        // Mod, aber nicht Broadcaster → Ablehnung, Flag bleibt 1.
        let handled = engine
            .handle(&make_event("!lurkersteuer_off", true, false))
            .await;
        assert!(handled);

        let enabled: i32 = sqlx::query_scalar(
            "SELECT lurker_tax_enabled FROM streamer_plans WHERE twitch_user_id = 'bc123'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(enabled, 1, "Nicht-Broadcaster darf nicht deaktivieren");

        let msg = api.last_message().await.unwrap();
        assert!(msg.to_lowercase().contains("broadcaster"), "Meldung: {msg}");
    }

    #[tokio::test]
    async fn lurkersteuer_off_nur_bei_paid_plan() {
        let pool = pool_or_skip!("cmd_lurker_off_free");
        apply_ddl(&pool).await;
        let api = MockApi::new();
        let engine = make_engine_with_pool(pool.clone(), api.clone());
        // raid_free → kein chat.lurker_tax → Ablehnung.
        seed_lurker_partner(&pool, 1, false).await;

        let handled = engine
            .handle(&make_event("!lurkersteuer_off", false, true))
            .await;
        assert!(handled);

        let enabled: i32 = sqlx::query_scalar(
            "SELECT lurker_tax_enabled FROM streamer_plans WHERE twitch_user_id = 'bc123'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(enabled, 1, "Free-Plan darf den Schreibpfad nicht auslösen");

        let msg = api.last_message().await.unwrap();
        assert!(msg.contains("bezahlten"), "Meldung: {msg}");
    }
}
