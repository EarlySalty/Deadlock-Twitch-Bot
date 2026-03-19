"""_EventSubMixin – EventSub capacity and listener management."""
from __future__ import annotations

import asyncio
import json
import os
import time
from datetime import UTC, datetime, timedelta
from typing import Any

import aiohttp

from .. import storage
from ..core.constants import log


class _EventSubMixin:

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
            with storage.get_conn() as c:
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
        # Webhook-basiert: Subscription-Liste aus lokalem Tracking
        tracked_subs: list[dict[str, Any]] = list(
            getattr(self, "_eventsub_webhook_active_subs", []) or []
        )
        active_subscriptions: list[dict[str, Any]] = []
        sub_type_counts: dict[str, int] = {}

        for sub in tracked_subs:
            sub_type = str(sub.get("sub_type") or "").strip().lower() or "unknown"
            broadcaster_user_id = str(sub.get("broadcaster_user_id") or "").strip()
            active_subscriptions.append(
                {
                    "listener_idx": 1,  # Webhook hat keinen Listener-Pool
                    "sub_type": sub_type,
                    "broadcaster_user_id": broadcaster_user_id,
                    "target_user_id": broadcaster_user_id,
                    "condition": {"broadcaster_user_id": broadcaster_user_id},
                }
            )
            sub_type_counts[sub_type] = int(sub_type_counts.get(sub_type, 0)) + 1

        used_slots = len(active_subscriptions)
        # Webhook skaliert auf 10.000 Subscriptions – repräsentiere das in der Snapshot-Struktur
        total_slots = 10000
        headroom_slots = max(0, total_slots - used_slots)
        utilization_pct = (
            (float(used_slots) / float(total_slots) * 100.0) if total_slots > 0 else 0.0
        )

        # listener_rows für Kompatibilität mit bestehenden Dashboard-Feldern
        listener_rows: list[dict[str, Any]] = [
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
            with storage.get_conn() as c:
                c.execute(
                    """
                    INSERT INTO twitch_eventsub_capacity_snapshot (
                        ts_utc, trigger_reason, listener_count, ready_listeners, failed_listeners,
                        used_slots, total_slots, headroom_slots, listeners_at_limit, utilization_pct, listeners_json
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
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
                        "DELETE FROM twitch_eventsub_capacity_snapshot WHERE ts_utc < ?",
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
        lookback = f"-{hours} hours"

        try:
            with storage.get_conn() as c:
                rows = c.execute(
                    """
                    SELECT ts_utc, trigger_reason, listener_count, ready_listeners, failed_listeners,
                           used_slots, total_slots, headroom_slots, listeners_at_limit, utilization_pct
                      FROM twitch_eventsub_capacity_snapshot
                     WHERE ts_utc >= datetime('now', ?)
                     ORDER BY ts_utc ASC
                    """,
                    (lookback,),
                ).fetchall()
                hourly_rows = c.execute(
                    """
                    SELECT CAST(strftime('%H', ts_utc) AS INTEGER) AS hour,
                           COUNT(*) AS samples,
                           AVG(utilization_pct) AS avg_utilization_pct,
                           MAX(utilization_pct) AS max_utilization_pct,
                           AVG(used_slots) AS avg_used_slots,
                           MAX(used_slots) AS max_used_slots,
                           AVG(listener_count) AS avg_listener_count,
                           MAX(listener_count) AS max_listener_count
                      FROM twitch_eventsub_capacity_snapshot
                     WHERE ts_utc >= datetime('now', ?)
                     GROUP BY hour
                     ORDER BY hour ASC
                    """,
                    (lookback,),
                ).fetchall()
                reason_rows = c.execute(
                    """
                    SELECT trigger_reason,
                           COUNT(*) AS samples,
                           MAX(utilization_pct) AS peak_utilization_pct
                      FROM twitch_eventsub_capacity_snapshot
                     WHERE ts_utc >= datetime('now', ?)
                     GROUP BY trigger_reason
                     ORDER BY samples DESC, trigger_reason ASC
                    """,
                    (lookback,),
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
            with storage.get_conn() as c:
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
            with storage.get_conn() as c:
                rows = c.execute(
                    """
                    SELECT s.twitch_user_id, s.twitch_login, a.scopes
                      FROM twitch_streamers_partner_state s
                      JOIN twitch_raid_auth a ON s.twitch_user_id = a.twitch_user_id
                     WHERE s.is_partner = 1
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
            with storage.get_conn() as c:
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
            with storage.get_conn() as c:
                row = c.execute(
                    """
                    SELECT is_live, last_seen_at, last_title, last_game, last_viewer_count,
                           last_stream_id, last_started_at, had_deadlock_in_session
                      FROM twitch_live_state
                     WHERE streamer_login = ?
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

    async def _on_eventsub_stream_offline(
        self, broadcaster_id: str, broadcaster_login: str | None
    ) -> None:
        """Direkter Auto-Raid-Trigger bei stream.offline EventSub."""
        if not broadcaster_id:
            return
        trigger_ts = time.monotonic()
        login_lower = (broadcaster_login or "").lower()
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
        if last_ts and now - last_ts < 90:
            return
        throttle[broadcaster_id] = now

        previous_state = self._load_live_state_row(login_lower)
        try:
            with storage.get_conn() as c:
                c.execute(
                    """
                    UPDATE twitch_live_state
                       SET is_live = 0,
                           last_seen_at = ?,
                           active_session_id = NULL
                     WHERE twitch_user_id = ?
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
        refresh = getattr(self, "_request_partner_raid_score_refresh", None)
        if callable(refresh):
            try:
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

    async def _cleanup_old_eventsub_subscriptions(self, webhook_url: str) -> None:
        """Löscht veraltete Webhook-Subscriptions vom letzten Start."""
        if not getattr(self, "api", None):
            return
        try:
            existing = await self.api.list_eventsub_subscriptions(status="enabled")
            deleted = 0
            for sub in existing:
                if sub.get("transport", {}).get("callback") == webhook_url:
                    sub_id = sub.get("id")
                    if sub_id:
                        await self.api.delete_eventsub_subscription(sub_id)
                        deleted += 1
            if deleted:
                log.info("EventSub Webhook: %d alte Subscriptions gelöscht", deleted)
        except Exception:
            log.exception("EventSub Webhook: Cleanup alter Subscriptions fehlgeschlagen")

    async def _start_eventsub_listener(self):
        """Startet Webhook-basierte EventSub Subscriptions."""
        if getattr(self, "_eventsub_started", False):
            log.debug("EventSub Listener bereits gestartet, überspringe.")
            return
        self._eventsub_started = True

        webhook_url = self._get_eventsub_webhook_url()
        webhook_secret = getattr(self, "_webhook_secret", None)
        webhook_handler = getattr(self, "_eventsub_webhook_handler", None)

        if not webhook_url or not webhook_secret or not webhook_handler:
            log.warning(
                "EventSub Webhook: TWITCH_WEBHOOK_SECRET nicht konfiguriert – "
                "EventSub Subscriptions werden nicht erstellt. "
                "Bitte TWITCH_WEBHOOK_SECRET setzen."
            )
            self._eventsub_started = False
            return

        if not getattr(self, "api", None):
            log.warning("EventSub Webhook: Keine API vorhanden, Listener wird nicht gestartet.")
            self._eventsub_started = False
            return

        try:
            await self.bot.wait_until_ready()
        except Exception:
            log.exception("EventSub Webhook: wait_until_ready fehlgeschlagen")
            self._eventsub_started = False
            return

        # Callbacks registrieren
        async def _offline_cb(bid: str, login: str, _event: dict):
            try:
                await self._on_eventsub_stream_offline(bid, login)
            except Exception:
                log.exception("EventSub Webhook: Offline-Callback fehlgeschlagen für %s", login)

        async def _raid_cb(to_bid: str, to_login: str, event: dict):
            try:
                raid_bot = getattr(self, "_raid_bot", None)
                if not raid_bot:
                    log.debug(
                        "EventSub Webhook: Raid-Bot nicht verfügbar für channel.raid von %s",
                        to_login,
                    )
                    return
                from_login = (event.get("from_broadcaster_user_login") or "").strip().lower()
                from_broadcaster_id = str(event.get("from_broadcaster_user_id") or "").strip()
                viewer_count = int(event.get("viewers") or 0)
                if not from_login:
                    log.warning(
                        "EventSub Webhook: channel.raid event ohne from_broadcaster_user_login"
                    )
                    return
                log.info(
                    "EventSub Webhook: channel.raid: %s -> %s (%d viewers)",
                    from_login,
                    to_login,
                    viewer_count,
                )
                await raid_bot.on_raid_arrival(
                    to_broadcaster_id=to_bid,
                    to_broadcaster_login=to_login,
                    from_broadcaster_login=from_login,
                    from_broadcaster_id=from_broadcaster_id,
                    viewer_count=viewer_count,
                )
            except Exception:
                log.exception(
                    "EventSub Webhook: Raid-Callback fehlgeschlagen für %s",
                    to_login or to_bid,
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

        async def _online_cb(bid: str, login: str, event: dict):
            try:
                await self._handle_stream_online(bid, login, event)
            except Exception:
                log.exception(
                    "EventSub Webhook: stream.online-Callback fehlgeschlagen für %s",
                    login,
                )

        async def _channel_update_cb(bid: str, login: str, event: dict):
            try:
                await self._handle_channel_update(bid, event)
            except Exception:
                log.exception(
                    "EventSub Webhook: channel.update-Callback fehlgeschlagen für %s",
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
                with storage.get_conn() as c:
                    c.execute(
                        """
                        INSERT INTO twitch_follow_events
                            (streamer_login, twitch_user_id, follower_login, follower_id, followed_at)
                        VALUES (?, ?, ?, ?, ?)
                        """,
                        (login, bid, user_login, user_id or None, followed_at),
                    )
            except Exception:
                log.exception("EventSub: _follow_cb – DB-Insert fehlgeschlagen für %s", login)

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

        webhook_handler.set_callback("stream.online", _online_cb)
        webhook_handler.set_callback("stream.offline", _offline_cb)
        webhook_handler.set_callback("channel.follow", _follow_cb)
        webhook_handler.set_callback("channel.raid", _raid_cb)
        webhook_handler.set_callback("channel.update", _channel_update_cb)
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

        # Go-Live Handler für Polling + EventSub (stream.online)
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

            try:
                # 1. Chat-Bot joinen (falls Partner mit Chat-Scope)
                chat_bot = getattr(self, "_twitch_chat_bot", None)
                if chat_bot:
                    login_norm = login or ""
                    if not login_norm:
                        try:
                            with storage.get_conn() as c:
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

                # 2. stream.offline Webhook Subscription (nur bei vollständiger Auth)
                # Webhook-Subscriptions erfordern App-Access-Token → oauth_token=None
                fully_authed = (
                    await self._is_fully_authed(str(bid))
                    if hasattr(self, "_is_fully_authed")
                    else True
                )
                if fully_authed:
                    if self._eventsub_has_sub("stream.offline", str(bid)):
                        log.debug(
                            "Polling: stream.offline bereits subscribed für %s, überspringe",
                            login or bid,
                        )
                    else:
                        result = await self.api.subscribe_eventsub_webhook(
                            sub_type="stream.offline",
                            condition={"broadcaster_user_id": str(bid)},
                            webhook_url=webhook_url,
                            secret=webhook_secret,
                            oauth_token=None,
                        )
                        if result:
                            log.info(
                                "Polling: stream.offline Webhook Subscription erstellt für %s",
                                login or bid,
                            )
                            self._eventsub_track_sub("stream.offline", str(bid))
                            await self._record_eventsub_capacity_snapshot(
                                "stream_offline_subscribed", force=True
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

                # 3. Broadcaster-Token Subscriptions (Bits, Hype, Subs, Ads, Channel Points)
                broadcaster_token = await self._resolve_eventsub_broadcaster_token(str(bid))
                token_scopes: set[str] = set()
                if broadcaster_token:
                    # Scopes des Tokens aus DB laden – nur Subs subscriben, für die der Scope vorhanden ist
                    try:
                        with storage.get_conn() as _sc:
                            _scope_row = _sc.execute(
                                "SELECT scopes FROM twitch_raid_auth WHERE twitch_user_id = ?",
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

        # 1. Alte Subscriptions bereinigen
        await self._cleanup_old_eventsub_subscriptions(webhook_url)

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
            return

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

        # 4. stream.online + stream.offline + channel.update für alle/live Streamer
        offline_added = 0
        online_added = 0
        update_added = 0
        for entry in raid_enabled_streamers:
            bid = entry.get("twitch_user_id")
            login = entry.get("twitch_login", "").lower()
            if not bid:
                continue

            # stream.online für ALLE (so erkennen wir Go-Live sofort statt per 15s-Polling)
            # Webhook-Subscriptions erfordern einen App-Access-Token (client_credentials),
            # daher oauth_token=None → TwitchAPI nutzt automatisch den App-Token.
            try:
                result = await self.api.subscribe_eventsub_webhook(
                    sub_type="stream.online",
                    condition={"broadcaster_user_id": str(bid)},
                    webhook_url=webhook_url,
                    secret=webhook_secret,
                    oauth_token=None,
                )
                if result:
                    self._eventsub_track_sub("stream.online", str(bid))
                    online_added += 1
            except Exception:
                log.debug(
                    "EventSub Webhook: stream.online fehlgeschlagen für %s",
                    login,
                    exc_info=True,
                )

            # channel.update für ALLE (Titel/Game-Änderungen mitbekommen)
            try:
                result = await self.api.subscribe_eventsub_webhook(
                    sub_type="channel.update",
                    condition={"broadcaster_user_id": str(bid)},
                    webhook_url=webhook_url,
                    secret=webhook_secret,
                    oauth_token=None,
                    version="2",
                )
                if result:
                    self._eventsub_track_sub("channel.update", str(bid))
                    update_added += 1
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
                result = await self.api.subscribe_eventsub_webhook(
                    sub_type="stream.offline",
                    condition={"broadcaster_user_id": str(bid)},
                    webhook_url=webhook_url,
                    secret=webhook_secret,
                    oauth_token=None,
                )
                if result:
                    self._eventsub_track_sub("stream.offline", str(bid))
                    offline_added += 1
                    log.debug("EventSub Webhook: stream.offline subscribed für %s", login)
            except Exception:
                log.exception("EventSub Webhook: stream.offline fehlgeschlagen für %s", login)

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
        """Prüft ob eine Webhook-Subscription bereits in dieser Session registriert wurde."""
        tracked: set = getattr(self, "_eventsub_webhook_tracked", None)
        if tracked is None:
            return False
        return (sub_type, str(broadcaster_user_id)) in tracked

    def _eventsub_track_sub(self, sub_type: str, broadcaster_user_id: str) -> None:
        """Merkt sich eine aktive Webhook-Subscription für spätere Capacity-Snapshots."""
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
            callback = str(transport.get("callback") or "").strip()
            if callback and callback != str(webhook_url).strip():
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

        webhook_url = self._get_eventsub_webhook_url()
        webhook_secret = getattr(self, "_webhook_secret", None)
        if not webhook_url or not webhook_secret:
            return False, "missing_webhook_config"
        if not getattr(self, "api", None):
            return False, "no_api"

        current_status = await self._get_eventsub_webhook_subscription_status(
            sub_type="channel.raid",
            broadcaster_id=target_id,
            webhook_url=webhook_url,
        )
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
        Erstellt dynamisch eine channel.raid Webhook-Subscription für einen Broadcaster.

        Wird aufgerufen wenn ein Raid gestartet wird, um zu erkennen wenn der Raid ankommt.

        Returns:
            True wenn die Subscription erfolgreich erstellt wurde, False sonst.
        """
        webhook_url = self._get_eventsub_webhook_url()
        webhook_secret = getattr(self, "_webhook_secret", None)

        if not webhook_url or not webhook_secret:
            log.error(
                "EventSub Webhook: Keine Webhook-URL/Secret konfiguriert für channel.raid subscription"
            )
            return False

        if not getattr(self, "api", None):
            log.error("EventSub Webhook: Keine API verfügbar für channel.raid subscription")
            await self._record_eventsub_capacity_snapshot("raid_no_api", force=True)
            return False

        try:
            result = await self.api.subscribe_eventsub_webhook(
                sub_type="channel.raid",
                condition={"to_broadcaster_user_id": str(broadcaster_id)},
                webhook_url=webhook_url,
                secret=webhook_secret,
                oauth_token=None,  # Webhook-Subscriptions benötigen App-Access-Token
            )
            if result:
                self._eventsub_track_sub("channel.raid", str(broadcaster_id))
                log.info(
                    "EventSub Webhook: channel.raid Subscription erstellt für %s (ID: %s)",
                    broadcaster_login,
                    broadcaster_id,
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

    async def _start_eventsub_offline_listener(self):
        """Kompatibilitäts-Stub (wird nun über _start_eventsub_listener erledigt)."""
        await self._start_eventsub_listener()

    async def _start_eventsub_online_listener(self):
        """Kompatibilitäts-Stub (wird nun über _start_eventsub_listener erledigt)."""
        pass
