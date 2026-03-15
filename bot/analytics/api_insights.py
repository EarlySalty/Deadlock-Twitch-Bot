"""
Analytics API v2 - Insights Mixin.

Insights and AI: coaching, chat analytics, monetization,
percentile helpers, generate insights/actions.
"""

from __future__ import annotations

import collections
import logging
from datetime import UTC, date, datetime, timedelta
from decimal import Decimal
from typing import Any
from zoneinfo import ZoneInfo, ZoneInfoNotFoundError

from aiohttp import web

from ..core.chat_bots import build_known_chat_bot_not_in_clause
from ..storage import pg as storage
from .raw_chat_status import build_raw_chat_status
from .coaching_engine import CoachingEngine
from .engagement_metrics import EngagementInputs, calculate_engagement

log = logging.getLogger("TwitchStreams.AnalyticsV2")

# Shared thresholds used by both _generate_insights and _generate_actions
# to ensure consistent classification boundaries.
RETENTION_LOW = 40.0   # % – below this → warn/act
RETENTION_HIGH = 65.0  # % – above this → positive feedback
CHAT_LOW = 5.0         # chatters/100 viewers – below this → warn/act
CHAT_HIGH = 30.0       # chatters/100 viewers – above this → positive feedback


class _AnalyticsInsightsMixin:
    """Mixin providing insights, coaching, chat analytics, and monetization endpoints."""

    @staticmethod
    def _sanitize_log_value(value: Any) -> str:
        text = "" if value is None else str(value)
        return text.replace("\r", "\\r").replace("\n", "\\n")

    def _get_category_percentiles(
        self, conn, since_date: str, threshold: float | None = None
    ) -> dict[str, Any]:
        """Get per-streamer AVG viewer_count from stats_category and compute percentiles.

        When threshold is set, streamers with avg_viewers above it are excluded
        (external-reach filter – e.g. EXTERNAL_REACH_AVG_THRESHOLD = 100).
        """
        having_clause = "HAVING AVG(viewer_count) <= ?" if threshold is not None else ""
        params: list = [since_date]
        if threshold is not None:
            params.append(threshold)
        rows = conn.execute(
            f"""
            SELECT streamer, AVG(viewer_count) as avg_vc
            FROM twitch_stats_category
            WHERE ts_utc >= ?
            GROUP BY streamer
            {having_clause}
            ORDER BY avg_vc
        """,
            params,
        ).fetchall()

        if not rows:
            return {"sorted_avgs": [], "streamer_map": {}, "total": 0}

        sorted_avgs = [float(r[1]) for r in rows]
        streamer_map = {r[0].lower(): float(r[1]) for r in rows}
        return {
            "sorted_avgs": sorted_avgs,
            "streamer_map": streamer_map,
            "total": len(rows),
        }

    def _percentile_of(self, sorted_avgs: list[float], value: float) -> float:
        """Return the percentile (0-1) of value within sorted_avgs."""
        if not sorted_avgs:
            return 0.5
        below = sum(1 for v in sorted_avgs if v < value)
        equal = sum(1 for v in sorted_avgs if v == value)
        return (below + 0.5 * equal) / len(sorted_avgs)

    @staticmethod
    def _interpolated_percentile(values: list[float], pct: float) -> float:
        """Return an interpolated percentile for a numeric list."""
        if not values:
            return 0.0
        s = sorted(values)
        idx = (len(s) - 1) * pct
        lo = int(idx)
        hi = min(lo + 1, len(s) - 1)
        return s[lo] + (s[hi] - s[lo]) * (idx - lo)

    def _generate_insights(self, metrics: dict[str, Any]) -> list[dict[str, str]]:
        """Generate findings/insights from metrics."""
        insights = []

        # Retention
        ret_10m = metrics.get("avg_retention_10m", 0)
        if metrics.get("retention_sample_count", 0) < 3:
            insights.append(
                {
                    "type": "info",
                    "title": "Retention-Daten unzureichend",
                    "text": "Zu wenige Sessions mit >=3 Viewern fur aussagekraftige Retention-Werte.",
                }
            )
        elif ret_10m < RETENTION_LOW:
            insights.append(
                {
                    "type": "neg",
                    "title": "Niedrige Retention",
                    "text": f"10-Min Retention bei {ret_10m:.1f}%. Verbessere den Stream-Einstieg.",
                }
            )
        elif ret_10m > RETENTION_HIGH:
            insights.append(
                {
                    "type": "pos",
                    "title": "Starke Retention",
                    "text": f"Exzellente {ret_10m:.1f}% Retention. Dein Content fesselt!",
                }
            )

        # Chat
        chat_100 = metrics.get("chat_per_100", 0)
        if metrics.get("chat_sample_count", 0) < 3:
            insights.append(
                {
                    "type": "info",
                    "title": "Chat-Daten unzureichend",
                    "text": "Zu wenige Sessions mit >=3 Viewern fur aussagekraftige Chat-Metriken.",
                }
            )
        elif chat_100 < CHAT_LOW:
            insights.append(
                {
                    "type": "warn",
                    "title": "Niedrige Chat-Aktivitat",
                    "text": f"Nur {chat_100:.1f} Chatter/100 Peak-Viewer (Proxy). Mehr Interaktion fordern!",
                }
            )
        elif chat_100 > CHAT_HIGH:
            insights.append(
                {
                    "type": "pos",
                    "title": "Aktive Community",
                    "text": f"{chat_100:.1f} Chatter/100 Peak-Viewer (Proxy) - sehr engagiert!",
                }
            )

        # Followers (skip when no valid follower data)
        fph = metrics.get("followers_per_hour", 0)
        gained_fph = metrics.get("gained_followers_per_hour", 0)
        follower_data_valid = metrics.get("follower_valid_count", 0) > 0
        if not follower_data_valid:
            pass  # No reliable follower data -- skip all follower insights
        elif fph < 0:
            insights.append(
                {
                    "type": "neg",
                    "title": "Follower-Verlust",
                    "text": f"Netto {fph:.2f} Follower/Stunde ({metrics.get('total_followers', 0):+d} gesamt). "
                    f"Gewonnen: {gained_fph:.2f}/h. Unfollows uberwiegen.",
                }
            )
        elif fph < 0.5:
            insights.append(
                {
                    "type": "warn",
                    "title": "Langsames Follower-Wachstum",
                    "text": f"Nur {fph:.2f} Follower/Stunde. Regelmaig an Follows erinnern!",
                }
            )
        elif fph > 3:
            insights.append(
                {
                    "type": "pos",
                    "title": "Starkes Wachstum",
                    "text": f"{fph:.1f} Follower/Stunde - ausgezeichnet!",
                }
            )

        return insights

    def _generate_actions(self, metrics: dict[str, Any]) -> list[dict[str, str]]:
        """Generate action recommendations."""
        actions = []

        ret_10m = metrics.get("avg_retention_10m", 0)
        if metrics.get("retention_sample_count", 0) >= 3 and ret_10m < RETENTION_LOW:
            actions.append(
                {
                    "tag": "Retention",
                    "text": "Starte mit einem starken Hook in den ersten 2 Minuten.",
                    "priority": "high",
                }
            )

        chat_100 = metrics.get("chat_per_100", 0)
        if metrics.get("chat_sample_count", 0) >= 3 and chat_100 < CHAT_LOW:
            actions.append(
                {
                    "tag": "Engagement",
                    "text": "Stelle alle 5-10 Minuten eine direkte Frage an den Chat.",
                    "priority": "medium",
                }
            )

        fph = metrics.get("followers_per_hour", 0)
        follower_data_valid = metrics.get("follower_valid_count", 0) > 0
        if follower_data_valid and fph < 0:
            actions.append(
                {
                    "tag": "Growth",
                    "text": "Follower-Verlust! Prufe ob Content-Wechsel oder lange Pausen Unfollows verursachen.",
                    "priority": "high",
                }
            )
        elif follower_data_valid and fph < 1:
            actions.append(
                {
                    "tag": "Growth",
                    "text": "Erinnere alle 20-30 Minuten an Follow mit konkretem Grund.",
                    "priority": "medium",
                }
            )

        return actions

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
                "Unknown timezone '%s' for chat analytics; falling back to UTC",
                _AnalyticsInsightsMixin._sanitize_log_value(tz_name),
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

    async def _api_v2_chat_analytics(self, request: web.Request) -> web.Response:
        """Get chat analytics."""
        self._require_v2_auth(request)

        streamer = request.query.get("streamer", "").strip() or None
        days = min(max(int(request.query.get("days", "30")), 7), 365)
        tz_requested = request.query.get("timezone", "UTC")
        target_tz, timezone_name = self._resolve_target_timezone(tz_requested)

        try:
            with storage.get_conn() as conn:
                since_date = (datetime.now(UTC) - timedelta(days=days)).isoformat()

                if not streamer:
                    return web.json_response({"error": "Streamer required"}, status=400)
                streamer_login = streamer.lower()
                msg_bot_clause, msg_bot_params = build_known_chat_bot_not_in_clause(
                    column_expr="chatter_login"
                )
                msg_bot_clause_cm, _ = build_known_chat_bot_not_in_clause(
                    column_expr="cm.chatter_login"
                )
                session_bot_clause, session_bot_params = build_known_chat_bot_not_in_clause(
                    column_expr="sc.chatter_login"
                )
                rollup_bot_clause, rollup_bot_params = build_known_chat_bot_not_in_clause(
                    column_expr="chatter_login"
                )

                # Session context for normalization (e.g. messages/minute, loyalty score).
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
                    WHERE s.started_at >= ?
                      AND LOWER(s.streamer_login) = ?
                      AND s.ended_at IS NOT NULL
                    """,
                    [since_date, streamer_login],
                ).fetchone()
                session_count = int(session_stats[0]) if session_stats and session_stats[0] else 0
                total_duration_seconds = (
                    float(session_stats[1]) if session_stats and session_stats[1] else 0.0
                )
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
                    WHERE s.started_at >= ?
                      AND LOWER(s.streamer_login) = ?
                      AND s.ended_at IS NOT NULL
                    """,
                    [since_date, streamer_login],
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

                session_benchmark_rows = conn.execute(
                    f"""
                    WITH session_messages AS (
                        SELECT
                            cm.session_id,
                            COUNT(*) AS message_count
                        FROM twitch_chat_messages cm
                        JOIN twitch_stream_sessions s ON s.id = cm.session_id
                        WHERE s.started_at >= ?
                          AND LOWER(s.streamer_login) = ?
                          AND s.ended_at IS NOT NULL
                          AND {msg_bot_clause_cm}
                        GROUP BY cm.session_id
                    ),
                    session_viewer_samples AS (
                        SELECT
                            sv.session_id,
                            COUNT(*) AS sample_count,
                            COALESCE(SUM(GREATEST(sv.viewer_count, 0)), 0) AS viewer_minutes
                        FROM twitch_session_viewers sv
                        JOIN twitch_stream_sessions s ON s.id = sv.session_id
                        WHERE s.started_at >= ?
                          AND LOWER(s.streamer_login) = ?
                          AND s.ended_at IS NOT NULL
                        GROUP BY sv.session_id
                    )
                    SELECT
                        s.id,
                        COALESCE(sm.message_count, 0) AS message_count,
                        CASE
                            WHEN COALESCE(svs.sample_count, 0) > 0
                            THEN COALESCE(svs.viewer_minutes, 0)
                            ELSE COALESCE(s.avg_viewers, 0) * GREATEST(COALESCE(s.duration_seconds, 0), 0) / 60.0
                        END AS viewer_minutes
                    FROM twitch_stream_sessions s
                    LEFT JOIN session_messages sm ON sm.session_id = s.id
                    LEFT JOIN session_viewer_samples svs ON svs.session_id = s.id
                    WHERE s.started_at >= ?
                      AND LOWER(s.streamer_login) = ?
                      AND s.ended_at IS NOT NULL
                    """,
                    [
                        since_date,
                        streamer_login,
                        *msg_bot_params,
                        since_date,
                        streamer_login,
                        since_date,
                        streamer_login,
                    ],
                ).fetchall()
                session_message_density_values: list[float] = []
                for row in session_benchmark_rows:
                    try:
                        session_viewer_minutes = float(row[2] or 0.0)
                        session_messages = int(row[1] or 0)
                    except Exception:
                        continue
                    if session_viewer_minutes <= 0:
                        continue
                    session_message_density_values.append(
                        (session_messages / session_viewer_minutes) * 100.0
                    )

                # True message counts from raw chat events in the selected time range.
                all_messages = conn.execute(
                    f"""
                    SELECT message_ts, content, is_command, chatter_login, chatter_id
                    FROM twitch_chat_messages
                    WHERE message_ts >= ?
                      AND LOWER(streamer_login) = ?
                      AND {msg_bot_clause}
                    """,
                    [since_date, streamer_login, *msg_bot_params],
                ).fetchall()

                total_messages = len(all_messages)
                command_messages = 0
                distinct_chatters_set = set()

                type_counts = collections.Counter()
                hour_counts = collections.Counter()

                for r in all_messages:
                    ts_value = r[0]
                    content = r[1] or ""
                    is_cmd = r[2]
                    chatter_key = r[3] or r[4] or ""

                    if is_cmd:
                        command_messages += 1
                    if chatter_key:
                        distinct_chatters_set.add(chatter_key)

                    # Type Analysis
                    msg_type = self._classify_message(content)
                    type_counts[msg_type] += 1

                    # Hourly Analysis (timezone-aware, robust against datetime/string variants).
                    parsed_ts = self._coerce_timestamp(ts_value)
                    if parsed_ts is None:
                        log.debug("Skipping invalid chat message timestamp: %r", ts_value)
                    else:
                        hour_counts[parsed_ts.astimezone(target_tz).hour] += 1

                distinct_chatters_from_messages = len(distinct_chatters_set)

                # Chatter cohort split + lurker stats from session-level chatter table.
                chatter_rows = conn.execute(
                    f"""
                    WITH per_user AS (
                        SELECT *
                        FROM (
                            SELECT
                                COALESCE(NULLIF(sc.chatter_login, ''), sc.chatter_id) AS chatter_key,
                                NULLIF(sc.chatter_login, '') AS chatter_login,
                                COUNT(DISTINCT sc.session_id) AS session_count,
                                SUM(sc.messages) AS total_messages,
                                MAX(CASE WHEN sc.messages > 0 THEN 1 ELSE 0 END) AS active_flag,
                                MAX(CASE WHEN sc.messages = 0 AND LOWER(COALESCE(CAST(sc.seen_via_chatters_api AS TEXT), '0')) IN ('1', 't', 'true') THEN 1 ELSE 0 END) AS lurker_flag,
                                MAX(CASE WHEN LOWER(COALESCE(CAST(sc.is_first_time_streamer AS TEXT), '0')) IN ('1', 't', 'true') THEN 1 ELSE 0 END) AS first_time_flag,
                                MAX(CASE WHEN sc.is_first_time_streamer IS NOT NULL THEN 1 ELSE 0 END) AS has_first_flag,
                                MAX(CASE WHEN LOWER(COALESCE(CAST(sc.seen_via_chatters_api AS TEXT), '0')) IN ('1', 't', 'true') THEN 1 ELSE 0 END) AS seen_flag
                            FROM twitch_session_chatters sc
                            JOIN twitch_stream_sessions s ON s.id = sc.session_id
                            WHERE s.started_at >= ?
                              AND LOWER(s.streamer_login) = ?
                              AND s.ended_at IS NOT NULL
                              AND {session_bot_clause}
                            GROUP BY 1, 2
                        ) grouped_chatters
                        WHERE chatter_key IS NOT NULL
                    ),
                    rollup AS (
                        SELECT
                            LOWER(streamer_login) AS streamer_login,
                            LOWER(chatter_login) AS chatter_login,
                            first_seen_at
                        FROM twitch_chatter_rollup
                        WHERE LOWER(streamer_login) = ?
                          AND {rollup_bot_clause}
                    )
                    SELECT
                        pu.chatter_key,
                        pu.chatter_login,
                        pu.session_count,
                        pu.total_messages,
                        pu.active_flag,
                        pu.lurker_flag,
                        pu.first_time_flag,
                        pu.has_first_flag,
                        pu.seen_flag,
                        CASE
                            WHEN r.chatter_login IS NOT NULL AND r.first_seen_at < ?
                            THEN 1 ELSE 0
                        END AS seen_before
                    FROM per_user pu
                    LEFT JOIN rollup r ON r.chatter_login = LOWER(pu.chatter_login)
                    """,
                    [
                        since_date,
                        streamer_login,
                        *session_bot_params,
                        streamer_login,
                        *rollup_bot_params,
                        since_date,
                    ],
                ).fetchall()

                chatter_entries = []
                has_first_flag_data = False
                for row in chatter_rows:
                    entry = {
                        "chatter_key": row[0],
                        "chatter_login": row[1],
                        "session_count": int(row[2] or 0),
                        "total_messages": int(row[3] or 0),
                        "active_flag": bool(row[4]),
                        "lurker_flag": bool(row[5]),
                        "first_time_flag": bool(row[6]),
                        "has_first_flag": bool(row[7]),
                        "seen_flag": bool(row[8]),
                        "seen_before": bool(row[9]),
                    }
                    has_first_flag_data = has_first_flag_data or entry["has_first_flag"]
                    chatter_entries.append(entry)

                tracked_unique_viewers = len(chatter_entries)
                sessions_with_chat_row = conn.execute(
                    f"""
                    SELECT COUNT(DISTINCT sc.session_id)
                    FROM twitch_session_chatters sc
                    JOIN twitch_stream_sessions s ON s.id = sc.session_id
                    WHERE s.started_at >= ?
                      AND LOWER(s.streamer_login) = ?
                      AND s.ended_at IS NOT NULL
                      AND {session_bot_clause}
                    """,
                    [since_date, streamer_login, *session_bot_params],
                ).fetchone()
                sessions_with_chat = int(sessions_with_chat_row[0]) if sessions_with_chat_row and sessions_with_chat_row[0] else 0

                active_chatters_count = sum(1 for c in chatter_entries if c["active_flag"])
                lurker_count = sum(
                    1 for c in chatter_entries if (not c["active_flag"]) and c["lurker_flag"]
                )
                chatters_api_seen = sum(1 for c in chatter_entries if c["seen_flag"])
                total_messages_per_user = sum(c["total_messages"] for c in chatter_entries)
                avg_messages_per_chatter = (
                    round(total_messages_per_user / active_chatters_count, 1)
                    if active_chatters_count > 0
                    else 0.0
                )

                # Detect cold-rollup: if >90% have seen_before=False, fall back
                # to session_count as the returning indicator.
                seen_before_count = sum(1 for c in chatter_entries if c["seen_before"])
                cold_rollup = len(chatter_entries) > 0 and (seen_before_count / len(chatter_entries)) < 0.1

                if days <= 7:
                    loyal_session_threshold = 2
                elif days <= 30:
                    loyal_session_threshold = 3
                elif days <= 90:
                    loyal_session_threshold = 8
                else:
                    loyal_session_threshold = 12

                first_time_chatters = 0
                returning_viewers = 0
                core_loyal_viewers = 0
                silent_core_loyal_viewers = 0
                for c in chatter_entries:
                    if cold_rollup:
                        is_first = c.get("session_count", 1) < 2
                    elif has_first_flag_data and c["has_first_flag"]:
                        is_first = c["first_time_flag"]
                        if (not is_first) and c["lurker_flag"] and (not c["seen_before"]):
                            is_first = True
                    else:
                        is_first = not c["seen_before"] if c["chatter_login"] else True

                    is_returning = not is_first
                    if c["active_flag"] and is_first:
                        first_time_chatters += 1
                    if is_returning:
                        returning_viewers += 1
                        if (
                            c["session_count"] >= loyal_session_threshold
                            and (c["active_flag"] or c["lurker_flag"] or c["seen_flag"])
                        ):
                            core_loyal_viewers += 1
                            if not c["active_flag"]:
                                silent_core_loyal_viewers += 1

                # Fallback for older rows where session_chatters may be sparse.
                if active_chatters_count == 0 and distinct_chatters_from_messages > 0:
                    active_chatters_count = distinct_chatters_from_messages
                    first_time_chatters = distinct_chatters_from_messages
                    returning_viewers = 0
                    core_loyal_viewers = 0
                    silent_core_loyal_viewers = 0
                    lurker_count = 0
                    chatters_api_seen = 0
                    tracked_unique_viewers = distinct_chatters_from_messages
                    avg_messages_per_chatter = 0.0

                unique_chatters = active_chatters_count
                first_time_chatters = min(first_time_chatters, unique_chatters)
                returning_chatters = max(0, unique_chatters - first_time_chatters)
                total_unique_viewers = (
                    tracked_unique_viewers if tracked_unique_viewers > 0 else unique_chatters
                )
                lurker_ratio = (
                    round(lurker_count / total_unique_viewers, 3) if total_unique_viewers > 0 else 0.0
                )
                total_minutes = total_duration_seconds / 60.0 if total_duration_seconds > 0 else 0.0
                messages_per_minute = (total_messages / total_minutes) if total_minutes > 0 else 0.0
                chatter_return_rate = (
                    (returning_chatters / unique_chatters) * 100.0 if unique_chatters > 0 else 0.0
                )
                core_loyal_viewer_rate = (
                    (core_loyal_viewers / total_unique_viewers) * 100.0
                    if total_unique_viewers > 0
                    else 0.0
                )
                session_message_density_sorted = sorted(session_message_density_values)
                messages_per_100_viewer_minutes_benchmark_sessions = len(
                    session_message_density_sorted
                )
                messages_per_100_viewer_minutes_percentile = None
                messages_per_100_viewer_minutes_median = None
                messages_per_100_viewer_minutes_p25 = None
                messages_per_100_viewer_minutes_p75 = None

                engagement = calculate_engagement(
                    EngagementInputs(
                        total_messages=total_messages,
                        active_chatters=active_chatters_count,
                        tracked_chat_accounts=total_unique_viewers,
                        chatters_api_seen=chatters_api_seen,
                        viewer_minutes=viewer_minutes,
                        viewer_minutes_has_real_samples=viewer_minutes_has_real_samples,
                        avg_viewers=avg_viewers,
                        session_count=session_count,
                        sessions_with_chat=sessions_with_chat,
                    )
                )
                if (
                    engagement.messages_per_100_viewer_minutes is not None
                    and messages_per_100_viewer_minutes_benchmark_sessions > 0
                ):
                    messages_per_100_viewer_minutes_percentile = round(
                        self._percentile_of(
                            session_message_density_sorted,
                            engagement.messages_per_100_viewer_minutes,
                        )
                        * 100.0,
                        1,
                    )
                    messages_per_100_viewer_minutes_median = round(
                        self._interpolated_percentile(session_message_density_sorted, 0.5),
                        2,
                    )
                    messages_per_100_viewer_minutes_p25 = round(
                        self._interpolated_percentile(session_message_density_sorted, 0.25),
                        2,
                    )
                    messages_per_100_viewer_minutes_p75 = round(
                        self._interpolated_percentile(session_message_density_sorted, 0.75),
                        2,
                    )
                active_ratio = engagement.active_ratio
                chat_session_coverage_ratio = engagement.chat_session_coverage
                chat_session_coverage_pct = round(chat_session_coverage_ratio * 100.0, 1)

                if engagement.method == "no_data":
                    confidence = "very_low"
                elif (
                    chat_session_coverage_ratio >= 0.7
                    and total_messages >= 500
                    and session_count >= 10
                ):
                    confidence = "high"
                elif (
                    chat_session_coverage_ratio >= 0.4
                    and total_messages >= 150
                    and session_count >= 5
                ):
                    confidence = "medium"
                else:
                    confidence = "low"
                data_method = engagement.method

                # Top chatters in selected period (not all-time rollup).
                top = conn.execute(
                    f"""
                    SELECT
                        COALESCE(NULLIF(cm.chatter_login, ''), cm.chatter_id, 'unknown') as chatter_key,
                        COUNT(*) as messages,
                        COUNT(DISTINCT cm.session_id) as sessions,
                        MIN(cm.message_ts) as first_seen,
                        MAX(cm.message_ts) as last_seen
                    FROM twitch_chat_messages cm
                    WHERE cm.message_ts >= ?
                      AND LOWER(cm.streamer_login) = ?
                      AND {msg_bot_clause_cm}
                    GROUP BY COALESCE(NULLIF(cm.chatter_login, ''), cm.chatter_id, 'unknown')
                    ORDER BY messages DESC
                    LIMIT 20
                    """,
                    [since_date, streamer_login, *msg_bot_params],
                ).fetchall()
                raw_chat_status = build_raw_chat_status(
                    conn,
                    streamer_login,
                    since_date=since_date,
                )

                return web.json_response(
                    {
                        "totalMessages": total_messages,
                        # Chatter fields only include active chatters (messages > 0).
                        "totalChatterSessions": unique_chatters,
                        "uniqueChatters": unique_chatters,
                        "totalTrackedViewers": total_unique_viewers,
                        "firstTimeChatters": first_time_chatters,
                        "returningChatters": returning_chatters,
                        "returningTrackedViewers": returning_viewers,
                        "coreLoyalViewers": core_loyal_viewers,
                        "silentCoreLoyalViewers": silent_core_loyal_viewers,
                        "coreLoyalViewerRate": round(core_loyal_viewer_rate, 1),
                        "loyaltySessionThreshold": loyal_session_threshold,
                        "messagesPerMinute": round(messages_per_minute, 2),
                        "chatterReturnRate": round(chatter_return_rate, 1),
                        "chatPenetrationPct": engagement.chat_penetration_pct,
                        "chatPenetrationReliable": engagement.chat_penetration_reliable,
                        "messagesPer100ViewerMinutes": engagement.messages_per_100_viewer_minutes,
                        "messagesPer100ViewerMinutesPercentile": messages_per_100_viewer_minutes_percentile,
                        "messagesPer100ViewerMinutesMedian": messages_per_100_viewer_minutes_median,
                        "messagesPer100ViewerMinutesP25": messages_per_100_viewer_minutes_p25,
                        "messagesPer100ViewerMinutesP75": messages_per_100_viewer_minutes_p75,
                        "messagesPer100ViewerMinutesBenchmarkSessions": messages_per_100_viewer_minutes_benchmark_sessions,
                        "viewerMinutes": engagement.viewer_minutes,
                        # Legacy compatibility (deprecated)
                        "legacyInteractionActivePerAvgViewer": engagement.legacy_interaction_active_per_avg_viewer,
                        "interactionRateActivePerViewer": engagement.chat_penetration_pct,
                        "interactionRateActivePerAvgViewer": engagement.legacy_interaction_active_per_avg_viewer,
                        "interactionRateReliable": engagement.chat_penetration_reliable,
                        "commandMessages": command_messages,
                        "nonCommandMessages": max(0, total_messages - command_messages),
                        "lurkerRatio": lurker_ratio,
                        "lurkerCount": lurker_count,
                        "activeChatters": active_chatters_count,
                        "activeRatio": active_ratio,
                        "avgMessagesPerChatter": avg_messages_per_chatter,
                        "timezone": timezone_name,
                        "topChatters": [
                            {
                                "login": r[0],
                                "totalMessages": int(r[1]) if r[1] else 0,
                                "totalSessions": int(r[2]) if r[2] else 0,
                                "firstSeen": r[3].isoformat()
                                if hasattr(r[3], "isoformat")
                                else r[3],
                                "lastSeen": r[4].isoformat()
                                if hasattr(r[4], "isoformat")
                                else r[4],
                                "loyaltyScore": round(
                                    min(
                                        100.0,
                                        ((int(r[2]) if r[2] else 0) / max(1, session_count))
                                        * 100.0,
                                    ),
                                    1,
                                ),
                            }
                            for r in top
                        ],
                        "messageTypes": [
                            {
                                "type": k,
                                "count": v,
                                "percentage": round(v / total_messages * 100, 1)
                                if total_messages > 0
                                else 0,
                            }
                            for k, v in type_counts.most_common()
                        ],
                        "hourlyActivity": [
                            {"hour": h, "count": hour_counts.get(h, 0)} for h in range(24)
                        ],
                        "dataQuality": {
                            "method": data_method,
                            "coverage": round(chat_session_coverage_ratio, 3),
                            "sampleCount": total_messages,
                            "confidence": confidence,
                            "sessions": session_count,
                            "sessionsWithChat": sessions_with_chat,
                            "chatSessionCoverage": chat_session_coverage_pct,
                            "chattersCoverage": engagement.chatters_coverage,
                            "chattersApiCoverage": engagement.chatters_coverage,
                            "passiveViewerSamples": engagement.passive_viewer_samples,
                            "viewerSampleCount": viewer_sample_count,
                            "viewerMinutesSource": (
                                "real_samples" if viewer_minutes_has_real_samples else "low_coverage"
                            ),
                            "botFilterApplied": True,
                        },
                        "rawChatStatus": raw_chat_status,
                    }
                )
        except Exception as exc:
            log.exception("Error in chat analytics API")
            return web.json_response({"error": str(exc)}, status=500)

    async def _api_v2_coaching(self, request: web.Request) -> web.Response:
        """Get personalized coaching data for a streamer."""
        self._require_v2_auth(request)
        self._require_extended_plan(request)

        streamer = request.query.get("streamer", "").strip()
        days = min(max(int(request.query.get("days", "30")), 7), 365)

        if not streamer:
            return web.json_response({"error": "Streamer required"}, status=400)

        try:
            with storage.get_conn() as conn:
                data = CoachingEngine.get_coaching_data(conn, streamer, days)

                # Normalize Decimal/Datetime values for JSON serialization
                def _sanitize(obj):
                    if isinstance(obj, Decimal):
                        return float(obj)
                    if isinstance(obj, (datetime, date)):
                        return obj.isoformat()
                    if isinstance(obj, dict):
                        return {k: _sanitize(v) for k, v in obj.items()}
                    if isinstance(obj, list):
                        return [_sanitize(v) for v in obj]
                    return obj

                data = _sanitize(data)
                return web.json_response(data)
        except Exception as exc:
            log.exception("Error in coaching API")
            return web.json_response({"error": str(exc)}, status=500)

    async def _api_v2_monetization(self, request: web.Request) -> web.Response:
        """Monetization & Hype Train overview for the last N days."""
        self._require_v2_auth(request)
        self._require_extended_plan(request)
        streamer = request.query.get("streamer", "").strip().lower()
        days = min(max(int(request.query.get("days", "30")), 7), 90)

        from datetime import datetime, timedelta

        cutoff = (datetime.now(UTC) - timedelta(days=days)).isoformat()

        ads: dict = {
            "total": 0,
            "auto": 0,
            "manual": 0,
            "sessions_with_ads": 0,
            "avg_duration_s": 0.0,
            "avg_viewer_drop_pct": None,
            "worst_ads": [],
        }
        hype_train: dict = {
            "total": 0,
            "avg_level": 0.0,
            "max_level": 0,
            "avg_duration_s": 0.0,
        }
        bits: dict = {"total": 0, "cheer_events": 0}
        subs: dict = {"total_events": 0, "gifted": 0}

        try:
            with storage.get_conn() as c:
                # --- Ad Break overview ---
                ad_agg = c.execute(
                    """
                    SELECT COUNT(*) AS total_ads,
                           SUM(CASE WHEN a.is_automatic IS TRUE THEN 1 ELSE 0 END) AS auto_ads,
                           AVG(a.duration_seconds) AS avg_duration,
                           COUNT(DISTINCT a.session_id) AS sessions_with_ads
                      FROM twitch_ad_break_events a
                      LEFT JOIN twitch_stream_sessions s ON s.id = a.session_id
                     WHERE a.started_at >= ?
                       AND (? = '' OR LOWER(s.streamer_login) = ?)
                    """,
                    (cutoff, streamer, streamer),
                ).fetchone()
                if ad_agg:
                    total = int(ad_agg["total_ads"] or 0)
                    auto = int(ad_agg["auto_ads"] or 0)
                    ads["total"] = total
                    ads["auto"] = auto
                    ads["manual"] = total - auto
                    ads["sessions_with_ads"] = int(ad_agg["sessions_with_ads"] or 0)
                    ads["avg_duration_s"] = round(float(ad_agg["avg_duration"] or 0.0), 1)

                # --- Viewer impact ---
                ad_rows = c.execute(
                    """
                    SELECT a.id, a.session_id, a.started_at, a.duration_seconds, a.is_automatic,
                           s.started_at AS session_start
                      FROM twitch_ad_break_events a
                      JOIN twitch_stream_sessions s ON s.id = a.session_id
                     WHERE a.started_at >= ?
                       AND a.session_id IS NOT NULL
                       AND (? = '' OR LOWER(s.streamer_login) = ?)
                     ORDER BY a.started_at DESC
                     LIMIT 200
                    """,
                    (cutoff, streamer, streamer),
                ).fetchall()

                timeline_map: dict = {}
                if ad_rows:
                    session_ids = list({int(r["session_id"]) for r in ad_rows if r["session_id"]})
                    if session_ids:
                        vrows = c.execute(
                            """
                            SELECT session_id, minutes_from_start, viewer_count
                              FROM twitch_session_viewers
                             WHERE session_id = ANY(?)
                             ORDER BY session_id, minutes_from_start
                            """,
                            (session_ids,),
                        ).fetchall()
                        for vr in vrows:
                            sid = int(vr["session_id"])
                            timeline_map.setdefault(sid, []).append(
                                (
                                    float(vr["minutes_from_start"] or 0),
                                    int(vr["viewer_count"] or 0),
                                )
                            )

                drop_pcts: list = []
                worst_ads: list = []
                position_buckets: dict[str, list] = {
                    "early_0_30m": [], "mid_30_60m": [], "late_60_90m": [], "endgame_90m": []
                }
                duration_buckets: dict[str, list] = {
                    "30s": [], "60s": [], "90s": [], "120s_plus": []
                }
                auto_drops: list = []
                manual_drops: list = []
                recovery_times: list = []
                duration_recovery: dict[str, list] = {
                    "30s": [], "60s": [], "90s": [], "120s_plus": []
                }

                for ad in ad_rows:
                    sid = int(ad["session_id"] or 0)
                    dur_s = float(ad["duration_seconds"] or 30)
                    try:
                        ad_dt = datetime.fromisoformat(str(ad["started_at"]).replace("Z", "+00:00"))
                        sess_dt = datetime.fromisoformat(
                            str(ad["session_start"]).replace("Z", "+00:00")
                        )
                        min_into = (ad_dt - sess_dt).total_seconds() / 60.0
                    except Exception:
                        continue
                    tl = timeline_map.get(sid, [])
                    if not tl:
                        continue
                    dur_min = dur_s / 60.0
                    pre = [v for m, v in tl if (min_into - 3) <= m < min_into]
                    post_start = min_into + dur_min
                    post = [v for m, v in tl if post_start <= m < (post_start + 2)]
                    if not pre or not post:
                        continue
                    pre_avg = sum(pre) / len(pre)
                    if pre_avg <= 0:
                        continue
                    drop = (pre_avg - sum(post) / len(post)) / pre_avg * 100.0
                    drop_pcts.append(drop)

                    # Recovery time: how many minutes until viewer count returns to pre-ad level
                    recovery_min = None
                    for m, v in sorted(tl, key=lambda x: x[0]):
                        if m > post_start and v >= pre_avg * 0.95:
                            recovery_min = round(m - post_start, 1)
                            break
                    if recovery_min is not None:
                        recovery_times.append(recovery_min)
                        # Also bucket by duration
                        dur_key = "30s" if dur_s <= 35 else "60s" if dur_s <= 65 else "90s" if dur_s <= 100 else "120s_plus"
                        duration_recovery[dur_key].append(recovery_min)

                    worst_ads.append(
                        {
                            "started_at": str(ad["started_at"] or "")[:16],
                            "duration_s": int(dur_s),
                            "drop_pct": round(drop, 1),
                            "is_automatic": bool(ad["is_automatic"]),
                            "min_into_stream": round(min_into, 1),
                            "recovery_min": recovery_min,
                        }
                    )

                    # Position bucketing
                    if min_into < 30:
                        position_buckets["early_0_30m"].append(drop)
                    elif min_into < 60:
                        position_buckets["mid_30_60m"].append(drop)
                    elif min_into < 90:
                        position_buckets["late_60_90m"].append(drop)
                    else:
                        position_buckets["endgame_90m"].append(drop)

                    # Duration bucketing
                    if dur_s <= 35:
                        duration_buckets["30s"].append(drop)
                    elif dur_s <= 65:
                        duration_buckets["60s"].append(drop)
                    elif dur_s <= 100:
                        duration_buckets["90s"].append(drop)
                    else:
                        duration_buckets["120s_plus"].append(drop)

                    # Auto vs manual
                    if ad["is_automatic"]:
                        auto_drops.append(drop)
                    else:
                        manual_drops.append(drop)

                def _avg(lst: list) -> float | None:
                    return round(sum(lst) / len(lst), 1) if lst else None

                if drop_pcts:
                    ads["avg_viewer_drop_pct"] = round(sum(drop_pcts) / len(drop_pcts), 1)
                worst_ads.sort(key=lambda x: x["drop_pct"], reverse=True)
                ads["worst_ads"] = worst_ads[:5]

                # Position impact
                ads["position_impact"] = {
                    bucket: {"avg_drop": _avg(drops), "count": len(drops)}
                    for bucket, drops in position_buckets.items()
                }

                # Duration impact
                ads["duration_impact"] = {
                    bucket: {"avg_drop": _avg(drops), "count": len(drops)}
                    for bucket, drops in duration_buckets.items()
                }

                # Auto vs manual comparison
                ads["auto_vs_manual"] = {
                    "auto_avg_drop": _avg(auto_drops),
                    "manual_avg_drop": _avg(manual_drops),
                    "auto_count": len(auto_drops),
                    "manual_count": len(manual_drops),
                }

                # Best ad time recommendation
                position_avgs = {
                    k: sum(v) / len(v) for k, v in position_buckets.items() if v
                }
                if position_avgs:
                    best_bucket = min(position_avgs, key=position_avgs.get)
                    bucket_labels = {
                        "early_0_30m": "ersten 30 Min",
                        "mid_30_60m": "Min 30-60",
                        "late_60_90m": "Min 60-90",
                        "endgame_90m": "nach Min 90",
                    }
                    worst_bucket = max(position_avgs, key=position_avgs.get)
                    ads["best_ad_time"] = (
                        f"Nach {bucket_labels.get(best_bucket, best_bucket)} "
                        f"(Ø -{position_avgs[best_bucket]:.1f}% statt "
                        f"-{position_avgs[worst_bucket]:.1f}% {bucket_labels.get(worst_bucket, worst_bucket)})"
                    )
                else:
                    ads["best_ad_time"] = None

                # Recovery time stats
                ads["avg_recovery_min"] = round(sum(recovery_times) / len(recovery_times), 1) if recovery_times else None
                ads["recovery_by_duration"] = {
                    bucket: {"avg_recovery_min": round(sum(r) / len(r), 1) if r else None, "count": len(r)}
                    for bucket, r in duration_recovery.items()
                }

                # Generate ad strategy recommendation
                recommendations: list[str] = []
                # Duration recommendation
                dur_avgs = {k: sum(v) / len(v) for k, v in duration_buckets.items() if v}
                if dur_avgs:
                    best_dur = min(dur_avgs, key=dur_avgs.get)
                    dur_labels = {"30s": "30s", "60s": "60s", "90s": "90s", "120s_plus": "120s+"}
                    recommendations.append(
                        f"{dur_labels[best_dur]}-Ads verursachen den geringsten Drop (Ø {dur_avgs[best_dur]:.1f}%)"
                    )
                # Auto vs manual
                if auto_drops and manual_drops:
                    auto_avg = sum(auto_drops) / len(auto_drops)
                    manual_avg = sum(manual_drops) / len(manual_drops)
                    if manual_avg < auto_avg * 0.7:
                        recommendations.append(
                            f"Manuelle Ads verlieren {((auto_avg - manual_avg) / auto_avg * 100):.0f}% weniger Viewer als automatische"
                        )
                # Timing recommendation
                if position_avgs:
                    best_pos = min(position_avgs, key=position_avgs.get)
                    pos_labels = {
                        "early_0_30m": "in den ersten 30 Min",
                        "mid_30_60m": "zwischen Min 30-60",
                        "late_60_90m": "zwischen Min 60-90",
                        "endgame_90m": "nach Min 90",
                    }
                    recommendations.append(
                        f"Beste Ad-Zeit: {pos_labels.get(best_pos, best_pos)} (Ø {position_avgs[best_pos]:.1f}% Drop)"
                    )
                # Recovery insight
                if recovery_times:
                    avg_rec = sum(recovery_times) / len(recovery_times)
                    recommendations.append(f"Ø Recovery-Zeit: {avg_rec:.1f} Minuten nach Ad-Ende")
                ads["recommendations"] = recommendations

                # --- Hype Train ---
                try:
                    ht = c.execute(
                        """
                        SELECT COUNT(*) AS total, AVG(h.level) AS avg_level,
                               MAX(h.level) AS max_level, AVG(h.duration_seconds) AS avg_dur
                          FROM twitch_hype_train_events h
                          LEFT JOIN twitch_stream_sessions s ON s.id = h.session_id
                         WHERE h.started_at >= ?
                           AND h.ended_at IS NOT NULL
                           AND (? = '' OR LOWER(s.streamer_login) = ?)
                        """,
                        (cutoff, streamer, streamer),
                    ).fetchone()
                    if ht:
                        hype_train = {
                            "total": int(ht["total"] or 0),
                            "avg_level": round(float(ht["avg_level"] or 0), 1),
                            "max_level": int(ht["max_level"] or 0),
                            "avg_duration_s": round(float(ht["avg_dur"] or 0), 0),
                        }
                except Exception:
                    log.debug("Hype train query failed", exc_info=True)

                # --- Bits ---
                try:
                    br = c.execute(
                        """
                        SELECT SUM(amount) AS total, COUNT(*) AS events
                          FROM twitch_bits_events
                         WHERE received_at >= ?
                           AND (? = '' OR LOWER(streamer_login) = ?)
                        """,
                        (cutoff, streamer, streamer),
                    ).fetchone()
                    if br:
                        bits = {
                            "total": int(br["total"] or 0),
                            "cheer_events": int(br["events"] or 0),
                        }
                except Exception:
                    log.debug("Bits query failed", exc_info=True)

                # --- Subs ---
                try:
                    sr = c.execute(
                        """
                        SELECT COUNT(*) AS total,
                               SUM(CASE WHEN is_gift=1 THEN 1 ELSE 0 END) AS gifted
                          FROM twitch_subscription_events
                         WHERE received_at >= ?
                           AND (? = '' OR LOWER(streamer_login) = ?)
                        """,
                        (cutoff, streamer, streamer),
                    ).fetchone()
                    if sr:
                        subs = {
                            "total_events": int(sr["total"] or 0),
                            "gifted": int(sr["gifted"] or 0),
                        }
                except Exception:
                    log.debug("Subs query failed", exc_info=True)

        except Exception as exc:
            log.exception("Error in monetization API")
            return web.json_response({"error": str(exc)}, status=500)

        return web.json_response(
            {
                "ads": ads,
                "hype_train": hype_train,
                "bits": bits,
                "subs": subs,
                "window_days": days,
            }
        )
