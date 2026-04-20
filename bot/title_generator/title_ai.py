from __future__ import annotations

import json
import re
import time
from collections import defaultdict
from typing import Any

MINIMAX_BASE_URL = "https://api.minimax.io/v1"
MINIMAX_MODEL = "MiniMax-M2.7"
EMOJI_PATTERN = re.compile(
    "[\U00010000-\U0010ffff\U0001F300-\U0001F9FF\u2600-\u26FF\u2700-\u27BF]",
    flags=re.UNICODE,
)
_CANONICAL_RANK_NAMES = (
    "Obscurus",
    "Seeker",
    "Alchemist",
    "Arcanist",
    "Ritualist",
    "Emissary",
    "Archon",
    "Oracle",
    "Phantom",
    "Ascendant",
    "Eternus",
)
_GENERIC_FILLER_PHRASES = (
    "heute ist es soweit",
    "heute ist es endlich soweit",
    "endlich ist es soweit",
    "endlich soweit",
)


class RateLimitExceeded(Exception):
    def __init__(self, retry_after: int):
        self.retry_after = retry_after
        super().__init__(f"Rate limit exceeded. Retry in {retry_after}s")


class TitleRateLimiter:
    def __init__(
        self,
        max_requests: int = 5,
        window_seconds: int = 600,
        dashboard_multiplier: int = 2,
    ):
        self._max = max_requests
        self._window = window_seconds
        self._dashboard_max = max_requests * dashboard_multiplier
        self._records: dict[str, list[float]] = defaultdict(list)

    def check_and_record(self, streamer_id: str, source: str) -> bool:
        now = time.monotonic()
        key = f"{streamer_id}:{source}"
        limit = self._dashboard_max if source == "dashboard" else self._max
        self._records[key] = [t for t in self._records[key] if now - t < self._window]
        if len(self._records[key]) >= limit:
            oldest = self._records[key][0]
            retry_after = int(self._window - (now - oldest)) + 1
            raise RateLimitExceeded(retry_after)
        self._records[key].append(now)
        return True


_rate_limiter = TitleRateLimiter(max_requests=5, window_seconds=600, dashboard_multiplier=2)


def _get_minimax_client() -> Any:
    from bot.analytics.api_ai import _load_secret
    from openai import AsyncOpenAI

    api_key = _load_secret("MINIMAX_TOKEN_PLAN_KEY", "MINIMAX_API_KEY", "MINMAX")
    if not api_key:
        raise RuntimeError(
            "MiniMax-Key nicht gefunden. Setze MINIMAX_TOKEN_PLAN_KEY, "
            "MINIMAX_API_KEY oder MINMAX via keyring/Umgebung."
        )

    return AsyncOpenAI(
        api_key=api_key,
        base_url=MINIMAX_BASE_URL,
        timeout=240.0,
    )


def _emoji_ratio(titles: list[dict]) -> float:
    if not titles:
        return 0.0
    with_emoji = sum(1 for t in titles if EMOJI_PATTERN.search(t.get("title", "")))
    return with_emoji / len(titles)


def _format_metric(value: Any, digits: int) -> str:
    if isinstance(value, (int, float)):
        return f"{value:.{digits}f}"
    return "n/a"


def _strip_code_fence(raw: str) -> str:
    return re.sub(r"^```(?:json)?\s*|\s*```$", "", raw.strip(), flags=re.MULTILINE)


def _extract_json_payload(raw: str) -> str:
    text = str(raw or "").strip()
    if not text:
        return ""

    fenced_match = re.search(r"```json\s*(\{.*?\})\s*```", text, flags=re.DOTALL | re.IGNORECASE)
    if fenced_match:
        return fenced_match.group(1).strip()

    object_match = re.search(r"(\{.*\})", text, flags=re.DOTALL)
    if object_match:
        return object_match.group(1).strip()

    return _strip_code_fence(text)


def _sanitize_generated_title(title: str, *, keywords: str, rank_display: str | None) -> str:
    cleaned = str(title or "").strip()
    if not cleaned:
        return ""

    keywords_clean = str(keywords or "").strip()
    lower_keywords = keywords_clean.lower()

    asc_match = re.search(r"\basc\s*(\d)\b", lower_keywords, flags=re.IGNORECASE)
    if asc_match:
        cleaned = re.sub(
            r"\bascension\s+rank\s*" + re.escape(asc_match.group(1)) + r"\b",
            f"Asc {asc_match.group(1)}",
            cleaned,
            flags=re.IGNORECASE,
        )

    if rank_display:
        canonical_rank_names = {name.lower() for name in _CANONICAL_RANK_NAMES}
        allowed_rank_names = {str(rank_display).split()[0].lower()}
        for rank_name in canonical_rank_names - allowed_rank_names:
            cleaned = re.sub(rf"\b{re.escape(rank_name)}(?:\s+\d)?\b", "", cleaned, flags=re.IGNORECASE)
    else:
        for rank_name in _CANONICAL_RANK_NAMES:
            if rank_name.lower() not in lower_keywords:
                cleaned = re.sub(rf"\b{re.escape(rank_name)}(?:\s+\d)?\b", "", cleaned, flags=re.IGNORECASE)

    filler_pattern = "|".join(re.escape(phrase) for phrase in _GENERIC_FILLER_PHRASES)
    cleaned = re.sub(
        rf"\s*[\-|:|]\s*(?:{filler_pattern})\b",
        "",
        cleaned,
        flags=re.IGNORECASE,
    )
    cleaned = re.sub(r"\s{2,}", " ", cleaned)
    cleaned = re.sub(r"\s+([|:,-])", r"\1", cleaned)
    cleaned = re.sub(r"([|:,-]){2,}", r"\1", cleaned)
    return cleaned.strip(" -|:,")


