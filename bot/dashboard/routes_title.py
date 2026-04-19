from __future__ import annotations

import asyncio
from typing import Any

from aiohttp import web

from bot.storage import pg as storage
from bot.title_generator.steam_lookup import (
    get_live_state_for_discord_user,
    get_rank_for_discord_user,
)
from bot.title_generator.title_ai import RateLimitExceeded, generate_title
from bot.title_generator.title_db import (
    get_latest_insights,
    get_streamer_avg_viewers,
    get_streamer_title_history,
    get_top_knowledge_titles,
)

def _get_discord_user_id(twitch_user_id: str) -> int | None:
    """Look up discord_user_id for a streamer from twitch_streamers table."""
    with storage.readonly_connection() as conn:
        row = conn.execute(
            "SELECT discord_user_id FROM twitch_streamers WHERE twitch_user_id = %s LIMIT 1",
            (twitch_user_id,),
        ).fetchone()
    if not row or not row[0]:
        return None
    try:
        return int(row[0])
    except (ValueError, TypeError):
        return None


def _resolve_twitch_user_id_from_session(server: Any, request: web.Request) -> str:
    session = server._get_dashboard_session(request)
    if not session:
        return ""

    twitch_user_id = str(session.get("twitch_user_id") or "").strip()
    if twitch_user_id:
        return twitch_user_id

    twitch_login = str(session.get("twitch_login") or "").strip().lower()
    if not twitch_login:
        return ""

    with storage.readonly_connection() as conn:
        row = conn.execute(
            "SELECT twitch_user_id FROM twitch_streamers WHERE LOWER(twitch_login) = %s LIMIT 1",
            (twitch_login,),
        ).fetchone()
    return str(row[0]) if row and row[0] else ""


def _enrich_history_with_scores(
    history: list[dict[str, Any]],
    own_avg: float,
) -> list[dict[str, Any]]:
    for item in history:
        avg = item.get("avg_viewers") or 0
        followers = item.get("followers_start") or 1
        item["relative_perf"] = avg / own_avg if own_avg > 0 else 0.0
        item["engagement_rate"] = avg / followers
    return history


async def title_suggest(server: Any, request: web.Request) -> web.Response:
    session = server._get_dashboard_session(request)
    if not session:
        return web.json_response({"error": "unauthorized"}, status=401)

    twitch_user_id = _resolve_twitch_user_id_from_session(server, request)
    if not twitch_user_id:
        session_twitch_login = str(session.get("twitch_login") or "").strip().lower()
        if not session_twitch_login:
            return web.json_response({"error": "unauthorized"}, status=401)
        return web.json_response({"error": "streamer not found"}, status=404)

    try:
        body = await request.json()
    except Exception:
        return web.json_response({"error": "invalid json"}, status=400)

    keywords = str(body.get("keywords") or "").strip()
    if not keywords:
        return web.json_response({"error": "keywords required"}, status=400)
    include_live = bool(body.get("include_live", False))

    loop = asyncio.get_running_loop()
    history = await loop.run_in_executor(None, get_streamer_title_history, twitch_user_id, 30)
    own_avg = await loop.run_in_executor(None, get_streamer_avg_viewers, twitch_user_id)
    enriched_history = _enrich_history_with_scores(history, own_avg)
    knowledge_titles = await loop.run_in_executor(None, get_top_knowledge_titles, 30)

    discord_user_id = await loop.run_in_executor(None, _get_discord_user_id, twitch_user_id)
    rank_display: str | None = None
    live_state: dict[str, Any] | None = None
    if discord_user_id:
        rank_info = await get_rank_for_discord_user(discord_user_id)
        if rank_info:
            rank_display = str(rank_info.get("rank_display") or "").strip() or None
        if include_live:
            live_state = await get_live_state_for_discord_user(discord_user_id)

    try:
        result = await generate_title(
            streamer_id=twitch_user_id,
            keywords=keywords,
            title_history=enriched_history,
            knowledge_titles=knowledge_titles,
            rank_display=rank_display,
            live_state=live_state,
            source="dashboard",
        )
    except RateLimitExceeded as exc:
        return web.json_response(
            {"error": "rate_limit", "retry_after": exc.retry_after},
            status=429,
        )

    result["title_analysis"] = enriched_history[:20]
    return web.json_response(result)


async def title_insights(server: Any, request: web.Request) -> web.Response:
    session = server._get_dashboard_session(request)
    if not session:
        return web.json_response({"error": "unauthorized"}, status=401)

    twitch_user_id = _resolve_twitch_user_id_from_session(server, request)
    if not twitch_user_id:
        return web.json_response({"insight": None})

    loop = asyncio.get_running_loop()
    insight = await loop.run_in_executor(None, get_latest_insights, twitch_user_id)
    if not insight:
        return web.json_response({"insight": None})
    return web.json_response({"insight": insight})


def build_route_defs(server: Any) -> list[web.RouteDef]:
    return [
        web.post("/twitch/api/v2/title/suggest", lambda r: title_suggest(server, r)),
        web.get("/twitch/api/v2/title/insights", lambda r: title_insights(server, r)),
    ]
