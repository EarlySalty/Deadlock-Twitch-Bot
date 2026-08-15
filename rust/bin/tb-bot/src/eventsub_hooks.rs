//! Echte EventSub-Hook-Implementierung: verdrahtet Monitoring-Events mit dem
//! Raid-Subsystem. Ersetzt den Interim-`SubscriptionEventSubHooks` (nur
//! Go-Live) — alle vier Raid-Kopplungen aus `04-cutover-plan.md` sind hier echt:
//!
//! - `on_stream_went_live`  → stream.offline-Subscription (wie bisher)
//! - `on_score_refresh`     → Partner-Score-Refresh (ScoreRefreshResolver)
//! - `on_stream_offline`    → Auto-Raid ([`OfflineRaidHandler`])
//! - `on_channel_raid`      → Arrival-Korrelation ([`RaidArrivalCoordinator`])
//! - `on_channel_moderate`  → Blacklist-Raid-Guard ([`BlacklistRaidGuard`])
//!
//! Abweichung von Python: Score-Refreshes laufen inline statt als
//! debounced Background-Task — ein Einzel-Partner-Refresh ist nur eine
//! Handvoll DB-Reads, und der Dispatcher verarbeitet Events sequenziell
//! pro message_id-Guard.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use chrono::Utc;
use serde_json::Value;
use sqlx::PgPool;
use tb_highlight::{
    twitch_vod::TwitchVodApi,
    vod_export::{
        export_latest_vod, export_log_description, export_log_title, format_bytes, format_duration,
        should_export, CommandRunner, ExportTargets, TokioCommandRunner, VodExportError,
        VodExportReport, TARGET_LOGIN,
    },
};
use tb_monitoring::{
    EventSubHooks, LiveStateStore, ModeratorProvisionOutcome, ModeratorProvisioner,
    SubscriptionManager,
};
use tb_raid::pending_raids::normalize_broadcaster_login;
use tb_raid::signal_correlation::{ChatNotificationInput, ChatUnraidInput};
use tb_raid::{
    classify_partner_raid_arrival, ArrivalTrackingStore, BotBanStatus, BotBanStatusProbe,
    ManualRaidSuppression, PendingRaidStore, RaidArrivalInput, RaidArrivalRuntime,
    RaidBlacklistStore, RaidGreetingRegistration, RaidSignalCorrelationService, RaidSignalOutcome,
    TokenProvider,
};
use tb_transport_discord::{BrokerRelay, DiscordBackend, SendAlertEmbed, SendUserDm};
use tb_transport_twitch::{AddModeratorOutcome, HelixClient, RemoveModeratorOutcome};

use crate::auto_raid::OfflineRaidHandler;
use crate::offline_side_effects::OfflineSideEffects;
use crate::partner_lookup::{
    is_target_partner, known_source, resolve_active_partner_id_by_login, PrefetchedLookups,
};
use crate::raid_greeting::OutgoingRaidSink;
use crate::reauth_reminder::ReauthReminder;
use crate::score_refresh::ScoreRefreshResolver;

const VOD_EXPORT_DISCORD_USER_ID: u64 = 279_971_744_964_542_464;
/// Admin-/Log-Channel, identisch mit `TOKEN_ERROR_CHANNEL_ID` — jeder
/// Export-Lauf meldet sich dort, Erfolg wie Abbruch.
const VOD_EXPORT_LOG_CHANNEL_ID: i64 = 1_374_364_800_817_303_632;
/// 📹-caster-chat: hier bekommen die Caster den fertigen VOD-Link.
const VOD_EXPORT_CASTER_CHANNEL_ID: i64 = 1_474_543_558_793_887_937;
const VOD_EXPORT_DELAY: Duration = Duration::from_secs(180);

pub struct VodExportOfflineHandler {
    api: Arc<dyn TwitchVodApi>,
    runner: Arc<dyn CommandRunner>,
    relay: BrokerRelay,
    yt_dlp_path: PathBuf,
    remote_base: String,
    temp_dir: PathBuf,
}

impl VodExportOfflineHandler {
    pub fn new(
        api: Arc<dyn TwitchVodApi>,
        relay: BrokerRelay,
        yt_dlp_path: PathBuf,
        remote_base: String,
    ) -> Self {
        Self {
            api,
            runner: Arc::new(TokioCommandRunner),
            relay,
            yt_dlp_path,
            remote_base,
            temp_dir: std::env::temp_dir().join("tb-vod-export"),
        }
    }

    pub fn spawn_for_offline(&self, twitch_user_id: &str, login: Option<&str>) {
        if !should_export(login) {
            return;
        }

        let api = Arc::clone(&self.api);
        let runner = Arc::clone(&self.runner);
        let relay = self.relay.clone();
        let yt_dlp_path = self.yt_dlp_path.clone();
        let remote_base = self.remote_base.clone();
        let temp_dir = self.temp_dir.clone();
        let twitch_user_id = twitch_user_id.to_string();
        let stream_offline_unix = Utc::now().timestamp();
        tokio::spawn(async move {
            tokio::time::sleep(VOD_EXPORT_DELAY).await;
            let targets = ExportTargets {
                yt_dlp_path: &yt_dlp_path,
                rclone_path: Path::new("rclone"),
                remote_base: &remote_base,
                temp_dir: &temp_dir,
            };
            let started = Instant::now();
            let result = export_latest_vod(
                api.as_ref(),
                runner.as_ref(),
                &targets,
                &twitch_user_id,
                stream_offline_unix,
            )
            .await;

            // DM nur im Erfolgsfall; ihr Zustellstatus wandert mit in den
            // Log-Channel, sonst bliebe ein fehlgeschlagener Versand unsichtbar.
            let mut dm_delivered = false;
            match &result {
                Ok(report) => {
                    let payload = SendUserDm {
                        user_id: VOD_EXPORT_DISCORD_USER_ID,
                        content: vod_export_dm_content(&report.link),
                    };
                    match relay.send_user_dm(payload).await {
                        Ok(_) => dm_delivered = true,
                        Err(error) => {
                            tracing::error!(%error, "VOD-Export: Discord-DM fehlgeschlagen")
                        }
                    }
                    tracing::info!(
                        twitch_user_id,
                        vod_id = %report.vod_id,
                        size_bytes = report.size_bytes,
                        duration_seconds = report.duration_seconds,
                        elapsed_seconds = started.elapsed().as_secs(),
                        dm_delivered,
                        "VOD-Export abgeschlossen"
                    );
                }
                Err(VodExportError::NoVod) => {
                    tracing::warn!(
                        twitch_user_id,
                        "VOD-Export: nach Wartezeit kein Archiv-VOD gefunden"
                    );
                }
                Err(VodExportError::NoNewVod) => {
                    tracing::warn!(
                        twitch_user_id,
                        "VOD-Export: nach Wartezeit weiterhin nur das vorherige VOD sichtbar, kein Export"
                    );
                }
                Err(error) => {
                    tracing::error!(%error, twitch_user_id, "VOD-Export fehlgeschlagen");
                }
            }

            let elapsed_seconds = started.elapsed().as_secs() as i64;
            if let Err(error) = relay
                .send_alert_embed(SendAlertEmbed {
                    channel_id: VOD_EXPORT_LOG_CHANNEL_ID,
                    content: None,
                    embed: vod_export_log_embed(&result, elapsed_seconds, dm_delivered),
                    allowed_role_ids: Vec::new(),
                })
                .await
            {
                tracing::error!(%error, "VOD-Export: Discord-Log fehlgeschlagen");
            }

            // Der Caster-Chat bekommt nur den fertigen Export: Link plus
            // Kennzahlen. Abbrüche bleiben im Admin-Log — ihr Grundtext trägt
            // rohes yt-dlp/rclone-stderr samt lokaler Pfade, und `NoVod` nach
            // einem Restream ist gar kein Fehler, sondern Normalbetrieb.
            if let Ok(report) = &result {
                if let Err(error) = relay
                    .send_alert_embed(SendAlertEmbed {
                        channel_id: VOD_EXPORT_CASTER_CHANNEL_ID,
                        content: Some(vod_export_channel_content(&report.link)),
                        embed: vod_export_caster_embed(report),
                        allowed_role_ids: Vec::new(),
                    })
                    .await
                {
                    tracing::error!(%error, "VOD-Export: Caster-Chat-Post fehlgeschlagen");
                }
            }
        });
    }
}

