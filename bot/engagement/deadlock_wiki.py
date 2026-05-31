"""Deadlock-Wissens-Grounding für den Engagement-Layer.

Zweck: die Engagement-AI soll über Deadlock NUR reden, was belegt ist — nicht
halluzinieren. (Anlass: die AI erfand im Live-Test ein Item "Cornucopius" mit
frei erfundener Mechanik.) Statt das Modell als "Deadlock-Experte" raten zu
lassen, bekommt es echte Fakten vorgesetzt und darf nur daraus sprechen.

Zwei echte Quellen:

- **Entity-Index** (Helden- + Item-Namen) von der Deadlock-Assets-API
  (``assets.deadlock-api.com``). Dient der Erkennung, WELCHE Spielsache in einer
  Chat-Nachricht erwähnt wird. Wird ~12h gecacht.
- **Faktentext** von der offiziellen ``deadlock.wiki`` (MediaWiki ``extracts``-API,
  Plain-Text — keine HTML-Parser-Dependency nötig). Liefert die belegte
  Beschreibung der erkannten Sache. Pro Seite ~1h gecacht.

Einspeisung: ``build_grounding_fragment(text)`` erkennt im Chat-Text einen
Helden/ein Item, holt den Wiki-Auszug und gibt einen "nutze nur diese
Fakten"-Block zurück. Erkennt es nichts, ist das Fragment leer — dann zwingt die
Anti-Halluzinations-Regel im Baseline-Prompt die AI, vage statt erfunden zu
antworten. Alle Netzfehler werden geschluckt (leeres Fragment = sichere Seite).
"""

from __future__ import annotations

import asyncio
import logging
import re
import time

import httpx

log = logging.getLogger("TwitchStreams.Engagement.DeadlockWiki")

_ASSETS_BASE = "https://assets.deadlock-api.com"
_WIKI_API = "https://deadlock.wiki/api.php"
_USER_AGENT = "deadlock-twitch-bot/1.0 (engagement-grounding)"
_HTTP_TIMEOUT = httpx.Timeout(8.0)

# Entity-Index: Liste (lower_name, original_name, kind), längster Name zuerst.
_ENTITIES: list[tuple[str, str, str]] = []
_INDEX_LOADED_AT: float = 0.0
_INDEX_TTL_SEC = 12 * 3600.0
_index_lock = asyncio.Lock()

# Wiki-Page-Cache: title -> (geladen_ts, extract_text)
_PAGE_CACHE: dict[str, tuple[float, str]] = {}
_PAGE_TTL_SEC = 3600.0

# Erkennung: kürzere Namen geben als Chat-Wort zu viele Fehltreffer.
_MIN_NAME_LEN = 4

# Trailing-Wiki-Sektionen, die für Grounding nur Rauschen sind.
_TRIM_AT = (
    "== Update history ==",
    "== Navigation ==",
    "== Gallery ==",
    "== Trivia ==",
    "== Backstory ==",
    "== See also ==",
    "== References ==",
)
_MAX_EXTRACT_CHARS = 700
_BLANK_RUN = re.compile(r"\n[ \t]*\n[ \t]*(?:\n[ \t]*)+")


def _new_client() -> httpx.AsyncClient:
    return httpx.AsyncClient(
        timeout=_HTTP_TIMEOUT,
        follow_redirects=True,  # assets.deadlock-api.com 301-redirected sonst weg
        headers={"Accept": "application/json", "User-Agent": _USER_AGENT},
    )


def _display_item_name(entry: dict) -> str | None:
    """Item-Name nur, wenn es ein echter Anzeigename ist.

    Die Assets-API liefert auch interne Einträge, deren ``name`` gleich dem
    ``class_name`` ist (z. B. ``citadel_weapon_bosstier2_set``). Echte
    Shop-Items haben einen lokalisierten Namen mit Großbuchstaben/Leerzeichen.
    """
    name = entry.get("name")
    if not isinstance(name, str):
        return None
    name = name.strip()
    if not name or name == entry.get("class_name"):
        return None
    # Interner snake_case-Name (nur klein, _, Ziffern) → kein Anzeigename.
    if re.fullmatch(r"[a-z0-9_]+", name):
        return None
    return name


def _hero_name(entry: dict) -> str | None:
    name = entry.get("name")
    if not isinstance(name, str):
        return None
    name = name.strip()
    return name or None


async def _fetch_json(client: httpx.AsyncClient, url: str, params: dict | None = None):
    r = await client.get(url, params=params)
    r.raise_for_status()
    return r.json()


