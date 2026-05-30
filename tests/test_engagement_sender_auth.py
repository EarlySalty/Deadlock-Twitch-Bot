import asyncio
import contextlib
import time
import unittest
import urllib.parse
from unittest import mock

from bot.engagement import sender_auth


class _FakeCrypto:
    """Reversibler Fake: encrypt -> b'enc:'+text, decrypt -> strip prefix."""

    def encrypt_field(self, plaintext, aad, kid="v1"):
        return ("enc:" + plaintext).encode("utf-8")

    def decrypt_field(self, blob, aad):
        return bytes(blob).decode("utf-8").removeprefix("enc:")


class _FakeConn:
    def __init__(self, captured, returning_row=None):
        self._captured = captured
        self._returning_row = returning_row
        self._last_row = None

    def execute(self, sql, params=None):
        self._captured.append((sql, params))
        self._last_row = self._returning_row
        return self

    def fetchone(self):
        return self._last_row


def _fake_transaction_factory(captured, returning_row=None):
    @contextlib.contextmanager
    def _fake_transaction():
        yield _FakeConn(captured, returning_row)
    return _fake_transaction


class BuildAuthorizeUrlTests(unittest.TestCase):
    def setUp(self):
        self._env = mock.patch.dict(
            "os.environ",
            {"TWITCH_CLIENT_ID": "cid_test", "TWITCH_CLIENT_SECRET": "sec_test"},
        )
        self._env.start()
        self.addCleanup(self._env.stop)

    def test_url_contains_scopes_redirect_and_persists_state(self):
        captured = []
        with mock.patch.object(sender_auth, "ensure_table", lambda: None), \
             mock.patch.object(sender_auth, "transaction", _fake_transaction_factory(captured)):
            url = sender_auth.build_authorize_url()

        parsed = urllib.parse.urlparse(url)
        qs = urllib.parse.parse_qs(parsed.query)
        self.assertEqual(qs["client_id"][0], "cid_test")
        self.assertEqual(qs["redirect_uri"][0], sender_auth.REDIRECT_URI)
        self.assertEqual(qs["scope"][0], "user:write:chat user:bot")
        self.assertTrue(qs["state"][0].startswith("engsender-"))
        # State wurde in oauth_state_tokens geschrieben mit platform=engagement_sender
        joined = " ".join(sql for sql, _ in captured)
        self.assertIn("oauth_state_tokens", joined)
        inserted_params = [p for sql, p in captured if "oauth_state_tokens" in sql][0]
        self.assertIn(sender_auth.PLATFORM, inserted_params)


class ConsumeStateTests(unittest.TestCase):
    def test_unknown_state_returns_false(self):
        captured = []
        with mock.patch.object(sender_auth, "transaction", _fake_transaction_factory(captured, returning_row=None)):
            self.assertFalse(sender_auth._consume_state("nope"))

    def test_valid_future_state_returns_true(self):
        captured = []
        future = "2999-01-01T00:00:00+00:00"
        with mock.patch.object(sender_auth, "transaction", _fake_transaction_factory(captured, returning_row=(future,))):
            self.assertTrue(sender_auth._consume_state("engsender-x"))

    def test_expired_state_returns_false(self):
        captured = []
        past = "2000-01-01T00:00:00+00:00"
        with mock.patch.object(sender_auth, "transaction", _fake_transaction_factory(captured, returning_row=(past,))):
            self.assertFalse(sender_auth._consume_state("engsender-x"))

    def test_empty_state_returns_false(self):
        self.assertFalse(sender_auth._consume_state(""))


class GetValidAccessTokenTests(unittest.TestCase):
    def setUp(self):
        self._env = mock.patch.dict(
            "os.environ",
            {"TWITCH_CLIENT_ID": "cid_test", "TWITCH_CLIENT_SECRET": "sec_test"},
        )
        self._env.start()
        self.addCleanup(self._env.stop)
        self._crypto = mock.patch.object(sender_auth, "get_crypto", return_value=_FakeCrypto())
        self._crypto.start()
        self.addCleanup(self._crypto.stop)

    def test_returns_none_when_no_account(self):
        with mock.patch.object(sender_auth, "_load_row", return_value=None):
            self.assertIsNone(asyncio.run(sender_auth.get_valid_access_token()))

    def test_returns_cached_token_when_not_expired(self):
        row = {
            "user_id": "uid1",
            "login": "smoke",
            "access_enc": b"enc:ACCESS_OK",
            "refresh_enc": b"enc:REFRESH_OK",
            "scopes": "user:write:chat user:bot",
            "expires_at": int(time.time()) + 3600,
        }
        with mock.patch.object(sender_auth, "_load_row", return_value=row):
            result = asyncio.run(sender_auth.get_valid_access_token())
        self.assertEqual(result, ("ACCESS_OK", "uid1"))

    def test_refreshes_when_expired(self):
        row = {
            "user_id": "uid1",
            "login": "smoke",
            "access_enc": b"enc:OLD",
            "refresh_enc": b"enc:REFRESH_OK",
            "scopes": "user:write:chat user:bot",
            "expires_at": int(time.time()) - 10,  # abgelaufen
        }
        stored = {}

        def _capture_store(**kwargs):
            stored.update(kwargs)

        token_resp = {
            "access_token": "NEW_ACCESS",
            "refresh_token": "NEW_REFRESH",
            "expires_in": 3600,
            "scope": ["user:write:chat", "user:bot"],
        }
        with mock.patch.object(sender_auth, "_load_row", return_value=row), \
             mock.patch.object(sender_auth, "_post_token", new=mock.AsyncMock(return_value=token_resp)), \
             mock.patch.object(sender_auth, "_store_tokens", side_effect=_capture_store):
            result = asyncio.run(sender_auth.get_valid_access_token())
        self.assertEqual(result, ("NEW_ACCESS", "uid1"))
        self.assertEqual(stored.get("access_token"), "NEW_ACCESS")
        self.assertEqual(stored.get("refresh_token"), "NEW_REFRESH")


if __name__ == "__main__":
    unittest.main()