/// Log-Embed fuer den Admin-Channel — gruen bei Erfolg, rot bei Abbruch.
fn vod_export_log_embed(
    result: &Result<VodExportReport, VodExportError>,
    elapsed_seconds: i64,
    dm_delivered: bool,
) -> Value {
    let success = result.is_ok();
    serde_json::json!({
        "title": export_log_title(success),
        "description": export_log_description(result, elapsed_seconds, dm_delivered),
        "color": if success { 0x2E_CC71 } else { 0xE7_4C3C },
    })
}

/// Text für den Caster-Chat. Kennzahlen stehen im Embed daneben, hier zählt nur
/// der klickbare Link.
fn vod_export_channel_content(link: &str) -> String {
    format!("VOD vom letzten {TARGET_LOGIN}-Stream: {link}")
}

/// Embed für den Caster-Chat: nur Kennzahlen des fertigen Exports. Bewusst nicht
/// `vod_export_log_embed` — dessen Fehlertext trägt rohes Prozess-stderr und
/// interne Pfade, die in einen Team-Channel nichts verloren haben.
fn vod_export_caster_embed(report: &VodExportReport) -> Value {
    serde_json::json!({
        "title": export_log_title(true),
        "description": format!(
            "Kanal: {TARGET_LOGIN}\nVOD: {vod_id}\nStreamlaenge: {dauer}\nGroesse: {groesse}",
            vod_id = report.vod_id,
            dauer = format_duration(report.duration_seconds),
            groesse = format_bytes(report.size_bytes),
        ),
        "color": 0x2E_CC71,
    })
}

fn vod_export_dm_content(link: &str) -> String {
    format!("VOD von deinem letzten dach_lock-Stream liegt im Drive: {link}\nDer Link bleibt gültig, solange die Datei dort liegt.")
}

fn event_str<'a>(event: &'a Value, key: &str) -> &'a str {
    event.get(key).and_then(Value::as_str).unwrap_or("").trim()
}

// ─── channel.raid → Arrival-Korrelation ─────────────────────────────────────

/// Orchestriert ein `channel.raid`-Event: Pending-Lookup → Plan
/// (Signal-Korrelation) → Plan-Ausführung gegen den Sink. Port des
/// channel.raid-Pfads aus `raid_arrival_runtime.py` (Z. 420–490).
///
/// Abweichung von Python: die Unabhängig-Erkennung ist hier eine reine
/// Klassifikation — die Schreib-Effekte (Arrival-Zeile, Suppression-Mark)
/// laufen ausschließlich über die Plan-Actions. Python schrieb beides
/// doppelt (Pre-Check UND Action führten `process_independent_…` aus).
pub struct RaidArrivalCoordinator {
    pool: PgPool,
    pending: Arc<Mutex<PendingRaidStore>>,
    runtime: RaidArrivalRuntime,
    /// P2.43: Recent-Arrival-Lookup für das Sekundär-Signal-Pre-Gate.
    arrival_store: ArrivalTrackingStore,
}

/// Recent-Raid-Arrival-TTL (Python `recent_raid_arrival_ttl_seconds = 600`,
/// raid_state_store.py:16). Identisch zu `raid_arrival_wiring.rs`.
const RECENT_ARRIVAL_TTL_SECS: i64 = 600;

impl RaidArrivalCoordinator {
    pub fn new(
        pool: PgPool,
        pending: Arc<Mutex<PendingRaidStore>>,
        runtime: RaidArrivalRuntime,
    ) -> Self {
        let arrival_store = ArrivalTrackingStore::new(pool.clone());
        Self {
            pool,
            pending,
            runtime,
            arrival_store,
        }
    }

    /// P2.43-Pre-Gate: existiert für (Ziel, Quelle) ein bestätigter Arrival
    /// innerhalb des Recent-Fensters (TTL 600 s), wird das zweite/späte Signal
    /// als Sekundär-Bestätigung dedupliziert (Plan-Pfad
    /// `secondary_signal_handled` → `RecordSecondarySignal`), statt erneut als
    /// Orphan/Mismatch/eigenständig klassifiziert zu werden. Port von
    /// `_handle_secondary_confirmed_signal` (raid_arrival_runtime.py:102-166),
    /// das jeder Handler ZUERST aufruft. Fehler beim Lookup → `false`
    /// (fail-open auf den normalen Korrelationspfad).
    async fn recent_arrival_present(
        &self,
        to_broadcaster_id: &str,
        from_broadcaster_login: &str,
    ) -> bool {
        if to_broadcaster_id.is_empty() || from_broadcaster_login.is_empty() {
            return false;
        }
        match self
            .arrival_store
            .find_recent_arrival(
                to_broadcaster_id,
                from_broadcaster_login,
                RECENT_ARRIVAL_TTL_SECS,
            )
            .await
        {
            Ok(found) => found.is_some(),
            Err(error) => {
                tracing::error!(%error, "Recent-Arrival-Pre-Gate-Lookup fehlgeschlagen");
                false
            }
        }
    }

    pub async fn handle_channel_raid(&self, event: &Value) {
        let from_login = event_str(event, "from_broadcaster_user_login").to_lowercase();
        let from_id = Some(event_str(event, "from_broadcaster_user_id"))
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let to_id = event_str(event, "to_broadcaster_user_id").to_string();
        let to_login = event_str(event, "to_broadcaster_user_login").to_lowercase();
        let viewer_count = event.get("viewers").and_then(Value::as_i64).unwrap_or(0) as i32;

        if from_login.is_empty() {
            tracing::warn!("channel.raid-Event ohne from_broadcaster_user_login");
            return;
        }
        if to_id.is_empty() {
            tracing::warn!(from = %from_login, "channel.raid-Event ohne to_broadcaster_user_id");
            return;
        }
        tracing::info!(from = %from_login, to = %to_login, viewer_count, "EventSub: channel.raid");

        let pending = self
            .pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&to_id, Some(&from_login))
            .cloned();

        // P2.43: Sekundär-Signal-Pre-Gate ZUERST (vor Unabhängig-Erkennung) —
        // ein zweites Signal für denselben Raid innerhalb des Recent-Fensters
        // wird als Bestätigung dedupliziert statt neu klassifiziert.
        let recent_arrival_present = self.recent_arrival_present(&to_id, &from_login).await;

        // Unabhängig-Erkennung nur ohne Pending (Python Z. 445–457):
        // Ziel-Partner-Status + Quell-Auflösung vorab laden, dann pure
        // Klassifikation — Some(_) = manueller/externer Raid auf einen Partner.
        // Bei Recent-Arrival short-circuitet der Plan ohnehin auf das
        // Sekundär-Signal; die teure Unabhängig-/Manual-Key-Auflösung dann sparen.
        let independent_manual_detected = if pending.is_none() && !recent_arrival_present {
            let lookups = PrefetchedLookups {
                target_is_partner: is_target_partner(&self.pool, &to_id, &to_login).await,
                known_source: known_source(&self.pool, from_id.as_deref(), &from_login).await,
            };
            classify_partner_raid_arrival(
                Some(&from_login),
                from_id.as_deref(),
                Some(&to_id),
                Some(&to_login),
                &lookups,
                &lookups,
            )
            .classification
            .is_some()
        } else {
            false
        };

        // Manual-Raid-Key: from-ID, sonst Auflösung über den Partner-Login.
        let manual_raid_source_key = if recent_arrival_present {
            None
        } else {
            match &from_id {
                Some(id) => Some(id.clone()),
                None => resolve_active_partner_id_by_login(&self.pool, &from_login).await,
            }
        };

        let plan = RaidSignalCorrelationService.plan_raid_arrival(RaidArrivalInput {
            to_broadcaster_id: to_id,
            to_broadcaster_login: to_login,
            from_broadcaster_login: from_login.clone(),
            from_broadcaster_id: from_id,
            viewer_count,
            pending_raid: pending,
            recent_arrival_present,
            independent_manual_detected,
            manual_raid_source_key,
        });

