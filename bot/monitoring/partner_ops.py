"""Partner raid score refresh helpers for the Twitch monitoring mixin."""

from __future__ import annotations

import asyncio
import inspect
import time

from ..core.constants import log


def partner_raid_score_refresh_interval_seconds(_host) -> float:
    return 300.0


def partner_raid_score_refresh_preferred_names(*, full_refresh: bool) -> tuple[str, ...]:
    if full_refresh:
        return (
            "refresh_all_partner_raid_scores_async",
            "refresh_all_partner_raid_score_caches_async",
            "refresh_partner_raid_score_cache_async",
            "refresh_partner_raid_scores_async",
            "refresh_all_partner_raid_scores",
            "refresh_all_partner_raid_score_caches",
            "refresh_partner_raid_score_cache",
            "refresh_partner_raid_scores",
        )
    return (
        "refresh_partner_raid_score_cache_async",
        "refresh_partner_raid_scores_async",
        "refresh_partner_raid_score_async",
        "refresh_partner_raid_score_cache",
        "refresh_partner_raid_scores",
        "refresh_partner_raid_score",
    )


def partner_raid_score_refresh_candidates(host) -> list[object]:
    candidates: list[object] = []
    for candidate in (
        getattr(host, "partner_raid_score_service", None),
        getattr(getattr(host, "_raid_bot", None), "partner_raid_score_service", None),
        getattr(host, "_raid_bot", None),
        host,
    ):
        if candidate is None or candidate in candidates:
            continue
        candidates.append(candidate)
    return candidates


def build_partner_raid_score_refresh_kwargs(
    func,
    *,
    twitch_user_id: str | None,
    login: str | None,
    trigger: str,
    full_refresh: bool,
) -> dict[str, object]:
    try:
        params = inspect.signature(func).parameters
    except (TypeError, ValueError):
        params = {}

    kwargs: dict[str, object] = {}
    for name in params:
        if name in {"self", "cls"}:
            continue
        if name in {"twitch_user_id", "broadcaster_user_id", "user_id", "partner_user_id"}:
            if twitch_user_id:
                kwargs[name] = twitch_user_id
            continue
        if name in {"login", "twitch_login", "broadcaster_login", "partner_login"}:
            if login:
                kwargs[name] = login
            continue
        if name in {"trigger", "reason", "source"}:
            kwargs[name] = trigger
            continue
        if name in {"full_refresh", "bulk", "refresh_all"}:
            kwargs[name] = full_refresh
            continue
        if name in {"immediate", "force"}:
            kwargs[name] = True
            continue
    return kwargs


async def request_partner_raid_score_refresh(
    host,
    *,
    twitch_user_id: str | None = None,
    login: str | None = None,
    trigger: str,
    full_refresh: bool = False,
) -> bool:
    preferred_names = host._partner_raid_score_refresh_preferred_names(full_refresh=full_refresh)

    for candidate in host._partner_raid_score_refresh_candidates():
        for name in preferred_names:
            func = getattr(candidate, name, None)
            if not callable(func):
                continue
            try:
                func_params = inspect.signature(func).parameters
            except (TypeError, ValueError):
                func_params = {}
            kwargs = host._build_partner_raid_score_refresh_kwargs(
                func,
                twitch_user_id=twitch_user_id,
                login=login,
                trigger=trigger,
                full_refresh=full_refresh,
            )
            if not full_refresh and not twitch_user_id:
                required_id_names = {
                    "twitch_user_id",
                    "broadcaster_user_id",
                    "user_id",
                    "partner_user_id",
                }
                if any(required_name in func_params for required_name in required_id_names):
                    continue
            try:
                if inspect.iscoroutinefunction(func):
                    result = await func(**kwargs)
                else:
                    result = await asyncio.to_thread(func, **kwargs)
            except Exception:
                log.exception(
                    "Partner raid score refresh handler failed (candidate=%s, func=%s)",
                    type(candidate).__name__,
                    name,
                )
                continue
            if inspect.isawaitable(result):
                await result
            return True

    log.debug(
        "Partner raid score refresh skipped: no refresh service available (trigger=%s, user_id=%s, full=%s)",
        trigger,
        twitch_user_id or login or "",
        full_refresh,
    )
    return False


async def run_partner_raid_score_refresh_task(
    host,
    *,
    task_key: str,
    twitch_user_id: str | None,
    login: str | None,
    trigger: str,
    full_refresh: bool,
) -> None:
    try:
        await host._request_partner_raid_score_refresh(
            twitch_user_id=twitch_user_id,
            login=login,
            trigger=trigger,
            full_refresh=full_refresh,
        )
    except Exception:
        log.exception(
            "Partner raid score refresh failed (trigger=%s, user_id=%s, full=%s)",
            trigger,
            twitch_user_id or login or "",
            full_refresh,
        )
    finally:
        pending = getattr(host, "_partner_raid_score_refresh_pending", None)
        if isinstance(pending, set):
            pending.discard(task_key)


def schedule_partner_raid_score_refresh(
    host,
    *,
    twitch_user_id: str | None = None,
    login: str | None = None,
    trigger: str,
    full_refresh: bool = False,
) -> bool:
    key = f"all:{trigger}" if full_refresh else str(twitch_user_id or login or "").strip().lower()
    if not key:
        return False

    pending = getattr(host, "_partner_raid_score_refresh_pending", None)
    if not isinstance(pending, set):
        pending = set()
        host._partner_raid_score_refresh_pending = pending
    if key in pending:
        return False
    pending.add(key)

    task = host._run_partner_raid_score_refresh_task(
        task_key=key,
        twitch_user_id=twitch_user_id,
        login=login,
        trigger=trigger,
        full_refresh=full_refresh,
    )
    spawn = getattr(host, "_spawn_bg_task", None)
    if callable(spawn):
        spawn(task, f"partner_raid_score_refresh.{key}")
    else:
        asyncio.create_task(task, name=f"partner_raid_score_refresh.{key}")
    return True


def schedule_partner_raid_score_refreshes(
    host,
    refreshes: list[tuple[str, str | None, str]],
) -> int:
    scheduled = 0
    seen: set[str] = set()
    for twitch_user_id, login, trigger in refreshes:
        user_id = str(twitch_user_id or "").strip()
        dedupe_key = user_id or str(login or "").strip().lower()
        if not dedupe_key or dedupe_key in seen:
            continue
        seen.add(dedupe_key)
        if host._schedule_partner_raid_score_refresh(
            twitch_user_id=user_id or None,
            login=login,
            trigger=trigger,
        ):
            scheduled += 1
    return scheduled


def maybe_schedule_partner_raid_score_reconciliation(host, *, trigger: str) -> bool:
    now = time.monotonic()
    interval = host._partner_raid_score_refresh_interval_seconds()
    last_run = float(
        getattr(host, "_partner_raid_score_reconciliation_last_monotonic", 0.0) or 0.0
    )
    if last_run and (now - last_run) < max(60.0, interval):
        return False
    host._partner_raid_score_reconciliation_last_monotonic = now
    return host._schedule_partner_raid_score_refresh(
        trigger=trigger,
        full_refresh=True,
    )


__all__ = [
    "build_partner_raid_score_refresh_kwargs",
    "maybe_schedule_partner_raid_score_reconciliation",
    "partner_raid_score_refresh_candidates",
    "partner_raid_score_refresh_interval_seconds",
    "partner_raid_score_refresh_preferred_names",
    "request_partner_raid_score_refresh",
    "run_partner_raid_score_refresh_task",
    "schedule_partner_raid_score_refresh",
    "schedule_partner_raid_score_refreshes",
]
