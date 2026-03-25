"""Prepared partner raid score cache helpers."""

from __future__ import annotations

import asyncio
import contextlib
import logging
from collections.abc import Iterable, Sequence
from dataclasses import dataclass
from datetime import UTC, date, datetime, timedelta
from typing import Any
from zoneinfo import ZoneInfo, ZoneInfoNotFoundError

from ..entitlements.catalog import legacy_plan_name_has_entitlement, plan_has_entitlement
from ..storage import transaction

log = logging.getLogger("TwitchStreams.PartnerRaidScores")

LOOKBACK_DAYS = 45
MIN_RELIABLE_SESSIONS = 3
NEUTRAL_SCORE = 0.5
NEW_PARTNER_MAX_MULTIPLIER = 1.25
NEW_PARTNER_RAID_THRESHOLD = 10
RAID_BOOST_MULTIPLIER = 1.25
DEFAULT_RAID_BOOST_MULTIPLIER = 1.0
READINESS_DURATION_WEIGHT = 0.6
READINESS_TIME_WEIGHT = 0.4
FINAL_READINESS_WEIGHT = 0.65
FINAL_FAIRNESS_WEIGHT = 0.35
FAIRNESS_BALANCE_DIVISOR = 20.0
FAIRNESS_RECEIVED_7D_THRESHOLD = 5.0
try:
    BERLIN_TZ = ZoneInfo("Europe/Berlin")
except ZoneInfoNotFoundError:  # pragma: no cover - environment dependent fallback
    BERLIN_TZ = UTC


def _executemany(conn, sql: str, params_seq) -> None:
    executemany = getattr(conn, "executemany", None)
    if callable(executemany):
        executemany(sql, params_seq)
        return
    with contextlib.closing(conn.cursor()) as cur:
        cur.executemany(sql, params_seq)


@dataclass(slots=True)
class _PartnerRow:
    twitch_user_id: str
    twitch_login: str
    is_partner_active: bool


@dataclass(slots=True)
class _PreparedScore:
    twitch_user_id: str
    twitch_login: str
    avg_duration_sec: int
    time_pattern_score_base: float
    received_successful_raids_total: int
    is_new_partner_preferred: bool
    new_partner_multiplier: float
    raid_boost_multiplier: float
    is_live: bool
    current_started_at: str | None
    current_uptime_sec: int
    duration_score: float
    time_pattern_score: float
    readiness_score: float
    fairness_score: float
    base_score: float
    final_score: float
    internal_sent_raids_30d: int
    internal_received_raids_30d: int
    internal_received_raids_7d: int
    today_received_raids: int
    last_computed_at: str

    def as_db_tuple(self) -> tuple[object, ...]:
        return (
            self.twitch_user_id,
            self.twitch_login,
            self.avg_duration_sec,
            self.time_pattern_score_base,
            self.received_successful_raids_total,
            int(self.is_new_partner_preferred),
            self.new_partner_multiplier,
            self.raid_boost_multiplier,
            int(self.is_live),
            self.current_started_at,
            self.current_uptime_sec,
            self.duration_score,
            self.time_pattern_score,
            self.readiness_score,
            self.fairness_score,
            self.base_score,
            self.final_score,
            self.internal_sent_raids_30d,
            self.internal_received_raids_30d,
            self.internal_received_raids_7d,
            self.today_received_raids,
            self.last_computed_at,
        )

    def as_dict(self) -> dict[str, object]:
        return {
            "twitch_user_id": self.twitch_user_id,
            "twitch_login": self.twitch_login,
            "avg_duration_sec": self.avg_duration_sec,
            "time_pattern_score_base": self.time_pattern_score_base,
            "received_successful_raids_total": self.received_successful_raids_total,
            "is_new_partner_preferred": self.is_new_partner_preferred,
            "new_partner_multiplier": self.new_partner_multiplier,
            "raid_boost_multiplier": self.raid_boost_multiplier,
            "is_live": self.is_live,
            "current_started_at": self.current_started_at,
            "current_uptime_sec": self.current_uptime_sec,
            "duration_score": self.duration_score,
            "time_pattern_score": self.time_pattern_score,
            "readiness_score": self.readiness_score,
            "fairness_score": self.fairness_score,
            "base_score": self.base_score,
            "final_score": self.final_score,
            "internal_sent_raids_30d": self.internal_sent_raids_30d,
            "internal_received_raids_30d": self.internal_received_raids_30d,
            "internal_received_raids_7d": self.internal_received_raids_7d,
            "today_received_raids": self.today_received_raids,
            "last_computed_at": self.last_computed_at,
        }