        let outcome = plan.outcome.clone();
        self.runtime.execute_plan(&plan).await;

        if outcome == RaidSignalOutcome::PendingMismatch {
            tracing::warn!(
                expected = plan
                    .pending_raid
                    .as_ref()
                    .map(|p| p.from_broadcaster_login.as_str())
                    .unwrap_or("?"),
                actual = %from_login,
                "Raid-Arrival-Mismatch: Quelle passt nicht zum Pending"
            );
        }
    }

    /// Ziel-seitige `channel.chat.notification`-Raidmeldung (notice_type=raid,
    /// B7-01). Sekundärpfad zum `channel.raid`-Webhook: `broadcaster_*` ist das
    /// Ziel, `raid.user_*` die Quelle. Pending nachschlagen → Plan über
    /// `plan_chat_notification` → ausführen (Orphan/Mismatch/Match werden in den
    /// Plan-Actions abgehandelt). Port von `on_chat_raid_notification`
    /// (raid_arrival_runtime.py Z. 534–614).
    pub async fn handle_chat_raid_notification(&self, event: &Value, message_id: Option<&str>) {
        let to_id = event_str(event, "broadcaster_user_id").to_string();
        let to_login = event_str(event, "broadcaster_user_login").to_lowercase();
        // Quelle (Raider) sitzt im `raid`-Sub-Objekt (Twitch chat.notification).
        let raid = event.get("raid");
        let from_login = raid
            .map(|r| event_str(r, "user_login"))
            .unwrap_or("")
            .to_lowercase();
        let from_id = raid
            .map(|r| event_str(r, "user_id"))
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let viewer_count = raid
            .and_then(|r| r.get("viewer_count"))
            .and_then(Value::as_i64)
            .unwrap_or(0) as i32;

        if to_id.is_empty() || from_login.is_empty() {
            tracing::debug!(
                to = %to_login,
                from = %from_login,
                "chat.notification-Raid ohne Ziel-ID/Quell-Login ignoriert"
            );
            return;
        }
        tracing::info!(from = %from_login, to = %to_login, viewer_count, "EventSub: chat.notification raid");

        let pending = self
            .pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&to_id, Some(&from_login))
            .cloned();

        // P2.43: Sekundär-Signal-Pre-Gate — chat.notification ist der häufigste
        // Sekundärpfad zum channel.raid-Webhook für denselben Raid.
        let recent_arrival_present = self.recent_arrival_present(&to_id, &from_login).await;

        let plan = RaidSignalCorrelationService.plan_chat_notification(ChatNotificationInput {
            to_broadcaster_id: to_id,
            to_broadcaster_login: to_login,
            from_broadcaster_login: from_login.clone(),
            from_broadcaster_id: from_id,
            viewer_count,
            message_id: message_id.map(str::to_string),
            event_timestamp: None,
            pending_raid: pending,
            recent_arrival_present,
        });

        let outcome = plan.outcome.clone();
        self.runtime.execute_plan(&plan).await;

        if outcome == RaidSignalOutcome::PendingMismatch {
            tracing::warn!(
                expected = plan
                    .pending_raid
                    .as_ref()
                    .map(|p| p.from_broadcaster_login.as_str())
                    .unwrap_or("?"),
                actual = %from_login,
                "chat.notification-Raid-Mismatch: Quelle passt nicht zum Pending"
            );
        }
    }

    /// `channel.chat.notification`-Unraidmeldung (notice_type=unraid, B7-02/03).
    /// Verzweigt wie Pythons Chat-Bot-Routing (bot.py Z. 1883–1926):
    ///
    /// - **Source-Self-Unraid** (Unraider == Kanal-Inhaber): der Quell-Streamer
    ///   bricht seine eigene Raid-Sequenz ab → ausstehende Auto-Raids dieses
    ///   Quell-Streamers stornieren (B7-03, [`Self::cancel_pending_for_source`]).
    /// - **Ziel-seitiges Unraid** (sonst): diagnostisch über `plan_chat_unraid`
    ///   am vorhandenen Pending vermerken (kein Mismatch-/Confirm-Pfad).
    pub async fn handle_chat_unraid_notification(&self, event: &Value, message_id: Option<&str>) {
        let broadcaster_id = event_str(event, "broadcaster_user_id").to_string();
        let broadcaster_login = event_str(event, "broadcaster_user_login").to_lowercase();
        // Unraider = chatter (chat.notification). Fällt der auf den Kanal-Inhaber,
        // ist es ein Source-Self-Unraid.
        let from_login = event_str(event, "chatter_user_login").to_lowercase();
        let from_id = Some(event_str(event, "chatter_user_id"))
            .filter(|s| !s.is_empty())
            .map(str::to_string);

        if broadcaster_id.is_empty() || broadcaster_login.is_empty() {
            return;
        }

        // Source-Self-Unraid: Unraider-Login == Kanal-Inhaber UND (ID unbekannt
        // oder gleich). Python bot.py Z. 1898–1918.
        let is_source_self = !from_login.is_empty()
            && from_login == broadcaster_login
            && from_id
                .as_deref()
                .map(|id| id == broadcaster_id)
                .unwrap_or(true);
        if is_source_self {
            let canceled = self.cancel_pending_for_source(&broadcaster_login, message_id);
            if canceled == 0 {
                tracing::info!(
                    source = %broadcaster_login,
                    message_id = message_id.unwrap_or("n/a"),
                    "Source-Self-Unraid ohne ausstehenden Auto-Raid"
                );
            }
            return;
        }

        // Ziel-seitiges Unraid: nur diagnostisch am Pending vermerken.
        let pending = self
            .pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&broadcaster_id, Some(&from_login))
            .cloned();
        // P2.43: Ein Unraid für einen bereits bestätigten Raid wird als
        // Sekundär-Signal mit `unraid_seen` an der Arrival-Zeile vermerkt
        // (Python `_handle_secondary_confirmed_signal`, unraid-Pfad).
        let recent_arrival_present = self
            .recent_arrival_present(&broadcaster_id, &from_login)
            .await;

        let plan = RaidSignalCorrelationService.plan_chat_unraid(ChatUnraidInput {
            to_broadcaster_id: broadcaster_id,
            to_broadcaster_login: broadcaster_login.clone(),
            from_broadcaster_login: from_login.clone(),
            from_broadcaster_id: from_id,
            pending_raid: pending,
            recent_arrival_present,
            event_timestamp: None,
        });
        self.runtime.execute_plan(&plan).await;
        tracing::info!(
            from = %from_login,
            to = %broadcaster_login,
            "chat.notification unraid ohne bestätigten Raid beobachtet"
        );
    }

    /// Storniert alle ausstehenden Auto-Raids eines Quell-Streamers (B7-03,
    /// Source-Self-Unraid) über die B7-`iter()`-basierte Store-API
    /// ([`PendingRaidStore::cancel_from_source`]) und loggt die Treffer. Port von
    /// `cancel_pending_raids_for_source_unraid` (raid_tracking_runtime.py Z. 160–220).
    fn cancel_pending_for_source(
        &self,
        from_broadcaster_login: &str,
        message_id: Option<&str>,
    ) -> usize {
        let normalized_from = normalize_broadcaster_login(from_broadcaster_login);
        if normalized_from.is_empty() {
            return 0;
        }
        let canceled = self
            .pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .cancel_from_source(&normalized_from);
        for raid in &canceled {
            tracing::info!(
                source = %normalized_from,
                target_id = %raid.to_broadcaster_id,
                message_id = message_id.unwrap_or("n/a"),
                "Ausstehender Auto-Raid durch Source-Unraid storniert"
            );
        }
        canceled.len()
    }
}

// ─── channel.moderate → ausgehende Raids beobachten ─────────────────────────

/// Auto-Raid-Sperre nach einem manuellen Raid (Python `mark_manual_raid_started`,
/// 180 s). Gleicher Wert wie im Arrival-Pfad.
const MANUAL_RAID_SUPPRESSION_SECS: f64 = 180.0;

