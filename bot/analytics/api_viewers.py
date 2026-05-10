"""
Analytics API v2 - Viewers Mixin.

Individual viewer analysis: viewer directory, viewer detail profiles,
viewer segmentation, and churn risk detection.
"""

from __future__ import annotations

import asyncio
import logging
from datetime import UTC, datetime, timedelta

from aiohttp import web

from ..core.chat_bots import (
    KNOWN_CHAT_BOTS,
    build_known_chat_bot_not_in_clause,
    is_known_chat_bot,
)
from ..storage import pg as storage
from .error_utils import analytics_internal_error_response
from .raw_chat_status import build_raw_chat_status, build_viewer_window_metadata

log = logging.getLogger("TwitchStreams.AnalyticsV2")


def _normalize_login(value: object) -> str:
    return str(value or "").strip().lower().lstrip("#")


def _collect_viewer_exclusion_logins(owner: object | None, streamer: str | None) -> list[str]:
    excluded = {_normalize_login(streamer)}

    bot_token_manager = getattr(owner, "_bot_token_manager", None)
    if bot_token_manager is not None:
        excluded.add(_normalize_login(getattr(bot_token_manager, "bot_login", None)))

    chat_bot = getattr(owner, "_twitch_chat_bot", None)
    if chat_bot is not None:
        excluded.add(_normalize_login(getattr(chat_bot, "nick", None)))
        token_manager = getattr(chat_bot, "_token_manager", None)
        if token_manager is not None:
            excluded.add(_normalize_login(getattr(token_manager, "bot_login", None)))

    raid_bot = getattr(owner, "_raid_bot", None)
    auth_manager = getattr(raid_bot, "auth_manager", None)
    token_manager = getattr(auth_manager, "token_manager", None)
    if token_manager is not None:
        excluded.add(_normalize_login(getattr(token_manager, "bot_login", None)))

    return sorted(login for login in excluded if login)


def _build_viewer_identity_not_in_clause(
    *,
    column_expr: str,
    excluded_logins: list[str] | tuple[str, ...] | None = None,
) -> tuple[str, list[str]]:
    combined = [*KNOWN_CHAT_BOTS, *(excluded_logins or [])]
    return build_known_chat_bot_not_in_clause(
        column_expr=column_expr,
        placeholder="%s",
        bots=combined,
    )


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


def _fetch_window_viewer_rows(conn, *, streamer: str, since_date: str, excluded_logins=None):
    rollup_bot_clause, rollup_bot_params = _build_viewer_identity_not_in_clause(
        column_expr="sc.chatter_login",
        excluded_logins=excluded_logins,
    )
    return conn.execute(  # nosemgrep: python.sqlalchemy.security.sqlalchemy-execute-raw-query.sqlalchemy-execute-raw-query
        f"""
        SELECT
            LOWER(sc.chatter_login) AS chatter_login,
            COUNT(DISTINCT sc.session_id) AS total_sessions,
            COALESCE(SUM(sc.messages), 0) AS total_messages,
            MIN(s.started_at) AS first_seen_at,
            MAX(COALESCE(s.ended_at, s.started_at)) AS last_seen_at
        FROM twitch_session_chatters sc
        JOIN twitch_stream_sessions s ON s.id = sc.session_id
        WHERE LOWER(sc.streamer_login) = %s
          AND s.started_at >= %s
          AND {rollup_bot_clause}
        GROUP BY LOWER(sc.chatter_login)
        """,
        [streamer, since_date, *rollup_bot_params],
    ).fetchall()


