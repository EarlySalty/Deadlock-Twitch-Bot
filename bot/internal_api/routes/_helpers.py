"""Shared helpers for internal API route handlers."""

from __future__ import annotations

from typing import Any, Callable

from aiohttp import web


def bind(server: Any, handler: Callable[[Any, web.Request], Any]) -> Callable[[web.Request], Any]:
    """Bind a handler function to a server instance.

    Creates a closure that passes the server to the handler function.
    Used to adapt handler(server, request) into aiohttp's handler(request) signature.
    """
    async def _handler(request: web.Request) -> web.StreamResponse:
        return await handler(server, request)

    return _handler


__all__ = [
    "bind",
]