/// Ziel eines ausgehenden Raids aus einem `channel.moderate`-Event
/// (`action = "raid"`). Das ist die einzige Quelle, die das echte Ziel auch
/// dann meldet, wenn es kein Partner ist — `channel.raid` und
/// `chat.notification` sieht der Bot nur bei Partner-Zielen.
fn parse_outgoing_raid(event: &Value) -> Option<(String, String)> {
    if !event_str(event, "action").eq_ignore_ascii_case("raid") {
        return None;
    }
    let raid = event.get("raid").filter(|value| value.is_object())?;
    let target_id = event_str(raid, "user_id").trim().to_string();
    let target_login = event_str(raid, "user_login").trim().to_lowercase();
    if target_id.is_empty() && target_login.is_empty() {
        return None;
    }
    Some((target_id, target_login))
}

/// Feld im `channel.moderate`-Payload, das den betroffenen Nutzer trägt — je
/// nach Action heißt das Unter-Objekt anders. Ohne Treffer bleibt das Ziel leer,
/// die Zeile wird trotzdem geschrieben.
const MODERATE_TARGET_KEYS: [&str; 12] = [
    "raid",
    "unraid",
    "ban",
    "unban",
    "timeout",
    "untimeout",
    "mod",
    "unmod",
    "vip",
    "unvip",
    "warn",
    "delete",
];

/// Betroffener Nutzer einer `channel.moderate`-Action, soweit im Payload.
fn moderate_target(event: &Value) -> Option<String> {
    MODERATE_TARGET_KEYS.iter().find_map(|key| {
        let target = event.get(*key)?;
        let login = event_str(target, "user_login").trim().to_lowercase();
        if login.is_empty() {
            None
        } else {
            Some(login)
        }
    })
}

/// Protokolliert jede Moderations-Action eines Kanals — auch die, für die es
/// keinen Handler gibt. Ohne diese Zeile ist im Nachhinein nicht feststellbar,
/// was in einem Kanal passiert ist; genau daran scheiterte die Aufklärung des
/// Doppel-Raids am 2026-07-31.
fn log_moderate_action(broadcaster_id: &str, login: &str, event: &Value) {
    let action = event_str(event, "action").trim().to_lowercase();
    if action.is_empty() {
        tracing::warn!(
            streamer = login,
            broadcaster_id,
            "channel.moderate ohne action-Feld"
        );
        return;
    }
    tracing::info!(
        streamer = login,
        broadcaster_id,
        action = %action,
        target = moderate_target(event).unwrap_or_default(),
        moderator = event_str(event, "moderator_user_login"),
        "channel.moderate"
    );
}

/// Lernt aus `channel.moderate` im Quellkanal, wohin ein Raid wirklich geht.
///
/// Zwei Wirkungen, beide unabhängig davon, ob das Ziel ein Partner ist:
/// 1. Auto-Raid-Sperre setzen, damit ein manuell gestarteter Raid nicht vom
///    Auto-Raid beim Offline-Gehen überschrieben wird.
/// 2. Eine offene Begrüßungs-Erinnerung auf das echte Ziel umziehen.
pub struct OutgoingRaidObserver {
    suppression: Arc<Mutex<ManualRaidSuppression>>,
    greeting: Option<Arc<dyn OutgoingRaidSink>>,
}

impl OutgoingRaidObserver {
    pub fn new(
        suppression: Arc<Mutex<ManualRaidSuppression>>,
        greeting: Option<Arc<dyn OutgoingRaidSink>>,
    ) -> Self {
        Self {
            suppression,
            greeting,
        }
    }

    pub fn handle(&self, broadcaster_id: &str, login: &str, event: &Value) {
        log_moderate_action(broadcaster_id, login, event);

        let Some((target_id, target_login)) = parse_outgoing_raid(event) else {
            return;
        };
        let broadcaster_id = broadcaster_id.trim();
        if broadcaster_id.is_empty() {
            return;
        }

        self.suppression
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .mark(broadcaster_id, MANUAL_RAID_SUPPRESSION_SECS, None);

        tracing::info!(
            streamer = login,
            target = %target_login,
            suppression_secs = MANUAL_RAID_SUPPRESSION_SECS,
            "Ausgehender Raid im Quellkanal erkannt"
        );

        if let Some(greeting) = &self.greeting {
            greeting.raid_retargeted(RaidGreetingRegistration {
                from_broadcaster_id: broadcaster_id.to_string(),
                from_broadcaster_login: login.trim().to_lowercase(),
                to_broadcaster_id: target_id,
                to_broadcaster_login: target_login,
            });
        }
    }
}

// ─── channel.moderate → Blacklist-Raid-Guard ────────────────────────────────

/// Bricht manuell gestartete Raids auf hart global gebannte Ziele ab. Port von
/// `eventsub_mixin.py` `_guard_blacklisted_outgoing_raid`.
pub struct BlacklistRaidGuard {
    blacklist: RaidBlacklistStore,
    token_provider: Arc<TokenProvider>,
    helix: HelixClient,
}

impl BlacklistRaidGuard {
    pub fn new(
        blacklist: RaidBlacklistStore,
        token_provider: Arc<TokenProvider>,
        helix: HelixClient,
    ) -> Self {
        Self {
            blacklist,
            token_provider,
            helix,
        }
    }

    pub async fn handle(&self, broadcaster_id: &str, login: &str, event: &Value) {
        if !event_str(event, "action").eq_ignore_ascii_case("raid") {
            return;
        }
        let Some(raid_info) = event.get("raid").filter(|v| v.is_object()) else {
            return;
        };
        let target_login = event_str(raid_info, "user_login").to_lowercase();
        let target_id = event_str(raid_info, "user_id").to_string();
        if target_login.is_empty() && target_id.is_empty() {
            return;
        }

        let hard_banned = match self
            .blacklist
            .is_hard_banned(Some(&target_id), &target_login)
            .await
        {
            Ok(hit) => hit,
            Err(error) => {
                tracing::error!(%error, target = %target_login, "Global-Ban-Prüfung fehlgeschlagen; fail-closed");
                true
            }
        };
        if !hard_banned {
            return;
        }

        tracing::warn!(
            streamer = login,
            target = %target_login,
            "Manueller Raid auf global gebanntes Ziel erkannt — versuche Abbruch"
        );

        let cancelled = self.cancel_raid(broadcaster_id).await;
        if cancelled {
            tracing::warn!(
                streamer = login,
                target = %target_login,
                "Raid auf global gebanntes Ziel abgebrochen"
            );
        } else {
            tracing::warn!(
                streamer = login,
                target = %target_login,
                "Raid-Abbruch nicht möglich — Raid auf global gebanntes Ziel lief durch"
            );
        }
    }

    async fn cancel_raid(&self, broadcaster_id: &str) -> bool {
        let token = match self
            .token_provider
            .get_valid_token(broadcaster_id, Utc::now())
            .await
        {
            Ok(Some(token)) => token,
            Ok(None) => {
                tracing::warn!(broadcaster_id, "Kein gültiger Token für Raid-Abbruch");
                return false;
            }
            Err(error) => {
                tracing::error!(%error, broadcaster_id, "Token-Lookup für Raid-Abbruch fehlgeschlagen");
                return false;
            }
        };
        match self.helix.cancel_raid(broadcaster_id, &token).await {
            Ok(Ok(())) => true,
            Ok(Err(api_error)) => {
                tracing::warn!(broadcaster_id, %api_error, "Cancel-Raid abgelehnt");
                false
            }
            Err(error) => {
                tracing::warn!(broadcaster_id, %error, "Cancel-Raid-Request fehlgeschlagen");
                false
            }
        }
    }
}

// ─── 403-Selbstheilung: Bot als Moderator nachsetzen (P1.2) ──────────────────

/// Setzt den Bot zur Laufzeit (wieder) als Moderator eines Kanals ein, wenn ein
/// Chat-Join/Sub-Create mit 403 fehlschlägt. Port von Pythons
/// `_ensure_bot_is_mod` (chat/connection.py:961): Streamer-Token auflösen
/// (`get_tokens_for_user` → [`TokenProvider::get_valid_token_unrestricted`]),
/// dann `POST /moderation/moderators` mit Bot-User-ID. Ohne gültigen
/// Streamer-Token oder Bot-ID heilt der Join nicht (`false`).
pub struct HelixModeratorProvisioner {
    token_provider: Arc<TokenProvider>,
    helix: HelixClient,
    bot_user_id: String,
}

