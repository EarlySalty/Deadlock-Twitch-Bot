"""
Analytics API v2 - Viewer Presence Timeline Mixin.

Session-scoped viewer presence spans derived from chatters poll ticks.
"""

from __future__ import annotations

import asyncio
import logging
from datetime import UTC, datetime
from typing import Any

from aiohttp import web

from ..core.chat_bots import is_known_chat_bot
from ..storage import pg as storage
from .api_viewers import (
    _build_viewer_identity_not_in_clause,
    _classify_viewer,
    _coerce_utc_datetime,
    _collect_viewer_exclusion_logins,
)
from .error_utils import analytics_internal_error_response

log = logging.getLogger("TwitchStreams.AnalyticsV2")


def _row_value(row: Any, key: str, index: int, default: Any = None) -> Any:
    if row is None:
        return default
    if hasattr(row, "get"):
        return row.get(key, default)
    values = tuple(row)
    return values[index] if index < len(values) else default


async def _run_viewer_timeline_loader(loader, *args, **kwargs):
    return await asyncio.to_thread(loader, *args, **kwargs)


def _load_viewer_timeline_payload(
    owner: object,
    *,
    streamer: str,
    session_id: int,
    min_present_min: int,
    segment: str | None,
    search: str,
    limit: int,
) -> tuple[int, dict[str, object]]:
    now = datetime.now(UTC)

    with storage.readonly_connection() as conn:
        session_row = conn.execute(
            """
            SELECT
                id,
                started_at,
                ROUND(
                    EXTRACT(
                        EPOCH FROM (
                            COALESCE(
                                ended_at,
                                started_at + COALESCE(duration_seconds, 0) * INTERVAL '1 second'
                            ) - started_at
                        )
                    ) / 60
                )::int AS duration_min
            FROM twitch_stream_sessions
            WHERE id = %s
              AND LOWER(streamer_login) = %s
            LIMIT 1
            """,
            [session_id, streamer],
        ).fetchone()

        if not session_row:
            return 404, {"error": "Session not found"}

        session_start = _coerce_utc_datetime(_row_value(session_row, "started_at", 1))
        if session_start is None:
            return 500, {"error": "Session start missing"}
        session_duration_min = max(0, int(_row_value(session_row, "duration_min", 2, 0) or 0))

        excluded_logins = _collect_viewer_exclusion_logins(owner, streamer)
        tick_bot_clause, tick_bot_params = _build_viewer_identity_not_in_clause(
            column_expr="viewer_login",
            excluded_logins=excluded_logins,
        )
        span_rows = conn.execute(
            f"""
            WITH ticked AS (
                SELECT
                    LOWER(viewer_login) AS viewer_login,
                    tick_at,
                    EXTRACT(EPOCH FROM (
                        tick_at - LAG(tick_at) OVER (
                            PARTITION BY LOWER(viewer_login)
                            ORDER BY tick_at
                        )
                    )) / 60 AS gap_min
                FROM twitch_viewer_presence_ticks
                WHERE session_id = %s
                  AND {tick_bot_clause}
            ),
            grouped AS (
                SELECT
                    viewer_login,
                    tick_at,
                    SUM(CASE WHEN gap_min > 2 OR gap_min IS NULL THEN 1 ELSE 0 END)
                        OVER (PARTITION BY viewer_login ORDER BY tick_at) AS span_id
                FROM ticked
            )
            SELECT
                viewer_login,
                GREATEST(
                    0,
                    ROUND(EXTRACT(EPOCH FROM (MIN(tick_at) - %s::timestamptz)) / 60)::int
                ) AS start_min,
                GREATEST(
                    0,
                    ROUND(EXTRACT(EPOCH FROM (MAX(tick_at) - %s::timestamptz)) / 60)::int
                ) AS end_min
            FROM grouped
            GROUP BY viewer_login, span_id
            ORDER BY viewer_login, start_min
            """,
            [session_id, *tick_bot_params, session_start.isoformat(), session_start.isoformat()],
        ).fetchall()

        viewer_spans: dict[str, list[dict[str, int]]] = {}
        for row in span_rows:
            viewer_login = str(_row_value(row, "viewer_login", 0, "") or "").strip().lower()
            if not viewer_login:
                continue
            start_min = max(0, int(_row_value(row, "start_min", 1, 0) or 0))
            end_min = max(start_min, int(_row_value(row, "end_min", 2, start_min) or start_min))
            if session_duration_min > 0:
                start_min = min(start_min, session_duration_min)
                end_min = min(end_min, session_duration_min)
            viewer_spans.setdefault(viewer_login, []).append(
                {"start_min": start_min, "end_min": end_min}
            )

        if not viewer_spans:
            return 200, {
                "session_id": session_id,
                "session_start": session_start.isoformat(),
                "session_duration_min": session_duration_min,
                "viewers": [],
                "total_unique_tracked": 0,
            }

        message_bot_clause, message_bot_params = _build_viewer_identity_not_in_clause(
            column_expr="chatter_login",
            excluded_logins=excluded_logins,
        )
        message_rows = conn.execute(
            f"""
            SELECT LOWER(chatter_login) AS viewer_login, COALESCE(messages, 0) AS messages
            FROM twitch_session_chatters
            WHERE session_id = %s
              AND LOWER(streamer_login) = %s
              AND {message_bot_clause}
            """,
            [session_id, streamer, *message_bot_params],
        ).fetchall()
        chat_messages_by_viewer = {
            str(_row_value(row, "viewer_login", 0, "") or "").strip().lower(): int(
                _row_value(row, "messages", 1, 0) or 0
            )
            for row in message_rows
            if str(_row_value(row, "viewer_login", 0, "") or "").strip()
        }

        viewer_logins = sorted(viewer_spans)
        profile_bot_clause, profile_bot_params = _build_viewer_identity_not_in_clause(
            column_expr="sc.chatter_login",
            excluded_logins=excluded_logins,
        )
        profile_rows = conn.execute(
            f"""
            SELECT
                LOWER(sc.chatter_login) AS viewer_login,
                COUNT(DISTINCT sc.session_id) AS total_sessions,
                COALESCE(SUM(sc.messages), 0) AS total_messages,
                MIN(s.started_at) AS first_seen_at,
                MAX(COALESCE(s.ended_at, s.started_at)) AS last_seen_at
            FROM twitch_session_chatters sc
            JOIN twitch_stream_sessions s ON s.id = sc.session_id
            WHERE LOWER(sc.streamer_login) = %s
              AND LOWER(sc.chatter_login) = ANY(%s)
              AND {profile_bot_clause}
            GROUP BY LOWER(sc.chatter_login)
            """,
            [streamer, viewer_logins, *profile_bot_params],
        ).fetchall()
        viewer_profiles = {
            str(_row_value(row, "viewer_login", 0, "") or "").strip().lower(): row
            for row in profile_rows
            if str(_row_value(row, "viewer_login", 0, "") or "").strip()
        }

    filtered_viewers: list[dict[str, object]] = []
    segment_filter = (segment or "").strip().lower()
    search_filter = (search or "").strip().lower()
    for viewer_login, spans in viewer_spans.items():
        profile_row = viewer_profiles.get(viewer_login)
        viewer_segment = None
        if profile_row is not None:
            total_sessions = int(_row_value(profile_row, "total_sessions", 1, 0) or 0)
            total_messages = int(_row_value(profile_row, "total_messages", 2, 0) or 0)
            first_seen_at = _coerce_utc_datetime(_row_value(profile_row, "first_seen_at", 3))
            last_seen_at = _coerce_utc_datetime(_row_value(profile_row, "last_seen_at", 4))
            days_since_last = (
                (now - last_seen_at).days
                if last_seen_at is not None
                else 9999
            )
            viewer_segment = _classify_viewer(
                total_sessions,
                total_messages,
                first_seen_at,
                last_seen_at,
                days_since_last,
            )

        total_present_min = sum(
            max(0, int(span["end_min"]) - int(span["start_min"]))
            for span in spans
        )
        if total_present_min < min_present_min:
            continue
        if segment_filter and viewer_segment != segment_filter:
            continue
        if search_filter and search_filter not in viewer_login:
            continue

        filtered_viewers.append(
            {
                "login": viewer_login,
                "segment": viewer_segment,
                "spans": spans,
                "total_present_min": total_present_min,
                "chat_messages": int(chat_messages_by_viewer.get(viewer_login, 0)),
            }
        )

    filtered_viewers.sort(
        key=lambda viewer: (
            -int(viewer["total_present_min"]),
            -int(viewer["chat_messages"]),
            str(viewer["login"]),
        )
    )

    return 200, {
        "session_id": session_id,
        "session_start": session_start.isoformat(),
        "session_duration_min": session_duration_min,
        "viewers": filtered_viewers[:limit],
        "total_unique_tracked": len(filtered_viewers),
    }


