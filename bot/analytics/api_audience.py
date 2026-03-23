"""
Analytics API v2 - Audience Mixin.

Viewer/audience analytics: watch time, follower funnel, viewer overlap,
audience insights, audience demographics.
"""

from __future__ import annotations

import contextlib
import json
import logging
import math
from datetime import UTC, datetime, timedelta
from typing import Any
from zoneinfo import ZoneInfo, ZoneInfoNotFoundError

from aiohttp import web

from ..core.chat_bots import build_known_chat_bot_not_in_clause
from ..storage import pg as storage
from .error_utils import analytics_internal_error_response
from .engagement_metrics import EngagementInputs, calculate_engagement

log = logging.getLogger("TwitchStreams.AnalyticsV2")


def _executemany(conn, sql: str, params_seq) -> None:
    executemany = getattr(conn, "executemany", None)
    if callable(executemany):
        executemany(sql, params_seq)
        return
    with contextlib.closing(conn.cursor()) as cur:
        cur.executemany(sql, params_seq)


class _AnalyticsAudienceMixin:
    """Mixin providing audience analytics endpoints."""

    WATCH_TIME_MIN_SAMPLES = 25
    WATCH_TIME_MIN_COVERAGE = 0.15
    PEAK_SESSION_WINDOW = 30
    PEAK_HALF_LIFE_SESSIONS = 8.0

    @staticmethod
    def _sanitize_log_value(value: Any) -> str:
        text = "" if value is None else str(value)
        return text.replace("\r", "\\r").replace("\n", "\\n")

    @staticmethod
    def _resolve_target_timezone(timezone_name: str | None) -> tuple[Any, str]:
        tz_name = (timezone_name or "UTC").strip()
        if not tz_name:
            return UTC, "UTC"
        if tz_name.upper() == "UTC":
            return UTC, "UTC"
        try:
            return ZoneInfo(tz_name), tz_name
        except ZoneInfoNotFoundError:
            log.debug(
                "Unknown timezone '%s' in audience analytics; fallback to UTC",
                _AnalyticsAudienceMixin._sanitize_log_value(tz_name),
            )
            return UTC, "UTC"

    @staticmethod
    def _coerce_timestamp(value: Any) -> datetime | None:
        if value is None:
            return None
        parsed: datetime | None = None
        if isinstance(value, datetime):
            parsed = value
        elif isinstance(value, (int, float)):
            try:
                parsed = datetime.fromtimestamp(float(value), tz=UTC)
            except (TypeError, ValueError, OSError):
                parsed = None
        elif isinstance(value, str):
            txt = value.strip()
            if not txt:
                return None
            normalized = f"{txt[:-1]}+00:00" if txt.endswith("Z") else txt
            try:
                parsed = datetime.fromisoformat(normalized)
            except ValueError:
                for fmt in (
                    "%Y-%m-%d %H:%M:%S.%f",
                    "%Y-%m-%d %H:%M:%S",
                    "%Y-%m-%dT%H:%M:%S.%f",
                    "%Y-%m-%dT%H:%M:%S",
                ):
                    try:
                        parsed = datetime.strptime(txt, fmt)
                        break
                    except ValueError:
                        continue
        if parsed is None:
            return None
        if parsed.tzinfo is None:
            parsed = parsed.replace(tzinfo=UTC)
        return parsed

    @staticmethod
    def _quantile(values: list[float], q: float) -> float:
        if not values:
            return 0.0
        ordered = sorted(values)
        if len(ordered) == 1:
            return float(ordered[0])
        pos = max(0.0, min(1.0, q)) * (len(ordered) - 1)
        lower = int(math.floor(pos))
        upper = int(math.ceil(pos))
        if lower == upper:
            return float(ordered[lower])
        fraction = pos - lower
        return float(ordered[lower] + (ordered[upper] - ordered[lower]) * fraction)

    def _compute_weighted_peak_hours(
        self,
        conn,
        streamer_login: str,
        since_date: str,
        target_tz: Any,
    ) -> tuple[list[int], dict[str, Any]]:
        session_rows = conn.execute(
            """
            SELECT s.id, s.started_at
            FROM twitch_stream_sessions s
            WHERE s.started_at >= %s
              AND LOWER(s.streamer_login) = %s
              AND s.ended_at IS NOT NULL
            ORDER BY s.started_at DESC
            LIMIT %s
            """,
            [since_date, streamer_login, self.PEAK_SESSION_WINDOW],
        ).fetchall()
        session_ids = [int(r[0]) for r in session_rows if r and r[0] is not None]
        if not session_ids:
            return [], {
                "sessionCount": 0,
                "sessionsWithActivity": 0,
                "sampleCount": 0,
                "coverage": 0.0,
            }

        # Exponential recency weighting: half-life in sessions.
        session_weights = {
            sid: math.pow(0.5, idx / self.PEAK_HALF_LIFE_SESSIONS)
            for idx, sid in enumerate(session_ids)
        }
        per_session_hours: dict[int, dict[int, int]] = {sid: {} for sid in session_ids}
        total_samples = 0

        session_ids_json = json.dumps(session_ids)
        msg_bot_clause, msg_bot_params = build_known_chat_bot_not_in_clause(
            column_expr="cm.chatter_login",
            placeholder="%s",
        )
        rows = conn.execute(
            f"""
            SELECT cm.session_id, cm.message_ts
            FROM twitch_chat_messages cm
            WHERE cm.session_id IN (
                SELECT CAST(value AS BIGINT)
                FROM json_array_elements_text(%s) AS t(value)
            )
              AND {msg_bot_clause}
            """,
            (session_ids_json, *msg_bot_params),
        ).fetchall()
        for row in rows:
            sid = int(row[0]) if row and row[0] is not None else None
            if sid is None or sid not in per_session_hours:
                continue
            parsed = self._coerce_timestamp(row[1])
            if parsed is None:
                continue
            hour = parsed.astimezone(target_tz).hour
            per_session_hours[sid][hour] = per_session_hours[sid].get(hour, 0) + 1
            total_samples += 1

        sessions_with_activity = sum(1 for sid in session_ids if per_session_hours[sid])
        coverage = sessions_with_activity / max(1, len(session_ids))

        # Light outlier protection: winsorize per-hour session counts at p90.
        hour_caps: dict[int, float] = {}
        for hour in range(24):
            hour_values = [float(per_session_hours[sid].get(hour, 0)) for sid in session_ids]
            hour_caps[hour] = max(1.0, self._quantile(hour_values, 0.90))

        weighted_scores = {hour: 0.0 for hour in range(24)}
        for sid in session_ids:
            session_weight = session_weights[sid]
            for hour in range(24):
                raw_count = float(per_session_hours[sid].get(hour, 0))
                capped_count = min(raw_count, hour_caps[hour])
                weighted_scores[hour] += session_weight * capped_count

        ordered_hours = sorted(
            weighted_scores.items(),
            key=lambda item: (-item[1], item[0]),
        )
        peak_hours = [hour for hour, score in ordered_hours if score > 0][:3]
        return peak_hours, {
            "sessionCount": len(session_ids),
            "sessionsWithActivity": sessions_with_activity,
            "sampleCount": total_samples,
            "coverage": round(coverage, 3),
        }

    def _backfill_last_seen_from_messages(self, conn, session_ids: list[int]) -> int:
        """Backfill missing/older last_seen_at using max message_ts per chatter+session."""
        normalized_ids = sorted({int(sid) for sid in session_ids if sid is not None})
        if not normalized_ids:
            return 0

        session_ids_json = json.dumps(normalized_ids)
        msg_bot_clause, msg_bot_params = build_known_chat_bot_not_in_clause(
            column_expr="cm.chatter_login",
            placeholder="%s",
        )
        rows = conn.execute(
            f"""
            SELECT
                cm.session_id,
                LOWER(NULLIF(cm.chatter_login, '')) AS chatter_login,
                cm.chatter_id,
                MAX(cm.message_ts) AS max_message_ts
            FROM twitch_chat_messages cm
            WHERE cm.session_id IN (
                SELECT CAST(value AS BIGINT)
                FROM json_array_elements_text(%s) AS t(value)
            )
              AND {msg_bot_clause}
            GROUP BY cm.session_id, LOWER(NULLIF(cm.chatter_login, '')), cm.chatter_id
            """,
            (session_ids_json, *msg_bot_params),
        ).fetchall()

        updates_by_login = []
        updates_by_id = []
        for row in rows:
            session_id = int(row[0]) if row and row[0] is not None else None
            chatter_login = (row[1] or "").strip().lower()
            chatter_id = row[2]
            max_seen = row[3]
            if session_id is None or max_seen is None:
                continue
            if chatter_login:
                updates_by_login.append((max_seen, session_id, chatter_login, max_seen))
            elif chatter_id:
                updates_by_id.append((max_seen, session_id, chatter_id, max_seen))

        if updates_by_login:
            _executemany(
                conn,
                """
                UPDATE twitch_session_chatters
                   SET last_seen_at = %s
                 WHERE session_id = %s
                   AND LOWER(chatter_login) = %s
                   AND (last_seen_at IS NULL OR last_seen_at < %s)
                """,
                updates_by_login,
            )
        if updates_by_id:
            _executemany(
                conn,
                """
                UPDATE twitch_session_chatters
                   SET last_seen_at = %s
                 WHERE session_id = %s
                   AND chatter_id = %s
                   AND (chatter_login IS NULL OR chatter_login = '')
                   AND (last_seen_at IS NULL OR last_seen_at < %s)
                """,
                updates_by_id,
            )
        return len(updates_by_login) + len(updates_by_id)

    def _calc_watch_distribution(
        self, sessions, conn=None, session_ids: list = None
    ) -> dict[str, Any]:
        """Calculate watch time distribution.

        Uses real per-viewer watch-time from chatters snapshots
        (last_seen_at - first_message_at). No hidden heuristic fallback.
        """
        base_payload = {
            "under5min": 0,
            "min5to15": 0,
            "min15to30": 0,
            "min30to60": 0,
            "over60min": 0,
            "avgWatchTime": 0,
            "medianWatchTime": 0,
        }
        if not sessions:
            return {
                **base_payload,
                "sessionCount": 0,
                "dataQuality": {
                    "method": "no_data",
                    "coverage": 0.0,
                    "sample_count": 0,
                    "viewer_base_count": 0,
                    "required_min_samples": self.WATCH_TIME_MIN_SAMPLES,
                    "required_min_coverage": self.WATCH_TIME_MIN_COVERAGE,
                },
            }

        total_sessions = len(sessions)

        real_minutes: list[float] = []
        viewer_base_count = 0
        coverage_real = 0.0
        sample_count = 0
        if conn is not None and session_ids:
            if session_ids:
                session_ids_json = json.dumps([int(sid) for sid in session_ids])
                session_bot_clause, session_bot_params = build_known_chat_bot_not_in_clause(
                    column_expr="chatter_login",
                    placeholder="%s",
                )
                base_row = conn.execute(
                    f"""
                    SELECT COUNT(
                        DISTINCT COALESCE(NULLIF(chatter_login, ''), chatter_id)
                    ) AS viewer_base_count
                    FROM twitch_session_chatters
                    WHERE session_id IN (
                        SELECT CAST(value AS BIGINT)
                        FROM json_array_elements_text(%s) AS t(value)
                    )
                      AND COALESCE(NULLIF(chatter_login, ''), chatter_id) IS NOT NULL
                      AND {session_bot_clause}
                    """,
                    (session_ids_json, *session_bot_params),
                ).fetchone()
                viewer_base_count = int(base_row[0] or 0) if base_row else 0

                rows = conn.execute(
                    f"""
                    SELECT
                        ROUND(
                            GREATEST(
                                EXTRACT(EPOCH FROM COALESCE(last_seen_at, first_message_at))
                                - EXTRACT(EPOCH FROM COALESCE(first_message_at, last_seen_at)),
                                0
                            ) / 60.0
                        ) as watch_minutes
                    FROM twitch_session_chatters
                    WHERE session_id IN (
                        SELECT CAST(value AS BIGINT)
                        FROM json_array_elements_text(%s) AS t(value)
                    )
                      AND first_message_at IS NOT NULL
                      AND last_seen_at IS NOT NULL
                      AND {session_bot_clause}
                    """,
                    (session_ids_json, *session_bot_params),
                ).fetchall()
                real_minutes = [float(r[0]) for r in rows if r[0] is not None and r[0] >= 0]
                sample_count = len(real_minutes)
                coverage_real = (
                    sample_count / max(1, viewer_base_count)
                    if viewer_base_count > 0
                    else sample_count / max(1, total_sessions)
                )

        if sample_count <= 0:
            method = "no_data"
        elif (
            sample_count < self.WATCH_TIME_MIN_SAMPLES
            or coverage_real < self.WATCH_TIME_MIN_COVERAGE
        ):
            method = "low_coverage"
        else:
            method = "real_samples"

        data_quality = {
            "method": method,
            "coverage": round(min(1.0, max(0.0, coverage_real)), 3),
            "sample_count": sample_count,
            "viewer_base_count": viewer_base_count,
            "required_min_samples": self.WATCH_TIME_MIN_SAMPLES,
            "required_min_coverage": self.WATCH_TIME_MIN_COVERAGE,
        }

        if method != "real_samples":
            return {
                **base_payload,
                "sessionCount": total_sessions,
                "dataQuality": data_quality,
            }

        total_viewers = len(real_minutes)
        under_5min = sum(1 for m in real_minutes if m < 5) / total_viewers * 100
        min_5_to_15 = sum(1 for m in real_minutes if 5 <= m < 15) / total_viewers * 100
        min_15_to_30 = sum(1 for m in real_minutes if 15 <= m < 30) / total_viewers * 100
        min_30_to_60 = sum(1 for m in real_minutes if 30 <= m < 60) / total_viewers * 100
        over_60min = sum(1 for m in real_minutes if m >= 60) / total_viewers * 100
        avg_watch_time = sum(real_minutes) / total_viewers
        sorted_m = sorted(real_minutes)
        mid = len(sorted_m) // 2
        median_watch_time = (
            sorted_m[mid] if len(sorted_m) % 2 == 1 else (sorted_m[mid - 1] + sorted_m[mid]) / 2
        )

        return {
            "under5min": round(max(0, under_5min), 1),
            "min5to15": round(max(0, min_5_to_15), 1),
            "min15to30": round(max(0, min_15_to_30), 1),
            "min30to60": round(max(0, min_30_to_60), 1),
            "over60min": round(max(0, over_60min), 1),
            "avgWatchTime": round(avg_watch_time, 1),
            "medianWatchTime": round(median_watch_time, 1),
            "sessionCount": total_sessions,
            "dataQuality": data_quality,
        }

    async def _api_v2_watch_time_distribution(self, request: web.Request) -> web.Response:
        """Get watch time distribution with previous period comparison."""
        self._require_v2_auth(request)

        streamer = request.query.get("streamer", "").strip() or None
        days = min(max(int(request.query.get("days", "30")), 7), 365)

        if not streamer:
            return web.json_response({"error": "Streamer required"}, status=400)

        try:
            with storage.transaction() as conn:
                since_date = (datetime.now(UTC) - timedelta(days=days)).isoformat()
                prev_since_date = (datetime.now(UTC) - timedelta(days=days * 2)).isoformat()

                # Current period
                current_sessions = conn.execute(
                    """
                    SELECT s.id, s.retention_5m, s.retention_10m,
                           s.retention_20m, s.avg_viewers, s.start_viewers, s.end_viewers,
                           s.duration_seconds
                    FROM twitch_stream_sessions s
                    WHERE s.started_at >= %s AND LOWER(s.streamer_login) = %s AND s.ended_at IS NOT NULL
                """,
                    [since_date, streamer.lower()],
                ).fetchall()
                # Remap: (duration_seconds, ret5, ret10, ret20, avg_viewers, ...) for _calc
                current_sessions_remapped = [
                    (r[7], r[1], r[2], r[3], r[4]) for r in current_sessions
                ]

                # Previous period
                prev_sessions = conn.execute(
                    """
                    SELECT s.id, s.retention_5m, s.retention_10m,
                           s.retention_20m, s.avg_viewers, s.start_viewers, s.end_viewers,
                           s.duration_seconds
                    FROM twitch_stream_sessions s
                    WHERE s.started_at >= %s AND s.started_at < %s AND LOWER(s.streamer_login) = %s AND s.ended_at IS NOT NULL
                """,
                    [prev_since_date, since_date, streamer.lower()],
                ).fetchall()
                prev_sessions_remapped = [(r[7], r[1], r[2], r[3], r[4]) for r in prev_sessions]

                current_ids = [r[0] for r in current_sessions]
                prev_ids = [r[0] for r in prev_sessions]
                self._backfill_last_seen_from_messages(conn, current_ids + prev_ids)
                current = self._calc_watch_distribution(
                    current_sessions_remapped, conn=conn, session_ids=current_ids
                )
                previous = self._calc_watch_distribution(
                    prev_sessions_remapped, conn=conn, session_ids=prev_ids
                )

                # Calculate deltas
                deltas = {}
                for key in [
                    "under5min",
                    "min5to15",
                    "min15to30",
                    "min30to60",
                    "over60min",
                    "avgWatchTime",
                ]:
                    curr_val = current.get(key, 0)
                    prev_val = previous.get(key, 0)
                    if prev_val > 0:
                        deltas[key] = round(((curr_val - prev_val) / prev_val) * 100, 1)
                    else:
                        deltas[key] = None

                session_count = int(current.get("sessionCount", 0) or 0)
                dq = current.get("dataQuality", {})
                method = str(dq.get("method") or "no_data")
                sample_count = int(dq.get("sample_count", 0) or 0)
                coverage = float(dq.get("coverage", 0.0) or 0.0)

                if method == "real_samples":
                    if sample_count >= 200 and coverage >= 0.35:
                        confidence = "high"
                    elif sample_count >= 80 and coverage >= 0.20:
                        confidence = "medium"
                    else:
                        confidence = "low"
                elif method == "low_coverage":
                    confidence = "low"
                    deltas = {key: None for key in deltas}
                else:
                    confidence = "very_low"
                    deltas = {key: None for key in deltas}

                return web.json_response(
                    {
                        **current,
                        "previous": previous,
                        "deltas": deltas,
                        "dataQuality": {
                            "confidence": confidence,
                            "sessions": session_count,
                            "method": method,
                            "coverage": round(coverage, 3),
                            "sample_count": sample_count,
                            "viewer_base_count": int(dq.get("viewer_base_count", 0) or 0),
                            "required_min_samples": int(
                                dq.get("required_min_samples", self.WATCH_TIME_MIN_SAMPLES)
                                or self.WATCH_TIME_MIN_SAMPLES
                            ),
                            "required_min_coverage": float(
                                dq.get("required_min_coverage", self.WATCH_TIME_MIN_COVERAGE)
                                or self.WATCH_TIME_MIN_COVERAGE
                            ),
                            "botFilterApplied": True,
                        },
                    }
                )
        except Exception as exc:
            log.exception("Error in watch time distribution API")
            return analytics_internal_error_response()

    async def _api_v2_follower_funnel(self, request: web.Request) -> web.Response:
        """Get follower conversion funnel data."""
        self._require_v2_auth(request)
        self._require_extended_plan(request)

        streamer = request.query.get("streamer", "").strip() or None
        days = min(max(int(request.query.get("days", "30")), 7), 365)

        if not streamer:
            return web.json_response({"error": "Streamer required"}, status=400)

        try:
            with storage.transaction() as conn:
                since_date = (datetime.now(UTC) - timedelta(days=days)).isoformat()
                streamer_login = streamer.lower()
                session_bot_clause, session_bot_params = build_known_chat_bot_not_in_clause(
                    column_expr="sc.chatter_login",
                    placeholder="%s",
                )

                # Base session and follower stats.
                stats = conn.execute(
                    """
                    SELECT
                        COUNT(*) as session_count,
                        COALESCE(SUM(s.duration_seconds), 0) as total_duration,
                        AVG(s.avg_viewers) as avg_viewers,
                        COALESCE(SUM(s.avg_viewers * s.duration_seconds / 3600.0), 0) as total_hours_watched,
                        AVG(s.retention_10m) as avg_retention_10m,
                        SUM(CASE WHEN s.follower_delta IS NOT NULL
                             AND NOT (s.followers_end = 0 AND s.followers_start > 0)
                             THEN s.follower_delta ELSE 0 END) as net_followers,
                        SUM(CASE WHEN s.follower_delta > 0
                             AND NOT (s.followers_end = 0 AND s.followers_start > 0)
                             THEN s.follower_delta ELSE 0 END) as gained_followers,
                        COUNT(CASE WHEN s.follower_delta IS NOT NULL
                             AND NOT (s.followers_end = 0 AND s.followers_start > 0)
                             THEN 1 END) as follower_valid_samples
                    FROM twitch_stream_sessions s
                    WHERE s.started_at >= %s
                      AND LOWER(s.streamer_login) = %s
                      AND s.ended_at IS NOT NULL
                    """,
                    [since_date, streamer_login],
                ).fetchone()

                session_count = int(stats[0]) if stats and stats[0] else 0
                if session_count == 0:
                    return web.json_response(
                        {
                            "uniqueViewers": 0,
                            "returningViewers": 0,
                            "newFollowers": 0,
                            "followsDuringStream": 0,
                            "netFollowerDelta": 0,
                            "conversionRate": 0,
                            "conversionDataSource": "session_delta_fallback",
                            "avgTimeToFollow": 0,
                            "followersBySource": {
                                "organic": 0,
                                "raids": 0,
                                "hosts": 0,
                                "other": 0,
                            },
                            "dataQuality": {
                                "confidence": "low",
                                "reason": "no_sessions",
                            },
                        }
                    )

                total_duration = float(stats[1]) if stats[1] else 0.0
                avg_viewers = float(stats[2]) if stats[2] else 0.0
                total_hours_watched = float(stats[3]) if stats[3] else 0.0
                avg_retention_10m = float(stats[4]) if stats[4] else 0.0  # 0..1
                net_followers = int(stats[5]) if stats[5] else 0
                gained_followers = int(stats[6]) if stats[6] else 0
                follower_valid_samples = int(stats[7]) if stats[7] else 0

                # Distinct chatter cohorts in selected period (less inflation than SUM per session).
                chatter_stats = conn.execute(
                    f"""
                    SELECT
                        COUNT(DISTINCT COALESCE(NULLIF(sc.chatter_login, ''), sc.chatter_id)) as unique_chatters,
                        COUNT(
                            DISTINCT CASE
                                WHEN LOWER(COALESCE(CAST(sc.is_first_time_streamer AS TEXT), '0')) NOT IN ('1', 't', 'true')
                                THEN COALESCE(NULLIF(sc.chatter_login, ''), sc.chatter_id)
                            END
                        ) as returning_chatters,
                        COUNT(DISTINCT COALESCE(NULLIF(sc.chatter_login, ''), sc.chatter_id)) as tracked_viewers
                    FROM twitch_session_chatters sc
                    JOIN twitch_stream_sessions s ON s.id = sc.session_id
                    WHERE s.started_at >= %s
                      AND LOWER(s.streamer_login) = %s
                      AND s.ended_at IS NOT NULL
                      AND {session_bot_clause}
                    """,
                    [since_date, streamer_login, *session_bot_params],
                ).fetchone()
                unique_chatters = int(chatter_stats[0]) if chatter_stats and chatter_stats[0] else 0
                returning_chatters = (
                    int(chatter_stats[1]) if chatter_stats and chatter_stats[1] else 0
                )
                total_viewers_tracked = (
                    int(chatter_stats[2]) if chatter_stats and chatter_stats[2] else 0
                )

                # Real follow events during streams (primary source)
                follow_events_row = conn.execute(
                    """
                    SELECT COUNT(DISTINCT fe.id) as follows_during_stream
                    FROM twitch_follow_events fe
                    JOIN twitch_stream_sessions ss
                        ON ss.streamer_login = fe.streamer_login
                       AND fe.followed_at BETWEEN ss.started_at
                           AND COALESCE(ss.ended_at, NOW())
                    WHERE LOWER(ss.streamer_login) = %s
                      AND ss.started_at >= %s
                      AND ss.ended_at IS NOT NULL
                    """,
                    [streamer_login, since_date],
                ).fetchone()
                follows_during_stream = (
                    int(follow_events_row[0]) if follow_events_row and follow_events_row[0] else 0
                )

                # Primäre und einzige Quelle: distinct Session-Chatters (inkl. Chatters-API Lurker).
                # Keine Schätzung, wenn keine Samples vorhanden.
                unique_viewers = total_viewers_tracked or unique_chatters
                unique_viewers_method = (
                    "distinct_session_chatters" if unique_viewers > 0 else "no_viewer_data"
                )

                # Follow conversion: prefer real events, fallback to session delta
                if follows_during_stream > 0:
                    conversion_source = "follow_events"
                    gained_followers_for_conversion = follows_during_stream
                else:
                    conversion_source = "session_delta_fallback"
                    gained_followers_for_conversion = gained_followers

                conversion_rate = (
                    (gained_followers_for_conversion / unique_viewers * 100.0)
                    if unique_viewers > 0
                    else 0.0
                )

                # Estimated time-to-follow: scaled to session length, bounded to practical range.
                avg_session_mins = (
                    (total_duration / max(1, session_count) / 60.0) if total_duration > 0 else 0.0
                )
                avg_time_to_follow = max(5.0, min(45.0, avg_session_mins * 0.4))

                # Raid inflow proxy for source split.
                raids_received = conn.execute(
                    """
                    SELECT COUNT(*), COALESCE(SUM(viewer_count), 0)
                    FROM twitch_raid_history
                    WHERE LOWER(to_broadcaster_login) = %s
                      AND executed_at >= %s
                      AND COALESCE(success, FALSE) IS TRUE
                    """,
                    [streamer_login, since_date],
                ).fetchone()
                raid_count = int(raids_received[0]) if raids_received and raids_received[0] else 0
                raid_viewers = int(raids_received[1]) if raids_received and raids_received[1] else 0

                # Conservative conversion assumption for raid-origin follows.
                raid_followers = min(int(raid_viewers * 0.05), gained_followers_for_conversion)
                organic_followers = max(0, gained_followers_for_conversion - raid_followers)

                if unique_viewers == 0:
                    confidence = "low"
                elif follower_valid_samples >= max(3, int(session_count * 0.6)):
                    confidence = "high"
                elif follower_valid_samples >= 1:
                    confidence = "medium"
                else:
                    confidence = "low"

                return web.json_response(
                    {
                        "uniqueViewers": unique_viewers,
                        "returningViewers": returning_chatters,
                        "newFollowers": gained_followers_for_conversion,
                        "followsDuringStream": follows_during_stream,
                        "netFollowerDelta": net_followers,
                        "conversionRate": round(conversion_rate, 2),
                        "conversionDataSource": conversion_source,
                        "avgTimeToFollow": round(avg_time_to_follow, 0),
                        "followersBySource": {
                            "organic": organic_followers,
                            "raids": raid_followers,
                            "hosts": 0,  # Host-specific attribution is not tracked.
                            "other": 0,
                        },
                        "dataQuality": {
                            "confidence": confidence,
                            "sessions": session_count,
                            "followerValidSamples": follower_valid_samples,
                            "raidEvents": raid_count,
                            "uniqueViewersMethod": unique_viewers_method,
                            "botFilterApplied": True,
                        },
                    }
                )
        except Exception as exc:
            log.exception("Error in follower funnel API")
            return analytics_internal_error_response()

    async def _api_v2_viewer_overlap(self, request: web.Request) -> web.Response:
        """Get viewer overlap with other channels."""
        self._require_v2_auth(request)

        streamer = request.query.get("streamer", "").strip()
        limit = min(max(int(request.query.get("limit", "20")), 5), 50)

        if not streamer:
            return web.json_response({"error": "Streamer required"}, status=400)

        try:
            with storage.transaction() as conn:
                base = streamer.lower()
                rollup_bot_clause_c1, rollup_bot_params_c1 = build_known_chat_bot_not_in_clause(
                    column_expr="c1.chatter_login",
                    placeholder="%s",
                )
                rollup_bot_clause_c2, rollup_bot_params_c2 = build_known_chat_bot_not_in_clause(
                    column_expr="c2.chatter_login",
                    placeholder="%s",
                )
                rollup_bot_clause, rollup_bot_params = build_known_chat_bot_not_in_clause(
                    column_expr="chatter_login",
                    placeholder="%s",
                )
                rows = conn.execute(
                    f"""
                    SELECT
                        c2.streamer_login as other_streamer,
                        COUNT(DISTINCT c1.chatter_login) as shared_chatters
                    FROM twitch_chatter_rollup c1
                    JOIN twitch_chatter_rollup c2 ON c1.chatter_login = c2.chatter_login
                    WHERE LOWER(c1.streamer_login) = %s
                      AND LOWER(c2.streamer_login) != %s
                      AND {rollup_bot_clause_c1}
                      AND {rollup_bot_clause_c2}
                    GROUP BY c2.streamer_login
                    ORDER BY shared_chatters DESC
                    LIMIT %s
                """,
                    [
                        base,
                        base,
                        *rollup_bot_params_c1,
                        *rollup_bot_params_c2,
                        limit,
                    ],
                ).fetchall()

                # Totals for A and B
                total_a = (
                    conn.execute(
                        f"""
                        SELECT COUNT(DISTINCT chatter_login)
                        FROM twitch_chatter_rollup
                        WHERE LOWER(streamer_login) = %s
                          AND {rollup_bot_clause}
                    """,
                        [base, *rollup_bot_params],
                    ).fetchone()[0]
                    or 1
                )

                totals_b = {
                    r[0].lower(): (
                        conn.execute(
                            f"""
                        SELECT COUNT(DISTINCT chatter_login)
                        FROM twitch_chatter_rollup
                        WHERE LOWER(streamer_login) = %s
                          AND {rollup_bot_clause}
                        """,
                            [r[0].lower(), *rollup_bot_params],
                        ).fetchone()[0]
                        or 1
                    )
                    for r in rows
                }

                data = []
                for r in rows:
                    other = r[0]
                    shared = r[1]
                    total_b = totals_b.get(other.lower(), 1)
                    jaccard = shared / max(1, (total_a + total_b - shared))
                    overlap_pct = round(jaccard * 100, 1)
                    data.append(
                        {
                            "streamerA": streamer,
                            "streamerB": other,
                            "sharedChatters": shared,
                            "totalChattersA": total_a,
                            "totalChattersB": total_b,
                            "overlapAtoB": round(shared / total_a * 100, 1),
                            "overlapBtoA": round(shared / total_b * 100, 1),
                            "jaccard": overlap_pct,
                            # Backwards compatible field expected by the React dashboard
                            # (used for bar width + display). Uses the symmetric Jaccard percentage.
                            "overlapPercentage": overlap_pct,
                        }
                    )

                return web.json_response(data)
        except Exception as exc:
            log.exception("Error in viewer overlap API")
            return analytics_internal_error_response()

    async def _api_v2_audience_insights(self, request: web.Request) -> web.Response:
        """Get combined audience insights (all in one call)."""
        self._require_v2_auth(request)
        self._require_extended_plan(request)

        streamer = request.query.get("streamer", "").strip() or None
        days = min(max(int(request.query.get("days", "30")), 7), 365)

        if not streamer:
            return web.json_response({"error": "Streamer required"}, status=400)

        try:
            # Call internal methods directly
            with storage.transaction() as conn:
                since_date = (datetime.now(UTC) - timedelta(days=days)).isoformat()
                prev_since = (datetime.now(UTC) - timedelta(days=days * 2)).isoformat()
                session_bot_clause, session_bot_params = build_known_chat_bot_not_in_clause(
                    column_expr="sc.chatter_login",
                    placeholder="%s",
                )
                rollup_bot_clause, rollup_bot_params = build_known_chat_bot_not_in_clause(
                    column_expr="cr.chatter_login",
                    placeholder="%s",
                )

                # Watch-time trend should use the same real watch-time distribution logic
                # as the dedicated endpoint, not retention as a proxy.
                current_sessions = conn.execute(
                    """
                    SELECT s.id, s.retention_5m, s.retention_10m,
                           s.retention_20m, s.avg_viewers, s.start_viewers, s.end_viewers,
                           s.duration_seconds
                    FROM twitch_stream_sessions s
                    WHERE s.started_at >= %s AND LOWER(s.streamer_login) = %s AND s.ended_at IS NOT NULL
                """,
                    [since_date, streamer.lower()],
                ).fetchall()
                prev_sessions = conn.execute(
                    """
                    SELECT s.id, s.retention_5m, s.retention_10m,
                           s.retention_20m, s.avg_viewers, s.start_viewers, s.end_viewers,
                           s.duration_seconds
                    FROM twitch_stream_sessions s
                    WHERE s.started_at >= %s AND s.started_at < %s AND LOWER(s.streamer_login) = %s AND s.ended_at IS NOT NULL
                """,
                    [prev_since, since_date, streamer.lower()],
                ).fetchall()
                current_sessions_remapped = [
                    (r[7], r[1], r[2], r[3], r[4]) for r in current_sessions
                ]
                prev_sessions_remapped = [(r[7], r[1], r[2], r[3], r[4]) for r in prev_sessions]
                current_ids = [r[0] for r in current_sessions]
                prev_ids = [r[0] for r in prev_sessions]
                self._backfill_last_seen_from_messages(conn, current_ids + prev_ids)
                current_watch = self._calc_watch_distribution(
                    current_sessions_remapped, conn=conn, session_ids=current_ids
                )
                previous_watch = self._calc_watch_distribution(
                    prev_sessions_remapped, conn=conn, session_ids=prev_ids
                )

                # Retention trend from session aggregates
                retention_row = conn.execute(
                    """
                    SELECT
                        AVG(s.retention_10m) as curr_ret,
                        (SELECT AVG(s2.retention_10m)
                         FROM twitch_stream_sessions s2
                         WHERE s2.started_at >= %s AND s2.started_at < %s
                           AND LOWER(s2.streamer_login) = %s AND s2.ended_at IS NOT NULL
                        ) as prev_ret
                    FROM twitch_stream_sessions s
                    WHERE s.started_at >= %s AND LOWER(s.streamer_login) = %s AND s.ended_at IS NOT NULL
                    """,
                    [prev_since, since_date, streamer.lower(), since_date, streamer.lower()],
                ).fetchone()

                curr_retention = float(retention_row[0]) * 100 if retention_row and retention_row[0] else 0
                prev_retention = float(retention_row[1]) * 100 if retention_row and retention_row[1] else 0

                # True return rate: distinct viewers this period who were known BEFORE the period.
                # Uses twitch_chatter_rollup.first_seen_at as the "seen before" signal.
                # Semantics: "% of this period's distinct chatters who had watched before"
                def _true_return_rate(period_start: str, period_end: str | None) -> tuple[float, int]:
                    """Return (return_rate_pct, total_distinct_viewers) for a date window.

                    Counts DISTINCT viewers in the window, then checks twitch_chatter_rollup
                    to see which were known before the window started.
                    """
                    end_filter = "AND s.started_at < %s" if period_end else ""
                    # Params order: period_start [period_end] streamer  streamer period_start
                    q_params = (
                        [period_start] + ([period_end] if period_end else []) + [streamer.lower()]
                        + [*session_bot_params]
                        + [streamer.lower(), *rollup_bot_params, period_start]
                    )

                    row = conn.execute(
                        f"""
                        WITH period_viewers AS (
                            SELECT DISTINCT
                                COALESCE(NULLIF(LOWER(sc.chatter_login), ''), sc.chatter_id) AS viewer_key,
                                NULLIF(LOWER(sc.chatter_login), '') AS chatter_login
                            FROM twitch_session_chatters sc
                            JOIN twitch_stream_sessions s ON s.id = sc.session_id
                            WHERE s.started_at >= %s
                              {end_filter}
                              AND LOWER(s.streamer_login) = %s
                              AND s.ended_at IS NOT NULL
                              AND COALESCE(NULLIF(sc.chatter_login, ''), sc.chatter_id) IS NOT NULL
                              AND {session_bot_clause}
                        )
                        SELECT
                            COUNT(DISTINCT pv.viewer_key) AS total_viewers,
                            COUNT(DISTINCT CASE
                                WHEN cr.chatter_login IS NOT NULL THEN pv.viewer_key
                            END) AS returning_viewers
                        FROM period_viewers pv
                        LEFT JOIN twitch_chatter_rollup cr
                            ON cr.chatter_login = pv.chatter_login
                           AND LOWER(cr.streamer_login) = %s
                           AND {rollup_bot_clause}
                           AND cr.first_seen_at < %s
                        """,
                        q_params,
                    ).fetchone()
                    total = int(row[0] or 0)
                    returning = int(row[1] or 0)
                    rate = round(returning / total * 100, 1) if total > 0 else 0.0
                    return rate, total

                curr_rate, curr_total = _true_return_rate(since_date, None)
                prev_rate, _ = _true_return_rate(prev_since, since_date)

                # Calculate trends
                def calc_trend(curr, prev):
                    if not prev or prev == 0:
                        return 0
                    return round(((curr - prev) / prev) * 100, 1)

                current_watch_method = str(
                    (current_watch.get("dataQuality", {}) or {}).get("method") or "no_data"
                )
                previous_watch_method = str(
                    (previous_watch.get("dataQuality", {}) or {}).get("method") or "no_data"
                )
                current_avg_watch_time = float(current_watch.get("avgWatchTime", 0) or 0)
                previous_avg_watch_time = float(previous_watch.get("avgWatchTime", 0) or 0)
                watch_time_trend_available = (
                    current_watch_method == "real_samples"
                    and previous_watch_method == "real_samples"
                    and previous_avg_watch_time > 0
                )
                if watch_time_trend_available:
                    watch_time_change = calc_trend(
                        current_avg_watch_time,
                        previous_avg_watch_time,
                    )
                else:
                    watch_time_change = None

                return_rate = curr_rate
                prev_return_rate = prev_rate
                viewer_return_trend_available = prev_return_rate > 0

                return web.json_response(
                    {
                        "trends": {
                            "watchTimeChange": watch_time_change,
                            "conversionChange": None,
                            "viewerReturnRate": round(return_rate, 1),
                            "viewerReturnChange": (
                                calc_trend(return_rate, prev_return_rate)
                                if viewer_return_trend_available
                                else None
                            ),
                        },
                        "distinctViewers": curr_total,
                        "returnRateMethod": "distinct_rollup",
                        "dataQuality": {
                            "botFilterApplied": True,
                            "watchTimeMethod": current_watch_method,
                            "watchTimeTrendAvailable": watch_time_trend_available,
                            "viewerReturnTrendAvailable": viewer_return_trend_available,
                            "conversionTrendAvailable": False,
                        },
                    }
                )
        except Exception as exc:
            log.exception("Error in audience insights API")
            return analytics_internal_error_response()

    async def _api_v2_audience_demographics(self, request: web.Request) -> web.Response:
        """Get estimated audience demographics based on available data."""
        self._require_v2_auth(request)
        self._require_extended_plan(request)

        streamer = request.query.get("streamer", "").strip() or None
        days = min(max(int(request.query.get("days", "30")), 7), 365)
        tz_requested = request.query.get("timezone", "UTC")
        target_tz, timezone_name = self._resolve_target_timezone(tz_requested)

        if not streamer:
            return web.json_response({"error": "Streamer required"}, status=400)

        try:
            with storage.transaction() as conn:
                since_date = (datetime.now(UTC) - timedelta(days=days)).isoformat()
                msg_bot_clause, msg_bot_params = build_known_chat_bot_not_in_clause(
                    column_expr="cm.chatter_login",
                    placeholder="%s",
                )
                session_bot_clause, session_bot_params = build_known_chat_bot_not_in_clause(
                    column_expr="sc.chatter_login",
                    placeholder="%s",
                )
                rollup_bot_clause, rollup_bot_params = build_known_chat_bot_not_in_clause(
                    column_expr="chatter_login",
                    placeholder="%s",
                )

                # Language mix (avoid hard-coded German defaults)
                language_rows = conn.execute(
                    """
                    SELECT
                        LOWER(COALESCE(NULLIF(s.language, ''), 'unknown')) as lang,
                        COUNT(*) as sessions,
                        AVG(s.avg_viewers) as avg_viewers
                    FROM twitch_stream_sessions s
                    WHERE s.started_at >= %s AND LOWER(s.streamer_login) = %s AND s.ended_at IS NOT NULL
                    GROUP BY lang
                    ORDER BY sessions DESC
                """,
                    [since_date, streamer.lower()],
                ).fetchall()

                language_session_total = int(sum(r[1] or 0 for r in language_rows))
                primary_lang_row = language_rows[0] if language_rows else None
                primary_lang_code = (
                    (primary_lang_row[0] or "unknown") if primary_lang_row else "unknown"
                )
                language_confidence = (
                    round(((primary_lang_row[1] or 0) / max(1, language_session_total)) * 100, 1)
                    if primary_lang_row
                    else 0.0
                )

                def _lang_label(code: str) -> str:
                    c = (code or "unknown").lower()
                    if c.startswith("de"):
                        return "German"
                    if c.startswith("en"):
                        return "English"
                    if c.startswith("fr"):
                        return "French"
                    if c.startswith("es"):
                        return "Spanish"
                    if c.startswith("pt"):
                        return "Portuguese"
                    if c.startswith("tr"):
                        return "Turkish"
                    if c.startswith("pl"):
                        return "Polish"
                    if c.startswith("ru"):
                        return "Russian"
                    if c.startswith("it"):
                        return "Italian"
                    if c == "unknown":
                        return "Unbekannt"
                    return c

                primary_language_label = _lang_label(primary_lang_code)

                # Analyze stream times to estimate audience timezone/region
                time_stats = conn.execute(
                    """
                    SELECT
                        EXTRACT(HOUR FROM (s.started_at AT TIME ZONE 'UTC'))::int as hour,
                        AVG(s.avg_viewers) as avg_viewers,
                        COUNT(*) as stream_count
                    FROM twitch_stream_sessions s
                    WHERE s.started_at >= %s AND LOWER(s.streamer_login) = %s AND s.ended_at IS NOT NULL
                    GROUP BY hour
                """,
                    [since_date, streamer.lower()],
                ).fetchall()

                def _hour_weight(row) -> float:
                    avg_v = float(row[1] or 0.0)
                    stream_count = float(row[2] or 0.0)
                    return max(1.0, avg_v) * max(1.0, stream_count)

                peak_hours, peak_meta = self._compute_weighted_peak_hours(
                    conn=conn,
                    streamer_login=streamer.lower(),
                    since_date=since_date,
                    target_tz=target_tz,
                )
                peak_hours_method = (
                    f"weighted_chat_activity_exp_decay_h{int(self.PEAK_HALF_LIFE_SESSIONS)}"
                    f"_w{self.PEAK_SESSION_WINDOW}_winsor_p90"
                )

                # Region estimation combines language hint + schedule
                region_scores = {"DACH": 0.0, "Rest EU": 0.0, "NA": 0.0, "Other": 0.0}
                lang_hint = primary_lang_code.lower()
                dach_langs = {"de", "de-de", "de-at", "de-ch", "ger", "german"}
                eu_langs = {
                    "fr",
                    "fr-fr",
                    "es",
                    "es-es",
                    "it",
                    "pl",
                    "ru",
                    "nl",
                    "sv",
                    "da",
                    "fi",
                    "tr",
                }

                if lang_hint in dach_langs:
                    region_scores["DACH"] += 3.5
                    region_scores["Rest EU"] += 2.0
                elif lang_hint in eu_langs:
                    region_scores["Rest EU"] += 3.0
                    region_scores["Other"] += 0.8
                elif lang_hint.startswith("en"):
                    region_scores["NA"] += 2.5
                    region_scores["Rest EU"] += 2.0
                elif lang_hint.startswith("pt") or lang_hint.startswith("es"):
                    region_scores["Other"] += 2.5  # LATAM/BR bucket -> Other bucket
                    region_scores["Rest EU"] += 1.0
                else:
                    region_scores["Other"] += 2.0

                for hour, avg_viewers, stream_count in time_stats:
                    score = _hour_weight((hour, avg_viewers, stream_count))
                    if 17 <= hour <= 23:
                        region_scores["Rest EU"] += score
                        if lang_hint in dach_langs:
                            region_scores["DACH"] += score * 0.7
                    if 0 <= hour <= 5:
                        region_scores["NA"] += score
                    if 6 <= hour <= 12:
                        region_scores["Other"] += score
                    if 12 < hour < 17:
                        region_scores["Rest EU"] += score * 0.5

                total_region_score = sum(region_scores.values()) or 1.0
                regions = [
                    {"region": name, "percentage": round(score / total_region_score * 100, 1)}
                    for name, score in region_scores.items()
                ]

                session_stats = conn.execute(
                    """
                    SELECT
                        COUNT(*) as session_count,
                        COALESCE(SUM(s.duration_seconds), 0) as total_duration_seconds,
                        AVG(s.avg_viewers) as avg_viewers,
                        COALESCE(
                            SUM(
                                COALESCE(s.avg_viewers, 0) * GREATEST(COALESCE(s.duration_seconds, 0), 0) / 60.0
                            ),
                            0
                        ) as viewer_minutes_fallback
                    FROM twitch_stream_sessions s
                    WHERE s.started_at >= %s AND LOWER(s.streamer_login) = %s AND s.ended_at IS NOT NULL
                    """,
                    [since_date, streamer.lower()],
                ).fetchone()
                session_count = int(session_stats[0]) if session_stats and session_stats[0] else 0
                avg_viewers = float(session_stats[2]) if session_stats and session_stats[2] else 0.0
                viewer_minutes_fallback = (
                    float(session_stats[3]) if session_stats and session_stats[3] else 0.0
                )

                viewer_sample_row = conn.execute(
                    """
                    SELECT
                        COUNT(*) as sample_count,
                        COALESCE(SUM(GREATEST(sv.viewer_count, 0)), 0) as viewer_minutes
                    FROM twitch_session_viewers sv
                    JOIN twitch_stream_sessions s ON s.id = sv.session_id
                    WHERE s.started_at >= %s
                      AND LOWER(s.streamer_login) = %s
                      AND s.ended_at IS NOT NULL
                    """,
                    [since_date, streamer.lower()],
                ).fetchone()
                viewer_sample_count = (
                    int(viewer_sample_row[0]) if viewer_sample_row and viewer_sample_row[0] else 0
                )
                viewer_minutes_samples = (
                    float(viewer_sample_row[1]) if viewer_sample_row and viewer_sample_row[1] else 0.0
                )
                viewer_minutes = (
                    viewer_minutes_samples if viewer_sample_count > 0 else viewer_minutes_fallback
                )
                viewer_minutes_has_real_samples = viewer_sample_count > 0

                # Distinct viewer cohorts with fallback when is_first_time_streamer is missing
                viewer_rows = conn.execute(
                    f"""
                    WITH per_user AS (
                        SELECT
                            COALESCE(NULLIF(sc.chatter_login, ''), sc.chatter_id) AS user_id,
                            NULLIF(sc.chatter_login, '') AS chatter_login,
                            COUNT(DISTINCT sc.session_id) AS session_count,
                            MAX(CASE WHEN sc.messages > 0 THEN 1 ELSE 0 END) AS active_flag,
                            MAX(CASE WHEN sc.messages = 0 AND sc.seen_via_chatters_api IS TRUE THEN 1 ELSE 0 END) AS lurker_flag,
                            MAX(
                                CASE
                                    WHEN LOWER(COALESCE(CAST(sc.is_first_time_streamer AS TEXT), '0')) IN ('1', 't', 'true')
                                    THEN 1
                                    ELSE 0
                                END
                            ) AS first_time_flag,
                            MAX(CASE WHEN sc.is_first_time_streamer IS NOT NULL THEN 1 ELSE 0 END) AS has_first_flag,
                            MAX(CASE WHEN sc.seen_via_chatters_api IS TRUE THEN 1 ELSE 0 END) AS seen_flag
                        FROM twitch_session_chatters sc
                        JOIN twitch_stream_sessions s ON s.id = sc.session_id
                        WHERE s.started_at >= %s AND LOWER(s.streamer_login) = %s AND s.ended_at IS NOT NULL
                          AND COALESCE(NULLIF(sc.chatter_login, ''), sc.chatter_id) IS NOT NULL
                          AND {session_bot_clause}
                        GROUP BY user_id, chatter_login
                    ),
                    rollup AS (
                        SELECT
                            LOWER(streamer_login) AS streamer_login,
                            LOWER(chatter_login) AS chatter_login,
                            first_seen_at
                        FROM twitch_chatter_rollup
                        WHERE LOWER(streamer_login) = %s
                          AND {rollup_bot_clause}
                    )
                    SELECT
                        pu.user_id,
                        pu.chatter_login,
                        pu.session_count,
                        pu.active_flag,
                        pu.lurker_flag,
                        pu.first_time_flag,
                        pu.has_first_flag,
                        pu.seen_flag,
                        CASE
                            WHEN r.chatter_login IS NOT NULL AND r.first_seen_at < %s
                            THEN 1 ELSE 0
                        END AS seen_before
                    FROM per_user pu
                    LEFT JOIN rollup r ON r.chatter_login = LOWER(pu.chatter_login)
                """,
                    [
                        since_date,
                        streamer.lower(),
                        *session_bot_params,
                        streamer.lower(),
                        *rollup_bot_params,
                        since_date,
                    ],
                ).fetchall()

                viewer_entries = []
                has_first_flag_data = False
                for row in viewer_rows:
                    entry = {
                        "user_id": row[0],
                        "chatter_login": row[1],
                        "session_count": int(row[2] or 0),
                        "active": bool(row[3]),
                        "lurker": bool(row[4]),
                        "first_flag": bool(row[5]),
                        "has_first_flag": bool(row[6]),
                        "seen_flag": bool(row[7]),
                        "seen_before": bool(row[8]),
                    }
                    has_first_flag_data = has_first_flag_data or entry["has_first_flag"]
                    viewer_entries.append(entry)

                total_viewers = len(viewer_entries)
                loyalty_returning = sum(1 for v in viewer_entries if v["session_count"] >= 2)

                first_time_viewers = 0
                returning_viewers = 0
                dedicated = 0
                casual = 0
                new_viewers = 0

                # Detect cold-rollup: if >90% have seen_before=False, the rollup
                # was recently built and first_seen_at is unreliable.  Fall back to
                # within-window session_count as the returning indicator.
                seen_before_count = sum(1 for v in viewer_entries if v["seen_before"])
                cold_rollup = total_viewers > 0 and (seen_before_count / total_viewers) < 0.1

                for v in viewer_entries:
                    if cold_rollup:
                        # Fallback: use session_count within the current window
                        is_returning = v["session_count"] >= 2
                    elif has_first_flag_data and v["has_first_flag"]:
                        is_first = v["first_flag"]
                        if (not is_first) and v["lurker"] and (not v["seen_before"]):
                            is_first = True
                        is_returning = not is_first
                    else:
                        is_returning = v["seen_before"] if v["chatter_login"] else False

                    if is_returning:
                        returning_viewers += 1
                        if v["active"] and v["session_count"] >= 3:
                            dedicated += 1
                        elif v["active"]:
                            # Regular = returning + active but fewer sessions
                            casual += 1  # counted below via regular
                    else:
                        first_time_viewers += 1
                        if v["active"]:
                            casual += 1
                        else:
                            new_viewers += 1

                # Clean classification with 5 categories including Silent Regulars
                dedicated = 0
                regular = 0
                casual = 0
                silent_regular = 0
                new_viewers = 0
                first_time_viewers = 0
                returning_viewers = 0

                for v in viewer_entries:
                    if cold_rollup:
                        is_returning = v["session_count"] >= 2
                    elif has_first_flag_data and v["has_first_flag"]:
                        is_first = v["first_flag"]
                        if (not is_first) and v["lurker"] and (not v["seen_before"]):
                            is_first = True
                        is_returning = not is_first
                    else:
                        is_returning = v["seen_before"] if v["chatter_login"] else False

                    if is_returning:
                        returning_viewers += 1
                        if v["active"] and v["session_count"] >= 3:
                            dedicated += 1
                        elif v["active"]:
                            regular += 1
                        elif v["lurker"] or v["seen_flag"]:
                            # Returning + lurker (seen via chatters API, no messages)
                            silent_regular += 1
                    else:
                        first_time_viewers += 1
                        if v["active"]:
                            casual += 1
                        else:
                            new_viewers += 1

                def _pct(part: int, whole: int) -> float:
                    return round((part / whole) * 100, 1) if whole > 0 else 0.0

                active_viewers = sum(1 for v in viewer_entries if v["active"])
                seen_via_chatters_viewers = sum(1 for v in viewer_entries if v["seen_flag"])
                total_messages_row = conn.execute(
                    f"""
                    SELECT COUNT(*)
                    FROM twitch_chat_messages cm
                    WHERE cm.message_ts >= %s AND LOWER(cm.streamer_login) = %s
                      AND {msg_bot_clause}
                    """,
                    [since_date, streamer.lower(), *msg_bot_params],
                ).fetchone()
                total_messages = (
                    int(total_messages_row[0]) if total_messages_row and total_messages_row[0] else 0
                )
                sessions_with_chat_row = conn.execute(
                    f"""
                    SELECT COUNT(DISTINCT sc.session_id)
                    FROM twitch_session_chatters sc
                    JOIN twitch_stream_sessions s ON s.id = sc.session_id
                    WHERE s.started_at >= %s
                      AND LOWER(s.streamer_login) = %s
                      AND s.ended_at IS NOT NULL
                      AND {session_bot_clause}
                    """,
                    [since_date, streamer.lower(), *session_bot_params],
                ).fetchone()
                sessions_with_chat = (
                    int(sessions_with_chat_row[0])
                    if sessions_with_chat_row and sessions_with_chat_row[0]
                    else 0
                )

                engagement = calculate_engagement(
                    EngagementInputs(
                        total_messages=total_messages,
                        active_chatters=active_viewers,
                        tracked_chat_accounts=total_viewers,
                        chatters_api_seen=seen_via_chatters_viewers,
                        viewer_minutes=viewer_minutes,
                        viewer_minutes_has_real_samples=viewer_minutes_has_real_samples,
                        avg_viewers=avg_viewers,
                        session_count=session_count,
                        sessions_with_chat=sessions_with_chat,
                    )
                )
                interaction_coverage = engagement.chatters_coverage
                interaction_rate_pct = (
                    engagement.chat_penetration_pct
                    if engagement.chat_penetration_pct is not None
                    else 0.0
                )
                interaction_rate_reliable = engagement.chat_penetration_reliable

                viewer_type = [
                    {"label": "Dedicated Fans", "percentage": _pct(dedicated, total_viewers)},
                    {"label": "Regular Viewers", "percentage": _pct(regular, total_viewers)},
                    {"label": "Silent Regulars", "percentage": _pct(silent_regular, total_viewers)},
                    {"label": "Casual Viewers", "percentage": _pct(casual, total_viewers)},
                    {"label": "New Visitors", "percentage": _pct(new_viewers, total_viewers)},
                ]

                loyalty_score = _pct(loyalty_returning, total_viewers)

                # Activity pattern based on stream schedule
                schedule_stats = conn.execute(
                    """
                    SELECT
                        EXTRACT(DOW FROM (s.started_at AT TIME ZONE 'UTC'))::int as weekday,
                        COUNT(*) as count
                    FROM twitch_stream_sessions s
                    WHERE s.started_at >= %s AND LOWER(s.streamer_login) = %s AND s.ended_at IS NOT NULL
                    GROUP BY weekday
                """,
                    [since_date, streamer.lower()],
                ).fetchall()

                weekday_counts = {r[0]: r[1] for r in schedule_stats}
                weekend_streams = weekday_counts.get(0, 0) + weekday_counts.get(6, 0)
                weekday_streams = sum(weekday_counts.get(i, 0) for i in range(1, 6))

                if weekend_streams > weekday_streams:
                    activity_pattern = "weekend-heavy"
                elif weekday_streams > weekend_streams * 2:
                    activity_pattern = "weekday-focused"
                else:
                    activity_pattern = "balanced"

                schedule_session_total = int(sum(weekday_counts.values()))
                peak_session_count = int(peak_meta.get("sessionCount", 0) or 0)
                peak_sessions_with_activity = int(
                    peak_meta.get("sessionsWithActivity", 0) or 0
                )
                peak_sample_count = int(peak_meta.get("sampleCount", 0) or 0)
                peak_coverage = float(peak_meta.get("coverage", 0.0) or 0.0)

                session_samples = max(
                    language_session_total,
                    schedule_session_total,
                    peak_session_count,
                )
                if peak_sample_count <= 0:
                    peak_quality_method = "no_data"
                elif peak_coverage < 0.20 or peak_sessions_with_activity < 3:
                    peak_quality_method = "low_coverage"
                else:
                    peak_quality_method = "real_samples"

                if peak_quality_method == "real_samples":
                    if peak_sample_count >= 500 and peak_coverage >= 0.60:
                        confidence = "high"
                    elif peak_sample_count >= 150 and peak_coverage >= 0.35:
                        confidence = "medium"
                    else:
                        confidence = "low"
                elif peak_quality_method == "low_coverage":
                    confidence = "low"
                else:
                    confidence = "very_low"

                peak_hours_response = peak_hours if peak_quality_method == "real_samples" else []

                return web.json_response(
                    {
                        "viewerTypes": viewer_type,
                        "activityPattern": activity_pattern,
                        "primaryLanguage": primary_language_label,
                        "languageConfidence": language_confidence,
                        "peakActivityHours": peak_hours_response,
                        "peakHoursMethod": peak_hours_method,
                        "chatPenetrationPct": engagement.chat_penetration_pct,
                        "chatPenetrationReliable": engagement.chat_penetration_reliable,
                        "messagesPer100ViewerMinutes": engagement.messages_per_100_viewer_minutes,
                        "viewerMinutes": engagement.viewer_minutes,
                        "legacyInteractionActivePerAvgViewer": engagement.legacy_interaction_active_per_avg_viewer,
                        "interactiveRate": round(interaction_rate_pct, 1),
                        "interactionRateActivePerViewer": round(interaction_rate_pct, 1),
                        "interactionRateActivePerAvgViewer": engagement.legacy_interaction_active_per_avg_viewer,
                        "interactionRateReliable": interaction_rate_reliable,
                        "loyaltyScore": loyalty_score,
                        "timezone": timezone_name,
                        "dataQuality": {
                            "confidence": confidence,
                            "sessions": session_samples,
                            "method": engagement.method,
                            "peakMethod": peak_quality_method,
                            "coverage": round(peak_coverage, 3),
                            "sampleCount": peak_sample_count,
                            "peakSessionCount": peak_session_count,
                            "peakSessionsWithActivity": peak_sessions_with_activity,
                            "interactiveSampleCount": active_viewers,
                            "interactionCoverage": interaction_coverage,
                            "chattersCoverage": interaction_coverage,
                            "chattersApiCoverage": interaction_coverage,
                            "passiveViewerSamples": engagement.passive_viewer_samples,
                            "viewerSampleCount": viewer_sample_count,
                            "viewerMinutesSource": (
                                "real_samples" if viewer_minutes_has_real_samples else "low_coverage"
                            ),
                            "sessionsWithChat": sessions_with_chat,
                            "chatSessionCoverage": round(engagement.chat_session_coverage * 100.0, 1),
                            "botFilterApplied": True,
                        },
                    }
                )
        except Exception as exc:
            log.exception("Error in audience demographics API")
            return analytics_internal_error_response()

    async def _api_v2_loyalty_curve(self, request: web.Request) -> web.Response:
        """Loyalty/churn distribution: how many chatters came 1x, 2x, 3x, 10x+."""
        self._require_v2_auth(request)
        self._require_extended_plan(request)

        streamer = request.query.get("streamer", "").strip().lower()
        if not streamer:
            return web.json_response({"error": "Streamer required"}, status=400)

        try:
            with storage.transaction() as conn:
                rollup_bot_clause, rollup_bot_params = build_known_chat_bot_not_in_clause(
                    column_expr="chatter_login",
                    placeholder="%s",
                )
                rows = conn.execute(
                    f"""
                    SELECT total_sessions, COUNT(DISTINCT chatter_login) AS chatter_count
                    FROM twitch_chatter_rollup
                    WHERE LOWER(streamer_login) = %s
                      AND {rollup_bot_clause}
                    GROUP BY total_sessions
                    ORDER BY total_sessions
                    """,
                    [streamer, *rollup_bot_params],
                ).fetchall()

                if not rows:
                    return web.json_response({"curve": [], "one_time_rate": None, "total_chatters": 0})

                total = sum(int(r[1] or 0) for r in rows)
                one_time = int(rows[0][1] or 0) if rows and int(rows[0][0] or 0) == 1 else 0

                curve = [
                    {
                        "sessions": int(r[0] or 0),
                        "chatters": int(r[1] or 0),
                        "percentage": round(int(r[1] or 0) / total * 100, 1) if total > 0 else 0,
                    }
                    for r in rows
                ]

                return web.json_response({
                    "curve": curve,
                    "total_chatters": total,
                    "one_time_rate": round(one_time / total * 100, 1) if total > 0 else None,
                    "window": "all_time",
                })
        except Exception as exc:
            log.exception("Error in loyalty curve API")
            return analytics_internal_error_response()
