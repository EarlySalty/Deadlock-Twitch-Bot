"""Fuzzy-Korrektur eines Whisper-Transkripts gegen das Deadlock-Vokabular.

Vorgehen:
1. Aus dem Vokabular wird ein Index aus (term, alias) -> canonical aufgebaut.
2. Tokens des Transkripts werden zuerst exakt (case-insensitive) gegen den Index
   geprueft. Findet sich ein Treffer, wird der Token durch das `canonical`
   ersetzt.
3. Schlaegt der exakte Match fehl, wird per Levenshtein-Distanz auf das gesamte
   Vokabular gesucht. Treffer mit Distanz <= adaptivem Schwellwert (laenger =
   tolerant) werden ersetzt.
4. Multi-Word-Aliase (z.B. "soul orb") werden in einer zweiten Pass-Phase als
   N-Grams (bigrams + trigrams) gegen den Index geprueft, bevor Token-by-Token
   korrigiert wird.

Rueckgabe enthaelt das korrigierte Transkript und alle erkannten kanonischen
Begriffe (deduped, sortiert nach Vorkommen-Reihenfolge), die spaeter dem LLM
als Domain-Hinweis uebergeben werden.
"""

from __future__ import annotations

import logging
import re
from dataclasses import dataclass, field
from typing import Iterable, Sequence

from .vocab import VocabEntry, load_all_vocab

log = logging.getLogger("TwitchStreams.SocialMedia.Correction")

_TOKEN_RE = re.compile(r"[A-Za-zÄÖÜäöüß']+", re.UNICODE)


@dataclass(frozen=True)
class CorrectionResult:
    corrected: str
    detected_terms: tuple[str, ...] = field(default_factory=tuple)
    replacements: tuple[tuple[str, str], ...] = field(default_factory=tuple)


# ---------- Levenshtein ----------

def _levenshtein(a: str, b: str) -> int:
    """Iterative Levenshtein with O(min(|a|,|b|)) memory."""
    if a == b:
        return 0
    if len(a) < len(b):
        a, b = b, a
    if not b:
        return len(a)
    previous = list(range(len(b) + 1))
    for i, ca in enumerate(a, start=1):
        current = [i] + [0] * len(b)
        for j, cb in enumerate(b, start=1):
            insert_cost = current[j - 1] + 1
            delete_cost = previous[j] + 1
            replace_cost = previous[j - 1] + (0 if ca == cb else 1)
            current[j] = min(insert_cost, delete_cost, replace_cost)
        previous = current
    return previous[-1]


def _adaptive_threshold(token_length: int) -> int:
    """Toleranzschwelle abhaengig von Token-Laenge (kuerzere Tokens strenger)."""
    if token_length <= 3:
        return 0
    if token_length <= 5:
        return 1
    if token_length <= 8:
        return 2
    return 2


# ---------- Index-Aufbau ----------

@dataclass(frozen=True)
class _VocabIndex:
    exact_lookup: dict[str, str]
    multi_word_lookup: dict[str, str]
    single_word_terms: tuple[tuple[str, str, int], ...]


def _build_index(entries: Sequence[VocabEntry]) -> _VocabIndex:
    exact: dict[str, str] = {}
    multi: dict[str, str] = {}
    singles: list[tuple[str, str, int]] = []
    for entry in entries:
        canonical = entry.canonical.strip()
        if not canonical:
            continue
        candidates = [entry.term, canonical, *entry.aliases]
        for cand in candidates:
            cand_text = str(cand or "").strip()
            if not cand_text:
                continue
            cand_lower = cand_text.lower()
            tokens = _TOKEN_RE.findall(cand_lower)
            if not tokens:
                continue
            if len(tokens) == 1:
                # exact lookup overrides only if heavier weight or new
                existing_weight = -1
                if cand_lower in exact:
                    # Hold first wins, no weight juggling needed
                    continue
                exact[cand_lower] = canonical
                singles.append((cand_lower, canonical, int(entry.weight or 1)))
                _ = existing_weight
            else:
                multi[" ".join(tokens)] = canonical

    # dedupe singles, keep highest weight
    dedup_singles: dict[str, tuple[str, int]] = {}
    for token, canon, weight in singles:
        prev = dedup_singles.get(token)
        if prev is None or weight > prev[1]:
            dedup_singles[token] = (canon, weight)
    singles_tuple = tuple(
        (token, canon, weight) for token, (canon, weight) in dedup_singles.items()
    )
    return _VocabIndex(
        exact_lookup=exact,
        multi_word_lookup=multi,
        single_word_terms=singles_tuple,
    )


