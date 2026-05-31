"""Live-Match-Kontext (Deadlock) für Engagement-Layer.

Pollt deadlock-api.com für den aktuellen Match-State eines Streamers (über
Steam-ID aus twitch_engagement_settings.steam_id). Persistiert in
twitch_channel_match_state. Pipeline liest den Snapshot synchron aus der DB
und hängt einen kurzen "Streamer spielt aktuell X"-Hint in den System-Prompt.

V1-Heuristik für "is_live": match_end_ts fehlt, duration_s ist 0/None,
match_started_at < 90 Min her.
"""

from __future__ import annotations

import asyncio
import logging
import time
from dataclasses import dataclass
from datetime import datetime, timezone

import httpx

from bot.storage.pg import query_one, transaction

log = logging.getLogger("TwitchStreams.Engagement.MatchContext")


DEADLOCK_API_BASE = "https://api.deadlock-api.com/v1"
ASSETS_API_BASE = "https://assets.deadlock-api.com"

_HERO_CACHE: dict[int, str] = {}
_HERO_CACHE_LOADED_AT: float = 0.0
_HERO_CACHE_TTL_SEC = 6 * 3600.0  # 6h


@dataclass(slots=True)
class MatchSnapshot:
    channel_login: str
    hero_id: int | None
    hero_name: str | None
    match_id: str | None
    match_started_at: datetime | None
    last_synced_at: datetime | None
    is_live: bool

    def to_prompt_fragment(self) -> str:
        if not self.is_live:
            return ""
        if self.hero_name:
            hero = self.hero_name
        elif self.hero_id is not None:
            hero = f"Hero #{self.hero_id}"
        else:
            hero = "einem unbekannten Hero"
        if self.match_started_at:
            elapsed_min = int(
                (datetime.now(timezone.utc) - self.match_started_at).total_seconds() // 60
            )
            duration = f" Match läuft seit ~{elapsed_min} Min."
        else:
            duration = ""
        return f"Streamer spielt aktuell {hero}.{duration}"


def _sync_load_match_state(channel_login: str) -> MatchSnapshot | None:
    row = query_one(
        """
        SELECT channel_login, hero_id, hero_name, match_id,
               match_started_at, last_synced_at, is_live
        FROM twitch_channel_match_state
        WHERE channel_login = %s
        """,
        [channel_login],
    )
    if row is None:
        return None
    return MatchSnapshot(
        channel_login=row[0],
        hero_id=row[1],
        hero_name=row[2],
        match_id=str(row[3]) if row[3] else None,
        match_started_at=row[4],
        last_synced_at=row[5],
        is_live=bool(row[6]),
    )


async def get_match_state(channel_login: str) -> MatchSnapshot | None:
    return await asyncio.to_thread(_sync_load_match_state, channel_login)


async def _fetch_heroes() -> dict[int, str]:
    # Heldenliste liegt auf der Assets-API; api.deadlock-api.com/v1/heroes ist 404.
    try:
        async with httpx.AsyncClient(timeout=10.0, follow_redirects=True) as client:
            r = await client.get(
                f"{ASSETS_API_BASE}/v2/heroes", params={"only_active": "true"}
            )
            r.raise_for_status()
            data = r.json()
    except Exception:
        log.warning("MatchContext: Hero-Liste konnte nicht geladen werden", exc_info=False)
        return {}
    out: dict[int, str] = {}
    if not isinstance(data, list):
        return out
    for item in data:
        if not isinstance(item, dict):
            continue
        hid = item.get("id")
        name = item.get("name") or item.get("display_name")
        if hid is not None and name:
            try:
                out[int(hid)] = str(name)
            except (TypeError, ValueError):
                continue
    return out


