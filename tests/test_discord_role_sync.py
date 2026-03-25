import unittest
from unittest.mock import ANY, AsyncMock, patch

from bot.community.admin import TwitchAdminMixin
from bot.discord_role_sync import sync_streamer_role


class _FakeRole:
    def __init__(self, role_id: int) -> None:
        self.id = role_id


class _FakeMember:
    def __init__(self, user_id: int, roles: list[_FakeRole] | None = None) -> None:
        self.id = user_id
        self.roles = list(roles or [])
        self.add_calls: list[tuple[int, str | None]] = []
        self.remove_calls: list[tuple[int, str | None]] = []

    async def add_roles(self, role: _FakeRole, *, reason: str | None = None) -> None:
        self.add_calls.append((role.id, reason))
        if role not in self.roles:
            self.roles.append(role)

    async def remove_roles(self, role: _FakeRole, *, reason: str | None = None) -> None:
        self.remove_calls.append((role.id, reason))
        self.roles = [item for item in self.roles if item is not role]


class _FakeGuild:
    def __init__(self, guild_id: int, role: _FakeRole, member: _FakeMember) -> None:
        self.id = guild_id
        self._role = role
        self._member = member

    def get_role(self, role_id: int) -> _FakeRole | None:
        return self._role if self._role.id == role_id else None

    def get_member(self, user_id: int) -> _FakeMember | None:
        return self._member if self._member.id == user_id else None

    async def fetch_member(self, user_id: int) -> _FakeMember | None:
        return self.get_member(user_id)


class _FakeDiscordBot:
    def __init__(self, guilds: list[_FakeGuild]) -> None:
        self.guilds = guilds
        self._guilds_by_id = {guild.id: guild for guild in guilds}

    def get_guild(self, guild_id: int) -> _FakeGuild | None:
        return self._guilds_by_id.get(guild_id)


class _DummyAdmin(TwitchAdminMixin):
    def __init__(self) -> None:
        self.bot = object()

    def _normalize_login(self, login: str) -> str:
        return str(login or "").strip().lower()


class DiscordRoleSyncTests(unittest.IsolatedAsyncioTestCase):
    async def test_sync_streamer_role_local_mode_grants_role(self) -> None:
        role = _FakeRole(42)
        member = _FakeMember(123)
        guild = _FakeGuild(100, role, member)
        bot = _FakeDiscordBot([guild])

        with patch.dict(
            "os.environ",
            {
                "TWITCH_DISCORD_ROLE_SYNC_MODE": "local",
                "STREAMER_ROLE_ID": "42",
                "STREAMER_GUILD_ID": "100",
            },
            clear=False,
        ):
            changed = await sync_streamer_role(
                bot,
                "123",
                should_have_role=True,
                reason="oauth success",
            )

        self.assertTrue(changed)
        self.assertEqual(member.add_calls, [(42, "oauth success")])
        self.assertIn(role, member.roles)

    async def test_sync_streamer_role_external_mode_skips_local_changes(self) -> None:
        role = _FakeRole(42)
        member = _FakeMember(123)
        guild = _FakeGuild(100, role, member)
        bot = _FakeDiscordBot([guild])

        with patch.dict(
            "os.environ",
            {
                "TWITCH_DISCORD_ROLE_SYNC_MODE": "external",
                "STREAMER_ROLE_ID": "42",
                "STREAMER_GUILD_ID": "100",
            },
            clear=False,
        ):
            changed = await sync_streamer_role(
                bot,
                "123",
                should_have_role=True,
                reason="oauth success",
            )

        self.assertFalse(changed)
        self.assertEqual(member.add_calls, [])
        self.assertEqual(member.remove_calls, [])
        self.assertEqual(member.roles, [])

    async def test_admin_remove_departnered_streamer_removes_role(self) -> None:
        handler = _DummyAdmin()

        class _Txn:
            def __enter__(self):
                return self

            def __exit__(self, exc_type, exc, tb):
                del exc_type, exc, tb
                return False

            def execute(self, query, params):
                del query, params
                return None

        with patch("bot.community.admin.storage.transaction", return_value=_Txn()), patch(
            "bot.community.admin.storage.departner_active_partner",
            return_value={"discord_user_id": "123"},
        ), patch(
            "bot.community.admin.sync_streamer_role",
            new=AsyncMock(return_value=True),
        ) as mocked_sync:
            result = await handler._cmd_remove("Alpha")

        self.assertEqual(result, "alpha operativ deaktiviert (Streamer-Rolle entfernt)")
        mocked_sync.assert_awaited_once_with(
            handler.bot,
            "123",
            should_have_role=False,
            reason="Streamer als Partner deaktiviert",
            logger=ANY,
        )


if __name__ == "__main__":
    unittest.main()
