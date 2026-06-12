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
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use sqlx::PgPool;
use tb_chat::commands::{
    AutobanEntry, CommandEngine, DiscordLinkPort, InvitePort, LastAutobanStore, RaidCommandPort,
    RaidStatusInfo, SuperModPort,
};
use tb_chat::moderation::{HelixChatClient, ModerationEngine, OutboundSuppressionStore};
use tb_chat::promos::{InviteResolver, PartnerChannelCheck, PromoEngine};
use tb_chat::scam_pitch::{AccountAgePort, ScamPitchDetector, SpamAiReviewer};
use tb_chat::spam_filter::{LearnedPatterns, SpamFilter};
use tb_chat::token::BotTokenManager;
use tb_chat::types::ChatMessageEvent;
use tb_chat::{
    ChannelClassifier, ChatApi, ChatPipeline, ChatPipelineParts, ChatterTracker, FunResponses,
    GlobalBanSweeper, GlobalChatterBanEnforcer, ModAlerter, PartnerRoster, PgHelixMentionResolver,
    ReviewLog, SusInviteCheck,
};
use tb_monitoring::{EventSubHooks, SubscriptionManager};
use tb_transport_twitch::HelixClient;

/// Reconcile-Intervall für Chat-Subscriptions (Python: periodischer
/// Channel-Join alle 30 Minuten, connection.py).
const CHAT_SUB_RECONCILE_INTERVAL: Duration = Duration::from_secs(30 * 60);

/// Fallback-Env für den globalen Discord-Invite (chat_command.rs / promos.py).
const PROMO_DISCORD_INVITE_ENV: &str = "PROMO_DISCORD_INVITE";

// ---------------------------------------------------------------------------
// Öffentlicher Einstieg
// ---------------------------------------------------------------------------

/// Phase 1: Bot-Token + ChatApi — wird VOR der Hooks-Komposition gebaut,
/// damit die OAuth-Followup-Begrüßung den nativen Send nutzen kann.
pub struct ChatApiHandle {
    pub api: Arc<dyn ChatApi>,
    pub bot_user_id: String,
    token_manager: Arc<BotTokenManager>,
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
    pool: PgPool,
    bot_user_id: String,
}

/// Phase 1: bootet den Bot-Token und baut die ChatApi, wenn `TB_CHAT_ENABLED=1`
/// und alle Voraussetzungen (Refresh-Token, Helix-Credentials) vorhanden sind.
/// `None` = Chat bleibt aus (Python bedient weiter).
pub async fn try_build_api(helix: Option<HelixClient>) -> Option<ChatApiHandle> {
    let enabled = std::env::var("TB_CHAT_ENABLED")
        .map(|v| v.trim() == "1")
        .unwrap_or(false);
    if !enabled {
        tracing::info!("TB_CHAT_ENABLED nicht gesetzt — nativer Chat bleibt aus (Python-Chat aktiv)");
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
        Ok(m) => Arc::new(m),
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
    for required in ["user:bot", "user:read:chat", "user:write:chat"] {
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
    })
}

