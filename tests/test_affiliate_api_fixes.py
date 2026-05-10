from __future__ import annotations

import importlib
import json
import sqlite3
import sys
import unittest
from datetime import UTC, datetime, timedelta
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

REPO_ROOT = Path(__file__).resolve().parents[1]
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))

from bot.analytics.api_admin import _AnalyticsAdminMixin  # noqa: E402
from bot.analytics.api_v2 import AnalyticsV2Mixin  # noqa: E402


class _CompatSqliteConn:
    def __init__(self, conn: sqlite3.Connection) -> None:
        self._conn = conn

    def execute(self, sql: str, params=None):
        sql_text = str(sql or "").replace("%s", "?")
        return self._conn.execute(sql_text, tuple(params or ()))

    def commit(self) -> None:
        self._conn.commit()

    def __getattr__(self, item):
        return getattr(self._conn, item)


class _ConnCtx:
    def __init__(self, conn: sqlite3.Connection) -> None:
        self._compat = _CompatSqliteConn(conn)

    def __enter__(self) -> _CompatSqliteConn:
        return self._compat

    def __exit__(self, exc_type, exc, tb) -> bool:
        return False


class _AdminRequest(dict):
    def __init__(self, *, login: str, body: dict | None = None, headers: dict | None = None) -> None:
        super().__init__()
        self.match_info = {"login": login}
        self.headers = headers or {}
        self._body = body or {}

    async def json(self):
        return dict(self._body)


class _AdminAffiliateHarness(_AnalyticsAdminMixin):
    def _require_v2_admin_api(self, _request):
        return None

    def _csrf_verify_token(self, _request, token: str) -> bool:
        return token == "csrf-ok"

    async def _api_admin_affiliate_detail(self, request):
        module = importlib.reload(importlib.import_module("bot.analytics.api_admin"))
        return await module._AnalyticsAdminMixin._api_admin_affiliate_detail(self, request)

    async def _api_admin_affiliate_toggle(self, request):
        module = importlib.reload(importlib.import_module("bot.analytics.api_admin"))
        return await module._AnalyticsAdminMixin._api_admin_affiliate_toggle(self, request)

    async def _api_admin_affiliate_stats(self, request):
        module = importlib.reload(importlib.import_module("bot.analytics.api_admin"))
        return await module._AnalyticsAdminMixin._api_admin_affiliate_stats(self, request)


class _AffiliatePortalHarness(AnalyticsV2Mixin):
    def __init__(self, *, auth_level: str = "partner", session: dict | None = None) -> None:
        self._auth_level = auth_level
        self._session = session or {"twitch_login": "affiliate_one", "display_name": "Affiliate One"}

    def _get_auth_level(self, _request):
        return self._auth_level

    def _get_dashboard_session(self, _request):
        return dict(self._session)

    async def _api_v2_affiliate_portal(self, request):
        module = importlib.reload(importlib.import_module("bot.analytics.api_v2"))
        return await module.AnalyticsV2Mixin._api_v2_affiliate_portal(self, request)


