"""Hintergrund-Jobs für den Engagement-Layer.

Vier asyncio-Loops, gestartet lazy beim ersten Pipeline-Aufruf:
1. Thread-Extractor (alle 15min, pro enabled Channel)
2. Match-Poller (alle 30s, pro enabled Channel mit steam_id)
3. Auto-Closer für Threads (alle 1h)
4. Conversation-Trim (alle 24h, behält letzte 500 pro Channel)

Lifecycle: `ensure_started()` idempotent, Tasks leben mit dem Prozess.
"""

from __future__ import annotations

import asyncio
import logging
import random
import threading

from bot.storage.pg import query_all, transaction

from .match_context import poll_match_state
from .minimax_chat import EngagementMinimaxClient
from .threads import auto_close_stale, extract_threads

log = logging.getLogger("TwitchStreams.Engagement.Background")


_THREAD_EXTRACTOR_INTERVAL_SEC = 15 * 60
_MATCH_POLLER_INTERVAL_SEC = 30
_AUTO_CLOSER_INTERVAL_SEC = 60 * 60
_CONVERSATION_TRIM_INTERVAL_SEC = 24 * 60 * 60
_CONVERSATION_KEEP_PER_CHANNEL = 500

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


def ensure_started() -> None:
    """Idempotent — startet die vier Background-Loops einmal pro Prozess.

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
        loop.create_task(_run_auto_closer_loop(), name="engagement-auto-closer")
        loop.create_task(_run_conversation_trim_loop(), name="engagement-conv-trim")
        log.info(
            "Engagement-Background-Jobs gestartet "
            "(thread-extractor=15min, match-poller=30s, auto-closer=1h, conv-trim=24h)"
        )
