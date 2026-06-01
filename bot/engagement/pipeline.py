"""Engagement-Pipeline: Haupt-Orchestrator pro eingehender Chat-Message.

Aufgerufen aus `bot/chat/bot.py:event_message()` (best-effort, Fehler
loggen aber nicht blockieren). Returnt `HandleResult` — bei `decision=SPOKE`
ist `response_text` gesetzt, der Hook in event_message sendet den Text in den
Chat.

Gate-Reihenfolge:
    1. settings = load(channel) → None/disabled → Decision.DISABLED
    2. opted_out(user) → Decision.OPTOUT
    3. note_user_post + append_user_turn (Fehler → PROVIDER_ERROR)
    4. anti_flood_ok? → Decision.FLOOD_GUARD
    5. anti_burst_ok? → Decision.ANTI_BURST
    6. load_history + build_system_prompt + minimax.generate
       - response.text is None → Decision.SILENT
       - sonst → note_bot_post + append_assistant_turn + Decision.SPOKE
    7. Exception bei Modell-Call → Decision.PROVIDER_ERROR
"""

from __future__ import annotations

import asyncio
import logging
import os
from dataclasses import dataclass
from datetime import datetime, timezone
from enum import Enum

from bot.storage.pg import query_one, transaction

from .conversation import ConversationBuffer
from .minimax_chat import (
    ChatMessage,
    EngagementMinimaxClient,
    LLMProviderUnavailable,
    build_baseline_system_prompt,
)
from .lurker_signal import known_regulars_currently_lurking, lurker_hint_to_prompt_fragment
from .match_context import get_match_state
from .persona import sample_tone
from .rhythm import RhythmGuard
from .style_examples import build_style_fragment
from .stream_transcripts import load_recent_segments, segments_to_prompt_fragment
from .threads import load_open_threads_for_user, mark_referenced, threads_to_prompt_fragment

log = logging.getLogger("TwitchStreams.Engagement.Pipeline")


def _calc_cost_usd(prompt_tokens: int | None, completion_tokens: int | None) -> float | None:
    if prompt_tokens is None or completion_tokens is None:
        return None
    try:
        input_rate = float(os.getenv("MINIMAX_PRICE_INPUT_PER_1K", "0.0008"))
        output_rate = float(os.getenv("MINIMAX_PRICE_OUTPUT_PER_1K", "0.0024"))
    except ValueError:
        input_rate, output_rate = 0.0008, 0.0024
    return (prompt_tokens / 1000.0) * input_rate + (completion_tokens / 1000.0) * output_rate


def _sync_log_decision(
    *,
    channel_login: str,
    triggered_by_msg_id: str | None,
    decision: str,
    response_text: str | None,
    referenced_thread_ids: list[int] | None,
    model: str | None,
    prompt_tokens: int | None,
    completion_tokens: int | None,
    cost_usd: float | None,
    latency_ms: int | None,
) -> None:
    with transaction() as conn:
        conn.execute(
            """
            INSERT INTO twitch_engagement_log
                (channel_login, triggered_by_msg_id, decision, response_text,
                 referenced_thread_ids, model, prompt_tokens, completion_tokens,
                 cost_usd_estimate, latency_ms)
            VALUES (%s, %s, %s, %s, %s, %s, %s, %s, %s, %s)
            """,
            [
                channel_login,
                triggered_by_msg_id,
                decision,
                response_text,
                referenced_thread_ids,
                model or "",
                prompt_tokens,
                completion_tokens,
                cost_usd,
                latency_ms,
            ],
        )


class Decision(str, Enum):
    SPOKE = "spoke"
    SILENT = "silent"
    ANTI_BURST = "anti_burst"
    FLOOD_GUARD = "flood_guard"
    OPTOUT = "optout"
    DISABLED = "disabled"
    PROVIDER_ERROR = "provider_error"


@dataclass(slots=True)
class EngagementSettings:
    channel_login: str
    enabled: bool
    steam_id: str | None
    persona_override: str | None
    tabu_topics: list[str]


@dataclass(slots=True)
class IncomingMessage:
    channel_login: str
    twitch_user_id: str
    twitch_login: str
    content: str
    message_id: str | None


@dataclass(slots=True)
class HandleResult:
    decision: Decision
    response_text: str | None = None
    model: str | None = None
    prompt_tokens: int | None = None
    completion_tokens: int | None = None
    latency_ms: int | None = None
    referenced_thread_ids: list[int] | None = None


def _sync_load_settings(channel_login: str):
    return query_one(
        """
        SELECT channel_login, enabled, steam_id, persona_override, tabu_topics
        FROM twitch_engagement_settings
        WHERE channel_login = %s
        """,
        [channel_login],
    )


def _sync_is_opted_out(twitch_user_id: str) -> bool:
    row = query_one(
        """
        SELECT 1 FROM twitch_user_engagement_optout WHERE twitch_user_id = %s
        """,
        [twitch_user_id],
    )
    return row is not None


