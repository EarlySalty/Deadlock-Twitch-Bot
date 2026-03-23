import time
import unittest
from types import SimpleNamespace
from unittest.mock import patch

from aiohttp import web

from bot.dashboard.auth.auth_mixin import _DashboardAuthMixin
from bot.dashboard.auth.state_store import DashboardAuthRateLimitStore
from bot.dashboard.server_v2 import DashboardV2Server


def _make_request(
    *,
    query: dict | None = None,
    cookies: dict | None = None,
    path_qs: str = "/twitch/dashboard",
    secure: bool = True,
) -> SimpleNamespace:
    return SimpleNamespace(
        query=query or {},
        cookies=cookies or {},
        rel_url=SimpleNamespace(path_qs=path_qs),
        headers={},
        secure=secure,
        remote="127.0.0.1",
        transport=None,
    )


class _PersistentAuthHarness(_DashboardAuthMixin):
    _normalize_discord_admin_next_path = staticmethod(
        DashboardV2Server._normalize_discord_admin_next_path
    )
    _canonical_discord_admin_post_login_path = staticmethod(
        DashboardV2Server._canonical_discord_admin_post_login_path
    )

    def __init__(self) -> None:
        self._oauth_client_id = "client-id"
        self._oauth_client_secret = "client-secret"
        self._oauth_redirect_uri = "https://dashboard.example/twitch/auth/callback"
        self._session_cookie_name = "twitch_dash_session"
        self._oauth_states = {}
        self._auth_sessions = {}
        self._oauth_state_ttl_seconds = 600
        self._session_ttl_seconds = 6 * 3600
        self._sessions_db_loaded = False
        self._discord_admin_enabled = True
        self._discord_admin_required = True
        self._discord_admin_client_id = "discord-client-id"
        self._discord_admin_client_secret = "discord-client-secret"
        self._discord_admin_redirect_uri = "https://dashboard.example/twitch/auth/discord/callback"
        self._discord_admin_cookie_name = "twitch_admin_session"
        self._discord_admin_session_ttl = 6 * 3600
        self._discord_admin_state_ttl = 600
        self._discord_admin_oauth_states = {}
        self._discord_admin_sessions = {}
        self._discord_sessions_db_loaded = False
        self._rate_limits = {}
        self.created_sessions = []
        self.exchange_calls = []

    def _check_v2_auth(self, request) -> bool:
        del request
        return False

    def _is_secure_request(self, request) -> bool:
        return bool(getattr(request, "secure", False))

    def _sanitize_log_value(self, value):
        return str(value or "")

    def _peer_host(self, request):
        return str(getattr(request, "remote", "") or "")

    def _rate_limit_key(self, request) -> str:
        del request
        return "client-1"

    async def _exchange_code_for_user(self, code: str, redirect_uri: str):
        self.exchange_calls.append((code, redirect_uri))
        return {
            "twitch_login": "partner_one",
            "twitch_user_id": "1001",
            "display_name": "Partner One",
        }

    def _is_partner_allowed(self, *, twitch_login: str, twitch_user_id: str):
        del twitch_login, twitch_user_id
        return {"twitch_login": "partner_one", "twitch_user_id": "1001"}

    def _create_dashboard_session(
        self, *, twitch_login: str, twitch_user_id: str, display_name: str
    ) -> str:
        self.created_sessions.append(
            {
                "twitch_login": twitch_login,
                "twitch_user_id": twitch_user_id,
                "display_name": display_name,
            }
        )
        return "session-123"


class DashboardAuthPersistenceTests(unittest.IsolatedAsyncioTestCase):
    async def test_auth_callback_uses_persisted_state_when_process_cache_is_empty(self) -> None:
        handler = _PersistentAuthHarness()
        context_token = "ctx_token_abcdefghijklmnop"
        state = "state_token_abcdefghijklmnop"
        request = _make_request(
            query={"state": state, "code": "oauth-code"},
            cookies={handler._oauth_context_cookie_name(): context_token},
        )
        persisted_state = {
            "created_at": time.time(),
            "next_path": "/twitch/dashboard",
            "redirect_uri": "https://dashboard.example/twitch/auth/callback",
            "context_token": context_token,
        }

        with patch.object(
            DashboardAuthRateLimitStore,
            "allow_request",
            return_value=True,
        ), patch(
            "bot.dashboard.auth.state_store.sessions_db.pop_session",
            return_value=persisted_state,
        ) as pop_session:
            with self.assertRaises(web.HTTPFound) as ctx:
                await handler.auth_callback(request)

        self.assertEqual(ctx.exception.location, "/twitch/dashboard")
        self.assertEqual(
            handler.exchange_calls,
            [("oauth-code", "https://dashboard.example/twitch/auth/callback")],
        )
        self.assertEqual(len(handler.created_sessions), 1)
        pop_session.assert_called_once()

    def test_dashboard_session_lookup_loads_single_persisted_session(self) -> None:
        handler = _PersistentAuthHarness()
        request = _make_request(cookies={handler._session_cookie_name: "session-abc"})
        stored_session = {
            "twitch_login": "partner_one",
            "twitch_user_id": "1001",
            "display_name": "Partner One",
            "created_at": time.time() - 60,
            "expires_at": time.time() + 3600,
        }

        with patch(
            "bot.dashboard.auth.state_store.sessions_db.load_session",
            return_value=stored_session,
        ) as load_session, patch(
            "bot.dashboard.auth.state_store.sessions_db.delete_expired_sessions"
        ):
            session = handler._get_dashboard_auth_session(request)

        self.assertEqual(session, stored_session)
        self.assertIn("session-abc", handler._auth_sessions)
        load_session.assert_called_once()

    def test_rate_limit_store_counts_persisted_hits(self) -> None:
        store = DashboardAuthRateLimitStore()
        owner = SimpleNamespace(_rate_limits={})

        with patch(
            "bot.dashboard.auth.state_store.sessions_db.reserve_rate_limit_slot",
            side_effect=[True, False],
        ) as reserve_slot:
            allowed_first = store.allow_request(
                owner=owner,
                key="client-1",
                max_requests=1,
                window_seconds=60.0,
                now=1000.0,
            )
            allowed_second = store.allow_request(
                owner=owner,
                key="client-1",
                max_requests=1,
                window_seconds=60.0,
                now=1001.0,
            )

        self.assertTrue(allowed_first)
        self.assertFalse(allowed_second)
        self.assertEqual(reserve_slot.call_count, 2)


if __name__ == "__main__":
    unittest.main()
