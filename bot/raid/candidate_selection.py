from __future__ import annotations

import logging
from dataclasses import dataclass, field
from typing import Any, Awaitable, Callable, ContextManager

from bot.storage import readonly_connection

try:
    from .partner_scores import (
        load_partner_raid_score_map as default_load_partner_raid_score_map,
        refresh_partner_raid_score_async as default_refresh_partner_raid_score_async,
    )
except Exception:  # pragma: no cover - optional fallback import
    default_load_partner_raid_score_map = None
    default_refresh_partner_raid_score_async = None


log = logging.getLogger(__name__)

Candidate = dict[str, Any]
ScoreMap = dict[str, dict[str, object]]

LoadPartnerRaidScoreMapFn = Callable[[list[str]], ScoreMap]
RefreshPartnerRaidScoreAsyncFn = Callable[[str], Awaitable[Any]]
RecentRaidTargetsLoaderFn = Callable[[str, int], set[str]]
AttachFollowersTotalsFn = Callable[[list[Candidate]], Awaitable[None]]
ReadonlyConnectionFactory = Callable[[], ContextManager[Any]]


@dataclass(slots=True)
class PreparedPartnerScore:
    twitch_user_id: str
    twitch_login: str
    is_live: bool
    final_score: float
    today_received_raids: int
    duration_score: float
    time_pattern_score: float
    readiness_score: float
    fairness_score: float
    base_score: float
    new_partner_multiplier: float
    raid_boost_multiplier: float
    last_computed_at: Any = None

    def to_mapping(self) -> dict[str, Any]:
        return {
            "twitch_user_id": self.twitch_user_id,
            "twitch_login": self.twitch_login,
            "is_live": self.is_live,
            "final_score": self.final_score,
            "today_received_raids": self.today_received_raids,
            "duration_score": self.duration_score,
            "time_pattern_score": self.time_pattern_score,
            "readiness_score": self.readiness_score,
            "fairness_score": self.fairness_score,
            "base_score": self.base_score,
            "new_partner_multiplier": self.new_partner_multiplier,
            "raid_boost_multiplier": self.raid_boost_multiplier,
            "last_computed_at": self.last_computed_at,
        }


