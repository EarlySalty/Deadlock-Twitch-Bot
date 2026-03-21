"""
Analytics API v2 – Experimental Mixin.

Stellt 4 Endpunkte unter /twitch/api/v2/exp/ bereit, die ausschließlich
auf den exp_* Tabellen operieren (exp_sessions, exp_snapshots, exp_game_transitions).
"""

from __future__ import annotations

import logging
from datetime import UTC, datetime, timedelta

from aiohttp import web

from ..storage import pg as storage
from .error_utils import analytics_internal_error_response

log = logging.getLogger("TwitchStreams.AnalyticsV2.Experimental")


class _AnalyticsExperimentalMixin:
    """Mixin providing experimental analytics endpoints (all-game session tracking)."""

    # ------------------------------------------------------------------
    #  GET /twitch/api/v2/exp/overview
    #  Parameter: streamer (required), days (optional, default 30)
    # ------------------------------------------------------------------

    async def _api_v2_exp_overview(self, request: web.Request) -> web.Response:
        """KPI overview from exp_sessions."""
        self._require_v2_auth(request)
        self._require_extended_plan(request)

        streamer = request.query.get("streamer", "").strip().lower() or None
        if not streamer:
            return web.json_response({"error": "streamer parameter required"}, status=400)

        try:
            days = min(max(int(request.query.get("days", "30")), 1), 365)
        except (ValueError, TypeError):
            days = 30

        since = (datetime.now(UTC) - timedelta(days=days)).isoformat()

        try:
            with storage.get_conn() as conn:
                row = conn.execute(
                    """
                    SELECT
                        COUNT(*) AS total_sessions,
                        COUNT(DISTINCT game_name) AS games_played,
                        COALESCE(AVG(avg_viewers), 0) AS avg_viewers,
                        COALESCE(MAX(avg_viewers), 0) AS max_avg_viewers
                    FROM exp_sessions
                    WHERE LOWER(streamer) = %s
                      AND started_at >= %s
                      AND ended_at IS NOT NULL
                    """,
                    (streamer, since),
                ).fetchone()

                total_sessions = int(row[0] or 0)
                games_played = int(row[1] or 0)
                avg_viewers = float(row[2] or 0.0)

                # Best game by avg_viewers
                best_row = conn.execute(
                    """
                    SELECT game_name, AVG(avg_viewers) AS gam_avg
                    FROM exp_sessions
                    WHERE LOWER(streamer) = %s
                      AND started_at >= %s
                      AND ended_at IS NOT NULL
                      AND game_name IS NOT NULL
                      AND game_name <> ''
                    GROUP BY game_name
                    ORDER BY gam_avg DESC
                    LIMIT 1
                    """,
                    (streamer, since),
                ).fetchone()

                best_game = str(best_row[0]) if best_row else ""
                best_game_avg = float(best_row[1]) if best_row else 0.0

                return web.json_response({
                    "totalSessions": total_sessions,
                    "gamesPlayed": games_played,
                    "avgViewers": round(avg_viewers, 1),
                    "bestGame": best_game,
                    "bestGameAvgViewers": round(best_game_avg, 1),
                })
        except Exception as exc:
            log.exception("Error in exp_overview API")
            return analytics_internal_error_response()

    # ------------------------------------------------------------------
    #  GET /twitch/api/v2/exp/game-breakdown
    #  Parameter: streamer (required), days (optional)
    # ------------------------------------------------------------------

    async def _api_v2_exp_game_breakdown(self, request: web.Request) -> web.Response:
        """Per-game aggregated stats from exp_sessions."""
        self._require_v2_auth(request)
        self._require_extended_plan(request)

        streamer = request.query.get("streamer", "").strip().lower() or None
        if not streamer:
            return web.json_response({"error": "streamer parameter required"}, status=400)

        try:
            days = min(max(int(request.query.get("days", "30")), 1), 365)
        except (ValueError, TypeError):
            days = 30

        since = (datetime.now(UTC) - timedelta(days=days)).isoformat()

        try:
            with storage.get_conn() as conn:
                rows = conn.execute(
                    """
                    SELECT
                        COALESCE(game_name, '') AS game_name,
                        COUNT(*) AS sessions,
                        COALESCE(AVG(avg_viewers), 0) AS avg_viewers,
                        COALESCE(MAX(peak_viewers), 0) AS peak_viewers,
                        COALESCE(AVG(duration_min), 0) AS avg_duration_min,
                        COALESCE(AVG(follower_delta), 0) AS avg_follower_delta
                    FROM exp_sessions
                    WHERE LOWER(streamer) = %s
                      AND started_at >= %s
                      AND ended_at IS NOT NULL
                    GROUP BY game_name
                    ORDER BY avg_viewers DESC
                    """,
                    (streamer, since),
                ).fetchall()

                data = [
                    {
                        "game": str(r[0]) or "(unbekannt)",
                        "sessions": int(r[1]),
                        "avgViewers": round(float(r[2] or 0), 1),
                        "peakViewers": int(r[3] or 0),
                        "avgDurationMin": round(float(r[4] or 0), 1),
                        "avgFollowerDelta": round(float(r[5] or 0), 1),
                    }
                    for r in rows
                ]

                return web.json_response(data)
        except Exception as exc:
            log.exception("Error in exp_game_breakdown API")
            return analytics_internal_error_response()

    # ------------------------------------------------------------------
    #  GET /twitch/api/v2/exp/game-transitions
    #  Parameter: streamer (required), days (optional)
    # ------------------------------------------------------------------

    async def _api_v2_exp_game_transitions(self, request: web.Request) -> web.Response:
        """Game switch events with viewer impact from exp_game_transitions."""
        self._require_v2_auth(request)
        self._require_extended_plan(request)

        streamer = request.query.get("streamer", "").strip().lower() or None
        if not streamer:
            return web.json_response({"error": "streamer parameter required"}, status=400)

        try:
            days = min(max(int(request.query.get("days", "30")), 1), 365)
        except (ValueError, TypeError):
            days = 30

        since = (datetime.now(UTC) - timedelta(days=days)).isoformat()

        try:
            with storage.get_conn() as conn:
                rows = conn.execute(
                    """
                    SELECT
                        COALESCE(from_game, '(unbekannt)') AS from_game,
                        COALESCE(to_game, '(unbekannt)') AS to_game,
                        COUNT(*) AS transition_count,
                        COALESCE(AVG(viewer_count), 0) AS avg_viewers_at_transition
                    FROM exp_game_transitions
                    WHERE LOWER(streamer) = %s
                      AND ts_utc >= %s
                    GROUP BY from_game, to_game
                    ORDER BY transition_count DESC
                    LIMIT 50
                    """,
                    (streamer, since),
                ).fetchall()

                # For viewer_delta we'd need pre/post viewers; we report what we have
                data = [
                    {
                        "fromGame": str(r[0]),
                        "toGame": str(r[1]),
                        "count": int(r[2]),
                        "avgViewersBefore": round(float(r[3] or 0), 1),
                        "avgViewersAfter": 0.0,   # not tracked in this schema
                        "viewerDelta": 0.0,
                    }
                    for r in rows
                ]

                return web.json_response(data)
        except Exception as exc:
            log.exception("Error in exp_game_transitions API")
            return analytics_internal_error_response()

    # ------------------------------------------------------------------
    #  GET /twitch/api/v2/exp/growth-curves
    #  Parameter: streamer (required), days (optional)
    # ------------------------------------------------------------------

    async def _api_v2_exp_growth_curves(self, request: web.Request) -> web.Response:
        """Average viewer curves per game (minutes_from_start bucketed)."""
        self._require_v2_auth(request)
        self._require_extended_plan(request)

        streamer = request.query.get("streamer", "").strip().lower() or None
        if not streamer:
            return web.json_response({"error": "streamer parameter required"}, status=400)

        try:
            days = min(max(int(request.query.get("days", "30")), 1), 365)
        except (ValueError, TypeError):
            days = 30

        since = (datetime.now(UTC) - timedelta(days=days)).isoformat()

        try:
            with storage.get_conn() as conn:
                rows = conn.execute(
                    """
                    SELECT
                        COALESCE(es.game_name, '(unbekannt)') AS game_name,
                        FLOOR(sn.minutes_from_start / 5) * 5 AS minute_bucket,
                        COALESCE(AVG(sn.viewer_count), 0) AS avg_viewers,
                        COUNT(*) AS sample_count
                    FROM exp_snapshots sn
                    JOIN exp_sessions es ON es.id = sn.exp_session_id
                    WHERE LOWER(es.streamer) = %s
                      AND es.started_at >= %s
                      AND sn.minutes_from_start IS NOT NULL
                      AND sn.minutes_from_start >= 0
                      AND sn.minutes_from_start <= 360
                    GROUP BY game_name, minute_bucket
                    ORDER BY game_name, minute_bucket
                    """,
                    (streamer, since),
                ).fetchall()

                data = [
                    {
                        "game": str(r[0]),
                        "minuteFromStart": int(r[1] or 0),
                        "avgViewers": round(float(r[2] or 0), 1),
                        "sampleCount": int(r[3] or 0),
                    }
                    for r in rows
                ]

                return web.json_response(data)
        except Exception as exc:
            log.exception("Error in exp_growth_curves API")
            return analytics_internal_error_response()
