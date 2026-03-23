import sqlite3
import unittest

from bot.entitlements.resolver import _is_missing_current_period_end_error, _load_billing_subscription


class _FakeConnUnexpectedError:
    def execute(self, _sql, _params=None):
        raise sqlite3.OperationalError("database is locked")


class _ConnMissingCurrentPeriodEnd:
    def __init__(self) -> None:
        self.calls = 0

    def execute(self, sql, _params=None):
        self.calls += 1
        if "current_period_end" in sql:
            raise sqlite3.OperationalError("no such column: current_period_end")
        return _FakeCursor([("partner_one", "raid_boost", "active", "2026-03-23T10:00:00+00:00")])


class _FakeCursor:
    def __init__(self, rows):
        self._rows = list(rows)

    def fetchone(self):
        return self._rows[0] if self._rows else None


class EntitlementResolverTests(unittest.TestCase):
    def test_missing_current_period_end_detection_is_specific(self) -> None:
        self.assertTrue(
            _is_missing_current_period_end_error(
                sqlite3.OperationalError("no such column: current_period_end")
            )
        )
        self.assertFalse(
            _is_missing_current_period_end_error(sqlite3.OperationalError("database is locked"))
        )

    def test_load_billing_subscription_falls_back_for_legacy_schema_only(self) -> None:
        conn = _ConnMissingCurrentPeriodEnd()

        payload = _load_billing_subscription(conn, ["partner_one"])

        self.assertIsNotNone(payload)
        assert payload is not None
        self.assertEqual(payload["plan_id"], "raid_boost")
        self.assertEqual(payload["updated_at"], "2026-03-23T10:00:00+00:00")
        self.assertIsNone(payload["current_period_end"])
        self.assertEqual(conn.calls, 2)

    def test_load_billing_subscription_reraises_unexpected_sql_errors(self) -> None:
        conn = _FakeConnUnexpectedError()

        with self.assertRaises(sqlite3.OperationalError):
            _load_billing_subscription(conn, ["partner_one"])


if __name__ == "__main__":
    unittest.main()
