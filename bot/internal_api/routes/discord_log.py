"""Internal route: relay a self-explainer Q&A embed to Discord via the master broker.

Worker und Dashboard sind headless (kein lokaler Discord-Client). Der Worker hat
aber `MASTER_BROKER_TOKEN`, das (eingeschränkter gescopte) Dashboard nicht. Daher
baut das Dashboard das Embed und schickt es hierher; der Worker relayt es an den
Master-Broker (`/internal/master/v1/discord/send-rich-message`).

Auth: die Internal-API-Middleware schützt diese Route bereits (Loopback +
X-Internal-Token). Der ausgehende Broker-Call nutzt MASTER_BROKER_TOKEN.
"""

from __future__ import annotations

import os
from typing import Any

from aiohttp import web

from ...core.constants import log
from ..contracts import INTERNAL_API_BASE_PATH
from ._helpers import bind

_MASTER_BROKER_DISCORD_PATH = "/internal/master/v1/discord/send-rich-message"


def _master_broker_base_url() -> str:
    explicit = (os.getenv("MASTER_BROKER_BASE_URL") or "").strip()
    if explicit:
        return explicit.rstrip("/")
    host = (os.getenv("MASTER_BROKER_HOST") or "127.0.0.1").strip() or "127.0.0.1"
    port = (os.getenv("MASTER_BROKER_PORT") or "8770").strip() or "8770"
    return f"http://{host}:{port}"


async def discord_self_explainer_log(server: Any, request: web.Request) -> web.Response:
    try:
        body = await request.json()
    except Exception:
        return web.json_response({"ok": False, "error": "invalid_json"}, status=400)

    channel_id = body.get("channel_id")
    embed = body.get("embed")
    if not channel_id or not isinstance(embed, dict):
        return web.json_response(
            {"ok": False, "error": "channel_id_and_embed_required"}, status=400
        )

    token = (os.getenv("MASTER_BROKER_TOKEN") or "").strip()
    if not token:
        log.warning("internal_api: MASTER_BROKER_TOKEN fehlt — self-explainer Discord-Log übersprungen")
        return web.json_response({"ok": False, "error": "master_broker_token_missing"}, status=503)

    payload = {
        "channel_id": int(channel_id),
        "content": body.get("content"),
        "embed": embed,
        "allowed_role_ids": [],
        "view_spec": None,
    }
    url = f"{_master_broker_base_url()}{_MASTER_BROKER_DISCORD_PATH}"

    import aiohttp

    try:
        async with aiohttp.ClientSession() as session:
            async with session.post(
                url,
                json=payload,
                headers={"X-Internal-Token": token, "Content-Type": "application/json"},
                timeout=aiohttp.ClientTimeout(total=10),
            ) as resp:
                ok = resp.status < 300
                if not ok:
                    detail = (await resp.text())[:200]
                    log.warning("internal_api: self-explainer Broker status=%s body=%s", resp.status, detail)
                return web.json_response(
                    {"ok": ok, "broker_status": resp.status}, status=200 if ok else 502
                )
    except Exception as exc:
        log.warning("internal_api: self-explainer Broker-Post fehlgeschlagen", exc_info=True)
        return web.json_response(
            {"ok": False, "error": "broker_post_failed", "detail": str(exc)[:200]}, status=502
        )


def build_discord_log_route_defs(server: Any) -> list[web.RouteDef]:
    base = str(getattr(server, "_base_path", INTERNAL_API_BASE_PATH) or INTERNAL_API_BASE_PATH).rstrip("/")
    return [
        web.post(f"{base}/discord/self-explainer-log", bind(server, discord_self_explainer_log)),
    ]


def attach_discord_log_routes(app: web.Application, server: Any) -> None:
    app.add_routes(build_discord_log_route_defs(server))


__all__ = [
    "attach_discord_log_routes",
    "build_discord_log_route_defs",
    "discord_self_explainer_log",
]
