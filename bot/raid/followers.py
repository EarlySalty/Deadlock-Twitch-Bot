from __future__ import annotations

import asyncio
from dataclasses import dataclass
from collections.abc import Mapping
from typing import Any, Awaitable, Callable, MutableMapping, Sequence

FollowerCandidate = MutableMapping[str, Any]
LoadCachedTotals = Callable[[Sequence[str]], Mapping[str, int] | Awaitable[Mapping[str, int]]]
ResolveUserToken = Callable[[str], str | None | Awaitable[str | None]]
FetchFollowersTotal = Callable[[str, str | None], int | None | Awaitable[int | None]]


@dataclass(slots=True)
class FollowerAuthContext:
    bot_token: str | None = None
    bot_scopes: set[str] | None = None


@dataclass(slots=True)
class _CandidateRef:
    index: int
    user_id: str | None
    login: str | None


def _normalize_login(value: object) -> str:
    return str(value or "").strip().lower()


def _normalize_user_id(value: object) -> str:
    return str(value or "").strip()


def _safe_int(value: object) -> int | None:
    try:
        if value is None:
            return None
        parsed = int(value)
    except (TypeError, ValueError):
        return None
    return parsed if parsed >= 0 else None


async def _maybe_await(value: object) -> object:
    if asyncio.iscoroutine(value) or isinstance(value, asyncio.Future):
        return await value
    return value


