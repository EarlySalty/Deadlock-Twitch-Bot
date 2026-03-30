"""Streamer/admin route handling for the internal API.

This module owns request parsing, validation, normalization, and response
orchestration for streamer-facing internal API endpoints. The server object
still provides shared helpers and business callbacks, but the route layer is
now the entrypoint for HTTP-specific control flow.
"""

from __future__ import annotations

from typing import Any, Callable

from aiohttp import web

from ...core.constants import log

INTERNAL_API_BASE_PATH = "/internal/twitch/v1"


def _bind(server: Any, handler: Callable[[Any, web.Request], Any]) -> Callable[[web.Request], Any]:
    async def _handler(request: web.Request) -> web.StreamResponse:
        return await handler(server, request)

    return _handler


async def streamers(server: Any, request: web.Request) -> web.Response:
    del request
    try:
        items = await server._list()
        if not isinstance(items, list):
            items = list(items) if items else []
        return server._json_response(items)
    except Exception:
        log.exception("internal api streamers listing failed")
        return server._json_error("internal_error", 500, "failed to list streamers")


async def streamer_add(server: Any, request: web.Request) -> web.Response:
    owner_key = ""
    owner_fingerprint = ""
    owner_response: web.Response | None = None
    owner_cacheable = False
    try:
        body = await server._json_body(request)
        (
            idempotency_key,
            idempotency_fingerprint,
            replay,
            wait_future,
            is_owner,
        ) = server._prepare_idempotency(
            request=request,
            payload=body,
        )
        if replay is not None:
            return replay
        if wait_future is not None:
            return await server._wait_idempotency_result(future=wait_future)
        if is_owner:
            owner_key = idempotency_key
            owner_fingerprint = idempotency_fingerprint
        login = server._normalize_login(
            str(body.get("login") or body.get("streamer") or body.get("twitch_login") or "")
        )
        if not login:
            owner_response = server._json_error("bad_request", 400, "invalid or missing login")
            return owner_response
        require_link = server._parse_bool(body.get("require_link"), default=False)
        message = await server._add(login, require_link)
        owner_cacheable = True
        owner_response = server._json_response(
            {
                "ok": True,
                "login": login,
                "message": str(message or "added"),
            },
            status=201,
        )
        return owner_response
    except ValueError as exc:
        owner_response = server._safe_bad_request(
            context="add streamer",
            exc=exc,
            message="invalid request body",
        )
        return owner_response
    except Exception:
        log.exception("internal api add streamer failed")
        owner_response = server._json_error("internal_error", 500, "failed to add streamer")
        return owner_response
    finally:
        server._release_idempotency_owner(
            key=owner_key,
            fingerprint=owner_fingerprint,
            response=owner_response,
            cacheable=owner_cacheable,
        )


async def streamer_remove(server: Any, request: web.Request) -> web.Response:
    (
        idempotency_key,
        idempotency_fingerprint,
        replay,
        wait_future,
        is_owner,
    ) = server._prepare_idempotency(
        request=request,
        payload=None,
    )
    owner_key = idempotency_key if is_owner else ""
    owner_fingerprint = idempotency_fingerprint if is_owner else ""
    owner_response: web.Response | None = None
    owner_cacheable = False
    if replay is not None:
        return replay
    if wait_future is not None:
        return await server._wait_idempotency_result(future=wait_future)
    try:
        raw_login = request.match_info.get("login", "")
        login = server._normalize_login(raw_login)
        if not login:
            owner_response = server._json_error("bad_request", 400, "invalid login")
            return owner_response
        message = await server._remove(login)
        owner_cacheable = True
        owner_response = server._json_response({"ok": True, "login": login, "message": str(message or "removed")})
        return owner_response
    except Exception:
        log.exception("internal api remove streamer failed")
        owner_response = server._json_error("internal_error", 500, "failed to remove streamer")
        return owner_response
    finally:
        server._release_idempotency_owner(
            key=owner_key,
            fingerprint=owner_fingerprint,
            response=owner_response,
            cacheable=owner_cacheable,
        )