def _load_viewer_timeline_profile_payload(
    *,
    streamer: str,
    login: str,
) -> tuple[int, dict[str, object]]:
    with storage.readonly_connection() as conn:
        rows = conn.execute(
            """
            WITH session_ids AS (
                SELECT DISTINCT session_id
                FROM twitch_viewer_presence_ticks
                WHERE LOWER(streamer_login) = %s
                  AND LOWER(viewer_login) = %s
                UNION
                SELECT DISTINCT session_id
                FROM twitch_session_chatters
                WHERE LOWER(streamer_login) = %s
                  AND LOWER(chatter_login) = %s
            ),
            ticked AS (
                SELECT
                    session_id,
                    tick_at,
                    EXTRACT(EPOCH FROM (
                        tick_at - LAG(tick_at) OVER (
                            PARTITION BY session_id
                            ORDER BY tick_at
                        )
                    )) / 60 AS gap_min
                FROM twitch_viewer_presence_ticks
                WHERE LOWER(streamer_login) = %s
                  AND LOWER(viewer_login) = %s
            ),
            grouped AS (
                SELECT
                    session_id,
                    tick_at,
                    SUM(CASE WHEN gap_min > 2 OR gap_min IS NULL THEN 1 ELSE 0 END)
                        OVER (PARTITION BY session_id ORDER BY tick_at) AS span_id
                FROM ticked
            ),
            span_groups AS (
                SELECT
                    session_id,
                    GREATEST(
                        0,
                        ROUND(EXTRACT(EPOCH FROM (MAX(tick_at) - MIN(tick_at))) / 60)::int
                    ) AS span_present_min
                FROM grouped
                GROUP BY session_id, span_id
            ),
            presence_totals AS (
                SELECT
                    session_id,
                    COALESCE(SUM(span_present_min), 0) AS total_present_min
                FROM span_groups
                GROUP BY session_id
            )
            SELECT
                s.id AS session_id,
                s.started_at,
                COALESCE(p.total_present_min, 0) AS total_present_min,
                COALESCE(sc.messages, 0) AS chat_messages
            FROM session_ids sid
            JOIN twitch_stream_sessions s ON s.id = sid.session_id
            LEFT JOIN presence_totals p ON p.session_id = s.id
            LEFT JOIN twitch_session_chatters sc
              ON sc.session_id = s.id
             AND LOWER(sc.streamer_login) = %s
             AND LOWER(sc.chatter_login) = %s
            WHERE LOWER(s.streamer_login) = %s
            ORDER BY s.started_at DESC
            """,
            [streamer, login, streamer, login, streamer, login, streamer, login, streamer],
        ).fetchall()

    return 200, {
        "streamer": streamer,
        "login": login,
        "sessions": [
            {
                "session_id": int(_row_value(row, "session_id", 0, 0) or 0),
                "started_at": (
                    started_at.isoformat()
                    if (started_at := _coerce_utc_datetime(_row_value(row, "started_at", 1))) is not None
                    else None
                ),
                "total_present_min": int(_row_value(row, "total_present_min", 2, 0) or 0),
                "chat_messages": int(_row_value(row, "chat_messages", 3, 0) or 0),
            }
            for row in rows
        ],
        "total_sessions": len(rows),
    }


