from __future__ import annotations

from dataclasses import dataclass
from typing import Any


@dataclass(slots=True, frozen=True)
class ChatTarget:
    name: str
    id: str


def normalize_chat_target_login(raw_value: str | None) -> str:
    return str(raw_value or "").strip().lower()


def make_chat_target(login: str, user_id: str) -> ChatTarget:
    return ChatTarget(
        name=normalize_chat_target_login(login),
        id=str(user_id or "").strip(),
    )


def lookup_outbound_chat_suppression(
    chat_bot: Any,
    *,
    target_login: str,
    target_id: str | None,
    source: str,
) -> dict[str, Any] | None:
    if not chat_bot or not hasattr(chat_bot, "_get_outbound_chat_suppression"):
        return None

    resolved_target_id = str(target_id or "").strip()
    if not resolved_target_id:
        return None

    try:
        return chat_bot._get_outbound_chat_suppression(
            make_chat_target(target_login, resolved_target_id),
            source,
        )
    except Exception:
        return None


__all__ = [
    "ChatTarget",
    "lookup_outbound_chat_suppression",
    "make_chat_target",
    "normalize_chat_target_login",
]
