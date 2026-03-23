import unittest
from types import SimpleNamespace
from unittest.mock import AsyncMock, patch

from bot.dashboard.mixin import (
    RAID_OAUTH_SUCCESS_REDIRECT_URL,
    TwitchDashboardMixin,
)
from bot.raid.scope_profiles import BASE_SCOPE_PROFILE, BASE_STREAMER_SCOPES


class _FakeUsersResponse:
    def __init__(self, payload: dict):
        self.status = 200
        self._payload = payload

    async def __aenter__(self):
        return self

    async def __aexit__(self, exc_type, exc, tb):
        return False

    async def text(self) -> str:
        return ""

    async def json(self) -> dict:
        return self._payload


class _FakeSession:
    def __init__(self, payload: dict):
        self._payload = payload

    def get(self, *_args, **_kwargs):
        return _FakeUsersResponse(self._payload)


class _FakeAuthManager:
    client_id = "client-id"
    redirect_uri = "https://raid.earlysalty.com/twitch/raid/callback"

    def __init__(
        self,
        *,
        requested_login: str = "partner_one",
        expected_twitch_login: str = "partner_one",
        expected_twitch_user_id: str | None = None,
        scopes: list[str] | None = None,
        existing_auth: bool = False,
    ):
        self._requested_login = requested_login
        self._expected_twitch_login = expected_twitch_login
        self._expected_twitch_user_id = expected_twitch_user_id
        self._scopes = list(scopes or BASE_STREAMER_SCOPES)
        self._existing_auth = existing_auth
        self.generated_calls: list[tuple[str, dict[str, object]]] = []

    def consume_state_details(self, _state: str):
        return SimpleNamespace(
            requested_login=self._requested_login,
            scope_profile=BASE_SCOPE_PROFILE,
            expected_twitch_login=self._expected_twitch_login,
            expected_twitch_user_id=self._expected_twitch_user_id,
            discord_user_id="123456789",
        )

    def has_saved_auth_record(self, **_kwargs) -> bool:
        return self._existing_auth

    async def exchange_code_for_token(self, _code: str, _session) -> dict:
        return {
            "access_token": "access-token",
            "refresh_token": "refresh-token",
            "expires_in": 3600,
            "scope": list(self._scopes),
        }

    def generate_auth_url(self, login: str, **kwargs) -> str:
        self.generated_calls.append((login, kwargs))
        return f"https://auth.example/{login}"

    def generate_discord_button_url(self, login: str, **kwargs) -> str:
        self.generated_calls.append((login, kwargs))
        return f"https://auth.example/discord/{login}"

    def save_auth(self, **_kwargs) -> None:
        return None


class _DummyDashboardMixin(TwitchDashboardMixin):
    def __init__(
        self,
        *,
        auth_manager: _FakeAuthManager | None = None,
        user_login: str = "partner_one",
    ) -> None:
        self._raid_bot = SimpleNamespace(
            auth_manager=auth_manager or _FakeAuthManager(),
            session=_FakeSession(payload={"data": [{"id": "1001", "login": user_login}]}),
        )
        self.spawned_tasks: list[tuple[str, object]] = []

    def _spawn_bg_task(self, coro, name: str):
        self.spawned_tasks.append((name, coro))
        return SimpleNamespace(done=lambda: False)


