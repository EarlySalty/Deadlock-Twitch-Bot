from __future__ import annotations

import sqlite3
import unittest

from bot.storage.partner_registry import load_offline_auto_raid_eligibility

from tests.sqlite_twitch_schema import ensure_sqlite_twitch_schema


class _SqlitePgCompatConnection:
    def __init__(self, conn: sqlite3.Connection) -> None:
        self._conn = conn

    @staticmethod
    def _translate_sql(sql: str) -> str:
        return str(sql).replace("%s", "?")

    def execute(self, sql, params=()):
        return self._conn.execute(self._translate_sql(sql), params)

    def __getattr__(self, name):
        return getattr(self._conn, name)


class _StaticCursor:
    def __init__(self, row) -> None:
        self._row = row

    def fetchone(self):
        return self._row


class OfflineAutoRaidEligibilityTests(unittest.TestCase):
    def setUp(self) -> None:
        self.conn = sqlite3.connect(":memory:")
        self.conn.row_factory = sqlite3.Row
        ensure_sqlite_twitch_schema(self.conn)
        self.compat_conn = _SqlitePgCompatConnection(self.conn)

    def tearDown(self) -> None:
        self.conn.close()

    def _insert_active_partner(
        self,
        *,
        twitch_user_id: str,
        twitch_login: str,
        raid_bot_enabled: int,
    ) -> None:
        self.conn.execute(
            """
            INSERT INTO twitch_streamer_identities (
                twitch_user_id, twitch_login
            ) VALUES (?, ?)
            ON CONFLICT(twitch_user_id) DO UPDATE SET
                twitch_login = excluded.twitch_login
            """,
            (twitch_user_id, twitch_login),
        )
        self.conn.execute(
            """
            INSERT INTO twitch_partners (
                twitch_user_id,
                twitch_login,
                raid_bot_enabled,
                status
            ) VALUES (?, ?, ?, 'active')
            """,
            (twitch_user_id, twitch_login, raid_bot_enabled),
        )

    def _insert_raid_auth(
        self,
        *,
        twitch_user_id: str,
        twitch_login: str,
        raid_enabled: int,
    ) -> None:
        self.conn.execute(
            """
            INSERT INTO twitch_raid_auth (
                twitch_user_id,
                twitch_login,
                raid_enabled
            ) VALUES (?, ?, ?)
            """,
            (twitch_user_id, twitch_login, raid_enabled),
        )

    def test_enabled_streamer_is_fully_eligible(self) -> None:
        self._insert_active_partner(
            twitch_user_id="1001",
            twitch_login="alpha",
            raid_bot_enabled=1,
        )
        self._insert_raid_auth(
            twitch_user_id="1001",
            twitch_login="alpha",
            raid_enabled=1,
        )

        result = load_offline_auto_raid_eligibility(
            self.compat_conn,
            twitch_user_id="1001",
        )

        self.assertEqual(result.twitch_user_id, "1001")
        self.assertEqual(result.twitch_login, "alpha")
        self.assertTrue(result.active_partner)
        self.assertTrue(result.auth_row_found)
        self.assertTrue(result.raid_bot_enabled)
        self.assertTrue(result.raid_auth_enabled)
        self.assertTrue(result.can_auto_raid)

    def test_disabled_by_partner_setting_blocks_auto_raid(self) -> None:
        self._insert_active_partner(
            twitch_user_id="2002",
            twitch_login="bravo",
            raid_bot_enabled=0,
        )
        self._insert_raid_auth(
            twitch_user_id="2002",
            twitch_login="bravo",
            raid_enabled=1,
        )

        result = load_offline_auto_raid_eligibility(
            self.compat_conn,
            twitch_user_id="2002",
        )

        self.assertTrue(result.active_partner)
        self.assertTrue(result.auth_row_found)
        self.assertFalse(result.raid_bot_enabled)
        self.assertTrue(result.raid_auth_enabled)
        self.assertFalse(result.can_auto_raid)

    def test_disabled_by_auth_blocks_auto_raid(self) -> None:
        self._insert_active_partner(
            twitch_user_id="3003",
            twitch_login="charlie",
            raid_bot_enabled=1,
        )
        self._insert_raid_auth(
            twitch_user_id="3003",
            twitch_login="charlie",
            raid_enabled=0,
        )

        result = load_offline_auto_raid_eligibility(
            self.compat_conn,
            twitch_user_id="3003",
        )

        self.assertTrue(result.active_partner)
        self.assertTrue(result.auth_row_found)
        self.assertTrue(result.raid_bot_enabled)
        self.assertFalse(result.raid_auth_enabled)
        self.assertFalse(result.can_auto_raid)

    def test_not_found_returns_not_eligible_defaults(self) -> None:
        result = load_offline_auto_raid_eligibility(
            self.compat_conn,
            twitch_user_id="4004",
        )

        self.assertEqual(result.twitch_user_id, "4004")
        self.assertIsNone(result.twitch_login)
        self.assertFalse(result.active_partner)
        self.assertFalse(result.auth_row_found)
        self.assertFalse(result.raid_bot_enabled)
        self.assertFalse(result.raid_auth_enabled)
        self.assertFalse(result.can_auto_raid)

    def test_missing_partner_tables_still_reports_auth_state(self) -> None:
        self._insert_raid_auth(
            twitch_user_id="5005",
            twitch_login="echo",
            raid_enabled=1,
        )
        self.conn.execute("DROP TABLE twitch_partners")
        self.conn.execute("DROP TABLE twitch_streamer_identities")

        result = load_offline_auto_raid_eligibility(
            self.compat_conn,
            twitch_user_id="5005",
        )

        self.assertFalse(result.active_partner)
        self.assertTrue(result.auth_row_found)
        self.assertFalse(result.raid_bot_enabled)
        self.assertTrue(result.raid_auth_enabled)
        self.assertEqual(result.twitch_login, "echo")

    def test_missing_auth_table_still_reports_partner_state(self) -> None:
        self._insert_active_partner(
            twitch_user_id="6006",
            twitch_login="foxtrot",
            raid_bot_enabled=1,
        )
        self.conn.execute("DROP TABLE twitch_raid_auth")

        result = load_offline_auto_raid_eligibility(
            self.compat_conn,
            twitch_user_id="6006",
        )

        self.assertTrue(result.active_partner)
        self.assertFalse(result.auth_row_found)
        self.assertTrue(result.raid_bot_enabled)
        self.assertFalse(result.raid_auth_enabled)
        self.assertEqual(result.twitch_login, "foxtrot")

    def test_permission_denied_for_partner_table_is_not_downgraded(self) -> None:
        class _PermissionDeniedConn:
            def execute(self, sql, params=()):
                del params
                if "FROM twitch_partners" in str(sql):
                    raise RuntimeError("permission denied for table twitch_partners")
                return _StaticCursor(None)

        with self.assertRaisesRegex(RuntimeError, "permission denied"):
            load_offline_auto_raid_eligibility(
                _PermissionDeniedConn(),
                twitch_user_id="7007",
            )

    def test_permission_denied_for_auth_table_is_not_downgraded(self) -> None:
        class _PermissionDeniedConn:
            def execute(self, sql, params=()):
                del params
                sql_text = str(sql)
                if "FROM twitch_raid_auth" in sql_text:
                    raise RuntimeError("permission denied for table twitch_raid_auth")
                return _StaticCursor(None)

        with self.assertRaisesRegex(RuntimeError, "permission denied"):
            load_offline_auto_raid_eligibility(
                _PermissionDeniedConn(),
                twitch_user_id="8008",
            )


if __name__ == "__main__":
    unittest.main()
