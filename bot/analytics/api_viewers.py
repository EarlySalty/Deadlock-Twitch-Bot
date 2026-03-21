"""
Analytics API v2 - Viewers Mixin.

Individual viewer analysis: viewer directory, viewer detail profiles,
viewer segmentation, and churn risk detection.
"""

from __future__ import annotations

import logging
import math
from datetime import UTC, datetime, timedelta

from aiohttp import web

from ..core.chat_bots import build_known_chat_bot_not_in_clause, is_known_chat_bot
from ..storage import pg as storage
from .error_utils import analytics_internal_error_response
from .raw_chat_status import build_raw_chat_status, build_viewer_window_metadata

log = logging.getLogger("TwitchStreams.AnalyticsV2")


def _coerce_utc_datetime(value):
    if value is None:
        return None
    if isinstance(value, datetime):
        parsed = value
    else:
        text = str(value).strip()
        if not text:
            return None
        normalized = f"{text[:-1]}+00:00" if text.endswith("Z") else text
        try:
            parsed = datetime.fromisoformat(normalized)
        except ValueError:
            return None
    if parsed.tzinfo is None:
        parsed = parsed.replace(tzinfo=UTC)
    return parsed.astimezone(UTC)


def _classify_viewer(total_sessions: int, total_messages: int, first_seen_at, last_seen_at, days_since_last: int) -> str:
    """Adaptive viewer classification based on engagement density.

    Uses sessions-per-week and messages-per-session instead of fixed
    thresholds so the classification scales with any stream frequency.
    """
    now = datetime.now(UTC)

    # ── 1. "New" check: first seen within last 14 days ──
    if first_seen_at:
        fs = first_seen_at
        if hasattr(fs, "tzinfo") and fs.tzinfo is None:
            fs = fs.replace(tzinfo=UTC)
        days_since_first = (now - fs).days
        if days_since_first <= 14 and total_sessions <= 3:
            return "new"
    else:
        days_since_first = 9999

    # ── 2. "Lurker" check: no messages at all ──
    if total_messages == 0:
        return "lurker"

    # ── 3. Engagement density metrics ──
    # Weeks active: time span from first to last seen (min 1 week)
    weeks_active = max(1.0, days_since_first / 7.0)
    sessions_per_week = total_sessions / weeks_active
    msgs_per_session = total_messages / max(1, total_sessions)

    # ── 4. Classify by density ──
    # Dedicated: shows up frequently AND actively chats
    #   ~2+ sessions/week with meaningful chat participation
    if sessions_per_week >= 1.5 and msgs_per_session >= 3.0 and total_sessions >= 4:
        return "dedicated"

    # Regular: consistent presence with some chat
    #   ~0.5+ sessions/week or decent total engagement
    if sessions_per_week >= 0.5 and total_sessions >= 3:
        return "regular"

    # Everything else is casual
    return "casual"


def _fetch_window_viewer_rows(conn, *, streamer: str, since_date: str):
    rollup_bot_clause, rollup_bot_params = build_known_chat_bot_not_in_clause(
        column_expr="sc.chatter_login"
    )
    return conn.execute(
        f"""
        SELECT
            LOWER(sc.chatter_login) AS chatter_login,
            COUNT(DISTINCT sc.session_id) AS total_sessions,
            COALESCE(SUM(sc.messages), 0) AS total_messages,
            MIN(s.started_at) AS first_seen_at,
            MAX(COALESCE(s.ended_at, s.started_at)) AS last_seen_at
        FROM twitch_session_chatters sc
        JOIN twitch_stream_sessions s ON s.id = sc.session_id
        WHERE LOWER(sc.streamer_login) = ?
          AND s.started_at >= ?
          AND {rollup_bot_clause}
        GROUP BY LOWER(sc.chatter_login)
        """,
        [streamer, since_date, *rollup_bot_params],
    ).fetchall()


