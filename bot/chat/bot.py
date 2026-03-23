# cogs/twitch/twitch_chat_bot.py
"""
Twitch IRC Chat Bot für Twitch-Bot-Steuerung.

Streamer können den Twitch-Bot direkt über Twitch-Chat-Commands steuern:
- !raid_enable / !raidbot - Aktiviert Auto-Raids
- !raid_disable / !raidbot_off - Deaktiviert Auto-Raids
- !raid_status - Zeigt den Status an
- !raid_history - Zeigt die letzten Raids
- !clip - Erstellt einen Clip und postet den Link
"""

import asyncio
import copy
import logging
import os
import random
import time
from collections import deque
from datetime import UTC, datetime

import discord

from ..api.token_manager import TwitchBotTokenManager
from ..core.constants import TWITCH_NOTIFY_CHANNEL_ID, TWITCH_TARGET_GAME_NAME
from ..logging_setup import ensure_twitch_logger_file_handler, log_path
from ..storage import get_conn, insert_observability_event
from .commands import RaidCommandsMixin
from .connection import ConnectionMixin
from .constants import (
    CHAT_JOIN_OFFLINE,
    _PROMO_ACTIVITY_ENABLED,
    PROMO_LOOP_INTERVAL_SEC,
    PROMO_MESSAGES,
    PROMO_VIEWER_SPIKE_ENABLED,
    SPAM_MIN_MATCHES,
    TWITCHIO_AVAILABLE,
    WHITELISTED_BOTS,
    twitchio_commands,
    twitchio_web,
)
from .moderation import ModerationMixin
from .promos import PromoMixin
from .service_pitch_warning import ServicePitchWarningMixin
from .tokens import (
    _KEYRING_SERVICE,
    TokenPersistenceMixin,
    load_bot_tokens,
)


# Dedizierter Twitch-Logger Setup
def _setup_twitch_logging():
    twitch_log = ensure_twitch_logger_file_handler()
    twitch_log.propagate = True  # Auch weiterhin im Master-Log behalten


_setup_twitch_logging()
log = logging.getLogger("TwitchStreams.ChatBot")


