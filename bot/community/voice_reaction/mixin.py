"""Mixin, das Voice-Reaction in den `RaidChatBot` einsteckt.

Hält die Scheduler-Instanz, stellt `_open_conversation` bereit (für
`partner_recruit` und `raid_pipeline`) und liefert einen `event_message`-
Hook, der vom IRC-Handler bei jeder eingehenden Chat-Message aufgerufen
wird (`_voice_reaction_dispatch_message`).
"""

from __future__ import annotations

import asyncio
import logging
import os
from typing import Any

from .chat_listener import maybe_dispatch_chat_message
from .conversation_brain import BrainUnavailable, ConversationBrain
from .scheduler import VoiceReactionConfig, VoiceReactionScheduler

log = logging.getLogger("TwitchStreams.VoiceReaction.Mixin")


class TwitchPartnerVoiceReactionMixin:
    """Voice-Reaction-Hooks für den Twitch-IRC-Bot."""

    _voice_reaction_scheduler: VoiceReactionScheduler | None = None
    _voice_reaction_started: bool = False

    # ------------------------------------------------------------------
    # Lifecycle
    # ------------------------------------------------------------------
    async def _ensure_voice_reaction_started(self) -> VoiceReactionScheduler | None:
        if self._voice_reaction_scheduler is not None and self._voice_reaction_started:
            return self._voice_reaction_scheduler

        config = VoiceReactionConfig.from_env()
        if not config.enabled:
            log.info("VoiceReaction: deaktiviert via VOICE_REACTION_ENABLED=false")
            self._voice_reaction_scheduler = VoiceReactionScheduler(config=config, chat_bot=self)
            self._voice_reaction_started = True
            return self._voice_reaction_scheduler

        # Bot-Login aus Konfig fallback aus self.nick falls vorhanden
        if config.bot_login is None:
            login = getattr(self, "nick", None) or getattr(self, "_bot_login", None) or ""
            login = str(login or "").strip().lower()
            if login:
                config.bot_login = login

        brain: ConversationBrain | None = None
        try:
            brain = ConversationBrain()
        except BrainUnavailable as exc:
            log.warning("VoiceReaction: Brain inaktiv (%s) — Trigger laufen ohne LLM-Antwort", exc)

        transcribe = self._build_voice_reaction_transcriber()
        live_check = self._build_voice_reaction_live_check()

        self._voice_reaction_scheduler = VoiceReactionScheduler(
            config=config,
            chat_bot=self,
            brain=brain,
            transcribe=transcribe,
            live_check=live_check,
        )
        try:
            await self._voice_reaction_scheduler.start()
        except Exception:
            log.exception("VoiceReaction: Scheduler-Start fehlgeschlagen")
        self._voice_reaction_started = True
        return self._voice_reaction_scheduler

    async def _shutdown_voice_reaction(self) -> None:
        scheduler = self._voice_reaction_scheduler
        if scheduler is None:
            return
        try:
            await scheduler.stop()
        finally:
            self._voice_reaction_started = False

    # ------------------------------------------------------------------
    # Public Hooks
    # ------------------------------------------------------------------
    async def _open_conversation(
        self,
        login: str,
        user_id: str | None,
        *,
        source: str,
        initial_text: str | None = None,
    ) -> str | None:
        """Wird von `partner_recruit` und `raid_pipeline` aufgerufen."""
        scheduler = await self._ensure_voice_reaction_started()
        if scheduler is None:
            return None
        try:
            return await scheduler.open_conversation(
                login=login,
                user_id=user_id,
                source=source,
                initial_text=initial_text,
            )
        except Exception:
            log.exception(
                "VoiceReaction: open_conversation fehlgeschlagen login=%s source=%s",
                login,
                source,
            )
            return None

    async def _voice_reaction_dispatch_message(
        self,
        *,
        channel_login: str,
        author: Any,
        text: str,
    ) -> None:
        """Wird aus `event_message` aufgerufen — leichtgewichtig."""
        scheduler = self._voice_reaction_scheduler
        if scheduler is None or not scheduler.config.enabled:
            return
        if not scheduler.is_active_channel(channel_login):
            return
        try:
            await maybe_dispatch_chat_message(
                scheduler=scheduler,
                channel_login=channel_login,
                author=author,
                text=text,
                bot_login=scheduler.config.bot_login,
            )
        except Exception:
            log.debug("VoiceReaction: dispatch_message fehlgeschlagen", exc_info=True)

    # ------------------------------------------------------------------
    # Helpers
    # ------------------------------------------------------------------
    def _build_voice_reaction_transcriber(self):
        """Liefert eine async-Funktion file_path → TranscriptionResult."""
        engine_name = (
            os.getenv("VOICE_REACTION_TRANSCRIBER")
            or os.getenv("SOCIAL_MEDIA_TRANSCRIBER")
            or "openai_api"
        )
        try:
            from bot.social_media.transcription.whisper import (
                get_transcriber,
                transcribe_clip,
            )
        except Exception as exc:
            log.warning("VoiceReaction: Transcriber-Import fehlgeschlagen: %s", exc)
            return None

        try:
            engine = get_transcriber(engine_name)
        except Exception as exc:
            log.warning("VoiceReaction: Transcriber %s nicht verfügbar: %s", engine_name, exc)
            return None

        async def _runner(media_path):
            return await transcribe_clip(media_path, engine=engine)

        return _runner

    def _build_voice_reaction_live_check(self):
        """Optionaler Live-Check via Helix/streamer cache."""
        async def _is_live(login: str) -> bool:
            for attr in ("_is_streamer_live", "is_streamer_live", "_is_streamer_live_lookup"):
                fn = getattr(self, attr, None)
                if not callable(fn):
                    continue
                try:
                    result = fn(login)
                    if asyncio.iscoroutine(result):
                        result = await result
                    return bool(result)
                except Exception:
                    log.debug("VoiceReaction: Live-Check %s warf", attr, exc_info=True)
                    continue
            return True  # konservativ: capture versuchen, scheitert dann sauber

        return _is_live


__all__ = ["TwitchPartnerVoiceReactionMixin"]
