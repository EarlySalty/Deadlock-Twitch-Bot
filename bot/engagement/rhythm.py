"""Konversations-Rhythmik: Anti-Flood + Anti-Burst.

Drei einfache Gates statt Cooldown-Slider:
- Anti-Flood: letzter Bot-Post < min_pause_sec (default 5s) → kein Call
- Anti-Burst: >= burst_limit (default 3) Bot-Posts in burst_window_sec (default
  60s) ohne dazwischenliegende User-Reaktion → kein Call bis nächste User-Message
- Sonst aktive Reaktivität (Antworten typischerweise in 10-20s)

In-Memory-State pro Channel; nach Bot-Restart wird mit leerem State begonnen,
was tolerierbar ist (max 1 Burst-Window doppelt). threading.Lock weil
Background-Jobs ebenfalls Posts notieren könnten.
"""

from __future__ import annotations

import os
import threading
from dataclasses import dataclass, field
from datetime import datetime, timedelta


def _env_float(name: str, default: float) -> float:
    raw = os.getenv(name)
    if not raw:
        return default
    try:
        return float(raw)
    except ValueError:
        return default


def _env_int(name: str, default: int) -> int:
    raw = os.getenv(name)
    if not raw:
        return default
    try:
        return int(raw)
    except ValueError:
        return default


@dataclass(slots=True)
class ChannelRhythmState:
    bot_post_times: list[datetime] = field(default_factory=list)
    user_post_since_last_bot: bool = False


class RhythmGuard:
    """Anti-Flood + Anti-Burst-Logik."""

    def __init__(
        self,
        *,
        min_pause_sec: float | None = None,
        burst_limit: int | None = None,
        burst_window_sec: float | None = None,
    ) -> None:
        self._min_pause_sec = (
            min_pause_sec
            if min_pause_sec is not None
            else _env_float("ENGAGEMENT_MIN_PAUSE_SEC", 5.0)
        )
        self._burst_limit = (
            burst_limit
            if burst_limit is not None
            else _env_int("ENGAGEMENT_BURST_LIMIT", 3)
        )
        self._burst_window_sec = (
            burst_window_sec
            if burst_window_sec is not None
            else _env_float("ENGAGEMENT_BURST_WINDOW_SEC", 60.0)
        )
        self._state: dict[str, ChannelRhythmState] = {}
        self._lock = threading.Lock()

    def _get_state(self, channel_login: str) -> ChannelRhythmState:
        state = self._state.get(channel_login)
        if state is None:
            state = ChannelRhythmState()
            self._state[channel_login] = state
        return state

    def anti_flood_ok(self, channel_login: str, *, now: datetime) -> bool:
        with self._lock:
            state = self._get_state(channel_login)
            if not state.bot_post_times:
                return True
            elapsed = (now - state.bot_post_times[-1]).total_seconds()
            return elapsed >= self._min_pause_sec

    def anti_burst_ok(self, channel_login: str, *, now: datetime) -> bool:
        with self._lock:
            state = self._get_state(channel_login)
            if state.user_post_since_last_bot:
                return True
            window_start = now - timedelta(seconds=self._burst_window_sec)
            recent_count = sum(1 for t in state.bot_post_times if t >= window_start)
            return recent_count < self._burst_limit

    def note_bot_post(self, channel_login: str, *, now: datetime) -> None:
        with self._lock:
            state = self._get_state(channel_login)
            state.bot_post_times.append(now)
            # Buffer trimmen: nur 2x das Window relevant, alles davor wegwerfen.
            cutoff = now - timedelta(seconds=self._burst_window_sec * 2)
            state.bot_post_times = [t for t in state.bot_post_times if t >= cutoff]
            state.user_post_since_last_bot = False

    def note_user_post(self, channel_login: str) -> None:
        with self._lock:
            state = self._get_state(channel_login)
            state.user_post_since_last_bot = True