def _normalized_ids(user_ids: Iterable[str] | None) -> list[str]:
    unique: list[str] = []
    seen: set[str] = set()
    for raw in user_ids or ():
        value = str(raw or "").strip()
        if not value or value in seen:
            continue
        seen.add(value)
        unique.append(value)
    return unique


def _row_value(row: Any, key: str, default: object = None) -> object:
    if row is None:
        return default
    if hasattr(row, "keys"):
        try:
            return row[key]
        except Exception:
            return default
    if isinstance(row, dict):
        return row.get(key, default)
    return default


def _row_dict(row: Any) -> dict[str, object]:
    if row is None:
        return {}
    if isinstance(row, dict):
        return dict(row)
    if hasattr(row, "keys"):
        return {str(key): row[key] for key in row.keys()}
    return {}


def _parse_dt(value: object) -> datetime | None:
    text = str(value or "").strip()
    if not text:
        return None
    normalized = text.replace("Z", "+00:00")
    try:
        parsed = datetime.fromisoformat(normalized)
    except ValueError:
        return None
    if parsed.tzinfo is None:
        parsed = parsed.replace(tzinfo=UTC)
    return parsed.astimezone(UTC)


def _iso_utc(value: datetime) -> str:
    return value.astimezone(UTC).isoformat(timespec="seconds")


def _clamp(value: float, minimum: float = 0.0, maximum: float = 1.0) -> float:
    return max(minimum, min(maximum, value))


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


def _round_score(value: float) -> float:
    return round(float(value), 6)


def _column_exists(conn, table: str, column: str) -> bool:
    try:
        row = conn.execute(
            """
            SELECT 1
            FROM information_schema.columns
            WHERE table_schema = current_schema()
              AND table_name = %s
              AND column_name = %s
            """,
            (table, column),
        ).fetchone()
        return bool(row)
    except Exception:
        try:
            rows = conn.execute(f"PRAGMA table_info({table})").fetchall()
        except Exception:
            return False
        for row in rows or ():
            name = ""
            if hasattr(row, "keys"):
                name = str(row["name"] or "").strip()
            elif len(row) > 1:
                name = str(row[1] or "").strip()
            if name == column:
                return True
        return False


def _placeholders(values: Sequence[object]) -> str:
    return ",".join("%s" for _ in values)


def _today_in_berlin(now_utc: datetime) -> date:
    return now_utc.astimezone(BERLIN_TZ).date()


def _new_partner_multiplier(received_successful_raids_total: int) -> float:
    capped = max(0, min(int(received_successful_raids_total or 0), NEW_PARTNER_RAID_THRESHOLD))
    step = (NEW_PARTNER_MAX_MULTIPLIER - 1.0) / float(NEW_PARTNER_RAID_THRESHOLD)
    return _round_score(max(1.0, NEW_PARTNER_MAX_MULTIPLIER - (step * capped)))


def _readiness_score(duration_score: float, time_pattern_score: float) -> float:
    return _round_score(
        (duration_score * READINESS_DURATION_WEIGHT)
        + (time_pattern_score * READINESS_TIME_WEIGHT)
    )


def _fairness_score(
    *,
    sent_30d: int,
    received_30d: int,
    received_7d: int,
    today_received_raids: int,
) -> float:
    balance_30d = _clamp(
        0.5 + ((int(sent_30d or 0) - int(received_30d or 0)) / FAIRNESS_BALANCE_DIVISOR)
    )
    received_7d_penalty = _clamp(
        1.0 - (int(received_7d or 0) / FAIRNESS_RECEIVED_7D_THRESHOLD)
    )
    today_penalty = 1.0 / (1.0 + max(0, int(today_received_raids or 0)))
    return _round_score(
        (balance_30d * 0.5)
        + (received_7d_penalty * 0.3)
        + (today_penalty * 0.2)
    )


