"""_EventSubMixin – EventSub capacity and listener management."""
from __future__ import annotations

import asyncio
import json
import os
import time
from contextlib import suppress
from datetime import UTC, datetime, timedelta
from typing import Any

import aiohttp

from .. import storage
from ..core.constants import log
from .eventsub_core_callbacks import register_core_eventsub_callbacks
from .eventsub_processing_inbox import EventSubProcessingInboxRuntime
from .eventsub_state_store import (
    EVENTSUB_STATE_KIND_BUSINESS_EFFECT,
    EVENTSUB_STATE_KIND_OFFLINE_THROTTLE,
    EventSubStateStore,
)
from .eventsub_ws import EventSubWSListener
from .eventsub_ws_pool import EventSubWSListenerPool

_EVENTSUB_LIVE_STATE_COLUMNS = (
    "is_live",
    "last_seen_at",
    "last_title",
    "last_game",
    "last_viewer_count",
    "last_stream_id",
    "last_started_at",
    "had_deadlock_in_session",
    "last_deadlock_seen_at",
)
_EVENTSUB_RETRY_MIN_DELAY_SECONDS = 5.0
_EVENTSUB_RETRY_MAX_DELAY_SECONDS = 60.0
_EVENTSUB_RETRY_BACKOFF_RESET_SECONDS = 15.0
_EVENTSUB_IDLE_RETRY_REASONS = frozenset({"no_streamers", "ws_capacity_exhausted"})
_EVENTSUB_WEBHOOK_REQUIRED_SUB_TYPES = ("stream.online", "stream.offline")
_EVENTSUB_WEBHOOK_CORE_SUB_TYPES = (*_EVENTSUB_WEBHOOK_REQUIRED_SUB_TYPES, "channel.update")


