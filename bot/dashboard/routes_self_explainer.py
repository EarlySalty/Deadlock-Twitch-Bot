"""Öffentlicher Frage-Box-Endpoint für /streamer: erklärt den Bot.

Ein (oft skeptischer) Streamer tippt auf der Website eine Frage; dieser Endpoint
beantwortet sie grounded über `bot.chat.self_explainer` (strikt aus dem
Steckbrief, kein Erfinden) und protokolliert Frage + Antwort dauerhaft:
- in die DB (`twitch_self_explainer_log`) und
- best-effort als Discord-Embed (Webhook `SELF_EXPLAINER_DISCORD_WEBHOOK`,
  zeigt auf den Protokoll-Channel 1374364800817303632).

Öffentlich (kein Login — /streamer ist öffentlich), aber per-IP rate-limitiert
und durch das Grounding/den gehärteten System-Prompt gegen Prompt-Injection
abgesichert. Logging-Fehler brechen die Antwort nie ab.
"""

from __future__ import annotations

import asyncio
import logging
import os
from typing import Any

from aiohttp import web

from bot.chat.self_explainer import (
    FALLBACK_UNSURE,
    SelfExplainerAnswer,
    answer_question,
    split_message,
)
from bot.storage import pg as storage

log = logging.getLogger("TwitchStreams.Dashboard.SelfExplainer")

DISCORD_WEBHOOK_ENV = "SELF_EXPLAINER_DISCORD_WEBHOOK"

_HARD_MAX_QUESTION = 1000
_ANSWER_TIMEOUT_SEC = 12.0

# Rate-Limit (in-memory, pro Prozess; reset bei Restart — reicht zur Abuse-Abwehr)
_RATE_WINDOW_SEC = 60.0
_RATE_MAX_HITS = 10


class _RateLimiter:
    """Sliding-Window-Limiter pro Peer. `allow(peer, now)` ist deterministisch testbar."""

    def __init__(self, *, window_sec: float, max_hits: int) -> None:
        self._window = float(window_sec)
        self._max = int(max_hits)
        self._hits: dict[str, list[float]] = {}

    def allow(self, peer: str, now: float) -> bool:
        recent = [t for t in self._hits.get(peer, ()) if now - t < self._window]
        if len(recent) >= self._max:
            self._hits[peer] = recent
            return False
        recent.append(now)
        self._hits[peer] = recent
        # Sanfte Speicherbremse: leere/abgelaufene Peers gelegentlich wegräumen.
        if len(self._hits) > 2048:
            self._hits = {
                p: [t for t in ts if now - t < self._window]
                for p, ts in self._hits.items()
                if any(now - t < self._window for t in ts)
            }
        return True


_rate_limiter = _RateLimiter(window_sec=_RATE_WINDOW_SEC, max_hits=_RATE_MAX_HITS)


def _peer(server: Any, request: web.Request) -> str:
    fn = getattr(server, "_peer_host", None)
    if callable(fn):
        try:
            value = str(fn(request) or "").strip()
            if value:
                return value
        except Exception:
            pass
    return str(getattr(request, "remote", "") or "unknown")