impl HelixModeratorProvisioner {
    pub fn new(
        token_provider: Arc<TokenProvider>,
        helix: HelixClient,
        bot_user_id: String,
    ) -> Self {
        Self {
            token_provider,
            helix,
            bot_user_id,
        }
    }
}

#[async_trait::async_trait]
impl ModeratorProvisioner for HelixModeratorProvisioner {
    async fn ensure_bot_is_mod(&self, broadcaster_id: &str, login: &str) -> bool {
        self.ensure_bot_is_mod_outcome(broadcaster_id, login).await
            == ModeratorProvisionOutcome::Ready
    }

    async fn ensure_bot_is_mod_outcome(
        &self,
        broadcaster_id: &str,
        login: &str,
    ) -> ModeratorProvisionOutcome {
        // Python connection.py:975-978: ohne Bot-ID kein Remod.
        if self.bot_user_id.trim().is_empty() {
            tracing::debug!(channel = login, "ensure_bot_is_mod: keine Bot-ID verfügbar");
            return ModeratorProvisionOutcome::RetryLater;
        }
        // Streamer-Token auflösen (connection.py:986 `get_tokens_for_user`) —
        // unrestricted, da der Remod auch bei deaktivierten Raids greifen muss.
        let token = match self
            .token_provider
            .get_valid_token_unrestricted(broadcaster_id, Utc::now())
            .await
        {
            Ok(Some(token)) => token,
            Ok(None) => {
                tracing::warn!(
                    channel = login,
                    "ensure_bot_is_mod: keine gültige Streamer-Autorisierung verfügbar"
                );
                return ModeratorProvisionOutcome::RetryLater;
            }
            Err(error) => {
                tracing::error!(
                    %error,
                    channel = login,
                    "ensure_bot_is_mod: Streamer-Token-Lookup fehlgeschlagen"
                );
                return ModeratorProvisionOutcome::RetryLater;
            }
        };
        match self
            .helix
            .add_channel_moderator(broadcaster_id, &self.bot_user_id, &token)
            .await
        {
            // 200/204 (Added) und 422/"already a mod" (AlreadyModerator) gelten
            // beide als Erfolg (connection.py:1013-1027).
            Ok(AddModeratorOutcome::Added) => {
                tracing::info!(
                    channel = login,
                    bot_user_id = %self.bot_user_id,
                    "ensure_bot_is_mod: Bot wieder als Moderator gesetzt"
                );
                ModeratorProvisionOutcome::Ready
            }
            Ok(AddModeratorOutcome::AlreadyModerator) => {
                tracing::info!(
                    channel = login,
                    "ensure_bot_is_mod: Bot ist bereits Moderator"
                );
                ModeratorProvisionOutcome::Ready
            }
            Ok(AddModeratorOutcome::BotBanned { status, body }) => {
                tracing::info!(
                    channel = login,
                    status,
                    body = %body,
                    "ensure_bot_is_mod: Bot ist im Kanal gebannt"
                );
                ModeratorProvisionOutcome::BotBanned
            }
            Ok(AddModeratorOutcome::AuthError { status, body }) => {
                // Kein Bann: der Streamer-Token trägt nicht mehr. Das gehört in
                // den Token-Lifecycle, nicht in die Ban-Reaktion.
                tracing::warn!(
                    channel = login,
                    status,
                    body = %body,
                    "ensure_bot_is_mod: Streamer-Autorisierung trägt nicht mehr"
                );
                ModeratorProvisionOutcome::RetryLater
            }
            Ok(AddModeratorOutcome::Failed { status, body }) => {
                tracing::warn!(
                    channel = login,
                    status,
                    body = %body,
                    "ensure_bot_is_mod: Remod fehlgeschlagen"
                );
                ModeratorProvisionOutcome::RetryLater
            }
            Err(error) => {
                tracing::error!(
                    %error,
                    channel = login,
                    "ensure_bot_is_mod: Remod-Request fehlgeschlagen"
                );
                ModeratorProvisionOutcome::RetryLater
            }
        }
    }
}

/// Gegenstück zum [`HelixModeratorProvisioner`]: gibt die Mod-Rechte des Bots in
/// einem Streamer-Kanal ab. Wird ausschließlich vom bewussten Trennen benutzt —
/// hier moddet niemand automatisch etwas zurück.
pub struct HelixModeratorRemover {
    token_provider: Arc<TokenProvider>,
    helix: HelixClient,
    bot_user_id: String,
}

impl HelixModeratorRemover {
    pub fn new(
        token_provider: Arc<TokenProvider>,
        helix: HelixClient,
        bot_user_id: String,
    ) -> Self {
        Self {
            token_provider,
            helix,
            bot_user_id,
        }
    }
}

#[async_trait::async_trait]
impl tb_internal_api::ModeratorRemovalPort for HelixModeratorRemover {
    async fn remove_bot_moderator(
        &self,
        broadcaster_id: &str,
        login: &str,
    ) -> tb_internal_api::ModeratorRemovalResult {
        use tb_internal_api::ModeratorRemovalResult as Result_;

        if self.bot_user_id.trim().is_empty() {
            return Result_::Failed {
                detail: "Bot-User-ID nicht verfügbar".to_string(),
            };
        }
        // Unrestricted wie beim Remod: der Token ist auch dann gültig, wenn
        // Raids für diesen Kanal aus sind — und genau dann trennen wir.
        let token = match self
            .token_provider
            .get_valid_token_unrestricted(broadcaster_id, Utc::now())
            .await
        {
            Ok(Some(token)) => token,
            Ok(None) => {
                tracing::warn!(
                    channel = login,
                    "Bot-Trennung: keine gültige Streamer-Autorisierung — Mod-Rechte bleiben"
                );
                return Result_::NoToken;
            }
            Err(error) => {
                tracing::error!(%error, channel = login, "Bot-Trennung: Token-Lookup fehlgeschlagen");
                return Result_::Failed {
                    detail: "Streamer-Token nicht ladbar".to_string(),
                };
            }
        };
        match self
            .helix
            .remove_channel_moderator(broadcaster_id, &self.bot_user_id, &token)
            .await
        {
            Ok(RemoveModeratorOutcome::Removed) => {
                tracing::info!(
                    channel = login,
                    bot_user_id = %self.bot_user_id,
                    "Bot-Trennung: Moderator-Rechte entzogen"
                );
                Result_::Removed
            }
            Ok(RemoveModeratorOutcome::NotModerator) => {
                tracing::info!(channel = login, "Bot-Trennung: Bot war kein Moderator");
                Result_::NotModerator
            }
            Ok(RemoveModeratorOutcome::Failed { status, body }) => {
                tracing::warn!(
                    channel = login,
                    status,
                    body = %body,
                    "Bot-Trennung: Mod-Entzug fehlgeschlagen"
                );
                Result_::Failed {
                    detail: format!("Twitch antwortete {status}"),
                }
            }
            Err(error) => {
                tracing::error!(%error, channel = login, "Bot-Trennung: Mod-Entzug-Request fehlgeschlagen");
                Result_::Failed {
                    detail: "Twitch nicht erreichbar".to_string(),
                }
            }
        }
    }
}

/// Die Deadlock-Pause entzieht dieselben Mod-Rechte wie das bewusste Trennen und
/// nimmt deshalb denselben Remover. Der Unterschied liegt nur im Auslöser, nicht
/// im Helix-Aufruf — ein zweiter Unmod-Pfad wäre eine Kopie mit eigenem Bug.
#[async_trait::async_trait]
impl tb_raid::DeadlockPauseUnmodPort for HelixModeratorRemover {
    async fn unmod_bot(&self, broadcaster_id: &str, twitch_login: &str) -> tb_raid::UnmodOutcome {
        use tb_internal_api::ModeratorRemovalPort;
        use tb_internal_api::ModeratorRemovalResult as R;

        match self
            .remove_bot_moderator(broadcaster_id, twitch_login)
            .await
        {
            R::Removed => tb_raid::UnmodOutcome::Removed,
            // Zielzustand erreicht, aber der Streamer merkt davon nichts: er
            // hatte dem Bot die Rechte längst selbst entzogen.
            R::NotModerator => tb_raid::UnmodOutcome::WasNotModerator,
            R::NoToken | R::Failed { .. } => tb_raid::UnmodOutcome::Failed,
        }
    }
}

