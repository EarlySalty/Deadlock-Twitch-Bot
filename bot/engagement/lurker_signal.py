"""Stammgast-Lurker-Erkennung mit Themen-Ankern (Black-Magic-UX).

V1-Variante: Stammgast = mind. N user-Turns in den letzten 30 Tagen aus
`twitch_engagement_conversation`. Lurker = Stammgast, der in den letzten
10 Minuten NICHT gepostet hat. Pro Lurker werden bis zu 2 offene Threads als
Themen-Anker mitgegeben — damit der Bot subtle Themen in deren Richtung
legen kann, ohne sie je direkt zu adressieren.

Spätere V2: presence-callback (Helix `get_chatters` / IRCLurkerTracker), damit
nur tatsächlich verbundene Lurker im Hint landen.
"""

from __future__ import annotations

import asyncio
import time
from dataclasses import dataclass

from bot.storage.pg import query_all

from .threads import Thread, load_open_threads_for_user


@dataclass(slots=True)
class LurkerHint:
    twitch_user_id: str
    twitch_login: str
    top_threads: list[Thread]


_cache: dict[str, tuple[float, list[LurkerHint]]] = {}
_CACHE_TTL_SEC = 30.0


def _sync_known_silent_regulars(
    channel_login: str,
    min_messages: int,
    days: int,
    recent_minutes: int,
    limit: int,
) -> list[tuple]:
    return query_all(
        f"""
        WITH regulars AS (
            SELECT twitch_user_id,
                   MAX(twitch_login) AS twitch_login,
                   COUNT(*) AS msg_count
            FROM twitch_engagement_conversation
            WHERE channel_login = %s
              AND role = 'user'
              AND twitch_user_id IS NOT NULL
              AND ts > NOW() - INTERVAL '{int(days)} days'
            GROUP BY twitch_user_id
            HAVING COUNT(*) >= %s
        )
        SELECT r.twitch_user_id, r.twitch_login
        FROM regulars r
        WHERE NOT EXISTS (
            SELECT 1 FROM twitch_engagement_conversation c
            WHERE c.channel_login = %s
              AND c.role = 'user'
              AND c.twitch_user_id = r.twitch_user_id
              AND c.ts > NOW() - INTERVAL '{int(recent_minutes)} minutes'
        )
        ORDER BY r.msg_count DESC
        LIMIT %s
        """,
        [channel_login, min_messages, channel_login, limit],
    )


async def known_regulars_currently_lurking(
    channel_login: str,
    *,
    min_messages: int = 10,
    days: int = 30,
    recent_minutes: int = 10,
    limit: int = 5,
) -> list[LurkerHint]:
    """Stammgäste, die aktuell still sind (V1: nur conversation-buffer-basiert)."""
    now = time.time()
    cached = _cache.get(channel_login)
    if cached and (now - cached[0]) < _CACHE_TTL_SEC:
        return cached[1]

    rows = await asyncio.to_thread(
        _sync_known_silent_regulars,
        channel_login,
        min_messages,
        days,
        recent_minutes,
        limit,
    )

    hints: list[LurkerHint] = []
    for user_id, login in rows:
        threads = await load_open_threads_for_user(user_id, channel_login, limit=2)
        hints.append(
            LurkerHint(
                twitch_user_id=user_id,
                twitch_login=login,
                top_threads=threads,
            )
        )

    _cache[channel_login] = (now, hints)
    return hints


def lurker_hint_to_prompt_fragment(hints: list[LurkerHint]) -> str:
    if not hints:
        return ""
    lines = [
        "Folgende Stammgäste sind möglicherweise gerade still im Chat. "
        "Wenn das laufende Thema natürlich an einen ihrer Interessen-Fäden andockt, "
        "kannst du subtle Themen-Anker in diese Richtung legen — "
        "NIEMALS direkt adressieren (kein 'Hey X' / kein '@X'):"
    ]
    for h in hints:
        if h.top_threads:
            summaries = "; ".join(t.summary for t in h.top_threads[:2])
            lines.append(f"  - {h.twitch_login}: {summaries}")
        else:
            lines.append(f"  - {h.twitch_login} (kein konkreter Faden bekannt)")
    return "\n".join(lines)
