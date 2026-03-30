"""Synchronous loader for chat social graph analytics."""

from __future__ import annotations

import re
from datetime import UTC, datetime, timedelta
from typing import Any

from ..core.chat_bots import build_known_chat_bot_not_in_clause
from ..storage import pg as storage
from .raw_chat_status import build_raw_chat_status

_MENTION_RE = re.compile(r"(?<!\w)@([A-Za-z0-9_]{3,25})\b")


def load_chat_social_graph_payload(*, streamer: str, days: int) -> dict[str, Any]:
    cutoff = (datetime.now(UTC) - timedelta(days=days)).isoformat()
    with storage.readonly_connection() as conn:
        bot_clause, bot_params = build_known_chat_bot_not_in_clause(
            column_expr="m.chatter_login",
            placeholder="%s",
        )

        rows = conn.execute(
            f"""
            SELECT m.chatter_login, m.content
            FROM twitch_chat_messages m
            JOIN twitch_stream_sessions s ON s.id = m.session_id
            WHERE LOWER(s.streamer_login) = %s
              AND m.message_ts >= %s
              AND m.content LIKE '%@%'
              AND {bot_clause}
            """,
            (streamer, cutoff, *bot_params),
        ).fetchall()

        mention_sent: dict[str, int] = {}
        mention_received: dict[str, int] = {}
        pair_counts: dict[tuple[str, str], int] = {}
        total_mentions = 0
        mentioners: set[str] = set()
        mentioned: set[str] = set()

        for row in rows:
            sender = (row[0] or "").lower()
            content = row[1] or ""
            targets = _MENTION_RE.findall(content)

            for target in targets:
                target_lower = target.lower()
                if target_lower == sender:
                    continue
                total_mentions += 1
                mentioners.add(sender)
                mentioned.add(target_lower)
                mention_sent[sender] = mention_sent.get(sender, 0) + 1
                mention_received[target_lower] = mention_received.get(target_lower, 0) + 1
                pair_key = (sender, target_lower)
                pair_counts[pair_key] = pair_counts.get(pair_key, 0) + 1

        hub_list = []
        all_users = set(mention_sent.keys()) | set(mention_received.keys())
        for user in all_users:
            sent = mention_sent.get(user, 0)
            received = mention_received.get(user, 0)
            hub_list.append(
                {
                    "login": user,
                    "mentionsSent": sent,
                    "mentionsReceived": received,
                    "score": sent + received,
                }
            )
        hub_list.sort(key=lambda hub: hub["score"], reverse=True)

        top_pairs = sorted(
            [
                {"from": pair[0], "to": pair[1], "count": count}
                for pair, count in pair_counts.items()
            ],
            key=lambda pair: pair["count"],
            reverse=True,
        )[:20]

        recv_counts = list(mention_received.values())
        raw_chat_status = build_raw_chat_status(
            conn,
            streamer,
            since_date=cutoff,
        )

    return {
        "totalMentions": total_mentions,
        "uniqueMentioners": len(mentioners),
        "uniqueMentioned": len(mentioned),
        "hubs": hub_list[:20],
        "topPairs": top_pairs,
        "mentionDistribution": {
            "mentionedOnce": sum(1 for count in recv_counts if count == 1),
            "mentioned2to5": sum(1 for count in recv_counts if 2 <= count <= 5),
            "mentioned5plus": sum(1 for count in recv_counts if count > 5),
        },
        "rawChatStatus": raw_chat_status,
    }
