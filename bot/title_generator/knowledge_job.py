# bot/title_generator/knowledge_job.py
from __future__ import annotations

import asyncio
import logging
import re
from collections import defaultdict
from datetime import datetime, timedelta, timezone
from typing import Any

from bot.storage import pg as storage
from bot.title_generator.title_db import (
    get_streamer_avg_viewers,
    get_streamer_session_count,
    upsert_knowledge_entry,
)

log = logging.getLogger(__name__)

EMOJI_RE = re.compile(
    "[\U00010000-\U0010ffff\U0001F300-\U0001F9FF\u2600-\u26FF\u2700-\u27BF]",  # lgtm[py/overly-large-range]
    flags=re.UNICODE,
)
SCORE_THRESHOLD = 1.2


def _classify_size(avg_viewers: float) -> str:
    if avg_viewers < 100:
        return "small"
    if avg_viewers < 500:
        return "medium"
    return "large"


def _extract_keywords(title: str) -> list[str]:
    words = re.findall(r"[a-zA-ZäöüÄÖÜß]{4,}", title.lower())
    stopwords = {"game", "stream", "live", "heute", "jetzt", "playing", "with", "ranked", "grind"}
    return [w for w in words if w not in stopwords][:8]


def _fetch_recent_sessions(days: int = 7) -> list[dict[str, Any]]:
    cutoff = datetime.now(timezone.utc) - timedelta(days=days)
    with storage.readonly_connection() as conn:
        rows = conn.execute(
            """
            SELECT
                streamer_login,
                stream_title,
                avg_viewers,
                peak_viewers,
                followers_start,
                started_at
            FROM twitch_stream_sessions
            WHERE started_at >= %s
              AND streamer_login IS NOT NULL AND streamer_login != ''
              AND stream_title IS NOT NULL AND stream_title != ''
              AND avg_viewers IS NOT NULL
              AND followers_start IS NOT NULL AND followers_start > 0
            """,
            (cutoff,),
        ).fetchall()
    return [
        {
            "streamer_login": str(r[0]).strip().lower(),
            "title": r[1],
            "avg_viewers": r[2],
            "peak_viewers": r[3],
            "followers_start": r[4],
            "started_at": r[5],
        }
        for r in rows
    ]


async def run_knowledge_job() -> None:
    log.info("title_generator: starting nightly knowledge job")
    sessions = await asyncio.get_event_loop().run_in_executor(None, _fetch_recent_sessions, 7)
    log.info("title_generator: processing %d sessions", len(sessions))

    by_streamer: dict[str, list[dict]] = defaultdict(list)
    for s in sessions:
        by_streamer[s["streamer_login"]].append(s)

    inserted = 0
    loop = asyncio.get_event_loop()
    for streamer_login, streamer_sessions in by_streamer.items():
        streamer_id = await loop.run_in_executor(None, _resolve_streamer_id_for_login, streamer_login)
        if not streamer_id:
            continue

        own_avg = await loop.run_in_executor(None, get_streamer_avg_viewers, streamer_id)
        session_count = await loop.run_in_executor(None, get_streamer_session_count, streamer_id)
        history_weight = min(session_count / 20, 1.0)

        for sess in streamer_sessions:
            if own_avg <= 0:
                continue
            relative_perf = sess["avg_viewers"] / own_avg
            engagement_rate = sess["avg_viewers"] / sess["followers_start"]
            normalized_score = (0.5 * relative_perf + 0.5 * engagement_rate * 100) * history_weight

            if normalized_score < SCORE_THRESHOLD:
                continue

            title = sess["title"].strip()
            if len(title) < 10 or len(title) > 140:
                continue

            keywords = _extract_keywords(title)
            streamer_size = _classify_size(own_avg)

            await loop.run_in_executor(
                None,
                lambda: upsert_knowledge_entry(
                    title=title,
                    keywords=keywords,
                    relative_perf=relative_perf,
                    engagement_rate=engagement_rate,
                    history_weight=history_weight,
                    normalized_score=normalized_score,
                    streamer_size=streamer_size,
                    source_streamer=streamer_id[:8] + "...",
                ),
            )
            inserted += 1

    log.info("title_generator: nightly job done — inserted/updated %d entries", inserted)


def _resolve_streamer_id_for_login(streamer_login: str) -> str:
    with storage.readonly_connection() as conn:
        row = conn.execute(
            """
            SELECT twitch_user_id
            FROM twitch_streamers
            WHERE LOWER(twitch_login) = %s
            LIMIT 1
            """,
            (str(streamer_login).strip().lower(),),
        ).fetchone()
    return str(row[0] or "").strip() if row and row[0] else ""


async def schedule_nightly_knowledge_job(start_delay_s: float = 0) -> None:
    """Long-running async task. Call with asyncio.create_task()."""
    if start_delay_s:
        await asyncio.sleep(start_delay_s)
    while True:
        try:
            await run_knowledge_job()
        except Exception:
            log.exception("title_generator: knowledge job failed")
        await asyncio.sleep(86400)