def _sanitize_title_result(data: dict[str, Any], *, keywords: str, rank_display: str | None) -> dict[str, Any]:
    primary = _sanitize_generated_title(
        data.get("primary", ""),
        keywords=keywords,
        rank_display=rank_display,
    )
    alternatives: list[str] = []
    seen = {primary.lower()} if primary else set()
    for title in data.get("alternatives", []):
        cleaned = _sanitize_generated_title(title, keywords=keywords, rank_display=rank_display)
        if not cleaned:
            continue
        key = cleaned.lower()
        if key in seen:
            continue
        seen.add(key)
        alternatives.append(cleaned)
        if len(alternatives) >= 2:
            break
    return {
        "primary": primary or (alternatives[0] if alternatives else ""),
        "alternatives": alternatives,
        "title_analysis": data.get("title_analysis", []),
    }


def build_title_prompt(
    keywords: str,
    title_history: list[dict],
    knowledge_titles: list[dict],
    rank_display: str | None,
    emoji_ratio: float,
    live_state: dict | None = None,
) -> str:
    emoji_rule = (
        "Verwende maximal einen Emoji und nur dann, wenn der Streamer bereits Emojis in seinen Titeln nutzt."
        if emoji_ratio >= 0.3
        else "Verwende KEINE Emojis im Titel."
    )

    sorted_history = sorted(
        title_history,
        key=lambda title: (
            float(title.get("relative_perf") or 0.0),
            float(title.get("engagement_rate") or 0.0),
        ),
        reverse=True,
    )

    top_reference_lines = "\n".join(
        (
            f'  - "{title.get("title", "")}" '
            f'(relative Perf: {_format_metric(title.get("relative_perf"), 2)}, '
            f'Engagement: {_format_metric(title.get("engagement_rate"), 3)})'
        )
        for title in sorted_history[:8]
    ) or "  (keine Daten)"

    history_lines = "\n".join(
        (
            f'  - "{title.get("title", "")}" '
            f'(relative Perf: {_format_metric(title.get("relative_perf"), 2)}, '
            f'Engagement: {_format_metric(title.get("engagement_rate"), 3)})'
        )
        for title in title_history[:20]
    ) or "  (keine Daten)"

    benchmark_lines = "\n".join(
        (
            f'  - "{title.get("title", "")}" '
            f'(Score: {_format_metric(title.get("normalized_score"), 2)})'
        )
        for title in knowledge_titles[:20]
    ) or "  (keine Daten)"

    rank_line = f"\nStreamer-Rang: {rank_display}" if rank_display else ""
    live_line = ""
    if live_state:
        live_line = (
            f'\nAktuelle Live-Daten: Hero={live_state.get("hero", "unbekannt")}, '
            f'Party={live_state.get("party_hint", "solo")}'
        )
    canonical_ranks = ", ".join(_CANONICAL_RANK_NAMES)

    return f"""Du bist ein Twitch-Stream-Titel-Experte für das Spiel Deadlock.

AUFGABE:
1. Analysiere die letzten Stream-Titel des Streamers (mit Performance-Metriken).
2. Generiere EINEN optimalen Stream-Titel basierend auf den angegebenen Keywords.
3. Gib zusätzlich 2 Alternativen an.
4. Bewerte kurz die 3 schlechtesten eigenen Titel (max. 1 Satz je Titel).

KEYWORDS (Intent des Streamers heute): {keywords}{rank_line}{live_line}

BESTE EIGENE REFERENZEN (priorisieren, zuerst daran orientieren):
{top_reference_lines}

EIGENE TITEL-HISTORY (relative_perf = avg_viewers / eigener_durchschnitt):
{history_lines}

COMMUNITY BENCHMARKS (beste Deadlock-Titel nach normalisiertem Score):
{benchmark_lines}

REGELN:
- Der Titel soll vollständig und einladend sein - kein reiner Keyword-Dump.
- Passe dich stilistisch zuerst den BESTEN EIGENEN REFERENZEN an, erst danach den Community-Benchmarks.
- Erfinde möglichst wenig neu. Bevorzuge bekannte Formulierungsbausteine, Satzrhythmus und Tonalität aus den Referenzen.
- Wenn Keywords ungewohnt sind, formuliere konservativ statt kreativ.
- {emoji_rule}
- Halte den Titel unter 140 Zeichen.
- Verwende Rangbegriffe nur, wenn sie explizit in den Keywords oder in "Streamer-Rang" stehen.
- Erfinde niemals Ränge, Skill-Stufen oder Match-Kontext.
- Deadlock-Ränge heißen nur: {canonical_ranks}.
- Schreibe Keywords nicht in andere Begriffe um. Beispiel: "Asc 2" bleibt "Asc 2" und wird NICHT zu "Ascension Rank 2" oder ähnlichem erweitert.
- Vermeide generische Füllphrasen wie "heute ist es soweit", "endlich soweit" oder ähnliche Trailer-Sätze.
- Die Performance-Scores basieren auf Viewer-Zahlen als Proxy (keine echten CTR-Daten).

ANTWORT-FORMAT (JSON, kein Markdown drumherum):
{{
  "primary_title": "<optimaler Titel>",
  "alternatives": ["<Alternative 1>", "<Alternative 2>"],
  "title_analysis": [
    {{"title": "<schlechtester eigener Titel>", "score": <1-10>, "feedback": "<1 Satz>"}},
    ...
  ]
}}"""


