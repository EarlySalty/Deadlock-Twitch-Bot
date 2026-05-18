import contextlib
import sqlite3
import unittest
from datetime import UTC, datetime, timedelta
from types import SimpleNamespace
from unittest.mock import patch

from bot.analytics.api_v2 import _get_plan_details_for_login, _get_plan_for_login
from bot.dashboard.billing_mixin import _DashboardBillingMixin


class _CompatSqliteConn:
    def __init__(self, conn: sqlite3.Connection) -> None:
        self._conn = conn

    def execute(self, sql: str, params=None):
        sql_text = str(sql or "").replace("%s", "?")
        return self._conn.execute(sql_text, tuple(params or ()))

    def executemany(self, sql: str, params=None):
        sql_text = str(sql or "").replace("%s", "?")
        return self._conn.executemany(sql_text, params or ())

    def commit(self) -> None:
        self._conn.commit()

    def rollback(self) -> None:
        self._conn.rollback()

    def __getattr__(self, item):
        return getattr(self._conn, item)


class _ConnContext:
    def __init__(self, conn: sqlite3.Connection):
        self._conn = _CompatSqliteConn(conn)

    def __enter__(self):
        return self._conn

    def __exit__(self, exc_type, exc, tb):
        return False


def _build_conn() -> sqlite3.Connection:
    conn = sqlite3.connect(":memory:")
    conn.row_factory = sqlite3.Row
    conn.execute(
        """
        CREATE TABLE twitch_streamers (
            twitch_user_id TEXT PRIMARY KEY,
            twitch_login TEXT NOT NULL,
            manual_partner_opt_out INTEGER NOT NULL DEFAULT 0,
            archived_at TEXT,
            is_on_discord INTEGER NOT NULL DEFAULT 0
        )
        """
    )
    conn.execute(
        """
        CREATE VIEW twitch_streamers_partner_state AS
        SELECT
            twitch_user_id,
            twitch_login,
            manual_partner_opt_out,
            archived_at,
            is_on_discord
        FROM twitch_streamers
        """
    )
    conn.execute(
        """
        CREATE TABLE streamer_plans (
            twitch_user_id TEXT PRIMARY KEY,
            twitch_login TEXT,
            plan_name TEXT NOT NULL DEFAULT 'free',
            promo_disabled INTEGER NOT NULL DEFAULT 0,
            activated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            expires_at TEXT,
            notes TEXT,
            raid_boost_enabled INTEGER NOT NULL DEFAULT 0,
            promo_message TEXT,
            manual_plan_id TEXT,
            manual_plan_expires_at TEXT,
            manual_plan_notes TEXT NOT NULL DEFAULT '',
            manual_plan_updated_at TEXT
        )
        """
    )
    conn.execute(
        """
        CREATE TABLE twitch_billing_subscriptions (
            stripe_subscription_id TEXT PRIMARY KEY,
            customer_reference TEXT,
            status TEXT,
            plan_id TEXT,
            updated_at TEXT
        )
        """
    )
    conn.execute(
        """
        INSERT INTO twitch_streamers (twitch_user_id, twitch_login, is_on_discord)
        VALUES ('12345', 'legacy_login', 1)
        """
    )
    conn.commit()
    return conn


class _ManualPlanHarness(_DashboardBillingMixin):
    def _get_dashboard_auth_session(self, _request):
        return {"twitch_user_id": "12345", "twitch_login": "legacy_login"}

    def _get_discord_admin_session(self, _request):
        return {}

    def _billing_ensure_storage_tables(self, _conn):
        return None

    def _billing_ensure_streamer_plan_columns(self, _conn):
        return None