def _fetch_window_viewer_row(conn, *, streamer: str, login: str, since_date: str, excluded_logins=None):
    rollup_bot_clause, rollup_bot_params = _build_viewer_identity_not_in_clause(
        column_expr="sc.chatter_login",
        excluded_logins=excluded_logins,
    )
    return conn.execute(  # nosemgrep: python.sqlalchemy.security.sqlalchemy-execute-raw-query.sqlalchemy-execute-raw-query
        f"""
        SELECT
            COUNT(DISTINCT sc.session_id) AS total_sessions,
            COALESCE(SUM(sc.messages), 0) AS total_messages,
            MIN(s.started_at) AS first_seen_at,
            MAX(COALESCE(s.ended_at, s.started_at)) AS last_seen_at
        FROM twitch_session_chatters sc
        JOIN twitch_stream_sessions s ON s.id = sc.session_id
        WHERE LOWER(sc.streamer_login) = %s
          AND LOWER(sc.chatter_login) = %s
          AND s.started_at >= %s
          AND {rollup_bot_clause}
        """,
        [streamer, login, since_date, *rollup_bot_params],
    ).fetchone()


def _load_viewer_directory_payload(
    owner: object,
    *,
    streamer: str,
    sort: str,
    order: str,
    filter_type: str,
    search: str,
    page: int,
    per_page: int,
    days: int,
) -> dict[str, object]:
    now = datetime.now(UTC)
    since_date = (now - timedelta(days=days)).isoformat()

    with storage.readonly_connection() as conn:
        excluded_logins = _collect_viewer_exclusion_logins(owner, streamer)
        raw_chat_status = build_raw_chat_status(
            conn,
            streamer,
            since_date=since_date,
        )
        rollup_bot_clause, rollup_bot_params = _build_viewer_identity_not_in_clause(
            column_expr="chatter_login",
            excluded_logins=excluded_logins,
        )
        rows = _fetch_window_viewer_rows(
            conn,
            streamer=streamer,
            since_date=since_date,
            excluded_logins=excluded_logins,
        )

        if not rows:
            return {
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
            }

        all_logins = [row[0] for row in rows]
        cross_channel: dict[str, int] = {}
        top_channels: dict[str, list[str]] = {}
        window_metadata = build_viewer_window_metadata(
            conn,
            streamer,
            all_logins,
            since_date=since_date,
        )

        batch_size = 200
        for batch_index in range(0, len(all_logins), batch_size):
            batch = all_logins[batch_index : batch_index + batch_size]
            placeholders = ",".join("%s" for _ in batch)

            cc_rows = conn.execute(  # nosemgrep: python.sqlalchemy.security.sqlalchemy-execute-raw-query.sqlalchemy-execute-raw-query
                f"""
                SELECT
                    LOWER(sc.chatter_login) AS chatter_login,
                    COUNT(DISTINCT LOWER(sc.streamer_login)) - 1 AS other_count
                FROM twitch_session_chatters sc
                JOIN twitch_stream_sessions s ON s.id = sc.session_id
                WHERE LOWER(sc.chatter_login) IN ({placeholders})
                  AND s.started_at >= %s
                  AND {rollup_bot_clause}
                GROUP BY LOWER(sc.chatter_login)
                """,
                [login.lower() for login in batch] + [since_date, *rollup_bot_params],
            ).fetchall()
            for row in cc_rows:
                cross_channel[row[0].lower()] = row[1]

            tc_rows = conn.execute(  # nosemgrep: python.sqlalchemy.security.sqlalchemy-execute-raw-query.sqlalchemy-execute-raw-query
                f"""
                SELECT
                    LOWER(sc.chatter_login) AS chatter_login,
                    LOWER(sc.streamer_login) AS streamer_login,
                    COUNT(DISTINCT sc.session_id) AS total_sessions
                FROM twitch_session_chatters sc
                JOIN twitch_stream_sessions s ON s.id = sc.session_id
                WHERE LOWER(sc.chatter_login) IN ({placeholders})
                  AND s.started_at >= %s
                  AND {rollup_bot_clause}
                  AND LOWER(sc.streamer_login) != %s
                GROUP BY LOWER(sc.chatter_login), LOWER(sc.streamer_login)
                ORDER BY chatter_login, total_sessions DESC
                """,
                [login.lower() for login in batch] + [since_date, *rollup_bot_params, streamer],
            ).fetchall()
            current_login = None
            current_channels: list[str] = []
            for row in tc_rows:
                login_lower = row[0].lower()
                if login_lower != current_login:
                    if current_login is not None:
                        top_channels[current_login] = current_channels[:3]
                    current_login = login_lower
                    current_channels = []
                current_channels.append(row[1])
            if current_login is not None:
                top_channels[current_login] = current_channels[:3]

        viewers: list[dict[str, object]] = []
        total_lurkers = 0
        total_exclusive = 0
        total_shared = 0
        total_active = 0
        sum_sessions = 0
        sum_other_channels = 0

        for row in rows:
            login = row[0]
            total_sessions = row[1] or 0
            total_messages = row[2] or 0
            first_seen = _coerce_utc_datetime(row[3]) or row[3]
            last_seen = _coerce_utc_datetime(row[4]) or row[4]

            last_seen_aware = _coerce_utc_datetime(last_seen)
            days_since = (now - last_seen_aware).days if last_seen_aware is not None else 9999

            other_ch = cross_channel.get(login.lower(), 0)
            category = _classify_viewer(
                total_sessions,
                total_messages,
                first_seen,
                last_seen,
                days_since,
            )
            is_lurker = total_messages == 0
            avg_msg = round(total_messages / total_sessions, 1) if total_sessions > 0 else 0

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
            viewers.append(
                {
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
                    "windowPresenceSessions": int(viewer_window_meta.get("windowPresenceSessions") or 0),
                    "windowPresenceMessages": int(viewer_window_meta.get("windowPresenceMessages") or 0),
                    "windowRawMessages": int(viewer_window_meta.get("windowRawMessages") or 0),
                    "hasRawMessages": bool(viewer_window_meta.get("hasRawMessages")),
                    "presenceOnlyInWindow": bool(viewer_window_meta.get("presenceOnlyInWindow")),
                    "messageGapNote": viewer_window_meta.get("messageGapNote"),
                }
            )

    total_viewers = len(viewers)
    avg_sessions = round(sum_sessions / total_viewers, 1) if total_viewers > 0 else 0
    avg_other = round(sum_other_channels / total_viewers, 1) if total_viewers > 0 else 0

    if filter_type == "active":
        viewers = [viewer for viewer in viewers if viewer["daysSinceLastSeen"] <= 14]
    elif filter_type == "lurker":
        viewers = [viewer for viewer in viewers if viewer["isLurker"]]
    elif filter_type == "exclusive":
        viewers = [viewer for viewer in viewers if viewer["otherChannels"] == 0]
    elif filter_type == "shared":
        viewers = [viewer for viewer in viewers if viewer["otherChannels"] > 0]
    elif filter_type == "new":
        viewers = [viewer for viewer in viewers if viewer["category"] == "new"]
    elif filter_type == "churned":
        viewers = [viewer for viewer in viewers if viewer["daysSinceLastSeen"] > 30]

    if search:
        viewers = [viewer for viewer in viewers if search in str(viewer["login"]).lower()]

    sort_key_map = {
        "sessions": "totalSessions",
        "messages": "totalMessages",
        "last_seen": "daysSinceLastSeen",
        "other_channels": "otherChannels",
        "first_seen": "firstSeen",
    }
    sort_key = sort_key_map.get(sort, "totalSessions")
    reverse = order == "desc"
    if sort == "last_seen":
        reverse = order == "asc"
    viewers.sort(key=lambda viewer: viewer.get(sort_key, 0) or 0, reverse=reverse)

    filtered_total = len(viewers)
    start = (page - 1) * per_page
    viewers_page = viewers[start : start + per_page]

    return {
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
    }


