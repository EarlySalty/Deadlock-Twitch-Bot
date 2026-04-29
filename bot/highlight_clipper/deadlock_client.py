from __future__ import annotations

import logging

import aiohttp

log = logging.getLogger("TwitchStreams.HighlightClipper")

_REQUEST_TIMEOUT = aiohttp.ClientTimeout(total=30)


async def _get_json(url: str) -> dict | list:
    async with aiohttp.ClientSession(timeout=_REQUEST_TIMEOUT) as session:
        async with session.get(url) as response:
            response.raise_for_status()
            return await response.json()


async def get_match_history(account_id: int, limit: int = 20) -> list[dict]:
    payload = await _get_json(
        f"https://api.deadlock-api.com/v1/players/{account_id}/match-history?limit={limit}"
    )
    if isinstance(payload, list):
        return [item for item in payload if isinstance(item, dict)]
    if isinstance(payload, dict):
        matches = payload.get("matches") or payload.get("data") or []
        if isinstance(matches, list):
            return [item for item in matches if isinstance(item, dict)]
    return []


async def get_match_metadata(match_id: int) -> dict:
    payload = await _get_json(f"https://api.deadlock-api.com/v1/matches/{match_id}/metadata")
    if isinstance(payload, dict):
        match_info = payload.get("match_info")
        if isinstance(match_info, dict):
            return match_info
        return payload
    return {}