def _fetch_window_viewer_row(conn, *, streamer: str, login: str, since_date: str):
    return conn.execute(
        """
        SELECT
            COUNT(DISTINCT sc.session_id) AS total_sessions,
            COALESCE(SUM(sc.messages), 0) AS total_messages,
            MIN(s.started_at) AS first_seen_at,
            MAX(COALESCE(s.ended_at, s.started_at)) AS last_seen_at
        FROM twitch_session_chatters sc
        JOIN twitch_stream_sessions s ON s.id = sc.session_id
        WHERE LOWER(sc.streamer_login) = ?
          AND LOWER(sc.chatter_login) = ?
          AND s.started_at >= ?
        """,
        [streamer, login, since_date],
    ).fetchone()


class _AnalyticsViewersMixin:
    """Mixin providing individual viewer analytics endpoints."""

    async def _api_v2_viewer_directory(self, request: web.Request) -> web.Response:
        """Paginated viewer directory with aggregated profile data."""
        self._require_v2_auth(request)
        self._require_extended_plan(request)

        streamer = request.query.get("streamer", "").strip().lower()
        if not streamer:
            return web.json_response({"error": "Streamer required"}, status=400)

        sort = request.query.get("sort", "sessions")
        order = request.query.get("order", "desc")
        filter_type = request.query.get("filter", "all")
        search = request.query.get("search", "").strip().lower()
        page = max(1, int(request.query.get("page", "1")))
        per_page = min(100, max(10, int(request.query.get("per_page", "50"))))
        days = min(365, max(1, int(request.query.get("days", "30"))))

        # Validate sort/order
        allowed_sorts = {"sessions", "messages", "last_seen", "other_channels", "first_seen"}
        if sort not in allowed_sorts:
            sort = "sessions"
        if order not in ("asc", "desc"):
            order = "desc"

        now = datetime.now(UTC)
        since_date = (now - timedelta(days=days)).isoformat()

        try:
            with storage.get_conn() as conn:
                raw_chat_status = build_raw_chat_status(
                    conn,
                    streamer,
                    since_date=since_date,
                )
                rollup_bot_clause, rollup_bot_params = build_known_chat_bot_not_in_clause(
                    column_expr="chatter_login"
                )
                # ── Core viewer data ──
                rows = _fetch_window_viewer_rows(
                    conn,
                    streamer=streamer,
                    since_date=since_date,
                )

                if not rows:
                    return web.json_response({
                        "viewers": [],
                        "total": 0,
                        "page": page,
                        "perPage": per_page,
                        "days": days,
                        "summary": {
                            "totalViewers": 0,
                            "activeViewers": 0,
                            "lurkers": 0,
                            "exclusiveViewers": 0,
                            "sharedViewers": 0,
                            "avgSessionsPerViewer": 0,
                            "avgOtherChannels": 0,
                        },
                        "rawChatStatus": raw_chat_status,
                    })

                # ── Cross-channel counts (batch) ──
                all_logins = [r[0] for r in rows]
                cross_channel = {}
                top_channels = {}
                window_metadata = build_viewer_window_metadata(
                    conn,
                    streamer,
                    all_logins,
                    since_date=since_date,
                )

                # Build cross-channel data in batches
                batch_size = 200
                for i in range(0, len(all_logins), batch_size):
                    batch = all_logins[i : i + batch_size]
                    placeholders = ",".join("?" for _ in batch)

                    # Count other channels per viewer
                    cc_rows = conn.execute(
                        f"""
                        SELECT
                            LOWER(sc.chatter_login) AS chatter_login,
                            COUNT(DISTINCT LOWER(sc.streamer_login)) - 1 AS other_count
                        FROM twitch_session_chatters sc
                        JOIN twitch_stream_sessions s ON s.id = sc.session_id
                        WHERE LOWER(sc.chatter_login) IN ({placeholders})
                          AND s.started_at >= ?
                          AND {rollup_bot_clause}
                        GROUP BY LOWER(sc.chatter_login)
                        """,
                        [login.lower() for login in batch] + [since_date, *rollup_bot_params],
                    ).fetchall()
                    for r in cc_rows:
                        cross_channel[r[0].lower()] = r[1]

                    # Top 3 other channels per viewer
                    tc_rows = conn.execute(
                        f"""
                        SELECT
                            LOWER(sc.chatter_login) AS chatter_login,
                            LOWER(sc.streamer_login) AS streamer_login,
                            COUNT(DISTINCT sc.session_id) AS total_sessions
                        FROM twitch_session_chatters sc
                        JOIN twitch_stream_sessions s ON s.id = sc.session_id
                        WHERE LOWER(sc.chatter_login) IN ({placeholders})
                          AND s.started_at >= ?
                          AND {rollup_bot_clause}
                          AND LOWER(sc.streamer_login) != ?
                        GROUP BY LOWER(sc.chatter_login), LOWER(sc.streamer_login)
                        ORDER BY chatter_login, total_sessions DESC
                        """,
                        [login.lower() for login in batch] + [since_date, *rollup_bot_params, streamer],
                    ).fetchall()
                    current_login = None
                    current_channels = []
                    for r in tc_rows:
                        login_lower = r[0].lower()
                        if login_lower != current_login:
                            if current_login is not None:
                                top_channels[current_login] = current_channels[:3]
                            current_login = login_lower
                            current_channels = []
                        current_channels.append(r[1])
                    if current_login is not None:
                        top_channels[current_login] = current_channels[:3]

                # ── Build viewer objects ──
                viewers = []
                total_lurkers = 0
                total_exclusive = 0
                total_shared = 0
                total_active = 0
                sum_sessions = 0
                sum_other_channels = 0

                for r in rows:
                    login = r[0]
                    total_sessions = r[1] or 0
                    total_messages = r[2] or 0
                    first_seen = _coerce_utc_datetime(r[3]) or r[3]
                    last_seen = _coerce_utc_datetime(r[4]) or r[4]

                    # Calculate days since last seen
                    last_seen_aware = _coerce_utc_datetime(last_seen)
                    if last_seen_aware is not None:
                        days_since = (now - last_seen_aware).days
                    else:
                        days_since = 9999

                    other_ch = cross_channel.get(login.lower(), 0)
                    category = _classify_viewer(
                        total_sessions, total_messages, first_seen, last_seen, days_since
                    )
                    is_lurker = total_messages == 0
                    avg_msg = round(total_messages / total_sessions, 1) if total_sessions > 0 else 0

                    # Summary counters
                    sum_sessions += total_sessions
                    sum_other_channels += other_ch
                    if is_lurker:
                        total_lurkers += 1
                    if other_ch == 0:
                        total_exclusive += 1
                    else:
                        total_shared += 1
                    if days_since <= 14:
                        total_active += 1

                    viewer_window_meta = window_metadata.get(login.lower(), {})
                    viewer = {
                        "login": login,
                        "totalSessions": total_sessions,
                        "totalMessages": total_messages,
                        "firstSeen": first_seen.isoformat() if hasattr(first_seen, "isoformat") else first_seen,
                        "lastSeen": last_seen.isoformat() if hasattr(last_seen, "isoformat") else last_seen,
                        "daysSinceLastSeen": days_since,
                        "otherChannels": other_ch,
                        "topOtherChannels": top_channels.get(login.lower(), []),
                        "category": category,
                        "avgMessagesPerSession": avg_msg,
                        "isLurker": is_lurker,
                        "windowPresenceSessions": int(
                            viewer_window_meta.get("windowPresenceSessions") or 0
                        ),
                        "windowPresenceMessages": int(
                            viewer_window_meta.get("windowPresenceMessages") or 0
                        ),
                        "windowRawMessages": int(
                            viewer_window_meta.get("windowRawMessages") or 0
                        ),
                        "hasRawMessages": bool(viewer_window_meta.get("hasRawMessages")),
                        "presenceOnlyInWindow": bool(
                            viewer_window_meta.get("presenceOnlyInWindow")
                        ),
                        "messageGapNote": viewer_window_meta.get("messageGapNote"),
                    }
                    viewers.append(viewer)

                total_viewers = len(viewers)
                avg_sessions = round(sum_sessions / total_viewers, 1) if total_viewers > 0 else 0
                avg_other = round(sum_other_channels / total_viewers, 1) if total_viewers > 0 else 0

                # ── Filter ──
                if filter_type == "active":
                    viewers = [v for v in viewers if v["daysSinceLastSeen"] <= 14]
                elif filter_type == "lurker":
                    viewers = [v for v in viewers if v["isLurker"]]
                elif filter_type == "exclusive":
                    viewers = [v for v in viewers if v["otherChannels"] == 0]
                elif filter_type == "shared":
                    viewers = [v for v in viewers if v["otherChannels"] > 0]
                elif filter_type == "new":
                    viewers = [v for v in viewers if v["category"] == "new"]
                elif filter_type == "churned":
                    viewers = [v for v in viewers if v["daysSinceLastSeen"] > 30]

                # ── Search ──
                if search:
                    viewers = [v for v in viewers if search in v["login"].lower()]

                # ── Sort ──
                sort_key_map = {
                    "sessions": "totalSessions",
                    "messages": "totalMessages",
                    "last_seen": "daysSinceLastSeen",
                    "other_channels": "otherChannels",
                    "first_seen": "firstSeen",
                }
                sk = sort_key_map.get(sort, "totalSessions")
                reverse = order == "desc"
                # For last_seen sort, invert because lower daysSince = more recent
                if sort == "last_seen":
                    reverse = order == "asc"
                viewers.sort(key=lambda v: v.get(sk, 0) or 0, reverse=reverse)

                # ── Paginate ──
                filtered_total = len(viewers)
                start = (page - 1) * per_page
                viewers_page = viewers[start : start + per_page]

                return web.json_response({
                    "viewers": viewers_page,
                    "total": filtered_total,
                    "page": page,
                    "perPage": per_page,
                    "days": days,
                    "summary": {
                        "totalViewers": total_viewers,
                        "activeViewers": total_active,
                        "lurkers": total_lurkers,
                        "exclusiveViewers": total_exclusive,
                        "sharedViewers": total_shared,
                        "avgSessionsPerViewer": avg_sessions,
                        "avgOtherChannels": avg_other,
                    },
                    "rawChatStatus": raw_chat_status,
                })

        except Exception as exc:
            log.exception("Error in viewer-directory API")
            return analytics_internal_error_response()

    async def _api_v2_viewer_detail(self, request: web.Request) -> web.Response:
        """Deep-dive into a single viewer's activity and cross-channel presence."""
        self._require_v2_auth(request)
        self._require_extended_plan(request)

        streamer = request.query.get("streamer", "").strip().lower()
        login = request.query.get("login", "").strip().lower()
        if not streamer or not login:
            return web.json_response({"error": "streamer and login required"}, status=400)
        if is_known_chat_bot(login):
            return web.json_response({"error": "Viewer not found"}, status=404)
        days = min(365, max(1, int(request.query.get("days", "30"))))

        now = datetime.now(UTC)
        cutoff_window = (now - timedelta(days=days)).isoformat()

        try:
            with storage.get_conn() as conn:
                raw_chat_status = build_raw_chat_status(
                    conn,
                    streamer,
                    since_date=cutoff_window,
                )
                # ── Overview from rollup ──
                row = _fetch_window_viewer_row(
                    conn,
                    streamer=streamer,
                    login=login,
                    since_date=cutoff_window,
                )

                if not row or int(row[0] or 0) <= 0:
                    return web.json_response({"error": "Viewer not found"}, status=404)

                total_sessions = row[0] or 0
                total_messages = row[1] or 0
                first_seen = _coerce_utc_datetime(row[2]) or row[2]
                last_seen = _coerce_utc_datetime(row[3]) or row[3]

                parsed_last_seen = _coerce_utc_datetime(last_seen)
                days_since = (now - parsed_last_seen).days if parsed_last_seen else 9999

                category = _classify_viewer(total_sessions, total_messages, first_seen, last_seen, days_since)
                is_lurker = total_messages == 0
                viewer_window_meta = build_viewer_window_metadata(
                    conn,
                    streamer,
                    [login],
                    since_date=cutoff_window,
                ).get(login, {})

                # ── Activity Timeline (per-session data, current window) ──
                session_rows = conn.execute(
                    """
                    SELECT
                        DATE(s.started_at) AS session_date,
                        COUNT(*) AS sessions,
                        COALESCE(SUM(sc.messages), 0) AS messages
                    FROM twitch_stream_sessions s
                    JOIN twitch_session_chatters sc ON sc.session_id = s.id
                    WHERE LOWER(s.streamer_login) = ?
                      AND LOWER(sc.chatter_login) = ?
                      AND s.started_at >= ?
                    GROUP BY DATE(s.started_at)
                    ORDER BY session_date
                    """,
                    [streamer, login, cutoff_window],
                ).fetchall()

                activity_timeline = [
                    {"date": str(r[0]), "sessions": r[1], "messages": r[2]}
                    for r in session_rows
                ]

                # ── Cross-Channel Presence ──
                cc_rows = conn.execute(
                    """
                    SELECT
                        LOWER(s.streamer_login) AS streamer_login,
                        COUNT(DISTINCT sc.session_id) AS total_sessions,
                        COALESCE(SUM(sc.messages), 0) AS total_messages,
                        MIN(s.started_at) AS first_seen_at,
                        MAX(COALESCE(s.ended_at, s.started_at)) AS last_seen_at
                    FROM twitch_session_chatters sc
                    JOIN twitch_stream_sessions s ON s.id = sc.session_id
                    WHERE LOWER(sc.chatter_login) = ?
                      AND LOWER(s.streamer_login) != ?
                      AND s.started_at >= ?
                    GROUP BY LOWER(s.streamer_login)
                    ORDER BY total_sessions DESC
                    LIMIT 15
                    """,
                    [login, streamer, cutoff_window],
                ).fetchall()

                cross_channel = []
                for cr in cc_rows:
                    cc_first = cr[3]
                    cc_last = cr[4]
                    # Determine overlap direction relative to this channel
                    if first_seen and cc_first:
                        if hasattr(cc_first, "timestamp"):
                            overlap = "before" if cc_first < first_seen else "after"
                        else:
                            overlap = "unknown"
                    else:
                        overlap = "unknown"

                    cross_channel.append({
                        "streamer": cr[0],
                        "sessions": cr[1] or 0,
                        "messages": cr[2] or 0,
                        "firstSeen": cc_first.isoformat() if hasattr(cc_first, "isoformat") else cc_first,
                        "lastSeen": cc_last.isoformat() if hasattr(cc_last, "isoformat") else cc_last,
                        "overlap": overlap,
                    })

                # ── Chat Patterns ──
                chat_rows = conn.execute(
                    """
                    SELECT
                        EXTRACT(HOUR FROM message_ts) AS hour,
                        EXTRACT(DOW FROM message_ts) AS dow,
                        COUNT(*) AS cnt
                    FROM twitch_chat_messages
                    WHERE LOWER(chatter_login) = ?
                      AND LOWER(streamer_login) = ?
                      AND message_ts >= ?
                    GROUP BY EXTRACT(HOUR FROM message_ts), EXTRACT(DOW FROM message_ts)
                    """,
                    [login, streamer, cutoff_window],
                ).fetchall()

                hour_counts: dict[int, int] = {}
                dow_counts: dict[int, int] = {}
                for cr in chat_rows:
                    h = int(cr[0])
                    d = int(cr[1])
                    c = cr[2]
                    hour_counts[h] = hour_counts.get(h, 0) + c
                    dow_counts[d] = dow_counts.get(d, 0) + c

                peak_hours = sorted(hour_counts, key=lambda h: hour_counts[h], reverse=True)[:3]
                dow_names = ["Sonntag", "Montag", "Dienstag", "Mittwoch", "Donnerstag", "Freitag", "Samstag"]
                most_active_day = dow_names[max(dow_counts, key=lambda d: dow_counts[d])] if dow_counts else "N/A"

                # ── Personality: classify messages into types ──
                personality = None
                personality_bot_clause, personality_bot_params = build_known_chat_bot_not_in_clause(
                    column_expr="m.chatter_login"
                )
                msg_rows = conn.execute(
                    f"""
                    SELECT m.content
                    FROM twitch_chat_messages m
                    JOIN twitch_stream_sessions s ON s.id = m.session_id
                    WHERE LOWER(s.streamer_login) = ?
                      AND LOWER(m.chatter_login) = ?
                      AND m.message_ts >= ?
                      AND {personality_bot_clause}
                    LIMIT 2000
                    """,
                    [streamer, login, cutoff_window, *personality_bot_params],
                ).fetchall()

                if msg_rows:
                    type_counts: dict[str, int] = {}
                    for mr in msg_rows:
                        msg_type = self._classify_message(mr[0] or "")
                        type_counts[msg_type] = type_counts.get(msg_type, 0) + 1

                    total_msgs_classified = sum(type_counts.values())
                    primary_type = max(type_counts, key=lambda k: type_counts[k]) if type_counts else "Other"
                    personality = {
                        "primary": primary_type,
                        "distribution": type_counts,
                    }

                # Message trend: compare first half vs second half of timeline
                if len(activity_timeline) >= 4:
                    mid = len(activity_timeline) // 2
                    first_half_msgs = sum(d["messages"] for d in activity_timeline[:mid])
                    second_half_msgs = sum(d["messages"] for d in activity_timeline[mid:])
                    if second_half_msgs > first_half_msgs * 1.2:
                        trend = "increasing"
                    elif first_half_msgs > second_half_msgs * 1.2:
                        trend = "decreasing"
                    else:
                        trend = "stable"
                else:
                    trend = "insufficient_data"

                avg_msg = round(total_messages / total_sessions, 1) if total_sessions > 0 else 0

                return web.json_response({
                    "login": login,
                    "days": days,
                    "overview": {
                        "totalSessions": total_sessions,
                        "totalMessages": total_messages,
                        "firstSeen": first_seen.isoformat() if hasattr(first_seen, "isoformat") else first_seen,
                        "lastSeen": last_seen.isoformat() if hasattr(last_seen, "isoformat") else last_seen,
                        "category": category,
                        "isLurker": is_lurker,
                        "windowPresenceSessions": int(
                            viewer_window_meta.get("windowPresenceSessions") or 0
                        ),
                        "windowPresenceMessages": int(
                            viewer_window_meta.get("windowPresenceMessages") or 0
                        ),
                        "windowRawMessages": int(
                            viewer_window_meta.get("windowRawMessages") or 0
                        ),
                        "hasRawMessages": bool(viewer_window_meta.get("hasRawMessages")),
                        "presenceOnlyInWindow": bool(
                            viewer_window_meta.get("presenceOnlyInWindow")
                        ),
                        "messageGapNote": viewer_window_meta.get("messageGapNote"),
                    },
                    "activityTimeline": activity_timeline,
                    "crossChannelPresence": cross_channel,
                    "chatPatterns": {
                        "peakHours": peak_hours,
                        "avgMessagesPerSession": avg_msg,
                        "mostActiveDay": most_active_day,
                        "messagesTrend": trend,
                    },
                    "rawChatStatus": raw_chat_status,
                    **({"personality": personality} if personality else {}),
                })

        except Exception as exc:
            log.exception("Error in viewer-detail API")
            return analytics_internal_error_response()

    async def _api_v2_viewer_segments(self, request: web.Request) -> web.Response:
        """Viewer segmentation with churn risk and cross-channel stats."""
        self._require_v2_auth(request)
        self._require_extended_plan(request)

        streamer = request.query.get("streamer", "").strip().lower()
        if not streamer:
            return web.json_response({"error": "Streamer required"}, status=400)

        now = datetime.now(UTC)
        days = min(365, max(1, int(request.query.get("days", "30"))))
        since_date = (now - timedelta(days=days)).isoformat()

        try:
            with storage.get_conn() as conn:
                rollup_bot_clause, rollup_bot_params = build_known_chat_bot_not_in_clause(
                    column_expr="chatter_login"
                )
                rows = _fetch_window_viewer_rows(
                    conn,
                    streamer=streamer,
                    since_date=since_date,
                )

                if not rows:
                    return web.json_response({
                        "days": days,
                        "segments": {},
                        "churnRisk": {"atRisk": 0, "recentlyChurned": 0, "atRiskViewers": []},
                        "crossChannelStats": {
                            "exclusiveViewersPct": 0,
                            "avgOtherChannels": 0,
                            "topSharedChannels": [],
                        },
                    })

                # ── Classify all viewers ──
                segments: dict[str, list[dict]] = {
                    "dedicated": [],
                    "regular": [],
                    "casual": [],
                    "lurker": [],
                    "new": [],
                }
                at_risk_detailed: list[dict] = []
                recently_churned_detailed: list[dict] = []

                for r in rows:
                    login = r[0]
                    ts = r[1] or 0
                    tm = r[2] or 0
                    fs = _coerce_utc_datetime(r[3]) or r[3]
                    ls = _coerce_utc_datetime(r[4]) or r[4]

                    parsed_last_seen = _coerce_utc_datetime(ls)
                    if parsed_last_seen is not None:
                        ds = (now - parsed_last_seen).days
                    else:
                        ds = 9999

                    cat = _classify_viewer(ts, tm, fs, ls, ds)
                    entry = {"login": login, "sessions": ts, "messages": tm}
                    if cat in segments:
                        segments[cat].append(entry)
                    else:
                        segments["casual"].append(entry)

                    # Churn detection — only for viewers who actually engaged
                    # (at least 3 sessions AND chatted), so the streamer gets
                    # an actionable "vermisst" list of real community members.
                    is_valuable = ts >= 3 and tm > 0
                    if is_valuable and 14 < ds <= 45:
                        at_risk_detailed.append({
                            "login": login,
                            "sessions": ts,
                            "messages": tm,
                            "daysSinceLastSeen": ds,
                            "category": cat,
                        })
                    elif is_valuable and ds > 45:
                        recently_churned_detailed.append({
                            "login": login,
                            "sessions": ts,
                            "messages": tm,
                            "daysSinceLastSeen": ds,
                            "category": cat,
                        })

                # Sort by most engaged first (the ones you'd miss most)
                at_risk_detailed.sort(key=lambda v: v["sessions"] * 2 + v["messages"], reverse=True)
                recently_churned_detailed.sort(key=lambda v: v["sessions"] * 2 + v["messages"], reverse=True)

                # ── Enrich at-risk viewers with "where are they now" ──
                at_risk_logins = [v["login"] for v in at_risk_detailed[:20]]
                viewer_whereabouts: dict[str, list[str]] = {}
                if at_risk_logins:
                    placeholders = ",".join("?" for _ in at_risk_logins)
                    whereabout_rows = conn.execute(
                        f"""
                        SELECT chatter_login, streamer_login, last_seen_at
                        FROM twitch_chatter_rollup
                        WHERE LOWER(chatter_login) IN ({placeholders})
                          AND LOWER(streamer_login) != ?
                          AND last_seen_at >= ?
                        ORDER BY chatter_login, last_seen_at DESC
                        """,
                        [login.lower() for login in at_risk_logins]
                        + [streamer, (now - timedelta(days=30)).isoformat()],
                    ).fetchall()
                    for wr in whereabout_rows:
                        wl = wr[0].lower()
                        if wl not in viewer_whereabouts:
                            viewer_whereabouts[wl] = []
                        if len(viewer_whereabouts[wl]) < 3:
                            viewer_whereabouts[wl].append(wr[1])

                # Attach whereabouts to at-risk entries
                for v in at_risk_detailed[:20]:
                    v["recentlySeenAt"] = viewer_whereabouts.get(v["login"].lower(), [])

                total = len(rows)
                segment_stats = {}
                for seg_name, seg_list in segments.items():
                    count = len(seg_list)
                    avg_msgs = round(
                        sum(v["messages"] for v in seg_list) / count, 1
                    ) if count > 0 else 0
                    avg_sess = round(
                        sum(v["sessions"] for v in seg_list) / count, 1
                    ) if count > 0 else 0
                    segment_stats[seg_name] = {
                        "count": count,
                        "pct": round(count / total * 100, 1) if total > 0 else 0,
                        "avgMessages": avg_msgs,
                        "avgSessions": avg_sess,
                    }

                # ── Cross-channel stats ──
                all_logins = [r[0] for r in rows]
                exclusive_count = 0
                other_channel_sum = 0

                # Batch check
                batch_size = 200
                for i in range(0, len(all_logins), batch_size):
                    batch = all_logins[i : i + batch_size]
                    placeholders = ",".join("?" for _ in batch)
                    cc_rows = conn.execute(
                        f"""
                        SELECT
                            LOWER(sc.chatter_login) AS chatter_login,
                            COUNT(DISTINCT LOWER(sc.streamer_login)) AS ch_count
                        FROM twitch_session_chatters sc
                        JOIN twitch_stream_sessions s ON s.id = sc.session_id
                        WHERE LOWER(sc.chatter_login) IN ({placeholders})
                          AND s.started_at >= ?
                          AND {rollup_bot_clause}
                        GROUP BY LOWER(sc.chatter_login)
                        """,
                        [login.lower() for login in batch] + [since_date, *rollup_bot_params],
                    ).fetchall()
                    for cr in cc_rows:
                        ch_count = cr[1]
                        if ch_count <= 1:
                            exclusive_count += 1
                        other_channel_sum += max(0, ch_count - 1)

                exclusive_pct = round(exclusive_count / total * 100, 1) if total > 0 else 0
                avg_other = round(other_channel_sum / total, 1) if total > 0 else 0

                # Top shared channels
                rollup_bot_clause_cr1, rollup_bot_params_cr1 = build_known_chat_bot_not_in_clause(
                    column_expr="sc1.chatter_login"
                )
                shared_rows = conn.execute(
                    f"""
                    SELECT LOWER(sc2.streamer_login) AS streamer_login,
                           COUNT(DISTINCT LOWER(sc2.chatter_login)) AS shared_count
                    FROM twitch_session_chatters sc1
                    JOIN twitch_stream_sessions s1 ON s1.id = sc1.session_id
                    JOIN twitch_session_chatters sc2
                      ON LOWER(sc1.chatter_login) = LOWER(sc2.chatter_login)
                    JOIN twitch_stream_sessions s2 ON s2.id = sc2.session_id
                    WHERE LOWER(sc1.streamer_login) = ?
                      AND s1.started_at >= ?
                      AND LOWER(sc2.streamer_login) != ?
                      AND s2.started_at >= ?
                      AND {rollup_bot_clause_cr1}
                    GROUP BY LOWER(sc2.streamer_login)
                    ORDER BY shared_count DESC
                    LIMIT 10
                    """,
                    [streamer, since_date, streamer, since_date, *rollup_bot_params_cr1],
                ).fetchall()

                top_shared = []
                for sr in shared_rows:
                    # Check if bidirectional (does the other streamer also share viewers back?)
                    top_shared.append({
                        "streamer": sr[0],
                        "sharedCount": sr[1],
                        "direction": "bidirectional",
                    })

                return web.json_response({
                    "days": days,
                    "segments": segment_stats,
                    "churnRisk": {
                        "atRisk": len(at_risk_detailed),
                        "recentlyChurned": len(recently_churned_detailed),
                        "atRiskViewers": at_risk_detailed[:20],
                    },
                    "crossChannelStats": {
                        "exclusiveViewersPct": exclusive_pct,
                        "avgOtherChannels": avg_other,
                        "topSharedChannels": top_shared,
                    },
                })

        except Exception as exc:
            log.exception("Error in viewer-segments API")
            return analytics_internal_error_response()
