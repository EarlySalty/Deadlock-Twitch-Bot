import logging
import time
from collections import defaultdict

log = logging.getLogger("TwitchStreams.ChatBot")

_MUTE_DAILY_THRESHOLD = 2
_MUTE_WEEKLY_THRESHOLD = 5
_MUTE_DURATION_SEC = 7 * 24 * 3600
_PITCH_COOLDOWN_SEC = 24 * 3600
_WEEK_SEC = 7 * 24 * 3600
_DAY_SEC = 24 * 3600

WERBEFREI_PITCH_URL = "https://deutsche-deadlock-community.de/twitch/pricing"
WERBEFREI_PITCH_MSG = (
    "Kurzer Hinweis: Beim letzten Stream wurde der Bot in diesem Chat getimed outed 🙈 "
    "Falls die automatischen Promo-Nachrichten stören – es gibt ein Werbefrei-Abo, "
    "das alle Bot-Features ohne automatische Nachrichten bietet: "
    + WERBEFREI_PITCH_URL
)

_BOT_TIMEOUT_DROP_CODES = frozenset({"sender_banned", "sender_timedout"})


class TimeoutGuard:
    """Trackt Bot-Timeouts pro Channel und schaltet den Bot bei Wiederholung stumm."""

    def __init__(self) -> None:
        self._timeouts: dict[str, list[float]] = defaultdict(list)
        self._muted_until: dict[str, float] = {}
        self._last_pitch: dict[str, float] = {}
        self._pending_pitch: set[str] = set()

    def _prune(self, login: str, now: float) -> None:
        cutoff = now - _WEEK_SEC
        self._timeouts[login] = [t for t in self._timeouts[login] if t > cutoff]

    def record_timeout(self, login: str, now: float | None = None) -> None:
        now = now or time.monotonic()
        self._timeouts[login].append(now)
        self._prune(login, now)

        day_count = sum(1 for t in self._timeouts[login] if t > now - _DAY_SEC)
        week_count = len(self._timeouts[login])
        if (day_count >= _MUTE_DAILY_THRESHOLD or week_count >= _MUTE_WEEKLY_THRESHOLD) and not self.is_muted(login, now):
            self._muted_until[login] = now + _MUTE_DURATION_SEC
            log.warning(
                "Bot-Timeout-Limit erreicht für %s (heute=%d, Woche=%d) → alle Bot-Funktionen für 7 Tage deaktiviert",
                login, day_count, week_count,
            )

        last_pitch = self._last_pitch.get(login, 0.0)
        if now - last_pitch >= _PITCH_COOLDOWN_SEC:
            self._pending_pitch.add(login)

    def is_muted(self, login: str, now: float | None = None) -> bool:
        until = self._muted_until.get(login, 0.0)
        if until <= 0:
            return False
        return (now or time.monotonic()) < until

    def consume_stream_start_pitch(self, login: str, now: float | None = None) -> bool:
        """True wenn beim nächsten Stream-Start ein Werbefrei-Pitch gesendet werden soll."""
        if login not in self._pending_pitch:
            return False
        self._pending_pitch.discard(login)
        self._last_pitch[login] = now or time.monotonic()
        return True

    def timeout_counts(self, login: str, now: float | None = None) -> tuple[int, int]:
        now = now or time.monotonic()
        self._prune(login, now)
        day = sum(1 for t in self._timeouts[login] if t > now - _DAY_SEC)
        week = len(self._timeouts[login])
        return day, week


_guard = TimeoutGuard()


def get_timeout_guard() -> TimeoutGuard:
    return _guard
