"""Sanity-Filter für ausgehende Bot-Chat-Messages.

Strippt URLs (auch ohne Schema), fremde @-Mentions und kappt die Länge.
Liefert dem Aufrufer immer ein `FilterResult` zurück, sodass das Audit-Log
die Original- und Filter-Texte plus die Strip-Gründe persistieren kann.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field

DEFAULT_MAX_LENGTH = 280

# Klassische Schema-URLs
_URL_RE = re.compile(r"https?://\S+", re.IGNORECASE)
# Bekannte schema-lose Hosts/TLDs (discord.gg, twitch.tv, www.example.com,
# example.com/path) — bewusst etwas breit, lieber zu viel strippen als URLs
# durchlassen.
_BARE_DOMAIN_RE = re.compile(
    r"\b(?:www\.|discord\.(?:gg|com|me)|twitch\.tv|youtu\.be|youtube\.com)\S*",
    re.IGNORECASE,
)
_GENERIC_DOMAIN_RE = re.compile(
    r"\b[a-z0-9-]+\.(?:com|net|org|io|dev|gg|tv|me|app|xyz|co|de|info)(?:/\S*)?",
    re.IGNORECASE,
)
_MENTION_RE = re.compile(r"@([A-Za-z0-9_]{2,25})")
_WHITESPACE_RE = re.compile(r"\s+")


@dataclass(frozen=True)
class FilterResult:
    """Ergebnis des Sanity-Filters."""

    original_text: str
    filtered_text: str
    strip_reasons: tuple[str, ...] = field(default_factory=tuple)
    blocked: bool = False
    block_reason: str | None = None

    @property
    def changed(self) -> bool:
        return self.original_text != self.filtered_text

    def to_audit_payload(self) -> dict[str, object]:
        return {
            "original_text": self.original_text,
            "filtered_text": self.filtered_text,
            "strip_reasons": list(self.strip_reasons),
            "blocked": self.blocked,
            "block_reason": self.block_reason,
        }


def sanitize(
    text: str | None,
    *,
    bot_login: str | None = None,
    max_length: int = DEFAULT_MAX_LENGTH,
) -> FilterResult:
    """Filtert URLs/Mentions/Längen aus einem Chat-Antwort-Text."""
    raw = "" if text is None else str(text)
    reasons: list[str] = []
    working = raw

    if _URL_RE.search(working):
        working = _URL_RE.sub("", working)
        reasons.append("url")
    if _BARE_DOMAIN_RE.search(working):
        working = _BARE_DOMAIN_RE.sub("", working)
        if "url" not in reasons:
            reasons.append("url")
    if _GENERIC_DOMAIN_RE.search(working):
        working = _GENERIC_DOMAIN_RE.sub("", working)
        if "url" not in reasons:
            reasons.append("url")

    bot_normalized = (bot_login or "").strip().lstrip("@").lower()

    def _mention_replace(match: re.Match[str]) -> str:
        login = match.group(1).lower()
        if bot_normalized and login == bot_normalized:
            return match.group(0)
        return ""

    new_working = _MENTION_RE.sub(_mention_replace, working)
    if new_working != working:
        reasons.append("foreign_mention")
        working = new_working

    working = _WHITESPACE_RE.sub(" ", working).strip()
    if " ," in working or " ." in working or " !" in working or " ?" in working:
        working = re.sub(r"\s+([,.!?])", r"\1", working)

    if len(working) > max_length:
        working = working[: max_length - 1].rstrip() + "…"
        reasons.append("length")

    blocked = False
    block_reason: str | None = None
    cleaned_alpha = re.sub(r"[^A-Za-z0-9]+", "", working)
    if not working:
        blocked = True
        block_reason = "empty_after_strip"
    elif len(cleaned_alpha) < 2:
        blocked = True
        block_reason = "no_alphanum_after_strip"

    return FilterResult(
        original_text=raw,
        filtered_text=working,
        strip_reasons=tuple(reasons),
        blocked=blocked,
        block_reason=block_reason,
    )


__all__ = ["FilterResult", "sanitize", "DEFAULT_MAX_LENGTH"]
