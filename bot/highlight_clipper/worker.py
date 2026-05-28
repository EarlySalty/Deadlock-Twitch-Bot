from __future__ import annotations

import asyncio
import logging
import shutil
import time
from pathlib import Path

from .config import CLIPS_DIR
from .config import CLIP_PADDING_SECONDS
from .config import POLL_INTERVAL_SECONDS
from .deadlock_client import get_match_history
from .deadlock_client import get_match_metadata
from .dm_sender import send_highlight_to_channel
from .event_detector import HighlightEvent
from .event_detector import detect_events
from .state import is_match_processed
from .state import load_state
from .state import mark_match_processed
from .twitch_vod import download_clip
from .twitch_vod import find_vod_for_match
from .twitch_vod import get_channel_id

log = logging.getLogger("TwitchStreams.HighlightClipper")

_STEAM64_BASE = 76561197960265728
_STEAM_LINKS_DB = Path("/home/naniadm/Documents/Deadlock-Bots/data/deadlock.sqlite3")

# Alle aktiven Partner mit Discord-User-ID aus der Twitch-Bot-DB
_PARTNERS_QUERY = """
    SELECT twitch_login, discord_user_id
      FROM twitch_streamers_partner_state
     WHERE is_partner_active = 1
       AND discord_user_id IS NOT NULL
     ORDER BY twitch_login
"""


class HighlightClipperWorker:
    def __init__(self, bot, *, cog=None) -> None:
        self.bot = bot
        self._cog = cog
        self._task: asyncio.Task[None] | None = None
        Path(CLIPS_DIR).mkdir(parents=True, exist_ok=True)

    async def start(self) -> None:
        if self._task is not None and not self._task.done():
            return
        self._task = asyncio.create_task(self._loop(), name="twitch.highlight_clipper")
        log.info("HighlightClipper worker started")

    async def stop(self) -> None:
        task = self._task
        self._task = None
        if task is None:
            return
        task.cancel()
        try:
            await task
        except asyncio.CancelledError:
            pass
        log.info("HighlightClipper worker stopped")

    async def _loop(self) -> None:
        await self.bot.wait_until_ready()
        while not self.bot.is_closed():
            try:
                await self._run_once()
            except asyncio.CancelledError:
                raise
            except Exception:
                log.exception("HighlightClipper: Fehler im Worker")
            await asyncio.sleep(POLL_INTERVAL_SECONDS)

    async def _run_once(self) -> None:
        twitch_api = self._get_twitch_api()
        if twitch_api is None:
            log.warning("HighlightClipper: TwitchAPI nicht verfügbar, überspringe")
            return

        streamers = await self._get_partner_streamers()
        if not streamers:
            log.info("HighlightClipper: Keine aktiven Partner mit Steam-ID gefunden")
            return
        log.info("HighlightClipper: %s Partner werden verarbeitet", len(streamers))

        state = load_state()
        now = int(time.time())

        for twitch_login, steam_id in streamers:
            try:
                await self._process_streamer(
                    state=state,
                    twitch_login=twitch_login,
                    steam_id=int(steam_id),
                    twitch_api=twitch_api,
                    now=now,
                )
            except asyncio.CancelledError:
                raise
            except Exception:
                log.exception("HighlightClipper: Fehler bei Streamer %s", twitch_login)

    async def _process_streamer(
        self,
        *,
        state: dict,
        twitch_login: str,
        steam_id: int,
        twitch_api,
        now: int,
    ) -> None:
        matches = await get_match_history(steam_id, limit=10)
        recent_matches = _filter_recent_matches(matches, state, login=twitch_login, now=now)
        if not recent_matches:
            return

        channel_id = await get_channel_id(twitch_login, twitch_api)
        if not channel_id:
            log.warning("HighlightClipper: Kein Twitch-Channel für %s", twitch_login)
            return

        for match in recent_matches:
            match_id = int(match["match_id"])
            clip_dir = Path(CLIPS_DIR) / twitch_login / str(match_id)
            clip_dir.mkdir(parents=True, exist_ok=True)
            try:
                await self._process_match(
                    state=state,
                    twitch_login=twitch_login,
                    steam_id=steam_id,
                    match=match,
                    channel_id=channel_id,
                    twitch_api=twitch_api,
                    clip_dir=clip_dir,
                )
            finally:
                shutil.rmtree(clip_dir, ignore_errors=True)

    async def _process_match(
        self,
        *,
        state: dict,
        twitch_login: str,
        steam_id: int,
        match: dict,
        channel_id: str,
        twitch_api,
        clip_dir: Path,
    ) -> None:
        match_id = int(match["match_id"])
        match_start_unix = int(match["start_time"])
        match_duration_s = int(match.get("match_duration_s") or 0)

        match_info = await get_match_metadata(match_id)
        events = detect_events(steam_id, match_info)
        if not events:
            mark_match_processed(state, twitch_login, match_id)
            return

        vod = await find_vod_for_match(
            channel_id,
            match_start_unix,
            match_duration_s,
            twitch_api,
        )
        if vod is None:
            log.warning(
                "HighlightClipper: Kein VOD für %s match_id=%s",
                twitch_login,
                match_id,
            )
            mark_match_processed(state, twitch_login, match_id)
            return

        clip_paths: list[str] = []
        clip_events: list[HighlightEvent] = []
        vod_offset_s = match_start_unix - int(vod["vod_started_at"])

        for index, event in enumerate(events, start=1):
            clip_start_s = max(0, vod_offset_s + event.game_time_s - CLIP_PADDING_SECONDS)
            clip_end_s = max(
                clip_start_s + 1,
                vod_offset_s + event.game_time_s + event.duration_s + CLIP_PADDING_SECONDS,
            )
            output_path = clip_dir / f"{index:02d}_{event.event_type}_{event.game_time_s}.mp4"
            downloaded = await download_clip(
                str(vod["vod_id"]),
                clip_start_s,
                clip_end_s,
                str(output_path),
            )
            if not downloaded:
                continue
            clip_paths.append(str(output_path))
            clip_events.append(event)

        if clip_paths:
            await send_highlight_to_channel(
                self.bot,
                twitch_login,
                match_id,
                clip_events,
                clip_paths,
            )
        else:
            log.warning(
                "HighlightClipper: Keine Clips erstellt für %s match_id=%s",
                twitch_login,
                match_id,
            )

        mark_match_processed(state, twitch_login, match_id)

    def _get_twitch_api(self):
        if self._cog is not None:
            return getattr(self._cog, "api", None)
        cog = getattr(self.bot, "cogs", {}).get("TwitchStreamCog")
        return getattr(cog, "api", None) if cog is not None else None

    async def _get_partner_streamers(self) -> list[tuple[str, str]]:
        loop = asyncio.get_event_loop()
        try:
            rows = await loop.run_in_executor(None, _query_partner_streamers)
        except Exception:
            log.exception("HighlightClipper: DB-Abfrage für Partner fehlgeschlagen")
            rows = []

        # Discord user_id → account_id aus Steam-Bot SQLite auflösen
        discord_to_account: dict[int, str] = {}
        try:
            discord_ids = [int(row[1]) for row in (rows or []) if row[1]]
            if discord_ids:
                discord_to_account = await loop.run_in_executor(
                    None, _load_steam_account_ids, discord_ids
                )
        except Exception:
            log.exception("HighlightClipper: Steam-Links-Abfrage fehlgeschlagen")

        result: dict[str, str] = {}
        for row in (rows or []):
            login = str(row[0] or "").strip()
            discord_id = _as_int(row[1])
            if not login or discord_id is None:
                continue
            account_id = discord_to_account.get(discord_id)
            if account_id:
                result[login] = account_id

        result.update(_load_manual_steamids())
        return list(result.items())


