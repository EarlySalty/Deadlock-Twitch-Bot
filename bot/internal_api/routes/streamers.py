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
    return [
        web.get(f"{INTERNAL_API_BASE_PATH}/streamers", _bind(server, "streamers")),
        web.post(f"{INTERNAL_API_BASE_PATH}/streamers", _bind(server, "streamer_add")),
        web.delete(
            f"{INTERNAL_API_BASE_PATH}/streamers/{{login}}",
            _bind(server, "streamer_remove"),
        ),
        web.post(
            f"{INTERNAL_API_BASE_PATH}/streamers/{{login}}/verify",
            _bind(server, "streamer_verify"),
        ),
        web.post(
            f"{INTERNAL_API_BASE_PATH}/streamers/{{login}}/archive",
            _bind(server, "streamer_archive"),
        ),
        web.post(
            f"{INTERNAL_API_BASE_PATH}/streamers/{{login}}/discord-flag",
            _bind(server, "streamer_discord_flag"),
        ),
        web.post(
            f"{INTERNAL_API_BASE_PATH}/streamers/{{login}}/discord-profile",
            _bind(server, "streamer_discord_profile"),
        ),
        web.get(f"{INTERNAL_API_BASE_PATH}/stats", _bind(server, "stats")),
        web.get(
            f"{INTERNAL_API_BASE_PATH}/analytics/streamer/{{login}}",
            _bind(server, "streamer_analytics"),
        ),
        web.get(
            f"{INTERNAL_API_BASE_PATH}/analytics/comparison",
            _bind(server, "analytics_comparison"),
        ),
        web.get(
            f"{INTERNAL_API_BASE_PATH}/sessions/{{session_id}}",
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
