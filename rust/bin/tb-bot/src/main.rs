//! tb-bot — Internes API-Binary auf Port 8776.
//!
//! Bindet ausschließlich auf 127.0.0.1 (Loopback). UFW blockt 8776 extern.
//! Auth: X-Internal-Token + loopback_only-Layer (Defense-in-Depth).
//!
//! Env-Variablen:
//!   TWITCH_ANALYTICS_DSN          — PostgreSQL-DSN
//!   TWITCH_INTERNAL_API_TOKEN     — Auth-Token
//!   TWITCH_CLIENT_ID              — Twitch Helix Client-ID (optional)
//!   TWITCH_CLIENT_SECRET          — Twitch Helix Client-Secret (optional)
//!   TWITCH_TARGET_GAME_NAME       — Ziel-Kategorie (default "Deadlock")
//!   TWITCH_WEBHOOK_SECRET         — EventSub-Webhook-Secret (optional)
//!   TWITCH_EVENTSUB_CALLBACK_URL  — öffentliche Callback-URL (optional;
//!                                   beide gesetzt → Subscription-Verwaltung)
//!   TB_MONITORING_POLL_ENABLED    — "1" startet den Poll-Loop (Cutover-Gate,
//!                                   default aus — Python bleibt Live-Writer)
//!   TWITCH_NOTIFY_CHANNEL_ID      — Discord-Kanal der Go-Live-Postings
//!   TWITCH_ALERT_MENTION          — optionale Alert-Mention (z. B. <@&id>)
//!   TWITCH_DISCORD_REF_CODE       — Referral-Code für Twitch-URLs
//!   TWITCH_LANGUAGE_FILTERS       — Komma-Liste (z. B. "de,en"), leer = alle
//!   DB_MASTER_KEY_V1              — AES-Master-Key (Hex); ohne ihn bleiben
//!                                   die Raid-Hooks deaktiviert (kein Token-Read)
//!   TB_INTERNAL_API_LEGACY_FALLBACK_URL — Basis-URL der Legacy-Python-API
//!                                   (z. B. http://127.0.0.1:8779); unbekannte
//!                                   interne-API-Routen werden dorthin
//!                                   geproxyt, leer = 404 wie bisher
//!   PORT                          — optional, default 8776
//!   TB_HIGHLIGHT_CLIPPER_ENABLED  — "1" startet die Highlight-Erstellung
//!                                   (default aus; benötigt Helix-Client)
//!   TB_CLIP_FETCHER_ENABLED       — "1" startet den Clip-Fetch-Task
//!                                   (default aus; benötigt Helix-Client)
//!   TB_SCOUT_ENABLED              — "1" startet den Scout-Task für live Deadlock-DE-Streams
//!                                   (default aus; benötigt Helix-Client)
//!   ENGAGEMENT_SHADOW_REVIEW_CHANNEL_ID — Discord-Kanal-ID für den Shadow-KI-
//!                                   Review-Ausgang (B19). Fehlt sie, bleibt der
//!                                   Forward-Loop aus (default aus, opt-in)

mod auto_raid;
mod chat_wiring;
mod chatters_wiring;
mod confirm_resolver;
mod eventsub_hooks;
mod eventsub_stats_adapter;
mod irc_lurker_wiring;
mod oauth_followups;
mod offline_side_effects;
mod partner_lookup;
mod partner_recruit;
mod raid_adapters;
mod raid_arrival_wiring;
mod raid_greeting;
mod raid_oauth_impl;
mod reauth_reminder;
mod scam_enforce_impl;
mod scam_notify_impl;
mod scam_revoke_impl;
mod score_refresh;
mod scout_chat;
mod shadow_review_wiring;
mod streamer_link;
mod task_supervisor;
mod token_lifecycle_wiring;
mod user_id_backfill;
mod wiring;

fn optional_env_bool(name: &str, default: bool) -> bool {
    match std::env::var(name) {
        Ok(value) => {
            let raw = value.trim().to_lowercase();
            match raw.as_str() {
                "" => default,
                "1" | "true" | "yes" | "on" => true,
                "0" | "false" | "no" | "off" => false,
                _ => {
                    tracing::warn!(
                        setting = name,
                        value = %value,
                        default,
                        "Ungültiger optionaler Bool-Env-Wert; Default wird verwendet"
                    );
                    default
                }
            }
        }
        Err(_) => default,
    }
}

fn opt_in_enabled(name: &str) -> bool {
    optional_env_bool(name, false)
}

fn watch_one_shot_task(task: &'static str, handle: tokio::task::JoinHandle<()>) {
    tokio::spawn(async move {
        if let Err(error) = handle.await {
            tracing::error!(task, %error, "One-Shot-Task fehlerhaft beendet");
        }
    });
}

fn optional_env_u16(name: &str, default: u16) -> u16 {
    match std::env::var(name) {
        Ok(value) if value.trim().is_empty() => default,
        Ok(value) => match value.trim().parse::<u16>() {
            Ok(parsed) if parsed > 0 => parsed,
            _ => {
                tracing::warn!(
                    setting = name,
                    value = %value,
                    default,
                    "Ungültiger optionaler Port-Env-Wert; Default wird verwendet"
                );
                default
            }
        },
        Err(_) => default,
    }
}

fn optional_env_i64(name: &str, default: i64) -> i64 {
    match std::env::var(name) {
        Ok(value) if value.trim().is_empty() => default,
        Ok(value) => match value.trim().parse::<i64>() {
            Ok(parsed) => parsed,
            Err(_) => {
                tracing::warn!(
                    setting = name,
                    value = %value,
                    default,
                    "Ungültiger optionaler Integer-Env-Wert; Default wird verwendet"
                );
                default
            }
        },
        Err(_) => default,
    }
}

fn optional_env_u64_with_fallback(primary: &str, fallback: &str, default: u64) -> u64 {
    for name in [primary, fallback] {
        match std::env::var(name) {
            Ok(value) if value.trim().is_empty() => {}
            Ok(value) => match value.trim().parse::<u64>() {
                Ok(parsed) if parsed > 0 => return parsed,
                _ => {
                    tracing::warn!(
                        setting = name,
                        value = %value,
                        default,
                        "Ungültiger optionaler Integer-Env-Wert; Default wird verwendet"
                    );
                    return default;
                }
            },
            Err(_) => {}
        }
    }
    default
}

fn optional_env_positive_i64(name: &str, default: i64) -> i64 {
    match std::env::var(name) {
        Ok(value) if value.trim().is_empty() => default,
        Ok(value) => match value.trim().parse::<i64>() {
            Ok(parsed) if parsed > 0 => parsed,
            _ => {
                tracing::warn!(
                    setting = name,
                    value = %value,
                    default,
                    "Ungültiger optionaler Integer-Env-Wert; Default wird verwendet"
                );
                default
            }
        },
        Err(_) => default,
    }
}

async fn bind_internal_listener_with_retry(
    addr: SocketAddr,
) -> std::io::Result<tokio::net::TcpListener> {
    const RETRY_DELAYS_MS: &[u64] = &[250, 500, 1_000, 2_000, 4_000];

    let mut attempt = 0usize;
    loop {
        match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => return Ok(listener),
            Err(error)
                if error.kind() == std::io::ErrorKind::AddrInUse
                    && attempt < RETRY_DELAYS_MS.len() =>
            {
                let delay = Duration::from_millis(RETRY_DELAYS_MS[attempt]);
                tracing::warn!(
                    %addr,
                    attempt = attempt + 1,
                    retry_in_ms = delay.as_millis(),
                    "Internal-API Port belegt, versuche erneut"
                );
                tokio::time::sleep(delay).await;
                attempt += 1;
            }
            Err(error) => return Err(error),
        }
    }
}

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tb_config::Settings;
use tb_crypto::FieldCipher;
use tb_internal_api::build_internal_router;
use tb_monitoring::poller::{ChannelInfoSource, PollHooks, StreamSource};
use tb_monitoring::sessions::store::SessionStore;
use tb_monitoring::sessions::tracker::FollowerCountSource;
use tb_monitoring::{
    AnnouncementSettings, AnnouncementSink, BrokerAnnouncementSink, CapacitySnapshotStore,
    EventSubDispatcher, EventSubHooks, ExpSessionStore, ExpSessionTracker, GuardStore,
    InboxRuntime, LiveStateStore, MonitoringEventHandler, NoFollowerSource, NoopAnnouncementSink,
    NoopEventSubHooks, PollConfig, PollEngine, PollIntervalStore, SessionTracker, StatsStore,
    SubscriptionConfig, SubscriptionManager, TelemetryStore, TrackedStore, VodPreviewSource,
};
use tb_raid::{
    AutoRaidPipeline, ManualRaidSuppression, OfflineEligibilityStore, OutreachBoostStore,
    PartnerRosterStore, PendingRaidStore, RaidArrivalRuntime, RaidAuthStore, RaidBlacklistStore,
    RaidExecutor, RaidHistoryStore, RaidTokenRefresher, ScoreStore, StrikesStore,
    TokenBlacklistStore, TokenProvider,
};
use tb_transport_discord::BrokerRelay;
use tb_transport_twitch::{HelixClient, HelixConfig};

use auto_raid::OfflineRaidHandler;
use eventsub_hooks::{
    BlacklistRaidGuard, RaidArrivalCoordinator, RaidEventSubHooks, RaidTrackingResolverAdapter,
};
use offline_side_effects::OfflineSideEffects;
use raid_adapters::{HelixFallbackStreams, HelixRaidApi, HelixTokenClient, ManualRaidAdapter};
use raid_arrival_wiring::RaidArrivalSinkImpl;
use reauth_reminder::ReauthReminder;
use score_refresh::ScoreRefreshResolver;
use wiring::{
    BotFollowerTokenSource, BrokerAnnouncementTransport, FollowerTokenSource,
    FollowerTokenSourceWithStreamerFallback, HelixChannelProfile, HelixFollowerSource,
    HelixStreamSource, HelixSubscriptionTransport, HelixVodPreview, HelixVodSource,
    LivePingRoleAuto, SubscriptionEventSubHooks,
};

struct InternalBulkReauthAdapter {
    store: tb_raid::ReauthAdminStore,
}

#[async_trait::async_trait]
impl tb_internal_api::handlers::reauth_all::BulkReauthPort for InternalBulkReauthAdapter {
    async fn snapshot_and_flag_reauth(&self) -> Result<u64, String> {
        self.store
            .snapshot_and_flag_reauth()
            .await
            .map_err(|error| error.to_string())
    }
}

/// Hooks des Poll-Loops: Go-Live → stream.offline-Subscription (wie EventSub),
/// Auto-Archiv/Entarchiv inaktiver Partner und Score-Refreshes pro Tick.
struct SubscriptionPollHooks {
    manager: Arc<SubscriptionManager>,
    pool: sqlx::PgPool,
    offline_raid: Option<Arc<OfflineRaidHandler>>,
    /// ChatApi für den Partner-Recruiting-Outreach; `None` ohne Bot-Token.
    chat_api: Option<Arc<dyn tb_chat::ChatApi>>,
    /// Letzter Recruiting-Durchlauf (interne 30-min-Drosselung, Python
    /// `_last_recruit_check`).
    recruit_last_check: std::sync::Mutex<Option<std::time::Instant>>,
}

