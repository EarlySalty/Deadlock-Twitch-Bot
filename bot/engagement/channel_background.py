"""Per-Streamer-Background für den Engagement-Layer.

Ein Reflexions-Job destilliert pro Channel aus dessen Chats ein kurzes Profil:
welche Helden der Streamer offenbar spielt/mag, Spielstil falls erkennbar,
Community-Vibe, Running-Gags, wiederkehrende Themen. Soziales Kontext-Wissen —
KEINE harten Spielfakten (die bleiben bei Wiki/Patches/Stats). Persistiert in
``twitch_engagement_channel_profile``; die Pipeline injiziert nur das Profil des
gerade behandelten Channels, damit sich der Bot dort natürlich einfügt.

Halluzinations-sicher: nur was sich aus den echten Nachrichten ablesen lässt; im
Prompt als Kontext markiert ("nie auswendig aufsagen"), nicht als Faktenquelle.
"""

from __future__ import annotations

import asyncio
import logging
import re

from bot.storage.pg import query_all, query_one, transaction

log = logging.getLogger("TwitchStreams.Engagement.ChannelBackground")

_THINK_RE = re.compile(r"<think>.*?</think>", re.DOTALL | re.IGNORECASE)

_POOL_LIMIT = 200          # so viele jüngste user-msgs des Channels ansehen
_MIN_MSGS = 15             # darunter lohnt sich kein Profil
_BUILD_MAX_TOKENS = 3000
_PROFILE_MAX_CHARS = 800

_SYS = (
    "Du bist ein nüchterner Beobachter. Gib nur die verlangte Zusammenfassung, "
    "kein Vorwort, keine Meta."
)


def _build_prompt(streamer: str, lines: list[str]) -> str:
    block = "\n".join(f"- {m}" for m in lines)
    return (
        f"Hier echte Chat-Nachrichten aus dem Twitch-Channel von {streamer} (ein "
        "Deadlock-Streamer). Fass in 2-4 knappen Stichpunkten zusammen, was man über DIESEN "
        "Streamer und seine Community erkennt: welche Helden er offenbar spielt oder mag, "
        "Spielstil falls ablesbar, Running-Gags, wiederkehrende Themen, der allgemeine Vibe. "
        "NUR was sich aus den Nachrichten ablesen lässt, NICHTS erfinden, keine harten "
        "Spielfakten behaupten. Ist die Datenlage für einen Punkt zu dünn, lass ihn weg. "
        f"Sachlich.\n\nNachrichten:\n{block}"
    )


def _sync_channel_msgs(channel_login: str, limit: int) -> list[str]:
    rows = query_all(
        """
        SELECT content FROM twitch_engagement_conversation
        WHERE channel_login = %s AND role = 'user'
        ORDER BY ts DESC LIMIT %s
        """,
        [channel_login, int(limit)],
    )
    return [r[0].strip() for r in rows if r and r[0] and len(r[0].strip()) > 3]


def _sync_channels_with_data(min_msgs: int) -> list[str]:
    rows = query_all(
        """
        SELECT channel_login FROM twitch_engagement_conversation
        WHERE role = 'user'
        GROUP BY channel_login HAVING count(*) >= %s
        """,
        [int(min_msgs)],
    )
    return [r[0] for r in rows if r and r[0]]


def _sync_upsert(channel_login: str, text: str, count: int) -> None:
    with transaction() as conn:
        conn.execute(
            """
            INSERT INTO twitch_engagement_channel_profile (channel_login, profile_text, msg_count, updated_at)
            VALUES (%s, %s, %s, NOW())
            ON CONFLICT (channel_login) DO UPDATE
              SET profile_text = EXCLUDED.profile_text,
                  msg_count = EXCLUDED.msg_count,
                  updated_at = NOW()
            """,
            [channel_login, text, count],
        )


def _sync_load(channel_login: str) -> str | None:
    row = query_one(
        "SELECT profile_text FROM twitch_engagement_channel_profile WHERE channel_login = %s",
        [channel_login],
    )
    return row[0] if row and row[0] else None


async def rebuild_channel_profile(channel_login: str, *, minimax) -> str | None:
    lines = await asyncio.to_thread(_sync_channel_msgs, channel_login, _POOL_LIMIT)
    if len(lines) < _MIN_MSGS:
        return None
    try:
        oc = minimax._ensure_client()
        resp = await oc.chat.completions.create(
            model=minimax._model,
            messages=[
                {"role": "system", "content": _SYS},
                {"role": "user", "content": _build_prompt(channel_login, lines)},
            ],
            max_tokens=_BUILD_MAX_TOKENS,
            temperature=0.4,
        )
    except Exception:
        log.warning("ChannelBackground: MiniMax-Call (%s) fehlgeschlagen", channel_login, exc_info=False)
        return None
    raw = resp.choices[0].message.content if resp.choices else ""
    text = _THINK_RE.sub("", raw or "").strip()
    if not text:
        return None
    text = text[:_PROFILE_MAX_CHARS]
    await asyncio.to_thread(_sync_upsert, channel_login, text, len(lines))
    log.info("ChannelBackground: Profil für %s aktualisiert (%d msgs)", channel_login, len(lines))
    return text


async def rebuild_all_channel_profiles(*, minimax) -> int:
    channels = await asyncio.to_thread(_sync_channels_with_data, _MIN_MSGS)
    n = 0
    for ch in channels:
        if await rebuild_channel_profile(ch, minimax=minimax):
            n += 1
    if n:
        log.info("ChannelBackground: %d Profile aktualisiert", n)
    return n


async def get_channel_profile_fragment(channel_login: str) -> str:
    text = await asyncio.to_thread(_sync_load, channel_login)
    if not text:
        return ""
    return (
        "Das weißt du über diesen Channel und seinen Streamer (Kontext, damit du dich natürlich "
        "einfügst — niemals auswendig aufsagen, nur einfließen lassen wo's passt):\n"
        f"{text}"
    )
