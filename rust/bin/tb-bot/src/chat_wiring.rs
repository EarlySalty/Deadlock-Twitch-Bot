//! Composition-Root des nativen Chat-Bots (Welle B).
//!
//! Baut die komplette tb-chat-Pipeline samt aller Port-Adapter und hängt sie
//! als [`tb_monitoring::EventSubHooks`]-Wrapper vor die bestehenden Hooks.
//! Gate: `TB_CHAT_ENABLED=1` — ohne Flag bleibt alles aus und der Python-Chat
//! bedient die Kanäle weiter (Flip-Prozedur: rust/docs/04-cutover-plan.md).
//!
//! Env:
//! - `TB_CHAT_ENABLED`            — "1" aktiviert den nativen Chat
//! - `TWITCH_BOT_TOKEN`           — Seed-Access-Token (darf tot sein)
//! - `TWITCH_BOT_REFRESH_TOKEN`   — Pflicht: Boot-Pfad über Refresh
//! - `TWITCH_CLIENT_ID/SECRET`    — App-Credentials (Token-Refresh)
//! - `TB_CHAT_REVIEW_LOG_DIR`     — Verzeichnis der Review-TSVs (Default
//!   `logs` relativ zum WorkingDirectory = Repo-Root, identisch zu Python)

use std::collections::HashSet;
use std::future;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use serde_json::Value;
use sqlx::PgPool;
use tb_chat::channel_policy::{ChannelPolicyChatApi, PolicyContext};
use tb_chat::commands::{
    ClipOutcome, ClipPort, CommandEngine, DiscordLinkPort, InvitePort, LastAutobanStore,
    RaidCommandPort, RaidStartResult, RaidStatusInfo, SuperModPort,
};
use tb_chat::conversation_scam::{
    ConversationScamGuard, MiniMaxScamJudge, ScamGuardCommands, ScamGuardNotifier,
};
use tb_chat::moderation::{
    HelixChatClient, ModerationEngine, OutboundSuppressionStore, TimeoutGuard, WERBEFREI_PITCH_MSG,
};
use tb_chat::promos::{
    InviteResolver, PartnerChannelCheck, PresetPicker, PromoEngine, PromoPreset, RandomPresetPicker,
};
use tb_chat::scam_pitch::{AccountAgePort, ScamPitchDetector, SpamAiReviewer};
use tb_chat::spam_filter::{LearnedPatterns, SpamFilter};
use tb_chat::style_score::{build_centroid, Centroid};
use tb_chat::timeout_tracking::{
    BotBannedChannelHandler, CombinedSuppression, TimeoutTrackingChatApi,
};
use tb_chat::token::BotTokenManager;
use tb_chat::types::ChatMessageEvent;
use tb_chat::{
    lfg_pitch_enabled_from_env, promo_invite_fallback, ChannelClassifier, ChatApi, ChatPipeline,
    ChatPipelineParts, ChatterTracker, FunResponses, GlobalBanSweeper, GlobalChatterBanEnforcer,
    InviteQuestionInviteUrlPort, InviteQuestionResponder, LfgPitchResponder,
    MiniMaxInviteQuestionJudge, MiniMaxLfgJudge, ModAlerter, PartnerRoster, PgHelixMentionResolver,
    PgInviteQuestionStore, ReviewLog, SusInviteCheck,
};
use tb_crypto::FieldCipher;
use tb_engagement::irc_reader::EngagementIrcReader;
use tb_engagement::minimax_chat::{ChatMessage, EngagementMinimaxClient};
use tb_engagement::pipeline::EngagementPipeline;
use tb_engagement::sender_auth::SenderAuthStore;
use tb_engagement::stealth_sender::StealthSender;
use tb_engagement::types::IncomingMessage;
use tb_knowledge::KnowledgeBase;
use tb_monitoring::{ChatNotificationKind, EventSubHooks, SubscriptionManager, TelemetryStore};
use tb_raid::{RaidAuthStore, RaidTokenRefresher, TokenBlacklistStore, TokenProvider};
use tb_transport_discord::BrokerRelay;
use tb_transport_twitch::HelixClient;

use crate::raid_adapters::HelixTokenClient;
use crate::raid_greeting::RaidGreetingMonitor;
use crate::task_supervisor::TaskSupervisor;

/// Reconcile-Intervall für Chat-Subscriptions (Python: periodischer
/// Channel-Join alle 30 Minuten, connection.py).
const CHAT_SUB_RECONCILE_INTERVAL: Duration = Duration::from_secs(30 * 60);

/// Fallback-Env für den globalen Discord-Invite (chat_command.rs / promos.py).
const PROMO_DISCORD_INVITE_ENV: &str = "PROMO_DISCORD_INVITE";

const MINIMAX_PRESET_SYSTEM_PROMPT: &str = "Du wählst für einen deutschen Twitch-Chat das am besten passende Discord-Einladungs-Preset aus. Du erhältst die verfügbaren Presets (jeweils im Format 'id: Text') und einige aktuelle Chat-Ausschnitte. Wähle das Preset, dessen Ton und Inhalt am besten zum Chat passen. Antworte ausschließlich mit der exakten Preset-id, ohne weitere Worte.";

fn knowledge_dir() -> PathBuf {
    std::env::var("KNOWLEDGE_DIR")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("rust/knowledge"))
}

fn knowledge_base() -> &'static KnowledgeBase {
    static KB: OnceLock<KnowledgeBase> = OnceLock::new();
    KB.get_or_init(|| match KnowledgeBase::load_from_dir(&knowledge_dir()) {
        Ok(kb) => {
            tracing::info!(
                "go-live-tipp: Wissensbasis geladen ({} Dokumente)",
                kb.len()
            );
            kb
        }
        Err(error) => {
            tracing::warn!(%error, "go-live-tipp: Wissensbasis nicht geladen");
            KnowledgeBase::default()
        }
    })
}

// ---------------------------------------------------------------------------
// Clip-Port (!clip)
// ---------------------------------------------------------------------------

/// Adapter für den `!clip`-Command: holt einen **gültigen** Broadcaster-Token
/// (mit Auto-Refresh bei Ablauf, ohne `raid_enabled`-Gate — wie Python
/// `get_tokens_for_user`, auth.py:1378) und ruft Helix `POST /clips`.
///
/// Fällt der Broadcaster-Token aus (`Ok(None)`), wird wie Python
/// (`commands.py`:322-337) der Bot-eigene Token als Fallback verwendet; erst
/// wenn auch der fehlt, gibt es `OAuthMissing`. Der Bot-Token wird dem
/// Broadcaster zwar nicht zugeschrieben — Python nimmt ihn dennoch als letzten
/// Versuch (Scope-Vorbehalt `clips:edit` siehe Python-Kommentar).
struct ChatClipAdapter {
    helix: Arc<HelixClient>,
    token_provider: Arc<TokenProvider>,
    bot_token: Arc<BotTokenManager>,
}

#[async_trait::async_trait]
impl ClipPort for ChatClipAdapter {
    async fn create_clip(
        &self,
        broadcaster_user_id: &str,
        _broadcaster_login: &str,
    ) -> ClipOutcome {
        // Ungated + Auto-Refresh: Streamer mit deaktivierten Raids dürfen clippen,
        // und ein abgelaufener Token wird erneuert statt 401 zu produzieren.
        let access_token = match self
            .token_provider
            .get_valid_token_unrestricted(broadcaster_user_id, chrono::Utc::now())
            .await
        {
            Ok(Some(t)) => t,
            // Fallback: Bot-eigenen Token verwenden (Python `commands.py`:322-337).
            Ok(None) => match self.bot_token.get_valid_token(false).await {
                Ok(t) => {
                    tracing::debug!("!clip: nutze Bot-Token als Fallback");
                    t
                }
                Err(error) => {
                    tracing::debug!(%error, "!clip: Bot-Token-Fallback nicht verfügbar");
                    return ClipOutcome::OAuthMissing;
                }
            },
            Err(error) => {
                tracing::warn!(%error, "!clip: Broadcaster-Token-Load fehlgeschlagen");
                return ClipOutcome::Failed;
            }
        };
        match self
            .helix
            .create_clip(broadcaster_user_id, &access_token)
            .await
        {
            Ok(Some(clip)) => {
                let url = if !clip.id.is_empty() {
                    format!("https://clips.twitch.tv/{}", clip.id)
                } else {
                    clip.edit_url
                };
                if url.is_empty() {
                    ClipOutcome::Failed
                } else {
                    ClipOutcome::Created { url }
                }
            }
            Ok(None) => ClipOutcome::Failed,
            Err(error) => {
                tracing::warn!(%error, "!clip: Helix POST /clips fehlgeschlagen");
                ClipOutcome::Failed
            }
        }
    }
}

/// Baut den optionalen Clip-Port — nur mit Helix-Client UND Krypto-Key.
/// Ohne beides bleibt `!clip` beim Migrations-Hinweis (kein Crash).
///
/// Konstruiert einen eigenen [`TokenProvider`] (Store + Refresher + Blacklist),
/// damit `!clip` einen abgelaufenen Broadcaster-Token selbst erneuern kann —
/// unabhängig davon, ob die Raid-Strecke nativ läuft. `redirect_uri` ist hier
/// belanglos (der Refresh-Grant braucht sie nicht; nur `exchange_code` täte es).
///
/// `bot_token` ist der live rotierte Bot-User-Token (aus [`ChatApiHandle`]) und
/// dient als Fallback, wenn der Broadcaster keinen Token hinterlegt hat — 1:1
/// zum Python-Pfad (`commands.py`:322-337).
pub fn build_clip_port(
    helix: Option<Arc<HelixClient>>,
    cipher: Option<Arc<FieldCipher>>,
    pool: PgPool,
    bot_token: Arc<BotTokenManager>,
) -> Option<Arc<dyn ClipPort>> {
    let (Some(helix), Some(cipher)) = (helix, cipher) else {
        return None;
    };
    let blacklist = Arc::new(TokenBlacklistStore::new(pool.clone()));
    let refresher = RaidTokenRefresher::new(
        pool.clone(),
        cipher.clone(),
        Arc::new(HelixTokenClient {
            helix: (*helix).clone(),
            redirect_uri: std::env::var("TWITCH_RAID_REDIRECT_URI").unwrap_or_default(),
        }),
        blacklist.clone(),
    );
    let token_provider = Arc::new(TokenProvider::new(
        RaidAuthStore::new(pool, cipher),
        refresher,
        blacklist,
    ));
    Some(Arc::new(ChatClipAdapter {
        helix,
        token_provider,
        bot_token,
    }))
}

/// Baut den Engagement-Stealth-Sender (Smoke-Account) — nur mit Krypto-Key UND
/// App-Credentials. Fehlt eins, gibt es keinen Sende-Account und die AI-Antwort
/// wird verworfen (`None`), exakt wie Pythons Fallback. Liest den bereits live
/// onboardeten Token aus `twitch_engagement_sender_auth` (Tabelle wird hier
/// idempotent angelegt).
async fn build_engagement_stealth(pool: PgPool) -> Option<Arc<StealthSender>> {
    let cipher = Arc::new(FieldCipher::from_env().ok()?);
    let client_id = std::env::var("TWITCH_CLIENT_ID")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())?;
    let auth = SenderAuthStore::from_env(pool.clone(), cipher)?;
    auth.ensure_table().await;
    Some(Arc::new(StealthSender::new(
        Arc::new(auth),
        client_id,
        pool,
    )))
}

// ---------------------------------------------------------------------------
// Chat-Action-Port (Bot-Token-Bridge) — POST /streamers/:login/chat-action
// ---------------------------------------------------------------------------

use tb_chat::types::SendOutcome;
use tb_internal_api::{ChatActionPort, ChatActionResult};

/// Erlaubte Chat-Action-Modi (Python `_CHAT_ACTION_MODES`, live.py).
/// Unbekannte Werte fallen auf `message` zurück (Python-Parität).
const CHAT_ACTION_MODES: &[&str] = &["message", "action", "announcement"];
/// Erlaubte Announcement-Farben (Python `_CHAT_ANNOUNCEMENT_COLORS`).
const CHAT_ANNOUNCEMENT_COLORS: &[&str] = &["blue", "green", "orange", "purple", "primary"];
const CHAT_HTTP_ERROR_BODY_SNIPPET_MAX_CHARS: usize = 240;
const ANNOUNCEMENT_FALLBACK_SUCCESS_LABEL: &str =
    "Announcement nicht möglich, als normale Chat-Nachricht gesendet";

/// Bridge zwischen der internen API und dem nativen Chat-Send: sendet die
/// Owner-Chat-Action über den live rotierten Bot-User-Token ([`ChatApi`], das
/// den Token intern via [`BotTokenManager`] bezieht und bei 401 erneuert).
///
/// Broadcaster-Auflösung wie Python (`mixin.py:_dashboard_partner_chat_action`):
/// zuerst `twitch_partners_all_state.twitch_user_id`, sonst Helix-Login-Lookup.
struct ChatActionAdapter {
    api: Arc<dyn ChatApi>,
    pool: PgPool,
}

impl ChatActionAdapter {
    /// Broadcaster-User-ID zum Login: DB-Primärpfad, Helix-Fallback.
    async fn resolve_broadcaster_id(&self, login: &str) -> Option<String> {
        let row: Option<(Option<String>,)> = sqlx::query_as(
            "SELECT twitch_user_id FROM twitch_partners_all_state \
             WHERE LOWER(twitch_login) = $1 LIMIT 1",
        )
        .bind(login.to_lowercase())
        .fetch_optional(&self.pool)
        .await
        .unwrap_or(None);
        if let Some((Some(id),)) = row {
            let id = id.trim().to_string();
            if !id.is_empty() {
                return Some(id);
            }
        }
        // Fallback: Helix-Login-Lookup über den Bot-Token (resolve_user_id).
        match self.api.resolve_user_id(login).await {
            Ok(Some(id)) if !id.trim().is_empty() => Some(id),
            _ => None,
        }
    }
}

fn chat_http_error_reason(status: u16, body: &str) -> String {
    let body = body.trim();
    if body.is_empty() {
        return format!("helix http {status}");
    }
    let mut chars = body.chars();
    let snippet: String = chars
        .by_ref()
        .take(CHAT_HTTP_ERROR_BODY_SNIPPET_MAX_CHARS)
        .collect();
    if chars.next().is_some() {
        format!("helix http {status}: {snippet}...")
    } else {
        format!("helix http {status}: {snippet}")
    }
}