async def _run_viewer_loader(loader, *args, **kwargs):
    return await asyncio.to_thread(loader, *args, **kwargs)


def _load_viewer_detail_payload(
    owner: object,
    *,
    streamer: str,
    login: str,
    days: int,
) -> tuple[int, dict[str, object]]:
    now = datetime.now(UTC)
    cutoff_window = (now - timedelta(days=days)).isoformat()
    excluded_logins = set(_collect_viewer_exclusion_logins(owner, streamer))

    with storage.readonly_connection() as conn:
        raw_chat_status = build_raw_chat_status(
            conn,
            streamer,
            since_date=cutoff_window,
        )
        row = _fetch_window_viewer_row(
            conn,
            streamer=streamer,
            login=login,
            since_date=cutoff_window,
            excluded_logins=excluded_logins,
        )
        if not row or int(row[0] or 0) <= 0:
            return 404, {"error": "Viewer not found"}

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

        session_rows = conn.execute(
            """
            SELECT
                DATE(s.started_at) AS session_date,
                COUNT(*) AS sessions,
                COALESCE(SUM(sc.messages), 0) AS messages
            FROM twitch_stream_sessions s
            JOIN twitch_session_chatters sc ON sc.session_id = s.id
            WHERE LOWER(s.streamer_login) = %s
              AND LOWER(sc.chatter_login) = %s
              AND s.started_at >= %s
            GROUP BY DATE(s.started_at)
            ORDER BY session_date
            """,
            [streamer, login, cutoff_window],
        ).fetchall()
        activity_timeline = [
            {"date": str(row[0]), "sessions": row[1], "messages": row[2]}
            for row in session_rows
        ]

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
            WHERE LOWER(sc.chatter_login) = %s
              AND LOWER(s.streamer_login) != %s
              AND s.started_at >= %s
            GROUP BY LOWER(s.streamer_login)
            ORDER BY total_sessions DESC
            LIMIT 15
            """,
            [login, streamer, cutoff_window],
        ).fetchall()

        cross_channel = []
        for row in cc_rows:
            cc_first = row[3]
            cc_last = row[4]
            if first_seen and cc_first and hasattr(cc_first, "timestamp"):
                overlap = "before" if cc_first < first_seen else "after"
            else:
                overlap = "unknown"
            cross_channel.append(
                {
                    "streamer": row[0],
                    "sessions": row[1] or 0,
                    "messages": row[2] or 0,
                    "firstSeen": cc_first.isoformat() if hasattr(cc_first, "isoformat") else cc_first,
                    "lastSeen": cc_last.isoformat() if hasattr(cc_last, "isoformat") else cc_last,
                    "overlap": overlap,
                }
            )

        chat_rows = conn.execute(
            """
            SELECT
                EXTRACT(HOUR FROM message_ts) AS hour,
                EXTRACT(DOW FROM message_ts) AS dow,
                COUNT(*) AS cnt
            FROM twitch_chat_messages
            WHERE LOWER(chatter_login) = %s
              AND LOWER(streamer_login) = %s
              AND message_ts >= %s
            GROUP BY EXTRACT(HOUR FROM message_ts), EXTRACT(DOW FROM message_ts)
            """,
            [login, streamer, cutoff_window],
        ).fetchall()

        hour_counts: dict[int, int] = {}
        dow_counts: dict[int, int] = {}
        for row in chat_rows:
            hour = int(row[0])
            dow = int(row[1])
            count = row[2]
            hour_counts[hour] = hour_counts.get(hour, 0) + count
            dow_counts[dow] = dow_counts.get(dow, 0) + count

        peak_hours = sorted(hour_counts, key=lambda hour: hour_counts[hour], reverse=True)[:3]
        dow_names = ["Sonntag", "Montag", "Dienstag", "Mittwoch", "Donnerstag", "Freitag", "Samstag"]
        most_active_day = dow_names[max(dow_counts, key=lambda dow: dow_counts[dow])] if dow_counts else "N/A"

        personality = None
        personality_bot_clause, personality_bot_params = build_known_chat_bot_not_in_clause(
            column_expr="m.chatter_login",
            placeholder="%s",
        )
        msg_rows = conn.execute(  # nosemgrep: python.sqlalchemy.security.sqlalchemy-execute-raw-query.sqlalchemy-execute-raw-query
            f"""
            SELECT m.content
            FROM twitch_chat_messages m
            JOIN twitch_stream_sessions s ON s.id = m.session_id
            WHERE LOWER(s.streamer_login) = %s
              AND LOWER(m.chatter_login) = %s
              AND m.message_ts >= %s
              AND {personality_bot_clause}
            LIMIT 2000
            """,
            [streamer, login, cutoff_window, *personality_bot_params],
        ).fetchall()
        if msg_rows:
            type_counts: dict[str, int] = {}
            for row in msg_rows:
                msg_type = owner._classify_message(row[0] or "")
                type_counts[msg_type] = type_counts.get(msg_type, 0) + 1

            primary_type = max(type_counts, key=lambda key: type_counts[key]) if type_counts else "Other"
            personality = {
                "primary": primary_type,
                "distribution": type_counts,
            }

    if len(activity_timeline) >= 4:
        midpoint = len(activity_timeline) // 2
        first_half_msgs = sum(item["messages"] for item in activity_timeline[:midpoint])
        second_half_msgs = sum(item["messages"] for item in activity_timeline[midpoint:])
        if second_half_msgs > first_half_msgs * 1.2:
            trend = "increasing"
        elif first_half_msgs > second_half_msgs * 1.2:
            trend = "decreasing"
        else:
            trend = "stable"
    else:
        trend = "insufficient_data"

    payload: dict[str, object] = {
        "login": login,
        "days": days,
        "overview": {
            "totalSessions": total_sessions,
            "totalMessages": total_messages,
            "firstSeen": first_seen.isoformat() if hasattr(first_seen, "isoformat") else first_seen,
            "lastSeen": last_seen.isoformat() if hasattr(last_seen, "isoformat") else last_seen,
            "category": category,
            "isLurker": is_lurker,
            "windowPresenceSessions": int(viewer_window_meta.get("windowPresenceSessions") or 0),
            "windowPresenceMessages": int(viewer_window_meta.get("windowPresenceMessages") or 0),
            "windowRawMessages": int(viewer_window_meta.get("windowRawMessages") or 0),
            "hasRawMessages": bool(viewer_window_meta.get("hasRawMessages")),
            "presenceOnlyInWindow": bool(viewer_window_meta.get("presenceOnlyInWindow")),
            "messageGapNote": viewer_window_meta.get("messageGapNote"),
        },
        "activityTimeline": activity_timeline,
        "crossChannelPresence": cross_channel,
        "chatPatterns": {
            "peakHours": peak_hours,
            "avgMessagesPerSession": round(total_messages / total_sessions, 1) if total_sessions > 0 else 0,
            "mostActiveDay": most_active_day,
            "messagesTrend": trend,
        },
        "rawChatStatus": raw_chat_status,
    }
    if personality:
        payload["personality"] = personality
    return 200, payload


def _load_viewer_segments_payload(
    owner: object,
    *,
    streamer: str,
    days: int,
) -> dict[str, object]:
    now = datetime.now(UTC)
    since_date = (now - timedelta(days=days)).isoformat()

    with storage.readonly_connection() as conn:
        excluded_logins = _collect_viewer_exclusion_logins(owner, streamer)
        rollup_bot_clause, rollup_bot_params = _build_viewer_identity_not_in_clause(
            column_expr="chatter_login",
            excluded_logins=excluded_logins,
        )
        rows = _fetch_window_viewer_rows(
            conn,
            streamer=streamer,
            since_date=since_date,
            excluded_logins=excluded_logins,
        )
        if not rows:
            return {
                "days": days,
                "segments": {},
                "churnRisk": {"atRisk": 0, "recentlyChurned": 0, "atRiskViewers": []},
                "crossChannelStats": {
                    "exclusiveViewersPct": 0,
                    "avgOtherChannels": 0,
                    "topSharedChannels": [],
                },
            }

        segments: dict[str, list[dict[str, object]]] = {
            "dedicated": [],
            "regular": [],
            "casual": [],
            "lurker": [],
            "new": [],
        }
        at_risk_detailed: list[dict[str, object]] = []
        recently_churned_detailed: list[dict[str, object]] = []

        for row in rows:
            login = row[0]
            total_sessions = row[1] or 0
            total_messages = row[2] or 0
            first_seen = _coerce_utc_datetime(row[3]) or row[3]
            last_seen = _coerce_utc_datetime(row[4]) or row[4]
            parsed_last_seen = _coerce_utc_datetime(last_seen)
            days_since = (now - parsed_last_seen).days if parsed_last_seen is not None else 9999

            category = _classify_viewer(total_sessions, total_messages, first_seen, last_seen, days_since)
            entry = {"login": login, "sessions": total_sessions, "messages": total_messages}
            if category in segments:
                segments[category].append(entry)
            else:
                segments["casual"].append(entry)

            is_valuable = total_sessions >= 3 and total_messages > 0
            if is_valuable and 14 < days_since <= 45:
                at_risk_detailed.append(
                    {
                        "login": login,
                        "sessions": total_sessions,
                        "messages": total_messages,
                        "daysSinceLastSeen": days_since,
                        "category": category,
                    }
                )
            elif is_valuable and days_since > 45:
                recently_churned_detailed.append(
                    {
                        "login": login,
                        "sessions": total_sessions,
                        "messages": total_messages,
                        "daysSinceLastSeen": days_since,
                        "category": category,
                    }
                )

        at_risk_detailed.sort(key=lambda viewer: viewer["sessions"] * 2 + viewer["messages"], reverse=True)
        recently_churned_detailed.sort(
            key=lambda viewer: viewer["sessions"] * 2 + viewer["messages"],
            reverse=True,
        )

        at_risk_logins = [viewer["login"] for viewer in at_risk_detailed[:20]]
        viewer_whereabouts: dict[str, list[str]] = {}
        if at_risk_logins:
            placeholders = ",".join("%s" for _ in at_risk_logins)
            whereabout_streamer_clause, whereabout_streamer_params = _build_viewer_identity_not_in_clause(
                column_expr="streamer_login",
                excluded_logins=excluded_logins,
            )
            whereabout_rows = conn.execute(  # nosemgrep: python.sqlalchemy.security.sqlalchemy-execute-raw-query.sqlalchemy-execute-raw-query
                f"""
                SELECT chatter_login, streamer_login, last_seen_at
                FROM twitch_chatter_rollup
                WHERE LOWER(chatter_login) IN ({placeholders})
                  AND LOWER(streamer_login) != %s
                  AND last_seen_at >= %s
                  AND {whereabout_streamer_clause}
                ORDER BY chatter_login, last_seen_at DESC
                """,
                [login.lower() for login in at_risk_logins]
                + [streamer, (now - timedelta(days=30)).isoformat(), *whereabout_streamer_params],
            ).fetchall()
            for row in whereabout_rows:
                login_lower = row[0].lower()
                if login_lower not in viewer_whereabouts:
                    viewer_whereabouts[login_lower] = []
                if len(viewer_whereabouts[login_lower]) < 3:
                    viewer_whereabouts[login_lower].append(row[1])

        for viewer in at_risk_detailed[:20]:
            viewer["recentlySeenAt"] = viewer_whereabouts.get(str(viewer["login"]).lower(), [])

        total = len(rows)
        segment_stats = {}
        for segment_name, segment_list in segments.items():
            count = len(segment_list)
            avg_msgs = round(sum(viewer["messages"] for viewer in segment_list) / count, 1) if count > 0 else 0
            avg_sessions = round(sum(viewer["sessions"] for viewer in segment_list) / count, 1) if count > 0 else 0
            segment_stats[segment_name] = {
                "count": count,
                "pct": round(count / total * 100, 1) if total > 0 else 0,
                "avgMessages": avg_msgs,
                "avgSessions": avg_sessions,
            }

        all_logins = [row[0] for row in rows]
        exclusive_count = 0
        other_channel_sum = 0
        batch_size = 200
        for batch_index in range(0, len(all_logins), batch_size):
            batch = all_logins[batch_index : batch_index + batch_size]
            placeholders = ",".join("%s" for _ in batch)
            cross_channel_streamer_clause, cross_channel_streamer_params = _build_viewer_identity_not_in_clause(
                column_expr="sc.streamer_login",
                excluded_logins=excluded_logins,
            )
            cc_rows = conn.execute(  # nosemgrep: python.sqlalchemy.security.sqlalchemy-execute-raw-query.sqlalchemy-execute-raw-query
                f"""
                SELECT
                    LOWER(sc.chatter_login) AS chatter_login,
                    COUNT(DISTINCT LOWER(sc.streamer_login)) AS ch_count
                FROM twitch_session_chatters sc
                JOIN twitch_stream_sessions s ON s.id = sc.session_id
                WHERE LOWER(sc.chatter_login) IN ({placeholders})
                  AND s.started_at >= %s
                  AND {rollup_bot_clause}
                  AND {cross_channel_streamer_clause}
                GROUP BY LOWER(sc.chatter_login)
                """,
                [
                    *[login.lower() for login in batch],
                    since_date,
                    *rollup_bot_params,
                    *cross_channel_streamer_params,
                ],
            ).fetchall()
            for row in cc_rows:
                channel_count = row[1]
                if channel_count <= 1:
                    exclusive_count += 1
                other_channel_sum += max(0, channel_count - 1)

        exclusive_pct = round(exclusive_count / total * 100, 1) if total > 0 else 0
        avg_other = round(other_channel_sum / total, 1) if total > 0 else 0

        rollup_bot_clause_cr1, rollup_bot_params_cr1 = _build_viewer_identity_not_in_clause(
            column_expr="sc1.chatter_login",
            excluded_logins=excluded_logins,
        )
        shared_streamer_clause, shared_streamer_params = _build_viewer_identity_not_in_clause(
            column_expr="sc2.streamer_login",
            excluded_logins=excluded_logins,
        )
        shared_rows = conn.execute(  # nosemgrep: python.sqlalchemy.security.sqlalchemy-execute-raw-query.sqlalchemy-execute-raw-query
            f"""
            SELECT LOWER(sc2.streamer_login) AS streamer_login,
                   COUNT(DISTINCT LOWER(sc2.chatter_login)) AS shared_count
            FROM twitch_session_chatters sc1
            JOIN twitch_stream_sessions s1 ON s1.id = sc1.session_id
            JOIN twitch_session_chatters sc2
              ON LOWER(sc1.chatter_login) = LOWER(sc2.chatter_login)
            JOIN twitch_stream_sessions s2 ON s2.id = sc2.session_id
            WHERE LOWER(sc1.streamer_login) = %s
              AND s1.started_at >= %s
              AND LOWER(sc2.streamer_login) != %s
              AND s2.started_at >= %s
              AND {rollup_bot_clause_cr1}
              AND {shared_streamer_clause}
            GROUP BY LOWER(sc2.streamer_login)
            ORDER BY shared_count DESC
            LIMIT 10
            """,
            [
                streamer,
                since_date,
                streamer,
                since_date,
                *rollup_bot_params_cr1,
                *shared_streamer_params,
            ],
        ).fetchall()

        shared_direction_map: dict[str, str] = {}
        if shared_rows:
            other_streamers = [str(row[0]).lower() for row in shared_rows if row and row[0]]
            if other_streamers:
                placeholders = ",".join("%s" for _ in other_streamers)
                target_rollup_clause, target_rollup_params = _build_viewer_identity_not_in_clause(
                    column_expr="target_rollup.chatter_login",
                    excluded_logins=excluded_logins,
                )
                other_rollup_clause, other_rollup_params = _build_viewer_identity_not_in_clause(
                    column_expr="other_rollup.chatter_login",
                    excluded_logins=excluded_logins,
                )
                other_streamer_clause, other_streamer_params = _build_viewer_identity_not_in_clause(
                    column_expr="other_rollup.streamer_login",
                    excluded_logins=excluded_logins,
                )
                direction_rows = conn.execute(  # nosemgrep: python.sqlalchemy.security.sqlalchemy-execute-raw-query.sqlalchemy-execute-raw-query
                    f"""
                    SELECT
                        LOWER(other_rollup.streamer_login) AS streamer_login,
                        SUM(
                            CASE
                                WHEN target_rollup.first_seen_at < other_rollup.first_seen_at
                                THEN 1 ELSE 0
                            END
                        ) AS outgoing_votes,
                        SUM(
                            CASE
                                WHEN other_rollup.first_seen_at < target_rollup.first_seen_at
                                THEN 1 ELSE 0
                            END
                        ) AS incoming_votes
                    FROM twitch_chatter_rollup target_rollup
                    JOIN twitch_chatter_rollup other_rollup
                      ON LOWER(target_rollup.chatter_login) = LOWER(other_rollup.chatter_login)
                    WHERE LOWER(target_rollup.streamer_login) = %s
                      AND LOWER(other_rollup.streamer_login) IN ({placeholders})
                      AND {target_rollup_clause}
                      AND {other_rollup_clause}
                      AND {other_streamer_clause}
                    GROUP BY LOWER(other_rollup.streamer_login)
                    """,
                    [
                        streamer,
                        *other_streamers,
                        *target_rollup_params,
                        *other_rollup_params,
                        *other_streamer_params,
                    ],
                ).fetchall()

                for row in direction_rows:
                    other_login = str(row[0] or "").lower()
                    outgoing_votes = int(row[1] or 0)
                    incoming_votes = int(row[2] or 0)
                    if incoming_votes > 0 and outgoing_votes > 0:
                        direction = "bidirectional"
                    elif incoming_votes > 0:
                        direction = "incoming"
                    elif outgoing_votes > 0:
                        direction = "outgoing"
                    else:
                        direction = "unknown"
                    shared_direction_map[other_login] = direction

    top_shared = [
        {
            "streamer": row[0],
            "sharedCount": row[1],
            "direction": shared_direction_map.get(str(row[0] or "").lower(), "unknown"),
        }
        for row in shared_rows
    ]
    return {
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
    }


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

        try:
            payload = await _run_viewer_loader(
                _load_viewer_directory_payload,
                self,
                streamer=streamer,
                sort=sort,
                order=order,
                filter_type=filter_type,
                search=search,
                page=page,
                per_page=per_page,
                days=days,
            )
            return web.json_response(payload)
        except Exception:
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
        excluded_logins = set(_collect_viewer_exclusion_logins(self, streamer))
        if is_known_chat_bot(login) or login in excluded_logins:
            return web.json_response({"error": "Viewer not found"}, status=404)
        days = min(365, max(1, int(request.query.get("days", "30"))))

        try:
            status, payload = await _run_viewer_loader(
                _load_viewer_detail_payload,
                self,
                streamer=streamer,
                login=login,
                days=days,
            )
            return web.json_response(payload, status=status)
        except Exception:
            log.exception("Error in viewer-detail API")
            return analytics_internal_error_response()

    async def _api_v2_viewer_segments(self, request: web.Request) -> web.Response:
        """Viewer segmentation with churn risk and cross-channel stats."""
        self._require_v2_auth(request)
        self._require_extended_plan(request)

        streamer = request.query.get("streamer", "").strip().lower()
        if not streamer:
            return web.json_response({"error": "Streamer required"}, status=400)

        days = min(365, max(1, int(request.query.get("days", "30"))))

        try:
            payload = await _run_viewer_loader(
                _load_viewer_segments_payload,
                self,
                streamer=streamer,
                days=days,
            )
            return web.json_response(payload)
        except Exception:
            log.exception("Error in viewer-segments API")
            return analytics_internal_error_response()
