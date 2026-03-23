from __future__ import annotations

import contextlib
import sqlite3
import unittest
from types import SimpleNamespace
from unittest.mock import patch

from aiohttp import web
from multidict import MultiDict

from bot.dashboard.routes_mixin import _DashboardRoutesMixin


class _ConnCtx:
    def __init__(self, conn: sqlite3.Connection) -> None:
        self._conn = conn

    def __enter__(self) -> _CompatSqliteConn:
        return _CompatSqliteConn(self._conn)

    def __exit__(self, exc_type, exc, tb) -> bool:
        return False


class _CompatSqliteConn:
    def __init__(self, conn: sqlite3.Connection) -> None:
        self._conn = conn

    def execute(self, sql: str, params=None):
        sql_text = str(sql or "").replace("%s", "?")
        return self._conn.execute(sql_text, tuple(params or ()))

    def __getattr__(self, item):
        return getattr(self._conn, item)


class _PostRequest(SimpleNamespace):
    def __init__(self, body) -> None:
        super().__init__(query={})
        self._body = body

    async def post(self):
        return self._body


class _AbboHarness(_DashboardRoutesMixin):
    def __init__(self, plan_id: str, session: dict | None = None) -> None:
        self.plan_id = plan_id
        self.session = session or {"twitch_user_id": "12345", "twitch_login": "partner_one"}

    def _check_v2_auth(self, _request):
        return True

    def _should_use_discord_admin_login(self, _request):
        return False

    def _dashboard_auth_redirect_or_unavailable(self, request, next_path, fallback_login_url):
        del request, next_path, fallback_login_url
        return web.HTTPFound("/")

    def _csrf_generate_token(self, _request):
        return "csrf-ok"

    def _csrf_verify_token(self, _request, token: str) -> bool:
        return token == "csrf-ok"

    def _is_discord_admin_request(self, _request):
        return False

    def _is_local_request(self, _request):
        return False

    def _billing_customer_record_for_request(self, _request):
        return {}

    def _billing_profile_for_request(self, _request):
        return {
            "recipient_name": "Partner Name",
            "recipient_email": "partner@example.com",
            "street_line1": "Main St 1",
            "postal_code": "12345",
            "city": "Berlin",
            "country_code": "DE",
            "company_name": "",
            "vat_id": "",
        }

    def _billing_profile_from_stripe_customer(self, _stripe_customer_id: str):
        return {}

    def _billing_prefill_profile_from_stripe(self, billing_profile, stripe_profile):
        del stripe_profile
        return billing_profile, []

    def _billing_current_plan_for_request(self, _request):
        return {"plan_id": self.plan_id}

    def _billing_plan_name_from_id(self, plan_id: str) -> str:
        return {
            "raid_boost": "raid_boost",
            "analysis_dashboard": "analysis",
            "bundle_analysis_raid_boost": "bundle",
        }.get(plan_id, "free")

    def _billing_ensure_streamer_plan_columns(self, _conn):
        return None

    def _get_dashboard_auth_session(self, _request):
        return dict(self.session)