fn chat_action_result_from_send_outcome(
    login: &str,
    mode: &str,
    outcome: SendOutcome,
    sent_label_override: Option<&str>,
) -> ChatActionResult {
    match outcome {
        SendOutcome::Sent => {
            let label = sent_label_override.map(str::to_string).unwrap_or_else(|| {
                let label = if mode == "action" {
                    "Action"
                } else {
                    "Nachricht"
                };
                format!("{label} an {login} gesendet")
            });
            ChatActionResult::Sent { label }
        }
        SendOutcome::Dropped { code, message } => ChatActionResult::Dropped { code, message },
        SendOutcome::HttpError { status, body } => ChatActionResult::Failed {
            reason: chat_http_error_reason(status, &body),
            detail: Some(body),
        },
    }
}

async fn send_chat_message_action(
    api: &dyn ChatApi,
    broadcaster_id: &str,
    login: &str,
    mode: &str,
    send_text: &str,
    sent_label_override: Option<&str>,
) -> ChatActionResult {
    match api.send_message(broadcaster_id, send_text).await {
        Ok(outcome) => {
            chat_action_result_from_send_outcome(login, mode, outcome, sent_label_override)
        }
        Err(reason) => ChatActionResult::Failed {
            reason,
            detail: None,
        },
    }
}

async fn send_announcement_fallback(
    api: &dyn ChatApi,
    broadcaster_id: &str,
    login: &str,
    send_text: &str,
    original_failure: &str,
) -> ChatActionResult {
    match send_chat_message_action(
        api,
        broadcaster_id,
        login,
        "message",
        send_text,
        Some(ANNOUNCEMENT_FALLBACK_SUCCESS_LABEL),
    )
    .await
    {
        ChatActionResult::Failed { reason, detail } => ChatActionResult::Failed {
            reason: format!("announcement failed: {original_failure}; fallback failed: {reason}"),
            detail,
        },
        other => other,
    }
}

#[async_trait::async_trait]
impl ChatActionPort for ChatActionAdapter {
    async fn send_chat_action(
        &self,
        login: &str,
        mode: &str,
        color: &str,
        message: &str,
    ) -> ChatActionResult {
        // Modus/Farbe normalisieren (Python: unbekannt → message/purple).
        let mode = if CHAT_ACTION_MODES.contains(&mode) {
            mode
        } else {
            "message"
        };
        let color = if CHAT_ANNOUNCEMENT_COLORS.contains(&color) {
            color
        } else {
            "purple"
        };

        let Some(broadcaster_id) = self.resolve_broadcaster_id(login).await else {
            return ChatActionResult::UnknownChannel;
        };

        // Python: Action-Modus prefixt "/me " (Slash-Command im Chat).
        let send_text = if mode == "action" {
            format!("/me {message}")
        } else {
            message.to_string()
        };

        if mode == "announcement" {
            match self
                .api
                .send_announcement_detailed(&broadcaster_id, &send_text, color)
                .await
            {
                Ok(outcome) if outcome.accepted => ChatActionResult::Sent {
                    label: format!("Announcement an {login} gesendet"),
                },
                Ok(outcome) => {
                    let original_failure = outcome
                        .detail
                        .as_deref()
                        .unwrap_or("announcement not accepted");
                    send_announcement_fallback(
                        self.api.as_ref(),
                        &broadcaster_id,
                        login,
                        &send_text,
                        original_failure,
                    )
                    .await
                }
                Err(reason) => {
                    send_announcement_fallback(
                        self.api.as_ref(),
                        &broadcaster_id,
                        login,
                        &send_text,
                        &reason,
                    )
                    .await
                }
            }
        } else {
            send_chat_message_action(
                self.api.as_ref(),
                &broadcaster_id,
                login,
                mode,
                &send_text,
                None,
            )
            .await
        }
    }
}

/// Baut den [`ChatActionPort`] aus der gebooteten [`ChatApi`] + Pool. `None`,
/// wenn der native Chat aus ist (kein Bot-Token gebootet) → der Handler
/// antwortet dann 503 statt stumm zu scheitern.
pub fn build_chat_action_port(
    chat_api: Option<Arc<dyn ChatApi>>,
    pool: PgPool,
) -> Option<Arc<dyn ChatActionPort>> {
    let api = chat_api?;
    Some(Arc::new(ChatActionAdapter { api, pool }))
}

// ---------------------------------------------------------------------------
// Öffentlicher Einstieg
// ---------------------------------------------------------------------------

/// Phase 1: Bot-Token + ChatApi — wird VOR der Hooks-Komposition gebaut,
/// damit die OAuth-Followup-Begrüßung den nativen Send nutzen kann.
pub struct ChatApiHandle {
    api: Arc<dyn ChatApi>,
    pub bot_user_id: String,
    token_manager: Arc<BotTokenManager>,
    roster: Arc<DbPartnerRoster>,
}

impl ChatApiHandle {
    /// Standardzugriff: jede Schreibaktion läuft durch die Partner-Policy.
    pub fn api(&self) -> Arc<dyn ChatApi> {
        let roster: Arc<dyn PartnerRoster> = self.roster.clone();
        Arc::new(ChannelPolicyChatApi::new(
            Arc::clone(&self.api),
            PolicyContext::Standard(roster),
        ))
    }

    /// Bewusst abweichender Kontext, aktuell ausschließlich für den Raid-Pfad.
    pub fn api_for_context(&self, context: PolicyContext) -> Arc<dyn ChatApi> {
        Arc::new(ChannelPolicyChatApi::new(Arc::clone(&self.api), context))
    }

    /// Live rotierter Bot-User-Token-Manager — vom `!clip`-Fallback genutzt,
    /// bevor das Handle in die Pipeline verbaut wird.
    pub fn bot_token_manager(&self) -> Arc<BotTokenManager> {
        Arc::clone(&self.token_manager)
    }
}

/// Gebaute Chat-Laufzeit — `hooks` ersetzt die bisherigen EventSub-Hooks,
/// `start_background` startet alle Loops (Token-Refresh, Sub-Reconcile,
/// Promo-Loop, Global-Ban-Sweeper).
pub struct ChatRuntime {
    pub hooks: Arc<dyn EventSubHooks>,
    token_manager: Arc<BotTokenManager>,
    promos: Arc<PromoEngine>,
    sweeper: Arc<GlobalBanSweeper>,
    roster: Arc<DbPartnerRoster>,
    /// P2.14: nur über [`Self::spawn_partner_invite_backfill`] genutzt — bis
    /// main.rs den Spawn verdrahtet (WIRING-TODO), als tot markiert.
    #[allow(dead_code)]
    invite_resolver: Arc<DbInviteResolver>,
    /// Geteilt mit [`ChatHooks::subscriptions`] (P2.15): wird in
    /// [`Self::start_background`] mit dem aktiven SubscriptionManager befüllt,
    /// damit der chat.notification-Fallback die dedizierten EventSub-Subs kennt.
    subscriptions: Arc<OnceLock<Arc<SubscriptionManager>>>,
    pool: PgPool,
    bot_user_id: String,
    /// Geteilter TimeoutGuard (P2.57): die Composition-Root injiziert ihn via
    /// [`Self::timeout_guard`] in den TelemetryStore, damit inbound erkannte
    /// Bot-Self-Timeouts (`channel.ban` mit `ends_at`) dieselbe Stumm-Zählung
    /// füttern wie der ausgehende Send-Pfad.
    timeout_guard: Arc<TimeoutGuard>,
    supervisor: TaskSupervisor,
}

/// Phase 1: bootet den Bot-Token und baut die ChatApi, wenn `TB_CHAT_ENABLED=1`
/// und alle Voraussetzungen (Refresh-Token, Helix-Credentials) vorhanden sind.
/// `None` = Chat bleibt aus (Python bedient weiter).
pub async fn try_build_api(helix: Option<HelixClient>, pool: PgPool) -> Option<ChatApiHandle> {
    let enabled = std::env::var("TB_CHAT_ENABLED")
        .map(|v| v.trim() == "1")
        .unwrap_or(false);
    if !enabled {
        tracing::info!(
            "TB_CHAT_ENABLED nicht gesetzt — nativer Chat bleibt aus (Python-Chat aktiv)"
        );
        return None;
    }

    let Some(helix) = helix else {
        tracing::error!("TB_CHAT_ENABLED=1, aber kein HelixClient — Chat kann nicht starten");
        return None;
    };
    let (Ok(client_id), Ok(client_secret)) = (
        std::env::var("TWITCH_CLIENT_ID"),
        std::env::var("TWITCH_CLIENT_SECRET"),
    ) else {
        tracing::error!("TB_CHAT_ENABLED=1, aber TWITCH_CLIENT_ID/SECRET fehlen");
        return None;
    };
    let Ok(refresh_token) = std::env::var("TWITCH_BOT_REFRESH_TOKEN") else {
        tracing::error!("TB_CHAT_ENABLED=1, aber TWITCH_BOT_REFRESH_TOKEN fehlt");
        return None;
    };

    // Token-Boot: Access-Seed darf tot sein (Infisical-Stand), Refresh trägt.
    let token_manager = match BotTokenManager::new(client_id, client_secret) {
        Ok(m) => {
            let m = if let Some(writer) = tb_chat::InfisicalWriter::from_env() {
                m.with_sink(Arc::new(writer))
            } else {
                tracing::warn!(
                    "Bot-Token-Write-Back deaktiviert: INFISICAL_WRITE_TOKEN/Config fehlt — Env-Snapshot veraltet weiter"
                );
                m
            };
            Arc::new(m)
        }
        Err(e) => {
            tracing::error!("BotTokenManager nicht initialisierbar: {e}");
            return None;
        }
    };
    let seed_access = std::env::var("TWITCH_BOT_TOKEN").ok();
    if let Err(e) = token_manager
        .initialize(seed_access.as_deref(), &refresh_token)
        .await
    {
        tracing::error!("Bot-Token-Boot fehlgeschlagen: {e} — nativer Chat bleibt aus");
        return None;
    }
    let bot_user_id = token_manager.bot_user_id().await;
    let scopes = token_manager.scopes().await;
    tracing::info!(
        bot_login = %token_manager.bot_login().await,
        bot_user_id = %bot_user_id,
        scopes = %scopes.join(" "),
        "Chat-Bot-Token validiert"
    );
    for required in [
        "user:bot",
        "user:read:chat",
        "user:write:chat",
        "user:manage:whispers",
    ] {
        if !scopes.iter().any(|s| s == required) {
            tracing::warn!("Bot-Token ohne Scope {required} — Chat-Funktionen eingeschränkt");
        }
    }

    let api: Arc<dyn ChatApi> = Arc::new(HelixChatClient::new(
        Arc::new(helix),
        Arc::clone(&token_manager),
    ));
    Some(ChatApiHandle {
        api,
        bot_user_id,
        token_manager,
        roster: Arc::new(DbPartnerRoster { pool }),
    })
}

/// Phase 2: baut die komplette Pipeline auf der gebooteten ChatApi.
pub struct ChatRuntimePorts {
    pub manual_raid: Option<Arc<dyn tb_internal_api::ManualRaidPort>>,
    pub clip_port: Option<Arc<dyn ClipPort>>,
    pub bot_ban_handler: Option<Arc<dyn BotBannedChannelHandler>>,
    pub invite_relay: Option<BrokerRelay>,
    pub scam_notifier: Option<Arc<dyn ScamGuardNotifier>>,
    pub raid_greeting: Option<Arc<RaidGreetingMonitor>>,
}

