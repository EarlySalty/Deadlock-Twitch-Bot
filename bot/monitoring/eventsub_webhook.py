"""Twitch EventSub Webhook handler – empfängt und verifiziert eingehende Notifications."""

from __future__ import annotations

import asyncio
import heapq
import hashlib
import hmac
import logging
from collections.abc import Awaitable, Callable
from datetime import UTC, datetime

from aiohttp import web

EventCallback = Callable[[str, str, dict], Awaitable[None]]

# Twitch EventSub Message-Types
MSG_TYPE_NOTIFICATION = "notification"
MSG_TYPE_CHALLENGE = "webhook_callback_verification"
MSG_TYPE_REVOCATION = "revocation"

# Max allowed age for incoming messages (Twitch recommendation: 10 minutes)
_MAX_MESSAGE_AGE_SECONDS = 600

# Optional hard limit for in-memory dedupe entries.
# If set to an int, new IDs are rejected once full until old entries expire.
_SEEN_ID_HARD_LIMIT: int | None = None


class EventSubWebhookHandler:
    """
    Empfängt und verifiziert Twitch EventSub Webhook Notifications.

    Registriert sich als Route-Handler in der aiohttp-App.
    Callbacks werden per sub_type registriert und bei eingehenden Notifications aufgerufen.
    """

    def __init__(self, secret: str, logger: logging.Logger | None = None):
        if not secret:
            raise ValueError("EventSub webhook secret darf nicht leer sein")
        self._secret = secret.encode("utf-8")
        self.log = logger or logging.getLogger("TwitchStreams.EventSubWebhook")
        self._callbacks: dict[str, EventCallback] = {}
        self._seen_message_ids: dict[str, float] = {}
        self._seen_expiry_heap: list[tuple[float, str]] = []

    def set_callback(self, sub_type: str, callback: EventCallback) -> None:
        """Registriert einen Callback für einen bestimmten EventSub-Typ."""
        self._callbacks[sub_type] = callback
        self.log.debug("EventSub Webhook: Callback gesetzt für '%s'", sub_type)

    @staticmethod
    def _log_level_for_notification(sub_type: str) -> int:
        important_types = {
            "stream.online",
            "stream.offline",
            "channel.update",
            "channel.raid",
        }
        return logging.INFO if sub_type in important_types else logging.DEBUG

    @staticmethod
    def _extract_notification_context(
        data: dict,
        fallback_sub_type: str = "",
    ) -> tuple[str, str, str]:
        payload = data.get("payload")
        if isinstance(payload, dict) and ("event" in payload or "subscription" in payload):
            envelope = payload
        else:
            envelope = data

        subscription = envelope.get("subscription") or data.get("subscription") or {}
        actual_sub_type = str(subscription.get("type") or fallback_sub_type or "").strip()
        event = envelope.get("event") or data.get("event") or {}
        condition = subscription.get("condition") or {}

        broadcaster_id = str(
            event.get("broadcaster_user_id")
            or event.get("to_broadcaster_user_id")
            or event.get("user_id")
            or condition.get("broadcaster_user_id")
            or condition.get("to_broadcaster_user_id")
            or ""
        ).strip()
        broadcaster_login = (
            str(
                event.get("broadcaster_user_login")
                or event.get("to_broadcaster_user_login")
                or event.get("user_login")
                or ""
            )
            .strip()
            .lower()
        )
        return actual_sub_type, broadcaster_id, broadcaster_login

    def _verify_signature(
        self, message_id: str, timestamp: str, raw_body: bytes, signature: str
    ) -> bool:
        """
        Verifiziert die HMAC-SHA256 Signatur einer Twitch EventSub Nachricht.

        Formel: HMAC-SHA256(secret, message_id + timestamp + raw_body)
        """
        if not signature or not signature.startswith("sha256="):
            return False
        expected_sig = signature[7:]  # Strip "sha256="
        message = message_id.encode("utf-8") + timestamp.encode("utf-8") + raw_body
        computed = hmac.new(self._secret, message, hashlib.sha256).hexdigest()
        return hmac.compare_digest(computed, expected_sig)

    def _is_message_too_old(self, timestamp: str) -> bool:
        """Prüft ob der Timestamp älter als _MAX_MESSAGE_AGE_SECONDS ist (Replay-Schutz)."""
        try:
            dt = datetime.fromisoformat(timestamp.replace("Z", "+00:00"))
            age = (datetime.now(UTC) - dt).total_seconds()
            return age > _MAX_MESSAGE_AGE_SECONDS
        except Exception:
            self.log.debug("EventSub Webhook: Konnte Timestamp nicht parsen: %r", timestamp)
            return True  # Bei Parse-Fehler: Nachricht ablehnen

    def _now_timestamp(self) -> float:
        """Aktuellen UTC-Zeitstempel in Sekunden liefern (testbar überschreibbar)."""
        return datetime.now(UTC).timestamp()

    def _cleanup_expired_message_ids(self, now: float | None = None) -> None:
        """Entfernt alle abgelaufenen Message-IDs aus dem Replay-Cache."""
        if now is None:
            now = self._now_timestamp()
        while self._seen_expiry_heap and self._seen_expiry_heap[0][0] <= now:
            expiry, message_id = heapq.heappop(self._seen_expiry_heap)
            current_expiry = self._seen_message_ids.get(message_id)
            # Nur entfernen, wenn dieser Heap-Eintrag noch aktuell ist.
            if current_expiry == expiry:
                self._seen_message_ids.pop(message_id, None)

    def _is_duplicate(self, message_id: str) -> bool:
        """Duplikat-Erkennung anhand der Message-ID."""
        now = self._now_timestamp()
        self._cleanup_expired_message_ids(now)
        expiry = self._seen_message_ids.get(message_id)
        return bool(expiry is not None and expiry > now)

    def _track_message_id(self, message_id: str) -> bool:
        """Speichert Message-ID für spätere Duplikat-Erkennung (TTL-basiert)."""
        now = self._now_timestamp()
        self._cleanup_expired_message_ids(now)
        if message_id in self._seen_message_ids:
            return True
        if _SEEN_ID_HARD_LIMIT is not None and len(self._seen_message_ids) >= _SEEN_ID_HARD_LIMIT:
            return False

        expiry = now + _MAX_MESSAGE_AGE_SECONDS
        self._seen_message_ids[message_id] = expiry
        heapq.heappush(self._seen_expiry_heap, (expiry, message_id))
        return True

    @staticmethod
    def _missing_required_headers(
        *,
        message_id: str,
        timestamp: str,
        signature: str,
        message_type: str,
    ) -> list[str]:
        missing: list[str] = []
        if not message_id:
            missing.append("Twitch-Eventsub-Message-Id")
        if not timestamp:
            missing.append("Twitch-Eventsub-Message-Timestamp")
        if not signature:
            missing.append("Twitch-Eventsub-Message-Signature")
        if not message_type:
            missing.append("Twitch-Eventsub-Message-Type")
        return missing

    async def handle_request(self, request: web.Request) -> web.Response:
        """
        Haupt-Handler für eingehende EventSub Webhook Requests.

        Twitch sendet drei Message-Types:
        1. webhook_callback_verification – Challenge-Response bei neuen Subscriptions
        2. notification – Eigentliche Event-Notification
        3. revocation – Subscription wurde widerrufen
        """
        # --- 1. Raw Body lesen (vor JSON-Parsing für HMAC-Verifikation) ---
        try:
            raw_body = await request.read()
        except Exception:
            self.log.warning("EventSub Webhook: Konnte Body nicht lesen")
            return web.Response(status=400)

        # --- 2. Headers extrahieren ---
        message_id = request.headers.get("Twitch-Eventsub-Message-Id", "")
        timestamp = request.headers.get("Twitch-Eventsub-Message-Timestamp", "")
        signature = request.headers.get("Twitch-Eventsub-Message-Signature", "")
        message_type = request.headers.get("Twitch-Eventsub-Message-Type", "")
        sub_type = request.headers.get("Twitch-Eventsub-Subscription-Type", "")

        missing_headers = self._missing_required_headers(
            message_id=message_id,
            timestamp=timestamp,
            signature=signature,
            message_type=message_type,
        )
        if missing_headers:
            self.log.info(
                "EventSub Webhook: Ungueltige Anfrage ohne erforderliche Twitch-Header "
                "(missing=%s)",
                ",".join(missing_headers),
            )
            return web.Response(status=400)

        # --- 3. Signatur verifizieren ---
        if not self._verify_signature(message_id, timestamp, raw_body, signature):
            self.log.warning(
                "EventSub Webhook: Signatur-Verifizierung fehlgeschlagen (msg_id=%r, type=%r)",
                message_id,
                message_type,
            )
            return web.Response(status=403)

        # --- 4. Replay-Schutz: Timestamp prüfen ---
        if self._is_message_too_old(timestamp):
            self.log.warning(
                "EventSub Webhook: Nachricht zu alt (ts=%r, id=%r) – abgelehnt",
                timestamp,
                message_id,
            )
            return web.Response(status=403)

        # --- 5. JSON parsen ---
        try:
            import json

            data = json.loads(raw_body)
        except Exception:
            self.log.warning("EventSub Webhook: Konnte Body nicht als JSON parsen")
            return web.Response(status=400)

        # --- 6. Nach Message-Type verarbeiten ---
        if message_type == MSG_TYPE_CHALLENGE:
            challenge = data.get("challenge", "")
            if not challenge:
                self.log.error("EventSub Webhook: Challenge-Request ohne challenge-Feld")
                return web.Response(status=400)
            self.log.debug(
                "EventSub Webhook: Challenge für '%s' beantwortet",
                data.get("subscription", {}).get("type", sub_type),
            )
            return web.Response(
                text=challenge,
                content_type="text/plain",
                status=200,
            )

        if message_type == MSG_TYPE_REVOCATION:
            revoked_type = data.get("subscription", {}).get("type", sub_type)
            reason = data.get("subscription", {}).get("status", "unknown")
            self.log.warning(
                "EventSub Webhook: Subscription widerrufen: type=%r reason=%r",
                revoked_type,
                reason,
            )
            return web.Response(status=204)

        if message_type != MSG_TYPE_NOTIFICATION:
            # Unbekannter Typ – trotzdem mit 200 antworten damit Twitch nicht retried
            self.log.debug(
                "EventSub Webhook: Unbekannter message_type=%r – ignoriert",
                message_type,
            )
            return web.Response(status=204)

        # --- 7. Duplikat-Schutz ---
        if self._is_duplicate(message_id):
            self.log.debug("EventSub Webhook: Duplikat-Nachricht ignoriert (id=%r)", message_id)
            return web.Response(status=204)
        if not self._track_message_id(message_id):
            self.log.warning(
                "EventSub Webhook: Replay-Cache voll (entries=%d) – Nachricht abgelehnt",
                len(self._seen_message_ids),
            )
            return web.Response(status=503)

        # --- 8. Notification dispatchen ---
        actual_sub_type, broadcaster_id, broadcaster_login = self._extract_notification_context(
            data,
            sub_type,
        )
        log_level = self._log_level_for_notification(actual_sub_type)
        self.log.log(
            log_level,
            "EventSub Webhook: Notification accepted type=%r broadcaster=%r id=%r msg_id=%r",
            actual_sub_type or sub_type,
            broadcaster_login or broadcaster_id,
            broadcaster_id or None,
            message_id,
        )
        asyncio.create_task(
            self._dispatch_notification(data, sub_type, message_id=message_id),
            name="eventsub.webhook.dispatch",
        )
        return web.Response(status=204)

    async def _dispatch_notification(
        self,
        data: dict,
        sub_type: str,
        *,
        message_id: str = "",
    ) -> None:
        """Verarbeitet eine Notification und ruft den passenden Callback auf."""
        actual_sub_type, broadcaster_id, broadcaster_login = self._extract_notification_context(
            data,
            sub_type,
        )
        log_level = self._log_level_for_notification(actual_sub_type)

        callback = self._callbacks.get(actual_sub_type)
        if not callback:
            self.log.log(
                logging.WARNING if log_level == logging.INFO else logging.DEBUG,
                "EventSub Webhook: Kein Callback für type=%r broadcaster=%r id=%r msg_id=%r",
                actual_sub_type,
                broadcaster_login or broadcaster_id,
                broadcaster_id or None,
                message_id or None,
            )
            return

        payload = data.get("payload")
        if isinstance(payload, dict) and ("event" in payload or "subscription" in payload):
            envelope = payload
        else:
            envelope = data
        event = envelope.get("event") or data.get("event") or {}

        if not broadcaster_id:
            self.log.debug(
                "EventSub Webhook: Notification für type=%r ohne broadcaster_id – ignoriert (msg_id=%r)",
                actual_sub_type,
                message_id or None,
            )
            return

        self.log.log(
            log_level,
            "EventSub Webhook: Dispatch start type=%r broadcaster=%r id=%r msg_id=%r",
            actual_sub_type,
            broadcaster_login or broadcaster_id,
            broadcaster_id,
            message_id or None,
        )
        try:
            await callback(broadcaster_id, broadcaster_login, event)
        except Exception:
            self.log.exception(
                "EventSub Webhook: Callback fehlgeschlagen für type=%r broadcaster=%r msg_id=%r",
                actual_sub_type,
                broadcaster_login or broadcaster_id,
                message_id or None,
            )
        else:
            self.log.log(
                log_level,
                "EventSub Webhook: Dispatch completed type=%r broadcaster=%r id=%r msg_id=%r",
                actual_sub_type,
                broadcaster_login or broadcaster_id,
                broadcaster_id,
                message_id or None,
            )