# ---------- Korrektur ----------

def _replace_multi_word(text: str, index: _VocabIndex) -> tuple[str, list[str]]:
    """Ersetze Multi-Word-Aliasse (Bigrams + Trigrams) im Volltext."""
    if not index.multi_word_lookup:
        return text, []

    detected: list[str] = []

    # Sort patterns by length DESC to prefer longer matches first
    patterns = sorted(index.multi_word_lookup.items(), key=lambda x: -len(x[0]))
    for phrase, canonical in patterns:
        # Word-boundary regex with whitespace tolerance
        pattern = re.compile(
            r"\b" + r"\s+".join(re.escape(p) for p in phrase.split()) + r"\b",
            re.IGNORECASE,
        )
        if not pattern.search(text):
            continue
        text, count = pattern.subn(canonical, text)
        if count > 0:
            detected.extend([canonical] * count)
    return text, detected


def _correct_token(token: str, index: _VocabIndex) -> tuple[str, str | None]:
    """Korrigiere einen Token, gib (replacement, canonical_or_None) zurueck."""
    if not token or not token.isalpha():
        return token, None
    lower = token.lower()
    canonical = index.exact_lookup.get(lower)
    if canonical:
        return canonical, canonical

    threshold = _adaptive_threshold(len(lower))
    if threshold <= 0:
        return token, None

    best_distance = threshold + 1
    best_canonical: str | None = None
    best_weight = -1
    for term, canon, weight in index.single_word_terms:
        if abs(len(term) - len(lower)) > threshold:
            continue
        dist = _levenshtein(lower, term)
        if dist < best_distance or (dist == best_distance and weight > best_weight):
            best_distance = dist
            best_canonical = canon
            best_weight = weight
    if best_canonical and best_distance <= threshold:
        return best_canonical, best_canonical
    return token, None


def correct_transcript(
    transcript: str,
    *,
    vocab: Iterable[VocabEntry] | None = None,
) -> CorrectionResult:
    """Korrigiere das Transkript und sammle erkannte Domain-Begriffe.

    Args:
        transcript: Whisper-Rohtext.
        vocab: Optionaler Vorab-geladener VocabEntry-Iterable. Wenn None, wird
            das gesamte Vokabular aus der DB geladen (Caching dem Aufrufer
            ueberlassen).
    """
    if not transcript or not transcript.strip():
        return CorrectionResult(corrected=transcript or "")

    entries = list(vocab) if vocab is not None else load_all_vocab()
    if not entries:
        return CorrectionResult(corrected=transcript)

    index = _build_index(entries)

    text = transcript
    text, multi_detected = _replace_multi_word(text, index)

    detected: list[str] = list(multi_detected)
    replacements: list[tuple[str, str]] = []

    def _process_token(match: re.Match[str]) -> str:
        original = match.group(0)
        replacement, canonical = _correct_token(original, index)
        if canonical and replacement.lower() != original.lower():
            replacements.append((original, replacement))
        if canonical:
            detected.append(canonical)
        return replacement

    corrected = _TOKEN_RE.sub(_process_token, text)

    # dedupe detected (preserve order)
    seen: set[str] = set()
    deduped: list[str] = []
    for term in detected:
        if term in seen:
            continue
        seen.add(term)
        deduped.append(term)

    return CorrectionResult(
        corrected=corrected,
        detected_terms=tuple(deduped),
        replacements=tuple(replacements),
    )
