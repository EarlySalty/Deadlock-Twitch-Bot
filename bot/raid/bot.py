# cogs/twitch/raid_manager.py
"""
Raid Bot Manager - RaidBot

Verwaltet:
- Automatische Raids beim Offline-Gehen
- Partner-Auswahl (niedrigste Viewer, optional niedrigste Follower)
- Raid-Metadaten und History
"""

import asyncio
import json
import logging
import os
import secrets
import time
from datetime import UTC, datetime
from typing import Any
from urllib.parse import urlencode

import aiohttp
import discord

from ..core.constants import TWITCH_TARGET_GAME_NAME
from ..discord_role_sync import normalize_discord_user_id, sync_streamer_role
from ..storage import (
    backfill_tracked_stats_from_category,
    get_conn,
    insert_observability_event,
    load_active_partner,
    load_streamer_identity,
    promote_streamer_to_partner,
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
RAID_SCOPES = [
    "channel:manage:raids",
    "channel:read:subscriptions",
    "channel:manage:moderators",
    "channel:bot",
    "clips:edit",
    "channel:read:ads",
    "bits:read",
    "channel:read:hype_train",
    "channel:read:redemptions",
]

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
        self._pending_raids: dict[str, dict[str, Any] | tuple[Any, ...]] = {}
        self._recent_raid_arrivals: dict[tuple[str, str], dict[str, Any]] = {}
        self._orphan_chat_raid_notifications: dict[tuple[str, str], dict[str, Any]] = {}
        # Unterdrückt den nächsten Offline-Auto-Raid, wenn kurz zuvor ein manueller/externer Raid erkannt wurde.
        self._manual_raid_suppression: dict[str, float] = {}
        self._user_scope_fallback_warned: set[tuple[str, str]] = set()

        # Cleanup-Task starten
        self._cleanup_task = asyncio.create_task(self._periodic_cleanup())

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
        if self._cleanup_task and not self._cleanup_task.done():
            self._cleanup_task.cancel()
            try:
                await self._cleanup_task
            except asyncio.CancelledError:
                log.debug("Cleanup task cancelled")

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

        last_state_cleanup = 0.0
        # Startup-Delay: erster Token-Refresh erst nach 5 Minuten (nicht sofort nach 60s).
        # Verhindert Race-Condition wenn ein alter Prozess noch kurz weiterläuft,
        # bevor der PID-Lock greift.
        last_token_refresh = time.time() - token_refresh_interval + 300.0
        last_blacklist_cleanup = 0.0
        last_raid_cleanup = 0.0
        last_grace_period_check = 0.0
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

                # 3. Pending Raids Cleanup (alle 2min)
                if now - last_raid_cleanup >= pending_raid_cleanup_interval:
                    self._cleanup_stale_pending_raids()
                    self._cleanup_recent_raid_arrivals()
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
        sequence = int(getattr(self, "_raid_observability_sequence", 0) or 0) + 1
        self._raid_observability_sequence = sequence
        return f"{str(prefix or 'raid').strip().lower()}-{int(time.time() * 1000)}-{sequence}"

    def _raid_observability_counters(self) -> dict[str, int]:
        counters = getattr(self, "_raid_observability_counter_store", None)
        if not isinstance(counters, dict):
            counters = {}
            self._raid_observability_counter_store = counters
        return counters

    def _increment_raid_observability_counter(self, name: str, amount: int = 1) -> int:
        counter_name = str(name or "").strip()
        if not counter_name:
            return 0
        counters = self._raid_observability_counters()
        counters[counter_name] = int(counters.get(counter_name, 0) or 0) + int(amount)
        return counters[counter_name]

    @staticmethod
    def _raid_observability_value(value: object, *, limit: int = 240) -> str:
        def _convert(obj: object) -> object:
            if obj is None:
                return None
            if isinstance(obj, datetime):
                return obj.isoformat()
            if isinstance(obj, set):
                return sorted(str(item) for item in obj)
            if isinstance(obj, (list, tuple)):
                return [_convert(item) for item in obj]
            if isinstance(obj, dict):
                return {str(key): _convert(val) for key, val in obj.items()}
            if isinstance(obj, (str, int, float, bool)):
                return obj
            return str(obj)

        normalized = _convert(value)
        if isinstance(normalized, str):
            text = normalized.replace("\r", " ").replace("\n", " ").strip()
        else:
            text = json.dumps(normalized, sort_keys=True, ensure_ascii=True, separators=(",", ":"))
        if len(text) > limit:
            return f"{text[:limit]}..."
        return text

    def _format_raid_observability_fields(self, **fields: object) -> str:
        parts = []
        for key in sorted(fields):
            value = fields[key]
            if value is None:
                continue
            parts.append(f"{str(key).strip()}={self._raid_observability_value(value)}")
        return " ".join(parts)

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
        payload = {
            "raid_flow_id": str(raid_flow_id or "").strip() or None,
            "step": str(step or "").strip() or None,
            "decision": str(decision or "").strip() or None,
            "from_broadcaster_login": self._normalize_broadcaster_login(from_broadcaster_login),
            "from_broadcaster_id": str(from_broadcaster_id or "").strip() or None,
            "to_broadcaster_login": self._normalize_broadcaster_login(to_broadcaster_login),
            "to_broadcaster_id": str(to_broadcaster_id or "").strip() or None,
            "details": details or {},
        }
        self._last_raid_observability_event = payload
        log.log(level, "raid_flow %s", self._format_raid_observability_fields(**payload))
        insert_observability_event(
            flow_type="raid",
            flow_id=str(payload.get("raid_flow_id") or ""),
            entity_login=str(payload.get("to_broadcaster_login") or payload.get("from_broadcaster_login") or ""),
            entity_id=str(payload.get("to_broadcaster_id") or payload.get("from_broadcaster_id") or ""),
            step=str(payload.get("step") or "event"),
            decision=str(payload.get("decision") or "unknown"),
            details=payload,
        )

    def get_observability_snapshot(self) -> dict[str, Any]:
        self._ensure_runtime_raid_tracking_state()
        pending_raids = getattr(self, "_pending_raids", {}) or {}
        recent_arrivals = getattr(self, "_recent_raid_arrivals", {}) or {}
        orphan_notifications = getattr(self, "_orphan_chat_raid_notifications", {}) or {}
        readiness_by_flow = getattr(self, "_raid_readiness_by_flow_id", {}) or {}
        return {
            "pendingCount": len(pending_raids),
            "pendingTargets": sorted(str(key) for key in list(pending_raids.keys())[:10]),
            "recentArrivalCount": len(recent_arrivals),
            "orphanChatNotificationCount": len(orphan_notifications),
            "readinessFlowCount": len(readiness_by_flow),
            "counters": dict(self._raid_observability_counters()),
            "lastEvent": getattr(self, "_last_raid_observability_event", None),
        }

    def _ensure_runtime_raid_tracking_state(self) -> None:
        if not isinstance(getattr(self, "_recent_raid_arrivals", None), dict):
            self._recent_raid_arrivals = {}
        if not isinstance(getattr(self, "_orphan_chat_raid_notifications", None), dict):
            self._orphan_chat_raid_notifications = {}
        readiness_by_flow = getattr(self, "_raid_readiness_by_flow_id", None)
        if not isinstance(readiness_by_flow, dict):
            self._raid_readiness_by_flow_id = {}

    def _coerce_pending_raid_record(
        self,
        pending: dict[str, Any] | tuple[Any, ...] | None,
        *,
        to_broadcaster_id: str | None = None,
    ) -> dict[str, Any] | None:
        if pending is None:
            return None
        if isinstance(pending, dict):
            record = dict(pending)
        else:
            record = {
                "from_broadcaster_login": pending[0] if len(pending) > 0 else "",
                "target_stream_data": pending[1] if len(pending) > 1 else None,
                "registered_ts": pending[2] if len(pending) > 2 else time.time(),
                "is_partner_raid": pending[3] if len(pending) > 3 else False,
                "registered_viewer_count": pending[4] if len(pending) > 4 else 0,
                "offline_trigger_ts": pending[5] if len(pending) > 5 else None,
            }
        record["to_broadcaster_id"] = str(
            record.get("to_broadcaster_id") or to_broadcaster_id or ""
        ).strip()
        record["from_broadcaster_login"] = self._normalize_broadcaster_login(
            record.get("from_broadcaster_login")
        )
        record["registered_ts"] = float(record.get("registered_ts") or time.time())
        record["is_partner_raid"] = bool(record.get("is_partner_raid"))
        record["registered_viewer_count"] = int(record.get("registered_viewer_count") or 0)
        record["raid_flow_id"] = str(record.get("raid_flow_id") or "").strip() or None
        record["channel_raid_ready_detail"] = (
            str(record.get("channel_raid_ready_detail") or "").strip() or None
        )
        offline_trigger_ts = record.get("offline_trigger_ts")
        record["offline_trigger_ts"] = (
            float(offline_trigger_ts) if offline_trigger_ts is not None else None
        )
        signal_observations = record.get("signal_observations")
        record["signal_observations"] = (
            dict(signal_observations) if isinstance(signal_observations, dict) else {}
        )
        return record

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
    ) -> dict[str, Any]:
        return {
            "from_broadcaster_login": self._normalize_broadcaster_login(from_broadcaster_login),
            "to_broadcaster_id": str(to_broadcaster_id or "").strip(),
            "target_stream_data": target_stream_data,
            "registered_ts": time.time(),
            "is_partner_raid": bool(is_partner_raid),
            "registered_viewer_count": int(viewer_count or 0),
            "offline_trigger_ts": float(offline_trigger_ts) if offline_trigger_ts else None,
            "raid_flow_id": str(raid_flow_id or "").strip() or None,
            "channel_raid_ready": channel_raid_ready,
            "channel_raid_ready_detail": str(channel_raid_ready_detail or "").strip() or None,
            "chat_notification_state": str(chat_notification_state or "").strip() or None,
            "chat_notification_detail": str(chat_notification_detail or "").strip() or None,
            "signal_observations": {},
        }

    @staticmethod
    def _record_pending_signal_observation(
        pending_record: dict[str, Any],
        *,
        signal_type: str,
        status: str,
        reason: str | None = None,
        detail: str | None = None,
    ) -> None:
        signal_observations = pending_record.get("signal_observations")
        if not isinstance(signal_observations, dict):
            signal_observations = {}
            pending_record["signal_observations"] = signal_observations
        observation = {"status": str(status or "").strip()}
        if reason:
            observation["reason"] = str(reason).strip()
        if detail:
            observation["detail"] = str(detail).strip()
        signal_observations[str(signal_type)] = observation

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
        key = self._build_raid_arrival_cache_key(
            to_broadcaster_id=str(payload.get("to_broadcaster_id") or "").strip(),
            from_broadcaster_login=str(payload.get("from_broadcaster_login") or "").strip(),
        )
        payload_copy = dict(payload)
        payload_copy["observed_ts"] = float(payload_copy.get("observed_ts") or time.time())
        self._orphan_chat_raid_notifications[key] = payload_copy

    def _pop_orphan_chat_raid_notification(
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
        return self._orphan_chat_raid_notifications.pop(key, None)

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
            self._pop_orphan_chat_raid_notification(
                to_broadcaster_id=str(payload.get("to_broadcaster_id") or ""),
                from_broadcaster_login=str(payload.get("from_broadcaster_login") or ""),
            )
            self._process_independent_partner_raid_arrival(
                to_broadcaster_id=str(payload.get("to_broadcaster_id") or ""),
                to_broadcaster_login=str(payload.get("to_broadcaster_login") or ""),
                from_broadcaster_login=str(payload.get("from_broadcaster_login") or ""),
                from_broadcaster_id=str(payload.get("from_broadcaster_id") or "") or None,
                viewer_count=int(payload.get("viewer_count") or 0),
                signal_type="channel.chat.notification",
                correlation_status="orphan_chat_notification",
                correlation_detail="channel.chat.notification arrived before pending raid registration",
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
            with get_conn() as conn:
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
    ) -> bool:
        broadcaster_key = str(broadcaster_id or "").strip()
        login_key = self._normalize_broadcaster_login(broadcaster_login)
        try:
            with get_conn() as conn:
                row = load_active_partner(
                    conn,
                    twitch_user_id=broadcaster_key or None,
                    twitch_login=login_key or None,
                )
            return bool(row)
        except Exception:
            log.debug(
                "Partner target lookup failed for %s (%s)",
                login_key,
                broadcaster_key,
                exc_info=True,
            )
            return False

    def _classify_partner_raid_arrival(
        self,
        *,
        from_broadcaster_login: str,
        from_broadcaster_id: str | None,
        to_broadcaster_id: str,
        to_broadcaster_login: str,
    ) -> tuple[str | None, str]:
        if not self._is_partner_target_channel(
            broadcaster_id=to_broadcaster_id,
            broadcaster_login=to_broadcaster_login,
        ):
            return None, "non_partner_target"

        known_source = self._resolve_known_streamer_identity(
            broadcaster_login=from_broadcaster_login,
            broadcaster_id=from_broadcaster_id,
        )
        if known_source:
            if known_source.get("twitch_user_id"):
                return "ours_to_partner", "known_streamer_id"
            return "ours_to_partner", "known_streamer_login"

        if not self._normalize_broadcaster_login(from_broadcaster_login) and not str(
            from_broadcaster_id or ""
        ).strip():
            return "unknown_source_to_partner", "missing_source_identity"

        return "external_to_partner", "unmatched_source"

    def _load_recent_raid_history_reference(
        self,
        *,
        from_broadcaster_login: str,
        to_broadcaster_id: str,
    ) -> tuple[int | None, str | None]:
        try:
            with get_conn() as conn:
                row = conn.execute(
                    """
                    SELECT id, executed_at
                    FROM twitch_raid_history
                    WHERE LOWER(from_broadcaster_login) = ?
                      AND to_broadcaster_id = ?
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
            with get_conn() as conn:
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
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
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
            with get_conn() as conn:
                conn.execute(
                    """
                    UPDATE twitch_raid_arrival_tracking
                    SET confirmation_signals = ?,
                        last_signal_at = CURRENT_TIMESTAMP,
                        unraid_seen = CASE WHEN ? THEN TRUE ELSE unraid_seen END,
                        last_unraid_at = CASE WHEN ? THEN ? ELSE last_unraid_at END
                    WHERE id = ?
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
        with get_conn() as conn:
            rows = conn.execute(
                """
                SELECT DISTINCT s.twitch_login, s.twitch_user_id,
                       r.raid_enabled, r.authorized_at
                  FROM twitch_streamers_partner_state s
                  LEFT JOIN twitch_raid_auth r ON s.twitch_user_id = r.twitch_user_id
                 WHERE s.is_partner_active = 1
                   AND s.twitch_user_id IS NOT NULL
                   AND s.twitch_login IS NOT NULL
                   AND s.twitch_user_id != ?
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

        placeholders = ",".join("?" * len(partner_logins_lower))
        with get_conn() as conn:
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
        with get_conn() as conn:
            row = conn.execute(
                """
                SELECT twitch_user_id, streamer_login, is_live, last_started_at,
                       last_game, last_viewer_count, had_deadlock_in_session, last_deadlock_seen_at
                  FROM twitch_live_state
                 WHERE twitch_user_id = ?
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
            with get_conn() as conn:
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
    ) -> str | None:
        provided_discord_id = self._normalize_discord_user_id(state_discord_user_id)
        existing_discord_id: str | None = None
        existing_display_name: str | None = None

        with get_conn() as conn:
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
        with get_conn() as conn:
            promote_streamer_to_partner(
                conn,
                twitch_login=twitch_login,
                twitch_user_id=twitch_user_id,
                discord_user_id=final_discord_id,
                discord_display_name=final_display_name,
                is_on_discord=is_on_discord_value,
                manual_verified_permanent=1,
                manual_verified_until=None,
                manual_verified_at=datetime.now(UTC).isoformat(),
                manual_partner_opt_out=0,
                raid_bot_enabled=1,
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
        """
        Entfernt pending raids, die älter als 5 Minuten sind (wahrscheinlich fehlgeschlagen).
        """
        now = time.time()
        timeout = 300  # 5 Minuten
        stale = [
            to_id
            for to_id, pending in self._pending_raids.items()
            if now
            - float(
                (
                    self._coerce_pending_raid_record(pending, to_broadcaster_id=to_id) or {}
                ).get("registered_ts")
                or 0.0
            )
            > timeout
        ]
        for to_id in stale:
            pending = self._coerce_pending_raid_record(
                self._pending_raids.pop(to_id),
                to_broadcaster_id=to_id,
            )
            if pending is None:
                continue
            from_login = str(pending.get("from_broadcaster_login") or "<unknown>")
            registered_ts = float(pending.get("registered_ts") or 0.0)
            offline_ts = pending.get("offline_trigger_ts")
            age = now - registered_ts
            offline_pending_s = (time.monotonic() - offline_ts) if offline_ts else -1.0
            raid_flow_id = str(pending.get("raid_flow_id") or "").strip() or self._next_raid_observability_flow_id(prefix="raid-timeout")
            self._increment_raid_observability_counter("raid_pending_timeout_total")
            log.warning(
                "Pending raid timed out after %.0fs: %s -> (ID: %s). %s offline->pending=%.0fs",
                age,
                from_login,
                to_id,
                self._build_pending_timeout_detail(pending),
                offline_pending_s,
            )
            self._log_raid_observability_event(
                raid_flow_id=raid_flow_id,
                step="pending_timeout",
                decision="timeout",
                level=logging.WARNING,
                from_broadcaster_login=from_login,
                to_broadcaster_id=to_id,
                details={
                    "age_seconds": round(age, 1),
                    "offline_to_pending_seconds": round(offline_pending_s, 1),
                    "timeout_detail": self._build_pending_timeout_detail(pending),
                },
            )

    def _clear_superseded_pending_raids(
        self,
        *,
        from_broadcaster_login: str,
        current_target_id: str,
    ) -> None:
        normalized_from = str(from_broadcaster_login or "").strip().lower()
        if not normalized_from:
            return

        current_target_key = str(current_target_id or "").strip()
        superseded: list[tuple[str, dict[str, Any]]] = []
        for to_id, pending in list(self._pending_raids.items()):
            if str(to_id) == current_target_key:
                continue
            pending_record = self._coerce_pending_raid_record(pending, to_broadcaster_id=to_id)
            if pending_record is None:
                continue
            pending_from = str(pending_record.get("from_broadcaster_login") or "").strip().lower()
            if pending_from != normalized_from:
                continue
            superseded.append((str(to_id), pending_record))

        for to_id, pending_record in superseded:
            removed = self._pending_raids.pop(to_id, None)
            if removed is None:
                continue
            target_stream_data = pending_record.get("target_stream_data")
            old_target_login = ""
            if isinstance(target_stream_data, dict):
                old_target_login = str(target_stream_data.get("user_login") or "").strip().lower()
            raid_flow_id = str(pending_record.get("raid_flow_id") or "").strip() or self._next_raid_observability_flow_id(prefix="raid-supersede")
            log.info(
                "Pending raid superseded before arrival: %s old_target=%s%s replaced_by=%s",
                from_broadcaster_login,
                to_id,
                f' ({old_target_login})' if old_target_login else "",
                current_target_key,
            )
            self._log_raid_observability_event(
                raid_flow_id=raid_flow_id,
                step="pending_superseded",
                decision="superseded",
                from_broadcaster_login=from_broadcaster_login,
                to_broadcaster_login=old_target_login or None,
                to_broadcaster_id=to_id,
                details={"replaced_by": current_target_key},
            )

    def _cancel_pending_raids_for_source_unraid(
        self,
        *,
        from_broadcaster_login: str,
        from_broadcaster_id: str | None = None,
        message_id: str | None = None,
        event_timestamp: str | None = None,
    ) -> int:
        normalized_from = self._normalize_broadcaster_login(from_broadcaster_login)
        if not normalized_from:
            return 0

        canceled = 0
        for to_id, pending in list(self._pending_raids.items()):
            pending_record = self._coerce_pending_raid_record(pending, to_broadcaster_id=to_id)
            if pending_record is None:
                continue
            pending_from = self._normalize_broadcaster_login(
                pending_record.get("from_broadcaster_login")
            )
            if pending_from != normalized_from:
                continue

            self._record_pending_signal_observation(
                pending_record,
                signal_type="channel.chat.notification.unraid_source",
                status="canceled",
                reason="source_self_unraid",
                detail=str(event_timestamp or message_id or "").strip() or None,
            )
            removed = self._pending_raids.pop(str(to_id), None)
            if removed is None:
                continue

            target_stream_data = pending_record.get("target_stream_data")
            target_login = ""
            if isinstance(target_stream_data, dict):
                target_login = self._normalize_broadcaster_login(
                    target_stream_data.get("user_login")
                )
            target_login = target_login or str(to_id)
            raid_flow_id = (
                str(pending_record.get("raid_flow_id") or "").strip()
                or self._next_raid_observability_flow_id(prefix="raid-source-unraid")
            )
            self._increment_raid_observability_counter("raid_pending_canceled_source_unraid_total")
            log.info(
                "Pending raid canceled by source unraid: %s -> %s (message_id=%s)",
                normalized_from,
                target_login,
                message_id or "n/a",
            )
            self._log_raid_observability_event(
                raid_flow_id=raid_flow_id,
                step="pending_canceled_source_unraid",
                decision="canceled",
                from_broadcaster_login=normalized_from,
                from_broadcaster_id=from_broadcaster_id,
                to_broadcaster_login=target_login if target_login != str(to_id) else None,
                to_broadcaster_id=str(to_id),
                details={"message_id": message_id, "event_timestamp": event_timestamp},
            )
            canceled += 1

        return canceled

    async def _ensure_raid_arrival_subscription_ready(
        self,
        *,
        to_broadcaster_id: str,
        to_broadcaster_login: str,
        raid_flow_id: str | None = None,
    ) -> bool:
        self._ensure_runtime_raid_tracking_state()
        cog = self._cog
        flow_id = str(raid_flow_id or "").strip() or self._next_raid_observability_flow_id(prefix="raid-ready")
        if cog is None:
            self._log_raid_observability_event(
                raid_flow_id=flow_id,
                step="readiness_check",
                decision="no_cog_best_effort",
                to_broadcaster_login=to_broadcaster_login,
                to_broadcaster_id=to_broadcaster_id,
            )
            return True

        has_sub = getattr(cog, "_eventsub_has_sub", None)
        locally_tracked = False
        if callable(has_sub):
            try:
                locally_tracked = bool(has_sub("channel.raid", str(to_broadcaster_id)))
            except Exception:
                log.debug(
                    "EventSub channel.raid local tracking lookup failed for %s",
                    to_broadcaster_login,
                    exc_info=True,
                )

        ensure_ready = getattr(cog, "ensure_raid_target_dynamic_ready", None)
        if callable(ensure_ready):
            try:
                ready, detail = await ensure_ready(
                    str(to_broadcaster_id),
                    to_broadcaster_login,
                    raid_flow_id=flow_id,
                )
            except Exception:
                self._increment_raid_observability_counter("raid_eventsub_ready_check_failed_total")
                self._log_raid_observability_event(
                    raid_flow_id=flow_id,
                    step="readiness_check",
                    decision="exception",
                    level=logging.ERROR,
                    to_broadcaster_login=to_broadcaster_login,
                    to_broadcaster_id=to_broadcaster_id,
                    details={"local_tracking": locally_tracked},
                )
                log.exception(
                    "EventSub channel.raid readiness check failed for %s",
                    to_broadcaster_login,
                )
                return False

            self._raid_readiness_by_flow_id[flow_id] = {
                "ready": bool(ready),
                "detail": str(detail or "").strip() or None,
                "local_tracking": bool(locally_tracked),
            }

            if ready:
                self._increment_raid_observability_counter("raid_eventsub_ready_true_total")
                log.info(
                    "EventSub channel.raid ready before raid start for %s (%s)",
                    to_broadcaster_login,
                    detail,
                )
            else:
                self._increment_raid_observability_counter("raid_eventsub_ready_false_total")
                if locally_tracked:
                    self._increment_raid_observability_counter("raid_eventsub_ready_false_local_true_total")
                    detail = f"{detail}; local_tracking_only"
                log.warning(
                    "EventSub channel.raid not confirmed enabled for %s before raid start (%s). Proceeding best-effort.",
                    to_broadcaster_login,
                    detail,
                )
            self._log_raid_observability_event(
                raid_flow_id=flow_id,
                step="readiness_check",
                decision="ready" if ready else "not_ready",
                level=logging.INFO if ready else logging.WARNING,
                to_broadcaster_login=to_broadcaster_login,
                to_broadcaster_id=to_broadcaster_id,
                details={"local_tracking": locally_tracked, "detail": detail},
            )
            return ready

        if locally_tracked:
            log.debug(
                "EventSub channel.raid for %s is only locally tracked; remote readiness check unavailable.",
                to_broadcaster_login,
            )
        self._raid_readiness_by_flow_id[flow_id] = {
            "ready": True,
            "detail": "local_tracking_only" if locally_tracked else "best_effort",
            "local_tracking": bool(locally_tracked),
        }
        self._log_raid_observability_event(
            raid_flow_id=flow_id,
            step="readiness_check",
            decision="best_effort",
            to_broadcaster_login=to_broadcaster_login,
            to_broadcaster_id=to_broadcaster_id,
            details={"local_tracking": locally_tracked},
        )
        return True

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
        """
        Registriert einen Raid, der auf EventSub Bestätigung wartet.

        Wird aufgerufen nach erfolgreichem API-Call, bevor der Raid tatsächlich beim Ziel ankommt.
        Erstellt dynamisch eine channel.raid EventSub subscription für das Ziel.

        Args:
            from_broadcaster_login: Login des Raiding-Streamers
            to_broadcaster_id: User-ID des Raid-Ziels
            to_broadcaster_login: Login des Raid-Ziels
            target_stream_data: Stream-Daten des Ziels (optional)
            is_partner_raid: True wenn es ein Partner-Raid ist (für Partner-Message)
            viewer_count: Viewer-Count beim Raid-Start (für Partner-Message)
        """
        self._ensure_runtime_raid_tracking_state()
        chat_notification_state, chat_notification_detail = (
            self._snapshot_chat_notification_subscription(to_broadcaster_login)
        )
        flow_id = str(raid_flow_id or "").strip() or self._next_raid_observability_flow_id(prefix="raid-pending")
        readiness_state = self._raid_readiness_by_flow_id.get(flow_id, {})
        self._clear_superseded_pending_raids(
            from_broadcaster_login=from_broadcaster_login,
            current_target_id=to_broadcaster_id,
        )
        pending_record = self._build_pending_raid_record(
            from_broadcaster_login=from_broadcaster_login,
            to_broadcaster_id=to_broadcaster_id,
            target_stream_data=target_stream_data,
            is_partner_raid=is_partner_raid,
            viewer_count=viewer_count,
            offline_trigger_ts=offline_trigger_ts,
            raid_flow_id=flow_id,
            channel_raid_ready=channel_raid_ready,
            channel_raid_ready_detail=str(readiness_state.get("detail") or "").strip() or None,
            chat_notification_state=chat_notification_state,
            chat_notification_detail=chat_notification_detail,
        )
        self._pending_raids[to_broadcaster_id] = pending_record
        self._increment_raid_observability_counter("raid_pending_registered_total")
        offline_to_pending_ms = (
            (time.monotonic() - offline_trigger_ts) * 1000 if offline_trigger_ts else None
        )
        log.info(
            "Pending raid registered: %s -> %s (ID: %s). Creating EventSub subscription... offline->pending=%s, chat_notification=%s",
            from_broadcaster_login,
            to_broadcaster_login,
            to_broadcaster_id,
            f"{offline_to_pending_ms:.0f}ms" if offline_to_pending_ms is not None else "n/a",
            chat_notification_state or "unknown",
        )
        self._log_raid_observability_event(
            raid_flow_id=flow_id,
            step="pending_registered",
            decision="registered",
            from_broadcaster_login=from_broadcaster_login,
            to_broadcaster_login=to_broadcaster_login,
            to_broadcaster_id=to_broadcaster_id,
            details={
                "viewer_count": viewer_count,
                "offline_to_pending_ms": int(offline_to_pending_ms) if offline_to_pending_ms is not None else None,
                "channel_raid_ready": channel_raid_ready,
                "channel_raid_ready_detail": readiness_state.get("detail"),
                "chat_notification_state": chat_notification_state,
                "chat_notification_detail": chat_notification_detail,
            },
        )

        success = bool(channel_raid_ready)
        if success:
            log.debug(
                "EventSub channel.raid readiness already confirmed for %s - skipping duplicate create",
                to_broadcaster_login,
            )

        # Dynamische EventSub subscription erstellen
        if not success and self._cog and hasattr(self._cog, "subscribe_raid_target_dynamic"):
            try:
                self._increment_raid_observability_counter("raid_eventsub_subscribe_attempt_total")
                success = await self._cog.subscribe_raid_target_dynamic(
                    to_broadcaster_id, to_broadcaster_login
                )
                if success:
                    self._increment_raid_observability_counter("raid_eventsub_subscribe_success_total")
                    log.info(
                        "EventSub channel.raid subscription created for %s",
                        to_broadcaster_login,
                    )
                else:
                    self._increment_raid_observability_counter("raid_eventsub_subscribe_failed_total")
                    log.warning(
                        "Failed to create EventSub subscription for %s - raid message may not be sent",
                        to_broadcaster_login,
                    )
            except Exception:
                self._increment_raid_observability_counter("raid_eventsub_subscribe_failed_total")
                log.exception(
                    "Error creating dynamic EventSub subscription for %s",
                    to_broadcaster_login,
                )
        else:
            if not success:
                log.warning(
                    "Cog reference not set - cannot create dynamic EventSub subscription for %s",
                    to_broadcaster_login,
                )
        self._log_raid_observability_event(
            raid_flow_id=flow_id,
            step="pending_subscription_create",
            decision="created" if success else "best_effort_only",
            level=logging.INFO if success else logging.WARNING,
            from_broadcaster_login=from_broadcaster_login,
            to_broadcaster_login=to_broadcaster_login,
            to_broadcaster_id=to_broadcaster_id,
            details={"channel_raid_ready": channel_raid_ready},
        )

        orphan_notification = self._pop_orphan_chat_raid_notification(
            to_broadcaster_id=to_broadcaster_id,
            from_broadcaster_login=from_broadcaster_login,
        )
        if orphan_notification:
            self._increment_raid_observability_counter("raid_orphan_chat_notification_total")
            log.info(
                "Pending raid %s -> %s matched earlier channel.chat.notification raid signal.",
                from_broadcaster_login,
                to_broadcaster_login,
            )
            self._log_raid_observability_event(
                raid_flow_id=flow_id,
                step="pending_orphan_notification_match",
                decision="matched",
                from_broadcaster_login=from_broadcaster_login,
                to_broadcaster_login=to_broadcaster_login,
                to_broadcaster_id=to_broadcaster_id,
                details={"message_id": orphan_notification.get("message_id")},
            )
            await self.on_chat_raid_notification(
                to_broadcaster_id=str(orphan_notification.get("to_broadcaster_id") or to_broadcaster_id),
                to_broadcaster_login=str(
                    orphan_notification.get("to_broadcaster_login") or to_broadcaster_login
                ),
                from_broadcaster_login=str(
                    orphan_notification.get("from_broadcaster_login") or from_broadcaster_login
                ),
                viewer_count=int(orphan_notification.get("viewer_count") or viewer_count),
                from_broadcaster_id=str(orphan_notification.get("from_broadcaster_id") or "") or None,
                message_id=str(orphan_notification.get("message_id") or "") or None,
                event_timestamp=str(orphan_notification.get("event_timestamp") or "") or None,
            )

    def _build_pending_timeout_detail(self, pending_record: dict[str, Any]) -> str:
        observations = pending_record.get("signal_observations")
        observation_parts: list[str] = []
        if isinstance(observations, dict):
            for signal_type in ("channel.raid", "channel.chat.notification"):
                observation = observations.get(signal_type)
                if not isinstance(observation, dict):
                    continue
                status = str(observation.get("status") or "").strip()
                reason = str(observation.get("reason") or "").strip()
                detail = str(observation.get("detail") or "").strip()
                text = f"{signal_type}:{status}" if status else signal_type
                if reason:
                    text += f" ({reason})"
                if detail:
                    text += f" [{detail}]"
                observation_parts.append(text)

        if not observation_parts:
            channel_raid_ready = pending_record.get("channel_raid_ready")
            channel_raid_detail = (
                "ready" if channel_raid_ready is not False else "subscription_not_ready"
            )
            chat_state = str(pending_record.get("chat_notification_state") or "").strip()
            chat_detail = str(pending_record.get("chat_notification_detail") or "").strip()
            if not chat_state:
                chat_state = "missing"
            chat_text = f"channel.chat.notification:{chat_state}"
            if chat_detail:
                chat_text += f" [{chat_detail}]"
            observation_parts.extend(
                [
                    f"channel.raid:{channel_raid_detail}",
                    chat_text,
                ]
            )

        return "Timeout detail: " + "; ".join(observation_parts)

    def _lookup_silent_raid_enabled(self, broadcaster_login: str) -> bool:
        try:
            with get_conn() as conn:
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
        recent_arrival = self._lookup_recent_raid_arrival(
            to_broadcaster_id=to_broadcaster_id,
            from_broadcaster_login=from_broadcaster_login,
        )
        if not recent_arrival:
            return False

        confirmation_signals = set(recent_arrival.get("confirmation_signals") or set())
        confirmation_signals.add(signal_type)
        recent_arrival["confirmation_signals"] = confirmation_signals
        recent_arrival["confirmed_ts"] = time.time()
        recent_arrival["viewer_count"] = max(
            int(recent_arrival.get("viewer_count") or 0),
            int(viewer_count or 0),
        )
        arrival_tracking_id = int(recent_arrival.get("arrival_tracking_id") or 0) or None
        if arrival_tracking_id is not None:
            self._update_partner_raid_arrival(
                arrival_tracking_id=arrival_tracking_id,
                confirmation_signals=confirmation_signals,
                unraid_seen=unraid_seen,
            )

        raid_flow_id = str(recent_arrival.get("raid_flow_id") or "").strip() or self._next_raid_observability_flow_id(prefix="raid-secondary")
        self._log_raid_observability_event(
            raid_flow_id=raid_flow_id,
            step="secondary_signal",
            decision=signal_type,
            from_broadcaster_login=from_broadcaster_login,
            to_broadcaster_login=to_broadcaster_login,
            to_broadcaster_id=to_broadcaster_id,
            details={"confirmation_signals": sorted(confirmation_signals), "unraid_seen": unraid_seen},
        )

        log.info(
            "Raid arrival secondary signal recorded: %s -> %s via %s (signals=%s)",
            from_broadcaster_login,
            to_broadcaster_login,
            signal_type,
            self._serialize_confirmation_signals(confirmation_signals),
        )
        return True

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
        pending = self._coerce_pending_raid_record(
            self._pending_raids.pop(to_broadcaster_id, None),
            to_broadcaster_id=to_broadcaster_id,
        )
        if pending is None:
            return
        raid_flow_id = str(pending.get("raid_flow_id") or "").strip() or self._next_raid_observability_flow_id(prefix="raid-arrival")

        target_stream_data = pending.get("target_stream_data")
        registered_ts = float(pending.get("registered_ts") or time.time())
        is_partner_raid = bool(pending.get("is_partner_raid"))
        registered_viewer_count = int(pending.get("registered_viewer_count") or viewer_count)
        offline_trigger_ts = pending.get("offline_trigger_ts")
        effective_viewer_count = int(viewer_count or registered_viewer_count or 0)

        classification, source_resolution = self._classify_partner_raid_arrival(
            from_broadcaster_login=from_broadcaster_login,
            from_broadcaster_id=from_broadcaster_id,
            to_broadcaster_id=to_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
        )
        raid_history_id = None
        raid_history_executed_at = None
        if is_partner_raid:
            raid_history_id, raid_history_executed_at = self._load_recent_raid_history_reference(
                from_broadcaster_login=from_broadcaster_login,
                to_broadcaster_id=to_broadcaster_id,
            )

        log.info(
            "Raid arrival confirmed via %s: %s -> %s (%d viewers, partner_raid=%s, classification=%s, api->arrival=%.0fs, offline->arrival=%.0fs)",
            signal_type,
            from_broadcaster_login,
            to_broadcaster_login,
            effective_viewer_count,
            is_partner_raid,
            classification or "non_partner_target",
            time.time() - registered_ts,
            (time.monotonic() - offline_trigger_ts) if offline_trigger_ts else -1.0,
        )

        arrival_tracking_id = None
        if classification is not None:
            arrival_tracking_id = self._store_partner_raid_arrival(
                from_broadcaster_id=from_broadcaster_id,
                from_broadcaster_login=from_broadcaster_login,
                to_broadcaster_id=to_broadcaster_id,
                to_broadcaster_login=to_broadcaster_login,
                viewer_count=effective_viewer_count,
                classification=classification,
                confirmation_signals={signal_type},
                primary_signal=signal_type,
                correlation_status="matched_pending",
                correlation_detail=None,
                source_resolution=source_resolution,
                raid_history_id=raid_history_id,
                raid_history_executed_at=raid_history_executed_at,
            )

        self._remember_recent_raid_arrival(
            to_broadcaster_id=to_broadcaster_id,
            from_broadcaster_login=from_broadcaster_login,
            from_broadcaster_id=from_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            viewer_count=effective_viewer_count,
            classification=classification,
            confirmation_signals={signal_type},
            arrival_tracking_id=arrival_tracking_id,
            raid_flow_id=raid_flow_id,
        )
        self._increment_raid_observability_counter(f"raid_arrival_confirmed_{signal_type.replace('.', '_')}_total")
        self._log_raid_observability_event(
            raid_flow_id=raid_flow_id,
            step="arrival_confirmed",
            decision=signal_type,
            from_broadcaster_login=from_broadcaster_login,
            from_broadcaster_id=from_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            to_broadcaster_id=to_broadcaster_id,
            details={
                "classification": classification,
                "source_resolution": source_resolution,
                "viewer_count": effective_viewer_count,
                "api_to_arrival_seconds": round(time.time() - registered_ts, 2),
            },
        )

        if is_partner_raid:
            await self._refresh_partner_score_cache_if_available(
                to_broadcaster_id,
                reason="incoming_partner_raid_confirmed",
            )
            if callable(track_confirmed_partner_raid):
                track_confirmed_partner_raid(
                    to_broadcaster_id=to_broadcaster_id,
                    to_broadcaster_login=to_broadcaster_login,
                    from_broadcaster_login=from_broadcaster_login,
                    from_broadcaster_id=from_broadcaster_id,
                    viewer_count=effective_viewer_count,
                    score_snapshot=(
                        target_stream_data.get("_partner_score")
                        if isinstance(target_stream_data, dict)
                        else None
                    ),
                )

        if self._lookup_silent_raid_enabled(to_broadcaster_login):
            log.info(
                "Raid message suppressed (silent_raid): %s -> %s",
                from_broadcaster_login,
                to_broadcaster_login,
            )
            return

        if is_partner_raid:
            await self._send_partner_raid_message(
                from_broadcaster_login=from_broadcaster_login,
                to_broadcaster_login=to_broadcaster_login,
                to_broadcaster_id=to_broadcaster_id,
                viewer_count=effective_viewer_count,
            )
        else:
            await self._send_recruitment_message_now(
                from_broadcaster_login=from_broadcaster_login,
                to_broadcaster_login=to_broadcaster_login,
                target_stream_data=target_stream_data,
            )

    async def on_raid_arrival(
        self,
        to_broadcaster_id: str,
        to_broadcaster_login: str,
        from_broadcaster_login: str,
        viewer_count: int,
        from_broadcaster_id: str | None = None,
    ):
        """
        Wird aufgerufen, wenn ein channel.raid EventSub Event eintrifft.

        Sendet entweder:
        - Partner-Message (bei Partner-Raids)
        - Recruitment-Message (bei Non-Partner-Raids)
        """
        normalized_from_login = self._normalize_broadcaster_login(from_broadcaster_login)
        pending = self._coerce_pending_raid_record(
            self._pending_raids.get(to_broadcaster_id),
            to_broadcaster_id=to_broadcaster_id,
        )

        if self._handle_secondary_confirmed_signal(
            signal_type="channel.raid",
            to_broadcaster_id=to_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            from_broadcaster_login=normalized_from_login,
            viewer_count=viewer_count,
        ):
            return

        if not pending:
            if self._process_independent_partner_raid_arrival(
                to_broadcaster_id=to_broadcaster_id,
                to_broadcaster_login=to_broadcaster_login,
                from_broadcaster_login=normalized_from_login,
                from_broadcaster_id=from_broadcaster_id,
                viewer_count=viewer_count,
                signal_type="channel.raid",
                correlation_status="independent_channel_raid",
                correlation_detail=None,
            ):
                return
            from_broadcaster_key = str(from_broadcaster_id or "").strip()
            if not from_broadcaster_key:
                from_broadcaster_key = self._resolve_streamer_id_by_login(normalized_from_login) or ""
            if from_broadcaster_key:
                self.mark_manual_raid_started(from_broadcaster_key, ttl_seconds=180.0)
                log.info(
                    "External/manual raid detected via EventSub: %s -> %s. "
                    "Suppressing next offline auto-raid for broadcaster_id=%s (ttl=180s/3min)",
                    normalized_from_login,
                    to_broadcaster_login,
                    from_broadcaster_key,
                )
            log.debug(
                "Raid arrival ignored (not pending): %s -> %s",
                normalized_from_login,
                to_broadcaster_login,
            )
            self._log_raid_observability_event(
                raid_flow_id=self._next_raid_observability_flow_id(prefix="raid-independent"),
                step="arrival_no_pending",
                decision="ignored_or_independent",
                from_broadcaster_login=normalized_from_login,
                from_broadcaster_id=from_broadcaster_id,
                to_broadcaster_login=to_broadcaster_login,
                to_broadcaster_id=to_broadcaster_id,
            )
            return

        expected_from = str(pending.get("from_broadcaster_login") or normalized_from_login)
        if expected_from != normalized_from_login:
            self._record_pending_signal_observation(
                pending,
                signal_type="channel.raid",
                status="ignored",
                reason="source_target_mismatch",
                detail=f"expected={expected_from} actual={normalized_from_login}",
            )
            self._pending_raids[to_broadcaster_id] = pending
            log.warning(
                "Raid arrival mismatch: expected from %s, got from %s",
                expected_from,
                normalized_from_login,
            )
            self._log_raid_observability_event(
                raid_flow_id=str(pending.get("raid_flow_id") or "") or self._next_raid_observability_flow_id(prefix="raid-mismatch"),
                step="arrival_mismatch",
                decision="ignored",
                level=logging.WARNING,
                from_broadcaster_login=normalized_from_login,
                to_broadcaster_login=to_broadcaster_login,
                to_broadcaster_id=to_broadcaster_id,
                details={"expected_from": expected_from},
            )
            return

        self._record_pending_signal_observation(
            pending,
            signal_type="channel.raid",
            status="matched_pending",
        )
        self._pending_raids[to_broadcaster_id] = pending
        await self._confirm_pending_raid_arrival(
            signal_type="channel.raid",
            to_broadcaster_id=to_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            from_broadcaster_login=normalized_from_login,
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
        normalized_from_login = self._normalize_broadcaster_login(from_broadcaster_login)
        pending = self._coerce_pending_raid_record(
            self._pending_raids.get(to_broadcaster_id),
            to_broadcaster_id=to_broadcaster_id,
        )

        if self._handle_secondary_confirmed_signal(
            signal_type="channel.chat.notification",
            to_broadcaster_id=to_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            from_broadcaster_login=normalized_from_login,
            viewer_count=viewer_count,
        ):
            return

        if not pending:
            self._store_orphan_chat_raid_notification(
                {
                    "to_broadcaster_id": str(to_broadcaster_id or "").strip(),
                    "to_broadcaster_login": self._normalize_broadcaster_login(
                        to_broadcaster_login
                    ),
                    "from_broadcaster_id": str(from_broadcaster_id or "").strip() or None,
                    "from_broadcaster_login": normalized_from_login,
                    "viewer_count": int(viewer_count or 0),
                    "message_id": str(message_id or "").strip() or None,
                    "event_timestamp": str(event_timestamp or "").strip() or None,
                    "observed_ts": time.time(),
                }
            )
            self._increment_raid_observability_counter("raid_orphan_chat_notification_total")
            log.info(
                "Orphan channel.chat.notification raid observed: %s -> %s (viewer_count=%d, grace=%.0fs, message_id=%s)",
                normalized_from_login,
                to_broadcaster_login,
                viewer_count,
                _PENDING_CHAT_NOTIFICATION_GRACE_SECONDS,
                message_id or "n/a",
            )
            self._log_raid_observability_event(
                raid_flow_id=self._next_raid_observability_flow_id(prefix="raid-orphan"),
                step="chat_notification_orphaned",
                decision="stored",
                from_broadcaster_login=normalized_from_login,
                from_broadcaster_id=from_broadcaster_id,
                to_broadcaster_login=to_broadcaster_login,
                to_broadcaster_id=to_broadcaster_id,
                details={"viewer_count": viewer_count, "message_id": message_id},
            )
            return

        expected_from = str(pending.get("from_broadcaster_login") or normalized_from_login)
        if expected_from != normalized_from_login:
            self._record_pending_signal_observation(
                pending,
                signal_type="channel.chat.notification",
                status="ignored",
                reason="source_target_mismatch",
                detail=f"expected={expected_from} actual={normalized_from_login}",
            )
            self._pending_raids[to_broadcaster_id] = pending
            log.warning(
                "Raid chat notification mismatch: expected from %s, got from %s",
                expected_from,
                normalized_from_login,
            )
            self._log_raid_observability_event(
                raid_flow_id=str(pending.get("raid_flow_id") or "") or self._next_raid_observability_flow_id(prefix="raid-chat-mismatch"),
                step="chat_notification_mismatch",
                decision="ignored",
                level=logging.WARNING,
                from_broadcaster_login=normalized_from_login,
                to_broadcaster_login=to_broadcaster_login,
                to_broadcaster_id=to_broadcaster_id,
                details={"expected_from": expected_from, "message_id": message_id},
            )
            return

        self._record_pending_signal_observation(
            pending,
            signal_type="channel.chat.notification",
            status="matched_pending",
            detail=str(message_id or "").strip() or None,
        )
        self._pending_raids[to_broadcaster_id] = pending
        await self._confirm_pending_raid_arrival(
            signal_type="channel.chat.notification",
            to_broadcaster_id=to_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            from_broadcaster_login=normalized_from_login,
            viewer_count=viewer_count,
            from_broadcaster_id=from_broadcaster_id,
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
        normalized_from_login = self._normalize_broadcaster_login(from_broadcaster_login)
        pending = self._coerce_pending_raid_record(
            self._pending_raids.get(to_broadcaster_id),
            to_broadcaster_id=to_broadcaster_id,
        )
        if pending is not None:
            self._record_pending_signal_observation(
                pending,
                signal_type="channel.chat.notification.unraid",
                status="diagnostic_only",
                reason="unraid_does_not_confirm",
                detail=str(event_timestamp or "").strip() or None,
            )
            self._pending_raids[to_broadcaster_id] = pending

        secondary_handled = self._handle_secondary_confirmed_signal(
            signal_type="channel.chat.notification.unraid",
            to_broadcaster_id=to_broadcaster_id,
            to_broadcaster_login=to_broadcaster_login,
            from_broadcaster_login=normalized_from_login,
            viewer_count=0,
            unraid_seen=True,
        )
        if secondary_handled:
            log.info(
                "channel.chat.notification unraid observed after confirmed raid: %s -> %s",
                normalized_from_login,
                to_broadcaster_login,
            )
            return

        log.info(
            "channel.chat.notification unraid observed without confirmed raid correlation: %s -> %s",
            normalized_from_login,
            to_broadcaster_login,
        )

    async def on_source_self_unraid_notification(
        self,
        *,
        broadcaster_id: str,
        broadcaster_login: str,
        message_id: str | None = None,
        event_timestamp: str | None = None,
    ) -> None:
        canceled = self._cancel_pending_raids_for_source_unraid(
            from_broadcaster_login=broadcaster_login,
            from_broadcaster_id=broadcaster_id,
            message_id=message_id,
            event_timestamp=event_timestamp,
        )
        if canceled > 0:
            return

        log.info(
            "Source self-unraid observed without pending auto-raid: %s (message_id=%s)",
            self._normalize_broadcaster_login(broadcaster_login),
            message_id or "n/a",
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

            viewer_word = "Viewer" if viewer_count == 1 else "Viewern"

            # 1. Channel beitreten (falls noch nicht joined)
            await self.chat_bot.join(to_broadcaster_login, channel_id=to_broadcaster_id)

            # 2. Kurze Verzögerung, damit der Bot bereit ist und der Raid-Alert durch ist
            await asyncio.sleep(5.0)

            # 3. Nachricht vorbereiten
            message = (
                f"Hey @{to_broadcaster_login}! 🎮 "
                f"@{from_broadcaster_login} hat dich gerade mit {viewer_count} {viewer_word} geraidet. "
                f"Das ist dein Raid Nr. {received_raid_count} aus dem Deadlock Streamer-Netzwerk. ❤️"
            )

            # 4. Nachricht senden
            if hasattr(self.chat_bot, "_send_chat_message"):
                success = await self.chat_bot._send_chat_message(
                    target_channel,
                    message,
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
            with get_conn() as conn:
                row = conn.execute(
                    """
                    SELECT COUNT(*)
                    FROM twitch_raid_history
                    WHERE to_broadcaster_id = ?
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

    @staticmethod
    def _parse_nonnegative_int(value: object) -> int | None:
        try:
            if value is None:
                return None
            parsed = int(value)
            return parsed if parsed >= 0 else None
        except (TypeError, ValueError):
            return None

    async def _resolve_recruitment_followers_total(
        self,
        *,
        login: str,
        target_id: str | None,
        target_stream_data: dict | None,
    ) -> int | None:
        cached_total = self._parse_nonnegative_int(
            (target_stream_data or {}).get("followers_total")
        )
        if cached_total is not None:
            return cached_total

        resolved_target_id = str(target_id or "").strip()
        if not resolved_target_id or not self.session:
            return None

        try:
            from ..api.twitch_api import TwitchAPI
        except Exception:
            return None

        try:
            api = TwitchAPI(
                self.auth_manager.client_id,
                self.auth_manager.client_secret,
                session=self.session,
            )
            # Prefer the central bot token for moderator-scoped reads; fall back to broadcaster grants.
            followers_total = None
            bot_token, _bot_id, bot_scopes = await self._resolve_bot_oauth_context()
            if bot_token and (not bot_scopes or "moderator:read:followers" in bot_scopes):
                followers_total = await api.get_followers_total(
                    resolved_target_id,
                    user_token=bot_token,
                )
            if followers_total is None:
                user_token: str | None = None
                try:
                    user_token = await self.auth_manager.get_valid_token(
                        resolved_target_id,
                        self.session,
                    )
                except Exception:
                    user_token = None
                if user_token:
                    self._warn_user_scope_fallback_once(
                        area="recruitment follower lookup",
                        subject=login or resolved_target_id,
                    )
                followers_total = await api.get_followers_total(
                    resolved_target_id,
                    user_token=user_token,
                )
        except Exception:
            log.debug("Follower-Check fehlgeschlagen fuer %s", login, exc_info=True)
            return None

        parsed_total = self._parse_nonnegative_int(followers_total)
        if parsed_total is not None and isinstance(target_stream_data, dict):
            target_stream_data["followers_total"] = parsed_total
        return parsed_total

    async def _send_recruitment_message_now(
        self,
        from_broadcaster_login: str,
        to_broadcaster_login: str,
        target_stream_data: dict | None = None,
    ):
        """
        Sendet eine Einladungs-Nachricht im Chat des geraideten Nicht-Partners.

        Diese Nachricht wird nur gesendet, wenn ein deutscher Deadlock-Streamer
        (kein Partner) geraidet wird, um ihn zur Community einzuladen.

        Zeigt dem Streamer minimale Stats als Teaser (Avg Viewer, Peak).
        """
        if not self.chat_bot:
            log.debug("Chat bot not available for recruitment message")
            return

        # 1. Sofort beitreten, damit wir bereit sind
        try:
            target_id = None
            if target_stream_data:
                target_id = target_stream_data.get("user_id")

            if not target_id:
                # Fallback: ID über Login-Namen auflösen
                users = await self.chat_bot.fetch_users(logins=[to_broadcaster_login])
                if users:
                    target_id = str(users[0].id)

            if not target_id:
                log.warning(
                    "Could not resolve user ID for recruitment message to %s",
                    to_broadcaster_login,
                )
                return

            target_channel = self._make_chat_target(to_broadcaster_login, target_id)
            suppression = self._lookup_outbound_chat_suppression(
                to_broadcaster_login,
                target_id,
                source="recruitment",
            )
            if suppression is not None:
                log.info(
                    "Skipping recruitment message to %s due stored chat suppression (code=%s, until=%s)",
                    to_broadcaster_login,
                    suppression.get("reason_code") or "unknown",
                    suppression.get("suppressed_until") or "-",
                )
                return

            await self.chat_bot.join(to_broadcaster_login, channel_id=target_id)
        except Exception:
            log.debug("Konnte Channel %s nicht vorab beitreten", to_broadcaster_login)
            target_channel = self._make_chat_target(to_broadcaster_login, target_id or "")

        # Follow-Status prüfen (Auto-Follow per API ist bei Twitch nicht mehr möglich).
        if target_id and hasattr(self.chat_bot, "follow_channel"):
            await self.chat_bot.follow_channel(target_id)

        # 2. 15 Sekunden warten, damit der Streamer den Raid-Alert verarbeiten kann
        log.info(
            "Warte 15s vor Senden der Recruitment-Message an %s...",
            to_broadcaster_login,
        )
        await asyncio.sleep(15.0)

        try:
            # 2. Anti-Spam Check: Haben wir diesen Streamer schon "kürzlich" geraidet?
            # Wir prüfen, ob es mehr als 1 erfolgreichen Raid in den letzten 24 Stunden gab.
            with get_conn() as conn:
                raid_check = conn.execute(
                    """
                    SELECT COUNT(*) FROM twitch_raid_history
                    WHERE to_broadcaster_id = ?
                      AND COALESCE(success, FALSE) IS TRUE
                      AND executed_at > datetime('now', '-1 day')
                    """,
                    (target_id,),
                ).fetchone()
                recent_raids = raid_check[0] if raid_check else 0

            if recent_raids > 2:
                log.info(
                    "Skipping recruitment message to %s (Anti-Spam: %d raids in last 24 hours)",
                    to_broadcaster_login,
                    recent_raids,
                )
                return

            # 3. Bestimme die Anzahl der bisherigen Netzwerk-Raids für diesen Streamer
            total_raids = self._get_received_network_raid_count(target_id)

            # 4. Nachricht vorbereiten (mit Stats Teaser)
            followers_total = await self._resolve_recruitment_followers_total(
                login=to_broadcaster_login,
                target_id=target_id,
                target_stream_data=target_stream_data,
            )
            use_direct_invite = (
                followers_total is not None
                and followers_total <= RECRUIT_DIRECT_INVITE_MAX_FOLLOWERS
            )
            discord_invite = (
                RECRUIT_DISCORD_INVITE_DIRECT if use_direct_invite else RECRUIT_DISCORD_INVITE
            )

            stats_teaser = ""
            try:
                with get_conn() as conn:
                    stats = conn.execute(
                        """
                        SELECT
                            ROUND(AVG(viewer_count)) as avg_viewers,
                            MAX(viewer_count) as peak_viewers
                        FROM twitch_stats_category
                        WHERE streamer = ?
                          AND viewer_count > 0
                        """,
                        (to_broadcaster_login.lower(),),
                    ).fetchone()

                if stats and stats[0]:
                    avg_viewers = int(stats[0])
                    peak_viewers = int(stats[1]) if stats[1] else 0
                    if peak_viewers > 0:
                        stats_teaser = f"Übrigens: Du hattest im Schnitt {avg_viewers} Viewer bei Deadlock, dein Peak war {peak_viewers}. "
            except Exception:
                log.debug("Could not fetch stats for %s", to_broadcaster_login, exc_info=True)

            # Nachrichtenauswahl basierend auf Raid-Anzahl
            if total_raids <= 1:
                message = (
                    f"Hey @{to_broadcaster_login}! Ich bin der Bot der deutschen Deadlock Community . "
                    f"Ich manage hier die Raids bei Twitch Deadlock.. "
                    f"Du wurdest gerade von @{from_broadcaster_login} geraidet, einem unserer Partner! <3 "
                    f"Falls du bock hast kannst auch Teil der Community werden und Support erhalten – "
                    f"schau gerne mal auf unserem Discord vorbei: {discord_invite} "
                    f"Dir noch einen wunderschönen Stream <3"
                )
            elif total_raids == 2:
                message = (
                    f"Hey @{to_broadcaster_login}! Na, schon der 2. Raid von uns! ❤️ "
                    f"@{from_broadcaster_login} bringt dir gerade Verstärkung aus dem Netzwerk vorbei. "
                    f"{stats_teaser}"
                    f"Unser Partner-Netzwerk wächst ständig und wir würden freuen uns über ein neues Gesicht freuen :). "
                    f"Schau mal rein: {discord_invite} 🎮"
                )
            elif total_raids == 3:
                message = (
                    f"Hey @{to_broadcaster_login}! Aller guten Dinge sind 3! Das ist schon der 3. Raid aus der Community für dich. ❤️ "
                    f"Hast du schon über eine Partnerschaft nachgedacht? Gemeinsam wachsen wir viel schneller! "
                    f"Join uns: {discord_invite} 🎮"
                )
            else:  # 4. Raid und mehr
                message = (
                    f"Hey @{to_broadcaster_login}! So langsam wird es Zeit für eine Partnerschaft, oder? 😉 "
                    f"Das ist schon der {total_raids}. Raid von uns (diesmal von @{from_broadcaster_login})! "
                    f"{stats_teaser}"
                    f"Komm in unser Netzwerk und profitiere von gegenseitigen Raids, Zugang zu der größten deutschen Deadlock Community und viel mehr. Schau doch gerne mal vorbei: {discord_invite} 🎮"
                )

            # 5. Sende Nachricht via Bot
            # TwitchIO 3.x: Nutze _send_chat_message helper (MockChannel)
            # Diese Methode existiert im chat_bot und funktioniert mit EventSub
            try:
                if hasattr(self.chat_bot, "_send_chat_message"):
                    success = await self.chat_bot._send_chat_message(
                        target_channel,
                        message,
                        source="recruitment",
                    )

                    if success:
                        log.info(
                            "Sent recruitment message in %s's chat (raided by %s)",
                            to_broadcaster_login,
                            from_broadcaster_login,
                        )
                    else:
                        log.warning(
                            "Failed to send recruitment message to %s (returned False)",
                            to_broadcaster_login,
                        )
                else:
                    log.debug(
                        "Chat bot does not have _send_chat_message method, skipping recruitment message to %s",
                        to_broadcaster_login,
                    )
            except Exception:
                log.exception(
                    "Failed to send recruitment message to %s (raided by %s)",
                    to_broadcaster_login,
                    from_broadcaster_login,
                )

        except Exception:
            log.exception(
                "Failed to send recruitment message to %s (raided by %s)",
                to_broadcaster_login,
                from_broadcaster_login,
            )

    @staticmethod
    def _make_chat_target(login: str, user_id: str):
        class _MockChannel:
            __slots__ = ("name", "id")

            def __init__(self, name: str, channel_id: str) -> None:
                self.name = name
                self.id = channel_id

        return _MockChannel(login, user_id)

    def _lookup_outbound_chat_suppression(
        self,
        target_login: str,
        target_id: str | None,
        *,
        source: str,
    ) -> dict | None:
        chat_bot = self.chat_bot
        if not chat_bot or not hasattr(chat_bot, "_get_outbound_chat_suppression"):
            return None

        resolved_target_id = str(target_id or "").strip()
        if not resolved_target_id:
            return None

        try:
            return chat_bot._get_outbound_chat_suppression(
                self._make_chat_target(target_login, resolved_target_id),
                source,
            )
        except Exception:
            log.debug(
                "Could not load outbound chat suppression for %s (source=%s)",
                target_login,
                source,
                exc_info=True,
            )
            return None

    def _get_recent_raid_targets(self, from_broadcaster_id: str, days: int) -> set[str]:
        if not from_broadcaster_id or days <= 0:
            return set()
        cutoff = f"-{int(days)} days"
        try:
            with get_conn() as conn:
                rows = conn.execute(
                    """
                    SELECT DISTINCT to_broadcaster_id
                    FROM twitch_raid_history
                    WHERE from_broadcaster_id = ?
                      AND COALESCE(success, FALSE) IS TRUE
                      AND executed_at >= datetime('now', ?)
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

        # 1. Candidates that still need a follower count
        needs_total = [
            c for c in candidates if c.get("followers_total") is None
        ]
        if not needs_total:
            return

        # 2. Bulk-query PG stream_sessions cache for known logins
        logins_needed = [
            (c.get("user_login") or "").lower() for c in needs_total
            if (c.get("user_login") or "").strip()
        ]
        if logins_needed:
            try:
                _ph = ",".join("?" * len(logins_needed))
                with get_conn() as conn:
                    _db_rows = conn.execute(
                        f"""
                        SELECT streamer_login, COALESCE(followers_end, followers_start) AS follower_total
                          FROM twitch_stream_sessions
                         WHERE streamer_login IN ({_ph})
                           AND COALESCE(followers_end, followers_start) IS NOT NULL
                         ORDER BY COALESCE(ended_at, started_at) DESC
                        """,
                        logins_needed,
                    ).fetchall()
                # Keep only most recent hit per login
                _db_map: dict[str, int] = {}
                for _r in _db_rows:
                    _login = str(_r[0]).lower()
                    if _login not in _db_map and _r[1] is not None:
                        _db_map[_login] = int(_r[1])
                # Write DB values into candidates
                for c in needs_total:
                    _clogin = (c.get("user_login") or "").lower()
                    if _clogin in _db_map:
                        c["followers_total"] = _db_map[_clogin]
            except Exception:
                log.debug("followers_totals: DB cache query failed", exc_info=True)

        # 3. Parallel API fallback for remaining candidates (no DB hit)
        api_needed = [
            c for c in needs_total if c.get("followers_total") is None
            and str(c.get("user_id") or "").strip()
        ]
        if not api_needed:
            return

        api = TwitchAPI(
            self.auth_manager.client_id,
            self.auth_manager.client_secret,
            session=self.session,
        )
        bot_token, _bot_id, bot_scopes = await self._resolve_bot_oauth_context()
        bot_can_read_followers = bool(
            bot_token and (not bot_scopes or "moderator:read:followers" in bot_scopes)
        )

        async def _fetch_one(candidate: dict) -> None:
            user_id = str(candidate.get("user_id") or "").strip()
            if not user_id:
                return

            # Prefer bot token; fall back to broadcaster token for edge cases.
            followers = None
            if bot_can_read_followers and bot_token:
                try:
                    followers = await api.get_followers_total(user_id, user_token=bot_token)
                except Exception:
                    followers = None

            if followers is None:
                try:
                    token = await self.auth_manager.get_valid_token(user_id, self.session)
                except Exception:
                    token = None
                if token:
                    self._warn_user_scope_fallback_once(
                        area="raid candidate follower lookup",
                        subject=str(candidate.get("user_login") or user_id),
                    )
                    try:
                        followers = await api.get_followers_total(user_id, user_token=token)
                    except Exception:
                        followers = None

            if followers is not None:
                candidate["followers_total"] = int(followers)

        await asyncio.gather(*(_fetch_one(c) for c in api_needed), return_exceptions=True)

    def _load_prepared_partner_scores(
        self,
        twitch_user_ids: list[str],
    ) -> dict[str, dict[str, object]]:
        requested = [str(user_id or "").strip() for user_id in twitch_user_ids if str(user_id or "").strip()]
        if not requested:
            return {}

        if callable(load_partner_raid_score_map):
            try:
                return load_partner_raid_score_map(requested)
            except Exception:
                log.debug("Prepared partner score helper failed", exc_info=True)

        sql = (
            "SELECT twitch_user_id, twitch_login, is_live, final_score, today_received_raids, "
            "duration_score, time_pattern_score, base_score, new_partner_multiplier, "
            "raid_boost_multiplier, last_computed_at "
            "FROM twitch_partner_raid_scores "
            f"WHERE twitch_user_id IN ({','.join('?' for _ in requested)})"
        )
        try:
            with get_conn() as conn:
                rows = conn.execute(sql, tuple(requested)).fetchall()
        except Exception:
            log.debug("Prepared partner score DB query failed", exc_info=True)
            return {}

        def _safe_int(value: object, default: int = 0) -> int:
            try:
                if value is None:
                    return default
                return int(value)
            except (TypeError, ValueError):
                return default

        def _safe_float(value: object, default: float = 0.0) -> float:
            try:
                if value is None:
                    return default
                return float(value)
            except (TypeError, ValueError):
                return default

        out: dict[str, dict[str, object]] = {}
        for row in rows:
            twitch_user_id = str(row["twitch_user_id"] if hasattr(row, "keys") else row[0] or "").strip()
            if not twitch_user_id:
                continue
            out[twitch_user_id] = {
                "twitch_user_id": twitch_user_id,
                "twitch_login": str(
                    row["twitch_login"] if hasattr(row, "keys") else row[1] or ""
                ).strip().lower(),
                "is_live": bool(_safe_int(row["is_live"] if hasattr(row, "keys") else row[2], 0)),
                "final_score": _safe_float(
                    row["final_score"] if hasattr(row, "keys") else row[3],
                    0.0,
                ),
                "today_received_raids": _safe_int(
                    row["today_received_raids"] if hasattr(row, "keys") else row[4],
                    0,
                ),
                "duration_score": _safe_float(
                    row["duration_score"] if hasattr(row, "keys") else row[5],
                    0.5,
                ),
                "time_pattern_score": _safe_float(
                    row["time_pattern_score"] if hasattr(row, "keys") else row[6],
                    0.5,
                ),
                "base_score": _safe_float(
                    row["base_score"] if hasattr(row, "keys") else row[7],
                    0.5,
                ),
                "new_partner_multiplier": _safe_float(
                    row["new_partner_multiplier"] if hasattr(row, "keys") else row[8],
                    1.0,
                ),
                "raid_boost_multiplier": _safe_float(
                    row["raid_boost_multiplier"] if hasattr(row, "keys") else row[9],
                    1.0,
                ),
                "last_computed_at": row["last_computed_at"] if hasattr(row, "keys") else row[10],
            }
        return out

    async def _refresh_partner_score_cache_if_available(
        self,
        twitch_user_id: str,
        *,
        reason: str,
    ) -> None:
        twitch_user_key = str(twitch_user_id or "").strip()
        if not twitch_user_key or not callable(refresh_partner_raid_score_async):
            return
        try:
            await refresh_partner_raid_score_async(twitch_user_key)
            log.info(
                "Prepared partner raid score cache refreshed for %s (%s)",
                twitch_user_key,
                reason,
            )
        except Exception:
            log.debug(
                "Prepared partner raid score cache refresh failed for %s (%s)",
                twitch_user_key,
                reason,
                exc_info=True,
            )

    async def _select_partner_candidate_by_score(
        self,
        candidates: list[dict],
        from_broadcaster_id: str,
    ) -> dict | None:
        """
        Wählt unter Partnern nur noch vorberechnete Cache-Scores aus.

        Primär: höchster final_score.
        Bei engem Score (<= 0.05): weniger today_received_raids.
        Danach: bestehender deterministischer Fallback viewer_count/followers_total/started_at.

        Wichtig: Der 7-Tage-Target-Cooldown gilt hier bewusst NICHT.
        Partner-Raids sollen innerhalb des Partner-Pools rein nach Score priorisiert werden.
        Der Cooldown bleibt nur für den Non-Partner-Fallback aktiv.
        """
        if not candidates:
            return None

        pool = list(candidates)

        score_map = self._load_prepared_partner_scores(
            [str(candidate.get("user_id") or "").strip() for candidate in pool]
        )

        scored_candidates: list[dict] = []
        cache_misses = 0
        stale_not_live = 0
        for candidate in pool:
            twitch_user_id = str(candidate.get("user_id") or "").strip()
            candidate_login = str(candidate.get("user_login") or "").strip().lower()
            if not twitch_user_id:
                cache_misses += 1
                log.info(
                    "Prepared partner score skipped for %s: missing twitch_user_id",
                    candidate_login or "<unknown>",
                )
                continue
            score_row = score_map.get(twitch_user_id)
            if not score_row:
                cache_misses += 1
                log.info(
                    "Prepared partner score cache miss for %s (%s)",
                    candidate_login or twitch_user_id,
                    twitch_user_id,
                )
                continue
            if not bool(score_row.get("is_live")):
                stale_not_live += 1
                log.info(
                    "Prepared partner score ignored for %s (%s): cache row is not live",
                    candidate_login or twitch_user_id,
                    twitch_user_id,
                )
                continue
            enriched = dict(candidate)
            enriched["_partner_score"] = score_row
            scored_candidates.append(enriched)

        if not scored_candidates:
            log.info(
                "No prepared partner score candidate available for broadcaster_id=%s "
                "(input=%d, cache_misses=%d, stale_not_live=%d)",
                from_broadcaster_id,
                len(candidates),
                cache_misses,
                stale_not_live,
            )
            return None

        def _safe_int(value: object, default: int) -> int:
            try:
                if value is None:
                    return default
                return int(value)
            except (TypeError, ValueError):
                return default

        def _safe_float(value: object, default: float = 0.0) -> float:
            try:
                if value is None:
                    return default
                return float(value)
            except (TypeError, ValueError):
                return default

        def _score(candidate: dict) -> float:
            score_row = candidate.get("_partner_score") or {}
            return _safe_float(score_row.get("final_score"), 0.0)

        def _today_received(candidate: dict) -> int:
            score_row = candidate.get("_partner_score") or {}
            return _safe_int(score_row.get("today_received_raids"), 10**9)

        def _fallback_sort_key(candidate: dict) -> tuple[int, int, str]:
            viewers = _safe_int(candidate.get("viewer_count"), 10**9)
            followers = _safe_int(candidate.get("followers_total"), 10**9)
            started_at = candidate.get("started_at") or "9999-99-99"
            return (viewers, followers, started_at)

        best_final_score = max(_score(candidate) for candidate in scored_candidates)
        close_candidates = [
            candidate
            for candidate in scored_candidates
            if abs(best_final_score - _score(candidate)) <= 0.05
        ]

        selection_reason = "highest_final_score"
        selected: dict
        if len(close_candidates) == 1:
            selected = close_candidates[0]
        else:
            lowest_today_received = min(_today_received(candidate) for candidate in close_candidates)
            tie_candidates = [
                candidate
                for candidate in close_candidates
                if _today_received(candidate) == lowest_today_received
            ]
            if len(tie_candidates) == 1:
                selection_reason = "today_received_raids"
                selected = tie_candidates[0]
            else:
                await self._attach_followers_totals(tie_candidates)
                tie_candidates.sort(key=_fallback_sort_key)
                selection_reason = "viewer_count_followers_started_at"
                selected = tie_candidates[0]

        selected_score = selected.get("_partner_score") or {}
        log.info(
            "Partner raid target selection (prepared score): %s final=%.3f today=%s "
            "reason=%s cache_misses=%d stale_not_live=%d from %d candidates",
            selected.get("user_login"),
            _safe_float(selected_score.get("final_score"), 0.0),
            _safe_int(selected_score.get("today_received_raids"), 0),
            selection_reason,
            cache_misses,
            stale_not_live,
            len(candidates),
        )

        return selected

    async def _select_fairest_candidate(
        self, candidates: list[dict], from_broadcaster_id: str
    ) -> dict | None:
        """
        Wählt den Raid-Kandidaten mit den wenigsten Viewern.
        Bei Gleichstand: Wenigste Follower (wenn verfügbar), danach kürzeste Stream-Zeit.
        Ziele der letzten Tage werden vermieden, sofern Alternativen existieren.
        """
        if not candidates:
            return None

        recent_targets = self._get_recent_raid_targets(
            from_broadcaster_id, RAID_TARGET_COOLDOWN_DAYS
        )
        if recent_targets:
            filtered = [c for c in candidates if str(c.get("user_id") or "") not in recent_targets]
        else:
            filtered = []

        pool = filtered or candidates

        await self._attach_followers_totals(pool)

        def _safe_int(value: object, default: int) -> int:
            try:
                if value is None:
                    return default
                return int(value)
            except (TypeError, ValueError):
                return default

        def _sort_key(candidate: dict) -> tuple[int, int, str]:
            viewers = _safe_int(candidate.get("viewer_count"), 10**9)
            followers = _safe_int(candidate.get("followers_total"), 10**9)
            started_at = candidate.get("started_at") or "9999-99-99"
            return (viewers, followers, started_at)

        pool.sort(key=_sort_key)

        selected = pool[0]
        log.info(
            "Raid target selection (min viewers): %s (viewers=%s, followers=%s, recent_filtered=%d) from %d candidates",
            selected.get("user_login"),
            selected.get("viewer_count"),
            selected.get("followers_total"),
            max(0, len(candidates) - len(pool)),
            len(candidates),
        )

        return selected

    def _is_blacklisted(self, target_id: str, target_login: str) -> bool:
        """Prüft, ob ein Ziel auf der Blacklist steht."""
        try:
            with get_conn() as conn:
                row = conn.execute(
                    """
                    SELECT 1 FROM twitch_raid_blacklist
                    WHERE (target_id IS NOT NULL AND target_id = ?)
                       OR lower(target_login) = lower(?)
                    """,
                    (target_id, target_login),
                ).fetchone()
                return bool(row)
        except Exception:
            log.error("Error checking blacklist", exc_info=True)
            return False

    def _add_to_blacklist(self, target_id: str, target_login: str, reason: str):
        """Fügt ein Ziel zur Blacklist hinzu."""
        try:
            with get_conn() as conn:
                conn.execute(
                    """
                    INSERT INTO twitch_raid_blacklist (target_id, target_login, reason)
                    VALUES (?, ?, ?)
                    ON CONFLICT (target_login) DO UPDATE SET
                        target_id = EXCLUDED.target_id,
                        reason = EXCLUDED.reason
                    """,
                    (target_id, target_login, reason),
                )
                # autocommit – no explicit commit needed
            log.info(
                "Added %s (ID: %s) to raid blacklist. Reason: %s",
                target_login,
                target_id,
                reason,
            )
        except Exception:
            log.error("Error adding to blacklist", exc_info=True)

    def _is_retryable_raid_error(self, error: str | None) -> bool:
        """Return True for raid target errors where we should try another target."""
        if not error:
            return False
        msg = error.lower()
        retryable_markers = (
            "cannot be raided",
            "does not allow you to raid",
            "do not allow you to raid",
            "not allow you to raid",
            "settings do not allow you to raid",
            "not accepting raids",
            "does not allow raids",
            "raids are disabled",
        )
        return any(marker in msg for marker in retryable_markers)

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
        flow_start_ts = offline_trigger_ts if offline_trigger_ts is not None else time.monotonic()
        offline_trigger_ts = flow_start_ts
        active_session = self.session
        if active_session is None:
            log.warning(
                "Raid pipeline unavailable for %s: no active HTTP session",
                broadcaster_login,
            )
            return {"status": "unavailable", "error": "no_active_session"}

        max_attempts = 3
        exclude_ids = {broadcaster_id}
        cached_de_streams = None

        blacklisted_ids: set[str] = set()
        blacklisted_logins: set[str] = set()
        try:
            with get_conn() as conn:
                for bl_row in conn.execute(
                    "SELECT target_id, lower(target_login) FROM twitch_raid_blacklist"
                ).fetchall():
                    if bl_row[0]:
                        blacklisted_ids.add(str(bl_row[0]))
                    blacklisted_logins.add(str(bl_row[1]))
        except Exception:
            log.error("Error loading blacklist", exc_info=True)

        for attempt in range(max_attempts):
            attempt_start_ts = time.monotonic()
            target = None
            is_partner_raid = False
            candidates_count = 0

            partner_candidates = [
                stream_data
                for stream_data in online_partners
                if stream_data.get("user_id") not in exclude_ids
                and bool(stream_data.get("raid_enabled", True))
                and str(stream_data.get("user_id") or "") not in blacklisted_ids
                and (stream_data.get("user_login") or "").lower() not in blacklisted_logins
            ]

            if partner_candidates:
                is_partner_raid = True
                target = await self._select_partner_candidate_by_score(
                    partner_candidates,
                    broadcaster_id,
                )
                candidates_count = len(partner_candidates)

            if not target and api and category_id:
                if cached_de_streams is None:
                    try:
                        log.info(
                            "No partners online for %s, fetching Deadlock-DE fallback",
                            broadcaster_login,
                        )
                        cached_de_streams = await api.get_streams_by_category(
                            category_id, language="de", limit=50
                        )
                    except Exception:
                        log.exception("Failed to get Deadlock-DE streams for fallback raid")
                        cached_de_streams = []

                fallback_candidates = [
                    stream_data
                    for stream_data in cached_de_streams
                    if stream_data.get("user_id") not in exclude_ids
                    and str(stream_data.get("user_id") or "") not in blacklisted_ids
                    and (stream_data.get("user_login") or "").lower() not in blacklisted_logins
                ]

                if fallback_candidates:
                    target = await self._select_fairest_candidate(
                        fallback_candidates,
                        broadcaster_id,
                    )
                    candidates_count = len(fallback_candidates)

            if not target:
                log.info(
                    "No valid raid target found for %s (Attempt %d/%d, total_elapsed=%.0fms, reason=%s)",
                    broadcaster_login,
                    attempt + 1,
                    max_attempts,
                    (time.monotonic() - flow_start_ts) * 1000.0,
                    reason,
                )
                return {"status": "no_target"}

            target_id = str(target.get("user_id") or "").strip()
            target_login = str(target.get("user_login") or "").strip().lower()
            target_started_at = target.get("started_at", "")

            selection_ms = (time.monotonic() - attempt_start_ts) * 1000.0
            raid_flow_id = self._next_raid_observability_flow_id(prefix="raid")
            self._increment_raid_observability_counter("raid_flow_started_total")
            self._log_raid_observability_event(
                raid_flow_id=raid_flow_id,
                step="attempt_selected",
                decision="candidate_selected",
                from_broadcaster_login=broadcaster_login,
                from_broadcaster_id=broadcaster_id,
                to_broadcaster_login=target_login,
                to_broadcaster_id=target_id,
                details={
                    "attempt": attempt + 1,
                    "max_attempts": max_attempts,
                    "selection_ms": int(selection_ms),
                    "candidates_count": candidates_count,
                    "reason": reason,
                },
            )
            log.info(
                "Executing raid attempt %d/%d: %s -> %s (selection %.0fms, candidates=%d, reason=%s)",
                attempt + 1,
                max_attempts,
                broadcaster_login,
                target_login,
                selection_ms,
                candidates_count,
                reason,
            )

            channel_raid_ready = await self._ensure_raid_arrival_subscription_ready(
                to_broadcaster_id=target_id,
                to_broadcaster_login=target_login,
                raid_flow_id=raid_flow_id,
            )

            api_call_start = time.monotonic()
            success, error = await self.raid_executor.start_raid(
                from_broadcaster_id=broadcaster_id,
                from_broadcaster_login=broadcaster_login,
                to_broadcaster_id=target_id,
                to_broadcaster_login=target_login,
                viewer_count=viewer_count,
                stream_duration_sec=stream_duration_sec,
                target_stream_started_at=target_started_at,
                candidates_count=candidates_count,
                session=active_session,
                reason=reason,
            )
            api_call_ms = (time.monotonic() - api_call_start) * 1000.0
            total_ms = (time.monotonic() - flow_start_ts) * 1000.0

            if success:
                await self._register_pending_raid(
                    from_broadcaster_login=broadcaster_login,
                    to_broadcaster_id=target_id,
                    to_broadcaster_login=target_login,
                    target_stream_data=target,
                    is_partner_raid=is_partner_raid,
                    viewer_count=viewer_count,
                    offline_trigger_ts=offline_trigger_ts,
                    raid_flow_id=raid_flow_id,
                    channel_raid_ready=channel_raid_ready,
                )
                if set_manual_suppression:
                    self.mark_manual_raid_started(
                        broadcaster_id=str(broadcaster_id),
                        ttl_seconds=180.0,
                    )
                log.info(
                    "Raid attempt %d/%d succeeded (%s -> %s) api=%.0fms, total_elapsed=%.0fms, reason=%s",
                    attempt + 1,
                    max_attempts,
                    broadcaster_login,
                    target_login,
                    api_call_ms,
                    total_ms,
                    reason,
                )
                return {
                    "status": "started",
                    "target_login": target_login,
                    "target": target,
                    "is_partner_raid": is_partner_raid,
                    "viewer_count": viewer_count,
                }

            exclude_ids.add(target_id)

            if self._is_retryable_raid_error(error):
                if is_partner_raid:
                    log.warning(
                        "Raid failed: Partner target %s does not allow raids. Skipping without blacklist.",
                        target_login,
                    )
                else:
                    log.warning(
                        "Raid failed: Target %s does not allow raids. Blacklisting and retrying.",
                        target_login,
                    )
                    self._add_to_blacklist(target_id, target_login, error)
                continue

            log.error(
                "Raid failed with non-retriable error after %.0fms (api=%.0fms, attempt=%d/%d, reason=%s): %s",
                total_ms,
                api_call_ms,
                attempt + 1,
                max_attempts,
                reason,
                error,
            )
            return {"status": "raid_failed", "error": error or "unknown_error"}

        return {"status": "raid_failed", "error": "no_valid_target_after_retries"}

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
        with get_conn() as conn:
            _s_row = load_active_partner(conn, twitch_user_id=broadcaster_id)
        with get_conn() as conn:
            _a_row = conn.execute(
                "SELECT raid_enabled FROM twitch_raid_auth WHERE twitch_user_id = ?",
                (broadcaster_id,),
            ).fetchone()
        row = (
            ((_s_row["raid_bot_enabled"] if hasattr(_s_row, "keys") else _s_row[13]) if _s_row else None),
            (_a_row[0] if _a_row else None),
        ) if (_s_row is not None or _a_row is not None) else None

        if not row:
            log.debug("Streamer %s not found in DB", broadcaster_login)
            return None

        raid_bot_enabled, raid_auth_enabled = row
        if not raid_bot_enabled:
            log.debug("Raid bot disabled for %s (setting)", broadcaster_login)
            return None
        if not raid_auth_enabled:
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

        return None
