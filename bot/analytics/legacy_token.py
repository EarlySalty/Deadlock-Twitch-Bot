"""
RaidAuthMixin – Hilfsmethoden für Auth-Status-Abfragen.
"""

from __future__ import annotations

from .. import storage as storage
from ..core.constants import log


class LegacyTokenAnalyticsMixin:
    """Stellt Hilfsmethoden für Auth-Status-Abfragen bereit."""

    async def _is_fully_authed(self, twitch_user_id: str) -> bool:
        """
        True = neuer Token vorhanden (needs_reauth=0) → voller Bot-Betrieb.
        False = Token fehlt oder ungültig → eingeschränkter Betrieb.
        """
        try:
            with storage.readonly_connection() as conn:
                row = conn.execute(
                    "SELECT needs_reauth FROM twitch_raid_auth WHERE twitch_user_id=%s",
                    (twitch_user_id,),
                ).fetchone()
            if not row:
                return False
            needs_reauth = row["needs_reauth"] if hasattr(row, "keys") else row[0]
            return needs_reauth == 0
        except Exception:
            log.debug(
                "RaidAuth: _is_fully_authed-Check fehlgeschlagen",
                exc_info=True,
            )
            return False

    async def _get_pending_reauth_count(self) -> int:
        """Anzahl Streamer die noch re-authen müssen (needs_reauth=1)."""
        try:
            with storage.readonly_connection() as conn:
                return conn.execute(
                    "SELECT COUNT(*) FROM twitch_raid_auth WHERE needs_reauth IS TRUE"
                ).fetchone()[0]
        except Exception:
            log.debug("RaidAuth: Konnte pending reauth count nicht lesen", exc_info=True)
            return 0