class _ViewerTimelineMixin:
    """Mixin providing session-specific viewer presence timeline endpoints."""

    def _register_v2_routes(self, router: web.UrlDispatcher) -> None:
        super()._register_v2_routes(router)
        router.add_get(
            "/twitch/api/v2/{streamer}/viewer-timeline",
            self._api_v2_viewer_timeline_session,
        )
        router.add_get(
            "/twitch/api/v2/{streamer}/viewer-timeline/profile",
            self._api_v2_viewer_timeline_profile,
        )

    async def _api_v2_viewer_timeline_session(self, request: web.Request) -> web.Response:
        self._require_v2_auth(request)
        self._require_extended_plan(request)

        streamer = str(request.match_info.get("streamer") or "").strip().lower()
        if not streamer:
            return web.json_response({"error": "Streamer required"}, status=400)

        session_id_raw = str(request.query.get("session_id") or "").strip()
        if not session_id_raw:
            return web.json_response({"error": "session_id required"}, status=400)
        try:
            session_id = int(session_id_raw)
        except ValueError:
            return web.json_response({"error": "session_id invalid"}, status=400)

        try:
            min_present_min = max(0, int(request.query.get("min_present_min", "0")))
        except ValueError:
            return web.json_response({"error": "min_present_min invalid"}, status=400)
        segment = str(request.query.get("segment") or "").strip().lower() or None
        if segment == "all":
            segment = None
        search = str(request.query.get("search") or "").strip().lower()
        try:
            limit = min(1000, max(1, int(request.query.get("limit", "200"))))
        except ValueError:
            return web.json_response({"error": "limit invalid"}, status=400)

        try:
            status, payload = await _run_viewer_timeline_loader(
                _load_viewer_timeline_payload,
                self,
                streamer=streamer,
                session_id=session_id,
                min_present_min=min_present_min,
                segment=segment,
                search=search,
                limit=limit,
            )
            return web.json_response(payload, status=status)
        except Exception:
            log.exception("Error in viewer timeline session API")
            return analytics_internal_error_response()

    async def _api_v2_viewer_timeline_profile(self, request: web.Request) -> web.Response:
        self._require_v2_auth(request)
        self._require_extended_plan(request)

        streamer = str(request.match_info.get("streamer") or "").strip().lower()
        login = str(request.query.get("login") or "").strip().lower()
        if not streamer or not login:
            return web.json_response({"error": "streamer and login required"}, status=400)

        excluded_logins = set(_collect_viewer_exclusion_logins(self, streamer))
        if is_known_chat_bot(login) or login in excluded_logins:
            return web.json_response({"error": "Viewer not found"}, status=404)

        try:
            status, payload = await _run_viewer_timeline_loader(
                _load_viewer_timeline_profile_payload,
                streamer=streamer,
                login=login,
            )
            return web.json_response(payload, status=status)
        except Exception:
            log.exception("Error in viewer timeline profile API")
            return analytics_internal_error_response()