#[async_trait::async_trait]
impl BotBanStatusProbe for HelixModeratorProvisioner {
    async fn bot_ban_status(&self, twitch_user_id: &str, twitch_login: &str) -> BotBanStatus {
        match self
            .ensure_bot_is_mod_outcome(twitch_user_id, twitch_login)
            .await
        {
            ModeratorProvisionOutcome::Ready => BotBanStatus::NotBanned,
            ModeratorProvisionOutcome::BotBanned => BotBanStatus::Banned,
            ModeratorProvisionOutcome::RetryLater => BotBanStatus::Unknown,
        }
    }
}

// ─── Session-Finalize → Raid-Score-Tracking-Resolve (B7) ─────────────────────

/// Verdrahtet den [`tb_monitoring::RaidTrackingResolver`]-Port gegen den
/// tb-raid-Score-Tracking-Store. Beim Session-Finalize werden die offenen
/// Deadlock-Raid-Zeilen der Session aufgelöst (sonst bleiben sie dauerhaft
/// `resolved_at IS NULL`). Reicht das Ziel-Spiel durch (Python `_target_game_lower`).
pub struct RaidTrackingResolverAdapter {
    store: tb_raid::ScoreTrackingStore,
    target_game_lower: String,
}

impl RaidTrackingResolverAdapter {
    pub fn new(pool: PgPool, target_game: &str) -> Self {
        Self {
            store: tb_raid::ScoreTrackingStore::new(pool),
            target_game_lower: target_game.trim().to_lowercase(),
        }
    }
}

#[async_trait::async_trait]
impl tb_monitoring::RaidTrackingResolver for RaidTrackingResolverAdapter {
    async fn resolve_for_session(
        &self,
        twitch_user_id: Option<&str>,
        streamer_login: &str,
        session_id: i64,
        session_ended_at: chrono::DateTime<Utc>,
    ) -> i64 {
        self.store
            .resolve_for_session(
                twitch_user_id,
                streamer_login,
                Some(session_id),
                Some(session_ended_at),
                &self.target_game_lower,
            )
            .await
    }
}

// ─── Hook-Bündel ─────────────────────────────────────────────────────────────

/// Vollständige EventSub-Hooks (Monitoring → Raid).
pub struct RaidEventSubHooks {
    pub manager: Arc<SubscriptionManager>,
    pub score_resolver: ScoreRefreshResolver,
    pub live_state: LiveStateStore,
    pub offline: Arc<OfflineRaidHandler>,
    pub side_effects: OfflineSideEffects,
    pub arrival: RaidArrivalCoordinator,
    pub guard: BlacklistRaidGuard,
    /// Lernt aus `channel.moderate`, wohin ein Raid wirklich geht.
    pub outgoing_raid: OutgoingRaidObserver,
    /// Go-Live-ReAuth-Reminder (B11); `None`, wenn kein nativer Chat-Send-Pfad
    /// gebootet ist (TB_CHAT_ENABLED≠1).
    pub reauth_reminder: Option<Arc<ReauthReminder>>,
    pub vod_export: Option<Arc<VodExportOfflineHandler>>,
    /// Pool für die Post-Stream-Analyse (B11), die in `on_stream_offline`
    /// fire-and-forget getriggert wird.
    pub pool: PgPool,
}

#[async_trait::async_trait]
impl EventSubHooks for RaidEventSubHooks {
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
        self.manager
            .ensure_offline_subscription(twitch_user_id, login)
            .await;
        // Go-Live-Followup (B11): Partner mit needs_reauth einmalig im Chat
        // an die fällige Re-Authentifizierung erinnern. Best-effort, eigener
        // Dedupe-Guard — der stream.offline-Sub-Pfad bleibt davon unberührt.
        if let Some(reminder) = &self.reauth_reminder {
            reminder
                .maybe_remind_for_stream(twitch_user_id, login, stream_id)
                .await;
        }
    }

    async fn on_score_refresh(
        &self,
        twitch_user_id: &str,
        login: Option<&str>,
        trigger: &'static str,
    ) {
        let user_id = twitch_user_id.trim();
        if user_id.is_empty() {
            return;
        }
        // Login auflösen, falls das Event keinen mitliefert (Sessions-Lookup
        // im Resolver läuft über den Login).
        let login = match login.map(str::trim).filter(|l| !l.is_empty()) {
            Some(l) => l.to_lowercase(),
            None => match self.live_state.login_for_user_id(user_id).await {
                Ok(Some(l)) => l,
                _ => {
                    tracing::debug!(user_id, trigger, "Score-Refresh ohne auflösbaren Login");
                    return;
                }
            },
        };
        match self
            .score_resolver
            .refresh_scores(&[(user_id.to_string(), login.clone())], Utc::now())
            .await
        {
            Ok(written) => {
                tracing::debug!(user_id, %login, trigger, written, "Partner-Score refresht");
            }
            Err(error) => {
                tracing::error!(%error, user_id, %login, trigger, "Score-Refresh fehlgeschlagen");
            }
        }
    }

    async fn on_stream_offline_engagement(&self, _twitch_user_id: &str, login: Option<&str>) {
        // Engagement-Auto-Off VOR dem Throttle (Python `eventsub_mixin.py`:1861):
        // feuert auch bei einem als Duplikat gedrosselten Offline, damit der
        // Engagement-Layer ans Stream-Leben gekoppelt bleibt.
        if let Some(login) = login.map(str::trim).filter(|l| !l.is_empty()) {
            self.side_effects.run_engagement_auto_off(login).await;
        }
    }

    async fn on_stream_offline_global_ban(&self, twitch_user_id: &str, login: Option<&str>) {
        // Global-Ban-Sweep NACH bestandenem Throttle, VOR State-Finalize
        // (Python `eventsub_mixin.py`:1908).
        if let Some(login) = login.map(str::trim).filter(|l| !l.is_empty()) {
            self.side_effects
                .run_global_ban_sweep(twitch_user_id, login)
                .await;
        }
    }

    async fn on_stream_offline(&self, twitch_user_id: &str, login: Option<&str>) {
        // Engagement-Off und Global-Ban-Sweep liefen bereits früher (siehe
        // on_stream_offline_engagement / on_stream_offline_global_ban). Hier nur
        // noch der Auto-Raid + Post-Stream-Analyse nach State-Finalize.
        self.offline
            .handle_streamer_offline(twitch_user_id, login)
            .await;
        if let Some(vod_export) = &self.vod_export {
            vod_export.spawn_for_offline(twitch_user_id, login);
        }

        // Post-Stream-Analyse (B11): fire-and-forget wie Python `create_task`,
        // damit der KI-schwere A/B-Trigger den sequenziellen EventSub-Dispatcher
        // nicht blockiert. Der Login (lowercased) genügt — der Trigger sucht die
        // letzte abgeschlossene Session selbst (im stream_offline_state-Effekt
        // wurde sie bereits finalisiert).
        if let Some(login) = login.map(str::trim).filter(|l| !l.is_empty()) {
            let pool = self.pool.clone();
            let streamer = login.to_lowercase();
            let task_streamer = streamer.clone();
            let handle = tokio::spawn(async move {
                tb_analytics::post_stream::trigger_post_stream_analysis(&pool, &streamer, None)
                    .await;
            });
            tokio::spawn(async move {
                if let Err(error) = handle.await {
                    tracing::error!(
                        streamer = %task_streamer,
                        %error,
                        "PostStream-Analyse-Task fehlerhaft beendet"
                    );
                }
            });
        }
    }

    async fn on_channel_raid(&self, event: &Value, _message_id: Option<&str>) {
        self.arrival.handle_channel_raid(event).await;
    }

    async fn on_channel_moderate(&self, broadcaster_id: &str, login: &str, event: &Value) {
        // Zuerst das echte Raid-Ziel lernen (auch bei Nicht-Partner-Zielen), dann
        // der Blacklist-Guard, der den Raid ggf. abbricht.
        self.outgoing_raid.handle(broadcaster_id, login, event);
        self.guard.handle(broadcaster_id, login, event).await;
    }

    async fn on_chat_raid_notification(&self, event: &Value, message_id: Option<&str>) {
        self.arrival
            .handle_chat_raid_notification(event, message_id)
            .await;
    }

    async fn on_chat_unraid_notification(&self, event: &Value, message_id: Option<&str>) {
        self.arrival
            .handle_chat_unraid_notification(event, message_id)
            .await;
    }
}

