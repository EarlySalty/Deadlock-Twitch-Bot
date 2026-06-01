"""Hintergrund-Jobs für den Engagement-Layer.

Vier asyncio-Loops, gestartet lazy beim ersten Pipeline-Aufruf:
1. Thread-Extractor (alle 15min, pro enabled Channel)
2. Match-Poller (alle 30s, pro enabled Channel mit steam_id)
3. Stream-Transkript-Worker (kurze OpenAI-STT-Chunks pro enabled Channel)
4. Auto-Closer für Threads (alle 1h)
5. Conversation-Trim (alle 24h, behält letzte 500 pro Channel)

Lifecycle: `ensure_started()` idempotent, Tasks leben mit dem Prozess.
"""

from __future__ import annotations

import asyncio
import logging
import os
import random
import threading
from datetime import UTC, datetime, timedelta

from bot.storage.pg import query_all, transaction

from .match_context import poll_match_state
from .minimax_chat import EngagementMinimaxClient
from .stream_transcripts import (
    StreamTranscriptSegment,
    append_segment,
    transcript_capture_seconds,
    transcript_poll_interval_seconds,
    transcript_quality,
    trim_segments,
)
from .threads import auto_close_stale, extract_threads

log = logging.getLogger("TwitchStreams.Engagement.Background")


_THREAD_EXTRACTOR_INTERVAL_SEC = 15 * 60
_MATCH_POLLER_INTERVAL_SEC = 30
_AUTO_CLOSER_INTERVAL_SEC = 60 * 60
_CONVERSATION_TRIM_INTERVAL_SEC = 24 * 60 * 60
_CONVERSATION_KEEP_PER_CHANNEL = 500
_TRANSCRIPT_TRIM_INTERVAL_SEC = 15 * 60
_GLOBAL_SENTIMENT_INTERVAL_SEC = 20 * 60

_started = False
_started_lock = threading.Lock()


def _sync_load_enabled_channels() -> list[tuple]:
    return query_all(
        "SELECT channel_login, steam_id FROM twitch_engagement_settings WHERE enabled = TRUE"
    )


def _sync_trim_conversation(keep_per_channel: int) -> int:
    with transaction() as conn:
        cur = conn.execute(
            f"""
            DELETE FROM twitch_engagement_conversation
            WHERE id IN (
                SELECT id FROM (
                    SELECT id,
                           ROW_NUMBER() OVER (
                               PARTITION BY channel_login ORDER BY ts DESC
                           ) AS rn
                    FROM twitch_engagement_conversation
                ) ranked
                WHERE rn > {int(keep_per_channel)}
            )
            """
        )
        return cur.rowcount or 0


async def _jittered_sleep(base_sec: float) -> None:
    jitter = base_sec * 0.1 * (random.random() * 2 - 1)
    await asyncio.sleep(max(1.0, base_sec + jitter))


async def _run_thread_extractor_loop() -> None:
    minimax = EngagementMinimaxClient()
    while True:
        try:
            channels = await asyncio.to_thread(_sync_load_enabled_channels)
            for channel_login, _steam in channels:
                try:
                    await extract_threads(channel_login, minimax=minimax)
                except Exception:
                    log.exception(
                        "Background: thread-extractor für %s fehlgeschlagen", channel_login
                    )
        except Exception:
            log.exception("Background: thread-extractor loop iteration fehlgeschlagen")
        await _jittered_sleep(_THREAD_EXTRACTOR_INTERVAL_SEC)


async def _run_match_poller_loop() -> None:
    while True:
        try:
            channels = await asyncio.to_thread(_sync_load_enabled_channels)
            for channel_login, steam_id in channels:
                if not steam_id:
                    continue
                try:
                    await poll_match_state(channel_login, str(steam_id))
                except Exception:
                    log.exception(
                        "Background: match-poller für %s fehlgeschlagen", channel_login
                    )
        except Exception:
            log.exception("Background: match-poller loop iteration fehlgeschlagen")
        await _jittered_sleep(_MATCH_POLLER_INTERVAL_SEC)


def _stream_transcripts_enabled() -> bool:
    value = str(os.getenv("ENGAGEMENT_STREAM_TRANSCRIPTS_ENABLED", "1")).strip().lower()
    return value not in {"", "0", "false", "no", "off"}


def _resolve_transcriber():
    engine_name = os.getenv("ENGAGEMENT_TRANSCRIBER") or "openai_api"
    from bot.social_media.transcription.whisper import get_transcriber

    return get_transcriber(engine_name)


