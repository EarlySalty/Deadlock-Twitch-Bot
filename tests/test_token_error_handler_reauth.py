from __future__ import annotations

import contextlib
import sqlite3
import unittest
from unittest.mock import ANY, patch

from bot.api.token_error_handler import TokenErrorHandler


def _make_conn() -> sqlite3.Connection:
    conn = sqlite3.connect(":memory:")
    conn.row_factory = sqlite3.Row
    conn.execute(
        """
        CREATE TABLE twitch_raid_auth (
            twitch_user_id TEXT PRIMARY KEY,
            twitch_login TEXT,
            raid_enabled INTEGER DEFAULT 1,
            needs_reauth INTEGER DEFAULT 0,
            reauth_notified_at TEXT
        )
        """
    )
    conn.execute(
        """
        CREATE TABLE twitch_token_blacklist (
            twitch_user_id TEXT PRIMARY KEY,
            grace_expires_at TEXT,
            notified INTEGER DEFAULT 0
        )
        """
    )
    return conn


class _CompatConn:
    def __init__(self, conn: sqlite3.Connection) -> None:
        self._conn = conn

    def execute(self, sql: str, params=()):
        return self._conn.execute(str(sql).replace("%s", "?"), params)

    def __getattr__(self, name: str):
        return getattr(self._conn, name)


class TokenErrorHandlerReauthTests(unittest.TestCase):
    def test_migrate_db_prepares_runtime_storage_when_called_too_early(self) -> None:
        transaction_calls = 0
        conn = _CompatConn(_make_conn())

        def _transaction():
            nonlocal transaction_calls
            transaction_calls += 1
            if transaction_calls == 1:
                raise RuntimeError(
                    "PostgreSQL storage is not initialized. Call prepare_runtime_storage() during startup before serving runtime requests."
                )
            return contextlib.nullcontext(conn)

        with (
            patch("bot.api.token_error_handler.transaction", side_effect=_transaction),
            patch("bot.api.token_error_handler.storage_pg.prepare_runtime_storage") as prepare_storage,
        ):
            TokenErrorHandler._migrate_db()

        prepare_storage.assert_called_once_with()
        self.assertEqual(transaction_calls, 2)

    def test_disable_raid_bot_marks_reauth_without_removing_auth_row(self) -> None:
        conn = _make_conn()
        conn.execute(
            """
            INSERT INTO twitch_raid_auth (twitch_user_id, twitch_login, raid_enabled, needs_reauth)
            VALUES (?, ?, 1, 0)
            """,
            ("1001", "alpha"),
        )
        conn.execute(
            """
            INSERT INTO twitch_token_blacklist (twitch_user_id, grace_expires_at, notified)
            VALUES (?, NULL, 0)
            """,
            ("1001",),
        )

        with (
            patch(
                "bot.api.token_error_handler.transaction",
                side_effect=lambda: contextlib.nullcontext(_CompatConn(conn)),
            ),
            patch(
                "bot.api.token_error_handler.readonly_connection",
                side_effect=lambda: contextlib.nullcontext(_CompatConn(conn)),
            ),
            patch("bot.api.token_error_handler.set_partner_raid_bot_enabled") as set_partner_flag,
            patch.object(TokenErrorHandler, "_migrate_db", return_value=None),
        ):
            handler = TokenErrorHandler()
            handler._disable_raid_bot("1001")

        row = conn.execute(
            """
            SELECT raid_enabled, needs_reauth, twitch_login
            FROM twitch_raid_auth
            WHERE twitch_user_id = ?
            """,
            ("1001",),
        ).fetchone()
        self.assertIsNotNone(row)
        self.assertEqual(int(row["raid_enabled"]), 0)
        self.assertEqual(int(row["needs_reauth"]), 1)
        self.assertEqual(row["twitch_login"], "alpha")
        set_partner_flag.assert_called_once_with(ANY, twitch_user_id="1001", enabled=False)


if __name__ == "__main__":
    unittest.main()
