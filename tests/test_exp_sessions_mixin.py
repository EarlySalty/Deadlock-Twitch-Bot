import contextlib
import unittest
from unittest.mock import patch

from bot.monitoring.exp_sessions_mixin import _ExpSessionsMixin


class _ExpHarness(_ExpSessionsMixin):
    pass


class _FetchOneCursor:
    def __init__(self, row):
        self._row = row

    def fetchone(self):
        return self._row


class _SampleConn:
    def __init__(self, *, inserted_row):
        self.calls: list[str] = []
        self._inserted_row = inserted_row

    def execute(self, sql: str, params=()):
        normalized = " ".join(str(sql).split())
        self.calls.append(normalized)
        if "SELECT started_at, samples, avg_viewers, peak_viewers FROM exp_sessions" in normalized:
            return _FetchOneCursor(("2026-03-31T10:00:00+00:00", 5, 10.0, 25))
        if "INSERT INTO exp_snapshots" in normalized:
            return _FetchOneCursor(self._inserted_row)
        if "UPDATE exp_sessions SET samples = %s, avg_viewers = %s, peak_viewers = %s" in normalized:
            return _FetchOneCursor(None)
        raise AssertionError(normalized)


class _ExistingExpSessionConn:
    def execute(self, sql: str, params=()):
        normalized = " ".join(str(sql).split())
        if "SELECT id FROM exp_sessions WHERE stream_id = %s AND ended_at IS NULL LIMIT 1" in normalized:
            return _FetchOneCursor((42,))
        raise AssertionError(normalized)


class _UniqueViolation(Exception):
    pass


class ExpSessionsMixinTests(unittest.TestCase):
    def test_exp_session_start_recovers_existing_session_after_unique_violation(self) -> None:
        harness = _ExpHarness()

        class _PsycopgErrors:
            UniqueViolation = _UniqueViolation

        fake_psycopg = type("_FakePsycopg", (), {"errors": _PsycopgErrors})()

        with (
            patch(
                "bot.monitoring.exp_sessions_mixin.storage.transaction",
                side_effect=_UniqueViolation("duplicate stream_id"),
            ),
            patch(
                "bot.monitoring.exp_sessions_mixin.storage.readonly_connection",
                side_effect=lambda: contextlib.nullcontext(_ExistingExpSessionConn()),
            ),
            patch("bot.monitoring.exp_sessions_mixin.psycopg", fake_psycopg),
        ):
            exp_id = harness._exp_on_session_start(
                login="foo",
                stream={"id": "stream-1", "viewer_count": 10},
                started_at_iso="2026-03-31T10:00:00+00:00",
            )

        self.assertEqual(exp_id, 42)
        self.assertEqual(harness._get_exp_session_id("foo"), 42)

    def test_duplicate_exp_snapshot_does_not_update_aggregates(self) -> None:
        harness = _ExpHarness()
        conn = _SampleConn(inserted_row=None)

        with patch(
            "bot.monitoring.exp_sessions_mixin.storage.transaction",
            side_effect=lambda: contextlib.nullcontext(conn),
        ):
            harness._exp_on_session_sample(
                login="foo",
                exp_session_id=7,
                stream={"viewer_count": 33},
            )

        self.assertEqual(len(conn.calls), 2)
        self.assertIn("SELECT started_at, samples, avg_viewers, peak_viewers FROM exp_sessions", conn.calls[0])
        self.assertIn("INSERT INTO exp_snapshots", conn.calls[1])
        self.assertFalse(any("UPDATE exp_sessions SET samples" in call for call in conn.calls))

    def test_new_exp_snapshot_updates_aggregates(self) -> None:
        harness = _ExpHarness()
        conn = _SampleConn(inserted_row=(99,))

        with patch(
            "bot.monitoring.exp_sessions_mixin.storage.transaction",
            side_effect=lambda: contextlib.nullcontext(conn),
        ):
            harness._exp_on_session_sample(
                login="foo",
                exp_session_id=7,
                stream={"viewer_count": 33},
            )

        self.assertTrue(any("UPDATE exp_sessions SET samples" in call for call in conn.calls))


if __name__ == "__main__":
    unittest.main()
