"""Asyncio-basierter Voice-Reaction-Scheduler.

Stellt zwei Trigger-Pfade bereit:

1. **Voice-Trigger** — Audio-Capture via `audio_capture.capture()`, Whisper-
   Transkription, anschließend Brain-Call.
2. **Chat-Trigger** — direkte Brain-Call ohne Audio (z. B. wenn der Streamer
   selbst etwas in seinen Chat schreibt).

Der Scheduler hält den Konversations-Zustand bewusst nur in der Datenbank
(`state_store`/`audit_log`). In-Memory existieren lediglich:
- die Worker-Queue,
- ein Set offener Channels (für O(1)-Lookups im IRC-Listener),
- ein Lock pro Konversation, damit ein Brain-Call nicht parallel doppelt
  läuft.
"""

from __future__ import annotations

import asyncio
import logging
import os
import random
import time
from dataclasses import dataclass, field
from datetime import UTC, datetime, timedelta
from typing import Any, Awaitable, Callable, Mapping, Sequence

from . import audio_capture, audit_log, chat_message_sender, discord_notifier, state_store
from .conversation_brain import (
    BrainCallInput,
    BrainCallOutput,
    BrainError,
    BrainUnavailable,
    ConversationBrain,
)

log = logging.getLogger("TwitchStreams.VoiceReaction.Scheduler")

DEFAULT_MAX_PARALLEL = 2
DEFAULT_CAPTURE_SECONDS = 75
DEFAULT_INACTIVITY_DAYS = 7
DEFAULT_FOLLOWUP_RECHECK_HOURS = 24
DEFAULT_RANDOM_SPREAD_RANGE = (60, 240)
COOLDOWN_DAYS_AFTER_CLOSE = 90
INACTIVITY_CHECK_INTERVAL_SECONDS = 1800  # 30 min


def _env_bool(name: str, default: bool) -> bool:
    raw = os.getenv(name)
    if raw is None:
        return default
    return str(raw).strip().lower() in {"1", "true", "yes", "y", "on"}


def _env_int(name: str, default: int) -> int:
    raw = os.getenv(name)
    if raw is None:
        return default
    try:
        return int(str(raw).strip())
    except (TypeError, ValueError):
        return default


@dataclass
class VoiceReactionConfig:
    enabled: bool = False
    dry_run: bool = True
    capture_seconds: int = DEFAULT_CAPTURE_SECONDS
    quality: str = audio_capture.DEFAULT_QUALITY
    max_parallel: int = DEFAULT_MAX_PARALLEL
    max_daily_transcriptions: int = 30
    followup_recheck_hours: int = DEFAULT_FOLLOWUP_RECHECK_HOURS
    inactivity_days: int = DEFAULT_INACTIVITY_DAYS
    bot_login: str | None = None
    random_spread_seconds: tuple[int, int] = DEFAULT_RANDOM_SPREAD_RANGE

    @classmethod
    def from_env(cls) -> "VoiceReactionConfig":
        return cls(
            enabled=_env_bool("VOICE_REACTION_ENABLED", False),
            dry_run=_env_bool("VOICE_REACTION_DRY_RUN", True),
            capture_seconds=_env_int("VOICE_REACTION_CAPTURE_SECONDS", DEFAULT_CAPTURE_SECONDS),
            quality=os.getenv("VOICE_REACTION_QUALITY", audio_capture.DEFAULT_QUALITY) or audio_capture.DEFAULT_QUALITY,
            max_parallel=_env_int("VOICE_REACTION_MAX_PARALLEL", DEFAULT_MAX_PARALLEL),
            max_daily_transcriptions=_env_int("VOICE_REACTION_MAX_DAILY_TRANSCRIPTIONS", 30),
            followup_recheck_hours=_env_int("VOICE_REACTION_FOLLOWUP_RECHECK_HOURS", DEFAULT_FOLLOWUP_RECHECK_HOURS),
            inactivity_days=_env_int("VOICE_REACTION_INACTIVITY_DAYS", DEFAULT_INACTIVITY_DAYS),
            bot_login=(os.getenv("VOICE_REACTION_BOT_LOGIN") or "").strip().lower() or None,
        )


# ---------- Trigger-Datenstrukturen ----------


