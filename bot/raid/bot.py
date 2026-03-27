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
import discord

from ..core.constants import TWITCH_TARGET_GAME_NAME
from ..discord_role_sync import normalize_discord_user_id, sync_streamer_role
from .scope_profiles import BASE_STREAMER_SCOPES
from .arrival_confirmation import ArrivalConfirmationService
from .raid_arrival_runtime import RaidArrivalRuntime, RaidArrivalRuntimeDependencies
from .candidate_selection import CandidateSelectionService
from .chat_targets import lookup_outbound_chat_suppression, make_chat_target
from .external_recruitment import ExternalRecruitmentService
from .followers import FollowerAuthContext, FollowerTotalEnricher
from .lifecycle import RaidBotLifecycle
from .observability import RaidObservabilityEvent, RaidObservabilityService
from .partner_resolution import classify_partner_raid_arrival
from .partner_raid_delivery import PartnerRaidDeliveryConfig, PartnerRaidDeliveryPlanner, PartnerRaidDeliveryRequest
from .pending_raids import PendingRaid, PendingRaidStore
from .raid_blacklist import RaidBlacklistConfig, RaidBlacklistDependencies, RaidBlacklistService
from .raid_pipeline import RaidPipelineDependencies, RaidPipelineRequest, RaidPipelineService, is_retryable_raid_error
from .raid_tracking_runtime import (
    RaidTrackingRuntimeDependencies,
    RaidTrackingRuntimeService,
    RaidTrackingRuntimeState,
)
from .recruitment_messaging import RecruitmentMessagingDependencies, RecruitmentMessagingService
from .signal_correlation import RaidSignalCorrelationService
from ..storage import (
    backfill_tracked_stats_from_category,
    insert_observability_event,
    load_active_partner,
    load_offline_auto_raid_eligibility,
    load_streamer_identity,
    promote_streamer_to_partner,
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
        def _load_blacklist_rows():
            with readonly_connection() as conn:
                return conn.execute(
                    "SELECT target_id, lower(target_login) AS target_login FROM twitch_raid_blacklist"
                ).fetchall()

        def _store_blacklist_entry(*, target_id: str | None, target_login: str, reason: str) -> None:
            with transaction() as conn:
                if target_id:
                    conn.execute(
                        """
                        DELETE FROM twitch_raid_blacklist
                        WHERE target_id = %s
                          AND lower(target_login) <> lower(%s)
                        """,
                        (target_id, target_login),
                    )
                conn.execute(
                    """
                    INSERT INTO twitch_raid_blacklist (target_id, target_login, reason)
                    VALUES (%s, %s, %s)
                    ON CONFLICT (target_login) DO UPDATE SET
                        target_id = COALESCE(EXCLUDED.target_id, twitch_raid_blacklist.target_id),
                        reason = EXCLUDED.reason,
                        added_at = CURRENT_TIMESTAMP
                    """,
                    (target_id, target_login, reason),
                )

        def _is_blacklisted(*, target_id: str, target_login: str) -> bool:
            with readonly_connection() as conn:
                row = conn.execute(
                    """
                    SELECT 1 FROM twitch_raid_blacklist
                    WHERE (target_id IS NOT NULL AND target_id = %s)
                       OR lower(target_login) = lower(%s)
                    """,
                    (target_id, target_login),
                ).fetchone()
            return bool(row)

        def _load_due_external_recruitment_blacklist_pending():
            with readonly_connection() as conn:
                return conn.execute(
                    """
                    SELECT target_id, target_login, confirmed_raid_count, threshold_reached_at
                    FROM twitch_external_recruitment_blacklist_pending
                    WHERE blacklist_after <= NOW()
                    ORDER BY blacklist_after ASC
                    LIMIT 50
                    """
                ).fetchall()

        def _schedule_external_recruitment_blacklist_pending(
            *,
            target_id: str,
            target_login: str,
            confirmed_raid_count: int,
            raid_flow_id: str | None,
            grace_seconds: int,
        ) -> None:
            with transaction() as conn:
                conn.execute(
                    """
                    INSERT INTO twitch_external_recruitment_blacklist_pending (
                        target_id,
                        target_login,
                        confirmed_raid_count,
                        threshold_reached_at,
                        blacklist_after,
                        last_raid_flow_id,
                        updated_at
                    )
                    VALUES (
                        %s,
                        %s,
                        %s,
                        CURRENT_TIMESTAMP,
                        CURRENT_TIMESTAMP + (%s * INTERVAL '1 second'),
                        %s,
                        CURRENT_TIMESTAMP
                    )
                    ON CONFLICT (target_id) DO UPDATE SET
                        target_login = EXCLUDED.target_login,
                        confirmed_raid_count = GREATEST(
                            twitch_external_recruitment_blacklist_pending.confirmed_raid_count,
                            EXCLUDED.confirmed_raid_count
                        ),
                        last_raid_flow_id = EXCLUDED.last_raid_flow_id,
                        updated_at = CURRENT_TIMESTAMP
                    """,
                    (
                        target_id,
                        target_login,
                        int(confirmed_raid_count),
                        int(grace_seconds),
                        str(raid_flow_id or "").strip() or None,
                    ),
                )

        def _delete_external_recruitment_blacklist_pending(target_id: str) -> None:
            with transaction() as conn:
                conn.execute(
                    """
                    DELETE FROM twitch_external_recruitment_blacklist_pending
                    WHERE target_id = %s
                    """,
                    (target_id,),
                )

        def _load_due_external_target_ban_checks():
            with readonly_connection() as conn:
                return conn.execute(
                    """
                    SELECT target_id, target_login, source
                    FROM twitch_external_bot_ban_check_pending
                    WHERE run_after <= NOW()
                    ORDER BY run_after ASC
                    LIMIT 25
                    """
                ).fetchall()

        def _schedule_external_target_ban_check(
            *,
            target_id: str | None,
            target_login: str,
            source: str,
            delay_seconds: int,
        ) -> None:
            with transaction() as conn:
                conn.execute(
                    """
                    INSERT INTO twitch_external_bot_ban_check_pending (
                        target_id,
                        target_login,
                        source,
                        run_after,
                        updated_at
                    )
                    VALUES (
                        %s,
                        %s,
                        %s,
                        CURRENT_TIMESTAMP + (%s * INTERVAL '1 second'),
                        CURRENT_TIMESTAMP
                    )
                    ON CONFLICT (target_id) DO UPDATE SET
                        target_login = EXCLUDED.target_login,
                        source = EXCLUDED.source,
                        run_after = EXCLUDED.run_after,
                        updated_at = CURRENT_TIMESTAMP
                    """,
                    (target_id, target_login, source, int(delay_seconds)),
                )

        def _delete_external_target_ban_check_pending(target_id: str) -> None:
            with transaction() as conn:
                conn.execute(
                    """
                    DELETE FROM twitch_external_bot_ban_check_pending
                    WHERE target_id = %s
                    """,
                    (target_id,),
                )

        def _reschedule_external_target_ban_check_pending(target_id: str, delay_seconds: int) -> None:
            with transaction() as conn:
                conn.execute(
                    """
                    UPDATE twitch_external_bot_ban_check_pending
                    SET run_after = CURRENT_TIMESTAMP + (%s * INTERVAL '1 second'),
                        updated_at = CURRENT_TIMESTAMP
                    WHERE target_id = %s
                    """,
                    (int(max(60, delay_seconds)), target_id),
                )

        async def _join_chat_channel(chat_bot, channel_login: str, channel_id: str | None) -> bool:
            return bool(await chat_bot.join(channel_login, channel_id=channel_id))

        async def _part_chat_channels(chat_bot, channels: list[str]) -> None:
            part_channels = getattr(chat_bot, "part_channels", None)
            if callable(part_channels):
                await part_channels(channels)

        return RaidBlacklistService(
            RaidBlacklistDependencies(
                load_blacklist_rows=_load_blacklist_rows,
                is_blacklisted=_is_blacklisted,
                store_blacklist_entry=_store_blacklist_entry,
                load_due_external_recruitment_blacklist_pending=_load_due_external_recruitment_blacklist_pending,
                schedule_external_recruitment_blacklist_pending=_schedule_external_recruitment_blacklist_pending,
                delete_external_recruitment_blacklist_pending=_delete_external_recruitment_blacklist_pending,
                is_target_partner=self._is_target_currently_partner,
                load_due_external_target_ban_checks=_load_due_external_target_ban_checks,
                schedule_external_target_ban_check=_schedule_external_target_ban_check,
                delete_external_target_ban_check_pending=_delete_external_target_ban_check_pending,
                reschedule_external_target_ban_check_pending=_reschedule_external_target_ban_check_pending,
                get_chat_bot=lambda: self.chat_bot,
                join_chat_channel=_join_chat_channel,
                part_chat_channels=_part_chat_channels,
            ),
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
        self._ensure_runtime_raid_tracking_state()

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
                    pending_store=self._pending_raid_store(),
                    recent_raid_arrivals=self._recent_raid_arrivals,
                    orphan_chat_raid_notifications=self._orphan_chat_raid_notifications,
                    readiness_states=self._raid_readiness_by_flow_id,
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
                load_recent_raid_history_reference=self._load_recent_raid_history_reference,
                process_independent_partner_raid_arrival=self._process_independent_partner_raid_arrival,
                cancel_pending_raids_for_source_unraid=self._cancel_pending_raids_for_source_unraid,
                resolve_streamer_id_by_login=self._resolve_streamer_id_by_login,
                mark_manual_raid_started=lambda broadcaster_id, ttl_seconds: self.mark_manual_raid_started(
                    broadcaster_id,
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
        def _create_twitch_api(session):
            from ..api.twitch_api import TwitchAPI

            return TwitchAPI(
                self.auth_manager.client_id,
                self.auth_manager.client_secret,
                session=session,
            )

        def _count_recent_raids(to_broadcaster_id: str) -> int:
            with readonly_connection() as conn:
                row = conn.execute(
                    """
                    SELECT COUNT(*) FROM twitch_raid_history
                    WHERE to_broadcaster_id = %s
                      AND COALESCE(success, FALSE) IS TRUE
                      AND executed_at > NOW() - INTERVAL '1 day'
                    """,
                    (to_broadcaster_id,),
                ).fetchone()
            return int(row[0]) if row and row[0] is not None else 0

        def _load_deadlock_stats(to_broadcaster_login: str):
            with readonly_connection() as conn:
                return conn.execute(
                    """
                    SELECT
                        ROUND(AVG(viewer_count)) as avg_viewers,
                        MAX(viewer_count) as peak_viewers
                    FROM twitch_stats_category
                    WHERE streamer = %s
                      AND viewer_count > 0
                    """,
                    (to_broadcaster_login,),
                ).fetchone()

        return RecruitmentMessagingService(
            RecruitmentMessagingDependencies(
                create_twitch_api=_create_twitch_api,
                resolve_bot_oauth_context=lambda _session: self._resolve_bot_oauth_context(),
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
                fetch_users=lambda chat_bot, logins: chat_bot.fetch_users(logins=logins),
                lookup_outbound_chat_suppression=lookup_outbound_chat_suppression,
                join_chat_channel=lambda chat_bot, channel_login, channel_id: chat_bot.join(
                    channel_login,
                    channel_id=channel_id,
                ),
                follow_channel=lambda chat_bot, target_id: chat_bot.follow_channel(target_id),
                send_chat_message=lambda chat_bot, channel, message, source: chat_bot._send_chat_message(
                    channel,
                    message,
                    source=source,
                )
                if hasattr(chat_bot, "_send_chat_message")
                else False,
                count_recent_raids=_count_recent_raids,
                count_confirmed_external_recruitment_raids=self._get_confirmed_external_recruitment_raid_count,
                schedule_external_target_ban_check=self._schedule_external_target_ban_check,
                load_deadlock_stats=_load_deadlock_stats,
                sleep=asyncio.sleep,
            )
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
        self._ensure_runtime_raid_tracking_state()
        self._cleanup_stale_raid_readiness_states()
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
        if not isinstance(getattr(self, "_pending_raids", None), dict):
            self._pending_raids = {}
        if not isinstance(getattr(self, "_recent_raid_arrivals", None), dict):
            self._recent_raid_arrivals = {}
        if not isinstance(getattr(self, "_orphan_chat_raid_notifications", None), dict):
            self._orphan_chat_raid_notifications = {}
        readiness_by_flow = getattr(self, "_raid_readiness_by_flow_id", None)
        if not isinstance(readiness_by_flow, dict):
            self._raid_readiness_by_flow_id = {}

    def _cleanup_stale_raid_readiness_states(self) -> None:
        self._ensure_runtime_raid_tracking_state()
        now = time.time()
        expired = [
            flow_id
            for flow_id, payload in self._raid_readiness_by_flow_id.items()
            if now - float((payload or {}).get("checked_ts") or 0.0)
            > _RAID_READINESS_TTL_SECONDS
        ]
        for flow_id in expired:
            self._raid_readiness_by_flow_id.pop(flow_id, None)

        overflow = len(self._raid_readiness_by_flow_id) - _RAID_READINESS_MAX_ENTRIES
        if overflow <= 0:
            return

        oldest_flow_ids = sorted(
            self._raid_readiness_by_flow_id.items(),
            key=lambda item: float((item[1] or {}).get("checked_ts") or 0.0),
        )[:overflow]
        for flow_id, _payload in oldest_flow_ids:
            self._raid_readiness_by_flow_id.pop(flow_id, None)

    @staticmethod
    def _format_pending_raid_key_for_log(key: object) -> str:
        if isinstance(key, tuple) and len(key) >= 2:
            return f"{key[0]}:{key[1]}"
        return str(key)

    def _build_pending_raid_storage_key(
        self,
        *,
        to_broadcaster_id: str,
        from_broadcaster_login: str,
    ) -> tuple[str, str]:
        return (
            str(to_broadcaster_id or "").strip(),
            self._normalize_broadcaster_login(from_broadcaster_login),
        )

    def _pending_raid_store(self) -> PendingRaidStore:
        self._ensure_runtime_raid_tracking_state()
        return PendingRaidStore(self._pending_raids)

    def _store_pending_raid(
        self,
        pending_record: PendingRaid | Mapping[str, Any] | tuple[Any, ...],
    ) -> PendingRaid | None:
        normalized = self._pending_raid_store().store(
            pending_record,
        )
        return normalized

    def _get_pending_raid(
        self,
        *,
        to_broadcaster_id: str,
        from_broadcaster_login: str | None = None,
    ) -> PendingRaid | None:
        return self._pending_raid_store().get(
            to_broadcaster_id=to_broadcaster_id,
            from_broadcaster_login=from_broadcaster_login,
        )

    def _pop_pending_raid(
        self,
        *,
        to_broadcaster_id: str,
        from_broadcaster_login: str | None = None,
    ) -> PendingRaid | None:
        return self._pending_raid_store().pop(
            to_broadcaster_id=to_broadcaster_id,
            from_broadcaster_login=from_broadcaster_login,
        )

    def _coerce_pending_raid_record(
        self,
        pending: PendingRaid | Mapping[str, Any] | tuple[Any, ...] | None,
        *,
        to_broadcaster_id: str | None = None,
    ) -> PendingRaid | None:
        return PendingRaid.from_payload(
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
        self._ensure_runtime_raid_tracking_state()
        key = self._build_raid_arrival_cache_key(
            to_broadcaster_id=to_broadcaster_id,
            from_broadcaster_login=from_broadcaster_login,
        )
        arrival = self._recent_raid_arrivals.get(key)
        if not arrival:
            return None
        confirmed_ts = float(arrival.get("confirmed_ts") or 0.0)
        if time.time() - confirmed_ts > _RECENT_RAID_ARRIVAL_TTL_SECONDS:
            self._recent_raid_arrivals.pop(key, None)
            return None
        return arrival

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
        self._ensure_runtime_raid_tracking_state()
        key = self._build_raid_arrival_cache_key(
            to_broadcaster_id=to_broadcaster_id,
            from_broadcaster_login=from_broadcaster_login,
        )
        self._recent_raid_arrivals[key] = {
            "to_broadcaster_id": str(to_broadcaster_id or "").strip(),
            "to_broadcaster_login": self._normalize_broadcaster_login(to_broadcaster_login),
            "from_broadcaster_id": str(from_broadcaster_id or "").strip() or None,
            "from_broadcaster_login": self._normalize_broadcaster_login(from_broadcaster_login),
            "viewer_count": int(viewer_count or 0),
            "classification": str(classification or "").strip() or None,
            "confirmation_signals": set(confirmation_signals),
            "arrival_tracking_id": arrival_tracking_id,
            "raid_flow_id": str(raid_flow_id or "").strip() or None,
            "confirmed_ts": time.time(),
        }

    def _cleanup_recent_raid_arrivals(self) -> None:
        self._ensure_runtime_raid_tracking_state()
        now = time.time()
        expired = [
            key
            for key, payload in self._recent_raid_arrivals.items()
            if now - float(payload.get("confirmed_ts") or 0.0) > _RECENT_RAID_ARRIVAL_TTL_SECONDS
        ]
        for key in expired:
            self._recent_raid_arrivals.pop(key, None)

    def _store_orphan_chat_raid_notification(self, payload: dict[str, Any]) -> None:
        self._ensure_runtime_raid_tracking_state()
        self._raid_tracking_runtime_service().store_orphan_chat_raid_notification(payload)

    def _pop_orphan_chat_raid_notification(
        self,
        *,
        to_broadcaster_id: str,
        from_broadcaster_login: str,
    ) -> dict[str, Any] | None:
        self._ensure_runtime_raid_tracking_state()
        return self._raid_tracking_runtime_service().pop_orphan_chat_raid_notification(
            to_broadcaster_id=to_broadcaster_id,
            from_broadcaster_login=from_broadcaster_login,
        )

    def _promote_stale_orphan_chat_raid_notifications(self) -> None:
        self._ensure_runtime_raid_tracking_state()
        now = time.time()
        stale_payloads = [
            payload
            for payload in self._orphan_chat_raid_notifications.values()
            if now - float(payload.get("observed_ts") or 0.0)
            >= _PENDING_CHAT_NOTIFICATION_GRACE_SECONDS
        ]
        if not stale_payloads:
            return
        for payload in stale_payloads:
            processed = self._process_independent_partner_raid_arrival(
                to_broadcaster_id=str(payload.get("to_broadcaster_id") or ""),
                to_broadcaster_login=str(payload.get("to_broadcaster_login") or ""),
                from_broadcaster_login=str(payload.get("from_broadcaster_login") or ""),
                from_broadcaster_id=str(payload.get("from_broadcaster_id") or "") or None,
                viewer_count=int(payload.get("viewer_count") or 0),
                signal_type="channel.chat.notification",
                correlation_status="orphan_chat_notification",
                correlation_detail="channel.chat.notification arrived before pending raid registration",
            )
            if processed:
                self._pop_orphan_chat_raid_notification(
                    to_broadcaster_id=str(payload.get("to_broadcaster_id") or ""),
                    from_broadcaster_login=str(payload.get("from_broadcaster_login") or ""),
                )
                continue

            observed_ts = float(payload.get("observed_ts") or 0.0)
            if now - observed_ts >= _ORPHAN_CHAT_NOTIFICATION_RETENTION_SECONDS:
                self._pop_orphan_chat_raid_notification(
                    to_broadcaster_id=str(payload.get("to_broadcaster_id") or ""),
                    from_broadcaster_login=str(payload.get("from_broadcaster_login") or ""),
                )
                log.info(
                    "Discarding stale orphan channel.chat.notification after %.0fs without correlation: %s -> %s",
                    now - observed_ts,
                    str(payload.get("from_broadcaster_login") or "").strip() or "<unknown>",
                    str(payload.get("to_broadcaster_login") or "").strip() or "<unknown>",
                )

    def _resolve_known_streamer_identity(
        self,
        *,
        broadcaster_login: str,
        broadcaster_id: str | None = None,
    ) -> dict[str, str] | None:
        login_key = self._normalize_broadcaster_login(broadcaster_login)
        broadcaster_key = str(broadcaster_id or "").strip()
        if not login_key and not broadcaster_key:
            return None
        try:
            with readonly_connection() as conn:
                row = load_streamer_identity(
                    conn,
                    twitch_user_id=broadcaster_key or None,
                    twitch_login=login_key or None,
                )
        except Exception:
            log.debug(
                "Konnte Streamer-Identity nicht auflösen: %s/%s",
                broadcaster_key,
                login_key,
                exc_info=True,
            )
            return None
        if not row:
            return None
        return {
            "twitch_user_id": str(row[0] if not hasattr(row, "keys") else row["twitch_user_id"] or "").strip(),
            "twitch_login": self._normalize_broadcaster_login(
                row[1] if not hasattr(row, "keys") else row["twitch_login"]
            ),
        }

    def _is_partner_target_channel(
        self,
        *,
        broadcaster_id: str,
        broadcaster_login: str,
        expected_partner: bool = False,
    ) -> bool:
        if expected_partner:
            return True
        return bool(
            self._lookup_partner_target_channel(
                broadcaster_id=broadcaster_id,
                broadcaster_login=broadcaster_login,
            )
        )

    def _lookup_partner_target_channel(
        self,
        *,
        broadcaster_id: str,
        broadcaster_login: str,
    ) -> Any:
        broadcaster_key = str(broadcaster_id or "").strip()
        login_key = self._normalize_broadcaster_login(broadcaster_login)
        try:
            with readonly_connection() as conn:
                return load_active_partner(
                    conn,
                    twitch_user_id=broadcaster_key or None,
                    twitch_login=login_key or None,
                )
        except Exception:
            log.debug(
                "Partner target lookup failed for %s (%s)",
                login_key,
                broadcaster_key,
                exc_info=True,
            )
            return None

    def _classify_partner_raid_arrival(
        self,
        *,
        from_broadcaster_login: str,
        from_broadcaster_id: str | None,
        to_broadcaster_id: str,
        to_broadcaster_login: str,
        expected_partner: bool = False,
    ) -> tuple[str | None, str]:
        partner_row = self._lookup_partner_target_channel(
            broadcaster_id=to_broadcaster_id,
            broadcaster_login=to_broadcaster_login,
        )
        result = classify_partner_raid_arrival(
            from_broadcaster_login=from_broadcaster_login,
            from_broadcaster_id=from_broadcaster_id,
            to_broadcaster_id=to_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            partner_lookup=lambda **_kwargs: partner_row,
            known_streamer_lookup=lambda **lookup_kwargs: self._resolve_known_streamer_identity(
                broadcaster_login=str(lookup_kwargs.get("broadcaster_login") or ""),
                broadcaster_id=str(lookup_kwargs.get("broadcaster_id") or "") or None,
            ),
        )
        if result.classification is None and expected_partner:
            result = classify_partner_raid_arrival(
                from_broadcaster_login=from_broadcaster_login,
                from_broadcaster_id=from_broadcaster_id,
                to_broadcaster_id=to_broadcaster_id,
                to_broadcaster_login=to_broadcaster_login,
                partner_lookup=lambda **_kwargs: {"source": "pending_partner_override"},
                known_streamer_lookup=lambda **lookup_kwargs: self._resolve_known_streamer_identity(
                    broadcaster_login=str(lookup_kwargs.get("broadcaster_login") or ""),
                    broadcaster_id=str(lookup_kwargs.get("broadcaster_id") or "") or None,
                ),
            )
        return result.as_tuple()

    def _load_recent_raid_history_reference(
        self,
        *,
        from_broadcaster_login: str,
        to_broadcaster_id: str,
    ) -> tuple[int | None, str | None]:
        try:
            with readonly_connection() as conn:
                row = conn.execute(
                    """
                    SELECT id, executed_at
                    FROM twitch_raid_history
                    WHERE LOWER(from_broadcaster_login) = %s
                      AND to_broadcaster_id = %s
                      AND COALESCE(success, FALSE) IS TRUE
                    ORDER BY executed_at DESC
                    LIMIT 1
                    """,
                    (
                        self._normalize_broadcaster_login(from_broadcaster_login),
                        str(to_broadcaster_id or "").strip(),
                    ),
                ).fetchone()
        except Exception:
            log.debug(
                "Could not load raid history reference for %s -> %s",
                from_broadcaster_login,
                to_broadcaster_id,
                exc_info=True,
            )
            return None, None
        if not row:
            return None, None
        raid_history_id = int(row[0] if not hasattr(row, "keys") else row["id"])
        executed_at = (
            str(row[1] if not hasattr(row, "keys") else row["executed_at"] or "").strip() or None
        )
        return raid_history_id, executed_at

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
        confirmation_signal_text = self._serialize_confirmation_signals(confirmation_signals)
        try:
            with transaction() as conn:
                row = conn.execute(
                    """
                    INSERT INTO twitch_raid_arrival_tracking (
                        from_broadcaster_id,
                        from_broadcaster_login,
                        to_broadcaster_id,
                        to_broadcaster_login,
                        viewer_count,
                        classification,
                        confirmation_signals,
                        primary_signal,
                        correlation_status,
                        correlation_detail,
                        source_resolution,
                        raid_history_id,
                        raid_history_executed_at,
                        unraid_seen,
                        last_unraid_at
                    ) VALUES (%s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s)
                    RETURNING id
                    """,
                    (
                        str(from_broadcaster_id or "").strip() or None,
                        self._normalize_broadcaster_login(from_broadcaster_login),
                        str(to_broadcaster_id or "").strip(),
                        self._normalize_broadcaster_login(to_broadcaster_login),
                        int(viewer_count or 0),
                        str(classification or "").strip(),
                        confirmation_signal_text,
                        str(primary_signal or "").strip(),
                        str(correlation_status or "").strip(),
                        str(correlation_detail or "").strip() or None,
                        str(source_resolution or "").strip(),
                        raid_history_id,
                        raid_history_executed_at,
                        bool(unraid_seen),
                        datetime.now(UTC).isoformat() if unraid_seen else None,
                    ),
                ).fetchone()
            if not row:
                return None
            return int(row[0] if not hasattr(row, "keys") else row["id"])
        except Exception:
            log.exception(
                "Failed to store partner raid arrival: %s -> %s (%s)",
                from_broadcaster_login,
                to_broadcaster_login,
                correlation_status,
            )
            return None

    def _update_partner_raid_arrival(
        self,
        *,
        arrival_tracking_id: int,
        confirmation_signals: set[str],
        unraid_seen: bool = False,
    ) -> None:
        if not arrival_tracking_id:
            return
        try:
            with transaction() as conn:
                conn.execute(
                    """
                    UPDATE twitch_raid_arrival_tracking
                    SET confirmation_signals = %s,
                        last_signal_at = CURRENT_TIMESTAMP,
                        unraid_seen = CASE WHEN %s THEN TRUE ELSE unraid_seen END,
                        last_unraid_at = CASE WHEN %s THEN %s ELSE last_unraid_at END
                    WHERE id = %s
                    """,
                    (
                        self._serialize_confirmation_signals(confirmation_signals),
                        bool(unraid_seen),
                        bool(unraid_seen),
                        datetime.now(UTC).isoformat() if unraid_seen else None,
                        int(arrival_tracking_id),
                    ),
                )
        except Exception:
            log.debug(
                "Could not update partner raid arrival tracking row %s",
                arrival_tracking_id,
                exc_info=True,
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
        classification, source_resolution = self._classify_partner_raid_arrival(
            from_broadcaster_login=from_broadcaster_login,
            from_broadcaster_id=from_broadcaster_id,
            to_broadcaster_id=to_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
        )
        if classification is None:
            return False

        from_broadcaster_key = str(from_broadcaster_id or "").strip()
        if not from_broadcaster_key:
            from_broadcaster_key = self._resolve_streamer_id_by_login(from_broadcaster_login) or ""
        if from_broadcaster_key:
            self.mark_manual_raid_started(from_broadcaster_key, ttl_seconds=180.0)

        arrival_tracking_id = self._store_partner_raid_arrival(
            from_broadcaster_id=from_broadcaster_id,
            from_broadcaster_login=from_broadcaster_login,
            to_broadcaster_id=to_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            viewer_count=viewer_count,
            classification=classification,
            confirmation_signals={signal_type},
            primary_signal=signal_type,
            correlation_status=correlation_status,
            correlation_detail=correlation_detail,
            source_resolution=source_resolution,
        )
        self._remember_recent_raid_arrival(
            to_broadcaster_id=to_broadcaster_id,
            from_broadcaster_login=from_broadcaster_login,
            from_broadcaster_id=from_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            viewer_count=viewer_count,
            classification=classification,
            confirmation_signals={signal_type},
            arrival_tracking_id=arrival_tracking_id,
        )
        log.info(
            "Partner raid arrival classified: %s -> %s (%s via %s)",
            from_broadcaster_login,
            to_broadcaster_login,
            classification,
            signal_type,
        )
        return True

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
        cached = getattr(self, "_target_game_lower", None)
        if isinstance(cached, str) and cached:
            return cached

        resolved = ""
        cog = getattr(self, "_cog", None)
        get_target_lower = getattr(cog, "_get_target_game_lower", None) if cog else None
        if callable(get_target_lower):
            try:
                resolved = str(get_target_lower() or "").strip().lower()
            except Exception:
                resolved = ""

        if not resolved:
            resolved = (TWITCH_TARGET_GAME_NAME or "").strip().lower()

        self._target_game_lower = resolved
        return resolved

    def _is_recent_deadlock(
        self,
        last_deadlock_seen_at: str | None,
        *,
        now_utc: datetime | None = None,
        recency_cap_seconds: int = 360,
    ) -> bool:
        last_deadlock_dt = self._parse_datetime(last_deadlock_seen_at)
        if last_deadlock_dt is None:
            return False
        reference = now_utc if now_utc is not None else datetime.now(UTC)
        return (reference - last_deadlock_dt).total_seconds() <= recency_cap_seconds

    def _evaluate_deadlock_raid_source(
        self,
        *,
        current_game: str,
        had_deadlock_session: bool,
        last_deadlock_seen_at: str | None,
    ) -> dict[str, object]:
        target_game_lower = self._get_target_game_lower()
        current_game_text = str(current_game or "").strip()
        current_game_lower = current_game_text.lower()

        recent_deadlock = False
        if target_game_lower and current_game_lower == target_game_lower:
            recent_deadlock = True
        elif current_game_lower == "just chatting" and had_deadlock_session:
            recent_deadlock = self._is_recent_deadlock(last_deadlock_seen_at)

        if not target_game_lower:
            return {
                "eligible": False,
                "reason": "target_game_unconfigured",
                "current_game": current_game_text,
                "recent_deadlock": recent_deadlock,
                "had_deadlock_session": had_deadlock_session,
            }

        if current_game_lower == target_game_lower:
            return {
                "eligible": True,
                "reason": "active_deadlock",
                "current_game": current_game_text,
                "recent_deadlock": True,
                "had_deadlock_session": had_deadlock_session,
            }

        if current_game_lower == "just chatting":
            if not had_deadlock_session:
                return {
                    "eligible": False,
                    "reason": "just_chatting_without_deadlock_session",
                    "current_game": current_game_text,
                    "recent_deadlock": False,
                    "had_deadlock_session": had_deadlock_session,
                }
            if recent_deadlock:
                return {
                    "eligible": True,
                    "reason": "recent_deadlock_session",
                    "current_game": current_game_text,
                    "recent_deadlock": True,
                    "had_deadlock_session": had_deadlock_session,
                }
            return {
                "eligible": False,
                "reason": "stale_deadlock_session",
                "current_game": current_game_text,
                "recent_deadlock": False,
                "had_deadlock_session": had_deadlock_session,
            }

        if not current_game_lower:
            reason = "missing_current_game"
        else:
            reason = "source_category_mismatch"
        return {
            "eligible": False,
            "reason": reason,
            "current_game": current_game_text,
            "recent_deadlock": False,
            "had_deadlock_session": had_deadlock_session,
        }

    def _is_deadlock_raid_source_eligible(
        self,
        *,
        last_game: str,
        had_deadlock_session: bool,
        last_deadlock_seen_at: str | None,
    ) -> bool:
        evaluation = self._evaluate_deadlock_raid_source(
            current_game=last_game,
            had_deadlock_session=had_deadlock_session,
            last_deadlock_seen_at=last_deadlock_seen_at,
        )
        return bool(evaluation.get("eligible"))

    def _is_deadlock_partner_candidate_eligible(
        self,
        *,
        game_name: str,
        had_deadlock_session: bool,
        last_deadlock_seen_at: str | None,
    ) -> bool:
        target_game_lower = self._get_target_game_lower()
        if not target_game_lower:
            return True

        game_lower = str(game_name or "").strip().lower()
        if game_lower == target_game_lower:
            return True
        if game_lower == "just chatting" and had_deadlock_session:
            return self._is_recent_deadlock(last_deadlock_seen_at)
        return False

    def _load_partner_roster_for_raid(self, source_user_id: str) -> list[dict[str, object]]:
        with readonly_connection() as conn:
            rows = conn.execute(
                """
                SELECT DISTINCT s.twitch_login, s.twitch_user_id,
                       r.raid_enabled, r.authorized_at
                  FROM twitch_streamers_partner_state s
                  LEFT JOIN twitch_raid_auth r ON s.twitch_user_id = r.twitch_user_id
                 WHERE s.is_partner_active = 1
                   AND s.twitch_user_id IS NOT NULL
                   AND s.twitch_login IS NOT NULL
                   AND s.twitch_user_id != %s
                """,
                (source_user_id,),
            ).fetchall()

        partners: list[dict[str, object]] = []
        for row in rows:
            partner_login = str(self._row_value(row, "twitch_login", 0, "") or "").strip().lower()
            partner_user_id = str(self._row_value(row, "twitch_user_id", 1, "") or "").strip()
            raid_enabled = bool(self._row_value(row, "raid_enabled", 2, False))
            raid_authorized_at = self._row_value(row, "authorized_at", 3, None)
            if not partner_login or not partner_user_id:
                continue
            if not raid_enabled and not raid_authorized_at:
                continue
            partners.append(
                {
                    "twitch_login": partner_login,
                    "twitch_user_id": partner_user_id,
                    "raid_enabled": raid_enabled or bool(raid_authorized_at),
                }
            )
        return partners

    def _build_online_partner_candidates(
        self,
        partner_rows: list[dict[str, object]],
        streams_by_login: dict[str, dict],
    ) -> list[dict]:
        online_partners: list[dict] = []
        for partner_row in partner_rows:
            partner_login = str(partner_row.get("twitch_login") or "").strip().lower()
            partner_user_id = str(partner_row.get("twitch_user_id") or "").strip()
            if not partner_login or not partner_user_id:
                continue
            stream_data = streams_by_login.get(partner_login)
            if not stream_data:
                continue
            candidate = dict(stream_data)
            candidate["user_id"] = partner_user_id
            candidate["raid_enabled"] = bool(partner_row.get("raid_enabled", True))
            online_partners.append(candidate)
        return online_partners

    def _load_partner_live_state_map(
        self,
        partner_logins_lower: list[str],
    ) -> dict[str, dict[str, object]]:
        if not partner_logins_lower:
            return {}

        placeholders = ",".join("%s" for _ in partner_logins_lower)
        with readonly_connection() as conn:
            rows = conn.execute(
                f"""
                SELECT streamer_login, had_deadlock_in_session, last_game, last_deadlock_seen_at
                  FROM twitch_live_state
                 WHERE streamer_login IN ({placeholders})
                """,
                partner_logins_lower,
            ).fetchall()

        live_state_by_login: dict[str, dict[str, object]] = {}
        for row in rows:
            login_lower = str(self._row_value(row, "streamer_login", 0, "") or "").strip().lower()
            if not login_lower:
                continue
            live_state_by_login[login_lower] = {
                "had_deadlock_in_session": bool(
                    self._safe_int(self._row_value(row, "had_deadlock_in_session", 1, 0), 0)
                ),
                "last_game": str(self._row_value(row, "last_game", 2, "") or "").strip(),
                "last_deadlock_seen_at": str(
                    self._row_value(row, "last_deadlock_seen_at", 3, "") or ""
                ).strip(),
            }
        return live_state_by_login

    def _filter_deadlock_eligible_partner_candidates(
        self,
        online_partners: list[dict],
    ) -> tuple[list[dict], list[str]]:
        target_game_lower = self._get_target_game_lower()
        if not target_game_lower or not online_partners:
            return list(online_partners), []

        partner_logins_lower = [
            str(stream_data.get("user_login") or "").strip().lower()
            for stream_data in online_partners
            if str(stream_data.get("user_login") or "").strip()
        ]
        live_state_by_login: dict[str, dict[str, object]] = {}
        try:
            live_state_by_login = self._load_partner_live_state_map(partner_logins_lower)
        except Exception:
            log.debug("Konnte Live-State für Partner nicht laden", exc_info=True)

        filtered_active: list[dict] = []
        filtered_recent: list[dict] = []
        filtered_out: list[str] = []

        for stream_data in online_partners:
            partner_login_lower = str(stream_data.get("user_login") or "").strip().lower()
            game_name = str(stream_data.get("game_name") or "").strip()
            game_lower = game_name.lower()
            live_state = live_state_by_login.get(partner_login_lower, {})
            had_deadlock_partner = bool(live_state.get("had_deadlock_in_session", False))
            last_game_state = str(live_state.get("last_game") or "").strip()
            last_deadlock_seen_partner = (
                str(live_state.get("last_deadlock_seen_at") or "").strip() or None
            )

            allow_partner = self._is_deadlock_partner_candidate_eligible(
                game_name=game_name,
                had_deadlock_session=had_deadlock_partner,
                last_deadlock_seen_at=last_deadlock_seen_partner,
            )
            if allow_partner:
                if game_lower == target_game_lower:
                    filtered_active.append(stream_data)
                else:
                    filtered_recent.append(stream_data)
                continue

            filtered_out.append(
                f"{partner_login_lower} (game='{game_name or last_game_state}', "
                f"had_deadlock_session={had_deadlock_partner}, "
                f"last_deadlock_seen={last_deadlock_seen_partner or 'none'})"
            )

        eligible_partners = filtered_active if filtered_active else filtered_recent
        return eligible_partners, filtered_out

    def _load_broadcaster_live_state(self, broadcaster_id: str) -> dict[str, object]:
        with readonly_connection() as conn:
            row = conn.execute(
                """
                SELECT twitch_user_id, streamer_login, is_live, last_started_at,
                       last_game, last_viewer_count, had_deadlock_in_session, last_deadlock_seen_at
                  FROM twitch_live_state
                 WHERE twitch_user_id = %s
                """,
                (broadcaster_id,),
            ).fetchone()

        if not row:
            return {}

        return {
            "twitch_user_id": str(self._row_value(row, "twitch_user_id", 0, "") or "").strip(),
            "streamer_login": str(self._row_value(row, "streamer_login", 1, "") or "").strip().lower(),
            "is_live": bool(self._safe_int(self._row_value(row, "is_live", 2, 0), 0)),
            "last_started_at": str(self._row_value(row, "last_started_at", 3, "") or "").strip(),
            "last_game": str(self._row_value(row, "last_game", 4, "") or "").strip(),
            "last_viewer_count": self._safe_int(self._row_value(row, "last_viewer_count", 5, 0), 0),
            "had_deadlock_in_session": bool(
                self._safe_int(self._row_value(row, "had_deadlock_in_session", 6, 0), 0)
            ),
            "last_deadlock_seen_at": str(
                self._row_value(row, "last_deadlock_seen_at", 7, "") or ""
            ).strip(),
        }

    def _calculate_stream_duration_sec(self, started_at: str | None) -> int:
        started_dt = self._parse_datetime(started_at)
        if started_dt is None:
            return 0
        return max(0, int((datetime.now(UTC) - started_dt).total_seconds()))

    def _raid_language_filters(self) -> list[str | None]:
        cog = getattr(self, "_cog", None)
        language_filter_values = getattr(cog, "_language_filter_values", None) if cog else None
        if callable(language_filter_values):
            try:
                values = list(language_filter_values())
            except Exception:
                values = []
            if values:
                return values
        return [None]

    def _create_twitch_api(self):
        session = self.session
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
        normalized_logins = [
            login.lower()
            for login in dict.fromkeys(str(login or "").strip() for login in logins)
            if login
        ]
        if not normalized_logins:
            return {}

        cog = getattr(self, "_cog", None)
        shared_fetch = getattr(cog, "_fetch_streams_by_logins_quick", None) if cog else None
        if callable(shared_fetch):
            try:
                streams_by_login = await shared_fetch(normalized_logins)
                if streams_by_login:
                    return streams_by_login
            except Exception:
                log.debug("RaidBot: shared stream fetch failed", exc_info=True)

        api_client = api or self._create_twitch_api()
        if api_client is None:
            return {}

        streams_by_login: dict[str, dict] = {}
        for language in self._raid_language_filters():
            try:
                streams = await api_client.get_streams_by_logins(
                    normalized_logins,
                    language=language,
                )
            except Exception:
                log.debug(
                    "RaidBot: get_streams_by_logins failed (language=%s)",
                    language or "any",
                    exc_info=True,
                )
                continue
            for stream in streams:
                login_lower = str(stream.get("user_login") or "").strip().lower()
                if login_lower:
                    streams_by_login[login_lower] = stream
        return streams_by_login

    def _overlay_broadcaster_live_state_from_stream(
        self,
        live_state: dict[str, object],
        stream_data: dict[str, object],
    ) -> dict[str, object]:
        merged_state = dict(live_state)
        twitch_user_id = str(
            stream_data.get("user_id") or merged_state.get("twitch_user_id") or ""
        ).strip()
        streamer_login = str(
            stream_data.get("user_login") or merged_state.get("streamer_login") or ""
        ).strip().lower()
        merged_state.update(
            {
                "twitch_user_id": twitch_user_id,
                "streamer_login": streamer_login,
                "is_live": True,
                "last_started_at": str(stream_data.get("started_at") or "").strip(),
                "last_game": str(stream_data.get("game_name") or "").strip(),
                "last_viewer_count": self._safe_int(stream_data.get("viewer_count"), 0),
            }
        )
        return merged_state

    async def _resolve_manual_raid_source_state(
        self,
        *,
        broadcaster_id: str,
        broadcaster_login: str,
        api=None,
    ) -> dict[str, object]:
        db_live_state = self._load_broadcaster_live_state(broadcaster_id)
        resolved_live_state = dict(db_live_state)
        normalized_login = str(broadcaster_login or "").strip().lower()
        api_client = api or self._create_twitch_api()

        if api_client is not None and normalized_login:
            try:
                streams = await api_client.get_streams_by_logins([normalized_login])
            except Exception:
                log.debug(
                    "Manual raid source refresh failed for %s; falling back to DB snapshot",
                    broadcaster_login,
                    exc_info=True,
                )
            else:
                matched_stream = next(
                    (
                        stream
                        for stream in streams
                        if str(stream.get("user_login") or "").strip().lower() == normalized_login
                    ),
                    None,
                )
                if matched_stream is None and len(streams) == 1:
                    matched_stream = streams[0]
                if matched_stream is not None:
                    return {
                        "status": "ok",
                        "state_source": "api_live",
                        "live_state": self._overlay_broadcaster_live_state_from_stream(
                            resolved_live_state,
                            matched_stream,
                        ),
                    }
                return {
                    "status": "source_not_live",
                    "state_source": "api_offline",
                    "live_state": resolved_live_state,
                }

        if resolved_live_state and bool(resolved_live_state.get("is_live")):
            return {
                "status": "ok",
                "state_source": "db",
                "live_state": resolved_live_state,
            }
        return {
            "status": "source_not_live",
            "state_source": "db",
            "live_state": resolved_live_state,
        }

    async def _resolve_target_category_id(self, api=None) -> str | None:
        cog = getattr(self, "_cog", None)
        cached_category_id = getattr(cog, "_category_id", None) if cog else None
        if cached_category_id:
            return str(cached_category_id)

        api_client = api or self._create_twitch_api()
        if api_client is None:
            return None

        try:
            return await api_client.get_category_id(TWITCH_TARGET_GAME_NAME)
        except Exception:
            log.debug("RaidBot: could not resolve target category id", exc_info=True)
            return None

    def mark_manual_raid_started(self, broadcaster_id: str, ttl_seconds: float = 300.0) -> None:
        """Unterdrückt den nächsten Offline-Auto-Raid für einen Streamer (z.B. nach !raid/!traid)."""
        broadcaster_key = str(broadcaster_id or "").strip()
        if not broadcaster_key:
            return
        ttl = max(30.0, float(ttl_seconds or 0.0))
        self._manual_raid_suppression[broadcaster_key] = time.time() + ttl

    def is_offline_auto_raid_suppressed(self, broadcaster_id: str) -> bool:
        """True, wenn für den Streamer aktuell eine manuelle-Raid-Sperre aktiv ist."""
        broadcaster_key = str(broadcaster_id or "").strip()
        if not broadcaster_key:
            return False
        now = time.time()
        until = self._manual_raid_suppression.get(broadcaster_key)
        if until is None:
            return False
        if now <= until:
            return True
        self._manual_raid_suppression.pop(broadcaster_key, None)
        return False

    def _resolve_streamer_id_by_login(self, broadcaster_login: str) -> str | None:
        """Best-effort: löst eine Twitch-User-ID aus twitch_streamers über den Login auf."""
        login_key = str(broadcaster_login or "").strip().lower()
        if not login_key:
            return None
        try:
            with readonly_connection() as conn:
                row = load_active_partner(conn, twitch_login=login_key)
            if not row:
                return None
            resolved = row["twitch_user_id"] if hasattr(row, "keys") else row[1]
            resolved_key = str(resolved or "").strip()
            return resolved_key or None
        except Exception:
            log.debug(
                "Konnte broadcaster_id nicht über Login auflösen: %s",
                login_key,
                exc_info=True,
            )
            return None

    def _cleanup_expired_manual_raid_suppressions(self) -> None:
        """Entfernt abgelaufene Einträge aus dem Manual-Raid-Suppression-Cache."""
        now = time.time()
        expired = [
            broadcaster_id
            for broadcaster_id, until in self._manual_raid_suppression.items()
            if now > float(until or 0.0)
        ]
        for broadcaster_id in expired:
            self._manual_raid_suppression.pop(broadcaster_id, None)
        if expired:
            log.debug("Cleaned up %d expired manual raid suppressions", len(expired))

    @staticmethod
    def _normalize_discord_user_id(raw: str | None) -> str | None:
        return normalize_discord_user_id(raw)

    async def _resolve_discord_display_name(self, discord_user_id: str | None) -> str | None:
        normalized_id = self._normalize_discord_user_id(discord_user_id)
        if not normalized_id:
            return None

        discord_bot = getattr(self.auth_manager, "_discord_bot", None)
        if discord_bot is None:
            return None

        user_id_int = int(normalized_id)
        user = discord_bot.get_user(user_id_int)
        if user is None:
            try:
                user = await discord_bot.fetch_user(user_id_int)
            except (discord.NotFound, discord.Forbidden, discord.HTTPException):
                return None

        if user is None:
            return None
        return (
            str(
                getattr(user, "global_name", None)
                or getattr(user, "display_name", None)
                or getattr(user, "name", None)
                or ""
            ).strip()
            or None
        )

    async def _apply_streamer_role(
        self,
        discord_user_id: str | None,
        *,
        should_have_role: bool,
        reason: str,
    ) -> None:
        discord_bot = getattr(self.auth_manager, "_discord_bot", None)
        await sync_streamer_role(
            discord_bot,
            discord_user_id,
            should_have_role=should_have_role,
            reason=reason,
            logger=log,
        )

    async def _sync_partner_state_after_auth(
        self,
        twitch_user_id: str,
        twitch_login: str,
        *,
        state_discord_user_id: str | None = None,
        activate_partner_features: bool = True,
    ) -> str | None:
        provided_discord_id = self._normalize_discord_user_id(state_discord_user_id)
        existing_discord_id: str | None = None
        existing_display_name: str | None = None

        with readonly_connection() as conn:
            row = load_streamer_identity(
                conn,
                twitch_user_id=twitch_user_id,
                twitch_login=twitch_login,
            )
            if row:
                existing_discord_id = self._normalize_discord_user_id(
                    row[2] if not hasattr(row, "keys") else row["discord_user_id"]
                )
                existing_display_name = (
                    str(
                        row[3] if not hasattr(row, "keys") else row["discord_display_name"] or ""
                    ).strip()
                    or None
                )

        final_discord_id = provided_discord_id or existing_discord_id
        final_display_name = existing_display_name or await self._resolve_discord_display_name(
            final_discord_id
        )

        is_on_discord_value = 1 if final_discord_id else 0
        with transaction() as conn:
            partner_kwargs: dict[str, object] = {
                "discord_user_id": final_discord_id,
                "discord_display_name": final_display_name,
                "is_on_discord": is_on_discord_value,
                "manual_verified_permanent": 1,
                "manual_verified_until": None,
                "manual_verified_at": datetime.now(UTC).isoformat(),
            }
            if activate_partner_features:
                partner_kwargs.update(
                    {
                        "manual_partner_opt_out": 0,
                        "raid_bot_enabled": 1,
                    }
                )
            promote_streamer_to_partner(
                conn,
                twitch_login=twitch_login,
                twitch_user_id=twitch_user_id,
                **partner_kwargs,
            )
            copied = backfill_tracked_stats_from_category(conn, twitch_login)
            if copied:
                log.info(
                    "Backfilled %d category samples into tracked for %s during partner sync",
                    copied,
                    twitch_login,
                )
            # autocommit – no explicit commit needed

        if final_discord_id:
            await self._apply_streamer_role(
                final_discord_id,
                should_have_role=True,
                reason="Twitch-Bot erfolgreich autorisiert",
            )
        return final_discord_id

    async def complete_setup_for_streamer(
        self,
        twitch_user_id: str,
        twitch_login: str,
        state_discord_user_id: str | None = None,
        activate_partner_features: bool = True,
    ):
        """
        Führt Aktionen nach erfolgreicher OAuth-Autorisierung aus:
        1. Bot als Moderator setzen
        2. Bestätigungsnachricht im Chat senden
        """
        log.info("Completing setup for streamer %s (%s)", twitch_login, twitch_user_id)

        try:
            await self._sync_partner_state_after_auth(
                twitch_user_id,
                twitch_login,
                state_discord_user_id=state_discord_user_id,
                activate_partner_features=activate_partner_features,
            )
        except Exception:
            log.exception(
                "Failed to sync partner state after auth for %s (%s)",
                twitch_login,
                twitch_user_id,
            )

        # 1. Tokens holen
        tokens = await self.auth_manager.get_tokens_for_user(twitch_user_id, self.session)
        if not tokens:
            log.warning("Could not load OAuth grant for %s to complete setup", twitch_login)
            return

        access_token, _ = tokens
        # Bot-ID: aus chat_bot wenn verfügbar, sonst aus gespeichertem _bot_id Fallback
        bot_id = None
        if self.chat_bot:
            bot_id = getattr(self.chat_bot, "bot_id_safe", None)
            if bot_id is None:
                bot_id_raw = getattr(self.chat_bot, "bot_id", None)
                bot_id = str(bot_id_raw).strip() if bot_id_raw and str(bot_id_raw).strip() else None
        if not bot_id:
            bot_id = getattr(self, "_bot_id", None)
        if not bot_id:
            # Letzte Chance: Bot-ID aus ENV
            import os

            bot_id = os.getenv("TWITCH_BOT_USER_ID", "").strip() or None
        if not bot_id:
            log.warning(
                "complete_setup: Keine Bot-ID verfügbar für %s (chat_bot=%s). Setze TWITCH_BOT_USER_ID ENV.",
                twitch_login,
                "None" if not self.chat_bot else "set",
            )
            return

        # 2. Bot als Moderator setzen
        if bot_id:
            try:
                url = f"{TWITCH_API_BASE}/moderation/moderators"
                params = {
                    "broadcaster_id": twitch_user_id,
                    "user_id": bot_id,
                }
                headers = {
                    "Client-ID": self.auth_manager.client_id,
                    "Authorization": f"Bearer {access_token}",
                }
                async with self.session.post(url, headers=headers, params=params) as r:
                    if r.status in {200, 204}:
                        log.info(
                            "Bot (ID: %s) is now moderator in %s's channel (ID: %s)",
                            bot_id,
                            twitch_login,
                            twitch_user_id,
                        )
                    elif r.status == 422:
                        log.info(
                            "Bot (ID: %s) is already moderator in %s's channel",
                            bot_id,
                            twitch_login,
                        )
                    else:
                        txt = await r.text()
                        if r.status == 400 and "already a mod" in txt.lower():
                            log.info(
                                "Bot (ID: %s) is already moderator in %s's channel (HTTP 400 variant)",
                                bot_id,
                                twitch_login,
                            )
                        else:
                            log.warning(
                                "Failed to add bot as moderator in %s: HTTP %s (used broadcaster grant)",
                                _mask_log_identifier(twitch_login),
                                r.status,
                            )
            except Exception:
                log.exception("Error adding bot as moderator for %s", twitch_login)

        # 3. Bestätigungsnachricht senden
        if self.chat_bot:
            try:
                # Sicherstellen, dass der Bot im Channel ist
                await self.chat_bot.join(twitch_login, channel_id=twitch_user_id)
                await asyncio.sleep(
                    2
                )  # Etwas mehr Zeit geben, damit der Mod-Status im Chat "ankommt"

                # Nachricht im Stil des Screenshots
                message = "Deadlock Chatbot Guard verbunden! 🎮"
                commands_public = (
                    "Commands für alle: "
                    "!ping (Bot-Status) | "
                    "!clip [beschreibung] (Clip erstellen) | "
                    "!raid_history (letzte Raids)"
                )
                commands_mod = (
                    "Mod-Commands: "
                    "!raid / !traid (Raid starten) | "
                    "!raid_status (Bot-Status) | "
                    "!uban / !unban (letzten Auto-Ban aufheben) | "
                    "!silentban / !silentraid (Benachrichtigungen an/aus)"
                )

                # Sende Nachrichten (EventSub kompatibel via ChatBot Methode)
                if hasattr(self.chat_bot, "_send_chat_message"):
                    # Mock Channel-Objekt für die interne Methode
                    class MockChannel:
                        def __init__(self, login, uid):
                            self.name = login
                            self.id = uid

                    mock_ch = MockChannel(twitch_login, twitch_user_id)
                    await self.chat_bot._send_chat_message(mock_ch, message)
                    await asyncio.sleep(1)
                    await self.chat_bot._send_chat_message(mock_ch, commands_public)
                    await asyncio.sleep(1)
                    await self.chat_bot._send_chat_message(mock_ch, commands_mod)
                elif hasattr(self.chat_bot, "send_message") and bot_id:
                    await self.chat_bot.send_message(str(twitch_user_id), str(bot_id), message)
                    await asyncio.sleep(1)
                    await self.chat_bot.send_message(
                        str(twitch_user_id), str(bot_id), commands_public
                    )
                    await asyncio.sleep(1)
                    await self.chat_bot.send_message(str(twitch_user_id), str(bot_id), commands_mod)

                log.info("Sent auth success message to %s", twitch_login)
            except Exception:
                log.exception("Error sending auth success message to %s", twitch_login)

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
        """
        Sendet eine Bestätigungs-Nachricht im Chat des geraideten Partners.

        Diese Nachricht zeigt dem Partner-Streamer, dass der Raid durch
        das Deadlock Streamer-Netzwerk kam, um den Mehrwert zu verdeutlichen.

        Wird aufgerufen NACH dem EventSub channel.raid Event, d.h. der Raid
        ist bereits beim Ziel angekommen.
        """
        if not self.chat_bot:
            log.debug("Chat bot not available for partner raid message")
            return

        try:
            target_channel = self._make_chat_target(to_broadcaster_login, to_broadcaster_id)
            suppression = self._lookup_outbound_chat_suppression(
                to_broadcaster_login,
                to_broadcaster_id,
                source="partner_raid",
            )
            if suppression is not None:
                log.info(
                    "Skipping partner raid message to %s due stored chat suppression (code=%s, until=%s)",
                    to_broadcaster_login,
                    suppression.get("reason_code") or "unknown",
                    suppression.get("suppressed_until") or "-",
                )
                return

            # Erfolgreiche Netzwerk-Raids für dieses Ziel zählen (inkl. aktuellem Raid)
            received_raid_count = self._get_received_network_raid_count(to_broadcaster_id)
            if received_raid_count <= 0:
                received_raid_count = 1

            plan = self._partner_raid_delivery_planner().plan(
                PartnerRaidDeliveryRequest(
                    from_broadcaster_login=from_broadcaster_login,
                    to_broadcaster_login=to_broadcaster_login,
                    to_broadcaster_id=to_broadcaster_id,
                    viewer_count=viewer_count,
                    received_raid_count=received_raid_count,
                    chat_bot_available=bool(self.chat_bot),
                    outbound_chat_suppressed=False,
                )
            )
            if not plan.should_deliver or not plan.message:
                log.info(
                    "Skipping partner raid message to %s (%s)",
                    to_broadcaster_login,
                    plan.reason or "blocked",
                )
                return

            # 1. Channel beitreten (falls noch nicht joined)
            await self.chat_bot.join(to_broadcaster_login, channel_id=to_broadcaster_id)

            # 2. Kurze Verzögerung, damit der Bot bereit ist und der Raid-Alert durch ist
            await asyncio.sleep(plan.delay_seconds)

            # 4. Nachricht senden
            if hasattr(self.chat_bot, "_send_chat_message"):
                success = await self.chat_bot._send_chat_message(
                    target_channel,
                    plan.message,
                    source="partner_raid",
                )

                if success:
                    log.info(
                        "✅ Sent partner raid message to %s (raided by %s with %d viewers, network_raid_no=%d)",
                        to_broadcaster_login,
                        from_broadcaster_login,
                        viewer_count,
                        received_raid_count,
                    )
                else:
                    log.warning(
                        "Failed to send partner raid message to %s",
                        to_broadcaster_login,
                    )
            else:
                log.debug(
                    "Chat bot does not have _send_chat_message method, skipping partner raid message to %s",
                    to_broadcaster_login,
                )
        except Exception:
            log.exception(
                "Failed to send partner raid message to %s (raided by %s)",
                to_broadcaster_login,
                from_broadcaster_login,
            )

    def _get_received_network_raid_count(self, to_broadcaster_id: str) -> int:
        """
        Anzahl erfolgreicher, vom Raid-Bot geloggter Raids auf dieses Ziel.

        Die History enthält nur Raids, die von unseren Streamern über den Bot
        ausgeführt wurden; damit entspricht der Wert den erhaltenen Netzwerk-Raids.
        """
        target_id = str(to_broadcaster_id or "").strip()
        if not target_id:
            return 0

        try:
            with readonly_connection() as conn:
                row = conn.execute(
                    """
                    SELECT COUNT(*)
                    FROM twitch_raid_history
                    WHERE to_broadcaster_id = %s
                      AND COALESCE(success, FALSE) IS TRUE
                    """,
                    (target_id,),
                ).fetchone()
            return int(row[0]) if row and row[0] is not None else 0
        except Exception:
            log.debug(
                "Could not count received network raids for %s",
                target_id,
                exc_info=True,
            )
            return 0

    def _get_confirmed_external_recruitment_raid_count(self, to_broadcaster_id: str) -> int:
        target_id = str(to_broadcaster_id or "").strip()
        if not target_id:
            return 0

        try:
            with readonly_connection() as conn:
                row = conn.execute(
                    """
                    SELECT COUNT(*)
                    FROM twitch_confirmed_external_recruitment_raids
                    WHERE to_broadcaster_id = %s
                    """,
                    (target_id,),
                ).fetchone()
            return int(row[0]) if row and row[0] is not None else 0
        except Exception:
            log.debug(
                "Could not count confirmed external recruitment raids for %s",
                target_id,
                exc_info=True,
            )
            return 0

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
        target_id = str(to_broadcaster_id or "").strip()
        target_login = self._normalize_broadcaster_login(to_broadcaster_login)
        if not target_id or not target_login:
            return None

        normalized_flow_id = (
            str(raid_flow_id or "").strip()
            or self._next_raid_observability_flow_id(prefix="external-confirmed")
        )
        try:
            with transaction() as conn:
                conn.execute(
                    """
                    INSERT INTO twitch_confirmed_external_recruitment_raids (
                        raid_flow_id,
                        from_broadcaster_id,
                        from_broadcaster_login,
                        to_broadcaster_id,
                        to_broadcaster_login,
                        viewer_count,
                        confirmation_signal
                    )
                    VALUES (%s, %s, %s, %s, %s, %s, %s)
                    ON CONFLICT (raid_flow_id) DO NOTHING
                    """,
                    (
                        normalized_flow_id,
                        str(from_broadcaster_id or "").strip() or None,
                        self._normalize_broadcaster_login(from_broadcaster_login),
                        target_id,
                        target_login,
                        int(viewer_count or 0),
                        str(confirmation_signal or "").strip() or None,
                    ),
                )

                row = conn.execute(
                    """
                    SELECT COUNT(*)
                    FROM twitch_confirmed_external_recruitment_raids
                    WHERE to_broadcaster_id = %s
                    """,
                    (target_id,),
                ).fetchone()
            return int(row[0]) if row and row[0] is not None else 0
        except Exception:
            log.exception(
                "Failed to persist confirmed external recruitment raid for %s (%s)",
                target_login,
                target_id,
            )
            try:
                return self._get_confirmed_external_recruitment_raid_count(target_id)
            except Exception:
                return None

    def _is_target_currently_partner(
        self,
        *,
        target_id: str,
        target_login: str,
    ) -> bool:
        normalized_id = str(target_id or "").strip()
        normalized_login = self._normalize_broadcaster_login(target_login)
        if not normalized_id or not normalized_login:
            return False
        try:
            return self._is_partner_target_channel(
                broadcaster_id=normalized_id,
                broadcaster_login=normalized_login,
            )
        except Exception:
            log.debug(
                "Partner lookup failed for %s (%s)",
                normalized_login,
                normalized_id,
                exc_info=True,
            )
            return False

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
        if not from_broadcaster_id or days <= 0:
            return set()
        cutoff = f"{int(days)} days"
        try:
            with readonly_connection() as conn:
                rows = conn.execute(
                    """
                    SELECT DISTINCT to_broadcaster_id
                    FROM twitch_raid_history
                    WHERE from_broadcaster_id = %s
                      AND COALESCE(success, FALSE) IS TRUE
                      AND executed_at >= NOW() - (%s::interval)
                    """,
                    (from_broadcaster_id, cutoff),
                ).fetchall()
            return {str(row[0]) for row in rows if row and row[0]}
        except Exception:
            log.debug(
                "Failed to load recent raid targets for %s",
                from_broadcaster_id,
                exc_info=True,
            )
            return set()

    async def _attach_followers_totals(self, candidates: list[dict]) -> None:
        if not candidates or not self.session:
            return
        try:
            from ..api.twitch_api import TwitchAPI
        except Exception:
            return
        api = TwitchAPI(
            self.auth_manager.client_id,
            self.auth_manager.client_secret,
            session=self.session,
        )
        bot_token, _bot_id, bot_scopes = await self._resolve_bot_oauth_context()
        bot_scope_set = set(bot_scopes or set())
        candidate_labels = {
            str(candidate.get("user_id") or "").strip(): str(candidate.get("user_login") or "").strip().lower()
            for candidate in candidates
            if str(candidate.get("user_id") or "").strip()
        }

        async def _load_cached_totals(logins: tuple[str, ...]) -> dict[str, int]:
            if not logins:
                return {}
            try:
                placeholders = ",".join("%s" for _ in logins)
                with readonly_connection() as conn:
                    rows = conn.execute(
                        f"""
                        SELECT streamer_login, COALESCE(followers_end, followers_start) AS follower_total
                          FROM twitch_stream_sessions
                         WHERE streamer_login IN ({placeholders})
                           AND COALESCE(followers_end, followers_start) IS NOT NULL
                         ORDER BY COALESCE(ended_at, started_at) DESC
                        """,
                        logins,
                    ).fetchall()
            except Exception:
                log.debug("followers_totals: DB cache query failed", exc_info=True)
                return {}

            db_map: dict[str, int] = {}
            for row in rows:
                login = str(row[0] or "").strip().lower()
                if not login or login in db_map or row[1] is None:
                    continue
                db_map[login] = int(row[1])
            return db_map

        async def _resolve_user_token(user_id: str) -> str | None:
            try:
                return await self.auth_manager.get_valid_token(user_id, self.session)
            except Exception:
                return None

        async def _fetch_followers_total(user_id: str, user_token: str | None) -> int | None:
            label = candidate_labels.get(user_id) or user_id
            is_bot_path = bool(bot_token and user_token and user_token == bot_token)
            if is_bot_path:
                self._increment_raid_observability_counter(
                    "followers_candidate_bot_path_attempt_total"
                )
            else:
                self._warn_user_scope_fallback_once(
                    area="raid candidate follower lookup",
                    subject=label,
                )

            result = await self._get_followers_total_result_with_legacy_fallback(
                api,
                user_id,
                user_token=user_token,
            )
            if result.get("ok") and result.get("data") is not None:
                if is_bot_path:
                    self._clear_user_scope_fallback_warning(
                        area="raid candidate follower lookup",
                        subject=label,
                    )
                    self._increment_raid_observability_counter(
                        "followers_candidate_bot_path_success_total"
                    )
                else:
                    self._increment_raid_observability_counter(
                        "followers_candidate_reason_fallback_to_streamer_token_total"
                    )
                return int(result["data"])

            error_code = str(result.get("error_code") or "helix_followers_failed")
            if is_bot_path:
                self._increment_raid_observability_counter(
                    "followers_candidate_bot_path_failure_total"
                )
            else:
                self._increment_raid_observability_counter(
                    f"followers_candidate_reason_{error_code}_total"
                )
            return None

        await FollowerTotalEnricher(max_concurrency=8).enrich_candidates(
            candidates,
            load_cached_totals=_load_cached_totals,
            fetch_followers_total=_fetch_followers_total,
            resolve_user_token=_resolve_user_token,
            auth_context=FollowerAuthContext(
                bot_token=bot_token,
                bot_scopes=bot_scope_set,
            ),
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
        api = self._create_twitch_api()
        source_state = await self._resolve_manual_raid_source_state(
            broadcaster_id=broadcaster_id,
            broadcaster_login=broadcaster_login,
            api=api,
        )
        live_state = dict(source_state.get("live_state") or {})
        if str(source_state.get("status") or "") == "source_not_live":
            log.info(
                "Manual raid skipped for %s: broadcaster is not live (source=%s)",
                broadcaster_login,
                source_state.get("state_source") or "unknown",
            )
            return {
                "status": "source_not_live",
                "reason": str(source_state.get("state_source") or ""),
            }

        last_game = str(live_state.get("last_game") or "").strip()
        had_deadlock_session = bool(live_state.get("had_deadlock_in_session", False))
        last_deadlock_seen_at = (
            str(live_state.get("last_deadlock_seen_at") or "").strip() or None
        )
        source_evaluation = self._evaluate_deadlock_raid_source(
            current_game=last_game,
            had_deadlock_session=had_deadlock_session,
            last_deadlock_seen_at=last_deadlock_seen_at,
        )
        if not bool(source_evaluation.get("eligible")):
            log.info(
                "Manual raid skipped for %s: source not Deadlock-eligible (reason=%s, current_game=%s, had_deadlock_session=%s, last_deadlock_seen_at=%s, source=%s)",
                broadcaster_login,
                source_evaluation.get("reason") or "unknown",
                last_game or "unbekannt",
                had_deadlock_session,
                last_deadlock_seen_at or "none",
                source_state.get("state_source") or "unknown",
            )
            return {
                "status": "source_not_eligible",
                "reason": str(source_evaluation.get("reason") or ""),
            }

        viewer_count = self._safe_int(live_state.get("last_viewer_count"), 0)
        stream_duration_sec = self._calculate_stream_duration_sec(
            str(live_state.get("last_started_at") or "").strip() or None
        )

        partner_rows = self._load_partner_roster_for_raid(broadcaster_id)
        streams_by_login = await self._fetch_streams_by_logins_for_raid(
            [str(row.get("twitch_login") or "") for row in partner_rows],
            api=api,
        )
        online_partners = self._build_online_partner_candidates(partner_rows, streams_by_login)
        eligible_partners, filtered_out = self._filter_deadlock_eligible_partner_candidates(
            online_partners
        )

        log.info(
            "Manual raid pipeline started for %s (id=%s): viewers=%d, stream_duration=%ds, online_partners=%d, eligible_partners=%d",
            broadcaster_login,
            broadcaster_id,
            viewer_count,
            stream_duration_sec,
            len(online_partners),
            len(eligible_partners),
        )
        if filtered_out:
            log.debug(
                "Manual raid: Partner ausgeschlossen (Kategorie/Session): %s",
                "; ".join(filtered_out),
            )

        category_id = await self._resolve_target_category_id(api)
        return await self._execute_raid_pipeline(
            broadcaster_id=broadcaster_id,
            broadcaster_login=broadcaster_login,
            viewer_count=viewer_count,
            stream_duration_sec=stream_duration_sec,
            online_partners=eligible_partners,
            api=api,
            category_id=category_id,
            reason="manual_chat_command",
            set_manual_suppression=True,
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
        """
        Wird aufgerufen, wenn ein Streamer offline geht.
        Versucht automatisch zu raiden, falls möglich.

        Features:
        - Auto-Retry bei Fehlern (z.B. Ziel hat Raids deaktiviert)
        - Blacklist-Management für nicht raidbare Kanäle
        """
        flow_start_ts = offline_trigger_ts if offline_trigger_ts is not None else time.monotonic()
        offline_trigger_ts = flow_start_ts

        # Prüfen, ob Auto-Raid durch manuellen Raid unterdrückt ist
        if self.is_offline_auto_raid_suppressed(broadcaster_id):
            log.info(
                "Auto-raid suppressed for %s (manual raid detected recently)",
                broadcaster_login,
            )
            return None

        # Prüfen, ob Streamer Auto-Raid aktiviert hat
        with readonly_connection() as conn:
            eligibility = load_offline_auto_raid_eligibility(
                conn,
                twitch_user_id=broadcaster_id,
            )

        if not eligibility.active_partner and not eligibility.auth_row_found:
            log.debug("Streamer %s not found in DB", broadcaster_login)
            return None

        if not eligibility.active_partner:
            log.debug("Raid bot disabled for %s (not active partner)", broadcaster_login)
            return None

        if not eligibility.raid_bot_enabled:
            log.debug("Raid bot disabled for %s (setting)", broadcaster_login)
            return None
        if not eligibility.raid_auth_enabled:
            log.debug("Raid bot disabled for %s (no auth)", broadcaster_login)
            return None

        log.info(
            "Auto-raid pipeline started for %s (id=%s): viewers=%d, stream_duration=%ds, online_partners=%d",
            broadcaster_login,
            broadcaster_id,
            viewer_count,
            stream_duration_sec,
            len(online_partners),
        )
        result = await self._execute_raid_pipeline(
            broadcaster_id=broadcaster_id,
            broadcaster_login=broadcaster_login,
            viewer_count=viewer_count,
            stream_duration_sec=stream_duration_sec,
            online_partners=online_partners,
            api=api,
            category_id=category_id,
            offline_trigger_ts=offline_trigger_ts,
            reason="auto_raid_on_offline",
        )
        if str(result.get("status") or "") == "started":
            return str(result.get("target_login") or "") or None
        return None