#[cfg(test)]
mod outgoing_raid_tests {
    use super::*;
    use serde_json::json;

    #[derive(Default)]
    struct FakeSink {
        retargeted: Mutex<Vec<(String, String)>>,
    }

    impl OutgoingRaidSink for FakeSink {
        fn raid_retargeted(&self, registration: RaidGreetingRegistration) {
            self.retargeted.lock().unwrap().push((
                registration.from_broadcaster_login,
                registration.to_broadcaster_login,
            ));
        }
    }

    fn raid_event(target_login: &str, target_id: &str) -> Value {
        json!({
            "action": "raid",
            "raid": { "user_login": target_login, "user_id": target_id, "viewer_count": 7 },
        })
    }

    #[test]
    fn raid_action_liefert_das_echte_ziel() {
        assert_eq!(
            parse_outgoing_raid(&raid_event("Dead_Eye_Nika", "224208315")),
            Some(("224208315".to_string(), "dead_eye_nika".to_string()))
        );
    }

    #[test]
    fn betroffener_nutzer_wird_je_action_gefunden() {
        assert_eq!(
            moderate_target(&raid_event("Dead_Eye_Nika", "224208315")).as_deref(),
            Some("dead_eye_nika")
        );
        assert_eq!(
            moderate_target(&json!({"action": "ban", "ban": {"user_login": "Spammer"}})).as_deref(),
            Some("spammer")
        );
        assert_eq!(
            moderate_target(&json!({"action": "timeout", "timeout": {"user_login": "x"}}))
                .as_deref(),
            Some("x")
        );
        // Actions ohne Nutzer-Bezug (z. B. emoteonly) liefern nichts — die
        // Log-Zeile entsteht trotzdem.
        assert_eq!(moderate_target(&json!({"action": "emoteonly"})), None);
    }

    #[test]
    fn andere_moderate_actions_liefern_nichts() {
        assert_eq!(parse_outgoing_raid(&json!({"action": "unraid"})), None);
        assert_eq!(
            parse_outgoing_raid(&json!({"action": "ban", "ban": {"user_login": "x"}})),
            None
        );
        assert_eq!(parse_outgoing_raid(&json!({"action": "raid"})), None);
    }

    #[test]
    fn manueller_raid_sperrt_den_auto_raid_und_zieht_die_erinnerung_um() {
        let suppression = Arc::new(Mutex::new(ManualRaidSuppression::new()));
        let sink = Arc::new(FakeSink::default());
        let observer = OutgoingRaidObserver::new(
            suppression.clone(),
            Some(sink.clone() as Arc<dyn OutgoingRaidSink>),
        );

        observer.handle(
            "1186925760",
            "earlysalty",
            &raid_event("dead_eye_nika", "224208315"),
        );

        assert!(suppression
            .lock()
            .unwrap()
            .is_suppressed("1186925760", None));
        assert_eq!(
            sink.retargeted.lock().unwrap().as_slice(),
            [("earlysalty".to_string(), "dead_eye_nika".to_string())]
        );
    }

    #[test]
    fn nicht_raid_actions_sperren_nichts() {
        let suppression = Arc::new(Mutex::new(ManualRaidSuppression::new()));
        let sink = Arc::new(FakeSink::default());
        let observer = OutgoingRaidObserver::new(
            suppression.clone(),
            Some(sink.clone() as Arc<dyn OutgoingRaidSink>),
        );

        observer.handle("1186925760", "earlysalty", &json!({"action": "unraid"}));

        assert!(!suppression
            .lock()
            .unwrap()
            .is_suppressed("1186925760", None));
        assert!(sink.retargeted.lock().unwrap().is_empty());
    }

    #[test]
    fn ohne_broadcaster_id_passiert_nichts() {
        let suppression = Arc::new(Mutex::new(ManualRaidSuppression::new()));
        let sink = Arc::new(FakeSink::default());
        let observer = OutgoingRaidObserver::new(
            suppression.clone(),
            Some(sink.clone() as Arc<dyn OutgoingRaidSink>),
        );

        observer.handle(
            "  ",
            "earlysalty",
            &raid_event("dead_eye_nika", "224208315"),
        );

        assert!(sink.retargeted.lock().unwrap().is_empty());
    }
}

#[cfg(test)]
mod vod_export_tests {
    use super::{vod_export_channel_content, vod_export_dm_content};

    #[test]
    fn dm_text_enthaelt_link_und_gueltigkeit() {
        let content = vod_export_dm_content("https://share.example/vod");

        assert!(content.contains("https://share.example/vod"));
        assert!(content.contains("Drive"));
    }

    #[test]
    fn caster_chat_text_traegt_den_link_und_den_kanal() {
        let content = vod_export_channel_content("https://share.example/vod");

        assert!(content.contains("https://share.example/vod"), "{content}");
        assert!(content.contains("dach_lock"), "{content}");
    }

    #[test]
    fn caster_chat_ist_nicht_der_admin_log() {
        assert_ne!(
            super::VOD_EXPORT_CASTER_CHANNEL_ID,
            super::VOD_EXPORT_LOG_CHANNEL_ID
        );
    }

    #[test]
    fn caster_embed_traegt_kennzahlen_aber_kein_prozess_stderr() {
        let report = tb_highlight::vod_export::VodExportReport {
            vod_id: "987".to_string(),
            link: "https://share.example/987".to_string(),
            duration_seconds: 3 * 60 * 60 + 25 * 60,
            size_bytes: 9_942_383_597,
        };

        let embed = super::vod_export_caster_embed(&report);
        let text = embed["description"].as_str().expect("description");

        assert!(text.contains("987"), "{text}");
        assert!(text.contains("3h 25m"), "{text}");
        assert!(text.contains("9.26 GB"), "{text}");
        // Der Link steht als Nachrichtentext daneben, nicht im Embed — und der
        // Fehlerzweig mit rohem stderr erreicht diesen Kanal gar nicht erst.
        assert!(!text.contains("share.example"), "{text}");
        assert_eq!(embed["title"], "VOD-Export erfolgreich");
    }
}

// ─── Tests (P2.43 Sekundär-Signal-Pre-Gate) ──────────────────────────────────

#[cfg(all(test, feature = "integration"))]
mod arrival_dedupe_tests {
    use super::*;
    use std::str::FromStr;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use tb_raid::pending_raids::PendingRaid;
    use tb_raid::RaidArrivalSink;