if TWITCHIO_AVAILABLE:
    from twitchio.authentication.scopes import Scopes, _scope_property
    from twitchio.eventsub.websockets import Websocket

    def _install_twitchio_scope_compat() -> tuple[str, ...]:
        """
        Add missing scope descriptors for newer Twitch scopes on older TwitchIO builds.

        Twitch occasionally adds scopes before the installed TwitchIO version knows about
        them. In that case `Scopes([...])` raises `AttributeError` during token registration
        or refresh handling even though the token itself is valid.
        """
        added: list[str] = []
        for scope_name in (
            "moderator_manage_suspicious_users",
        ):
            if scope_name in vars(Scopes):
                continue
            descriptor = _scope_property()
            descriptor.__set_name__(Scopes, scope_name)
            type.__setattr__(Scopes, scope_name, descriptor)
            added.append(scope_name)
        return tuple(added)

    _TWITCHIO_SCOPE_COMPAT = _install_twitchio_scope_compat()
    if _TWITCHIO_SCOPE_COMPAT:
        log.warning(
            "Installed TwitchIO scope compatibility shim for: %s",
            ", ".join(scope.replace("_", ":", 2) for scope in _TWITCHIO_SCOPE_COMPAT),
        )

    class RaidChatBot(
        TokenPersistenceMixin,
        ModerationMixin,
        ServicePitchWarningMixin,
        PromoMixin,
        ConnectionMixin,
        RaidCommandsMixin,
        twitchio_commands.Bot,
    ):
        """Twitch IRC Bot für Raid-Commands im Chat."""

        # TwitchIO 3.x Component compatibility: guards list expected by Command._run_guards
        __all_guards__: list = []

        async def component_before_invoke(self, ctx) -> None:
            """TwitchIO 3.x Component hook stub – required when _injected is set on commands."""
            pass

        async def component_command_error(self, payload) -> None:
            """TwitchIO 3.x Component hook stub – required when _injected is set on commands."""
            pass

        async def component_after_invoke(self, ctx) -> None:
            """TwitchIO 3.x Component hook stub – required when _injected is set on commands."""
            pass

        def __init__(
            self,
            token: str,
            client_id: str,
            client_secret: str,
            bot_id: str | None = None,
            prefix: str = "!",
            initial_channels: list | None = None,
            refresh_token: str | None = None,
            web_adapter: object | None = None,
            token_manager: TwitchBotTokenManager | None = None,
        ):
            # In 3.x ist bot_id ein positionales/keyword Argument in Client, aber REQUIRED in Bot
            base_kwargs = {"adapter": web_adapter} if web_adapter is not None else {}
            # Speichere bot_id als Instanzvariable BEVOR wir super().__init__ aufrufen
            self._bot_id_stored = bot_id
            super().__init__(
                client_id=client_id,
                client_secret=client_secret,
                bot_id=bot_id
                or "",  # Fallback auf leeren String falls None (für TwitchIO Kompatibilität)
                prefix=prefix,
                case_insensitive=True,
                **base_kwargs,
            )
            self.prefix = prefix
            self._client_id = client_id
            self._bot_token = token
            self._bot_refresh_token = refresh_token
            self._token_manager = token_manager
            if self._token_manager:
                self._token_manager.set_refresh_callback(self._on_token_manager_refresh)
            self._raid_bot = None  # Wird später gesetzt
            self._initial_channels = initial_channels or []
            self._monitored_streamers: set[str] = set()
            self._channel_subscription_types: dict[str, set[str]] = {}
            self._channel_subscription_state: dict[str, dict[str, dict[str, str]]] = {}
            self._session_cache: dict[str, tuple[int, datetime]] = {}
            self._last_autoban: dict[str, dict[str, str]] = {}
            # Cooldown: Verhindert, dass _ensure_bot_is_mod auf einem
            # gebannten Channel sekundlich wiederholt wird.
            # Key = channel_login (lowercase), Value = nächster erlaubter Zeitpunkt.
            self._mod_retry_cooldown: dict[str, datetime] = {}
            self._autoban_log = log_path("twitch_autobans.log")
            self._suspicious_log = log_path("twitch_suspicious.log")
            self._init_service_pitch_warning()
            self._target_game_lower = (TWITCH_TARGET_GAME_NAME or "").strip().lower()
            # Cache for category checks in chat tracking (login -> (monotonic_ts, is_target_game))
            self._chat_category_cache: dict[str, tuple[float, bool]] = {}
            self._chat_category_cache_ttl_sec = 15.0
            # Periodische Chat-Promos
            self._channel_ids: dict[str, str] = {}  # login -> broadcaster_id
            self._last_promo_sent: dict[str, float] = {}  # login -> monotonic timestamp
            self._last_promo_attempt: dict[str, float] = {}  # login -> monotonic timestamp
            self._last_raw_chat_message_ts: dict[str, float] = {}
            self._raw_msg_count_since_promo: dict[str, int] = {}
            self._promo_activity: dict[str, deque[tuple[float, str]]] = {}
            self._promo_chatter_dedupe: dict[str, dict[str, float]] = {}
            self._last_promo_viewer_spike: dict[str, float] = {}
            self._promo_task: asyncio.Task | None = None
            self._last_invite_reply: dict[str, float] = {}
            self._last_invite_reply_user: dict[tuple[str, str], float] = {}
            self._fun_reply_cd: dict[str, float] = {}
            self._bot_promo_cd: dict[str, float] = {}
            # Kurzantworten auf "Danke" vorerst deaktiviert (kann später wieder aktiviert werden).
            self._fun_thanks_reply_enabled = False
            self._discord_bot: discord.Client | None = None
            self._discord_invite_channel_id: int | None = None
            self._promo_invite_cache: dict[str, str] = {}
            self._monitored_only_channels: set[str] = set()
            self._restart_lock = asyncio.Lock()
            self._restart_task: asyncio.Task | None = None
            self._restart_cooldown_until: float = 0.0
            self._restart_cooldown_seconds: float = 30.0
            self._restart_transport_ready_attempts: int = 3
            self._restart_transport_ready_backoff_seconds: float = 2.0
            self._restart_rejoin_retry_attempts: int = 2
            self._restart_rejoin_retry_backoff_seconds: float = 5.0
            self._skip_initial_join_once: bool = False
            self._managed_start_with_adapter: bool | None = None
            self._managed_load_tokens: bool = False
            self._managed_save_tokens: bool = False
            self._register_inline_commands()
            log.info(
                "Twitch Chat Bot initialized with %d initial channels",
                len(self._initial_channels),
            )

        def set_monitored_channels(self, channels: list[str]) -> None:
            """Set the list of read-only monitored channels."""
            for ch in channels:
                normalized = str(ch or "").strip().lower().lstrip("#")
                if normalized:
                    self._monitored_only_channels.add(normalized)
                    if normalized not in self._initial_channels:
                        self._initial_channels.append(normalized)

        def _is_monitored_only(self, channel_name: str) -> bool:
            return channel_name.lower() in self._monitored_only_channels

        @staticmethod
        def _bounded_runtime_sample(values: object, *, limit: int = 8) -> list[str]:
            if isinstance(values, dict):
                source = values.keys()
            elif isinstance(values, (set, list, tuple)):
                source = values
            else:
                return []
            normalized = [
                str(value or "").strip().lower().lstrip("#")
                for value in source
                if str(value or "").strip()
            ]
            return sorted(dict.fromkeys(normalized))[:limit]

        def _snapshot_chat_runtime_state(self) -> dict[str, object]:
            websocket_objects = self._iter_eventsub_websockets()
            websocket_sessions = [
                str(getattr(websocket, "session_id", "") or "").strip()
                for websocket in websocket_objects
                if str(getattr(websocket, "session_id", "") or "").strip()
            ]
            websocket_connected_count = sum(
                1
                for websocket in websocket_objects
                if bool(getattr(websocket, "connected", False))
                and str(getattr(websocket, "session_id", "") or "").strip()
            )
            return {
                "monitored_count": len(getattr(self, "_monitored_streamers", set()) or ()),
                "subscription_entry_count": len(
                    getattr(self, "_channel_subscription_types", {}) or {}
                ),
                "subscription_state_entry_count": len(
                    getattr(self, "_channel_subscription_state", {}) or {}
                ),
                "channel_id_count": len(getattr(self, "_channel_ids", {}) or {}),
                "initial_channel_count": len(getattr(self, "_initial_channels", []) or []),
                "monitored_only_count": len(
                    getattr(self, "_monitored_only_channels", set()) or ()
                ),
                "websocket_transport_count": len(websocket_objects),
                "websocket_connected_count": websocket_connected_count,
                "monitored_sample": self._bounded_runtime_sample(
                    getattr(self, "_monitored_streamers", set())
                ),
                "subscription_sample": self._bounded_runtime_sample(
                    getattr(self, "_channel_subscription_types", {})
                ),
                "channel_id_sample": self._bounded_runtime_sample(getattr(self, "_channel_ids", {})),
                "initial_channel_sample": self._bounded_runtime_sample(
                    getattr(self, "_initial_channels", [])
                ),
                "monitored_only_sample": self._bounded_runtime_sample(
                    getattr(self, "_monitored_only_channels", set())
                ),
                "websocket_session_sample": self._bounded_runtime_sample(websocket_sessions),
            }

        def _iter_eventsub_websockets(self, *, token_for: str | None = None) -> list[object]:
            websockets = getattr(self, "_websockets", None)
            if not isinstance(websockets, dict):
                return []

            pairs: list[dict[str, object]] = []
            if token_for:
                candidate = websockets.get(token_for)
                if isinstance(candidate, dict):
                    pairs.append(candidate)
            else:
                pairs.extend(pair for pair in websockets.values() if isinstance(pair, dict))

            collected: list[object] = []
            for pair in pairs:
                collected.extend(pair.values())
            return collected

        def _find_connected_eventsub_websocket(self, *, token_for: str) -> object | None:
            for websocket in self._iter_eventsub_websockets(token_for=token_for):
                if bool(getattr(websocket, "connected", False)) and str(
                    getattr(websocket, "session_id", "") or ""
                ).strip():
                    return websocket
            return None

        def get_observability_snapshot(self) -> dict[str, object]:
            restart_task = getattr(self, "_restart_task", None)
            return {
                "runtime": self._snapshot_chat_runtime_state(),
                "counters": dict(getattr(self, "_chat_observability_counter_store", {}) or {}),
                "last_join_diagnostic": getattr(self, "_last_chat_join_diagnostic", None),
                "last_runtime_snapshot": getattr(self, "_last_chat_runtime_snapshot", None),
                "restart_task_pending": bool(restart_task and not restart_task.done()),
            }

        def _log_chat_runtime_snapshot(
            self,
            *,
            flow_id: str,
            phase: str,
            reason: str,
            failed_channel: str | None = None,
            channel_list: list[str] | None = None,
            level: int = logging.INFO,
            **extra_fields: object,
        ) -> None:
            payload = {
                "flow_id": flow_id,
                "phase": str(phase or "").strip() or "unknown",
                "reason": str(reason or "").strip() or "unknown",
                "failed_channel": str(failed_channel or "").strip().lower().lstrip("#") or None,
                "channel_list_count": len(channel_list or []),
                "channel_list_sample": self._bounded_runtime_sample(channel_list or []),
                "skip_initial_join_once": bool(getattr(self, "_skip_initial_join_once", False)),
                **self._snapshot_chat_runtime_state(),
                **extra_fields,
            }
            self._last_chat_runtime_snapshot = payload
            log.log(
                level,
                "chat_runtime_snapshot %s",
                self._format_chat_observability_fields(**payload),
            )
            insert_observability_event(
                flow_type="chat_runtime",
                flow_id=flow_id,
                entity_login=payload.get("failed_channel"),
                step=str(payload.get("phase") or "runtime_snapshot"),
                decision=str(payload.get("reason") or "unknown"),
                details=payload,
            )

        def configure_managed_start(
            self,
            *,
            with_adapter: bool,
            load_tokens: bool = False,
            save_tokens: bool = False,
        ) -> None:
            """Persist start options so the bot can restart itself after transport failures."""
            self._managed_start_with_adapter = bool(with_adapter)
            self._managed_load_tokens = bool(load_tokens)
            self._managed_save_tokens = bool(save_tokens)

        def _reset_managed_transport_restart_state(self) -> None:
            """
            Reset TwitchIO client internals before reusing the same bot instance.

            TwitchIO clients are not designed around repeated start/close cycles on the
            same object. After a managed restart we must drop cached EventSub websocket
            state and reopen the client lifecycle flags, otherwise subscribe_websocket()
            may reuse stale transport sessions from the previous run.
            """
            websockets = getattr(self, "_websockets", None)
            if isinstance(websockets, dict):
                websockets.clear()

            if hasattr(self, "_login_called"):
                self._login_called = False
            if hasattr(self, "_has_closed"):
                self._has_closed = False
            ready_event = getattr(self, "_ready_event", None)
            if ready_event is not None and hasattr(ready_event, "clear"):
                ready_event.clear()

        async def _ensure_eventsub_transport_ready(
            self,
            *,
            flow_id: str,
            reason: str,
            channel_list: list[str],
        ) -> bool:
            token_for = str(getattr(self, "bot_id", "") or "").strip()
            if not token_for:
                return True

            websockets = getattr(self, "_websockets", None)
            if not isinstance(websockets, dict):
                return True

            attempts = max(1, int(getattr(self, "_restart_transport_ready_attempts", 3) or 3))
            base_backoff = max(
                0.25,
                float(getattr(self, "_restart_transport_ready_backoff_seconds", 2.0) or 2.0),
            )
            last_error: Exception | None = None

            for attempt in range(1, attempts + 1):
                existing = self._find_connected_eventsub_websocket(token_for=token_for)
                if existing is not None:
                    self._log_chat_runtime_snapshot(
                        flow_id=flow_id,
                        phase="restart_transport_ready",
                        reason=reason,
                        channel_list=channel_list,
                        level=logging.INFO,
                        transport_attempt=attempt,
                    )
                    return True

                pair = websockets.get(token_for)
                if not isinstance(pair, dict):
                    pair = {}
                    websockets[token_for] = pair

                stale_ids = [
                    socket_id
                    for socket_id, websocket in list(pair.items())
                    if not bool(getattr(websocket, "connected", False))
                    or not str(getattr(websocket, "session_id", "") or "").strip()
                ]
                for socket_id in stale_ids:
                    pair.pop(socket_id, None)

                try:
                    websocket = Websocket(client=self, token_for=token_for, http=self._http)
                    await websocket.connect(fail_once=True)
                    session_id = str(getattr(websocket, "session_id", "") or "").strip()
                    if not session_id:
                        raise RuntimeError("eventsub websocket connected without session_id")
                    pair[session_id] = websocket
                    self._log_chat_runtime_snapshot(
                        flow_id=flow_id,
                        phase="restart_transport_connected",
                        reason=reason,
                        channel_list=channel_list,
                        level=logging.INFO,
                        transport_attempt=attempt,
                    )
                    return True
                except Exception as exc:
                    last_error = exc
                    self._log_chat_runtime_snapshot(
                        flow_id=flow_id,
                        phase="restart_transport_connect_retry",
                        reason=reason,
                        channel_list=channel_list,
                        level=logging.WARNING if attempt == attempts else logging.INFO,
                        transport_attempt=attempt,
                        transport_error=str(exc)[:200],
                    )
                    if attempt < attempts:
                        await asyncio.sleep(base_backoff * attempt)

            log.warning(
                "Chat transport restart: EventSub websocket not ready after %d attempt(s); skipping rejoin batch.",
                attempts,
                exc_info=last_error,
            )
            return False

        async def request_transport_restart(
            self,
            *,
            reason: str,
            failed_channel: str | None = None,
        ) -> bool:
            """
            Schedule a throttled bot restart after a broken EventSub chat transport.

            Returns True if a restart was scheduled, False if one is already pending or throttled.
            """
            now = time.monotonic()
            flow_id = self._next_chat_observability_flow_id(prefix="restart")
            if self._restart_task and not self._restart_task.done():
                self._increment_chat_observability_counter("chat_transport_restart_duplicate_total")
                self._log_chat_runtime_snapshot(
                    flow_id=flow_id,
                    phase="restart_request_duplicate",
                    reason=reason,
                    failed_channel=failed_channel,
                    level=logging.INFO,
                    restart_task_pending=True,
                )
                log.info(
                    "Chat transport restart already pending; ignoring duplicate request (%s).",
                    reason,
                )
                return False
            if now < self._restart_cooldown_until:
                remaining = max(0.0, self._restart_cooldown_until - now)
                self._increment_chat_observability_counter("chat_transport_restart_throttled_total")
                self._log_chat_runtime_snapshot(
                    flow_id=flow_id,
                    phase="restart_request_throttled",
                    reason=reason,
                    failed_channel=failed_channel,
                    level=logging.INFO,
                    cooldown_remaining_ms=int(remaining * 1000),
                )
                log.info(
                    "Chat transport restart throttled for %.1fs (%s).",
                    remaining,
                    reason,
                )
                return False

            channel_candidates = {
                self._normalize_channel_login(channel)
                for channel in (
                    list(getattr(self, "_initial_channels", []) or [])
                    + list(getattr(self, "_monitored_streamers", set()) or set())
                    + list(getattr(self, "_monitored_only_channels", set()) or set())
                )
            }
            channel_list = sorted(channel for channel in channel_candidates if channel)
            normalized_failed = self._normalize_channel_login(failed_channel or "")
            if normalized_failed and normalized_failed not in channel_list:
                channel_list.append(normalized_failed)

            self._restart_cooldown_until = now + self._restart_cooldown_seconds
            self._increment_chat_observability_counter("chat_transport_restart_total")
            self._log_chat_runtime_snapshot(
                flow_id=flow_id,
                phase="restart_request_scheduled",
                reason=reason,
                failed_channel=normalized_failed,
                channel_list=channel_list,
                level=logging.WARNING,
                cooldown_remaining_ms=int(self._restart_cooldown_seconds * 1000),
            )
            self._restart_task = asyncio.create_task(
                self._restart_after_transport_failure(
                    channel_list=channel_list,
                    reason=reason,
                    flow_id=flow_id,
                    failed_channel=normalized_failed or None,
                ),
                name="twitch.chat_bot.transport_restart",
            )
            return True

        async def _restart_after_transport_failure(
            self,
            *,
            channel_list: list[str],
            reason: str,
            flow_id: str,
            failed_channel: str | None = None,
        ) -> None:
            async with self._restart_lock:
                self._log_chat_runtime_snapshot(
                    flow_id=flow_id,
                    phase="restart_begin",
                    reason=reason,
                    failed_channel=failed_channel,
                    channel_list=channel_list,
                    level=logging.WARNING,
                )
                log.warning(
                    "Restarting Twitch Chat Bot after broken EventSub transport (%s). channels=%d",
                    reason,
                    len(channel_list),
                )

                self._monitored_streamers.clear()
                self._channel_subscription_types.clear()
                self._channel_subscription_state.clear()
                self._channel_ids.clear()
                self._log_chat_runtime_snapshot(
                    flow_id=flow_id,
                    phase="restart_caches_cleared",
                    reason=reason,
                    failed_channel=failed_channel,
                    channel_list=channel_list,
                    level=logging.INFO,
                )

                try:
                    await self.close()
                except Exception:
                    log.exception("Chat transport restart: close() failed")

                await asyncio.sleep(2.0)

                with_adapter = self._managed_start_with_adapter
                if with_adapter is None:
                    with_adapter = getattr(self, "adapter", None) is not None

                try:
                    self._reset_managed_transport_restart_state()
                    self._skip_initial_join_once = True
                    asyncio.create_task(
                        self.start(
                            with_adapter=with_adapter,
                            load_tokens=self._managed_load_tokens,
                            save_tokens=self._managed_save_tokens,
                        ),
                        name="twitch.chat_bot.restart_start",
                    )
                    self._log_chat_runtime_snapshot(
                        flow_id=flow_id,
                        phase="restart_start_scheduled",
                        reason=reason,
                        failed_channel=failed_channel,
                        channel_list=channel_list,
                        level=logging.INFO,
                        with_adapter=with_adapter,
                    )
                except Exception:
                    log.exception("Chat transport restart: could not schedule bot start")
                    return

                if channel_list:
                    asyncio.create_task(
                        self._rejoin_channels_after_restart(
                            channel_list,
                            flow_id=flow_id,
                            reason=reason,
                        ),
                        name="twitch.chat_bot.restart_rejoin",
                    )

        async def _rejoin_channels_after_restart(
            self,
            channels: list[str],
            *,
            flow_id: str,
            reason: str,
            transport_retry: int = 0,
        ) -> None:
            try:
                await self.wait_until_ready()
            except Exception:
                self._log_chat_runtime_snapshot(
                    flow_id=flow_id,
                    phase="restart_wait_until_ready_failed",
                    reason=reason,
                    channel_list=channels,
                    level=logging.ERROR,
                )
                log.exception("Chat transport restart: wait_until_ready failed during rejoin")
                return

            normalized = []
            seen: set[str] = set()
            for channel in channels:
                login = self._normalize_channel_login(channel)
                if not login or login in seen:
                    continue
                seen.add(login)
                normalized.append(login)

            if not normalized:
                self._log_chat_runtime_snapshot(
                    flow_id=flow_id,
                    phase="restart_rejoin_skipped",
                    reason=reason,
                    channel_list=channels,
                    level=logging.INFO,
                    rejoin_candidates=0,
                )
                return

            transport_ready = await self._ensure_eventsub_transport_ready(
                flow_id=flow_id,
                reason=reason,
                channel_list=normalized,
            )
            if not transport_ready:
                self._log_chat_runtime_snapshot(
                    flow_id=flow_id,
                    phase="restart_rejoin_deferred",
                    reason=reason,
                    channel_list=normalized,
                    level=logging.WARNING,
                    rejoin_candidates=len(normalized),
                    transport_retry=transport_retry,
                )
                retry_attempts = max(
                    0,
                    int(getattr(self, "_restart_rejoin_retry_attempts", 2) or 0),
                )
                if transport_retry < retry_attempts:
                    delay = max(
                        0.25,
                        float(getattr(self, "_restart_rejoin_retry_backoff_seconds", 5.0) or 5.0),
                    ) * (transport_retry + 1)
                    await asyncio.sleep(delay)
                    await self._rejoin_channels_after_restart(
                        normalized,
                        flow_id=flow_id,
                        reason=reason,
                        transport_retry=transport_retry + 1,
                    )
                return

            try:
                self._increment_chat_observability_counter("chat_rejoin_attempt_total")
                joined = await self.join_channels(
                    normalized,
                    rate_limit_delay=0.35,
                    mark_monitored_only=False,
                )
                self._increment_chat_observability_counter("chat_rejoin_success_total", joined)
                self._log_chat_runtime_snapshot(
                    flow_id=flow_id,
                    phase="restart_rejoin_finished",
                    reason=reason,
                    channel_list=normalized,
                    level=logging.INFO,
                    rejoin_candidates=len(normalized),
                    rejoin_joined=joined,
                    rejoin_failed=max(0, len(normalized) - joined),
                )
                log.info(
                    "Chat transport restart: rejoin finished (%d/%d).",
                    joined,
                    len(normalized),
                )
            except Exception:
                self._log_chat_runtime_snapshot(
                    flow_id=flow_id,
                    phase="restart_rejoin_failed",
                    reason=reason,
                    channel_list=normalized,
                    level=logging.ERROR,
                    rejoin_candidates=len(normalized),
                )
                log.exception("Chat transport restart: rejoin failed")

        def _is_partner_channel_for_chat_tracking(self, login: str) -> bool:
            """
            Überschreibt die Standard-Logik:
            Erlaubt Chat-Tracking sowohl für verifizierte Partner ALS AUCH für Monitored-Only Channels.
            """
            if self._is_monitored_only(login):
                return True
            return super()._is_partner_channel_for_chat_tracking(login)

        def _is_deadlock_live(self, login: str) -> bool:
            """True, wenn Channel gerade Deadlock spielt (für Chat-Antworten/Logging)."""
            try:
                session_id = self._resolve_session_id(login)
                return bool(self._is_target_game_live_for_chat(login, session_id))
            except Exception:
                return False

        @staticmethod
        def _cooldown_ok(store: dict[str, float], key: str, seconds: float) -> bool:
            now = time.monotonic()
            last = store.get(key, 0.0)
            if now - last < seconds:
                return False
            store[key] = now
            return True

        @staticmethod
        def _is_twitchio_scope_compat_error(exc: Exception) -> bool:
            msg = str(exc or "")
            return (
                "Scopes" in msg
                and "has no attribute" in msg
                and "suspicious_users" in msg
            )

        @staticmethod
        def _looks_like_bot_question(text: str) -> bool:
            """Heuristik für 'was ist das für ein Bot / wer hat ihn gebaut'."""
            if not text:
                return False
            t = text.lower()
            return " bot" in t and any(word in t for word in ("wer", "was", "gebaut", "gemacht"))

        async def _maybe_fun_responses(self, message, channel_login: str) -> None:
            """Freche Kurz-Antworten (Danke/Bot-Fragen) – nur wenn Deadlock live."""
            content = message.content or ""
            raw = content.strip()
            if not raw:
                return
            if raw.startswith(self.prefix or "!"):
                return
            low = raw.lower()
            channel = getattr(message, "channel", None)
            if channel is None:
                return

            # Danke-Trigger
            if self._fun_thanks_reply_enabled:
                thanks_hits = any(word in low for word in ("danke", "thanks", "thx", "merci", "ty"))
                if thanks_hits and "http" not in low:
                    if self._cooldown_ok(self._fun_reply_cd, channel_login, 90.0):
                        reply = random.choice(
                            [
                                "Danke, ich wusste ja, dass ich gut bin. WiltedRose",
                                "Oh stop it, you :relaxed:",
                            ]
                        )
                        await self._send_chat_message(channel, reply)

            # Bot-Promo / Herkunft
            if self._looks_like_bot_question(low):
                if self._cooldown_ok(self._bot_promo_cd, channel_login, 300.0):
                    await self._send_chat_message(
                        channel,
                        "Gebaut von EarlySalty; Beschwerden & Liebesbriefe an https://twitch.tv/EarlySalty – follow da!",
                    )

        def _register_inline_commands(self) -> None:
            """Register @command methods on the Bot class (TwitchIO 3.x does not auto-register)."""
            for cls in self.__class__.mro():
                for _, value in cls.__dict__.items():
                    if not isinstance(value, twitchio_commands.Command):
                        continue
                    if value.name in self.commands:
                        continue
                    cmd = copy.copy(value)
                    cmd._injected = self
                    try:
                        self.add_command(cmd)
                    except Exception:
                        log.debug(
                            "Konnte Command nicht registrieren: %s",
                            cmd.name,
                            exc_info=True,
                        )

        @property
        def bot_id_safe(self) -> str | None:
            """Gibt eine sichere bot_id zurück (None statt leerer String)."""
            # Prüfe zuerst die gespeicherte ID
            if self._bot_id_stored and str(self._bot_id_stored).strip():
                return str(self._bot_id_stored)
            # Fallback auf die TwitchIO bot_id Property
            bot_id = getattr(self, "bot_id", None)
            if bot_id and str(bot_id).strip():
                return str(bot_id)
            return None

        def set_raid_bot(self, raid_bot):
            """Setzt die RaidBot-Instanz für OAuth-URLs."""
            self._raid_bot = raid_bot

        def set_discord_bot(
            self,
            discord_bot: discord.Client | None,
            *,
            invite_channel_id: int | None = None,
        ) -> None:
            """Assign the Discord bot instance for promo invite creation."""
            self._discord_bot = discord_bot
            channel_id: int | None = None
            if invite_channel_id:
                try:
                    channel_id = int(invite_channel_id)
                except (TypeError, ValueError):
                    channel_id = None
            if not channel_id:
                try:
                    default_id = int(TWITCH_NOTIFY_CHANNEL_ID or 0)
                except (TypeError, ValueError):
                    default_id = 0
                if default_id:
                    channel_id = default_id
            self._discord_invite_channel_id = channel_id
            log.info(
                "Discord bot set for promo invites (channel_id=%s)",
                str(channel_id) if channel_id else "-",
            )

        def _load_streamer_invite_from_db(self, login: str) -> str | None:
            login_norm = (login or "").strip().lower()
            if not login_norm:
                return None
            try:
                with get_conn() as conn:
                    row = conn.execute(
                        """
                        SELECT invite_url, invite_code
                          FROM twitch_streamer_invites
                         WHERE streamer_login = ?
                        """,
                        (login_norm,),
                    ).fetchone()
                if not row:
                    return None
                invite_url = row["invite_url"] if hasattr(row, "keys") else row[0]
                invite_code = row["invite_code"] if hasattr(row, "keys") else row[1]
                if invite_url:
                    return str(invite_url)
                if invite_code:
                    return f"https://discord.gg/{invite_code}"
            except Exception:
                log.debug("Promo invite DB lookup failed for %s", login_norm, exc_info=True)
            return None

        def _store_streamer_invite(
            self,
            login: str,
            *,
            guild_id: int,
            channel_id: int,
            invite_code: str,
            invite_url: str,
        ) -> None:
            login_norm = (login or "").strip().lower()
            if not login_norm:
                return
            now = datetime.now(UTC).isoformat(timespec="seconds")
            try:
                with get_conn() as conn:
                    conn.execute(
                        """
                        INSERT INTO twitch_streamer_invites (
                            streamer_login,
                            guild_id,
                            channel_id,
                            invite_code,
                            invite_url,
                            created_at,
                            last_sent_at
                        ) VALUES (?, ?, ?, ?, ?, ?, ?)
                        ON CONFLICT(streamer_login) DO UPDATE SET
                            guild_id = excluded.guild_id,
                            channel_id = excluded.channel_id,
                            invite_code = excluded.invite_code,
                            invite_url = excluded.invite_url,
                            created_at = excluded.created_at
                        """,
                        (
                            login_norm,
                            int(guild_id),
                            int(channel_id),
                            str(invite_code),
                            str(invite_url),
                            now,
                            None,
                        ),
                    )
                    conn.execute(
                        """
                        INSERT INTO discord_invite_codes (
                            guild_id,
                            invite_code,
                            created_at,
                            last_seen_at
                        ) VALUES (?, ?, ?, ?)
                        ON CONFLICT(guild_id, invite_code)
                        DO UPDATE SET last_seen_at = excluded.last_seen_at
                        """,
                        (int(guild_id), str(invite_code), now, now),
                    )
                    conn.commit()
            except Exception:
                log.debug("Could not store promo invite for %s", login_norm, exc_info=True)

        def _mark_streamer_invite_sent(self, login: str) -> None:
            login_norm = (login or "").strip().lower()
            if not login_norm:
                return
            now = datetime.now(UTC).isoformat(timespec="seconds")
            try:
                with get_conn() as conn:
                    conn.execute(
                        "UPDATE twitch_streamer_invites SET last_sent_at = ? WHERE streamer_login = ?",
                        (now, login_norm),
                    )
                    conn.commit()
            except Exception:
                log.debug(
                    "Could not update promo invite last_sent_at for %s",
                    login_norm,
                    exc_info=True,
                )

        async def _candidate_invite_channels(self) -> list:
            bot = self._discord_bot
            if not bot:
                return []

            channels = []
            seen = set()

            def _add_channel(channel) -> None:
                if not channel or not hasattr(channel, "create_invite"):
                    return
                cid = getattr(channel, "id", None)
                if cid is None or cid in seen:
                    return
                seen.add(cid)
                channels.append(channel)

            if self._discord_invite_channel_id:
                channel = bot.get_channel(self._discord_invite_channel_id)
                if channel is None and hasattr(bot, "fetch_channel"):
                    try:
                        channel = await bot.fetch_channel(self._discord_invite_channel_id)
                    except Exception:
                        channel = None
                _add_channel(channel)

            for guild in getattr(bot, "guilds", []):
                _add_channel(getattr(guild, "system_channel", None))

            for guild in getattr(bot, "guilds", []):
                for channel in getattr(guild, "text_channels", []):
                    _add_channel(channel)

            return channels

        async def _create_streamer_invite(self, login: str) -> str | None:
            bot = self._discord_bot
            if not bot:
                return None

            if hasattr(bot, "wait_until_ready"):
                try:
                    await bot.wait_until_ready()
                except Exception:
                    log.debug("Discord bot readiness check failed", exc_info=True)

            candidates = await self._candidate_invite_channels()
            if not candidates:
                return None

            for channel in candidates:
                try:
                    invite = await channel.create_invite(
                        max_uses=0,
                        max_age=0,
                        unique=True,
                        reason=f"Twitch promo invite for {login}",
                    )
                except discord.Forbidden:
                    continue
                except discord.HTTPException:
                    continue
                except Exception:
                    log.debug(
                        "Failed to create invite in channel %s for %s",
                        getattr(channel, "id", "?"),
                        login,
                        exc_info=True,
                    )
                    continue

                invite_code = str(getattr(invite, "code", "") or "").strip()
                invite_url = str(getattr(invite, "url", "") or "").strip()
                if not invite_url and invite_code:
                    invite_url = f"https://discord.gg/{invite_code}"

                guild = getattr(channel, "guild", None)
                guild_id = getattr(guild, "id", None) if guild else None
                channel_id = getattr(channel, "id", None)
                if not invite_code or not invite_url or not guild_id or not channel_id:
                    continue

                self._store_streamer_invite(
                    login,
                    guild_id=int(guild_id),
                    channel_id=int(channel_id),
                    invite_code=invite_code,
                    invite_url=invite_url,
                )
                log.info(
                    "Created promo invite for %s (guild=%s, channel=%s, code=%s)",
                    login,
                    guild_id,
                    channel_id,
                    invite_code,
                )
                return invite_url

            return None

        async def _resolve_streamer_invite(self, login: str) -> tuple[str | None, bool]:
            login_norm = (login or "").strip().lower()
            if not login_norm:
                return None, False

            cached = self._promo_invite_cache.get(login_norm)
            if cached:
                return cached, True

            invite_url = self._load_streamer_invite_from_db(login_norm)
            if invite_url:
                self._promo_invite_cache[login_norm] = invite_url
                return invite_url, True

            invite_url = await self._create_streamer_invite(login_norm)
            if invite_url:
                self._promo_invite_cache[login_norm] = invite_url
                return invite_url, True

            return None, False

        async def setup_hook(self):
            """Wird beim Starten aufgerufen, um initiales Setup zu machen."""
            # Token registrieren, damit TwitchIO ihn nutzt
            try:
                if self._token_manager:
                    access_token, bot_id = await self._token_manager.get_valid_token()
                    if access_token:
                        self._bot_token = access_token
                    # bot_id wird bereits im __init__ oder via add_token gehandelt
                    self._bot_refresh_token = (
                        self._token_manager.refresh_token or self._bot_refresh_token
                    )

                api_token = (self._bot_token or "").replace("oauth:", "").strip()
                if api_token:
                    # Wir fügen den Token hinzu. Refresh-Token ist bei TMI-Tokens meist nicht vorhanden (None).
                    # ABER: Wenn wir einen haben (aus ENV/Tresor), übergeben wir ihn, damit TwitchIO refreshen kann.
                    registered = await self._register_bot_token_with_twitchio(
                        access_token=api_token,
                        refresh_token=self._bot_refresh_token,
                    )
                    if registered:
                        log.info(
                            "Bot auth added (refresh available: %s).",
                            "yes" if self._bot_refresh_token else "no",
                        )  # nosemgrep: python.lang.security.audit.logging.logger-credential-leak.python-logger-credential-disclosure
                    else:
                        log.warning(
                            "Bot auth is present but could not be registered in TwitchIO (refresh available: %s).",
                            "yes" if self._bot_refresh_token else "no",
                        )
                    await self._persist_bot_tokens(
                        access_token=self._bot_token,
                        refresh_token=self._bot_refresh_token,
                        expires_in=None,
                        scopes=None,
                        user_id=self.bot_id,
                    )
                else:
                    log.warning("Kein gültiger TWITCH_BOT_TOKEN gefunden.")
            except Exception as e:
                if self._is_twitchio_scope_compat_error(e):
                    log.error(
                        "Bot-Token konnte wegen inkompatibler TwitchIO-Scope-Mappings nicht in TwitchIO registriert werden. "
                        "Das ist kein sicherer Hinweis auf einen ungültigen oder abgelaufenen Token. Fehler: %s",
                        e,
                    )
                else:
                    log.error(
                        "Der TWITCH_BOT_TOKEN ist ungültig oder abgelaufen. "
                        "Bitte führe den OAuth-Flow für den Bot aus (Client-ID/Secret + Redirect), "
                        "um Access- und Refresh-Token zu erhalten. Fehler: %s",
                        e,
                    )
                # Wir machen weiter, damit der Bot zumindest "ready" wird und andere Cogs nicht blockiert

            # Initial channels werden NICHT hier gejoined –
            # setup_hook() läuft vor event_ready(), die WS-Session von TwitchIO
            # ist noch nicht aufgebaut. Joins hier führen zu
            # "invalid transport and auth combination" (400).
            # Defer nach event_ready().

        async def event_ready(self):
            """Wird aufgerufen, wenn der Bot verbunden ist."""
            name = self.user.name if self.user else "Unknown"
            log.info("Twitch Chat Bot ready | Logged in as: %s", name)
            # Debug: Registrierte Commands loggen
            cmds = ", ".join(sorted(self.commands.keys()))
            log.info("Registered Chat Commands: %s", cmds)

            # Initial channels erst hier joinen – WS-Session ist jetzt bereit
            if self._skip_initial_join_once:
                self._skip_initial_join_once = False
                self._log_chat_runtime_snapshot(
                    flow_id=self._next_chat_observability_flow_id(prefix="ready"),
                    phase="event_ready_skip_initial_join",
                    reason="managed_transport_restart",
                    level=logging.INFO,
                )
                log.info("Skipping initial channel join after managed transport restart.")
            elif self._initial_channels:
                log.info("Joining %d initial channels...", len(self._initial_channels))
                for channel in self._initial_channels:
                    try:
                        success = await self.join(channel)
                        if success:
                            await asyncio.sleep(0.2)  # Rate limiting
                    except Exception as e:
                        log.debug(
                            "Konnte initialem Channel %s nicht beitreten: %s",
                            channel,
                            e,
                        )

            if PROMO_MESSAGES and (_PROMO_ACTIVITY_ENABLED or PROMO_VIEWER_SPIKE_ENABLED):
                if not self._promo_task or self._promo_task.done():
                    self._promo_task = asyncio.create_task(
                        self._periodic_promo_loop(),
                        name="twitch.chat_bot.promos",
                    )
                    log.debug(
                        "Chat-Promo-Loop gestartet (Check alle %ss)",
                        max(15, int(PROMO_LOOP_INTERVAL_SEC)),
                    )

        async def close(self):
            promo_task = self._promo_task
            self._promo_task = None
            if promo_task and not promo_task.done():
                promo_task.cancel()
                try:
                    await promo_task
                except asyncio.CancelledError:
                    log.debug("Promo-Task wurde beim Shutdown abgebrochen")
                except Exception:
                    log.debug("Promo-Task konnte nicht sauber beendet werden", exc_info=True)
            await super().close()

        async def event_command_error(self, payload):
            """Fehlerbehandlung für Commands."""
            ctx = payload.context
            error = payload.exception

            if isinstance(error, twitchio_commands.CommandNotFound):
                # Kein Traceback für unbekannte Commands, nur Debug
                log.debug("Command not found: %s", ctx.message.content)
                return

            # Andere Fehler loggen
            log.exception(
                "Error invoking command %s: %s",
                ctx.command.name if ctx.command else "Unknown",
                error,
            )

        async def event_token_refreshed(self, payload):
            """Persistiert erneuerte Bot-Tokens, sobald TwitchIO sie refreshed."""
            try:
                # Wir speichern die ID intern, falls wir sie brauchen,
                # aber vermeiden das Setzen der read-only Property bot_id
                if payload.user_id:
                    pass
                if self.bot_id and str(payload.user_id) != str(self.bot_id):
                    return  # Nur den Bot-Token persistieren, nicht Streamer-Tokens
                self._bot_token = (
                    f"oauth:{payload.token}"
                    if not payload.token.startswith("oauth:")
                    else payload.token
                )
                self._bot_refresh_token = payload.refresh_token
            except Exception:
                return
            try:
                await self._persist_bot_tokens(
                    access_token=self._bot_token or payload.token,
                    refresh_token=self._bot_refresh_token or payload.refresh_token,
                    expires_in=payload.expires_in,
                    scopes=list(payload.scopes.selected),
                    user_id=payload.user_id,
                )
            except Exception:
                log.debug("Konnte refreshed Bot-Token nicht persistieren", exc_info=True)

        async def _on_token_manager_refresh(
            self,
            access_token: str,
            refresh_token: str | None,
            _expires_at: datetime | None,
        ) -> None:
            """Registriert neue Tokens aus dem Token Manager und updated TwitchIO."""
            self._bot_token = access_token
            self._bot_refresh_token = refresh_token
            api_token = (access_token or "").replace("oauth:", "").strip()
            if not api_token:
                return
            try:
                registered = await self._register_bot_token_with_twitchio(
                    access_token=api_token,
                    refresh_token=refresh_token,
                )
                if not registered:
                    log.warning(
                        "Refreshed Bot-Token ist vorhanden, konnte aber nicht in TwitchIO registriert werden."
                    )
            except Exception as exc:
                if self._is_twitchio_scope_compat_error(exc):
                    log.warning(
                        "Refreshed Bot-Token konnte wegen inkompatibler TwitchIO-Scope-Mappings nicht in TwitchIO registriert werden: %s",
                        exc,
                    )
                else:
                    log.debug(
                        "Konnte refreshed Bot-Token nicht in TwitchIO registrieren",
                        exc_info=True,
                    )

        async def event_message(self, message):
            """Wird bei jeder Chat-Nachricht aufgerufen."""
            # Compatibility layer for TwitchIO 3.x EventSub
            # In 3.x, message is a ChatMessage with text/chatter/broadcaster
            # and optional source_broadcaster (shared chat).
            # In 2.x, message is a Message with content/author/channel

            # Detect 3.x by presence of 'text' or 'chatter' (and absence of 'content')
            is_3x = hasattr(message, "chatter") and not hasattr(message, "content")

            if is_3x:
                # Aliases for 2.x compatibility
                if not hasattr(message, "content"):
                    message.content = getattr(message, "text", "")
                if not hasattr(message, "author"):
                    message.author = message.chatter

                # Ensure author has 2.x style flags if missing
                author = message.author
                if not hasattr(author, "moderator") and hasattr(author, "is_moderator"):
                    author.moderator = author.is_moderator
                if not hasattr(author, "broadcaster") and hasattr(author, "is_broadcaster"):
                    author.broadcaster = author.is_broadcaster

                # In 3.x EventSub payloads there is no message.channel by default.
                # Normalize to a 2.x-like shape expected by downstream code.
                channel = getattr(message, "channel", None)
                if channel is None:
                    channel = getattr(message, "source_broadcaster", None) or getattr(
                        message, "broadcaster", None
                    )
                    if channel is not None:
                        try:
                            message.channel = channel
                        except (AttributeError, TypeError):
                            log.debug(
                                "Could not assign normalized channel on EventSub message",
                                exc_info=True,
                            )

                if (
                    channel is not None
                    and not hasattr(channel, "name")
                    and hasattr(channel, "login")
                ):
                    try:
                        channel.name = channel.login
                    except (AttributeError, TypeError):
                        log.debug(
                            "Could not normalize channel.name from channel.login",
                            exc_info=True,
                        )

            # Fallback for echo if still missing (unlikely in 3.x)
            if not hasattr(message, "echo"):
                safe_bot_id = self.bot_id_safe or self.bot_id or ""
                message.echo = str(getattr(message, "chatter", message).id) == str(safe_bot_id)

            # Ignoriere Bot-Nachrichten
            if message.echo:
                return

            # Whitelist-Check: Bekannte Bot-Accounts überspringen Spam-Prüfung
            author_name = getattr(message.author, "name", "").lower()
            if author_name in WHITELISTED_BOTS:
                # Bot ist whitelisted - überspringe Spam-Detection komplett
                try:
                    await self._track_chat_health(message)
                except Exception:
                    log.exception("Chat-Health-Tracking fuer Whitelist-Bot fehlgeschlagen")
                await self.process_commands(message)
                return

            channel_login = self._normalize_channel_login_safe(getattr(message, "channel", None))
            is_deadlock_live = bool(channel_login and self._is_deadlock_live(channel_login))

            # --- DATENSAMMLUNG FÜR ALLE, BOT-FUNKTIONEN NUR FÜR ECHTE PARTNER ---
            # WICHTIG: Monitored-Only Channels sind KEINE Partner!
            # _is_partner_channel_for_chat_tracking() ist in dieser Klasse überschrieben
            # und gibt True für monitored-only zurück (für Datensammlung), daher explizit
            # ausschließen, damit monitored-only Channels KEINE Bot-Funktionen bekommen.
            is_monitored_only_ch = bool(channel_login and self._is_monitored_only(channel_login))
            is_partner = (
                bool(channel_login)
                and not is_monitored_only_ch
                and self._is_partner_channel_for_chat_tracking(channel_login)
            )

            if not is_partner:
                # NON-PARTNER: Nur passive Datensammlung, KEINE Bot-Funktionen!
                # - Chat-Messages werden geloggt (für Analyse)
                # - KEINE Auto-Moderation
                # - KEINE Commands
                # - KEINE Promo-Messages
                # - KEINE Discord-Invites
                try:
                    await self._track_chat_health(message)
                except Exception:
                    log.exception("Chat-Health-Tracking fuer Non-Partner fehlgeschlagen")
                return
            # ---------------------------------------------------------------

            # AB HIER: Nur noch Partner! (Volle Bot-Funktionen)
            if is_partner:
                try:
                    await self._maybe_warn_service_pitch(message, channel_login=channel_login)
                except Exception:
                    log.debug("Service-Pitch-Warnung fehlgeschlagen", exc_info=True)

                try:
                    spam_score, spam_reasons = self._calculate_spam_score(message.content or "")
                    has_phrase_or_fragment_signal = any(
                        reason.startswith("Phrase(") or reason.startswith("Fragment(")
                        for reason in spam_reasons
                    )
                    mention_score, mention_reasons = await self._score_mention_patterns(
                        message.content or "",
                        host_login=channel_login,
                        allow_host_bonus=has_phrase_or_fragment_signal,
                    )
                    if mention_score:
                        spam_score += mention_score
                        spam_reasons.extend(mention_reasons)

                    # 2. Faktor: Account-Alter prüft nur den letzten fehlenden Punkt zum Ban.
                    # Ein junges Konto soll nur dann eskalieren, wenn bereits zwei Signale vorliegen.
                    if spam_score == (SPAM_MIN_MATCHES - 1):
                        try:
                            author_id = getattr(message.author, "id", None)
                            if author_id:
                                # fetch_users benötigt IDs. Twitch IDs sind numerisch.
                                users = await self.fetch_users(ids=[int(author_id)])
                                if users and users[0].created_at:
                                    created_at = users[0].created_at
                                    if created_at.tzinfo is None:
                                        created_at = created_at.replace(tzinfo=UTC)

                                    age = datetime.now(UTC) - created_at
                                    if age.days < 90:  # Jünger als 3 Monate
                                        spam_score += 1
                                        spam_reasons.append(f"Account-Alter: {age.days} Tage")
                        except Exception:
                            log.debug(
                                "Konnte User-Alter für Spam-Check nicht laden",
                                exc_info=True,
                            )

                    if spam_score >= SPAM_MIN_MATCHES:
                        enforced = await self._auto_ban_and_cleanup(message)
                        if not enforced:
                            channel_obj = getattr(message, "channel", None)
                            channel_name = (
                                getattr(channel_obj, "name", "")
                                or getattr(channel_obj, "login", "")
                                or "unknown"
                            )
                            log.warning(
                                "Spam erkannt in %s (Score: %d, Treffer: %s), aber Auto-Ban konnte nicht durchgesetzt werden.",
                                channel_name,
                                spam_score,
                                ", ".join(spam_reasons),
                            )
                        return
                    elif spam_score > 0:
                        channel_obj = getattr(message, "channel", None)
                        channel_name = (
                            getattr(channel_obj, "name", "")
                            or getattr(channel_obj, "login", "")
                            or "unknown"
                        )
                        author_name = getattr(message.author, "name", "unknown")
                        author_id = str(getattr(message.author, "id", ""))

                        # Logge Verdacht in Datei für Feinabstimmung
                        reasons_str = ", ".join(spam_reasons)
                        self._record_autoban(
                            channel_name=channel_name,
                            chatter_login=author_name,
                            chatter_id=author_id,
                            content=message.content or "",
                            status=f"SUSPICIOUS({spam_score})",
                            reason=reasons_str,
                        )

                        log.info(
                            "Verdächtige Nachricht (Score %d, Treffer: %s) in %s von %s: %s",
                            spam_score,
                            reasons_str,
                            channel_name,
                            author_name,
                            message.content,
                        )
                except Exception:
                    log.debug("Auto-Ban Prüfung fehlgeschlagen", exc_info=True)

            # Freche Auto-Replies nur, wenn Deadlock läuft
            if is_deadlock_live:
                try:
                    await self._maybe_fun_responses(message, channel_login)
                except Exception:
                    log.debug("Fun-Response fehlgeschlagen", exc_info=True)

            try:
                await self._track_chat_health(message)
            except Exception:
                log.exception("Chat-Health-Tracking fehlgeschlagen")

            sent_invite = False
            if is_deadlock_live:
                try:
                    sent_invite = await self._maybe_send_deadlock_access_hint(message)
                except Exception:
                    log.debug("Deadlock-Invite-Check fehlgeschlagen", exc_info=True)

                if _PROMO_ACTIVITY_ENABLED and not sent_invite:
                    try:
                        await self._maybe_send_activity_promo(message)
                    except Exception:
                        log.debug("Promo-Activity-Check fehlgeschlagen", exc_info=True)

            # Verarbeite Commands
            await self.process_commands(message)

        async def event_chat_notification(self, payload) -> None:
            """Raid- und Subscription-relevante channel.chat.notification Events verarbeiten."""
            raid_bot = getattr(self, "_raid_bot", None)
            if raid_bot is None:
                return

            try:
                broadcaster = getattr(payload, "broadcaster", None)
                broadcaster_login = (
                    str(
                        getattr(broadcaster, "name", None)
                        or getattr(broadcaster, "login", None)
                        or ""
                    )
                    .strip()
                    .lower()
                )
                broadcaster_id = str(getattr(broadcaster, "id", None) or "").strip()
                if not broadcaster_id or not broadcaster_login:
                    return

                notice_type = str(getattr(payload, "notice_type", "") or "").strip().lower()
                timestamp_value = getattr(payload, "timestamp", None)
                event_timestamp = str(timestamp_value).strip() if timestamp_value else None

                if notice_type == "raid":
                    raid_payload = getattr(payload, "raid", None)
                    if raid_payload is None:
                        return
                    raid_user = getattr(raid_payload, "user", None)
                    from_broadcaster_login = (
                        str(
                            getattr(raid_user, "name", None)
                            or getattr(raid_user, "login", None)
                            or ""
                        )
                        .strip()
                        .lower()
                    )
                    if not from_broadcaster_login:
                        return
                    from_broadcaster_id = str(getattr(raid_user, "id", None) or "").strip() or None
                    viewer_count = int(getattr(raid_payload, "viewer_count", 0) or 0)
                    await raid_bot.on_chat_raid_notification(
                        to_broadcaster_id=broadcaster_id,
                        to_broadcaster_login=broadcaster_login,
                        from_broadcaster_login=from_broadcaster_login,
                        from_broadcaster_id=from_broadcaster_id,
                        viewer_count=viewer_count,
                        message_id=str(getattr(payload, "id", None) or "").strip() or None,
                        event_timestamp=event_timestamp,
                    )
                    return

                if notice_type == "unraid":
                    chatter = getattr(payload, "chatter", None)
                    from_broadcaster_login = (
                        str(
                            getattr(chatter, "name", None)
                            or getattr(chatter, "login", None)
                            or ""
                        )
                        .strip()
                        .lower()
                    )
                    from_broadcaster_id = str(getattr(chatter, "id", None) or "").strip() or None
                    # Source-side self-unraid notices do not identify a target raid arrival.
                    # Treat them as source-side cancellation diagnostics instead of
                    # target-arrival correlation to avoid misleading "source -> source" logs.
                    if from_broadcaster_login and from_broadcaster_login == broadcaster_login:
                        if from_broadcaster_id is None or from_broadcaster_id == broadcaster_id:
                            handle_source_unraid = getattr(
                                raid_bot,
                                "on_source_self_unraid_notification",
                                None,
                            )
                            if callable(handle_source_unraid):
                                await handle_source_unraid(
                                    broadcaster_id=broadcaster_id,
                                    broadcaster_login=broadcaster_login,
                                    message_id=str(getattr(payload, "id", None) or "").strip() or None,
                                    event_timestamp=event_timestamp,
                                )
                            else:
                                log.debug(
                                    "Ignoring self unraid chat notification in source channel %s (message_id=%s)",
                                    broadcaster_login,
                                    str(getattr(payload, "id", None) or "").strip() or "n/a",
                                )
                            return
                    await raid_bot.on_chat_unraid_notification(
                        to_broadcaster_id=broadcaster_id,
                        to_broadcaster_login=broadcaster_login,
                        from_broadcaster_login=from_broadcaster_login,
                        from_broadcaster_id=from_broadcaster_id,
                        event_timestamp=event_timestamp,
                    )
                    return

                subscription_notice = self._build_subscription_event_from_chat_notification(
                    payload,
                    notice_type=notice_type,
                )
                if subscription_notice is not None:
                    event_type, event = subscription_notice
                    handle_subscription = getattr(
                        raid_bot,
                        "on_chat_subscription_notification",
                        None,
                    )
                    if callable(handle_subscription):
                        await handle_subscription(
                            broadcaster_id=broadcaster_id,
                            broadcaster_login=broadcaster_login,
                            notice_type=notice_type,
                            event_type=event_type,
                            event=event,
                        )
            except Exception:
                log.exception("event_chat_notification failed")

        @staticmethod
        def _chat_notification_extract_user(user_obj) -> tuple[str | None, str | None]:
            if user_obj is None:
                return None, None

            candidates = [user_obj]
            for attr_name in ("recipient", "user", "gifter", "chatter"):
                nested = getattr(user_obj, attr_name, None)
                if nested is not None and nested is not user_obj:
                    candidates.append(nested)

            for candidate in candidates:
                login = (
                    str(
                        getattr(candidate, "login", None)
                        or getattr(candidate, "user_login", None)
                        or getattr(candidate, "name", None)
                        or getattr(candidate, "user_name", None)
                        or getattr(candidate, "display_name", None)
                        or ""
                    )
                    .strip()
                    .lower()
                ) or None
                user_id = str(getattr(candidate, "id", None) or "").strip() or None
                if login or user_id:
                    return login, user_id

            return None, None

        @staticmethod
        def _chat_notification_message_text(payload) -> str | None:
            for candidate in (getattr(payload, "message", None), payload):
                if candidate is None:
                    continue

                text = str(getattr(candidate, "text", None) or "").strip()
                if text:
                    return text

                fragments = getattr(candidate, "fragments", None)
                if isinstance(fragments, (list, tuple)):
                    joined = "".join(
                        str(getattr(fragment, "text", None) or "")
                        for fragment in fragments
                    ).strip()
                    if joined:
                        return joined
            return None

        def _build_subscription_event_from_chat_notification(
            self,
            payload,
            *,
            notice_type: str,
        ) -> tuple[str, dict[str, object]] | None:
            normalized_notice = str(notice_type or "").strip().lower()
            if normalized_notice.startswith("shared_chat_"):
                normalized_notice = normalized_notice.removeprefix("shared_chat_")
            chatter_login, chatter_id = self._chat_notification_extract_user(
                getattr(payload, "chatter", None)
            )
            message_text = self._chat_notification_message_text(payload)

            if normalized_notice == "sub":
                sub_payload = getattr(payload, "sub", None)
                if sub_payload is None:
                    return None
                return (
                    "subscribe",
                    {
                        "user_login": chatter_login,
                        "user_id": chatter_id,
                        "tier": str(
                            getattr(sub_payload, "sub_tier", None)
                            or getattr(sub_payload, "tier", None)
                            or "1000"
                        ).strip(),
                        "is_gift": False,
                    },
                )

            if normalized_notice == "resub":
                resub_payload = getattr(payload, "resub", None)
                if resub_payload is None:
                    return None
                gifter_login, gifter_id = self._chat_notification_extract_user(
                    getattr(resub_payload, "gifter", None)
                )
                is_gift = bool(
                    getattr(resub_payload, "gift", None)
                    if getattr(resub_payload, "gift", None) is not None
                    else getattr(resub_payload, "is_gift", False)
                )
                event = {
                    "user_login": chatter_login,
                    "user_id": chatter_id,
                    "tier": str(
                        getattr(resub_payload, "sub_tier", None)
                        or getattr(resub_payload, "tier", None)
                        or "1000"
                    ).strip(),
                    "is_gift": is_gift,
                    "gifter_login": gifter_login,
                    "gifter_user_id": gifter_id,
                    "cumulative_months": int(
                        getattr(resub_payload, "cumulative_months", 0) or 0
                    )
                    or None,
                    "streak_months": int(getattr(resub_payload, "streak_months", 0) or 0)
                    or None,
                }
                if message_text:
                    event["message"] = {"text": message_text}
                return "resub", event

            if normalized_notice == "sub_gift":
                gift_payload = getattr(payload, "sub_gift", None)
                if gift_payload is None:
                    return None
                recipient_source = getattr(gift_payload, "recipient", None) or gift_payload
                recipient_login, recipient_id = self._chat_notification_extract_user(
                    recipient_source
                )
                gift_total = int(
                    getattr(gift_payload, "cumulative_total", None)
                    or getattr(gift_payload, "total", 0)
                    or 0
                ) or None
                recipient_login = (
                    recipient_login
                    or (
                        str(
                            getattr(gift_payload, "recipient_user_login", None)
                            or getattr(gift_payload, "recipient_user_name", None)
                            or ""
                        )
                        .strip()
                        .lower()
                    )
                    or None
                )
                recipient_id = recipient_id or (
                    str(getattr(gift_payload, "recipient_user_id", None) or "").strip() or None
                )
                return (
                    "gift",
                    {
                        "user_login": recipient_login,
                        "user_id": recipient_id,
                        "recipient_login": recipient_login,
                        "recipient_user_id": recipient_id,
                        "tier": str(
                            getattr(gift_payload, "sub_tier", None)
                            or getattr(gift_payload, "tier", None)
                            or "1000"
                        ).strip(),
                        "is_gift": True,
                        "gifter_login": chatter_login,
                        "gifter_user_id": chatter_id,
                        "total": 1,
                        "gift_total": gift_total,
                        "gift_total_kind": "cumulative_total",
                    },
                )

            if normalized_notice == "community_sub_gift":
                community_gift_payload = getattr(payload, "community_sub_gift", None)
                if community_gift_payload is None:
                    return None
                gift_total = int(
                    getattr(community_gift_payload, "total", None)
                    or getattr(community_gift_payload, "gift_total", None)
                    or 0
                ) or None
                return (
                    "gift",
                    {
                        "tier": str(
                            getattr(community_gift_payload, "sub_tier", None)
                            or getattr(community_gift_payload, "tier", None)
                            or "1000"
                        ).strip(),
                        "is_gift": True,
                        "gifter_login": chatter_login,
                        "gifter_user_id": chatter_id,
                        "total": gift_total,
                        "gift_total": gift_total,
                        "gift_total_kind": "batch_total",
                    },
                )

            return None

        def _get_streamer_by_channel(self, channel_name: str) -> tuple | None:
            """
            Findet Streamer-Daten anhand des Channel-Namens.

            WICHTIG: Gibt nur PARTNER zurück (nicht Monitored-Only)!
            Bot-Funktionen (Commands, Raids, etc.) nur für Partner.
            """
            normalized = self._normalize_channel_login(channel_name)
            with get_conn() as conn:
                row = conn.execute(
                    """
                    SELECT twitch_login, twitch_user_id, raid_bot_enabled
                    FROM twitch_streamers_partner_state
                    WHERE LOWER(twitch_login) = ?
                      AND is_partner_active = 1
                    """,
                    (normalized,),
                ).fetchone()
            return row

        @staticmethod
        def _normalize_channel_login(channel_name: str) -> str:
            return (channel_name or "").lower().lstrip("#")

        def _resolve_session_id(self, login: str) -> int | None:
            """Best-effort Mapping von Channel zu offener Twitch-Session."""
            cache_key = login.lower()
            cached = self._session_cache.get(cache_key)
            now_ts = datetime.now(UTC)
            if cached:
                cached_id, cached_at = cached
                if (now_ts - cached_at).total_seconds() < 60:
                    return cached_id

            with get_conn() as conn:
                row = conn.execute(
                    """
                    SELECT id FROM twitch_stream_sessions
                     WHERE streamer_login = ? AND ended_at IS NULL
                     ORDER BY started_at DESC
                     LIMIT 1
                    """,
                    (cache_key,),
                ).fetchone()
            if not row:
                return None

            session_id = int(row["id"] if hasattr(row, "keys") else row[0])
            self._session_cache[cache_key] = (session_id, now_ts)
            return session_id


