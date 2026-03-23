import contextlib
import json
import io
import pathlib
import sqlite3
import tempfile
import unittest
from types import SimpleNamespace
from unittest.mock import AsyncMock, patch
from urllib.error import HTTPError

from aiohttp import web

from bot import _invoke_twitch_leaderboard_callback
from bot.analytics.api_ai import _AnalyticsAIMixin
from bot.analytics.api_overview import _AnalyticsOverviewMixin
from bot.analytics.api_v2 import AnalyticsV2Mixin
from bot.dashboard.auth_mixin import _DashboardAuthMixin
from bot.dashboard.billing_mixin import _DashboardBillingMixin
from bot.dashboard.live import DashboardLiveMixin
from bot.dashboard.raid_mixin import PUBLIC_STREAMER_ONBOARDING_URL, _DashboardRaidMixin
from bot.dashboard.routes_mixin import _DashboardRoutesMixin
from bot.dashboard.server_v2 import DashboardV2Server
from bot.raid.scope_profiles import BASE_SCOPE_PROFILE
from bot.social_media.clip_manager import ClipManager
from bot.social_media.dashboard import SocialMediaDashboard

_MALICIOUS_NEXT_VARIANTS = (
    "//evil.example",
    "%2F%2Fevil.example",
    "http://evil.example/path",
    "\\\\evil.example",
)


class _DummyBilling(_DashboardBillingMixin):
    def __init__(self) -> None:
        self.dashboard_session = {}
        self.discord_admin_session = {}

    def _get_dashboard_auth_session(self, request):
        return self.dashboard_session

    def _get_discord_admin_session(self, request):
        return self.discord_admin_session


class _DummyRoutes(_DashboardRoutesMixin):
    def __init__(self) -> None:
        self.dashboard_session = None
        self.discord_admin_session = None

    def _get_dashboard_auth_session(self, request):
        return self.dashboard_session

    def _get_discord_admin_session(self, request):
        return self.discord_admin_session


class _DummyRoutesApi(_DashboardRoutesMixin, AnalyticsV2Mixin):
    def _check_v2_auth(self, request):
        return True

    def _is_local_request(self, request):
        return False

    def _is_discord_admin_request(self, request):
        return False

    def _check_admin_token(self, token):
        return False

    def _get_auth_level(self, request):
        return "partner"


class _DummyV2HeaderOnlyAuth(AnalyticsV2Mixin):
    def __init__(self) -> None:
        self._noauth = False
        self._token = "admin-secret"
        self._partner_token = "partner-secret"


class _DummyOverviewAssetsAuth(_AnalyticsOverviewMixin):
    def _check_v2_auth(self, request):
        return False

    def _should_use_discord_admin_login(self, request):
        return False


class _DummyOverviewMetrics(_AnalyticsOverviewMixin):
    pass


class _DummyAdminOverviewAssets(_AnalyticsOverviewMixin):
    def __init__(self, dist_root: pathlib.Path) -> None:
        self._dist_root = dist_root.resolve()

    def _admin_dashboard_spa_gate(self, request):
        return None

    def _admin_dashboard_dist_root(self):
        return self._dist_root


class _DummyInternalHomeApi(AnalyticsV2Mixin):
    def __init__(self, dashboard_session=None, discord_admin_session=None) -> None:
        self._noauth = False
        self._token = "admin-secret"
        self._partner_token = "partner-secret"
        self.dashboard_session = dashboard_session
        self.discord_admin_session = discord_admin_session
        self.payload_args = None
        self.changelog_can_write = None
        self.created_changelog_args = None

    def _get_dashboard_auth_session(self, request):
        return self.dashboard_session

    def _get_discord_admin_session(self, request):
        return self.discord_admin_session

    def _build_internal_home_payload(self, *, twitch_login, twitch_user_id, display_name, days):
        self.payload_args = {
            "twitch_login": twitch_login,
            "twitch_user_id": twitch_user_id,
            "display_name": display_name,
            "days": days,
        }
        events = [
            {
                "type": "raid_history",
                "target_login": "raid_target",
                "target_id": "424242",
                "timestamp": "2026-03-03T12:30:00+00:00",
                "status_label": "[RAID]",
            }
        ]
        return {
            "profile": {"twitch_login": twitch_login, "twitch_user_id": twitch_user_id},
            "status": {"raid_status": {"state": "active", "read_only": True}},
            "kpis": {
                "streams_count": 0,
                "avg_viewers": 0.0,
                "follower_delta": 0,
                "bot_bans_keyword_count": 0,
            },
            "recent_streams": [],
            "bot_impact": {"events": list(events), "summary": {}, "note": "ok"},
            "bot_activity": {"events": list(events)},
            "links": {"dashboard": "/twitch/dashboard"},
        }

    def _get_internal_home_changelog_payload(self, *, can_write):
        self.changelog_can_write = can_write
        return {
            "entries": [
                {
                    "id": 1,
                    "entry_date": "2026-03-03",
                    "title": "Backend deployed",
                    "content": "Internal Home changelog is live.",
                    "created_at": "2026-03-03T12:00:00+00:00",
                }
            ],
            "can_write": can_write,
            "max_entries": 20,
        }

    def _create_internal_home_changelog_entry(self, *, title, content, entry_date):
        self.created_changelog_args = {
            "title": title,
            "content": content,
            "entry_date": entry_date,
        }
        return {
            "id": 2,
            "entry_date": entry_date.isoformat(),
            "title": title,
            "content": content,
            "created_at": "2026-03-03T13:00:00+00:00",
        }


class _DummyV2Errors(AnalyticsV2Mixin):
    def _check_v2_auth(self, request):
        return True


class _DummyInternalHomeRateLimit(AnalyticsV2Mixin):
    def _check_rate_limit(
        self,
        request,
        *,
        max_requests: int = 10,
        window_seconds: float = 60.0,
    ):
        del request, max_requests, window_seconds
        raise TypeError("internal bug")


class _DummyAIErrors(_AnalyticsAIMixin):
    def _require_v2_admin_api(self, request):
        return None

    async def _run_ai_analysis(self, streamer: str, days: int, since: str, game_filter: str):
        del streamer, days, since, game_filter
        raise RuntimeError("secret upstream detail")


class _DummyLiveActions(DashboardLiveMixin):
    def __init__(self) -> None:
        self.add_calls: list[str] = []
        self.manual_plan_set_calls: list[dict[str, str | None]] = []
        self.manual_plan_clear_calls: list[str] = []

    def _require_token(self, request):
        return None

    def _csrf_verify_token(self, request, provided_token: str) -> bool:
        return provided_token == "valid-csrf"

    def _redirect_location(self, request, *, ok=None, err=None, default_path="/twitch/stats"):
        if err:
            return "/twitch/admin?err=csrf"
        if ok:
            return "/twitch/admin?ok=1"
        return default_path

    async def _do_add(self, raw: str) -> str:
        self.add_calls.append(raw)
        return raw or "added"

    def _billing_admin_set_manual_plan(
        self,
        *,
        twitch_login: str,
        plan_id: str,
        expires_at: str | None = None,
        notes: str | None = None,
    ):
        self.manual_plan_set_calls.append(
            {
                "twitch_login": twitch_login,
                "plan_id": plan_id,
                "expires_at": expires_at,
                "notes": notes,
            }
        )
        return {
            "effective_plan_id": plan_id,
            "effective_plan_source": "manual_override",
        }

    def _billing_admin_clear_manual_plan(self, *, twitch_login: str):
        self.manual_plan_clear_calls.append(twitch_login)
        return {
            "effective_plan_id": "raid_free",
            "effective_plan_source": "default_basic",
        }


class _AdminPostGate(_DummyLiveActions):
    _discord_admin_required = True
    _normalize_discord_admin_next_path = staticmethod(
        DashboardV2Server._normalize_discord_admin_next_path
    )
    _path_matches_prefixes = staticmethod(DashboardV2Server._path_matches_prefixes)
    _safe_discord_admin_login_redirect = staticmethod(
        DashboardV2Server._safe_discord_admin_login_redirect
    )

    def _check_v2_auth(self, request):
        del request
        return False

    def _is_discord_admin_request(self, request):
        del request
        return False

    def _build_discord_admin_login_url(self, request, *, next_path=None):
        return DashboardV2Server._build_discord_admin_login_url(
            self,
            request,
            next_path=next_path,
        )

    def _require_token(self, request):
        return DashboardV2Server._require_token(self, request)


class _DummyRaidAuthRoute(_DashboardRaidMixin):
    def __init__(self) -> None:
        self.dashboard_session = None
        self.require_token_calls = 0
        self.last_auth_login = None
        self.last_auth_scope_profile = None

        def _generate_auth_url(login: str, **kwargs) -> str:
            self.last_auth_login = login
            self.last_auth_scope_profile = kwargs.get("scope_profile")
            if self.last_auth_scope_profile:
                return f"https://auth.example/{login}?scope_profile={self.last_auth_scope_profile}"
            return f"https://auth.example/{login}"

        self._raid_bot = SimpleNamespace(
            auth_manager=SimpleNamespace(
                client_id="raid-client-id",
                redirect_uri="https://twitch.earlysalty.com/twitch/raid/callback",
                generate_auth_url=_generate_auth_url,
            )
        )

    def _get_dashboard_auth_session(self, request):
        return self.dashboard_session

    def _require_token(self, request):
        del request
        self.require_token_calls += 1
        return None


class _DummyDashboardChatBot:
    def __init__(self, lookup_user_id: str | None = None) -> None:
        self.chat_calls: list[dict[str, str]] = []
        self.announcement_calls: list[dict[str, str]] = []
        self.lookup_user_id = lookup_user_id

    @staticmethod
    def _make_promo_channel(login: str, channel_id: str):
        return SimpleNamespace(name=login, id=channel_id)

    async def _send_chat_message(self, channel, text: str, source: str | None = None) -> bool:
        self.chat_calls.append(
            {
                "login": str(getattr(channel, "name", "") or ""),
                "channel_id": str(getattr(channel, "id", "") or ""),
                "text": text,
                "source": str(source or ""),
            }
        )
        return True

    async def _send_announcement(
        self,
        channel,
        text: str,
        color: str = "purple",
        source: str | None = None,
    ) -> bool:
        self.announcement_calls.append(
            {
                "login": str(getattr(channel, "name", "") or ""),
                "channel_id": str(getattr(channel, "id", "") or ""),
                "text": text,
                "color": color,
                "source": str(source or ""),
            }
        )
        return True

    async def fetch_user(self, login: str):
        if not self.lookup_user_id:
            return None
        return SimpleNamespace(id=self.lookup_user_id, login=login)


