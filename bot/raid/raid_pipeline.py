from __future__ import annotations

import asyncio
import inspect
import logging
import time
from dataclasses import dataclass
from typing import Any, Awaitable, Callable

log = logging.getLogger("TwitchStreams.RaidManager")


def _is_awaitable(value: object) -> bool:
    return inspect.isawaitable(value)


async def _maybe_await(value: object) -> object:
    if _is_awaitable(value):
        return await value  # type: ignore[no-any-return]
    return value


def is_retryable_raid_error(error: str | None) -> bool:
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


@dataclass(slots=True)
class RaidPipelineRequest:
    broadcaster_id: str
    broadcaster_login: str
    viewer_count: int
    stream_duration_sec: int
    online_partners: list[dict[str, Any]]
    session: Any | None
    api: Any | None = None
    category_id: str | None = None
    offline_trigger_ts: float | None = None
    reason: str = "auto_raid_on_offline"
    set_manual_suppression: bool = False


@dataclass(slots=True)
class RaidPipelineDependencies:
    load_raid_blacklist: Callable[[], tuple[set[str], set[str]]]
    add_to_blacklist: Callable[[str, str, str], None]
    select_partner_candidate_by_score: Callable[
        [list[dict[str, Any]], str],
        Awaitable[dict[str, Any] | None] | dict[str, Any] | None,
    ]
    select_fairest_candidate: Callable[
        [list[dict[str, Any]], str],
        Awaitable[dict[str, Any] | None] | dict[str, Any] | None,
    ]
    ensure_raid_arrival_subscription_ready: Callable[
        [str, str, str | None],
        Awaitable[bool] | bool,
    ]
    start_raid: Callable[..., Awaitable[tuple[bool, str | None]]]
    register_pending_raid: Callable[..., Awaitable[Any] | Any]
    mark_manual_raid_started: Callable[[str, float], Any]
    logger: logging.Logger = log
    next_raid_observability_flow_id: Callable[[str], str] = lambda prefix: f"{prefix}-{int(time.time() * 1000)}"
    increment_raid_disabled_strikes: Callable[[str, str, str], int] | None = None
    increment_raid_observability_counter: Callable[[str, int], int] | None = None
    log_raid_observability_event: Callable[..., Any] | None = None
    monotonic: Callable[[], float] = time.monotonic
    to_thread: Callable[..., Awaitable[Any]] = asyncio.to_thread
    load_outreach_boost_logins: Callable[[], dict[str, dict[str, Any]]] | None = None
    mark_outreach_boost_used: Callable[[str], bool] | None = None


