"""Multi-transport EventSub WebSocket pool for fallback mode."""

from __future__ import annotations

import asyncio
import logging
from collections.abc import Awaitable, Callable
from contextlib import suppress

from .eventsub_ws import (
    MAX_SUBSCRIPTIONS_PER_TRANSPORT,
    EventCallback,
    EventSubWSListener,
)
from .eventsub_state_store import EventSubStateStore

MAX_WEBSOCKET_TRANSPORTS = 3


class EventSubWSListenerPool:
    """Distributes EventSub subscriptions across up to three websocket transports."""

    def __init__(
        self,
        api,
        logger: logging.Logger | None = None,
        token_resolver: Callable[[str], Awaitable[str | None]] | None = None,
        state_store: EventSubStateStore | None = None,
        *,
        listener_factory: Callable[..., EventSubWSListener] = EventSubWSListener,
        max_transports: int = MAX_WEBSOCKET_TRANSPORTS,
    ) -> None:
        self.api = api
        self.log = logger or logging.getLogger("TwitchStreams.EventSubWSPool")
        self._token_resolver = token_resolver
        self._state_store = state_store
        self._listener_factory = listener_factory
        self._max_transports = max(1, int(max_transports))
        self._callbacks: dict[str, EventCallback] = {}
        self._listeners: list[EventSubWSListener] = []
        self._listener_tasks: dict[EventSubWSListener, asyncio.Task[None]] = {}
        self._run_started = False
        self._stop = False
        self._state_changed = asyncio.Event()

    def _finalize_completed_listener_tasks(self) -> None:
        completed_items = [
            (listener, task)
            for listener, task in list(self._listener_tasks.items())
            if task.done()
        ]
        if not completed_items:
            return

        pool_failed = False
        for listener, task in completed_items:
            self._listener_tasks.pop(listener, None)
            with suppress(ValueError):
                self._listeners.remove(listener)
            try:
                exc = task.exception()
            except asyncio.CancelledError:
                if not self._stop:
                    self.log.debug("EventSub WS: Transport-Task wurde abgebrochen")
                continue
            if exc is not None:
                pool_failed = True
                self.log.error(
                    "EventSub WS: Transport-Task fehlgeschlagen",
                    exc_info=(type(exc), exc, exc.__traceback__),
                )
        if pool_failed and not self._stop:
            self.log.warning(
                "EventSub WS: Mindestens ein Transport ist fehlgeschlagen; stoppe den Pool für einen sauberen Supervisor-Restart."
            )
            self._stop = True
            for listener in self._active_listeners():
                listener.stop()
        self._state_changed.set()

    def _active_listeners(self) -> tuple[EventSubWSListener, ...]:
        self._finalize_completed_listener_tasks()
        return tuple(self._listeners)

    @property
    def listener_count(self) -> int:
        return len(self._active_listeners())

    @property
    def ready_listener_count(self) -> int:
        return sum(1 for listener in self._active_listeners() if listener.is_ready)

    @property
    def failed_listener_count(self) -> int:
        return sum(1 for listener in self._active_listeners() if listener.is_failed)

    @property
    def listeners_at_limit(self) -> int:
        return sum(1 for listener in self._active_listeners() if not listener.has_capacity)

    @property
    def subscription_count(self) -> int:
        return sum(listener.subscription_count for listener in self._active_listeners())

    @property
    def cost(self) -> int:
        listeners = self._active_listeners()
        return sum(
            min(
                int(getattr(listener, "cost", listener.subscription_count) or 0),
                MAX_SUBSCRIPTIONS_PER_TRANSPORT,
            )
            for listener in listeners
        )

    @property
    def has_capacity(self) -> bool:
        listeners = self._active_listeners()
        return any(listener.has_capacity for listener in listeners) or (
            len(self._listeners) < self._max_transports
        )

    @property
    def is_ready(self) -> bool:
        return any(listener.is_ready for listener in self._active_listeners())

    @property
    def is_failed(self) -> bool:
        listeners = self._active_listeners()
        return bool(listeners) and all(listener.is_failed for listener in listeners)

    def set_callback(self, sub_type: str, callback: EventCallback) -> None:
        self._callbacks[sub_type] = callback
        for listener in self._active_listeners():
            listener.set_callback(sub_type, callback)

    def stop(self) -> None:
        self._stop = True
        for listener in self._active_listeners():
            listener.stop()
        self._state_changed.set()

    def _create_listener(self) -> EventSubWSListener:
        listener = self._listener_factory(
            api=self.api,
            logger=self.log,
            token_resolver=self._token_resolver,
            state_store=self._state_store,
        )
        for sub_type, callback in self._callbacks.items():
            listener.set_callback(sub_type, callback)
        self._listeners.append(listener)
        return listener

    def _listeners_with_capacity(self, *, ready_only: bool) -> list[EventSubWSListener]:
        listeners: list[EventSubWSListener] = []
        for listener in self._active_listeners():
            if ready_only and not listener.is_ready:
                continue
            if listener.has_capacity:
                listeners.append(listener)
        return listeners

    def _pick_listener_with_capacity(self, *, ready_only: bool) -> EventSubWSListener | None:
        listeners = self._listeners_with_capacity(ready_only=ready_only)
        if listeners:
            return listeners[0]
        if ready_only or len(self._listeners) >= self._max_transports:
            return None
        return self._create_listener()

    def add_subscription(
        self,
        sub_type: str,
        broadcaster_id: str,
        condition: dict | None = None,
    ) -> bool:
        listener = self._pick_listener_with_capacity(ready_only=False)
        if listener is None:
            self.log.error(
                "EventSub WS: Keine freie Transport-Kapazität für %s von %s",
                sub_type,
                broadcaster_id,
            )
            return False
        listener.add_subscription(sub_type, broadcaster_id, condition)
        return True

    async def add_subscription_dynamic(
        self,
        sub_type: str,
        broadcaster_id: str,
        condition: dict | None = None,
        oauth_token: str | None = None,
    ) -> bool:
        attempted_ready_transports = 0
        for listener in self._listeners_with_capacity(ready_only=True):
            attempted_ready_transports += 1
            if await listener.add_subscription_dynamic(
                sub_type,
                broadcaster_id,
                condition=condition,
                oauth_token=oauth_token,
            ):
                return True
            self.log.warning(
                "EventSub WS: Dynamische Subscription %s für %s auf bestehendem Transport fehlgeschlagen; versuche weiteren Transport.",
                sub_type,
                broadcaster_id,
            )

        if len(self._listeners) >= self._max_transports:
            self.log.error(
                "EventSub WS: Keine freie Transport-Kapazität für dynamische Subscription %s von %s nach %d Transport-Versuchen",
                sub_type,
                broadcaster_id,
                attempted_ready_transports,
            )
            return False

        if not self._run_started:
            self.log.error(
                "EventSub WS: Listener-Pool läuft nicht; dynamische Subscription %s für %s nicht möglich",
                sub_type,
                broadcaster_id,
            )
            return False

        listener = self._create_listener()
        self._start_listener_task(listener)
        if not await listener.wait_until_ready(timeout=8.0, poll_interval=0.1):
            self.log.error(
                "EventSub WS: Neuer Transport wurde für dynamische Subscription %s von %s nicht rechtzeitig bereit",
                sub_type,
                broadcaster_id,
            )
            return False
        return await listener.add_subscription_dynamic(
            sub_type,
            broadcaster_id,
            condition=condition,
            oauth_token=oauth_token,
        )

    async def wait_until_ready(self, timeout: float = 8.0, poll_interval: float = 0.1) -> bool:
        loop = asyncio.get_running_loop()
        deadline = loop.time() + max(0.0, timeout)
        while loop.time() < deadline:
            if self.is_ready:
                return True
            if self.is_failed or self._stop:
                return False
            await asyncio.sleep(max(0.01, poll_interval))
        return self.is_ready

    async def wait_until_initial_registration(
        self,
        timeout: float = 8.0,
        poll_interval: float = 0.1,
    ) -> bool:
        listeners = self._active_listeners()
        if not listeners:
            return False
        loop = asyncio.get_running_loop()
        deadline = loop.time() + max(0.0, timeout)
        while loop.time() < deadline:
            listeners = self._active_listeners()
            if not listeners:
                return False
            if all(
                listener.initial_registration_complete or listener.is_failed
                for listener in listeners
            ):
                return True
            if self.is_failed or self._stop:
                return False
            await asyncio.sleep(max(0.01, poll_interval))
        listeners = self._active_listeners()
        if not listeners:
            return False
        return all(
            listener.initial_registration_complete or listener.is_failed
            for listener in listeners
        )

    def get_tracked_subscriptions(self) -> list[dict[str, object]]:
        rows: list[dict[str, object]] = []
        for index, listener in enumerate(self._active_listeners(), start=1):
            for subscription in listener.get_tracked_subscriptions():
                row = dict(subscription)
                row["listener_idx"] = index
                rows.append(row)
        return rows

    def get_capacity_rows(self) -> list[dict[str, int]]:
        rows: list[dict[str, int]] = []
        for index, listener in enumerate(self._active_listeners(), start=1):
            registered_count = int(
                getattr(listener, "registered_subscription_count", listener.subscription_count) or 0
            )
            rows.append(
                {
                    "idx": index,
                    "ready": 1 if listener.is_ready else 0,
                    "failed": 1 if listener.is_failed else 0,
                    "subscriptions": registered_count,
                    "free_slots": max(
                        0,
                        MAX_SUBSCRIPTIONS_PER_TRANSPORT - registered_count,
                    ),
                }
            )
        return rows

    def has_registered_subscription(
        self,
        sub_type: str,
        broadcaster_id: str,
        condition: dict | None = None,
    ) -> bool:
        return any(
            listener.has_registered_subscription(sub_type, broadcaster_id, condition)
            for listener in self._active_listeners()
        )

    def is_subscription_ready(
        self,
        sub_type: str,
        broadcaster_id: str,
        condition: dict | None = None,
    ) -> bool:
        return any(
            listener.is_subscription_ready(sub_type, broadcaster_id, condition)
            for listener in self._active_listeners()
        )

    def _start_listener_task(self, listener: EventSubWSListener) -> asyncio.Task[None]:
        existing = self._listener_tasks.get(listener)
        if existing is not None:
            return existing

        task = asyncio.create_task(
            listener.run(),
            name=f"eventsub.ws.transport.{len(self._listener_tasks) + 1}",
        )

        def _notify_state_change(completed: asyncio.Task[None]) -> None:
            del completed
            self._state_changed.set()

        task.add_done_callback(_notify_state_change)
        self._listener_tasks[listener] = task
        self._state_changed.set()
        return task

    async def run(self) -> None:
        self._stop = False
        self._run_started = True
        for listener in self._active_listeners():
            self._start_listener_task(listener)

        if not self._listener_tasks:
            self.log.debug("EventSub WS: Listener-Pool ohne Transport-Aufgaben gestartet.")
            self._run_started = False
            return

        try:
            while self._listener_tasks:
                state_waiter = asyncio.create_task(self._state_changed.wait())
                active_tasks = list(self._listener_tasks.values())
                done, pending = await asyncio.wait(
                    [state_waiter, *active_tasks],
                    return_when=asyncio.FIRST_COMPLETED,
                )

                if state_waiter in done:
                    self._state_changed.clear()
                else:
                    state_waiter.cancel()
                    with suppress(asyncio.CancelledError):
                        await state_waiter

                self._finalize_completed_listener_tasks()

                if self._stop and not self._listener_tasks:
                    break
        finally:
            self._run_started = False
