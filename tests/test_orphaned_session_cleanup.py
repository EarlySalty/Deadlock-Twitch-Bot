import contextlib
import unittest
from unittest.mock import patch

from bot.monitoring.sessions_mixin import _SessionsMixin


class _FakeCursor:
    def __init__(self, rows: list[object] | None = None) -> None:
        self._rows = list(rows or [])

    def fetchall(self) -> list[object]:
        return list(self._rows)


class _RecordingConn:
    def __init__(self) -> None:
        self.calls: list[tuple[str, tuple[object, ...]]] = []
        self._responses = [
            _FakeCursor(
                rows=[
                    {"id": 101, "streamer_login": "partner_one"},
                ]
            ),
            _FakeCursor(
                rows=[
                    {"id": 202, "streamer_login": "partner_two"},
                ]
            ),
            _FakeCursor(),
        ]

    def execute(self, sql: str, params=()):
        self.calls.append((str(sql), tuple(params or ())))
        if not self._responses:
            raise AssertionError("No fake cursor configured")
        return self._responses.pop(0)


class _SessionsHarness(_SessionsMixin):
    def __init__(self) -> None:
        self._active_sessions = {
            "partner_one": 101,
            "partner_two": 202,
            "partner_three": 303,
        }


class OrphanedSessionCleanupTests(unittest.TestCase):
    def test_cleanup_clears_live_state_active_session_ids_and_runtime_cache(self) -> None:
        harness = _SessionsHarness()
        conn = _RecordingConn()

        with patch(
            "bot.monitoring.sessions_mixin.storage.readonly_connection",
            side_effect=lambda: contextlib.nullcontext(conn),
        ):
            closed = harness._cleanup_orphaned_sessions()

        self.assertEqual(closed, 2)
        self.assertEqual(harness._active_sessions, {"partner_three": 303})
        self.assertEqual(len(conn.calls), 3)
        self.assertIn("UPDATE twitch_live_state", conn.calls[2][0])
        self.assertIn("is_live = 0", conn.calls[2][0])
        self.assertEqual(conn.calls[2][1], ([101, 202],))


if __name__ == "__main__":
    unittest.main()