pub async fn build_runtime(
    handle: ChatApiHandle,
    pool: PgPool,
    ports: ChatRuntimePorts,
    inner_hooks: Arc<dyn EventSubHooks>,
    supervisor: TaskSupervisor,
) -> ChatRuntime {
    let ChatRuntimePorts {
        manual_raid,
        clip_port,
        bot_ban_handler,
        invite_relay,
        scam_notifier,
        raid_greeting,
    } = ports;
    let ChatApiHandle {
        api,
        bot_user_id,
        token_manager,
        roster,
    } = handle;

    // TimeoutGuard verdrahten: zählt eigene Bot-Timeouts (Drop-Code
    // sender_banned/sender_timedout) und schaltet bei 2/Tag bzw. 5/Woche für
    // 7 Tage stumm (Port: timeout_guard.py). Die ChatApi wird EINMAL dekoriert,
    // BEVOR sie an die Komponenten verteilt wird — so läuft jeder ausgehende
    // send_message (Moderation/Promos/Commands/Scam-Pitch/Fun/Pipeline/
    // Mention-Resolver) durch das Tracking.
    let timeout_guard = Arc::new(TimeoutGuard::new());
    let tracked_api: Arc<dyn ChatApi> = Arc::new(
        TimeoutTrackingChatApi::new(api, Arc::clone(&timeout_guard), pool.clone())
            .with_bot_ban_handler(bot_ban_handler),
    );
    let policy_roster: Arc<dyn PartnerRoster> = roster.clone();
    let api: Arc<dyn ChatApi> = Arc::new(ChannelPolicyChatApi::new(
        tracked_api,
        PolicyContext::Standard(policy_roster),
    ));

    // Lern-Muster einmalig laden (Python lädt sie beim Bot-Start).
    let learned = LearnedPatterns::load(&pool).await;
    let crew_centroid = Arc::new(
        match build_centroid(&pool, &tb_chat::crew_guard::evidence_logins()).await {
            Ok(centroid) => centroid,
            Err(error) => {
                tracing::warn!(%error, "crew_guard: Stil-Zentroid konnte nicht gebaut werden");
                Centroid::default()
            }
        },
    );
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .unwrap_or_default();

    // Promo-Suppression kombiniert die bestehende DB-Suppression
    // (twitch_outbound_chat_suppressions, Quelle "promo") mit dem In-Memory-
    // TimeoutGuard: der Promo-Pfad sendet weder in DB-stummgeschaltete noch in
    // per Bot-Timeout stummgeschaltete Kanäle (Port: promos.py:1137 prüft
    // timeout_guard.is_muted vor jedem Promo-Send).
    let suppression: Arc<dyn tb_chat::promos::OutboundSuppressionCheck> =
        Arc::new(CombinedSuppression::new(
            Arc::new(OutboundSuppressionStore::new(pool.clone())),
            Arc::clone(&timeout_guard),
        ));
    let moderation = Arc::new(
        ModerationEngine::new(Arc::clone(&api), pool.clone())
            .with_notice_suppression(Arc::clone(&suppression)),
    );
    let mut conversation_scam = ConversationScamGuard::new(
        pool.clone(),
        bot_user_id.clone(),
        Arc::new(MiniMaxScamJudge::new(EngagementMinimaxClient::new(
            None, None, None, None,
        ))),
        Arc::clone(&api),
        Arc::clone(&moderation),
    );
    if let Some(notifier) = scam_notifier {
        conversation_scam = conversation_scam.with_notifier(notifier);
    }
    let conversation_scam = Arc::new(conversation_scam);
    // Promo-Invite-Resolver: lazy On-Miss-Erstellung (PromoEngine) UND eager
    // Backfill beim Startup (P2.14) teilen sich denselben Resolver, damit beide
    // Pfade dieselbe Broker-/DB-Logik nutzen.
    let invite_resolver = Arc::new(DbInviteResolver {
        pool: pool.clone(),
        relay: invite_relay,
        invite_channel_id: invite_channel_id_from_env(),
    });
    let promos = Arc::new(
        PromoEngine::new(pool.clone(), Arc::clone(&api), Arc::clone(&suppression))
            // P1.1: Schreibseite der Outbound-Suppression — derselbe Store, der
            // bereits als Read-Seite in CombinedSuppression hängt. channel_settings-
            // Drops werden so 7d/3d persistiert (promo/recruitment 7d, partner_raid 3d).
            .set_suppression_writer(Arc::new(OutboundSuppressionStore::new(pool.clone())))
            // P1.4: Bot-Token-Scope-Quelle für den Lurker-Tax-Fallback. Der zentrale
            // BotTokenManager implementiert BotScopeProvider — bot-zentrierter
            // moderator:read:chatters-Scope greift, wenn der Streamer-Scope fehlt.
            .set_bot_scope_provider(
                Arc::clone(&token_manager) as Arc<dyn tb_chat::promos::BotScopeProvider>
            )
            .set_invite_resolver(Arc::clone(&invite_resolver) as Arc<dyn InviteResolver>)
            .set_partner_check(Arc::new(DbPartnerCheck { pool: pool.clone() }))
            .set_preset_picker(Arc::new(MinimaxPresetPicker::new(
                EngagementMinimaxClient::new(None, None, None, None),
            ))),
    );

    let discord_link: Arc<dyn DiscordLinkPort> = Arc::new(DbDiscordLink { pool: pool.clone() });
    let mut command_engine = CommandEngine::new(
        pool.clone(),
        Arc::clone(&api),
        Arc::new(RaidCommandAdapter {
            manual: manual_raid,
            pool: pool.clone(),
        }),
        Arc::clone(&discord_link),
        Arc::new(DbInvitePort { pool: pool.clone() }),
        Arc::new(DbSuperMod { pool: pool.clone() }),
        Arc::clone(&moderation) as Arc<dyn LastAutobanStore>,
    );
    if let Some(cp) = clip_port {
        command_engine = command_engine.set_clip_port(cp);
    }
    command_engine = command_engine.set_scam_port(Arc::new(ScamGuardCommands::new(
        pool.clone(),
        EngagementMinimaxClient::new(None, None, None, None),
    )));
    command_engine = command_engine.set_invite_reply_notifier(
        Arc::clone(&promos) as Arc<dyn tb_chat::commands::InviteReplyNotifier>
    );
    let commands = Arc::new(command_engine);

    let review_log_dir =
        std::env::var("TB_CHAT_REVIEW_LOG_DIR").unwrap_or_else(|_| "logs".to_string());

    // Spam-Filter halten, damit ein Hintergrund-Task die gelernten Muster
    // periodisch neu laden kann (Python-Cache-TTL 120 s). Ohne Reload griffen
    // KI-neu-gelernte Spam-/Safe-Muster im nativen Betrieb erst nach Neustart.
    let spam_filter = Arc::new(SpamFilter::new(learned));
    {
        let spam_filter = Arc::clone(&spam_filter);
        let pool = pool.clone();
        supervisor.spawn("chat_spam_filter_reload", async move {
            let mut tick = tokio::time::interval(Duration::from_secs(120));
            tick.tick().await; // erster Tick feuert sofort — überspringen (frisch geladen)
            loop {
                tick.tick().await;
                spam_filter.reload(&pool).await;
            }
        });
    }

    let pipeline = Arc::new(ChatPipeline::new(ChatPipelineParts {
        bot_user_id: bot_user_id.clone(),
        api: Arc::clone(&api),
        pool: pool.clone(),
        classifier: Arc::new(ChannelClassifier::new(pool.clone())),
        tracker: Arc::new(ChatterTracker::new(pool.clone())),
        global_ban: Arc::new(GlobalChatterBanEnforcer::new(pool.clone())),
        scam_pitch: Arc::new(ScamPitchDetector::new(
            Arc::clone(&api),
            Arc::new(HelixAccountAge {
                api: Arc::clone(&api),
            }),
            pool.clone(),
        )),
        conversation_scam,
        spam_filter: Arc::clone(&spam_filter),
        ai_reviewer: Arc::new(SpamAiReviewer::new(pool.clone(), http.clone())),
        moderation,
        sus_invite: Arc::new(SusInviteCheck::new(pool.clone())),
        // _fun_thanks_reply_enabled ist in Python default false (bot.py Z. 190).
        fun: Arc::new(FunResponses::new(Arc::clone(&api), false)),
        invite_question: Arc::new(InviteQuestionResponder::new(
            Arc::clone(&api),
            Arc::new(DbInviteUrlWithFallback { pool: pool.clone() }),
            Arc::new(PgInviteQuestionStore::new(pool.clone())),
            Arc::new(MiniMaxInviteQuestionJudge::new(
                EngagementMinimaxClient::new(None, None, None, None),
            )),
            Some(Arc::clone(&promos) as Arc<dyn tb_chat::commands::PromoBlockCheck>),
            Some(Arc::clone(&promos) as Arc<dyn tb_chat::commands::InviteReplyNotifier>),
        )),
        lfg_pitch: Arc::new(LfgPitchResponder::new(
            Arc::clone(&api),
            Arc::new(DbInviteUrlWithFallback { pool: pool.clone() }),
            Arc::new(MiniMaxLfgJudge::new(EngagementMinimaxClient::new(
                None, None, None, None,
            ))),
            lfg_pitch_enabled_from_env(),
            Some(Arc::clone(&promos) as Arc<dyn tb_chat::commands::PromoBlockCheck>),
            Some(Arc::clone(&promos) as Arc<dyn tb_chat::commands::InviteReplyNotifier>),
        )),
        promos: Arc::clone(&promos),
        commands,
        mention_resolver: Arc::new(PgHelixMentionResolver::new(pool.clone(), Arc::clone(&api))),
        review_log: Arc::new(ReviewLog::new(review_log_dir)),
        alerter: Arc::new(ModAlerter::new(http)),
        crew_centroid,
    }));

    let sweeper = Arc::new(GlobalBanSweeper::new(pool.clone(), Arc::clone(&api)));
    // Engagement-Layer (KI-Stammgast): Der EventSub-Partnerpfad darf weiterhin
    // über den separaten Smoke-Account antworten. Der anonyme IRC-Reader weiter
    // unten besitzt bewusst keinen Sender.
    // Die sieben Background-Jobs (Thread-Extractor, Match-Poller, …) laufen
    // unabhängig vom Sende-Account (Python `ensure_started`).
    let engagement = Arc::new(EngagementPipeline::with_defaults(
        pool.clone(),
        EngagementMinimaxClient::new(None, None, None, None),
    ));
    let stealth = build_engagement_stealth(pool.clone()).await;
    if stealth.is_none() {
        tracing::warn!(
            "Engagement: kein Stealth-Sender (DB_MASTER_KEY_V1/TWITCH_CLIENT_ID fehlt) — \
             AI-Antworten werden verworfen, Background-Jobs laufen trotzdem"
        );
    }
    tb_engagement::background::spawn_all(pool.clone());

    // IRC-Reader: zweiter Chat-Input für `irc_read`-Kanäle (einwilligende
    // Streamer OHNE EventSub-`channel:bot`). Disjunkte Kanal-Menge zum
    // EventSub-Pfad → kein Doppel-Processing. No-op, wenn keine irc_read-Kanäle.
    let engagement_irc_reader = EngagementIrcReader::new(pool.clone(), Arc::clone(&engagement));
    supervisor.spawn("engagement_irc_reader", async move {
        engagement_irc_reader.run().await;
        future::pending::<()>().await;
    });

    // P2.15: geteilte Zelle für den SubscriptionManager (erst in
    // start_background bekannt). ChatHooks und ChatRuntime halten denselben Arc.
    let subscriptions_cell: Arc<OnceLock<Arc<SubscriptionManager>>> = Arc::new(OnceLock::new());

    tracing::info!("Nativer Chat-Bot verdrahtet — Pipeline aktiv (TB_CHAT_ENABLED=1)");
    ChatRuntime {
        hooks: Arc::new(ChatHooks {
            inner: inner_hooks,
            pipeline,
            api: Arc::clone(&api),
            pool: pool.clone(),
            timeout_guard: Arc::clone(&timeout_guard),
            promos: Arc::clone(&promos),
            engagement,
            stealth,
            bot_user_id: bot_user_id.clone(),
            telemetry: TelemetryStore::new(pool.clone()),
            subscriptions: Arc::clone(&subscriptions_cell),
            raid_greeting,
        }),
        token_manager,
        promos,
        sweeper,
        roster,
        invite_resolver,
        subscriptions: subscriptions_cell,
        pool,
        bot_user_id,
        timeout_guard,
        supervisor,
    }
}

fn invite_channel_id_from_env() -> Option<u64> {
    std::env::var("TWITCH_NOTIFY_CHANNEL_ID")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|&value| value > 0)
}

impl ChatRuntime {
    /// Zentraler Bot-User-ID-Wert (P2.57): identisch mit dem `target_id`, den
    /// die inbound `channel.ban`-Telemetrie gegen den Bot prüft.
    pub fn bot_user_id(&self) -> String {
        self.bot_user_id.clone()
    }

    /// Geteilter [`TimeoutGuard`] (P2.57): füttert sowohl den ausgehenden
    /// Send-Pfad als auch — nach Injection in den TelemetryStore — die inbound
    /// erkannten Bot-Self-Timeouts.
    pub fn timeout_guard(&self) -> Arc<TimeoutGuard> {
        Arc::clone(&self.timeout_guard)
    }

    /// Startet alle Hintergrund-Loops: Token-Refresh (30 min), Promo-Loop
    /// (60 s), Global-Ban-Sweeper (120 s + 6-Uhr-Vollsweep) und den
    /// Chat-Subscription-Reconcile (Start + alle 30 min — der Python-Join).
    pub fn start_background(
        &self,
        subscriptions: Option<Arc<SubscriptionManager>>,
        reconcile_now: Arc<tokio::sync::Notify>,
    ) {
        self.token_manager.spawn_refresh_loop();
        Arc::clone(&self.promos).spawn_periodic_loop();
        Arc::clone(&self.sweeper).spawn(Arc::clone(&self.roster) as Arc<dyn PartnerRoster>);

        let Some(manager) = subscriptions else {
            // Ohne Manager gibt es keinen Reconcile-Loop; Signale dürfen verfallen.
            tracing::warn!(
                "Kein SubscriptionManager — Chat-Subscriptions werden nicht angelegt \
                 (Webhook-Config/Helix fehlt)"
            );
            return;
        };
        // P2.15: Hooks den aktiven SubscriptionManager bekanntmachen, damit der
        // chat.notification-Fallback dedizierte EventSub-Subs erkennt.
        let _ = self.subscriptions.set(Arc::clone(&manager));
        let pool = self.pool.clone();
        let bot_user_id = self.bot_user_id.clone();
        let token_manager = Arc::clone(&self.token_manager);
        self.supervisor
            .spawn("chat_subscription_reconcile", async move {
                let mut tick = tokio::time::interval(CHAT_SUB_RECONCILE_INTERVAL);
                tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                loop {
                    tokio::select! {
                        _ = tick.tick() => {}
                        _ = reconcile_now.notified() => {}
                    }
                    reconcile_chat_subscriptions(&manager, &pool, &bot_user_id, &token_manager)
                        .await;
                }
            });
    }

    /// P2.14: Startet den einmaligen Eager-Partner-Invite-Backfill als
    /// Hintergrund-Task (Python `_ensure_partner_invites`, beim Start gespawnt).
    /// Läuft einmal beim Boot durch (inkl. 60-s-Retry der Fehlschläge) und endet
    /// dann — der laufende Promo-Pfad erstellt fehlende Invites weiter on-miss.
    ///
    /// WIRING-TODO(P2.14): In `bin/tb-bot/src/main.rs` nach dem ChatRuntime-Aufbau
    /// (gleiche Stelle wie `start_background`) einmalig `runtime
    /// .spawn_partner_invite_backfill()` aufrufen.
    #[allow(dead_code)]
    pub fn spawn_partner_invite_backfill(&self) {
        let resolver = Arc::clone(&self.invite_resolver);
        tokio::spawn(async move {
            resolver.ensure_partner_invites().await;
        });
    }
}

/// Gemeinsamer Broadcaster-Roster für EventSub-Wartung und Chat-Reconcile.
/// `active_ids` aus diesem Roster schützt genau die Broadcaster, für die einer
/// der Rust-Reconcile-Pfade Subscriptions halten darf.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EventSubSubscriptionBroadcaster {
    pub(crate) login: String,
    pub(crate) twitch_user_id: String,
    pub(crate) is_partner: bool,
    pub(crate) core_subscriptions: bool,
    pub(crate) chat_subscriptions: bool,
}

