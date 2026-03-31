"""Operational and telemetry routes for the internal API."""

from __future__ import annotations

from functools import partial
from typing import Any

from aiohttp import web

from ...app_keys import ANALYTICS_DB_FINGERPRINT_DETAILS_KEY
from ...core.constants import log

INTERNAL_API_BASE_PATH = "/internal/twitch/v1"


async def healthz(server: Any, request: web.Request) -> web.Response:
    analytics_db = request.app.get(ANALYTICS_DB_FINGERPRINT_DETAILS_KEY) or {}
    return server._json_response(
        {
            "ok": True,
            "service": "twitch-internal-api",
            "analyticsDbFingerprint": analytics_db.get("fingerprint"),
            "analyticsDb": analytics_db,
        }
    )


async def observability_debug(server: Any, request: web.Request) -> web.Response:
    analytics_db = request.app.get(ANALYTICS_DB_FINGERPRINT_DETAILS_KEY) or {}
    try:
        payload = await server._observability_snapshot()
        if not isinstance(payload, dict):
            payload = {"value": payload}
        return server._json_response(
            {
                "ok": True,
                "service": "twitch-internal-api",
                "analyticsDbFingerprint": analytics_db.get("fingerprint"),
                "observability": payload,
            }
        )
    except ValueError as exc:
        return server._safe_exception_error(
            context="observability snapshot",
            exc=exc,
            error="internal_error",
            status=500,
            message="failed to build observability snapshot",
        )
    except Exception:
        log.exception("internal api observability snapshot failed")
        return server._json_error(
            "internal_error",
            500,
            "failed to build observability snapshot",
        )


async def chatters_debug(server: Any, request: web.Request) -> web.Response:
    raw_login = request.match_info.get("login")
    login = server._normalize_login(raw_login or "")
    if not login:
        return server._json_error("invalid_login", 400, "invalid twitch login")

    analytics_db = request.app.get(ANALYTICS_DB_FINGERPRINT_DETAILS_KEY) or {}
    try:
        payload = await server._chatters_debug(login)
        if not isinstance(payload, dict):
            payload = {"value": payload}
        return server._json_response(
            {
                "ok": True,
                "service": "twitch-internal-api",
                "analyticsDbFingerprint": analytics_db.get("fingerprint"),
                "chattersDebug": payload,
            }
        )
    except ValueError as exc:
        return server._safe_exception_error(
            context="chatters debug",
            exc=exc,
            error="internal_error",
            status=500,
            message="failed to build chatters debug payload",
        )
    except Exception:
        log.exception("internal api chatters debug failed")
        return server._json_error(
            "internal_error",
            500,
            "failed to build chatters debug payload",
        )


async def live_active_announcements(server: Any, request: web.Request) -> web.Response:
    del request
    try:
        items = await server._live_active_announcements()
        if not isinstance(items, list):
            items = list(items) if items else []
        normalized = [server._normalize_live_announcement_item(item) for item in items]
        return server._json_response(normalized)
    except ValueError as exc:
        return server._safe_exception_error(
            context="live active announcements",
            exc=exc,
            error="internal_error",
            status=500,
            message="failed to list active live announcements",
        )
    except Exception:
        log.exception("internal api live active announcements failed")
        return server._json_error(
            "internal_error",
            500,
            "failed to list active live announcements",
        )


async def live_link_click(server: Any, request: web.Request) -> web.Response:
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

        streamer_login = server._normalize_login(str(body.get("streamer_login") or ""))
        if not streamer_login:
            raise ValueError("invalid streamer_login")

        tracking_token = server._normalize_tracking_token(
            body.get("tracking_token"),
            required=True,
        )
        discord_user_id = server._normalize_discord_user_id_param(
            body.get("discord_user_id"),
            required=True,
        )
        discord_username = server._normalize_text_field(
            body.get("discord_username"),
            field_name="discord_username",
            required=True,
            max_length=200,
        )
        guild_id = server._coerce_optional_positive_int(body.get("guild_id"), key="guild_id")
        channel_id = server._coerce_optional_positive_int(body.get("channel_id"), key="channel_id")
        if channel_id is None:
            raise ValueError("invalid channel_id")
        message_id = server._coerce_optional_positive_int(body.get("message_id"), key="message_id")
        if message_id is None:
            raise ValueError("invalid message_id")
        source_hint = server._normalize_text_field(
            body.get("source_hint"),
            field_name="source_hint",
            required=True,
            max_length=100,
        )

        await server._live_link_click(
            streamer_login=streamer_login,
            tracking_token=tracking_token,
            discord_user_id=discord_user_id,
            discord_username=discord_username,
            guild_id=str(guild_id) if guild_id is not None else None,
            channel_id=str(channel_id),
            message_id=str(message_id),
            source_hint=source_hint,
        )

        owner_cacheable = True
        owner_response = server._json_response({"ok": True})
        return owner_response
    except ValueError as exc:
        owner_response = server._safe_bad_request(
            context="live link click",
            exc=exc,
            message="invalid request body",
        )
        return owner_response
    except PermissionError as exc:
        owner_response = server._safe_exception_error(
            context="live link click forbidden",
            exc=exc,
            error="forbidden",
            status=403,
            message="action outside configured scope",
        )
        return owner_response
    except Exception:
        log.exception("internal api live link click failed")
        owner_response = server._json_error(
            "internal_error",
            500,
            "failed to persist live link click",
        )
        return owner_response
    finally:
        server._release_idempotency_owner(
            key=owner_key,
            fingerprint=owner_fingerprint,
            response=owner_response,
            cacheable=owner_cacheable,
        )