class _DummyLiveOwnerChatAction(DashboardLiveMixin):
    def __init__(
        self,
        user_id: str = "662995601738170389",
        *,
        is_partner: bool = True,
        missing_channel_id: bool = False,
        db_user_id: str = "",
        api_user_id: str | None = None,
    ) -> None:
        self.chat_bot = _DummyDashboardChatBot(lookup_user_id=api_user_id)
        self._raid_bot = SimpleNamespace(chat_bot=self.chat_bot)
        self.discord_admin_session = {"user_id": user_id}
        self._is_partner = is_partner
        self._missing_channel_id = missing_channel_id
        self._db_user_id = db_user_id
        self.persisted_user_ids: list[tuple[str, str]] = []

    def _require_token(self, request):
        return None

    def _get_discord_admin_session(self, request):
        return self.discord_admin_session

    def _csrf_verify_token(self, request, provided_token: str) -> bool:
        return provided_token == "valid-csrf"

    def _redirect_location(self, request, *, ok=None, err=None, default_path="/twitch/stats"):
        if err:
            return "/twitch/admin?err=1"
        if ok:
            return "/twitch/admin?ok=1"
        return default_path

    def _resolve_streamer_user_id_from_db(self, login: str) -> str:
        del login
        return self._db_user_id

    def _persist_streamer_user_id(self, login: str, user_id: str) -> None:
        self.persisted_user_ids.append((login, user_id))

    async def _list(self) -> list[dict]:
        return [
            {
                "twitch_login": "partner_one",
                "twitch_user_id": None if self._missing_channel_id else "1001",
                "manual_partner_opt_out": 0 if self._is_partner else 1,
                "archived_at": None,
            }
        ]


class _WebhookConn:
    def execute(self, sql, params=None):
        if "INSERT INTO twitch_billing_events" in sql:
            raise RuntimeError("UNIQUE constraint failed: twitch_billing_events.stripe_event_id")
        return self

    def fetchone(self):
        return None


class _ConnCtx:
    def __init__(self, conn):
        self._conn = conn

    def __enter__(self):
        return self._conn

    def __exit__(self, exc_type, exc, tb):
        return False


class _CompatSqliteConn:
    def __init__(self, conn: sqlite3.Connection) -> None:
        self._conn = conn

    @staticmethod
    def _translate_sql(sql: str) -> str:
        return str(sql).replace("%s", "?")

    def execute(self, sql, params=()):
        return self._conn.execute(self._translate_sql(sql), params)

    def executemany(self, sql, params_seq):
        return self._conn.executemany(self._translate_sql(sql), params_seq)

    def __getattr__(self, name):
        return getattr(self._conn, name)


class _DummyWebhookRoutes(_DashboardRoutesMixin, _DashboardBillingMixin):
    def __init__(self) -> None:
        self._billing_stripe_webhook_secret = "whsec_test"
        self.apply_calls = 0

    def _billing_refresh_runtime_secrets(self) -> None:
        return None

    def _billing_ensure_storage_tables(self, conn) -> None:
        return None

    def _billing_import_stripe(self):
        class _Stripe:
            class Webhook:
                @staticmethod
                def construct_event(payload, sig_header, secret):
                    return {
                        "id": "evt_test",
                        "type": "invoice.payment_succeeded",
                        "data": {"object": {"id": "in_test"}},
                        "livemode": False,
                    }

        return _Stripe, None

    def _billing_apply_webhook_event(self, conn, **kwargs):
        self.apply_calls += 1
        return "applied"