/// Source of Truth für Subscription-Broadcaster:
/// - Core/Event-Lifecycle: aktive Partner plus monitored-only Kanäle.
/// - Chat-Reconcile: `(raid_enabled OR is_partner_active)` mit gültigem
///   `channel:bot`-Grant und ohne Reauth-Block.
pub(crate) async fn select_eventsub_subscription_broadcasters(
    pool: &PgPool,
) -> Result<Vec<EventSubSubscriptionBroadcaster>, sqlx::Error> {
    let rows: Vec<(String, String, bool, bool, bool)> = sqlx::query_as(
        r#"
        WITH roster AS (
            SELECT LOWER(ps.twitch_login) AS login,
                   ps.twitch_user_id AS twitch_user_id,
                   TRUE AS core_subscriptions,
                   (COALESCE(ps.is_partner_active, 0) = 1) AS is_partner,
                   FALSE AS chat_subscriptions
              FROM twitch_streamers_partner_state ps
             WHERE COALESCE(ps.is_partner_active, 0) = 1
               AND COALESCE(ps.twitch_user_id, '') <> ''

            UNION ALL

            SELECT LOWER(s.twitch_login) AS login,
                   s.twitch_user_id AS twitch_user_id,
                   TRUE AS core_subscriptions,
                   FALSE AS is_partner,
                   FALSE AS chat_subscriptions
              FROM twitch_streamers s
             WHERE COALESCE(s.twitch_user_id, '') <> ''
               AND NOT EXISTS (
                   SELECT 1
                     FROM twitch_partners p
                    WHERE p.twitch_user_id = s.twitch_user_id
                       OR LOWER(p.twitch_login) = LOWER(s.twitch_login)
               )

            UNION ALL

            SELECT LOWER(ps.twitch_login) AS login,
                   ps.twitch_user_id AS twitch_user_id,
                   FALSE AS core_subscriptions,
                   FALSE AS is_partner,
                   TRUE AS chat_subscriptions
              FROM twitch_streamers_partner_state ps
              JOIN twitch_raid_auth ra ON ra.twitch_user_id = ps.twitch_user_id
             WHERE (ra.raid_enabled IS TRUE OR COALESCE(ps.is_partner_active, 0) = 1)
               AND COALESCE(ps.twitch_user_id, '') <> ''
               AND ra.needs_reauth = FALSE
               AND COALESCE(ra.scopes, '') LIKE '%channel:bot%'
        )
        SELECT login,
               twitch_user_id,
               BOOL_OR(is_partner) AS is_partner,
               BOOL_OR(core_subscriptions) AS core_subscriptions,
               BOOL_OR(chat_subscriptions) AS chat_subscriptions
          FROM roster
         WHERE COALESCE(login, '') <> ''
           AND COALESCE(twitch_user_id, '') <> ''
         GROUP BY login, twitch_user_id
         ORDER BY login
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(
            |(login, twitch_user_id, is_partner, core_subscriptions, chat_subscriptions)| {
                EventSubSubscriptionBroadcaster {
                    login,
                    twitch_user_id,
                    is_partner,
                    core_subscriptions,
                    chat_subscriptions,
                }
            },
        )
        .collect())
}

/// Kanal-Auswahl für die Chat-Subscriptions (Python `join_partner_channels`,
/// connection.py:2050-2051). Filtert aus dem gemeinsamen Subscription-Roster,
/// damit Cleanup und Chat-Reconcile dieselbe Broadcaster-Definition teilen.
async fn select_chat_subscription_channels(
    pool: &PgPool,
) -> Result<Vec<(String, String)>, sqlx::Error> {
    Ok(select_eventsub_subscription_broadcasters(pool)
        .await?
        .into_iter()
        .filter(|row| row.chat_subscriptions)
        .map(|row| (row.login, row.twitch_user_id))
        .collect())
}

/// „Join" im Webhook-Modell: für jeden Partner- und Monitored-Kanal die
/// `channel.chat.message`/`channel.chat.notification`-Subscriptions sicherstellen.
/// Nur Kanäle mit erteiltem `channel:bot`-Scope erhalten Chat-Subscriptions —
/// Python-Parität: `join_partner_channels()` filtert via INNER JOIN auf
/// `twitch_raid_auth` + `"channel:bot" in scopes`. Kanäle ohne Grant werden
/// nie erst versucht, wodurch das 403-Rauschen entfällt.
async fn reconcile_chat_subscriptions(
    manager: &SubscriptionManager,
    pool: &PgPool,
    bot_user_id: &str,
    token_manager: &BotTokenManager,
) {
    let rows: Vec<(String, String)> = match select_chat_subscription_channels(pool).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("chat-sub-reconcile: Kanal-Query fehlgeschlagen: {e}");
            return;
        }
    };

    let mut ok = 0usize;
    let mut failed = 0usize;
    let mut blocked = 0usize;
    for (login, broadcaster_id) in &rows {
        if manager
            .ensure_chat_subscriptions(broadcaster_id, bot_user_id, login)
            .await
        {
            ok += 1;
        } else if manager.chat_subscriptions_permanently_blocked(broadcaster_id) {
            blocked += 1;
        } else {
            failed += 1;
        }
    }
    tracing::info!(
        kanäle = rows.len(),
        ok,
        blocked,
        failed,
        "chat-sub-reconcile abgeschlossen"
    );

    // Bot-Token-Subs pro Partner-Kanal (alle brauchen den Bot-User-Token):
    //  - channel.moderate (Guard-Quelle für den BlacklistRaidGuard, eventsub_mixin.py:1711)
    //  - channel.chat.user_first_message (First-Message-Funnel, B5-01, :2692)
    //  - channel.follow/ban/unban/shoutout (Daten-Telemetrie, B5-02, moderator_subs :1704)
    // Möglich, seit Rust den Bot-Token allein refresht (Python-Chat abgeschaltet).
    // Kanäle ohne Bot-Moderator → 403 → perm_failed (kein Retry-Spam). Scope-Filter
    // pro Sub-Typ greift in den Manager-Methoden (fehlt der Scope → übersprungen).
    let scopes = token_manager.scopes().await;
    let bot_token = match token_manager.access_token().await {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("Bot-Token-Sub-Reconcile: Bot-Token nicht verfügbar: {e}");
            return;
        }
    };

    let has_moderate = scopes.iter().any(|s| s == "channel:moderate");
    if !has_moderate {
        tracing::debug!(
            "channel.moderate-Reconcile übersprungen: Bot-Token ohne channel:moderate-Scope"
        );
    }
    let mut mod_ok = 0usize;
    let mut first_msg_ok = 0usize;
    let mut mod_telemetry = 0usize;
    for (login, broadcaster_id) in &rows {
        let chat_blocked = manager.chat_subscriptions_permanently_blocked(broadcaster_id);
        let moderator_subscription_ok = !chat_blocked
            && has_moderate
            && manager
                .ensure_moderator_subscription(broadcaster_id, bot_user_id, &bot_token, login)
                .await;
        if moderator_subscription_ok {
            mod_ok += 1;
        }
        // B5-01: First-Message-Subscription (braucht user:read:chat — der Bot
        // hat denselben Token wie für die Chat-Subs, der Scope ist also gegeben).
        if !chat_blocked
            && manager
                .ensure_first_message_subscription(broadcaster_id, bot_user_id, &bot_token, login)
                .await
        {
            first_msg_ok += 1;
        }
        // B5-02: Moderator-Daten-Telemetrie (follow/ban/unban/shoutout). Scope-
        // Filter in der Methode überspringt fehlende Scopes still. Wenn der
        // channel.moderate-Guard mit demselben Bot-Token 403 liefert, ist der
        // Bot für diesen Kanal nicht als Moderator nutzbar; dann keinen
        // zusätzlichen Bot-Token-Versuch für ban/unban/shoutout/follow feuern.
        // Der Broadcaster-Fallback bleibt aktiv, weil er einen anderen
        // moderator_user_id nutzt.
        let (telemetry_bot_user_id, telemetry_bot_token, telemetry_scopes): (
            &str,
            &str,
            &[String],
        ) = if chat_blocked || (has_moderate && !moderator_subscription_ok) {
            ("", "", &[])
        } else {
            (bot_user_id, bot_token.as_str(), scopes.as_slice())
        };
        mod_telemetry += manager
            .ensure_moderator_telemetry_subscriptions(
                broadcaster_id,
                telemetry_bot_user_id,
                telemetry_bot_token,
                telemetry_scopes,
                login,
            )
            .await;
    }
    tracing::info!(
        kanäle = rows.len(),
        mod_ok,
        first_msg_ok,
        mod_telemetry,
        "Bot-Token-Sub-Reconcile abgeschlossen"
    );
}

// ---------------------------------------------------------------------------
// EventSubHooks-Wrapper — delegiert alles, fängt channel.chat.message ab
// ---------------------------------------------------------------------------

struct ChatHooks {
    inner: Arc<dyn EventSubHooks>,
    pipeline: Arc<ChatPipeline>,
    /// Go-Live-Tipp-Hook: nutzt denselben dekorierten Chat-Sendepfad wie die
    /// übrige Pipeline und liest Gates/Live-State aus Postgres.
    api: Arc<dyn ChatApi>,
    pool: PgPool,
    /// Für den Werbefrei-Pitch beim Go-Live: der geteilte TimeoutGuard
    /// (SET-Seite armiert den Pitch) + die PromoEngine, über die der Pitch
    /// gesendet wird (Suppression-Check + Promo-Cooldown, Python-Parität).
    timeout_guard: Arc<TimeoutGuard>,
    promos: Arc<PromoEngine>,
    /// KI-Engagement-Pipeline für den autorisierten EventSub-Partnerpfad.
    engagement: Arc<EngagementPipeline>,
    stealth: Option<Arc<StealthSender>>,
    /// User-ID des zentralen Bots — eigene Nachrichten überspringen das
    /// Engagement (Python `event_message`: `message.echo` → return).
    bot_user_id: String,
    /// P2.15: Sub-Telemetrie-Schreibpfad für den chat.notification-Fallback.
    telemetry: TelemetryStore,
    /// P2.15: geteilter Griff auf den SubscriptionManager, erst in
    /// [`ChatRuntime::start_background`] gesetzt (vorher unbekannt). Über
    /// `tracked_pairs` wird geprüft, ob für den Broadcaster bereits eine
    /// dedizierte channel.subscribe*-EventSub existiert (dann KEIN Fallback —
    /// Doppel-Count vermeiden). Leer/ungesetzt → Fallback an (Python: kein
    /// `has_sub`-Checker ⇒ `return True`).
    subscriptions: Arc<OnceLock<Arc<SubscriptionManager>>>,
    raid_greeting: Option<Arc<RaidGreetingMonitor>>,
}

impl ChatHooks {
    /// Spawnt die Engagement-Verarbeitung für autorisierte EventSub-Partner.
    fn spawn_engagement(&self, event: &ChatMessageEvent) {
        // Eigene (zentrale) Bot-Nachrichten überspringen.
        if event.chatter_user_id == self.bot_user_id {
            return;
        }
        let engagement = Arc::clone(&self.engagement);
        let stealth = self.stealth.clone();
        let broadcaster_id = event.broadcaster_user_id.clone();
        let msg = IncomingMessage {
            channel_login: event.broadcaster_user_login.to_lowercase(),
            twitch_user_id: event.chatter_user_id.clone(),
            twitch_login: event.chatter_user_login.clone(),
            content: event.message.text.clone(),
            message_id: Some(event.message_id.clone()),
        };
        tokio::spawn(async move {
            let result = engagement.handle(&msg).await;
            let Some(text) = result.response_text else {
                return;
            };
            match &stealth {
                Some(sender)
                    if sender
                        .send(&broadcaster_id, &msg.channel_login, &text)
                        .await
                        .is_none() =>
                {
                    tracing::info!(
                        "Engagement: kein Sende-Account onboarded, AI-Antwort verworfen"
                    );
                }
                Some(_) => {}
                None => {
                    tracing::debug!("Engagement: Stealth-Sender nicht verfügbar, Antwort verworfen")
                }
            }
        });
    }

    async fn maybe_send_golive_tip(&self, twitch_user_id: &str, login: &str) {
        // Go-Live-Tipps sind temporär global deaktiviert (GH #565): Die Tipp-Texte
        // sind inhaltlich zu schwach ("das wusste ich schon") und werden überarbeitet.
        // Bis dahin bleibt nur der Versand gesperrt — Auswahl-/Gate-/Persistenz-Logik
        // darunter ist unverändert. Reaktivierung ohne Rebuild via TB_GOLIVE_TIPS_ENABLED=1.
        if std::env::var("TB_GOLIVE_TIPS_ENABLED").as_deref() != Ok("1") {
            return;
        }

        let last_game = match sqlx::query_as::<_, (Option<String>,)>(
            "SELECT last_game FROM twitch_live_state WHERE twitch_user_id = $1",
        )
        .bind(twitch_user_id)
        .fetch_optional(&self.pool)
        .await
        {
            Ok(Some((last_game,))) => last_game.unwrap_or_default(),
            Ok(None) => return,
            Err(error) => {
                tracing::warn!(
                    %error,
                    %twitch_user_id,
                    %login,
                    "go-live-tipp: Deadlock-Check fehlgeschlagen"
                );
                return;
            }
        };
        if last_game.trim().to_lowercase() != "deadlock" {
            return;
        }

        let settings = match tb_tips::repo::tip_settings(&self.pool, twitch_user_id).await {
            Ok(settings) => settings,
            Err(error) => {
                tracing::warn!(
                    %error,
                    %twitch_user_id,
                    %login,
                    "go-live-tipp: Settings konnten nicht gelesen werden"
                );
                return;
            }
        };
        if !tb_tips::engine::passes_gates(
            &settings,
            chrono::Utc::now(),
            tb_tips::engine::MIN_GAP_HOURS,
        ) {
            return;
        }

        let pick =
            match tb_tips::engine::pick_tip(&self.pool, knowledge_base(), twitch_user_id).await {
                Ok(pick) => pick,
                Err(error) => {
                    tracing::warn!(
                        %error,
                        %twitch_user_id,
                        %login,
                        "go-live-tipp: Tipp-Auswahl fehlgeschlagen"
                    );
                    return;
                }
            };
        let Some((slug, tip_text)) = pick else {
            return;
        };

        match self.api.send_message(twitch_user_id, &tip_text).await {
            Ok(SendOutcome::Sent) => {
                match tb_tips::repo::record_tip_shown(&self.pool, twitch_user_id, &slug).await {
                    Ok(()) => tracing::info!(
                        %twitch_user_id,
                        %login,
                        %slug,
                        "go-live-tipp gesendet"
                    ),
                    Err(error) => tracing::warn!(
                        %error,
                        %twitch_user_id,
                        %login,
                        %slug,
                        "go-live-tipp: Historie konnte nicht geschrieben werden"
                    ),
                }
                if let Err(error) =
                    tb_tips::repo::record_feature_used(&self.pool, twitch_user_id, &slug).await
                {
                    tracing::warn!(
                        %error,
                        %twitch_user_id,
                        %login,
                        %slug,
                        "go-live-tipp: Feature-Nutzung konnte nicht geschrieben werden"
                    );
                }
            }
            Ok(SendOutcome::Dropped { code, message }) => tracing::warn!(
                %twitch_user_id,
                %login,
                %slug,
                %code,
                reason = %message,
                "go-live-tipp von Twitch gedroppt"
            ),
            Ok(SendOutcome::HttpError { status, .. }) => tracing::warn!(
                %twitch_user_id,
                %login,
                %slug,
                %status,
                "go-live-tipp: Chat-Send HTTP-Fehler"
            ),
            Err(error) => tracing::warn!(
                %error,
                %twitch_user_id,
                %login,
                %slug,
                "go-live-tipp: Chat-Send fehlgeschlagen"
            ),
        }
    }
}

