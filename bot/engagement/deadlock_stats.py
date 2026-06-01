"""Hero-Win/Pick-Stats als Grounding-Anker für den Engagement-Layer.

Quelle: ``deadlock-api`` Analytics (``/v1/analytics/hero-stats`` für Win/Loss/Matches
pro Held, ``/v1/assets/heroes`` für id→name). Daraus pro Held ein grober
Stärke-/Beliebtheits-Eindruck.

Einspeisung (entity-getriggert, analog zu ``deadlock_wiki``/``deadlock_patches``):
wird ein Held erkannt, kommt ein kurzer Anhaltspunkt rein, ob er gerade
stark/überrepräsentiert ist. Bewusst QUALITATIV (über/unter 50%, oft/selten
gepickt) statt roher Prozente — der Bot soll keine Tabelle vorlesen, nur ein
Gefühl haben. ~6h gecacht; Netzfehler → leeres Fragment (sichere Seite).
"""

from __future__ import annotations

import asyncio
import logging
import time

import httpx

from .deadlock_wiki import _detect_entity, _ensure_index

log = logging.getLogger("TwitchStreams.Engagement.DeadlockStats")

_API_BASE = "https://api.deadlock-api.com"
_UA = "deadlock-twitch-bot/1.0 (engagement-stats)"
_HTTP_TIMEOUT = httpx.Timeout(10.0)
_TTL_SEC = 6 * 3600.0

# name_lower -> {"name", "wr", "pr_ratio"}
_STATS: dict[str, dict] = {}
_STATS_AT: float = 0.0
_lock = asyncio.Lock()


async def _fetch_json(client: httpx.AsyncClient, url: str, params: dict | None = None):
    r = await client.get(url, params=params)
    r.raise_for_status()
    return r.json()


async def _load_stats() -> dict[str, dict]:
    async with httpx.AsyncClient(
        timeout=_HTTP_TIMEOUT,
        follow_redirects=True,
        headers={"User-Agent": _UA, "Accept": "application/json"},
    ) as client:
        heroes = await _fetch_json(client, f"{_API_BASE}/v1/assets/heroes", {"only_active": "true"})
        stats = await _fetch_json(client, f"{_API_BASE}/v1/analytics/hero-stats")

    id_to_name: dict[int, str] = {}
    for h in heroes if isinstance(heroes, list) else []:
        if isinstance(h, dict) and h.get("id") is not None and h.get("name"):
            id_to_name[int(h["id"])] = str(h["name"])

    rows = [r for r in stats if isinstance(r, dict) and r.get("matches")]
    if not rows:
        return {}
    avg_matches = sum(int(r["matches"]) for r in rows) / len(rows)

    out: dict[str, dict] = {}
    for r in rows:
        hid = r.get("hero_id")
        name = id_to_name.get(int(hid)) if hid is not None else None
        if not name:
            continue
        matches = int(r["matches"])
        wins = int(r.get("wins") or 0)
        wr = wins / matches if matches else 0.0
        out[name.lower()] = {
            "name": name,
            "wr": wr,
            "pr_ratio": (matches / avg_matches) if avg_matches else 1.0,
        }
    return out


async def _ensure_stats() -> dict[str, dict]:
    global _STATS, _STATS_AT
    now = time.time()
    if _STATS and (now - _STATS_AT) < _TTL_SEC:
        return _STATS
    async with _lock:
        now = time.time()
        if _STATS and (now - _STATS_AT) < _TTL_SEC:
            return _STATS
        try:
            fresh = await _load_stats()
        except Exception:
            log.warning("DeadlockStats: Stats-Fetch fehlgeschlagen", exc_info=False)
            return _STATS
        if fresh:
            _STATS = fresh
            _STATS_AT = now
        return _STATS


def _wr_label(wr: float) -> str:
    if wr >= 0.52:
        return "Winrate deutlich über 50%"
    if wr >= 0.505:
        return "Winrate leicht über 50%"
    if wr > 0.495:
        return "Winrate um die 50%"
    if wr > 0.48:
        return "Winrate leicht unter 50%"
    return "Winrate deutlich unter 50%"


def _pr_label(ratio: float) -> str:
    if ratio >= 1.25:
        return "wird grad sehr oft gespielt"
    if ratio >= 0.8:
        return "wird durchschnittlich oft gespielt"
    return "wird eher selten gespielt"


async def build_stats_fragment(message_text: str) -> str:
    """Grober Stärke-/Beliebtheits-Anhaltspunkt zum erkannten Held — sonst "" ."""
    try:
        await _ensure_index()
    except Exception:
        return ""
    hit = _detect_entity(message_text or "")
    if not hit:
        return ""
    name, kind = hit
    if kind != "hero":
        return ""
    stats = await _ensure_stats()
    row = stats.get(name.lower())
    if not row:
        return ""
    return (
        f"Grober Stärke-Anhaltspunkt zu '{name}' (echte Aggregat-Stats): "
        f"{_wr_label(row['wr'])}, {_pr_label(row['pr_ratio'])}. "
        "Nimm das nur als Gefühl, ob er grad stark/meta ist — red locker drüber, lies KEINE "
        "Zahlen oder Prozente vor wie eine Tabelle, und sag nie, woher du das hast."
    )