async def streamer_verify(server: Any, request: web.Request) -> web.Response:
    raw_login = request.match_info.get("login", "")
    login = server._normalize_login(raw_login)
    if not login:
        return server._json_error("bad_request", 400, "invalid login")
    owner_key = ""
    owner_fingerprint = ""
    owner_response: web.Response | None = None
    owner_cacheable = False
    try:
        body = await server._json_body(request)
        (
            idempotency_key,
            idempotency_fingerprint,
            replay,
            wait_future,
            is_owner,
        ) = server._prepare_idempotency(
            request=request,
            payload=body,
        )
        if replay is not None:
            return replay
        if wait_future is not None:
            return await server._wait_idempotency_result(future=wait_future)
        if is_owner:
            owner_key = idempotency_key
            owner_fingerprint = idempotency_fingerprint
        mode = str(body.get("mode") or "permanent").strip().lower()
        if not mode:
            mode = "permanent"
        message = await server._verify(login, mode)
        owner_cacheable = True
        owner_response = server._json_response({"ok": True, "login": login, "message": str(message or "verified")})
        return owner_response
    except ValueError as exc:
        owner_response = server._safe_bad_request(
            context="verify streamer",
            exc=exc,
            message="invalid request body",
        )
        return owner_response
    except Exception:
        log.exception("internal api verify streamer failed")
        owner_response = server._json_error("internal_error", 500, "failed to verify streamer")
        return owner_response
    finally:
        server._release_idempotency_owner(
            key=owner_key,
            fingerprint=owner_fingerprint,
            response=owner_response,
            cacheable=owner_cacheable,
        )


async def streamer_archive(server: Any, request: web.Request) -> web.Response:
    raw_login = request.match_info.get("login", "")
    login = server._normalize_login(raw_login)
    if not login:
        return server._json_error("bad_request", 400, "invalid login")
    owner_key = ""
    owner_fingerprint = ""
    owner_response: web.Response | None = None
    owner_cacheable = False
    try:
        body = await server._json_body(request)
        (
            idempotency_key,
            idempotency_fingerprint,
            replay,
            wait_future,
            is_owner,
        ) = server._prepare_idempotency(
            request=request,
            payload=body,
        )
        if replay is not None:
            return replay
        if wait_future is not None:
            return await server._wait_idempotency_result(future=wait_future)
        if is_owner:
            owner_key = idempotency_key
            owner_fingerprint = idempotency_fingerprint
        mode = str(body.get("mode") or "toggle").strip().lower()
        if not mode:
            mode = "toggle"
        message = await server._archive(login, mode)
        owner_cacheable = True
        owner_response = server._json_response({"ok": True, "login": login, "message": str(message or "updated")})
        return owner_response
    except ValueError as exc:
        owner_response = server._safe_bad_request(
            context="archive streamer",
            exc=exc,
            message="invalid request body",
        )
        return owner_response
    except Exception:
        log.exception("internal api archive streamer failed")
        owner_response = server._json_error("internal_error", 500, "failed to update archive state")
        return owner_response
    finally:
        server._release_idempotency_owner(
            key=owner_key,
            fingerprint=owner_fingerprint,
            response=owner_response,
            cacheable=owner_cacheable,
        )


