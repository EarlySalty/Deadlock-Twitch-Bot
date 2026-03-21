import contextlib
import json
import unittest
from unittest.mock import patch

from bot.raid.auth import RaidAuthManager


class _FakeCursor:
    def __init__(self, row=None, rowcount: int = 0):
        self._row = row
        self.rowcount = rowcount

    def fetchone(self):
        return self._row


class _FakeConn:
    def __init__(self, rows_by_fragment: dict[str, object] | None = None) -> None:
        self.rows_by_fragment = rows_by_fragment or {}
        self.calls: list[tuple[str, tuple]] = []

    def execute(self, sql: str, params=()):
        params_tuple = tuple(params or ())
        self.calls.append((sql, params_tuple))
        for fragment, row in self.rows_by_fragment.items():
            if fragment in sql:
                return _FakeCursor(row=row, rowcount=1 if row is not None else 0)
        return _FakeCursor(row=None, rowcount=0)


class RaidAuthStatePersistenceTests(unittest.TestCase):
    def test_generate_auth_url_persists_state_token_in_db(self) -> None:
        fake_conn = _FakeConn()

        manager = RaidAuthManager(
            client_id="cid",
            client_secret="secret",
            redirect_uri="https://raid.earlysalty.com/twitch/raid/callback",
        )

        with (
            patch("bot.raid.auth.secrets.token_urlsafe", return_value="state-123"),
            patch("bot.raid.auth.time.time", return_value=1700000000.0),
            patch(
                "bot.raid.auth.get_conn",
                side_effect=lambda: contextlib.nullcontext(fake_conn),
            ),
        ):
            auth_url = manager.generate_auth_url(
                "partner_one",
                expected_twitch_login="partner_one",
                expected_twitch_user_id="1001",
                discord_user_id="123456789",
            )

        self.assertIn("state=state-123", auth_url)
        insert_calls = [call for call in fake_conn.calls if "INSERT INTO oauth_state_tokens" in call[0]]
        self.assertEqual(len(insert_calls), 1)
        _, params = insert_calls[0]
        self.assertEqual(params[0], "state-123")
        self.assertEqual(params[1], "twitch_raid")
        self.assertEqual(params[2], "partner_one")
        meta = json.loads(params[4])
        self.assertEqual(meta["scope_profile"], "base")
        self.assertEqual(meta["expected_twitch_login"], "partner_one")
        self.assertEqual(meta["expected_twitch_user_id"], "1001")
        self.assertEqual(meta["discord_user_id"], "123456789")

    def test_get_pending_auth_url_rebuilds_from_persisted_state(self) -> None:
        fake_conn = _FakeConn(
            rows_by_fragment={
                "SELECT streamer_login": {
                    "streamer_login": "partner_one",
                    "pkce_verifier": json.dumps(
                        {
                            "scope_profile": "dashboard_reauth",
                            "expected_twitch_login": "partner_one",
                            "expected_twitch_user_id": "1001",
                            "discord_user_id": "42",
                        }
                    ),
                }
            }
        )
        manager = RaidAuthManager(
            client_id="cid",
            client_secret="secret",
            redirect_uri="https://raid.earlysalty.com/twitch/raid/callback",
        )

        with patch(
            "bot.raid.auth.get_conn",
            side_effect=lambda: contextlib.nullcontext(fake_conn),
        ):
            full_url = manager.get_pending_auth_url("state-xyz")

        assert full_url is not None
        self.assertIn("id.twitch.tv/oauth2/authorize", full_url)
        self.assertIn("state=state-xyz", full_url)
        self.assertIn("force_verify=true", full_url)

    def test_verify_state_consumes_state_from_db(self) -> None:
        fake_conn = _FakeConn(
            rows_by_fragment={
                "DELETE FROM oauth_state_tokens": {
                    "streamer_login": "partner_one",
                    "pkce_verifier": json.dumps(
                        {
                            "scope_profile": "base",
                            "expected_twitch_login": "partner_one",
                            "expected_twitch_user_id": "1001",
                            "discord_user_id": "777",
                        }
                    ),
                },
            }
        )
        manager = RaidAuthManager(
            client_id="cid",
            client_secret="secret",
            redirect_uri="https://raid.earlysalty.com/twitch/raid/callback",
        )

        with patch(
            "bot.raid.auth.get_conn",
            side_effect=lambda: contextlib.nullcontext(fake_conn),
        ):
            login = manager.verify_state("state-consume")

        self.assertEqual(login, "partner_one")
        delete_calls = [call for call in fake_conn.calls if "DELETE FROM oauth_state_tokens" in call[0]]
        self.assertEqual(len(delete_calls), 1)

    def test_consume_state_details_returns_bound_discord_user_id(self) -> None:
        fake_conn = _FakeConn(
            rows_by_fragment={
                "DELETE FROM oauth_state_tokens": {
                    "streamer_login": "partner_one",
                    "pkce_verifier": json.dumps(
                        {
                            "scope_profile": "dashboard_reauth",
                            "expected_twitch_login": "partner_one",
                            "expected_twitch_user_id": "1001",
                            "discord_user_id": "123456789",
                        }
                    ),
                },
            }
        )
        manager = RaidAuthManager(
            client_id="cid",
            client_secret="secret",
            redirect_uri="https://raid.earlysalty.com/twitch/raid/callback",
        )

        with patch(
            "bot.raid.auth.get_conn",
            side_effect=lambda: contextlib.nullcontext(fake_conn),
        ):
            state = manager.consume_state_details("state-consume")

        assert state is not None
        self.assertEqual(state.requested_login, "partner_one")
        self.assertEqual(state.expected_twitch_login, "partner_one")
        self.assertEqual(state.expected_twitch_user_id, "1001")
        self.assertEqual(state.discord_user_id, "123456789")
        self.assertEqual(state.scope_profile, "dashboard_reauth")


class RaidAuthReauthFlagTests(unittest.IsolatedAsyncioTestCase):
    async def test_snapshot_and_flag_reauth_marks_existing_grants_without_plaintext_filter(self) -> None:
        fake_conn = _FakeConn(rows_by_fragment={"SELECT changes()": (4,)})
        manager = RaidAuthManager(
            client_id="cid",
            client_secret="secret",
            redirect_uri="https://raid.earlysalty.com/twitch/raid/callback",
        )

        with patch(
            "bot.raid.auth.get_conn",
            side_effect=lambda: contextlib.nullcontext(fake_conn),
        ):
            changed = await manager.snapshot_and_flag_reauth()

        self.assertEqual(changed, 4)
        update_sql = next(
            sql for sql, _params in fake_conn.calls if "UPDATE twitch_raid_auth" in sql
        )
        self.assertIn("needs_reauth IS NOT TRUE", update_sql)
        self.assertIn("authorized_at IS NOT NULL", update_sql)
        self.assertNotIn("access_token <> 'ENC'", update_sql)


if __name__ == "__main__":
    unittest.main()