class DashboardMixinRaidCallbackRedirectTests(unittest.IsolatedAsyncioTestCase):
    async def test_dashboard_callback_payload_contains_redirect_url(self) -> None:
        handler = _DummyDashboardMixin()

        payload = await handler._dashboard_raid_oauth_callback(
            code="oauth-code",
            state="valid-state",
            error="",
        )

        self.assertEqual(payload.get("status"), 200)
        self.assertEqual(payload.get("redirect_url"), RAID_OAUTH_SUCCESS_REDIRECT_URL)

    async def test_dashboard_callback_rejects_wrong_twitch_account(self) -> None:
        handler = _DummyDashboardMixin(user_login="wrong_account")

        payload = await handler._dashboard_raid_oauth_callback(
            code="oauth-code",
            state="valid-state",
            error="",
        )

        self.assertEqual(payload.get("status"), 403)
        self.assertEqual(payload.get("title"), "Falscher Twitch-Account")

    async def test_dashboard_callback_accepts_login_rename_when_user_id_matches(self) -> None:
        handler = _DummyDashboardMixin(
            auth_manager=_FakeAuthManager(
                expected_twitch_login="old_partner_name",
                expected_twitch_user_id="1001",
            ),
            user_login="partner_one",
        )

        payload = await handler._dashboard_raid_oauth_callback(
            code="oauth-code",
            state="valid-state",
            error="",
        )

        self.assertEqual(payload.get("status"), 200)

    async def test_dashboard_callback_accepts_public_onboarding_without_expected_login(self) -> None:
        handler = _DummyDashboardMixin(
            auth_manager=_FakeAuthManager(
                requested_login="public:website_onboarding",
                expected_twitch_login="",
                expected_twitch_user_id=None,
            ),
            user_login="fresh_partner",
        )

        payload = await handler._dashboard_raid_oauth_callback(
            code="oauth-code",
            state="valid-state",
            error="",
        )

        self.assertEqual(payload.get("status"), 200)

    async def test_dashboard_raid_auth_url_allows_public_onboarding_login(self) -> None:
        auth_manager = _FakeAuthManager()
        handler = _DummyDashboardMixin(auth_manager=auth_manager)

        auth_url = await handler._dashboard_raid_auth_url(
            "public:website_onboarding",
            scope_profile="base",
        )

        self.assertEqual(auth_url, "https://auth.example/public:website_onboarding")
        self.assertEqual(
            auth_manager.generated_calls,
            [("public:website_onboarding", {"scope_profile": "base"})],
        )

    async def test_dashboard_callback_rejects_unexpected_scope_widening(self) -> None:
        handler = _DummyDashboardMixin(
            auth_manager=_FakeAuthManager(
                scopes=list(BASE_STREAMER_SCOPES) + ["channel:read:hype_train"]
            )
        )

        payload = await handler._dashboard_raid_oauth_callback(
            code="oauth-code",
            state="valid-state",
            error="",
        )

        self.assertEqual(payload.get("status"), 400)
        self.assertEqual(payload.get("title"), "Ungültige Berechtigungen")

    async def test_dashboard_callback_syncs_discord_link_on_reauth_without_full_setup(self) -> None:
        raid_bot = SimpleNamespace(
            auth_manager=_FakeAuthManager(existing_auth=True),
            session=_FakeSession(payload={"data": [{"id": "1001", "login": "partner_one"}]}),
            complete_setup_for_streamer=AsyncMock(),
            _sync_partner_state_after_auth=AsyncMock(),
        )
        handler = _DummyDashboardMixin()
        handler._raid_bot = raid_bot
        with patch(
            "bot.dashboard.raids.oauth_callback.asyncio.create_task",
            side_effect=AssertionError("raw create_task should not be used"),
        ):
            payload = await handler._dashboard_raid_oauth_callback(
                code="oauth-code",
                state="valid-state",
                error="",
            )

        self.assertEqual(payload.get("status"), 200)
        raid_bot.complete_setup_for_streamer.assert_not_called()
        self.assertEqual(len(handler.spawned_tasks), 1)
        self.assertEqual(
            handler.spawned_tasks[0][0], "twitch.raid.sync_partner_state_after_auth"
        )
        await handler.spawned_tasks[0][1]
        raid_bot._sync_partner_state_after_auth.assert_awaited_once_with(
            "1001",
            "partner_one",
            state_discord_user_id="123456789",
            activate_partner_features=False,
        )

    async def test_dashboard_callback_returns_500_when_followup_task_cannot_be_scheduled(self) -> None:
        raid_bot = SimpleNamespace(
            auth_manager=_FakeAuthManager(),
            session=_FakeSession(payload={"data": [{"id": "1001", "login": "partner_one"}]}),
            complete_setup_for_streamer=AsyncMock(),
            _sync_partner_state_after_auth=AsyncMock(),
        )
        handler = _DummyDashboardMixin()
        handler._raid_bot = raid_bot
        handler._spawn_bg_task = lambda coro, name: (coro.close(), None)[1]

        payload = await handler._dashboard_raid_oauth_callback(
            code="oauth-code",
            state="valid-state",
            error="",
        )

        self.assertEqual(payload.get("status"), 500)
        self.assertEqual(payload.get("title"), "Autorisierung fehlgeschlagen")

    async def test_dashboard_callback_does_not_log_raw_state_on_early_failure(self) -> None:
        auth_manager = _FakeAuthManager()
        auth_manager.consume_state_details = lambda _state: (_ for _ in ()).throw(RuntimeError("boom"))
        handler = _DummyDashboardMixin(auth_manager=auth_manager)

        with patch("bot.dashboard.raids.oauth_callback.log.exception") as mocked_log_exception:
            payload = await handler._dashboard_raid_oauth_callback(
                code="oauth-code",
                state="sensitive-state-token",
                error="",
            )

        self.assertEqual(payload.get("status"), 500)
        logged_args = mocked_log_exception.call_args.args
        self.assertEqual(logged_args[1], "<unknown>")


if __name__ == "__main__":
    unittest.main()
