"""
Analytics API v2 - Performance Mixin.

Performance metrics: heatmaps, periodic stats, tags, title performance,
rankings, category comparison, category timings, category activity series,
viewer timeline, category leaderboard.
"""

from __future__ import annotations

import asyncio
import collections
import json
import logging
from datetime import UTC, datetime, timedelta
from typing import Any

from aiohttp import web

from ..storage import pg as storage
from .error_utils import analytics_internal_error_response

log = logging.getLogger("TwitchStreams.AnalyticsV2")

# Streamers whose average viewer count exceeds this threshold are considered to have
# external reach (e.g. YouTube / large social media following) and are excluded from
# category averages and percentile calculations when exclude_external=1 is requested.
EXTERNAL_REACH_AVG_THRESHOLD = 100.0

_MONTH_LABELS = [
    "",
    "Jan",
    "Feb",
    "Mar",
    "Apr",
    "Mai",
    "Jun",
    "Jul",
    "Aug",
    "Sep",
    "Okt",
    "Nov",
    "Dez",
]
_WEEKDAY_LABELS = ["So", "Mo", "Di", "Mi", "Do", "Fr", "Sa"]


def _parse_bounded_query_int(
    request: web.Request,
    *,
    name: str,
    default: int,
    minimum: int,
    maximum: int,
) -> int:
    raw_value = (request.query.get(name, str(default)) or str(default)).strip()
    try:
        parsed = int(raw_value)
    except (TypeError, ValueError) as exc:
        raise ValueError(f"{name} must be an integer") from exc
    return min(max(parsed, minimum), maximum)


def _load_hourly_heatmap_payload(*, streamer: str | None, days: int) -> list[dict[str, Any]]:
    since_date = (datetime.now(UTC) - timedelta(days=days)).isoformat()
    streamer_login = streamer.lower() if streamer else None
    with storage.readonly_connection() as conn:
        rows = conn.execute(
            """
            SELECT
                EXTRACT(DOW FROM s.started_at)::integer as weekday,
                EXTRACT(HOUR FROM s.started_at)::integer as hour,
                COUNT(*) as stream_count,
                AVG(s.avg_viewers) as avg_viewers,
                AVG(s.peak_viewers) as avg_peak
            FROM twitch_stream_sessions s
            WHERE s.started_at >= %s
              AND s.ended_at IS NOT NULL
              AND (COALESCE(%s, '') = '' OR LOWER(s.streamer_login) = %s)
            GROUP BY 1, 2
            """,
            (since_date, streamer_login, streamer_login),
        ).fetchall()
    return [
        {
            "weekday": row[0],
            "hour": row[1],
            "streamCount": row[2],
            "avgViewers": float(row[3]) if row[3] else 0,
            "avgPeak": float(row[4]) if row[4] else 0,
        }
        for row in rows
    ]


def _load_calendar_heatmap_payload(*, streamer: str | None, days: int) -> list[dict[str, Any]]:
    since_date = (datetime.now(UTC) - timedelta(days=days)).isoformat()
    streamer_login = streamer.lower() if streamer else None
    with storage.readonly_connection() as conn:
        rows = conn.execute(
            """
            SELECT
                DATE(s.started_at) as date,
                COUNT(*) as stream_count,
                SUM(s.avg_viewers * s.duration_seconds / 3600.0) as hours_watched
            FROM twitch_stream_sessions s
            WHERE s.started_at >= %s
              AND s.ended_at IS NOT NULL
              AND (COALESCE(%s, '') = '' OR LOWER(s.streamer_login) = %s)
            GROUP BY DATE(s.started_at)
            """,
            (since_date, streamer_login, streamer_login),
        ).fetchall()
    return [
        {
            "date": row[0].isoformat() if hasattr(row[0], "isoformat") else str(row[0]),
            "streamCount": row[1],
            "hoursWatched": float(row[2]) if row[2] else 0,
            "value": float(row[2]) if row[2] else 0,
        }
        for row in rows
    ]


def _load_monthly_stats_payload(*, streamer: str | None, months: int) -> list[dict[str, Any]]:
    since_date = (datetime.now(UTC) - timedelta(days=round(months * 30.44))).isoformat()
    streamer_login = streamer.lower() if streamer else None
    with storage.readonly_connection() as conn:
        rows = conn.execute(
            """
            SELECT
                EXTRACT(YEAR FROM s.started_at)::integer as year,
                EXTRACT(MONTH FROM s.started_at)::integer as month,
                SUM(s.avg_viewers * s.duration_seconds / 3600.0) as hours_watched,
                SUM(s.duration_seconds / 3600.0) as airtime,
                AVG(s.avg_viewers) as avg_viewers,
                MAX(s.peak_viewers) as peak_viewers,
                SUM(CASE WHEN s.follower_delta IS NOT NULL
                     AND NOT (s.followers_end = 0 AND s.followers_start > 0)
                     THEN s.follower_delta ELSE 0 END) as follower_delta,
                SUM(s.unique_chatters) as total_chatter_sessions,
                COUNT(*) as stream_count
            FROM twitch_stream_sessions s
            WHERE s.started_at >= %s
              AND s.ended_at IS NOT NULL
              AND (COALESCE(%s, '') = '' OR LOWER(s.streamer_login) = %s)
            GROUP BY 1, 2
            ORDER BY 1 DESC, 2 DESC
            """,
            (since_date, streamer_login, streamer_login),
        ).fetchall()
    return [
        {
            "year": row[0],
            "month": row[1],
            "monthLabel": _MONTH_LABELS[row[1]] if row[1] else "",
            "totalHoursWatched": float(row[2]) if row[2] else 0,
            "totalAirtime": float(row[3]) if row[3] else 0,
            "avgViewers": float(row[4]) if row[4] else 0,
            "peakViewers": int(row[5]) if row[5] else 0,
            "followerDelta": int(row[6]) if row[6] else 0,
            "totalChatterSessions": int(row[7]) if row[7] else 0,
            "streamCount": row[8],
        }
        for row in rows
    ]


def _load_weekly_stats_payload(*, streamer: str | None, days: int) -> list[dict[str, Any]]:
    since_date = (datetime.now(UTC) - timedelta(days=days)).isoformat()
    streamer_login = streamer.lower() if streamer else None
    with storage.readonly_connection() as conn:
        rows = conn.execute(
            """
            SELECT
                EXTRACT(DOW FROM s.started_at)::integer as weekday,
                COUNT(*) as stream_count,
                AVG(s.duration_seconds / 3600.0) as avg_hours,
                AVG(s.avg_viewers) as avg_viewers,
                AVG(s.peak_viewers) as avg_peak,
                SUM(CASE WHEN s.follower_delta IS NOT NULL
                     AND NOT (s.followers_end = 0 AND s.followers_start > 0)
                     THEN s.follower_delta ELSE 0 END) as total_followers
            FROM twitch_stream_sessions s
            WHERE s.started_at >= %s
              AND s.ended_at IS NOT NULL
              AND (COALESCE(%s, '') = '' OR LOWER(s.streamer_login) = %s)
            GROUP BY 1
            ORDER BY 1
            """,
            (since_date, streamer_login, streamer_login),
        ).fetchall()
    return [
        {
            "weekday": row[0],
            "weekdayLabel": _WEEKDAY_LABELS[row[0]] if row[0] is not None else "",
            "streamCount": row[1],
            "avgHours": float(row[2]) if row[2] else 0,
            "avgViewers": float(row[3]) if row[3] else 0,
            "avgPeak": float(row[4]) if row[4] else 0,
            "totalFollowers": int(row[5]) if row[5] else 0,
        }
        for row in rows
    ]