@dataclass(slots=True)
class CandidateSelectionService:
    load_partner_raid_score_map: LoadPartnerRaidScoreMapFn | None = None
    refresh_partner_raid_score_async: RefreshPartnerRaidScoreAsyncFn | None = None
    recent_raid_targets_loader: RecentRaidTargetsLoaderFn | None = None
    attach_followers_totals: AttachFollowersTotalsFn | None = None
    readonly_connection_factory: ReadonlyConnectionFactory | None = None
    logger: logging.Logger = field(default_factory=lambda: log)
    partner_score_threshold: float = 0.05
    recent_raid_cooldown_days: int = 7

    def load_prepared_partner_scores(self, twitch_user_ids: list[str]) -> ScoreMap:
        requested = [str(user_id or "").strip() for user_id in twitch_user_ids if str(user_id or "").strip()]
        if not requested:
            return {}

        loader = self.load_partner_raid_score_map or default_load_partner_raid_score_map
        if callable(loader):
            try:
                return loader(requested)
            except Exception:
                self.logger.debug("Prepared partner score helper failed", exc_info=True)

        sql = (
            "SELECT twitch_user_id, twitch_login, is_live, final_score, today_received_raids, "
            "duration_score, time_pattern_score, base_score, "
            "new_partner_multiplier, raid_boost_multiplier, last_computed_at "
            "FROM twitch_partner_raid_scores "
            f"WHERE twitch_user_id IN ({','.join('%s' for _ in requested)})"
        )
        try:
            connection_factory = self.readonly_connection_factory or readonly_connection
            with connection_factory() as conn:
                rows = conn.execute(sql, tuple(requested)).fetchall()
        except Exception:
            self.logger.debug("Prepared partner score DB query failed", exc_info=True)
            return {}

        out: ScoreMap = {}
        for row in rows:
            row_keys = row.keys() if hasattr(row, "keys") else None

            def _value(index: int, key: str) -> Any:
                if row_keys is not None:
                    return row[key]
                return row[index]

            twitch_user_id = str(_value(0, "twitch_user_id") or "").strip()
            if not twitch_user_id:
                continue

            duration_score = _safe_float(_value(5, "duration_score"), 0.5)
            time_pattern_score = _safe_float(_value(6, "time_pattern_score"), 0.5)
            base_score = _safe_float(_value(7, "base_score"), 0.5)
            readiness_score = _clamp((duration_score * 0.6) + (time_pattern_score * 0.4))
            fairness_score = _clamp((base_score - (readiness_score * 0.65)) / 0.35)

            score = PreparedPartnerScore(
                twitch_user_id=twitch_user_id,
                twitch_login=str(_value(1, "twitch_login") or "").strip().lower(),
                is_live=bool(_safe_int(_value(2, "is_live"), 0)),
                final_score=_safe_float(_value(3, "final_score"), 0.0),
                today_received_raids=_safe_int(_value(4, "today_received_raids"), 0),
                duration_score=duration_score,
                time_pattern_score=time_pattern_score,
                readiness_score=readiness_score,
                fairness_score=fairness_score,
                base_score=base_score,
                new_partner_multiplier=_safe_float(_value(8, "new_partner_multiplier"), 1.0),
                raid_boost_multiplier=_safe_float(_value(9, "raid_boost_multiplier"), 1.0),
                last_computed_at=_value(10, "last_computed_at"),
            )
            out[twitch_user_id] = score.to_mapping()

        return out

    async def refresh_partner_score_cache_if_available(
        self,
        twitch_user_id: str,
        *,
        reason: str,
    ) -> None:
        twitch_user_key = str(twitch_user_id or "").strip()
        if not twitch_user_key:
            return

        refresher = self.refresh_partner_raid_score_async or default_refresh_partner_raid_score_async
        if not callable(refresher):
            return

        try:
            await refresher(twitch_user_key)
            self.logger.info(
                "Prepared partner raid score cache refreshed for %s (%s)",
                twitch_user_key,
                reason,
            )
        except Exception:
            self.logger.debug(
                "Prepared partner raid score cache refresh failed for %s (%s)",
                twitch_user_key,
                reason,
                exc_info=True,
            )

    def get_recent_raid_targets(self, from_broadcaster_id: str, days: int) -> set[str]:
        normalized_id = str(from_broadcaster_id or "").strip()
        if not normalized_id or days <= 0:
            return set()

        loader = self.recent_raid_targets_loader
        if callable(loader):
            try:
                return {str(target or "").strip() for target in loader(normalized_id, days) if str(target or "").strip()}
            except Exception:
                self.logger.debug(
                    "Failed to load recent raid targets via injected helper for %s",
                    normalized_id,
                    exc_info=True,
                )

        cutoff = f"{int(days)} days"
        try:
            connection_factory = self.readonly_connection_factory or readonly_connection
            with connection_factory() as conn:
                rows = conn.execute(
                    """
                    SELECT DISTINCT to_broadcaster_id
                    FROM twitch_raid_history
                    WHERE from_broadcaster_id = %s
                      AND COALESCE(success, FALSE) IS TRUE
                      AND executed_at >= NOW() - (%s::interval)
                    """,
                    (normalized_id, cutoff),
                ).fetchall()
            return {str(row[0]) for row in rows if row and row[0]}
        except Exception:
            self.logger.debug(
                "Failed to load recent raid targets for %s",
                normalized_id,
                exc_info=True,
            )
            return set()

    async def select_partner_candidate_by_score(
        self,
        candidates: list[Candidate],
        from_broadcaster_id: str,
    ) -> Candidate | None:
        if not candidates:
            return None

        score_map = self.load_prepared_partner_scores(
            [str(candidate.get("user_id") or "").strip() for candidate in candidates]
        )

        scored_candidates: list[Candidate] = []
        cache_misses = 0
        stale_not_live = 0

        for candidate in candidates:
            twitch_user_id = str(candidate.get("user_id") or "").strip()
            candidate_login = str(candidate.get("user_login") or "").strip().lower()
            if not twitch_user_id:
                cache_misses += 1
                self.logger.info(
                    "Prepared partner score skipped for %s: missing twitch_user_id",
                    candidate_login or "<unknown>",
                )
                continue

            score_row = score_map.get(twitch_user_id)
            if not score_row:
                cache_misses += 1
                self.logger.info(
                    "Prepared partner score cache miss for %s (%s)",
                    candidate_login or twitch_user_id,
                    twitch_user_id,
                )
                continue

            if not bool(score_row.get("is_live")):
                stale_not_live += 1
                self.logger.info(
                    "Prepared partner score ignored for %s (%s): cache row is not live",
                    candidate_login or twitch_user_id,
                    twitch_user_id,
                )
                continue

            enriched = dict(candidate)
            enriched["_partner_score"] = score_row
            scored_candidates.append(enriched)

        if not scored_candidates:
            self.logger.info(
                "No prepared partner score candidate available for broadcaster_id=%s "
                "(input=%d, cache_misses=%d, stale_not_live=%d)",
                from_broadcaster_id,
                len(candidates),
                cache_misses,
                stale_not_live,
            )
            return None

        def _score(candidate: Candidate) -> float:
            score_row = candidate.get("_partner_score") or {}
            return _safe_float(score_row.get("final_score"), 0.0)

        def _today_received(candidate: Candidate) -> int:
            score_row = candidate.get("_partner_score") or {}
            return _safe_int(score_row.get("today_received_raids"), 10**9)

        def _fallback_sort_key(candidate: Candidate) -> tuple[int, int, str]:
            viewers = _safe_int(candidate.get("viewer_count"), 10**9)
            followers = _safe_int(candidate.get("followers_total"), 10**9)
            started_at = str(candidate.get("started_at") or "9999-99-99")
            return (viewers, followers, started_at)

        best_final_score = max(_score(candidate) for candidate in scored_candidates)
        close_candidates = [
            candidate
            for candidate in scored_candidates
            if abs(best_final_score - _score(candidate)) <= self.partner_score_threshold
        ]

        selection_reason = "highest_final_score"
        selected: Candidate
        if len(close_candidates) == 1:
            selected = close_candidates[0]
        else:
            lowest_today_received = min(_today_received(candidate) for candidate in close_candidates)
            tie_candidates = [
                candidate
                for candidate in close_candidates
                if _today_received(candidate) == lowest_today_received
            ]
            if len(tie_candidates) == 1:
                selection_reason = "today_received_raids"
                selected = tie_candidates[0]
            else:
                if callable(self.attach_followers_totals):
                    await self.attach_followers_totals(tie_candidates)
                tie_candidates.sort(key=_fallback_sort_key)
                selection_reason = "viewer_count_followers_started_at"
                selected = tie_candidates[0]

        selected_score = selected.get("_partner_score") or {}
        self.logger.info(
            "Partner raid target selection (prepared score): %s final=%.3f today=%s "
            "reason=%s cache_misses=%d stale_not_live=%d from %d candidates",
            selected.get("user_login"),
            _safe_float(selected_score.get("final_score"), 0.0),
            _safe_int(selected_score.get("today_received_raids"), 0),
            selection_reason,
            cache_misses,
            stale_not_live,
            len(candidates),
        )

        return selected

    async def select_fairest_candidate(
        self,
        candidates: list[Candidate],
        from_broadcaster_id: str,
    ) -> Candidate | None:
        if not candidates:
            return None

        recent_targets = self.get_recent_raid_targets(
            from_broadcaster_id,
            self.recent_raid_cooldown_days,
        )
        if recent_targets:
            filtered = [c for c in candidates if str(c.get("user_id") or "") not in recent_targets]
        else:
            filtered = []

        pool = filtered or candidates

        if callable(self.attach_followers_totals):
            await self.attach_followers_totals(pool)

        def _sort_key(candidate: Candidate) -> tuple[int, int, str]:
            viewers = _safe_int(candidate.get("viewer_count"), 10**9)
            followers = _safe_int(candidate.get("followers_total"), 10**9)
            started_at = str(candidate.get("started_at") or "9999-99-99")
            return (viewers, followers, started_at)

        pool.sort(key=_sort_key)

        selected = pool[0]
        self.logger.info(
            "Raid target selection (min viewers): %s (viewers=%s, followers=%s, recent_filtered=%d) from %d candidates",
            selected.get("user_login"),
            selected.get("viewer_count"),
            selected.get("followers_total"),
            max(0, len(candidates) - len(pool)),
            len(candidates),
        )

        return selected


def _safe_int(value: object, default: int = 0) -> int:
    try:
        if value is None:
            return default
        return int(value)
    except (TypeError, ValueError):
        return default


def _safe_float(value: object, default: float = 0.0) -> float:
    try:
        if value is None:
            return default
        return float(value)
    except (TypeError, ValueError):
        return default


def _clamp(value: float, minimum: float = 0.0, maximum: float = 1.0) -> float:
    return max(minimum, min(maximum, value))


__all__ = [
    "AttachFollowersTotalsFn",
    "CandidateSelectionService",
    "LoadPartnerRaidScoreMapFn",
    "PreparedPartnerScore",
    "ReadonlyConnectionFactory",
    "RecentRaidTargetsLoaderFn",
    "RefreshPartnerRaidScoreAsyncFn",
]