def parse_title_response(raw: str) -> dict[str, Any]:
    try:
        data = json.loads(_extract_json_payload(raw))
        return {
            "primary": data.get("primary_title", ""),
            "alternatives": data.get("alternatives", [])[:2],
            "title_analysis": data.get("title_analysis", []),
        }
    except (json.JSONDecodeError, AttributeError, TypeError):
        return {"primary": "", "alternatives": [], "title_analysis": []}


async def generate_title(
    streamer_id: str,
    keywords: str,
    title_history: list[dict],
    knowledge_titles: list[dict],
    rank_display: str | None,
    live_state: dict | None,
    source: str = "chat",
) -> dict[str, Any]:
    """Generate a stream title using MiniMax. Raises RateLimitExceeded if over limit."""
    _rate_limiter.check_and_record(streamer_id, source=source)

    emoji_ratio = _emoji_ratio(title_history)
    prompt = build_title_prompt(
        keywords=keywords,
        title_history=title_history,
        knowledge_titles=knowledge_titles,
        rank_display=rank_display,
        emoji_ratio=emoji_ratio,
        live_state=live_state,
    )

    client = _get_minimax_client()
    completion = await client.chat.completions.create(
        model=MINIMAX_MODEL,
        messages=[{"role": "user", "content": prompt}],
        temperature=0.35,
        max_tokens=2000,
    )
    raw = completion.choices[0].message.content
    return _sanitize_title_result(
        parse_title_response(raw),
        keywords=keywords,
        rank_display=rank_display,
    )


async def generate_insight(title_history: list[dict], period_label: str) -> dict[str, Any]:
    """Generate a weekly insight analysis for a streamer's title history."""
    if not title_history:
        return {}

    history_lines = "\n".join(
        (
            f'  - "{title.get("title", "")}" '
            f'(relative Perf: {_format_metric(title.get("relative_perf", 0), 2)}, '
            f'Engagement: {_format_metric(title.get("engagement_rate", 0), 3)})'
        )
        for title in title_history[:40]
    )

    prompt = f"""Analysiere die Stream-Titel-Performance dieses Deadlock-Streamers für {period_label}.

TITEL-HISTORY (relative_perf = avg_viewers / eigener_durchschnitt):
{history_lines}

Identifiziere:
1. Was läuft gut (Stärken)
2. Was läuft schlecht (Schwächen)
3. Erkannte Muster (z.B. "Titles mit Rang performen besser")
4. Genau 3 konkrete Handlungsempfehlungen

ANTWORT-FORMAT (JSON):
{{
  "strengths": "<Freitext>",
  "weaknesses": "<Freitext>",
  "patterns": "<Freitext>",
  "recommendations": ["<Empfehlung 1>", "<Empfehlung 2>", "<Empfehlung 3>"]
}}"""

    client = _get_minimax_client()
    completion = await client.chat.completions.create(
        model=MINIMAX_MODEL,
        messages=[{"role": "user", "content": prompt}],
        temperature=0.5,
        max_tokens=1500,
    )
    raw = completion.choices[0].message.content
    try:
        data = json.loads(_strip_code_fence(raw))
        recs = data.get("recommendations", [])
        if isinstance(recs, list):
            recs_str = "\n".join(f"• {recommendation}" for recommendation in recs[:3])
        else:
            recs_str = str(recs)
        return {
            "strengths": data.get("strengths", ""),
            "weaknesses": data.get("weaknesses", ""),
            "patterns": data.get("patterns", ""),
            "recommendations": recs_str,
            "raw": data,
        }
    except (json.JSONDecodeError, AttributeError, TypeError):
        return {}
