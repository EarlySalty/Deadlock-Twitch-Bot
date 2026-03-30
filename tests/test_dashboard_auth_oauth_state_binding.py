import time
import unittest
from types import SimpleNamespace
from unittest.mock import patch

from aiohttp import web

from bot.dashboard.auth_mixin import _DashboardAuthMixin
from bot.dashboard.server_v2 import DashboardV2Server

_MALICIOUS_NEXT_VARIANTS = (
    "//evil.example",
    "%2F%2Fevil.example",
    "http://evil.example/path",
    "\\\\evil.example",
)


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


class _AuthHarness(_DashboardAuthMixin):
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
        self._oauth_states: dict[str, dict] = {}
        self._auth_sessions: dict[str, dict] = {}
        self._oauth_state_ttl_seconds = 600
        self._session_ttl_seconds = 6 * 3600
        self._sessions_db_loaded = True
        self._discord_admin_enabled = True
        self._discord_admin_required = True
        self._discord_admin_client_id = "discord-client-id"
        self._discord_admin_client_secret = "discord-client-secret"
        self._discord_admin_redirect_uri = "https://dashboard.example/twitch/auth/discord/callback"
        self._discord_admin_cookie_name = "twitch_admin_session"
        self._discord_admin_session_ttl = 6 * 3600
        self._discord_admin_state_ttl = 600
        self._discord_admin_oauth_states: dict[str, dict] = {}
        self._discord_admin_sessions: dict[str, dict] = {}
        self._discord_sessions_db_loaded = True
        self._rate_limits: dict[str, list[float]] = {}
        self.exchange_calls: list[tuple[str, str]] = []
        self.created_sessions: list[dict[str, str]] = []
        self.discord_exchange_calls: list[tuple[str, str]] = []
        self.discord_user_tokens: list[str] = []
        self.discord_membership_checks: list[int] = []
        self._state_repo = _InMemoryAuthStateRepo(self)

    def _check_v2_auth(self, request) -> bool:
        del request
        return False

    def _check_rate_limit(
        self, request, *, max_requests: int = 10, window_seconds: float = 60.0
    ) -> bool:
        del request, max_requests, window_seconds
        return True

    def _is_secure_request(self, request) -> bool:
        return bool(getattr(request, "secure", False))

    def _sanitize_log_value(self, value):
        return str(value or "")

    def _peer_host(self, request):
        return str(getattr(request, "remote", "") or "")

    def _normalized_discord_admin_redirect_uri(self):
        return DashboardV2Server._normalized_discord_admin_redirect_uri(self)

    def _dashboard_auth_state_repo(self):
        return self._state_repo

    async def _exchange_code_for_user(self, code: str, redirect_uri: str):
        self.exchange_calls.append((code, redirect_uri))
        return {
            "twitch_login": "partner_one",
            "twitch_user_id": "1001",
            "display_name": "Partner One",
        }

    async def _exchange_discord_admin_code(self, code: str, redirect_uri: str):
        self.discord_exchange_calls.append((code, redirect_uri))
        return {"access_token": "discord-access-token"}

    async def _fetch_discord_admin_user(self, access_token: str):
        self.discord_user_tokens.append(access_token)
        return {
            "id": "42",
            "username": "mod_user",
            "global_name": "Moderator User",
            "discriminator": "0",
        }

    async def _check_discord_admin_membership(self, user_id: int):
        self.discord_membership_checks.append(user_id)
        return True, "moderator_role:1"

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


