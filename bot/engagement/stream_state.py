"""Gate: Engagement nur, wenn der Channel GERADE live ist UND Deadlock streamt.

Liest ``twitch_live_state`` (vom Live-Monitoring des Bots gepflegt: ``is_live`` +
``last_game``). Damit redet der Bot nicht in Streams, die offline sind oder gerade
ein anderes Spiel / Just Chatting laufen haben. Kurz gecacht (60s), damit nicht pro
Nachricht die DB getroffen wird.
"""

from __future__ import annotations

import asyncio
import time

from bot.storage.pg import query_one

_CACHE: dict[str, tuple[float, bool]] = {}
_TTL_SEC = 60.0


def _sync_check(channel_login: str) -> bool:
    row = query_one(
        "SELECT is_live, last_game FROM twitch_live_state WHERE streamer_login = %s",
        [channel_login],
    )
    if not row:
        return False
    is_live = bool(row[0])
    last_game = (row[1] or "").strip().lower()
    return is_live and last_game == "deadlock"


async def is_streaming_deadlock(channel_login: str) -> bool:
    cl = (channel_login or "").strip().lower()
    if not cl:
        return False
    now = time.time()
    cached = _CACHE.get(cl)
    if cached and (now - cached[0]) < _TTL_SEC:
        return cached[1]
    try:
        val = await asyncio.to_thread(_sync_check, cl)
    except Exception:
        return _CACHE.get(cl, (0.0, False))[1]
    _CACHE[cl] = (now, val)
    return val
