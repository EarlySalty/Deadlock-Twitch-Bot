"""Permission-Layer für Engagement-Toggle.

`super_mod` aus `twitch_admin_roles` darf in jedem Channel toggeln — auch
ohne Twitch-Mod-Status. Sonst gilt: Channel-Owner oder Twitch-Mod.
"""

from __future__ import annotations

import asyncio

from bot.storage.pg import query_one


def _sync_is_super_mod(twitch_user_id: str) -> bool:
    row = query_one(
        """
        SELECT 1 FROM twitch_admin_roles
        WHERE twitch_user_id = %s AND role = 'super_mod'
        """,
        [twitch_user_id],
    )
    return row is not None


async def is_super_mod(twitch_user_id: str | None) -> bool:
    """True wenn user in twitch_admin_roles mit role='super_mod'."""
    if not twitch_user_id:
        return False
    return await asyncio.to_thread(_sync_is_super_mod, str(twitch_user_id))


async def can_toggle_channel(
    *,
    actor_user_id: str | None,
    is_broadcaster: bool,
    is_moderator: bool,
) -> bool:
    """True wenn actor Broadcaster, Twitch-Mod im Channel ODER super_mod."""
    if is_broadcaster or is_moderator:
        return True
    return await is_super_mod(actor_user_id)
