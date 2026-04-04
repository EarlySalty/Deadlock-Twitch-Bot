"""Analytics background tasks for Twitch."""

from __future__ import annotations

import asyncio
import json
import logging
import logging.handlers
import time
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

from discord.ext import tasks

from ..core.chat_bots import build_known_chat_bot_not_in_clause, is_known_chat_bot
from ..core.partner_utils import is_operational_partner_channel
from ..logging_setup import log_path
from .. import storage as storage_runtime
from ..storage import pg as storage

# NOTE: Twitch game-owner analytics are intentionally unused.
# The data is global and tied to owned/managed games, not to individual
# streamer channels, so we do not request or query it in this product.

log = logging.getLogger("TwitchStreams.Analytics")
_IRC_EXPERIMENT_LOG = logging.getLogger("TwitchStreams.Analytics.IRCLurkerExperiment")
_IRC_EXPERIMENT_LOG_FILE = log_path("twitch_irc_lurker_experiment.log")
_CHATTERS_STARTUP_GRACE_SECONDS = 45.0


def _ensure_irc_experiment_logger() -> logging.Logger:
    formatter = logging.Formatter("%(asctime)s - %(name)s - %(levelname)s - %(message)s")
    target = Path(_IRC_EXPERIMENT_LOG_FILE)
    for handler in _IRC_EXPERIMENT_LOG.handlers:
        if isinstance(handler, logging.handlers.RotatingFileHandler):
            try:
                if Path(str(getattr(handler, "baseFilename", ""))).resolve() == target.resolve():
                    handler.setFormatter(formatter)
                    if handler.level > logging.INFO:
                        handler.setLevel(logging.INFO)
                    return _IRC_EXPERIMENT_LOG
            except Exception:
                continue
    handler = logging.handlers.RotatingFileHandler(
        target,
        maxBytes=5 * 1024 * 1024,
        backupCount=5,
        encoding="utf-8",
    )
    handler.setFormatter(formatter)
    handler.setLevel(logging.INFO)
    _IRC_EXPERIMENT_LOG.addHandler(handler)
    if _IRC_EXPERIMENT_LOG.level == logging.NOTSET or _IRC_EXPERIMENT_LOG.level > logging.INFO:
        _IRC_EXPERIMENT_LOG.setLevel(logging.INFO)
    _IRC_EXPERIMENT_LOG.propagate = True
    return _IRC_EXPERIMENT_LOG