async def streamer_discord_flag(server: Any, request: web.Request) -> web.Response:
    raw_login = request.match_info.get("login", "")
    login = server._normalize_login(raw_login)
    if not login:
        return server._json_error("bad_request", 400, "invalid login")
    owner_key = ""
    owner_fingerprint = ""
    owner_response: web.Response | None = None
    owner_cacheable = False
    try:
        body = await server._json_body(request)
        (
            idempotency_key,
            idempotency_fingerprint,
            replay,
            wait_future,
            is_owner,
        ) = server._prepare_idempotency(
            request=request,
            payload=body,
        )
        if replay is not None:
            return replay
        if wait_future is not None:
            return await server._wait_idempotency_result(future=wait_future)
        if is_owner:
            owner_key = idempotency_key
            owner_fingerprint = idempotency_fingerprint
        server._enforce_discord_action_scope(body)
        if "is_on_discord" not in body and "enabled" not in body and "value" not in body:
            owner_response = server._json_error("bad_request", 400, "is_on_discord is required")
            return owner_response
        enabled = server._parse_bool(
            body.get("is_on_discord", body.get("enabled", body.get("value"))),
            default=False,
        )
        message = await server._discord_flag(login, enabled)
        owner_cacheable = True
        owner_response = server._json_response({"ok": True, "login": login, "message": str(message or "updated")})
        return owner_response
    except ValueError as exc:
        owner_response = server._safe_bad_request(
            context="discord flag",
            exc=exc,
            message="invalid request body",
        )
        return owner_response
    except PermissionError as exc:
        owner_response = server._safe_exception_error(
            context="discord flag scope",
            exc=exc,
            error="forbidden",
            status=403,
            message="action outside configured scope",
        )
        return owner_response
    except Exception:
        log.exception("internal api discord flag failed")
        owner_response = server._json_error("internal_error", 500, "failed to update discord flag")
        return owner_response
    finally:
        server._release_idempotency_owner(
            key=owner_key,
            fingerprint=owner_fingerprint,
            response=owner_response,
            cacheable=owner_cacheable,
        )


async def streamer_discord_profile(server: Any, request: web.Request) -> web.Response:
    raw_login = request.match_info.get("login", "")
    login = server._normalize_login(raw_login)
    if not login:
        return server._json_error("bad_request", 400, "invalid login")
    owner_key = ""
    owner_fingerprint = ""
    owner_response: web.Response | None = None
    owner_cacheable = False
    try:
        body = await server._json_body(request)
        (
            idempotency_key,
            idempotency_fingerprint,
            replay,
            wait_future,
            is_owner,
        ) = server._prepare_idempotency(
            request=request,
            payload=body,
        )
        if replay is not None:
            return replay
        if wait_future is not None:
            return await server._wait_idempotency_result(future=wait_future)
        if is_owner:
            owner_key = idempotency_key
            owner_fingerprint = idempotency_fingerprint
        server._enforce_discord_action_scope(body)
        discord_user_id = body.get("discord_user_id")
        if discord_user_id is not None:
            discord_user_id = str(discord_user_id).strip() or None
        discord_display_name = body.get("discord_display_name")
        if discord_display_name is not None:
            discord_display_name = str(discord_display_name).strip() or None
        mark_member = server._parse_bool(body.get("mark_member", body.get("member_flag")), default=True)
        message = await server._discord_profile(
            login,
            discord_user_id=discord_user_id,
            discord_display_name=discord_display_name,
            mark_member=mark_member,
        )
        owner_cacheable = True
        owner_response = server._json_response({"ok": True, "login": login, "message": str(message or "updated")})
        return owner_response
    except ValueError as exc:
        owner_response = server._safe_bad_request(
            context="discord profile",
            exc=exc,
            message="invalid request body",
        )
        return owner_response
    except PermissionError as exc:
        owner_response = server._safe_exception_error(
            context="discord profile scope",
            exc=exc,
            error="forbidden",
            status=403,
            message="action outside configured scope",
        )
        return owner_response
    except Exception:
        log.exception("internal api discord profile failed")
        owner_response = server._json_error("internal_error", 500, "failed to update discord profile")
        return owner_response
    finally:
        server._release_idempotency_owner(
            key=owner_key,
            fingerprint=owner_fingerprint,
            response=owner_response,
            cacheable=owner_cacheable,
        )


async def stats(server: Any, request: web.Request) -> web.Response:
    try:
        hour_from = server._parse_optional_int(request.query.get("hour_from"), minimum=0)
        hour_to = server._parse_optional_int(request.query.get("hour_to"), minimum=0)
        streamer_raw = str(request.query.get("streamer") or "").strip()
        streamer = None
        if streamer_raw:
            streamer = server._normalize_login(streamer_raw)
            if streamer is None:
                return server._json_error("bad_request", 400, "invalid streamer login")
        payload = await server._stats(hour_from=hour_from, hour_to=hour_to, streamer=streamer)
        return server._json_response(payload if isinstance(payload, dict) else {})
    except ValueError as exc:
        return server._safe_bad_request(
            context="stats query",
            exc=exc,
            message="invalid query parameters",
        )
    except Exception:
        log.exception("internal api stats failed")
        return server._json_error("internal_error", 500, "failed to fetch stats")