class _InMemoryAuthStateRepo:
    def __init__(self, owner: _AuthHarness) -> None:
        self.owner = owner

    def save_twitch_oauth_state(self, *, state: str, payload: dict, ttl_seconds: float, now=None) -> None:
        del ttl_seconds, now
        self.owner._dashboard_auth_state_cache("_oauth_states").put(state, payload)

    def consume_twitch_oauth_state(self, state: str, *, now=None) -> dict | None:
        del now
        return self.owner._dashboard_auth_state_cache("_oauth_states").pop(state, None)

    def save_discord_admin_oauth_state(
        self, *, state: str, payload: dict, ttl_seconds: float, now=None
    ) -> None:
        del ttl_seconds, now
        self.owner._dashboard_auth_state_cache("_discord_admin_oauth_states").put(state, payload)

    def consume_discord_admin_oauth_state(self, state: str, *, now=None) -> dict | None:
        del now
        return self.owner._dashboard_auth_state_cache("_discord_admin_oauth_states").pop(state, None)

    def load_dashboard_session(self, session_id: str, *, now=None) -> dict | None:
        del now
        return self.owner._dashboard_auth_state_cache("_auth_sessions").get(session_id)

    def save_dashboard_session(
        self, *, session_id: str, payload: dict, created_at: float, expires_at: float
    ) -> None:
        del created_at, expires_at
        self.owner._dashboard_auth_state_cache("_auth_sessions").put(session_id, payload)

    def load_discord_admin_session(self, session_id: str, *, now=None) -> dict | None:
        del now
        return self.owner._dashboard_auth_state_cache("_discord_admin_sessions").get(session_id)

    def save_discord_admin_session(
        self, *, session_id: str, payload: dict, created_at: float, expires_at: float
    ) -> None:
        del created_at, expires_at
        self.owner._dashboard_auth_state_cache("_discord_admin_sessions").put(session_id, payload)

    def delete_session(self, session_id: str) -> None:
        self.owner._dashboard_auth_state_cache("_auth_sessions").pop(session_id, None)
        self.owner._dashboard_auth_state_cache("_discord_admin_sessions").pop(session_id, None)

    def delete_expired(self, now: float | None = None) -> None:
        del now


