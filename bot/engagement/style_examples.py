"""Konkrete Stil-Beispiele aus echtem Channel-Chat (Few-Shot).

Ergänzt `persona.py`: während persona den Vibe *statistisch beschreibt*
("kurze Sätze, vereinzelte Emojis, Slang"), zeigt dieses Modul dem Modell
*konkrete echte Nachrichten* als Stilvorlage. LLMs imitieren Beispiele
treffsicherer als Stil-Adjektive — das ist die "show, don't tell"-Schicht
gegen AI-isches Schreiben.

Quelle: die letzten User-Turns aus `twitch_engagement_conversation` (dieselbe
Tabelle wie persona). Gefiltert auf kurze, saubere, repräsentative Zeilen.

WICHTIG: Der erzeugte Prompt-Block trennt hart zwischen Stil und Inhalt — das
Modell soll NUR Schreibweise/Ton/Länge nachahmen, niemals Behauptungen oder
Spielfakten aus den Beispielen übernehmen. Sonst käme die Halluzinations-
Kontamination über die Hintertür zurück (vgl. das erfundene "Cornucopius"-Item).
Faktenquelle bleibt ausschliesslich das Wiki-Grounding (`deadlock_wiki`).
"""

from __future__ import annotations

import asyncio
import time

from bot.storage.pg import query_all

_POOL_LIMIT = 120        # so viele jüngste Turns durchsuchen
_MAX_EXAMPLES = 6        # so viele Beispiele in den Prompt
_MIN_LEN = 8
_MAX_LEN = 100

_cache: dict[str, tuple[float, str]] = {}
_CACHE_TTL_SEC = 600.0   # 10min


def _sync_load_user_turns(channel_login: str, limit: int) -> list[str]:
    rows = query_all(
        """
        SELECT content FROM twitch_engagement_conversation
        WHERE channel_login = %s AND role = 'user'
        ORDER BY ts DESC LIMIT %s
        """,
        [channel_login, limit],
    )
    return [r[0] for r in rows if r and r[0]]


def _is_good_example(text: str) -> bool:
    if not (_MIN_LEN <= len(text) <= _MAX_LEN):
        return False
    if " " not in text:  # Einzel-Token / reines Emote
        return False
    if text[:1] in ("!", "/", "."):  # Commands
        return False
    low = text.lower()
    if "http" in low or "www." in low:  # Links
        return False
    letters = [c for c in text if c.isalpha()]
    if letters and sum(c.isupper() for c in letters) / len(letters) > 0.6:
        return False  # CAPS-Spam
    return True


def _select_examples(texts: list[str], max_n: int = _MAX_EXAMPLES) -> list[str]:
    out: list[str] = []
    seen: set[str] = set()
    for raw in texts:
        text = " ".join(str(raw).split())
        if not _is_good_example(text):
            continue
        key = text.lower()
        if key in seen:
            continue
        seen.add(key)
        out.append(text)
        if len(out) >= max_n:
            break
    return out


def _build_fragment(examples: list[str]) -> str:
    if not examples:
        return ""
    lines = "\n".join(f"- {e}" for e in examples)
    return (
        "So schreiben echte Leute in diesem Chat. Ahme NUR Schreibweise, Ton und Länge nach "
        "(Kleinschreibung/Slang/Emotes wie hier üblich, knapp, keine perfekte Grammatik). "
        "Den INHALT dieser Beispiele und alle darin enthaltenen Behauptungen oder Spielfakten "
        "IGNORIERST du komplett — sie sind reine Stilvorlage, keine Quelle:\n"
        f"{lines}"
    )


async def build_style_fragment(channel_login: str) -> str:
    """Few-Shot-Stilblock pro Channel; cached 10min. "" wenn zu wenig Material."""
    now = time.time()
    cached = _cache.get(channel_login)
    if cached and (now - cached[0]) < _CACHE_TTL_SEC:
        return cached[1]
    texts = await asyncio.to_thread(_sync_load_user_turns, channel_login, _POOL_LIMIT)
    fragment = _build_fragment(_select_examples(texts))
    _cache[channel_login] = (now, fragment)
    return fragment
