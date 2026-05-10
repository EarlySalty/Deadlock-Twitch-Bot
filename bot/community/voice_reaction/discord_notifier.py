"""Discord-Webhook-Notifier für Sales-Conversion-Hinweise.

Schickt einen kompakten Embed an den Sales-Channel, sobald der Brain
`should_notify_human=true` setzt. Idempotent: das Aufrufen liegt beim Caller
(siehe `human_notify_sent_at` in der Conversations-Tabelle).
"""

from __future__ import annotations

import logging
import os
from dataclasses import dataclass
from typing import Any, Awaitable, Callable, Mapping, Sequence

log = logging.getLogger("TwitchStreams.VoiceReaction.Discord")

DEFAULT_USERNAME = "Voice-Reaction"
DEFAULT_COLOR = 0x5865F2  # Discord-Blurple


@dataclass(frozen=True)
class NotifyResult:
    sent: bool
    status_code: int | None
    embed: Mapping[str, object]
    reason: str | None = None


def webhook_url() -> str | None:
    return os.getenv("DISCORD_SALES_NOTIFY_WEBHOOK") or None


async def notify_human(
    *,
    streamer_login: str,
    stance: str,
    confidence: float,
    reasoning_summary: str,
    history_excerpt: Sequence[Mapping[str, object]],
    extra_fields: Mapping[str, str] | None = None,
    webhook_url_override: str | None = None,
    sender: Callable[[str, dict], Awaitable[int]] | None = None,
) -> NotifyResult:
    """Postet einen Embed an den Sales-Webhook."""
    target_url = webhook_url_override or webhook_url()
    embed = _build_embed(
        streamer_login=streamer_login,
        stance=stance,
        confidence=confidence,
        reasoning_summary=reasoning_summary,
        history_excerpt=list(history_excerpt or []),
        extra_fields=dict(extra_fields or {}),
    )
    payload = {"username": DEFAULT_USERNAME, "embeds": [embed]}

    if not target_url:
        return NotifyResult(sent=False, status_code=None, embed=embed, reason="no_webhook_configured")

    try:
        if sender is not None:
            status_code = await sender(target_url, payload)
        else:
            status_code = await _post(target_url, payload)
    except Exception as exc:
        log.warning("VoiceReaction: Discord-Notify-Fehler %s", exc, exc_info=True)
        return NotifyResult(sent=False, status_code=None, embed=embed, reason=str(exc))

    sent = 200 <= int(status_code) < 300
    return NotifyResult(sent=sent, status_code=int(status_code), embed=embed)


async def _post(url: str, payload: dict[str, Any]) -> int:
    import aiohttp  # type: ignore

    async with aiohttp.ClientSession() as session:
        async with session.post(url, json=payload, timeout=aiohttp.ClientTimeout(total=15)) as resp:
            return int(resp.status)


def _build_embed(
    *,
    streamer_login: str,
    stance: str,
    confidence: float,
    reasoning_summary: str,
    history_excerpt: list[Mapping[str, object]],
    extra_fields: dict[str, str],
) -> dict[str, object]:
    transcript_lines: list[str] = []
    for entry in history_excerpt[-8:]:
        role = str(entry.get("role") or "system")
        text = str(entry.get("text") or "").strip()
        if not text:
            continue
        prefix = {
            "voice": "voice",
            "streamer_chat": "streamer",
            "bot_chat": "bot",
        }.get(role, role)
        if len(text) > 220:
            text = text[:217].rstrip() + "…"
        transcript_lines.append(f"**{prefix}**: {text}")

    description = "\n".join(transcript_lines) or "_keine relevante History_"
    if len(description) > 3500:
        description = description[:3497].rstrip() + "…"

    fields: list[dict[str, object]] = [
        {"name": "Stance", "value": stance, "inline": True},
        {"name": "Confidence", "value": f"{confidence:.2f}", "inline": True},
        {
            "name": "Reasoning",
            "value": (reasoning_summary or "—")[:1024],
            "inline": False,
        },
    ]
    for key, value in extra_fields.items():
        fields.append({"name": key, "value": str(value)[:1024], "inline": True})

    return {
        "title": f"Sales-Lead: {streamer_login}",
        "url": f"https://twitch.tv/{streamer_login}",
        "description": description,
        "color": DEFAULT_COLOR,
        "fields": fields,
    }


__all__ = ["notify_human", "NotifyResult", "webhook_url"]