class FollowerTotalEnricher:
    """Hydrate and backfill `followers_total` values on raid candidate dicts."""

    def __init__(self, *, max_concurrency: int = 8) -> None:
        self._max_concurrency = max(1, int(max_concurrency or 1))

    async def enrich_candidates(
        self,
        candidates: list[FollowerCandidate],
        *,
        fetch_followers_total: FetchFollowersTotal,
        load_cached_totals: LoadCachedTotals | None = None,
        resolve_user_token: ResolveUserToken | None = None,
        auth_context: FollowerAuthContext | None = None,
        login_key: str = "user_login",
        user_id_key: str = "user_id",
        followers_total_key: str = "followers_total",
    ) -> None:
        if not candidates:
            return

        refs: list[_CandidateRef] = []
        cache_by_login: dict[str, int] = {}
        cache_by_user_id: dict[str, int] = {}
        pending_logins: list[str] = []
        pending_login_seen: set[str] = set()

        for index, candidate in enumerate(candidates):
            total = _safe_int(candidate.get(followers_total_key))
            login = _normalize_login(candidate.get(login_key))
            user_id = _normalize_user_id(candidate.get(user_id_key))

            if total is not None:
                if login:
                    cache_by_login[login] = total
                if user_id:
                    cache_by_user_id[user_id] = total
                continue

            refs.append(_CandidateRef(index=index, user_id=user_id or None, login=login or None))
            if login and login not in pending_login_seen:
                pending_login_seen.add(login)
                pending_logins.append(login)

        if not refs:
            return

        if load_cached_totals is not None and pending_logins:
            try:
                loaded = await _maybe_await(load_cached_totals(tuple(pending_logins)))
            except Exception:
                loaded = {}
            for raw_login, raw_total in dict(loaded or {}).items():
                login = _normalize_login(raw_login)
                total = _safe_int(raw_total)
                if not login or total is None:
                    continue
                cache_by_login[login] = total

        unresolved: list[_CandidateRef] = []
        for ref in refs:
            total = self._resolve_from_cache(ref, cache_by_login, cache_by_user_id)
            if total is not None:
                self._store_cache_value(ref, total, cache_by_login, cache_by_user_id)
                candidates[ref.index][followers_total_key] = total
                continue
            unresolved.append(ref)

        if not unresolved:
            return

        bot_token = auth_context.bot_token if auth_context else None
        bot_scopes = auth_context.bot_scopes if auth_context else None
        bot_can_read_followers = bool(
            bot_token
            and (
                bot_scopes is None
                or not bot_scopes
                or "moderator:read:followers" in bot_scopes
            )
        )

        semaphore = asyncio.Semaphore(self._max_concurrency)

        jobs: dict[str, list[_CandidateRef]] = {}
        for ref in unresolved:
            if self._resolve_from_cache(ref, cache_by_login, cache_by_user_id) is not None:
                continue
            if not ref.user_id:
                continue
            jobs.setdefault(ref.user_id, []).append(ref)

        async def _run_job(user_id: str, job_refs: list[_CandidateRef]) -> None:
            async with semaphore:
                total = await self._fetch_total(
                    user_id,
                    fetch_followers_total=fetch_followers_total,
                    resolve_user_token=resolve_user_token,
                    bot_can_read_followers=bot_can_read_followers,
                    bot_token=bot_token,
                )
            if total is None:
                return
            for ref in job_refs:
                if ref.user_id:
                    cache_by_user_id[ref.user_id] = total
                if ref.login:
                    cache_by_login[ref.login] = total
                candidates[ref.index][followers_total_key] = total

        await asyncio.gather(
            *(_run_job(user_id, job_refs) for user_id, job_refs in jobs.items())
        )

        for ref in unresolved:
            total = self._resolve_from_cache(ref, cache_by_login, cache_by_user_id)
            if total is not None:
                self._store_cache_value(ref, total, cache_by_login, cache_by_user_id)
                candidates[ref.index][followers_total_key] = total

    @staticmethod
    def _resolve_from_cache(
        ref: _CandidateRef,
        cache_by_login: dict[str, int],
        cache_by_user_id: dict[str, int],
    ) -> int | None:
        if ref.user_id and ref.user_id in cache_by_user_id:
            return cache_by_user_id[ref.user_id]
        if ref.login and ref.login in cache_by_login:
            return cache_by_login[ref.login]
        return None

    @staticmethod
    def _store_cache_value(
        ref: _CandidateRef,
        total: int,
        cache_by_login: dict[str, int],
        cache_by_user_id: dict[str, int],
    ) -> None:
        if ref.user_id:
            cache_by_user_id[ref.user_id] = total
        if ref.login:
            cache_by_login[ref.login] = total

    async def _fetch_total(
        self,
        user_id: str,
        *,
        fetch_followers_total: FetchFollowersTotal,
        resolve_user_token: ResolveUserToken | None,
        bot_can_read_followers: bool,
        bot_token: str | None,
    ) -> int | None:
        total = None

        if bot_can_read_followers and bot_token:
            try:
                total = await _maybe_await(fetch_followers_total(user_id, bot_token))
            except Exception:
                total = None

        if total is None and resolve_user_token is not None:
            try:
                resolved_token = await _maybe_await(resolve_user_token(user_id))
            except Exception:
                resolved_token = None
            if resolved_token:
                try:
                    total = await _maybe_await(fetch_followers_total(user_id, resolved_token))
                except Exception:
                    total = None

        parsed = _safe_int(total)
        if parsed is None:
            return None
        return parsed


async def enrich_followers_totals(
    candidates: list[FollowerCandidate],
    *,
    fetch_followers_total: FetchFollowersTotal,
    load_cached_totals: LoadCachedTotals | None = None,
    resolve_user_token: ResolveUserToken | None = None,
    auth_context: FollowerAuthContext | None = None,
    max_concurrency: int = 8,
    login_key: str = "user_login",
    user_id_key: str = "user_id",
    followers_total_key: str = "followers_total",
) -> None:
    await FollowerTotalEnricher(max_concurrency=max_concurrency).enrich_candidates(
        candidates,
        fetch_followers_total=fetch_followers_total,
        load_cached_totals=load_cached_totals,
        resolve_user_token=resolve_user_token,
        auth_context=auth_context,
        login_key=login_key,
        user_id_key=user_id_key,
        followers_total_key=followers_total_key,
    )
