"""Twitch EventSub Webhook handler – empfängt und verifiziert eingehende Notifications."""

from __future__ import annotations

import asyncio
import heapq
import hashlib
import hmac
import inspect
import logging
from collections.abc import Awaitable, Callable
from datetime import UTC, datetime

from aiohttp import web
from .eventsub_core_callbacks import is_core_eventsub_delivery_type
from .eventsub_state_store import (
    EVENTSUB_STATE_KIND_MESSAGE_ID,
    EventSubStateStore,
)

EventCallback = Callable[..., Awaitable[None]]
RevocationCallback = Callable[..., Awaitable[None] | None]

# Twitch EventSub Message-Types
MSG_TYPE_NOTIFICATION = "notification"
MSG_TYPE_CHALLENGE = "webhook_callback_verification"
MSG_TYPE_REVOCATION = "revocation"

# Max allowed age for incoming messages (Twitch recommendation: 10 minutes)
_MAX_MESSAGE_AGE_SECONDS = 600

# Optional hard limit for in-memory dedupe entries.
# If set to an int, new IDs are rejected once full until old entries expire.
_SEEN_ID_HARD_LIMIT: int | None = None


class EventSubCallbackNotRegistered(RuntimeError):
    """Signals a valid notification that currently has no registered callback."""

    def __init__(self, sub_type: str) -> None:
        self.sub_type = str(sub_type or "").strip()
        super().__init__(f"no callback registered for EventSub type {self.sub_type!r}")


