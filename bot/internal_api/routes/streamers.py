"""Streamer/admin route registration for the internal API."""

from __future__ import annotations

from typing import Any, Callable

from aiohttp import web

INTERNAL_API_BASE_PATH = "/internal/twitch/v1"


def _bind(server: Any, method_name: str) -> Callable[[web.Request], Any]:
    async def _handler(request: web.Request) -> web.StreamResponse:
        return await getattr(server, method_name)(request)

    return _handler


def build_streamer_route_defs(server: Any) -> list[web.RouteDef]:
    base = str(getattr(server, "_base_path", INTERNAL_API_BASE_PATH) or INTERNAL_API_BASE_PATH).rstrip("/")
    return [
        web.get(f"{base}/streamers", _bind(server, "streamers")),
        web.post(f"{base}/streamers", _bind(server, "streamer_add")),
        web.delete(
            f"{base}/streamers/{{login}}",
            _bind(server, "streamer_remove"),
        ),
        web.post(
            f"{base}/streamers/{{login}}/verify",
            _bind(server, "streamer_verify"),
        ),
        web.post(
            f"{base}/streamers/{{login}}/archive",
            _bind(server, "streamer_archive"),
        ),
        web.post(
            f"{base}/streamers/{{login}}/discord-flag",
            _bind(server, "streamer_discord_flag"),
        ),
        web.post(
            f"{base}/streamers/{{login}}/discord-profile",
            _bind(server, "streamer_discord_profile"),
        ),
        web.get(f"{base}/stats", _bind(server, "stats")),
        web.get(
            f"{base}/analytics/streamer/{{login}}",
            _bind(server, "streamer_analytics"),
        ),
        web.get(
            f"{base}/analytics/comparison",
            _bind(server, "analytics_comparison"),
        ),
        web.get(
            f"{base}/sessions/{{session_id}}",
            _bind(server, "session_detail"),
        ),
    ]


def attach_streamer_routes(app: web.Application, server: Any) -> None:
    app.add_routes(build_streamer_route_defs(server))


__all__ = [
    "INTERNAL_API_BASE_PATH",
    "attach_streamer_routes",
    "build_streamer_route_defs",
]