def _combine_preboost_score(readiness_score: float, fairness_score: float) -> float:
    return _round_score(
        (readiness_score * FINAL_READINESS_WEIGHT)
        + (fairness_score * FINAL_FAIRNESS_WEIGHT)
    )


class PartnerRaidScoreService:
    """Computes and loads prepared partner raid scores."""

    def __init__(self, conn_factory=transaction):
        self._conn_factory = conn_factory
        self._runtime_schema_ready = False

    def refresh_partner_score(
        self,
        twitch_user_id: str,
        *,
        now: datetime | None = None,
    ) -> dict[str, object] | None:
        user_id = str(twitch_user_id or "").strip()
        if not user_id:
            return None
        prepared_rows = self._refresh_scores_for_ids([user_id], active_only=False, now=now)
        if not prepared_rows:
            return None
        return prepared_rows[0].as_dict()

    def refresh_all_partner_scores(
        self,
        *,
        now: datetime | None = None,
    ) -> dict[str, dict[str, object]]:
        prepared_rows = self._refresh_scores_for_ids(None, active_only=True, now=now)
        return {row.twitch_user_id: row.as_dict() for row in prepared_rows}

    def load_scores(
        self,
        twitch_user_ids: Iterable[str],
        *,
        live_only: bool = False,
    ) -> dict[str, dict[str, object]]:
        user_ids = _normalized_ids(twitch_user_ids)
        if not user_ids:
            return {}
        with self._conn_factory() as conn:
            self._ensure_runtime_schema(conn)
            rows = self._load_cached_rows(conn, user_ids, live_only=live_only)
        return {
            str(_row_value(row, "twitch_user_id", "")).strip(): _row_dict(row)
            for row in rows
            if str(_row_value(row, "twitch_user_id", "")).strip()
        }

    def _refresh_scores_for_ids(
        self,
        user_ids: Sequence[str] | None,
        *,
        active_only: bool,
        now: datetime | None,
    ) -> list[_PreparedScore]:
        now_utc = (now or datetime.now(UTC)).astimezone(UTC)
        lookback_cutoff = now_utc - timedelta(days=LOOKBACK_DAYS)

        with self._conn_factory() as conn:
            self._ensure_runtime_schema(conn)
            partners = self._load_partners(conn, user_ids, active_only=active_only)
            if not partners:
                return []

            partner_ids = [partner.twitch_user_id for partner in partners]
            partner_logins = [partner.twitch_login for partner in partners]

            live_state_by_id = self._load_live_state(conn, partner_ids)
            sessions_by_login = self._load_sessions(conn, partner_logins)
            raid_timestamps_by_id = self._load_raid_timestamps(conn, partner_ids)
            internal_raid_metrics = self._load_internal_raid_metrics(
                conn,
                partner_ids,
                now_utc=now_utc,
            )
            boost_flags = self._load_boost_flags(conn, partner_ids, now_utc=now_utc)
            existing_cache = self._load_cached_rows_by_id(conn, partner_ids)

            prepared_rows = [
                self._build_score(
                    partner=partner,
                    live_state=live_state_by_id.get(partner.twitch_user_id),
                    session_rows=sessions_by_login.get(partner.twitch_login, ()),
                    raid_timestamps=raid_timestamps_by_id.get(partner.twitch_user_id, ()),
                    internal_raid_metrics=internal_raid_metrics.get(
                        partner.twitch_user_id,
                        (0, 0, 0),
                    ),
                    raid_boost_enabled=boost_flags.get(partner.twitch_user_id, False),
                    existing_cache=existing_cache.get(partner.twitch_user_id),
                    now_utc=now_utc,
                    lookback_cutoff=lookback_cutoff,
                )
                for partner in partners
            ]

            self._upsert_scores(conn, prepared_rows)

        return prepared_rows

    def _ensure_runtime_schema(self, conn) -> None:
        if self._runtime_schema_ready:
            return
        required_columns = (
            (
                "readiness_score",
                "DOUBLE PRECISION NOT NULL DEFAULT 0.5",
            ),
            (
                "fairness_score",
                "DOUBLE PRECISION NOT NULL DEFAULT 0.5",
            ),
            (
                "internal_sent_raids_30d",
                "INTEGER NOT NULL DEFAULT 0",
            ),
            (
                "internal_received_raids_30d",
                "INTEGER NOT NULL DEFAULT 0",
            ),
            (
                "internal_received_raids_7d",
                "INTEGER NOT NULL DEFAULT 0",
            ),
        )
        for column, column_type in required_columns:
            if _column_exists(conn, "twitch_partner_raid_scores", column):
                continue
            conn.execute(
                f"ALTER TABLE twitch_partner_raid_scores ADD COLUMN {column} {column_type}"
            )
            log.info(
                "Partner raid score runtime schema guard added column %s.%s",
                "twitch_partner_raid_scores",
                column,
            )
        self._runtime_schema_ready = True

    def _load_partners(
        self,
        conn,
        user_ids: Sequence[str] | None,
        *,
        active_only: bool,
    ) -> list[_PartnerRow]:
        params: list[object] = []
        where_clauses = [
            "twitch_user_id IS NOT NULL",
            "twitch_login IS NOT NULL",
        ]
        if active_only:
            where_clauses.append("COALESCE(is_partner_active, 0) = 1")
        if user_ids:
            params.extend(user_ids)
            where_clauses.append(f"twitch_user_id IN ({_placeholders(user_ids)})")

        sql = (
            "SELECT twitch_user_id, twitch_login, COALESCE(is_partner_active, 0) AS is_partner_active "
            "FROM twitch_streamers_partner_state "
            f"WHERE {' AND '.join(where_clauses)} "
            "ORDER BY LOWER(twitch_login)"
        )
        rows = conn.execute(sql, params).fetchall()
        partners: list[_PartnerRow] = []
        for row in rows:
            twitch_user_id = str(_row_value(row, "twitch_user_id", "")).strip()
            twitch_login = str(_row_value(row, "twitch_login", "")).strip().lower()
            if not twitch_user_id or not twitch_login:
                continue
            partners.append(
                _PartnerRow(
                    twitch_user_id=twitch_user_id,
                    twitch_login=twitch_login,
                    is_partner_active=bool(_safe_int(_row_value(row, "is_partner_active", 0))),
                )
            )
        return partners

    def _load_live_state(self, conn, user_ids: Sequence[str]) -> dict[str, Any]:
        if not user_ids:
            return {}
        rows = conn.execute(
            "SELECT twitch_user_id, is_live, last_started_at "
            "FROM twitch_live_state "
            f"WHERE twitch_user_id IN ({_placeholders(user_ids)})",
            list(user_ids),
        ).fetchall()
        return {
            str(_row_value(row, "twitch_user_id", "")).strip(): row
            for row in rows
            if str(_row_value(row, "twitch_user_id", "")).strip()
        }

    def _load_sessions(self, conn, logins: Sequence[str]) -> dict[str, list[Any]]:
        if not logins:
            return {}
        lowered = [login.lower() for login in logins]
        rows = conn.execute(
            "SELECT streamer_login, started_at, duration_seconds "
            "FROM twitch_stream_sessions "
            f"WHERE LOWER(streamer_login) IN ({_placeholders(lowered)})",
            lowered,
        ).fetchall()
        sessions_by_login: dict[str, list[Any]] = {login: [] for login in lowered}
        for row in rows:
            login = str(_row_value(row, "streamer_login", "")).strip().lower()
            if not login:
                continue
            sessions_by_login.setdefault(login, []).append(row)
        return sessions_by_login

    def _load_raid_timestamps(self, conn, user_ids: Sequence[str]) -> dict[str, list[datetime]]:
        if not user_ids:
            return {}
        rows = conn.execute(
            "SELECT to_broadcaster_id, executed_at "
            "FROM twitch_raid_history "
            f"WHERE to_broadcaster_id IN ({_placeholders(user_ids)}) "
            "AND COALESCE(success, FALSE) IS TRUE",
            list(user_ids),
        ).fetchall()
        raid_timestamps: dict[str, list[datetime]] = {user_id: [] for user_id in user_ids}
        for row in rows:
            user_id = str(_row_value(row, "to_broadcaster_id", "")).strip()
            executed_at = _parse_dt(_row_value(row, "executed_at"))
            if not user_id or executed_at is None:
                continue
            raid_timestamps.setdefault(user_id, []).append(executed_at)
        return raid_timestamps

    def _load_internal_raid_metrics(
        self,
        conn,
        user_ids: Sequence[str],
        *,
        now_utc: datetime,
    ) -> dict[str, tuple[int, int, int]]:
        if not user_ids:
            return {}

        metrics: dict[str, list[int]] = {user_id: [0, 0, 0] for user_id in user_ids}
        partner_placeholders = _placeholders(user_ids)
        partner_params = list(user_ids)
        cutoff_30d = _iso_utc(now_utc - timedelta(days=30))
        cutoff_7d = _iso_utc(now_utc - timedelta(days=7))

        sent_rows = conn.execute(
            "SELECT from_broadcaster_id, COUNT(*) AS sent_count "
            "FROM twitch_raid_history "
            "WHERE COALESCE(success, FALSE) IS TRUE "
            f"AND from_broadcaster_id IN ({partner_placeholders}) "
            f"AND to_broadcaster_id IN ({partner_placeholders}) "
            "AND executed_at >= %s "
            "GROUP BY from_broadcaster_id",
            [*partner_params, *partner_params, cutoff_30d],
        ).fetchall()
        for row in sent_rows:
            user_id = str(_row_value(row, "from_broadcaster_id", "")).strip()
            if user_id in metrics:
                metrics[user_id][0] = _safe_int(_row_value(row, "sent_count", 0), 0)

        received_30d_rows = conn.execute(
            "SELECT to_broadcaster_id, COUNT(*) AS received_count "
            "FROM twitch_raid_history "
            "WHERE COALESCE(success, FALSE) IS TRUE "
            f"AND from_broadcaster_id IN ({partner_placeholders}) "
            f"AND to_broadcaster_id IN ({partner_placeholders}) "
            "AND executed_at >= %s "
            "GROUP BY to_broadcaster_id",
            [*partner_params, *partner_params, cutoff_30d],
        ).fetchall()
        for row in received_30d_rows:
            user_id = str(_row_value(row, "to_broadcaster_id", "")).strip()
            if user_id in metrics:
                metrics[user_id][1] = _safe_int(
                    _row_value(row, "received_count", 0),
                    0,
                )

        received_7d_rows = conn.execute(
            "SELECT to_broadcaster_id, COUNT(*) AS received_count "
            "FROM twitch_raid_history "
            "WHERE COALESCE(success, FALSE) IS TRUE "
            f"AND from_broadcaster_id IN ({partner_placeholders}) "
            f"AND to_broadcaster_id IN ({partner_placeholders}) "
            "AND executed_at >= %s "
            "GROUP BY to_broadcaster_id",
            [*partner_params, *partner_params, cutoff_7d],
        ).fetchall()
        for row in received_7d_rows:
            user_id = str(_row_value(row, "to_broadcaster_id", "")).strip()
            if user_id in metrics:
                metrics[user_id][2] = _safe_int(
                    _row_value(row, "received_count", 0),
                    0,
                )

        return {user_id: tuple(values) for user_id, values in metrics.items()}

    def _load_boost_flags(
        self,
        conn,
        user_ids: Sequence[str],
        *,
        now_utc: datetime,
    ) -> dict[str, bool]:
        if not user_ids:
            return {}
        rows = conn.execute(
            "SELECT twitch_user_id, COALESCE(raid_boost_enabled, 0) AS raid_boost_enabled, "
            "COALESCE(plan_name, '') AS plan_name, "
            "COALESCE(manual_plan_id, '') AS manual_plan_id, "
            "manual_plan_expires_at "
            "FROM streamer_plans "
            f"WHERE twitch_user_id IN ({_placeholders(user_ids)})",
            list(user_ids),
        ).fetchall()
        flags: dict[str, bool] = {}
        for row in rows:
            twitch_user_id = str(_row_value(row, "twitch_user_id", "")).strip()
            if not twitch_user_id:
                continue
            raid_boost_enabled = bool(_safe_int(_row_value(row, "raid_boost_enabled", 0)))
            plan_name = str(_row_value(row, "plan_name", "")).strip().lower()
            manual_plan_id = str(_row_value(row, "manual_plan_id", "")).strip().lower()
            manual_plan_expires_at = _parse_dt(_row_value(row, "manual_plan_expires_at"))
            manual_override_active = bool(
                manual_plan_id
                and (manual_plan_expires_at is None or manual_plan_expires_at >= now_utc)
            )
            flags[twitch_user_id] = bool(
                raid_boost_enabled
                or legacy_plan_name_has_entitlement(plan_name, "raid.priority")
                or (
                    manual_override_active
                    and plan_has_entitlement(manual_plan_id, "raid.priority")
                )
            )
        return flags

    def _load_cached_rows_by_id(self, conn, user_ids: Sequence[str]) -> dict[str, Any]:
        rows = self._load_cached_rows(conn, user_ids, live_only=False)
        return {
            str(_row_value(row, "twitch_user_id", "")).strip(): row
            for row in rows
            if str(_row_value(row, "twitch_user_id", "")).strip()
        }

    def _load_cached_rows(self, conn, user_ids: Sequence[str], *, live_only: bool) -> list[Any]:
        if not user_ids:
            return []
        where = [f"twitch_user_id IN ({_placeholders(user_ids)})"]
        params: list[object] = list(user_ids)
        if live_only:
            where.append("COALESCE(is_live, 0) = 1")
        sql = (
            "SELECT twitch_user_id, twitch_login, avg_duration_sec, time_pattern_score_base, "
            "received_successful_raids_total, is_new_partner_preferred, new_partner_multiplier, "
            "raid_boost_multiplier, is_live, current_started_at, current_uptime_sec, "
            "duration_score, time_pattern_score, readiness_score, fairness_score, "
            "base_score, final_score, internal_sent_raids_30d, "
            "internal_received_raids_30d, internal_received_raids_7d, "
            "today_received_raids, last_computed_at "
            "FROM twitch_partner_raid_scores "
            f"WHERE {' AND '.join(where)}"
        )
        return conn.execute(sql, params).fetchall()

    def _build_score(
        self,
        *,
        partner: _PartnerRow,
        live_state: Any,
        session_rows: Sequence[Any],
        raid_timestamps: Sequence[datetime],
        internal_raid_metrics: tuple[int, int, int],
        raid_boost_enabled: bool,
        existing_cache: Any,
        now_utc: datetime,
        lookback_cutoff: datetime,
    ) -> _PreparedScore:
        recent_started: list[datetime] = []
        recent_durations: list[int] = []
        for row in session_rows:
            started_at = _parse_dt(_row_value(row, "started_at"))
            if started_at is None or started_at < lookback_cutoff:
                continue
            recent_started.append(started_at)
            duration_seconds = _safe_int(_row_value(row, "duration_seconds"), 0)
            if duration_seconds > 0:
                recent_durations.append(duration_seconds)

        avg_duration_sec = (
            int(round(sum(recent_durations) / len(recent_durations))) if recent_durations else 0
        )
        duration_history_reliable = len(recent_durations) >= MIN_RELIABLE_SESSIONS and avg_duration_sec > 0

        now_bucket = now_utc.astimezone(BERLIN_TZ)
        if len(recent_started) >= MIN_RELIABLE_SESSIONS:
            matching = sum(
                1
                for started_at in recent_started
                if (
                    started_at.astimezone(BERLIN_TZ).weekday() == now_bucket.weekday()
                    and started_at.astimezone(BERLIN_TZ).hour == now_bucket.hour
                )
            )
            time_pattern_score_base = _round_score(matching / len(recent_started))
            time_pattern_reliable = True
        else:
            time_pattern_score_base = NEUTRAL_SCORE
            time_pattern_reliable = False

        raid_total = len(raid_timestamps)
        today_received_raids = sum(
            1
            for executed_at in raid_timestamps
            if executed_at.astimezone(BERLIN_TZ).date() == _today_in_berlin(now_utc)
        )
        sent_30d, received_30d, received_7d = internal_raid_metrics
        is_new_partner_preferred = raid_total < NEW_PARTNER_RAID_THRESHOLD
        new_partner_multiplier = _new_partner_multiplier(raid_total)
        raid_boost_multiplier = (
            RAID_BOOST_MULTIPLIER if bool(raid_boost_enabled) else DEFAULT_RAID_BOOST_MULTIPLIER
        )

        is_live = bool(_safe_int(_row_value(live_state, "is_live", 0)))
        started_at_live = _parse_dt(_row_value(live_state, "last_started_at"))

        if is_live and started_at_live is not None:
            current_started_at = _iso_utc(started_at_live)
            current_uptime_sec = max(0, int((now_utc - started_at_live).total_seconds()))
            if duration_history_reliable and avg_duration_sec > 0:
                duration_score = _round_score(
                    _clamp((avg_duration_sec - current_uptime_sec) / float(avg_duration_sec))
                )
            else:
                duration_score = NEUTRAL_SCORE
            time_pattern_score = time_pattern_score_base if time_pattern_reliable else NEUTRAL_SCORE
            readiness_score = _readiness_score(duration_score, time_pattern_score)
            fairness_score = _fairness_score(
                sent_30d=sent_30d,
                received_30d=received_30d,
                received_7d=received_7d,
                today_received_raids=today_received_raids,
            )
            base_score = _combine_preboost_score(readiness_score, fairness_score)
            final_score = _round_score(
                base_score * new_partner_multiplier * raid_boost_multiplier
            )
        elif existing_cache is not None:
            current_started_at = str(_row_value(existing_cache, "current_started_at") or "").strip() or None
            current_uptime_sec = _safe_int(_row_value(existing_cache, "current_uptime_sec"), 0)
            duration_score = _round_score(
                _safe_float(_row_value(existing_cache, "duration_score"), NEUTRAL_SCORE)
            )
            time_pattern_score = _round_score(
                _safe_float(_row_value(existing_cache, "time_pattern_score"), NEUTRAL_SCORE)
            )
            readiness_score = _round_score(
                _safe_float(_row_value(existing_cache, "readiness_score"), NEUTRAL_SCORE)
            )
            fairness_score = _round_score(
                _safe_float(_row_value(existing_cache, "fairness_score"), NEUTRAL_SCORE)
            )
            base_score = _round_score(_safe_float(_row_value(existing_cache, "base_score"), NEUTRAL_SCORE))
            final_score = _round_score(
                _safe_float(_row_value(existing_cache, "final_score"), base_score)
            )
        else:
            current_started_at = None
            current_uptime_sec = 0
            duration_score = NEUTRAL_SCORE
            time_pattern_score = time_pattern_score_base if time_pattern_reliable else NEUTRAL_SCORE
            readiness_score = _readiness_score(duration_score, time_pattern_score)
            fairness_score = _fairness_score(
                sent_30d=sent_30d,
                received_30d=received_30d,
                received_7d=received_7d,
                today_received_raids=today_received_raids,
            )
            base_score = _combine_preboost_score(readiness_score, fairness_score)
            final_score = _round_score(
                base_score * new_partner_multiplier * raid_boost_multiplier
            )

        return _PreparedScore(
            twitch_user_id=partner.twitch_user_id,
            twitch_login=partner.twitch_login,
            avg_duration_sec=avg_duration_sec,
            time_pattern_score_base=_round_score(time_pattern_score_base),
            received_successful_raids_total=raid_total,
            is_new_partner_preferred=is_new_partner_preferred,
            new_partner_multiplier=new_partner_multiplier,
            raid_boost_multiplier=_round_score(raid_boost_multiplier),
            is_live=is_live,
            current_started_at=current_started_at,
            current_uptime_sec=current_uptime_sec,
            duration_score=_round_score(duration_score),
            time_pattern_score=_round_score(time_pattern_score),
            readiness_score=_round_score(readiness_score),
            fairness_score=_round_score(fairness_score),
            base_score=_round_score(base_score),
            final_score=_round_score(final_score),
            internal_sent_raids_30d=int(sent_30d or 0),
            internal_received_raids_30d=int(received_30d or 0),
            internal_received_raids_7d=int(received_7d or 0),
            today_received_raids=today_received_raids,
            last_computed_at=_iso_utc(now_utc),
        )

    def _upsert_scores(self, conn, prepared_rows: Sequence[_PreparedScore]) -> None:
        if not prepared_rows:
            return
        _executemany(
            conn,
            """
            INSERT INTO twitch_partner_raid_scores (
                twitch_user_id,
                twitch_login,
                avg_duration_sec,
                time_pattern_score_base,
                received_successful_raids_total,
                is_new_partner_preferred,
                new_partner_multiplier,
                raid_boost_multiplier,
                is_live,
                current_started_at,
                current_uptime_sec,
                duration_score,
                time_pattern_score,
                readiness_score,
                fairness_score,
                base_score,
                final_score,
                internal_sent_raids_30d,
                internal_received_raids_30d,
                internal_received_raids_7d,
                today_received_raids,
                last_computed_at
            ) VALUES (%s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s)
            ON CONFLICT (twitch_user_id) DO UPDATE SET
                twitch_login = EXCLUDED.twitch_login,
                avg_duration_sec = EXCLUDED.avg_duration_sec,
                time_pattern_score_base = EXCLUDED.time_pattern_score_base,
                received_successful_raids_total = EXCLUDED.received_successful_raids_total,
                is_new_partner_preferred = EXCLUDED.is_new_partner_preferred,
                new_partner_multiplier = EXCLUDED.new_partner_multiplier,
                raid_boost_multiplier = EXCLUDED.raid_boost_multiplier,
                is_live = EXCLUDED.is_live,
                current_started_at = EXCLUDED.current_started_at,
                current_uptime_sec = EXCLUDED.current_uptime_sec,
                duration_score = EXCLUDED.duration_score,
                time_pattern_score = EXCLUDED.time_pattern_score,
                readiness_score = EXCLUDED.readiness_score,
                fairness_score = EXCLUDED.fairness_score,
                base_score = EXCLUDED.base_score,
                final_score = EXCLUDED.final_score,
                internal_sent_raids_30d = EXCLUDED.internal_sent_raids_30d,
                internal_received_raids_30d = EXCLUDED.internal_received_raids_30d,
                internal_received_raids_7d = EXCLUDED.internal_received_raids_7d,
                today_received_raids = EXCLUDED.today_received_raids,
                last_computed_at = EXCLUDED.last_computed_at
            """,
            [row.as_db_tuple() for row in prepared_rows],
        )


