from __future__ import annotations

import asyncio
import bz2
import logging
from pathlib import Path

import aiohttp

log = logging.getLogger("TwitchStreams.HighlightClipper")

_SALTS_URL = "https://api.deadlock-api.com/v1/matches/{match_id}/salts"
_DEMO_CACHE_DIR = Path("data/highlight_clipper/demos")
_TIMEOUT = aiohttp.ClientTimeout(total=120)


async def get_demo_path(match_id: int) -> Path | None:
    """Gibt den Pfad zur entpackten .dem-Datei zurück, lädt sie ggf. herunter."""
    cache = _DEMO_CACHE_DIR.resolve()
    cache.mkdir(parents=True, exist_ok=True)
    dem_path = cache / f"{match_id}.dem"
    if dem_path.exists():
        return dem_path

    demo_url = await _get_demo_url(match_id)
    if not demo_url:
        return None

    log.info("HighlightClipper: Demo-Download für match=%s", match_id)
    bz2_data = await _download_bytes(demo_url)
    if not bz2_data:
        return None

    try:
        raw = bz2.decompress(bz2_data)
    except Exception:
        log.exception("HighlightClipper: Demo-Dekomprimierung fehlgeschlagen match=%s", match_id)
        return None

    dem_path.write_bytes(raw)
    log.info("HighlightClipper: Demo entpackt → %s (%.1fMB)", dem_path.name, len(raw) / 1_048_576)
    return dem_path


def cleanup_demo(match_id: int) -> None:
    path = (_DEMO_CACHE_DIR.resolve() / f"{match_id}.dem")
    path.unlink(missing_ok=True)


async def _get_demo_url(match_id: int) -> str | None:
    url = _SALTS_URL.format(match_id=match_id)
    try:
        async with aiohttp.ClientSession(timeout=aiohttp.ClientTimeout(total=10)) as s:
            async with s.get(url) as r:
                if r.status != 200:
                    return None
                data = await r.json()
                return data.get("demo_url")
    except Exception:
        log.exception("HighlightClipper: Salts-Abfrage fehlgeschlagen match=%s", match_id)
        return None


async def _download_bytes(url: str) -> bytes | None:
    try:
        async with aiohttp.ClientSession(timeout=_TIMEOUT) as s:
            async with s.get(url) as r:
                if r.status != 200:
                    log.warning("HighlightClipper: Demo-Download HTTP %s", r.status)
                    return None
                return await r.read()
    except Exception:
        log.exception("HighlightClipper: Demo-Download fehlgeschlagen")
        return None
