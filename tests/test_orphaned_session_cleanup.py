import contextlib
import unittest

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
                    {
                        "id": 101,
                        "streamer_login": "partner_one",
                        "finalized_at": "2026-03-29T10:00:00+00:00",
                    },
                ]
            ),
            _FakeCursor(
                rows=[
                    {
                        "id": 202,
                        "streamer_login": "partner_two",
                        "finalized_at": "2026-03-29T11:00:00+00:00",
                    },
                ]
            ),
        ]

    def execute(self, sql: str, params=()):
        self.calls.append((str(sql), tuple(params or ())))
        if not self._responses:
            raise AssertionError("No fake cursor configured")
        return self._responses.pop(0)


class _SessionsHarness(_SessionsMixin):
    def __init__(self) -> None:
        self.finalize_calls: list[dict[str, object]] = []

    async def _finalize_stream_session(self, **kwargs) -> bool:
        self.finalize_calls.append(dict(kwargs))
        return True


class OrphanedSessionCleanupTests(unittest.IsolatedAsyncioTestCase):
    async def test_cleanup_uses_regular_finalize_flow_for_detected_orphans(self) -> None:
        harness = _SessionsHarness()
        conn = _RecordingConn()

        with unittest.mock.patch(
            "bot.monitoring.sessions_mixin.storage.readonly_connection",
            side_effect=lambda: contextlib.nullcontext(conn),
        ):
            closed = await harness._cleanup_orphaned_sessions()

        self.assertEqual(closed, 2)
        self.assertEqual(len(conn.calls), 2)
        self.assertIn("SELECT id,", conn.calls[0][0])
        self.assertIn("MAX(sv.ts_utc)", conn.calls[1][0])
        self.assertEqual(
            harness.finalize_calls,
            [
                {
                    "login": "partner_one",
                    "reason": "auto-closed: orphaned session (no samples, open > 24h)",
                    "session_id": 101,
                    "ended_at": unittest.mock.ANY,
                },
                {
                    "login": "partner_two",
                    "reason": "auto-closed: stale session (last viewer data > 1h ago)",
                    "session_id": 202,
                    "ended_at": unittest.mock.ANY,
                },
            ],
        )


if __name__ == "__main__":
    unittest.main()