class EventSubWebhookHandler:
    """
    Empfängt und verifiziert Twitch EventSub Webhook Notifications.

    Registriert sich als Route-Handler in der aiohttp-App.
    Callbacks werden per sub_type registriert und bei eingehenden Notifications aufgerufen.
    """

    def __init__(
        self,
        secret: str,
        logger: logging.Logger | None = None,
        *,
        synchronous_notifications: bool = False,
        state_store: EventSubStateStore | None = None,
    ):
        if not secret:
            raise ValueError("EventSub webhook secret darf nicht leer sein")
        self._secret = secret.encode("utf-8")
        self.log = logger or logging.getLogger("TwitchStreams.EventSubWebhook")
        self._callbacks: dict[str, EventCallback] = {}
        self._revocation_callback: RevocationCallback | None = None
        self._seen_message_ids: dict[str, float] = {}
        self._seen_expiry_heap: list[tuple[float, str]] = []
        self._notification_dispatch_active = False
        self._synchronous_notifications = bool(synchronous_notifications)
        self._state_store = state_store

    def set_callback(self, sub_type: str, callback: EventCallback) -> None:
        """Registriert einen Callback für einen bestimmten EventSub-Typ."""
        self._callbacks[sub_type] = callback
        self.log.debug("EventSub Webhook: Callback gesetzt für '%s'", sub_type)

    def set_revocation_callback(self, callback: RevocationCallback | None) -> None:
        """Registriert einen Callback für Twitch-Revocations."""
        self._revocation_callback = callback
        self.log.debug(
            "EventSub Webhook: Revocation-Callback %s",
            "gesetzt" if callback else "entfernt",
        )

    def activate_notification_dispatch(self) -> None:
        """Erlaubt die Annahme eingehender Twitch-Notifications."""
        self._notification_dispatch_active = True
        self.log.debug(
            "EventSub Webhook: Notification-Dispatch aktiviert (callbacks=%d)",
            len(self._callbacks),
        )

    def deactivate_notification_dispatch(self) -> None:
        """Stoppt die Annahme eingehender Twitch-Notifications vorübergehend."""
        self._notification_dispatch_active = False
        self.log.debug("EventSub Webhook: Notification-Dispatch deaktiviert")

    def _has_callback(self, sub_type: str) -> bool:
        return bool(self._callbacks.get(str(sub_type or "").strip()))

    @staticmethod
    def _log_level_for_notification(sub_type: str) -> int:
        normalized_sub_type = str(sub_type or "").strip().lower()
        return (
            logging.INFO
            if is_core_eventsub_delivery_type(normalized_sub_type)
            else logging.DEBUG
        )

    def _should_process_notification_inline(self, sub_type: str) -> bool:
        normalized_sub_type = str(sub_type or "").strip().lower()
        return self._synchronous_notifications or is_core_eventsub_delivery_type(
            normalized_sub_type
        )

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
            return age > _MAX_MESSAGE_AGE_SECONDS or age < -_MAX_MESSAGE_AGE_SECONDS
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
        normalized = str(message_id or "").strip()
        if not normalized:
            return False
        now = self._now_timestamp()
        self._cleanup_expired_message_ids(now)
        expiry = self._seen_message_ids.get(normalized)
        if expiry is not None and expiry > now:
            return True
        if self._state_store is None:
            return False
        if self._state_store.is_active(EVENTSUB_STATE_KIND_MESSAGE_ID, normalized):
            self._remember_message_id_locally(normalized, now=now)
            return True
        return False

    def _remember_message_id_locally(self, message_id: str, *, now: float | None = None) -> None:
        normalized = str(message_id or "").strip()
        if not normalized:
            return
        current_now = self._now_timestamp() if now is None else now
        expiry = current_now + _MAX_MESSAGE_AGE_SECONDS
        self._seen_message_ids[normalized] = expiry
        heapq.heappush(self._seen_expiry_heap, (expiry, normalized))

    def _track_message_id(self, message_id: str) -> bool:
        """Speichert Message-ID für spätere Duplikat-Erkennung (TTL-basiert)."""
        normalized = str(message_id or "").strip()
        if not normalized:
            return False
        now = self._now_timestamp()
        self._cleanup_expired_message_ids(now)
        if normalized in self._seen_message_ids:
            return True
        if _SEEN_ID_HARD_LIMIT is not None and len(self._seen_message_ids) >= _SEEN_ID_HARD_LIMIT:
            return False
        if self._state_store is not None:
            claimed = self._state_store.claim(
                EVENTSUB_STATE_KIND_MESSAGE_ID,
                normalized,
                ttl_seconds=_MAX_MESSAGE_AGE_SECONDS,
            )
            if not claimed:
                return False
        self._remember_message_id_locally(normalized, now=now)
        return True

    def _forget_message_id(self, message_id: str) -> None:
        normalized = str(message_id or "").strip()
        if not normalized:
            return
        self._seen_message_ids.pop(normalized, None)
        if self._state_store is not None:
            self._state_store.release(EVENTSUB_STATE_KIND_MESSAGE_ID, normalized)

    def _claim_message_id(self, message_id: str) -> str:
        normalized = str(message_id or "").strip()
        if not normalized:
            return "invalid"
        if self._is_duplicate(normalized):
            return "duplicate"
        if not self._track_message_id(normalized):
            if self._is_duplicate(normalized):
                return "duplicate"
            return "rejected"
        return "tracked"

    def _assert_dispatch_ready(
        self,
        *,
        actual_sub_type: str,
        fallback_sub_type: str,
        broadcaster_id: str,
        broadcaster_login: str,
        message_id: str,
        internal: bool,
    ) -> None:
        kind = "Internal notification" if internal else "Notification"
        if not self._notification_dispatch_active:
            self.log.warning(
                "EventSub Webhook: %s empfangen bevor Dispatch aktiviert wurde "
                "(type=%r, broadcaster=%r, id=%r, msg_id=%r)",
                kind,
                actual_sub_type or fallback_sub_type,
                broadcaster_login or broadcaster_id,
                broadcaster_id or None,
                message_id or None,
            )
            raise RuntimeError("eventsub notification dispatch inactive")
        if not self._has_callback(actual_sub_type):
            self.log.warning(
                "EventSub Webhook: %s ohne registrierten Callback abgelehnt "
                "(type=%r, broadcaster=%r, id=%r, msg_id=%r)",
                kind,
                actual_sub_type or fallback_sub_type,
                broadcaster_login or broadcaster_id,
                broadcaster_id or None,
                message_id or None,
            )
            raise EventSubCallbackNotRegistered(actual_sub_type or fallback_sub_type)

    def _consume_dispatch_task_result(
        self,
        task: asyncio.Task[None],
        *,
        message_id: str = "",
        preserve_message_id_on_missing_callback: bool = False,
    ) -> None:
        try:
            exc = task.exception()
        except asyncio.CancelledError:
            self._forget_message_id(message_id)
            self.log.debug("EventSub Webhook: Dispatch-Task wurde abgebrochen")
            return
        if exc is None:
            return
        if preserve_message_id_on_missing_callback and isinstance(exc, EventSubCallbackNotRegistered):
            return
        self._forget_message_id(message_id)
        self.log.error(
            "EventSub Webhook: Dispatch-Task fehlgeschlagen (msg_id=%r)",
            message_id or None,
            exc_info=(type(exc), exc, exc.__traceback__),
        )

    def _queue_dispatch_task(
        self,
        data: dict,
        sub_type: str,
        *,
        message_id: str = "",
        preserve_message_id_on_missing_callback: bool = False,
    ) -> asyncio.Task[None]:
        dispatch_coro = self._dispatch_notification(data, sub_type, message_id=message_id)
        try:
            task = asyncio.create_task(
                dispatch_coro,
                name="eventsub.webhook.dispatch",
            )
        except Exception:
            dispatch_coro.close()
            raise
        task.add_done_callback(
            lambda completed: self._consume_dispatch_task_result(
                completed,
                message_id=message_id,
                preserve_message_id_on_missing_callback=preserve_message_id_on_missing_callback,
            )
        )
        return task

    async def _handle_revocation(
        self,
        data: dict,
        *,
        message_id: str = "",
    ) -> None:
        callback = self._revocation_callback
        if not callable(callback):
            return
        try:
            result = callback(
                data,
                message_id=message_id or None,
            )
            if inspect.isawaitable(result):
                await result
        except Exception:
            revoked_type = str(
                ((data.get("subscription") or {}).get("type") if isinstance(data, dict) else "") or ""
            ).strip()
            self.log.exception(
                "EventSub Webhook: Revocation-Callback fehlgeschlagen für type=%r msg_id=%r",
                revoked_type or None,
                message_id or None,
            )

    @staticmethod
    def _callback_accepts_kwarg(callback: EventCallback, name: str) -> bool:
        try:
            parameters = inspect.signature(callback).parameters.values()
        except (TypeError, ValueError):
            return False
        for parameter in parameters:
            if parameter.kind is inspect.Parameter.VAR_KEYWORD:
                return True
            if parameter.name == name:
                return True
        return False

    @classmethod
    def _callback_accepts_message_id(cls, callback: EventCallback) -> bool:
        return cls._callback_accepts_kwarg(callback, "message_id")

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

    def dispatch_notification_internal(
        self,
        data: dict,
        sub_type: str,
        *,
        message_id: str = "",
    ) -> dict[str, object]:
        """Legacy sync API removed; callers must await internal dispatch explicitly."""
        del data, sub_type, message_id
        raise RuntimeError(
            "dispatch_notification_internal is deprecated; use dispatch_notification_internal_async"
        )

    async def dispatch_notification_internal_async(
        self,
        data: dict,
        sub_type: str,
        *,
        message_id: str = "",
    ) -> dict[str, object]:
        """Dispatch an internal notification and honor synchronous delivery mode."""
        normalized_message_id = str(message_id or "").strip()
        actual_sub_type, broadcaster_id, broadcaster_login = self._extract_notification_context(
            data,
            sub_type,
        )
        self._assert_dispatch_ready(
            actual_sub_type=actual_sub_type,
            fallback_sub_type=str(sub_type or "").strip(),
            broadcaster_id=broadcaster_id,
            broadcaster_login=broadcaster_login,
            message_id=normalized_message_id,
            internal=True,
        )
        if normalized_message_id and self._is_duplicate(normalized_message_id):
            self.log.debug(
                "EventSub Webhook: Interne Duplikat-Nachricht ignoriert (id=%r)",
                normalized_message_id,
            )
            return {
                "ok": True,
                "duplicate": True,
                "queued": False,
                "sub_type": str(sub_type or "").strip(),
            }
        if normalized_message_id and not self._track_message_id(normalized_message_id):
            self.log.warning(
                "EventSub Webhook: Replay-Cache voll (entries=%d) – interne Nachricht abgelehnt",
                len(self._seen_message_ids),
            )
            raise RuntimeError("eventsub replay cache full")
        log_level = self._log_level_for_notification(actual_sub_type)
        self.log.log(
            log_level,
            "EventSub Webhook: Internal notification accepted type=%r broadcaster=%r id=%r msg_id=%r",
            actual_sub_type or sub_type,
            broadcaster_login or broadcaster_id,
            broadcaster_id or None,
            normalized_message_id or None,
        )
        if self._should_process_notification_inline(actual_sub_type or sub_type):
            try:
                await self._dispatch_notification(
                    data,
                    sub_type,
                    message_id=normalized_message_id,
                )
            except Exception:
                self._forget_message_id(normalized_message_id)
                raise
            return {
                "ok": True,
                "duplicate": False,
                "queued": False,
                "processed": True,
                "sub_type": actual_sub_type or str(sub_type or "").strip(),
            }
        try:
            self._queue_dispatch_task(
                data,
                sub_type,
                message_id=normalized_message_id,
            )
        except Exception:
            self._forget_message_id(normalized_message_id)
            self.log.exception(
                "EventSub Webhook: Internal dispatch could not be queued (msg_id=%r)",
                normalized_message_id or None,
            )
            raise
        return {
            "ok": True,
            "duplicate": False,
            "queued": True,
            "sub_type": actual_sub_type or str(sub_type or "").strip(),
        }

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
            await self._handle_revocation(data, message_id=message_id)
            return web.Response(status=204)

        if message_type != MSG_TYPE_NOTIFICATION:
            # Unbekannter Typ – trotzdem mit 200 antworten damit Twitch nicht retried
            self.log.debug(
                "EventSub Webhook: Unbekannter message_type=%r – ignoriert",
                message_type,
            )
            return web.Response(status=204)

        # --- 7. Notification-Readiness prüfen ---
        actual_sub_type, broadcaster_id, broadcaster_login = self._extract_notification_context(
            data,
            sub_type,
        )
        try:
            self._assert_dispatch_ready(
                actual_sub_type=actual_sub_type,
                fallback_sub_type=sub_type,
                broadcaster_id=broadcaster_id,
                broadcaster_login=broadcaster_login,
                message_id=message_id,
                internal=False,
            )
        except Exception:
            return web.Response(status=503)

        # --- 8. Duplikat-Schutz ---
        claim_result = self._claim_message_id(message_id)
        if claim_result == "duplicate":
            self.log.debug("EventSub Webhook: Duplikat-Nachricht ignoriert (id=%r)", message_id)
            return web.Response(status=204)
        if claim_result != "tracked":
            self.log.warning(
                "EventSub Webhook: Replay-Cache voll (entries=%d) – Nachricht abgelehnt",
                len(self._seen_message_ids),
            )
            return web.Response(status=503)

        # --- 9. Notification dispatchen ---
        log_level = self._log_level_for_notification(actual_sub_type)
        self.log.log(
            log_level,
            "EventSub Webhook: Notification accepted type=%r broadcaster=%r id=%r msg_id=%r",
            actual_sub_type or sub_type,
            broadcaster_login or broadcaster_id,
            broadcaster_id or None,
            message_id,
        )
        if self._should_process_notification_inline(actual_sub_type or sub_type):
            try:
                await self._dispatch_notification(data, sub_type, message_id=message_id)
            except Exception:
                self._forget_message_id(message_id)
                return web.Response(status=503)
            return web.Response(status=204)
        try:
            self._queue_dispatch_task(
                data,
                sub_type,
                message_id=message_id,
            )
        except Exception:
            self._forget_message_id(message_id)
            self.log.exception(
                "EventSub Webhook: Notification dispatch could not be queued (msg_id=%r)",
                message_id or None,
            )
            return web.Response(status=503)
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
            raise EventSubCallbackNotRegistered(actual_sub_type)

        payload = data.get("payload")
        if isinstance(payload, dict) and ("event" in payload or "subscription" in payload):
            envelope = payload
        else:
            envelope = data
        event = envelope.get("event") or data.get("event") or {}
        subscription = envelope.get("subscription") or data.get("subscription") or {}

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
            callback_kwargs: dict[str, object] = {}
            if self._callback_accepts_message_id(callback):
                callback_kwargs["message_id"] = message_id or None
            if self._callback_accepts_kwarg(callback, "subscription"):
                callback_kwargs["subscription"] = subscription if isinstance(subscription, dict) else {}
            if self._callback_accepts_kwarg(callback, "payload"):
                callback_kwargs["payload"] = envelope if isinstance(envelope, dict) else {}
            await callback(
                broadcaster_id,
                broadcaster_login,
                event,
                **callback_kwargs,
            )
        except Exception:
            self.log.exception(
                "EventSub Webhook: Callback fehlgeschlagen für type=%r broadcaster=%r msg_id=%r",
                actual_sub_type,
                broadcaster_login or broadcaster_id,
                message_id or None,
            )
            raise
        else:
            self.log.log(
                log_level,
                "EventSub Webhook: Dispatch completed type=%r broadcaster=%r id=%r msg_id=%r",
                actual_sub_type,
                broadcaster_login or broadcaster_id,
                broadcaster_id,
                message_id or None,
            )