#[async_trait::async_trait]
impl EventSubHooks for ChatHooks {
    async fn on_channel_raid(&self, event: &Value, message_id: Option<&str>) {
        self.inner.on_channel_raid(event, message_id).await;
    }
    async fn on_channel_moderate(&self, broadcaster_id: &str, login: &str, event: &Value) {
        self.inner
            .on_channel_moderate(broadcaster_id, login, event)
            .await;
    }
    async fn on_stream_went_live(&self, twitch_user_id: &str, login: &str) {
        self.on_stream_went_live_with_stream_id(twitch_user_id, login, None)
            .await;
    }

    async fn on_stream_went_live_with_stream_id(
        &self,
        twitch_user_id: &str,
        login: &str,
        stream_id: Option<&str>,
    ) {
        self.inner
            .on_stream_went_live_with_stream_id(twitch_user_id, login, stream_id)
            .await;
        self.maybe_send_golive_tip(twitch_user_id, login).await;

        // Werbefrei-Pitch (Python eventsub_mixin.py:1523-1555): War der Bot in
        // diesem Kanal getimed-outed (TimeoutGuard-SET), ist beim Stream-Start
        // ein Pitch fällig → 90 s nach Go-Live einmalig senden. consume_* setzt
        // zugleich den Pitch-Cooldown, ist also idempotent pro Stream-Start. Der
        // Send läuft über die PromoEngine (Suppression-Check + Promo-Cooldown),
        // damit ein gemuteter Kanal verschont bleibt und nicht direkt danach eine
        // reguläre Promo feuert (Python-Parität source="promo" + mark_promo_sent).
        if self.timeout_guard.consume_stream_start_pitch(login) {
            let promos = Arc::clone(&self.promos);
            let broadcaster_id = twitch_user_id.to_string();
            let login = login.to_string();
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(90)).await;
                if promos
                    .send_timeout_pitch(&broadcaster_id, &login, WERBEFREI_PITCH_MSG)
                    .await
                {
                    tracing::info!(login = %login, "Werbefrei-Pitch nach Stream-Start gesendet");
                } else {
                    tracing::debug!(login = %login, "Werbefrei-Pitch nicht gesendet (Suppression/Drop)");
                }
            });
        }
    }
    async fn on_score_refresh(
        &self,
        twitch_user_id: &str,
        login: Option<&str>,
        trigger: &'static str,
    ) {
        self.inner
            .on_score_refresh(twitch_user_id, login, trigger)
            .await;
    }
    async fn on_stream_offline(&self, twitch_user_id: &str, login: Option<&str>) {
        self.inner.on_stream_offline(twitch_user_id, login).await;
    }
    async fn on_chat_message(&self, event: &Value, message_id: Option<&str>) {
        self.inner.on_chat_message(event, message_id).await;
        match serde_json::from_value::<ChatMessageEvent>(event.clone()) {
            Ok(chat_event) => {
                if let Some(monitor) = &self.raid_greeting {
                    monitor.observe_chat(&chat_event);
                }
                if self.pipeline.handle(&chat_event).await {
                    self.spawn_engagement(&chat_event);
                }
            }
            Err(e) => tracing::warn!("chat.message nicht deserialisierbar: {e}"),
        }
    }

    // B7: chat.notification-Raid/Unraid an die Raid-Schicht durchreichen (der
    // Demux in tb-monitoring klassifiziert vorab nach notice_type).
    async fn on_chat_raid_notification(&self, event: &Value, message_id: Option<&str>) {
        self.inner
            .on_chat_raid_notification(event, message_id)
            .await;
    }
    async fn on_chat_unraid_notification(&self, event: &Value, message_id: Option<&str>) {
        self.inner
            .on_chat_unraid_notification(event, message_id)
            .await;
    }
    // P2.15: Chat-notification-Sub/Resub/Gift-Telemetrie-Fallback. Python
    // (raid/bot.py:422-453) persistiert chat-abgeleitete Sub-Notices NUR, wenn
    // für den Broadcaster KEINE dedizierte channel.subscribe*-EventSub existiert
    // (should_capture-Gate, raid/bot.py:396-420) — sonst Doppel-Count. Wir bauen
    // aus dem rohen chat.notification-Event dasselbe normalisierte Event wie der
    // native channel.subscribe-Pfad und schreiben über store_subscription_event.
    async fn on_chat_subscription_notification(
        &self,
        kind: ChatNotificationKind,
        event: &Value,
        message_id: Option<&str>,
    ) {
        self.inner
            .on_chat_subscription_notification(kind, event, message_id)
            .await;

        let Some((event_type, normalized)) = chat_notification_to_subscription_event(kind, event)
        else {
            return;
        };
        let broadcaster_id = str_field(event, &["broadcaster_user_id"]).unwrap_or_default();
        if broadcaster_id.is_empty() {
            tracing::debug!(
                "chat.notification-Sub-Fallback: kein broadcaster_user_id — übersprungen"
            );
            return;
        }
        if !self.should_capture_chat_subscription(kind, &broadcaster_id) {
            tracing::debug!(
                broadcaster_id,
                ?kind,
                "chat.notification-Sub-Fallback übersprungen: dedizierte EventSub aktiv"
            );
            return;
        }
        if let Err(error) = self
            .telemetry
            .store_subscription_event(&broadcaster_id, &normalized, event_type, chrono::Utc::now())
            .await
        {
            tracing::warn!(
                %error,
                broadcaster_id,
                "chat.notification-Sub-Fallback: store_subscription_event fehlgeschlagen"
            );
        }
    }
}

impl ChatHooks {
    /// `true` = chat-abgeleitetes Sub-Notice persistieren (Python
    /// `should_capture_chat_subscription_notice`, raid/bot.py:396-420). Gibt es
    /// für den Broadcaster bereits die dedizierte EventSub (channel.subscribe /
    /// .message / .gift), liefert der native Pfad die Telemetrie → `false`
    /// (Doppel-Count vermeiden). Ohne bekannten SubscriptionManager (Zelle leer)
    /// fällt der Fallback wie Python (kein `has_sub`-Checker) auf `true` zurück.
    fn should_capture_chat_subscription(
        &self,
        kind: ChatNotificationKind,
        broadcaster_id: &str,
    ) -> bool {
        let eventsub_type = chat_notification_eventsub_type(kind);
        let Some(manager) = self.subscriptions.get() else {
            return true;
        };
        !manager
            .tracked_pairs()
            .iter()
            .any(|(sub_type, bid)| sub_type == eventsub_type && bid == broadcaster_id)
    }
}

/// `notice_type`-Klasse → dedizierter EventSub-Typ (Python
/// `_subscription_notice_eventsub_type`, raid/bot.py:386-394). Bestimmt, welche
/// native Subscription den Fallback abschalten würde.
fn chat_notification_eventsub_type(kind: ChatNotificationKind) -> &'static str {
    match kind {
        ChatNotificationKind::Sub => "channel.subscribe",
        ChatNotificationKind::Resub => "channel.subscription.message",
        ChatNotificationKind::SubGift | ChatNotificationKind::CommunitySubGift => {
            "channel.subscription.gift"
        }
        // Raid/Unraid laufen nie über diesen Pfad (anderer Hook); defensiver
        // Fallback ohne Match-Effekt.
        ChatNotificationKind::Raid | ChatNotificationKind::Unraid => "",
    }
}