class TwitchAnalyticsMixin:
    """
    Mixin for periodic analytics collection (Subs, Ads, etc.).
    Requires authorized OAuth tokens and matching scopes.
    """

    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        # Warn about missing chatters scope only once per live session to avoid log spam
        self._chatters_scope_warned: set[tuple[str, int]] = set()
        self._chatters_user_fallback_warned: set[tuple[str, int]] = set()
        self._analytics_observability_counter_store: dict[str, int] = {}
        self._last_chatters_diagnostic: dict[str, object] | None = None
        self._last_followers_diagnostic: dict[str, object] | None = None
        self._last_analytics_decision_sample: dict[str, object] | None = None
        self._irc_lurker_experiment_session_stats: dict[int, dict[str, object]] = {}
        self._chatters_startup_grace_started_at = time.monotonic()
        self._chatters_startup_deferral_logged = False
        self._moderator_self_heal_cooldown_until: dict[tuple[str, str], float] = {}
        _ensure_irc_experiment_logger()
        self._analytics_task = self.collect_analytics_data.start()
        self._chatters_task = self.collect_chatters_data.start()
        self._retention_task = self.compute_raid_retention.start()

    async def cog_unload(self):
        await super().cog_unload()
        self.collect_analytics_data.cancel()
        self.collect_chatters_data.cancel()
        self.compute_raid_retention.cancel()

    def _analytics_observability_counters(self) -> dict[str, int]:
        counters = getattr(self, "_analytics_observability_counter_store", None)
        if not isinstance(counters, dict):
            counters = {}
            self._analytics_observability_counter_store = counters
        return counters

    def _moderator_self_heal_cooldowns(self) -> dict[tuple[str, str], float]:
        cooldowns = getattr(self, "_moderator_self_heal_cooldown_until", None)
        if not isinstance(cooldowns, dict):
            cooldowns = {}
            self._moderator_self_heal_cooldown_until = cooldowns
        return cooldowns

    @staticmethod
    def _moderator_self_heal_key(login: str, required_scope: str) -> tuple[str, str]:
        return (
            str(login or "").strip().lower().lstrip("#"),
            str(required_scope or "").strip().lower(),
        )

    def _is_moderator_self_heal_target(self, login: str) -> bool:
        normalized_login = str(login or "").strip().lower().lstrip("#")
        if not normalized_login:
            return False
        try:
            return bool(is_operational_partner_channel(normalized_login))
        except Exception:
            log.debug(
                "Analytics self-heal: partner target check failed for %s",
                normalized_login,
                exc_info=True,
            )
            return False

    async def _attempt_bot_moderator_self_heal(
        self,
        *,
        broadcaster_id: str,
        login: str,
        required_scope: str,
        flow: str,
    ) -> bool:
        key = self._moderator_self_heal_key(login, required_scope)
        now = time.monotonic()
        cooldowns = self._moderator_self_heal_cooldowns()
        cooldown_until = float(cooldowns.get(key, 0.0) or 0.0)
        if cooldown_until > now:
            return False
        if not self._is_moderator_self_heal_target(login):
            return False

        chat_bot = getattr(self, "_twitch_chat_bot", None)
        ensure_bot_is_mod = getattr(chat_bot, "_ensure_bot_is_mod", None)
        if not callable(ensure_bot_is_mod):
            return False

        try:
            mod_restored = bool(await ensure_bot_is_mod(str(broadcaster_id), str(login)))
        except Exception:
            log.debug(
                "Analytics self-heal: auto-re-mod failed for %s (%s)",
                login,
                flow,
                exc_info=True,
            )
            mod_restored = False

        if mod_restored:
            cooldowns.pop(key, None)
            self._increment_analytics_observability_counter(
                f"{flow}_moderator_self_heal_success_total"
            )
            return True

        cooldowns[key] = now + 600.0
        self._increment_analytics_observability_counter(
            f"{flow}_moderator_self_heal_failure_total"
        )
        return False

    def _restore_bot_ban_opt_out_if_healthy(
        self,
        *,
        twitch_user_id: str,
        login: str,
        flow: str,
    ) -> None:
        raid_bot = getattr(self, "_raid_bot", None)
        auth_manager = getattr(raid_bot, "auth_manager", None) if raid_bot else None
        token_error_handler = (
            getattr(auth_manager, "token_error_handler", None) if auth_manager else None
        )
        restore = getattr(token_error_handler, "restore_bot_banned_channel", None)
        if not callable(restore):
            return
        try:
            restored = bool(restore(str(twitch_user_id), str(login)))
        except Exception:
            log.debug(
                "Analytics self-heal: could not restore technical bot-ban opt-out for %s (%s)",
                login,
                flow,
                exc_info=True,
            )
            return
        if restored:
            self._increment_analytics_observability_counter(
                f"{flow}_bot_ban_opt_out_restore_total"
            )

    @staticmethod
    def _analytics_login_sample(values: set[str], *, limit: int = 12) -> list[str]:
        return sorted(str(value or "").strip().lower() for value in values if str(value or "").strip())[
            :limit
        ]

    def _record_irc_lurker_experiment_sample(
        self,
        *,
        login: str,
        session_id: int,
        now_iso: str,
        helix_chatters: list[dict[str, object]],
    ) -> None:
        if not getattr(self, "_experimental_irc_lurker_enabled", False):
            return

        tracker = getattr(self, "_irc_lurker_tracker", None)
        if tracker is None or not hasattr(tracker, "get_chatters"):
            return

        allowlist = {
            str(channel or "").strip().lower().lstrip("#")
            for channel in (getattr(self, "_experimental_irc_lurker_channels", set()) or set())
            if str(channel or "").strip()
        }
        if not allowlist:
            return

        login_lower = str(login or "").strip().lower().lstrip("#")
        if login_lower not in allowlist:
            return

        helix_set = {
            str(
                entry.get("user_login")
                or entry.get("chatter_login")
                or entry.get("login")
                or ""
            )
            .strip()
            .lower()
            for entry in (helix_chatters or [])
            if isinstance(entry, dict)
        }
        helix_set.discard("")
        try:
            irc_set = {
                str(value or "").strip().lower()
                for value in (tracker.get_chatters(login_lower) or set())
                if str(value or "").strip()
            }
        except Exception:
            log.debug("IRC experiment: get_chatters failed for %s", login_lower, exc_info=True)
            return

        overlap = helix_set & irc_set
        helix_only = helix_set - irc_set
        irc_only = irc_set - helix_set
        comparison_store = getattr(self, "_irc_lurker_experiment_session_stats", None)
        if not isinstance(comparison_store, dict):
            comparison_store = {}
            self._irc_lurker_experiment_session_stats = comparison_store
        stats = comparison_store.get(session_id)
        if not isinstance(stats, dict):
            stats = {
                "login": login_lower,
                "first_sample_at": now_iso,
                "last_sample_at": now_iso,
                "sample_count": 0,
                "equal_sample_count": 0,
                "helix_led_sample_count": 0,
                "irc_led_sample_count": 0,
                "helix_total_sum": 0,
                "irc_total_sum": 0,
                "overlap_total_sum": 0,
                "helix_only_total_sum": 0,
                "irc_only_total_sum": 0,
                "max_helix_count": 0,
                "max_irc_count": 0,
                "max_overlap_count": 0,
                "max_helix_only_count": 0,
                "max_irc_only_count": 0,
                "distinct_helix": set(),
                "distinct_irc": set(),
                "distinct_overlap": set(),
                "distinct_helix_only": set(),
                "distinct_irc_only": set(),
            }
            comparison_store[session_id] = stats

        helix_count = len(helix_set)
        irc_count = len(irc_set)
        overlap_count = len(overlap)
        helix_only_count = len(helix_only)
        irc_only_count = len(irc_only)

        stats["last_sample_at"] = now_iso
        stats["sample_count"] = int(stats.get("sample_count", 0) or 0) + 1
        if helix_count == irc_count:
            stats["equal_sample_count"] = int(stats.get("equal_sample_count", 0) or 0) + 1
        elif helix_count > irc_count:
            stats["helix_led_sample_count"] = int(stats.get("helix_led_sample_count", 0) or 0) + 1
        else:
            stats["irc_led_sample_count"] = int(stats.get("irc_led_sample_count", 0) or 0) + 1
        stats["helix_total_sum"] = int(stats.get("helix_total_sum", 0) or 0) + helix_count
        stats["irc_total_sum"] = int(stats.get("irc_total_sum", 0) or 0) + irc_count
        stats["overlap_total_sum"] = int(stats.get("overlap_total_sum", 0) or 0) + overlap_count
        stats["helix_only_total_sum"] = int(stats.get("helix_only_total_sum", 0) or 0) + helix_only_count
        stats["irc_only_total_sum"] = int(stats.get("irc_only_total_sum", 0) or 0) + irc_only_count
        stats["max_helix_count"] = max(int(stats.get("max_helix_count", 0) or 0), helix_count)
        stats["max_irc_count"] = max(int(stats.get("max_irc_count", 0) or 0), irc_count)
        stats["max_overlap_count"] = max(int(stats.get("max_overlap_count", 0) or 0), overlap_count)
        stats["max_helix_only_count"] = max(
            int(stats.get("max_helix_only_count", 0) or 0),
            helix_only_count,
        )
        stats["max_irc_only_count"] = max(
            int(stats.get("max_irc_only_count", 0) or 0),
            irc_only_count,
        )
        stats["distinct_helix"].update(helix_set)
        stats["distinct_irc"].update(irc_set)
        stats["distinct_overlap"].update(overlap)
        stats["distinct_helix_only"].update(helix_only)
        stats["distinct_irc_only"].update(irc_only)

        _IRC_EXPERIMENT_LOG.info(
            "irc_vs_helix_sample %s",
            self._format_analytics_observability_fields(
                login=login_lower,
                session_id=session_id,
                sample_at=now_iso,
                helix_count=helix_count,
                irc_count=irc_count,
                overlap_count=overlap_count,
                helix_only_count=helix_only_count,
                irc_only_count=irc_only_count,
                helix_only_sample=self._analytics_login_sample(helix_only),
                irc_only_sample=self._analytics_login_sample(irc_only),
            ),
        )

    def _finalize_irc_lurker_experiment_session(
        self,
        *,
        login: str,
        session_id: int,
        reason: str,
        ended_at: datetime,
    ) -> None:
        comparison_store = getattr(self, "_irc_lurker_experiment_session_stats", None)
        if not isinstance(comparison_store, dict):
            return
        stats = comparison_store.pop(int(session_id), None)
        if not isinstance(stats, dict):
            return

        def _safe_avg(total_key: str, count_key: str = "sample_count") -> float:
            divisor = int(stats.get(count_key, 0) or 0)
            if divisor <= 0:
                return 0.0
            return round(float(stats.get(total_key, 0) or 0) / divisor, 2)

        distinct_helix = set(stats.get("distinct_helix", set()) or set())
        distinct_irc = set(stats.get("distinct_irc", set()) or set())
        distinct_overlap = set(stats.get("distinct_overlap", set()) or set())
        distinct_helix_only = set(stats.get("distinct_helix_only", set()) or set())
        distinct_irc_only = set(stats.get("distinct_irc_only", set()) or set())
        distinct_union_count = len(distinct_helix | distinct_irc)
        distinct_overlap_count = len(distinct_overlap)
        distinct_jaccard = (
            round(distinct_overlap_count / distinct_union_count, 3)
            if distinct_union_count > 0
            else 0.0
        )
        _IRC_EXPERIMENT_LOG.info(
            "irc_vs_helix_summary %s",
            self._format_analytics_observability_fields(
                login=str(login or "").strip().lower().lstrip("#"),
                session_id=session_id,
                reason=reason,
                first_sample_at=stats.get("first_sample_at"),
                last_sample_at=stats.get("last_sample_at"),
                ended_at=ended_at.isoformat(timespec="seconds"),
                sample_count=stats.get("sample_count"),
                equal_sample_count=stats.get("equal_sample_count"),
                helix_led_sample_count=stats.get("helix_led_sample_count"),
                irc_led_sample_count=stats.get("irc_led_sample_count"),
                avg_helix_count=_safe_avg("helix_total_sum"),
                avg_irc_count=_safe_avg("irc_total_sum"),
                avg_overlap_count=_safe_avg("overlap_total_sum"),
                avg_helix_only_count=_safe_avg("helix_only_total_sum"),
                avg_irc_only_count=_safe_avg("irc_only_total_sum"),
                max_helix_count=stats.get("max_helix_count"),
                max_irc_count=stats.get("max_irc_count"),
                max_overlap_count=stats.get("max_overlap_count"),
                max_helix_only_count=stats.get("max_helix_only_count"),
                max_irc_only_count=stats.get("max_irc_only_count"),
                distinct_helix_count=len(distinct_helix),
                distinct_irc_count=len(distinct_irc),
                distinct_overlap_count=distinct_overlap_count,
                distinct_helix_only_count=len(distinct_helix_only),
                distinct_irc_only_count=len(distinct_irc_only),
                distinct_jaccard=distinct_jaccard,
                helix_only_sample=self._analytics_login_sample(distinct_helix_only),
                irc_only_sample=self._analytics_login_sample(distinct_irc_only),
            ),
        )

    def _increment_analytics_observability_counter(self, name: str, amount: int = 1) -> int:
        counter_name = str(name or "").strip()
        if not counter_name:
            return 0
        counters = self._analytics_observability_counters()
        counters[counter_name] = int(counters.get(counter_name, 0) or 0) + int(amount)
        return counters[counter_name]

    def _next_analytics_observability_flow_id(self, prefix: str) -> str:
        normalized = str(prefix or "analytics").strip().lower() or "analytics"
        sequence = int(getattr(self, "_analytics_observability_sequence", 0) or 0) + 1
        self._analytics_observability_sequence = sequence
        return f"{normalized}-{int(time.time() * 1000)}-{sequence}"

    @staticmethod
    def _analytics_observability_value(value: object, *, limit: int = 240) -> str:
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
        return f"{text[:limit]}..." if len(text) > limit else text

    def _format_analytics_observability_fields(self, **fields: object) -> str:
        parts = []
        for key in sorted(fields):
            value = fields[key]
            if value is None:
                continue
            parts.append(
                f"{str(key).strip()}={self._analytics_observability_value(value)}"
            )
        return " ".join(parts)

    @staticmethod
    def _scope_presence_state(
        *,
        scopes: set[str] | None,
        required_scope: str,
        token_available: bool,
    ) -> str:
        if not token_available:
            return "absent"
        normalized_scopes = {str(scope).strip().lower() for scope in (scopes or set()) if str(scope).strip()}
        if not normalized_scopes:
            return "unknown"
        return "present" if required_scope in normalized_scopes else "missing"

    @staticmethod
    def _structured_result_meta(result: dict[str, object] | None) -> tuple[str, int | None, str | None]:
        payload = result or {}
        request_result = "success" if payload.get("ok") else "failed"
        http_status_raw = payload.get("http_status")
        try:
            http_status = int(http_status_raw) if http_status_raw is not None else None
        except (TypeError, ValueError):
            http_status = None
        error_code = str(payload.get("error_code") or "").strip() or None
        return request_result, http_status, error_code

    def _build_analytics_runtime_state(self, login: str | None = None) -> dict[str, object]:
        normalized_login = str(login or "").strip().lower().lstrip("#")
        runtime_sources = []
        collect_sources = getattr(self, "_collect_bot_chatters_runtime_sources", None)
        if normalized_login and callable(collect_sources):
            try:
                runtime_sources = sorted(collect_sources(normalized_login))
            except Exception:
                log.debug(
                    "Analytics runtime source collection failed for %s",
                    normalized_login,
                    exc_info=True,
                )
        return {
            "analytics_runtime_available": bool(getattr(self, "api", None)),
            "chat_bot_available": bool(getattr(self, "_twitch_chat_bot", None)),
            "bot_token_manager_available": bool(getattr(self, "_bot_token_manager", None)),
            "raid_bot_available": bool(getattr(self, "_raid_bot", None)),
            "runtime_sources": runtime_sources,
        }

    async def _get_chatters_result_with_legacy_fallback(
        self,
        *,
        broadcaster_id: str,
        moderator_id: str,
        user_token: str,
    ) -> dict[str, object]:
        result_getter = getattr(self.api, "get_chatters_result", None)
        if callable(result_getter):
            return await result_getter(
                broadcaster_id=broadcaster_id,
                moderator_id=moderator_id,
                user_token=user_token,
            )
        legacy_chatters = await self.api.get_chatters(
            broadcaster_id=broadcaster_id,
            moderator_id=moderator_id,
            user_token=user_token,
        )
        return {
            "ok": isinstance(legacy_chatters, list),
            "data": legacy_chatters,
            "http_status": 200 if isinstance(legacy_chatters, list) else None,
            "error_code": None if isinstance(legacy_chatters, list) else "legacy_none_result",
            "request_attempted": True,
        }

    async def _get_subscriptions_result_with_legacy_fallback(
        self,
        *,
        user_id: str,
        user_token: str,
    ) -> dict[str, object]:
        result_getter = getattr(self.api, "get_broadcaster_subscriptions_result", None)
        if callable(result_getter):
            return await result_getter(user_id, user_token)
        legacy_payload = await self.api.get_broadcaster_subscriptions(user_id, user_token)
        return {
            "ok": isinstance(legacy_payload, dict),
            "data": legacy_payload,
            "http_status": 200 if isinstance(legacy_payload, dict) else None,
            "error_code": None if isinstance(legacy_payload, dict) else "legacy_none_result",
            "request_attempted": True,
        }

    async def _get_ad_schedule_result_with_legacy_fallback(
        self,
        *,
        user_id: str,
        user_token: str,
    ) -> dict[str, object]:
        result_getter = getattr(self.api, "get_ad_schedule_result", None)
        if callable(result_getter):
            return await result_getter(user_id, user_token)
        legacy_payload = await self.api.get_ad_schedule(user_id, user_token)
        return {
            "ok": isinstance(legacy_payload, dict),
            "data": legacy_payload,
            "http_status": 200 if isinstance(legacy_payload, dict) else None,
            "error_code": None if isinstance(legacy_payload, dict) else "legacy_none_result",
            "request_attempted": True,
        }

    def _store_analytics_diagnostic(self, flow: str, payload: dict[str, object]) -> None:
        flow_key = str(flow or "").strip().lower()
        self._last_analytics_decision_sample = payload
        if flow_key == "chatters":
            self._last_chatters_diagnostic = payload
        if "followers" in flow_key:
            self._last_followers_diagnostic = payload

    @staticmethod
    def _analytics_decision_log_level(
        *,
        flow: str,
        decision: str,
        reason: str,
        level: int,
    ) -> int:
        flow_key = str(flow or "").strip().lower()
        decision_key = str(decision or "").strip().lower()
        reason_key = str(reason or "").strip().lower()
        if (
            level == logging.INFO
            and flow_key == "chatters"
            and (
                (decision_key == "failed" and reason_key in {
                    "chat_bot_unavailable",
                    "channel_not_tracked_in_chat_runtime",
                    "helix_403_not_moderator",
                })
                or (decision_key == "success" and reason_key == "bot_path_success")
            )
        ):
            return logging.DEBUG
        if (
            level == logging.INFO
            and decision_key == "success"
            and (
                (flow_key == "subscriptions" and reason_key == "subscriptions_collected")
                or (flow_key == "ads" and reason_key == "ads_collected")
            )
        ):
            return logging.DEBUG
        return level

    def _log_analytics_decision(
        self,
        *,
        flow_id: str,
        flow: str,
        login: str,
        session_id: int | None = None,
        decision: str,
        reason: str,
        request_attempted: object,
        request_result: str,
        http_status: int | None,
        scope_state: dict[str, object],
        runtime_state: dict[str, object],
        level: int = logging.INFO,
        **extra_fields: object,
    ) -> dict[str, object]:
        payload: dict[str, object] = {
            "flow_id": str(flow_id or "").strip() or None,
            "flow": str(flow or "").strip().lower() or "analytics",
            "login": str(login or "").strip().lower().lstrip("#") or None,
            "session_id": int(session_id) if session_id is not None else None,
            "decision": str(decision or "").strip() or "unknown",
            "reason": str(reason or "").strip() or "unknown",
            "request_attempted": request_attempted,
            "request_result": str(request_result or "").strip() or "unknown",
            "http_status": int(http_status) if http_status is not None else None,
            "scope_state": scope_state,
            "runtime_state": runtime_state,
            **extra_fields,
        }
        self._store_analytics_diagnostic(str(payload.get("flow") or ""), payload)
        effective_level = self._analytics_decision_log_level(
            flow=str(payload.get("flow") or ""),
            decision=str(payload.get("decision") or ""),
            reason=str(payload.get("reason") or ""),
            level=level,
        )
        log.log(
            effective_level,
            "analytics_decision %s",
            self._format_analytics_observability_fields(**payload),
        )
        storage.insert_observability_event(
            flow_type="analytics",
            flow_id=str(payload.get("flow_id") or ""),
            entity_login=str(payload.get("login") or ""),
            entity_id=str(payload.get("session_id") or ""),
            step="terminal_decision",
            decision=str(payload.get("decision") or "unknown"),
            details=payload,
        )
        return payload

    def get_analytics_observability_snapshot(self) -> dict[str, Any]:
        return {
            "runtimeAvailable": bool(getattr(self, "api", None)),
            "chatBotAvailable": bool(getattr(self, "_twitch_chat_bot", None)),
            "botTokenManagerAvailable": bool(getattr(self, "_bot_token_manager", None)),
            "counters": dict(self._analytics_observability_counters()),
            "lastChattersDiagnostic": getattr(self, "_last_chatters_diagnostic", None),
            "lastFollowersDiagnostic": getattr(self, "_last_followers_diagnostic", None),
            "lastDecisionSample": getattr(self, "_last_analytics_decision_sample", None),
        }

    @tasks.loop(hours=6)
    async def collect_analytics_data(self):
        """
        Periodically collect analytics data for authorized streamers.
        Runs every 6 hours to avoid API spam, as these numbers don't change extremely fast.
        """
        if not self.api:
            return

        try:
            await self.bot.wait_until_ready()
        except Exception:
            return

        log.debug("Starting analytics collection (Subs + Ads)...")

        # Get authorized users with raid_enabled=1 (assuming they granted scopes)
        # Note: We should actually check if they have the specific scope,
        # but for now we assume the new scope set is used if they re-authed.
        try:
            with storage_runtime.transaction() as conn:
                rows = conn.execute(
                    """
                    SELECT twitch_user_id, twitch_login
                    FROM twitch_raid_auth
                    WHERE raid_enabled IS TRUE
                    """
                ).fetchall()
        except Exception:
            log.exception("Failed to load authorized users for analytics")
            return

        users_processed = 0
        subs_snapshots = 0
        ads_snapshots = 0
        for row in rows:
            user_id = row[0] if not hasattr(row, "keys") else row["twitch_user_id"]
            login = row[1] if not hasattr(row, "keys") else row["twitch_login"]

            # Use RaidBot's auth manager to get a fresh token if possible
            if not getattr(self, "_raid_bot", None):
                continue

            session = self.api.get_http_session()
            token = await self._raid_bot.auth_manager.get_valid_token(user_id, session)

            if not token:
                log.debug("Skipping analytics collection: no valid authorization available.")
                continue

            scopes = {s.lower() for s in self._raid_bot.auth_manager.get_scopes(user_id)}
            did_collect_for_user = False

            try:
                if "channel:read:subscriptions" in scopes:
                    if await self._collect_subs_for_user(user_id, login, token):
                        subs_snapshots += 1
                        did_collect_for_user = True

                if "channel:read:ads" in scopes:
                    if await self._collect_ads_schedule_for_user(user_id, login, token):
                        ads_snapshots += 1
                        did_collect_for_user = True
            except Exception:
                log.exception("Failed to collect analytics for %s", login)

            if did_collect_for_user:
                users_processed += 1
                # Sleep to be nice to the API
                await asyncio.sleep(2)
            else:
                log.debug(
                    "Skipping analytics metrics for %s: missing scopes (need channel:read:subscriptions and/or channel:read:ads).",
                    login,
                )

        log.debug(
            "Analytics collection finished. users=%d, subs_snapshots=%d, ads_snapshots=%d",
            users_processed,
            subs_snapshots,
            ads_snapshots,
        )

    async def _collect_subs_for_user(self, user_id: str, login: str, token: str) -> bool:
        """Fetch and store subscription data."""
        flow_id = self._next_analytics_observability_flow_id("subscriptions")
        result = await self._get_subscriptions_result_with_legacy_fallback(
            user_id=user_id,
            user_token=token,
        )
        data = result.get("data") if isinstance(result.get("data"), dict) else None
        request_result, http_status, error_code = self._structured_result_meta(result)
        if not result.get("ok") or not data:
            reason = error_code or "helix_subscriptions_failed"
            self._increment_analytics_observability_counter("subscriptions_request_failure_total")
            self._increment_analytics_observability_counter(
                f"subscriptions_reason_{reason}_total"
            )
            self._log_analytics_decision(
                flow_id=flow_id,
                flow="subscriptions",
                login=login,
                decision="failed",
                reason=reason,
                request_attempted=result.get("request_attempted"),
                request_result=request_result,
                http_status=http_status,
                scope_state={"streamer": "present"},
                runtime_state=self._build_analytics_runtime_state(login),
                request_message=result.get("message"),
            )
            return False

        total = int(data.get("total", 0))
        points = int(data.get("points", 0))

        # Determine breakdown from 'data' list if available (depends on API response pagination,
        # usually getting 'total' is enough for the headline number.
        # Detailed breakdown per tier might require iterating all pages which is expensive.
        # For now, we store total and points.

        # Twitch API /subscriptions returns a list of sub objects.
        # "total" field in the response represents the total number of subscriptions.
        # "points" is also returned in the response root.

        # We can try to approximate tiers if we only fetch the first page, but 'total' is exact.

        now_iso = datetime.now(UTC).isoformat()

        with storage.transaction() as conn:
            conn.execute(
                """
                INSERT INTO twitch_subscriptions_snapshot
                (twitch_user_id, twitch_login, total, points, snapshot_at)
                VALUES (%s, %s, %s, %s, %s)
                """,
                (user_id, login, total, points, now_iso),
            )
        self._increment_analytics_observability_counter("subscriptions_request_success_total")
        self._log_analytics_decision(
            flow_id=flow_id,
            flow="subscriptions",
            login=login,
            decision="success",
            reason="subscriptions_collected",
            request_attempted=result.get("request_attempted"),
            request_result=request_result,
            http_status=http_status,
            scope_state={"streamer": "present"},
            runtime_state=self._build_analytics_runtime_state(login),
            snapshot_total=total,
            snapshot_points=points,
        )
        return True

    async def _collect_ads_schedule_for_user(self, user_id: str, login: str, token: str) -> bool:
        """Fetch and store ad schedule data."""
        flow_id = self._next_analytics_observability_flow_id("ads")
        result = await self._get_ad_schedule_result_with_legacy_fallback(
            user_id=user_id,
            user_token=token,
        )
        data = result.get("data") if isinstance(result.get("data"), dict) else None
        request_result, http_status, error_code = self._structured_result_meta(result)
        if not result.get("ok") or not data:
            reason = error_code or "helix_ads_failed"
            self._increment_analytics_observability_counter("ads_request_failure_total")
            self._increment_analytics_observability_counter(f"ads_reason_{reason}_total")
            self._log_analytics_decision(
                flow_id=flow_id,
                flow="ads",
                login=login,
                decision="failed",
                reason=reason,
                request_attempted=result.get("request_attempted"),
                request_result=request_result,
                http_status=http_status,
                scope_state={"streamer": "present"},
                runtime_state=self._build_analytics_runtime_state(login),
                request_message=result.get("message"),
            )
            return False

        def _safe_int(value):
            if value is None:
                return None
            try:
                return int(value)
            except (TypeError, ValueError):
                return None

        def _safe_time_text(value):
            if value is None:
                return None
            if isinstance(value, str):
                return value.strip() or None
            if isinstance(value, (int, float)):
                ts = float(value)
                if ts <= 0:
                    return None
                # Some APIs occasionally return milliseconds; normalize to seconds.
                if ts > 10_000_000_000:
                    ts = ts / 1000.0
                try:
                    return datetime.fromtimestamp(ts, tz=UTC).isoformat()
                except (OverflowError, OSError, ValueError):
                    return str(int(ts))
            text = str(value).strip()
            return text or None

        now_iso = datetime.now(UTC).isoformat()
        next_ad_at = _safe_time_text(data.get("next_ad_at"))
        last_ad_at = _safe_time_text(data.get("last_ad_at"))
        duration = _safe_int(data.get("duration"))
        preroll_free_time = _safe_int(data.get("preroll_free_time"))
        snooze_count = _safe_int(data.get("snooze_count"))
        snooze_refresh_at = _safe_time_text(data.get("snooze_refresh_at"))

        with storage.transaction() as conn:
            conn.execute(
                """
                INSERT INTO twitch_ads_schedule_snapshot
                (
                    twitch_user_id, twitch_login, next_ad_at, last_ad_at,
                    duration, preroll_free_time, snooze_count, snooze_refresh_at, snapshot_at
                )
                VALUES (%s, %s, %s, %s, %s, %s, %s, %s, %s)
                """,
                (
                    user_id,
                    login,
                    next_ad_at,
                    last_ad_at,
                    duration,
                    preroll_free_time,
                    snooze_count,
                    snooze_refresh_at,
                    now_iso,
                ),
            )
        self._increment_analytics_observability_counter("ads_request_success_total")
        self._log_analytics_decision(
            flow_id=flow_id,
            flow="ads",
            login=login,
            decision="success",
            reason="ads_collected",
            request_attempted=result.get("request_attempted"),
            request_result=request_result,
            http_status=http_status,
            scope_state={"streamer": "present"},
            runtime_state=self._build_analytics_runtime_state(login),
            next_ad_at=next_ad_at,
            last_ad_at=last_ad_at,
        )
        return True

    @collect_analytics_data.before_loop
    async def _before_analytics(self):
        await self.bot.wait_until_ready()

    # ------------------------------------------------------------------
    # Chatters Poller (alle 5 Min, nur für live Streams)
    # Tracked Lurker via GET /helix/chat/chatters (moderator:read:chatters)
    # ------------------------------------------------------------------

    def _get_chatters_user_fallback_warned_cache(self) -> set[tuple[str, int]]:
        warned = getattr(self, "_chatters_user_fallback_warned", None)
        if warned is None:
            warned = set()
            self._chatters_user_fallback_warned = warned
        return warned

    def _should_defer_chatters_collection_for_startup(self) -> bool:
        """Avoid false chatters failures while bot auth/chat runtime is still warming up."""
        if getattr(self, "_twitch_chat_bot", None):
            self._chatters_startup_deferral_logged = False
            return False

        token_mgr = getattr(self, "_bot_token_manager", None)
        if not token_mgr or not getattr(self, "_raid_bot", None):
            return False

        cached_bot_token = str(getattr(self, "_twitch_bot_token", "") or "").strip()
        manager_token = str(getattr(token_mgr, "access_token", "") or "").strip()
        manager_bot_id = str(getattr(token_mgr, "bot_id", "") or "").strip()

        if manager_token and manager_bot_id:
            self._chatters_startup_deferral_logged = False
            return False
        if not cached_bot_token and not manager_token:
            return False

        started_at = float(getattr(self, "_chatters_startup_grace_started_at", 0.0) or 0.0)
        if started_at <= 0:
            started_at = time.monotonic()
            self._chatters_startup_grace_started_at = started_at
        startup_age = time.monotonic() - started_at
        if startup_age > _CHATTERS_STARTUP_GRACE_SECONDS:
            return False

        if not getattr(self, "_chatters_startup_deferral_logged", False):
            log.debug(
                "Chatters-Poller: delaying startup cycle while bot auth/chat runtime is still initialising "
                "(age=%.1fs, cached_token=%s, bot_id_present=%s)",
                startup_age,
                "yes" if (cached_bot_token or manager_token) else "no",
                bool(manager_bot_id),
            )
            self._chatters_startup_deferral_logged = True
        return True

    def _warn_chatters_user_fallback_once(
        self,
        *,
        user_id: str,
        session_id: int,
        login: str,
        bot_diagnostics: dict[str, object] | None = None,
    ) -> None:
        key = (str(user_id), int(session_id))
        warned = self._get_chatters_user_fallback_warned_cache()
        if key in warned:
            return
        warned.add(key)
        diagnostics = self._format_bot_chatters_diagnostics(bot_diagnostics)
        log.warning(
            "Chatters-Poller: nutze Legacy-Broadcaster-Token fuer %s (Session %s). "
            "Bot-Pfad: %s",
            login,
            session_id,
            diagnostics,
        )

    def _clear_chatters_user_fallback_warning(
        self,
        *,
        user_id: str,
        session_id: int,
    ) -> None:
        self._get_chatters_user_fallback_warned_cache().discard((str(user_id), int(session_id)))

    async def _poll_chatters_single(
        self,
        user_id: str,
        login: str,
        session_id: int,
        now_iso: str,
        token: str | None = None,
    ) -> tuple[int, str, list[dict]] | None:
        """Pollt Chatters für einen Streamer via Helix API (nur wenn Token + moderator:read:chatters Scope vorhanden)."""
        flow_id = self._next_analytics_observability_flow_id("chatters")
        chatters: list[dict] = []
        streamer_scopes = (
            {s.lower() for s in self._raid_bot.auth_manager.get_scopes(user_id)}
            if token and getattr(self, "_raid_bot", None)
            else set()
        )
        has_streamer_chatters_scope = "moderator:read:chatters" in streamer_scopes
        missing_streamer_scope = bool(token) and not has_streamer_chatters_scope
        runtime_state = self._build_analytics_runtime_state(login)
        final_reason = "unknown"
        final_request_attempted: object = "none"
        final_request_result = "not_attempted"
        final_http_status: int | None = None
        bot_request_result = "not_attempted"
        bot_http_status: int | None = None
        streamer_request_result = "not_attempted"
        streamer_http_status: int | None = None

        # 1. Versuch: Bot-Token verwenden, sobald der Channel lokal als Bot-Channel
        # bekannt ist oder der Streamer selbst autorisiert ist. Fuer /chat/chatters
        # zaehlt Twitch-seitig der Mod-Status des Bots, nicht der lokale Runtime-Cache.
        bot_request_succeeded = False
        bot_token, bot_id, bot_scopes, bot_diagnostics = await self._resolve_bot_chatters_fallback(
            login,
            allow_untracked=bool(token),
        )
        if bot_token and bot_id and (not bot_scopes or "moderator:read:chatters" in bot_scopes):
            bot_diagnostics["bot_request_attempted"] = True
            self._increment_analytics_observability_counter("chatters_bot_path_attempt_total")
            bot_result = await self._get_chatters_result_with_legacy_fallback(
                broadcaster_id=user_id,
                moderator_id=bot_id,
                user_token=bot_token,
            )
            bot_request_result, bot_http_status, bot_error_code = self._structured_result_meta(bot_result)
            if bot_result.get("ok"):
                bot_chatters = bot_result.get("data")
                if isinstance(bot_chatters, list):
                    chatters = bot_chatters
                    bot_request_succeeded = True
                    bot_diagnostics["reason"] = "bot_path_success"
                    bot_diagnostics["bot_request_success"] = True
                    self._increment_analytics_observability_counter("chatters_bot_path_success_total")
                    log.debug(
                        "Chatters-Poller: %d Chatters via Bot-Fallback für %s",
                        len(chatters),
                        login,
                    )
                else:
                    bot_diagnostics["reason"] = "invalid_response"
                    self._increment_analytics_observability_counter("chatters_bot_path_failure_total")
            else:
                bot_diagnostics["reason"] = bot_error_code or "helix_chatters_failed"
                bot_diagnostics["error"] = bot_result.get("message")
                bot_diagnostics["bot_request_success"] = False
                self._increment_analytics_observability_counter("chatters_bot_path_failure_total")
                if (
                    bot_error_code == "helix_403_not_moderator"
                    and await self._attempt_bot_moderator_self_heal(
                        broadcaster_id=user_id,
                        login=login,
                        required_scope="moderator:read:chatters",
                        flow="chatters",
                    )
                ):
                    retry_result = await self._get_chatters_result_with_legacy_fallback(
                        broadcaster_id=user_id,
                        moderator_id=bot_id,
                        user_token=bot_token,
                    )
                    bot_request_result, bot_http_status, bot_error_code = self._structured_result_meta(
                        retry_result
                    )
                    if retry_result.get("ok"):
                        retry_chatters = retry_result.get("data")
                        if isinstance(retry_chatters, list):
                            chatters = retry_chatters
                            bot_request_succeeded = True
                            bot_diagnostics["reason"] = "bot_path_success"
                            bot_diagnostics["bot_request_success"] = True
                            self._increment_analytics_observability_counter(
                                "chatters_bot_path_success_total"
                            )
                            log.debug(
                                "Chatters-Poller: auto-re-mod repaired moderator access for %s",
                                login,
                            )
        elif bot_token and bot_id:
            bot_diagnostics["reason"] = "bot_scope_missing"
        else:
            bot_diagnostics["bot_request_success"] = False

        # 2. Fallback: Broadcaster-Token nur noch als Legacy-Rettungsnetz verwenden.
        if not bot_request_succeeded and not chatters and token:
            if has_streamer_chatters_scope:
                self._warn_chatters_user_fallback_once(
                    user_id=user_id,
                    session_id=session_id,
                    login=login,
                    bot_diagnostics=bot_diagnostics,
                )
                streamer_result = await self._get_chatters_result_with_legacy_fallback(
                    broadcaster_id=user_id,
                    moderator_id=user_id,
                    user_token=token,
                )
                streamer_request_result, streamer_http_status, streamer_error_code = (
                    self._structured_result_meta(streamer_result)
                )
                if streamer_result.get("ok"):
                    streamer_chatters = streamer_result.get("data")
                    if isinstance(streamer_chatters, list):
                        chatters = streamer_chatters
                        log.debug(
                            "Chatters-Poller: %d Chatters via Helix API für %s",
                            len(chatters),
                            login,
                        )
                else:
                    bot_diagnostics["streamer_error"] = streamer_result.get("message")
                    if not bot_request_succeeded:
                        final_reason = streamer_error_code or "helix_chatters_failed"

        if not chatters:
            if missing_streamer_scope and not bot_request_succeeded:
                key = (user_id, session_id)
                if key not in self._chatters_scope_warned:
                    self._chatters_scope_warned.add(key)
                    log.warning(
                        "Chatters-Poller: %s hat keinen Zugriff auf 'moderator:read:chatters'. "
                        "Bot-Token (Scope + Mod-Status im Channel) ist erforderlich; "
                        "Bot-Pfad: %s; Streamer-Token dient nur als Legacy-Fallback.",
                        login,
                        self._format_bot_chatters_diagnostics(bot_diagnostics),
                    )
            final_request_attempted = (
                "bot,streamer"
                if bot_diagnostics.get("bot_request_attempted") and has_streamer_chatters_scope and token
                else ("bot" if bot_diagnostics.get("bot_request_attempted") else ("streamer" if token and has_streamer_chatters_scope else "none"))
            )
            if bot_diagnostics.get("bot_request_attempted"):
                final_request_result = bot_request_result
                final_http_status = bot_http_status
                final_reason = str(bot_diagnostics.get("reason") or final_reason or "helix_chatters_failed")
            elif token and has_streamer_chatters_scope:
                final_request_result = streamer_request_result
                final_http_status = streamer_http_status
                final_reason = final_reason if final_reason != "unknown" else "helix_chatters_failed"
            else:
                final_reason = str(bot_diagnostics.get("reason") or "bot_path_unavailable")
            if missing_streamer_scope and not bot_request_succeeded and final_reason == "unknown":
                final_reason = "bot_scope_missing" if str(bot_diagnostics.get("reason")) == "bot_scope_missing" else "bot_path_unavailable"
            self._increment_analytics_observability_counter(
                f"chatters_reason_{final_reason}_total"
            )
            self._log_analytics_decision(
                flow_id=flow_id,
                flow="chatters",
                login=login,
                session_id=session_id,
                decision="failed",
                reason=final_reason,
                request_attempted=final_request_attempted,
                request_result=final_request_result,
                http_status=final_http_status,
                scope_state={
                    "bot": bot_diagnostics.get("bot_scope_present"),
                    "streamer": self._scope_presence_state(
                        scopes=streamer_scopes,
                        required_scope="moderator:read:chatters",
                        token_available=bool(token),
                    ),
                },
                runtime_state=runtime_state,
                chat_bot_available=bot_diagnostics.get("chat_bot_available"),
                bot_token_manager_available=bot_diagnostics.get("bot_token_manager_available"),
                bot_token_present=bot_diagnostics.get("bot_token_present"),
                bot_id_present=bot_diagnostics.get("bot_id_present"),
                bot_scope_present=bot_diagnostics.get("bot_scope_present"),
                streamer_scope_present=self._scope_presence_state(
                    scopes=streamer_scopes,
                    required_scope="moderator:read:chatters",
                    token_available=bool(token),
                ),
                runtime_sources=bot_diagnostics.get("runtime_sources"),
                allow_untracked=bot_diagnostics.get("allow_untracked"),
                bot_request_attempted=bot_diagnostics.get("bot_request_attempted", False),
                bot_request_success=bot_diagnostics.get("bot_request_success", False),
                bot_http_status=bot_http_status,
                streamer_http_status=streamer_http_status,
                diagnostic_now=now_iso,
            )
            return None

        if bot_request_succeeded and missing_streamer_scope:
            key = (user_id, session_id)
            self._chatters_scope_warned.discard(key)
        if bot_request_succeeded:
            self._clear_chatters_user_fallback_warning(user_id=user_id, session_id=session_id)
            self._restore_bot_ban_opt_out_if_healthy(
                twitch_user_id=user_id,
                login=login,
                flow="chatters",
            )
        try:
            self._record_irc_lurker_experiment_sample(
                login=login,
                session_id=session_id,
                now_iso=now_iso,
                helix_chatters=chatters,
            )
        except Exception:
            log.debug(
                "IRC experiment: comparison sampling failed for %s session=%s",
                login,
                session_id,
                exc_info=True,
            )

        log.debug(
            "Chatters-Poller: %d Chatters für %s (session %s)",
            len(chatters),
            login,
            session_id,
        )
        final_reason = "bot_path_success" if bot_request_succeeded else "fallback_to_streamer_token"
        if not bot_request_succeeded:
            self._increment_analytics_observability_counter("chatters_reason_fallback_to_streamer_token_total")
        final_request_attempted = "bot" if bot_request_succeeded else (
            "bot,streamer" if bot_diagnostics.get("bot_request_attempted") else "streamer"
        )
        final_request_result = "success"
        final_http_status = 200
        self._log_analytics_decision(
            flow_id=flow_id,
            flow="chatters",
            login=login,
            session_id=session_id,
            decision="success",
            reason=final_reason,
            request_attempted=final_request_attempted,
            request_result=final_request_result,
            http_status=final_http_status,
            scope_state={
                "bot": bot_diagnostics.get("bot_scope_present"),
                "streamer": self._scope_presence_state(
                    scopes=streamer_scopes,
                    required_scope="moderator:read:chatters",
                    token_available=bool(token),
                ),
            },
            runtime_state=runtime_state,
            chat_bot_available=bot_diagnostics.get("chat_bot_available"),
            bot_token_manager_available=bot_diagnostics.get("bot_token_manager_available"),
            bot_token_present=bot_diagnostics.get("bot_token_present"),
            bot_id_present=bot_diagnostics.get("bot_id_present"),
            bot_scope_present=bot_diagnostics.get("bot_scope_present"),
            streamer_scope_present=self._scope_presence_state(
                scopes=streamer_scopes,
                required_scope="moderator:read:chatters",
                token_available=bool(token),
            ),
            runtime_sources=bot_diagnostics.get("runtime_sources"),
            allow_untracked=bot_diagnostics.get("allow_untracked"),
            bot_request_attempted=bot_diagnostics.get("bot_request_attempted", False),
            bot_request_success=bot_request_succeeded,
            bot_http_status=bot_http_status,
            streamer_http_status=streamer_http_status,
            chatter_count=len(chatters),
            diagnostic_now=now_iso,
        )
        return (session_id, login, chatters)

    @staticmethod
    def _normalize_scope_values(scopes_raw) -> set[str]:
        if isinstance(scopes_raw, str):
            return {scope.strip().lower() for scope in scopes_raw.split() if scope.strip()}
        if isinstance(scopes_raw, (list, tuple, set)):
            return {
                str(scope).strip().lower()
                for scope in scopes_raw
                if str(scope).strip()
            }
        return set()

    @staticmethod
    def _normalize_login_values(values_raw) -> set[str]:
        if isinstance(values_raw, dict):
            values_iterable = values_raw.keys()
        elif isinstance(values_raw, (list, tuple, set)):
            values_iterable = values_raw
        else:
            return set()
        return {
            str(value or "").strip().lower().lstrip("#")
            for value in values_iterable
            if str(value or "").strip()
        }

    def _collect_bot_chatters_runtime_sources(self, login: str) -> set[str]:
        login_norm = str(login or "").strip().lower().lstrip("#")
        if not login_norm:
            return set()

        chat_bot = getattr(self, "_twitch_chat_bot", None)
        if not chat_bot:
            return set()

        runtime_sources: set[str] = set()
        if login_norm in self._normalize_login_values(
            getattr(chat_bot, "_monitored_streamers", set())
        ):
            runtime_sources.add("monitored_streamers")
        if login_norm in self._normalize_login_values(
            getattr(chat_bot, "_initial_channels", [])
        ):
            runtime_sources.add("initial_channels")
        if login_norm in self._normalize_login_values(
            getattr(chat_bot, "_monitored_only_channels", set())
        ):
            runtime_sources.add("monitored_only_channels")
        if login_norm in self._normalize_login_values(getattr(chat_bot, "_channel_ids", {})):
            runtime_sources.add("channel_ids")
        if login_norm in self._normalize_login_values(
            getattr(chat_bot, "_channel_subscription_types", {})
        ):
            runtime_sources.add("channel_subscription_types")

        is_subscription_ready = getattr(chat_bot, "is_channel_subscription_ready", None)
        if callable(is_subscription_ready):
            try:
                if is_subscription_ready(login_norm):
                    runtime_sources.add("subscription_ready")
            except Exception:
                log.debug(
                    "Chatters-Poller: konnte Subscription-Readiness fuer %s nicht pruefen",
                    login_norm,
                    exc_info=True,
                )

        return runtime_sources

    @staticmethod
    def _format_bot_chatters_diagnostics(bot_diagnostics: dict[str, object] | None) -> str:
        diagnostics = bot_diagnostics or {}
        reason = str(diagnostics.get("reason") or "unknown").strip() or "unknown"
        runtime_sources = diagnostics.get("runtime_sources") or []
        if isinstance(runtime_sources, (set, tuple)):
            runtime_sources = list(runtime_sources)
        if not isinstance(runtime_sources, list):
            runtime_sources = [str(runtime_sources)]
        runtime_sources_text = ",".join(
            sorted(str(source).strip() for source in runtime_sources if str(source).strip())
        ) or "-"
        bot_scope_state = str(diagnostics.get("bot_scope_present") or diagnostics.get("bot_scope_state") or "unknown").strip() or "unknown"
        details = [
            f"reason={reason}",
            f"runtime_sources={runtime_sources_text}",
            f"bot_scope={bot_scope_state}",
        ]
        if diagnostics.get("chat_bot_available") is False:
            details.append("chat_bot=absent")
        if diagnostics.get("bot_token_manager_available") is False:
            details.append("token_manager=absent")
        if diagnostics.get("allow_untracked"):
            details.append("auth_override=1")
        if diagnostics.get("bot_request_attempted"):
            details.append("bot_request=attempted")
        error_text = str(diagnostics.get("error") or "").strip()
        if error_text:
            details.append(f"error={error_text[:180]}")
        return "; ".join(details)

    async def _resolve_bot_chatters_fallback(
        self,
        login: str,
        *,
        allow_untracked: bool = False,
    ) -> tuple[str | None, str | None, set[str], dict[str, object]]:
        login_norm = str(login or "").strip().lower().lstrip("#")
        diagnostics: dict[str, object] = {
            "allow_untracked": bool(allow_untracked),
            "runtime_sources": [],
            "chat_bot_available": False,
            "bot_token_manager_available": False,
            "bot_token_present": False,
            "bot_id_present": False,
            "bot_scope_present": "unknown",
        }
        if not login_norm:
            diagnostics["reason"] = "missing_login"
            return None, None, set(), diagnostics

        chat_bot = getattr(self, "_twitch_chat_bot", None)
        diagnostics["chat_bot_available"] = bool(chat_bot)
        runtime_sources = sorted(self._collect_bot_chatters_runtime_sources(login_norm))
        diagnostics["runtime_sources"] = runtime_sources
        if not chat_bot:
            diagnostics["reason"] = "chat_bot_unavailable"
            if not allow_untracked:
                return None, None, set(), diagnostics
        elif not runtime_sources and not allow_untracked:
            diagnostics["reason"] = "channel_not_tracked_in_chat_runtime"
            return None, None, set(), diagnostics

        token_mgr = getattr(self, "_bot_token_manager", None)
        if not token_mgr:
            diagnostics["reason"] = "bot_token_manager_unavailable"
            return None, None, set(), diagnostics
        diagnostics["bot_token_manager_available"] = True

        try:
            token, manager_bot_id = await token_mgr.get_valid_token()
        except Exception:
            log.debug("Chatters-Poller: konnte Bot-Token nicht laden", exc_info=True)
            diagnostics["reason"] = "bot_token_load_failed"
            return None, None, set(), diagnostics

        token = str(token or "").strip()
        if token.lower().startswith("oauth:"):
            token = token[6:]
        bot_id = (
            str(
                manager_bot_id
                or getattr(chat_bot, "bot_id_safe", None)
                or getattr(chat_bot, "bot_id", None)
                or ""
            ).strip()
            or None
        )
        bot_scopes = self._normalize_scope_values(getattr(token_mgr, "scopes", ()))
        diagnostics["bot_token_present"] = bool(token)
        diagnostics["bot_id_present"] = bool(bot_id)
        diagnostics["bot_scope_present"] = (
            "present"
            if "moderator:read:chatters" in bot_scopes
            else ("unknown" if not bot_scopes else "missing")
        )
        diagnostics["bot_scope_state"] = diagnostics["bot_scope_present"]
        if not token:
            diagnostics["reason"] = "bot_token_missing"
            return None, bot_id, bot_scopes, diagnostics
        if not bot_id:
            diagnostics["reason"] = "bot_id_missing"
            return None, bot_id, bot_scopes, diagnostics
        diagnostics["reason"] = "ready"
        return token, bot_id, bot_scopes, diagnostics

    @tasks.loop(seconds=30)
    async def collect_chatters_data(self):
        """
        Pollt Chatters-Liste für ALLE live Streamer (Partner + Monitored + Category).

        WICHTIG: Datensammlung für Analyse läuft für ALLE.
        Bot-Funktionen (Raids, Commands, etc.) nur für Partner!
        """
        if not self.api:
            return
        if self._should_defer_chatters_collection_for_startup():
            return

        try:
            # Live-Sessions kommen aus Postgres (Analytics-DB)
            with storage.transaction() as conn:
                rows = conn.execute(
                    """
                    SELECT twitch_user_id, streamer_login, active_session_id
                    FROM twitch_live_state
                    WHERE is_live = 1
                      AND active_session_id IS NOT NULL
                    """
                ).fetchall()

            # OAuth/Permissions live in the shared runtime storage.
            auth_ids: set[str] = set()
            with storage_runtime.transaction() as conn:
                auth_rows = conn.execute(
                    "SELECT twitch_user_id FROM twitch_raid_auth WHERE raid_enabled IS TRUE"
                ).fetchall()
                auth_ids = {
                    (r["twitch_user_id"] if hasattr(r, "keys") else r[0]) for r in auth_rows
                }

            # Track active sessions to reset per-session warning cache
            active_sessions = {
                (r[2] if not hasattr(r, "keys") else r["active_session_id"]) for r in rows
            }
            if self._chatters_scope_warned:
                self._chatters_scope_warned = {
                    key for key in self._chatters_scope_warned if key[1] in active_sessions
                }
            if self._get_chatters_user_fallback_warned_cache():
                self._chatters_user_fallback_warned = {
                    key
                    for key in self._get_chatters_user_fallback_warned_cache()
                    if key[1] in active_sessions
                }

            # Proactive cleanup for experiment stats if sessions were missed by finalizer
            comparison_store = getattr(self, "_irc_lurker_experiment_session_stats", None)
            if isinstance(comparison_store, dict) and comparison_store:
                expired_sessions = [sid for sid in comparison_store if sid not in active_sessions]
                for sid in expired_sessions:
                    comparison_store.pop(sid, None)

            if rows:
                log.debug(
                    "Chatters-Poller: Tracking %d live Streamer (alle für Analyse)",
                    len(rows),
                )
        except Exception:
            log.exception("Chatters-Poller: Fehler beim Laden der live Streamer")
            return

        if not rows:
            return

        now_iso = datetime.now(UTC).isoformat(timespec="seconds")
        tasks_list = []

        # Token-Resolution vorbereiten (nur für Partner)
        session = self.api.get_http_session()
        auth_mgr = getattr(self, "_raid_bot", None) and getattr(
            self._raid_bot, "auth_manager", None
        )

        for row in rows:
            user_id = row[0] if not hasattr(row, "keys") else row["twitch_user_id"]
            login = row[1] if not hasattr(row, "keys") else row["streamer_login"]
            sess_id = row[2] if not hasattr(row, "keys") else row["active_session_id"]
            has_auth = user_id in auth_ids

            async def _wrap_poll(u_id, lgn, s_id, has_auth_flag):
                tok = None
                if has_auth_flag and auth_mgr:
                    tok = await auth_mgr.get_valid_token(u_id, session)
                return await self._poll_chatters_single(u_id, lgn, s_id, now_iso, tok)

            tasks_list.append(_wrap_poll(user_id, login, sess_id, has_auth))

        # Alle API-Calls parallel feuern
        results = await asyncio.gather(*tasks_list, return_exceptions=True)

        # Batch-Write
        payloads = [r for r in results if isinstance(r, tuple)]
        if not payloads:
            return

        try:
            with storage.transaction() as conn:
                for session_id, login, chatters in payloads:
                    # Build normalized chatter list from API response
                    chatter_entries: list[tuple[str, str | None]] = []
                    excluded_bots = 0
                    for chatter in chatters:
                        c_login = (chatter.get("user_login") or "").lower().strip()
                        c_id = (chatter.get("user_id") or "").strip() or None
                        if c_login and not is_known_chat_bot(c_login):
                            chatter_entries.append((c_login, c_id))
                        elif c_login:
                            excluded_bots += 1

                    if not chatter_entries:
                        continue

                    logins = [e[0] for e in chatter_entries]

                    with conn.cursor() as cur:
                        # Check rollup to determine is_first_time_streamer per chatter
                        cur.execute(
                            "SELECT chatter_login FROM twitch_chatter_rollup"
                            " WHERE streamer_login = %s AND chatter_login = ANY(%s)",
                            (login, logins),
                        )
                        known_globally: set[str] = {row[0] for row in cur.fetchall()}

                        # Upsert all chatters into session table.
                        # ON CONFLICT: only refresh last_seen_at — don't overwrite messages
                        # or seen_via_chatters_api if the IRC path already set them.
                        cur.executemany(
                            """
                            INSERT INTO twitch_session_chatters (
                                session_id, streamer_login, chatter_login, chatter_id,
                                first_message_at, messages, is_first_time_streamer,
                                seen_via_chatters_api, last_seen_at
                            ) VALUES (%s, %s, %s, %s, %s, 0, %s, TRUE, %s)
                            ON CONFLICT (session_id, chatter_login) DO UPDATE
                                SET last_seen_at = EXCLUDED.last_seen_at
                            """,
                            [
                                (session_id, login, c_login, c_id, now_iso,
                                 c_login not in known_globally, now_iso)
                                for c_login, c_id in chatter_entries
                            ],
                        )

                        # Upsert rollup so lurkers become part of the global chatter history.
                        # ON CONFLICT: only refresh last_seen_at, preserve chatter_id if known.
                        cur.executemany(
                            """
                            INSERT INTO twitch_chatter_rollup (
                                streamer_login, chatter_login, chatter_id,
                                first_seen_at, last_seen_at, total_messages, total_sessions
                            ) VALUES (%s, %s, %s, %s, %s, 0, 1)
                            ON CONFLICT (streamer_login, chatter_login) DO UPDATE
                                SET last_seen_at = EXCLUDED.last_seen_at,
                                    chatter_id = COALESCE(
                                        twitch_chatter_rollup.chatter_id, EXCLUDED.chatter_id
                                    )
                            """,
                            [
                                (login, c_login, c_id, now_iso, now_iso)
                                for c_login, c_id in chatter_entries
                            ],
                        )

                        # Presence-Tick für jeden Chatter dieses Polls eintragen
                        cur.executemany(
                            """
                            INSERT INTO twitch_viewer_presence_ticks
                                (session_id, streamer_login, viewer_login, tick_at)
                            VALUES (%s, %s, %s, %s)
                            ON CONFLICT (session_id, viewer_login, tick_at) DO NOTHING
                            """,
                            [
                                (session_id, login, c_login, now_iso)
                                for c_login, _ in chatter_entries
                            ],
                        )

                    log.debug(
                        "Chatters-Poller: %d Chatter für %s (session %s) gespeichert (%d erstmalig, %d Bots gefiltert)",
                        len(chatter_entries), login, session_id,
                        sum(1 for c_login, _ in chatter_entries if c_login not in known_globally),
                        excluded_bots,
                    )
        except Exception:
            log.exception("Chatters-Poller: Batch-DB-Fehler")

    @collect_chatters_data.before_loop
    async def _before_chatters(self):
        await self.bot.wait_until_ready()

    # ------------------------------------------------------------------
    async def _run_stream_online_followups(
        self,
        *,
        broadcaster_user_id: str,
        broadcaster_login: str,
        login_value: str,
        defer_refresh: bool,
        message_id: str | None = None,
    ) -> None:
        handler = getattr(self, "_handle_stream_went_live", None)
        if callable(handler):
            executed = True
            run_once = getattr(self, "_run_eventsub_business_effect_once", None)
            if callable(run_once):
                executed = await run_once(
                    message_id=message_id,
                    effect_name="stream_online_went_live",
                    coro_factory=lambda: handler(broadcaster_user_id, broadcaster_login),
                )
            else:
                await handler(broadcaster_user_id, broadcaster_login)
            if executed:
                log.info(
                    "EventSub stream.online: %s (%s) ist live – triggere Go-Live-Handler",
                    broadcaster_login or broadcaster_user_id,
                    broadcaster_user_id,
                )
            else:
                log.debug(
                    "EventSub stream.online: Go-Live-Handler bereits verarbeitet fuer %s msg_id=%s",
                    broadcaster_user_id,
                    message_id or "n/a",
                )

        async def _refresh_once() -> None:
            refresh = getattr(self, "_request_partner_raid_score_refresh", None)
            if callable(refresh):
                try:
                    await refresh(
                        twitch_user_id=broadcaster_user_id,
                        login=login_value or broadcaster_login,
                        trigger="eventsub_stream_online",
                    )
                except Exception:
                    log.debug(
                        "_handle_stream_online: Partner raid score refresh failed for %s",
                        broadcaster_user_id,
                        exc_info=True,
                    )

        schedule_refresh = getattr(self, "_schedule_partner_raid_score_refresh", None)
        if defer_refresh and callable(schedule_refresh):
            try:
                schedule_refresh(
                    twitch_user_id=broadcaster_user_id,
                    login=login_value or broadcaster_login,
                    trigger="eventsub_stream_online",
                )
            except Exception:
                log.debug(
                    "_handle_stream_online: Partner raid score refresh scheduling failed for %s",
                    broadcaster_user_id,
                    exc_info=True,
                )
            return

        run_once = getattr(self, "_run_eventsub_business_effect_once", None)
        if callable(run_once):
            await run_once(
                message_id=message_id,
                effect_name="stream_online_refresh",
                coro_factory=_refresh_once,
            )
            return
        await _refresh_once()

    async def _handle_stream_online(
        self,
        broadcaster_user_id: str,
        broadcaster_login: str,
        event: dict,
        *,
        message_id: str | None = None,
    ) -> None:
        """Wird von stream.online EventSub aufgerufen – triggert sofort den Go-Live-Handler."""
        started_at = (event.get("started_at") or "").strip() or None
        stream_id = str(event.get("id") or event.get("stream_id") or "").strip() or None
        login_value = (broadcaster_login or event.get("broadcaster_user_login") or "").strip().lower()
        now_iso = datetime.now(UTC).isoformat(timespec="seconds")

        try:
            with storage.transaction() as c:
                if login_value:
                    c.execute(
                        """
                        DELETE FROM twitch_live_state
                         WHERE LOWER(streamer_login) = LOWER(%s)
                           AND LOWER(COALESCE(twitch_user_id, '')) <> LOWER(%s)
                        """,
                        (login_value, broadcaster_user_id),
                    )
                c.execute(
                    """
                    INSERT INTO twitch_live_state (
                        twitch_user_id, streamer_login, is_live, last_seen_at, last_stream_id, last_started_at
                    )
                    VALUES (%s, %s, 1, %s, %s, %s)
                    ON CONFLICT (twitch_user_id) DO UPDATE
                        SET streamer_login = COALESCE(NULLIF(EXCLUDED.streamer_login, ''), twitch_live_state.streamer_login),
                            is_live = 1,
                            last_seen_at = EXCLUDED.last_seen_at,
                            last_stream_id = COALESCE(EXCLUDED.last_stream_id, twitch_live_state.last_stream_id),
                            last_started_at = COALESCE(EXCLUDED.last_started_at, twitch_live_state.last_started_at)
                    """,
                    (
                        broadcaster_user_id,
                        login_value or broadcaster_user_id,
                        now_iso,
                        stream_id,
                        started_at,
                    ),
                )
        except Exception:
            log.debug(
                "_handle_stream_online: Konnte minimalen Live-State nicht speichern fuer %s",
                broadcaster_user_id,
                exc_info=True,
            )
        should_defer_followups = bool(
            getattr(self, "_eventsub_defer_stream_online_followups", False)
        )
        enqueue_followups = getattr(self, "_enqueue_eventsub_stream_online_followups_processing", None)
        if should_defer_followups and callable(enqueue_followups):
            await enqueue_followups(
                broadcaster_user_id=broadcaster_user_id,
                broadcaster_login=broadcaster_login,
                login_value=login_value,
                message_id=message_id,
            )
            return
        await self._run_stream_online_followups(
            broadcaster_user_id=broadcaster_user_id,
            broadcaster_login=broadcaster_login,
            login_value=login_value,
            defer_refresh=should_defer_followups,
            message_id=message_id,
        )

    async def _handle_channel_update(
        self,
        broadcaster_user_id: str,
        event: dict,
        *,
        message_id: str | None = None,
        allow_background_refresh: bool = True,
    ) -> None:
        """Speichert eine channel.update Notification (Titel/Game-Änderung) in der DB."""
        title = (event.get("title") or "").strip() or None
        game_name = (event.get("category_name") or event.get("game_name") or "").strip() or None
        language = (event.get("broadcaster_language") or "").strip() or None
        spawn_bg_task = getattr(self, "_spawn_bg_task", None)
        async def _persist_update() -> None:
            with storage.transaction() as c:
                c.execute(
                    """
                    INSERT INTO twitch_channel_updates (twitch_user_id, title, game_name, language, recorded_at)
                    VALUES (%s, %s, %s, %s, %s)
                    """,
                    (
                        broadcaster_user_id,
                        title,
                        game_name,
                        language,
                        datetime.now(UTC).isoformat(timespec="seconds"),
                    ),
                )
                c.execute(
                    """
                    UPDATE twitch_live_state
                       SET last_title = COALESCE(%s, last_title),
                           last_game  = COALESCE(%s, last_game)
                     WHERE twitch_user_id = %s AND is_live = 1
                    """,
                    (title, game_name, broadcaster_user_id),
                )

        try:
            run_once = getattr(self, "_run_eventsub_business_effect_once", None)
            if callable(run_once):
                await run_once(
                    message_id=message_id,
                    effect_name="channel_update_db",
                    coro_factory=_persist_update,
                )
            else:
                await _persist_update()
            schedule_refresh = getattr(self, "_schedule_partner_raid_score_refresh", None)
            if allow_background_refresh and callable(schedule_refresh) and callable(spawn_bg_task):
                schedule_refresh(
                    twitch_user_id=broadcaster_user_id,
                    trigger="eventsub_channel_update",
                )
            else:
                refresh = getattr(self, "_request_partner_raid_score_refresh", None)
                if callable(refresh):
                    if callable(run_once):
                        await run_once(
                            message_id=message_id,
                            effect_name="channel_update_refresh",
                            coro_factory=lambda: refresh(
                                twitch_user_id=broadcaster_user_id,
                                trigger="eventsub_channel_update",
                            ),
                        )
                    else:
                        await refresh(
                            twitch_user_id=broadcaster_user_id,
                            trigger="eventsub_channel_update",
                        )
        except Exception:
            log.exception("_handle_channel_update: Fehler für %s", broadcaster_user_id)

    async def _store_subscription_event(
        self, broadcaster_user_id: str, event: dict, event_type: str
    ) -> None:
        """Speichert channel.subscribe / channel.subscription.gift / channel.subscription.message."""
        user_login = (
            event.get("user_login") or event.get("user_name") or ""
        ).strip().lower() or None
        tier = (event.get("tier") or "1000").strip()
        is_gift = bool(event.get("is_gift"))
        gifter_login = (
            event.get("gifter_login") or event.get("gifter_user_login") or ""
        ).strip().lower() or None
        cumulative_months = int(event.get("cumulative_months") or event.get("months") or 0) or None
        streak_months = int(event.get("streak_months") or 0) or None
        message_data = event.get("message") or {}
        if isinstance(message_data, dict):
            message = (message_data.get("text") or "").strip() or None
        else:
            message = str(message_data).strip() or None
        gift_total_kind = str(event.get("gift_total_kind") or "").strip().lower()
        if gift_total_kind == "batch_total":
            total_gifted = int(event.get("gift_total") or event.get("total") or 0) or None
        elif gift_total_kind == "cumulative_total":
            total_gifted = int(event.get("total") or 1)
        else:
            total_gifted = int(event.get("total") or event.get("gift_total") or 0) or None

        session_id = self._get_active_session_id_by_user_id(broadcaster_user_id)

        try:
            with storage.transaction() as c:
                c.execute(
                    """
                    INSERT INTO twitch_subscription_events
                        (session_id, twitch_user_id, event_type, user_login, tier,
                         is_gift, gifter_login, cumulative_months, streak_months,
                         message, total_gifted, received_at)
                    VALUES (%s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s)
                    """,
                    (
                        session_id,
                        broadcaster_user_id,
                        event_type,
                        user_login,
                        tier,
                        is_gift,
                        gifter_login,
                        cumulative_months,
                        streak_months,
                        message,
                        total_gifted,
                        datetime.now(UTC).isoformat(timespec="seconds"),
                    ),
                )
        except Exception:
            log.exception(
                "_store_subscription_event: Fehler für %s (%s)",
                broadcaster_user_id,
                event_type,
            )

    def _get_active_session_id_by_user_id(self, broadcaster_user_id: str) -> int | None:
        """Gibt die aktive session_id für einen Broadcaster zurück (über twitch_live_state).

        twitch_stream_sessions hat keine twitch_user_id-Spalte – deshalb über
        twitch_live_state.active_session_id lookupaben.
        """
        try:
            with storage.transaction() as c:
                row = c.execute(
                    "SELECT active_session_id FROM twitch_live_state WHERE twitch_user_id = %s",
                    (broadcaster_user_id,),
                ).fetchone()
            if row and row[0] is not None:
                return int(row[0] if not hasattr(row, "keys") else row["active_session_id"])
        except Exception:
            log.debug(
                "_get_active_session_id_by_user_id: Fehler für %s",
                broadcaster_user_id,
                exc_info=True,
            )
        return None

    async def _store_ad_break_event(self, broadcaster_user_id: str, event: dict) -> None:
        """Speichert ein channel.ad_break.begin Event."""
        duration_seconds = int(event.get("duration_seconds") or 0) or None
        is_automatic_raw = event.get("is_automatic")
        # Use a real boolean so Postgres boolean columns accept the value.
        is_automatic = bool(is_automatic_raw) if is_automatic_raw is not None else False

        session_id = self._get_active_session_id_by_user_id(broadcaster_user_id)

        try:
            with storage.transaction() as c:
                c.execute(
                    """
                    INSERT INTO twitch_ad_break_events
                        (session_id, twitch_user_id, duration_seconds, is_automatic, started_at)
                    VALUES (%s, %s, %s, %s, %s)
                    """,
                    (
                        session_id,
                        broadcaster_user_id,
                        duration_seconds,
                        is_automatic,
                        datetime.now(UTC).isoformat(timespec="seconds"),
                    ),
                )
        except Exception:
            log.exception("_store_ad_break_event: Fehler für %s", broadcaster_user_id)

    async def _store_bits_event(self, broadcaster_user_id: str, event: dict) -> None:
        """Speichert ein channel.cheer (Bits) Event in der Datenbank."""
        donor_login = (
            event.get("user_login") or event.get("user_name") or ""
        ).strip().lower() or None
        amount = int(event.get("bits") or event.get("amount") or 0)
        # Message kann ein String oder ein Dict {"text": "...", "emotes": ...} sein
        message_data = event.get("message")
        if isinstance(message_data, dict):
            message = (message_data.get("text") or "").strip() or None
        elif isinstance(message_data, str):
            message = message_data.strip() or None
        else:
            message = None
        if not amount:
            return
        # Session ID für den aktuellen Stream bestimmen (optional)
        session_id = self._get_active_session_id_by_user_id(broadcaster_user_id)

        try:
            with storage.transaction() as c:
                c.execute(
                    """
                    INSERT INTO twitch_bits_events
                        (session_id, twitch_user_id, donor_login, amount, message, received_at)
                    VALUES (%s, %s, %s, %s, %s, %s)
                    """,
                    (
                        session_id,
                        broadcaster_user_id,
                        donor_login,
                        amount,
                        message,
                        datetime.now(UTC).isoformat(timespec="seconds"),
                    ),
                )
        except Exception:
            log.exception("_store_bits_event: Fehler beim Speichern für %s", broadcaster_user_id)

    async def _store_channel_points_event(self, broadcaster_user_id: str, event: dict) -> None:
        """Speichert ein channel.channel_points_*_reward_redemption.add Event."""
        user_login = (
            event.get("user_login") or event.get("user_name") or ""
        ).strip().lower() or None
        reward = event.get("reward") or {}
        reward_id = (reward.get("id") or event.get("reward_id") or "").strip() or None
        reward_title = (reward.get("title") or event.get("reward_title") or "").strip() or None
        reward_cost = int(reward.get("cost") or event.get("reward_cost") or 0) or None
        user_input = (event.get("user_input") or "").strip() or None
        redeemed_at = (event.get("redeemed_at") or "").strip() or datetime.now(UTC).isoformat(
            timespec="seconds"
        )

        session_id = self._get_active_session_id_by_user_id(broadcaster_user_id)

        try:
            with storage.transaction() as c:
                c.execute(
                    """
                    INSERT INTO twitch_channel_points_events
                        (session_id, twitch_user_id, user_login, reward_id, reward_title, reward_cost, user_input, redeemed_at)
                    VALUES (%s, %s, %s, %s, %s, %s, %s, %s)
                    """,
                    (
                        session_id,
                        broadcaster_user_id,
                        user_login,
                        reward_id,
                        reward_title,
                        reward_cost,
                        user_input,
                        redeemed_at,
                    ),
                )
        except Exception:
            log.exception(
                "_store_channel_points_event: Fehler beim Speichern für %s",
                broadcaster_user_id,
            )

    async def _store_hype_train_event(
        self,
        broadcaster_user_id: str,
        event: dict,
        *,
        ended: bool,
        progress: bool = False,
    ) -> None:
        """Speichert ein channel.hype_train.begin/progress/end Event in der Datenbank."""
        started_at = (event.get("started_at") or "").strip() or None
        ended_at = (event.get("ended_at") or "").strip() or None if ended else None
        level = int(event.get("level") or 0) or None
        total_progress = int(event.get("total") or event.get("total_progress") or 0) or None
        duration_seconds: int | None = None
        if started_at and ended_at:
            try:
                from datetime import datetime as _dt

                dt_start = _dt.fromisoformat(started_at.replace("Z", "+00:00"))
                dt_end = _dt.fromisoformat(ended_at.replace("Z", "+00:00"))
                duration_seconds = max(0, int((dt_end - dt_start).total_seconds()))
            except (TypeError, ValueError):
                log.debug(
                    "_store_hype_train_event: Konnte Dauer nicht berechnen für %s",
                    broadcaster_user_id,
                    exc_info=True,
                )

        session_id = self._get_active_session_id_by_user_id(broadcaster_user_id)

        try:
            with storage.transaction() as c:
                if ended:
                    # Versuche, ein bereits vorhandenes begin-Event zu aktualisieren
                    updated = c.execute(
                        """
                        UPDATE twitch_hype_train_events
                           SET ended_at = %s,
                               duration_seconds = %s,
                               level = COALESCE(%s, level),
                               total_progress = COALESCE(%s, total_progress)
                         WHERE twitch_user_id = %s
                           AND started_at = %s
                           AND ended_at IS NULL
                        """,
                        (
                            ended_at,
                            duration_seconds,
                            level,
                            total_progress,
                            broadcaster_user_id,
                            started_at,
                        ),
                    ).rowcount
                    if updated:
                        return
                phase = "progress" if progress else ("end" if ended else "begin")
                c.execute(
                    """
                    INSERT INTO twitch_hype_train_events
                        (session_id, twitch_user_id, started_at, ended_at,
                         duration_seconds, level, total_progress, event_phase)
                    VALUES (%s, %s, %s, %s, %s, %s, %s, %s)
                    """,
                    (
                        session_id,
                        broadcaster_user_id,
                        started_at,
                        ended_at,
                        duration_seconds,
                        level,
                        total_progress,
                        phase,
                    ),
                )
        except Exception:
            log.exception(
                "_store_hype_train_event: Fehler beim Speichern für %s",
                broadcaster_user_id,
            )

    async def _store_ban_event(
        self, broadcaster_user_id: str, event: dict, *, unbanned: bool = False
    ) -> None:
        """Speichert ein channel.ban / channel.unban Event."""
        event_type = "unban" if unbanned else "ban"
        target_login = (
            event.get("user_login") or event.get("user_name") or ""
        ).strip().lower() or None
        target_id = str(event.get("user_id") or "").strip() or None
        moderator_login = (event.get("moderator_user_login") or "").strip().lower() or None
        reason = (event.get("reason") or "").strip() or None
        ends_at = (event.get("ends_at") or "").strip() or None  # None = permanent

        session_id = self._get_active_session_id_by_user_id(broadcaster_user_id)

        try:
            with storage.transaction() as c:
                c.execute(
                    """
                    INSERT INTO twitch_ban_events
                        (session_id, twitch_user_id, event_type, target_login, target_id,
                         moderator_login, reason, ends_at, received_at)
                    VALUES (%s, %s, %s, %s, %s, %s, %s, %s, %s)
                    """,
                    (
                        session_id,
                        broadcaster_user_id,
                        event_type,
                        target_login,
                        target_id,
                        moderator_login,
                        reason,
                        ends_at,
                        datetime.now(UTC).isoformat(timespec="seconds"),
                    ),
                )
        except Exception:
            log.exception("_store_ban_event: Fehler für %s (%s)", broadcaster_user_id, event_type)

    async def _store_shoutout_event(
        self, broadcaster_user_id: str, event: dict, *, direction: str
    ) -> None:
        """Speichert ein channel.shoutout.create / channel.shoutout.receive Event.
        direction: 'sent' | 'received'
        """
        if direction == "sent":
            other_id = str(event.get("to_broadcaster_user_id") or "").strip() or None
            other_login = (event.get("to_broadcaster_user_login") or "").strip().lower() or None
            moderator_login = (event.get("moderator_user_login") or "").strip().lower() or None
            viewer_count = int(event.get("viewer_count") or 0)
        else:
            other_id = str(event.get("from_broadcaster_user_id") or "").strip() or None
            other_login = (event.get("from_broadcaster_user_login") or "").strip().lower() or None
            moderator_login = None
            viewer_count = int(event.get("viewer_count") or 0)

        try:
            with storage.transaction() as c:
                c.execute(
                    """
                    INSERT INTO twitch_shoutout_events
                        (twitch_user_id, direction, other_broadcaster_id, other_broadcaster_login,
                         moderator_login, viewer_count, received_at)
                    VALUES (%s, %s, %s, %s, %s, %s, %s)
                    """,
                    (
                        broadcaster_user_id,
                        direction,
                        other_id,
                        other_login,
                        moderator_login,
                        viewer_count,
                        datetime.now(UTC).isoformat(timespec="seconds"),
                    ),
                )
        except Exception:
            log.exception(
                "_store_shoutout_event: Fehler für %s (%s)",
                broadcaster_user_id,
                direction,
            )

    @tasks.loop(hours=1)
    async def compute_raid_retention(self):
        """Hourly: compute retention metrics for recent outgoing raids into twitch_raid_retention."""
        try:
            with storage.transaction() as analytics_conn:
                raids = analytics_conn.execute(
                    """
                    SELECT id, from_broadcaster_login, to_broadcaster_login,
                           viewer_count, executed_at
                    FROM twitch_raid_history
                    WHERE executed_at >= NOW() - INTERVAL '7 days'
                    ORDER BY executed_at DESC
                    """
                ).fetchall()
        except Exception:
            log.exception("compute_raid_retention: failed to load raids from analytics storage")
            return

        if not raids:
            return

        session_bot_clause, session_bot_params = build_known_chat_bot_not_in_clause(
            column_expr="sc.chatter_login",
            placeholder="%s",
        )
        rollup_bot_clause, rollup_bot_params = build_known_chat_bot_not_in_clause(
            column_expr="chatter_login",
            placeholder="%s",
        )

        processed = 0
        for raid in raids:
            raid_id = raid[0]
            from_login = raid[1].lower()
            to_login = raid[2].lower()
            viewer_count = raid[3]
            executed_at_raw = raid[4]

            try:
                from datetime import UTC, datetime as _dt
                if isinstance(executed_at_raw, str):
                    executed_at = _dt.fromisoformat(executed_at_raw.replace("Z", "+00:00"))
                elif isinstance(executed_at_raw, _dt):
                    executed_at = executed_at_raw
                    if executed_at.tzinfo is None:
                        executed_at = executed_at.replace(tzinfo=UTC)
                else:
                    continue

                with storage.transaction() as pg:
                    existing = pg.execute(
                        """
                        SELECT raid_id
                        FROM twitch_raid_retention
                        WHERE raid_id = %s
                          AND executed_at = %s
                        """,
                        (raid_id, executed_at),
                    ).fetchone()
                    if existing:
                        continue

                    target_session = pg.execute(
                        """
                        SELECT id FROM twitch_stream_sessions
                        WHERE LOWER(streamer_login) = %s
                          AND started_at <= %s
                          AND (ended_at IS NULL OR ended_at >= %s)
                        ORDER BY started_at DESC LIMIT 1
                        """,
                        (to_login, executed_at, executed_at),
                    ).fetchone()
                    if not target_session:
                        continue

                    target_session_id = target_session["id"]

                    def _count_chatters(offset_min: int) -> int:
                        row = pg.execute(
                            f"""
                            SELECT COUNT(
                                DISTINCT COALESCE(NULLIF(sc.chatter_login, ''), sc.chatter_id)
                            ) AS cnt
                            FROM twitch_session_chatters sc
                            WHERE sc.session_id = %s
                              AND sc.last_seen_at >= %s
                              AND sc.last_seen_at <= (%s + (%s || ' minutes')::INTERVAL)
                              AND {session_bot_clause}
                            """,
                            (
                                target_session_id,
                                executed_at,
                                executed_at,
                                str(offset_min),
                                *session_bot_params,
                            ),
                        ).fetchone()
                        return row["cnt"] if row else 0

                    c5 = _count_chatters(5)
                    c15 = _count_chatters(15)
                    c30 = _count_chatters(30)

                    known_row = pg.execute(
                        f"""
                        SELECT COUNT(DISTINCT sc.chatter_login) AS known
                        FROM twitch_session_chatters sc
                        WHERE sc.session_id = %s
                          AND sc.last_seen_at >= %s
                          AND {session_bot_clause}
                          AND sc.chatter_login IN (
                              SELECT chatter_login FROM twitch_chatter_rollup
                              WHERE LOWER(streamer_login) = %s
                                AND {rollup_bot_clause}
                          )
                        """,
                        (
                            target_session_id,
                            executed_at,
                            *session_bot_params,
                            from_login,
                            *rollup_bot_params,
                        ),
                    ).fetchone()
                    known_from_raider = known_row["known"] if known_row else 0

                    new_row = pg.execute(
                        f"""
                        SELECT COUNT(
                            DISTINCT COALESCE(NULLIF(sc.chatter_login, ''), sc.chatter_id)
                        ) AS new_viewers
                        FROM twitch_session_chatters sc
                        WHERE sc.session_id = %s
                          AND sc.last_seen_at >= %s
                          AND {session_bot_clause}
                          AND (
                              sc.chatter_login IS NULL
                              OR sc.chatter_login = ''
                              OR sc.chatter_login NOT IN (
                                  SELECT chatter_login FROM twitch_chatter_rollup
                                  WHERE LOWER(streamer_login) = %s
                                    AND first_seen_at < %s
                                    AND {rollup_bot_clause}
                              )
                          )
                        """,
                        (
                            target_session_id,
                            executed_at,
                            *session_bot_params,
                            to_login,
                            executed_at,
                            *rollup_bot_params,
                        ),
                    ).fetchone()
                    new_to_target = new_row["new_viewers"] if new_row else 0

                    new_chat_row = pg.execute(
                        f"""
                        SELECT COUNT(
                            DISTINCT COALESCE(NULLIF(sc.chatter_login, ''), sc.chatter_id)
                        ) AS new_chatters
                        FROM twitch_session_chatters sc
                        WHERE sc.session_id = %s
                          AND sc.first_message_at >= %s
                          AND sc.messages > 0
                          AND {session_bot_clause}
                          AND (
                              sc.chatter_login IS NULL
                              OR sc.chatter_login = ''
                              OR sc.chatter_login NOT IN (
                                  SELECT chatter_login FROM twitch_chatter_rollup
                                  WHERE LOWER(streamer_login) = %s
                                    AND first_seen_at < %s
                                    AND {rollup_bot_clause}
                              )
                          )
                        """,
                        (
                            target_session_id,
                            executed_at,
                            *session_bot_params,
                            to_login,
                            executed_at,
                            *rollup_bot_params,
                        ),
                    ).fetchone()
                    new_chatters = new_chat_row["new_chatters"] if new_chat_row else 0

                    pg.execute(
                        """
                        INSERT INTO twitch_raid_retention
                            (raid_id, from_broadcaster_login, to_broadcaster_login,
                             viewer_count_sent, executed_at, target_session_id,
                             chatters_at_plus5m, chatters_at_plus15m, chatters_at_plus30m,
                             known_from_raider, new_to_target, new_chatters)
                        VALUES (%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s)
                        ON CONFLICT (raid_id, executed_at) DO NOTHING
                        """,
                        (
                            raid_id,
                            from_login,
                            to_login,
                            viewer_count,
                            executed_at,
                            target_session_id,
                            c5,
                            c15,
                            c30,
                            known_from_raider,
                            new_to_target,
                            new_chatters,
                        ),
                    )
                    processed += 1

            except Exception:
                log.exception("compute_raid_retention: error for raid_id=%s", raid_id)
                continue

        if processed:
            log.info("compute_raid_retention: inserted %d new rows", processed)
