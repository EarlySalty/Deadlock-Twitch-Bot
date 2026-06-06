"""Internal route: relay a self-explainer Q&A embed to Discord via the master broker.

Worker und Dashboard sind headless (kein lokaler Discord-Client); nur der
Master-Prozess hält den Discord-Client und betreibt den Master-Broker (8770).
Das Dashboard baut das Embed und schickt es hierher; der Worker relayt es an den
Master-Broker (`/internal/master/v1/discord/send-rich-message`).

Auth: die Internal-API-Middleware schützt diese Route bereits (Loopback +
X-Internal-Token). Der ausgehende Broker-Call nutzt dieselbe Token-Fallback-Kette
wie der Live-Announcement-Pfad (`monitoring.py`) und der Broker selbst
(`bot_core/master_bot.py` im Master): MASTER_BROKER_TOKEN → MAIN_BOT_INTERNAL_TOKEN
→ TWITCH_INTERNAL_API_TOKEN. Letzterer ist im Worker ohnehin vorhanden, daher
braucht es kein separates Broker-Secret.
"""

from __future__ import annotations

import hashlib
import json
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


def _master_broker_token() -> str:
    """Broker-Auth-Token mit identischer Fallback-Kette wie Master + Live-Announcements.

    Der Master-Broker (`bot_core/master_bot.py`) leitet seinen Auth-Token aus genau
    dieser Reihenfolge ab; der Worker hat zwar kein eigenes MASTER_BROKER_TOKEN, wohl
    aber TWITCH_INTERNAL_API_TOKEN — denselben geteilten Token, den der Broker dann
    akzeptiert. Reihenfolge muss synchron zu Master/`monitoring.py` bleiben.
    """
    for key in ("MASTER_BROKER_TOKEN", "MAIN_BOT_INTERNAL_TOKEN", "TWITCH_INTERNAL_API_TOKEN"):
        value = (os.getenv(key) or "").strip()
        if value:
            return value
    return ""


def _idempotency_key(payload: dict[str, Any]) -> str:
    """Stabiler Dedup-Key aus dem Payload — der Broker verlangt X-Idempotency-Key.

    Gleiche Logik wie der Broker selbst (`_payload_hash`): kanonisches JSON + sha256.
    Der Embed-Footer trägt den `peer`, daher kollidieren nur echte Wiederholungen
    desselben Besuchers mit identischer Frage+Antwort (gewollter Dedup); ≤128 Zeichen.
    """
    encoded = json.dumps(payload, ensure_ascii=True, sort_keys=True, separators=(",", ":"))
    digest = hashlib.sha256(encoded.encode("utf-8")).hexdigest()
    return f"self-explainer:{digest[:48]}"


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

    token = _master_broker_token()
    if not token:
        log.warning(
            "internal_api: kein Broker-Token (MASTER_BROKER_TOKEN/MAIN_BOT_INTERNAL_TOKEN/"
            "TWITCH_INTERNAL_API_TOKEN) — self-explainer Discord-Log übersprungen"
        )
        return web.json_response({"ok": False, "error": "master_broker_token_missing"}, status=503)

    payload = {
        "channel_id": int(channel_id),
        "content": body.get("content"),
        "embed": embed,
        "allowed_role_ids": [],
        "view_spec": None,
    }
    url = f"{_master_broker_base_url()}{_MASTER_BROKER_DISCORD_PATH}"
    headers = {
        "X-Internal-Token": token,
        "X-Idempotency-Key": _idempotency_key(payload),
        "Content-Type": "application/json",
    }

    import aiohttp

    try:
        async with aiohttp.ClientSession() as session:
            async with session.post(
                url,
                json=payload,
                headers=headers,
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
