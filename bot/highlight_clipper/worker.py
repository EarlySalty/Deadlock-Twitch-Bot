from __future__ import annotations

import asyncio
import logging
import os
import shutil
import time
from pathlib import Path

from ..secret_store import load_secret_value
from .config import BETA_DISCORD_USER_ID
from .config import BETA_STEAM_ACCOUNT_ID
from .config import BETA_TWITCH_LOGIN
from .config import CLIPS_DIR
from .config import CLIP_PADDING_SECONDS
from .config import POLL_INTERVAL_SECONDS
from .deadlock_client import get_match_history
from .deadlock_client import get_match_metadata
from .dm_sender import send_highlight_dm
from .event_detector import HighlightEvent
from .event_detector import detect_events
from .state import is_match_processed
from .state import load_state
from .state import mark_match_processed
from .state import save_state
from .twitch_vod import download_clip
from .twitch_vod import find_vod_for_match
from .twitch_vod import get_channel_id

log = logging.getLogger("TwitchStreams.HighlightClipper")


class HighlightClipperWorker:
    def __init__(self, bot) -> None:
        self.bot = bot
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
        state = load_state()
        now = int(time.time())
        state["last_checked"] = now
        save_state(state)

        matches = await get_match_history(BETA_STEAM_ACCOUNT_ID, limit=10)
        recent_matches = _filter_recent_matches(matches, state, now=now)
        if not recent_matches:
            return

        client_id, access_token = self._load_twitch_credentials()
        if not client_id or not access_token:
            log.warning("HighlightClipper: Twitch credentials fehlen, Matches werden uebersprungen")
            return

        channel_id = await get_channel_id(BETA_TWITCH_LOGIN, client_id, access_token)
        if not channel_id:
            log.warning("HighlightClipper: Twitch channel id nicht gefunden fuer %s", BETA_TWITCH_LOGIN)
            return

        for match in recent_matches:
            match_id = int(match["match_id"])
            clip_dir = Path(CLIPS_DIR) / str(match_id)
            clip_dir.mkdir(parents=True, exist_ok=True)
            try:
                await self._process_match(
                    state=state,
                    match=match,
                    channel_id=channel_id,
                    client_id=client_id,
                    access_token=access_token,
                    clip_dir=clip_dir,
                )
            finally:
                shutil.rmtree(clip_dir, ignore_errors=True)

    async def _process_match(
        self,
        *,
        state: dict,
        match: dict,
        channel_id: str,
        client_id: str,
        access_token: str,
        clip_dir: Path,
    ) -> None:
        match_id = int(match["match_id"])
        match_start_unix = int(match["start_time"])
        match_duration_s = int(match.get("match_duration_s") or 0)
        match_info = await get_match_metadata(match_id)
        events = detect_events(BETA_STEAM_ACCOUNT_ID, match_info)
        if not events:
            mark_match_processed(state, match_id)
            return

        vod = await find_vod_for_match(
            channel_id,
            match_start_unix,
            match_duration_s,
            client_id,
            access_token,
        )
        if vod is None:
            log.warning("HighlightClipper: Kein passendes VOD fuer match_id=%s gefunden", match_id)
            mark_match_processed(state, match_id)
            return

        clip_paths: list[str] = []
        clip_events: list[HighlightEvent] = []
        vod_offset_s = match_start_unix - int(vod["vod_started_at"])

        for index, event in enumerate(events, start=1):
            clip_start_s = max(0, vod_offset_s + event.game_time_s - CLIP_PADDING_SECONDS)
            clip_end_s = max(clip_start_s + 1, vod_offset_s + event.game_time_s + event.duration_s + CLIP_PADDING_SECONDS)
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
            await send_highlight_dm(
                self.bot,
                BETA_DISCORD_USER_ID,
                match_id,
                clip_events,
                clip_paths,
            )
        else:
            log.warning("HighlightClipper: Keine Clips fuer match_id=%s erstellt", match_id)

        mark_match_processed(state, match_id)

    def _load_twitch_credentials(self) -> tuple[str, str]:
        client_id = str(os.getenv("TWITCH_CLIENT_ID") or "").strip()
        access_token = str(os.getenv("TWITCH_ACCESS_TOKEN") or "").strip()
        if client_id and access_token:
            return client_id, access_token

        secret_store = getattr(self.bot, "secret_store", None)
        loader = getattr(secret_store, "load_secret_value", None)
        if callable(loader):
            client_id = client_id or str(loader("TWITCH_CLIENT_ID") or "").strip()
            access_token = access_token or str(loader("TWITCH_ACCESS_TOKEN") or "").strip()
        else:
            client_id = client_id or load_secret_value("TWITCH_CLIENT_ID")
            access_token = access_token or load_secret_value("TWITCH_ACCESS_TOKEN")
        return client_id, access_token


def _filter_recent_matches(matches: list[dict], state: dict, *, now: int) -> list[dict]:
    min_start = now - 86400
    filtered: list[dict] = []
    for match in matches:
        if not isinstance(match, dict):
            continue
        match_id = _as_int(match.get("match_id"))
        start_time = _as_int(match.get("start_time"))
        if match_id is None or start_time is None:
            continue
        if start_time <= min_start or is_match_processed(state, match_id):
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