async fn mark_partner_inactivity_flagged(
    pool: &sqlx::PgPool,
    login: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE twitch_partners SET inactivity_flagged_at = NOW()::text \
         WHERE LOWER(twitch_login) = LOWER($1) \
           AND COALESCE(status, 'active') = 'active' \
           AND inactivity_flagged_at IS NULL",
    )
    .bind(login)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

async fn clear_partner_inactivity_flag(
    pool: &sqlx::PgPool,
    login: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE twitch_partners SET inactivity_flagged_at = NULL \
         WHERE LOWER(twitch_login) = LOWER($1) \
           AND inactivity_flagged_at IS NOT NULL",
    )
    .bind(login)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

impl SubscriptionPollHooks {
    /// `true` wenn der Recruiting-Durchlauf fällig ist (≥ 30 min seit dem
    /// letzten) und stempelt zugleich neu. Python `_run_partner_recruit`-Guard.
    fn recruit_due(&self) -> bool {
        let now = std::time::Instant::now();
        let mut guard = self.recruit_last_check.lock().unwrap();
        let due = match *guard {
            Some(last) => now.duration_since(last) >= std::time::Duration::from_secs(1800),
            None => true,
        };
        if due {
            *guard = Some(now);
        }
        due
    }
}

#[async_trait::async_trait]
impl PollHooks for SubscriptionPollHooks {
    async fn on_stream_went_live(&self, twitch_user_id: &str, login: &str) {
        self.manager
            .ensure_offline_subscription(twitch_user_id, login)
            .await;
    }

    async fn on_stream_offline_raid(&self, twitch_user_id: &str, login: Option<&str>) {
        if let Some(handler) = &self.offline_raid {
            handler.handle_streamer_offline(twitch_user_id, login).await;
        }
    }

    /// Inaktiver Partner (> N Tage keine relevante Aktivität) → informativ
    /// markieren. Das ist keine Operator-Deaktivierung.
    async fn on_auto_archive(&self, login: &str) -> bool {
        let changed = match mark_partner_inactivity_flagged(&self.pool, login).await {
            Ok(changed) => changed,
            Err(e) => {
                tracing::warn!(
                    login,
                    "auto-inactivity (twitch_partners) fehlgeschlagen: {e}"
                );
                return false;
            }
        };
        if changed {
            tracing::info!(login, "Partner automatisch als inaktiv markiert");
        }
        changed
    }

    /// Informativ inaktiver Partner streamt wieder Deadlock → Flag loeschen.
    async fn on_auto_unarchive(&self, login: &str) -> bool {
        let changed = match clear_partner_inactivity_flag(&self.pool, login).await {
            Ok(changed) => changed,
            Err(e) => {
                tracing::warn!(
                    login,
                    "auto-inactivity-clear (twitch_partners) fehlgeschlagen: {e}"
                );
                return false;
            }
        };
        if changed {
            tracing::info!(login, "Partner automatisch nicht mehr als inaktiv markiert");
        }
        changed
    }

    /// Tick-Abschluss: Partner-Recruiting-Outreach aus dem category_streams-Sample
    /// dieses Ticks (Python `_run_partner_recruit`) + fällige Partner-Raid-Score-
    /// Refreshes aus Poll-Transitions (zusätzlich zum 300s-Voll-Reconcile).
    async fn after_tick(&self, report: tb_monitoring::TickReport) {
        // Partner-Recruiting: intern auf 30 min gedrosselt; die schwere Arbeit
        // (Kandidaten-Query + Sends mit 60s-Throttle) läuft gespawnt, damit der
        // Tick nicht blockiert. Nur mit gebootetem Bot-Token (chat_api Some).
        if let Some(chat_api) = self.chat_api.clone() {
            if self.recruit_due() {
                let pool = self.pool.clone();
                let category_streams = report.category_streams;
                let handle = tokio::spawn(async move {
                    partner_recruit::run_partner_recruit(&pool, &chat_api, &category_streams).await;
                });
                watch_one_shot_task("partner_recruit_tick", handle);
            }
        }

        // Fällige Partner-Raid-Score-Refreshes anwenden.
        if !report.score_refreshes.is_empty() {
            let pairs: Vec<(String, String)> = report
                .score_refreshes
                .iter()
                .map(|s| (s.twitch_user_id.clone(), s.login.clone()))
                .collect();
            let resolver = ScoreRefreshResolver::new(self.pool.clone());
            if let Err(e) = resolver.refresh_scores(&pairs, chrono::Utc::now()).await {
                tracing::warn!("after_tick: Score-Refresh fehlgeschlagen: {e}");
            }
        }
    }

    /// B5-08: jeden Poll-Tick die EventSub-Capacity-Zeitreihe fortschreiben.
    /// Die Sample-/Retention-Drosselung sitzt im `SubscriptionManager`
    /// (`record_capacity_snapshot_periodic`); der Tick gibt nur den Takt vor.
    async fn on_capacity_tick(&self) {
        self.manager
            .record_capacity_snapshot_periodic("poll_tick")
            .await;
    }
}

