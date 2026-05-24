"""Auto-Disable des Engagement-Layers wenn ein Channel offline geht.

Wird vom Stream-Offline-Handler (`_on_eventsub_stream_offline`) best-effort
aufgerufen. Idempotent: `WHERE enabled = TRUE` verhindert No-Op-Updates und
liefert via rowcount, ob tatsächlich umgeschaltet wurde.
"""

from __future__ import annotations

import asyncio

from bot.storage.pg import transaction


def _sync_disable(channel_login: str) -> int:
    with transaction() as conn:
        cur = conn.execute(
            """
            UPDATE twitch_engagement_settings
               SET enabled = FALSE,
                   updated_at = NOW()
             WHERE channel_login = %s
               AND enabled = TRUE
            """,
            [channel_login],
        )
        return cur.rowcount or 0


async def auto_disable_on_offline(channel_login: str) -> int:
    """Setzt `enabled = FALSE` für den Channel, falls aktuell aktiv.

    Returns:
        rowcount — 0 wenn Channel nicht (mehr) aktiv war, sonst 1.
    """
    if not channel_login:
        return 0
    return await asyncio.to_thread(_sync_disable, channel_login)