@dataclass(order=True)
class _Trigger:
    """Internes Queue-Item."""

    not_before: float = field(compare=True)
    seq: int = field(compare=True)
    kind: str = field(compare=False)  # 'voice' | 'chat'
    streamer_login: str = field(compare=False)
    streamer_user_id: str | None = field(compare=False, default=None)
    chat_text: str | None = field(compare=False, default=None)
    chat_author: str | None = field(compare=False, default=None)
    correlation_id: str = field(compare=False, default="")
    is_initial: bool = field(compare=False, default=False)


# ---------- Public Scheduler ----------


class VoiceReactionScheduler:
    """Worker-Pool + Resume-Loop für Voice-Reaction-Konversationen."""

    def __init__(
        self,
        *,
        config: VoiceReactionConfig | None = None,
        chat_bot: Any | None = None,
        brain: ConversationBrain | None = None,
        transcribe: Callable[[Any], Awaitable[Any]] | None = None,
        live_check: Callable[[str], Awaitable[bool]] | None = None,
        webhook_url_override: str | None = None,
        capture_runner: Callable[..., Awaitable[tuple[int, bytes]]] | None = None,
    ) -> None:
        self._config = config or VoiceReactionConfig.from_env()
        self._chat_bot = chat_bot
        self._brain = brain
        self._transcribe = transcribe
        self._live_check = live_check
        self._webhook_url_override = webhook_url_override
        self._capture_runner = capture_runner

        self._queue: asyncio.PriorityQueue[_Trigger] = asyncio.PriorityQueue()
        self._semaphore: asyncio.Semaphore | None = None
        self._workers: list[asyncio.Task] = []
        self._inactivity_task: asyncio.Task | None = None
        self._seq = 0
        self._active_channels: set[str] = set()
        self._conversation_locks: dict[str, asyncio.Lock] = {}
        self._daily_transcription_day: str | None = None
        self._daily_transcription_count: int = 0
        self._started = False

    # -------------- Lifecycle --------------

    async def start(self) -> None:
        if self._started:
            return
        if not self._config.enabled:
            log.info("VoiceReaction: deaktiviert (VOICE_REACTION_ENABLED=false)")
            self._started = True
            return

        # Lazy-Brain bauen falls noch nicht injiziert
        if self._brain is None:
            try:
                self._brain = ConversationBrain()
            except BrainUnavailable as exc:
                log.warning(
                    "VoiceReaction: Brain nicht verfügbar (%s) — Scheduler läuft im DRY_RUN-only-Modus",
                    exc,
                )
                self._brain = None

        self._semaphore = asyncio.Semaphore(max(1, self._config.max_parallel))
        try:
            audio_capture.cleanup_stale_capture_dirs()
        except Exception:
            log.debug("VoiceReaction: Boot-Cleanup fehlgeschlagen", exc_info=True)

        await self._resume_from_db()

        for idx in range(max(1, self._config.max_parallel)):
            task = asyncio.create_task(
                self._worker_loop(),
                name=f"voice-reaction-worker-{idx}",
            )
            self._workers.append(task)

        self._inactivity_task = asyncio.create_task(
            self._inactivity_loop(),
            name="voice-reaction-inactivity",
        )

        self._started = True
        log.info(
            "VoiceReaction: gestartet enabled=%s dry_run=%s parallel=%s",
            self._config.enabled,
            self._config.dry_run,
            self._config.max_parallel,
        )

    async def stop(self) -> None:
        for task in self._workers:
            task.cancel()
        if self._inactivity_task is not None:
            self._inactivity_task.cancel()
        for task in list(self._workers) + ([self._inactivity_task] if self._inactivity_task else []):
            try:
                await task
            except (asyncio.CancelledError, Exception):
                pass
        self._workers.clear()
        self._inactivity_task = None
        self._started = False

    # -------------- Public API --------------

    @property
    def config(self) -> VoiceReactionConfig:
        return self._config

    def is_active_channel(self, login: str) -> bool:
        return str(login or "").strip().lower() in self._active_channels

    async def open_conversation(
        self,
        *,
        login: str,
        user_id: str | None,
        source: str,
        initial_text: str | None = None,
    ) -> str | None:
        """Legt eine Conversation-Row an + queued einen ersten Voice-Trigger.

        Liefert die `correlation_id` zurück — nützlich für Tests / Trigger-Caller.
        Wenn der Scheduler deaktiviert ist, passiert nichts.
        """
        normalized = str(login or "").strip().lower()
        if not normalized:
            return None
        if not self._config.enabled:
            return None

        initial_messages: list[dict[str, object]] = []
        if initial_text:
            initial_messages.append(
                {
                    "role": "bot_chat",
                    "ts": _now_iso(),
                    "text": initial_text,
                    "meta": {"source": source, "kind": "initial_outreach"},
                }
            )

        opened = state_store.open_conversation(
            streamer_login=normalized,
            streamer_user_id=str(user_id) if user_id else None,
            source=source,
            initial_messages=initial_messages,
        )
        correlation_id = audit_log.new_correlation_id()
        if opened:
            audit_log.audit(
                normalized,
                "conversation_opened",
                {
                    "source": source,
                    "initial_text": initial_text or "",
                    "user_id": str(user_id) if user_id else None,
                },
                correlation_id=correlation_id,
            )
            log.info(
                "VoiceReaction: Conversation eröffnet login=%s source=%s",
                normalized,
                source,
            )
        else:
            log.debug(
                "VoiceReaction: Conversation existiert bereits oder konnte nicht angelegt werden login=%s",
                normalized,
            )

        self._active_channels.add(normalized)

        delay = random.randint(*self._config.random_spread_seconds)
        await self._enqueue(
            _Trigger(
                not_before=time.time() + delay,
                seq=self._next_seq(),
                kind="voice",
                streamer_login=normalized,
                streamer_user_id=str(user_id) if user_id else None,
                correlation_id=correlation_id,
                is_initial=True,
            )
        )
        return correlation_id

    async def enqueue_voice(
        self,
        *,
        login: str,
        user_id: str | None = None,
        delay_seconds: int | None = None,
    ) -> None:
        if not self._config.enabled:
            return
        normalized = str(login or "").strip().lower()
        if not normalized:
            return
        delay = (
            delay_seconds
            if delay_seconds is not None
            else random.randint(*self._config.random_spread_seconds)
        )
        await self._enqueue(
            _Trigger(
                not_before=time.time() + max(0, int(delay)),
                seq=self._next_seq(),
                kind="voice",
                streamer_login=normalized,
                streamer_user_id=str(user_id) if user_id else None,
                correlation_id=audit_log.new_correlation_id(),
            )
        )

    async def enqueue_chat(
        self,
        *,
        login: str,
        text: str,
        author: str | None,
        user_id: str | None = None,
    ) -> None:
        if not self._config.enabled:
            return
        normalized = str(login or "").strip().lower()
        if not normalized:
            return
        if normalized not in self._active_channels:
            return
        await self._enqueue(
            _Trigger(
                not_before=time.time(),
                seq=self._next_seq(),
                kind="chat",
                streamer_login=normalized,
                streamer_user_id=str(user_id) if user_id else None,
                chat_text=str(text or ""),
                chat_author=str(author or "") or None,
                correlation_id=audit_log.new_correlation_id(),
            )
        )

    # -------------- Worker --------------

    async def _enqueue(self, trigger: _Trigger) -> None:
        await self._queue.put(trigger)

    def _next_seq(self) -> int:
        self._seq += 1
        return self._seq

    async def _worker_loop(self) -> None:
        try:
            while True:
                trigger = await self._queue.get()
                try:
                    await self._handle_trigger(trigger)
                except Exception:
                    log.exception(
                        "VoiceReaction: Worker-Crash bei Trigger %s/%s",
                        trigger.kind,
                        trigger.streamer_login,
                    )
                finally:
                    self._queue.task_done()
        except asyncio.CancelledError:
            return

    async def _handle_trigger(self, trigger: _Trigger) -> None:
        wait = max(0.0, trigger.not_before - time.time())
        if wait > 0:
            await asyncio.sleep(wait)

        login = trigger.streamer_login
        if login not in self._active_channels:
            log.debug("VoiceReaction: skippe Trigger %s — Channel nicht (mehr) aktiv", login)
            return

        lock = self._conversation_locks.setdefault(login, asyncio.Lock())
        async with lock:
            if trigger.kind == "voice":
                await self._handle_voice_trigger(trigger)
            elif trigger.kind == "chat":
                await self._handle_chat_trigger(trigger)
            else:
                log.warning("VoiceReaction: unbekannter Trigger-Kind %r", trigger.kind)

    # -------------- Voice-Pfad --------------

    async def _handle_voice_trigger(self, trigger: _Trigger) -> None:
        login = trigger.streamer_login

        if self._cost_cap_reached():
            audit_log.audit(
                login,
                "cost_cap_skip",
                {"max_daily_transcriptions": self._config.max_daily_transcriptions},
                correlation_id=trigger.correlation_id,
            )
            return

        if self._live_check is not None:
            try:
                is_live = await self._live_check(login)
            except Exception:
                log.debug("VoiceReaction: Live-Check fehlgeschlagen", exc_info=True)
                is_live = False
            if not is_live:
                audit_log.audit(
                    login,
                    "voice_capture_failed",
                    {"reason": "offline"},
                    correlation_id=trigger.correlation_id,
                )
                return

        assert self._semaphore is not None
        async with self._semaphore:
            audit_log.audit(
                login,
                "voice_capture_started",
                {
                    "duration_target": self._config.capture_seconds,
                    "quality": self._config.quality,
                },
                correlation_id=trigger.correlation_id,
            )
            try:
                capture_result = await audio_capture.capture(
                    login,
                    duration_seconds=self._config.capture_seconds,
                    quality=self._config.quality,
                    runner=self._capture_runner,
                )
            except audio_capture.AudioCaptureError as exc:
                audit_log.audit(
                    login,
                    "voice_capture_failed",
                    {"reason": str(exc)},
                    correlation_id=trigger.correlation_id,
                )
                return

            audit_log.audit(
                login,
                "voice_capture_done",
                {
                    "bytes": capture_result.bytes,
                    "duration_actual_s": capture_result.actual_duration_seconds,
                    "quality": capture_result.quality,
                    "media_path": str(capture_result.media_path),
                },
                correlation_id=trigger.correlation_id,
            )

            transcript_text = ""
            transcript_payload: dict[str, object] = {}
            try:
                if self._transcribe is not None:
                    result = await self._transcribe(capture_result.media_path)
                    transcript_text, transcript_payload = _transcript_to_payload(result)
                else:
                    log.debug("VoiceReaction: kein Transcriber injiziert, skippe Whisper")
            except Exception as exc:
                audit_log.audit(
                    login,
                    "whisper_failed",
                    {"error": str(exc)},
                    correlation_id=trigger.correlation_id,
                )
                capture_result.cleanup()
                return
            finally:
                capture_result.cleanup()

            self._track_daily_transcription()

            audit_log.audit(
                login,
                "whisper_call",
                transcript_payload,
                correlation_id=trigger.correlation_id,
            )

            if transcript_text:
                state_store.append_message(
                    streamer_login=login,
                    role="voice",
                    text=transcript_text,
                    meta={
                        "duration_s": transcript_payload.get("duration_s"),
                        "language": transcript_payload.get("language"),
                    },
                )

        if not transcript_text:
            log.debug("VoiceReaction: leeres Transkript für %s — keine Brain-Call", login)
            return

        await self._run_brain_call(
            login=login,
            user_id=trigger.streamer_user_id,
            latest_signal_kind="voice",
            latest_signal_text=transcript_text,
            latest_signal_meta={
                "duration_s": transcript_payload.get("duration_s"),
                "language": transcript_payload.get("language"),
            },
            correlation_id=trigger.correlation_id,
            source_trigger="voice",
        )

    # -------------- Chat-Pfad --------------

    async def _handle_chat_trigger(self, trigger: _Trigger) -> None:
        login = trigger.streamer_login
        text = (trigger.chat_text or "").strip()
        if not text:
            return

        audit_log.audit(
            login,
            "streamer_chat_received",
            {"author": trigger.chat_author, "text": text},
            correlation_id=trigger.correlation_id,
        )

        state_store.append_message(
            streamer_login=login,
            role="streamer_chat",
            text=text,
            meta={"author": trigger.chat_author},
        )

        await self._run_brain_call(
            login=login,
            user_id=trigger.streamer_user_id,
            latest_signal_kind="streamer_chat",
            latest_signal_text=text,
            latest_signal_meta={"author": trigger.chat_author},
            correlation_id=trigger.correlation_id,
            source_trigger="chat",
        )

    # -------------- Brain-Call --------------

    async def _run_brain_call(
        self,
        *,
        login: str,
        user_id: str | None,
        latest_signal_kind: str,
        latest_signal_text: str,
        latest_signal_meta: Mapping[str, object] | None,
        correlation_id: str,
        source_trigger: str,
    ) -> None:
        if self._brain is None:
            audit_log.audit(
                login,
                "brain_failed",
                {"reason": "brain_unavailable"},
                correlation_id=correlation_id,
            )
            return

        conversation = state_store.get_conversation(streamer_login=login)
        if conversation is None:
            log.debug("VoiceReaction: Conversation %s nicht gefunden, skippe Brain", login)
            return
        if conversation.get("state", "").startswith("closed_"):
            log.debug("VoiceReaction: Conversation %s ist closed, skippe Brain", login)
            self._active_channels.discard(login)
            return

        history: Sequence[Mapping[str, object]] = list(conversation.get("messages_json") or [])
        streamer_context = {
            "login": login,
            "user_id": user_id or conversation.get("streamer_user_id"),
            "language": "de",
            "current_game": "Deadlock",
            "trigger_source": conversation.get("source") or source_trigger,
        }

        state_store.update_state(streamer_login=login, new_state="brain_pending")

        try:
            call_input, call_output = await self._brain.respond(
                streamer_context=streamer_context,
                history=history,
                latest_signal_kind=latest_signal_kind,
                latest_signal_text=latest_signal_text,
                latest_signal_meta=latest_signal_meta or {},
            )
        except BrainError as exc:
            audit_log.audit(
                login,
                "brain_failed",
                {"error": str(exc)},
                correlation_id=correlation_id,
            )
            state_store.update_state(streamer_login=login, new_state="listening")
            return

        audit_log.audit(
            login,
            "brain_call_input",
            call_input.to_audit_payload(),
            correlation_id=correlation_id,
        )
        audit_log.audit(
            login,
            "brain_call_output",
            call_output.to_audit_payload(),
            correlation_id=correlation_id,
        )

        decision = call_output.decision

        # Versende-Pfad
        if decision.should_respond and decision.response_text:
            outcome = await chat_message_sender.send_response(
                chat_bot=self._chat_bot,
                streamer_login=login,
                streamer_user_id=user_id,
                response_text=decision.response_text,
                bot_login=self._config.bot_login,
                dry_run=self._config.dry_run,
            )
            audit_log.audit(
                login,
                "bot_message_sent" if outcome.sent else "bot_message_send_failed",
                outcome.to_audit_payload(),
                correlation_id=correlation_id,
            )
            if outcome.sent:
                state_store.append_message(
                    streamer_login=login,
                    role="bot_chat",
                    text=outcome.filter_result.filtered_text,
                    meta={
                        "stance": decision.stance,
                        "confidence": decision.confidence,
                    },
                )

        # Discord-Notify: Pending-Marker setzen (Discord-Bot pollt + sendet DM)
        if (
            decision.should_notify_human
            and not conversation.get("human_notify_sent_at")
            and not conversation.get("human_notify_pending_at")
        ):
            self._mark_human_notify_pending(login)
            audit_log.audit(
                login,
                "discord_notify_pending",
                {
                    "stance": decision.stance,
                    "confidence": decision.confidence,
                    "trigger_source": source_trigger,
                },
                correlation_id=correlation_id,
            )

        # State-Übergang
        if decision.should_close:
            close_reason = (decision.close_reason or "").strip().lower() or "exhausted"
            extend_cooldown = (
                COOLDOWN_DAYS_AFTER_CLOSE
                if close_reason in {"declined", "exhausted"}
                else None
            )
            audit_log.audit(
                login,
                "state_transition",
                {"from": "brain_pending", "to": f"closed_{close_reason}"},
                correlation_id=correlation_id,
            )
            state_store.close_conversation(
                streamer_login=login,
                close_reason=close_reason,
                extend_cooldown_days=extend_cooldown,
            )
            audit_log.audit(
                login,
                "conversation_closed",
                {
                    "close_reason": close_reason,
                    "stance": decision.stance,
                    "trigger_source": source_trigger,
                },
                correlation_id=correlation_id,
            )
            self._active_channels.discard(login)
            return

        audit_log.audit(
            login,
            "state_transition",
            {"from": "brain_pending", "to": "listening"},
            correlation_id=correlation_id,
        )
        state_store.update_state(
            streamer_login=login,
            new_state="listening",
            last_stance=decision.stance,
            last_confidence=decision.confidence,
        )

        if decision.suggest_voice_recheck_after_seconds is not None:
            recheck = max(0, int(decision.suggest_voice_recheck_after_seconds))
            if recheck > 0:
                await self.enqueue_voice(
                    login=login,
                    user_id=user_id,
                    delay_seconds=recheck,
                )

    # -------------- Resume / Inactivity --------------

    async def _resume_from_db(self) -> None:
        active = state_store.load_active_conversations()
        for entry in active:
            login = str(entry.get("streamer_login") or "").strip().lower()
            if login:
                self._active_channels.add(login)
        log.info("VoiceReaction: Resume — %d offene Konversationen geladen", len(active))

    async def _inactivity_loop(self) -> None:
        try:
            while True:
                await asyncio.sleep(INACTIVITY_CHECK_INTERVAL_SECONDS)
                try:
                    await self._sweep_inactivity()
                except Exception:
                    log.exception("VoiceReaction: Inactivity-Sweep gecrasht")
        except asyncio.CancelledError:
            return

    async def _sweep_inactivity(self) -> None:
        cutoff = datetime.now(UTC) - timedelta(days=self._config.inactivity_days)
        for entry in state_store.load_active_conversations():
            last_signal_raw = entry.get("last_streamer_signal_at")
            login = str(entry.get("streamer_login") or "").strip().lower()
            if not login:
                continue
            if last_signal_raw is None:
                continue
            try:
                last_signal = datetime.fromisoformat(str(last_signal_raw).replace("Z", "+00:00"))
            except ValueError:
                continue
            if last_signal.tzinfo is None:
                last_signal = last_signal.replace(tzinfo=UTC)
            if last_signal < cutoff:
                state_store.close_conversation(
                    streamer_login=login,
                    close_reason="no_signal",
                )
                audit_log.audit(
                    login,
                    "conversation_closed",
                    {"close_reason": "no_signal"},
                )
                self._active_channels.discard(login)

    # -------------- Helpers --------------

    def _cost_cap_reached(self) -> bool:
        today = datetime.now(UTC).strftime("%Y-%m-%d")
        if self._daily_transcription_day != today:
            self._daily_transcription_day = today
            self._daily_transcription_count = 0
        return self._daily_transcription_count >= self._config.max_daily_transcriptions

    def _track_daily_transcription(self) -> None:
        today = datetime.now(UTC).strftime("%Y-%m-%d")
        if self._daily_transcription_day != today:
            self._daily_transcription_day = today
            self._daily_transcription_count = 0
        self._daily_transcription_count += 1

    def _mark_human_notify_pending(self, login: str) -> None:
        try:
            from bot.storage import transaction

            with transaction() as conn:
                conn.execute(
                    """
                    UPDATE twitch_partner_outreach_conversations
                       SET human_notify_pending_at = NOW()
                     WHERE streamer_login = %s
                       AND human_notify_pending_at IS NULL
                    """,
                    (login,),
                )
                conn.commit()
        except Exception:
            log.debug("VoiceReaction: human_notify_pending_at-Update fehlgeschlagen", exc_info=True)


def _now_iso() -> str:
    return datetime.now(UTC).isoformat()


def _transcript_to_payload(result: Any) -> tuple[str, dict[str, object]]:
    text = str(getattr(result, "text", "") or "").strip()
    payload: dict[str, object] = {
        "engine": str(getattr(result, "engine", "") or ""),
        "model": str(getattr(result, "model", "") or ""),
        "language": str(getattr(result, "language", "") or "") or None,
        "duration_s": getattr(result, "duration_seconds", None),
        "segments_count": len(list(getattr(result, "segments", None) or [])),
        "transcript_text": text,
    }
    return text, payload


__all__ = [
    "VoiceReactionConfig",
    "VoiceReactionScheduler",
]