partner_raid_score_service = PartnerRaidScoreService()


def refresh_partner_raid_score(
    twitch_user_id: str,
    *,
    now: datetime | None = None,
) -> dict[str, object] | None:
    return partner_raid_score_service.refresh_partner_score(twitch_user_id, now=now)


def refresh_all_partner_raid_scores(
    *,
    now: datetime | None = None,
) -> dict[str, dict[str, object]]:
    return partner_raid_score_service.refresh_all_partner_scores(now=now)


def load_partner_raid_scores(
    twitch_user_ids: Iterable[str],
    *,
    live_only: bool = False,
) -> dict[str, dict[str, object]]:
    return partner_raid_score_service.load_scores(twitch_user_ids, live_only=live_only)


def load_partner_raid_score_map(
    twitch_user_ids: Iterable[str],
    *,
    live_only: bool = False,
) -> dict[str, dict[str, object]]:
    return load_partner_raid_scores(twitch_user_ids, live_only=live_only)


async def refresh_partner_raid_score_async(
    twitch_user_id: str,
    *,
    now: datetime | None = None,
) -> dict[str, object] | None:
    return await asyncio.to_thread(refresh_partner_raid_score, twitch_user_id, now=now)


async def refresh_all_partner_raid_scores_async(
    *,
    now: datetime | None = None,
) -> dict[str, dict[str, object]]:
    return await asyncio.to_thread(refresh_all_partner_raid_scores, now=now)


__all__ = [
    "PartnerRaidScoreService",
    "BERLIN_TZ",
    "load_partner_raid_score_map",
    "load_partner_raid_scores",
    "partner_raid_score_service",
    "refresh_all_partner_raid_scores",
    "refresh_all_partner_raid_scores_async",
    "refresh_partner_raid_score",
    "refresh_partner_raid_score_async",
]
