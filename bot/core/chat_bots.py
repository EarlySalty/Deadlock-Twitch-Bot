"""Shared known-chat-bot helpers used by chat ingestion and analytics."""

from __future__ import annotations

from collections.abc import Iterable

# Single source of truth for known service/chat bot accounts.
KNOWN_CHAT_BOTS: tuple[str, ...] = (
    "botrix",
    "deutschedeadlockcommunity",
    "fossabot",
    "moobot",
    "nightbot",
    "pretzelrocks",
    "soundalerts",
    "streamlabs",
    "streamelements",
    "wizebot",
)

_KNOWN_CHAT_BOTS_SET = frozenset(KNOWN_CHAT_BOTS)


def normalize_chat_login(login: str | None) -> str:
    """Normalize a Twitch login for case-insensitive bot checks."""
    return (login or "").strip().lower()


def is_known_chat_bot(login: str | None) -> bool:
    """Return True if login belongs to a known chat bot account."""
    normalized = normalize_chat_login(login)
    return bool(normalized) and normalized in _KNOWN_CHAT_BOTS_SET


def build_known_chat_bot_not_in_clause(
    *,
    column_expr: str,
    placeholder: str = "?",
    bots: Iterable[str] | None = None,
) -> tuple[str, list[str]]:
    """Build a SQL clause + params to exclude known bot logins by chatter_login.

    The returned clause keeps rows where login is missing so anonymous chatter_id
    rows are not incorrectly filtered out.
    """
    source = KNOWN_CHAT_BOTS if bots is None else tuple(bots)
    normalized = sorted(
        {
            normalized_login
            for normalized_login in (normalize_chat_login(value) for value in source)
            if normalized_login
        }
    )
    if not normalized:
        return "1=1", []

    placeholders = ", ".join(placeholder for _ in normalized)
    clause = (
        f"(({column_expr}) IS NULL OR ({column_expr}) = '' "
        f"OR LOWER({column_expr}) NOT IN ({placeholders}))"
    )
    return clause, normalized


__all__ = [
    "KNOWN_CHAT_BOTS",
    "build_known_chat_bot_not_in_clause",
    "is_known_chat_bot",
    "normalize_chat_login",
]
