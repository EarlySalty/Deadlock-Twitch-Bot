"""Shared core EventSub callback registration for webhook and websocket transports."""

from __future__ import annotations

import logging
from collections.abc import Awaitable, Callable
from typing import Any, Protocol

EventCallback = Callable[..., Awaitable[None]]
EVENTSUB_CORE_DELIVERY_TYPES = frozenset(
    {
        "stream.online",
        "stream.offline",
        "channel.update",
        "channel.raid",
    }
)


class EventSubCallbackSink(Protocol):
    def set_callback(self, sub_type: str, callback: EventCallback) -> None: ...


def is_core_eventsub_delivery_type(sub_type: str) -> bool:
    return str(sub_type or "").strip().lower() in EVENTSUB_CORE_DELIVERY_TYPES


def register_core_eventsub_callbacks(
    owner: Any,
    handler: EventSubCallbackSink,
    *,
    logger: logging.Logger | None = None,
    propagate_callback_errors: bool = False,
    delivery_mode: str = "inline",
) -> None:
    log = logger or logging.getLogger("TwitchStreams")
    if delivery_mode not in {"inline", "enqueue"}:
        raise ValueError("delivery_mode must be 'inline' or 'enqueue'")

    async def _run_callback(
        callback: EventCallback,
        failure_message: str,
        target: str,
    ) -> None:
        try:
            await callback()
        except Exception:
            log.exception(failure_message, target)
            if propagate_callback_errors:
                raise

    async def _offline_cb(
        broadcaster_id: str,
        broadcaster_login: str,
        _event: dict[str, Any],
        *,
        message_id: str | None = None,
    ) -> None:
        offline_dispatch = getattr(owner, "_enqueue_eventsub_stream_offline_processing", None)
        if delivery_mode == "enqueue" and callable(offline_dispatch):
            callback = lambda: offline_dispatch(
                broadcaster_id,
                broadcaster_login,
                message_id=message_id,
            )
        else:
            callback = lambda: owner._on_eventsub_stream_offline(
                broadcaster_id,
                broadcaster_login,
            )
        await _run_callback(
            callback,
            "EventSub: Offline-Callback fehlgeschlagen für %s",
            broadcaster_login or broadcaster_id,
        )

    async def _raid_cb(
        to_broadcaster_id: str,
        to_broadcaster_login: str,
        event: dict[str, Any],
        *,
        message_id: str | None = None,
    ) -> None:
        if delivery_mode == "enqueue":
            enqueue = getattr(owner, "_enqueue_eventsub_raid_processing", None)
            if callable(enqueue):
                await _run_callback(
                    lambda: enqueue(
                        to_broadcaster_id,
                        to_broadcaster_login,
                        event,
                        message_id=message_id,
                    ),
                    "EventSub: Raid-Callback fehlgeschlagen für %s",
                    to_broadcaster_login or to_broadcaster_id,
                )
                return

        async def _dispatch() -> None:
            raid_bot = getattr(owner, "_raid_bot", None)
            if not raid_bot:
                log.debug(
                    "EventSub: Raid-Bot nicht verfügbar für channel.raid von %s",
                    to_broadcaster_login,
                )
                return
            from_login = str(event.get("from_broadcaster_user_login") or "").strip().lower()
            from_broadcaster_id = str(event.get("from_broadcaster_user_id") or "").strip()
            viewer_count = int(event.get("viewers") or 0)
            if not from_login:
                log.warning("EventSub: channel.raid event ohne from_broadcaster_user_login")
                return
            log.info(
                "EventSub: channel.raid: %s -> %s (%d viewers)",
                from_login,
                to_broadcaster_login,
                viewer_count,
            )
            await raid_bot.on_raid_arrival(
                to_broadcaster_id=to_broadcaster_id,
                to_broadcaster_login=to_broadcaster_login,
                from_broadcaster_login=from_login,
                from_broadcaster_id=from_broadcaster_id,
                viewer_count=viewer_count,
            )
        await _run_callback(
            _dispatch,
            "EventSub: Raid-Callback fehlgeschlagen für %s",
            to_broadcaster_login or to_broadcaster_id,
        )

    async def _online_cb(
        broadcaster_id: str,
        broadcaster_login: str,
        event: dict[str, Any],
        *,
        message_id: str | None = None,
    ) -> None:
        if delivery_mode == "enqueue":
            enqueue = getattr(owner, "_enqueue_eventsub_stream_online_processing", None)
            if callable(enqueue):
                await _run_callback(
                    lambda: enqueue(
                        broadcaster_id,
                        broadcaster_login,
                        event,
                        message_id=message_id,
                    ),
                    "EventSub: stream.online-Callback fehlgeschlagen für %s",
                    broadcaster_login or broadcaster_id,
                )
                return
        await _run_callback(
            lambda: owner._handle_stream_online(broadcaster_id, broadcaster_login, event),
            "EventSub: stream.online-Callback fehlgeschlagen für %s",
            broadcaster_login or broadcaster_id,
        )

    async def _channel_update_cb(
        broadcaster_id: str,
        broadcaster_login: str,
        event: dict[str, Any],
        *,
        message_id: str | None = None,
    ) -> None:
        if delivery_mode == "enqueue":
            enqueue = getattr(owner, "_enqueue_eventsub_channel_update_processing", None)
            if callable(enqueue):
                await _run_callback(
                    lambda: enqueue(
                        broadcaster_id,
                        broadcaster_login,
                        event,
                        message_id=message_id,
                    ),
                    "EventSub: channel.update-Callback fehlgeschlagen für %s",
                    broadcaster_login or broadcaster_id,
                )
                return
        await _run_callback(
            lambda: owner._handle_channel_update(broadcaster_id, event),
            "EventSub: channel.update-Callback fehlgeschlagen für %s",
            broadcaster_login or broadcaster_id,
        )

    handler.set_callback("stream.online", _online_cb)
    handler.set_callback("stream.offline", _offline_cb)
    handler.set_callback("channel.raid", _raid_cb)
    handler.set_callback("channel.update", _channel_update_cb)