class EngagementPipeline:
    """V1 minimal: Buffer + Gates + Modell-Call. Persona/Threads/Lurker/Match
    folgen in späteren Rollout-Schritten und erweitern den System-Prompt."""

    def __init__(
        self,
        *,
        conversation: ConversationBuffer,
        rhythm: RhythmGuard,
        minimax: EngagementMinimaxClient,
    ) -> None:
        self._conversation = conversation
        self._rhythm = rhythm
        self._minimax = minimax

    async def handle(self, msg: IncomingMessage) -> HandleResult:
        try:
            from .background import ensure_started as _engagement_bg_ensure_started

            _engagement_bg_ensure_started()
        except Exception:
            log.exception("Engagement: background ensure_started fehlgeschlagen")
        result = await self._handle_inner(msg)
        # DISABLED ist Rauschen (jeder nicht-aktive Channel), nicht loggen.
        if result.decision is not Decision.DISABLED:
            log.info(
                "engagement decision=%s channel=%s user=%s tokens=%s/%s latency=%sms text=%r",
                result.decision.value,
                msg.channel_login,
                msg.twitch_login,
                result.prompt_tokens,
                result.completion_tokens,
                result.latency_ms,
                (result.response_text or "")[:120],
            )
            cost = _calc_cost_usd(result.prompt_tokens, result.completion_tokens)
            try:
                await asyncio.to_thread(
                    _sync_log_decision,
                    channel_login=msg.channel_login,
                    triggered_by_msg_id=msg.message_id,
                    decision=result.decision.value,
                    response_text=result.response_text,
                    referenced_thread_ids=result.referenced_thread_ids,
                    model=result.model,
                    prompt_tokens=result.prompt_tokens,
                    completion_tokens=result.completion_tokens,
                    cost_usd=cost,
                    latency_ms=result.latency_ms,
                )
            except Exception:
                log.exception("Engagement: log-Insert fehlgeschlagen")
        return result

    async def _handle_inner(self, msg: IncomingMessage) -> HandleResult:
        settings = await self._load_settings(msg.channel_login)
        if settings is None or not settings.enabled:
            return HandleResult(decision=Decision.DISABLED)

        # Nur wenn der Channel GERADE live ist UND Deadlock streamt — sonst Funkstille.
        from .stream_state import is_streaming_deadlock

        if not await is_streaming_deadlock(msg.channel_login):
            return HandleResult(decision=Decision.DISABLED)

        if await self._is_opted_out(msg.twitch_user_id):
            return HandleResult(decision=Decision.OPTOUT)

        self._rhythm.note_user_post(msg.channel_login)
        try:
            await self._conversation.append_user_turn(
                channel_login=msg.channel_login,
                twitch_user_id=msg.twitch_user_id,
                twitch_login=msg.twitch_login,
                content=msg.content,
                message_id=msg.message_id,
            )
        except Exception:
            log.exception("Engagement: append_user_turn fehlgeschlagen")
            return HandleResult(decision=Decision.PROVIDER_ERROR)

        now = datetime.now(timezone.utc)
        if not self._rhythm.anti_flood_ok(msg.channel_login, now=now):
            return HandleResult(decision=Decision.FLOOD_GUARD)
        if not self._rhythm.anti_burst_ok(msg.channel_login, now=now):
            return HandleResult(decision=Decision.ANTI_BURST)

        try:
            history_turns = await self._conversation.load_recent_buffer(
                channel_login=msg.channel_login, limit=100
            )
        except Exception:
            log.exception("Engagement: load_recent_buffer fehlgeschlagen")
            return HandleResult(decision=Decision.PROVIDER_ERROR)

        history = [
            ChatMessage(
                role=turn.role,
                content=turn.content,
                name=turn.twitch_login if turn.role == "user" else None,
            )
            for turn in history_turns
        ]

        system_prompt = build_baseline_system_prompt(streamer_login=msg.channel_login)
        try:
            from .soul_store import get_soul_extension_fragment

            soul_ext = await get_soul_extension_fragment()
            if soul_ext:
                system_prompt = f"{system_prompt}\n\n{soul_ext}"
        except Exception:
            log.exception("Engagement: soul-extension fehlgeschlagen")

        try:
            from .channel_background import get_channel_profile_fragment

            channel_profile = await get_channel_profile_fragment(msg.channel_login)
            if channel_profile:
                system_prompt = f"{system_prompt}\n\n{channel_profile}"
        except Exception:
            log.exception("Engagement: channel-background fehlgeschlagen")

        try:
            persona = await sample_tone(msg.channel_login)
            system_prompt = f"{system_prompt}\n\n{persona.to_prompt_fragment()}"
        except Exception:
            log.exception("Engagement: persona-sample fehlgeschlagen")

        try:
            style_fragment = await build_style_fragment(msg.channel_login)
            if style_fragment:
                system_prompt = f"{system_prompt}\n\n{style_fragment}"
        except Exception:
            log.exception("Engagement: style-examples fehlgeschlagen")

        threads: list = []
        try:
            threads = await load_open_threads_for_user(
                msg.twitch_user_id, msg.channel_login, limit=5
            )
            if threads:
                fragment = threads_to_prompt_fragment(msg.twitch_login, threads)
                if fragment:
                    system_prompt = f"{system_prompt}\n\n{fragment}"
        except Exception:
            log.exception("Engagement: thread-load fehlgeschlagen")

        try:
            lurkers = await known_regulars_currently_lurking(msg.channel_login, limit=5)
            if lurkers:
                lurker_fragment = lurker_hint_to_prompt_fragment(lurkers)
                if lurker_fragment:
                    system_prompt = f"{system_prompt}\n\n{lurker_fragment}"
        except Exception:
            log.exception("Engagement: lurker-signal fehlgeschlagen")

        try:
            match_state = await get_match_state(msg.channel_login)
            if match_state and match_state.is_live:
                match_fragment = match_state.to_prompt_fragment()
                if match_fragment:
                    system_prompt = f"{system_prompt}\n\n{match_fragment}"
        except Exception:
            log.exception("Engagement: match-context fehlgeschlagen")

        try:
            from .deadlock_wiki import build_grounding_fragment

            grounding_fragment = await build_grounding_fragment(msg.content)
            if grounding_fragment:
                system_prompt = f"{system_prompt}\n\n{grounding_fragment}"
        except Exception:
            log.exception("Engagement: deadlock-wiki-grounding fehlgeschlagen")

        try:
            transcript_segments = await load_recent_segments(msg.channel_login)
            transcript_fragment = segments_to_prompt_fragment(transcript_segments)
            if transcript_fragment:
                system_prompt = f"{system_prompt}\n\n{transcript_fragment}"
        except Exception:
            log.exception("Engagement: stream-transcript-context fehlgeschlagen")

        try:
            from .global_sentiment import get_sentiment_fragment

            sentiment_fragment = await get_sentiment_fragment()
            if sentiment_fragment:
                system_prompt = f"{system_prompt}\n\n{sentiment_fragment}"
        except Exception:
            log.exception("Engagement: global-sentiment-context fehlgeschlagen")

        try:
            from .deadlock_patches import build_patch_fragment, get_patch_digest_fragment

            patch_fragment = await build_patch_fragment(msg.content)
            if not patch_fragment:
                patch_fragment = await get_patch_digest_fragment(msg.content)
            if patch_fragment:
                system_prompt = f"{system_prompt}\n\n{patch_fragment}"
        except Exception:
            log.exception("Engagement: deadlock-patch-context fehlgeschlagen")

        try:
            from .deadlock_stats import build_stats_fragment

            stats_fragment = await build_stats_fragment(msg.content)
            if stats_fragment:
                system_prompt = f"{system_prompt}\n\n{stats_fragment}"
        except Exception:
            log.exception("Engagement: deadlock-stats-context fehlgeschlagen")

        if settings.persona_override:
            system_prompt = (
                f"{system_prompt}\n\nZusätzliche Persona-Hinweise: {settings.persona_override}"
            )
        if settings.tabu_topics:
            joined = ", ".join(t for t in settings.tabu_topics if t)
            if joined:
                system_prompt = f"{system_prompt}\n\nTabu-Themen (niemals ansprechen): {joined}"

        try:
            response = await self._minimax.generate(
                system_prompt=system_prompt,
                history=history,
                max_output_tokens=500,
            )
        except LLMProviderUnavailable:
            log.warning("Engagement: MiniMax-Provider nicht verfügbar")
            return HandleResult(decision=Decision.PROVIDER_ERROR)
        except Exception:
            log.exception("Engagement: MiniMax-Call fehlgeschlagen")
            return HandleResult(decision=Decision.PROVIDER_ERROR)

        if response.text is None:
            return HandleResult(
                decision=Decision.SILENT,
                model=response.model,
                prompt_tokens=response.prompt_tokens,
                completion_tokens=response.completion_tokens,
                latency_ms=response.latency_ms,
            )

        self._rhythm.note_bot_post(
            msg.channel_login, now=datetime.now(timezone.utc)
        )
        try:
            await self._conversation.append_assistant_turn(
                channel_login=msg.channel_login,
                content=response.text,
            )
        except Exception:
            log.exception("Engagement: append_assistant_turn fehlgeschlagen")

        referenced_ids = [t.id for t in threads] if threads else None
        if referenced_ids:
            try:
                await mark_referenced(referenced_ids)
            except Exception:
                log.exception("Engagement: mark_referenced fehlgeschlagen")

        return HandleResult(
            decision=Decision.SPOKE,
            response_text=response.text,
            model=response.model,
            prompt_tokens=response.prompt_tokens,
            completion_tokens=response.completion_tokens,
            latency_ms=response.latency_ms,
            referenced_thread_ids=referenced_ids,
        )

    async def _load_settings(self, channel_login: str) -> EngagementSettings | None:
        row = await asyncio.to_thread(_sync_load_settings, channel_login)
        if row is None:
            return None
        return EngagementSettings(
            channel_login=row[0],
            enabled=bool(row[1]),
            steam_id=row[2],
            persona_override=row[3],
            tabu_topics=list(row[4] or []),
        )

    async def _is_opted_out(self, twitch_user_id: str) -> bool:
        return await asyncio.to_thread(_sync_is_opted_out, twitch_user_id)