def _load_manual_steamids() -> dict[str, str]:
    """Lädt manuelle Steam-ID-Zuordnungen aus data/highlight_clipper/steamids.json."""
    import json
    path = Path("data/highlight_clipper/steamids.json")
    if not path.exists():
        return {}
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
        if isinstance(data, dict):
            return {str(k): str(v) for k, v in data.items() if k and v}
    except Exception:
        log.warning("HighlightClipper: steamids.json konnte nicht gelesen werden")
    return {}


def _query_partner_streamers() -> list:
    from ..storage.pg import query_all
    return query_all(_PARTNERS_QUERY) or []


def _load_steam_account_ids(discord_ids: list[int]) -> dict[int, str]:
    """Liest primary Steam-Account-IDs aus der Steam-Bot-SQLite für gegebene Discord-User-IDs."""
    import sqlite3
    if not _STEAM_LINKS_DB.exists():
        log.warning("HighlightClipper: Steam-Links-DB nicht gefunden: %s", _STEAM_LINKS_DB)
        return {}
    placeholders = ",".join("?" * len(discord_ids))
    conn = sqlite3.connect(f"file:{_STEAM_LINKS_DB}?mode=ro", uri=True)
    try:
        rows = conn.execute(
            f"SELECT user_id, steam_id FROM steam_links"
            f" WHERE user_id IN ({placeholders}) AND primary_account = 1",
            discord_ids,
        ).fetchall()
    finally:
        conn.close()
    result: dict[int, str] = {}
    for user_id, steam_id in rows:
        try:
            account_id = int(steam_id) - _STEAM64_BASE
            if account_id > 0:
                result[int(user_id)] = str(account_id)
        except (ValueError, TypeError):
            pass
    return result


def _filter_recent_matches(
    matches: list[dict],
    state: dict,
    *,
    login: str,
    now: int,
) -> list[dict]:
    min_start = now - 86400
    filtered: list[dict] = []
    for match in matches:
        if not isinstance(match, dict):
            continue
        match_id = _as_int(match.get("match_id"))
        start_time = _as_int(match.get("start_time"))
        if match_id is None or start_time is None:
            continue
        if start_time <= min_start or is_match_processed(state, login, match_id):
            continue
        filtered.append(
            {
                "match_id": match_id,
                "start_time": start_time,
                "match_duration_s": _as_int(match.get("match_duration_s")) or 0,
            }
        )
    filtered.sort(key=lambda item: item["start_time"])
    return filtered


def _as_int(value: object) -> int | None:
    try:
        return int(value)
    except (TypeError, ValueError):
        return None
