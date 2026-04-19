# bot/title_generator/insight_job.py
from __future__ import annotations

import asyncio
import logging
from datetime import datetime, timedelta, timezone
from typing import Any

from bot.storage import pg as storage
from bot.title_generator.title_ai import generate_insight
from bot.title_generator.title_db import (
    get_streamer_avg_viewers,
    insert_insight,
)

log = logging.getLogger(__name__)


def _fetch_active_partner_ids() -> list[str]:
    with storage.readonly_connection() as conn:
        rows = conn.execute(
            """
            SELECT twitch_user_id FROM twitch_partners
            WHERE archived_at IS NULL
            """,
        ).fetchall()
    return [r[0] for r in rows]


def _fetch_history_for_period(
    streamer_id: str, start: datetime, end: datetime
) -> list[dict[str, Any]]:
    with storage.readonly_connection() as conn:
        rows = conn.execute(
            """
            SELECT title, avg_viewers, followers_start
            FROM twitch_stream_sessions
            WHERE twitch_user_id = %s
              AND started_at BETWEEN %s AND %s
              AND title IS NOT NULL AND title != ''
              AND avg_viewers IS NOT NULL
            ORDER BY started_at DESC
            """,
            (streamer_id, start, end),
        ).fetchall()
    return [
        {"title": r[0], "avg_viewers": r[1], "followers_start": r[2]}
        for r in rows
    ]


def _enrich_with_scores(sessions: list[dict], own_avg: float) -> list[dict]:
    enriched = []
    for s in sessions:
        if own_avg <= 0 or not s.get("followers_start"):
            continue
        s["relative_perf"] = s["avg_viewers"] / own_avg
        s["engagement_rate"] = s["avg_viewers"] / s["followers_start"]
        enriched.append(s)
    return enriched


async def run_insight_job() -> None:
    log.info("title_generator: starting weekly insight job")
    now = datetime.now(timezone.utc)
    period_start = now - timedelta(days=28)

    loop = asyncio.get_event_loop()
    partner_ids = await loop.run_in_executor(None, _fetch_active_partner_ids)
    log.info("title_generator: generating insights for %d partners", len(partner_ids))

    for streamer_id in partner_ids:
        try:
            sessions = await loop.run_in_executor(
                None, _fetch_history_for_period, streamer_id, period_start, now
            )
            if len(sessions) < 3:
                continue

            own_avg = await loop.run_in_executor(
                None, get_streamer_avg_viewers, streamer_id
            )
            enriched = _enrich_with_scores(sessions, own_avg)
            if not enriched:
                continue

            period_label = (
                f"{period_start.strftime('%d.%m.')} – {now.strftime('%d.%m.%Y')}"
            )
            result = await generate_insight(enriched, period_label)
            if not result:
                continue

            await loop.run_in_executor(
                None,
                lambda: insert_insight(
                    streamer_id=streamer_id,
                    period_start=period_start,
                    period_end=now,
                    strengths=result.get("strengths", ""),
                    weaknesses=result.get("weaknesses", ""),
                    patterns=result.get("patterns", ""),
                    recommendations=result.get("recommendations", ""),
                    raw_response=result.get("raw", {}),
                ),
            )
            log.info("title_generator: insight saved for %s", streamer_id)
            await asyncio.sleep(2)

        except Exception:
            log.exception("title_generator: insight job failed for %s", streamer_id)

    log.info("title_generator: weekly insight job done")


async def schedule_weekly_insight_job(start_delay_s: float = 0) -> None:
    """Long-running async task. Call with asyncio.create_task()."""
    if start_delay_s:
        await asyncio.sleep(start_delay_s)
    while True:
        try:
            await run_insight_job()
        except Exception:
            log.exception("title_generator: weekly insight loop failed")
        await asyncio.sleep(7 * 86400)
