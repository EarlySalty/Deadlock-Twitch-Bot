"""Kurzer Stream-Audio-Transkript-Kontext für den Engagement-Layer."""

from __future__ import annotations

import asyncio
import logging
import os
from dataclasses import dataclass
from datetime import UTC, datetime, timedelta

from bot.storage.pg import query_all, transaction

log = logging.getLogger("TwitchStreams.Engagement.StreamTranscripts")

_table_ready = False


def _env_int(name: str, default: int, *, minimum: int) -> int:
    raw = str(os.getenv(name) or "").strip()
    if not raw:
        return default
    try:
        return max(minimum, int(raw))
    except ValueError:
        log.warning("Invalid %s=%r; using default %s", name, raw, default)
        return default


def _env_float(name: str, default: float, *, minimum: float) -> float:
    raw = str(os.getenv(name) or "").strip()
    if not raw:
        return default
    try:
        return max(minimum, float(raw))
    except ValueError:
        log.warning("Invalid %s=%r; using default %s", name, raw, default)
        return default


@dataclass(frozen=True, slots=True)
class StreamTranscriptSegment:
    channel_login: str
    started_at: datetime
    ended_at: datetime
    text: str
    engine: str
    model: str | None = None


def _ensure_table(conn) -> None:
    global _table_ready
    if _table_ready:
        return
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS twitch_engagement_stream_transcripts (
            id BIGSERIAL PRIMARY KEY,
            channel_login TEXT NOT NULL,
            started_at TIMESTAMPTZ NOT NULL,
            ended_at TIMESTAMPTZ NOT NULL,
            text TEXT NOT NULL,
            engine TEXT NOT NULL,
            model TEXT,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )
        """
    )
    conn.execute(
        """
        CREATE INDEX IF NOT EXISTS idx_eng_stream_transcripts_channel_ended
        ON twitch_engagement_stream_transcripts (channel_login, ended_at DESC)
        """
    )
    _table_ready = True


def _sync_append_segment(segment: StreamTranscriptSegment) -> None:
    text = " ".join(str(segment.text or "").split())
    if not text:
        return
    with transaction() as conn:
        _ensure_table(conn)
        conn.execute(
            """
            INSERT INTO twitch_engagement_stream_transcripts
                (channel_login, started_at, ended_at, text, engine, model)
            VALUES (%s, %s, %s, %s, %s, %s)
            """,
            [
                segment.channel_login,
                segment.started_at,
                segment.ended_at,
                text,
                segment.engine,
                segment.model,
            ],
        )


def _sync_load_recent(
    channel_login: str,
    *,
    max_age_minutes: int,
    limit: int,
) -> list[tuple]:
    cutoff = datetime.now(UTC) - timedelta(minutes=max_age_minutes)
    with transaction() as conn:
        _ensure_table(conn)
    return query_all(
        """
        SELECT channel_login, started_at, ended_at, text, engine, model
        FROM twitch_engagement_stream_transcripts
        WHERE channel_login = %s
          AND ended_at >= %s
        ORDER BY ended_at DESC
        LIMIT %s
        """,
        [channel_login, cutoff, limit],
    )


def _sync_trim(
    *,
    max_age_minutes: int,
    keep_per_channel: int,
) -> int:
    cutoff = datetime.now(UTC) - timedelta(minutes=max_age_minutes)
    with transaction() as conn:
        _ensure_table(conn)
        cur = conn.execute(
            """
            DELETE FROM twitch_engagement_stream_transcripts
            WHERE created_at < %s
               OR id IN (
                   SELECT id FROM (
                       SELECT id,
                              ROW_NUMBER() OVER (
                                  PARTITION BY channel_login ORDER BY ended_at DESC
                              ) AS rn
                       FROM twitch_engagement_stream_transcripts
                   ) ranked
                   WHERE rn > %s
               )
            """,
            [cutoff, keep_per_channel],
        )
        return cur.rowcount or 0


async def append_segment(segment: StreamTranscriptSegment) -> None:
    await asyncio.to_thread(_sync_append_segment, segment)


async def load_recent_segments(
    channel_login: str,
    *,
    max_age_minutes: int | None = None,
    limit: int | None = None,
) -> list[StreamTranscriptSegment]:
    rows = await asyncio.to_thread(
        _sync_load_recent,
        channel_login,
        max_age_minutes=max_age_minutes
        if max_age_minutes is not None
        else _env_int("ENGAGEMENT_TRANSCRIPT_CONTEXT_MINUTES", 15, minimum=1),
        limit=limit
        if limit is not None
        else _env_int("ENGAGEMENT_TRANSCRIPT_CONTEXT_LIMIT", 8, minimum=1),
    )
    segments = [
        StreamTranscriptSegment(
            channel_login=str(row[0]),
            started_at=row[1],
            ended_at=row[2],
            text=str(row[3] or ""),
            engine=str(row[4] or ""),
            model=str(row[5]) if row[5] else None,
        )
        for row in reversed(rows)
    ]
    return [segment for segment in segments if segment.text.strip()]


async def trim_segments(
    *,
    max_age_minutes: int | None = None,
    keep_per_channel: int | None = None,
) -> int:
    return await asyncio.to_thread(
        _sync_trim,
        max_age_minutes=max_age_minutes
        if max_age_minutes is not None
        else _env_int("ENGAGEMENT_TRANSCRIPT_RETENTION_MINUTES", 60, minimum=1),
        keep_per_channel=keep_per_channel
        if keep_per_channel is not None
        else _env_int("ENGAGEMENT_TRANSCRIPT_KEEP_PER_CHANNEL", 40, minimum=1),
    )


def segments_to_prompt_fragment(
    segments: list[StreamTranscriptSegment],
    *,
    max_chars: int | None = None,
) -> str:
    if not segments:
        return ""
    budget = max_chars if max_chars is not None else _env_int(
        "ENGAGEMENT_TRANSCRIPT_PROMPT_MAX_CHARS",
        1200,
        minimum=200,
    )
    parts: list[str] = []
    for segment in segments:
        text = " ".join(segment.text.split())
        if not text:
            continue
        ts = segment.ended_at.astimezone(UTC).strftime("%H:%M:%S")
        parts.append(f"- {ts}: {text}")
    joined = "\n".join(parts)
    if len(joined) > budget:
        joined = joined[-budget:].lstrip()
        first_break = joined.find("\n")
        if first_break != -1:
            joined = joined[first_break + 1 :]
    if not joined:
        return ""
    return (
        "Aktueller Stream-Audio-Kontext aus Voice-to-Text. "
        "Nutze ihn nur, wenn er zur Chat-Nachricht passt; er kann unvollständig sein.\n"
        f"{joined}"
    )


def transcript_capture_seconds() -> int:
    return _env_int("ENGAGEMENT_TRANSCRIPT_CAPTURE_SECONDS", 45, minimum=10)


def transcript_poll_interval_seconds() -> float:
    return _env_float("ENGAGEMENT_TRANSCRIPT_INTERVAL_SECONDS", 75.0, minimum=15.0)


def transcript_quality() -> str:
    return os.getenv("ENGAGEMENT_TRANSCRIPT_QUALITY") or "audio_only"
