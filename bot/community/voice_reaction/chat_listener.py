"""Hook in den IRC-event_message-Pfad, der Streamer-Reaktionen an den
Voice-Reaction-Scheduler weiterleitet.

Filter-Regel (rein O(1)):
- Channel muss eine offene Konversation haben (Set-Lookup im Scheduler).
- Author muss entweder der Channel-Owner (Streamer selbst) sein, ODER
  die Nachricht enthält `@<bot_login>` als Mention.
"""

from __future__ import annotations

import logging
import re
from typing import Any

log = logging.getLogger("TwitchStreams.VoiceReaction.ChatListener")

_MENTION_RE = re.compile(r"@([A-Za-z0-9_]{2,25})")


def _normalize(value: object) -> str:
    return str(value or "").strip().lower()


def _channel_login(channel: Any) -> str:
    if channel is None:
        return ""
    for attr in ("name", "login", "user_login"):
        value = getattr(channel, attr, None)
        if value:
            return _normalize(value)
    return ""


def _author_login(author: Any) -> str:
    if author is None:
        return ""
    for attr in ("name", "login", "user_login"):
        value = getattr(author, attr, None)
        if value:
            return _normalize(value)
    return ""


async def maybe_dispatch_chat_message(
    *,
    scheduler: Any,
    channel_login: str | object,
    author: Any,
    text: str,
    bot_login: str | None = None,
) -> bool:
    """Dispatcht eine Chat-Message an den Scheduler, wenn der Filter passt.

    Liefert `True`, wenn ein Trigger eingequeued wurde — sonst `False`.
    """
    if scheduler is None:
        return False

    login = (
        channel_login
        if isinstance(channel_login, str)
        else _channel_login(channel_login)
    )
    login = _normalize(login)
    if not login:
        return False

    if not scheduler.is_active_channel(login):
        return False

    text_str = str(text or "").strip()
    if not text_str:
        return False

    author_name = _author_login(author)
    is_streamer = bool(author_name) and author_name == login

    bot_login_normalized = _normalize(bot_login or "")
    mentions_bot = False
    if bot_login_normalized:
        for match in _MENTION_RE.findall(text_str):
            if match.lower() == bot_login_normalized:
                mentions_bot = True
                break

    if not (is_streamer or mentions_bot):
        try:
            from . import audit_log

            audit_log.audit(
                login,
                "streamer_chat_filtered_out",
                {
                    "author": author_name,
                    "text": text_str,
                    "filter_reason": "not_streamer_no_bot_mention",
                },
            )
        except Exception:
            log.debug("VoiceReaction: filter-out audit fehlgeschlagen", exc_info=True)
        return False

    try:
        await scheduler.enqueue_chat(
            login=login,
            text=text_str,
            author=author_name or None,
        )
    except Exception:
        log.debug("VoiceReaction: enqueue_chat fehlgeschlagen", exc_info=True)
        return False
    return True


__all__ = ["maybe_dispatch_chat_message"]
