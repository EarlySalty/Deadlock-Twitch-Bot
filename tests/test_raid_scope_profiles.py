from __future__ import annotations

import contextlib
import sqlite3
import unittest
from urllib.parse import parse_qs, urlparse
from unittest.mock import patch

from bot.raid.auth import RaidAuthManager
from tests.sqlite_twitch_schema import ensure_sqlite_twitch_schema


class RaidScopeProfileTests(unittest.TestCase):
    def setUp(self) -> None:
        self.conn = sqlite3.connect(":memory:")
        self.conn.row_factory = sqlite3.Row
        ensure_sqlite_twitch_schema(self.conn)

    def tearDown(self) -> None:
        self.conn.close()

    def _manager(self) -> RaidAuthManager:
        return RaidAuthManager(
            client_id="client-id",
            client_secret="client-secret",
            redirect_uri="https://example.com/twitch/auth/callback",
        )

    @staticmethod
    def _extract_scope_set(auth_url: str) -> set[str]:
        parsed = urlparse(auth_url)
        raw_scope = parse_qs(parsed.query).get("scope", [""])[0]
        return {scope.strip() for scope in raw_scope.split() if scope.strip()}

    def test_generate_auth_url_uses_base_scopes_for_fresh_login(self) -> None:
        manager = self._manager()

        with patch(
            "bot.raid.auth.get_conn",
            side_effect=lambda: contextlib.nullcontext(self.conn),
        ):
            auth_url = manager.generate_auth_url("freshstreamer")

        scopes = self._extract_scope_set(auth_url)
        self.assertIn("channel:manage:raids", scopes)
        self.assertNotIn("channel:read:subscriptions", scopes)
        self.assertNotIn("channel:read:hype_train", scopes)

    def test_generate_auth_url_upgrades_existing_streamer_login(self) -> None:
        self.conn.execute(
            """
            INSERT INTO twitch_raid_auth (twitch_user_id, twitch_login, scopes, raid_enabled)
            VALUES (?, ?, ?, ?)
            """,
            ("9009", "knownstreamer", "channel:manage:raids", 1),
        )
        self.conn.commit()
        manager = self._manager()

        with patch(
            "bot.raid.auth.get_conn",
            side_effect=lambda: contextlib.nullcontext(self.conn),
        ):
            auth_url = manager.generate_auth_url("knownstreamer")

        scopes = self._extract_scope_set(auth_url)
        self.assertIn("channel:read:subscriptions", scopes)
        self.assertIn("channel:read:hype_train", scopes)

    def test_generate_auth_url_upgrades_existing_discord_link(self) -> None:
        self.conn.execute(
            """
            INSERT INTO twitch_streamer_identities (twitch_user_id, twitch_login, discord_user_id)
            VALUES (?, ?, ?)
            """,
            ("9009", "knownstreamer", "42"),
        )
        self.conn.execute(
            """
            INSERT INTO twitch_raid_auth (twitch_user_id, twitch_login, scopes, raid_enabled)
            VALUES (?, ?, ?, ?)
            """,
            ("9009", "knownstreamer", "channel:manage:raids", 1),
        )
        self.conn.commit()
        manager = self._manager()

        with patch(
            "bot.raid.auth.get_conn",
            side_effect=lambda: contextlib.nullcontext(self.conn),
        ):
            auth_url = manager.generate_auth_url("discord:42")

        scopes = self._extract_scope_set(auth_url)
        self.assertIn("channel:read:subscriptions", scopes)
        self.assertIn("channel:read:hype_train", scopes)


if __name__ == "__main__":
    unittest.main()
