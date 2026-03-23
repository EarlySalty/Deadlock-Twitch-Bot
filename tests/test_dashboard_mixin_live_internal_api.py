import json
import unittest
from unittest.mock import patch

from bot.dashboard.mixin import TwitchDashboardMixin


class _FakeCursor:
    def __init__(self, rows=None) -> None:
        self._rows = list(rows or [])

    def fetchall(self):
        return list(self._rows)

    def fetchone(self):
        return self._rows[0] if self._rows else None


class _FakeConn:
    def __init__(self) -> None:
        self.executed: list[tuple[str, tuple]] = []

    def execute(self, sql: str, params=()):
        self.executed.append((sql, tuple(params)))
        if "FROM twitch_live_state" in sql:
            return _FakeCursor(
                [
                    {
                        "streamer_login": "partner_one",
                        "last_discord_message_id": "123",
                        "last_tracking_token": "deadbeef1234",
                    }
                ]
            )
        if "FROM twitch_live_announcement_configs" in sql:
            return _FakeCursor(
                [
                    {
                        "config_json": json.dumps(
                            {"button": {"label": "Jetzt reinsehen"}}
                        )
                    }
                ]
            )
        return _FakeCursor()


class _FakeConnCtx:
    def __init__(self, conn: _FakeConn) -> None:
        self._conn = conn

    def __enter__(self) -> _FakeConn:
        return self._conn

    def __exit__(self, exc_type, exc, tb) -> bool:
        return False


class _DummyDashboardMixin(TwitchDashboardMixin):
    def __init__(self) -> None:
        self._notify_channel_id = 789

    @staticmethod
    def _normalize_login(raw: str) -> str | None:
        value = str(raw or "").strip().lower()
        return value or None


class DashboardMixinLiveInternalApiTests(unittest.IsolatedAsyncioTestCase):
    async def test_dashboard_live_active_announcements_builds_expected_payload(self) -> None:
        handler = _DummyDashboardMixin()
        fake_conn = _FakeConn()

        with patch("bot.dashboard.mixin.storage.readonly_connection", return_value=_FakeConnCtx(fake_conn)):
            payload = await handler._dashboard_live_active_announcements()

        self.assertEqual(
            payload,
            [
                {
                    "streamer_login": "partner_one",
                    "message_id": 123,
                    "tracking_token": "deadbeef1234",
                    "referral_url": "https://www.twitch.tv/partner_one?ref=DE-Deadlock-Discord",
                    "button_label": "Jetzt reinsehen",
                    "channel_id": 789,
                }
            ],
        )

    async def test_dashboard_live_link_click_persists_expected_columns(self) -> None:
        handler = _DummyDashboardMixin()
        fake_conn = _FakeConn()

        with patch("bot.dashboard.mixin.storage.transaction", return_value=_FakeConnCtx(fake_conn)):
            payload = await handler._dashboard_live_link_click(
                streamer_login="partner_one",
                tracking_token="deadbeef1234",
                discord_user_id="12345",
                discord_username="Viewer One",
                guild_id="111",
                channel_id="222",
                message_id="333",
                source_hint="discord_button",
            )

        self.assertEqual(payload, {"ok": True})
        insert_statements = [
            params
            for sql, params in fake_conn.executed
            if "INSERT INTO twitch_link_clicks" in sql
        ]
        self.assertEqual(len(insert_statements), 1)
        inserted = insert_statements[0]
        self.assertEqual(inserted[1], "partner_one")
        self.assertEqual(inserted[2], "deadbeef1234")
        self.assertEqual(inserted[3], "12345")
        self.assertEqual(inserted[4], "Viewer One")
        self.assertEqual(inserted[5], "111")
        self.assertEqual(inserted[6], "222")
        self.assertEqual(inserted[7], "333")
        self.assertEqual(inserted[8], "DE-Deadlock-Discord")
        self.assertEqual(inserted[9], "discord_button")


if __name__ == "__main__":
    unittest.main()