def _ensure_log_table(conn: Any) -> None:
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_self_explainer_log (
            id BIGSERIAL PRIMARY KEY,
            question TEXT NOT NULL,
            answer TEXT NOT NULL,
            grounded BOOLEAN NOT NULL DEFAULT FALSE,
            flagged_injection BOOLEAN NOT NULL DEFAULT FALSE,
            peer TEXT,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )
        """
    )


def _log_to_db_sync(question: str, result: SelfExplainerAnswer, peer: str) -> None:
    with storage.transaction() as conn:
        _ensure_log_table(conn)
        conn.execute(
            """
            INSERT INTO twitch_self_explainer_log
                (question, answer, grounded, flagged_injection, peer)
            VALUES (%s, %s, %s, %s, %s)
            """,
            (question, result.answer, result.grounded, result.flagged_injection, peer),
        )


def _build_discord_payload(question: str, result: SelfExplainerAnswer, peer: str) -> dict[str, Any]:
    color = 0xED4245 if result.flagged_injection else (0x57F287 if result.grounded else 0xFEE75C)
    fields: list[dict[str, Any]] = [
        {"name": "Frage", "value": (question or "—")[:1024], "inline": False},
    ]
    answer_parts = split_message(result.answer or "—", 1000) or ["—"]
    if len(answer_parts) == 1:
        fields.append({"name": "Antwort", "value": answer_parts[0][:1024], "inline": False})
    else:
        for idx, part in enumerate(answer_parts, 1):
            fields.append(
                {
                    "name": f"Antwort ({idx}/{len(answer_parts)})",
                    "value": part[:1024],
                    "inline": False,
                }
            )
    fields.append(
        {
            "name": "Quelle",
            "value": "Steckbrief (grounded)" if result.grounded else "Fallback (Generik)",
            "inline": True,
        }
    )
    if result.flagged_injection:
        fields.append({"name": "⚠️", "value": "Injection-Marker erkannt", "inline": True})
    return {
        "username": "Frage-Box",
        "embeds": [
            {
                "title": "Frage-Box: neue Frage zum Bot",
                "color": color,
                "fields": fields,
                "footer": {"text": f"peer: {peer}"},
            }
        ],
    }


async def _post_discord(question: str, result: SelfExplainerAnswer, peer: str) -> None:
    url = os.getenv(DISCORD_WEBHOOK_ENV) or None
    if not url:
        return
    payload = _build_discord_payload(question, result, peer)
    try:
        import aiohttp

        async with aiohttp.ClientSession() as session:
            async with session.post(
                url, json=payload, timeout=aiohttp.ClientTimeout(total=10)
            ) as resp:
                if resp.status >= 300:
                    log.debug("self_explainer: Discord-Webhook status=%s", resp.status)
    except Exception:
        log.debug("self_explainer: Discord-Webhook-Post fehlgeschlagen", exc_info=True)


async def _safe_log(question: str, result: SelfExplainerAnswer, peer: str) -> None:
    loop = asyncio.get_running_loop()
    try:
        await loop.run_in_executor(None, _log_to_db_sync, question, result, peer)
    except Exception:
        log.debug("self_explainer: DB-Log fehlgeschlagen", exc_info=True)
    try:
        asyncio.create_task(_post_discord(question, result, peer))
    except Exception:
        log.debug("self_explainer: Discord-Task konnte nicht gestartet werden", exc_info=True)


async def self_explainer_ask(server: Any, request: web.Request) -> web.Response:
    peer = _peer(server, request)
    if not _rate_limiter.allow(peer, asyncio.get_running_loop().time()):
        return web.json_response({"error": "rate_limit"}, status=429)

    try:
        body = await request.json()
    except Exception:
        return web.json_response({"error": "invalid json"}, status=400)

    question = str((body or {}).get("question") or "").strip()
    if not question:
        return web.json_response({"error": "question required"}, status=400)
    if len(question) > _HARD_MAX_QUESTION:
        question = question[:_HARD_MAX_QUESTION]

    try:
        result = await asyncio.wait_for(answer_question(question), timeout=_ANSWER_TIMEOUT_SEC)
    except asyncio.TimeoutError:
        result = SelfExplainerAnswer(FALLBACK_UNSURE, grounded=False, flagged_injection=False)
    except Exception:
        log.debug("self_explainer: answer_question fehlgeschlagen", exc_info=True)
        result = SelfExplainerAnswer(FALLBACK_UNSURE, grounded=False, flagged_injection=False)

    await _safe_log(question, result, peer)
    return web.json_response(
        {
            "answer": result.answer,
            "parts": split_message(result.answer),
            "grounded": result.grounded,
        }
    )


def build_route_defs(server: Any) -> list[web.RouteDef]:
    return [
        web.post("/twitch/api/v2/self-explainer/ask", lambda r: self_explainer_ask(server, r)),
    ]


__all__ = ["build_route_defs", "self_explainer_ask"]
