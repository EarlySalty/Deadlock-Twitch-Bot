"""
Analytics API v2 – Public Mixin.

Public endpoints for the EarlySalty website (no auth required).
Provides live ban feed, raid ticker, and network visualisation data.
"""

from __future__ import annotations

import asyncio
import json
import logging
from datetime import UTC, datetime, timedelta
from typing import Any

from aiohttp import web

from ..storage import pg as storage

log = logging.getLogger("TwitchStreams.AnalyticsV2.Public")


class _AnalyticsPublicMixin:
    """Mixin providing public API endpoints for the EarlySalty website."""

    # ------------------------------------------------------------------
    #  Route registration
    # ------------------------------------------------------------------

    def _register_v2_public_routes(self, router: web.UrlDispatcher) -> None:
        """Register public API routes (no authentication required)."""
        router.add_get(
            "/twitch/api/v2/public/recent-bans", self._api_v2_public_recent_bans
        )
        router.add_get(
            "/twitch/api/v2/public/recent-raids", self._api_v2_public_recent_raids
        )
        router.add_get(
            "/twitch/api/v2/public/network", self._api_v2_public_network
        )

    # ------------------------------------------------------------------
    #  CORS helper
    # ------------------------------------------------------------------

    def _public_json_response(
        self,
        data: Any,
        *,
        status: int = 200,
    ) -> web.Response:
        """Return a JSON response with permissive CORS headers for public use."""
        return web.Response(
            text=json.dumps(data, default=str),
            status=status,
            content_type="application/json",
            headers={
                "Access-Control-Allow-Origin": "*",
            },
        )

    # ------------------------------------------------------------------
    #  GET /twitch/api/v2/public/recent-bans
    # ------------------------------------------------------------------

    @staticmethod
    def _serialize_timestamp(value: Any) -> str | None:
        """Serialize timestamp-like values from heterogeneous DB backends."""
        if value is None:
            return None
        isoformat = getattr(value, "isoformat", None)
        if callable(isoformat):
            try:
                return str(isoformat())
            except Exception:
                return str(value)
        text = str(value).strip()
        return text or None

    def _load_recent_bans_sync(self) -> dict[str, Any]:
        """Synchronous DB query for recent bans and aggregate stats."""
        with storage.readonly_connection() as conn:
            # Last 20 bans
            rows = conn.execute(
                """
                SELECT target_login, moderator_login, reason, received_at
                FROM twitch_ban_events
                ORDER BY received_at DESC
                LIMIT 20
                """
            ).fetchall()

            bans: list[dict[str, Any]] = []
            for row in rows:
                bans.append({
                    "target_login": str(row[0] or ""),
                    "moderator_login": str(row[1] or ""),
                    "reason": str(row[2] or ""),
                    "received_at": self._serialize_timestamp(row[3]),
                })

            # Aggregate stats
            now = datetime.now(UTC)
            today_start = now.replace(hour=0, minute=0, second=0, microsecond=0)
            thirty_days_ago = now - timedelta(days=30)

            stats_row = conn.execute(
                """
                SELECT
                    COALESCE(SUM(CASE WHEN received_at >= %s THEN 1 ELSE 0 END), 0) AS today,
                    COUNT(*) AS total_30d,
                    COUNT(DISTINCT twitch_user_id) AS channels_protected
                FROM twitch_ban_events
                WHERE received_at >= %s
                """,
                (today_start.isoformat(), thirty_days_ago.isoformat()),
            ).fetchone()

            stats = {
                "today": int(stats_row[0] or 0) if stats_row else 0,
                "total_30d": int(stats_row[1] or 0) if stats_row else 0,
                "channels_protected": int(stats_row[2] or 0) if stats_row else 0,
            }

        return {"bans": bans, "stats": stats}

    async def _api_v2_public_recent_bans(self, request: web.Request) -> web.Response:
        """Public endpoint: recent spam-bot bans with aggregate statistics."""
        try:
            data = await asyncio.to_thread(self._load_recent_bans_sync)
            return self._public_json_response(data)
        except Exception:
            log.exception("Public API: failed to load recent bans")
            return self._public_json_response(
                {"error": "internal_error"}, status=500
            )

    # ------------------------------------------------------------------
    #  GET /twitch/api/v2/public/recent-raids
    # ------------------------------------------------------------------

    def _load_recent_raids_sync(self) -> dict[str, Any]:
        """Synchronous DB query for recent successful raids."""
        with storage.readonly_connection() as conn:
            rows = conn.execute(
                """
                SELECT
                    from_broadcaster_login,
                    to_broadcaster_login,
                    viewer_count,
                    executed_at
                FROM twitch_raid_history
                WHERE success = TRUE
                ORDER BY executed_at DESC
                LIMIT 10
                """
            ).fetchall()

            raids: list[dict[str, Any]] = []
            for row in rows:
                raids.append({
                    "from_channel": str(row[0] or ""),
                    "to_channel": str(row[1] or ""),
                    "viewers": int(row[2] or 0),
                    "executed_at": self._serialize_timestamp(row[3]),
                })

        return {"raids": raids}

    async def _api_v2_public_recent_raids(self, request: web.Request) -> web.Response:
        """Public endpoint: recent successful raid events."""
        try:
            data = await asyncio.to_thread(self._load_recent_raids_sync)
            return self._public_json_response(data)
        except Exception:
            log.exception("Public API: failed to load recent raids")
            return self._public_json_response(
                {"error": "internal_error"}, status=500
            )

    # ------------------------------------------------------------------
    #  GET /twitch/api/v2/public/network
    # ------------------------------------------------------------------

    def _load_network_sync(self) -> dict[str, Any]:
        """Synchronous DB query for active partner streamers with live status."""
        with storage.readonly_connection() as conn:
            # Check if twitch_streamers_partner_state view exists
            has_partner_view = True
            try:
                conn.execute(
                    "SELECT 1 FROM twitch_streamers_partner_state LIMIT 1"
                )
            except Exception:
                has_partner_view = False

            if not has_partner_view:
                return {"streamers": []}

            rows = conn.execute(
                """
                SELECT
                    sp.twitch_login,
                    COALESCE(ls.is_live, 0) AS is_live,
                    COALESCE(ls.last_viewer_count, 0) AS viewer_count
                FROM twitch_streamers_partner_state sp
                LEFT JOIN twitch_live_state ls
                    ON LOWER(ls.streamer_login) = LOWER(sp.twitch_login)
                WHERE sp.is_partner_active = 1
                ORDER BY
                    COALESCE(ls.is_live, 0) DESC,
                    COALESCE(ls.last_viewer_count, 0) DESC,
                    LOWER(sp.twitch_login) ASC
                """
            ).fetchall()

            streamers: list[dict[str, Any]] = []
            for row in rows:
                login = str(row[0] or "").strip().lower()
                if not login:
                    continue
                streamers.append({
                    "login": login,
                    "is_partner": True,
                    "is_live": bool(int(row[1] or 0)),
                    "viewer_count": int(row[2] or 0),
                })

        return {"streamers": streamers}

    async def _api_v2_public_network(self, request: web.Request) -> web.Response:
        """Public endpoint: partner network with live status."""
        try:
            data = await asyncio.to_thread(self._load_network_sync)
            return self._public_json_response(data)
        except Exception:
            log.exception("Public API: failed to load network data")
            return self._public_json_response(
                {"error": "internal_error"}, status=500
            )