async def _load_entity_index() -> list[tuple[str, str, str]]:
    async with _new_client() as client:
        heroes = await _fetch_json(
            client, f"{_ASSETS_BASE}/v2/heroes", params={"only_active": "true"}
        )
        items = await _fetch_json(client, f"{_ASSETS_BASE}/v2/items")

    # lower_name -> (original_name, kind); Helden gewinnen bei Namensgleichheit.
    seen: dict[str, tuple[str, str]] = {}
    for it in items if isinstance(items, list) else []:
        if not isinstance(it, dict):
            continue
        name = _display_item_name(it)
        if name and len(name) >= _MIN_NAME_LEN:
            seen.setdefault(name.lower(), (name, "item"))
    for h in heroes if isinstance(heroes, list) else []:
        if not isinstance(h, dict):
            continue
        name = _hero_name(h)
        if name and len(name) >= _MIN_NAME_LEN:
            seen[name.lower()] = (name, "hero")

    entities = [(low, orig, kind) for low, (orig, kind) in seen.items()]
    # Längster Name zuerst → spezifischster Treffer gewinnt.
    entities.sort(key=lambda t: len(t[0]), reverse=True)
    return entities


async def _ensure_index() -> None:
    global _ENTITIES, _INDEX_LOADED_AT
    now = time.time()
    if _ENTITIES and (now - _INDEX_LOADED_AT) < _INDEX_TTL_SEC:
        return
    async with _index_lock:
        now = time.time()
        if _ENTITIES and (now - _INDEX_LOADED_AT) < _INDEX_TTL_SEC:
            return
        try:
            fresh = await _load_entity_index()
        except Exception:
            log.warning("DeadlockWiki: Entity-Index konnte nicht geladen werden", exc_info=False)
            return  # alten (ggf. leeren) Index behalten — Grounding bleibt dann aus
        if fresh:
            _ENTITIES = fresh
            _INDEX_LOADED_AT = now


def _detect_entity(text: str) -> tuple[str, str] | None:
    """Erkennt den spezifischsten genannten Helden/Item. (original_name, kind)."""
    if not text:
        return None
    haystack = text.lower()
    for low, orig, kind in _ENTITIES:
        # Wortgrenzen, damit 'reach' nicht in 'breach' triggert.
        if re.search(rf"(?<!\w){re.escape(low)}(?!\w)", haystack):
            return orig, kind
    return None


def _trim_extract(extract: str) -> str:
    if not extract:
        return ""
    cut = len(extract)
    for marker in _TRIM_AT:
        idx = extract.find(marker)
        if idx != -1:
            cut = min(cut, idx)
    text = extract[:cut]
    text = _BLANK_RUN.sub("\n", text).strip()
    # Leere "== Abschnitt ==" ohne Inhalt entfernen.
    text = "\n".join(
        line for line in text.splitlines() if not re.fullmatch(r"==+ .+? ==+", line.strip())
    ).strip()
    if len(text) > _MAX_EXTRACT_CHARS:
        text = text[: _MAX_EXTRACT_CHARS - 1].rstrip() + "…"
    return text


async def _fetch_wiki_extract(title: str) -> str | None:
    now = time.time()
    cached = _PAGE_CACHE.get(title.lower())
    if cached and (now - cached[0]) < _PAGE_TTL_SEC:
        return cached[1] or None

    try:
        async with _new_client() as client:
            data = await _fetch_json(
                client,
                _WIKI_API,
                params={
                    "action": "query",
                    "prop": "extracts",
                    "explaintext": "1",
                    "redirects": "1",
                    "format": "json",
                    "titles": title,
                },
            )
    except Exception:
        log.info("DeadlockWiki: extract-Fetch für %r fehlgeschlagen", title, exc_info=False)
        return None

    pages = (((data or {}).get("query") or {}).get("pages") or {}) if isinstance(data, dict) else {}
    extract = ""
    for page in pages.values():
        if isinstance(page, dict) and isinstance(page.get("extract"), str):
            extract = page["extract"]
            break
    trimmed = _trim_extract(extract)
    _PAGE_CACHE[title.lower()] = (now, trimmed)
    return trimmed or None


async def build_grounding_fragment(message_text: str) -> str:
    """System-Prompt-Fragment mit belegten Deadlock-Fakten — oder "" wenn nichts erkannt."""
    await _ensure_index()
    hit = _detect_entity(message_text or "")
    if not hit:
        return ""
    name, kind = hit
    extract = await _fetch_wiki_extract(name)
    if not extract:
        return ""
    label = "Held" if kind == "hero" else "Item"
    return (
        "Beleg aus dem Deadlock-Wiki (offizielle Quelle). Wenn du in deiner Antwort etwas "
        f"über '{name}' sagst, stütze dich AUSSCHLIESSLICH auf diese Fakten — nichts dazu "
        "erfinden, nichts aus dem Gedächtnis ergänzen. Stehen Details (Zahlen, Effekte) hier "
        "nicht drin, sag das nicht, sondern bleib allgemein.\n"
        f"[{label}: {name}]\n{extract}"
    )