async def eventsub_dispatch(server: Any, request: web.Request) -> web.Response:
    try:
        body = await server._json_body(request)
        sub_type = str(body.get("sub_type") or "").strip()
        if not sub_type:
            raise ValueError("invalid or missing sub_type")
        raw_message_id = body.get("message_id")
        message_id = str(raw_message_id).strip() if raw_message_id is not None else None
        payload = body.get("payload")
        if not isinstance(payload, dict):
            raise ValueError("invalid payload")
        result = await server._eventsub_dispatch(
            sub_type=sub_type,
            message_id=message_id,
            payload=payload,
        )
        if not isinstance(result, dict):
            result = {"ok": True}
        if result.get("ok") is False:
            return server._json_error(
                "upstream_unavailable",
                503,
                str(result.get("message") or "eventsub dispatch unavailable"),
            )
        return server._json_response(result)
    except ValueError as exc:
        return server._safe_bad_request(
            context="eventsub dispatch",
            exc=exc,
            message="invalid request body",
        )
    except RuntimeError as exc:
        detail = server._safe_bad_request_detail(exc)
        return server._safe_exception_error(
            context="eventsub dispatch runtime",
            exc=exc,
            error="upstream_unavailable",
            status=503,
            message=detail or "upstream unavailable",
        )
    except Exception:
        log.exception("internal api eventsub dispatch failed")
        return server._json_error(
            "internal_error",
            500,
            "failed to dispatch eventsub notification",
        )


async def eventsub_processing_debug(server: Any, request: web.Request) -> web.Response:
    try:
        raw_limit = request.query.get("limit")
        limit = int(str(raw_limit or "20").strip() or "20")
        if limit < 1 or limit > 200:
            raise ValueError("invalid limit")
        payload = await server._eventsub_processing_debug(limit=limit)
        if not isinstance(payload, dict):
            payload = {"value": payload}
        return server._json_response({"ok": True, "eventsubProcessing": payload})
    except ValueError as exc:
        return server._safe_bad_request(
            context="eventsub processing debug",
            exc=exc,
            message="invalid request",
        )
    except Exception:
        log.exception("internal api eventsub processing debug failed")
        return server._json_error(
            "internal_error",
            500,
            "failed to build eventsub processing payload",
        )


async def eventsub_processing_requeue(server: Any, request: web.Request) -> web.Response:
    try:
        body = await server._json_body(request)
        work_id = str(body.get("work_id") or "").strip()
        if not work_id:
            raise ValueError("invalid or missing work_id")
        payload = await server._eventsub_processing_requeue(work_id)
        if not isinstance(payload, dict):
            payload = {"ok": True, "workId": work_id, "requeued": True}
        return server._json_response(payload)
    except ValueError as exc:
        return server._safe_bad_request(
            context="eventsub processing requeue",
            exc=exc,
            message="invalid request body",
        )
    except Exception:
        log.exception("internal api eventsub processing requeue failed")
        return server._json_error(
            "internal_error",
            500,
            "failed to requeue eventsub processing entry",
        )


def build_telemetry_route_defs(server: Any) -> list[web.RouteDef]:
    base = str(getattr(server, "_base_path", INTERNAL_API_BASE_PATH) or INTERNAL_API_BASE_PATH).rstrip("/")
    return [
        web.get(f"{base}/healthz", partial(healthz, server)),
        web.get(f"{base}/debug/observability", partial(observability_debug, server)),
        web.get(f"{base}/debug/eventsub-processing", partial(eventsub_processing_debug, server)),
        web.get(f"{base}/debug/chatters/{{login}}", partial(chatters_debug, server)),
        web.get(f"{base}/live/active-announcements", partial(live_active_announcements, server)),
        web.post(f"{base}/live/link-click", partial(live_link_click, server)),
        web.post(f"{base}/eventsub/dispatch", partial(eventsub_dispatch, server)),
        web.post(f"{base}/eventsub/processing/requeue", partial(eventsub_processing_requeue, server)),
    ]


def attach_telemetry_routes(app: web.Application, server: Any) -> None:
    app.add_routes(build_telemetry_route_defs(server))


__all__ = [
    "attach_telemetry_routes",
    "build_telemetry_route_defs",
    "chatters_debug",
    "eventsub_dispatch",
    "eventsub_processing_debug",
    "eventsub_processing_requeue",
    "healthz",
    "live_active_announcements",
    "live_link_click",
    "observability_debug",
]
