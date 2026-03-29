"""Raid route registration for the internal API."""

from __future__ import annotations

from typing import Any, Callable

from aiohttp import web

INTERNAL_API_BASE_PATH = "/internal/twitch/v1"


def _bind(server: Any, method_name: str) -> Callable[[web.Request], Any]:
    async def _handler(request: web.Request) -> web.StreamResponse:
        return await getattr(server, method_name)(request)

    return _handler


def build_raid_route_defs(server: Any) -> list[web.RouteDef]:
    return [
        web.get(f"{INTERNAL_API_BASE_PATH}/raid/auth-url", _bind(server, "raid_auth_url")),
        web.get(f"{INTERNAL_API_BASE_PATH}/raid/auth-state", _bind(server, "raid_auth_state")),
        web.get(f"{INTERNAL_API_BASE_PATH}/raid/block-state", _bind(server, "raid_block_state")),
        web.get(f"{INTERNAL_API_BASE_PATH}/raid/go-url", _bind(server, "raid_go_url")),
        web.post(
            f"{INTERNAL_API_BASE_PATH}/raid/requirements",
            _bind(server, "raid_requirements"),
        ),
        web.post(
            f"{INTERNAL_API_BASE_PATH}/raid/oauth-callback",
            _bind(server, "raid_oauth_callback"),
        ),
    ]


def attach_raid_routes(app: web.Application, server: Any) -> None:
    app.add_routes(build_raid_route_defs(server))


__all__ = [
    "INTERNAL_API_BASE_PATH",
    "attach_raid_routes",
    "build_raid_route_defs",
]