async def streamer_analytics(server: Any, request: web.Request) -> web.Response:
    raw_login = request.match_info.get("login", "")
    login = server._normalize_login(raw_login)
    if not login:
        return server._json_error("bad_request", 400, "invalid login")
    try:
        days = server._parse_optional_int(request.query.get("days"), minimum=1) or 30
        payload = await server._streamer_analytics(login, int(days))
        return server._json_response(payload if isinstance(payload, dict) else {})
    except ValueError as exc:
        return server._safe_bad_request(
            context="streamer analytics query",
            exc=exc,
            message="invalid query parameters",
        )
    except Exception:
        log.exception("internal api streamer analytics failed")
        return server._json_error("internal_error", 500, "failed to fetch streamer analytics")


async def analytics_comparison(server: Any, request: web.Request) -> web.Response:
    try:
        days = server._parse_optional_int(request.query.get("days"), minimum=1) or 30
        payload = await server._comparison(int(days))
        return server._json_response(payload if isinstance(payload, dict) else {})
    except ValueError as exc:
        return server._safe_bad_request(
            context="comparison analytics query",
            exc=exc,
            message="invalid query parameters",
        )
    except Exception:
        log.exception("internal api comparison analytics failed")
        return server._json_error("internal_error", 500, "failed to fetch comparison analytics")


async def session_detail(server: Any, request: web.Request) -> web.Response:
    raw_session_id = request.match_info.get("session_id", "")
    try:
        session_id = int(str(raw_session_id).strip())
    except ValueError:
        return server._json_error("bad_request", 400, "invalid session id")
    try:
        payload = await server._session(session_id)
        if isinstance(payload, dict) and not payload:
            return server._json_error("not_found", 404, "session not found")
        return server._json_response(payload if isinstance(payload, dict) else {})
    except ValueError as exc:
        return server._safe_bad_request(
            context="session detail",
            exc=exc,
            message="invalid request parameters",
        )
    except Exception:
        log.exception("internal api session detail failed")
        return server._json_error("internal_error", 500, "failed to fetch session detail")


def build_streamer_route_defs(server: Any) -> list[web.RouteDef]:
    base = str(getattr(server, "_base_path", INTERNAL_API_BASE_PATH) or INTERNAL_API_BASE_PATH).rstrip("/")
    return [
        web.get(f"{base}/streamers", _bind(server, streamers)),
        web.post(f"{base}/streamers", _bind(server, streamer_add)),
        web.delete(f"{base}/streamers/{{login}}", _bind(server, streamer_remove)),
        web.post(f"{base}/streamers/{{login}}/verify", _bind(server, streamer_verify)),
        web.post(f"{base}/streamers/{{login}}/archive", _bind(server, streamer_archive)),
        web.post(f"{base}/streamers/{{login}}/discord-flag", _bind(server, streamer_discord_flag)),
        web.post(f"{base}/streamers/{{login}}/discord-profile", _bind(server, streamer_discord_profile)),
        web.get(f"{base}/stats", _bind(server, stats)),
        web.get(f"{base}/analytics/streamer/{{login}}", _bind(server, streamer_analytics)),
        web.get(f"{base}/analytics/comparison", _bind(server, analytics_comparison)),
        web.get(f"{base}/sessions/{{session_id}}", _bind(server, session_detail)),
    ]


def attach_streamer_routes(app: web.Application, server: Any) -> None:
    app.add_routes(build_streamer_route_defs(server))


__all__ = [
    "INTERNAL_API_BASE_PATH",
    "analytics_comparison",
    "attach_streamer_routes",
    "build_streamer_route_defs",
    "session_detail",
    "stats",
    "streamer_add",
    "streamer_analytics",
    "streamer_archive",
    "streamer_discord_flag",
    "streamer_discord_profile",
    "streamer_remove",
    "streamer_verify",
    "streamers",
]