class DashboardOAuthStateBindingTests(unittest.IsolatedAsyncioTestCase):
    def _assert_cookie_security_flags(
        self,
        cookie,
        *,
        path: str,
        secure: bool = True,
        max_age: str | None = None,
    ) -> None:
        self.assertIsNotNone(cookie)
        self.assertTrue(cookie["httponly"])
        self.assertEqual(cookie["secure"], secure)
        self.assertEqual(cookie["samesite"], "Lax")
        self.assertEqual(cookie["path"], path)
        if max_age is not None:
            self.assertEqual(cookie["max-age"], max_age)

    async def test_auth_login_sets_context_cookie_and_binds_state(self) -> None:
        handler = _AuthHarness()
        request = _make_request(query={"next": "/twitch/dashboard"})
        context_token = "ctx_token_abcdefghijklmnop"
        state = "state_token_abcdefghijklmnop"

        with patch(
            "bot.dashboard.auth.auth_mixin.secrets.token_urlsafe",
            side_effect=[context_token, state],
        ):
            with self.assertRaises(web.HTTPFound) as ctx:
                await handler.auth_login(request)

        self.assertIn(f"state={state}", ctx.exception.location)
        oauth_cache = handler._dashboard_auth_state_cache("_oauth_states")
        self.assertIn(state, oauth_cache.data())
        self.assertEqual(oauth_cache.get(state).get("context_token"), context_token)
        self.assertEqual(oauth_cache.get(state).get("next_path"), "/twitch/dashboard")
        cookie = ctx.exception.cookies.get(handler._oauth_context_cookie_name())
        self.assertEqual(cookie.value, context_token)
        self._assert_cookie_security_flags(
            cookie,
            path="/twitch/auth/callback",
            max_age=str(handler._oauth_state_ttl_seconds),
        )

    async def test_auth_login_rejects_malicious_next_variants(self) -> None:
        handler = _AuthHarness()

        for index, raw_next in enumerate(_MALICIOUS_NEXT_VARIANTS, start=1):
            state = f"state_token_{index:02d}_abcdefghijklmnop"
            context_token = f"ctx_token_{index:02d}_abcdefghijklmnop"
            request = _make_request(query={"next": raw_next})

            with self.subTest(next_value=raw_next):
                with patch(
                    "bot.dashboard.auth.auth_mixin.secrets.token_urlsafe",
                    side_effect=[context_token, state],
                ):
                    with self.assertRaises(web.HTTPFound):
                        await handler.auth_login(request)

                self.assertEqual(
                    handler._dashboard_auth_state_cache("_oauth_states").get(state).get("next_path"),
                    "/twitch/dashboard",
                )

    async def test_auth_callback_rejects_missing_context_cookie_and_consumes_state(self) -> None:
        handler = _AuthHarness()
        state = "state_token_missing_cookie_123"
        handler._dashboard_auth_state_cache("_oauth_states").put(state, {
            "created_at": time.time(),
            "next_path": "/twitch/dashboard",
            "redirect_uri": "https://dashboard.example/twitch/auth/callback",
            "context_token": "ctx_token_abcdefghijklmnop",
        })
        request = _make_request(query={"state": state, "code": "oauth-code"}, cookies={})

        response = await handler.auth_callback(request)

        self.assertEqual(response.status, 400)
        self.assertIn("OAuth state ungültig oder abgelaufen.", response.text)
        self.assertNotIn(state, handler._dashboard_auth_state_cache("_oauth_states").data())
        self.assertEqual(handler.exchange_calls, [])

    async def test_auth_callback_requires_bound_cookie_and_state_is_one_time(self) -> None:
        handler = _AuthHarness()
        context_token = "ctx_token_abcdefghijklmnop"
        state = "state_token_abcdefghijklmnop"
        handler._dashboard_auth_state_cache("_oauth_states").put(state, {
            "created_at": time.time(),
            "next_path": "/twitch/dashboard",
            "redirect_uri": "https://dashboard.example/twitch/auth/callback",
            "context_token": context_token,
        })
        request = _make_request(
            query={"state": state, "code": "oauth-code"},
            cookies={handler._oauth_context_cookie_name(): context_token},
        )

        with self.assertRaises(web.HTTPFound) as ctx:
            await handler.auth_callback(request)

        self.assertEqual(ctx.exception.location, "/twitch/dashboard")
        self.assertEqual(
            handler.exchange_calls,
            [("oauth-code", "https://dashboard.example/twitch/auth/callback")],
        )
        self.assertEqual(len(handler.created_sessions), 1)
        session_cookie = ctx.exception.cookies.get(handler._session_cookie_name)
        self.assertEqual(session_cookie.value, "session-123")
        self._assert_cookie_security_flags(
            session_cookie,
            path="/",
            max_age=str(handler._session_ttl_seconds),
        )
        oauth_cookie = ctx.exception.cookies.get(handler._oauth_context_cookie_name())
        self.assertEqual(oauth_cookie.value, "")
        self._assert_cookie_security_flags(
            oauth_cookie,
            path="/twitch/auth/callback",
            max_age="0",
        )

        replay = await handler.auth_callback(request)
        self.assertEqual(replay.status, 400)
        self.assertIn("OAuth state ungültig oder abgelaufen.", replay.text)

    async def test_discord_auth_login_sets_context_cookie_and_binds_state(self) -> None:
        handler = _AuthHarness()
        request = _make_request(
            query={"next": "/twitch/admin/announcements?tab=mod"},
            path_qs="/twitch/admin",
        )
        state = "discord_state_abcdefghijklmnop"
        context_token = "discord_ctx_abcdefghijklmnop"

        with patch(
            "bot.dashboard.auth.auth_mixin.secrets.token_urlsafe",
            side_effect=[state, context_token],
        ):
            with self.assertRaises(web.HTTPFound) as ctx:
                await handler.discord_auth_login(request)

        self.assertIn("/oauth2/authorize?", ctx.exception.location)
        self.assertIn(f"state={state}", ctx.exception.location)
        discord_oauth_cache = handler._dashboard_auth_state_cache("_discord_admin_oauth_states")
        self.assertIn(state, discord_oauth_cache.data())
        self.assertEqual(
            discord_oauth_cache.get(state).get("context_token"),
            context_token,
        )
        self.assertEqual(
            discord_oauth_cache.get(state).get("next_path"),
            "/twitch/admin/announcements?tab=mod",
        )
        cookie = ctx.exception.cookies.get(handler._discord_oauth_context_cookie_name())
        self.assertEqual(cookie.value, context_token)
        self._assert_cookie_security_flags(
            cookie,
            path="/twitch/auth/discord/callback",
            max_age=str(handler._discord_admin_state_ttl),
        )

    async def test_discord_auth_login_rejects_malicious_next_variants(self) -> None:
        handler = _AuthHarness()

        for index, raw_next in enumerate(_MALICIOUS_NEXT_VARIANTS, start=1):
            state = f"discord_state_{index:02d}_abcdefghijklmnop"
            context_token = f"discord_ctx_{index:02d}_abcdefghijklmnop"
            request = _make_request(query={"next": raw_next}, path_qs="/twitch/admin")

            with self.subTest(next_value=raw_next):
                with patch(
                    "bot.dashboard.auth.auth_mixin.secrets.token_urlsafe",
                    side_effect=[state, context_token],
                ):
                    with self.assertRaises(web.HTTPFound):
                        await handler.discord_auth_login(request)

                self.assertEqual(
                    handler._dashboard_auth_state_cache("_discord_admin_oauth_states")
                    .get(state)
                    .get("next_path"),
                    "/twitch/admin",
                )

    async def test_discord_auth_callback_sets_admin_cookie_and_clears_oauth_cookie(self) -> None:
        handler = _AuthHarness()
        context_token = "discord_ctx_abcdefghijklmnop"
        state = "discord_state_abcdefghijklmnop"
        handler._dashboard_auth_state_cache("_discord_admin_oauth_states").put(state, {
            "created_at": time.time(),
            "next_path": "/twitch/admin",
            "redirect_uri": "https://dashboard.example/twitch/auth/discord/callback",
            "context_token": context_token,
        })
        request = _make_request(
            query={"state": state, "code": "discord-oauth-code"},
            cookies={handler._discord_oauth_context_cookie_name(): context_token},
            path_qs="/twitch/auth/discord/callback",
        )

        with patch(
            "bot.dashboard.auth.auth_mixin.secrets.token_urlsafe",
            return_value="discord-session-123",
        ):
            with self.assertRaises(web.HTTPFound) as ctx:
                await handler.discord_auth_callback(request)

        self.assertEqual(ctx.exception.location, "/twitch/admin")
        self.assertEqual(
            handler.discord_exchange_calls,
            [("discord-oauth-code", "https://dashboard.example/twitch/auth/discord/callback")],
        )
        self.assertEqual(handler.discord_user_tokens, ["discord-access-token"])
        self.assertEqual(handler.discord_membership_checks, [42])
        admin_cookie = ctx.exception.cookies.get(handler._discord_admin_cookie_name)
        self.assertEqual(admin_cookie.value, "discord-session-123")
        self._assert_cookie_security_flags(
            admin_cookie,
            path="/",
            max_age=str(handler._discord_admin_session_ttl),
        )
        oauth_cookie = ctx.exception.cookies.get(handler._discord_oauth_context_cookie_name())
        self.assertEqual(oauth_cookie.value, "")
        self._assert_cookie_security_flags(
            oauth_cookie,
            path="/twitch/auth/discord/callback",
            max_age="0",
        )
        discord_session_cache = handler._dashboard_auth_state_cache("_discord_admin_sessions")
        self.assertIn("discord-session-123", discord_session_cache.data())
        self.assertEqual(
            discord_session_cache.get("discord-session-123").get("auth_type"),
            "discord_admin",
        )



if __name__ == "__main__":
    unittest.main()