/// Baut aus einem rohen `channel.chat.notification`-Event dasselbe normalisierte
/// Sub-Event, das der native `store_subscription_event`-Pfad erwartet (Python
/// `_build_subscription_event_from_chat_notification`, chat/bot.py:2000-2142).
/// Liefert `(event_type, event_value)` oder `None`, wenn das Notice keine
/// Sub-Klasse ist bzw. der erwartete Nested-Payload fehlt.
fn chat_notification_to_subscription_event(
    kind: ChatNotificationKind,
    event: &Value,
) -> Option<(&'static str, Value)> {
    let chatter_login = str_lower(event, &["chatter_user_login", "chatter_user_name"]);
    let chatter_id = str_field(event, &["chatter_user_id"]);
    let message_text = event
        .get("message")
        .and_then(|m| m.get("text"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    match kind {
        ChatNotificationKind::Sub => {
            let sub = event.get("sub")?;
            let tier = tier_of(sub);
            Some((
                "subscribe",
                serde_json::json!({
                    "user_login": chatter_login,
                    "user_id": chatter_id,
                    "tier": tier,
                    "is_gift": false,
                }),
            ))
        }
        ChatNotificationKind::Resub => {
            let resub = event.get("resub")?;
            let gifter_login = str_lower(resub, &["gifter_user_login", "gifter_user_name"]);
            let gifter_id = str_field(resub, &["gifter_user_id"]);
            let is_gift = resub
                .get("gift")
                .or_else(|| resub.get("is_gift"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let mut obj = serde_json::json!({
                "user_login": chatter_login,
                "user_id": chatter_id,
                "tier": tier_of(resub),
                "is_gift": is_gift,
                "gifter_login": gifter_login,
                "gifter_user_id": gifter_id,
                "cumulative_months": pos_int(resub, &["cumulative_months"]),
                "streak_months": pos_int(resub, &["streak_months"]),
            });
            if let Some(text) = message_text {
                obj["message"] = serde_json::json!({ "text": text });
            }
            Some(("resub", obj))
        }
        ChatNotificationKind::SubGift => {
            let gift = event.get("sub_gift")?;
            let recipient_login = str_lower(gift, &["recipient_user_login", "recipient_user_name"]);
            let recipient_id = str_field(gift, &["recipient_user_id"]);
            let gift_total = pos_int(gift, &["cumulative_total", "total"]);
            Some((
                "gift",
                serde_json::json!({
                    "user_login": recipient_login,
                    "user_id": recipient_id,
                    "recipient_login": recipient_login,
                    "recipient_user_id": recipient_id,
                    "tier": tier_of(gift),
                    "is_gift": true,
                    "gifter_login": chatter_login,
                    "gifter_user_id": chatter_id,
                    "total": 1,
                    "gift_total": gift_total,
                    "gift_total_kind": "cumulative_total",
                }),
            ))
        }
        ChatNotificationKind::CommunitySubGift => {
            let gift = event.get("community_sub_gift")?;
            let gift_total = pos_int(gift, &["total", "gift_total"]);
            Some((
                "gift",
                serde_json::json!({
                    "tier": tier_of(gift),
                    "is_gift": true,
                    "gifter_login": chatter_login,
                    "gifter_user_id": chatter_id,
                    "total": gift_total,
                    "gift_total": gift_total,
                    "gift_total_kind": "batch_total",
                }),
            ))
        }
        ChatNotificationKind::Raid | ChatNotificationKind::Unraid => None,
    }
}

/// Sub-Tier eines Nested-Payloads (`sub_tier`/`tier`, Default `"1000"` wie
/// Python). Twitch liefert chat.notification-Tiers als `sub_tier`.
fn tier_of(payload: &Value) -> String {
    str_field(payload, &["sub_tier", "tier"]).unwrap_or_else(|| "1000".to_string())
}

/// Erstes nicht-leeres String-Feld (getrimmt) aus den Kandidaten.
fn str_field(value: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(s) = value.get(*key).and_then(Value::as_str) {
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

/// Wie [`str_field`], aber lowercased (Login-Felder).
fn str_lower(value: &Value, keys: &[&str]) -> Option<String> {
    str_field(value, keys).map(|s| s.to_lowercase())
}

/// Erstes positives Integer-Feld (Zahl oder numerischer String); `0`/None → None
/// (Python: `int(...) or None`).
fn pos_int(value: &Value, keys: &[&str]) -> Option<i64> {
    for key in keys {
        let raw = value.get(*key);
        let parsed = raw.and_then(Value::as_i64).or_else(|| {
            raw.and_then(Value::as_str)
                .and_then(|s| s.trim().parse::<i64>().ok())
        });
        if let Some(n) = parsed {
            if n != 0 {
                return Some(n);
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Port-Adapter
// ---------------------------------------------------------------------------

/// Account-Alter via Helix `users.created_at` (ChatApi mit Bot-Token).
struct HelixAccountAge {
    api: Arc<dyn ChatApi>,
}

#[async_trait::async_trait]
impl AccountAgePort for HelixAccountAge {
    async fn user_created_at_days(&self, user_id: &str, _login: &str) -> Option<i64> {
        match self.api.user_created_at(user_id).await {
            Ok(Some(created_at)) => Some((chrono::Utc::now() - created_at).num_days()),
            _ => None,
        }
    }
}

struct MinimaxPresetPicker {
    client: EngagementMinimaxClient,
}

impl MinimaxPresetPicker {
    fn new(client: EngagementMinimaxClient) -> Self {
        Self { client }
    }
}

#[async_trait::async_trait]
impl PresetPicker for MinimaxPresetPicker {
    async fn pick_preset<'a>(
        &self,
        presets: &'a [PromoPreset],
        snippets: &[String],
        target_login: &str,
    ) -> &'a PromoPreset {
        if presets.len() <= 1 || snippets.is_empty() {
            return RandomPresetPicker
                .pick_preset(presets, snippets, target_login)
                .await;
        }

        let user_prompt = minimax_preset_user_prompt(presets, snippets, target_login);
        let history = [ChatMessage {
            role: "user".to_string(),
            content: user_prompt,
            name: None,
        }];
        match tokio::time::timeout(
            Duration::from_secs(5),
            self.client
                .generate(MINIMAX_PRESET_SYSTEM_PROMPT, &history, 32, 128),
        )
        .await
        {
            Ok(Ok(response)) => {
                if let Some(text) = response.text.as_deref() {
                    if let Some(preset) = match_preset_id(presets, text) {
                        return preset;
                    }
                }
            }
            Ok(Err(error)) => {
                tracing::debug!(%error, "MiniMax preset picker failed");
            }
            Err(_) => {
                tracing::debug!("MiniMax preset picker timeout");
            }
        }

        RandomPresetPicker
            .pick_preset(presets, snippets, target_login)
            .await
    }
}

fn minimax_preset_user_prompt(
    presets: &[PromoPreset],
    snippets: &[String],
    target_login: &str,
) -> String {
    let mut prompt = format!("target_login: {target_login}\n\npresets:\n");
    for preset in presets {
        prompt.push_str(preset.id);
        prompt.push_str(": ");
        prompt.push_str(preset.text);
        prompt.push('\n');
    }
    prompt.push_str("\nsnippets:\n");
    for snippet in snippets {
        prompt.push_str("- ");
        prompt.push_str(snippet);
        prompt.push('\n');
    }
    prompt.push_str("\nReturn exactly one preset id.");
    prompt
}

fn match_preset_id<'a>(presets: &'a [PromoPreset], reply: &str) -> Option<&'a PromoPreset> {
    let reply = reply.trim();
    presets
        .iter()
        .find(|preset| reply == preset.id)
        .or_else(|| presets.iter().find(|preset| reply.contains(preset.id)))
}

/// Raid-Commands: manueller Raid direkt über die tb-raid-Schicht
/// (kein HTTP-Loop), Status/Toggles per SQL (commands.py Z. 94–128/423/479).
struct RaidCommandAdapter {
    manual: Option<Arc<dyn tb_internal_api::ManualRaidPort>>,
    pool: PgPool,
}

#[async_trait::async_trait]
impl RaidCommandPort for RaidCommandAdapter {
    async fn manual_raid(
        &self,
        broadcaster_id: &str,
        broadcaster_login: &str,
    ) -> Result<RaidStartResult, String> {
        let Some(port) = &self.manual else {
            return Ok(RaidStartResult {
                status: "unavailable".to_string(),
                target_login: None,
            });
        };
        let value = port
            .start_manual_raid(broadcaster_id, broadcaster_login)
            .await;
        Ok(RaidStartResult {
            status: value
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("unavailable")
                .to_string(),
            target_login: value
                .get("target_login")
                .and_then(Value::as_str)
                .map(|s| s.to_string()),
        })
    }

    async fn raid_status(&self, broadcaster_id: &str) -> Result<RaidStatusInfo, String> {
        // raid_enabled = boolean, authorized_at = timestamptz (Prod-Schema).
        let auth: Option<(Option<bool>, Option<chrono::DateTime<chrono::Utc>>)> = sqlx::query_as(
            "SELECT raid_enabled, authorized_at FROM twitch_raid_auth \
                 WHERE twitch_user_id = $1",
        )
        .bind(broadcaster_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| e.to_string())?;

        let (total, successful): (i64, i64) = sqlx::query_as(
            "SELECT COUNT(*), COUNT(*) FILTER (WHERE success IS TRUE) \
             FROM twitch_raid_history WHERE from_broadcaster_id = $1",
        )
        .bind(broadcaster_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| e.to_string())?;

        let last: Option<(
            Option<String>,
            Option<i32>,
            Option<chrono::DateTime<chrono::Utc>>,
        )> = sqlx::query_as(
            "SELECT to_broadcaster_login, viewer_count, executed_at \
                 FROM twitch_raid_history WHERE from_broadcaster_id = $1 \
                 ORDER BY executed_at DESC LIMIT 1",
        )
        .bind(broadcaster_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| e.to_string())?;

        let (raid_enabled, authorized_at) = auth.unwrap_or((None, None));
        let (last_login, last_viewers, last_at) = last.unwrap_or((None, None, None));
        Ok(RaidStatusInfo {
            raid_enabled,
            authorized_at,
            total_raids: total,
            successful_raids: successful,
            last_raid_login: last_login,
            last_raid_viewers: last_viewers.map(i64::from),
            last_raid_at: last_at,
        })
    }

    async fn toggle_silent_ban(&self, twitch_login: &str) -> Result<i32, String> {
        toggle_partner_flag(&self.pool, twitch_login, "silent_ban").await
    }

    async fn toggle_silent_raid(&self, twitch_login: &str) -> Result<i32, String> {
        toggle_partner_flag(&self.pool, twitch_login, "silent_raid").await
    }
}

/// Toggle eines INTEGER-Flags auf dem aktiven Partner (`status = 'active'`,
/// jüngste Zeile — wie `load_active_partner` + `set_partner_silent_flags`,
/// partner_registry.py Z. 1808). Gibt den neuen Wert zurück.
async fn toggle_partner_flag(pool: &PgPool, twitch_login: &str, flag: &str) -> Result<i32, String> {
    // flag ist eine interne Konstante ("silent_ban"/"silent_raid") — kein Injection-Risiko.
    let sql = format!(
        "UPDATE twitch_partners SET {flag} = CASE WHEN COALESCE({flag}, 0) = 0 THEN 1 ELSE 0 END \
         WHERE id = ( \
             SELECT id FROM twitch_partners \
             WHERE LOWER(twitch_login) = $1 AND status = 'active' \
             ORDER BY id DESC LIMIT 1 \
         ) \
         RETURNING {flag}"
    );
    let new_value: Option<i32> = sqlx::query_scalar(&sql)
        .bind(twitch_login.to_lowercase())
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?;
    new_value.ok_or_else(|| "kein aktiver Partner".to_string())
}

/// !dldc/!dlde — Discord-Invite des Streamers (discord_invite.rs-SQL).
struct DbDiscordLink {
    pool: PgPool,
}

#[async_trait::async_trait]
impl DiscordLinkPort for DbDiscordLink {
    async fn discord_invite(&self, channel_login: &str) -> Result<Option<String>, String> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT invite_url FROM twitch_streamer_invites WHERE LOWER(streamer_login) = $1",
        )
        .bind(channel_login)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(row.map(|(url,)| url).filter(|u| !u.trim().is_empty()))
    }
}

/// Invite-Question — URL wie `!invite`: Streamer-Invite, sonst Env-Fallback.
struct DbInviteUrlWithFallback {
    pool: PgPool,
}

#[async_trait::async_trait]
impl InviteQuestionInviteUrlPort for DbInviteUrlWithFallback {
    async fn invite_url(&self, channel_login: &str) -> Result<Option<String>, String> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT invite_url FROM twitch_streamer_invites \
             WHERE LOWER(streamer_login) = $1 LIMIT 1",
        )
        .bind(channel_login)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(row
            .map(|(url,)| url)
            .filter(|url| !url.trim().is_empty())
            .or_else(|| {
                std::env::var(PROMO_DISCORD_INVITE_ENV)
                    .ok()
                    .filter(|url| !url.trim().is_empty())
            }))
    }
}

/// !invite — Antwortzeile, Port der chat_command.rs-Logik (Deadlock-live-Gate
/// + streamer-spezifischer Invite mit Env-Fallback).
struct DbInvitePort {
    pool: PgPool,
}

#[async_trait::async_trait]
impl InvitePort for DbInvitePort {
    async fn invite_line(
        &self,
        channel_login: &str,
        chatter_login: &str,
    ) -> Result<Option<String>, String> {
        // Kanal muss live Deadlock streamen (chat_command.rs Z. 56–80).
        let live: Option<(i32, Option<String>)> = sqlx::query_as(
            "SELECT is_live, last_game FROM twitch_live_state \
             WHERE LOWER(streamer_login) = $1 LIMIT 1",
        )
        .bind(channel_login)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        let is_deadlock_live = live
            .map(|(is_live, game)| {
                is_live == 1
                    && game
                        .as_deref()
                        .map(|g| g.to_lowercase().contains("deadlock"))
                        .unwrap_or(false)
            })
            .unwrap_or(false);
        if !is_deadlock_live {
            return Ok(None);
        }

        let row: Option<(String,)> = sqlx::query_as(
            "SELECT invite_url FROM twitch_streamer_invites \
             WHERE LOWER(streamer_login) = $1 LIMIT 1",
        )
        .bind(channel_login)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        let invite_url = row
            .map(|(url,)| url)
            .filter(|u| !u.trim().is_empty())
            .or_else(|| {
                std::env::var(PROMO_DISCORD_INVITE_ENV)
                    .ok()
                    .filter(|v| !v.trim().is_empty())
            });
        let Some(invite_url) = invite_url else {
            return Ok(None);
        };

        Ok(Some(format!(
            "@{chatter_login} Wenn du einen Zugang benötigst, schau gerne auf unserem Discord \
             vorbei, dort bekommst du eine Einladung und Hilfe beim Einstieg :) {invite_url}"
        )))
    }
}

/// Super-Mod-Prüfung via `twitch_admin_roles` (Port von
/// `engagement.admin.is_super_mod`): ein User mit `role = 'super_mod'` darf
/// Engagement in jedem Kanal toggeln — auch ohne Twitch-Mod-Status. Leere
/// Actor-ID oder DB-/Verbindungsfehler → `false` (graceful, mirror von Pythons
/// `if not twitch_user_id`).
struct DbSuperMod {
    pool: PgPool,
}

#[async_trait::async_trait]
impl SuperModPort for DbSuperMod {
    async fn is_super_mod(&self, actor_id: &str) -> bool {
        if actor_id.is_empty() {
            return false;
        }
        sqlx::query_scalar::<_, i32>(
            "SELECT 1 FROM twitch_admin_roles \
             WHERE twitch_user_id = $1 AND role = 'super_mod' LIMIT 1",
        )
        .bind(actor_id)
        .fetch_optional(&self.pool)
        .await
        .unwrap_or(None)
        .is_some()
    }
}

/// Promo-Partner-Gate: aktiver Partner (partner_utils.py Z. 153–181).
struct DbPartnerCheck {
    pool: PgPool,
}

#[async_trait::async_trait]
impl PartnerChannelCheck for DbPartnerCheck {
    async fn is_partner_channel_for_chat_tracking(&self, channel_login: &str) -> bool {
        sqlx::query_scalar::<_, i32>(
            "SELECT COALESCE(is_partner_active, 0) \
             FROM twitch_streamers_partner_state WHERE LOWER(twitch_login) = $1",
        )
        .bind(channel_login)
        .fetch_optional(&self.pool)
        .await
        .unwrap_or(None)
        .unwrap_or(0)
            != 0
    }
}

/// Promo-Invite: streamer-spezifisch → Broker-Erstellung → globaler Fallback.
struct DbInviteResolver {
    pool: PgPool,
    relay: Option<BrokerRelay>,
    invite_channel_id: Option<u64>,
}

#[async_trait::async_trait]
impl InviteResolver for DbInviteResolver {
    async fn resolve_invite(&self, channel_login: &str) -> (String, bool) {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT invite_url FROM twitch_streamer_invites \
             WHERE LOWER(streamer_login) = $1 LIMIT 1",
        )
        .bind(channel_login)
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();
        if let Some((url,)) = row {
            if !url.trim().is_empty() {
                return (url, true);
            }
        }
        if let Some(url) = self.create_and_store_streamer_invite(channel_login).await {
            return (url, true);
        }
        (
            promo_invite_fallback(std::env::var(PROMO_DISCORD_INVITE_ENV).ok().as_deref()),
            false,
        )
    }
}

impl DbInviteResolver {
    async fn create_and_store_streamer_invite(&self, channel_login: &str) -> Option<String> {
        let login = channel_login.trim().to_lowercase();
        if login.is_empty() {
            return None;
        }
        let relay = self.relay.as_ref()?;
        let channel_id = self.invite_channel_id?;
        let reason = format!("streamer-invite:{login}");
        let invite = match relay.create_invite(channel_id, &reason).await {
            Ok(invite) => invite,
            Err(error) => {
                tracing::warn!(%error, login, "Promo-Invite-Erstellung via Broker fehlgeschlagen");
                return None;
            }
        };
        let invite_url = invite.invite_url.trim().to_string();
        let invite_code = invite.code.trim().to_string();
        if invite_url.is_empty()
            || invite_code.is_empty()
            || invite.guild_id == 0
            || invite.channel_id == 0
        {
            tracing::warn!(
                login,
                "Promo-Invite-Erstellung lieferte unvollständige Broker-Antwort"
            );
            return None;
        }
        let Ok(guild_id) = i64::try_from(invite.guild_id) else {
            tracing::warn!(login, "Promo-Invite-Guild-ID passt nicht in i64");
            return None;
        };
        let Ok(channel_id) = i64::try_from(invite.channel_id) else {
            tracing::warn!(login, "Promo-Invite-Channel-ID passt nicht in i64");
            return None;
        };
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let stored = sqlx::query(
            "INSERT INTO twitch_streamer_invites \
             (streamer_login, guild_id, channel_id, invite_code, invite_url, created_at, last_sent_at) \
             VALUES ($1, $2, $3, $4, $5, $6, NULL) \
             ON CONFLICT(streamer_login) DO UPDATE SET \
                guild_id = excluded.guild_id, \
                channel_id = excluded.channel_id, \
                invite_code = excluded.invite_code, \
                invite_url = excluded.invite_url, \
                created_at = excluded.created_at",
        )
        .bind(&login)
        .bind(guild_id)
        .bind(channel_id)
        .bind(&invite_code)
        .bind(&invite_url)
        .bind(&now)
        .execute(&self.pool)
        .await;
        if let Err(error) = stored {
            tracing::warn!(%error, login, "Promo-Invite konnte nicht gespeichert werden");
            return Some(invite_url);
        }
        if let Err(error) = sqlx::query(
            "INSERT INTO discord_invite_codes (guild_id, invite_code, created_at, last_seen_at) \
             VALUES ($1, $2, $3, $3) \
             ON CONFLICT(guild_id, invite_code) DO UPDATE SET last_seen_at = excluded.last_seen_at",
        )
        .bind(guild_id)
        .bind(&invite_code)
        .bind(&now)
        .execute(&self.pool)
        .await
        {
            tracing::debug!(%error, login, "discord_invite_codes konnte nicht aktualisiert werden");
        }
        tracing::info!(login, "Promo-Invite für Streamer erstellt und gespeichert");
        Some(invite_url)
    }

    /// Eager Partner-Invite-Backfill (P2.14, Python `_ensure_partner_invites`,
    /// bot.py:1172-1260). Selektiert alle aktiven Partner OHNE
    /// `twitch_streamer_invites`-Zeile und erstellt für jeden proaktiv einen
    /// Invite (0,5 s Pacing). Fehlschläge werden nach 60 s einmal wiederholt.
    /// So existiert die Invite-Zeile bereits VOR der ersten Promo — cross-system
    /// Konsumenten (Discord-Bot-Sync, Invite-GET-Handler) sehen keine Lücke.
    ///
    /// Kein Broker (`relay`/`invite_channel_id` fehlt) → No-op (wie Python ohne
    /// Discord-Bot: früher Return).
    async fn ensure_partner_invites(&self) {
        if self.relay.is_none() || self.invite_channel_id.is_none() {
            tracing::debug!(
                "Partner-Invite-Backfill übersprungen: kein Broker/Notify-Channel konfiguriert"
            );
            return;
        }
        let logins = match self.partners_without_invite().await {
            Ok(rows) => rows,
            Err(error) => {
                tracing::warn!(%error, "Partner-Invite-Backfill: DB-Abfrage fehlgeschlagen");
                return;
            }
        };
        if logins.is_empty() {
            tracing::debug!("Partner-Invite-Backfill: alle aktiven Partner haben einen Invite");
            return;
        }
        tracing::info!(
            count = logins.len(),
            "Partner-Invite-Backfill: erstelle fehlende Invites"
        );
        let failed = self.create_invites_paced(&logins).await;
        if failed.is_empty() {
            return;
        }
        tracing::info!(
            count = failed.len(),
            "Partner-Invite-Backfill: Fehlschläge, Retry in 60s"
        );
        tokio::time::sleep(Duration::from_secs(60)).await;
        let still_failed = self.create_invites_paced(&failed).await;
        if !still_failed.is_empty() {
            tracing::warn!(
                count = still_failed.len(),
                "Partner-Invite-Backfill: auch nach Retry ohne Invite (nächste Promo holt nach)"
            );
        }
    }

    /// Aktive Partner ohne Invite-Zeile (Python-SELECT bot.py:1184-1197).
    async fn partners_without_invite(&self) -> Result<Vec<String>, sqlx::Error> {
        sqlx::query_scalar::<_, String>(
            "SELECT LOWER(p.twitch_login) \
             FROM twitch_partners p \
             WHERE p.status = 'active' \
               AND p.admin_archived_at IS NULL \
               AND p.departnered_at IS NULL \
               AND NOT EXISTS ( \
                   SELECT 1 FROM twitch_streamer_invites i \
                    WHERE LOWER(i.streamer_login) = LOWER(p.twitch_login) \
               )",
        )
        .fetch_all(&self.pool)
        .await
    }

    /// Erstellt für jede Login einen Invite mit 0,5 s Pacing (Python-Parität:
    /// schont das Broker-/Discord-Rate-Limit). Liefert die Logins, für die kein
    /// Invite erstellt werden konnte.
    async fn create_invites_paced(&self, logins: &[String]) -> Vec<String> {
        let mut failed = Vec::new();
        for login in logins {
            if self.create_and_store_streamer_invite(login).await.is_none() {
                failed.push(login.clone());
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        failed
    }
}

/// Partner-Roster für den Global-Ban-Sweeper (global_ban_sweep.py Z. 145–197).
struct DbPartnerRoster {
    pool: PgPool,
}

#[async_trait::async_trait]
impl PartnerRoster for DbPartnerRoster {
    async fn all_active_partners(&self) -> Vec<(String, String)> {
        sqlx::query_as::<_, (String, String)>(
            "SELECT LOWER(twitch_login), twitch_user_id \
             FROM twitch_streamers_partner_state \
             WHERE is_partner_active = 1 AND COALESCE(twitch_user_id, '') <> ''",
        )
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default()
    }

    async fn valid_auth_ids(&self) -> HashSet<String> {
        sqlx::query_scalar::<_, String>(
            "SELECT twitch_user_id FROM twitch_raid_auth WHERE needs_reauth = FALSE",
        )
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default()
        .into_iter()
        .collect()
    }

    async fn live_broadcaster_ids(&self) -> HashSet<String> {
        sqlx::query_scalar::<_, String>(
            "SELECT twitch_user_id FROM twitch_live_state \
             WHERE is_live = 1 AND COALESCE(twitch_user_id, '') <> ''",
        )
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default()
        .into_iter()
        .collect()
    }

    async fn is_operational_partner_channel(&self, channel: &str) -> bool {
        sqlx::query_scalar::<_, i32>(
            "SELECT COALESCE(is_partner_active, 0) \
             FROM twitch_streamers_partner_state \
             WHERE LOWER(twitch_login) = LOWER($1) OR twitch_user_id = $1",
        )
        .bind(channel)
        .fetch_optional(&self.pool)
        .await
        .unwrap_or(None)
        .unwrap_or(0)
            != 0
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod chat_notification_tests {
    use super::*;
    use serde_json::json;
    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use std::str::FromStr;
    use std::sync::Mutex;
    use tb_chat::types::ChatMessageBody;
    use tb_chat::{AutobanEntry, MentionResolver};

    struct FakeChatApi {
        send_message_outcome: Mutex<Result<SendOutcome, String>>,
        sent_messages: Mutex<Vec<(String, String)>>,
    }

    impl FakeChatApi {
        fn new(send_message_outcome: Result<SendOutcome, String>) -> Self {
            Self {
                send_message_outcome: Mutex::new(send_message_outcome),
                sent_messages: Mutex::new(Vec::new()),
            }
        }

        fn sent_messages(&self) -> Vec<(String, String)> {
            self.sent_messages
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()
        }
    }

    #[async_trait::async_trait]
    impl ChatApi for FakeChatApi {
        async fn send_message(
            &self,
            broadcaster_id: &str,
            message: &str,
        ) -> Result<SendOutcome, String> {
            self.sent_messages
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push((broadcaster_id.to_string(), message.to_string()));
            self.send_message_outcome
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()
        }

        async fn send_announcement(
            &self,
            _broadcaster_id: &str,
            _message: &str,
            _color: &str,
        ) -> Result<bool, String> {
            Ok(false)
        }

        async fn ban_user(
            &self,
            _broadcaster_id: &str,
            _target_user_id: &str,
            _reason: &str,
        ) -> Result<tb_chat::BanOutcome, String> {
            Ok(tb_chat::BanOutcome::Banned)
        }

        async fn timeout_user(
            &self,
            _broadcaster_id: &str,
            _target_user_id: &str,
            _duration_secs: u32,
            _reason: &str,
        ) -> Result<tb_chat::BanOutcome, String> {
            Ok(tb_chat::BanOutcome::Banned)
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
            _broadcaster_id: &str,
            _message_id: &str,
        ) -> Result<bool, String> {
            Ok(true)
        }

        async fn user_created_at(
            &self,
            _user_id: &str,
        ) -> Result<Option<chrono::DateTime<chrono::Utc>>, String> {
            Ok(None)
        }

        async fn resolve_user_id(&self, _login: &str) -> Result<Option<String>, String> {
            Ok(None)
        }

        async fn bot_user_id(&self) -> String {
            "bot".to_string()
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
    impl InviteQuestionInviteUrlPort for NoopDiscordLink {
        async fn invite_url(&self, _channel_login: &str) -> Result<Option<String>, String> {
            Ok(None)
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

    fn non_partner_chat_event() -> ChatMessageEvent {
        ChatMessageEvent {
            broadcaster_user_id: "broadcaster-id".to_string(),
            broadcaster_user_login: "nonpartner".to_string(),
            broadcaster_user_name: String::new(),
            chatter_user_id: "chatter-id".to_string(),
            chatter_user_login: "viewer".to_string(),
            chatter_user_name: String::new(),
            message_id: "msg-1".to_string(),
            message: ChatMessageBody {
                text: "hallo".to_string(),
                fragments: vec![],
            },
            badges: vec![],
            color: String::new(),
            source_broadcaster_user_id: None,
            source_broadcaster_user_login: None,
            source_message_id: None,
        }
    }

    fn pipeline_for_non_partner(api: Arc<FakeChatApi>, pool: PgPool) -> ChatPipeline {
        let api_trait: Arc<dyn ChatApi> = api;
        let http = reqwest::Client::new();
        let moderation = Arc::new(ModerationEngine::new(Arc::clone(&api_trait), pool.clone()));
        let promos = Arc::new(PromoEngine::new(
            pool.clone(),
            Arc::clone(&api_trait),
            Arc::new(tb_chat::NoopSuppressionCheck),
        ));
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
                Arc::new(MiniMaxScamJudge::new(EngagementMinimaxClient::new(
                    None, None, None, None,
                ))),
                Arc::clone(&api_trait),
                Arc::clone(&moderation),
            )),
            spam_filter: Arc::new(SpamFilter::new(Default::default())),
            ai_reviewer: Arc::new(SpamAiReviewer::new(pool.clone(), http.clone())),
            moderation,
            sus_invite: Arc::new(SusInviteCheck::new(pool.clone())),
            fun: Arc::new(FunResponses::new(Arc::clone(&api_trait), false)),
            invite_question: Arc::new(InviteQuestionResponder::new(
                Arc::clone(&api_trait),
                Arc::new(NoopDiscordLink),
                Arc::new(PgInviteQuestionStore::new(pool.clone())),
                Arc::new(MiniMaxInviteQuestionJudge::new(
                    EngagementMinimaxClient::new(None, None, None, None),
                )),
                Some(Arc::clone(&promos) as Arc<dyn tb_chat::commands::PromoBlockCheck>),
                Some(Arc::clone(&promos) as Arc<dyn tb_chat::commands::InviteReplyNotifier>),
            )),
            lfg_pitch: Arc::new(LfgPitchResponder::new(
                Arc::clone(&api_trait),
                Arc::new(NoopDiscordLink),
                Arc::new(MiniMaxLfgJudge::new(EngagementMinimaxClient::new(
                    None, None, None, None,
                ))),
                false,
                Some(Arc::clone(&promos) as Arc<dyn tb_chat::commands::PromoBlockCheck>),
                Some(Arc::clone(&promos) as Arc<dyn tb_chat::commands::InviteReplyNotifier>),
            )),
            promos,
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
            crew_centroid: Arc::new(Centroid::default()),
        })
    }

    async fn setup_non_partner_pipeline_pool(schema: &str) -> Option<PgPool> {
        let dsn = std::env::var("TB_TEST_DATABASE_URL").ok()?;
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
            "CREATE TABLE twitch_partners (twitch_user_id TEXT, twitch_login TEXT)",
            "CREATE TABLE twitch_streamers (twitch_login TEXT, twitch_user_id TEXT)",
            "CREATE TABLE twitch_streamers_partner_state (
                twitch_login TEXT,
                is_partner_active INTEGER DEFAULT 0
            )",
            "CREATE TABLE twitch_live_state (
                streamer_login TEXT PRIMARY KEY,
                is_live INTEGER DEFAULT 0,
                last_game TEXT
            )",
            "CREATE TABLE twitch_stream_sessions (
                id BIGINT PRIMARY KEY,
                streamer_login TEXT,
                started_at TIMESTAMPTZ DEFAULT now(),
                ended_at TIMESTAMPTZ,
                game_name TEXT
            )",
            "CREATE TABLE twitch_raw_chat_ingest_health (
                streamer_login TEXT PRIMARY KEY,
                last_raw_chat_message_at TEXT,
                last_raw_chat_insert_ok_at TEXT,
                last_raw_chat_insert_error_at TEXT,
                last_raw_chat_error TEXT,
                raw_chat_lag_seconds INTEGER,
                updated_at TEXT
            )",
        ] {
            sqlx::query(ddl).execute(&pool).await.unwrap();
        }
        Some(pool)
    }

    #[test]
    fn chat_http_error_reason_enthaelt_gekuerztes_body_detail() {
        let body = format!(
            "{}tail",
            "x".repeat(CHAT_HTTP_ERROR_BODY_SNIPPET_MAX_CHARS + 20)
        );
        let reason = chat_http_error_reason(500, &body);

        assert!(reason.starts_with("helix http 500: "));
        assert!(reason.ends_with("..."));
        assert!(reason.contains(&"x".repeat(CHAT_HTTP_ERROR_BODY_SNIPPET_MAX_CHARS)));
        assert!(!reason.contains("tail"));
    }

    #[tokio::test]
    async fn announcement_fallback_sendet_chat_mit_originaltext_und_placeholder_label() {
        let api = FakeChatApi::new(Ok(SendOutcome::Sent));
        let result = send_announcement_fallback(
            &api,
            "broadcaster_1",
            "nani",
            "Original Announcement",
            "announcement not accepted",
        )
        .await;

        assert_eq!(
            result,
            ChatActionResult::Sent {
                label: ANNOUNCEMENT_FALLBACK_SUCCESS_LABEL.to_string()
            }
        );
        assert_eq!(
            api.sent_messages(),
            vec![(
                "broadcaster_1".to_string(),
                "Original Announcement".to_string()
            )]
        );
    }

    #[tokio::test]
    async fn send_message_http_error_uebernimmt_body_snippet() {
        let api = FakeChatApi::new(Ok(SendOutcome::HttpError {
            status: 403,
            body: "{\"message\":\"missing scope\"}".to_string(),
        }));
        let result =
            send_chat_message_action(&api, "broadcaster_1", "nani", "message", "Hi", None).await;

        assert_eq!(
            result,
            ChatActionResult::Failed {
                reason: "helix http 403: {\"message\":\"missing scope\"}".to_string(),
                detail: Some("{\"message\":\"missing scope\"}".to_string())
            }
        );
    }

    #[tokio::test]
    async fn non_partner_event_liefert_false_damit_engagement_nicht_startet() {
        let Some(pool) = setup_non_partner_pipeline_pool("t_chat_wiring_nonpartner").await else {
            eprintln!("SKIP: TB_TEST_DATABASE_URL nicht gesetzt");
            return;
        };
        let api = Arc::new(FakeChatApi::new(Ok(SendOutcome::Sent)));
        let pipeline = pipeline_for_non_partner(Arc::clone(&api), pool);

        assert!(!pipeline.handle(&non_partner_chat_event()).await);
        assert!(
            api.sent_messages().is_empty(),
            "Non-Partner-Pfad darf keine Chat-Aktion auslösen"
        );
    }

    // P2.15: notice_type-Klasse → dedizierter EventSub-Typ (Python
    // _subscription_notice_eventsub_type). Steuert das should_capture-Gate.
    #[test]
    fn eventsub_type_mappt_jede_sub_klasse() {
        assert_eq!(
            chat_notification_eventsub_type(ChatNotificationKind::Sub),
            "channel.subscribe"
        );
        assert_eq!(
            chat_notification_eventsub_type(ChatNotificationKind::Resub),
            "channel.subscription.message"
        );
        assert_eq!(
            chat_notification_eventsub_type(ChatNotificationKind::SubGift),
            "channel.subscription.gift"
        );
        assert_eq!(
            chat_notification_eventsub_type(ChatNotificationKind::CommunitySubGift),
            "channel.subscription.gift"
        );
    }

    #[test]
    fn sub_notice_wird_zu_subscribe_event() {
        let event = json!({
            "broadcaster_user_id": "100",
            "chatter_user_id": "42",
            "chatter_user_login": "Sub_User",
            "notice_type": "sub",
            "sub": { "sub_tier": "2000", "is_prime": false }
        });
        let (event_type, value) =
            chat_notification_to_subscription_event(ChatNotificationKind::Sub, &event).unwrap();
        assert_eq!(event_type, "subscribe");
        assert_eq!(value["user_login"], "sub_user");
        assert_eq!(value["user_id"], "42");
        assert_eq!(value["tier"], "2000");
        assert_eq!(value["is_gift"], false);
    }

    #[test]
    fn sub_ohne_nested_payload_ergibt_none() {
        let event = json!({ "broadcaster_user_id": "100", "notice_type": "sub" });
        assert!(
            chat_notification_to_subscription_event(ChatNotificationKind::Sub, &event).is_none()
        );
    }

    #[test]
    fn resub_notice_traegt_monate_gift_und_message() {
        let event = json!({
            "broadcaster_user_id": "100",
            "chatter_user_id": "42",
            "chatter_user_login": "ReSubber",
            "notice_type": "resub",
            "resub": {
                "cumulative_months": 12,
                "streak_months": 3,
                "sub_tier": "1000",
                "gift": true,
                "gifter_user_login": "Patron",
                "gifter_user_id": "7"
            },
            "message": { "text": "  danke!  " }
        });
        let (event_type, value) =
            chat_notification_to_subscription_event(ChatNotificationKind::Resub, &event).unwrap();
        assert_eq!(event_type, "resub");
        assert_eq!(value["user_login"], "resubber");
        assert_eq!(value["cumulative_months"], 12);
        assert_eq!(value["streak_months"], 3);
        assert_eq!(value["is_gift"], true);
        assert_eq!(value["gifter_login"], "patron");
        assert_eq!(value["message"]["text"], "danke!");
    }

    #[test]
    fn resub_null_monate_werden_zu_none() {
        let event = json!({
            "broadcaster_user_id": "100",
            "chatter_user_id": "42",
            "chatter_user_login": "x",
            "notice_type": "resub",
            "resub": { "cumulative_months": 0, "streak_months": 0, "sub_tier": "1000" }
        });
        let (_t, value) =
            chat_notification_to_subscription_event(ChatNotificationKind::Resub, &event).unwrap();
        assert!(value["cumulative_months"].is_null());
        assert!(value["streak_months"].is_null());
        // Kein message-Block, wenn kein Text.
        assert!(value.get("message").is_none());
    }

    #[test]
    fn sub_gift_setzt_recipient_und_cumulative_total() {
        let event = json!({
            "broadcaster_user_id": "100",
            "chatter_user_id": "7",
            "chatter_user_login": "Gifter",
            "notice_type": "sub_gift",
            "sub_gift": {
                "sub_tier": "1000",
                "cumulative_total": 25,
                "recipient_user_login": "Lucky",
                "recipient_user_id": "555"
            }
        });
        let (event_type, value) =
            chat_notification_to_subscription_event(ChatNotificationKind::SubGift, &event).unwrap();
        assert_eq!(event_type, "gift");
        assert_eq!(value["is_gift"], true);
        assert_eq!(value["gifter_login"], "gifter");
        assert_eq!(value["user_login"], "lucky");
        assert_eq!(value["recipient_user_id"], "555");
        assert_eq!(value["total"], 1);
        assert_eq!(value["gift_total"], 25);
        assert_eq!(value["gift_total_kind"], "cumulative_total");
    }

    #[test]
    fn community_sub_gift_setzt_batch_total() {
        let event = json!({
            "broadcaster_user_id": "100",
            "chatter_user_id": "7",
            "chatter_user_login": "BigGifter",
            "notice_type": "community_sub_gift",
            "community_sub_gift": { "sub_tier": "3000", "total": 10 }
        });
        let (event_type, value) =
            chat_notification_to_subscription_event(ChatNotificationKind::CommunitySubGift, &event)
                .unwrap();
        assert_eq!(event_type, "gift");
        assert_eq!(value["tier"], "3000");
        assert_eq!(value["is_gift"], true);
        assert_eq!(value["gifter_login"], "biggifter");
        assert_eq!(value["total"], 10);
        assert_eq!(value["gift_total"], 10);
        assert_eq!(value["gift_total_kind"], "batch_total");
    }

    #[test]
    fn fehlendes_tier_faellt_auf_1000() {
        let event = json!({
            "broadcaster_user_id": "100",
            "chatter_user_id": "42",
            "chatter_user_login": "x",
            "notice_type": "sub",
            "sub": {}
        });
        let (_t, value) =
            chat_notification_to_subscription_event(ChatNotificationKind::Sub, &event).unwrap();
        assert_eq!(value["tier"], "1000");
    }

    #[test]
    fn pos_int_akzeptiert_zahl_und_string_und_filtert_null() {
        let v = json!({ "a": 5, "b": "9", "c": 0, "d": "0", "e": "x" });
        assert_eq!(pos_int(&v, &["a"]), Some(5));
        assert_eq!(pos_int(&v, &["b"]), Some(9));
        assert_eq!(pos_int(&v, &["c"]), None);
        assert_eq!(pos_int(&v, &["d"]), None);
        assert_eq!(pos_int(&v, &["e"]), None);
        assert_eq!(pos_int(&v, &["c", "a"]), Some(5));
    }
}

