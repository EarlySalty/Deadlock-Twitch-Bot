import time
import unittest
from types import SimpleNamespace
from urllib.parse import parse_qs, urlparse
from unittest.mock import patch

from aiohttp import web

from bot.dashboard.affiliate_mixin import _DashboardAffiliateMixin
from bot.dashboard.server_v2 import DashboardV2Server


class _ConnContext:
    def __init__(self, conn):
        self._conn = conn

    def __enter__(self):
        return self._conn

    def __exit__(self, exc_type, exc, tb):
        if exc_type is None:
            commit = getattr(self._conn, "commit", None)
            if callable(commit):
                commit()
        return False


class _RecordingConn:
    def __init__(self):
        self.executed: list[tuple[str, tuple]] = []
        self.commits = 0

    def execute(self, sql, params=None):
        self.executed.append((sql, tuple(params or ())))
        return None

    def commit(self):
        self.commits += 1


class _FakeResponse:
    def __init__(self, *, status: int, payload: dict):
        self.status = status
        self._payload = dict(payload)

    async def __aenter__(self):
        return self

    async def __aexit__(self, exc_type, exc, tb):
        return False

    async def json(self):
        return dict(self._payload)


class _RecordingClientSession:
    def __init__(self, response: _FakeResponse):
        self._response = response
        self.calls: list[tuple[str, dict]] = []

    async def __aenter__(self):
        return self

    async def __aexit__(self, exc_type, exc, tb):
        return False

    def post(self, url, data=None):
        self.calls.append((url, dict(data or {})))
        return self._response


class _AffiliateStripeHarness(_DashboardAffiliateMixin):
    def __init__(
        self,
        *,
        stripe_connect_client_id: str = "",
        loader_value: str = "",
        public_url: str = "https://twitch.earlysalty.com",
    ) -> None:
        self._stripe_connect_client_id = stripe_connect_client_id
        self._affiliate_connect_states = {}
        self._public_url = public_url
        self._loader_value = loader_value
        self.loader_calls: list[tuple[str, ...]] = []

    def _get_affiliate_session(self, _request):
        return {
            "twitch_login": "partner_one",
            "twitch_user_id": "1001",
            "display_name": "Partner One",
        }

    def _load_secret_value(self, *keys: str) -> str:
        self.loader_calls.append(tuple(keys))
        return self._loader_value


class _AffiliateStripeCallbackHarness(_DashboardAffiliateMixin):
    def __init__(
        self,
        *,
        redirect_uri: str,
        session_login: str = "partner_one",
        state_login: str = "partner_one",
    ) -> None:
        self._affiliate_connect_states = {
            "state-123": {
                "created_at": time.time(),
                "redirect_uri": redirect_uri,
                "twitch_login": state_login,
            }
        }
        self._session_login = session_login
        self.loader_calls: list[tuple[str, ...]] = []

    def _get_affiliate_session(self, _request):
        return {
            "twitch_login": self._session_login,
            "twitch_user_id": "1001",
            "display_name": "Partner One",
        }

    def _load_secret_value(self, *keys: str) -> str:
        self.loader_calls.append(tuple(keys))
        if keys == ("STRIPE_SECRET_KEY", "TWITCH_BILLING_STRIPE_SECRET_KEY"):
            return "sk_test_123"
        return ""