class _AnalyticsPerformanceMixin:
    """Mixin providing performance metrics endpoints."""

    def _get_peer_group_stats(self, conn, streamer_login: str, since_date: str) -> dict | None:
        """Calculate peer group stats for a streamer.

        Peer-Group Tiers:
        - starter: 0\u201315 avg viewers
        - rising: 15\u201350 avg viewers
        - established: 50\u2013150 avg viewers
        - featured: 150\u2013500 avg viewers
        - top: 500+ avg viewers
        """
        import statistics as _stats

        # 1. Get all streamer averages in the category
        all_avgs = conn.execute(
            """
            SELECT streamer, AVG(viewer_count) AS avg_vc
            FROM twitch_stats_category
            WHERE ts_utc >= %s
            GROUP BY streamer
            """,
            (since_date,),
        ).fetchall()

        if not all_avgs:
            return None

        # 2. Build dict of streamer -> avg_vc
        streamer_avgs: dict[str, float] = {
            str(r[0]).lower(): float(r[1]) for r in all_avgs if r[1] is not None
        }

        # 3. Determine this streamer's average
        my_avg = streamer_avgs.get(streamer_login.lower())
        if my_avg is None:
            # Streamer not in category data, try from sessions
            sess_row = conn.execute(
                """
                SELECT AVG(avg_viewers) as avg_vc
                FROM twitch_stream_sessions
                WHERE LOWER(streamer_login) = %s AND started_at >= %s AND ended_at IS NOT NULL
                """,
                (streamer_login.lower(), since_date),
            ).fetchone()
            if sess_row and sess_row[0]:
                my_avg = float(sess_row[0])
            else:
                return None

        # 4. Classify into tier
        def _get_tier(avg: float) -> tuple[str, str]:
            if avg < 15:
                return ("starter", "Starter (0\u201315 \u00d8)")
            if avg < 50:
                return ("rising", "Rising (15\u201350 \u00d8)")
            if avg < 150:
                return ("established", "Established (50\u2013150 \u00d8)")
            if avg < 500:
                return ("featured", "Featured (150\u2013500 \u00d8)")
            return ("top", "Top (500+ \u00d8)")

        my_tier, my_tier_label = _get_tier(my_avg)

        # 5. Get all streamers in same tier
        peer_logins = [s for s, avg in streamer_avgs.items() if _get_tier(avg)[0] == my_tier]

        if not peer_logins:
            return None

        # 6. Get session-level metrics for peer streamers
        peer_metrics = conn.execute(
            """
            SELECT
                LOWER(s.streamer_login) as login,
                AVG(s.avg_viewers) as avg_viewers,
                MAX(s.peak_viewers) as peak_viewers,
                AVG(s.retention_10m) as retention_10m,
                AVG(CASE WHEN s.avg_viewers > 0
                    THEN s.unique_chatters * 100.0 / s.avg_viewers ELSE 0 END) as chat_health
            FROM twitch_stream_sessions s
            WHERE LOWER(s.streamer_login) = ANY(%s)
              AND s.started_at >= %s AND s.ended_at IS NOT NULL
            GROUP BY LOWER(s.streamer_login)
            """,
            (peer_logins, since_date),
        ).fetchall()

        # 7. Calculate median and percentile for each metric
        def _safe_median(values: list[float]) -> float | None:
            return _stats.median(values) if values else None

        def _peer_percentile(sorted_vals: list[float], value: float | None) -> float | None:
            if not sorted_vals or value is None:
                return None
            count_below = sum(1 for v in sorted_vals if v < value)
            return round(count_below / len(sorted_vals) * 100, 1)

        avg_viewers_list = sorted([float(r[1]) for r in peer_metrics if r[1] is not None])
        peak_viewers_list = sorted([float(r[2]) for r in peer_metrics if r[2] is not None])
        retention_list = sorted([float(r[3]) for r in peer_metrics if r[3] is not None])
        chat_health_list = sorted([float(r[4]) for r in peer_metrics if r[4] is not None])

        # Get this streamer's metrics from the peer query
        my_row = next(
            (r for r in peer_metrics if str(r[0]).lower() == streamer_login.lower()),
            None,
        )

        return {
            "tier": my_tier,
            "tierLabel": my_tier_label,
            "tierSize": len(peer_logins),
            "peerAvg": {
                "avgViewers": round(_safe_median(avg_viewers_list) or 0, 1),
                "peakViewers": round(_safe_median(peak_viewers_list) or 0),
                "retention10m": round((_safe_median(retention_list) or 0) * 100, 1),
                "chatHealth": round(_safe_median(chat_health_list) or 0, 1),
            },
            "peerPercentiles": {
                "avgViewers": _peer_percentile(
                    avg_viewers_list,
                    float(my_row[1]) if my_row and my_row[1] else my_avg,
                ),
                "peakViewers": _peer_percentile(
                    peak_viewers_list,
                    float(my_row[2]) if my_row and my_row[2] else None,
                ),
                "retention10m": _peer_percentile(
                    retention_list,
                    float(my_row[3]) if my_row and my_row[3] else None,
                ),
                "chatHealth": _peer_percentile(
                    chat_health_list,
                    float(my_row[4]) if my_row and my_row[4] else None,
                ),
            },
        }

    async def _api_v2_hourly_heatmap(self, request: web.Request) -> web.Response:
        """Get hourly heatmap data."""
        self._require_v2_auth(request)

        streamer = request.query.get("streamer", "").strip() or None
        try:
            days = _parse_bounded_query_int(
                request,
                name="days",
                default=30,
                minimum=7,
                maximum=365,
            )
        except ValueError as exc:
            return web.json_response({"error": str(exc)}, status=400)

        try:
            data = await asyncio.to_thread(
                _load_hourly_heatmap_payload,
                streamer=streamer,
                days=days,
            )
            return web.json_response(data)
        except Exception:
            log.exception("Error in hourly heatmap API")
            return analytics_internal_error_response()

    async def _api_v2_calendar_heatmap(self, request: web.Request) -> web.Response:
        """Get calendar heatmap data."""
        self._require_v2_auth(request)

        streamer = request.query.get("streamer", "").strip() or None
        try:
            days = _parse_bounded_query_int(
                request,
                name="days",
                default=365,
                minimum=30,
                maximum=365,
            )
        except ValueError as exc:
            return web.json_response({"error": str(exc)}, status=400)

        try:
            data = await asyncio.to_thread(
                _load_calendar_heatmap_payload,
                streamer=streamer,
                days=days,
            )
            return web.json_response(data)
        except Exception:
            log.exception("Error in calendar heatmap API")
            return analytics_internal_error_response()

    async def _api_v2_monthly_stats(self, request: web.Request) -> web.Response:
        """Get monthly aggregated stats."""
        self._require_v2_auth(request)

        streamer = request.query.get("streamer", "").strip() or None
        try:
            months = _parse_bounded_query_int(
                request,
                name="months",
                default=12,
                minimum=1,
                maximum=24,
            )
        except ValueError as exc:
            return web.json_response({"error": str(exc)}, status=400)

        try:
            data = await asyncio.to_thread(
                _load_monthly_stats_payload,
                streamer=streamer,
                months=months,
            )
            return web.json_response(data)
        except Exception:
            log.exception("Error in monthly stats API")
            return analytics_internal_error_response()

    async def _api_v2_weekly_stats(self, request: web.Request) -> web.Response:
        """Get weekday analysis stats."""
        self._require_v2_auth(request)

        streamer = request.query.get("streamer", "").strip() or None
        try:
            days = _parse_bounded_query_int(
                request,
                name="days",
                default=30,
                minimum=7,
                maximum=365,
            )
        except ValueError as exc:
            return web.json_response({"error": str(exc)}, status=400)

        try:
            data = await asyncio.to_thread(
                _load_weekly_stats_payload,
                streamer=streamer,
                days=days,
            )
            return web.json_response(data)
        except Exception:
            log.exception("Error in weekly stats API")
            return analytics_internal_error_response()

    async def _api_v2_tag_analysis(self, request: web.Request) -> web.Response:
        """Get tag performance analysis."""
        self._require_v2_auth(request)

        try:
            # Tags are stored as JSON in the tags column
            # This is a simplified version - full implementation would parse JSON
            return web.json_response([])
        except Exception:
            log.exception("Error in tag analysis API")
            return analytics_internal_error_response()

    def _load_tag_analysis_extended_payload_sync(
        self,
        *,
        streamer: str | None,
        days: int,
        limit: int,
    ) -> dict[str, Any]:
        since_date = (datetime.now(UTC) - timedelta(days=days)).isoformat()
        streamer_login = streamer.lower() if streamer else None
        with storage.readonly_connection() as conn:
            rows = conn.execute(
                """
                SELECT
                    s.id,
                    s.tags,
                    s.avg_viewers,
                    s.retention_10m,
                    CASE WHEN s.follower_delta IS NOT NULL
                         AND NOT (s.followers_end = 0 AND s.followers_start > 0)
                         THEN s.follower_delta ELSE NULL END as follower_delta,
                    s.duration_seconds,
                    EXTRACT(HOUR FROM s.started_at) as start_hour
                FROM twitch_stream_sessions s
                WHERE s.started_at >= %s
                  AND s.ended_at IS NOT NULL
                  AND s.tags IS NOT NULL
                  AND (COALESCE(%s, '') = '' OR LOWER(s.streamer_login) = %s)
            """,
                (since_date, streamer_login, streamer_login),
            ).fetchall()

            tag_stats: dict[str, dict[str, Any]] = {}
            for row in rows:
                tags_str = row[1] or ""
                if tags_str.startswith("["):
                    try:
                        tags = json.loads(tags_str)
                    except json.JSONDecodeError:
                        tags = [tags_str]
                else:
                    tags = [t.strip() for t in tags_str.split(",") if t.strip()]

                seen_tags: set[str] = set()
                for tag in tags[:5]:
                    if tag in seen_tags:
                        continue
                    seen_tags.add(tag)
                    bucket = tag_stats.setdefault(
                        tag,
                        {
                            "viewers": [],
                            "retention": [],
                            "followers": [],
                            "durations": [],
                            "hours": [],
                            "samples": 0,
                        },
                    )
                    bucket["viewers"].append(float(row[2]) if row[2] else 0.0)
                    if row[3] is not None:
                        bucket["retention"].append(float(row[3]) * 100.0)
                    if row[4] is not None:
                        bucket["followers"].append(float(row[4]))
                    bucket["durations"].append(float(row[5]) if row[5] else 0.0)
                    if row[6] is not None:
                        bucket["hours"].append(int(row[6]))
                    bucket["samples"] += 1

            def _median(values: list[float]) -> float:
                if not values:
                    return 0.0
                vals = sorted(values)
                n = len(vals)
                mid = n // 2
                if n % 2 == 1:
                    return vals[mid]
                return (vals[mid - 1] + vals[mid]) / 2

            filtered = {tag: data for tag, data in tag_stats.items() if data["samples"] >= 3}
            sorted_tags = sorted(
                filtered.items(),
                key=lambda item: (_median(item[1]["viewers"]), item[1]["samples"]),
                reverse=True,
            )

            result = []
            for rank, (tag, data) in enumerate(sorted_tags[:limit], 1):
                avg_v = _median(data["viewers"])
                avg_r = _median(data["retention"])
                med_f = _median(data["followers"])
                avg_d = _median(data["durations"])

                if data["hours"]:
                    hour_counts = collections.Counter(data["hours"])
                    best_hour = hour_counts.most_common(1)[0][0]
                    best_slot = f"{best_hour:02d}:00"
                else:
                    best_slot = "18:00-22:00"

                result.append(
                    {
                        "tagName": tag,
                        "usageCount": data["samples"],
                        "avgViewers": round(avg_v, 1),
                        "avgRetention10m": round(avg_r, 1),
                        "avgFollowerGain": round(med_f, 1),
                        "trend": "stable",
                        "trendValue": 0,
                        "bestTimeSlot": best_slot,
                        "avgStreamDuration": round(avg_d, 0),
                        "categoryRank": rank,
                    }
                )

            peer_benchmark = None
            if streamer_login:
                peer_group = self._get_peer_group_stats(conn, streamer_login, since_date)
                if peer_group:
                    peer_benchmark = {
                        "avgViewers": peer_group["peerAvg"]["avgViewers"],
                        "retention10m": peer_group["peerAvg"]["retention10m"],
                    }

        return {
            "tags": result,
            "peerBenchmark": peer_benchmark,
        }

    async def _api_v2_tag_analysis_extended(self, request: web.Request) -> web.Response:
        """Get extended tag performance with trends."""
        self._require_v2_auth(request)
        self._require_extended_plan(request)

        streamer = request.query.get("streamer", "").strip() or None
        try:
            days = _parse_bounded_query_int(
                request,
                name="days",
                default=30,
                minimum=7,
                maximum=365,
            )
            limit = _parse_bounded_query_int(
                request,
                name="limit",
                default=20,
                minimum=5,
                maximum=50,
            )
        except ValueError as exc:
            return web.json_response({"error": str(exc)}, status=400)

        try:
            data = await asyncio.to_thread(
                self._load_tag_analysis_extended_payload_sync,
                streamer=streamer,
                days=days,
                limit=limit,
            )
            return web.json_response(data)
        except Exception:
            log.exception("Error in tag analysis extended API")
            return analytics_internal_error_response()

    @staticmethod
    def _extract_title_keywords(title: str) -> list[str]:
        """Extract a small set of meaningful keywords from a stream title."""
        import re

        stop_words = {
            "der",
            "die",
            "das",
            "und",
            "oder",
            "mit",
            "fur",
            "the",
            "and",
            "or",
            "with",
            "for",
            "to",
            "a",
            "an",
        }
        words = re.findall(r"\b\w{3,}\b", title.lower())
        keywords = [word.capitalize() for word in words if word not in stop_words]
        return keywords[:5]

    def _load_title_performance_payload_sync(
        self,
        *,
        streamer: str,
        days: int,
        limit: int,
    ) -> dict[str, Any]:
        since_date = (datetime.now(UTC) - timedelta(days=days)).isoformat()
        streamer_login = streamer.lower()
        with storage.readonly_connection() as conn:
            rows = conn.execute(
                """
                SELECT
                    s.stream_title,
                    COUNT(*) as usage_count,
                    AVG(s.avg_viewers) as avg_viewers,
                    AVG(s.retention_10m) as avg_retention,
                    AVG(CASE WHEN s.follower_delta IS NOT NULL
                         AND NOT (s.followers_end = 0 AND s.followers_start > 0)
                         THEN s.follower_delta ELSE NULL END) as avg_followers,
                    MAX(s.peak_viewers) as peak_viewers
                FROM twitch_stream_sessions s
                WHERE s.started_at >= %s AND LOWER(s.streamer_login) = %s
                  AND s.ended_at IS NOT NULL AND s.stream_title IS NOT NULL AND s.stream_title != ''
                GROUP BY s.stream_title
                ORDER BY avg_viewers DESC
                LIMIT %s
            """,
                (since_date, streamer_login, limit),
            ).fetchall()

            result = [
                {
                    "title": row[0] or "",
                    "usageCount": row[1],
                    "avgViewers": round(float(row[2]), 1) if row[2] else 0,
                    "avgRetention10m": round(float(row[3]) * 100, 1) if row[3] else 0,
                    "avgFollowerGain": round(float(row[4]), 1) if row[4] else 0,
                    "peakViewers": int(row[5]) if row[5] else 0,
                    "keywords": self._extract_title_keywords(row[0] or ""),
                }
                for row in rows
            ]

            peer_group = self._get_peer_group_stats(conn, streamer_login, since_date)
            peer_benchmark = None
            if peer_group:
                peer_benchmark = {
                    "avgViewers": peer_group["peerAvg"]["avgViewers"],
                    "retention10m": peer_group["peerAvg"]["retention10m"],
                }

        return {
            "titles": result,
            "peerBenchmark": peer_benchmark,
        }

    async def _api_v2_title_performance(self, request: web.Request) -> web.Response:
        """Get stream title performance analysis."""
        self._require_v2_auth(request)
        self._require_extended_plan(request)

        streamer = request.query.get("streamer", "").strip() or None
        try:
            days = _parse_bounded_query_int(
                request,
                name="days",
                default=30,
                minimum=7,
                maximum=365,
            )
            limit = _parse_bounded_query_int(
                request,
                name="limit",
                default=20,
                minimum=5,
                maximum=50,
            )
        except ValueError as exc:
            return web.json_response({"error": str(exc)}, status=400)

        if not streamer:
            return web.json_response({"error": "Streamer required"}, status=400)

        try:
            payload = await asyncio.to_thread(
                self._load_title_performance_payload_sync,
                streamer=streamer,
                days=days,
                limit=limit,
            )
            return web.json_response(payload)
        except Exception:
            log.exception("Error in title performance API")
            return analytics_internal_error_response()

    def _load_rankings_payload_sync(
        self,
        *,
        metric: str,
        days: int,
        limit: int,
        threshold: float | None,
    ) -> list[dict[str, Any]]:
        having_ext = " AND AVG(s.avg_viewers) <= %s" if threshold is not None else ""
        since_date = (datetime.now(UTC) - timedelta(days=days)).isoformat()
        with storage.readonly_connection() as conn:
            if metric == "retention":
                ranking_sql = f"""
                SELECT
                    s.streamer_login,
                    AVG(s.retention_10m) as value
                FROM twitch_stream_sessions s
                WHERE s.started_at >= %s AND s.ended_at IS NOT NULL
                GROUP BY s.streamer_login
                HAVING COUNT(*) >= 3{having_ext}
                ORDER BY value DESC
                LIMIT %s
                """
            elif metric == "growth":
                ranking_sql = f"""
                SELECT
                    s.streamer_login,
                    SUM(CASE WHEN s.follower_delta IS NOT NULL
                         AND NOT (s.followers_end = 0 AND s.followers_start > 0)
                         THEN s.follower_delta ELSE 0 END) as value
                FROM twitch_stream_sessions s
                WHERE s.started_at >= %s AND s.ended_at IS NOT NULL
                GROUP BY s.streamer_login
                HAVING COUNT(*) >= 3{having_ext}
                ORDER BY value DESC
                LIMIT %s
                """
            else:
                metric = "viewers"
                ranking_sql = f"""
                SELECT
                    s.streamer_login,
                    AVG(s.avg_viewers) as value
                FROM twitch_stream_sessions s
                WHERE s.started_at >= %s AND s.ended_at IS NOT NULL
                GROUP BY s.streamer_login
                HAVING COUNT(*) >= 3{having_ext}
                ORDER BY value DESC
                LIMIT %s
                """

            params: list[Any] = [since_date]
            if threshold is not None:
                params.append(threshold)
            params.append(limit)
            rows = conn.execute(ranking_sql, tuple(params)).fetchall()

        return [
            {
                "rank": index + 1,
                "login": row[0],
                "value": (float(row[1]) * 100 if metric == "retention" else float(row[1]))
                if row[1]
                else 0,
                "trend": "same",
                "trendValue": 0,
            }
            for index, row in enumerate(rows)
        ]

    async def _api_v2_rankings(self, request: web.Request) -> web.Response:
        """Get streamer rankings."""
        self._require_v2_auth(request)

        metric = request.query.get("metric", "viewers")
        try:
            days = _parse_bounded_query_int(
                request,
                name="days",
                default=30,
                minimum=7,
                maximum=365,
            )
            limit = _parse_bounded_query_int(
                request,
                name="limit",
                default=20,
                minimum=5,
                maximum=50,
            )
        except ValueError as exc:
            return web.json_response({"error": str(exc)}, status=400)
        exclude_external = request.query.get("exclude_external", "0") == "1"
        threshold = EXTERNAL_REACH_AVG_THRESHOLD if exclude_external else None

        try:
            payload = await asyncio.to_thread(
                self._load_rankings_payload_sync,
                metric=metric,
                days=days,
                limit=limit,
                threshold=threshold,
            )
            return web.json_response(payload)
        except Exception:
            log.exception("Error in rankings API")
            return analytics_internal_error_response()

    def _load_category_comparison_payload_sync(
        self,
        *,
        streamer: str,
        days: int,
        threshold: float | None,
    ) -> dict[str, Any]:
        since_date = (datetime.now(UTC) - timedelta(days=days)).isoformat()
        streamer_login = streamer.lower()
        with storage.readonly_connection() as conn:
            your_tracked = conn.execute(
                """
                SELECT AVG(viewer_count), MAX(viewer_count)
                FROM twitch_stats_tracked
                WHERE ts_utc >= %s AND LOWER(streamer) = %s
            """,
                (since_date, streamer_login),
            ).fetchone()

            your_session = conn.execute(
                """
                SELECT
                    AVG(s.avg_viewers) as avg_viewers,
                    MAX(s.peak_viewers) as peak_viewers,
                    AVG(s.retention_10m) as retention10m,
                    AVG(CASE WHEN s.avg_viewers > 0 THEN s.unique_chatters * 100.0 / s.avg_viewers ELSE 0 END) as chat_health
                FROM twitch_stream_sessions s
                WHERE s.started_at >= %s AND LOWER(s.streamer_login) = %s AND s.ended_at IS NOT NULL
            """,
                (since_date, streamer_login),
            ).fetchone()

            your_avg = (
                float(your_tracked[0])
                if your_tracked and your_tracked[0]
                else (float(your_session[0]) if your_session and your_session[0] else 0)
            )
            your_peak = (
                int(your_tracked[1])
                if your_tracked and your_tracked[1]
                else (int(your_session[1]) if your_session and your_session[1] else 0)
            )
            your_ret = float(your_session[2]) * 100 if your_session and your_session[2] else 0
            your_chat = float(your_session[3]) if your_session and your_session[3] else 0

            cat_data = self._get_category_percentiles(conn, since_date, threshold)
            sorted_avgs = cat_data["sorted_avgs"]
            category_total = cat_data["total"]
            cat_avg_viewers = sum(sorted_avgs) / len(sorted_avgs) if sorted_avgs else 0

            cat_peak_having = "HAVING AVG(viewer_count) <= %s" if threshold is not None else ""
            cat_peak_params: list[Any] = [since_date]
            if threshold is not None:
                cat_peak_params.append(threshold)
            cat_peak = conn.execute(  # nosemgrep: python.sqlalchemy.security.sqlalchemy-execute-raw-query.sqlalchemy-execute-raw-query
                f"""
                SELECT AVG(max_vc) FROM (
                    SELECT MAX(viewer_count) as max_vc
                    FROM twitch_stats_category
                    WHERE ts_utc >= %s
                    GROUP BY streamer
                    {cat_peak_having}
                )
            """,
                tuple(cat_peak_params),
            ).fetchone()
            cat_avg_peak = float(cat_peak[0]) if cat_peak and cat_peak[0] else 0

            if threshold is not None:
                cat_session_avgs = conn.execute(
                    """
                    SELECT
                        AVG(s.retention_10m) as avg_ret,
                        AVG(CASE WHEN s.avg_viewers > 0 THEN s.unique_chatters * 100.0 / s.avg_viewers ELSE 0 END) as avg_chat
                    FROM twitch_stream_sessions s
                    WHERE s.started_at >= %s AND s.ended_at IS NOT NULL
                      AND LOWER(s.streamer_login) NOT IN (
                          SELECT LOWER(streamer_login) FROM twitch_stream_sessions
                          WHERE started_at >= %s AND ended_at IS NOT NULL
                          GROUP BY LOWER(streamer_login)
                          HAVING AVG(avg_viewers) > %s
                      )
                """,
                    (since_date, since_date, threshold),
                ).fetchone()
            else:
                cat_session_avgs = conn.execute(
                    """
                    SELECT
                        AVG(s.retention_10m) as avg_ret,
                        AVG(CASE WHEN s.avg_viewers > 0 THEN s.unique_chatters * 100.0 / s.avg_viewers ELSE 0 END) as avg_chat
                    FROM twitch_stream_sessions s
                    WHERE s.started_at >= %s AND s.ended_at IS NOT NULL
                """,
                    (since_date,),
                ).fetchone()
            cat_avg_ret = float(cat_session_avgs[0]) * 100 if cat_session_avgs and cat_session_avgs[0] else 0
            cat_avg_chat = float(cat_session_avgs[1]) if cat_session_avgs and cat_session_avgs[1] else 0

            per_ret_having = "HAVING AVG(s.avg_viewers) <= %s" if threshold is not None else ""
            per_ret_params: list[Any] = [since_date]
            if threshold is not None:
                per_ret_params.append(threshold)
            per_streamer_ret = conn.execute(  # nosemgrep: python.sqlalchemy.security.sqlalchemy-execute-raw-query.sqlalchemy-execute-raw-query
                f"""
                SELECT AVG(s.retention_10m) as ret
                FROM twitch_stream_sessions s
                WHERE s.started_at >= %s AND s.ended_at IS NOT NULL
                GROUP BY LOWER(s.streamer_login)
                {per_ret_having}
                ORDER BY ret
            """,
                tuple(per_ret_params),
            ).fetchall()
            ret_sorted = [float(row[0]) * 100 for row in per_streamer_ret if row[0] is not None]

            per_chat_having = "HAVING AVG(s.avg_viewers) <= %s" if threshold is not None else ""
            per_chat_params: list[Any] = [since_date]
            if threshold is not None:
                per_chat_params.append(threshold)
            per_streamer_chat = conn.execute(  # nosemgrep: python.sqlalchemy.security.sqlalchemy-execute-raw-query.sqlalchemy-execute-raw-query
                f"""
                SELECT AVG(CASE WHEN s.avg_viewers > 0 THEN s.unique_chatters * 100.0 / s.avg_viewers ELSE 0 END) as ch
                FROM twitch_stream_sessions s
                WHERE s.started_at >= %s AND s.ended_at IS NOT NULL
                GROUP BY LOWER(s.streamer_login)
                {per_chat_having}
                ORDER BY ch
            """,
                tuple(per_chat_params),
            ).fetchall()
            chat_sorted = [float(row[0]) for row in per_streamer_chat if row[0] is not None]

            avg_percentile = int(self._percentile_of(sorted_avgs, your_avg) * 100) if sorted_avgs else 0

            peak_having = "HAVING AVG(viewer_count) <= %s" if threshold is not None else ""
            peak_params: list[Any] = [since_date]
            if threshold is not None:
                peak_params.append(threshold)
            peak_avgs = conn.execute(  # nosemgrep: python.sqlalchemy.security.sqlalchemy-execute-raw-query.sqlalchemy-execute-raw-query
                f"""
                SELECT MAX(viewer_count) as peak
                FROM twitch_stats_category
                WHERE ts_utc >= %s
                GROUP BY streamer
                {peak_having}
                ORDER BY peak
            """,
                tuple(peak_params),
            ).fetchall()
            peak_sorted = [float(row[0]) for row in peak_avgs] if peak_avgs else []
            peak_percentile = int(self._percentile_of(peak_sorted, your_peak) * 100) if peak_sorted else 50

            ret_percentile = int(self._percentile_of(ret_sorted, your_ret) * 100) if ret_sorted else 50
            chat_percentile = int(self._percentile_of(chat_sorted, your_chat) * 100) if chat_sorted else 50
            category_rank = category_total - int(avg_percentile / 100 * category_total) if category_total else 0
            peer_group = self._get_peer_group_stats(conn, streamer_login, since_date)

        return {
            "yourStats": {
                "avgViewers": round(your_avg, 1),
                "peakViewers": your_peak,
                "retention10m": round(your_ret, 1),
                "chatHealth": round(your_chat, 1),
            },
            "categoryAvg": {
                "avgViewers": round(cat_avg_viewers, 1),
                "peakViewers": round(cat_avg_peak, 0),
                "retention10m": round(cat_avg_ret, 1),
                "chatHealth": round(cat_avg_chat, 1),
            },
            "percentiles": {
                "avgViewers": avg_percentile,
                "peakViewers": peak_percentile,
                "retention10m": ret_percentile,
                "chatHealth": chat_percentile,
            },
            "categoryRank": category_rank,
            "categoryTotal": category_total,
            "peerGroup": peer_group,
        }

    async def _api_v2_category_comparison(self, request: web.Request) -> web.Response:
        """Compare streamer to category averages."""
        self._require_v2_auth(request)

        streamer = request.query.get("streamer", "").strip()
        try:
            days = _parse_bounded_query_int(
                request,
                name="days",
                default=30,
                minimum=7,
                maximum=365,
            )
        except ValueError as exc:
            return web.json_response({"error": str(exc)}, status=400)
        exclude_external = request.query.get("exclude_external", "0") == "1"
        threshold: float | None = EXTERNAL_REACH_AVG_THRESHOLD if exclude_external else None

        if not streamer:
            return web.json_response({"error": "Streamer required"}, status=400)

        try:
            payload = await asyncio.to_thread(
                self._load_category_comparison_payload_sync,
                streamer=streamer,
                days=days,
                threshold=threshold,
            )
            return web.json_response(payload)
        except Exception:
            log.exception("Error in category comparison API")
            return analytics_internal_error_response()

    def _load_viewer_timeline_payload_sync(
        self,
        *,
        streamer: str,
        days: int,
    ) -> list[dict[str, Any]]:
        since_date = (datetime.now(UTC) - timedelta(days=days)).isoformat()
        streamer_login = streamer.lower()
        if days <= 7:
            timeline_sql = """
            SELECT
                TO_CHAR(
                    DATE_TRUNC('hour', ts_utc)
                    + FLOOR(EXTRACT(MINUTE FROM ts_utc) / 5) * INTERVAL '5 minutes',
                    'YYYY-MM-DD HH24:MI'
                ) as bucket,
                AVG(viewer_count) as avg_vc,
                MAX(viewer_count) as peak_vc,
                MIN(viewer_count) as min_vc,
                COUNT(*) as samples
            FROM twitch_stats_tracked
            WHERE ts_utc >= %s AND LOWER(streamer) = %s
            GROUP BY 1
            ORDER BY 1
            """
        elif days <= 30:
            timeline_sql = """
            SELECT
                TO_CHAR(
                    DATE_TRUNC('hour', ts_utc)
                    + CASE
                        WHEN EXTRACT(MINUTE FROM ts_utc) < 30 THEN INTERVAL '0 minutes'
                        ELSE INTERVAL '30 minutes'
                    END,
                    'YYYY-MM-DD HH24:MI'
                ) as bucket,
                AVG(viewer_count) as avg_vc,
                MAX(viewer_count) as peak_vc,
                MIN(viewer_count) as min_vc,
                COUNT(*) as samples
            FROM twitch_stats_tracked
            WHERE ts_utc >= %s AND LOWER(streamer) = %s
            GROUP BY 1
            ORDER BY 1
            """
        else:
            timeline_sql = """
            SELECT
                TO_CHAR(DATE_TRUNC('hour', ts_utc), 'YYYY-MM-DD HH24:MI') as bucket,
                AVG(viewer_count) as avg_vc,
                MAX(viewer_count) as peak_vc,
                MIN(viewer_count) as min_vc,
                COUNT(*) as samples
            FROM twitch_stats_tracked
            WHERE ts_utc >= %s AND LOWER(streamer) = %s
            GROUP BY 1
            ORDER BY 1
            """

        with storage.readonly_connection() as conn:
            rows = conn.execute(timeline_sql, (since_date, streamer_login)).fetchall()

        return [
            {
                "timestamp": row[0],
                "avgViewers": round(float(row[1]), 1) if row[1] else 0,
                "peakViewers": int(row[2]) if row[2] else 0,
                "minViewers": int(row[3]) if row[3] else 0,
                "samples": row[4] or 0,
            }
            for row in rows
        ]

    async def _api_v2_viewer_timeline(self, request: web.Request) -> web.Response:
        """Return bucketed viewer data from twitch_stats_tracked."""
        self._require_v2_auth(request)
        self._require_extended_plan(request)

        streamer = request.query.get("streamer", "").strip()
        days_raw = (request.query.get("days", "7") or "7").strip()
        try:
            days = int(days_raw)
        except (TypeError, ValueError):
            return web.json_response({"error": "days must be an integer"}, status=400)
        days = min(max(days, 1), 365)

        if not streamer:
            return web.json_response({"error": "Streamer required"}, status=400)

        try:
            payload = await asyncio.to_thread(
                self._load_viewer_timeline_payload_sync,
                streamer=streamer,
                days=days,
            )
            return web.json_response(payload)
        except Exception:
            log.exception("Error in viewer timeline API")
            return analytics_internal_error_response()

    def _load_category_leaderboard_payload_sync(
        self,
        *,
        streamer: str,
        days: int,
        limit: int,
        sort_mode: str,
        tier_filter: str | None,
        threshold: float | None,
    ) -> dict[str, Any]:
        lb_having = "HAVING AVG(c.viewer_count) <= %s" if threshold is not None else ""
        tier_ranges: dict[str, tuple[float, float]] = {
            "starter": (0, 15),
            "rising": (15, 50),
            "established": (50, 150),
            "featured": (150, 500),
            "top": (500, float("inf")),
        }
        since_date = (datetime.now(UTC) - timedelta(days=days)).isoformat()
        with storage.readonly_connection() as conn:
            lb_params: list[Any] = [since_date]
            if threshold is not None:
                lb_params.append(threshold)

            if sort_mode == "peak":
                leaderboard_sql = f"""
                SELECT
                    c.streamer,
                    AVG(c.viewer_count) as avg_vc,
                    MAX(c.viewer_count) as peak_vc,
                    BOOL_OR(c.is_partner) as is_partner
                FROM twitch_stats_category c
                WHERE c.ts_utc >= %s
                GROUP BY c.streamer
                {lb_having}
                ORDER BY peak_vc DESC
                """
            else:
                leaderboard_sql = f"""
                SELECT
                    c.streamer,
                    AVG(c.viewer_count) as avg_vc,
                    MAX(c.viewer_count) as peak_vc,
                    BOOL_OR(c.is_partner) as is_partner
                FROM twitch_stats_category c
                WHERE c.ts_utc >= %s
                GROUP BY c.streamer
                {lb_having}
                ORDER BY avg_vc DESC
                """

            rows = conn.execute(leaderboard_sql, tuple(lb_params)).fetchall()
            if tier_filter and tier_filter in tier_ranges:
                tier_min, tier_max = tier_ranges[tier_filter]
                rows = [
                    row for row in rows
                    if row[1] is not None and tier_min <= float(row[1]) < tier_max
                ]

            total_streamers = len(rows)
            your_tier = None
            if streamer:
                peer_group = self._get_peer_group_stats(conn, streamer, since_date)
                if peer_group:
                    your_tier = peer_group["tier"]

        leaderboard: list[dict[str, Any]] = []
        your_rank = None
        streamer_lower = streamer.lower() if streamer else ""
        your_entry = None

        for index, row in enumerate(rows):
            rank = index + 1
            entry = {
                "rank": rank,
                "streamer": row[0],
                "avgViewers": round(float(row[1]), 1) if row[1] else 0,
                "peakViewers": int(row[2]) if row[2] else 0,
                "isPartner": bool(row[3]),
                "isYou": row[0].lower() == streamer_lower,
            }
            if row[0].lower() == streamer_lower:
                your_rank = rank
                your_entry = entry
            if rank <= limit:
                leaderboard.append(entry)

        if your_entry and your_rank and your_rank > limit:
            leaderboard.append(your_entry)

        return {
            "leaderboard": leaderboard,
            "totalStreamers": total_streamers,
            "yourRank": your_rank,
            "yourTier": your_tier,
        }

    async def _api_v2_category_leaderboard(self, request: web.Request) -> web.Response:
        """Top-N streamers from twitch_stats_category."""
        self._require_v2_auth(request)
        self._require_extended_plan(request)

        streamer = request.query.get("streamer", "").strip()
        days_raw = (request.query.get("days", "30") or "30").strip()
        limit_raw = (request.query.get("limit", "25") or "25").strip()
        try:
            days = int(days_raw)
        except (TypeError, ValueError):
            return web.json_response({"error": "days must be an integer"}, status=400)
        try:
            limit = int(limit_raw)
        except (TypeError, ValueError):
            return web.json_response({"error": "limit must be an integer"}, status=400)
        days = min(max(days, 1), 365)
        limit = min(max(limit, 5), 100)
        sort_mode = request.query.get("sort", "avg")  # avg or peak
        exclude_external = request.query.get("exclude_external", "0") == "1"
        tier_filter = request.query.get("tier", "").strip().lower() or None  # starter/rising/established/featured/top
        threshold: float | None = EXTERNAL_REACH_AVG_THRESHOLD if exclude_external else None

        try:
            payload = await asyncio.to_thread(
                self._load_category_leaderboard_payload_sync,
                streamer=streamer,
                days=days,
                limit=limit,
                sort_mode=sort_mode,
                tier_filter=tier_filter,
                threshold=threshold,
            )
            return web.json_response(payload)
        except Exception:
            log.exception("Error in category leaderboard API")
            return analytics_internal_error_response()

    def _load_category_timings_payload_sync(
        self,
        *,
        days: int,
        source: str,
    ) -> dict[str, Any]:
        from statistics import median, quantiles

        cutoff = (datetime.now(UTC) - timedelta(days=days)).isoformat()
        with storage.readonly_connection() as conn:
            if source == "tracked":
                rows = conn.execute(
                    """
                    SELECT streamer,
                           EXTRACT(HOUR FROM ts_utc)::integer AS hour,
                           EXTRACT(DOW FROM ts_utc)::integer AS weekday,
                           viewer_count
                      FROM twitch_stats_tracked
                     WHERE ts_utc >= %s
                       AND viewer_count IS NOT NULL
                       AND viewer_count > 0
                    """,
                    (cutoff,),
                ).fetchall()
            else:
                rows = conn.execute(
                    """
                    SELECT streamer,
                           EXTRACT(HOUR FROM ts_utc)::integer AS hour,
                           EXTRACT(DOW FROM ts_utc)::integer AS weekday,
                           viewer_count
                      FROM twitch_stats_category
                     WHERE ts_utc >= %s
                       AND viewer_count IS NOT NULL
                       AND viewer_count > 0
                    """,
                    (cutoff,),
                ).fetchall()

        hour_data: dict[int, dict[str, list[float]]] = collections.defaultdict(
            lambda: collections.defaultdict(list)
        )
        weekday_data: dict[int, dict[str, list[float]]] = collections.defaultdict(
            lambda: collections.defaultdict(list)
        )

        for row in rows:
            streamer = row[0]
            hour = int(row[1])
            weekday = int(row[2])
            viewer_count = float(row[3])
            hour_data[hour][streamer].append(viewer_count)
            weekday_data[weekday][streamer].append(viewer_count)

        def _robust_stats(slot_data: dict[str, list[float]]) -> dict[str, Any]:
            if not slot_data:
                return {
                    "median": None,
                    "p25": None,
                    "p75": None,
                    "streamer_count": 0,
                    "sample_count": 0,
                }
            per_streamer = [median(values) for values in slot_data.values() if values]
            per_streamer.sort()
            count = len(per_streamer)
            sample_count = sum(len(values) for values in slot_data.values())
            if count == 0:
                return {
                    "median": None,
                    "p25": None,
                    "p75": None,
                    "streamer_count": 0,
                    "sample_count": 0,
                }
            med = median(per_streamer)
            if count >= 4:
                quartiles = quantiles(per_streamer, n=4)
                p25, p75 = quartiles[0], quartiles[2]
            elif count >= 2:
                p25 = per_streamer[0]
                p75 = per_streamer[-1]
            else:
                p25 = p75 = per_streamer[0]
            return {
                "median": round(med, 1),
                "p25": round(p25, 1),
                "p75": round(p75, 1),
                "streamer_count": count,
                "sample_count": sample_count,
            }

        weekday_names = ["So", "Mo", "Di", "Mi", "Do", "Fr", "Sa"]
        hourly = []
        for hour in range(24):
            slot = _robust_stats(hour_data.get(hour, {}))
            slot["hour"] = hour
            hourly.append(slot)

        weekly = []
        for weekday in [1, 2, 3, 4, 5, 6, 0]:
            slot = _robust_stats(weekday_data.get(weekday, {}))
            slot["weekday"] = weekday
            slot["label"] = weekday_names[weekday]
            weekly.append(slot)

        return {
            "hourly": hourly,
            "weekly": weekly,
            "total_streamers": len({row[0] for row in rows}),
            "window_days": days,
            "method": "median_of_medians",
        }

    async def _api_v2_category_timings(self, request: web.Request) -> web.Response:
        """
        Outlier-resistente Stunden- und Wochentags-Analyse fur die gesamte Kategorie.
        Methode: Median der Streamer-Mediane (zweistufig) + P25/P75 Konfidenzband.
        Einzelne Streamer mit extrem hohen Viewerzahlen verzerren so das Ergebnis nicht.
        """
        self._require_v2_auth(request)
        self._require_extended_plan(request)
        try:
            days = _parse_bounded_query_int(
                request,
                name="days",
                default=30,
                minimum=7,
                maximum=90,
            )
        except ValueError as exc:
            return web.json_response({"error": str(exc)}, status=400)
        source = request.query.get("source", "category")  # 'category' | 'tracked'

        try:
            payload = await asyncio.to_thread(
                self._load_category_timings_payload_sync,
                days=days,
                source=source,
            )
            return web.json_response(payload)
        except Exception:
            log.exception("category-timings query failed")
            return analytics_internal_error_response()

    @staticmethod
    def _float_or_none(value: Any, *, digits: int = 1) -> float | None:
        if value is None:
            return None
        try:
            return round(float(value), digits)
        except (TypeError, ValueError):
            return None

    @staticmethod
    def _int_or_none(value: Any) -> int | None:
        if value is None:
            return None
        try:
            return int(value)
        except (TypeError, ValueError):
            return None

    def _load_category_activity_series_payload_sync(self, *, days: int) -> dict[str, Any]:
        cutoff = (datetime.now(UTC) - timedelta(days=days)).isoformat()
        with storage.readonly_connection() as conn:
            hourly_rows = conn.execute(
                """
                WITH source_rows AS (
                    SELECT 'tracked' AS source_key, viewer_count, ts_utc
                      FROM twitch_stats_tracked
                    UNION ALL
                    SELECT 'category' AS source_key, viewer_count, ts_utc
                      FROM twitch_stats_category
                )
                SELECT source_key,
                       EXTRACT(HOUR FROM ts_utc)::integer AS hour,
                       AVG(viewer_count) AS avg_viewers,
                       MAX(viewer_count) AS max_viewers,
                       COUNT(*) AS samples
                  FROM source_rows
                 WHERE ts_utc >= %s
                 GROUP BY 1, 2
                 ORDER BY 1, 2
                """,
                (cutoff,),
            ).fetchall()

            weekday_rows = conn.execute(
                """
                WITH source_rows AS (
                    SELECT 'tracked' AS source_key, viewer_count, ts_utc
                      FROM twitch_stats_tracked
                    UNION ALL
                    SELECT 'category' AS source_key, viewer_count, ts_utc
                      FROM twitch_stats_category
                )
                SELECT source_key,
                       EXTRACT(DOW FROM ts_utc)::integer AS weekday,
                       AVG(viewer_count) AS avg_viewers,
                       MAX(viewer_count) AS max_viewers,
                       COUNT(*) AS samples
                  FROM source_rows
                 WHERE ts_utc >= %s
                 GROUP BY 1, 2
                 ORDER BY 1, 2
                """,
                (cutoff,),
            ).fetchall()

        hourly_map: dict[str, dict[int, dict[str, Any]]] = {"category": {}, "tracked": {}}
        weekday_map: dict[str, dict[int, dict[str, Any]]] = {"category": {}, "tracked": {}}

        for row in hourly_rows:
            source_key = str(row[0] or "").strip().lower()
            hour = self._int_or_none(row[1])
            if source_key not in hourly_map or hour is None:
                continue
            hourly_map[source_key][hour] = {
                "avg": self._float_or_none(row[2]),
                "peak": self._int_or_none(row[3]),
                "samples": self._int_or_none(row[4]) or 0,
            }

        for row in weekday_rows:
            source_key = str(row[0] or "").strip().lower()
            weekday = self._int_or_none(row[1])
            if source_key not in weekday_map or weekday is None:
                continue
            weekday_map[source_key][weekday] = {
                "avg": self._float_or_none(row[2]),
                "peak": self._int_or_none(row[3]),
                "samples": self._int_or_none(row[4]) or 0,
            }

        hourly: list[dict[str, Any]] = []
        for hour in range(24):
            category_point = hourly_map["category"].get(hour, {})
            tracked_point = hourly_map["tracked"].get(hour, {})
            hourly.append(
                {
                    "hour": hour,
                    "label": f"{hour:02d}:00",
                    "categoryAvg": category_point.get("avg"),
                    "trackedAvg": tracked_point.get("avg"),
                    "categoryPeak": category_point.get("peak"),
                    "trackedPeak": tracked_point.get("peak"),
                    "categorySamples": int(category_point.get("samples") or 0),
                    "trackedSamples": int(tracked_point.get("samples") or 0),
                }
            )

        weekday_labels = {
            0: "Sonntag",
            1: "Montag",
            2: "Dienstag",
            3: "Mittwoch",
            4: "Donnerstag",
            5: "Freitag",
            6: "Samstag",
        }
        weekly: list[dict[str, Any]] = []
        for weekday in [1, 2, 3, 4, 5, 6, 0]:
            category_point = weekday_map["category"].get(weekday, {})
            tracked_point = weekday_map["tracked"].get(weekday, {})
            weekly.append(
                {
                    "weekday": weekday,
                    "label": weekday_labels.get(weekday, str(weekday)),
                    "categoryAvg": category_point.get("avg"),
                    "trackedAvg": tracked_point.get("avg"),
                    "categoryPeak": category_point.get("peak"),
                    "trackedPeak": tracked_point.get("peak"),
                    "categorySamples": int(category_point.get("samples") or 0),
                    "trackedSamples": int(tracked_point.get("samples") or 0),
                }
            )

        return {
            "hourly": hourly,
            "weekly": weekly,
            "windowDays": days,
            "source": "legacy_stats_chart",
        }

    async def _api_v2_category_activity_series(self, request: web.Request) -> web.Response:
        """
        Legacy stats-style comparison series for category vs tracked.
        Provides hourly and weekday rows with average and peak values.
        """
        self._require_v2_auth(request)
        self._require_extended_plan(request)
        try:
            days = _parse_bounded_query_int(
                request,
                name="days",
                default=30,
                minimum=7,
                maximum=365,
            )
        except ValueError as exc:
            return web.json_response({"error": str(exc)}, status=400)
        try:
            payload = await asyncio.to_thread(
                self._load_category_activity_series_payload_sync,
                days=days,
            )
            return web.json_response(payload)
        except Exception:
            log.exception("category-activity-series query failed")
            return analytics_internal_error_response()

    def _load_retention_curve_payload_sync(
        self,
        *,
        streamer: str,
        days: int,
    ) -> dict[str, Any]:
        since_date = (datetime.now(UTC) - timedelta(days=days)).isoformat()
        with storage.readonly_connection() as conn:
            session_rows = conn.execute(
                """
                SELECT id, peak_viewers
                FROM twitch_stream_sessions
                WHERE LOWER(streamer_login) = %s AND started_at >= %s AND ended_at IS NOT NULL
                ORDER BY started_at DESC
                LIMIT 50
                """,
                (streamer, since_date),
            ).fetchall()

            if not session_rows:
                return {"retention_curve": [], "drop_events": [], "sessions_used": 0}

            session_ids = [int(row[0]) for row in session_rows]
            peak_by_session = {int(row[0]): int(row[1] or 1) for row in session_rows}
            placeholders = ",".join(["%s"] * len(session_ids))
            viewer_rows = conn.execute(  # nosemgrep: python.sqlalchemy.security.sqlalchemy-execute-raw-query.sqlalchemy-execute-raw-query
                f"""
                SELECT session_id, minutes_from_start, viewer_count
                FROM twitch_session_viewers
                WHERE session_id IN ({placeholders})
                ORDER BY session_id, minutes_from_start
                """,
                tuple(session_ids),
            ).fetchall()

            minute_data: dict[int, list[float]] = collections.defaultdict(list)
            for viewer_row in viewer_rows:
                session_id = int(viewer_row[0])
                minute = int(viewer_row[1] or 0)
                count = int(viewer_row[2] or 0)
                peak = peak_by_session.get(session_id, 1)
                if peak > 0:
                    minute_data[minute].append(count / peak)

            if not minute_data:
                return {"retention_curve": [], "drop_events": [], "sessions_used": 0}

            def _percentile(values: list[float], pct: float) -> float:
                if not values:
                    return 0.0
                sorted_values = sorted(values)
                index = (len(sorted_values) - 1) * pct
                lower = int(index)
                upper = min(lower + 1, len(sorted_values) - 1)
                return sorted_values[lower] + (sorted_values[upper] - sorted_values[lower]) * (index - lower)

            max_minute = min(max(minute_data.keys()), 180)
            curve = []
            for minute in sorted(candidate for candidate in minute_data if candidate <= max_minute):
                values = minute_data[minute]
                if not values:
                    continue
                median_retention = _percentile(values, 0.5)
                curve.append(
                    {
                        "minute": minute,
                        "median_retention": round(median_retention, 3),
                        "p25": round(_percentile(values, 0.25), 3),
                        "p75": round(_percentile(values, 0.75), 3),
                        "sample_count": len(values),
                    }
                )

            ad_times: set[int] = set()
            try:
                ad_rows = conn.execute(  # nosemgrep: python.sqlalchemy.security.sqlalchemy-execute-raw-query.sqlalchemy-execute-raw-query
                    f"""
                    SELECT a.started_at, s.started_at AS session_start
                    FROM twitch_ad_break_events a
                    JOIN twitch_stream_sessions s ON s.id = a.session_id
                    WHERE a.session_id IN ({placeholders})
                    """,
                    tuple(session_ids),
                ).fetchall()
                for ad_row in ad_rows:
                    try:
                        ad_dt = datetime.fromisoformat(str(ad_row[0]).replace("Z", "+00:00"))
                        session_dt = datetime.fromisoformat(str(ad_row[1]).replace("Z", "+00:00"))
                        ad_times.add(int((ad_dt - session_dt).total_seconds() / 60.0))
                    except Exception:
                        pass
            except Exception:
                pass

        drop_events = []
        for index in range(1, len(curve)):
            previous_retention = curve[index - 1]["median_retention"]
            current_retention = curve[index]["median_retention"]
            if previous_retention <= 0:
                continue
            delta = (current_retention - previous_retention) / previous_retention
            if delta < -0.10:
                minute = curve[index]["minute"]
                drop_events.append(
                    {
                        "minute": minute,
                        "drop_pct": round(abs(delta) * 100, 1),
                        "type": "ad_break" if minute in ad_times else "unknown",
                    }
                )

        avg_watch = None
        for point in curve:
            if point["median_retention"] < 0.5:
                avg_watch = point["minute"]
                break

        return {
            "retention_curve": curve,
            "drop_events": drop_events,
            "avg_watch_duration_min": avg_watch,
            "sessions_used": len(session_ids),
            "window_days": days,
        }

    async def _api_v2_retention_curve(self, request: web.Request) -> web.Response:
        """Aggregated viewer retention curve from twitch_session_viewers."""
        self._require_v2_auth(request)
        self._require_extended_plan(request)

        streamer = request.query.get("streamer", "").strip().lower()
        try:
            days = _parse_bounded_query_int(
                request,
                name="days",
                default=30,
                minimum=7,
                maximum=365,
            )
        except ValueError as exc:
            return web.json_response({"error": str(exc)}, status=400)

        if not streamer:
            return web.json_response({"error": "Streamer required"}, status=400)

        try:
            payload = await asyncio.to_thread(
                self._load_retention_curve_payload_sync,
                streamer=streamer,
                days=days,
            )
            return web.json_response(payload)
        except Exception:
            log.exception("Error in retention curve API")
            return analytics_internal_error_response()
