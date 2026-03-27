from __future__ import annotations

import asyncio
import inspect
from dataclasses import dataclass
from collections.abc import Mapping
from typing import Any, Awaitable, Callable, MutableMapping, Sequence

FollowerCandidate = MutableMapping[str, Any]
LoadCachedTotals = Callable[[Sequence[str]], Mapping[str, int] | Awaitable[Mapping[str, int]]]
ResolveUserToken = Callable[[str], str | None | Awaitable[str | None]]
FetchFollowersTotal = Callable[[str, str | None], int | None | Awaitable[int | None]]
CreateTwitchApi = Callable[[Any], Any]
ResolveBotOauthContext = Callable[[], Awaitable[tuple[str | None, str | None, set[str]]] | tuple[str | None, str | None, set[str]]]
IncrementCounter = Callable[[str, int], int]
ScopeFallbackWarning = Callable[..., None]
GetFollowersTotalResult = Callable[[Any, str, str | None], Awaitable[dict[str, object]] | dict[str, object]]
ResolveValidToken = Callable[[str, Any], Awaitable[str | None] | str | None]


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
    if inspect.isawaitable(value):
        return await value
    return value


@dataclass(slots=True)
class CandidateFollowersDependencies:
    create_twitch_api: CreateTwitchApi
    resolve_bot_oauth_context: ResolveBotOauthContext
    get_followers_total_result: GetFollowersTotalResult
    resolve_valid_token: ResolveValidToken
    increment_counter: IncrementCounter
    warn_user_scope_fallback_once: ScopeFallbackWarning
    clear_user_scope_fallback_warning: ScopeFallbackWarning
    logger: Any


class CandidateFollowersService:
    def __init__(
        self,
        dependencies: CandidateFollowersDependencies,
        *,
        max_concurrency: int = 8,
    ) -> None:
        self._deps = dependencies
        self._max_concurrency = max(1, int(max_concurrency or 1))

    async def attach_followers_totals(
        self,
        candidates: list[FollowerCandidate],
        *,
        session: Any,
        auth_subject_label_by_id: Mapping[str, str] | None = None,
    ) -> None:
        if not candidates or not session:
            return

        api = self._deps.create_twitch_api(session)
        bot_token, _bot_id, bot_scopes = await _maybe_await(
            self._deps.resolve_bot_oauth_context()
        )
        bot_scope_set = set(bot_scopes or set())
        candidate_labels = {
            str(candidate.get("user_id") or "").strip(): str(candidate.get("user_login") or "").strip().lower()
            for candidate in candidates
            if str(candidate.get("user_id") or "").strip()
        }
        if auth_subject_label_by_id:
            candidate_labels.update(
                {
                    str(user_id or "").strip(): str(label or "").strip().lower()
                    for user_id, label in auth_subject_label_by_id.items()
                    if str(user_id or "").strip()
                }
            )

        async def _load_cached_totals(logins: tuple[str, ...]) -> dict[str, int]:
            if not logins:
                return {}
            try:
                from bot.storage import readonly_connection

                placeholders = ",".join("%s" for _ in logins)
                with readonly_connection() as conn:
                    rows = conn.execute(
                        f"""
                        SELECT streamer_login, COALESCE(followers_end, followers_start) AS follower_total
                          FROM twitch_stream_sessions
                         WHERE streamer_login IN ({placeholders})
                           AND COALESCE(followers_end, followers_start) IS NOT NULL
                         ORDER BY COALESCE(ended_at, started_at) DESC
                        """,
                        logins,
                    ).fetchall()
            except Exception:
                self._deps.logger.debug("followers_totals: DB cache query failed", exc_info=True)
                return {}

            db_map: dict[str, int] = {}
            for row in rows:
                login = str(row[0] or "").strip().lower()
                if not login or login in db_map or row[1] is None:
                    continue
                db_map[login] = int(row[1])
            return db_map

        async def _resolve_user_token(user_id: str) -> str | None:
            try:
                return await _maybe_await(self._deps.resolve_valid_token(user_id, session))
            except Exception:
                return None

        async def _fetch_followers_total(user_id: str, user_token: str | None) -> int | None:
            label = candidate_labels.get(user_id) or user_id
            is_bot_path = bool(bot_token and user_token and user_token == bot_token)
            if is_bot_path:
                self._deps.increment_counter("followers_candidate_bot_path_attempt_total", 1)
            else:
                self._deps.warn_user_scope_fallback_once(
                    area="raid candidate follower lookup",
                    subject=label,
                )

            result = await _maybe_await(
                self._deps.get_followers_total_result(api, user_id, user_token)
            )
            if result.get("ok") and result.get("data") is not None:
                if is_bot_path:
                    self._deps.clear_user_scope_fallback_warning(
                        area="raid candidate follower lookup",
                        subject=label,
                    )
                    self._deps.increment_counter(
                        "followers_candidate_bot_path_success_total",
                        1,
                    )
                else:
                    self._deps.increment_counter(
                        "followers_candidate_reason_fallback_to_streamer_token_total",
                        1,
                    )
                return int(result["data"])

            error_code = str(result.get("error_code") or "helix_followers_failed")
            if is_bot_path:
                self._deps.increment_counter("followers_candidate_bot_path_failure_total", 1)
            else:
                self._deps.increment_counter(
                    f"followers_candidate_reason_{error_code}_total",
                    1,
                )
            return None

        await FollowerTotalEnricher(max_concurrency=self._max_concurrency).enrich_candidates(
            candidates,
            load_cached_totals=_load_cached_totals,
            fetch_followers_total=_fetch_followers_total,
            resolve_user_token=_resolve_user_token,
            auth_context=FollowerAuthContext(
                bot_token=bot_token,
                bot_scopes=bot_scope_set,
            ),
        )


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