class AffiliateApiFixTests(unittest.IsolatedAsyncioTestCase):
    def setUp(self) -> None:
        self.conn = sqlite3.connect(":memory:", check_same_thread=False)
        self.conn.row_factory = sqlite3.Row
        self.conn.execute(
            """
            CREATE TABLE affiliate_accounts (
                twitch_login TEXT PRIMARY KEY,
                display_name TEXT,
                is_active INTEGER,
                created_at TEXT,
                email TEXT,
                stripe_connect_status TEXT,
                stripe_account_id TEXT,
                updated_at TEXT
            )
            """
        )
        self.conn.execute(
            """
            CREATE TABLE affiliate_streamer_claims (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                affiliate_twitch_login TEXT,
                claimed_streamer_login TEXT,
                claimed_at TEXT
            )
            """
        )
        self.conn.execute(
            """
            CREATE TABLE affiliate_commissions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                affiliate_twitch_login TEXT,
                streamer_login TEXT,
                commission_cents INTEGER,
                status TEXT,
                created_at TEXT
            )
            """
        )
        self.conn.execute(
            """
            CREATE TABLE streamer_plans (
                twitch_user_id TEXT,
                twitch_login TEXT,
                manual_plan_id TEXT,
                manual_plan_expires_at TEXT,
                manual_plan_notes TEXT DEFAULT '',
                manual_plan_updated_at TEXT
            )
            """
        )
        self.conn.execute(
            """
            CREATE TABLE twitch_billing_subscriptions (
                customer_reference TEXT,
                plan_id TEXT,
                status TEXT,
                current_period_end TEXT,
                updated_at TEXT
            )
            """
        )
        self.conn.execute(
            """
            CREATE TABLE twitch_streamers (
                twitch_login TEXT,
                display_name TEXT
            )
            """
        )

        now = datetime.now(UTC).replace(microsecond=0)
        month_start = now.replace(day=1, hour=0, minute=0, second=0)
        prior_month = (month_start - timedelta(days=1)).replace(day=10, hour=12, minute=0, second=0)
        self.current_month_iso = (month_start + timedelta(days=1)).isoformat()
        self.prior_month_iso = prior_month.isoformat()
        self.current_commission_iso = (month_start + timedelta(days=2)).isoformat()
        self.last_month_commission_iso = (month_start - timedelta(days=2)).isoformat()

        self.conn.execute(
            """
            INSERT INTO affiliate_accounts (
                twitch_login, display_name, is_active, created_at, email,
                stripe_connect_status, stripe_account_id, updated_at
            )
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            """,
            (
                "affiliate_one",
                "Affiliate One",
                1,
                now.isoformat(),
                "affiliate@example.com",
                "connected",
                "acct_1234567890",
                now.isoformat(),
            ),
        )
        self.conn.executemany(
            """
            INSERT INTO affiliate_streamer_claims (
                affiliate_twitch_login, claimed_streamer_login, claimed_at
            ) VALUES (?, ?, ?)
            """,
            [
                ("affiliate_one", "customer_alpha", self.prior_month_iso),
                ("affiliate_one", "customer_beta", self.current_month_iso),
            ],
        )
        self.conn.executemany(
            """
            INSERT INTO affiliate_commissions (
                affiliate_twitch_login, streamer_login, commission_cents, status, created_at
            ) VALUES (?, ?, ?, ?, ?)
            """,
            [
                ("affiliate_one", "customer_alpha", 500, "pending", self.current_commission_iso),
                ("affiliate_one", "customer_alpha", 700, "failed", self.current_commission_iso),
                ("affiliate_one", "customer_beta", 300, "transferred", self.last_month_commission_iso),
                ("affiliate_one", "customer_beta", 200, "skipped", self.current_commission_iso),
            ],
        )
        self.conn.executemany(
            "INSERT INTO twitch_streamers (twitch_login, display_name) VALUES (?, ?)",
            [
                ("customer_alpha", "Customer Alpha"),
                ("customer_beta", "Customer Beta"),
            ],
        )
        self.conn.execute(
            """
            INSERT INTO streamer_plans (
                twitch_user_id, twitch_login, manual_plan_id, manual_plan_expires_at, manual_plan_updated_at
            )
            VALUES (?, ?, ?, ?, ?)
            """,
            ("cust_alpha_id", "customer_alpha", "raid_boost", None, now.isoformat()),
        )
        self.conn.commit()

    def tearDown(self) -> None:
        self.conn.close()

    async def test_admin_toggle_requires_valid_csrf(self) -> None:
        handler = _AdminAffiliateHarness()
        request = _AdminRequest(login="affiliate_one", body={"csrf_token": "bad"})

        with patch("bot.storage.pg.transaction", return_value=_ConnCtx(self.conn)):
            response = await handler._api_admin_affiliate_toggle(request)

        payload = json.loads(response.body.decode("utf-8"))
        self.assertEqual(response.status, 403)
        self.assertEqual(payload["error"], "invalid_csrf")
        row = self.conn.execute(
            "SELECT is_active FROM affiliate_accounts WHERE twitch_login = ?",
            ("affiliate_one",),
        ).fetchone()
        self.assertEqual(row["is_active"], 1)

    async def test_admin_affiliate_detail_and_stats_use_claim_rows_for_counts(self) -> None:
        handler = _AdminAffiliateHarness()

        with patch("bot.storage.pg.transaction", return_value=_ConnCtx(self.conn)):
            detail_response = await handler._api_admin_affiliate_detail(
                _AdminRequest(login="affiliate_one")
            )
            stats_response = await handler._api_admin_affiliate_stats(SimpleNamespace())

        detail_payload = json.loads(detail_response.body.decode("utf-8"))
        stats_payload = json.loads(stats_response.body.decode("utf-8"))

        self.assertEqual(detail_response.status, 200)
        self.assertEqual(detail_payload["stats"]["total_claims"], 2)
        self.assertEqual(detail_payload["stats"]["total_provision"], 8.0)
        self.assertEqual(detail_payload["stats"]["avg_provision"], 4.0)

        alpha_claim = next(
            item for item in detail_payload["claims"] if item["customer_login"] == "customer_alpha"
        )
        beta_claim = next(
            item for item in detail_payload["claims"] if item["customer_login"] == "customer_beta"
        )
        self.assertEqual(alpha_claim["commission_count"], 1)
        self.assertEqual(alpha_claim["commission_cents"], 500)
        self.assertEqual(beta_claim["commission_count"], 1)
        self.assertEqual(beta_claim["commission_cents"], 300)
        self.assertIn("claimed_at", alpha_claim)

        self.assertEqual(stats_response.status, 200)
        self.assertEqual(stats_payload["total_claims"], 2)
        self.assertEqual(stats_payload["this_month_claims"], 1)
        self.assertEqual(stats_payload["total_provision"], 8.0)
        self.assertEqual(stats_payload["this_month_provision"], 5.0)

    async def test_affiliate_portal_returns_claim_centric_stats_and_pending_payout(self) -> None:
        handler = _AffiliatePortalHarness()

        with patch("bot.storage.pg.readonly_connection", return_value=_ConnCtx(self.conn)):
            response = await handler._api_v2_affiliate_portal(SimpleNamespace())

        payload = json.loads(response.body.decode("utf-8"))
        self.assertEqual(response.status, 200)
        self.assertEqual(payload["affiliate"]["login"], "affiliate_one")
        self.assertEqual(
            payload["affiliate"]["referral_url"],
            "https://www.twitch.tv/affiliate_one?ref=DE-Deadlock-Discord",
        )
        self.assertEqual(payload["stats"]["total_claims"], 2)
        self.assertEqual(payload["stats"]["this_month_claims"], 1)
        self.assertEqual(payload["stats"]["total_provision"], 8.0)
        self.assertEqual(payload["stats"]["this_month_provision"], 5.0)
        self.assertEqual(payload["stats"]["pending_payout"], 5.0)
        self.assertEqual(len(payload["recent_claims"]), 2)
        self.assertEqual(payload["recent_claims"][0]["customer_display_name"], "Customer Beta")
        self.assertEqual(payload["recent_claims"][0]["amount"], 3.0)
        self.assertEqual(payload["recent_claims"][1]["customer_display_name"], "Customer Alpha")
        self.assertEqual(payload["recent_claims"][1]["amount"], 5.0)
        self.assertEqual(payload["recent_claims"][1]["plan_name"], "Basic")

    async def test_affiliate_portal_returns_not_found_for_non_affiliate(self) -> None:
        handler = _AffiliatePortalHarness(
            session={"twitch_login": "missing_affiliate", "display_name": "Missing Affiliate"}
        )

        with patch("bot.storage.pg.readonly_connection", return_value=_ConnCtx(self.conn)):
            response = await handler._api_v2_affiliate_portal(SimpleNamespace())

        payload = json.loads(response.body.decode("utf-8"))
        self.assertEqual(response.status, 404)
        self.assertEqual(payload["error"], "not_found")


if __name__ == "__main__":
    unittest.main()
