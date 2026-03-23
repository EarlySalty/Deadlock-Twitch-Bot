"""Legacy shim: old code imported twitch_cog.storage_pg.

We now forward everything to the new implementation in bot.storage.pg, which
provides the PostgreSQL-backed storage surface. This keeps existing imports
working without preserving any legacy compatibility shims.
"""

from __future__ import annotations

# Re-export the full surface
from bot.storage.pg import *  # noqa: F401,F403
