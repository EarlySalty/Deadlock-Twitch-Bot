"""Raid route handling for the internal API."""

from __future__ import annotations

from typing import Any

from aiohttp import web

from ...core.constants import log
from ..contracts import INTERNAL_API_BASE_PATH
from ._helpers import bind


async def raid_auth_url(server: Any, request: web.Request) -> web.Response:
    login = server._normalize_raid_auth_target(request.query.get("login", ""))
    if not login:
        return server._json_error("bad_request", 400, "invalid or missing login")
    discord_user_id = server._normalize_discord_user_id_param(
        request.query.get("discord_user_id"),
        required=False,
    )
    scope_profile = str(request.query.get("scope_profile") or "").strip() or None
    try:
        if login.startswith("discord:"):
            target_discord_user_id = login.split(":", 1)[1]
            if discord_user_id is not None and discord_user_id != target_discord_user_id:
                raise ValueError("discord_user_id does not match login target")
            discord_user_id = target_discord_user_id
        auth_url = await server._invoke_raid_auth_url(
            login,
            discord_user_id=discord_user_id,
            scope_profile=scope_profile,
        )
        if not auth_url:
            return server._json_error("upstream_unavailable", 503, "raid bot not initialized")
        return server._json_response({"ok": True, "auth_url": auth_url, "login": login})
    except ValueError as exc:
        return server._safe_bad_request(
            context="raid auth url",
            exc=exc,
            message="invalid request parameters",
        )
    except LookupError as exc:
        return server._safe_exception_error(
            context="raid auth url not found",
            exc=exc,
            error="not_found",
            status=404,
            message="resource not found",
        )
    except PermissionError as exc:
        return server._safe_exception_error(
            context="raid auth url forbidden",
            exc=exc,
            error="forbidden",
            status=403,
            message="forbidden",
        )
    except RuntimeError as exc:
        return server._safe_exception_error(
            context="raid auth url runtime",
            exc=exc,
            error="upstream_unavailable",
            status=503,
            message="upstream unavailable",
        )
    except Exception:
        log.exception("internal api raid auth url failed")
        return server._json_error("internal_error", 500, "failed to generate raid auth url")


async def raid_auth_state(server: Any, request: web.Request) -> web.Response:
    try:
        discord_user_id = server._normalize_discord_user_id_param(
            request.query.get("discord_user_id"),
            required=True,
        )
        payload = await server._raid_auth_state(discord_user_id)
        return server._json_response(
            {
                "ok": True,
                **server._normalize_raid_state_payload(
                    payload,
                    discord_user_id=discord_user_id,
                    twitch_login=None,
                ),
            }
        )
    except ValueError as exc:
        return server._safe_bad_request(
            context="raid auth state",
            exc=exc,
            message="invalid query parameters",
        )
    except Exception:
        log.exception("internal api raid auth state failed")
        return server._json_error("internal_error", 500, "failed to fetch raid auth state")


async def raid_block_state(server: Any, request: web.Request) -> web.Response:
    try:
        discord_user_id = server._normalize_discord_user_id_param(
            request.query.get("discord_user_id"),
            required=False,
        )
        raw_login = str(request.query.get("twitch_login") or "").strip()
        twitch_login = server._normalize_login(raw_login) if raw_login else None
        if raw_login and not twitch_login:
            raise ValueError("invalid twitch_login")
        if discord_user_id is None and not twitch_login:
            raise ValueError("discord_user_id or twitch_login is required")
        payload = await server._raid_block_state(
            discord_user_id=discord_user_id,
            twitch_login=twitch_login,
        )
        return server._json_response(
            {
                "ok": True,
                **server._normalize_raid_state_payload(
                    payload,
                    discord_user_id=discord_user_id,
                    twitch_login=twitch_login,
                ),
            }
        )
    except ValueError as exc:
        return server._safe_bad_request(
            context="raid block state",
            exc=exc,
            message="invalid query parameters",
        )
    except Exception:
        log.exception("internal api raid block state failed")
        return server._json_error("internal_error", 500, "failed to fetch raid block state")


async def raid_go_url(server: Any, request: web.Request) -> web.Response:
    state = str(request.query.get("state") or "").strip()
    if not state:
        return server._json_error("bad_request", 400, "missing state parameter")
    try:
        auth_url = await server._raid_go_url(state)
        auth_url_str = str(auth_url or "").strip()
        if not auth_url_str:
            return server._json_error("not_found", 404, "state not found or expired")
        return server._json_response({"ok": True, "auth_url": auth_url_str})
    except ValueError as exc:
        return server._safe_bad_request(
            context="raid go url",
            exc=exc,
            message="invalid request parameters",
        )
    except RuntimeError as exc:
        return server._safe_exception_error(
            context="raid go url runtime",
            exc=exc,
            error="upstream_unavailable",
            status=503,
            message="upstream unavailable",
        )
    except Exception:
        log.exception("internal api raid go url failed")
        return server._json_error("internal_error", 500, "failed to resolve raid auth url")