async def _transcribe_capture(channel_login: str, transcriber) -> None:
    from bot.community.voice_reaction import audio_capture
    from bot.social_media.transcription.whisper import transcribe_clip

    capture_result = None
    try:
        capture_result = await audio_capture.capture(
            channel_login,
            duration_seconds=transcript_capture_seconds(),
            quality=transcript_quality(),
        )
        result = await transcribe_clip(capture_result.media_path, engine=transcriber)
        text = " ".join(str(getattr(result, "text", "") or "").split())
        if not text:
            return
        duration_seconds = (
            float(getattr(result, "duration_seconds", 0.0) or 0.0)
            or float(capture_result.actual_duration_seconds or 0.0)
            or float(capture_result.requested_duration_seconds or 0.0)
        )
        ended_at = datetime.now(UTC)
        started_at = ended_at - timedelta(seconds=max(1.0, duration_seconds))
        await append_segment(
            StreamTranscriptSegment(
                channel_login=channel_login,
                started_at=started_at,
                ended_at=ended_at,
                text=text,
                engine=str(getattr(result, "engine", "") or "openai_api"),
                model=str(getattr(result, "model", "") or "") or None,
            )
        )
    finally:
        if capture_result is not None:
            capture_result.cleanup()


async def _run_stream_transcript_loop() -> None:
    transcriber = None
    last_trim_at = 0.0
    while True:
        if not _stream_transcripts_enabled():
            await _jittered_sleep(transcript_poll_interval_seconds())
            continue
        try:
            if transcriber is None:
                transcriber = _resolve_transcriber()
            channels = await asyncio.to_thread(_sync_load_enabled_channels)
            for channel_login, _steam_id in channels:
                try:
                    await _transcribe_capture(str(channel_login), transcriber)
                except Exception:
                    log.debug(
                        "Background: stream-transcript für %s fehlgeschlagen",
                        channel_login,
                        exc_info=True,
                    )
            now = asyncio.get_running_loop().time()
            if now - last_trim_at >= _TRANSCRIPT_TRIM_INTERVAL_SEC:
                last_trim_at = now
                deleted = await trim_segments()
                if deleted:
                    log.info("Background: stream-transcript-trim deleted %d rows", deleted)
        except Exception:
            log.exception("Background: stream-transcript loop iteration fehlgeschlagen")
            transcriber = None
        await _jittered_sleep(transcript_poll_interval_seconds())


async def _run_auto_closer_loop() -> None:
    while True:
        try:
            counts = await auto_close_stale()
            if any(counts.values()):
                log.info("Background: auto-closer %s", counts)
        except Exception:
            log.exception("Background: auto-closer fehlgeschlagen")
        await _jittered_sleep(_AUTO_CLOSER_INTERVAL_SEC)


async def _run_conversation_trim_loop() -> None:
    while True:
        try:
            deleted = await asyncio.to_thread(
                _sync_trim_conversation, _CONVERSATION_KEEP_PER_CHANNEL
            )
            if deleted:
                log.info("Background: conversation-trim deleted %d rows", deleted)
        except Exception:
            log.exception("Background: conversation-trim fehlgeschlagen")
        await _jittered_sleep(_CONVERSATION_TRIM_INTERVAL_SEC)


async def _run_global_sentiment_loop() -> None:
    minimax = EngagementMinimaxClient(timeout=180.0)
    while True:
        try:
            from .global_sentiment import rebuild_global_sentiment

            await rebuild_global_sentiment(minimax=minimax)
        except Exception:
            log.exception("Background: global-sentiment loop iteration fehlgeschlagen")
        await _jittered_sleep(_GLOBAL_SENTIMENT_INTERVAL_SEC)


def ensure_started() -> None:
    """Idempotent — startet die Background-Loops einmal pro Prozess.

    Muss aus einem laufenden asyncio-Event-Loop heraus aufgerufen werden;
    aus sync-Kontext ohne laufenden Loop ist es ein No-Op.
    """
    global _started
    if _started:
        return
    with _started_lock:
        if _started:
            return
        try:
            loop = asyncio.get_running_loop()
        except RuntimeError:
            log.debug("Background: kein running loop, skip ensure_started")
            return
        _started = True
        loop.create_task(_run_thread_extractor_loop(), name="engagement-thread-extractor")
        loop.create_task(_run_match_poller_loop(), name="engagement-match-poller")
        loop.create_task(_run_stream_transcript_loop(), name="engagement-stream-transcripts")
        loop.create_task(_run_auto_closer_loop(), name="engagement-auto-closer")
        loop.create_task(_run_conversation_trim_loop(), name="engagement-conv-trim")
        loop.create_task(_run_global_sentiment_loop(), name="engagement-global-sentiment")
        log.info(
            "Engagement-Background-Jobs gestartet "
            "(thread-extractor=15min, match-poller=30s, stream-transcripts=on, "
            "auto-closer=1h, conv-trim=24h, global-sentiment=20min)"
        )
