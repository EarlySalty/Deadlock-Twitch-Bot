import json
import unittest
from types import SimpleNamespace
from unittest.mock import patch

from aiohttp import web

from bot.analytics.api_v2 import AnalyticsV2Mixin
from bot.dashboard.billing_mixin import _DashboardBillingMixin
from bot.dashboard.routes_mixin import _DashboardRoutesMixin


class _FakeCursor:
    def __init__(self, rows):
        self._rows = list(rows)

    def fetchone(self):
        return self._rows[0] if self._rows else None


class _ConnContext:
    def __init__(self, conn):
        self._conn = conn

    def __enter__(self):
        return self._conn

    def __exit__(self, exc_type, exc, tb):
        return False


class _BillingLookupConn:
    def __init__(self):
        self.refs = []

    def execute(self, sql, params=None):
        if "FROM twitch_billing_subscriptions" not in sql:
            raise AssertionError(f"Unexpected SQL in billing lookup test: {sql[:120]}")
        args = tuple(params or ())
        ref = str(args[0] if args else "")
        self.refs.append(ref)
        if ref == "legacy_login":
            return _FakeCursor(
                [
                    {
                        "customer_reference": "legacy_login",
                        "stripe_customer_id": "cus_legacy",
                        "stripe_subscription_id": "sub_legacy",
                        "status": "active",
                    }
                ]
            )
        return _FakeCursor([])


class _BillingLookupHarness(_DashboardBillingMixin):
    def _get_dashboard_auth_session(self, _request):
        return {"twitch_user_id": "12345", "twitch_login": "legacy_login"}

    def _get_discord_admin_session(self, _request):
        return {}

    def _billing_ensure_storage_tables(self, _conn):
        return None


class _ProfileSaveHarness(_DashboardRoutesMixin, _DashboardBillingMixin):
    def __init__(self):
        self.saved_customer_reference = None

    def _check_v2_auth(self, _request):
        return True

    def _should_use_discord_admin_login(self, _request):
        return False

    def _csrf_verify_token(self, _request, _provided_token):
        return True

    def _get_dashboard_auth_session(self, _request):
        return {"twitch_user_id": "12345", "twitch_login": "legacy_login"}

    def _get_discord_admin_session(self, _request):
        return {}

    def _billing_ensure_storage_tables(self, _conn):
        return None

    def _billing_upsert_profile(self, _conn, **kwargs):
        self.saved_customer_reference = kwargs.get("customer_reference")


class _PostRequest:
    def __init__(self, payload):
        self._payload = dict(payload)

    async def post(self):
        return dict(self._payload)


class _StripeSyncHarness(_DashboardRoutesMixin, AnalyticsV2Mixin):
    def __init__(self, auth_level):
        self._auth_level = auth_level
        self.import_calls = 0

    def _get_auth_level(self, _request):
        return self._auth_level

    async def _billing_read_request_body(self, _request):
        return {}

    def _billing_import_stripe(self):
        self.import_calls += 1
        return None, "missing"


class BillingSecurityTests(unittest.IsolatedAsyncioTestCase):
    async def test_customer_reference_lookup_ignores_query_and_uses_session_refs(self) -> None:
        handler = _BillingLookupHarness()
        request = SimpleNamespace(query={"customer_reference": "victim_ref"})
        conn = _BillingLookupConn()

        with patch(
            "bot.dashboard.billing_mixin.storage.readonly_connection",
            return_value=_ConnContext(conn),
        ):
            record = handler._billing_customer_record_for_request(request)

        self.assertEqual(conn.refs, ["12345", "legacy_login"])
        self.assertNotIn("victim_ref", conn.refs)
        self.assertEqual(record["customer_reference"], "legacy_login")
        self.assertEqual(record["stripe_customer_id"], "cus_legacy")

    async def test_profile_save_ignores_form_customer_reference(self) -> None:
        handler = _ProfileSaveHarness()
        request = _PostRequest(
            {
                "csrf_token": "ok",
                "cycle": "1",
                "customer_reference": "victim_ref",
                "recipient_name": "Partner Name",
                "recipient_email": "partner@example.com",
                "street_line1": "Main St 1",
                "postal_code": "12345",
                "city": "Berlin",
                "country_code": "DE",
            }
        )

        with patch(
            "bot.dashboard.abbo_billing_routes.storage.transaction",
            return_value=_ConnContext(object()),
        ):
            with self.assertRaises(web.HTTPFound):
                await handler.abbo_profile_save(request)

        self.assertEqual(handler.saved_customer_reference, "12345")

    async def test_stripe_sync_products_requires_admin(self) -> None:
        handler = _StripeSyncHarness(auth_level="partner")
        response = await handler.api_billing_stripe_sync_products(SimpleNamespace())
        payload = json.loads(response.body.decode("utf-8"))

        self.assertEqual(response.status, 403)
        self.assertEqual(payload.get("error"), "admin_required")
        self.assertEqual(handler.import_calls, 0)

    async def test_stripe_sync_products_allows_admin_flow(self) -> None:
        handler = _StripeSyncHarness(auth_level="admin")
        response = await handler.api_billing_stripe_sync_products(SimpleNamespace())
        payload = json.loads(response.body.decode("utf-8"))

        self.assertEqual(response.status, 503)
        self.assertEqual(payload.get("error"), "stripe_sdk_missing")
        self.assertEqual(payload.get("message"), "stripe_sdk_missing")
        self.assertNotIn("details", payload)
        self.assertEqual(handler.import_calls, 1)


if __name__ == "__main__":
    unittest.main()