if not TWITCHIO_AVAILABLE:

    class RaidChatBot:  # type: ignore[redefined-outer-name]
        """Stub, damit Import-Caller nicht crashen, wenn twitchio fehlt."""

        pass


async def create_twitch_chat_bot(
    client_id: str,
    client_secret: str,
    redirect_uri: str,
    raid_bot=None,
    bot_token: str | None = None,
    bot_refresh_token: str | None = None,
    log_missing: bool = True,
    token_manager: TwitchBotTokenManager | None = None,
) -> RaidChatBot | None:
    """
    Erstellt einen Twitch Chat Bot mit Bot-Account-Token.

    Env-Variablen:
    - TWITCH_BOT_TOKEN: OAuth-Token für den Bot-Account
    """
    if not TWITCHIO_AVAILABLE:
        log.warning(
            "TwitchIO nicht installiert – Twitch Chat Bot wird übersprungen. "
            "Installation optional: pip install twitchio"
        )
        return None

    """
    Env-Variablen:
    - TWITCH_BOT_TOKEN: OAuth-Token für den Bot-Account
    - TWITCH_BOT_TOKEN_FILE: Optionaler Dateipfad, der das OAuth-Token enthaelt
    - TWITCH_BOT_NAME: Name des Bot-Accounts (optional)
    """
    if not TWITCHIO_AVAILABLE:
        log.warning(
            "TwitchIO nicht installiert – Twitch Chat Bot wird übersprungen. "
            "Installation optional: pip install twitchio"
        )
        return None

    token = bot_token
    refresh_token = bot_refresh_token

    if not token:
        token, refresh_from_store, _ = load_bot_tokens(log_missing=log_missing)
        refresh_token = refresh_token or refresh_from_store
    else:
        _, refresh_from_store, _ = load_bot_tokens(log_missing=False)
        refresh_token = refresh_token or refresh_from_store

    if not token:
        return None

    token_mgr = token_manager
    token_mgr_created = False
    if token_mgr is None and client_id:
        token_mgr = TwitchBotTokenManager(
            client_id, client_secret or "", keyring_service=_KEYRING_SERVICE
        )
        token_mgr_created = True

    bot_id = None
    if token_mgr:
        initialised = await token_mgr.initialize(access_token=token, refresh_token=refresh_token)
        if not initialised:
            log.error(
                "Twitch Bot Token Manager konnte nicht initialisiert werden (kein Refresh-Token?)."
            )
            if token_mgr_created:
                await token_mgr.cleanup()
            return None
        token = token_mgr.access_token or token
        refresh_token = token_mgr.refresh_token or refresh_token
        bot_id = token_mgr.bot_id

    # Partner-Channels abrufen (nur wenn Raid-Auth + Chat-Scopes + aktuell live)
    with get_conn() as conn:
        partners = conn.execute(
            """
            SELECT DISTINCT s.twitch_login,
                            s.twitch_user_id,
                            a.scopes,
                            l.is_live,
                            COALESCE(l.last_game, '')
              FROM twitch_streamers_partner_state s
              JOIN twitch_raid_auth a ON s.twitch_user_id = a.twitch_user_id
              LEFT JOIN twitch_live_state l ON s.twitch_user_id = l.twitch_user_id
             WHERE s.is_partner_active = 1
            """
        ).fetchall()

        # Monitored-Only Channels abrufen (kein Auth nötig).
        # Ohne offene Session würde der Chat-Bot zwar joinen, könnte eingehende
        # Nachrichten aber nicht einer Session zuordnen und nur skip_missing_session
        # loggen. Deshalb hier nur Channels laden, für die Monitoring bereits eine
        # offene Session angelegt hat.
        monitored_rows = conn.execute(
            """
            SELECT s.twitch_login
              FROM twitch_streamers s
             WHERE s.is_monitored_only = 1
               AND EXISTS (
                    SELECT 1
                      FROM twitch_stream_sessions sess
                     WHERE LOWER(sess.streamer_login) = LOWER(s.twitch_login)
                       AND sess.ended_at IS NULL
               )
            """
        ).fetchall()

    initial_channels = []
    monitored_channels = []

    # 1. Partner adden
    for login, user_id, scopes_raw, is_live, last_game in partners:
        login_norm = (login or "").strip()
        if not login_norm:
            continue
        scopes = [s.strip().lower() for s in (scopes_raw or "").split() if s.strip()]
        has_channel_bot_grant = "channel:bot" in scopes
        if not has_channel_bot_grant:
            continue
        # Optional: auch Offline-Partner direkt joinen, damit Commands wie !ping funktionieren
        if (is_live is None or not bool(is_live)) and not CHAT_JOIN_OFFLINE:
            continue
        initial_channels.append(login_norm)

    # 2. Monitored-Only adden, aber nur wenn Monitoring bereits eine offene
    # Session kennt. Neue Channels werden vom Scout zuerst in die DB geschrieben,
    # dann wird dort sofort eine Session vor dem Chat-Join erzeugt.
    for row in monitored_rows:
        login = str(row[0]).strip().lower()
        if login and login not in initial_channels:
            initial_channels.append(login)
            monitored_channels.append(login)

    log.info(
        "Creating Twitch Chat Bot for %d channels (%d monitored only)",
        len(initial_channels),
        len(monitored_channels),
    )

    # Bot-ID via API abrufen (TwitchIO braucht diese zwingend bei user:bot Scope)
    if bot_id is None:
        try:
            import aiohttp

            async with aiohttp.ClientSession() as session:
                api_token = token.replace("oauth:", "")

                # 1. Versuch: id.twitch.tv/oauth2/validate (oft am tolerantesten für User-IDs)
                # Wir probieren beide Header-Varianten
                for auth_header in [f"OAuth {api_token}", f"Bearer {api_token}"]:
                    async with session.get(
                        "https://id.twitch.tv/oauth2/validate",
                        headers={"Authorization": auth_header},
                    ) as r:
                        if r.status == 200:
                            val_data = await r.json()
                            bot_id = val_data.get("user_id")
                            if bot_id:
                                log.info("Validated Bot ID: %s", bot_id)
                                break

                # 2. Versuch: Helix users (falls validate fehlschlug)
                if not bot_id:
                    headers = {
                        "Client-ID": client_id,
                        "Authorization": f"Bearer {api_token}",
                    }
                    async with session.get(
                        "https://api.twitch.tv/helix/users", headers=headers
                    ) as r:
                        if r.status == 200:
                            data = await r.json()
                            if data.get("data"):
                                bot_id = data["data"][0]["id"]
                                log.info("Fetched Bot ID via Helix: %s", bot_id)
                        elif r.status == 401:
                            log.warning(
                                "Twitch API 401 Unauthorized: Der TWITCH_BOT_TOKEN scheint ungültig zu sein."
                            )
                        else:
                            log.warning("Could not fetch Bot ID: HTTP %s", r.status)
        except Exception as e:
            log.warning("Failed to fetch Bot ID: %s", e)

    # Fallback: Wenn Fetch fehlschlägt, aber Token existiert, versuchen wir es ohne ID (könnte failen)
    # oder übergeben einen Dummy, falls TwitchIO das schluckt.
    # Besser: Wir übergeben was wir haben.

    adapter_host = (os.getenv("TWITCH_CHAT_ADAPTER_HOST") or "").strip() or "127.0.0.1"
    adapter_port_raw = (os.getenv("TWITCH_CHAT_ADAPTER_PORT") or "").strip()
    adapter_port = 4343
    if adapter_port_raw:
        try:
            adapter_port = int(adapter_port_raw)
        except ValueError:
            log.warning(
                "TWITCH_CHAT_ADAPTER_PORT '%s' ist ungueltig - es wird der Standardport 4343 genutzt",
                adapter_port_raw,
            )
            adapter_port = 4343

    # Adapter nur starten wenn TWITCH_CHAT_ADAPTER nicht explizit deaktiviert ist
    # UND der Port frei ist. TwitchIO 3.x erstellt intern einen Default-Adapter wenn
    # keiner übergeben wird – wir kontrollieren das hier explizit, um Port-Konflikte
    # bei Cog-Reloads zu vermeiden.
    adapter_disabled = (os.getenv("TWITCH_CHAT_ADAPTER") or "").strip().lower() in {
        "0",
        "false",
        "off",
        "no",
    }
    web_adapter = None
    if not adapter_disabled:
        import socket as _socket

        try:
            # Versuch einer Verbindung, um zu prüfen, ob der Port belegt ist
            # Connect-Check ist passiv und verursacht kein TIME_WAIT wie Bind-Check
            with _socket.create_connection((adapter_host, adapter_port), timeout=0.2):
                # Verbindung erfolgreich -> Port ist belegt
                log.debug(
                    "TwitchIO Web Adapter Port %s auf %s ist belegt (Verbindung erfolgreich) – starte ohne Adapter "
                    "(Webhooks/OAuth für Chat-Bot ausgeschaltet).",
                    adapter_port,
                    adapter_host,
                )
                web_adapter = None
        except OSError:
            # Verbindung fehlgeschlagen -> Port ist wahrscheinlich frei
            try:
                web_adapter = twitchio_web.AiohttpAdapter(
                    host=adapter_host,
                    port=adapter_port,
                )
                log.info(
                    "TwitchIO Web Adapter wird auf %s:%s gestartet",
                    adapter_host,
                    adapter_port,
                )
            except Exception as e:
                log.error("Fehler beim Erstellen des Adapters trotz freiem Port: %s", e)
                web_adapter = None
    else:
        log.info("TwitchIO Web Adapter deaktiviert per TWITCH_CHAT_ADAPTER.")

    bot = RaidChatBot(
        token=token,
        client_id=client_id,
        client_secret=client_secret,
        bot_id=bot_id,
        prefix="!",
        initial_channels=initial_channels,
        refresh_token=refresh_token,
        web_adapter=web_adapter,
        token_manager=token_mgr,
    )
    bot.set_raid_bot(raid_bot)
    bot.set_monitored_channels(monitored_channels)

    return bot