/// Sprachfilter fürs Kategorie-Sampling/Scout. Python hartkodiert
/// `TWITCH_LANGUAGE="de de-de de-at de-ch"` (core/constants.py); hier ist ein
/// Env-Override via `TWITCH_LANGUAGE_FILTERS` erlaubt, aber leer/ungesetzt fällt
/// auf den deutschen Default zurück — **nicht** auf „alle Sprachen", sonst landet
/// das Kategorie-Sample sprachgemischt in den Stats (Port-Bug bis 13.6.).
fn language_filters_from_env() -> Vec<String> {
    let parsed: Vec<String> = std::env::var("TWITCH_LANGUAGE_FILTERS")
        .map(|v| {
            v.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();
    if parsed.is_empty() {
        ["de", "de-de", "de-at", "de-ch"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    } else {
        parsed
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let supervisor = task_supervisor::TaskSupervisor::start();

    let settings = Settings::from_env().unwrap_or_else(|e| {
        tracing::error!("Konfigurationsfehler: {e}");
        std::process::exit(1);
    });

    let pool = tb_db::connect(&settings.db).await.unwrap_or_else(|e| {
        tracing::error!("DB-Verbindungsfehler: {e}");
        std::process::exit(1);
    });

    // Native sqlx-Migrationen anwenden. Schema-/Migrationsfehler sind fatal:
    // mit kaputtem oder halb migriertem Schema darf der Bot nicht starten.
    if optional_env_bool("TB_DB_MIGRATE", true) {
        match tb_db::run_migrations(&pool).await {
            Ok(()) => tracing::info!("DB-Migrationen angewendet (oder bereits aktuell)"),
            Err(e) => {
                tracing::error!("DB-Migrationen fehlgeschlagen; Startup wird abgebrochen: {e}");
                std::process::exit(1);
            }
        }
    } else {
        tracing::warn!("DB-Migrationen deaktiviert (TB_DB_MIGRATE=0)");
    }

    let port: u16 = optional_env_u16("PORT", 8776);

    // HelixClient aus Env bauen — optional, Bot startet auch ohne Helix
    let helix: Arc<Option<HelixClient>> = {
        let client_id = std::env::var("TWITCH_CLIENT_ID").ok();
        let client_secret = std::env::var("TWITCH_CLIENT_SECRET").ok();
        match (client_id, client_secret) {
            (Some(id), Some(secret)) => match HelixClient::new(HelixConfig::new(id, secret)) {
                Ok(c) => {
                    tracing::info!("HelixClient initialisiert");
                    Arc::new(Some(c))
                }
                Err(e) => {
                    tracing::warn!("HelixClient-Initialisierung fehlgeschlagen: {e}");
                    Arc::new(None)
                }
            },
            _ => {
                tracing::warn!(
                    "TWITCH_CLIENT_ID/TWITCH_CLIENT_SECRET fehlen — Helix-API deaktiviert"
                );
                Arc::new(None)
            }
        }
    };

    // EventSub-Ingress: Inbox-Worker + Dispatcher. Mit Webhook-Config + Helix
    // verwaltet Rust die Core-Subscriptions selbst (Go-Live → stream.offline);
    // mit Krypto-Key sind zusätzlich alle Raid-Hooks echt (s. unten).
    let target_game =
        std::env::var("TWITCH_TARGET_GAME_NAME").unwrap_or_else(|_| "Deadlock".to_string());
    let guard = GuardStore::new(pool.clone());
    // P2.57: `mut`, weil der inbound Bot-Timeout-Guard erst nach dem
    // ChatRuntime-Aufbau injiziert wird (s. `with_bot_timeout_guard` unten).
    let mut telemetry = TelemetryStore::new(pool.clone());
    let live_state = LiveStateStore::new(pool.clone());
    // Welle B Phase 1: Bot-Token booten + ChatApi bauen (TB_CHAT_ENABLED=1).
    // Früh gezogen (vor Follower-Source + Hooks-Komposition): die Follower-Total-
    // Quelle (P1.7) braucht den Bot-Token mit `moderator:read:followers`, und die
    // OAuth-Followup-Begrüßung den nativen Send statt des Python-Umwegs (8779).
    // Es gibt nur DIESEN einen BotTokenManager (kein zweiter Refresher).
    let chat_api_handle = chat_wiring::try_build_api(helix.as_ref().clone(), pool.clone()).await;
    // P1.7: Bot-Token-Quelle für den Follower-Total-Abruf (moderator:read:followers).
    // P1.19: mit verfügbarem Streamer-Token-Provider (Krypto-Key + Helix) wird die
    // Bot-Token-Quelle mit dem Streamer-OAuth-Token-Fallback umwickelt — bei 403
    // greift dann der Broadcaster-Token statt nur Bot-/App-Token. Der Provider ist
    // eine eigene Instanz mit Arc-Clones (gleicher Advisory-Lock wie der Raid-Pfad,
    // kein Dual-Refresh), da der Raid-TokenProvider erst weiter unten gebaut wird.
    let follower_streamer_token_provider: Option<Arc<TokenProvider>> = helix
        .as_ref()
        .clone()
        .and_then(|hc| build_moderator_token_provider(pool.clone(), hc));
    let follower_token_source: Option<Arc<dyn FollowerTokenSource>> =
        chat_api_handle.as_ref().map(|h| {
            let bot_source: Arc<dyn FollowerTokenSource> = Arc::new(BotFollowerTokenSource {
                token_manager: h.bot_token_manager(),
            });
            match follower_streamer_token_provider.clone() {
                Some(token_provider) => Arc::new(FollowerTokenSourceWithStreamerFallback {
                    inner: bot_source,
                    token_provider,
                }) as Arc<dyn FollowerTokenSource>,
                None => bot_source,
            }
        });
    let followers: Arc<dyn FollowerCountSource> = match helix.as_ref().clone() {
        Some(helix_client) => Arc::new(HelixFollowerSource {
            helix: helix_client,
            token_source: follower_token_source,
            // P3.9: Once-only-WARN beim Legacy-Broadcaster-Token-Fallback.
            scope_fallback_warner: Some(Arc::new(tb_raid::ScopeFallbackWarner::new())),
        }),
        None => Arc::new(NoFollowerSource),
    };
    let tracker = Arc::new(
        SessionTracker::new(
            SessionStore::new(pool.clone()),
            live_state.clone(),
            ExpSessionTracker::new(ExpSessionStore::new(pool.clone())),
            followers.clone(),
            &target_game,
        )
        // B7 raid-scores-tracking-1: offene Partner-Raid-Score-Tracking-Zeilen
        // der Session beim Finalize auflösen (entkoppelt via Port → tb-raid).
        .with_raid_resolver(Arc::new(RaidTrackingResolverAdapter::new(
            pool.clone(),
            &target_game,
        ))),
    );
    tracker.rehydrate().await;
    {
        let backfill_pool = pool.clone();
        let backfill_helix = helix.as_ref().clone();
        let handle = tokio::spawn(async move {
            user_id_backfill::sync_missing_user_ids(&backfill_pool, backfill_helix.as_ref()).await;
        });
        watch_one_shot_task("user_id_backfill", handle);
    }
    // Session-Tracker-Clone für den Scout-Task FRÜH ziehen: der Poll-Loop
    // konsumiert das `tracker`-Arc weiter unten (PollEngine::new), die
    // Scout-Konstruktion liegt danach und hätte sonst keinen Zugriff mehr.
    let scout_tracker = tracker.clone();

    let webhook_secret = std::env::var("TWITCH_WEBHOOK_SECRET")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    let callback_url = std::env::var("TWITCH_EVENTSUB_CALLBACK_URL")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    let subscription_manager: Option<Arc<SubscriptionManager>> =
        match (webhook_secret, callback_url, helix.as_ref().clone()) {
            (Some(secret), Some(callback_url), Some(helix_client)) => {
                let mut manager_builder = SubscriptionManager::new(
                    Arc::new(HelixSubscriptionTransport {
                        helix: helix_client.clone(),
                    }),
                    SubscriptionConfig {
                        callback_url,
                        secret,
                    },
                    CapacitySnapshotStore::new(pool.clone()),
                );
                // P1.2: Mod-Provisioner für die 403-Selbstheilung im Chat-/Sub-Pfad
                // (Python `_ensure_bot_is_mod`). Braucht den Streamer-Token-Resolver
                // (cipher-gated) + die Bot-User-ID aus dem gebooteten Chat-Handle.
                // Fehlt eines, bleibt es beim Cooldown-Pfad ohne Re-Mod.
                let bot_user_id = chat_api_handle
                    .as_ref()
                    .map(|h| h.bot_user_id.clone())
                    .unwrap_or_default();
                if !bot_user_id.trim().is_empty() {
                    if let Some(token_provider) =
                        build_moderator_token_provider(pool.clone(), helix_client.clone())
                    {
                        manager_builder = manager_builder.with_moderator_provisioner(Arc::new(
                            eventsub_hooks::HelixModeratorProvisioner::new(
                                token_provider,
                                helix_client.clone(),
                                bot_user_id,
                            ),
                        ));
                        tracing::info!(
                        "Mod-Provisioner aktiv (403-Selbstheilung: Bot-Remod via Streamer-Token)"
                    );
                    } else {
                        tracing::info!(
                            "DB_MASTER_KEY_V1 fehlt — Mod-Provisioner aus (403-Cooldown aktiv)"
                        );
                    }
                }
                // P2.56: Broadcaster-Token-Fallback für Moderator-Telemetrie-Subs.
                // Wenn dem Bot-Token der Scope fehlt (403/Scope-Lücke), versucht der
                // Manager dieselben Subs mit dem Broadcaster-Token (Condition
                // moderator_user_id = broadcaster_id, wie Python). Quelle ist der
                // bestehende Raid-Auth-Tokenpfad (cipher-gated); ohne Krypto-Key
                // bleibt der Fallback aus.
                if let Some((token_provider, raid_auth)) =
                    build_telemetry_sub_auth(pool.clone(), helix_client.clone())
                {
                    manager_builder = manager_builder.with_broadcaster_eventsub_token_provider(
                        Arc::new(RaidBroadcasterEventSubTokenProvider {
                            token_provider,
                            raid_auth,
                        }),
                    );
                    tracing::info!(
                        "Broadcaster-EventSub-Token-Provider aktiv (Moderator-Telemetrie-Fallback)"
                    );
                }
                let manager = Arc::new(manager_builder);
                manager.rehydrate().await;
                tracing::info!("EventSub-Subscription-Verwaltung aktiv (Webhook-Modus)");
                // Startup-Cleanup + periodischer Core-Sub-Reconcile:
                // Python führte beim Webhook-Start _cleanup_old_eventsub_subscriptions
                // aus und stelle stream.online/offline/channel.update für alle aktiven
                // Partner sicher. Rust macht das hier als Background-Task. Zusätzlich
                // die Broadcaster-Telemetrie-Subs (B9), sofern der Krypto-Key da ist.
                {
                    let m = manager.clone();
                    let p = pool.clone();
                    let telemetry_auth = build_telemetry_sub_auth(pool.clone(), helix_client);
                    supervisor.spawn("eventsub_subscription_maintenance", async move {
                        subscription_maintenance_loop(m, p, telemetry_auth).await;
                    });
                }
                Some(manager)
            }
            _ => {
                tracing::info!(
                    "TWITCH_WEBHOOK_SECRET/TWITCH_EVENTSUB_CALLBACK_URL nicht gesetzt — \
                     Subscription-Verwaltung deaktiviert (Hooks Noop)"
                );
                None
            }
        };
    // ChatApi-Clone für den Partner-Recruiting-Outreach FRÜH ziehen — das Handle
    // wird weiter unten (Pipeline-Aufbau) konsumiert und ist bei der
    // SubscriptionPollHooks-Konstruktion sonst nicht mehr im Scope.
    let recruit_chat_api: Option<Arc<dyn tb_chat::ChatApi>> =
        chat_api_handle.as_ref().map(|h| h.api());
    // Bot-Token-Bridge (F3): ChatApi-Clone für die Owner-Chat-Action der
    // internen API (POST /streamers/:login/chat-action). Früh gezogen, da das
    // Handle weiter unten beim Pipeline-Aufbau konsumiert wird. Der Send läuft
    // über den live rotierten Bot-User-Token (ChatApi → BotTokenManager).
    let chat_action_api: Option<Arc<dyn tb_chat::ChatApi>> =
        chat_api_handle.as_ref().map(|h| h.api());
    // ChatApi-Clone für den Scam-Guard-Revoke-Port der internen API
    // (POST /scam-guard/revoke): der Unban läuft über den live rotierten
    // Bot-User-Token, identisch zum Auto-Ban-Pfad des Wächters.
    let scam_revoke_api: Option<Arc<dyn tb_chat::ChatApi>> =
        chat_api_handle.as_ref().map(|h| h.api());
    let scam_enforce_api: Option<Arc<dyn tb_chat::ChatApi>> =
        chat_api_handle.as_ref().map(|h| h.api());
    // BotTokenManager-Clone für den Chatters-Poller (#11): bot_token/-user_id/
    // -login + Scope-Check für den Helix-`GET /chat/chatters`-Call. Früh gezogen,
    // da `chat_api_handle` weiter unten beim Pipeline-Aufbau konsumiert wird.
    let chatters_bot_token_manager: Option<Arc<tb_chat::token::BotTokenManager>> =
        chat_api_handle.as_ref().map(|h| h.bot_token_manager());
    let raid_greeting_monitor: Option<Arc<raid_greeting::RaidGreetingMonitor>> = chat_api_handle
        .as_ref()
        .map(|h| {
            Arc::new(raid_greeting::RaidGreetingMonitor::new(
                h.api_for_context(tb_chat::channel_policy::PolicyContext::Raid),
            ))
        });

    // Raid-Verdrahtung: mit Manager + Helix + Krypto-Key sind alle vier
    // Raid-Kopplungen echt (Auto-Raid, Arrival, Score-Refresh, Blacklist-Guard).
    let suppression = Arc::new(std::sync::Mutex::new(ManualRaidSuppression::new()));
    let mut manual_raid_port: Option<Arc<dyn tb_internal_api::ManualRaidPort>> = None;
    let mut raid_oauth_port: Option<Arc<dyn tb_internal_api::RaidOAuthPort>> = None;
    let mut poll_offline_raid_handler: Option<Arc<OfflineRaidHandler>> = None;
    let eventsub_hooks: Arc<dyn EventSubHooks> = match (
        &subscription_manager,
        helix.as_ref().clone(),
        FieldCipher::from_env(),
    ) {
        (Some(manager), Some(helix_client), Ok(cipher)) => {
            let cipher = Arc::new(cipher);

            // Raid-OAuth-Strecke (Welle B): StateStore + AuthWriter +
            // Token-Client zur Composition-Root verdrahten. redirect_uri wie
            // Python (TWITCH_RAID_REDIRECT_URI mit Hardcode-Default,
            // runtime_bootstrap.py:341).
            let raid_redirect_uri = std::env::var("TWITCH_RAID_REDIRECT_URI")
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| {
                    "https://deutsche-deadlock-community.de/callback/twitch".to_string()
                });
            if let Ok(client_id) = std::env::var("TWITCH_CLIENT_ID") {
                // Followup-Service: Discord via Master-Broker, Moderator via
                // Helix, Chat-Begrüßung via Legacy-Python (8779).
                let followup_relay = match BrokerRelay::new(&settings.broker) {
                    Ok(relay) => Some(relay),
                    Err(e) => {
                        tracing::warn!(
                            "BrokerRelay für OAuth-Followups nicht initialisierbar: {e} — \
                             Rollen-Sync/Display-Name entfallen"
                        );
                        None
                    }
                };
                let partner_setup = oauth_followups::build_partner_setup_service(
                    pool.clone(),
                    helix_client.clone(),
                    followup_relay.clone(),
                    chat_api_handle.as_ref().map(|h| h.api()),
                );
                if partner_setup.is_none() {
                    tracing::warn!(
                        "PartnerSetupService nicht konstruierbar — OAuth-Followups entfallen"
                    );
                }
                let raid_oauth_impl = raid_oauth_impl::TbRaidOAuthImpl::new(
                    pool.clone(),
                    tb_raid::state_store::StateStore::new(pool.clone(), raid_redirect_uri.clone()),
                    tb_raid::auth_writer::AuthWriter::new(pool.clone(), cipher.clone()),
                    Arc::new(HelixTokenClient {
                        helix: helix_client.clone(),
                        redirect_uri: raid_redirect_uri.clone(),
                    }),
                    client_id,
                    raid_redirect_uri.clone(),
                    partner_setup,
                )
                .with_requirements_relay(followup_relay);
                raid_oauth_port = Some(Arc::new(raid_oauth_impl));
                tracing::info!(
                    "Raid-OAuth-Strecke nativ aktiv (auth-url/auth-state/block-state/go-url/requirements/oauth-callback inkl. Followups)"
                );
            }

            let token_blacklist = Arc::new(TokenBlacklistStore::new(pool.clone()));
            let refresher = RaidTokenRefresher::new(
                pool.clone(),
                cipher.clone(),
                Arc::new(HelixTokenClient {
                    helix: helix_client.clone(),
                    redirect_uri: raid_redirect_uri.clone(),
                }),
                token_blacklist.clone(),
            );

            // Proaktiver Hintergrund-Token-Refresh (Python `refresh_all_tokens`,
            // periodische Wartung): refresht alle raid-aktivierten Tokens, die
            // in < 2 h ablaufen — verhindert Token-Ablauf im Nutzungspfad statt
            // erst bei Bedarf (mit Latenz) zu erneuern. Eigene Refresher-Instanz
            // (nur Arc-Clones), damit der `token_provider` `cipher` weiter
            // konsumieren kann; cross-process über denselben Advisory-Lock
            // serialisiert wie der reaktive Pfad.
            {
                let maintenance_refresher = RaidTokenRefresher::new(
                    pool.clone(),
                    cipher.clone(),
                    Arc::new(HelixTokenClient {
                        helix: helix_client.clone(),
                        redirect_uri: raid_redirect_uri.clone(),
                    }),
                    token_blacklist.clone(),
                );
                supervisor.spawn("raid_token_proactive_refresh", async move {
                    let mut tick = tokio::time::interval(std::time::Duration::from_secs(300));
                    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                    loop {
                        tick.tick().await;
                        match maintenance_refresher
                            .refresh_all_due(chrono::Utc::now())
                            .await
                        {
                            Ok(refreshed) if refreshed > 0 => tracing::info!(
                                refreshed,
                                "Proaktiver Token-Refresh: Tokens erneuert"
                            ),
                            Ok(_) => {}
                            Err(error) => tracing::error!(
                                %error,
                                "Proaktiver Token-Refresh fehlgeschlagen"
                            ),
                        }
                    }
                });
            }

            let token_provider = Arc::new(TokenProvider::new(
                RaidAuthStore::new(pool.clone(), cipher),
                refresher,
                token_blacklist,
            ));
            let pending = Arc::new(std::sync::Mutex::new(PendingRaidStore::new()));

            // W3b: Observability-Sink + Flow-Services. Der Writer batcht Events
            // asynchron (mpsc) in `twitch_observability_events`; beide Services
            // teilen denselben Sink. Ohne sie liefe der Raid-Pfad exakt wie bisher
            // (None-Default) — mit ihnen werden strukturierte Flow-Events, Counter
            // (raid_flow_started_total, raid_orphan_chat_notification_total) und
            // Analytics-Decisions (followers terminal_decision) persistiert
            // (P2.42/P3.8/P2.44/P2.45/P3.14).
            let obs_sink: Arc<dyn tb_observability::EventSink> =
                Arc::new(tb_observability::ObservabilityWriter::spawn(
                    pool.clone(),
                    tb_observability::DEFAULT_QUEUE_CAPACITY,
                    tb_observability::DEFAULT_BATCH_SIZE,
                ));
            let raid_observability = Arc::new(tb_observability::RaidObservabilityService::new(
                Some(obs_sink.clone()),
            ));
            let analytics_observability =
                Arc::new(tb_observability::AnalyticsObservabilityService::new(
                    Some(obs_sink.clone()),
                    true,                      // runtime_available: Raid-Runtime hier aktiv
                    chat_api_handle.is_some(), // chat_bot_available: bool(get_chat_bot())
                    true,                      // bot_token_manager_available: TokenProvider steht
                ));
            let executor = RaidExecutor::new(
                Arc::new(HelixRaidApi {
                    helix: helix_client.clone(),
                }),
                token_provider.clone(),
                RaidHistoryStore::new(pool.clone()),
                RaidBlacklistStore::new(pool.clone()),
            );
            let sink = Arc::new(RaidArrivalSinkImpl::new(
                pool.clone(),
                pending.clone(),
                suppression.clone(),
                &target_game.to_lowercase(),
                // B3-2d: Chat-Send-Port + DB-Chat-Suppression für die
                // Partner-Raid-Dankesnachricht durchreichen. chat_api ist None,
                // wenn kein Bot-Token gebootet wurde (Python get_chat_bot()→None).
                chat_api_handle.as_ref().map(|h| h.api()),
                Some(Arc::new(
                    tb_chat::moderation::OutboundSuppressionStore::new(pool.clone()),
                )),
                Some(followers.clone()),
            ));
            let mut pipeline = AutoRaidPipeline::new(
                RaidBlacklistStore::new(pool.clone()),
                ScoreStore::new(pool.clone()),
                RaidHistoryStore::new(pool.clone()),
                StrikesStore::new(pool.clone()),
                executor,
                pending.clone(),
                // P2.58: ensure_ready liefert erst bei Subscription-Status
                // `enabled` true (8s-Deadline, 500ms-Poll), statt sofort nach dem
                // best-effort-Create.
                Arc::new(raid_adapters::ManagerArrivalReadinessWithStatusPoll {
                    manager: manager.clone(),
                    status_poll: raid_adapters::RaidSubscriptionStatusPoll {
                        transport: Arc::new(HelixSubscriptionTransport {
                            helix: helix_client.clone(),
                        }),
                        wait_timeout: std::time::Duration::from_secs_f64(8.0),
                        poll_interval: std::time::Duration::from_millis(500),
                    },
                }),
                Some(Arc::new(HelixFallbackStreams {
                    helix: helix_client.clone(),
                })),
                // P2.49: Session-Cache-Backfill (twitch_stream_sessions) vor Helix
                // — günstig, offline-resilient, kein Moderator-Token nötig.
                Some(Arc::new(raid_adapters::CachedFollowerEnricher {
                    followers: followers.clone(),
                    pool: pool.clone(),
                })),
                Some(OutreachBoostStore::new(pool.clone())),
            )
            .with_orphan_replay(sink.clone())
            .with_observability(
                Some(raid_observability.clone()),
                Some(analytics_observability.clone()),
            );
            if let Some(monitor) = raid_greeting_monitor.as_ref() {
                let monitor: Arc<dyn tb_raid::RaidGreetingMonitorPort> = monitor.clone();
                pipeline = pipeline.with_greeting_monitor(monitor);
            }
            let offline = Arc::new(OfflineRaidHandler::new(
                suppression.clone(),
                OfflineEligibilityStore::new(pool.clone()),
                live_state.clone(),
                PartnerRosterStore::new(pool.clone()),
                helix_client.clone(),
                followers.clone(),
                pipeline,
                &target_game,
            ));

            // Orphan-Sweeper: promotet channel.chat.notification ohne
            // korrelierendes Raid-Event nach 15 s Grace als eigenständigen
            // Arrival (Python ruft promote_stale_* bei jedem Tracking-Tick).
            {
                let sweeper_sink = sink.clone();
                supervisor.spawn("raid_orphan_sweeper", async move {
                    let mut tick = tokio::time::interval(std::time::Duration::from_secs(5));
                    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                    loop {
                        tick.tick().await;
                        sweeper_sink.promote_stale_orphans().await;
                    }
                });
            }

            // W3b: periodischer Stale-Pending-Raid-Sweep (Python
            // RaidTrackingRuntimeService.cleanup_stale_pending_raids, bot.py:343,
            // cleanup_timeout_seconds = 300.0). Entfernt Pendings, deren Arrival
            // nie bestätigt wurde; `sweep_stale` loggt je Eintrag das Timeout-Detail.
            {
                let sweep_pending = pending.clone();
                supervisor.spawn("raid_pending_sweeper", async move {
                    let mut tick = tokio::time::interval(std::time::Duration::from_secs(300));
                    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                    loop {
                        tick.tick().await;
                        let swept = {
                            let mut store = match sweep_pending.lock() {
                                Ok(guard) => guard,
                                Err(poisoned) => poisoned.into_inner(),
                            };
                            store.sweep_stale(300.0, None)
                        };
                        if !swept.is_empty() {
                            tracing::debug!(count = swept.len(), "Stale-Pending-Raids entfernt");
                        }
                    }
                });
            }

            // Recruitment-Blacklist-Maintenance: trägt verzögerte externe
            // Recruitment-Ziele nach Ablauf der 48h-Grace tatsächlich in die
            // Raid-Blacklist ein (Python
            // process_due_external_recruitment_blacklist_pending, periodischer Tick).
            {
                let due_sink = sink.clone();
                supervisor.spawn("raid_recruitment_maintenance", async move {
                    let mut tick = tokio::time::interval(std::time::Duration::from_secs(300));
                    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                    loop {
                        tick.tick().await;
                        due_sink.process_due_recruitment_blacklists().await;
                        due_sink.process_due_external_bot_ban_checks().await;
                    }
                });
            }

            // Periodischer Voll-Refresh aller Partner-Raid-Scores (Python
            // maybe_schedule_partner_raid_score_reconciliation, Intervall
            // 300 s): fängt Partner, deren Online/Offline-Events verpasst
            // wurden — sonst veralten deren Scores dauerhaft.
            {
                let refresh_pool = pool.clone();
                supervisor.spawn("raid_partner_score_refresh", async move {
                    let resolver = ScoreRefreshResolver::new(refresh_pool.clone());
                    let mut tick = tokio::time::interval(std::time::Duration::from_secs(300));
                    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                    loop {
                        tick.tick().await;
                        // partner_ops-1: auf `is_partner_active` keyen statt auf
                        // `status='active'` (View-Superset). `status='active'`
                        // umfasst auch departured/pausierte Partner, deren
                        // Operational-Flag aus ist — die Python-Score-Roster
                        // filtert auf `COALESCE(is_partner_active,0)=1`
                        // (partner_scores.py:439), nicht den View-Status.
                        let partners: Result<Vec<(String, String)>, _> = sqlx::query_as(
                            r#"
                            SELECT twitch_user_id, twitch_login
                            FROM twitch_streamers_partner_state
                            WHERE is_partner_active = 1
                              AND COALESCE(twitch_user_id, '') <> ''
                            "#,
                        )
                        .fetch_all(&refresh_pool)
                        .await;
                        match partners {
                            Ok(pairs) if !pairs.is_empty() => {
                                match resolver.refresh_scores(&pairs, chrono::Utc::now()).await {
                                    Ok(written) => tracing::debug!(
                                        partners = pairs.len(),
                                        written,
                                        "Periodischer Partner-Score-Refresh abgeschlossen"
                                    ),
                                    Err(error) => tracing::error!(
                                        %error,
                                        "Periodischer Partner-Score-Refresh fehlgeschlagen"
                                    ),
                                }
                            }
                            Ok(_) => {}
                            Err(error) => tracing::error!(
                                %error,
                                "Partner-Liste für Score-Refresh nicht ladbar"
                            ),
                        }
                    }
                });
            }

            let arrival = RaidArrivalCoordinator::new(
                pool.clone(),
                pending,
                RaidArrivalRuntime::new(sink).with_observability(raid_observability.clone()),
            );
            let blacklist_guard = BlacklistRaidGuard::new(
                RaidBlacklistStore::new(pool.clone()),
                token_provider,
                helix_client,
            );
            manual_raid_port = Some(Arc::new(ManualRaidAdapter {
                handler: offline.clone(),
            }));
            poll_offline_raid_handler = Some(offline.clone());
            // Go-Live-ReAuth-Reminder (B11): braucht den nativen Chat-Send-Pfad.
            // chat_api_handle ist hier noch in Scope (wird erst unten von der
            // Pipeline konsumiert) — gleiches Durchreich-Muster wie partner_setup.
            let reauth_reminder = chat_api_handle
                .as_ref()
                .map(|h| Arc::new(ReauthReminder::new(pool.clone(), h.api())));
            tracing::info!(
                "Raid-EventSub-Hooks aktiv (Auto-Raid, Arrival, Score-Refresh, Blacklist-Guard{})",
                if reauth_reminder.is_some() {
                    ", ReAuth-Reminder"
                } else {
                    ""
                }
            );
            Arc::new(RaidEventSubHooks {
                manager: manager.clone(),
                score_resolver: ScoreRefreshResolver::new(pool.clone()),
                live_state: live_state.clone(),
                offline,
                side_effects: OfflineSideEffects::new(pool.clone()),
                arrival,
                guard: blacklist_guard,
                reauth_reminder,
                pool: pool.clone(),
            })
        }
        (Some(manager), _, cipher) => {
            if let Err(error) = cipher {
                tracing::warn!(
                    %error,
                    "DB_MASTER_KEY_V1 fehlt/ungültig — Raid-Hooks deaktiviert, nur Go-Live-Subscription aktiv"
                );
            }
            Arc::new(SubscriptionEventSubHooks {
                manager: manager.clone(),
            })
        }
        _ => Arc::new(NoopEventSubHooks),
    };
    // Welle B Phase 2: Pipeline auf der gebooteten ChatApi aufbauen. Wrappt
    // die Hooks, damit channel.chat.message in die tb-chat-Pipeline läuft;
    // startet Token-Loop, Promo-Loop, Global-Ban-Sweeper und den
    // 30-min-Subscription-Reconcile.
    let eventsub_hooks: Arc<dyn EventSubHooks> = match chat_api_handle {
        Some(handle) => {
            // !clip: Broadcaster-Token-Clip-Port (Fallback: Bot-Token), nur mit
            // Helix + Krypto-Key.
            let clip_port = chat_wiring::build_clip_port(
                helix.as_ref().clone().map(Arc::new),
                FieldCipher::from_env().ok().map(Arc::new),
                pool.clone(),
                handle.bot_token_manager(),
            );
            // Discord-Sichtbarkeit des Scam-Wächters: postet Bans/Vorschläge in
            // den Aufsichts-Channel (Default 1374364800817303632, per Env
            // überschreibbar) mit Revoke-Button. Ohne Broker → None (kein Post).
            let scam_notifier = scam_notify_impl::build_scam_notifier(
                &settings.broker,
                optional_env_positive_i64("SCAM_GUARD_DISCORD_CHANNEL_ID", 1374364800817303632),
            );
            let runtime = chat_wiring::build_runtime(
                handle,
                pool.clone(),
                chat_wiring::ChatRuntimePorts {
                    manual_raid: manual_raid_port.clone(),
                    clip_port,
                    bot_ban_handler: Some(token_lifecycle_wiring::build_bot_ban_handler(
                        pool.clone(),
                        &settings.broker,
                    )),
                    invite_relay: BrokerRelay::new(&settings.broker).ok(),
                    scam_notifier,
                    raid_greeting: raid_greeting_monitor.clone(),
                },
                eventsub_hooks.clone(),
                supervisor.clone(),
            )
            .await;
            runtime.start_background(subscription_manager.clone());
            // P2.14: einmaliger Eager-Partner-Invite-Backfill (inkl. 60s-Retry,
            // danach Ende). Spawnt selbst einen Task und kehrt sofort zurück;
            // NACH start_background, damit der Promo-Pfad parallel läuft.
            runtime.spawn_partner_invite_backfill();
            // P2.57: den geteilten TimeoutGuard + Bot-User-ID des Chat-Runtimes
            // in den TelemetryStore injizieren, BEVOR `telemetry` an
            // MonitoringEventHandler (Z.~941, .clone()) und EventSubDispatcher
            // (Z.~954, move) verteilt wird. Beide Konsumenten sehen so die
            // with-Guard-Variante; inbound `channel.ban`-Self-Timeouts füttern
            // dieselbe Stumm-Zählung wie der ausgehende Send-Pfad.
            telemetry =
                telemetry.with_bot_timeout_guard(runtime.bot_user_id(), runtime.timeout_guard());
            runtime.hooks.clone()
        }
        None => eventsub_hooks,
    };
    // Go-Live-Enrichment: gezielter /channels-Lookup beim stream.online-Event
    // (sprachfilter-frei) — nur mit HelixClient verfügbar.
    let channel_info_source: Option<Arc<dyn ChannelInfoSource>> = helix
        .as_ref()
        .clone()
        .map(|client| Arc::new(HelixStreamSource { helix: client }) as Arc<dyn ChannelInfoSource>);
    let handler = Arc::new(MonitoringEventHandler::new(
        guard.clone(),
        live_state.clone(),
        tracker.clone(),
        telemetry.clone(),
        eventsub_hooks.clone(),
        channel_info_source,
        Arc::new(tb_monitoring::epoch_clock),
    ));
    let inbox = InboxRuntime::new(
        tb_monitoring::ProcessingInboxStore::new(pool.clone()),
        handler,
    )
    .start();
    let dispatcher = Arc::new(EventSubDispatcher::new(
        guard.clone(),
        inbox.enqueuer(),
        telemetry,
        eventsub_hooks,
        Arc::new(tb_monitoring::epoch_clock),
    ));

    // Nativer EventSub-Webhook-Empfänger (12.6.): eigener Loopback-Listener,
    // Caddy proxyt /twitch/eventsub/callback hierher — ersetzt die
    // Python-Bridge-Strecke (8765 → HTTP-Hop → 8776), die Notifications auf
    // stillen Pfaden verlor. Signatur = Auth; Dedup = persistenter Guard.
    if let Ok(secret) = std::env::var("TWITCH_WEBHOOK_SECRET") {
        let secret = secret.trim().to_string();
        if !secret.is_empty() {
            let receiver_port: u16 = optional_env_u16("TB_EVENTSUB_RECEIVER_PORT", 8786);
            // P1.17/18/20: Revocation-Sink verdrahten. Bei EventSub-Revocation
            // (z. B. stream.online/offline/channel.update widerrufen) untrackt der
            // SubscriptionManager die Sub, sodass der nächste Reconcile-Zyklus sie
            // neu anlegt — Selbstheilung zur Laufzeit statt erst beim Neustart.
            // Ohne aktiven Manager (Webhook-Modus aus) bleibt es beim reinen Logging.
            let mut receiver_builder =
                tb_monitoring::WebhookReceiver::new(secret, dispatcher.clone());
            if let Some(manager) = subscription_manager.as_ref() {
                receiver_builder = receiver_builder.with_revocation_sink(manager.clone());
            }
            let receiver = Arc::new(receiver_builder);
            let router = receiver.router();
            supervisor.spawn("eventsub_webhook_receiver", async move {
                let addr = SocketAddr::from(([127, 0, 0, 1], receiver_port));
                match tokio::net::TcpListener::bind(addr).await {
                    Ok(listener) => {
                        tracing::info!("EventSub-Webhook-Empfänger lauscht auf {addr}");
                        if let Err(e) = axum::serve(listener, router).await {
                            tracing::error!("EventSub-Webhook-Empfänger beendet: {e}");
                        }
                    }
                    Err(e) => {
                        tracing::error!("EventSub-Webhook-Empfänger: Bind-Fehler {addr}: {e}")
                    }
                }
            });
        }
    }

    // Post-Stream-A/B-Reports (B11) + Title-Generator-Jobs: vier best-effort-
    // Background-Tasks, in Python (runtime_bootstrap) gemeinsam im `if cog.api:`-
    // Block gespawnt — gleiches An/Aus-Gate. Backfill der letzten Sessions ohne
    // done-Report beim Start, Retry fehlgeschlagener Reports (alle 30 min nach
    // 1800s), nächtlicher Knowledge-Job (nach 300s, dann täglich) und
    // wöchentlicher Insight-Job (nach 600s, dann alle 7 Tage).
    {
        if tb_analytics::post_stream::post_stream_reports_enabled() {
            let backfill_pool = pool.clone();
            let handle = tokio::spawn(async move {
                tb_analytics::post_stream::backfill_post_stream_reports(&backfill_pool, 3).await;
            });
            watch_one_shot_task("post_stream_report_backfill", handle);
            supervisor.spawn(
                "post_stream_report_retry",
                tb_analytics::post_stream::schedule_report_retry_job(pool.clone(), 1800),
            );
        }
        supervisor.spawn("title_nightly_knowledge", tb_chat::title_jobs::schedule_nightly_knowledge_job(
            pool.clone(),
            300,
        ));
        supervisor.spawn("title_weekly_insight", tb_chat::title_jobs::schedule_weekly_insight_job(
            pool.clone(),
            600,
        ));
        // Self-Learning des Conversation-Scam-Guards: erstmals nach 900s, danach
        // alle 6h aus bestätigten Scams + aufgehobenen Fehlalarmen destillieren.
        supervisor.spawn("conversation_scam_learning", tb_chat::conversation_scam::schedule_scam_learnings(
            pool.clone(),
            900,
        ));
    }

    // P1.21/P1.22/P2.63: 6h-Subs+Ads-Snapshot-Collector (Python
    // `collect_analytics_data`). Je raid-aktivem Partner mit gültigem
    // Broadcaster-Token wird scope-abhängig die Ad-Schedule
    // (`channel:read:ads`) und der Subscriber-Snapshot
    // (`channel:read:subscriptions`) über Helix abgerufen und persistiert.
    // 2s-Throttle zwischen Usern; best-effort, Fehler nur geloggt. Gated auf
    // HelixClient + DB_MASTER_KEY_V1 (Token-Auflösung via Raid-TokenProvider).
    if let Some(helix_client) = helix.as_ref().clone() {
        if let Some((token_provider, raid_auth)) =
            build_telemetry_sub_auth(pool.clone(), helix_client.clone())
        {
            let collector_pool = pool.clone();
            supervisor.spawn("subs_ads_collector", async move {
                const INTERVAL: std::time::Duration = std::time::Duration::from_secs(6 * 60 * 60);
                let mut tick = tokio::time::interval(INTERVAL);
                tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                loop {
                    tick.tick().await;
                    let partners: Result<Vec<(String, String)>, _> = sqlx::query_as(
                        r#"
                        SELECT twitch_user_id, twitch_login
                        FROM twitch_raid_auth
                        WHERE raid_enabled IS TRUE
                          AND needs_reauth = FALSE
                          AND COALESCE(twitch_user_id, '') <> ''
                        "#,
                    )
                    .fetch_all(&collector_pool)
                    .await;
                    let partners = match partners {
                        Ok(rows) => rows,
                        Err(error) => {
                            tracing::error!(%error, "Subs+Ads-Collector: Partner-Liste nicht ladbar");
                            continue;
                        }
                    };
                    for (uid, login) in &partners {
                        let now = chrono::Utc::now();
                        let token = match token_provider
                            .get_valid_token_unrestricted(uid, now)
                            .await
                        {
                            Ok(Some(token)) => token,
                            Ok(None) => continue,
                            Err(error) => {
                                tracing::debug!(uid = %uid, %error, "Subs+Ads-Collector: Token-Lookup fehlgeschlagen");
                                continue;
                            }
                        };
                        let scopes = raid_auth.get_scopes(uid).await.unwrap_or_default();
                        if scopes.iter().any(|s| s == "channel:read:ads") {
                            if let Err(error) =
                                tb_analytics::ads_schedule_collector::collect_ads_schedule_for_user(
                                    &collector_pool,
                                    &helix_client,
                                    uid,
                                    login,
                                    &token,
                                )
                                .await
                            {
                                tracing::warn!(uid = %uid, %error, "Subs+Ads-Collector: Ad-Schedule fehlgeschlagen");
                            }
                        }
                        if scopes.iter().any(|s| s == "channel:read:subscriptions") {
                            if let Err(error) =
                                tb_analytics::subs_snapshot_collector::collect_subs_for_user(
                                    &collector_pool,
                                    &helix_client,
                                    uid,
                                    login,
                                    &token,
                                )
                                .await
                            {
                                tracing::warn!(uid = %uid, %error, "Subs+Ads-Collector: Subs-Snapshot fehlgeschlagen");
                            }
                        }
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    }
                }
            });
        }
    }

    // Highlight-Erstellung bleibt nach Grillme Block 15/20 standardmäßig AUS.
    // Der Port bleibt testbar und kann später bewusst per Opt-in aktiviert werden.
    if opt_in_enabled("TB_HIGHLIGHT_CLIPPER_ENABLED") {
        if let Some(helix_client) = helix.as_ref().clone() {
            let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
            let hc_config = tb_highlight::worker::HighlightClipperConfig::new(
                cwd.join("tools/boon"),
                cwd.join(".venv/bin/yt-dlp"),
            );
            let hc_worker = tb_highlight::worker::HighlightClipperWorker::new(
                pool.clone(),
                Arc::new(HelixVodSource {
                    helix: helix_client,
                }),
                hc_config,
            );
            supervisor.spawn("highlight_clipper", async move {
                loop {
                    hc_worker.run_once().await;
                    tokio::time::sleep(std::time::Duration::from_secs(
                        tb_highlight::config::POLL_INTERVAL_SECONDS,
                    ))
                    .await;
                }
            });
        } else {
            tracing::warn!("HighlightClipper: aktiviert, aber kein HelixClient verfügbar");
        }
    } else {
        tracing::info!("HighlightClipper deaktiviert (TB_HIGHLIGHT_CLIPPER_ENABLED != 1)");
    }

    // Social-Media-Posting-Pipeline (Port von bot/social_media): sieben
    // Hintergrund-Worker. In Python (bootstrap) bedingungslos instanziiert
    // (`if services.api`) → hier ebenso bedingungslos gespawnt; jeder Worker hat
    // ein eigenes Intervall + Initial-Delay und ist still, solange keine Arbeit
    // ansteht (keine onboardeten Plattformen / keine pending Clips). An/Aus wird
    // datengetrieben über `social_media_settings` gesteuert (Consent +
    // Auto-Approve je Plattform) — identisch zu Python.
    {
        // Cipher-freie Worker: Retention-Cleanup, Approval-Queue, Report-Dispatcher.
        let retention = tb_social_media::retention_worker::RetentionWorker::new(pool.clone());
        supervisor.spawn("social_retention_worker", async move { retention.run().await });
        let approval = tb_social_media::approval_worker::ApprovalWorker::new(pool.clone());
        supervisor.spawn("social_approval_worker", async move { approval.run().await });
        let reports = tb_social_media::report_dispatcher::ReportDispatcher::new(pool.clone());
        supervisor.spawn("social_report_dispatcher", async move { reports.run().await });

        // Enrichment: LLM-Dispatcher (Consent aus Settings). Transkription ist
        // entfernt (B15-OFF-transcription: OpenAI-Whisper raus, kein Ersatz) —
        // der Enrichment-Worker läuft ohne Transcriber, die Transkriptions-Stage
        // wird übersprungen (None-Pfad).
        let llm: Arc<dyn tb_social_media::enrich_pipeline::EnrichmentLlm> = Arc::new(
            tb_social_media::llm_dispatch::LlmDispatcher::new(pool.clone()),
        );
        let enrichment =
            tb_social_media::enrichment_worker::EnrichmentWorker::new(pool.clone(), llm);
        supervisor.spawn("social_enrichment_worker", async move { enrichment.run().await });

        // Upload + Token-Refresh + Insights brauchen den Field-Cipher
        // (verschlüsselte Plattform-Tokens). Fehlt DB_MASTER_KEY_V1, laufen nur
        // die cipher-freien Worker — die Token-abhängigen bleiben aus statt zu
        // paniken.
        match tb_crypto::FieldCipher::from_env() {
            Ok(cipher) => {
                let cipher = Arc::new(cipher);
                let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
                let upload_creds = tb_social_media::credentials::CredentialManager::new(
                    pool.clone(),
                    cipher.clone(),
                );
                // yt-dlp wie beim Highlight-Clipper aus dem venv (systemd-PATH
                // enthält ~/.local/bin nicht); clips_dir = Python-Default data/clips.
                let upload =
                    tb_social_media::upload_worker::UploadWorker::new(pool.clone(), upload_creds)
                        .with_yt_dlp(cwd.join(".venv/bin/yt-dlp").to_string_lossy().into_owned());
                supervisor.spawn("social_upload_worker", async move { upload.run().await });

                let refresh_oauth =
                    tb_social_media::oauth::OAuthManager::new(pool.clone(), cipher.clone());
                let refresh = tb_social_media::refresh_worker::TokenRefreshWorker::new(
                    pool.clone(),
                    cipher.clone(),
                    refresh_oauth,
                );
                supervisor.spawn("social_token_refresh_worker", async move { refresh.run().await });

                let insights_creds =
                    tb_social_media::credentials::CredentialManager::new(pool.clone(), cipher);
                let insights = tb_social_media::insights_worker::InsightsWorker::new(
                    pool.clone(),
                    insights_creds,
                );
                supervisor.spawn("social_insights_worker", async move { insights.run().await });
            }
            Err(e) => {
                tracing::warn!(
                    "Social-Media Upload/Refresh/Insights: kein Field-Cipher ({e}) — Worker aus"
                );
            }
        }
        tracing::info!("Social-Media-Pipeline-Worker gestartet (7 Loops)");
    }

    // Poll-Loop: das Cutover-Gate. Default AUS — Python bleibt alleiniger
    // Live-Writer, bis der Flip (04-cutover-plan) explizit erfolgt.
    let poll_enabled = opt_in_enabled("TB_MONITORING_POLL_ENABLED");
    let _poll_stop = if poll_enabled {
        match helix.as_ref().clone() {
            Some(helix_client) => {
                let notify_channel_id: i64 = optional_env_i64("TWITCH_NOTIFY_CHANNEL_ID", 0);
                let sink: Arc<dyn AnnouncementSink> = if notify_channel_id > 0 {
                    match BrokerRelay::new(&settings.broker) {
                        Ok(relay) => {
                            let vod: Arc<dyn VodPreviewSource> = Arc::new(HelixVodPreview {
                                helix: helix_client.clone(),
                            });
                            let profile: Arc<dyn tb_monitoring::ChannelProfileSource> =
                                Arc::new(HelixChannelProfile {
                                    helix: helix_client.clone(),
                                });
                            // Ziel-Guild der Live-Ping-Rolle: Env-Override
                            // (STREAMER_GUILD_ID → MAIN_GUILD_ID) oder Default auf
                            // die Haupt-Community-Guild — identisch zu streamer_link.rs,
                            // wo Discord-Rollen-Operationen bereits auf diese Guild
                            // defaulten. Ohne Default wäre die Auto-Anlage still aus,
                            // sobald die Env-Var fehlt; der Notify-Channel liegt ohnehin
                            // in dieser Guild, also wird die Rolle dort angelegt.
                            let live_ping_guild_id = optional_env_u64_with_fallback(
                                "STREAMER_GUILD_ID",
                                "MAIN_GUILD_ID",
                                1_289_721_245_281_292_288,
                            );
                            tracing::info!(
                                guild_id = live_ping_guild_id,
                                "Live-Ping-Rollen-Auto-Anlage verdrahtet"
                            );
                            let live_ping_role_provider: Option<
                                Arc<dyn tb_monitoring::LivePingRoleProvider>,
                            > = Some(Arc::new(LivePingRoleAuto {
                                relay: Arc::new(relay.clone()),
                                pool: pool.clone(),
                                guild_id: live_ping_guild_id,
                            })
                                as Arc<dyn tb_monitoring::LivePingRoleProvider>);
                            Arc::new(BrokerAnnouncementSink::new(
                                Arc::new(BrokerAnnouncementTransport { relay }),
                                vod,
                                profile,
                                AnnouncementSettings {
                                    notify_channel_id,
                                    alert_mention: std::env::var("TWITCH_ALERT_MENTION").ok(),
                                    ref_code: std::env::var("TWITCH_DISCORD_REF_CODE").ok(),
                                    target_game: target_game.clone(),
                                },
                                live_ping_role_provider,
                            ))
                        }
                        Err(e) => {
                            tracing::warn!(
                                "BrokerRelay nicht initialisierbar: {e} — Announcements aus"
                            );
                            Arc::new(NoopAnnouncementSink)
                        }
                    }
                } else {
                    tracing::info!("TWITCH_NOTIFY_CHANNEL_ID fehlt — Announcements aus");
                    Arc::new(NoopAnnouncementSink)
                };
                let poll_hooks: Arc<dyn PollHooks> = match &subscription_manager {
                    Some(manager) => Arc::new(SubscriptionPollHooks {
                        manager: manager.clone(),
                        pool: pool.clone(),
                        offline_raid: poll_offline_raid_handler.clone(),
                        chat_api: recruit_chat_api.clone(),
                        recruit_last_check: std::sync::Mutex::new(None),
                    }),
                    None => Arc::new(tb_monitoring::NoopPollHooks),
                };
                let language_filters: Vec<String> = language_filters_from_env();
                let source: Arc<dyn StreamSource> = Arc::new(HelixStreamSource {
                    helix: helix_client,
                });
                let engine = Arc::new(PollEngine::new(
                    source,
                    TrackedStore::new(pool.clone()),
                    live_state,
                    SessionStore::new(pool.clone()),
                    tracker,
                    StatsStore::new(pool.clone()),
                    guard,
                    sink,
                    poll_hooks,
                    PollIntervalStore::new(pool.clone()),
                    PollConfig {
                        target_game,
                        language_filters,
                        ..PollConfig::default()
                    },
                ));
                let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
                supervisor.spawn("monitoring_poll_engine", engine.run(stop_rx));
                tracing::info!("Monitoring-Poll-Loop gestartet (Cutover-Gate aktiv)");
                Some(stop_tx)
            }
            None => {
                tracing::error!("TB_MONITORING_POLL_ENABLED=1, aber kein HelixClient — Poll aus");
                None
            }
        }
    } else {
        tracing::info!("Poll-Loop deaktiviert (TB_MONITORING_POLL_ENABLED != 1)");
        None
    };

    // Auch der Twitch-Clip-Fetch bleibt vorerst deaktiviert. Der reparierte
    // Datenpfad kann später mit explizitem Opt-in wieder aufgenommen werden.
    if opt_in_enabled("TB_CLIP_FETCHER_ENABLED") {
        if let Some(ref h) = *helix {
            tb_social_media::build_clip_fetch_task(pool.clone(), std::sync::Arc::new(h.clone()))
                .start();
        } else {
            tracing::warn!("clip_fetch: aktiviert, aber kein HelixClient verfügbar");
        }
    } else {
        tracing::info!("clip_fetch deaktiviert (TB_CLIP_FETCHER_ENABLED != 1)");
    }

    // Scout-Task: entdeckt live Deadlock-Streamer und registriert sie als monitoring-only.
    // Deaktiviert bis TB_SCOUT_ENABLED=1 gesetzt ist.
    if let Some(ref h) = *helix {
        let scout_game =
            std::env::var("TWITCH_TARGET_GAME_NAME").unwrap_or_else(|_| "Deadlock".to_string());
        let scout_lang_filters: Vec<String> = language_filters_from_env();
        tb_monitoring::build_scout_task(
            pool.clone(),
            std::sync::Arc::new(h.clone()),
            scout_game,
            scout_lang_filters,
        )
        // Session-Priming neu entdeckter Kanäle (Python
        // `_prime_monitored_only_sessions`) — der wertschöpfende Hook, voll
        // verdrahtet über den bestehenden SessionTracker.
        .with_session_tracker(scout_tracker)
        // Chat-Sync-Port: hält die Heal-Prädikate (monitoring-only ⇒ kein Heal)
        // und meldet den fehlenden anonymen Read-Membership-Handle als Handoff
        // (EventSub-Modell, s. scout_chat.rs). Kein Override der Defaults ⇒ kein
        // An/Aus-Zustandswechsel ggü. dem bisherigen NoopScoutChatSink.
        .with_chat_sink(std::sync::Arc::new(scout_chat::ScoutChatAdapter::new()))
        .start_if_enabled();
    }

    // Token-Lifecycle-Reaktionen (Block 4): Admin-Embed + User-DM bei
    // Token-Fehler (1×/Streamer), 7-Tage-Grace-Sweep mit Rollen-Entzug
    // (stündlich) und Blacklist-Cleanup >30 Tage (3,5 h) — alles über den
    // F4-Master-Broker, da der Twitch-Bot keinen Discord-Zugang hat.
    token_lifecycle_wiring::spawn_token_lifecycle_schedulers(
        &supervisor,
        pool.clone(),
        &settings.broker,
    );

    // Shadow-Review-Ausgang (B19): leitet gestagte Shadow-KI-Antworten periodisch
    // in den Engagement-Review-Discord-Kanal weiter (Master-Broker). Default AUS —
    // startet nur mit gesetztem ENGAGEMENT_SHADOW_REVIEW_CHANNEL_ID; ohne opt-in
    // output_mode='shadow' ist die Queue ohnehin leer (no-op).
    shadow_review_wiring::spawn_shadow_review_scheduler(
        &supervisor,
        pool.clone(),
        &settings.broker,
    );

    // Streamer-Link-Matcher: verknüpft neue Twitch-Partner mit ihrem Discord-Account.
    // Läuft alle 6h, ist still wenn keine neuen Kandidaten vorhanden.
    if let Ok(sl_relay) = BrokerRelay::new(&settings.broker) {
        let sl_config = Arc::new(streamer_link::StreamerLinkConfig::from_env());
        let sl_pool = pool.clone();
        let sl_base = format!("http://127.0.0.1:{port}");
        let sl_token = settings.internal_api.token.clone();
        supervisor.spawn("streamer_link_matcher", streamer_link::streamer_link_task(
            sl_pool, sl_relay, sl_config, sl_base, sl_token,
        ));
    }

    // Chatters-Poller (#11): 30s-Collect (Helix `GET /chat/chatters` → Lurker-/
    // Presence-Spiegelung) + stündliche Raid-Retention. Der Collect-Loop läuft
    // nur, wenn Bot-Token-Manager + HelixClient + Streamer-TokenProvider
    // verfügbar sind; die Retention braucht kein Token-Plumbing und läuft immer.
    {
        let chatters_auth = chatters_wiring::build_bot_chatter_auth(chatters_bot_token_manager);
        // Bot-User-ID für den Mod-Self-Heal-Provisioner (sync benötigt) aus dem
        // Bot-Auth-Port auflösen.
        let chatters_bot_user_id = match chatters_auth.as_ref() {
            Some(auth) => auth.bot_user_id().await,
            None => None,
        };
        // Mod-TokenProvider + HelixClient → Streamer-Token-Quelle + Self-Heal-
        // Provisioner (P1.2). Ohne DB_MASTER_KEY_V1 oder Helix bleibt der
        // 403-Self-Heal aus; der Collect-Loop startet dann nicht (kein Poll).
        let chatters_mod_token_provider = helix
            .as_ref()
            .clone()
            .and_then(|helix_client| build_moderator_token_provider(pool.clone(), helix_client));
        let chatters_streamer_tokens =
            chatters_wiring::build_streamer_token_source(chatters_mod_token_provider.clone());
        let chatters_fetcher = chatters_wiring::build_chatters_fetcher(helix.as_ref().clone());
        let chatters_provisioner: Option<Arc<dyn tb_monitoring::ModeratorProvisioner>> = match (
            chatters_mod_token_provider,
            helix.as_ref().clone(),
            chatters_bot_user_id,
        ) {
            (Some(token_provider), Some(helix_client), Some(bot_user_id)) => {
                Some(Arc::new(eventsub_hooks::HelixModeratorProvisioner::new(
                    token_provider,
                    helix_client,
                    bot_user_id,
                )))
            }
            _ => None,
        };
        chatters_wiring::spawn_chatters_schedulers(
            &supervisor,
            pool.clone(),
            chatters_auth,
            chatters_streamer_tokens,
            chatters_fetcher,
            chatters_provisioner,
        );
    }

    irc_lurker_wiring::spawn_irc_lurker(&supervisor, pool.clone());

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let token = settings.internal_api.token.clone();
    let legacy_proxy = std::env::var("TB_INTERNAL_API_LEGACY_FALLBACK_URL")
        .ok()
        .map(|raw| raw.trim().to_string())
        .filter(|raw| !raw.is_empty())
        .map(|url| {
            tracing::info!("Legacy-Fallback aktiv: unbekannte interne-API-Routen → {url}");
            Arc::new(tb_internal_api::LegacyProxy::new(url))
        });
    // EventSub-Sektion von GET /stats: Live-`current`-Snapshot aus dem nativen
    // SubscriptionManager (Webhook-Modus). Ohne Manager (kein Helix) → None,
    // dann bleibt nur der DB-Capacity-Block (wie bisher).
    let eventsub_stats: Option<Arc<dyn tb_internal_api::EventSubStatsSource>> =
        subscription_manager.as_ref().map(|mgr| {
            Arc::new(eventsub_stats_adapter::ManagerEventSubStats::new(
                mgr.clone(),
                pool.clone(),
            )) as Arc<dyn tb_internal_api::EventSubStatsSource>
        });
    // Discord-Streamer-Rollen-Sync für POST …/discord-profile (Master-Broker).
    // Frischer Relay aus der Broker-Config; ohne Relay loggt der Port nur einen
    // Hinweis (best-effort, wie Python `sync_streamer_role`).
    let discord_role: Option<Arc<dyn tb_internal_api::DiscordRolePort>> = Some(Arc::new(
        oauth_followups::BrokerDiscordDirectory::from_env(BrokerRelay::new(&settings.broker).ok()),
    )
        as Arc<dyn tb_internal_api::DiscordRolePort>);
    // Bot-Token-Bridge (F3): Owner-Chat-Action sendet über den live rotierten
    // Bot-User-Token. `None`, wenn der native Chat aus ist (kein Token gebootet)
    // → der Handler antwortet 503 statt stumm zu scheitern.
    let chat_action: Option<Arc<dyn tb_internal_api::ChatActionPort>> =
        chat_wiring::build_chat_action_port(chat_action_api, pool.clone());
    let scam_revoke: Option<Arc<dyn tb_internal_api::ScamRevokePort>> =
        scam_revoke_impl::build_scam_revoke_port(scam_revoke_api, pool.clone());
    let scam_enforce: Option<Arc<dyn tb_internal_api::ScamEnforcePort>> =
        scam_enforce_impl::build_scam_enforce_port(scam_enforce_api, pool.clone());
    let bulk_reauth: Option<Arc<dyn tb_internal_api::handlers::reauth_all::BulkReauthPort>> =
        Some(Arc::new(InternalBulkReauthAdapter {
            store: tb_raid::ReauthAdminStore::new(pool.clone()),
        }));
    let app = build_internal_router(
        pool,
        token,
        helix,
        Some(dispatcher),
        manual_raid_port,
        raid_oauth_port,
        eventsub_stats,
        discord_role,
        chat_action,
        scam_revoke,
        scam_enforce,
        bulk_reauth,
        legacy_proxy,
    );

    // Block 10: Split-Deployment-Härtung vor dem Bind. `role = None` liest die
    // Runtime-Rolle aus der Umgebung (kombiniertes Deployment: tb-bot fährt die
    // interne API selbst). Bei Fehlkonfiguration sauberer Abbruch (Log + exit),
    // kein Panic im Prod-Pfad. Härtung ist via TWITCH_RUNTIME_ENFORCE=0
    // abschaltbar (Python-Parität).
    match tb_internal_api::enforce_internal_api_runtime(None, port) {
        Ok(role) => {
            tracing::info!(runtime_role = %role, port, "Internal-API Runtime-Härtung bestanden");
        }
        Err(e) => {
            tracing::error!("Internal-API Runtime-Härtung verletzt: {e}");
            std::process::exit(1);
        }
    }

    tracing::info!("tb-bot lauscht auf {addr}");
    let listener = bind_internal_listener_with_retry(addr)
        .await
        .unwrap_or_else(|e| {
            tracing::error!("Bind-Fehler auf {addr}: {e}");
            std::process::exit(1);
        });

    if let Err(error) = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    {
        tracing::error!(%error, "Internal-API Server beendet");
        std::process::exit(1);
    }
}

/// Startup-Cleanup + periodischer Core-Sub-Reconcile (alle 6h).
///
/// Python-Äquivalent: `_cleanup_old_eventsub_subscriptions` (Webhook-Start)
/// + `_subscribe_core_eventsub_webhooks` (per-Kanal bei Webhook-Start).
///
/// Läuft einmal beim Start und danach jede 6h:
/// 1. Aktive Partner-User-IDs aus DB laden
/// 2. `cleanup_stale` — veraltete Twitch-Subs für entfernte Partner löschen
/// 3. `ensure_core_subscriptions` — stream.online/offline/channel.update für
///    alle aktiven Partner sicherstellen (fängt neue Kanäle + revoked Subs)
///
/// Baut die Auth-Bausteine für die Broadcaster-Telemetrie-Subs (B9):
/// `TokenProvider` für den Broadcaster-User-Token (refresht bei Ablauf) +
/// `RaidAuthStore` für dessen Scopes. `None`, wenn `DB_MASTER_KEY_V1` fehlt —
/// dann macht der Reconcile-Loop nur die App-Token-Core-Subs. Der Broadcaster-
/// Token-Refresh gehört in Rust ohnehin dem Raid-Pfad (kein Dual-Refresh).
fn build_telemetry_sub_auth(
    pool: sqlx::PgPool,
    helix_client: tb_transport_twitch::HelixClient,
) -> Option<(Arc<TokenProvider>, RaidAuthStore)> {
    let cipher = Arc::new(FieldCipher::from_env().ok()?);
    let redirect_uri = std::env::var("TWITCH_RAID_REDIRECT_URI")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "https://deutsche-deadlock-community.de/callback/twitch".to_string());
    let token_blacklist = Arc::new(TokenBlacklistStore::new(pool.clone()));
    let refresher = RaidTokenRefresher::new(
        pool.clone(),
        cipher.clone(),
        Arc::new(HelixTokenClient {
            helix: helix_client,
            redirect_uri,
        }),
        token_blacklist.clone(),
    );
    let raid_auth = RaidAuthStore::new(pool.clone(), cipher.clone());
    let token_provider = Arc::new(TokenProvider::new(
        RaidAuthStore::new(pool, cipher),
        refresher,
        token_blacklist,
    ));
    Some((token_provider, raid_auth))
}

/// P2.56: Broadcaster-Token-Fallback für Moderator-Telemetrie-Subs.
///
/// Reicht den schon vorhandenen Broadcaster-/Raid-Auth-Tokenpfad an den
/// `SubscriptionManager` durch — identisch zum B9-Pfad in
/// [`subscription_maintenance_loop`]: `get_valid_token_unrestricted` löst den
/// (bei Ablauf refreshten) Broadcaster-Token auf, `RaidAuthStore::get_scopes`
/// liefert dessen Scopes. `needs_reauth`/Blacklist/Cooldown landen in
/// `Ok(None)`, Fehler in `Err` — beide → `None` (still überspringen, kein
/// 401-Spam). Doppel-Sends sind ausgeschlossen: der Manager probiert den
/// Broadcaster-Token erst nach gescheitertem Bot-Versuch und ist pro
/// `(sub_type, broadcaster_id)` dedupliziert (`is_tracked`).
struct RaidBroadcasterEventSubTokenProvider {
    token_provider: Arc<TokenProvider>,
    raid_auth: RaidAuthStore,
}

#[async_trait::async_trait]
impl tb_monitoring::BroadcasterEventSubTokenProvider for RaidBroadcasterEventSubTokenProvider {
    async fn eventsub_broadcaster_token(
        &self,
        broadcaster_id: &str,
        login: &str,
    ) -> Option<tb_monitoring::EventSubUserToken> {
        match self
            .token_provider
            .get_valid_token_unrestricted(broadcaster_id, chrono::Utc::now())
            .await
        {
            Ok(Some(token)) => {
                let scopes = self
                    .raid_auth
                    .get_scopes(broadcaster_id)
                    .await
                    .unwrap_or_default();
                Some(tb_monitoring::EventSubUserToken::new(token, scopes))
            }
            Ok(None) => None,
            Err(e) => {
                tracing::debug!(
                    login,
                    "P2.56: Broadcaster-Token-Lookup für Moderator-Telemetrie fehlgeschlagen: {e}"
                );
                None
            }
        }
    }
}

/// Baut den `TokenProvider` für den Mod-Provisioner (P1.2): löst den
/// Streamer-Token (`get_valid_token_unrestricted`) für den Bot-Remod auf.
/// `None`, wenn `DB_MASTER_KEY_V1` fehlt — dann bleibt der 403-Pfad beim
/// Alt-Verhalten. Eigene Instanz mit Arc-Clones; derselbe Advisory-Lock
/// serialisiert den Refresh cross-process wie der Raid-Pfad (kein Dual-Refresh).
fn build_moderator_token_provider(
    pool: sqlx::PgPool,
    helix_client: tb_transport_twitch::HelixClient,
) -> Option<Arc<TokenProvider>> {
    let cipher = Arc::new(FieldCipher::from_env().ok()?);
    let redirect_uri = std::env::var("TWITCH_RAID_REDIRECT_URI")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "https://deutsche-deadlock-community.de/callback/twitch".to_string());
    let token_blacklist = Arc::new(TokenBlacklistStore::new(pool.clone()));
    let refresher = RaidTokenRefresher::new(
        pool.clone(),
        cipher.clone(),
        Arc::new(HelixTokenClient {
            helix: helix_client,
            redirect_uri,
        }),
        token_blacklist.clone(),
    );
    Some(Arc::new(TokenProvider::new(
        RaidAuthStore::new(pool, cipher),
        refresher,
        token_blacklist,
    )))
}

async fn subscription_maintenance_loop(
    manager: std::sync::Arc<tb_monitoring::SubscriptionManager>,
    pool: sqlx::PgPool,
    telemetry_auth: Option<(Arc<TokenProvider>, RaidAuthStore)>,
) {
    const INTERVAL: std::time::Duration = std::time::Duration::from_secs(6 * 60 * 60);
    let mut tick = tokio::time::interval(INTERVAL);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tick.tick().await;
        let rows = match chat_wiring::select_eventsub_subscription_broadcasters(&pool).await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("sub-maintenance: Subscription-Roster-Query fehlgeschlagen: {e}");
                continue;
            }
        };

        let active_ids: std::collections::HashSet<String> =
            rows.iter().map(|row| row.twitch_user_id.clone()).collect();

        let deleted = if active_ids.is_empty() {
            tracing::warn!(
                "sub-maintenance: Subscription-Roster leer, Stale-Cleanup fail-open übersprungen"
            );
            0
        } else {
            let deleted = manager.cleanup_stale(&active_ids).await;
            tracing::debug!(deleted, "sub-maintenance: Stale-Cleanup abgeschlossen");
            deleted
        };

        let mut ensured = 0usize;
        let mut ensure_failed = 0usize;
        let mut telemetry_ensured = 0usize;
        let mut raid_failed = 0usize;
        for row in &rows {
            let login = row.login.as_str();
            let uid = row.twitch_user_id.as_str();
            if row.core_subscriptions {
                let report = manager.ensure_core_subscriptions(uid, login).await;
                ensured += report.succeeded;
                ensure_failed += report.failed();
            }

            // channel.raid (Arrival) proaktiv pro Partner abonnieren — Python
            // (eventsub_mixin.py:2666) subscribt channel.raid für ALLE Streamer,
            // damit eingehende/manuelle Raids erkannt werden, unabhängig von
            // eigenen Outgoing-Raids. Partner-only: die Raid-Dankesnachricht ist
            // partner-gebunden, monitored-only Kanäle haben keinen Konsumenten.
            if row.is_partner && !manager.ensure_raid_subscription(uid, login).await {
                raid_failed += 1;
                tracing::warn!(
                    uid,
                    login,
                    "sub-maintenance: channel.raid-Ensure fehlgeschlagen"
                );
            }

            // Broadcaster-Telemetrie-Subs (B9): nur mit gültigem Broadcaster-
            // Token + dessen Scopes. needs_reauth/blacklist → still überspringen.
            if row.core_subscriptions {
                if let Some((token_provider, raid_auth)) = &telemetry_auth {
                    match token_provider
                        .get_valid_token_unrestricted(uid, chrono::Utc::now())
                        .await
                    {
                        Ok(Some(token)) => {
                            let scopes = raid_auth.get_scopes(uid).await.unwrap_or_default();
                            telemetry_ensured += manager
                                .ensure_broadcaster_telemetry_subscriptions(
                                    uid, login, &token, &scopes,
                                )
                                .await;
                        }
                        Ok(None) => {}
                        Err(e) => {
                            tracing::debug!(uid = %uid, "sub-maintenance: Telemetrie-Token-Lookup fehlgeschlagen: {e}");
                        }
                    }
                }
            }
        }
        tracing::info!(
            kanäle = rows.len(),
            ensured,
            ensure_failed,
            raid_failed,
            telemetry_ensured,
            deleted,
            "sub-maintenance: Core-Sub-Reconcile abgeschlossen"
        );
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
    use sqlx::PgPool;

    async fn pool_in_schema(schema: &str) -> Option<PgPool> {
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
            .ok()?;
        sqlx::query(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(&admin)
            .await
            .ok()?;
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .ok()?;
        admin.close().await;

        let opts = PgConnectOptions::from_str(&dsn)
            .ok()?
            .options([("search_path", schema)]);
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(opts)
            .await
            .ok()?;
        sqlx::query(
            "CREATE TABLE twitch_partners (
                twitch_login TEXT PRIMARY KEY,
                status TEXT,
                admin_archived_at TEXT,
                inactivity_flagged_at TEXT
            )",
        )
        .execute(&pool)
        .await
        .ok()?;
        Some(pool)
    }

    #[tokio::test]
    async fn auto_archive_markiert_nur_inaktivitaetsflag() {
        let Some(pool) = pool_in_schema("tb_bot_inactivity_mark").await else {
            return;
        };
        sqlx::query("INSERT INTO twitch_partners (twitch_login) VALUES ('sleepy')")
            .execute(&pool)
            .await
            .unwrap();

        assert!(super::mark_partner_inactivity_flagged(&pool, "Sleepy")
            .await
            .unwrap());
        let (admin_archived_at, inactivity_flagged_at): (Option<String>, Option<String>) =
            sqlx::query_as(
                "SELECT admin_archived_at, inactivity_flagged_at
                   FROM twitch_partners WHERE twitch_login = 'sleepy'",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(
            admin_archived_at, None,
            "admin_archived_at bleibt unangetastet"
        );
        assert!(
            inactivity_flagged_at.is_some(),
            "Inaktivitaet wird rein informativ markiert"
        );
    }

    #[tokio::test]
    async fn auto_unarchive_cleart_nur_inaktivitaetsflag() {
        let Some(pool) = pool_in_schema("tb_bot_inactivity_clear").await else {
            return;
        };
        sqlx::query(
            "INSERT INTO twitch_partners
                (twitch_login, admin_archived_at, inactivity_flagged_at)
             VALUES ('sleepy', 'operator', '2026-06-01T00:00:00Z')",
        )
        .execute(&pool)
        .await
        .unwrap();

        assert!(super::clear_partner_inactivity_flag(&pool, "sleepy")
            .await
            .unwrap());
        let (admin_archived_at, inactivity_flagged_at): (Option<String>, Option<String>) =
            sqlx::query_as(
                "SELECT admin_archived_at, inactivity_flagged_at
                   FROM twitch_partners WHERE twitch_login = 'sleepy'",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(
            admin_archived_at.as_deref(),
            Some("operator"),
            "Operator-Archivierung bleibt erhalten"
        );
        assert_eq!(inactivity_flagged_at, None);
    }
}
