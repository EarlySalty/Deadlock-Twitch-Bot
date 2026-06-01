"""Aktuelle Deadlock-Patchnotes als Grounding für den Engagement-Layer.

Quelle: die offizielle Steam-News-API (appid 1422450). Sie liefert die echten,
aktuellen Patchnotes als BBCode-JSON — kein HTML-Scraping/bs4 nötig (passend zur
bs4-freien Linie von ``deadlock_wiki``). Wird zu Change-Zeilen normalisiert und
~6h gecacht.

Zwei Einspeisungen, analog zu ``deadlock_wiki`` / ``global_sentiment``:
- **entity-getriggert** (``build_patch_fragment``): wird ein Held/Item in der
  Nachricht erkannt, kommen GENAU dessen echte Patch-Änderungen rein — der Bot darf
  sie einschätzen (Buff/Nerf, gut/schlecht), aber nur auf Basis dieser Zeilen.
- **ambient** (``get_patch_digest_fragment``): redet jemand über Patch/Meta, kommt ein
  kompakter Überblick des letzten Patches, damit der Bot allgemein urteilen kann.

Halluzinations-sicher: nur belegte Change-Zeilen, nichts erfunden, Quelle nie genannt.
Der Entity-Detektor wird aus ``deadlock_wiki`` wiederverwendet (Helden/Item-Index).
"""

from __future__ import annotations

import asyncio
import html
import logging
import re
import time

import httpx

from .deadlock_wiki import _detect_entity, _ensure_index

log = logging.getLogger("TwitchStreams.Engagement.DeadlockPatches")

_STEAM_NEWS_URL = "https://api.steampowered.com/ISteamNews/GetNewsForApp/v2/"
_APPID = 1422450
_UA = "deadlock-twitch-bot/1.0 (engagement-patchnotes)"
_HTTP_TIMEOUT = httpx.Timeout(10.0)

_TTL_SEC = 6 * 3600.0
_MIN_CHANGE_LINES = 5      # weniger -> ist wohl kein Changelog, sondern andere News
_MAX_ENTITY_LINES = 10
_MAX_DIGEST_LINES = 14

# Latest-Patch-Cache: {"title": str, "date": int|None, "lines": list[str]}
_LATEST: dict | None = None
_LATEST_AT: float = 0.0
_lock = asyncio.Lock()

# Nur bei echtem Patch-/Meta-Gespräch den Ambient-Digest einspeisen (sonst zu aufdringlich).
_PATCH_TALK_RE = re.compile(
    r"\b(patch|update|hotfix|nerf|nerv|buff|gebufft|generft|generved|meta|balance|patchnotes)\w*",
    re.IGNORECASE,
)

_IMG_RE = re.compile(r"\[img\].*?\[/img\]", re.IGNORECASE | re.DOTALL)
_URL_RE = re.compile(r"\[url=(.*?)\](.*?)\[/url\]", re.IGNORECASE | re.DOTALL)
_TAG_STRIP_RE = re.compile(
    r"\[/?(?:b|i|u|list(?:=[^\]]*)?|url[^\]]*|h[1-6]|quote|code|noparse|table|tr|td|spoiler|strike)\]",
    re.IGNORECASE,
)


def _bbcode_to_change_lines(body: str) -> list[str]:
    """BBCode-Patchtext → Liste echter Change-Zeilen (führendes '- ' entfernt)."""
    t = html.unescape(body or "").replace("\r", "\n")
    t = re.sub(r"(?i)\[/?p\]", "\n", t)
    t = re.sub(r"(?i)\[h[12]\](.*?)\[/h[12]\]", r"\n[ \1 ]\n", t)
    t = re.sub(r"(?i)\[\*\]", "\n- ", t)
    t = _IMG_RE.sub("", t)
    t = _URL_RE.sub(r"\2", t)
    t = _TAG_STRIP_RE.sub("", t)

    changes: list[str] = []
    for raw in t.split("\n"):
        s = raw.strip()
        if not s.startswith("- "):
            continue
        s = s[2:].strip()
        if len(s) >= 4:
            changes.append(s)
    return changes