class _EventSubMixin:

    def _get_eventsub_processing_inbox(self) -> EventSubProcessingInboxRuntime:
        runtime = getattr(self, "_eventsub_processing_inbox", None)
        if isinstance(runtime, EventSubProcessingInboxRuntime):
            return runtime
        runtime = EventSubProcessingInboxRuntime(
            handler=self._process_eventsub_processing_record,
            on_dead_letter=self._handle_eventsub_processing_dead_letter,
            logger=log,
        )
        self._eventsub_processing_inbox = runtime
        return runtime

    async def _ensure_eventsub_processing_inbox_started(self) -> None:
        await self._get_eventsub_processing_inbox().start()

    async def _internal_eventsub_processing_debug(self, *, limit: int = 20) -> dict[str, Any]:
        return await self._get_eventsub_processing_inbox().snapshot(limit=max(1, int(limit)))

    async def _internal_eventsub_processing_requeue(self, work_id: str) -> dict[str, Any]:
        normalized_work_id = str(work_id or "").strip()
        if not normalized_work_id:
            raise ValueError("work_id is required")
        requeued = await self._get_eventsub_processing_inbox().requeue_dead_letter(
            work_id=normalized_work_id
        )
        if not requeued:
            raise ValueError("unknown work_id")
        return {"ok": True, "workId": normalized_work_id, "requeued": True}

    async def _handle_eventsub_processing_dead_letter(self, payload: dict[str, Any]) -> None:
        work_type = str(payload.get("work_type") or "").strip().lower()
        work_id = str(payload.get("work_id") or "").strip() or "n/a"
        message_id = str(payload.get("message_id") or "").strip() or "n/a"
        attempt_count = int(payload.get("attempt_count") or 0)
        last_error = str(payload.get("last_error") or "").strip() or "unknown"
        log.critical(
            "EventSub processing dead-lettered work_type=%s work_id=%s msg_id=%s attempts=%d error=%s",
            work_type or "unknown",
            work_id,
            message_id,
            attempt_count,
            last_error,
        )
        self._eventsub_retry_reason = "processing_dead_letter"
        self._request_eventsub_supervisor_wakeup("processing_dead_letter")

    async def _process_eventsub_processing_record(
        self,
        work_type: str,
        payload: dict[str, Any],
    ) -> None:
        normalized_work_type = str(work_type or "").strip().lower()
        message_id = str(payload.get("message_id") or "").strip() or None
        if normalized_work_type == "stream.offline":
            await self._on_eventsub_stream_offline(
                str(payload.get("broadcaster_id") or "").strip(),
                str(payload.get("broadcaster_login") or "").strip() or None,
                message_id=message_id,
                allow_scheduled_refresh=False,
            )
            return
        if normalized_work_type == "stream.online":
            await self._handle_stream_online(
                str(payload.get("broadcaster_id") or "").strip(),
                str(payload.get("broadcaster_login") or "").strip(),
                dict(payload.get("event") or {}),
                message_id=message_id,
            )
            return
        if normalized_work_type == "stream.online.followups":
            await self._run_stream_online_followups(
                broadcaster_user_id=str(payload.get("broadcaster_user_id") or "").strip(),
                broadcaster_login=str(payload.get("broadcaster_login") or "").strip(),
                login_value=str(payload.get("login_value") or "").strip().lower(),
                defer_refresh=False,
                message_id=message_id,
            )
            return
        if normalized_work_type == "channel.update":
            await self._handle_channel_update(
                str(payload.get("broadcaster_id") or "").strip(),
                dict(payload.get("event") or {}),
                message_id=message_id,
                allow_background_refresh=False,
            )
            return
        if normalized_work_type == "channel.raid":
            raid_bot = getattr(self, "_raid_bot", None)
            if not raid_bot:
                log.debug("EventSub: Raid-Bot nicht verfügbar für persistente Verarbeitung")
                return
            event = dict(payload.get("event") or {})
            to_broadcaster_id = str(payload.get("to_broadcaster_id") or "").strip()
            to_broadcaster_login = str(payload.get("to_broadcaster_login") or "").strip()
            from_login = str(event.get("from_broadcaster_user_login") or "").strip().lower()
            from_broadcaster_id = str(event.get("from_broadcaster_user_id") or "").strip() or None
            viewer_count = int(event.get("viewers") or 0)
            if not to_broadcaster_id or not from_login:
                raise RuntimeError("invalid channel.raid processing payload")
            await self._run_eventsub_business_effect_once(
                message_id=message_id,
                effect_name="channel_raid_arrival",
                coro_factory=lambda: raid_bot.on_raid_arrival(
                    to_broadcaster_id=to_broadcaster_id,
                    to_broadcaster_login=to_broadcaster_login,
                    from_broadcaster_login=from_login,
                    from_broadcaster_id=from_broadcaster_id,
                    viewer_count=viewer_count,
                ),
            )
            return
        raise RuntimeError(f"unknown eventsub processing work_type: {normalized_work_type}")

    def _get_eventsub_state_store(self) -> EventSubStateStore:
        store = getattr(self, "_eventsub_state_store", None)
        if isinstance(store, EventSubStateStore):
            return store
        store = EventSubStateStore(logger=log)
        self._eventsub_state_store = store
        return store

    def _persistent_eventsub_guards_enabled(self) -> bool:
        return bool(getattr(self, "_eventsub_enable_persistent_guards", False))

    async def _run_eventsub_business_effect_once(
        self,
        *,
        message_id: str | None,
        effect_name: str,
        coro_factory: Any,
        ttl_seconds: float = 7 * 24 * 3600.0,
    ) -> bool:
        normalized_message_id = str(message_id or "").strip()
        normalized_effect_name = str(effect_name or "").strip().lower()
        if not normalized_effect_name:
            raise ValueError("effect_name is required")
        if not normalized_message_id:
            await coro_factory()
            return True
        guard_key = f"{normalized_effect_name}:{normalized_message_id}"
        claimed = self._get_eventsub_state_store().claim(
            EVENTSUB_STATE_KIND_BUSINESS_EFFECT,
            guard_key,
            ttl_seconds=ttl_seconds,
        )
        if not claimed:
            return False
        try:
            await coro_factory()
        except Exception:
            self._get_eventsub_state_store().release(
                EVENTSUB_STATE_KIND_BUSINESS_EFFECT,
                guard_key,
            )
            raise
        return True

    def _eventsub_retry_delay_seconds(self, consecutive_failures: int) -> float:
        failures = max(0, int(consecutive_failures))
        retry_delay = _EVENTSUB_RETRY_MIN_DELAY_SECONDS * (2**failures)
        return min(_EVENTSUB_RETRY_MAX_DELAY_SECONDS, retry_delay)

    def _get_eventsub_supervisor_wakeup(self) -> asyncio.Event:
        wakeup = getattr(self, "_eventsub_supervisor_wakeup", None)
        if not isinstance(wakeup, asyncio.Event):
            wakeup = asyncio.Event()
            self._eventsub_supervisor_wakeup = wakeup
        return wakeup

    def _request_eventsub_supervisor_wakeup(self, reason: str) -> None:
        wakeup = getattr(self, "_eventsub_supervisor_wakeup", None)
        if not isinstance(wakeup, asyncio.Event):
            return
        self._eventsub_supervisor_last_wakeup_reason = str(reason or "").strip() or None
        wakeup.set()

    @staticmethod
    def _is_eventsub_webhook_startup_healthy(
        raid_enabled_streamers: list[dict[str, str]],
        *,
        startup_coverage: dict[str, set[str]],
    ) -> tuple[bool, list[dict[str, Any]]]:
        missing_critical: list[dict[str, Any]] = []
        for entry in raid_enabled_streamers:
            broadcaster_id = str(entry.get("twitch_user_id") or "").strip()
            if not broadcaster_id:
                continue
            coverage = {
                str(sub_type or "").strip().lower()
                for sub_type in startup_coverage.get(broadcaster_id, set())
                if str(sub_type or "").strip()
            }
            missing_required = [
                sub_type
                for sub_type in _EVENTSUB_WEBHOOK_REQUIRED_SUB_TYPES
                if sub_type not in coverage
            ]
            if not missing_required:
                continue
            missing_critical.append(
                {
                    "broadcaster_user_id": broadcaster_id,
                    "broadcaster_login": str(entry.get("twitch_login") or "").strip().lower(),
                    "missing_required": missing_required,
                    "present": sorted(coverage),
                }
            )
        return not missing_critical, missing_critical

    async def _run_eventsub_listener_supervisor(self) -> None:
        """Betreibt EventSub mit Retry-Logik bis ein stabiler Modus aktiv ist."""
        wakeup = self._get_eventsub_supervisor_wakeup()
        current_task = asyncio.current_task()
        if current_task is not None:
            self._eventsub_supervisor_task = current_task
        consecutive_failures = 0
        while True:
            cycle_started = time.monotonic()
            try:
                stable = bool(await self._start_eventsub_listener())
            except asyncio.CancelledError:
                raise
            except Exception:
                stable = False
                self._eventsub_started = False
                self._set_eventsub_webhook_notification_dispatch(active=False)
                self._eventsub_retry_reason = "listener_exception"
                log.exception("EventSub Supervisor: Startzyklus fehlgeschlagen")

            if stable:
                return

            reason = (
                str(getattr(self, "_eventsub_retry_reason", "") or "").strip()
                or "listener_retry"
            )
            if reason in _EVENTSUB_IDLE_RETRY_REASONS:
                log.info(
                    "EventSub Supervisor: Kein aktives Roster, warte auf Wakeup (reason=%s)",
                    reason,
                )
                if wakeup.is_set():
                    wakeup.clear()
                    consecutive_failures = 0
                    continue
                await wakeup.wait()
                wakeup.clear()
                consecutive_failures = 0
                continue

            cycle_runtime = time.monotonic() - cycle_started
            if cycle_runtime >= _EVENTSUB_RETRY_BACKOFF_RESET_SECONDS:
                consecutive_failures = 0
            retry_delay = self._eventsub_retry_delay_seconds(consecutive_failures)
            consecutive_failures += 1
            log.info(
                "EventSub Supervisor: Neuer Startversuch in %.1fs (reason=%s, consecutive_failures=%d)",
                retry_delay,
                reason,
                consecutive_failures,
            )

            if wakeup.is_set():
                wakeup.clear()
                consecutive_failures = 0
                continue
            try:
                await asyncio.wait_for(wakeup.wait(), timeout=retry_delay)
            except asyncio.TimeoutError:
                continue
            wakeup.clear()
            consecutive_failures = 0

    def _ensure_eventsub_supervisor_running(self, reason: str) -> asyncio.Task[Any] | None:
        """Wake or spawn the supervisor when EventSub should resume from a stopped state."""
        task = getattr(self, "_eventsub_supervisor_task", None)
        if isinstance(task, asyncio.Task) and not task.done():
            self._request_eventsub_supervisor_wakeup(reason)
            return task
        if bool(getattr(self, "_eventsub_started", False)):
            return task if isinstance(task, asyncio.Task) else None
        runner = getattr(self, "_run_eventsub_listener_supervisor", None)
        if not callable(runner):
            return None
        spawn = getattr(self, "_spawn_bg_task", None)
        if callable(spawn):
            task = spawn(runner(), "twitch.eventsub")
        else:
            try:
                task = asyncio.create_task(runner(), name="twitch.eventsub")
            except RuntimeError:
                log.debug(
                    "EventSub Supervisor: Konnte Supervisor ohne laufenden Event Loop nicht starten",
                    exc_info=True,
                )
                return None
        if isinstance(task, asyncio.Task):
            self._eventsub_supervisor_task = task
            return task
        return None

    def _eventsub_capacity_sample_interval_seconds(self) -> int:
        raw = (os.getenv("TWITCH_EVENTSUB_CAPACITY_SAMPLE_SECONDS") or "").strip()
        default_value = 300
        if not raw:
            return default_value
        try:
            value = int(raw)
        except ValueError:
            return default_value
        return max(30, min(3600, value))

    def _eventsub_capacity_retention_days(self) -> int:
        raw = (os.getenv("TWITCH_EVENTSUB_CAPACITY_RETENTION_DAYS") or "").strip()
        default_value = 45
        if not raw:
            return default_value
        try:
            value = int(raw)
        except ValueError:
            return default_value
        return max(7, min(365, value))

    @staticmethod
    def _eventsub_target_user_id(condition: dict[str, Any] | None, *, fallback: str = "") -> str:
        condition_map = condition if isinstance(condition, dict) else {}
        for key in (
            "broadcaster_user_id",
            "to_broadcaster_user_id",
            "from_broadcaster_user_id",
            "user_id",
        ):
            value = str(condition_map.get(key) or "").strip()
            if value:
                return value
        return str(fallback or "").strip()

    @staticmethod
    def _eventsub_is_already_exists_error(exc: BaseException) -> bool:
        status = int(getattr(exc, "status", 0) or 0)
        if status != 409:
            return False
        message = f"{getattr(exc, 'message', '')} {exc}".strip().lower()
        return not message or "already exists" in message

    async def _create_eventsub_webhook_subscription(
        self,
        *,
        sub_type: str,
        condition: dict[str, str],
        webhook_url: str,
        secret: str,
        version: str = "1",
        oauth_token: str | None = None,
    ) -> tuple[bool, bool]:
        try:
            result = await self.api.subscribe_eventsub_webhook(
                sub_type=sub_type,
                condition=condition,
                webhook_url=webhook_url,
                secret=secret,
                version=version,
                oauth_token=oauth_token,
            )
        except Exception as exc:
            if self._eventsub_is_already_exists_error(exc):
                return True, True
            raise
        already_exists = bool(isinstance(result, dict) and result.get("already_exists"))
        return bool(result), already_exists

    def _resolve_twitch_logins_by_user_id(self, user_ids: list[str]) -> dict[str, str]:
        unique_ids: list[str] = []
        seen: set[str] = set()
        for raw in user_ids:
            value = str(raw or "").strip()
            if not value or value in seen:
                continue
            seen.add(value)
            unique_ids.append(value)

        if not unique_ids:
            return {}

        wanted_ids = set(unique_ids)
        try:
            with storage.readonly_connection() as c:
                rows = c.execute(
                    """
                    SELECT twitch_user_id, twitch_login
                    FROM twitch_streamer_identities
                    WHERE twitch_login IS NOT NULL
                    """
                ).fetchall()
            out: dict[str, str] = {}
            for row in rows:
                uid = str(row["twitch_user_id"] if hasattr(row, "keys") else row[0]).strip()
                if uid not in wanted_ids:
                    continue
                login = str(row["twitch_login"] if hasattr(row, "keys") else row[1]).strip().lower()
                if uid and login:
                    out[uid] = login
                    if len(out) >= len(wanted_ids):
                        break
            return out
        except Exception:
            log.debug("EventSub: konnte twitch_login Mapping nicht laden", exc_info=True)
            return {}

    def _collect_eventsub_capacity_snapshot(self, *, reason: str) -> dict[str, Any]:
        ws_listener = self._get_eventsub_ws_listener()
        active_subscriptions: list[dict[str, Any]] = []
        sub_type_counts: dict[str, int] = {}

        if ws_listener is not None:
            tracked_subs = ws_listener.get_tracked_subscriptions()
            for sub in tracked_subs:
                condition = sub.get("condition") if isinstance(sub.get("condition"), dict) else {}
                broadcaster_user_id = str(sub.get("broadcaster_id") or "").strip()
                listener_idx = int(sub.get("listener_idx") or 1)
                target_user_id = self._eventsub_target_user_id(
                    condition,
                    fallback=broadcaster_user_id,
                )
                sub_type = str(sub.get("type") or "").strip().lower() or "unknown"
                active_subscriptions.append(
                    {
                        "listener_idx": listener_idx,
                        "sub_type": sub_type,
                        "broadcaster_user_id": broadcaster_user_id,
                        "target_user_id": target_user_id,
                        "condition": condition,
                    }
                )
                sub_type_counts[sub_type] = int(sub_type_counts.get(sub_type, 0)) + 1

            used_slots = len(active_subscriptions)
            get_capacity_rows = getattr(ws_listener, "get_capacity_rows", None)
            if callable(get_capacity_rows):
                listener_rows = list(get_capacity_rows())
            else:
                registered_count = int(
                    getattr(
                        ws_listener,
                        "registered_subscription_count",
                        ws_listener.subscription_count,
                    )
                    or 0
                )
                listener_rows = [
                    {
                        "idx": 1,
                        "ready": 1 if ws_listener.is_ready else 0,
                        "failed": 1 if ws_listener.is_failed else 0,
                        "subscriptions": registered_count,
                        "free_slots": max(0, 10 - registered_count),
                    }
                ]
            total_slots = max(10, len(listener_rows) * 10)
            headroom_slots = max(0, total_slots - used_slots)
            utilization_pct = (
                (float(used_slots) / float(total_slots) * 100.0) if total_slots > 0 else 0.0
            )
            listener_count = len(listener_rows)
            ready_count = sum(int(row.get("ready") or 0) for row in listener_rows)
            failed_count = sum(int(row.get("failed") or 0) for row in listener_rows)
            listeners_at_limit = sum(
                1 for row in listener_rows if int(row.get("free_slots") or 0) <= 0
            )
        else:
            tracked_subs: list[dict[str, Any]] = list(
                getattr(self, "_eventsub_webhook_active_subs", []) or []
            )
            for sub in tracked_subs:
                sub_type = str(sub.get("sub_type") or "").strip().lower() or "unknown"
                broadcaster_user_id = str(sub.get("broadcaster_user_id") or "").strip()
                active_subscriptions.append(
                    {
                        "listener_idx": 1,
                        "sub_type": sub_type,
                        "broadcaster_user_id": broadcaster_user_id,
                        "target_user_id": broadcaster_user_id,
                        "condition": {"broadcaster_user_id": broadcaster_user_id},
                    }
                )
                sub_type_counts[sub_type] = int(sub_type_counts.get(sub_type, 0)) + 1

            used_slots = len(active_subscriptions)
            total_slots = 10000
            headroom_slots = max(0, total_slots - used_slots)
            utilization_pct = (
                (float(used_slots) / float(total_slots) * 100.0) if total_slots > 0 else 0.0
            )
            listener_rows = [
                {
                    "idx": 1,
                    "ready": 1,
                    "failed": 0,
                    "subscriptions": used_slots,
                    "free_slots": headroom_slots,
                }
            ]
            listener_count = 1
            ready_count = 1
            failed_count = 0
            listeners_at_limit = 0

        login_map = self._resolve_twitch_logins_by_user_id(
            [str(row.get("target_user_id") or "") for row in active_subscriptions]
        )

        for row in active_subscriptions:
            target_user_id = str(row.get("target_user_id") or "").strip()
            target_login = login_map.get(target_user_id)
            if not target_login:
                condition = row.get("condition") if isinstance(row.get("condition"), dict) else {}
                target_login = (
                    str(condition.get("broadcaster_user_login") or "").strip().lower()
                    or str(condition.get("to_broadcaster_user_login") or "").strip().lower()
                    or None
                )
            row["target_login"] = target_login

        channel_map: dict[str, dict[str, Any]] = {}
        for row in active_subscriptions:
            target_user_id = str(row.get("target_user_id") or "").strip()
            if not target_user_id:
                continue
            sub_type = str(row.get("sub_type") or "").strip().lower() or "unknown"
            target_login = str(row.get("target_login") or "").strip().lower() or None
            channel_entry = channel_map.setdefault(
                target_user_id,
                {
                    "twitch_user_id": target_user_id,
                    "twitch_login": target_login,
                    "subscription_count": 0,
                    "sub_types": set(),
                },
            )
            channel_entry["subscription_count"] = (
                int(channel_entry.get("subscription_count") or 0) + 1
            )
            if target_login and not channel_entry.get("twitch_login"):
                channel_entry["twitch_login"] = target_login
            channel_entry["sub_types"].add(sub_type)

        subscription_channels = sorted(
            [
                {
                    "twitch_user_id": str(entry.get("twitch_user_id") or ""),
                    "twitch_login": (str(entry.get("twitch_login") or "").strip().lower() or None),
                    "subscription_count": int(entry.get("subscription_count") or 0),
                    "sub_types": sorted(
                        str(sub_type) for sub_type in entry.get("sub_types", set())
                    ),
                }
                for entry in channel_map.values()
            ],
            key=lambda entry: (
                -int(entry.get("subscription_count") or 0),
                str(entry.get("twitch_login") or ""),
                str(entry.get("twitch_user_id") or ""),
            ),
        )

        subscription_types = [
            {"sub_type": sub_type, "count": int(count)}
            for sub_type, count in sorted(
                sub_type_counts.items(),
                key=lambda item: (-int(item[1] or 0), str(item[0])),
            )
        ]

        return {
            "ts_utc": datetime.now(UTC).isoformat(timespec="seconds"),
            "reason": (reason or "unknown").strip()[:64],
            "listener_count": listener_count,
            "ready_listeners": ready_count,
            "failed_listeners": failed_count,
            "used_slots": used_slots,
            "total_slots": total_slots,
            "headroom_slots": headroom_slots,
            "listeners_at_limit": listeners_at_limit,
            "utilization_pct": round(utilization_pct, 2),
            "listeners": listener_rows,
            "subscription_count": len(active_subscriptions),
            "subscriptions": active_subscriptions,
            "subscription_types": subscription_types,
            "subscription_channels": subscription_channels,
        }

    async def _record_eventsub_capacity_snapshot(self, reason: str, *, force: bool = False) -> None:
        now_monotonic = time.monotonic()
        interval = self._eventsub_capacity_sample_interval_seconds()
        last_snapshot = float(getattr(self, "_eventsub_capacity_last_snapshot", 0.0) or 0.0)
        if not force and last_snapshot and (now_monotonic - last_snapshot) < interval:
            return

        snapshot = self._collect_eventsub_capacity_snapshot(reason=reason)
        listeners_json = json.dumps(
            snapshot.get("listeners", []),
            ensure_ascii=True,
            separators=(",", ":"),
        )

        try:
            with storage.transaction() as c:
                c.execute(
                    """
                    INSERT INTO twitch_eventsub_capacity_snapshot (
                        ts_utc, trigger_reason, listener_count, ready_listeners, failed_listeners,
                        used_slots, total_slots, headroom_slots, listeners_at_limit, utilization_pct, listeners_json
                    ) VALUES (%s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s)
                    """,
                    (
                        snapshot.get("ts_utc"),
                        snapshot.get("reason"),
                        int(snapshot.get("listener_count") or 0),
                        int(snapshot.get("ready_listeners") or 0),
                        int(snapshot.get("failed_listeners") or 0),
                        int(snapshot.get("used_slots") or 0),
                        int(snapshot.get("total_slots") or 0),
                        int(snapshot.get("headroom_slots") or 0),
                        int(snapshot.get("listeners_at_limit") or 0),
                        float(snapshot.get("utilization_pct") or 0.0),
                        listeners_json,
                    ),
                )

                last_cleanup = float(getattr(self, "_eventsub_capacity_last_cleanup", 0.0) or 0.0)
                if (now_monotonic - last_cleanup) >= 3600:
                    retention_days = self._eventsub_capacity_retention_days()
                    cutoff = datetime.now(UTC) - timedelta(days=retention_days)
                    c.execute(
                        "DELETE FROM twitch_eventsub_capacity_snapshot WHERE ts_utc < %s",
                        (cutoff.isoformat(timespec="seconds"),),
                    )
                    self._eventsub_capacity_last_cleanup = now_monotonic

            self._eventsub_capacity_last_snapshot = now_monotonic
            utilization_pct = float(snapshot.get("utilization_pct") or 0.0)
            if utilization_pct >= 90.0:
                last_warn = float(getattr(self, "_eventsub_capacity_last_warn", 0.0) or 0.0)
                if (now_monotonic - last_warn) >= 600:
                    log.warning(
                        "EventSub Capacity hoch: %.1f%% (%d/%d Slots, %d Listener, Trigger=%s)",
                        utilization_pct,
                        int(snapshot.get("used_slots") or 0),
                        int(snapshot.get("total_slots") or 0),
                        int(snapshot.get("listener_count") or 0),
                        str(snapshot.get("reason") or "unknown"),
                    )
                    self._eventsub_capacity_last_warn = now_monotonic
        except Exception:
            log.debug("EventSub: konnte Capacity-Snapshot nicht speichern", exc_info=True)

    async def _get_eventsub_capacity_overview(self, *, hours: int = 24) -> dict[str, Any]:
        hours = max(1, min(168, int(hours or 24)))
        lookback_interval = f"{hours} hours"

        try:
            with storage.readonly_connection() as c:
                rows = c.execute(
                    """
                    SELECT ts_utc, trigger_reason, listener_count, ready_listeners, failed_listeners,
                           used_slots, total_slots, headroom_slots, listeners_at_limit, utilization_pct
                      FROM twitch_eventsub_capacity_snapshot
                     WHERE ts_utc >= NOW() - (%s::interval)
                     ORDER BY ts_utc ASC
                    """,
                    (lookback_interval,),
                ).fetchall()
                hourly_rows = c.execute(
                    """
                    SELECT EXTRACT(HOUR FROM ts_utc AT TIME ZONE 'UTC')::int AS hour,
                           COUNT(*) AS samples,
                           AVG(utilization_pct) AS avg_utilization_pct,
                           MAX(utilization_pct) AS max_utilization_pct,
                           AVG(used_slots) AS avg_used_slots,
                           MAX(used_slots) AS max_used_slots,
                           AVG(listener_count) AS avg_listener_count,
                           MAX(listener_count) AS max_listener_count
                      FROM twitch_eventsub_capacity_snapshot
                     WHERE ts_utc >= NOW() - (%s::interval)
                     GROUP BY hour
                     ORDER BY hour ASC
                    """,
                    (lookback_interval,),
                ).fetchall()
                reason_rows = c.execute(
                    """
                    SELECT trigger_reason,
                           COUNT(*) AS samples,
                           MAX(utilization_pct) AS peak_utilization_pct
                      FROM twitch_eventsub_capacity_snapshot
                     WHERE ts_utc >= NOW() - (%s::interval)
                     GROUP BY trigger_reason
                     ORDER BY samples DESC, trigger_reason ASC
                    """,
                    (lookback_interval,),
                ).fetchall()
        except Exception:
            log.debug("EventSub: konnte Capacity-Overview nicht laden", exc_info=True)
            rows = []
            hourly_rows = []
            reason_rows = []

        utilization_values: list[float] = []
        used_slot_values: list[float] = []
        listener_count_values: list[float] = []
        ready_count_values: list[float] = []
        failed_count_values: list[float] = []
        for row in rows:
            if hasattr(row, "keys"):
                utilization_values.append(float(row["utilization_pct"] or 0.0))
                used_slot_values.append(float(row["used_slots"] or 0.0))
                listener_count_values.append(float(row["listener_count"] or 0.0))
                ready_count_values.append(float(row["ready_listeners"] or 0.0))
                failed_count_values.append(float(row["failed_listeners"] or 0.0))
            else:
                utilization_values.append(float(row[9] or 0.0))
                used_slot_values.append(float(row[5] or 0.0))
                listener_count_values.append(float(row[2] or 0.0))
                ready_count_values.append(float(row[3] or 0.0))
                failed_count_values.append(float(row[4] or 0.0))

        def _avg(values: list[float]) -> float:
            return (sum(values) / len(values)) if values else 0.0

        def _max(values: list[float]) -> float:
            return max(values) if values else 0.0

        def _p95(values: list[float]) -> float:
            if not values:
                return 0.0
            ordered = sorted(values)
            idx = int(round((len(ordered) - 1) * 0.95))
            idx = max(0, min(len(ordered) - 1, idx))
            return ordered[idx]

        current_snapshot = self._collect_eventsub_capacity_snapshot(reason="current")

        hourly: list[dict[str, Any]] = []
        for row in hourly_rows:
            if hasattr(row, "keys"):
                hourly.append(
                    {
                        "hour": int(row["hour"] or 0),
                        "samples": int(row["samples"] or 0),
                        "avg_utilization_pct": float(row["avg_utilization_pct"] or 0.0),
                        "max_utilization_pct": float(row["max_utilization_pct"] or 0.0),
                        "avg_used_slots": float(row["avg_used_slots"] or 0.0),
                        "max_used_slots": int(row["max_used_slots"] or 0),
                        "avg_listener_count": float(row["avg_listener_count"] or 0.0),
                        "max_listener_count": int(row["max_listener_count"] or 0),
                    }
                )
            else:
                hourly.append(
                    {
                        "hour": int(row[0] or 0),
                        "samples": int(row[1] or 0),
                        "avg_utilization_pct": float(row[2] or 0.0),
                        "max_utilization_pct": float(row[3] or 0.0),
                        "avg_used_slots": float(row[4] or 0.0),
                        "max_used_slots": int(row[5] or 0),
                        "avg_listener_count": float(row[6] or 0.0),
                        "max_listener_count": int(row[7] or 0),
                    }
                )

        reasons: list[dict[str, Any]] = []
        for row in reason_rows:
            if hasattr(row, "keys"):
                reasons.append(
                    {
                        "reason": str(row["trigger_reason"] or ""),
                        "samples": int(row["samples"] or 0),
                        "peak_utilization_pct": float(row["peak_utilization_pct"] or 0.0),
                    }
                )
            else:
                reasons.append(
                    {
                        "reason": str(row[0] or ""),
                        "samples": int(row[1] or 0),
                        "peak_utilization_pct": float(row[2] or 0.0),
                    }
                )

        last_snapshot_at = None
        if rows:
            last_row = rows[-1]
            last_snapshot_at = (
                str(last_row["ts_utc"]) if hasattr(last_row, "keys") else str(last_row[0])
            )

        return {
            "window_hours": hours,
            "samples": len(rows),
            "last_snapshot_at": last_snapshot_at,
            "avg_utilization_pct": round(_avg(utilization_values), 2),
            "p95_utilization_pct": round(_p95(utilization_values), 2),
            "max_utilization_pct": round(_max(utilization_values), 2),
            "avg_used_slots": round(_avg(used_slot_values), 2),
            "max_used_slots": int(round(_max(used_slot_values))),
            "avg_listener_count": round(_avg(listener_count_values), 2),
            "max_listener_count": int(round(_max(listener_count_values))),
            "avg_ready_listeners": round(_avg(ready_count_values), 2),
            "max_failed_listeners": int(round(_max(failed_count_values))),
            "hourly": hourly,
            "reasons": reasons,
            "active_subscriptions": current_snapshot.get("subscriptions", []),
            "active_subscription_types": current_snapshot.get("subscription_types", []),
            "active_subscription_channels": current_snapshot.get("subscription_channels", []),
            "current": {
                "ts_utc": current_snapshot.get("ts_utc"),
                "listener_count": int(current_snapshot.get("listener_count") or 0),
                "ready_listeners": int(current_snapshot.get("ready_listeners") or 0),
                "failed_listeners": int(current_snapshot.get("failed_listeners") or 0),
                "used_slots": int(current_snapshot.get("used_slots") or 0),
                "total_slots": int(current_snapshot.get("total_slots") or 0),
                "headroom_slots": int(current_snapshot.get("headroom_slots") or 0),
                "listeners_at_limit": int(current_snapshot.get("listeners_at_limit") or 0),
                "utilization_pct": float(current_snapshot.get("utilization_pct") or 0.0),
                "subscription_count": int(current_snapshot.get("subscription_count") or 0),
            },
        }

    def _get_raid_enabled_streamers_for_eventsub(self) -> list[dict[str, str]]:
        """Broadcaster-Liste für EventSub stream.offline (nur raid_bot_enabled=1)."""
        try:
            with storage.readonly_connection() as c:
                rows = c.execute(
                    """
                    SELECT twitch_user_id, twitch_login
                      FROM twitch_streamers_partner_state
                     WHERE is_partner_active = 1
                       AND COALESCE(raid_bot_enabled, 0) = 1
                       AND twitch_user_id IS NOT NULL
                       AND twitch_login IS NOT NULL
                    """
                ).fetchall()
            return [
                {
                    "twitch_user_id": str(r["twitch_user_id"] if hasattr(r, "keys") else r[0]),
                    "twitch_login": str(r["twitch_login"] if hasattr(r, "keys") else r[1]).lower(),
                }
                for r in rows
            ]
        except Exception:
            log.debug("EventSub: konnte raid_enabled Streamer nicht laden", exc_info=True)
            return []

    def _get_chat_scope_streamers_for_eventsub(self) -> list[dict[str, str]]:
        """Broadcaster mit `channel:bot`-Freigabe fuer botzentrierte Chat-Features."""
        try:
            with storage.readonly_connection() as c:
                rows = c.execute(
                    """
                    SELECT s.twitch_user_id, s.twitch_login, a.scopes
                      FROM twitch_streamers_partner_state s
                      JOIN twitch_raid_auth a ON s.twitch_user_id = a.twitch_user_id
                     WHERE s.is_partner_active = 1
                       AND s.twitch_user_id IS NOT NULL
                       AND s.twitch_login IS NOT NULL
                    """
                ).fetchall()
            out: list[dict[str, str]] = []
            seen: set[str] = set()
            for row in rows:
                user_id = str(row["twitch_user_id"] if hasattr(row, "keys") else row[0]).strip()
                login = str(row["twitch_login"] if hasattr(row, "keys") else row[1]).strip().lower()
                scopes_raw = row["scopes"] if hasattr(row, "keys") else row[2]
                scopes = [s.strip().lower() for s in (scopes_raw or "").split() if s.strip()]
                has_channel_bot_grant = "channel:bot" in scopes
                if not has_channel_bot_grant or not user_id or not login:
                    continue
                key = f"{user_id}:{login}"
                if key in seen:
                    continue
                seen.add(key)
                out.append({"twitch_user_id": user_id, "twitch_login": login})
            return out
        except Exception:
            log.debug("EventSub online: konnte Streamer-Liste nicht laden", exc_info=True)
            return []

    def _get_tracked_logins_for_eventsub(self) -> list[str]:
        """Alle bekannten Streamer-Logins (für Online-Status der Partner bei EventSub)."""
        try:
            with storage.readonly_connection() as c:
                rows = c.execute(
                    """
                    SELECT twitch_login
                    FROM twitch_streamers_partner_state
                    WHERE is_partner_active = 1
                      AND twitch_login IS NOT NULL
                    """
                ).fetchall()
            return [str(r["twitch_login"] if hasattr(r, "keys") else r[0]).lower() for r in rows]
        except Exception:
            log.debug("EventSub: konnte tracked Logins nicht laden", exc_info=True)
            return []

    async def _fetch_streams_by_logins_quick(self, logins: list[str]) -> dict[str, dict]:
        """Hol Live-Streams fœr angegebene Logins (reduziert auf einmal pro EventSub-Offline)."""
        if not getattr(self, "api", None):
            return {}
        streams_by_login: dict[str, dict] = {}
        logins = [lg for lg in logins if lg]
        if not logins:
            return {}
        for language in self._language_filter_values():
            try:
                streams = await self.api.get_streams_by_logins(logins, language=language)
            except Exception:
                label = language or "any"
                log.debug("EventSub: Streams fetch failed (language=%s)", label, exc_info=True)
                continue
            for stream in streams:
                login = (stream.get("user_login") or "").lower()
                if login:
                    streams_by_login[login] = stream
        return streams_by_login

    def _load_live_state_row(self, login_lower: str) -> dict:
        """Lädt letzten Live-State aus DB, damit EventSub-Offlines sofort Daten haben."""
        if not login_lower:
            return {}
        try:
            columns_sql = ", ".join(_EVENTSUB_LIVE_STATE_COLUMNS)
            with storage.readonly_connection() as c:
                row = c.execute(
                    f"""
                    SELECT {columns_sql}
                      FROM twitch_live_state
                     WHERE streamer_login = %s
                    """,
                    (login_lower,),
                ).fetchone()
            return dict(row) if row else {}
        except Exception:
            log.debug(
                "EventSub: konnte live_state für %s nicht laden",
                login_lower,
                exc_info=True,
            )
            return {}

    def _get_eventsub_ws_listener(self) -> EventSubWSListener | EventSubWSListenerPool | None:
        listener = getattr(self, "_eventsub_ws_listener", None)
        if isinstance(listener, (EventSubWSListener, EventSubWSListenerPool)):
            return listener
        if listener is None:
            return None
        wait_until_ready = getattr(listener, "wait_until_ready", None)
        add_dynamic = getattr(listener, "add_subscription_dynamic", None)
        if callable(wait_until_ready) and callable(add_dynamic):
            return listener
        return None

    def _has_eventsub_webhook_transport(self) -> bool:
        return bool(
            self._get_eventsub_webhook_url()
            and getattr(self, "_webhook_secret", None)
            and getattr(self, "_eventsub_webhook_handler", None)
        )

    def _resolve_eventsub_broadcaster_login(
        self,
        broadcaster_id: str,
        broadcaster_login: str | None,
    ) -> str:
        login_lower = str(broadcaster_login or "").strip().lower()
        if login_lower:
            return login_lower

        try:
            with storage.readonly_connection() as c:
                row = c.execute(
                    """
                    SELECT streamer_login
                      FROM twitch_live_state
                     WHERE twitch_user_id = %s
                     ORDER BY last_seen_at DESC
                     LIMIT 1
                    """,
                    (str(broadcaster_id or "").strip(),),
                ).fetchone()
            if row:
                return str(row["streamer_login"] if hasattr(row, "keys") else row[0]).strip().lower()
        except Exception:
            log.debug(
                "EventSub: konnte streamer_login nicht aus live_state laden für %s",
                broadcaster_id,
                exc_info=True,
            )

        try:
            with storage.readonly_connection() as c:
                row = storage.load_streamer_identity(c, twitch_user_id=str(broadcaster_id or "").strip())
            if row:
                return str(row["twitch_login"] if hasattr(row, "keys") else row[1]).strip().lower()
        except Exception:
            log.debug(
                "EventSub: konnte streamer_login nicht aus streamer identity laden für %s",
                broadcaster_id,
                exc_info=True,
            )
        return ""

    def _is_ws_subscription_ready(
        self,
        ws_listener: EventSubWSListener | EventSubWSListenerPool,
        *,
        sub_type: str,
        broadcaster_id: str,
        condition: dict[str, str] | None = None,
    ) -> bool:
        checker = getattr(ws_listener, "is_subscription_ready", None)
        if callable(checker):
            try:
                return bool(checker(sub_type, broadcaster_id, condition))
            except Exception:
                log.debug(
                    "EventSub WS: Subscription-Readiness-Pruefung fehlgeschlagen fuer %s/%s",
                    sub_type,
                    broadcaster_id,
                    exc_info=True,
                )
                return False
        return bool(getattr(ws_listener, "is_ready", False))

    def _clear_eventsub_offline_throttle(self, broadcaster_id: str) -> None:
        throttle = getattr(self, "_eventsub_offline_throttle", None)
        if isinstance(throttle, dict):
            throttle.pop(str(broadcaster_id or "").strip(), None)
        if not self._persistent_eventsub_guards_enabled():
            return
        try:
            self._get_eventsub_state_store().release(
                EVENTSUB_STATE_KIND_OFFLINE_THROTTLE,
                str(broadcaster_id or "").strip(),
            )
        except Exception:
            log.debug(
                "EventSub: Konnte persistenten Offline-Throttle nicht freigeben fuer %s",
                broadcaster_id,
                exc_info=True,
            )

    def _spawn_eventsub_task(
        self,
        coro: Any,
        *,
        name: str,
        on_failure: Any | None = None,
    ) -> asyncio.Task[Any] | None:
        spawn = getattr(self, "_spawn_bg_task", None)
        task: asyncio.Task[Any] | None = None
        if callable(spawn):
            task = spawn(coro, name)
        else:
            try:
                task = asyncio.create_task(coro, name=name)
            except Exception:
                with suppress(Exception):
                    coro.close()
                raise

        if not isinstance(task, asyncio.Task):
            return task

        def _consume(completed: asyncio.Task[Any]) -> None:
            try:
                exc = completed.exception()
            except asyncio.CancelledError:
                log.debug("EventSub background task cancelled: %s", completed.get_name())
                return
            if exc is not None:
                log.error(
                    "EventSub background task failed: %s",
                    completed.get_name(),
                    exc_info=(type(exc), exc, exc.__traceback__),
                )
                if callable(on_failure):
                    with suppress(Exception):
                        on_failure(exc)

        task.add_done_callback(_consume)
        return task

    def _handle_eventsub_background_processing_failure(
        self,
        *,
        task_name: str,
        listener: EventSubWSListener | EventSubWSListenerPool | None = None,
    ) -> None:
        ws_listener = self._get_eventsub_ws_listener()
        if listener is not None and ws_listener is not listener:
            log.debug(
                "EventSub WS: Ignoriere veralteten Background-Task-Fehler fuer nicht mehr aktiven Listener "
                "(task=%s)",
                task_name,
            )
            return
        if ws_listener is None:
            return
        stop = getattr(ws_listener, "stop", None)
        if callable(stop):
            with suppress(Exception):
                stop()
        self._eventsub_started = False
        self._eventsub_retry_reason = "ws_processing_failed"
        log.warning(
            "EventSub WS: Asynchrone Verarbeitung fehlgeschlagen; fordere Supervisor-Restart an (task=%s)",
            task_name,
        )
        self._request_eventsub_supervisor_wakeup("ws_processing_failed")

    async def _enqueue_eventsub_stream_offline_processing(
        self,
        broadcaster_id: str,
        broadcaster_login: str | None,
        *,
        message_id: str | None = None,
    ) -> None:
        bid = str(broadcaster_id or "").strip()
        if not bid:
            raise ValueError("broadcaster_id is required")
        await self._get_eventsub_processing_inbox().enqueue(
            work_type="stream.offline",
            message_id=message_id,
            payload={
                "message_id": str(message_id or "").strip() or None,
                "broadcaster_id": bid,
                "broadcaster_login": str(broadcaster_login or "").strip() or None,
            },
        )

    async def _enqueue_eventsub_stream_online_processing(
        self,
        broadcaster_id: str,
        broadcaster_login: str | None,
        event: dict[str, Any],
        *,
        message_id: str | None = None,
    ) -> None:
        bid = str(broadcaster_id or "").strip()
        if not bid:
            raise ValueError("broadcaster_id is required")
        await self._get_eventsub_processing_inbox().enqueue(
            work_type="stream.online",
            message_id=message_id,
            payload={
                "message_id": str(message_id or "").strip() or None,
                "broadcaster_id": bid,
                "broadcaster_login": str(broadcaster_login or "").strip(),
                "event": dict(event or {}),
            },
        )

    async def _enqueue_eventsub_channel_update_processing(
        self,
        broadcaster_id: str,
        broadcaster_login: str | None,
        event: dict[str, Any],
        *,
        message_id: str | None = None,
    ) -> None:
        bid = str(broadcaster_id or "").strip()
        if not bid:
            raise ValueError("broadcaster_id is required")
        await self._get_eventsub_processing_inbox().enqueue(
            work_type="channel.update",
            message_id=message_id,
            payload={
                "message_id": str(message_id or "").strip() or None,
                "broadcaster_id": bid,
                "broadcaster_login": str(broadcaster_login or "").strip().lower(),
                "event": dict(event or {}),
            },
        )

    async def _enqueue_eventsub_stream_online_followups_processing(
        self,
        *,
        broadcaster_user_id: str,
        broadcaster_login: str,
        login_value: str,
        message_id: str | None = None,
    ) -> None:
        bid = str(broadcaster_user_id or "").strip()
        if not bid:
            raise ValueError("broadcaster_user_id is required")
        await self._get_eventsub_processing_inbox().enqueue(
            work_type="stream.online.followups",
            payload={
                "message_id": str(message_id or "").strip() or None,
                "broadcaster_user_id": bid,
                "broadcaster_login": str(broadcaster_login or "").strip(),
                "login_value": str(login_value or "").strip().lower(),
            },
        )

    async def _enqueue_eventsub_raid_processing(
        self,
        to_broadcaster_id: str,
        to_broadcaster_login: str | None,
        event: dict[str, Any],
        *,
        message_id: str | None = None,
    ) -> None:
        raid_bot = getattr(self, "_raid_bot", None)
        if not raid_bot:
            log.debug(
                "EventSub: Raid-Bot nicht verfügbar für channel.raid von %s",
                to_broadcaster_login or to_broadcaster_id,
            )
            return
        to_bid = str(to_broadcaster_id or "").strip()
        if not to_bid:
            raise ValueError("to_broadcaster_id is required")
        from_login = str(event.get("from_broadcaster_user_login") or "").strip().lower()
        from_broadcaster_id = str(event.get("from_broadcaster_user_id") or "").strip() or None
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
        await self._get_eventsub_processing_inbox().enqueue(
            work_type="channel.raid",
            message_id=message_id,
            payload={
                "message_id": str(message_id or "").strip() or None,
                "to_broadcaster_id": to_bid,
                "to_broadcaster_login": str(to_broadcaster_login or "").strip(),
                "event": {
                    "from_broadcaster_user_login": from_login,
                    "from_broadcaster_user_id": from_broadcaster_id,
                    "viewers": viewer_count,
                },
            },
        )

    def _set_eventsub_webhook_notification_dispatch(self, *, active: bool) -> None:
        handler = getattr(self, "_eventsub_webhook_handler", None)
        if handler is None:
            return
        method_name = (
            "activate_notification_dispatch" if active else "deactivate_notification_dispatch"
        )
        method = getattr(handler, method_name, None)
        if callable(method):
            method()

    def _set_eventsub_webhook_revocation_callback(self) -> None:
        handler = getattr(self, "_eventsub_webhook_handler", None)
        if handler is None:
            return
        method = getattr(handler, "set_revocation_callback", None)
        if callable(method):
            method(self._handle_eventsub_webhook_revocation)

    async def _handle_eventsub_webhook_revocation(
        self,
        payload: dict[str, Any],
        *,
        message_id: str | None = None,
    ) -> None:
        subscription = payload.get("subscription") if isinstance(payload, dict) else {}
        subscription_map = subscription if isinstance(subscription, dict) else {}
        revoked_type = str(subscription_map.get("type") or "").strip().lower()
        broadcaster_id = self._eventsub_target_user_id(
            subscription_map.get("condition") if isinstance(subscription_map.get("condition"), dict) else {},
        )
        if revoked_type and broadcaster_id:
            self._eventsub_untrack_sub(revoked_type, broadcaster_id)
        if revoked_type not in _EVENTSUB_WEBHOOK_CORE_SUB_TYPES:
            return
        self._eventsub_started = False
        self._eventsub_retry_reason = "webhook_revocation"
        self._set_eventsub_webhook_notification_dispatch(active=False)
        log.warning(
            "EventSub Webhook: Kern-Subscription revocation erkannt, starte Supervisor neu "
            "(type=%s, broadcaster_id=%s, msg_id=%s)",
            revoked_type or "unknown",
            broadcaster_id or "-",
            str(message_id or "").strip() or "-",
        )
        self._ensure_eventsub_supervisor_running("webhook_revocation")

    async def _ensure_eventsub_offline_subscription(
        self,
        broadcaster_id: str,
        broadcaster_login: str,
        *,
        webhook_url: str | None = None,
        webhook_secret: str | None = None,
    ) -> bool:
        bid = str(broadcaster_id or "").strip()
        login = str(broadcaster_login or "").strip().lower()
        if not bid:
            return False

        condition = {"broadcaster_user_id": bid}
        if self._has_eventsub_webhook_transport():
            if not webhook_url or not webhook_secret:
                return False
            if self._eventsub_has_sub("stream.offline", bid):
                log.debug(
                    "EventSub Webhook: stream.offline bereits subscribed für %s, überspringe",
                    login or bid,
                )
                return True
            created, already_exists = await self._create_eventsub_webhook_subscription(
                sub_type="stream.offline",
                condition=condition,
                webhook_url=webhook_url,
                secret=webhook_secret,
                oauth_token=None,
            )
            if not created:
                return False
            self._eventsub_track_sub("stream.offline", bid)
            log.info(
                "EventSub Webhook: stream.offline Subscription %s für %s",
                "bereits vorhanden" if already_exists else "erstellt",
                login or bid,
            )
            await self._record_eventsub_capacity_snapshot("stream_offline_subscribed", force=True)
            return True

        listener = self._get_eventsub_ws_listener()
        if listener is None:
            log.warning(
                "EventSub WS: Kein aktiver Listener für stream.offline Subscription von %s",
                login or bid,
            )
            self._request_eventsub_supervisor_wakeup("stream_offline_no_listener")
            await self._record_eventsub_capacity_snapshot(
                "stream_offline_subscribe_no_listener",
                force=True,
            )
            return False

        if self._eventsub_has_sub("stream.offline", bid) and self._is_ws_subscription_ready(
            listener,
            sub_type="stream.offline",
            broadcaster_id=bid,
            condition=condition,
        ):
            return True

        if not await listener.wait_until_ready(timeout=8.0, poll_interval=0.1):
            log.warning(
                "EventSub WS: Listener nicht bereit für stream.offline Subscription von %s",
                login or bid,
            )
            self._request_eventsub_supervisor_wakeup("stream_offline_listener_not_ready")
            await self._record_eventsub_capacity_snapshot(
                "stream_offline_subscribe_not_ready",
                force=True,
            )
            return False

        success = await listener.add_subscription_dynamic(
            "stream.offline",
            bid,
            condition=condition,
        )
        if not success:
            log.warning(
                "EventSub WS: stream.offline Subscription fehlgeschlagen für %s",
                login or bid,
            )
            await self._record_eventsub_capacity_snapshot(
                "stream_offline_subscribe_failed",
                force=True,
            )
            return False

        self._eventsub_track_sub("stream.offline", bid)
        log.info(
            "EventSub WS: stream.offline Subscription erstellt für %s (ID: %s)",
            login or bid,
            bid,
        )
        await self._record_eventsub_capacity_snapshot("stream_offline_subscribed", force=True)
        return True

    async def _finalize_eventsub_offline_session(
        self,
        *,
        broadcaster_id: str,
        login_lower: str,
    ) -> None:
        finalize = getattr(self, "_finalize_stream_session", None)
        if not callable(finalize) or not login_lower:
            return
        try:
            await finalize(login=login_lower, reason="offline")
        except Exception:
            log.exception(
                "EventSub: Konnte Streamsitzung nicht per stream.offline abschliessen für %s (%s)",
                login_lower,
                broadcaster_id,
            )

    def _install_stream_went_live_handler(
        self,
        *,
        webhook_url: str | None = None,
        webhook_secret: str | None = None,
    ) -> None:
        """Install the shared go-live handler used by polling and EventSub."""

        async def _handle_stream_went_live(bid: str, login: str):
            """
            Wird von Polling UND EventSub stream.online aufgerufen wenn ein Stream live geht.
            Subscribed stream.offline via Webhook und joined Chat-Bot.
            """
            # Debounce: verhindert Doppelausführung wenn EventSub + Polling fast gleichzeitig feuern.
            # 60s-Fenster ist deutlich größer als das 15s-Poll-Intervall.
            _golive_ts: dict = getattr(self, "_golive_last_handled_ts", None)
            if not isinstance(_golive_ts, dict):
                _golive_ts = {}
                self._golive_last_handled_ts = _golive_ts
            _now = time.time()
            _bid_key = str(bid).strip()
            if _now - float(_golive_ts.get(_bid_key) or 0.0) < 60.0:
                log.debug(
                    "Go-Live Handler: Doppelaufruf innerhalb 60s für %s ignoriert",
                    login or bid,
                )
                return
            _golive_ts[_bid_key] = _now
            self._clear_eventsub_offline_throttle(_bid_key)

            try:
                # 1. Chat-Bot joinen (falls Partner mit Chat-Scope)
                chat_bot = getattr(self, "_twitch_chat_bot", None)
                login_norm = (login or "").strip().lower()
                if chat_bot:
                    if not login_norm:
                        try:
                            with storage.readonly_connection() as c:
                                row = storage.load_streamer_identity(c, twitch_user_id=bid)
                            if row:
                                login_norm = str(
                                    row["twitch_login"] if hasattr(row, "keys") else row[1]
                                ).lower()
                        except Exception:
                            log.debug(
                                "Polling: login lookup for user id %s failed",
                                bid,
                                exc_info=True,
                            )
                    if login_norm:
                        monitored = getattr(chat_bot, "_monitored_streamers", set())
                        if login_norm not in monitored:
                            success = await chat_bot.join(login_norm, channel_id=bid)
                            if success:
                                log.info(
                                    "Polling: Chat-Bot joined %s (%s) nach Go-Live",
                                    login_norm,
                                    bid,
                                )

                webhook_transport_available = bool(
                    webhook_url and webhook_secret and self._has_eventsub_webhook_transport()
                )
                # 2. stream.offline Subscription für den aktiven Transport nachziehen.
                if webhook_transport_available:
                    fully_authed = (
                        await self._is_fully_authed(str(bid))
                        if hasattr(self, "_is_fully_authed")
                        else True
                    )
                    if fully_authed:
                        await self._ensure_eventsub_offline_subscription(
                            str(bid),
                            login or bid,
                            webhook_url=webhook_url,
                            webhook_secret=webhook_secret,
                        )
                    else:
                        log.info(
                            "Polling: stream.offline übersprungen für %s (needs_reauth=1)",
                            login or bid,
                        )
                        if chat_bot and login_norm:
                            try:
                                await self._maybe_send_reauth_chat_reminder(
                                    chat_bot=chat_bot,
                                    broadcaster_id=str(bid),
                                    login_lower=login_norm,
                                )
                            except Exception:
                                log.debug(
                                    "ReAuth reminder: Chat-Hinweis fehlgeschlagen für %s",
                                    login_norm,
                                    exc_info=True,
                                )
                else:
                    await self._ensure_eventsub_offline_subscription(
                        str(bid),
                        login or bid,
                    )
                    return

                # 3. Broadcaster-Token Subscriptions (Bits, Hype, Subs, Ads, Channel Points)
                broadcaster_token = await self._resolve_eventsub_broadcaster_token(str(bid))
                token_scopes: set[str] = set()
                if broadcaster_token:
                    # Scopes des Tokens aus DB laden – nur Subs subscriben, für die der Scope vorhanden ist
                    try:
                        with storage.readonly_connection() as _sc:
                            _scope_row = _sc.execute(
                                "SELECT scopes FROM twitch_raid_auth WHERE twitch_user_id = %s",
                                (str(bid),),
                            ).fetchone()
                        token_scopes = {
                            scope.strip().lower()
                            for scope in str((_scope_row[0] if _scope_row else "") or "").split()
                            if scope.strip()
                        }
                    except Exception:
                        log.debug(
                            "EventSub: Konnte Scopes für %s nicht laden",
                            login or bid,
                            exc_info=True,
                        )
                        token_scopes = set()

                    broadcaster_subs = [
                        # (sub_type, version, required_scope)
                        ("channel.cheer", "1", "bits:read"),
                        ("channel.bits.use", "1", "bits:read"),
                        ("channel.hype_train.begin", "1", "channel:read:hype_train"),
                        ("channel.hype_train.progress", "1", "channel:read:hype_train"),
                        ("channel.hype_train.end", "1", "channel:read:hype_train"),
                        ("channel.subscribe", "1", "channel:read:subscriptions"),
                        ("channel.subscription.gift", "1", "channel:read:subscriptions"),
                        ("channel.subscription.message", "1", "channel:read:subscriptions"),
                        ("channel.subscription.end", "1", "channel:read:subscriptions"),
                        ("channel.ad_break.begin", "1", "channel:read:ads"),
                        (
                            "channel.channel_points_automatic_reward_redemption.add",
                            "2",
                            "channel:read:redemptions",
                        ),
                        (
                            "channel.channel_points_custom_reward_redemption.add",
                            "1",
                            "channel:read:redemptions",
                        ),
                    ]
                    for sub_type, version, required_scope in broadcaster_subs:
                        # Scope-Check: überspringen wenn Token den Scope nicht hat
                        if token_scopes and required_scope not in token_scopes:
                            log.debug(
                                "EventSub Webhook: %s übersprungen für %s (Scope '%s' fehlt im Token)",
                                sub_type,
                                login or bid,
                                required_scope,
                            )
                            continue
                        if self._eventsub_has_sub(sub_type, str(bid)):
                            log.debug(
                                "EventSub Webhook: %s bereits subscribed für %s, überspringe",
                                sub_type,
                                login or bid,
                            )
                            continue
                        try:
                            await self.api.subscribe_eventsub_webhook(
                                sub_type=sub_type,
                                condition={"broadcaster_user_id": str(bid)},
                                webhook_url=webhook_url,
                                secret=webhook_secret,
                                version=version,
                                oauth_token=broadcaster_token,
                            )
                            self._eventsub_track_sub(sub_type, str(bid))
                            log.debug(
                                "EventSub Webhook: %s Subscription erstellt für %s",
                                sub_type,
                                login or bid,
                            )
                        except aiohttp.ClientResponseError as exc:
                            if int(getattr(exc, "status", 0) or 0) == 401:
                                log.warning(
                                    "EventSub Webhook: %s fehlgeschlagen für %s (HTTP 401 Invalid OAuth token). "
                                    "Weitere Broadcaster-Subscriptions werden übersprungen.",
                                    sub_type,
                                    login or bid,
                                )
                                break
                            log.debug(
                                "EventSub Webhook: %s fehlgeschlagen für %s (HTTP %s, evtl. Scope fehlt)",
                                sub_type,
                                login or bid,
                                int(getattr(exc, "status", 0) or 0),
                                exc_info=True,
                            )
                        except Exception:
                            log.debug(
                                "EventSub Webhook: %s fehlgeschlagen für %s (evtl. Scope fehlt)",
                                sub_type,
                                login or bid,
                                exc_info=True,
                            )

                # 4. Moderator-Subscriptions (Bans, Shoutouts, Follow) → Bot-Token bevorzugen
                bot_token, bot_id, bot_scopes = await self._resolve_eventsub_bot_auth()
                moderator_subs = [
                    # (sub_type, version, required_scope, requires_moderator_user_id)
                    ("channel.ban", "1", "moderator:manage:banned_users", True),
                    ("channel.unban", "1", "moderator:manage:banned_users", True),
                    ("channel.shoutout.create", "1", "moderator:manage:shoutouts", True),
                    ("channel.shoutout.receive", "1", "moderator:manage:shoutouts", True),
                    ("channel.follow", "2", "moderator:read:followers", True),
                ]

                for sub_type, version, required_scope, needs_moderator_id in moderator_subs:
                    if self._eventsub_has_sub(sub_type, str(bid)):
                        continue

                    async def _try_moderator_subscription(
                        *,
                        auth_label: str,
                        oauth_token: str,
                        condition: dict[str, str],
                    ) -> bool:
                        try:
                            await self.api.subscribe_eventsub_webhook(
                                sub_type=sub_type,
                                condition=condition,
                                webhook_url=webhook_url,
                                secret=webhook_secret,
                                version=version,
                                oauth_token=oauth_token,
                            )
                            self._eventsub_track_sub(sub_type, str(bid))
                            if auth_label == "broadcaster":
                                log.warning(
                                    "EventSub Webhook: %s Subscription erstellt fuer %s via Broadcaster-Fallback. "
                                    "Der Bot-Token sollte diesen Moderator-Pfad uebernehmen.",
                                    sub_type,
                                    login or bid,
                                )
                            else:
                                log.debug(
                                    "EventSub Webhook: %s Subscription erstellt für %s via %s",
                                    sub_type,
                                    login or bid,
                                    auth_label,
                                )
                            return True
                        except aiohttp.ClientResponseError as exc:
                            # Twitch may require `moderator_user_id` for some mod events; retry once if we can.
                            if (
                                int(getattr(exc, "status", 0) or 0) == 400
                                and not needs_moderator_id
                                and "moderator_user_id" in str(getattr(exc, "message", "") or "").lower()
                            ):
                                retry_condition = dict(condition)
                                if oauth_token == bot_token and bot_id:
                                    retry_condition["moderator_user_id"] = str(bot_id)
                                elif oauth_token == broadcaster_token:
                                    retry_condition["moderator_user_id"] = str(bid)
                                if "moderator_user_id" in retry_condition:
                                    try:
                                        await self.api.subscribe_eventsub_webhook(
                                            sub_type=sub_type,
                                            condition=retry_condition,
                                            webhook_url=webhook_url,
                                            secret=webhook_secret,
                                            version=version,
                                            oauth_token=oauth_token,
                                        )
                                        self._eventsub_track_sub(sub_type, str(bid))
                                        if auth_label == "broadcaster":
                                            log.warning(
                                                "EventSub Webhook: %s Subscription erstellt fuer %s via Broadcaster-Fallback "
                                                "(retry mit moderator_user_id). Der Bot-Token sollte diesen Moderator-Pfad uebernehmen.",
                                                sub_type,
                                                login or bid,
                                            )
                                        else:
                                            log.debug(
                                                "EventSub Webhook: %s Subscription erstellt für %s via %s (retry with moderator_user_id)",
                                                sub_type,
                                                login or bid,
                                                auth_label,
                                            )
                                        return True
                                    except Exception:
                                        log.debug(
                                            "EventSub Webhook: %s retry fehlgeschlagen für %s via %s",
                                            sub_type,
                                            login or bid,
                                            auth_label,
                                            exc_info=True,
                                        )
                            log.debug(
                                "EventSub Webhook: %s fehlgeschlagen für %s via %s (HTTP %s, evtl. Scope fehlt)",
                                sub_type,
                                login or bid,
                                auth_label,
                                int(getattr(exc, "status", 0) or 0),
                                exc_info=True,
                            )
                            return False
                        except Exception:
                            log.debug(
                                "EventSub Webhook: %s fehlgeschlagen für %s via %s (evtl. Scope fehlt)",
                                sub_type,
                                login or bid,
                                auth_label,
                                exc_info=True,
                            )
                            return False

                    auth_attempts: list[tuple[str, str, dict[str, str]]] = []

                    if bot_token and (not bot_scopes or required_scope in bot_scopes):
                        bot_condition = {"broadcaster_user_id": str(bid)}
                        if needs_moderator_id:
                            if bot_id:
                                bot_condition["moderator_user_id"] = str(bot_id)
                                auth_attempts.append(("bot", bot_token, bot_condition))
                        else:
                            auth_attempts.append(("bot", bot_token, bot_condition))

                    if broadcaster_token and (not token_scopes or required_scope in token_scopes):
                        broadcaster_condition = {"broadcaster_user_id": str(bid)}
                        if needs_moderator_id:
                            broadcaster_condition["moderator_user_id"] = str(bid)
                        auth_attempts.append(
                            ("broadcaster", broadcaster_token, broadcaster_condition)
                        )

                    for auth_label, oauth_token, condition in auth_attempts:
                        if await _try_moderator_subscription(
                            auth_label=auth_label,
                            oauth_token=oauth_token,
                            condition=condition,
                        ):
                            break

            except Exception:
                log.exception("Polling: Go-Live Handler fehlgeschlagen für %s", login or bid)

        self._handle_stream_went_live = _handle_stream_went_live

    async def _on_eventsub_stream_offline(
        self,
        broadcaster_id: str,
        broadcaster_login: str | None,
        *,
        message_id: str | None = None,
        allow_scheduled_refresh: bool = True,
    ) -> None:
        """Direkter Auto-Raid-Trigger bei stream.offline EventSub."""
        if not broadcaster_id:
            return
        trigger_ts = time.monotonic()
        login_lower = self._resolve_eventsub_broadcaster_login(broadcaster_id, broadcaster_login)
        # Fallback-Dedupe-Guard zurücksetzen, damit beim nächsten Streamstart erneut erinnert werden kann.
        try:
            fallback_guard = getattr(self, "_reauth_reminder_last_sent_ts", None)
            if isinstance(fallback_guard, dict):
                fallback_guard.pop(str(broadcaster_id), None)
        except Exception:
            log.debug(
                "ReAuth reminder: Konnte Fallback-Guard nicht zurücksetzen",
                exc_info=True,
            )
        # Doppel-Trigger (Polling + EventSub) vermeiden
        throttle = getattr(self, "_eventsub_offline_throttle", None)
        if throttle is None:
            throttle = {}
            self._eventsub_offline_throttle = throttle
        now = time.time()
        last_ts = throttle.get(broadcaster_id)
        if last_ts and now - last_ts < 120:
            log.debug("EventSub Offline-Throttle: %s noch in 120s-Fenster, ignoriere", broadcaster_id)
            return
        if self._persistent_eventsub_guards_enabled():
            guard_claimed = self._get_eventsub_state_store().claim(
                EVENTSUB_STATE_KIND_OFFLINE_THROTTLE,
                str(broadcaster_id),
                ttl_seconds=120.0,
            )
            if not guard_claimed:
                throttle[broadcaster_id] = now
                return
        throttle[broadcaster_id] = now

        previous_state = self._load_live_state_row(login_lower)

        async def _persist_offline_state() -> None:
            await self._finalize_eventsub_offline_session(
                broadcaster_id=broadcaster_id,
                login_lower=login_lower,
            )
            try:
                with storage.transaction() as c:
                    c.execute(
                        """
                        UPDATE twitch_live_state
                           SET is_live = 0,
                               last_seen_at = %s,
                               active_session_id = NULL
                         WHERE twitch_user_id = %s
                        """,
                        (
                            datetime.now(UTC).isoformat(timespec="seconds"),
                            broadcaster_id,
                        ),
                    )
            except Exception:
                log.debug(
                    "EventSub: konnte Live-State nicht sofort auf offline setzen fuer %s",
                    broadcaster_id,
                    exc_info=True,
                )

        run_once = getattr(self, "_run_eventsub_business_effect_once", None)
        if callable(run_once):
            await run_once(
                message_id=message_id,
                effect_name="stream_offline_state",
                coro_factory=_persist_offline_state,
            )
        else:
            await _persist_offline_state()
        schedule_refresh = getattr(self, "_schedule_partner_raid_score_refresh", None)
        if allow_scheduled_refresh and callable(schedule_refresh):
            try:
                schedule_refresh(
                    twitch_user_id=broadcaster_id,
                    login=login_lower or broadcaster_login,
                    trigger="eventsub_stream_offline",
                )
            except Exception:
                log.debug(
                    "EventSub: partner raid score refresh scheduling failed for offline %s",
                    broadcaster_login or broadcaster_id,
                    exc_info=True,
                )
        else:
            refresh = getattr(self, "_request_partner_raid_score_refresh", None)
            if callable(refresh):
                try:
                    if callable(run_once):
                        await run_once(
                            message_id=message_id,
                            effect_name="stream_offline_refresh",
                            coro_factory=lambda: refresh(
                                twitch_user_id=broadcaster_id,
                                login=login_lower or broadcaster_login,
                                trigger="eventsub_stream_offline",
                            ),
                        )
                    else:
                        await refresh(
                            twitch_user_id=broadcaster_id,
                            login=login_lower or broadcaster_login,
                            trigger="eventsub_stream_offline",
                        )
                except Exception:
                    log.debug(
                        "EventSub: partner raid score refresh failed for offline %s",
                        broadcaster_login or broadcaster_id,
                        exc_info=True,
                    )

        # Frische Online-Streams sammeln, damit Auto-Raid Partner erkennen kann
        tracked_logins = self._get_tracked_logins_for_eventsub()
        streams_by_login = await self._fetch_streams_by_logins_quick(tracked_logins)

        log.info(
            "EventSub stream.offline received for %s (id=%s) -> triggering auto-raid pipeline",
            broadcaster_login or login_lower,
            broadcaster_id,
        )

        try:
            if callable(run_once):
                await run_once(
                    message_id=message_id,
                    effect_name="stream_offline_auto_raid",
                    coro_factory=lambda: self._handle_auto_raid_on_offline(
                        login=login_lower or broadcaster_login or "",
                        twitch_user_id=broadcaster_id,
                        previous_state=previous_state,
                        streams_by_login=streams_by_login,
                        offline_trigger_ts=trigger_ts,
                    ),
                )
            else:
                await self._handle_auto_raid_on_offline(
                    login=login_lower or broadcaster_login or "",
                    twitch_user_id=broadcaster_id,
                    previous_state=previous_state,
                    streams_by_login=streams_by_login,
                    offline_trigger_ts=trigger_ts,
                )
        except Exception:
            log.exception(
                "EventSub: Auto-Raid offline handling failed for %s",
                broadcaster_login or broadcaster_id,
            )

    def _get_eventsub_webhook_url(self) -> str | None:
        """Gibt die vollständige Webhook-Callback-URL zurück, falls konfiguriert."""
        base = getattr(self, "_webhook_base_url", None)
        if not base:
            return None
        return f"{base}/twitch/eventsub/callback"

    async def _cleanup_old_eventsub_subscriptions(
        self,
        webhook_url: str,
        *,
        active_target_user_ids: set[str] | None = None,
    ) -> None:
        """Löscht veraltete Webhook-Subscriptions nur noch gezielt."""
        if not getattr(self, "api", None):
            return
        try:
            existing = await self.api.list_eventsub_subscriptions(status="")
            deleted = 0
            active_ids = {
                str(value or "").strip()
                for value in (active_target_user_ids or set())
                if str(value or "").strip()
            }
            for sub in existing:
                if sub.get("transport", {}).get("callback") == webhook_url:
                    if active_ids:
                        condition = (
                            sub.get("condition")
                            if isinstance(sub.get("condition"), dict)
                            else {}
                        )
                        target_user_id = self._eventsub_target_user_id(condition)
                        if target_user_id and target_user_id in active_ids:
                            continue
                    sub_id = sub.get("id")
                    if sub_id:
                        await self.api.delete_eventsub_subscription(sub_id)
                        deleted += 1
            if deleted:
                log.info("EventSub Webhook: %d veraltete Subscriptions gelöscht", deleted)
        except Exception:
            log.exception("EventSub Webhook: Cleanup alter Subscriptions fehlgeschlagen")

    async def _start_eventsub_websocket_listener(self) -> bool:
        """Startet den WebSocket-Fallback für die zentralen EventSub-Typen."""
        if not getattr(self, "api", None):
            log.warning("EventSub WS: Keine API vorhanden, Listener wird nicht gestartet.")
            self._eventsub_retry_reason = "no_api"
            self._eventsub_started = False
            return False

        token = await self._resolve_eventsub_bot_token()
        if not token:
            log.warning(
                "EventSub WS: Kein Bot-Token verfügbar, WebSocket-Fallback kann nicht gestartet werden."
            )
            self._eventsub_retry_reason = "missing_bot_token"
            self._eventsub_started = False
            return False

        listener = EventSubWSListenerPool(
            api=self.api,
            logger=log,
            token_resolver=self._resolve_eventsub_ws_token,
            state_store=self._get_eventsub_state_store(),
        )
        self._eventsub_ws_listener = listener
        register_core_eventsub_callbacks(
            self,
            listener,
            logger=log,
            propagate_callback_errors=True,
            delivery_mode="enqueue",
        )
        self._install_stream_went_live_handler()

        raid_enabled_streamers = self._get_raid_enabled_streamers_for_eventsub()
        if not raid_enabled_streamers:
            log.info("EventSub WS: Keine Streamer für den WebSocket-Fallback gefunden.")
            try:
                await self._record_eventsub_capacity_snapshot("startup_no_streamers", force=True)
            except Exception:
                log.debug(
                    "EventSub WS: Snapshot für startup_no_streamers fehlgeschlagen",
                    exc_info=True,
                )
            self._eventsub_ws_listener = None
            self._eventsub_retry_reason = "no_streamers"
            self._eventsub_started = False
            return False

        dropped_subscriptions = 0
        for entry in raid_enabled_streamers:
            bid = str(entry.get("twitch_user_id") or "").strip()
            if not bid:
                continue
            if not listener.add_subscription("stream.online", bid, {"broadcaster_user_id": bid}):
                dropped_subscriptions += 1
            if not listener.add_subscription("stream.offline", bid, {"broadcaster_user_id": bid}):
                dropped_subscriptions += 1
            if not listener.add_subscription("channel.update", bid, {"broadcaster_user_id": bid}):
                dropped_subscriptions += 1
            if not listener.add_subscription("channel.raid", bid, {"to_broadcaster_user_id": bid}):
                dropped_subscriptions += 1
            else:
                self._eventsub_track_sub("channel.raid", bid)

        log.info(
            "EventSub WS: Fallback mit %d initialen Subscriptions auf %d Transporten für %d Streamer gestartet",
            listener.subscription_count,
            listener.listener_count,
            len(raid_enabled_streamers),
        )
        if dropped_subscriptions:
            log.error(
                "EventSub WS: %d initiale Subscriptions konnten wegen Transport-Limits nicht eingeplant werden",
                dropped_subscriptions,
            )
            try:
                await self._record_eventsub_capacity_snapshot(
                    "startup_capacity_exhausted",
                    force=True,
                )
            except Exception:
                log.debug(
                    "EventSub WS: Snapshot für startup_capacity_exhausted fehlgeschlagen",
                    exc_info=True,
                )
            self._eventsub_ws_listener = None
            self._eventsub_retry_reason = "ws_capacity_exhausted"
            self._eventsub_started = False
            return False
        run_task = asyncio.create_task(listener.run(), name="eventsub.ws.pool")
        try:
            if not await listener.wait_until_initial_registration(timeout=12.0, poll_interval=0.1):
                log.warning(
                    "EventSub WS: Initiale Subscription-Registrierung konnte nicht rechtzeitig bestätigt werden."
                )
                listener.stop()
                run_task.cancel()
                with suppress(asyncio.CancelledError):
                    await run_task
                self._eventsub_retry_reason = "ws_initial_registration_timeout"
                self._eventsub_started = False
                return False
            try:
                await self._record_eventsub_capacity_snapshot("startup_distribution", force=True)
            except Exception:
                log.debug("EventSub WS: Startup-Capacity-Snapshot fehlgeschlagen", exc_info=True)
            await run_task
        except asyncio.CancelledError:
            listener.stop()
            run_task.cancel()
            with suppress(asyncio.CancelledError):
                await run_task
            raise
        finally:
            if self._eventsub_ws_listener is listener:
                self._eventsub_ws_listener = None
            if str(getattr(self, "_eventsub_retry_reason", "") or "").strip() in {
                "",
                "webhook_ready",
                "ws_listener_exited",
            }:
                self._eventsub_retry_reason = "ws_listener_exited"
            self._eventsub_started = False
        return False

    async def _start_eventsub_listener(self) -> bool:
        """Startet EventSub bevorzugt via Webhook, sonst via WebSocket-Fallback."""
        if getattr(self, "_eventsub_started", False):
            log.debug("EventSub Listener bereits gestartet, überspringe.")
            return True
        self._eventsub_started = True

        webhook_url = self._get_eventsub_webhook_url()
        webhook_secret = getattr(self, "_webhook_secret", None)
        webhook_handler = getattr(self, "_eventsub_webhook_handler", None)

        if not getattr(self, "api", None):
            log.warning("EventSub: Keine API vorhanden, Listener wird nicht gestartet.")
            self._eventsub_retry_reason = "no_api"
            self._eventsub_started = False
            return False

        try:
            await self.bot.wait_until_ready()
        except Exception:
            log.exception("EventSub: wait_until_ready fehlgeschlagen")
            self._eventsub_retry_reason = "bot_not_ready"
            self._eventsub_started = False
            return False
        self._eventsub_enable_persistent_guards = True
        self._eventsub_defer_stream_online_followups = True
        try:
            await self._ensure_eventsub_processing_inbox_started()
        except Exception:
            log.exception("EventSub: Konnte persistente Processing-Inbox nicht starten")
            self._eventsub_retry_reason = "processing_inbox_unavailable"
            self._eventsub_started = False
            return False

        self._eventsub_webhook_active_subs = []
        self._eventsub_webhook_tracked = set()

        if not webhook_url or not webhook_secret or not webhook_handler:
            self._set_eventsub_webhook_notification_dispatch(active=False)
            log.warning(
                "EventSub Webhook: Konfiguration unvollständig, wechsle auf WebSocket-Fallback "
                "(nur Kern-Events: stream.online/offline, channel.update, dynamische Raids)."
            )
            return await self._start_eventsub_websocket_listener()

        # Callbacks registrieren
        self._set_eventsub_webhook_notification_dispatch(active=False)
        register_core_eventsub_callbacks(
            self,
            webhook_handler,
            logger=log,
            propagate_callback_errors=True,
            delivery_mode="inline",
        )

        async def _bits_cb(bid: str, login: str, event: dict):
            try:
                await self._store_bits_event(bid, event)
            except Exception:
                log.exception("EventSub Webhook: Bits-Callback fehlgeschlagen für %s", login)

        async def _hype_begin_cb(bid: str, login: str, event: dict):
            try:
                await self._store_hype_train_event(bid, event, ended=False)
            except Exception:
                log.exception(
                    "EventSub Webhook: Hype-Train-Begin-Callback fehlgeschlagen für %s",
                    login,
                )

        async def _hype_end_cb(bid: str, login: str, event: dict):
            try:
                await self._store_hype_train_event(bid, event, ended=True)
            except Exception:
                log.exception(
                    "EventSub Webhook: Hype-Train-End-Callback fehlgeschlagen für %s",
                    login,
                )

        async def _hype_progress_cb(bid: str, login: str, event: dict):
            try:
                await self._store_hype_train_event(bid, event, ended=False, progress=True)
            except Exception:
                log.exception(
                    "EventSub Webhook: Hype-Train-Progress-Callback fehlgeschlagen für %s",
                    login,
                )

        async def _sub_end_cb(bid: str, login: str, event: dict):
            try:
                await self._store_subscription_event(bid, event, "end")
            except Exception:
                log.exception(
                    "EventSub Webhook: subscription.end-Callback fehlgeschlagen für %s",
                    login,
                )

        async def _ban_cb(bid: str, login: str, event: dict):
            try:
                await self._store_ban_event(bid, event, unbanned=False)
            except Exception:
                log.exception(
                    "EventSub Webhook: channel.ban-Callback fehlgeschlagen für %s",
                    login,
                )

        async def _unban_cb(bid: str, login: str, event: dict):
            try:
                await self._store_ban_event(bid, event, unbanned=True)
            except Exception:
                log.exception(
                    "EventSub Webhook: channel.unban-Callback fehlgeschlagen für %s",
                    login,
                )

        async def _bits_use_cb(bid: str, login: str, event: dict):
            try:
                await self._store_bits_event(bid, event)
            except Exception:
                log.exception(
                    "EventSub Webhook: channel.bits.use-Callback fehlgeschlagen für %s",
                    login,
                )

        async def _shoutout_create_cb(bid: str, login: str, event: dict):
            try:
                await self._store_shoutout_event(bid, event, direction="sent")
            except Exception:
                log.exception(
                    "EventSub Webhook: shoutout.create-Callback fehlgeschlagen für %s",
                    login,
                )

        async def _shoutout_receive_cb(bid: str, login: str, event: dict):
            try:
                await self._store_shoutout_event(bid, event, direction="received")
            except Exception:
                log.exception(
                    "EventSub Webhook: shoutout.receive-Callback fehlgeschlagen für %s",
                    login,
                )

        async def _subscribe_cb(bid: str, login: str, event: dict):
            try:
                await self._store_subscription_event(bid, event, "subscribe")
            except Exception:
                log.exception(
                    "EventSub Webhook: channel.subscribe-Callback fehlgeschlagen für %s",
                    login,
                )

        async def _gift_cb(bid: str, login: str, event: dict):
            try:
                await self._store_subscription_event(bid, event, "gift")
            except Exception:
                log.exception(
                    "EventSub Webhook: channel.subscription.gift-Callback fehlgeschlagen für %s",
                    login,
                )

        async def _resub_cb(bid: str, login: str, event: dict):
            try:
                await self._store_subscription_event(bid, event, "resub")
            except Exception:
                log.exception(
                    "EventSub Webhook: channel.subscription.message-Callback fehlgeschlagen für %s",
                    login,
                )

        async def _ad_break_cb(bid: str, login: str, event: dict):
            try:
                await self._store_ad_break_event(bid, event)
            except Exception:
                log.exception(
                    "EventSub Webhook: channel.ad_break.begin-Callback fehlgeschlagen für %s",
                    login,
                )

        async def _follow_cb(bid: str, login: str, event: dict):
            user_login = (event.get("user_login") or event.get("user_name") or "").strip().lower()
            user_id = str(event.get("user_id") or "").strip()
            followed_at = event.get("followed_at") or datetime.now(UTC).isoformat()
            log.debug("EventSub: channel.follow – %s followed %s", user_login, login)
            try:
                with storage.transaction() as c:
                    c.execute(
                        """
                        INSERT INTO twitch_follow_events
                            (streamer_login, twitch_user_id, follower_login, follower_id, followed_at)
                        VALUES (%s, %s, %s, %s, %s)
                        """,
                        (login, bid, user_login, user_id or None, followed_at),
                    )
            except Exception:
                log.exception("EventSub: _follow_cb – DB-Insert fehlgeschlagen für %s", login)

        async def _first_message_cb(bid: str, login: str, event: dict):
            chatter_login = (
                event.get("chatter_user_login") or event.get("user_login") or ""
            ).strip().lower()
            chatter_id = str(
                event.get("chatter_user_id") or event.get("user_id") or ""
            ).strip() or None
            message_id = str(event.get("message_id") or "").strip() or None
            message_text = str(
                (event.get("message") or {}).get("text") or ""
            ).strip() or None
            ts_iso = datetime.now(UTC).isoformat(timespec="seconds")
            log.debug(
                "EventSub: channel.chat.user_first_message – %s in %s", chatter_login, login
            )
            if not chatter_login:
                return
            try:
                with storage.transaction() as c:
                    session_row = c.execute(
                        """
                        SELECT id FROM twitch_stream_sessions
                         WHERE streamer_login = %s AND ended_at IS NULL
                         ORDER BY started_at DESC LIMIT 1
                        """,
                        (login,),
                    ).fetchone()
                    session_id = int(session_row[0]) if session_row else None

                    c.execute(
                        """
                        INSERT INTO twitch_first_message_events
                            (streamer_login, broadcaster_id, chatter_login, chatter_id,
                             message_id, message_text, event_ts)
                        VALUES (%s, %s, %s, %s, %s, %s, %s)
                        """,
                        (login, bid, chatter_login, chatter_id, message_id, message_text, ts_iso),
                    )

                    if session_id:
                        c.execute(
                            """
                            UPDATE twitch_session_chatters
                               SET confirmed_first_ever = TRUE
                             WHERE session_id = %s AND chatter_login = %s
                            """,
                            (session_id, chatter_login),
                        )
            except Exception:
                log.exception(
                    "EventSub: _first_message_cb – DB-Insert fehlgeschlagen für %s in %s",
                    chatter_login,
                    login,
                )

        async def _points_auto_cb(bid: str, login: str, event: dict):
            try:
                await self._store_channel_points_event(bid, event)
            except Exception:
                log.exception(
                    "EventSub: channel.channel_points_automatic_reward_redemption.add fehlgeschlagen für %s",
                    login,
                )

        async def _points_custom_cb(bid: str, login: str, event: dict):
            try:
                await self._store_channel_points_event(bid, event)
            except Exception:
                log.exception(
                    "EventSub: channel.channel_points_custom_reward_redemption.add fehlgeschlagen für %s",
                    login,
                )

        webhook_handler.set_callback("channel.follow", _follow_cb)
        webhook_handler.set_callback("channel.subscribe", _subscribe_cb)
        webhook_handler.set_callback("channel.subscription.gift", _gift_cb)
        webhook_handler.set_callback("channel.subscription.message", _resub_cb)
        webhook_handler.set_callback("channel.ad_break.begin", _ad_break_cb)
        webhook_handler.set_callback("channel.cheer", _bits_cb)
        webhook_handler.set_callback("channel.hype_train.begin", _hype_begin_cb)
        webhook_handler.set_callback("channel.hype_train.end", _hype_end_cb)
        webhook_handler.set_callback("channel.hype_train.progress", _hype_progress_cb)
        webhook_handler.set_callback("channel.subscription.end", _sub_end_cb)
        webhook_handler.set_callback("channel.ban", _ban_cb)
        webhook_handler.set_callback("channel.unban", _unban_cb)
        webhook_handler.set_callback("channel.bits.use", _bits_use_cb)
        webhook_handler.set_callback("channel.shoutout.create", _shoutout_create_cb)
        webhook_handler.set_callback("channel.shoutout.receive", _shoutout_receive_cb)
        webhook_handler.set_callback(
            "channel.channel_points_automatic_reward_redemption.add", _points_auto_cb
        )
        webhook_handler.set_callback(
            "channel.channel_points_custom_reward_redemption.add", _points_custom_cb
        )
        webhook_handler.set_callback("channel.chat.user_first_message", _first_message_cb)
        self._set_eventsub_webhook_revocation_callback()
        self._install_stream_went_live_handler(
            webhook_url=webhook_url,
            webhook_secret=webhook_secret,
        )

        # Lokale Subscription-Tracking-Liste leeren
        self._eventsub_webhook_active_subs = []
        self._eventsub_webhook_tracked = set()

        # 2. Broadcaster sammeln
        raid_enabled_streamers = self._get_raid_enabled_streamers_for_eventsub()
        if not raid_enabled_streamers:
            log.info("EventSub Webhook: Keine Streamer für EventSub monitoring gefunden.")
            try:
                await self._record_eventsub_capacity_snapshot("startup_no_streamers", force=True)
            except Exception:
                log.debug(
                    "EventSub: Snapshot für startup_no_streamers fehlgeschlagen",
                    exc_info=True,
                )
            self._eventsub_retry_reason = "no_streamers"
            self._eventsub_started = False
            return False

        # 3. Live-Status abrufen
        raid_logins = [s["twitch_login"] for s in raid_enabled_streamers]
        currently_live_streams: dict[str, dict] = {}
        try:
            live_streams = await self.api.get_streams_by_logins(raid_logins)
            for stream in live_streams:
                login_lower = (stream.get("user_login") or "").lower()
                if login_lower:
                    currently_live_streams[login_lower] = stream
            log.info(
                "EventSub Webhook: %d von %d raid-enabled Streamern sind aktuell live",
                len(currently_live_streams),
                len(raid_enabled_streamers),
            )
        except Exception:
            log.exception(
                "EventSub Webhook: Konnte Live-Status nicht abrufen, "
                "subscribe keine stream.offline beim Start"
            )

        # 3b. Bot-Auth für channel.chat.user_first_message auflösen
        _bot_token_fm: str | None = None
        _bot_id_fm: str | None = None
        try:
            _bot_token_fm, _bot_id_fm, _ = await self._resolve_eventsub_bot_auth()
        except Exception:
            log.debug(
                "EventSub Webhook: Bot-Auth für channel.chat.user_first_message nicht verfügbar",
                exc_info=True,
            )

        # 4. stream.online + stream.offline + channel.update für alle/live Streamer
        offline_added = 0
        online_added = 0
        update_added = 0
        startup_coverage: dict[str, set[str]] = {}
        for entry in raid_enabled_streamers:
            bid = entry.get("twitch_user_id")
            login = entry.get("twitch_login", "").lower()
            if not bid:
                continue
            broadcaster_id = str(bid)

            # stream.online für ALLE (so erkennen wir Go-Live sofort statt per 15s-Polling)
            # Webhook-Subscriptions erfordern einen App-Access-Token (client_credentials),
            # daher oauth_token=None → TwitchAPI nutzt automatisch den App-Token.
            try:
                result, already_exists = await self._create_eventsub_webhook_subscription(
                    sub_type="stream.online",
                    condition={"broadcaster_user_id": str(bid)},
                    webhook_url=webhook_url,
                    secret=webhook_secret,
                    oauth_token=None,
                )
                if result:
                    self._eventsub_track_sub("stream.online", broadcaster_id)
                    startup_coverage.setdefault(broadcaster_id, set()).add("stream.online")
                    online_added += 1
                    if already_exists:
                        log.debug("EventSub Webhook: stream.online bereits vorhanden für %s", login)
            except Exception:
                log.debug(
                    "EventSub Webhook: stream.online fehlgeschlagen für %s",
                    login,
                    exc_info=True,
                )

            # channel.update für ALLE (Titel/Game-Änderungen mitbekommen)
            try:
                result, already_exists = await self._create_eventsub_webhook_subscription(
                    sub_type="channel.update",
                    condition={"broadcaster_user_id": str(bid)},
                    webhook_url=webhook_url,
                    secret=webhook_secret,
                    oauth_token=None,
                    version="2",
                )
                if result:
                    self._eventsub_track_sub("channel.update", broadcaster_id)
                    startup_coverage.setdefault(broadcaster_id, set()).add("channel.update")
                    update_added += 1
                    if already_exists:
                        log.debug("EventSub Webhook: channel.update bereits vorhanden für %s", login)
            except Exception:
                log.debug(
                    "EventSub Webhook: channel.update fehlgeschlagen für %s",
                    login,
                    exc_info=True,
                )

            # stream.offline für ALLE Streamer (nicht nur live) – so wird
            # auch ein Offline-Ereignis erkannt wenn der Bot während eines
            # laufenden Streams neu gestartet wurde oder der Streamer offline
            # ist, aber danach wieder live geht und wir keinen Neustart haben.
            try:
                result, already_exists = await self._create_eventsub_webhook_subscription(
                    sub_type="stream.offline",
                    condition={"broadcaster_user_id": str(bid)},
                    webhook_url=webhook_url,
                    secret=webhook_secret,
                    oauth_token=None,
                )
                if result:
                    self._eventsub_track_sub("stream.offline", broadcaster_id)
                    startup_coverage.setdefault(broadcaster_id, set()).add("stream.offline")
                    offline_added += 1
                    log.debug(
                        "EventSub Webhook: stream.offline %s für %s",
                        "bereits vorhanden" if already_exists else "subscribed",
                        login,
                    )
            except Exception:
                log.exception("EventSub Webhook: stream.offline fehlgeschlagen für %s", login)

            # channel.raid für ALLE Streamer – so werden eingehende Raids
            # zuverlässig erkannt ohne auf die dynamische Subscription im
            # Raid-Moment angewiesen zu sein (eliminiert Race Condition).
            try:
                result, already_exists = await self._create_eventsub_webhook_subscription(
                    sub_type="channel.raid",
                    condition={"to_broadcaster_user_id": broadcaster_id},
                    webhook_url=webhook_url,
                    secret=webhook_secret,
                    oauth_token=None,
                )
                if result:
                    self._eventsub_track_sub("channel.raid", broadcaster_id)
                    startup_coverage.setdefault(broadcaster_id, set()).add("channel.raid")
                    log.debug(
                        "EventSub Webhook: channel.raid %s für %s",
                        "bereits vorhanden" if already_exists else "subscribed",
                        login,
                    )
            except Exception:
                log.exception("EventSub Webhook: channel.raid fehlgeschlagen für %s", login)

            # channel.chat.user_first_message – erstes Mal überhaupt im Channel schreiben
            if _bot_token_fm and _bot_id_fm:
                try:
                    result, already_exists = await self._create_eventsub_webhook_subscription(
                        sub_type="channel.chat.user_first_message",
                        condition={
                            "broadcaster_user_id": broadcaster_id,
                            "user_id": str(_bot_id_fm),
                        },
                        webhook_url=webhook_url,
                        secret=webhook_secret,
                        oauth_token=_bot_token_fm,
                    )
                    if result:
                        self._eventsub_track_sub("channel.chat.user_first_message", broadcaster_id)
                        log.debug(
                            "EventSub Webhook: channel.chat.user_first_message %s für %s",
                            "bereits vorhanden" if already_exists else "subscribed",
                            login,
                        )
                except Exception:
                    log.debug(
                        "EventSub Webhook: channel.chat.user_first_message fehlgeschlagen für %s",
                        login,
                        exc_info=True,
                    )

        log.info(
            "EventSub Webhook: stream.online=%d, channel.update=%d, stream.offline=%d subscribiert",
            online_added,
            update_added,
            offline_added,
        )
        try:
            await self._record_eventsub_capacity_snapshot("startup_distribution", force=True)
        except Exception:
            log.debug("EventSub: Startup-Capacity-Snapshot fehlgeschlagen", exc_info=True)
        startup_healthy, missing_critical = self._is_eventsub_webhook_startup_healthy(
            raid_enabled_streamers,
            startup_coverage=startup_coverage,
        )
        if not startup_healthy:
            log.warning(
                "EventSub Webhook: Startup unvollständig "
                "(streamers=%d, stream.online=%d, channel.update=%d, stream.offline=%d) "
                "– Supervisor wird erneut versuchen. missing_critical=%s",
                len(raid_enabled_streamers),
                online_added,
                update_added,
                offline_added,
                missing_critical,
            )
            self._set_eventsub_webhook_notification_dispatch(active=False)
            self._eventsub_retry_reason = "webhook_startup_incomplete"
            self._eventsub_started = False
            return False
        active_target_user_ids = {
            str(entry.get("twitch_user_id") or "").strip()
            for entry in raid_enabled_streamers
            if str(entry.get("twitch_user_id") or "").strip()
        }
        try:
            await self._cleanup_old_eventsub_subscriptions(
                webhook_url,
                active_target_user_ids=active_target_user_ids,
            )
        except Exception:
            log.debug(
                "EventSub Webhook: Gezieltes Cleanup veralteter Subscriptions fehlgeschlagen",
                exc_info=True,
            )
        self._set_eventsub_webhook_notification_dispatch(active=True)
        self._eventsub_retry_reason = "webhook_ready"
        return True

    async def _resolve_eventsub_bot_auth(self) -> tuple[str | None, str | None, set[str]]:
        """Return bot token + bot id + scopes (best-effort) for EventSub auth.

        Token: always without the legacy `oauth:` prefix.
        Scopes: may be empty when unknown/uninitialised.
        """
        bot_token_mgr = getattr(self, "_bot_token_manager", None)
        if not bot_token_mgr:
            return None, None, set()
        try:
            token, bot_id = await bot_token_mgr.get_valid_token()
            token = str(token or "").strip()
            if token.lower().startswith("oauth:"):
                token = token[6:]
            resolved_bot_id = str(bot_id or getattr(bot_token_mgr, "bot_id", "") or "").strip() or None
            scopes = {
                str(scope).strip().lower()
                for scope in (getattr(bot_token_mgr, "scopes", None) or set())
                if str(scope).strip()
            }
            return token or None, resolved_bot_id, scopes
        except Exception:
            log.debug("EventSub Webhook: konnte Bot-Auth nicht laden", exc_info=True)
            return None, None, set()

    async def _resolve_eventsub_bot_token(self) -> str | None:
        """Gibt den aktuellen Bot-Token zurück (ohne 'oauth:' Präfix)."""
        token, _, _ = await self._resolve_eventsub_bot_auth()
        return token

    async def _resolve_eventsub_ws_token(self, _: str) -> str | None:
        """Kompatibilitäts-Resolver für EventSubWSListener."""
        return await self._resolve_eventsub_bot_token()

    async def _resolve_eventsub_broadcaster_token(self, broadcaster_user_id: str) -> str | None:
        """Gibt den Broadcaster-Token für eine bestimmte User-ID zurück.
        Gibt None zurück wenn needs_reauth=True oder kein Token vorhanden."""
        try:
            # Kein Token für User die noch re-authen müssen
            if hasattr(self, "_is_fully_authed") and not await self._is_fully_authed(str(broadcaster_user_id)):
                return None
            raid_bot = getattr(self, "_raid_bot", None)
            auth_manager = getattr(raid_bot, "auth_manager", None) if raid_bot else None
            session = getattr(raid_bot, "session", None) if raid_bot else None
            if not auth_manager or not session or getattr(session, "closed", False):
                return None
            token = await auth_manager.get_valid_token(str(broadcaster_user_id), session)
            token = str(token or "").strip()
            if not token:
                return None
            if token.lower().startswith("oauth:"):
                token = token[6:]
            return token or None
        except Exception:
            log.debug(
                "EventSub Webhook: konnte Broadcaster-Token nicht laden",
                exc_info=True,
            )
            return None

    def _eventsub_has_sub(self, sub_type: str, broadcaster_user_id: str) -> bool:
        """Prüft ob eine EventSub-Subscription bereits in dieser Session registriert wurde."""
        tracked: set = getattr(self, "_eventsub_webhook_tracked", None)
        if tracked is None:
            return False
        return (sub_type, str(broadcaster_user_id)) in tracked

    def _eventsub_track_sub(self, sub_type: str, broadcaster_user_id: str) -> None:
        """Merkt sich eine aktive EventSub-Subscription für spätere Capacity-Snapshots."""
        tracked: set = getattr(self, "_eventsub_webhook_tracked", None)
        if tracked is None:
            tracked = set()
            self._eventsub_webhook_tracked = tracked
        tracked.add((sub_type, str(broadcaster_user_id)))
        # Kompatibilität: active_subs-Liste für Capacity-Snapshots weiter befüllen
        active_subs: list[dict] = getattr(self, "_eventsub_webhook_active_subs", None)
        if active_subs is None:
            active_subs = []
            self._eventsub_webhook_active_subs = active_subs
        if not any(
            s.get("sub_type") == sub_type
            and s.get("broadcaster_user_id") == str(broadcaster_user_id)
            for s in active_subs
        ):
            active_subs.append(
                {
                    "sub_type": sub_type,
                    "broadcaster_user_id": str(broadcaster_user_id),
                }
            )

    def _eventsub_untrack_sub(self, sub_type: str, broadcaster_user_id: str) -> None:
        tracked: set | None = getattr(self, "_eventsub_webhook_tracked", None)
        if isinstance(tracked, set):
            tracked.discard((str(sub_type), str(broadcaster_user_id)))
        active_subs: list[dict[str, Any]] | None = getattr(self, "_eventsub_webhook_active_subs", None)
        if isinstance(active_subs, list):
            self._eventsub_webhook_active_subs = [
                sub
                for sub in active_subs
                if not (
                    str(sub.get("sub_type") or "") == str(sub_type)
                    and str(sub.get("broadcaster_user_id") or "") == str(broadcaster_user_id)
                )
            ]

    @staticmethod
    def _eventsub_subscription_matches(
        subscription: dict[str, Any] | None,
        *,
        sub_type: str,
        broadcaster_id: str,
        webhook_url: str | None = None,
    ) -> bool:
        subscription_map = subscription if isinstance(subscription, dict) else {}
        expected_type = str(sub_type or "").strip().lower()
        actual_type = str(subscription_map.get("type") or "").strip().lower()
        if not expected_type or actual_type != expected_type:
            return False

        target_id = _EventSubMixin._eventsub_target_user_id(
            subscription_map.get("condition"),
            fallback="",
        )
        if target_id != str(broadcaster_id or "").strip():
            return False

        if webhook_url:
            transport = (
                subscription_map.get("transport")
                if isinstance(subscription_map.get("transport"), dict)
                else {}
            )
            method = str(transport.get("method") or "").strip().lower()
            callback = str(transport.get("callback") or "").strip()
            if method and method != "webhook":
                return False
            if callback != str(webhook_url).strip():
                return False

        return True

    async def _get_eventsub_webhook_subscription_status(
        self,
        *,
        sub_type: str,
        broadcaster_id: str,
        webhook_url: str | None = None,
    ) -> str | None:
        if not getattr(self, "api", None):
            return None

        try:
            subscriptions = await self.api.list_eventsub_subscriptions(status="")
        except Exception:
            log.debug(
                "EventSub Webhook: konnte Subscription-Status nicht laden für %s (%s)",
                sub_type,
                broadcaster_id,
                exc_info=True,
            )
            return None

        for subscription in subscriptions:
            if self._eventsub_subscription_matches(
                subscription,
                sub_type=sub_type,
                broadcaster_id=broadcaster_id,
                webhook_url=webhook_url,
            ):
                status = str(subscription.get("status") or "").strip().lower()
                return status or "unknown"

        return None

    async def ensure_raid_target_dynamic_ready(
        self,
        broadcaster_id: str,
        broadcaster_login: str,
        *,
        raid_flow_id: str | None = None,
        wait_timeout_seconds: float = 8.0,
        poll_interval_seconds: float = 0.5,
    ) -> tuple[bool, str]:
        """
        Stellt sicher, dass eine channel.raid Subscription für das Ziel existiert
        und möglichst schon auf `enabled` steht, bevor der Raid gestartet wird.
        """
        target_id = str(broadcaster_id or "").strip()
        if not target_id:
            return False, "missing_broadcaster_id"

        if not getattr(self, "api", None):
            return False, "no_api"

        local_tracking = False
        has_sub = getattr(self, "_eventsub_has_sub", None)
        if callable(has_sub):
            try:
                local_tracking = bool(has_sub("channel.raid", target_id))
            except Exception:
                log.debug(
                    "EventSub readiness local tracking lookup failed for %s",
                    broadcaster_login,
                    exc_info=True,
                )

        if not self._has_eventsub_webhook_transport():
            ws_listener = self._get_eventsub_ws_listener()
            if ws_listener is None:
                return False, "missing_ws_listener"
            raid_condition = {"to_broadcaster_user_id": target_id}
            if local_tracking and self._is_ws_subscription_ready(
                ws_listener,
                sub_type="channel.raid",
                broadcaster_id=target_id,
                condition=raid_condition,
            ):
                return True, "ws_already_tracked"
            if not await ws_listener.wait_until_ready(
                timeout=max(0.0, float(wait_timeout_seconds)),
                poll_interval=max(0.05, float(poll_interval_seconds)),
            ):
                return False, "ws_not_ready"
            if local_tracking and self._is_ws_subscription_ready(
                ws_listener,
                sub_type="channel.raid",
                broadcaster_id=target_id,
                condition=raid_condition,
            ):
                return True, "ws_tracked_ready"
            subscribe_success = await self.subscribe_raid_target_dynamic(
                target_id,
                broadcaster_login,
            )
            return (
                (True, "ws_subscribed")
                if subscribe_success
                else (False, "ws_subscribe_failed")
            )

        webhook_url = self._get_eventsub_webhook_url()
        webhook_secret = getattr(self, "_webhook_secret", None)
        if not webhook_url or not webhook_secret:
            return False, "missing_webhook_config"

        current_status = await self._get_eventsub_webhook_subscription_status(
            sub_type="channel.raid",
            broadcaster_id=target_id,
            webhook_url=webhook_url,
        )
        if current_status == "enabled":
            self._eventsub_track_sub("channel.raid", target_id)
            log.info(
                "eventsub_raid_ready raid_flow_id=%s broadcaster_id=%s broadcaster_login=%s local_tracking=%s remote_status_before=%s subscribe_attempted=%s subscribe_success=%s final_ready=%s final_detail=%s",
                raid_flow_id or "-",
                target_id,
                broadcaster_login,
                local_tracking,
                current_status,
                False,
                None,
                True,
                "already_enabled",
            )
            return True, "already_enabled"

        subscribe_attempted = False
        subscribe_success = None
        if current_status != "webhook_callback_verification_pending":
            subscribe_attempted = True
            subscribe_success = await self.subscribe_raid_target_dynamic(
                target_id,
                broadcaster_login,
            )
            if not subscribe_success:
                current_status = await self._get_eventsub_webhook_subscription_status(
                    sub_type="channel.raid",
                    broadcaster_id=target_id,
                    webhook_url=webhook_url,
                )
                if current_status == "enabled":
                    self._eventsub_track_sub("channel.raid", target_id)
                    log.info(
                        "eventsub_raid_ready raid_flow_id=%s broadcaster_id=%s broadcaster_login=%s local_tracking=%s remote_status_before=%s remote_status_after=%s subscribe_attempted=%s subscribe_success=%s final_ready=%s final_detail=%s",
                        raid_flow_id or "-",
                        target_id,
                        broadcaster_login,
                        local_tracking,
                        "missing",
                        current_status,
                        subscribe_attempted,
                        subscribe_success,
                        True,
                        "enabled_after_retry",
                    )
                    return True, "enabled_after_retry"
                log.warning(
                    "eventsub_raid_ready raid_flow_id=%s broadcaster_id=%s broadcaster_login=%s local_tracking=%s remote_status_before=%s remote_status_after=%s subscribe_attempted=%s subscribe_success=%s final_ready=%s final_detail=%s",
                    raid_flow_id or "-",
                    target_id,
                    broadcaster_login,
                    local_tracking,
                    "missing",
                    current_status or "missing",
                    subscribe_attempted,
                    subscribe_success,
                    False,
                    f"subscribe_failed:{current_status or 'missing'}",
                )
                return False, f"subscribe_failed:{current_status or 'missing'}"

        deadline = time.monotonic() + max(0.0, float(wait_timeout_seconds))
        last_status = current_status or "missing"
        while time.monotonic() < deadline:
            current_status = await self._get_eventsub_webhook_subscription_status(
                sub_type="channel.raid",
                broadcaster_id=target_id,
                webhook_url=webhook_url,
            )
            if current_status:
                last_status = current_status
            if current_status == "enabled":
                self._eventsub_track_sub("channel.raid", target_id)
                log.info(
                    "eventsub_raid_ready raid_flow_id=%s broadcaster_id=%s broadcaster_login=%s local_tracking=%s remote_status_before=%s remote_status_after=%s subscribe_attempted=%s subscribe_success=%s final_ready=%s final_detail=%s",
                    raid_flow_id or "-",
                    target_id,
                    broadcaster_login,
                    local_tracking,
                    last_status,
                    current_status,
                    subscribe_attempted,
                    subscribe_success,
                    True,
                    "enabled",
                )
                return True, "enabled"
            if current_status not in (None, "", "webhook_callback_verification_pending"):
                log.warning(
                    "eventsub_raid_ready raid_flow_id=%s broadcaster_id=%s broadcaster_login=%s local_tracking=%s remote_status_before=%s remote_status_after=%s subscribe_attempted=%s subscribe_success=%s final_ready=%s final_detail=%s",
                    raid_flow_id or "-",
                    target_id,
                    broadcaster_login,
                    local_tracking,
                    last_status,
                    current_status,
                    subscribe_attempted,
                    subscribe_success,
                    False,
                    f"status:{current_status}",
                )
                return False, f"status:{current_status}"
            await asyncio.sleep(max(0.0, float(poll_interval_seconds)))

        current_status = await self._get_eventsub_webhook_subscription_status(
            sub_type="channel.raid",
            broadcaster_id=target_id,
            webhook_url=webhook_url,
        )
        if current_status == "enabled":
            self._eventsub_track_sub("channel.raid", target_id)
            log.info(
                "eventsub_raid_ready raid_flow_id=%s broadcaster_id=%s broadcaster_login=%s local_tracking=%s remote_status_before=%s remote_status_after=%s subscribe_attempted=%s subscribe_success=%s final_ready=%s final_detail=%s",
                raid_flow_id or "-",
                target_id,
                broadcaster_login,
                local_tracking,
                last_status,
                current_status,
                subscribe_attempted,
                subscribe_success,
                True,
                "enabled",
            )
            return True, "enabled"
        if current_status:
            last_status = current_status
        log.warning(
            "eventsub_raid_ready raid_flow_id=%s broadcaster_id=%s broadcaster_login=%s local_tracking=%s remote_status_before=%s remote_status_after=%s subscribe_attempted=%s subscribe_success=%s final_ready=%s final_detail=%s",
            raid_flow_id or "-",
            target_id,
            broadcaster_login,
            local_tracking,
            "missing",
            current_status or last_status,
            subscribe_attempted,
            subscribe_success,
            False,
            f"status:{last_status}",
        )
        return False, f"status:{last_status}"

    async def subscribe_raid_target_dynamic(
        self, broadcaster_id: str, broadcaster_login: str
    ) -> bool:
        """
        Erstellt dynamisch eine channel.raid Subscription für einen Broadcaster.

        Wird aufgerufen wenn ein Raid gestartet wird, um zu erkennen wenn der Raid ankommt.

        Returns:
            True wenn die Subscription erfolgreich erstellt wurde, False sonst.
        """
        target_id = str(broadcaster_id or "").strip()
        if not target_id:
            return False

        if not getattr(self, "api", None):
            log.error("EventSub: Keine API verfügbar für channel.raid subscription")
            await self._record_eventsub_capacity_snapshot("raid_no_api", force=True)
            return False

        if not self._has_eventsub_webhook_transport():
            listener = self._get_eventsub_ws_listener()
            if listener is None:
                log.error("EventSub WS: Kein aktiver Listener für channel.raid subscription")
                self._request_eventsub_supervisor_wakeup("raid_subscribe_no_listener")
                await self._record_eventsub_capacity_snapshot("raid_subscribe_no_listener", force=True)
                return False
            if not await listener.wait_until_ready(timeout=8.0, poll_interval=0.1):
                log.warning(
                    "EventSub WS: Listener nicht bereit für channel.raid Subscription von %s",
                    broadcaster_login,
                )
                self._request_eventsub_supervisor_wakeup("raid_subscribe_not_ready")
                await self._record_eventsub_capacity_snapshot("raid_subscribe_not_ready", force=True)
                return False
            success = await listener.add_subscription_dynamic(
                "channel.raid",
                target_id,
                condition={"to_broadcaster_user_id": target_id},
            )
            if success:
                self._eventsub_track_sub("channel.raid", target_id)
                log.info(
                    "EventSub WS: channel.raid Subscription erstellt für %s (ID: %s)",
                    broadcaster_login,
                    target_id,
                )
                await self._record_eventsub_capacity_snapshot("raid_subscribed", force=True)
                return True
            log.warning(
                "EventSub WS: channel.raid Subscription fehlgeschlagen für %s",
                broadcaster_login,
            )
            await self._record_eventsub_capacity_snapshot("raid_subscribe_failed", force=True)
            return False

        webhook_url = self._get_eventsub_webhook_url()
        webhook_secret = getattr(self, "_webhook_secret", None)

        if not webhook_url or not webhook_secret:
            log.error(
                "EventSub Webhook: Keine Webhook-URL/Secret konfiguriert für channel.raid subscription"
            )
            return False

        try:
            success, already_exists = await self._create_eventsub_webhook_subscription(
                sub_type="channel.raid",
                condition={"to_broadcaster_user_id": target_id},
                webhook_url=webhook_url,
                secret=webhook_secret,
                oauth_token=None,  # Webhook-Subscriptions benötigen App-Access-Token
            )
            if success:
                self._eventsub_track_sub("channel.raid", target_id)
                if already_exists:
                    log.info(
                        "EventSub Webhook: channel.raid Subscription bereits vorhanden für %s (ID: %s)",
                        broadcaster_login,
                        target_id,
                    )
                else:
                    log.info(
                        "EventSub Webhook: channel.raid Subscription erstellt für %s (ID: %s)",
                        broadcaster_login,
                        target_id,
                    )
                await self._record_eventsub_capacity_snapshot("raid_subscribed", force=True)
                return True
            log.error(
                "EventSub Webhook: channel.raid Subscription fehlgeschlagen für %s",
                broadcaster_login,
            )
            await self._record_eventsub_capacity_snapshot("raid_subscribe_failed", force=True)
            return False
        except Exception:
            log.exception(
                "EventSub Webhook: channel.raid Subscription fehlgeschlagen für %s",
                broadcaster_login,
            )
            await self._record_eventsub_capacity_snapshot("raid_subscribe_error", force=True)
            return False