class DashboardLurkerTaxTests(unittest.IsolatedAsyncioTestCase):
    def setUp(self) -> None:
        self.conn = sqlite3.connect(":memory:")
        self.conn.row_factory = sqlite3.Row
        self.conn.execute(
            """
            CREATE TABLE streamer_plans (
                twitch_user_id TEXT PRIMARY KEY,
                twitch_login TEXT,
                plan_name TEXT NOT NULL DEFAULT 'free',
                lurker_tax_enabled INTEGER NOT NULL DEFAULT 0,
                promo_disabled INTEGER NOT NULL DEFAULT 0,
                promo_message TEXT,
                manual_plan_id TEXT,
                manual_plan_expires_at TEXT
            )
            """
        )
        self.conn.execute(
            """
            CREATE TABLE twitch_raid_auth (
                twitch_user_id TEXT,
                twitch_login TEXT,
                scopes TEXT
            )
            """
        )
        self.conn.execute(
            """
            INSERT INTO streamer_plans (twitch_user_id, twitch_login, plan_name, lurker_tax_enabled)
            VALUES (?, ?, ?, ?)
            """,
            ("12345", "partner_one", "raid_boost", 0),
        )
        self.conn.commit()

    def tearDown(self) -> None:
        self.conn.close()

    def _conn_patch(self):
        stack = contextlib.ExitStack()
        compat = _ConnCtx(self.conn)
        stack.enter_context(
            patch("bot.dashboard.abbo_routes.storage.readonly_connection", return_value=compat)
        )
        stack.enter_context(
            patch("bot.dashboard.abbo_routes.storage.transaction", return_value=compat)
        )
        return stack

    async def test_abbo_entry_shows_locked_teaser_for_free_plan(self) -> None:
        handler = _AbboHarness(plan_id="raid_free")

        with self._conn_patch():
            response = await handler.abbo_entry(SimpleNamespace(query={}))

        html = response.text
        self.assertIn("Lurker Steuer", html)
        self.assertIn("Verf\u00fcgbar in Raid Boost, Analyse Dashboard und im Bundle.", html)
        self.assertNotIn("/twitch/abbo/lurker-tax-settings", html)

    async def test_abbo_entry_shows_toggle_and_scope_warning_for_paid_plan(self) -> None:
        handler = _AbboHarness(plan_id="raid_boost")
        self.conn.execute(
            "UPDATE streamer_plans SET lurker_tax_enabled = 1 WHERE twitch_user_id = ?",
            ("12345",),
        )
        self.conn.commit()

        with self._conn_patch():
            response = await handler.abbo_entry(SimpleNamespace(query={}))

        html = response.text
        self.assertIn("/twitch/abbo/lurker-tax-settings", html)
        self.assertIn("moderator:read:chatters", html)
        self.assertIn("checked", html)
        self.assertNotIn("/twitch/abbo/promo-settings", html)

    async def test_abbo_entry_shows_promo_toggle_only_for_bundle_entitlement(self) -> None:
        handler = _AbboHarness(plan_id="bundle_analysis_raid_boost")

        with self._conn_patch():
            response = await handler.abbo_entry(SimpleNamespace(query={}))

        html = response.text
        self.assertIn("/twitch/abbo/promo-settings", html)
        self.assertIn("Chat-Promos", html)

    async def test_abbo_entry_keeps_warning_when_bot_scope_is_unknown(self) -> None:
        handler = _AbboHarness(plan_id="raid_boost")
        handler._raid_bot = SimpleNamespace(
            chat_bot=SimpleNamespace(
                _token_manager=SimpleNamespace(
                    scopes=set(),
                    bot_id=None,
                    expires_at=None,
                )
            )
        )

        with self._conn_patch():
            response = await handler.abbo_entry(SimpleNamespace(query={}))

        html = response.text
        self.assertIn("Bot-Scope <code>moderator:read:chatters</code> fehlt", html)

    async def test_abbo_entry_login_only_lookup_ignores_unrelated_blank_user_id_rows(self) -> None:
        handler = _AbboHarness(
            plan_id="raid_boost",
            session={"twitch_user_id": "", "twitch_login": "blank_target"},
        )
        self.conn.execute(
            """
            INSERT INTO streamer_plans (
                twitch_user_id, twitch_login, plan_name, lurker_tax_enabled, promo_message
            ) VALUES (?, ?, ?, ?, ?)
            """,
            (None, "other_blank", "analysis", 1, "wrong promo"),
        )
        self.conn.execute(
            """
            INSERT INTO streamer_plans (
                twitch_user_id, twitch_login, plan_name, lurker_tax_enabled, promo_message
            ) VALUES (?, ?, ?, ?, ?)
            """,
            (None, "blank_target", "analysis", 0, "target promo"),
        )
        self.conn.execute(
            """
            INSERT INTO twitch_raid_auth (twitch_user_id, twitch_login, scopes)
            VALUES (?, ?, ?)
            """,
            (None, "other_blank", "moderator:read:chatters"),
        )
        self.conn.execute(
            """
            INSERT INTO twitch_raid_auth (twitch_user_id, twitch_login, scopes)
            VALUES (?, ?, ?)
            """,
            (None, "blank_target", "chat:read chat:edit"),
        )
        self.conn.commit()

        with self._conn_patch():
            response = await handler.abbo_entry(SimpleNamespace(query={}))

        html = response.text
        self.assertIn("target promo", html)
        self.assertNotIn("wrong promo", html)
        self.assertIn("moderator:read:chatters", html)
        self.assertNotIn("checked", html)

    async def test_post_lurker_tax_settings_saves_flag_for_paid_plan(self) -> None:
        handler = _AbboHarness(plan_id="analysis_dashboard")
        request = _PostRequest(
            MultiDict(
                [
                    ("lurker_tax_enabled", "0"),
                    ("lurker_tax_enabled", "1"),
                    ("csrf_token", "csrf-ok"),
                ]
            )
        )

        with self._conn_patch():
            with self.assertRaises(web.HTTPFound) as ctx:
                await handler.abbo_lurker_tax_settings(request)

        saved = self.conn.execute(
            "SELECT lurker_tax_enabled FROM streamer_plans WHERE twitch_user_id = ?",
            ("12345",),
        ).fetchone()
        self.assertEqual(ctx.exception.location, "/twitch/abbo?lurker_tax=saved")
        self.assertEqual(saved["lurker_tax_enabled"], 1)

    async def test_post_lurker_tax_settings_ignores_free_plan(self) -> None:
        handler = _AbboHarness(plan_id="raid_free")
        request = _PostRequest({"lurker_tax_enabled": "1", "csrf_token": "csrf-ok"})

        with self._conn_patch():
            with self.assertRaises(web.HTTPFound) as ctx:
                await handler.abbo_lurker_tax_settings(request)

        saved = self.conn.execute(
            "SELECT lurker_tax_enabled FROM streamer_plans WHERE twitch_user_id = ?",
            ("12345",),
        ).fetchone()
        self.assertEqual(ctx.exception.location, "/twitch/abbo")
        self.assertEqual(saved["lurker_tax_enabled"], 0)

    async def test_post_lurker_tax_settings_invalid_flag_defaults_to_disabled(self) -> None:
        handler = _AbboHarness(plan_id="analysis_dashboard")
        request = _PostRequest(MultiDict([("lurker_tax_enabled", "abc"), ("csrf_token", "csrf-ok")]))

        with self._conn_patch():
            with self.assertRaises(web.HTTPFound) as ctx:
                await handler.abbo_lurker_tax_settings(request)

        saved = self.conn.execute(
            "SELECT lurker_tax_enabled FROM streamer_plans WHERE twitch_user_id = ?",
            ("12345",),
        ).fetchone()
        self.assertEqual(ctx.exception.location, "/twitch/abbo?lurker_tax=saved")
        self.assertEqual(saved["lurker_tax_enabled"], 0)

    async def test_post_lurker_tax_settings_returns_error_on_write_failure(self) -> None:
        handler = _AbboHarness(plan_id="analysis_dashboard")
        request = _PostRequest(MultiDict([("lurker_tax_enabled", "1"), ("csrf_token", "csrf-ok")]))

        with patch.object(handler, "_abbo_upsert_lurker_tax_setting", side_effect=RuntimeError("boom")):
            with self.assertRaises(web.HTTPFound) as ctx:
                await handler.abbo_lurker_tax_settings(request)

        saved = self.conn.execute(
            "SELECT lurker_tax_enabled FROM streamer_plans WHERE twitch_user_id = ?",
            ("12345",),
        ).fetchone()
        self.assertEqual(ctx.exception.location, "/twitch/abbo?lurker_tax=error")
        self.assertEqual(saved["lurker_tax_enabled"], 0)

    async def test_post_promo_settings_saves_flag_for_bundle_plan(self) -> None:
        handler = _AbboHarness(plan_id="bundle_analysis_raid_boost")
        request = _PostRequest(
            MultiDict(
                [
                    ("promo_disabled", "0"),
                    ("promo_disabled", "1"),
                    ("csrf_token", "csrf-ok"),
                ]
            )
        )

        with self._conn_patch():
            with self.assertRaises(web.HTTPFound) as ctx:
                await handler.abbo_promo_settings(request)

        saved = self.conn.execute(
            "SELECT promo_disabled FROM streamer_plans WHERE twitch_user_id = ?",
            ("12345",),
        ).fetchone()
        self.assertEqual(ctx.exception.location, "/twitch/abbo?profile=saved")
        self.assertEqual(saved["promo_disabled"], 1)

    async def test_post_promo_settings_ignores_non_bundle_plan(self) -> None:
        handler = _AbboHarness(plan_id="analysis_dashboard")
        request = _PostRequest({"promo_disabled": "1", "csrf_token": "csrf-ok"})

        with self._conn_patch():
            with self.assertRaises(web.HTTPFound) as ctx:
                await handler.abbo_promo_settings(request)

        saved = self.conn.execute(
            "SELECT promo_disabled FROM streamer_plans WHERE twitch_user_id = ?",
            ("12345",),
        ).fetchone()
        self.assertEqual(ctx.exception.location, "/twitch/abbo")
        self.assertEqual(saved["promo_disabled"], 0)

    async def test_post_promo_settings_rejects_invalid_csrf(self) -> None:
        handler = _AbboHarness(plan_id="bundle_analysis_raid_boost")
        request = _PostRequest({"promo_disabled": "1", "csrf_token": "bad-token"})

        with self._conn_patch():
            response = await handler.abbo_promo_settings(request)

        saved = self.conn.execute(
            "SELECT promo_disabled FROM streamer_plans WHERE twitch_user_id = ?",
            ("12345",),
        ).fetchone()
        self.assertEqual(response.status, 403)
        self.assertEqual(response.text, '{"error": "csrf_token_invalid"}')
        self.assertEqual(saved["promo_disabled"], 0)

    async def test_post_lurker_tax_settings_returns_error_when_login_only_row_is_missing(self) -> None:
        handler = _AbboHarness(
            plan_id="analysis_dashboard",
            session={"twitch_user_id": "", "twitch_login": "missing_partner"},
        )
        request = _PostRequest(MultiDict([("lurker_tax_enabled", "1"), ("csrf_token", "csrf-ok")]))

        with self._conn_patch():
            with self.assertRaises(web.HTTPFound) as ctx:
                await handler.abbo_lurker_tax_settings(request)

        self.assertEqual(ctx.exception.location, "/twitch/abbo?lurker_tax=error")

    async def test_abbo_entry_shows_error_notice_for_failed_lurker_tax_save(self) -> None:
        handler = _AbboHarness(plan_id="analysis_dashboard")

        with self._conn_patch():
            response = await handler.abbo_entry(SimpleNamespace(query={"lurker_tax": "error"}))

        self.assertIn("Lurker Steuer Einstellung konnte nicht gespeichert werden.", response.text)


if __name__ == "__main__":
    unittest.main()
