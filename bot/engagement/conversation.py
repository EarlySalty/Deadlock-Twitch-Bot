"""Rolling Multi-Turn-Konversations-Buffer pro Channel.

Hält die letzten ~100 Turns als `[{role, name, content, ts}, …]` in der Tabelle
`twitch_engagement_conversation`. User- und Bot-Turns werden so abwechselnd
persistiert, wie es OpenAI-/MiniMax-kompatible Chat-Completion-APIs erwarten.

Storage-Pfad: `bot.storage.pg.transaction` / `query_all` (synchron, pooled).
Async-Kontext: jede Methode wraped sync-Calls in `asyncio.to_thread`.
"""

from __future__ import annotations

import asyncio
from dataclasses import dataclass
from datetime import datetime

from bot.storage.pg import query_all, transaction


@dataclass(slots=True)
class ConversationTurn:
    role: str  # 'user' | 'assistant' | 'system'
    twitch_user_id: str | None
    twitch_login: str | None
    content: str
    message_id: str | None
    ts: datetime


def _sync_append_user(
    channel_login: str,
    twitch_user_id: str,
    twitch_login: str,
    content: str,
    message_id: str | None,
) -> None:
    with transaction() as conn:
        conn.execute(
            """
            INSERT INTO twitch_engagement_conversation
                (channel_login, role, twitch_user_id, twitch_login, content, message_id)
            VALUES (%s, 'user', %s, %s, %s, %s)
            """,
            [channel_login, twitch_user_id, twitch_login, content, message_id],
        )


def _sync_append_assistant(channel_login: str, content: str) -> None:
    with transaction() as conn:
        conn.execute(
            """
            INSERT INTO twitch_engagement_conversation
                (channel_login, role, content)
            VALUES (%s, 'assistant', %s)
            """,
            [channel_login, content],
        )


def _sync_load_recent(channel_login: str, limit: int) -> list[tuple]:
    return query_all(
        """
        SELECT role, twitch_user_id, twitch_login, content, message_id, ts
        FROM twitch_engagement_conversation
        WHERE channel_login = %s
        ORDER BY ts DESC
        LIMIT %s
        """,
        [channel_login, limit],
    )


class ConversationBuffer:
    """Persistenter Multi-Turn-Buffer pro Channel."""

    async def append_user_turn(
        self,
        *,
        channel_login: str,
        twitch_user_id: str,
        twitch_login: str,
        content: str,
        message_id: str | None,
    ) -> None:
        await asyncio.to_thread(
            _sync_append_user,
            channel_login,
            twitch_user_id,
            twitch_login,
            content,
            message_id,
        )

    async def append_assistant_turn(
        self,
        *,
        channel_login: str,
        content: str,
    ) -> None:
        await asyncio.to_thread(_sync_append_assistant, channel_login, content)

    async def load_recent_buffer(
        self,
        *,
        channel_login: str,
        limit: int = 100,
    ) -> list[ConversationTurn]:
        rows = await asyncio.to_thread(_sync_load_recent, channel_login, limit)
        turns: list[ConversationTurn] = []
        for row in reversed(rows):
            turns.append(
                ConversationTurn(
                    role=row[0],
                    twitch_user_id=row[1],
                    twitch_login=row[2],
                    content=row[3],
                    message_id=row[4],
                    ts=row[5],
                )
            )
        return turns
