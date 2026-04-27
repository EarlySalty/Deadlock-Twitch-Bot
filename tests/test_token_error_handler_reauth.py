from __future__ import annotations

import contextlib
import sqlite3
import unittest
from unittest.mock import ANY, AsyncMock, patch

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
            twitch_login TEXT,
            error_message TEXT,
            error_count INTEGER DEFAULT 1,
            first_error_at TEXT,
            last_error_at TEXT,
            grace_expires_at TEXT,
            notified INTEGER DEFAULT 0,
            user_dm_sent INTEGER DEFAULT 0,
            reminder_sent INTEGER DEFAULT 0,
            role_removed INTEGER DEFAULT 0
        )
        """
    )
    conn.execute(
        """
        CREATE TABLE twitch_partners (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            twitch_user_id TEXT,
            twitch_login TEXT,
            manual_partner_opt_out INTEGER DEFAULT 0,
            raid_bot_enabled INTEGER DEFAULT 1,
            status TEXT DEFAULT 'active'
        )
        """
    )
    conn.execute(
        """
        CREATE TABLE twitch_raid_blacklist (
            target_login TEXT PRIMARY KEY,
            reason TEXT
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

    def test_disable_raid_bot_marks_reauth_without_partner_opt_out(self) -> None:
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
        conn.execute(
            """
            INSERT INTO twitch_partners (twitch_user_id, twitch_login, manual_partner_opt_out, raid_bot_enabled, status)
            VALUES (?, ?, 0, 1, 'active')
            """,
            ("1001", "alpha"),
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
            patch(
                "bot.api.token_error_handler.load_active_partner",
                return_value={"id": 1},
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
        partner_row = conn.execute(
            """
            SELECT manual_partner_opt_out, raid_bot_enabled
            FROM twitch_partners
            WHERE twitch_user_id = ?
            """,
            ("1001",),
        ).fetchone()
        self.assertIsNotNone(partner_row)
        self.assertEqual(int(partner_row["manual_partner_opt_out"]), 0)
        self.assertEqual(int(partner_row["raid_bot_enabled"]), 1)
        set_partner_flag.assert_called_once_with(ANY, twitch_user_id="1001", enabled=False)

    def test_add_to_blacklist_marks_reauth_without_partner_opt_out_on_first_failure(self) -> None:
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
            INSERT INTO twitch_partners (twitch_user_id, twitch_login, manual_partner_opt_out, raid_bot_enabled, status)
            VALUES (?, ?, 0, 1, 'active')
            """,
            ("1001", "alpha"),
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
            handler.add_to_blacklist(
                "1001",
                "alpha",
                'HTTP 400: {"status":400,"message":"Invalid refresh token"}',
            )

        auth_row = conn.execute(
            "SELECT raid_enabled, needs_reauth FROM twitch_raid_auth WHERE twitch_user_id = ?",
            ("1001",),
        ).fetchone()
        self.assertEqual(int(auth_row["raid_enabled"]), 0)
        self.assertEqual(int(auth_row["needs_reauth"]), 1)
        partner_row = conn.execute(
            """
            SELECT manual_partner_opt_out, raid_bot_enabled
            FROM twitch_partners
            WHERE twitch_user_id = ?
            """,
            ("1001",),
        ).fetchone()
        self.assertEqual(int(partner_row["manual_partner_opt_out"]), 0)
        self.assertEqual(int(partner_row["raid_bot_enabled"]), 1)
        blacklist_row = conn.execute(
            "SELECT error_count, grace_expires_at FROM twitch_token_blacklist WHERE twitch_user_id = ?",
            ("1001",),
        ).fetchone()
        self.assertEqual(int(blacklist_row["error_count"]), 1)
        self.assertTrue(blacklist_row["grace_expires_at"])
        set_partner_flag.assert_called_once_with(ANY, twitch_user_id="1001", enabled=False)

    def test_mark_partner_opt_out_only_keeps_needs_reauth_false(self) -> None:
        conn = _make_conn()
        conn.execute(
            """
            INSERT INTO twitch_raid_auth (twitch_user_id, twitch_login, raid_enabled, needs_reauth)
            VALUES (?, ?, 1, 0)
            """,
            ("2002", "bravo"),
        )
        conn.execute(
            """
            INSERT INTO twitch_partners (twitch_user_id, twitch_login, manual_partner_opt_out, raid_bot_enabled, status)
            VALUES (?, ?, 0, 1, 'active')
            """,
            ("2002", "bravo"),
        )

        with (
            patch(
                "bot.api.token_error_handler.transaction",
                side_effect=lambda: contextlib.nullcontext(_CompatConn(conn)),
            ),
            patch(
                "bot.api.token_error_handler.load_active_partner",
                return_value={"id": 1},
            ),
            patch("bot.api.token_error_handler.set_partner_raid_bot_enabled") as set_partner_flag,
            patch.object(TokenErrorHandler, "_migrate_db", return_value=None),
        ):
            handler = TokenErrorHandler()
            handler._mark_partner_opt_out_only("2002", "bravo")

        row = conn.execute(
            """
            SELECT raid_enabled, needs_reauth
            FROM twitch_raid_auth
            WHERE twitch_user_id = ?
            """,
            ("2002",),
        ).fetchone()
        self.assertEqual(int(row["raid_enabled"]), 0)
        self.assertEqual(int(row["needs_reauth"]), 0)
        partner_row = conn.execute(
            """
            SELECT manual_partner_opt_out, raid_bot_enabled
            FROM twitch_partners
            WHERE twitch_user_id = ?
            """,
            ("2002",),
        ).fetchone()
        self.assertEqual(int(partner_row["manual_partner_opt_out"]), 1)
        self.assertEqual(int(partner_row["raid_bot_enabled"]), 0)
        set_partner_flag.assert_called_once_with(ANY, twitch_user_id="2002", enabled=False)

    def test_restore_bot_banned_channel_reenables_partner_after_health_recovers(self) -> None:
        conn = _make_conn()
        conn.execute(
            """
            INSERT INTO twitch_raid_auth (twitch_user_id, twitch_login, raid_enabled, needs_reauth)
            VALUES (?, ?, 0, 0)
            """,
            ("3003", "charlie"),
        )
        conn.execute(
            """
            INSERT INTO twitch_partners (id, twitch_user_id, twitch_login, manual_partner_opt_out, raid_bot_enabled, status)
            VALUES (?, ?, ?, 1, 0, 'active')
            """,
            (1, "3003", "charlie"),
        )
        conn.execute(
            """
            INSERT INTO twitch_raid_blacklist (target_login, reason)
            VALUES (?, ?)
            """,
            ("charlie", "chat_bot_banned_in_channel"),
        )

        with (
            patch(
                "bot.api.token_error_handler.transaction",
                side_effect=lambda: contextlib.nullcontext(_CompatConn(conn)),
            ),
            patch(
                "bot.api.token_error_handler.load_active_partner",
                return_value={"id": 1, "manual_partner_opt_out": 1},
            ),
            patch("bot.api.token_error_handler.set_partner_raid_bot_enabled") as set_partner_flag,
            patch.object(TokenErrorHandler, "_migrate_db", return_value=None),
        ):
            handler = TokenErrorHandler()
            restored = handler.restore_bot_banned_channel("3003", "charlie")

        self.assertTrue(restored)
        row = conn.execute(
            """
            SELECT raid_enabled, needs_reauth
            FROM twitch_raid_auth
            WHERE twitch_user_id = ?
            """,
            ("3003",),
        ).fetchone()
        self.assertEqual(int(row["raid_enabled"]), 1)
        self.assertEqual(int(row["needs_reauth"]), 0)
        partner_row = conn.execute(
            """
            SELECT manual_partner_opt_out, raid_bot_enabled
            FROM twitch_partners
            WHERE twitch_user_id = ?
            """,
            ("3003",),
        ).fetchone()
        self.assertEqual(int(partner_row["manual_partner_opt_out"]), 0)
        self.assertEqual(int(partner_row["raid_bot_enabled"]), 1)
        blacklist_row = conn.execute(
            "SELECT 1 FROM twitch_raid_blacklist WHERE target_login = ?",
            ("charlie",),
        ).fetchone()
        self.assertIsNone(blacklist_row)
        set_partner_flag.assert_called_once_with(ANY, twitch_user_id="3003", enabled=True)


class _DiscordBotWithoutChannel:
    def get_channel(self, _channel_id: int):
        return None


class _DiscordBotMissingUser:
    def get_user(self, _user_id: int):
        return None

    async def fetch_user(self, _user_id: int):
        return None


class TokenErrorHandlerNotificationTests(unittest.IsolatedAsyncioTestCase):
    async def test_check_grace_periods_departners_after_expiry(self) -> None:
        conn = _make_conn()
        conn.execute(
            """
            INSERT INTO twitch_token_blacklist (
                twitch_user_id, twitch_login, error_message, error_count,
                first_error_at, last_error_at, grace_expires_at,
                notified, reminder_sent, role_removed
            )
            VALUES (?, ?, ?, 3, ?, ?, ?, 0, 0, 0)
            """,
            (
                "1001",
                "alpha",
                'HTTP 400: {"status":400,"message":"Invalid refresh token"}',
                "2026-04-01T00:00:00+00:00",
                "2026-04-01T00:00:00+00:00",
                "2026-04-08T00:00:00+00:00",
            ),
        )

        with (
            patch.object(TokenErrorHandler, "_migrate_db", return_value=None),
            patch(
                "bot.api.token_error_handler.transaction",
                side_effect=lambda: contextlib.nullcontext(_CompatConn(conn)),
            ),
            patch(
                "bot.api.token_error_handler.readonly_connection",
                side_effect=lambda: contextlib.nullcontext(_CompatConn(conn)),
            ),
            patch("bot.api.token_error_handler.departner_active_partner") as departner,
        ):
            handler = TokenErrorHandler(discord_bot=_DiscordBotWithoutChannel())
            handler._get_discord_user_id = lambda *_: "123456789"  # type: ignore[method-assign]
            handler._send_user_dm_token_error = AsyncMock(return_value=True)  # type: ignore[method-assign]
            handler.schedule_streamer_role_sync = lambda *_, **__: None  # type: ignore[method-assign]
            await handler.check_grace_periods()

        departner.assert_called_once_with(
            ANY,
            twitch_user_id="1001",
            twitch_login="alpha",
            restore_non_partner=False,
            disable_raid_auth=True,
            clear_verification=True,
        )
        row = conn.execute(
            "SELECT reminder_sent, role_removed FROM twitch_token_blacklist WHERE twitch_user_id = ?",
            ("1001",),
        ).fetchone()
        self.assertEqual(int(row["reminder_sent"]), 1)
        self.assertEqual(int(row["role_removed"]), 1)

    async def test_notify_token_error_still_sends_user_dm_when_admin_channel_missing(self) -> None:
        conn = _make_conn()
        conn.execute(
            """
            INSERT INTO twitch_token_blacklist (twitch_user_id, grace_expires_at, notified)
            VALUES (?, NULL, 0)
            """,
            ("1001",),
        )
        conn.execute(
            """
            INSERT INTO twitch_raid_auth (twitch_user_id, twitch_login, raid_enabled, needs_reauth)
            VALUES (?, ?, 0, 1)
            """,
            ("1001", "alpha"),
        )

        with patch.object(TokenErrorHandler, "_migrate_db", return_value=None):
            handler = TokenErrorHandler(discord_bot=_DiscordBotWithoutChannel())

        handler._send_user_dm_token_error = AsyncMock(return_value=True)  # type: ignore[method-assign]

        with (
            patch(
                "bot.api.token_error_handler.transaction",
                side_effect=lambda: contextlib.nullcontext(_CompatConn(conn)),
            ),
            patch(
                "bot.api.token_error_handler.readonly_connection",
                side_effect=lambda: contextlib.nullcontext(_CompatConn(conn)),
            ),
            patch.object(handler, "_mark_reauth_required") as mark_reauth,
        ):
            await handler.notify_token_error(
                twitch_user_id="1001",
                twitch_login="alpha",
                error_message='HTTP 400: {"status":400,"message":"Invalid refresh token"}',
            )

        handler._send_user_dm_token_error.assert_awaited_once_with(
            "1001",
            "alpha",
            'HTTP 400: {"status":400,"message":"Invalid refresh token"}',
        )
        mark_reauth.assert_called_once_with("1001", "alpha", mark_notified=True)
        row = conn.execute(
            "SELECT notified FROM twitch_token_blacklist WHERE twitch_user_id = ?",
            ("1001",),
        ).fetchone()
        self.assertIsNotNone(row)
        self.assertEqual(int(row["notified"]), 1)

    async def test_send_user_dm_token_error_returns_false_when_user_cannot_be_resolved(self) -> None:
        with patch.object(TokenErrorHandler, "_migrate_db", return_value=None):
            handler = TokenErrorHandler(discord_bot=_DiscordBotMissingUser())

        with patch.object(handler, "_get_discord_user_id", return_value="137246526119477248"):
            sent = await handler._send_user_dm_token_error(
                "87111803",
                "snaqeu",
                'HTTP 400: {"status":400,"message":"Invalid refresh token"}',
            )

        self.assertFalse(sent)

    async def test_send_user_dm_bot_banned_returns_false_when_user_cannot_be_resolved(self) -> None:
        with patch.object(TokenErrorHandler, "_migrate_db", return_value=None):
            handler = TokenErrorHandler(discord_bot=_DiscordBotMissingUser())

        with patch.object(handler, "_get_discord_user_id", return_value="137246526119477248"):
            sent = await handler._send_user_dm_bot_banned(
                "87111803",
                "snaqeu",
                "chat_bot_banned_in_channel",
            )

        self.assertFalse(sent)


if __name__ == "__main__":
    unittest.main()