async def _ensure_hero_cache() -> dict[int, str]:
    global _HERO_CACHE_LOADED_AT
    now = time.time()
    if _HERO_CACHE and (now - _HERO_CACHE_LOADED_AT) < _HERO_CACHE_TTL_SEC:
        return _HERO_CACHE
    fresh = await _fetch_heroes()
    if fresh:
        _HERO_CACHE.clear()
        _HERO_CACHE.update(fresh)
        _HERO_CACHE_LOADED_AT = now
    return _HERO_CACHE


async def _fetch_last_match(steam_id: str) -> dict | None:
    try:
        async with httpx.AsyncClient(timeout=10.0) as client:
            r = await client.get(
                f"{DEADLOCK_API_BASE}/players/{steam_id}/match-history",
                params={"limit": 1},
            )
            r.raise_for_status()
            data = r.json()
    except Exception:
        log.warning(
            "MatchContext: match-history fehlgeschlagen für %s", steam_id, exc_info=False
        )
        return None
    if not isinstance(data, list) or not data:
        return None
    item = data[0]
    return item if isinstance(item, dict) else None


def _parse_ts(value) -> datetime | None:
    if value is None:
        return None
    if isinstance(value, (int, float)):
        try:
            return datetime.fromtimestamp(int(value), tz=timezone.utc)
        except (OverflowError, ValueError):
            return None
    if isinstance(value, str):
        try:
            return datetime.fromisoformat(value.replace("Z", "+00:00"))
        except ValueError:
            return None
    return None


def _sync_upsert_match_state(
    *,
    channel_login: str,
    hero_id: int | None,
    hero_name: str | None,
    match_id: str | None,
    match_started_at: datetime | None,
    is_live: bool,
) -> None:
    with transaction() as conn:
        conn.execute(
            """
            INSERT INTO twitch_channel_match_state
                (channel_login, hero_id, hero_name, match_id,
                 match_started_at, last_synced_at, is_live)
            VALUES (%s, %s, %s, %s, %s, NOW(), %s)
            ON CONFLICT (channel_login) DO UPDATE SET
                hero_id = EXCLUDED.hero_id,
                hero_name = EXCLUDED.hero_name,
                match_id = EXCLUDED.match_id,
                match_started_at = EXCLUDED.match_started_at,
                last_synced_at = NOW(),
                is_live = EXCLUDED.is_live;
            """,
            [
                channel_login,
                hero_id,
                hero_name,
                match_id,
                match_started_at,
                is_live,
            ],
        )


async def poll_match_state(channel_login: str, steam_id: str) -> MatchSnapshot | None:
    """API-Poll + Persistierung. Returns aktuellen Snapshot oder None."""
    if not steam_id:
        return None
    item = await _fetch_last_match(steam_id)
    if item is None:
        return await get_match_state(channel_login)

    hero_id_raw = item.get("hero_id")
    match_id_raw = item.get("match_id")
    start_ts_raw = (
        item.get("start_time")
        or item.get("match_start")
        or item.get("started_at")
        or item.get("start_time_iso")
    )
    end_ts_raw = (
        item.get("end_time")
        or item.get("match_end")
        or item.get("ended_at")
        or item.get("end_time_iso")
    )
    duration_s = item.get("duration_s") or item.get("duration") or 0

    match_started_at = _parse_ts(start_ts_raw)

    is_live = False
    if match_started_at and not end_ts_raw and (not duration_s):
        age = (datetime.now(timezone.utc) - match_started_at).total_seconds()
        if 0 < age < 90 * 60:
            is_live = True

    hero_id: int | None = None
    if hero_id_raw is not None:
        try:
            hero_id = int(hero_id_raw)
        except (TypeError, ValueError):
            hero_id = None

    hero_name: str | None = None
    if hero_id is not None:
        cache = await _ensure_hero_cache()
        hero_name = cache.get(hero_id)

    await asyncio.to_thread(
        _sync_upsert_match_state,
        channel_login=channel_login,
        hero_id=hero_id,
        hero_name=hero_name,
        match_id=str(match_id_raw) if match_id_raw is not None else None,
        match_started_at=match_started_at,
        is_live=is_live,
    )
    return await get_match_state(channel_login)