async def raid_requirements(server: Any, request: web.Request) -> web.Response:
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
        login = server._normalize_login(
            str(body.get("login") or body.get("streamer") or body.get("twitch_login") or "")
        )
        if not login:
            owner_response = server._json_error("bad_request", 400, "invalid or missing login")
            return owner_response
        message = await server._raid_requirements(login)
        owner_response = server._json_response({"ok": True, "login": login, "message": str(message or "sent")})
        owner_cacheable = True
        return owner_response
    except ValueError as exc:
        owner_response = server._safe_bad_request(
            context="raid requirements",
            exc=exc,
            message="invalid request body",
        )
        return owner_response
    except PermissionError as exc:
        owner_response = server._safe_exception_error(
            context="raid requirements forbidden",
            exc=exc,
            error="forbidden",
            status=403,
            message="action outside configured scope",
        )
        return owner_response
    except LookupError as exc:
        owner_response = server._safe_exception_error(
            context="raid requirements not found",
            exc=exc,
            error="not_found",
            status=404,
            message="resource not found",
        )
        return owner_response
    except RuntimeError as exc:
        owner_response = server._safe_exception_error(
            context="raid requirements runtime",
            exc=exc,
            error="upstream_unavailable",
            status=503,
            message="upstream unavailable",
        )
        return owner_response
    except Exception:
        log.exception("internal api raid requirements failed")
        owner_response = server._json_error("internal_error", 500, "failed to send raid requirements")
        return owner_response
    finally:
        server._release_idempotency_owner(
            key=owner_key,
            fingerprint=owner_fingerprint,
            response=owner_response,
            cacheable=owner_cacheable,
        )


async def raid_oauth_callback(server: Any, request: web.Request) -> web.Response:
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
        result = await server._raid_oauth_callback(
            code=str(body.get("code") or ""),
            state=str(body.get("state") or ""),
            error=str(body.get("error") or ""),
        )
        if not isinstance(result, dict):
            result = {
                "status": 500,
                "title": "Autorisierung fehlgeschlagen",
                "body_html": "<p>Ungültige Antwort vom Raid OAuth Callback.</p>",
            }
        status = result.get("status", 200)
        try:
            status_code = int(status)
        except (TypeError, ValueError):
            status_code = 200
        status_code = max(200, min(status_code, 599))
        result["status"] = status_code
        owner_response = server._json_response(result)
        owner_cacheable = True
        return owner_response
    except ValueError as exc:
        owner_response = server._safe_bad_request(
            context="raid oauth callback",
            exc=exc,
            message="invalid request body",
        )
        return owner_response
    except PermissionError as exc:
        owner_response = server._safe_exception_error(
            context="raid oauth callback forbidden",
            exc=exc,
            error="forbidden",
            status=403,
            message="action outside configured scope",
        )
        return owner_response
    except Exception:
        log.exception("internal api raid oauth callback failed")
        owner_response = server._json_error("internal_error", 500, "failed to process raid oauth callback")
        return owner_response
    finally:
        server._release_idempotency_owner(
            key=owner_key,
            fingerprint=owner_fingerprint,
            response=owner_response,
            cacheable=owner_cacheable,
        )


def build_raid_route_defs(server: Any) -> list[web.RouteDef]:
    base = str(getattr(server, "_base_path", INTERNAL_API_BASE_PATH) or INTERNAL_API_BASE_PATH).rstrip("/")
    return [
        web.get(f"{base}/raid/auth-url", bind(server, raid_auth_url)),
        web.get(f"{base}/raid/auth-state", bind(server, raid_auth_state)),
        web.get(f"{base}/raid/block-state", bind(server, raid_block_state)),
        web.get(f"{base}/raid/go-url", bind(server, raid_go_url)),
        web.post(f"{base}/raid/requirements", bind(server, raid_requirements)),
        web.post(f"{base}/raid/oauth-callback", bind(server, raid_oauth_callback)),
    ]


def attach_raid_routes(app: web.Application, server: Any) -> None:
    app.add_routes(build_raid_route_defs(server))


__all__ = [
    "INTERNAL_API_BASE_PATH",
    "attach_raid_routes",
    "build_raid_route_defs",
    "raid_auth_state",
    "raid_auth_url",
    "raid_block_state",
    "raid_go_url",
    "raid_oauth_callback",
    "raid_requirements",
]