class DashboardSecurityRegressionTests(unittest.IsolatedAsyncioTestCase):
    def test_billing_checkout_url_host_allowlist_blocks_untrusted_domain(self) -> None:
        handler = _DummyBilling()
        handler._billing_checkout_success_url = "https://safe.example/twitch/abbo?checkout=success"
        handler._billing_checkout_cancel_url = "https://safe.example/twitch/abbo?checkout=cancelled"

        self.assertTrue(handler._billing_is_http_url("https://safe.example/ok"))
        self.assertTrue(handler._billing_is_http_url("http://localhost:8080/ok"))
        self.assertFalse(handler._billing_is_http_url("https://evil.example/phish"))

    def test_billing_base_url_ignores_untrusted_host_header(self) -> None:
        class _BaseUrlRoutes(_DummyRoutes):
            _billing_checkout_success_url = "https://safe.example/checkout/success"
            _billing_checkout_cancel_url = "https://safe.example/checkout/cancel"

            def _is_local_request(self, _request):
                return False

        handler = _BaseUrlRoutes()
        request = SimpleNamespace(host="evil.example")

        self.assertEqual(handler._billing_base_url_for_request(request), "https://safe.example")

    async def test_checkout_session_helper_runs_in_thread(self) -> None:
        handler = _DummyRoutes()

        with patch(
            "bot.dashboard.routes_mixin.asyncio.to_thread",
            new=AsyncMock(return_value=({"id": "cs_test"}, None)),
        ) as mocked_to_thread:
            session_obj, error = await handler._billing_create_checkout_session_best_effort_async(
                session_payload={"mode": "subscription"},
                idempotency_key="idem_test",
            )

        self.assertEqual(session_obj, {"id": "cs_test"})
        self.assertIsNone(error)
        mocked_to_thread.assert_awaited_once()

    async def test_v2_streamers_error_is_sanitized(self) -> None:
        api = _DummyV2Errors()

        with patch(
            "bot.analytics.api_v2.storage.readonly_connection",
            side_effect=RuntimeError("db password leaked"),
        ):
            response = await api._api_v2_streamers(SimpleNamespace())

        payload = json.loads(response.text)
        self.assertEqual(response.status, 500)
        self.assertEqual(payload["code"], "streamers_load_failed")
        self.assertEqual(payload["error"], "Streamer konnten nicht geladen werden.")
        self.assertNotIn("db password leaked", response.text)

    def test_internal_home_rate_limit_type_error_fails_closed(self) -> None:
        api = _DummyInternalHomeRateLimit()

        response = api._internal_home_rate_limit_response(
            SimpleNamespace(),
            max_requests=3,
            window_seconds=30.0,
        )

        self.assertIsNotNone(response)
        self.assertEqual(response.status, 429)
        payload = json.loads(response.text)
        self.assertEqual(payload["error"], "rate_limit_exceeded")

    async def test_ai_analysis_error_is_sanitized(self) -> None:
        api = _DummyAIErrors()
        request = SimpleNamespace(query={"streamer": "streamer_one", "days": "30"})

        response = await api._api_v2_ai_analysis(request)

        payload = json.loads(response.text)
        self.assertEqual(response.status, 500)
        self.assertEqual(payload["code"], "ai_analysis_failed")
        self.assertEqual(payload["error"], "KI-Analyse konnte nicht geladen werden.")
        self.assertNotIn("secret upstream detail", response.text)

    async def test_twitch_leaderboard_helper_does_not_mask_internal_type_error(self) -> None:
        calls: list[str] = []

        async def _leaderboard(_ctx, *, filters: str = ""):
            calls.append(filters)
            raise TypeError("internal bug")

        with self.assertRaises(TypeError):
            await _invoke_twitch_leaderboard_callback(
                _leaderboard,
                SimpleNamespace(),
                filters="samples=7",
            )

        self.assertEqual(calls, ["samples=7"])

    def test_checkout_session_rest_http_error_is_sanitized(self) -> None:
        with patch(
            "bot.dashboard.routes_mixin._urlrequest.urlopen",
            side_effect=HTTPError(
                url="https://api.stripe.com/v1/checkout/sessions",
                code=402,
                msg="payment required",
                hdrs=None,
                fp=io.BytesIO(
                    b'{"error":{"message":"secret upstream detail","type":"card_error"}}'
                ),
            ),
        ):
            session_obj, error = _DummyRoutes._billing_create_checkout_session_rest(
                stripe_secret_key="sk_test",
                session_payload={"mode": "subscription"},
            )

        self.assertIsNone(session_obj)
        self.assertEqual(error, "stripe_http_error_402")
        self.assertNotIn("secret", error)

    def test_checkout_session_rest_generic_error_is_sanitized(self) -> None:
        with patch(
            "bot.dashboard.routes_mixin._urlrequest.urlopen",
            side_effect=RuntimeError("dsn=postgres://secret"),
        ):
            session_obj, error = _DummyRoutes._billing_create_checkout_session_rest(
                stripe_secret_key="sk_test",
                session_payload={"mode": "subscription"},
            )

        self.assertIsNone(session_obj)
        self.assertEqual(error, "stripe_checkout_create_failed")
        self.assertNotIn("secret", error)

    def test_social_media_blocks_partner_token_without_session_scope(self) -> None:
        dashboard = SocialMediaDashboard(
            clip_manager=ClipManager(),
            auth_checker=lambda _req: True,
            auth_level_getter=lambda _req: "partner",
        )
        request = SimpleNamespace(
            headers={"Host": "dashboard.example"},
            host="dashboard.example",
            remote="203.0.113.10",
            transport=None,
        )

        with self.assertRaises(web.HTTPForbidden):
            dashboard._resolve_streamer_scope(request, requested_streamer="victim_streamer")

    def test_social_media_oauth_origin_uses_configured_public_url(self) -> None:
        dashboard = SocialMediaDashboard(
            clip_manager=ClipManager(),
            auth_checker=lambda _req: True,
            auth_level_getter=lambda _req: "admin",
            public_base_url="https://safe.example",
        )
        request = SimpleNamespace(
            headers={"Host": "evil.example"},
            host="evil.example",
            remote="203.0.113.10",
            transport=None,
            url=SimpleNamespace(origin=lambda: "https://evil.example"),
        )

        self.assertEqual(dashboard._oauth_public_origin(request), "https://safe.example")

    def test_social_media_returns_503_when_twitch_oauth_missing(self) -> None:
        dashboard = SocialMediaDashboard(
            clip_manager=ClipManager(),
            auth_checker=lambda _req: False,
            oauth_ready_checker=lambda: False,
        )
        request = SimpleNamespace()

        with self.assertRaises(web.HTTPServiceUnavailable):
            dashboard._require_auth(request)

    def test_social_media_redirects_to_login_when_unauthenticated(self) -> None:
        dashboard = SocialMediaDashboard(
            clip_manager=ClipManager(),
            auth_checker=lambda _req: False,
            oauth_ready_checker=lambda: True,
        )
        request = SimpleNamespace()

        with self.assertRaises(web.HTTPFound) as ctx:
            dashboard._require_auth(request)

        self.assertEqual(ctx.exception.location, "/twitch/auth/login?next=%2Fsocial-media")

    def test_overview_monetization_counts_use_twitch_user_id_backed_schema(self) -> None:
        conn = sqlite3.connect(":memory:")
        try:
            conn.execute(
                """
                CREATE TABLE twitch_subscription_events (
                    session_id INTEGER,
                    twitch_user_id TEXT NOT NULL,
                    received_at TEXT NOT NULL
                )
                """
            )
            conn.execute(
                """
                CREATE TABLE twitch_bits_events (
                    session_id INTEGER,
                    twitch_user_id TEXT NOT NULL,
                    received_at TEXT NOT NULL
                )
                """
            )
            conn.execute(
                """
                CREATE TABLE twitch_hype_train_events (
                    session_id INTEGER,
                    started_at TEXT NOT NULL,
                    ended_at TEXT
                )
                """
            )
            conn.execute(
                """
                CREATE TABLE twitch_stream_sessions (
                    id INTEGER PRIMARY KEY,
                    streamer_login TEXT NOT NULL
                )
                """
            )
            conn.execute(
                """
                CREATE TABLE twitch_live_state (
                    twitch_user_id TEXT PRIMARY KEY,
                    streamer_login TEXT NOT NULL
                )
                """
            )
            conn.execute(
                """
                INSERT INTO twitch_live_state (twitch_user_id, streamer_login)
                VALUES ('user-1', 'streamer_a'), ('user-2', 'other_streamer')
                """
            )
            conn.execute(
                """
                INSERT INTO twitch_subscription_events (session_id, twitch_user_id, received_at)
                VALUES (NULL, 'user-1', '2026-03-18T10:00:00+00:00')
                """
            )
            conn.execute(
                """
                INSERT INTO twitch_bits_events (session_id, twitch_user_id, received_at)
                VALUES (NULL, 'user-1', '2026-03-18T10:05:00+00:00')
                """
            )
            conn.execute(
                """
                INSERT INTO twitch_bits_events (session_id, twitch_user_id, received_at)
                VALUES (NULL, 'user-2', '2026-03-18T10:06:00+00:00')
                """
            )

            result = _DummyOverviewMetrics()._get_monetization_event_counts(
                _CompatSqliteConn(conn),
                "2026-03-18T00:00:00+00:00",
                "streamer_a",
            )
        finally:
            conn.close()

        self.assertEqual(result["sub_events"], 1)
        self.assertEqual(result["bits_events"], 1)
        self.assertEqual(result["hype_trains"], 0)

    async def test_social_media_platform_status_masks_global_fallback_identity_for_streamer_scope(
        self,
    ) -> None:
        dashboard = SocialMediaDashboard(
            clip_manager=ClipManager(),
            auth_checker=lambda _req: True,
            auth_level_getter=lambda _req: "admin",
        )
        request = SimpleNamespace(query={"streamer": "streamer_a"})

        fake_status = {
            "youtube": {
                "connected": True,
                "username": "shared-channel",
                "user_id": "shared-user-id",
                "expires_at": "2026-03-18T10:00:00+00:00",
                "expired": False,
                "uses_global_fallback": True,
            }
        }
        fake_manager = SimpleNamespace(get_all_platforms_status=lambda _streamer: fake_status)

        with patch(
            "bot.social_media.credential_manager.SocialMediaCredentialManager",
            return_value=fake_manager,
        ):
            response = await dashboard.api_platforms_status(request)

        payload = json.loads(response.text)
        self.assertEqual(response.status, 200)
        self.assertEqual(payload["platforms"][0]["platform"], "youtube")
        self.assertTrue(payload["platforms"][0]["connected"])
        self.assertIsNone(payload["platforms"][0]["username"])
        self.assertIsNone(payload["platforms"][0]["user_id"])
        self.assertTrue(payload["platforms"][0]["uses_global_fallback"])

    async def test_social_media_oauth_start_allows_global_scope_without_streamer(self) -> None:
        dashboard = SocialMediaDashboard(
            clip_manager=ClipManager(),
            auth_checker=lambda _req: True,
            auth_level_getter=lambda _req: "admin",
            public_base_url="https://safe.example",
        )
        request = SimpleNamespace(match_info={"platform": "youtube"}, query={})

        captured: list[tuple[str, str | None, str]] = []
        fake_manager = SimpleNamespace(
            generate_auth_url=lambda platform, streamer, redirect_uri: (
                captured.append((platform, streamer, redirect_uri)) or "https://oauth.example/start"
            )
        )

        with patch(
            "bot.social_media.oauth_manager.SocialMediaOAuthManager",
            return_value=fake_manager,
        ):
            response = await dashboard.oauth_start(request)

        self.assertEqual(response.location, "https://oauth.example/start")
        self.assertEqual(
            captured,
            [("youtube", None, "https://safe.example/social-media/oauth/callback")],
        )

    async def test_social_media_oauth_disconnect_allows_global_scope_without_streamer(self) -> None:
        dashboard = SocialMediaDashboard(
            clip_manager=ClipManager(),
            auth_checker=lambda _req: True,
            auth_level_getter=lambda _req: "admin",
        )
        request = SimpleNamespace(match_info={"platform": "youtube"}, query={})

        class _Conn:
            def __init__(self) -> None:
                self.executed: list[tuple[str, tuple[object, ...]]] = []

            def execute(self, sql, params=()):
                self.executed.append((str(sql), tuple(params or ())))
                return None

        conn = _Conn()
        with patch("bot.social_media.dashboard.transaction", return_value=contextlib.nullcontext(conn)):
            response = await dashboard.oauth_disconnect(request)

        payload = json.loads(response.text)
        self.assertEqual(response.status, 200)
        self.assertTrue(payload["success"])
        self.assertEqual(conn.executed[0][1], ("youtube", None, None))

    async def test_social_media_clips_list_rejects_invalid_limit(self) -> None:
        dashboard = SocialMediaDashboard(
            clip_manager=ClipManager(),
            auth_checker=lambda _req: True,
            auth_level_getter=lambda _req: "admin",
        )
        request = SimpleNamespace(query={"limit": "invalid"})

        response = await dashboard.clips_list(request)
        payload = json.loads(response.text)

        self.assertEqual(response.status, 400)
        self.assertEqual(payload.get("error"), "invalid_limit")

    async def test_market_data_error_response_hides_internal_exception_message(self) -> None:
        class _MarketAuth(_DummyRoutesApi):
            def _is_discord_admin_request(self, request):
                return True

        handler = _MarketAuth()
        request = SimpleNamespace(headers={}, query={})

        with patch(
            "bot.dashboard.routes_mixin.storage.readonly_connection",
            side_effect=RuntimeError("dsn leak"),
        ):
            response = await handler.api_market_data(request)

        payload = json.loads(response.text)
        self.assertEqual(response.status, 500)
        self.assertEqual(payload.get("error"), "market_data_failed")
        self.assertIn("error_id", payload)
        self.assertNotIn("dsn leak", response.text)

    def test_billing_candidate_refs_ignore_query_tampering(self) -> None:
        handler = _DummyBilling()
        handler.dashboard_session = {"twitch_login": "owner_login", "twitch_user_id": "12345"}
        request = SimpleNamespace(query={"customer_reference": "attacker_ref"})

        refs = handler._billing_candidate_refs_for_request(request)

        self.assertEqual(refs, ["12345", "owner_login"])

    def test_billing_candidate_refs_include_discord_admin_namespace(self) -> None:
        handler = _DummyBilling()
        handler.discord_admin_session = {"user_id": 77, "display_name": "Admin"}
        request = SimpleNamespace(query={})

        refs = handler._billing_candidate_refs_for_request(request)

        self.assertEqual(refs, ["discord_admin:77"])

    def test_csrf_uses_discord_admin_session(self) -> None:
        handler = _DummyRoutes()
        handler.discord_admin_session = {"user_id": 1001}
        request = SimpleNamespace()

        token = handler._csrf_generate_token(request)

        self.assertEqual(handler.discord_admin_session.get("csrf_token"), token)
        self.assertTrue(handler._csrf_verify_token(request, token))

    async def test_abbo_cancel_get_does_not_mutate_state(self) -> None:
        class _CancelGuard(_DummyRoutes):
            def _check_v2_auth(self, request):
                return True

        handler = _CancelGuard()
        request = SimpleNamespace(method="GET")

        with self.assertRaises(web.HTTPFound) as ctx:
            await handler.abbo_cancel(request)
        self.assertEqual(ctx.exception.location, "/twitch/abbo?cancel=post_required")

    async def test_stripe_sync_products_requires_admin_scope(self) -> None:
        handler = _DummyRoutesApi()
        request = SimpleNamespace(headers={}, query={})

        response = await handler.api_billing_stripe_sync_products(request)

        self.assertEqual(response.status, 403)
        payload = json.loads(response.text)
        self.assertEqual(payload.get("error"), "admin_required")

    async def test_stripe_sync_products_product_error_is_sanitized(self) -> None:
        class _StripeSyncProductError(_DummyRoutesApi):
            _billing_stripe_secret_key = "sk_test"

            def _get_auth_level(self, request):
                return "admin"

            async def _billing_read_request_body(self, _request):
                return {}

            def _billing_import_stripe(self):
                class _Stripe:
                    api_key = ""

                    class Product:
                        @staticmethod
                        def retrieve(_product_id):
                            raise RuntimeError("missing")

                        @staticmethod
                        def create(**_kwargs):
                            exc = RuntimeError("secret product failure")
                            exc.user_message = "secret product detail"
                            raise exc

                    class Price:
                        @staticmethod
                        def retrieve(_price_id):
                            raise RuntimeError("missing")

                        @staticmethod
                        def list(**_kwargs):
                            return {"data": []}

                return _Stripe, None

            def _billing_product_id_map(self):
                return {}

            def _billing_price_id_map(self):
                return {}

            def _billing_save_product_id_map(self, _mapping):
                return None

            def _billing_save_price_id_map(self, _mapping):
                return None

            @staticmethod
            def _billing_stripe_obj_get(obj, key, default=None):
                if isinstance(obj, dict):
                    return obj.get(key, default)
                return getattr(obj, key, default)

        handler = _StripeSyncProductError()
        request = SimpleNamespace(headers={}, query={})

        response = await handler.api_billing_stripe_sync_products(request)

        self.assertEqual(response.status, 502)
        payload = json.loads(response.text)
        self.assertEqual(payload.get("error"), "stripe_product_create_failed")
        self.assertEqual(payload.get("message"), "stripe_product_create_failed")
        self.assertNotIn("secret", payload.get("message", ""))

    async def test_stripe_sync_products_price_error_is_sanitized(self) -> None:
        class _StripeSyncPriceError(_DummyRoutesApi):
            _billing_stripe_secret_key = "sk_test"

            def _get_auth_level(self, request):
                return "admin"

            async def _billing_read_request_body(self, _request):
                return {}

            def _billing_import_stripe(self):
                class _Stripe:
                    api_key = ""

                    class Product:
                        @staticmethod
                        def retrieve(_product_id):
                            raise RuntimeError("missing")

                        @staticmethod
                        def create(**_kwargs):
                            return SimpleNamespace(id="prod_test")

                    class Price:
                        @staticmethod
                        def retrieve(_price_id):
                            raise RuntimeError("missing")

                        @staticmethod
                        def list(**_kwargs):
                            return {"data": []}

                        @staticmethod
                        def create(**_kwargs):
                            exc = RuntimeError("secret price failure")
                            exc.user_message = "secret price detail"
                            raise exc

                return _Stripe, None

            def _billing_product_id_map(self):
                return {}

            def _billing_price_id_map(self):
                return {}

            def _billing_save_product_id_map(self, _mapping):
                return None

            def _billing_save_price_id_map(self, _mapping):
                return None

            @staticmethod
            def _billing_stripe_obj_get(obj, key, default=None):
                if isinstance(obj, dict):
                    return obj.get(key, default)
                return getattr(obj, key, default)

        handler = _StripeSyncPriceError()
        request = SimpleNamespace(headers={}, query={})

        response = await handler.api_billing_stripe_sync_products(request)

        self.assertEqual(response.status, 502)
        payload = json.loads(response.text)
        self.assertEqual(payload.get("error"), "stripe_price_create_failed")
        self.assertEqual(payload.get("message"), "stripe_price_create_failed")
        self.assertNotIn("secret", payload.get("message", ""))

    async def test_market_data_allows_discord_admin_session(self) -> None:
        class _MarketAuth(_DummyRoutesApi):
            def _is_discord_admin_request(self, request):
                return True

        handler = _MarketAuth()
        request = SimpleNamespace(headers={}, query={})

        with patch(
            "bot.dashboard.routes_mixin.storage.readonly_connection",
            side_effect=RuntimeError("boom"),
        ):
            response = await handler.api_market_data(request)
        self.assertEqual(response.status, 500)

    async def test_live_fetch_user_internal_type_error_is_not_retried(self) -> None:
        class _BrokenFetchBot:
            def __init__(self) -> None:
                self.calls: list[str] = []

            async def fetch_user(self, *, login: str):
                self.calls.append(login)
                raise TypeError("broken internals")

        handler = _DummyLiveOwnerChatAction(api_user_id=None)
        chat_bot = _BrokenFetchBot()

        resolved = await handler._resolve_streamer_user_id_from_twitch(chat_bot, "partner_one")

        self.assertEqual(resolved, "")
        self.assertEqual(chat_bot.calls, ["partner_one"])

    async def test_live_fetch_user_positional_fallback_still_works(self) -> None:
        class _PositionalFetchBot:
            async def fetch_user(self, login: str):
                return SimpleNamespace(id="12345", login=login)

        handler = _DummyLiveOwnerChatAction(api_user_id=None)

        resolved = await handler._resolve_streamer_user_id_from_twitch(
            _PositionalFetchBot(),
            "partner_one",
        )

        self.assertEqual(resolved, "12345")

    async def test_market_dashboard_uses_html_escaping_in_templates(self) -> None:
        class _MarketPage(_DummyRoutes):
            def _require_token(self, request):
                return None

        handler = _MarketPage()
        request = SimpleNamespace()

        response = await handler.market_research(request)

        self.assertEqual(response.status, 200)
        self.assertIn("const escapeHtml", response.text)
        self.assertIn("${escapeHtml(q.content)}", response.text)
        self.assertIn("${escapeHtml(c.login)}", response.text)

    async def test_stripe_webhook_duplicate_event_returns_idempotent_success(self) -> None:
        handler = _DummyWebhookRoutes()

        class _Req:
            headers = {"Stripe-Signature": "sig_test"}

            async def read(self):
                return b'{"id":"evt_test"}'

        request = _Req()
        with patch(
            "bot.dashboard.routes_mixin.storage.transaction",
            return_value=_ConnCtx(_WebhookConn()),
        ):
            response = await handler.api_billing_stripe_webhook(request)

        self.assertEqual(response.status, 200)
        payload = json.loads(response.text)
        self.assertEqual(payload.get("status"), "duplicate")
        self.assertEqual(handler.apply_calls, 0)

    def test_api_v2_auth_ignores_query_tokens(self) -> None:
        handler = _DummyV2HeaderOnlyAuth()
        request_query_only = SimpleNamespace(
            headers={"Host": "dashboard.example"},
            query={"token": "admin-secret", "partner_token": "partner-secret"},
            host="dashboard.example",
            remote="203.0.113.10",
            transport=None,
        )
        request_partner_header = SimpleNamespace(
            headers={"Host": "dashboard.example", "X-Partner-Token": "partner-secret"},
            query={},
            host="dashboard.example",
            remote="203.0.113.10",
            transport=None,
        )
        request_admin_header = SimpleNamespace(
            headers={"Host": "dashboard.example", "X-Admin-Token": "admin-secret"},
            query={},
            host="dashboard.example",
            remote="203.0.113.10",
            transport=None,
        )

        self.assertFalse(handler._check_v2_auth(request_query_only))
        self.assertEqual(handler._get_auth_level(request_query_only), "none")
        self.assertTrue(handler._check_v2_auth(request_partner_header))
        self.assertEqual(handler._get_auth_level(request_partner_header), "partner")
        self.assertTrue(handler._check_v2_auth(request_admin_header))
        self.assertEqual(handler._get_auth_level(request_admin_header), "admin")

    def test_api_v2_auth_rejects_partner_token_on_admin_host(self) -> None:
        handler = _DummyV2HeaderOnlyAuth()
        request_partner_admin_host = SimpleNamespace(
            path="/twitch/api/v2/overview",
            headers={"Host": "admin.earlysalty.de", "X-Partner-Token": "partner-secret"},
            query={},
            host="admin.earlysalty.de",
            remote="203.0.113.10",
            transport=None,
            rel_url=SimpleNamespace(path_qs="/twitch/api/v2/overview"),
        )
        request_admin_admin_host = SimpleNamespace(
            path="/twitch/api/v2/overview",
            headers={"Host": "admin.earlysalty.de", "X-Admin-Token": "admin-secret"},
            query={},
            host="admin.earlysalty.de",
            remote="203.0.113.10",
            transport=None,
            rel_url=SimpleNamespace(path_qs="/twitch/api/v2/overview"),
        )

        self.assertFalse(handler._check_v2_auth(request_partner_admin_host))
        with self.assertRaises(web.HTTPForbidden) as partner_ctx:
            handler._require_v2_auth(request_partner_admin_host)
        partner_payload = json.loads(partner_ctx.exception.text)
        self.assertEqual(partner_payload.get("error"), "admin_required")
        self.assertEqual(partner_payload.get("required"), "admin")

        self.assertTrue(handler._check_v2_auth(request_admin_admin_host))

    def test_admin_require_token_rejects_header_token_without_discord_session(self) -> None:
        class _Gate:
            _discord_admin_required = True

            @staticmethod
            def _path_matches_prefixes(path, prefixes):
                return DashboardV2Server._path_matches_prefixes(path, prefixes)

            def _is_discord_admin_request(self, request):
                return False

            def _build_discord_admin_login_url(self, request, *, next_path=None):
                return "/twitch/auth/discord/login?next=%2Ftwitch%2Fadmin"

            def _safe_discord_admin_login_redirect(self, raw_url):
                return raw_url

            def _check_v2_auth(self, request):
                return False

        request = SimpleNamespace(
            path="/twitch/admin",
            method="GET",
            headers={"X-Admin-Token": "admin-secret"},
            rel_url=SimpleNamespace(path_qs="/twitch/admin"),
        )

        with self.assertRaises(web.HTTPFound) as ctx:
            DashboardV2Server._require_token(_Gate(), request)
        self.assertEqual(ctx.exception.location, "/twitch/auth/discord/login?next=%2Ftwitch%2Fadmin")

    def test_admin_require_token_returns_503_when_discord_oauth_missing(self) -> None:
        class _Gate:
            _discord_admin_required = False

            @staticmethod
            def _path_matches_prefixes(path, prefixes):
                return DashboardV2Server._path_matches_prefixes(path, prefixes)

            def _is_discord_admin_request(self, request):
                return False

            def _check_v2_auth(self, request):
                return False

        request = SimpleNamespace(
            path="/twitch/admin",
            method="GET",
            headers={},
            rel_url=SimpleNamespace(path_qs="/twitch/admin"),
        )

        with self.assertRaises(web.HTTPServiceUnavailable):
            DashboardV2Server._require_token(_Gate(), request)

    def test_safe_internal_redirect_rejects_malicious_next_variants(self) -> None:
        for raw_next in _MALICIOUS_NEXT_VARIANTS:
            with self.subTest(next_value=raw_next):
                self.assertEqual(
                    _DashboardAuthMixin._safe_internal_redirect(
                        raw_next,
                        fallback="/twitch/dashboard",
                    ),
                    "/twitch/dashboard",
                )

    def test_discord_admin_login_url_rejects_malicious_next_variants(self) -> None:
        handler = DashboardV2Server(
            app_token=None,
            noauth=False,
            partner_token=None,
            oauth_client_id="client-id",
            oauth_client_secret="client-secret",
            oauth_redirect_uri="https://dashboard.example/twitch/auth/callback",
        )
        handler._discord_admin_required = True
        request = SimpleNamespace(rel_url=SimpleNamespace(path_qs="/twitch/admin"))

        for raw_next in _MALICIOUS_NEXT_VARIANTS:
            with self.subTest(next_value=raw_next):
                self.assertEqual(
                    handler._build_discord_admin_login_url(request, next_path=raw_next),
                    "/twitch/auth/discord/login?next=%2Ftwitch%2Fadmin",
                )

    async def test_sensitive_admin_post_routes_require_discord_admin_context(self) -> None:
        handler = _AdminPostGate()
        cases = (
            (
                "/twitch/admin/chat_action",
                handler.admin_partner_chat_action,
                "/twitch/auth/discord/login?next=%2Ftwitch%2Fadmin%2Fchat_action",
            ),
            (
                "/twitch/admin/manual-plan",
                handler.admin_manual_plan_save,
                "/twitch/auth/discord/login?next=%2Ftwitch%2Fadmin%2Fmanual-plan",
            ),
            (
                "/twitch/admin/manual-plan/clear",
                handler.admin_manual_plan_clear,
                "/twitch/auth/discord/login?next=%2Ftwitch%2Fadmin%2Fmanual-plan%2Fclear",
            ),
        )

        def _make_post_request(path: str):
            class _Request:
                method = "POST"
                headers = {}
                rel_url = SimpleNamespace(path_qs=path)
                query = {}

                def __init__(self) -> None:
                    self.path = path

                async def post(self):
                    raise AssertionError("request body should not be read without admin context")

            return _Request()

        for path, route, login_url in cases:
            with self.subTest(path=path):
                with self.assertRaises(web.HTTPUnauthorized) as ctx:
                    await route(_make_post_request(path))
                self.assertEqual(ctx.exception.text, "Discord admin authentication required")
                self.assertEqual(ctx.exception.headers.get("X-Auth-Login"), login_url)

        self.assertEqual(handler.manual_plan_set_calls, [])
        self.assertEqual(handler.manual_plan_clear_calls, [])

    def test_live_announcement_is_not_treated_as_admin_context(self) -> None:
        handler = DashboardV2Server.__new__(DashboardV2Server)
        handler._discord_admin_required = True

        live_announcement_request = SimpleNamespace(path="/twitch/live-announcement")
        live_admin_request = SimpleNamespace(path="/twitch/live")

        self.assertFalse(
            DashboardV2Server._should_use_discord_admin_login(handler, live_announcement_request)
        )
        self.assertTrue(
            DashboardV2Server._should_use_discord_admin_login(handler, live_admin_request)
        )

    def test_add_routes_are_post_only(self) -> None:
        app = web.Application()
        handler = DashboardV2Server(
            app_token="admin-secret",
            noauth=False,
            partner_token="partner-secret",
        )
        fake_conn = SimpleNamespace(
            execute=lambda *args, **kwargs: SimpleNamespace(fetchall=lambda: [])
        )
        with patch(
            "bot.dashboard.affiliate.affiliate_mixin.storage.transaction",
            return_value=contextlib.nullcontext(fake_conn),
        ):
            handler.attach(app)

        for path in (
            "/twitch/add_any",
            "/twitch/add_url",
            "/twitch/add_login/{login}",
            "/twitch/admin/chat_action",
        ):
            methods = {route.method for route in app.router.routes() if route.resource.canonical == path}
            self.assertEqual(methods, {"POST"})

    def test_internal_home_changelog_route_is_post_only(self) -> None:
        app = web.Application()
        handler = DashboardV2Server(
            app_token="admin-secret",
            noauth=False,
            partner_token="partner-secret",
        )
        fake_conn = SimpleNamespace(
            execute=lambda *args, **kwargs: SimpleNamespace(fetchall=lambda: [])
        )
        with patch(
            "bot.dashboard.affiliate.affiliate_mixin.storage.transaction",
            return_value=contextlib.nullcontext(fake_conn),
        ):
            handler.attach(app)

        methods = {
            route.method
            for route in app.router.routes()
            if route.resource.canonical == "/twitch/api/v2/internal-home/changelog"
        }
        self.assertEqual(methods, {"POST"})

    async def test_raid_auth_start_uses_session_login_without_admin_gate(self) -> None:
        handler = _DummyRaidAuthRoute()
        handler.dashboard_session = {"twitch_login": "partner_one"}
        request = SimpleNamespace(query={})

        with self.assertRaises(web.HTTPFound) as ctx:
            await handler.raid_auth_start(request)
        self.assertEqual(ctx.exception.location, "https://auth.example/partner_one")
        self.assertEqual(handler.require_token_calls, 0)

    async def test_raid_auth_start_login_override_requires_admin_gate(self) -> None:
        handler = _DummyRaidAuthRoute()
        handler.dashboard_session = {"twitch_login": "partner_one"}
        request = SimpleNamespace(query={"login": "victim"})

        with self.assertRaises(web.HTTPFound) as ctx:
            await handler.raid_auth_start(request)
        self.assertEqual(ctx.exception.location, "https://auth.example/victim")
        self.assertEqual(handler.require_token_calls, 1)

    async def test_raid_auth_start_without_session_starts_public_base_scope(self) -> None:
        handler = _DummyRaidAuthRoute()
        request = SimpleNamespace(query={})

        with self.assertRaises(web.HTTPFound) as ctx:
            await handler.raid_auth_start(request)
        self.assertEqual(
            ctx.exception.location,
            "https://auth.example/public:website_onboarding?scope_profile=base",
        )
        self.assertEqual(handler.require_token_calls, 0)
        self.assertEqual(handler.last_auth_login, "public:website_onboarding")
        self.assertEqual(handler.last_auth_scope_profile, BASE_SCOPE_PROFILE)

    async def test_raid_auth_start_without_session_requires_public_oauth_config(self) -> None:
        handler = _DummyRaidAuthRoute()
        handler._raid_bot.auth_manager.client_id = ""
        request = SimpleNamespace(query={})

        response = await handler.raid_auth_start(request)
        self.assertEqual(response.status, 503)
        self.assertEqual(response.text, "Raid bot OAuth is not configured")
        self.assertEqual(handler.require_token_calls, 0)

    async def test_raid_auth_start_public_onboarding_starts_base_scope_without_session(self) -> None:
        handler = _DummyRaidAuthRoute()
        request = SimpleNamespace(
            query={
                "source": "website_onboarding",
                "scope_profile": "base",
            }
        )

        with self.assertRaises(web.HTTPFound) as ctx:
            await handler.raid_auth_start(request)
        self.assertEqual(
            ctx.exception.location,
            "https://auth.example/public:website_onboarding?scope_profile=base",
        )
        self.assertEqual(handler.require_token_calls, 0)
        self.assertEqual(handler.last_auth_login, "public:website_onboarding")
        self.assertEqual(handler.last_auth_scope_profile, BASE_SCOPE_PROFILE)

    async def test_raid_auth_start_public_onboarding_rejects_non_base_scope_without_session(self) -> None:
        handler = _DummyRaidAuthRoute()
        request = SimpleNamespace(
            query={
                "source": "website_onboarding",
                "scope_profile": "dashboard_reauth",
            }
        )

        with self.assertRaises(web.HTTPFound) as ctx:
            await handler.raid_auth_start(request)
        self.assertEqual(ctx.exception.location, PUBLIC_STREAMER_ONBOARDING_URL)
        self.assertEqual(handler.require_token_calls, 0)

    def test_canonical_post_login_destination_keeps_raid_auth_path(self) -> None:
        self.assertEqual(
            _DashboardAuthMixin._canonical_post_login_destination(
                "/twitch/raid/auth?streamer=earlysalty"
            ),
            "/twitch/raid/auth?streamer=earlysalty",
        )

    def test_canonical_post_login_destination_keeps_live_builder_path(self) -> None:
        self.assertEqual(
            _DashboardAuthMixin._canonical_post_login_destination(
                "/twitch/live-announcement?streamer=earlysalty"
            ),
            "/twitch/live-announcement?streamer=earlysalty",
        )

    def test_rate_limit_key_uses_forwarded_headers_for_trusted_proxy(self) -> None:
        handler = DashboardV2Server.__new__(DashboardV2Server)
        handler._trusted_proxy_networks = DashboardV2Server._parse_proxy_networks()
        request = SimpleNamespace(
            headers={
                "X-Forwarded-For": "203.0.113.99",
                "X-Real-IP": "198.51.100.42",
            },
            remote="127.0.0.1",
            transport=None,
        )

        key = DashboardV2Server._rate_limit_key(handler, request)
        self.assertEqual(key, "203.0.113.99")

    def test_rate_limit_key_ignores_forwarded_headers_for_untrusted_proxy(self) -> None:
        handler = DashboardV2Server.__new__(DashboardV2Server)
        handler._trusted_proxy_networks = ()
        request = SimpleNamespace(
            headers={
                "X-Forwarded-For": "203.0.113.99",
                "X-Real-IP": "198.51.100.42",
            },
            remote="127.0.0.1",
            transport=None,
        )

        key = DashboardV2Server._rate_limit_key(handler, request)
        self.assertEqual(key, "127.0.0.1")

    def test_dashboard_server_uses_auth_mixin_rate_limiter(self) -> None:
        self.assertIs(DashboardV2Server._check_rate_limit, _DashboardAuthMixin._check_rate_limit)

    async def test_discord_admin_membership_fails_closed_without_allowlist(self) -> None:
        handler = DashboardV2Server.__new__(DashboardV2Server)
        handler._discord_admin_owner_user_id = None
        handler._discord_admin_guild_ids = ()
        handler._discord_admin_moderator_role_id = 55
        handler._raid_bot = SimpleNamespace(
            auth_manager=SimpleNamespace(
                _discord_bot=SimpleNamespace(
                    guilds=[SimpleNamespace(id=1)],
                    get_guild=lambda _guild_id: None,
                )
            )
        )

        allowed, reason = await DashboardV2Server._check_discord_admin_membership(handler, 42)

        self.assertFalse(allowed)
        self.assertEqual(reason, "admin_guild_not_configured")

    async def test_live_add_any_requires_csrf(self) -> None:
        handler = _DummyLiveActions()

        class _BadRequest:
            path = "/twitch/add_any"
            method = "POST"
            headers = {}
            rel_url = SimpleNamespace(path_qs="/twitch/add_any")
            query = {}

            async def post(self):
                return {"q": "streamer_a", "csrf_token": "invalid"}

        with self.assertRaises(web.HTTPFound) as bad_ctx:
            await handler.add_any(_BadRequest())
        self.assertEqual(bad_ctx.exception.location, "/twitch/admin?err=csrf")
        self.assertEqual(handler.add_calls, [])

        class _GoodRequest:
            path = "/twitch/add_any"
            method = "POST"
            headers = {}
            rel_url = SimpleNamespace(path_qs="/twitch/add_any")
            query = {}

            async def post(self):
                return {"q": "streamer_a", "csrf_token": "valid-csrf"}

        with self.assertRaises(web.HTTPFound) as good_ctx:
            await handler.add_any(_GoodRequest())
        self.assertEqual(good_ctx.exception.location, "/twitch?ok=streamer_a")
        self.assertEqual(handler.add_calls, ["streamer_a"])

    async def test_owner_chat_action_rejects_non_owner_discord_session(self) -> None:
        handler = _DummyLiveOwnerChatAction(user_id="123456789")

        class _Request:
            path = "/twitch/admin/chat_action"
            method = "POST"
            headers = {}
            rel_url = SimpleNamespace(path_qs="/twitch/admin/chat_action")
            query = {}

            async def post(self):
                return {
                    "login": "partner_one",
                    "mode": "message",
                    "message": "test",
                    "csrf_token": "valid-csrf",
                }

        with self.assertRaises(web.HTTPFound) as ctx:
            await handler.admin_partner_chat_action(_Request())
        self.assertEqual(ctx.exception.location, "/twitch/admin?err=1")
        self.assertEqual(handler.chat_bot.chat_calls, [])

    async def test_owner_chat_action_sends_action_message(self) -> None:
        handler = _DummyLiveOwnerChatAction()

        class _Request:
            path = "/twitch/admin/chat_action"
            method = "POST"
            headers = {}
            rel_url = SimpleNamespace(path_qs="/twitch/admin/chat_action")
            query = {}

            async def post(self):
                return {
                    "login": "partner_one",
                    "mode": "action",
                    "message": "hallo chat",
                    "csrf_token": "valid-csrf",
                }

        with self.assertRaises(web.HTTPFound) as ctx:
            await handler.admin_partner_chat_action(_Request())
        self.assertEqual(ctx.exception.location, "/twitch/admin?ok=1")
        self.assertEqual(len(handler.chat_bot.chat_calls), 1)
        self.assertEqual(handler.chat_bot.chat_calls[0]["login"], "partner_one")
        self.assertEqual(handler.chat_bot.chat_calls[0]["channel_id"], "1001")
        self.assertEqual(handler.chat_bot.chat_calls[0]["text"], "/me hallo chat")
        self.assertEqual(handler.chat_bot.chat_calls[0]["source"], "admin_dashboard_manual")

    async def test_owner_chat_action_resolves_missing_user_id_via_twitch_lookup(self) -> None:
        handler = _DummyLiveOwnerChatAction(missing_channel_id=True, api_user_id="2002")

        class _Request:
            path = "/twitch/admin/chat_action"
            method = "POST"
            headers = {}
            rel_url = SimpleNamespace(path_qs="/twitch/admin/chat_action")
            query = {}

            async def post(self):
                return {
                    "login": "partner_one",
                    "mode": "message",
                    "message": "hallo",
                    "csrf_token": "valid-csrf",
                }

        with self.assertRaises(web.HTTPFound) as ctx:
            await handler.admin_partner_chat_action(_Request())
        self.assertEqual(ctx.exception.location, "/twitch/admin?ok=1")
        self.assertEqual(len(handler.chat_bot.chat_calls), 1)
        self.assertEqual(handler.chat_bot.chat_calls[0]["channel_id"], "2002")
        self.assertEqual(handler.persisted_user_ids, [("partner_one", "2002")])

    async def test_owner_chat_action_rejects_non_partner_streamer(self) -> None:
        handler = _DummyLiveOwnerChatAction(is_partner=False)

        class _Request:
            path = "/twitch/admin/chat_action"
            method = "POST"
            headers = {}
            rel_url = SimpleNamespace(path_qs="/twitch/admin/chat_action")
            query = {}

            async def post(self):
                return {
                    "login": "partner_one",
                    "mode": "message",
                    "message": "test",
                    "csrf_token": "valid-csrf",
                }

        with self.assertRaises(web.HTTPFound) as ctx:
            await handler.admin_partner_chat_action(_Request())
        self.assertEqual(ctx.exception.location, "/twitch/admin?err=1")
        self.assertEqual(handler.chat_bot.chat_calls, [])

    async def test_admin_manual_plan_save_requires_valid_csrf_and_calls_helper(self) -> None:
        handler = _DummyLiveActions()

        class _BadRequest:
            path = "/twitch/admin/manual-plan"
            method = "POST"
            headers = {}
            rel_url = SimpleNamespace(path_qs="/twitch/admin/manual-plan")
            query = {}

            async def post(self):
                return {
                    "login": "partner_one",
                    "plan_id": "analysis_dashboard",
                    "csrf_token": "invalid",
                }

        with self.assertRaises(web.HTTPFound) as bad_ctx:
            await handler.admin_manual_plan_save(_BadRequest())
        self.assertEqual(bad_ctx.exception.location, "/twitch/admin?err=csrf")
        self.assertEqual(handler.manual_plan_set_calls, [])

        class _GoodRequest:
            path = "/twitch/admin/manual-plan"
            method = "POST"
            headers = {}
            rel_url = SimpleNamespace(path_qs="/twitch/admin/manual-plan")
            query = {}

            async def post(self):
                return {
                    "login": "partner_one",
                    "plan_id": "analysis_dashboard",
                    "expires_at": "2026-04-01",
                    "notes": "VIP grant",
                    "csrf_token": "valid-csrf",
                }

        with self.assertRaises(web.HTTPFound) as good_ctx:
            await handler.admin_manual_plan_save(_GoodRequest())
        self.assertEqual(good_ctx.exception.location, "/twitch/admin?ok=1")
        self.assertEqual(
            handler.manual_plan_set_calls,
            [
                {
                    "twitch_login": "partner_one",
                    "plan_id": "analysis_dashboard",
                    "expires_at": "2026-04-01",
                    "notes": "VIP grant",
                }
            ],
        )

    async def test_admin_manual_plan_clear_calls_helper(self) -> None:
        handler = _DummyLiveActions()

        class _Request:
            path = "/twitch/admin/manual-plan/clear"
            method = "POST"
            headers = {}
            rel_url = SimpleNamespace(path_qs="/twitch/admin/manual-plan/clear")
            query = {}

            async def post(self):
                return {
                    "login": "partner_one",
                    "csrf_token": "valid-csrf",
                }

        with self.assertRaises(web.HTTPFound) as ctx:
            await handler.admin_manual_plan_clear(_Request())
        self.assertEqual(ctx.exception.location, "/twitch/admin?ok=1")
        self.assertEqual(handler.manual_plan_clear_calls, ["partner_one"])

    async def test_discord_link_rejects_invalid_csrf(self) -> None:
        class _DiscordLinkHandler(_DummyRoutes):
            def __init__(self) -> None:
                super().__init__()
                self.dashboard_session = {"csrf_token": "valid-csrf"}
                self.saved_login = None

                async def _profile(login, **kwargs):
                    self.saved_login = login
                    return "saved"

                self._discord_profile = _profile

            def _require_token(self, request):
                return None

            def _redirect_location(self, request, *, ok=None, err=None, default_path="/twitch/stats"):
                if err:
                    return "/twitch/stats?err=csrf"
                return "/twitch/stats?ok=1"

            def _safe_internal_redirect(self, location, fallback="/twitch/stats"):
                return location

        handler = _DiscordLinkHandler()

        class _BadRequest:
            async def post(self):
                return {"login": "streamer_a", "csrf_token": "invalid"}

        with self.assertRaises(web.HTTPFound) as bad_ctx:
            await handler.discord_link(_BadRequest())
        self.assertEqual(bad_ctx.exception.location, "/twitch/stats?err=csrf")
        self.assertIsNone(handler.saved_login)

        class _GoodRequest:
            async def post(self):
                return {"login": "streamer_a", "csrf_token": "valid-csrf"}

        with self.assertRaises(web.HTTPFound) as good_ctx:
            await handler.discord_link(_GoodRequest())
        self.assertEqual(good_ctx.exception.location, "/twitch/stats?ok=1")
        self.assertEqual(handler.saved_login, "streamer_a")

    async def test_dashboard_v2_assets_redirect_when_unauthenticated(self) -> None:
        handler = _DummyOverviewAssetsAuth()
        request = SimpleNamespace(match_info={"path": "index.html"})

        with self.assertRaises(web.HTTPFound) as ctx:
            await handler._serve_dashboard_v2_assets(request)
        self.assertEqual(ctx.exception.location, "/twitch/auth/login?next=%2Ftwitch%2Fdashboard-v2")

    async def test_admin_dashboard_assets_route_resolves_dist_assets_directory(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            dist_root = pathlib.Path(tmp_dir)
            asset_path = (dist_root / "assets" / "index.js").resolve()
            asset_path.parent.mkdir(parents=True, exist_ok=True)
            asset_path.write_text("console.log('admin asset');", encoding="utf-8")

            handler = _DummyAdminOverviewAssets(dist_root)
            request = SimpleNamespace(match_info={"path": "index.js"})

            response = await handler._serve_admin_dashboard_assets(request)
            self.assertIsInstance(response, web.FileResponse)
            self.assertEqual(pathlib.Path(response._path), asset_path)

    async def test_admin_dashboard_legacy_path_delegates_to_legacy_dashboard(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            handler = _DummyAdminOverviewAssets(pathlib.Path(tmp_dir))
            request = SimpleNamespace(match_info={"path": "legacy"})
            expected = web.Response(text="legacy")

            with patch(
                "bot.analytics.api_overview.DashboardLiveMixin.index",
                AsyncMock(return_value=expected),
            ) as legacy_index:
                response = await handler._serve_admin_dashboard_path(request)

        self.assertIs(response, expected)
        legacy_index.assert_awaited_once_with(handler, request)

    async def test_dashboard_redirect_when_unauthenticated_uses_dashboard_next(self) -> None:
        handler = _DummyOverviewAssetsAuth()
        request = SimpleNamespace()

        with self.assertRaises(web.HTTPFound) as ctx:
            await handler._serve_dashboard(request)
        self.assertEqual(ctx.exception.location, "/twitch/auth/login?next=%2Ftwitch%2Fdashboard")

    async def test_dashboard_route_on_admin_host_returns_404_without_auth(self) -> None:
        handler = DashboardV2Server(
            app_token=None,
            noauth=False,
            partner_token=None,
            oauth_client_id=None,
            oauth_client_secret=None,
            oauth_redirect_uri="https://twitch.earlysalty.com/twitch/auth/callback",
        )
        request = SimpleNamespace(
            path="/twitch/dashboard",
            headers={"Host": "admin.earlysalty.de"},
            host="admin.earlysalty.de",
            remote="203.0.113.10",
            transport=None,
        )

        response = await handler._serve_dashboard(request)
        self.assertEqual(response.status, 404)

    async def test_dashboard_route_on_admin_host_returns_404_for_partner(self) -> None:
        handler = DashboardV2Server(
            app_token=None,
            noauth=False,
            partner_token=None,
            oauth_client_id=None,
            oauth_client_secret=None,
            oauth_redirect_uri="https://twitch.earlysalty.com/twitch/auth/callback",
        )
        handler._get_dashboard_auth_session = lambda _request: {  # type: ignore[method-assign]
            "twitch_login": "partner_one",
            "twitch_user_id": "1001",
            "display_name": "Partner One",
        }
        request = SimpleNamespace(
            path="/twitch/dashboard",
            headers={"Host": "admin.earlysalty.de"},
            host="admin.earlysalty.de",
            remote="203.0.113.10",
            transport=None,
        )

        response = await handler._serve_dashboard(request)
        self.assertEqual(response.status, 404)

    async def test_dashboard_route_on_admin_host_returns_404_for_admin(self) -> None:
        handler = DashboardV2Server(
            app_token=None,
            noauth=False,
            partner_token=None,
            oauth_client_id=None,
            oauth_client_secret=None,
            oauth_redirect_uri="https://twitch.earlysalty.com/twitch/auth/callback",
        )
        handler._get_dashboard_auth_session = lambda _request: {  # type: ignore[method-assign]
            "auth_type": "discord_admin",
            "discord_user_id": "42",
        }
        request = SimpleNamespace(
            path="/twitch/dashboard",
            headers={"Host": "admin.earlysalty.de"},
            host="admin.earlysalty.de",
            remote="203.0.113.10",
            transport=None,
        )

        response = await handler._serve_dashboard(request)
        self.assertEqual(response.status, 404)

    async def test_dashboard_route_returns_503_when_twitch_oauth_missing(self) -> None:
        handler = DashboardV2Server(
            app_token=None,
            noauth=False,
            partner_token=None,
            oauth_client_id=None,
            oauth_client_secret=None,
            oauth_redirect_uri="https://twitch.earlysalty.com/twitch/auth/callback",
        )
        handler._discord_admin_required = False
        request = SimpleNamespace(
            path="/twitch/dashboard",
            headers={"Host": "dashboard.example"},
            host="dashboard.example",
            remote="203.0.113.10",
            transport=None,
        )

        response = await handler._serve_dashboard(request)
        self.assertEqual(response.status, 503)
        self.assertIn("Twitch OAuth ist aktuell nicht konfiguriert", response.text)

    async def test_dashboard_v2_assets_return_503_when_twitch_oauth_missing(self) -> None:
        handler = DashboardV2Server(
            app_token=None,
            noauth=False,
            partner_token=None,
            oauth_client_id=None,
            oauth_client_secret=None,
            oauth_redirect_uri="https://twitch.earlysalty.com/twitch/auth/callback",
        )
        handler._discord_admin_required = False
        request = SimpleNamespace(
            path="/twitch/dashboard-v2/assets/index.js",
            headers={"Host": "dashboard.example"},
            host="dashboard.example",
            remote="203.0.113.10",
            transport=None,
            match_info={"path": "index.js"},
        )

        response = await handler._serve_dashboard_v2_assets(request)
        self.assertEqual(response.status, 503)
        self.assertIn("Twitch OAuth ist aktuell nicht konfiguriert", response.text)

    async def test_dashboard_route_falls_back_to_discord_login_when_twitch_oauth_missing(self) -> None:
        handler = DashboardV2Server(
            app_token=None,
            noauth=False,
            partner_token=None,
            oauth_client_id=None,
            oauth_client_secret=None,
            oauth_redirect_uri="https://twitch.earlysalty.com/twitch/auth/callback",
        )
        handler._discord_admin_required = True
        request = SimpleNamespace(
            path="/twitch/dashboard",
            headers={"Host": "dashboard.example"},
            host="dashboard.example",
            remote="203.0.113.10",
            transport=None,
        )

        with self.assertRaises(web.HTTPFound) as ctx:
            await handler._serve_dashboard(request)
        self.assertEqual(ctx.exception.location, "/twitch/auth/discord/login?next=%2Ftwitch%2Fdashboard")

    def test_build_dashboard_login_url_falls_back_to_discord_when_twitch_oauth_missing(self) -> None:
        handler = DashboardV2Server(
            app_token=None,
            noauth=False,
            partner_token=None,
            oauth_client_id=None,
            oauth_client_secret=None,
            oauth_redirect_uri="https://twitch.earlysalty.com/twitch/auth/callback",
        )
        handler._discord_admin_required = True
        request = SimpleNamespace(
            path="/twitch/dashboard",
            rel_url=SimpleNamespace(path_qs="/twitch/dashboard"),
        )

        self.assertEqual(
            handler._build_dashboard_login_url(request),
            "/twitch/auth/discord/login?next=%2Ftwitch%2Fdashboard",
        )

    def test_require_v2_auth_uses_discord_login_url_when_twitch_oauth_missing(self) -> None:
        handler = DashboardV2Server(
            app_token=None,
            noauth=False,
            partner_token=None,
            oauth_client_id=None,
            oauth_client_secret=None,
            oauth_redirect_uri="https://twitch.earlysalty.com/twitch/auth/callback",
        )
        handler._discord_admin_required = True
        request = SimpleNamespace(
            path="/twitch/api/v2/internal-home",
            headers={"Host": "dashboard.example"},
            host="dashboard.example",
            remote="203.0.113.10",
            transport=None,
            rel_url=SimpleNamespace(path_qs="/twitch/api/v2/internal-home"),
        )

        with self.assertRaises(web.HTTPUnauthorized) as ctx:
            handler._require_v2_auth(request)
        payload = json.loads(ctx.exception.text)
        self.assertEqual(
            payload.get("loginUrl"),
            "/twitch/auth/discord/login?next=%2Ftwitch%2Fdashboard-v2",
        )

    async def test_stats_entry_returns_503_when_twitch_oauth_missing(self) -> None:
        handler = DashboardV2Server(
            app_token=None,
            noauth=False,
            partner_token=None,
            oauth_client_id=None,
            oauth_client_secret=None,
            oauth_redirect_uri="https://twitch.earlysalty.com/twitch/auth/callback",
        )
        handler._discord_admin_required = False
        request = SimpleNamespace(
            path="/twitch/stats",
            headers={"Host": "dashboard.example"},
            host="dashboard.example",
            remote="203.0.113.10",
            transport=None,
            query_string="",
        )

        response = await handler.stats_entry(request)
        self.assertEqual(response.status, 503)
        self.assertIn("Twitch OAuth ist aktuell nicht konfiguriert", response.text)

    async def test_internal_home_requires_streamer_dashboard_session(self) -> None:
        handler = _DummyInternalHomeApi(dashboard_session=None)
        request = SimpleNamespace(query={})

        with self.assertRaises(web.HTTPUnauthorized) as ctx:
            await handler._api_v2_internal_home(request)
        payload = json.loads(ctx.exception.text)
        self.assertEqual(payload.get("error"), "auth_required")
        self.assertEqual(payload.get("loginUrl"), "/twitch/auth/login?next=%2Ftwitch%2Fdashboard")

    async def test_internal_home_returns_stable_shape_for_session(self) -> None:
        handler = _DummyInternalHomeApi(
            dashboard_session={
                "twitch_login": "Partner_One",
                "twitch_user_id": "1001",
                "display_name": "Partner One",
            }
        )
        request = SimpleNamespace(query={})

        response = await handler._api_v2_internal_home(request)
        payload = json.loads(response.text)

        self.assertEqual(response.status, 200)
        self.assertIn("profile", payload)
        self.assertIn("status", payload)
        self.assertIn("kpis", payload)
        self.assertIn("recent_streams", payload)
        self.assertIn("bot_impact", payload)
        self.assertIn("changelog", payload)
        self.assertIn("links", payload)
        self.assertEqual(payload["status"]["raid_status"]["state"], "active")
        self.assertTrue(payload["status"]["raid_status"]["read_only"])
        self.assertEqual(payload["changelog"]["entries"][0]["id"], 1)
        self.assertFalse(payload["changelog"]["can_write"])
        self.assertEqual(payload["changelog"]["max_entries"], 20)
        self.assertNotIn("target_id", payload["bot_impact"]["events"][0])
        self.assertNotIn("target_id", payload["bot_activity"]["events"][0])
        self.assertEqual(handler.payload_args["twitch_login"], "partner_one")
        self.assertFalse(handler.changelog_can_write)
        self.assertEqual(handler.payload_args["days"], 30)

    async def test_internal_home_keeps_target_id_for_admin_session(self) -> None:
        handler = _DummyInternalHomeApi(
            dashboard_session={
                "twitch_login": "earlysalty",
                "twitch_user_id": "1",
                "display_name": "Admin",
            }
        )
        request = SimpleNamespace(
            headers={"Host": "dashboard.example"},
            host="dashboard.example",
            remote="203.0.113.10",
            transport=None,
            query={},
        )

        response = await handler._api_v2_internal_home(request)
        payload = json.loads(response.text)

        self.assertEqual(response.status, 200)
        self.assertEqual(payload["bot_impact"]["events"][0].get("target_id"), "424242")
        self.assertEqual(payload["bot_activity"]["events"][0].get("target_id"), "424242")

    async def test_internal_home_rejects_discord_admin_without_twitch_binding(self) -> None:
        handler = _DummyInternalHomeApi(
            dashboard_session=None,
            discord_admin_session={"user_id": 77, "display_name": "Admin User"},
        )
        request = SimpleNamespace(query={})

        with self.assertRaises(web.HTTPUnauthorized) as ctx:
            await handler._api_v2_internal_home(request)
        payload = json.loads(ctx.exception.text)
        self.assertEqual(payload.get("error"), "streamer_session_required")
        self.assertNotIn("profile", payload)

    async def test_internal_home_allows_admin_streamer_override_without_twitch_binding(self) -> None:
        handler = _DummyInternalHomeApi(
            dashboard_session=None,
            discord_admin_session={"user_id": 77, "display_name": "Admin User"},
        )
        request = SimpleNamespace(
            headers={"Host": "dashboard.example"},
            host="dashboard.example",
            remote="203.0.113.10",
            transport=None,
            query={"streamer": "partner_two"},
        )

        response = await handler._api_v2_internal_home(request)
        payload = json.loads(response.text)

        self.assertEqual(response.status, 200)
        self.assertEqual(payload["profile"]["twitch_login"], "partner_two")
        self.assertEqual(handler.payload_args["twitch_login"], "partner_two")
        self.assertEqual(handler.payload_args["display_name"], "partner_two")

    async def test_internal_home_rejects_partner_streamer_override(self) -> None:
        handler = _DummyInternalHomeApi(
            dashboard_session={
                "twitch_login": "partner_one",
                "twitch_user_id": "1001",
                "display_name": "Partner One",
            }
        )
        request = SimpleNamespace(
            headers={"Host": "dashboard.example"},
            host="dashboard.example",
            remote="203.0.113.10",
            transport=None,
            query={"streamer": "partner_two"},
        )

        with self.assertRaises(web.HTTPForbidden) as ctx:
            await handler._api_v2_internal_home(request)
        payload = json.loads(ctx.exception.text)
        self.assertEqual(payload.get("error"), "streamer_override_requires_admin")

    async def test_internal_home_changelog_create_rejects_partner_session(self) -> None:
        handler = _DummyInternalHomeApi(
            dashboard_session={
                "twitch_login": "partner_one",
                "twitch_user_id": "1001",
                "display_name": "Partner One",
            }
        )

        class _Request:
            headers = {"Host": "dashboard.example"}
            host = "dashboard.example"
            remote = "203.0.113.10"
            transport = None

            async def json(self):
                return {"content": "Should not write"}

        response = await handler._api_v2_internal_home_changelog_create(_Request())
        payload = json.loads(response.text)

        self.assertEqual(response.status, 403)
        self.assertEqual(payload.get("error"), "admin_required")
        self.assertIsNone(handler.created_changelog_args)

    async def test_internal_home_changelog_create_allows_admin_session(self) -> None:
        handler = _DummyInternalHomeApi(
            dashboard_session={
                "twitch_login": "earlysalty",
                "twitch_user_id": "1",
                "display_name": "Admin",
            }
        )

        class _Request:
            headers = {
                "Host": "dashboard.example",
                "Origin": "http://dashboard.example",
            }
            host = "dashboard.example"
            remote = "203.0.113.10"
            transport = None

            async def json(self):
                return {
                    "title": "Backend",
                    "content": "Stored from admin session.",
                    "entry_date": "2026-03-03",
                }

        response = await handler._api_v2_internal_home_changelog_create(_Request())
        payload = json.loads(response.text)

        self.assertEqual(response.status, 201)
        self.assertEqual(payload.get("id"), 2)
        self.assertEqual(payload.get("entry_date"), "2026-03-03")
        self.assertEqual(payload.get("title"), "Backend")
        self.assertEqual(payload.get("content"), "Stored from admin session.")
        self.assertIsNotNone(handler.created_changelog_args)
        self.assertEqual(handler.created_changelog_args["entry_date"].isoformat(), "2026-03-03")

    async def test_internal_home_changelog_create_rejects_session_without_origin_headers(self) -> None:
        handler = _DummyInternalHomeApi(
            dashboard_session={
                "twitch_login": "earlysalty",
                "twitch_user_id": "1",
                "display_name": "Admin",
            }
        )

        class _Request:
            headers = {"Host": "dashboard.example"}
            host = "dashboard.example"
            remote = "203.0.113.10"
            transport = None

            async def json(self):
                return {"content": "No origin"}

        response = await handler._api_v2_internal_home_changelog_create(_Request())
        payload = json.loads(response.text)

        self.assertEqual(response.status, 403)
        self.assertEqual(payload.get("error"), "csrf_origin_invalid")
        self.assertIsNone(handler.created_changelog_args)

    async def test_internal_home_changelog_create_rejects_cross_origin_session_request(self) -> None:
        handler = _DummyInternalHomeApi(
            dashboard_session={
                "twitch_login": "earlysalty",
                "twitch_user_id": "1",
                "display_name": "Admin",
            }
        )

        class _Request:
            headers = {
                "Host": "dashboard.example",
                "Origin": "https://evil.example",
            }
            host = "dashboard.example"
            remote = "203.0.113.10"
            transport = None

            async def json(self):
                return {"content": "Cross origin"}

        response = await handler._api_v2_internal_home_changelog_create(_Request())
        payload = json.loads(response.text)

        self.assertEqual(response.status, 403)
        self.assertEqual(payload.get("error"), "csrf_origin_invalid")
        self.assertIsNone(handler.created_changelog_args)

    async def test_internal_home_changelog_create_allows_admin_token_without_origin_header(self) -> None:
        handler = _DummyInternalHomeApi(dashboard_session=None)

        class _Request:
            headers = {
                "Host": "dashboard.example",
                "X-Admin-Token": "admin-secret",
            }
            host = "dashboard.example"
            remote = "203.0.113.10"
            transport = None

            async def json(self):
                return {
                    "title": "Token path",
                    "content": "Stored from token-auth API client.",
                    "entry_date": "2026-03-03",
                }

        response = await handler._api_v2_internal_home_changelog_create(_Request())
        payload = json.loads(response.text)

        self.assertEqual(response.status, 201)
        self.assertEqual(payload.get("id"), 2)
        self.assertEqual(payload.get("title"), "Token path")
        self.assertIsNotNone(handler.created_changelog_args)

    async def test_internal_home_changelog_create_applies_rate_limit(self) -> None:
        class _RateLimitedInternalHomeApi(_DummyInternalHomeApi):
            def __init__(self, **kwargs) -> None:
                super().__init__(**kwargs)
                self.rate_limit_args = None

            def _check_rate_limit(self, request, *, max_requests=10, window_seconds=60.0):
                del request
                self.rate_limit_args = (max_requests, window_seconds)
                return False

        handler = _RateLimitedInternalHomeApi(
            dashboard_session={
                "twitch_login": "earlysalty",
                "twitch_user_id": "1",
                "display_name": "Admin",
            }
        )

        class _Request:
            headers = {
                "Host": "dashboard.example",
                "Origin": "http://dashboard.example",
            }
            host = "dashboard.example"
            remote = "203.0.113.10"
            transport = None

            async def json(self):
                return {"content": "Should be rate-limited"}

        response = await handler._api_v2_internal_home_changelog_create(_Request())
        payload = json.loads(response.text)

        self.assertEqual(response.status, 429)
        self.assertEqual(payload.get("error"), "rate_limit_exceeded")
        self.assertEqual(handler.rate_limit_args, (10, 60.0))
        self.assertIsNone(handler.created_changelog_args)

    async def test_live_auth_status_includes_demo_mode_false(self) -> None:
        handler = _DummyV2HeaderOnlyAuth()
        request = SimpleNamespace(
            headers={"Host": "dashboard.example"},
            host="dashboard.example",
            remote="203.0.113.10",
            transport=None,
        )

        response = await handler._api_v2_auth_status(request)
        payload = json.loads(response.text)

        self.assertEqual(response.status, 200)
        self.assertFalse(payload.get("demoMode"))

    def test_demo_auth_status_uses_allowlisted_default_profile(self) -> None:
        from bot.analytics.demo_data import DEFAULT_DEMO_PROFILE, build_demo_auth_status

        payload = build_demo_auth_status()

        self.assertTrue(payload.get("demoMode"))
        self.assertEqual(payload.get("twitchLogin"), DEFAULT_DEMO_PROFILE)

    def test_demo_streamer_allowlist_blocks_untrusted_profile_override(self) -> None:
        from bot.analytics.demo_data import DEFAULT_DEMO_PROFILE, build_demo_payload

        payload = build_demo_payload("overview", streamer="evil_streamer")

        self.assertEqual(payload.get("streamer"), DEFAULT_DEMO_PROFILE)
        self.assertNotEqual(payload.get("streamer"), "evil_streamer")

    def test_demo_ai_fixture_stays_on_allowlisted_profile(self) -> None:
        from bot.analytics.demo_data import DEFAULT_DEMO_PROFILE, build_demo_ai_analysis

        payload = build_demo_ai_analysis(streamer="totally_not_allowed", days=30, game_filter="deadlock")

        self.assertEqual(payload.get("streamer"), DEFAULT_DEMO_PROFILE)
        self.assertEqual(payload.get("gameFilter"), "deadlock")
        self.assertEqual(len(payload.get("points", [])), 10)
        self.assertIn("dataSnapshot", payload)

    def test_demo_streamer_list_contains_multiple_profiles(self) -> None:
        from bot.analytics.demo_data import ALLOWED_DEMO_PROFILES, build_demo_streamers

        payload = build_demo_streamers()

        self.assertEqual([entry["login"] for entry in payload], list(ALLOWED_DEMO_PROFILES))
        self.assertGreaterEqual(len(payload), 3)

    def test_demo_profiles_have_distinct_small_mid_large_scales(self) -> None:
        from bot.analytics.demo_data import ALLOWED_DEMO_PROFILES, build_demo_payload

        overviews = {
            login: build_demo_payload("overview", streamer=login, days=30)
            for login in ALLOWED_DEMO_PROFILES
        }
        viewer_dirs = {
            login: build_demo_payload("viewer-directory", streamer=login, per_page=10)
            for login in ALLOWED_DEMO_PROFILES
        }

        avg_viewers = [overviews[login]["summary"]["avgViewers"] for login in ALLOWED_DEMO_PROFILES]
        viewer_totals = [viewer_dirs[login]["summary"]["totalViewers"] for login in ALLOWED_DEMO_PROFILES]

        self.assertEqual(avg_viewers, sorted(avg_viewers))
        self.assertEqual(viewer_totals, sorted(viewer_totals))
        self.assertGreater(avg_viewers[-1], avg_viewers[0] * 20)
        self.assertGreater(viewer_totals[-1], viewer_totals[0] * 5)

    def test_demo_monthly_stats_honor_month_limit(self) -> None:
        from bot.analytics.demo_data import build_demo_payload

        payload = build_demo_payload("monthly-stats", streamer="midcore_live", months=4)

        self.assertEqual(len(payload), 4)
        self.assertEqual([entry["month"] for entry in payload], [11, 12, 1, 2])

    def test_demo_category_rank_matches_leaderboard_rank(self) -> None:
        from bot.analytics.demo_data import ALLOWED_DEMO_PROFILES, build_demo_payload

        for login in ALLOWED_DEMO_PROFILES:
            overview = build_demo_payload("overview", streamer=login, days=30)
            comparison = build_demo_payload("category-comparison", streamer=login, days=30)
            leaderboard = build_demo_payload(
                "category-leaderboard",
                streamer=login,
                days=30,
                limit=64,
                sort="avg",
            )

            self.assertEqual(overview["categoryRank"], comparison["categoryRank"])
            self.assertEqual(overview["categoryRank"], leaderboard["yourRank"])

    def test_demo_category_leaderboard_honors_limit_and_sort(self) -> None:
        from bot.analytics.demo_data import build_demo_payload

        payload = build_demo_payload(
            "category-leaderboard",
            streamer="megaarena_gg",
            days=30,
            limit=5,
            sort="peak",
        )

        self.assertEqual(len(payload["leaderboard"]), 5)
        peaks = [entry["peakViewers"] for entry in payload["leaderboard"]]
        self.assertEqual(peaks, sorted(peaks, reverse=True))

    def test_demo_rankings_honor_limit(self) -> None:
        from bot.analytics.demo_data import build_demo_payload

        payload = build_demo_payload("rankings", streamer="midcore_live", metric="viewers", limit=3)

        self.assertEqual(len(payload), 3)

    async def test_demo_dashboard_v2_assets_do_not_redirect_when_unauthenticated(self) -> None:
        handler = _DummyOverviewAssetsAuth()
        request = SimpleNamespace(match_info={"path": "missing-demo-asset.js"})

        response = await handler._serve_demo_dashboard_assets(request)
        self.assertEqual(response.status, 404)

    async def test_demo_dashboard_rewrites_asset_prefix_to_demo_path(self) -> None:
        handler = _DummyOverviewAssetsAuth()
        request = SimpleNamespace()

        response = await handler._serve_demo_dashboard(request)
        self.assertEqual(response.status, 200)
        self.assertIn("/twitch/demo/dashboard-v2/assets/", response.text)
        self.assertNotIn("/twitch/dashboard-v2/assets/", response.text)
        self.assertIn('"apiBase":"/twitch/demo/api/v2"', response.text)
        self.assertIn('"demoMode":true', response.text)
        self.assertIn('"allowedDemoProfiles"', response.text)
        self.assertNotIn("window.fetch=function", response.text)

    async def test_social_media_fetch_clips_without_twitch_api_returns_503(self) -> None:
        handler = SocialMediaDashboard(clip_manager=ClipManager(), auth_checker=lambda _req: True)

        class _Request:
            query = {}

            async def json(self):
                return {"streamer": "streamer_a", "limit": 5, "days": 3}

        response = await handler.api_fetch_clips(_Request())
        payload = json.loads(response.text)

        self.assertEqual(response.status, 503)
        self.assertEqual(payload.get("error"), "twitch_api_unavailable")


if __name__ == "__main__":
    unittest.main()
