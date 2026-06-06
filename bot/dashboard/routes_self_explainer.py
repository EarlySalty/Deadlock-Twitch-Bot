"""Öffentlicher Frage-Box-Endpoint für /streamer: erklärt den Bot.

Ein (oft skeptischer) Streamer tippt auf der Website eine Frage; dieser Endpoint
beantwortet sie grounded über `bot.chat.self_explainer` (strikt aus dem
Steckbrief, kein Erfinden) und protokolliert Frage + Antwort dauerhaft:
- in die DB (`twitch_self_explainer_log`) und
- best-effort als Discord-Embed über die bestehende Discord-Bot-Integration des
  Dashboards in den Protokoll-Channel 1374364800817303632 (kein Webhook).

Öffentlich (kein Login — /streamer ist öffentlich), aber per-IP rate-limitiert
und durch das Grounding/den gehärteten System-Prompt gegen Prompt-Injection
abgesichert. Logging-Fehler brechen die Antwort nie ab.
"""

from __future__ import annotations

import asyncio
import logging
from typing import Any

import discord
from aiohttp import web

from bot.chat.self_explainer import (
    FALLBACK_UNSURE,
    SelfExplainerAnswer,
    answer_question,
    split_message,
)
from bot.core.constants import TWITCH_ALERT_CHANNEL_ID
from bot.storage import pg as storage

log = logging.getLogger("TwitchStreams.Dashboard.SelfExplainer")

# Protokoll-Channel (= TWITCH_ALERT_CHANNEL_ID, 1374...): jede Frage+Antwort wird
# über die bestehende Discord-Bot-Integration des Dashboards hierher gespiegelt
# (kein Webhook), damit die Konversationen sichtbar sind.
_LOG_CHANNEL_ID = TWITCH_ALERT_CHANNEL_ID

_HARD_MAX_QUESTION = 1000
# Reasoning-Modell braucht bei vollen Antworten Zeit; Token-Budget ist großzügig,
# also dem Modell auch zeitlich Luft geben statt vorzeitig in den Fallback zu kippen.
_ANSWER_TIMEOUT_SEC = 55.0

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


def _build_discord_embed(question: str, result: SelfExplainerAnswer, peer: str) -> discord.Embed:
    color = (
        discord.Color.red()
        if result.flagged_injection
        else (discord.Color.green() if result.grounded else discord.Color.gold())
    )
    embed = discord.Embed(title="Frage-Box: neue Frage zum Bot", color=color)
    embed.add_field(name="Frage", value=(question or "—")[:1024], inline=False)
    answer_parts = split_message(result.answer or "—", 1000) or ["—"]
    if len(answer_parts) == 1:
        embed.add_field(name="Antwort", value=answer_parts[0][:1024], inline=False)
    else:
        for idx, part in enumerate(answer_parts, 1):
            embed.add_field(
                name=f"Antwort ({idx}/{len(answer_parts)})",
                value=part[:1024],
                inline=False,
            )
    embed.add_field(
        name="Quelle",
        value="Steckbrief (grounded)" if result.grounded else "Fallback (Generik)",
        inline=True,
    )
    if result.flagged_injection:
        embed.add_field(name="⚠️", value="Injection-Marker erkannt", inline=True)
    embed.set_footer(text=f"peer: {peer}")
    return embed


async def _post_discord_via_bot(
    server: Any, question: str, result: SelfExplainerAnswer, peer: str
) -> None:
    """Spiegelt Frage+Antwort über den bestehenden Discord-Bot des Dashboards in den
    Protokoll-Channel. Best-effort — bricht die Antwort an den Besucher nie ab."""
    resolver = getattr(server, "_dashboard_discord_bot", None)
    discord_bot = resolver() if callable(resolver) else None
    if discord_bot is None:
        log.debug("self_explainer: Discord-Bot nicht verfügbar, überspringe Channel-Log")
        return
    try:
        channel = discord_bot.get_channel(_LOG_CHANNEL_ID)
        if channel is None and hasattr(discord_bot, "fetch_channel"):
            channel = await discord_bot.fetch_channel(_LOG_CHANNEL_ID)
        if channel is None:
            log.debug("self_explainer: Protokoll-Channel %s nicht gefunden", _LOG_CHANNEL_ID)
            return
        await channel.send(embed=_build_discord_embed(question, result, peer))
    except Exception:
        log.debug("self_explainer: Discord-Channel-Post fehlgeschlagen", exc_info=True)


async def _safe_log(
    server: Any, question: str, result: SelfExplainerAnswer, peer: str
) -> None:
    loop = asyncio.get_running_loop()
    try:
        await loop.run_in_executor(None, _log_to_db_sync, question, result, peer)
    except Exception:
        log.debug("self_explainer: DB-Log fehlgeschlagen", exc_info=True)
    try:
        asyncio.create_task(_post_discord_via_bot(server, question, result, peer))
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

    await _safe_log(server, question, result, peer)
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
