import contextlib
import sqlite3
import unittest
from unittest.mock import patch

from bot.raid.integration_state import RaidIntegrationStateResolver


class _CompatConn:
    def __init__(self, conn: sqlite3.Connection) -> None:
        self._conn = conn

    def execute(self, sql: str, params=()):
        return self._conn.execute(str(sql).replace("%s", "?"), params)

    def __getattr__(self, name: str):
        return getattr(self._conn, name)


class _FakeAuthManager:
    def __init__(self, authorized_user_ids: set[str] | None = None) -> None:
        self.authorized_user_ids = authorized_user_ids or set()
        self.checked_user_ids: list[str] = []

    def has_enabled_auth(self, twitch_user_id: str) -> bool:
        self.checked_user_ids.append(twitch_user_id)
        return twitch_user_id in self.authorized_user_ids


class _FakeTokenErrorHandler:
    def __init__(self, blacklisted_user_ids: set[str] | None = None) -> None:
        self.blacklisted_user_ids = blacklisted_user_ids or set()
        self.checked_user_ids: list[str] = []

    def is_token_blacklisted(self, twitch_user_id: str) -> bool:
        self.checked_user_ids.append(twitch_user_id)
        return twitch_user_id in self.blacklisted_user_ids


class RaidIntegrationStateResolverTests(unittest.TestCase):
    def _make_conn(self) -> sqlite3.Connection:
        conn = sqlite3.connect(":memory:")
        conn.row_factory = sqlite3.Row
        conn.execute(
            """
            CREATE TABLE twitch_streamers_partner_state (
                twitch_login TEXT,
                twitch_user_id TEXT,
                discord_user_id TEXT,
                manual_partner_opt_out INTEGER DEFAULT 0,
                manual_verified_at TEXT,
                created_at TEXT
            )
            """
        )
        conn.execute(
            """
            CREATE TABLE twitch_raid_auth (
                twitch_login TEXT,
                twitch_user_id TEXT,
                raid_enabled INTEGER DEFAULT 0,
                authorized_at TEXT
            )
            """
        )
        conn.execute(
            """
            CREATE TABLE twitch_token_blacklist (
                twitch_user_id TEXT,
                error_count INTEGER DEFAULT 0
            )
            """
        )
        conn.execute(
            """
            CREATE TABLE twitch_raid_blacklist (
                target_login TEXT,
                reason TEXT
            )
            """
        )
        return conn

    def test_resolve_auth_state_reuses_auth_manager_for_linked_discord_user(self) -> None:
        conn = self._make_conn()
        conn.execute(
            """
            INSERT INTO twitch_streamers_partner_state
                (twitch_login, twitch_user_id, discord_user_id, manual_partner_opt_out, manual_verified_at, created_at)
            VALUES (?, ?, ?, ?, ?, ?)
            """,
            ("partner_one", "1001", "123", 0, "2026-03-07T10:00:00+00:00", "2026-03-07T09:00:00+00:00"),
        )
        auth_manager = _FakeAuthManager({"1001"})
        resolver = RaidIntegrationStateResolver(
            auth_manager=auth_manager,
            token_error_handler=_FakeTokenErrorHandler(),
        )

        with patch(
            "bot.raid.integration_state.readonly_connection",
            side_effect=lambda: contextlib.nullcontext(_CompatConn(conn)),
        ):
            state = resolver.resolve_auth_state("123")

        self.assertEqual(state.discord_user_id, "123")
        self.assertEqual(state.twitch_login, "partner_one")
        self.assertEqual(state.twitch_user_id, "1001")
        self.assertTrue(state.authorized)
        self.assertFalse(state.blocked)
        self.assertEqual(auth_manager.checked_user_ids, ["1001"])
        conn.close()

    def test_resolve_block_state_aggregates_opt_out_and_blacklists(self) -> None:
        conn = self._make_conn()
        conn.execute(
            """
            INSERT INTO twitch_streamers_partner_state
                (twitch_login, twitch_user_id, discord_user_id, manual_partner_opt_out, manual_verified_at, created_at)
            VALUES (?, ?, ?, ?, ?, ?)
            """,
            ("partner_two", "2002", "456", 1, "2026-03-07T10:00:00+00:00", "2026-03-07T09:00:00+00:00"),
        )
        conn.execute(
            "INSERT INTO twitch_raid_blacklist (target_login, reason) VALUES (?, ?)",
            ("partner_two", "manual block"),
        )
        token_error_handler = _FakeTokenErrorHandler({"2002"})
        resolver = RaidIntegrationStateResolver(token_error_handler=token_error_handler)

        with patch(
            "bot.raid.integration_state.readonly_connection",
            side_effect=lambda: contextlib.nullcontext(_CompatConn(conn)),
        ):
            state = resolver.resolve_block_state(discord_user_id="456")

        self.assertEqual(state.twitch_login, "partner_two")
        self.assertTrue(state.partner_opt_out)
        self.assertTrue(state.token_blacklisted)
        self.assertTrue(state.raid_blacklisted)
        self.assertTrue(state.blocked)
        self.assertEqual(token_error_handler.checked_user_ids, ["2002"])
        conn.close()

    def test_resolve_block_state_supports_login_without_discord_mapping(self) -> None:
        conn = self._make_conn()
        conn.execute(
            """
            INSERT INTO twitch_raid_auth (twitch_login, twitch_user_id, raid_enabled, authorized_at)
            VALUES (?, ?, ?, ?)
            """,
            ("solo_streamer", "3003", 1, "2026-03-07T10:00:00+00:00"),
        )
        resolver = RaidIntegrationStateResolver()

        with patch(
            "bot.raid.integration_state.readonly_connection",
            side_effect=lambda: contextlib.nullcontext(_CompatConn(conn)),
        ):
            state = resolver.resolve_block_state(twitch_login="solo_streamer")

        self.assertIsNone(state.discord_user_id)
        self.assertEqual(state.twitch_login, "solo_streamer")
        self.assertEqual(state.twitch_user_id, "3003")
        self.assertTrue(state.authorized)
        self.assertFalse(state.blocked)
        conn.close()

    def test_resolve_block_state_requires_identifier(self) -> None:
        resolver = RaidIntegrationStateResolver()

        with self.assertRaises(ValueError):
            resolver.resolve_block_state()


if __name__ == "__main__":
    unittest.main()