#[cfg(all(test, feature = "integration"))]
mod db_tests {
    use super::*;
    use std::str::FromStr;

    async fn setup(schema: &str) -> PgPool {
        let url = std::env::var("TB_TEST_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://postgres:tbtest@127.0.0.1:5434/postgres".to_string());
        let admin = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
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
        let opts = sqlx::postgres::PgConnectOptions::from_str(&url)
            .unwrap()
            .options([("search_path", schema)]);
        sqlx::postgres::PgPoolOptions::new()
            .max_connections(4)
            .connect_with(opts)
            .await
            .unwrap()
    }

    // P2.11: select_chat_subscription_channels zieht raid_enabled-only-Kanäle
    // (is_partner_active != 1) wieder ein (OR statt der AND-Verengung).
    #[tokio::test]
    async fn p2_11_raid_enabled_nicht_partner_wird_eingeschlossen() {
        let pool = setup("t_p2_11_chatchannels").await;
        // Vereinfachte Tabelle statt der View — genau die Spalten, die die Query liest.
        sqlx::query(
            "CREATE TABLE twitch_streamers_partner_state (
                twitch_login TEXT, twitch_user_id TEXT, is_partner_active INTEGER DEFAULT 0)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE twitch_raid_auth (
                twitch_user_id TEXT, twitch_login TEXT, scopes TEXT,
                raid_enabled BOOLEAN DEFAULT TRUE, needs_reauth BOOLEAN DEFAULT FALSE)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE twitch_streamers (
                twitch_login TEXT, twitch_user_id TEXT)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE twitch_partners (
                twitch_login TEXT, twitch_user_id TEXT)",
        )
        .execute(&pool)
        .await
        .unwrap();

