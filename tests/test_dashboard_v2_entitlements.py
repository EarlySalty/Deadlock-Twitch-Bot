import json
import unittest
from types import SimpleNamespace
from unittest.mock import patch

from aiohttp import web

from bot.analytics.api_v2 import AnalyticsV2Mixin


class _DummyAuthStatusHandler(AnalyticsV2Mixin):
    def __init__(self) -> None:
        self._session = {
            "twitch_login": "partner_one",
            "display_name": "Partner One",
        }

    def _get_auth_level(self, _request):
        return "partner"

    def _get_dashboard_session(self, _request):
        return dict(self._session)

    def _csrf_get_token(self, _request):
        return "csrf-token"


class _DummyBillingCatalogHandler(_DummyAuthStatusHandler):
    pass


class _DummyExtendedGateHandler(AnalyticsV2Mixin):
    def _get_auth_level(self, _request):
        return "partner"

    def _get_dashboard_session(self, _request):
        return {"twitch_login": "partner_one"}


class DashboardV2EntitlementTests(unittest.IsolatedAsyncioTestCase):
    async def test_auth_status_includes_resolved_entitlements(self) -> None:
        handler = _DummyAuthStatusHandler()
        request = SimpleNamespace()

        with patch(
            "bot.analytics.api_v2._resolve_plan_snapshot_for_login",
            return_value={
                "plan_id": "analysis_dashboard",
                "plan_name": "Erweitert",
                "tier": "extended",
                "is_extended": True,
                "entitlements": [
                    "analytics.basic",
                    "analytics.extended",
                    "chat.lurker_tax",
                ],
                "expires_at": None,
                "source": "billing_subscription",
            },
        ):
            response = await handler._api_v2_auth_status(request)

        payload = json.loads(response.text)
        self.assertEqual(response.status, 200)
        self.assertEqual(payload["plan"]["planId"], "analysis_dashboard")
        self.assertEqual(
            payload["plan"]["entitlements"],
            ["analytics.basic", "analytics.extended", "chat.lurker_tax"],
        )
        self.assertEqual(payload["plan"]["source"], "billing_subscription")

    async def test_billing_catalog_includes_entitlements(self) -> None:
        handler = _DummyBillingCatalogHandler()
        request = SimpleNamespace()

        with patch(
            "bot.analytics.api_v2._resolve_plan_snapshot_for_login",
            return_value={
                "plan_id": "raid_boost",
                "plan_name": "Basic",
                "tier": "basic",
                "is_extended": False,
                "entitlements": ["analytics.basic", "chat.lurker_tax", "raid.priority"],
                "expires_at": None,
                "source": "billing_subscription",
            },
        ):
            response = await handler._api_v2_billing_catalog(request)

        payload = json.loads(response.text)
        self.assertEqual(response.status, 200)
        basic_plan = next(plan for plan in payload["plans"] if plan["id"] == "raid_boost")
        self.assertEqual(
            basic_plan["entitlements"],
            ["analytics.ai_mini", "analytics.basic", "chat.lurker_tax", "raid.priority"],
        )
        self.assertTrue(basic_plan["is_current"])

    def test_require_extended_plan_returns_required_entitlements_contract(self) -> None:
        handler = _DummyExtendedGateHandler()
        request = SimpleNamespace(query={"streamer": "other_streamer"})

        with patch(
            "bot.analytics.api_v2._resolve_plan_snapshot_for_login",
            return_value={
                "plan_id": "raid_boost",
                "entitlements": ["analytics.basic", "chat.lurker_tax", "raid.priority"],
            },
        ):
            with self.assertRaises(web.HTTPForbidden) as ctx:
                handler._require_extended_plan(request)

        payload = json.loads(ctx.exception.text)
        self.assertEqual(payload["error"], "plan_required")
        self.assertEqual(payload["required_entitlements"], ["analytics.extended"])
        self.assertIn("analysis_dashboard", payload["required_plans"])
        self.assertIn("bundle_analysis_raid_boost", payload["required_plans"])


if __name__ == "__main__":
    unittest.main()
