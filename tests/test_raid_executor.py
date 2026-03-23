from __future__ import annotations

import contextlib
import unittest
from unittest.mock import patch

from bot.raid.executor import RaidExecutor


class _RecordingConn:
    def __init__(self) -> None:
        self.calls: list[tuple[str, tuple[object, ...]]] = []

    def execute(self, sql: str, params=()) -> None:
        self.calls.append((sql, tuple(params)))


class RaidExecutorTests(unittest.TestCase):
    def test_save_raid_history_uses_psycopg_placeholders(self) -> None:
        conn = _RecordingConn()
        executor = RaidExecutor("client-id", auth_manager=object())

        with patch(
            "bot.raid.executor.transaction",
            side_effect=lambda: contextlib.nullcontext(conn),
        ):
            executor._save_raid_history(
                "100",
                "derechtecoolys",
                "200",
                "svvagnertv",
                42,
                3600,
                "2026-03-23T14:30:00+00:00",
                7,
                "auto_raid_on_offline",
                success=True,
                error_message=None,
            )

        self.assertEqual(len(conn.calls), 1)
        sql, params = conn.calls[0]
        self.assertIn("VALUES (%s, %s, %s, %s, %s, %s, %s, %s, %s, %s, %s)", sql)
        self.assertEqual(
            params,
            (
                "100",
                "derechtecoolys",
                "200",
                "svvagnertv",
                42,
                3600,
                "auto_raid_on_offline",
                True,
                None,
                "2026-03-23T14:30:00+00:00",
                7,
            ),
        )


if __name__ == "__main__":
    unittest.main()
