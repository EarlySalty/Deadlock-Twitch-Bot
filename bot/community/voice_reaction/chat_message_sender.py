"""Sender-Wrapper für Voice-Reaction-Bot-Antworten im Twitch-Chat.

Wendet immer den `sanity_filter` an, ruft dann `_send_chat_message` auf dem
übergebenen RaidChatBot auf und liefert einen `SendOutcome`-Datensatz für
das Audit-Log zurück.
"""

from __future__ import annotations

import logging
from dataclasses import dataclass
from typing import Any

from .sanity_filter import FilterResult, sanitize

log = logging.getLogger("TwitchStreams.VoiceReaction.Sender")

_OUTBOUND_SOURCE = "voice_reaction"


@dataclass(frozen=True)
class SendOutcome:
    sent: bool
    skipped: bool
    skip_reason: str | None
    filter_result: FilterResult
    suppression: dict | None = None
    send_error: str | None = None

    def to_audit_payload(self) -> dict[str, object]:
        return {
            "sent": self.sent,
            "skipped": self.skipped,
            "skip_reason": self.skip_reason,
            "filter_result": self.filter_result.to_audit_payload(),
            "suppression": dict(self.suppression) if self.suppression else None,
            "send_error": self.send_error,
        }


class _Channel:
    __slots__ = ("name", "id")

    def __init__(self, name: str, channel_id: str | None) -> None:
        self.name = name
        self.id = channel_id


async def send_response(
    *,
    chat_bot: Any,
    streamer_login: str,
    streamer_user_id: str | None,
    response_text: str,
    bot_login: str | None = None,
    dry_run: bool = False,
    source: str = _OUTBOUND_SOURCE,
) -> SendOutcome:
    """Sendet `response_text` (nach Filtering) in den Channel des Streamers."""
    filter_result = sanitize(response_text, bot_login=bot_login)

    if filter_result.blocked or not filter_result.filtered_text:
        return SendOutcome(
            sent=False,
            skipped=True,
            skip_reason=filter_result.block_reason or "blocked_by_filter",
            filter_result=filter_result,
        )

    if dry_run:
        log.info(
            "VoiceReaction[DRY_RUN]: würde an %s senden: %s",
            streamer_login,
            filter_result.filtered_text,
        )
        return SendOutcome(
            sent=False,
            skipped=True,
            skip_reason="dry_run",
            filter_result=filter_result,
        )

    if chat_bot is None or not hasattr(chat_bot, "_send_chat_message"):
        return SendOutcome(
            sent=False,
            skipped=True,
            skip_reason="chat_bot_missing",
            filter_result=filter_result,
        )

    channel = _Channel(streamer_login, streamer_user_id)

    suppression: dict | None = None
    if hasattr(chat_bot, "_get_outbound_chat_suppression"):
        try:
            suppression = chat_bot._get_outbound_chat_suppression(channel, source)
        except Exception:
            log.debug(
                "VoiceReaction: Suppression-Check fehlgeschlagen für %s",
                streamer_login,
                exc_info=True,
            )

    if suppression is not None:
        return SendOutcome(
            sent=False,
            skipped=True,
            skip_reason="chat_suppressed",
            filter_result=filter_result,
            suppression=dict(suppression),
        )

    try:
        success = await chat_bot._send_chat_message(
            channel,
            filter_result.filtered_text,
            source=source,
        )
    except Exception as exc:
        log.warning("VoiceReaction: _send_chat_message warf für %s: %s", streamer_login, exc)
        return SendOutcome(
            sent=False,
            skipped=False,
            skip_reason=None,
            filter_result=filter_result,
            send_error=str(exc),
        )

    if not bool(success):
        return SendOutcome(
            sent=False,
            skipped=False,
            skip_reason="send_failed",
            filter_result=filter_result,
        )

    return SendOutcome(
        sent=True,
        skipped=False,
        skip_reason=None,
        filter_result=filter_result,
    )


__all__ = ["SendOutcome", "send_response"]
