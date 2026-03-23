import sqlite3
import unittest
from types import SimpleNamespace
from unittest.mock import patch

from bot.raid.auth import RAID_SCOPES
from bot.raid.commands import RaidCommandsMixin


class _ConnContext:
    def __init__(self, conn: sqlite3.Connection):
        self._conn = _CompatConn(conn)

    def __enter__(self) -> sqlite3.Connection:
        return self._conn

    def __exit__(self, exc_type, exc, tb) -> bool:
        return False


class _CompatConn:
    def __init__(self, conn: sqlite3.Connection) -> None:
        self._conn = conn

    def execute(self, sql: str, params=()):
        return self._conn.execute(str(sql).replace("%s", "?"), params)

    def __getattr__(self, name: str):
        return getattr(self._conn, name)


class _DummyCtx:
    def __init__(self, author_id: int = 42):
        self.author = SimpleNamespace(id=author_id)
        self.sent_messages: list[dict] = []

    async def send(self, content: str | None = None, **kwargs):
        payload = {"content": content}
        payload.update(kwargs)
        self.sent_messages.append(payload)


class _DummyRaidCommands(RaidCommandsMixin):
    pass


class CheckAuthCommandTests(unittest.IsolatedAsyncioTestCase):
    def _setup_conn(self) -> sqlite3.Connection:
        conn = sqlite3.connect(":memory:")
        conn.execute(
            """
            CREATE TABLE twitch_streamers (
                discord_user_id TEXT,
                twitch_login TEXT,
                twitch_user_id TEXT
            )
            """
        )
        conn.execute(
            """
            CREATE TABLE twitch_raid_auth (
                twitch_user_id TEXT,
                scopes TEXT,
                needs_reauth INTEGER
            )
            """
        )
        conn.commit()
        return conn

    async def test_check_auth_reports_success_when_all_scopes_present(self) -> None:
        conn = self._setup_conn()
        try:
            conn.execute(
                "INSERT INTO twitch_streamers (discord_user_id, twitch_login, twitch_user_id) VALUES (?, ?, ?)",
                ("42", "alpha", "uid_1"),
            )
            conn.execute(
                "INSERT INTO twitch_raid_auth (twitch_user_id, scopes, needs_reauth) VALUES (?, ?, ?)",
                ("uid_1", " ".join(RAID_SCOPES), 0),
            )
            conn.commit()

            cog = _DummyRaidCommands()
            ctx = _DummyCtx(author_id=42)
            with (
                patch("bot.raid.commands.readonly_connection", return_value=_ConnContext(conn)),
                patch(
                    "bot.raid.commands.load_partner_by_discord_user_id",
                    return_value={"twitch_login": "alpha", "twitch_user_id": "uid_1"},
                ),
            ):
                await _DummyRaidCommands.cmd_check_auth.callback(cog, ctx)

            self.assertEqual(len(ctx.sent_messages), 1)
            message = ctx.sent_messages[0]
            self.assertTrue(message.get("ephemeral"))
            self.assertIn("Alle Scopes vorhanden", str(message.get("content") or ""))
        finally:
            conn.close()

    async def test_check_auth_lists_missing_scopes_and_sets_reauth_note(self) -> None:
        conn = self._setup_conn()
        try:
            missing_scope = RAID_SCOPES[-1]
            conn.execute(
                "INSERT INTO twitch_streamers (discord_user_id, twitch_login, twitch_user_id) VALUES (?, ?, ?)",
                ("42", "alpha", "uid_1"),
            )
            conn.execute(
                "INSERT INTO twitch_raid_auth (twitch_user_id, scopes, needs_reauth) VALUES (?, ?, ?)",
                ("uid_1", " ".join(RAID_SCOPES[:-1]), 1),
            )
            conn.commit()

            cog = _DummyRaidCommands()
            cog._raid_bot = SimpleNamespace(auth_manager=SimpleNamespace())
            ctx = _DummyCtx(author_id=42)
            with (
                patch("bot.raid.commands.readonly_connection", return_value=_ConnContext(conn)),
                patch(
                    "bot.raid.commands.load_partner_by_discord_user_id",
                    return_value={"twitch_login": "alpha", "twitch_user_id": "uid_1"},
                ),
            ):
                await _DummyRaidCommands.cmd_check_auth.callback(cog, ctx)

            self.assertEqual(len(ctx.sent_messages), 1)
            message = ctx.sent_messages[0]
            content = str(message.get("content") or "")
            self.assertTrue(message.get("ephemeral"))
            self.assertIn("Fehlende Scopes", content)
            self.assertIn(missing_scope, content)
            self.assertIn("needs_reauth=1", content)
            self.assertIsNotNone(message.get("view"))
        finally:
            conn.close()

    async def test_check_auth_without_saved_auth_and_without_raid_bot(self) -> None:
        conn = self._setup_conn()
        try:
            conn.execute(
                "INSERT INTO twitch_streamers (discord_user_id, twitch_login, twitch_user_id) VALUES (?, ?, ?)",
                ("42", "alpha", "uid_1"),
            )
            conn.commit()

            cog = _DummyRaidCommands()
            cog._raid_bot = None
            ctx = _DummyCtx(author_id=42)
            with (
                patch("bot.raid.commands.readonly_connection", return_value=_ConnContext(conn)),
                patch(
                    "bot.raid.commands.load_partner_by_discord_user_id",
                    return_value={"twitch_login": "alpha", "twitch_user_id": "uid_1"},
                ),
            ):
                await _DummyRaidCommands.cmd_check_auth.callback(cog, ctx)

            self.assertEqual(len(ctx.sent_messages), 1)
            message = ctx.sent_messages[0]
            self.assertTrue(message.get("ephemeral"))
            self.assertIn("Keine Twitch-Autorisierung gefunden", str(message.get("content") or ""))
        finally:
            conn.close()


if __name__ == "__main__":
    unittest.main()
