"""Promo cooldown persistence helpers.

This module keeps the promo-specific storage logic out of the large
``pg.py`` module while preserving the public API through re-exports.
"""

from __future__ import annotations

import time


def save_promo_cooldown(login: str, cooldown_type: str, wall_ts: float) -> None:
    """Persist a promo cooldown timestamp. Non-fatal on failure."""
    try:
        from .pg import transaction

        with transaction() as conn:
            conn.execute(
                """INSERT INTO twitch_promo_cooldowns (login, cooldown_type, wall_ts, updated_at)
                   VALUES (%s, %s, %s, now())
                   ON CONFLICT (login, cooldown_type)
                   DO UPDATE SET wall_ts = EXCLUDED.wall_ts, updated_at = now()""",
                (login.lower(), cooldown_type, wall_ts),
            )
    except Exception:
        from ..core.constants import log

        log.debug(
            "Failed to persist promo cooldown for %s/%s",
            login,
            cooldown_type,
            exc_info=True,
        )


def load_promo_cooldowns() -> list[tuple[str, str, float]]:
    """Load all promo cooldowns. Returns list of (login, cooldown_type, wall_ts)."""
    try:
        from .pg import readonly_connection

        with readonly_connection() as conn:
            rows = conn.execute(
                "SELECT login, cooldown_type, wall_ts FROM twitch_promo_cooldowns"
            ).fetchall()
            return [
                (str(r["login"]), str(r["cooldown_type"]), float(r["wall_ts"]))
                for r in (rows or [])
            ]
    except Exception:
        from ..core.constants import log

        log.debug("Failed to load promo cooldowns", exc_info=True)
        return []


def cleanup_stale_promo_cooldowns(max_age_hours: int = 24) -> None:
    """Delete cooldown entries older than max_age_hours."""
    try:
        from .pg import transaction

        with transaction() as conn:
            conn.execute(
                "DELETE FROM twitch_promo_cooldowns WHERE wall_ts < %s",
                (time.time() - max_age_hours * 3600,),
            )
    except Exception:
        from ..core.constants import log

        log.debug("Failed to clean up stale promo cooldowns", exc_info=True)


__all__ = [
    "cleanup_stale_promo_cooldowns",
    "load_promo_cooldowns",
    "save_promo_cooldown",
]