        // (A) raid-aktiv, KEIN Partner, channel:bot-Scope → muss erscheinen.
        sqlx::query("INSERT INTO twitch_streamers_partner_state VALUES ('RaidOnly', '10', 0)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO twitch_raid_auth VALUES ('10', 'raidonly', 'channel:bot user:read', TRUE, FALSE)")
            .execute(&pool).await.unwrap();

        // (B) aktiver Partner, raid_enabled=FALSE, channel:bot → muss erscheinen.
        sqlx::query("INSERT INTO twitch_streamers_partner_state VALUES ('PartnerOnly', '20', 1)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO twitch_raid_auth VALUES ('20', 'partneronly', 'channel:bot', FALSE, FALSE)")
            .execute(&pool).await.unwrap();

        // (C) raid-aktiv, aber OHNE channel:bot-Scope → NICHT erscheinen.
        sqlx::query("INSERT INTO twitch_streamers_partner_state VALUES ('NoScope', '30', 0)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO twitch_raid_auth VALUES ('30', 'noscope', 'user:read', TRUE, FALSE)",
        )
        .execute(&pool)
        .await
        .unwrap();

        // (D) raid-aktiv, channel:bot, aber needs_reauth=TRUE → NICHT erscheinen.
        sqlx::query("INSERT INTO twitch_streamers_partner_state VALUES ('Reauth', '40', 0)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO twitch_raid_auth VALUES ('40', 'reauth', 'channel:bot', TRUE, TRUE)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let mut rows = select_chat_subscription_channels(&pool).await.unwrap();
        rows.sort();
        assert_eq!(
            rows,
            vec![
                ("partneronly".to_string(), "20".to_string()),
                ("raidonly".to_string(), "10".to_string()),
            ]
        );
    }

    #[tokio::test]
    async fn ws_d_subscription_broadcaster_roster_ist_cleanup_und_chat_source_of_truth() {
        let pool = setup("t_wsd_subscription_roster").await;
        sqlx::query(
            "CREATE TABLE twitch_streamers_partner_state (
                twitch_login TEXT, twitch_user_id TEXT,
                is_partner_active INTEGER DEFAULT 0, is_partner INTEGER DEFAULT 0)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE twitch_streamers (
                twitch_login TEXT, twitch_user_id TEXT)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE twitch_partners (
                twitch_login TEXT, twitch_user_id TEXT)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE twitch_raid_auth (
                twitch_user_id TEXT, twitch_login TEXT, scopes TEXT,
                raid_enabled BOOLEAN DEFAULT TRUE, needs_reauth BOOLEAN DEFAULT FALSE)",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO twitch_streamers_partner_state VALUES
                ('RaidOnly', '10', 0, 1),
                ('PartnerOnly', '20', 1, 1),
                ('NoScope', '30', 0, 1)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO twitch_raid_auth VALUES
                ('10', 'raidonly', 'channel:bot user:read', TRUE, FALSE),
                ('20', 'partneronly', 'channel:bot', FALSE, FALSE),
                ('30', 'noscope', 'user:read', TRUE, FALSE)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO twitch_streamers VALUES ('Lurker', '77')")
            .execute(&pool)
            .await
            .unwrap();

        let mut rows = select_eventsub_subscription_broadcasters(&pool)
            .await
            .unwrap();
        rows.sort_by(|a, b| a.twitch_user_id.cmp(&b.twitch_user_id));

        let simplified: Vec<(String, String, bool, bool, bool)> = rows
            .into_iter()
            .map(|row| {
                (
                    row.login,
                    row.twitch_user_id,
                    row.is_partner,
                    row.core_subscriptions,
                    row.chat_subscriptions,
                )
            })
            .collect();

        assert_eq!(
            simplified,
            vec![
                ("raidonly".to_string(), "10".to_string(), false, false, true),
                (
                    "partneronly".to_string(),
                    "20".to_string(),
                    true,
                    true,
                    true
                ),
                ("lurker".to_string(), "77".to_string(), false, true, false),
            ]
        );
    }

    // P2.14: partners_without_invite liefert genau die aktiven Partner ohne
    // twitch_streamer_invites-Zeile (Python _ensure_partner_invites-SELECT).
    #[tokio::test]
    async fn p2_14_partner_ohne_invite_werden_selektiert() {
        let pool = setup("t_p2_14_backfill").await;
        sqlx::query(
            "CREATE TABLE twitch_partners (
                twitch_login TEXT, status TEXT DEFAULT 'active',
                admin_archived_at TEXT, departnered_at TEXT)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("CREATE TABLE twitch_streamer_invites (streamer_login TEXT, invite_url TEXT)")
            .execute(&pool)
            .await
            .unwrap();

        // Aktiver Partner OHNE Invite → muss erscheinen.
        sqlx::query(
            "INSERT INTO twitch_partners (twitch_login, status) VALUES ('Fresh', 'active')",
        )
        .execute(&pool)
        .await
        .unwrap();
        // Aktiver Partner MIT Invite → nicht.
        sqlx::query("INSERT INTO twitch_partners (twitch_login, status) VALUES ('Warm', 'active')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO twitch_streamer_invites VALUES ('warm', 'https://x')")
            .execute(&pool)
            .await
            .unwrap();
        // Archivierter Partner ohne Invite → nicht.
        sqlx::query("INSERT INTO twitch_partners (twitch_login, status, admin_archived_at) VALUES ('Arch', 'active', '2026-01-01')")
            .execute(&pool).await.unwrap();
        // Departnered → nicht.
        sqlx::query("INSERT INTO twitch_partners (twitch_login, status, departnered_at) VALUES ('Gone', 'active', '2026-01-01')")
            .execute(&pool).await.unwrap();
        // Inaktiver Status → nicht.
        sqlx::query(
            "INSERT INTO twitch_partners (twitch_login, status) VALUES ('Paused', 'paused')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let resolver = DbInviteResolver {
            pool: pool.clone(),
            relay: None,
            invite_channel_id: None,
        };
        let logins = resolver.partners_without_invite().await.unwrap();
        assert_eq!(logins, vec!["fresh".to_string()]);
    }
}