async def _fetch_latest_patch() -> dict | None:
    async with httpx.AsyncClient(
        timeout=_HTTP_TIMEOUT,
        headers={"User-Agent": _UA, "Accept": "application/json"},
    ) as client:
        r = await client.get(
            _STEAM_NEWS_URL,
            params={"appid": _APPID, "count": 15, "maxlength": 0, "format": "json"},
        )
        r.raise_for_status()
        data = r.json()

    items = (((data or {}).get("appnews") or {}).get("newsitems") or []) if isinstance(data, dict) else []
    for it in items:
        if not isinstance(it, dict):
            continue
        lines = _bbcode_to_change_lines(it.get("contents") or "")
        if len(lines) >= _MIN_CHANGE_LINES:
            return {
                "title": (it.get("title") or "Update").strip(),
                "date": it.get("date"),
                "lines": lines,
            }
    return None


async def _ensure_latest() -> dict | None:
    global _LATEST, _LATEST_AT
    now = time.time()
    if _LATEST and (now - _LATEST_AT) < _TTL_SEC:
        return _LATEST
    async with _lock:
        now = time.time()
        if _LATEST and (now - _LATEST_AT) < _TTL_SEC:
            return _LATEST
        try:
            fresh = await _fetch_latest_patch()
        except Exception:
            log.warning("DeadlockPatches: Patch-Fetch fehlgeschlagen", exc_info=False)
            return _LATEST  # alten Stand behalten (leeres Fragment = sichere Seite)
        if fresh:
            _LATEST = fresh
            _LATEST_AT = now
        return _LATEST


def _lines_for_entity(name: str, lines: list[str]) -> list[str]:
    pat = re.compile(rf"(?<!\w){re.escape(name.lower())}(?!\w)")
    hits = [ln for ln in lines if pat.search(ln.lower())]
    return hits[:_MAX_ENTITY_LINES]


async def build_patch_fragment(message_text: str) -> str:
    """Echte Patch-Änderungen zum erkannten Held/Item — oder "" wenn nichts passt."""
    try:
        await _ensure_index()
    except Exception:
        return ""
    hit = _detect_entity(message_text or "")
    if not hit:
        return ""
    name, _kind = hit
    patch = await _ensure_latest()
    if not patch:
        return ""
    lines = _lines_for_entity(name, patch["lines"])
    if not lines:
        return ""
    body = "\n".join(f"- {ln}" for ln in lines)
    return (
        f"Echte Änderungen aus dem letzten Deadlock-Patch ('{patch['title']}') zu '{name}'. "
        "Du darfst die einschätzen — Buff oder Nerf, ob sich das gut/stark anfühlt — aber "
        "AUSSCHLIESSLICH auf Basis dieser Zeilen, nichts dazu erfinden, und sag nie, woher du "
        "das hast:\n"
        f"{body}"
    )


async def get_patch_digest_fragment(message_text: str) -> str:
    """Kompakter Überblick des letzten Patches — nur bei Patch-/Meta-Gespräch."""
    if not _PATCH_TALK_RE.search(message_text or ""):
        return ""
    patch = await _ensure_latest()
    if not patch or not patch["lines"]:
        return ""
    shown = patch["lines"][:_MAX_DIGEST_LINES]
    body = "\n".join(f"- {ln}" for ln in shown)
    more = len(patch["lines"]) - len(shown)
    tail = f"\n(… und {more} weitere Änderungen)" if more > 0 else ""
    return (
        f"Der letzte Deadlock-Patch ('{patch['title']}') hat u.a. das hier geändert (echte "
        "Patch-Zeilen). Wenn jemand über den Patch oder die Meta redet, darfst du das einschätzen "
        "(was ist Buff/Nerf, was tut dem Game gut/weh) — aber nur auf Basis dieser Zeilen, nichts "
        "erfinden, Quelle nie nennen:\n"
        f"{body}{tail}"
    )
