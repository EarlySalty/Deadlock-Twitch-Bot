"""Adaptive Channel-Vibe-Sampling.

Sampelt die letzten ~50 user-turns aus `twitch_engagement_conversation`,
errechnet Sprache (de/en heuristisch), Emoji-Dichte, Caps-Anteil, mittlere
Länge und Twitch-Slang-Vorkommen — liefert einen Prompt-Baustein, den die
Pipeline an den System-Prompt anhängt.

Cache: pro Channel 5 Minuten in-memory. Bei Bot-Restart wird neu gesampelt.
"""

from __future__ import annotations

import asyncio
import re
import time
from dataclasses import dataclass

from bot.storage.pg import query_all


_GERMAN_MARKERS = {
    "der", "die", "das", "und", "ist", "nicht", "auch", "auf", "mit",
    "für", "fuer", "ein", "eine", "den", "im", "haben", "sind", "war",
    "wie", "noch", "schon", "hat", "wird", "halt", "ne", "geh", "gleich",
    "ja", "nein", "aber", "doch", "echt", "krass", "sehr", "mehr", "wenn",
}

_ENGLISH_MARKERS = {
    "the", "and", "is", "you", "what", "this", "that", "with", "have",
    "for", "are", "your", "they", "from", "just", "like", "but", "out",
    "yeah", "nah", "now", "really", "more", "if", "when",
}

_TWITCH_SLANG = {
    "kekw", "pog", "pogchamp", "lul", "omegalul", "kappa", "monkas",
    "jebaited", "sadge", "copium", "ratjam", "peped", "5head", "ezclap",
    "kekwait", "yepw", "pepega", "pepehands", "nyaa", "okayeg",
}

_EMOJI_RE = re.compile(
    "[\U0001F300-\U0001FAFF\U0001F600-\U0001F64F\U0001F900-\U0001F9FF☀-➿]"
)

_WORD_RE = re.compile(r"[a-zA-ZäöüÄÖÜß]+")


@dataclass(slots=True)
class PersonaSnapshot:
    language: str  # 'de' | 'en' | 'mixed'
    avg_length_chars: int
    emoji_density: float
    caps_ratio: float
    slang_terms: list[str]
    sample_count: int

    def to_prompt_fragment(self) -> str:
        if self.sample_count == 0:
            return "Channel-Vibe: noch keine Daten — antworte freundlich-kurz, 1-2 Sätze."

        lang_name = {
            "de": "deutsch",
            "en": "englisch",
            "mixed": "deutsch/englisch gemischt",
        }[self.language]
        bits: list[str] = [f"dominant {lang_name}"]

        if self.avg_length_chars <= 25:
            bits.append("sehr kurze Sätze")
        elif self.avg_length_chars <= 60:
            bits.append("mittlere Satzlänge")
        else:
            bits.append("längere Sätze")

        if self.emoji_density >= 0.5:
            bits.append("hohe Emoji-Dichte")
        elif self.emoji_density >= 0.15:
            bits.append("vereinzelte Emojis")
        else:
            bits.append("kaum Emojis")

        if self.caps_ratio >= 0.35:
            bits.append("oft GROSSGESCHRIEBEN")

        if self.slang_terms:
            top = ", ".join(self.slang_terms[:4])
            bits.append(f"Twitch-Slang vorhanden ({top})")

        joined = ", ".join(bits)
        return (
            f"Channel-Vibe: {joined}. Spiegele diesen Stil, ohne ihn zu karikieren. "
            "Antworten 1-2 Sätze, niemals länger."
        )


_cache: dict[str, tuple[float, PersonaSnapshot]] = {}
_CACHE_TTL_SEC = 300.0


def _sync_load_user_turns(channel_login: str, limit: int) -> list[str]:
    rows = query_all(
        """
        SELECT content FROM twitch_engagement_conversation
        WHERE channel_login = %s AND role = 'user'
        ORDER BY ts DESC LIMIT %s
        """,
        [channel_login, limit],
    )
    return [r[0] for r in rows if r[0]]


def _compute(texts: list[str]) -> PersonaSnapshot:
    if not texts:
        return PersonaSnapshot(
            language="mixed",
            avg_length_chars=0,
            emoji_density=0.0,
            caps_ratio=0.0,
            slang_terms=[],
            sample_count=0,
        )

    de_hits = 0
    en_hits = 0
    total_letters = 0
    total_caps = 0
    total_emojis = 0
    total_length = 0
    slang_counts: dict[str, int] = {}

    for text in texts:
        total_length += len(text)
        for w in _WORD_RE.findall(text):
            wl = w.lower()
            if wl in _GERMAN_MARKERS:
                de_hits += 1
            elif wl in _ENGLISH_MARKERS:
                en_hits += 1
            if wl in _TWITCH_SLANG:
                slang_counts[wl] = slang_counts.get(wl, 0) + 1
            for c in w:
                if c.isalpha():
                    total_letters += 1
                    if c.isupper():
                        total_caps += 1
        total_emojis += len(_EMOJI_RE.findall(text))

    n = len(texts)
    if de_hits >= en_hits * 2 and de_hits > 0:
        language = "de"
    elif en_hits >= de_hits * 2 and en_hits > 0:
        language = "en"
    else:
        language = "mixed"

    avg_len = total_length // n if n else 0
    emoji_density = total_emojis / n if n else 0.0
    caps_ratio = (total_caps / total_letters) if total_letters else 0.0
    top_slang = sorted(slang_counts.items(), key=lambda kv: -kv[1])
    slang_terms = [w for w, _ in top_slang[:4]]

    return PersonaSnapshot(
        language=language,
        avg_length_chars=avg_len,
        emoji_density=emoji_density,
        caps_ratio=caps_ratio,
        slang_terms=slang_terms,
        sample_count=n,
    )


async def sample_tone(channel_login: str, *, limit: int = 50) -> PersonaSnapshot:
    """PersonaSnapshot pro Channel; cached 5min."""
    now = time.time()
    cached = _cache.get(channel_login)
    if cached and (now - cached[0]) < _CACHE_TTL_SEC:
        return cached[1]
    texts = await asyncio.to_thread(_sync_load_user_turns, channel_login, limit)
    snapshot = _compute(texts)
    _cache[channel_login] = (now, snapshot)
    return snapshot
