"""Chat-Send über den Engagement-Sende-Account (Smoke-Account).

Spiegelt den Helix-Send-Pfad aus ``bot/chat/moderation.py:_send_chat_message``,
nutzt aber NICHT die zentrale Bot-Identität, sondern das Token + die User-ID des
separaten Engagement-Accounts (siehe ``sender_auth``). Damit erscheint die
AI-Antwort im Chat als unauffälliger Zuschauer statt als „der Bot".

``send()`` ist best-effort: liefert True nur bei bestätigtem Versand
(``is_sent``), sonst False mit Log. Fehlt ein onboardeter Account, gibt es
sauber None zurück, damit der Aufrufer auf sein Default-Verhalten zurückfallen
kann.
"""

from __future__ import annotations

import json
import logging

import aiohttp

from .sender_auth import _client_credentials, get_valid_access_token

log = logging.getLogger("TwitchStreams.Engagement.StealthSender")

HELIX_CHAT_MESSAGES_URL = "https://api.twitch.tv/helix/chat/messages"


async def send(broadcaster_id: str, text: str) -> bool | None:
    """Sendet ``text`` als Smoke-Account in den Chat von ``broadcaster_id``.

    Returns:
        True  – Nachricht bestätigt versendet.
        False – Account vorhanden, aber Versand fehlgeschlagen/gedroppt.
        None  – kein Sende-Account onboarded (Aufrufer soll Fallback nutzen).
    """
    broadcaster_id = str(broadcaster_id or "").strip()
    text = (text or "").strip()
    if not broadcaster_id or not text:
        return False

    creds = await get_valid_access_token()
    if creds is None:
        return None
    access_token, sender_id = creds

    try:
        client_id, _ = _client_credentials()
    except Exception:
        log.warning("StealthSender: Client-Credentials fehlen")
        return None

    headers = {
        "Client-ID": client_id,
        "Authorization": f"Bearer {access_token}",
        "Content-Type": "application/json",
    }
    payload = {
        "broadcaster_id": broadcaster_id,
        "sender_id": str(sender_id),
        "message": text,
    }

    try:
        async with aiohttp.ClientSession() as session:
            async with session.post(HELIX_CHAT_MESSAGES_URL, headers=headers, json=payload) as r:
                body = await r.text()
                if r.status not in {200, 204}:
                    log.warning("StealthSender: Helix HTTP %s: %s", r.status, body[:200])
                    return False
                if r.status == 204:
                    return True

                # HTTP 200 kann trotzdem einen serverseitigen Drop bedeuten.
                try:
                    parsed = json.loads(body) if body else {}
                except Exception:
                    parsed = {}
                data = parsed.get("data") if isinstance(parsed, dict) else None
                if isinstance(data, list) and data and isinstance(data[0], dict):
                    is_sent = data[0].get("is_sent")
                    if is_sent is True:
                        return True
                    if is_sent is False:
                        drop = data[0].get("drop_reason")
                        log.warning("StealthSender: Nachricht gedroppt: %s", drop)
                        return False
                # Kein eindeutiges is_sent -> optimistisch True (Helix-Erfolg)
                return True
    except Exception:
        log.exception("StealthSender: Send fehlgeschlagen")
        return False
