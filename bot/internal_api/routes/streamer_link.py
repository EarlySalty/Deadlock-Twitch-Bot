"""Streamer-Discord-Verknüpfung: interne API (Kandidaten-Liste).

Liefert dem Discord-Bot-seitigen Matcher die Streamer, die noch keine
``discord_user_id`` tragen. Der Matcher gleicht diese Logins gegen die
Discord-Memberliste ab, bewertet die Übereinstimmung per AI und verknüpft
sichere Treffer über den bestehenden ``/streamers/{login}/discord-profile``-
Endpunkt zurück. Hier wird also nur gelesen -- reine DB-Operation, daher ruft
der Handler die Storage-Funktion direkt auf (analog zum Global-Ban-Endpoint).
"""

from __future__ import annotations

from typing import Any

from aiohttp import web

from ...core.constants import log
from ..contracts import INTERNAL_API_BASE_PATH
from ._helpers import bind


async def link_candidates(server: Any, request: web.Request) -> web.Response:
    del request
    try:
        from ...storage import pg

        entries = pg.list_unlinked_streamers()
        return server._json_response({"ok": True, "entries": entries or []})
    except Exception:
        log.exception("internal api link candidates failed")
        return server._json_error(
            "internal_error", 500, "failed to list link candidates"
        )


def build_streamer_link_route_defs(server: Any) -> list[web.RouteDef]:
    base = str(
        getattr(server, "_base_path", INTERNAL_API_BASE_PATH) or INTERNAL_API_BASE_PATH
    ).rstrip("/")
    return [
        web.get(f"{base}/streamers/link-candidates", bind(server, link_candidates)),
    ]


def attach_streamer_link_routes(app: web.Application, server: Any) -> None:
    app.add_routes(build_streamer_link_route_defs(server))


__all__ = [
    "attach_streamer_link_routes",
    "build_streamer_link_route_defs",
    "link_candidates",
]
