from __future__ import annotations

from aiohttp import web


def analytics_internal_error_response(
    *,
    error: str = "Analytics-Daten konnten nicht geladen werden.",
    code: str = "analytics_request_failed",
    status: int = 500,
) -> web.Response:
    return web.json_response(
        {
            "error": error,
            "code": code,
        },
        status=status,
    )
