# cogs/twitch/raid_manager.py
"""
Raid Bot Manager - RaidBot

Verwaltet:
- Automatische Raids beim Offline-Gehen
- Partner-Auswahl (niedrigste Viewer, optional niedrigste Follower)
- Raid-Metadaten und History
"""

import asyncio
import logging
import os
import time
from collections.abc import Mapping
from datetime import UTC, datetime
from typing import Any

import aiohttp

from ..core.constants import TWITCH_TARGET_GAME_NAME
from .scope_profiles import BASE_STREAMER_SCOPES
from .arrival_confirmation import ArrivalConfirmationService
from .raid_arrival_runtime import RaidArrivalRuntime, RaidArrivalRuntimeDependencies
from .candidate_selection import CandidateSelectionService
from .chat_targets import lookup_outbound_chat_suppression, make_chat_target
from .external_recruitment import ExternalRecruitmentService
from .followers import CandidateFollowersDependencies, CandidateFollowersService
from .lifecycle import RaidBotLifecycle
from .manual_raid_suppression import (
    ManualRaidSuppressionDependencies,
    ManualRaidSuppressionService,
)
from .offline_raid_orchestrator import OfflineRaidOrchestrator
from .observability import RaidObservabilityEvent, RaidObservabilityService
from .partner_arrival_tracking import (
    PartnerArrivalTrackingDependencies,
    PartnerArrivalTrackingService,
)
from .partner_setup_service import PartnerSetupService
from .partner_raid_delivery import (
    PartnerRaidDeliveryConfig,
    PartnerRaidDeliveryDependencies,
    PartnerRaidDeliveryPlanner,
    PartnerRaidDeliveryService,
)
from .pending_raids import PendingRaid, PendingRaidStore
from .raid_blacklist import RaidBlacklistConfig, RaidBlacklistService, build_runtime_raid_blacklist_service
from .raid_data_sources import RaidDataSourceService
from .raid_metrics_store import RaidMetricsStore
from .raid_pipeline import RaidPipelineDependencies, RaidPipelineRequest, RaidPipelineService, is_retryable_raid_error
from .raid_state_store import RaidStateStore, RaidStateStoreConfig
from .raid_tracking_runtime import (
    RaidTrackingRuntimeDependencies,
    RaidTrackingRuntimeService,
    RaidTrackingRuntimeState,
)
from .recruitment_messaging import RecruitmentMessagingService, build_runtime_recruitment_messaging_service
from .signal_correlation import RaidSignalCorrelationService
from ..storage import (
    insert_observability_event,
    load_active_partner,
    load_offline_auto_raid_eligibility,
    load_streamer_identity,
    readonly_connection,
    transaction,
)
try:
    from .partner_scores import (
        load_partner_raid_score_map,
        refresh_partner_raid_score_async,
    )
except Exception:  # pragma: no cover - best effort if helper is unavailable during partial deploys
    load_partner_raid_score_map = None  # type: ignore[assignment]
    refresh_partner_raid_score_async = None  # type: ignore[assignment]
try:
    from .partner_raid_score_tracking import track_confirmed_partner_raid
except Exception:  # pragma: no cover - best effort if helper is unavailable during partial deploys
    track_confirmed_partner_raid = None  # type: ignore[assignment]

TWITCH_TOKEN_URL = "https://id.twitch.tv/oauth2/token"  # noqa: S105
TWITCH_AUTHORIZE_URL = "https://id.twitch.tv/oauth2/authorize"
TWITCH_API_BASE = "https://api.twitch.tv/helix"

# Erforderliche Scopes für Raid-Funktionalität + Zusatz-Metriken (Follower/Chat)
# Hinweis: Re-Auth notwendig, falls bisher nur channel:manage:raids erteilt war.
RAID_SCOPES = list(BASE_STREAMER_SCOPES)

RAID_TARGET_COOLDOWN_DAYS = 7  # Avoid repeating the same raid target if alternatives exist
RECRUIT_DISCORD_INVITE = (
    os.getenv("RECRUIT_DISCORD_INVITE") or ""
).strip() or "Discord: Server hinzufügen & Code eingeben: z5TfVHuQq2"
RECRUIT_DISCORD_INVITE_DIRECT = (
    os.getenv("RECRUIT_DISCORD_INVITE_DIRECT") or ""
).strip() or "https://discord.gg/z5TfVHuQq2"

_recruit_direct_invite_threshold_raw = (
    os.getenv("RECRUIT_DIRECT_INVITE_MAX_FOLLOWERS") or "120"
).strip()
try:
    RECRUIT_DIRECT_INVITE_MAX_FOLLOWERS = max(0, int(_recruit_direct_invite_threshold_raw))
except ValueError:
    RECRUIT_DIRECT_INVITE_MAX_FOLLOWERS = 120
log = logging.getLogger("TwitchStreams.RaidManager")

_PENDING_CHAT_NOTIFICATION_GRACE_SECONDS = 15.0
_RECENT_RAID_ARRIVAL_TTL_SECONDS = 600.0
_ORPHAN_CHAT_NOTIFICATION_RETENTION_SECONDS = 900.0
_RAID_READINESS_TTL_SECONDS = 900.0
_RAID_READINESS_MAX_ENTRIES = 512
_EXTERNAL_RECRUITMENT_RAID_LIMIT = 4
_EXTERNAL_BAN_CHECK_DELAY_SECONDS = 3600.0
_EXTERNAL_RECRUITMENT_BLACKLIST_GRACE_SECONDS = 48 * 3600.0


def _mask_log_identifier(value: object, *, visible_prefix: int = 3, visible_suffix: int = 2) -> str:
    text = str(value or "").strip()
    if not text:
        return "<empty>"
    if len(text) <= visible_prefix + visible_suffix:
        return "***"
    return f"{text[:visible_prefix]}...{text[-visible_suffix:]}"


from .auth import RaidAuthManager  # noqa: E402
from .executor import RaidExecutor  # noqa: E402


