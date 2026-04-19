# bot/title_generator/title_db.py
from __future__ import annotations

import json
from typing import Any

from bot.storage import pg as storage


def get_streamer_title_history(streamer_id: str, limit: int = 30) -> list[dict[str, Any]]:
    """Return recent stream sessions with title + viewer stats for the given streamer."""
    with storage.readonly_connection() as conn:
        rows = conn.execute(
            """
            SELECT
                s.title,
                s.avg_viewers,
                s.peak_viewers,
                s.followers_start,
                s.started_at
            FROM twitch_stream_sessions s
            WHERE s.twitch_user_id = %s
              AND s.title IS NOT NULL
              AND s.title != ''
            ORDER BY s.started_at DESC
            LIMIT %s
            """,
            (streamer_id, limit),
        ).fetchall()
    return [
        {
            "title": r[0],
            "avg_viewers": r[1],
            "peak_viewers": r[2],
            "followers_start": r[3],
            "started_at": r[4],
        }
        for r in rows
    ]


def get_streamer_avg_viewers(streamer_id: str) -> float:
    """Return the streamer's average viewer count over all sessions."""
    with storage.readonly_connection() as conn:
        row = conn.execute(
            """
            SELECT AVG(avg_viewers)::float
            FROM twitch_stream_sessions
            WHERE twitch_user_id = %s AND avg_viewers IS NOT NULL
            """,
            (streamer_id,),
        ).fetchone()
    return float(row[0]) if row and row[0] is not None else 0.0


def get_streamer_session_count(streamer_id: str) -> int:
    """Return total number of recorded sessions for the given streamer."""
    with storage.readonly_connection() as conn:
        row = conn.execute(
            "SELECT COUNT(*) FROM twitch_stream_sessions WHERE twitch_user_id = %s",
            (streamer_id,),
        ).fetchone()
    return int(row[0]) if row else 0


def get_top_knowledge_titles(limit: int = 30) -> list[dict[str, Any]]:
    """Return top curated titles from the knowledge table, sorted by normalized_score."""
    with storage.readonly_connection() as conn:
        rows = conn.execute(
            """
            SELECT title, normalized_score, keywords, quality_tier
            FROM title_generator_knowledge
            WHERE game_context = 'deadlock'
            ORDER BY normalized_score DESC
            LIMIT %s
            """,
            (limit,),
        ).fetchall()
    return [
        {
            "title": r[0],
            "normalized_score": r[1],
            "keywords": list(r[2]) if r[2] else [],
            "quality_tier": r[3],
        }
        for r in rows
    ]


def upsert_knowledge_entry(
    title: str,
    keywords: list[str],
    relative_perf: float,
    engagement_rate: float,
    history_weight: float,
    normalized_score: float,
    streamer_size: str,
    source_streamer: str,
) -> None:
    """Insert or update a knowledge entry. Keeps highest score on conflict."""
    with storage.transaction() as conn:
        conn.execute(
            """
            INSERT INTO title_generator_knowledge
                (title, keywords, relative_perf, engagement_rate, history_weight,
                 normalized_score, streamer_size, source_streamer)
            VALUES (%s, %s, %s, %s, %s, %s, %s, %s)
            ON CONFLICT (title, game_context)
            DO UPDATE SET
                normalized_score = GREATEST(title_generator_knowledge.normalized_score, EXCLUDED.normalized_score),
                quality_tier = CASE WHEN EXCLUDED.normalized_score > 2.0 THEN 3
                                    WHEN EXCLUDED.normalized_score > 1.5 THEN 2
                                    ELSE 1 END
            """,
            (title, keywords, relative_perf, engagement_rate, history_weight,
             normalized_score, streamer_size, source_streamer),
        )


def insert_insight(
    streamer_id: str,
    period_start: Any,
    period_end: Any,
    strengths: str,
    weaknesses: str,
    patterns: str,
    recommendations: str,
    raw_response: dict,
) -> None:
    """Persist a weekly insight record."""
    with storage.transaction() as conn:
        conn.execute(
            """
            INSERT INTO title_generator_insights
                (streamer_id, period_start, period_end, strengths, weaknesses,
                 patterns, recommendations, raw_response)
            VALUES (%s, %s, %s, %s, %s, %s, %s, %s)
            """,
            (streamer_id, period_start, period_end, strengths, weaknesses,
             patterns, recommendations, json.dumps(raw_response)),
        )


def get_latest_insights(streamer_id: str) -> dict[str, Any] | None:
    """Return the most recent insight record for a streamer."""
    with storage.readonly_connection() as conn:
        row = conn.execute(
            """
            SELECT strengths, weaknesses, patterns, recommendations, generated_at
            FROM title_generator_insights
            WHERE streamer_id = %s
            ORDER BY generated_at DESC
            LIMIT 1
            """,
            (streamer_id,),
        ).fetchone()
    if not row:
        return None
    return {
        "strengths": row[0],
        "weaknesses": row[1],
        "patterns": row[2],
        "recommendations": row[3],
        "generated_at": row[4].isoformat() if row[4] else None,
    }