class AffiliateStripeConnectTests(unittest.IsolatedAsyncioTestCase):
    async def test_connect_redirect_uses_static_client_id_without_secret_lookup(self) -> None:
        handler = _AffiliateStripeHarness(stripe_connect_client_id="ca_code_123")

        with patch(
            "bot.dashboard.affiliate_mixin.secrets.token_urlsafe",
            return_value="state-123",
        ):
            with self.assertRaises(web.HTTPFound) as ctx:
                await handler._affiliate_connect_stripe(SimpleNamespace())

        params = parse_qs(urlparse(ctx.exception.location).query)
        self.assertEqual(params.get("client_id"), ["ca_code_123"])
        self.assertEqual(
            params.get("redirect_uri"),
            ["https://twitch.earlysalty.com/twitch/affiliate/connect/stripe/callback"],
        )
        self.assertEqual(params.get("state"), ["state-123"])
        self.assertEqual(handler.loader_calls, [])
        self.assertEqual(
            handler._affiliate_connect_states["state-123"]["redirect_uri"],
            "https://twitch.earlysalty.com/twitch/affiliate/connect/stripe/callback",
        )

    async def test_connect_redirect_falls_back_to_secret_loader_when_needed(self) -> None:
        handler = _AffiliateStripeHarness(loader_value="ca_loader_456")

        with patch(
            "bot.dashboard.affiliate_mixin.secrets.token_urlsafe",
            return_value="state-123",
        ):
            with self.assertRaises(web.HTTPFound) as ctx:
                await handler._affiliate_connect_stripe(SimpleNamespace())

        params = parse_qs(urlparse(ctx.exception.location).query)
        self.assertEqual(params.get("client_id"), ["ca_loader_456"])
        self.assertEqual(handler.loader_calls, [("STRIPE_CONNECT_CLIENT_ID",)])

    async def test_connect_callback_uses_same_redirect_uri_for_token_exchange(self) -> None:
        redirect_uri = "https://twitch.earlysalty.com/twitch/affiliate/connect/stripe/callback"
        handler = _AffiliateStripeCallbackHarness(redirect_uri=redirect_uri)
        handler._affiliate_ensure_tables = lambda _conn: None  # type: ignore[method-assign]
        conn = _RecordingConn()
        http_session = _RecordingClientSession(
            _FakeResponse(status=200, payload={"stripe_user_id": "acct_123"})
        )
        request = SimpleNamespace(query={"state": "state-123", "code": "oauth-code"})

        with patch(
            "bot.dashboard.affiliate_mixin.aiohttp.ClientSession",
            return_value=http_session,
        ):
            with patch(
                "bot.dashboard.affiliate_mixin.storage.transaction",
                return_value=_ConnContext(conn),
            ):
                with self.assertRaises(web.HTTPFound) as ctx:
                    await handler._affiliate_connect_stripe_callback(request)

        self.assertEqual(ctx.exception.location, "/twitch/affiliate/portal")
        self.assertEqual(len(http_session.calls), 1)
        _, payload = http_session.calls[0]
        self.assertEqual(payload.get("client_secret"), "sk_test_123")
        self.assertEqual(payload.get("code"), "oauth-code")
        self.assertEqual(payload.get("grant_type"), "authorization_code")
        self.assertEqual(payload.get("redirect_uri"), redirect_uri)
        self.assertEqual(
            handler.loader_calls,
            [("STRIPE_SECRET_KEY", "TWITCH_BILLING_STRIPE_SECRET_KEY")],
        )
        self.assertEqual(conn.commits, 1)

    async def test_connect_callback_rejects_when_session_login_does_not_match_state(self) -> None:
        redirect_uri = "https://twitch.earlysalty.com/twitch/affiliate/connect/stripe/callback"
        handler = _AffiliateStripeCallbackHarness(
            redirect_uri=redirect_uri,
            session_login="partner_two",
            state_login="partner_one",
        )
        request = SimpleNamespace(query={"state": "state-123", "code": "oauth-code"})

        with patch("bot.dashboard.affiliate_mixin.aiohttp.ClientSession") as client_session:
            with patch("bot.dashboard.affiliate_mixin.storage.transaction") as tx:
                response = await handler._affiliate_connect_stripe_callback(request)

        self.assertEqual(response.status, 403)
        self.assertEqual(
            response.text,
            "Affiliate-Session passt nicht zum Stripe Connect state.",
        )
        self.assertEqual(handler.loader_calls, [])
        client_session.assert_not_called()
        tx.assert_not_called()

    def test_dashboard_server_prefers_static_connect_client_id(self) -> None:
        with patch("bot.dashboard.server_v2.STATIC_STRIPE_CONNECT_CLIENT_ID", "ca_static_789"):
            with patch.object(DashboardV2Server, "_load_secret_value", return_value=""):
                handler = DashboardV2Server(
                    app_token=None,
                    noauth=False,
                    partner_token=None,
                    oauth_client_id="oauth-client-id",
                    oauth_client_secret="oauth-client-secret",
                    oauth_redirect_uri="https://twitch.earlysalty.com/twitch/auth/callback",
                )

        self.assertEqual(handler._stripe_connect_client_id, "ca_static_789")


if __name__ == "__main__":
    unittest.main()
