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
from datetime import UTC, datetime  # noqa: F401
from typing import Any

import aiohttp

from ..core.constants import TWITCH_TARGET_GAME_NAME
from . import raid_dependencies as _raid_dependencies
from .raid_dependencies import RaidRuntimeDeps, build_default_raid_runtime_deps
from .scope_profiles import BASE_STREAMER_SCOPES
from .data_setup_facade import RaidDataSetupFacadeMixin
from .delivery_selection_facade import RaidDeliverySelectionFacadeMixin
from .lifecycle import RaidBotLifecycle
from .pending_raids import PendingRaid
from .runtime_support import create_twitch_api
from .runtime_core_facade import RaidRuntimeCoreFacadeMixin
from .tracking_arrival_facade import RaidTrackingArrivalFacadeMixin

# Legacy compatibility exports for patch-based tests during the dependency-container migration.
readonly_connection = _raid_dependencies.readonly_connection
transaction = _raid_dependencies.transaction
load_partner_raid_score_map = _raid_dependencies.load_partner_raid_score_map
refresh_partner_raid_score_async = _raid_dependencies.refresh_partner_raid_score_async
track_confirmed_partner_raid = _raid_dependencies.track_confirmed_partner_raid

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


class RaidBot(
    RaidTrackingArrivalFacadeMixin,
    RaidDeliverySelectionFacadeMixin,
    RaidDataSetupFacadeMixin,
    RaidRuntimeCoreFacadeMixin,
):
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
        deps: RaidRuntimeDeps | None = None,
    ):
        self.auth_manager = RaidAuthManager(client_id, client_secret, redirect_uri)
        self.raid_executor = RaidExecutor(client_id, self.auth_manager)
        self._session = session
        self._deps = deps or build_default_raid_runtime_deps()
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

    def _get_target_game_lower(self) -> str:
        return str(TWITCH_TARGET_GAME_NAME or "").strip().lower()

    def _create_twitch_api(self, *, session=None):
        return create_twitch_api(self, session=session)


