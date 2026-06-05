"""Global-Ban-Liste: interne API (Add / Remove / Check / List).

Parallel zum Raid-Blacklist-Endpoint, aber für die netzwerkweite Chatter-
Bannliste (``twitch_chatter_global_ban``). Einträge werden vom proaktiven
Offline-Sweep über alle Partner-Kanäle durchgesetzt -- und reaktiv, falls der
Gelistete schreibt, bevor der Sweep den Kanal erreicht hat.

Anders als der Raid-Blacklist-Endpoint braucht das hier keinen laufenden
Raid-Bot: eine Listen-Mutation ist reine DB-Operation, daher rufen die Handler
die Storage-Funktionen direkt auf. Das Hinzufügen spiegelt den Eintrag
zusätzlich in die Raid-Blacklist (Einbahn: global -> raid).
"""

from __future__ import annotations

from typing import Any

from aiohttp import web

from ...core.constants import log
from ..contracts import INTERNAL_API_BASE_PATH
from ._helpers import bind


async def global_ban_add(server: Any, request: web.Request) -> web.Response:
    try:
        body = await server._json_body(request)
        login = server._normalize_login(
            str(body.get("login") or body.get("twitch_login") or "")
        )
        if not login:
            return server._json_error("bad_request", 400, "invalid or missing login")
        reason = (
            str(body.get("reason") or "manual_ban:absolut").strip() or "manual_ban:absolut"
        )
        chatter_id = (
            str(body.get("chatter_id") or body.get("twitch_user_id") or "").strip() or None
        )
        from ...storage import pg

        pg.add_chatter_global_ban(login, chatter_id, reason, "internal_api")
        return server._json_response({"ok": True, "login": login, "reason": reason})
    except Exception:
        log.exception("internal api global ban add failed")
        return server._json_error("internal_error", 500, "failed to add to global ban list")


async def global_ban_remove(server: Any, request: web.Request) -> web.Response:
    try:
        body = await server._json_body(request)
        login = server._normalize_login(
            str(body.get("login") or body.get("twitch_login") or "")
        )
        if not login:
            return server._json_error("bad_request", 400, "invalid or missing login")
        from ...storage import pg

        removed = pg.remove_chatter_global_ban(login)
        return server._json_response(
            {"ok": True, "login": login, "removed": bool(removed)}
        )
    except Exception:
        log.exception("internal api global ban remove failed")
        return server._json_error(
            "internal_error", 500, "failed to remove from global ban list"
        )


async def global_ban_check(server: Any, request: web.Request) -> web.Response:
    try:
        login = server._normalize_login(str(request.query.get("login") or ""))
        if not login:
            return server._json_error("bad_request", 400, "invalid or missing login")
        from ...storage import pg

        banned = pg.is_chatter_globally_banned(login, "")
        return server._json_response({"ok": True, "login": login, "banned": bool(banned)})
    except Exception:
        log.exception("internal api global ban check failed")
        return server._json_error("internal_error", 500, "failed to check global ban list")


async def global_ban_list(server: Any, request: web.Request) -> web.Response:
    try:
        from ...storage import pg

        entries = pg.list_chatter_global_bans()
        return server._json_response({"ok": True, "entries": entries or []})
    except Exception:
        log.exception("internal api global ban list failed")
        return server._json_error("internal_error", 500, "failed to list global ban")


def build_global_ban_route_defs(server: Any) -> list[web.RouteDef]:
    base = str(
        getattr(server, "_base_path", INTERNAL_API_BASE_PATH) or INTERNAL_API_BASE_PATH
    ).rstrip("/")
    return [
        web.post(f"{base}/globalban/add", bind(server, global_ban_add)),
        web.post(f"{base}/globalban/remove", bind(server, global_ban_remove)),
        web.get(f"{base}/globalban/check", bind(server, global_ban_check)),
        web.get(f"{base}/globalban", bind(server, global_ban_list)),
    ]


def attach_global_ban_routes(app: web.Application, server: Any) -> None:
    app.add_routes(build_global_ban_route_defs(server))


__all__ = [
    "attach_global_ban_routes",
    "build_global_ban_route_defs",
    "global_ban_add",
    "global_ban_check",
    "global_ban_list",
    "global_ban_remove",
]
