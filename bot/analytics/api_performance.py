"""
Analytics API v2 - Performance Mixin.

Performance metrics: heatmaps, periodic stats, tags, title performance,
rankings, category comparison, category timings, category activity series,
viewer timeline, category leaderboard.
"""

from __future__ import annotations

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


class _AnalyticsPerformanceMixin:
    """Mixin providing performance metrics endpoints."""

    async def _api_v2_hourly_heatmap(self, request: web.Request) -> web.Response:
        """Get hourly heatmap data."""
        self._require_v2_auth(request)

        streamer = request.query.get("streamer", "").strip() or None
        days = min(max(int(request.query.get("days", "30")), 7), 365)

        try:
            with storage.readonly_connection() as conn:
                since_date = (datetime.now(UTC) - timedelta(days=days)).isoformat()
                streamer_login = streamer.lower() if streamer else None
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

                data = [
                    {
                        "weekday": r[0],
                        "hour": r[1],
                        "streamCount": r[2],
                        "avgViewers": float(r[3]) if r[3] else 0,
                        "avgPeak": float(r[4]) if r[4] else 0,
                    }
                    for r in rows
                ]

                return web.json_response(data)
        except Exception as exc:
            log.exception("Error in hourly heatmap API")
            return analytics_internal_error_response()

    async def _api_v2_calendar_heatmap(self, request: web.Request) -> web.Response:
        """Get calendar heatmap data."""
        self._require_v2_auth(request)

        streamer = request.query.get("streamer", "").strip() or None
        days = min(max(int(request.query.get("days", "365")), 30), 365)

        try:
            with storage.readonly_connection() as conn:
                since_date = (datetime.now(UTC) - timedelta(days=days)).isoformat()
                streamer_login = streamer.lower() if streamer else None
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

                data = [
                    {
                        "date": r[0].isoformat() if hasattr(r[0], "isoformat") else str(r[0]),
                        "streamCount": r[1],
                        "hoursWatched": float(r[2]) if r[2] else 0,
                        "value": float(r[2]) if r[2] else 0,
                    }
                    for r in rows
                ]

                return web.json_response(data)
        except Exception as exc:
            log.exception("Error in calendar heatmap API")
            return analytics_internal_error_response()

    async def _api_v2_monthly_stats(self, request: web.Request) -> web.Response:
        """Get monthly aggregated stats."""
        self._require_v2_auth(request)

        streamer = request.query.get("streamer", "").strip() or None
        months = min(max(int(request.query.get("months", "12")), 1), 24)

        try:
            with storage.readonly_connection() as conn:
                since_date = (datetime.now(UTC) - timedelta(days=round(months * 30.44))).isoformat()
                streamer_login = streamer.lower() if streamer else None
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

                month_names = [
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
                data = [
                    {
                        "year": r[0],
                        "month": r[1],
                        "monthLabel": month_names[r[1]] if r[1] else "",
                        "totalHoursWatched": float(r[2]) if r[2] else 0,
                        "totalAirtime": float(r[3]) if r[3] else 0,
                        "avgViewers": float(r[4]) if r[4] else 0,
                        "peakViewers": int(r[5]) if r[5] else 0,
                        "followerDelta": int(r[6]) if r[6] else 0,
                        "totalChatterSessions": int(r[7]) if r[7] else 0,
                        "streamCount": r[8],
                    }
                    for r in rows
                ]

                return web.json_response(data)
        except Exception as exc:
            log.exception("Error in monthly stats API")
            return analytics_internal_error_response()

    async def _api_v2_weekly_stats(self, request: web.Request) -> web.Response:
        """Get weekday analysis stats."""
        self._require_v2_auth(request)

        streamer = request.query.get("streamer", "").strip() or None
        days = min(max(int(request.query.get("days", "30")), 7), 365)

        try:
            with storage.readonly_connection() as conn:
                since_date = (datetime.now(UTC) - timedelta(days=days)).isoformat()
                streamer_login = streamer.lower() if streamer else None
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

                weekday_names = ["So", "Mo", "Di", "Mi", "Do", "Fr", "Sa"]
                data = [
                    {
                        "weekday": r[0],
                        "weekdayLabel": weekday_names[r[0]] if r[0] is not None else "",
                        "streamCount": r[1],
                        "avgHours": float(r[2]) if r[2] else 0,
                        "avgViewers": float(r[3]) if r[3] else 0,
                        "avgPeak": float(r[4]) if r[4] else 0,
                        "totalFollowers": int(r[5]) if r[5] else 0,
                    }
                    for r in rows
                ]

                return web.json_response(data)
        except Exception as exc:
            log.exception("Error in weekly stats API")
            return analytics_internal_error_response()

    async def _api_v2_tag_analysis(self, request: web.Request) -> web.Response:
        """Get tag performance analysis."""
        self._require_v2_auth(request)

        try:
            # Tags are stored as JSON in the tags column
            # This is a simplified version - full implementation would parse JSON
            return web.json_response([])
        except Exception as exc:
            log.exception("Error in tag analysis API")
            return analytics_internal_error_response()

    async def _api_v2_tag_analysis_extended(self, request: web.Request) -> web.Response:
        """Get extended tag performance with trends."""
        self._require_v2_auth(request)
        self._require_extended_plan(request)

        streamer = request.query.get("streamer", "").strip() or None
        days = min(max(int(request.query.get("days", "30")), 7), 365)
        limit = min(max(int(request.query.get("limit", "20")), 5), 50)

        try:
            with storage.readonly_connection() as conn:
                since_date = (datetime.now(UTC) - timedelta(days=days)).isoformat()
                streamer_login = streamer.lower() if streamer else None

                # Hole Sessions mit Tags
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

                    # Jede Session zahlt pro Tag hochstens einmal
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

                # Filter: nur Tags mit ausreichend Samples
                filtered = {tag: data for tag, data in tag_stats.items() if data["samples"] >= 3}

                sorted_tags = sorted(
                    filtered.items(),
                    key=lambda x: (_median(x[1]["viewers"]), x[1]["samples"]),
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

                return web.json_response(result)
        except Exception as exc:
            log.exception("Error in tag analysis extended API")
            return analytics_internal_error_response()

    async def _api_v2_title_performance(self, request: web.Request) -> web.Response:
        """Get stream title performance analysis."""
        self._require_v2_auth(request)
        self._require_extended_plan(request)

        streamer = request.query.get("streamer", "").strip() or None
        days = min(max(int(request.query.get("days", "30")), 7), 365)
        limit = min(max(int(request.query.get("limit", "20")), 5), 50)

        if not streamer:
            return web.json_response({"error": "Streamer required"}, status=400)

        try:
            with storage.readonly_connection() as conn:
                since_date = (datetime.now(UTC) - timedelta(days=days)).isoformat()

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
                    (since_date, streamer.lower(), limit),
                ).fetchall()

                def extract_keywords(title: str) -> list[str]:
                    """Extract meaningful keywords from title."""
                    import re

                    # Remove common words and punctuation
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
                    keywords = [w.capitalize() for w in words if w not in stop_words]
                    return keywords[:5]  # Max 5 keywords

                result = [
                    {
                        "title": row[0] or "",
                        "usageCount": row[1],
                        "avgViewers": round(float(row[2]), 1) if row[2] else 0,
                        "avgRetention10m": round(float(row[3]) * 100, 1) if row[3] else 0,
                        "avgFollowerGain": round(float(row[4]), 1) if row[4] else 0,
                        "peakViewers": int(row[5]) if row[5] else 0,
                        "keywords": extract_keywords(row[0] or ""),
                    }
                    for row in rows
                ]

                return web.json_response(result)
        except Exception as exc:
            log.exception("Error in title performance API")
            return analytics_internal_error_response()

    async def _api_v2_rankings(self, request: web.Request) -> web.Response:
        """Get streamer rankings."""
        self._require_v2_auth(request)

        metric = request.query.get("metric", "viewers")
        days = min(max(int(request.query.get("days", "30")), 7), 365)
        limit = min(max(int(request.query.get("limit", "20")), 5), 50)
        exclude_external = request.query.get("exclude_external", "0") == "1"
        threshold = EXTERNAL_REACH_AVG_THRESHOLD if exclude_external else None

        # Optionally append a second HAVING condition to filter external-reach streamers.
        having_ext = " AND AVG(s.avg_viewers) <= %s" if threshold is not None else ""

        try:
            with storage.readonly_connection() as conn:
                since_date = (datetime.now(UTC) - timedelta(days=days)).isoformat()

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

                data = [
                    {
                        "rank": i + 1,
                        "login": r[0],
                        "value": (float(r[1]) * 100 if metric == "retention" else float(r[1]))
                        if r[1]
                        else 0,
                        "trend": "same",
                        "trendValue": 0,
                    }
                    for i, r in enumerate(rows)
                ]

                return web.json_response(data)
        except Exception as exc:
            log.exception("Error in rankings API")
            return analytics_internal_error_response()

    async def _api_v2_category_comparison(self, request: web.Request) -> web.Response:
        """Compare streamer to category averages."""
        self._require_v2_auth(request)

        streamer = request.query.get("streamer", "").strip()
        days = min(max(int(request.query.get("days", "30")), 7), 365)
        exclude_external = request.query.get("exclude_external", "0") == "1"
        threshold: float | None = EXTERNAL_REACH_AVG_THRESHOLD if exclude_external else None

        if not streamer:
            return web.json_response({"error": "Streamer required"}, status=400)

        try:
            with storage.readonly_connection() as conn:
                since_date = (datetime.now(UTC) - timedelta(days=days)).isoformat()

                # Your stats from stats_tracked (higher accuracy for tracked streamers)
                your_tracked = conn.execute(
                    """
                    SELECT AVG(viewer_count), MAX(viewer_count)
                    FROM twitch_stats_tracked
                    WHERE ts_utc >= %s AND LOWER(streamer) = %s
                """,
                    (since_date, streamer.lower()),
                ).fetchone()

                # Fallback to session data
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
                    (since_date, streamer.lower()),
                ).fetchone()

                # Use tracked data if available, else session data
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

                # Category stats from stats_category (per-streamer aggregates).
                # When exclude_external=1, streamers above EXTERNAL_REACH_AVG_THRESHOLD are
                # excluded so that percentiles and averages reflect the organic community.
                cat_data = self._get_category_percentiles(conn, since_date, threshold)
                sorted_avgs = cat_data["sorted_avgs"]
                category_total = cat_data["total"]

                # Category averages
                cat_avg_viewers = sum(sorted_avgs) / len(sorted_avgs) if sorted_avgs else 0

                # Peak viewers per streamer from category (with optional threshold filter)
                cat_peak_having = "HAVING AVG(viewer_count) <= %s" if threshold is not None else ""
                cat_peak_params: list[Any] = [since_date]
                if threshold is not None:
                    cat_peak_params.append(threshold)
                cat_peak = conn.execute(
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

                # Category-wide retention and chat health from session data.
                # When filtering, exclude high-avg-viewer streamers via NOT IN subquery.
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
                cat_avg_ret = (
                    float(cat_session_avgs[0]) * 100
                    if cat_session_avgs and cat_session_avgs[0]
                    else 0
                )
                cat_avg_chat = (
                    float(cat_session_avgs[1]) if cat_session_avgs and cat_session_avgs[1] else 0
                )

                # Per-streamer retention and chat for percentile ranking
                per_ret_having = "HAVING AVG(s.avg_viewers) <= %s" if threshold is not None else ""
                per_ret_params: list[Any] = [since_date]
                if threshold is not None:
                    per_ret_params.append(threshold)
                per_streamer_ret = conn.execute(
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
                ret_sorted = [float(r[0]) * 100 for r in per_streamer_ret if r[0] is not None]

                per_chat_having = "HAVING AVG(s.avg_viewers) <= %s" if threshold is not None else ""
                per_chat_params: list[Any] = [since_date]
                if threshold is not None:
                    per_chat_params.append(threshold)
                per_streamer_chat = conn.execute(
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
                chat_sorted = [float(r[0]) for r in per_streamer_chat if r[0] is not None]

                # Percentiles for avgViewers
                avg_percentile = (
                    int(self._percentile_of(sorted_avgs, your_avg) * 100) if sorted_avgs else 0
                )

                # Percentile for peakViewers
                peak_having = "HAVING AVG(viewer_count) <= %s" if threshold is not None else ""
                peak_params: list[Any] = [since_date]
                if threshold is not None:
                    peak_params.append(threshold)
                peak_avgs = conn.execute(
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
                peak_sorted = [float(r[0]) for r in peak_avgs] if peak_avgs else []
                peak_percentile = (
                    int(self._percentile_of(peak_sorted, your_peak) * 100) if peak_sorted else 50
                )

                # Percentiles for retention and chat
                ret_percentile = (
                    int(self._percentile_of(ret_sorted, your_ret) * 100) if ret_sorted else 50
                )
                chat_percentile = (
                    int(self._percentile_of(chat_sorted, your_chat) * 100) if chat_sorted else 50
                )

                # Category rank (1 = best)
                category_rank = (
                    category_total - int(avg_percentile / 100 * category_total)
                    if category_total
                    else 0
                )

                return web.json_response(
                    {
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
                    }
                )
        except Exception as exc:
            log.exception("Error in category comparison API")
            return analytics_internal_error_response()

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
            with storage.readonly_connection() as conn:
                since_date = (datetime.now(UTC) - timedelta(days=days)).isoformat()

                # Determine bucket size based on range
                if days <= 7:
                    bucket_minutes = 5
                elif days <= 30:
                    bucket_minutes = 30
                else:
                    bucket_minutes = 60

                # Use static SQL variants per bucket size to avoid dynamic query construction.
                if bucket_minutes == 5:
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
                elif bucket_minutes == 30:
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

                rows = conn.execute(timeline_sql, (since_date, streamer.lower())).fetchall()

                data = [
                    {
                        "timestamp": r[0],
                        "avgViewers": round(float(r[1]), 1) if r[1] else 0,
                        "peakViewers": int(r[2]) if r[2] else 0,
                        "minViewers": int(r[3]) if r[3] else 0,
                        "samples": r[4] or 0,
                    }
                    for r in rows
                ]

                return web.json_response(data)
        except Exception as exc:
            log.exception("Error in viewer timeline API")
            return analytics_internal_error_response()

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
        threshold: float | None = EXTERNAL_REACH_AVG_THRESHOLD if exclude_external else None

        # HAVING clause appended when external-reach filter is active
        lb_having = "HAVING AVG(c.viewer_count) <= %s" if threshold is not None else ""

        try:
            with storage.readonly_connection() as conn:
                since_date = (datetime.now(UTC) - timedelta(days=days)).isoformat()
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

                total_streamers = len(rows)

                # Build ranked list
                leaderboard = []
                your_rank = None
                streamer_lower = streamer.lower() if streamer else ""
                your_entry = None

                for i, r in enumerate(rows):
                    rank = i + 1
                    entry = {
                        "rank": rank,
                        "streamer": r[0],
                        "avgViewers": round(float(r[1]), 1) if r[1] else 0,
                        "peakViewers": int(r[2]) if r[2] else 0,
                        "isPartner": bool(r[3]),
                        "isYou": r[0].lower() == streamer_lower,
                    }
                    if r[0].lower() == streamer_lower:
                        your_rank = rank
                        your_entry = entry
                    if rank <= limit:
                        leaderboard.append(entry)

                # If streamer is not in top-N, append them
                if your_entry and your_rank and your_rank > limit:
                    leaderboard.append(your_entry)

                return web.json_response(
                    {
                        "leaderboard": leaderboard,
                        "totalStreamers": total_streamers,
                        "yourRank": your_rank,
                    }
                )
        except Exception as exc:
            log.exception("Error in category leaderboard API")
            return analytics_internal_error_response()

    async def _api_v2_category_timings(self, request: web.Request) -> web.Response:
        """
        Outlier-resistente Stunden- und Wochentags-Analyse fur die gesamte Kategorie.
        Methode: Median der Streamer-Mediane (zweistufig) + P25/P75 Konfidenzband.
        Einzelne Streamer mit extrem hohen Viewerzahlen verzerren so das Ergebnis nicht.
        """
        self._require_v2_auth(request)
        self._require_extended_plan(request)
        days = min(max(int(request.query.get("days", "30")), 7), 90)
        source = request.query.get("source", "category")  # 'category' | 'tracked'

        from datetime import datetime, timedelta
        from statistics import median, quantiles

        cutoff = (datetime.now(UTC) - timedelta(days=days)).isoformat()
        try:
            with storage.readonly_connection() as c:
                if source == "tracked":
                    rows = c.execute(
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
                    rows = c.execute(
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
        except Exception as exc:
            log.exception("category-timings query failed")
            return analytics_internal_error_response()

        # --- Stunde: Streamer -> Stunde -> Liste Viewerzahlen ---
        # hour_data[hour][streamer] = [viewer_counts...]
        hour_data: dict = collections.defaultdict(lambda: collections.defaultdict(list))
        weekday_data: dict = collections.defaultdict(lambda: collections.defaultdict(list))

        for row in rows:
            streamer = row[0]
            hour = int(row[1])
            wd = int(row[2])
            vc = float(row[3])
            hour_data[hour][streamer].append(vc)
            weekday_data[wd][streamer].append(vc)

        def _robust_stats(slot_data: dict) -> dict:
            """Fur einen Slot (Stunde/Wochentag): Median der Streamer-Mediane + P25/P75."""
            if not slot_data:
                return {
                    "median": None,
                    "p25": None,
                    "p75": None,
                    "streamer_count": 0,
                    "sample_count": 0,
                }
            # Schritt 1: pro Streamer einen Median berechnen
            per_streamer = [median(vals) for vals in slot_data.values() if vals]
            per_streamer.sort()
            n = len(per_streamer)
            sample_count = sum(len(v) for v in slot_data.values())
            if n == 0:
                return {
                    "median": None,
                    "p25": None,
                    "p75": None,
                    "streamer_count": 0,
                    "sample_count": 0,
                }
            # Schritt 2: Median der Mediane
            med = median(per_streamer)
            # P25 / P75
            if n >= 4:
                qs = quantiles(per_streamer, n=4)
                p25, p75 = qs[0], qs[2]
            elif n >= 2:
                p25 = per_streamer[0]
                p75 = per_streamer[-1]
            else:
                p25 = p75 = per_streamer[0]
            return {
                "median": round(med, 1),
                "p25": round(p25, 1),
                "p75": round(p75, 1),
                "streamer_count": n,
                "sample_count": sample_count,
            }

        weekday_names = ["So", "Mo", "Di", "Mi", "Do", "Fr", "Sa"]

        hourly = []
        for h in range(24):
            s = _robust_stats(hour_data.get(h, {}))
            s["hour"] = h
            hourly.append(s)

        weekday_order = [1, 2, 3, 4, 5, 6, 0]  # Mo-So
        weekly = []
        for wd in weekday_order:
            s = _robust_stats(weekday_data.get(wd, {}))
            s["weekday"] = wd
            s["label"] = weekday_names[wd]
            weekly.append(s)

        total_streamers = len({row[0] for row in rows})

        return web.json_response(
            {
                "hourly": hourly,
                "weekly": weekly,
                "total_streamers": total_streamers,
                "window_days": days,
                "method": "median_of_medians",
            }
        )

    async def _api_v2_category_activity_series(self, request: web.Request) -> web.Response:
        """
        Legacy stats-style comparison series for category vs tracked.
        Provides hourly and weekday rows with average and peak values.
        """
        self._require_v2_auth(request)
        self._require_extended_plan(request)
        days = min(max(int(request.query.get("days", "30")), 7), 365)
        cutoff = (datetime.now(UTC) - timedelta(days=days)).isoformat()

        hourly_rows: list[Any] = []
        weekday_rows: list[Any] = []

        try:
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
        except Exception as exc:
            log.exception("category-activity-series query failed")
            return analytics_internal_error_response()

        def _float_or_none(value: Any, *, digits: int = 1) -> float | None:
            if value is None:
                return None
            try:
                return round(float(value), digits)
            except (TypeError, ValueError):
                return None

        def _int_or_none(value: Any) -> int | None:
            if value is None:
                return None
            try:
                return int(value)
            except (TypeError, ValueError):
                return None

        hourly_map: dict[str, dict[int, dict[str, Any]]] = {
            "category": {},
            "tracked": {},
        }
        weekday_map: dict[str, dict[int, dict[str, Any]]] = {
            "category": {},
            "tracked": {},
        }

        for row in hourly_rows:
            source_key = str(row[0] or "").strip().lower()
            hour = _int_or_none(row[1])
            if source_key not in hourly_map or hour is None:
                continue
            hourly_map[source_key][hour] = {
                "avg": _float_or_none(row[2]),
                "peak": _int_or_none(row[3]),
                "samples": _int_or_none(row[4]) or 0,
            }

        for row in weekday_rows:
            source_key = str(row[0] or "").strip().lower()
            weekday = _int_or_none(row[1])
            if source_key not in weekday_map or weekday is None:
                continue
            weekday_map[source_key][weekday] = {
                "avg": _float_or_none(row[2]),
                "peak": _int_or_none(row[3]),
                "samples": _int_or_none(row[4]) or 0,
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
        weekday_order = [1, 2, 3, 4, 5, 6, 0]  # Mo-So

        weekly: list[dict[str, Any]] = []
        for weekday in weekday_order:
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

        return web.json_response(
            {
                "hourly": hourly,
                "weekly": weekly,
                "windowDays": days,
                "source": "legacy_stats_chart",
            }
        )

    async def _api_v2_retention_curve(self, request: web.Request) -> web.Response:
        """Aggregated viewer retention curve from twitch_session_viewers."""
        self._require_v2_auth(request)
        self._require_extended_plan(request)

        streamer = request.query.get("streamer", "").strip().lower()
        days = min(max(int(request.query.get("days", "30")), 7), 365)

        if not streamer:
            return web.json_response({"error": "Streamer required"}, status=400)

        try:
            with storage.readonly_connection() as conn:
                since_date = (datetime.now(UTC) - timedelta(days=days)).isoformat()

                # Get all session IDs for this streamer in range
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
                    return web.json_response({"retention_curve": [], "drop_events": [], "sessions_used": 0})

                session_ids = [int(r[0]) for r in session_rows]
                peak_by_session = {int(r[0]): int(r[1] or 1) for r in session_rows}

                # Fetch all viewer timeline data for these sessions
                placeholders = ",".join(["%s"] * len(session_ids))
                viewer_rows = conn.execute(
                    f"""
                    SELECT session_id, minutes_from_start, viewer_count
                    FROM twitch_session_viewers
                    WHERE session_id IN ({placeholders})
                    ORDER BY session_id, minutes_from_start
                    """,
                    tuple(session_ids),
                ).fetchall()

                # Group by minute, collect retention values across sessions
                from collections import defaultdict
                minute_data: dict[int, list[float]] = defaultdict(list)
                for vr in viewer_rows:
                    sid = int(vr[0])
                    minute = int(vr[1] or 0)
                    count = int(vr[2] or 0)
                    peak = peak_by_session.get(sid, 1)
                    if peak > 0:
                        minute_data[minute].append(count / peak)

                if not minute_data:
                    return web.json_response({"retention_curve": [], "drop_events": [], "sessions_used": 0})

                # Build curve: median retention + P25/P75 per minute
                def _percentile(values: list, pct: float) -> float:
                    if not values:
                        return 0.0
                    s = sorted(values)
                    idx = (len(s) - 1) * pct
                    lo, hi = int(idx), min(int(idx) + 1, len(s) - 1)
                    return s[lo] + (s[hi] - s[lo]) * (idx - lo)

                max_minute = min(max(minute_data.keys()), 180)  # cap at 3h
                curve = []
                for minute in sorted(m for m in minute_data if m <= max_minute):
                    vals = minute_data[minute]
                    if len(vals) < 1:
                        continue
                    median_ret = _percentile(vals, 0.5)
                    curve.append({
                        "minute": minute,
                        "median_retention": round(median_ret, 3),
                        "p25": round(_percentile(vals, 0.25), 3),
                        "p75": round(_percentile(vals, 0.75), 3),
                        "sample_count": len(vals),
                    })

                # Detect drop events: >10% relative drop between consecutive minutes
                # Annotate with ad_break type if possible
                ad_times: set[int] = set()
                try:
                    ad_rows_q = conn.execute(
                        f"""
                        SELECT a.started_at, s.started_at AS session_start
                        FROM twitch_ad_break_events a
                        JOIN twitch_stream_sessions s ON s.id = a.session_id
                        WHERE a.session_id IN ({placeholders})
                        """,
                        tuple(session_ids),
                    ).fetchall()
                    for ar in ad_rows_q:
                        try:
                            ad_dt = datetime.fromisoformat(str(ar[0]).replace("Z", "+00:00"))
                            sess_dt = datetime.fromisoformat(str(ar[1]).replace("Z", "+00:00"))
                            min_into = int((ad_dt - sess_dt).total_seconds() / 60.0)
                            ad_times.add(min_into)
                        except Exception:
                            pass
                except Exception:
                    pass

                drop_events = []
                for i in range(1, len(curve)):
                    prev_ret = curve[i - 1]["median_retention"]
                    curr_ret = curve[i]["median_retention"]
                    if prev_ret > 0:
                        delta = (curr_ret - prev_ret) / prev_ret
                        if delta < -0.10:
                            minute = curve[i]["minute"]
                            drop_events.append({
                                "minute": minute,
                                "drop_pct": round(abs(delta) * 100, 1),
                                "type": "ad_break" if minute in ad_times else "unknown",
                            })

                # Average watch duration estimate (minute where median drops below 0.5)
                avg_watch = None
                for pt in curve:
                    if pt["median_retention"] < 0.5:
                        avg_watch = pt["minute"]
                        break

                return web.json_response({
                    "retention_curve": curve,
                    "drop_events": drop_events,
                    "avg_watch_duration_min": avg_watch,
                    "sessions_used": len(session_ids),
                    "window_days": days,
                })
        except Exception as exc:
            log.exception("Error in retention curve API")
            return analytics_internal_error_response()
