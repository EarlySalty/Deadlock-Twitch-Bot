# bot/title_generator/steam_lookup.py
from __future__ import annotations

import os
from typing import Any

import aiosqlite

STEAM_DB_PATH = os.environ.get(
    "STEAM_BOT_DB_PATH",
    os.path.expanduser("~/Documents/Deadlock/service/deadlock.sqlite3"),
)

_RANK_NAMES = {
    0: "Obscurus",
    1: "Seeker",
    2: "Alchemist",
    3: "Arcanist",
    4: "Ritualist",
    5: "Emissary",
    6: "Archon",
    7: "Oracle",
    8: "Phantom",
    9: "Ascendant",
    10: "Eternus",
    11: "Eternus",
}


async def _fetch_rank_row(discord_user_id: int) -> dict | None:
    async with aiosqlite.connect(STEAM_DB_PATH) as db:
        db.row_factory = aiosqlite.Row
        async with db.execute(
            """
            SELECT sl.deadlock_rank, sl.deadlock_subrank
            FROM steam_links sl
            WHERE sl.discord_user_id = ?
            LIMIT 1
            """,
            (discord_user_id,),
        ) as cur:
            row = await cur.fetchone()
    return dict(row) if row else None


async def _fetch_live_row(discord_user_id: int) -> dict | None:
    async with aiosqlite.connect(STEAM_DB_PATH) as db:
        db.row_factory = aiosqlite.Row
        async with db.execute(
            """
            SELECT lps.in_deadlock_now, lps.in_match_now_strict,
                   lps.deadlock_hero, lps.deadlock_party_hint, lps.deadlock_stage
            FROM steam_links sl
            JOIN live_player_state lps ON sl.steam_id = lps.steam_id
            WHERE sl.discord_user_id = ?
            LIMIT 1
            """,
            (discord_user_id,),
        ) as cur:
            row = await cur.fetchone()
    return dict(row) if row else None


async def get_rank_for_discord_user(discord_user_id: int) -> dict[str, Any] | None:
    """Return rank info for a Discord user, or None if not linked."""
    row = await _fetch_rank_row(discord_user_id)
    if not row:
        return None
    rank_num = row.get("deadlock_rank") or 0
    return {
        "rank_name": _RANK_NAMES.get(rank_num, "Unknown"),
        "rank_num": rank_num,
        "subrank": row.get("deadlock_subrank") or 0,
        "rank_display": f"{_RANK_NAMES.get(rank_num, 'Unknown')} {row.get('deadlock_subrank') or ''}".strip(),
    }


async def get_live_state_for_discord_user(discord_user_id: int) -> dict[str, Any] | None:
    """Return live in-game state if currently in Deadlock, else None."""
    row = await _fetch_live_row(discord_user_id)
    if not row or not row.get("in_deadlock_now"):
        return None
    return {
        "in_match": bool(row.get("in_match_now_strict")),
        "hero": row.get("deadlock_hero"),
        "party_hint": row.get("deadlock_party_hint"),
        "stage": row.get("deadlock_stage"),
    }