class RaidBot:
    """
    Hauptklasse für automatische Raid-Verwaltung.

    - Erkennt, wenn ein Partner offline geht
    - Wählt Partner nach niedrigsten Viewern (Tie-Breaker: Follower, dann Stream-Zeit)
    - Führt den Raid aus und loggt Metadaten (gesendete + empfangene Raids)
    """

    def __init__(
        self,
        client_id: str,
        client_secret: str,
        redirect_uri: str,
        session: aiohttp.ClientSession,
    ):
        self.auth_manager = RaidAuthManager(client_id, client_secret, redirect_uri)
        self.raid_executor = RaidExecutor(client_id, self.auth_manager)
        self._session = session
        self.chat_bot = None  # Wird später gesetzt
        self._bot_id = None  # Wird bei set_chat_bot gesetzt als Fallback
        self._cog = None  # Referenz zum TwitchStreamCog für EventSub subscriptions

        # Pending Raids werden bis zur Ziel-Bestaetigung per channel.raid und/oder
        # channel.chat.notification gehalten.
        self._pending_raids: dict[tuple[str, str], PendingRaid] = {}
        self._recent_raid_arrivals: dict[tuple[str, str], dict[str, Any]] = {}
        self._orphan_chat_raid_notifications: dict[tuple[str, str], dict[str, Any]] = {}
        # Unterdrückt den nächsten Offline-Auto-Raid, wenn kurz zuvor ein manueller/externer Raid erkannt wurde.
        self._manual_raid_suppression: dict[str, float] = {}
        self._user_scope_fallback_warned: set[tuple[str, str]] = set()
        self._lifecycle = RaidBotLifecycle(
            self._periodic_cleanup,
            logger=log,
        )
        self._managed_bg_tasks: set[asyncio.Task[Any]] = self._lifecycle._managed_tasks
        self._cleanup_task: asyncio.Task[Any] | None = None

    def _managed_bg_task_registry(self) -> set[asyncio.Task[Any]]:
        lifecycle = getattr(self, "_lifecycle", None)
        lifecycle_tasks = getattr(lifecycle, "_managed_tasks", None)
        if isinstance(lifecycle_tasks, set):
            self._managed_bg_tasks = lifecycle_tasks
            return lifecycle_tasks
        tasks = getattr(self, "_managed_bg_tasks", None)
        if not isinstance(tasks, set):
            tasks = set()
            self._managed_bg_tasks = tasks
        return tasks

    def _track_bg_task(self, task: asyncio.Task[Any]) -> asyncio.Task[Any]:
        registry = self._managed_bg_task_registry()
        registry.add(task)

        def _discard(completed: asyncio.Task[Any]) -> None:
            registry.discard(completed)

        task.add_done_callback(_discard)
        return task

    def _spawn_bg_task(
        self,
        coro: Any,
        name: str,
    ) -> asyncio.Task[Any] | None:
        lifecycle = getattr(self, "_lifecycle", None)
        if lifecycle is not None:
            task = lifecycle.spawn_background_task(coro, name=name)
            self._cleanup_task = lifecycle.cleanup_task
            return task
        try:
            task = asyncio.create_task(coro, name=name)
        except RuntimeError as exc:
            log.error("Cannot start RaidBot background task %s (no running loop yet): %s", name, exc)
            coro.close()
            return None
        except Exception:
            log.exception("Failed to start RaidBot background task %s", name)
            coro.close()
            return None
        return self._track_bg_task(task)

    async def _cancel_managed_bg_tasks(self) -> None:
        lifecycle = getattr(self, "_lifecycle", None)
        if lifecycle is not None:
            await lifecycle.stop()
            self._cleanup_task = lifecycle.cleanup_task
            self._managed_bg_tasks = lifecycle._managed_tasks
            return
        registry = list(self._managed_bg_task_registry())
        if not registry:
            return
        self._managed_bg_tasks = set()
        for task in registry:
            if task.done():
                continue
            task.cancel()
        for task in registry:
            if task.done():
                continue
            try:
                await task
            except asyncio.CancelledError:
                log.debug("RaidBot managed background task cancelled: %s", task.get_name())
            except Exception:
                log.debug(
                    "RaidBot managed background task failed during shutdown: %s",
                    task.get_name(),
                    exc_info=True,
                )

    @property
    def session(self) -> aiohttp.ClientSession | None:
        """Return an active HTTP session; refresh from cog/api if the cached one is closed."""
        if self._session is not None and not self._session.closed:
            return self._session

        cog = getattr(self, "_cog", None)
        api = getattr(cog, "api", None) if cog is not None else None
        if api is not None:
            try:
                refreshed = api.get_http_session()
                if refreshed is not None and not refreshed.closed:
                    if self._session is not refreshed:
                        log.warning(
                            "RaidBot detected closed HTTP session; switched to fresh TwitchAPI session"
                        )
                    self._session = refreshed
                    return refreshed
            except Exception:
                log.debug(
                    "RaidBot could not refresh HTTP session from TwitchAPI",
                    exc_info=True,
                )
        return None

    @session.setter
    def session(self, value: aiohttp.ClientSession | None) -> None:
        self._session = value

    async def cleanup(self):
        """Stoppt Hintergrund-Tasks."""
        await self._cancel_managed_bg_tasks()

    def start(self) -> asyncio.Task[Any] | None:
        """Start explicit RaidBot background work after construction."""
        lifecycle = getattr(self, "_lifecycle", None)
        if lifecycle is None:
            self._cleanup_task = self._spawn_bg_task(
                self._periodic_cleanup(),
                "raid.bot.periodic_cleanup",
            )
            return self._cleanup_task
        self._cleanup_task = lifecycle.start()
        self._managed_bg_tasks = lifecycle._managed_tasks
        return self._cleanup_task

    async def _periodic_cleanup(self):
        """
        Periodische Wartung:
        1. Cleanup abgelaufener Auth-States (alle 30min)
        2. Proaktiver Refresh von User-Tokens (alle 30min; intern expiry-gebremst)
        3. Cleanup alter pending raids (alle 2min)
        """
        state_cleanup_interval = 1800.0
        token_refresh_interval = 1800.0
        blacklist_cleanup_interval = 7 * 1800.0
        pending_raid_cleanup_interval = 120.0
        grace_period_check_interval = 3600.0  # stündlich
        external_limit_pending_interval = 600.0
        external_bot_ban_check_interval = 300.0

        last_state_cleanup = 0.0
        # Startup-Delay: erster Token-Refresh erst nach 5 Minuten (nicht sofort nach 60s).
        # Verhindert Race-Condition wenn ein alter Prozess noch kurz weiterläuft,
        # bevor der PID-Lock greift.
        last_token_refresh = time.time() - token_refresh_interval + 300.0
        last_blacklist_cleanup = 0.0
        last_raid_cleanup = 0.0
        last_grace_period_check = 0.0
        last_external_limit_pending_check = 0.0
        last_external_bot_ban_check = 0.0
        while True:
            await asyncio.sleep(60)  # Loop-Tick (Wartungs-Tasks laufen in eigenen Intervallen)
            try:
                now = time.time()

                # 1. State Cleanup (alle 30min)
                if now - last_state_cleanup >= state_cleanup_interval:
                    self.auth_manager.cleanup_states()
                    last_state_cleanup = now

                # 2. Token Maintenance (alle 30min; refresh_all_tokens prüft intern Expiry)
                if now - last_token_refresh >= token_refresh_interval:
                    active_session = self.session
                    if active_session is None:
                        log.warning("Skipping token maintenance: no active HTTP session available")
                    else:
                        try:
                            await self.auth_manager.refresh_all_tokens(active_session)
                        except RuntimeError as exc:
                            if "Session is closed" in str(exc):
                                self.session = None
                                log.warning(
                                    "Token maintenance deferred: shared HTTP session closed; retrying next tick"
                                )
                            else:
                                raise
                        else:
                            last_token_refresh = now

                # Token Blacklist Cleanup (alle 3.5h)
                if now - last_blacklist_cleanup >= blacklist_cleanup_interval:
                    self.auth_manager.token_error_handler.cleanup_old_entries(days=30)
                    last_blacklist_cleanup = now

                # Grace-Period Check (stündlich): Erinnerung + Rolle entfernen bei Ablauf
                if now - last_grace_period_check >= grace_period_check_interval:
                    await self.auth_manager.token_error_handler.check_grace_periods()
                    last_grace_period_check = now

                if now - last_external_limit_pending_check >= external_limit_pending_interval:
                    await asyncio.to_thread(self._process_due_external_recruitment_blacklist_pending)
                    last_external_limit_pending_check = now

                if now - last_external_bot_ban_check >= external_bot_ban_check_interval:
                    await self._process_due_external_target_ban_checks()
                    last_external_bot_ban_check = now

                # 3. Pending Raids Cleanup (alle 2min)
                if now - last_raid_cleanup >= pending_raid_cleanup_interval:
                    self._cleanup_stale_pending_raids()
                    self._cleanup_recent_raid_arrivals()
                    self._cleanup_stale_raid_readiness_states()
                    self._promote_stale_orphan_chat_raid_notifications()
                    self._cleanup_expired_manual_raid_suppressions()
                    last_raid_cleanup = now

            except Exception:
                log.exception("Error during periodic raid bot maintenance")

    def set_chat_bot(self, chat_bot):
        """Setzt den Twitch Chat Bot für Recruitment-Nachrichten."""
        self.chat_bot = chat_bot
        # Bot-ID speichern damit complete_setup auch ohne chat_bot funktioniert
        if chat_bot:
            bot_id = getattr(chat_bot, "bot_id_safe", None) or getattr(chat_bot, "bot_id", None)
            if bot_id and str(bot_id).strip():
                self._bot_id = str(bot_id).strip()

    def set_discord_bot(self, discord_bot):
        """
        Setzt die Discord Bot-Instanz für Token-Error-Benachrichtigungen.

        Args:
            discord_bot: Discord Client/Bot Instanz
        """
        self.auth_manager.token_error_handler.discord_bot = discord_bot
        self.auth_manager._discord_bot = discord_bot
        log.debug("Discord bot set for token error notifications")

    def set_cog(self, cog):
        """
        Setzt die Cog-Referenz für dynamische EventSub subscriptions.

        Args:
            cog: TwitchStreamCog Instanz
        """
        self._cog = cog
        log.debug("Cog reference set for dynamic EventSub subscriptions")

    @staticmethod
    def _subscription_notice_eventsub_type(notice_type: str | None) -> str | None:
        normalized = str(notice_type or "").strip().lower()
        if normalized.startswith("shared_chat_"):
            normalized = normalized.removeprefix("shared_chat_")
        if normalized == "sub":
            return "channel.subscribe"
        if normalized == "resub":
            return "channel.subscription.message"
        if normalized in {"sub_gift", "community_sub_gift"}:
            return "channel.subscription.gift"
        return None

    def should_capture_chat_subscription_notice(
        self,
        *,
        broadcaster_id: str,
        notice_type: str | None,
    ) -> bool:
        eventsub_type = self._subscription_notice_eventsub_type(notice_type)
        if not eventsub_type:
            return False

        cog = getattr(self, "_cog", None)
        has_sub = getattr(cog, "_eventsub_has_sub", None) if cog is not None else None
        if not callable(has_sub):
            return True

        try:
            return not bool(has_sub(eventsub_type, str(broadcaster_id or "").strip()))
        except Exception:
            log.debug(
                "Chat subscription notice fallback check failed for %s (%s)",
                broadcaster_id,
                eventsub_type,
                exc_info=True,
            )
            return True

    async def on_chat_subscription_notification(
        self,
        *,
        broadcaster_id: str,
        broadcaster_login: str,
        notice_type: str | None,
        event_type: str,
        event: dict[str, Any],
    ) -> bool:
        if not self.should_capture_chat_subscription_notice(
            broadcaster_id=broadcaster_id,
            notice_type=notice_type,
        ):
            log.debug(
                "Skipping chat-derived subscription fallback for %s (%s): dedicated EventSub active",
                broadcaster_login or broadcaster_id,
                notice_type or event_type,
            )
            return False

        cog = getattr(self, "_cog", None)
        store_event = getattr(cog, "_store_subscription_event", None) if cog is not None else None
        if not callable(store_event):
            log.debug(
                "Skipping chat-derived subscription fallback for %s (%s): no storage handler",
                broadcaster_login or broadcaster_id,
                notice_type or event_type,
            )
            return False

        await store_event(str(broadcaster_id or "").strip(), event, event_type)
        return True

    @staticmethod
    def _normalize_broadcaster_login(raw_value: str | None) -> str:
        return str(raw_value or "").strip().lower()

    @staticmethod
    def _build_raid_arrival_cache_key(
        *,
        to_broadcaster_id: str,
        from_broadcaster_login: str,
    ) -> tuple[str, str]:
        return (
            str(to_broadcaster_id or "").strip(),
            str(from_broadcaster_login or "").strip().lower(),
        )

    @staticmethod
    def _serialize_confirmation_signals(signals: set[str] | list[str] | tuple[str, ...]) -> str:
        return ",".join(sorted({str(signal).strip() for signal in signals if str(signal).strip()}))

    def _next_raid_observability_flow_id(self, *, prefix: str) -> str:
        service = self._make_raid_observability_service()
        flow_id = service.next_flow_id(prefix=prefix)
        self._raid_observability_sequence = service.sequence
        self._raid_observability_counter_store = service.counter_store
        return flow_id

    def _raid_observability_counters(self) -> dict[str, int]:
        counters = getattr(self, "_raid_observability_counter_store", None)
        if not isinstance(counters, dict):
            counters = {}
            self._raid_observability_counter_store = counters
        return counters

    def _make_raid_observability_service(self) -> RaidObservabilityService:
        def _sink(event: RaidObservabilityEvent) -> None:
            storage_payload = event.as_storage_payload()
            insert_observability_event(
                flow_type=str(storage_payload.get("flow_type") or "raid"),
                flow_id=str(storage_payload.get("flow_id") or ""),
                entity_login=str(storage_payload.get("entity_login") or ""),
                entity_id=str(storage_payload.get("entity_id") or ""),
                step=str(storage_payload.get("step") or "event"),
                decision=str(storage_payload.get("decision") or "unknown"),
                details=event.as_log_fields(),
            )

        service = RaidObservabilityService(
            event_sink=_sink,
            counter_store=self._raid_observability_counters(),
        )
        service.sequence = int(getattr(self, "_raid_observability_sequence", 0) or 0)
        return service

    @staticmethod
    def _partner_raid_delivery_planner() -> PartnerRaidDeliveryPlanner:
        return PartnerRaidDeliveryPlanner(
            PartnerRaidDeliveryConfig(
                delay_seconds=5.0,
            )
        )

    def _partner_raid_delivery_service(self) -> PartnerRaidDeliveryService:
        return PartnerRaidDeliveryService(
            PartnerRaidDeliveryDependencies(
                get_chat_bot=lambda: self.chat_bot,
                count_received_network_raids=self._get_received_network_raid_count,
                lookup_outbound_chat_suppression=lambda **kwargs: self._lookup_outbound_chat_suppression(
                    str(kwargs.get("target_login") or ""),
                    str(kwargs.get("target_id") or "") or None,
                    source=str(kwargs.get("source") or ""),
                ),
                join_chat_channel=lambda chat_bot, channel_login, channel_id: chat_bot.join(
                    channel_login,
                    channel_id=channel_id,
                ),
                send_chat_message=lambda chat_bot, channel, message, source: (
                    chat_bot._send_chat_message(
                        channel,
                        message,
                        source=source,
                    )
                    if hasattr(chat_bot, "_send_chat_message")
                    else None
                ),
                logger=log,
            ),
            planner=self._partner_raid_delivery_planner(),
        )

    def _external_recruitment_service(self) -> ExternalRecruitmentService:
        return ExternalRecruitmentService(
            persist_confirmed_raid=self._record_confirmed_external_recruitment_raid,
            count_confirmed_raids=self._get_confirmed_external_recruitment_raid_count,
            schedule_pending_blacklist=self._schedule_external_recruitment_blacklist_pending,
            delete_pending_blacklist=self._delete_external_recruitment_blacklist_pending,
            is_target_partner=self._is_target_currently_partner,
        )

    def _arrival_confirmation_service(self) -> ArrivalConfirmationService:
        return ArrivalConfirmationService(
            partner_lookup=lambda **lookup_kwargs: self._lookup_partner_target_channel(
                broadcaster_id=str(lookup_kwargs.get("twitch_user_id") or ""),
                broadcaster_login=str(lookup_kwargs.get("twitch_login") or ""),
            ),
            known_streamer_lookup=lambda **lookup_kwargs: self._resolve_known_streamer_identity(
                broadcaster_login=str(lookup_kwargs.get("broadcaster_login") or ""),
                broadcaster_id=str(lookup_kwargs.get("broadcaster_id") or "") or None,
            ),
        )

    @staticmethod
    def _raid_state_store_config() -> RaidStateStoreConfig:
        return RaidStateStoreConfig(
            recent_raid_arrival_ttl_seconds=_RECENT_RAID_ARRIVAL_TTL_SECONDS,
            orphan_chat_notification_grace_seconds=_PENDING_CHAT_NOTIFICATION_GRACE_SECONDS,
            orphan_chat_notification_retention_seconds=_ORPHAN_CHAT_NOTIFICATION_RETENTION_SECONDS,
            raid_readiness_ttl_seconds=_RAID_READINESS_TTL_SECONDS,
            raid_readiness_max_entries=_RAID_READINESS_MAX_ENTRIES,
        )

    def _raid_state_store(self) -> RaidStateStore:
        return RaidStateStore(
            self,
            config=self._raid_state_store_config(),
            logger=log,
        )

    def _manual_raid_suppression_service(self) -> ManualRaidSuppressionService:
        return ManualRaidSuppressionService(
            self,
            ManualRaidSuppressionDependencies(
                readonly_connection=readonly_connection,
                load_active_partner=load_active_partner,
                logger=log,
            ),
        )

    def _partner_arrival_tracking_service(self) -> PartnerArrivalTrackingService:
        manual_suppression = self._manual_raid_suppression_service()
        state_store = self._raid_state_store()
        return PartnerArrivalTrackingService(
            PartnerArrivalTrackingDependencies(
                readonly_connection=readonly_connection,
                transaction=transaction,
                load_active_partner=load_active_partner,
                load_streamer_identity=load_streamer_identity,
                resolve_streamer_id_by_login=manual_suppression.resolve_streamer_id_by_login,
                mark_manual_raid_started=manual_suppression.mark_manual_raid_started,
                remember_recent_raid_arrival=state_store.remember_recent_raid_arrival,
                logger=log,
            )
        )

    def _raid_data_source_service(self) -> RaidDataSourceService:
        return RaidDataSourceService(
            client_id=self.auth_manager.client_id,
            client_secret=self.auth_manager.client_secret,
            session_getter=lambda: self.session,
            target_game_lower_getter=lambda: (
                str(getattr(self._cog, "_get_target_game_lower")() or "").strip().lower()
                if getattr(self, "_cog", None) is not None
                and callable(getattr(self._cog, "_get_target_game_lower", None))
                else None
            ),
            language_filter_getter=lambda: (
                list(getattr(self._cog, "_language_filter_values")())
                if getattr(self, "_cog", None) is not None
                and callable(getattr(self._cog, "_language_filter_values", None))
                else []
            ),
            shared_stream_fetch=(
                getattr(self._cog, "_fetch_streams_by_logins_quick")
                if getattr(self, "_cog", None) is not None
                and callable(getattr(self._cog, "_fetch_streams_by_logins_quick", None))
                else None
            ),
            cached_category_id_getter=lambda: getattr(self._cog, "_category_id", None)
            if getattr(self, "_cog", None) is not None
            else None,
            readonly_connection_factory=readonly_connection,
            utcnow=lambda: datetime.now(UTC),
            logger=log,
        )

    def _partner_setup_service(self) -> PartnerSetupService:
        return PartnerSetupService(
            auth_manager=self.auth_manager,
            session_getter=lambda: self.session,
            chat_bot_getter=lambda: self.chat_bot,
            bot_id_getter=self._resolve_bot_id_for_setup,
            readonly_connection_factory=readonly_connection,
            transaction_factory=transaction,
            moderator_url_base=TWITCH_API_BASE,
            mask_log_identifier=_mask_log_identifier,
            logger=log,
        )

    def _offline_raid_orchestrator(self) -> OfflineRaidOrchestrator:
        return OfflineRaidOrchestrator(
            create_twitch_api=self._create_twitch_api,
            resolve_manual_raid_source_state=self._resolve_manual_raid_source_state,
            evaluate_deadlock_raid_source=self._evaluate_deadlock_raid_source,
            safe_int=self._safe_int,
            calculate_stream_duration_sec=self._calculate_stream_duration_sec,
            load_partner_roster_for_raid=self._load_partner_roster_for_raid,
            fetch_streams_by_logins_for_raid=self._fetch_streams_by_logins_for_raid,
            build_online_partner_candidates=self._build_online_partner_candidates,
            filter_deadlock_eligible_partner_candidates=self._filter_deadlock_eligible_partner_candidates,
            resolve_target_category_id=self._resolve_target_category_id,
            execute_raid_pipeline=self._execute_raid_pipeline,
            is_offline_auto_raid_suppressed=self.is_offline_auto_raid_suppressed,
            load_offline_auto_raid_eligibility=self._load_offline_auto_raid_eligibility,
            logger=log,
        )

    def _raid_metrics_store(self) -> RaidMetricsStore:
        return RaidMetricsStore(
            readonly_connection=readonly_connection,
            transaction=transaction,
            normalize_broadcaster_login=self._normalize_broadcaster_login,
            is_partner_target_channel=self._is_partner_target_channel,
            next_raid_observability_flow_id=self._next_raid_observability_flow_id,
            logger=log,
        )

    def _candidate_followers_service(self) -> CandidateFollowersService:
        return CandidateFollowersService(
            CandidateFollowersDependencies(
                create_twitch_api=lambda session: self._create_twitch_api(session=session),
                resolve_bot_oauth_context=self._resolve_bot_oauth_context,
                get_followers_total_result=lambda api, user_id, user_token: self._get_followers_total_result_with_legacy_fallback(
                    api,
                    user_id,
                    user_token=user_token,
                ),
                resolve_valid_token=lambda user_id, session: self.auth_manager.get_valid_token(
                    user_id,
                    session,
                ),
                increment_counter=self._increment_raid_observability_counter,
                warn_user_scope_fallback_once=self._warn_user_scope_fallback_once,
                clear_user_scope_fallback_warning=self._clear_user_scope_fallback_warning,
                logger=log,
            ),
            max_concurrency=8,
        )

    def _resolve_bot_id_for_setup(self) -> str | None:
        chat_bot = self.chat_bot
        if chat_bot is not None:
            bot_id = getattr(chat_bot, "bot_id_safe", None)
            if bot_id is None:
                bot_id_raw = getattr(chat_bot, "bot_id", None)
                bot_id = (
                    str(bot_id_raw).strip()
                    if bot_id_raw and str(bot_id_raw).strip()
                    else None
                )
            if bot_id:
                return str(bot_id)

        fallback_bot_id = getattr(self, "_bot_id", None)
        if fallback_bot_id:
            return str(fallback_bot_id).strip() or None

        return os.getenv("TWITCH_BOT_USER_ID", "").strip() or None

    def _load_offline_auto_raid_eligibility(self, broadcaster_id: str) -> Any:
        with readonly_connection() as conn:
            return load_offline_auto_raid_eligibility(
                conn,
                twitch_user_id=broadcaster_id,
            )

    def _candidate_selection_service(self) -> CandidateSelectionService:
        return CandidateSelectionService(
            load_partner_raid_score_map=load_partner_raid_score_map,
            refresh_partner_raid_score_async=refresh_partner_raid_score_async,
            recent_raid_targets_loader=self._get_recent_raid_targets,
            attach_followers_totals=self._attach_followers_totals,
            readonly_connection_factory=readonly_connection,
            logger=log,
            recent_raid_cooldown_days=RAID_TARGET_COOLDOWN_DAYS,
        )

    def _raid_blacklist_service(self) -> RaidBlacklistService:
        return build_runtime_raid_blacklist_service(
            readonly_connection_factory=readonly_connection,
            transaction_factory=transaction,
            is_target_partner=self._is_target_currently_partner,
            get_chat_bot=lambda: self.chat_bot,
            config=RaidBlacklistConfig(
                external_recruitment_raid_limit=_EXTERNAL_RECRUITMENT_RAID_LIMIT,
                external_recruitment_blacklist_grace_seconds=int(
                    _EXTERNAL_RECRUITMENT_BLACKLIST_GRACE_SECONDS
                ),
                external_target_ban_check_delay_seconds=int(
                    _EXTERNAL_BAN_CHECK_DELAY_SECONDS
                ),
            ),
        )

    def _raid_pipeline_service(self) -> RaidPipelineService:
        def _log_event(**payload: object) -> None:
            level = payload.get("level", logging.INFO)
            if isinstance(level, str):
                level = getattr(logging, level.upper(), logging.INFO)
            self._log_raid_observability_event(
                raid_flow_id=str(payload.get("flow_id") or ""),
                step=str(payload.get("step") or "event"),
                decision=str(payload.get("decision") or "unknown"),
                level=level,
                from_broadcaster_login=payload.get("from_broadcaster_login"),
                from_broadcaster_id=payload.get("from_broadcaster_id"),
                to_broadcaster_login=payload.get("to_broadcaster_login"),
                to_broadcaster_id=payload.get("to_broadcaster_id"),
                details=payload.get("details"),
            )

        return RaidPipelineService(
            RaidPipelineDependencies(
                load_raid_blacklist=self._load_raid_blacklist,
                add_to_blacklist=self._add_to_blacklist,
                select_partner_candidate_by_score=self._select_partner_candidate_by_score,
                select_fairest_candidate=self._select_fairest_candidate,
                ensure_raid_arrival_subscription_ready=lambda target_id, target_login, raid_flow_id: self._ensure_raid_arrival_subscription_ready(
                    to_broadcaster_id=target_id,
                    to_broadcaster_login=target_login,
                    raid_flow_id=raid_flow_id,
                ),
                start_raid=self.raid_executor.start_raid,
                register_pending_raid=self._register_pending_raid,
                mark_manual_raid_started=lambda broadcaster_id, ttl_seconds: self.mark_manual_raid_started(
                    broadcaster_id=broadcaster_id,
                    ttl_seconds=ttl_seconds,
                ),
                logger=log,
                next_raid_observability_flow_id=lambda prefix: self._next_raid_observability_flow_id(
                    prefix=prefix
                ),
                increment_raid_observability_counter=self._increment_raid_observability_counter,
                log_raid_observability_event=_log_event,
                to_thread=asyncio.to_thread,
            )
        )

    def _raid_tracking_runtime_service(self) -> RaidTrackingRuntimeService:
        state_store = self._raid_state_store()
        state_store.ensure_runtime_raid_tracking_state()

        def _eventsub_has_sub(cog, sub_type: str, broadcaster_user_id: str) -> bool:
            checker = getattr(cog, "_eventsub_has_sub", None)
            return bool(checker(sub_type, broadcaster_user_id)) if callable(checker) else False

        async def _ensure_raid_target_dynamic_ready(
            cog,
            broadcaster_user_id: str,
            broadcaster_login: str,
            raid_flow_id: str | None,
        ):
            ensure_ready = getattr(cog, "ensure_raid_target_dynamic_ready", None)
            if not callable(ensure_ready):
                return False, None
            return await ensure_ready(
                broadcaster_user_id,
                broadcaster_login,
                raid_flow_id=raid_flow_id,
            )

        async def _subscribe_raid_target_dynamic(
            cog,
            broadcaster_user_id: str,
            broadcaster_login: str,
        ) -> bool:
            subscribe = getattr(cog, "subscribe_raid_target_dynamic", None)
            if not callable(subscribe):
                return False
            return bool(await subscribe(broadcaster_user_id, broadcaster_login))

        return RaidTrackingRuntimeService(
            RaidTrackingRuntimeDependencies(
                state=RaidTrackingRuntimeState(
                    pending_store=state_store.pending_raid_store(),
                    recent_raid_arrivals=getattr(self, "_recent_raid_arrivals", {}),
                    orphan_chat_raid_notifications=getattr(self, "_orphan_chat_raid_notifications", {}),
                    readiness_states=getattr(self, "_raid_readiness_by_flow_id", {}),
                ),
                snapshot_chat_notification_subscription=self._snapshot_chat_notification_subscription,
                get_cog=lambda: self._cog,
                eventsub_has_sub=_eventsub_has_sub,
                ensure_raid_target_dynamic_ready=_ensure_raid_target_dynamic_ready,
                subscribe_raid_target_dynamic=_subscribe_raid_target_dynamic,
                orphan_chat_raid_notification_handler=lambda payload: self.on_chat_raid_notification(
                    to_broadcaster_id=str(payload.get("to_broadcaster_id") or ""),
                    to_broadcaster_login=str(payload.get("to_broadcaster_login") or ""),
                    from_broadcaster_login=str(payload.get("from_broadcaster_login") or ""),
                    viewer_count=int(payload.get("viewer_count") or 0),
                    from_broadcaster_id=str(payload.get("from_broadcaster_id") or "") or None,
                    message_id=str(payload.get("message_id") or "") or None,
                    event_timestamp=str(payload.get("event_timestamp") or "") or None,
                ),
                next_raid_observability_flow_id=lambda prefix: self._next_raid_observability_flow_id(
                    prefix=prefix
                ),
                increment_raid_observability_counter=self._increment_raid_observability_counter,
                log_raid_observability_event=self._log_raid_observability_event,
            )
        )

    def _raid_arrival_runtime(self) -> RaidArrivalRuntime:
        external_recruitment = self._external_recruitment_service()
        arrival_confirmation = self._arrival_confirmation_service()

        def _confirm_pending_raid_arrival_with_overrides(
            *,
            pending_raid: PendingRaid,
            signal_type: str,
            to_broadcaster_id: str,
            to_broadcaster_login: str,
            from_broadcaster_login: str,
            viewer_count: int,
            from_broadcaster_id: str | None = None,
        ):
            classification, source_resolution = self._classify_partner_raid_arrival(
                from_broadcaster_login=from_broadcaster_login,
                from_broadcaster_id=from_broadcaster_id,
                to_broadcaster_id=to_broadcaster_id,
                to_broadcaster_login=to_broadcaster_login,
                expected_partner=bool(pending_raid.is_partner_raid),
            )
            return arrival_confirmation.confirm_pending_raid_arrival(
                pending_raid=pending_raid,
                signal_type=signal_type,
                to_broadcaster_id=to_broadcaster_id,
                to_broadcaster_login=to_broadcaster_login,
                from_broadcaster_login=from_broadcaster_login,
                from_broadcaster_id=from_broadcaster_id,
                viewer_count=viewer_count,
                classification_override=classification,
                source_resolution_override=source_resolution,
                target_is_partner_override=classification is not None,
            )

        return RaidArrivalRuntime(
            RaidArrivalRuntimeDependencies(
                arrival_confirmation_service=type(
                    "_ArrivalConfirmationProxy",
                    (),
                    {"confirm_pending_raid_arrival": staticmethod(_confirm_pending_raid_arrival_with_overrides)},
                )(),
                signal_correlation_service=self._signal_correlation_service(),
                get_pending_raid=lambda to_broadcaster_id, from_broadcaster_login: self._get_pending_raid(
                    to_broadcaster_id=to_broadcaster_id,
                    from_broadcaster_login=from_broadcaster_login,
                ),
                store_pending_raid=self._store_pending_raid,
                pop_pending_raid=lambda to_broadcaster_id, from_broadcaster_login: self._pop_pending_raid(
                    to_broadcaster_id=to_broadcaster_id,
                    from_broadcaster_login=from_broadcaster_login,
                ),
                record_pending_signal_observation=lambda pending, signal_type, status, reason, detail: self._record_pending_signal_observation(
                    pending,
                    signal_type=signal_type,
                    status=status,
                    reason=reason,
                    detail=detail,
                ),
                store_orphan_chat_raid_notification=self._store_orphan_chat_raid_notification,
                lookup_recent_raid_arrival=lambda to_broadcaster_id, from_broadcaster_login: self._lookup_recent_raid_arrival(
                    to_broadcaster_id=to_broadcaster_id,
                    from_broadcaster_login=from_broadcaster_login,
                ),
                remember_recent_raid_arrival=self._remember_recent_raid_arrival,
                update_partner_raid_arrival=lambda arrival_tracking_id, confirmation_signals, unraid_seen: self._update_partner_raid_arrival(
                    arrival_tracking_id=arrival_tracking_id,
                    confirmation_signals=confirmation_signals,
                    unraid_seen=unraid_seen,
                ),
                store_partner_raid_arrival=self._store_partner_raid_arrival,
                load_recent_raid_history_reference=lambda from_broadcaster_login, to_broadcaster_id: self._load_recent_raid_history_reference(
                    from_broadcaster_login=from_broadcaster_login,
                    to_broadcaster_id=to_broadcaster_id,
                ),
                process_independent_partner_raid_arrival=self._process_independent_partner_raid_arrival,
                cancel_pending_raids_for_source_unraid=self._cancel_pending_raids_for_source_unraid,
                resolve_streamer_id_by_login=self._resolve_streamer_id_by_login,
                mark_manual_raid_started=lambda broadcaster_id, ttl_seconds: self.mark_manual_raid_started(
                    broadcaster_id=broadcaster_id,
                    ttl_seconds=ttl_seconds,
                ),
                lookup_silent_raid_enabled=self._lookup_silent_raid_enabled,
                refresh_partner_score_cache_if_available=lambda twitch_user_id, reason: self._refresh_partner_score_cache_if_available(
                    twitch_user_id,
                    reason=reason,
                ),
                track_confirmed_partner_raid=track_confirmed_partner_raid,
                delete_external_recruitment_blacklist_pending=self._delete_external_recruitment_blacklist_pending,
                record_confirmed_external_recruitment_raid=lambda **kwargs: external_recruitment.record_confirmed_raid(
                    **kwargs
                ).persisted_count,
                maybe_schedule_external_recruitment_blacklist_pending=external_recruitment.maybe_schedule_blacklist,
                send_partner_raid_message=self._send_partner_raid_message,
                send_recruitment_message=self._send_recruitment_message_now,
                increment_raid_observability_counter=self._increment_raid_observability_counter,
                log_raid_observability_event=self._log_raid_observability_event,
                next_raid_observability_flow_id=lambda prefix: self._next_raid_observability_flow_id(
                    prefix=prefix
                ),
            )
        )

    def _recruitment_messaging_service(self) -> RecruitmentMessagingService:
        return build_runtime_recruitment_messaging_service(
            create_twitch_api=lambda session: self._create_twitch_api(session=session),
            readonly_connection_factory=readonly_connection,
            resolve_bot_oauth_context=self._resolve_bot_oauth_context,
            resolve_valid_token=lambda twitch_user_id, session: self.auth_manager.get_valid_token(
                twitch_user_id,
                session,
            ),
            get_followers_total_result=lambda api, twitch_user_id, user_token: self._get_followers_total_result_with_legacy_fallback(
                api,
                twitch_user_id,
                user_token=user_token,
            ),
            build_followers_runtime_state=self._build_analytics_followers_runtime_state,
            increment_counter=self._increment_raid_observability_counter,
            log_followers_decision=self._log_analytics_followers_decision,
            next_flow_id=lambda prefix: self._next_raid_observability_flow_id(prefix=prefix),
            warn_user_scope_fallback_once=self._warn_user_scope_fallback_once,
            clear_user_scope_fallback_warning=self._clear_user_scope_fallback_warning,
            get_chat_bot=lambda: self.chat_bot,
            count_confirmed_external_recruitment_raids=self._get_confirmed_external_recruitment_raid_count,
            schedule_external_target_ban_check=self._schedule_external_target_ban_check,
            sleep=asyncio.sleep,
        )

    @staticmethod
    def _signal_correlation_service() -> RaidSignalCorrelationService:
        return RaidSignalCorrelationService()

    def _increment_raid_observability_counter(self, name: str, amount: int = 1) -> int:
        service = self._make_raid_observability_service()
        value = service.increment_counter(name, amount)
        self._raid_observability_sequence = service.sequence
        self._raid_observability_counter_store = service.counter_store
        return value

    @staticmethod
    def _raid_observability_value(value: object, *, limit: int = 240) -> str:
        return RaidObservabilityService.normalize_value(value, limit=limit)

    def _format_raid_observability_fields(self, **fields: object) -> str:
        return self._make_raid_observability_service().format_fields(**fields)

    def _log_raid_observability_event(
        self,
        *,
        raid_flow_id: str,
        step: str,
        decision: str,
        level: int = logging.INFO,
        from_broadcaster_login: str | None = None,
        from_broadcaster_id: str | None = None,
        to_broadcaster_login: str | None = None,
        to_broadcaster_id: str | None = None,
        details: dict[str, object] | None = None,
    ) -> None:
        service = self._make_raid_observability_service()
        event = service.emit_event(
            flow_type="raid",
            flow_id=str(raid_flow_id or "").strip(),
            step=step,
            decision=decision,
            from_broadcaster_login=from_broadcaster_login,
            from_broadcaster_id=from_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            to_broadcaster_id=to_broadcaster_id,
            details=details or {},
        )
        payload = event.as_log_fields()
        self._last_raid_observability_event = payload
        self._raid_observability_sequence = service.sequence
        self._raid_observability_counter_store = service.counter_store
        log.log(level, "raid_flow %s", service.format_fields(**payload))

    def get_observability_snapshot(self) -> dict[str, Any]:
        state_store = self._raid_state_store()
        state_store.ensure_runtime_raid_tracking_state()
        state_store.cleanup_stale_raid_readiness_states()
        pending_raids = getattr(self, "_pending_raids", {}) or {}
        recent_arrivals = getattr(self, "_recent_raid_arrivals", {}) or {}
        orphan_notifications = getattr(self, "_orphan_chat_raid_notifications", {}) or {}
        readiness_by_flow = getattr(self, "_raid_readiness_by_flow_id", {}) or {}
        return {
            "pendingCount": len(pending_raids),
            "pendingTargets": sorted(
                self._format_pending_raid_key_for_log(key)
                for key in list(pending_raids.keys())[:10]
            ),
            "recentArrivalCount": len(recent_arrivals),
            "orphanChatNotificationCount": len(orphan_notifications),
            "readinessFlowCount": len(readiness_by_flow),
            "counters": dict(self._raid_observability_counters()),
            "lastEvent": getattr(self, "_last_raid_observability_event", None),
            "lastAnalyticsFollowersDiagnostic": getattr(
                self,
                "_last_analytics_followers_diagnostic",
                None,
            ),
        }

    def _build_analytics_followers_runtime_state(self) -> dict[str, object]:
        chat_bot = getattr(self, "chat_bot", None)
        token_mgr = getattr(chat_bot, "_token_manager", None) if chat_bot is not None else None
        return {
            "chat_bot_available": bool(chat_bot),
            "bot_token_manager_available": bool(token_mgr),
            "raid_session_available": bool(self.session),
        }

    def _log_analytics_followers_decision(
        self,
        *,
        flow_id: str,
        flow: str,
        login: str,
        target_id: str | None,
        decision: str,
        reason: str,
        request_attempted: object,
        request_result: str,
        http_status: int | None,
        scope_state: dict[str, object],
        runtime_state: dict[str, object],
        level: int = logging.INFO,
        **extra_fields: object,
    ) -> None:
        payload = {
            "flow_id": str(flow_id or "").strip() or None,
            "flow": str(flow or "").strip().lower() or "followers",
            "login": str(login or "").strip().lower() or None,
            "target_id": str(target_id or "").strip() or None,
            "decision": str(decision or "").strip() or "unknown",
            "reason": str(reason or "").strip() or "unknown",
            "request_attempted": request_attempted,
            "request_result": str(request_result or "").strip() or "unknown",
            "http_status": int(http_status) if http_status is not None else None,
            "scope_state": scope_state,
            "runtime_state": runtime_state,
            **extra_fields,
        }
        self._last_analytics_followers_diagnostic = payload
        log.log(level, "analytics_decision %s", self._format_raid_observability_fields(**payload))
        insert_observability_event(
            flow_type="analytics",
            flow_id=str(payload.get("flow_id") or ""),
            entity_login=str(payload.get("login") or ""),
            entity_id=str(payload.get("target_id") or ""),
            step="terminal_decision",
            decision=str(payload.get("decision") or "unknown"),
            details=payload,
        )

    def _ensure_runtime_raid_tracking_state(self) -> None:
        self._raid_state_store().ensure_runtime_raid_tracking_state()

    def _cleanup_stale_raid_readiness_states(self) -> None:
        self._raid_state_store().cleanup_stale_raid_readiness_states()

    @staticmethod
    def _format_pending_raid_key_for_log(key: object) -> str:
        return RaidStateStore.format_pending_raid_key_for_log(key)

    def _build_pending_raid_storage_key(
        self,
        *,
        to_broadcaster_id: str,
        from_broadcaster_login: str,
    ) -> tuple[str, str]:
        return self._raid_state_store().build_pending_raid_storage_key(
            to_broadcaster_id=to_broadcaster_id,
            from_broadcaster_login=from_broadcaster_login,
        )

    def _pending_raid_store(self) -> PendingRaidStore:
        return self._raid_state_store().pending_raid_store()

    def _store_pending_raid(
        self,
        pending_record: PendingRaid | Mapping[str, Any] | tuple[Any, ...],
    ) -> PendingRaid | None:
        return self._raid_state_store().store_pending_raid(pending_record)

    def _get_pending_raid(
        self,
        *,
        to_broadcaster_id: str,
        from_broadcaster_login: str | None = None,
    ) -> PendingRaid | None:
        return self._raid_state_store().get_pending_raid(
            to_broadcaster_id=to_broadcaster_id,
            from_broadcaster_login=from_broadcaster_login,
        )

    def _pop_pending_raid(
        self,
        *,
        to_broadcaster_id: str,
        from_broadcaster_login: str | None = None,
    ) -> PendingRaid | None:
        return self._raid_state_store().pop_pending_raid(
            to_broadcaster_id=to_broadcaster_id,
            from_broadcaster_login=from_broadcaster_login,
        )

    def _coerce_pending_raid_record(
        self,
        pending: PendingRaid | Mapping[str, Any] | tuple[Any, ...] | None,
        *,
        to_broadcaster_id: str | None = None,
    ) -> PendingRaid | None:
        return self._raid_state_store().coerce_pending_raid_record(
            pending,
            to_broadcaster_id=to_broadcaster_id,
        )

    def _build_pending_raid_record(
        self,
        *,
        from_broadcaster_login: str,
        to_broadcaster_id: str,
        target_stream_data: dict | None,
        is_partner_raid: bool,
        viewer_count: int,
        offline_trigger_ts: float | None,
        raid_flow_id: str | None = None,
        channel_raid_ready: bool | None = None,
        channel_raid_ready_detail: str | None = None,
        chat_notification_state: str | None = None,
        chat_notification_detail: str | None = None,
    ) -> PendingRaid:
        return self._raid_tracking_runtime_service()._build_pending_raid_record(
            from_broadcaster_login=from_broadcaster_login,
            to_broadcaster_id=to_broadcaster_id,
            target_stream_data=target_stream_data,
            is_partner_raid=is_partner_raid,
            viewer_count=viewer_count,
            offline_trigger_ts=offline_trigger_ts,
            raid_flow_id=raid_flow_id,
            channel_raid_ready=channel_raid_ready,
            channel_raid_ready_detail=channel_raid_ready_detail,
            chat_notification_state=chat_notification_state,
            chat_notification_detail=chat_notification_detail,
        )

    @staticmethod
    def _record_pending_signal_observation(
        pending_record: PendingRaid,
        *,
        signal_type: str,
        status: str,
        reason: str | None = None,
        detail: str | None = None,
    ) -> None:
        pending_record.record_signal_observation(
            signal_type=signal_type,
            status=status,
            reason=reason,
            detail=detail,
        )

    def _snapshot_chat_notification_subscription(
        self,
        broadcaster_login: str,
    ) -> tuple[str | None, str | None]:
        chat_bot = getattr(self, "chat_bot", None)
        if chat_bot is None:
            return "no_chat_bot", "chat bot unavailable"

        get_state = getattr(chat_bot, "get_channel_subscription_state", None)
        if callable(get_state):
            try:
                state = get_state(broadcaster_login)
            except Exception:
                log.debug(
                    "Could not resolve channel.chat.notification subscription state for %s",
                    broadcaster_login,
                    exc_info=True,
                )
            else:
                notification_state = state.get("channel.chat.notification")
                if isinstance(notification_state, dict):
                    return (
                        str(notification_state.get("state") or "").strip() or None,
                        str(notification_state.get("detail") or "").strip() or None,
                    )

        is_ready = getattr(chat_bot, "is_channel_subscription_ready", None)
        if callable(is_ready):
            try:
                if is_ready(broadcaster_login, "channel.chat.notification"):
                    return "subscribed", None
            except Exception:
                log.debug(
                    "Could not check channel.chat.notification readiness for %s",
                    broadcaster_login,
                    exc_info=True,
                )

        return "not_joined", "channel.chat.notification not subscribed"

    def _lookup_recent_raid_arrival(
        self,
        *,
        to_broadcaster_id: str,
        from_broadcaster_login: str,
    ) -> dict[str, Any] | None:
        return self._raid_state_store().lookup_recent_raid_arrival(
            to_broadcaster_id=to_broadcaster_id,
            from_broadcaster_login=from_broadcaster_login,
        )

    def _remember_recent_raid_arrival(
        self,
        *,
        to_broadcaster_id: str,
        from_broadcaster_login: str,
        from_broadcaster_id: str | None,
        to_broadcaster_login: str,
        viewer_count: int,
        classification: str | None,
        confirmation_signals: set[str],
        arrival_tracking_id: int | None,
        raid_flow_id: str | None = None,
    ) -> None:
        self._raid_state_store().remember_recent_raid_arrival(
            to_broadcaster_id=to_broadcaster_id,
            from_broadcaster_login=from_broadcaster_login,
            from_broadcaster_id=from_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            viewer_count=viewer_count,
            classification=classification,
            confirmation_signals=confirmation_signals,
            arrival_tracking_id=arrival_tracking_id,
            raid_flow_id=raid_flow_id,
        )

    def _cleanup_recent_raid_arrivals(self) -> None:
        self._raid_state_store().cleanup_recent_raid_arrivals()

    def _store_orphan_chat_raid_notification(self, payload: dict[str, Any]) -> None:
        self._raid_state_store().store_orphan_chat_raid_notification(payload)

    def _pop_orphan_chat_raid_notification(
        self,
        *,
        to_broadcaster_id: str,
        from_broadcaster_login: str,
    ) -> dict[str, Any] | None:
        return self._raid_state_store().pop_orphan_chat_raid_notification(
            to_broadcaster_id=to_broadcaster_id,
            from_broadcaster_login=from_broadcaster_login,
        )

    def _promote_stale_orphan_chat_raid_notifications(self) -> None:
        self._raid_state_store().promote_stale_orphan_chat_raid_notifications(
            process_independent_partner_raid_arrival=self._process_independent_partner_raid_arrival
        )

    def _resolve_known_streamer_identity(
        self,
        *,
        broadcaster_login: str,
        broadcaster_id: str | None = None,
    ) -> dict[str, str] | None:
        return self._partner_arrival_tracking_service().resolve_known_streamer_identity(
            broadcaster_login=broadcaster_login,
            broadcaster_id=broadcaster_id,
        )

    def _is_partner_target_channel(
        self,
        *,
        broadcaster_id: str,
        broadcaster_login: str,
        expected_partner: bool = False,
    ) -> bool:
        return self._partner_arrival_tracking_service().is_partner_target_channel(
            broadcaster_id=broadcaster_id,
            broadcaster_login=broadcaster_login,
            expected_partner=expected_partner,
        )

    def _lookup_partner_target_channel(
        self,
        *,
        broadcaster_id: str,
        broadcaster_login: str,
    ) -> Any:
        return self._partner_arrival_tracking_service().lookup_partner_target_channel(
            broadcaster_id=broadcaster_id,
            broadcaster_login=broadcaster_login,
        )

    def _classify_partner_raid_arrival(
        self,
        *,
        from_broadcaster_login: str,
        from_broadcaster_id: str | None,
        to_broadcaster_id: str,
        to_broadcaster_login: str,
        expected_partner: bool = False,
    ) -> tuple[str | None, str]:
        return self._partner_arrival_tracking_service().classify_partner_raid_arrival(
            from_broadcaster_login=from_broadcaster_login,
            from_broadcaster_id=from_broadcaster_id,
            to_broadcaster_id=to_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            expected_partner=expected_partner,
        )

    def _load_recent_raid_history_reference(
        self,
        *,
        from_broadcaster_login: str,
        to_broadcaster_id: str,
    ) -> tuple[int | None, str | None]:
        return self._partner_arrival_tracking_service().load_recent_raid_history_reference(
            from_broadcaster_login=from_broadcaster_login,
            to_broadcaster_id=to_broadcaster_id,
        )

    def _store_partner_raid_arrival(
        self,
        *,
        from_broadcaster_id: str | None,
        from_broadcaster_login: str,
        to_broadcaster_id: str,
        to_broadcaster_login: str,
        viewer_count: int,
        classification: str,
        confirmation_signals: set[str],
        primary_signal: str,
        correlation_status: str,
        correlation_detail: str | None = None,
        source_resolution: str,
        raid_history_id: int | None = None,
        raid_history_executed_at: str | None = None,
        unraid_seen: bool = False,
    ) -> int | None:
        return self._partner_arrival_tracking_service().store_partner_raid_arrival(
            from_broadcaster_id=from_broadcaster_id,
            from_broadcaster_login=from_broadcaster_login,
            to_broadcaster_id=to_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            viewer_count=viewer_count,
            classification=classification,
            confirmation_signals=confirmation_signals,
            primary_signal=primary_signal,
            correlation_status=correlation_status,
            correlation_detail=correlation_detail,
            source_resolution=source_resolution,
            raid_history_id=raid_history_id,
            raid_history_executed_at=raid_history_executed_at,
            unraid_seen=unraid_seen,
        )

    def _update_partner_raid_arrival(
        self,
        *,
        arrival_tracking_id: int,
        confirmation_signals: set[str],
        unraid_seen: bool = False,
    ) -> None:
        self._partner_arrival_tracking_service().update_partner_raid_arrival(
            arrival_tracking_id=arrival_tracking_id,
            confirmation_signals=confirmation_signals,
            unraid_seen=unraid_seen,
        )

    def _process_independent_partner_raid_arrival(
        self,
        *,
        to_broadcaster_id: str,
        to_broadcaster_login: str,
        from_broadcaster_login: str,
        from_broadcaster_id: str | None,
        viewer_count: int,
        signal_type: str,
        correlation_status: str,
        correlation_detail: str | None = None,
    ) -> bool:
        result = self._partner_arrival_tracking_service().process_independent_partner_raid_arrival_result(
            to_broadcaster_id=to_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            from_broadcaster_login=from_broadcaster_login,
            from_broadcaster_id=from_broadcaster_id,
            viewer_count=viewer_count,
            signal_type=signal_type,
            correlation_status=correlation_status,
            correlation_detail=correlation_detail,
        )
        if result.processed:
            log.info(
                "Partner raid arrival classified: %s -> %s (%s via %s)",
                from_broadcaster_login,
                to_broadcaster_login,
                result.classification,
                signal_type,
            )
        return result.processed

    async def _resolve_bot_oauth_context(self) -> tuple[str | None, str | None, set[str]]:
        """Resolve bot OAuth token + bot id + scopes (best-effort).

        This enables moderator-scoped reads/actions to be executed via the central bot token,
        while keeping broadcaster tokens as a fallback during migration.
        """
        token_mgr = None
        chat_bot = getattr(self, "chat_bot", None)
        if chat_bot is not None:
            token_mgr = getattr(chat_bot, "_token_manager", None)
        if token_mgr is None:
            cog = getattr(self, "_cog", None)
            token_mgr = getattr(cog, "_bot_token_manager", None) if cog is not None else None
        if token_mgr is None:
            return None, None, set()

        try:
            token, bot_id = await token_mgr.get_valid_token()
        except Exception:
            return None, None, set()

        token = str(token or "").strip()
        if token.lower().startswith("oauth:"):
            token = token[6:]
        resolved_bot_id = str(bot_id or getattr(token_mgr, "bot_id", "") or "").strip() or None
        scopes = {
            str(scope).strip().lower()
            for scope in (getattr(token_mgr, "scopes", None) or set())
            if str(scope).strip()
        }
        return token or None, resolved_bot_id, scopes

    def _warn_user_scope_fallback_once(
        self,
        *,
        area: str,
        subject: str,
    ) -> None:
        subject_key = str(subject or "").strip().lower() or "<unknown>"
        key = (str(area or "").strip().lower(), subject_key)
        if key in self._user_scope_fallback_warned:
            return
        self._user_scope_fallback_warned.add(key)
        log.warning(
            "RaidBot: nutze Legacy-Broadcaster-Token fuer %s (%s). "
            "Der Bot-Token sollte diesen Pfad uebernehmen.",
            area,
            subject or "<unknown>",
        )

    def _clear_user_scope_fallback_warning(
        self,
        *,
        area: str,
        subject: str,
    ) -> None:
        subject_key = str(subject or "").strip().lower() or "<unknown>"
        key = (str(area or "").strip().lower(), subject_key)
        self._user_scope_fallback_warned.discard(key)

    @staticmethod
    async def _get_followers_total_result_with_legacy_fallback(
        api,
        user_id: str,
        *,
        user_token: str | None = None,
    ) -> dict[str, object]:
        result_getter = getattr(api, "get_followers_total_result", None)
        if callable(result_getter):
            return await result_getter(user_id, user_token=user_token)
        legacy_total = await api.get_followers_total(user_id, user_token=user_token)
        return {
            "ok": legacy_total is not None,
            "data": legacy_total,
            "http_status": 200 if legacy_total is not None else None,
            "error_code": None if legacy_total is not None else "legacy_none_result",
            "request_attempted": True,
        }

    @staticmethod
    def _row_value(row, key: str, index: int, default=None):
        if row is None:
            return default
        try:
            if hasattr(row, "keys"):
                return row[key]
            return row[index]
        except Exception:
            return default

    @staticmethod
    def _safe_int(value: object, default: int = 0) -> int:
        try:
            if value is None:
                return default
            return int(value)
        except (TypeError, ValueError):
            return default

    @staticmethod
    def _parse_datetime(value: object) -> datetime | None:
        if value is None:
            return None
        text = str(value).strip()
        if not text:
            return None
        try:
            parsed = datetime.fromisoformat(text.replace("Z", "+00:00"))
        except Exception:
            return None
        if parsed.tzinfo is None:
            parsed = parsed.replace(tzinfo=UTC)
        return parsed.astimezone(UTC)

    def _get_target_game_lower(self) -> str:
        return str(TWITCH_TARGET_GAME_NAME or "").strip().lower()

    def _is_recent_deadlock(
        self,
        last_deadlock_seen_at: str | None,
        *,
        now_utc: datetime | None = None,
        recency_cap_seconds: int = 360,
    ) -> bool:
        return self._raid_data_source_service().is_recent_deadlock(
            last_deadlock_seen_at,
            now_utc=now_utc,
            recency_cap_seconds=recency_cap_seconds,
        )

    def _evaluate_deadlock_raid_source(
        self,
        *,
        current_game: str,
        had_deadlock_session: bool,
        last_deadlock_seen_at: str | None,
    ) -> dict[str, object]:
        return self._raid_data_source_service().evaluate_deadlock_raid_source(
            current_game=current_game,
            had_deadlock_session=had_deadlock_session,
            last_deadlock_seen_at=last_deadlock_seen_at,
        )

    def _is_deadlock_raid_source_eligible(
        self,
        *,
        last_game: str,
        had_deadlock_session: bool,
        last_deadlock_seen_at: str | None,
    ) -> bool:
        return self._raid_data_source_service().is_deadlock_raid_source_eligible(
            last_game=last_game,
            had_deadlock_session=had_deadlock_session,
            last_deadlock_seen_at=last_deadlock_seen_at,
        )

    def _is_deadlock_partner_candidate_eligible(
        self,
        *,
        game_name: str,
        had_deadlock_session: bool,
        last_deadlock_seen_at: str | None,
    ) -> bool:
        return self._raid_data_source_service().is_deadlock_partner_candidate_eligible(
            game_name=game_name,
            had_deadlock_session=had_deadlock_session,
            last_deadlock_seen_at=last_deadlock_seen_at,
        )

    def _load_partner_roster_for_raid(self, source_user_id: str) -> list[dict[str, object]]:
        return self._raid_data_source_service().load_partner_roster_for_raid(source_user_id)

    def _build_online_partner_candidates(
        self,
        partner_rows: list[dict[str, object]],
        streams_by_login: dict[str, dict],
    ) -> list[dict]:
        return self._raid_data_source_service().build_online_partner_candidates(
            partner_rows,
            streams_by_login,
        )

    def _load_partner_live_state_map(
        self,
        partner_logins_lower: list[str],
    ) -> dict[str, dict[str, object]]:
        return self._raid_data_source_service().load_partner_live_state_map(
            partner_logins_lower
        )

    def _filter_deadlock_eligible_partner_candidates(
        self,
        online_partners: list[dict],
    ) -> tuple[list[dict], list[str]]:
        return self._raid_data_source_service().filter_deadlock_eligible_partner_candidates(
            online_partners
        )

    def _load_broadcaster_live_state(self, broadcaster_id: str) -> dict[str, object]:
        return self._raid_data_source_service().load_broadcaster_live_state(broadcaster_id)

    def _calculate_stream_duration_sec(self, started_at: str | None) -> int:
        return self._raid_data_source_service().calculate_stream_duration_sec(started_at)

    def _raid_language_filters(self) -> list[str | None]:
        return self._raid_data_source_service().raid_language_filters()

    def _create_twitch_api(self, *, session=None):
        session = session or self.session
        if session is None:
            return None
        try:
            from ..api.twitch_api import TwitchAPI
        except Exception:
            return None
        return TwitchAPI(
            self.auth_manager.client_id,
            self.auth_manager.client_secret,
            session=session,
        )

    async def _fetch_streams_by_logins_for_raid(
        self,
        logins: list[str],
        *,
        api=None,
    ) -> dict[str, dict]:
        return await self._raid_data_source_service().fetch_streams_by_logins_for_raid(
            logins,
            api=api,
        )

    def _overlay_broadcaster_live_state_from_stream(
        self,
        live_state: dict[str, object],
        stream_data: dict[str, object],
    ) -> dict[str, object]:
        return self._raid_data_source_service().overlay_broadcaster_live_state_from_stream(
            live_state,
            stream_data,
        )

    async def _resolve_manual_raid_source_state(
        self,
        *,
        broadcaster_id: str,
        broadcaster_login: str,
        api=None,
    ) -> dict[str, object]:
        return await self._raid_data_source_service().resolve_manual_raid_source_state(
            broadcaster_id=broadcaster_id,
            broadcaster_login=broadcaster_login,
            api=api,
        )

    async def _resolve_target_category_id(self, api=None) -> str | None:
        return await self._raid_data_source_service().resolve_target_category_id(api)

    def mark_manual_raid_started(self, broadcaster_id: str, ttl_seconds: float = 300.0) -> None:
        """Unterdrückt den nächsten Offline-Auto-Raid für einen Streamer (z.B. nach !raid/!traid)."""
        self._manual_raid_suppression_service().mark_manual_raid_started(
            broadcaster_id=broadcaster_id,
            ttl_seconds=ttl_seconds,
        )

    def is_offline_auto_raid_suppressed(self, broadcaster_id: str) -> bool:
        """True, wenn für den Streamer aktuell eine manuelle-Raid-Sperre aktiv ist."""
        return self._manual_raid_suppression_service().is_offline_auto_raid_suppressed(
            broadcaster_id
        )

    def _resolve_streamer_id_by_login(self, broadcaster_login: str) -> str | None:
        """Best-effort: löst eine Twitch-User-ID aus twitch_streamers über den Login auf."""
        return self._manual_raid_suppression_service().resolve_streamer_id_by_login(
            broadcaster_login
        )

    def _cleanup_expired_manual_raid_suppressions(self) -> None:
        """Entfernt abgelaufene Einträge aus dem Manual-Raid-Suppression-Cache."""
        self._manual_raid_suppression_service().cleanup_expired_manual_raid_suppressions()

    @staticmethod
    def _normalize_discord_user_id(raw: str | None) -> str | None:
        return PartnerSetupService.normalize_discord_user_id(raw)

    async def _resolve_discord_display_name(self, discord_user_id: str | None) -> str | None:
        return await self._partner_setup_service().resolve_discord_display_name(
            discord_user_id
        )

    async def _apply_streamer_role(
        self,
        discord_user_id: str | None,
        *,
        should_have_role: bool,
        reason: str,
    ) -> None:
        await self._partner_setup_service().apply_streamer_role(
            discord_user_id,
            should_have_role=should_have_role,
            reason=reason,
        )

    async def _sync_partner_state_after_auth(
        self,
        twitch_user_id: str,
        twitch_login: str,
        *,
        state_discord_user_id: str | None = None,
        activate_partner_features: bool = True,
    ) -> str | None:
        return await self._partner_setup_service().sync_partner_state_after_auth(
            twitch_user_id,
            twitch_login,
            state_discord_user_id=state_discord_user_id,
            activate_partner_features=activate_partner_features,
        )

    async def complete_setup_for_streamer(
        self,
        twitch_user_id: str,
        twitch_login: str,
        state_discord_user_id: str | None = None,
        activate_partner_features: bool = True,
    ):
        await self._partner_setup_service().complete_setup_for_streamer(
            twitch_user_id,
            twitch_login,
            state_discord_user_id=state_discord_user_id,
            activate_partner_features=activate_partner_features,
        )

    def _cleanup_stale_pending_raids(self):
        self._raid_tracking_runtime_service().cleanup_stale_pending_raids()

    def _clear_superseded_pending_raids(
        self,
        *,
        from_broadcaster_login: str,
        current_target_id: str,
    ) -> None:
        self._raid_tracking_runtime_service().clear_superseded_pending_raids(
            from_broadcaster_login=from_broadcaster_login,
            current_target_id=current_target_id,
        )

    def _cancel_pending_raids_for_source_unraid(
        self,
        *,
        from_broadcaster_login: str,
        from_broadcaster_id: str | None = None,
        message_id: str | None = None,
        event_timestamp: str | None = None,
    ) -> int:
        return self._raid_tracking_runtime_service().cancel_pending_raids_for_source_unraid(
            from_broadcaster_login=from_broadcaster_login,
            from_broadcaster_id=from_broadcaster_id,
            message_id=message_id,
            event_timestamp=event_timestamp,
        )

    async def _ensure_raid_arrival_subscription_ready(
        self,
        *,
        to_broadcaster_id: str,
        to_broadcaster_login: str,
        raid_flow_id: str | None = None,
    ) -> bool:
        self._ensure_runtime_raid_tracking_state()
        return await self._raid_tracking_runtime_service().ensure_raid_arrival_subscription_ready(
            to_broadcaster_id=to_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            raid_flow_id=raid_flow_id,
        )

    async def _register_pending_raid(
        self,
        from_broadcaster_login: str,
        to_broadcaster_id: str,
        to_broadcaster_login: str,
        target_stream_data: dict | None = None,
        is_partner_raid: bool = False,
        viewer_count: int = 0,
        offline_trigger_ts: float | None = None,
        raid_flow_id: str | None = None,
        channel_raid_ready: bool | None = None,
    ):
        self._ensure_runtime_raid_tracking_state()
        await self._raid_tracking_runtime_service().register_pending_raid(
            from_broadcaster_login=from_broadcaster_login,
            to_broadcaster_id=to_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            target_stream_data=target_stream_data,
            is_partner_raid=is_partner_raid,
            viewer_count=viewer_count,
            offline_trigger_ts=offline_trigger_ts,
            raid_flow_id=raid_flow_id,
            channel_raid_ready=channel_raid_ready,
        )

    def _build_pending_timeout_detail(self, pending_record: PendingRaid) -> str:
        return self._raid_tracking_runtime_service().build_pending_timeout_detail(
            pending_record
        )

    def _lookup_silent_raid_enabled(self, broadcaster_login: str) -> bool:
        try:
            with readonly_connection() as conn:
                partner_row = load_active_partner(
                    conn,
                    twitch_login=self._normalize_broadcaster_login(broadcaster_login),
                )
                return bool(
                    int(
                        (
                            partner_row["silent_raid"]
                            if partner_row and hasattr(partner_row, "keys")
                            else (partner_row[15] if partner_row else 0)
                        )
                        or 0
                    )
                )
        except Exception:
            log.debug(
                "Raid arrival: silent_raid lookup failed for %s",
                broadcaster_login,
                exc_info=True,
            )
            return False

    def _handle_secondary_confirmed_signal(
        self,
        *,
        signal_type: str,
        to_broadcaster_id: str,
        to_broadcaster_login: str,
        from_broadcaster_login: str,
        viewer_count: int,
        unraid_seen: bool = False,
    ) -> bool:
        return self._raid_arrival_runtime()._handle_secondary_confirmed_signal(
            signal_type=signal_type,
            to_broadcaster_id=to_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            from_broadcaster_login=from_broadcaster_login,
            viewer_count=viewer_count,
            unraid_seen=unraid_seen,
        )

    async def _execute_signal_plan_actions(self, actions: tuple[object, ...]) -> None:
        await self._raid_arrival_runtime()._execute_signal_plan_actions(actions)

    async def _confirm_pending_raid_arrival(
        self,
        *,
        signal_type: str,
        to_broadcaster_id: str,
        to_broadcaster_login: str,
        from_broadcaster_login: str,
        viewer_count: int,
        from_broadcaster_id: str | None = None,
    ) -> None:
        await self._raid_arrival_runtime().confirm_pending_raid_arrival(
            signal_type=signal_type,
            to_broadcaster_id=to_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            from_broadcaster_login=from_broadcaster_login,
            viewer_count=viewer_count,
            from_broadcaster_id=from_broadcaster_id,
        )

    async def on_raid_arrival(
        self,
        to_broadcaster_id: str,
        to_broadcaster_login: str,
        from_broadcaster_login: str,
        viewer_count: int,
        from_broadcaster_id: str | None = None,
    ):
        await self._raid_arrival_runtime().on_raid_arrival(
            to_broadcaster_id=to_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            from_broadcaster_login=from_broadcaster_login,
            viewer_count=viewer_count,
            from_broadcaster_id=from_broadcaster_id,
        )

    async def on_chat_raid_notification(
        self,
        *,
        to_broadcaster_id: str,
        to_broadcaster_login: str,
        from_broadcaster_login: str,
        viewer_count: int,
        from_broadcaster_id: str | None = None,
        message_id: str | None = None,
        event_timestamp: str | None = None,
    ) -> None:
        await self._raid_arrival_runtime().on_chat_raid_notification(
            to_broadcaster_id=to_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            from_broadcaster_login=from_broadcaster_login,
            viewer_count=viewer_count,
            from_broadcaster_id=from_broadcaster_id,
            message_id=message_id,
            event_timestamp=event_timestamp,
        )

    async def on_chat_unraid_notification(
        self,
        *,
        to_broadcaster_id: str,
        to_broadcaster_login: str,
        from_broadcaster_login: str,
        from_broadcaster_id: str | None = None,
        event_timestamp: str | None = None,
    ) -> None:
        await self._raid_arrival_runtime().on_chat_unraid_notification(
            to_broadcaster_id=to_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            from_broadcaster_login=from_broadcaster_login,
            from_broadcaster_id=from_broadcaster_id,
            event_timestamp=event_timestamp,
        )

    async def on_source_self_unraid_notification(
        self,
        *,
        broadcaster_id: str,
        broadcaster_login: str,
        message_id: str | None = None,
        event_timestamp: str | None = None,
    ) -> None:
        await self._raid_arrival_runtime().on_source_self_unraid_notification(
            broadcaster_id=broadcaster_id,
            broadcaster_login=broadcaster_login,
            message_id=message_id,
            event_timestamp=event_timestamp,
        )

    async def _send_partner_raid_message(
        self,
        from_broadcaster_login: str,
        to_broadcaster_login: str,
        to_broadcaster_id: str,
        viewer_count: int,
    ):
        await self._partner_raid_delivery_service().send_partner_raid_message(
            from_broadcaster_login=from_broadcaster_login,
            to_broadcaster_login=to_broadcaster_login,
            to_broadcaster_id=to_broadcaster_id,
            viewer_count=viewer_count,
        )

    def _get_received_network_raid_count(self, to_broadcaster_id: str) -> int:
        return self._raid_metrics_store().get_received_network_raid_count(to_broadcaster_id)

    def _get_confirmed_external_recruitment_raid_count(self, to_broadcaster_id: str) -> int:
        return self._raid_metrics_store().get_confirmed_external_recruitment_raid_count(
            to_broadcaster_id
        )

    def _record_confirmed_external_recruitment_raid(
        self,
        *,
        raid_flow_id: str | None,
        from_broadcaster_id: str | None,
        from_broadcaster_login: str,
        to_broadcaster_id: str,
        to_broadcaster_login: str,
        viewer_count: int,
        confirmation_signal: str,
    ) -> int | None:
        return self._raid_metrics_store().record_confirmed_external_recruitment_raid(
            raid_flow_id=raid_flow_id,
            from_broadcaster_id=from_broadcaster_id,
            from_broadcaster_login=from_broadcaster_login,
            to_broadcaster_id=to_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            viewer_count=viewer_count,
            confirmation_signal=confirmation_signal,
        )

    def _is_target_currently_partner(
        self,
        *,
        target_id: str,
        target_login: str,
    ) -> bool:
        return self._raid_metrics_store().is_target_currently_partner(
            target_id=target_id,
            target_login=target_login,
        )

    def _schedule_external_recruitment_blacklist_pending(
        self,
        *,
        target_id: str,
        target_login: str,
        confirmed_raid_count: int,
        raid_flow_id: str | None,
    ) -> None:
        self._raid_blacklist_service().schedule_external_recruitment_blacklist_pending(
            target_id=target_id,
            target_login=target_login,
            confirmed_raid_count=confirmed_raid_count,
            raid_flow_id=raid_flow_id,
        )

    def _delete_external_recruitment_blacklist_pending(self, target_id: str) -> None:
        self._raid_blacklist_service().delete_external_recruitment_blacklist_pending(
            target_id
        )

    def _process_due_external_recruitment_blacklist_pending(self) -> None:
        self._raid_blacklist_service().process_due_external_recruitment_blacklist_pending()

    def _schedule_external_target_ban_check(
        self,
        *,
        target_id: str | None,
        target_login: str,
        source: str,
    ) -> None:
        self._raid_blacklist_service().schedule_external_target_ban_check(
            target_id=target_id,
            target_login=target_login,
            source=source,
        )

    def _delete_external_target_ban_check_pending(self, target_id: str) -> None:
        self._raid_blacklist_service().delete_external_target_ban_check_pending(target_id)

    def _reschedule_external_target_ban_check_pending(self, target_id: str, delay_seconds: int = 900) -> None:
        self._raid_blacklist_service().reschedule_external_target_ban_check_pending(
            target_id,
            delay_seconds=delay_seconds,
        )

    async def _process_due_external_target_ban_checks(self) -> None:
        await self._raid_blacklist_service().process_due_external_target_ban_checks()

    @staticmethod
    def _parse_nonnegative_int(value: object) -> int | None:
        return RecruitmentMessagingService.parse_nonnegative_int(value)

    async def _resolve_recruitment_followers_total(
        self,
        *,
        login: str,
        target_id: str | None,
        target_stream_data: dict | None,
    ) -> int | None:
        return await self._recruitment_messaging_service().resolve_recruitment_followers_total(
            login=login,
            target_id=target_id,
            target_stream_data=target_stream_data,
            session=self.session,
        )

    async def _send_recruitment_message_now(
        self,
        from_broadcaster_login: str,
        to_broadcaster_login: str,
        target_stream_data: dict | None = None,
        confirmed_external_raid_count: int | None = None,
    ):
        await self._recruitment_messaging_service().send_recruitment_message_now(
            from_broadcaster_login=from_broadcaster_login,
            to_broadcaster_login=to_broadcaster_login,
            target_stream_data=target_stream_data,
            confirmed_external_raid_count=confirmed_external_raid_count,
            session=getattr(self, "_session", None),
            chat_bot=self.chat_bot,
        )

    @staticmethod
    def _make_chat_target(login: str, user_id: str):
        return make_chat_target(login, user_id)

    def _lookup_outbound_chat_suppression(
        self,
        target_login: str,
        target_id: str | None,
        *,
        source: str,
    ) -> dict | None:
        return lookup_outbound_chat_suppression(
            self.chat_bot,
            target_login=target_login,
            target_id=target_id,
            source=source,
        )

    def _get_recent_raid_targets(self, from_broadcaster_id: str, days: int) -> set[str]:
        return self._raid_metrics_store().get_recent_raid_targets(
            from_broadcaster_id,
            days,
        )

    async def _attach_followers_totals(self, candidates: list[dict]) -> None:
        session = self.session
        if not candidates or session is None:
            return
        await self._candidate_followers_service().attach_followers_totals(
            candidates,
            session=session,
        )

    def _load_prepared_partner_scores(
        self,
        twitch_user_ids: list[str],
    ) -> dict[str, dict[str, object]]:
        return self._candidate_selection_service().load_prepared_partner_scores(
            twitch_user_ids
        )

    async def _refresh_partner_score_cache_if_available(
        self,
        twitch_user_id: str,
        *,
        reason: str,
    ) -> None:
        await self._candidate_selection_service().refresh_partner_score_cache_if_available(
            twitch_user_id,
            reason=reason,
        )

    async def _select_partner_candidate_by_score(
        self,
        candidates: list[dict],
        from_broadcaster_id: str,
    ) -> dict | None:
        return await self._candidate_selection_service().select_partner_candidate_by_score(
            candidates,
            from_broadcaster_id,
        )

    async def _select_fairest_candidate(
        self, candidates: list[dict], from_broadcaster_id: str
    ) -> dict | None:
        return await self._candidate_selection_service().select_fairest_candidate(
            candidates,
            from_broadcaster_id,
        )

    def _is_blacklisted(self, target_id: str, target_login: str) -> bool:
        return self._raid_blacklist_service().is_blacklisted(target_id, target_login)

    def _load_raid_blacklist(self) -> tuple[set[str], set[str]]:
        return self._raid_blacklist_service().load_raid_blacklist()

    def _add_to_blacklist(self, target_id: str, target_login: str, reason: str):
        self._raid_blacklist_service().add_to_blacklist(target_id, target_login, reason)

    def _is_retryable_raid_error(self, error: str | None) -> bool:
        return is_retryable_raid_error(error)

    async def _execute_raid_pipeline(
        self,
        *,
        broadcaster_id: str,
        broadcaster_login: str,
        viewer_count: int,
        stream_duration_sec: int,
        online_partners: list[dict],
        api=None,
        category_id: str | None = None,
        offline_trigger_ts: float | None = None,
        reason: str,
        set_manual_suppression: bool = False,
    ) -> dict[str, object]:
        return await self._raid_pipeline_service().execute(
            RaidPipelineRequest(
                broadcaster_id=broadcaster_id,
                broadcaster_login=broadcaster_login,
                viewer_count=viewer_count,
                stream_duration_sec=stream_duration_sec,
                online_partners=online_partners,
                session=self.session,
                api=api,
                category_id=category_id,
                offline_trigger_ts=offline_trigger_ts,
                reason=reason,
                set_manual_suppression=set_manual_suppression,
            )
        )

    async def start_manual_raid(
        self,
        *,
        broadcaster_id: str,
        broadcaster_login: str,
    ) -> dict[str, object]:
        return await self._offline_raid_orchestrator().start_manual_raid(
            broadcaster_id=broadcaster_id,
            broadcaster_login=broadcaster_login,
        )

    async def handle_streamer_offline(
        self,
        broadcaster_id: str,
        broadcaster_login: str,
        viewer_count: int,
        stream_duration_sec: int,
        online_partners: list[dict],
        api=None,
        category_id: str | None = None,
        offline_trigger_ts: float | None = None,
    ) -> str | None:
        return await self._offline_raid_orchestrator().handle_streamer_offline(
            broadcaster_id=broadcaster_id,
            broadcaster_login=broadcaster_login,
            viewer_count=viewer_count,
            stream_duration_sec=stream_duration_sec,
            online_partners=online_partners,
            api=api,
            category_id=category_id,
            offline_trigger_ts=offline_trigger_ts,
        )
