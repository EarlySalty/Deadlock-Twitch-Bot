from __future__ import annotations

import logging

import aiohttp

from .config import HIGHLIGHT_DISCORD_CHANNEL_ID

log = logging.getLogger("TwitchStreams.HighlightClipper")

_HIGHLIGHT_API_URL = "http://127.0.0.1:8899/highlight-clips"
_API_TOKEN = "changeme-local"
_TIMEOUT = aiohttp.ClientTimeout(total=120)


async def send_highlight_to_channel(
    bot,
    streamer_login: str,
    match_id: int,
    events: list,
    clip_paths: list[str],
) -> None:
    payload = {
        "token": _API_TOKEN,
        "channel_id": HIGHLIGHT_DISCORD_CHANNEL_ID,
        "streamer_login": streamer_login,
        "match_id": match_id,
        "events": [
            {"event_type": e.event_type, "label": e.label, "game_time_s": e.game_time_s}
            for e in events
        ],
        "clip_paths": clip_paths,
    }
    try:
        async with aiohttp.ClientSession(timeout=_TIMEOUT) as session:
            async with session.post(_HIGHLIGHT_API_URL, json=payload) as resp:
                body = await resp.json()
                if not body.get("ok"):
                    log.error("HighlightClipper: highlight-clips API Fehler: %s", body)
                    return
        log.info(
            "HighlightClipper: %s Clips für %s match=%s gepostet",
            len(clip_paths), streamer_login, match_id,
        )
    except Exception:
        log.exception("HighlightClipper: Fehler beim Senden an highlight-clips API")