    /// Zählender Stub-Sink: protokolliert pro Action-Kind, wie oft er gerufen
    /// wurde — so lässt sich beweisen, dass ein dedupliziertes Signal NUR
    /// `record_secondary_signal` auslöst (kein Orphan/Independent/Confirm).
    #[derive(Default)]
    struct CountingSink {
        secondary: AtomicUsize,
        orphan: AtomicUsize,
        independent: AtomicUsize,
        confirm: AtomicUsize,
        store_pending: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl RaidArrivalSink for CountingSink {
        async fn record_secondary_signal(
            &self,
            _signal_type: &str,
            _from_broadcaster_login: &str,
            _from_broadcaster_id: Option<&str>,
            _to_broadcaster_login: &str,
            _to_broadcaster_id: &str,
            _viewer_count: i32,
            _unraid_seen: bool,
        ) {
            self.secondary.fetch_add(1, Ordering::SeqCst);
        }
        async fn record_pending_observation(
            &self,
            _pending: &PendingRaid,
            _signal_type: &str,
            _status: &str,
            _reason: Option<&str>,
            _detail: Option<&str>,
        ) {
        }
        async fn store_pending_raid(&self, _pending: &PendingRaid) {
            self.store_pending.fetch_add(1, Ordering::SeqCst);
        }
        async fn store_orphan_chat_notification(
            &self,
            _to_broadcaster_id: &str,
            _to_broadcaster_login: &str,
            _from_broadcaster_id: Option<&str>,
            _from_broadcaster_login: &str,
            _viewer_count: i32,
            _message_id: Option<&str>,
            _event_timestamp: Option<&str>,
        ) {
            self.orphan.fetch_add(1, Ordering::SeqCst);
        }
        async fn confirm_pending_raid(
            &self,
            _signal_type: &str,
            _to_broadcaster_id: &str,
            _to_broadcaster_login: &str,
            _from_broadcaster_login: &str,
            _from_broadcaster_id: Option<&str>,
            _viewer_count: i32,
        ) {
            self.confirm.fetch_add(1, Ordering::SeqCst);
        }
        async fn mark_manual_raid_started(&self, _source_key: &str, _ttl_seconds: f64) {}
        async fn record_independent_raid_arrival(
            &self,
            _signal_type: &str,
            _from_broadcaster_login: &str,
            _from_broadcaster_id: Option<&str>,
            _to_broadcaster_login: &str,
            _to_broadcaster_id: &str,
            _viewer_count: i32,
        ) {
            self.independent.fetch_add(1, Ordering::SeqCst);
        }
    }

    async fn setup_db(schema: &str) -> PgPool {
        let url = std::env::var("TB_TEST_DATABASE_URL").expect(
            "TB_TEST_DATABASE_URL fehlt — `rust/scripts/test_db.sh up` und die URL exportieren",
        );
        let admin = sqlx::PgPool::connect(&url).await.unwrap();
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
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(4)
            .connect_with(opts)
            .await
            .unwrap();
        sqlx::query(
            r#"
            CREATE TABLE twitch_raid_arrival_tracking (
                id                        SERIAL PRIMARY KEY,
                detected_at               TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                last_signal_at            TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                from_broadcaster_id       TEXT,
                from_broadcaster_login    TEXT NOT NULL,
                to_broadcaster_id         TEXT NOT NULL,
                to_broadcaster_login      TEXT NOT NULL,
                viewer_count              INTEGER NOT NULL DEFAULT 0,
                classification            TEXT NOT NULL DEFAULT '',
                confirmation_signals      TEXT NOT NULL DEFAULT '',
                primary_signal            TEXT NOT NULL DEFAULT '',
                correlation_status        TEXT NOT NULL DEFAULT '',
                correlation_detail        TEXT,
                source_resolution         TEXT NOT NULL DEFAULT '',
                raid_history_id           BIGINT,
                raid_history_executed_at  TIMESTAMPTZ,
                unraid_seen               BOOLEAN NOT NULL DEFAULT FALSE,
                last_unraid_at            TIMESTAMPTZ
            )
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    fn coordinator(pool: PgPool, sink: Arc<CountingSink>) -> RaidArrivalCoordinator {
        let runtime = RaidArrivalRuntime::new(sink);
        let pending = Arc::new(Mutex::new(PendingRaidStore::new()));
        RaidArrivalCoordinator::new(pool, pending, runtime)
    }

    /// Zweites channel.raid-Event für denselben from->to innerhalb der TTL
    /// wird als Sekundär-Signal dedupliziert: genau EIN secondary, KEIN
    /// independent/orphan/confirm. Ohne das Pre-Gate würde der zweite Raid
    /// (kein Pending) als eigenständiger Arrival erneut verarbeitet.
    #[tokio::test]
    async fn zweites_raid_signal_wird_als_secondary_dedupliziert() {
        let pool = setup_db("p243_dedupe").await;
        // Bestätigter Arrival ist bereits vorhanden (von der ersten Korrelation).
        sqlx::query(
            "INSERT INTO twitch_raid_arrival_tracking
                (from_broadcaster_login, to_broadcaster_id, to_broadcaster_login,
                 confirmation_signals, detected_at)
             VALUES ('raider', '200', 'target', 'channel_raid', NOW())",
        )
        .execute(&pool)
        .await
        .unwrap();

        let sink = Arc::new(CountingSink::default());
        let coord = coordinator(pool, sink.clone());
        let event = serde_json::json!({
            "from_broadcaster_user_login": "raider",
            "from_broadcaster_user_id": "100",
            "to_broadcaster_user_id": "200",
            "to_broadcaster_user_login": "target",
            "viewers": 42,
        });
        coord.handle_channel_raid(&event).await;

        assert_eq!(
            sink.secondary.load(Ordering::SeqCst),
            1,
            "genau ein Sekundär-Signal"
        );
        assert_eq!(
            sink.independent.load(Ordering::SeqCst),
            0,
            "kein zweiter eigenständiger Arrival"
        );
        assert_eq!(sink.orphan.load(Ordering::SeqCst), 0);
        assert_eq!(sink.confirm.load(Ordering::SeqCst), 0);

        // Nur eine Arrival-Zeile insgesamt (keine Dublette).
        let count: (i64,) =
            sqlx::query_as("SELECT COUNT(*)::bigint FROM twitch_raid_arrival_tracking")
                .fetch_one(&coord.pool)
                .await
                .unwrap();
        assert_eq!(count.0, 1, "keine zweite/orphan Arrival-Zeile");
    }

    /// Späte chat.notification für denselben Raid → Sekundär-Signal mit
    /// unraid_seen=false, NICHT als Orphan.
    #[tokio::test]
    async fn spaete_chat_notification_wird_sekundaer_statt_orphan() {
        let pool = setup_db("p243_chatnotif").await;
        sqlx::query(
            "INSERT INTO twitch_raid_arrival_tracking
                (from_broadcaster_login, to_broadcaster_id, to_broadcaster_login,
                 confirmation_signals, detected_at)
             VALUES ('raider', '200', 'target', 'channel_raid', NOW())",
        )
        .execute(&pool)
        .await
        .unwrap();

        let sink = Arc::new(CountingSink::default());
        let coord = coordinator(pool, sink.clone());
        let event = serde_json::json!({
            "broadcaster_user_id": "200",
            "broadcaster_user_login": "target",
            "raid": { "user_login": "raider", "user_id": "100", "viewer_count": 42 },
        });
        coord
            .handle_chat_raid_notification(&event, Some("msg-1"))
            .await;

        assert_eq!(sink.secondary.load(Ordering::SeqCst), 1);
        assert_eq!(
            sink.orphan.load(Ordering::SeqCst),
            0,
            "kein Orphan trotz fehlendem Pending"
        );
    }

    /// Ohne jüngeren Arrival im Fenster greift das Gate NICHT: der Plan läuft
    /// den normalen Pfad (hier: eigenständiger Arrival), kein Sekundär-Signal.
    #[tokio::test]
    async fn ohne_recent_arrival_kein_secondary() {
        let pool = setup_db("p243_none").await;
        // Arrival existiert, aber außerhalb der TTL (> 600 s alt).
        sqlx::query(
            "INSERT INTO twitch_raid_arrival_tracking
                (from_broadcaster_login, to_broadcaster_id, to_broadcaster_login,
                 confirmation_signals, detected_at)
             VALUES ('raider', '200', 'target', 'channel_raid', NOW() - INTERVAL '20 minutes')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let sink = Arc::new(CountingSink::default());
        let coord = coordinator(pool, sink.clone());
        let event = serde_json::json!({
            "from_broadcaster_user_login": "raider",
            "from_broadcaster_user_id": "100",
            "to_broadcaster_user_id": "200",
            "to_broadcaster_user_login": "target",
            "viewers": 42,
        });
        coord.handle_channel_raid(&event).await;

        assert_eq!(
            sink.secondary.load(Ordering::SeqCst),
            0,
            "alter Arrival → kein Sekundär-Signal"
        );
    }
}