/// Phase 2: baut die komplette Pipeline auf der gebooteten ChatApi.
pub async fn build_runtime(
    handle: ChatApiHandle,
    pool: PgPool,
    manual_raid: Option<Arc<dyn tb_internal_api::ManualRaidPort>>,
    inner_hooks: Arc<dyn EventSubHooks>,
) -> ChatRuntime {
    let ChatApiHandle {
        api,
        bot_user_id,
        token_manager,
    } = handle;

    ensure_autoban_log_table(&pool).await;

    // Lern-Muster einmalig laden (Python lädt sie beim Bot-Start).
    let learned = LearnedPatterns::load(&pool).await;
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .unwrap_or_default();

    let moderation = Arc::new(ModerationEngine::new(Arc::clone(&api), pool.clone()));
    let suppression = Arc::new(OutboundSuppressionStore::new(pool.clone()));
    let promos = Arc::new(
        PromoEngine::new(pool.clone(), Arc::clone(&api), suppression)
            .set_invite_resolver(Arc::new(DbInviteResolver { pool: pool.clone() }))
            .set_partner_check(Arc::new(DbPartnerCheck { pool: pool.clone() })),
    );

    let commands = Arc::new(CommandEngine::new(
        pool.clone(),
        Arc::clone(&api),
        Arc::new(RaidCommandAdapter {
            manual: manual_raid,
            pool: pool.clone(),
        }),
        Arc::new(DbDiscordLink { pool: pool.clone() }),
        Arc::new(DbInvitePort { pool: pool.clone() }),
        Arc::new(NoopSuperMod),
        Arc::new(EngineAutobanStore {
            engine: Arc::clone(&moderation),
        }),
    ));

    let review_log_dir =
        std::env::var("TB_CHAT_REVIEW_LOG_DIR").unwrap_or_else(|_| "logs".to_string());

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
        spam_filter: Arc::new(SpamFilter::new(learned)),
        ai_reviewer: Arc::new(SpamAiReviewer::new(pool.clone(), http.clone())),
        moderation,
        sus_invite: Arc::new(SusInviteCheck::new(pool.clone())),
        // _fun_thanks_reply_enabled ist in Python default false (bot.py Z. 190).
        fun: Arc::new(FunResponses::new(Arc::clone(&api), false)),
        promos: Arc::clone(&promos),
        commands,
        mention_resolver: Arc::new(PgHelixMentionResolver::new(pool.clone(), Arc::clone(&api))),
        review_log: Arc::new(ReviewLog::new(review_log_dir)),
        alerter: Arc::new(ModAlerter::new(http)),
    }));

    let sweeper = Arc::new(GlobalBanSweeper::new(pool.clone(), Arc::clone(&api)));
    let roster = Arc::new(DbPartnerRoster { pool: pool.clone() });

    tracing::info!("Nativer Chat-Bot verdrahtet — Pipeline aktiv (TB_CHAT_ENABLED=1)");
    ChatRuntime {
        hooks: Arc::new(ChatHooks {
            inner: inner_hooks,
            pipeline,
        }),
        token_manager,
        promos,
        sweeper,
        roster,
        pool,
        bot_user_id,
    }
}

impl ChatRuntime {
    /// Startet alle Hintergrund-Loops: Token-Refresh (30 min), Promo-Loop
    /// (60 s), Global-Ban-Sweeper (120 s + 6-Uhr-Vollsweep) und den
    /// Chat-Subscription-Reconcile (Start + alle 30 min — der Python-Join).
    pub fn start_background(&self, subscriptions: Option<Arc<SubscriptionManager>>) {
        self.token_manager.spawn_refresh_loop();
        Arc::clone(&self.promos).spawn_periodic_loop();
        Arc::clone(&self.sweeper).spawn(Arc::clone(&self.roster) as Arc<dyn PartnerRoster>);

        let Some(manager) = subscriptions else {
            tracing::warn!(
                "Kein SubscriptionManager — Chat-Subscriptions werden nicht angelegt \
                 (Webhook-Config/Helix fehlt)"
            );
            return;
        };
        let pool = self.pool.clone();
        let bot_user_id = self.bot_user_id.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(CHAT_SUB_RECONCILE_INTERVAL);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tick.tick().await;
                reconcile_chat_subscriptions(&manager, &pool, &bot_user_id).await;
            }
        });
    }
}

/// „Join" im Webhook-Modell: für jeden Partner- und Monitored-Kanal die
/// `channel.chat.message`/`channel.chat.notification`-Subscriptions sicherstellen.
/// Kanäle ohne `channel:bot`-Autorisierung schlagen einzeln fehl (Log) —
/// identisch zum Python-Scope-Filter-Verhalten beim Join.
async fn reconcile_chat_subscriptions(
    manager: &SubscriptionManager,
    pool: &PgPool,
    bot_user_id: &str,
) {
    let rows: Vec<(String, String)> = match sqlx::query_as(
        "SELECT LOWER(ps.twitch_login), ps.twitch_user_id \
         FROM twitch_streamers_partner_state ps \
         WHERE ps.is_partner_active = 1 AND COALESCE(ps.twitch_user_id, '') <> '' \
         UNION \
         SELECT LOWER(s.twitch_login), s.twitch_user_id \
         FROM twitch_streamers s \
         WHERE COALESCE(s.is_monitored_only, 0) = 1 AND COALESCE(s.twitch_user_id, '') <> ''",
    )
    .fetch_all(pool)
    .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("chat-sub-reconcile: Kanal-Query fehlgeschlagen: {e}");
            return;
        }
    };

    let mut ok = 0usize;
    let mut failed = 0usize;
    for (login, broadcaster_id) in &rows {
        if manager
            .ensure_chat_subscriptions(broadcaster_id, bot_user_id, login)
            .await
        {
            ok += 1;
        } else {
            failed += 1;
        }
    }
    tracing::info!(
        kanäle = rows.len(),
        ok,
        failed,
        "chat-sub-reconcile abgeschlossen"
    );
}

