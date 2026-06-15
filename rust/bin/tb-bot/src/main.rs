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
//!   TB_CLIP_FETCHER_ENABLED       — "1" startet den Clip-Fetch-Task (default aus;
//!                                   benötigt Helix-Client)
//!   TB_SCOUT_ENABLED              — "1" startet den Scout-Task für live Deadlock-DE-Streams
//!                                   (default aus; benötigt Helix-Client)

mod auto_raid;
mod chat_wiring;
mod streamer_link;
mod confirm_resolver;
mod eventsub_hooks;
mod eventsub_stats_adapter;
mod oauth_followups;
mod offline_side_effects;
mod partner_lookup;
mod partner_recruit;
mod raid_adapters;
mod raid_arrival_wiring;
mod raid_oauth_impl;
mod reauth_reminder;
mod score_refresh;
mod token_lifecycle_wiring;
mod wiring;

use std::net::SocketAddr;
use std::sync::Arc;
use tb_config::Settings;
use tb_crypto::FieldCipher;
use tb_internal_api::build_internal_router;
use tb_monitoring::poller::{ChannelInfoSource, PollHooks, StreamSource};
use tb_monitoring::sessions::store::SessionStore;
use tb_monitoring::sessions::tracker::FollowerCountSource;
use tb_monitoring::{
    AnnounceConfigStore, AnnouncementSettings, AnnouncementSink, BrokerAnnouncementSink,
    CapacitySnapshotStore, EventSubDispatcher, EventSubHooks, ExpSessionStore, ExpSessionTracker,
    GuardStore, InboxRuntime, LiveStateStore, MonitoringEventHandler, NoFollowerSource,
    NoopAnnouncementSink, NoopEventSubHooks, PollConfig, PollEngine, PollIntervalStore,
    SessionTracker, StatsStore, SubscriptionConfig, SubscriptionManager, TelemetryStore,
    TrackedStore, VodPreviewSource,
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
use reauth_reminder::ReauthReminder;
use offline_side_effects::OfflineSideEffects;
use raid_adapters::{
    HelixFallbackStreams, HelixRaidApi, HelixTokenClient, ManagerArrivalReadiness,
    ManualRaidAdapter,
};
use raid_arrival_wiring::RaidArrivalSinkImpl;
use score_refresh::ScoreRefreshResolver;
use wiring::{
    BrokerAnnouncementTransport, HelixFollowerSource, HelixStreamSource,
    HelixSubscriptionTransport, HelixVodPreview, HelixVodSource, LivePingRoleAuto,
    SubscriptionEventSubHooks,
};

/// Hooks des Poll-Loops: Go-Live → stream.offline-Subscription (wie EventSub),
/// Auto-Archiv/Entarchiv inaktiver Partner und Score-Refreshes pro Tick.
struct SubscriptionPollHooks {
    manager: Arc<SubscriptionManager>,
    pool: sqlx::PgPool,
    /// ChatApi für den Partner-Recruiting-Outreach; `None` ohne Bot-Token.
    chat_api: Option<Arc<dyn tb_chat::ChatApi>>,
    /// Letzter Recruiting-Durchlauf (interne 30-min-Drosselung, Python
    /// `_last_recruit_check`).
    recruit_last_check: std::sync::Mutex<Option<std::time::Instant>>,
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

    /// Inaktiver Partner (> N Tage kein Deadlock-Stream) → archivieren.
    /// Mirror von Python `set_streamer_archive_state(archived=True)`:
    /// `admin_archived_at` im aktiven `twitch_partners`-Eintrag + `archived_at`
    /// in `twitch_streamers`. Ohne diesen Sink blieben Karteileichen sichtbar.
    async fn on_auto_archive(&self, login: &str) -> bool {
        let mut changed = false;
        match sqlx::query(
            "UPDATE twitch_partners SET admin_archived_at = NOW() \
             WHERE LOWER(twitch_login) = LOWER($1) AND admin_archived_at IS NULL",
        )
        .bind(login)
        .execute(&self.pool)
        .await
        {
            Ok(r) => changed |= r.rows_affected() > 0,
            Err(e) => {
                tracing::warn!(login, "auto-archive (twitch_partners) fehlgeschlagen: {e}");
                return false;
            }
        }
        match sqlx::query(
            "UPDATE twitch_streamers SET archived_at = NOW() \
             WHERE LOWER(twitch_login) = LOWER($1) AND archived_at IS NULL",
        )
        .bind(login)
        .execute(&self.pool)
        .await
        {
            Ok(r) => changed |= r.rows_affected() > 0,
            Err(e) => tracing::warn!(login, "auto-archive (twitch_streamers) fehlgeschlagen: {e}"),
        }
        if changed {
            tracing::info!(login, "Partner automatisch archiviert (inaktiv)");
        }
        changed
    }

    /// Archivierter Partner streamt wieder Deadlock → entarchivieren
    /// (`set_streamer_archive_state(archived=False)`).
    async fn on_auto_unarchive(&self, login: &str) -> bool {
        let mut changed = false;
        match sqlx::query(
            "UPDATE twitch_partners SET admin_archived_at = NULL \
             WHERE LOWER(twitch_login) = LOWER($1) AND admin_archived_at IS NOT NULL",
        )
        .bind(login)
        .execute(&self.pool)
        .await
        {
            Ok(r) => changed |= r.rows_affected() > 0,
            Err(e) => {
                tracing::warn!(login, "auto-unarchive (twitch_partners) fehlgeschlagen: {e}");
                return false;
            }
        }
        match sqlx::query(
            "UPDATE twitch_streamers SET archived_at = NULL \
             WHERE LOWER(twitch_login) = LOWER($1) AND archived_at IS NOT NULL",
        )
        .bind(login)
        .execute(&self.pool)
        .await
        {
            Ok(r) => changed |= r.rows_affected() > 0,
            Err(e) => tracing::warn!(login, "auto-unarchive (twitch_streamers) fehlgeschlagen: {e}"),
        }
        if changed {
            tracing::info!(login, "Partner automatisch entarchiviert (wieder Deadlock live)");
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
                tokio::spawn(async move {
                    partner_recruit::run_partner_recruit(&pool, &chat_api, &category_streams).await;
                });
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

    let settings = Settings::from_env().unwrap_or_else(|e| {
        tracing::error!("Konfigurationsfehler: {e}");
        std::process::exit(1);
    });

    let pool = tb_db::connect(&settings.db).await.unwrap_or_else(|e| {
        tracing::error!("DB-Verbindungsfehler: {e}");
        std::process::exit(1);
    });

    // Native sqlx-Migrationen anwenden (idempotent, CREATE ... IF NOT EXISTS).
    // Gegen das bestehende Prod-Schema no-op außer fehlenden Indizes/Tabellen;
    // Python bleibt im Strangler-Betrieb Schema-Owner. Fehler werden geloggt,
    // brechen den Bot aber NICHT ab. Abschaltbar via TB_DB_MIGRATE=0.
    if std::env::var("TB_DB_MIGRATE").as_deref() != Ok("0") {
        match tb_db::run_migrations(&pool).await {
            Ok(()) => tracing::info!("DB-Migrationen angewendet (oder bereits aktuell)"),
            Err(e) => tracing::warn!("DB-Migrationen fehlgeschlagen (übersprungen): {e}"),
        }
    }

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8776);

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
    let telemetry = TelemetryStore::new(pool.clone());
    let live_state = LiveStateStore::new(pool.clone());
    let followers: Arc<dyn FollowerCountSource> = match helix.as_ref().clone() {
        Some(helix_client) => Arc::new(HelixFollowerSource {
            helix: helix_client,
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
                let manager = Arc::new(SubscriptionManager::new(
                    Arc::new(HelixSubscriptionTransport {
                        helix: helix_client.clone(),
                    }),
                    SubscriptionConfig {
                        callback_url,
                        secret,
                    },
                    CapacitySnapshotStore::new(pool.clone()),
                ));
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
                    tokio::spawn(async move {
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
    // Welle B Phase 1: Bot-Token booten + ChatApi bauen (TB_CHAT_ENABLED=1).
    // Muss VOR der Hooks-Komposition passieren, damit die OAuth-Followup-
    // Begrüßung den nativen Send statt des Python-Umwegs (8779) nutzt.
    let chat_api_handle = chat_wiring::try_build_api(helix.as_ref().clone()).await;
    // ChatApi-Clone für den Partner-Recruiting-Outreach FRÜH ziehen — das Handle
    // wird weiter unten (Pipeline-Aufbau) konsumiert und ist bei der
    // SubscriptionPollHooks-Konstruktion sonst nicht mehr im Scope.
    let recruit_chat_api: Option<Arc<dyn tb_chat::ChatApi>> =
        chat_api_handle.as_ref().map(|h| h.api.clone());
    // Bot-Token-Bridge (F3): ChatApi-Clone für die Owner-Chat-Action der
    // internen API (POST /streamers/:login/chat-action). Früh gezogen, da das
    // Handle weiter unten beim Pipeline-Aufbau konsumiert wird. Der Send läuft
    // über den live rotierten Bot-User-Token (ChatApi → BotTokenManager).
    let chat_action_api: Option<Arc<dyn tb_chat::ChatApi>> =
        chat_api_handle.as_ref().map(|h| h.api.clone());

    // Raid-Verdrahtung: mit Manager + Helix + Krypto-Key sind alle vier
    // Raid-Kopplungen echt (Auto-Raid, Arrival, Score-Refresh, Blacklist-Guard).
    let suppression = Arc::new(std::sync::Mutex::new(ManualRaidSuppression::new()));
    let mut manual_raid_port: Option<Arc<dyn tb_internal_api::ManualRaidPort>> = None;
    let mut raid_oauth_port: Option<Arc<dyn tb_internal_api::RaidOAuthPort>> = None;
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
                    followup_relay,
                    chat_api_handle.as_ref().map(|h| h.api.clone()),
                );
                if partner_setup.is_none() {
                    tracing::warn!(
                        "PartnerSetupService nicht konstruierbar (TWITCH_INTERNAL_API_TOKEN fehlt) \
                         — OAuth-Followups entfallen"
                    );
                }
                raid_oauth_port = Some(Arc::new(raid_oauth_impl::TbRaidOAuthImpl::new(
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
                )));
                tracing::info!(
                    "Raid-OAuth-Strecke nativ aktiv (auth-url/auth-state/block-state/go-url/oauth-callback inkl. Followups); requirements weiter via Proxy"
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
            let token_provider = Arc::new(TokenProvider::new(
                RaidAuthStore::new(pool.clone(), cipher),
                refresher,
                token_blacklist,
            ));
            let pending = Arc::new(std::sync::Mutex::new(PendingRaidStore::new()));
            let executor = RaidExecutor::new(
                Arc::new(HelixRaidApi {
                    helix: helix_client.clone(),
                }),
                token_provider.clone(),
                RaidHistoryStore::new(pool.clone()),
            );
            let pipeline = AutoRaidPipeline::new(
                RaidBlacklistStore::new(pool.clone()),
                ScoreStore::new(pool.clone()),
                RaidHistoryStore::new(pool.clone()),
                StrikesStore::new(pool.clone()),
                executor,
                pending.clone(),
                Arc::new(ManagerArrivalReadiness {
                    manager: manager.clone(),
                }),
                Some(Arc::new(HelixFallbackStreams {
                    helix: helix_client.clone(),
                })),
                Some(OutreachBoostStore::new(pool.clone())),
            );
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
            let sink = Arc::new(RaidArrivalSinkImpl::new(
                pool.clone(),
                pending.clone(),
                suppression.clone(),
                &target_game.to_lowercase(),
                // B3-2d: Chat-Send-Port + DB-Chat-Suppression für die
                // Partner-Raid-Dankesnachricht durchreichen. chat_api ist None,
                // wenn kein Bot-Token gebootet wurde (Python get_chat_bot()→None).
                chat_api_handle.as_ref().map(|h| h.api.clone()),
                Some(Arc::new(
                    tb_chat::moderation::OutboundSuppressionStore::new(pool.clone()),
                )),
            ));

            // Orphan-Sweeper: promotet channel.chat.notification ohne
            // korrelierendes Raid-Event nach 15 s Grace als eigenständigen
            // Arrival (Python ruft promote_stale_* bei jedem Tracking-Tick).
            {
                let sweeper_sink = sink.clone();
                tokio::spawn(async move {
                    let mut tick = tokio::time::interval(std::time::Duration::from_secs(5));
                    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                    loop {
                        tick.tick().await;
                        sweeper_sink.promote_stale_orphans().await;
                    }
                });
            }

            // Recruitment-Blacklist-Maintenance: trägt verzögerte externe
            // Recruitment-Ziele nach Ablauf der 48h-Grace tatsächlich in die
            // Raid-Blacklist ein (Python
            // process_due_external_recruitment_blacklist_pending, periodischer Tick).
            {
                let due_sink = sink.clone();
                tokio::spawn(async move {
                    let mut tick = tokio::time::interval(std::time::Duration::from_secs(300));
                    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                    loop {
                        tick.tick().await;
                        due_sink.process_due_recruitment_blacklists().await;
                    }
                });
            }

            // Periodischer Voll-Refresh aller Partner-Raid-Scores (Python
            // maybe_schedule_partner_raid_score_reconciliation, Intervall
            // 300 s): fängt Partner, deren Online/Offline-Events verpasst
            // wurden — sonst veralten deren Scores dauerhaft.
            {
                let refresh_pool = pool.clone();
                tokio::spawn(async move {
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

            let arrival =
                RaidArrivalCoordinator::new(pool.clone(), pending, RaidArrivalRuntime::new(sink));
            let blacklist_guard = BlacklistRaidGuard::new(
                RaidBlacklistStore::new(pool.clone()),
                token_provider,
                helix_client,
            );
            manual_raid_port = Some(Arc::new(ManualRaidAdapter {
                handler: offline.clone(),
            }));
            // Go-Live-ReAuth-Reminder (B11): braucht den nativen Chat-Send-Pfad.
            // chat_api_handle ist hier noch in Scope (wird erst unten von der
            // Pipeline konsumiert) — gleiches Durchreich-Muster wie partner_setup.
            let reauth_reminder = chat_api_handle
                .as_ref()
                .map(|h| Arc::new(ReauthReminder::new(pool.clone(), h.api.clone())));
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
            // !clip: Broadcaster-Token-Clip-Port, nur mit Helix + Krypto-Key.
            let clip_port = chat_wiring::build_clip_port(
                helix.as_ref().clone().map(Arc::new),
                FieldCipher::from_env().ok().map(Arc::new),
                pool.clone(),
            );
            let runtime = chat_wiring::build_runtime(
                handle,
                pool.clone(),
                manual_raid_port.clone(),
                clip_port,
                eventsub_hooks.clone(),
            )
            .await;
            runtime.start_background(subscription_manager.clone());
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
            let receiver_port: u16 = std::env::var("TB_EVENTSUB_RECEIVER_PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(8786);
            let receiver =
                Arc::new(tb_monitoring::WebhookReceiver::new(secret, dispatcher.clone()));
            let router = receiver.router();
            tokio::spawn(async move {
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
        let backfill_pool = pool.clone();
        tokio::spawn(async move {
            tb_analytics::post_stream::backfill_post_stream_reports(&backfill_pool, 3).await;
        });
        tokio::spawn(tb_analytics::post_stream::schedule_report_retry_job(
            pool.clone(),
            1800,
        ));
        tokio::spawn(tb_chat::title_jobs::schedule_nightly_knowledge_job(
            pool.clone(),
            300,
        ));
        tokio::spawn(tb_chat::title_jobs::schedule_weekly_insight_job(
            pool.clone(),
            600,
        ));
    }

    // Highlight-Clipper (Port von bot/highlight_clipper): pollt aktive Partner
    // auf neue Deadlock-Matches, schneidet Highlight-Clips aus dem Twitch-VOD und
    // postet sie über den lokalen Relay. In Python via `_hc_start` bedingungslos
    // AN; hier an die Helix-Verfügbarkeit gebunden (ohne Helix kein VOD-Lookup).
    // boon- und yt-dlp-Pfade relativ zum Service-WorkingDirectory (Repo-Root).
    if let Some(helix_client) = helix.as_ref().clone() {
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let hc_config = tb_highlight::worker::HighlightClipperConfig::new(
            cwd.join("tools/boon"),
            cwd.join(".venv/bin/yt-dlp"),
        );
        let hc_worker = tb_highlight::worker::HighlightClipperWorker::new(
            pool.clone(),
            Arc::new(HelixVodSource { helix: helix_client }),
            hc_config,
        );
        tokio::spawn(async move {
            loop {
                hc_worker.run_once().await;
                tokio::time::sleep(std::time::Duration::from_secs(
                    tb_highlight::config::POLL_INTERVAL_SECONDS,
                ))
                .await;
            }
        });
    } else {
        tracing::warn!("HighlightClipper: kein HelixClient — Worker nicht gestartet");
    }

    // Social-Media-Posting-Pipeline (Port von bot/social_media): sechs
    // Hintergrund-Worker. In Python (bootstrap) bedingungslos instanziiert
    // (`if services.api`) → hier ebenso bedingungslos gespawnt; jeder Worker hat
    // ein eigenes Intervall + Initial-Delay und ist still, solange keine Arbeit
    // ansteht (keine onboardeten Plattformen / keine pending Clips). An/Aus wird
    // datengetrieben über `social_media_settings` gesteuert (Consent +
    // Auto-Approve je Plattform) — identisch zu Python.
    {
        // Cipher-freie Worker: Retention-Cleanup, Approval-Queue, Report-Dispatcher.
        let retention = tb_social_media::retention_worker::RetentionWorker::new(pool.clone());
        tokio::spawn(async move { retention.run().await });
        let approval = tb_social_media::approval_worker::ApprovalWorker::new(pool.clone());
        tokio::spawn(async move { approval.run().await });
        let reports = tb_social_media::report_dispatcher::ReportDispatcher::new(pool.clone());
        tokio::spawn(async move { reports.run().await });

        // Enrichment: LLM-Dispatcher (Consent aus Settings) + optionaler
        // OpenAI-Whisper-Transcriber. Ohne OPENAI_API_KEY bleibt der Transcriber
        // None → Transkription wird übersprungen (1:1 wie ein fehlender Key).
        let llm: Arc<dyn tb_social_media::enrich_pipeline::EnrichmentLlm> =
            Arc::new(tb_social_media::llm_dispatch::LlmDispatcher::new(pool.clone()));
        let mut enrichment =
            tb_social_media::enrichment_worker::EnrichmentWorker::new(pool.clone(), llm);
        match tb_social_media::whisper::OpenAiTranscriber::from_env() {
            Some(transcriber) => {
                enrichment = enrichment.with_transcriber(Arc::new(transcriber));
                tracing::info!("Social-Media-Enrichment: OpenAI-Whisper-Transcriber aktiv");
            }
            None => {
                tracing::info!(
                    "Social-Media-Enrichment: kein OPENAI_API_KEY — Transkription übersprungen"
                );
            }
        }
        tokio::spawn(async move { enrichment.run().await });

        // Upload + Insights brauchen den Field-Cipher (verschlüsselte
        // Plattform-Tokens). Fehlt DB_MASTER_KEY_V1, laufen nur die cipher-freien
        // Worker — die Token-abhängigen bleiben aus statt zu paniken.
        match tb_crypto::FieldCipher::from_env() {
            Ok(cipher) => {
                let cipher = Arc::new(cipher);
                let cwd =
                    std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
                let upload_creds = tb_social_media::credentials::CredentialManager::new(
                    pool.clone(),
                    cipher.clone(),
                );
                // yt-dlp wie beim Highlight-Clipper aus dem venv (systemd-PATH
                // enthält ~/.local/bin nicht); clips_dir = Python-Default data/clips.
                let upload = tb_social_media::upload_worker::UploadWorker::new(
                    pool.clone(),
                    upload_creds,
                )
                .with_yt_dlp(cwd.join(".venv/bin/yt-dlp").to_string_lossy().into_owned());
                tokio::spawn(async move { upload.run().await });

                let insights_creds =
                    tb_social_media::credentials::CredentialManager::new(pool.clone(), cipher);
                let insights = tb_social_media::insights_worker::InsightsWorker::new(
                    pool.clone(),
                    insights_creds,
                );
                tokio::spawn(async move { insights.run().await });
            }
            Err(e) => {
                tracing::warn!(
                    "Social-Media Upload/Insights: kein Field-Cipher ({e}) — Worker aus"
                );
            }
        }
        tracing::info!("Social-Media-Pipeline-Worker gestartet (6 Loops)");
    }

    // Poll-Loop: das Cutover-Gate. Default AUS — Python bleibt alleiniger
    // Live-Writer, bis der Flip (04-cutover-plan) explizit erfolgt.
    let poll_enabled = std::env::var("TB_MONITORING_POLL_ENABLED")
        .map(|v| v.trim() == "1")
        .unwrap_or(false);
    let _poll_stop = if poll_enabled {
        match helix.as_ref().clone() {
            Some(helix_client) => {
                let notify_channel_id: i64 = std::env::var("TWITCH_NOTIFY_CHANNEL_ID")
                    .ok()
                    .and_then(|v| v.trim().parse().ok())
                    .unwrap_or(0);
                let sink: Arc<dyn AnnouncementSink> = if notify_channel_id > 0 {
                    match BrokerRelay::new(&settings.broker) {
                        Ok(relay) => {
                            let vod: Arc<dyn VodPreviewSource> = Arc::new(HelixVodPreview {
                                helix: helix_client.clone(),
                            });
                            // Ziel-Guild der Live-Ping-Rolle: Env-Override
                            // (STREAMER_GUILD_ID → MAIN_GUILD_ID) oder Default auf
                            // die Haupt-Community-Guild — identisch zu streamer_link.rs,
                            // wo Discord-Rollen-Operationen bereits auf diese Guild
                            // defaulten. Ohne Default wäre die Auto-Anlage still aus,
                            // sobald die Env-Var fehlt; der Notify-Channel liegt ohnehin
                            // in dieser Guild, also wird die Rolle dort angelegt.
                            let live_ping_guild_id = std::env::var("STREAMER_GUILD_ID")
                                .ok()
                                .or_else(|| std::env::var("MAIN_GUILD_ID").ok())
                                .and_then(|v| v.trim().parse::<u64>().ok())
                                .filter(|&g| g > 0)
                                .unwrap_or(1_289_721_245_281_292_288);
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
                                AnnounceConfigStore::new(pool.clone()),
                                vod,
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
                tokio::spawn(engine.run(stop_rx));
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

    // Clip-Fetch-Task: füttert die Social-Media-Pipeline mit neuen Twitch-Clips
    // aktiver Partner (alle 6h). In Python (ClipFetcher.__init__) bedingungslos
    // AN; hier — wie der Highlight-Clipper — an die Helix-Verfügbarkeit gebunden
    // (ohne Helix keine Clip-Reads). Auto-Uploads bleiben datengetrieben über
    // social_media_settings (Consent + Auto-Approve) gegated, 1:1 zu Python.
    if let Some(ref h) = *helix {
        tb_social_media::build_clip_fetch_task(pool.clone(), std::sync::Arc::new(h.clone())).start();
    } else {
        tracing::warn!("clip_fetch: kein HelixClient — Clip-Fetcher nicht gestartet");
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
        .start_if_enabled();
    }

    // Token-Lifecycle-Reaktionen (Block 4): Admin-Embed + User-DM bei
    // Token-Fehler (1×/Streamer), 7-Tage-Grace-Sweep mit Rollen-Entzug
    // (stündlich) und Blacklist-Cleanup >30 Tage (3,5 h) — alles über den
    // F4-Master-Broker, da der Twitch-Bot keinen Discord-Zugang hat.
    token_lifecycle_wiring::spawn_token_lifecycle_schedulers(pool.clone(), &settings.broker);

    // Streamer-Link-Matcher: verknüpft neue Twitch-Partner mit ihrem Discord-Account.
    // Läuft alle 6h, ist still wenn keine neuen Kandidaten vorhanden.
    if let Ok(sl_relay) = BrokerRelay::new(&settings.broker) {
        let sl_config = Arc::new(streamer_link::StreamerLinkConfig::from_env());
        let sl_pool = pool.clone();
        let sl_base = format!("http://127.0.0.1:{port}");
        let sl_token = settings.internal_api.token.clone();
        tokio::spawn(streamer_link::streamer_link_task(
            sl_pool,
            sl_relay,
            sl_config,
            sl_base,
            sl_token,
        ));
    }

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
        legacy_proxy,
    );

    tracing::info!("tb-bot lauscht auf {addr}");
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap_or_else(|e| {
            tracing::error!("Bind-Fehler auf {addr}: {e}");
            std::process::exit(1);
        });

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .unwrap();
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
        let rows: Vec<(String, String, i32)> = match sqlx::query_as(
            "SELECT LOWER(twitch_login), twitch_user_id, 1 AS is_partner \
             FROM twitch_streamers_partner_state \
             WHERE is_partner_active = 1 AND COALESCE(twitch_user_id, '') <> '' \
             UNION \
             SELECT LOWER(twitch_login), twitch_user_id, 0 AS is_partner \
             FROM twitch_streamers \
             WHERE COALESCE(is_monitored_only, 0) = 1 AND COALESCE(twitch_user_id, '') <> ''",
        )
        .fetch_all(&pool)
        .await
        {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("sub-maintenance: Partner-Query fehlgeschlagen: {e}");
                continue;
            }
        };

        let active_ids: std::collections::HashSet<String> =
            rows.iter().map(|(_, uid, _)| uid.clone()).collect();

        let deleted = manager.cleanup_stale(&active_ids).await;
        tracing::debug!(deleted, "sub-maintenance: Stale-Cleanup abgeschlossen");

        let mut ensured = 0usize;
        let mut telemetry_ensured = 0usize;
        for (login, uid, is_partner) in &rows {
            manager.ensure_core_subscriptions(uid, login).await;
            ensured += 1;

            // channel.raid (Arrival) proaktiv pro Partner abonnieren — Python
            // (eventsub_mixin.py:2666) subscribt channel.raid für ALLE Streamer,
            // damit eingehende/manuelle Raids erkannt werden, unabhängig von
            // eigenen Outgoing-Raids. Partner-only: die Raid-Dankesnachricht ist
            // partner-gebunden, monitored-only Kanäle haben keinen Konsumenten.
            if *is_partner == 1 {
                manager.ensure_raid_subscription(uid, login).await;
            }

            // Broadcaster-Telemetrie-Subs (B9): nur mit gültigem Broadcaster-
            // Token + dessen Scopes. needs_reauth/blacklist → still überspringen.
            if let Some((token_provider, raid_auth)) = &telemetry_auth {
                match token_provider
                    .get_valid_token_unrestricted(uid, chrono::Utc::now())
                    .await
                {
                    Ok(Some(token)) => {
                        let scopes = raid_auth.get_scopes(uid).await.unwrap_or_default();
                        telemetry_ensured += manager
                            .ensure_broadcaster_telemetry_subscriptions(uid, login, &token, &scopes)
                            .await;
                    }
                    Ok(None) => {}
                    Err(e) => {
                        tracing::debug!(uid = %uid, "sub-maintenance: Telemetrie-Token-Lookup fehlgeschlagen: {e}");
                    }
                }
            }
        }
        tracing::info!(
            kanäle = rows.len(),
            ensured,
            telemetry_ensured,
            deleted,
            "sub-maintenance: Core-Sub-Reconcile abgeschlossen"
        );
    }
}
