"""Route group for dashboard settings actions."""

from __future__ import annotations

from typing import Any

from aiohttp import web

from . import abbo_routes as _abbo_routes


def build_route_defs(server: Any) -> list[web.RouteDef]:
    """Return route definitions for plan settings handlers."""
    return [
        web.post("/twitch/abbo/promo-settings", server.abbo_promo_settings),
        web.post("/twitch/abbo/lurker-tax-settings", server.abbo_lurker_tax_settings),
        web.post("/twitch/abbo/promo-message", server.abbo_promo_message),
    ]


async def abbo_promo_settings(server: Any, request: web.Request) -> web.StreamResponse:
    return await _abbo_routes.abbo_promo_settings(server, request)


async def abbo_lurker_tax_settings(server: Any, request: web.Request) -> web.StreamResponse:
    return await _abbo_routes.abbo_lurker_tax_settings(server, request)


async def abbo_promo_message(server: Any, request: web.Request) -> web.StreamResponse:
    return await _abbo_routes.abbo_promo_message(server, request)