class ManualPlanOverrideTests(unittest.TestCase):
    def setUp(self) -> None:
        self.conn = _build_conn()
        self.handler = _ManualPlanHarness()

    def tearDown(self) -> None:
        self.conn.close()

    def _patch_billing_conn(self):
        stack = contextlib.ExitStack()
        stack.enter_context(
            patch(
                "bot.dashboard.billing.billing_mixin.storage.readonly_connection",
                return_value=_ConnContext(self.conn),
            )
        )
        stack.enter_context(
            patch(
                "bot.dashboard.billing.billing_mixin.storage.transaction",
                return_value=_ConnContext(self.conn),
            )
        )
        return stack

    def _patch_api_v2_conn(self):
        return patch(
            "bot.entitlements.resolver.storage.readonly_connection",
            return_value=_ConnContext(self.conn),
        )

    def test_current_plan_prefers_active_manual_override(self) -> None:
        now_iso = datetime.now(UTC).isoformat()
        self.conn.execute(
            """
            INSERT INTO streamer_plans (
                twitch_user_id,
                twitch_login,
                manual_plan_id,
                manual_plan_notes,
                manual_plan_updated_at
            ) VALUES (?, ?, ?, ?, ?)
            """,
            ("12345", "legacy_login", "analysis_dashboard", "manual grant", now_iso),
        )
        self.conn.execute(
            """
            INSERT INTO twitch_billing_subscriptions (
                stripe_subscription_id,
                customer_reference,
                status,
                plan_id,
                updated_at
            ) VALUES (?, ?, ?, ?, ?)
            """,
            ("sub_123", "legacy_login", "active", "raid_boost", now_iso),
        )
        self.conn.commit()

        with self._patch_billing_conn():
            current_plan = self.handler._billing_current_plan_for_request(SimpleNamespace())

        self.assertEqual(current_plan["plan_id"], "analysis_dashboard")
        self.assertEqual(current_plan["source"], "manual_override")
        self.assertEqual(current_plan["manual_override"]["notes"], "manual grant")
        self.assertEqual(current_plan["billing_subscription"]["plan_id"], "raid_boost")

    def test_current_plan_falls_back_to_billing_when_manual_override_expired(self) -> None:
        now_iso = datetime.now(UTC).isoformat()
        expired_iso = (datetime.now(UTC) - timedelta(days=2)).isoformat()
        self.conn.execute(
            """
            INSERT INTO streamer_plans (
                twitch_user_id,
                twitch_login,
                manual_plan_id,
                manual_plan_expires_at,
                manual_plan_updated_at
            ) VALUES (?, ?, ?, ?, ?)
            """,
            ("12345", "legacy_login", "analysis_dashboard", expired_iso, now_iso),
        )
        self.conn.execute(
            """
            INSERT INTO twitch_billing_subscriptions (
                stripe_subscription_id,
                customer_reference,
                status,
                plan_id,
                updated_at
            ) VALUES (?, ?, ?, ?, ?)
            """,
            ("sub_456", "legacy_login", "active", "bundle_analysis_raid_boost", now_iso),
        )
        self.conn.commit()

        with self._patch_billing_conn():
            current_plan = self.handler._billing_current_plan_for_request(SimpleNamespace())

        self.assertEqual(current_plan["plan_id"], "bundle_analysis_raid_boost")
        self.assertEqual(current_plan["source"], "billing_subscription")
        self.assertTrue(current_plan["manual_override"]["is_expired"])

    def test_admin_set_and_clear_manual_override_round_trips_effective_plan(self) -> None:
        now_iso = datetime.now(UTC).isoformat()
        future_override_date = (datetime.now(UTC) + timedelta(days=30)).date().isoformat()
        self.conn.execute(
            """
            INSERT INTO twitch_billing_subscriptions (
                stripe_subscription_id,
                customer_reference,
                status,
                plan_id,
                updated_at
            ) VALUES (?, ?, ?, ?, ?)
            """,
            ("sub_789", "legacy_login", "active", "raid_boost", now_iso),
        )
        self.conn.commit()

        with self._patch_billing_conn():
            saved_row = self.handler._billing_admin_set_manual_plan(
                twitch_login="legacy_login",
                plan_id="bundle_analysis_raid_boost",
                expires_at=future_override_date,
                notes="VIP grant",
            )

        db_row = self.conn.execute(
            """
            SELECT manual_plan_id, manual_plan_expires_at, manual_plan_notes
            FROM streamer_plans
            WHERE twitch_user_id = ?
            """,
            ("12345",),
        ).fetchone()
        self.assertEqual(saved_row["effective_plan_id"], "bundle_analysis_raid_boost")
        self.assertEqual(saved_row["effective_plan_source"], "manual_override")
        self.assertEqual(db_row["manual_plan_id"], "bundle_analysis_raid_boost")
        self.assertEqual(db_row["manual_plan_notes"], "VIP grant")
        self.assertTrue(
            str(db_row["manual_plan_expires_at"]).startswith(f"{future_override_date}T23:59:59")
        )

        with self._patch_billing_conn():
            cleared_row = self.handler._billing_admin_clear_manual_plan(
                twitch_login="legacy_login"
            )

        cleared_db_row = self.conn.execute(
            """
            SELECT manual_plan_id, manual_plan_expires_at, manual_plan_notes
            FROM streamer_plans
            WHERE twitch_user_id = ?
            """,
            ("12345",),
        ).fetchone()
        self.assertIsNone(cleared_db_row["manual_plan_id"])
        self.assertIsNone(cleared_db_row["manual_plan_expires_at"])
        self.assertEqual(cleared_db_row["manual_plan_notes"], "")
        self.assertEqual(cleared_row["effective_plan_id"], "raid_boost")
        self.assertEqual(cleared_row["effective_plan_source"], "billing_subscription")

    def test_get_plan_for_login_uses_manual_override_before_billing(self) -> None:
        now_iso = datetime.now(UTC).isoformat()
        self.conn.execute(
            """
            INSERT INTO streamer_plans (
                twitch_user_id,
                twitch_login,
                manual_plan_id,
                manual_plan_updated_at
            ) VALUES (?, ?, ?, ?)
            """,
            ("12345", "legacy_login", "analysis_dashboard", now_iso),
        )
        self.conn.execute(
            """
            INSERT INTO twitch_billing_subscriptions (
                stripe_subscription_id,
                customer_reference,
                status,
                plan_id,
                updated_at
            ) VALUES (?, ?, ?, ?, ?)
            """,
            ("sub_321", "legacy_login", "active", "raid_boost", now_iso),
        )
        self.conn.commit()

        with self._patch_api_v2_conn():
            plan_id = _get_plan_for_login("legacy_login")

        self.assertEqual(plan_id, "analysis_dashboard")

    def test_get_plan_for_login_ignores_expired_manual_override(self) -> None:
        expired_iso = (datetime.now(UTC) - timedelta(days=1)).isoformat()
        now_iso = datetime.now(UTC).isoformat()
        self.conn.execute(
            """
            INSERT INTO streamer_plans (
                twitch_user_id,
                twitch_login,
                manual_plan_id,
                manual_plan_expires_at,
                manual_plan_updated_at
            ) VALUES (?, ?, ?, ?, ?)
            """,
            ("12345", "legacy_login", "analysis_dashboard", expired_iso, now_iso),
        )
        self.conn.execute(
            """
            INSERT INTO twitch_billing_subscriptions (
                stripe_subscription_id,
                customer_reference,
                status,
                plan_id,
                updated_at
            ) VALUES (?, ?, ?, ?, ?)
            """,
            ("sub_654", "legacy_login", "active", "bundle_analysis_raid_boost", now_iso),
        )
        self.conn.commit()

        with self._patch_api_v2_conn():
            plan_id = _get_plan_for_login("legacy_login")

        self.assertEqual(plan_id, "bundle_analysis_raid_boost")

    def test_get_plan_details_include_entitlements_and_source(self) -> None:
        now_iso = datetime.now(UTC).isoformat()
        self.conn.execute(
            """
            INSERT INTO twitch_billing_subscriptions (
                stripe_subscription_id,
                customer_reference,
                status,
                plan_id,
                updated_at
            ) VALUES (?, ?, ?, ?, ?)
            """,
            ("sub_777", "legacy_login", "active", "bundle_analysis_raid_boost", now_iso),
        )
        self.conn.commit()

        with self._patch_api_v2_conn():
            details = _get_plan_details_for_login("legacy_login")

        self.assertEqual(details["planId"], "bundle_analysis_raid_boost")
        self.assertEqual(details["tier"], "extended")
        self.assertTrue(details["isExtended"])
        self.assertEqual(
            details["entitlements"],
            [
                "analytics.ai_full",
                "analytics.basic",
                "analytics.extended",
                "chat.lurker_tax",
                "chat.promos.disable",
                "raid.priority",
            ],
        )
        self.assertEqual(details["source"], "billing_subscription")


if __name__ == "__main__":
    unittest.main()
