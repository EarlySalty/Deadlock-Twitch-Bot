import asyncio
import inspect
import json
import logging
import time
from datetime import UTC, datetime, timedelta

from ..core.partner_utils import is_partner_channel_for_chat_tracking
from ..storage import get_conn, insert_observability_event
from .constants import CHAT_JOIN_OFFLINE, eventsub

log = logging.getLogger("TwitchStreams.ChatBot")

_CHAT_EVENTSUB_TYPES: tuple[str, ...] = (
    "channel.chat.message",
    "channel.chat.notification",
)
_CHAT_EVENTSUB_CLASS_NAMES: frozenset[str] = frozenset(
    {
        "ChannelChatMessage",
        "ChannelChatNotification",
    }
)
_CHAT_REQUIRED_BOT_SCOPES: frozenset[str] = frozenset({"user:read:chat"})
_CHAT_REQUIRED_BROADCASTER_GRANTS: frozenset[str] = frozenset({"channel:bot"})


class ConnectionMixin:
    def _next_chat_observability_flow_id(self, *, prefix: str) -> str:
        sequence = int(getattr(self, "_chat_observability_sequence", 0) or 0) + 1
        self._chat_observability_sequence = sequence
        return f"{str(prefix or 'chat').strip().lower()}-{int(time.time() * 1000)}-{sequence}"

    def _chat_observability_counters(self) -> dict[str, int]:
        counters = getattr(self, "_chat_observability_counter_store", None)
        if not isinstance(counters, dict):
            counters = {}
            self._chat_observability_counter_store = counters
        return counters

    def _increment_chat_observability_counter(self, name: str, amount: int = 1) -> int:
        counter_name = str(name or "").strip()
        if not counter_name:
            return 0
        counters = self._chat_observability_counters()
        counters[counter_name] = int(counters.get(counter_name, 0) or 0) + int(amount)
        return counters[counter_name]

    @staticmethod
    def _chat_observability_normalize(value: object, *, limit: int = 240) -> str:
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

    def _format_chat_observability_fields(self, **fields: object) -> str:
        ordered = []
        for key in sorted(fields):
            value = fields[key]
            if value is None:
                continue
            ordered.append(
                f"{str(key).strip()}={self._chat_observability_normalize(value)}"
            )
        return " ".join(ordered)

    def _log_chat_join_decision(
        self,
        *,
        flow_id: str,
        channel_login: str,
        channel_id: str | None,
        remaining_missing: list[str],
        decision: str,
        decision_detail: str | None = None,
        level: int = logging.INFO,
        auth_diagnostics: dict[str, object] | None = None,
        join_state: dict[str, bool] | None = None,
        exception: Exception | None = None,
    ) -> None:
        normalized_login = str(channel_login or "").strip().lower().lstrip("#")
        cooldown_store = getattr(self, "_mod_retry_cooldown", None)
        cooldown_until = None
        if isinstance(cooldown_store, dict):
            cooldown_until = cooldown_store.get(normalized_login)
        payload = {
            "flow_id": flow_id,
            "channel_login": normalized_login or str(channel_login or "").strip(),
            "channel_id": str(channel_id or "").strip() or None,
            "remaining_missing": sorted(str(item) for item in remaining_missing if str(item).strip()),
            "tracked_subscription_types": sorted(self._get_tracked_chat_subscription_types(normalized_login)),
            "subscription_state": self.get_channel_subscription_state(normalized_login),
            "auth_diagnostics": auth_diagnostics or {},
            "join_state": join_state or {},
            "decision": str(decision or "").strip() or "unknown",
            "decision_detail": str(decision_detail or "").strip() or None,
            "mod_retry_cooldown_until": (
                cooldown_until.isoformat()
                if isinstance(cooldown_until, datetime)
                else str(cooldown_until or "").strip() or None
            ),
            "exception_class": exception.__class__.__name__ if exception is not None else None,
            "exception_text": (
                str(exception or "").replace("\r", " ").replace("\n", " ").strip()[:240]
                if exception is not None
                else None
            ),
        }
        self._last_chat_join_diagnostic = payload
        log.log(
            level,
            "join_decision %s",
            self._format_chat_observability_fields(**payload),
        )
        insert_observability_event(
            flow_type="chat_join",
            flow_id=flow_id,
            entity_login=payload.get("channel_login"),
            entity_id=payload.get("channel_id"),
            step="terminal_decision",
            decision=str(payload.get("decision") or "unknown"),
            details=payload,
        )

    @staticmethod
    def _required_chat_subscription_types() -> tuple[str, ...]:
        return _CHAT_EVENTSUB_TYPES

    @staticmethod
    def _is_chat_eventsub_subscription_type(sub_type: object) -> bool:
        sub_type_value = str(sub_type or "").strip()
        if not sub_type_value:
            return False
        if sub_type_value in _CHAT_EVENTSUB_TYPES:
            return True
        return any(class_name in sub_type_value for class_name in _CHAT_EVENTSUB_CLASS_NAMES)

    def _get_tracked_chat_subscription_types(self, channel_login: str) -> set[str]:
        normalized_login = str(channel_login or "").strip().lower().lstrip("#")
        tracked = getattr(self, "_channel_subscription_types", None)
        if not isinstance(tracked, dict):
            tracked = {}
            self._channel_subscription_types = tracked
        entry = tracked.get(normalized_login)
        if isinstance(entry, set):
            return set(entry)
        return set()

    def _record_chat_subscription_state(
        self,
        channel_login: str,
        sub_type: str,
        state: str,
        *,
        detail: str | None = None,
    ) -> None:
        normalized_login = str(channel_login or "").strip().lower().lstrip("#")
        if not normalized_login or not sub_type:
            return
        snapshot = getattr(self, "_channel_subscription_state", None)
        if not isinstance(snapshot, dict):
            snapshot = {}
            self._channel_subscription_state = snapshot
        channel_snapshot = snapshot.get(normalized_login)
        if not isinstance(channel_snapshot, dict):
            channel_snapshot = {}
            snapshot[normalized_login] = channel_snapshot
        payload = {"state": str(state or "").strip()}
        detail_text = str(detail or "").strip()
        if detail_text:
            payload["detail"] = detail_text
        channel_snapshot[str(sub_type)] = payload

    def _track_chat_subscription(
        self,
        channel_login: str,
        *,
        channel_id: str,
        sub_type: str,
    ) -> None:
        normalized_login = str(channel_login or "").strip().lower().lstrip("#")
        if not normalized_login or not sub_type:
            return
        tracked = getattr(self, "_channel_subscription_types", None)
        if not isinstance(tracked, dict):
            tracked = {}
            self._channel_subscription_types = tracked
        sub_types = tracked.get(normalized_login)
        if not isinstance(sub_types, set):
            sub_types = set()
            tracked[normalized_login] = sub_types
        sub_types.add(str(sub_type))
        self._monitored_streamers.add(normalized_login)
        self._channel_ids[normalized_login] = str(channel_id)
        self._record_chat_subscription_state(normalized_login, sub_type, "ok")

    def _untrack_chat_subscription(self, channel_login: str, *, sub_type: str | None = None) -> None:
        normalized_login = str(channel_login or "").strip().lower().lstrip("#")
        if not normalized_login:
            return
        tracked = getattr(self, "_channel_subscription_types", None)
        if isinstance(tracked, dict):
            if sub_type:
                entry = tracked.get(normalized_login)
                if isinstance(entry, set):
                    entry.discard(str(sub_type))
                    if not entry:
                        tracked.pop(normalized_login, None)
            else:
                tracked.pop(normalized_login, None)
        snapshot = getattr(self, "_channel_subscription_state", None)
        if isinstance(snapshot, dict):
            if sub_type:
                channel_snapshot = snapshot.get(normalized_login)
                if isinstance(channel_snapshot, dict):
                    channel_snapshot.pop(str(sub_type), None)
                    if not channel_snapshot:
                        snapshot.pop(normalized_login, None)
            else:
                snapshot.pop(normalized_login, None)

    def is_channel_subscription_ready(self, channel_login: str, sub_type: str | None = None) -> bool:
        normalized_login = str(channel_login or "").strip().lower().lstrip("#")
        tracked_types = self._get_tracked_chat_subscription_types(normalized_login)
        if sub_type:
            return str(sub_type) in tracked_types
        return set(self._required_chat_subscription_types()).issubset(tracked_types)

    def get_channel_subscription_state(self, channel_login: str) -> dict[str, dict[str, str]]:
        normalized_login = str(channel_login or "").strip().lower().lstrip("#")
        snapshot = getattr(self, "_channel_subscription_state", None)
        if not isinstance(snapshot, dict):
            return {}
        channel_snapshot = snapshot.get(normalized_login)
        if not isinstance(channel_snapshot, dict):
            return {}
        return {
            str(sub_type): {
                str(key): str(value)
                for key, value in payload.items()
                if isinstance(payload, dict)
            }
            for sub_type, payload in channel_snapshot.items()
            if isinstance(payload, dict)
        }

    @staticmethod
    def _normalize_chat_subscription_type(sub_type: object) -> str:
        sub_type_value = str(getattr(sub_type, "value", None) or sub_type or "").strip()
        if sub_type_value in _CHAT_EVENTSUB_TYPES:
            return sub_type_value
        lowered = sub_type_value.lower()
        if "channelchatmessage" in lowered or "channel.chat.message" in lowered:
            return "channel.chat.message"
        if "channelchatnotification" in lowered or "channel.chat.notification" in lowered:
            return "channel.chat.notification"
        return sub_type_value

    @staticmethod
    def _extract_chat_subscription_broadcaster_id(subscription: object) -> str:
        condition = getattr(subscription, "condition", None)
        if isinstance(condition, dict):
            return str(
                condition.get("broadcaster_user_id")
                or condition.get("broadcaster_id")
                or ""
            ).strip()
        return str(
            getattr(condition, "broadcaster_user_id", "")
            or getattr(condition, "broadcaster_id", "")
            or ""
        ).strip()

    @staticmethod
    def _extract_chat_subscription_status(
        subscription: object,
        *,
        default: str,
    ) -> str:
        raw_status = getattr(subscription, "status", None)
        status = str(getattr(raw_status, "value", None) or raw_status or "").strip().lower()
        return status or default

    @staticmethod
    def _prefer_chat_subscription_status(current: str | None, new_value: str) -> str:
        new_status = str(new_value or "").strip().lower() or "unknown"
        current_status = str(current or "").strip().lower()
        if current_status == "enabled":
            return current_status
        if new_status == "enabled" or not current_status:
            return new_status
        return current_status

    async def _load_remote_chat_subscription_statuses(
        self,
        *,
        channel_id: str,
        auth_user_id: str | None = None,
    ) -> tuple[dict[str, str], str | None]:
        target_id = str(channel_id or "").strip()
        if not target_id:
            return {}, None
        normalized_auth_user_id = str(auth_user_id or "").strip()

        async def _collect_subscription_objects(source: object) -> list[object]:
            items: list[object] = []
            if source is None:
                return items
            if hasattr(source, "__aiter__"):
                async for item in source:
                    items.append(item)
                return items
            if hasattr(source, "__anext__"):
                while True:
                    try:
                        item = await source.__anext__()
                    except StopAsyncIteration:
                        break
                    items.append(item)
                return items

            nested = getattr(source, "subscriptions", None)
            if nested is not None:
                return await _collect_subscription_objects(nested)

            try:
                items.extend(list(source))
            except TypeError:
                log.debug(
                    "join(): unerwarteter EventSub subscriptions-Typ: %s",
                    type(source),
                )
            return items

        fetch_subs = getattr(self, "fetch_eventsub_subscriptions", None)
        if callable(fetch_subs):
            try:
                fetch_kwargs = {}
                if normalized_auth_user_id:
                    # WebSocket EventSub subscriptions are user-bound. Without the
                    # bot user context Helix lists only app-token subscriptions and
                    # falsely reports chat subscriptions as missing.
                    fetch_kwargs["token_for"] = normalized_auth_user_id
                    fetch_kwargs["user_id"] = normalized_auth_user_id
                subs_result = fetch_subs(**fetch_kwargs)
                if inspect.isawaitable(subs_result):
                    subs_result = await subs_result
                statuses: dict[str, str] = {}
                for sub in await _collect_subscription_objects(subs_result):
                    sub_type = self._normalize_chat_subscription_type(
                        getattr(sub, "type", "")
                        or getattr(sub, "subscription_type", "")
                    )
                    if not self._is_chat_eventsub_subscription_type(sub_type):
                        continue
                    broadcaster_id = self._extract_chat_subscription_broadcaster_id(sub)
                    if broadcaster_id != target_id:
                        continue
                    statuses[sub_type] = self._prefer_chat_subscription_status(
                        statuses.get(sub_type),
                        self._extract_chat_subscription_status(sub, default="unknown"),
                    )
                return statuses, "helix"
            except Exception:
                log.debug(
                    "join(): fetch_eventsub_subscriptions fehlgeschlagen für %s",
                    target_id,
                    exc_info=True,
                )

        ws_subs = getattr(self, "websocket_subscriptions", None)
        if callable(ws_subs):
            try:
                subs_map = await ws_subs()
                statuses = {}
                if isinstance(subs_map, dict):
                    for sub in subs_map.values():
                        sub_type = self._normalize_chat_subscription_type(
                            getattr(getattr(sub, "type", ""), "value", None)
                            or getattr(sub, "type", "")
                        )
                        if not self._is_chat_eventsub_subscription_type(sub_type):
                            continue
                        broadcaster_id = self._extract_chat_subscription_broadcaster_id(sub)
                        if broadcaster_id != target_id:
                            continue
                        statuses[sub_type] = self._prefer_chat_subscription_status(
                            statuses.get(sub_type),
                            self._extract_chat_subscription_status(sub, default="enabled"),
                        )
                return statuses, "websocket_registry"
            except Exception:
                log.debug(
                    "join(): websocket_subscriptions fehlgeschlagen für %s",
                    target_id,
                    exc_info=True,
                )

        return {}, None

    async def _refresh_remote_chat_subscription_tracking(
        self,
        *,
        channel_login: str,
        channel_id: str,
        auth_user_id: str | None = None,
        wait_timeout_seconds: float = 6.0,
        poll_interval_seconds: float = 0.5,
    ) -> bool:
        normalized_login = str(channel_login or "").strip().lower().lstrip("#")
        target_id = str(channel_id or "").strip()
        if not normalized_login or not target_id:
            return False

        required_types = tuple(self._required_chat_subscription_types())
        deadline = time.monotonic() + max(0.0, float(wait_timeout_seconds))
        last_source: str | None = None

        while True:
            statuses, source = await self._load_remote_chat_subscription_statuses(
                channel_id=target_id,
                auth_user_id=auth_user_id,
            )
            if source is not None:
                last_source = source
                for sub_type in required_types:
                    status = str(statuses.get(sub_type) or "").strip().lower()
                    if status == "enabled":
                        self._track_chat_subscription(
                            normalized_login,
                            channel_id=target_id,
                            sub_type=sub_type,
                        )
                        self._record_chat_subscription_state(
                            normalized_login,
                            sub_type,
                            "ok",
                            detail=f"verified via {source}",
                        )
                        continue

                    self._untrack_chat_subscription(normalized_login, sub_type=sub_type)
                    if status:
                        self._record_chat_subscription_state(
                            normalized_login,
                            sub_type,
                            "subscription_not_enabled",
                            detail=f"{source}:{status}",
                        )
                    else:
                        self._record_chat_subscription_state(
                            normalized_login,
                            sub_type,
                            "subscription_missing",
                            detail=f"{source}:missing",
                        )

                if self.is_channel_subscription_ready(normalized_login):
                    return True

            if time.monotonic() >= deadline:
                break
            await asyncio.sleep(max(0.0, float(poll_interval_seconds)))

        if last_source is None:
            for sub_type in required_types:
                self._untrack_chat_subscription(normalized_login, sub_type=sub_type)
                self._record_chat_subscription_state(
                    normalized_login,
                    sub_type,
                    "verification_unavailable",
                    detail="no remote EventSub listing available",
                )
        return False

    def _has_active_transport_restart(self) -> bool:
        restart_task = getattr(self, "_restart_task", None)
        if restart_task is not None and not restart_task.done():
            return True
        cooldown_until = float(getattr(self, "_restart_cooldown_until", 0.0) or 0.0)
        return cooldown_until > time.monotonic()

    def _build_required_chat_subscription_payloads(
        self,
        *,
        broadcaster_id: str,
        user_id: str,
    ) -> tuple[tuple[str, object], ...]:
        return (
            (
                "channel.chat.message",
                eventsub.ChatMessageSubscription(
                    broadcaster_user_id=str(broadcaster_id),
                    user_id=str(user_id),
                ),
            ),
            (
                "channel.chat.notification",
                eventsub.ChatNotificationSubscription(
                    broadcaster_user_id=str(broadcaster_id),
                    user_id=str(user_id),
                ),
            ),
        )

    async def _subscribe_missing_chat_subscriptions(
        self,
        *,
        channel_login: str,
        channel_id: str,
        safe_bot_id: str,
    ) -> bool:
        normalized_login = str(channel_login or "").strip().lower().lstrip("#")
        missing_types = [
            sub_type
            for sub_type in self._required_chat_subscription_types()
            if not self.is_channel_subscription_ready(normalized_login, sub_type)
        ]
        if not missing_types:
            self._monitored_streamers.add(normalized_login)
            self._channel_ids[normalized_login] = str(channel_id)
            return True

        for sub_type, payload in self._build_required_chat_subscription_payloads(
            broadcaster_id=str(channel_id),
            user_id=str(safe_bot_id),
        ):
            if sub_type not in missing_types:
                continue
            self._record_chat_subscription_state(
                normalized_login,
                sub_type,
                "subscribe_requested",
            )
            await self.subscribe_websocket(payload=payload)
        return await self._refresh_remote_chat_subscription_tracking(
            channel_login=normalized_login,
            channel_id=str(channel_id),
            auth_user_id=str(safe_bot_id),
        )

    def _resolve_chat_bot_scope_set(self) -> set[str]:
        token_mgr = getattr(self, "_token_manager", None)
        scopes = getattr(token_mgr, "scopes", None) if token_mgr is not None else None
        return {
            str(scope).strip().lower()
            for scope in (scopes or set())
            if str(scope).strip()
        }

    def _load_broadcaster_chat_scope_set(
        self,
        *,
        channel_id: str | None,
        channel_login: str,
    ) -> set[str]:
        target_id = str(channel_id or "").strip()
        normalized_login = str(channel_login or "").strip().lower().lstrip("#")
        try:
            with get_conn() as conn:
                row = conn.execute(
                    """
                    SELECT scopes
                    FROM twitch_raid_auth
                    WHERE (? <> '' AND twitch_user_id = ?)
                       OR (? <> '' AND LOWER(twitch_login) = ?)
                    ORDER BY authorized_at DESC
                    LIMIT 1
                    """,
                    (
                        target_id,
                        target_id,
                        normalized_login,
                        normalized_login,
                    ),
                ).fetchone()
        except Exception:
            log.debug(
                "Konnte Broadcaster-Scopes für Chat-Subscription nicht laden: %s",
                normalized_login or target_id,
                exc_info=True,
            )
            return set()
        scopes_raw = ""
        if row is not None:
            scopes_raw = str(row[0] if not hasattr(row, "keys") else row["scopes"] or "")
        return {
            scope.strip().lower()
            for scope in scopes_raw.split()
            if scope.strip()
        }

    def _diagnose_chat_subscription_authorization(
        self,
        *,
        channel_login: str,
        channel_id: str | None,
    ) -> dict[str, object]:
        bot_scopes = self._resolve_chat_bot_scope_set()
        broadcaster_scopes = self._load_broadcaster_chat_scope_set(
            channel_id=channel_id,
            channel_login=channel_login,
        )
        bot_scope_state_known = bool(bot_scopes)
        missing_bot_scopes = sorted(
            scope for scope in _CHAT_REQUIRED_BOT_SCOPES if bot_scopes and scope not in bot_scopes
        )
        missing_broadcaster_scopes = sorted(
            scope
            for scope in _CHAT_REQUIRED_BROADCASTER_GRANTS
            if broadcaster_scopes and scope not in broadcaster_scopes
        )
        return {
            "bot_scope_state_known": bot_scope_state_known,
            "missing_bot_scopes": missing_bot_scopes,
            "missing_broadcaster_scopes": missing_broadcaster_scopes,
        }

    def _purge_local_channel_state(self, channel_login: str) -> None:
        normalized_login = str(channel_login or "").strip().lower().lstrip("#")
        if not normalized_login:
            return
        monitored = getattr(self, "_monitored_streamers", None)
        if isinstance(monitored, set):
            monitored.discard(normalized_login)
        channel_ids = getattr(self, "_channel_ids", None)
        if isinstance(channel_ids, dict):
            channel_ids.pop(normalized_login, None)
        channel_subscriptions = getattr(self, "_channel_subscription_types", None)
        if isinstance(channel_subscriptions, dict):
            channel_subscriptions.pop(normalized_login, None)
        channel_subscription_state = getattr(self, "_channel_subscription_state", None)
        if isinstance(channel_subscription_state, dict):
            channel_subscription_state.pop(normalized_login, None)
        monitored_only = getattr(self, "_monitored_only_channels", None)
        if isinstance(monitored_only, set):
            monitored_only.discard(normalized_login)
        initial_channels = getattr(self, "_initial_channels", None)
        if isinstance(initial_channels, list):
            self._initial_channels = [
                channel
                for channel in initial_channels
                if str(channel or "").strip().lower().lstrip("#") != normalized_login
            ]

    def _load_chat_join_channel_state(
        self,
        *,
        channel_login: str,
        channel_id: str | None,
    ) -> dict[str, bool]:
        normalized_login = str(channel_login or "").strip().lower().lstrip("#")
        target_id = str(channel_id or "").strip()
        state = {
            "is_partner_active": False,
            "exists_in_streamers": False,
            "is_monitored_only": False,
            "has_raid_auth": False,
        }
        if not normalized_login and not target_id:
            return state
        try:
            with get_conn() as conn:
                partner_row = conn.execute(
                    """
                    SELECT is_partner_active
                    FROM twitch_streamers_partner_state
                    WHERE (? <> '' AND twitch_user_id = ?)
                       OR (? <> '' AND LOWER(twitch_login) = ?)
                    ORDER BY is_partner_active DESC
                    LIMIT 1
                    """,
                    (
                        target_id,
                        target_id,
                        normalized_login,
                        normalized_login,
                    ),
                ).fetchone()
                if partner_row is not None:
                    state["is_partner_active"] = bool(
                        partner_row[0]
                        if not hasattr(partner_row, "keys")
                        else partner_row["is_partner_active"]
                    )

                streamer_row = conn.execute(
                    """
                    SELECT COALESCE(is_monitored_only, 0) AS is_monitored_only
                    FROM twitch_streamers
                    WHERE (? <> '' AND twitch_user_id = ?)
                       OR (? <> '' AND LOWER(twitch_login) = ?)
                    LIMIT 1
                    """,
                    (
                        target_id,
                        target_id,
                        normalized_login,
                        normalized_login,
                    ),
                ).fetchone()
                if streamer_row is not None:
                    state["exists_in_streamers"] = True
                    state["is_monitored_only"] = bool(
                        streamer_row[0]
                        if not hasattr(streamer_row, "keys")
                        else streamer_row["is_monitored_only"]
                    )

                auth_row = conn.execute(
                    """
                    SELECT 1
                    FROM twitch_raid_auth
                    WHERE (? <> '' AND twitch_user_id = ?)
                       OR (? <> '' AND LOWER(twitch_login) = ?)
                    LIMIT 1
                    """,
                    (
                        target_id,
                        target_id,
                        normalized_login,
                        normalized_login,
                    ),
                ).fetchone()
                state["has_raid_auth"] = auth_row is not None
        except Exception:
            log.debug(
                "Konnte Channel-Status für Chat-Join nicht laden: %s",
                normalized_login or target_id,
                exc_info=True,
            )
        return state

    @staticmethod
    def _looks_like_transport_session_gone_error(text: str) -> bool:
        lowered = str(text or "").lower()
        return (
            "websocket transport session does not exist" in lowered
            or "session does not exist" in lowered
            or "has already disconnected" in lowered
            or "session has disconnected" in lowered
        )

    @staticmethod
    def _looks_like_bot_banned_error(status: int | None, text: str) -> bool:
        if not text:
            return False
        lowered = text.lower()
        if "user is banned" in lowered:
            return True
        if "banned" in lowered:
            return True
        if status in {400, 403} and "ban" in lowered:
            return True
        return False

    @staticmethod
    def _is_partner_channel_for_chat_tracking(login: str) -> bool:
        """Check if channel is a partner (wrapper for partner_utils)."""
        return is_partner_channel_for_chat_tracking(login)

    def _blacklist_streamer_for_bot_ban(
        self,
        broadcaster_id: str | None,
        broadcaster_login: str,
        status: int | None,
        text: str,
    ) -> None:
        login = str(broadcaster_login or "").strip().lower().lstrip("#")
        if not login:
            return
        try:
            if self._is_partner_channel_for_chat_tracking(login):
                log.info(
                    "Blacklist übersprungen für Partner-Channel %s (_ensure_bot_is_mod)",
                    login,
                )
                return
        except Exception:
            log.debug(
                "Partner-Check in _ensure_bot_is_mod fehlgeschlagen fuer %s",
                login,
                exc_info=True,
            )

        target_id = str(broadcaster_id or "").strip() or None
        snippet = (text or "").replace("\n", " ").strip()[:180]
        reason = "chat_bot_banned_in_channel"
        if status is not None:
            reason += f" (HTTP {status})"
        if snippet:
            reason += f": {snippet}"

        raid_bot = getattr(self, "_raid_bot", None)
        if raid_bot and hasattr(raid_bot, "_add_to_blacklist"):
            raid_bot._add_to_blacklist(target_id, login, reason)
        else:
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
                        (target_id, login, reason),
                    )
                    conn.commit()
            except Exception:
                log.debug(
                    "Konnte Bot-Ban Blacklist nicht schreiben fuer %s",
                    login,
                    exc_info=True,
                )

        log.warning("Bot-Ban erkannt: %s auf Raid-Blacklist gesetzt.", login)

    async def _ensure_bot_is_mod(self, broadcaster_id: str, broadcaster_login: str) -> bool:
        """
        Setzt den Bot als Moderator im Ziel-Channel über den Streamer-Token.
        Wird aufgerufen wenn ein join() mit 403 fehlschlägt.
        Gibt True zurück wenn der Bot erfolgreich als Mod gesetzt wurde.
        """
        raid_bot = getattr(self, "_raid_bot", None)
        if not raid_bot or not hasattr(raid_bot, "auth_manager"):
            log.debug(
                "_ensure_bot_is_mod: Kein RaidManager verfügbar für %s",
                broadcaster_login,
            )
            return False

        safe_bot_id = self.bot_id_safe or self.bot_id or ""
        if not safe_bot_id:
            log.debug("_ensure_bot_is_mod: Keine Bot-ID verfügbar")
            return False

        # Streamer-Token holen (wird bei Bedarf automatisch refreshed)
        session = raid_bot.session if hasattr(raid_bot, "session") else None
        if not session:
            log.debug("_ensure_bot_is_mod: Keine HTTP-Session im RaidManager")
            return False

        tokens = await raid_bot.auth_manager.get_tokens_for_user(broadcaster_id, session)
        if not tokens:
            log.warning(
                "_ensure_bot_is_mod: Keine gültige Autorisierung für %s verfügbar.",
                broadcaster_login,
            )
            return False

        access_token, _ = tokens

        try:
            import aiohttp

            url = "https://api.twitch.tv/helix/moderation/moderators"
            params = {
                "broadcaster_id": str(broadcaster_id),
                "user_id": str(safe_bot_id),
            }
            headers = {
                "Client-ID": self._client_id,
                "Authorization": f"Bearer {access_token}",
            }
            # Eigene Session öffnen – raid_bot.session kann jederzeit geschlossen
            # sein (Shutdown, Polling-Zyklus).  Konsistent mit _auto_ban_and_cleanup
            # und _unban_user.
            async with aiohttp.ClientSession() as mod_session:
                async with mod_session.post(url, headers=headers, params=params) as r:
                    if r.status in {200, 204}:
                        log.info(
                            "_ensure_bot_is_mod: Bot (ID: %s) ist jetzt Mod in %s (ID: %s)",
                            safe_bot_id,
                            broadcaster_login,
                            broadcaster_id,
                        )
                        return True
                    if r.status == 422:
                        # 422 = Bot ist bereits Mod → sollte nicht vorkommen wenn 403 vorher kam
                        log.info(
                            "_ensure_bot_is_mod: Bot ist bereits Mod in %s (422)",
                            broadcaster_login,
                        )
                        return True
                    txt = await r.text()
                    if self._looks_like_bot_banned_error(r.status, txt):
                        self._blacklist_streamer_for_bot_ban(
                            broadcaster_id=str(broadcaster_id),
                            broadcaster_login=broadcaster_login,
                            status=r.status,
                            text=txt,
                        )
                    # 400 "user is banned" → Bot wurde im Channel gebannt,
                    # Mod-Status kann nicht gesetzt werden bis der Ban aufgehoben wurde
                    log.warning(
                        "_ensure_bot_is_mod: Bot konnte nicht Mod werden in %s: HTTP %s %s",
                        broadcaster_login,
                        r.status,
                        txt[:180].replace("\n", " "),
                    )
                    return False
        except Exception:
            log.exception("_ensure_bot_is_mod: Exception für %s", broadcaster_login)
            return False

    async def _ensure_bot_token_registered(self) -> None:
        """
        TwitchIO nutzt intern den Token, der über add_token()
        registriert wurde.  Falls setup_hook() noch nicht fertig war oder der
        Token zwischenzeitlich refreshed wurde, kann dieser fehlen.  Wir
        registrieren ihn hier nochmal, um die Fehlerquelle zu eliminieren.
        """
        api_token = (self._bot_token or "").replace("oauth:", "").strip()
        if not api_token:
            return
        try:
            await self.add_token(api_token, self._bot_refresh_token)
        except Exception:
            log.debug("_ensure_bot_token_registered: add_token fehlgeschlagen", exc_info=True)

    async def join(self, channel_login: str, channel_id: str | None = None):
        """Joint einen Channel via EventSub (TwitchIO 3.x)."""
        normalized_login = str(channel_login or "").strip().lower().lstrip("#")
        flow_id = self._next_chat_observability_flow_id(prefix="join")
        self._increment_chat_observability_counter("chat_join_attempt_total")

        def _remaining_missing() -> list[str]:
            if not normalized_login:
                return list(self._required_chat_subscription_types())
            return [
                sub_type
                for sub_type in self._required_chat_subscription_types()
                if not self.is_channel_subscription_ready(normalized_login, sub_type)
            ]

        def _emit_join_decision(
            *,
            decision: str,
            detail: str | None = None,
            level: int = logging.INFO,
            auth_diagnostics: dict[str, object] | None = None,
            join_state: dict[str, bool] | None = None,
            exception: Exception | None = None,
        ) -> None:
            self._log_chat_join_decision(
                flow_id=flow_id,
                channel_login=normalized_login or channel_login,
                channel_id=str(channel_id or "").strip() or None,
                remaining_missing=_remaining_missing(),
                decision=decision,
                decision_detail=detail,
                level=level,
                auth_diagnostics=auth_diagnostics,
                join_state=join_state,
                exception=exception,
            )

        try:
            if not channel_id:
                user = await self.fetch_user(login=channel_login.lstrip("#"))
                if not user:
                    log.error("Could not find user ID for channel %s", channel_login)
                    self._increment_chat_observability_counter("chat_join_failure_total")
                    self._increment_chat_observability_counter("chat_join_failure_total_channel_not_found")
                    _emit_join_decision(
                        decision="channel_not_found",
                        detail="fetch_user returned no user",
                        level=logging.ERROR,
                    )
                    return False
                channel_id = str(user.id)

            # Wir nutzen IMMER den Bot-Token für alle Channels.
            # Das hält die Anzahl der WebSocket-Verbindungen auf 1 (Limit bei Twitch ist 3 pro Client ID).
            # Voraussetzung: Der Bot muss Moderator im Ziel-Kanal sein.
            safe_bot_id = self.bot_id_safe or self.bot_id or ""

            # Token vor dem Subscribe sicherstellen – verhindert
            # "invalid transport and auth combination" wenn setup_hook()
            # noch nicht vollständig abgeschlossen war.
            await self._ensure_bot_token_registered()
            if await self._subscribe_missing_chat_subscriptions(
                channel_login=normalized_login,
                channel_id=str(channel_id),
                safe_bot_id=str(safe_bot_id),
            ):
                self._increment_chat_observability_counter("chat_join_success_total")
                _emit_join_decision(
                    decision="joined",
                    detail="required chat subscriptions ready",
                    level=logging.DEBUG,
                )
                return True
            log.warning(
                "join(): Chat-Subscriptions für %s sind unvollständig (%s)",
                channel_login,
                ", ".join(
                    sorted(
                        self._required_chat_subscription_types()
                    )
                ),
            )
            self._increment_chat_observability_counter("chat_join_failure_total")
            self._increment_chat_observability_counter("chat_join_failure_total_incomplete_subscriptions")
            _emit_join_decision(
                decision="subscriptions_incomplete",
                detail="required chat subscriptions still not ready after subscribe attempt",
                level=logging.WARNING,
            )
            return False
        except Exception as e:
            msg = str(e)
            remaining_missing = _remaining_missing()
            if self._looks_like_transport_session_gone_error(msg):
                for sub_type in remaining_missing:
                    self._record_chat_subscription_state(
                        normalized_login,
                        sub_type,
                        "transport_session_invalid",
                        detail="eventsub websocket transport session does not exist",
                    )
                log.warning(
                    "join(): transport session invalid for %s - scheduling chat bot restart.",
                    channel_login,
                )
                restart = getattr(self, "request_transport_restart", None)
                if callable(restart):
                    try:
                        await restart(
                            reason="eventsub websocket transport session does not exist",
                            failed_channel=normalized_login,
                        )
                    except Exception:
                        log.exception(
                            "join(): failed to schedule chat bot restart for %s",
                            channel_login,
                        )
                log.error("Failed to join channel %s: %s", channel_login, e)
                self._increment_chat_observability_counter("chat_join_failure_total")
                self._increment_chat_observability_counter("chat_join_failure_total_transport_session_invalid")
                _emit_join_decision(
                    decision="transport_session_invalid",
                    detail="eventsub websocket transport session does not exist",
                    level=logging.ERROR,
                    exception=e,
                )
                return False
            if "invalid transport and auth combination" in msg:
                # Token war zum Zeitpunkt des ersten Versuchs noch nicht
                # gebunden.  Kurz warten, Token nochmal registrieren und
                # einmal erneut versuchen.
                log.warning(
                    "join(): 'invalid transport and auth combination' für %s – "
                    "Token wird neu registriert und ein Retry folgt.",
                    channel_login,
                )
                for sub_type in remaining_missing:
                    self._record_chat_subscription_state(
                        normalized_login,
                        sub_type,
                        "bot_token_not_registered",
                        detail="invalid transport and auth combination",
                    )
                await asyncio.sleep(1)
                await self._ensure_bot_token_registered()
                try:
                        if await self._subscribe_missing_chat_subscriptions(
                            channel_login=normalized_login,
                            channel_id=str(channel_id),
                            safe_bot_id=str(safe_bot_id),
                        ):
                            log.info(
                                "join(): Retry erfolgreich für %s nach Token-Registrierung",
                                channel_login,
                            )
                            self._increment_chat_observability_counter("chat_join_success_total")
                            _emit_join_decision(
                                decision="joined_after_token_retry",
                                detail="retry after bot token registration succeeded",
                                exception=e,
                            )
                            return True
                except Exception as retry_err:
                    log.error(
                        "join(): Retry für %s fehlgeschlagen: %s",
                        channel_login,
                        retry_err,
                    )
                    self._increment_chat_observability_counter("chat_join_failure_total")
                    self._increment_chat_observability_counter("chat_join_failure_total_token_retry_failed")
                    _emit_join_decision(
                        decision="bot_token_retry_failed",
                        detail="retry after bot token registration failed",
                        level=logging.ERROR,
                        exception=retry_err,
                    )
                    return False
                self._increment_chat_observability_counter("chat_join_failure_total")
                self._increment_chat_observability_counter("chat_join_failure_total_bot_token_not_registered")
                _emit_join_decision(
                    decision="bot_token_not_registered",
                    detail="invalid transport and auth combination",
                    level=logging.WARNING,
                    exception=e,
                )
                return False
            if "403" in msg and "subscription missing proper authorization" in msg:
                auth_diagnostics = self._diagnose_chat_subscription_authorization(
                    channel_login=normalized_login,
                    channel_id=str(channel_id),
                )
                bot_scope_state_known = bool(auth_diagnostics.get("bot_scope_state_known"))
                join_state = self._load_chat_join_channel_state(
                    channel_login=normalized_login,
                    channel_id=str(channel_id),
                )
                if auth_diagnostics["missing_bot_scopes"]:
                    missing_bot_scopes = ", ".join(auth_diagnostics["missing_bot_scopes"])
                    for sub_type in remaining_missing:
                        self._record_chat_subscription_state(
                            normalized_login,
                            sub_type,
                            "missing_bot_scope",
                            detail=missing_bot_scopes,
                        )
                    log.warning(
                        "join(): Chat-Subscription für %s blockiert – zentraler Bot-Scope fehlt (%s).",
                        channel_login,
                        missing_bot_scopes,
                    )
                    self._increment_chat_observability_counter("chat_join_failure_total")
                    self._increment_chat_observability_counter("chat_join_failure_total_missing_bot_scope")
                    _emit_join_decision(
                        decision="missing_bot_scope",
                        detail=missing_bot_scopes,
                        level=logging.WARNING,
                        auth_diagnostics=auth_diagnostics,
                        join_state=join_state,
                        exception=e,
                    )
                    return False
                if auth_diagnostics["missing_broadcaster_scopes"]:
                    missing_broadcaster_scopes = ", ".join(
                        auth_diagnostics["missing_broadcaster_scopes"]
                    )
                    for sub_type in remaining_missing:
                        self._record_chat_subscription_state(
                            normalized_login,
                            sub_type,
                            "missing_broadcaster_scope",
                            detail=missing_broadcaster_scopes,
                        )
                    log.warning(
                        "join(): Chat-Subscription für %s blockiert – Broadcaster-Freigabe fehlt (%s).",
                        channel_login,
                        missing_broadcaster_scopes,
                    )
                    self._increment_chat_observability_counter("chat_join_failure_total")
                    self._increment_chat_observability_counter("chat_join_failure_total_missing_broadcaster_scope")
                    _emit_join_decision(
                        decision="missing_broadcaster_scope",
                        detail=missing_broadcaster_scopes,
                        level=logging.WARNING,
                        auth_diagnostics=auth_diagnostics,
                        join_state=join_state,
                        exception=e,
                    )
                    return False
                if not bot_scope_state_known:
                    for sub_type in remaining_missing:
                        self._record_chat_subscription_state(
                            normalized_login,
                            sub_type,
                            "unknown_bot_scope_state",
                            detail="central bot scope set unavailable during 403 auth diagnosis",
                        )
                    log.warning(
                        "join(): Chat-Subscription für %s blockiert – zentraler Bot-Scope-Zustand ist unbekannt. Kein Mod-Retry.",
                        channel_login,
                    )
                    self._increment_chat_observability_counter("chat_join_failure_total")
                    self._increment_chat_observability_counter("chat_join_failure_total_unknown_bot_scope_state")
                    _emit_join_decision(
                        decision="unknown_bot_scope_state",
                        detail="central bot scope set unavailable during 403 auth diagnosis",
                        level=logging.WARNING,
                        auth_diagnostics=auth_diagnostics,
                        join_state=join_state,
                        exception=e,
                    )
                    return False
                if join_state["is_partner_active"] and not join_state["has_raid_auth"]:
                    for sub_type in remaining_missing:
                        self._record_chat_subscription_state(
                            normalized_login,
                            sub_type,
                            "missing_broadcaster_authorization",
                            detail="partner channel has no twitch_raid_auth authorization record",
                        )
                    log.warning(
                        "join(): Chat-Subscription für %s blockiert – Partner-Channel hat keine Broadcaster-Autorisierung. Kein Mod-Retry.",
                        channel_login,
                    )
                    self._increment_chat_observability_counter("chat_join_failure_total")
                    self._increment_chat_observability_counter("chat_join_failure_total_missing_broadcaster_authorization")
                    _emit_join_decision(
                        decision="missing_broadcaster_authorization",
                        detail="partner channel has no twitch_raid_auth authorization record",
                        level=logging.WARNING,
                        auth_diagnostics=auth_diagnostics,
                        join_state=join_state,
                        exception=e,
                    )
                    return False
                if (
                    not join_state["exists_in_streamers"]
                    and not join_state["is_partner_active"]
                    and not join_state["has_raid_auth"]
                ):
                    for sub_type in remaining_missing:
                        self._record_chat_subscription_state(
                            normalized_login,
                            sub_type,
                            "stale_removed_channel",
                            detail="channel no longer tracked locally or authorized",
                        )
                    self._purge_local_channel_state(normalized_login)
                    log.info(
                        "join(): stale/removed channel %s detected after 403; local chat state purged, no mod retry.",
                        channel_login,
                    )
                    self._increment_chat_observability_counter("chat_join_failure_total")
                    self._increment_chat_observability_counter("chat_join_purged_stale_total")
                    self._increment_chat_observability_counter("chat_join_failure_total_stale_removed_channel")
                    _emit_join_decision(
                        decision="stale_removed_channel",
                        detail="channel no longer tracked locally or authorized",
                        auth_diagnostics=auth_diagnostics,
                        join_state=join_state,
                        exception=e,
                    )
                    return False
                # Monitored-Only Channels: kein Mod-Versuch, einfach überspringen.
                # Diese Channels haben keinen Streamer-Token, daher ist _ensure_bot_is_mod
                # sinnlos und würde nur Warnungen produzieren.
                if self._is_monitored_only(normalized_login) or join_state["is_monitored_only"]:
                    for sub_type in remaining_missing:
                        self._record_chat_subscription_state(
                            normalized_login,
                            sub_type,
                            "monitored_only_no_broadcaster_auth",
                            detail="subscription missing proper authorization",
                        )
                    log.info(
                        "join(): 403 für Monitored-Only Channel %s – kein Mod-Versuch, "
                        "Channel wird übersprungen (kein Streamer-Token verfügbar).",
                        channel_login,
                    )
                    self._increment_chat_observability_counter("chat_join_failure_total")
                    self._increment_chat_observability_counter("chat_join_failure_total_monitored_only_no_broadcaster_auth")
                    _emit_join_decision(
                        decision="monitored_only_no_broadcaster_auth",
                        detail="subscription missing proper authorization",
                        auth_diagnostics=auth_diagnostics,
                        join_state=join_state,
                        exception=e,
                    )
                    return False

                # Cooldown-Prüfung: Bei gebannen Bots nicht wiederholt versuchen
                cd_key = normalized_login
                cd_until = self._mod_retry_cooldown.get(cd_key)
                if cd_until and datetime.now(UTC) < cd_until:
                    log.debug(
                        "join(): Mod-Retry für %s auf Cooldown bis %s – überspringe",
                        channel_login,
                        cd_until.isoformat(),
                    )
                    self._increment_chat_observability_counter("chat_join_failure_total")
                    self._increment_chat_observability_counter("chat_join_failure_total_mod_retry_cooldown")
                    _emit_join_decision(
                        decision="mod_retry_cooldown",
                        detail="mod retry cooldown active",
                        level=logging.DEBUG,
                        auth_diagnostics=auth_diagnostics,
                        join_state=join_state,
                        exception=e,
                    )
                    return False

                # Automatischer Retry: Bot als Mod setzen und nochmal versuchen
                self._increment_chat_observability_counter("chat_join_mod_retry_total")
                log.info(
                    "join(): 403 für %s – versuche Bot automatisch als Mod zu setzen...",
                    channel_login,
                )
                mod_set = await self._ensure_bot_is_mod(str(channel_id), channel_login)
                if mod_set:
                    # Kurze Pause damit Twitch den Mod-Status propagiert
                    await asyncio.sleep(1)
                    try:
                        if await self._subscribe_missing_chat_subscriptions(
                            channel_login=normalized_login,
                            channel_id=str(channel_id),
                            safe_bot_id=str(safe_bot_id),
                        ):
                            log.info(
                                "join(): Retry erfolgreich für %s nach Mod-Autorisierung",
                                channel_login,
                            )
                            self._increment_chat_observability_counter("chat_join_success_total")
                            _emit_join_decision(
                                decision="joined_after_mod_retry",
                                detail="retry after mod authorization succeeded",
                                auth_diagnostics=auth_diagnostics,
                                join_state=join_state,
                                exception=e,
                            )
                            return True
                    except Exception as retry_err:
                        log.warning(
                            "join(): Retry für %s fehlgeschlagen nach Mod-Autorisierung: %s",
                            channel_login,
                            retry_err,
                        )
                        self._increment_chat_observability_counter("chat_join_failure_total")
                        self._increment_chat_observability_counter("chat_join_failure_total_mod_retry_failed")
                        _emit_join_decision(
                            decision="mod_retry_failed",
                            detail="retry after mod authorization failed",
                            level=logging.WARNING,
                            auth_diagnostics=auth_diagnostics,
                            join_state=join_state,
                            exception=retry_err,
                        )
                        return False
                else:
                    # Cooldown setzen: Nächster Retry erst nach 10 Minuten
                    self._mod_retry_cooldown[cd_key] = datetime.now(UTC) + timedelta(minutes=10)
                    log.warning(
                        "join(): Konnte Bot nicht als Mod in %s setzen. "
                        "Falls der Bot im Channel gebannt ist, muss er dort zuerst "
                        "entbannt werden (/unban deutschedeadlockcommunity), "
                        "danach /mod deutschedeadlockcommunity ausführen. "
                        "Nächster Retry in 10 min.",
                        channel_login,
                    )
                self._increment_chat_observability_counter("chat_join_failure_total")
                self._increment_chat_observability_counter("chat_join_failure_total_mod_retry_not_resolved")
                _emit_join_decision(
                    decision="mod_retry_not_resolved",
                    detail="mod retry path did not restore required chat subscriptions",
                    level=logging.WARNING,
                    auth_diagnostics=auth_diagnostics,
                    join_state=join_state,
                    exception=e,
                )
            elif "429" in msg or "transport limit exceeded" in msg.lower():
                for sub_type in remaining_missing:
                    self._record_chat_subscription_state(
                        normalized_login,
                        sub_type,
                        "transport_limit",
                        detail="WebSocket transport limit reached",
                    )
                log.error(
                    "Cannot join chat for %s: WebSocket Transport Limit (429) reached. "
                    "Ensure the bot uses only one WebSocket connection.",
                    channel_login,
                )
                self._increment_chat_observability_counter("chat_join_failure_total")
                self._increment_chat_observability_counter("chat_join_failure_total_transport_limit")
                _emit_join_decision(
                    decision="transport_limit",
                    detail="websocket transport limit reached",
                    level=logging.ERROR,
                    exception=e,
                )
            else:
                for sub_type in remaining_missing:
                    self._record_chat_subscription_state(
                        normalized_login,
                        sub_type,
                        "subscribe_failed",
                        detail=msg[:200],
                    )
                log.error("Failed to join channel %s: %s", channel_login, e)
                self._increment_chat_observability_counter("chat_join_failure_total")
                self._increment_chat_observability_counter("chat_join_failure_total_subscribe_failed")
                _emit_join_decision(
                    decision="subscribe_failed",
                    detail=msg[:200],
                    level=logging.ERROR,
                    exception=e,
                )
            return False

    async def join_channels(
        self,
        channels: list[str],
        rate_limit_delay: float = 0.2,
        *,
        mark_monitored_only: bool = True,
    ) -> int:
        """Kompatibilitäts-Helper für Bulk-Joins (z.B. Scout-Task)."""
        if not channels:
            return 0

        normalized = [str(ch or "").strip().lower().lstrip("#") for ch in channels]
        normalized = [ch for ch in normalized if ch]
        if not normalized:
            return 0

        try:
            set_monitored = getattr(self, "set_monitored_channels", None)
            if mark_monitored_only and callable(set_monitored):
                set_monitored(normalized)
        except Exception:
            log.debug(
                "join_channels: konnte monitored-only Liste nicht aktualisieren",
                exc_info=True,
            )

        joined = 0
        for login in normalized:
            try:
                success = await self.join(login)
                if success:
                    joined += 1
                    if rate_limit_delay > 0:
                        await asyncio.sleep(rate_limit_delay)
                elif self._has_active_transport_restart():
                    log.warning(
                        "join_channels: breche Batch nach %s ab, da ein Chat-Transport-Restart aktiv ist.",
                        login,
                    )
                    break
            except Exception:
                log.exception("join_channels: unerwarteter Fehler bei %s", login)
                if self._has_active_transport_restart():
                    log.warning(
                        "join_channels: breche Batch nach Fehler bei %s ab, da ein Chat-Transport-Restart aktiv ist.",
                        login,
                    )
                    break

        return joined

    async def part_channels(self, channels: list[str]) -> int:
        """Best-effort Unsubscribe + lokale Cache-Bereinigung für Channels."""
        if not channels:
            return 0

        normalized = [str(ch or "").strip().lower().lstrip("#") for ch in channels]
        normalized = [ch for ch in normalized if ch]
        if not normalized:
            return 0

        # Kandidaten-IDs vor dem Cache-Cleanup sichern
        channel_ids: dict[str, str] = {}
        channel_id_map = getattr(self, "_channel_ids", None)
        if isinstance(channel_id_map, dict):
            for login in normalized:
                raw_id = channel_id_map.get(login)
                if raw_id:
                    channel_ids[login] = str(raw_id)

        # Fehlende IDs nachladen (best effort)
        missing = [login for login in normalized if login not in channel_ids]
        fetch_users = getattr(self, "fetch_users", None)
        if missing and callable(fetch_users):
            try:
                users = await fetch_users(logins=missing)
                for user in users or []:
                    login = str(getattr(user, "login", "") or "").lower()
                    uid = str(getattr(user, "id", "") or "").strip()
                    if login and uid:
                        channel_ids[login] = uid
            except Exception:
                log.debug(
                    "part_channels: konnte User-IDs nicht nachladen (%s)",
                    ", ".join(missing),
                    exc_info=True,
                )
        elif missing:
            fetch_user = getattr(self, "fetch_user", None)
            if callable(fetch_user):
                for login in missing:
                    try:
                        user = await fetch_user(login=login)
                        uid = str(getattr(user, "id", "") or "").strip() if user else ""
                        if uid:
                            channel_ids[login] = uid
                    except Exception:
                        log.debug(
                            "part_channels: fetch_user fehlgeschlagen für %s",
                            login,
                            exc_info=True,
                        )

        target_ids = {uid for uid in channel_ids.values() if uid}
        unsubscribed = 0

        # 1) Primärpfad: Helix EventSub listing (TwitchIO fetch_eventsub_subscriptions)
        fetch_subs = getattr(self, "fetch_eventsub_subscriptions", None)
        delete_sub = getattr(self, "delete_eventsub_subscription", None)
        if target_ids and callable(fetch_subs) and callable(delete_sub):
            try:
                subs_result = fetch_subs()
                if inspect.isawaitable(subs_result):
                    subs_result = await subs_result

                subs_list = []

                async def _consume_async_iter(source) -> bool:
                    if source is None:
                        return False
                    if hasattr(source, "__aiter__"):
                        async for sub in source:
                            subs_list.append(sub)
                        return True
                    if hasattr(source, "__anext__"):
                        while True:
                            try:
                                sub = await source.__anext__()
                            except StopAsyncIteration:
                                break
                            subs_list.append(sub)
                        return True
                    return False

                handled = await _consume_async_iter(subs_result)
                if not handled and subs_result is not None:
                    inner = getattr(subs_result, "subscriptions", None)
                    if inner is not None:
                        handled = await _consume_async_iter(inner)
                        if not handled:
                            try:
                                subs_list.extend(list(inner))
                            except TypeError:
                                log.debug(
                                    "part_channels: unerwarteter subscriptions-Typ: %s",
                                    type(inner),
                                )
                    else:
                        try:
                            subs_list.extend(list(subs_result))
                        except TypeError:
                            log.debug(
                                "part_channels: unerwarteter fetch_eventsub_subscriptions-Typ: %s",
                                type(subs_result),
                            )

                for sub in subs_list:
                    try:
                        sub_type = getattr(sub, "type", "") or getattr(sub, "subscription_type", "")
                        if not self._is_chat_eventsub_subscription_type(sub_type):
                            continue
                        condition = getattr(sub, "condition", None)
                        if isinstance(condition, dict):
                            broadcaster_id = str(
                                condition.get("broadcaster_user_id")
                                or condition.get("broadcaster_id")
                                or ""
                            ).strip()
                        else:
                            broadcaster_id = str(
                                getattr(condition, "broadcaster_user_id", "")
                                or getattr(condition, "broadcaster_id", "")
                                or ""
                            ).strip()
                        if not broadcaster_id or broadcaster_id not in target_ids:
                            continue

                        sub_id = (
                            getattr(sub, "id", None)
                            or getattr(sub, "subscription_id", None)
                            or getattr(sub, "uuid", None)
                        )
                        if sub_id:
                            await delete_sub(sub_id)
                            unsubscribed += 1
                    except Exception:
                        log.debug(
                            "part_channels: Fehler beim Loeschen einer EventSub-Subscription",
                            exc_info=True,
                        )
            except Exception:
                log.debug("part_channels: fetch_eventsub_subscriptions fehlgeschlagen", exc_info=True)

        # 2) Fallback: TwitchIO 3.x WebSocket-Subscriptions (falls verfügbar)
        if target_ids and unsubscribed == 0:
            ws_subs = getattr(self, "websocket_subscriptions", None)
            ws_delete = getattr(self, "delete_websocket_subscription", None)
            if callable(ws_subs) and callable(ws_delete):
                try:
                    subs_map = await ws_subs()
                    if isinstance(subs_map, dict):
                        for sub_id, sub in subs_map.items():
                            try:
                                condition = getattr(sub, "condition", None)
                                if isinstance(condition, dict):
                                    broadcaster_id = str(
                                        condition.get("broadcaster_user_id")
                                        or condition.get("broadcaster_id")
                                        or ""
                                    ).strip()
                                else:
                                    broadcaster_id = str(
                                        getattr(condition, "broadcaster_user_id", "")
                                        or getattr(condition, "broadcaster_id", "")
                                        or ""
                                    ).strip()
                                if not broadcaster_id or broadcaster_id not in target_ids:
                                    continue

                                sub_type = getattr(sub, "type", "")
                                sub_type_value = getattr(sub_type, "value", None) or str(sub_type or "")
                                if not sub_type_value:
                                    continue
                                if not self._is_chat_eventsub_subscription_type(sub_type_value):
                                    continue

                                await ws_delete(sub_id)
                                unsubscribed += 1
                            except Exception:
                                log.debug(
                                    "part_channels: Fehler beim Loeschen einer WebSocket-Subscription",
                                    exc_info=True,
                                )
                except Exception:
                    log.debug("part_channels: websocket_subscriptions fehlgeschlagen", exc_info=True)

        # Lokale Caches bereinigen
        monitored = getattr(self, "_monitored_streamers", None)
        if isinstance(monitored, set):
            for login in normalized:
                monitored.discard(login)
        if isinstance(channel_id_map, dict):
            for login in normalized:
                channel_id_map.pop(login, None)
        channel_subscriptions = getattr(self, "_channel_subscription_types", None)
        if isinstance(channel_subscriptions, dict):
            for login in normalized:
                channel_subscriptions.pop(login, None)
        channel_subscription_state = getattr(self, "_channel_subscription_state", None)
        if isinstance(channel_subscription_state, dict):
            for login in normalized:
                channel_subscription_state.pop(login, None)
        monitored_only = getattr(self, "_monitored_only_channels", None)
        if isinstance(monitored_only, set):
            for login in normalized:
                monitored_only.discard(login)
        initial_channels = getattr(self, "_initial_channels", None)
        if isinstance(initial_channels, list):
            normalized_set = set(normalized)
            self._initial_channels = [
                channel
                for channel in initial_channels
                if str(channel or "").strip().lower().lstrip("#") not in normalized_set
            ]

        if unsubscribed:
            log.info(
                "part_channels: %d EventSub-Subscription(s) geloescht (%d Channels).",
                unsubscribed,
                len(normalized),
            )
        else:
            log.debug(
                "part_channels: keine EventSub-Subscription geloescht (Channels=%d, targets=%d)",
                len(normalized),
                len(target_ids),
            )

        return unsubscribed

    async def follow_channel(self, broadcaster_id: str) -> bool:
        """
        Prüft, ob der Bot dem Channel bereits folgt.

        Hinweis: Twitch bietet seit dem 28.07.2021 keine öffentliche Helix-API
        mehr zum Erstellen von Follows an.
        """
        safe_bot_id = self.bot_id_safe or self.bot_id
        if not safe_bot_id or not self._token_manager:
            log.debug("follow_channel: Kein Bot-ID oder Token-Manager verfügbar")
            return False

        import aiohttp

        for attempt in range(2):
            try:
                tokens = await self._token_manager.get_valid_token()
                if not tokens:
                    return False
                access_token, _ = tokens

                async with aiohttp.ClientSession() as session:
                    headers = {
                        "Client-ID": self._client_id,
                        "Authorization": f"Bearer {access_token}",
                    }
                    params = {
                        "user_id": str(safe_bot_id),
                        "broadcaster_id": str(broadcaster_id),
                    }
                    async with session.get(
                        "https://api.twitch.tv/helix/channels/followed",
                        headers=headers,
                        params=params,
                    ) as r:
                        if r.status == 200:
                            data = await r.json(content_type=None)
                            follows = data.get("data", []) if isinstance(data, dict) else []
                            if follows:
                                log.info(
                                    "follow_channel: Bot folgt bereits %s",
                                    broadcaster_id,
                                )
                                return True

                            if not getattr(self, "_follow_api_create_removed_logged", False):
                                log.info(
                                    "follow_channel: Twitch-API kann keine Follows mehr erstellen "
                                    "(abgeschaltet am 28.07.2021). Manual Follow erforderlich."
                                )
                                self._follow_api_create_removed_logged = True
                            log.debug(
                                "follow_channel: Bot folgt %s derzeit nicht",
                                broadcaster_id,
                            )
                            return False
                        txt = await r.text()
                        if r.status == 401:
                            txt_l = txt.lower()
                            if "user:read:follows" in txt_l or "missing required scope" in txt_l:
                                if not getattr(self, "_follow_scope_missing_logged", False):
                                    log.warning(
                                        "follow_channel: Bot-Token ohne Scope user:read:follows; "
                                        "Follow-Status kann nicht geprüft werden."
                                    )
                                    self._follow_scope_missing_logged = True
                                return False
                            if attempt == 0:
                                log.debug(
                                    "follow_channel: 401 für %s, triggere Token-Refresh",
                                    broadcaster_id,
                                )
                                await self._token_manager.get_valid_token(force_refresh=True)
                                continue
                        log.debug(
                            "follow_channel: Follow-Check HTTP %s – %s",
                            r.status,
                            txt[:200],
                        )
                        return False
            except Exception:
                log.debug("follow_channel: Exception", exc_info=True)
                return False
        return False

    async def join_partner_channels(self):
        """
        Joint ALLE Channels (Partner + Monitored + Category).

        Datensammlung: ALLE
        Bot-Funktionen: Nur Partner (wird in event_message geprüft)
        """
        with get_conn() as conn:
            # Hole ALLE Streamer mit OAuth (Partner + wer OAuth hat)
            # Datensammlung läuft für alle, Bot-Funktionen nur für Partner
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
                WHERE a.raid_enabled IS TRUE
                   OR s.is_partner_active = 1
                """
            ).fetchall()

        channels_to_join = []
        for login, uid, scopes_raw, is_live, last_game in partners:
            login_norm = (login or "").strip()
            if not login_norm:
                continue
            scopes = [s.strip().lower() for s in (scopes_raw or "").split() if s.strip()]
            has_channel_bot_grant = "channel:bot" in scopes
            if not has_channel_bot_grant:
                continue
            if (is_live is None or not bool(is_live)) and not CHAT_JOIN_OFFLINE:
                continue
            # Normalisieren und prüfen
            normalized_login = login_norm.lower().lstrip("#")

            if self.is_channel_subscription_ready(normalized_login):
                continue
            channels_to_join.append((login_norm, uid))

        if channels_to_join:
            label = "LIVE partner" if not CHAT_JOIN_OFFLINE else "partner"
            log.info(
                "Joining %d new %s channels: %s",
                len(channels_to_join),
                label,
                ", ".join([c[0] for c in channels_to_join[:10]]),
            )
            for login, uid in channels_to_join:
                try:
                    # Wir übergeben ID falls vorhanden, sonst wird sie in join() gefetched
                    success = await self.join(login, channel_id=uid)
                    if success:
                        await asyncio.sleep(0.2)  # Rate limiting
                except Exception as e:
                    log.exception("Unexpected error joining channel %s: %s", login, e)