class RaidPipelineService:
    def __init__(self, dependencies: RaidPipelineDependencies):
        self._deps = dependencies

    async def execute(self, request: RaidPipelineRequest) -> dict[str, object]:
        flow_start_ts = (
            request.offline_trigger_ts
            if request.offline_trigger_ts is not None
            else self._deps.monotonic()
        )
        offline_trigger_ts = flow_start_ts
        active_session = request.session
        if active_session is None:
            self._deps.logger.warning(
                "Raid pipeline unavailable for %s: no active HTTP session",
                request.broadcaster_login,
            )
            return {"status": "unavailable", "error": "no_active_session"}

        try:
            blacklisted_ids, blacklisted_logins = await self._deps.to_thread(
                self._deps.load_raid_blacklist
            )
        except Exception:
            self._deps.logger.exception("Raid pipeline blocked because blacklist load failed")
            return {"status": "blocked", "error": "blacklist_unavailable"}

        max_attempts = 3
        exclude_ids = {request.broadcaster_id}
        cached_de_streams: list[dict[str, Any]] | None = None

        outreach_boost_logins: dict[str, dict[str, Any]] = {}
        if callable(self._deps.load_outreach_boost_logins):
            try:
                loaded = self._deps.load_outreach_boost_logins()
                outreach_boost_logins = {
                    str(login or "").strip().lower(): info or {}
                    for login, info in (loaded or {}).items()
                    if str(login or "").strip()
                }
            except Exception:
                self._deps.logger.debug(
                    "Raid pipeline: Outreach-Boost-Loader fehlgeschlagen",
                    exc_info=True,
                )
                outreach_boost_logins = {}

        for attempt in range(max_attempts):
            attempt_start_ts = self._deps.monotonic()
            target: dict[str, Any] | None = None
            is_partner_raid = False
            is_outreach_boost = False
            candidates_count = 0

            if outreach_boost_logins and request.api and request.category_id:
                if cached_de_streams is None:
                    try:
                        cached_de_streams = await request.api.get_streams_by_category(
                            request.category_id,
                            language="de",
                            limit=50,
                        )
                    except Exception:
                        self._deps.logger.exception(
                            "Failed to get Deadlock-DE streams for outreach boost"
                        )
                        cached_de_streams = []

                boost_matches = [
                    stream_data
                    for stream_data in (cached_de_streams or [])
                    if (stream_data.get("user_login") or "").lower() in outreach_boost_logins
                    and stream_data.get("user_id") not in exclude_ids
                    and str(stream_data.get("user_id") or "") not in blacklisted_ids
                    and (stream_data.get("user_login") or "").lower() not in blacklisted_logins
                ]
                if boost_matches:
                    boost_matches.sort(
                        key=lambda s: (int(s.get("viewer_count") or 0), str(s.get("started_at") or ""))
                    )
                    target = boost_matches[0]
                    is_outreach_boost = True
                    candidates_count = len(boost_matches)
                    self._deps.logger.info(
                        "Raid pipeline: Outreach-Boost-Ziel gewählt %s -> %s (boost_pool=%d)",
                        request.broadcaster_login,
                        target.get("user_login"),
                        len(boost_matches),
                    )

            partner_candidates = [
                stream_data
                for stream_data in request.online_partners
                if stream_data.get("user_id") not in exclude_ids
                and bool(stream_data.get("raid_enabled", True))
                and str(stream_data.get("user_id") or "") not in blacklisted_ids
                and (stream_data.get("user_login") or "").lower() not in blacklisted_logins
            ]

            if not target and partner_candidates:
                target = await _maybe_await(
                    self._deps.select_partner_candidate_by_score(
                        partner_candidates,
                        request.broadcaster_id,
                    )
                )
                target = target if isinstance(target, dict) else None
                if target is not None:
                    is_partner_raid = True
                candidates_count = len(partner_candidates)

            if not target and request.api and request.category_id:
                if cached_de_streams is None:
                    try:
                        self._deps.logger.info(
                            "No partners online for %s, fetching Deadlock-DE fallback",
                            request.broadcaster_login,
                        )
                        cached_de_streams = await request.api.get_streams_by_category(
                            request.category_id,
                            language="de",
                            limit=50,
                        )
                    except Exception:
                        self._deps.logger.exception(
                            "Failed to get Deadlock-DE streams for fallback raid"
                        )
                        cached_de_streams = []

                fallback_candidates = [
                    stream_data
                    for stream_data in cached_de_streams
                    if stream_data.get("user_id") not in exclude_ids
                    and str(stream_data.get("user_id") or "") not in blacklisted_ids
                    and (stream_data.get("user_login") or "").lower() not in blacklisted_logins
                ]

                if fallback_candidates:
                    target = await _maybe_await(
                        self._deps.select_fairest_candidate(
                            fallback_candidates,
                            request.broadcaster_id,
                        )
                    )
                    target = target if isinstance(target, dict) else None
                    candidates_count = len(fallback_candidates)

            if not target:
                self._deps.logger.info(
                    "No valid raid target found for %s (Attempt %d/%d, total_elapsed=%.0fms, reason=%s)",
                    request.broadcaster_login,
                    attempt + 1,
                    max_attempts,
                    (self._deps.monotonic() - flow_start_ts) * 1000.0,
                    request.reason,
                )
                self._emit_observability_event(
                    flow_id=self._next_flow_id(prefix="raid-no-target"),
                    step="no_target",
                    decision="no_target",
                    from_broadcaster_login=request.broadcaster_login,
                    from_broadcaster_id=request.broadcaster_id,
                    details={"attempt": attempt + 1, "reason": request.reason},
                )
                return {"status": "no_target"}

            target_id = str(target.get("user_id") or "").strip()
            target_login = str(target.get("user_login") or "").strip().lower()
            target_started_at = target.get("started_at", "")
            if not target_id or not target_login:
                self._deps.logger.warning(
                    "Raid candidate missing identity: from=%s target_id=%r target_login=%r reason=%s",
                    request.broadcaster_login,
                    target_id,
                    target_login,
                    request.reason,
                )
                self._emit_observability_event(
                    flow_id=self._next_flow_id(prefix="raid-invalid-target"),
                    step="invalid_target",
                    decision="invalid_target_identity",
                    from_broadcaster_login=request.broadcaster_login,
                    from_broadcaster_id=request.broadcaster_id,
                    details={
                        "attempt": attempt + 1,
                        "max_attempts": max_attempts,
                        "reason": request.reason,
                    },
                )
                return {"status": "no_target", "error": "invalid_target_identity"}

            selection_ms = (self._deps.monotonic() - attempt_start_ts) * 1000.0
            raid_flow_id = self._next_flow_id(prefix="raid")
            self._increment_counter("raid_flow_started_total")
            self._emit_observability_event(
                flow_id=raid_flow_id,
                step="attempt_selected",
                decision="candidate_selected",
                from_broadcaster_login=request.broadcaster_login,
                from_broadcaster_id=request.broadcaster_id,
                to_broadcaster_login=target_login,
                to_broadcaster_id=target_id,
                details={
                    "attempt": attempt + 1,
                    "max_attempts": max_attempts,
                    "selection_ms": int(selection_ms),
                    "candidates_count": candidates_count,
                    "reason": request.reason,
                },
            )
            self._deps.logger.info(
                "Executing raid attempt %d/%d: %s -> %s (selection %.0fms, candidates=%d, reason=%s)",
                attempt + 1,
                max_attempts,
                request.broadcaster_login,
                target_login,
                selection_ms,
                candidates_count,
                request.reason,
            )

            channel_raid_ready = await _maybe_await(
                self._deps.ensure_raid_arrival_subscription_ready(
                    target_id,
                    target_login,
                    raid_flow_id,
                )
            )
            channel_raid_ready = bool(channel_raid_ready)

            api_call_start = self._deps.monotonic()
            success, error = await self._deps.start_raid(
                from_broadcaster_id=request.broadcaster_id,
                from_broadcaster_login=request.broadcaster_login,
                to_broadcaster_id=target_id,
                to_broadcaster_login=target_login,
                viewer_count=request.viewer_count,
                stream_duration_sec=request.stream_duration_sec,
                target_stream_started_at=target_started_at,
                candidates_count=candidates_count,
                session=active_session,
                reason=request.reason,
            )
            api_call_ms = (self._deps.monotonic() - api_call_start) * 1000.0
            total_ms = (self._deps.monotonic() - flow_start_ts) * 1000.0

            if success:
                await _maybe_await(
                    self._deps.register_pending_raid(
                        from_broadcaster_login=request.broadcaster_login,
                        to_broadcaster_id=target_id,
                        to_broadcaster_login=target_login,
                        target_stream_data=target,
                        is_partner_raid=is_partner_raid,
                        viewer_count=request.viewer_count,
                        offline_trigger_ts=offline_trigger_ts,
                        raid_flow_id=raid_flow_id,
                        channel_raid_ready=channel_raid_ready,
                    )
                )
                if request.set_manual_suppression:
                    self._deps.mark_manual_raid_started(
                        broadcaster_id=str(request.broadcaster_id),
                        ttl_seconds=180.0,
                    )
                if is_outreach_boost and callable(self._deps.mark_outreach_boost_used):
                    try:
                        marked = self._deps.mark_outreach_boost_used(target_login)
                        if marked:
                            self._deps.logger.info(
                                "Raid pipeline: Outreach-Boost verbraucht für %s",
                                target_login,
                            )
                    except Exception:
                        self._deps.logger.debug(
                            "Raid pipeline: Outreach-Boost-Markierung fehlgeschlagen für %s",
                            target_login,
                            exc_info=True,
                        )
                self._deps.logger.info(
                    "Raid attempt %d/%d succeeded (%s -> %s) api=%.0fms, total_elapsed=%.0fms, reason=%s",
                    attempt + 1,
                    max_attempts,
                    request.broadcaster_login,
                    target_login,
                    api_call_ms,
                    total_ms,
                    request.reason,
                )
                self._emit_observability_event(
                    flow_id=raid_flow_id,
                    step="raid_started",
                    decision="success",
                    from_broadcaster_login=request.broadcaster_login,
                    from_broadcaster_id=request.broadcaster_id,
                    to_broadcaster_login=target_login,
                    to_broadcaster_id=target_id,
                    details={"api_call_ms": int(api_call_ms), "total_ms": int(total_ms)},
                )
                return {
                    "status": "started",
                    "target_login": target_login,
                    "target": target,
                    "is_partner_raid": is_partner_raid,
                    "viewer_count": request.viewer_count,
                }

            exclude_ids.add(target_id)

            if is_retryable_raid_error(error):
                if is_partner_raid:
                    self._deps.logger.warning(
                        "Raid failed: Partner target %s does not allow raids. Skipping without blacklist.",
                        target_login,
                    )
                    blacklist_decision = "skip_blacklist"
                else:
                    if self._deps.increment_raid_disabled_strikes is not None:
                        strikes = self._deps.increment_raid_disabled_strikes(
                            target_id, target_login, error or "unknown_error"
                        )
                    else:
                        strikes = 2
                    if strikes >= 2:
                        self._deps.logger.warning(
                            "Raid failed: Target %s does not allow raids (strike %d/2). Blacklisting and retrying.",
                            target_login,
                            strikes,
                        )
                        self._deps.add_to_blacklist(target_id, target_login, error or "unknown_error")
                        blacklist_decision = "retry"
                    else:
                        self._deps.logger.info(
                            "Raid failed: Target %s does not allow raids (strike %d/2). Not yet blacklisting, retrying.",
                            target_login,
                            strikes,
                        )
                        blacklist_decision = "retry_no_blacklist"
                self._emit_observability_event(
                    flow_id=raid_flow_id,
                    step="raid_failed_retryable",
                    decision=blacklist_decision,
                    from_broadcaster_login=request.broadcaster_login,
                    from_broadcaster_id=request.broadcaster_id,
                    to_broadcaster_login=target_login,
                    to_broadcaster_id=target_id,
                    details={"error": error, "attempt": attempt + 1},
                )
                continue

            self._deps.logger.error(
                "Raid failed with non-retriable error after %.0fms (api=%.0fms, attempt=%d/%d, reason=%s): %s",
                total_ms,
                api_call_ms,
                attempt + 1,
                max_attempts,
                request.reason,
                error,
            )
            self._emit_observability_event(
                flow_id=raid_flow_id,
                step="raid_failed",
                decision="non_retryable",
                level="error",
                from_broadcaster_login=request.broadcaster_login,
                from_broadcaster_id=request.broadcaster_id,
                to_broadcaster_login=target_login,
                to_broadcaster_id=target_id,
                details={"error": error, "attempt": attempt + 1},
            )
            return {"status": "raid_failed", "error": error or "unknown_error"}

        return {"status": "raid_failed", "error": "no_valid_target_after_retries"}

    def _next_flow_id(self, *, prefix: str) -> str:
        try:
            flow_id = self._deps.next_raid_observability_flow_id(prefix)
        except Exception:
            flow_id = ""
        return str(flow_id or "").strip() or f"{prefix}-{int(self._deps.monotonic() * 1000)}"

    def _increment_counter(self, name: str, amount: int = 1) -> None:
        increment = self._deps.increment_raid_observability_counter
        if not callable(increment):
            return
        try:
            increment(name, amount)
        except Exception:
            self._deps.logger.debug("Raid pipeline observability counter update failed: %s", name, exc_info=True)

    def _emit_observability_event(self, **payload: Any) -> None:
        emit = self._deps.log_raid_observability_event
        if not callable(emit):
            return
        try:
            emit(**payload)
        except Exception:
            self._deps.logger.debug("Raid pipeline observability event failed", exc_info=True)


async def execute_raid_pipeline(
    dependencies: RaidPipelineDependencies,
    request: RaidPipelineRequest,
) -> dict[str, object]:
    return await RaidPipelineService(dependencies).execute(request)