/// `tb_chat_autoban_log` ist eine neue Rust-Tabelle (nicht im Python-Schema) —
/// beim Start anlegen, damit `ModerationEngine::persist_autoban_record` schreiben kann.
async fn ensure_autoban_log_table(pool: &PgPool) {
    if let Err(e) = sqlx::query(
        "CREATE TABLE IF NOT EXISTS tb_chat_autoban_log (
            id BIGSERIAL PRIMARY KEY,
            channel_login TEXT NOT NULL,
            chatter_id TEXT NOT NULL,
            chatter_login TEXT NOT NULL,
            content TEXT,
            banned_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )",
    )
    .execute(pool)
    .await
    {
        tracing::warn!("tb_chat_autoban_log-Migration fehlgeschlagen: {e}");
    }
}

// ---------------------------------------------------------------------------
// EventSubHooks-Wrapper — delegiert alles, fängt channel.chat.message ab
// ---------------------------------------------------------------------------

struct ChatHooks {
    inner: Arc<dyn EventSubHooks>,
    pipeline: Arc<ChatPipeline>,
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
        self.inner.on_stream_went_live(twitch_user_id, login).await;
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
            Ok(chat_event) => self.pipeline.handle(&chat_event).await,
            Err(e) => tracing::warn!("chat.message nicht deserialisierbar: {e}"),
        }
    }
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
    ) -> Result<String, String> {
        let Some(port) = &self.manual else {
            return Ok("unavailable".to_string());
        };
        let value = port
            .start_manual_raid(broadcaster_id, broadcaster_login)
            .await;
        Ok(value
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("unavailable")
            .to_string())
    }

    async fn raid_status(&self, broadcaster_id: &str) -> Result<RaidStatusInfo, String> {
        // raid_enabled = boolean, authorized_at = timestamptz (Prod-Schema).
        let auth: Option<(Option<bool>, Option<chrono::DateTime<chrono::Utc>>)> =
            sqlx::query_as(
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

        let last: Option<(Option<String>, Option<i32>, Option<chrono::DateTime<chrono::Utc>>)> =
            sqlx::query_as(
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
async fn toggle_partner_flag(
    pool: &PgPool,
    twitch_login: &str,
    flag: &str,
) -> Result<i32, String> {
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

/// Super-Mod-Prüfung — Engagement-Phase steht aus, bis dahin niemand.
struct NoopSuperMod;

#[async_trait::async_trait]
impl SuperModPort for NoopSuperMod {
    async fn is_super_mod(&self, _actor_id: &str) -> bool {
        false
    }
}

/// !uban — letzter Auto-Ban aus dem In-Memory-Store der ModerationEngine.
struct EngineAutobanStore {
    engine: Arc<ModerationEngine>,
}

#[async_trait::async_trait]
impl LastAutobanStore for EngineAutobanStore {
    async fn last_autoban(&self, channel_key: &str) -> Option<AutobanEntry> {
        self.engine.last_autoban(channel_key).map(|r| AutobanEntry {
            user_id: r.user_id,
            login: r.login,
        })
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

/// Promo-Invite: streamer-spezifisch → (url, true), sonst Env-Fallback → (env, false).
struct DbInviteResolver {
    pool: PgPool,
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
        (
            std::env::var(PROMO_DISCORD_INVITE_ENV).unwrap_or_default(),
            false,
        )
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

    async fn is_operational_partner_channel(&self, login: &str) -> bool {
        sqlx::query_scalar::<_, i32>(
            "SELECT COALESCE(is_partner_active, 0) \
             FROM twitch_streamers_partner_state WHERE LOWER(twitch_login) = $1",
        )
        .bind(login.to_lowercase())
        .fetch_optional(&self.pool)
        .await
        .unwrap_or(None)
        .unwrap_or(0)
            != 0
    }
}
